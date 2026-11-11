//! Recognising the analytic surfaces and curves hiding inside truck's
//! geometry, by fitting and then **verifying**.
//!
//! truck has four surface types (`Plane`, `BSplineSurface`, `NurbsSurface`,
//! `RevolutedCurve`) and four curve types (`Line`, `BSplineCurve`,
//! `NurbsCurve`, `IntersectionCurve`). None of them is a cylinder, a cone, a
//! sphere or a torus: a hole drilled through a Cadpole part is an extruded
//! circular arc, which `builder::tsweep` turns into a *rational B-spline
//! surface*, and a revolved profile is a `RevolutedCurve`. Writing those
//! straight out is correct but wasteful and, more to the point, unhelpful: in
//! FreeCAD a `CYLINDRICAL_SURFACE` is a cylinder you can measure, fillet and
//! put a thread on, while a NURBS patch of the same shape is a freeform
//! surface that no feature recognises.
//!
//! So this module recognises. Every recogniser has the same shape:
//!
//! 1. **Sample** the truck surface on a grid over its own parameter range,
//!    keeping the point and the normal at each sample.
//! 2. **Fit** a candidate analytic surface (closed form; no iteration).
//! 3. **Verify** that *every* sample lies on the candidate to within the
//!    export tolerance, and that the candidate's normal agrees with truck's
//!    to within an angle. A recogniser that cannot verify returns `None` and
//!    the caller writes the exact B-spline instead.
//!
//! Step 3 is the whole point. A mis-recognised surface is the one way an
//! exporter can be *silently* wrong — the file parses, the shell closes, and
//! the part is the wrong shape — so nothing is claimed that has not been
//! measured. The normal check is not decoration either: it is what fixes the
//! `same_sense` flag on the face that uses the surface, and an inverted face
//! turns a solid inside out.

use truck_modeling::*;

/// How far apart two directions may be and still count as the same, in the
/// cosine of the angle. 0.5° — tight enough that a cone will never be called
/// a cylinder, loose enough to survive a tessellation-grade fit.
const NORMAL_COS: f64 = 0.999_961;

/// Samples per parameter direction when recognising a surface. 5x5 is 25
/// constraints against at most five unknowns, and the grid includes both ends
/// of the range, where a bad fit shows up first.
const GRID: usize = 5;

/// Samples along a curve when recognising a circle.
const CURVE_SAMPLES: usize = 9;

/// An analytic surface, in the terms ISO 10303-42 states them.
///
/// Every variant carries a right-handed frame (`axis` is the placement's *z*,
/// `ref_dir` its *x*) because that is what `AXIS2_PLACEMENT_3D` wants and
/// because the frame is what fixes the surface's natural normal.
#[derive(Clone, Copy, Debug, PartialEq)]
pub enum Analytic {
    /// `PLANE`. Natural normal: the placement axis.
    Plane { origin: Point3, axis: Vector3, ref_dir: Vector3 },
    /// `CYLINDRICAL_SURFACE`. Natural normal: away from the axis.
    Cylinder { origin: Point3, axis: Vector3, ref_dir: Vector3, radius: f64 },
    /// `CONICAL_SURFACE`. `radius` is the radius *at the placement origin*
    /// and the radius grows along `+axis` with `tan(semi_angle)`.
    Cone { origin: Point3, axis: Vector3, ref_dir: Vector3, radius: f64, semi_angle: f64 },
    /// `SPHERICAL_SURFACE`. Natural normal: away from the centre.
    Sphere { origin: Point3, axis: Vector3, ref_dir: Vector3, radius: f64 },
    /// `TOROIDAL_SURFACE`. Natural normal: away from the spine circle.
    Torus { origin: Point3, axis: Vector3, ref_dir: Vector3, major: f64, minor: f64 },
}

