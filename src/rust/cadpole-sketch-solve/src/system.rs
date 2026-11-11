//! The public API: build a system, add constraints, solve, diagnose, drag.

use crate::constraint::{
    Constraint, ConstraintId, ConstraintRecord, Error, fix_params, pick_tangent_mode, row_count,
    rows_of, validate,
};
use crate::diagnose::{Diagnosis, classify};
use crate::entity::{
    ArcId, CircleId, Entities, EntityId, EntityKind, LineId, PointId, PointRef, Unit,
};
use crate::linalg::{self, Mat, norm_inf};
use crate::solver::{self, SolveOptions, SolveStatus};
use core::fmt;

/// What one call to [`System::solve`] did.
#[derive(Clone, Debug, PartialEq)]
pub struct SolveResult {
    pub status: SolveStatus,
    /// Trust-region iterations taken.
    pub iterations: usize,
    /// The largest single constraint error left, in sketch units (radians for
    /// a pure angle constraint).
    pub max_residual: f64,
    /// Degrees of freedom remaining, from the Jacobian's rank.
    pub dof: usize,
}

impl SolveResult {
    pub fn is_converged(&self) -> bool {
        self.status.is_converged()
    }
}

impl fmt::Display for SolveResult {
    fn fmt(&self, f: &mut fmt::Formatter<'_>) -> fmt::Result {
        write!(
            f,
            "{:?} in {} iterations, residual {:.3e}, {} DoF",
            self.status, self.iterations, self.max_residual, self.dof
        )
    }
}

/// What one call to [`System::drag`] did.
#[derive(Clone, Debug, PartialEq)]
pub struct DragResult {
    /// The state of the sketch's *own* constraints after the drag. The
    /// temporary pin is not part of it.
    pub solve: SolveResult,
    /// Did the point actually get to where the cursor was? `false` means the
    /// constraints would not allow it and the point went as far as it could.
    pub reached: bool,
    /// How far from the cursor the point ended up, in sketch units.
    pub miss: f64,
}

impl DragResult {
    /// The sketch still satisfies its own constraints.
    pub fn is_converged(&self) -> bool {
        self.solve.is_converged()
    }
    pub fn status(&self) -> SolveStatus {
        self.solve.status
    }
}

impl fmt::Display for DragResult {
    fn fmt(&self, f: &mut fmt::Formatter<'_>) -> fmt::Result {
        if self.reached {
            write!(f, "reached the cursor; {}", self.solve)
        } else {
            write!(f, "missed the cursor by {:.4}; {}", self.miss, self.solve)
        }
    }
}

/// A read-only view of one entity.
#[derive(Clone, Debug, PartialEq)]
pub struct EntityInfo {
    pub id: EntityId,
    pub kind: EntityKind,
    /// Construction geometry exists only to be constrained against (a
    /// centreline, a symmetry axis). The solver treats it like any other
    /// entity; the caller decides not to turn it into a profile.
    pub construction: bool,
    pub name: Option<String>,
}

/// A 2D sketch as a constraint system.
///
/// ```
/// use cadpole_sketch_solve::{Constraint, System};
///
/// let mut s = System::new();
/// let a = s.point(0.0, 0.0);
/// let b = s.point(30.0, 4.0);
/// let l = s.line(a, b);
/// s.constrain(Constraint::fix(a)).unwrap();
/// s.constrain(Constraint::horizontal(l)).unwrap();
/// s.constrain(Constraint::distance(a, b, 40.0)).unwrap();
///
/// let result = s.solve();
/// assert!(result.is_converged());
/// assert_eq!(result.dof, 0);
/// let (x, y) = s.point_pos(b);
/// assert!((x - 40.0).abs() < 1e-9 && y.abs() < 1e-9, "{x} {y}");
/// ```
#[derive(Clone, Debug, Default)]
pub struct System {
    ents: Entities,
    x: Vec<f64>,
    cons: Vec<ConstraintRecord>,
    active: Vec<bool>,
    options: SolveOptions,
}

impl System {
    pub fn new() -> System {
        System::default()
    }

    pub fn with_options(options: SolveOptions) -> System {
        System { options, ..System::default() }
    }

    pub fn options(&self) -> &SolveOptions {
        &self.options
    }

    pub fn options_mut(&mut self) -> &mut SolveOptions {
        &mut self.options
    }

    // ---- entities ----------------------------------------------------

    /// A free point. Two parameters.
    pub fn point(&mut self, x: f64, y: f64) -> PointId {
        let id = self.ents.add(EntityKind::Point, 2);
        self.x.push(x);
        self.x.push(y);
        PointId(id)
    }

