//! The SVG writer. No dependency: SVG is text, and writing it by hand is how
//! the arcs stay arcs.
//!
//! The output is deliberately *importable*. Coordinates are millimetres, the
//! root carries `width="Wmm" height="Hmm" viewBox="0 0 W H"` so the units are
//! an exact statement rather than a guess, each layer is one `<g id="A-WALL">`,
//! and circular arcs are written as `A` commands with equal radii. Cadpole's own
//! SVG importer reads that back into the same loops, arcs included, which is
//! the round-trip the tests check — and any other tool reading it gets a
//! correctly scaled drawing instead of a pile of pixels.
//!
//! SVG's y axis points down, so every coordinate is mirrored on the way out
//! (`y' = top - y`). The mirror is baked into the numbers rather than applied
//! as a group transform, because a transform would also flip the text.

use std::fmt::Write as _;

use cadpole_core::sketch::{arc_params, Path, Segment, P2};

use crate::ir::{Dash, Drawing, FontRef, HAlign, Prim, Resolved, Rgb};

/// How the SVG is written.
#[derive(Clone, Debug)]
pub struct SvgOptions {
    /// Digits after the decimal point. Four is a hundredth of a micron and
    /// keeps files small; it is also what makes output byte-stable.
    pub decimals: usize,
    /// Margin around the content when the drawing has no sheet, in mm.
    pub margin_mm: f64,
    /// Paint the page. `None` leaves it transparent.
    pub background: Option<Rgb>,
    /// A `<title>`, which is what screen readers and file managers show.
    pub title: Option<String>,
    /// Write every string as filled glyph outlines (the builtin face)
    /// instead of `<text>`, for a file that must look the same with no
    /// fonts installed. Text set in the builtin face is written as
    /// `<text>` naming Inter either way; the outlines are what a laser or a
    /// viewer with no fonts wants.
    pub text_outlines: bool,
}

impl Default for SvgOptions {
    fn default() -> Self {
        SvgOptions {
            decimals: 4,
            margin_mm: 5.0,
            background: None,
            title: None,
            text_outlines: false,
        }
    }
}

/// The page rectangle: the sheet if there is one, else the content plus a
/// margin.
pub fn page(d: &Drawing, margin_mm: f64) -> (P2, P2) {
    if let Some(s) = &d.sheet {
        return (P2::new(0.0, 0.0), P2::new(s.size.w_mm, s.size.h_mm));
    }
    match d.bounds() {
        Some((lo, hi)) => (
            P2::new(lo.x - margin_mm, lo.y - margin_mm),
            P2::new(hi.x + margin_mm, hi.y + margin_mm),
        ),
        None => (P2::new(0.0, 0.0), P2::new(10.0, 10.0)),
    }
}

