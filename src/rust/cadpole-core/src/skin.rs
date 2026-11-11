//! Global B-spline skinning: one surface through *N* section curves.
//!
//! `kernel::loft` builds a ruled band with one surface per pair of adjacent
//! sections, so a stack of twelve circles is twelve exact cone bands with
//! twelve tangent creases across the section planes. Skinning replaces that
//! stack with a **single** surface that passes through every section exactly
//! and is C² across the ones in the middle. It is Piegl & Tiller's §10.3
//! algorithm.
//!
//! The algorithm, in the order the code runs it:
//!
//! 1. **Make the sections compatible** ([`make_compatible`]). Elevate every
//!    section curve to the highest degree present, normalise every knot vector
//!    to `[0,1]`, and refine each one to the multiset union of all of them.
//!    All three steps are *exact* — the compatible curve is the same curve,
//!    point for point; only its representation grew.
//! 2. **One parameter per section** ([`VDirection::new`]). For each column of
//!    the control-point grid take the centripetal (α = ½) parameters of that
//!    column's polygon, then **average across the columns**. One vector, used
//!    by every column. Done per column instead,
//!    each column gets its own knot vector, only the last
//!    one survives into the surface, and the result stops interpolating the
//!    sections the moment they are not related by a uniform scale.
//! 3. **The v knot vector by averaging** ([`averaged_knots`]) — the choice that
//!    makes every knot span contain a section parameter (Schoenberg–Whitney),
//!    which is what makes step 4's matrix nonsingular.
//! 4. **One banded LU, factored once** ([`Banded`]). The interpolation matrix
//!    `A[k][l] = N_{l,q}(v_k)` does not depend on which column is being
//!    solved, so it is built and factored once for the whole loft and
//!    back-substituted per column. It is totally positive, so elimination
//!    without pivoting is stable (P&T Ch. 9).
//!
//! **Everything is done on homogeneous control points.** A circle is rational;
//! degree elevation and knot insertion are linear maps on the control polygon,
//! so they commute with homogenisation, and elevating the Euclidean points and
//! the weights separately would distort the arc. The solve is homogeneous too,
//! which is what preserves the exact-cone property the ruled loft has today:
//! sections with identical weights interpolate to a surface with those same
//! weights.
//!
//! ## Why `ruled=True` is this same code
//!
//! At `q = 1` the algorithm *is* the ruled homotopy, and not approximately:
//! there are no interior v-knots to average, so `V = [0,0,1,1]`; the degree-1
//! basis functions at `v_0 = 0` and `v_1 = 1` are the rows of the identity, so
//! the LU is the identity and [`Banded::solve`] returns its right-hand side
//! untouched (it skips the zero multiplications and the division by one, so
//! this holds bit for bit and not merely to rounding); and the surface control
//! points are therefore the section control points. That is exactly
//! `BSplineSurface::homotopy`'s output, over exactly the knot vector
//! `syncro_knots` would have produced — so a ruled loft keeps building the
//! solid it built before, down to the last bit, and `loft(ruled=False)` is the
//! same code path with `q = 3`.
//!
//! ## What this module does not do
//!
//! No end conditions (a loft "normal to the start section" would be two
//! more rows per column, the matrix still banded). No knot removal: the union can multiply the control-point
//! count, and [`Compatible::width`] is bounded by
//! `Σ_k (n_k + 1) − K·(p + 1)` in the worst case of wholly disjoint interior
//! knots. No approximate skinning, and no periodic closure in v.
//! A surface of revolution should be `revolve`, not a skinned
//! loft.

use truck_modeling::{BSplineCurve, BSplineSurface, KnotVec, Vector4};

use crate::{CadpoleError, Result};
use crate::fmath;

/// The exponent in the parameterisation of a control-point column:
/// `|ΔP|^α`. `1` is chord length, `0` uniform, and `½` is **centripetal**,
/// which is Lee 1989's answer to unevenly spaced data. Sections a script writes are unevenly spaced almost by
/// definition — a bell profile crowds them where it curves — so this is the
/// value that matters.
const ALPHA: f64 = 0.5;

