//! Shadow footprints, sightlines and view cones: the geometry
//! behind `shadow_study`, `sightline`, `sightlines` and `view_cone`.
//!
//! **A shadow on a plane is exact for the tessellation.** The shadow a body
//! throws on a plane along the sun's direction is the union of its
//! triangles projected onto that plane — each one clipped to the sun's
//! side of the plane first, since a triangle below the plane casts nothing
//! on it — and that union is a polygon boolean (`i_overlay`, the same
//! engine `fill::rebuild` uses), so the footprint is the true silhouette
//! extruded along the sun direction and cut by the plane, with no
//! sampling in it. A receiving *face* clips the footprint to its own
//! outline (its triangles, unioned the same way); a bare plane receives
//! all of it. The mesh is the geometry for a mesh body and a chord
//! approximation for an exact one, so the footprint is exact to the
//! tessellation tolerance and no further.
//!
//! **A shadow on terrain is sampled.** A receiving mesh is not a plane, so
//! every one of its triangles that faces the sun asks a spread of points
//! — [`TERRAIN_SAMPLES`] over the whole receiver, shared out by area with
//! at least four per triangle, low-discrepancy within each — whether a ray
//! toward the sun meets a caster, and takes that fraction of its area as
//! shadowed. The answer is a set of triangles with their shaded fractions
//! and an area whose error is the sampling's: about one part in the
//! square root of the sample count along the shadow's edge.
//!
//! **A sightline is one ray** through the renderer's BVH
//! ([`crate::render::bvh`]), from the eye to the target, stopping at the
//! first body it meets; a fan is many of them, to points spread over a
//! target by area, and counts those that reach their point before anything
//! else; a view cone is a grid of rays over a window of azimuth and
//! elevation, and counts those that reach the sky.

use i_overlay::core::fill_rule::FillRule;
use i_overlay::core::overlay_rule::OverlayRule;
use i_overlay::float::simplify::SimplifyShape;
use i_overlay::float::single::SingleFloatOverlay;

use crate::fmath;
use crate::geom::{Plane, Vec3};
use crate::render::bvh::{Bvh, Tri};
use crate::sketch::P2;
use crate::tessellate::Mesh;

/// A polygon region in a plane's 2D coordinates: shapes, each an outer
/// contour (counter-clockwise) followed by its holes (clockwise), as
/// `i_overlay` returns them.
pub type Shapes = Vec<Vec<Vec<[f64; 2]>>>;

/// The shadow one or more casters throw on a plane.
#[derive(Clone, Debug, Default, PartialEq)]
pub struct Footprint {
    /// The footprint, clipped to the receiver when there was one.
    pub shapes: Shapes,
    /// Its area, mm².
    pub area: f64,
    /// The plane's normal faces the sun, so the plane is lit at all. A
    /// receiver whose normal points away from the sun is in its own
    /// shadow, and the footprint is empty.
    pub lit: bool,
}

fn signed_area(c: &[[f64; 2]]) -> f64 {
    let n = c.len();
    if n < 3 {
        return 0.0;
    }
    let mut a = 0.0;
    for i in 0..n {
        let (p, q) = (c[i], c[(i + 1) % n]);
        a += p[0] * q[1] - q[0] * p[1];
    }
    a * 0.5
}

/// The net area of a set of shapes: outers count, holes count against.
pub fn shapes_area(shapes: &Shapes) -> f64 {
    shapes.iter().flat_map(|s| s.iter()).map(|c| signed_area(c)).sum::<f64>().max(0.0)
}

fn ccw(mut c: Vec<[f64; 2]>) -> Vec<[f64; 2]> {
    if signed_area(&c) < 0.0 {
        c.reverse();
    }
    c
}

/// The union of a pile of possibly overlapping polygons, each wound
/// counter-clockwise: non-zero winding merges them.
fn union(contours: Vec<Vec<[f64; 2]>>) -> Shapes {
    let contours: Vec<Vec<[f64; 2]>> = contours.into_iter().filter(|c| c.len() >= 3 && signed_area(c) > 0.0).collect();
    if contours.is_empty() {
        return Vec::new();
    }
    contours.simplify_shape_as::<i64>(FillRule::NonZero)
}

fn to2(plane: &Plane, p: Vec3) -> [f64; 2] {
    let r = p - plane.origin;
    [r.dot(plane.x_dir), r.dot(plane.y_dir())]
}