/// Write `d` as an SVG document.
pub fn write(d: &Drawing, opts: &SvgOptions) -> String {
    let (lo, hi) = page(d, opts.margin_mm);
    let (w, h) = ((hi.x - lo.x).max(1e-6), (hi.y - lo.y).max(1e-6));
    let n = |v: f64| num(v, opts.decimals);
    // Model millimetres to SVG user units, y mirrored.
    let m = |p: P2| P2::new(p.x - lo.x, hi.y - p.y);

    let mut s = String::new();
    s.push_str("<?xml version=\"1.0\" encoding=\"UTF-8\"?>\n");
    let _ = write!(
        s,
        "<svg xmlns=\"http://www.w3.org/2000/svg\" version=\"1.1\" width=\"{}mm\" height=\"{}mm\" viewBox=\"0 0 {} {}\">\n",
        n(w),
        n(h),
        n(w),
        n(h)
    );
    if let Some(t) = &opts.title {
        let _ = write!(s, "  <title>{}</title>\n", escape(t));
    }
    if let Some(bg) = opts.background {
        let _ = write!(
            s,
            "  <rect x=\"0\" y=\"0\" width=\"{}\" height=\"{}\" fill=\"{}\"/>\n",
            n(w),
            n(h),
            bg.hex()
        );
    }

    let prims = d.flatten();
    let open_group = |s: &mut String, name: &str, base: &Resolved, first: bool| {
        // The first group of a layer carries the layer's id, which is what
        // the importer and an editor's layer list read; a later run of the
        // same layer (an ordered drawing) repeats the name as a class, since
        // an id may appear once.
        let ident = if first {
            format!(" id=\"{}\"", escape(name))
        } else {
            format!(" class=\"{}\"", escape(name))
        };
        let _ = write!(
            s,
            "  <g{ident} fill=\"none\" stroke=\"{}\" stroke-width=\"{}\" stroke-linecap=\"round\" stroke-linejoin=\"round\"{}>\n",
            base.color.hex(),
            n(base.weight_mm),
            dash_attr(&base.dash, opts.decimals)
        );
    };
    if d.ordered {
        // Paint order is the drawing's own: a white knock-out has to come
        // after what it covers, whatever layer either is on.
        let mut seen: Vec<String> = Vec::new();
        let mut open: Option<(String, Resolved)> = None;
        for p in &prims {
            let st = p.style();
            let same = open.as_ref().map(|(name, _)| name == &st.layer).unwrap_or(false);
            if !same {
                if open.is_some() {
                    s.push_str("  </g>\n");
                }
                let first = !seen.contains(&st.layer);
                open_group(&mut s, &st.layer, st, first);
                if first {
                    seen.push(st.layer.clone());
                }
                open = Some((st.layer.clone(), st.clone()));
            }
            let base = &open.as_ref().unwrap().1;
            emit(&mut s, p, base, &m, opts);
        }
        if open.is_some() {
            s.push_str("  </g>\n");
        }
        s.push_str("</svg>\n");
        return s;
    }

    // One <g> per layer, in the drawing's own layer order, so an editor shows
    // the same list an architect's template has.
    let mut names: Vec<String> = d
        .layers
        .iter()
        .filter(|l| l.print)
        .map(|l| l.name.clone())
        .collect();
    for p in &prims {
        let name = p.style().layer.clone();
        if !names.contains(&name) {
            names.push(name);
        }
    }
    for name in &names {
        let mine: Vec<&Prim> = prims
            .iter()
            .filter(|p| &p.style().layer == name)
            .collect();
        if mine.is_empty() {
            continue;
        }
        let base = mine[0].style().clone();
        open_group(&mut s, name, &base, true);
        for p in mine {
            emit(&mut s, p, &base, &m, opts);
        }
        s.push_str("  </g>\n");
    }
    s.push_str("</svg>\n");
    s
}

