//! truck geometry to ISO 10303-42 entities.
//!
//! Two rules run through the whole module.
//!
//! **Nothing is written that has not been checked.** Every coordinate is
//! tested for finiteness before it is formatted, every placement direction for
//! being non-degenerate, and every analytic surface is a *verified* fit
//! ([`crate::analytic`]) rather than a pattern match on truck's internals. A
//! surface or curve that cannot be expressed comes back as
//! [`Error::Unsupported`](crate::Error::Unsupported) naming the type, because
//! a STEP file that is quietly the wrong shape is worse than no file.
//!
//! **Surfaces report which way they face.** [`Writer::surface`] returns
//! `agrees`: whether the STEP surface's natural normal (the direction of
//! `dsigma/du x dsigma/dv` under the parameterisation ISO 10303-42 fixes) is
//! the same as truck's `ParametricSurface3D::normal`. `topology.rs` folds that
//! into `ADVANCED_FACE.same_sense`. Get it wrong and the solid is inside out
//! with no other symptom, which is why it is computed rather than assumed:
//! for the five analytic surfaces the fit measures it, and for the other two
//! it is derived once, here, from truck's own definitions.

use truck_modeling::*;

use crate::analytic::{self, Analytic};
use crate::text::{boolean, ints, real, reals, refs, Id};
use crate::{Error, Result, Writer};

/// Number of attempts `to_bspline_leader` gets when refitting a boolean's
/// intersection polyline as a cubic B-spline.
const REFIT_TRIALS: usize = 20;

/// Samples used to check that a refitted intersection curve still follows the
/// curve it replaced.
const REFIT_SAMPLES: usize = 32;

impl Writer {
    // ---- placements ------------------------------------------------------

    pub(crate) fn point(&mut self, p: Point3) -> Result<Id> {
        finite3(p.x, p.y, p.z, "CARTESIAN_POINT")?;
        Ok(self.em.emit_shared(format!("CARTESIAN_POINT('', {})", reals([p.x, p.y, p.z]))))
    }

    /// A `DIRECTION`. The vector is normalised first: part 21 does not require
    /// it, but every reader assumes it and an un-normalised direction on a
    /// `CYLINDRICAL_SURFACE` silently scales the surface.
    pub(crate) fn direction(&mut self, v: Vector3) -> Result<Id> {
        finite3(v.x, v.y, v.z, "DIRECTION")?;
        let l = v.magnitude();
        if !(l > 1e-12) {
            return Err(Error::Invalid("a DIRECTION would be the zero vector".into()));
        }
        let v = v / l;
        Ok(self.em.emit_shared(format!("DIRECTION('', {})", reals([v.x, v.y, v.z]))))
    }

    fn vector(&mut self, v: Vector3) -> Result<Id> {
        let l = v.magnitude();
        if !(l > 1e-12) {
            return Err(Error::Invalid("a VECTOR would have zero magnitude".into()));
        }
        let d = self.direction(v)?;
        Ok(self.em.emit_shared(format!("VECTOR('', #{d}, {})", real(l))))
    }

    /// `AXIS2_PLACEMENT_3D`: a location, the local *z* (`axis`) and an
    /// approximate local *x* (`ref_direction`). `ref_direction` is
    /// orthogonalised against `axis` here so the frame the reader builds is
    /// the frame this crate reasoned about.
    pub(crate) fn axis2(&mut self, location: Point3, axis: Vector3, ref_dir: Vector3) -> Result<Id> {
        let z = normalise(axis, "AXIS2_PLACEMENT_3D axis")?;
        let x = ref_dir - z * ref_dir.dot(z);
        let x = if x.magnitude() > 1e-9 { x } else { fallback_perpendicular(z) };
        let x = normalise(x, "AXIS2_PLACEMENT_3D ref_direction")?;
        let (l, a, r) = (self.point(location)?, self.direction(z)?, self.direction(x)?);
        Ok(self.em.emit_shared(format!("AXIS2_PLACEMENT_3D('', #{l}, #{a}, #{r})")))
    }