    /// A segment between two existing points. No parameters of its own, so
    /// two lines that share a corner share it exactly.
    pub fn line(&mut self, a: PointId, b: PointId) -> LineId {
        LineId(self.ents.add(EntityKind::Line { a, b }, 0))
    }

    /// A full circle about an existing point. One parameter, the radius.
    pub fn circle(&mut self, center: PointId, r: f64) -> CircleId {
        let id = self.ents.add(EntityKind::Circle { center }, 1);
        self.x.push(r);
        CircleId(id)
    }

    /// An arc about an existing point, counter-clockwise from `start` to
    /// `end` (radians). Three parameters: radius, start, end. Its endpoints
    /// are [`PointRef::Start`] and [`PointRef::End`] and can be constrained
    /// like any other point.
    pub fn arc(&mut self, center: PointId, r: f64, start: f64, end: f64) -> ArcId {
        let id = self.ents.add(EntityKind::Arc { center }, 3);
        self.x.push(r);
        self.x.push(start);
        self.x.push(end);
        ArcId(id)
    }

    /// Mark an entity as construction geometry (or not).
    pub fn set_construction(&mut self, e: impl Into<EntityId>, construction: bool) {
        let e = e.into();
        self.ents.recs[e.0 as usize].construction = construction;
    }

    pub fn is_construction(&self, e: impl Into<EntityId>) -> bool {
        self.ents.get(e.into()).construction
    }

    /// Give an entity a name, used in diagnostics.
    pub fn name_entity(&mut self, e: impl Into<EntityId>, name: impl Into<String>) {
        let e = e.into();
        self.ents.recs[e.0 as usize].name = Some(name.into());
    }

    pub fn entity_count(&self) -> usize {
        self.ents.recs.len()
    }

    pub fn entity(&self, e: impl Into<EntityId>) -> EntityInfo {
        let e = e.into();
        let r = self.ents.get(e);
        EntityInfo { id: e, kind: r.kind, construction: r.construction, name: r.name.clone() }
    }