fn emit(s: &mut String, p: &Prim, base: &Resolved, m: &impl Fn(P2) -> P2, opts: &SvgOptions) {
    let n = |v: f64| num(v, opts.decimals);
    match p {
        Prim::Path {
            style,
            name,
            path,
            closed,
        } => {
            let d = path_data(path, *closed, m, opts.decimals);
            if d.is_empty() {
                return;
            }
            let id = if name.is_empty() {
                String::new()
            } else {
                format!(" id=\"{}\"", escape(name))
            };
            // `fill="none"` is repeated on every stroked path rather than left
            // to the group: SVG inherits presentation attributes, but a reader
            // that looks only at the element (ours does) must still be able to
            // tell a drawn outline from a filled shape.
            let _ = write!(
                s,
                "    <path{id} fill=\"none\" d=\"{d}\"{}/>\n",
                overrides(style, base, opts.decimals)
            );
        }
        Prim::Fill {
            style,
            name,
            outer,
            holes,
        } => {
            let mut d = loop_data(outer, m, opts.decimals);
            for h in holes {
                d.push(' ');
                d.push_str(&loop_data(h, m, opts.decimals));
            }
            if d.trim().is_empty() {
                return;
            }
            let id = if name.is_empty() {
                String::new()
            } else {
                format!(" id=\"{}\"", escape(name))
            };
            let _ = write!(
                s,
                "    <path{id} d=\"{d}\" fill=\"{}\" fill-rule=\"evenodd\" stroke=\"none\"/>\n",
                style.color.hex()
            );
        }
        Prim::Image {
            at, w, h, image, ..
        } => {
            // The PNG the picture came from, as it was; a picture with no
            // file behind it has nothing an SVG can carry without an
            // encoder, and is left out.
            let Some(png) = &image.png else { return };
            let q = m(P2::new(at.x, at.y + h));
            let _ = write!(
                s,
                "    <image x=\"{}\" y=\"{}\" width=\"{}\" height=\"{}\" preserveAspectRatio=\"none\" href=\"data:image/png;base64,{}\"/>\n",
                n(q.x),
                n(q.y),
                n(*w),
                n(*h),
                base64(png)
            );
        }
        Prim::Text {
            style,
            at,
            text,
            height_mm,
            rotation_deg,
            anchor,
            font,
        } => {
            if text.is_empty() {
                return;
            }
            if opts.text_outlines {
                let loops = crate::text::outlines(text, *height_mm, *at, *rotation_deg, *anchor, font);
                let mut d = String::new();
                for l in &loops {
                    if !d.is_empty() {
                        d.push(' ');
                    }
                    d.push_str(&loop_data(l, m, opts.decimals));
                }
                if !d.trim().is_empty() {
                    let _ = write!(
                        s,
                        "    <path d=\"{d}\" fill=\"{}\" fill-rule=\"nonzero\" stroke=\"none\"/>\n",
                        style.color.hex()
                    );
                }
                return;
            }
            let b = P2::new(
                at.x,
                at.y + crate::text::baseline_shift(*anchor, *height_mm),
            );
            let q = m(b);
            let anchor_attr = match anchor.h {
                HAlign::Left => "start",
                HAlign::Center => "middle",
                HAlign::Right => "end",
            };
            // SVG angles run clockwise on a y-down canvas; ours run
            // counter-clockwise on the model's y-up one.
            let rot = if rotation_deg.abs() > 1e-9 {
                format!(
                    " transform=\"rotate({} {} {})\"",
                    n(-rotation_deg),
                    n(q.x),
                    n(q.y)
                )
            } else {
                String::new()
            };
            let family = match font {
                FontRef::Builtin | FontRef::BuiltinBold => "Inter, system-ui, sans-serif".to_owned(),
                FontRef::Asset(a) => format!("{}, Inter, sans-serif", escape(a)),
                FontRef::Standard(f) => f.svg_family().to_owned(),
            };
            let weight = if font.bold() { " font-weight=\"bold\"" } else { "" };
            let slant = match font {
                FontRef::Standard(f) if f.italic() => " font-style=\"italic\"",
                _ => "",
            };
            let _ = write!(
                s,
                "    <text x=\"{}\" y=\"{}\" font-family=\"{}\"{weight}{slant} font-size=\"{}\" text-anchor=\"{}\" fill=\"{}\" stroke=\"none\"{}>{}</text>\n",
                n(q.x),
                n(q.y),
                family,
                n(crate::text::em_for_cap_height_in(*height_mm, font)),
                anchor_attr,
                style.color.hex(),
                rot,
                escape(text)
            );
        }
    }
}

/// Per-element attributes for anything that differs from its layer.
fn overrides(style: &Resolved, base: &Resolved, decimals: usize) -> String {
    let mut s = String::new();
    if (style.weight_mm - base.weight_mm).abs() > 1e-9 {
        let _ = write!(s, " stroke-width=\"{}\"", num(style.weight_mm, decimals));
    }
    if style.color != base.color {
        let _ = write!(s, " stroke=\"{}\"", style.color.hex());
    }
    if style.dash != base.dash {
        s.push_str(&dash_attr(&style.dash, decimals));
    }
    s
}

fn dash_attr(dash: &Dash, decimals: usize) -> String {
    let p = dash.pattern();
    if p.is_empty() {
        return String::new();
    }
    let list: Vec<String> = p.iter().map(|v| num(*v, decimals)).collect();
    format!(" stroke-dasharray=\"{}\"", list.join(","))
}

fn path_data(path: &Path, closed: bool, m: &impl Fn(P2) -> P2, decimals: usize) -> String {
    if path.segments.is_empty() {
        return String::new();
    }
    let mut d = String::new();
    let start = m(path.start);
    let _ = write!(d, "M {} {}", num(start.x, decimals), num(start.y, decimals));
    for seg in &path.segments {
        append_segment(&mut d, seg, m, decimals);
    }
    if closed {
        d.push_str(" Z");
    }
    d
}

fn loop_data(l: &cadpole_core::sketch::Loop, m: &impl Fn(P2) -> P2, decimals: usize) -> String {
    if l.segments.is_empty() {
        return String::new();
    }
    let mut d = String::new();
    let start = m(l.segments[0].start());
    let _ = write!(d, "M {} {}", num(start.x, decimals), num(start.y, decimals));
    for seg in &l.segments {
        append_segment(&mut d, seg, m, decimals);
    }
    d.push_str(" Z");
    d
}

