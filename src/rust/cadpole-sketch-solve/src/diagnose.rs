//! Diagnostics: degrees of freedom, redundancy and conflict, by name.
//!
//! "Over-constrained" on its own is a useless message. What a sketch UI needs
//! is *which* constraint is the extra one and *which* constraints already
//! implied it, and the difference between an extra constraint that happens to
//! agree with the others (redundant — annoying, deletable) and one that
//! disagrees (conflicting — the sketch has no solution).
//!
//! Both fall out of the same linear algebra, done at the solution:
//!
//! * **Degrees of freedom** = parameters − rank(J), the rank being the
//!   *numerical* rank of the weighted Jacobian at the solved configuration.
//!   This is the witness-method answer (Michelucci & Foufou 2007): a numerical
//!   rank at a generic configuration finds dependences that a graph-based
//!   degree count misses, including the theorem-induced ones — the third angle
//!   of a triangle, the fourth side of a rectangle.
//!
//! * **Dependence** comes from the same factorisation, [`crate::linalg::row_space`]:
//!   the Jacobian's rows are orthogonalised in the order the user added the
//!   constraints, and a row whose component orthogonal to everything before it
//!   is negligible adds nothing to the rank — it is implied by earlier rows.
//!   Because each basis vector is kept as an explicit combination of the rows
//!   that built it, the coefficients `y` that reproduce the dependent row come
//!   out of the same bookkeeping, and their support *names the constraints
//!   responsible*. Taking the rows in insertion order means the blame lands on
//!   the constraint the user added last, which is the one they are looking at.
//!
//! * **Redundant or conflicting** is then one dot product. The dependence is a
//!   vector `z` with `zᵀJ ≈ 0`; the constraints it involves can only be
//!   satisfied together if `zᵀr ≈ 0` as well. At a least-squares stationary
//!   point the residual is orthogonal to the range of `J`, so any leftover
//!   error lives entirely in these dependence directions: `zᵀr` is exactly the
//!   amount by which the group contradicts itself, in millimetres. Above
//!   tolerance it is a conflict, below it a harmless redundancy.

use crate::constraint::{ConstraintId, ConstraintRecord};
use crate::entity::EntityId;
use crate::linalg::RowSpace;
use core::fmt;

/// One constraint that adds no rank, and the constraints that already imply
/// it.
#[derive(Clone, Debug, PartialEq)]
pub struct Dependency {
    /// The dependent constraint — the one to delete or to change.
    pub constraint: ConstraintId,
    /// Its name, or `kind#id` when it has none.
    pub label: String,
    /// The constraints whose combination reproduces it.
    pub implied_by: Vec<ConstraintId>,
    /// The same, by name, for a message a user can read.
    pub implied_by_labels: Vec<String>,
    /// How far the group is from being mutually satisfiable, in sketch units.
    /// Zero for a redundancy; the size of the contradiction for a conflict.
    pub gap: f64,
}

impl fmt::Display for Dependency {
    fn fmt(&self, f: &mut fmt::Formatter<'_>) -> fmt::Result {
        write!(f, "{}", self.label)?;
        if !self.implied_by_labels.is_empty() {
            write!(f, " (with {})", self.implied_by_labels.join(", "))?;
        }
        if self.gap.abs() > 0.0 {
            write!(f, " off by {:.4}", self.gap)?;
        }
        Ok(())
    }
}

/// Everything the solver can say about a sketch.
#[derive(Clone, Debug, PartialEq)]
pub struct Diagnosis {
    /// Size of the parameter vector.
    pub parameters: usize,
    /// Number of scalar equations (constraints contribute one or more each).
    pub equations: usize,
    /// Numerical rank of the Jacobian: the number of *independent* equations.
    pub rank: usize,
    /// `parameters − rank`. Zero means fully constrained.
    pub dof: usize,
    /// Every constraint is met to tolerance.
    pub satisfied: bool,
    /// The largest single constraint error, in sketch units (radians for a
    /// pure angle constraint).
    pub max_residual: f64,
    /// Constraints that add nothing but do not contradict anything.
    pub redundant: Vec<Dependency>,
    /// Constraints that cannot hold at the same time as the ones they depend
    /// on.
    pub conflicting: Vec<Dependency>,
    /// Entities that still have freedom left, in id order.
    pub free_entities: Vec<EntityId>,
    /// The parameter indices behind `free_entities`.
    pub free_parameters: Vec<usize>,
    /// Constraints whose own residual is above tolerance, with that residual.
    pub unsatisfied: Vec<(ConstraintId, String, f64)>,
}

