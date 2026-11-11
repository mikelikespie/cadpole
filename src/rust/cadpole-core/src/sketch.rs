//! 2D sketches: closed loops of segments on a plane.
//!
//! Loops are classified by containment: the outermost loops are material,
//! loops nested one level deeper are holes, two levels deeper are islands,
//! and so on (even-odd nesting).

use crate::geom::{Plane, Vec3};
use crate::fmath;

/// A point in sketch (2D) coordinates. `Default` is the sketch origin.
#[derive(Clone, Copy, Debug, Default, PartialEq)]
pub struct P2 {
    pub x: f64,
    pub y: f64,
}

impl P2 {
    pub const fn new(x: f64, y: f64) -> P2 {
        P2 { x, y }
    }
}

/// The arithmetic a Bézier needs, so that one implementation of the curve
/// machinery — evaluation, arc length, the inverse, de Casteljau — serves
/// both a 2D [`P2`] and a 3D [`Vec3`] (`crate::curve`).
///
/// The operations are spelled out component by component in exactly the
/// order the 2D code always used, so a 2D segment gets the same bits it got
/// before this trait existed: the loft's section matcher moves its
/// split points by the numbers these produce, and a change of one ulp there
/// would move a vertex.
pub trait Coord: Copy + std::fmt::Debug + PartialEq {
    /// `self + (o - self) * t`, per component.
    fn lerp(self, o: Self, t: f64) -> Self;
    /// `(b - a) * k`, per component.
    fn scaled_diff(a: Self, b: Self, k: f64) -> Self;
    /// `self + o`, per component.
    fn add(self, o: Self) -> Self;
    /// Euclidean length of the vector.
    fn norm(self) -> f64;
    /// Euclidean distance to `o`.
    fn dist(self, o: Self) -> f64;
    fn zero() -> Self;
}

impl Coord for P2 {
    #[inline]
    fn lerp(self, o: P2, t: f64) -> P2 {
        P2::new(self.x + (o.x - self.x) * t, self.y + (o.y - self.y) * t)
    }
    #[inline]
    fn scaled_diff(a: P2, b: P2, k: f64) -> P2 {
        P2::new((b.x - a.x) * k, (b.y - a.y) * k)
    }
    #[inline]
    fn add(self, o: P2) -> P2 {
        P2::new(self.x + o.x, self.y + o.y)
    }
    #[inline]
    fn norm(self) -> f64 {
        (self.x * self.x + self.y * self.y).sqrt()
    }
    #[inline]
    fn dist(self, o: P2) -> f64 {
        (fmath::powi(o.x - self.x, 2) + fmath::powi(o.y - self.y, 2)).sqrt()
    }
    fn zero() -> P2 {
        P2::new(0.0, 0.0)
    }
}

impl Coord for Vec3 {
    #[inline]
    fn lerp(self, o: Vec3, t: f64) -> Vec3 {
        Vec3::new(self.x + (o.x - self.x) * t, self.y + (o.y - self.y) * t, self.z + (o.z - self.z) * t)
    }
    #[inline]
    fn scaled_diff(a: Vec3, b: Vec3, k: f64) -> Vec3 {
        Vec3::new((b.x - a.x) * k, (b.y - a.y) * k, (b.z - a.z) * k)
    }
    #[inline]
    fn add(self, o: Vec3) -> Vec3 {
        Vec3::new(self.x + o.x, self.y + o.y, self.z + o.z)
    }
    #[inline]
    fn norm(self) -> f64 {
        (self.x * self.x + self.y * self.y + self.z * self.z).sqrt()
    }
    #[inline]
    fn dist(self, o: Vec3) -> f64 {
        (fmath::powi(o.x - self.x, 2) + fmath::powi(o.y - self.y, 2) + fmath::powi(o.z - self.z, 2)).sqrt()
    }
    fn zero() -> Vec3 {
        Vec3::ZERO
    }
}

/// One segment of a closed loop, in sketch (2D) coordinates.
#[derive(Clone, Debug, PartialEq)]
pub enum Segment {
    Line { a: P2, b: P2 },
    /// Circular arc from `a` to `b` passing through `via`.
    Arc { a: P2, b: P2, via: P2 },
    /// Bézier curve from `a` to `b` with interior control points (1 = quadratic, 2 = cubic).
    Bezier { a: P2, b: P2, ctrl: Vec<P2> },
}

impl Segment {
    pub fn start(&self) -> P2 {
        match self {
            Segment::Line { a, .. } | Segment::Arc { a, .. } | Segment::Bezier { a, .. } => *a,
        }
    }
    pub fn end(&self) -> P2 {
        match self {
            Segment::Line { b, .. } | Segment::Arc { b, .. } | Segment::Bezier { b, .. } => *b,
        }
    }
    /// Direction leaving the start of the segment.
    pub fn start_direction(&self) -> Option<P2> {
        let (a, next) = match self {
            Segment::Line { a, b } => (*a, *b),
            Segment::Arc { a, b, via } => {
                let (c, _, t0, t1) = arc_params(*a, *b, *via)?;
                let s = if t1 > t0 { 1.0 } else { -1.0 };
                let (ex, ey) = (a.x - c.x, a.y - c.y);
                return unit(P2::new(-ey * s, ex * s));
            }
            Segment::Bezier { a, b, ctrl } => (*a, ctrl.first().copied().unwrap_or(*b)),
        };
        unit(P2::new(next.x - a.x, next.y - a.y))
    }
    /// Direction arriving at the end of the segment.
    pub fn end_direction(&self) -> Option<P2> {
        let (prev, b) = match self {
            Segment::Line { a, b } => (*a, *b),
            Segment::Arc { a, b, via } => {
                let (c, _, t0, t1) = arc_params(*a, *b, *via)?;
                let s = if t1 > t0 { 1.0 } else { -1.0 };
                let (ex, ey) = (b.x - c.x, b.y - c.y);
                return unit(P2::new(-ey * s, ex * s));
            }
            Segment::Bezier { a, b, ctrl } => (ctrl.last().copied().unwrap_or(*a), *b),
        };
        unit(P2::new(b.x - prev.x, b.y - prev.y))
    }

    /// Arc length of the segment, in sketch units.
    ///
    /// Exact for lines and circular arcs. A Bézier is integrated numerically
    /// with a fixed composite Gauss–Legendre rule, so it is *deterministic*:
    /// the same control points always give the same number, to the last bit,
    /// which is what the loft's section matcher needs.
    pub fn length(&self) -> f64 {
        match self {
            Segment::Line { a, b } => (fmath::powi(b.x - a.x, 2) + fmath::powi(b.y - a.y, 2)).sqrt(),
            Segment::Arc { a, b, via } => match arc_params(*a, *b, *via) {
                Some((_, r, t0, t1)) => r * (t1 - t0).abs(),
                // A degenerate "arc" (three collinear points) is its chord.
                None => (fmath::powi(b.x - a.x, 2) + fmath::powi(b.y - a.y, 2)).sqrt(),
            },
            Segment::Bezier { a, b, ctrl } => bezier_length_to(&bezier_hull(*a, *b, ctrl), 1.0),
        }
    }

    /// The point a distance `s` along the segment from its start (`s` is
    /// clamped to the segment).
    pub fn point_at_length(&self, s: f64) -> P2 {
        let len = self.length();
        let u = if len > 0.0 { (s / len).clamp(0.0, 1.0) } else { 0.0 };
        match self {
            Segment::Line { a, b } => P2::new(a.x + (b.x - a.x) * u, a.y + (b.y - a.y) * u),
            Segment::Arc { a, b, via } => match arc_params(*a, *b, *via) {
                Some((c, r, t0, t1)) => {
                    let t = t0 + (t1 - t0) * u;
                    P2::new(c.x + r * fmath::cos(t), c.y + r * fmath::sin(t))
                }
                None => P2::new(a.x + (b.x - a.x) * u, a.y + (b.y - a.y) * u),
            },
            Segment::Bezier { a, b, ctrl } => {
                let hull = bezier_hull(*a, *b, ctrl);
                bezier_point(&hull, bezier_param_at_length(&hull, s))
            }
        }
    }

    /// The piece of this segment between arc lengths `s0` and `s1`, **as the
    /// same kind of curve on the same underlying geometry**.
    ///
    /// This is what makes the loft's section matcher lossless:
    /// splitting a line gives collinear lines; splitting a circular arc gives
    /// circular arcs *of the same circle* (the sub-arc's `via` is the point at
    /// the middle **angle**, so the three points reconstruct the original
    /// centre, radius and sense); splitting a Bézier is de Casteljau
    /// subdivision, which reproduces the curve exactly. Nothing is ever
    /// replaced by a chord — a band between two concentric sub-arcs of equal
    /// angular span is still an exact cone, which is the whole reason `loft`
    /// is on the B-rep path at all.
    ///
    /// `None` when the piece would be empty or the arc lengths are reversed.
    pub fn sub(&self, s0: f64, s1: f64) -> Option<Segment> {
        let len = self.length();
        if s1 <= s0 || len <= 0.0 {
            return None;
        }
        let (s0, s1) = (s0.clamp(0.0, len), s1.clamp(0.0, len));
        if s1 <= s0 {
            return None;
        }
        // The whole segment: hand back the original, bit for bit, so a loop
        // that needed no splitting there is unchanged.
        if s0 <= 0.0 && s1 >= len {
            return Some(self.clone());
        }
        Some(match self {
            Segment::Line { .. } => {
                Segment::Line { a: self.point_at_length(s0), b: self.point_at_length(s1) }
            }
            Segment::Arc { a, b, via } => match arc_params(*a, *b, *via) {
                Some(_) => Segment::Arc {
                    a: self.point_at_length(s0),
                    b: self.point_at_length(s1),
                    via: self.point_at_length((s0 + s1) / 2.0),
                },
                None => Segment::Line { a: self.point_at_length(s0), b: self.point_at_length(s1) },
            },
            Segment::Bezier { a, b, ctrl } => {
                let hull = bezier_hull(*a, *b, ctrl);
                let t0 = bezier_param_at_length(&hull, s0);
                let t1 = bezier_param_at_length(&hull, s1);
                let piece = bezier_sub(&hull, t0, t1);
                Segment::Bezier {
                    a: piece[0],
                    b: *piece.last().unwrap(),
                    ctrl: piece[1..piece.len() - 1].to_vec(),
                }
            }
        })
    }
}

