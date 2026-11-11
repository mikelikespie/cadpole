// Shared declarations for every Cadpole GPU pass: the frame uniform, the material
// table, and the small maths the CPU rasterizer and this renderer must agree
// on (src/rust/cadpole-core/src/render.rs and render/aa.rs).
//
// Depth convention: reverse-Z. `view_proj` maps a world point to clip space
// with ndc_z = near / z_view in perspective and ndc_z = (far - z) / (far -
// near) in orthographic, so a cleared depth of 0 means "nothing here" and
// `view_z()` below inverts it back to the CPU renderer's `Frame::depth` (the
// distance along `fwd`, INFINITY for background).

struct Globals {
    view_proj: mat4x4<f32>,
    // xyz eye, w = 1 for orthographic
    eye: vec4<f32>,
    // xyz right, w = focal (pixels)
    right: vec4<f32>,
    // xyz up, w = ortho_scale (pixels per mm)
    up: vec4<f32>,
    // xyz fwd, w = near plane
    fwd: vec4<f32>,
    // width, height, far plane, edge crease threshold (degrees)
    viewport: vec4<f32>,
    // scene lo xyz, w = bbox diagonal
    scene_lo: vec4<f32>,
    // scene hi xyz, w = ground z
    scene_hi: vec4<f32>,
    // scene centre xyz, w = exposure
    center: vec4<f32>,
    // display-linear background rgb, w = the tone curve (`ToneMap::index`:
    // 0 clamp, 1 ACES, 2 Neutral)
    background: vec4<f32>,
    // grid: minor step, major step, fade start, fade end
    grid: vec4<f32>,
    // ssao: radius, bias, blur depth limit, 1 = enabled
    ao: vec4<f32>,
    // contact heightfield: origin x, origin y, cell x, cell y
    hf: vec4<f32>,
    // heightfield size, ground z, 1 = enabled, near-ground fade distance
    hf2: vec4<f32>,
    // sample count, item count, unused, unused
    counts: vec4<u32>,
}

// One material per scene item. Mirrors `cadpole_core::context::Material` as far
// as the rasterizer reads it.
struct GpuMaterial {
    // base colour rgb, w = roughness
    base: vec4<f32>,
    // metallic, texture scale (mm), opacity, index of refraction
    params: vec4<f32>,
    // emissive rgb, w = texture kind (0 none, 1 checker, 2 stripes, 3 noise,
    // 4 wood, 5 dots)
    emissive: vec4<f32>,
}

const NO_ITEM: u32 = 0xffffu;
const NO_FACE: u32 = 0xffffffffu;
const HIGHLIGHT: vec3<f32> = vec3<f32>(0.98, 0.62, 0.12);
const PI: f32 = 3.14159265358979323846;
const INF: f32 = 3.0e38;

@group(0) @binding(0) var<uniform> G: Globals;
@group(0) @binding(1) var<storage, read> materials: array<GpuMaterial>;

// `smoothstep` with no ordering requirement on the edges: the CPU renderer
// calls it with e0 > e1 in several places (dots, the contact window) and
// WGSL's builtin leaves that undefined.
fn sstep(e0: f32, e1: f32, x: f32) -> f32 {
    let t = clamp((x - e0) / (e1 - e0), 0.0, 1.0);
    return t * t * (3.0 - 2.0 * t);
}

fn luminance(c: vec3<f32>) -> f32 {
    return 0.2126 * c.x + 0.7152 * c.y + 0.0722 * c.z;
}

fn is_ortho() -> bool {
    return G.eye.w > 0.5;
}

// Depth-buffer value -> the CPU renderer's `Frame::depth` (view-space z along
// `fwd`). A cleared 0 is background, which the CPU spells INFINITY.
fn view_z(d: f32) -> f32 {
    if (d <= 0.0) {
        return INF;
    }
    if (is_ortho()) {
        let near = G.fwd.w;
        let far = G.viewport.z;
        return far - d * (far - near);
    }
    return G.fwd.w / d;
}

// Inverse of `View::project` for a pixel centre sample: screen (px, py) and
// view depth z back to a world point. Identical arithmetic to
// `cadpole_core::render::View::unproject` with jitter (0.5, 0.5).
fn unproject(px: f32, py: f32, z: f32) -> vec3<f32> {
    let nx = px - G.viewport.x * 0.5;
    let ny = G.viewport.y * 0.5 - py;
    var x: f32;
    var y: f32;
    if (is_ortho()) {
        x = nx / G.up.w;
        y = ny / G.up.w;
    } else {
        x = nx * z / G.right.w;
        y = ny * z / G.right.w;
    }
    return G.eye.xyz + G.right.xyz * x + G.up.xyz * y + G.fwd.xyz * z;
}

