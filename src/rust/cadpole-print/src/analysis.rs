//! Analysis views: the numbers the checks measured, painted over the surface
//! with a legend (docs/views.md, "Analysis views").
//!
//! The checks already return a value per triangle, so a view is a colour ramp
//! and nothing else — there is no second traversal of the geometry and no way
//! for the picture and the report to disagree. The colours go into
//! `Item::tri_colors`, which the rasterizer's shading uses instead of the
//! material's base colour, and the legend is drawn into the finished image.
//!
//! The ramps are **anchored on the threshold**, not on the data range: the
//! question is "is this wall thin enough to matter", and a ramp that rescales
//! itself makes the thinnest wall red whether it is 0.3 mm or 3 mm.

use cadpole_core::render::Image;

use crate::{OverhangReport, Profile, ThicknessReport, num};

/// Which field to paint.
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub enum Analysis {
    Thickness,
    Overhang,
}

impl Analysis {
    pub fn parse(s: &str) -> Result<Analysis, String> {
        match s.to_ascii_lowercase().as_str() {
            "thickness" | "wall" => Ok(Analysis::Thickness),
            "overhang" | "overhangs" => Ok(Analysis::Overhang),
            other => Err(format!(
                "unknown analysis {other:?}; \"thickness\" and \"overhang\" are implemented \
                 (draft, curvature, zebra, height, distance and interference are not yet)"
            )),
        }
    }
    pub fn as_str(self) -> &'static str {
        match self {
            Analysis::Thickness => "thickness",
            Analysis::Overhang => "overhang",
        }
    }
}

/// A colour bar with labelled stops, drawn into the image so the picture is
/// readable on its own.
#[derive(Clone, Debug)]
pub struct Legend {
    pub title: String,
    /// Left to right: `(label, display-linear colour)`.
    pub stops: Vec<(String, [f32; 3])>,
}

/// Red below the minimum, amber at it, green and then blue above: the eye
/// reads "red is a problem" before it reads the numbers.
const THIN: [f32; 3] = [0.72, 0.06, 0.06];
const NEAR: [f32; 3] = [0.92, 0.66, 0.10];
const FINE: [f32; 3] = [0.22, 0.62, 0.28];
const THICK: [f32; 3] = [0.16, 0.34, 0.70];
/// Vertical-enough to print, for the overhang ramp.
const SAFE: [f32; 3] = [0.55, 0.58, 0.60];

/// Colour per triangle for the wall-thickness field.
///
/// The ramp runs `0 → min_wall → 2 × min_wall → 3 × min_wall and up`, so red
/// means "thinner than the perimeters that have to fit here" at any scale.
/// Triangles with no measurement (nothing opposite them) are drawn as thick.
pub fn thickness_colors(r: &ThicknessReport, profile: &Profile) -> Vec<[f32; 3]> {
    let w = profile.min_wall.max(1e-6) as f32;
    r.per_triangle
        .iter()
        .map(|&t| {
            if !t.is_finite() {
                return THICK;
            }
            let x = t / w;
            if x <= 1.0 {
                mix(THIN, NEAR, x)
            } else if x <= 2.0 {
                mix(NEAR, FINE, x - 1.0)
            } else {
                mix(FINE, THICK, (x - 2.0).min(1.0))
            }
        })
        .collect()
}

pub fn thickness_legend(profile: &Profile) -> Legend {
    let w = profile.min_wall;
    Legend {
        title: format!("wall thickness, mm (minimum {})", num(w)),
        stops: vec![
            ("0".to_owned(), THIN),
            (num(w), NEAR),
            (num(w * 2.0), FINE),
            (format!("{}+", num(w * 3.0)), THICK),
        ],
    }
}

