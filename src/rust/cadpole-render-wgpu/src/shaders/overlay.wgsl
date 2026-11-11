// The line overlay: ground grid, feature edges, silhouettes and selection
// highlights, drawn as ink into the display-linear image after the tonemap.
// Ported from src/rust/cadpole-core/src/render/lines.rs, which computes coverage
// analytically (a segment is a round-capped capsule, a pixel's coverage is
// clamp(halfwidth + 0.5 - distance, 0, 1)) rather than by supersampling.
//
// Ink layers combine with `max`, so a polyline does not darken at its joins;
// that is a `BlendOperation::Max` on the R16Float ink target here and a
// per-pixel max on the CPU. Layers are then composited in order (grid,
// silhouette, crease, highlight) by `fs_composite`, whose premultiplied output
// reproduces the CPU's successive `lerp3(display, ink, coverage)`.

@group(1) @binding(0) var g_depth: texture_depth_multisampled_2d;
@group(1) @binding(1) var g_ids: texture_multisampled_2d<u32>;
@group(1) @binding(2) var g_normal: texture_multisampled_2d<f32>;
@group(1) @binding(3) var ink_src: texture_2d<f32>;
@group(1) @binding(4) var<uniform> OV: OverlayParams;

struct OverlayParams {
    // Ink colour (display-linear) and the layer's opacity multiplier.
    color: vec4<f32>,
};

const GRID_WIDTH_PX: f32 = 1.0;
const ALPHA_SILHOUETTE: f32 = 0.62;
const ALPHA_GRID_MINOR: f32 = 0.45;
const ALPHA_GRID_MAJOR: f32 = 0.70;
const ALPHA_GRID_AXIS: f32 = 0.80;
// sRGB8 (48, 50, 56), (76, 79, 87), (128, 58, 58), (64, 118, 66) in
// display-linear, matching `lines::ink`.
const GRID_MINOR_INK: vec3<f32> = vec3<f32>(0.0295568351, 0.0318960324, 0.0395462364);
const GRID_MAJOR_INK: vec3<f32> = vec3<f32>(0.0722718537, 0.0781874284, 0.0953074694);
const GRID_AXIS_X_INK: vec3<f32> = vec3<f32>(0.215860531, 0.0423114114, 0.0423114114);
const GRID_AXIS_Y_INK: vec3<f32> = vec3<f32>(0.0512694642, 0.18116425, 0.0544802807);

fn depth_at(c: vec2<i32>, s: i32) -> f32 {
    return view_z(textureLoad(g_depth, c, s));
}

fn in_bounds(c: vec2<i32>) -> bool {
    let dim = vec2<i32>(textureDimensions(g_ids));
    return c.x >= 0 && c.y >= 0 && c.x < dim.x && c.y < dim.y;
}

// Soft depth test with a slope-scaled bias (`lines::visibility`), averaged
// over the multisamples of the pixel -- the CPU gets the same softening by
// depth-testing the same unjittered ink against every jittered pass.
fn visibility(c: vec2<i32>, z: f32) -> f32 {
    let diag = G.scene_lo.w;
    var slope = 0.0;
    let d0 = depth_at(c, 0);
    if (d0 < INF) {
        let steps = array<vec2<i32>, 4>(
            vec2<i32>(-1, 0), vec2<i32>(1, 0), vec2<i32>(0, -1), vec2<i32>(0, 1),
        );
        for (var i = 0; i < 4; i = i + 1) {
            let q = c + steps[i];
            if (in_bounds(q)) {
                let dn = depth_at(q, 0);
                if (dn < INF) {
                    slope = max(slope, abs(dn - d0));
                }
            }
        }
    }
    slope = min(slope, 0.006 * diag);
    let bias = 0.0008 * diag + slope;
    let samples = i32(G.counts.x);
    var vis = 0.0;
    for (var s = 0; s < samples; s = s + 1) {
        let d = depth_at(c, s);
        if (d >= INF) {
            vis = vis + 1.0;
        } else {
            vis = vis + 1.0 - sstep(bias, bias + 0.0008 * diag + slope, z - d);
        }
    }
    return vis / f32(samples);
}

// ---------------------------------------------------------------------------
// The infinite ground grid.

