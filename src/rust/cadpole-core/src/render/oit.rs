//! Order-independent translucency: an exact per-pixel sorted A-buffer.
//!
//! Translucent items (`Item::is_translucent`) are skipped by the opaque
//! geometry pass and rasterized here, after shading, with the same triangle
//! inner loop the opaque pass uses. Every fragment that passes the opaque
//! depth test (no depth write) is shaded and captured in a per-pixel linked
//! list; the list is then sorted back-to-front and composited over the
//! already-shaded `Frame::hdr`. Because the sort is by depth, the result does
//! not depend on the order the items were submitted in.
//!
//! Memory is bounded twice over: at most [`CAP`] fragments per pixel, and at
//! most `NODE_BUDGET_PER_PIXEL` fragments per pixel on average across the
//! frame. A fragment that does not fit is not dropped: it is merged into the
//! farthest node of its pixel (the exact composite of the two, in depth
//! order), so energy and the accumulated transmittance are preserved and the
//! failure mode is a loss of ordering among the farthest layers rather than a
//! pop. The bound is 21 B/px plus 32 B per captured fragment.
//!
//! Determinism: capture is sequential in (item, triangle, raster) order and
//! the resolve is a pure function of the captured lists, so the output is
//! byte-identical for any thread count.

use super::aa::for_each_fragment;
use super::shading::{self, Lights, Surface};
use super::{COV_TRANSLUCENT, Ctx, Frame, NO_FACE, NO_ITEM, add, dot, lerp3, luminance, mul, normalize, scale, sub};

/// Maximum layers kept per pixel. Deeper stacks merge into the farthest
/// layer (see the module comment).
pub(crate) const CAP: usize = 16;

/// Fragments per pixel the node arena may grow to on average, so that a
/// pathological scene cannot exhaust memory: 4 x 32 B = 128 B/px.
const NODE_BUDGET_PER_PIXEL: usize = 4;

const NONE: u32 = u32::MAX;

/// One captured translucent fragment.
#[derive(Clone, Copy)]
struct Node {
    /// View-space depth along `fwd`.
    z: f32,
    /// Radiance this layer adds, already weighted by its alpha.
    rgb: [f32; 3],
    /// What this layer passes through from behind: `(1 - alpha) * tint`.
    t: [f32; 3],
    next: u32,
}

pub(crate) struct Buffers {
    /// Per pixel; 1 = nothing translucent in front of the opaque surface.
    pub transmittance: Vec<[f32; 3]>,
    /// Nearest translucent layer depth; INF = none.
    pub near_z: Vec<f32>,
    /// Head of each pixel's fragment list; `NONE` = empty.
    heads: Vec<u32>,
    /// Fragments captured per pixel, saturating at [`CAP`].
    counts: Vec<u8>,
    /// Fragment arena, appended in capture order.
    nodes: Vec<Node>,
    /// Hard cap on `nodes.len()`.
    budget: usize,
}

impl Buffers {
    pub(crate) fn new(w: u32, h: u32) -> Buffers {
        let n = (w as usize) * (h as usize);
        Buffers {
            transmittance: vec![[1.0; 3]; n],
            near_z: vec![f32::INFINITY; n],
            heads: vec![NONE; n],
            counts: vec![0; n],
            nodes: Vec::new(),
            budget: n.saturating_mul(NODE_BUDGET_PER_PIXEL),
        }
    }

    pub(crate) fn clear(&mut self) {
        self.transmittance.fill([1.0; 3]);
        self.near_z.fill(f32::INFINITY);
        self.heads.fill(NONE);
        self.counts.fill(0);
        self.nodes.clear();
    }

    /// True when no translucent fragment was captured this pass.
    pub(crate) fn is_empty(&self) -> bool {
        self.nodes.is_empty()
    }

    /// Appends a fragment to pixel `idx`, or merges it into the pixel's
    /// farthest layer when the per-pixel cap or the arena budget is reached.
    fn push(&mut self, idx: usize, node: Node) {
        if (self.counts[idx] as usize) < CAP && self.nodes.len() < self.budget {
            let at = self.nodes.len() as u32;
            self.nodes.push(Node { next: self.heads[idx], ..node });
            self.heads[idx] = at;
            self.counts[idx] += 1;
            return;
        }
        // Merge into the farthest layer, keeping both in depth order:
        // c = near.rgb + near.t * (far.rgb + far.t * c_behind).
        let mut far = NONE;
        let mut cur = self.heads[idx];
        while cur != NONE {
            if far == NONE || self.nodes[cur as usize].z > self.nodes[far as usize].z {
                far = cur;
            }
            cur = self.nodes[cur as usize].next;
        }
        if far == NONE {
            return;
        }
        let old = self.nodes[far as usize];
        let (near, back) = if node.z < old.z { (node, old) } else { (old, node) };
        self.nodes[far as usize] =
            Node { z: near.z, rgb: add(near.rgb, mul(near.t, back.rgb)), t: mul(near.t, back.t), next: old.next };
    }
}

