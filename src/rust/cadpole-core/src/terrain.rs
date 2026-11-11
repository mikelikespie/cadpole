//! Terrain and site surfaces: a ground surface from scattered spot
//! elevations, as a triangulated irregular network or as a fitted B-spline,
//! and the queries a site model asks of it.
//!
//! **Two methods, one value.** A survey gives a few hundred `(x, y, z)`
//! spot elevations, a LiDAR footprint a few, a contour drawing a few
//! thousand along lines. [`Method::Tin`] is the Delaunay triangulation of
//! those points ([`crate::cdt`], exact integer predicates, no dependency):
//! it passes through every point, it is what a surveyor's software draws,
//! and its body is a mesh — a slab from the triangles down to a base plane,
//! watertight by construction. [`Method::Fit`] is a least-squares bicubic
//! B-spline surface through the same points (Piegl & Tiller §9.4.3, the
//! global approximation, on a clamped uniform knot vector), and its body is
//! an exact B-rep slab whose top face is that surface, so a boolean with a
//! zoning envelope or a building stays on the exact path and exports to
//! STEP. The fit smooths; the TIN does not.
//!
//! **A height field, always.** Both surfaces are `z = f(x, y)` over a
//! rectangle or a convex region of the plane: `z_at`, `slope_at` and the
//! contours are defined on that, an overhang is not a terrain, and the
//! B-spline's control net has its `x` and `y` at the Greville abscissae, so
//! the patch is *exactly* `x = x(u)`, `y = y(v)` (linear precision) and its
//! four boundary curves lie in the four vertical planes of the extent. That
//! is what lets the slab's sides be planar faces.
//!
//! **Offset is a shift.** "Thirty feet above grade at every point" is the
//! surface raised by thirty feet in `z`: the same TIN with every vertex
//! moved, the same B-spline with every control point moved — exact in both
//! cases, and a boolean-ready body in both. It is not a normal offset (the
//! rule is written in elevation, not in distance from the ground), which is
//! why it costs nothing.
//!
//! **A rectangle comes out exactly.** [`Terrain::clip`] is the same surface
//! over a smaller rectangle: a fit split by knot insertion, a TIN's
//! triangles clipped, `z` unchanged wherever both are defined. That is how
//! a lot's envelope is a slab over the lot with no boolean — the exact
//! kernel's intersection of the patch with a prism gives a trimmed face the
//! tessellator cannot always close.
//!
//! **What is not here.** Breaklines on the TIN (the CDT can take them; no
//! script asks yet), a surface clipped to an arbitrary polygon (a rectangle
//! is exact, anything else is a boolean with the slab), datum conversion (a
//! number the script adds), and
//! any transcendental: the module is arithmetic and square roots, so it is
//! the same on every target.

use std::collections::HashMap;

use truck_modeling::{
    builder, BSplineCurve, BSplineSurface, Curve as TCurve, Edge, Face, KnotVec, NurbsCurve, NurbsSurface, Point3,
    Shell, Solid, Surface, Vector4, Vertex, Wire,
};

use crate::cdt::{orient2d, Cdt, IPoint, Snap};
use crate::curve::{Curve, Segment3};
use crate::geom::Vec3;
use crate::kernel::KSolid;
use crate::mesh_kernel::MeshBody;
use crate::sketch::P2;
use crate::tessellate::{FaceInfo, Mesh, Topology};
use crate::{CadpoleError, Result};

/// The two constructions, the members of the `terrain` vocabulary.
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub enum Method {
    /// A Delaunay triangulation of the points: through every one, a mesh body.
    Tin,
    /// A least-squares bicubic B-spline: smooth, an exact B-rep body.
    Fit,
}

impl Method {
    /// Both, in documentation order; the vocabulary reads this list.
    pub const ALL: [Method; 2] = [Method::Tin, Method::Fit];

    pub fn name(self) -> &'static str {
        match self {
            Method::Tin => "tin",
            Method::Fit => "fit",
        }
    }

    pub fn parse(s: &str) -> Result<Method> {
        Self::ALL.into_iter().find(|m| m.name() == s).ok_or_else(|| {
            CadpoleError::invalid(format!(
                "terrain: method {s:?} is not one of these; valid: {}",
                Self::ALL.iter().map(|m| m.name()).collect::<Vec<_>>().join(", ")
            ))
        })
    }
}

/// The rectangle a terrain is defined over, in mm.
#[derive(Clone, Copy, Debug, PartialEq)]
pub struct Extent {
    pub x0: f64,
    pub y0: f64,
    pub x1: f64,
    pub y1: f64,
}

impl Extent {
    pub fn new(x0: f64, y0: f64, x1: f64, y1: f64) -> Result<Extent> {
        if !(x0.is_finite() && y0.is_finite() && x1.is_finite() && y1.is_finite()) {
            return Err(CadpoleError::invalid("terrain: extent must be finite"));
        }
        if x1 <= x0 || y1 <= y0 {
            return Err(CadpoleError::invalid(format!(
                "terrain: extent ({x0}, {y0}) .. ({x1}, {y1}) must have positive width and height"
            )));
        }
        Ok(Extent { x0, y0, x1, y1 })
    }

    /// The bounding rectangle of `points` in the plane.
    pub fn of_points(points: &[Vec3]) -> Result<Extent> {
        let mut e = Extent { x0: f64::MAX, y0: f64::MAX, x1: f64::MIN, y1: f64::MIN };
        for p in points {
            e.x0 = e.x0.min(p.x);
            e.y0 = e.y0.min(p.y);
            e.x1 = e.x1.max(p.x);
            e.y1 = e.y1.max(p.y);
        }
        Extent::new(e.x0, e.y0, e.x1, e.y1)
            .map_err(|_| CadpoleError::invalid("terrain: the points must span a rectangle of positive width and height"))
    }

    pub fn width(&self) -> f64 {
        self.x1 - self.x0
    }

    pub fn height(&self) -> f64 {
        self.y1 - self.y0
    }

    pub fn contains(&self, x: f64, y: f64) -> bool {
        x >= self.x0 && x <= self.x1 && y >= self.y0 && y <= self.y1
    }

    /// Whether `self` lies inside `outer` (an edge may coincide).
    pub fn within(&self, outer: &Extent) -> bool {
        self.x0 >= outer.x0 && self.y0 >= outer.y0 && self.x1 <= outer.x1 && self.y1 <= outer.y1
    }

    /// The four corners, counter-clockwise from `(x0, y0)`.
    pub fn corners(&self) -> [P2; 4] {
        [P2::new(self.x0, self.y0), P2::new(self.x1, self.y0), P2::new(self.x1, self.y1), P2::new(self.x0, self.y1)]
    }
}

// ------------------------------------------------------------------ TIN

/// A triangulated irregular network: the Delaunay triangulation of the
/// spot elevations, counter-clockwise seen from above, with its boundary.
#[derive(Clone, Debug)]
pub struct Tin {
    /// The vertices, the points as given (duplicates in the plane dropped).
    pub points: Vec<Vec3>,
    /// Triangles as vertex indices, counter-clockwise from +z.
    pub triangles: Vec<[u32; 3]>,
    /// The boundary loop, counter-clockwise: the convex hull of the points,
    /// or the extent rectangle (with any point that lies on its edge).
    pub boundary: Vec<u32>,
    /// Points that shared a plane position with an earlier one and were
    /// dropped (the first one's `z` stands).
    pub duplicates: usize,
    /// Points outside the extent, dropped from the triangulation (they
    /// still informed the corner elevations).
    pub outside: usize,
}

/// The least-squares plane `z = a x + b y + c` through `points`, or a
/// horizontal plane at their mean when they do not span one.
fn plane_fit(points: &[Vec3]) -> (f64, f64, f64) {
    let n = points.len() as f64;
    if points.is_empty() {
        return (0.0, 0.0, 0.0);
    }
    let (mut mx, mut my, mut mz) = (0.0, 0.0, 0.0);
    for p in points {
        mx += p.x;
        my += p.y;
        mz += p.z;
    }
    let (mx, my, mz) = (mx / n, my / n, mz / n);
    let (mut sxx, mut sxy, mut syy, mut sxz, mut syz) = (0.0, 0.0, 0.0, 0.0, 0.0);
    for p in points {
        let (dx, dy, dz) = (p.x - mx, p.y - my, p.z - mz);
        sxx += dx * dx;
        sxy += dx * dy;
        syy += dy * dy;
        sxz += dx * dz;
        syz += dy * dz;
    }
    let det = sxx * syy - sxy * sxy;
    let scale = (sxx * syy).max(1e-300);
    if det.abs() <= 1e-12 * scale {
        return (0.0, 0.0, mz);
    }
    let a = (sxz * syy - syz * sxy) / det;
    let b = (syz * sxx - sxz * sxy) / det;
    (a, b, mz - a * mx - b * my)
}

/// `z` at `(x, y)` from the plane through the `k` nearest points: how a
/// corner of the extent outside the data is given an elevation.
fn local_plane_z(points: &[Vec3], x: f64, y: f64) -> f64 {
    let k = points.len().min(12);
    let mut by_distance: Vec<(f64, usize)> =
        points.iter().enumerate().map(|(i, p)| ((p.x - x) * (p.x - x) + (p.y - y) * (p.y - y), i)).collect();
    by_distance.sort_by(|a, b| a.0.partial_cmp(&b.0).unwrap_or(std::cmp::Ordering::Equal).then(a.1.cmp(&b.1)));
    let near: Vec<Vec3> = by_distance.iter().take(k).map(|&(_, i)| points[i]).collect();
    let (a, b, c) = plane_fit(&near);
    a * x + b * y + c
}

fn check_points(points: &[Vec3], what: &str) -> Result<()> {
    if points.len() < 3 {
        return Err(CadpoleError::invalid(format!("{what}: at least 3 points are needed, got {}", points.len())));
    }
    for (i, p) in points.iter().enumerate() {
        if !(p.x.is_finite() && p.y.is_finite() && p.z.is_finite()) {
            return Err(CadpoleError::invalid(format!("{what}: point {i} is not finite")));
        }
    }
    Ok(())
}

/// Triangulate `points`. With no `extent` the TIN covers their convex hull;
/// with one it covers exactly that rectangle: the four corners are added
/// (interpolated from the points' own TIN where they fall inside its hull,
/// extrapolated from the plane of the twelve nearest points where they do
/// not), the rectangle's edges are constraints, and points outside it are
/// left out.
pub fn triangulate(points: &[Vec3], extent: Option<Extent>) -> Result<Tin> {
    check_points(points, "terrain")?;
    let Some(extent) = extent else { return triangulate_hull(points) };
    // The corners' elevations come from the data alone.
    let data = triangulate_hull(points)?;
    let corners: Vec<Vec3> = extent
        .corners()
        .iter()
        .map(|c| Vec3::new(c.x, c.y, data.z_at(c.x, c.y).unwrap_or_else(|| local_plane_z(points, c.x, c.y))))
        .collect();
    let outside = points.iter().filter(|p| !extent.contains(p.x, p.y)).count();
    let inside: Vec<Vec3> = points.iter().copied().filter(|p| extent.contains(p.x, p.y)).collect();
    let mut all = corners;
    all.extend(inside);
    triangulate_in(&all, Some(extent)).map(|mut t| {
        t.outside = outside;
        t
    })
}

fn triangulate_hull(points: &[Vec3]) -> Result<Tin> {
    triangulate_in(points, None)
}

