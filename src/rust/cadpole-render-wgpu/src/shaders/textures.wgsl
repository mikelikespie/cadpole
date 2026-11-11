// Procedural textures: a line-for-line port of
// src/rust/cadpole-core/src/render/textures.rs, which was written in the subset
// of arithmetic WGSL also has (floor, fract, abs, min/max, mix, smoothstep,
// dot, sqrt and integer ops -- no sin/cos/pow/exp and no state) precisely so
// the two can be kept in step. `textures_match_the_cpu_port` in lib.rs renders
// the same textured surface both ways and bounds the difference.
//
// A texture is a multiplier: albedo is in 0..1, so textures darken and
// modulate, never brighten; roughness_delta is added before the shading clamp.

struct Texel {
    albedo: vec3<f32>,
    roughness_delta: f32,
};

const TEX_NONE: u32 = 0u;
const TEX_CHECKER: u32 = 1u;
const TEX_STRIPES: u32 = 2u;
const TEX_NOISE: u32 = 3u;
const TEX_WOOD: u32 = 4u;
const TEX_DOTS: u32 = 5u;

const DEFAULT_SCALE: f32 = 10.0;
const COORD_LIMIT: f32 = 1.0e6;

const CHECKER_MEAN: f32 = 0.775;
const CHECKER_AMP: f32 = 0.225;
const STRIPES_MEAN: f32 = 0.8;
const STRIPES_AMP: f32 = 0.2;
const DOT_DEPTH: f32 = 0.45;
const DOTS_MEAN: f32 = 0.9585;
const WOOD_DARK: vec3<f32> = vec3<f32>(0.75, 0.6, 0.45);
const WOOD_MEAN: vec3<f32> = vec3<f32>(0.7019, 0.6581, 0.6142);
const WOOD_ROUGHNESS: f32 = -0.15;
const WOOD_WARP: f32 = 0.4;
const NOISE_CONTRAST: f32 = 2.5;
const HASH_SALT: u32 = 0x9e3779b9u;

fn tex_fract(x: f32) -> f32 {
    return x - floor(x);
}

// Finite and bounded, so `floor` never overflows a cast and NaN never escapes.
fn san(x: f32) -> f32 {
    if (x == x && abs(x) < INF) {
        return clamp(x, -COORD_LIMIT, COORD_LIMIT);
    }
    return 0.0;
}

fn finite_clamp(x: f32, lo: f32, hi: f32, fallback: f32) -> f32 {
    if (x == x && abs(x) < INF) {
        return clamp(x, lo, hi);
    }
    return fallback;
}

// p(u) = (-1)^floor(u), without an integer cast.
fn square_wave(u: f32) -> f32 {
    if (tex_fract(u * 0.5) >= 0.5) {
        return -1.0;
    }
    return 1.0;
}

// Antiderivative of square_wave: a triangle wave in 0..1, period 2.
fn tri_wave(u: f32) -> f32 {
    return 1.0 - abs(1.0 - 2.0 * tex_fract(u * 0.5));
}

// Mean of square_wave over [u - f/2, u + f/2], exactly.
fn filtered_square(u: f32, f: f32) -> f32 {
    if (f < 1.0e-4) {
        return square_wave(u);
    }
    return clamp((tri_wave(u + 0.5 * f) - tri_wave(u - 0.5 * f)) / f, -1.0, 1.0);
}

// How far a pattern with no closed-form filter has faded to its mean.
fn minify(f: f32) -> f32 {
    return sstep(0.25, 1.0, f);
}

fn checker_pat(u: f32, v: f32, f: f32) -> f32 {
    return CHECKER_MEAN + CHECKER_AMP * filtered_square(u, f) * filtered_square(v, f);
}

fn stripes_pat(u: f32, v: f32, f: f32) -> f32 {
    return STRIPES_MEAN + STRIPES_AMP * filtered_square(u, f);
}