    fn axis1(&mut self, location: Point3, axis: Vector3) -> Result<Id> {
        let z = normalise(axis, "AXIS1_PLACEMENT axis")?;
        let (l, a) = (self.point(location)?, self.direction(z)?);
        Ok(self.em.emit_shared(format!("AXIS1_PLACEMENT('', #{l}, #{a})")))
    }

    // ---- curves ----------------------------------------------------------

    /// Write one edge curve. `curve` is taken by value because an
    /// intersection curve may be refitted on the way out.
    pub(crate) fn curve(&mut self, curve: Curve) -> Result<Id> {
        match curve {
            Curve::Line(l) => {
                self.stats.lines += 1;
                let d = l.1 - l.0;
                let (p, v) = (self.point(l.0)?, self.vector(d)?);
                Ok(self.em.emit(format!("LINE('', #{p}, #{v})")))
            }
            Curve::BSplineCurve(b) => {
                if let Some(id) = self.maybe_circle(&Curve::BSplineCurve(b.clone()))? {
                    return Ok(id);
                }
                self.bspline_curve(b.control_points(), b.degree(), b.knot_vec(), None)
            }
            Curve::NurbsCurve(n) => {
                if let Some(id) = self.maybe_circle(&Curve::NurbsCurve(n.clone()))? {
                    return Ok(id);
                }
                let pts: Vec<Point3> = n.control_points().iter().map(|v| v.to_point()).collect();
                let ws: Vec<f64> = n.control_points().iter().map(|v| v.weight()).collect();
                self.bspline_curve(&pts, n.degree(), n.knot_vec(), Some(&ws))
            }
            Curve::IntersectionCurve(_) => self.intersection_curve(curve),
        }
    }

    /// A circular arc, when the curve really is one.
    ///
    /// The frame comes back from the fit already oriented so the curve runs
    /// counter-clockwise about the placement axis, which is the direction
    /// `CIRCLE` parameterises in — so the `EDGE_CURVE` that uses it needs no
    /// `same_sense` correction.
    fn maybe_circle(&mut self, c: &Curve) -> Result<Option<Id>> {
        if !self.options.analytic_curves {
            return Ok(None);
        }
        let Some(circle) = analytic::recognise_circle(c, self.options.tolerance) else {
            return Ok(None);
        };
        let a = self.axis2(circle.centre, circle.axis, circle.ref_dir)?;
        self.stats.circles += 1;
        Ok(Some(self.em.emit(format!("CIRCLE('', #{a}, {})", real(circle.radius)))))
    }

    /// The curve a boolean leaves on a new edge.
    ///
    /// truck represents it as an `IntersectionCurve` whose *leader* is the
    /// polyline the intersector walked. Two things follow. (1) The leader is
    /// the geometry; the two surfaces it was cut from are already the faces'
    /// own geometry, so writing a `SURFACE_CURVE` / `INTERSECTION_CURVE`
    /// around it would add a second, redundant statement of the same fact and
    /// a second chance to disagree with it. We write the leader alone.
    /// (2) A polyline edge between smooth faces is honest but ugly, so
    /// `refit_intersections` first asks truck to fit a cubic B-spline through
    /// it — and then **checks the fit**, sampling both curves and keeping the
    /// polyline unless the spline tracks it within the export tolerance and
    /// meets both end points. The check is what stops the refit from quietly
    /// moving an edge.
    fn intersection_curve(&mut self, curve: Curve) -> Result<Id> {
        let leader = match self.refit(curve)? {
            Leader::BSpline(b) => {
                self.stats.refitted_intersections += 1;
                return self.bspline_curve(b.control_points(), b.degree(), b.knot_vec(), None);
            }
            other => other,
        };
        let Leader::Polyline(poly) = leader else { unreachable!("handled above") };
        if poly.len() < 2 {
            return Err(Error::Invalid("a boolean produced an edge with fewer than two points".into()));
        }
        if let Some(id) = self.maybe_line(&poly)? {
            return Ok(id);
        }
        self.stats.polyline_intersections += 1;
        let ids = poly.iter().map(|p| self.point(*p)).collect::<Result<Vec<_>>>()?;
        Ok(self.em.emit(format!("POLYLINE('', {})", refs(ids))))
    }

