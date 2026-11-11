// The G-buffer pass: the GPU's version of `render/aa.rs::geometry_pass`.
// Multisampled, no culling (the CPU rasterizer draws both faces and resolves
// with depth alone), normals flipped toward the viewer.
//
// Vertices are de-indexed on upload so each one can carry its triangle's
// topology face id, the item index and the selection flag; that is also what
// lets the whole opaque scene go in one draw call.

struct VsIn {
    @location(0) pos: vec3<f32>,
    @location(1) nrm: vec3<f32>,
    // Topology face id of this triangle.
    @location(2) face: u32,
    // item index in the low 16 bits, flags in the high 16 (bit 0: the face is
    // in the item's selection highlight set).
    @location(3) item_flags: u32,
};

struct VsOut {
    @builtin(position) clip: vec4<f32>,
    @location(0) world: vec3<f32>,
    @location(1) nrm: vec3<f32>,
    @location(2) @interpolate(flat) face: u32,
    @location(3) @interpolate(flat) item_flags: u32,
};

struct GBuf {
    @location(0) normal: vec4<f32>,
    @location(1) ids: vec4<u32>,
};

@vertex
fn vs_geom(v: VsIn) -> VsOut {
    var out: VsOut;
    out.clip = G.view_proj * vec4<f32>(v.pos, 1.0);
    out.world = v.pos;
    out.nrm = v.nrm;
    out.face = v.face;
    out.item_flags = v.item_flags;
    return out;
}

@fragment
fn fs_geom(v: VsOut) -> GBuf {
    var n = normalize(v.nrm);
    // Flip toward the viewer; only the sign matters, so the eye vector is not
    // normalised (as on the CPU side).
    var facing: f32;
    if (is_ortho()) {
        facing = -dot(n, G.fwd.xyz);
    } else {
        facing = dot(n, G.eye.xyz - v.world);
    }
    if (facing < 0.0) {
        n = -n;
    }
    var out: GBuf;
    out.normal = vec4<f32>(n, 1.0);
    out.ids = vec4<u32>(
        v.item_flags & 0xffffu,
        v.face & 0xffffu,
        v.face >> 16u,
        v.item_flags >> 16u,
    );
    return out;
}

// The id pass: one non-multisampled `Rg32Uint` target holding
// (item, face) per pixel, which `pick()` reads back. Edges are drawn into the
// same buffer afterwards as fat lines with the edge bit set, so they win over
// faces within the pick radius.
@fragment
fn fs_ids(v: VsOut) -> @location(0) vec2<u32> {
    return vec2<u32>(v.item_flags & 0xffffu, v.face);
}

// Edges in the id pass: the same screen-space capsules the overlay draws, but
// fat (a few pixels) and writing an edge-flagged id, so `pick(x, y, radius)`
// can prefer an edge to the face behind it.
struct PickSegIn {
    @location(0) a: vec3<f32>,
    @location(1) hw: f32,
    @location(2) b: vec3<f32>,
    @location(5) item: u32,
    @location(6) pick: u32,
};

struct PickSegOut {
    @builtin(position) clip: vec4<f32>,
    @location(0) @interpolate(flat) a: vec3<f32>,
    @location(1) @interpolate(flat) b: vec3<f32>,
    @location(2) @interpolate(flat) hw: f32,
    @location(3) @interpolate(flat) item: u32,
    @location(4) @interpolate(flat) pick: u32,
};

// View depth -> the reverse-Z depth value `view_z` inverts, nudged toward the
// camera so a line lying on its own surface wins the depth test.
fn depth_of(z: f32) -> f32 {
    let bias = 0.999;
    if (is_ortho()) {
        let near = G.fwd.w;
        let far = G.viewport.z;
        return clamp((far - z * bias) / (far - near), 0.0, 1.0);
    }
    return clamp(G.fwd.w / max(z * bias, 1e-9), 0.0, 1.0);
}

@vertex
fn vs_edge_id(seg: PickSegIn, @builtin(vertex_index) vi: u32) -> PickSegOut {
    var d = seg.b.xy - seg.a.xy;
    let len = length(d);
    if (len > 1e-6) {
        d = d / len;
    } else {
        d = vec2<f32>(1.0, 0.0);
    }
    let perp = vec2<f32>(-d.y, d.x);
    let reach = seg.hw + 1.0;
    let corner = array<vec2<f32>, 6>(
        vec2<f32>(-1.0, -1.0), vec2<f32>(-1.0, 1.0), vec2<f32>(1.0, -1.0),
        vec2<f32>(1.0, -1.0), vec2<f32>(-1.0, 1.0), vec2<f32>(1.0, 1.0),
    );
    let k = corner[vi];
    var base = seg.a.xy;
    var z = seg.a.z;
    if (k.x > 0.0) {
        base = seg.b.xy;
        z = seg.b.z;
    }
    let p = base + d * (reach * k.x) + perp * (reach * k.y);
    var out: PickSegOut;
    out.clip = vec4<f32>(
        p.x / G.viewport.x * 2.0 - 1.0,
        1.0 - p.y / G.viewport.y * 2.0,
        depth_of(z),
        1.0,
    );
    out.a = seg.a;
    out.b = seg.b;
    out.hw = seg.hw;
    out.item = seg.item;
    out.pick = seg.pick;
    return out;
}

@fragment
fn fs_edge_id(v: PickSegOut) -> @location(0) vec2<u32> {
    let d = v.b.xy - v.a.xy;
    let len2 = dot(d, d);
    var t = 0.0;
    if (len2 > 1e-12) {
        t = clamp(((v.clip.x - v.a.x) * d.x + (v.clip.y - v.a.y) * d.y) / len2, 0.0, 1.0);
    }
    let q = v.a.xy + d * t;
    if (length(v.clip.xy - q) > v.hw) {
        discard;
    }
    return vec2<u32>(v.item, v.pick);
}
