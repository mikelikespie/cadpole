//! Degrees of freedom, redundancy and conflict — the part a sketch UI lives
//! on. Every case here checks that a *specific* constraint is named, not that
//! something vague went wrong.

use super::near;
use crate::{Constraint, SolveStatus, System};

/// A rectangle whose width is dimensioned twice, consistently. The second
/// dimension is redundant: the sketch still solves, but the extra constraint
/// has to be named so the UI can offer to delete it.
#[test]
fn a_redundant_dimension_is_identified_by_name() {
    let (mut s, [_a, _b, c, d]) = super::shapes::rectangle(40.0, 25.0);
    // The top edge's width is already implied by the bottom edge's width and
    // the four horizontal/vertical constraints.
    s.constrain_named("top width", Constraint::distance(d, c, 40.0)).unwrap();

    let r = s.solve();
    assert!(r.is_converged(), "a consistent redundancy still solves: {r}");
    assert_eq!(r.dof, 0, "{r}");

    let diag = s.diagnose();
    assert!(diag.conflicting.is_empty(), "nothing contradicts anything: {diag}");
    assert_eq!(diag.redundant.len(), 1, "{diag}");

    let dep = &diag.redundant[0];
    assert_eq!(dep.label, "top width", "the newest constraint takes the blame");
    // And it says *why*: the constraints that already implied it.
    assert!(
        dep.implied_by_labels.contains(&"width".to_string()),
        "expected the other width dimension in {:?}",
        dep.implied_by_labels
    );
    assert!(diag.is_over_constrained());
    assert!(format!("{diag}").contains("redundant: top width is already implied by"), "{diag}");
}

/// The same rectangle, but the second width disagrees with the first. That is
/// not redundancy, it is a contradiction, and it must be reported as one
/// rather than silently landing halfway between 40 and 45.
#[test]
fn a_conflicting_dimension_is_reported_as_conflicting() {
    let (mut s, [_a, _b, c, d]) = super::shapes::rectangle(40.0, 25.0);
    s.constrain_named("top width", Constraint::distance(d, c, 45.0)).unwrap();

    let r = s.solve();
    assert_eq!(r.status, SolveStatus::Inconsistent, "{r}");
    assert!(r.max_residual > 1.0, "the error is real, not round-off: {r}");

    let diag = s.diagnose();
    assert!(!diag.satisfied);
    assert_eq!(diag.conflicting.len(), 1, "{diag}");
    let dep = &diag.conflicting[0];
    assert_eq!(dep.label, "top width");
    assert!(
        dep.implied_by_labels.contains(&"width".to_string()),
        "expected the conflicting partner in {:?}",
        dep.implied_by_labels
    );
    // The gap is the size of the contradiction: 45 asked for, 40 available.
    near(dep.gap.abs(), 5.0, 1e-6, "contradiction size");
    assert!(diag.redundant.is_empty(), "{diag}");
    assert!(format!("{diag}").contains("conflicting: top width contradicts"), "{diag}");

    // And the individual constraints carrying the error are listed.
    assert!(!diag.unsatisfied.is_empty());
}

/// Two dimensions on the same pair of points that cannot both hold. There is
/// nothing structural to find here — a degree-of-freedom count says the
/// sketch is fine — so this is purely the rank test earning its keep.
#[test]
fn two_lengths_on_one_line_conflict() {
    let mut s = System::new();
    let (a, b) = (s.point(0.0, 0.0), s.point(10.0, 0.0));
    s.constrain(Constraint::fix(a)).unwrap();
    let l = s.line(a, b);
    s.constrain(Constraint::horizontal(l)).unwrap();
    s.constrain_named("short", Constraint::distance(a, b, 20.0)).unwrap();
    s.constrain_named("long", Constraint::distance(a, b, 32.0)).unwrap();

    let r = s.solve();
    assert_eq!(r.status, SolveStatus::Inconsistent, "{r}");
    let diag = s.diagnose();
    assert_eq!(diag.conflicting.len(), 1, "{diag}");
    assert_eq!(diag.conflicting[0].label, "long");
    assert_eq!(diag.conflicting[0].implied_by_labels, vec!["short".to_string()]);
    near(diag.conflicting[0].gap.abs(), 12.0, 1e-6, "contradiction size");
}