/// The triangulation proper. With `rect`, its first four points are the
/// rectangle's corners in counter-clockwise order and every other point
/// lies in or on it.
fn triangulate_in(points: &[Vec3], rect: Option<Extent>) -> Result<Tin> {
    let extent = Extent::of_points(points)?;
    let snap = Snap::over([extent.x0, extent.y0], [extent.x1, extent.y1]);
    let mut cdt = Cdt::new(points.len());
    // `vertex_of[i]` is the CDT vertex of kept point `i`; `point_of[v]` the
    // kept point of vertex `v`.
    let mut kept: Vec<Vec3> = Vec::with_capacity(points.len());
    let mut vertex_of: Vec<u32> = Vec::with_capacity(points.len());
    let mut point_of: Vec<u32> = vec![u32::MAX; 3];
    let mut duplicates = 0;
    for p in points {
        let (v, inserted) = cdt.insert(snap.snap([p.x, p.y]));
        if !inserted {
            duplicates += 1;
            continue;
        }
        vertex_of.push(v);
        point_of.push(kept.len() as u32);
        kept.push(*p);
    }
    if kept.len() < 3 {
        return Err(CadpoleError::invalid(format!(
            "terrain: only {} distinct point{} in the plane; a surface needs 3",
            kept.len(),
            if kept.len() == 1 { "" } else { "s" }
        )));
    }
    // The loop the surface is bounded by, as a chain of vertices: the
    // rectangle's corners with every kept point that snapped onto an edge,
    // or the convex hull of the points with its collinear points kept — so
    // no constraint passes through a vertex. The chain's edges are
    // constrained and the triangles inside it are the TIN: a super-triangle
    // this close to the points does not leave the hull's edges in place by
    // itself, and a boundary that is not the hull is what a fan across the
    // bottom cannot close over.
    let chain: Vec<u32> = match rect {
        Some(_) => {
            let corner = |k: usize| cdt.point(vertex_of[k]);
            let mut chain: Vec<u32> = Vec::new();
            for k in 0..4 {
                let (a, b) = (corner(k), corner((k + 1) % 4));
                let mut on: Vec<(i64, u32)> = Vec::new();
                for &v in vertex_of.iter().skip(4) {
                    let p = cdt.point(v);
                    if orient2d(a, b, p) != 0 {
                        continue;
                    }
                    let (lo0, hi0) = (a[0].min(b[0]), a[0].max(b[0]));
                    let (lo1, hi1) = (a[1].min(b[1]), a[1].max(b[1]));
                    if p[0] < lo0 || p[0] > hi0 || p[1] < lo1 || p[1] > hi1 {
                        continue;
                    }
                    // Along the edge: the dot product with a → b.
                    let along = (p[0] - a[0]) * (b[0] - a[0]) + (p[1] - a[1]) * (b[1] - a[1]);
                    on.push((along, v));
                }
                on.sort();
                chain.push(vertex_of[k]);
                chain.extend(on.into_iter().map(|(_, v)| v));
            }
            chain
        }
        None => convex_hull(&vertex_of.iter().map(|&v| (cdt.point(v), v)).collect::<Vec<_>>()),
    };
    let n = chain.len();
    for i in 0..n {
        cdt.constrain(chain[i], chain[(i + 1) % n]).map_err(|e| CadpoleError::invalid(format!("terrain: {e}")))?;
    }
    let inside: Option<Vec<bool>> = Some(cdt.classify());
    let mut triangles: Vec<[u32; 3]> = Vec::with_capacity(cdt.triangle_count());
    for (t, tri) in cdt.triangles().enumerate() {
        if tri.iter().any(|&v| Cdt::is_super(v)) {
            continue;
        }
        if let Some(inside) = &inside
            && !inside[t]
        {
            continue;
        }
        let tri = [point_of[tri[0] as usize], point_of[tri[1] as usize], point_of[tri[2] as usize]];
        // A sliver the snapped predicates called a triangle but the floats
        // do not: keep it, the chain and the mesh both close over it, and
        // it renders as nothing.
        triangles.push(tri);
    }
    if triangles.is_empty() {
        return Err(CadpoleError::invalid("terrain: the points are collinear; a surface needs three that are not"));
    }
    let boundary = boundary_loop(&triangles)?;
    Ok(Tin { points: kept, triangles, boundary, duplicates, outside: 0 })
}

/// The convex hull of snapped points, counter-clockwise, as vertex ids:
/// Andrew's monotone chain on the exact predicate, keeping the points that
/// lie on a hull edge so the chain can be constrained without a constraint
/// passing through a vertex.
fn convex_hull(pts: &[(IPoint, u32)]) -> Vec<u32> {
    let mut sorted: Vec<(IPoint, u32)> = pts.to_vec();
    sorted.sort();
    let chain = |order: &[(IPoint, u32)]| -> Vec<(IPoint, u32)> {
        let mut h: Vec<(IPoint, u32)> = Vec::with_capacity(order.len());
        for &p in order {
            while h.len() >= 2 && orient2d(h[h.len() - 2].0, h[h.len() - 1].0, p.0) < 0 {
                h.pop();
            }
            h.push(p);
        }
        h
    };
    let lower = chain(&sorted);
    sorted.reverse();
    let upper = chain(&sorted);
    let mut out: Vec<u32> = lower[..lower.len() - 1].iter().map(|p| p.1).collect();
    out.extend(upper[..upper.len() - 1].iter().map(|p| p.1));
    out
}

/// The directed edges that belong to exactly one triangle, chained: the
/// counter-clockwise boundary of a counter-clockwise triangulation.
fn boundary_loop(triangles: &[[u32; 3]]) -> Result<Vec<u32>> {
    let mut count: HashMap<(u32, u32), i32> = HashMap::new();
    for t in triangles {
        for i in 0..3 {
            let (a, b) = (t[i], t[(i + 1) % 3]);
            *count.entry((a.min(b), a.max(b))).or_default() += if a < b { 1 } else { -1 };
        }
    }
    // A boundary edge is used once: +1 when it runs low → high, −1 the
    // other way, so the sign gives its direction.
    let mut next: HashMap<u32, u32> = HashMap::new();
    for (&(lo, hi), &c) in &count {
        match c {
            1 => {
                next.insert(lo, hi);
            }
            -1 => {
                next.insert(hi, lo);
            }
            0 => {}
            _ => return Err(CadpoleError::kernel("terrain: a non-manifold edge in the triangulation")),
        }
    }
    let Some(&start) = next.keys().min() else {
        return Err(CadpoleError::kernel("terrain: the triangulation has no boundary"));
    };
    let mut boundary = vec![start];
    let mut at = start;
    while let Some(&n) = next.get(&at) {
        if n == start {
            break;
        }
        boundary.push(n);
        at = n;
        if boundary.len() > next.len() {
            return Err(CadpoleError::kernel("terrain: the boundary does not close"));
        }
    }
    if boundary.len() != next.len() {
        return Err(CadpoleError::kernel("terrain: the boundary is more than one loop"));
    }
    Ok(boundary)
}

impl Tin {
    /// The triangle containing `(x, y)` and its barycentric coordinates.
    fn locate(&self, x: f64, y: f64) -> Option<(usize, [f64; 3])> {
        let mut best: Option<(usize, [f64; 3], f64)> = None;
        for (t, tri) in self.triangles.iter().enumerate() {
            let [a, b, c] = [self.points[tri[0] as usize], self.points[tri[1] as usize], self.points[tri[2] as usize]];
            let area = (b.x - a.x) * (c.y - a.y) - (b.y - a.y) * (c.x - a.x);
            if area <= 0.0 {
                continue;
            }
            let wa = ((b.x - x) * (c.y - y) - (b.y - y) * (c.x - x)) / area;
            let wb = ((c.x - x) * (a.y - y) - (c.y - y) * (a.x - x)) / area;
            let wc = 1.0 - wa - wb;
            let worst = wa.min(wb).min(wc);
            // Strictly inside, or on an edge within rounding: the first
            // triangle that holds the point wins; the least-outside one
            // stands in for a point a rounding error off the boundary.
            if worst >= 0.0 {
                return Some((t, [wa, wb, wc]));
            }
            if worst >= -1e-9 && best.is_none_or(|(_, _, w)| worst > w) {
                best = Some((t, [wa, wb, wc], worst));
            }
        }
        best.map(|(t, w, _)| (t, w))
    }

    /// The elevation at `(x, y)`, or `None` outside the triangulation.
    pub fn z_at(&self, x: f64, y: f64) -> Option<f64> {
        let (t, w) = self.locate(x, y)?;
        let tri = self.triangles[t];
        Some(w[0] * self.points[tri[0] as usize].z + w[1] * self.points[tri[1] as usize].z + w[2] * self.points[tri[2] as usize].z)
    }

    /// `(∂z/∂x, ∂z/∂y)` of the triangle under `(x, y)`.
    pub fn gradient_at(&self, x: f64, y: f64) -> Option<(f64, f64)> {
        let (t, _) = self.locate(x, y)?;
        let tri = self.triangles[t];
        let [a, b, c] = [self.points[tri[0] as usize], self.points[tri[1] as usize], self.points[tri[2] as usize]];
        let area = (b.x - a.x) * (c.y - a.y) - (b.y - a.y) * (c.x - a.x);
        let dzdx = ((b.z - a.z) * (c.y - a.y) - (c.z - a.z) * (b.y - a.y)) / area;
        let dzdy = ((c.z - a.z) * (b.x - a.x) - (b.z - a.z) * (c.x - a.x)) / area;
        Some((dzdx, dzdy))
    }

    pub fn z_range(&self) -> (f64, f64) {
        z_range(&self.points)
    }

    /// The same surface raised by `dz`.
    pub fn raised(&self, dz: f64) -> Tin {
        let mut t = self.clone();
        for p in &mut t.points {
            p.z += dz;
        }
        t
    }