impl Analytic {
    /// The name of the STEP entity this becomes, for diagnostics.
    pub fn entity_name(&self) -> &'static str {
        match self {
            Analytic::Plane { .. } => "PLANE",
            Analytic::Cylinder { .. } => "CYLINDRICAL_SURFACE",
            Analytic::Cone { .. } => "CONICAL_SURFACE",
            Analytic::Sphere { .. } => "SPHERICAL_SURFACE",
            Analytic::Torus { .. } => "TOROIDAL_SURFACE",
        }
    }

    /// Signed distance from `p` to the surface. Zero on it; the sign is along
    /// [`Analytic::normal_at`].
    pub fn signed_distance(&self, p: Point3) -> f64 {
        match *self {
            Analytic::Plane { origin, axis, .. } => (p - origin).dot(axis),
            Analytic::Cylinder { origin, axis, radius, .. } => radial(p, origin, axis).1 - radius,
            Analytic::Cone { origin, axis, radius, semi_angle, .. } => {
                let (h, r) = radial(p, origin, axis);
                // Distance to the cone's ruling, measured perpendicular to it.
                (r - (radius + h * semi_angle.tan())) * semi_angle.cos()
            }
            Analytic::Sphere { origin, radius, .. } => p.distance(origin) - radius,
            Analytic::Torus { origin, axis, major, minor, .. } => {
                let (h, r) = radial(p, origin, axis);
                ((r - major).powi(2) + h * h).sqrt() - minor
            }
        }
    }

    /// The surface's *natural* normal at `p` — the direction of
    /// `dsigma/du x dsigma/dv` under the parameterisation ISO 10303-42 gives
    /// the entity. For all five it is "away from the axis, the centre or the
    /// spine", which is why it can be written as a function of the point
    /// rather than of the parameters.
    ///
    /// `None` at a point where the normal is undefined (on the axis of a
    /// cylinder, at the apex of a cone, at the poles of a sphere).
    pub fn normal_at(&self, p: Point3) -> Option<Vector3> {
        let out = match *self {
            Analytic::Plane { axis, .. } => axis,
            Analytic::Cylinder { origin, axis, .. } => unit(p - origin - axis * (p - origin).dot(axis))?,
            Analytic::Cone { origin, axis, semi_angle, .. } => {
                let radial_dir = unit(p - origin - axis * (p - origin).dot(axis))?;
                unit(radial_dir - axis * semi_angle.tan())?
            }
            Analytic::Sphere { origin, .. } => unit(p - origin)?,
            Analytic::Torus { origin, axis, major, .. } => {
                let along = (p - origin).dot(axis);
                let radial_dir = unit(p - origin - axis * along)?;
                unit(p - (origin + radial_dir * major))?
            }
        };
        Some(out)
    }
}

/// Height along the axis and distance from it.
fn radial(p: Point3, origin: Point3, axis: Vector3) -> (f64, f64) {
    let d = p - origin;
    let h = d.dot(axis);
    (h, (d - axis * h).magnitude())
}

fn unit(v: Vector3) -> Option<Vector3> {
    let l = v.magnitude();
    if l > 1e-12 && l.is_finite() { Some(v / l) } else { None }
}

/// Any unit vector perpendicular to `axis`, used as the placement's `x` when
/// the surface itself does not care which way round it is.
fn any_perpendicular(axis: Vector3) -> Vector3 {
    let a = if axis.x.abs() < 0.9 { Vector3::new(1.0, 0.0, 0.0) } else { Vector3::new(0.0, 1.0, 0.0) };
    unit(a - axis * a.dot(axis)).unwrap_or(Vector3::new(1.0, 0.0, 0.0))
}

/// One sample of a surface: where it is and which way it faces.
struct Sample {
    p: Point3,
    n: Vector3,
}

/// Sample a truck surface on a `GRID x GRID` net over its parameter range.
/// Returns `None` if the range is unbounded or any sample is not finite.
fn sample_surface(s: &Surface) -> Option<Vec<Sample>> {
    let (ur, vr) = s.try_range_tuple();
    let (u0, u1) = ur?;
    let (v0, v1) = vr?;
    if !(u0.is_finite() && u1.is_finite() && v0.is_finite() && v1.is_finite()) {
        return None;
    }
    let mut out = Vec::with_capacity(GRID * GRID);
    for i in 0..GRID {
        // Stay a hair inside the range: a revolved surface's v range is
        // half-open, and a B-spline's derivative at a clamped end can be
        // degenerate when the end control points coincide.
        let tu = (i as f64 + 0.5) / GRID as f64;
        let u = u0 + (u1 - u0) * tu;
        for j in 0..GRID {
            let tv = (j as f64 + 0.5) / GRID as f64;
            let v = v0 + (v1 - v0) * tv;
            let p = s.subs(u, v);
            let n = s.normal(u, v);
            if !(p.x.is_finite() && p.y.is_finite() && p.z.is_finite() && n.magnitude2().is_finite()) {
                return None;
            }
            // A degenerate normal (a pole) is dropped rather than failing the
            // whole surface: a sphere has two of them.
            if let Some(n) = unit(n) {
                out.push(Sample { p, n });
            }
        }
    }
    if out.len() < 6 { None } else { Some(out) }
}