/// Colour per triangle for the overhang field: grey while the surface is
/// vertical enough to print, amber at the threshold, red at a flat ceiling.
pub fn overhang_colors(r: &OverhangReport, profile: &Profile) -> Vec<[f32; 3]> {
    let th = profile.max_overhang_degrees.clamp(1.0, 89.0) as f32;
    r.per_triangle
        .iter()
        .map(|&d| {
            if d <= th {
                mix(SAFE, NEAR, (d / th).clamp(0.0, 1.0) * 0.5)
            } else {
                mix(NEAR, THIN, ((d - th) / (90.0 - th)).clamp(0.0, 1.0))
            }
        })
        .collect()
}

pub fn overhang_legend(profile: &Profile) -> Legend {
    let th = profile.max_overhang_degrees;
    Legend {
        title: format!("overhang from vertical, degrees (support past {})", num(th)),
        stops: vec![
            ("0".to_owned(), SAFE),
            (num(th), NEAR),
            (num((th + 90.0) / 2.0), mix(NEAR, THIN, 0.5)),
            ("90".to_owned(), THIN),
        ],
    }
}

fn mix(a: [f32; 3], b: [f32; 3], t: f32) -> [f32; 3] {
    let t = t.clamp(0.0, 1.0);
    [a[0] + (b[0] - a[0]) * t, a[1] + (b[1] - a[1]) * t, a[2] + (b[2] - a[2]) * t]
}

/// Draw the legend into the bottom-left of a finished image: a gradient bar
/// with a tick and a label under each stop, on a panel dark enough to read on
/// any background.
pub fn draw_legend(img: &mut Image, legend: &Legend) {
    const PAD: u32 = 8;
    const BAR_H: u32 = 10;
    let scale = if img.width >= 900 { 2 } else { 1 };
    let char_w = 6 * scale;
    let text_h = 7 * scale;
    let bar_w = (img.width.saturating_sub(PAD * 4)).min(260 * scale).max(60);
    let panel_w = bar_w + PAD * 2;
    let panel_h = BAR_H + text_h * 2 + PAD * 3;
    if img.width < panel_w + PAD || img.height < panel_h + PAD {
        return; // a thumbnail has no room for a legend; the report still has the numbers
    }
    let x0 = PAD;
    let y0 = img.height - panel_h - PAD;

    // Panel.
    for y in y0..y0 + panel_h {
        for x in x0..x0 + panel_w {
            let under = img.rgba[(((y * img.width) + x) * 4) as usize..][..3].to_vec();
            let blend = |c: u8| ((c as u32 * 25 + 18 * 75) / 100) as u8;
            img.put(x, y, [blend(under[0]), blend(under[1]), blend(under[2]), 255]);
        }
    }
    img.text(x0 + PAD, y0 + PAD, &legend.title, scale, [235, 235, 235, 255]);

    // The bar, interpolating between the stops.
    let bar_y = y0 + PAD + text_h + PAD / 2;
    let n = legend.stops.len().max(2) - 1;
    for i in 0..bar_w {
        let t = i as f32 / (bar_w - 1).max(1) as f32 * n as f32;
        let k = (t.floor() as usize).min(n - 1);
        let c = mix(legend.stops[k].1, legend.stops[k + 1].1, t - k as f32);
        let px = [srgb8(c[0]), srgb8(c[1]), srgb8(c[2]), 255];
        for y in bar_y..bar_y + BAR_H {
            img.put(x0 + PAD + i, y, px);
        }
    }

    // Labels under the stops, nudged so the first and last stay inside.
    let label_y = bar_y + BAR_H + PAD / 2;
    for (i, (label, _)) in legend.stops.iter().enumerate() {
        let at = x0 + PAD + (bar_w - 1) * i as u32 / n as u32;
        let w = label.chars().count() as u32 * char_w;
        let lx = at.saturating_sub(w / 2).clamp(x0 + 2, x0 + panel_w - w - 2);
        img.text(lx, label_y, label, scale, [225, 225, 225, 255]);
    }
}

