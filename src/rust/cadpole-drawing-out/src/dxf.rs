//! DXF output, over the `dxf` crate (IxMilia, MIT) — the same crate the
//! importer uses, run in reverse.
//!
//! What matters here is that **nothing is approximated that need not be**. A
//! circular arc becomes an `LWPOLYLINE` vertex *bulge*, which is the exact DXF
//! encoding of an arc (`bulge = tan(θ/4)`), so a hole exported from Cadpole and
//! re-imported is the same arc rather than a chain of chords. Layers carry the
//! drawing's own names — the AIA/NCS ones for a plan — so the file opens into
//! an architect's template with the walls already on `A-WALL`.
//!
//! Nothing in the file comes from the clock or from a random source: the
//! `dxf` crate would stamp a fresh v4 GUID and the wall time into every
//! drawing, and both are overwritten, so the same drawing exports to the same
//! bytes.
//!
//! Two honest limits, both about what the `dxf` crate exposes rather than about
//! DXF:
//!
//! * **Line weights are BYLAYER.** `dxf::LineWeight` has no public constructor
//!   for a numeric width, so the layer table carries names and colours but not
//!   the ISO 128 widths. Templates that plot by layer or by colour are
//!   unaffected; the drawing's own weights still reach SVG and PDF.
//! * **Dimensions are exploded**, into lines, solid arrowheads and `TEXT`, not
//!   written as native `DIMENSION` entities, so every reader shows
//!   what Cadpole drew rather than re-deriving it from its own dimension style.

use chrono::{Local, NaiveDate, TimeZone, Utc};
use dxf::entities::{Entity, EntityType, Line, LwPolyline, Solid, Text};
use dxf::enums::{AcadVersion, DrawingUnits, HorizontalTextJustification, Units, VerticalTextJustification};
use dxf::tables::{Layer, LineType};
use dxf::{Color, Drawing as DxfDrawing, LwPolylineVertex, Point};
use cadpole_core::sketch::{arc_params, Loop, Path, Segment, P2};
use cadpole_drawing::ir::{Drawing, HAlign, Prim, VAlign};
use uuid::Uuid;

/// How the DXF is written.
#[derive(Clone, Debug)]
pub struct DxfOptions {
    /// `$INSUNITS`. The IR is in paper millimetres, so millimetres is the
    /// truthful default; an inch-first shop can say so.
    pub units: Units,
    pub version: AcadVersion,
    /// Chord tolerance for Bézier segments, in mm. Arcs are exact and are not
    /// affected.
    pub tolerance_mm: f64,
    /// Write every string as closed polylines of its glyph outlines rather
    /// than `TEXT`: what a cutter or a reader with no fonts wants.
    pub text_outlines: bool,
}

impl Default for DxfOptions {
    fn default() -> Self {
        DxfOptions {
            units: Units::Millimeters,
            version: AcadVersion::R2010,
            tolerance_mm: 0.05,
            text_outlines: false,
        }
    }
}

impl DxfOptions {
    /// `$INSUNITS` from a drawing unit: the coordinates are written as they
    /// are, so this says what they *mean* — a survey drawn in feet is written
    /// in feet, and `import_dxf` scales it by `$INSUNITS` on the way back.
    /// `ft-in` is a display format, not a unit: it writes feet.
    pub fn with_units(mut self, unit: cadpole_drawing::LengthUnit) -> DxfOptions {
        use cadpole_drawing::LengthUnit;
        self.units = match unit {
            LengthUnit::Mm => Units::Millimeters,
            LengthUnit::Cm => Units::Centimeters,
            LengthUnit::M => Units::Meters,
            LengthUnit::In => Units::Inches,
            LengthUnit::Ft | LengthUnit::FtIn => Units::Feet,
        };
        self
    }
}

