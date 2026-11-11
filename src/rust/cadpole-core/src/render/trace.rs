//! CPU path tracer: the high-quality, non-interactive render backend.
//!
//! `cadpole render --backend path --samples N` runs this instead of the
//! rasterizer. It is a unidirectional path tracer with next-event estimation
//! and multiple importance sampling, a BVH over every triangle in the scene
//! ([`super::bvh`]), and a BSDF built for **3-D printed parts**: GGX
//! microfacet reflection *and* transmission, Beer-Lambert absorption and
//! single-scattering inside the solid, emissive surfaces that actually light
//! the scene, and a cheap anisotropic roughness aligned to the print's layer
//! direction.
//!
//! Why a CPU tracer when docs/design.md makes the *production* path tracer
//! wgpu compute: this one runs headless, in CI and in wasm, with no
//! GPU and no driver, and it is deterministic. It is the reference the GPU
//! implementation is checked against — same material semantics, same light
//! rig, same tonemap, so the two can be compared pixel-for-pixel.
//!
//! # Determinism
//!
//! Every pixel's value is a pure function of `(x, y, sample index, seed)`:
//! the sampler is a scrambled Halton sequence keyed by a hash of the pixel
//! index, there is no shared RNG state, and the image is identical for any
//! thread count (bands write disjoint memory, see `super::aa::par_bands2`).
//! The BVH build is sequential and deterministic. wasm32 runs single
//! threaded and produces the same pixels.
//!
//! # Structure
//!
//! * [`PathTraceOptions`] — bounces, light size, firefly clamp, ground plane.
//! * [`Pbr`] — the material the tracer works with; see `material_for`.
//! * `Sampler` — scrambled Halton, per pixel and sample.
//! * `Bsdf` — Lambert + anisotropic GGX reflection + GGX transmission.
//! * `Medium` — Beer-Lambert absorption and an optional scattering random walk.
//! * `Tracer` — the scene (BVH, materials, lights, ground) and the integrator.

#![allow(clippy::too_many_arguments)]

use std::collections::HashMap;

use super::aa::{band_rows, bloom, par_bands2, resolve, resolve_threads, tonemap_rgb};
use super::bvh::{Bvh, BvhStats, Hit, ShadowHits};
use super::shading::{Lights, STUDIO_LIGHT_DEGREES, dielectric_f0, env, fresnel_schlick};
use super::{
    Camera, HIGHLIGHT, Image, Item, RenderOptions, RenderStats, Scene, View, add, cross, dot, inverse_tonemap_rgb,
    lerp3, luminance, mul, normalize, scale, sub, textures,
};
use crate::context::Material;

const PI: f32 = std::f32::consts::PI;
const INV_PI: f32 = 1.0 / PI;
const TAU: f32 = std::f32::consts::TAU;
/// Largest f32 strictly below 1.0; sample values are clamped to it so that
/// `floor(u * n)` can never reach `n`.
const ONE_MINUS_EPS: f32 = 1.0 - f32::EPSILON / 2.0;
/// Microfacet alpha at or below which a lobe is sampled as a mirror rather
/// than through the VNDF: roughness 0.141 and below, which is every preset
/// whose name says "clear".
///
/// This threshold is the single biggest lever on how long a translucent
/// scene takes to converge, and the reason is worth writing down. A lobe
/// this narrow subtends well under a degree, so sampling it barely changes
/// where the ray goes — but "barely" is enough to turn "this path reaches
/// the key light" into a coin flip, and the key light is a small, bright
/// source whose radiance is tens of units against a pixel worth tenths.
/// One camera ray through a clear slab crosses two such interfaces, so the
/// coin is flipped twice and the result is salt-and-pepper that a thousand
/// samples per pixel does not remove.
///
/// Sampled as a delta instead, the path through a clear part is a *pure
/// function of the pixel*: it refracts, it reflects, it either sees the
/// light or it does not, and the image is crisp at 64 samples. Nothing real
/// is lost, because an optically clear cast or moulded part is smooth —
/// the small roughness those presets carry exists to size the rasterizer's
/// highlight, not because the surface scatters. Frosted and translucent
/// presets sit well above this line and keep their real, wide lobes.
const DELTA_ALPHA: f32 = 0.02;
/// Rows per parallel band, before [`super::aa::band_rows`] narrows it to
/// what this image height and thread count actually want. Large enough that
/// the band queue is not the bottleneck; `band_rows` shrinks it when the
/// image is short or the pool is wide, so the tail of the queue cannot
/// leave a worker idle.
const BAND_ROWS: usize = 4;
/// Deterministic path splits allowed per camera ray at smooth dielectric
/// interfaces. Eight covers a ray that crosses four clear walls; past that
/// the branches are dim enough that a Fresnel coin costs nothing.
const MAX_SPLITS: u32 = 8;
/// See-through interfaces a shadow ray accounts for. A ray that crosses 32
/// has told us all we are going to learn about this light; it is also how
/// many crossings [`Tracer::visibility`] collects before it falls back to
/// stepping through them one at a time.
const MAX_SHADOW_INTERFACES: usize = 32;
/// The diffuse albedo (weight times reflectance) below which a surface
/// whose microfacet lobe is a delta gets no shadow ray (`Bsdf::nee_useful`).
const NEE_MIN_DIFFUSE: f32 = 0.1;
/// Bins of the environment sampler's table over `dir.z` ([`EnvSampler`]).
const ENV_BINS: usize = 64;

// ---------------------------------------------------------------------------
// Options.

/// Path-tracer settings. Sample count comes from [`RenderOptions::samples`]
/// (the rasterizer snaps that to 1/4/8; the tracer uses it verbatim).
#[derive(Clone, Debug)]
pub struct PathTraceOptions {
    /// Maximum number of surface scattering events per path (default 8).
    /// Russian roulette usually terminates paths well before this.
    pub max_bounces: u32,
    /// Angular size of the rig's lights, in degrees (default 14). Each
    /// light owns its radius (`shading::Light::radius_degrees`: 14 for all
    /// three studio lights, 22/30/8 for the product rig's key/fill/rim) and
    /// this scales them all: at 14 it is a multiplier of 1, so the studio
    /// cones are 14° and the product cones what the rig says; at 0.5 the
    /// studio has a hard sun and at 25 an overcast softbox. The rig's total
    /// irradiance is held constant as it changes, so a wider light is not a
    /// brighter one — but it is a much less peaked one, and that is most of
    /// what keeps a glossy highlight from throwing fireflies.
    pub light_angle_degrees: f32,
    /// Firefly clamp: every contribution that has already scattered at
    /// least once is clipped to this radiance (default 10; 0 disables).
    ///
    /// Next-event estimation at the primary hit is never clamped, so the
    /// direct lighting that carries the image is exact. What is clamped is
    /// the path that refracts through a clear part and *then* finds a light:
    /// that is a caustic, it arrives as a single sample worth tens of units
    /// against a pixel worth tenths, and no sample count a person will wait
    /// for makes it converge. The bias is small where it is visible at all,
    /// because the ACES curve maps anything past ~3 to white anyway: a
    /// genuine highlight clipped from 27 to 10 moves from 0.985 to 0.96.
    pub clamp: f32,
    /// Add an implicit ground plane at `Scene::ground_z` so parts have real
    /// contact shadows and pick up bounce light (default true).
    pub ground: bool,
    /// Albedo of that plane, linear RGB. Deliberately dark: a photographic
    /// sweep is much darker than the subject, which is what separates a
    /// light-grey printed part from the surface it stands on, and it lands
    /// the floor close to the rasterizer's background colour so the two
    /// backends' backdrops agree.
    pub ground_albedo: [f32; 3],
    /// GGX roughness of that plane (default 0.85, a matte sweep). The
    /// plane is a dielectric with the F0 of 0.04, so at 0.35 (`--floor
    /// gloss`) it reflects the part faintly, the way a slightly glossy table
    /// in a product photograph anchors what stands on it.
    pub ground_roughness: f32,
    /// Camera rays that hit nothing show the environment gradient instead of
    /// `RenderOptions::background` (default false, so path-traced and
    /// rasterized images have the same backdrop).
    pub show_environment: bool,
    /// Edge-break radius in mm for every body whose material does not set
    /// its own `edge_break` (default 0, off). See [`EdgeBreak`].
    pub edge_break_mm: f32,
    /// Thin-lens depth of field: the f-number, 0 for a pinhole (default).
    /// Focus is at the camera target; see `View::with_dof` for the lens.
    /// f/8 on a 60 mm part is a gentle fall-off at the far edge of the
    /// floor, f/4 a hero shot. Consumes two sampler dimensions per path
    /// only when set, so an image without it is byte-identical to before.
    pub dof_f_number: f32,
    /// Maximum scattering events inside a translucent solid before the walk
    /// is abandoned (default 64).
    pub max_volume_steps: u32,
    /// Sequence seed; changing it re-randomizes the noise without changing
    /// the converged image.
    pub seed: u32,
    /// Replace the studio rig with [`Lights::furnace`]: a uniform
    /// environment of 1 and no direct light. This is what makes the white
    /// furnace and energy-conservation tests meaningful, and it is the only
    /// switch that changes the light rig — everything else about the two
    /// backends' lighting is shared.
    pub furnace: bool,
    /// Multiplier on the three rig lights (default 1). Zero leaves only the
    /// environment gradient, which is how a test isolates what a material
    /// does to a *background* — refraction, blur, absorption — from what the
    /// key light does to its surface.
    pub direct_scale: f32,
    /// Sample the environment gradient by next-event estimation as well as
    /// by following the BSDF (see [`EnvSampler`]). Default **false**, on
    /// the measurement: against a 4096-path reference at 64 paths, the
    /// strategy *raised* the RMS error by 4 % on a glossy metal and 21 % on
    /// a matte sphere (`grain_bench`). A gradient sky is smooth, cosine and
    /// VNDF sampling already find it well, and the share of shadow rays
    /// the sky takes comes out of the cones, which are where the variance
    /// is. It stays as an option because an environment with structure
    /// (an HDRI) is the case
    /// it was built for. Off, the sky is found only by the BSDF path with
    /// weight 1, as it always was.
    pub env_sampling: bool,
    /// Run the edge-aware à-trous denoiser on the finished image (default
    /// false; see [`denoise`]).
    pub denoise: bool,
}

impl Default for PathTraceOptions {
    fn default() -> Self {
        PathTraceOptions {
            max_bounces: 8,
            light_angle_degrees: 14.0,
            clamp: 6.0,
            ground: true,
            ground_albedo: [0.09, 0.091, 0.097],
            ground_roughness: 0.85,
            show_environment: false,
            edge_break_mm: 0.0,
            dof_f_number: 0.0,
            max_volume_steps: 64,
            seed: 0,
            furnace: false,
            direct_scale: 1.0,
            env_sampling: false,
            denoise: false,
        }
    }
}

impl PathTraceOptions {
    /// The rig the white-furnace and energy tests use: uniform environment,
    /// no direct light, no ground plane, no clamp and no background
    /// substitution, so a covered pixel converges to the environment radiance
    /// times the material's albedo response and nothing else.
    pub fn furnace_test() -> PathTraceOptions {
        PathTraceOptions {
            furnace: true,
            ground: false,
            show_environment: true,
            clamp: 0.0,
            max_bounces: 24,
            ..PathTraceOptions::default()
        }
    }
}

// ---------------------------------------------------------------------------
// Material.

/// The material the path tracer works with: `crate::context::Material` in
/// f32, with the strings resolved and every value already clamped to its
/// documented range, so the integrator never has to re-check.
///
/// There is no second preset table here: `Material` carries all thirteen
/// fields and `render/presets.rs` is the one place they are authored. The
/// only thing [`material_for`] decides on its own is what `transmission = 0`
/// means on a material whose `opacity` is translucent (see below).
#[derive(Clone, Copy, Debug)]
pub struct Pbr {
    /// Linear RGB 0..1. Also the volume's transmission colour at
    /// `attenuation_distance`.
    pub base_color: [f32; 3],
    /// 0..1, clamped to [0.03, 1] as the contract requires; alpha = r*r.
    pub roughness: f32,
    pub metallic: f32,
    /// 1.0 .. 2.5; drives both Fresnel and refraction here.
    pub ior: f32,
    /// Linear radiance, unbounded. Emissive surfaces are sampled as lights.
    pub emissive: [f32; 3],
    /// Fraction of the surface that refracts rather than reflects/diffuses.
    /// A clear printed part is `transmission = 1` with a short
    /// `attenuation_distance`, not a half-transparent surface: that is what
    /// makes a 3 mm wall read darker than a 0.8 mm one.
    pub transmission: f32,
    /// Distance in mm over which the interior attenuates to `base_color`
    /// (Beer-Lambert). `<= 0` or `inf` = no absorption.
    pub attenuation_distance: f32,
    /// Mean free path in mm between scattering events inside the solid;
    /// `<= 0` = no scattering (clear). Short values give milky, filled
    /// filaments and the soft glow of a thick translucent wall.
    pub scatter_distance: f32,
    /// Henyey-Greenstein asymmetry, -1 (back) .. 1 (forward).
    pub scatter_g: f32,
    /// Layer-line anisotropy, 0..1: roughness across the layer stacking
    /// direction is raised and along the extrusion lowered, so FDM parts
    /// read as printed rather than moulded. Zero on faces whose normal is
    /// parallel to Z (the layer faces themselves).
    pub anisotropy: f32,
    /// Procedural texture (applied as an albedo multiplier + roughness
    /// delta, exactly as the contract specifies).
    pub texture: textures::Kind,
    pub texture_scale: f32,
    /// Groove depth of a machined texture in mm; 0 = the kind's default.
    pub texture_depth: f32,
    /// Edge-break radius in mm; 0 = the render's `edge_break_mm`.
    pub edge_break: f32,
    /// Set per hit by [`Tracer::shade_material`] from a machined texture:
    /// the direction the grooves run in, which the anisotropic lobe's
    /// smooth axis follows instead of the print's layer direction. Zero
    /// (the material's own value) means "no groove": the FDM rule.
    pub groove: [f32; 3],
    /// Set per hit alongside `groove`: slope variance the pixel footprint
    /// averaged out of the grooves, folded into the roughness across them.
    pub slope_var: f32,
    /// Whether the body casts a shadow. False is guide geometry:
    /// camera rays see it, and nothing else does — no shadow ray stops at
    /// it or is tinted by it, no bounce hits it, it is no light source, and
    /// it has no interior medium. The camera's view is the pure specular
    /// chain from the eye (`!caustic`), so a guide is still seen through a
    /// clear guide and in a mirror.
    pub shadow: bool,
}

impl Default for Pbr {
    fn default() -> Self {
        Pbr {
            base_color: [0.72, 0.74, 0.78],
            roughness: 0.6,
            metallic: 0.0,
            ior: 1.5,
            emissive: [0.0; 3],
            transmission: 0.0,
            attenuation_distance: 0.0,
            scatter_distance: 0.0,
            scatter_g: 0.0,
            anisotropy: 0.0,
            texture: textures::Kind::None,
            texture_scale: 10.0,
            texture_depth: 0.0,
            edge_break: 0.0,
            groove: [0.0; 3],
            slope_var: 0.0,
            shadow: true,
        }
    }
}

impl Pbr {
    #[inline]
    fn is_transmissive(&self) -> bool {
        self.transmission > 1e-3
    }

    #[inline]
    fn emits(&self) -> bool {
        self.emissive[0] > 0.0 || self.emissive[1] > 0.0 || self.emissive[2] > 0.0
    }
}

/// An item's material as the tracer draws it, with the item's opacity
/// override (`Item::opacity`, what the `xray` view sets) in place of the
/// material's own: an override below 1 is see-through the way a script's
/// `opacity=` is, and an override of 1 (a debug view) is opaque whatever
/// the material says.
pub fn item_material(item: &Item<'_>) -> Pbr {
    match item.opacity {
        None => material_for(item.material),
        Some(o) => {
            let mut m = item.material.clone();
            m.opacity = o.clamp(0.0, 1.0);
            if m.opacity >= 0.999 {
                m.transmission = 0.0;
            } else {
                m.transmission = m.transmission.max(1.0 - m.opacity);
            }
            material_for(&m)
        }
    }
}

/// Convert a scene [`Material`] into the tracer's [`Pbr`].
///
/// Everything is a straight copy except `transmission`. Presets and
/// `material(transmission=...)` author it directly, but a script that says
/// only `opacity=0.3` still means "this part is see-through". So a zero
/// `transmission` on a translucent material is read as `1 - opacity`: that
/// script refracts instead of merely blending, and an opaque material is
/// untouched.
pub fn material_for(m: &Material) -> Pbr {
    let opacity = (m.opacity as f32).clamp(0.0, 1.0);
    let transmission = if m.transmission > 0.0 {
        (m.transmission as f32).clamp(0.0, 1.0)
    } else if opacity < 0.999 {
        1.0 - opacity
    } else {
        0.0
    };
    // `opacity` itself has no further meaning here: it is the rasterizer's
    // blend factor, and everything the tracer does with a see-through part
    // goes through `transmission` and the medium below it.
    Pbr {
        base_color: m.base_color.map(|c| (c as f32).clamp(0.0, 1.0)),
        roughness: (m.roughness as f32).clamp(0.03, 1.0),
        metallic: (m.metallic as f32).clamp(0.0, 1.0),
        ior: (m.ior as f32).clamp(1.0, 2.5),
        emissive: m.emissive.map(|c| if (c as f32).is_finite() { (c as f32).max(0.0) } else { 0.0 }),
        transmission,
        attenuation_distance: (m.attenuation_distance as f32).max(0.0),
        scatter_distance: (m.scatter_distance as f32).max(0.0),
        scatter_g: (m.scatter_g as f32).clamp(-0.95, 0.95),
        anisotropy: (m.anisotropy as f32).clamp(0.0, 1.0),
        texture: textures::kind(&m.texture).unwrap_or(textures::Kind::None),
        texture_scale: if m.texture_scale > 0.0 { m.texture_scale as f32 } else { 10.0 },
        texture_depth: if (m.texture_depth as f32).is_finite() { (m.texture_depth as f32).max(0.0) } else { 0.0 },
        edge_break: if (m.edge_break as f32).is_finite() { (m.edge_break as f32).max(0.0) } else { 0.0 },
        groove: [0.0; 3],
        slope_var: 0.0,
        shadow: m.shadow,
    }
}

// ---------------------------------------------------------------------------
// Sampler: scrambled Halton.

/// First 16 primes: one Halton base per sample dimension.
const PRIMES: [u32; 16] = [2, 3, 5, 7, 11, 13, 17, 19, 23, 29, 31, 37, 41, 43, 47, 53];

#[inline]
fn lowbias32(mut x: u32) -> u32 {
    x ^= x >> 16;
    x = x.wrapping_mul(0x7feb_352d);
    x ^= x >> 15;
    x = x.wrapping_mul(0x846c_a68b);
    x ^= x >> 16;
    x
}

#[inline]
fn hash2(a: u32, b: u32) -> u32 {
    lowbias32(a ^ lowbias32(b.wrapping_mul(0x9e37_79b9).wrapping_add(0x85eb_ca6b)))
}

/// `u32` -> `[0, 1)` using the top 24 bits (exactly representable in f32).
#[inline]
fn u01(x: u32) -> f32 {
    (x >> 8) as f32 * (1.0 / 16_777_216.0)
}

/// Van der Corput / Halton radical inverse of `i` in `base`.
#[inline]
fn radical_inverse(base: u32, i: u32) -> f32 {
    if base == 2 {
        // Bit reversal is the base-2 radical inverse.
        return u01(i.reverse_bits());
    }
    let inv = 1.0 / base as f32;
    let mut r = 0.0f32;
    let mut f = inv;
    let mut i = i;
    while i > 0 {
        r += (i % base) as f32 * f;
        i /= base;
        f *= inv;
    }
    r
}

/// The Halton point for one sample index: the radical inverse of
/// `sample + 1` in each of the sixteen prime bases. It is the same for every
/// pixel of the image, so [`trace_hdr`] computes each row once and hands the
/// sampler a reference; computed on demand it was the division loop of
/// [`radical_inverse`] on every one of the sixteen stratified dimensions of
/// every path, which was six per cent of an opaque scene's render.
#[inline]
fn halton_row(sample: u32) -> [f32; 16] {
    let index = sample.wrapping_add(1);
    PRIMES.map(|p| radical_inverse(p, index))
}

/// A stratified, low-discrepancy sample stream for one pixel and one sample
/// index. The Halton point is rotated per pixel (Cranley-Patterson) by a
/// hash of the pixel index, so neighbouring pixels are decorrelated while
/// each pixel still sees a well-stratified sequence. There is no shared
/// state: the stream depends only on `(x, y, sample, seed)`.
struct Sampler<'h> {
    key: u32,
    index: u32,
    dim: u32,
    /// [`halton_row`] of `index - 1`.
    halton: &'h [f32; 16],
}

impl<'h> Sampler<'h> {
    #[inline]
    fn new(x: u32, y: u32, w: u32, sample: u32, seed: u32, halton: &'h [f32; 16]) -> Sampler<'h> {
        let pixel = (y as u64 * w as u64 + x as u64) as u32;
        Sampler { key: hash2(pixel, seed ^ 0x2545_f491), index: sample.wrapping_add(1), dim: 0, halton }
    }

    #[inline]
    fn next(&mut self) -> f32 {
        let d = self.dim;
        self.dim = self.dim.wrapping_add(1);
        let v = if (d as usize) < PRIMES.len() {
            let h = self.halton[d as usize];
            let r = u01(hash2(self.key, d.wrapping_add(1)));
            let s = h + r;
            if s >= 1.0 { s - 1.0 } else { s }
        } else {
            // Beyond the stratified dimensions, plain hashed noise: deep
            // bounces contribute little and stratifying them buys nothing.
            u01(hash2(self.key ^ self.index.wrapping_mul(0x9e37_79b9), d))
        };
        v.clamp(0.0, ONE_MINUS_EPS)
    }

    #[inline]
    fn next2(&mut self) -> (f32, f32) {
        (self.next(), self.next())
    }
}

// ---------------------------------------------------------------------------
// Small geometry helpers.

/// An orthonormal basis whose `t` axis runs *along* the print layers
/// (horizontal, perpendicular to Z) and whose `b` axis runs up the layer
/// stack. That is the frame the anisotropic roughness is expressed in.
#[derive(Clone, Copy)]
struct Onb {
    t: [f32; 3],
    b: [f32; 3],
    n: [f32; 3],
}

impl Onb {
    fn layer_aligned(n: [f32; 3]) -> Onb {
        let mut t = cross([0.0, 0.0, 1.0], n);
        if dot(t, t) < 1e-10 {
            t = cross([1.0, 0.0, 0.0], n);
        }
        if dot(t, t) < 1e-10 {
            t = [1.0, 0.0, 0.0];
        }
        let t = normalize(t);
        let b = cross(n, t);
        Onb { t, b, n }
    }

    /// The frame the anisotropic lobe is expressed in at a surface: `t`
    /// along the grooves when a machined texture gave a direction, and
    /// along the print layers otherwise — the FDM rule, untouched.
    fn for_surface(n: [f32; 3], groove: [f32; 3]) -> Onb {
        if dot(groove, groove) <= 1e-6 {
            return Onb::layer_aligned(n);
        }
        let t = sub(groove, scale(n, dot(n, groove)));
        if dot(t, t) < 1e-10 {
            return Onb::layer_aligned(n);
        }
        let t = normalize(t);
        let b = cross(n, t);
        Onb { t, b, n }
    }

    #[inline]
    fn to_local(&self, v: [f32; 3]) -> [f32; 3] {
        [dot(v, self.t), dot(v, self.b), dot(v, self.n)]
    }

