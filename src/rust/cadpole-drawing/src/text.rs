//! Text on a drawing, shaped by `cadpole-font`.
//!
//! Drawings measure type by **cap height** — "2.5 mm text" means the capitals
//! are 2.5 mm tall — while a font's natural size is the em. This module is the
//! conversion, plus the two things the rest of the pipeline asks of a string:
//! how wide it is (so a dimension can decide whether its text fits between the
//! witness lines) and what its outlines are (so a PDF can fill them).
//!
//! Both answers come from the same shaper the `text()` builtin uses, so a
//! label on a drawing and an engraved label on a part are set identically:
//! real kerning, real ligatures, and byte-identical results on every platform,
//! which is what lets the placement tests assert on exact numbers.

use std::sync::OnceLock;

use cadpole_core::sketch::{Loop, P2};

use crate::ir::{Anchor, FontRef, HAlign, VAlign};

/// Shaping to glyph ids and writing a font subset, for a PDF writer that
/// embeds the bundled face.
pub use cadpole_font::embed;

/// The bundled face's bytes.
pub fn builtin() -> &'static [u8] {
    cadpole_font::BUILTIN_SANS
}

/// Cap height of the bundled face as a fraction of the em, measured once.
fn cap_ratio() -> f64 {
    static RATIO: OnceLock<f64> = OnceLock::new();
    *RATIO.get_or_init(|| {
        let opts = cadpole_font::Options {
            size: 1000.0,
            ..Default::default()
        };
        match cadpole_font::text(builtin(), "H", &opts) {
            Ok(t) if t.cap_height > 0.0 => t.cap_height / 1000.0,
            // A face with no OS/2 cap height: 0.7 em is the usual value and is
            // only ever a fallback.
            _ => 0.7,
        }
    })
}

/// Em size that gives capitals `height_mm` tall.
pub fn em_for_cap_height(height_mm: f64) -> f64 {
    let r = cap_ratio();
    if r > 1e-6 {
        height_mm / r
    } else {
        height_mm
    }
}

/// The `wght` the bundled face is set at for `font`.
pub fn builtin_weight(font: &FontRef) -> f64 {
    if *font == FontRef::BuiltinBold {
        700.0
    } else {
        400.0
    }
}

fn shape(text: &str, height_mm: f64, align: &str, baseline: &str, weight: f64) -> Option<cadpole_font::Text> {
    if text.is_empty() || !(height_mm.is_finite() && height_mm > 1e-9) {
        return None;
    }
    let opts = cadpole_font::Options {
        size: em_for_cap_height(height_mm),
        align: align.to_owned(),
        baseline: baseline.to_owned(),
        weight,
        ..Default::default()
    };
    cadpole_font::text(builtin(), text, &opts).ok()
}

/// Advance width of `text` set in the builtin face at cap height
/// `height_mm`, in the same units.
///
/// Used by dimension placement to decide whether a value fits between its
/// witness lines, and by [`crate::ir::Drawing::bounds`].
pub fn width(text: &str, height_mm: f64) -> f64 {
    shape(text, height_mm, "left", "alphabetic", 400.0)
        .map(|t| t.width)
        .unwrap_or(0.0)
}

/// Advance width of `text` at cap height `height_mm` in `font`: the shaper
/// for the builtin face, the Adobe metrics for a standard one. The
/// widest line counts when the string has several.
pub fn width_in(text: &str, height_mm: f64, font: &FontRef) -> f64 {
    match font {
        FontRef::Standard(f) => {
            if !(height_mm.is_finite() && height_mm > 0.0) {
                return 0.0;
            }
            let em = height_mm / f.cap_ratio();
            text.split('\n')
                .map(|line| f.advance_em(line) * em)
                .fold(0.0, f64::max)
        }
        FontRef::BuiltinBold => shape(text, height_mm, "left", "alphabetic", 700.0)
            .map(|t| t.width)
            .unwrap_or(0.0),
        _ => width(text, height_mm),
    }
}

/// Em size that gives capitals `height_mm` tall in `font`.
pub fn em_for_cap_height_in(height_mm: f64, font: &FontRef) -> f64 {
    match font {
        FontRef::Standard(f) => height_mm / f.cap_ratio(),
        _ => em_for_cap_height(height_mm),
    }
}

/// Greedy word wrap of `text` to `max_width_mm` at cap height `height_mm`
/// in `font`, honouring the newlines it already has. A word longer than the
/// width stands on its own line rather than being broken.
pub fn wrap(text: &str, max_width_mm: f64, height_mm: f64, font: &FontRef) -> Vec<String> {
    let mut out = Vec::new();
    for para in text.split('\n') {
        let mut line = String::new();
        for word in para.split(' ').filter(|w| !w.is_empty()) {
            let candidate = if line.is_empty() {
                word.to_owned()
            } else {
                format!("{line} {word}")
            };
            if line.is_empty() || width_in(&candidate, height_mm, font) <= max_width_mm {
                line = candidate;
            } else {
                out.push(std::mem::take(&mut line));
                line = word.to_owned();
            }
        }
        out.push(line);
    }
    out
}

