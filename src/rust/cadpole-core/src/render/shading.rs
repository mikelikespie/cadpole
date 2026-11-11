//! Lights and the surface BRDF: Cook-Torrance with a GGX normal
//! distribution, height-correlated Smith visibility and Schlick Fresnel, lit
//! by a three-light studio rig plus a gradient hemisphere environment.
//!
//! Everything here is scene-linear radiance: [`shade_surface`] returns
//! unbounded HDR, [`shade`] writes it into `Frame::hdr`, and the tonemap
//! is the only place values are squeezed into 0..1. `shade_surface` is a pure
//! function of its arguments so `oit` can reuse it for translucent
//! fragments.
//!
//! Energy notes: diffuse is scaled by `1 - metallic` and by the light's
//! transmitted fraction `1 - F`, so a metal has no diffuse lobe and a
//! dielectric never reflects more than it receives. A white dielectric in the
//! furnace (uniform environment of 1, no direct light) returns ~1.

use super::aa::par_bands;
use super::{
    Backdrop, COV_OPAQUE, Ctx, HIGHLIGHT, Lighting, RenderOptions, add, dot, inverse_tonemap_rgb, lerp3, mul, normalize, scale,
    smoothstep, sub, textures,
};

/// Punctual lights are stand-ins for studio softboxes: clamping the GGX width
/// from below keeps a mirror-smooth material from collapsing its highlight
/// into a single blown pixel (and is what a light of finite size would do).
const MIN_DIRECT_ALPHA: f32 = 0.06;

/// Angular radius of the studio rig's three lights, in degrees, and the
/// value `PathTraceOptions::light_angle_degrees` defaults to: the option is
/// a multiplier on every light's own radius, and at this value it is 1.
pub const STUDIO_LIGHT_DEGREES: f32 = 14.0;

pub struct Light {
    /// Toward the light, unit, world space.
    pub dir: [f32; 3],
    /// Irradiance on a surface square to the light (the rasterizer's
    /// `radiance * n·l`); the path tracer spreads it over the cone.
    pub radiance: [f32; 3],
    /// Angular radius of the light as the path tracer sees it, in degrees:
    /// the softness of its shadows and the size of its reflection. The
    /// rasterizer stands in for it with [`MIN_DIRECT_ALPHA`].
    pub radius_degrees: f32,
}

pub struct Lights {
    pub key: Light,
    pub fill: Light,
    pub rim: Light,
    pub sky: [f32; 3],
    pub horizon: [f32; 3],
    pub ground: [f32; 3],
    /// 1.0; tests set 0.
    pub direct_scale: f32,
    /// 1.0
    pub env_scale: f32,
    /// The rasterizer weights the rim by `(1 - n·v)²` so it shows only
    /// where the surface turns away from the camera: an art light, and the
    /// studio look depends on it. The product rig's rim is a real strip
    /// light and gets no such factor, so both backends treat it alike.
    pub rim_art: bool,
    /// The sun: a directional light with the sun's own angular
    /// radius, so the path tracer's shadows have a real penumbra and the
    /// rasterizer's are sharp. `None` for every rig unless a render asks
    /// for one ([`RenderOptions::sun`]).
    pub sun: Option<Light>,
}

/// The sun's angular radius as seen from Earth: a disc of 0.53° across.
pub const SUN_RADIUS_DEGREES: f32 = 0.265;

/// How dark the rasterizer draws the sun's shadow on its backdrop: the
/// backdrop is a flat colour rather than a lit surface, so the shadow is a
/// fixed darkening, chosen so a shadow study reads at a glance without
/// going black. The path tracer's floor is a real surface and needs none.
pub const SUN_GROUND_SHADOW: f32 = 0.55;

/// How much of the rig stays on under a sun. A sun beside the full studio
/// rig would be one more light of three; at this the sun is the light
/// that models the form and the rig is the fill that keeps the shadows
/// readable.
const RIG_UNDER_SUN: f32 = 0.45;

impl Lights {
    /// The default studio rig: a warm key from the upper left front, a cool
    /// fill from the opposite side, a rim from behind and above, and a
    /// sky/horizon/ground gradient for the ambient.
    pub fn studio() -> Lights {
        let r = STUDIO_LIGHT_DEGREES;
        Lights {
            key: Light { dir: normalize([-0.4, -0.6, 0.7]), radiance: [5.00, 4.80, 4.40], radius_degrees: r },
            fill: Light { dir: normalize([0.6, 0.3, 0.2]), radiance: [0.55, 0.62, 0.78], radius_degrees: r },
            rim: Light { dir: normalize([0.3, 0.8, 0.4]), radiance: [1.30, 1.30, 1.30], radius_degrees: r },
            sky: [0.30, 0.36, 0.48],
            horizon: [0.19, 0.19, 0.21],
            ground: [0.22, 0.21, 0.20],
            direct_scale: 1.0,
            env_scale: 1.0,
            rim_art: true,
            sun: None,
        }
    }