    /// A `LINE`, when the leader is straight.
    ///
    /// Where two planes meet, the intersection is a straight line and truck's
    /// walker returns it as three or four collinear points. A cubic cannot be
    /// fitted through three points, so the refit above declines and the edge
    /// went out as `POLYLINE('', (#36, #82, #38))` with a redundant midpoint:
    /// valid AP214, and read back as a degree-1 B-spline where a `LINE` was
    /// available. That is a straight edge a CAD user cannot select as one, so
    /// cannot fillet or dimension as one, on the pocket opening of
    /// `examples/print_ready.star` among others.
    ///
    /// Straight means every interior point lies on the segment between the
    /// ends, within the export tolerance — measured, like every other analytic
    /// fit here, rather than assumed from the point count.
    fn maybe_line(&mut self, poly: &[Point3]) -> Result<Option<Id>> {
        if !self.options.analytic_curves {
            return Ok(None);
        }
        let tol = self.options.tolerance;
        let (a, b) = (poly[0], poly[poly.len() - 1]);
        let d = b - a;
        let len = d.magnitude();
        if len <= tol {
            // Both ends in the same place: a closed or vanishing edge is not a
            // line, whatever the points in between do.
            return Ok(None);
        }
        let u = d / len;
        for p in &poly[1..poly.len() - 1] {
            let w = *p - a;
            let along = w.dot(u);
            // Off the line, or doubling back past an end: not a straight edge.
            if along < -tol || along > len + tol || (w - u * along).magnitude() > tol {
                return Ok(None);
            }
        }
        let (p, v) = (self.point(a)?, self.vector(d)?);
        self.stats.lines += 1;
        Ok(Some(self.em.emit(format!("LINE('', #{p}, #{v})"))))
    }

    fn refit(&mut self, curve: Curve) -> Result<Leader> {
        let Curve::IntersectionCurve(ic) = &curve else {
            return Err(Error::Invalid("refit called on a curve that is not an intersection".into()));
        };
        if let Leader::BSpline(b) = ic.leader() {
            return Ok(Leader::BSpline(b.clone()));
        }
        let original = curve.clone();
        if !self.options.refit_intersections {
            let Curve::IntersectionCurve(ic) = original else { unreachable!() };
            return Ok(ic.leader().clone());
        }
        let mut fitted = curve.clone();
        let tol = self.options.tolerance;
        if fitted.to_bspline_leader(tol, tol, REFIT_TRIALS) && follows(&original, &fitted, tol) {
            let Curve::IntersectionCurve(ic) = fitted else { unreachable!() };
            return Ok(ic.leader().clone());
        }
        let Curve::IntersectionCurve(ic) = original else { unreachable!() };
        Ok(ic.leader().clone())
    }

    /// `B_SPLINE_CURVE_WITH_KNOTS`, in the plain form or — with weights — in
    /// the complex `RATIONAL_B_SPLINE_CURVE` form. The sub-supertype names of
    /// a complex instance are in the alphabetical order part 21 clause 11
    /// requires (`BOUNDED_CURVE` before `B_SPLINE_CURVE`: `O` sorts before
    /// `_`).
    fn bspline_curve(&mut self, pts: &[Point3], degree: usize, knots: &KnotVec, weights: Option<&[f64]>) -> Result<Id> {
        if pts.len() < 2 || degree < 1 {
            return Err(Error::Invalid(format!("a B-spline curve with {} control points of degree {degree}", pts.len())));
        }
        let (knot_values, mults) = knots.to_single_multi();
        for k in &knot_values {
            finite(*k, "B_SPLINE_CURVE_WITH_KNOTS knot")?;
        }
        let ids = pts.iter().map(|p| self.point(*p)).collect::<Result<Vec<_>>>()?;
        let cps = refs(ids);
        let (km, kv) = (ints(mults), reals(knot_values));
        match weights {
            None => {
                self.stats.bspline_curves += 1;
                Ok(self.em.emit(format!(
                    "B_SPLINE_CURVE_WITH_KNOTS('', {degree}, {cps}, .UNSPECIFIED., .U., .U., {km}, {kv}, .UNSPECIFIED.)"
                )))
            }
            Some(w) => {
                for x in w {
                    finite(*x, "RATIONAL_B_SPLINE_CURVE weight")?;
                }
                self.stats.rational_curves += 1;
                Ok(self.em.emit(format!(
                    "( BOUNDED_CURVE() B_SPLINE_CURVE({degree}, {cps}, .UNSPECIFIED., .U., .U.) \
                     B_SPLINE_CURVE_WITH_KNOTS({km}, {kv}, .UNSPECIFIED.) CURVE() \
                     GEOMETRIC_REPRESENTATION_ITEM() RATIONAL_B_SPLINE_CURVE({}) REPRESENTATION_ITEM('') )",
                    reals(w.iter().copied())
                )))
            }
        }
    }

