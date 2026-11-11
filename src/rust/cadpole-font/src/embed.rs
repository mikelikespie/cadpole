//! Fonts a PDF can embed: a string shaped to glyph ids, and a small
//! static TrueType font of exactly the glyphs a document used, instanced at
//! one weight.
//!
//! A PDF's text is only *text* — selectable, searchable, read back by
//! `pdftotext` — when the file says which glyph each code is and what
//! character it stands for. For the standard faces the reader brings the
//! glyphs; for the bundled face (Inter) the file has to carry them.
//! So the writer shapes each string here, collects the glyph ids, and asks
//! [`instance_subset`] for a font program holding those glyphs and nothing
//! else.
//!
//! The font program is written from scratch rather than cut out of the
//! original: Inter is a *variable* font, and a PDF has no way to select a
//! weight, so a subset of the original's `glyf` table would always show the
//! default instance and a bold title would come out regular. Instead every
//! glyph is drawn through skrifa at the requested location — variations
//! applied, composites flattened — and written back as a simple TrueType
//! glyph, with its advance from the same location. What comes out is a
//! static, unhinted TrueType with nine tables, a few kilobytes for a sheet's
//! worth of glyphs, and byte-identical for the same input on every platform.
//! A face with cubic outlines (CFF) is written too: each cubic becomes four
//! quadratics, well under a font unit off on any real glyph.
//!
//! Glyphs are renumbered densely in the order given: new glyph 0 is
//! `.notdef` (empty), glyph `i` is `gids[i - 1]`. A PDF that maps codes to
//! glyphs one-to-one (`/CIDToGIDMap /Identity`) then uses the new numbers as
//! its codes.

use harfrust::{ShaperData, ShaperInstance, UnicodeBuffer};
use skrifa::instance::{Location, LocationRef, Size};
use skrifa::outline::{DrawSettings, OutlinePen};
use skrifa::{setting::VariationSetting, GlyphId, MetadataProvider};

/// One glyph of a shaped line, in font units.
#[derive(Clone, Debug, PartialEq)]
pub struct ShapedGlyph {
    /// The glyph id in the original font; 0 is `.notdef`, a character the
    /// face has no glyph for.
    pub gid: u32,
    /// Byte offset of the first character this glyph stands for.
    pub cluster: usize,
    /// How far the pen moves after it: the glyph's advance plus any kerning.
    pub advance: f64,
    pub x_offset: f64,
    pub y_offset: f64,
}

/// A line of text shaped at one weight.
#[derive(Clone, Debug, Default)]
pub struct ShapedLine {
    pub glyphs: Vec<ShapedGlyph>,
    pub units_per_em: f64,
}

impl ShapedLine {
    /// The text each glyph stands for: its cluster's characters, the first
    /// glyph of a cluster carrying them all and the rest none. What a PDF's
    /// `ToUnicode` map says, so a ligature reads back as its two letters.
    pub fn glyph_texts(&self, line: &str) -> Vec<String> {
        let mut starts: Vec<usize> = self.glyphs.iter().map(|g| g.cluster.min(line.len())).collect();
        starts.sort_unstable();
        starts.dedup();
        let mut seen = std::collections::BTreeSet::new();
        self.glyphs
            .iter()
            .map(|g| {
                let start = g.cluster.min(line.len());
                if !seen.insert(start) {
                    return String::new();
                }
                let end = starts.iter().copied().find(|s| *s > start).unwrap_or(line.len());
                line.get(start..end).unwrap_or("").to_owned()
            })
            .collect()
    }
}

/// The variation location for `weight` (the `wght` axis), or the default
/// instance when the face has no such axis or the weight is 400.
fn location_for(font: &skrifa::FontRef<'_>, weight: f64) -> (Location, Vec<harfrust::Variation>) {
    let axes = font.axes();
    let has_wght = axes.iter().any(|a| a.tag() == skrifa::Tag::new(b"wght"));
    if !has_wght || (weight - 400.0).abs() < 0.5 {
        return (Location::default(), Vec::new());
    }
    let settings: Vec<VariationSetting> = vec![("wght", weight as f32).into()];
    let location = axes.location(settings.iter().copied());
    let variations = settings
        .iter()
        .map(|s| harfrust::Variation { tag: harfrust::Tag::new(&s.selector.to_be_bytes()), value: s.value })
        .collect();
    (location, variations)
}

