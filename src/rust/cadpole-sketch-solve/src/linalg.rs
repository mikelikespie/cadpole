//! Dense linear algebra for small systems.
//!
//! Sketches have tens of parameters, not thousands, so the whole of the
//! numerical machinery a constraint solver needs fits in one file: a row-major
//! matrix, a Cholesky solve of the damped normal equations (the step), and a
//! rank-revealing orthogonalisation of the Jacobian's rows (the diagnostics).
//! Writing it here keeps `cadpole-sketch-solve` free of third-party code, keeps
//! the wasm build trivial, and keeps the licence table unchanged.
//!
//! **Why not an SVD.** The obvious choice is an SVD of the
//! Jacobian for rank and degrees of freedom. An SVD is the gold standard for
//! a rank decision, but a one-sided Jacobi SVD of a 100 × 100 matrix costs
//! around 30 Mflop, which measured at ~12 ms — three of those on every pointer
//! move is not an interactive drag. Two cheaper factorisations do the same
//! work here:
//!
//! * [`row_space`] is a modified Gram–Schmidt with one reorthogonalisation —
//!   a QR factorisation of `Jᵀ` computed row by row. It gives the rank, the
//!   null space, *and*, because it keeps each basis vector as an explicit
//!   combination of the rows that built it, the coefficients that name which
//!   constraints imply a dependent one. An SVD gives no such attribution.
//! * [`cholesky_solve`] on `JᵀJ + λI` gives the Levenberg–Marquardt step. As
//!   `λ → 0` that *is* the minimum-norm Gauss–Newton step — components along
//!   the null space are damped to nothing — which is the property the solver
//!   needs and the reason an SVD looked necessary in the first place.
//!
//! Everything here is deterministic: fixed loop orders, no pivoting on values,
//! no randomness.

use core::fmt;

/// A dense row-major matrix of `f64`.
#[derive(Clone, PartialEq)]
pub struct Mat {
    rows: usize,
    cols: usize,
    data: Vec<f64>,
}

impl Mat {
    pub fn zeros(rows: usize, cols: usize) -> Mat {
        Mat { rows, cols, data: vec![0.0; rows * cols] }
    }

    #[inline]
    pub fn rows(&self) -> usize {
        self.rows
    }

    #[inline]
    pub fn cols(&self) -> usize {
        self.cols
    }

    #[inline]
    pub fn at(&self, r: usize, c: usize) -> f64 {
        self.data[r * self.cols + c]
    }

    #[inline]
    pub fn set(&mut self, r: usize, c: usize, v: f64) {
        self.data[r * self.cols + c] = v;
    }

    #[inline]
    pub fn add_at(&mut self, r: usize, c: usize, v: f64) {
        self.data[r * self.cols + c] += v;
    }

    #[inline]
    pub fn row(&self, r: usize) -> &[f64] {
        &self.data[r * self.cols..(r + 1) * self.cols]
    }

    #[inline]
    pub fn row_mut(&mut self, r: usize) -> &mut [f64] {
        let c = self.cols;
        &mut self.data[r * c..(r + 1) * c]
    }

    pub fn fill_zero(&mut self) {
        self.data.iter_mut().for_each(|v| *v = 0.0);
    }

    /// `self * x`.
    pub fn mul_vec(&self, x: &[f64]) -> Vec<f64> {
        debug_assert_eq!(x.len(), self.cols);
        let mut out = vec![0.0; self.rows];
        for (r, o) in out.iter_mut().enumerate() {
            let row = self.row(r);
            let mut acc = 0.0;
            for c in 0..self.cols {
                acc += row[c] * x[c];
            }
            *o = acc;
        }
        out
    }

    /// `selfᵀ * y`.
    pub fn mul_transpose_vec(&self, y: &[f64]) -> Vec<f64> {
        debug_assert_eq!(y.len(), self.rows);
        let mut out = vec![0.0; self.cols];
        for r in 0..self.rows {
            let yr = y[r];
            if yr == 0.0 {
                continue;
            }
            let row = self.row(r);
            for c in 0..self.cols {
                out[c] += row[c] * yr;
            }
        }
        out
    }
}