    /// The product-shot rig, locked to the camera: `azimuth` is the camera's, in radians
    /// (`View::azimuth`), and the three lights turn with it while their
    /// elevations stay world-relative so the key is always above the part.
    ///
    /// * key: 40° to camera-left, 40° up, 22° radius, 3.2 — the classic
    ///   45/45, modelling form on the front and the side face;
    /// * fill: 55° to camera-right, 15° up, 30° radius, 0.8 — opens the
    ///   shadow side to about 2.5:1, and is larger than the key so it adds
    ///   no second highlight;
    /// * rim: 155° round (behind, camera-right), 35° up, 8° radius, 1.6 —
    ///   a strip light, a bright edge along the top-back that separates
    ///   the part from the backdrop;
    /// * a neutral surround, sky 0.60 / horizon 0.35 / ground 0.25 — a soft
    ///   overhead source plus a grey wall, bright enough that a glossy face
    ///   has something to reflect.
    ///
    /// All neutral: a white part renders white, which is what a colour-
    /// accurate shot wants. `Lights::studio` is untouched by this.
    pub fn product(azimuth: f32) -> Lights {
        let az = azimuth.to_degrees();
        let dir = |az_deg: f32, el_deg: f32| {
            let (a, e) = (az_deg.to_radians(), el_deg.to_radians());
            normalize([a.sin() * e.cos(), -a.cos() * e.cos(), e.sin()])
        };
        Lights {
            key: Light { dir: dir(az - 40.0, 40.0), radiance: [3.2; 3], radius_degrees: 22.0 },
            fill: Light { dir: dir(az + 55.0, 15.0), radiance: [0.8; 3], radius_degrees: 30.0 },
            rim: Light { dir: dir(az + 155.0, 35.0), radiance: [1.6; 3], radius_degrees: 8.0 },
            sky: [0.60; 3],
            horizon: [0.35; 3],
            ground: [0.25; 3],
            direct_scale: 1.0,
            env_scale: 1.0,
            rim_art: false,
            sun: None,
        }
    }

    /// The same rig under another surround.
    pub fn with_backdrop(mut self, b: &Backdrop) -> Lights {
        let (sky, horizon, ground) = b.env();
        self.sky = sky;
        self.horizon = horizon;
        self.ground = ground;
        self
    }

    /// The same rig with a sun added: the three rig lights are
    /// turned down to [`RIG_UNDER_SUN`] of themselves so the sun is what
    /// models the form and casts the shadow, and the rig is the fill. A sun
    /// at or below the horizon adds nothing and leaves the rig alone.
    pub fn with_sun(mut self, sun: &super::SunLight) -> Lights {
        if sun.dir[2] <= 0.0 {
            return self;
        }
        for l in [&mut self.key, &mut self.fill, &mut self.rim] {
            l.radiance = scale(l.radiance, RIG_UNDER_SUN);
        }
        self.sun = Some(Light { dir: normalize(sun.dir), radiance: sun.irradiance, radius_degrees: SUN_RADIUS_DEGREES });
        self
    }

    /// The rig a render asked for: `opts.lighting` (the product rig turned
    /// to `azimuth`), under `opts.backdrop` when one was given, with
    /// `opts.sun` when there is one. One function for both backends, so
    /// they cannot disagree on the rig.
    pub fn for_options(opts: &RenderOptions, azimuth: f32) -> Lights {
        let rig = match opts.lighting {
            Lighting::Studio => Lights::studio(),
            Lighting::Product => Lights::product(azimuth),
        };
        let rig = match &opts.backdrop {
            Some(b) => rig.with_backdrop(b),
            None => rig,
        };
        match &opts.sun {
            Some(s) => rig.with_sun(s),
            None => rig,
        }
    }

    /// Uniform environment of 1, no direct light (white-furnace tests).
    pub fn furnace() -> Lights {
        let r = STUDIO_LIGHT_DEGREES;
        Lights {
            key: Light { dir: [0.0, 0.0, 1.0], radiance: [0.0; 3], radius_degrees: r },
            fill: Light { dir: [0.0, 0.0, 1.0], radiance: [0.0; 3], radius_degrees: r },
            rim: Light { dir: [0.0, 0.0, 1.0], radiance: [0.0; 3], radius_degrees: r },
            sky: [1.0; 3],
            horizon: [1.0; 3],
            ground: [1.0; 3],
            direct_scale: 0.0,
            env_scale: 1.0,
            rim_art: true,
            sun: None,
        }
    }
}

pub struct Surface {
    pub pos: [f32; 3],
    pub n: [f32; 3],
    /// Unit vector toward the eye.
    pub v: [f32; 3],
    pub albedo: [f32; 3],
    pub roughness: f32,
    pub metallic: f32,
    pub f0: [f32; 3],
    pub emissive: [f32; 3],
    pub ao: f32,
    /// How much of the sun this point sees, 0..1: 1 where nothing stands
    /// between it and the sun (and wherever there is no sun), 0 in shadow.
    /// The rasterizer fills it from a shadow ray through the scene's BVH.
    pub sun_vis: f32,
}

/// `((ior - 1) / (ior + 1))^2`
pub fn dielectric_f0(ior: f32) -> f32 {
    let r = (ior - 1.0) / (ior + 1.0);
    r * r
}

pub fn fresnel_schlick(f0: [f32; 3], cos_theta: f32) -> [f32; 3] {
    let t = (1.0 - cos_theta.clamp(0.0, 1.0)).powi(5);
    [f0[0] + (1.0 - f0[0]) * t, f0[1] + (1.0 - f0[1]) * t, f0[2] + (1.0 - f0[2]) * t]
}

/// GGX / Trowbridge-Reitz normal distribution; `a2 = alpha^2`.
#[inline]
fn ggx_d(n_h: f32, a2: f32) -> f32 {
    let d = n_h * n_h * (a2 - 1.0) + 1.0;
    a2 / (std::f32::consts::PI * d * d).max(1e-12)
}

/// Height-correlated Smith visibility (the `G / (4 NdotL NdotV)` factor).
#[inline]
fn smith_vis(n_v: f32, n_l: f32, a2: f32) -> f32 {
    let lv = n_l * (n_v * n_v * (1.0 - a2) + a2).sqrt();
    let ll = n_v * (n_l * n_l * (1.0 - a2) + a2).sqrt();
    0.5 / (lv + ll).max(1e-6)
}

/// Karis/Lazarov analytic split-sum approximation of the environment BRDF:
/// the specular environment is `f0 * scale + bias`.
#[inline]
fn env_brdf_approx(roughness: f32, n_v: f32) -> (f32, f32) {
    const C0: [f32; 4] = [-1.0, -0.0275, -0.572, 0.022];
    const C1: [f32; 4] = [1.0, 0.0425, 1.04, -0.04];
    let r = [
        roughness * C0[0] + C1[0],
        roughness * C0[1] + C1[1],
        roughness * C0[2] + C1[2],
        roughness * C0[3] + C1[3],
    ];
    let a004 = (r[0] * r[0]).min((-9.28 * n_v).exp2()) * r[0] + r[1];
    (a004 * -1.04 + r[2], a004 * 1.04 + r[3])
}