/// Perpendicular distance from `p` to the line through `a` and `b`.
fn point_line_distance(p: P2, a: P2, b: P2) -> f64 {
    let (dx, dy) = (b.x - a.x, b.y - a.y);
    let len = (dx * dx + dy * dy).sqrt();
    if len < 1e-12 {
        return (fmath::powi(p.x - a.x, 2) + fmath::powi(p.y - a.y, 2)).sqrt();
    }
    ((p.x - a.x) * dy - (p.y - a.y) * dx).abs() / len
}

fn unit(v: P2) -> Option<P2> {
    let l = (v.x * v.x + v.y * v.y).sqrt();
    if l == 0.0 { None } else { Some(P2::new(v.x / l, v.y / l)) }
}

/// Evaluate a Bézier with control polygon `pts` at `t` (de Casteljau).
pub fn bezier_point<P: Coord>(pts: &[P], t: f64) -> P {
    let mut p: Vec<P> = pts.to_vec();
    let n = p.len();
    for r in 1..n {
        for i in 0..n - r {
            p[i] = p[i].lerp(p[i + 1], t);
        }
    }
    p[0]
}

/// The full control polygon of a Bézier segment: start, interior controls, end.
pub(crate) fn bezier_hull<P: Coord>(a: P, b: P, ctrl: &[P]) -> Vec<P> {
    let mut v = Vec::with_capacity(ctrl.len() + 2);
    v.push(a);
    v.extend_from_slice(ctrl);
    v.push(b);
    v
}

/// The hodograph (derivative) control polygon: `B'(t)` is a Bézier of one
/// lower degree over these points.
pub(crate) fn bezier_derivative<P: Coord>(pts: &[P]) -> Vec<P> {
    let n = pts.len();
    if n < 2 {
        return vec![P::zero()];
    }
    let k = (n - 1) as f64;
    (0..n - 1).map(|i| P::scaled_diff(pts[i], pts[i + 1], k)).collect()
}

/// Gauss–Legendre nodes and weights on `[-1, 1]`, 5 points (exact to degree 9,
/// so a cubic's |B'| is integrated to well past `f64` noise once the interval
/// is subdivided). A fixed rule, deliberately: the arc length of a spline has
/// to be the *same number* every run or the matcher would move its splits.
pub(crate) const GL5: [(f64, f64); 5] = [
    (-0.906_179_845_938_664, 0.236_926_885_056_189),
    (-0.538_469_310_105_683, 0.478_628_670_499_366),
    (0.0, 0.568_888_888_888_889),
    (0.538_469_310_105_683, 0.478_628_670_499_366),
    (0.906_179_845_938_664, 0.236_926_885_056_189),
];

/// How many sub-intervals the composite rule uses over `[0, 1]`.
const BEZIER_QUAD_STEPS: usize = 16;

/// Arc length of the Bézier `pts` from `t = 0` to `t = t_end`.
pub(crate) fn bezier_length_to<P: Coord>(pts: &[P], t_end: f64) -> f64 {
    let t_end = t_end.clamp(0.0, 1.0);
    if t_end <= 0.0 {
        return 0.0;
    }
    let d = bezier_derivative(pts);
    let step = t_end / BEZIER_QUAD_STEPS as f64;
    let mut total = 0.0;
    for k in 0..BEZIER_QUAD_STEPS {
        let (lo, hi) = (k as f64 * step, (k + 1) as f64 * step);
        let (mid, half) = ((lo + hi) / 2.0, (hi - lo) / 2.0);
        for (x, w) in GL5 {
            let v = bezier_point(&d, mid + half * x);
            total += w * v.norm() * half;
        }
    }
    total
}

/// The parameter `t` at arc length `s` along the Bézier `pts`.
///
/// Bisection on a monotone function, a fixed 60 iterations, so it is
/// deterministic and converges to the last bit of an `f64` parameter.
pub(crate) fn bezier_param_at_length<P: Coord>(pts: &[P], s: f64) -> f64 {
    let total = bezier_length_to(pts, 1.0);
    if total <= 0.0 {
        return 0.0;
    }
    let target = s.clamp(0.0, total);
    let (mut lo, mut hi) = (0.0f64, 1.0f64);
    for _ in 0..60 {
        let mid = (lo + hi) / 2.0;
        if bezier_length_to(pts, mid) < target { lo = mid } else { hi = mid }
    }
    (lo + hi) / 2.0
}

/// de Casteljau subdivision at `t`: the control polygons of the two halves.
fn bezier_split<P: Coord>(pts: &[P], t: f64) -> (Vec<P>, Vec<P>) {
    let mut rows: Vec<Vec<P>> = vec![pts.to_vec()];
    for r in 1..pts.len() {
        let prev = &rows[r - 1];
        let cur = (0..prev.len() - 1).map(|i| prev[i].lerp(prev[i + 1], t)).collect();
        rows.push(cur);
    }
    let left: Vec<P> = rows.iter().map(|r| r[0]).collect();
    let right: Vec<P> = rows.iter().rev().map(|r| *r.last().unwrap()).collect();
    (left, right)
}

/// The control polygon of the piece of `pts` between parameters `t0 < t1`.
/// Exact: de Casteljau reproduces the curve, it does not approximate it.
pub(crate) fn bezier_sub<P: Coord>(pts: &[P], t0: f64, t1: f64) -> Vec<P> {
    let (_, right) = bezier_split(pts, t0.clamp(0.0, 1.0));
    let rest = 1.0 - t0;
    let tt = if rest.abs() < 1e-15 { 1.0 } else { ((t1 - t0) / rest).clamp(0.0, 1.0) };
    bezier_split(&right, tt).0
}

/// The cubic Bézier control points of a Catmull-Rom spline through `points`
/// (`tension` 0..1 where 0.5 is the classic centripetal-ish look), one
/// `(a, c1, c2, b)` per span. `closed` wraps around. The 2D and 3D splines
/// are this one function, so a spline drawn in a sketch and lifted with
/// `Path.on(plane)` is the spline drawn in space through the same points.
pub(crate) fn catmull_rom_controls<P: Coord>(points: &[P], closed: bool, tension: f64) -> Vec<(P, P, P, P)> {
    let n = points.len();
    let mut out = Vec::new();
    if n < 2 {
        return out;
    }
    let k = (1.0 - tension) / 2.0 * 2.0 / 3.0 * 1.5; // = (1 - tension)/2 scaled to bezier handles
    let get = |i: isize| -> P {
        if closed {
            points[i.rem_euclid(n as isize) as usize]
        } else {
            points[i.clamp(0, n as isize - 1) as usize]
        }
    };
    let count = if closed { n } else { n - 1 };
    for i in 0..count as isize {
        let (p0, p1, p2, p3) = (get(i - 1), get(i), get(i + 1), get(i + 2));
        let c1 = p1.add(P::scaled_diff(p0, p2, k));
        let c2 = p2.add(P::scaled_diff(p3, p1, k));
        out.push((p1, c1, c2, p2));
    }
    out
}

/// Cubic Bézier segments through `points` (Catmull-Rom, `tension` 0..1 where
/// 0.5 is the classic centripetal-ish look). `closed` wraps around.
pub fn catmull_rom(points: &[P2], closed: bool, tension: f64) -> Vec<Segment> {
    catmull_rom_controls(points, closed, tension)
        .into_iter()
        .map(|(a, c1, c2, b)| Segment::Bezier { a, b, ctrl: vec![c1, c2] })
        .collect()
}

/// A closed loop of segments. Each segment starts where the previous ended.
#[derive(Clone, Debug, PartialEq)]
pub struct Loop {
    pub segments: Vec<Segment>,
}

impl Loop {
    pub fn polygon(points: &[P2]) -> Loop {
        let n = points.len();
        let segments =
            (0..n).map(|i| Segment::Line { a: points[i], b: points[(i + 1) % n] }).collect();
        Loop { segments }
    }

    pub fn rect(center: P2, w: f64, h: f64) -> Loop {
        let (hw, hh) = (w / 2.0, h / 2.0);
        Loop::polygon(&[
            P2::new(center.x - hw, center.y - hh),
            P2::new(center.x + hw, center.y - hh),
            P2::new(center.x + hw, center.y + hh),
            P2::new(center.x - hw, center.y + hh),
        ])
    }

    /// A full circle as two half arcs (the kernel needs distinct end vertices).
    pub fn circle(c: P2, r: f64) -> Loop {
        let p0 = P2::new(c.x + r, c.y);
        let p1 = P2::new(c.x - r, c.y);
        Loop {
            segments: vec![
                Segment::Arc { a: p0, b: p1, via: P2::new(c.x, c.y + r) },
                Segment::Arc { a: p1, b: p0, via: P2::new(c.x, c.y - r) },
            ],
        }
    }