/// Shape one line of `text` (no newlines) at `weight`, with the face's
/// default features: kerning, ligatures, contextual alternates.
pub fn shape_line(font_bytes: &[u8], text: &str, weight: f64) -> Result<ShapedLine, String> {
    let outline_font = skrifa::FontRef::new(font_bytes).map_err(|e| format!("font could not be parsed: {e}"))?;
    let shape_font = harfrust::FontRef::new(font_bytes).map_err(|e| format!("font could not be parsed: {e}"))?;
    let upem = outline_font.metrics(Size::unscaled(), LocationRef::default()).units_per_em as f64;
    if upem <= 0.0 {
        return Err("font declares no units per em".into());
    }
    let (_, variations) = location_for(&outline_font, weight);
    let data = ShaperData::new(&shape_font);
    let instance = if variations.is_empty() {
        None
    } else {
        Some(ShaperInstance::from_variations(&shape_font, variations.iter().copied()))
    };
    let shaper = data.shaper(&shape_font).instance(instance.as_ref()).build();
    let mut buffer = UnicodeBuffer::new();
    buffer.push_str(text);
    buffer.guess_segment_properties();
    let out = shaper.shape(buffer, harfrust::ShapeOptions::new());
    let glyphs = out
        .glyph_infos()
        .iter()
        .zip(out.glyph_positions())
        .map(|(info, pos)| ShapedGlyph {
            gid: info.glyph_id,
            cluster: info.cluster as usize,
            advance: pos.x_advance as f64,
            x_offset: pos.x_offset as f64,
            y_offset: pos.y_offset as f64,
        })
        .collect();
    Ok(ShapedLine { glyphs, units_per_em: upem })
}

/// A font program for a PDF, and what the PDF says about it.
#[derive(Clone, Debug)]
pub struct EmbeddedFont {
    /// A complete TrueType file (`FontFile2`).
    pub ttf: Vec<u8>,
    pub units_per_em: u16,
    /// Advance of each new glyph id, in font units, `.notdef` first.
    pub advances: Vec<u16>,
    pub ascent: i16,
    pub descent: i16,
    pub cap_height: i16,
    /// xMin, yMin, xMax, yMax over every glyph, font units.
    pub bbox: [i16; 4],
    /// The face's PostScript-style family name, letters and digits only
    /// (`Inter`), with the weight (`Inter-Bold`).
    pub ps_name: String,
    pub weight: u16,
}

/// The advance of each glyph at `weight`, in whole font units: what
/// [`instance_subset`] writes into the font's `hmtx` for it, so a PDF can
/// position text against the widths the font will carry before the font is
/// written.
pub fn advances(font_bytes: &[u8], weight: f64, gids: &[u32]) -> Result<Vec<u16>, String> {
    let font = skrifa::FontRef::new(font_bytes).map_err(|e| format!("font could not be parsed: {e}"))?;
    let (location, _) = location_for(&font, weight);
    let m = font.glyph_metrics(Size::unscaled(), LocationRef::from(&location));
    Ok(gids
        .iter()
        .map(|g| m.advance_width(GlyphId::new(*g)).unwrap_or(0.0).round().clamp(0.0, u16::MAX as f32) as u16)
        .collect())
}