    #[inline]
    fn to_world(&self, v: [f32; 3]) -> [f32; 3] {
        add(add(scale(self.t, v[0]), scale(self.b, v[1])), scale(self.n, v[2]))
    }
}

#[inline]
fn cosine_hemisphere(u1: f32, u2: f32) -> [f32; 3] {
    let r = u1.max(0.0).sqrt();
    let phi = TAU * u2;
    [r * phi.cos(), r * phi.sin(), (1.0 - u1).max(0.0).sqrt()]
}

/// Balance/power heuristic with beta = 2.
#[inline]
fn power_heuristic(a: f32, b: f32) -> f32 {
    let (a2, b2) = (a * a, b * b);
    if a2 + b2 <= 0.0 { 0.0 } else { a2 / (a2 + b2) }
}

#[inline]
fn max3(c: [f32; 3]) -> f32 {
    c[0].max(c[1]).max(c[2])
}

#[inline]
fn is_black(c: [f32; 3]) -> bool {
    c[0] <= 0.0 && c[1] <= 0.0 && c[2] <= 0.0
}

/// Exact dielectric Fresnel reflectance for unpolarized light. `eta` is
/// `n_incident / n_transmitted`, so `< 1` entering a denser medium.
fn fresnel_dielectric(cos_i: f32, eta: f32) -> f32 {
    let cos_i = cos_i.clamp(0.0, 1.0);
    let sin2_t = eta * eta * (1.0 - cos_i * cos_i);
    if sin2_t >= 1.0 {
        return 1.0; // total internal reflection
    }
    let cos_t = (1.0 - sin2_t).max(0.0).sqrt();
    let r_par = (cos_i / eta - cos_t) / (cos_i / eta + cos_t);
    let r_per = (cos_i - cos_t / eta) / (cos_i + cos_t / eta);
    (0.5 * (r_par * r_par + r_per * r_per)).clamp(0.0, 1.0)
}

/// Refract `wo` (pointing away from the surface, `wo.z > 0`) about the
/// microfacet normal `wh` (`wh.z > 0`). Returns `None` on total internal
/// reflection.
#[inline]
fn refract(wo: [f32; 3], wh: [f32; 3], eta: f32) -> Option<[f32; 3]> {
    let cos_i = dot(wo, wh);
    if cos_i <= 0.0 {
        return None;
    }
    let sin2_t = eta * eta * (1.0 - cos_i * cos_i);
    if sin2_t >= 1.0 {
        return None;
    }
    let cos_t = (1.0 - sin2_t).max(0.0).sqrt();
    Some(add(scale(wo, -eta), scale(wh, eta * cos_i - cos_t)))
}

// ---------------------------------------------------------------------------
// Microfacet distribution (anisotropic GGX with Smith masking-shadowing).

/// Trowbridge-Reitz / GGX normal distribution, anisotropic.
#[inline]
fn ggx_d(wh: [f32; 3], ax: f32, ay: f32) -> f32 {
    let (x, y, z) = (wh[0] / ax, wh[1] / ay, wh[2]);
    let d = x * x + y * y + z * z;
    if d <= 0.0 {
        return 0.0;
    }
    1.0 / (PI * ax * ay * d * d)
}

/// Smith Lambda term for GGX.
#[inline]
fn ggx_lambda(w: [f32; 3], ax: f32, ay: f32) -> f32 {
    let cos2 = w[2] * w[2];
    if cos2 >= 1.0 - 1e-9 {
        return 0.0;
    }
    let a2 = (ax * w[0]) * (ax * w[0]) + (ay * w[1]) * (ay * w[1]);
    (-1.0 + (1.0 + a2 / cos2).max(0.0).sqrt()) * 0.5
}

#[inline]
fn smith_g1(w: [f32; 3], ax: f32, ay: f32) -> f32 {
    1.0 / (1.0 + ggx_lambda(w, ax, ay))
}

/// Height-correlated Smith masking-shadowing.
#[inline]
fn smith_g2(wo: [f32; 3], wi: [f32; 3], ax: f32, ay: f32) -> f32 {
    1.0 / (1.0 + ggx_lambda(wo, ax, ay) + ggx_lambda(wi, ax, ay))
}

/// Heitz 2018: sample the distribution of visible normals. `wo` must have
/// `wo.z > 0`; the returned half vector also has `z > 0`.
fn sample_vndf(wo: [f32; 3], ax: f32, ay: f32, u1: f32, u2: f32) -> [f32; 3] {
    let vh = normalize([ax * wo[0], ay * wo[1], wo[2]]);
    let lensq = vh[0] * vh[0] + vh[1] * vh[1];
    let t1 = if lensq > 1e-12 {
        let inv = 1.0 / lensq.sqrt();
        [-vh[1] * inv, vh[0] * inv, 0.0]
    } else {
        [1.0, 0.0, 0.0]
    };
    let t2 = cross(vh, t1);
    let r = u1.max(0.0).sqrt();
    let phi = TAU * u2;
    let p1 = r * phi.cos();
    let mut p2 = r * phi.sin();
    let s = 0.5 * (1.0 + vh[2]);
    p2 = (1.0 - s) * (1.0 - p1 * p1).max(0.0).sqrt() + s * p2;
    let nh = add(add(scale(t1, p1), scale(t2, p2)), scale(vh, (1.0 - p1 * p1 - p2 * p2).max(0.0).sqrt()));
    normalize([ax * nh[0], ay * nh[1], nh[2].max(1e-6)])
}

/// pdf of the visible-normal distribution for `wh`.
#[inline]
fn vndf_pdf(wo: [f32; 3], wh: [f32; 3], ax: f32, ay: f32) -> f32 {
    let cos_oh = dot(wo, wh);
    if cos_oh <= 0.0 || wo[2] <= 0.0 {
        return 0.0;
    }
    smith_g1(wo, ax, ay) * cos_oh * ggx_d(wh, ax, ay) / wo[2]
}

// ---------------------------------------------------------------------------
// BSDF.

/// The surface BSDF in a local frame where `wo.z > 0` always (the frame is
/// flipped toward the incoming ray) and `wi.z < 0` means transmission.
///
/// Three lobes:
///
/// * **Lambert diffuse**, weight `(1 - metallic) * (1 - transmission)`.
/// * **GGX reflection**, Schlick Fresnel from `f0 = mix(dielectric_f0(ior),
///   base_color, metallic)` for opaque materials, exact dielectric Fresnel
///   for transmissive ones so that reflection and refraction sum to one.
/// * **GGX transmission** (Walter et al. 2007), weight
///   `(1 - metallic) * transmission`, including the `eta^2` radiance scaling
///   that cancels on the way back out of a closed solid.
///
/// `render/shading.rs` owns the analytic version of the same model, and the
/// Fresnel, F0 and environment functions are shared with it verbatim, which
/// is what keeps the two backends on one look. The GGX and Smith terms are
/// deliberately *not* shared: the rasterizer only ever needs the split-sum
/// approximation of a fixed three-light rig, while the tracer needs an
/// importance-samplable, anisotropic distribution with a matching pdf. A
/// common `render/bsdf.rs` would have to carry both.
#[derive(Clone, Copy)]
struct Bsdf {
    albedo: [f32; 3],
    f0: [f32; 3],
    ax: f32,
    ay: f32,
    metallic: f32,
    /// Weight of the transmission lobe.
    trans: f32,
    /// Weight of the diffuse lobe.
    diff: f32,
    /// `n_incident / n_transmitted` from the side `wo` is on.
    eta: f32,
}

struct BsdfSample {
    wi: [f32; 3],
    /// `f * |cos| / pdf`, already divided out.
    weight: [f32; 3],
    pdf: f32,
    /// A mirror/refraction lobe: MIS weight 1, light sampling cannot reach it.
    specular: bool,
    transmitted: bool,
}

impl Bsdf {
    fn new(m: &Pbr, ns: [f32; 3], entering: bool) -> Bsdf {
        let alpha = (m.roughness * m.roughness).max(1e-4);
        // Layer anisotropy. `ax` runs along the layers (horizontal, in the
        // surface) and `ay` up the layer stack, so a printed wall is smooth
        // in the direction the nozzle moved and rough across the ridges it
        // left. The asymmetry is deliberately large — a 0.2 mm layer line
        // is a real ridge, not a statistical wobble — and the product
        // `ax * ay` is roughly preserved, so an anisotropic material is a
        // *differently* shaped highlight rather than simply a rougher one.
        //
        // It fades out on faces that *are* layer faces (normal parallel to
        // Z): looking straight down at the top surface of a print there are
        // no layer lines to see, only the infill pattern, which is a
        // texture and not this.
        //
        // A machined texture replaces the layer direction with its grooves
        // (the frame is `Onb::for_surface`), and there is no top-face fade:
        // a turned end face has grooves. What its footprint filter averaged
        // away comes back as slope variance across the grooves, so ridges
        // below a pixel are the brushed sheen they look like from a distance
        // (GGX's alpha is about sqrt 2 times the slope deviation).
        let machined = dot(m.groove, m.groove) > 1e-6;
        let k = if machined { m.anisotropy.clamp(0.0, 1.0) } else { m.anisotropy.clamp(0.0, 1.0) * (1.0 - ns[2].abs()) };
        let ax = (alpha * (1.0 - 0.70 * k)).clamp(1e-5, 1.0);
        let ay = if m.slope_var > 0.0 {
            let ay0 = alpha * (1.0 + 3.00 * k);
            (ay0 * ay0 + 2.0 * m.slope_var).sqrt().clamp(1e-5, 1.0)
        } else {
            (alpha * (1.0 + 3.00 * k)).clamp(1e-5, 1.0)
        };
        let trans = (1.0 - m.metallic) * m.transmission;
        let diff = (1.0 - m.metallic) * (1.0 - m.transmission);
        let f0 = lerp3([dielectric_f0(m.ior); 3], m.base_color, m.metallic);
        let eta = if entering { 1.0 / m.ior } else { m.ior };
        Bsdf { albedo: m.base_color, f0, ax, ay, metallic: m.metallic, trans, diff, eta }
    }

    #[inline]
    fn is_delta(&self) -> bool {
        self.ax <= DELTA_ALPHA && self.ay <= DELTA_ALPHA
    }

    /// Fresnel reflectance at `cos`: exact dielectric for transmissive
    /// materials (so R + T = 1), Schlick from `f0` otherwise — which is what
    /// the frozen contract specifies for the rasterizer.
    #[inline]
    fn fresnel(&self, cos: f32) -> [f32; 3] {
        if self.trans > 1e-3 {
            [fresnel_dielectric(cos, self.eta); 3]
        } else {
            fresnel_schlick(self.f0, cos)
        }
    }

    /// Lobe selection probabilities `(diffuse, microfacet)`; they sum to 1.
    ///
    /// Only two, not three: reflection and refraction share one half vector,
    /// and the sampler splits between them by the exact Fresnel term *at
    /// that half vector*. Weighting them beforehand from `wo.z` (which is
    /// what a three-way split has to do) makes `sample` and `eval` disagree
    /// on rough glass by however much `cos_oh` differs from `wo.z` — a
    /// mismatched pdf, which is bias, not noise. Splitting inside the lobe
    /// means `eval` can recover the same probability from the half vector it
    /// already has.
    fn lobe_probs(&self, wo: [f32; 3]) -> (f32, f32) {
        let cos = wo[2].clamp(1e-4, 1.0);
        let fr = luminance(self.fresnel(cos)).clamp(0.0, 1.0);
        // Give the microfacet lobe a floor so a matte dielectric's highlight
        // is not sampled once in fifty.
        let w_micro = if self.metallic > 0.0 {
            fr.max(0.1).max(self.metallic)
        } else {
            fr.max(0.12) + self.trans * (1.0 - fr)
        };
        let w_diff = self.diff * luminance(self.albedo).max(0.02);
        let sum = w_micro + w_diff;
        if sum <= 0.0 {
            return (0.0, 1.0);
        }
        (w_diff / sum, w_micro / sum)
    }

    /// Next-event estimation is worth a shadow ray only when some lobe is
    /// wide enough to catch a light of finite size. A delta lobe never is
    /// (`eval` returns 0 for it, so the ray would be pure cost), and the
    /// diffuse remainder beside one is worth the traversal only when it
    /// returns more than a tenth of a matte surface's: the glass shroud of
    /// `examples/impeller.star` keeps a 10 % diffuse lobe at opacity 0.10,
    /// and its shadow rays were a quarter of that render for a tenth of a
    /// matte surface's light.
    /// Skipped, the lobe's share is carried by the BSDF path with weight 1
    /// ([`Tracer::mis_weight`]'s `nee` flag), so nothing is lost.
    #[inline]
    fn nee_useful(&self) -> bool {
        if !self.is_delta() {
            return true;
        }
        self.diff * luminance(self.albedo) >= NEE_MIN_DIFFUSE
    }

    /// `(f, pdf)` for an explicit direction pair. Delta lobes evaluate to
    /// zero here, which is what makes next-event estimation on a mirror a
    /// no-op instead of a bias.
    fn eval(&self, wo: [f32; 3], wi: [f32; 3]) -> ([f32; 3], f32) {
        let (p_diff, p_micro) = self.lobe_probs(wo);
        let transmissive = self.trans > 1e-4;
        let mut f = [0.0f32; 3];
        let mut pdf = 0.0f32;
        if wi[2] > 0.0 {
            if self.diff > 1e-4 {
                let fr = self.fresnel(wi[2]);
                let kd = [1.0 - fr[0], 1.0 - fr[1], 1.0 - fr[2]];
                f = add(f, mul(scale(self.albedo, self.diff * INV_PI), kd));
                pdf += p_diff * wi[2] * INV_PI;
            }
            if !self.is_delta() {
                let wh = normalize(add(wo, wi));
                if wh[2] > 0.0 {
                    let cos_oh = dot(wo, wh).max(1e-6);
                    let d = ggx_d(wh, self.ax, self.ay);
                    let g = smith_g2(wo, wi, self.ax, self.ay);
                    let fr = self.fresnel(cos_oh);
                    let s = d * g / (4.0 * wo[2] * wi[2]).max(1e-9);
                    f = add(f, scale(fr, s));
                    // The sampler reaches this direction by picking the
                    // microfacet lobe and then reflecting rather than
                    // refracting, which it decides with this exact Fresnel.
                    let p_r = if transmissive { fresnel_dielectric(cos_oh, self.eta) } else { 1.0 };
                    pdf += p_micro * p_r * vndf_pdf(wo, wh, self.ax, self.ay) / (4.0 * cos_oh);
                }
            }
        } else if wi[2] < 0.0 && transmissive && !self.is_delta() {
            // Walter et al. generalized half vector, oriented into +z.
            let mut wh = normalize(scale(add(wo, scale(wi, 1.0 / self.eta)), -1.0));
            if wh[2] < 0.0 {
                wh = scale(wh, -1.0);
            }
            let cos_oh = dot(wo, wh);
            let cos_ih = dot(wi, wh);
            if cos_oh > 0.0 && cos_ih < 0.0 {
                let denom = cos_oh + cos_ih / self.eta;
                let denom2 = (denom * denom).max(1e-12);
                let d = ggx_d(wh, self.ax, self.ay);
                let g = smith_g2(wo, wi, self.ax, self.ay);
                let fr = fresnel_dielectric(cos_oh, self.eta);
                let v = (cos_ih.abs() * cos_oh) / (wo[2] * wi[2].abs()).max(1e-9) * (1.0 - fr) * g * d / denom2;
                f = add(f, [self.trans * v; 3]);
                let jac = cos_ih.abs() / (self.eta * self.eta) / denom2;
                pdf += p_micro * (1.0 - fr) * vndf_pdf(wo, wh, self.ax, self.ay) * jac;
            }
        }
        (f, pdf.max(0.0))
    }

    fn sample(&self, wo: [f32; 3], u_lobe: f32, u1: f32, u2: f32) -> Option<BsdfSample> {
        if wo[2] <= 0.0 {
            return None;
        }
        let (p_diff, p_micro) = self.lobe_probs(wo);
        let delta = self.is_delta();

        // --- Delta dielectric / mirror: sample reflection or refraction
        // exactly, no VNDF noise.
        if delta {
            let wh = [0.0, 0.0, 1.0];
            if self.trans > 1e-4 {
                let fr = fresnel_dielectric(wo[2], self.eta);
                // Split by Fresnel, which makes the weight exactly 1.
                if u_lobe < fr {
                    let wi = [-wo[0], -wo[1], wo[2]];
                    return Some(BsdfSample { wi, weight: [1.0; 3], pdf: 1.0, specular: true, transmitted: false });
                }
                let wi = refract(wo, wh, self.eta)?;
                let e2 = self.eta * self.eta;
                return Some(BsdfSample { wi, weight: [e2; 3], pdf: 1.0, specular: true, transmitted: true });
            }
            if u_lobe >= p_diff {
                let wi = [-wo[0], -wo[1], wo[2]];
                let fr = self.fresnel(wo[2]);
                return Some(BsdfSample {
                    wi,
                    weight: scale(fr, 1.0 / p_micro.max(1e-4)),
                    pdf: 1.0,
                    specular: true,
                    transmitted: false,
                });
            }
            // fall through to the diffuse lobe below
        }

        // --- Diffuse.
        if u_lobe < p_diff {
            let wi = cosine_hemisphere(u1, u2);
            let (f, pdf) = self.eval(wo, wi);
            if pdf <= 0.0 {
                return None;
            }
            return Some(BsdfSample { wi, weight: scale(f, wi[2] / pdf), pdf, specular: false, transmitted: false });
        }

        // --- Microfacet: one half vector, then reflect or refract through it.
        let wh = sample_vndf(wo, self.ax, self.ay, u1, u2);
        let cos_oh = dot(wo, wh);
        if cos_oh <= 0.0 {
            return None;
        }
        let reflect_only = self.trans <= 1e-4;
        let choose_reflect = if reflect_only {
            true
        } else {
            // Within the microfacet lobe, split by Fresnel at *this* half
            // vector, which is exactly what `eval` assumes (see lobe_probs).
            let fr = fresnel_dielectric(cos_oh, self.eta);
            let u = ((u_lobe - p_diff) / p_micro.max(1e-6)).clamp(0.0, ONE_MINUS_EPS);
            u < fr
        };
        if choose_reflect {
            let wi = sub(scale(wh, 2.0 * cos_oh), wo);
            if wi[2] <= 0.0 {
                return None;
            }
            let (f, pdf) = self.eval(wo, wi);
            if pdf <= 0.0 || is_black(f) {
                return None;
            }
            Some(BsdfSample { wi, weight: scale(f, wi[2] / pdf), pdf, specular: false, transmitted: false })
        } else {
            let wi = match refract(wo, wh, self.eta) {
                Some(wi) if wi[2] < 0.0 => wi,
                // Total internal reflection off this microfacet.
                _ => {
                    let wi = sub(scale(wh, 2.0 * cos_oh), wo);
                    if wi[2] <= 0.0 {
                        return None;
                    }
                    let (f, pdf) = self.eval(wo, wi);
                    if pdf <= 0.0 {
                        return None;
                    }
                    return Some(BsdfSample { wi, weight: scale(f, wi[2] / pdf), pdf, specular: false, transmitted: false });
                }
            };
            let (f, pdf) = self.eval(wo, wi);
            if pdf <= 0.0 || is_black(f) {
                return None;
            }
            Some(BsdfSample { wi, weight: scale(f, wi[2].abs() / pdf), pdf, specular: false, transmitted: true })
        }
    }
}

// ---------------------------------------------------------------------------
// Participating medium inside a translucent solid.

/// Homogeneous interior of a transmissive material: Beer-Lambert absorption
/// tinted by `base_color`, plus optional isotropic-ish scattering for milky
/// filaments (a random walk, the standard way to get subsurface look without
/// a diffusion approximation).
#[derive(Clone, Copy)]
struct Medium {
    sigma_s: [f32; 3],
    /// `sigma_a + sigma_s`. The absorption half is what Beer-Lambert needs;
    /// the scattering half is what the random walk needs; every use wants
    /// the sum, so only the sum and the scattering coefficient are kept.
    sigma_t: [f32; 3],
    g: f32,
    scattering: bool,
}

impl Medium {
    fn new(m: &Pbr) -> Option<Medium> {
        let has_abs = m.attenuation_distance > 0.0 && m.attenuation_distance.is_finite();
        let has_sca = m.scatter_distance > 0.0 && m.scatter_distance.is_finite();
        // A guide body has no interior: a medium only the camera's
        // specular chain can enter has no wall to leave through once the
        // path is light transport, which skips the body. So `attenuation=`
        // and `scatter=` do nothing on one, and it is a pair of interfaces.
        if (!has_abs && !has_sca) || !m.shadow {
            return None;
        }
        // Transmittance over `attenuation_distance` equals base_color.
        let sigma_a = if has_abs {
            m.base_color.map(|c| -(c.clamp(1e-3, 0.999).ln()) / m.attenuation_distance)
        } else {
            [0.0; 3]
        };
        let sigma_s = if has_sca { [1.0 / m.scatter_distance; 3] } else { [0.0; 3] };
        let sigma_t = add(sigma_a, sigma_s);
        Some(Medium { sigma_s, sigma_t, g: m.scatter_g.clamp(-0.95, 0.95), scattering: has_sca })
    }

    /// Analytic transmittance over `d`.
    #[inline]
    fn transmittance(&self, d: f32) -> [f32; 3] {
        [(-self.sigma_t[0] * d).exp(), (-self.sigma_t[1] * d).exp(), (-self.sigma_t[2] * d).exp()]
    }
}

/// Henyey-Greenstein phase function of the angle between the direction of
/// travel and the scattered direction. It integrates to 1 over the sphere,
/// so this is also the pdf [`sample_hg`] draws from — which is what lets
/// next-event estimation inside a medium be multiple-importance-sampled
/// against the phase function exactly as a surface is against its BSDF.
#[inline]
fn hg_phase(cos_t: f32, g: f32) -> f32 {
    let g = g.clamp(-0.95, 0.95);
    let d = (1.0 + g * g - 2.0 * g * cos_t.clamp(-1.0, 1.0)).max(1e-6);
    (1.0 - g * g) / (4.0 * PI * d * d.sqrt())
}

/// Henyey-Greenstein phase sampling about `w` (the direction of travel).
fn sample_hg(w: [f32; 3], g: f32, u1: f32, u2: f32) -> [f32; 3] {
    let cos_t = if g.abs() < 1e-3 {
        1.0 - 2.0 * u1
    } else {
        let s = (1.0 - g * g) / (1.0 + g - 2.0 * g * u1);
        -(1.0 + g * g - s * s) / (2.0 * g)
    }
    .clamp(-1.0, 1.0);
    let sin_t = (1.0 - cos_t * cos_t).max(0.0).sqrt();
    let phi = TAU * u2;
    let f = Onb::layer_aligned(w);
    f.to_world([sin_t * phi.cos(), sin_t * phi.sin(), cos_t])
}

// ---------------------------------------------------------------------------
// Lights.

/// One of the studio rig's three lights, as a cone of directions of finite
/// angular radius — a distant area light, which is what makes the shadows
/// soft. `radiance` is scaled by the solid angle so the rig's total
/// irradiance does not change when the size does.
#[derive(Clone, Copy)]
struct ConeLight {
    dir: [f32; 3],
    radiance: [f32; 3],
    cos_max: f32,
    pdf: f32,
    /// Probability that [`Tracer::pick_light`] chooses this light, and so
    /// the factor its solid-angle pdf is multiplied by. See `cone_pick`.
    q: f32,
    /// The frame `sample` builds its direction in. It is a pure function of
    /// `dir`, which never changes, so it is built once here rather than on
    /// every shadow ray — `Onb::layer_aligned` is a cross, a normalize and
    /// a second cross, and next-event estimation asks for one of these at
    /// every bounce of every path.
    frame: Onb,
}

impl ConeLight {
    fn new(dir: [f32; 3], irradiance: [f32; 3], half_angle: f32) -> ConeLight {
        let cos_max = half_angle.cos();
        let solid = (TAU * (1.0 - cos_max)).max(1e-6);
        let dir = normalize(dir);
        ConeLight {
            dir,
            radiance: scale(irradiance, 1.0 / solid),
            cos_max,
            pdf: 1.0 / solid,
            // Filled in by `Tracer::new` once every cone is known.
            q: 0.0,
            frame: Onb::layer_aligned(dir),
        }
    }

    fn sample(&self, u1: f32, u2: f32) -> ([f32; 3], f32) {
        let cos_t = 1.0 - u1 * (1.0 - self.cos_max);
        let sin_t = (1.0 - cos_t * cos_t).max(0.0).sqrt();
        let phi = TAU * u2;
        (self.frame.to_world([sin_t * phi.cos(), sin_t * phi.sin(), cos_t]), self.pdf)
    }
}

/// The environment gradient as a light next-event estimation can aim at.
///
/// The gradient is a function of `dir.z` alone, so its distribution over
/// the sphere is one dimension: a table of the luminance in [`ENV_BINS`]
/// bins of `z` (uniform in `z` is uniform on the sphere), sampled by its
/// CDF with the azimuth uniform. Before this the sky was reached only by
/// following the BSDF, at weight 1; with it a glossy surface gets a second,
/// MIS-weighted estimate of the sky it reflects, which is most of what a
/// metal under the product surround reflects. In a furnace the table is
/// flat and the strategy is uniform sphere sampling, still unbiased.
struct EnvSampler {
    /// Cumulative distribution over the bins, `cdf[0] = 0`, `cdf[N] = 1`.
    cdf: [f32; ENV_BINS + 1],
    /// Density over `z` per bin, so the solid-angle pdf is this over 2π.
    pdf_z: [f32; ENV_BINS],
    /// Irradiance the whole sky delivers to an up-facing plane, the
    /// "power" its share of the shadow rays is set by.
    power: f32,
    /// Probability [`Tracer::pick_light`] chooses it. See `ConeLight::q`.
    q: f32,
}

impl EnvSampler {
    fn new(lights: &Lights) -> Option<EnvSampler> {
        let dz = 2.0 / ENV_BINS as f32;
        let mut l = [0.0f32; ENV_BINS];
        let mut total = 0.0f32;
        let mut power = 0.0f32;
        for (i, v) in l.iter_mut().enumerate() {
            let z = -1.0 + (i as f32 + 0.5) * dz;
            *v = luminance(env(lights, [(1.0 - z * z).max(0.0).sqrt(), 0.0, z])).max(0.0);
            total += *v * dz;
            if z > 0.0 {
                power += *v * z * dz * TAU;
            }
        }
        if !(total > 0.0) {
            return None;
        }
        let mut cdf = [0.0f32; ENV_BINS + 1];
        let mut pdf_z = [0.0f32; ENV_BINS];
        for i in 0..ENV_BINS {
            pdf_z[i] = l[i] / total;
            cdf[i + 1] = cdf[i] + l[i] * dz / total;
        }
        cdf[ENV_BINS] = 1.0;
        Some(EnvSampler { cdf, pdf_z, power, q: 0.0 })
    }

    /// A direction and its solid-angle pdf (before the pick probability).
    fn sample(&self, u1: f32, u2: f32) -> ([f32; 3], f32) {
        let i = match self.cdf[1..].binary_search_by(|c| c.partial_cmp(&u1).unwrap_or(std::cmp::Ordering::Less)) {
            Ok(i) => i + 1,
            Err(i) => i,
        }
        .min(ENV_BINS - 1);
        let (c0, c1) = (self.cdf[i], self.cdf[i + 1]);
        let t = if c1 > c0 { ((u1 - c0) / (c1 - c0)).clamp(0.0, ONE_MINUS_EPS) } else { 0.5 };
        let dz = 2.0 / ENV_BINS as f32;
        let z = (-1.0 + (i as f32 + t) * dz).clamp(-1.0, 1.0);
        let r = (1.0 - z * z).max(0.0).sqrt();
        let phi = TAU * u2;
        ([r * phi.cos(), r * phi.sin(), z], self.pdf_z[i] / TAU)
    }

    /// The solid-angle pdf `sample` has for `dir`, before the pick
    /// probability.
    #[inline]
    fn pdf(&self, dir: [f32; 3]) -> f32 {
        let i = (((dir[2].clamp(-1.0, 1.0) + 1.0) * 0.5 * ENV_BINS as f32) as usize).min(ENV_BINS - 1);
        self.pdf_z[i] / TAU
    }
}

/// An emissive triangle, picked with probability proportional to its
/// emitted power `area * luminance(emissive)` and then sampled uniformly
/// over its surface.
///
/// The area is not stored: picking by power and then uniformly over the
/// triangle gives an area-measure density of
/// `(area_i * L_i / total) / area_i = L_i / total`, so the areas cancel and
/// only `Tracer::tri_power` is needed at sampling time.
#[derive(Clone, Copy)]
struct TriLight {
    /// Triangle slot in the BVH (`Bvh::tri`).
    tri: u32,
    emissive: [f32; 3],
}

// ---------------------------------------------------------------------------
// Edge break.

/// One crease of a mesh: a segment, and the geometric normals of the two
/// triangles that fold across it.
#[derive(Clone, Copy)]
struct Crease {
    a: [f32; 3],
    b: [f32; 3],
    n1: [f32; 3],
    n2: [f32; 3],
}

/// The bevel shader's index over one body's crease edges
/// (`Pbr::edge_break`, `PathTraceOptions::edge_break_mm`).
///
/// A real machined part has no razor edges: every one is deburred to a
/// 0.1–0.3 mm round, and those rounds are what catch the light as the fine
/// bright lines that make a photograph of a billet part read as metal. The
/// geometry is left alone; instead, within the radius of a crease the
/// shading normal turns toward the neighbouring face, reaching the two
/// faces' bisector on the edge itself — the normal field of a round-over of
/// that radius, without modelling one.
///
/// The creases are the mesh's own edges whose two triangles fold by at
/// least the crease angle, not the topology's polylines: a crease needs the
/// normals on both sides of it, which the mesh has and a polyline does not;
/// it makes a mesh body's creases the same thing as a B-rep's; and the rule
/// that decides what is a crease is then exactly the line overlay's and
/// `Mesh::smooth_seams`' (a fold past the crease angle), so what is drawn
/// as an edge is what is bevelled as one. Positions are welded at the
/// tolerance `smooth_seams` uses, because the faces of a B-rep body are
/// tessellated separately and agree along a shared edge only to that.
///
/// The segments sit in a uniform grid of cells `2r` wide, each filed under
/// every cell within `r` of it, so the query for a shading point is one
/// cell lookup and a few segment distances. Nothing is built when the
/// radius is zero, which is the default, so a render without it pays one
/// branch per hit.
struct EdgeBreak {
    radius: f32,
    inv_cell: f32,
    creases: Vec<Crease>,
    /// Cell -> `refs[start..end]`, indices into `creases`.
    cells: HashMap<[i32; 3], (u32, u32)>,
    refs: Vec<u32>,
}

impl EdgeBreak {
    fn build(mesh: &crate::tessellate::Mesh, radius: f32, crease_degrees: f32) -> Option<EdgeBreak> {
        if !(radius > 0.0) || !radius.is_finite() || mesh.indices.len() < 3 {
            return None;
        }
        let nv = mesh.positions.len() / 3;
        let pos = |i: usize| [mesh.positions[i * 3], mesh.positions[i * 3 + 1], mesh.positions[i * 3 + 2]];
        // Weld positions the way `Mesh::smooth_seams` does: a thousandth of
        // a percent of the extent, floored at the mesh kernel's tolerance,
        // with the 27 neighbouring cells searched so a point on a cell
        // boundary still finds its twin. A vertex's class is the index of
        // the first vertex found at its position.
        let (lo, hi) = mesh.bbox()?;
        let eps = ((hi - lo).length() as f32 * 1e-5).max(crate::mesh_kernel::WELD_EPS);
        let q = |v: f32| (v / eps).floor() as i64;
        let mut weld: HashMap<(i64, i64, i64), Vec<u32>> = HashMap::new();
        let mut class: Vec<u32> = Vec::with_capacity(nv);
        for i in 0..nv {
            let p = pos(i);
            let key = (q(p[0]), q(p[1]), q(p[2]));
            let mut found = None;
            'search: for dx in -1..=1 {
                for dy in -1..=1 {
                    for dz in -1..=1 {
                        if let Some(ids) = weld.get(&(key.0 + dx, key.1 + dy, key.2 + dz)) {
                            for &id in ids {
                                let d = sub(pos(id as usize), p);
                                if dot(d, d) <= eps * eps {
                                    found = Some(class[id as usize]);
                                    break 'search;
                                }
                            }
                        }
                    }
                }
            }
            let c = found.unwrap_or_else(|| {
                weld.entry(key).or_default().push(i as u32);
                i as u32
            });
            class.push(c);
        }
        // Every welded edge with the geometric normal of each triangle on
        // it. A BTreeMap, so the creases come out in one order however the
        // hashes fall: the query below breaks a tie by index, and an image
        // must not depend on which run built the tree.
        let mut edges: std::collections::BTreeMap<(u32, u32), Vec<[f32; 3]>> = std::collections::BTreeMap::new();
        // A triangle thinner than the weld tolerance (the ring round a
        // sphere's pole, where every vertex is within rounding of the pole)
        // has a normal that is rounding error, and no edge to fold on.
        let min_area2 = (eps * eps) * (eps * eps);
        for t in mesh.indices.chunks_exact(3) {
            let idx = [t[0] as usize, t[1] as usize, t[2] as usize];
            let n = cross(sub(pos(idx[1]), pos(idx[0])), sub(pos(idx[2]), pos(idx[0])));
            if dot(n, n) <= min_area2 {
                continue;
            }
            let n = normalize(n);
            for k in 0..3 {
                let (ca, cb) = (class[idx[k]], class[idx[(k + 1) % 3]]);
                if ca == cb {
                    continue;
                }
                let key = if ca < cb { (ca, cb) } else { (cb, ca) };
                edges.entry(key).or_default().push(n);
            }
        }
        let cos_max = crease_degrees.clamp(0.0, 180.0).to_radians().cos();
        let mut creases = Vec::new();
        for ((ca, cb), normals) in &edges {
            if normals.len() < 2 {
                continue;
            }
            // The pair that folds the most: on a doubled wall the coplanar
            // twin must not hide the real corner behind it.
            let mut best: Option<([f32; 3], [f32; 3], f32)> = None;
            for i in 0..normals.len() {
                for j in i + 1..normals.len() {
                    let c = dot(normals[i], normals[j]);
                    if best.is_none_or(|b| c < b.2) {
                        best = Some((normals[i], normals[j], c));
                    }
                }
            }
            let (n1, n2, c) = best.unwrap();
            // A fold of less than the crease angle is a seam that shades
            // smooth already; it gets no break.
            if c > cos_max {
                continue;
            }
            creases.push(Crease { a: pos(*ca as usize), b: pos(*cb as usize), n1, n2 });
        }
        if creases.is_empty() {
            return None;
        }
        let inv_cell = 1.0 / (2.0 * radius);
        let mut buckets: HashMap<[i32; 3], Vec<u32>> = HashMap::new();
        for (i, c) in creases.iter().enumerate() {
            let lo = [0, 1, 2].map(|k| ((c.a[k].min(c.b[k]) - radius) * inv_cell).floor() as i32);
            let hi = [0, 1, 2].map(|k| ((c.a[k].max(c.b[k]) + radius) * inv_cell).floor() as i32);
            for x in lo[0]..=hi[0] {
                for y in lo[1]..=hi[1] {
                    for z in lo[2]..=hi[2] {
                        buckets.entry([x, y, z]).or_default().push(i as u32);
                    }
                }
            }
        }
        let mut cells = HashMap::with_capacity(buckets.len());
        let mut refs = Vec::new();
        for (key, list) in buckets {
            let start = refs.len() as u32;
            refs.extend(list);
            cells.insert(key, (start, refs.len() as u32));
        }
        Some(EdgeBreak { radius, inv_cell, creases, cells, refs })
    }