/// The highest degree a skinned surface takes in the v (section) direction.
///
/// Three, not "the same as the sections". Degree 3 is C² across every section
/// plane, which is what a user means by a smooth loft, and it keeps the
/// v-direction band of the interpolation matrix seven wide. Taking
/// the v degree from the u degree would make a loft through circles —
/// rational *quadratics* — quadratic in v for no reason at all.
pub const MAX_V_DEGREE: usize = 3;

/// Two knots this close together are one knot.
///
/// Absolute, but applied only after every knot vector has been normalised to
/// `[0,1]`, so it is relative to the domain. Tight enough that no knot a
/// sketch asks for is swallowed (truck's own `syncro_knots` merges at
/// `1e-6`), loose enough that two knots that should be equal and differ by
/// rounding become one.
const KNOT_EPS: f64 = 1e-12;

/// A control-point column that moves less than this, summed over the whole
/// loft, has no parameterisation of its own and is given the uniform one.
///
/// Without the guard a column whose control point does not move between
/// sections — the axis-side corner of a section that is never scaled, say —
/// divides by zero and poisons the average for every other column.
const FLAT_COLUMN: f64 = 1e-10;

/// The smallest weight a solved homogeneous control point may have.
///
/// Interpolation does not preserve positivity: nothing stops the solve from
/// returning a control point with `w ≤ 0`, and the surface is then undefined
/// where that point has influence. Cadpole
/// checks, and says so with a diagnostic.
const MIN_WEIGHT: f64 = 1e-9;

/// The pivot below which the interpolation matrix is called singular.
const MIN_PIVOT: f64 = 1e-14;

/// *N* section curves in homogeneous form, all of one degree and sharing one
/// knot vector: what the interpolation actually needs, and what
/// [`make_compatible`] produces.
#[derive(Clone, Debug)]
pub struct Compatible {
    /// The knot vector every section now has — the surface's u knots.
    pub knots: KnotVec,
    /// `points[k][i]`: section *k*'s *i*-th homogeneous control point.
    pub points: Vec<Vec<Vector4>>,
}

impl Compatible {
    /// How many sections: `K + 1`.
    pub fn sections(&self) -> usize {
        self.points.len()
    }

    /// How many control points each section ended up with: `n + 1`.
    pub fn width(&self) -> usize {
        self.points[0].len()
    }

    /// The common degree.
    pub fn degree(&self) -> usize {
        self.knots.len() - self.width() - 1
    }
}

/// `knots` split into its distinct values and their multiplicities, merging at
/// [`KNOT_EPS`].
///
/// truck's own `KnotVec::to_single_multi` does this with its global `near`,
/// which is decades coarser; writing it here keeps the merge tolerance a
/// decision of this module rather than of whatever `TOLERANCE` happens to be.
fn single_multi(knots: &KnotVec) -> (Vec<f64>, Vec<usize>) {
    let mut values: Vec<f64> = Vec::new();
    let mut mults: Vec<usize> = Vec::new();
    for &k in knots.as_slice() {
        match values.last() {
            Some(&last) if (k - last).abs() <= KNOT_EPS => *mults.last_mut().unwrap() += 1,
            _ => {
                values.push(k);
                mults.push(1);
            }
        }
    }
    (values, mults)
}