/// The glyphs `gids` (original ids, in the order their new ids are wanted)
/// drawn at `weight` and written as a static TrueType font. See the module
/// documentation for the numbering. A `None` is an empty glyph with no
/// advance: a slot another weight's font fills, so that two fonts can share
/// one code space (and one `ToUnicode` map).
pub fn instance_subset(font_bytes: &[u8], weight: f64, gids: &[Option<u32>]) -> Result<EmbeddedFont, String> {
    let font = skrifa::FontRef::new(font_bytes).map_err(|e| format!("font could not be parsed: {e}"))?;
    let (location, _) = location_for(&font, weight);
    let loc = LocationRef::from(&location);
    let metrics = font.metrics(Size::unscaled(), loc);
    let upem = metrics.units_per_em;
    if upem == 0 {
        return Err("font declares no units per em".into());
    }
    let outlines = font.outline_glyphs();
    let glyph_metrics = font.glyph_metrics(Size::unscaled(), loc);

    // Every glyph: its encoded `glyf` entry, advance and bounds.
    let mut glyf: Vec<u8> = Vec::new();
    let mut loca: Vec<u32> = vec![0];
    let mut hmtx: Vec<u8> = Vec::new();
    let mut advances: Vec<u16> = Vec::new();
    let mut bbox = [i16::MAX, i16::MAX, i16::MIN, i16::MIN];
    let (mut max_points, mut max_contours) = (0u16, 0u16);
    let ids: Vec<Option<u32>> = std::iter::once(None).chain(gids.iter().copied()).collect();
    for id in &ids {
        let mut pen = QuadPen::default();
        let mut advance = 0.0f32;
        if let Some(gid) = id {
            let g = GlyphId::new(*gid);
            if let Some(o) = outlines.get(g) {
                o.draw(DrawSettings::unhinted(Size::unscaled(), loc), &mut pen)
                    .map_err(|e| format!("glyph {gid}: {e:?}"))?;
                pen.close();
            }
            advance = glyph_metrics.advance_width(g).unwrap_or(0.0);
        }
        let encoded = encode_glyph(&pen.contours);
        let adv = advance.round().clamp(0.0, u16::MAX as f32) as u16;
        let lsb = encoded.as_ref().map(|e| e.bbox[0]).unwrap_or(0);
        if let Some(e) = &encoded {
            bbox[0] = bbox[0].min(e.bbox[0]);
            bbox[1] = bbox[1].min(e.bbox[1]);
            bbox[2] = bbox[2].max(e.bbox[2]);
            bbox[3] = bbox[3].max(e.bbox[3]);
            max_points = max_points.max(e.points);
            max_contours = max_contours.max(e.contours);
            glyf.extend_from_slice(&e.bytes);
            while glyf.len() % 4 != 0 {
                glyf.push(0);
            }
        }
        loca.push(glyf.len() as u32);
        hmtx.extend_from_slice(&adv.to_be_bytes());
        hmtx.extend_from_slice(&lsb.to_be_bytes());
        advances.push(adv);
    }
    if bbox[0] > bbox[2] {
        bbox = [0, 0, 0, 0];
    }
    let n = ids.len() as u16;
    let ascent = metrics.ascent.round() as i16;
    let descent = metrics.descent.round() as i16;
    let cap_height = metrics.cap_height.unwrap_or(metrics.ascent * 0.7).round() as i16;
    let weight_class = weight.round().clamp(1.0, 1000.0) as u16;

    // head: the original's, its bounds, style and location format replaced.
    let mut head = table(font_bytes, b"head").map(|t| t.to_vec()).unwrap_or_default();
    if head.len() < 54 {
        head = vec![0; 54];
        head[0..4].copy_from_slice(&0x0001_0000u32.to_be_bytes());
        head[12..16].copy_from_slice(&0x5F0F_3CF5u32.to_be_bytes());
        head[18..20].copy_from_slice(&upem.to_be_bytes());
    }
    head.truncate(54);
    head[8..12].copy_from_slice(&[0; 4]);
    for (k, v) in bbox.iter().enumerate() {
        head[36 + 2 * k..38 + 2 * k].copy_from_slice(&v.to_be_bytes());
    }
    let mac_style: u16 = if weight_class >= 600 { 1 } else { 0 };
    head[44..46].copy_from_slice(&mac_style.to_be_bytes());
    head[50..52].copy_from_slice(&1i16.to_be_bytes());
    head[52..54].copy_from_slice(&0i16.to_be_bytes());

    // hhea: every glyph has its own metric.
    let mut hhea = vec![0u8; 36];
    hhea[0..4].copy_from_slice(&0x0001_0000u32.to_be_bytes());
    hhea[4..6].copy_from_slice(&ascent.to_be_bytes());
    hhea[6..8].copy_from_slice(&descent.to_be_bytes());
    hhea[10..12].copy_from_slice(&advances.iter().copied().max().unwrap_or(0).to_be_bytes());
    hhea[12..14].copy_from_slice(&bbox[0].to_be_bytes());
    hhea[16..18].copy_from_slice(&bbox[2].to_be_bytes());
    hhea[18..20].copy_from_slice(&1i16.to_be_bytes()); // caretSlopeRise
    hhea[34..36].copy_from_slice(&n.to_be_bytes());

    // maxp 1.0: the simple-glyph maxima measured, no composites, no hinting.
    let mut maxp = vec![0u8; 32];
    maxp[0..4].copy_from_slice(&0x0001_0000u32.to_be_bytes());
    maxp[4..6].copy_from_slice(&n.to_be_bytes());
    maxp[6..8].copy_from_slice(&max_points.to_be_bytes());
    maxp[8..10].copy_from_slice(&max_contours.to_be_bytes());
    maxp[14..16].copy_from_slice(&1u16.to_be_bytes()); // maxZones

    // OS/2: the original's, the weight class and the bold/regular bits set.
    let os2 = table(font_bytes, b"OS/2").map(|t| {
        let mut t = t.to_vec();
        if t.len() >= 64 {
            t[4..6].copy_from_slice(&weight_class.to_be_bytes());
            let mut sel = u16::from_be_bytes([t[62], t[63]]) & !(0x0020 | 0x0040);
            sel |= if weight_class >= 600 { 0x0020 } else { 0x0040 };
            t[62..64].copy_from_slice(&sel.to_be_bytes());
        }
        t
    });
    // post 3.0: no glyph names.
    let mut post = vec![0u8; 32];
    post[0..4].copy_from_slice(&0x0003_0000u32.to_be_bytes());
    if let Some(orig) = table(font_bytes, b"post").filter(|t| t.len() >= 32) {
        post[4..16].copy_from_slice(&orig[4..16]); // italic angle, underline
    }
    let loca_bytes: Vec<u8> = loca.iter().flat_map(|o| o.to_be_bytes()).collect();

    let mut tables: Vec<([u8; 4], Vec<u8>)> = Vec::new();
    if let Some(os2) = os2 {
        tables.push((*b"OS/2", os2));
    }
    tables.push((*b"glyf", glyf));
    tables.push((*b"head", head));
    tables.push((*b"hhea", hhea));
    tables.push((*b"hmtx", hmtx));
    tables.push((*b"loca", loca_bytes));
    tables.push((*b"maxp", maxp));
    if let Some(name) = table(font_bytes, b"name") {
        tables.push((*b"name", name.to_vec()));
    }
    tables.push((*b"post", post));
    let ttf = write_sfnt(tables);

    let family = family_name(&font);
    let ps_name = if weight_class >= 600 { format!("{family}-Bold") } else { format!("{family}-Regular") };
    Ok(EmbeddedFont {
        ttf,
        units_per_em: upem,
        advances,
        ascent,
        descent,
        cap_height,
        bbox,
        ps_name,
        weight: weight_class,
    })
}