    /// A rectangle with rounded corners of radius `r` (r = 0 gives a plain rect).
    pub fn rounded_rect(center: P2, w: f64, h: f64, r: f64) -> Loop {
        let r = r.min(w / 2.0).min(h / 2.0).max(0.0);
        if r == 0.0 {
            return Loop::rect(center, w, h);
        }
        let (hw, hh) = (w / 2.0, h / 2.0);
        let (cx, cy) = (center.x, center.y);
        let k = std::f64::consts::FRAC_1_SQRT_2;
        // Counter-clockwise from the bottom edge.
        let mut segs = Vec::new();
        let corner = |x: f64, y: f64| P2::new(x, y);
        segs.push(Segment::Line { a: corner(cx - hw + r, cy - hh), b: corner(cx + hw - r, cy - hh) });
        segs.push(Segment::Arc { a: corner(cx + hw - r, cy - hh), b: corner(cx + hw, cy - hh + r), via: corner(cx + hw - r + r * k, cy - hh + r - r * k) });
        segs.push(Segment::Line { a: corner(cx + hw, cy - hh + r), b: corner(cx + hw, cy + hh - r) });
        segs.push(Segment::Arc { a: corner(cx + hw, cy + hh - r), b: corner(cx + hw - r, cy + hh), via: corner(cx + hw - r + r * k, cy + hh - r + r * k) });
        segs.push(Segment::Line { a: corner(cx + hw - r, cy + hh), b: corner(cx - hw + r, cy + hh) });
        segs.push(Segment::Arc { a: corner(cx - hw + r, cy + hh), b: corner(cx - hw, cy + hh - r), via: corner(cx - hw + r - r * k, cy + hh - r + r * k) });
        segs.push(Segment::Line { a: corner(cx - hw, cy + hh - r), b: corner(cx - hw, cy - hh + r) });
        segs.push(Segment::Arc { a: corner(cx - hw, cy - hh + r), b: corner(cx - hw + r, cy - hh), via: corner(cx - hw + r - r * k, cy - hh + r - r * k) });
        Loop { segments: segs }
    }

    /// A stadium slot of overall `length` (x) and `width` (y), centred at `center`.
    pub fn slot(center: P2, length: f64, width: f64) -> Loop {
        let r = width / 2.0;
        let half = (length / 2.0 - r).max(0.0);
        let (cx, cy) = (center.x, center.y);
        Loop {
            segments: vec![
                Segment::Line { a: P2::new(cx - half, cy - r), b: P2::new(cx + half, cy - r) },
                Segment::Arc { a: P2::new(cx + half, cy - r), b: P2::new(cx + half, cy + r), via: P2::new(cx + half + r, cy) },
                Segment::Line { a: P2::new(cx + half, cy + r), b: P2::new(cx - half, cy + r) },
                Segment::Arc { a: P2::new(cx - half, cy + r), b: P2::new(cx - half, cy - r), via: P2::new(cx - half - r, cy) },
            ],
        }
    }

    /// Arc length at the *start* of each segment, followed by the total: `n + 1`
    /// entries for `n` segments. The walk starts at segment 0's start point,
    /// which is the loop's seam and the only place a loop has a distinguished
    /// point — see [`crate::loft`].
    pub fn cumulative_lengths(&self) -> Vec<f64> {
        let mut cum = Vec::with_capacity(self.segments.len() + 1);
        let mut s = 0.0;
        cum.push(0.0);
        for seg in &self.segments {
            s += seg.length();
            cum.push(s);
        }
        cum
    }

    /// Total arc length of the loop, in sketch units.
    pub fn length(&self) -> f64 {
        self.segments.iter().map(Segment::length).sum()
    }

    /// Sample the loop as a polyline (arcs subdivided) for area / containment.
    pub fn sample(&self) -> Vec<P2> {
        self.sample_with(16)
    }

    /// Sample so that the chord error of every curved segment stays below
    /// `tolerance` (in sketch units). Used wherever the polygon stands in for
    /// the curve and the error matters — 2D fill-rule rebuilds, glyph unions
    /// — rather than the fixed 16 steps that are enough for containment.
    pub fn sample_tolerance(&self, tolerance: f64) -> Vec<P2> {
        let tol = tolerance.max(1e-12);
        let mut steps = 8usize;
        for s in &self.segments {
            let n = match s {
                Segment::Line { .. } => 1,
                // Chord error of a circular arc split into equal steps of
                // angle `per` is r(1 - cos(per/2)); solve for `per`.
                Segment::Arc { a, b, via } => match arc_params(*a, *b, *via) {
                    Some((_, r, t0, t1)) if r > tol => {
                        let sweep = (t1 - t0).abs();
                        let per = 2.0 * fmath::acos((1.0 - tol / r).clamp(-1.0, 1.0));
                        if per > 1e-6 { (sweep / per).ceil() as usize + 1 } else { 128 }
                    }
                    _ => 8,
                },
                // The control polygon bounds the curve, so its deviation from
                // the chord bounds the flattening error; it falls as 1/n^2.
                Segment::Bezier { a, b, ctrl } => {
                    let dev = ctrl.iter().map(|c| point_line_distance(*c, *a, *b)).fold(0.0f64, f64::max);
                    (dev / tol).sqrt().ceil() as usize
                }
            };
            steps = steps.max(n.clamp(1, 256));
        }
        self.sample_with(steps)
    }

    fn sample_with(&self, steps: usize) -> Vec<P2> {
        let mut out = Vec::new();
        for s in &self.segments {
            match s {
                Segment::Line { a, .. } => out.push(*a),
                Segment::Arc { a, b, via } => {
                    if let Some((c, r, t0, t1)) = arc_params(*a, *b, *via) {
                        let steps = steps.max(2);
                        for i in 0..steps {
                            let t = t0 + (t1 - t0) * (i as f64 / steps as f64);
                            out.push(P2::new(c.x + r * fmath::cos(t), c.y + r * fmath::sin(t)));
                        }
                    } else {
                        out.push(*a);
                    }
                }
                Segment::Bezier { a, b, ctrl } => {
                    let mut pts = vec![*a];
                    pts.extend_from_slice(ctrl);
                    pts.push(*b);
                    let steps = steps.max(2);
                    for i in 0..steps {
                        out.push(bezier_point(&pts, i as f64 / steps as f64));
                    }
                }
            }
        }
        out
    }

    /// Signed area (positive = counter-clockwise).
    pub fn signed_area(&self) -> f64 {
        let pts = self.sample();
        let n = pts.len();
        let mut a = 0.0;
        for i in 0..n {
            let p = pts[i];
            let q = pts[(i + 1) % n];
            a += p.x * q.y - q.x * p.y;
        }
        a / 2.0
    }

    pub fn reversed(&self) -> Loop {
        let segments = self
            .segments
            .iter()
            .rev()
            .map(|s| match s {
                Segment::Line { a, b } => Segment::Line { a: *b, b: *a },
                Segment::Arc { a, b, via } => Segment::Arc { a: *b, b: *a, via: *via },
                Segment::Bezier { a, b, ctrl } => {
                    Segment::Bezier { a: *b, b: *a, ctrl: ctrl.iter().rev().copied().collect() }
                }
            })
            .collect();
        Loop { segments }
    }

    /// A closed smooth loop through `points` (Catmull-Rom splines).
    pub fn spline(points: &[P2], tension: f64) -> crate::Result<Loop> {
        if points.len() < 3 {
            return Err(crate::CadpoleError::invalid("a closed spline needs at least 3 points"));
        }
        Ok(Loop { segments: catmull_rom(points, true, tension) })
    }

    pub fn contains_point(&self, p: P2) -> bool {
        let pts = self.sample();
        let n = pts.len();
        let mut inside = false;
        let mut j = n - 1;
        for i in 0..n {
            let (pi, pj) = (pts[i], pts[j]);
            if (pi.y > p.y) != (pj.y > p.y)
                && p.x < (pj.x - pi.x) * (p.y - pi.y) / (pj.y - pi.y) + pi.x
            {
                inside = !inside;
            }
            j = i;
        }
        inside
    }

    pub fn is_closed(&self, eps: f64) -> bool {
        match (self.segments.first(), self.segments.last()) {
            (Some(f), Some(l)) => {
                let (a, b) = (f.start(), l.end());
                (a.x - b.x).abs() < eps && (a.y - b.y).abs() < eps
            }
            _ => false,
        }
    }
}

/// Center, radius and start/end angles of the arc a -> b through `via`.
/// Angles are chosen so that sweeping from t0 to t1 passes through `via`.
pub fn arc_params(a: P2, b: P2, via: P2) -> Option<(P2, f64, f64, f64)> {
    let (ax, ay, bx, by, cx, cy) = (a.x, a.y, via.x, via.y, b.x, b.y);
    let d = 2.0 * (ax * (by - cy) + bx * (cy - ay) + cx * (ay - by));
    if d.abs() < 1e-12 {
        return None;
    }
    let ux = ((ax * ax + ay * ay) * (by - cy)
        + (bx * bx + by * by) * (cy - ay)
        + (cx * cx + cy * cy) * (ay - by))
        / d;
    let uy = ((ax * ax + ay * ay) * (cx - bx)
        + (bx * bx + by * by) * (ax - cx)
        + (cx * cx + cy * cy) * (bx - ax))
        / d;
    let c = P2::new(ux, uy);
    let r = (fmath::powi(ax - ux, 2) + fmath::powi(ay - uy, 2)).sqrt();
    let t0 = fmath::atan2(ay - uy, ax - ux);
    let tv = fmath::atan2(via.y - uy, via.x - ux);
    let mut t1 = fmath::atan2(b.y - uy, b.x - ux);
    // Choose the direction (ccw or cw) that passes through `via`.
    let ccw = |from: f64, to: f64| {
        let mut d = to - from;
        while d < 0.0 {
            d += std::f64::consts::TAU;
        }
        d
    };
    if ccw(t0, tv) < ccw(t0, t1) {
        // counter-clockwise reaches via before b
        t1 = t0 + ccw(t0, t1);
    } else {
        t1 = t0 - (std::f64::consts::TAU - ccw(t0, t1));
    }
    Some((c, r, t0, t1))
}