/// Does `cand` explain every sample, in position *and* in facing?
///
/// `flip` comes back `true` when the analytic surface's natural normal is the
/// *opposite* of truck's at every sample, which the caller turns into the
/// face's `same_sense` flag. A candidate whose normals agree at some samples
/// and disagree at others is rejected outright: that is not the same surface.
fn verify(cand: &Analytic, samples: &[Sample], tol: f64) -> Option<bool> {
    let mut agree = 0usize;
    let mut oppose = 0usize;
    for s in samples {
        if cand.signed_distance(s.p).abs() > tol {
            return None;
        }
        let n = cand.normal_at(s.p)?;
        let d = n.dot(s.n);
        if d >= NORMAL_COS {
            agree += 1;
        } else if d <= -NORMAL_COS {
            oppose += 1;
        } else {
            return None;
        }
    }
    match (agree, oppose) {
        (0, 0) => None,
        (_, 0) => Some(false),
        (0, _) => Some(true),
        _ => None,
    }
}

/// A recognised surface and whether its natural normal runs against truck's.
#[derive(Clone, Copy, Debug)]
pub struct Recognised {
    pub surface: Analytic,
    /// `true` when the analytic surface's normal is the negative of the
    /// truck surface's normal at every point.
    pub flipped: bool,
}

/// Try to see one of the five analytic surfaces in `s`.
///
/// `Surface::Plane` is not handled here: it is already analytic and goes
/// straight out without a fit, keeping its own frame (see `geometry.rs`).
pub fn recognise_surface(s: &Surface, tol: f64) -> Option<Recognised> {
    let samples = sample_surface(s)?;
    let candidates = [fit_plane(&samples), fit_cylinder(&samples), fit_sphere(&samples)];
    for cand in candidates.into_iter().flatten() {
        if let Some(flipped) = verify(&cand, &samples, tol) {
            return Some(Recognised { surface: cand, flipped });
        }
    }
    // A cone is only recognised from a revolved line, where it is exact; see
    // `revolved_analytic`. Fitting one from samples needs an iteration and
    // Cadpole has no operation that makes a NURBS cone.
    None
}

/// The analytic surface a *revolved* profile is, exactly, when the profile is
/// a straight line or a circular arc. No fitting: the profile says what it is.
///
/// `axis` must be a unit vector.
pub fn revolved_analytic(profile: &Curve, origin: Point3, axis: Vector3, tol: f64) -> Option<Analytic> {
    match profile {
        Curve::Line(l) => revolved_line(l.0, l.1, origin, axis),
        other => {
            let c = recognise_circle(other, tol)?;
            revolved_circle(&c, origin, axis, tol)
        }
    }
}

fn revolved_line(p0: Point3, p1: Point3, origin: Point3, axis: Vector3) -> Option<Analytic> {
    let (h0, r0) = radial(p0, origin, axis);
    let (h1, r1) = radial(p1, origin, axis);
    let dh = h1 - h0;
    let dr = r1 - r0;
    // The line must lie in a plane through the axis, or revolving it does not
    // give a quadric at all. Both endpoints' radial directions must agree.
    let rd0 = unit(p0 - origin - axis * h0);
    let rd1 = unit(p1 - origin - axis * h1);
    if let (Some(a), Some(b)) = (rd0, rd1) {
        if a.dot(b) < NORMAL_COS {
            return None;
        }
    }
    let ref_dir = rd0.or(rd1).unwrap_or_else(|| any_perpendicular(axis));
    if dh.abs() < 1e-12 {
        // Perpendicular to the axis: an annulus, which is a plane.
        return Some(Analytic::Plane { origin: origin + axis * h0, axis, ref_dir });
    }
    if dr.abs() < 1e-12 {
        return Some(Analytic::Cylinder { origin, axis, ref_dir, radius: r0 });
    }
    // radius(h) = r0 + (h - h0) * dr/dh, so the half-angle at the apex is
    // atan(dr/dh) and the placement origin is the point where h = 0.
    let slope = dr / dh;
    let semi_angle = slope.atan();
    let radius = r0 - h0 * slope;
    if radius < 0.0 || semi_angle.abs() >= std::f64::consts::FRAC_PI_2 - 1e-9 {
        return None;
    }
    Some(Analytic::Cone { origin, axis, ref_dir, radius, semi_angle })
}