/// The triangles of `mesh` in the range `tris` (all of them when `None`),
/// each clipped to the sun's side of the plane and projected onto it along
/// the sun direction, in the plane's 2D coordinates and wound
/// counter-clockwise. `n` is the plane's normal as oriented toward the sun
/// and `cos` the sun direction's component along it (positive).
fn projected(mesh: &Mesh, tris: Option<(usize, usize)>, plane: &Plane, n: Vec3, sun: Vec3, cos: f64, eps: f64) -> Vec<Vec<[f64; 2]>> {
    let (start, end) = tris.unwrap_or((0, mesh.triangle_count()));
    let mut out = Vec::with_capacity(end.saturating_sub(start));
    for t in start..end.min(mesh.triangle_count()) {
        let Some(tri) = mesh.triangle(t) else { continue };
        // Sutherland–Hodgman against the half-space s >= eps, where s is
        // the signed distance toward the sun.
        let s: Vec<f64> = tri.iter().map(|p| (*p - plane.origin).dot(n)).collect();
        let mut poly: Vec<Vec3> = Vec::with_capacity(4);
        for i in 0..3 {
            let (a, b) = (tri[i], tri[(i + 1) % 3]);
            let (sa, sb) = (s[i], s[(i + 1) % 3]);
            let (ina, inb) = (sa >= eps, sb >= eps);
            if ina {
                poly.push(a);
            }
            if ina != inb {
                let t = (eps - sa) / (sb - sa);
                poly.push(a + (b - a) * t);
            }
        }
        if poly.len() < 3 {
            continue;
        }
        // Slide each point down the sun direction onto the plane.
        let c: Vec<[f64; 2]> = poly
            .iter()
            .map(|p| {
                let s = (*p - plane.origin).dot(n);
                to2(plane, *p - sun * (s / cos))
            })
            .collect();
        out.push(ccw(c));
    }
    out
}

/// The receiver plane's normal turned toward the sun, and the cosine
/// between them; `None` when the sun is at or below the plane.
fn toward_sun(plane: &Plane, sun: Vec3, oriented: bool) -> Option<(Vec3, f64)> {
    let n = plane.normal.normalized();
    let c = sun.normalized().dot(n);
    if oriented {
        if c <= 1e-9 { None } else { Some((n, c)) }
    } else if c.abs() <= 1e-9 {
        None
    } else if c < 0.0 {
        Some((-n, -c))
    } else {
        Some((n, c))
    }
}

/// The shadow `casters` throw on `plane` under a sun along `sun` (unit,
/// toward the sun). With `oriented`, the plane's normal is the receiving
/// side (a face) and a sun behind it lights nothing; otherwise (a bare
/// plane) whichever side faces the sun receives. `receiver` clips the
/// footprint to a region of the plane, in its 2D coordinates.
pub fn footprint_on_plane(casters: &[&Mesh], sun: Vec3, plane: &Plane, oriented: bool, receiver: Option<&Shapes>, tolerance: f64) -> Footprint {
    let Some((n, cos)) = toward_sun(plane, sun, oriented) else {
        return Footprint { shapes: Vec::new(), area: 0.0, lit: false };
    };
    let sun = sun.normalized();
    // Everything at or above the plane casts, a triangle lying on it
    // casting itself: the clip is exact, not padded, or a body standing on
    // the plane loses a sliver of its base. `tolerance` is the mesh's own
    // chord error, which the footprint carries whatever is done here.
    let _ = tolerance;
    let eps = 0.0;
    let mut contours = Vec::new();
    for m in casters {
        contours.extend(projected(m, None, plane, n, sun, cos, eps));
    }
    let shadow = union(contours);
    let shapes = match receiver {
        Some(r) if !shadow.is_empty() && !r.is_empty() => shadow.overlay_as::<i64>(r, OverlayRule::Intersect, FillRule::NonZero),
        Some(_) => Vec::new(),
        None => shadow,
    };
    let area = shapes_area(&shapes);
    Footprint { shapes, area, lit: true }
}