impl fmt::Debug for Mat {
    fn fmt(&self, f: &mut fmt::Formatter<'_>) -> fmt::Result {
        writeln!(f, "Mat {}x{}", self.rows, self.cols)?;
        for r in 0..self.rows {
            for c in 0..self.cols {
                write!(f, "{:10.4} ", self.at(r, c))?;
            }
            writeln!(f)?;
        }
        Ok(())
    }
}

/// Euclidean norm.
pub fn norm2(v: &[f64]) -> f64 {
    v.iter().map(|a| a * a).sum::<f64>().sqrt()
}

/// Largest absolute entry.
pub fn norm_inf(v: &[f64]) -> f64 {
    v.iter().fold(0.0f64, |m, a| m.max(a.abs()))
}

pub fn dot(a: &[f64], b: &[f64]) -> f64 {
    a.iter().zip(b).map(|(x, y)| x * y).sum()
}

/// `Jᵀ J`, the normal matrix.
///
/// A constraint row touches only the handful of parameters belonging to the
/// entities it names — never more than about a dozen out of a hundred — so the
/// outer product is taken over each row's non-zero columns rather than over
/// all of them. On a fifty-entity sketch that is the difference between half a
/// megaflop and a few thousand flops per iteration.
pub fn normal_matrix(j: &Mat) -> Mat {
    let n = j.cols();
    let mut a = Mat::zeros(n, n);
    let mut nz: Vec<usize> = Vec::with_capacity(16);
    for r in 0..j.rows() {
        let row = j.row(r);
        nz.clear();
        for (c, v) in row.iter().enumerate() {
            if *v != 0.0 {
                nz.push(c);
            }
        }
        for (i, &p) in nz.iter().enumerate() {
            let vp = row[p];
            for &q in &nz[i..] {
                a.add_at(p, q, vp * row[q]);
            }
        }
    }
    for p in 0..n {
        for q in (p + 1)..n {
            let v = a.at(p, q);
            a.set(q, p, v);
        }
    }
    a
}

/// Solve `A x = b` for symmetric positive-definite `A` by Cholesky.
///
/// `a` is overwritten with its factor. Returns `None` when a pivot is not
/// positive, which means `A` was not positive definite — the caller's cue to
/// raise the damping and try again.
pub fn cholesky_solve(a: &mut Mat, b: &[f64]) -> Option<Vec<f64>> {
    let n = a.rows();
    debug_assert_eq!(n, a.cols());
    debug_assert_eq!(n, b.len());
    // L Lᵀ = A, stored in the lower triangle.
    for i in 0..n {
        for jx in 0..=i {
            let mut sum = a.at(i, jx);
            for k in 0..jx {
                sum -= a.at(i, k) * a.at(jx, k);
            }
            if i == jx {
                if !(sum > 0.0) {
                    return None;
                }
                a.set(i, i, sum.sqrt());
            } else {
                let d = a.at(jx, jx);
                a.set(i, jx, sum / d);
            }
        }
    }
    // Forward then back substitution.
    let mut y = vec![0.0; n];
    for i in 0..n {
        let mut sum = b[i];
        for k in 0..i {
            sum -= a.at(i, k) * y[k];
        }
        y[i] = sum / a.at(i, i);
    }
    let mut x = vec![0.0; n];
    for i in (0..n).rev() {
        let mut sum = y[i];
        for k in (i + 1)..n {
            sum -= a.at(k, i) * x[k];
        }
        x[i] = sum / a.at(i, i);
    }
    Some(x)
}

