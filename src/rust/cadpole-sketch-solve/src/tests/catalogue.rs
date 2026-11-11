//! One test per constraint the Starlark API promises, so the catalogue is
//! covered even where a shape test does not reach it.

use super::{dist, near, near_point, point_line_distance};
use crate::constraint::Error;
use crate::{Constraint, PointRef, System};
use core::f64::consts::PI;

#[test]
fn parallel_lines_end_up_pointing_the_same_way() {
    let mut s = System::new();
    let (a0, a1) = (s.point(0.0, 0.0), s.point(10.0, 0.0));
    let (b0, b1) = (s.point(0.0, 5.0), s.point(9.0, 8.0));
    let a = s.line(a0, a1);
    let b = s.line(b0, b1);
    s.constrain(Constraint::fix(a0)).unwrap();
    s.constrain(Constraint::fix(a1)).unwrap();
    s.constrain(Constraint::fix(b0)).unwrap();
    s.constrain(Constraint::parallel(a, b)).unwrap();

    let r = s.solve();
    assert!(r.is_converged(), "{r}");
    near(s.point_pos(b1).1, 5.0, 1e-9, "b is level with b0, like a");
}

#[test]
fn perpendicular_lines_end_up_at_a_right_angle() {
    let mut s = System::new();
    let (a0, a1) = (s.point(0.0, 0.0), s.point(10.0, 0.0));
    let (b0, b1) = (s.point(0.0, 0.0), s.point(3.0, 9.0));
    let a = s.line(a0, a1);
    let b = s.line(b0, b1);
    s.constrain(Constraint::fix(a0)).unwrap();
    s.constrain(Constraint::fix(a1)).unwrap();
    s.constrain(Constraint::fix(b0)).unwrap();
    s.constrain(Constraint::distance(b0, b1, 10.0)).unwrap();
    s.constrain(Constraint::perpendicular(a, b)).unwrap();

    let r = s.solve();
    assert!(r.is_converged(), "{r}");
    assert_eq!(r.dof, 0, "{r}");
    near_point(s.point_pos(b1), (0.0, 10.0), 1e-9, "straight up");
}

#[test]
fn a_midpoint_lands_halfway_along() {
    let mut s = System::new();
    let (a, b) = (s.point(0.0, 0.0), s.point(40.0, 20.0));
    let m = s.point(5.0, 5.0);
    let line = s.line(a, b);
    s.constrain(Constraint::fix(a)).unwrap();
    s.constrain(Constraint::fix(b)).unwrap();
    s.constrain(Constraint::midpoint(m, line)).unwrap();

    let r = s.solve();
    assert!(r.is_converged(), "{r}");
    assert_eq!(r.dof, 0, "{r}");
    near_point(s.point_pos(m), (20.0, 10.0), 1e-10, "midpoint");
}

#[test]
fn a_point_can_be_dimensioned_off_a_line() {
    let mut s = System::new();
    let (a, b) = (s.point(0.0, 0.0), s.point(50.0, 0.0));
    let line = s.line(a, b);
    let p = s.point(20.0, 4.0);
    s.constrain(Constraint::fix(a)).unwrap();
    s.constrain(Constraint::fix(b)).unwrap();
    s.constrain(Constraint::point_line_distance(p, line, 12.0)).unwrap();

    let r = s.solve();
    assert!(r.is_converged(), "{r}");
    assert_eq!(r.dof, 1, "the point still slides along: {r}");
    near(point_line_distance(s.point_pos(p), (0.0, 0.0), (50.0, 0.0)), 12.0, 1e-9, "offset");
    assert!(s.point_pos(p).1 > 0.0, "it stayed on the side it started");
}

#[test]
fn fix_holds_every_parameter_of_an_arc() {
    let mut s = System::new();
    let c = s.point(3.0, 4.0);
    let arc = s.arc(c, 6.0, 0.0, PI);
    s.constrain(Constraint::fix(arc)).unwrap();
    // Something else pulling on the same arc must lose.
    let p = s.point(20.0, 20.0);
    s.constrain(Constraint::coincident(p, PointRef::end(arc))).unwrap();

    let r = s.solve();
    assert!(r.is_converged(), "{r}");
    near_point(s.point_pos(c), (3.0, 4.0), 1e-10, "centre held");
    near(s.radius(arc).unwrap(), 6.0, 1e-10, "radius held");
    // The arc's end at 180° is (3 − 6, 4); the loose point had to come to it.
    near_point(s.point_pos(p), (-3.0, 4.0), 1e-9, "point pulled to the arc end");
}