fn revolved_circle(c: &Circle3, origin: Point3, axis: Vector3, tol: f64) -> Option<Analytic> {
    // The profile circle must lie in a plane containing the axis: its own
    // normal is then perpendicular to the axis.
    if c.axis.dot(axis).abs() > 1e-6 {
        return None;
    }
    let (h, major) = radial(c.centre, origin, axis);
    if major <= tol {
        // The circle's centre is on the axis: a sphere.
        return Some(Analytic::Sphere {
            origin: c.centre,
            axis,
            ref_dir: any_perpendicular(axis),
            radius: c.radius,
        });
    }
    if major <= c.radius {
        // Self-intersecting (a "spindle" torus). STEP allows it, but no
        // reader is obliged to be sensible about it, so fall back.
        return None;
    }
    let ref_dir = unit(c.centre - origin - axis * h)?;
    Some(Analytic::Torus { origin: origin + axis * h, axis, ref_dir, major, minor: c.radius })
}

// ---------------------------------------------------------------------------
// Fits
// ---------------------------------------------------------------------------

fn fit_plane(samples: &[Sample]) -> Option<Analytic> {
    let n = unit(samples.iter().fold(Vector3::zero(), |a, s| a + s.n))?;
    let origin = samples[0].p;
    Some(Analytic::Plane { origin, axis: n, ref_dir: any_perpendicular(n) })
}

/// A cylinder's normals are all perpendicular to its axis, so the axis is the
/// cross product of the two most-separated normals; the section is then a
/// circle fitted in the plane perpendicular to it.
fn fit_cylinder(samples: &[Sample]) -> Option<Analytic> {
    let axis = axis_from_normals(samples)?;
    let (e1, e2) = frame(axis);
    let pts: Vec<(f64, f64)> = samples.iter().map(|s| (s.p.to_vec().dot(e1), s.p.to_vec().dot(e2))).collect();
    let (cx, cy, radius) = fit_circle_2d(&pts)?;
    let origin = Point3::from_vec(e1 * cx + e2 * cy);
    Some(Analytic::Cylinder { origin, axis, ref_dir: e1, radius })
}

fn fit_sphere(samples: &[Sample]) -> Option<Analytic> {
    // Each normal is radial, so the centre is p - r*n for one common r. Solve
    // the linear least-squares |p - c|^2 = r^2 in the usual algebraic form:
    // 2 p.c - |c|^2 + r^2 = |p|^2, unknowns (cx, cy, cz, k = r^2 - |c|^2).
    let mut ata = [[0.0f64; 4]; 4];
    let mut atb = [0.0f64; 4];
    for s in samples {
        let row = [2.0 * s.p.x, 2.0 * s.p.y, 2.0 * s.p.z, 1.0];
        let b = s.p.to_vec().magnitude2();
        for i in 0..4 {
            for j in 0..4 {
                ata[i][j] += row[i] * row[j];
            }
            atb[i] += row[i] * b;
        }
    }
    let x = solve(&mut ata, &mut atb)?;
    let centre = Point3::new(x[0], x[1], x[2]);
    let r2 = x[3] + centre.to_vec().magnitude2();
    if r2 <= 0.0 {
        return None;
    }
    let axis = Vector3::new(0.0, 0.0, 1.0);
    Some(Analytic::Sphere { origin: centre, axis, ref_dir: any_perpendicular(axis), radius: r2.sqrt() })
}