/// Rasterizes translucent items with [`for_each_fragment`], depth-tests
/// against `frame.depth` (no write), shades with `shading::shade_surface`,
/// sorts per pixel and composites into `frame.hdr`, ORs `COV_TRANSLUCENT`
/// into coverage and fills `buf.transmittance` / `buf.near_z`.
pub(crate) fn translucent_pass(frame: &mut Frame, ctx: &Ctx<'_>, buf: &mut Buffers) {
    buf.clear();
    if !ctx.items.iter().any(|it| it.is_translucent()) {
        return;
    }
    let lights = Lights::studio();
    let (w, h) = (frame.w, frame.h);
    let ortho = ctx.view.is_orthographic();
    let fwd = ctx.view.fwd;
    let eye = ctx.view.eye;
    {
        let depth = &frame.depth;
        for (ii, item) in ctx.items.iter().enumerate() {
            if !item.is_translucent() || ii >= NO_ITEM as usize {
                continue;
            }
            let m = item.mesh;
            let opacity = item.opacity() as f32;
            // What the layer lets through: white glass is neutral, coloured
            // glass filters, and the filtering grows as the material clears.
            let base = item.material.base_color.map(|c| c as f32);
            let tint = lerp3([1.0; 3], base, 1.0 - opacity);
            let vertex = |i: u32| {
                let i = i as usize * 3;
                [m.positions[i], m.positions[i + 1], m.positions[i + 2]]
            };
            let vnormal = |i: u32| {
                let i = i as usize * 3;
                if i + 2 < m.normals.len() { [m.normals[i], m.normals[i + 1], m.normals[i + 2]] } else { [0.0, 0.0, 1.0] }
            };
            for (t, tri) in m.indices.chunks_exact(3).enumerate() {
                let face = m.face_ids.get(t).copied().unwrap_or(NO_FACE);
                let p = [vertex(tri[0]), vertex(tri[1]), vertex(tri[2])];
                let n = [vnormal(tri[0]), vnormal(tri[1]), vnormal(tri[2])];
                for_each_fragment(ctx.view, w, h, p, |x, y, z, b| {
                    let idx = (y as usize) * (w as usize) + x as usize;
                    if !(z < depth[idx]) {
                        return;
                    }
                    let pos = add(add(scale(p[0], b[0]), scale(p[1], b[1])), scale(p[2], b[2]));
                    let mut nrm = normalize(add(add(scale(n[0], b[0]), scale(n[1], b[1])), scale(n[2], b[2])));
                    // Both faces are drawn; the normal is flipped toward the
                    // viewer exactly as the opaque pass does.
                    let facing = if ortho { -dot(nrm, fwd) } else { dot(nrm, sub(eye, pos)) };
                    if facing < 0.0 {
                        nrm = scale(nrm, -1.0);
                    }
                    let s = shading::surface_for(ctx, ii as u16, face, t as u32, pos, nrm, 1.0);
                    // Glancing angles turn reflective even on nearly clear
                    // glass, so alpha is the larger of opacity and Fresnel.
                    let ndv = dot(s.n, s.v).clamp(0.0, 1.0);
                    let a = opacity.max(luminance(shading::fresnel_schlick(s.f0, ndv))).clamp(0.0, 1.0);
                    let total = shading::shade_surface(&s, &lights);
                    let spec = shading::shade_surface(
                        &Surface {
                            pos: s.pos,
                            n: s.n,
                            v: s.v,
                            albedo: [0.0; 3],
                            roughness: s.roughness,
                            metallic: s.metallic,
                            f0: s.f0,
                            emissive: [0.0; 3],
                            ao: s.ao,
                            sun_vis: s.sun_vis,
                        },
                        &lights,
                    );
                    // Body colour fades out with opacity; reflections do not.
                    let body = sub(total, spec).map(|v| v.max(0.0));
                    let rgb = add(scale(body, opacity), spec);
                    buf.push(idx, Node { z, rgb, t: scale(tint, 1.0 - a), next: NONE });
                });
            }
        }
    }
    if buf.is_empty() {
        return;
    }
    resolve(frame, buf);
}