/// An orthonormal basis of a matrix's row space, with every basis vector kept
/// as an explicit combination of the rows that produced it.
pub struct RowSpace {
    /// Orthonormal, one per row that raised the rank.
    pub basis: Vec<Vec<f64>>,
    /// Indices of those rows, in the order they were seen.
    pub accepted: Vec<usize>,
    /// Rows that raised the rank of nothing, each with the coefficients that
    /// reproduce it over `accepted` — empty unless `with_coefficients`.
    pub dependent: Vec<(usize, Vec<f64>)>,
}

impl RowSpace {
    pub fn rank(&self) -> usize {
        self.accepted.len()
    }

    /// For each of the `n` columns, how much of that unit axis lies *outside*
    /// the row space: `‖(I − QᵀQ) eⱼ‖`. Zero means the parameter is pinned;
    /// anything larger means it can still move, and by that much.
    pub fn null_weights(&self, n: usize) -> Vec<f64> {
        let mut w = vec![1.0; n];
        for q in &self.basis {
            for (j, wj) in w.iter_mut().enumerate() {
                *wj -= q[j] * q[j];
            }
        }
        w.iter().map(|v| v.max(0.0).sqrt()).collect()
    }
}

/// Rank-revealing orthogonalisation of a matrix's rows, in order.
///
/// Modified Gram–Schmidt with one reorthogonalisation pass ("twice is enough",
/// Kahan), which is a QR factorisation of `Jᵀ` built a row at a time. A row
/// whose component orthogonal to everything before it is smaller than `tol`
/// times its own norm adds no rank; that test is relative to the row, so it
/// means the same thing for a row of unit coefficients and a row scaled by the
/// sketch size.
///
/// Taking the rows *in order* is deliberate: when two constraints are
/// dependent, the one added later is the one reported, which is the one the
/// user is looking at.
pub fn row_space(j: &Mat, tol: f64, with_coefficients: bool) -> RowSpace {
    let m = j.rows();
    let mut basis: Vec<Vec<f64>> = Vec::new();
    // `coeffs[t][k]`: basis[t] = Σ coeffs[t][k] · rows[accepted[k]].
    let mut coeffs: Vec<Vec<f64>> = Vec::new();
    let mut accepted: Vec<usize> = Vec::new();
    let mut dependent: Vec<(usize, Vec<f64>)> = Vec::new();

    for i in 0..m {
        let row = j.row(i);
        let n0 = norm2(row);
        let mut w = row.to_vec();
        let mut c = vec![0.0; basis.len()];
        for _ in 0..2 {
            for (t, q) in basis.iter().enumerate() {
                let d = dot(&w, q);
                if d == 0.0 {
                    continue;
                }
                c[t] += d;
                for (k, wk) in w.iter_mut().enumerate() {
                    *wk -= d * q[k];
                }
            }
        }
        let n1 = norm2(&w);
        if n0 > 0.0 && n1 > tol * n0 {
            let inv = 1.0 / n1;
            if with_coefficients {
                let k = accepted.len();
                let mut lnew = vec![0.0; k + 1];
                for (t, lt) in coeffs.iter().enumerate() {
                    if c[t] == 0.0 {
                        continue;
                    }
                    for (jx, &ltj) in lt.iter().enumerate() {
                        lnew[jx] -= c[t] * ltj;
                    }
                }
                for v in lnew.iter_mut() {
                    *v *= inv;
                }
                lnew[k] += inv;
                coeffs.push(lnew);
            }
            for v in w.iter_mut() {
                *v *= inv;
            }
            basis.push(w);
            accepted.push(i);
        } else if with_coefficients {
            let mut y = vec![0.0; accepted.len()];
            for (t, lt) in coeffs.iter().enumerate() {
                if c[t] == 0.0 {
                    continue;
                }
                for (jx, &ltj) in lt.iter().enumerate() {
                    y[jx] += c[t] * ltj;
                }
            }
            dependent.push((i, y));
        }
    }
    RowSpace { basis, accepted, dependent }
}