/// The direction perpendicular to every sampled normal, taken from the pair of
/// normals furthest apart so the cross product is well conditioned.
fn axis_from_normals(samples: &[Sample]) -> Option<Vector3> {
    let mut best = (0.0f64, Vector3::zero());
    for (i, a) in samples.iter().enumerate() {
        for b in &samples[i + 1..] {
            let c = a.n.cross(b.n);
            let l = c.magnitude();
            if l > best.0 {
                best = (l, c);
            }
        }
    }
    if best.0 < 1e-6 {
        return None;
    }
    unit(best.1)
}

/// Two unit vectors spanning the plane perpendicular to `axis`.
fn frame(axis: Vector3) -> (Vector3, Vector3) {
    let e1 = any_perpendicular(axis);
    (e1, axis.cross(e1))
}

/// Kasa's algebraic circle fit: solve `A x + B y + C = x^2 + y^2` for
/// `(A, B, C)`, whence the centre is `(A/2, B/2)`.
fn fit_circle_2d(pts: &[(f64, f64)]) -> Option<(f64, f64, f64)> {
    let mut ata = [[0.0f64; 3]; 3];
    let mut atb = [0.0f64; 3];
    for (x, y) in pts {
        let row = [*x, *y, 1.0];
        let b = x * x + y * y;
        for i in 0..3 {
            for j in 0..3 {
                ata[i][j] += row[i] * row[j];
            }
            atb[i] += row[i] * b;
        }
    }
    let x = solve(&mut ata, &mut atb)?;
    let (cx, cy) = (x[0] / 2.0, x[1] / 2.0);
    let r2 = x[2] + cx * cx + cy * cy;
    if r2 <= 0.0 || !r2.is_finite() {
        return None;
    }
    Some((cx, cy, r2.sqrt()))
}

/// Gaussian elimination with partial pivoting on an `N x N` system. `N` is 3
/// or 4 here, so this is both the simplest and the fastest thing to write.
fn solve<const N: usize>(a: &mut [[f64; N]; N], b: &mut [f64; N]) -> Option<[f64; N]> {
    for col in 0..N {
        let mut pivot = col;
        for r in col + 1..N {
            if a[r][col].abs() > a[pivot][col].abs() {
                pivot = r;
            }
        }
        if a[pivot][col].abs() < 1e-12 {
            return None;
        }
        a.swap(col, pivot);
        b.swap(col, pivot);
        for r in col + 1..N {
            let f = a[r][col] / a[col][col];
            for c in col..N {
                a[r][c] -= f * a[col][c];
            }
            b[r] -= f * b[col];
        }
    }
    let mut x = [0.0f64; N];
    for i in (0..N).rev() {
        let mut s = b[i];
        for j in i + 1..N {
            s -= a[i][j] * x[j];
        }
        x[i] = s / a[i][i];
    }
    if x.iter().all(|v| v.is_finite()) { Some(x) } else { None }
}

// ---------------------------------------------------------------------------
// Circles
// ---------------------------------------------------------------------------

/// A circular arc in space, in the frame `CIRCLE` wants: the arc runs
/// *counter-clockwise about `axis`* starting from `ref_dir`, so an edge that
/// uses it needs no `same_sense` correction.
#[derive(Clone, Copy, Debug, PartialEq)]
pub struct Circle3 {
    pub centre: Point3,
    pub axis: Vector3,
    pub ref_dir: Vector3,
    pub radius: f64,
}

/// Recognise a circular arc in any bounded truck curve.
///
/// Samples the curve, fits a plane through the samples, fits a circle in that
/// plane and checks every sample against it. `Curve::Line` is never a circle
/// and is rejected immediately.
pub fn recognise_circle(c: &Curve, tol: f64) -> Option<Circle3> {
    if matches!(c, Curve::Line(_)) {
        return None;
    }
    let (t0, t1) = c.try_range_tuple()?;
    if !(t0.is_finite() && t1.is_finite()) || t1 <= t0 {
        return None;
    }
    let pts: Vec<Point3> = (0..CURVE_SAMPLES)
        .map(|i| c.subs(t0 + (t1 - t0) * i as f64 / (CURVE_SAMPLES - 1) as f64))
        .collect();
    if pts.iter().any(|p| !(p.x.is_finite() && p.y.is_finite() && p.z.is_finite())) {
        return None;
    }
    fit_circle_3d(&pts, tol)
}