impl Diagnosis {
    pub fn is_fully_constrained(&self) -> bool {
        self.dof == 0 && self.conflicting.is_empty()
    }
    pub fn is_under_constrained(&self) -> bool {
        self.dof > 0
    }
    /// Over-constrained means there is at least one dependent constraint,
    /// whether or not it contradicts the others.
    pub fn is_over_constrained(&self) -> bool {
        !self.redundant.is_empty() || !self.conflicting.is_empty()
    }
}

impl fmt::Display for Diagnosis {
    fn fmt(&self, f: &mut fmt::Formatter<'_>) -> fmt::Result {
        let mut lines: Vec<String> = Vec::new();
        if self.dof == 0 {
            lines.push("fully constrained".to_string());
        } else {
            let free: Vec<String> =
                self.free_entities.iter().map(|e| format!("{e}")).collect();
            lines.push(format!(
                "under-constrained: {} DoF (free: {})",
                self.dof,
                free.join(", ")
            ));
        }
        for d in &self.conflicting {
            lines.push(format!(
                "conflicting: {} contradicts {} by {:.6}",
                d.label,
                d.implied_by_labels.join(", "),
                d.gap.abs()
            ));
        }
        for d in &self.redundant {
            if d.implied_by_labels.is_empty() {
                // A row that is identically zero: degenerate geometry, such as
                // an angle between two zero-length lines.
                lines.push(format!("redundant: {} has no effect", d.label));
            } else {
                lines.push(format!(
                    "redundant: {} is already implied by {}",
                    d.label,
                    d.implied_by_labels.join(", ")
                ));
            }
        }
        write!(f, "{}", lines.join("\n"))
    }
}

/// Turn dependent rows into named redundancies and conflicts.
///
/// `row_owner[i]` is the index into `cons` of the constraint that produced row
/// `i`; `r` is the weighted residual vector at the current solution.
pub(crate) fn classify(
    space: &RowSpace,
    r: &[f64],
    row_owner: &[usize],
    cons: &[&ConstraintRecord],
    conflict_tol: f64,
) -> (Vec<Dependency>, Vec<Dependency>) {
    let mut redundant: Vec<Dependency> = Vec::new();
    let mut conflicting: Vec<Dependency> = Vec::new();

    for (row_index, y) in &space.dependent {
        let owner = cons[row_owner[*row_index]];
        let ymax = y.iter().fold(0.0f64, |m, v| m.max(v.abs()));
        let mut gap = r[*row_index];
        let mut l1 = 0.0;
        let mut implied: Vec<ConstraintId> = Vec::new();
        let mut implied_labels: Vec<String> = Vec::new();
        for (jx, &yy) in y.iter().enumerate() {
            let row = space.accepted[jx];
            gap -= yy * r[row];
            l1 += yy.abs();
            if ymax > 0.0 && yy.abs() > 1e-6 * ymax {
                let c = cons[row_owner[row]];
                if !implied.contains(&c.id) {
                    implied.push(c.id);
                    implied_labels.push(c.label());
                }
            }
        }
        let dep = Dependency {
            constraint: owner.id,
            label: owner.label(),
            implied_by: implied,
            implied_by_labels: implied_labels,
            gap,
        };
        if gap.abs() > conflict_tol * (1.0 + l1) {
            merge(&mut conflicting, dep);
        } else {
            merge(&mut redundant, Dependency { gap: 0.0, ..dep });
        }
    }
    (redundant, conflicting)
}

/// A constraint with several rows can be dependent more than once; report it
/// once, keeping the largest gap and the union of the causes.
fn merge(into: &mut Vec<Dependency>, dep: Dependency) {
    if let Some(e) = into.iter_mut().find(|e| e.constraint == dep.constraint) {
        if dep.gap.abs() > e.gap.abs() {
            e.gap = dep.gap;
        }
        for (id, label) in dep.implied_by.iter().zip(&dep.implied_by_labels) {
            if !e.implied_by.contains(id) {
                e.implied_by.push(*id);
                e.implied_by_labels.push(label.clone());
            }
        }
    } else {
        into.push(dep);
    }
}
