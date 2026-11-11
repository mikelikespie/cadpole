//! The sketches a user actually draws.

use super::{corner_degrees, dist, near, near_point, point_line_distance};
use crate::{Constraint, PointId, PointRef, System};
use core::f64::consts::PI;

/// The rectangle every sketcher draws first: four lines drawn roughly, their
/// corners pinned together, two sides horizontal, two vertical, and two
/// dimensions. It must land on exactly zero degrees of freedom and on exactly
/// the dimensions asked for.
#[test]
fn a_rectangle_of_four_lines_reaches_zero_dof_and_the_right_size() {
    let mut s = System::new();
    // Eight endpoints, because four separately drawn lines have eight.
    let (a0, a1) = (s.point(0.0, 0.0), s.point(38.0, 1.0));
    let (b0, b1) = (s.point(39.0, 2.0), s.point(41.0, 26.0));
    let (c0, c1) = (s.point(40.0, 24.0), s.point(1.0, 27.0));
    let (d0, d1) = (s.point(0.0, 26.0), s.point(-1.0, 1.0));
    let bottom = s.line(a0, a1);
    let right = s.line(b0, b1);
    let top = s.line(c0, c1);
    let left = s.line(d0, d1);

    s.constrain(Constraint::coincident(a1, b0)).unwrap();
    s.constrain(Constraint::coincident(b1, c0)).unwrap();
    s.constrain(Constraint::coincident(c1, d0)).unwrap();
    s.constrain(Constraint::coincident(d1, a0)).unwrap();
    s.constrain(Constraint::horizontal(bottom)).unwrap();
    s.constrain(Constraint::horizontal(top)).unwrap();
    s.constrain(Constraint::vertical(right)).unwrap();
    s.constrain(Constraint::vertical(left)).unwrap();
    s.constrain_named("width", Constraint::distance(a0, a1, 40.0)).unwrap();
    s.constrain_named("height", Constraint::distance(b0, b1, 25.0)).unwrap();
    s.constrain(Constraint::fix(a0)).unwrap();

    let r = s.solve();
    assert!(r.is_converged(), "{r}");
    assert_eq!(r.dof, 0, "{r}");
    assert!(r.max_residual < 1e-9, "{r}");

    near_point(s.point_pos(a0), (0.0, 0.0), 1e-9, "bottom left");
    near_point(s.point_pos(a1), (40.0, 0.0), 1e-9, "bottom right");
    near_point(s.point_pos(b1), (40.0, 25.0), 1e-9, "top right");
    near_point(s.point_pos(c1), (0.0, 25.0), 1e-9, "top left");

    let d = s.diagnose();
    assert!(d.is_fully_constrained(), "{d}");
    assert!(d.redundant.is_empty(), "{d}");
    assert_eq!(d.parameters, 16);
    assert_eq!(d.equations, 16);
    assert_eq!(d.rank, 16);
}

/// An isosceles triangle: two sides equal, one angle dimensioned.
#[test]
fn a_triangle_with_two_equal_sides_and_an_angle() {
    let mut s = System::new();
    let p0 = s.point(0.0, 0.0);
    let p1 = s.point(30.0, 2.0);
    let p2 = s.point(12.0, 20.0);
    let base = s.line(p0, p1);
    let side = s.line(p1, p2);

    s.constrain(Constraint::fix(p0)).unwrap();
    s.constrain(Constraint::horizontal(base)).unwrap();
    s.constrain_named("base", Constraint::distance(p0, p1, 30.0)).unwrap();
    s.constrain_named("legs equal", Constraint::equal_length(base, side)).unwrap();
    s.constrain_named("apex", Constraint::angle_degrees(base, side, 130.0)).unwrap();

    let r = s.solve();
    assert!(r.is_converged(), "{r}");
    assert_eq!(r.dof, 0, "{r}");

    near(dist(&s, p0, p1), 30.0, 1e-9, "base length");
    near(dist(&s, p1, p2), 30.0, 1e-9, "side length");
    // Turning 130° left off the base leaves a 50° interior angle at p1.
    near(corner_degrees(&s, p0, p1, p2), 50.0, 1e-8, "interior angle");
    assert!(s.point_pos(p2).1 > 0.0, "the apex stayed on the side it started");
}