/// A planar face of a mesh as a region of its plane: the union of the
/// triangles `tris` (a `FaceInfo`'s `tri_start..tri_start + tri_count`)
/// in the plane's 2D coordinates.
pub fn face_region(mesh: &Mesh, tris: (usize, usize), plane: &Plane) -> Shapes {
    let (start, end) = tris;
    let mut contours = Vec::with_capacity(end.saturating_sub(start));
    for t in start..end.min(mesh.triangle_count()) {
        let Some(tri) = mesh.triangle(t) else { continue };
        contours.push(ccw(tri.iter().map(|p| to2(plane, *p)).collect()));
    }
    union(contours)
}

/// A 2D region lifted back into space on its plane, one loop per contour.
pub fn shapes_to_world(shapes: &Shapes, plane: &Plane) -> Vec<Vec<Vec3>> {
    shapes
        .iter()
        .flat_map(|s| s.iter())
        .map(|c| c.iter().map(|p| plane.to_world(p[0], p[1])).collect())
        .collect()
}

/// A 2D contour as sketch points.
pub fn contour_points(c: &[[f64; 2]]) -> Vec<P2> {
    c.iter().map(|p| P2::new(p[0], p[1])).collect()
}

/// The shadow `casters` throw on a mesh (terrain), sampled per triangle.
#[derive(Clone, Debug, Default, PartialEq)]
pub struct MeshShadow {
    /// Receiver triangles at least partly shadowed, with the fraction of
    /// each (the four samples' share) that is.
    pub shaded: Vec<(usize, f64)>,
    /// Shadowed area, mm²: each triangle's area times its fraction.
    pub area: f64,
    /// The area of the receiver that faces the sun at all.
    pub lit_area: f64,
}

/// How many sample points a terrain shadow spreads over the receiver's
/// sun-facing area, at least four to a triangle.
pub const TERRAIN_SAMPLES: usize = 4096;

/// Rays from every sun-facing triangle of `receiver` toward the sun,
/// against `casters`.
pub fn footprint_on_mesh(casters: &[&Mesh], sun: Vec3, receiver: &Mesh) -> MeshShadow {
    let sun = sun.normalized();
    let occ = Occluders::new(casters);
    let d = [sun.x as f32, sun.y as f32, sun.z as f32];
    let mut out = MeshShadow::default();
    let diag = receiver.bbox().map(|(lo, hi)| (hi - lo).length()).unwrap_or(1.0);
    let eps = (diag * 1e-5).max(1e-6) as f32;
    // The sun-facing triangles and their areas, so the samples can be
    // shared out by area.
    let mut facing: Vec<(usize, Vec3, f64)> = Vec::new();
    for t in 0..receiver.triangle_count() {
        let Some(tri) = receiver.triangle(t) else { continue };
        let n = (tri[1] - tri[0]).cross(tri[2] - tri[0]);
        let area = n.length() * 0.5;
        if area <= 0.0 {
            continue;
        }
        let n = n * (1.0 / (2.0 * area));
        if n.dot(sun) <= 0.0 {
            continue;
        }
        out.lit_area += area;
        facing.push((t, n, area));
    }
    if out.lit_area <= 0.0 {
        return out;
    }
    const PHI: f64 = 0.618_033_988_749_895;
    const PSI: f64 = 0.754_877_666_246_693;
    for (t, n, area) in facing {
        let tri = receiver.triangle(t).expect("listed above");
        let count = ((TERRAIN_SAMPLES as f64 * area / out.lit_area).round() as usize).max(4);
        let mut hits = 0;
        for i in 0..count {
            let (mut a, mut b) = (((i as f64 + 0.5) * PHI).fract(), ((i as f64 + 0.5) * PSI).fract());
            if a + b > 1.0 {
                a = 1.0 - a;
                b = 1.0 - b;
            }
            let p = tri[0] + (tri[1] - tri[0]) * a + (tri[2] - tri[0]) * b + n * (eps as f64 * 4.0);
            let o = [p.x as f32, p.y as f32, p.z as f32];
            if occ.bvh.occluded(o, d, eps, f32::MAX) {
                hits += 1;
            }
        }
        if hits > 0 {
            let f = hits as f64 / count as f64;
            out.shaded.push((t, f));
            out.area += area * f;
        }
    }
    out
}

/// A set of meshes a ray can be cast through.
pub struct Occluders {
    pub bvh: Bvh,
}

/// What a ray met first.
#[derive(Clone, Copy, Debug, PartialEq)]
pub struct Blocker {
    /// Index into the mesh list the occluders were built from.
    pub body: usize,
    /// Triangle index within that mesh.
    pub tri: usize,
    pub point: Vec3,
    /// From the ray's origin, mm.
    pub distance: f64,
}