// Ray through a screen sample scaled so its dot product with `fwd` is 1, which
// makes the ray parameter at a hit the view depth (`lines::depth_ray`).
fn depth_ray_origin(sx: f32, sy: f32) -> vec3<f32> {
    if (is_ortho()) {
        return unproject(sx, sy, 0.0);
    }
    return G.eye.xyz;
}

fn depth_ray_dir(sx: f32, sy: f32) -> vec3<f32> {
    if (is_ortho()) {
        return G.fwd.xyz;
    }
    return unproject(sx, sy, 1.0) - G.eye.xyz;
}

// (world hit, view depth) where the pixel's ray meets z = plane_z; w < 0 for a
// miss.
fn plane_hit(sx: f32, sy: f32, plane_z: f32) -> vec4<f32> {
    let o = depth_ray_origin(sx, sy);
    let d = depth_ray_dir(sx, sy);
    if (abs(d.z) < 1e-12) {
        return vec4<f32>(0.0, 0.0, 0.0, -1.0);
    }
    let t = (plane_z - o.z) / d.z;
    if (!(t > 1e-4) || t >= INF) {
        return vec4<f32>(0.0, 0.0, 0.0, -1.0);
    }
    let p = o + d * t;
    return vec4<f32>(p, t);
}

fn grid_line(coord: f32, spacing: f32, per_px: f32, hw: f32) -> f32 {
    let q = coord / spacing;
    let g = abs(q - round(q)) * spacing / per_px;
    return clamp(hw + 0.5 - g, 0.0, 1.0);
}

@fragment
fn fs_grid(in: FsOut) -> @location(0) vec4<f32> {
    let c = vec2<i32>(i32(in.pos.x), i32(in.pos.y));
    let x = in.pos.x;
    let y = in.pos.y;
    let plane = G.scene_hi.w;
    let hit = plane_hit(x, y, plane);
    if (hit.w < 0.0) {
        discard;
    }
    let p = hit.xyz;
    let z = hit.w;
    let dist = sqrt((p.x - G.center.x) * (p.x - G.center.x) + (p.y - G.center.y) * (p.y - G.center.y));
    let fade = 1.0 - sstep(G.grid.z, G.grid.w, dist);
    if (fade <= 0.0) {
        discard;
    }
    let vis = visibility(c, z);
    if (vis <= 0.0) {
        discard;
    }
    let qx = plane_hit(x + 1.0, y, plane);
    let qy = plane_hit(x, y + 1.0, plane);
    if (qx.w < 0.0 || qy.w < 0.0) {
        discard;
    }
    // World size of one pixel on the plane, per world axis.
    let wx = max(max(abs(qx.x - p.x), abs(qy.x - p.x)), 1e-9);
    let wy = max(max(abs(qx.y - p.y), abs(qy.y - p.y)), 1e-9);
    let step = G.grid.x;
    let major = G.grid.y;
    let hw = GRID_WIDTH_PX * 0.5;
    // Spacing in pixels; below a few pixels the lines would merge into moire,
    // so they fade out instead.
    let fx = sstep(4.0, 14.0, step / wx);
    let fy = sstep(4.0, 14.0, step / wy);
    let mx = sstep(4.0, 14.0, major / wx);
    let my = sstep(4.0, 14.0, major / wy);
    let c_minor = max(grid_line(p.x, step, wx, hw) * fx, grid_line(p.y, step, wy, hw) * fy);
    let c_major = max(grid_line(p.x, major, wx, hw) * mx, grid_line(p.y, major, wy, hw) * my);
    let c_ax = clamp((hw + 0.5) - abs(p.y) / wy, 0.0, 1.0) * my;
    let c_ay = clamp((hw + 0.5) - abs(p.x) / wx, 0.0, 1.0) * mx;
    let amt = fade * vis;

    // The CPU applies four successive `lerp3(col, ink, a)`. Expanded, that is
    // `col * prod(1 - a) + sum(ink_i a_i prod_{j>i} (1 - a_j))`, which is
    // exactly a premultiplied source over the destination.
    var rgb = vec3<f32>(0.0);
    var keep = 1.0;
    let a0 = c_minor * ALPHA_GRID_MINOR * amt;
    let a1 = c_major * ALPHA_GRID_MAJOR * amt;
    let a2 = c_ax * ALPHA_GRID_AXIS * amt;
    let a3 = c_ay * ALPHA_GRID_AXIS * amt;
    rgb = GRID_MINOR_INK * a0;
    keep = 1.0 - a0;
    rgb = rgb * (1.0 - a1) + GRID_MAJOR_INK * a1;
    keep = keep * (1.0 - a1);
    rgb = rgb * (1.0 - a2) + GRID_AXIS_X_INK * a2;
    keep = keep * (1.0 - a2);
    rgb = rgb * (1.0 - a3) + GRID_AXIS_Y_INK * a3;
    keep = keep * (1.0 - a3);
    return vec4<f32>(rgb, 1.0 - keep);
}