/// Write `d` as a DXF file.
pub fn write(d: &Drawing, opts: &DxfOptions) -> Result<Vec<u8>, String> {
    let mut out = DxfDrawing::new();
    out.header.version = opts.version;
    out.header.default_drawing_units = opts.units;
    stamp(&mut out);

    // Linetype table before the layer table. `add_layer` calls the crate's
    // `ensure_line_type_is_present`, which inserts a bare record for a name it
    // has not seen; adding ours second left the file with two LTYPEs called
    // HIDDEN, the crate's empty one first.
    let mut linetypes: Vec<(&'static str, &[f64])> = Vec::new();
    for l in d.used_layers() {
        let lt = l.dash.dxf_name();
        if lt != "CONTINUOUS" && !linetypes.iter().any(|(n, _)| *n == lt) {
            linetypes.push((lt, l.dash.pattern()));
        }
    }
    for (lt, pattern) in linetypes {
        out.add_line_type(line_type(lt, pattern));
    }
    for l in d.used_layers() {
        out.add_layer(Layer {
            name: l.name.clone(),
            color: Color::from_index(l.color.aci().clamp(1, 255) as u8),
            line_type_name: l.dash.dxf_name().to_owned(),
            is_layer_plotted: l.print,
            ..Default::default()
        });
    }

    for prim in d.flatten() {
        match &prim {
            Prim::Path {
                style,
                path,
                closed,
                ..
            } => {
                push_path(&mut out, &style.layer, path, *closed, opts.tolerance_mm);
            }
            Prim::Fill {
                style,
                outer,
                holes,
                ..
            } => {
                // A triangle or quad becomes a real filled SOLID (which is how
                // AutoCAD has always drawn an arrowhead); anything else is
                // written as its outline, because the `dxf` crate's HATCH
                // boundary model is not worth the fidelity it would buy here.
                let pts = corners(outer);
                if holes.is_empty() && (3..=4).contains(&pts.len()) {
                    out.add_entity(with_layer(EntityType::Solid(solid(&pts)), &style.layer));
                } else {
                    push_loop(&mut out, &style.layer, outer, opts.tolerance_mm);
                    for h in holes {
                        push_loop(&mut out, &style.layer, h, opts.tolerance_mm);
                    }
                }
            }
            // A DXF has no place for a raster picture short of an external
            // reference, which this writer does not make; the PDF and the
            // SVG carry it.
            Prim::Image { .. } => {}
            Prim::Text {
                style,
                at,
                text,
                height_mm,
                rotation_deg,
                anchor,
                font,
            } if opts.text_outlines => {
                for l in cadpole_drawing::text::outlines(text, *height_mm, *at, *rotation_deg, *anchor, font) {
                    push_loop(&mut out, &style.layer, &l, opts.tolerance_mm);
                }
            }
            Prim::Text {
                style,
                at,
                text,
                height_mm,
                rotation_deg,
                anchor,
                ..
            } => {
                let (h, v) = (
                    match anchor.h {
                        HAlign::Left => HorizontalTextJustification::Left,
                        HAlign::Center => HorizontalTextJustification::Center,
                        HAlign::Right => HorizontalTextJustification::Right,
                    },
                    match anchor.v {
                        VAlign::Baseline => VerticalTextJustification::Baseline,
                        VAlign::Bottom => VerticalTextJustification::Bottom,
                        VAlign::Middle => VerticalTextJustification::Middle,
                        VAlign::Top => VerticalTextJustification::Top,
                    },
                );
                let p = Point::new(finite(at.x), finite(at.y), 0.0);
                out.add_entity(with_layer(
                    EntityType::Text(Text {
                        location: p.clone(),
                        // A justified TEXT entity is positioned by its second
                        // alignment point, so both must be set.
                        second_alignment_point: p,
                        text_height: finite(*height_mm),
                        value: text.clone(),
                        rotation: finite(*rotation_deg),
                        horizontal_text_justification: h,
                        vertical_text_justification: v,
                        ..Default::default()
                    }),
                    &style.layer,
                ));
            }
        }
    }

    // $EXTMIN/$EXTMAX: the box of what was written, or AutoCAD's ±1e20
    // for nothing. The crate's default is 0,0,0 for both, which declared a
    // zero-size box at the origin whatever the drawing held.
    let (lo, hi) = extents(&out).unwrap_or((P2::new(1e20, 1e20), P2::new(-1e20, -1e20)));
    let (zlo, zhi) = if lo.x <= hi.x { (0.0, 0.0) } else { (1e20, -1e20) };
    out.header.minimum_drawing_extents = Point::new(lo.x, lo.y, zlo);
    out.header.maximum_drawing_extents = Point::new(hi.x, hi.y, zhi);

    let mut bytes: Vec<u8> = Vec::new();
    out.save(&mut bytes)
        .map_err(|e| format!("writing DXF: {e}"))?;
    Ok(conform(bytes, opts.version >= AcadVersion::R13))
}

/// The box of every entity in `out`, exact for lines, solids and bulged
/// polyline arcs; a `TEXT` counts as the box its string would fill in the
/// built-in font, turned by its rotation. `None` when nothing was written.
fn extents(out: &DxfDrawing) -> Option<(P2, P2)> {
    let mut lo = P2::new(f64::MAX, f64::MAX);
    let mut hi = P2::new(f64::MIN, f64::MIN);
    let mut any = false;
    let mut eat = |p: P2| {
        if p.x.is_finite() && p.y.is_finite() {
            lo = P2::new(lo.x.min(p.x), lo.y.min(p.y));
            hi = P2::new(hi.x.max(p.x), hi.y.max(p.y));
            any = true;
        }
    };
    let pt = |p: &Point| P2::new(p.x, p.y);
    for e in out.entities() {
        match &e.specific {
            EntityType::Line(l) => {
                eat(pt(&l.p1));
                eat(pt(&l.p2));
            }
            EntityType::Solid(s) => {
                for c in [&s.first_corner, &s.second_corner, &s.third_corner, &s.fourth_corner] {
                    eat(pt(c));
                }
            }
            EntityType::LwPolyline(pl) => {
                let v = &pl.vertices;
                let n = v.len();
                let edges = if pl.flags & 1 != 0 { n } else { n.saturating_sub(1) };
                for w in v {
                    eat(P2::new(w.x, w.y));
                }
                for i in 0..edges {
                    let (a, b) = (&v[i], &v[(i + 1) % n]);
                    for p in bulge_extremes(P2::new(a.x, a.y), P2::new(b.x, b.y), a.bulge) {
                        eat(p);
                    }
                }
            }
            EntityType::Text(t) => {
                let at = pt(&t.second_alignment_point);
                let h = t.text_height;
                let w = cadpole_drawing::text::width(&t.value, h);
                let x0 = match t.horizontal_text_justification {
                    HorizontalTextJustification::Center | HorizontalTextJustification::Middle => -w / 2.0,
                    HorizontalTextJustification::Right => -w,
                    _ => 0.0,
                };
                let (sn, cs) = t.rotation.to_radians().sin_cos();
                for (dx, dy) in [(x0, -h), (x0 + w, -h), (x0, h), (x0 + w, h)] {
                    eat(P2::new(at.x + dx * cs - dy * sn, at.y + dx * sn + dy * cs));
                }
            }
            _ => {}
        }
    }
    any.then_some((lo, hi))
}

/// The axis-extreme points of the arc a polyline bulge encodes from `a` to
/// `b` (`bulge = tan(θ/4)`, positive counter-clockwise): every point where
/// the arc crosses a quarter-turn angle. Empty for a straight edge.
fn bulge_extremes(a: P2, b: P2, bulge: f64) -> Vec<P2> {
    let (dx, dy) = (b.x - a.x, b.y - a.y);
    let chord = dx.hypot(dy);
    if bulge.abs() < 1e-12 || !bulge.is_finite() || chord <= 0.0 {
        return Vec::new();
    }
    let sweep = 4.0 * bulge.atan();
    // The centre sits on the chord's perpendicular bisector, to the left of
    // a → b for a counter-clockwise arc under a half turn.
    let k = 0.5 / (sweep / 2.0).tan();
    let c = P2::new((a.x + b.x) / 2.0 - dy * k, (a.y + b.y) / 2.0 + dx * k);
    let r = (a.x - c.x).hypot(a.y - c.y);
    let t0 = (a.y - c.y).atan2(a.x - c.x);
    let (from, to) = if sweep >= 0.0 { (t0, t0 + sweep) } else { (t0 + sweep, t0) };
    let quarter = std::f64::consts::FRAC_PI_2;
    let mut out = Vec::new();
    let mut q = (from / quarter).ceil();
    while q * quarter <= to && out.len() < 8 {
        let (sn, cs) = (q * quarter).sin_cos();
        out.push(P2::new(c.x + r * cs, c.y + r * sn));
        q += 1.0;
    }
    out
}

/// Repair the two things the `dxf` crate (0.6.1) writes that a strict
/// reader rejects, on the ASCII it produced. The crate exposes neither, so
/// this is a pass over its output rather than a setting.
///
/// * **Group 440 = 0 on every entity.** The crate writes `transparency`
///   unconditionally from R2004 with a default of 0, which is no
///   transparency at all: a valid value carries a type byte (`0x02000000 |
///   alpha`, or `0x01000000` for ByBlock). The DXF reference omits 440 for
///   ByLayer, which is what these entities mean, so the pair is dropped.
/// * **TABLE heads without a handle.** From R13 every table head is an
///   object with its own handle (group 5) and owner (330, the root, 0); the
///   crate writes handles only on the records. Each head gets the next free
///   handle, and `$HANDSEED` is set past every handle in the file, which
///   the crate's own value was not.
///
/// Anything that does not look like the crate's code/value lines is passed
/// through untouched.
fn conform(bytes: Vec<u8>, handles: bool) -> Vec<u8> {
    let Ok(text) = std::str::from_utf8(&bytes) else { return bytes };
    let mut lines: Vec<&str> = text.split("\r\n").collect();
    if lines.last() == Some(&"") {
        lines.pop();
    }
    if lines.len() % 2 != 0 {
        return bytes;
    }
    let mut pairs: Vec<(i32, &str, &str)> = Vec::with_capacity(lines.len() / 2);
    for c in lines.chunks_exact(2) {
        let Ok(code) = c[0].trim().parse::<i32>() else { return bytes };
        pairs.push((code, c[0], c[1]));
    }

    // Every handle in use, outside the header (whose group 5 is $HANDSEED).
    let mut section = "";
    let mut next: u64 = 1;
    for (i, (code, _, value)) in pairs.iter().enumerate() {
        if *code == 0 && value.trim() == "SECTION" {
            section = pairs.get(i + 1).map(|p| p.2.trim()).unwrap_or("");
            continue;
        }
        if section != "HEADER" && (*code == 5 || *code == 105) {
            if let Ok(h) = u64::from_str_radix(value.trim(), 16) {
                next = next.max(h + 1);
            }
        }
    }

    let mut out: Vec<(String, String)> = Vec::with_capacity(pairs.len() + 32);
    let mut section = "";
    let mut i = 0;
    while i < pairs.len() {
        let (code, raw_code, value) = pairs[i];
        if code == 0 && value.trim() == "SECTION" {
            section = pairs.get(i + 1).map(|p| p.2.trim()).unwrap_or("");
        }
        if code == 440 && matches!(section, "ENTITIES" | "BLOCKS") && value.trim().parse::<i64>() == Ok(0) {
            i += 1;
            continue;
        }
        out.push((raw_code.to_owned(), value.to_owned()));
        if handles && section == "TABLES" && code == 0 && value.trim() == "TABLE" {
            if let Some(&(2, name_code, name)) = pairs.get(i + 1) {
                out.push((name_code.to_owned(), name.to_owned()));
                out.push(("  5".to_owned(), format!("{next:X}")));
                out.push(("330".to_owned(), "0".to_owned()));
                next += 1;
                i += 2;
                continue;
            }
        }
        i += 1;
    }

    if handles {
        // $HANDSEED: the header variable's value is the group 5 after it.
        if let Some(at) = out.iter().position(|(c, v)| c.trim() == "9" && v.trim() == "$HANDSEED") {
            if let Some(seed) = out.get_mut(at + 1).filter(|(c, _)| c.trim() == "5") {
                seed.1 = format!("{next:X}");
            }
        }
    }

    let mut s = String::with_capacity(text.len() + 256);
    for (c, v) in out {
        s.push_str(&c);
        s.push_str("\r\n");
        s.push_str(&v);
        s.push_str("\r\n");
    }
    s.into_bytes()
}

/// Replace the two things the `dxf` crate would otherwise make random.
///
/// It stamps a fresh v4 GUID and the wall clock into every drawing, so two
/// exports of the same model would differ. Cadpole's exports are reproducible, so
/// the GUIDs are nil and the dates are a fixed *local* wall time — fixed in
/// local terms rather than in UTC because that is the form DXF stores, which
/// makes the bytes the same on every machine as well as on every run.
/// An LTYPE record that actually describes the dashes.
///
/// `Dash::pattern` is dash, gap, dash, gap... in paper millimetres. DXF wants
/// one signed length per element — pen down positive, pen up negative, a dot
/// exactly zero — plus the element count (73) and the length of one repeat
/// (40). Leaving those at their defaults, which is what `..Default::default()`
/// does, writes a linetype named HIDDEN whose pattern says "solid": SVG and
/// PDF dashed the same edges while every DXF reader drew them continuous, with
/// only the layer name and colour 8 to tell them apart.
fn line_type(name: &str, pattern: &[f64]) -> LineType {
    let lengths: Vec<f64> =
        pattern.iter().enumerate().map(|(i, v)| if i % 2 == 0 { v.abs() } else { -v.abs() }).collect();
    LineType {
        name: name.to_owned(),
        description: format!("Cadpole {name}"),
        element_count: lengths.len() as i32,
        total_pattern_length: lengths.iter().map(|v| v.abs()).sum(),
        dash_dot_space_lengths: lengths,
        ..Default::default()
    }
}

fn stamp(out: &mut DxfDrawing) {
    // $MEASUREMENT says which linetype and hatch pattern files to load, and
    // it is a separate variable from $INSUNITS: leaving it at its English
    // default beside `$INSUNITS = 4` sends CAD looking for the imperial ones.
    out.header.drawing_units = DrawingUnits::Metric;
    out.header.fingerprint_guid = Uuid::nil();
    out.header.version_guid = Uuid::nil();
    let naive = NaiveDate::from_ymd_opt(2000, 1, 1).and_then(|d| d.and_hms_opt(12, 0, 0));
    let Some(naive) = naive else { return };
    if let Some(t) = Local.from_local_datetime(&naive).earliest() {
        out.header.creation_date = t;
        out.header.update_date = t;
    }
    // The file carries the same instant twice, once local and once UTC, and
    // both default to the clock.
    let utc = Utc.from_utc_datetime(&naive);
    out.header.creation_date_universal = utc;
    out.header.update_date_universal = utc;
}

/// `tan(sweep/4)`, the DXF encoding of an arc, guarded so a degenerate sweep
/// can never write `inf` into the file.
fn bulge(sweep: f64) -> f64 {
    let b = (sweep / 4.0).tan();
    if b.is_finite() {
        b
    } else {
        0.0
    }
}

/// 0 for anything that is not a real number.
fn finite(v: f64) -> f64 {
    if v.is_finite() {
        v
    } else {
        0.0
    }
}

fn with_layer(t: EntityType, layer: &str) -> Entity {
    let mut e = Entity::new(t);
    e.common.layer = layer.to_owned();
    e
}

fn solid(pts: &[P2]) -> Solid {
    // DXF SOLID takes its corners in the order 1, 2, 4, 3 — the "bowtie"
    // order. A triangle repeats its last corner.
    let p = |i: usize| {
        let q = pts
            .get(i.min(pts.len() - 1))
            .copied()
            .unwrap_or(P2::new(0.0, 0.0));
        Point::new(finite(q.x), finite(q.y), 0.0)
    };
    Solid {
        first_corner: p(0),
        second_corner: p(1),
        third_corner: p(if pts.len() > 3 { 3 } else { 2 }),
        fourth_corner: p(2),
        ..Default::default()
    }
}

/// The distinct corners of a loop made only of lines, or nothing.
fn corners(l: &Loop) -> Vec<P2> {
    let mut out = Vec::new();
    for s in &l.segments {
        match s {
            Segment::Line { a, .. } => out.push(*a),
            _ => return Vec::new(),
        }
    }
    out
}

fn push_path(out: &mut DxfDrawing, layer: &str, path: &Path, closed: bool, tolerance: f64) {
    if path.segments.is_empty() {
        return;
    }
    if let Some(v) = vertices(path.start, &path.segments, tolerance) {
        add_poly(out, layer, v, closed);
    }
}

fn push_loop(out: &mut DxfDrawing, layer: &str, l: &Loop, tolerance: f64) {
    if l.segments.is_empty() {
        return;
    }
    if let Some(v) = vertices(l.segments[0].start(), &l.segments, tolerance) {
        add_poly(out, layer, v, true);
    }
}

fn add_poly(out: &mut DxfDrawing, layer: &str, vertices: Vec<LwPolylineVertex>, closed: bool) {
    match vertices.len() {
        0 | 1 => {}
        2 if vertices[0].bulge == 0.0 && !closed => {
            // Two points and no curvature: a LINE reads better in every editor
            // than a two-vertex polyline.
            out.add_entity(with_layer(
                EntityType::Line(Line {
                    p1: Point::new(vertices[0].x, vertices[0].y, 0.0),
                    p2: Point::new(vertices[1].x, vertices[1].y, 0.0),
                    ..Default::default()
                }),
                layer,
            ));
        }
        _ => {
            out.add_entity(with_layer(
                EntityType::LwPolyline(LwPolyline {
                    flags: if closed { 1 } else { 0 },
                    vertices,
                    ..Default::default()
                }),
                layer,
            ));
        }
    }
}

/// Segments as polyline vertices, arcs kept exact through their bulge.
///
/// `bulge = tan(θ/4)` for an included angle θ, positive counter-clockwise —
/// the same relation the importer reads, so a round trip is lossless.
fn vertices(start: P2, segments: &[Segment], tolerance: f64) -> Option<Vec<LwPolylineVertex>> {
    fn vertex(p: P2) -> LwPolylineVertex {
        // DXF is a text format and a non-finite coordinate is written
        // verbatim, producing a file every reader rejects. Nothing upstream
        // should produce one; this is where saying so is cheapest.
        LwPolylineVertex {
            x: finite(p.x),
            y: finite(p.y),
            bulge: 0.0,
            ..Default::default()
        }
    }
    let mut v: Vec<LwPolylineVertex> = Vec::with_capacity(segments.len() + 1);
    v.push(vertex(start));
    for s in segments {
        match s {
            Segment::Line { b, .. } => v.push(vertex(*b)),
            Segment::Arc { a, b, via } => {
                match arc_params(*a, *b, *via) {
                    // `bulge = tan(sweep/4)` runs to infinity as the sweep
                    // approaches a full turn, so an arc of more than half a
                    // turn is written as two — which is exact, and is what
                    // `Loop::circle` does anyway.
                    Some((c, r, t0, t1))
                        if r.is_finite() && r > 0.0 && (t1 - t0).abs() > std::f64::consts::PI =>
                    {
                        let tm = (t0 + t1) / 2.0;
                        let mid = P2::new(c.x + tm.cos() * r, c.y + tm.sin() * r);
                        if let Some(last) = v.last_mut() {
                            last.bulge = bulge(tm - t0);
                        }
                        let mut mv = vertex(mid);
                        mv.bulge = bulge(t1 - tm);
                        v.push(mv);
                        v.push(vertex(*b));
                        continue;
                    }
                    Some((_, r, t0, t1)) if r.is_finite() && r > 0.0 => {
                        if let Some(last) = v.last_mut() {
                            last.bulge = bulge(t1 - t0);
                        }
                    }
                    _ => {}
                }
                v.push(vertex(*b));
            }
            Segment::Bezier { .. } => {
                // Béziers have no exact DXF form short of a SPLINE; a
                // chord-bounded polyline is the honest approximation and the
                // one every reader agrees about.
                let l = Loop {
                    segments: vec![s.clone()],
                };
                for p in l.sample_tolerance(tolerance).into_iter().skip(1) {
                    v.push(vertex(p));
                }
                v.push(vertex(s.end()));
            }
        }
    }
    // Drop a duplicated closing vertex: the closed flag says it already.
    if v.len() > 2 {
        let (f, l) = (v[0].clone(), v[v.len() - 1].clone());
        if (f.x - l.x).abs() < 1e-9 && (f.y - l.y).abs() < 1e-9 && l.bulge == 0.0 {
            v.pop();
        }
    }
    if v.len() < 2 {
        None
    } else {
        Some(v)
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use cadpole_drawing::{layer, Anchor};

    fn text_of(bytes: &[u8]) -> String {
        String::from_utf8_lossy(bytes).into_owned()
    }

    #[test]
    fn a_rectangle_is_one_closed_polyline_on_its_layer() {
        let mut d = Drawing::new();
        let l = d.layer(layer::WALL);
        d.loop_stroke(l, &Loop::rect(P2::new(0.0, 0.0), 100.0, 50.0));
        let s = text_of(&write(&d, &DxfOptions::default()).unwrap());
        assert_eq!(s.matches("LWPOLYLINE").count(), 1, "{s}");
        assert!(s.contains("A-WALL"));
        assert!(s.contains("$INSUNITS"));
    }

    /// Every record in the TABLES section as `(type, [(code, value)])`, in
    /// file order and keeping repeats — group code 49 appears once per dash.
    fn table_records(bytes: &[u8]) -> Vec<(String, Vec<(i32, String)>)> {
        let text = text_of(bytes);
        let lines: Vec<&str> = text.split("\r\n").collect();
        let pairs: Vec<(i32, &str)> = lines
            .chunks_exact(2)
            .filter_map(|c| c[0].trim().parse::<i32>().ok().map(|code| (code, c[1])))
            .collect();
        let mut out: Vec<(String, Vec<(i32, String)>)> = Vec::new();
        let mut in_tables = false;
        for (i, (code, value)) in pairs.iter().enumerate() {
            if *code == 0 && value.trim() == "SECTION" {
                in_tables = pairs.get(i + 1).map(|(_, v)| v.trim()) == Some("TABLES");
                continue;
            }
            if *code == 0 && value.trim() == "ENDSEC" {
                in_tables = false;
                continue;
            }
            if !in_tables {
                continue;
            }
            if *code == 0 {
                out.push((value.trim().to_owned(), Vec::new()));
            } else if let Some(last) = out.last_mut() {
                last.1.push((*code, value.trim().to_owned()));
            }
        }
        out
    }

    fn header_var(bytes: &[u8], name: &str) -> Option<String> {
        let text = text_of(bytes);
        let lines: Vec<&str> = text.split("\r\n").collect();
        let at = lines.iter().position(|l| l.trim() == name)?;
        lines.get(at + 2).map(|v| v.trim().to_owned())
    }

    #[test]
    fn a_hidden_layer_writes_a_linetype_that_actually_dashes() {
        // The DXF said HIDDEN and drew solid: the LTYPE record carried a name
        // and nothing else, so every reader plotted the hidden edges
        // continuous while the SVG and the PDF dashed them.
        let mut d = Drawing::new();
        let h = d.layer(layer::HIDDEN);
        d.loop_stroke(h, &Loop::rect(P2::new(0.0, 0.0), 10.0, 10.0));
        let bytes = write(&d, &DxfOptions::default()).unwrap();
        let hidden: Vec<_> = table_records(&bytes)
            .into_iter()
            .filter(|(t, r)| t == "LTYPE" && r.iter().any(|(c, v)| *c == 2 && v == "HIDDEN"))
            .collect();
        // One record, not two: `add_layer` inserts a bare one for a name it
        // has not seen, so the linetypes have to go in first.
        assert_eq!(hidden.len(), 1, "one HIDDEN LTYPE: {hidden:?}");
        let r = &hidden[0].1;
        let get = |code: i32| r.iter().find(|(c, _)| *c == code).map(|(_, v)| v.clone());
        assert_eq!(get(73).as_deref(), Some("2"), "element count: {r:?}");
        assert_eq!(get(40).and_then(|v| v.parse::<f64>().ok()), Some(3.0), "pattern length: {r:?}");
        // Dash 2 mm, gap 1 mm — the same 2,1 the SVG writes as a dasharray.
        let dashes: Vec<f64> =
            r.iter().filter(|(c, _)| *c == 49).filter_map(|(_, v)| v.parse().ok()).collect();
        assert_eq!(dashes, vec![2.0, -1.0], "{r:?}");
    }

    #[test]
    fn a_centre_line_keeps_its_four_element_pattern() {
        let mut d = Drawing::new();
        let c = d.layer(layer::CENTER);
        d.loop_stroke(c, &Loop::rect(P2::new(0.0, 0.0), 10.0, 10.0));
        let bytes = write(&d, &DxfOptions::default()).unwrap();
        let rec = table_records(&bytes)
            .into_iter()
            .find(|(t, r)| t == "LTYPE" && r.iter().any(|(c, v)| *c == 2 && v == "CENTER"))
            .expect("a CENTER linetype");
        let dashes: Vec<f64> =
            rec.1.iter().filter(|(c, _)| *c == 49).filter_map(|(_, v)| v.parse().ok()).collect();
        assert_eq!(dashes, vec![8.0, -1.5, 1.5, -1.5]);
        assert_eq!(
            rec.1.iter().find(|(c, _)| *c == 40).map(|(_, v)| v.parse::<f64>().unwrap()),
            Some(12.5)
        );
    }

    #[test]
    fn the_header_is_metric_in_both_places_that_say_so() {
        let mut d = Drawing::new();
        let l = d.layer(layer::VISIBLE);
        d.loop_stroke(l, &Loop::rect(P2::new(0.0, 0.0), 10.0, 10.0));
        let bytes = write(&d, &DxfOptions::default()).unwrap();
        // $INSUNITS 4 is millimetres; $MEASUREMENT 1 is metric, and it is what
        // picks the linetype and hatch pattern files.
        assert_eq!(header_var(&bytes, "$INSUNITS").as_deref(), Some("4"));
        assert_eq!(header_var(&bytes, "$MEASUREMENT").as_deref(), Some("1"));
    }

    #[test]
    fn a_circle_keeps_its_curvature_as_bulges() {
        let mut d = Drawing::new();
        let l = d.layer(layer::VISIBLE);
        d.loop_stroke(l, &Loop::circle(P2::new(0.0, 0.0), 10.0));
        let bytes = write(&d, &DxfOptions::default()).unwrap();
        let s = text_of(&bytes);
        // Group code 42 is the vertex bulge; a half circle's bulge is 1.
        assert!(s.contains("\r\n 42\r\n"), "no bulges written");
        assert_eq!(
            s.matches("\r\n 42\r\n1.0\r\n").count(),
            2,
            "two half arcs of bulge 1"
        );
    }

    #[test]
    fn an_arrowhead_becomes_a_filled_solid() {
        use cadpole_drawing::{Dim, DimStyle, Item, Style};
        let mut d = Drawing::new();
        let dl = d.layer(layer::DIMS);
        d.push(Item::Dim(Dim::linear(
            Style::on(dl),
            P2::new(0.0, 0.0),
            P2::new(100.0, 0.0),
            P2::new(1.0, 0.0),
            10.0,
            DimStyle::mechanical(),
        )));
        let s = text_of(&write(&d, &DxfOptions::default()).unwrap());
        assert_eq!(s.matches("\r\nSOLID\r\n").count(), 2, "two arrowheads");
        assert!(s.contains("\r\nTEXT\r\n"));
        assert!(s.contains("100"));
    }

    #[test]
    fn text_carries_its_height_and_justification() {
        let mut d = Drawing::new();
        let l = d.layer(layer::TEXT);
        d.text(l, P2::new(10.0, 20.0), "CADPOLE", 2.5, Anchor::CENTER);
        let s = text_of(&write(&d, &DxfOptions::default()).unwrap());
        assert!(s.contains("\r\nCADPOLE\r\n"));
        assert!(
            s.contains("\r\n 40\r\n2.5\r\n"),
            "text height as group code 40"
        );
    }

    #[test]
    fn text_can_be_written_as_outlines_instead() {
        let mut d = Drawing::new();
        let l = d.layer(layer::TEXT);
        d.text(l, P2::new(10.0, 20.0), "CADPOLE", 2.5, Anchor::CENTER);
        let s = text_of(&write(&d, &DxfOptions { text_outlines: true, ..Default::default() }).unwrap());
        assert!(!s.contains("\r\nCADPOLE\r\n"), "no TEXT entity");
        assert!(s.matches("LWPOLYLINE").count() >= 4, "one closed polyline per contour: {}", s.matches("LWPOLYLINE").count());
    }

    #[test]
    fn nothing_in_the_header_comes_from_the_clock() {
        // Two exports of one model are the same bytes, which means no GUID and
        // no timestamp: both the local and the universal date fields are
        // pinned, and `dxf` writes both.
        let mut d = Drawing::new();
        let l = d.layer(layer::VISIBLE);
        d.loop_stroke(l, &Loop::rect(P2::new(0.0, 0.0), 10.0, 10.0));
        let s = text_of(&write(&d, &DxfOptions::default()).unwrap());
        // The two GUIDs are nil rather than fresh v4 randomness.
        assert_eq!(s.matches("00000000-0000-0000-0000-000000000000").count(), 2);
        // 2000-01-01T12:00 is julian day 2451545.0; the local field lands
        // within a day of it whatever the machine's zone.
        let julians: Vec<f64> = s
            .lines()
            .filter_map(|l| l.trim().parse::<f64>().ok())
            .filter(|v| (2_400_000.0..2_500_000.0).contains(v))
            .collect();
        assert_eq!(julians.len(), 4, "TDCREATE/TDUCREATE/TDUPDATE/TDUUPDATE");
        for j in julians {
            assert!(
                (j - 2_451_545.0).abs() < 1.0,
                "pinned to 2000-01-01, got {j}"
            );
        }
    }

    #[test]
    fn a_nearly_closed_arc_does_not_write_an_infinite_bulge() {
        use cadpole_core::sketch::{Path, Segment};
        // A single arc of almost a full turn: `tan(sweep/4)` heads for
        // infinity, so it must be written as two.
        let r = 10.0;
        let a = P2::new(r, 0.0);
        let end = (-0.02f64).to_radians();
        let b = P2::new(r * end.cos(), r * end.sin());
        let via = P2::new(-r, 0.0);
        let mut d = Drawing::new();
        let l = d.layer(layer::VISIBLE);
        d.stroke(
            l,
            Path {
                start: a,
                segments: vec![Segment::Arc { a, b, via }],
            },
            false,
        );
        let s = text_of(&write(&d, &DxfOptions::default()).unwrap());
        assert!(!s.contains("inf"), "no infinite bulge");
        assert!(!s.contains("NaN"), "no NaN");
        // Two bulges, each of a half-turn or less. Code 42 means other things
        // elsewhere in a DXF (a text style's last-used height, for one), so
        // look only inside the polyline this test wrote.
        let entity = &s[s.find("LWPOLYLINE").expect("one polyline")..];
        let bulges: Vec<f64> = entity
            .split("\r\n 42\r\n")
            .skip(1)
            .filter_map(|rest| rest.split("\r\n").next()?.trim().parse::<f64>().ok())
            .collect();
        assert_eq!(bulges.len(), 2, "split into two arcs: {bulges:?}");
        for b in bulges {
            assert!(b.abs() <= 1.0 + 1e-9, "each is at most a half turn: {b}");
        }
    }

    /// Every group code/value pair in the file, in order.
    fn pairs(bytes: &[u8]) -> Vec<(i32, String)> {
        let text = text_of(bytes);
        let lines: Vec<&str> = text.split("\r\n").collect();
        lines
            .chunks_exact(2)
            .filter_map(|c| c[0].trim().parse::<i32>().ok().map(|code| (code, c[1].trim().to_owned())))
            .collect()
    }

    /// A drawing with a bit of everything the writer emits: polylines on a
    /// dashed layer, an exploded dimension (SOLIDs, LINEs, TEXT) and text.
    fn everything() -> Drawing {
        use cadpole_drawing::{Dim, DimStyle, Item, Style};
        let mut d = Drawing::new();
        let h = d.layer(layer::HIDDEN);
        d.loop_stroke(h, &Loop::rect(P2::new(0.0, 0.0), 10.0, 10.0));
        let v = d.layer(layer::VISIBLE);
        d.loop_stroke(v, &Loop::circle(P2::new(50.0, 0.0), 10.0));
        let dl = d.layer(layer::DIMS);
        d.push(Item::Dim(Dim::linear(
            Style::on(dl),
            P2::new(0.0, 0.0),
            P2::new(100.0, 0.0),
            P2::new(1.0, 0.0),
            10.0,
            DimStyle::mechanical(),
        )));
        let t = d.layer(layer::TEXT);
        d.text(t, P2::new(10.0, 20.0), "CADPOLE", 2.5, Anchor::CENTER);
        d
    }

    #[test]
    fn no_entity_carries_the_invalid_transparency_zero() {
        // The `dxf` crate writes group 440 = 0 on every entity from R2004 on.
        // 0 is not a transparency (the type byte is missing); ezdxf's audit
        // repaired it on every entity. Absent means ByLayer.
        let p = pairs(&write(&everything(), &DxfOptions::default()).unwrap());
        let bad = p.iter().filter(|(c, v)| *c == 440 && v.parse::<i64>() == Ok(0)).count();
        assert_eq!(bad, 0, "440 = 0 pairs written");
    }

    #[test]
    fn every_table_head_has_its_own_handle_and_the_seed_is_past_them_all() {
        let bytes = write(&everything(), &DxfOptions::default()).unwrap();
        let p = pairs(&bytes);
        let mut heads = 0;
        for (i, (c, v)) in p.iter().enumerate() {
            if *c == 0 && v == "TABLE" {
                heads += 1;
                assert_eq!(p[i + 1].0, 2, "a table head names its table first");
                let name = &p[i + 1].1;
                // The head's handle comes before its subclass marker.
                let rest = &p[i + 2..];
                let end = rest.iter().position(|(c, _)| *c == 0).unwrap();
                let head = &rest[..end];
                let h = head.iter().position(|(c, _)| *c == 5);
                let sub = head.iter().position(|(c, v)| *c == 100 && v == "AcDbSymbolTable");
                assert!(h.is_some() && sub.is_some() && h < sub, "TABLE {name} has no head handle: {head:?}");
            }
        }
        assert!(heads >= 7, "VPORT LTYPE LAYER STYLE APPID DIMSTYLE BLOCK_RECORD at least: {heads}");
        // Handles are unique, and $HANDSEED is past every one of them. The
        // header comes first, and its group 5 is $HANDSEED itself.
        let after_header = p.iter().position(|(c, v)| *c == 0 && v == "ENDSEC").unwrap();
        let mut handles: Vec<u64> = p[after_header..]
            .iter()
            .filter(|(c, _)| *c == 5 || *c == 105)
            .filter_map(|(_, v)| u64::from_str_radix(v, 16).ok())
            .collect();
        let n = handles.len();
        handles.sort_unstable();
        handles.dedup();
        assert_eq!(handles.len(), n, "duplicate handles");
        let seed = header_var(&bytes, "$HANDSEED").and_then(|v| u64::from_str_radix(&v, 16).ok()).unwrap();
        assert!(seed > *handles.last().unwrap(), "seed {seed:X} not past {:X}", handles.last().unwrap());
    }

    /// A header point variable as (x, y, z).
    fn header_point(bytes: &[u8], name: &str) -> (f64, f64, f64) {
        let p = pairs(bytes);
        let at = p.iter().position(|(c, v)| *c == 9 && v == name).unwrap_or_else(|| panic!("no {name}"));
        let get = |code: i32| {
            p[at + 1..at + 4].iter().find(|(c, _)| *c == code).and_then(|(_, v)| v.parse::<f64>().ok()).unwrap()
        };
        (get(10), get(20), get(30))
    }

    #[test]
    fn the_header_extents_are_the_drawings() {
        // A 40 x 30 rectangle centred on (100, 50) and a circle of r = 10 on
        // (200, 0): the circle's extremes are on its bulged arcs, not its
        // vertices. The header used to say 0,0,0 to 0,0,0 whatever was drawn.
        let mut d = Drawing::new();
        let l = d.layer(layer::VISIBLE);
        d.loop_stroke(l, &Loop::rect(P2::new(100.0, 50.0), 40.0, 30.0));
        d.loop_stroke(l, &Loop::circle(P2::new(200.0, 0.0), 10.0));
        let bytes = write(&d, &DxfOptions::default()).unwrap();
        let (lo, hi) = (header_point(&bytes, "$EXTMIN"), header_point(&bytes, "$EXTMAX"));
        let close = |a: f64, b: f64| (a - b).abs() < 1e-9;
        assert!(close(lo.0, 80.0) && close(lo.1, -10.0) && close(lo.2, 0.0), "EXTMIN {lo:?}");
        assert!(close(hi.0, 210.0) && close(hi.1, 65.0) && close(hi.2, 0.0), "EXTMAX {hi:?}");
        assert!(!crate::dxf::tests::text_of(&bytes).contains("\r\n-0.0\r\n"), "no negative zero");
    }

    #[test]
    fn the_header_extents_cover_the_text_too() {
        let mut d = Drawing::new();
        let l = d.layer(layer::TEXT);
        d.text(l, P2::new(10.0, 20.0), "CADPOLE", 2.5, Anchor::CENTER);
        let bytes = write(&d, &DxfOptions::default()).unwrap();
        let (lo, hi) = (header_point(&bytes, "$EXTMIN"), header_point(&bytes, "$EXTMAX"));
        assert!(lo.0 < 10.0 && hi.0 > 10.0 && lo.1 <= 20.0 && hi.1 >= 20.0, "{lo:?} {hi:?}");
        assert!(hi.0 - lo.0 > 2.5, "the box is as wide as the string, roughly: {lo:?} {hi:?}");
    }

    #[test]
    fn an_empty_drawing_declares_no_extents() {
        // AutoCAD's convention for "nothing drawn", which `import_dxf` reads
        // back as no extents rather than a box at the origin.
        let bytes = write(&Drawing::new(), &DxfOptions::default()).unwrap();
        let (lo, hi) = (header_point(&bytes, "$EXTMIN"), header_point(&bytes, "$EXTMAX"));
        assert!(lo.0 >= 1e20 && hi.0 <= -1e20, "{lo:?} {hi:?}");
    }

    #[test]
    fn output_is_byte_identical_across_runs() {
        let mut d = Drawing::new();
        let l = d.layer(layer::VISIBLE);
        d.loop_stroke(l, &Loop::rect(P2::new(0.0, 0.0), 10.0, 10.0));
        let a = write(&d, &DxfOptions::default()).unwrap();
        let b = write(&d, &DxfOptions::default()).unwrap();
        assert_eq!(a, b);
    }
}