/// The section curves, degree-elevated and knot-refined until they share a
/// degree and a knot vector — exactly, not approximately.
///
/// The curves come in as homogeneous `BSplineCurve<Vector4>` (truck's
/// `Curve::lift_up`), so a rational arc's weights ride along as a fourth
/// coordinate and go through elevation and refinement with the rest of the
/// control point, which is the only way to keep the arc an arc.
///
/// Order matters for one reason that is not obvious: degree elevation runs
/// **before** normalisation, which is the order truck's own
/// `syncro_degree`/`syncro_knots` pair uses inside `BSplineSurface::homotopy`.
/// The textbook order is normalisation first; on a curve already clamped to
/// `[0,1]` — which is every curve Cadpole builds — normalisation is the identity
/// and the two orders agree, but matching truck's order is what lets the
/// two-section case come out bit for bit identical to the homotopy it
/// replaces.
pub fn make_compatible(mut curves: Vec<BSplineCurve<Vector4>>) -> Result<Compatible> {
    if curves.len() < 2 {
        return Err(CadpoleError::invalid(
            "a skinned loft needs at least two section curves to interpolate",
        ));
    }
    for (k, c) in curves.iter().enumerate() {
        if c.degree() == 0 || c.control_points().is_empty() {
            return Err(CadpoleError::invalid(format!(
                "loft section {k} has a curve of degree {} with {} control points, which is not a \
                 curve the sections can be made compatible over",
                c.degree(),
                c.control_points().len()
            )));
        }
        if !(c.knot_vec().range_length() > 0.0) {
            return Err(CadpoleError::invalid(format!(
                "loft section {k} has a curve whose parameter range is empty, so the sections \
                 cannot be put on a common domain"
            )));
        }
        if !c.is_clamped() {
            return Err(CadpoleError::invalid(format!(
                "loft section {k} has an unclamped curve; a skinned loft interpolates sections \
                 that start and end where their control polygons do"
            )));
        }
    }

    // (b) one degree, by elevation. Exact: the curve does not move.
    let degree = curves.iter().map(|c| c.degree()).max().unwrap();
    for c in curves.iter_mut() {
        while c.degree() < degree {
            c.elevate_degree();
        }
    }

    // (a) one domain. A linear reparameterisation is not a change of shape.
    for c in curves.iter_mut() {
        c.knot_normalize();
    }

    // (c) one knot vector: every distinct knot value at the greatest
    // multiplicity any section gives it. The representative value of a merged
    // group is the one the *first* section used, so section 0's knot vector
    // survives untouched whenever it already had the knot — which is what
    // keeps the two-section case bit-identical to `syncro_knots`.
    let mut values: Vec<f64> = Vec::new();
    let mut mults: Vec<usize> = Vec::new();
    for c in &curves {
        let (vs, ms) = single_multi(c.knot_vec());
        for (v, m) in vs.iter().zip(&ms) {
            match values.iter().position(|x| (x - v).abs() <= KNOT_EPS) {
                Some(i) => mults[i] = mults[i].max(*m),
                None => {
                    values.push(*v);
                    mults.push(*m);
                }
            }
        }
    }
    let mut order: Vec<usize> = (0..values.len()).collect();
    order.sort_by(|&a, &b| values[a].total_cmp(&values[b]));
    let union: Vec<(f64, usize)> = order.iter().map(|&i| (values[i], mults[i])).collect();

    for c in curves.iter_mut() {
        let (vs, ms) = single_multi(c.knot_vec());
        // Ascending, so a refinement never re-visits a span it already split:
        // the same order `syncro_knots` inserts in.
        for &(v, want) in &union {
            let have =
                vs.iter().position(|x| (x - v).abs() <= KNOT_EPS).map(|i| ms[i]).unwrap_or(0);
            for _ in have..want {
                c.add_knot(v);
            }
        }
    }

    // Section 0's refined knot vector *is* the union, so use it rather than
    // rebuilding one: identical values, and no chance of a rounding difference
    // between the vector the control points were refined against and the one
    // the surface is built on.
    let knots = curves[0].knot_vec().clone();
    let width = curves[0].control_points().len();
    for (k, c) in curves.iter().enumerate() {
        if c.control_points().len() != width || c.knot_vec().len() != knots.len() {
            return Err(CadpoleError::invalid(format!(
                "loft section {k} could not be made compatible with section 0: it ended with {} \
                 control points over {} knots against section 0's {width} over {}. Sections whose \
                 segments are shaped differently enough for this cannot be skinned; splitting \
                 them to a common segment count (match=\"arclen\") is the way to pair them",
                c.control_points().len(),
                c.knot_vec().len(),
                knots.len()
            )));
        }
    }

    let points = curves.into_iter().map(|c| c.control_points().clone()).collect();
    Ok(Compatible { knots, points })
}