// ---------------------------------------------------------------------------
// Feature edges: instanced round-capped capsules in screen space.

struct SegIn {
    // screen x, y, view z of the two ends
    @location(0) a: vec3<f32>,
    @location(1) hw: f32,
    @location(2) b: vec3<f32>,
    @location(3) alpha: f32,
    // bit 0: estimate the crease weight from the G-buffer normals.
    @location(4) flags: u32,
};

struct SegOut {
    @builtin(position) clip: vec4<f32>,
    @location(0) @interpolate(flat) a: vec3<f32>,
    @location(1) @interpolate(flat) b: vec3<f32>,
    @location(2) @interpolate(flat) hw: f32,
    @location(3) @interpolate(flat) alpha: f32,
    @location(4) @interpolate(flat) flags: u32,
};

@vertex
fn vs_edge(seg: SegIn, @builtin(vertex_index) vi: u32) -> SegOut {
    var d = seg.b.xy - seg.a.xy;
    let len = length(d);
    if (len > 1e-6) {
        d = d / len;
    } else {
        d = vec2<f32>(1.0, 0.0);
    }
    let perp = vec2<f32>(-d.y, d.x);
    let reach = seg.hw + 1.0;
    // Two triangles over the capsule's oriented bounding box.
    let corner = array<vec2<f32>, 6>(
        vec2<f32>(-1.0, -1.0), vec2<f32>(-1.0, 1.0), vec2<f32>(1.0, -1.0),
        vec2<f32>(1.0, -1.0), vec2<f32>(-1.0, 1.0), vec2<f32>(1.0, 1.0),
    );
    let k = corner[vi];
    var base = seg.a.xy;
    if (k.x > 0.0) {
        base = seg.b.xy;
    }
    let p = base + d * (reach * k.x) + perp * (reach * k.y);
    var out: SegOut;
    out.clip = vec4<f32>(p.x / G.viewport.x * 2.0 - 1.0, 1.0 - p.y / G.viewport.y * 2.0, 0.0, 1.0);
    out.a = seg.a;
    out.b = seg.b;
    out.hw = seg.hw;
    out.alpha = seg.alpha;
    out.flags = seg.flags;
    return out;
}

fn normal_depth_at(x: f32, y: f32) -> vec4<f32> {
    let c = vec2<i32>(i32(x), i32(y));
    if (!(x >= 0.0 && y >= 0.0) || !in_bounds(c)) {
        return vec4<f32>(0.0, 0.0, 0.0, -1.0);
    }
    let n = textureLoad(g_normal, c, 0).xyz;
    return vec4<f32>(n, depth_at(c, 0));
}

// `lines::Estimator::weight`: with no topology, keep the line only where the
// surfaces either side of it disagree by more than the crease angle, or where
// one side is a different surface altogether.
fn estimator_weight(mid: vec2<f32>, dir: vec2<f32>) -> f32 {
    let len = length(dir);
    if (!(len > 1e-6)) {
        return 1.0;
    }
    let px = -dir.y / len * 2.0;
    let py = dir.x / len * 2.0;
    let m = normal_depth_at(mid.x, mid.y);
    let a = normal_depth_at(mid.x + px, mid.y + py);
    let b = normal_depth_at(mid.x - px, mid.y - py);
    if (m.w < 0.0 || a.w < 0.0 || b.w < 0.0) {
        return 1.0;
    }
    if (m.w >= INF || a.w >= INF || b.w >= INF) {
        // One side is background: a silhouette, always drawn.
        return 1.0;
    }
    let jump = 0.01 * G.scene_lo.w;
    if (abs(a.w + b.w - 2.0 * m.w) > jump) {
        return 1.0;
    }
    let cos_thr = cos(G.viewport.w * PI / 180.0);
    return sstep(cos_thr + 0.05, cos_thr - 0.05, dot(a.xyz, b.xyz));
}

