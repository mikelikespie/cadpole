// Lights and the surface BRDF: a port of
// src/rust/cadpole-core/src/render/shading.rs. Cook-Torrance with a GGX normal
// distribution, height-correlated Smith visibility and Schlick Fresnel, lit by
// a three-light studio rig plus a gradient hemisphere environment, with
// Fdez-Aguera multiple scattering on the ambient term.
//
// Everything here is scene-linear radiance; the tonemap is the only place
// values are squeezed into 0..1.

// Punctual lights stand in for softboxes: clamping the GGX width from below
// keeps a mirror-smooth material from collapsing its highlight into one pixel.
const MIN_DIRECT_ALPHA: f32 = 0.06;

// Already normalised, to the f32 the CPU's `normalize` produces: a GPU's
// `normalize` may use a fast reciprocal square root, and the rig is a constant
// either way.
const KEY_DIR: vec3<f32> = vec3<f32>(-0.398014873, -0.597022295, 0.696525991);
const KEY_RADIANCE: vec3<f32> = vec3<f32>(5.00, 4.80, 4.40);
const FILL_DIR: vec3<f32> = vec3<f32>(0.857142806, 0.428571403, 0.285714269);
const FILL_RADIANCE: vec3<f32> = vec3<f32>(0.55, 0.62, 0.78);
const RIM_DIR: vec3<f32> = vec3<f32>(0.317999363, 0.847998321, 0.423999161);
const RIM_RADIANCE: vec3<f32> = vec3<f32>(1.30, 1.30, 1.30);
const ENV_SKY: vec3<f32> = vec3<f32>(0.30, 0.36, 0.48);
const ENV_HORIZON: vec3<f32> = vec3<f32>(0.19, 0.19, 0.21);
const ENV_GROUND: vec3<f32> = vec3<f32>(0.22, 0.21, 0.20);

struct Surface {
    pos: vec3<f32>,
    n: vec3<f32>,
    v: vec3<f32>,
    albedo: vec3<f32>,
    roughness: f32,
    metallic: f32,
    f0: vec3<f32>,
    emissive: vec3<f32>,
    ao: f32,
};

fn dielectric_f0(ior: f32) -> f32 {
    let r = (ior - 1.0) / (ior + 1.0);
    return r * r;
}

fn fresnel_schlick(f0: vec3<f32>, cos_theta: f32) -> vec3<f32> {
    let c = clamp(cos_theta, 0.0, 1.0);
    let x = 1.0 - c;
    let x2 = x * x;
    // `powi(5)` on the CPU side: plain multiplication, not exp2/log2.
    let t = x2 * x2 * x;
    return f0 + (vec3<f32>(1.0) - f0) * t;
}

fn ggx_d(n_h: f32, a2: f32) -> f32 {
    let d = n_h * n_h * (a2 - 1.0) + 1.0;
    return a2 / max(PI * d * d, 1e-12);
}

fn smith_vis(n_v: f32, n_l: f32, a2: f32) -> f32 {
    let lv = n_l * sqrt(n_v * n_v * (1.0 - a2) + a2);
    let ll = n_v * sqrt(n_l * n_l * (1.0 - a2) + a2);
    return 0.5 / max(lv + ll, 1e-6);
}

// Karis/Lazarov analytic split-sum approximation.
fn env_brdf_approx(roughness: f32, n_v: f32) -> vec2<f32> {
    let c0 = vec4<f32>(-1.0, -0.0275, -0.572, 0.022);
    let c1 = vec4<f32>(1.0, 0.0425, 1.04, -0.04);
    let r = roughness * c0 + c1;
    let a004 = min(r.x * r.x, exp2(-9.28 * n_v)) * r.x + r.y;
    return vec2<f32>(a004 * -1.04 + r.z, a004 * 1.04 + r.w);
}

// 3-stop vertical gradient environment.
fn env_radiance(dir: vec3<f32>) -> vec3<f32> {
    let z = clamp(dir.z, -1.0, 1.0);
    if (z >= 0.0) {
        return mix(ENV_HORIZON, ENV_SKY, sstep(0.0, 1.0, z));
    }
    return mix(ENV_HORIZON, ENV_GROUND, sstep(0.0, 1.0, -z));
}

// Diffuse irradiance of the same gradient, horizon widened for the cosine lobe.
fn env_diffuse(n: vec3<f32>) -> vec3<f32> {
    let t = sstep(-0.6, 0.6, n.z);
    if (t >= 0.5) {
        return mix(ENV_HORIZON, ENV_SKY, (t - 0.5) * 2.0);
    }
    return mix(ENV_GROUND, ENV_HORIZON, t * 2.0);
}

fn env_mix(n: vec3<f32>, v: vec3<f32>, alpha: f32) -> vec3<f32> {
    let r = n * (2.0 * dot(n, v)) - v;
    return mix(env_radiance(r), env_radiance(n), clamp(alpha, 0.0, 1.0));
}

