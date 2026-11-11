//! Where per-face colour lives in the feature tree.
//!
//! `paint(body, faces=…, color=…)` in a script records a feature of kind
//! `paint` whose `args` carry the body it painted, the face names and the
//! colour. Nothing is stored on the body itself, so:
//!
//! * paint is part of the document like any other feature and shows in the
//!   tree, with its own source span;
//! * the renderer and the 3MF exporter read it from the same place, so the
//!   viewport shows paint as it will print;
//! * a later feature can re-paint without anything having to be undone.
//!
//! This module is only the *contract*: the key names and how a colour is
//! spelled. `cadpole-script` writes it and `cadpole-engine` reads it, and neither
//! needs to know about the other.

use std::collections::BTreeMap;

use crate::context::Context;
use crate::fmath;

/// Feature kind a `paint(...)` call records.
pub const KIND: &str = "paint";
/// `args` key: the painted body's id string (`feature#n`).
pub const BODY: &str = "body";
/// `args` key: face names, comma separated, or `*` for the whole body.
pub const FACES: &str = "faces";
/// `args` key: `#RRGGBB`.
pub const COLOR: &str = "color";
/// `args` key: a pinned 1-based filament slot. Absent means "assign one".
pub const SLOT: &str = "slot";

/// The `args` map for one `paint(...)` feature.
pub fn args(body_id: &str, faces: &[String], color: [f64; 3], slot: Option<u32>) -> BTreeMap<String, String> {
    let mut args = BTreeMap::new();
    args.insert(BODY.to_owned(), body_id.to_owned());
    args.insert(FACES.to_owned(), if faces.is_empty() { "*".to_owned() } else { faces.join(",") });
    args.insert(COLOR.to_owned(), hex(color));
    if let Some(s) = slot {
        args.insert(SLOT.to_owned(), s.to_string());
    }
    args
}

/// One paint feature, parsed back out of the tree.
#[derive(Clone, Debug, PartialEq)]
pub struct Paint {
    pub feature: String,
    pub body_id: String,
    /// Empty means the whole body.
    pub faces: Vec<String>,
    pub color: [f64; 3],
    pub hex: String,
    pub slot: Option<u32>,
}

/// Every paint feature, in feature order — which is the order slots are
/// assigned in, so a script always gets the same filament numbering.
pub fn paints(ctx: &Context) -> Vec<Paint> {
    ctx.features
        .iter()
        .filter(|f| f.kind == KIND)
        .map(|f| {
            let hex_str = f.args.get(COLOR).cloned().unwrap_or_default();
            Paint {
                feature: f.id.0.clone(),
                body_id: f.args.get(BODY).cloned().unwrap_or_default(),
                faces: f
                    .args
                    .get(FACES)
                    .map(|s| {
                        s.split(',').map(str::trim).filter(|s| !s.is_empty() && *s != "*").map(str::to_owned).collect()
                    })
                    .unwrap_or_default(),
                color: unhex(&hex_str),
                hex: hex_str,
                slot: f.args.get(SLOT).and_then(|s| s.parse().ok()).filter(|s| *s > 0),
            }
        })
        .collect()
}

/// sRGB 0..1 — the encoding every `#rrggbb` byte is in — to display-linear
/// 0..1: the IEC 61966-2-1 transfer, a linear toe below 0.04045 and a 2.4
/// power above it. `Material::base_color` is linear, because the renderer
/// multiplies it by light, so every colour that arrives as bytes comes in
/// through here and goes out through [`linear_to_srgb`]. Reading a byte as
/// `byte / 255` and calling that linear was the mistake this replaces: a
/// filament red of sRGB saturation 0.70 rendered at 0.22, and every hex
/// colour in every script looked washed out.
pub fn srgb_to_linear(s: f64) -> f64 {
    let s = s.clamp(0.0, 1.0);
    if s <= 0.04045 { s / 12.92 } else { fmath::powf((s + 0.055) / 1.055, 2.4) }
}

