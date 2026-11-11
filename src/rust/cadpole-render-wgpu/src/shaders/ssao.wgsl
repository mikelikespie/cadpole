// Screen-space ambient occlusion: a port of the first two stages of
// src/rust/cadpole-core/src/render/occlusion.rs. 16 cosine-distributed
// hemisphere taps per pixel, rotated per pixel from a constant table, then a
// separable depth-aware 5-tap blur. Run at one sample per pixel over sample 0
// of the multisampled G-buffer; the CPU runs it once per anti-aliasing pass
// and the result is blurred either way.

@group(1) @binding(0) var g_depth: texture_depth_multisampled_2d;
@group(1) @binding(1) var g_normal: texture_multisampled_2d<f32>;
@group(1) @binding(2) var g_ids: texture_multisampled_2d<u32>;
// Only bound for the blur passes; the raw pass binds a 1x1 dummy.
@group(1) @binding(3) var ao_src: texture_2d<f32>;

// 16 cosine-distributed hemisphere samples (Hammersley), pulled toward the
// origin by lerp(0.1, 1, (i/16)^2).
const KERNEL: array<vec3<f32>, 16> = array<vec3<f32>, 16>(
    vec3<f32>(0.017678, 0.000000, 0.098425),
    vec3<f32>(-0.031695, 0.000000, 0.098544),
    vec3<f32>(0.000000, 0.045087, 0.104773),
    vec3<f32>(0.000000, -0.061569, 0.116355),
    vec3<f32>(0.058594, 0.058594, 0.132467),
    vec3<f32>(-0.077895, -0.077895, 0.152209),
    vec3<f32>(-0.102110, 0.102110, 0.174578),
    vec3<f32>(0.131810, -0.131810, 0.198446),
    vec3<f32>(0.218851, 0.090651, 0.222512),
    vec3<f32>(-0.273913, -0.113459, 0.245241),
    vec3<f32>(-0.139988, 0.337962, 0.264752),
    vec3<f32>(0.170455, -0.411516, 0.278630),
    vec3<f32>(0.205063, 0.495065, 0.283547),
    vec3<f32>(-0.244002, -0.589074, 0.274383),
    vec3<f32>(-0.693986, 0.287458, 0.241600),
    vec3<f32>(0.810227, -0.335607, 0.157511),
);

// Per-pixel rotation of the kernel, indexed by (y & 3) * 4 + (x & 3).
const ROT: array<vec2<f32>, 16> = array<vec2<f32>, 16>(
    vec2<f32>(1.000000, 0.000000),
    vec2<f32>(-1.000000, 0.000000),
    vec2<f32>(0.707107, 0.707107),
    vec2<f32>(-0.707107, -0.707107),
    vec2<f32>(0.000000, -1.000000),
    vec2<f32>(0.000000, 1.000000),
    vec2<f32>(0.707107, -0.707107),
    vec2<f32>(-0.707107, 0.707107),
    vec2<f32>(0.382683, 0.923880),
    vec2<f32>(-0.382683, -0.923880),
    vec2<f32>(0.923880, 0.382683),
    vec2<f32>(-0.923880, -0.382683),
    vec2<f32>(0.923880, -0.382683),
    vec2<f32>(-0.923880, 0.382683),
    vec2<f32>(0.382683, -0.923880),
    vec2<f32>(-0.382683, 0.923880),
);

fn covered_at(c: vec2<i32>) -> bool {
    let dim = vec2<i32>(textureDimensions(g_ids));
    if (c.x < 0 || c.y < 0 || c.x >= dim.x || c.y >= dim.y) {
        return false;
    }
    return textureLoad(g_ids, c, 0).x != NO_ITEM;
}

fn depth_at(c: vec2<i32>) -> f32 {
    return view_z(textureLoad(g_depth, c, 0));
}

@fragment
fn fs_ssao(in: FsOut) -> @location(0) f32 {
    let c = vec2<i32>(i32(in.pos.x), i32(in.pos.y));
    if (!covered_at(c)) {
        return 1.0;
    }
    let dim = vec2<i32>(textureDimensions(g_ids));
    let frag_z = depth_at(c);
    let p = unproject(in.pos.x, in.pos.y, frag_z);
    let n = normalize(textureLoad(g_normal, c, 0).xyz);
    let radius = G.ao.x;
    let bias = G.ao.y;

    let r = ROT[(u32(c.y) & 3u) * 4u + (u32(c.x) & 3u)];
    var up = vec3<f32>(0.0, 0.0, 1.0);
    if (abs(n.z) > 0.9) {
        up = vec3<f32>(1.0, 0.0, 0.0);
    }
    let t0 = normalize(cross(up, n));
    let b0 = cross(n, t0);
    let t = t0 * r.x + b0 * r.y;
    let b = t0 * (-r.y) + b0 * r.x;

    var occ = 0.0;
    for (var k = 0u; k < 16u; k = k + 1u) {
        let kk = KERNEL[k];
        let off = t * kk.x + b * kk.y + n * kk.z;
        let sp = p + off * radius;
        let s = project(sp);
        if (!is_ortho() && s.z <= 1e-4) {
            continue;
        }
        if (!(s.x >= 0.0 && s.y >= 0.0 && s.x < G.viewport.x && s.y < G.viewport.y)) {
            continue;
        }
        let sc = vec2<i32>(i32(s.x), i32(s.y));
        if (sc.x >= dim.x || sc.y >= dim.y) {
            continue;
        }
        let sd = depth_at(sc);
        if (sd >= INF || sd >= s.z - bias) {
            continue;
        }
        // Something is in front of the sample point: occluded, unless it is
        // so much closer that it is a different surface.
        occ = occ + sstep(0.0, 1.0, min(radius / abs(frag_z - sd), 1.0));
    }
    return 1.0 - occ / 16.0;
}

// One axis of the depth-aware 5-tap blur, weights (1, 4, 6, 4, 1).
fn blur(c: vec2<i32>, step: vec2<i32>) -> f32 {
    let dim = vec2<i32>(textureDimensions(g_ids));
    let z0 = depth_at(c);
    let dmax = G.ao.z;
    let w = array<f32, 5>(1.0, 4.0, 6.0, 4.0, 1.0);
    var sum = 0.0;
    var wsum = 0.0;
    for (var i = 0; i < 5; i = i + 1) {
        let q = c + step * (i - 2);
        if (q.x < 0 || q.y < 0 || q.x >= dim.x || q.y >= dim.y) {
            continue;
        }
        if (!covered_at(q) || abs(depth_at(q) - z0) > dmax) {
            continue;
        }
        sum = sum + textureLoad(ao_src, q, 0).x * w[i];
        wsum = wsum + w[i];
    }
    if (wsum > 0.0) {
        return sum / wsum;
    }
    return 1.0;
}

@fragment
fn fs_blur_x(in: FsOut) -> @location(0) f32 {
    let c = vec2<i32>(i32(in.pos.x), i32(in.pos.y));
    if (!covered_at(c)) {
        return 1.0;
    }
    return blur(c, vec2<i32>(1, 0));
}

@fragment
fn fs_blur_y(in: FsOut) -> @location(0) f32 {
    let c = vec2<i32>(i32(in.pos.x), i32(in.pos.y));
    if (!covered_at(c)) {
        return 1.0;
    }
    return blur(c, vec2<i32>(0, 1));
}