    /// The same surface over the rectangle `e`, exactly: every triangle is
    /// clipped to the rectangle (Sutherland–Hodgman against its four lines)
    /// and the convex pieces fanned, so every new vertex lies on an old
    /// edge or inside an old triangle's plane and `z` is unchanged wherever
    /// both surfaces are defined. A crossing of an edge with a clip line is
    /// computed from the edge's endpoints in one fixed order, so the two
    /// triangles that share the edge share the vertex to the bit and the
    /// clipped surface is watertight without a weld. A hull TIN keeps the
    /// hull's edge where it runs inside the rectangle.
    pub fn clipped(&self, e: Extent) -> Result<Tin> {
        let extent = Extent::of_points(&self.points)?;
        if !e.within(&extent) {
            return Err(CadpoleError::invalid(format!(
                "terrain: the clip rectangle ({}, {}) .. ({}, {}) must lie within the surface's ({}, {}) .. ({}, {})",
                e.x0, e.y0, e.x1, e.y1, extent.x0, extent.y0, extent.x1, extent.y1
            )));
        }
        // How far inside half-plane `k` a point is (x >= x0, x <= x1,
        // y >= y0, y <= y1), and where a segment crosses its line.
        fn side(k: usize, e: &Extent, p: Vec3) -> f64 {
            match k {
                0 => p.x - e.x0,
                1 => e.x1 - p.x,
                2 => p.y - e.y0,
                _ => e.y1 - p.y,
            }
        }
        fn crossing(k: usize, e: &Extent, a: Vec3, b: Vec3) -> Vec3 {
            let (p, q) = if (a.x, a.y, a.z) <= (b.x, b.y, b.z) { (a, b) } else { (b, a) };
            if k < 2 {
                let x = if k == 0 { e.x0 } else { e.x1 };
                let t = (x - p.x) / (q.x - p.x);
                Vec3::new(x, p.y + t * (q.y - p.y), p.z + t * (q.z - p.z))
            } else {
                let y = if k == 2 { e.y0 } else { e.y1 };
                let t = (y - p.y) / (q.y - p.y);
                Vec3::new(p.x + t * (q.x - p.x), y, p.z + t * (q.z - p.z))
            }
        }
        #[derive(Clone, Copy)]
        struct V {
            p: Vec3,
            original: Option<u32>,
        }
        let bits = |p: Vec3| (p.x.to_bits(), p.y.to_bits(), p.z.to_bits());
        let mut points: Vec<Vec3> = Vec::new();
        let mut of_original: Vec<Option<u32>> = vec![None; self.points.len()];
        let mut by_bits: HashMap<(u64, u64, u64), u32> = HashMap::new();
        let mut triangles: Vec<[u32; 3]> = Vec::new();
        for t in &self.triangles {
            let mut poly: Vec<V> = t.iter().map(|&i| V { p: self.points[i as usize], original: Some(i) }).collect();
            for k in 0..4 {
                let mut out: Vec<V> = Vec::with_capacity(poly.len() + 2);
                for i in 0..poly.len() {
                    let a = poly[i];
                    let b = poly[(i + 1) % poly.len()];
                    let (sa, sb) = (side(k, &e, a.p), side(k, &e, b.p));
                    if sa >= 0.0 {
                        out.push(a);
                    }
                    // A strict crossing; an endpoint on the line is kept as
                    // itself by the test above or the next iteration's.
                    if sa * sb < 0.0 {
                        out.push(V { p: crossing(k, &e, a.p, b.p), original: None });
                    }
                }
                poly = out;
                if poly.len() < 3 {
                    poly.clear();
                    break;
                }
            }
            poly.dedup_by(|a, b| bits(a.p) == bits(b.p));
            if poly.len() > 1 && bits(poly[0].p) == bits(poly[poly.len() - 1].p) {
                poly.pop();
            }
            if poly.len() < 3 {
                continue;
            }
            let ids: Vec<u32> = poly
                .iter()
                .map(|v| match v.original {
                    Some(i) => *of_original[i as usize].get_or_insert_with(|| {
                        points.push(self.points[i as usize]);
                        points.len() as u32 - 1
                    }),
                    None => *by_bits.entry(bits(v.p)).or_insert_with(|| {
                        points.push(v.p);
                        points.len() as u32 - 1
                    }),
                })
                .collect();
            for i in 1..ids.len() - 1 {
                let [a, b, c] = [points[ids[0] as usize], points[ids[i] as usize], points[ids[i + 1] as usize]];
                let area = (b.x - a.x) * (c.y - a.y) - (b.y - a.y) * (c.x - a.x);
                if area > 0.0 {
                    triangles.push([ids[0], ids[i], ids[i + 1]]);
                }
            }
        }
        if triangles.is_empty() {
            return Err(CadpoleError::invalid("terrain: no part of the surface lies inside the clip rectangle"));
        }
        let boundary = boundary_loop(&triangles)?;
        let outside = self.points.iter().filter(|p| !e.contains(p.x, p.y)).count();
        Ok(Tin { points, triangles, boundary, duplicates: self.duplicates, outside })
    }
}

fn z_range(points: &[Vec3]) -> (f64, f64) {
    let mut lo = f64::MAX;
    let mut hi = f64::MIN;
    for p in points {
        lo = lo.min(p.z);
        hi = hi.max(p.z);
    }
    (lo, hi)
}

// ------------------------------------------------------------------ Fit

/// The B-spline degree of a fitted surface, in both directions.
pub const FIT_DEGREE: usize = 3;

/// The fewest and the most control points per direction a fit takes.
pub const FIT_MIN_CONTROL: usize = 4;
pub const FIT_MAX_CONTROL: usize = 16;

/// The second-difference penalty on the control net, relative to the data
/// term: what keeps a knot span with no point in it from swinging. It is
/// far below anything a survey can measure and it makes the normal
/// equations positive definite whatever the points do.
pub const FIT_SMOOTHING: f64 = 1e-8;

/// A least-squares bicubic B-spline height field over an extent.
#[derive(Clone, Debug)]
pub struct Fit {
    pub extent: Extent,
    /// The clamped knot vector in `u` (along x), on `[0, 1]`.
    pub knots_u: Vec<f64>,
    /// The clamped knot vector in `v` (along y), on `[0, 1]`.
    pub knots_v: Vec<f64>,
    /// `ctrl[i][j]`: the elevation of control point `(i, j)`; its plane
    /// position is the Greville abscissa `(x_i, y_j)`.
    pub ctrl: Vec<Vec<f64>>,
    /// Root-mean-square residual over the fitted points, mm.
    pub rms: f64,
    /// The largest residual, mm.
    pub max_residual: f64,
    /// How many points were fitted (those inside the extent).
    pub fitted: usize,
    /// Points outside the extent, left out.
    pub outside: usize,
}

/// The knot span index of `t` (P&T A2.1), with `t = 1` landing in the last
/// span rather than past it.
fn find_span(knots: &[f64], p: usize, t: f64) -> usize {
    let n = knots.len() - p - 2;
    if t >= knots[n + 1] {
        return n;
    }
    if t <= knots[p] {
        return p;
    }
    let (mut lo, mut hi) = (p, n + 1);
    let mut mid = (lo + hi) / 2;
    while t < knots[mid] || t >= knots[mid + 1] {
        if t < knots[mid] {
            hi = mid;
        } else {
            lo = mid;
        }
        mid = (lo + hi) / 2;
    }
    mid
}

/// The `p + 1` basis functions that are non-zero on `span` at `t`, in
/// order `N_{span-p} .. N_{span}` (P&T A2.2).
fn basis_funs(knots: &[f64], p: usize, span: usize, t: f64) -> Vec<f64> {
    let mut n = vec![0.0; p + 1];
    let mut left = vec![0.0; p + 1];
    let mut right = vec![0.0; p + 1];
    n[0] = 1.0;
    for j in 1..=p {
        left[j] = t - knots[span + 1 - j];
        right[j] = knots[span + j] - t;
        let mut saved = 0.0;
        for r in 0..j {
            let den = right[r + 1] + left[j - r];
            let temp = if den == 0.0 { 0.0 } else { n[r] / den };
            n[r] = saved + right[r + 1] * temp;
            saved = left[j - r] * temp;
        }
        n[j] = saved;
    }
    n
}

/// The basis functions and their first derivatives on `span` at `t`.
fn basis_and_derivative(knots: &[f64], p: usize, span: usize, t: f64) -> (Vec<f64>, Vec<f64>) {
    let n = basis_funs(knots, p, span, t);
    // N'_{i,p} = p / (t_{i+p} − t_i) · N_{i,p−1} − p / (t_{i+p+1} − t_{i+1}) · N_{i+1,p−1}
    let lower = basis_funs(knots, p - 1, span, t);
    let mut d = vec![0.0; p + 1];
    for k in 0..=p {
        let i = span - p + k;
        let left = if k >= 1 { lower[k - 1] } else { 0.0 };
        let right = if k <= p - 1 { lower[k] } else { 0.0 };
        let dl = knots[i + p] - knots[i];
        let dr = knots[i + p + 1] - knots[i + 1];
        let a = if dl > 0.0 { p as f64 / dl * left } else { 0.0 };
        let b = if dr > 0.0 { p as f64 / dr * right } else { 0.0 };
        d[k] = a - b;
    }
    (n, d)
}

/// A clamped knot vector on `[0, 1]` for `c` control points of degree `p`,
/// with its interior knots uniform.
fn clamped_uniform_knots(c: usize, p: usize) -> Vec<f64> {
    let interior = c - p - 1;
    let mut k = vec![0.0; p + 1];
    for i in 1..=interior {
        k.push(i as f64 / (interior + 1) as f64);
    }
    k.extend(std::iter::repeat_n(1.0, p + 1));
    k
}

/// Greville abscissae: where a control point "is" along its parameter.
fn greville(knots: &[f64], p: usize) -> Vec<f64> {
    let c = knots.len() - p - 1;
    (0..c).map(|i| knots[i + 1..=i + p].iter().sum::<f64>() / p as f64).collect()
}

/// Solve the symmetric positive-definite system `m x = b` in place by
/// Cholesky; `m` is row-major `n × n`.
fn cholesky_solve(mut m: Vec<f64>, mut b: Vec<f64>, n: usize) -> Result<Vec<f64>> {
    for j in 0..n {
        let mut d = m[j * n + j];
        for k in 0..j {
            d -= m[j * n + k] * m[j * n + k];
        }
        if !(d > 0.0) {
            return Err(CadpoleError::kernel("terrain fit: the normal equations are not positive definite"));
        }
        let d = d.sqrt();
        m[j * n + j] = d;
        for i in j + 1..n {
            let mut s = m[i * n + j];
            for k in 0..j {
                s -= m[i * n + k] * m[j * n + k];
            }
            m[i * n + j] = s / d;
        }
    }
    // Forward: L y = b.
    for i in 0..n {
        let mut s = b[i];
        for k in 0..i {
            s -= m[i * n + k] * b[k];
        }
        b[i] = s / m[i * n + i];
    }
    // Back: Lᵀ x = y.
    for i in (0..n).rev() {
        let mut s = b[i];
        for k in i + 1..n {
            s -= m[k * n + i] * b[k];
        }
        b[i] = s / m[i * n + i];
    }
    Ok(b)
}

/// How many control points per direction `n` points earn: about two
/// points per control point over the net, within the bounds — 56 shots
/// give a 5 × 5 net, 300 a 12 × 12.
pub fn control_count(points: usize) -> usize {
    let c = ((points as f64) / 2.0).sqrt().floor() as usize;
    c.clamp(FIT_MIN_CONTROL, FIT_MAX_CONTROL)
}