impl Occluders {
    /// Over every triangle of every mesh, `body` being the mesh's index.
    pub fn new(meshes: &[&Mesh]) -> Occluders {
        let mut tris = Vec::new();
        for (ii, m) in meshes.iter().enumerate() {
            for (t, tri) in m.indices.chunks_exact(3).enumerate() {
                let p = |i: u32| {
                    let i = i as usize * 3;
                    [m.positions[i], m.positions[i + 1], m.positions[i + 2]]
                };
                let (p0, p1, p2) = (p(tri[0]), p(tri[1]), p(tri[2]));
                let e1 = [p1[0] - p0[0], p1[1] - p0[1], p1[2] - p0[2]];
                let e2 = [p2[0] - p0[0], p2[1] - p0[1], p2[2] - p0[2]];
                let n = [e1[1] * e2[2] - e1[2] * e2[1], e1[2] * e2[0] - e1[0] * e2[2], e1[0] * e2[1] - e1[1] * e2[0]];
                let nn = n[0] * n[0] + n[1] * n[1] + n[2] * n[2];
                if nn <= 0.0 || !nn.is_finite() {
                    continue;
                }
                tris.push(Tri { p0, e1, e2, item: ii as u32, tri: t as u32 });
            }
        }
        Occluders { bvh: Bvh::from_triangles(tris) }
    }

    /// The first thing on the segment from `from` to `to`, if anything.
    pub fn cast(&self, from: Vec3, to: Vec3) -> Option<Blocker> {
        let v = to - from;
        let dist = v.length();
        if dist <= 0.0 {
            return None;
        }
        self.cast_ray(from, v * (1.0 / dist), dist)
    }

    /// The first thing along `dir` (unit) from `from` within `max`.
    pub fn cast_ray(&self, from: Vec3, dir: Vec3, max: f64) -> Option<Blocker> {
        let o = [from.x as f32, from.y as f32, from.z as f32];
        let d = [dir.x as f32, dir.y as f32, dir.z as f32];
        // A whisker past the origin, so a ray leaving a surface does not
        // meet it again; a tenth of a micron when the reach is unbounded.
        // The BVH walks with finite bounds, so "unbounded" is a thousand
        // kilometres, farther than any model reaches in millimetres.
        let max = if max.is_finite() { max } else { 1e9 };
        let eps = ((max.max(1.0)) * 1e-6).clamp(1e-5, 1e-3) as f32;
        let hit = self.bvh.intersect(o, d, eps, max as f32)?;
        let tri = self.bvh.tri(hit.tri);
        Some(Blocker {
            body: tri.item as usize,
            tri: tri.tri as usize,
            point: from + dir * (hit.t as f64),
            distance: hit.t as f64,
        })
    }

    /// Whether a ray from `from` to the point `to` on the surface of mesh
    /// `target` reaches it: nothing nearer along the segment, the target
    /// itself included (its far side hides a point on it).
    pub fn reaches(&self, from: Vec3, to: Vec3, target: usize) -> Option<Blocker> {
        let v = to - from;
        let dist = v.length();
        if dist <= 0.0 {
            return None;
        }
        let b = self.cast_ray(from, v * (1.0 / dist), dist * (1.0 + 1e-4))?;
        // A hit on the target within a whisker of the point is the point.
        if b.body == target && b.distance >= dist * (1.0 - 1e-3) - 1e-6 {
            return None;
        }
        Some(b)
    }
}

/// `n` points spread over the triangles `tris` of `mesh` in proportion to
/// their area, deterministically (a golden-ratio sequence over the
/// cumulative area, and low-discrepancy barycentrics within each).
pub fn surface_samples(mesh: &Mesh, tris: Option<(usize, usize)>, n: usize) -> Vec<Vec3> {
    let (start, end) = tris.unwrap_or((0, mesh.triangle_count()));
    let end = end.min(mesh.triangle_count());
    let mut cdf = Vec::with_capacity(end.saturating_sub(start));
    let mut total = 0.0;
    for t in start..end {
        if let Some(tri) = mesh.triangle(t) {
            total += (tri[1] - tri[0]).cross(tri[2] - tri[0]).length() * 0.5;
        }
        cdf.push(total);
    }
    if total <= 0.0 || n == 0 {
        return Vec::new();
    }
    const PHI: f64 = 0.618_033_988_749_895;
    const PSI: f64 = 0.754_877_666_246_693; // plastic-number sequence
    let mut out = Vec::with_capacity(n);
    for i in 0..n {
        let u = ((i as f64 + 0.5) * PHI).fract() * total;
        let k = cdf.partition_point(|&c| c < u).min(cdf.len() - 1);
        let Some(tri) = mesh.triangle(start + k) else { continue };
        let (mut a, mut b) = (((i as f64 + 0.5) * PSI).fract(), ((i as f64 + 0.5) * PSI * PSI).fract());
        if a + b > 1.0 {
            a = 1.0 - a;
            b = 1.0 - b;
        }
        out.push(tri[0] + (tri[1] - tri[0]) * a + (tri[2] - tri[0]) * b);
    }
    out
}