fn dots_pat(u: vec3<f32>, f: f32) -> f32 {
    let q = vec3<f32>(tex_fract(u.x) - 0.5, tex_fract(u.y) - 0.5, tex_fract(u.z) - 0.5);
    let d = sqrt(dot(q, q));
    let a = 1.0 - DOT_DEPTH * sstep(0.30, 0.26, d);
    return mix(a, DOTS_MEAN, minify(f));
}

// lowbias32 (Chris Wellons).
fn lowbias32(x0: u32) -> u32 {
    var x = x0;
    x = x ^ (x >> 16u);
    x = x * 0x7feb352du;
    x = x ^ (x >> 15u);
    x = x * 0x846ca68bu;
    x = x ^ (x >> 16u);
    return x;
}

fn hash3(ix: i32, iy: i32, iz: i32, seed: u32) -> u32 {
    let h0 = lowbias32(bitcast<u32>(iz) ^ seed ^ HASH_SALT);
    let h1 = lowbias32(bitcast<u32>(iy) + h0);
    return lowbias32(bitcast<u32>(ix) + h1);
}

// Quintic fade 6t^5 - 15t^4 + 10t^3.
fn quintic(t: f32) -> f32 {
    return t * t * t * (t * (t * 6.0 - 15.0) + 10.0);
}

// Trilinear value noise in 0..1 with a quintic fade.
fn value_noise(p: vec3<f32>) -> f32 {
    let fl = floor(p);
    let i = vec3<i32>(i32(fl.x), i32(fl.y), i32(fl.z));
    var c: array<f32, 8>;
    for (var dz = 0; dz < 2; dz = dz + 1) {
        let hz = lowbias32(bitcast<u32>(i.z + dz) ^ HASH_SALT);
        for (var dy = 0; dy < 2; dy = dy + 1) {
            let hy = lowbias32(bitcast<u32>(i.y + dy) + hz);
            for (var dx = 0; dx < 2; dx = dx + 1) {
                let h = lowbias32(bitcast<u32>(i.x + dx) + hy);
                c[dx + 2 * dy + 4 * dz] = f32(h) * (1.0 / 4294967296.0);
            }
        }
    }
    let t = vec3<f32>(quintic(p.x - fl.x), quintic(p.y - fl.y), quintic(p.z - fl.z));
    let x0 = mix(c[0], c[1], t.x);
    let x1 = mix(c[2], c[3], t.x);
    let x2 = mix(c[4], c[5], t.x);
    let x3 = mix(c[6], c[7], t.x);
    return mix(mix(x0, x1, t.y), mix(x2, x3, t.y), t.z);
}

// fBm that drops octaves whose wavelength falls below twice the footprint.
fn fbm_filtered(p0: vec3<f32>, octaves: u32, f: f32) -> f32 {
    let p = vec3<f32>(san(p0.x), san(p0.y), san(p0.z));
    var sum = 0.0;
    var norm = 0.0;
    var amp = 1.0;
    var freq = 1.0;
    for (var i = 0u; i < min(octaves, 8u); i = i + 1u) {
        var w = 1.0;
        if (f > 0.0) {
            w = 1.0 - sstep(0.5, 1.0, 2.0 * f * freq);
        }
        if (w > 0.0) {
            sum = sum + amp * w * value_noise(p * freq);
            norm = norm + amp * w;
        }
        amp = amp * 0.5;
        freq = freq * 2.0;
    }
    if (norm > 0.0) {
        return sum / norm;
    }
    return 0.5;
}

fn noise_texel(u: vec3<f32>, f: f32) -> Texel {
    let n = clamp((fbm_filtered(u, 4u, f) - 0.5) * NOISE_CONTRAST + 0.5, 0.0, 1.0);
    return Texel(vec3<f32>(0.75 + 0.25 * n), 0.3 * (n - 0.5));
}