/// An open chain of segments being built into a loop (the `path()` API).
#[derive(Clone, Debug, PartialEq)]
pub struct Path {
    pub start: P2,
    pub segments: Vec<Segment>,
}

impl Path {
    pub fn new(start: P2) -> Path {
        Path { start, segments: Vec::new() }
    }
    pub fn current(&self) -> P2 {
        self.segments.last().map(|s| s.end()).unwrap_or(self.start)
    }
    pub fn line_to(&self, p: P2) -> Path {
        let mut n = self.clone();
        n.segments.push(Segment::Line { a: self.current(), b: p });
        n
    }
    /// Arc to `p` passing through `via`.
    pub fn arc_to(&self, p: P2, via: P2) -> Path {
        let mut n = self.clone();
        n.segments.push(Segment::Arc { a: self.current(), b: p, via });
        n
    }
    /// Arc to `p` that starts tangent to the previous segment's direction.
    pub fn tangent_arc_to(&self, p: P2) -> crate::Result<Path> {
        let a = self.current();
        let dir = self.end_direction().ok_or_else(|| crate::CadpoleError::invalid("tangent_arc needs a previous segment"))?;
        // Chord and its midpoint; the arc's centre lies on the perpendicular
        // to `dir` through `a` and on the chord's perpendicular bisector.
        let chord = P2::new(p.x - a.x, p.y - a.y);
        let chord_len2 = chord.x * chord.x + chord.y * chord.y;
        if chord_len2 < 1e-18 {
            return Err(crate::CadpoleError::invalid("tangent_arc: end point equals start"));
        }
        let n = P2::new(-dir.y, dir.x); // perpendicular to the tangent
        // centre = a + t*n ; |centre - p|^2 = |centre - a|^2 = t^2
        // => t = |chord|^2 / (2 * chord . n)
        let cn = chord.x * n.x + chord.y * n.y;
        if cn.abs() < 1e-12 {
            // Collinear: a straight line is the limit of the arc.
            return Ok(self.line_to(p));
        }
        let t = chord_len2 / (2.0 * cn);
        let c = P2::new(a.x + t * n.x, a.y + t * n.y);
        let r = t.abs();
        // The via point is on the arc midway between a and p, on the side of the tangent direction.
        let mid = P2::new((a.x + p.x) / 2.0, (a.y + p.y) / 2.0);
        let mut d = P2::new(mid.x - c.x, mid.y - c.y);
        let dl = (d.x * d.x + d.y * d.y).sqrt();
        if dl < 1e-12 {
            // Half circle: via is a + r*dir.
            d = P2::new(dir.x, dir.y);
        } else {
            d = P2::new(d.x / dl, d.y / dl);
        }
        let via = P2::new(c.x + r * d.x, c.y + r * d.y);
        Ok(self.arc_to(p, via))
    }
    /// Cubic (two controls) or quadratic (one control) Bézier to `p`.
    pub fn bezier_to(&self, p: P2, ctrl: Vec<P2>) -> crate::Result<Path> {
        if ctrl.is_empty() || ctrl.len() > 2 {
            return Err(crate::CadpoleError::invalid("bezier needs 1 or 2 control points"));
        }
        let mut n = self.clone();
        n.segments.push(Segment::Bezier { a: self.current(), b: p, ctrl });
        Ok(n)
    }
    /// Smooth spline from the current point through `points`.
    pub fn spline_through(&self, points: &[P2], tension: f64) -> crate::Result<Path> {
        if points.is_empty() {
            return Err(crate::CadpoleError::invalid("spline needs at least one point"));
        }
        let mut pts = vec![self.current()];
        pts.extend_from_slice(points);
        let mut n = self.clone();
        n.segments.extend(catmull_rom(&pts, false, tension));
        Ok(n)
    }
    /// Unit direction at the end of the path.
    pub fn end_direction(&self) -> Option<P2> {
        self.segments.last()?.end_direction()
    }
    /// Close the path with a straight segment back to the start (unless it is
    /// already closed) and return the loop.
    pub fn close(&self) -> crate::Result<Loop> {
        if self.segments.is_empty() {
            return Err(crate::CadpoleError::invalid("path has no segments"));
        }
        let mut segs = self.segments.clone();
        let end = self.current();
        if (end.x - self.start.x).abs() > 1e-9 || (end.y - self.start.y).abs() > 1e-9 {
            segs.push(Segment::Line { a: end, b: self.start });
        }
        if segs.len() < 2 {
            return Err(crate::CadpoleError::invalid("a closed path needs at least two segments"));
        }
        Ok(Loop { segments: segs })
    }
}

/// A sketch: a plane plus loops. Immutable; builder methods return new sketches.
///
/// Imported geometry (SVG, DXF, text) also carries two things a hand-written
/// sketch does not need: a **name per loop**, derived from the source file's
/// own identifiers so selections survive re-evaluation
/// (`import_svg_1/badge/path[3]/loop[0]`), and **construction paths** — open
/// chains that cannot bound a face but are still useful (a floor plan's
/// unjoined centre lines, a stroked SVG path). `regions()` ignores `paths`.
#[derive(Clone, Debug, PartialEq)]
pub struct Sketch {
    pub plane: Plane,
    pub loops: Vec<Loop>,
    /// Name of each loop, parallel to `loops`; empty for an unnamed loop.
    pub loop_names: Vec<String>,
    /// Open chains that bound nothing. Never extruded.
    pub paths: Vec<Path>,
    /// Name of each path, parallel to `paths`; empty for an unnamed path.
    pub path_names: Vec<String>,
}

/// A material region: one outer loop and zero or more holes.
#[derive(Clone, Debug)]
pub struct Region {
    pub outer: Loop,
    pub holes: Vec<Loop>,
}

/// A region that still knows which sketch loop each of its loops came from.
///
/// A feature that has to match loops across several sketches — a loft pairs
/// loop `l` of every section with loop `l` of the next — needs the index, not
/// just the geometry. [`Sketch::regions`] is this with the indices dropped.
#[derive(Clone, Debug)]
pub struct IndexedRegion {
    pub outer: (usize, Loop),
    pub holes: Vec<(usize, Loop)>,
}

impl Sketch {
    pub fn new(plane: Plane) -> Sketch {
        Sketch {
            plane,
            loops: Vec::new(),
            loop_names: Vec::new(),
            paths: Vec::new(),
            path_names: Vec::new(),
        }
    }

    pub fn with_loop(&self, l: Loop) -> Sketch {
        self.with_named_loop(l, String::new())
    }

    /// Add a loop under a stable name. Imports use the source file's own
    /// identifiers (`badge/path[3]/loop[0]`); an empty name means "unnamed",
    /// as for every hand-drawn loop.
    pub fn with_named_loop(&self, l: Loop, name: impl Into<String>) -> Sketch {
        let mut s = self.clone();
        s.push_named_loop(l, name);
        s
    }

    /// [`Sketch::with_named_loop`] in place. A builder adding many loops
    /// uses this: the `with_` form copies the whole sketch for each one,
    /// which is quadratic in the loop count.
    pub fn push_named_loop(&mut self, l: Loop, name: impl Into<String>) {
        self.loops.push(l);
        self.loop_names.resize(self.loops.len() - 1, String::new());
        self.loop_names.push(name.into());
    }

    /// Add a construction path. It is reported and selectable but never
    /// contributes to a region, so an open chain in an imported file does not
    /// stop the closed ones extruding.
    pub fn with_path(&self, p: Path, name: impl Into<String>) -> Sketch {
        let mut s = self.clone();
        s.push_path(p, name);
        s
    }

    /// [`Sketch::with_path`] in place, for a builder adding many paths (a
    /// floor plan's tens of thousands of loose lines).
    pub fn push_path(&mut self, p: Path, name: impl Into<String>) {
        self.paths.push(p);
        self.path_names.resize(self.paths.len() - 1, String::new());
        self.path_names.push(name.into());
    }

    /// The name of loop `i`, or "" when it has none.
    pub fn loop_name(&self, i: usize) -> &str {
        self.loop_names.get(i).map(String::as_str).unwrap_or("")
    }

    /// The name of path `i`, or "" when it has none.
    pub fn path_name(&self, i: usize) -> &str {
        self.path_names.get(i).map(String::as_str).unwrap_or("")
    }

    /// Group loops into regions by even-odd nesting depth. Outer loops are
    /// returned counter-clockwise, holes clockwise.
    pub fn regions(&self) -> crate::Result<Vec<Region>> {
        Ok(self
            .regions_indexed()?
            .into_iter()
            .map(|r| Region { outer: r.outer.1, holes: r.holes.into_iter().map(|(_, l)| l).collect() })
            .collect())
    }

