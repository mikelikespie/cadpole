//! Interval arithmetic: the half of the tape that makes meshing tractable.
//!
//! Every tape node has two evaluators. A *point* evaluator answers "what is
//! the distance at `p`" in plain `f64` and is used at the leaves of the
//! octree, where the surface actually is. An *interval* evaluator answers
//! "over this whole box, what is the range of the distance", and is used to
//! throw away entire subtrees of the octree without ever sampling them
//! (Duff 1992; Keeter 2020). A box whose interval does not straddle zero
//! cannot contain the surface, so it is never subdivided and never sampled.
//!
//! Every operation here is an **inclusion function**: the returned interval
//! is guaranteed to contain the true range of the operation over its inputs.
//! It may be wider (interval arithmetic always over-estimates when a variable
//! appears twice), which costs work but never correctness. If any of these
//! ever returned too *narrow* an interval, the mesher would silently drop
//! geometry, so the tests check containment by brute-force sampling.

use cadpole_core::fmath;

/// A closed real interval `[lo, hi]`. `lo > hi` means empty, which only ever
/// comes out of [`Interval::EMPTY`]; no operation here produces one.
#[derive(Clone, Copy, Debug, PartialEq)]
pub struct Interval {
    pub lo: f64,
    pub hi: f64,
}

impl Interval {
    pub const EMPTY: Interval = Interval { lo: f64::INFINITY, hi: f64::NEG_INFINITY };

    #[inline]
    pub const fn new(lo: f64, hi: f64) -> Interval {
        Interval { lo, hi }
    }

    /// The degenerate interval `[v, v]`.
    #[inline]
    pub const fn point(v: f64) -> Interval {
        Interval { lo: v, hi: v }
    }

    #[inline]
    pub fn contains_zero(self) -> bool {
        self.lo <= 0.0 && self.hi >= 0.0
    }

    #[inline]
    pub fn width(self) -> f64 {
        self.hi - self.lo
    }

    #[inline]
    pub fn mid(self) -> f64 {
        0.5 * (self.lo + self.hi)
    }

    /// True when every value in the interval is strictly negative — the box
    /// is entirely inside the solid.
    #[inline]
    pub fn all_negative(self) -> bool {
        self.hi < 0.0
    }

    /// True when every value is strictly positive — the box is entirely
    /// outside the solid.
    #[inline]
    pub fn all_positive(self) -> bool {
        self.lo > 0.0
    }

    #[inline]
    pub fn neg(self) -> Interval {
        Interval::new(-self.hi, -self.lo)
    }

    #[inline]
    pub fn add(self, o: Interval) -> Interval {
        Interval::new(self.lo + o.lo, self.hi + o.hi)
    }

    #[inline]
    pub fn sub(self, o: Interval) -> Interval {
        Interval::new(self.lo - o.hi, self.hi - o.lo)
    }

    #[inline]
    pub fn add_scalar(self, s: f64) -> Interval {
        Interval::new(self.lo + s, self.hi + s)
    }

    #[inline]
    pub fn mul(self, o: Interval) -> Interval {
        let a = self.lo * o.lo;
        let b = self.lo * o.hi;
        let c = self.hi * o.lo;
        let d = self.hi * o.hi;
        Interval::new(a.min(b).min(c).min(d), a.max(b).max(c).max(d))
    }

    #[inline]
    pub fn mul_scalar(self, s: f64) -> Interval {
        if s >= 0.0 { Interval::new(self.lo * s, self.hi * s) } else { Interval::new(self.hi * s, self.lo * s) }
    }

    #[inline]
    pub fn min(self, o: Interval) -> Interval {
        Interval::new(self.lo.min(o.lo), self.hi.min(o.hi))
    }

    #[inline]
    pub fn max(self, o: Interval) -> Interval {
        Interval::new(self.lo.max(o.lo), self.hi.max(o.hi))
    }

    /// `|x|` over the interval: zero is the minimum whenever the interval
    /// straddles it.
    #[inline]
    pub fn abs(self) -> Interval {
        if self.lo >= 0.0 {
            self
        } else if self.hi <= 0.0 {
            self.neg()
        } else {
            Interval::new(0.0, self.lo.abs().max(self.hi))
        }
    }