    /// The nearest crease within the radius of `p` whose two faces include
    /// the one `ng` is the normal of: `(distance, the normal of the other
    /// face)`. Being near a crease is not enough — a point across a thin
    /// wall from an edge is within the radius and not on the edge — so a
    /// crease counts only when one of its normals is this face's.
    fn nearest(&self, p: [f32; 3], ng: [f32; 3]) -> Option<(f32, [f32; 3])> {
        let key = [0, 1, 2].map(|k| (p[k] * self.inv_cell).floor() as i32);
        let &(start, end) = self.cells.get(&key)?;
        let r2 = self.radius * self.radius;
        let mut best: Option<(f32, [f32; 3])> = None;
        for &i in &self.refs[start as usize..end as usize] {
            let c = &self.creases[i as usize];
            let (d1, d2) = (dot(ng, c.n1), dot(ng, c.n2));
            if d1.max(d2) < 0.9 {
                continue;
            }
            let ab = sub(c.b, c.a);
            let t = (dot(sub(p, c.a), ab) / dot(ab, ab).max(1e-12)).clamp(0.0, 1.0);
            let dq = sub(p, add(c.a, scale(ab, t)));
            let dist2 = dot(dq, dq);
            if dist2 < r2 && best.is_none_or(|b| dist2 < b.0) {
                best = Some((dist2, if d1 >= d2 { c.n2 } else { c.n1 }));
            }
        }
        best.map(|(d2, n)| (d2.sqrt(), n))
    }

    /// The shading normal at `p` with the break applied: `ns` turned toward
    /// the neighbouring face along the profile of a round of the radius,
    /// so it is `ns` at the radius and the bisector on the edge. `ns` has
    /// already been flipped toward the ray; `entering` says which side of
    /// the surface that is, so the neighbour's normal is flipped alike. A
    /// bisector seen from a grazing angle can turn past the viewer, which
    /// would black the pixel; it is bent back to face the ray by a hair,
    /// the way any bump map has to be.
    ///
    /// The profile is the round's own, not a straight blend. A round of
    /// radius `r` tangent to this face at distance `r` from the edge has,
    /// at distance `d` along the face, the normal of the arc point above
    /// it: turned by `asin(1 - d / r)`, slowly where the arc leaves the
    /// face and quickly at the edge, reaching this face's half of the turn
    /// (the bisector) at `d = r·(1 - sin(half the turn))` — the arc's other
    /// half hangs over the neighbouring face, which shades it. Inside that
    /// the bisector holds. A blend that is linear in `d` was tried first
    /// and is a *chamfer*: its normal is nearly the same across the whole
    /// width, so every edge carried a uniform bright stripe `r` wide that
    /// read as an outline drawn on the part, where a round catches the
    /// light on a thin line near the edge and fades into the face.
    fn bevel(&self, p: [f32; 3], ns: [f32; 3], ng: [f32; 3], dir: [f32; 3], entering: bool) -> [f32; 3] {
        let Some((d, other)) = self.nearest(p, ng) else { return ns };
        let other = if entering { other } else { scale(other, -1.0) };
        // Half the fold between the two faces, in radians: the turn this
        // face's side of the round makes before the bisector. The turn at
        // `d` is the round's, capped there; the normal is `ns` rotated by
        // it toward `other` in their common plane (a knife edge, whose two
        // normals are opposite, has no such plane and keeps `ns`).
        let c = dot(ns, other).clamp(-1.0, 1.0);
        let half = 0.5 * c.acos();
        let turn = (1.0 - d / self.radius).clamp(0.0, 1.0).asin().min(half);
        let perp = sub(other, scale(ns, c));
        let n = if turn > 1e-5 && dot(perp, perp) > 1e-10 {
            normalize(add(scale(ns, turn.cos()), scale(normalize(perp), turn.sin())))
        } else {
            ns
        };
        let v = scale(dir, -1.0);
        let c = dot(n, v);
        if c < 0.02 { normalize(add(n, scale(v, 0.02 - c))) } else { n }
    }
}

// ---------------------------------------------------------------------------
// Scene.

/// A surface the ray hit.
struct Surf {
    p: [f32; 3],
    /// Geometric normal, unit, in mesh orientation.
    ng: [f32; 3],
    /// Shading normal, unit, flipped toward the incoming ray.
    ns: [f32; 3],
    /// The ray arrived from the outside of the surface.
    entering: bool,
    mat: Pbr,
    /// BVH triangle slot, or `u32::MAX` for the ground plane.
    tri: u32,
}

/// One unfinished branch of a path: everything [`Tracer::trace_segment`]
/// needs to pick it up later. A smooth dielectric interface pushes one of
/// these instead of choosing between reflection and refraction.
#[derive(Clone, Copy)]
struct Seg {
    o: [f32; 3],
    d: [f32; 3],
    beta: [f32; 3],
    /// Scattering events already taken along this branch.
    depth: u32,
    /// The medium this branch starts inside, if any.
    medium: Option<Medium>,
    /// Distance travelled so far, for the texture footprint.
    travelled: f32,
    /// Scattering events already taken inside a medium.
    walk: u32,
    /// The event that created this branch was a delta lobe, so light found
    /// along it carries MIS weight 1.
    specular: bool,
    prev_pdf: f32,
    /// Every event so far was a transmission: the branch is still the
    /// camera's look through see-through walls, so what it finds behind
    /// them is what is behind the model.
    through: bool,
    /// This branch has already scattered off something that was not a delta
    /// lobe, so a light reached from here through a chain of mirrors is a
    /// caustic. See [`Tracer::trace_segment`].
    caustic: bool,
    /// Next-event estimation ran at the vertex this branch left, so a light
    /// found by following the BSDF competes with it in the MIS weight; when
    /// it did not (`Bsdf::nee_useful`), the BSDF path is the only estimator
    /// and carries weight 1.
    nee: bool,
}

pub(crate) struct Tracer<'a> {
    items: &'a [Item<'a>],
    mats: Vec<Pbr>,
    /// Per item, whether a shadow ray stops at it: `!is_transmissive`, in
    /// the form the BVH's shadow traversal indexes by `Tri::item`.
    opaque: Vec<bool>,
    /// Per item, whether it is guide geometry (`!Pbr::shadow`) —
    /// `None` when nothing is, so every traversal is the one it always was
    /// and a scene without guides renders to the bit as before. With
    /// guides, the camera's specular chain walks the whole BVH and light
    /// transport walks it through the `_where` variants that skip them:
    /// one tree, so the emissive triangles' slots stay one numbering.
    guide: Option<Vec<bool>>,
    bvh: Bvh,
    lights: Lights,
    cones: Vec<ConeLight>,
    /// The sky as a next-event strategy, when `env_sampling` asks for it.
    env_sampler: Option<EnvSampler>,
    tri_lights: Vec<TriLight>,
    /// Prefix sums of `area * luminance(emissive)`.
    tri_cdf: Vec<f32>,
    /// BVH triangle slot -> `tri_lights` index, `u32::MAX` = not a light.
    light_of_tri: Vec<u32>,
    /// Sum of `area * luminance(emissive)` over `tri_lights`, precomputed:
    /// both the light pdf and its MIS counterpart need it per shadow ray.
    tri_power: f32,
    ground: Option<Pbr>,
    ground_z: f32,
    ground_c: [f32; 2],
    /// The scene centre, for a machined texture on a body with no frames.
    ground_c3: [f32; 3],
    ground_r: f32,
    has_transmissive: bool,
    /// Number of next-event strategies: cones plus (emissive triangles as one).
    n_strategies: u32,
    /// Probability that [`Tracer::pick_light`] chooses the emissive-geometry
    /// strategy. The cones carry their own (`ConeLight::q`); together these
    /// sum to 1.
    tri_q: f32,
    pt: PathTraceOptions,
    /// `inverse_tonemap(background)`: what a camera ray that hits nothing
    /// contributes, so the backdrop lands on exactly `background`.
    bg_hdr: [f32; 3],
    /// [`RenderOptions::transparent`]: the backdrop is alpha, not colour.
    transparent: bool,
    /// The ground plane is a shadow catcher ([`Tracer::catch_shadow`]).
    catcher: bool,
    /// Per item, the crease index its edge break is shaded from; `None`
    /// for the bodies (all of them, by default) that have no break.
    edge_fields: Vec<Option<EdgeBreak>>,
    eps: f32,
    far: f32,
    bvh_stats: BvhStats,
}

/// The smallest offset that moves a point of `scene` to a different f32:
/// 64 ulps of its largest coordinate.
fn magnitude_eps(scene: &Scene) -> f32 {
    let m = scene.lo.iter().chain(scene.hi.iter()).fold(0.0f32, |m, v| m.max(v.abs()));
    m * f32::EPSILON * 64.0
}