/// 3-stop vertical gradient environment: ground below, horizon at the
/// equator, sky at the zenith, blended with a smoothstep on `dir.z`.
pub fn env(l: &Lights, dir: [f32; 3]) -> [f32; 3] {
    let z = dir[2].clamp(-1.0, 1.0);
    let c = if z >= 0.0 {
        lerp3(l.horizon, l.sky, smoothstep(0.0, 1.0, z))
    } else {
        lerp3(l.horizon, l.ground, smoothstep(0.0, 1.0, -z))
    };
    scale(c, l.env_scale)
}

/// Diffuse irradiance of the same gradient: the cosine lobe over a hemisphere
/// sees a much wider slice of the environment than a mirror does, so the
/// horizon is widened to `smoothstep(-0.6, 0.6, n.z)`.
pub fn env_diffuse(l: &Lights, n: [f32; 3]) -> [f32; 3] {
    let t = smoothstep(-0.6, 0.6, n[2]);
    let c = if t >= 0.5 { lerp3(l.horizon, l.sky, (t - 0.5) * 2.0) } else { lerp3(l.ground, l.horizon, t * 2.0) };
    scale(c, l.env_scale)
}

/// Mirror direction of `v` about `n` (both unit, `v` pointing away from the
/// surface).
#[inline]
fn reflect(n: [f32; 3], v: [f32; 3]) -> [f32; 3] {
    sub(scale(n, 2.0 * dot(n, v)), v)
}

/// Cook-Torrance GGX under the rig plus the split-sum environment; linear
/// radiance, >= 0, unbounded. Used by [`shade`] and by `oit`.
pub fn shade_surface(s: &Surface, l: &Lights) -> [f32; 3] {
    let n = s.n;
    let v = s.v;
    let n_v = dot(n, v).max(1e-4);
    let rough = s.roughness.clamp(0.03, 1.0);
    let alpha = rough * rough;
    let metallic = s.metallic.clamp(0.0, 1.0);
    // Lambert, already divided by pi and by the metal's missing diffuse lobe.
    let diffuse_base = scale(s.albedo, (1.0 - metallic) * std::f32::consts::FRAC_1_PI);

    let mut out = [0.0f32; 3];

    // --- direct: key, fill, rim -------------------------------------------
    if l.direct_scale != 0.0 {
        let a = alpha.max(MIN_DIRECT_ALPHA);
        let a2 = a * a;
        // The studio rim is an art light: it only shows where the surface
        // turns away from the camera. The product rim is a real one.
        let rim_w = if l.rim_art { (1.0 - n_v) * (1.0 - n_v) } else { 1.0 };
        // The sun is a fourth direct light, weighted by how much of it the
        // point sees.
        let sun_vis = s.sun_vis.clamp(0.0, 1.0);
        let lights: [(&Light, f32); 4] = [
            (&l.key, 1.0),
            (&l.fill, 1.0),
            (&l.rim, rim_w),
            (l.sun.as_ref().unwrap_or(&l.key), if l.sun.is_some() { sun_vis } else { 0.0 }),
        ];
        for (light, extra) in lights {
            let n_l = dot(n, light.dir);
            if n_l <= 0.0 || extra <= 0.0 {
                continue;
            }
            let h = normalize(add(light.dir, v));
            let n_h = dot(n, h).clamp(0.0, 1.0);
            let v_h = dot(v, h).clamp(0.0, 1.0);
            let f = fresnel_schlick(s.f0, v_h);
            let spec = ggx_d(n_h, a2) * smith_vis(n_v, n_l, a2);
            let mut c = [0.0f32; 3];
            for i in 0..3 {
                c[i] = diffuse_base[i] * (1.0 - f[i]) + f[i] * spec;
            }
            out = add(out, mul(c, scale(light.radiance, n_l * extra * l.direct_scale)));
        }
    }

    // --- ambient: gradient environment, split-sum -------------------------
    if l.env_scale != 0.0 {
        let (e_scale, e_bias) = env_brdf_approx(rough, n_v);
        let spec_env = env_mix(l, n, v, alpha);
        let irradiance = env_diffuse(l, n);
        // Fdez-Aguera's single term for the light the single-scattering
        // split-sum loses between microfacets: without it a rough metal goes
        // implausibly dark (a white one in a white furnace would return 0.6
        // instead of 1.0) and rough dielectrics lose their sheen.
        let e_ms = (1.0 - (e_scale + e_bias)).max(0.0);
        let mut amb = [0.0f32; 3];
        for i in 0..3 {
            let f_ss = s.f0[i] * e_scale + e_bias;
            let f_avg = s.f0[i] + (1.0 - s.f0[i]) / 21.0;
            let f_ms = e_ms * f_ss * f_avg / (1.0 - f_avg * e_ms).max(1e-4);
            // Whatever neither scattering path reflected is available to the
            // diffuse lobe, so the ambient conserves energy.
            let k_d = (1.0 - f_ss - f_ms).max(0.0) * (1.0 - metallic) * s.albedo[i];
            amb[i] = spec_env[i] * f_ss + irradiance[i] * (f_ms + k_d);
        }
        let ao = s.ao.clamp(0.0, 1.0);
        out = add(out, scale(amb, ao * ao.sqrt()));
    }

    add(out, s.emissive).map(|c| if c.is_finite() { c.max(0.0) } else { 0.0 })
}

/// Specular environment lookup: the mirror direction for a smooth surface,
/// blurred toward the normal's direction as roughness grows.
#[inline]
fn env_mix(l: &Lights, n: [f32; 3], v: [f32; 3], alpha: f32) -> [f32; 3] {
    let r = reflect(n, v);
    lerp3(env(l, r), env(l, n), alpha.clamp(0.0, 1.0))
}