fn append_segment(d: &mut String, seg: &Segment, m: &impl Fn(P2) -> P2, decimals: usize) {
    let n = |v: f64| num(v, decimals);
    match seg {
        Segment::Line { b, .. } => {
            let p = m(*b);
            let _ = write!(d, " L {} {}", n(p.x), n(p.y));
        }
        Segment::Arc { a, b, via } => {
            // Solve the arc in the *mirrored* frame so the sweep flag is right
            // for the coordinates actually written.
            let (ma, mb, mv) = (m(*a), m(*b), m(*via));
            match arc_params(ma, mb, mv) {
                Some((_, r, t0, t1)) if r.is_finite() && r > 0.0 => {
                    let large = if (t1 - t0).abs() > std::f64::consts::PI {
                        1
                    } else {
                        0
                    };
                    let sweep = if t1 > t0 { 1 } else { 0 };
                    let _ = write!(
                        d,
                        " A {} {} 0 {} {} {} {}",
                        n(r),
                        n(r),
                        large,
                        sweep,
                        n(mb.x),
                        n(mb.y)
                    );
                }
                // Three collinear points: a line is the honest answer.
                _ => {
                    let _ = write!(d, " L {} {}", n(mb.x), n(mb.y));
                }
            }
        }
        Segment::Bezier { b, ctrl, .. } => {
            let p = m(*b);
            match ctrl.len() {
                1 => {
                    let c = m(ctrl[0]);
                    let _ = write!(d, " Q {} {} {} {}", n(c.x), n(c.y), n(p.x), n(p.y));
                }
                _ if ctrl.len() >= 2 => {
                    let (c1, c2) = (m(ctrl[0]), m(ctrl[1]));
                    let _ = write!(
                        d,
                        " C {} {} {} {} {} {}",
                        n(c1.x),
                        n(c1.y),
                        n(c2.x),
                        n(c2.y),
                        n(p.x),
                        n(p.y)
                    );
                }
                _ => {
                    let _ = write!(d, " L {} {}", n(p.x), n(p.y));
                }
            }
        }
    }
}

/// A number with trailing zeros trimmed. Deterministic: the same drawing
/// always produces the same bytes.
fn num(v: f64, decimals: usize) -> String {
    if !v.is_finite() {
        return "0".to_owned();
    }
    let s = format!("{v:.decimals$}");
    let s = if s.contains('.') {
        s.trim_end_matches('0').trim_end_matches('.')
    } else {
        &s
    };
    if s.is_empty() || s == "-0" {
        "0".to_owned()
    } else {
        s.to_owned()
    }
}

/// Standard base64 with padding, for a data URI. Written here rather than
/// pulled in: it is twenty lines.
pub fn base64(bytes: &[u8]) -> String {
    const T: &[u8; 64] = b"ABCDEFGHIJKLMNOPQRSTUVWXYZabcdefghijklmnopqrstuvwxyz0123456789+/";
    let mut out = String::with_capacity(bytes.len().div_ceil(3) * 4);
    for chunk in bytes.chunks(3) {
        let b = [chunk[0], *chunk.get(1).unwrap_or(&0), *chunk.get(2).unwrap_or(&0)];
        let v = ((b[0] as u32) << 16) | ((b[1] as u32) << 8) | b[2] as u32;
        out.push(T[(v >> 18) as usize & 63] as char);
        out.push(T[(v >> 12) as usize & 63] as char);
        out.push(if chunk.len() > 1 { T[(v >> 6) as usize & 63] as char } else { '=' });
        out.push(if chunk.len() > 2 { T[v as usize & 63] as char } else { '=' });
    }
    out
}