impl<'a> Tracer<'a> {
    /// `azimuth` is the camera's (`View::azimuth`), which the product rig
    /// turns with; the studio rig ignores it.
    pub(crate) fn new(items: &'a [Item<'a>], scene: &Scene, opts: &RenderOptions, azimuth: f32) -> Tracer<'a> {
        let pt = opts.path.clone();
        let bvh = Bvh::build(items);
        let bvh_stats = bvh.stats;
        let mats: Vec<Pbr> = items.iter().map(item_material).collect();
        let opaque: Vec<bool> = mats.iter().map(|m| !m.is_transmissive()).collect();
        let guide: Option<Vec<bool>> =
            mats.iter().any(|m| !m.shadow).then(|| mats.iter().map(|m| !m.shadow).collect());
        let lights = if pt.furnace { Lights::furnace() } else { Lights::for_options(opts, azimuth) };
        // The option scales every light's own radius; at its default the
        // factor is exactly 1, so the studio cones are the 14° they were.
        let size = pt.light_angle_degrees / STUDIO_LIGHT_DEGREES;
        let mut cones = Vec::new();
        let direct = lights.direct_scale * pt.direct_scale.max(0.0);
        for l in [&lights.key, &lights.fill, &lights.rim] {
            let irradiance = scale(l.radiance, direct);
            if !is_black(irradiance) {
                let half = (l.radius_degrees * size).clamp(0.05, 60.0).to_radians();
                cones.push(ConeLight::new(l.dir, irradiance, half));
            }
        }
        // The sun is a fourth cone, at its own angular radius: the
        // option that scales the rig's cones is about the softboxes, and
        // the sun's disc is what it is. Its shadows get the sun's penumbra
        // -- about a centimetre of soft edge per metre of drop.
        if let Some(sun) = &lights.sun {
            let irradiance = scale(sun.radiance, direct);
            if !is_black(irradiance) {
                cones.push(ConeLight::new(sun.dir, irradiance, sun.radius_degrees.to_radians()));
            }
        }

        // Emissive triangles become area lights. An emissive guide
        // is not one: the camera sees it glow, and it lights nothing.
        let mut tri_lights = Vec::new();
        let mut tri_cdf = Vec::new();
        let mut light_of_tri = Vec::new();
        let mut tri_power = 0.0f32;
        let any_emissive = mats.iter().any(|m| m.emits() && m.shadow);
        if any_emissive {
            light_of_tri = vec![u32::MAX; bvh.len()];
            let mut total = 0.0f32;
            for (i, t) in bvh.triangles() {
                let m = &mats[t.item as usize];
                if !m.emits() || !m.shadow {
                    continue;
                }
                let area = t.area();
                if area <= 0.0 {
                    continue;
                }
                let power = area * luminance(m.emissive).max(1e-6);
                total += power;
                light_of_tri[i as usize] = tri_lights.len() as u32;
                tri_lights.push(TriLight { tri: i, emissive: m.emissive });
                tri_cdf.push(total);
            }
            if total > 0.0 {
                for c in &mut tri_cdf {
                    *c /= total;
                }
            } else {
                tri_lights.clear();
                tri_cdf.clear();
            }
            tri_power = total;
        }

        // Without a backdrop there is no floor to see either: the ground
        // plane is there only as the shadow catcher's.
        let catcher = opts.transparent && opts.shadow_catcher;
        let ground = (if opts.transparent { catcher } else { pt.ground }).then(|| Pbr {
            base_color: pt.ground_albedo,
            roughness: pt.ground_roughness.clamp(0.03, 1.0),
            metallic: 0.0,
            ..Pbr::default()
        });
        // Over what a shadow ray can meet: a see-through guide is skipped,
        // so a scene whose only clear body is the envelope keeps the fast
        // opaque-only shadow test.
        let has_transmissive = mats.iter().any(|m| m.is_transmissive() && m.shadow);
        let mut env_sampler = if pt.env_sampling { EnvSampler::new(&lights) } else { None };
        let n_strategies = cones.len() as u32 + u32::from(env_sampler.is_some()) + u32::from(!tri_lights.is_empty());

        // How next-event estimation splits its one shadow ray between the
        // strategies. The cones and the sky keep the share they have
        // between them — `(cones + sky) / n_strategies`, so a scene with
        // emissive geometry is unchanged in how often that geometry is
        // sampled — but they divide it by *power* rather than equally.
        //
        // The rig is not a fair coin: the key light carries about 71% of the
        // irradiance, the rim 19% and the fill 9%. Picking one of the three
        // uniformly and dividing by 1/3 is unbiased but noisy, because the
        // variance of a one-sample discrete estimator is the sum of
        // `L_i^2 / q_i` and a uniform `q` pays `3 * sum(L_i^2)` where
        // `q_i ∝ L_i` pays `sum(L_i)^2`. For this rig that is 45.3 against
        // 75.7: a 40% cut in the variance of the direct-lighting term for no
        // extra work at all. Any positive `q` summing to 1 is unbiased; this
        // one is merely the good choice.
        //
        // What makes it *correct* rather than merely different is that the
        // two places which reconstruct this density for multiple importance
        // sampling — `escaped` for a cone found by following the BSDF, and
        // `tri_light_pdf` for an emissive triangle — use the same `q`. A `q`
        // here that they do not know about is bias, not noise.
        let n_sky = cones.len() + usize::from(env_sampler.is_some());
        let cone_share = if n_strategies == 0 { 0.0 } else { n_sky as f32 / n_strategies as f32 };
        let env_power = env_sampler.as_ref().map_or(0.0, |e| e.power);
        let total_power: f32 = cones.iter().map(|c| luminance(c.radiance) / c.pdf.max(1e-30)).sum::<f32>() + env_power;
        for c in cones.iter_mut() {
            // `radiance / pdf` is `irradiance`: the cone's radiance was
            // divided by its solid angle, and `pdf` is one over that angle.
            let w = luminance(c.radiance) / c.pdf.max(1e-30);
            c.q = if total_power > 0.0 { cone_share * (w / total_power) } else { cone_share / (n_sky as f32).max(1.0) };
        }
        if let Some(e) = env_sampler.as_mut() {
            e.q = if total_power > 0.0 { cone_share * (e.power / total_power) } else { cone_share / (n_sky as f32).max(1.0) };
        }
        let tri_q = if tri_lights.is_empty() || n_strategies == 0 { 0.0 } else { 1.0 / n_strategies as f32 };
        let bg_hdr = inverse_tonemap_rgb(opts.background, opts.exposure, opts.tonemap);
        // A body's own radius wins over the render's; zero either way is no
        // index at all, which is what makes the default free.
        let edge_fields = items
            .iter()
            .zip(&mats)
            .map(|(it, m)| {
                let radius = if m.edge_break > 0.0 { m.edge_break } else { pt.edge_break_mm };
                EdgeBreak::build(it.mesh, radius, opts.edge_crease_degrees)
            })
            .collect();
        Tracer {
            items,
            mats,
            opaque,
            guide,
            bvh,
            lights,
            cones,
            env_sampler,
            tri_lights,
            tri_cdf,
            light_of_tri,
            tri_power,
            ground,
            ground_z: scene.ground_z,
            ground_c: [scene.center[0], scene.center[1]],
            ground_c3: scene.center,
            // Large enough that its rim is never in shot: a disc a few
            // times the scene shows up as an arc across the middle of the
            // frame, which reads as a mistake, while one this size puts the
            // edge at the true horizon line where a floor belongs.
            ground_r: scene.diag * 60.0,
            has_transmissive,
            n_strategies,
            tri_q,
            pt,
            bg_hdr,
            transparent: opts.transparent,
            catcher,
            edge_fields,
            // Relative to the scene's size, and never below what f32 can
            // tell apart at the scene's distance from the origin (about
            // 64 ulps there): a hit point 1e7 mm out is only known to
            // ~1 mm, so a size-only epsilon of a few microns let every
            // shadow and bounce ray hit the surface it left.
            eps: (scene.diag * 1e-4).max(magnitude_eps(scene)).max(1e-5),
            far: scene.diag * 1e4 + 1e4,
            bvh_stats,
        }
    }

    // --- intersection ------------------------------------------------------

    /// Closest surface along the ray, including the implicit ground plane.
    /// `travelled` is the path's length so far, so the texture footprint at
    /// the hit is the pixel's size *there*, `travelled + t` from the eye.
    /// Taken at the start of the segment instead, a camera ray's footprint
    /// would be the eye's, nothing, and every texture the camera sees would
    /// be sampled unfiltered.
    ///
    /// `camera` is whether the ray is still the camera's own view — the
    /// specular chain from the eye — which sees guide geometry;
    /// light transport does not, and takes [`Tracer::intersect_light`].
    #[allow(clippy::too_many_arguments)]
    fn intersect(&self, o: [f32; 3], d: [f32; 3], tmin: f32, tmax: f32, travelled: f32, view: &View, camera: bool) -> Option<Surf> {
        let mut best: Option<Surf> = None;
        let mut tbest = tmax;
        let hit = if camera { self.bvh.intersect(o, d, tmin, tbest) } else { self.intersect_light(o, d, tmin, tbest) };
        if let Some(h) = hit {
            tbest = h.t;
            let tri = self.bvh.tri(h.tri);
            let it = &self.items[tri.item as usize];
            let ng = normalize(tri.geometric_normal());
            let p = tri.point(h.u, h.v);
            // Interpolated (smooth) shading normal from the mesh.
            let mesh = it.mesh;
            let i0 = tri.tri as usize * 3;
            let ns = if mesh.normals.len() >= mesh.positions.len() && i0 + 2 < mesh.indices.len() {
                let nv = |k: usize| {
                    let i = mesh.indices[i0 + k] as usize * 3;
                    [mesh.normals[i], mesh.normals[i + 1], mesh.normals[i + 2]]
                };
                let w = 1.0 - h.u - h.v;
                let n = add(add(scale(nv(0), w), scale(nv(1), h.u)), scale(nv(2), h.v));
                if dot(n, n) > 1e-12 { normalize(n) } else { ng }
            } else {
                ng
            };
            let entering = dot(d, ng) < 0.0;
            let ns = if dot(ns, d) > 0.0 { scale(ns, -1.0) } else { ns };
            let ns = match &self.edge_fields[tri.item as usize] {
                Some(eb) => eb.bevel(p, ns, ng, d, entering),
                None => ns,
            };
            let face = mesh.face_ids.get(tri.tri as usize).copied().unwrap_or(super::NO_FACE);
            // The width the textures resolve: `RESOLVE_PIXELS` pixels,
            // *on the surface*. A pixel `footprint` wide across the ray
            // covers `footprint / cos` of a surface seen at that angle to
            // the ray, in the direction the ray leans; the textures filter
            // with one width in every direction, so they take the long
            // one — a flank seen at a grazing angle is minified more, not
            // less, and its grooves must fade sooner. Capped at four times
            // the width so a silhouette does not ask for an infinite filter.
            let footprint = RESOLVE_PIXELS * view.footprint((travelled + h.t).max(1e-4)) / dot(ng, d).abs().max(0.25);
            let (mat, ns) = self.shade_material(tri.item as usize, face, tri.tri, p, ns, footprint);
            best = Some(Surf { p, ng, ns, entering, mat, tri: h.tri });
        }
        if let Some(g) = self.ground {
            if d[2].abs() > 1e-9 {
                let t = (self.ground_z - o[2]) / d[2];
                if t > tmin && t < tbest {
                    let p = add(o, scale(d, t));
                    let dx = p[0] - self.ground_c[0];
                    let dy = p[1] - self.ground_c[1];
                    if dx * dx + dy * dy <= self.ground_r * self.ground_r {
                        let ng = [0.0, 0.0, 1.0];
                        let ns = if d[2] > 0.0 { [0.0, 0.0, -1.0] } else { ng };
                        best = Some(Surf { p, ng, ns, entering: d[2] < 0.0, mat: g, tri: u32::MAX });
                    }
                }
            }
        }
        best
    }

    // --- what light can meet ---------------------------------------
    //
    // The three traversals below are the BVH's own with guide geometry
    // left out, and the BVH's own exactly when there is none: a scene
    // without a `shadow=False` body never takes a `_where` path.

    /// Closest hit for a ray that is light transport rather than the
    /// camera's view: a bounce, or the stepwise shadow ray.
    #[inline]
    fn intersect_light(&self, o: [f32; 3], d: [f32; 3], tmin: f32, tmax: f32) -> Option<Hit> {
        match &self.guide {
            None => self.bvh.intersect(o, d, tmin, tmax),
            Some(g) => self.bvh.intersect_where(o, d, tmin, tmax, |slot| !g[self.bvh.item_of(slot) as usize]),
        }
    }

    /// Whether anything a shadow ray stops at lies in `(tmin, tmax)`.
    #[inline]
    fn occluded_light(&self, o: [f32; 3], d: [f32; 3], tmin: f32, tmax: f32) -> bool {
        match &self.guide {
            None => self.bvh.occluded(o, d, tmin, tmax),
            Some(g) => self.bvh.occluded_where(o, d, tmin, tmax, |slot| !g[self.bvh.item_of(slot) as usize]),
        }
    }

    /// [`Bvh::shadow_hits`] over what a shadow ray can meet.
    #[inline]
    fn shadow_hits_light(&self, o: [f32; 3], d: [f32; 3], tmin: f32, tmax: f32, hits: &mut Vec<Hit>) -> ShadowHits {
        match &self.guide {
            None => self.bvh.shadow_hits(o, d, tmin, tmax, &self.opaque, hits, MAX_SHADOW_INTERFACES),
            Some(g) => self.bvh.shadow_hits_where(o, d, tmin, tmax, &self.opaque, hits, MAX_SHADOW_INTERFACES, |slot| {
                !g[self.bvh.item_of(slot) as usize]
            }),
        }
    }

    /// Material at a hit, with the procedural texture and the selection
    /// highlight applied exactly as `shading::surface_for` does, and the
    /// shading normal, which a machined texture tilts.
    fn shade_material(&self, item: usize, face: u32, tri: u32, p: [f32; 3], n: [f32; 3], footprint: f32) -> (Pbr, [f32; 3]) {
        let mut m = self.mats[item];
        let mut n = n;
        // An analysis view replaces the material with the colour of the
        // measured value under this triangle, and turns the material's
        // character off so the reading is the colour and nothing else —
        // the same rule `shading::surface_for` applies, so `--backend path
        // --analysis thickness` paints the same field the rasterizer does.
        let it = &self.items[item];
        if let Some(c) = it.tri_colors.get(tri as usize).filter(|_| it.paint) {
            // Paint changes the colour and nothing else.
            m.base_color = c.map(|v| v.clamp(0.0, 1.0));
        } else if let Some(c) = it.tri_colors.get(tri as usize) {
            m = Pbr {
                base_color: c.map(|v| v.clamp(0.0, 1.0)),
                roughness: 0.85,
                metallic: 0.0,
                ior: 1.5,
                anisotropy: 0.0,
                ..Pbr::default()
            };
        }
        // A scan's own colour, per vertex, under the material's —
        // unless an analysis view has replaced the material above.
        if self.items[item].tri_colors.is_empty() || self.items[item].paint {
            if let Some(v) = super::vertex_albedo(self.items[item].mesh, tri, p) {
                m.base_color = mul(m.base_color, v);
            }
        }
        if m.texture.is_relief() {
            // A machined finish: the grooves tilt the normal while they are
            // larger than a pixel, and are anisotropic roughness along
            // their direction once they are not (`Bsdf::new`).
            let it = &self.items[item];
            let frame = it.face_frames.get(face as usize).copied().unwrap_or(super::FaceFrame::fallback(self.ground_c3));
            let r = textures::machined(
                m.texture,
                m.texture_scale,
                m.texture_depth,
                p,
                n,
                footprint,
                frame.axis_point,
                frame.axis,
                frame.origin,
                frame.dir,
                frame.turned,
            );
            n = normalize(add(n, r.bump));
            m.groove = r.groove;
            m.slope_var = r.slope_var;
        } else if m.texture != textures::Kind::None {
            // The same call, with the same arguments, as
            // `shading::surface_for`, so the two renderers see identical texels.
            let t = textures::sample(m.texture, m.texture_scale, p, n, footprint);
            m.base_color = mul(m.base_color, t.albedo);
            m.roughness = (m.roughness + t.roughness_delta).clamp(0.03, 1.0);
        }
        if self.items[item].highlight_faces.contains(&face) {
            m.base_color = lerp3(m.base_color, HIGHLIGHT, 0.75);
            m.roughness = 0.5;
            m.metallic = 0.0;
            m.transmission = 0.0;
            m.emissive = add(m.emissive, scale(HIGHLIGHT, 0.15));
        }
        (m, n)
    }

    /// Shadow ray. Opaque geometry blocks; transmissive geometry tints and
    /// lets the ray through, so glass does not cast a black shadow.
    ///
    /// The ray is deliberately *not* bent: a shadow ray that refracts is a
    /// caustic, and caustics belong to the BSDF path, not to next-event
    /// estimation — bending it here would double-count them. What it does do
    /// is integrate Beer-Lambert over the distance the ray actually spends
    /// inside each solid, between an entry face and its matching exit, which
    /// is why a thick translucent wall shadows darker than a thin one.
    /// `start` is the medium the ray *begins* in, which is `Some` only for a
    /// shadow ray cast from a scattering event inside a solid: the
    /// transmittance from that point out to the wall is part of the answer.
    fn visibility(&self, o: [f32; 3], d: [f32; 3], dist: f32, start: Option<Medium>, hits: &mut Vec<Hit>) -> [f32; 3] {
        // Stop short of the light by more than the shading point was pushed
        // off its own surface. `offset_origin` moves the ray start toward
        // the light, so the distance actually left to travel is a little
        // less than `dist` — and a margin smaller than that lets the ray
        // hit the emissive triangle it is aiming at and report the light as
        // blocked by itself. That failure is invisible at grazing angles
        // and total when the light is straight overhead, which is exactly
        // the case a lamp in a ceiling is.
        let dist = dist - self.shadow_margin();
        if dist <= self.eps {
            return [1.0; 3];
        }
        if self.ground_blocks(o, d, dist) {
            return [0.0; 3];
        }
        if !self.has_transmissive {
            return if self.occluded_light(o, d, self.eps, dist) { [0.0; 3] } else { [1.0; 3] };
        }
        // One traversal collects every see-through crossing up to the first
        // opaque one, rather than a closest-hit search per interface: most
        // shadow rays through a thin glass shell cross two walls and then
        // find the sky, three traversals to learn what one can. The answer
        // is the stepwise search's, to the bit: the crossings are walked in
        // distance order below with the same rule about what to step over,
        // and the one case that rule cannot settle from what was collected
        // is handed back to the stepwise search.
        let blocked = match self.shadow_hits_light(o, d, self.eps, dist, hits) {
            ShadowHits::Blocked(t) => Some(t),
            ShadowHits::Through => None,
            ShadowHits::Overflow => return self.visibility_stepwise(o, d, dist, start),
        };
        // Stable, so two crossings at one distance keep the order the tree
        // found them in, which is the order the stepwise search took them.
        hits.sort_by(|a, b| a.t.partial_cmp(&b.t).unwrap_or(std::cmp::Ordering::Equal));
        let mut tint = [1.0f32; 3];
        let mut t0 = self.eps;
        let mut inside: Option<(Medium, f32)> = start.map(|m| (m, 0.0));
        for h in hits.iter() {
            // What the stepwise search stepped over: anything within `eps`
            // of the crossing before it, which is the same triangle again or
            // its coplanar twin.
            if h.t <= t0 {
                continue;
            }
            let tri = self.bvh.tri(h.tri);
            let m = &self.mats[tri.item as usize];
            let entering = dot(d, tri.geometric_normal()) < 0.0;
            match (inside, entering) {
                (Some((med, t_in)), false) => {
                    tint = mul(tint, med.transmittance((h.t - t_in).max(0.0)));
                    inside = None;
                }
                (None, true) => inside = Medium::new(m).map(|med| (med, h.t)),
                _ => {}
            }
            // One Fresnel-free stand-in per interface; a closed solid pays it
            // twice, on the way in and on the way out.
            tint = scale(tint, m.transmission);
            if max3(tint) < 1e-4 {
                return [0.0; 3];
            }
            t0 = h.t + self.eps;
        }
        match blocked {
            None => tint,
            Some(t) if t > t0 => [0.0; 3],
            // The opaque triangle is within `eps` behind the last crossing
            // accepted — a wall resting on the part it covers — and the
            // stepwise search stepped over it and carried on. What lies
            // beyond was never collected, so let that search have the ray;
            // it is a few rays in ten thousand on a scene built that way and
            // none at all on any other.
            Some(_) => self.visibility_stepwise(o, d, dist, start),
        }
    }

    /// [`Tracer::visibility`] the slow way, one closest-hit search per
    /// interface: for the ray that crosses more see-through triangles than
    /// the one-pass search allows for, and for the one whose blocker the
    /// one-pass walk cannot place. It is the original form of the function
    /// and defines the answer; the one-pass form reproduces it.
    fn visibility_stepwise(&self, o: [f32; 3], d: [f32; 3], dist: f32, start: Option<Medium>) -> [f32; 3] {
        let mut tint = [1.0f32; 3];
        let mut t0 = self.eps;
        let mut inside: Option<(Medium, f32)> = start.map(|m| (m, 0.0));
        // Bounded: a shadow ray that crosses 32 interfaces has told us all we
        // are going to learn about this light.
        for _ in 0..MAX_SHADOW_INTERFACES {
            let Some(h) = self.intersect_light(o, d, t0, dist) else { break };
            let tri = self.bvh.tri(h.tri);
            let m = &self.mats[tri.item as usize];
            if !m.is_transmissive() {
                return [0.0; 3];
            }
            let entering = dot(d, tri.geometric_normal()) < 0.0;
            match (inside, entering) {
                (Some((med, t_in)), false) => {
                    tint = mul(tint, med.transmittance((h.t - t_in).max(0.0)));
                    inside = None;
                }
                (None, true) => inside = Medium::new(m).map(|med| (med, h.t)),
                _ => {}
            }
            tint = scale(tint, m.transmission);
            if max3(tint) < 1e-4 {
                return [0.0; 3];
            }
            t0 = h.t + self.eps;
        }
        tint
    }

    #[inline]
    fn ground_blocks(&self, o: [f32; 3], d: [f32; 3], dist: f32) -> bool {
        if self.ground.is_none() || d[2].abs() <= 1e-9 {
            return false;
        }
        let t = (self.ground_z - o[2]) / d[2];
        if t <= self.eps || t >= dist - self.eps {
            return false;
        }
        let p = add(o, scale(d, t));
        let (dx, dy) = (p[0] - self.ground_c[0], p[1] - self.ground_c[1]);
        dx * dx + dy * dy <= self.ground_r * self.ground_r
    }

    // --- lights ------------------------------------------------------------

    /// Radiance seen by a ray that leaves the scene, split into the smooth
    /// environment and the studio cones, each with the pdf next-event
    /// estimation has for this direction *for that light*: the sky and the
    /// cones are two lights with two strategies, and each is weighted
    /// against its own (a cone's pdf is 0 outside it; the sky's is 0 when
    /// [`PathTraceOptions::env_sampling`] is off, which makes its weight 1
    /// as it always was).
    fn escaped(&self, d: [f32; 3]) -> ([f32; 3], [f32; 3], f32, f32) {
        let e = env(&self.lights, d);
        let mut cone = [0.0f32; 3];
        let mut pdf = 0.0f32;
        for c in &self.cones {
            if dot(d, c.dir) >= c.cos_max {
                cone = add(cone, c.radiance);
                // `c.q`, not `1 / n_strategies`: this has to be the density
                // `pick_light` actually samples from, or the power heuristic
                // weighs the two strategies against the wrong thing.
                pdf += c.pdf * c.q;
            }
        }
        let pdf_env = self.env_sampler.as_ref().map_or(0.0, |s| s.pdf(d) * s.q);
        (e, cone, pdf, pdf_env)
    }

    /// pdf next-event estimation would assign to hitting emissive triangle
    /// `tri` from `p` along `d` at distance `dist`.
    fn tri_light_pdf(&self, tri: u32, d: [f32; 3], dist: f32, cos_l: f32) -> f32 {
        if self.tri_lights.is_empty() || tri as usize >= self.light_of_tri.len() {
            return 0.0;
        }
        let li = self.light_of_tri[tri as usize];
        if li == u32::MAX || cos_l <= 1e-6 {
            return 0.0;
        }
        let _ = d;
        if self.tri_power <= 0.0 {
            return 0.0;
        }
        let l = &self.tri_lights[li as usize];
        // Picking by power then uniformly over the triangle: the areas
        // cancel, leaving the power fraction converted to solid angle.
        let pdf_area = luminance(l.emissive).max(1e-6) / self.tri_power;
        pdf_area * dist * dist / cos_l * self.tri_q
    }

    // --- integrator --------------------------------------------------------

    fn offset_origin(&self, s: &Surf, dir: [f32; 3]) -> [f32; 3] {
        let sign = if dot(dir, s.ng) < 0.0 { -1.0 } else { 1.0 };
        add(s.p, scale(s.ng, sign * self.eps * 4.0))
    }

    /// How far short of a light a shadow ray stops. Comfortably more than
    /// the `4 * eps` [`Tracer::offset_origin`] pushes the ray start, and
    /// still ~1e-3 of the scene, so it cannot skip a real occluder.
    #[inline]
    fn shadow_margin(&self) -> f32 {
        self.eps * 16.0
    }

    /// Radiance along one camera ray.
    ///
    /// The path is traced segment by segment. Usually there is one segment;
    /// a smooth dielectric interface makes two (see the split below), and a
    /// [`Seg`] queued on the stack is the branch that is not being followed
    /// right now. The accumulated radiance is the sum over all of them.
    ///
    /// `stack` is scratch owned by the caller and cleared here. It is
    /// borrowed rather than built per ray because a ray through a clear
    /// part does queue branches, and a `Vec` created per camera ray is a
    /// `malloc`/`free` per camera ray on exactly the scene that is already
    /// the slowest. Reused across a band it allocates once. `hits` is the
    /// same kind of scratch for the shadow rays' crossings.
    fn radiance(
        &self,
        o0: [f32; 3],
        d0: [f32; 3],
        view: &View,
        smp: &mut Sampler,
        stack: &mut Vec<Seg>,
        hits: &mut Vec<Hit>,
        first: &mut FirstHit,
        seen: &mut Seen,
    ) -> ([f32; 3], bool) {
        let mut l = [0.0f32; 3];
        let mut hit_anything = false;
        let mut splits = 0u32;
        let mut seg = Seg {
            o: o0,
            d: d0,
            beta: [1.0; 3],
            depth: 0,
            medium: None,
            travelled: 0.0,
            walk: 0,
            specular: true,
            prev_pdf: 0.0,
            through: true,
            caustic: false,
            nee: true,
        };
        stack.clear();
        loop {
            hit_anything |= self.trace_segment(seg, view, smp, &mut l, stack, &mut splits, hits, first, seen);
            match stack.pop() {
                Some(next) => seg = next,
                None => break,
            }
        }
        (l, hit_anything)
    }

    /// Trace one segment to its end, adding what it sees to `l` and pushing
    /// any split branch onto `stack`. Returns whether it touched anything,
    /// which is what tells the driver a pixel is not pure background.
    fn trace_segment(
        &self,
        seg: Seg,
        view: &View,
        smp: &mut Sampler,
        l: &mut [f32; 3],
        stack: &mut Vec<Seg>,
        splits: &mut u32,
        hits: &mut Vec<Hit>,
        first: &mut FirstHit,
        seen: &mut Seen,
    ) -> bool {
        let Seg {
            mut o,
            mut d,
            mut beta,
            mut depth,
            mut medium,
            mut travelled,
            mut walk,
            mut specular,
            mut prev_pdf,
            mut through,
            mut caustic,
            mut nee,
        } = seg;
        let mut hit_anything = false;
        let clamp = if self.pt.clamp > 0.0 { self.pt.clamp } else { f32::INFINITY };
        // `depth` counts scattering events already taken: 0 is the surface
        // the camera sees, so next-event estimation there is direct lighting
        // and goes in exactly. Everything else has bounced at least once.
        let indirect = |depth: u32, c: [f32; 3]| if depth == 0 { c } else { clamped(c, clamp) };

        loop {
            // The camera's view is the specular chain from the eye: until
            // the path has scattered off something matte (`caustic`), it is
            // what the eye sees — through a clear guide, or in a mirror —
            // and it sees guide geometry. After that it is light
            // transport, and guides are not there.
            let hit = self.intersect(o, d, self.eps, self.far, travelled, view, !caustic);

            // --- travel through a medium, if we are inside one
            if let Some(m) = medium {
                let t_surf = hit.as_ref().map(|h| dot(sub(h.p, o), d)).unwrap_or(self.far);
                if m.scattering {
                    let (uc, ut) = smp.next2();
                    let ch = ((uc * 3.0) as usize).min(2);
                    let density = m.sigma_t[ch].max(1e-8);
                    let t = -(1.0 - ut).max(1e-9).ln() / density;
                    if t < t_surf {
                        // Real scattering event: spectral MIS over the three
                        // channel-sampling strategies.
                        let tr = m.transmittance(t);
                        let pdf: f32 = (0..3).map(|k| m.sigma_t[k] * tr[k]).sum::<f32>() / 3.0;
                        if pdf <= 0.0 {
                            break;
                        }
                        beta = mul(beta, scale(mul(m.sigma_s, tr), 1.0 / pdf));
                        o = add(o, scale(d, t));
                        // Light the scattering event directly, then scatter.
                        let (direct, _) = self.next_event_volume(o, d, &m, smp, hits);
                        if !is_black(direct) {
                            *l = add(*l, indirect(depth, mul(beta, direct)));
                        }
                        let (u1, u2) = smp.next2();
                        let d_next = sample_hg(d, m.g, u1, u2);
                        prev_pdf = hg_phase(dot(d, d_next), m.g);
                        d = d_next;
                        travelled += t;
                        walk += 1;
                        specular = false;
                        through = false;
                        caustic = true;
                        nee = true;
                        if walk > self.pt.max_volume_steps || max3(beta) <= 0.0 {
                            break;
                        }
                        // Russian roulette on long walks.
                        if walk > 8 {
                            let q = max3(beta).clamp(0.05, 0.95);
                            if smp.next() > q {
                                break;
                            }
                            beta = scale(beta, 1.0 / q);
                        }
                        continue;
                    }
                    let tr = m.transmittance(t_surf);
                    let pdf: f32 = tr.iter().sum::<f32>() / 3.0;
                    if pdf <= 0.0 {
                        break;
                    }
                    beta = mul(beta, scale(tr, 1.0 / pdf));
                } else {
                    // Pure absorption: analytic, no noise at all.
                    beta = mul(beta, m.transmittance(t_surf.min(self.far)));
                }
            }

            let Some(s) = hit else {
                if (depth == 0 && !self.pt.show_environment) || (self.transparent && through) {
                    // The backdrop, which is not light: the driver draws
                    // it (or leaves it out) by how much of the pixel saw it.
                    seen.background += (beta[0] + beta[1] + beta[2]) / 3.0;
                } else {
                    let (e, cone, pdf_cone, pdf_env) = self.escaped(d);
                    let w_cone = self.mis_weight(specular, caustic, nee, prev_pdf, pdf_cone);
                    let w_env = self.mis_weight(specular, caustic, nee, prev_pdf, pdf_env);
                    let term = mul(beta, add(scale(e, w_env), scale(cone, w_cone)));
                    *l = add(*l, indirect(depth, term));
                }
                break;
            };
            if self.catcher && through && s.tri == u32::MAX {
                // The shadow catcher's ground is seen as its shadow only.
                if depth == 0 {
                    self.catch_shadow(&s, smp, hits, seen);
                } else {
                    seen.background += (beta[0] + beta[1] + beta[2]) / 3.0;
                }
                break;
            }
            hit_anything = true;
            travelled += dot(sub(s.p, o), d).abs();
            if depth == 0 && !first.set {
                *first = FirstHit { n: s.ns, albedo: s.mat.base_color, dist: travelled, set: true };
            }

            // --- emission
            if s.mat.emits() && dot(d, s.ng) < 0.0 {
                let dist = dot(sub(s.p, o), d).abs().max(1e-6);
                let cos_l = dot(s.ng, scale(d, -1.0)).max(0.0);
                let pdf_l = self.tri_light_pdf(s.tri, d, dist, cos_l);
                let w = self.mis_weight(specular, caustic, nee, prev_pdf, pdf_l);
                let term = scale(mul(beta, s.mat.emissive), w);
                *l = add(*l, indirect(depth, term));
            }

            if depth >= self.pt.max_bounces {
                break;
            }

            let onb = Onb::for_surface(s.ns, s.mat.groove);
            let wo = onb.to_local(scale(d, -1.0));
            if wo[2] <= 0.0 {
                break;
            }
            let bsdf = Bsdf::new(&s.mat, s.ns, s.entering);

            // --- next-event estimation, inside a solid as well as outside
            // it: the shadow ray goes straight out through the wall with its
            // transmittance rather than refracting, which is the same
            // approximation every other shadow ray here makes.
            let direct = self.next_event_at(&s, &bsdf, &onb, wo, smp, medium, hits);
            if !is_black(direct) {
                *l = add(*l, indirect(depth, mul(beta, direct)));
            }
            // Whether a light found by the next bounce competes with a
            // shadow ray from here, or is the only estimate there is.
            nee = bsdf.nee_useful();

            // --- a smooth dielectric splits the path instead of tossing a
            // coin between its two halves.
            //
            // A clear interface reflects a few percent and refracts the
            // rest, and those two halves are wildly unequal in what they go
            // on to see: a softbox worth tens of radiance units on one side,
            // a dim interior worth tenths on the other. Choosing between
            // them by a Fresnel-weighted coin flip is unbiased, but the
            // variance does not care how many samples are taken — the
            // relative error of a 5%-probability branch is 39% at 128
            // samples per pixel and still 20% at 512. A camera ray through a
            // 12 mm clear slab flips that coin three or four times, and the
            // result is the salt-and-pepper that makes a path tracer look
            // broken on exactly the materials this renderer exists for.
            //
            // So follow the transmission and queue the reflection, and let
            // the sum of the two be the answer. Both halves are always
            // traced, there is nothing random left at the interface, and the
            // estimator is still the same integral. The cost is bounded by
            // [`MAX_SPLITS`] per camera ray, after which the coin comes back
            // — by then the branches are deep, dim and comparable, which is
            // when a coin is a good deal. Total internal reflection is not a
            // split at all: there is only one outgoing direction.
            let mut forced: Option<BsdfSample> = None;
            if bsdf.is_delta() && bsdf.trans > 1e-4 && *splits < MAX_SPLITS {
                let fr = fresnel_dielectric(wo[2], bsdf.eta);
                if fr > 1e-4 && fr < 1.0 - 1e-4 {
                    if let Some(wt_l) = refract(wo, [0.0, 0.0, 1.0], bsdf.eta) {
                        *splits += 1;
                        let wr = onb.to_world([-wo[0], -wo[1], wo[2]]);
                        stack.push(Seg {
                            o: self.offset_origin(&s, wr),
                            d: wr,
                            beta: scale(beta, fr),
                            depth: depth + 1,
                            // A reflection stays on the side it came from,
                            // so it stays in whatever medium we are in.
                            medium,
                            travelled,
                            walk,
                            specular: true,
                            prev_pdf: 1.0,
                            through: false,
                            caustic,
                            nee,
                        });
                        let e2 = bsdf.eta * bsdf.eta;
                        forced = Some(BsdfSample {
                            wi: wt_l,
                            weight: [(1.0 - fr) * e2; 3],
                            pdf: 1.0,
                            specular: true,
                            transmitted: true,
                        });
                    }
                }
            }

            // --- BSDF sampling
            let (ul, u1) = smp.next2();
            let u2 = smp.next();
            let bs = match forced {
                Some(b) => b,
                None => match bsdf.sample(wo, ul, u1, u2) {
                    Some(b) => b,
                    None => break,
                },
            };
            if bs.pdf <= 0.0 || is_black(bs.weight) {
                break;
            }
            beta = mul(beta, bs.weight);
            let wi = onb.to_world(bs.wi);
            if bs.transmitted {
                medium = if s.entering { Medium::new(&s.mat) } else { None };
            }
            o = self.offset_origin(&s, wi);
            d = wi;
            prev_pdf = bs.pdf;
            specular = bs.specular;
            // Frosted or clear, what a transmission goes on to see is what
            // is behind the part. A transparent image cannot blur what it
            // does not have, so both count as letting it through.
            through &= bs.transmitted;
            caustic |= !bs.specular;
            depth += 1;

            // --- Russian roulette
            //
            // The floor is high and it starts late on purpose. `max_bounces`
            // already bounds the path, so roulette is here to save work on
            // dim tails, not to make the path finite — and an aggressive
            // roulette is just another coin flip on a branch that may still
            // be about to find a light. A quarter survival at worst costs a
            // little time and removes a lot of noise.
            if depth > 3 {
                let q = max3(beta).clamp(0.25, 1.0);
                if q < 1.0 {
                    if smp.next() > q {
                        break;
                    }
                    beta = scale(beta, 1.0 / q);
                }
            }
            if max3(beta) <= 0.0 || !max3(beta).is_finite() {
                break;
            }
        }
        hit_anything
    }

    /// One sample of how much of the ground's light the model blocks at
    /// `s`, a point of the shadow catcher's plane the camera sees directly:
    /// the light a matte ground would receive there from one point on the
    /// rig and one direction of the sky, with and without the model in the
    /// way. The pixel's shadow is one minus the ratio of the two sums.
    fn catch_shadow(&self, s: &Surf, smp: &mut Sampler, hits: &mut Vec<Hit>, seen: &mut Seen) {
        seen.caught = true;
        let up = [0.0, 0.0, 1.0];
        let origin = add(s.p, scale(up, self.eps * 4.0));
        if let Some((dir, pdf_l, radiance, dist)) = self.pick_light(s.p, smp) {
            if dir[2] > 0.0 && pdf_l > 0.0 {
                let open = luminance(radiance) * dir[2] / (std::f32::consts::PI * pdf_l);
                let vis = self.visibility(origin, dir, dist, None, hits);
                seen.open += open;
                seen.lit += open * luminance(vis).clamp(0.0, 1.0);
            }
        }
        let (u1, u2) = smp.next2();
        let dir = cosine_hemisphere(u1, u2);
        let sky = luminance(env(&self.lights, dir));
        seen.open += sky;
        if !self.occluded_light(origin, dir, self.eps, self.far) {
            seen.lit += sky;
        }
    }

    /// MIS weight for a light found by following the BSDF.
    ///
    /// A non-delta lobe gets the power heuristic against what next-event
    /// estimation would have done: the textbook case, and the reason a
    /// glossy highlight and a soft shadow both come out right.
    ///
    /// A delta lobe cannot be reached by next-event estimation at all, so it
    /// normally carries the whole contribution — unless the path has already
    /// scattered diffusely, in which case this is a caustic: light that
    /// reaches the eye by bouncing off something matte and *then* through a
    /// chain of mirrors or clear walls. Those get weight 0, for two reasons
    /// that agree. They are already in the image: [`Tracer::visibility`]
    /// lets a shadow ray pass straight through a clear part with its
    /// transmittance, so the matte surface under a translucent wall is
    /// already lit through it, and counting the refracted path as well would
    /// be counting it twice. And they are the single worst source of noise
    /// in a path tracer: a small bright light found at the end of a delta
    /// chain arrives as one sample worth tens of units, with no second
    /// strategy to weigh it against.
    ///
    /// What this gives up is the sharp focused caustic — the bright crescent
    /// a glass ring throws on a table. What it keeps is a light, converged
    /// image of a translucent part, which is what this renderer is for. A
    /// bidirectional integrator or photon map is the way to have both, and
    /// is left to the GPU implementation.
    ///
    /// A non-delta lobe from a vertex that ran no next-event estimation
    /// (`nee` false: a diffuse remainder too small for a shadow ray beside a
    /// delta lobe) has nothing to be weighed against and carries weight 1.
    #[inline]
    fn mis_weight(&self, specular: bool, caustic: bool, nee: bool, pdf_b: f32, pdf_l: f32) -> f32 {
        if specular {
            if caustic { 0.0 } else { 1.0 }
        } else if !nee {
            1.0
        } else {
            power_heuristic(pdf_b, pdf_l)
        }
    }

    /// Pick one light and one point on it, as seen from `p`.
    ///
    /// Returns `(direction, solid-angle pdf, radiance, distance)` with the
    /// pdf already divided by the number of strategies, because picking
    /// uniformly among the rig's cones and the emissive geometry is itself
    /// part of the density. Shared by the surface and volume estimators so
    /// the two cannot drift apart.
    fn pick_light(&self, p: [f32; 3], smp: &mut Sampler) -> Option<([f32; 3], f32, [f32; 3], f32)> {
        if self.n_strategies == 0 {
            return None;
        }
        // Walk the cones' probabilities, then the sky's, then whatever is
        // left is the emissive-geometry strategy. `u` is consumed either
        // way, so the sampler's dimension budget does not depend on which
        // is picked.
        let u = smp.next();
        let (u1, u2) = smp.next2();
        let mut acc = 0.0f32;
        let mut chosen: Option<&ConeLight> = None;
        for c in &self.cones {
            acc += c.q;
            if u < acc {
                chosen = Some(c);
                break;
            }
        }
        let (dir, pdf_raw, radiance, dist, q) = match chosen {
            Some(c) => {
                let (dir, pdf) = c.sample(u1, u2);
                (dir, pdf, c.radiance, self.far, c.q)
            }
            None => match &self.env_sampler {
                Some(sky) if u < acc + sky.q => {
                    let (dir, pdf) = sky.sample(u1, u2);
                    (dir, pdf, env(&self.lights, dir), self.far, sky.q)
                }
                _ => {
                    let u_pick = smp.next();
                    let (dir, pdf, radiance, dist) = self.sample_emissive(p, u_pick, u1, u2)?;
                    (dir, pdf, radiance, dist, self.tri_q)
                }
            },
        };
        let pdf_l = pdf_raw * q;
        if pdf_l <= 0.0 || is_black(radiance) {
            return None;
        }
        Some((dir, pdf_l, radiance, dist))
    }

    /// One next-event sample at a scattering event inside a solid.
    ///
    /// Without this a milky filament has no way to see a light except by a
    /// random walk that happens to end pointing at one, which is a 1-in-70
    /// event worth tens of radiance units: the reason a subsurface material
    /// looks like static instead of like plastic. With it the light is found
    /// by a shadow ray every time, and the walk's own escapes are weighted
    /// down to nothing by the power heuristic against the phase pdf.
    ///
    /// The shadow ray does not refract on its way out of the solid; it is
    /// tinted by the wall's transmission and by Beer-Lambert over the
    /// distance still inside, which is what [`Tracer::visibility`] does for
    /// every other shadow ray.
    fn next_event_volume(&self, p: [f32; 3], d: [f32; 3], m: &Medium, smp: &mut Sampler, hits: &mut Vec<Hit>) -> ([f32; 3], f32) {
        let Some((dir, pdf_l, radiance, dist)) = self.pick_light(p, smp) else { return ([0.0; 3], 0.0) };
        let phase = hg_phase(dot(d, dir), m.g);
        if phase <= 0.0 {
            return ([0.0; 3], 0.0);
        }
        let vis = self.visibility(p, dir, dist, Some(*m), hits);
        if is_black(vis) {
            return ([0.0; 3], 0.0);
        }
        let w = power_heuristic(pdf_l, phase);
        (scale(mul(radiance, vis), phase * w / pdf_l), phase)
    }

    /// One next-event sample at a surface point.
    fn next_event_at(
        &self,
        s: &Surf,
        bsdf: &Bsdf,
        onb: &Onb,
        wo: [f32; 3],
        smp: &mut Sampler,
        in_medium: Option<Medium>,
        hits: &mut Vec<Hit>,
    ) -> [f32; 3] {
        if !bsdf.nee_useful() {
            return [0.0; 3];
        }
        let Some((dir, pdf_l, radiance, dist)) = self.pick_light(s.p, smp) else { return [0.0; 3] };
        let wi = onb.to_local(dir);
        if wi[2] == 0.0 {
            return [0.0; 3];
        }
        let (f, pdf_b) = bsdf.eval(wo, wi);
        if is_black(f) {
            return [0.0; 3];
        }
        let origin = self.offset_origin(s, dir);
        // Which side of the interface the shadow ray starts on decides
        // whether it is still inside the solid. The local frame is flipped
        // toward the ray that arrived, so standing on an inside face,
        // `wi.z > 0` points back into the solid and the ray has to pay
        // Beer-Lambert all the way to whichever wall it leaves through;
        // `wi.z < 0` leaves immediately and pays nothing.
        let start = if wi[2] > 0.0 { in_medium } else { None };
        let vis = self.visibility(origin, dir, dist, start, hits);
        if is_black(vis) {
            return [0.0; 3];
        }
        let w = power_heuristic(pdf_l, pdf_b);
        scale(mul(mul(f, radiance), vis), wi[2].abs() * w / pdf_l)
    }

    /// Sample a point on the emissive geometry as seen from `p`.
    /// Returns `(direction, solid-angle pdf, radiance, distance)`.
    fn sample_emissive(&self, p: [f32; 3], u_pick: f32, u1: f32, u2: f32) -> Option<([f32; 3], f32, [f32; 3], f32)> {
        if self.tri_lights.is_empty() {
            return None;
        }
        let li = match self.tri_cdf.binary_search_by(|c| c.partial_cmp(&u_pick).unwrap_or(std::cmp::Ordering::Less)) {
            Ok(i) => i,
            Err(i) => i,
        }
        .min(self.tri_lights.len() - 1);
        let l = &self.tri_lights[li];
        let tri = self.bvh.tri(l.tri);
        let su = u1.max(0.0).sqrt();
        let (b0, b1) = (1.0 - su, u2 * su);
        let q = tri.point(b0, b1);
        let ng = normalize(tri.geometric_normal());
        let dv = sub(q, p);
        let dist2 = dot(dv, dv);
        if dist2 <= 1e-12 {
            return None;
        }
        let dist = dist2.sqrt();
        let dir = scale(dv, 1.0 / dist);
        // One-sided emission, out of the geometric normal.
        let cos_l = dot(ng, scale(dir, -1.0));
        if cos_l <= 1e-6 {
            return None;
        }
        if self.tri_power <= 0.0 {
            return None;
        }
        let pdf = luminance(l.emissive).max(1e-6) / self.tri_power * dist2 / cos_l;
        Some((dir, pdf, l.emissive, dist))
    }
}