@fragment
fn fs_edge(v: SegOut) -> @location(0) f32 {
    let fx = v.clip.x;
    let fy = v.clip.y;
    let d = v.b.xy - v.a.xy;
    let len2 = dot(d, d);
    var t = 0.0;
    if (len2 > 1e-12) {
        t = clamp(((fx - v.a.x) * d.x + (fy - v.a.y) * d.y) / len2, 0.0, 1.0);
    }
    let q = v.a.xy + d * t;
    let dist = length(vec2<f32>(fx, fy) - q);
    var alpha = v.alpha;
    if ((v.flags & 1u) != 0u) {
        let mid = (v.a.xy + v.b.xy) * 0.5;
        alpha = alpha * estimator_weight(mid, d);
    }
    let cov = clamp(v.hw + 0.5 - dist, 0.0, 1.0) * alpha;
    if (cov <= 0.0) {
        discard;
    }
    let z = v.a.z + (v.b.z - v.a.z) * t;
    let vis = visibility(vec2<i32>(i32(fx), i32(fy)), z);
    return cov * vis;
}

// ---------------------------------------------------------------------------
// Screen-space silhouettes (`lines::silhouettes`).

fn item_at(c: vec2<i32>) -> u32 {
    return textureLoad(g_ids, c, 0).x;
}

@fragment
fn fs_silhouette(in: FsOut) -> @location(0) f32 {
    let c = vec2<i32>(i32(in.pos.x), i32(in.pos.y));
    let d = depth_at(c, 0);
    if (d >= INF) {
        discard;
    }
    // A feature edge already draws this line.
    var drawn = textureLoad(ink_src, c, 0).x > 0.15;
    let steps = array<vec2<i32>, 4>(
        vec2<i32>(-1, 0), vec2<i32>(1, 0), vec2<i32>(0, -1), vec2<i32>(0, 1),
    );
    for (var i = 0; i < 4; i = i + 1) {
        let q = c + steps[i];
        if (in_bounds(q) && textureLoad(ink_src, q, 0).x > 0.15) {
            drawn = true;
        }
    }
    if (drawn) {
        discard;
    }
    let floor_d = 0.02 * G.scene_lo.w;
    var cov = 0.0;
    for (var axis = 0; axis < 2; axis = axis + 1) {
        var step = vec2<i32>(1, 0);
        if (axis == 1) {
            step = vec2<i32>(0, 1);
        }
        for (var s = 0; s < 2; s = s + 1) {
            var dir = step;
            if (s == 1) {
                dir = -step;
            }
            let j = c + dir;
            if (!in_bounds(j)) {
                continue;
            }
            let dn = depth_at(j, 0);
            var v = 0.0;
            if (dn >= INF) {
                v = 1.0;
            } else if (dn < d) {
                // The neighbour is nearer; it owns the outline.
                v = 0.0;
            } else if (item_at(c) != item_at(j) && item_at(c) != NO_ITEM && item_at(j) != NO_ITEM) {
                v = 1.0;
            } else {
                // Slope on the opposite side, as the expectation for a
                // continuous surface.
                var slope = 0.0;
                let back = c - dir;
                if (in_bounds(back)) {
                    let db = depth_at(back, 0);
                    if (db < INF) {
                        slope = abs(d - db);
                    }
                }
                let thr = max(floor_d, 4.0 * slope);
                v = sstep(thr, thr * 2.0, dn - d);
            }
            cov = max(cov, v);
        }
    }
    if (cov <= 0.0) {
        discard;
    }
    return cov * ALPHA_SILHOUETTE * visibility(c, d);
}

// ---------------------------------------------------------------------------
// Compositing one finished ink layer, premultiplied.

@fragment
fn fs_composite(in: FsOut) -> @location(0) vec4<f32> {
    let c = vec2<i32>(i32(in.pos.x), i32(in.pos.y));
    let a = clamp(textureLoad(ink_src, c, 0).x, 0.0, 1.0);
    if (a <= 0.0) {
        discard;
    }
    return vec4<f32>(OV.color.xyz * a, a);
}