/// The family name, letters and digits only, for a PostScript name.
fn family_name(font: &skrifa::FontRef<'_>) -> String {
    let raw: String = font
        .localized_strings(skrifa::string::StringId::FAMILY_NAME)
        .english_or_first()
        .map(|s| s.chars().collect())
        .unwrap_or_default();
    let clean: String = raw.chars().filter(|c| c.is_ascii_alphanumeric()).collect();
    if clean.is_empty() {
        "Font".to_owned()
    } else {
        clean
    }
}

/// The bytes of table `tag` in a single-font sfnt.
fn table<'a>(font: &'a [u8], tag: &[u8; 4]) -> Option<&'a [u8]> {
    let be16 = |o: usize| font.get(o..o + 2).map(|b| u16::from_be_bytes([b[0], b[1]]));
    let be32 = |o: usize| font.get(o..o + 4).map(|b| u32::from_be_bytes([b[0], b[1], b[2], b[3]]));
    let count = be16(4)? as usize;
    for i in 0..count {
        let rec = 12 + 16 * i;
        if font.get(rec..rec + 4)? == tag {
            let off = be32(rec + 8)? as usize;
            let len = be32(rec + 12)? as usize;
            return font.get(off..off.checked_add(len)?);
        }
    }
    None
}

fn checksum(data: &[u8]) -> u32 {
    data.chunks(4).fold(0u32, |acc, c| {
        let mut w = [0u8; 4];
        w[..c.len()].copy_from_slice(c);
        acc.wrapping_add(u32::from_be_bytes(w))
    })
}

