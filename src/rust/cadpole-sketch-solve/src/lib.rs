//! Cadpole's 2D sketch constraint solver.
//!
//! A [`System`] is a set of *entities* (points, lines, circles, arcs) over one
//! flat parameter vector and a set of *constraints* over them. [`System::solve`]
//! moves the entities until every constraint holds, starting from where they
//! already are, so the answer is the nearest solution rather than an arbitrary
//! one. [`System::diagnose`] says how many degrees of freedom are left, which
//! entities still have them, and — by name — which constraints are redundant
//! or in conflict. [`System::drag`] re-solves with one point pinned to the
//! cursor, which is how a sketch UI moves geometry without losing constraints.
//!
//! This crate is standalone: no third-party dependencies, no `unsafe`, no
//! randomness, and nothing platform-specific, so it builds for wasm32 and for
//! every native target unchanged.
//!
//! # Why we wrote our own
//!
//! Every mature 2D constraint solver is copyleft — SolveSpace is GPL-3.0,
//! FreeCAD's planegcs LGPL-2.1+ — and Cadpole ships Apache-2.0 with a
//! permissive-only dependency policy. The literature
//! is open even where the code is not, so this is a from-scratch implementation
//! of the standard approach: analytic Jacobians,
//! damped Gauss–Newton from the current configuration, and *numerical*
//! diagnostics from the Jacobian's rank rather than a graph analysis, because
//! a graph cannot see the theorem-induced dependences a real sketch is full of
//! (the fourth side of a rectangle, the third angle of a triangle).
//!
//! # A worked example
//!
//! ```
//! use cadpole_sketch_solve::{Constraint, System};
//!
//! // A rectangle from four lines whose corners are pinned together.
//! let mut s = System::new();
//! let (a, b) = (s.point(0.0, 0.0), s.point(30.0, 2.0));
//! let (c, d) = (s.point(28.0, 18.0), s.point(-1.0, 20.0));
//! let bottom = s.line(a, b);
//! let right = s.line(b, c);
//! let top = s.line(c, d);
//! let left = s.line(d, a);
//!
//! s.constrain(Constraint::fix(a)).unwrap();
//! s.constrain(Constraint::horizontal(bottom)).unwrap();
//! s.constrain(Constraint::horizontal(top)).unwrap();
//! s.constrain(Constraint::vertical(right)).unwrap();
//! s.constrain(Constraint::vertical(left)).unwrap();
//! s.constrain_named("width", Constraint::distance(a, b, 40.0)).unwrap();
//! s.constrain_named("height", Constraint::distance(b, c, 25.0)).unwrap();
//!
//! let result = s.solve();
//! assert!(result.is_converged());
//! assert_eq!(result.dof, 0);
//! let (x, y) = s.point_pos(c);
//! assert!((x - 40.0).abs() < 1e-9 && (y - 25.0).abs() < 1e-9, "{x} {y}");
//! ```
//!
//! # The entity model
//!
//! Parameterisation is minimal, because every parameter is a degree of freedom
//! the user has to constrain away:
//!
//! | entity     | built from          | parameters              |
//! | ---------- | ------------------- | ----------------------- |
//! | **point**  | —                   | `x`, `y`                |
//! | **line**   | two points          | *none*                  |
//! | **circle** | a centre point      | `r`                     |
//! | **arc**    | a centre point      | `r`, `start`, `end` (radians) |
//!
//! A line owns no coordinates, so two lines that share a corner share it
//! exactly and need no constraint to stay together. Anything point-like is a
//! [`PointRef`]: `PointRef::Start(line)`, `PointRef::End(arc)`,
//! `PointRef::Center(circle)`, or a plain [`PointId`], and every constraint
//! that takes a point takes any of them. An arc endpoint is
//! `centre + r·(cos θ, sin θ)`, and the solver differentiates through that, so
//! a line can be made coincident with an arc endpoint and the arc's angle will
//! move to suit.
//!
//! Any entity can be flagged as [construction](System::set_construction)
//! geometry — a centreline or a symmetry axis. The solver treats it like any
//! other entity; the flag is for the caller deciding what becomes a profile.
//!
//! # The constraint catalogue
//!
//! These are the constraints `docs/starlark-api.md` promises, and the
//! [`Constraint`] constructor for each. "Rows" is how many scalar equations
//! the constraint contributes, which is also how many degrees of freedom it
//! removes when it is independent.
//!
//! | Starlark            | constructor                              | rows |
//! | ------------------- | ---------------------------------------- | ---- |
//! | `coincident(a, b)`  | [`Constraint::coincident`]                | 2    |
//! | `horizontal(l)`     | [`Constraint::horizontal`]                | 1    |
//! | `vertical(l)`       | [`Constraint::vertical`]                  | 1    |
//! | `distance(a, b, d)` | [`Constraint::distance`], [`Constraint::point_line_distance`] | 1 |
//! | `angle(l1, l2, deg)`| [`Constraint::angle_degrees`]             | 1    |
//! | `parallel(a, b)`    | [`Constraint::parallel`]                  | 1    |
//! | `perpendicular(a,b)`| [`Constraint::perpendicular`]             | 1    |
//! | `tangent(a, b)`     | [`Constraint::tangent`], [`Constraint::tangent_at`] | 1 |
//! | `equal(a, b)`       | [`Constraint::equal_length`], [`Constraint::equal_radius`] | 1 |
//! | `symmetric(a,b,ax)` | [`Constraint::symmetric`]                 | 2    |
//! | `fix(entity)`       | [`Constraint::fix`], [`Constraint::point_at`] | 2–5 |
//! | `radius(c, r)`      | [`Constraint::radius`]                    | 1    |
//! | `concentric(a, b)`  | [`Constraint::concentric`]                | 2    |
//! | `midpoint(p, l)`    | [`Constraint::midpoint`]                  | 2    |
//! | `on_curve(p, c)`    | [`Constraint::on_curve`]                  | 1    |
//!
//! `equal` and `distance` are two constructors each because the Starlark name
//! is overloaded on its operand types; the script layer dispatches. `tangent`
//! is two because the residual has to be written differently when the curves
//! already touch at a known point — see [`Constraint::Tangent`].
//!
//! Constraints are added with [`System::constrain`], or with
//! [`System::constrain_named`] to give them the name the script used. Names
//! are worth setting: they are what diagnostics print.
//!
//! # Reading the diagnosis
//!
//! ```
//! use cadpole_sketch_solve::{Constraint, System};
//!
//! let mut s = System::new();
//! let (a, b) = (s.point(0.0, 0.0), s.point(10.0, 0.0));
//! s.constrain(Constraint::fix(a)).unwrap();
//! let l = s.line(a, b);
//! s.constrain(Constraint::horizontal(l)).unwrap();
//! s.constrain_named("length", Constraint::distance(a, b, 25.0)).unwrap();
//! // The same length again: implied, not contradicted.
//! s.constrain_named("length again", Constraint::distance(a, b, 25.0)).unwrap();
//!
//! s.solve();
//! let d = s.diagnose();
//! assert_eq!(d.dof, 0);
//! assert!(d.conflicting.is_empty());
//! assert_eq!(d.redundant[0].label, "length again");
//! assert_eq!(d.redundant[0].implied_by_labels, vec!["length".to_string()]);
//! ```
//!
//! Had the second distance said 30 mm, it would have appeared in
//! `conflicting` instead, with `gap` reporting the 5 mm contradiction, and
//! [`System::solve`] would have returned [`SolveStatus::Inconsistent`] rather
//! than quietly landing halfway.
//!
//! # What the UI layer needs
//!
//! * [`System::dof`] — the number in the corner of the sketch window.
//! * [`Diagnosis::free_entities`] — what to draw in "not yet constrained"
//!   colour.
//! * [`Diagnosis::redundant`] and [`Diagnosis::conflicting`] — what to
//!   highlight, and what to offer to delete. Each names the offending
//!   constraint *and* the constraints that already implied it.
//! * [`System::drag`] — call it on every pointer move. Measured at 3 ms for a
//!   fifty-entity sketch and 0.2 ms for a ten-entity one, on one core of a
//!   desktop x86-64; run `//src/rust/cadpole-sketch-solve:bench` for your own
//!   numbers. Build with `--config=opt`: a debug build is roughly fifteen times
//!   slower (measured) and will not feel interactive.
//! * [`System::parameters`] / [`System::set_parameters`] — snapshot and
//!   restore, for cancelling a gesture.
//!
//! # Algorithm
//!
//! [`solver`] documents the numerics: Gauss–Newton, Levenberg–Marquardt
//! damping, a Powell dogleg trust region, and a minimum-norm step so that an
//! under-constrained sketch moves as little as possible. [`diagnose`]
//! documents how one rank-revealing QR of the Jacobian's rows yields the
//! degrees of freedom, the free entities and the named redundancies at once.
//! [`linalg`] is the dense linear algebra both rest on, and says why it is a
//! Cholesky and a QR rather than the SVD the design first called for.

#![forbid(unsafe_code)]

pub mod constraint;
pub mod diagnose;
pub mod entity;
pub mod linalg;
pub mod solver;
pub mod system;

#[cfg(test)]
mod tests;

pub use constraint::{Constraint, ConstraintId, Error};
pub use diagnose::{Dependency, Diagnosis};
pub use entity::{ArcId, CircleId, CurveRef, EntityId, EntityKind, LineId, PointId, PointRef};
pub use solver::{SolveOptions, SolveStatus};
pub use system::{DragResult, EntityInfo, SolveResult, System};