/// A line tangent to a circle, checked the way a draughtsman would: the
/// distance from the centre to the line equals the radius.
#[test]
fn a_line_tangent_to_a_circle_sits_exactly_one_radius_away() {
    let mut s = System::new();
    let centre = s.point(0.0, 0.0);
    let circle = s.circle(centre, 7.0);
    // A nearly vertical line 15 mm away, which is not yet tangent.
    let (p0, p1) = (s.point(15.0, -30.0), s.point(16.0, 30.0));
    let line = s.line(p0, p1);

    s.constrain(Constraint::fix(centre)).unwrap();
    s.constrain(Constraint::radius(circle, 10.0)).unwrap();
    s.constrain(Constraint::vertical(line)).unwrap();
    s.constrain(Constraint::tangent(line, circle)).unwrap();

    let r = s.solve();
    assert!(r.is_converged(), "{r}");
    // Both endpoints may still slide along the line: two degrees of freedom.
    assert_eq!(r.dof, 2, "{r}");

    let d = point_line_distance((0.0, 0.0), s.point_pos(p0), s.point_pos(p1));
    near(d, 10.0, 1e-9, "centre to tangent line");
    near(s.point_pos(p0).0, 10.0, 1e-9, "the line moved in to x = r");
    near(s.point_pos(p1).0, 10.0, 1e-9, "and stayed vertical");
}

/// Concentric circles with their radii dimensioned: a bearing seat.
#[test]
fn concentric_circles_share_a_centre_and_keep_their_radii() {
    let mut s = System::new();
    let inner_c = s.point(0.0, 0.0);
    let outer_c = s.point(-3.0, 2.0);
    let inner = s.circle(inner_c, 8.0);
    let outer = s.circle(outer_c, 20.0);

    s.constrain(Constraint::fix(inner_c)).unwrap();
    s.constrain(Constraint::concentric(inner, outer)).unwrap();
    s.constrain_named("bore", Constraint::radius(inner, 10.0)).unwrap();
    s.constrain_named("seat", Constraint::radius(outer, 25.0)).unwrap();

    let r = s.solve();
    assert!(r.is_converged(), "{r}");
    assert_eq!(r.dof, 0, "{r}");
    near_point(s.point_pos(outer_c), (0.0, 0.0), 1e-9, "outer centre");
    near(s.radius(inner).unwrap(), 10.0, 1e-10, "bore");
    near(s.radius(outer).unwrap(), 25.0, 1e-10, "seat");
}

/// Equal radii are a relation between two circles rather than a dimension on
/// each: constrain one and the other follows.
#[test]
fn equal_radius_ties_two_circles_together() {
    let mut s = System::new();
    let ca = s.point(0.0, 0.0);
    let cb = s.point(40.0, 0.0);
    let a = s.circle(ca, 6.0);
    let b = s.circle(cb, 11.0);
    s.constrain(Constraint::equal_radius(a, b)).unwrap();
    s.constrain(Constraint::radius(a, 9.0)).unwrap();

    assert!(s.solve().is_converged());
    near(s.radius(a).unwrap(), 9.0, 1e-10, "a");
    near(s.radius(b).unwrap(), 9.0, 1e-10, "b");
}