    // ---- surfaces --------------------------------------------------------

    /// Write a face's surface. Returns its id and whether the STEP surface's
    /// natural normal agrees with truck's.
    pub(crate) fn surface(&mut self, s: &Surface) -> Result<(Id, bool)> {
        // A truck `Plane` is already an ISO 10303-42 `plane`, with its own
        // frame; refitting it could only lose precision.
        if let Surface::Plane(p) = s {
            let id = self.plane(p.origin(), p.normal(), p.u_axis())?;
            return Ok((id, true));
        }
        if self.options.analytic_surfaces {
            if let Surface::RevolutedCurve(proc) = s {
                // A revolved profile says exactly what it is; no fit needed.
                let rev = proc.entity();
                let (curve, origin, axis) = self.revolved_frame(proc)?;
                let _ = rev;
                if let Some(a) = analytic::revolved_analytic(&curve, origin, axis, self.options.tolerance) {
                    // truck's revolved normal is `C'(u) x (axis x r)`, the
                    // *negative* of the `du x dv` of an analytic surface
                    // whose normal points outward; `Processor::orientation`
                    // flips it again when the surface was inverted.
                    let outward = a.normal_at(curve_mid_point(&curve, origin, axis));
                    if outward.is_some() {
                        let id = self.analytic_surface(&a)?;
                        let agrees = self.revolved_agrees(s, &a);
                        if let Some(agrees) = agrees {
                            return Ok((id, agrees));
                        }
                        // The fit was exact but the facing could not be
                        // measured; fall through to the general path rather
                        // than guess.
                    }
                }
            } else if let Some(r) = analytic::recognise_surface(s, cadpole_core::surface_class::EXACT_TOL) {
                // Exact, not the document tolerance: the face's edges are
                // written exactly, so a surface that is only *near* the patch
                // (a narrow band of a shallow cone is within 0.05 mm of a
                // plane) leaves them off their own face.
                let id = self.analytic_surface(&r.surface)?;
                return Ok((id, !r.flipped));
            }
        }
        match s {
            Surface::Plane(_) => unreachable!("handled above"),
            Surface::BSplineSurface(b) => {
                let id = self.bspline_surface(b.control_points(), b.udegree(), b.vdegree(), b.uknot_vec(), b.vknot_vec(), None)?;
                // A B-spline's normal is `du x dv` of the same control net in
                // truck and in every STEP reader, so the two agree.
                Ok((id, true))
            }
            Surface::NurbsSurface(n) => {
                let pts: Vec<Vec<Point3>> =
                    n.control_points().iter().map(|row| row.iter().map(|v| v.to_point()).collect()).collect();
                let ws: Vec<Vec<f64>> =
                    n.control_points().iter().map(|row| row.iter().map(|v| v.weight()).collect()).collect();
                let id = self.bspline_surface(&pts, n.udegree(), n.vdegree(), n.uknot_vec(), n.vknot_vec(), Some(&ws))?;
                Ok((id, true))
            }
            Surface::RevolutedCurve(proc) => {
                let (curve, origin, axis) = self.revolved_frame(proc)?;
                let c = self.curve(curve)?;
                let a = self.axis1(origin, axis)?;
                self.stats.surfaces_of_revolution += 1;
                let id = self.em.emit(format!("SURFACE_OF_REVOLUTION('', #{c}, #{a})"));
                // ISO 10303-42 parameterises a surface of revolution as
                // (angle, curve); truck's `RevolutedCurve` is (curve, angle).
                // Swapping the two parameters negates `du x dv`, so the two
                // normals are opposite — and `Processor::orientation() ==
                // false` means truck already inverted its own, flipping the
                // comparison back.
                Ok((id, !proc.orientation()))
            }
        }
    }