/// Fit a circle through sampled points, oriented so the points run
/// counter-clockwise about the returned axis.
pub fn fit_circle_3d(pts: &[Point3], tol: f64) -> Option<Circle3> {
    if pts.len() < 4 {
        return None;
    }
    // Newell's normal about the polyline's own centroid: the sum of the
    // triangle normals, which is the plane normal for planar points and
    // simultaneously fixes the traversal direction as counter-clockwise.
    let centroid = pts.iter().fold(Vector3::zero(), |a, p| a + p.to_vec()) / pts.len() as f64;
    let mut n = Vector3::zero();
    for w in pts.windows(2) {
        n += (w[0].to_vec() - centroid).cross(w[1].to_vec() - centroid);
    }
    let axis = unit(n)?;
    // Planar?
    let plane_origin = Point3::from_vec(centroid);
    if pts.iter().any(|p| (p - plane_origin).dot(axis).abs() > tol) {
        return None;
    }
    let (e1, e2) = frame(axis);
    let flat: Vec<(f64, f64)> = pts.iter().map(|p| ((p - plane_origin).dot(e1), (p - plane_origin).dot(e2))).collect();
    let (cx, cy, radius) = fit_circle_2d(&flat)?;
    if radius <= tol {
        return None;
    }
    for (x, y) in &flat {
        if (((x - cx).powi(2) + (y - cy).powi(2)).sqrt() - radius).abs() > tol {
            return None;
        }
    }
    let centre = plane_origin + e1 * cx + e2 * cy;
    let ref_dir = unit(pts[0] - centre)?;
    Some(Circle3 { centre, axis, ref_dir, radius })
}

#[cfg(test)]
mod tests {
    use super::*;
    use std::f64::consts::PI;
    use truck_modeling::{builder, Rad};

    const TOL: f64 = 1e-6;

    fn cylinder_surface(r: f64, h: f64) -> Surface {
        // A quarter cylinder swept from an arc, exactly as `extrude` of a
        // circular sketch segment produces it: a rational B-spline surface.
        let v0 = builder::vertex(Point3::new(r, 0.0, 0.0));
        let v1 = builder::vertex(Point3::new(0.0, r, 0.0));
        let arc = builder::circle_arc(&v0, &v1, Point3::new(r * 0.5f64.sqrt(), r * 0.5f64.sqrt(), 0.0));
        let face = builder::tsweep(&arc, Vector3::new(0.0, 0.0, h));
        face.surface()
    }

    #[test]
    fn an_extruded_arc_is_recognised_as_a_cylinder() {
        let s = cylinder_surface(5.0, 12.0);
        let r = recognise_surface(&s, TOL).expect("a swept arc is a cylinder");
        match r.surface {
            Analytic::Cylinder { origin, axis, radius, .. } => {
                assert!((radius - 5.0).abs() < 1e-9, "radius {radius}");
                assert!(axis.cross(Vector3::new(0.0, 0.0, 1.0)).magnitude() < 1e-9, "axis {axis:?}");
                assert!(origin.x.abs() < 1e-9 && origin.y.abs() < 1e-9, "origin {origin:?}");
            }
            other => panic!("expected a cylinder, got {other:?}"),
        }
    }

    #[test]
    fn the_recognised_cylinder_reproduces_every_sampled_point_and_normal() {
        let s = cylinder_surface(3.25, 4.0);
        let r = recognise_surface(&s, TOL).unwrap();
        let samples = sample_surface(&s).unwrap();
        for sm in &samples {
            assert!(r.surface.signed_distance(sm.p).abs() < TOL);
            let n = r.surface.normal_at(sm.p).unwrap();
            let d = n.dot(sm.n) * if r.flipped { -1.0 } else { 1.0 };
            assert!(d > NORMAL_COS, "normal disagrees: {d}");
        }
    }