    pub fn entities(&self) -> impl Iterator<Item = EntityInfo> + '_ {
        (0..self.ents.recs.len()).map(|i| self.entity(EntityId(i as u32)))
    }

    // ---- constraints -------------------------------------------------

    /// Add a constraint. The returned id names it in diagnostics and can be
    /// passed to [`System::remove_constraint`].
    pub fn constrain(&mut self, c: Constraint) -> Result<ConstraintId, Error> {
        self.add(c, None)
    }

    /// Add a constraint under a caller-chosen name, which is what diagnostics
    /// will print instead of `distance#7`. Give the names the script or the UI
    /// already uses and an over-constrained report reads like the source.
    pub fn constrain_named(
        &mut self,
        name: impl Into<String>,
        c: Constraint,
    ) -> Result<ConstraintId, Error> {
        self.add(c, Some(name.into()))
    }

    fn add(&mut self, c: Constraint, name: Option<String>) -> Result<ConstraintId, Error> {
        validate(&self.ents, &c)?;
        let id = ConstraintId(self.cons.len() as u32);
        let fixed = match &c {
            Constraint::Fix(e) => fix_params(&self.ents, *e)
                .into_iter()
                .map(|(i, u)| (i, self.x[i], u))
                .collect(),
            _ => Vec::new(),
        };
        let tangent_internal = pick_tangent_mode(&self.ents, &c, &self.x);
        let rows = match &c {
            Constraint::Fix(_) => fixed.len(),
            _ => row_count(&self.ents, &c),
        };
        self.cons.push(ConstraintRecord {
            id,
            kind: c,
            name,
            fixed,
            tangent_internal,
            rows,
            weight: 1.0,
        });
        self.active.push(true);
        Ok(id)
    }

    /// Drop a constraint. Ids of the remaining constraints do not change.
    pub fn remove_constraint(&mut self, id: ConstraintId) {
        if let Some(a) = self.active.get_mut(id.0 as usize) {
            *a = false;
        }
    }

    pub fn constraint(&self, id: ConstraintId) -> Option<&Constraint> {
        match self.active.get(id.0 as usize) {
            Some(true) => Some(&self.cons[id.0 as usize].kind),
            _ => None,
        }
    }

    /// The name a diagnostic would print for this constraint.
    pub fn label(&self, id: ConstraintId) -> String {
        self.cons[id.0 as usize].label()
    }

    pub fn constraint_ids(&self) -> impl Iterator<Item = ConstraintId> + '_ {
        self.cons
            .iter()
            .zip(&self.active)
            .filter(|(_, a)| **a)
            .map(|(c, _)| c.id)
    }

    pub fn constraint_count(&self) -> usize {
        self.active.iter().filter(|a| **a).count()
    }

    /// The worst residual of one constraint, in sketch units.
    pub fn residual(&self, id: ConstraintId) -> f64 {
        let rec = &self.cons[id.0 as usize];
        rows_of(&self.ents, rec, &self.x)
            .iter()
            .fold(0.0f64, |m, r| m.max(r.residual.abs()))
    }

    // ---- geometry access ---------------------------------------------

    pub fn point_pos(&self, p: PointId) -> (f64, f64) {
        let b = self.ents.get(p.0).base;
        (self.x[b], self.x[b + 1])
    }

    pub fn set_point(&mut self, p: PointId, x: f64, y: f64) {
        let b = self.ents.get(p.0).base;
        self.x[b] = x;
        self.x[b + 1] = y;
    }

    /// The position of anything point-like: a point, a line end, an arc end,
    /// a circle centre.
    pub fn position(&self, r: PointRef) -> Option<(f64, f64)> {
        self.ents.point(r, &self.x).map(|(p, _)| p)
    }

    pub fn radius(&self, e: impl Into<EntityId>) -> Option<f64> {
        self.ents.radius_param(e.into()).map(|i| self.x[i])
    }

    pub fn set_radius(&mut self, e: impl Into<EntityId>, r: f64) {
        if let Some(i) = self.ents.radius_param(e.into()) {
            self.x[i] = r;
        }
    }

    /// `(start, end)` angles of an arc, in radians.
    pub fn arc_angles(&self, a: ArcId) -> Option<(f64, f64)> {
        match self.ents.get(a.0).kind {
            EntityKind::Arc { .. } => {
                let b = self.ents.get(a.0).base;
                Some((self.x[b + 1], self.x[b + 2]))
            }
            _ => None,
        }
    }

    /// The raw parameter vector. Its layout is an implementation detail; use
    /// it only to save and restore a configuration.
    pub fn parameters(&self) -> &[f64] {
        &self.x
    }

    pub fn set_parameters(&mut self, x: &[f64]) {
        assert_eq!(x.len(), self.x.len(), "parameter vector length");
        self.x.copy_from_slice(x);
    }

    // ---- solving ------------------------------------------------------

    /// Solve from the sketch's current positions.
    ///
    /// The solve is deterministic: the same entities, constraints and starting
    /// values always give the same answer, and the answer is the root nearest
    /// the starting point, not an arbitrary one.
    pub fn solve(&mut self) -> SolveResult {
        let report = self.resolve();
        let (max_residual, dof) = self.stats();
        SolveResult { status: report.status, iterations: report.iterations, max_residual, dof }
    }

    /// Solve without computing the rank. The rank is the expensive half of a
    /// `solve`, and a drag does not need it for its first pass.
    fn resolve(&mut self) -> solver::Report {
        let size = self.characteristic_size();
        let m = self.row_total();
        let mut report = self.minimize_at(size, m);
        // A very poor starting guess can make the sketch look far bigger than
        // it turns out to be, and the tolerance is relative to that size. Solve
        // once more against the size it actually settled at, so "converged"
        // means the same thing however bad the guess was.
        let settled = self.characteristic_size();
        if settled < 0.5 * size {
            let again = self.minimize_at(settled, m);
            report.status = again.status;
            report.iterations += again.iterations;
        }
        report
    }

    fn minimize_at(&mut self, size: f64, m: usize) -> solver::Report {
        let mut x = core::mem::take(&mut self.x);
        let report = {
            let ents = &self.ents;
            let cons: Vec<&ConstraintRecord> = self.active_records();
            let mut full = |x: &[f64], r: &mut Vec<f64>, j: &mut Mat| {
                fill(ents, &cons, x, size, Some(j), r, None);
            };
            let mut only_r = |x: &[f64], r: &mut Vec<f64>| {
                fill(ents, &cons, x, size, None, r, None);
            };
            solver::minimize(&mut x, m, &self.options, size, &mut full, &mut only_r)
        };
        self.x = x;
        report
    }

    /// Degrees of freedom left: parameters minus the numerical rank of the
    /// Jacobian at the current configuration.
    pub fn dof(&self) -> usize {
        self.stats().1
    }

    /// The full report: DOF, which entities are still free, which constraints
    /// are redundant and which conflict, each by name.
    ///
    /// Call it after [`System::solve`]: redundancy is a property of the
    /// Jacobian anywhere, but telling a *consistent* redundancy from a
    /// *contradiction* needs the residuals at a solution.
    pub fn diagnose(&self) -> Diagnosis {
        let size = self.characteristic_size();
        let cons = self.active_records();
        let (j, weighted, raw, owner) = self.assemble(&self.x, size);
        let n = self.ents.nparams;

        // One rank-revealing orthogonalisation of the rows answers all three
        // questions: the rank (and so the degrees of freedom), which unit
        // directions the constraints do not reach (and so which entities are
        // still free), and which rows are combinations of earlier ones (and so
        // which constraints are redundant or in conflict).
        let space = linalg::row_space(&j, self.options.rank_tol, true);
        let rank = space.rank();
        let dof = n.saturating_sub(rank);

        let (redundant, conflicting) = classify(&space, &weighted, &owner, &cons, 1e-6 * size);

        let weights = space.null_weights(n);
        let mut free_parameters = Vec::new();
        let mut free_entities: Vec<EntityId> = Vec::new();
        for (p, w) in weights.iter().enumerate() {
            if *w > 1e-6 {
                free_parameters.push(p);
                let e = self.ents.owner_of_param(p);
                if !free_entities.contains(&e) {
                    free_entities.push(e);
                }
            }
        }
        free_entities.sort();

        let report_tol = 1e-7 * size;
        let mut unsatisfied = Vec::new();
        let mut max_residual = 0.0f64;
        for (i, &o) in owner.iter().enumerate() {
            max_residual = max_residual.max(raw[i].abs());
            if weighted[i].abs() > report_tol {
                let c = cons[o];
                if !unsatisfied.iter().any(|(id, _, _)| *id == c.id) {
                    unsatisfied.push((c.id, c.label(), raw[i]));
                }
            }
        }

        Diagnosis {
            parameters: n,
            equations: weighted.len(),
            rank,
            dof,
            satisfied: norm_inf(&weighted) <= self.options.residual_tol * size,
            max_residual,
            redundant,
            conflicting,
            free_entities,
            free_parameters,
            unsatisfied,
        }
    }

    /// Move `p` towards `to` and re-solve, keeping every constraint.
    ///
    /// Two passes, which is what makes a drag feel right rather than merely
    /// terminate:
    ///
    /// 1. The point is pinned at the cursor by a temporary constraint and the
    ///    whole system is solved. The pin carries a fifth of the weight of a
    ///    real constraint (SolveSpace's ratio), so wherever
    ///    the cursor and the sketch disagree the sketch wins; along directions
    ///    the sketch leaves free — which is most of them, during a drag — the
    ///    pin is the only thing acting and the point tracks the cursor exactly.
    /// 2. The pin is dropped and the sketch is solved again from there. Because
    ///    the Gauss–Newton step is the minimum-norm one, this pass is a
    ///    projection back onto the constraint manifold along the shortest path:
    ///    it restores the constraints exactly while barely moving anything,
    ///    which is also why a point constrained onto a circle follows the
    ///    cursor *around* the circle instead of being torn off it.
    ///
    /// The sketch the caller keeps is unchanged apart from its positions: the
    /// pin never survives the call, and the returned [`DragResult::solve`]
    /// describes the sketch's own constraints, not the pin.
    /// [`DragResult::reached`] says whether the point got to the cursor.
    pub fn drag(&mut self, p: PointId, to: (f64, f64)) -> DragResult {
        const PIN_WEIGHT: f64 = 0.05;
        let before = self.x.clone();
        self.set_point(p, to.0, to.1);
        let id = ConstraintId(self.cons.len() as u32);
        self.cons.push(ConstraintRecord {
            id,
            kind: Constraint::point_at(p, to.0, to.1),
            name: Some("drag".to_string()),
            fixed: Vec::new(),
            tangent_internal: false,
            rows: 2,
            weight: PIN_WEIGHT,
        });
        self.active.push(true);
        let pinned = self.resolve();
        self.cons.pop();
        self.active.pop();
        if !matches!(pinned.status, SolveStatus::Converged | SolveStatus::Inconsistent) {
            // A drag that went nowhere must not leave the sketch broken.
            self.x = before;
        }
        let solve = self.solve();
        let pos = self.point_pos(p);
        let miss = (((pos.0 - to.0) * (pos.0 - to.0)) + ((pos.1 - to.1) * (pos.1 - to.1))).sqrt();
        let reached = miss <= 1e-6 * self.characteristic_size();
        DragResult { solve, reached, miss }
    }

    // ---- internals ----------------------------------------------------

    fn active_records(&self) -> Vec<&ConstraintRecord> {
        self.cons
            .iter()
            .zip(&self.active)
            .filter(|(_, a)| **a)
            .map(|(c, _)| c)
            .collect()
    }

    fn row_total(&self) -> usize {
        self.cons
            .iter()
            .zip(&self.active)
            .filter(|(_, a)| **a)
            .map(|(c, _)| c.rows)
            .sum()
    }

    /// The sketch's characteristic length: what "one unit of badness" means
    /// for an angle. Taken from the current geometry so the weighting adapts
    /// to a 2 mm sketch and a 2 m one alike, and held fixed for the duration
    /// of a solve so the objective does not move under the solver's feet.
    fn characteristic_size(&self) -> f64 {
        let (mut lo, mut hi) = ((f64::MAX, f64::MAX), (f64::MIN, f64::MIN));
        let mut any = false;
        let mut max_r = 0.0f64;
        for rec in &self.ents.recs {
            match rec.kind {
                EntityKind::Point => {
                    let (x, y) = (self.x[rec.base], self.x[rec.base + 1]);
                    lo = (lo.0.min(x), lo.1.min(y));
                    hi = (hi.0.max(x), hi.1.max(y));
                    any = true;
                }
                EntityKind::Circle { .. } | EntityKind::Arc { .. } => {
                    max_r = max_r.max(self.x[rec.base].abs());
                }
                EntityKind::Line { .. } => {}
            }
        }
        let diag = if any {
            (((hi.0 - lo.0) * (hi.0 - lo.0)) + ((hi.1 - lo.1) * (hi.1 - lo.1))).sqrt()
        } else {
            0.0
        };
        diag.max(2.0 * max_r).max(1.0)
    }

    fn assemble(&self, x: &[f64], size: f64) -> (Mat, Vec<f64>, Vec<f64>, Vec<usize>) {
        let cons = self.active_records();
        let m = self.row_total();
        let mut j = Mat::zeros(m, self.ents.nparams);
        let mut weighted = vec![0.0; m];
        let mut raw = vec![0.0; m];
        let mut owner = vec![0usize; m];
        fill(&self.ents, &cons, x, size, Some(&mut j), &mut weighted, Some((&mut raw, &mut owner)));
        (j, weighted, raw, owner)
    }

    /// `(worst residual in sketch units, degrees of freedom)`.
    fn stats(&self) -> (f64, usize) {
        let size = self.characteristic_size();
        let (j, _weighted, raw, _owner) = self.assemble(&self.x, size);
        let rank = linalg::rank(&j, self.options.rank_tol);
        (norm_inf(&raw), self.ents.nparams.saturating_sub(rank))
    }
}