    /// The profile curve, axis origin and axis direction of a revolved
    /// surface, with the decorator's transform already applied.
    ///
    /// Refuses a transform that is not a similarity: revolving a profile about
    /// an axis and then shearing it is not a surface of revolution at all, and
    /// no STEP entity says it.
    fn revolved_frame(&self, proc: &Processor<RevolutedCurve<Curve>, Matrix4>) -> Result<(Curve, Point3, Vector3)> {
        let m = *proc.transform();
        let rev = proc.entity();
        if !is_similarity(&m) {
            return Err(Error::Unsupported {
                what: "RevolutedCurve".into(),
                detail: "its transform is not a rigid motion with uniform scale, so it is not a \
                         surface of revolution; rebuild the feature without a non-uniform scale"
                    .into(),
            });
        }
        let curve = rev.entity_curve().transformed(m);
        let origin = m.transform_point(rev.origin());
        let axis = normalise(m.transform_vector(rev.axis()), "revolved surface axis")?;
        Ok((curve, origin, axis))
    }

    /// Whether the analytic stand-in for a revolved surface faces the same way
    /// truck's does. Measured, at a point on the surface, not assumed.
    fn revolved_agrees(&self, s: &Surface, a: &Analytic) -> Option<bool> {
        let (ur, vr) = s.try_range_tuple();
        let (u0, u1) = ur?;
        let (v0, v1) = vr?;
        let (u, v) = (u0 + (u1 - u0) * 0.5, v0 + (v1 - v0) * 0.5);
        let p = s.subs(u, v);
        let n = s.normal(u, v);
        if !n.magnitude2().is_finite() || n.magnitude2() < 0.5 {
            return None;
        }
        let out = a.normal_at(p)?;
        let d = out.dot(n);
        if d.abs() < 0.9 { None } else { Some(d > 0.0) }
    }

    fn plane(&mut self, origin: Point3, normal: Vector3, ref_dir: Vector3) -> Result<Id> {
        let a = self.axis2(origin, normal, ref_dir)?;
        self.stats.planes += 1;
        Ok(self.em.emit_shared(format!("PLANE('', #{a})")))
    }

    fn analytic_surface(&mut self, a: &Analytic) -> Result<Id> {
        let id = match *a {
            Analytic::Plane { origin, axis, ref_dir } => return self.plane(origin, axis, ref_dir),
            Analytic::Cylinder { origin, axis, ref_dir, radius } => {
                let p = self.axis2(origin, axis, ref_dir)?;
                self.stats.cylinders += 1;
                self.em.emit_shared(format!("CYLINDRICAL_SURFACE('', #{p}, {})", real(radius)))
            }
            Analytic::Cone { origin, axis, ref_dir, radius, semi_angle } => {
                let p = self.axis2(origin, axis, ref_dir)?;
                self.stats.cones += 1;
                self.em.emit_shared(format!(
                    "CONICAL_SURFACE('', #{p}, {}, {})",
                    real(radius),
                    real(semi_angle)
                ))
            }
            Analytic::Sphere { origin, axis, ref_dir, radius } => {
                let p = self.axis2(origin, axis, ref_dir)?;
                self.stats.spheres += 1;
                self.em.emit_shared(format!("SPHERICAL_SURFACE('', #{p}, {})", real(radius)))
            }
            Analytic::Torus { origin, axis, ref_dir, major, minor } => {
                let p = self.axis2(origin, axis, ref_dir)?;
                self.stats.tori += 1;
                self.em.emit_shared(format!("TOROIDAL_SURFACE('', #{p}, {}, {})", real(major), real(minor)))
            }
        };
        Ok(id)
    }

