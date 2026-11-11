// Deferred shading, tonemap and multisample resolve in one pass: the GPU's
// version of `shading::shade` + `occlusion::ground_contact` + `aa::tonemap` +
// `aa::accumulate`.
//
// The CPU renderer runs its whole pipeline once per anti-aliasing sample and
// averages the *display-linear* results (never in sRGB). This does the same
// thing in one pass: every multisample of the G-buffer is shaded and
// tonemapped on its own, and the average is written to a single-sample
// display-linear texture. Pixel centres, not the CPU's camera jitter, are what
// moves between samples -- hardware multisampling puts the coverage in the
// same place.

@group(1) @binding(0) var g_depth: texture_depth_multisampled_2d;
@group(1) @binding(1) var g_normal: texture_multisampled_2d<f32>;
@group(1) @binding(2) var g_ids: texture_multisampled_2d<u32>;
@group(1) @binding(3) var ao_tex: texture_2d<f32>;
// Baked ground contact shadow, `occlusion::Heightfield::shadow`.
@group(1) @binding(4) var contact: texture_2d<f32>;
// The nearest translucent layer, if any.
@group(1) @binding(5) var t_depth: texture_depth_multisampled_2d;
@group(1) @binding(6) var t_normal: texture_multisampled_2d<f32>;
@group(1) @binding(7) var t_ids: texture_multisampled_2d<u32>;

// `Heightfield::ground_at`: bilinear over the baked shadow grid, exactly 1
// outside it.
fn ground_at(x: f32, y: f32) -> f32 {
    if (G.hf2.z < 0.5) {
        return 1.0;
    }
    let size = i32(G.hf2.x);
    let fx = (x - G.hf.x) / G.hf.z - 0.5;
    let fy = (y - G.hf.y) / G.hf.w - 0.5;
    let ix = i32(floor(fx));
    let iy = i32(floor(fy));
    let tx = fx - floor(fx);
    let ty = fy - floor(fy);
    var s: array<f32, 4>;
    for (var k = 0; k < 4; k = k + 1) {
        let qx = ix + (k & 1);
        let qy = iy + (k >> 1);
        if (qx < 0 || qy < 0 || qx >= size || qy >= size) {
            s[k] = 1.0;
        } else {
            s[k] = textureLoad(contact, vec2<i32>(qx, qy), 0).x;
        }
    }
    let a = s[0] * (1.0 - tx) + s[1] * tx;
    let b = s[2] * (1.0 - tx) + s[3] * tx;
    return a * (1.0 - ty) + b * ty;
}

// Closed-form inverse of the ACES fit, so anything composited over the
// background lands exactly on it after the tonemap (`aa::inverse_tonemap_rgb`).
fn inv_aces(y0: f32) -> f32 {
    let y = clamp(y0, 0.0, 1.0);
    if (y <= 0.0) {
        return 0.0;
    }
    let a = 2.51 - 2.43 * y;
    let b = 0.03 - 0.59 * y;
    let c = -0.14 * y;
    let disc = max(b * b - 4.0 * a * c, 0.0);
    return max((-b + sqrt(disc)) / (2.0 * a), 0.0);
}

fn inverse_tonemap(d: vec3<f32>, exposure: f32) -> vec3<f32> {
    let e = max(exposure, 1e-6);
    return vec3<f32>(inv_aces(d.x) / e, inv_aces(d.y) / e, inv_aces(d.z) / e);
}

// Closed-form inverse of Neutral (`aa::inv_neutral`): the compressed peak
// is the output's maximum, the original peak follows from the rational
// curve, the mix toward white is undone with the same `g`, and the
// black-point offset is recovered from the minimum channel.
fn inv_neutral(d: vec3<f32>) -> vec3<f32> {
    let o = clamp(d, vec3<f32>(0.0), vec3<f32>(1.0));
    let np = max(o.x, max(o.y, o.z));
    var c = o;
    if (np >= 0.76) {
        let dd = 0.24;
        let npc = min(np, 1.0 - 1e-6);
        let p = dd * dd / (1.0 - npc) - dd + 0.76;
        let g = 1.0 - 1.0 / (0.15 * (p - npc) + 1.0);
        c = max((o - vec3<f32>(npc * g)) / (1.0 - g), vec3<f32>(0.0)) * (p / npc);
    }
    let mc = min(c.x, min(c.y, c.z));
    let x = sqrt(mc / 6.25);
    let offset = select(0.04, x - mc, mc < 0.04);
    return c + vec3<f32>(offset);
}

fn inverse_tonemap_mode(d: vec3<f32>, exposure: f32, mode: u32) -> vec3<f32> {
    if (mode == 2u) {
        return inv_neutral(d) / max(exposure, 1e-6);
    }
    if (mode == 1u) {
        return inverse_tonemap(d, exposure);
    }
    return d;
}

fn face_of(ids: vec4<u32>) -> u32 {
    return ids.y | (ids.z << 16u);
}