    /// [`Sketch::regions`] for building faces from, which refuses by name
    /// what no face can be bounded by: a loop that crosses or touches
    /// itself (a bow-tie, a figure eight), and a hole that touches its
    /// outline or another hole of the same region. The kernel would refuse
    /// them too, but in its own terms ("cannot attach a plane to a wire that
    /// is not on one plane" for a planar bow-tie, a triangulation failure
    /// for a tangent hole), which name nothing in the script. Separate
    /// outlines that touch or cross are two regions and stay allowed.
    /// Checked on the loops' samples, so a touch is seen where the
    /// sampled boundaries meet. The fixed 16 steps per curve cut corners by
    /// up to their chord sag, so a contact found on them between curved
    /// loops is a candidate only: it is confirmed on samples within a
    /// ten-thousandth of the sketch's size of the true curves, where two
    /// curves that never meet (the two walls of a thin split ring) do not.
    pub fn face_regions(&self) -> crate::Result<Vec<Region>> {
        let indexed = self.regions_indexed()?;
        let samples: Vec<Vec<P2>> = self.loops.iter().map(|l| l.sample()).collect();
        let boxes: Vec<(P2, P2)> = samples.iter().map(|s| sample_box(s)).collect();
        let extent = boxes.iter().fold(0.0f64, |m, (lo, hi)| m.max(hi.x - lo.x).max(hi.y - lo.y));
        let eps = 1e-9 * extent.max(1.0);
        // The confirmation's tolerance: finer than any wall a face can be
        // built with, coarse enough that a curve's samples are not millions.
        let fine_tol = 1e-4 * extent.max(1.0);
        // A repeated point is a zero-length edge, which the kernel takes in
        // its stride; it is not a self-contact.
        let dedup = |pts: Vec<P2>| {
            let mut out: Vec<P2> = Vec::with_capacity(pts.len());
            for p in pts {
                if out.last().is_none_or(|q: &P2| (q.x - p.x).abs() > eps || (q.y - p.y).abs() > eps) {
                    out.push(p);
                }
            }
            while out.len() > 1 && {
                let (f, l) = (out[0], out[out.len() - 1]);
                (f.x - l.x).abs() <= eps && (f.y - l.y).abs() <= eps
            } {
                out.pop();
            }
            out
        };
        let samples: Vec<Vec<P2>> = samples.into_iter().map(dedup).collect();
        let curved = |i: usize| self.loops[i].segments.iter().any(|s| !matches!(s, Segment::Line { .. }));
        let fine = |i: usize| dedup(self.loops[i].sample_tolerance(fine_tol));
        let name = |i: usize| {
            let n = self.loop_name(i);
            if n.is_empty() { format!("loop[{i}]") } else { format!("{n:?}") }
        };
        let at = |p: P2| format!("({}, {})", crate::context::fmt_num(round_to(p.x, eps)), crate::context::fmt_num(round_to(p.y, eps)));
        for (i, pts) in samples.iter().enumerate() {
            let contact = self_contact(pts, eps).and_then(|p| if curved(i) { self_contact(&fine(i), fine_tol) } else { Some(p) });
            if let Some(p) = contact {
                return Err(crate::CadpoleError::invalid(format!(
                    "sketch {} crosses itself at {}: a face needs an outline that does not cross or touch itself; \
                     draw the two parts as separate loops",
                    name(i),
                    at(p)
                )));
            }
        }
        for r in &indexed {
            // Only members whose boxes come within `eps` of each other can
            // touch. The outline is tested against every hole; holes only
            // against holes that share a cell of a grid as fine as a typical
            // hole, so a plate of thousands of holes tests neighbours, not
            // every pair (a sweep in x alone was still all-pairs down a
            // column).
            let holes: Vec<usize> = r.holes.iter().map(|h| h.0).collect();
            let mut pairs: Vec<(usize, usize)> = holes.iter().map(|&h| (r.outer.0, h)).collect();
            let near = |a: usize, b: usize| {
                let ((alo, ahi), (blo, bhi)) = (boxes[a], boxes[b]);
                !(blo.x > ahi.x + eps || alo.x > bhi.x + eps || blo.y > ahi.y + eps || alo.y > bhi.y + eps)
            };
            if holes.len() > 1 {
                let mut sizes: Vec<f64> =
                    holes.iter().map(|&h| (boxes[h].1.x - boxes[h].0.x).max(boxes[h].1.y - boxes[h].0.y)).collect();
                sizes.sort_by(f64::total_cmp);
                let cell = sizes[sizes.len() / 2].max(eps * 1e3).max(1e-9);
                let cell_of = |v: f64| (v / cell).floor() as i64;
                let mut grid: std::collections::HashMap<(i64, i64), Vec<usize>> = std::collections::HashMap::new();
                let mut big: Vec<usize> = Vec::new();
                for &h in &holes {
                    let (lo, hi) = boxes[h];
                    let (x0, x1, y0, y1) = (cell_of(lo.x - eps), cell_of(hi.x + eps), cell_of(lo.y - eps), cell_of(hi.y + eps));
                    // A hole spanning many cells is tested against all.
                    if (x1 - x0 + 1) * (y1 - y0 + 1) > 64 {
                        big.push(h);
                        continue;
                    }
                    for x in x0..=x1 {
                        for y in y0..=y1 {
                            grid.entry((x, y)).or_default().push(h);
                        }
                    }
                }
                let mut seen: std::collections::HashSet<(usize, usize)> = std::collections::HashSet::new();
                let mut keys: Vec<&(i64, i64)> = grid.keys().collect();
                keys.sort();
                for key in keys {
                    let list = &grid[key];
                    for (i, &a) in list.iter().enumerate() {
                        for &b in &list[i + 1..] {
                            if seen.insert((a.min(b), a.max(b))) {
                                pairs.push((a.min(b), a.max(b)));
                            }
                        }
                    }
                }
                for &a in &big {
                    for &b in &holes {
                        if a != b && seen.insert((a.min(b), a.max(b))) {
                            pairs.push((a.min(b), a.max(b)));
                        }
                    }
                }
            }
            #[cfg(test)]
            PAIRS_TESTED.with(|n| n.set(n.get() + pairs.len()));
            for (a, b) in pairs {
                if !near(a, b) {
                    continue;
                }
                let contact = mutual_contact(&samples[a], &samples[b], eps).and_then(|p| {
                    if curved(a) || curved(b) { mutual_contact(&fine(a), &fine(b), fine_tol) } else { Some(p) }
                });
                if let Some(p) = contact {
                    let (hole, other) = if a == r.outer.0 { (b, a) } else { (a, b) };
                    return Err(crate::CadpoleError::invalid(format!(
                        "sketch hole {} touches {} at {}: the face between them would have no width there; \
                         a hole must lie strictly inside its outline and clear of the other holes",
                        name(hole),
                        name(other),
                        at(p)
                    )));
                }
            }
        }
        Ok(indexed
            .into_iter()
            .map(|r| Region { outer: r.outer.1, holes: r.holes.into_iter().map(|(_, l)| l).collect() })
            .collect())
    }

    /// [`Sketch::regions`], keeping the sketch loop index of every loop.
    ///
    /// Loop `i`'s parent is the smallest loop that holds *all* of it: every
    /// sampled point of `i` inside `j` or on its boundary. A single probe
    /// point per loop is not enough: two loops that cross — a plus sign
    /// drawn as two bars, a label box over a wall — can each hold the
    /// other's probe, and one or both would come out as a hole of nothing.
    /// Loops that cross are neither's parent: both are outers, drawn, and
    /// what their overlap means is the 2D booleans' business.
    pub fn regions_indexed(&self) -> crate::Result<Vec<IndexedRegion>> {
        let n = self.loops.len();
        let mut depth = vec![0usize; n];
        let mut parent = vec![None; n];
        let samples: Vec<Vec<P2>> = self.loops.iter().map(|l| l.sample()).collect();
        for i in 0..n {
            if samples[i].is_empty() {
                return Err(crate::CadpoleError::invalid("empty loop in sketch"));
            }
            if !self.loops[i].is_closed(1e-9) {
                return Err(crate::CadpoleError::invalid(format!("loop {i} is not closed")));
            }
        }
        let boxes: Vec<(P2, P2)> = samples.iter().map(|s| sample_box(s)).collect();
        // A point this close to a boundary is on it, whichever side the
        // even-odd test puts it: a hole drawn touching its outer is still
        // the outer's hole.
        let extent = boxes.iter().fold(0.0f64, |m, (lo, hi)| m.max(hi.x - lo.x).max(hi.y - lo.y));
        let eps = 1e-9 * extent.max(1.0);
        let areas: Vec<f64> = self.loops.iter().map(|l| l.signed_area().abs()).collect();
        for i in 0..n {
            let mut best: Option<(usize, f64)> = None;
            for j in 0..n {
                if i == j {
                    continue;
                }
                let (lo, hi) = boxes[j];
                let (ilo, ihi) = boxes[i];
                if ilo.x < lo.x - eps || ilo.y < lo.y - eps || ihi.x > hi.x + eps || ihi.y > hi.y + eps {
                    continue;
                }
                // Two identical loops would each hold all of the other; the
                // earlier one is the parent, so they nest rather than cycle.
                if areas[j] < areas[i] || (areas[j] == areas[i] && j > i) {
                    continue;
                }
                // Every point of `i` inside `j`, and no edge of `i` through
                // one of `j`'s: a rectangle whose corners sit in both arms of
                // an L is not inside the L.
                if !samples[i].iter().all(|p| polygon_holds(&samples[j], *p, eps)) || boundaries_cross(&samples[i], &samples[j], eps) {
                    continue;
                }
                match best {
                    Some((_, a)) if a <= areas[j] => {}
                    _ => best = Some((j, areas[j])),
                }
            }
            parent[i] = best.map(|(j, _)| j);
        }
        for i in 0..n {
            let mut d = 0;
            let mut cur = parent[i];
            while let Some(p) = cur {
                d += 1;
                cur = parent[p];
                if d > n {
                    break;
                }
            }
            depth[i] = d;
        }
        let mut regions: Vec<IndexedRegion> = Vec::new();
        for i in 0..n {
            if depth[i] % 2 == 0 {
                let mut outer = self.loops[i].clone();
                if outer.signed_area() < 0.0 {
                    outer = outer.reversed();
                }
                regions.push(IndexedRegion { outer: (i, outer), holes: Vec::new() });
            }
        }
        for i in 0..n {
            if depth[i] % 2 == 1 {
                let p = parent[i].expect("odd depth implies parent");
                let mut hole = self.loops[i].clone();
                if hole.signed_area() > 0.0 {
                    hole = hole.reversed();
                }
                if let Some(r) = regions.iter_mut().find(|r| r.outer.0 == p) {
                    r.holes.push((i, hole));
                }
            }
        }
        Ok(regions)
    }

