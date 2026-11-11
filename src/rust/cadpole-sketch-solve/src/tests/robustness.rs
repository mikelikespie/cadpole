//! Poor initial guesses, determinism, and input that would make a naive
//! Newton step blow up.

use super::{dist, near, near_point};
use crate::{Constraint, SolveOptions, SolveStatus, System};

/// An equilateral triangle asked for from three points that all start within a
/// hundredth of a millimetre of the origin. Plain Newton takes an enormous
/// first step here; the trust region has to walk it in.
#[test]
fn a_poor_initial_guess_still_converges() {
    let mut s = System::new();
    let p0 = s.point(0.0, 0.0);
    let p1 = s.point(0.01, 0.0);
    let p2 = s.point(0.004, 0.008);
    let base = s.line(p0, p1);

    s.constrain(Constraint::fix(p0)).unwrap();
    s.constrain(Constraint::horizontal(base)).unwrap();
    s.constrain(Constraint::distance(p0, p1, 50.0)).unwrap();
    s.constrain(Constraint::distance(p1, p2, 50.0)).unwrap();
    s.constrain(Constraint::distance(p2, p0, 50.0)).unwrap();

    let r = s.solve();
    assert!(r.is_converged(), "{r}");
    assert_eq!(r.dof, 0, "{r}");
    near(dist(&s, p0, p1), 50.0, 1e-8, "side");
    near(dist(&s, p1, p2), 50.0, 1e-8, "side");
    near(dist(&s, p2, p0), 50.0, 1e-8, "side");
}

/// The same rectangle as always, but every corner starts somewhere silly —
/// including on the wrong side of the sketch.
#[test]
fn a_rectangle_converges_from_scattered_corners() {
    let mut s = System::new();
    let a = s.point(0.0, 0.0);
    let b = s.point(-120.0, 300.0);
    let c = s.point(5.0, -80.0);
    let d = s.point(200.0, 2.0);
    let bottom = s.line(a, b);
    let right = s.line(b, c);
    let top = s.line(c, d);
    let left = s.line(d, a);
    s.constrain(Constraint::fix(a)).unwrap();
    s.constrain(Constraint::horizontal(bottom)).unwrap();
    s.constrain(Constraint::horizontal(top)).unwrap();
    s.constrain(Constraint::vertical(right)).unwrap();
    s.constrain(Constraint::vertical(left)).unwrap();
    s.constrain(Constraint::distance(a, b, 40.0)).unwrap();
    s.constrain(Constraint::distance(b, c, 25.0)).unwrap();

    let r = s.solve();
    assert!(r.is_converged(), "{r}");
    assert_eq!(r.dof, 0, "{r}");
    near(dist(&s, a, b), 40.0, 1e-8, "width");
    near(dist(&s, b, c), 25.0, 1e-8, "height");
    near(s.point_pos(b).1, 0.0, 1e-8, "bottom is level");
}

/// Circles found from a long way off: an intersection problem with two roots
/// where the solver must find the one it started nearest.
#[test]
fn an_intersection_converges_to_the_nearer_root() {
    let build = |start: (f64, f64)| {
        let mut s = System::new();
        let c0 = s.point(0.0, 0.0);
        let c1 = s.point(30.0, 0.0);
        let p = s.point(start.0, start.1);
        s.constrain(Constraint::fix(c0)).unwrap();
        s.constrain(Constraint::fix(c1)).unwrap();
        s.constrain(Constraint::distance(c0, p, 25.0)).unwrap();
        s.constrain(Constraint::distance(c1, p, 25.0)).unwrap();
        let r = s.solve();
        assert!(r.is_converged(), "{r}");
        s.point_pos(p)
    };
    let expected = (15.0, (625.0f64 - 225.0).sqrt());
    near_point(build((2.0, 60.0)), expected, 1e-8, "upper root");
    near_point(build((2.0, -60.0)), (expected.0, -expected.1), 1e-8, "lower root");
}

/// Two solves of the same sketch must agree bit for bit. Nothing in the solver
/// depends on hashing, timing or randomness, and the tests rely on that.
#[test]
fn solving_is_deterministic() {
    let build = || {
        let (mut s, _) = super::shapes::rectangle(40.0, 25.0);
        s.solve();
        s.parameters().to_vec()
    };
    assert_eq!(build(), build());
}