/// Fit a bicubic B-spline to `points` over `extent` (their bounding
/// rectangle when `None`). The least-squares plane through the points is
/// taken out first and added back into the control net exactly, so a knot
/// span with no data continues the plane rather than falling to zero.
pub fn fit(points: &[Vec3], extent: Option<Extent>) -> Result<Fit> {
    check_points(points, "terrain")?;
    let extent = match extent {
        Some(e) => e,
        None => Extent::of_points(points)?,
    };
    let inside: Vec<Vec3> = points.iter().copied().filter(|p| extent.contains(p.x, p.y)).collect();
    let outside = points.len() - inside.len();
    if inside.len() < 3 {
        return Err(CadpoleError::invalid(format!(
            "terrain: {} point{} inside the extent; a fit needs at least 3",
            inside.len(),
            if inside.len() == 1 { "" } else { "s" }
        )));
    }
    let p = FIT_DEGREE;
    let c = control_count(inside.len());
    let knots_u = clamped_uniform_knots(c, p);
    let knots_v = clamped_uniform_knots(c, p);
    let n = c * c;
    let (pa, pb, pc) = plane_fit(&inside);
    // Normal equations AᵀA c = Aᵀ r over the residual from the plane.
    let mut m = vec![0.0; n * n];
    let mut rhs = vec![0.0; n];
    let mut row: Vec<(usize, f64)> = Vec::with_capacity((p + 1) * (p + 1));
    for q in &inside {
        let u = (q.x - extent.x0) / extent.width();
        let v = (q.y - extent.y0) / extent.height();
        let su = find_span(&knots_u, p, u);
        let sv = find_span(&knots_v, p, v);
        let nu = basis_funs(&knots_u, p, su, u);
        let nv = basis_funs(&knots_v, p, sv, v);
        row.clear();
        for (a, bu) in nu.iter().enumerate() {
            for (b, bv) in nv.iter().enumerate() {
                let i = (su - p + a) * c + (sv - p + b);
                row.push((i, bu * bv));
            }
        }
        let r = q.z - (pa * q.x + pb * q.y + pc);
        for &(i, wi) in &row {
            rhs[i] += wi * r;
            for &(j, wj) in &row {
                m[i * n + j] += wi * wj;
            }
        }
    }
    // The smoothing term: second differences along both directions of the
    // control net, weighted against the data term's size.
    let trace: f64 = (0..n).map(|i| m[i * n + i]).sum::<f64>();
    let mu = FIT_SMOOTHING * trace / n as f64;
    let idx = |i: usize, j: usize| i * c + j;
    let mut add_diff = |triple: [usize; 3]| {
        let coef = [1.0, -2.0, 1.0];
        for (a, &ia) in triple.iter().enumerate() {
            for (b, &ib) in triple.iter().enumerate() {
                m[ia * n + ib] += mu * coef[a] * coef[b];
            }
        }
    };
    for i in 0..c {
        for j in 1..c - 1 {
            add_diff([idx(i, j - 1), idx(i, j), idx(i, j + 1)]);
        }
    }
    for j in 0..c {
        for i in 1..c - 1 {
            add_diff([idx(i - 1, j), idx(i, j), idx(i + 1, j)]);
        }
    }
    let solved = cholesky_solve(m, rhs, n)?;
    let gu = greville(&knots_u, p);
    let gv = greville(&knots_v, p);
    let mut ctrl = vec![vec![0.0; c]; c];
    for i in 0..c {
        for j in 0..c {
            let x = extent.x0 + gu[i] * extent.width();
            let y = extent.y0 + gv[j] * extent.height();
            ctrl[i][j] = solved[idx(i, j)] + pa * x + pb * y + pc;
        }
    }
    let mut f = Fit { extent, knots_u, knots_v, ctrl, rms: 0.0, max_residual: 0.0, fitted: inside.len(), outside };
    let mut sum = 0.0;
    let mut worst = 0.0f64;
    for q in &inside {
        let e = f.z_at(q.x, q.y).unwrap_or(f64::NAN) - q.z;
        sum += e * e;
        worst = worst.max(e.abs());
    }
    f.rms = (sum / inside.len() as f64).sqrt();
    f.max_residual = worst;
    Ok(f)
}

impl Fit {
    /// Control points along `x` and along `y` (equal as fitted; a
    /// [`Fit::restricted`] piece can differ).
    pub fn control(&self) -> (usize, usize) {
        (self.ctrl.len(), self.ctrl[0].len())
    }

    /// Knot spans along `x` and along `y`.
    pub fn spans(&self) -> (usize, usize) {
        let (cu, cv) = self.control();
        (cu - FIT_DEGREE, cv - FIT_DEGREE)
    }

    /// The same surface over the rectangle `e` inside the extent, exactly:
    /// the patch is split at the rectangle's parameters by knot insertion
    /// (P&T A5.1, through truck's `ucut`/`vcut`), the piece re-parameterised
    /// onto `[0, 1]²`, and its control net read back. By linear precision
    /// the piece's control points sit at the new knots' Greville abscissae
    /// in `x` and `y`, so it is a `Fit` like any other and its slab's sides
    /// are still planar. The residual reported is the whole fit's.
    pub fn restricted(&self, e: Extent) -> Result<Fit> {
        if !e.within(&self.extent) {
            return Err(CadpoleError::invalid(format!(
                "terrain: the clip rectangle ({}, {}) .. ({}, {}) must lie within the extent ({}, {}) .. ({}, {})",
                e.x0, e.y0, e.x1, e.y1, self.extent.x0, self.extent.y0, self.extent.x1, self.extent.y1
            )));
        }
        // `ucut(u)` leaves `[u0, u]` in place and returns `[u, u1]`.
        const EPS: f64 = 1e-12;
        let u0 = (e.x0 - self.extent.x0) / self.extent.width();
        let u1 = (e.x1 - self.extent.x0) / self.extent.width();
        let v0 = (e.y0 - self.extent.y0) / self.extent.height();
        let v1 = (e.y1 - self.extent.y0) / self.extent.height();
        let mut s = self.truck_surface();
        if u0 > EPS {
            s = s.ucut(u0);
        }
        if u1 < 1.0 - EPS {
            let _tail = s.ucut(u1);
        }
        if v0 > EPS {
            s = s.vcut(v0);
        }
        if v1 < 1.0 - EPS {
            let _tail = s.vcut(v1);
        }
        s.knot_normalize();
        let knots_u: Vec<f64> = s.uknot_vec().iter().copied().collect();
        let knots_v: Vec<f64> = s.vknot_vec().iter().copied().collect();
        let ctrl: Vec<Vec<f64>> = s.control_points().iter().map(|row| row.iter().map(|p| p.z / p.w).collect()).collect();
        if ctrl.len() < FIT_DEGREE + 1 || ctrl[0].len() < FIT_DEGREE + 1 {
            return Err(CadpoleError::kernel("terrain: the clipped patch lost its degree"));
        }
        Ok(Fit {
            extent: e,
            knots_u,
            knots_v,
            ctrl,
            rms: self.rms,
            max_residual: self.max_residual,
            fitted: self.fitted,
            outside: self.outside,
        })
    }

    fn uv(&self, x: f64, y: f64) -> Option<(f64, f64)> {
        if !self.extent.contains(x, y) {
            return None;
        }
        Some(((x - self.extent.x0) / self.extent.width(), (y - self.extent.y0) / self.extent.height()))
    }

    /// The surface at parameters `(u, v)`.
    pub fn z_uv(&self, u: f64, v: f64) -> f64 {
        let p = FIT_DEGREE;
        let su = find_span(&self.knots_u, p, u);
        let sv = find_span(&self.knots_v, p, v);
        let nu = basis_funs(&self.knots_u, p, su, u);
        let nv = basis_funs(&self.knots_v, p, sv, v);
        let mut z = 0.0;
        for (a, bu) in nu.iter().enumerate() {
            for (b, bv) in nv.iter().enumerate() {
                z += bu * bv * self.ctrl[su - p + a][sv - p + b];
            }
        }
        z
    }

    /// The elevation at `(x, y)`, or `None` outside the extent.
    pub fn z_at(&self, x: f64, y: f64) -> Option<f64> {
        let (u, v) = self.uv(x, y)?;
        Some(self.z_uv(u, v))
    }

    /// `(∂z/∂x, ∂z/∂y)` at `(x, y)`, analytically.
    pub fn gradient_at(&self, x: f64, y: f64) -> Option<(f64, f64)> {
        let (u, v) = self.uv(x, y)?;
        let p = FIT_DEGREE;
        let su = find_span(&self.knots_u, p, u);
        let sv = find_span(&self.knots_v, p, v);
        let (nu, du) = basis_and_derivative(&self.knots_u, p, su, u);
        let (nv, dv) = basis_and_derivative(&self.knots_v, p, sv, v);
        let (mut zu, mut zv) = (0.0, 0.0);
        for a in 0..=p {
            for b in 0..=p {
                let cp = self.ctrl[su - p + a][sv - p + b];
                zu += du[a] * nv[b] * cp;
                zv += nu[a] * dv[b] * cp;
            }
        }
        Some((zu / self.extent.width(), zv / self.extent.height()))
    }

    /// The range the surface can take: the control net's, which bounds it.
    pub fn control_range(&self) -> (f64, f64) {
        let mut lo = f64::MAX;
        let mut hi = f64::MIN;
        for row in &self.ctrl {
            for &z in row {
                lo = lo.min(z);
                hi = hi.max(z);
            }
        }
        (lo, hi)
    }

    /// The same surface raised by `dz`.
    pub fn raised(&self, dz: f64) -> Fit {
        let mut f = self.clone();
        for row in &mut f.ctrl {
            for z in row {
                *z += dz;
            }
        }
        f
    }

    /// The plane position of control point `(i, j)`: the Greville
    /// abscissae, which is what makes `x(u)` and `y(v)` linear.
    fn control_xy(&self, i: usize, j: usize) -> (f64, f64) {
        let gu = greville(&self.knots_u, FIT_DEGREE);
        let gv = greville(&self.knots_v, FIT_DEGREE);
        (self.extent.x0 + gu[i] * self.extent.width(), self.extent.y0 + gv[j] * self.extent.height())
    }

    /// The surface as truck holds it: a rational patch with unit weights.
    pub fn truck_surface(&self) -> BSplineSurface<Vector4> {
        let (cu, cv) = self.control();
        let mut pts = vec![vec![Vector4::new(0.0, 0.0, 0.0, 1.0); cv]; cu];
        for i in 0..cu {
            for j in 0..cv {
                let (x, y) = self.control_xy(i, j);
                pts[i][j] = Vector4::new(x, y, self.ctrl[i][j], 1.0);
            }
        }
        BSplineSurface::new((KnotVec::from(self.knots_u.clone()), KnotVec::from(self.knots_v.clone())), pts)
    }
}

// ------------------------------------------------------------- Terrain

/// A ground surface: which construction, and the surface itself.
#[derive(Clone, Debug)]
pub enum Ground {
    Tin(Tin),
    Fit(Fit),
}

/// A terrain: a height field over its extent, possibly raised.
#[derive(Clone, Debug)]
pub struct Terrain {
    pub ground: Ground,
    pub extent: Extent,
    /// How far the surface has been raised above the data (`offset_up`).
    pub offset: f64,
}

impl Terrain {
    /// Build a terrain from spot elevations by `method`.
    pub fn new(points: &[Vec3], method: Method, extent: Option<Extent>) -> Result<Terrain> {
        match method {
            Method::Tin => {
                let tin = triangulate(points, extent)?;
                let extent = match extent {
                    Some(e) => e,
                    None => Extent::of_points(&tin.points)?,
                };
                Ok(Terrain { ground: Ground::Tin(tin), extent, offset: 0.0 })
            }
            Method::Fit => {
                let f = fit(points, extent)?;
                let extent = f.extent;
                Ok(Terrain { ground: Ground::Fit(f), extent, offset: 0.0 })
            }
        }
    }

    pub fn method(&self) -> Method {
        match &self.ground {
            Ground::Tin(_) => Method::Tin,
            Ground::Fit(_) => Method::Fit,
        }
    }

    /// The elevation at `(x, y)`, raised by the offset; `None` off the surface.
    pub fn z_at(&self, x: f64, y: f64) -> Option<f64> {
        let z = match &self.ground {
            Ground::Tin(t) => t.z_at(x, y)?,
            Ground::Fit(f) => f.z_at(x, y)?,
        };
        Some(z + self.offset)
    }

    /// `(∂z/∂x, ∂z/∂y)` at `(x, y)`.
    pub fn gradient_at(&self, x: f64, y: f64) -> Option<(f64, f64)> {
        match &self.ground {
            Ground::Tin(t) => t.gradient_at(x, y),
            Ground::Fit(f) => f.gradient_at(x, y),
        }
    }

    /// The grade at `(x, y)`: rise over run, `0.25` for a 25 % slope.
    pub fn slope_at(&self, x: f64, y: f64) -> Option<f64> {
        let (a, b) = self.gradient_at(x, y)?;
        Some((a * a + b * b).sqrt())
    }

    /// The direction of steepest descent at `(x, y)`, a unit vector in the
    /// plane; `None` on flat ground.
    pub fn downhill_at(&self, x: f64, y: f64) -> Option<Vec3> {
        let (a, b) = self.gradient_at(x, y)?;
        let len = (a * a + b * b).sqrt();
        (len > 0.0).then(|| Vec3::new(-a / len, -b / len, 0.0))
    }