    /// The first pair of loops that cross — each reaching inside the other
    /// without either holding the other whole — or `None`. Nesting treats
    /// such a pair as two outers, which is right for drawing them and
    /// wrong for measuring what they enclose together, so `Sketch.area()`
    /// refuses them by name rather than count the overlap twice.
    pub fn crossing_pair(&self) -> Option<(usize, usize)> {
        let samples: Vec<Vec<P2>> = self.loops.iter().map(|l| l.sample()).collect();
        let boxes: Vec<(P2, P2)> = samples.iter().map(|s| sample_box(s)).collect();
        let extent = boxes.iter().fold(0.0f64, |m, (lo, hi)| m.max(hi.x - lo.x).max(hi.y - lo.y));
        let eps = 1e-9 * extent.max(1.0);
        let within = |i: usize, j: usize| samples[i].iter().all(|p| polygon_holds(&samples[j], *p, eps));
        let reaches_into = |i: usize, j: usize| {
            samples[i].iter().any(|p| polygon_holds(&samples[j], *p, 0.0) && !polygon_holds_boundary(&samples[j], *p, eps))
        };
        for i in 0..self.loops.len() {
            for j in i + 1..self.loops.len() {
                let ((ilo, ihi), (jlo, jhi)) = (boxes[i], boxes[j]);
                if ihi.x < jlo.x || jhi.x < ilo.x || ihi.y < jlo.y || jhi.y < ilo.y {
                    continue;
                }
                if boundaries_cross(&samples[i], &samples[j], eps) {
                    return Some((i, j));
                }
                if within(i, j) || within(j, i) {
                    continue;
                }
                if reaches_into(i, j) || reaches_into(j, i) {
                    return Some((i, j));
                }
            }
        }
        None
    }

    /// Map a 2D sketch point to world space.
    pub fn to_world(&self, p: P2) -> Vec3 {
        self.plane.to_world(p.x, p.y)
    }
}

/// The bounding box of a sampled loop.
fn sample_box(pts: &[P2]) -> (P2, P2) {
    let mut lo = P2::new(f64::MAX, f64::MAX);
    let mut hi = P2::new(f64::MIN, f64::MIN);
    for p in pts {
        lo = P2::new(lo.x.min(p.x), lo.y.min(p.y));
        hi = P2::new(hi.x.max(p.x), hi.y.max(p.y));
    }
    (lo, hi)
}

/// `x` with the noise below `eps` rounded off, for a message.
fn round_to(x: f64, eps: f64) -> f64 {
    let q = eps.max(1e-12) * 1e3;
    (x / q).round() * q
}

/// Where segment `p0`–`p1` meets segment `q0`–`q1`: a crossing, or an end
/// of one within `eps` of the other. `None` when they stay clear.
fn segment_contact(p0: P2, p1: P2, q0: P2, q1: P2, eps: f64) -> Option<P2> {
    if p0.x.max(p1.x) < q0.x.min(q1.x) - eps
        || q0.x.max(q1.x) < p0.x.min(p1.x) - eps
        || p0.y.max(p1.y) < q0.y.min(q1.y) - eps
        || q0.y.max(q1.y) < p0.y.min(p1.y) - eps
    {
        return None;
    }
    let cross = |o: P2, a: P2, b: P2| (a.x - o.x) * (b.y - o.y) - (a.y - o.y) * (b.x - o.x);
    let (d1, d2) = (cross(p0, p1, q0), cross(p0, p1, q1));
    let (d3, d4) = (cross(q0, q1, p0), cross(q0, q1, p1));
    if ((d1 > 0.0 && d2 < 0.0) || (d1 < 0.0 && d2 > 0.0)) && ((d3 > 0.0 && d4 < 0.0) || (d3 < 0.0 && d4 > 0.0)) {
        let t = d3 / (d3 - d4);
        return Some(P2::new(p0.x + (p1.x - p0.x) * t, p0.y + (p1.y - p0.y) * t));
    }
    let near = |p: P2, a: P2, b: P2| {
        let (dx, dy) = (b.x - a.x, b.y - a.y);
        let len2 = dx * dx + dy * dy;
        let t = if len2 > 0.0 { (((p.x - a.x) * dx + (p.y - a.y) * dy) / len2).clamp(0.0, 1.0) } else { 0.0 };
        let (cx, cy) = (a.x + dx * t - p.x, a.y + dy * t - p.y);
        cx * cx + cy * cy <= eps * eps
    };
    [(q0, p0, p1), (q1, p0, p1), (p0, q0, q1), (p1, q0, q1)].into_iter().find(|&(p, a, b)| near(p, a, b)).map(|(p, _, _)| p)
}

/// The edges of a closed polygon, sorted by their boxes' left side, for a
/// sweep: `(index, lo, hi)`.
fn swept_edges(pts: &[P2]) -> Vec<(usize, P2, P2)> {
    let n = pts.len();
    let mut edges: Vec<(usize, P2, P2)> = (0..n)
        .map(|i| {
            let (a, b) = (pts[i], pts[(i + 1) % n]);
            (i, P2::new(a.x.min(b.x), a.y.min(b.y)), P2::new(a.x.max(b.x), a.y.max(b.y)))
        })
        .collect();
    edges.sort_by(|a, b| a.1.x.total_cmp(&b.1.x).then(a.0.cmp(&b.0)));
    edges
}

#[cfg(test)]
thread_local! {
    /// Member pairs `face_regions` put to the contact test, for a test of
    /// how many it considers.
    static PAIRS_TESTED: std::cell::Cell<usize> = const { std::cell::Cell::new(0) };
}

/// A point where the closed polygon `pts` crosses or touches itself: two
/// edges that are not neighbours meeting, or a neighbour pair that doubles
/// back along itself (a spike). `None` for a simple polygon.
fn self_contact(pts: &[P2], eps: f64) -> Option<P2> {
    let n = pts.len();
    if n < 3 {
        return None;
    }
    for i in 0..n {
        let (a, v, b) = (pts[(i + n - 1) % n], pts[i], pts[(i + 1) % n]);
        let (u, w) = (P2::new(a.x - v.x, a.y - v.y), P2::new(b.x - v.x, b.y - v.y));
        let (lu, lw) = ((u.x * u.x + u.y * u.y).sqrt(), (w.x * w.x + w.y * w.y).sqrt());
        if lu > eps && lw > eps && (u.x * w.y - u.y * w.x).abs() <= eps * lu.max(lw) && u.x * w.x + u.y * w.y > 0.0 {
            return Some(v);
        }
    }
    let edges = swept_edges(pts);
    for (k, &(i, lo, hi)) in edges.iter().enumerate() {
        for &(j, lo2, hi2) in &edges[k + 1..] {
            if lo2.x > hi.x + eps {
                break;
            }
            if lo2.y > hi.y + eps || lo.y > hi2.y + eps {
                continue;
            }
            let d = (i + n - j) % n;
            if d == 0 || d == 1 || d == n - 1 {
                continue;
            }
            if let Some(p) = segment_contact(pts[i], pts[(i + 1) % n], pts[j], pts[(j + 1) % n], eps) {
                return Some(p);
            }
        }
    }
    None
}

/// A point where the closed polygons `a` and `b` cross or touch, or `None`.
fn mutual_contact(a: &[P2], b: &[P2], eps: f64) -> Option<P2> {
    let (alo, ahi) = sample_box(a);
    let (blo, bhi) = sample_box(b);
    if ahi.x < blo.x - eps || bhi.x < alo.x - eps || ahi.y < blo.y - eps || bhi.y < alo.y - eps {
        return None;
    }
    // One sweep over both polygons' edges by their left side: each edge is
    // tested against the other polygon's edges still open at that x.
    let (ea, eb) = (swept_edges(a), swept_edges(b));
    let mut events: Vec<(bool, usize, P2, P2)> = ea.iter().map(|&(i, lo, hi)| (false, i, lo, hi)).collect();
    events.extend(eb.iter().map(|&(j, lo, hi)| (true, j, lo, hi)));
    events.sort_by(|x, y| x.2.x.total_cmp(&y.2.x).then(x.0.cmp(&y.0)).then(x.1.cmp(&y.1)));
    let mut open: [Vec<(usize, P2)>; 2] = [Vec::new(), Vec::new()];
    let edge = |in_b: bool, i: usize| {
        let poly = if in_b { b } else { a };
        (poly[i], poly[(i + 1) % poly.len()])
    };
    for (in_b, i, lo, hi) in events {
        let other = &mut open[usize::from(!in_b)];
        other.retain(|&(_, ohi)| ohi.x >= lo.x - eps);
        let (p0, p1) = edge(in_b, i);
        for &(j, _) in other.iter() {
            let (q0, q1) = edge(!in_b, j);
            if let Some(p) = segment_contact(p0, p1, q0, q1, eps) {
                return Some(p);
            }
        }
        open[usize::from(in_b)].push((i, hi));
    }
    None
}