/// Builds a Surface for a fragment: converts Material to f32, applies
/// `textures::sample` (albedo multiplier, roughness delta) and the highlight
/// rules.
pub(crate) fn surface_for(
    ctx: &Ctx<'_>,
    item: u16,
    face: u32,
    tri: u32,
    pos: [f32; 3],
    n: [f32; 3],
    ao: f32,
) -> Surface {
    let it = &ctx.items[item as usize];
    let m = it.material;
    // An analysis view replaces the material's colour with the colour of the
    // measured value under this triangle, and turns the material's character
    // off so the reading is the colour and nothing else.
    let painted = if it.paint { it.tri_colors.get(tri as usize) } else { None };
    if let Some(c) = it.tri_colors.get(tri as usize).filter(|_| !it.paint) {
        let albedo = c.map(|v| v.clamp(0.0, 1.0));
        let albedo = if !it.highlight_faces.is_empty() && it.highlight_faces.contains(&face) {
            lerp3(albedo, HIGHLIGHT, 0.75)
        } else {
            albedo
        };
        return Surface {
            pos,
            n,
            v: ctx.view.to_eye(pos),
            albedo,
            roughness: 0.85,
            metallic: 0.0,
            f0: [dielectric_f0(1.5); 3],
            emissive: [0.0; 3],
            ao,
            sun_vis: 1.0,
        };
    }
    let kind = textures::kind(&m.texture).unwrap_or(textures::Kind::None);
    let mut albedo = match painted {
        Some(c) => c.map(|v| v.clamp(0.0, 1.0)),
        None => m.base_color.map(|c| (c as f32).clamp(0.0, 1.0)),
    };
    // A scan's own colour, per vertex, under the material's (or the paint's).
    if let Some(v) = super::vertex_albedo(it.mesh, tri, pos) {
        albedo = mul(albedo, v);
    }
    let mut roughness = m.roughness as f32;
    let mut n = n;
    if kind.is_relief() {
        // A machined finish tilts the normal (the grooves) and, once they
        // are below a pixel, roughens the surface across them instead. The
        // rasterizer has no anisotropic lobe, so the roughness goes in
        // isotropically; the tracer keeps the direction.
        let depth = dot(sub(pos, ctx.view.eye), ctx.view.fwd);
        let frame = it.face_frames.get(face as usize).copied().unwrap_or(super::FaceFrame::fallback(ctx.scene.center));
        let r = textures::machined(
            kind,
            m.texture_scale as f32,
            m.texture_depth as f32,
            pos,
            n,
            ctx.view.footprint(depth),
            frame.axis_point,
            frame.axis,
            frame.origin,
            frame.dir,
            frame.turned,
        );
        n = normalize(add(n, r.bump));
        roughness = (roughness * roughness + 2.0 * r.slope_var).sqrt();
    } else if kind != textures::Kind::None {
        let depth = dot(sub(pos, ctx.view.eye), ctx.view.fwd);
        let texel = textures::sample(kind, m.texture_scale as f32, pos, n, ctx.view.footprint(depth));
        albedo = mul(albedo, texel.albedo);
        roughness += texel.roughness_delta;
    }
    let mut roughness = roughness.clamp(0.03, 1.0);
    let mut metallic = (m.metallic as f32).clamp(0.0, 1.0);
    let mut emissive = [0.0f32; 3];
    // The rasterizer shades every material as a 1.5-ior dielectric with no
    // emission of its own; the path tracer reads `ior` and `emissive`.
    let ior = 1.5f32;
    if !it.highlight_faces.is_empty() && it.highlight_faces.contains(&face) {
        albedo = lerp3(albedo, HIGHLIGHT, 0.75);
        roughness = 0.5;
        metallic = 0.0;
        emissive = add(emissive, scale(HIGHLIGHT, 0.15));
    }
    let f0 = lerp3([dielectric_f0(ior); 3], albedo, metallic);
    Surface { pos, n, v: ctx.view.to_eye(pos), albedo, roughness, metallic, f0, emissive, ao, sun_vis: 1.0 }
}

/// Opaque pixels: `hdr = shade_surface(surface_for(..))`. Background pixels:
/// `hdr = inverse_tonemap_rgb(bg * ground, exposure)` so anything composited
/// over them later lands exactly on the background after the tonemap.
pub(crate) fn shade(frame: &mut super::Frame, ctx: &Ctx<'_>) {
    if let Some(view) = ctx.opts.debug {
        return shade_debug(frame, ctx, view);
    }
    let lights = Lights::for_options(ctx.opts, ctx.view.azimuth());
    let bg = ctx.opts.background;
    let exposure = ctx.opts.exposure;
    let tm = ctx.opts.tonemap;
    let w = frame.w as usize;
    let bg_hdr = |g: f32| inverse_tonemap_rgb([bg[0] * g, bg[1] * g, bg[2] * g], exposure, tm);
    // Unshadowed background is by far the common case; compute it once.
    let bg_plain = bg_hdr(1.0);
    // The sun's shadow: one shadow ray per sample through the
    // opaque geometry's BVH, from the surface point for a body and from
    // where the view ray meets the ground plane for the backdrop. Sharp,
    // as the rasterizer's shadows are; the anti-aliasing pattern softens
    // the edge by a pixel.
    let sun = match (&ctx.sun_shadow, &lights.sun) {
        (Some(sh), Some(l)) => Some((sh, l.dir)),
        _ => None,
    };
    let sc = ctx.scene;
    let view = ctx.view;
    let eps = 1e-4 * sc.diag;
    let far = 4.0 * sc.diag;
    let (cov, item, face, tri, pos, normal, ao, ground) = (
        &frame.coverage,
        &frame.item,
        &frame.face,
        &frame.tri,
        &frame.pos,
        &frame.normal,
        &frame.ao,
        &frame.ground,
    );
    par_bands(&mut frame.hdr, w, 32, ctx.threads, |y0, band| {
        let base = y0 * w;
        for (i, h) in band.iter_mut().enumerate() {
            let idx = base + i;
            if cov[idx] & COV_OPAQUE != 0 {
                let mut s = surface_for(ctx, item[idx], face[idx], tri[idx], pos[idx], normal[idx], ao[idx]);
                if let Some((sh, dir)) = sun {
                    let o = add(pos[idx], scale(normal[idx], eps));
                    s.sun_vis = if sh.bvh.occluded(o, dir, 0.0, far) { 0.0 } else { 1.0 };
                }
                *h = shade_surface(&s, &lights);
            } else {
                let mut g = ground[idx];
                if let Some((sh, dir)) = sun {
                    let x = (idx % w) as f32 + 0.5;
                    let y = (idx / w) as f32 + 0.5;
                    let (o, d) = view.ray(x, y);
                    if d[2] < -1e-6 {
                        let t = (sc.ground_z - o[2]) / d[2];
                        if t > 0.0 {
                            let hit = add(o, scale(d, t));
                            let hit = [hit[0], hit[1], sc.ground_z + eps];
                            if sh.bvh.occluded(hit, dir, 0.0, far) {
                                g *= 1.0 - SUN_GROUND_SHADOW;
                            }
                        }
                    }
                }
                *h = if g == 1.0 { bg_plain } else { bg_hdr(g) };
            }
        }
    });
}