    #[test]
    fn a_bilinear_patch_is_recognised_as_a_plane() {
        // `tsweep` of two straight edges already yields `Surface::Plane`, so
        // the interesting case is the one where a planar patch reaches us as
        // a B-spline: it must come out as `PLANE`, not as a NURBS.
        let bs = BSplineSurface::new(
            (KnotVec::bezier_knot(1), KnotVec::bezier_knot(1)),
            vec![
                vec![Point3::new(0.0, 0.0, 0.0), Point3::new(0.0, 0.0, 4.0)],
                vec![Point3::new(10.0, 0.0, 0.0), Point3::new(10.0, 0.0, 4.0)],
            ],
        );
        let r = recognise_surface(&Surface::BSplineSurface(bs), TOL).expect("a bilinear patch is a plane");
        match r.surface {
            Analytic::Plane { axis, .. } => {
                assert!(axis.cross(Vector3::new(0.0, 1.0, 0.0)).magnitude() < 1e-9, "axis {axis:?}")
            }
            other => panic!("expected a plane, got {other:?}"),
        }
    }

    #[test]
    fn revolving_a_line_parallel_to_the_axis_gives_a_cylinder() {
        let a = revolved_line(
            Point3::new(4.0, 0.0, 0.0),
            Point3::new(4.0, 0.0, 9.0),
            Point3::new(0.0, 0.0, 0.0),
            Vector3::new(0.0, 0.0, 1.0),
        )
        .unwrap();
        match a {
            Analytic::Cylinder { radius, .. } => assert!((radius - 4.0).abs() < 1e-12),
            other => panic!("expected a cylinder, got {other:?}"),
        }
    }

    #[test]
    fn revolving_a_slanted_line_gives_a_cone_with_the_right_half_angle() {
        // From r = 2 at z = 0 to r = 5 at z = 3: 45 degrees.
        let a = revolved_line(
            Point3::new(2.0, 0.0, 0.0),
            Point3::new(5.0, 0.0, 3.0),
            Point3::new(0.0, 0.0, 0.0),
            Vector3::new(0.0, 0.0, 1.0),
        )
        .unwrap();
        match a {
            Analytic::Cone { radius, semi_angle, .. } => {
                assert!((radius - 2.0).abs() < 1e-12, "radius {radius}");
                assert!((semi_angle - PI / 4.0).abs() < 1e-12, "semi angle {semi_angle}");
            }
            other => panic!("expected a cone, got {other:?}"),
        }
        // and the surface passes through both defining points
        assert!(a.signed_distance(Point3::new(2.0, 0.0, 0.0)).abs() < 1e-12);
        assert!(a.signed_distance(Point3::new(0.0, 5.0, 3.0)).abs() < 1e-12);
    }

    #[test]
    fn revolving_a_line_perpendicular_to_the_axis_gives_a_plane() {
        let a = revolved_line(
            Point3::new(2.0, 0.0, 7.0),
            Point3::new(6.0, 0.0, 7.0),
            Point3::new(0.0, 0.0, 0.0),
            Vector3::new(0.0, 0.0, 1.0),
        )
        .unwrap();
        assert!(matches!(a, Analytic::Plane { .. }), "{a:?}");
        assert!(a.signed_distance(Point3::new(0.0, 3.0, 7.0)).abs() < 1e-12);
    }

    #[test]
    fn a_circular_arc_is_recognised_with_a_counter_clockwise_frame() {
        let v0 = builder::vertex(Point3::new(6.0, 0.0, 1.0));
        let v1 = builder::vertex(Point3::new(-6.0, 0.0, 1.0));
        let arc = builder::circle_arc(&v0, &v1, Point3::new(0.0, 6.0, 1.0));
        let c = recognise_circle(&arc.curve(), TOL).expect("a circle arc is a circle");
        assert!((c.radius - 6.0).abs() < 1e-9, "radius {}", c.radius);
        assert!((c.centre - Point3::new(0.0, 0.0, 1.0)).magnitude() < 1e-9, "centre {:?}", c.centre);
        // The arc goes +x -> +y -> -x, i.e. counter-clockwise about +Z.
        assert!(c.axis.dot(Vector3::new(0.0, 0.0, 1.0)) > 0.999, "axis {:?}", c.axis);
        assert!(c.ref_dir.dot(Vector3::new(1.0, 0.0, 0.0)) > 0.999, "ref {:?}", c.ref_dir);
    }