/// An sfnt file from its tables, which must be in tag order; the `head`
/// table's checksum adjustment is filled in.
fn write_sfnt(tables: Vec<([u8; 4], Vec<u8>)>) -> Vec<u8> {
    let n = tables.len() as u16;
    let mut pow = 1u16;
    let mut log = 0u16;
    while pow * 2 <= n {
        pow *= 2;
        log += 1;
    }
    let search_range = pow * 16;
    let mut out = Vec::new();
    out.extend_from_slice(&0x0001_0000u32.to_be_bytes());
    out.extend_from_slice(&n.to_be_bytes());
    out.extend_from_slice(&search_range.to_be_bytes());
    out.extend_from_slice(&log.to_be_bytes());
    out.extend_from_slice(&(n * 16 - search_range).to_be_bytes());
    let mut offset = 12 + 16 * tables.len();
    let mut head_at = None;
    let mut body = Vec::new();
    for (tag, data) in &tables {
        out.extend_from_slice(tag);
        out.extend_from_slice(&checksum(data).to_be_bytes());
        out.extend_from_slice(&(offset as u32).to_be_bytes());
        out.extend_from_slice(&(data.len() as u32).to_be_bytes());
        if tag == b"head" {
            head_at = Some(offset);
        }
        body.extend_from_slice(data);
        while body.len() % 4 != 0 {
            body.push(0);
        }
        offset = 12 + 16 * tables.len() + body.len();
    }
    out.extend_from_slice(&body);
    if let Some(h) = head_at {
        let adj = 0xB1B0_AFBAu32.wrapping_sub(checksum(&out));
        out[h + 8..h + 12].copy_from_slice(&adj.to_be_bytes());
    }
    out
}

/// A contour as TrueType points: `(x, y, on_curve)`.
type Contour = Vec<(f32, f32, bool)>;

/// Collects quadratic contours from skrifa; a cubic is split into four
/// quadratics.
#[derive(Default)]
struct QuadPen {
    contours: Vec<Contour>,
    current: Contour,
    cur: (f32, f32),
}

impl QuadPen {
    fn flush(&mut self) {
        let mut c = std::mem::take(&mut self.current);
        // A closing point that repeats the first is implied by the format.
        if c.len() > 1 {
            let (a, b) = (c[0], c[c.len() - 1]);
            if b.2 && (a.0 - b.0).abs() < 1e-3 && (a.1 - b.1).abs() < 1e-3 {
                c.pop();
            }
        }
        if c.len() >= 2 {
            self.contours.push(c);
        }
    }
}