/// How much of a debug view's colour survives on a face seen edge-on. High,
/// because the reading is the hue and the headlight is only there to keep
/// the form legible; a grazing face at a third of its colour would be taken
/// for a different palette entry.
const HEADLIGHT_AMBIENT: f32 = 0.55;

/// The debug views' lighting (`RenderOptions::debug`): the triangle's colour
/// — or the material's, where nothing painted it — under a single
/// directional light along the view axis, with no environment, no specular
/// and no occlusion. Directional rather than a point at the eye on purpose:
/// `dot(n, -fwd)` is constant across a flat face under any projection, so
/// a face is *one* colour and `faces` mode counts as many colours as it has
/// faces. `normals` skips the light altogether and writes the shading
/// normal as `n * 0.5 + 0.5`, which is the reading itself.
fn shade_debug(frame: &mut super::Frame, ctx: &Ctx<'_>, view: super::DebugView) {
    let bg = ctx.opts.background;
    let w = frame.w as usize;
    let toward_eye = scale(ctx.view.fwd, -1.0);
    let (cov, item, face, tri, normal, ground) =
        (&frame.coverage, &frame.item, &frame.face, &frame.tri, &frame.normal, &frame.ground);
    par_bands(&mut frame.hdr, w, 32, ctx.threads, |y0, band| {
        let base = y0 * w;
        for (i, h) in band.iter_mut().enumerate() {
            let idx = base + i;
            if cov[idx] & COV_OPAQUE == 0 {
                let g = ground[idx];
                *h = [bg[0] * g, bg[1] * g, bg[2] * g];
                continue;
            }
            let n = normal[idx];
            if view == super::DebugView::Normals {
                *h = [n[0] * 0.5 + 0.5, n[1] * 0.5 + 0.5, n[2] * 0.5 + 0.5];
                continue;
            }
            let it = &ctx.items[item[idx] as usize];
            let mut albedo = match it.tri_colors.get(tri[idx] as usize) {
                Some(c) => c.map(|v| v.clamp(0.0, 1.0)),
                None => it.material.base_color.map(|c| (c as f32).clamp(0.0, 1.0)),
            };
            if !it.highlight_faces.is_empty() && it.highlight_faces.contains(&face[idx]) {
                albedo = lerp3(albedo, HIGHLIGHT, 0.75);
            }
            let light = HEADLIGHT_AMBIENT + (1.0 - HEADLIGHT_AMBIENT) * dot(n, toward_eye).max(0.0);
            *h = scale(albedo, light);
        }
    });
}

#[cfg(test)]
mod tests {
    use super::super::testing::*;
    use super::super::{Camera, Item, RenderOptions, luminance, normalize, tonemap_rgb};
    use super::*;
    use crate::context::Material;

    fn surf(n: [f32; 3], v: [f32; 3], albedo: [f32; 3], roughness: f32, metallic: f32) -> Surface {
        let f0 = lerp3([dielectric_f0(1.5); 3], albedo, metallic);
        Surface {
            pos: [0.0; 3],
            n: normalize(n),
            v: normalize(v),
            albedo,
            roughness,
            metallic,
            f0,
            emissive: [0.0; 3],
            ao: 1.0,
            sun_vis: 1.0,
        }
    }

    /// A white dielectric or metal in a uniform environment of 1 must return
    /// roughly what it receives: no energy created, little lost.
    #[test]
    fn white_furnace_is_energy_sane() {
        let l = Lights::furnace();
        for &r in &[0.05f32, 0.3, 0.6, 1.0] {
            for &m in &[0.0f32, 1.0] {
                for &n in &[[0.0f32, 0.0, 1.0], [1.0, 0.0, 0.0], [0.3, -0.9, 0.2]] {
                    for &v in &[[0.0f32, 0.0, 1.0], [0.6, -0.6, 0.5]] {
                        let s = surf(n, v, [1.0; 3], r, m);
                        let c = shade_surface(&s, &l);
                        for ch in c {
                            assert!((0.85..=1.05).contains(&ch), "furnace r={r} m={m} n={n:?} -> {c:?}");
                        }
                    }
                }
            }
        }
    }

