//! Test support shared by the render modules: synthetic and kernel-built
//! meshes, image statistics, hashing and a single-pass frame runner.

#![allow(dead_code)]

use super::{Camera, Ctx, Frame, Image, Item, RenderOptions, Scene, View, aa, lines, occlusion, oit, shading};
use crate::geom::{Plane, Vec3};
use crate::sketch::{Loop, P2, Sketch};
use crate::tessellate::Mesh;

pub type Edges = Vec<Vec<Vec3>>;

/// Axis-aligned unit cube 0..1 with flat per-face normals and its 12 edges.
pub fn unit_box() -> (Mesh, Edges) {
    let faces: [([f32; 3], [[f32; 3]; 4]); 6] = [
        ([0.0, 0.0, -1.0], [[0.0, 0.0, 0.0], [0.0, 1.0, 0.0], [1.0, 1.0, 0.0], [1.0, 0.0, 0.0]]),
        ([0.0, 0.0, 1.0], [[0.0, 0.0, 1.0], [1.0, 0.0, 1.0], [1.0, 1.0, 1.0], [0.0, 1.0, 1.0]]),
        ([0.0, -1.0, 0.0], [[0.0, 0.0, 0.0], [1.0, 0.0, 0.0], [1.0, 0.0, 1.0], [0.0, 0.0, 1.0]]),
        ([0.0, 1.0, 0.0], [[0.0, 1.0, 0.0], [0.0, 1.0, 1.0], [1.0, 1.0, 1.0], [1.0, 1.0, 0.0]]),
        ([-1.0, 0.0, 0.0], [[0.0, 0.0, 0.0], [0.0, 0.0, 1.0], [0.0, 1.0, 1.0], [0.0, 1.0, 0.0]]),
        ([1.0, 0.0, 0.0], [[1.0, 0.0, 0.0], [1.0, 1.0, 0.0], [1.0, 1.0, 1.0], [1.0, 0.0, 1.0]]),
    ];
    let mut mesh = Mesh { positions: Vec::new(), normals: Vec::new(), indices: Vec::new(), face_ids: Vec::new(), colors: Vec::new() };
    for (fi, (n, quad)) in faces.iter().enumerate() {
        let base = (mesh.positions.len() / 3) as u32;
        for p in quad {
            mesh.positions.extend_from_slice(p);
            mesh.normals.extend_from_slice(n);
        }
        mesh.indices.extend_from_slice(&[base, base + 1, base + 2, base, base + 2, base + 3]);
        mesh.face_ids.extend_from_slice(&[fi as u32, fi as u32]);
    }
    let v = |x: f64, y: f64, z: f64| Vec3::new(x, y, z);
    let mut edges = Vec::new();
    for z in [0.0, 1.0] {
        edges.push(vec![v(0.0, 0.0, z), v(1.0, 0.0, z)]);
        edges.push(vec![v(1.0, 0.0, z), v(1.0, 1.0, z)]);
        edges.push(vec![v(1.0, 1.0, z), v(0.0, 1.0, z)]);
        edges.push(vec![v(0.0, 1.0, z), v(0.0, 0.0, z)]);
    }
    for (x, y) in [(0.0, 0.0), (1.0, 0.0), (1.0, 1.0), (0.0, 1.0)] {
        edges.push(vec![v(x, y, 0.0), v(x, y, 1.0)]);
    }
    (mesh, edges)
}

/// Unit sphere at the origin with smooth normals; `lat` x `lon` segments.
pub fn uv_sphere(lat: u32, lon: u32) -> (Mesh, Edges) {
    let (lat, lon) = (lat.max(2), lon.max(3));
    let mut mesh = Mesh { positions: Vec::new(), normals: Vec::new(), indices: Vec::new(), face_ids: Vec::new(), colors: Vec::new() };
    for i in 0..=lat {
        let theta = std::f32::consts::PI * i as f32 / lat as f32;
        for j in 0..=lon {
            let phi = 2.0 * std::f32::consts::PI * j as f32 / lon as f32;
            let p = [theta.sin() * phi.cos(), theta.sin() * phi.sin(), theta.cos()];
            mesh.positions.extend_from_slice(&p);
            mesh.normals.extend_from_slice(&p);
        }
    }
    let row = lon + 1;
    for i in 0..lat {
        for j in 0..lon {
            let a = i * row + j;
            let b = a + row;
            mesh.indices.extend_from_slice(&[a, b, a + 1, a + 1, b, b + 1]);
            mesh.face_ids.extend_from_slice(&[0, 0]);
        }
    }
    (mesh, Vec::new())
}