fn escape(s: &str) -> String {
    let mut out = String::with_capacity(s.len());
    for c in s.chars() {
        match c {
            '&' => out.push_str("&amp;"),
            '<' => out.push_str("&lt;"),
            '>' => out.push_str("&gt;"),
            '"' => out.push_str("&quot;"),
            '\'' => out.push_str("&apos;"),
            _ => out.push(c),
        }
    }
    out
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::ir::{layer, Anchor};
    use cadpole_core::sketch::Loop;

    fn rect_drawing() -> Drawing {
        let mut d = Drawing::new();
        let l = d.layer(layer::VISIBLE);
        d.loop_stroke(l, &Loop::rect(P2::new(50.0, 25.0), 100.0, 50.0));
        d
    }

    #[test]
    fn the_root_states_millimetres_exactly() {
        let svg = write(
            &rect_drawing(),
            &SvgOptions {
                margin_mm: 0.0,
                ..Default::default()
            },
        );
        assert!(svg.contains("width=\"100mm\""), "{svg}");
        assert!(svg.contains("height=\"50mm\""), "{svg}");
        assert!(svg.contains("viewBox=\"0 0 100 50\""), "{svg}");
        assert!(svg.contains("<g id=\"M-VIEW\""));
    }

    #[test]
    fn a_circle_is_written_as_arcs_not_as_a_polyline() {
        let mut d = Drawing::new();
        let l = d.layer(layer::VISIBLE);
        d.loop_stroke(l, &Loop::circle(P2::new(20.0, 20.0), 10.0));
        let svg = write(
            &d,
            &SvgOptions {
                margin_mm: 0.0,
                ..Default::default()
            },
        );
        assert_eq!(svg.matches(" A ").count(), 2, "two half arcs: {svg}");
        assert!(!svg.contains(" L "), "no chords: {svg}");
    }

    #[test]
    fn output_is_byte_identical_across_runs() {
        let d = rect_drawing();
        assert_eq!(
            write(&d, &SvgOptions::default()),
            write(&d, &SvgOptions::default())
        );
    }

    #[test]
    fn text_is_escaped_and_anchored() {
        let mut d = Drawing::new();
        let l = d.layer(layer::TEXT);
        d.text(l, P2::new(10.0, 10.0), "A & B <x>", 2.5, Anchor::CENTER);
        let svg = write(&d, &SvgOptions::default());
        assert!(svg.contains("A &amp; B &lt;x&gt;"), "{svg}");
        assert!(svg.contains("text-anchor=\"middle\""));
    }

    #[test]
    fn hidden_layers_carry_a_dash_array() {
        let mut d = Drawing::new();
        let l = d.layer(layer::HIDDEN);
        d.polyline(l, &[P2::new(0.0, 0.0), P2::new(10.0, 0.0)], false);
        let svg = write(&d, &SvgOptions::default());
        assert!(svg.contains("stroke-dasharray=\"2,1\""), "{svg}");
    }

    #[test]
    fn base64_matches_the_standard_alphabet_and_padding() {
        assert_eq!(base64(b""), "");
        assert_eq!(base64(b"f"), "Zg==");
        assert_eq!(base64(b"fo"), "Zm8=");
        assert_eq!(base64(b"foo"), "Zm9v");
        assert_eq!(base64(b"foobar"), "Zm9vYmFy");
    }

    #[test]
    fn an_ordered_drawing_keeps_paint_order_and_a_standard_face_names_its_family() {
        let mut d = Drawing::new();
        d.ordered = true;
        let a = d.layer(layer::VISIBLE);
        let b = d.layer(layer::TEXT);
        d.polyline(a, &[P2::new(0.0, 0.0), P2::new(10.0, 0.0)], false);
        d.push(crate::ir::Item::Text(crate::ir::TextItem {
            style: crate::ir::Style::on(b),
            at: P2::new(5.0, 5.0),
            text: "A".into(),
            height_mm: 2.5,
            rotation_deg: 0.0,
            anchor: Anchor::CENTER,
            font: FontRef::Standard(crate::stdfont::StdFont::HelveticaBold),
            model_height: false,
        }));
        d.polyline(a, &[P2::new(0.0, 5.0), P2::new(10.0, 5.0)], false);
        let svg = write(&d, &SvgOptions::default());
        // Three groups: the second run of M-VIEW is a class, not a second id.
        assert_eq!(svg.matches("<g ").count(), 3, "{svg}");
        assert_eq!(svg.matches("id=\"M-VIEW\"").count(), 1);
        assert_eq!(svg.matches("class=\"M-VIEW\"").count(), 1);
        assert!(svg.contains("font-family=\"Helvetica, Arial, sans-serif\" font-weight=\"bold\""), "{svg}");
        let outlined = write(&d, &SvgOptions { text_outlines: true, ..Default::default() });
        assert!(!outlined.contains("<text"), "outlines instead of text");
        assert!(outlined.contains("fill-rule=\"nonzero\""));
    }

    #[test]
    fn numbers_are_trimmed_and_never_negative_zero() {
        assert_eq!(num(1.50000, 4), "1.5");
        assert_eq!(num(-0.00001, 4), "0");
        assert_eq!(num(12.0, 4), "12");
        assert_eq!(num(f64::NAN, 4), "0");
    }
}