/// A fan of sightlines: how many of `points` (each on mesh `target`) the
/// eye at `from` reaches, and which bodies stop the rest, by count.
#[derive(Clone, Debug, Default, PartialEq)]
pub struct Fan {
    pub visible: usize,
    pub total: usize,
    /// Body index → rays it stopped.
    pub blocked_by: Vec<(usize, usize)>,
}

impl Fan {
    pub fn fraction(&self) -> f64 {
        if self.total == 0 { 0.0 } else { self.visible as f64 / self.total as f64 }
    }
}

pub fn fan(occ: &Occluders, from: Vec3, points: &[Vec3], target: Option<usize>) -> Fan {
    let mut out = Fan { total: points.len(), ..Default::default() };
    for &p in points {
        let blocker = match target {
            Some(t) => occ.reaches(from, p, t),
            None => occ.cast(from, p),
        };
        match blocker {
            None => out.visible += 1,
            Some(b) => match out.blocked_by.iter_mut().find(|(body, _)| *body == b.body) {
                Some(e) => e.1 += 1,
                None => out.blocked_by.push((b.body, 1)),
            },
        }
    }
    out.blocked_by.sort_by(|a, b| b.1.cmp(&a.1).then(a.0.cmp(&b.0)));
    out
}

/// A window of sky: compass azimuths `az_lo..az_hi` and elevations
/// `el_lo..el_hi`, in degrees, sampled on a `cols × rows` grid of rays
/// from `from`; `north_degrees` turns the compass into the model. A ray
/// that meets nothing within `range` (mm) sees the sky.
pub fn view_cone(occ: &Occluders, from: Vec3, az: (f64, f64), el: (f64, f64), grid: (usize, usize), range: f64, north_degrees: f64) -> Fan {
    let (cols, rows) = (grid.0.max(1), grid.1.max(1));
    let mut out = Fan { total: cols * rows, ..Default::default() };
    for j in 0..rows {
        let e = el.0 + (el.1 - el.0) * ((j as f64 + 0.5) / rows as f64);
        for i in 0..cols {
            let a = az.0 + (az.1 - az.0) * ((i as f64 + 0.5) / cols as f64);
            let dir = crate::sun::compass_to_model(crate::sun::direction(a, e), north_degrees);
            match occ.cast_ray(from, dir, range) {
                None => out.visible += 1,
                Some(b) => match out.blocked_by.iter_mut().find(|(body, _)| *body == b.body) {
                    Some(x) => x.1 += 1,
                    None => out.blocked_by.push((b.body, 1)),
                },
            }
        }
    }
    out.blocked_by.sort_by(|a, b| b.1.cmp(&a.1).then(a.0.cmp(&b.0)));
    out
}