fn kernel_mesh(solid: &crate::kernel::KSolid) -> (Mesh, Edges) {
    let (mesh, topo) = crate::tessellate::tessellate(solid, 0.05).expect("tessellate");
    let edges = topo.edges.iter().map(|e| e.polyline.clone()).collect();
    (mesh, edges)
}

/// Cylinder r=10, h=20 through the kernel.
pub fn cylinder() -> (Mesh, Edges) {
    let s = Sketch::new(Plane::XY).with_loop(Loop::circle(P2::new(0.0, 0.0), 10.0));
    let solid = crate::kernel::extrude(&s, 20.0).unwrap().remove(0);
    kernel_mesh(&solid)
}

/// An L-shaped extrusion (20 x 20 footprint, 5 tall) through the kernel.
pub fn l_block() -> (Mesh, Edges) {
    let pts = [P2::new(0.0, 0.0), P2::new(20.0, 0.0), P2::new(20.0, 8.0), P2::new(8.0, 8.0), P2::new(8.0, 20.0), P2::new(0.0, 20.0)];
    let s = Sketch::new(Plane::XY).with_loop(Loop::polygon(&pts));
    let solid = crate::kernel::extrude(&s, 5.0).unwrap().remove(0);
    kernel_mesh(&solid)
}

/// The `examples/planter.star` shell with default parameters: a revolved
/// tapered profile with a drainage hole.
pub fn planter_shell() -> (Mesh, Edges) {
    let (h, r_top, r_bottom, wall, base, drain) = (90.0, 55.0, 40.0, 2.4, 4.0, 4.0);
    let slope = (r_top - r_bottom) / h;
    let pts = [
        P2::new(drain, 0.0),
        P2::new(r_bottom, 0.0),
        P2::new(r_top, h),
        P2::new(r_top - wall, h),
        P2::new(r_bottom - wall + slope * base, base),
        P2::new(drain, base),
    ];
    let s = Sketch::new(Plane::XZ).with_loop(Loop::polygon(&pts));
    let solid = crate::kernel::revolve(&s, Vec3::ZERO, Vec3::Z, std::f64::consts::TAU).unwrap().remove(0);
    kernel_mesh(&solid)
}

/// A one-sided quad `p0 p1 p2 p3` (counter-clockwise seen from the front),
/// with the flat normal on every vertex. The building block for the path
/// tracer's test scenes: a Cornell box is six of these and a light.
pub fn quad(p0: [f32; 3], p1: [f32; 3], p2: [f32; 3], p3: [f32; 3]) -> Mesh {
    let e1 = [p1[0] - p0[0], p1[1] - p0[1], p1[2] - p0[2]];
    let e2 = [p3[0] - p0[0], p3[1] - p0[1], p3[2] - p0[2]];
    let n = [e1[1] * e2[2] - e1[2] * e2[1], e1[2] * e2[0] - e1[0] * e2[2], e1[0] * e2[1] - e1[1] * e2[0]];
    let len = (n[0] * n[0] + n[1] * n[1] + n[2] * n[2]).sqrt().max(1e-20);
    let n = [n[0] / len, n[1] / len, n[2] / len];
    let mut mesh = Mesh { positions: Vec::new(), normals: Vec::new(), indices: Vec::new(), face_ids: Vec::new(), colors: Vec::new() };
    for p in [p0, p1, p2, p3] {
        mesh.positions.extend_from_slice(&p);
        mesh.normals.extend_from_slice(&n);
    }
    mesh.indices.extend_from_slice(&[0, 1, 2, 0, 2, 3]);
    mesh.face_ids.extend_from_slice(&[0, 0]);
    mesh
}

/// A closed axis-aligned box from `lo` to `hi`, outward normals.
pub fn box_mesh(lo: [f32; 3], hi: [f32; 3]) -> Mesh {
    let (unit, _) = unit_box();
    let mut m = unit;
    for p in m.positions.chunks_exact_mut(3) {
        for k in 0..3 {
            p[k] = lo[k] + p[k] * (hi[k] - lo[k]);
        }
    }
    m
}

/// Rec. 709 luma of the 8-bit encoded pixel, 0..255.
pub fn luma(img: &Image, x: u32, y: u32) -> f32 {
    let i = ((y * img.width + x) * 4) as usize;
    0.2126 * img.rgba[i] as f32 + 0.7152 * img.rgba[i + 1] as f32 + 0.0722 * img.rgba[i + 2] as f32
}