/// Display-linear 0..1 to sRGB 0..1; the exact inverse of [`srgb_to_linear`].
pub fn linear_to_srgb(l: f64) -> f64 {
    let l = l.clamp(0.0, 1.0);
    if l <= 0.0031308 { 12.92 * l } else { 1.055 * fmath::powf(l, 1.0 / 2.4) - 0.055 }
}

/// Linear `[0..1; 3]` to `#RRGGBB`, sRGB-encoded: the bytes a slicer, an SVG
/// or a 3MF `displaycolor` expects. The exact reverse of [`unhex`], so six
/// digits written in a script come back out as the same six digits.
pub fn hex(c: [f64; 3]) -> String {
    let b = |v: f64| (linear_to_srgb(v) * 255.0).round() as u32;
    format!("#{:02X}{:02X}{:02X}", b(c[0]), b(c[1]), b(c[2]))
}

/// `#RRGGBB` (or `RRGGBB`), sRGB bytes, to linear `[0..1; 3]`. Unparseable
/// strings become a neutral grey rather than an error: a colour is never
/// worth failing a build.
pub fn unhex(s: &str) -> [f64; 3] {
    let s = s.trim().trim_start_matches('#');
    let v = if s.len() == 6 { u32::from_str_radix(s, 16).unwrap_or(0xC0C0C0) } else { 0xC0C0C0 };
    let channel = |shift: u32| srgb_to_linear(((v >> shift) & 0xff) as f64 / 255.0);
    [channel(16), channel(8), channel(0)]
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn a_colour_survives_the_round_trip_to_a_byte_and_back() {
        for c in [[0.0, 0.0, 0.0], [1.0, 1.0, 1.0], [0.2, 0.47, 0.75]] {
            let back = unhex(&hex(c));
            // Half a byte step in sRGB is up to 2.4 / 510 in linear at the
            // bright end, so that is the tolerance, not 1/255.
            assert!((0..3).all(|i| (back[i] - c[i]).abs() < 0.005), "{c:?} -> {} -> {back:?}", hex(c));
        }
        // Linear 0.47 is sRGB byte 182 and 0.75 is 225: brighter bytes than
        // `v * 255` gives, because sRGB spends its bytes where the eye can
        // tell them apart.
        assert_eq!(hex([0.0, 0.47, 0.75]), "#00B6E1");
        // Six digits in are the same six digits out.
        for h in ["#C98A6B", "#0078BF", "#010203", "#FEFDFC", "#000000", "#FFFFFF"] {
            assert_eq!(hex(unhex(h)), h);
        }
    }

    /// The transfer function itself, against the value everyone quotes:
    /// sRGB 0.5 is linear 0.214, not 0.5.
    #[test]
    fn the_srgb_transfer_is_the_standard_one() {
        assert!((srgb_to_linear(0.5) - 0.2140411).abs() < 1e-6, "{}", srgb_to_linear(0.5));
        assert!((linear_to_srgb(0.2140411) - 0.5).abs() < 1e-6);
        assert_eq!(srgb_to_linear(0.0), 0.0);
        assert_eq!(srgb_to_linear(1.0), 1.0);
        // The toe and the curve meet where the standard says they do.
        assert!((srgb_to_linear(0.04045) - srgb_to_linear(0.040451)).abs() < 1e-6);
        for s in [0.0, 0.01, 0.1, 0.3, 0.5, 0.9, 1.0] {
            assert!((linear_to_srgb(srgb_to_linear(s)) - s).abs() < 1e-12, "{s}");
        }
    }

    #[test]
    fn a_bad_colour_is_grey_not_an_error() {
        let grey = srgb_to_linear(0xC0 as f64 / 255.0);
        assert_eq!(unhex("nonsense"), [grey, grey, grey]);
    }

    #[test]
    fn args_spell_the_whole_body_case() {
        let a = args("extrude_1#0", &[], [1.0, 0.0, 0.0], None);
        assert_eq!(a[FACES], "*");
        assert!(!a.contains_key(SLOT));
        let b = args("extrude_1#0", &["face:1".into(), "face:2".into()], [1.0, 0.0, 0.0], Some(2));
        assert_eq!(b[FACES], "face:1,face:2");
        assert_eq!(b[SLOT], "2");
        assert_eq!(b[COLOR], "#FF0000");
    }
}