#[inline]
fn clamped(c: [f32; 3], limit: f32) -> [f32; 3] {
    let m = max3(c);
    if m > limit && m > 0.0 { scale(c, limit / m) } else { c }
}

/// How many pixels a texture's feature must span before it is drawn as a
/// feature rather than averaged into the material: the textures are given
/// this many pixel footprints to filter over, not one.
///
/// One pixel is the wrong width. The tracer already averages its samples
/// over each pixel, so a one-pixel filter in the texture only repeats that;
/// what neither does is keep a pattern of two or three pixels' period off
/// the pixel *grid*. A period between two and about three and a half
/// pixels is below Nyquist and still beats against the grid — 2.4 pixels
/// against 2 is a moiré with a 12-pixel period — and the default machined
/// pitch of 0.15 mm lands exactly there at a 1600-wide product shot, as a
/// diagonal hatching that read as tessellation. Measured: at two widths
/// the grooves at that pitch kept 20% of their slope and the hatching
/// stayed; at three they are filtered flat and become sheen, a pitch of
/// five pixels keeps half its slope, one of ten 90%.
pub(crate) const RESOLVE_PIXELS: f32 = 3.0;

/// What one camera ray saw that is not radiance.
#[derive(Clone, Copy, Default)]
struct Seen {
    /// The share of the ray that reached the backdrop: 1 for a ray that
    /// missed everything, the transmittance of the clear walls in front for
    /// one that looked through them (without a backdrop only; with one,
    /// glass shows the environment).
    background: f32,
    /// The ray ended on the shadow catcher's ground.
    caught: bool,
    /// What that point receives with the model in the way, and without.
    lit: f32,
    open: f32,
}

/// What the camera ray saw first: the guide the denoiser stops its filter
/// at. Recorded once per path, at the primary hit.
#[derive(Clone, Copy, Default)]
struct FirstHit {
    n: [f32; 3],
    albedo: [f32; 3],
    dist: f32,
    set: bool,
}

// ---------------------------------------------------------------------------
// Denoise.

/// Per-pixel guides for [`denoise`], averaged over the pixel's samples: the
/// first hit's shading normal, base colour and distance (`INFINITY` where
/// nothing was hit).
pub(crate) struct Guides {
    pub normal: Vec<[f32; 3]>,
    pub albedo: Vec<[f32; 3]>,
    pub depth: Vec<f32>,
    /// Standard deviation of the pixel's mean luminance, from the spread
    /// of its own samples: how much of a difference to a neighbour is
    /// grain rather than signal.
    pub sigma: Vec<f32>,
}

/// Iterations of the à-trous filter; the last one spans 8-pixel steps.
/// Five (to 16 pixels) took more structure than grain on the grain bench.
const DENOISE_PASSES: u32 = 4;

/// Edge-aware à-trous wavelet filtering of the finished HDR image (Dammertz
/// et al., *Edge-Avoiding À-Trous Wavelet Transform for Fast Global
/// Illumination Filtering*, 2010, with the colour test of Schied et al.'s
/// SVGF, 2017): five passes of a 5×5 B3-spline kernel with holes at 1, 2,
/// 4, 8 and 16 pixels, each tap weighted by how alike the two pixels are in
/// colour — measured against the grain the pixel's own samples say it has,
/// so a difference of a few standard deviations is signal and less is
/// noise — in first-hit normal, in first-hit depth and in first-hit albedo.
/// A silhouette, a crease, a change of material or a change of tone stops
/// the filter; a flat expanse of one surface is averaged over up to about
/// 30 pixels.
///
/// What it costs: fine detail *below* the noise level goes with the noise.
/// Grooves the machined textures still resolve as normal changes are kept
/// (the normal guide carries the bump); the sheen the sub-pixel grooves
/// leave, and any highlight structure of the same amplitude as the grain,
/// is smoothed. That is why it is opt-in and for a still, not for judging
/// a surface finish (`--denoise`).
pub(crate) fn denoise(hdr: &mut Vec<[f32; 3]>, g: &Guides, w: u32, h: u32, threads: u32) {
    const KERNEL: [f32; 5] = [1.0 / 16.0, 1.0 / 4.0, 3.0 / 8.0, 1.0 / 4.0, 1.0 / 16.0];
    let (w, h) = (w as usize, h as usize);
    if hdr.len() < w * h || w < 2 || h < 2 {
        return;
    }
    let mut src = hdr.clone();
    let mut dst = vec![[0.0f32; 3]; w * h];
    for pass in 0..DENOISE_PASSES {
        let step = 1usize << pass;
        // Each pass averages several pixels' worth of grain away, so the
        // noise a later pass is judging against is smaller than the
        // samples reported: about half per pass.
        let noise_scale = 0.5f32.powi(pass as i32);
        {
            let src = &src;
            super::aa::par_bands(&mut dst, w, 8, threads, |y0, band| {
                for (i, out) in band.iter_mut().enumerate() {
                    let idx = y0 * w + i;
                    let (x, y) = (idx % w, idx / w);
                    let c0 = src[idx];
                    let (n0, a0, z0, s0) = (g.normal[idx], g.albedo[idx], g.depth[idx], g.sigma[idx]);
                    let l0 = luminance(c0);
                    let mut acc = [0.0f32; 3];
                    let mut wsum = 0.0f32;
                    for (ky, kwy) in KERNEL.iter().enumerate() {
                        let yy = y as i64 + (ky as i64 - 2) * step as i64;
                        if yy < 0 || yy >= h as i64 {
                            continue;
                        }
                        for (kx, kwx) in KERNEL.iter().enumerate() {
                            let xx = x as i64 + (kx as i64 - 2) * step as i64;
                            if xx < 0 || xx >= w as i64 {
                                continue;
                            }
                            let j = yy as usize * w + xx as usize;
                            let c = src[j];
                            // Colour: the difference in units of the two
                            // pixels' own grain (plus a floor of 2 % of the
                            // tone, so a converged image still filters
                            // nothing but its own noise floor).
                            let l = luminance(c);
                            let sigma = (s0 + g.sigma[j]) * noise_scale + 0.02 * (l0 + l) + 1e-4;
                            let dl = (l - l0).abs() / (2.0 * sigma);
                            let w_c = (-(dl * dl)).exp();
                            // Normal: a crease or a curvature change.
                            let w_n = dot(n0, g.normal[j]).max(0.0).powi(32);
                            // Depth: a silhouette. Two backgrounds agree;
                            // a background and a surface never do.
                            let (z, zj) = (z0, g.depth[j]);
                            let w_z = if z.is_infinite() && zj.is_infinite() {
                                1.0
                            } else if z.is_infinite() || zj.is_infinite() {
                                0.0
                            } else {
                                let dz = (z - zj).abs() / (z.min(zj).max(1e-3) * 0.05 * step as f32);
                                (-dz * dz).exp()
                            };
                            // Albedo: a material or texture boundary.
                            let da = sub(a0, g.albedo[j]);
                            let w_a = (-dot(da, da) / 0.02).exp();
                            let wt = kwx * kwy * w_c * w_n * w_z * w_a;
                            acc = add(acc, scale(c, wt));
                            wsum += wt;
                        }
                    }
                    *out = if wsum > 0.0 { scale(acc, 1.0 / wsum) } else { c0 };
                }
            });
        }
        std::mem::swap(&mut src, &mut dst);
    }
    *hdr = src;
}

// ---------------------------------------------------------------------------
// Driver.