    fn bspline_surface(
        &mut self,
        pts: &[Vec<Point3>],
        u_degree: usize,
        v_degree: usize,
        uknots: &KnotVec,
        vknots: &KnotVec,
        weights: Option<&[Vec<f64>]>,
    ) -> Result<Id> {
        if pts.len() < 2 || pts[0].len() < 2 {
            return Err(Error::Invalid("a B-spline surface with a degenerate control net".into()));
        }
        let (uk, um) = uknots.to_single_multi();
        let (vk, vm) = vknots.to_single_multi();
        for k in uk.iter().chain(vk.iter()) {
            finite(*k, "B_SPLINE_SURFACE_WITH_KNOTS knot")?;
        }
        let mut rows = Vec::with_capacity(pts.len());
        for row in pts {
            let ids = row.iter().map(|p| self.point(*p)).collect::<Result<Vec<_>>>()?;
            rows.push(refs(ids));
        }
        let cps = format!("({})", rows.join(", "));
        let (umk, vmk) = (ints(um), ints(vm));
        let (ukv, vkv) = (reals(uk), reals(vk));
        match weights {
            None => {
                self.stats.bspline_surfaces += 1;
                Ok(self.em.emit(format!(
                    "B_SPLINE_SURFACE_WITH_KNOTS('', {u_degree}, {v_degree}, {cps}, .UNSPECIFIED., .U., .U., .U., \
                     {umk}, {vmk}, {ukv}, {vkv}, .UNSPECIFIED.)"
                )))
            }
            Some(w) => {
                let mut wrows = Vec::with_capacity(w.len());
                for row in w {
                    for x in row {
                        finite(*x, "RATIONAL_B_SPLINE_SURFACE weight")?;
                    }
                    wrows.push(reals(row.iter().copied()));
                }
                self.stats.rational_surfaces += 1;
                Ok(self.em.emit(format!(
                    "( BOUNDED_SURFACE() B_SPLINE_SURFACE({u_degree}, {v_degree}, {cps}, .UNSPECIFIED., .U., .U., .U.) \
                     B_SPLINE_SURFACE_WITH_KNOTS({umk}, {vmk}, {ukv}, {vkv}, .UNSPECIFIED.) \
                     GEOMETRIC_REPRESENTATION_ITEM() RATIONAL_B_SPLINE_SURFACE(({})) REPRESENTATION_ITEM('') SURFACE() )",
                    wrows.join(", ")
                )))
            }
        }
    }

    pub(crate) fn bool_lit(&self, b: bool) -> &'static str {
        boolean(b)
    }
}

// ---------------------------------------------------------------------------
// Numeric helpers
// ---------------------------------------------------------------------------

fn finite(x: f64, what: &str) -> Result<()> {
    if x.is_finite() {
        Ok(())
    } else {
        Err(Error::Invalid(format!("{what} would be written as {x}, which is not a number STEP can carry")))
    }
}

fn finite3(x: f64, y: f64, z: f64, what: &str) -> Result<()> {
    finite(x, what)?;
    finite(y, what)?;
    finite(z, what)
}

fn normalise(v: Vector3, what: &str) -> Result<Vector3> {
    finite3(v.x, v.y, v.z, what)?;
    let l = v.magnitude();
    if l > 1e-12 { Ok(v / l) } else { Err(Error::Invalid(format!("{what} is the zero vector"))) }
}

fn fallback_perpendicular(z: Vector3) -> Vector3 {
    let a = if z.x.abs() < 0.9 { Vector3::new(1.0, 0.0, 0.0) } else { Vector3::new(0.0, 1.0, 0.0) };
    a - z * a.dot(z)
}