/// A square band matrix in LU form, factored without pivoting.
///
/// The interpolation matrix is `(K+1) × (K+1)` with at most `q + 1` nonzeros
/// per row, and because the section parameters increase monotonically the band
/// advances monotonically down the rows: nothing sits further than `q` off the
/// diagonal. It is also **totally positive**, which is the standard
/// justification (de Boor; P&T Ch. 9) for eliminating without pivoting —
/// growth is bounded, so the cheap factorisation is the stable one. No
/// pivoting also means no fill-in outside the band, which is why the whole
/// thing fits in `n × (2q + 1)` numbers.
#[derive(Clone, Debug)]
pub struct Banded {
    n: usize,
    q: usize,
    /// Row-major `n × (2q + 1)`; entry `(i, j)` lives at `i·(2q+1) + j − i + q`.
    a: Vec<f64>,
}

impl Banded {
    fn new(n: usize, q: usize) -> Banded {
        Banded { n, q, a: vec![0.0; n * (2 * q + 1)] }
    }

    fn slot(&self, i: usize, j: usize) -> Option<usize> {
        let d = j as isize - i as isize;
        let q = self.q as isize;
        (-q..=q).contains(&d).then(|| i * (2 * self.q + 1) + (d + q) as usize)
    }

    fn at(&self, i: usize, j: usize) -> f64 {
        self.slot(i, j).map(|s| self.a[s]).unwrap_or(0.0)
    }

    fn set(&mut self, i: usize, j: usize, v: f64) {
        if let Some(s) = self.slot(i, j) {
            self.a[s] = v;
        }
    }

    /// Eliminate, in place. Fails rather than returning a nonsense
    /// factorisation when a pivot vanishes — which, given the averaging knot
    /// vector, means the caller handed in section parameters that are not
    /// strictly increasing.
    fn factor(&mut self) -> Result<()> {
        for k in 0..self.n {
            let pivot = self.at(k, k);
            if pivot.abs() < MIN_PIVOT {
                return Err(CadpoleError::invalid(format!(
                    "the sections cannot be interpolated: the interpolation matrix is singular at \
                     section {k}. That means two sections share a parameter along the loft, which \
                     is what happens when two of them are the same curve"
                )));
            }
            let hi = self.n.min(k + self.q + 1);
            for i in (k + 1)..hi {
                let m = self.at(i, k) / pivot;
                self.set(i, k, m);
                if m == 0.0 {
                    continue;
                }
                for j in (k + 1)..hi {
                    let v = self.at(i, j) - m * self.at(k, j);
                    self.set(i, j, v);
                }
            }
        }
        Ok(())
    }

    /// Back-substitute one right-hand side. Four coordinates at a time, since
    /// a homogeneous control point travels together.
    ///
    /// Zero multipliers and unit pivots are skipped rather than applied. That
    /// is not an optimisation: it is what makes the identity matrix — the
    /// `q = 1` case, i.e. every ruled loft — return its input *bit for bit*
    /// rather than to within a rounding of zero.
    pub fn solve(&self, b: &[Vector4]) -> Vec<Vector4> {
        let mut x = b.to_vec();
        for i in 0..self.n {
            let lo = i.saturating_sub(self.q);
            for j in lo..i {
                let l = self.at(i, j);
                if l != 0.0 {
                    x[i] = x[i] - x[j] * l;
                }
            }
        }
        for i in (0..self.n).rev() {
            let hi = self.n.min(i + self.q + 1);
            for j in (i + 1)..hi {
                let u = self.at(i, j);
                if u != 0.0 {
                    x[i] = x[i] - x[j] * u;
                }
            }
            let d = self.at(i, i);
            if d != 1.0 {
                x[i] = x[i] / d;
            }
        }
        x
    }
}

/// The v (section) direction of one loft: where each section sits, the knot
/// vector over those parameters, and the interpolation matrix, factored once.
///
/// One of these serves a whole loft — every loop, every segment. It has to:
/// two side faces that meet along a spoke edge each own one boundary curve of
/// that edge, and those two curves are the same curve only because they
/// interpolate the same section corners through the same knot vector. A
/// v-direction computed per face would leave a hairline mismatch at every
/// segment boundary of every section.
#[derive(Clone, Debug)]
pub struct VDirection {
    params: Vec<f64>,
    knots: KnotVec,
    degree: usize,
    lu: Banded,
}