    /// `x^2`, tighter than `mul(self, self)` because it knows the two factors
    /// are the same variable.
    #[inline]
    pub fn square(self) -> Interval {
        let a = self.abs();
        Interval::new(a.lo * a.lo, a.hi * a.hi)
    }

    /// `sqrt(x)` clamped at zero: the tape only ever takes roots of sums of
    /// squares, so a negative low end is rounding, not a domain error.
    #[inline]
    pub fn sqrt(self) -> Interval {
        Interval::new(self.lo.max(0.0).sqrt(), self.hi.max(0.0).sqrt())
    }

    /// `max(x, 0)`, the clamp that every "distance outside the box" formula
    /// is built from.
    #[inline]
    pub fn max0(self) -> Interval {
        Interval::new(self.lo.max(0.0), self.hi.max(0.0))
    }

    /// `min(x, 0)`.
    #[inline]
    pub fn min0(self) -> Interval {
        Interval::new(self.lo.min(0.0), self.hi.min(0.0))
    }

    /// Euclidean length of a vector of intervals: `sqrt(sum of squares)`.
    #[inline]
    pub fn length2(x: Interval, y: Interval) -> Interval {
        x.square().add(y.square()).sqrt()
    }

    #[inline]
    pub fn length3(x: Interval, y: Interval, z: Interval) -> Interval {
        x.square().add(y.square()).add(z.square()).sqrt()
    }

    /// The hull of two intervals (used where a case split cannot be resolved).
    #[inline]
    pub fn hull(self, o: Interval) -> Interval {
        Interval::new(self.lo.min(o.lo), self.hi.max(o.hi))
    }

    /// `sin` over the interval, exact: the extremes are the endpoints unless
    /// the interval sweeps past a peak or a trough.
    pub fn sin(self) -> Interval {
        trig(self, 0.0)
    }

    /// `cos` over the interval, exact (as `sin(x + pi/2)`).
    pub fn cos(self) -> Interval {
        trig(self, std::f64::consts::FRAC_PI_2)
    }
}

/// `sin(x + phase)` over an interval. Returns `[-1, 1]` once the interval is
/// wider than a full turn, and otherwise checks whether the swept arc passes
/// the peak at `pi/2` or the trough at `3pi/2`.
fn trig(i: Interval, phase: f64) -> Interval {
    use std::f64::consts::{PI, TAU};
    let (a, b) = (i.lo + phase, i.hi + phase);
    if !(a.is_finite() && b.is_finite()) || b - a >= TAU {
        return Interval::new(-1.0, 1.0);
    }
    let sa = fmath::sin(a);
    let sb = fmath::sin(b);
    let mut lo = sa.min(sb);
    let mut hi = sa.max(sb);
    // Peaks at pi/2 + 2k*pi, troughs at 3pi/2 + 2k*pi.
    let first_peak = (a - PI / 2.0) / TAU;
    if (first_peak.ceil() * TAU + PI / 2.0) <= b {
        hi = 1.0;
    }
    let first_trough = (a - 3.0 * PI / 2.0) / TAU;
    if (first_trough.ceil() * TAU + 3.0 * PI / 2.0) <= b {
        lo = -1.0;
    }
    Interval::new(lo, hi)
}

/// An axis-aligned box of intervals: the domain a tape node is evaluated over.
#[derive(Clone, Copy, Debug, PartialEq)]
pub struct Box3 {
    pub x: Interval,
    pub y: Interval,
    pub z: Interval,
}

impl Box3 {
    pub fn new(lo: [f64; 3], hi: [f64; 3]) -> Box3 {
        Box3 {
            x: Interval::new(lo[0], hi[0]),
            y: Interval::new(lo[1], hi[1]),
            z: Interval::new(lo[2], hi[2]),
        }
    }

    pub fn point(p: [f64; 3]) -> Box3 {
        Box3 { x: Interval::point(p[0]), y: Interval::point(p[1]), z: Interval::point(p[2]) }
    }

    pub fn lo(&self) -> [f64; 3] {
        [self.x.lo, self.y.lo, self.z.lo]
    }

    pub fn hi(&self) -> [f64; 3] {
        [self.x.hi, self.y.hi, self.z.hi]
    }

    pub fn center(&self) -> [f64; 3] {
        [self.x.mid(), self.y.mid(), self.z.mid()]
    }