/// Evaluate residuals (always), the Jacobian (when asked) and the row
/// bookkeeping (when asked) at `x`, with rows weighted onto a common scale.
///
/// A length residual counts as itself; an angle or a sine counts as itself
/// times the sketch size, so that one radian of angular error weighs the same
/// as one sketch-width of positional error and the two converge together.
fn fill(
    ents: &Entities,
    cons: &[&ConstraintRecord],
    x: &[f64],
    size: f64,
    mut j: Option<&mut Mat>,
    weighted: &mut Vec<f64>,
    mut extra: Option<(&mut Vec<f64>, &mut Vec<usize>)>,
) {
    let m: usize = cons.iter().map(|c| c.rows).sum();
    weighted.clear();
    weighted.resize(m, 0.0);
    if let Some((raw, owner)) = extra.as_mut() {
        raw.clear();
        raw.resize(m, 0.0);
        owner.clear();
        owner.resize(m, 0);
    }
    if let Some(j) = j.as_deref_mut() {
        if j.rows() != m || j.cols() != x.len() {
            *j = Mat::zeros(m, x.len());
        } else {
            j.fill_zero();
        }
    }
    let mut row = 0usize;
    for (ci, c) in cons.iter().enumerate() {
        for rr in rows_of(ents, c, x) {
            let w = c.weight
                * match rr.unit {
                    Unit::Length => 1.0,
                    Unit::Angle | Unit::Ratio => size,
                };
            weighted[row] = rr.residual * w;
            if let Some((raw, owner)) = extra.as_mut() {
                raw[row] = rr.residual;
                owner[row] = ci;
            }
            if let Some(j) = j.as_deref_mut() {
                for (idx, coeff) in rr.terms {
                    j.add_at(row, idx, coeff * w);
                }
            }
            row += 1;
        }
    }
}