/// Restoring a saved parameter vector restores the sketch exactly, which is
/// what cancelling a gesture needs.
#[test]
fn parameters_round_trip() {
    let (mut s, [_a, b, _c, _d]) = super::shapes::rectangle(40.0, 25.0);
    s.solve();
    let saved = s.parameters().to_vec();
    s.drag(b, (100.0, 100.0));
    s.set_parameters(&saved);
    assert_eq!(s.parameters(), &saved[..]);
    near_point(s.point_pos(b), (40.0, 0.0), 1e-9, "restored");
}

/// A sketch measured in metres and one measured in millimetres must behave the
/// same way: the row scaling is what makes an angle and a length converge
/// together at any size.
#[test]
fn scale_does_not_change_the_answer() {
    let solve_at = |k: f64| {
        let mut s = System::new();
        let p0 = s.point(0.0, 0.0);
        let p1 = s.point(0.9 * k, 0.05 * k);
        let p2 = s.point(0.3 * k, 0.7 * k);
        let base = s.line(p0, p1);
        let side = s.line(p1, p2);
        s.constrain(Constraint::fix(p0)).unwrap();
        s.constrain(Constraint::horizontal(base)).unwrap();
        s.constrain(Constraint::distance(p0, p1, k)).unwrap();
        s.constrain(Constraint::distance(p1, p2, 0.6 * k)).unwrap();
        s.constrain(Constraint::angle_degrees(base, side, 140.0)).unwrap();
        let r = s.solve();
        assert!(r.is_converged(), "at scale {k}: {r}");
        let p = s.point_pos(p2);
        (p.0 / k, p.1 / k)
    };
    let small = solve_at(0.04);
    let large = solve_at(4000.0);
    near_point(small, large, 1e-9, "same shape at any size");
}

/// Degenerate input must be reported, not panicked on: an angle between two
/// zero-length lines has no direction and therefore no derivative to offer.
/// The row is identically zero, so the constraint has no effect, and that is
/// what the diagnosis says.
#[test]
fn a_degenerate_line_is_reported_not_panicked_on() {
    let mut s = System::new();
    let a = s.point(1.0, 1.0);
    let b = s.point(1.0, 1.0);
    let c = s.point(2.0, 2.0);
    let d = s.point(2.0, 2.0);
    let l0 = s.line(a, b);
    let l1 = s.line(c, d);
    s.constrain(Constraint::fix(a)).unwrap();
    s.constrain(Constraint::fix(b)).unwrap();
    s.constrain(Constraint::fix(c)).unwrap();
    s.constrain(Constraint::fix(d)).unwrap();
    s.constrain_named("no direction", Constraint::angle_degrees(l0, l1, 90.0)).unwrap();
    let r = s.solve();
    assert!(r.is_converged(), "{r}");
    assert_eq!(s.dof(), 0);
    let d = s.diagnose();
    assert_eq!(d.redundant.len(), 1, "{d}");
    assert_eq!(d.redundant[0].label, "no direction");
    assert!(d.redundant[0].implied_by.is_empty());
    assert!(format!("{d}").contains("no direction has no effect"), "{d}");
}

/// An empty system is legal and trivially solved.
#[test]
fn an_empty_system_solves() {
    let mut s = System::new();
    let r = s.solve();
    assert_eq!(r.status, SolveStatus::Converged);
    assert_eq!(r.dof, 0);
    assert_eq!(s.diagnose().equations, 0);
}

/// The iteration cap is honoured and reported rather than being exceeded
/// silently.
#[test]
fn the_iteration_cap_is_reported() {
    let opts = SolveOptions { max_iterations: 1, ..SolveOptions::default() };
    let mut s = System::with_options(opts);
    let p0 = s.point(0.0, 0.0);
    let p1 = s.point(0.001, 0.0);
    let p2 = s.point(0.0, 0.001);
    let base = s.line(p0, p1);
    s.constrain(Constraint::fix(p0)).unwrap();
    s.constrain(Constraint::horizontal(base)).unwrap();
    s.constrain(Constraint::distance(p0, p1, 900.0)).unwrap();
    s.constrain(Constraint::distance(p1, p2, 900.0)).unwrap();
    s.constrain(Constraint::distance(p2, p0, 900.0)).unwrap();
    let r = s.solve();
    assert_eq!(r.status, SolveStatus::MaxIterations, "{r}");
    assert_eq!(r.iterations, 1);
}