/// A theorem-induced dependency: the three angles of a triangle sum to 180°,
/// so dimensioning all three is over-constrained even though no two of them
/// share a point in the obvious way. A structural degree-of-freedom count
/// misses this; the Jacobian's rank does not (the witness
/// method).
#[test]
fn the_third_angle_of_a_triangle_is_redundant() {
    let mut s = System::new();
    let p0 = s.point(0.0, 0.0);
    let p1 = s.point(40.0, 0.0);
    let p2 = s.point(18.0, 22.0);
    let ab = s.line(p0, p1);
    let bc = s.line(p1, p2);
    let ca = s.line(p2, p0);

    s.constrain(Constraint::fix(p0)).unwrap();
    s.constrain(Constraint::fix(p1)).unwrap();
    // Exterior turns at each corner: they must add to 360°.
    s.constrain_named("angle A", Constraint::angle_degrees(ab, bc, 130.0)).unwrap();
    s.constrain_named("angle B", Constraint::angle_degrees(bc, ca, 120.0)).unwrap();
    s.constrain_named("angle C", Constraint::angle_degrees(ca, ab, 110.0)).unwrap();

    let r = s.solve();
    assert!(r.is_converged(), "{r}");
    let diag = s.diagnose();
    assert_eq!(diag.redundant.len(), 1, "{diag}");
    assert_eq!(diag.redundant[0].label, "angle C");
    assert!(diag.conflicting.is_empty(), "{diag}");
    assert_eq!(diag.dof, 0);
}

/// The same triangle with angles that do not add up: now it is a conflict.
#[test]
fn angles_that_do_not_add_up_conflict() {
    let mut s = System::new();
    let p0 = s.point(0.0, 0.0);
    let p1 = s.point(40.0, 0.0);
    let p2 = s.point(18.0, 22.0);
    let ab = s.line(p0, p1);
    let bc = s.line(p1, p2);
    let ca = s.line(p2, p0);
    s.constrain(Constraint::fix(p0)).unwrap();
    s.constrain(Constraint::fix(p1)).unwrap();
    s.constrain_named("angle A", Constraint::angle_degrees(ab, bc, 130.0)).unwrap();
    s.constrain_named("angle B", Constraint::angle_degrees(bc, ca, 120.0)).unwrap();
    s.constrain_named("angle C", Constraint::angle_degrees(ca, ab, 95.0)).unwrap();

    let r = s.solve();
    assert_eq!(r.status, SolveStatus::Inconsistent, "{r}");
    let diag = s.diagnose();
    assert_eq!(diag.conflicting.len(), 1, "{diag}");
    assert_eq!(diag.conflicting[0].label, "angle C");
    for other in ["angle A", "angle B"] {
        assert!(
            diag.conflicting[0].implied_by_labels.contains(&other.to_string()),
            "expected {other} in {:?}",
            diag.conflicting[0].implied_by_labels
        );
    }
}

/// An under-constrained sketch says how much freedom is left *and where*.
#[test]
fn under_constrained_entities_are_named() {
    let mut s = System::new();
    let a = s.point(0.0, 0.0);
    let b = s.point(30.0, 0.0);
    let loose = s.point(5.0, 9.0);
    let l = s.line(a, b);
    s.constrain(Constraint::fix(a)).unwrap();
    s.constrain(Constraint::horizontal(l)).unwrap();

    s.solve();
    let diag = s.diagnose();
    // b's x, and both of the loose point's coordinates.
    assert_eq!(diag.dof, 3, "{diag}");
    assert_eq!(diag.free_entities, vec![b.0, loose.0], "{diag}");
    assert!(format!("{diag}").starts_with("under-constrained: 3 DoF"), "{diag}");
}

/// Fixing the same point twice is the simplest redundancy there is, and a
/// common one when a script fixes an entity a helper already fixed.
#[test]
fn fixing_a_point_twice_is_redundant_not_conflicting() {
    let mut s = System::new();
    let p = s.point(2.0, 3.0);
    s.constrain_named("anchor", Constraint::fix(p)).unwrap();
    s.constrain_named("anchor again", Constraint::fix(p)).unwrap();
    assert!(s.solve().is_converged());
    let diag = s.diagnose();
    assert_eq!(diag.redundant.len(), 1, "{diag}");
    assert_eq!(diag.redundant[0].label, "anchor again");
    assert_eq!(diag.redundant[0].implied_by_labels, vec!["anchor".to_string()]);
}