/// Does an edge of the closed polygon `a` pass through an edge of `b`, each
/// strictly crossing the other's line? Touching — a shared vertex, an edge
/// lying along an edge — is not crossing.
fn boundaries_cross(a: &[P2], b: &[P2], eps: f64) -> bool {
    let (na, nb) = (a.len(), b.len());
    if na < 2 || nb < 2 {
        return false;
    }
    let (alo, ahi) = sample_box(a);
    let (blo, bhi) = sample_box(b);
    if ahi.x < blo.x - eps || bhi.x < alo.x - eps || ahi.y < blo.y - eps || bhi.y < alo.y - eps {
        return false;
    }
    // Signed area of (o, p, q), scaled to a distance from o–p's line.
    let side = |o: P2, p: P2, q: P2| {
        let (dx, dy) = (p.x - o.x, p.y - o.y);
        let len = (dx * dx + dy * dy).sqrt();
        if len == 0.0 {
            return 0.0;
        }
        (dx * (q.y - o.y) - dy * (q.x - o.x)) / len
    };
    let strictly_opposite = |s: f64, t: f64| (s > eps && t < -eps) || (s < -eps && t > eps);
    for i in 0..na {
        let (p0, p1) = (a[i], a[(i + 1) % na]);
        let (lo, hi) = (P2::new(p0.x.min(p1.x), p0.y.min(p1.y)), P2::new(p0.x.max(p1.x), p0.y.max(p1.y)));
        for k in 0..nb {
            let (q0, q1) = (b[k], b[(k + 1) % nb]);
            if q0.x.max(q1.x) < lo.x - eps || q0.x.min(q1.x) > hi.x + eps || q0.y.max(q1.y) < lo.y - eps || q0.y.min(q1.y) > hi.y + eps {
                continue;
            }
            if strictly_opposite(side(p0, p1, q0), side(p0, p1, q1)) && strictly_opposite(side(q0, q1, p0), side(q0, q1, p1)) {
                return true;
            }
        }
    }
    false
}

/// Is `p` within `eps` of the boundary of the closed polygon `poly`?
fn polygon_holds_boundary(poly: &[P2], p: P2, eps: f64) -> bool {
    let n = poly.len();
    let mut j = n.saturating_sub(1);
    for i in 0..n {
        let (a, b) = (poly[j], poly[i]);
        let (dx, dy) = (b.x - a.x, b.y - a.y);
        let len2 = dx * dx + dy * dy;
        let t = if len2 > 0.0 { (((p.x - a.x) * dx + (p.y - a.y) * dy) / len2).clamp(0.0, 1.0) } else { 0.0 };
        let (cx, cy) = (a.x + dx * t - p.x, a.y + dy * t - p.y);
        if cx * cx + cy * cy <= eps * eps {
            return true;
        }
        j = i;
    }
    false
}

/// Is `p` inside the closed polygon `poly` (even-odd), or within `eps` of
/// its boundary?
fn polygon_holds(poly: &[P2], p: P2, eps: f64) -> bool {
    let n = poly.len();
    if n < 3 {
        return false;
    }
    let mut inside = false;
    let mut j = n - 1;
    for i in 0..n {
        let (pi, pj) = (poly[i], poly[j]);
        if (pi.y > p.y) != (pj.y > p.y) && p.x < (pj.x - pi.x) * (p.y - pi.y) / (pj.y - pi.y) + pi.x {
            inside = !inside;
        }
        j = i;
    }
    if inside {
        return true;
    }
    // On the boundary: the distance to the nearest edge.
    let mut j = n - 1;
    for i in 0..n {
        let (a, b) = (poly[j], poly[i]);
        let (dx, dy) = (b.x - a.x, b.y - a.y);
        let len2 = dx * dx + dy * dy;
        let t = if len2 > 0.0 { (((p.x - a.x) * dx + (p.y - a.y) * dy) / len2).clamp(0.0, 1.0) } else { 0.0 };
        let (cx, cy) = (a.x + dx * t - p.x, a.y + dy * t - p.y);
        if cx * cx + cy * cy <= eps * eps {
            return true;
        }
        j = i;
    }
    false
}

#[cfg(test)]
mod tests {
    use super::*;

    /// A bow-tie is planar, but no face can be bounded by it: extrude used
    /// to fail with the kernel's "cannot attach a plane to a wire that is
    /// not on one plane", which is false. It is refused naming the loop and
    /// where it crosses itself.
    #[test]
    fn a_self_crossing_loop_is_refused_by_name_where_it_crosses() {
        let bow = Sketch::new(Plane::XY).with_loop(Loop::polygon(&[
            P2::new(0.0, 0.0),
            P2::new(10.0, 10.0),
            P2::new(10.0, 0.0),
            P2::new(0.0, 10.0),
        ]));
        let e = bow.face_regions().unwrap_err().to_string();
        assert!(e.contains("loop[0]") && e.contains("crosses itself") && e.contains("(5, 5)"), "{e}");
        let ext = crate::kernel::extrude(&bow, 2.0).map(|v| v.len()).unwrap_err().to_string();
        assert!(ext.contains("crosses itself"), "{ext}");
        // A loop pinched to touch itself at a point is refused the same way.
        let pinch = Sketch::new(Plane::XY).with_named_loop(
            Loop::polygon(&[
                P2::new(0.0, 0.0),
                P2::new(10.0, 0.0),
                P2::new(5.0, 5.0),
                P2::new(10.0, 10.0),
                P2::new(0.0, 10.0),
                P2::new(5.0, 5.0),
            ]),
            "eight",
        );
        let e = pinch.face_regions().unwrap_err().to_string();
        assert!(e.contains("eight") && e.contains("itself"), "{e}");
    }

    /// A hole tangent to its outline leaves a face whose boundary touches
    /// itself; the tessellator used to report that as an untriangulable
    /// segment. It is refused naming both loops and the point.
    #[test]
    fn a_hole_touching_its_outline_is_refused_by_name() {
        let s = Sketch::new(Plane::XY)
            .with_named_loop(Loop::rect(P2::new(0.0, 0.0), 10.0, 10.0), "plate")
            .with_named_loop(Loop::circle(P2::new(0.0, 0.0), 5.0), "bore");
        let e = s.face_regions().unwrap_err().to_string();
        assert!(e.contains("bore") && e.contains("plate") && e.contains("touches"), "{e}");
        let ext = crate::kernel::extrude(&s, 2.0).map(|v| v.len()).unwrap_err().to_string();
        assert!(ext.contains("touches"), "{ext}");
        // Clear of it, the same hole is fine, and so are separate outers
        // that touch each other (two regions, which is allowed).
        let ok = Sketch::new(Plane::XY)
            .with_loop(Loop::rect(P2::new(0.0, 0.0), 10.0, 10.0))
            .with_loop(Loop::circle(P2::new(0.0, 0.0), 4.0))
            .with_loop(Loop::rect(P2::new(10.0, 0.0), 10.0, 10.0));
        assert_eq!(ok.face_regions().unwrap().len(), 2);
        assert_eq!(crate::kernel::extrude(&ok, 2.0).unwrap().len(), 2);
    }

    /// A split ring drawn as one loop: one 348-degree arc outside, two arcs
    /// inside, a wall thinner than the chord sag of 16 steps. The curves
    /// never meet, but their 16-step samples cross; the ring was refused as
    /// crossing itself, at a point on neither curve.
    #[test]
    fn a_thin_split_ring_is_not_refused_for_its_samples() {
        for (big, wall) in [(50.0f64, 0.8f64), (10.0, 0.1)] {
            let small = big - wall;
            let (xo, xi) = ((big * big - 1.0).sqrt(), (small * small - 1.0).sqrt());
            let ring = Loop {
                segments: vec![
                    Segment::Arc { a: P2::new(xo, 1.0), b: P2::new(xo, -1.0), via: P2::new(-big, 0.0) },
                    Segment::Line { a: P2::new(xo, -1.0), b: P2::new(xi, -1.0) },
                    Segment::Arc { a: P2::new(xi, -1.0), b: P2::new(-small, 0.0), via: P2::new(0.0, -small) },
                    Segment::Arc { a: P2::new(-small, 0.0), b: P2::new(xi, 1.0), via: P2::new(0.0, small) },
                    Segment::Line { a: P2::new(xi, 1.0), b: P2::new(xo, 1.0) },
                ],
            };
            let sk = Sketch::new(Plane::XY).with_loop(ring);
            sk.face_regions().unwrap_or_else(|e| panic!("R={big}: {e}"));
            let body = crate::kernel::extrude(&sk, 2.0).unwrap_or_else(|e| panic!("R={big}: {e}"));
            assert_eq!(body.len(), 1);
        }
    }

    /// The hole checks look at neighbours, not every pair: a plate of 900
    /// holes tests a few pairs per hole, where every pair is 404,550 and a
    /// sweep in x alone was 13,500 (every hole against the rest of its
    /// column).
    #[test]
    fn hole_checks_scale_with_the_holes_not_their_pairs() {
        let n = 30;
        let size = 4.0 * n as f64;
        let mut sk = Sketch::new(Plane::XY).with_loop(Loop::rect(P2::new(size / 2.0, size / 2.0), size + 4.0, size + 4.0));
        for i in 0..n {
            for j in 0..n {
                sk = sk.with_loop(Loop::circle(P2::new(2.0 + 4.0 * i as f64, 2.0 + 4.0 * j as f64), 1.0));
            }
        }
        PAIRS_TESTED.with(|c| c.set(0));
        let r = sk.face_regions().unwrap();
        assert_eq!(r[0].holes.len(), n * n);
        let tested = PAIRS_TESTED.with(|c| c.get());
        assert!(tested <= 6 * n * n, "{tested} pairs tested for {} holes", n * n);
    }

