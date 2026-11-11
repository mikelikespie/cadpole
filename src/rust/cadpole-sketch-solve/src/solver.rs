//! Gauss–Newton with Levenberg–Marquardt damping inside a Powell dogleg
//! trust region.
//!
//! Why this combination. A sketch is a small, dense, usually *square-ish*
//! nonlinear least-squares problem that is often rank-deficient (an
//! under-constrained sketch) and occasionally inconsistent (an
//! over-constrained one). Plain Newton diverges on a bad initial guess and
//! has nothing to say when the Jacobian loses rank. The trust region fixes
//! the first: every step is bounded by a radius that grows when the linear
//! model predicts well and shrinks when it does not, so a guess that starts
//! far away walks in rather than overshooting. The dogleg construction gets
//! most of Gauss–Newton's quadratic convergence near the solution while
//! degrading to steepest descent far from it, at the cost of one extra
//! matrix–vector product. Levenberg–Marquardt damping fixes the second: the
//! Gauss–Newton step comes from `(JᵀJ + λI) h = −Jᵀr`, and with `λ` at its
//! floor — a thousandth of a part per billion of `‖JᵀJ‖` — that is the
//! *minimum-norm* step, because a direction the Jacobian cannot see is damped
//! to nothing instead of blowing up. A sketch with three degrees of freedom
//! left therefore moves as little as it can rather than wandering along its
//! null space, which is exactly the behaviour dragging needs. `λ` is raised
//! only when a step is rejected or the factorisation refuses, so it costs
//! nothing on the common path.
//!
//! Scaling. Residuals are weighted before they reach this file: a length
//! residual counts as itself, an angle residual is multiplied by the sketch's
//! characteristic size, so one radian of angular error and one sketch-width of
//! positional error are treated as equally bad. Without it a solver in
//! millimetres would converge the angles to the precision of the lengths
//! divided by the sketch size — visibly wrong on a 200 mm part.
//!
//! Determinism. No random restarts, no randomised preconditioning, no
//! iteration order that depends on hashing. The same sketch and the same
//! starting values always produce the same answer.

use crate::linalg::{Mat, cholesky_solve, dot, norm2, norm_inf, normal_matrix};

/// Smallest Levenberg–Marquardt damping, as a fraction of the largest diagonal
/// of `JᵀJ`. Small enough that a well-conditioned step is untouched, large
/// enough that the Cholesky of a rank-deficient `JᵀJ` still succeeds — and it
/// is what makes the step the *minimum-norm* one, since a direction the
/// Jacobian cannot see is damped to nothing rather than blowing up.
const MIN_DAMPING: f64 = 1e-12;

/// Numerical knobs. The defaults suit a sketch measured in millimetres.
#[derive(Clone, Debug)]
pub struct SolveOptions {
    /// Give up after this many accepted-or-rejected iterations.
    pub max_iterations: usize,
    /// Converged when every weighted residual is below `residual_tol × size`,
    /// where `size` is the sketch's characteristic length. The default is a
    /// thousand times tighter than anything a machine tool can hold, and it is
    /// cheap: the damped Newton step contracts the error by a factor of about
    /// `1 / MIN_DAMPING` each iteration, so tightening it costs one iteration,
    /// not many.
    pub residual_tol: f64,
    /// A least-squares stationary point: the largest cosine between the
    /// residual and any column of the Jacobian falls below this. Being
    /// dimensionless, it means the same thing on a 2 mm sketch and a 2 m one;
    /// a plain `‖Jᵀr‖` threshold does not (MINPACK's `gtol`).
    pub gradient_tol: f64,
    /// Give up when the step falls below this fraction of `‖x‖`.
    pub step_tol: f64,
    /// A constraint row whose component orthogonal to the rows before it is
    /// smaller than `rank_tol` times its own length adds no rank. This single
    /// number decides the degrees of freedom and which constraints are
    /// reported as redundant, so it is deliberately loose enough to survive
    /// double-precision round-off on a well-scaled sketch and tight enough not
    /// to hide a genuine near-dependency.
    pub rank_tol: f64,
}

impl Default for SolveOptions {
    fn default() -> SolveOptions {
        SolveOptions {
            max_iterations: 200,
            residual_tol: 1e-12,
            gradient_tol: 1e-11,
            step_tol: 1e-14,
            rank_tol: 1e-9,
        }
    }
}

/// Why the solver stopped.
#[derive(Clone, Copy, PartialEq, Eq, Debug)]
pub enum SolveStatus {
    /// Every constraint is satisfied to tolerance. The sketch may still have
    /// degrees of freedom left — ask [`crate::System::dof`].
    Converged,
    /// A least-squares stationary point whose residual is still too large: the
    /// constraints contradict each other. [`crate::System::diagnose`] names
    /// the conflicting set.
    Inconsistent,
    /// The iteration cap was reached while still making progress. Raise
    /// [`SolveOptions::max_iterations`] or start from a better guess.
    MaxIterations,
    /// The trust region collapsed: steps became too small to matter before the
    /// residual came down. Usually a badly scaled or nearly degenerate sketch
    /// (a zero-length line inside an angle constraint, say).
    Stalled,
}