/// Sorts each pixel's list back to front and composites it over `frame.hdr`.
fn resolve(frame: &mut Frame, buf: &mut Buffers) {
    let mut order = [0u32; CAP];
    for idx in 0..buf.heads.len() {
        let head = buf.heads[idx];
        if head == NONE {
            continue;
        }
        // Walk the list (newest first) back into capture order, then sort by
        // depth descending with a stable insertion sort: equal depths keep
        // capture order, everything else is order-independent.
        let mut n = 0usize;
        let mut cur = head;
        while cur != NONE && n < CAP {
            order[n] = cur;
            n += 1;
            cur = buf.nodes[cur as usize].next;
        }
        order[..n].reverse();
        for i in 1..n {
            let k = order[i];
            let kz = buf.nodes[k as usize].z;
            let mut j = i;
            while j > 0 && buf.nodes[order[j - 1] as usize].z < kz {
                order[j] = order[j - 1];
                j -= 1;
            }
            order[j] = k;
        }
        let mut c = frame.hdr[idx];
        let mut trans = [1.0f32; 3];
        let mut near = f32::INFINITY;
        for &oi in &order[..n] {
            let node = buf.nodes[oi as usize];
            c = add(mul(c, node.t), node.rgb);
            trans = mul(trans, node.t);
            near = near.min(node.z);
        }
        frame.hdr[idx] = c;
        frame.coverage[idx] |= COV_TRANSLUCENT;
        buf.transmittance[idx] = trans;
        buf.near_z[idx] = near;
    }
}

/// Ink attenuation for an overlay element at depth `z`: the product of the
/// transmittances of the translucent layers strictly in front of it, so an
/// edge behind glass is dimmed and tinted but still visible, while an edge on
/// the glass itself (or in front of it) is untouched.
pub(crate) fn transmittance_at(buf: &Buffers, idx: usize, z: f32) -> [f32; 3] {
    let mut cur = match buf.heads.get(idx) {
        Some(&h) => h,
        None => return [1.0; 3],
    };
    if cur == NONE {
        return [1.0; 3];
    }
    // Layers within a thousandth of the query depth count as coplanar with it.
    let limit = z - 1.0e-3 * z.abs().max(1.0);
    let mut t = [1.0f32; 3];
    while cur != NONE {
        let node = buf.nodes[cur as usize];
        if node.z < limit {
            t = mul(t, node.t);
        }
        cur = node.next;
    }
    t
}

#[cfg(test)]
mod tests {
    use super::super::testing::*;
    use super::super::*;
    use crate::context::Material;
    use crate::geom::Vec3;
    use crate::tessellate::Mesh;

    /// A quad in the XY plane at height `z`, facing +Z, spanning +-8.
    fn layer(z: f32) -> Mesh {
        let p = [[-8.0, -8.0, z], [8.0, -8.0, z], [8.0, 8.0, z], [-8.0, 8.0, z]];
        let mut m = Mesh { positions: Vec::new(), normals: Vec::new(), indices: Vec::new(), face_ids: Vec::new(), colors: Vec::new() };
        for v in p {
            m.positions.extend_from_slice(&v);
            m.normals.extend_from_slice(&[0.0, 0.0, 1.0]);
        }
        m.indices.extend_from_slice(&[0, 1, 2, 0, 2, 3]);
        m.face_ids.extend_from_slice(&[0, 0]);
        m
    }

    /// Orthographic top view, so every layer shades identically (the eye
    /// vector does not depend on position) and depth is the layer height.
    fn top_camera() -> Camera {
        let mut cam = Camera::preset("top", Vec3::new(-8.0, -8.0, 0.0), Vec3::new(8.0, 8.0, 8.0), 0.1);
        cam.orthographic = true;
        cam
    }

    fn opts(bg: [f32; 3]) -> RenderOptions {
        RenderOptions {
            width: 32,
            height: 32,
            background: bg,
            tonemap: super::super::ToneMap::None,
            draw_edges: false,
            draw_grid: false,
            draw_silhouettes: false,
            samples: 1,
            threads: 1,
            // These tests are about depth and compositing. Ambient occlusion
            // and the contact shadow legitimately react to geometry hidden
            // behind an opaque wall, which would mask what is being checked.
            ambient_occlusion: false,
            contact_shadow: false,
            ..Default::default()
        }
    }

    fn stack(n: usize) -> Vec<Mesh> {
        (0..n).map(|i| layer(1.0 + i as f32)).collect()
    }

    fn items<'a>(meshes: &'a [Mesh], mat: &'a Material, opacity: f64) -> Vec<Item<'a>> {
        meshes.iter().map(|m| Item { opacity: Some(opacity), ..Item::new(m, mat, &[]) }).collect()
    }

