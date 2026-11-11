//! Turning `cadpole_core::render::Item`s into GPU buffers: de-indexed vertices
//! that carry their triangle's face id, one material per item, the screen-space
//! segments the line overlay draws, and the top-down heightfield the ground
//! contact shadow is baked from.
//!
//! Everything here is plain CPU data derived only from the scene and the
//! camera, so it is deterministic and independent of the device.

use bytemuck::{Pod, Zeroable};
use cadpole_core::geom::Vec3;
use cadpole_core::render::{Camera, EdgeMode, Item, RenderOptions, View};

/// `Frame::item` value for "no item", matching the CPU renderer.
pub const NO_ITEM: u32 = 0xffff;

// ---------------------------------------------------------------------------
// Vertices and materials.

#[repr(C)]
#[derive(Copy, Clone, Pod, Zeroable, Debug)]
pub struct Vertex {
    pub pos: [f32; 3],
    pub nrm: [f32; 3],
    /// Topology face id of the triangle this vertex was expanded from.
    pub face: u32,
    /// Item index in the low 16 bits; bit 16 marks a highlighted face.
    pub item_flags: u32,
}

#[repr(C)]
#[derive(Copy, Clone, Pod, Zeroable, Debug)]
pub struct GpuMaterial {
    /// base colour rgb, w = roughness
    pub base: [f32; 4],
    /// metallic, texture scale, opacity, ior
    pub params: [f32; 4],
    /// emissive rgb, w = texture kind
    pub emissive: [f32; 4],
}

/// One screen-space overlay segment (a round-capped capsule).
#[repr(C)]
#[derive(Copy, Clone, Pod, Zeroable, Debug)]
pub struct Seg {
    /// screen x, y and view depth of the first end
    pub a: [f32; 3],
    pub hw: f32,
    pub b: [f32; 3],
    pub alpha: f32,
    /// bit 0: estimate the crease weight from the G-buffer normals
    pub flags: u32,
    pub item: u32,
    /// id written by the pick pass
    pub pick: u32,
    pub _pad: u32,
}

/// Bit set in a pick id to mark an edge, so edges can win over faces.
pub const PICK_EDGE: u32 = 0x8000_0000;

fn texture_kind(name: &str) -> f32 {
    match cadpole_core::render::texture_kind(name) {
        Some(cadpole_core::render::TextureKind::Checker) => 1.0,
        Some(cadpole_core::render::TextureKind::Stripes) => 2.0,
        Some(cadpole_core::render::TextureKind::Noise) => 3.0,
        Some(cadpole_core::render::TextureKind::Wood) => 4.0,
        Some(cadpole_core::render::TextureKind::Dots) => 5.0,
        _ => 0.0,
    }
}

/// The CPU-side scene: everything the GPU passes need, in upload order.
pub struct Upload {
    pub opaque: Vec<Vertex>,
    pub translucent: Vec<Vertex>,
    pub materials: Vec<GpuMaterial>,
    /// Feature-edge segments (creases), already filtered and weighted.
    pub creases: Vec<Seg>,
    /// Selection highlight segments.
    pub highlights: Vec<Seg>,
    /// Every edge segment, for the pick pass (fat lines, no crease filter).
    pub pick_edges: Vec<Seg>,
    pub scene: SceneBounds,
    pub heightfield: Heightfield,
}

#[derive(Clone, Copy, Debug)]
pub struct SceneBounds {
    pub lo: [f32; 3],
    pub hi: [f32; 3],
    pub center: [f32; 3],
    pub diag: f32,
    pub ground_z: f32,
}