impl OutlinePen for QuadPen {
    fn move_to(&mut self, x: f32, y: f32) {
        self.flush();
        self.current.push((x, y, true));
        self.cur = (x, y);
    }
    fn line_to(&mut self, x: f32, y: f32) {
        self.current.push((x, y, true));
        self.cur = (x, y);
    }
    fn quad_to(&mut self, cx: f32, cy: f32, x: f32, y: f32) {
        self.current.push((cx, cy, false));
        self.current.push((x, y, true));
        self.cur = (x, y);
    }
    fn curve_to(&mut self, c0x: f32, c0y: f32, c1x: f32, c1y: f32, x: f32, y: f32) {
        // Four pieces by de Casteljau, each approximated by the quadratic
        // whose control point is the midpoint of the two the cubic implies.
        let p = [(self.cur.0, self.cur.1), (c0x, c0y), (c1x, c1y), (x, y)];
        let at = |t: f32| {
            let u = 1.0 - t;
            let b = |a: f32, b: f32, c: f32, d: f32| u * u * u * a + 3.0 * u * u * t * b + 3.0 * u * t * t * c + t * t * t * d;
            let db = |a: f32, b: f32, c: f32, d: f32| 3.0 * (u * u * (b - a) + 2.0 * u * t * (c - b) + t * t * (d - c));
            (
                (b(p[0].0, p[1].0, p[2].0, p[3].0), b(p[0].1, p[1].1, p[2].1, p[3].1)),
                (db(p[0].0, p[1].0, p[2].0, p[3].0), db(p[0].1, p[1].1, p[2].1, p[3].1)),
            )
        };
        let pieces = 4;
        for k in 0..pieces {
            let (t0, t1) = (k as f32 / pieces as f32, (k + 1) as f32 / pieces as f32);
            let ((x0, y0), (dx0, dy0)) = at(t0);
            let ((x1, y1), (dx1, dy1)) = at(t1);
            let h = (t1 - t0) / 3.0;
            // The cubic's own control points for this piece, then the
            // quadratic's as their average extrapolation.
            let (a1, b1) = (x0 + dx0 * h, y0 + dy0 * h);
            let (a2, b2) = (x1 - dx1 * h, y1 - dy1 * h);
            let cx = (3.0 * (a1 + a2) - x0 - x1) / 4.0;
            let cy = (3.0 * (b1 + b2) - y0 - y1) / 4.0;
            self.current.push((cx, cy, false));
            self.current.push((x1, y1, true));
        }
        self.cur = (x, y);
    }
    fn close(&mut self) {
        self.flush();
    }
}

struct Encoded {
    bytes: Vec<u8>,
    bbox: [i16; 4],
    points: u16,
    contours: u16,
}