/// Mean luma over the half-open rect `(x0, y0, x1, y1)`.
pub fn mean_luma(img: &Image, rect: (u32, u32, u32, u32)) -> f32 {
    let (x0, y0, x1, y1) = rect;
    let mut sum = 0.0f64;
    let mut n = 0u64;
    for y in y0..y1.min(img.height) {
        for x in x0..x1.min(img.width) {
            sum += luma(img, x, y) as f64;
            n += 1;
        }
    }
    if n == 0 { 0.0 } else { (sum / n as f64) as f32 }
}

/// Number of distinct rounded luma levels in the rect.
pub fn distinct_levels(img: &Image, rect: (u32, u32, u32, u32)) -> usize {
    let (x0, y0, x1, y1) = rect;
    let mut seen = [false; 256];
    for y in y0..y1.min(img.height) {
        for x in x0..x1.min(img.width) {
            seen[luma(img, x, y).round().clamp(0.0, 255.0) as usize] = true;
        }
    }
    seen.iter().filter(|&&s| s).count()
}

pub fn fnv1a64(bytes: &[u8]) -> u64 {
    let mut h = 0xcbf2_9ce4_8422_2325u64;
    for &b in bytes {
        h ^= b as u64;
        h = h.wrapping_mul(0x0000_0100_0000_01b3);
    }
    h
}