    /// Smoother surfaces concentrate the specular lobe: with the diffuse and
    /// ambient terms switched off, the peak falls and the lobe covers more
    /// directions as roughness rises (the same energy, spread wider).
    #[test]
    fn roughness_spreads_the_specular_lobe() {
        let mut l = Lights::studio();
        l.env_scale = 0.0;
        l.fill.radiance = [0.0; 3];
        l.rim.radiance = [0.0; 3];
        let v = normalize([0.6, -0.8, 0.4]);
        let mut peak_prev = f32::INFINITY;
        let mut wide_prev = 0usize;
        for &r in &[0.1f32, 0.4, 0.9] {
            let mut vals: Vec<f32> = Vec::new();
            for i in 0..128 {
                for j in 0..64 {
                    let theta = std::f32::consts::PI * (i as f32 + 0.5) / 128.0;
                    let phi = 2.0 * std::f32::consts::PI * (j as f32 + 0.5) / 64.0;
                    let n = [theta.sin() * phi.cos(), theta.sin() * phi.sin(), theta.cos()];
                    if dot(n, v) <= 0.0 {
                        continue;
                    }
                    // Black albedo: what is left is exactly the specular lobe.
                    vals.push(luminance(shade_surface(&surf(n, v, [0.0; 3], r, 0.0), &l)));
                }
            }
            let peak = vals.iter().copied().fold(0.0f32, f32::max);
            let wide = vals.iter().filter(|&&x| x > 0.02 * peak_prev.min(peak)).count();
            assert!(peak < peak_prev, "peak should fall with roughness: r={r} {peak} !< {peak_prev}");
            assert!(wide > wide_prev, "lobe should widen: r={r} covers {wide} directions, was {wide_prev}");
            peak_prev = peak;
            wide_prev = wide;
        }
    }

    /// A red metal tints its specular red; a red dielectric keeps a white
    /// highlight.
    #[test]
    fn metal_tints_its_specular_dielectric_does_not() {
        let l = Lights::studio();
        let red = [0.8f32, 0.05, 0.05];
        let v = normalize([0.0, -1.0, 0.0]);
        // Half vector of the key light and this view, so the lobe is hit.
        let n = normalize(add(l.key.dir, v));
        let metal = shade_surface(&surf(n, v, red, 0.15, 1.0), &l);
        let plastic = shade_surface(&surf(n, v, red, 0.15, 0.0), &l);
        assert!(metal[0] / metal[2].max(1e-6) > 4.0, "metal highlight should be red: {metal:?}");
        let p_ratio = plastic[0] / plastic[2].max(1e-6);
        assert!(p_ratio < 2.0, "dielectric highlight should stay white: {plastic:?} ratio {p_ratio}");
        // And the metal has no diffuse: away from the lobe it is much darker.
        let away = normalize([-l.key.dir[0], -l.key.dir[1], 0.2]);
        let m_dark = luminance(shade_surface(&surf(away, v, red, 0.15, 1.0), &l));
        let p_dark = luminance(shade_surface(&surf(away, v, red, 0.15, 0.0), &l));
        assert!(m_dark < p_dark, "metal should be darker than plastic off-lobe: {m_dark} vs {p_dark}");
    }

    #[test]
    fn emissive_passes_through_unlit() {
        let mut l = Lights::furnace();
        l.env_scale = 0.0;
        let mut s = surf([0.0, 0.0, 1.0], [0.0, 0.0, 1.0], [0.0; 3], 0.5, 0.0);
        s.emissive = [1.0, 0.0, 0.0];
        assert_eq!(shade_surface(&s, &l), [1.0, 0.0, 0.0]);
    }

    #[test]
    fn ambient_occlusion_only_touches_the_ambient() {
        let l = Lights::studio();
        let lit = surf(l.key.dir, l.key.dir, [0.7; 3], 0.5, 0.0);
        let mut dark = surf(l.key.dir, l.key.dir, [0.7; 3], 0.5, 0.0);
        dark.ao = 0.0;
        let a = luminance(shade_surface(&lit, &l));
        let b = luminance(shade_surface(&dark, &l));
        assert!(b < a, "ao must darken");
        // Direct light survives full occlusion.
        assert!(b > 0.25 * a, "ao must not kill the direct lighting: {b} vs {a}");
    }

    #[test]
    fn never_nan_or_negative() {
        let l = Lights::studio();
        for i in 0..2000u32 {
            let h = |k: u32| (textures::hash3(i as i32, k as i32, 3, 9) % 20_000) as f32 / 10_000.0 - 1.0;
            let n = [h(0), h(1), h(2)];
            if dot(n, n) < 1e-6 {
                continue;
            }
            let v = [h(3), h(4), h(5).abs() + 1e-3];
            let r = (h(6).abs()).clamp(0.0, 1.0);
            let m = if h(7) > 0.0 { 1.0 } else { 0.0 };
            let c = shade_surface(&surf(n, v, [h(8).abs(), h(9).abs(), 0.5], r, m), &l);
            for ch in c {
                assert!(ch.is_finite() && ch >= 0.0, "bad radiance {c:?} for n={n:?} v={v:?} r={r} m={m}");
            }
        }
    }

    /// A brighter key light makes a sphere brighter, monotonically, over its
    /// whole visible hemisphere.
    #[test]
    fn sphere_mean_radiance_rises_with_the_key_light() {
        let v = normalize([0.6, -0.8, 0.4]);
        let mut prev = -1.0f32;
        for &k in &[0.0f32, 0.5, 1.0, 2.0, 4.0] {
            let mut l = Lights::studio();
            l.key.radiance = [k, k, k];
            let mut sum = 0.0f64;
            let mut n_px = 0u32;
            for i in 0..48 {
                for j in 0..96 {
                    let theta = std::f32::consts::PI * (i as f32 + 0.5) / 48.0;
                    let phi = 2.0 * std::f32::consts::PI * (j as f32 + 0.5) / 96.0;
                    let n = [theta.sin() * phi.cos(), theta.sin() * phi.sin(), theta.cos()];
                    if dot(n, v) <= 0.0 {
                        continue;
                    }
                    sum += luminance(shade_surface(&surf(n, v, [0.75; 3], 0.6, 0.0), &l)) as f64;
                    n_px += 1;
                }
            }
            let mean = (sum / n_px as f64) as f32;
            assert!(mean > prev * 1.02, "key {k} mean {mean} should exceed {prev}");
            prev = mean;
        }
    }