    /// Half the length of the box diagonal — the Lipschitz radius used to
    /// bound a 1-Lipschitz field (a true signed distance) over the box from a
    /// single sample at its centre.
    pub fn radius(&self) -> f64 {
        0.5 * (fmath::powi(self.x.width(), 2) + fmath::powi(self.y.width(), 2) + fmath::powi(self.z.width(), 2)).sqrt()
    }

    /// Grow the box by `d` in every direction.
    pub fn expanded(&self, d: f64) -> Box3 {
        Box3 {
            x: Interval::new(self.x.lo - d, self.x.hi + d),
            y: Interval::new(self.y.lo - d, self.y.hi + d),
            z: Interval::new(self.z.lo - d, self.z.hi + d),
        }
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    /// Brute-force check that an interval extension really does contain the
    /// range of the scalar operation over the same inputs.
    fn contains_range(got: Interval, f: impl Fn(f64, f64) -> f64, a: Interval, b: Interval) {
        let n = 41;
        for i in 0..=n {
            for j in 0..=n {
                let x = a.lo + (a.hi - a.lo) * i as f64 / n as f64;
                let y = b.lo + (b.hi - b.lo) * j as f64 / n as f64;
                let v = f(x, y);
                assert!(
                    v >= got.lo - 1e-12 && v <= got.hi + 1e-12,
                    "f({x}, {y}) = {v} escapes {got:?}"
                );
            }
        }
    }

    #[test]
    fn arithmetic_extensions_contain_the_range() {
        let cases = [
            (Interval::new(-3.0, 2.0), Interval::new(0.5, 4.0)),
            (Interval::new(-7.0, -1.0), Interval::new(-2.0, 6.0)),
            (Interval::new(0.0, 0.0), Interval::new(-1.0, 1.0)),
        ];
        for (a, b) in cases {
            contains_range(a.add(b), |x, y| x + y, a, b);
            contains_range(a.sub(b), |x, y| x - y, a, b);
            contains_range(a.mul(b), |x, y| x * y, a, b);
            contains_range(a.min(b), |x, y| x.min(y), a, b);
            contains_range(a.max(b), |x, y| x.max(y), a, b);
            contains_range(a.abs(), |x, _| x.abs(), a, b);
            contains_range(a.square(), |x, _| x * x, a, b);
            contains_range(a.max0(), |x, _| x.max(0.0), a, b);
            contains_range(a.min0(), |x, _| x.min(0.0), a, b);
            contains_range(Interval::length2(a, b), |x, y| (x * x + y * y).sqrt(), a, b);
        }
    }

    #[test]
    fn square_is_tighter_than_multiplying_by_itself() {
        let a = Interval::new(-3.0, 2.0);
        assert_eq!(a.square(), Interval::new(0.0, 9.0));
        // The generic product cannot know the two factors move together.
        assert_eq!(a.mul(a), Interval::new(-6.0, 9.0));
    }

    #[test]
    fn trig_finds_the_peaks_it_sweeps_past() {
        let half = std::f64::consts::FRAC_PI_2;
        // Straddles the peak of sin.
        let i = Interval::new(half - 0.1, half + 0.1).sin(); // not f64: Interval
        assert!((i.hi - 1.0).abs() < 1e-12, "{i:?}");
        // Does not reach it.
        let j = Interval::new(0.0, 1.0).sin(); // not f64: Interval
        assert!(j.hi < 1.0 - 1e-6, "{j:?}");
        // Wider than a turn is the full range.
        let k = Interval::new(-10.0, 10.0).sin(); // not f64: Interval
        assert_eq!(k, Interval::new(-1.0, 1.0));
        for (lo, hi) in [(-0.3, 0.9), (2.0, 5.0), (-4.0, -1.0), (0.0, 0.0)] {
            let a = Interval::new(lo, hi);
            contains_range(a.sin(), |x, _| fmath::sin(x), a, a); // not f64: Interval
            contains_range(a.cos(), |x, _| fmath::cos(x), a, a); // not f64: Interval
        }
    }

    #[test]
    fn a_box_radius_bounds_a_lipschitz_field() {
        let b = Box3::new([0.0, 0.0, 0.0], [2.0, 2.0, 2.0]);
        assert!((b.radius() - 3.0f64.sqrt()).abs() < 1e-12);
        assert_eq!(b.center(), [1.0, 1.0, 1.0]);
    }
}