impl SceneBounds {
    pub fn from_items(items: &[Item<'_>]) -> SceneBounds {
        let mut lo = Vec3::new(f64::MAX, f64::MAX, f64::MAX);
        let mut hi = Vec3::new(f64::MIN, f64::MIN, f64::MIN);
        for it in items {
            if let Some((a, b)) = it.mesh.bbox() {
                lo = lo.min(a);
                hi = hi.max(b);
            }
        }
        if lo.x == f64::MAX {
            lo = Vec3::ZERO;
            hi = Vec3::ZERO;
        }
        let f = |v: Vec3| [v.x as f32, v.y as f32, v.z as f32];
        let (lo, hi) = (f(lo), f(hi));
        let center = [(lo[0] + hi[0]) * 0.5, (lo[1] + hi[1]) * 0.5, (lo[2] + hi[2]) * 0.5];
        let d = [hi[0] - lo[0], hi[1] - lo[1], hi[2] - lo[2]];
        let diag = (d[0] * d[0] + d[1] * d[1] + d[2] * d[2]).sqrt().max(1e-3);
        SceneBounds { lo, hi, center, diag, ground_z: lo[2] }
    }
}

/// World-space length of one screen-space piece of a projected polyline
/// (`lines::PIECE_PX`).
const PIECE_PX: f32 = 6.0;
const HIGHLIGHT_WIDTH_PX: f32 = 2.8;
/// `lines::ALPHA_CREASE` / `ALPHA_ALL`.
const ALPHA_CREASE: f32 = 0.42;
const ALPHA_ALL: f32 = 0.62;
/// Half width of the fat lines the pick pass draws, in pixels.
const PICK_EDGE_HW: f32 = 2.5;

fn smoothstep(e0: f32, e1: f32, x: f32) -> f32 {
    let t = ((x - e0) / (e1 - e0)).clamp(0.0, 1.0);
    t * t * (3.0 - 2.0 * t)
}

/// Projects a polyline and appends its pieces, subdividing in world space so
/// the screen-linear depth of each piece stays close to the perspective-correct
/// depth in the G-buffer (`lines::push_polyline`).
fn push_polyline(out: &mut Vec<Seg>, view: &View, poly: &[Vec3], hw: f32, alpha: f32, estimate: bool, item: u32, pick: u32) {
    for seg in poly.windows(2) {
        let a = [seg[0].x as f32, seg[0].y as f32, seg[0].z as f32];
        let b = [seg[1].x as f32, seg[1].y as f32, seg[1].z as f32];
        let (Some(sa), Some(sb)) = (view.project(a), view.project(b)) else { continue };
        let len = ((sb[0] - sa[0]).powi(2) + (sb[1] - sa[1]).powi(2)).sqrt();
        if !len.is_finite() {
            continue;
        }
        let n = if view.is_orthographic() { 1 } else { (len / PIECE_PX).ceil().clamp(1.0, 4096.0) as u32 };
        let mut prev = sa;
        for i in 1..=n {
            let t = i as f32 / n as f32;
            let p = [a[0] + (b[0] - a[0]) * t, a[1] + (b[1] - a[1]) * t, a[2] + (b[2] - a[2]) * t];
            let Some(sp) = view.project(p) else { break };
            if alpha > 0.0 {
                out.push(Seg {
                    a: prev,
                    hw,
                    b: sp,
                    alpha,
                    flags: u32::from(estimate),
                    item,
                    pick,
                    _pad: 0,
                });
            }
            prev = sp;
        }
    }
}

impl Upload {
    pub fn build(items: &[Item<'_>], cam: &Camera, opts: &RenderOptions) -> Upload {
        let scene = SceneBounds::from_items(items);
        let (w, h) = (opts.width.max(1), opts.height.max(1));
        let view = View::new(cam, w, h);
        let mut up = Upload {
            opaque: Vec::new(),
            translucent: Vec::new(),
            materials: Vec::new(),
            creases: Vec::new(),
            highlights: Vec::new(),
            pick_edges: Vec::new(),
            scene,
            heightfield: Heightfield::disabled(scene),
        };

        for (ii, item) in items.iter().enumerate() {
            let m = item.material;
            up.materials.push(GpuMaterial {
                base: [
                    m.base_color[0] as f32,
                    m.base_color[1] as f32,
                    m.base_color[2] as f32,
                    m.roughness as f32,
                ],
                params: [m.metallic as f32, m.texture_scale as f32, item.opacity(), m.ior as f32],
                emissive: [
                    m.emissive[0] as f32,
                    m.emissive[1] as f32,
                    m.emissive[2] as f32,
                    texture_kind(&m.texture),
                ],
            });
            if ii >= NO_ITEM as usize {
                continue;
            }
            let mesh = item.mesh;
            let target = if item.is_translucent() { &mut up.translucent } else { &mut up.opaque };
            let vertex = |i: u32| {
                let i = i as usize * 3;
                [mesh.positions[i], mesh.positions[i + 1], mesh.positions[i + 2]]
            };
            let normal = |i: u32| {
                let i = i as usize * 3;
                if i + 2 < mesh.normals.len() {
                    [mesh.normals[i], mesh.normals[i + 1], mesh.normals[i + 2]]
                } else {
                    [0.0, 0.0, 1.0]
                }
            };
            for (t, tri) in mesh.indices.chunks_exact(3).enumerate() {
                let face = mesh.face_ids.get(t).copied().unwrap_or(u32::MAX);
                let highlighted = !item.highlight_faces.is_empty() && item.highlight_faces.contains(&face);
                let flags = (ii as u32) | (u32::from(highlighted) << 16);
                for &v in tri {
                    target.push(Vertex { pos: vertex(v), nrm: normal(v), face, item_flags: flags });
                }
            }
        }

        up.build_segments(items, &view, opts);
        if opts.contact_shadow {
            up.heightfield = Heightfield::build(items, scene);
        }
        up
    }

    /// Feature edges (crease-filtered), selection highlights, and the fat
    /// lines the id pass draws so picking can prefer an edge to a face.
    fn build_segments(&mut self, items: &[Item<'_>], view: &View, opts: &RenderOptions) {
        let mode = opts.edge_mode();
        let hw = opts.edge_width_px.max(0.2) * 0.5;
        let thr = opts.edge_crease_degrees.clamp(0.0, 180.0);
        let alpha = if mode == EdgeMode::All { ALPHA_ALL } else { ALPHA_CREASE };
        for (ii, item) in items.iter().enumerate() {
            for (i, poly) in item.edges.iter().enumerate() {
                push_polyline(
                    &mut self.pick_edges,
                    view,
                    poly,
                    PICK_EDGE_HW,
                    1.0,
                    false,
                    ii as u32,
                    PICK_EDGE | i as u32,
                );
                if !matches!(mode, EdgeMode::Creases | EdgeMode::All) {
                    continue;
                }
                if mode == EdgeMode::All {
                    push_polyline(&mut self.creases, view, poly, hw, alpha, false, ii as u32, 0);
                    continue;
                }
                match item.edge_dihedrals.get(i) {
                    // Real crease data: 0 is a tangent seam between two patches
                    // of one smooth surface, -1 an open boundary.
                    Some(&deg) => {
                        let w = if deg < 0.0 { 1.0 } else { smoothstep(thr - 4.0, thr + 4.0, deg as f32) };
                        if w > 0.0 {
                            push_polyline(&mut self.creases, view, poly, hw, alpha * w, false, ii as u32, 0);
                        }
                    }
                    // No topology: the shader estimates the dihedral from the
                    // G-buffer normals either side of the line.
                    None => push_polyline(&mut self.creases, view, poly, hw, alpha, true, ii as u32, 0),
                }
            }
            for poly in item.highlight_edges {
                push_polyline(
                    &mut self.highlights,
                    view,
                    poly,
                    HIGHLIGHT_WIDTH_PX * 0.5,
                    1.0,
                    false,
                    ii as u32,
                    0,
                );
            }
        }
    }
}

// ---------------------------------------------------------------------------
// Ground contact heightfield.

/// SSAO radius as a fraction of the scene diagonal (`occlusion::AO_RADIUS`).
pub const AO_RADIUS: f32 = 0.08;
pub const AO_BIAS: f32 = 0.002;
pub const AO_BLUR_DEPTH: f32 = 0.02;
const CONTACT_RADIUS: f32 = 0.25;
const CONTACT_STRENGTH: f32 = 0.85;
pub const HF_SIZE: u32 = 128;

/// 12-tap disc (golden-angle spiral), radii in 0..1 of the contact radius.
const DISC: [[f32; 2]; 12] = [
    [0.179136, 0.097862],
    [-0.343282, 0.084600],
    [0.253008, -0.379895],
    [0.082890, 0.533663],
    [-0.478055, -0.382706],
    [0.675504, -0.045024],
    [-0.508424, 0.532138],
    [0.016587, -0.790395],
    [0.555364, 0.632380],
    [-0.884522, -0.096367],
    [0.754124, -0.553442],
    [-0.190703, 0.960191],
];

/// The baked ground contact shadow of `occlusion::Heightfield`: a top-down
/// min-z raster of the opaque geometry that casts shadows (not guide
/// geometry), turned into a per-cell darkening that
/// the shading pass samples bilinearly. Built on the CPU and uploaded as a
/// single `HF_SIZE x HF_SIZE` R32Float texture, exactly as the CPU renderer
/// bakes it -- a function of world xy only, so it is far cheaper here than 24
/// heightfield taps per background pixel and smoother as well.
pub struct Heightfield {
    pub size: u32,
    pub shadow: Vec<f32>,
    pub origin: [f32; 2],
    pub cell: [f32; 2],
    pub enabled: bool,
    min_z: Vec<f32>,
    ground_z: f32,
    empty: f32,
}

impl Heightfield {
    fn disabled(scene: SceneBounds) -> Heightfield {
        Heightfield {
            size: 1,
            shadow: vec![1.0],
            origin: [0.0; 2],
            cell: [1.0; 2],
            enabled: false,
            min_z: Vec::new(),
            ground_z: scene.ground_z,
            empty: scene.ground_z,
        }
    }

    fn build(items: &[Item<'_>], sc: SceneBounds) -> Heightfield {
        let size = HF_SIZE;
        let radius = CONTACT_RADIUS * sc.diag;
        let ext = [(sc.hi[0] - sc.lo[0]).max(1e-4), (sc.hi[1] - sc.lo[1]).max(1e-4)];
        // Pad by a third of the footprint, but never by less than the contact
        // radius: the shadow has to reach zero inside the grid.
        let pad = [(0.35 * ext[0]).max(1.25 * radius), (0.35 * ext[1]).max(1.25 * radius)];
        let origin = [sc.center[0] - ext[0] * 0.5 - pad[0], sc.center[1] - ext[1] * 0.5 - pad[1]];
        let cell = [(ext[0] + 2.0 * pad[0]) / size as f32, (ext[1] + 2.0 * pad[1]) / size as f32];
        let n = (size * size) as usize;
        let mut hf = Heightfield {
            size,
            shadow: vec![1.0; n],
            origin,
            cell,
            enabled: true,
            min_z: vec![f32::INFINITY; n],
            ground_z: sc.ground_z,
            empty: sc.ground_z + 4.0 * sc.diag,
        };
        let z_cut = sc.ground_z + 2.0 * radius;
        for it in items {
            // As the CPU's `occlusion::Heightfield`: translucent items and
            // guide geometry (`Material::shadow` false) cast none.
            if it.is_translucent() || !it.casts_shadow() {
                continue;
            }
            let m = it.mesh;
            for tri in m.indices.chunks_exact(3) {
                let p = |i: u32| {
                    let i = i as usize * 3;
                    [m.positions[i], m.positions[i + 1], m.positions[i + 2]]
                };
                let (a, b, c) = (p(tri[0]), p(tri[1]), p(tri[2]));
                if a[2].min(b[2]).min(c[2]) > z_cut {
                    continue;
                }
                for v in [a, b, c] {
                    hf.mark(v[0], v[1], v[2]);
                }
                hf.scan(a, b, c);
            }
        }
        hf.bake_shadow(radius);
        hf
    }

    fn cell_of(&self, x: f32, y: f32) -> Option<(u32, u32)> {
        let fx = (x - self.origin[0]) / self.cell[0];
        let fy = (y - self.origin[1]) / self.cell[1];
        if !(fx >= 0.0 && fy >= 0.0) {
            return None;
        }
        let (ix, iy) = (fx as u32, fy as u32);
        if ix >= self.size || iy >= self.size { None } else { Some((ix, iy)) }
    }

    fn mark(&mut self, x: f32, y: f32, z: f32) {
        if let Some((ix, iy)) = self.cell_of(x, y) {
            self.store((iy * self.size + ix) as usize, z);
        }
    }

    fn store(&mut self, i: usize, z: f32) {
        if z < self.min_z[i] {
            self.min_z[i] = z;
        }
    }

    /// Rasterizes the triangle top-down, marking every cell whose centre is
    /// inside it with the interpolated z.
    fn scan(&mut self, a: [f32; 3], b: [f32; 3], c: [f32; 3]) {
        let area = (b[0] - a[0]) * (c[1] - a[1]) - (b[1] - a[1]) * (c[0] - a[0]);
        if !(area.abs() > 1e-12) {
            return;
        }
        let inv = 1.0 / area;
        let min_x = a[0].min(b[0]).min(c[0]);
        let max_x = a[0].max(b[0]).max(c[0]);
        let min_y = a[1].min(b[1]).min(c[1]);
        let max_y = a[1].max(b[1]).max(c[1]);
        let i0 = ((min_x - self.origin[0]) / self.cell[0]).floor().max(0.0) as u32;
        let i1 = (((max_x - self.origin[0]) / self.cell[0]).ceil() as i64).clamp(0, self.size as i64 - 1) as u32;
        let j0 = ((min_y - self.origin[1]) / self.cell[1]).floor().max(0.0) as u32;
        let j1 = (((max_y - self.origin[1]) / self.cell[1]).ceil() as i64).clamp(0, self.size as i64 - 1) as u32;
        if i0 > i1 || j0 > j1 || i0 >= self.size || j0 >= self.size {
            return;
        }
        for j in j0..=j1.min(self.size - 1) {
            let py = self.origin[1] + (j as f32 + 0.5) * self.cell[1];
            for i in i0..=i1.min(self.size - 1) {
                let px = self.origin[0] + (i as f32 + 0.5) * self.cell[0];
                let w0 = ((b[0] - a[0]) * (py - a[1]) - (b[1] - a[1]) * (px - a[0])) * inv;
                let w1 = ((c[0] - b[0]) * (py - b[1]) - (c[1] - b[1]) * (px - b[0])) * inv;
                let w2 = ((a[0] - c[0]) * (py - c[1]) - (a[1] - c[1]) * (px - c[0])) * inv;
                if w0 < 0.0 || w1 < 0.0 || w2 < 0.0 {
                    continue;
                }
                let z = w1 * a[2] + w2 * b[2] + w0 * c[2];
                self.store((j * self.size + i) as usize, z);
            }
        }
    }

    fn raw(&self, ix: i64, iy: i64) -> f32 {
        if ix < 0 || iy < 0 || ix >= self.size as i64 || iy >= self.size as i64 {
            return self.empty;
        }
        let z = self.min_z[(iy as u32 * self.size + ix as u32) as usize];
        if z.is_finite() { z } else { self.empty }
    }

    fn height_above_ground(&self, x: f32, y: f32) -> f32 {
        if self.min_z.is_empty() {
            return f32::INFINITY;
        }
        let fx = (x - self.origin[0]) / self.cell[0] - 0.5;
        let fy = (y - self.origin[1]) / self.cell[1] - 0.5;
        let (ix, iy) = (fx.floor(), fy.floor());
        let (tx, ty) = (fx - ix, fy - iy);
        let (ix, iy) = (ix as i64, iy as i64);
        let z00 = self.raw(ix, iy);
        let z10 = self.raw(ix + 1, iy);
        let z01 = self.raw(ix, iy + 1);
        let z11 = self.raw(ix + 1, iy + 1);
        let z = (z00 * (1.0 - tx) + z10 * tx) * (1.0 - ty) + (z01 * (1.0 - tx) + z11 * tx) * ty;
        (z - self.ground_z).max(0.0)
    }

    /// Two rings of disc taps asking the heightfield how close the model is
    /// overhead, windowed to exactly zero well before the "nothing here"
    /// height so distant ground stays bit-exactly the background.
    fn bake_shadow(&mut self, radius: f32) {
        let soft = 0.35 * radius;
        for j in 0..self.size {
            let y = self.origin[1] + (j as f32 + 0.5) * self.cell[1];
            for i in 0..self.size {
                let x = self.origin[0] + (i as f32 + 0.5) * self.cell[0];
                let mut num = 0.0f32;
                let mut den = 0.0f32;
                for ring in [0.45f32, 1.0] {
                    for d in DISC {
                        let h = self.height_above_ground(x + d[0] * radius * ring, y + d[1] * radius * ring);
                        let window = smoothstep(2.0 * radius, 0.5 * radius, h);
                        if window > 0.0 {
                            let t = h / soft;
                            num += window / (1.0 + t * t);
                        }
                        den += 1.0;
                    }
                }
                self.shadow[(j * self.size + i) as usize] = 1.0 - CONTACT_STRENGTH * (num / den);
            }
        }
    }
}