/// A simple TrueType glyph from its contours: every coordinate a full
/// 16-bit delta, which is larger than the packed forms and simpler than
/// them, and deflate takes most of the difference back.
fn encode_glyph(contours: &[Contour]) -> Option<Encoded> {
    if contours.is_empty() {
        return None;
    }
    let round = |v: f32| v.round().clamp(i16::MIN as f32, i16::MAX as f32) as i16;
    let mut ends: Vec<u16> = Vec::new();
    let mut pts: Vec<(i16, i16, bool)> = Vec::new();
    for c in contours {
        for p in c {
            pts.push((round(p.0), round(p.1), p.2));
        }
        ends.push((pts.len() - 1) as u16);
    }
    let bbox = pts.iter().fold([i16::MAX, i16::MAX, i16::MIN, i16::MIN], |b, p| {
        [b[0].min(p.0), b[1].min(p.1), b[2].max(p.0), b[3].max(p.1)]
    });
    let mut out = Vec::new();
    out.extend_from_slice(&(contours.len() as i16).to_be_bytes());
    for v in bbox {
        out.extend_from_slice(&v.to_be_bytes());
    }
    for e in &ends {
        out.extend_from_slice(&e.to_be_bytes());
    }
    out.extend_from_slice(&0u16.to_be_bytes()); // no instructions
    for p in &pts {
        out.push(if p.2 { 1 } else { 0 });
    }
    let (mut x, mut y) = (0i32, 0i32);
    for p in &pts {
        let d = (p.0 as i32 - x) as i16;
        out.extend_from_slice(&d.to_be_bytes());
        x = p.0 as i32;
    }
    for p in &pts {
        let d = (p.1 as i32 - y) as i16;
        out.extend_from_slice(&d.to_be_bytes());
        y = p.1 as i32;
    }
    Some(Encoded { bytes: out, bbox, points: pts.len() as u16, contours: contours.len() as u16 })
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::BUILTIN_SANS;

    fn gids(s: &str, weight: f64) -> Vec<u32> {
        let mut v: Vec<u32> = shape_line(BUILTIN_SANS, s, weight).unwrap().glyphs.iter().map(|g| g.gid).collect();
        v.sort_unstable();
        v.dedup();
        v
    }

    #[test]
    fn a_shaped_line_has_one_glyph_a_letter_and_its_advances_sum_to_the_width() {
        let l = shape_line(BUILTIN_SANS, "Kern", 400.0).unwrap();
        assert_eq!(l.glyphs.len(), 4);
        assert_eq!(l.units_per_em, 2048.0);
        let w: f64 = l.glyphs.iter().map(|g| g.advance).sum();
        let t = crate::text(BUILTIN_SANS, "Kern", &crate::Options { size: 2048.0, ..Default::default() }).unwrap();
        assert!((w - t.width).abs() < 1e-6, "the shaper behind text() agrees: {w} vs {}", t.width);
        assert_eq!(l.glyph_texts("Kern"), vec!["K", "e", "r", "n"]);
        // Bold is wider.
        let b = shape_line(BUILTIN_SANS, "Kern", 700.0).unwrap();
        assert!(b.glyphs.iter().map(|g| g.advance).sum::<f64>() > w);
    }

    #[test]
    fn the_subset_is_a_font_skrifa_reads_with_the_glyphs_asked_for() {
        let ids = gids("Family room 297 sf", 400.0);
        let slots: Vec<Option<u32>> = ids.iter().map(|g| Some(*g)).collect();
        let f = instance_subset(BUILTIN_SANS, 400.0, &slots).unwrap();
        assert_eq!(advances(BUILTIN_SANS, 400.0, &ids).unwrap(), f.advances[1..].to_vec());
        assert_eq!(f.advances.len(), ids.len() + 1, ".notdef plus one per glyph");
        assert_eq!(f.ps_name, "Inter-Regular");
        assert!(f.ttf.len() < 12_000, "a few kilobytes: {}", f.ttf.len());
        let back = skrifa::FontRef::new(&f.ttf).expect("the subset parses");
        let m = back.metrics(Size::unscaled(), LocationRef::default());
        assert_eq!(m.units_per_em, 2048);
        assert_eq!(m.glyph_count as usize, ids.len() + 1);
        // Each glyph comes back with the original's advance and outline
        // extent, to the font unit.
        let orig = skrifa::FontRef::new(BUILTIN_SANS).unwrap();
        let om = orig.glyph_metrics(Size::unscaled(), LocationRef::default());
        let bm = back.glyph_metrics(Size::unscaled(), LocationRef::default());
        for (k, gid) in ids.iter().enumerate() {
            let a = om.advance_width(GlyphId::new(*gid)).unwrap();
            let b = bm.advance_width(GlyphId::new(k as u32 + 1)).unwrap();
            assert!((a - b).abs() <= 0.5, "glyph {gid}: {a} vs {b}");
            let ob = om.bounds(GlyphId::new(*gid));
            let bb = bm.bounds(GlyphId::new(k as u32 + 1));
            if let (Some(ob), Some(bb)) = (ob, bb) {
                assert!((ob.x_min - bb.x_min).abs() <= 1.0 && (ob.y_max - bb.y_max).abs() <= 1.0, "glyph {gid}: {ob:?} vs {bb:?}");
            }
        }
        // The same request is the same bytes.
        assert_eq!(f.ttf, instance_subset(BUILTIN_SANS, 400.0, &slots).unwrap().ttf);
        // An empty slot is a glyph with no outline and no advance.
        let holed = instance_subset(BUILTIN_SANS, 400.0, &[None, slots[0]]).unwrap();
        assert_eq!(holed.advances[1], 0);
        assert_eq!(holed.advances[2], f.advances[1]);
        // Checksums: the whole file sums to the magic number.
        assert_eq!(checksum(&f.ttf), 0xB1B0_AFBA);
    }

    #[test]
    fn bold_is_an_instance_not_the_default_with_a_new_name() {
        let ids: Vec<Option<u32>> = gids("HEAVY", 700.0).into_iter().map(Some).collect();
        let reg = instance_subset(BUILTIN_SANS, 400.0, &ids).unwrap();
        let bold = instance_subset(BUILTIN_SANS, 700.0, &ids).unwrap();
        assert_eq!(bold.ps_name, "Inter-Bold");
        assert!(bold.advances.iter().sum::<u16>() > reg.advances.iter().sum::<u16>(), "bold glyphs are wider");
        assert_ne!(bold.ttf, reg.ttf);
    }

    #[test]
    fn a_ligature_reads_back_as_its_letters() {
        let l = shape_line(BUILTIN_SANS, "office", 400.0).unwrap();
        let texts = l.glyph_texts("office");
        assert_eq!(texts.concat(), "office", "{texts:?}");
    }
}