impl SolveStatus {
    pub fn is_converged(self) -> bool {
        self == SolveStatus::Converged
    }
}

pub(crate) struct Report {
    pub status: SolveStatus,
    pub iterations: usize,
}

/// Minimise `½‖r(x)‖²`.
///
/// `full` evaluates weighted residuals and the weighted Jacobian; `only_r`
/// evaluates weighted residuals alone, for trial points that may be rejected.
pub(crate) fn minimize(
    x: &mut [f64],
    m: usize,
    opts: &SolveOptions,
    size: f64,
    full: &mut dyn FnMut(&[f64], &mut Vec<f64>, &mut Mat),
    only_r: &mut dyn FnMut(&[f64], &mut Vec<f64>),
) -> Report {
    let n = x.len();
    if m == 0 || n == 0 {
        return Report { status: SolveStatus::Converged, iterations: 0 };
    }
    let tol = opts.residual_tol * size;

    let mut r = vec![0.0; m];
    let mut j = Mat::zeros(m, n);
    full(x, &mut r, &mut j);
    let mut f = 0.5 * dot(&r, &r);
    let mut jtj = normal_matrix(&j);
    let mut dmax = max_diagonal(&jtj);

    let mut trust = size.max(1.0);
    // Damping as a fraction of the largest diagonal of JᵀJ, so it is scale
    // free. Raised when a step is rejected, lowered when one works well.
    let mut lam_rel = MIN_DAMPING;
    let mut trial = vec![0.0; m];
    let mut status = SolveStatus::MaxIterations;
    let mut iterations = 0;

    for _ in 0..opts.max_iterations {
        if norm_inf(&r) <= tol {
            status = SolveStatus::Converged;
            break;
        }
        iterations += 1;

        let g = j.mul_transpose_vec(&r);
        if is_stationary(&jtj, &r, &g, opts.gradient_tol) {
            // The residual is orthogonal to everything the Jacobian can reach,
            // and it is still above tolerance: this is the least-squares
            // minimum of a system whose constraints contradict each other.
            status = SolveStatus::Inconsistent;
            break;
        }

        // Cauchy point: the steepest-descent minimiser of the linear model.
        let jg = j.mul_vec(&g);
        let jg2 = dot(&jg, &jg);
        let alpha = if jg2 > 0.0 { dot(&g, &g) / jg2 } else { 0.0 };
        let h_cp: Vec<f64> = g.iter().map(|v| -alpha * v).collect();

        // Gauss–Newton step from the damped normal equations: minimum-norm,
        // because a direction with no curvature is damped away rather than
        // amplified. Raise the damping if the factorisation refuses.
        let neg_g: Vec<f64> = g.iter().map(|v| -v).collect();
        let mut h_gn = None;
        for _ in 0..6 {
            let mut a = jtj.clone();
            let lambda = lam_rel * dmax;
            for i in 0..n {
                a.add_at(i, i, lambda);
            }
            match cholesky_solve(&mut a, &neg_g) {
                Some(h) => {
                    h_gn = Some(h);
                    break;
                }
                None => lam_rel *= 100.0,
            }
        }
        let h_gn = h_gn.unwrap_or_else(|| h_cp.clone());

        let h = dogleg(&h_cp, &h_gn, trust);
        let hnorm = norm2(&h);
        if hnorm <= opts.step_tol * (norm2(x) + opts.step_tol) {
            status = SolveStatus::Stalled;
            break;
        }

        let xn: Vec<f64> = x.iter().zip(&h).map(|(a, b)| a + b).collect();
        only_r(&xn, &mut trial);
        let f_new = 0.5 * dot(&trial, &trial);

        // Reduction the linear model predicted, ½‖r‖² − ½‖r + Jh‖².
        let jh = j.mul_vec(&h);
        let lin: Vec<f64> = r.iter().zip(&jh).map(|(a, b)| a + b).collect();
        let pred = f - 0.5 * dot(&lin, &lin);
        let rho = if pred > 0.0 { (f - f_new) / pred } else { -1.0 };

        if rho > 0.0 {
            x.copy_from_slice(&xn);
            full(x, &mut r, &mut j);
            f = 0.5 * dot(&r, &r);
            jtj = normal_matrix(&j);
            dmax = max_diagonal(&jtj);
        }

        if rho > 0.75 {
            trust = trust.max(3.0 * hnorm);
            lam_rel = (lam_rel * 0.1).max(MIN_DAMPING);
        } else if rho < 0.25 {
            trust *= 0.5;
            lam_rel = (lam_rel * 10.0).max(MIN_DAMPING);
        }
    }
    if norm_inf(&r) <= tol {
        status = SolveStatus::Converged;
    } else if status == SolveStatus::Stalled {
        // A trust region that collapsed on a residual that cannot come down is
        // an inconsistent system by another route.
        let g = j.mul_transpose_vec(&r);
        if is_stationary(&jtj, &r, &g, opts.gradient_tol.max(1e-8)) {
            status = SolveStatus::Inconsistent;
        }
    }
    Report { status, iterations }
}