fn wood_texel(u: vec3<f32>, f: f32) -> Texel {
    let warp = fbm_filtered(u * 0.5, 3u, f * 0.5) - 0.5;
    let r = sqrt(u.x * u.x + u.y * u.y) + WOOD_WARP * warp;
    let t = sstep(0.35, 0.65, tex_fract(1.5 * r));
    let tone = 0.55 + 0.45 * t;
    let albedo = mix(WOOD_DARK, vec3<f32>(1.0), t) * tone;
    let m = minify(f);
    return Texel(mix(albedo, WOOD_MEAN, m), mix(WOOD_ROUGHNESS * t, WOOD_ROUGHNESS * 0.5, m));
}

// Triplanar weights |n|^4, normalised.
fn triplanar_weights(n: vec3<f32>) -> vec3<f32> {
    let sq = n * n;
    let w = sq * sq;
    let s = w.x + w.y + w.z;
    if (s == s && s > 1e-12) {
        return w / s;
    }
    return vec3<f32>(0.0, 0.0, 1.0);
}

fn triplanar_checker(u: vec3<f32>, n: vec3<f32>, f: f32) -> f32 {
    let w = triplanar_weights(vec3<f32>(san(n.x), san(n.y), san(n.z)));
    var v = 0.0;
    if (w.x > 0.0) { v = v + w.x * checker_pat(u.y, u.z, f); }
    if (w.y > 0.0) { v = v + w.y * checker_pat(u.x, u.z, f); }
    if (w.z > 0.0) { v = v + w.z * checker_pat(u.x, u.y, f); }
    return v;
}

fn triplanar_stripes(u: vec3<f32>, n: vec3<f32>, f: f32) -> f32 {
    let w = triplanar_weights(vec3<f32>(san(n.x), san(n.y), san(n.z)));
    var v = 0.0;
    if (w.x > 0.0) { v = v + w.x * stripes_pat(u.y, u.z, f); }
    if (w.y > 0.0) { v = v + w.y * stripes_pat(u.x, u.z, f); }
    if (w.z > 0.0) { v = v + w.z * stripes_pat(u.x, u.y, f); }
    return v;
}

// `textures::sample`: object-space position in mm, unit normal, and the world
// size of one pixel there.
fn texture_sample(kind: u32, scale_mm: f32, pos: vec3<f32>, n: vec3<f32>, fp: f32) -> Texel {
    if (kind == TEX_NONE) {
        return Texel(vec3<f32>(1.0), 0.0);
    }
    var scale = DEFAULT_SCALE;
    if (scale_mm == scale_mm && scale_mm > 0.0 && scale_mm < INF) {
        scale = scale_mm;
    }
    let inv = 1.0 / scale;
    let u = vec3<f32>(san(pos.x) * inv, san(pos.y) * inv, san(pos.z) * inv);
    var f = 0.0;
    if (fp == fp && abs(fp) < INF) {
        f = clamp(fp * inv, 0.0, COORD_LIMIT);
    }
    var t = Texel(vec3<f32>(1.0), 0.0);
    if (kind == TEX_CHECKER) {
        t = Texel(vec3<f32>(triplanar_checker(u, n, f)), 0.0);
    } else if (kind == TEX_STRIPES) {
        t = Texel(vec3<f32>(triplanar_stripes(u, n, f)), 0.0);
    } else if (kind == TEX_NOISE) {
        t = noise_texel(u, f);
    } else if (kind == TEX_WOOD) {
        t = wood_texel(u, f);
    } else if (kind == TEX_DOTS) {
        t = Texel(vec3<f32>(dots_pat(u, f)), 0.0);
    }
    return Texel(
        vec3<f32>(
            finite_clamp(t.albedo.x, 0.0, 1.0, 1.0),
            finite_clamp(t.albedo.y, 0.0, 1.0, 1.0),
            finite_clamp(t.albedo.z, 0.0, 1.0, 1.0),
        ),
        finite_clamp(t.roughness_delta, -0.3, 0.3, 0.0),
    );
}