// `View::project` at the pixel-centre jitter: world -> (screen x, screen y,
// view depth). A returned z <= 1e-4 in perspective means "behind the eye",
// which the CPU spells `None`.
fn project(p: vec3<f32>) -> vec3<f32> {
    let d = p - G.eye.xyz;
    let x = dot(d, G.right.xyz);
    let y = dot(d, G.up.xyz);
    let z = dot(d, G.fwd.xyz);
    if (is_ortho()) {
        return vec3<f32>(G.viewport.x * 0.5 + x * G.up.w, G.viewport.y * 0.5 - y * G.up.w, z);
    }
    if (z <= 1e-4) {
        return vec3<f32>(0.0, 0.0, z);
    }
    return vec3<f32>(
        G.viewport.x * 0.5 + x * G.right.w / z,
        G.viewport.y * 0.5 - y * G.right.w / z,
        z,
    );
}

// Unit vector from a surface point toward the eye (`View::to_eye`).
fn to_eye(p: vec3<f32>) -> vec3<f32> {
    if (is_ortho()) {
        return -G.fwd.xyz;
    }
    return normalize(G.eye.xyz - p);
}

// World size of one pixel at view depth z (`View::footprint`).
fn footprint(z: f32) -> f32 {
    if (is_ortho()) {
        return 1.0 / G.up.w;
    }
    return max(z, 1e-4) / G.right.w;
}

// ---------------------------------------------------------------------------
// Tonemap and transfer, ported from render/aa.rs.

// ACES fitted curve (Narkowicz).
fn aces(x0: f32) -> f32 {
    let x = max(x0, 0.0);
    return clamp((x * (2.51 * x + 0.03)) / (x * (2.43 * x + 0.59) + 0.14), 0.0, 1.0);
}

fn tonemap_rgb(c: vec3<f32>, exposure: f32) -> vec3<f32> {
    return vec3<f32>(aces(c.x * exposure), aces(c.y * exposure), aces(c.z * exposure));
}

// Khronos PBR Neutral (render/aa.rs `neutral`): a black-point lift from the
// minimum channel, a rational compression of the maximum with the others
// scaled along, then a controlled mix toward white. No hue rotation.
fn neutral(c0: vec3<f32>) -> vec3<f32> {
    let c1 = max(c0, vec3<f32>(0.0));
    let x = min(c1.x, min(c1.y, c1.z));
    let offset = select(0.04, x - 6.25 * x * x, x < 0.08);
    let c = c1 - vec3<f32>(offset);
    let peak = max(c.x, max(c.y, c.z));
    if (peak < 0.76) {
        return clamp(c, vec3<f32>(0.0), vec3<f32>(1.0));
    }
    let d = 0.24;
    let new_peak = 1.0 - d * d / (peak + d - 0.76);
    let cs = c * (new_peak / peak);
    let g = 1.0 - 1.0 / (0.15 * (peak - new_peak) + 1.0);
    return clamp(mix(cs, vec3<f32>(new_peak), g), vec3<f32>(0.0), vec3<f32>(1.0));
}

// `ToneMap::index`: 0 a plain clamp (no exposure), 1 ACES, 2 Neutral.
fn tonemap_mode(c: vec3<f32>, exposure: f32, mode: u32) -> vec3<f32> {
    if (mode == 2u) {
        return neutral(c * exposure);
    }
    if (mode == 1u) {
        return tonemap_rgb(c, exposure);
    }
    return clamp(c, vec3<f32>(0.0), vec3<f32>(1.0));
}

fn to_srgb(v0: f32) -> f32 {
    let v = clamp(v0, 0.0, 1.0);
    if (v <= 0.0031308) {
        return 12.92 * v;
    }
    return 1.055 * pow(v, 1.0 / 2.4) - 0.055;
}

// A full-screen triangle; no vertex buffer. Fragments read `@builtin(position)`,
// which is in framebuffer coordinates (x right, y down, pixel centres at
// +0.5) -- the CPU rasterizer's screen space exactly.
struct FsOut {
    @builtin(position) pos: vec4<f32>,
};

@vertex
fn vs_fullscreen(@builtin(vertex_index) vi: u32) -> FsOut {
    var out: FsOut;
    let x = -1.0 + f32((vi & 1u) * 4u);
    let y = -1.0 + f32((vi & 2u) * 2u);
    out.pos = vec4<f32>(x, y, 0.0, 1.0);
    return out;
}
