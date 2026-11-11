//! Text as geometry: shaped glyph outlines turned into sketch loops.
//!
//! The stack: `skrifa` for
//! outlines and variable-font instances, `harfrust` for shaping, `fontdb` for
//! system fonts natively. Shaping is not a nicety — kerning and ligatures are
//! the difference between an engraved label that looks typeset and one that
//! does not.
//!
//! ## Overlapping contours
//!
//! Glyph contours are drawn for the **nonzero** rule: outer contours wind one
//! way, counters the other, and contours are allowed to overlap. Cadpole nests
//! loops by *containment*, which agrees with nonzero only while nothing
//! overlaps — and real fonts overlap, in composite glyphs whose components
//! touch, in heavy variable-font instances, and in designs built from
//! overlapping strokes. Left alone, a `%` or a heavy `ß` comes out with a
//! hole punched where two strokes cross.
//!
//! So every glyph's contours go through an **unconditional nonzero union**
//! before anything else happens to them. It costs microseconds per glyph, and
//! afterwards containment nesting is exact. The union runs **in font units**
//! at a fixed fraction of the em, so the same string gives byte-identical
//! loops at every size and on every platform — which is what makes the
//! golden tests meaningful. The price is that glyph outlines arrive as
//! polygons rather than quadratics; at [`FLATTEN_EM_FRACTION`] the chord
//! error is 0.05 % of the em, 6 µm on a 12 mm capital.

pub mod embed;

use std::collections::BTreeMap;

use harfrust::{ShaperData, ShaperInstance, UnicodeBuffer};
use cadpole_core::fill::{self, FillRule};
use cadpole_core::sketch::{Loop, P2, Segment};
use skrifa::instance::{LocationRef, Size};
use skrifa::outline::{DrawSettings, OutlinePen};
use skrifa::{MetadataProvider, setting::VariationSetting};

/// Chord error of the glyph flattening, as a fraction of the em. Fixed rather
/// than derived from the requested size so results do not change with `size`.
pub const FLATTEN_EM_FRACTION: f64 = 1.0 / 2000.0;

/// Largest font file we will parse.
pub const MAX_FONT_BYTES: usize = 16 * 1024 * 1024;

/// The bundled sans: Inter, SIL OFL 1.1, no Reserved Font Names.
/// See `build_support/licenses/fonts.tsv` and `assets/fonts/Inter-OFL.txt`.
pub const BUILTIN_SANS: &[u8] = include_bytes!(env!("CADPOLE_BUILTIN_FONT"));

/// The name `font=` uses for the bundled face.
pub const BUILTIN_NAME: &str = "builtin";

/// A diagnostic the caller should surface.
#[derive(Clone, Debug)]
pub struct Note {
    pub warning: bool,
    pub message: String,
}

impl Note {
    fn info(m: impl Into<String>) -> Note {
        Note { warning: false, message: m.into() }
    }
    fn warn(m: impl Into<String>) -> Note {
        Note { warning: true, message: m.into() }
    }
}

/// How `text(...)` sets a string.
#[derive(Clone, Debug)]
pub struct Options {
    /// Em size in millimetres. Cap height is reported back for people who
    /// want "10 mm tall letters".
    pub size: f64,
    /// `"left"`, `"center"` or `"right"`, about x = 0.
    pub align: String,
    /// `"alphabetic"` (the baseline at y = 0), `"middle"` or `"top"`.
    pub baseline: String,
    /// Extra advance between glyphs, in millimetres.
    pub spacing: f64,
    /// Line advance as a multiple of the em size.
    pub line_height: f64,
    /// `wght` axis value for a variable font, or the nearest static style.
    pub weight: f64,
    /// Any other variable axes, by four-character tag.
    pub axes: BTreeMap<String, f64>,
    /// OpenType features in HarfBuzz syntax (`"-liga"`, `"+smcp"`, `"tnum"`).
    pub features: Vec<String>,
    /// Union touching glyphs so `extrude` sees one region (script faces).
    pub merge: bool,
}