    /// The average grade over `polygon`: the slope of the least-squares
    /// plane through the surface sampled on a grid inside it (about 400
    /// samples), rise over run. `None` when no sample lands inside.
    pub fn average_grade(&self, polygon: &[P2]) -> Option<f64> {
        let (a, b) = self.plane_over(polygon)?;
        Some((a * a + b * b).sqrt())
    }

    /// The lowest and highest elevation over `polygon`, from the surface
    /// sampled on the same grid as [`Terrain::average_grade`] and at the
    /// polygon's own vertices — a sampling, so a peak between samples can
    /// sit a little outside the range. `None` when nothing lands on the
    /// surface.
    pub fn z_range_over(&self, polygon: &[P2]) -> Option<(f64, f64)> {
        if polygon.len() < 3 {
            return None;
        }
        let mut samples = self.grid_samples(polygon);
        samples.extend(polygon.iter().filter_map(|p| self.z_at(p.x, p.y).map(|z| Vec3::new(p.x, p.y, z))));
        if samples.is_empty() {
            return None;
        }
        Some(z_range(&samples))
    }

    /// The least-squares plane's gradient over `polygon`.
    fn plane_over(&self, polygon: &[P2]) -> Option<(f64, f64)> {
        let samples = self.grid_samples(polygon);
        if samples.len() < 3 {
            return None;
        }
        let (a, b, _) = plane_fit(&samples);
        Some((a, b))
    }

    /// The surface on a grid of about 400 points over `polygon`'s bounding
    /// box, keeping those inside the polygon and on the surface.
    fn grid_samples(&self, polygon: &[P2]) -> Vec<Vec3> {
        if polygon.len() < 3 {
            return Vec::new();
        }
        let (mut x0, mut y0, mut x1, mut y1) = (f64::MAX, f64::MAX, f64::MIN, f64::MIN);
        for p in polygon {
            x0 = x0.min(p.x);
            y0 = y0.min(p.y);
            x1 = x1.max(p.x);
            y1 = y1.max(p.y);
        }
        let w = x1 - x0;
        let h = y1 - y0;
        if !(w > 0.0 && h > 0.0) {
            return Vec::new();
        }
        // About 400 samples, cells as square as the box allows.
        let cells = 400.0;
        let nx = ((cells * w / h).sqrt().round() as usize).clamp(4, 200);
        let ny = ((cells * h / w).sqrt().round() as usize).clamp(4, 200);
        let mut samples = Vec::new();
        for i in 0..nx {
            for j in 0..ny {
                let x = x0 + (i as f64 + 0.5) / nx as f64 * w;
                let y = y0 + (j as f64 + 0.5) / ny as f64 * h;
                if !point_in_polygon(polygon, x, y) {
                    continue;
                }
                if let Some(z) = self.z_at(x, y) {
                    samples.push(Vec3::new(x, y, z));
                }
            }
        }
        samples
    }

    /// The same surface over the rectangle `e` inside the extent: a fit
    /// split there by knot insertion ([`Fit::restricted`]), a TIN's
    /// triangles clipped ([`Tin::clipped`]) — exact in both cases, `z_at`
    /// agreeing to rounding wherever both are defined — so a lot's slab
    /// comes out of a site's surface with no boolean.
    pub fn clip(&self, e: Extent) -> Result<Terrain> {
        if !e.within(&self.extent) {
            return Err(CadpoleError::invalid(format!(
                "terrain: the clip rectangle ({}, {}) .. ({}, {}) must lie within the extent ({}, {}) .. ({}, {})",
                e.x0, e.y0, e.x1, e.y1, self.extent.x0, self.extent.y0, self.extent.x1, self.extent.y1
            )));
        }
        let ground = match &self.ground {
            Ground::Tin(t) => Ground::Tin(t.clipped(e)?),
            Ground::Fit(f) => Ground::Fit(f.restricted(e)?),
        };
        Ok(Terrain { ground, extent: e, offset: self.offset })
    }

    /// The surface's elevation range (the control net's for a fit, which
    /// bounds it), raised by the offset.
    pub fn z_range(&self) -> (f64, f64) {
        let (lo, hi) = match &self.ground {
            Ground::Tin(t) => t.z_range(),
            Ground::Fit(f) => f.control_range(),
        };
        (lo + self.offset, hi + self.offset)
    }

    /// The same surface raised by `h`.
    pub fn offset_up(&self, h: f64) -> Terrain {
        Terrain { ground: self.ground.clone(), extent: self.extent, offset: self.offset + h }
    }

    /// Where a body's base plane goes when the script does not say: below
    /// the lowest point by a tenth of the relief, and by half a metre at
    /// least.
    pub fn default_base(&self) -> f64 {
        let (lo, hi) = self.z_range();
        lo - (0.1 * (hi - lo)).max(500.0)
    }

    /// The residual of a fit, `(rms, max)`; `None` for a TIN.
    pub fn residual(&self) -> Option<(f64, f64)> {
        match &self.ground {
            Ground::Tin(_) => None,
            Ground::Fit(f) => Some((f.rms, f.max_residual)),
        }
    }

    /// Contour lines at every multiple of `interval` within the surface's
    /// range: polylines on the surface (`z` exact on a TIN's edges; on a
    /// fit, marched over a `FIT_CONTOUR_GRID`² sampling of it, so a line is
    /// within a cell of the true isoline).
    pub fn contours(&self, interval: f64) -> Result<Vec<Curve>> {
        if !(interval > 0.0) || !interval.is_finite() {
            return Err(CadpoleError::invalid(format!("terrain: the contour interval must be positive, got {interval}")));
        }
        let (lo, hi) = self.z_range();
        if (hi - lo) / interval > MAX_CONTOURS as f64 {
            return Err(CadpoleError::invalid(format!(
                "terrain: {} contours at {interval} mm over {} mm of relief; the most is {MAX_CONTOURS}",
                ((hi - lo) / interval).ceil(),
                hi - lo
            )));
        }
        let (points, triangles): (Vec<Vec3>, Vec<[u32; 3]>) = match &self.ground {
            Ground::Tin(t) => (t.points.iter().map(|p| Vec3::new(p.x, p.y, p.z + self.offset)).collect(), t.triangles.clone()),
            Ground::Fit(f) => {
                let n = FIT_CONTOUR_GRID;
                let mut pts = Vec::with_capacity((n + 1) * (n + 1));
                for i in 0..=n {
                    for j in 0..=n {
                        let u = i as f64 / n as f64;
                        let v = j as f64 / n as f64;
                        let x = f.extent.x0 + u * f.extent.width();
                        let y = f.extent.y0 + v * f.extent.height();
                        pts.push(Vec3::new(x, y, f.z_uv(u, v) + self.offset));
                    }
                }
                let at = |i: usize, j: usize| (i * (n + 1) + j) as u32;
                let mut tris = Vec::with_capacity(2 * n * n);
                for i in 0..n {
                    for j in 0..n {
                        tris.push([at(i, j), at(i + 1, j), at(i + 1, j + 1)]);
                        tris.push([at(i, j), at(i + 1, j + 1), at(i, j + 1)]);
                    }
                }
                (pts, tris)
            }
        };
        let mut out = Vec::new();
        let first = (lo / interval).ceil() as i64;
        let last = (hi / interval).floor() as i64;
        for k in first..=last {
            let level = k as f64 * interval;
            out.extend(contour_at(&points, &triangles, level));
        }
        Ok(out)
    }

    /// The mesh body of the surface as a slab down to `base`: the
    /// triangles on top (a TIN's own; a fit's sampled at
    /// `FIT_CONTOUR_GRID`²), a fan across the bottom and a skirt round the
    /// boundary, in three named groups `<prefix>/top`, `/bottom`, `/side`.
    pub fn mesh_body(&self, base: f64, prefix: &str) -> Result<MeshBody> {
        let (lo, _) = self.z_range();
        if !(base < lo) {
            return Err(CadpoleError::invalid(format!(
                "terrain: the base {base} must lie below the lowest point of the surface ({lo})"
            )));
        }
        let (points, triangles, boundary) = match &self.ground {
            Ground::Tin(t) => (
                t.points.iter().map(|p| Vec3::new(p.x, p.y, p.z + self.offset)).collect::<Vec<_>>(),
                t.triangles.clone(),
                t.boundary.clone(),
            ),
            Ground::Fit(_) => {
                return Err(CadpoleError::invalid("terrain: a fitted surface makes an exact body, not a mesh"));
            }
        };
        Ok(slab_mesh(&points, &triangles, &boundary, base, prefix))
    }

    /// The exact body of a fitted surface as a slab down to `base`: the
    /// B-spline patch on top, four planar sides on the extent's vertical
    /// planes, a planar bottom. Faces in the order `bottom`, `side[0..4]`
    /// (south, east, north, west), `top`.
    pub fn brep_body(&self, base: f64) -> Result<KSolid> {
        let Ground::Fit(f) = &self.ground else {
            return Err(CadpoleError::invalid("terrain: a TIN makes a mesh body, not an exact one"));
        };
        let (lo, _) = self.z_range();
        if !(base < lo) {
            return Err(CadpoleError::invalid(format!(
                "terrain: the base {base} must lie below the lowest point of the surface ({lo})"
            )));
        }
        let f = f.raised(self.offset);
        crate::kernel::guarded("terrain", std::panic::AssertUnwindSafe(|| brep_slab(&f, base)))?
    }

    /// The face names of [`Terrain::brep_body`]'s solid, in face order.
    pub fn brep_face_names(prefix: &str) -> Vec<(String, Vec<String>)> {
        let mut names = vec![(format!("{prefix}/bottom"), Vec::new())];
        for k in 0..4 {
            names.push((format!("{prefix}/side[{k}]"), Vec::new()));
        }
        names.push((format!("{prefix}/top"), Vec::new()));
        names
    }
}

/// The most contour lines one call draws.
pub const MAX_CONTOURS: usize = 500;

/// The grid a fitted surface is sampled on for its contours and its mesh.
pub const FIT_CONTOUR_GRID: usize = 96;

/// Even-odd point-in-polygon.
fn point_in_polygon(poly: &[P2], x: f64, y: f64) -> bool {
    let mut inside = false;
    let n = poly.len();
    let mut j = n - 1;
    for i in 0..n {
        let (a, b) = (poly[i], poly[j]);
        if (a.y > y) != (b.y > y) {
            let xi = a.x + (y - a.y) / (b.y - a.y) * (b.x - a.x);
            if x < xi {
                inside = !inside;
            }
        }
        j = i;
    }
    inside
}