    fn centre_hdr(meshes: &[Mesh], mat: &Material, opacity: f64, bg: [f32; 3]) -> [f32; 3] {
        let its = items(meshes, mat, opacity);
        let f = render_frame(&its, &top_camera(), &opts(bg));
        f.hdr[f.idx(16, 16)]
    }

    /// The composite must be exactly `background * t + rgb` per layer. `t`
    /// and `rgb` are recovered by rendering one layer over two different
    /// backgrounds (two equations, two unknowns); the three-layer stack is
    /// then predicted analytically and compared with the render.
    #[test]
    fn stacked_layers_match_an_analytic_alpha_composite() {
        let opacity = 0.5;
        let mat = Material { base_color: [0.35, 0.55, 0.8], ..Material::default() };
        let one = stack(1);
        let b1 = [0.1, 0.2, 0.3];
        let b2 = [0.7, 0.6, 0.5];
        let c1 = centre_hdr(&one, &mat, opacity, b1);
        let c2 = centre_hdr(&one, &mat, opacity, b2);
        let mut t = [0.0f32; 3];
        let mut r = [0.0f32; 3];
        for k in 0..3 {
            t[k] = (c1[k] - c2[k]) / (b1[k] - b2[k]);
            r[k] = c1[k] - b1[k] * t[k];
            assert!(t[k] > 0.05 && t[k] < 0.95, "channel {k}: transmittance {} is not glassy", t[k]);
        }
        let three = stack(3);
        let got = centre_hdr(&three, &mat, opacity, b1);
        for k in 0..3 {
            let want = b1[k] * t[k] * t[k] * t[k] + r[k] * (1.0 + t[k] + t[k] * t[k]);
            assert!((got[k] - want).abs() < 1e-4, "channel {k}: got {} want {}", got[k], want);
        }
    }

    /// Three layers of different colours at different depths must give the
    /// same image whatever order they are submitted in.
    #[test]
    fn output_is_independent_of_item_order() {
        let mats = [
            Material { base_color: [0.9, 0.2, 0.2], ..Material::default() },
            Material { base_color: [0.2, 0.9, 0.2], ..Material::default() },
            Material { base_color: [0.2, 0.2, 0.9], ..Material::default() },
        ];
        let meshes = [layer(1.0), layer(3.0), layer(5.0)];
        let mut hashes = Vec::new();
        for perm in [[0, 1, 2], [0, 2, 1], [1, 0, 2], [1, 2, 0], [2, 0, 1], [2, 1, 0]] {
            let its: Vec<Item<'_>> =
                perm.iter().map(|&i| Item { opacity: Some(0.45), ..Item::new(&meshes[i], &mats[i], &[]) }).collect();
            let img = render(&its, &top_camera(), &opts([0.13, 0.14, 0.16]));
            hashes.push(fnv1a64(&img.rgba));
        }
        assert!(hashes.windows(2).all(|w| w[0] == w[1]), "order changed the image: {hashes:?}");
    }

    /// Translucent fragments are depth-tested against the opaque buffer but
    /// never write it: glass behind an opaque surface changes nothing, glass
    /// in front changes what it covers.
    #[test]
    fn translucent_fragments_respect_opaque_depth_without_writing_it() {
        let mat = Material::default();
        let wall = layer(3.0);
        let behind = layer(1.0);
        let front = layer(5.0);
        let cam = top_camera();
        let o = opts([0.13, 0.14, 0.16]);
        let solid = [Item::new(&wall, &mat, &[])];
        let base = render(&solid, &cam, &o);
        let with_behind = render(
            &[Item::new(&wall, &mat, &[]), Item { opacity: Some(0.4), ..Item::new(&behind, &mat, &[]) }],
            &cam,
            &o,
        );
        let with_front =
            render(&[Item::new(&wall, &mat, &[]), Item { opacity: Some(0.4), ..Item::new(&front, &mat, &[]) }], &cam, &o);
        assert_eq!(fnv1a64(&base.rgba), fnv1a64(&with_behind.rgba), "glass behind an opaque wall changed the image");
        assert_ne!(fnv1a64(&base.rgba), fnv1a64(&with_front.rgba), "glass in front of the wall changed nothing");
        let its = [Item::new(&wall, &mat, &[]), Item { opacity: Some(0.4), ..Item::new(&front, &mat, &[]) }];
        let f = render_frame(&its, &cam, &o);
        let g = render_frame(&solid, &cam, &o);
        let i = f.idx(16, 16);
        assert_eq!(f.depth[i], g.depth[i], "the translucent pass wrote the depth buffer");
        assert_ne!(f.coverage[i] & COV_TRANSLUCENT, 0);
    }

