//! `cadpole render-beads`: path-trace a bead scene (docs/beads.md).
//!
//! A bead scene is geometry that is not a solid model: thousands to
//! millions of swept beads (extruded material, wire, tubing) in a binary
//! file, plus a JSON file that names it and gives the materials, a few
//! boxes to stand them on, the camera and the render settings. It renders
//! with the path tracer only; there is no document, no script and no
//! server operation behind it, so the command reads exactly the two files
//! it is given.

use std::path::{Path, PathBuf};

use anyhow::{Context as _, bail};
use cadpole_engine::cadpole_core::context::Material;
use cadpole_engine::cadpole_core::geom::Vec3;
use cadpole_engine::cadpole_core::render::beads::{self, BeadMaterial, BeadSet};
use cadpole_engine::cadpole_core::render::{
    Backend, Camera, Item, RenderOptions, RigLight, material_preset, render_beads, texture_kind,
};
use cadpole_engine::cadpole_core::tessellate::Mesh;
use serde::Deserialize;

#[derive(Deserialize, Debug, Default)]
#[serde(deny_unknown_fields)]
struct SceneFile {
    /// The bead file, relative to the scene file.
    beads: String,
    materials: Vec<MaterialSpec>,
    #[serde(default)]
    boxes: Vec<BoxSpec>,
    /// The plane the beads stand on (see `BeadSet::floor_z`).
    #[serde(default)]
    floor_z: Option<f32>,
    /// Beads longer than this are split for the tree (mm; default 3).
    #[serde(default)]
    max_len: Option<f32>,
    camera: CameraSpec,
    #[serde(default)]
    render: RenderSpec,
}

#[derive(Deserialize, Debug, Default, Clone)]
#[serde(deny_unknown_fields)]
struct MaterialSpec {
    #[serde(default)]
    preset: Option<String>,
    /// `#rrggbb`, sRGB.
    #[serde(default)]
    color: Option<String>,
    #[serde(default)]
    roughness: Option<f64>,
    #[serde(default)]
    metallic: Option<f64>,
    #[serde(default)]
    ior: Option<f64>,
    /// Linear radiance.
    #[serde(default)]
    emissive: Option<[f64; 3]>,
    #[serde(default)]
    transmission: Option<f64>,
    #[serde(default)]
    attenuation_distance: Option<f64>,
    #[serde(default)]
    scatter_distance: Option<f64>,
    #[serde(default)]
    scatter_g: Option<f64>,
    #[serde(default)]
    anisotropy: Option<f64>,
    #[serde(default)]
    texture: Option<String>,
    #[serde(default)]
    texture_scale: Option<f64>,
    /// Subsurface mean free path in mm (beads only).
    #[serde(default)]
    subsurface: Option<f32>,
    /// Embedded flakes: glitter, metal fill, the glints of cut fibre
    /// (beads only).
    #[serde(default)]
    flakes: Option<FlakesSpec>,
    /// Suspended particles of another colour (beads only).
    #[serde(default)]
    speckle: Option<SpeckleSpec>,
    /// Layer-to-layer tone variation, 0..1 (beads only).
    #[serde(default)]
    streak: Option<f32>,
}

#[derive(Deserialize, Debug, Default, Clone)]
#[serde(deny_unknown_fields)]
struct FlakesSpec {
    density: f32,
    /// Spacing in mm.
    size: f32,
    #[serde(default)]
    tilt_degrees: Option<f32>,
    #[serde(default)]
    roughness: Option<f32>,
    /// `#rrggbb`, the flake's reflectance.
    #[serde(default)]
    color: Option<String>,
}

#[derive(Deserialize, Debug, Default, Clone)]
#[serde(deny_unknown_fields)]
struct SpeckleSpec {
    density: f32,
    size: f32,
    color: String,
}

#[derive(Deserialize, Debug, Default, Clone)]
#[serde(deny_unknown_fields)]
struct LightSpec {
    azimuth_degrees: f32,
    elevation_degrees: f32,
    intensity: f32,
    #[serde(default)]
    radius_degrees: Option<f32>,
}