/// The isoline of `level` over a triangulation, as chained polylines.
///
/// Every crossing lies on a mesh edge, so a crossing is keyed by its edge
/// and two segments that meet on an edge meet exactly, whatever rounding
/// did to the point: the chains close where the surface closes.
fn contour_at(points: &[Vec3], triangles: &[[u32; 3]], level: f64) -> Vec<Curve> {
    type EdgeKey = (u32, u32);
    let key = |a: u32, b: u32| -> EdgeKey { (a.min(b), a.max(b)) };
    let crossing = |a: u32, b: u32| -> Vec3 {
        let (pa, pb) = (points[a as usize], points[b as usize]);
        let t = (level - pa.z) / (pb.z - pa.z);
        let t = t.clamp(0.0, 1.0);
        Vec3::new(pa.x + t * (pb.x - pa.x), pa.y + t * (pb.y - pa.y), level)
    };
    let mut position: HashMap<EdgeKey, Vec3> = HashMap::new();
    let mut links: HashMap<EdgeKey, Vec<EdgeKey>> = HashMap::new();
    for t in triangles {
        let above = [points[t[0] as usize].z >= level, points[t[1] as usize].z >= level, points[t[2] as usize].z >= level];
        let mut crossed: Vec<EdgeKey> = Vec::with_capacity(2);
        for i in 0..3 {
            let (a, b) = (t[i], t[(i + 1) % 3]);
            if above[i] != above[(i + 1) % 3] {
                let k = key(a, b);
                position.entry(k).or_insert_with(|| crossing(a, b));
                crossed.push(k);
            }
        }
        if crossed.len() == 2 {
            links.entry(crossed[0]).or_default().push(crossed[1]);
            links.entry(crossed[1]).or_default().push(crossed[0]);
        }
    }
    // Walk the chains: open ones from an end first, then whatever is left
    // closes on itself.
    let mut visited: HashMap<EdgeKey, bool> = HashMap::new();
    let mut keys: Vec<EdgeKey> = links.keys().copied().collect();
    keys.sort();
    let mut out = Vec::new();
    let walk = |start: EdgeKey, visited: &mut HashMap<EdgeKey, bool>| -> Option<Curve> {
        if visited.get(&start).copied().unwrap_or(false) {
            return None;
        }
        let mut chain = vec![start];
        visited.insert(start, true);
        let mut prev = start;
        let mut at = start;
        loop {
            let next = links[&at].iter().copied().find(|n| *n != prev && !visited.get(n).copied().unwrap_or(false));
            match next {
                Some(n) => {
                    chain.push(n);
                    visited.insert(n, true);
                    prev = at;
                    at = n;
                }
                None => break,
            }
        }
        let closed = chain.len() > 2 && links[&at].contains(&start);
        if chain.len() < 2 {
            return None;
        }
        let pts: Vec<Vec3> = chain.iter().map(|k| position[k]).collect();
        let mut segments = Vec::with_capacity(pts.len());
        for w in pts.windows(2) {
            if (w[1] - w[0]).length() > 0.0 {
                segments.push(Segment3::Line { a: w[0], b: w[1] });
            }
        }
        if closed && (pts[0] - pts[pts.len() - 1]).length() > 0.0 {
            segments.push(Segment3::Line { a: pts[pts.len() - 1], b: pts[0] });
        }
        if segments.is_empty() {
            return None;
        }
        Some(Curve { start: pts[0], segments, closed })
    };
    for &k in keys.iter().filter(|k| links[*k].len() == 1) {
        if let Some(c) = walk(k, &mut visited) {
            out.push(c);
        }
    }
    for &k in &keys {
        if let Some(c) = walk(k, &mut visited) {
            out.push(c);
        }
    }
    out
}

/// A watertight slab: `top` over the triangles, `bottom` a fan across the
/// boundary at `base`, `side` a quad per boundary edge. Vertices are not
/// shared between the three groups, so each carries its own normals, and
/// the mesh kernel's weld closes the seams as it does for every
/// tessellated body.
fn slab_mesh(points: &[Vec3], triangles: &[[u32; 3]], boundary: &[u32], base: f64, prefix: &str) -> MeshBody {
    let mut mesh = Mesh::default();
    let push = |mesh: &mut Mesh, p: Vec3, n: Vec3| -> u32 {
        let i = mesh.positions.len() / 3;
        mesh.positions.extend_from_slice(&[p.x as f32, p.y as f32, p.z as f32]);
        mesh.normals.extend_from_slice(&[n.x as f32, n.y as f32, n.z as f32]);
        i as u32
    };
    // Top: area-weighted vertex normals over the surface.
    let mut normals = vec![Vec3::ZERO; points.len()];
    for t in triangles {
        let [a, b, c] = [points[t[0] as usize], points[t[1] as usize], points[t[2] as usize]];
        let n = (b - a).cross(c - a);
        for &v in t {
            normals[v as usize] = normals[v as usize] + n;
        }
    }
    let top_base = mesh.positions.len() as u32 / 3;
    for (p, n) in points.iter().zip(&normals) {
        let n = if n.length() > 0.0 { n.normalized() } else { Vec3::Z };
        push(&mut mesh, *p, n);
    }
    let mut groups: Vec<(String, u32, u32)> = Vec::new();
    let mut tri_count = 0u32;
    for t in triangles {
        mesh.indices.extend_from_slice(&[top_base + t[0], top_base + t[1], top_base + t[2]]);
        mesh.face_ids.push(0);
        tri_count += 1;
    }
    groups.push((format!("{prefix}/top"), 0, tri_count));
    // Bottom: a fan from the boundary's centroid (inside it, the boundary
    // being convex), wound to face −z.
    let start = mesh.face_ids.len() as u32;
    let m = boundary.len();
    let mut centre = Vec3::ZERO;
    for &v in boundary {
        centre = centre + points[v as usize];
    }
    let centre = push(&mut mesh, Vec3::new(centre.x / m as f64, centre.y / m as f64, base), Vec3::new(0.0, 0.0, -1.0));
    let bottom: Vec<u32> = boundary
        .iter()
        .map(|&v| {
            let p = points[v as usize];
            push(&mut mesh, Vec3::new(p.x, p.y, base), Vec3::new(0.0, 0.0, -1.0))
        })
        .collect();
    for k in 0..m {
        mesh.indices.extend_from_slice(&[centre, bottom[(k + 1) % m], bottom[k]]);
        mesh.face_ids.push(1);
    }
    groups.push((format!("{prefix}/bottom"), start, mesh.face_ids.len() as u32 - start));
    // Side: one quad per boundary edge, outward.
    let start = mesh.face_ids.len() as u32;
    for k in 0..m {
        let a = points[boundary[k] as usize];
        let b = points[boundary[(k + 1) % m] as usize];
        let down = Vec3::new(0.0, 0.0, -1.0);
        let n = down.cross(Vec3::new(b.x - a.x, b.y - a.y, 0.0));
        let n = if n.length() > 0.0 { n.normalized() } else { Vec3::X };
        let a_top = push(&mut mesh, a, n);
        let b_top = push(&mut mesh, b, n);
        let b_bot = push(&mut mesh, Vec3::new(b.x, b.y, base), n);
        let a_bot = push(&mut mesh, Vec3::new(a.x, a.y, base), n);
        mesh.indices.extend_from_slice(&[a_top, a_bot, b_bot, a_top, b_bot, b_top]);
        mesh.face_ids.push(2);
        mesh.face_ids.push(2);
    }
    groups.push((format!("{prefix}/side"), start, mesh.face_ids.len() as u32 - start));
    let faces = groups
        .into_iter()
        .map(|(name, tri_start, tri_count)| {
            let mut area = 0.0;
            let mut centroid = Vec3::ZERO;
            let mut normal = Vec3::ZERO;
            for k in tri_start..tri_start + tri_count {
                if let Some(tri) = mesh.triangle(k as usize) {
                    let n = (tri[1] - tri[0]).cross(tri[2] - tri[0]);
                    let a = n.length() / 2.0;
                    area += a;
                    centroid = centroid + (tri[0] + tri[1] + tri[2]) * (a / 3.0);
                    normal = normal + n;
                }
            }
            FaceInfo {
                name,
                surface_kind: "group",
                centroid: if area > 0.0 { centroid * (1.0 / area) } else { centroid },
                normal: if normal.length() > 0.0 { normal.normalized() } else { Vec3::ZERO },
                area,
                tri_start,
                tri_count,
                ..Default::default()
            }
        })
        .collect();
    (mesh, Topology { faces, edges: Vec::new(), vertices: Vec::new() })
}

/// A planar face on `wires` whose outward normal points along `want`.
fn planar_face(wires: Vec<Wire>, want: Vec3, what: &str) -> Result<Face> {
    let f = builder::try_attach_plane(&wires).map_err(|e| CadpoleError::kernel(format!("terrain {what}: {e}")))?;
    let outward = match f.oriented_surface() {
        Surface::Plane(p) => Vec3::from_vector(p.normal()),
        _ => want,
    };
    Ok(if outward.dot(want) < 0.0 { f.inverse() } else { f })
}

/// The exact slab under a fitted surface.
fn brep_slab(f: &Fit, base: f64) -> Result<KSolid> {
    brep_slab_surface(f.truck_surface(), base)
}

/// The exact slab under a height-field patch whose control net has its
/// `x` linear in `u` and its `y` linear in `v` — a fit's, or one cut out
/// of a fit's by knot insertion — down to `base`.
fn brep_slab_surface(surface: BSplineSurface<Vector4>, base: f64) -> Result<KSolid> {
    let pts = surface.control_points();
    let cu = pts.len();
    let cv = pts[0].len();
    let corner = |i: usize, j: usize| {
        let p = pts[i][j];
        Point3::new(p.x, p.y, p.z)
    };
    // Corners: t0 (x0, y0), t1 (x1, y0), t2 (x1, y1), t3 (x0, y1) on top,
    // b0..b3 straight below them.
    let t = [
        builder::vertex(corner(0, 0)),
        builder::vertex(corner(cu - 1, 0)),
        builder::vertex(corner(cu - 1, cv - 1)),
        builder::vertex(corner(0, cv - 1)),
    ];
    let below = |i: usize, j: usize| {
        let p = pts[i][j];
        builder::vertex(Point3::new(p.x, p.y, base))
    };
    let b = [below(0, 0), below(cu - 1, 0), below(cu - 1, cv - 1), below(0, cv - 1)];
    let ku = surface.uknot_vec().clone();
    let kv = surface.vknot_vec().clone();
    let spline = |from: &Vertex, to: &Vertex, knots: &KnotVec, cps: Vec<Vector4>| -> Result<Edge> {
        let curve =
            BSplineCurve::try_new(knots.clone(), cps).map_err(|e| CadpoleError::kernel(format!("terrain edge: {e}")))?;
        Ok(Edge::new(from, to, TCurve::NurbsCurve(NurbsCurve::new(curve))))
    };
    // The four boundary curves, each in the parameter's increasing sense.
    let south = spline(&t[0], &t[1], &ku, (0..cu).map(|i| pts[i][0]).collect())?;
    let east = spline(&t[1], &t[2], &kv, (0..cv).map(|j| pts[cu - 1][j]).collect())?;
    let north = spline(&t[3], &t[2], &ku, (0..cu).map(|i| pts[i][cv - 1]).collect())?;
    let west = spline(&t[0], &t[3], &kv, (0..cv).map(|j| pts[0][j]).collect())?;
    let up = [
        builder::line(&b[0], &t[0]),
        builder::line(&b[1], &t[1]),
        builder::line(&b[2], &t[2]),
        builder::line(&b[3], &t[3]),
    ];
    let floor = [
        builder::line(&b[0], &b[1]),
        builder::line(&b[1], &b[2]),
        builder::line(&b[2], &b[3]),
        builder::line(&b[3], &b[0]),
    ];
    let wire = |edges: Vec<Edge>| -> Wire { edges.into() };
    let mut shell = Shell::new();
    shell.push(planar_face(
        vec![wire(vec![floor[3].inverse(), floor[2].inverse(), floor[1].inverse(), floor[0].inverse()])],
        Vec3::new(0.0, 0.0, -1.0),
        "bottom",
    )?);
    shell.push(planar_face(
        vec![wire(vec![floor[0].clone(), up[1].clone(), south.inverse(), up[0].inverse()])],
        Vec3::new(0.0, -1.0, 0.0),
        "south side",
    )?);
    shell.push(planar_face(
        vec![wire(vec![floor[1].clone(), up[2].clone(), east.inverse(), up[1].inverse()])],
        Vec3::new(1.0, 0.0, 0.0),
        "east side",
    )?);
    shell.push(planar_face(
        vec![wire(vec![floor[2].clone(), up[3].clone(), north.clone(), up[2].inverse()])],
        Vec3::new(0.0, 1.0, 0.0),
        "north side",
    )?);
    shell.push(planar_face(
        vec![wire(vec![floor[3].clone(), up[0].clone(), west.clone(), up[3].inverse()])],
        Vec3::new(-1.0, 0.0, 0.0),
        "west side",
    )?);
    let top = Face::try_new(
        vec![wire(vec![south, east, north.inverse(), west.inverse()])],
        Surface::NurbsSurface(NurbsSurface::new(surface)),
    )
    .map_err(|e| CadpoleError::kernel(format!("terrain top: {e}")))?;
    shell.push(top);
    Solid::try_new(vec![shell]).map_err(|e| CadpoleError::kernel(format!("terrain: the slab did not close ({e})")))
}