/// Path-trace `items` and return the image plus timings. Called by
/// [`super::render_with_stats`] when `RenderOptions::backend` is
/// [`super::Backend::Path`].
///
/// The work is split into bands of [`BAND_ROWS`] rows over
/// [`super::aa::par_bands2`], which hands disjoint slices to a fixed pool.
/// Nothing is shared between bands and every pixel is a pure function of
/// `(x, y, sample, seed)`, so the image is byte-identical for any thread
/// count and on wasm32, where the pool is one thread.
pub fn render(items: &[Item<'_>], cam: &Camera, opts: &RenderOptions) -> (Image, RenderStats) {
    let Layers { mut model, coverage, through, shadow, backdrop, guides, mut stats } = trace_layers(items, cam, opts, opts.path.denoise);
    let (w, h) = (opts.width.max(1), opts.height.max(1));
    if let Some(g) = &guides {
        let t0 = super::aa::now_ms();
        denoise(&mut model, g, w, h, resolve_threads(opts.threads));
        stats.resolve_ms += super::aa::now_ms() - t0;
    }
    // Bloom is light added on top of everything, backdrop included: take it
    // from the image the lens would see and keep it as its own layer.
    let glow = opts.bloom.then(|| {
        let seen: Vec<[f32; 3]> = (0..model.len())
            .map(|i| if opts.transparent { scale(model[i], coverage[i]) } else { lerp3(backdrop[i], model[i], coverage[i]) })
            .collect();
        let mut lit = seen.clone();
        bloom(&mut lit, w, h);
        lit.iter().zip(&seen).map(|(a, b)| sub(*a, *b)).collect::<Vec<_>>()
    });
    let glow_at = |i: usize| glow.as_ref().map_or([0.0; 3], |g| g[i]);
    // The tone map goes on the model's own radiance, and the pixel is that
    // colour by its coverage: filtered after the curve, not before it. A
    // silhouette pixel half on a highlight worth 5 and half on a backdrop
    // worth 0.01 averages to 2.5 in radiance, which the curve sends to
    // nearly the same white as the highlight — every partly covered pixel
    // of a bright edge comes out fully bright, and the edge is a staircase.
    // Half of the tone-mapped highlight over half of the backdrop is the
    // ramp the samples measured.
    let exposure = opts.exposure;
    let tm = opts.tonemap;
    if opts.transparent {
        let mut alpha = vec![0.0f32; model.len()];
        for (i, px) in model.iter_mut().enumerate() {
            let (c, g) = (coverage[i], glow_at(i));
            let on = if c > 0.0 { scale(tonemap_rgb(add(*px, g), exposure, tm), c) } else { [0.0; 3] };
            let off = if is_black(g) { [0.0; 3] } else { scale(tonemap_rgb(g, exposure, tm), 1.0 - c) };
            *px = add(on, off);
            alpha[i] = c * (1.0 - through[i]) + shadow[i];
        }
        return (super::alpha::resolve(&model, &alpha, 1, w, h), stats);
    }
    // Then hand the display-linear buffer to the shared resolve, so
    // path-traced and rasterized images encode identically and a pixel
    // nothing covered is `background` bit for bit.
    let bg = opts.background;
    let flat = !opts.path.show_environment;
    for (i, px) in model.iter_mut().enumerate() {
        let (c, g) = (coverage[i], glow_at(i));
        let behind = if flat && is_black(g) { bg } else { tonemap_rgb(add(backdrop[i], g), exposure, tm) };
        *px = if c <= 0.0 {
            behind
        } else if c >= 1.0 {
            tonemap_rgb(add(*px, g), exposure, tm)
        } else {
            lerp3(behind, tonemap_rgb(add(*px, g), exposure, tm), c)
        };
    }
    (resolve(&model, 1, w, h, Some(bg)), stats)
}

/// The tracer's image as scene-linear radiance per pixel, backdrop
/// included, plus a mask that is 1 where every sample of the pixel escaped
/// without touching anything. Tests read this rather than the encoded
/// image, because the physics lives here and 8-bit sRGB is a lossy view of
/// it.
#[cfg(test)]
pub(crate) fn trace_hdr(items: &[Item<'_>], cam: &Camera, opts: &RenderOptions) -> (Vec<[f32; 3]>, Vec<u8>, RenderStats) {
    let (hdr, mask, _, stats) = trace_hdr_guided(items, cam, opts, false);
    (hdr, mask, stats)
}

/// [`trace_hdr`] with the [`Guides`] the denoiser wants.
#[cfg(test)]
pub(crate) fn trace_hdr_guided(
    items: &[Item<'_>],
    cam: &Camera,
    opts: &RenderOptions,
    want_guides: bool,
) -> (Vec<[f32; 3]>, Vec<u8>, Option<Guides>, RenderStats) {
    let l = trace_layers(items, cam, opts, want_guides);
    let hdr = l.model.iter().zip(&l.coverage).zip(&l.backdrop).map(|((m, &c), b)| add(scale(*m, c), scale(*b, 1.0 - c))).collect();
    let mask = l.coverage.iter().map(|&c| u8::from(c <= 0.0)).collect();
    (hdr, mask, l.guides, l.stats)
}

/// What the tracer hands the driver: the model and how much of each pixel
/// it is, kept apart so the tone map can go on the model alone.
pub(crate) struct Layers {
    /// Scene-linear radiance of the model, averaged over the samples that
    /// touched it (so it is the surface's colour, not darkened by the ones
    /// that missed); black where none did.
    pub model: Vec<[f32; 3]>,
    /// The share of the pixel's samples that touched the model, 0..1.
    pub coverage: Vec<f32>,
    /// Of those, the share that went on through clear walls to the
    /// backdrop: what makes glass less than opaque without a backdrop, and
    /// zero with one.
    pub through: Vec<f32>,
    /// The shadow catcher's alpha: the share of the pixel on bare ground
    /// times the share of the ground's light the model blocks there.
    pub shadow: Vec<f32>,
    /// What the samples that missed saw, in radiance: the environment when
    /// it is shown (`PathTraceOptions::show_environment`), otherwise
    /// `inverse_tonemap(background)` on every pixel.
    pub backdrop: Vec<[f32; 3]>,
    pub guides: Option<Guides>,
    pub stats: RenderStats,
}

/// Per pixel, what the band closure writes besides the radiance: the
/// layers' weights, and the denoiser's guides summed over the samples.
#[derive(Clone, Copy, Default)]
struct Aux {
    coverage: f32,
    through: f32,
    shadow: f32,
    backdrop: [f32; 3],
    /// Samples whose first hit was recorded.
    hits: u32,
    normal: [f32; 3],
    albedo: [f32; 3],
    dist: f32,
    /// Standard deviation of the model's mean luminance.
    sigma: f32,
}

/// The tracer proper. Recording the [`Guides`] costs a few stores per
/// path, and is only done when asked, so the ordinary render is untouched.
pub(crate) fn trace_layers(items: &[Item<'_>], cam: &Camera, opts: &RenderOptions, want_guides: bool) -> Layers {
    let (w, h) = (opts.width.max(1), opts.height.max(1));
    let scene = Scene::from_items(items);
    let view = View::new(cam, w, h).with_dof(opts.path.dof_f_number);
    let threads = resolve_threads(opts.threads);
    let samples = opts.samples.max(1);

    let t0 = super::aa::now_ms();
    let tracer = Tracer::new(items, &scene, opts, view.azimuth());
    let t1 = super::aa::now_ms();

    let n = (w as usize) * (h as usize);
    let flat = tracer.bg_hdr;
    let mut hdr = vec![[0.0f32; 3]; n];
    let mut aux = vec![Aux::default(); n];
    let wi = w as usize;
    let seed = opts.path.seed;
    let rows = band_rows(h as usize, threads, BAND_ROWS);
    // One Halton point per sample index, shared by every pixel.
    let halton: Vec<[f32; 16]> = (0..samples).map(halton_row).collect();
    let shown = opts.path.show_environment && !opts.transparent;
    par_bands2(&mut hdr, &mut aux, wi, rows, threads, |y0, band, aux| {
        // Scratch for `Tracer::radiance`, allocated once per band instead of
        // once per camera ray. `MAX_SPLITS` and `MAX_SHADOW_INTERFACES`
        // bound how much they ever hold.
        let mut branches: Vec<Seg> = Vec::with_capacity(MAX_SPLITS as usize);
        let mut crossings: Vec<Hit> = Vec::with_capacity(MAX_SHADOW_INTERFACES);
        for (i, (px, a)) in band.iter_mut().zip(aux.iter_mut()).enumerate() {
            let y = (y0 + i / wi) as u32;
            let x = (i % wi) as u32;
            let mut sum = [0.0f32; 3];
            let mut missed = [0.0f32; 3];
            let mut lum2 = 0.0f32;
            let mut hits = 0u32;
            let mut through = 0.0f32;
            let (mut caught, mut lit, mut open) = (0u32, 0.0f32, 0.0f32);
            for si in 0..samples {
                let mut smp = Sampler::new(x, y, w, si, seed, &halton[si as usize]);
                let (jx, jy) = smp.next2();
                // The lens takes two more dimensions, and only when there
                // is a lens: a pinhole image is the one it always was.
                let (o, d) = if view.has_dof() {
                    let (u1, u2) = smp.next2();
                    view.ray_dof(x as f32 + jx, y as f32 + jy, u1, u2)
                } else {
                    view.ray(x as f32 + jx, y as f32 + jy)
                };
                let mut first = FirstHit::default();
                let mut seen = Seen::default();
                let (c, hit) = tracer.radiance(o, d, &view, &mut smp, &mut branches, &mut crossings, &mut first, &mut seen);
                if seen.caught {
                    caught += 1;
                    lit += seen.lit;
                    open += seen.open;
                }
                if !hit {
                    // Nothing but the environment, when it is shown.
                    missed = add(missed, c);
                    continue;
                }
                hits += 1;
                sum = add(sum, c);
                through += seen.background;
                if want_guides {
                    let l = luminance(c);
                    lum2 += l * l;
                    if first.set {
                        a.hits += 1;
                        a.normal = add(a.normal, first.n);
                        a.albedo = add(a.albedo, first.albedo);
                        a.dist += first.dist;
                    }
                }
            }
            a.backdrop = if shown && hits < samples { scale(missed, 1.0 / (samples - hits) as f32) } else { flat };
            if hits > 0 {
                let inv = 1.0 / hits as f32;
                *px = scale(sum, inv);
                a.coverage = hits as f32 / samples as f32;
                a.through = (through * inv).clamp(0.0, 1.0);
                // Var(mean) = (E[l²] − E[l]²) / n over the samples that
                // make the mean.
                let m = luminance(*px);
                a.sigma = ((lum2 * inv - m * m).max(0.0) * inv).sqrt();
            }
            if caught > 0 && open > 0.0 {
                a.shadow = caught as f32 / samples as f32 * (1.0 - lit / open).clamp(0.0, 1.0);
            }
        }
    });
    let guides = want_guides.then(|| Guides {
        normal: aux.iter().map(|a| if dot(a.normal, a.normal) > 1e-12 { normalize(a.normal) } else { [0.0, 0.0, 1.0] }).collect(),
        albedo: aux.iter().map(|a| if a.hits > 0 { scale(a.albedo, 1.0 / a.hits as f32) } else { [0.0; 3] }).collect(),
        depth: aux.iter().map(|a| if a.hits > 0 { a.dist / a.hits as f32 } else { f32::INFINITY }).collect(),
        sigma: aux.iter().map(|a| a.sigma).collect(),
    });
    let t2 = super::aa::now_ms();

    let bs = tracer.bvh_stats;
    let stats = RenderStats {
        geometry_ms: t1 - t0,
        shade_ms: t2 - t1,
        threads_used: threads,
        passes: samples,
        triangles: bs.triangles,
        bvh_nodes: bs.nodes,
        ..Default::default()
    };
    Layers {
        model: hdr,
        coverage: aux.iter().map(|a| a.coverage).collect(),
        through: aux.iter().map(|a| a.through).collect(),
        shadow: aux.iter().map(|a| a.shadow).collect(),
        backdrop: aux.iter().map(|a| a.backdrop).collect(),
        guides,
        stats,
    }
}

#[cfg(test)]
mod tests {
    use super::super::testing::{box_mesh, fnv1a64, quad, uv_sphere};
    use super::super::{Backdrop, Backend, Image, Lighting, ToneMap, render_with_stats};
    use super::*;
    use crate::geom::Vec3;

    /// A path-traced render whose numbers are scene-linear radiance, not
    /// sRGB: every physical assertion below is made on these.
    fn trace(items: &[Item<'_>], cam: &Camera, opts: &RenderOptions) -> Hdr {
        let (px, bg, _) = trace_hdr(items, cam, opts);
        Hdr { w: opts.width.max(1), h: opts.height.max(1), px, bg }
    }

    struct Hdr {
        w: u32,
        h: u32,
        px: Vec<[f32; 3]>,
        bg: Vec<u8>,
    }

    impl Hdr {
        fn at(&self, x: u32, y: u32) -> [f32; 3] {
            self.px[(y * self.w + x) as usize]
        }

        /// Mean radiance over the half-open rect.
        fn mean(&self, x0: u32, y0: u32, x1: u32, y1: u32) -> [f32; 3] {
            let mut sum = [0.0f64; 3];
            let mut n = 0u64;
            for y in y0..y1.min(self.h) {
                for x in x0..x1.min(self.w) {
                    let c = self.at(x, y);
                    for k in 0..3 {
                        sum[k] += c[k] as f64;
                    }
                    n += 1;
                }
            }
            let n = n.max(1) as f64;
            [(sum[0] / n) as f32, (sum[1] / n) as f32, (sum[2] / n) as f32]
        }

        /// Every pixel the geometry touched, as (x, y, radiance).
        fn covered(&self) -> impl Iterator<Item = (u32, u32, [f32; 3])> + '_ {
            (0..self.h).flat_map(move |y| {
                (0..self.w).filter_map(move |x| {
                    let i = (y * self.w + x) as usize;
                    (self.bg[i] == 0).then(|| (x, y, self.px[i]))
                })
            })
        }
    }

    fn lum(c: [f32; 3]) -> f32 {
        luminance(c)
    }

    fn base_opts(w: u32, h: u32, samples: u32) -> RenderOptions {
        RenderOptions {
            width: w,
            height: h,
            samples,
            threads: 0,
            backend: Backend::Path,
            tonemap: ToneMap::None,
            draw_edges: false,
            draw_grid: false,
            ..Default::default()
        }
    }

    /// Uniform environment of 1, no direct light, no ground, no clamp: the
    /// rig every energy statement below is made under.
    fn furnace_opts(w: u32, h: u32, samples: u32) -> RenderOptions {
        RenderOptions { path: PathTraceOptions::furnace_test(), ..base_opts(w, h, samples) }
    }

    /// The studio environment gradient with the three lights turned off, so
    /// what reaches the camera is only what the material did to a
    /// background.
    fn env_only_opts(w: u32, h: u32, samples: u32) -> RenderOptions {
        RenderOptions {
            path: PathTraceOptions {
                direct_scale: 0.0,
                ground: false,
                show_environment: true,
                clamp: 0.0,
                max_bounces: 16,
                ..PathTraceOptions::default()
            },
            ..base_opts(w, h, samples)
        }
    }

    fn white(roughness: f64, metallic: f64) -> Material {
        Material { base_color: [1.0, 1.0, 1.0], roughness, metallic, ..Material::default() }
    }

    fn sphere_scene() -> (crate::tessellate::Mesh, Camera) {
        let (mesh, _) = uv_sphere(48, 96);
        let (lo, hi) = mesh.bbox().unwrap();
        (mesh, Camera::preset("iso", lo, hi, 0.25))
    }

    // -----------------------------------------------------------------
    // Energy.

    /// The white furnace: a white object in an environment of uniform
    /// radiance 1 must itself be radiance 1, because nothing else is
    /// possible — every photon it does not absorb it re-emits, and it
    /// absorbs none. Anything below 1 is energy the BSDF lost; anything
    /// above it is energy the estimator invented, which is far worse.
    ///
    /// The floor is 0.88 rather than 1.0 because this is single-scattering
    /// GGX: microfacets that shadow each other are dropped instead of being
    /// re-scattered, and at roughness 0.5 that is a few percent. A
    /// multiple-scattering compensation term would close it; this tracer does not
    /// carry one.
    #[test]
    fn white_furnace_converges_to_the_environment_radiance() {
        let (mesh, cam) = sphere_scene();
        let mat = white(0.5, 0.0);
        let opts = furnace_opts(96, 96, 96);
        let hdr = trace(&[Item::new(&mesh, &mat, &[])], &cam, &opts);
        // The background is the environment itself, exactly.
        for (x, y) in [(1u32, 1u32), (94, 1), (1, 94), (94, 94)] {
            let c = hdr.at(x, y);
            assert!((lum(c) - 1.0).abs() < 1e-3, "background at ({x},{y}) is {c:?}, not the environment");
        }
        let m = hdr.mean(40, 40, 56, 56);
        assert!(m.iter().all(|v| (0.88..=1.02).contains(v)), "sphere centre is {m:?}, not ~1");
        // No pixel anywhere may exceed the environment by more than the
        // noise floor: a furnace cannot be brighter than its walls.
        let worst = hdr.covered().map(|(_, _, c)| max3(c)).fold(0.0f32, f32::max);
        assert!(worst <= 1.15, "a pixel reached {worst}, brighter than the furnace");
    }

    /// Energy conservation across the whole material range.
    ///
    /// The hard half is the one that matters: **no** combination of
    /// roughness and metallic may return more than the furnace put in. An
    /// estimator that invents energy makes every image it produces a lie,
    /// and it is the failure mode a bad pdf or a mismatched MIS weight
    /// shows up as.
    ///
    /// The soft half records where this BSDF stands today. GGX here is
    /// single-scattering: a microfacet that shadows another simply loses
    /// that light instead of passing it on, and at roughness 1 with a metal
    /// (where the Fresnel term never damps it) that is most of the energy.
    /// A polished metal keeps nearly all of it, which is the case a print
    /// preview actually renders. Restoring the rest wants a Kulla-Conty
    /// compensation term and its albedo table, which this tracer does not
    /// carry.
    #[test]
    fn no_material_reflects_more_energy_than_it_receives() {
        let (mesh, cam) = sphere_scene();
        let opts = furnace_opts(64, 64, 64);
        for &roughness in &[0.05, 0.3, 0.6, 1.0] {
            for &metallic in &[0.0, 1.0] {
                let mat = white(roughness, metallic);
                let hdr = trace(&[Item::new(&mesh, &mat, &[])], &cam, &opts);
                let m = lum(hdr.mean(27, 27, 37, 37));
                assert!(m.is_finite() && m > 0.0, "roughness {roughness} metallic {metallic} produced {m}");
                assert!(m <= 1.02, "roughness {roughness} metallic {metallic} returned {m} > 1");
                assert!(m >= 0.28, "roughness {roughness} metallic {metallic} kept only {m}");
                if roughness <= 0.05 {
                    assert!(m >= 0.90, "a polished surface should be nearly lossless, got {m}");
                }
                if metallic == 0.0 {
                    // A dielectric is mostly diffuse, and Lambert loses
                    // nothing, so the single-scattering hole barely shows.
                    assert!(m >= 0.85, "a dielectric at roughness {roughness} kept only {m}");
                }
            }
        }
    }

    // -----------------------------------------------------------------
    // Refraction.

    /// Snell's law on the function the tracer actually refracts with, over
    /// the whole range of angles and indices it will ever see, including
    /// the total-internal-reflection boundary.
    #[test]
    fn refraction_obeys_snells_law() {
        for &ior in &[1.05f32, 1.33, 1.49, 1.57, 2.4] {
            for side in [true, false] {
                let eta = if side { 1.0 / ior } else { ior };
                // Entering the denser medium there is no critical angle at
                // all; leaving it, it is asin(1 / ior).
                let critical = if side { PI } else { (1.0 / ior).asin() };
                let mut checked = 0;
                for deg in 1..89 {
                    let theta_i = (deg as f32).to_radians();
                    let wo = [theta_i.sin(), 0.0, theta_i.cos()];
                    match refract(wo, [0.0, 0.0, 1.0], eta) {
                        None => assert!(theta_i >= critical - 1e-3, "{ior} side={side}: no refraction at {deg} deg"),
                        Some(wi) => {
                            assert!(theta_i < critical + 1e-3, "{ior} side={side}: refracted past critical at {deg}");
                            assert!(wi[2] < 0.0, "refracted ray stayed on the incident side");
                            let sin_t = (wi[0] * wi[0] + wi[1] * wi[1]).sqrt();
                            // n_i sin(i) = n_t sin(t), and eta = n_i / n_t.
                            let want = eta * theta_i.sin();
                            assert!((sin_t - want).abs() < 1e-4, "{ior} side={side} {deg} deg: sin_t {sin_t} want {want}");
                            let len = dot(wi, wi).sqrt();
                            assert!((len - 1.0).abs() < 1e-4, "refracted direction is not unit: {len}");
                            checked += 1;
                        }
                    }
                }
                assert!(checked > 10, "{ior} side={side}: only {checked} angles refracted");
            }
        }
    }

    /// A ball lens turns the world upside down. The environment here is a
    /// vertical gradient, so the background above the sphere and the
    /// background below it differ; look *through* the sphere and that
    /// difference must come back with the opposite sign. An opaque sphere
    /// of the same shape must not do that, which is what rules out the test
    /// passing on shading alone.
    #[test]
    fn a_glass_sphere_inverts_the_background_it_refracts() {
        let (mesh, _) = uv_sphere(64, 128);
        // A backdrop split across the horizon: bright above, dim below.
        // Both faces point at the camera (-y), so nothing lights the scene
        // from behind and what the sphere shows is purely refraction.
        let up_wall = quad([-60.0, 40.0, 0.0], [60.0, 40.0, 0.0], [60.0, 40.0, 60.0], [-60.0, 40.0, 60.0]);
        let down_wall = quad([-60.0, 40.0, -60.0], [60.0, 40.0, -60.0], [60.0, 40.0, 0.0], [-60.0, 40.0, 0.0]);
        let bright = Material { base_color: [0.0; 3], emissive: [6.0, 6.0, 6.0], ..Material::default() };
        let dim = Material { base_color: [0.0; 3], emissive: [0.5, 0.5, 0.5], ..Material::default() };
        let cam = Camera {
            eye: Vec3::new(0.0, -7.0, 0.0),
            target: Vec3::ZERO,
            up: Vec3::Z,
            fov_degrees: 30.0,
            orthographic: false,
        };
        let opts = env_only_opts(128, 128, 96);
        let glass = Material {
            base_color: [1.0, 1.0, 1.0],
            roughness: 0.03,
            transmission: 1.0,
            ior: 1.5,
            ..Material::default()
        };
        let opaque = Material { base_color: [0.8, 0.8, 0.8], roughness: 0.6, ..Material::default() };
        let shot = |m: &Material| {
            trace(
                &[
                    Item::new(&mesh, m, &[]),
                    Item::new(&up_wall, &bright, &[]),
                    Item::new(&down_wall, &dim, &[]),
                ],
                &cam,
                &opts,
            )
        };
        // The sphere is centred; its silhouette is well inside these rows.
        let (top_in, bot_in) = (48u32, 80u32);
        let g = shot(&glass);
        let o = shot(&opaque);

        let bg_up = lum(g.mean(4, 6, 20, 14));
        let bg_down = lum(g.mean(4, 114, 20, 122));
        assert!(bg_up > bg_down * 3.0, "the backdrop is not split: up {bg_up} down {bg_down}");

        let g_up = lum(g.mean(56, top_in, 72, top_in + 6));
        let g_down = lum(g.mean(56, bot_in - 6, 72, bot_in));
        assert!(
            g_up < g_down * 0.7,
            "the glass sphere did not invert the background: through-top {g_up} through-bottom {g_down}"
        );

        // An opaque sphere of the same shape sees the bright half with its
        // upper surface, which is the opposite ordering: the test cannot be
        // passed by shading alone.
        let o_up = lum(o.mean(56, top_in, 72, top_in + 6));
        let o_down = lum(o.mean(56, bot_in - 6, 72, bot_in));
        assert!(o_up > o_down, "the opaque sphere faces the bright half with its top: {o_up} vs {o_down}");
    }

    // -----------------------------------------------------------------
    // Volume absorption.

    /// Beer-Lambert, measured rather than eyeballed.
    ///
    /// The slab has `ior = 1`, so there is no Fresnel reflection and no
    /// bending at either face, and an orthographic camera looking straight
    /// down means the path length inside is exactly the thickness. What
    /// comes out is then `transmittance = base_color ^ (d / attenuation)`
    /// with nothing else mixed in, per channel — which is the whole point
    /// of the field: 12 mm of translucent PETG is not three times as dark
    /// as 4 mm, it is the cube of it, and it is more saturated as well.
    #[test]
    fn a_thick_slab_absorbs_by_the_beer_lambert_factor() {
        let color = [0.5f64, 0.8, 0.65];
        let attenuation = 20.0f64;
        let mat = |_d: f64| Material {
            base_color: color,
            roughness: 0.03,
            transmission: 1.0,
            ior: 1.0,
            attenuation_distance: attenuation,
            ..Material::default()
        };
        let opts = furnace_opts(24, 24, 8);
        let measure = |d: f64| {
            let m = box_mesh([-20.0, -20.0, 0.0], [20.0, 20.0, d as f32]);
            let cam = Camera {
                eye: Vec3::new(0.0, 0.0, 200.0),
                target: Vec3::new(0.0, 0.0, 0.0),
                up: Vec3::Y,
                fov_degrees: 20.0,
                orthographic: true,
            };
            let mat = mat(d);
            trace(&[Item::new(&m, &mat, &[])], &cam, &opts).mean(10, 10, 14, 14)
        };
        for &(d1, d2) in &[(4.0f64, 12.0f64), (2.0, 16.0)] {
            let (a, b) = (measure(d1), measure(d2));
            for k in 0..3 {
                let want = (color[k] as f32).powf(((d2 - d1) / attenuation) as f32);
                let got = b[k] / a[k].max(1e-6);
                assert!(
                    (got - want).abs() < 0.02,
                    "channel {k}, {d1} mm -> {d2} mm: ratio {got}, Beer-Lambert says {want} (a={a:?} b={b:?})"
                );
            }
            // And the thicker slab is the more saturated one: the strongly
            // absorbed channel falls further behind the weakly absorbed one.
            let sat = |c: [f32; 3]| c[1] / c[0].max(1e-6);
            assert!(sat(b) > sat(a) * 1.1, "the thick slab is no more saturated than the thin one");
        }
    }

    // -----------------------------------------------------------------
    // Surface roughness.

    /// A body behind a translucent shell is seen through it: a red light
    /// under a slab that says only `opacity=0.42` (so the tracer derives
    /// its transmission) reads red through the slab, and the same slab
    /// opaque hides it.
    #[test]
    fn a_body_behind_a_translucent_one_is_visible() {
        let lamp = quad([-60.0, -60.0, -20.0], [60.0, -60.0, -20.0], [60.0, 60.0, -20.0], [-60.0, 60.0, -20.0]);
        let red = Material { base_color: [0.0; 3], emissive: [4.0, 0.0, 0.0], ..Material::default() };
        let slab = box_mesh([-140.0, -140.0, 0.0], [140.0, 140.0, 3.0]);
        let cam = Camera { eye: Vec3::new(0.0, 0.0, 200.0), target: Vec3::ZERO, up: Vec3::Y, fov_degrees: 24.0, orthographic: true };
        let opts = RenderOptions {
            path: PathTraceOptions { direct_scale: 0.0, ground: false, clamp: 0.0, ..PathTraceOptions::default() },
            ..base_opts(12, 12, 24)
        };
        let centre = |opacity: f64| {
            let shell = Material { base_color: [0.36, 0.78, 0.73], roughness: 0.06, ior: 1.57, opacity, ..Material::default() };
            let hdr = trace(&[Item::new(&slab, &shell, &[]), Item::new(&lamp, &red, &[])], &cam, &opts);
            hdr.mean(3, 3, 9, 9)
        };
        let seen = centre(0.42);
        let hidden = centre(1.0);
        assert!(seen[0] > 0.3 && seen[0] > 2.0 * seen[1], "the red light behind the shell does not show: {seen:?}");
        assert!(hidden[0] < 1.5 * hidden[1] + 0.02, "an opaque shell let the light behind show: {hidden:?}");
    }

    /// A rough dielectric scatters what it transmits; a smooth one does
    /// not. Behind both slabs is the same set of emissive stripes, and the
    /// measure is how much of that pattern survives: the variation along a
    /// line across the stripes, which a blur destroys and a clear pane
    /// keeps.
    #[test]
    fn a_rough_dielectric_blurs_a_background_more_than_a_smooth_one() {
        // Emissive stripes 100 mm below the slab, 16 mm pitch, half on and
        // half off. Far enough that a rough interface smears one stripe
        // into the next, close enough that a clear one keeps them apart.
        let mut stripes = Vec::new();
        for i in -10..10 {
            let x0 = i as f32 * 16.0;
            stripes.push(quad(
                [x0, -200.0, -100.0],
                [x0 + 8.0, -200.0, -100.0],
                [x0 + 8.0, 200.0, -100.0],
                [x0, 200.0, -100.0],
            ));
        }
        let emitter = Material { base_color: [0.0; 3], emissive: [6.0, 6.0, 6.0], ..Material::default() };
        let slab = box_mesh([-140.0, -140.0, 0.0], [140.0, 140.0, 4.0]);
        let cam = Camera {
            eye: Vec3::new(0.0, 0.0, 200.0),
            target: Vec3::ZERO,
            up: Vec3::Y,
            fov_degrees: 24.0,
            orthographic: true,
        };
        let opts = RenderOptions {
            path: PathTraceOptions { direct_scale: 0.0, ground: false, clamp: 0.0, ..PathTraceOptions::default() },
            ..base_opts(96, 32, 128)
        };
        let contrast = |roughness: f64| {
            let mat = Material {
                base_color: [1.0, 1.0, 1.0],
                roughness,
                transmission: 1.0,
                ior: 1.5,
                ..Material::default()
            };
            let mut items: Vec<Item<'_>> = vec![Item::new(&slab, &mat, &[])];
            for m in &stripes {
                items.push(Item::new(m, &emitter, &[]));
            }
            let hdr = trace(&items, &cam, &opts);
            // Standard deviation along the middle band: the stripes' signal,
            // averaged down the rows so pixel noise does not count as one.
            let row: Vec<f32> = (12..84).map(|x| lum(hdr.mean(x, 10, x + 1, 22))).collect();
            let mean = row.iter().sum::<f32>() / row.len() as f32;
            (row.iter().map(|v| (v - mean) * (v - mean)).sum::<f32>() / row.len() as f32).sqrt()
        };
        let smooth = contrast(0.03);
        let rough = contrast(0.6);
        assert!(smooth > 0.2, "the smooth slab did not show the stripes at all ({smooth})");
        assert!(rough * 3.0 < smooth, "roughness did not blur the background: smooth {smooth}, rough {rough}");
        // And a middling roughness sits between the two, so this is a
        // continuum and not a switch.
        let middling = contrast(0.25);
        assert!(middling < smooth && middling > rough, "blur is not monotone: {smooth} {middling} {rough}");
    }

    // -----------------------------------------------------------------
    // Emissive geometry as a light source.

    /// An emissive surface has to *light* what is next to it, not merely
    /// glow. The test is differential — the same scene with the emission
    /// switched off, so geometry and occlusion are identical — and it
    /// checks the falloff as well as the brightness, because a constant
    /// offset would pass the first half on its own.
    #[test]
    fn an_emissive_surface_lights_a_nearby_wall() {
        // A wall in the xz plane facing the camera, and a small panel
        // standing off it to one side, facing the wall.
        let wall = quad([-70.0, 0.0, 0.0], [70.0, 0.0, 0.0], [70.0, 0.0, 40.0], [-70.0, 0.0, 40.0]);
        // Normal -y, i.e. facing the wall and away from the camera: the
        // camera never sees the emitter itself, only what it does.
        let panel = quad([10.0, 14.0, 12.0], [22.0, 14.0, 12.0], [22.0, 14.0, 24.0], [10.0, 14.0, 24.0]);
        let wall_mat = Material { base_color: [0.85, 0.85, 0.85], roughness: 0.9, ..Material::default() };
        let cam = Camera {
            eye: Vec3::new(-6.0, 130.0, 20.0),
            target: Vec3::new(-6.0, 0.0, 20.0),
            up: Vec3::Z,
            fov_degrees: 40.0,
            orthographic: false,
        };
        let opts = RenderOptions {
            path: PathTraceOptions { direct_scale: 0.0, ground: false, clamp: 0.0, ..PathTraceOptions::default() },
            ..base_opts(80, 80, 128)
        };
        let render = |emissive: [f64; 3]| {
            let pm = Material { base_color: [0.0; 3], emissive, ..Material::default() };
            trace(&[Item::new(&wall, &wall_mat, &[]), Item::new(&panel, &pm, &[])], &cam, &opts)
        };
        let lit = render([14.0, 14.0, 14.0]);
        let dark = render([0.0; 3]);
        // Two patches of wall, both visible, at different distances from
        // the panel. The camera looks along -y with +x to the *left* of the
        // frame, and the panel is at x in [10, 22], so the near patch is
        // just right of it and the far patch is at the other edge.
        let near = |h: &Hdr| lum(h.mean(30, 36, 44, 50));
        let far = |h: &Hdr| lum(h.mean(62, 36, 76, 50));
        let d_near = near(&lit) - near(&dark);
        let d_far = far(&lit) - far(&dark);
        assert!(d_near > 0.02, "the emissive panel did not light the wall next to it ({d_near})");
        assert!(d_near > d_far * 1.6, "no falloff: near {d_near}, far {d_far}");
        assert!(d_far >= -1e-3, "switching the panel on made the far wall darker: {d_far}");
    }

    /// Colour bleeding: a Cornell-style box with one red wall and one green
    /// one, a white block in the middle, and a light in the ceiling. The
    /// block's own material is neutral grey, so any colour on its sides can
    /// only have come off the walls.
    #[test]
    fn colour_bleeds_between_the_walls_of_a_cornell_box() {
        let (a, b) = (-30.0f32, 30.0f32);
        let (zf, zc) = (0.0f32, 60.0f32);
        let back = -30.0f32;
        let front = 60.0f32;
        let floor = quad([a, back, zf], [b, back, zf], [b, front, zf], [a, front, zf]);
        let ceil = quad([a, back, zc], [a, front, zc], [b, front, zc], [b, back, zc]);
        let rear = quad([a, back, zf], [a, back, zc], [b, back, zc], [b, back, zf]);
        let left = quad([a, back, zf], [a, front, zf], [a, front, zc], [a, back, zc]);
        let right = quad([b, back, zf], [b, back, zc], [b, front, zc], [b, front, zf]);
        let lamp = quad([-10.0, -10.0, zc - 0.5], [-10.0, 10.0, zc - 0.5], [10.0, 10.0, zc - 0.5], [10.0, -10.0, zc - 0.5]);
        let block = box_mesh([-8.0, -8.0, 0.0], [8.0, 8.0, 26.0]);

        let grey = Material { base_color: [0.75, 0.75, 0.75], roughness: 0.9, ..Material::default() };
        let red = Material { base_color: [0.72, 0.06, 0.06], roughness: 0.9, ..Material::default() };
        let green = Material { base_color: [0.08, 0.62, 0.09], roughness: 0.9, ..Material::default() };
        let light = Material { base_color: [0.0; 3], emissive: [12.0, 12.0, 12.0], ..Material::default() };

        let cam = Camera {
            eye: Vec3::new(0.0, 150.0, 30.0),
            target: Vec3::new(0.0, 0.0, 26.0),
            up: Vec3::Z,
            fov_degrees: 40.0,
            orthographic: false,
        };
        let opts = RenderOptions {
            path: PathTraceOptions {
                direct_scale: 0.0,
                ground: false,
                clamp: 0.0,
                max_bounces: 6,
                ..PathTraceOptions::default()
            },
            ..base_opts(96, 96, 192)
        };
        let items = vec![
            Item::new(&floor, &grey, &[]),
            Item::new(&ceil, &grey, &[]),
            Item::new(&rear, &grey, &[]),
            Item::new(&left, &red, &[]),
            Item::new(&right, &green, &[]),
            Item::new(&lamp, &light, &[]),
            Item::new(&block, &grey, &[]),
        ];
        let hdr = trace(&items, &cam, &opts);
        // The camera looks along -y with +x to the *left* of the image, so
        // the red (x = a) wall is on the right of the frame.
        let left_face = hdr.mean(18, 46, 30, 62);
        let right_face = hdr.mean(66, 46, 78, 62);
        let ratio = |c: [f32; 3]| c[0] / c[1].max(1e-6);
        assert!(lum(left_face) > 0.02 && lum(right_face) > 0.02, "the box is black: {left_face:?} {right_face:?}");
        assert!(
            ratio(right_face) > ratio(left_face) * 1.25,
            "no colour bleeding: red-side r/g {} vs green-side r/g {} ({right_face:?} {left_face:?})",
            ratio(right_face),
            ratio(left_face)
        );
        // The green side must actually be greener, not merely less red.
        assert!(left_face[1] > left_face[0] * 1.1, "the green-lit face is not green: {left_face:?}");
    }

    // -----------------------------------------------------------------
    // Layer anisotropy.

    /// The layer term widens the specular lobe across the stacking
    /// direction and narrows it along the extrusion, and does neither on a
    /// face whose normal is the stacking direction — there are no layer
    /// lines to see looking straight down at the top of a print.
    #[test]
    fn layer_anisotropy_stretches_the_lobe_only_on_the_walls() {
        let m = Pbr { roughness: 0.4, anisotropy: 0.8, ..Pbr::default() };
        let wall = Bsdf::new(&m, [1.0, 0.0, 0.0], true);
        assert!(wall.ay > wall.ax * 3.0, "a vertical wall is not anisotropic: ax {} ay {}", wall.ax, wall.ay);
        let top = Bsdf::new(&m, [0.0, 0.0, 1.0], true);
        assert!((top.ax - top.ay).abs() < 1e-6, "the top face picked up layer lines: ax {} ay {}", top.ax, top.ay);
        let iso = Bsdf::new(&Pbr { anisotropy: 0.0, ..m }, [1.0, 0.0, 0.0], true);
        assert!((iso.ax - iso.ay).abs() < 1e-6, "anisotropy 0 is not isotropic");
        // Widening one axis and narrowing the other keeps the lobe's area
        // roughly fixed, so a printed part is not simply rougher.
        let area = |b: &Bsdf| b.ax * b.ay;
        let r = area(&wall) / area(&iso);
        assert!((0.5..2.0).contains(&r), "anisotropy changed the overall roughness by {r}x");
    }

    /// And it reaches the image, in the shape it is supposed to.
    ///
    /// A flat wall lit by one small panel shows the specular lobe directly:
    /// the highlight *is* the lobe, smeared over the panel's size. Counting
    /// changed pixels would only prove the number moved, so this measures
    /// the highlight's aspect ratio instead. Isotropic roughness gives a
    /// round spot; the layer term must stretch it up the stacking direction
    /// — which is what a printed wall looks like under a lamp, and what an
    /// injection-moulded one does not.
    #[test]
    fn layer_anisotropy_stretches_the_highlight_across_the_layers() {
        let wall = quad([-40.0, 0.0, -40.0], [-40.0, 0.0, 40.0], [40.0, 0.0, 40.0], [40.0, 0.0, -40.0]);
        let panel = quad([-3.0, 50.0, 17.0], [3.0, 50.0, 17.0], [3.0, 50.0, 23.0], [-3.0, 50.0, 23.0]);
        let lamp = Material { base_color: [0.0; 3], emissive: [40.0, 40.0, 40.0], ..Material::default() };
        let cam = Camera {
            eye: Vec3::new(0.0, 60.0, 0.0),
            target: Vec3::ZERO,
            up: Vec3::Z,
            fov_degrees: 44.0,
            orthographic: false,
        };
        let opts = RenderOptions {
            path: PathTraceOptions { direct_scale: 0.0, ground: false, clamp: 0.0, ..PathTraceOptions::default() },
            ..base_opts(96, 96, 96)
        };
        // Aspect ratio of the bright region: standard deviation up the
        // frame over standard deviation across it.
        let aspect = |anisotropy: f64| {
            let mat = Material {
                base_color: [0.9, 0.9, 0.9],
                roughness: 0.25,
                metallic: 1.0,
                anisotropy,
                ..Material::default()
            };
            let hdr = trace(&[Item::new(&wall, &mat, &[]), Item::new(&panel, &lamp, &[])], &cam, &opts);
            let peak = hdr.covered().map(|(_, _, c)| lum(c)).fold(0.0f32, f32::max);
            let cut = peak * 0.25;
            let (mut n, mut sx, mut sy, mut sxx, mut syy) = (0.0f64, 0.0f64, 0.0f64, 0.0f64, 0.0f64);
            for (x, y, c) in hdr.covered() {
                let w = (lum(c) - cut).max(0.0) as f64;
                n += w;
                sx += w * x as f64;
                sy += w * y as f64;
                sxx += w * (x * x) as f64;
                syy += w * (y * y) as f64;
            }
            assert!(n > 0.0, "no highlight at anisotropy {anisotropy}");
            let vx = (sxx / n - (sx / n) * (sx / n)).max(1e-6);
            let vy = (syy / n - (sy / n) * (sy / n)).max(1e-6);
            (vy / vx).sqrt() as f32
        };
        let round = aspect(0.0);
        let printed = aspect(0.95);
        assert!((0.6..1.6).contains(&round), "the isotropic highlight is not round: aspect {round}");
        assert!(printed > round * 1.8, "the layer term did not stretch the highlight: {round} -> {printed}");
    }

    // -----------------------------------------------------------------
    // Determinism and wiring.

    fn small_scene() -> (crate::tessellate::Mesh, Material, Camera) {
        let (mesh, _) = uv_sphere(24, 48);
        let (lo, hi) = mesh.bbox().unwrap();
        (mesh, Material { roughness: 0.4, ..Material::default() }, Camera::preset("iso", lo, hi, 0.2))
    }

    /// The contract that makes a path tracer usable in CI: the image is a
    /// pure function of the scene and the sample count, and the thread pool
    /// is free to schedule however it likes.
    #[test]
    fn thread_count_never_changes_the_image() {
        let (mesh, mat, cam) = small_scene();
        let items = [Item::new(&mesh, &mat, &[])];
        let mut hashes = Vec::new();
        for threads in [1u32, 2, 3, 7, 16] {
            let opts = RenderOptions { threads, ..base_opts(72, 56, 24) };
            let (img, stats) = render(&items, &cam, &opts);
            assert_eq!(stats.passes, 24);
            hashes.push((threads, fnv1a64(&img.rgba)));
        }
        let first = hashes[0].1;
        for (threads, h) in &hashes {
            assert_eq!(*h, first, "threads={threads} produced a different image");
        }
    }

    /// The studio rig is the engineering look, and every golden in the
    /// repository was taken under it, so the product-shot rig must leave
    /// it alone to the bit. The hashes below are of the studio rig's own
    /// image: a sphere and a box on the floor, default options, 16 paths.
    ///
    /// The one change that touches this image is the sky as a
    /// next-event strategy (`env_sampling`), which reorders every path's
    /// random numbers by design — its converged image is the same, its
    /// noise is not — so the pin holds it off. The other sampling change,
    /// skipping shadow rays from near-mirror surfaces, cannot touch this
    /// scene (nothing in it is a mirror), so the hashes are the ones
    /// the studio rig alone gives.
    #[test]
    fn the_studio_rig_is_bit_identical_to_before_the_product_shot() {
        let (sphere, _) = uv_sphere(16, 32);
        let block = box_mesh([-30.0, -30.0, -10.0], [-12.0, -12.0, 6.0]);
        let mat = Material { roughness: 0.3, metallic: 1.0, anisotropy: 0.5, ..Material::default() };
        let plastic = Material { base_color: [0.7, 0.3, 0.2], ..Material::default() };
        let items = [Item::new(&sphere, &mat, &[]), Item::new(&block, &plastic, &[])];
        let (lo, hi) = (Vec3::new(-30.0, -30.0, -10.0), Vec3::new(10.0, 10.0, 10.0));
        let cam = Camera::preset("iso", lo, hi, 0.2);
        let opts = RenderOptions { tonemap: ToneMap::Aces, ..base_opts(64, 48, 16) };
        let before = PathTraceOptions { env_sampling: false, ..PathTraceOptions::default() };
        let img = render(&items, &cam, &RenderOptions { path: PathTraceOptions { show_environment: true, ..before.clone() }, ..opts.clone() }).0;
        assert_eq!(fnv1a64(&img.rgba), 12_052_437_758_666_102_446, "the studio rig moved (environment shown)");
        // Without the floor the corners of the frame are backdrop, which is
        // the other thing the rig work must not move. (This hash is of the
        // image whose silhouette pixels are mixed with the backdrop after
        // the tone map; every fully covered pixel is as it was.)
        let img = render(&items, &cam, &RenderOptions { path: PathTraceOptions { ground: false, ..before }, ..opts }).0;
        assert_eq!(fnv1a64(&img.rgba), 9_039_875_109_378_493_271, "the studio rig moved (flat backdrop, no floor)");
    }

    #[test]
    fn the_same_scene_twice_is_byte_identical() {
        let (mesh, mat, cam) = small_scene();
        let items = [Item::new(&mesh, &mat, &[])];
        let opts = base_opts(64, 48, 16);
        let a = render(&items, &cam, &opts).0;
        let b = render(&items, &cam, &opts).0;
        assert_eq!(fnv1a64(&a.rgba), fnv1a64(&b.rgba));
    }

    /// `RenderOptions::backend` is what routes a render here, and the
    /// backdrop still lands on exactly `background` so the two backends can
    /// be laid out in one contact sheet.
    #[test]
    fn the_backend_switch_reaches_the_tracer_and_keeps_the_backdrop() {
        let (mesh, mat, cam) = small_scene();
        let items = [Item::new(&mesh, &mat, &[])];
        // No ground plane: with one, the corner of the frame is floor, and
        // what is being pinned here is the *backdrop*.
        let opts = RenderOptions {
            path: PathTraceOptions { ground: false, ..PathTraceOptions::default() },
            ..base_opts(64, 48, 16)
        };
        let (img, stats) = render_with_stats(&items, &cam, &RenderOptions { tonemap: ToneMap::Aces, ..opts.clone() });
        assert!(stats.triangles > 100, "the path backend did not build a BVH: {stats:?}");
        let bg = Image::new(1, 1, opts.background).rgba;
        assert_eq!(&img.rgba[0..3], &bg[0..3], "the corner pixel is not the background colour");
        let raster = render_with_stats(&items, &cam, &RenderOptions { backend: Backend::Raster, ..opts }).0;
        assert_eq!(&raster.rgba[0..3], &bg[0..3]);
        assert_ne!(fnv1a64(&img.rgba), fnv1a64(&raster.rgba), "the two backends produced the same image");
    }

    /// Sample counts compose: doubling them averages the same estimator, so
    /// the image moves toward the converged answer rather than jumping
    /// somewhere else.
    #[test]
    fn more_samples_move_toward_the_converged_image() {
        let (mesh, cam) = sphere_scene();
        let mat = white(0.6, 0.0);
        let opts = furnace_opts(48, 48, 0);
        let err = |samples: u32| {
            let hdr = trace(&[Item::new(&mesh, &mat, &[])], &cam, &RenderOptions { samples, ..opts.clone() });
            let n = hdr.covered().count().max(1) as f32;
            // In a furnace the converged answer is a constant, so the error
            // is just the spread about it.
            let mean = hdr.covered().map(|(_, _, c)| lum(c)).sum::<f32>() / n;
            (hdr.covered().map(|(_, _, c)| (lum(c) - mean).powi(2)).sum::<f32>() / n).sqrt()
        };
        let (e8, e128) = (err(8), err(128));
        assert!(e128 < e8 * 0.6, "16x the samples barely helped: {e8} -> {e128}");
    }

    // -----------------------------------------------------------------
    // The material bridge.

    /// The property the power weighting could break, tested on its own,
    /// away from the BSDF and the image.
    ///
    /// `pick_light` is free to choose between the rig's lights however it
    /// likes -- any strictly positive `q` summing to 1 is unbiased -- but
    /// only if the pdf it reports is the density it actually sampled from.
    /// Get `q` into one and not the other and the result is bias, and bias
    /// in a path tracer looks exactly like a slightly different scene.
    ///
    /// So estimate the irradiance the rig delivers to a surface of a given
    /// normal, using nothing but `pick_light` and the pdf it returns, and
    /// compare it against the analytic answer: the sum over the three lights
    /// of `radiance * max(cos, 0)`. The lights are made near-point (a 1
    /// degree half-angle) so the cone's own smoothing of the cosine is
    /// smaller than the tolerance.
    #[test]
    fn next_event_estimation_integrates_the_rig_to_its_irradiance() {
        let scene = Scene::from_items(&[]);
        // The cones alone: the sky strategy is measured by its own test.
        let opts = RenderOptions {
            path: PathTraceOptions { ground: false, light_angle_degrees: 1.0, env_sampling: false, ..PathTraceOptions::default() },
            ..RenderOptions::default()
        };
        let tracer = Tracer::new(&[], &scene, &opts, 0.0);
        assert_eq!(tracer.cones.len(), 3, "the studio rig is three lights");
        let q: f32 = tracer.cones.iter().map(|c| c.q).sum();
        assert!((q - 1.0).abs() < 1e-5, "selection probabilities sum to {q}, not 1");
        assert!(tracer.cones.iter().all(|c| c.q > 0.0), "every light must be reachable");
        // It really is power weighted, not uniform: the key light carries
        // most of the rig and should be picked most of the time.
        let biggest = tracer.cones.iter().map(|c| c.q).fold(0.0f32, f32::max);
        assert!(biggest > 0.5, "the key light's share is {biggest}, so this is not power weighted");

        let lights = Lights::studio();
        let p = [0.0f32, 0.0, 0.0];
        for nrm in [[0.0f32, 0.0, 1.0], normalize([-0.4, -0.6, 0.7]), normalize([0.2, 0.9, 0.1]), normalize([1.0, 0.0, 0.3])] {
            let mut want = [0.0f32; 3];
            for l in [&lights.key, &lights.fill, &lights.rim] {
                let c = dot(nrm, normalize(l.dir)).max(0.0);
                want = add(want, scale(l.radiance, c));
            }
            let n = 400_000u32;
            let mut sum = [0.0f64; 3];
            for i in 0..n {
                let row = halton_row(i);
                let mut smp = Sampler::new(i % 601, i / 601, 601, i, 0x51ed, &row);
                if let Some((dir, pdf_l, radiance, _)) = tracer.pick_light(p, &mut smp) {
                    let c = dot(dir, nrm).max(0.0);
                    if c > 0.0 && pdf_l > 0.0 {
                        for k in 0..3 {
                            sum[k] += (radiance[k] * c / pdf_l) as f64;
                        }
                    }
                }
            }
            let got = [(sum[0] / n as f64) as f32, (sum[1] / n as f64) as f32, (sum[2] / n as f64) as f32];
            for k in 0..3 {
                let tol = 0.02 * want[k].max(0.05);
                assert!(
                    (got[k] - want[k]).abs() <= tol,
                    "normal {nrm:?} channel {k}: estimated {} against the rig's own {} (tolerance {tol})",
                    got[k],
                    want[k]
                );
            }
        }
    }

    /// The sky as a fourth next-event strategy: with it on, `pick_light`
    /// estimates the rig's irradiance *plus* the environment's — the cosine-
    /// weighted integral of the gradient over the hemisphere, computed here
    /// by quadrature — from the same pdf it samples with, for a face of any
    /// orientation, under both rigs and in the furnace, where the table is
    /// flat and the strategy is uniform sphere sampling. The selection
    /// probabilities still sum to 1 with the sky's included, and its share
    /// is set by power, so under the product surround (a bright sky) it is
    /// picked more often than under the studio's.
    #[test]
    fn next_event_estimation_integrates_the_sky_when_it_samples_it() {
        let scene = Scene::from_items(&[]);
        for (lighting, furnace) in [(Lighting::Studio, false), (Lighting::Product, false), (Lighting::Studio, true)] {
            let opts = RenderOptions {
                lighting,
                path: PathTraceOptions { ground: false, light_angle_degrees: 1.0, furnace, env_sampling: true, ..PathTraceOptions::default() },
                ..RenderOptions::default()
            };
            let tracer = Tracer::new(&[], &scene, &opts, 0.5);
            let sky = tracer.env_sampler.as_ref().expect("the sky is a strategy when asked for");
            let q: f32 = tracer.cones.iter().map(|c| c.q).sum::<f32>() + sky.q;
            assert!((q - 1.0).abs() < 1e-5, "selection probabilities sum to {q}, not 1");
            assert!(sky.q > 0.0 && sky.q < 1.0 || furnace, "the sky's share is {}", sky.q);
            let lights = if furnace { Lights::furnace() } else { Lights::for_options(&opts, 0.5) };
            for nrm in [[0.0f32, 0.0, 1.0], normalize([0.6, -0.5, 0.3]), normalize([0.0, 1.0, -0.4]), [0.0, 0.0, -1.0]] {
                // The analytic part: the cones.
                let mut want = [0.0f32; 3];
                for l in [&lights.key, &lights.fill, &lights.rim] {
                    let c = dot(nrm, normalize(l.dir)).max(0.0);
                    want = add(want, scale(l.radiance, c * lights.direct_scale));
                }
                // The quadrature part: the sky, 400 x 800 over the sphere.
                let (nt, np) = (400, 800);
                let mut sky_e = [0.0f64; 3];
                for i in 0..nt {
                    let theta = PI * (i as f32 + 0.5) / nt as f32;
                    for j in 0..np {
                        let phi = TAU * (j as f32 + 0.5) / np as f32;
                        let d = [theta.sin() * phi.cos(), theta.sin() * phi.sin(), theta.cos()];
                        let c = dot(d, nrm).max(0.0);
                        if c > 0.0 {
                            let e = env(&lights, d);
                            let dw = theta.sin() * (PI / nt as f32) * (TAU / np as f32);
                            for k in 0..3 {
                                sky_e[k] += (e[k] * c * dw) as f64;
                            }
                        }
                    }
                }
                for k in 0..3 {
                    want[k] += sky_e[k] as f32;
                }
                let n = 400_000u32;
                let mut sum = [0.0f64; 3];
                for i in 0..n {
                    let row = halton_row(i);
                    let mut smp = Sampler::new(i % 601, i / 601, 601, i, 0x5c15, &row);
                    if let Some((dir, pdf_l, radiance, _)) = tracer.pick_light([0.0; 3], &mut smp) {
                        let c = dot(dir, nrm).max(0.0);
                        if c > 0.0 && pdf_l > 0.0 {
                            for k in 0..3 {
                                sum[k] += (radiance[k] * c / pdf_l) as f64;
                            }
                        }
                    }
                }
                let got = [(sum[0] / n as f64) as f32, (sum[1] / n as f64) as f32, (sum[2] / n as f64) as f32];
                for k in 0..3 {
                    let tol = 0.03 * want[k].max(0.05);
                    assert!(
                        (got[k] - want[k]).abs() <= tol,
                        "{lighting:?} furnace={furnace} normal {nrm:?} channel {k}: estimated {} against cones + sky {} (tolerance {tol})",
                        got[k],
                        want[k]
                    );
                }
            }
        }
        // Brighter sky, bigger share.
        let share = |lighting: Lighting| {
            let opts = RenderOptions { lighting, path: PathTraceOptions { ground: false, env_sampling: true, ..PathTraceOptions::default() }, ..RenderOptions::default() };
            Tracer::new(&[], &scene, &opts, 0.0).env_sampler.as_ref().unwrap().q
        };
        assert!(share(Lighting::Product) > share(Lighting::Studio), "product {} vs studio {}", share(Lighting::Product), share(Lighting::Studio));
        // Off — the default, on the measurement in `PathTraceOptions` — the
        // sky is not a strategy and the cones keep the whole share.
        let opts = RenderOptions { path: PathTraceOptions { ground: false, ..PathTraceOptions::default() }, ..RenderOptions::default() };
        let tracer = Tracer::new(&[], &scene, &opts, 0.0);
        assert!(tracer.env_sampler.is_none());
        assert!((tracer.cones.iter().map(|c| c.q).sum::<f32>() - 1.0).abs() < 1e-5);
    }

    /// The product rig's calibration, measured through `pick_light` the way
    /// the studio rig is above. The key is 3.2 square-on; the top face sees
    /// `3.2 sin 40° + 0.8 sin 15° + 1.6 sin 35°` from the three lights; the
    /// side facing the fill sees the fill's 0.8 and the key at 95° round
    /// from it, which is nearly nothing. What the environment adds to the
    /// key-facing face is checked separately against `env_diffuse`: the surround is
    /// 0.60 / 0.35 / 0.25, neutral to the bit, and the whole rig turns with
    /// the camera so the key stays 40° to camera-left from any azimuth.
    #[test]
    fn the_product_rig_delivers_its_calibration() {
        let scene = Scene::from_items(&[]);
        let opts = RenderOptions {
            lighting: Lighting::Product,
            path: PathTraceOptions { ground: false, light_angle_degrees: 1.0, env_sampling: false, ..PathTraceOptions::default() },
            ..RenderOptions::default()
        };
        for az_deg in [0.0f32, 30.0, 135.0, -90.0] {
            let az = az_deg.to_radians();
            let tracer = Tracer::new(&[], &scene, &opts, az);
            assert_eq!(tracer.cones.len(), 3, "the product rig is three lights");
            let lights = Lights::product(az);
            // Neutral, and the product surround.
            for l in [&lights.key, &lights.fill, &lights.rim] {
                assert!(l.radiance[0] == l.radiance[1] && l.radiance[1] == l.radiance[2], "the product rig is neutral: {:?}", l.radiance);
            }
            assert_eq!((lights.sky, lights.horizon, lights.ground), ([0.60; 3], [0.35; 3], [0.25; 3]));
            assert_eq!((lights.key.radius_degrees, lights.fill.radius_degrees, lights.rim.radius_degrees), (22.0, 30.0, 8.0));
            // Placement relative to the camera: the key is 40° to the left
            // and 40° up whatever the azimuth, the fill 55° right and 15°
            // up, the rim behind-right and 35° up.
            let dir = |a: f32, e: f32| {
                let (a, e) = ((az_deg + a).to_radians(), e.to_radians());
                normalize([a.sin() * e.cos(), -a.cos() * e.cos(), e.sin()])
            };
            for (l, want) in [(&lights.key, dir(-40.0, 40.0)), (&lights.fill, dir(55.0, 15.0)), (&lights.rim, dir(155.0, 35.0))] {
                assert!(dot(l.dir, want) > 0.9999, "az {az_deg}: light at {:?}, expected {want:?}", l.dir);
            }
            // The irradiances, from the estimator.
            let top = [0.0f32, 0.0, 1.0];
            let toward_key = lights.key.dir;
            let toward_fill = lights.fill.dir;
            let estimate = |nrm: [f32; 3]| {
                let n = 200_000u32;
                let mut sum = 0.0f64;
                for i in 0..n {
                    let row = halton_row(i);
                    let mut smp = Sampler::new(i % 601, i / 601, 601, i, 0x7a11, &row);
                    if let Some((dir, pdf_l, radiance, _)) = tracer.pick_light([0.0; 3], &mut smp) {
                        let c = dot(dir, nrm).max(0.0);
                        if c > 0.0 && pdf_l > 0.0 {
                            sum += (radiance[1] * c / pdf_l) as f64;
                        }
                    }
                }
                (sum / n as f64) as f32
            };
            let key_face = estimate(toward_key);
            let key_fill = 0.8 * dot(toward_fill, toward_key).max(0.0) + 1.6 * dot(lights.rim.dir, toward_key).max(0.0);
            assert!((key_face - (3.2 + key_fill)).abs() < 0.08, "az {az_deg}: square to the key: {key_face}, expected 3.2 (+{key_fill})");
            let top_face = estimate(top);
            let want_top = 3.2 * 40f32.to_radians().sin() + 0.8 * 15f32.to_radians().sin() + 1.6 * 35f32.to_radians().sin();
            assert!((top_face - want_top).abs() < 0.08, "az {az_deg}: top face {top_face}, expected {want_top}");
            let fill_face = estimate(toward_fill);
            let fill_key = 3.2 * dot(toward_key, toward_fill).max(0.0);
            assert!((fill_face - (0.8 + fill_key)).abs() < 0.05, "az {az_deg}: facing the fill: {fill_face}, expected 0.8 (+{fill_key})");
            // Key:fill on opposite faces is about 2.4:1 — the open look.
            let ratio = key_face / fill_face;
            assert!((2.0..3.0).contains(&ratio), "az {az_deg}: key:fill {ratio}, expected about 2.4:1");
            // The environment's share, in E/π terms, as the rasterizer's
            // widened cosine lobe reads it: the sky's 0.60 on the top face,
            // and between the horizon's 0.35 and the sky's 0.60 toward the
            // key (the tracer integrates the gradient itself).
            let env_top = super::super::shading::env_diffuse(&lights, top)[1];
            let env_key = super::super::shading::env_diffuse(&lights, toward_key)[1];
            let env_down = super::super::shading::env_diffuse(&lights, [0.0, 0.0, -1.0])[1];
            assert!((env_top - 0.60).abs() < 1e-5, "top face sees the sky: {env_top}");
            assert!((0.35..=0.60).contains(&env_key), "the key-facing face sees sky and horizon: {env_key}");
            assert!((env_down - 0.25).abs() < 1e-5, "the underside sees the ground: {env_down}");
        }
        // And the studio rig, next to it, is what it always was.
        let studio = Lights::studio();
        assert_eq!(studio.key.radiance, [5.00, 4.80, 4.40]);
        assert!(studio.rim_art, "the studio rim keeps its art factor");
    }

    /// The product-shot backdrop: a floor of
    /// albedo 0.40, lit by the product key at 40° plus the sky, comes out
    /// close to the horizon tone 0.35, which is what makes the floor and
    /// the wall read as one cyclorama with no line between them.
    #[test]
    fn the_studio_backdrop_floor_meets_the_horizon() {
        let opts = RenderOptions {
            lighting: Lighting::Product,
            backdrop: Some(Backdrop::Studio),
            path: PathTraceOptions {
                ground_albedo: Backdrop::Studio.floor_albedo(),
                show_environment: true,
                ..PathTraceOptions::default()
            },
            ..base_opts(48, 48, 64)
        };
        // A camera looking down at bare floor from the product elevation.
        let cam = Camera {
            eye: Vec3::new(0.0, -100.0, 32.0),
            target: Vec3::ZERO,
            up: Vec3::Z,
            fov_degrees: 24.0,
            orthographic: false,
        };
        // The scene needs one item for a floor to exist at all; a tiny box
        // far off to the side gives the floor its height and stays out of
        // the frame.
        let dot_mesh = box_mesh([200.0, 200.0, 0.0], [201.0, 201.0, 1.0]);
        let mat = Material::default();
        let hdr = trace(&[Item::new(&dot_mesh, &mat, &[])], &cam, &opts);
        let floor = lum(hdr.mean(8, 8, 40, 40));
        assert!((0.27..0.43).contains(&floor), "the lit floor is {floor}, the horizon is 0.35");
    }

    /// The lens: with an f-number set, a point far from the focus plane
    /// spreads over its neighbours and a point on it does not; without one
    /// the image is byte-identical to the pinhole image, sampler
    /// dimensions included.
    #[test]
    fn depth_of_field_blurs_off_the_focus_plane_and_costs_nothing_when_off() {
        let (mesh, mat, cam) = small_scene();
        let items = [Item::new(&mesh, &mat, &[])];
        let opts = base_opts(64, 48, 16);
        let pinhole = render(&items, &cam, &opts).0;
        let zero = render(&items, &cam, &RenderOptions { path: PathTraceOptions { dof_f_number: 0.0, ..PathTraceOptions::default() }, ..opts.clone() }).0;
        assert_eq!(fnv1a64(&pinhole.rgba), fnv1a64(&zero.rgba), "f/0 must be the pinhole image to the byte");
        // A sharp emissive edge behind the focus plane blurs at f/1.
        let far = quad([-40.0, 200.0, -40.0], [0.0, 200.0, -40.0], [0.0, 200.0, 40.0], [-40.0, 200.0, 40.0]);
        let lamp = Material { base_color: [0.0; 3], emissive: [4.0, 4.0, 4.0], ..Material::default() };
        let cam = Camera { eye: Vec3::new(0.0, -60.0, 0.0), target: Vec3::ZERO, up: Vec3::Z, fov_degrees: 30.0, orthographic: false };
        let opts = RenderOptions {
            path: PathTraceOptions { direct_scale: 0.0, ground: false, clamp: 0.0, ..PathTraceOptions::default() },
            ..base_opts(96, 32, 64)
        };
        let edge_width = |f: f32| {
            let hdr = trace(&[Item::new(&far, &lamp, &[])], &cam, &RenderOptions { path: PathTraceOptions { dof_f_number: f, ..opts.path.clone() }, ..opts.clone() });
            // Pixels along the middle row that are neither lit nor dark:
            // the width of the edge's transition.
            (0..96).filter(|&x| { let v = lum(hdr.mean(x, 14, x + 1, 18)); v > 0.2 && v < 3.8 }).count()
        };
        let sharp = edge_width(0.0);
        let blurred = edge_width(1.0);
        assert!(blurred > sharp + 4, "f/1 did not widen the edge: {sharp} -> {blurred} pixels");
    }

    /// The edge break: with a radius of zero the image is the one before
    /// the feature existed, to the bit (no index is built, nothing is
    /// consulted); with a radius, only pixels whose primary hit lies within
    /// it of a crease change, and the crease catches the light. The bright
    /// line a real edge break shows is a specular event — the bevel's
    /// normals sweep from one face to the other, and somewhere on the sweep
    /// the mirror direction finds a light neither face reflects — so the
    /// scene stages exactly that: a metal box, the camera on the bisector
    /// of its top-front edge, and a panel light behind the camera. The flat
    /// top face mirrors that panel up and back, the front face down and
    /// forward; only the bevel mirrors it into the lens. Lit directly only
    /// (one bounce, no floor, no clamp), so a pixel is a function of its
    /// own hit's normal and the "nothing else" half of the claim is exact
    /// rather than statistical.
    #[test]
    fn the_edge_break_brightens_crease_pixels_and_nothing_else() {
        let block = box_mesh([-10.0, -10.0, 0.0], [10.0, 10.0, 12.0]);
        let mat = Material { base_color: [0.9; 3], roughness: 0.25, metallic: 1.0, ..Material::default() };
        // The top-front edge runs along x at (y, z) = (-10, 12); its
        // bisector normal is (0, -1, 1) / sqrt 2, and the camera sits on it.
        let along = Vec3::new(0.0, -1.0, 1.0).normalized();
        let edge_mid = Vec3::new(0.0, -10.0, 12.0);
        let cam = Camera { eye: edge_mid + along * 80.0, target: edge_mid, up: Vec3::Z, fov_degrees: 20.0, orthographic: false };
        // A 16 mm panel 20 mm behind the camera, facing the box.
        let pc = cam.eye + along * 20.0;
        let (ex, ey) = (Vec3::X, along.cross(Vec3::X).normalized());
        let corner = |sx: f64, sy: f64| {
            let p = pc + ex * (8.0 * sx) + ey * (8.0 * sy);
            [p.x as f32, p.y as f32, p.z as f32]
        };
        // Wound so the panel faces the box (`quad` takes its normal from
        // `(p1 - p0) x (p3 - p0)`), or the scene is dark and the test would
        // be measuring the environment.
        let (c0, c1, c2, c3) = (corner(-1.0, -1.0), corner(-1.0, 1.0), corner(1.0, 1.0), corner(1.0, -1.0));
        let toward_box = [-along.x as f32, -along.y as f32, -along.z as f32];
        let panel = if dot(cross(sub(c1, c0), sub(c3, c0)), toward_box) > 0.0 { quad(c0, c1, c2, c3) } else { quad(c0, c3, c2, c1) };
        let lamp = Material { base_color: [0.0; 3], emissive: [20.0, 20.0, 20.0], ..Material::default() };
        let items = [Item::new(&block, &mat, &[]), Item::new(&panel, &lamp, &[])];
        let opts = RenderOptions {
            path: PathTraceOptions {
                direct_scale: 0.0,
                ground: false,
                clamp: 0.0,
                max_bounces: 1,
                show_environment: true,
                ..PathTraceOptions::default()
            },
            ..base_opts(128, 96, 24)
        };
        let with = |r: f32| trace(&items, &cam, &RenderOptions { path: PathTraceOptions { edge_break_mm: r, ..opts.path.clone() }, ..opts.clone() });
        let plain = trace(&items, &cam, &opts);
        assert_eq!(plain.px, with(0.0).px, "a zero radius must be the image before the feature");
        let radius = 1.5;
        let broken = with(radius);
        // Sort the pixels by their centre ray's distance to a crease, using
        // the index itself: inside the radius they may change; beyond it
        // plus a pixel's worth of jitter they must not. A pixel's jitter
        // covers three times its footprint on the top face, which the
        // product camera sees at 18°, so the margin is generous.
        let scene = Scene::from_items(&items);
        let view = View::new(&cam, 128, 96);
        let tracer = Tracer::new(&items, &scene, &opts, view.azimuth());
        let near = EdgeBreak::build(&block, radius, 30.0).expect("a box has twelve creases");
        assert_eq!(near.creases.len(), 12, "a box has twelve crease edges, found {}", near.creases.len());
        let footprint = view.footprint((cam.target - cam.eye).length() as f32);
        let far = EdgeBreak::build(&block, radius + 5.0 * footprint, 30.0).unwrap();
        let (mut n_near, mut n_far, mut n_changed) = (0, 0, 0);
        let (mut peak_plain, mut peak_broken) = (0.0f32, 0.0f32);
        for (x, y, c) in plain.covered() {
            let (o, d) = view.ray(x as f32 + 0.5, y as f32 + 0.5);
            let Some(s) = tracer.intersect(o, d, tracer.eps, tracer.far, 0.0, &view, true) else { continue };
            let b = broken.at(x, y);
            if far.nearest(s.p, s.ng).is_none() {
                n_far += 1;
                assert_eq!(c, b, "pixel ({x},{y}) is far from every crease and changed");
            } else if near.nearest(s.p, s.ng).is_some_and(|(dist, _)| dist < 0.5 * radius) {
                n_near += 1;
                n_changed += usize::from(c != b);
                peak_plain = peak_plain.max(lum(c));
                peak_broken = peak_broken.max(lum(b));
            }
        }
        assert!(n_far > 200 && n_near > 50, "the test saw {n_far} far and {n_near} near pixels");
        assert!(n_changed > n_near / 2, "only {n_changed} of {n_near} crease pixels changed");
        assert!(peak_broken > peak_plain * 1.05, "no crease pixel caught the light: brightest {peak_plain} -> {peak_broken}");
        // And the brightest crease pixel now outshines the brightest flat
        // pixel anywhere on the box: the line reads as a line.
        let flat_peak = plain.covered().map(|(_, _, c)| lum(c)).fold(0.0f32, f32::max);
        assert!(peak_broken > flat_peak, "the edge ({peak_broken}) is no brighter than the brightest face ({flat_peak})");
    }

    /// The anisotropic lobe follows a machined texture's grooves: given a
    /// groove direction the frame's smooth axis lies along it, whatever
    /// the print's layer direction would have been, and the top-face fade
    /// is off (a turned end face has grooves). Without one, the frame and
    /// the lobe are the FDM ones to the bit — the only rule the layer-line
    /// presets ever had.
    #[test]
    fn anisotropy_follows_the_groove_direction_and_the_fdm_rule_otherwise() {
        let n = normalize([0.3, -0.2, 0.93]);
        for groove in [[1.0f32, 0.0, 0.0], normalize([0.0, 1.0, 0.4]), normalize([-0.5, 0.5, 0.1])] {
            let f = Onb::for_surface(n, groove);
            assert!(dot(f.t, n).abs() < 1e-5 && dot(f.b, n).abs() < 1e-5, "the frame is not tangent");
            let want = normalize(sub(groove, scale(n, dot(n, groove))));
            assert!(dot(f.t, want) > 0.9999, "t {:?} does not follow the groove {want:?}", f.t);
        }
        // No groove: the layer frame, bit for bit.
        for nrm in [[1.0f32, 0.0, 0.0], normalize([0.2, 0.5, 0.8]), [0.0, 0.0, 1.0]] {
            let a = Onb::for_surface(nrm, [0.0; 3]);
            let b = Onb::layer_aligned(nrm);
            assert_eq!((a.t, a.b, a.n), (b.t, b.b, b.n));
        }
        // On a top face the FDM lobe is round; a turned top face is not.
        let fdm = Bsdf::new(&Pbr { roughness: 0.4, anisotropy: 0.8, ..Pbr::default() }, [0.0, 0.0, 1.0], true);
        assert!((fdm.ax - fdm.ay).abs() < 1e-6);
        let turned = Bsdf::new(&Pbr { roughness: 0.4, anisotropy: 0.8, groove: [1.0, 0.0, 0.0], ..Pbr::default() }, [0.0, 0.0, 1.0], true);
        assert!(turned.ay > turned.ax * 3.0, "the grooves did not stretch the lobe: ax {} ay {}", turned.ax, turned.ay);
        // Slope variance the footprint filtered away widens the lobe across
        // the grooves and leaves it alone along them.
        let far = Bsdf::new(
            &Pbr { roughness: 0.16, anisotropy: 0.0, groove: [1.0, 0.0, 0.0], slope_var: 0.012, ..Pbr::default() },
            [0.0, 0.0, 1.0],
            true,
        );
        let near = Bsdf::new(&Pbr { roughness: 0.16, anisotropy: 0.0, groove: [1.0, 0.0, 0.0], ..Pbr::default() }, [0.0, 0.0, 1.0], true);
        assert_eq!(far.ax, near.ax);
        assert!((far.ay - (near.ay * near.ay + 2.0 * 0.012).sqrt()).abs() < 1e-6, "ay {} vs {}", far.ay, near.ay);
        // And a material with no groove and no variance is untouched.
        let plain = Bsdf::new(&Pbr { roughness: 0.4, anisotropy: 0.55, ..Pbr::default() }, normalize([1.0, 0.0, 0.3]), true);
        let (alpha, k) = (0.4f32 * 0.4, 0.55 * (1.0 - normalize([1.0, 0.0, 0.3])[2].abs()));
        assert_eq!((plain.ax, plain.ay), ((alpha * (1.0 - 0.70 * k)).clamp(1e-5, 1.0), (alpha * (1.0 + 3.00 * k)).clamp(1e-5, 1.0)));
    }

    /// A machined texture is filtered where the camera sees it. A turned
    /// disc whose groove pitch is a twentieth of a pixel: the primary hit's
    /// relief must be flat (the shading normal is the geometric one) with
    /// the grooves' slope variance handed to the roughness. Before the
    /// footprint was taken at the hit it was taken at the start of the
    /// segment — the eye, for a camera ray — so the footprint there was
    /// nothing, the relief came through unfiltered and tilted the normal by
    /// its full amplitude, and a pitch of two pixels aliased into hatching.
    #[test]
    fn machined_grooves_are_filtered_at_the_primary_hit() {
        let disc = quad([-20.0, 0.0, -20.0], [20.0, 0.0, -20.0], [20.0, 0.0, 20.0], [-20.0, 0.0, 20.0]);
        let frames = [super::super::FaceFrame { axis_point: [0.0; 3], axis: [0.0, 1.0, 0.0], origin: [0.0; 3], dir: [1.0, 0.0, 0.0], turned: true }];
        let cam = Camera { eye: Vec3::new(0.0, -90.0, 0.0), target: Vec3::ZERO, up: Vec3::Z, fov_degrees: 28.0, orthographic: false };
        let opts = base_opts(96, 96, 1);
        let view = View::new(&cam, 96, 96);
        // A pixel is about 0.47 mm on the disc; the pitch is 0.02 mm.
        let mat = Material { base_color: [0.9; 3], roughness: 0.2, metallic: 1.0, texture: "turned".into(), texture_scale: 0.02, ..Material::default() };
        let mut item = Item::new(&disc, &mat, &[]);
        item.face_frames = &frames;
        let items = [item];
        let scene = Scene::from_items(&items);
        let tracer = Tracer::new(&items, &scene, &opts, view.azimuth());
        let (mut n, mut worst_tilt) = (0, 0.0f32);
        for (x, y) in [(30u32, 40u32), (48, 48), (60, 55), (40, 66)] {
            let (o, d) = view.ray(x as f32 + 0.5, y as f32 + 0.5);
            let s = tracer.intersect(o, d, tracer.eps, tracer.far, 0.0, &view, true).expect("the disc fills the middle of the frame");
            assert!(s.mat.slope_var > 1e-4, "pixel ({x}, {y}): the grooves were not filtered, slope variance {}", s.mat.slope_var);
            worst_tilt = worst_tilt.max(dot(s.ns, s.ng).abs().min(1.0).acos());
            n += 1;
        }
        assert_eq!(n, 4);
        assert!(worst_tilt < 1e-3, "sub-pixel grooves still tilt the primary hit's normal by {worst_tilt} rad");
    }

    /// A machined texture is stable under resolution: the same disc
    /// rendered at one size and at four times that size, box-averaged back
    /// down, agree pixel by pixel, because the relief is filtered by the
    /// pixel footprint (grooves below a pixel average to flat and widen
    /// the lobe instead) rather than left to alias. Also deterministic:
    /// twice is the same image.
    #[test]
    fn machined_textures_converge_across_resolutions() {
        // A 40 mm disc facing the camera, turned about its own axis,
        // under the environment alone so the shading is smooth and the
        // grooves are the only structure.
        let disc = quad([-20.0, 0.0, -20.0], [20.0, 0.0, -20.0], [20.0, 0.0, 20.0], [-20.0, 0.0, 20.0]);
        let frames = [super::super::FaceFrame { axis_point: [0.0; 3], axis: [0.0, 1.0, 0.0], origin: [0.0; 3], dir: [1.0, 0.0, 0.0], turned: true }];
        let cam = Camera { eye: Vec3::new(0.0, -90.0, 0.0), target: Vec3::ZERO, up: Vec3::Z, fov_degrees: 28.0, orthographic: false };
        let opts = |w: u32, h: u32, spp: u32| RenderOptions {
            path: PathTraceOptions { direct_scale: 0.0, ground: false, clamp: 0.0, max_bounces: 2, show_environment: true, ..PathTraceOptions::default() },
            ..base_opts(w, h, spp)
        };
        for kind in ["turned", "milled"] {
            // The pitch is chosen so that the grooves are about half a pixel
            // at the small size and two pixels at the large one: the case
            // where an unfiltered pattern aliases and a filtered one hands
            // over to roughness.
            let mat = Material {
                base_color: [0.9; 3],
                roughness: 0.2,
                metallic: 1.0,
                texture: kind.into(),
                texture_scale: 0.6,
                ..Material::default()
            };
            let mut item = Item::new(&disc, &mat, &[]);
            item.face_frames = &frames;
            let items = [item];
            let small = trace(&items, &cam, &opts(48, 48, 16));
            let again = trace(&items, &cam, &opts(48, 48, 16));
            assert_eq!(small.px, again.px, "{kind}: not deterministic");
            let large = trace(&items, &cam, &opts(192, 192, 16));
            let (mut worst, mut sum, mut n) = (0.0f32, 0.0f32, 0);
            for y in 12..36u32 {
                for x in 12..36u32 {
                    let a = lum(small.at(x, y));
                    let b = lum(large.mean(4 * x, 4 * y, 4 * x + 4, 4 * y + 4));
                    let d = (a - b).abs() / b.max(1e-3);
                    worst = worst.max(d);
                    sum += d;
                    n += 1;
                }
            }
            let mean = sum / n as f32;
            assert!(mean < 0.06, "{kind}: the small image is {mean:.3} (mean) off the box-averaged large one");
            assert!(worst < 0.25, "{kind}: worst pixel {worst:.3} off");
        }
    }

    /// The denoiser takes the grain out and leaves the edges in. Grain is
    /// measured as grain: the same scene at two seeds differs only in
    /// noise, so half the variance of their difference is the noise
    /// variance, before and after filtering — on a matte sphere under the
    /// product rig at 16 paths it falls by more than half, while the
    /// region's mean moves by about 2 %. (On a glossy metal the same
    /// filter takes about a third: a highlight's structure is at the
    /// grain's own scale and has to be kept, which is the limit `grain_bench`
    /// measures.) The silhouette stays where it was: every backdrop pixel is
    /// bit-identical, nothing bleeds out over it. Off by default: the image
    /// is the traced one to the bit.
    #[test]
    fn the_denoiser_removes_grain_and_keeps_edges() {
        let (sphere, _) = uv_sphere(24, 48);
        let mat = Material { base_color: [0.7, 0.3, 0.2], roughness: 0.6, ..Material::default() };
        let items = [Item::new(&sphere, &mat, &[])];
        let (lo, hi) = sphere.bbox().unwrap();
        let cam = Camera::preset("product", lo, hi, 0.4);
        // The backdrop is the flat `background` (the sky still lights the
        // sphere), so every backdrop pixel is one constant and any surface
        // light bleeding onto it would show as a change.
        let opts = |seed: u32| RenderOptions {
            lighting: Lighting::Product,
            path: PathTraceOptions { ground: false, seed, ..PathTraceOptions::default() },
            ..base_opts(96, 72, 16)
        };
        let noisy = trace(&items, &cam, &opts(0));
        let (hdr_a, _, guides_a, _) = trace_hdr_guided(&items, &cam, &opts(0), true);
        assert_eq!(hdr_a, noisy.px, "recording the guides must not change the image");
        let (hdr_b, _, guides_b, _) = trace_hdr_guided(&items, &cam, &opts(1), true);
        let (mut clean_a, mut clean_b) = (hdr_a.clone(), hdr_b.clone());
        denoise(&mut clean_a, &guides_a.unwrap(), 96, 72, 4);
        denoise(&mut clean_b, &guides_b.unwrap(), 96, 72, 4);
        // A patch on the sphere, well inside the silhouette.
        let patch = || (26..38u32).flat_map(|y| (38..50u32).map(move |x| (y * 96 + x) as usize));
        let noise_var = |a: &Vec<[f32; 3]>, b: &Vec<[f32; 3]>| {
            let n = patch().count() as f32;
            0.5 * patch().map(|i| (lum(a[i]) - lum(b[i])).powi(2)).sum::<f32>() / n
        };
        let mean = |a: &Vec<[f32; 3]>| patch().map(|i| lum(a[i])).sum::<f32>() / patch().count() as f32;
        let (v0, v1) = (noise_var(&hdr_a, &hdr_b), noise_var(&clean_a, &clean_b));
        assert!(v0 > 0.0, "the 16-path render has no grain to remove?");
        assert!(v1 < v0 * 0.6, "noise variance {v0} -> {v1}: the filter did little");
        // The patch is on a sphere, so it carries a real shading gradient
        // the filter tempers a little at its edges; 3 % is that, not bias.
        let (m0, m1) = (mean(&hdr_a), mean(&clean_a));
        assert!((m1 - m0).abs() < 0.03 * m0.max(0.05), "the filter moved the mean {m0} -> {m1}");
        // Edges: every background pixel is unchanged to rounding (its only
        // admitted neighbours are the same constant; a surface pixel is
        // stopped by the depth test), so nothing bleeds out over the
        // silhouette.
        let mut bg_same = 0;
        for (i, &m) in noisy.bg.iter().enumerate() {
            if m != 0 {
                for k in 0..3 {
                    let (a, b) = (clean_a[i][k], hdr_a[i][k]);
                    assert!((a - b).abs() <= 1e-5 * b.abs().max(1e-3), "background pixel {i} changed: {b} -> {a}");
                }
                bg_same += 1;
            }
        }
        assert!(bg_same > 1000, "the test saw {bg_same} background pixels");
        let opts = opts(0);
        // And the traced image itself is untouched when the option is off.
        let plain = render(&items, &cam, &opts).0;
        let off = render(&items, &cam, &RenderOptions { path: PathTraceOptions { denoise: false, ..opts.path.clone() }, ..opts.clone() }).0;
        assert_eq!(fnv1a64(&plain.rgba), fnv1a64(&off.rgba));
        let on = render(&items, &cam, &RenderOptions { path: PathTraceOptions { denoise: true, ..opts.path.clone() }, ..opts.clone() }).0;
        assert_ne!(fnv1a64(&plain.rgba), fnv1a64(&on.rgba), "--denoise did nothing");
    }

    /// Not a test: a grain measurement.
    /// A metal sphere and a matte box under the product rig, 64 paths
    /// against a 4096-path reference, RMS error over the covered pixels
    /// with the sky strategy on and off, and with the denoiser. Run with
    /// `--test_arg=--ignored --test_arg=grain_bench --test_output=all`.
    #[test]
    #[ignore]
    fn grain_bench() {
        let (sphere, _) = uv_sphere(32, 64);
        let block = box_mesh([-1.6, -1.6, -1.0], [-0.6, -0.6, 0.2]);
        let metal = Material { base_color: [0.9; 3], roughness: 0.16, metallic: 1.0, anisotropy: 0.5, ..Material::default() };
        let matte = Material { base_color: [0.7, 0.3, 0.2], ..Material::default() };
        let cam = Camera::preset("product", Vec3::new(-1.6, -1.6, -1.0), Vec3::new(1.0, 1.0, 1.0), 0.3);
        for (what, sphere_mat) in [("metal sphere", &metal), ("matte sphere", &matte)] {
        let items = [Item::new(&sphere, sphere_mat, &[]), Item::new(&block, &matte, &[])];
        let opts = |spp: u32, env: bool, dn: bool| RenderOptions {
            lighting: Lighting::Product,
            backdrop: Some(Backdrop::Studio),
            path: PathTraceOptions {
                ground_albedo: [0.4; 3],
                ground_roughness: 0.35,
                show_environment: true,
                env_sampling: env,
                denoise: dn,
                ..PathTraceOptions::default()
            },
            ..base_opts(160, 120, spp)
        };
        let reference = trace(&items, &cam, &opts(4096, true, false));
        let rms = |hdr: &Vec<[f32; 3]>| {
            let (mut s, mut n) = (0.0f64, 0);
            for (i, &m) in reference.bg.iter().enumerate() {
                if m == 0 {
                    let d = lum(hdr[i]) - lum(reference.px[i]);
                    s += (d * d) as f64;
                    n += 1;
                }
            }
            (s / n as f64).sqrt()
        };
        for (env, dn) in [(false, false), (true, false), (false, true), (true, true)] {
            let o = opts(64, env, dn);
            let (mut hdr, _, guides, _) = trace_hdr_guided(&items, &cam, &o, dn);
            if let Some(g) = &guides {
                denoise(&mut hdr, g, 160, 120, 4);
            }
            println!("{what}: sky sampling {env}, denoise {dn}: rms error {:.4} at 64 paths", rms(&hdr));
        }
        }
    }

    /// A mesh body's creases are found by the same rule: a welded box
    /// tessellated with shared vertices and one built face by face give the
    /// same twelve, and a smooth cylinder's seams give none.
    #[test]
    fn crease_edges_come_from_the_mesh_by_the_crease_rule() {
        let (sphere, _) = uv_sphere(24, 48);
        assert!(EdgeBreak::build(&sphere, 0.5, 30.0).is_none(), "a smooth sphere has no crease to break");
        let block = box_mesh([0.0, 0.0, 0.0], [10.0, 20.0, 5.0]);
        let eb = EdgeBreak::build(&block, 0.5, 30.0).unwrap();
        assert_eq!(eb.creases.len(), 12);
        // Every crease of a box folds by exactly 90°.
        for c in &eb.creases {
            assert!(dot(c.n1, c.n2).abs() < 1e-5, "a box crease with normals {:?} {:?}", c.n1, c.n2);
        }
        // The query finds the nearest crease of the face the point is on,
        // and reports the other face's normal.
        let (d, other) = eb.nearest([0.2, 10.0, 5.0], [0.0, 0.0, 1.0]).expect("near the top-left edge");
        assert!((d - 0.2).abs() < 1e-5, "distance {d}");
        assert!((other[0] + 1.0).abs() < 1e-5, "the neighbour across that edge faces -x: {other:?}");
        // A point 0.2 in from the edge on the *side* face sees the top face.
        let (_, other) = eb.nearest([0.0, 10.0, 4.8], [-1.0, 0.0, 0.0]).unwrap();
        assert!((other[2] - 1.0).abs() < 1e-5, "{other:?}");
        // Out of range, and on a face the crease does not bound: nothing.
        assert!(eb.nearest([2.0, 10.0, 5.0], [0.0, 0.0, 1.0]).is_none());
        assert!(eb.nearest([0.2, 10.0, 5.0], [0.0, -1.0, 0.0]).is_none(), "the front face does not share the top-left edge");
        // No index at all for radius 0, and none when the crease angle is
        // set past a box corner.
        assert!(EdgeBreak::build(&block, 0.0, 30.0).is_none());
        assert!(EdgeBreak::build(&block, 0.5, 95.0).is_none());
    }

    #[test]
    fn material_for_reads_the_contract_and_derives_transmission() {
        // An explicit transmission wins.
        let m = Material { transmission: 0.4, opacity: 0.9, ..Material::default() };
        assert!((material_for(&m).transmission - 0.4).abs() < 1e-6);
        // A script that only knows `opacity` still refracts.
        let m = Material { opacity: 0.3, ..Material::default() };
        assert!((material_for(&m).transmission - 0.7).abs() < 1e-6);
        // An opaque material does not.
        assert_eq!(material_for(&Material::default()).transmission, 0.0);
        // Ranges are clamped once, here, so the integrator never re-checks.
        let wild = Material {
            roughness: -5.0,
            metallic: 9.0,
            ior: 99.0,
            scatter_g: -20.0,
            anisotropy: 4.0,
            emissive: [-1.0, 2.0, 3.0],
            ..Material::default()
        };
        let p = material_for(&wild);
        assert_eq!(p.roughness, 0.03);
        assert_eq!(p.metallic, 1.0);
        assert_eq!(p.ior, 2.5);
        assert_eq!(p.scatter_g, -0.95);
        assert_eq!(p.anisotropy, 1.0);
        assert_eq!(p.emissive, [0.0, 2.0, 3.0]);
    }

    /// Every preset reaches the tracer without producing a NaN, a negative
    /// radiance or a black image, and the ones that say "clear" really do
    /// let the background through.
    #[test]
    fn every_preset_path_traces() {
        let (mesh, _) = uv_sphere(20, 40);
        let (lo, hi) = mesh.bbox().unwrap();
        let cam = Camera::preset("iso", lo, hi, 0.2);
        let opts = base_opts(40, 40, 12);
        for name in super::super::material_preset_names() {
            let mat = super::super::material_preset(name).expect("listed preset");
            let hdr = trace(&[Item::new(&mesh, &mat, &[])], &cam, &opts);
            for (x, y, c) in hdr.covered() {
                assert!(c.iter().all(|v| v.is_finite() && *v >= 0.0), "{name} produced {c:?} at ({x},{y})");
            }
            let m = lum(hdr.mean(16, 16, 24, 24));
            assert!(m > 0.005, "{name} rendered black ({m})");
        }
    }


    /// The sun in the tracer: a fourth cone of the sun's own
    /// angular radius, whatever the rig's light-size option says, taking
    /// its share of the shadow rays by power; on the floor the box's shadow
    /// is a real shadow — lit by the sky and the turned-down rig alone —
    /// and a sun below the horizon leaves the three cones as they were.
    #[test]
    fn the_sun_is_a_narrow_cone_that_shadows_the_floor() {
        use super::super::testing::unit_box;
        use super::super::SunLight;
        use crate::geom::Vec3;
        let scene = Scene::from_items(&[]);
        let opts = RenderOptions {
            sun: Some(SunLight::new(Vec3::new(1.0, 0.0, 1.0))),
            path: PathTraceOptions { light_angle_degrees: 3.0, ..PathTraceOptions::default() },
            ..RenderOptions::default()
        };
        let tracer = Tracer::new(&[], &scene, &opts, 0.0);
        assert_eq!(tracer.cones.len(), 4, "the rig plus the sun");
        let sun = tracer.cones.last().unwrap();
        assert!((sun.cos_max - 0.265f32.to_radians().cos()).abs() < 1e-7, "the sun's disc is 0.53° across: {}", sun.cos_max);
        assert!(sun.q > 0.3, "the sun carries most of the power: q = {}", sun.q);
        let q: f32 = tracer.cones.iter().map(|c| c.q).sum();
        assert!((q - 1.0).abs() < 1e-5);
        let night = RenderOptions { sun: Some(SunLight::new(Vec3::new(1.0, 0.0, -0.5))), ..RenderOptions::default() };
        assert_eq!(Tracer::new(&[], &scene, &night, 0.0).cones.len(), 3);

        let (mesh, edges) = unit_box();
        let mat = Material::default();
        let items = [Item::new(&mesh, &mat, &edges)];
        let (lo, hi) = (Vec3::new(-2.0, -1.0, 0.0), Vec3::new(3.0, 2.0, 1.0));
        let mut cam = Camera::preset("top", lo, hi, 0.1);
        cam.fit(lo, hi, 0.1, 4.0 / 3.0);
        let (w, h) = (96u32, 72u32);
        let opts = RenderOptions {
            width: w,
            height: h,
            samples: 64,
            threads: 1,
            sun: Some(SunLight::new(Vec3::new(1.0, 0.0, 1.0))),
            ..RenderOptions::default()
        };
        let (hdr, _, _) = trace_hdr(&items, &cam, &opts);
        let view = View::new(&cam, w, h);
        let px = |p: [f32; 3]| {
            let s = view.project(p).expect("in front of the camera");
            s[0] as usize + s[1] as usize * w as usize
        };
        let shadow = luminance(hdr[px([-0.5, 0.5, 0.0])]);
        let open = luminance(hdr[px([2.0, 0.5, 0.0])]);
        assert!(shadow < 0.6 * open, "the floor in the box's shadow is dark: {shadow} vs {open}");
        let plain = RenderOptions { sun: None, ..opts.clone() };
        let (flat, _, _) = trace_hdr(&items, &cam, &plain);
        let a = luminance(flat[px([-0.5, 0.5, 0.0])]);
        let b = luminance(flat[px([2.0, 0.5, 0.0])]);
        assert!((a - b).abs() < 0.25 * b.max(1e-6), "without a sun the two floor probes are alike: {a} vs {b}");
    }

    /// Guide geometry: a translucent box marked `shadow: false`
    /// stands in the sun's path, and the floor behind it is as lit as with
    /// the box absent — no shadow ray stops at it or is tinted by it, no
    /// bounce meets it, and with it the scene's only clear body gone from
    /// what light can meet, the shadow test is the opaque-only one — while
    /// the same box casting shadows tints that floor, and the camera still
    /// sees the guide itself. Two pegs pin the scene's bounds, so the three
    /// renders share one floor, one `eps` and one camera fit and differ
    /// only by the box.
    #[test]
    fn a_guide_body_casts_no_shadow_in_the_tracer() {
        use super::super::SunLight;
        use super::super::testing::unit_box;
        let (mesh, edges) = unit_box();
        let peg_a = box_mesh([-2.0, -1.0, 0.0], [-1.9, -0.9, 0.1]);
        let peg_b = box_mesh([2.9, 1.9, 0.9], [3.0, 2.0, 1.0]);
        let translucent = Material { base_color: [0.95, 0.75, 0.25], opacity: 0.3, roughness: 0.6, ..Material::default() };
        let guide = Material { shadow: false, ..translucent.clone() };
        let peg = Material::default();
        let pegs = || [Item::new(&peg_a, &peg, &[]), Item::new(&peg_b, &peg, &[])];
        let (lo, hi) = (Vec3::new(-2.0, -1.0, 0.0), Vec3::new(3.0, 2.0, 1.0));
        let mut cam = Camera::preset("top", lo, hi, 0.1);
        cam.fit(lo, hi, 0.1, 4.0 / 3.0);
        let (w, h) = (96u32, 72u32);
        let opts = RenderOptions {
            width: w,
            height: h,
            samples: 64,
            threads: 1,
            sun: Some(SunLight::new(Vec3::new(1.0, 0.0, 1.0))),
            ..RenderOptions::default()
        };
        let view = View::new(&cam, w, h);
        let px = |p: [f32; 3]| {
            let s = view.project(p).expect("in front of the camera");
            (s[0] as u32, s[1] as u32)
        };
        // A patch of floor in the box's shadow, one in the open, and the
        // box's own top.
        let shaded = px([-0.5, 0.5, 0.0]);
        let open = px([2.0, 0.5, 0.0]);
        let top = px([0.5, 0.5, 1.0]);
        let probe = |hdr: &Hdr, (x, y): (u32, u32)| hdr.mean(x - 2, y - 2, x + 3, y + 3);
        let with = |mat: &Material| {
            let [a, b] = pegs();
            trace(&[Item::new(&mesh, mat, &edges), a, b], &cam, &opts)
        };
        let shadows = with(&translucent);
        let guided = with(&guide);
        let absent = {
            let [a, b] = pegs();
            trace(&[a, b], &cam, &opts)
        };
        // Two crossings at transmission 0.7 pass 0.49 of the sun, which is
        // the floor's largest light but not its only one: measured, the
        // patch is 12 % darker than the open floor.
        let (s_shaded, s_open) = (lum(probe(&shadows, shaded)), lum(probe(&shadows, open)));
        assert!(s_shaded < 0.93 * s_open, "a translucent box that casts shadows tints the floor behind it: {s_shaded} vs {s_open}");
        let (g_shaded, g_open) = (lum(probe(&guided, shaded)), lum(probe(&guided, open)));
        let a_shaded = lum(probe(&absent, shaded));
        assert!((g_shaded - a_shaded).abs() < 0.02 * a_shaded, "behind a guide the floor is as lit as with no box: {g_shaded} vs {a_shaded}");
        assert!((g_shaded - g_open).abs() < 0.05 * g_open, "and as lit as the open floor: {g_shaded} vs {g_open}");
        // The camera still sees it: over the box the picture is not the
        // floor's.
        let (g_top, a_top) = (probe(&guided, top), probe(&absent, top));
        let diff = (0..3).map(|k| (g_top[k] - a_top[k]).abs()).fold(0.0f32, f32::max);
        assert!(diff > 0.05 * lum(a_top).max(1e-6), "the guide is drawn: {g_top:?} vs the bare floor {a_top:?}");
        // What the tracer built: the guide is not an item light meets.
        let scene = Scene::from_items(&[]);
        let items = [Item::new(&mesh, &guide, &edges)];
        let t = Tracer::new(&items, &scene, &opts, 0.0);
        assert_eq!(t.guide.as_deref(), Some(&[true][..]));
        assert!(!t.has_transmissive, "a clear guide does not put the shadow rays on the see-through path");
        let items = [Item::new(&mesh, &translucent, &edges)];
        let t = Tracer::new(&items, &scene, &opts, 0.0);
        assert!(t.guide.is_none() && t.has_transmissive);
    }

    /// An emissive guide glows to the eye and lights nothing: it is
    /// out of the area-light list, so the floor beside it is lit as with no
    /// box at all, while the camera sees its emission at full strength. A
    /// real lamp in the same scene keeps that list non-empty, so a camera
    /// ray that meets the guide's glowing top asks the list for a triangle
    /// it does not hold — the case an emissive guide adds.
    #[test]
    fn an_emissive_guide_glows_and_lights_nothing() {
        use super::super::testing::unit_box;
        let (mesh, edges) = unit_box();
        let lamp = box_mesh([2.5, -0.9, 0.0], [2.9, -0.5, 0.4]);
        let peg_a = box_mesh([-2.0, -1.0, 0.0], [-1.9, -0.9, 0.1]);
        let peg_b = box_mesh([2.9, 1.9, 0.9], [3.0, 2.0, 1.0]);
        let glow = Material { base_color: [0.0; 3], emissive: [20.0, 20.0, 20.0], ..Material::default() };
        let guide = Material { shadow: false, ..glow.clone() };
        let peg = Material::default();
        let (lo, hi) = (Vec3::new(-2.0, -1.0, 0.0), Vec3::new(3.0, 2.0, 1.0));
        let mut cam = Camera::preset("top", lo, hi, 0.1);
        cam.fit(lo, hi, 0.1, 4.0 / 3.0);
        let (w, h) = (96u32, 72u32);
        let opts = RenderOptions { width: w, height: h, samples: 64, threads: 1, ..RenderOptions::default() };
        let view = View::new(&cam, w, h);
        let px = |p: [f32; 3]| {
            let s = view.project(p).expect("in front of the camera");
            (s[0] as u32, s[1] as u32)
        };
        let probe = |hdr: &Hdr, (x, y): (u32, u32)| hdr.mean(x - 2, y - 2, x + 3, y + 3);
        let beside = px([-0.5, 0.5, 0.0]);
        let top = px([0.5, 0.5, 1.0]);
        let render = |with_box: Option<&Material>| {
            let mut items = vec![Item::new(&lamp, &glow, &[]), Item::new(&peg_a, &peg, &[]), Item::new(&peg_b, &peg, &[])];
            if let Some(m) = with_box {
                items.push(Item::new(&mesh, m, &edges));
            }
            trace(&items, &cam, &opts)
        };
        let lit = render(Some(&glow));
        let guided = render(Some(&guide));
        let absent = render(None);
        let (l_beside, g_beside, a_beside) = (lum(probe(&lit, beside)), lum(probe(&guided, beside)), lum(probe(&absent, beside)));
        assert!(l_beside > 1.25 * a_beside, "an emissive box that is a light brightens the floor beside it: {l_beside} vs {a_beside}");
        assert!((g_beside - a_beside).abs() < 0.02 * a_beside, "a glowing guide does not: {g_beside} vs {a_beside}");
        let g_top = lum(probe(&guided, top));
        assert!(g_top > 15.0, "the camera sees the guide's own emission: {g_top}");
        // What the tracer built: the lamp's triangles are the whole light
        // list, and none of the guide's is in it.
        let scene = Scene::from_items(&[]);
        let items = [Item::new(&lamp, &glow, &[]), Item::new(&mesh, &guide, &edges)];
        let t = Tracer::new(&items, &scene, &opts, 0.0);
        assert_eq!(t.tri_lights.len(), lamp.indices.len() / 3, "the lamp's triangles and nothing else");
        let items = [Item::new(&mesh, &guide, &edges)];
        let t = Tracer::new(&items, &scene, &opts, 0.0);
        assert!(t.tri_lights.is_empty(), "a guide alone is no light");
    }
}