    /// Far more layers than the per-pixel cap must still render, stay
    /// deterministic and land on the analytic limit of the stack: with
    /// `t` around 0.25 the nearest [`CAP`] layers already carry everything
    /// above 1e-9, so the merged tail cannot be seen.
    #[test]
    fn the_fragment_cap_degrades_gracefully() {
        let opacity = 0.5;
        let mat = Material { base_color: [0.4, 0.6, 0.9], ..Material::default() };
        let bg = [0.1, 0.2, 0.3];
        let c1 = centre_hdr(&stack(1), &mat, opacity, bg);
        let c2 = centre_hdr(&stack(2), &mat, opacity, bg);
        // c1 = B t + r and c2 = B t^2 + r (1 + t)  =>  t = (c2 - c1)/(c1 - B).
        let mut t = [0.0f32; 3];
        let mut r = [0.0f32; 3];
        for k in 0..3 {
            t[k] = (c2[k] - c1[k]) / (c1[k] - bg[k]);
            r[k] = c1[k] - bg[k] * t[k];
        }
        let many = stack(100);
        let deep = centre_hdr(&many, &mat, opacity, bg);
        let again = centre_hdr(&many, &mat, opacity, bg);
        assert_eq!(format!("{deep:?}"), format!("{again:?}"), "deep stacks must be deterministic");
        let reversed: Vec<Mesh> = many.iter().rev().cloned().collect();
        let other_order = centre_hdr(&reversed, &mat, opacity, bg);
        for k in 0..3 {
            let want = r[k] / (1.0 - t[k]);
            assert!((deep[k] - want).abs() < 2.0 / 255.0, "channel {k}: got {} want {}", deep[k], want);
            assert!((other_order[k] - want).abs() < 2.0 / 255.0, "reversed channel {k}: {}", other_order[k]);
        }
    }

    /// An all-opaque scene must be untouched by the translucency pass.
    #[test]
    fn opaque_scenes_leave_the_oit_buffers_pristine() {
        let (mesh, edges) = unit_box();
        let mat = Material::default();
        let (lo, hi) = mesh.bbox().unwrap();
        let cam = Camera::preset("iso", lo, hi, 0.2);
        let its = [Item::new(&mesh, &mat, &edges)];
        let f = render_frame(&its, &cam, &RenderOptions { width: 64, height: 48, ..Default::default() });
        assert!(f.coverage.iter().all(|c| c & COV_TRANSLUCENT == 0));
    }

    /// A body behind a translucent one shows through it in its own colour:
    /// a red or a green layer under the same `material(opacity=0.42)` slab
    /// gives a red or a green pixel, and the slab made opaque hides both.
    /// The material's own opacity, not the x-ray override, drives this.
    #[test]
    fn a_body_behind_a_translucent_one_is_visible() {
        let behind = layer(1.0);
        let front = layer(3.0);
        let cam = top_camera();
        let o = opts([0.13, 0.14, 0.16]);
        let red = Material { base_color: [0.9, 0.08, 0.08], ..Material::default() };
        let green = Material { base_color: [0.08, 0.9, 0.08], ..Material::default() };
        let centre = |under: &Material, opacity: f64| {
            let shell = Material { base_color: [0.36, 0.78, 0.73], opacity, ..Material::default() };
            let its = [Item::new(&behind, under, &[]), Item::new(&front, &shell, &[])];
            let f = render_frame(&its, &cam, &o);
            f.hdr[f.idx(16, 16)]
        };
        let r = centre(&red, 0.42);
        let g = centre(&green, 0.42);
        assert!(r[0] > r[1] + 0.05, "red behind the shell does not read red: {r:?}");
        assert!(g[1] > g[0] + 0.05, "green behind the shell does not read green: {g:?}");
        assert!(r[0] - g[0] > 0.1 && g[1] - r[1] > 0.1, "the body behind changed too little: {r:?} vs {g:?}");
        assert_eq!(centre(&red, 1.0), centre(&green, 1.0), "an opaque shell let the body behind show");
    }

    /// Thread count must not change a translucent render.
    #[test]
    fn translucency_is_thread_invariant() {
        let mat = Material { base_color: [0.3, 0.7, 0.9], ..Material::default() };
        let meshes = stack(4);
        let its = items(&meshes, &mat, 0.35);
        let one = render(&its, &top_camera(), &RenderOptions { threads: 1, ..opts([0.13, 0.14, 0.16]) });
        let four = render(&its, &top_camera(), &RenderOptions { threads: 4, ..opts([0.13, 0.14, 0.16]) });
        assert_eq!(fnv1a64(&one.rgba), fnv1a64(&four.rgba));
    }
}