#[test]
fn an_arc_endpoint_is_a_point_like_any_other() {
    let mut s = System::new();
    let c = s.point(0.0, 0.0);
    let arc = s.arc(c, 10.0, 0.0, 0.25 * PI);
    let p = s.point(9.0, 9.0);
    s.constrain(Constraint::fix(c)).unwrap();
    s.constrain(Constraint::radius(arc, 10.0)).unwrap();
    s.constrain(Constraint::coincident(p, PointRef::end(arc))).unwrap();
    // Pin the loose point on the 45° diagonal; because it is also the arc's
    // end, the arc's end *angle* is what has to move.
    let diag0 = s.point(0.0, 0.0);
    let diag1 = s.point(10.0, 10.0);
    let diag = s.line(diag0, diag1);
    s.constrain(Constraint::fix(diag0)).unwrap();
    s.constrain(Constraint::fix(diag1)).unwrap();
    s.constrain(Constraint::on_curve(p, diag)).unwrap();

    let r = s.solve();
    assert!(r.is_converged(), "{r}");
    // Only the arc's *start* angle is still free.
    assert_eq!(r.dof, 1, "{r}");
    let e = s.position(PointRef::end(arc)).unwrap();
    near(e.0, 10.0 / 2f64.sqrt(), 1e-9, "arc end x");
    near(e.1, 10.0 / 2f64.sqrt(), 1e-9, "arc end y");
    near(s.arc_angles(arc).unwrap().1, 0.25 * PI, 1e-9, "end angle");
}

#[test]
fn point_at_pins_a_point_without_snapshotting() {
    let mut s = System::new();
    let p = s.point(1.0, 1.0);
    s.constrain(Constraint::point_at(p, 7.0, -3.0)).unwrap();
    assert!(s.solve().is_converged());
    near_point(s.point_pos(p), (7.0, -3.0), 1e-10, "pinned");
}

#[test]
fn equal_length_makes_two_sides_match() {
    let mut s = System::new();
    let (a0, a1) = (s.point(0.0, 0.0), s.point(20.0, 0.0));
    let (b0, b1) = (s.point(0.0, 10.0), s.point(30.0, 10.0));
    let a = s.line(a0, a1);
    let b = s.line(b0, b1);
    s.constrain(Constraint::fix(a0)).unwrap();
    s.constrain(Constraint::fix(a1)).unwrap();
    s.constrain(Constraint::fix(b0)).unwrap();
    s.constrain(Constraint::horizontal(b)).unwrap();
    s.constrain(Constraint::equal_length(a, b)).unwrap();

    assert!(s.solve().is_converged());
    near(dist(&s, b0, b1), 20.0, 1e-9, "b matched a");
}

#[test]
fn an_unconstrained_point_has_two_degrees_of_freedom() {
    let mut s = System::new();
    let p = s.point(1.0, 2.0);
    assert_eq!(s.dof(), 2);
    let d = s.diagnose();
    assert!(d.is_under_constrained());
    assert_eq!(d.free_entities, vec![p.0]);
    s.constrain(Constraint::fix(p)).unwrap();
    assert_eq!(s.dof(), 0);
    assert!(s.diagnose().free_entities.is_empty());
}

#[test]
fn bad_constraints_are_rejected_rather_than_solved() {
    let mut s = System::new();
    let p = s.point(0.0, 0.0);
    let q = s.point(1.0, 0.0);
    let line = s.line(p, q);
    let other = s.line(q, p);
    assert_eq!(s.constrain(Constraint::radius(p, 3.0)), Err(Error::NotRadial(p.0)));
    assert_eq!(
        s.constrain(Constraint::tangent(line, other)),
        Err(Error::TangentNeedsACurve)
    );
    assert_eq!(
        s.constrain(Constraint::coincident(PointRef::Center(line.0), p)),
        Err(Error::BadPointRef(PointRef::Center(line.0)))
    );
    assert_eq!(s.constrain(Constraint::distance(p, q, -1.0)), Err(Error::BadDimension(-1.0)));
    assert_eq!(s.constraint_count(), 0);
}

/// A radius of zero is not a dimension a circle can have: the solver used
/// to accept it, drive the circle to a point (radius 5e-24), and leave the
/// failure to whatever consumed the sketch, as a kernel message about
/// planes. It is refused at the constraint, by name. A distance of zero
/// stays legal: it is how a point is put on a line.
#[test]
fn a_zero_radius_is_refused_at_the_constraint() {
    let mut s = System::new();
    let o = s.point(0.0, 0.0);
    let c = s.circle(o, 5.0);
    let q = s.point(3.0, 0.0);
    assert_eq!(s.constrain(Constraint::radius(c, 0.0)), Err(Error::BadRadius(0.0)));
    assert!(Error::BadRadius(0.0).to_string().contains("positive"), "{}", Error::BadRadius(0.0));
    assert_eq!(s.constraint_count(), 0);
    assert!(s.constrain(Constraint::radius(c, 2.0)).is_ok());
    assert!(s.constrain(Constraint::distance(o, q, 0.0)).is_ok());
}

#[test]
fn a_removed_constraint_gives_its_freedom_back() {
    let mut s = System::new();
    let p = s.point(0.0, 0.0);
    let id = s.constrain(Constraint::fix(p)).unwrap();
    assert_eq!(s.dof(), 0);
    s.remove_constraint(id);
    assert_eq!(s.dof(), 2);
    assert!(s.constraint(id).is_none());
}