impl VDirection {
    /// The v direction for a loft whose columns are `columns`, at degree
    /// `min(max_degree, K)`.
    ///
    /// `max_degree` is [`MAX_V_DEGREE`] for a smooth loft and `1` for a ruled
    /// one; clamping to `K` is what makes a two-section "smooth" loft the
    /// ruled loft, which is the honest answer — there is nothing to be smooth
    /// between.
    pub fn new(columns: &[Compatible], max_degree: usize) -> Result<VDirection> {
        let Some(first) = columns.first() else {
            return Err(CadpoleError::invalid("a skinned loft needs at least one section curve"));
        };
        let sections = first.sections();
        if columns.iter().any(|c| c.sections() != sections) {
            return Err(CadpoleError::kernel(
                "skinning: the loft's columns disagree on how many sections there are",
            ));
        }
        if sections < 2 {
            return Err(CadpoleError::invalid("a skinned loft needs at least two sections"));
        }
        let k = sections - 1;
        let degree = max_degree.min(k).max(1);
        let params = section_params(columns, k);
        let knots = averaged_knots(&params, degree);

        // A[k][l] = N_{l,q}(v_k). One matrix for every column of the loft.
        let mut lu = Banded::new(sections, degree);
        for (row, &v) in params.iter().enumerate() {
            let basis = knots.try_bspline_basis_functions(degree, v).map_err(|e| {
                CadpoleError::kernel(format!("skinning: the section parameters are unusable ({e})"))
            })?;
            for (col, &b) in basis.iter().enumerate() {
                if b == 0.0 {
                    continue;
                }
                if lu.slot(row, col).is_none() {
                    return Err(CadpoleError::kernel(format!(
                        "skinning: basis function {col} is nonzero at section {row}, which is \
                         outside the band the averaging knot vector guarantees"
                    )));
                }
                lu.set(row, col, b);
            }
        }
        lu.factor()?;
        Ok(VDirection { params, knots, degree, lu })
    }

    /// The parameter each section sits at: `S(u, v_k) = C_k(u)`.
    pub fn params(&self) -> &[f64] {
        &self.params
    }

    /// The surface's v knot vector.
    pub fn knots(&self) -> &KnotVec {
        &self.knots
    }

    /// `q`.
    pub fn degree(&self) -> usize {
        self.degree
    }

    /// The skinned surface through one column's sections, in homogeneous
    /// form. `NurbsSurface::new` of this is the face's surface; the
    /// non-rational case has every weight `1` and comes out the same either
    /// way.
    pub fn skin(&self, column: &Compatible) -> Result<BSplineSurface<Vector4>> {
        if column.sections() != self.params.len() {
            return Err(CadpoleError::kernel(format!(
                "skinning: a column of {} sections against a v direction built for {}",
                column.sections(),
                self.params.len()
            )));
        }
        let mut control_points = Vec::with_capacity(column.width());
        let mut rhs = vec![Vector4::new(0.0, 0.0, 0.0, 0.0); column.sections()];
        for i in 0..column.width() {
            for (k, r) in rhs.iter_mut().enumerate() {
                *r = column.points[k][i];
            }
            let solved = self.lu.solve(&rhs);
            for p in &solved {
                if !(p.w > MIN_WEIGHT) {
                    return Err(CadpoleError::invalid(format!(
                        "the skinned surface through these sections has a control point of weight \
                         {:.3e}, which does not define a surface. Interpolating weights can turn \
                         them negative when the sections' shape changes sharply from one section \
                         to the next; add an intermediate section, or pass ruled=True",
                        p.w
                    )));
                }
            }
            control_points.push(solved);
        }
        BSplineSurface::try_new((column.knots.clone(), self.knots.clone()), control_points).map_err(
            |e| CadpoleError::kernel(format!("skinning: the surface is not well formed ({e})")),
        )
    }
}

