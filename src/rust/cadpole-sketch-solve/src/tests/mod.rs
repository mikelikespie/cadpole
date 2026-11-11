//! The test suite.
//!
//! * `shapes` — the sketches a real user draws: a rectangle, a triangle, a
//!   tangent line, concentric circles, a mirrored pair, a slot.
//! * `catalogue` — one test per remaining constraint.
//! * `diagnostics` — degrees of freedom, redundancy and conflict, by name.
//! * `dragging` — a drag moves one point and the rest follows.
//! * `robustness` — poor initial guesses, determinism, degenerate input.
//! * `property` — randomly generated well-constrained systems all converge.

mod catalogue;
mod diagnostics;
mod dragging;
mod property;
mod robustness;
mod shapes;

use crate::System;
use crate::entity::PointId;

/// Assert two numbers agree, printing both when they do not.
#[track_caller]
pub(crate) fn near(a: f64, b: f64, tol: f64, what: &str) {
    assert!((a - b).abs() <= tol, "{what}: {a} != {b} (tolerance {tol})");
}

#[track_caller]
pub(crate) fn near_point(p: (f64, f64), q: (f64, f64), tol: f64, what: &str) {
    near(p.0, q.0, tol, &format!("{what}.x"));
    near(p.1, q.1, tol, &format!("{what}.y"));
}

pub(crate) fn dist(s: &System, a: PointId, b: PointId) -> f64 {
    let (p, q) = (s.point_pos(a), s.point_pos(b));
    ((p.0 - q.0) * (p.0 - q.0) + (p.1 - q.1) * (p.1 - q.1)).sqrt()
}

/// The angle at `b` in the corner `a-b-c`, in degrees.
pub(crate) fn corner_degrees(s: &System, a: PointId, b: PointId, c: PointId) -> f64 {
    let (pa, pb, pc) = (s.point_pos(a), s.point_pos(b), s.point_pos(c));
    let u = (pa.0 - pb.0, pa.1 - pb.1);
    let v = (pc.0 - pb.0, pc.1 - pb.1);
    let cross = u.0 * v.1 - u.1 * v.0;
    let dot = u.0 * v.0 + u.1 * v.1;
    libm::atan2(cross, dot).abs().to_degrees()
}

/// Distance from a point to the infinite line through `a` and `b`.
pub(crate) fn point_line_distance(p: (f64, f64), a: (f64, f64), b: (f64, f64)) -> f64 {
    let (ux, uy) = (b.0 - a.0, b.1 - a.1);
    let l = (ux * ux + uy * uy).sqrt();
    ((ux * (p.1 - a.1) - uy * (p.0 - a.0)) / l).abs()
}

/// A deterministic 64-bit PRNG (SplitMix64) for *generating test cases*. The
/// solver itself contains no randomness whatsoever.
pub(crate) struct Rng(u64);

impl Rng {
    pub fn new(seed: u64) -> Rng {
        Rng(seed.wrapping_mul(0x9E37_79B9_7F4A_7C15).wrapping_add(0x1234_5678))
    }
    pub fn next_u64(&mut self) -> u64 {
        self.0 = self.0.wrapping_add(0x9E37_79B9_7F4A_7C15);
        let mut z = self.0;
        z = (z ^ (z >> 30)).wrapping_mul(0xBF58_476D_1CE4_E5B9);
        z = (z ^ (z >> 27)).wrapping_mul(0x94D0_49BB_1331_11EB);
        z ^ (z >> 31)
    }
    /// Uniform in `[lo, hi)`.
    pub fn range(&mut self, lo: f64, hi: f64) -> f64 {
        let u = (self.next_u64() >> 11) as f64 / (1u64 << 53) as f64;
        lo + (hi - lo) * u
    }
}
