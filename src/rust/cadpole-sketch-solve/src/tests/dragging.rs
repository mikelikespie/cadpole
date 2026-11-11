//! Dragging: move one point, and the rest of the sketch follows while every
//! constraint keeps holding.

use super::{near, near_point};
use crate::{Constraint, System};

/// A rectangle with its height dimensioned but not its width. Dragging the
/// bottom-right corner must widen the rectangle, carry the corner above it
/// along, pull the dragged corner back onto the horizontal it is constrained
/// to, and leave the fixed corner alone.
#[test]
fn dragging_a_corner_moves_the_rest_and_keeps_the_constraints() {
    let mut s = System::new();
    let a = s.point(0.0, 0.0);
    let b = s.point(40.0, 0.0);
    let c = s.point(40.0, 25.0);
    let d = s.point(0.0, 25.0);
    let bottom = s.line(a, b);
    let right = s.line(b, c);
    let top = s.line(c, d);
    let left = s.line(d, a);
    s.constrain(Constraint::fix(a)).unwrap();
    s.constrain(Constraint::horizontal(bottom)).unwrap();
    s.constrain(Constraint::horizontal(top)).unwrap();
    s.constrain(Constraint::vertical(right)).unwrap();
    s.constrain(Constraint::vertical(left)).unwrap();
    s.constrain_named("height", Constraint::distance(b, c, 25.0)).unwrap();
    assert!(s.solve().is_converged());
    assert_eq!(s.dof(), 1, "the width is the one thing left free");

    // Drag B out to 65 mm and a little off the axis; the horizontal constraint
    // must pull it back down to y = 0 while the width follows the cursor.
    let r = s.drag(b, (65.0, 6.0));
    assert!(r.is_converged(), "{r}");
    assert!(!r.reached, "y = 6 was never available: {r}");
    assert_eq!(r.solve.dof, 1, "the drag is temporary: the freedom is still there");

    near_point(s.point_pos(a), (0.0, 0.0), 1e-9, "the fixed corner did not move");
    near_point(s.point_pos(b), (65.0, 0.0), 1e-7, "the dragged corner");
    near_point(s.point_pos(c), (65.0, 25.0), 1e-7, "the corner above it followed");
    near_point(s.point_pos(d), (0.0, 25.0), 1e-7, "the far corner stayed put");
    near(s.diagnose().max_residual, 0.0, 1e-9, "every constraint still holds");
}

/// A point constrained onto a circle follows the cursor *around* the circle
/// rather than being torn off it or refusing to move.
#[test]
fn a_drag_that_the_constraints_forbid_lands_as_close_as_it_can() {
    let mut s = System::new();
    let centre = s.point(0.0, 0.0);
    let circle = s.circle(centre, 10.0);
    let p = s.point(10.0, 0.0);
    s.constrain(Constraint::fix(centre)).unwrap();
    s.constrain(Constraint::radius(circle, 10.0)).unwrap();
    s.constrain(Constraint::on_curve(p, circle)).unwrap();
    assert!(s.solve().is_converged());

    let r = s.drag(p, (0.0, 40.0));
    assert!(r.is_converged(), "the sketch is still valid: {r}");
    assert!(!r.reached, "the cursor was off the circle: {r}");
    near(r.solve.max_residual, 0.0, 1e-8, "the real constraints still hold");
    near_point(s.point_pos(p), (0.0, 10.0), 1e-6, "the nearest point of the circle");
    near(r.miss, 30.0, 1e-6, "and it says how far short it fell");
}

/// A drag must move as little as the constraints allow: a point that has
/// nothing to do with the drag must not move at all.
#[test]
fn a_drag_does_not_disturb_what_it_need_not_touch() {
    let mut s = System::new();
    let a = s.point(0.0, 0.0);
    let b = s.point(20.0, 0.0);
    let line = s.line(a, b);
    let unrelated = s.point(-30.0, 12.0);
    s.constrain(Constraint::fix(a)).unwrap();
    s.constrain(Constraint::horizontal(line)).unwrap();
    assert!(s.solve().is_converged());

    let before = s.point_pos(unrelated);
    let r = s.drag(b, (55.0, 9.0));
    assert!(r.is_converged(), "{r}");
    near_point(s.point_pos(b), (55.0, 0.0), 1e-7, "dragged onto the axis");
    near_point(s.point_pos(unrelated), before, 1e-12, "the unrelated point stayed");
}

/// Dragging along a free direction reaches the cursor exactly.
#[test]
fn a_drag_along_a_free_direction_reaches_the_cursor() {
    let mut s = System::new();
    let a = s.point(0.0, 0.0);
    let b = s.point(20.0, 0.0);
    let line = s.line(a, b);
    s.constrain(Constraint::fix(a)).unwrap();
    s.constrain(Constraint::horizontal(line)).unwrap();
    let r = s.drag(b, (77.0, 0.0));
    assert!(r.reached, "{r}");
    near(r.miss, 0.0, 1e-9, "landed on the cursor");
    near_point(s.point_pos(b), (77.0, 0.0), 1e-9, "b");
}

/// A stream of drags is what a pointer actually produces. On a fully
/// dimensioned sketch every one of them is refused, and the sketch must come
/// through unharmed rather than drifting a little each time.
#[test]
fn a_stream_of_drags_leaves_a_fully_constrained_sketch_alone() {
    let (mut s, [a, b, _c, _d]) = super::shapes::rectangle(40.0, 25.0);
    assert!(s.solve().is_converged());
    for step in 0..8 {
        let t = step as f64;
        let r = s.drag(b, (40.0 + t, 3.0 - 0.5 * t));
        assert!(r.is_converged(), "the sketch is still solved: {r}");
        assert!(!r.reached, "a fully constrained corner cannot be dragged: {r}");
        near(r.solve.max_residual, 0.0, 1e-8, "no drift");
    }
    near_point(s.point_pos(a), (0.0, 0.0), 1e-9, "corner");
    near_point(s.point_pos(b), (40.0, 0.0), 1e-6, "corner");
}