/// One parameter per section, centripetal and averaged across every
/// control-point column of every one of the loft's columns.
///
/// The parameters describe
/// *the loft*, not a column of it: a column that happens to bunch its points
/// towards the top does not get to decide that the sections are bunched
/// towards the top, it gets one vote.
fn section_params(columns: &[Compatible], k: usize) -> Vec<f64> {
    let mut sum = vec![0.0; k + 1];
    let mut counted = 0.0;
    for column in columns {
        for i in 0..column.width() {
            // Euclidean, not homogeneous: this is a measurement of how far the
            // control point *moves* between sections, and a weight is not a
            // length. (The weights matter to the shape, and they went through
            // compatibility together with the point; they have no business in
            // a distance.)
            let poly: Vec<[f64; 3]> = (0..=k)
                .map(|s| {
                    let p = column.points[s][i];
                    let w = if p.w.abs() > MIN_WEIGHT { p.w } else { 1.0 };
                    [p.x / w, p.y / w, p.z / w]
                })
                .collect();
            let step = |a: &[f64; 3], b: &[f64; 3]| {
                let (dx, dy, dz) = (b[0] - a[0], b[1] - a[1], b[2] - a[2]);
                fmath::powf((dx * dx + dy * dy + dz * dz).sqrt(), ALPHA)
            };
            let total: f64 = (1..=k).map(|s| step(&poly[s - 1], &poly[s])).sum();
            counted += 1.0;
            if total < FLAT_COLUMN {
                // A column that does not move has no shape to read parameters
                // off, so it says "evenly spaced" and lets the others decide.
                for (s, acc) in sum.iter_mut().enumerate() {
                    *acc += s as f64 / k as f64;
                }
                continue;
            }
            let mut run = 0.0;
            for s in 1..=k {
                run += step(&poly[s - 1], &poly[s]);
                sum[s] += run / total;
            }
        }
    }
    let mut params: Vec<f64> = sum.iter().map(|s| s / counted).collect();
    // The ends are exact, not the average of numbers that should have summed
    // to them.
    params[0] = 0.0;
    params[k] = 1.0;
    params
}

/// The v knot vector by the averaging formula.
///
/// `V = [0×(q+1), v̄_1 … v̄_{K−q}, 1×(q+1)]` with
/// `v̄_j = (1/q)·Σ_{k=j..j+q−1} v_k`. The point of averaging rather than, say,
/// spreading the interior knots uniformly is that it puts at least one section
/// parameter in the support of every basis function — the Schoenberg–Whitney
/// condition — and *that* is what guarantees the interpolation matrix is
/// nonsingular. Any other spacing can produce a singular system for section
/// positions that are perfectly reasonable.
fn averaged_knots(params: &[f64], q: usize) -> KnotVec {
    let k = params.len() - 1;
    let mut v = vec![0.0; q + 1];
    for j in 1..=k.saturating_sub(q) {
        v.push(params[j..j + q].iter().sum::<f64>() / q as f64);
    }
    v.extend(std::iter::repeat(1.0).take(q + 1));
    KnotVec::from(v)
}

#[cfg(test)]
mod tests {
    use super::*;
    use truck_modeling::{InnerSpace, ParametricCurve, ParametricSurface};

    /// A straight-line section curve as a homogeneous B-spline: degree 1, two
    /// control points, weights 1.
    fn line(a: [f64; 3], b: [f64; 3]) -> BSplineCurve<Vector4> {
        BSplineCurve::new(
            KnotVec::from(vec![0.0, 0.0, 1.0, 1.0]),
            vec![Vector4::new(a[0], a[1], a[2], 1.0), Vector4::new(b[0], b[1], b[2], 1.0)],
        )
    }