/// Just the rank, skipping the bookkeeping the attribution needs.
pub fn rank(j: &Mat, tol: f64) -> usize {
    row_space(j, tol, false).rank()
}

#[cfg(test)]
mod tests {
    use super::*;

    fn mat(rows: usize, cols: usize, vals: &[f64]) -> Mat {
        let mut m = Mat::zeros(rows, cols);
        for (i, v) in vals.iter().enumerate() {
            m.set(i / cols, i % cols, *v);
        }
        m
    }

    #[test]
    fn cholesky_solves_a_small_spd_system() {
        // A = [[4,1],[1,3]], b = [1,2] → x = [1/11, 7/11].
        let mut a = mat(2, 2, &[4.0, 1.0, 1.0, 3.0]);
        let x = cholesky_solve(&mut a, &[1.0, 2.0]).unwrap();
        assert!((x[0] - 1.0 / 11.0).abs() < 1e-12, "{x:?}");
        assert!((x[1] - 7.0 / 11.0).abs() < 1e-12, "{x:?}");
    }

    #[test]
    fn cholesky_refuses_an_indefinite_matrix() {
        let mut a = mat(2, 2, &[0.0, 1.0, 1.0, 0.0]);
        assert!(cholesky_solve(&mut a, &[1.0, 1.0]).is_none());
    }

    #[test]
    fn the_normal_matrix_matches_the_dense_product() {
        let j = mat(3, 4, &[1.0, 0.0, 2.0, 0.0, 0.0, 3.0, 0.0, 1.0, 5.0, 0.0, 0.0, -2.0]);
        let a = normal_matrix(&j);
        for p in 0..4 {
            for q in 0..4 {
                let mut want = 0.0;
                for r in 0..3 {
                    want += j.at(r, p) * j.at(r, q);
                }
                assert!((a.at(p, q) - want).abs() < 1e-12, "{p},{q}");
            }
        }
    }

    #[test]
    fn a_damped_normal_solve_is_the_minimum_norm_step() {
        // x + y = 2 has a one-dimensional solution set; the minimum-norm
        // answer is (1, 1), and tiny damping picks it out.
        let j = mat(1, 2, &[1.0, 1.0]);
        let mut a = normal_matrix(&j);
        let dmax = a.at(0, 0).max(a.at(1, 1));
        for i in 0..2 {
            a.add_at(i, i, 1e-12 * dmax);
        }
        let g = j.mul_transpose_vec(&[2.0]);
        let x = cholesky_solve(&mut a, &g).unwrap();
        assert!((x[0] - 1.0).abs() < 1e-9, "{x:?}");
        assert!((x[1] - 1.0).abs() < 1e-9, "{x:?}");
    }

    #[test]
    fn row_space_finds_the_rank_of_a_dependent_matrix() {
        // Third row is the sum of the first two: rank 2.
        let j = mat(3, 3, &[1.0, 2.0, 3.0, 4.0, 5.0, 6.0, 5.0, 7.0, 9.0]);
        let rs = row_space(&j, 1e-9, true);
        assert_eq!(rs.rank(), 2);
        assert_eq!(rs.accepted, vec![0, 1]);
        assert_eq!(rs.dependent.len(), 1);
        let (row, y) = &rs.dependent[0];
        assert_eq!(*row, 2);
        assert!((y[0] - 1.0).abs() < 1e-9 && (y[1] - 1.0).abs() < 1e-9, "{y:?}");
    }

    #[test]
    fn null_weights_find_the_free_column() {
        // One equation in three unknowns pins one direction and leaves two.
        let j = mat(1, 3, &[0.0, 0.0, 2.0]);
        let rs = row_space(&j, 1e-9, false);
        let w = rs.null_weights(3);
        assert!((w[0] - 1.0).abs() < 1e-12, "{w:?}");
        assert!((w[1] - 1.0).abs() < 1e-12, "{w:?}");
        assert!(w[2] < 1e-12, "{w:?}");
    }
}