/// Two points mirrored about a construction axis.
#[test]
fn symmetric_points_mirror_across_a_construction_axis() {
    let mut s = System::new();
    let q0 = s.point(0.0, -10.0);
    let q1 = s.point(0.0, 10.0);
    let axis = s.line(q0, q1);
    s.set_construction(axis, true);
    let pa = s.point(-5.0, 3.0);
    let pb = s.point(6.0, 5.0);

    s.constrain(Constraint::fix(q0)).unwrap();
    s.constrain(Constraint::fix(q1)).unwrap();
    s.constrain_named("mirror", Constraint::symmetric(pa, pb, axis)).unwrap();
    s.constrain_named("span", Constraint::distance(pa, pb, 12.0)).unwrap();

    let r = s.solve();
    assert!(r.is_converged(), "{r}");
    // The shared height is still free: one degree of freedom.
    assert_eq!(r.dof, 1, "{r}");

    let (a, b) = (s.point_pos(pa), s.point_pos(pb));
    near(a.0, -6.0, 1e-9, "left x");
    near(b.0, 6.0, 1e-9, "right x");
    near(a.1, b.1, 1e-9, "shared height");
    assert!(s.is_construction(axis));
}

/// A slot: two arcs capping two tangent lines. This is the shape a real sketch
/// needs and the one that exercises arc endpoints, tangency at a shared point,
/// equal radii and a construction centreline at once.
#[test]
fn a_slot_is_two_lines_tangent_to_two_arc_caps() {
    let mut s = System::new();
    // Deliberately sloppy starting geometry: the caps are different sizes, the
    // centreline is not level, and the flanks are not tangent.
    let ca = s.point(0.0, 0.0);
    let cb = s.point(28.0, 3.0);
    let cap_l = s.arc(ca, 5.0, 0.5 * PI, 1.5 * PI);
    let cap_r = s.arc(cb, 4.0, -0.5 * PI, 0.5 * PI);
    let (f0, f1) = (s.point(0.5, -4.5), s.point(27.0, -1.5));
    let (g0, g1) = (s.point(28.0, 7.5), s.point(-0.5, 5.5));
    let bottom = s.line(f0, f1);
    let top = s.line(g0, g1);
    let centreline = s.line(ca, cb);
    s.set_construction(centreline, true);

    // Round the outline: arc end → line → arc start, all the way round.
    s.constrain(Constraint::coincident(PointRef::start(bottom), PointRef::end(cap_l))).unwrap();
    s.constrain(Constraint::coincident(PointRef::end(bottom), PointRef::start(cap_r))).unwrap();
    s.constrain(Constraint::coincident(PointRef::start(top), PointRef::end(cap_r))).unwrap();
    s.constrain(Constraint::coincident(PointRef::end(top), PointRef::start(cap_l))).unwrap();

    // Tangency *at the shared endpoint*, not curve-to-curve: see the note on
    // `Constraint::Tangent`.
    s.constrain(Constraint::tangent_at(bottom, cap_l, PointRef::start(bottom))).unwrap();
    s.constrain(Constraint::tangent_at(bottom, cap_r, PointRef::end(bottom))).unwrap();
    s.constrain(Constraint::tangent_at(top, cap_r, PointRef::start(top))).unwrap();
    s.constrain(Constraint::tangent_at(top, cap_l, PointRef::end(top))).unwrap();

    s.constrain_named("caps equal", Constraint::equal_radius(cap_l, cap_r)).unwrap();
    s.constrain_named("slot radius", Constraint::radius(cap_l, 5.0)).unwrap();
    s.constrain_named("level", Constraint::horizontal(centreline)).unwrap();
    s.constrain_named("slot length", Constraint::distance(ca, cb, 30.0)).unwrap();
    s.constrain(Constraint::fix(ca)).unwrap();

    let r = s.solve();
    assert!(r.is_converged(), "{r}");
    assert_eq!(r.dof, 0, "{r}");
    let d = s.diagnose();
    assert!(d.redundant.is_empty() && d.conflicting.is_empty(), "{d}");

    // A 30 mm slot on 5 mm radii: flanks 30 mm long at y = ±5.
    near_point(s.point_pos(cb), (30.0, 0.0), 1e-8, "right centre");
    near(s.radius(cap_l).unwrap(), 5.0, 1e-9, "left radius");
    near(s.radius(cap_r).unwrap(), 5.0, 1e-9, "right radius");
    let (b0, b1) = (s.point_pos(f0), s.point_pos(f1));
    let (t0, t1) = (s.point_pos(g0), s.point_pos(g1));
    near(b0.1, -5.0, 1e-8, "bottom flank height");
    near(b1.1, -5.0, 1e-8, "bottom flank height");
    near(t0.1, 5.0, 1e-8, "top flank height");
    near(t1.1, 5.0, 1e-8, "top flank height");
    near((b1.0 - b0.0).abs(), 30.0, 1e-8, "bottom flank length");
    near((t1.0 - t0.0).abs(), 30.0, 1e-8, "top flank length");

    // Tangency proper: each flank is exactly one radius from each centre.
    for (p, q) in [(b0, b1), (t0, t1)] {
        near(point_line_distance((0.0, 0.0), p, q), 5.0, 1e-8, "flank to left centre");
        near(point_line_distance((30.0, 0.0), p, q), 5.0, 1e-8, "flank to right centre");
    }
}