/// The outlines of `text`, placed at `at` with `anchor` and rotated by
/// `rotation_deg`, ready to be filled.
///
/// Returns nothing for an empty string or a face that cannot set it, which is
/// the right behaviour for an exporter: a missing glyph must not fail a whole
/// drawing.
pub fn outlines(
    text: &str,
    height_mm: f64,
    at: P2,
    rotation_deg: f64,
    anchor: Anchor,
    font: &FontRef,
) -> Vec<Loop> {
    // Asset faces resolve in the host; the IR records the intent.
    let align = match anchor.h {
        HAlign::Left => "left",
        HAlign::Center => "center",
        HAlign::Right => "right",
    };
    let baseline = match anchor.v {
        VAlign::Baseline | VAlign::Bottom => "alphabetic",
        VAlign::Middle => "middle",
        VAlign::Top => "top",
    };
    let Some(t) = shape(text, height_mm, align, baseline, builtin_weight(font)) else {
        return Vec::new();
    };
    let a = rotation_deg.to_radians();
    let (c, s) = (a.cos(), a.sin());
    let place = |p: P2| P2::new(at.x + p.x * c - p.y * s, at.y + p.x * s + p.y * c);
    t.loops
        .into_iter()
        .map(|(_, l)| Loop {
            segments: l
                .segments
                .iter()
                .map(|sg| crate::ir::xform_segment_with(sg, place))
                .collect(),
        })
        .collect()
}

/// Baseline offset for an anchor, as a fraction of the cap height. Writers
/// that place native text (SVG `<text>`, DXF `TEXT`) use this so their output
/// lands where the outlines would have.
pub fn baseline_shift(anchor: Anchor, height_mm: f64) -> f64 {
    match anchor.v {
        VAlign::Baseline | VAlign::Bottom => 0.0,
        VAlign::Middle => -height_mm / 2.0,
        VAlign::Top => -height_mm,
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn cap_height_is_what_was_asked_for() {
        let em = em_for_cap_height(2.5);
        let t = cadpole_font::text(
            builtin(),
            "H",
            &cadpole_font::Options {
                size: em,
                ..Default::default()
            },
        )
        .expect("builtin face sets an H");
        assert!(
            (t.cap_height - 2.5).abs() < 0.01,
            "cap height {}",
            t.cap_height
        );
    }

    #[test]
    fn width_scales_with_height_and_is_deterministic() {
        let a = width("12'-6\"", 2.5);
        let b = width("12'-6\"", 5.0);
        assert!(a > 0.0);
        assert!((b / a - 2.0).abs() < 1e-9, "{a} {b}");
        assert_eq!(
            width("12'-6\"", 2.5).to_bits(),
            a.to_bits(),
            "same input, same bits"
        );
        assert_eq!(width("", 2.5), 0.0);
    }

    #[test]
    fn outlines_land_where_the_anchor_says() {
        let at = P2::new(100.0, 50.0);
        let ls = outlines("CADPOLE", 4.0, at, 0.0, Anchor::CENTER, &FontRef::Builtin);
        assert!(!ls.is_empty());
        let w = width("CADPOLE", 4.0);
        let xs: Vec<f64> = ls
            .iter()
            .flat_map(|l| l.sample().into_iter().map(|p| p.x))
            .collect();
        let lo = xs.iter().cloned().fold(f64::MAX, f64::min);
        let hi = xs.iter().cloned().fold(f64::MIN, f64::max);
        // Centred: the ink straddles the anchor, inside the advance width.
        assert!(
            (lo + hi) / 2.0 > at.x - w * 0.15 && (lo + hi) / 2.0 < at.x + w * 0.15,
            "{lo} {hi}"
        );
        assert!(outlines("", 4.0, at, 0.0, Anchor::CENTER, &FontRef::Builtin).is_empty());
    }

    #[test]
    fn standard_faces_measure_from_the_afm_table_and_wrap_by_it() {
        let helv = FontRef::Standard(crate::stdfont::StdFont::Helvetica);
        // "Hello" is 2.278 em; at a 2.5 mm cap height the em is 2.5 / 0.718.
        let w = width_in("Hello", 2.5, &helv);
        assert!((w - 2.278 * 2.5 / 0.718).abs() < 1e-9, "{w}");
        assert!(width_in("Hello", 2.5, &FontRef::Standard(crate::stdfont::StdFont::HelveticaBold)) > w);
        assert_eq!(width_in("", 2.5, &helv), 0.0);
        // Wrapping breaks at spaces and never inside a word.
        let lines = wrap("the quick brown fox jumps", 20.0, 2.5, &helv);
        assert!(lines.len() >= 2, "{lines:?}");
        assert_eq!(lines.join(" "), "the quick brown fox jumps");
        for l in &lines {
            assert!(width_in(l, 2.5, &helv) <= 20.0 || !l.contains(' '), "{l}");
        }
        assert_eq!(wrap("a\nb", 100.0, 2.5, &helv), vec!["a", "b"]);
    }

    #[test]
    fn rotation_turns_the_ink() {
        let flat = outlines(
            "I",
            4.0,
            P2::new(0.0, 0.0),
            0.0,
            Anchor::LEFT,
            &FontRef::Builtin,
        );
        let up = outlines(
            "I",
            4.0,
            P2::new(0.0, 0.0),
            90.0,
            Anchor::LEFT,
            &FontRef::Builtin,
        );
        let span = |ls: &[Loop], vertical: bool| {
            let vs: Vec<f64> = ls
                .iter()
                .flat_map(|l| {
                    l.sample()
                        .into_iter()
                        .map(|p| if vertical { p.y } else { p.x })
                })
                .collect();
            vs.iter().cloned().fold(f64::MIN, f64::max)
                - vs.iter().cloned().fold(f64::MAX, f64::min)
        };
        assert!(
            span(&flat, true) > span(&flat, false),
            "an upright I is tall"
        );
        assert!(span(&up, false) > span(&up, true), "a rotated I is wide");
    }
}