#[derive(Deserialize, Debug, Clone)]
#[serde(deny_unknown_fields)]
struct BoxSpec {
    lo: [f64; 3],
    hi: [f64; 3],
    material: MaterialSpec,
}

#[derive(Deserialize, Debug, Default)]
#[serde(deny_unknown_fields)]
struct CameraSpec {
    eye: [f64; 3],
    target: [f64; 3],
    #[serde(default)]
    up: Option<[f64; 3]>,
    #[serde(default)]
    fov_degrees: Option<f64>,
}

#[derive(Deserialize, Debug, Default)]
#[serde(deny_unknown_fields)]
struct RenderSpec {
    #[serde(default)]
    width: Option<u32>,
    #[serde(default)]
    height: Option<u32>,
    #[serde(default)]
    samples: Option<u32>,
    #[serde(default)]
    threads: Option<u32>,
    #[serde(default)]
    denoise: Option<bool>,
    #[serde(default)]
    f_number: Option<f32>,
    #[serde(default)]
    lighting: Option<String>,
    #[serde(default)]
    backdrop: Option<String>,
    #[serde(default)]
    floor: Option<String>,
    #[serde(default)]
    exposure: Option<f32>,
    #[serde(default)]
    max_bounces: Option<u32>,
    #[serde(default)]
    light_angle_degrees: Option<f32>,
    #[serde(default)]
    seed: Option<u32>,
    /// `#rrggbb` behind the subject when no backdrop is shown.
    #[serde(default)]
    background: Option<String>,
    /// `aces` (the default), `agx`, `neutral` or `none`.
    #[serde(default)]
    tonemap: Option<String>,
    /// Adaptive sampling's error target (see `PathTraceOptions::
    /// adaptive_error`); 0 takes every sample in every pixel. Default 0.004.
    #[serde(default)]
    adaptive: Option<f32>,
    /// The product rig's key, fill and rim placed by hand, in that order,
    /// relative to the camera.
    #[serde(default)]
    lights: Option<Vec<LightSpec>>,
}

/// What the command may override on the scene's own settings.
pub struct Overrides {
    pub samples: Option<u32>,
    pub threads: Option<u32>,
    pub size: Option<(u32, u32)>,
}

fn srgb_hex(s: &str) -> anyhow::Result<[f64; 3]> {
    let h = s.trim().trim_start_matches('#');
    if h.len() != 6 && h.len() != 8 {
        bail!("colour {s:?} is not #rrggbb");
    }
    let byte = |i: usize| {
        u8::from_str_radix(&h[i..i + 2], 16).with_context(|| format!("colour {s:?} is not #rrggbb"))
    };
    let lin = |v: u8| cadpole_engine::cadpole_core::paint::srgb_to_linear(v as f64 / 255.0);
    Ok([lin(byte(0)?), lin(byte(2)?), lin(byte(4)?)])
}

fn bead_material(spec: &MaterialSpec) -> anyhow::Result<BeadMaterial> {
    let (material, subsurface) = material(spec)?;
    let fin = |v: f32, lo: f32, hi: f32| if v.is_finite() { v.clamp(lo, hi) } else { lo };
    let flakes = match &spec.flakes {
        None => None,
        Some(f) => Some(beads::Flakes {
            density: fin(f.density, 0.0, 1.0),
            size: fin(f.size, 0.005, 10.0),
            tilt_degrees: fin(f.tilt_degrees.unwrap_or(25.0), 0.0, 80.0),
            roughness: fin(f.roughness.unwrap_or(0.15), 0.03, 1.0),
            color: srgb_hex(f.color.as_deref().unwrap_or("#d8d8d8"))?.map(|v| v as f32),
        }),
    };
    let speckle = match &spec.speckle {
        None => None,
        Some(s) => Some(beads::Speckle {
            density: fin(s.density, 0.0, 1.0),
            size: fin(s.size, 0.005, 10.0),
            color: srgb_hex(&s.color)?.map(|v| v as f32),
        }),
    };
    Ok(BeadMaterial {
        material,
        subsurface,
        flakes,
        speckle,
        streak: fin(spec.streak.unwrap_or(0.0), 0.0, 1.0),
    })
}