/// Points along contour polylines, for a terrain from contours: every
/// vertex, plus points along any segment longer than `spacing` so the TIN
/// follows the line. Each polyline carries its own `z` on its points.
pub fn sample_contours(polylines: &[Vec<Vec3>], spacing: f64) -> Result<Vec<Vec3>> {
    if !(spacing > 0.0) || !spacing.is_finite() {
        return Err(CadpoleError::invalid(format!("terrain_from_contours: spacing must be positive, got {spacing}")));
    }
    let mut out = Vec::new();
    for line in polylines {
        for (k, p) in line.iter().enumerate() {
            out.push(*p);
            if k + 1 < line.len() {
                let q = line[k + 1];
                let d = (q - *p).length();
                let steps = (d / spacing).floor() as usize;
                for s in 1..=steps {
                    let t = s as f64 * spacing / d;
                    if t < 1.0 - 1e-9 {
                        out.push(*p + (q - *p) * t);
                    }
                }
            }
        }
    }
    Ok(out)
}

/// A spacing for [`sample_contours`] when the script gives none: the
/// median length of the polylines' segments.
pub fn default_contour_spacing(polylines: &[Vec<Vec3>]) -> f64 {
    let mut lengths: Vec<f64> = polylines.iter().flat_map(|l| l.windows(2).map(|w| (w[1] - w[0]).length())).collect();
    lengths.retain(|l| *l > 0.0);
    if lengths.is_empty() {
        return 1.0;
    }
    lengths.sort_by(|a, b| a.partial_cmp(b).unwrap_or(std::cmp::Ordering::Equal));
    lengths[lengths.len() / 2]
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::solid;
    use truck_modeling::ParametricSurface;

    /// A deterministic scatter over `[0, w] × [0, h]`: a fixed-seed LCG, so
    /// the tests are the same on every target.
    fn scatter(n: usize, w: f64, h: f64, z: impl Fn(f64, f64) -> f64) -> Vec<Vec3> {
        let mut s: u64 = 0x9e37_79b9_7f4a_7c15;
        let mut next = || {
            s = s.wrapping_mul(6364136223846793005).wrapping_add(1442695040888963407);
            ((s >> 11) as f64) / ((1u64 << 53) as f64)
        };
        let mut pts = Vec::with_capacity(n + 4);
        for &(x, y) in &[(0.0, 0.0), (w, 0.0), (w, h), (0.0, h)] {
            pts.push(Vec3::new(x, y, z(x, y)));
        }
        for _ in 0..n {
            let x = next() * w;
            let y = next() * h;
            pts.push(Vec3::new(x, y, z(x, y)));
        }
        pts
    }

    #[test]
    fn a_tin_of_a_plane_reproduces_it_exactly() {
        let pts = scatter(60, 20000.0, 30000.0, |x, y| 100.0 + 0.3 * x - 0.1 * y);
        let t = Terrain::new(&pts, Method::Tin, None).unwrap();
        for (x, y) in [(1000.0, 2000.0), (12345.0, 6789.0), (19999.0, 29999.0), (0.0, 0.0)] {
            let z = t.z_at(x, y).unwrap();
            assert!((z - (100.0 + 0.3 * x - 0.1 * y)).abs() < 1e-9, "z at ({x}, {y}) = {z}");
            let (a, b) = t.gradient_at(x, y).unwrap();
            assert!((a - 0.3).abs() < 1e-9 && (b + 0.1).abs() < 1e-9, "gradient {a}, {b}");
        }
        assert!(t.z_at(-1.0, 0.0).is_none(), "outside the hull");
        let slope = t.slope_at(5000.0, 5000.0).unwrap();
        assert!((slope - (0.09f64 + 0.01).sqrt()).abs() < 1e-9, "{slope}");
        let lot = [P2::new(0.0, 0.0), P2::new(20000.0, 0.0), P2::new(20000.0, 30000.0), P2::new(0.0, 30000.0)];
        let g = t.average_grade(&lot).unwrap();
        assert!((g - (0.09f64 + 0.01).sqrt()).abs() < 1e-6, "{g}");
    }

    #[test]
    fn a_fit_of_a_paraboloid_is_within_its_residual_and_its_gradient_is_analytic() {
        let f = |x: f64, y: f64| 1000.0 + 2e-4 * (x - 10000.0) * (x - 10000.0) + 1e-4 * (y - 15000.0) * (y - 15000.0);
        let pts = scatter(300, 20000.0, 30000.0, f);
        let t = Terrain::new(&pts, Method::Fit, None).unwrap();
        let (rms, worst) = t.residual().unwrap();
        // A quadratic is in the cubic space: the residual is rounding plus
        // the smoothing term's bias, both far below a millimetre on a
        // surface 1–50 m high.
        assert!(rms < 0.05, "rms {rms}");
        assert!(worst < 0.5, "max {worst}");
        for (x, y) in [(3000.0, 4000.0), (10000.0, 15000.0), (17500.0, 26000.0)] {
            let z = t.z_at(x, y).unwrap();
            assert!((z - f(x, y)).abs() < 0.5, "z at ({x}, {y}) = {z}, want {}", f(x, y));
            let (a, b) = t.gradient_at(x, y).unwrap();
            let (wa, wb) = (4e-4 * (x - 10000.0), 2e-4 * (y - 15000.0));
            assert!((a - wa).abs() < 2e-3 && (b - wb).abs() < 2e-3, "gradient ({a}, {b}) want ({wa}, {wb})");
        }
        let Ground::Fit(fit) = &t.ground else { unreachable!() };
        assert_eq!(fit.control(), (12, 12), "sqrt(304 / 2) = 12");
        assert_eq!(fit.spans(), (9, 9));
        // truck evaluates the same patch to the same numbers.
        let s = fit.truck_surface();
        for (u, v) in [(0.0, 0.0), (0.37, 0.81), (1.0, 1.0), (0.5, 0.0)] {
            let p = s.subs(u, v);
            let z = fit.z_uv(u, v);
            assert!((p.z / p.w - z).abs() < 1e-6, "({u}, {v}): truck {} vs {z}", p.z / p.w);
            assert!((p.x / p.w - (u * 20000.0)).abs() < 1e-6, "x is linear in u");
            assert!((p.y / p.w - (v * 30000.0)).abs() < 1e-6, "y is linear in v");
        }
    }

    #[test]
    fn z_at_on_a_slope_and_the_offset_are_what_they_say() {
        let pts = scatter(40, 14000.0, 21000.0, |_, y| 50000.0 - 0.31 * y);
        for method in Method::ALL {
            let t = Terrain::new(&pts, method, None).unwrap();
            let z = t.z_at(7000.0, 10000.0).unwrap();
            assert!((z - (50000.0 - 3100.0)).abs() < 1.0, "{}: {z}", method.name());
            let up = t.offset_up(9144.0);
            assert!((up.z_at(7000.0, 10000.0).unwrap() - z - 9144.0).abs() < 1e-9);
            assert!((t.slope_at(3000.0, 3000.0).unwrap() - 0.31).abs() < 1e-3, "{}", method.name());
            let down = t.downhill_at(3000.0, 3000.0).unwrap();
            assert!(down.y > 0.999, "downhill is +y: {down:?}");
        }
    }

    #[test]
    fn the_contours_of_a_cone_are_circles_of_the_right_radii() {
        // A cone, apex 1000 up at the centre of a 20 m square, 1:10 slope.
        let f = |x: f64, y: f64| 1000.0 - 0.1 * ((x - 10000.0) * (x - 10000.0) + (y - 10000.0) * (y - 10000.0)).sqrt();
        let pts = scatter(2000, 20000.0, 20000.0, f);
        let t = Terrain::new(&pts, Method::Tin, None).unwrap();
        let curves = t.contours(200.0).unwrap();
        // Levels 200, 400, 600, 800 close inside the square (radius 8, 6,
        // 4, 2 m); 0 is at the edge, 1000 the apex.
        let mut closed: Vec<(f64, f64)> = curves
            .iter()
            .filter(|c| c.closed)
            .map(|c| {
                let pts = c.vertices();
                let r: f64 = pts
                    .iter()
                    .map(|p| ((p.x - 10000.0) * (p.x - 10000.0) + (p.y - 10000.0) * (p.y - 10000.0)).sqrt())
                    .sum::<f64>()
                    / pts.len() as f64;
                (c.start.z, r)
            })
            .collect();
        closed.sort_by(|a, b| a.0.partial_cmp(&b.0).unwrap());
        for &(z, r) in &closed {
            let want = (1000.0 - z) / 0.1;
            assert!((r - want).abs() < 0.03 * want, "level {z}: radius {r}, want {want}");
        }
        let levels: Vec<f64> = closed.iter().map(|c| c.0).collect();
        for want in [200.0, 400.0, 600.0, 800.0] {
            assert!(levels.iter().any(|z| (z - want).abs() < 1e-9), "a closed contour at {want}: {levels:?}");
        }
        // The fit's contours are isolines of the smooth surface, chained
        // over the sampling grid.
        let tf = Terrain::new(&pts, Method::Fit, None).unwrap();
        let cf = tf.contours(200.0).unwrap();
        assert!(cf.iter().filter(|c| c.closed).count() >= 3, "{} curves", cf.len());
    }

    #[test]
    fn the_mesh_slab_is_a_solid_and_offset_up_adds_area_times_height() {
        let pts = scatter(80, 20000.0, 30000.0, |x, y| 3000.0 + 0.2 * x + 0.05 * y);
        let t = Terrain::new(&pts, Method::Tin, None).unwrap();
        let base = t.default_base();
        let (m, topo) = t.mesh_body(base, "grade").unwrap();
        let r = solid::validate(&m);
        assert!(r.is_solid(), "{}", r.summary());
        assert_eq!(topo.faces.iter().map(|f| f.name.as_str()).collect::<Vec<_>>(), ["grade/top", "grade/bottom", "grade/side"]);
        let v0 = m.volume();
        assert!(v0 > 0.0);
        let (m1, _) = t.offset_up(1234.5).mesh_body(base, "grade").unwrap();
        let dv = m1.volume() - v0;
        assert!((dv - 20000.0 * 30000.0 * 1234.5).abs() < 1e-6 * dv, "dv {dv}");
    }

    #[test]
    fn the_exact_slab_is_a_solid_and_offset_up_adds_area_times_height() {
        let pts = scatter(120, 20000.0, 30000.0, |x, y| 3000.0 + 0.2 * x + 0.05 * y + 1e-5 * x * y / 30.0);
        let t = Terrain::new(&pts, Method::Fit, None).unwrap();
        let base = t.default_base();
        let s = t.brep_body(base).unwrap();
        assert_eq!(crate::kernel::face_count(&s), 6);
        let (m, _) = crate::tessellate::tessellate(&s, 0.05).unwrap();
        let r = solid::validate(&m);
        assert!(r.is_solid(), "{}", r.summary());
        let v0 = m.volume();
        let s1 = t.offset_up(1000.0).brep_body(base).unwrap();
        let (m1, _) = crate::tessellate::tessellate(&s1, 0.05).unwrap();
        let dv = m1.volume() - v0;
        assert!((dv - 20000.0 * 30000.0 * 1000.0).abs() < 2e-3 * dv, "dv {dv}");
    }

    #[test]
    fn an_extent_clips_the_tin_to_the_rectangle_and_extrapolates_its_corners() {
        let pts = scatter(100, 20000.0, 30000.0, |x, y| 100.0 + 0.3 * x - 0.1 * y);
        // A rectangle inside the data, and one reaching outside it.
        let inner = Extent::new(2000.0, 3000.0, 15000.0, 25000.0).unwrap();
        let t = Terrain::new(&pts, Method::Tin, Some(inner)).unwrap();
        let Ground::Tin(tin) = &t.ground else { unreachable!() };
        assert!(tin.outside > 0);
        for p in &tin.points {
            assert!(inner.contains(p.x, p.y), "{p:?}");
        }
        assert!(t.z_at(1999.0, 5000.0).is_none());
        let z = t.z_at(14999.0, 24999.0).unwrap();
        assert!((z - (100.0 + 0.3 * 14999.0 - 0.1 * 24999.0)).abs() < 1e-6, "{z}");
        let outer = Extent::new(-5000.0, -5000.0, 25000.0, 35000.0).unwrap();
        let t = Terrain::new(&pts, Method::Tin, Some(outer)).unwrap();
        let z = t.z_at(-4000.0, -4000.0).unwrap();
        assert!((z - (100.0 + 0.3 * -4000.0 - 0.1 * -4000.0)).abs() < 1e-6, "extrapolated corner: {z}");
        let (m, _) = t.mesh_body(t.default_base(), "g").unwrap();
        assert!(solid::validate(&m).is_solid());
    }

    #[test]
    fn a_fit_restricted_by_knot_insertion_is_the_same_surface_and_still_a_slab() {
        let f = |x: f64, y: f64| 1000.0 + 2e-4 * (x - 10000.0) * (x - 10000.0) + 1e-4 * (y - 15000.0) * (y - 15000.0) + 0.05 * x;
        let pts = scatter(300, 20000.0, 30000.0, f);
        let t = Terrain::new(&pts, Method::Fit, None).unwrap();
        let inner = Extent::new(3000.0, 4000.0, 15000.0, 22000.0).unwrap();
        let c = t.clip(inner).unwrap();
        assert_eq!(c.extent, inner);
        let Ground::Fit(piece) = &c.ground else { unreachable!() };
        // The piece keeps the spans the rectangle covers and no more.
        assert!(piece.control().0 < 12 && piece.control().1 < 12, "{:?}", piece.control());
        for i in 0..=12 {
            for j in 0..=12 {
                let x = inner.x0 + inner.width() * i as f64 / 12.0;
                let y = inner.y0 + inner.height() * j as f64 / 12.0;
                let (a, b) = (t.z_at(x, y).unwrap(), c.z_at(x, y).unwrap());
                assert!((a - b).abs() < 1e-7, "({x}, {y}): {a} vs {b}");
                let (ga, gb) = (t.gradient_at(x, y).unwrap(), c.gradient_at(x, y).unwrap());
                assert!((ga.0 - gb.0).abs() < 1e-9 && (ga.1 - gb.1).abs() < 1e-9, "gradient at ({x}, {y})");
            }
        }
        assert!(c.z_at(2999.0, 10000.0).is_none(), "off the piece");
        // Linear precision survives the cut: the piece's control net sits at
        // its own Greville abscissae, which is what keeps the slab's sides
        // planar.
        let s = piece.truck_surface();
        let (cu, cv) = piece.control();
        for i in 0..cu {
            for j in 0..cv {
                let p = s.control_points()[i][j];
                let (x, y) = piece.control_xy(i, j);
                assert!((p.x / p.w - x).abs() < 1e-6 && (p.y / p.w - y).abs() < 1e-6, "control ({i}, {j})");
            }
        }
        let solid = c.brep_body(c.default_base()).unwrap();
        assert_eq!(crate::kernel::face_count(&solid), 6);
        let (m, _) = crate::tessellate::tessellate(&solid, 1.0).unwrap();
        assert!(solid::validate(&m).is_solid());
        // A rectangle sharing an edge with the extent, and one outside it.
        let edge = Extent::new(0.0, 4000.0, 15000.0, 30000.0).unwrap();
        let ce = t.clip(edge).unwrap();
        assert!((ce.z_at(0.0, 30000.0).unwrap() - t.z_at(0.0, 30000.0).unwrap()).abs() < 1e-7);
        let e = t.clip(Extent::new(-1.0, 0.0, 5000.0, 5000.0).unwrap()).unwrap_err().to_string();
        assert!(e.contains("must lie within the extent"), "{e}");
        // Raised, then clipped, is clipped, then raised.
        let up = t.offset_up(500.0).clip(inner).unwrap();
        assert!((up.z_at(9000.0, 9000.0).unwrap() - c.z_at(9000.0, 9000.0).unwrap() - 500.0).abs() < 1e-7);
    }

    #[test]
    fn a_tin_clipped_to_a_rectangle_is_the_same_surface_and_its_slab_is_a_solid() {
        // Not a plane, so the clip has to reproduce curvature: a saddle and
        // a bilinear term on top of the slope (no transcendental).
        let f = |x: f64, y: f64| {
            3000.0 + 0.2 * x - 0.1 * y + 1e-5 * x * y / 30.0 + 3e-6 * (x - 9000.0) * (x - 9000.0) - 2e-6 * (y - 14000.0) * (y - 14000.0)
        };
        let pts = scatter(150, 20000.0, 30000.0, f);
        let outer = Extent::new(-1000.0, -1000.0, 21000.0, 31000.0).unwrap();
        let t = Terrain::new(&pts, Method::Tin, Some(outer)).unwrap();
        let inner = Extent::new(2500.0, 3500.0, 14500.0, 21500.0).unwrap();
        let c = t.clip(inner).unwrap();
        let Ground::Tin(tin) = &c.ground else { unreachable!() };
        let Ground::Tin(whole) = &t.ground else { unreachable!() };
        assert!(tin.outside > 0 && tin.triangles.len() < whole.triangles.len());
        for p in &tin.points {
            assert!(inner.contains(p.x, p.y), "{p:?} outside the clip");
        }
        // The boundary is the rectangle: every boundary vertex on its edge,
        // all four corners present.
        for &v in &tin.boundary {
            let p = tin.points[v as usize];
            let on = (p.x - inner.x0).abs() < 1e-9 || (p.x - inner.x1).abs() < 1e-9 || (p.y - inner.y0).abs() < 1e-9 || (p.y - inner.y1).abs() < 1e-9;
            assert!(on, "boundary vertex {p:?} off the rectangle");
        }
        for corner in inner.corners() {
            assert!(tin.points.iter().any(|p| (p.x - corner.x).abs() < 1e-9 && (p.y - corner.y).abs() < 1e-9), "corner {corner:?}");
        }
        for i in 0..=15 {
            for j in 0..=15 {
                let x = inner.x0 + inner.width() * i as f64 / 15.0;
                let y = inner.y0 + inner.height() * j as f64 / 15.0;
                let (a, b) = (t.z_at(x, y).unwrap(), c.z_at(x, y).unwrap());
                assert!((a - b).abs() < 1e-7, "({x}, {y}): {a} vs {b}");
            }
        }
        assert!(c.z_at(2499.0, 10000.0).is_none());
        let (m, topo) = c.mesh_body(c.default_base(), "lot").unwrap();
        let r = solid::validate(&m);
        assert!(r.is_solid(), "{}", r.summary());
        assert_eq!(topo.faces.iter().map(|f| f.name.as_str()).collect::<Vec<_>>(), ["lot/top", "lot/bottom", "lot/side"]);
        // A hull TIN keeps its hull edge inside the rectangle: the clip of a
        // rectangle that reaches past the hull is the hull's part of it.
        let hull = Terrain::new(&pts[4..], Method::Tin, None).unwrap();
        let reach = Extent::new(hull.extent.x0, hull.extent.y0, hull.extent.x0 + 6000.0, hull.extent.y0 + 8000.0).unwrap();
        let hc = hull.clip(reach).unwrap();
        assert!(hc.z_at(hull.extent.x0 + 1.0, hull.extent.y0 + 1.0).is_none() || hull.z_at(hull.extent.x0 + 1.0, hull.extent.y0 + 1.0).is_some());
        let (m, _) = hc.mesh_body(hc.default_base(), "h").unwrap();
        assert!(solid::validate(&m).is_solid());
        let e = t.clip(Extent::new(-2000.0, 0.0, 5000.0, 5000.0).unwrap()).unwrap_err().to_string();
        assert!(e.contains("must lie within"), "{e}");
    }

    #[test]
    fn z_range_over_a_polygon_brackets_the_surface() {
        let pts = scatter(60, 20000.0, 30000.0, |x, y| 100.0 + 0.3 * x - 0.1 * y);
        for method in Method::ALL {
            let t = Terrain::new(&pts, method, None).unwrap();
            let lot = [P2::new(1000.0, 2000.0), P2::new(9000.0, 2000.0), P2::new(9000.0, 12000.0), P2::new(1000.0, 12000.0)];
            let (lo, hi) = t.z_range_over(&lot).unwrap();
            // A plane's extremes over a rectangle are at its corners, which
            // the sampling includes.
            let tol = if method == Method::Tin { 1e-9 } else { 0.5 };
            assert!((lo - (100.0 + 300.0 - 1200.0)).abs() < tol, "{}: lo {lo}", method.name());
            assert!((hi - (100.0 + 2700.0 - 200.0)).abs() < tol, "{}: hi {hi}", method.name());
            assert!(t.z_range_over(&lot[..2]).is_none());
        }
    }

    #[test]
    fn contour_points_sample_long_segments() {
        let lines = vec![vec![Vec3::new(0.0, 0.0, 5.0), Vec3::new(10.0, 0.0, 5.0)], vec![Vec3::new(0.0, 3.0, 6.0), Vec3::new(1.0, 3.0, 6.0)]];
        let pts = sample_contours(&lines, 2.5).unwrap();
        assert_eq!(pts.len(), 2 + 3 + 2, "{pts:?}");
        assert!((default_contour_spacing(&lines) - 10.0).abs() < 1e-9 || (default_contour_spacing(&lines) - 1.0).abs() < 1e-9);
    }

    #[test]
    fn refusals_name_the_problem() {
        let e = Terrain::new(&[Vec3::ZERO, Vec3::X], Method::Tin, None).unwrap_err().to_string();
        assert!(e.contains("at least 3 points"), "{e}");
        let e = Terrain::new(&[Vec3::ZERO, Vec3::X, Vec3::X * 2.0], Method::Tin, None).unwrap_err().to_string();
        assert!(e.contains("positive width and height") || e.contains("collinear"), "{e}");
        let pts = scatter(10, 100.0, 100.0, |_, _| 1.0);
        let t = Terrain::new(&pts, Method::Tin, None).unwrap();
        let e = t.mesh_body(5.0, "g").unwrap_err().to_string();
        assert!(e.contains("below the lowest point"), "{e}");
        assert!(Method::parse("spline").unwrap_err().to_string().contains("valid: tin, fit"));
    }
}