/// Display-linear 0..1 to an sRGB byte.
fn srgb8(v: f32) -> u8 {
    let v = v.clamp(0.0, 1.0);
    let s = if v <= 0.003_130_8 { v * 12.92 } else { 1.055 * v.powf(1.0 / 2.4) - 0.055 };
    (s * 255.0 + 0.5) as u8
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::testing::*;
    use crate::{Profile, overhang, thickness};

    fn close(a: [f32; 3], b: [f32; 3]) -> bool {
        (0..3).all(|i| (a[i] - b[i]).abs() < 1e-5)
    }

    #[test]
    fn the_thickness_ramp_is_anchored_on_the_minimum() {
        let p = Profile::default(); // 0.8 mm
        let thin = ThicknessReport { per_triangle: vec![0.4, 0.8, 1.6, 2.4, f32::INFINITY], ..Default::default() };
        let c = thickness_colors(&thin, &p);
        assert!(close(c[1], NEAR), "exactly the minimum is the amber anchor: {:?}", c[1]);
        assert!(close(c[2], FINE), "twice the minimum is green: {:?}", c[2]);
        assert!(close(c[3], THICK), "{:?}", c[3]);
        assert!(close(c[4], THICK), "unmeasured reads as thick, not as a problem");
        assert!(c[0][0] > c[0][1], "half the minimum is red-dominant: {:?}", c[0]);
    }

    #[test]
    fn the_overhang_ramp_turns_at_the_threshold() {
        let p = Profile::default(); // 45 deg
        let r = OverhangReport { per_triangle: vec![0.0, 45.0, 90.0], ..Default::default() };
        let c = overhang_colors(&r, &p);
        assert!(close(c[0], SAFE), "{:?}", c[0]);
        assert!(close(c[2], THIN), "a flat ceiling is as red as it gets: {:?}", c[2]);
        assert!(c[1][0] > c[0][0], "the threshold is already warmer than vertical");
    }

    #[test]
    fn a_real_thin_shell_comes_out_red_and_a_thick_one_does_not() {
        let p = Profile::default();
        let thin = thickness::check(&hollow_box(20.0, 0.6), &p, &mut Vec::new());
        let thick = thickness::check(&hollow_box(20.0, 2.4), &p, &mut Vec::new());
        let red = |c: &[[f32; 3]]| c.iter().all(|c| c[0] > c[2]);
        assert!(red(&thickness_colors(&thin, &p)), "0.6 mm walls read red");
        assert!(!red(&thickness_colors(&thick, &p)), "2.4 mm walls do not");
    }

    #[test]
    fn the_field_has_one_colour_per_triangle() {
        let m = leaning_prism(10.0, 10.0, 60.0, 10.0);
        let r = overhang::check(&m, &Profile::default(), &mut Vec::new());
        assert_eq!(overhang_colors(&r, &Profile::default()).len(), m.triangle_count());
    }

    #[test]
    fn the_legend_draws_inside_the_image_and_skips_a_thumbnail() {
        let mut img = Image::new(640, 400, [0.1, 0.1, 0.1]);
        let before = img.rgba.clone();
        draw_legend(&mut img, &thickness_legend(&Profile::default()));
        assert_ne!(img.rgba, before, "something was drawn");
        // The top half of the image is untouched.
        let half = (img.width * (img.height / 2) * 4) as usize;
        assert_eq!(img.rgba[..half], before[..half]);

        let mut tiny = Image::new(64, 48, [0.1, 0.1, 0.1]);
        let before = tiny.rgba.clone();
        draw_legend(&mut tiny, &thickness_legend(&Profile::default()));
        assert_eq!(tiny.rgba, before, "no room, so no legend rather than a mess");
    }

    #[test]
    fn analysis_names_parse_and_bad_ones_say_what_is_available() {
        assert_eq!(Analysis::parse("thickness").unwrap(), Analysis::Thickness);
        assert_eq!(Analysis::parse("Overhang").unwrap(), Analysis::Overhang);
        let e = Analysis::parse("zebra").unwrap_err();
        assert!(e.contains("thickness") && e.contains("zebra"), "{e}");
    }
}