fn shade_surface(s: Surface) -> vec3<f32> {
    let n = s.n;
    let v = s.v;
    let n_v = max(dot(n, v), 1e-4);
    let rough = clamp(s.roughness, 0.03, 1.0);
    let alpha = rough * rough;
    let metallic = clamp(s.metallic, 0.0, 1.0);
    let diffuse_base = s.albedo * ((1.0 - metallic) * (1.0 / PI));

    var out = vec3<f32>(0.0);

    // --- direct: key, fill, rim ------------------------------------------
    let a = max(alpha, MIN_DIRECT_ALPHA);
    let a2 = a * a;
    // The rim light is an art light: it shows only where the surface turns
    // away from the camera.
    let rim_w = (1.0 - n_v) * (1.0 - n_v);
    for (var i = 0u; i < 3u; i = i + 1u) {
        var ldir = KEY_DIR;
        var radiance = KEY_RADIANCE;
        var extra = 1.0;
        if (i == 1u) {
            ldir = FILL_DIR;
            radiance = FILL_RADIANCE;
        } else if (i == 2u) {
            ldir = RIM_DIR;
            radiance = RIM_RADIANCE;
            extra = rim_w;
        }
        let n_l = dot(n, ldir);
        if (n_l <= 0.0 || extra <= 0.0) {
            continue;
        }
        let h = normalize(ldir + v);
        let n_h = clamp(dot(n, h), 0.0, 1.0);
        let v_h = clamp(dot(v, h), 0.0, 1.0);
        let f = fresnel_schlick(s.f0, v_h);
        let spec = ggx_d(n_h, a2) * smith_vis(n_v, n_l, a2);
        let c = diffuse_base * (vec3<f32>(1.0) - f) + f * spec;
        out = out + c * (radiance * (n_l * extra));
    }

    // --- ambient: gradient environment, split-sum ------------------------
    let eb = env_brdf_approx(rough, n_v);
    let spec_env = env_mix(n, v, alpha);
    let irradiance = env_diffuse(n);
    let e_ms = max(1.0 - (eb.x + eb.y), 0.0);
    var amb = vec3<f32>(0.0);
    for (var c = 0; c < 3; c = c + 1) {
        let f0c = s.f0[c];
        let f_ss = f0c * eb.x + eb.y;
        let f_avg = f0c + (1.0 - f0c) / 21.0;
        let f_ms = e_ms * f_ss * f_avg / max(1.0 - f_avg * e_ms, 1e-4);
        let k_d = max(1.0 - f_ss - f_ms, 0.0) * (1.0 - metallic) * s.albedo[c];
        amb[c] = spec_env[c] * f_ss + irradiance[c] * (f_ms + k_d);
    }
    let ao = clamp(s.ao, 0.0, 1.0);
    out = out + amb * (ao * sqrt(ao));

    out = out + s.emissive;
    // `.map(|c| if c.is_finite() { c.max(0.0) } else { 0.0 })`
    return vec3<f32>(
        select(0.0, max(out.x, 0.0), out.x == out.x && abs(out.x) < INF),
        select(0.0, max(out.y, 0.0), out.y == out.y && abs(out.y) < INF),
        select(0.0, max(out.z, 0.0), out.z == out.z && abs(out.z) < INF),
    );
}

// `shading::surface_for`: material -> Surface, with the procedural texture and
// the selection-highlight rules applied.
fn surface_for(item: u32, face: u32, highlighted: bool, pos: vec3<f32>, n: vec3<f32>, ao: f32) -> Surface {
    let m = materials[item];
    let kind = u32(m.emissive.w);
    var albedo = clamp(m.base.xyz, vec3<f32>(0.0), vec3<f32>(1.0));
    var roughness = m.base.w;
    if (kind != TEX_NONE) {
        let depth = dot(pos - G.eye.xyz, G.fwd.xyz);
        let texel = texture_sample(kind, m.params.y, pos, n, footprint(depth));
        albedo = albedo * texel.albedo;
        roughness = roughness + texel.roughness_delta;
    }
    roughness = clamp(roughness, 0.03, 1.0);
    var metallic = clamp(m.params.x, 0.0, 1.0);
    var emissive = m.emissive.xyz;
    let ior = m.params.w;
    if (highlighted) {
        albedo = mix(albedo, HIGHLIGHT, 0.75);
        roughness = 0.5;
        metallic = 0.0;
        emissive = emissive + HIGHLIGHT * 0.15;
    }
    let f0 = mix(vec3<f32>(dielectric_f0(ior)), albedo, metallic);
    var s: Surface;
    s.pos = pos;
    s.n = n;
    s.v = to_eye(pos);
    s.albedo = albedo;
    s.roughness = roughness;
    s.metallic = metallic;
    s.f0 = f0;
    s.emissive = emissive;
    s.ao = ao;
    return s;
}