impl Default for Options {
    fn default() -> Self {
        Options {
            size: 10.0,
            align: "left".into(),
            baseline: "alphabetic".into(),
            spacing: 0.0,
            line_height: 1.2,
            weight: 400.0,
            axes: BTreeMap::new(),
            features: Vec::new(),
            merge: true,
        }
    }
}

/// Shaped and outlined text.
#[derive(Clone, Debug, Default)]
pub struct Text {
    /// Loops named `glyph[<n>]/loop[<k>]`, or `merged/loop[<k>]` when
    /// touching glyphs were unioned.
    pub loops: Vec<(String, Loop)>,
    pub notes: Vec<Note>,
    /// Advance width of the widest line, in millimetres.
    pub width: f64,
    /// Cap height of the face at this size, in millimetres.
    pub cap_height: f64,
    /// Number of glyphs the string shaped to, missing ones included.
    pub glyphs: usize,
}

/// Where a face's bytes come from.
pub enum Source<'a> {
    /// The font bundled with Cadpole.
    Builtin,
    /// A font asset, pinned by hash like every other asset.
    Bytes(&'a [u8]),
    /// A family installed on the machine. Native only, and it warns: a model
    /// that depends on the local font set is not the same model elsewhere.
    System(&'a str),
}

/// Resolve a [`Source`] to font bytes plus any diagnostic that goes with it.
pub fn load(source: Source<'_>) -> Result<(Vec<u8>, Vec<Note>), String> {
    match source {
        Source::Builtin => Ok((BUILTIN_SANS.to_vec(), Vec::new())),
        Source::Bytes(b) => {
            if b.len() > MAX_FONT_BYTES {
                return Err(format!(
                    "font is {} bytes, over the {} MB limit",
                    b.len(),
                    MAX_FONT_BYTES / (1024 * 1024)
                ));
            }
            if b.starts_with(b"wOFF") || b.starts_with(b"wOF2") {
                return Err("WOFF fonts are not supported; add the TTF or OTF instead".into());
            }
            Ok((b.to_vec(), Vec::new()))
        }
        Source::System(family) => system_font(family),
    }
}

#[cfg(not(target_arch = "wasm32"))]
fn system_font(family: &str) -> Result<(Vec<u8>, Vec<Note>), String> {
    let mut db = fontdb::Database::new();
    db.load_system_fonts();
    let query = fontdb::Query {
        families: &[fontdb::Family::Name(family)],
        ..Default::default()
    };
    let Some(id) = db.query(&query) else {
        let mut names: Vec<String> = db.faces().map(|f| f.families[0].0.clone()).collect();
        names.sort();
        names.dedup();
        names.truncate(24);
        return Err(format!(
            "no system font family {family:?} (some that are installed: {})",
            names.join(", ")
        ));
    };
    let bytes = db
        .with_face_data(id, |data, _index| data.to_vec())
        .ok_or_else(|| format!("system font {family:?} could not be read"))?;
    Ok((
        bytes,
        vec![Note::warn(format!(
            "font {family:?} was resolved from this machine's font set, so the model is not \
             reproducible elsewhere — add the font file as an asset and use font=asset(...) to pin it"
        ))],
    ))
}

#[cfg(target_arch = "wasm32")]
fn system_font(family: &str) -> Result<(Vec<u8>, Vec<Note>), String> {
    Err(format!(
        "system fonts are not available in the browser, so font=\"system:{family}\" cannot be \
         resolved — add the font file as an asset and use font=asset(...) to pin it"
    ))
}

/// Collects one glyph's contours from skrifa, in font units.
#[derive(Default)]
struct ContourPen {
    contours: Vec<Vec<Segment>>,
    current: Vec<Segment>,
    start: P2,
    cur: P2,
}

impl ContourPen {
    fn finish_contour(&mut self) {
        if self.current.is_empty() {
            return;
        }
        let end = self.current.last().unwrap().end();
        if (end.x - self.start.x).abs() > 1e-9 || (end.y - self.start.y).abs() > 1e-9 {
            self.current.push(Segment::Line { a: end, b: self.start });
        }
        self.contours.push(std::mem::take(&mut self.current));
    }
}

impl OutlinePen for ContourPen {
    fn move_to(&mut self, x: f32, y: f32) {
        self.finish_contour();
        self.start = P2::new(x as f64, y as f64);
        self.cur = self.start;
    }
    fn line_to(&mut self, x: f32, y: f32) {
        let b = P2::new(x as f64, y as f64);
        self.current.push(Segment::Line { a: self.cur, b });
        self.cur = b;
    }
    fn quad_to(&mut self, cx: f32, cy: f32, x: f32, y: f32) {
        let b = P2::new(x as f64, y as f64);
        self.current.push(Segment::Bezier {
            a: self.cur,
            b,
            ctrl: vec![P2::new(cx as f64, cy as f64)],
        });
        self.cur = b;
    }
    fn curve_to(&mut self, c0x: f32, c0y: f32, c1x: f32, c1y: f32, x: f32, y: f32) {
        let b = P2::new(x as f64, y as f64);
        self.current.push(Segment::Bezier {
            a: self.cur,
            b,
            ctrl: vec![P2::new(c0x as f64, c0y as f64), P2::new(c1x as f64, c1y as f64)],
        });
        self.cur = b;
    }
    fn close(&mut self) {
        self.finish_contour();
        self.cur = self.start;
    }
}

/// Shape `string` with `font_bytes` and return its outlines as sketch loops,
/// in millimetres, with the baseline of the first line at y = 0.
pub fn text(font_bytes: &[u8], string: &str, opts: &Options) -> Result<Text, String> {
    if opts.size <= 0.0 {
        return Err(format!("text: size must be positive, got {}", opts.size));
    }
    let outline_font =
        skrifa::FontRef::new(font_bytes).map_err(|e| format!("font could not be parsed: {e}"))?;
    let shape_font =
        harfrust::FontRef::new(font_bytes).map_err(|e| format!("font could not be parsed: {e}"))?;

    let upem = outline_font.metrics(Size::unscaled(), LocationRef::default()).units_per_em as f64;
    if upem <= 0.0 {
        return Err("font declares no units per em".into());
    }
    let mut notes = Vec::new();

    // Variable axes: `weight` is sugar for `wght`, and `axes` sets the rest.
    let mut settings: Vec<VariationSetting> = Vec::new();
    let axes = outline_font.axes();
    let has_wght = axes.iter().any(|a| a.tag() == skrifa::Tag::new(b"wght"));
    if (opts.weight - 400.0).abs() > 0.5 {
        if has_wght {
            settings.push(("wght", opts.weight as f32).into());
        } else {
            notes.push(Note::info(format!(
                "this font has no weight axis; weight={} was ignored",
                opts.weight
            )));
        }
    }
    for (tag, v) in &opts.axes {
        settings.push((tag.as_str(), *v as f32).into());
    }
    let location = axes.location(settings.iter().copied());
    let variations: Vec<harfrust::Variation> = settings
        .iter()
        .filter_map(|s| {
            let tag = harfrust::Tag::new(&s.selector.to_be_bytes());
            Some(harfrust::Variation { tag, value: s.value })
        })
        .collect();

    let metrics = outline_font.metrics(Size::unscaled(), &location);
    let cap_height = metrics.cap_height.unwrap_or(metrics.ascent) as f64;

    let shaper_data = ShaperData::new(&shape_font);
    let instance = if variations.is_empty() {
        None
    } else {
        Some(ShaperInstance::from_variations(&shape_font, variations.iter().copied()))
    };
    let shaper = shaper_data.shaper(&shape_font).instance(instance.as_ref()).build();

    let mut features = Vec::new();
    for f in &opts.features {
        match f.parse::<harfrust::Feature>() {
            Ok(feat) => features.push(feat),
            Err(_) => notes.push(Note::warn(format!("feature {f:?} is not valid HarfBuzz syntax; ignored"))),
        }
    }

    let outlines = outline_font.outline_glyphs();
    let flatten_tol = upem * FLATTEN_EM_FRACTION;
    let mm_per_unit = opts.size / upem;
    let line_step = opts.line_height * opts.size;
    // Letter spacing is quoted in millimetres; the pen runs in font units.
    let spacing_units = opts.spacing / mm_per_unit;

    let mut glyph_index = 0usize;
    let mut missing: Vec<String> = Vec::new();
    let mut lines: Vec<(f64, Vec<(String, Loop)>)> = Vec::new();
    let mut max_width: f64 = 0.0;

    for (line_no, line) in string.split('\n').enumerate() {
        // A CRLF line break is one line break: the `\r` is not a character
        // to set (it has no glyph, and its .notdef advance would widen the
        // line and shift centred text). A tab sets as a space. Both keep
        // their one byte, so the shaper's byte clusters still index `line`.
        let line = line.strip_suffix('\r').unwrap_or(line);
        let line = line.replace('\t', " ");
        let line = line.as_str();
        let mut buffer = UnicodeBuffer::new();
        buffer.push_str(line);
        buffer.guess_segment_properties();
        let glyphs = shaper.shape(buffer, harfrust::ShapeOptions::new().features(&features));
        let infos = glyphs.glyph_infos();
        let positions = glyphs.glyph_positions();

        let mut pen_x = 0.0f64;
        let mut line_loops: Vec<(String, Loop)> = Vec::new();
        for (info, pos) in infos.iter().zip(positions.iter()) {
            let gid = skrifa::GlyphId::new(info.glyph_id);
            let ox = pen_x + pos.x_offset as f64;
            let oy = pos.y_offset as f64;
            if info.glyph_id == 0 {
                // .notdef: the character has no glyph in this face. The
                // cluster is a UTF-8 byte offset into the line, not a
                // character index.
                let ch = line.get(info.cluster as usize..).and_then(|r| r.chars().next()).unwrap_or('?');
                missing.push(ch.to_string());
            } else if let Some(glyph) = outlines.get(gid) {
                let mut pen = ContourPen::default();
                let settings = DrawSettings::unhinted(Size::unscaled(), &location);
                if glyph.draw(settings, &mut pen).is_ok() {
                    pen.finish_contour();
                    let contours: Vec<Loop> =
                        pen.contours.into_iter().filter(|c| c.len() >= 2).map(|segments| Loop { segments }).collect();
                    if !contours.is_empty() {
                        // The unconditional nonzero union, in font units.
                        let merged = fill::rebuild(&contours, FillRule::NonZero, flatten_tol);
                        for (k, l) in merged.into_iter().enumerate() {
                            let mut l = l;
                            for s in &mut l.segments {
                                map_segment(s, |p| {
                                    P2::new(
                                        (p.x + ox) * mm_per_unit,
                                        (p.y + oy) * mm_per_unit - line_no as f64 * line_step,
                                    )
                                });
                            }
                            line_loops.push((format!("glyph[{glyph_index}]/loop[{k}]"), l));
                        }
                    }
                }
            }
            glyph_index += 1;
            pen_x += pos.x_advance as f64 + spacing_units;
        }
        // The trailing letter-spacing is not part of the line's width.
        let width = (pen_x - if infos.is_empty() { 0.0 } else { spacing_units }).max(0.0) * mm_per_unit;
        max_width = max_width.max(width);
        lines.push((width, line_loops));
    }

    // Alignment, per line, then the baseline shift for the whole block.
    let line_count = lines.len() as f64;
    let baseline_shift = match opts.baseline.as_str() {
        // The block runs from the cap top of the first line down to the last
        // baseline; "middle" centres that span on y = 0.
        "middle" => (-cap_height * mm_per_unit + (line_count - 1.0) * line_step) / 2.0,
        "top" => -cap_height * mm_per_unit,
        _ => 0.0,
    };
    let mut loops = Vec::new();
    for (width, line_loops) in lines {
        let dx = match opts.align.as_str() {
            "center" => -width / 2.0,
            "right" => -width,
            _ => 0.0,
        };
        for (name, mut l) in line_loops {
            if dx != 0.0 || baseline_shift != 0.0 {
                for s in &mut l.segments {
                    map_segment(s, |p| P2::new(p.x + dx, p.y + baseline_shift));
                }
            }
            loops.push((name, l));
        }
    }

    if !matches!(opts.align.as_str(), "left" | "center" | "right") {
        notes.push(Note::warn(format!(
            "align {:?} is not one of \"left\", \"center\", \"right\"; used \"left\"",
            opts.align
        )));
    }
    if !matches!(opts.baseline.as_str(), "alphabetic" | "middle" | "top") {
        notes.push(Note::warn(format!(
            "baseline {:?} is not one of \"alphabetic\", \"middle\", \"top\"; used \"alphabetic\"",
            opts.baseline
        )));
    }
    if !missing.is_empty() {
        missing.sort();
        missing.dedup();
        notes.push(Note::warn(format!(
            "{} {} no glyph in this font: {}",
            missing.len(),
            if missing.len() == 1 { "character has" } else { "characters have" },
            missing.iter().map(|m| format!("{m:?}")).collect::<Vec<_>>().join(", ")
        )));
    }

    // Script faces and tight spacing make adjacent glyphs touch; a union
    // there keeps `extrude` from seeing two bodies fighting over one area.
    if opts.merge && loops.len() > 1 {
        let only: Vec<Loop> = loops.iter().map(|(_, l)| l.clone()).collect();
        let tol = flatten_tol * mm_per_unit;
        if fill::loops_cross(&only, tol) {
            let merged = fill::rebuild(&only, FillRule::NonZero, tol);
            notes.push(Note::info("glyphs overlap; they were unioned into one region".to_string()));
            loops = merged.into_iter().enumerate().map(|(i, l)| (format!("merged/loop[{i}]"), l)).collect();
        }
    }

    Ok(Text {
        loops,
        notes,
        width: max_width,
        cap_height: cap_height * mm_per_unit,
        glyphs: glyph_index,
    })
}

fn map_segment(s: &mut Segment, f: impl Fn(P2) -> P2) {
    match s {
        Segment::Line { a, b } => {
            *a = f(*a);
            *b = f(*b);
        }
        Segment::Arc { a, b, via } => {
            *a = f(*a);
            *b = f(*b);
            *via = f(*via);
        }
        Segment::Bezier { a, b, ctrl } => {
            *a = f(*a);
            *b = f(*b);
            for c in ctrl.iter_mut() {
                *c = f(*c);
            }
        }
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use cadpole_core::{Plane, Sketch};

    fn bbox(t: &Text) -> (P2, P2) {
        let mut lo = P2::new(f64::MAX, f64::MAX);
        let mut hi = P2::new(f64::MIN, f64::MIN);
        for (_, l) in &t.loops {
            for p in l.sample_tolerance(0.01) {
                lo = P2::new(lo.x.min(p.x), lo.y.min(p.y));
                hi = P2::new(hi.x.max(p.x), hi.y.max(p.y));
            }
        }
        (lo, hi)
    }

    #[test]
    fn the_bundled_font_is_a_font_and_has_a_weight_axis() {
        let f = skrifa::FontRef::new(BUILTIN_SANS).expect("Inter parses");
        let m = f.metrics(Size::unscaled(), LocationRef::default());
        assert_eq!(m.units_per_em, 2048);
        assert!(f.axes().iter().any(|a| a.tag() == skrifa::Tag::new(b"wght")), "Inter is variable");
        assert!(f.charmap().map('K').is_some());
    }

    #[test]
    fn a_known_string_has_the_glyph_loops_and_the_bbox_we_expect() {
        let t = text(BUILTIN_SANS, "Kern", &Options { size: 10.0, ..Default::default() }).unwrap();
        assert_eq!(t.glyphs, 4, "four glyphs, no ligature in \"Kern\"");
        // K, r and f have one contour each; e has an outer contour and a
        // counter. No contour of these four overlaps, so the nonzero union
        // changes the loop count for none of them.
        assert_eq!(t.loops.len(), 5, "{:?}", t.loops.iter().map(|(n, _)| n).collect::<Vec<_>>());
        assert_eq!(t.loops[0].0, "glyph[0]/loop[0]");
        let (lo, hi) = bbox(&t);
        // Baseline at y = 0: nothing in "Kern" descends, and the only ink
        // below the line is the round overshoot of the 'e' (~24 font units).
        assert!(lo.y < 0.0 && lo.y > -0.2, "baseline is y = 0, got {}", lo.y);
        // Inter's cap height is 1490/2048 em; "K" and "f" reach about that.
        assert!((t.cap_height - 10.0 * 1490.0 / 2048.0).abs() < 0.01, "cap height {}", t.cap_height);
        assert!(hi.y > t.cap_height * 0.98 && hi.y < 10.0, "top {}", hi.y);
        assert!(lo.x >= -0.2 && lo.x < 1.0, "left edge {}", lo.x);
        // Four letters of a 10 mm em are about two ems wide.
        assert!(t.width > 15.0 && t.width < 28.0, "width {}", t.width);
        assert!((hi.x - t.width).abs() < 2.0, "ink ends near the advance: {} vs {}", hi.x, t.width);
        assert!(t.notes.iter().all(|n| !n.warning), "{:?}", t.notes);
    }

    #[test]
    fn the_e_counter_is_a_hole_once_the_loops_are_nested() {
        let t = text(BUILTIN_SANS, "e", &Options { size: 20.0, ..Default::default() }).unwrap();
        let s = t
            .loops
            .iter()
            .fold(Sketch::new(Plane::XY), |s, (n, l)| s.with_named_loop(l.clone(), n.clone()));
        let regions = s.regions().unwrap();
        assert_eq!(regions.len(), 1);
        assert_eq!(regions[0].holes.len(), 1, "the counter of 'e' is a hole");
        assert!(regions[0].outer.signed_area() > 0.0);
        assert!(regions[0].holes[0].signed_area() < 0.0);
    }

    #[test]
    fn overlapping_contours_do_not_punch_a_hole() {
        // A glyph whose two contours are wound the same way and overlap: with
        // containment nesting alone the inner region would become a hole.
        // After the nonzero union it is one loop with the right area.
        let a = Loop::rect(P2::new(0.0, 0.0), 100.0, 40.0);
        let b = Loop::rect(P2::new(30.0, 0.0), 100.0, 40.0);
        let merged = fill::rebuild(&[a, b], FillRule::NonZero, 0.5);
        assert_eq!(merged.len(), 1);
        let s = Sketch::new(Plane::XY).with_loop(merged[0].clone());
        let regions = s.regions().unwrap();
        assert_eq!(regions.len(), 1);
        assert!(regions[0].holes.is_empty(), "no hole where the strokes cross");
        assert!((regions[0].outer.signed_area() - 130.0 * 40.0).abs() < 1.0);
    }

    #[test]
    fn size_scales_linearly_and_the_shape_is_size_independent() {
        let a = text(BUILTIN_SANS, "Kern", &Options { size: 10.0, ..Default::default() }).unwrap();
        let b = text(BUILTIN_SANS, "Kern", &Options { size: 30.0, ..Default::default() }).unwrap();
        assert_eq!(a.loops.len(), b.loops.len(), "the same loops at any size");
        for ((_, la), (_, lb)) in a.loops.iter().zip(b.loops.iter()) {
            assert_eq!(la.segments.len(), lb.segments.len(), "identical flattening at any size");
        }
        assert!((b.width / a.width - 3.0).abs() < 1e-9, "{} vs {}", a.width, b.width);
    }

    #[test]
    fn alignment_and_spacing_move_the_ink_the_way_they_say() {
        let left = text(BUILTIN_SANS, "Kern", &Options { size: 10.0, ..Default::default() }).unwrap();
        let centred = text(
            BUILTIN_SANS,
            "Kern",
            &Options { size: 10.0, align: "center".into(), ..Default::default() },
        )
        .unwrap();
        let (llo, lhi) = bbox(&left);
        let (clo, chi) = bbox(&centred);
        assert!(((clo.x + chi.x) / 2.0).abs() < 1.0, "centred text straddles x = 0: {clo:?} {chi:?}");
        assert!((llo.x + lhi.x) / 2.0 > 1.0, "left-aligned text does not");
        let spaced = text(
            BUILTIN_SANS,
            "Kern",
            &Options { size: 10.0, spacing: 2.0, ..Default::default() },
        )
        .unwrap();
        assert!((spaced.width - left.width - 6.0).abs() < 1e-6, "three gaps of 2 mm: {} vs {}", spaced.width, left.width);
    }

    #[test]
    fn a_heavier_weight_makes_more_ink() {
        let light = text(BUILTIN_SANS, "KERN", &Options { size: 10.0, ..Default::default() }).unwrap();
        let heavy =
            text(BUILTIN_SANS, "KERN", &Options { size: 10.0, weight: 900.0, ..Default::default() }).unwrap();
        let area = |t: &Text| -> f64 { t.loops.iter().map(|(_, l)| l.signed_area().abs()).sum() };
        assert!(area(&heavy) > area(&light) * 1.2, "{} vs {}", area(&heavy), area(&light));
    }

    #[test]
    fn multiline_text_stacks_downwards() {
        let t = text(BUILTIN_SANS, "A\nB", &Options { size: 10.0, ..Default::default() }).unwrap();
        let (lo, hi) = bbox(&t);
        // Two lines, 1.2 em apart: the block is taller than one cap height.
        assert!(hi.y - lo.y > 12.0 && hi.y - lo.y < 22.0, "{lo:?} {hi:?}");
        assert!(lo.y < -10.0, "the second line is below the first baseline: {lo:?}");
    }

    #[test]
    fn missing_glyphs_are_reported_not_drawn() {
        let t = text(BUILTIN_SANS, "A\u{10FFFD}", &Options { size: 10.0, ..Default::default() }).unwrap();
        assert!(t.notes.iter().any(|n| n.warning && n.message.contains("no glyph")), "{:?}", t.notes);
    }

    fn missing_note(t: &Text) -> String {
        t.notes
            .iter()
            .find(|n| n.warning && n.message.contains("no glyph"))
            .map(|n| n.message.clone())
            .unwrap_or_default()
    }

    #[test]
    fn the_missing_glyph_warning_names_the_character_that_is_missing() {
        // The shaper's cluster is a byte offset into the line; read as a
        // character index it lands on the wrong character after any
        // non-ASCII text.
        let t = text(BUILTIN_SANS, "\u{e9}\u{e9}\u{10FFFD}abc", &Options::default()).unwrap();
        // The warning quotes each character as Rust's Debug does.
        let quoted = |c: char| format!("{:?}", c.to_string());
        let m = missing_note(&t);
        assert!(m.contains(&quoted('\u{10FFFD}')) && m.starts_with("1 character has"), "{m}");
        let t = text(BUILTIN_SANS, "\u{e9}\u{10FFFD}\u{10FFFC}", &Options::default()).unwrap();
        let m = missing_note(&t);
        assert!(
            m.starts_with("2 characters have") && m.contains(&quoted('\u{10FFFD}')) && m.contains(&quoted('\u{10FFFC}')),
            "{m}"
        );
    }

    #[test]
    fn a_crlf_line_break_is_a_line_break() {
        let lf = text(BUILTIN_SANS, "HHH\nH", &Options { align: "center".into(), ..Default::default() }).unwrap();
        let crlf =
            text(BUILTIN_SANS, "HHH\r\nH", &Options { align: "center".into(), ..Default::default() }).unwrap();
        assert!(crlf.notes.iter().all(|n| !n.warning), "{:?}", crlf.notes);
        assert!((crlf.width - lf.width).abs() < 1e-9, "{} vs {}", crlf.width, lf.width);
        assert_eq!(bbox(&crlf), bbox(&lf));
        // A tab sets as a space: whitespace, not a missing character.
        let tab = text(BUILTIN_SANS, "H\tH", &Options::default()).unwrap();
        let space = text(BUILTIN_SANS, "H H", &Options::default()).unwrap();
        assert!(tab.notes.iter().all(|n| !n.warning), "{:?}", tab.notes);
        assert!((tab.width - space.width).abs() < 1e-9, "{} vs {}", tab.width, space.width);
    }

    #[test]
    fn a_bad_size_or_a_non_font_is_an_error_not_a_panic() {
        assert!(text(BUILTIN_SANS, "x", &Options { size: 0.0, ..Default::default() }).is_err());
        assert!(text(b"not a font at all", "x", &Options::default()).is_err());
        assert!(matches!(load(Source::Bytes(b"wOFF....")), Err(e) if e.contains("WOFF")));
    }
}
