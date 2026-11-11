//! Named material presets: one line per material a 3-D print, a shop or a
//! mock-up is actually made of, so a script can say `preset="brass"` instead
//! of guessing five PBR numbers.
//!
//! Every preset is a complete [`Material`] within the documented ranges
//! (roughness and metallic 0..1, opacity 0..1, ior 1..2.5, emissive >= 0,
//! transmission 0..1, anisotropy 0..1, scatter_g -1..1, the two distances in
//! mm and >= 0). Scripts apply a preset first and then override individual
//! fields.
//!
//! The last five columns are the print-material terms the path tracer reads
//! (docs/design.md); the rasterizer ignores them, so a preset looks the
//! same under `--backend raster` as it always did. What they buy under
//! `--backend path`: `transmission` makes a clear part refract instead of
//! merely blending, `attenuation` makes a thick section darker and more
//! saturated than a thin one (Beer-Lambert), `scatter` turns a clear resin
//! into a milky filament, and `aniso` stretches the specular highlight along
//! the print's layer direction so an FDM part reads as printed rather than
//! injection moulded (on the metals the same term reads as a brushed finish).

use crate::context::Material;

struct Preset {
    name: &'static str,
    /// Linear RGB 0..1.
    base_color: [f64; 3],
    roughness: f64,
    metallic: f64,
    texture: &'static str,
    /// Texture repeat in mm.
    texture_scale: f64,
    opacity: f64,
    ior: f64,
    emissive: [f64; 3],
    /// Refracting fraction, 0..1.
    transmission: f64,
    /// Beer-Lambert distance in mm; 0 = no absorption.
    attenuation_distance: f64,
    /// Scattering mean free path in mm; 0 = clear.
    scatter_distance: f64,
    scatter_g: f64,
    anisotropy: f64,
}

#[rustfmt::skip]
const PRESETS: [Preset; 18] = [
    // name               base_color                rough  metal  texture  scale  opacity  ior    emissive             trans  atten  scatt  g     aniso
    p("pla_matte",        [0.72, 0.74, 0.78],       0.60,  0.0,   "",      10.0,  1.00,    1.50,  [0.0; 3],            0.00,    0.0,   0.0,  0.0,  0.55),
    p("pla_silk",         [0.76, 0.78, 0.82],       0.25,  0.0,   "",      10.0,  1.00,    1.50,  [0.0; 3],            0.00,    0.0,   0.0,  0.0,  0.75),
    p("pla_glow",         [0.78, 0.86, 0.76],       0.55,  0.0,   "",      10.0,  1.00,    1.50,  [0.10, 0.55, 0.35],  0.00,    0.0,   0.0,  0.0,  0.50),
    p("petg_glossy",      [0.66, 0.74, 0.80],       0.20,  0.0,   "",      10.0,  1.00,    1.57,  [0.0; 3],            0.00,    0.0,   0.0,  0.0,  0.60),
    p("petg_clear",       [0.86, 0.93, 0.90],       0.08,  0.0,   "",      10.0,  0.12,    1.57,  [0.0; 3],            1.00,   55.0,   0.0,  0.0,  0.15),
    p("petg_translucent", [0.80, 0.88, 0.84],       0.30,  0.0,   "",      10.0,  0.35,    1.57,  [0.0; 3],            1.00,   14.0,   3.0,  0.4,  0.50),
    p("abs",              [0.68, 0.68, 0.70],       0.50,  0.0,   "",      10.0,  1.00,    1.52,  [0.0; 3],            0.00,    0.0,   0.0,  0.0,  0.45),
    p("tpu_rubber",       [0.10, 0.10, 0.11],       0.95,  0.0,   "",      10.0,  1.00,    1.50,  [0.0; 3],            0.00,    0.0,   0.0,  0.0,  0.25),
    p("resin_clear",      [0.94, 0.94, 0.92],       0.06,  0.0,   "",      10.0,  0.12,    1.52,  [0.0; 3],            1.00,   45.0,   0.0,  0.0,  0.00),
    p("resin_frosted",    [0.92, 0.92, 0.90],       0.45,  0.0,   "",      10.0,  0.50,    1.52,  [0.0; 3],            1.00,   25.0,   6.0,  0.2,  0.00),
    p("glass",            [0.92, 0.96, 0.98],       0.05,  0.0,   "",      10.0,  0.15,    1.50,  [0.0; 3],            1.00,  220.0,   0.0,  0.0,  0.00),
    p("acrylic_clear",    [0.94, 0.95, 0.96],       0.08,  0.0,   "",      10.0,  0.25,    1.49,  [0.0; 3],            1.00,  120.0,   0.0,  0.0,  0.00),
    p("acrylic_frosted",  [0.90, 0.92, 0.94],       0.55,  0.0,   "",      10.0,  0.60,    1.49,  [0.0; 3],            1.00,   40.0,   5.0,  0.0,  0.00),
    p("brass",            [0.89, 0.72, 0.35],       0.35,  1.0,   "",      10.0,  1.00,    1.50,  [0.0; 3],            0.00,    0.0,   0.0,  0.0,  0.25),
    p("aluminum",         [0.91, 0.92, 0.92],       0.40,  1.0,   "",      10.0,  1.00,    1.50,  [0.0; 3],            0.00,    0.0,   0.0,  0.0,  0.25),
    p("steel",            [0.77, 0.78, 0.78],       0.30,  1.0,   "",      10.0,  1.00,    1.50,  [0.0; 3],            0.00,    0.0,   0.0,  0.0,  0.25),
    p("wood_oak",         [0.62, 0.45, 0.28],       0.70,  0.0,   "wood",  40.0,  1.00,    1.50,  [0.0; 3],            0.00,    0.0,   0.0,  0.0,  0.00),
    p("led",              [0.90, 0.90, 0.88],       0.35,  0.0,   "",      10.0,  1.00,    1.50,  [4.0, 4.0, 3.0],     0.00,    0.0,   0.0,  0.0,  0.00),
];