/// Is `m` a rotation, a translation and a uniform scale, and nothing else?
fn is_similarity(m: &Matrix4) -> bool {
    let cols = [m.x.truncate(), m.y.truncate(), m.z.truncate()];
    let lens: Vec<f64> = cols.iter().map(|c| c.magnitude()).collect();
    if lens.iter().any(|l| !l.is_finite() || *l < 1e-12) {
        return false;
    }
    if (lens[0] - lens[1]).abs() > 1e-9 * lens[0] || (lens[1] - lens[2]).abs() > 1e-9 * lens[1] {
        return false;
    }
    let orth = |a: Vector3, b: Vector3| (a.dot(b) / (a.magnitude() * b.magnitude())).abs() < 1e-9;
    if !(orth(cols[0], cols[1]) && orth(cols[1], cols[2]) && orth(cols[0], cols[2])) {
        return false;
    }
    // The bottom row must be (0, 0, 0, 1): anything else is a projection.
    (m.x.w.abs() < 1e-12) && (m.y.w.abs() < 1e-12) && (m.z.w.abs() < 1e-12) && ((m.w.w - 1.0).abs() < 1e-9)
}

/// A point on the revolved surface, for measuring which way it faces.
fn curve_mid_point(c: &Curve, origin: Point3, axis: Vector3) -> Point3 {
    let _ = (origin, axis);
    match c.try_range_tuple() {
        Some((a, b)) => c.subs(a + (b - a) * 0.5),
        None => c.subs(0.5),
    }
}