fn max_diagonal(a: &Mat) -> f64 {
    let mut m = 0.0f64;
    for i in 0..a.rows() {
        m = m.max(a.at(i, i));
    }
    if m > 0.0 { m } else { 1.0 }
}

/// Is `r` orthogonal to the range of `J`, to within `tol`?
///
/// MINPACK's scale-free test: the largest cosine between the residual and any
/// column of the Jacobian. At a least-squares minimum it is zero; anywhere
/// else there is a direction that still reduces the residual. The column norms
/// come free from the diagonal of `JᵀJ`.
fn is_stationary(jtj: &Mat, r: &[f64], g: &[f64], tol: f64) -> bool {
    let rn = norm2(r);
    if rn == 0.0 {
        return true;
    }
    let mut worst: f64 = 0.0;
    for (c, gc) in g.iter().enumerate() {
        let col2 = jtj.at(c, c);
        if col2 <= 0.0 {
            continue;
        }
        worst = worst.max(gc.abs() / (col2.sqrt() * rn));
    }
    worst <= tol
}

/// Powell's dogleg: the Gauss–Newton step when it fits inside the trust
/// radius, the clipped Cauchy step when even that does not, and the point
/// where the segment between them crosses the trust-region boundary otherwise.
fn dogleg(h_cp: &[f64], h_gn: &[f64], trust: f64) -> Vec<f64> {
    let gn = norm2(h_gn);
    if gn <= trust {
        return h_gn.to_vec();
    }
    let cp = norm2(h_cp);
    if cp >= trust {
        if cp == 0.0 {
            return vec![0.0; h_cp.len()];
        }
        let k = trust / cp;
        return h_cp.iter().map(|v| v * k).collect();
    }
    // Solve ‖h_cp + β (h_gn − h_cp)‖ = trust for β ∈ [0, 1].
    let d: Vec<f64> = h_gn.iter().zip(h_cp).map(|(a, b)| a - b).collect();
    let a = dot(&d, &d);
    let b = 2.0 * dot(h_cp, &d);
    let c = cp * cp - trust * trust;
    let beta = if a > 0.0 {
        let disc = (b * b - 4.0 * a * c).max(0.0);
        ((-b + disc.sqrt()) / (2.0 * a)).clamp(0.0, 1.0)
    } else {
        0.0
    };
    h_cp.iter().zip(&d).map(|(p, q)| p + beta * q).collect()
}

#[cfg(test)]
mod tests {
    use super::*;

    /// Rosenbrock as a least-squares problem: r = (10(y − x²), 1 − x), whose
    /// only root is (1, 1). A classic test of whether a trust region survives
    /// a curved valley.
    #[test]
    fn dogleg_walks_the_rosenbrock_valley() {
        let mut x = vec![-1.2, 1.0];
        let opts = SolveOptions::default();
        let rep = minimize(
            &mut x,
            2,
            &opts,
            1.0,
            &mut |x, r, j| {
                r.clear();
                r.push(10.0 * (x[1] - x[0] * x[0]));
                r.push(1.0 - x[0]);
                *j = Mat::zeros(2, 2);
                j.set(0, 0, -20.0 * x[0]);
                j.set(0, 1, 10.0);
                j.set(1, 0, -1.0);
                j.set(1, 1, 0.0);
            },
            &mut |x, r| {
                r.clear();
                r.push(10.0 * (x[1] - x[0] * x[0]));
                r.push(1.0 - x[0]);
            },
        );
        assert_eq!(rep.status, SolveStatus::Converged, "{rep:?}", rep = rep.status);
        assert!((x[0] - 1.0).abs() < 1e-8 && (x[1] - 1.0).abs() < 1e-8, "{x:?}");
    }

    /// x = 1 and x = 2 at once: a stationary point at 1.5 that is not a root.
    #[test]
    fn an_inconsistent_system_is_reported_not_looped_on() {
        let mut x = vec![0.0];
        let opts = SolveOptions::default();
        let rep = minimize(
            &mut x,
            2,
            &opts,
            1.0,
            &mut |x, r, j| {
                r.clear();
                r.push(x[0] - 1.0);
                r.push(x[0] - 2.0);
                *j = Mat::zeros(2, 1);
                j.set(0, 0, 1.0);
                j.set(1, 0, 1.0);
            },
            &mut |x, r| {
                r.clear();
                r.push(x[0] - 1.0);
                r.push(x[0] - 2.0);
            },
        );
        assert_eq!(rep.status, SolveStatus::Inconsistent);
        assert!((x[0] - 1.5).abs() < 1e-6, "{x:?}");
    }
}