/// Preset names in documentation order, for script validation, diagnostics
/// and `docs/starlark-api.md`.
pub fn names() -> &'static [&'static str] {
    const NAMES: [&str; PRESETS.len()] = [
        "pla_matte",
        "pla_silk",
        "pla_glow",
        "petg_glossy",
        "petg_clear",
        "petg_translucent",
        "abs",
        "tpu_rubber",
        "resin_clear",
        "resin_frosted",
        "glass",
        "acrylic_clear",
        "acrylic_frosted",
        "brass",
        "aluminum",
        "steel",
        "wood_oak",
        "led",
    ];
    &NAMES
}

/// The full [`Material`] for a preset name, or `None` if there is no such
/// preset (the caller turns that into a diagnostic listing [`names`]).
pub fn lookup(name: &str) -> Option<Material> {
    PRESETS.iter().find(|p| p.name == name).map(|p| Material {
        name: p.name.to_owned(),
        base_color: p.base_color,
        roughness: p.roughness,
        metallic: p.metallic,
        texture: p.texture.to_owned(),
        texture_scale: p.texture_scale,
        opacity: p.opacity,
        ior: p.ior,
        emissive: p.emissive,
        transmission: p.transmission,
        attenuation_distance: p.attenuation_distance,
        scatter_distance: p.scatter_distance,
        scatter_g: p.scatter_g,
        anisotropy: p.anisotropy,
        edge_break: 0.0,
        texture_depth: 0.0,
        shadow: true,
    })
}

#[allow(clippy::too_many_arguments)]
const fn p(
    name: &'static str,
    base_color: [f64; 3],
    roughness: f64,
    metallic: f64,
    texture: &'static str,
    texture_scale: f64,
    opacity: f64,
    ior: f64,
    emissive: [f64; 3],
    transmission: f64,
    attenuation_distance: f64,
    scatter_distance: f64,
    scatter_g: f64,
    anisotropy: f64,
) -> Preset {
    Preset {
        name,
        base_color,
        roughness,
        metallic,
        texture,
        texture_scale,
        opacity,
        ior,
        emissive,
        transmission,
        attenuation_distance,
        scatter_distance,
        scatter_g,
        anisotropy,
    }
}

#[cfg(test)]
mod tests {
    use super::super::testing::{mean_luma, uv_sphere};
    use super::super::{Camera, Item, RenderOptions, render};
    use super::*;