    #[test]
    fn rect_area_is_ccw_positive() {
        let r = Loop::rect(P2::new(0.0, 0.0), 4.0, 2.0);
        assert!((r.signed_area() - 8.0).abs() < 1e-9);
        assert!(r.is_closed(1e-9));
    }

    #[test]
    fn circle_area_is_close_to_pi_r2() {
        let c = Loop::circle(P2::new(1.0, 1.0), 2.0);
        let a = c.signed_area();
        assert!((a - std::f64::consts::PI * 4.0).abs() < 0.2, "area {a}");
        assert!(c.contains_point(P2::new(1.0, 1.0)));
        assert!(!c.contains_point(P2::new(4.0, 1.0)));
    }

    #[test]
    fn nested_loops_become_holes() {
        let s = Sketch::new(Plane::XY)
            .with_loop(Loop::rect(P2::new(0.0, 0.0), 10.0, 10.0))
            .with_loop(Loop::circle(P2::new(0.0, 0.0), 2.0))
            .with_loop(Loop::rect(P2::new(20.0, 0.0), 4.0, 4.0));
        let regions = s.regions().unwrap();
        assert_eq!(regions.len(), 2);
        let with_hole = regions.iter().find(|r| !r.holes.is_empty()).unwrap();
        assert_eq!(with_hole.holes.len(), 1);
        assert!(with_hole.holes[0].signed_area() < 0.0);
        assert!(with_hole.outer.signed_area() > 0.0);
    }

    /// Two loops that cross, each holding the other's probe point, must not
    /// become mutual parents at odd depth and both vanish.
    #[test]
    fn crossing_loops_are_two_outers_not_nothing() {
        let s = Sketch::new(Plane::XY)
            .with_loop(Loop::rect(P2::new(0.0, 0.0), 10.0, 2.0))
            .with_loop(Loop::rect(P2::new(0.0, 0.0), 2.0, 10.0));
        let regions = s.regions().unwrap();
        assert_eq!(regions.len(), 2, "both bars are drawn");
        assert!(regions.iter().all(|r| r.holes.is_empty()));
        // A genuinely nested pair still nests.
        let s = Sketch::new(Plane::XY)
            .with_loop(Loop::rect(P2::new(0.0, 0.0), 10.0, 10.0))
            .with_loop(Loop::rect(P2::new(0.0, 0.0), 4.0, 4.0));
        let regions = s.regions().unwrap();
        assert_eq!(regions.len(), 1);
        assert_eq!(regions[0].holes.len(), 1);
        // Three deep: an island in the hole is an outer of its own.
        let s = Sketch::new(Plane::XY)
            .with_loop(Loop::rect(P2::new(0.0, 0.0), 10.0, 10.0))
            .with_loop(Loop::rect(P2::new(0.0, 0.0), 6.0, 6.0))
            .with_loop(Loop::circle(P2::new(0.0, 0.0), 1.5));
        let regions = s.regions().unwrap();
        assert_eq!(regions.len(), 2, "the outer with its hole, and the island");
        assert_eq!(regions.iter().map(|r| r.holes.len()).sum::<usize>(), 1);
        // A small box that crosses a big one's edge is not its hole: before,
        // its probe point fell inside the big box and it vanished as a hole
        // that was half outside.
        let s = Sketch::new(Plane::XY)
            .with_loop(Loop::rect(P2::new(0.0, 0.0), 10.0, 10.0))
            .with_loop(Loop::rect(P2::new(5.0, 0.0), 4.0, 4.0));
        let regions = s.regions().unwrap();
        assert_eq!(regions.len(), 2);
        assert!(regions.iter().all(|r| r.holes.is_empty()));
        // Crossing is a pair a measurement can refuse; nesting, touching
        // and being apart are not.
        let plus = Sketch::new(Plane::XY)
            .with_loop(Loop::rect(P2::new(0.0, 0.0), 10.0, 2.0))
            .with_loop(Loop::rect(P2::new(0.0, 0.0), 2.0, 10.0));
        assert_eq!(plus.crossing_pair(), Some((0, 1)));
        let apart = Sketch::new(Plane::XY)
            .with_loop(Loop::rect(P2::new(0.0, 0.0), 2.0, 2.0))
            .with_loop(Loop::rect(P2::new(2.0, 0.0), 2.0, 2.0))
            .with_loop(Loop::rect(P2::new(20.0, 0.0), 2.0, 2.0));
        assert_eq!(apart.crossing_pair(), None, "side by side, touching along x = 1, is not crossing");
        let nested = Sketch::new(Plane::XY)
            .with_loop(Loop::rect(P2::new(0.0, 0.0), 10.0, 10.0))
            .with_loop(Loop::rect(P2::new(0.0, 0.0), 4.0, 4.0));
        assert_eq!(nested.crossing_pair(), None);
        // A rectangle with its corners in both arms of an L crosses the L's
        // inner corner: two outers, and a crossing pair.
        let l_shape = Loop::polygon(&[
            P2::new(0.0, 0.0),
            P2::new(10.0, 0.0),
            P2::new(10.0, 2.0),
            P2::new(2.0, 2.0),
            P2::new(2.0, 10.0),
            P2::new(0.0, 10.0),
        ]);
        let bar = Loop::polygon(&[P2::new(1.0, 1.0), P2::new(9.0, 1.0), P2::new(9.0, 1.5), P2::new(1.5, 9.0), P2::new(1.0, 9.0)]);
        let s = Sketch::new(Plane::XY).with_loop(l_shape).with_loop(bar);
        assert_eq!(s.regions().unwrap().len(), 2, "not a hole of the L");
        assert_eq!(s.crossing_pair(), Some((0, 1)));
        // A hole touching its outer's edge is still a hole.
        let s = Sketch::new(Plane::XY)
            .with_loop(Loop::rect(P2::new(0.0, 0.0), 10.0, 10.0))
            .with_loop(Loop::rect(P2::new(3.0, 0.0), 4.0, 4.0));
        let regions = s.regions().unwrap();
        assert_eq!(regions.len(), 1);
        assert_eq!(regions[0].holes.len(), 1);
    }

    #[test]
    fn rounded_rect_and_slot_are_closed_ccw_loops() {
        let rr = Loop::rounded_rect(P2::new(0.0, 0.0), 10.0, 6.0, 1.0);
        assert!(rr.is_closed(1e-9));
        let a = rr.signed_area();
        // area = w*h - (4 - pi) r^2
        assert!((a - (60.0 - (4.0 - std::f64::consts::PI))).abs() < 0.05, "{a}");
        let sl = Loop::slot(P2::new(0.0, 0.0), 10.0, 4.0);
        assert!(sl.is_closed(1e-9));
        let a = sl.signed_area();
        assert!((a - (6.0 * 4.0 + std::f64::consts::PI * 4.0)).abs() < 0.1, "{a}");
    }

    #[test]
    fn path_builder_closes_and_tangent_arcs_are_smooth() {
        let l = Path::new(P2::new(0.0, 0.0)).line_to(P2::new(10.0, 0.0)).line_to(P2::new(10.0, 5.0)).close().unwrap();
        assert_eq!(l.segments.len(), 3);
        assert!(l.is_closed(1e-9));
        // A line heading +x then a tangent arc to (12, 2): centre (10, 2), radius 2.
        let p = Path::new(P2::new(0.0, 0.0)).line_to(P2::new(10.0, 0.0)).tangent_arc_to(P2::new(12.0, 2.0)).unwrap();
        let Segment::Arc { via, .. } = p.segments[1] else { panic!() };
        let (c, r, _, _) = arc_params(P2::new(10.0, 0.0), P2::new(12.0, 2.0), via).unwrap();
        assert!((c.x - 10.0).abs() < 1e-9 && (c.y - 2.0).abs() < 1e-9, "{c:?}");
        assert!((r - 2.0).abs() < 1e-9);
        let d = p.end_direction().unwrap();
        assert!((d.x).abs() < 1e-9 && (d.y - 1.0).abs() < 1e-9, "end direction {d:?}");
    }

    #[test]
    fn splines_are_closed_smooth_loops() {
        let pts = [P2::new(10.0, 0.0), P2::new(0.0, 8.0), P2::new(-10.0, 0.0), P2::new(0.0, -8.0)];
        let l = Loop::spline(&pts, 0.5).unwrap();
        assert_eq!(l.segments.len(), 4);
        assert!(l.is_closed(1e-9));
        assert!(l.signed_area() > 150.0, "{}", l.signed_area());
        // Tangent continuity at joints: end dir of segment i == start dir of i+1.
        for i in 0..4 {
            let a = l.segments[i].end_direction().unwrap();
            let b = l.segments[(i + 1) % 4].start_direction().unwrap();
            assert!((a.x - b.x).abs() < 1e-9 && (a.y - b.y).abs() < 1e-9, "joint {i}: {a:?} vs {b:?}");
        }
        let p = Path::new(P2::new(0.0, 0.0)).spline_through(&[P2::new(5.0, 5.0), P2::new(10.0, 0.0)], 0.5).unwrap();
        assert_eq!(p.segments.len(), 2);
        assert!(p.bezier_to(P2::new(0.0, 0.0), vec![]).is_err());
    }

    #[test]
    fn arc_through_via_goes_the_right_way() {
        // Upper half circle from (1,0) to (-1,0) via (0,1).
        let (c, r, t0, t1) = arc_params(P2::new(1.0, 0.0), P2::new(-1.0, 0.0), P2::new(0.0, 1.0))
            .unwrap();
        assert!(c.x.abs() < 1e-9 && c.y.abs() < 1e-9);
        assert!((r - 1.0).abs() < 1e-9);
        assert!(t1 > t0, "should sweep counter-clockwise");
        assert!(((t1 - t0) - std::f64::consts::PI).abs() < 1e-9);
    }
}