fn material(spec: &MaterialSpec) -> anyhow::Result<(Material, f32)> {
    let mut m = match &spec.preset {
        Some(p) => material_preset(p).with_context(|| format!("unknown material preset {p:?}"))?,
        None => Material::default(),
    };
    if let Some(c) = &spec.color {
        m.base_color = srgb_hex(c)?;
    }
    let set = |dst: &mut f64, v: Option<f64>| {
        if let Some(v) = v.filter(|v| v.is_finite()) {
            *dst = v;
        }
    };
    set(&mut m.roughness, spec.roughness);
    set(&mut m.metallic, spec.metallic);
    set(&mut m.ior, spec.ior);
    set(&mut m.transmission, spec.transmission);
    set(&mut m.attenuation_distance, spec.attenuation_distance);
    set(&mut m.scatter_distance, spec.scatter_distance);
    set(&mut m.scatter_g, spec.scatter_g);
    set(&mut m.anisotropy, spec.anisotropy);
    set(&mut m.texture_scale, spec.texture_scale);
    if let Some(e) = spec.emissive {
        m.emissive = e;
    }
    if let Some(t) = &spec.texture {
        texture_kind(t).with_context(|| format!("unknown texture {t:?}"))?;
        m.texture = t.clone();
    }
    // A material with a transmission is see-through; the opacity the
    // rasterizer would blend with follows it.
    if m.transmission > 0.0 && m.opacity >= 0.999 {
        m.opacity = 0.35;
    }
    Ok((m, spec.subsurface.unwrap_or(0.0)))
}

fn box_mesh(lo: [f64; 3], hi: [f64; 3]) -> Mesh {
    let mut m = Mesh::default();
    let faces: [([f64; 3], [[f64; 3]; 4]); 6] = [
        (
            [0.0, 0.0, 1.0],
            [
                [lo[0], lo[1], hi[2]],
                [hi[0], lo[1], hi[2]],
                [hi[0], hi[1], hi[2]],
                [lo[0], hi[1], hi[2]],
            ],
        ),
        (
            [0.0, 0.0, -1.0],
            [
                [lo[0], lo[1], lo[2]],
                [lo[0], hi[1], lo[2]],
                [hi[0], hi[1], lo[2]],
                [hi[0], lo[1], lo[2]],
            ],
        ),
        (
            [1.0, 0.0, 0.0],
            [
                [hi[0], lo[1], lo[2]],
                [hi[0], hi[1], lo[2]],
                [hi[0], hi[1], hi[2]],
                [hi[0], lo[1], hi[2]],
            ],
        ),
        (
            [-1.0, 0.0, 0.0],
            [
                [lo[0], lo[1], lo[2]],
                [lo[0], lo[1], hi[2]],
                [lo[0], hi[1], hi[2]],
                [lo[0], hi[1], lo[2]],
            ],
        ),
        (
            [0.0, 1.0, 0.0],
            [
                [lo[0], hi[1], lo[2]],
                [lo[0], hi[1], hi[2]],
                [hi[0], hi[1], hi[2]],
                [hi[0], hi[1], lo[2]],
            ],
        ),
        (
            [0.0, -1.0, 0.0],
            [
                [lo[0], lo[1], lo[2]],
                [hi[0], lo[1], lo[2]],
                [hi[0], lo[1], hi[2]],
                [lo[0], lo[1], hi[2]],
            ],
        ),
    ];
    for (f, (n, quad)) in faces.iter().enumerate() {
        let base = (m.positions.len() / 3) as u32;
        for p in quad {
            m.positions.extend(p.iter().map(|v| *v as f32));
            m.normals.extend(n.iter().map(|v| *v as f32));
        }
        m.indices
            .extend([base, base + 1, base + 2, base, base + 2, base + 3]);
        m.face_ids.extend([f as u32, f as u32]);
    }
    m
}