    #[test]
    fn every_preset_round_trips_and_is_in_range() {
        assert_eq!(names().len(), PRESETS.len());
        for (i, name) in names().iter().enumerate() {
            let m = lookup(name).unwrap_or_else(|| panic!("{name} is listed but missing"));
            assert_eq!(&m.name, name, "preset {i} labels itself {:?}", m.name);
            assert_eq!(PRESETS[i].name, *name, "names() is out of order at {i}");
            assert!(m.base_color.iter().all(|c| (0.0..=1.0).contains(c)), "{name} base_color {:?}", m.base_color);
            assert!((0.0..=1.0).contains(&m.roughness), "{name} roughness {}", m.roughness);
            assert!((0.0..=1.0).contains(&m.metallic), "{name} metallic {}", m.metallic);
            assert!((0.0..=1.0).contains(&m.opacity), "{name} opacity {}", m.opacity);
            assert!((1.0..=2.5).contains(&m.ior), "{name} ior {}", m.ior);
            assert!(m.emissive.iter().all(|c| *c >= 0.0), "{name} emissive {:?}", m.emissive);
            assert!(m.texture_scale > 0.0, "{name} texture_scale {}", m.texture_scale);
            assert!(super::super::texture_kind(&m.texture).is_some(), "{name} has texture {:?}", m.texture);
            assert!((0.0..=1.0).contains(&m.transmission), "{name} transmission {}", m.transmission);
            assert!(m.attenuation_distance >= 0.0, "{name} attenuation {}", m.attenuation_distance);
            assert!(m.scatter_distance >= 0.0, "{name} scatter {}", m.scatter_distance);
            assert!((-1.0..=1.0).contains(&m.scatter_g), "{name} scatter_g {}", m.scatter_g);
            assert!((0.0..=1.0).contains(&m.anisotropy), "{name} anisotropy {}", m.anisotropy);
            // A refracting preset needs a finite absorption distance, or a
            // thick section would read exactly like a thin one.
            if m.transmission > 0.0 {
                assert!(m.attenuation_distance > 0.0, "{name} refracts but never absorbs");
                assert!(m.opacity < 0.999, "{name} refracts but is opaque to the rasterizer");
            }
            // Scattering without refraction would be invisible: the walk only
            // runs inside a solid the ray actually entered.
            if m.scatter_distance > 0.0 {
                assert!(m.transmission > 0.0, "{name} scatters but never transmits");
            }
        }
        assert!(lookup("nope").is_none());
        assert!(lookup("").is_none());
    }

    #[test]
    fn presets_render_with_led_brightest_and_rubber_darkest() {
        let (mesh, edges) = uv_sphere(24, 48);
        let (lo, hi) = mesh.bbox().unwrap();
        let cam = Camera::preset("iso", lo, hi, 0.1);
        let opts = RenderOptions {
            width: 96,
            height: 72,
            draw_edges: false,
            draw_grid: false,
            samples: 1,
            ..Default::default()
        };
        let mut opaque: Vec<(&str, f32)> = Vec::new();
        for name in names() {
            let m = lookup(name).unwrap();
            let translucent = m.opacity < 0.999;
            let img = render(&[Item::new(&mesh, &m, &edges)], &cam, &opts);
            assert_eq!(img.rgba.len(), 96 * 72 * 4);
            // Mean luma (0..255) over a rect well inside the silhouette.
            let luma = mean_luma(&img, (38, 28, 58, 44));
            assert!(luma.is_finite(), "{name} produced a non-finite image");
            if !translucent {
                // A NaN anywhere in the shading path would encode as black.
                assert!(luma > 5.0, "{name} rendered black ({luma})");
                opaque.push((name, luma));
            }
        }
        let darkest = opaque.iter().min_by(|a, b| a.1.total_cmp(&b.1)).unwrap();
        assert_eq!(darkest.0, "tpu_rubber", "darkest preset was {opaque:?}");
        // Two presets emit: the glow filament and the LED lens. How bright
        // they render is up to the shading stage, which reads
        // `Material::emissive`; under `--backend path` they are also sampled
        // as light sources and light what they sit next to.
        let emitters: Vec<&str> = names().iter().copied().filter(|n| lookup(n).unwrap().emissive.iter().any(|c| *c > 0.0)).collect();
        assert_eq!(emitters, ["pla_glow", "led"]);
    }

    /// The preset library is only useful if its texture reaches the pixels:
    /// `wood_oak` must not render like the same material without a texture.
    #[test]
    fn a_textured_preset_differs_from_its_untextured_twin() {
        let (mesh, edges) = uv_sphere(24, 48);
        let (lo, hi) = mesh.bbox().unwrap();
        let cam = Camera::preset("iso", lo, hi, 0.1);
        let opts = RenderOptions { width: 96, height: 72, draw_edges: false, draw_grid: false, samples: 1, ..Default::default() };
        let oak = lookup("wood_oak").unwrap();
        let plain = Material { texture: String::new(), ..oak.clone() };
        let a = render(&[Item::new(&mesh, &oak, &edges)], &cam, &opts);
        let b = render(&[Item::new(&mesh, &plain, &edges)], &cam, &opts);
        let differing = a.rgba.iter().zip(&b.rgba).filter(|(x, y)| x != y).count();
        assert!(differing > 200, "the wood texture changed only {differing} bytes");
    }
}