    /// Compatibility only ever adds representation, never moves the curve: a
    /// degree-1 line made compatible with a knotted cubic is still that line.
    #[test]
    fn making_sections_compatible_does_not_move_them() {
        let straight = line([0.0, 0.0, 0.0], [10.0, 0.0, 0.0]);
        let mut cubic = line([0.0, 0.0, 4.0], [10.0, 0.0, 4.0]);
        cubic.elevate_degree();
        cubic.elevate_degree();
        cubic.add_knot(0.3);
        let before = straight.clone();
        let c = make_compatible(vec![straight, cubic]).expect("compatible");
        assert_eq!(c.degree(), 3, "both sections at the higher degree");
        assert_eq!(c.width(), c.points[1].len(), "and the same width");
        let after = BSplineCurve::new(c.knots.clone(), c.points[0].clone());
        for i in 0..=20 {
            let t = i as f64 / 20.0;
            let (p, q) = (before.subs(t), after.subs(t));
            assert!((p - q).magnitude() < 1e-12, "at t={t}: {p:?} against {q:?}");
        }
    }

    /// The v direction of a two-section loft is the ruled one, exactly: knots
    /// `[0,0,1,1]` and an interpolation that is the identity on its input.
    #[test]
    fn two_sections_reduce_to_the_ruled_knot_vector_and_the_identity() {
        let a = line([0.0, 0.0, 0.0], [10.0, 0.0, 0.0]);
        let b = line([0.0, 0.0, 5.0], [7.0, 3.0, 5.0]);
        let c = make_compatible(vec![a, b]).expect("compatible");
        let v = VDirection::new(std::slice::from_ref(&c), MAX_V_DEGREE).expect("a v direction");
        assert_eq!(v.degree(), 1);
        assert_eq!(v.knots().as_slice(), &[0.0, 0.0, 1.0, 1.0]);
        let rhs = vec![Vector4::new(1.5, -2.5, 3.5, 1.0), Vector4::new(-4.0, 0.25, 9.0, 0.5)];
        let got = v.lu.solve(&rhs);
        // Bit for bit, not "near": this is what makes a ruled loft's solid
        // identical to the one the homotopy built.
        assert_eq!(got[0], rhs[0]);
        assert_eq!(got[1], rhs[1]);
    }

    /// The defining property: the surface passes through every section.
    #[test]
    fn a_skinned_surface_passes_through_every_section() {
        // Five sections that are *not* related by a uniform scale, which is
        // the case a per-column parameterisation gets wrong.
        let sections = vec![
            line([0.0, 0.0, 0.0], [10.0, 0.0, 0.0]),
            line([1.0, 0.0, 2.0], [9.0, 4.0, 2.0]),
            line([0.5, 0.0, 5.0], [14.0, 1.0, 5.0]),
            line([2.0, 0.0, 9.0], [11.0, -3.0, 9.0]),
            line([0.0, 0.0, 12.0], [10.0, 0.0, 12.0]),
        ];
        let want = sections.clone();
        let c = make_compatible(sections).expect("compatible");
        let v = VDirection::new(std::slice::from_ref(&c), MAX_V_DEGREE).expect("a v direction");
        assert_eq!(v.degree(), 3);
        let s = v.skin(&c).expect("a surface");
        for (k, curve) in want.iter().enumerate() {
            for i in 0..=20 {
                let u = i as f64 / 20.0;
                let on_surface = s.subs(u, v.params()[k]);
                let on_section = curve.subs(u);
                let (a, b) = (on_surface / on_surface.w, on_section / on_section.w);
                assert!(
                    (a - b).magnitude() < 1e-9,
                    "section {k} at u={u}: surface {a:?} against section {b:?}"
                );
            }
        }
    }