    #[test]
    fn a_line_is_never_a_circle() {
        let v0 = builder::vertex(Point3::new(0.0, 0.0, 0.0));
        let v1 = builder::vertex(Point3::new(1.0, 2.0, 3.0));
        assert!(recognise_circle(&builder::line(&v0, &v1).curve(), TOL).is_none());
    }

    #[test]
    fn revolving_a_circle_about_a_distant_axis_gives_a_torus() {
        let v0 = builder::vertex(Point3::new(12.0, 0.0, 0.0));
        let v1 = builder::vertex(Point3::new(8.0, 0.0, 0.0));
        let arc = builder::circle_arc(&v0, &v1, Point3::new(10.0, 0.0, 2.0));
        let c = recognise_circle(&arc.curve(), TOL).unwrap();
        let a = revolved_circle(&c, Point3::new(0.0, 0.0, 0.0), Vector3::new(0.0, 0.0, 1.0), TOL).unwrap();
        match a {
            Analytic::Torus { major, minor, .. } => {
                assert!((major - 10.0).abs() < 1e-9, "major {major}");
                assert!((minor - 2.0).abs() < 1e-9, "minor {minor}");
            }
            other => panic!("expected a torus, got {other:?}"),
        }
        assert!(a.signed_distance(Point3::new(12.0, 0.0, 0.0)).abs() < 1e-9);
        assert!(a.signed_distance(Point3::new(0.0, 10.0, 2.0)).abs() < 1e-9);
    }

    #[test]
    fn revolving_a_circle_centred_on_the_axis_gives_a_sphere() {
        let v0 = builder::vertex(Point3::new(5.0, 0.0, 0.0));
        let v1 = builder::vertex(Point3::new(-5.0, 0.0, 0.0));
        let arc = builder::circle_arc(&v0, &v1, Point3::new(0.0, 0.0, 5.0));
        let c = recognise_circle(&arc.curve(), TOL).unwrap();
        let a = revolved_circle(&c, Point3::new(0.0, 0.0, 0.0), Vector3::new(0.0, 0.0, 1.0), TOL).unwrap();
        match a {
            Analytic::Sphere { radius, origin, .. } => {
                assert!((radius - 5.0).abs() < 1e-9, "radius {radius}");
                assert!(origin.to_vec().magnitude() < 1e-9, "origin {origin:?}");
            }
            other => panic!("expected a sphere, got {other:?}"),
        }
    }

    #[test]
    fn a_revolved_surface_that_is_none_of_the_five_is_refused() {
        // A revolved cubic Bezier: not a quadric, and it must not be claimed
        // as one.
        let v0 = builder::vertex(Point3::new(3.0, 0.0, 0.0));
        let v1 = builder::vertex(Point3::new(3.0, 0.0, 10.0));
        let e = builder::bezier(&v0, &v1, vec![Point3::new(6.0, 0.0, 3.0), Point3::new(1.0, 0.0, 7.0)]);
        let s = builder::rsweep(&e, Point3::new(0.0, 0.0, 0.0), Vector3::new(0.0, 0.0, 1.0), Rad(1.0));
        let face = s.face_iter().next().unwrap();
        assert!(recognise_surface(&face.surface(), TOL).is_none(), "a wavy revolve is not a quadric");
        assert!(revolved_analytic(&e.curve(), Point3::new(0.0, 0.0, 0.0), Vector3::new(0.0, 0.0, 1.0), TOL).is_none());
    }

    #[test]
    fn a_tight_tolerance_refuses_a_loose_fit() {
        // A cylinder sampled against a 10 % wrong radius must not verify.
        let s = cylinder_surface(5.0, 3.0);
        let samples = sample_surface(&s).unwrap();
        let wrong = Analytic::Cylinder {
            origin: Point3::new(0.0, 0.0, 0.0),
            axis: Vector3::new(0.0, 0.0, 1.0),
            ref_dir: Vector3::new(1.0, 0.0, 0.0),
            radius: 5.5,
        };
        assert!(verify(&wrong, &samples, 1e-3).is_none());
    }
}