/// Two circles tangent to each other, externally.
#[test]
fn two_circles_can_be_tangent_to_each_other() {
    let mut s = System::new();
    let ca = s.point(0.0, 0.0);
    let cb = s.point(22.0, 0.0);
    let a = s.circle(ca, 10.0);
    let b = s.circle(cb, 6.0);
    s.constrain(Constraint::fix(ca)).unwrap();
    s.constrain(Constraint::radius(a, 10.0)).unwrap();
    s.constrain(Constraint::radius(b, 6.0)).unwrap();
    s.constrain(Constraint::tangent(a, b)).unwrap();
    let line = s.line(ca, cb);
    s.constrain(Constraint::horizontal(line)).unwrap();

    let r = s.solve();
    assert!(r.is_converged(), "{r}");
    assert_eq!(r.dof, 0, "{r}");
    near(s.point_pos(cb).0, 16.0, 1e-9, "centres are r1 + r2 apart");
    near(s.point_pos(cb).1, 0.0, 1e-9, "on the same level");
}

/// A point constrained onto the circle of an arc, and onto a line.
#[test]
fn on_curve_pins_a_point_to_a_circle_and_to_a_line() {
    let mut s = System::new();
    let centre = s.point(0.0, 0.0);
    let circle = s.circle(centre, 10.0);
    let p = s.point(4.0, 4.0);
    let (l0, l1) = (s.point(-20.0, 0.0), s.point(20.0, 0.0));
    let line = s.line(l0, l1);

    s.constrain(Constraint::fix(centre)).unwrap();
    s.constrain(Constraint::radius(circle, 10.0)).unwrap();
    s.constrain(Constraint::fix(l0)).unwrap();
    s.constrain(Constraint::fix(l1)).unwrap();
    s.constrain(Constraint::on_curve(p, circle)).unwrap();
    s.constrain(Constraint::on_curve(p, line)).unwrap();

    let r = s.solve();
    assert!(r.is_converged(), "{r}");
    assert_eq!(r.dof, 0, "{r}");
    // Of the two intersections the solver keeps the one it started nearest.
    near_point(s.point_pos(p), (10.0, 0.0), 1e-9, "intersection");
}

/// Helper used by several tests: a rectangle with named dimensions.
pub(crate) fn rectangle(w: f64, h: f64) -> (System, [PointId; 4]) {
    let mut s = System::new();
    let a = s.point(0.0, 0.0);
    let b = s.point(w * 0.9, 1.0);
    let c = s.point(w * 1.1, h * 0.8);
    let d = s.point(-1.0, h * 1.2);
    let bottom = s.line(a, b);
    let right = s.line(b, c);
    let top = s.line(c, d);
    let left = s.line(d, a);
    s.constrain(Constraint::fix(a)).unwrap();
    s.constrain(Constraint::horizontal(bottom)).unwrap();
    s.constrain(Constraint::horizontal(top)).unwrap();
    s.constrain(Constraint::vertical(right)).unwrap();
    s.constrain(Constraint::vertical(left)).unwrap();
    s.constrain_named("width", Constraint::distance(a, b, w)).unwrap();
    s.constrain_named("height", Constraint::distance(b, c, h)).unwrap();
    (s, [a, b, c, d])
}