/// Does `fitted` track `original` to within `tol` everywhere, ends included?
fn follows(original: &Curve, fitted: &Curve, tol: f64) -> bool {
    let (Some((a0, a1)), Some((b0, b1))) = (original.try_range_tuple(), fitted.try_range_tuple()) else {
        return false;
    };
    (0..=REFIT_SAMPLES).all(|i| {
        let t = i as f64 / REFIT_SAMPLES as f64;
        let p = original.subs(a0 + (a1 - a0) * t);
        let q = fitted.subs(b0 + (b1 - b0) * t);
        p.distance(q) <= tol
    })
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::{Options, Writer};

    fn writer() -> Writer {
        Writer::new(Options::default())
    }

    #[test]
    fn a_direction_is_normalised_and_shared() {
        let mut w = writer();
        let a = w.direction(Vector3::new(0.0, 0.0, 5.0)).unwrap();
        let b = w.direction(Vector3::new(0.0, 0.0, 1.0)).unwrap();
        assert_eq!(a, b, "two spellings of +Z must be one DIRECTION");
        assert!(w.em.render().contains("DIRECTION('', (0.0, 0.0, 1.0))"));
    }

    #[test]
    fn a_zero_direction_is_refused_rather_than_written() {
        let mut w = writer();
        let e = w.direction(Vector3::new(0.0, 0.0, 0.0)).unwrap_err();
        assert!(e.to_string().contains("zero vector"), "{e}");
    }

    #[test]
    fn a_non_finite_coordinate_is_refused() {
        let mut w = writer();
        let e = w.point(Point3::new(1.0, f64::NAN, 0.0)).unwrap_err();
        assert!(e.to_string().contains("CARTESIAN_POINT"), "{e}");
    }

    #[test]
    fn a_line_becomes_a_line_entity_with_a_unit_direction_and_a_magnitude() {
        let mut w = writer();
        w.curve(Curve::Line(Line(Point3::new(0.0, 0.0, 0.0), Point3::new(0.0, 0.0, 3.0)))).unwrap();
        let text = w.em.render();
        assert!(text.contains("VECTOR('', #2, 3.0)"), "{text}");
        assert!(text.contains("LINE('', #1, #3)"), "{text}");
    }

    #[test]
    fn a_planar_face_writes_a_plane_that_agrees_with_trucks_normal() {
        let mut w = writer();
        let plane = Plane::new(
            Point3::new(0.0, 0.0, 0.0),
            Point3::new(1.0, 0.0, 0.0),
            Point3::new(0.0, 1.0, 0.0),
        );
        let (_, agrees) = w.surface(&Surface::Plane(plane)).unwrap();
        assert!(agrees);
        assert!(w.em.render().contains("PLANE('', #"));
    }

    #[test]
    fn a_shear_on_a_revolved_surface_is_refused_by_name() {
        let mut w = writer();
        let line = Curve::Line(Line(Point3::new(2.0, 0.0, 0.0), Point3::new(2.0, 0.0, 5.0)));
        let rev = RevolutedCurve::by_revolution(line, Point3::new(0.0, 0.0, 0.0), Vector3::new(0.0, 0.0, 1.0));
        let mut shear = Matrix4::from_scale(1.0);
        shear.y.x = 0.5;
        let mut proc = Processor::new(rev);
        proc.transform_by(shear);
        let e = w.surface(&Surface::RevolutedCurve(proc)).unwrap_err();
        assert!(e.to_string().contains("RevolutedCurve"), "{e}");
    }

    #[test]
    fn a_revolved_line_writes_a_cylinder_not_a_surface_of_revolution() {
        let mut w = writer();
        let line = Curve::Line(Line(Point3::new(2.0, 0.0, 0.0), Point3::new(2.0, 0.0, 5.0)));
        let rev = RevolutedCurve::by_revolution(line, Point3::new(0.0, 0.0, 0.0), Vector3::new(0.0, 0.0, 1.0));
        let surface = Surface::RevolutedCurve(Processor::new(rev));
        let (_, agrees) = w.surface(&surface).unwrap();
        let text = w.em.render();
        assert!(text.contains("CYLINDRICAL_SURFACE('', #4, 2.0)"), "{text}");
        // truck's revolved normal at the midpoint points *into* the axis, so
        // the outward cylinder normal disagrees with it.
        let n = surface.normal(0.5, 1.0);
        let p = surface.subs(0.5, 1.0);
        let outward = (p - Point3::new(0.0, 0.0, p.z)).normalize();
        assert_eq!(agrees, n.dot(outward) > 0.0, "the measured facing must match the flag");
    }

    #[test]
    fn analytic_recognition_can_be_turned_off() {
        let mut w = Writer::new(Options { analytic_surfaces: false, ..Options::default() });
        let line = Curve::Line(Line(Point3::new(2.0, 0.0, 0.0), Point3::new(2.0, 0.0, 5.0)));
        let rev = RevolutedCurve::by_revolution(line, Point3::new(0.0, 0.0, 0.0), Vector3::new(0.0, 0.0, 1.0));
        w.surface(&Surface::RevolutedCurve(Processor::new(rev))).unwrap();
        let text = w.em.render();
        assert!(text.contains("SURFACE_OF_REVOLUTION"), "{text}");
        assert!(!text.contains("CYLINDRICAL_SURFACE"), "{text}");
    }

    #[test]
    fn a_rational_curve_uses_the_complex_entity_form_in_alphabetical_order() {
        let mut w = Writer::new(Options { analytic_curves: false, ..Options::default() });
        let v0 = builder::vertex(Point3::new(4.0, 0.0, 0.0));
        let v1 = builder::vertex(Point3::new(-4.0, 0.0, 0.0));
        let arc = builder::circle_arc(&v0, &v1, Point3::new(0.0, 4.0, 0.0));
        w.curve(arc.curve()).unwrap();
        let text = w.em.render();
        let complex = text.lines().find(|l| l.contains("RATIONAL_B_SPLINE_CURVE")).expect("a rational curve");
        let order: Vec<&str> = ["BOUNDED_CURVE", "B_SPLINE_CURVE(", "B_SPLINE_CURVE_WITH_KNOTS", "CURVE()", "GEOMETRIC_REPRESENTATION_ITEM", "RATIONAL_B_SPLINE_CURVE", "REPRESENTATION_ITEM"]
            .into_iter()
            .collect();
        let mut at = 0usize;
        for name in order {
            let found = complex[at..].find(name).unwrap_or_else(|| panic!("{name} missing or out of order in {complex}"));
            at += found + name.len();
        }
    }

    #[test]
    fn an_arc_is_written_as_a_circle_when_recognition_is_on() {
        let mut w = writer();
        let v0 = builder::vertex(Point3::new(4.0, 0.0, 0.0));
        let v1 = builder::vertex(Point3::new(-4.0, 0.0, 0.0));
        let arc = builder::circle_arc(&v0, &v1, Point3::new(0.0, 4.0, 0.0));
        w.curve(arc.curve()).unwrap();
        let text = w.em.render();
        assert!(text.contains("CIRCLE('', #4, 4.0)"), "{text}");
    }
}