/// Runs one pass with jitter (0.5, 0.5) and returns the `Frame` for
/// buffer-level assertions.
pub fn render_frame(items: &[Item<'_>], cam: &Camera, opts: &RenderOptions) -> Frame {
    let (w, h) = (opts.width.max(1), opts.height.max(1));
    let scene = Scene::from_items(items);
    let view = View::new(cam, w, h);
    let threads = aa::resolve_threads(opts.threads);
    let sun_shadow = opts.sun.filter(|s| s.is_up()).map(|_| super::SunShadow::build(items));
    let ctx = Ctx { items, opts, scene: &scene, view: &view, threads, sun_shadow: sun_shadow.as_ref() };
    let hf = occlusion::Heightfield::build(&ctx);
    let mut frame = Frame::new(w, h);
    let mut oit_buf = oit::Buffers::new(w, h);
    frame.clear();
    aa::geometry_pass(&mut frame, &ctx);
    occlusion::ssao(&mut frame, &ctx);
    occlusion::ground_contact(&mut frame, &ctx, &hf);
    shading::shade(&mut frame, &ctx);
    oit::translucent_pass(&mut frame, &ctx, &mut oit_buf);
    aa::tonemap(&mut frame, &ctx, &oit_buf);
    lines::overlay(&mut frame, &ctx, &oit_buf);
    frame
}

/// A plain Lambert + Blinn rasterizer, independent of the frame pipeline:
/// the reference silhouette for the samples=1 coverage test.
pub mod reference {
    use super::super::{Camera, HIGHLIGHT, Image, Item, RenderOptions, dot, normalize, to_srgb8};
    use crate::geom::Vec3;

    struct View {
        eye: [f32; 3],
        right: [f32; 3],
        up: [f32; 3],
        fwd: [f32; 3],
        focal: f32,
        ortho_scale: f32,
        orthographic: bool,
        w: f32,
        h: f32,
    }

    impl View {
        fn new(cam: &Camera, w: u32, h: u32) -> View {
            let fwd = (cam.target - cam.eye).normalized();
            let right = fwd.cross(cam.up).normalized();
            let up = right.cross(fwd);
            let focal = (h as f64 * 0.5) / (cam.fov_degrees.to_radians() * 0.5).tan();
            let dist = (cam.target - cam.eye).length();
            let ortho_scale = focal / dist as f64;
            let f = |v: Vec3| [v.x as f32, v.y as f32, v.z as f32];
            View {
                eye: f(cam.eye),
                right: f(right),
                up: f(up),
                fwd: f(fwd),
                focal: focal as f32,
                ortho_scale: ortho_scale as f32,
                orthographic: cam.orthographic,
                w: w as f32,
                h: h as f32,
            }
        }

        fn project(&self, p: [f32; 3]) -> Option<[f32; 3]> {
            let d = [p[0] - self.eye[0], p[1] - self.eye[1], p[2] - self.eye[2]];
            let x = dot(d, self.right);
            let y = dot(d, self.up);
            let z = dot(d, self.fwd);
            if self.orthographic {
                Some([self.w * 0.5 + x * self.ortho_scale, self.h * 0.5 - y * self.ortho_scale, z])
            } else {
                if z <= 1e-4 {
                    return None;
                }
                Some([self.w * 0.5 + x * self.focal / z, self.h * 0.5 - y * self.focal / z, z])
            }
        }
    }

    pub fn render(items: &[Item<'_>], cam: &Camera, opts: &RenderOptions) -> Image {
        let (w, h) = (opts.width.max(1), opts.height.max(1));
        let mut img = Image::new(w, h, opts.background);
        let mut depth = vec![f32::INFINITY; (w * h) as usize];
        let view = View::new(cam, w, h);
        let key = normalize([-0.4, -0.6, 0.7]);
        let fill = normalize([0.6, 0.3, 0.2]);
        let view_dir = [-view.fwd[0], -view.fwd[1], -view.fwd[2]];
        if opts.draw_grid {
            draw_grid(&mut img, &depth, &view, items);
        }
        for item in items {
            let m = item.mesh;
            let mat_base = item.material.base_color.map(|c| c as f32);
            let rough = item.material.roughness as f32;
            for (t, tri) in m.indices.chunks_exact(3).enumerate() {
                let face = m.face_ids.get(t).copied().unwrap_or(u32::MAX);
                let base = if item.highlight_faces.contains(&face) { HIGHLIGHT } else { mat_base };
                let p = |i: u32| {
                    let i = i as usize * 3;
                    [m.positions[i], m.positions[i + 1], m.positions[i + 2]]
                };
                let n = |i: u32| {
                    let i = i as usize * 3;
                    [m.normals[i], m.normals[i + 1], m.normals[i + 2]]
                };
                let (p0, p1, p2) = (p(tri[0]), p(tri[1]), p(tri[2]));
                let (Some(s0), Some(s1), Some(s2)) = (view.project(p0), view.project(p1), view.project(p2)) else {
                    continue;
                };
                let shade = |nv: [f32; 3]| -> [f32; 3] {
                    let nv = normalize(nv);
                    let nv = if dot(nv, view_dir) < 0.0 { [-nv[0], -nv[1], -nv[2]] } else { nv };
                    let hemi = 0.5 + 0.5 * nv[2];
                    let ambient = 0.18 + 0.12 * hemi;
                    let diff = 0.75 * dot(nv, key).max(0.0) + 0.25 * dot(nv, fill).max(0.0);
                    let half = normalize([key[0] + view_dir[0], key[1] + view_dir[1], key[2] + view_dir[2]]);
                    let shininess = 8.0 + (1.0 - rough) * 120.0;
                    let spec = dot(nv, half).max(0.0).powf(shininess) * (0.05 + 0.4 * (1.0 - rough));
                    [base[0] * (ambient + diff) + spec, base[1] * (ambient + diff) + spec, base[2] * (ambient + diff) + spec]
                };
                let c = [shade(n(tri[0])), shade(n(tri[1])), shade(n(tri[2]))];
                raster_triangle(&mut img, &mut depth, w, h, [s0, s1, s2], c);
            }
        }
        if opts.draw_edges {
            for item in items {
                for poly in item.edges {
                    draw_polyline(&mut img, &depth, w, h, &view, poly, [28, 30, 34, 255], 0.998, 1);
                }
            }
        }
        let hl = [to_srgb8(HIGHLIGHT[0]), to_srgb8(HIGHLIGHT[1]), to_srgb8(HIGHLIGHT[2]), 255];
        for item in items {
            for poly in item.highlight_edges {
                draw_polyline(&mut img, &depth, w, h, &view, poly, hl, 0.99, 3);
            }
        }
        img
    }

    #[allow(clippy::too_many_arguments)]
    fn draw_polyline(img: &mut Image, depth: &[f32], w: u32, h: u32, view: &View, poly: &[Vec3], color: [u8; 4], bias: f32, thickness: i32) {
        for seg in poly.windows(2) {
            let a = [seg[0].x as f32, seg[0].y as f32, seg[0].z as f32];
            let b = [seg[1].x as f32, seg[1].y as f32, seg[1].z as f32];
            if let (Some(sa), Some(sb)) = (view.project(a), view.project(b)) {
                for dy in -(thickness / 2)..=(thickness / 2) {
                    for dx in -(thickness / 2)..=(thickness / 2) {
                        let o = |p: [f32; 3]| [p[0] + dx as f32, p[1] + dy as f32, p[2]];
                        draw_line(img, depth, w, h, o(sa), o(sb), color, bias);
                    }
                }
            }
        }
    }

    fn raster_triangle(img: &mut Image, depth: &mut [f32], w: u32, h: u32, s: [[f32; 3]; 3], c: [[f32; 3]; 3]) {
        let min_x = s.iter().map(|p| p[0]).fold(f32::INFINITY, f32::min).floor().max(0.0) as i32;
        let max_x = s.iter().map(|p| p[0]).fold(f32::NEG_INFINITY, f32::max).ceil().min(w as f32 - 1.0) as i32;
        let min_y = s.iter().map(|p| p[1]).fold(f32::INFINITY, f32::min).floor().max(0.0) as i32;
        let max_y = s.iter().map(|p| p[1]).fold(f32::NEG_INFINITY, f32::max).ceil().min(h as f32 - 1.0) as i32;
        if min_x > max_x || min_y > max_y {
            return;
        }
        let area = edge(s[0], s[1], s[2]);
        if area.abs() < 1e-12 {
            return;
        }
        let inv = 1.0 / area;
        for y in min_y..=max_y {
            let py = y as f32 + 0.5;
            for x in min_x..=max_x {
                let px = x as f32 + 0.5;
                let w0 = edge(s[1], s[2], [px, py, 0.0]) * inv;
                let w1 = edge(s[2], s[0], [px, py, 0.0]) * inv;
                let w2 = edge(s[0], s[1], [px, py, 0.0]) * inv;
                if w0 < 0.0 || w1 < 0.0 || w2 < 0.0 {
                    continue;
                }
                let z = w0 * s[0][2] + w1 * s[1][2] + w2 * s[2][2];
                let di = (y as u32 * w + x as u32) as usize;
                if z < depth[di] {
                    depth[di] = z;
                    let r = w0 * c[0][0] + w1 * c[1][0] + w2 * c[2][0];
                    let g = w0 * c[0][1] + w1 * c[1][1] + w2 * c[2][1];
                    let b = w0 * c[0][2] + w1 * c[1][2] + w2 * c[2][2];
                    img.put(x as u32, y as u32, [to_srgb8(r), to_srgb8(g), to_srgb8(b), 255]);
                }
            }
        }
    }

    fn edge(a: [f32; 3], b: [f32; 3], p: [f32; 3]) -> f32 {
        (b[0] - a[0]) * (p[1] - a[1]) - (b[1] - a[1]) * (p[0] - a[0])
    }

    #[allow(clippy::too_many_arguments)]
    fn draw_line(img: &mut Image, depth: &[f32], w: u32, h: u32, a: [f32; 3], b: [f32; 3], color: [u8; 4], bias: f32) {
        let dx = b[0] - a[0];
        let dy = b[1] - a[1];
        let steps = dx.abs().max(dy.abs()).ceil().max(1.0) as i32;
        for i in 0..=steps {
            let t = i as f32 / steps as f32;
            let x = a[0] + dx * t;
            let y = a[1] + dy * t;
            let z = (a[2] + (b[2] - a[2]) * t) * bias;
            if x < 0.0 || y < 0.0 || x >= w as f32 || y >= h as f32 {
                continue;
            }
            let di = (y as u32 * w + x as u32) as usize;
            if z <= depth[di] {
                img.put(x as u32, y as u32, color);
            }
        }
    }

    fn draw_grid(img: &mut Image, depth: &[f32], view: &View, items: &[Item<'_>]) {
        let mut lo = Vec3::new(f64::MAX, f64::MAX, f64::MAX);
        let mut hi = Vec3::new(f64::MIN, f64::MIN, f64::MIN);
        for it in items {
            if let Some((a, b)) = it.mesh.bbox() {
                lo = lo.min(a);
                hi = hi.max(b);
            }
        }
        if lo.x == f64::MAX {
            return;
        }
        let size = (hi - lo).length().max(1.0);
        let step = super::lines::nice_step(size / 8.0);
        let z = lo.z as f32;
        let ext = (size * 1.2 / step).ceil() as i32;
        let cx = ((lo.x + hi.x) / 2.0 / step).round() * step;
        let cy = ((lo.y + hi.y) / 2.0 / step).round() * step;
        let (w, h) = (img.width, img.height);
        for i in -ext..=ext {
            let o = i as f64 * step;
            let col = if i == 0 { [90, 92, 98, 255] } else { [48, 50, 56, 255] };
            let a = [(cx + o) as f32, (cy - ext as f64 * step) as f32, z];
            let b = [(cx + o) as f32, (cy + ext as f64 * step) as f32, z];
            if let (Some(sa), Some(sb)) = (view.project(a), view.project(b)) {
                draw_line(img, depth, w, h, sa, sb, col, 1.0);
            }
            let a = [(cx - ext as f64 * step) as f32, (cy + o) as f32, z];
            let b = [(cx + ext as f64 * step) as f32, (cy + o) as f32, z];
            if let (Some(sa), Some(sb)) = (view.project(a), view.project(b)) {
                draw_line(img, depth, w, h, sa, sb, col, 1.0);
            }
        }
    }
}