    /// The same at image level, through the whole pipeline.
    #[test]
    fn sphere_brightens_with_exposure() {
        let (mesh, _) = uv_sphere(32, 48);
        let cam = Camera::preset("iso", crate::geom::Vec3::new(-1.0, -1.0, -1.0), crate::geom::Vec3::new(1.0, 1.0, 1.0), 0.1);
        let mat = Material::default();
        let items = [Item::new(&mesh, &mat, &[])];
        let mut prev = -1.0f32;
        for &e in &[0.35f32, 0.7, 1.0, 1.6] {
            let opts = RenderOptions {
                width: 120,
                height: 120,
                exposure: e,
                draw_grid: false,
                draw_edges: false,
                samples: 1,
                ..Default::default()
            };
            let img = super::super::render(&items, &cam, &opts);
            let m = mean_luma(&img, (30, 30, 90, 90));
            assert!(m > prev + 1.0, "exposure {e} should brighten: {m} vs {prev}");
            prev = m;
        }
    }

    /// The terminator is smooth and monotonic: walking a sphere's normals
    /// from the key direction toward the anti-key direction, radiance never
    /// increases (no facets, no banding steps).
    #[test]
    fn terminator_is_monotonic_along_the_lit_direction() {
        let mut l = Lights::studio();
        l.fill.radiance = [0.0; 3];
        l.rim.radiance = [0.0; 3];
        l.env_scale = 0.0;
        let key = l.key.dir;
        let up = normalize(super::super::cross(key, [0.0, 0.0, 1.0]));
        let mut prev = f32::INFINITY;
        for i in 0..=180 {
            let a = std::f32::consts::PI * i as f32 / 180.0;
            let n = normalize(add(scale(key, a.cos()), scale(up, a.sin())));
            let c = luminance(shade_surface(&surf(n, key, [0.75; 3], 0.6, 0.0), &l));
            assert!(c <= prev + 1e-6, "radiance rose at {i} degrees: {c} > {prev}");
            prev = c;
        }
        assert!(prev <= 1e-6, "back side should be unlit, got {prev}");
    }

    /// The rasterizer's studio look is pinned the same way as the tracer's:
    /// the product-shot rig must leave it alone to the bit.
    #[test]
    fn the_raster_studio_rig_is_bit_identical_to_before_the_product_shot() {
        let (mesh, edges) = uv_sphere(16, 32);
        let block = super::super::testing::box_mesh([-30.0, -30.0, -10.0], [-12.0, -12.0, 6.0]);
        let mat = Material { roughness: 0.3, metallic: 1.0, ..Material::default() };
        let plastic = Material { base_color: [0.7, 0.3, 0.2], ..Material::default() };
        let items = [Item::new(&mesh, &mat, &edges), Item::new(&block, &plastic, &[])];
        let cam = Camera::preset("iso", crate::geom::Vec3::new(-30.0, -30.0, -10.0), crate::geom::Vec3::new(10.0, 10.0, 10.0), 0.2);
        let opts = RenderOptions { width: 96, height: 72, samples: 4, ..Default::default() };
        let img = super::super::render(&items, &cam, &opts);
        assert_eq!(super::super::testing::fnv1a64(&img.rgba), 3_791_816_006_272_706_796, "the raster studio look moved");
    }

    /// The default material in the default rig lands in a readable range:
    /// a lit top face near 0.8 display luminance, a shadowed side well below
    /// it, nothing clipped to white.
    #[test]
    fn default_material_tonemaps_into_a_readable_range() {
        let l = Lights::studio();
        let v = normalize([0.6, -0.6, 0.5]);
        let albedo = Material::default().base_color.map(|c| c as f32);
        let top = tonemap_rgb(shade_surface(&surf([0.0, 0.0, 1.0], v, albedo, 0.6, 0.0), &l), 1.0, super::super::ToneMap::Aces);
        let under = tonemap_rgb(shade_surface(&surf([0.0, 0.0, -1.0], v, albedo, 0.6, 0.0), &l), 1.0, super::super::ToneMap::Aces);
        let lt = luminance(top);
        let lu = luminance(under);
        assert!((0.62..=0.92).contains(&lt), "top face display luminance {lt}");
        assert!((0.05..=0.40).contains(&lu), "underside display luminance {lu}");
        assert!(top.iter().all(|&c| c < 0.999), "top face must not clip: {top:?}");
    }

    /// Key and fill differ in colour temperature, so opposite sides of the
    /// same white object are warm and cool.
    #[test]
    fn key_is_warm_and_fill_is_cool() {
        let l = Lights::studio();
        let v = normalize([0.6, -0.6, 0.5]);
        let warm = shade_surface(&surf(l.key.dir, v, [0.75; 3], 0.6, 0.0), &l);
        let cool = shade_surface(&surf(l.fill.dir, v, [0.75; 3], 0.6, 0.0), &l);
        assert!(warm[0] / warm[2] > cool[0] / cool[2], "key {warm:?} should be warmer than fill {cool:?}");
    }