/// The elevation angle, in degrees, from `from` to `to`: what a target
/// subtends above the observer's horizon.
pub fn elevation_to(from: Vec3, to: Vec3) -> f64 {
    let v = to - from;
    let h = fmath::hypot(v.x, v.y);
    fmath::atan2(v.z, h).to_degrees()
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::render::testing::box_mesh;

    fn v(x: f64, y: f64, z: f64) -> Vec3 {
        Vec3::new(x, y, z)
    }

    /// A 10 mm cube on the ground under a sun from the south-west at 45°
    /// throws a footprint whose area is exact: its own 10 × 10 base plus
    /// the two 10 × 10 parallelograms its east... the footprint of a unit
    /// cube's shadow along (−1, −1, 1)/√3 down onto z = 0 is a hexagon of
    /// area 3 (side² + 2 × side × drop), which for 10 mm is 300 mm².
    #[test]
    fn a_cube_throws_an_exact_hexagon() {
        let cube = box_mesh([0.0, 0.0, 0.0], [10.0, 10.0, 10.0]);
        let sun = v(1.0, 1.0, 1.0).normalized();
        let f = footprint_on_plane(&[&cube], sun, &Plane::XY, false, None, 0.05);
        assert!(f.lit);
        assert!((f.area - 300.0).abs() < 1e-3, "{}", f.area);
        assert_eq!(f.shapes.len(), 1, "one shape");
        assert_eq!(f.shapes[0].len(), 1, "no holes");
        // Straight overhead: the footprint is the base, 100 mm².
        let f = footprint_on_plane(&[&cube], v(0.0, 0.0, 1.0), &Plane::XY, false, None, 0.05);
        assert!((f.area - 100.0).abs() < 1e-3, "{}", f.area);
        // A low sun from the east stretches the shadow west: 10 × (10 + 10·cot 30°).
        let f = footprint_on_plane(&[&cube], v(fmath::cos(30f64.to_radians()), 0.0, fmath::sin(30f64.to_radians())), &Plane::XY, false, None, 0.05);
        assert!((f.area - 10.0 * (10.0 + 10.0 / fmath::tan(30f64.to_radians()))).abs() < 1e-3, "{}", f.area);
        // The sun below the horizon lights nothing.
        let f = footprint_on_plane(&[&cube], v(1.0, 0.0, -0.5), &Plane::XY, true, None, 0.05);
        assert!(!f.lit && f.area == 0.0);
        // A bare plane receives on whichever side faces the sun.
        let f = footprint_on_plane(&[&cube], v(0.0, 0.0, -1.0), &Plane::XY, false, None, 0.05);
        assert!(f.lit && (f.area - 100.0).abs() < 1e-3);
    }

    /// A receiver region clips the footprint: a tower's shadow on a
    /// neighbour's 20 × 20 court, with the sun low from the tower's side,
    /// covers the strip of the court the shadow reaches and nothing else;
    /// move the court out of reach and the area is zero.
    #[test]
    fn a_receiver_clips_the_footprint_exactly() {
        // A tower 10 wide, 10 deep, 30 tall at the origin; the sun from
        // +x at 45°, so the shadow runs 30 mm to −x.
        let tower = box_mesh([0.0, 0.0, 0.0], [10.0, 10.0, 30.0]);
        let sun = v(1.0, 0.0, 1.0);
        let court = box_mesh([-40.0, -5.0, -1.0], [-20.0, 15.0, 0.0]);
        // The court's top face is its triangles with z = 0: find them.
        let top: Vec<usize> = (0..court.triangle_count())
            .filter(|&t| court.triangle(t).unwrap().iter().all(|p| p.z == 0.0))
            .collect();
        assert_eq!(top.len(), 2);
        let region = face_region(&court, (top[0], top[0] + 2), &Plane::XY);
        assert!((shapes_area(&region) - 400.0).abs() < 1e-3, "{}", shapes_area(&region));
        let f = footprint_on_plane(&[&tower], sun, &Plane::XY, true, Some(&region), 0.05);
        // The shadow spans x ∈ [−30, 0], y ∈ [0, 10]; the court x ∈ [−40, −20],
        // y ∈ [−5, 15]: the overlap is x ∈ [−30, −20] × y ∈ [0, 10] = 100.
        assert!((f.area - 100.0).abs() < 1e-3, "{}", f.area);
        // Out of reach: the sun higher, the shadow only 10 mm long.
        let f = footprint_on_plane(&[&tower], v(1.0, 0.0, 3.0), &Plane::XY, true, Some(&region), 0.05);
        assert_eq!(f.area, 0.0);
        assert!(f.lit);
        // Two casters: their shadows union, overlaps counted once.
        let twin = box_mesh([5.0, 0.0, 0.0], [15.0, 10.0, 30.0]);
        let one = footprint_on_plane(&[&tower], sun, &Plane::XY, false, None, 0.05).area;
        let both = footprint_on_plane(&[&tower, &twin], sun, &Plane::XY, false, None, 0.05).area;
        assert!((one - 400.0).abs() < 1e-3 && (both - 450.0).abs() < 1e-3, "{one} {both}");
        let world = shapes_to_world(&f.shapes, &Plane::XY);
        assert!(world.is_empty());
    }

    /// Terrain: a sloping receiver under a tower, sampled per triangle.
    #[test]
    fn a_mesh_receiver_is_sampled_per_triangle() {
        let tower = box_mesh([0.0, 0.0, 0.0], [10.0, 10.0, 30.0]);
        // A ground slab to the −x side, 1 mm thick, whose top is 400 mm².
        let ground = box_mesh([-40.0, -5.0, -1.0], [-20.0, 15.0, 0.0]);
        let s = footprint_on_mesh(&[&tower], v(1.0, 0.0, 1.0), &ground);
        // The top (400) and the +x side (20) face the sun: the shadow runs
        // x ∈ [−30, 0] × y ∈ [0, 10], so it covers 100 of the top and, at
        // the side (x = −20, z ∈ [−1, 0]), the half with y ∈ [0, 10]: 110,
        // to the sampling's few per cent.
        assert!((s.lit_area - 420.0).abs() < 1e-6, "{}", s.lit_area);
        assert!((s.area - 110.0).abs() < 8.0, "{}", s.area);
        assert!(!s.shaded.is_empty());
        let none = footprint_on_mesh(&[&tower], v(-1.0, 0.0, 1.0), &ground);
        assert_eq!(none.area, 0.0);
    }

    /// A sightline through a box: blocked, with the hit on its near face;
    /// past the box, clear. A fan over a target wall behind the box sees
    /// the part the box does not cover; the box's own far side is not a
    /// blocker of points on its near side.
    #[test]
    fn sightlines_and_fans() {
        let wall = box_mesh([50.0, -20.0, 0.0], [52.0, 20.0, 20.0]);
        let block = box_mesh([20.0, -5.0, 0.0], [25.0, 5.0, 12.0]);
        let occ = Occluders::new(&[&block, &wall]);
        let eye = v(0.0, 0.0, 5.0);
        let b = occ.cast(eye, v(51.0, 0.0, 5.0)).expect("the block is in the way");
        assert_eq!(b.body, 0);
        assert!((b.point.x - 20.0).abs() < 1e-3, "{:?}", b.point);
        assert!((b.distance - 20.0).abs() < 1e-3);
        assert!(occ.cast(eye, v(51.0, 15.0, 5.0)).map(|b| b.body) == Some(1), "past the block, the wall itself");
        assert!(occ.cast(eye, v(10.0, 0.0, 5.0)).is_none(), "short of everything");
        // The wall's near face (x = 50) points: 400 samples over its face.
        let near: Vec<usize> = (0..wall.triangle_count())
            .filter(|&t| wall.triangle(t).unwrap().iter().all(|p| p.x == 50.0))
            .collect();
        let pts = surface_samples(&wall, Some((near[0], near[0] + 2)), 400);
        assert_eq!(pts.len(), 400);
        assert!(pts.iter().all(|p| (p.x - 50.0).abs() < 1e-6));
        let f = fan(&occ, eye, &pts, Some(1));
        assert_eq!(f.total, 400);
        // The block, 5 wide at x = 20..25 and 12 tall, seen from x = 0,
        // covers y ∈ ±12.5 and z up to 22.5 on the wall at x = 50: most of
        // it. What is visible is the outer strips |y| > 12.5.
        let frac = f.fraction();
        assert!(frac > 0.3 && frac < 0.45, "{frac}");
        assert_eq!(f.blocked_by.len(), 1);
        assert_eq!(f.blocked_by[0].0, 0);
        // Without the block: everything.
        let clear = Occluders::new(&[&wall]);
        assert_eq!(fan(&clear, eye, &pts, Some(0)).visible, 400);
        // A view cone: 60° wide to the east, 0..30° up; the wall stands
        // 20 tall at 50 away, so it fills up to ~21.8° over the azimuths
        // it spans (|y| ≤ 20 at x = 50 is ±21.8°).
        let vc = view_cone(&clear, eye, (60.0, 120.0), (0.0, 30.0), (60, 30), 1e6, 0.0);
        assert_eq!(vc.total, 1800);
        let blocked = 1.0 - vc.fraction();
        assert!(blocked > 0.3 && blocked < 0.6, "{blocked}");
        assert!((elevation_to(eye, v(50.0, 0.0, 20.0)) - 16.7).abs() < 0.1);
    }
}