    /// **The knot-union blow-up, bounded.** Sections with unlike knot vectors
    /// refine each other, so the compatible sections carry every section's
    /// knots at once. The bound that follows from the construction is
    ///
    /// ```text
    ///     n + 1  ≤  (p + 1) + Σ_k (n_k − p)
    /// ```
    ///
    /// — the clamped end knots once, plus every section's interior knots, in
    /// the worst case where no two sections share one. Here that is 4 + 3 + 3 +
    /// 3 = 13 control points for three cubics with three disjoint interior
    /// knots each, against the 4 a single section had.
    ///
    /// This is a real cost; the remedies are knot removal within tolerance
    /// (Tiller 1992) and approximate skinning. Cadpole does neither, which
    /// is fine for a handful of hand-written sections and is exactly why the
    /// bound is written down and asserted rather than left to be discovered on
    /// a section stack that matters.
    #[test]
    fn unlike_knot_vectors_blow_the_control_points_up_to_the_stated_bound() {
        let knotted = |ks: [f64; 3]| {
            let mut c = line([0.0, 0.0, 0.0], [10.0, 0.0, 0.0]);
            c.elevate_degree();
            c.elevate_degree();
            for k in ks {
                c.add_knot(k);
            }
            c
        };
        let sections = vec![knotted([0.1, 0.2, 0.3]), knotted([0.4, 0.5, 0.6]), knotted([0.7, 0.8, 0.9])];
        let widths: Vec<usize> = sections.iter().map(|c| c.control_points().len()).collect();
        let p = 3;
        let bound: usize = (p + 1) + widths.iter().map(|n| n - 1 - p).sum::<usize>();
        assert_eq!(bound, 13, "four cubic control points plus three interior knots each");
        let c = make_compatible(sections).expect("compatible");
        assert_eq!(c.degree(), p);
        assert_eq!(c.width(), bound, "the union of disjoint interior knots, exactly");

        // And when the sections *share* their knots there is no blow-up at all:
        // the union is one section's knot vector.
        let shared = vec![knotted([0.1, 0.2, 0.3]), knotted([0.1, 0.2, 0.3])];
        assert_eq!(make_compatible(shared).expect("compatible").width(), 7);
    }

    /// Sections that cannot be made compatible are a diagnostic naming the
    /// section, never a panic — Cadpole's rule for everything a script can reach.
    #[test]
    fn sections_that_cannot_be_made_compatible_are_a_diagnostic() {
        let straight = line([0.0, 0.0, 0.0], [10.0, 0.0, 0.0]);

        let one = make_compatible(vec![straight.clone()]).unwrap_err().to_string();
        assert!(one.contains("at least two section curves"), "{one}");

        // A curve whose knot vector is not clamped does not start where its
        // control polygon does, so there is nothing to put on a common domain.
        let unclamped = BSplineCurve::new(
            KnotVec::from(vec![0.0, 1.0, 2.0, 3.0, 4.0, 5.0]),
            vec![
                Vector4::new(0.0, 0.0, 0.0, 1.0),
                Vector4::new(1.0, 1.0, 0.0, 1.0),
                Vector4::new(2.0, 0.0, 0.0, 1.0),
            ],
        );
        let e = make_compatible(vec![straight.clone(), unclamped]).unwrap_err().to_string();
        assert!(e.contains("loft section 1") && e.contains("unclamped"), "{e}");

        // Two sections that are the same curve give two identical rows in the
        // interpolation matrix, which is singular. Say so; do not divide by it.
        let sections = vec![straight.clone(), straight.clone(), line([0.0, 0.0, 6.0], [9.0, 2.0, 6.0])];
        let c = make_compatible(sections).expect("compatible");
        let e = VDirection::new(std::slice::from_ref(&c), MAX_V_DEGREE).unwrap_err().to_string();
        assert!(e.contains("singular"), "{e}");
    }

    /// Schoenberg–Whitney: every knot span of the v knot vector holds a
    /// section parameter, for section spacing as uneven as a script can write.
    #[test]
    fn the_averaged_knots_put_a_section_in_every_span() {
        let params = vec![0.0, 0.001, 0.002, 0.9, 0.95, 1.0];
        for q in 1..=3 {
            let kv = averaged_knots(&params, q);
            let n = kv.len() - q - 1;
            assert_eq!(n, params.len(), "one basis function per section");
            for l in 0..n {
                let (lo, hi) = (kv[l], kv[l + q + 1]);
                assert!(
                    params.iter().any(|&v| v > lo - 1e-15 && v < hi + 1e-15),
                    "q={q}: basis {l} over [{lo}, {hi}] has no section parameter"
                );
            }
        }
    }
}