// One layer of order-independent translucency (`render/oit.rs`): the nearest
// translucent fragment in front of the opaque surface, composited with the
// same alpha, tint and body/reflection split. The CPU keeps a per-pixel list;
// this keeps only the front layer, so a hollow translucent body shows one wall
// rather than two.
fn composite_glass(c: vec2<i32>, si: i32, behind: vec3<f32>) -> vec3<f32> {
    let ids = textureLoad(t_ids, c, si);
    let item = ids.x;
    let z = view_z(textureLoad(t_depth, c, si));
    let pos = unproject(f32(c.x) + 0.5, f32(c.y) + 0.5, z);
    let n = normalize(textureLoad(t_normal, c, si).xyz);
    let m = materials[item];
    let opacity = clamp(m.params.z, 0.0, 1.0);
    // What the layer lets through: white glass is neutral, coloured glass
    // filters, and the filtering grows as the material clears.
    let tint = mix(vec3<f32>(1.0), clamp(m.base.xyz, vec3<f32>(0.0), vec3<f32>(1.0)), 1.0 - opacity);
    let s = surface_for(item, face_of(ids), ids.w != 0u, pos, n, 1.0);
    let ndv = clamp(dot(s.n, s.v), 0.0, 1.0);
    let a = clamp(max(opacity, luminance(fresnel_schlick(s.f0, ndv))), 0.0, 1.0);
    let total = shade_surface(s);
    var only_spec = s;
    only_spec.albedo = vec3<f32>(0.0);
    only_spec.emissive = vec3<f32>(0.0);
    let spec = shade_surface(only_spec);
    let body = max(total - spec, vec3<f32>(0.0));
    let rgb = body * opacity + spec;
    return behind * (tint * (1.0 - a)) + rgb;
}

@fragment
fn fs_shade(in: FsOut) -> @location(0) vec4<f32> {
    let c = vec2<i32>(i32(in.pos.x), i32(in.pos.y));
    let samples = G.counts.x;
    let exposure = G.center.w;
    let tm_mode = u32(G.background.w + 0.5);
    let bg = G.background.xyz;
    let ao_base = textureLoad(ao_tex, c, 0).x;

    // The ground contact shadow of this pixel's background ray, computed once:
    // it is a function of the plane hit, which the sub-pixel samples share to
    // within a fraction of a pixel.
    var ground = 1.0;
    if (G.hf2.z > 0.5) {
        var o = G.eye.xyz;
        var d = normalize(unproject(in.pos.x, in.pos.y, 1.0) - G.eye.xyz);
        if (is_ortho()) {
            o = unproject(in.pos.x, in.pos.y, 0.0);
            d = G.fwd.xyz;
        }
        if (d.z < -1e-6) {
            let t = (G.scene_hi.w - o.z) / d.z;
            if (t > 0.0) {
                let hit = o + d * t;
                let far = 4.0 * G.scene_lo.w;
                let dx = hit.x - G.center.x;
                let dy = hit.y - G.center.y;
                if (dx * dx + dy * dy <= far * far) {
                    ground = ground_at(hit.x, hit.y);
                }
            }
        }
    }
    let bg_display = bg * ground;

    var acc = vec3<f32>(0.0);
    var covered = 0.0;
    for (var s = 0u; s < samples; s = s + 1u) {
        let si = i32(s);
        let ids = textureLoad(g_ids, c, si);
        let item = ids.x;
        let opaque_z = view_z(textureLoad(g_depth, c, si));
        let tids = textureLoad(t_ids, c, si);
        // The translucent pass has its own depth buffer, so a glass fragment
        // behind the opaque surface is rejected here instead -- the CPU's
        // `if !(z < depth[idx]) { return; }`.
        let has_glass = tids.x != NO_ITEM && view_z(textureLoad(t_depth, c, si)) < opaque_z;
        var display: vec3<f32>;
        if (item == NO_ITEM && !has_glass) {
            display = bg_display;
            if (ground < 1.0) {
                covered = covered + 1.0;
            }
        } else {
            var hdr: vec3<f32>;
            if (item == NO_ITEM) {
                hdr = inverse_tonemap_mode(bg_display, exposure, tm_mode);
            } else {
                let pos = unproject(in.pos.x, in.pos.y, opaque_z);
                let n = normalize(textureLoad(g_normal, c, si).xyz);
                var ao = ao_base;
                // Geometry close to the ground picks up a little of its own
                // contact shadow (occlusion::ground_contact).
                if (G.hf2.z > 0.5) {
                    let w = 0.35 * sstep(G.hf2.w, 0.0, pos.z - G.scene_hi.w);
                    if (w > 0.0) {
                        ao = ao * (1.0 + (ground_at(pos.x, pos.y) - 1.0) * w);
                    }
                }
                let surf = surface_for(item, face_of(ids), ids.w != 0u, pos, n, ao);
                hdr = shade_surface(surf);
            }
            if (has_glass) {
                hdr = composite_glass(c, si, hdr);
            }
            display = tonemap_mode(hdr, exposure, tm_mode);
            covered = covered + 1.0;
        }
        acc = acc + display;
    }
    return vec4<f32>(acc / f32(samples), covered / f32(samples));
}