    /// The sun: a unit box under a sun from +x at 45° throws a
    /// shadow toward −x on the backdrop, one box-height long. The backdrop
    /// there is darkened by the fixed factor, the backdrop on the sunny
    /// side is exactly what it was, the box's top face sees the sun and
    /// shades brighter than under the rig alone, and a sun below the
    /// horizon changes nothing at all.
    #[test]
    fn the_sun_casts_a_sharp_shadow_on_the_backdrop_and_lights_the_top() {
        use super::super::testing::{render_frame, unit_box};
        use super::super::{COV_OPAQUE, Camera, Frame, Item, RenderOptions, SunLight, View, luminance};
        use crate::context::Material;
        use crate::geom::Vec3;
        let (mesh, edges) = unit_box();
        let mat = Material::default();
        let items = [Item::new(&mesh, &mat, &edges)];
        let (lo, hi) = (Vec3::new(-2.0, -1.0, 0.0), Vec3::new(3.0, 2.0, 1.0));
        let mut cam = Camera::preset("top", lo, hi, 0.1);
        cam.fit(lo, hi, 0.1, 4.0 / 3.0);
        let (w, h) = (160u32, 120u32);
        let base = RenderOptions {
            width: w,
            height: h,
            draw_grid: false,
            draw_edges: false,
            ambient_occlusion: false,
            contact_shadow: false,
            samples: 1,
            threads: 1,
            ..Default::default()
        };
        let plain = render_frame(&items, &cam, &base);
        let sunny = RenderOptions { sun: Some(SunLight::new(Vec3::new(1.0, 0.0, 1.0))), ..base.clone() };
        let lit = render_frame(&items, &cam, &sunny);
        let view = View::new(&cam, w, h);
        let px = |p: [f32; 3]| {
            let s = view.project(p).expect("in front of the camera");
            s[0] as usize + s[1] as usize * w as usize
        };
        let shadow = px([-0.5, 0.5, 0.0]);
        let open = px([2.0, 0.5, 0.0]);
        let top = px([0.5, 0.5, 1.0]);
        let lum = |f: &Frame, i: usize| luminance(f.hdr[i]);
        assert!(lit.coverage[shadow] == 0 && lit.coverage[open] == 0, "both probes are backdrop");
        assert!(lit.coverage[top] & COV_OPAQUE != 0, "the top probe is the box");
        assert!(lum(&lit, shadow) < 0.7 * lum(&lit, open), "shadow {} vs open {}", lum(&lit, shadow), lum(&lit, open));
        assert_eq!(lit.hdr[open], plain.hdr[open], "the backdrop outside the shadow is what it was");
        assert_eq!(plain.hdr[shadow], plain.hdr[open], "without a sun the backdrop is flat");
        assert!(lum(&lit, top) > lum(&plain, top), "the sunlit top is brighter: {} vs {}", lum(&lit, top), lum(&plain, top));
        // The shadow ends one box-height from the box: at x = −1.2 the
        // backdrop is open again.
        let beyond = px([-1.3, 0.5, 0.0]);
        assert_eq!(lit.hdr[beyond], plain.hdr[beyond], "the shadow is a box-height long at 45°");
        // Below the horizon: nothing changes, to the bit.
        let night = RenderOptions { sun: Some(SunLight::new(Vec3::new(1.0, 0.0, -0.5))), ..base.clone() };
        let dark = render_frame(&items, &cam, &night);
        assert_eq!(dark.hdr, plain.hdr);
    }

    /// Guide geometry in the rasterizer: the same box marked
    /// `shadow: false` is drawn and sunlit as before, and the backdrop
    /// behind it under the sun is what it is with no box at all, to the
    /// bit — the box is not in the sun-shadow BVH — and the contact shadow
    /// leaves the backdrop beside it alone too, because the box is not in
    /// the heightfield either.
    #[test]
    fn a_guide_body_casts_no_shadow_in_the_rasterizer() {
        use super::super::testing::{render_frame, unit_box};
        use super::super::{COV_OPAQUE, Camera, Item, RenderOptions, SunLight, View, luminance};
        use crate::context::Material;
        use crate::geom::Vec3;
        let (mesh, edges) = unit_box();
        let solid = Material::default();
        let guide = Material { shadow: false, ..Material::default() };
        let (lo, hi) = (Vec3::new(-2.0, -1.0, 0.0), Vec3::new(3.0, 2.0, 1.0));
        let mut cam = Camera::preset("top", lo, hi, 0.1);
        cam.fit(lo, hi, 0.1, 4.0 / 3.0);
        let (w, h) = (160u32, 120u32);
        let sunny = RenderOptions {
            width: w,
            height: h,
            draw_grid: false,
            draw_edges: false,
            ambient_occlusion: false,
            contact_shadow: false,
            samples: 1,
            threads: 1,
            sun: Some(SunLight::new(Vec3::new(1.0, 0.0, 1.0))),
            ..Default::default()
        };
        let view = View::new(&cam, w, h);
        let px = |p: [f32; 3]| {
            let s = view.project(p).expect("in front of the camera");
            s[0] as usize + s[1] as usize * w as usize
        };
        let shadow = px([-0.5, 0.5, 0.0]);
        let top = px([0.5, 0.5, 1.0]);
        let cast = render_frame(&[Item::new(&mesh, &solid, &edges)], &cam, &sunny);
        let guided = render_frame(&[Item::new(&mesh, &guide, &edges)], &cam, &sunny);
        let empty = render_frame(&[], &cam, &sunny);
        assert!(luminance(cast.hdr[shadow]) < 0.7 * luminance(empty.hdr[shadow]), "the solid box shadows the backdrop");
        assert_eq!(guided.hdr[shadow], empty.hdr[shadow], "behind a guide the backdrop is what it is with no box");
        assert!(guided.coverage[top] & COV_OPAQUE != 0, "the guide is drawn");
        assert_eq!(guided.hdr[top], cast.hdr[top], "and shaded as the solid box is, sun included");

        // The contact shadow: the backdrop just beyond the box's foot (clear
        // of its silhouette from above) is darkened for a solid box and
        // untouched for a guide.
        let contact = RenderOptions { sun: None, contact_shadow: true, ..sunny.clone() };
        let foot = px([-0.3, 0.5, 0.0]);
        let cast = render_frame(&[Item::new(&mesh, &solid, &edges)], &cam, &contact);
        let guided = render_frame(&[Item::new(&mesh, &guide, &edges)], &cam, &contact);
        assert!(cast.coverage[foot] == 0 && guided.coverage[foot] == 0, "the foot probe is backdrop");
        assert!(cast.ground[foot] < 1.0, "a solid box has a contact shadow: {}", cast.ground[foot]);
        assert_eq!(guided.ground[foot], 1.0, "a guide has none");
    }
}