/// Load, render and write the PNG. Prints timings and sizes to stderr.
pub fn run(scene_path: &Path, out: &PathBuf, ov: Overrides) -> anyhow::Result<()> {
    let t0 = std::time::Instant::now();
    let text = std::fs::read_to_string(scene_path)
        .with_context(|| format!("reading {}", scene_path.display()))?;
    let scene: SceneFile =
        serde_json::from_str(&text).with_context(|| format!("parsing {}", scene_path.display()))?;
    let dir = scene_path.parent().unwrap_or(Path::new("."));
    let bead_path = dir.join(&scene.beads);
    let bytes =
        std::fs::read(&bead_path).with_context(|| format!("reading {}", bead_path.display()))?;
    let list =
        beads::decode(&bytes).map_err(|e| anyhow::anyhow!("{}: {e}", bead_path.display()))?;
    drop(bytes);
    if scene.materials.is_empty() {
        bail!("a bead scene needs at least one material");
    }
    let mats = scene
        .materials
        .iter()
        .map(bead_material)
        .collect::<anyhow::Result<Vec<_>>>()?;
    let t_load = t0.elapsed().as_secs_f64();
    let mut set = BeadSet::new(list, mats, scene.max_len.unwrap_or(3.0));
    set.floor_z = scene.floor_z;

    let boxes: Vec<(Mesh, Material)> = scene
        .boxes
        .iter()
        .map(|b| Ok((box_mesh(b.lo, b.hi), material(&b.material)?.0)))
        .collect::<anyhow::Result<_>>()?;
    let items: Vec<Item<'_>> = boxes
        .iter()
        .map(|(m, mat)| Item::new(m, mat, &[]))
        .collect();

    let r = &scene.render;
    let mut opts = RenderOptions {
        backend: Backend::Path,
        ..RenderOptions::default()
    };
    opts.width = r.width.unwrap_or(1600);
    opts.height = r.height.unwrap_or(1000);
    if let Some((w, h)) = ov.size {
        opts.width = w;
        opts.height = h;
    }
    opts.samples = ov.samples.or(r.samples).unwrap_or(96).max(1);
    opts.threads = ov.threads.or(r.threads).unwrap_or(0);
    opts.path.denoise = r.denoise.unwrap_or(true);
    opts.path.dof_f_number = r
        .f_number
        .filter(|f| f.is_finite() && *f > 0.0)
        .unwrap_or(0.0);
    if let Some(e) = r.exposure.filter(|e| e.is_finite() && *e > 0.0) {
        opts.exposure = e;
    }
    if let Some(b) = r.max_bounces {
        opts.path.max_bounces = b.clamp(1, 64);
    }
    if let Some(a) = r.light_angle_degrees.filter(|a| a.is_finite() && *a > 0.0) {
        opts.path.light_angle_degrees = a;
    }
    opts.path.seed = r.seed.unwrap_or(0);
    let keep_background = r.background.is_some();
    if let Some(bg) = &r.background {
        let c = srgb_hex(bg)?;
        opts.background = [c[0] as f32, c[1] as f32, c[2] as f32];
    }
    opts.path.adaptive_error = r
        .adaptive
        .filter(|a| a.is_finite())
        .unwrap_or(0.004)
        .max(0.0);
    let look = cadpole_engine::Look::parse(
        r.lighting.as_deref().unwrap_or("product"),
        r.backdrop.as_deref().unwrap_or(""),
        r.floor.as_deref().unwrap_or(""),
        r.tonemap.as_deref().unwrap_or(""),
    )?;
    look.apply(&mut opts, keep_background);
    if let Some(ls) = &r.lights {
        if ls.len() != 3 {
            bail!(
                "render.lights is the key, fill and rim: three lights, not {}",
                ls.len()
            );
        }
        let l = |s: &LightSpec| RigLight {
            azimuth_degrees: s.azimuth_degrees,
            elevation_degrees: s.elevation_degrees,
            intensity: s.intensity,
            radius_degrees: s.radius_degrees.unwrap_or(20.0),
        };
        opts.rig = Some([l(&ls[0]), l(&ls[1]), l(&ls[2])]);
    }

    let c = &scene.camera;
    let v = |a: [f64; 3]| Vec3::new(a[0], a[1], a[2]);
    let cam = Camera {
        eye: v(c.eye),
        target: v(c.target),
        up: v(c.up.unwrap_or([0.0, 0.0, 1.0])),
        fov_degrees: c.fov_degrees.unwrap_or(30.0),
        orthographic: false,
        shift: 0.0,
    };
    let t1 = std::time::Instant::now();
    let (img, stats) = render_beads(&items, &set, &cam, &opts);
    let t_render = t1.elapsed().as_secs_f64();
    std::fs::write(out, img.to_png()).with_context(|| format!("writing {}", out.display()))?;
    let bs = set.stats;
    eprintln!(
        "{{\"beads_in\":{},\"beads\":{},\"bvh_nodes\":{},\"bvh_depth\":{},\"bead_mb\":{:.1},\"build_ms\":{:.0},\"load_s\":{:.2},\"trace_s\":{:.2},\"shade_ms\":{:.0},\"denoise_ms\":{:.0},\"samples\":{},\"mean_samples\":{:.1},\"threads\":{},\"width\":{},\"height\":{}}}",
        bs.input,
        bs.beads,
        bs.nodes,
        bs.max_depth,
        bs.bytes as f64 / 1048576.0,
        bs.build_ms,
        t_load,
        t_render,
        stats.shade_ms,
        stats.resolve_ms,
        opts.samples,
        stats.mean_samples,
        stats.threads_used,
        opts.width,
        opts.height
    );
    Ok(())
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn a_small_scene_renders_and_an_unknown_key_is_refused() {
        let dir = std::env::temp_dir().join(format!("cadpole-beads-{}", std::process::id()));
        std::fs::create_dir_all(&dir).unwrap();
        let mut list = Vec::new();
        for layer in 0..10 {
            let z = 0.1 + 0.2 * layer as f32;
            for i in 0..4 {
                let y = i as f32 * 0.45;
                list.push(beads::Bead {
                    a: [0.0, y, z],
                    b: [10.0, y, z],
                    rx: 0.25,
                    rz: 0.12,
                    material: 0,
                });
            }
        }
        std::fs::write(dir.join("w.beads"), beads::encode(&list)).unwrap();
        let scene = r##"{"beads":"w.beads","materials":[{"preset":"pla_matte","color":"#c03020","subsurface":0.3}],
            "boxes":[{"lo":[-5,-5,-1],"hi":[15,7,0],"material":{"color":"#555048","texture":"stipple","texture_scale":0.3}}],
            "floor_z":0,"camera":{"eye":[30,-30,20],"target":[5,1,1]},
            "render":{"width":48,"height":32,"samples":4,"threads":2}}"##;
        std::fs::write(dir.join("s.json"), scene).unwrap();
        let out = dir.join("o.png");
        run(
            &dir.join("s.json"),
            &out,
            Overrides {
                samples: None,
                threads: None,
                size: None,
            },
        )
        .unwrap();
        let img =
            cadpole_engine::cadpole_core::render::Image::from_png(&std::fs::read(&out).unwrap())
                .unwrap();
        assert_eq!((img.width, img.height), (48, 32));
        std::fs::write(
            dir.join("bad.json"),
            scene.replace("\"floor_z\"", "\"flor_z\""),
        )
        .unwrap();
        assert!(
            run(
                &dir.join("bad.json"),
                &out,
                Overrides {
                    samples: None,
                    threads: None,
                    size: None
                }
            )
            .is_err()
        );
        let _ = std::fs::remove_dir_all(&dir);
    }
}
