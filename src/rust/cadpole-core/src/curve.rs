//! 3D curves: an open or closed chain of segments in world space.
//!
//! A [`Curve`] is what `curve(start)` builds in a script: the same three
//! segment kinds as a sketch [`Path`] — lines, three-point circular arcs and
//! Béziers, with `spline()` producing one cubic Bézier per span — but every
//! point is a world `(x, y, z)`, so the chain need not lie in any plane. It is
//! the section type a `loft` accepts alongside sketches, and the type a
//! `sweep_along` spine will be: one curve value in the language, not two.
//!
//! The arc-length machinery is [`crate::sketch`]'s, made generic over the
//! point type: a Bézier's length is the same fixed Gauss–Legendre sum, its
//! inverse the same 60-step bisection, and [`Segment3::sub`] is the same exact
//! splitter — a line piece is a collinear line, an arc piece an arc of the same
//! circle, a Bézier piece a de Casteljau subdivision. That is what lets
//! `loft(match="arclen")` and the ruled cap cut a 3D section without ever
//! replacing a curve by a chord.
//!
//! The two bridges from the 2D world are [`Curve::from_path`] (`Path.on(plane)`)
//! and [`Curve::from_loop`] (`Sketch.curves()`): a point `(u, v)` becomes
//! `plane.to_world(u, v)`, which is the very mapping the kernel applies when
//! it builds a sketch's wire, so a lifted path is the sketch's geometry to the
//! bit.

use crate::geom::{Plane, Vec3};
use crate::sketch::{
    bezier_hull, bezier_length_to, bezier_param_at_length, bezier_point, bezier_sub, catmull_rom_controls,
    Coord, Loop, Path, Segment, P2,
};
use crate::{CadpoleError, Result};
use crate::fmath;

/// How far the control points of a curve may leave their best-fit plane, in
/// mm, for the curve to count as planar. This is truck's own tolerance
/// (`truck_base::tolerance::TOLERANCE`), and it has to be: `cap="plane"`
/// hands the wire to `builder::try_attach_plane`, which refuses anything
/// further out than this, so [`Curve::plane`] must say "planar" for exactly
/// the curves the kernel will cap with a plane.
pub const PLANAR_TOL: f64 = 1e-6;

/// Two points closer than this, in mm, are the same point: a `close()` that
/// finds the end here adds no segment, and a `line()` to here is refused.
const SAME_POINT: f64 = 1e-9;

/// Tolerance for "this arc length is a segment boundary", in mm.
const BOUNDARY_EPS: f64 = 1e-9;

/// One segment of a [`Curve`], in world coordinates: the 3D twin of
/// [`crate::sketch::Segment`].
#[derive(Clone, Debug, PartialEq)]
pub enum Segment3 {
    Line { a: Vec3, b: Vec3 },
    /// Circular arc from `a` to `b` passing through `via`.
    Arc { a: Vec3, b: Vec3, via: Vec3 },
    /// Bézier curve from `a` to `b` with interior control points (1 = quadratic, 2 = cubic).
    Bezier { a: Vec3, b: Vec3, ctrl: Vec<Vec3> },
    /// A helix, stored analytically: from `a`, about the axis through
    /// `centre` along the unit vector `axis`, turning by `angle` (signed
    /// radians; positive is counter-clockwise about `axis`, the right-hand
    /// rule) while advancing `advance` along the axis (signed) and tapering
    /// from `|a − centre|` at the start to `r_end` at the end. A helix is not
    /// a rational curve, so it has no exact NURBS form; a segment that must
    /// become a kernel edge is approximated to a fixed tolerance
    /// ([`HELIX_EDGE_TOL`]), and a sweep along one never needs the edge.
    Helix { a: Vec3, centre: Vec3, axis: Vec3, r_end: f64, angle: f64, advance: f64 },
}

/// How far the cubic B-spline standing in for a helix *edge* may leave the
/// true helix, in mm. Only a helix that becomes an edge of a solid — a loft
/// section, a cap — is ever approximated; `sweep_along` reads the helix
/// itself and its solid has no edge along the path. A millidegree of a
/// micron would be pointless; a thousandth of a millimetre is a hundredth
/// of the coarsest tessellation and well under any print's resolution.
pub const HELIX_EDGE_TOL: f64 = 1e-3;

/// A helix in closed form: `centre + axis·(advance·t) + r(t)·(cos(angle·t)·u
/// + sin(angle·t)·w)` for `t` in `[0, 1]`, with `r(t)` linear from `r0` to
/// `r1`, `u` the unit vector from the axis to the start and `w = axis × u`.
#[derive(Clone, Copy, Debug)]
pub struct Helix3 {
    pub centre: Vec3,
    pub axis: Vec3,
    pub u: Vec3,
    pub w: Vec3,
    pub r0: f64,
    pub r1: f64,
    pub angle: f64,
    pub advance: f64,
}

impl Helix3 {
    /// The point at parameter `t` in 0..1.
    pub fn point(&self, t: f64) -> Vec3 {
        let (s, c) = fmath::sin_cos(self.angle * t);
        let r = self.r0 + (self.r1 - self.r0) * t;
        self.centre + self.axis * (self.advance * t) + self.u * (r * c) + self.w * (r * s)
    }

    /// First and second derivatives with respect to `t`.
    pub fn derivatives(&self, t: f64) -> (Vec3, Vec3) {
        let (s, c) = fmath::sin_cos(self.angle * t);
        let r = self.r0 + (self.r1 - self.r0) * t;
        let dr = self.r1 - self.r0;
        let radial = self.u * c + self.w * s;
        let around = self.u * (-s) + self.w * c;
        let d1 = self.axis * self.advance + radial * dr + around * (r * self.angle);
        let d2 = around * (2.0 * dr * self.angle) - radial * (r * self.angle * self.angle);
        (d1, d2)
    }

    /// Unit tangent at `t`.
    pub fn tangent(&self, t: f64) -> Vec3 {
        self.derivatives(t).0.normalized()
    }

    /// Arc length from `t = 0` to `t_end`: closed form for a cylindrical
    /// helix (constant speed), the same fixed Gauss–Legendre rule a Bézier
    /// uses for a tapered one — deterministic to the bit either way.
    pub fn length_to(&self, t_end: f64) -> f64 {
        let t_end = t_end.clamp(0.0, 1.0);
        if t_end <= 0.0 {
            return 0.0;
        }
        if self.r0 == self.r1 {
            let around = self.r0 * self.angle;
            return t_end * fmath::hypot(around, self.advance);
        }
        // Composite rule: one sub-interval per quarter turn at least, so a
        // long tapered helix is integrated as finely as a short one.
        let steps = ((self.angle.abs() / std::f64::consts::FRAC_PI_2).ceil() as usize).clamp(4, 4096) * 2;
        let step = t_end / steps as f64;
        let mut total = 0.0;
        for k in 0..steps {
            let (lo, hi) = (k as f64 * step, (k + 1) as f64 * step);
            let (mid, half) = ((lo + hi) / 2.0, (hi - lo) / 2.0);
            for (x, wgt) in crate::sketch::GL5 {
                total += wgt * self.derivatives(mid + half * x).0.length() * half;
            }
        }
        total
    }

    /// The parameter at arc length `s` from the start: exact for a
    /// cylindrical helix, a fixed 60-step bisection for a tapered one.
    pub fn param_at_length(&self, s: f64) -> f64 {
        let total = self.length_to(1.0);
        if total <= 0.0 {
            return 0.0;
        }
        if self.r0 == self.r1 {
            return (s / total).clamp(0.0, 1.0);
        }
        let s = s.clamp(0.0, total);
        let (mut lo, mut hi) = (0.0, 1.0);
        for _ in 0..60 {
            let mid = (lo + hi) / 2.0;
            if self.length_to(mid) < s {
                lo = mid;
            } else {
                hi = mid;
            }
        }
        (lo + hi) / 2.0
    }

    /// Radius of curvature of a cylindrical helix, `(r² + c²) / r` with
    /// `c` the advance per radian; `None` for a tapered one (not constant).
    pub fn curvature_radius(&self) -> Option<f64> {
        if self.r0 != self.r1 || self.angle == 0.0 || self.r0 <= 0.0 {
            return None;
        }
        let c = self.advance / self.angle;
        Some((self.r0 * self.r0 + c * c) / self.r0)
    }
}

/// The closed form of a helix segment. `None` when the start is on the axis
/// (no radial direction) or the axis has no length.
pub fn helix_params(a: Vec3, centre: Vec3, axis: Vec3, r_end: f64, angle: f64, advance: f64) -> Option<Helix3> {
    let axis = unit(axis)?;
    let radial = a - centre;
    let radial = radial - axis * radial.dot(axis);
    let r0 = radial.length();
    if !(r0 > 0.0) || !r0.is_finite() {
        return None;
    }
    let u = radial * (1.0 / r0);
    let w = axis.cross(u);
    Some(Helix3 { centre, axis, u, w, r0, r1: r_end, angle, advance })
}

/// The kernel's stand-in for a helix edge: a clamped cubic B-spline through
/// points of the helix with the helix's own tangents at each (piecewise
/// Hermite, so it is C¹), with enough spans that it sits within
/// [`HELIX_EDGE_TOL`] of the true curve. Cubic Hermite interpolation of a
/// circle of radius `r` at a step of `θ` radians errs by at most
/// `r θ⁴ / 384`, which sets the span count; a taper or an advance only
/// makes the curve straighter.
pub fn helix_edge_curve(h: &Helix3) -> truck_modeling::Curve {
    use truck_modeling::{BSplineCurve, KnotVec, Point3};
    let r = h.r0.max(h.r1).max(1e-9);
    let step = fmath::powf(384.0 * HELIX_EDGE_TOL / r, 0.25).min(std::f64::consts::FRAC_PI_4);
    let spans = ((h.angle.abs() / step).ceil() as usize).clamp(1, 4096);
    let mut knots: Vec<f64> = vec![0.0; 4];
    let mut points: Vec<Point3> = Vec::with_capacity(3 * spans + 1);
    for k in 0..spans {
        let (t0, t1) = (k as f64 / spans as f64, (k + 1) as f64 / spans as f64);
        let (p0, p1) = (h.point(t0), h.point(t1));
        let (d0, d1) = (h.derivatives(t0).0, h.derivatives(t1).0);
        let hstep = (t1 - t0) / 3.0;
        if k == 0 {
            points.push(p0.point());
        }
        points.push((p0 + d0 * hstep).point());
        points.push((p1 - d1 * hstep).point());
        points.push(p1.point());
        if k + 1 < spans {
            knots.extend([t1, t1, t1]);
        }
    }
    knots.extend([1.0; 4]);
    truck_modeling::Curve::BSplineCurve(BSplineCurve::new(KnotVec::from(knots), points))
}

/// A circular arc in space: `centre + radius (cos t · u + sin t · w)` for `t`
/// from 0 to `sweep`, which is signed — positive turns from `u` towards `w`.
#[derive(Clone, Copy, Debug)]
pub struct Arc3 {
    pub centre: Vec3,
    pub radius: f64,
    /// Unit vector from the centre to the arc's start.
    pub u: Vec3,
    /// Unit vector in the arc's plane, perpendicular to `u`, on the `via` side.
    pub w: Vec3,
    /// Signed angle swept from the start to the end, radians.
    pub sweep: f64,
}

impl Arc3 {
    /// The unit normal of the arc's plane, `u × w`.
    pub fn normal(&self) -> Vec3 {
        self.u.cross(self.w)
    }

    /// The point a fraction `f` of the way along the arc.
    pub fn point(&self, f: f64) -> Vec3 {
        let t = self.sweep * f;
        self.centre + self.u * (self.radius * fmath::cos(t)) + self.w * (self.radius * fmath::sin(t))
    }

    /// Unit tangent a fraction `f` of the way along the arc.
    pub fn tangent(&self, f: f64) -> Vec3 {
        let t = self.sweep * f;
        (self.u * (-fmath::sin(t)) + self.w * fmath::cos(t)) * self.sweep.signum()
    }
}

/// Centre, radius, frame and signed sweep of the arc `a -> b` through `via`,
/// or `None` when the three points are collinear or two coincide. The frame is
/// chosen so `via` sits at a positive angle, and the sweep's sign then says
/// whether `b` is reached turning through `via` (positive) or the long way
/// round (negative) — the same rule as the 2D [`crate::sketch::arc_params`].
pub fn arc3_params(a: Vec3, b: Vec3, via: Vec3) -> Option<Arc3> {
    let (p, q) = (a - via, b - via);
    let pxq = p.cross(q);
    if !(pxq.length() > 1e-9 * p.length() * q.length()) {
        return None;
    }
    let centre = via + (q * p.dot(p) - p * q.dot(q)).cross(pxq) * (1.0 / (2.0 * pxq.dot(pxq)));
    let radius = (a - centre).length();
    if !(radius > 0.0) || !radius.is_finite() {
        return None;
    }
    let u = (a - centre) * (1.0 / radius);
    let n = (a - centre).cross(via - centre).normalized();
    let w = n.cross(u);
    let angle = |p: Vec3| {
        let d = p - centre;
        fmath::atan2(d.dot(w), d.dot(u))
    };
    let t_via = angle(via);
    let mut t_b = angle(b);
    if t_b < 0.0 {
        t_b += std::f64::consts::TAU;
    }
    // `via` is at a positive angle by construction of `w`; counter-clockwise
    // reaches it before `b` exactly when `b` is further round.
    let sweep = if t_b > t_via { t_b } else { t_b - std::f64::consts::TAU };
    Some(Arc3 { centre, radius, u, w, sweep })
}

impl Segment3 {
    pub fn start(&self) -> Vec3 {
        match self {
            Segment3::Line { a, .. } | Segment3::Arc { a, .. } | Segment3::Bezier { a, .. } | Segment3::Helix { a, .. } => *a,
        }
    }

    pub fn end(&self) -> Vec3 {
        match self {
            Segment3::Line { b, .. } | Segment3::Arc { b, .. } | Segment3::Bezier { b, .. } => *b,
            Segment3::Helix { .. } => self.point_at_param(1.0),
        }
    }

    /// `"line"`, `"arc"`, `"bezier"` or `"helix"`.
    pub fn kind(&self) -> &'static str {
        match self {
            Segment3::Line { .. } => "line",
            Segment3::Arc { .. } => "arc",
            Segment3::Bezier { .. } => "bezier",
            Segment3::Helix { .. } => "helix",
        }
    }

    /// The helix in closed form, for a helix segment; `None` for the rest
    /// (and for a helix whose start sits on its axis).
    pub fn helix(&self) -> Option<Helix3> {
        match self {
            Segment3::Helix { a, centre, axis, r_end, angle, advance } => {
                helix_params(*a, *centre, *axis, *r_end, *angle, *advance)
            }
            _ => None,
        }
    }

    /// The points that define the segment, in order: both ends, and the `via`
    /// or the Bézier controls between them. A segment lies in every plane its
    /// control points lie in, which is how planarity is decided. A helix has
    /// no finite control polygon; it is represented by points on it, eight
    /// per turn, which decide planarity the same way (only a helix with no
    /// advance lies in a plane).
    pub fn control_points(&self) -> Vec<Vec3> {
        match self {
            Segment3::Line { a, b } => vec![*a, *b],
            Segment3::Arc { a, b, via } => vec![*a, *via, *b],
            Segment3::Bezier { a, b, ctrl } => bezier_hull(*a, *b, ctrl),
            Segment3::Helix { a, angle, .. } => match self.helix() {
                Some(h) => {
                    let n = ((angle.abs() / std::f64::consts::FRAC_PI_4).ceil() as usize).clamp(2, 4096);
                    (0..=n).map(|i| h.point(i as f64 / n as f64)).collect()
                }
                None => vec![*a],
            },
        }
    }

    /// Unit direction leaving the start of the segment.
    pub fn start_direction(&self) -> Option<Vec3> {
        match self {
            Segment3::Line { a, b } => unit(*b - *a),
            Segment3::Arc { a, b, via } => match arc3_params(*a, *b, *via) {
                Some(arc) => Some(arc.tangent(0.0)),
                None => unit(*b - *a),
            },
            Segment3::Bezier { a, b, ctrl } => unit(ctrl.first().copied().unwrap_or(*b) - *a),
            Segment3::Helix { .. } => self.helix().and_then(|h| unit(h.derivatives(0.0).0)),
        }
    }

    /// Unit direction arriving at the end of the segment.
    pub fn end_direction(&self) -> Option<Vec3> {
        match self {
            Segment3::Line { a, b } => unit(*b - *a),
            Segment3::Arc { a, b, via } => match arc3_params(*a, *b, *via) {
                Some(arc) => Some(arc.tangent(1.0)),
                None => unit(*b - *a),
            },
            Segment3::Bezier { a, b, ctrl } => unit(*b - ctrl.last().copied().unwrap_or(*a)),
            Segment3::Helix { .. } => self.helix().and_then(|h| unit(h.derivatives(1.0).0)),
        }
    }

    /// First and second derivatives with respect to the segment's own
    /// parameter `t` in 0..1 (the one [`Segment3::point_at_param`] uses):
    /// what a moving frame is built from. A line's second derivative
    /// is zero; an arc's and a helix's are the closed forms; a Bézier's are
    /// its hodographs.
    pub fn derivatives(&self, t: f64) -> (Vec3, Vec3) {
        match self {
            Segment3::Line { a, b } => (*b - *a, Vec3::ZERO),
            Segment3::Arc { a, b, via } => match arc3_params(*a, *b, *via) {
                Some(arc) => {
                    let th = arc.sweep * t;
                    let (s, c) = fmath::sin_cos(th);
                    let radial = arc.u * c + arc.w * s;
                    let around = arc.u * (-s) + arc.w * c;
                    (around * (arc.radius * arc.sweep), radial * (-arc.radius * arc.sweep * arc.sweep))
                }
                None => (*b - *a, Vec3::ZERO),
            },
            Segment3::Bezier { a, b, ctrl } => {
                let hull = bezier_hull(*a, *b, ctrl);
                let d1 = crate::sketch::bezier_derivative(&hull);
                let d2 = crate::sketch::bezier_derivative(&d1);
                (bezier_point(&d1, t), bezier_point(&d2, t))
            }
            Segment3::Helix { a, .. } => match self.helix() {
                Some(h) => h.derivatives(t),
                None => (self.end() - *a, Vec3::ZERO),
            },
        }
    }

    /// Arc length, in mm. Exact for lines, arcs and cylindrical helices; a
    /// Bézier and a tapered helix are the same fixed quadrature the 2D
    /// segment uses, so they are deterministic to the bit.
    pub fn length(&self) -> f64 {
        match self {
            Segment3::Line { a, b } => a.dist(*b),
            Segment3::Arc { a, b, via } => match arc3_params(*a, *b, *via) {
                Some(arc) => arc.radius * arc.sweep.abs(),
                // A degenerate "arc" (three collinear points) is its chord.
                None => a.dist(*b),
            },
            Segment3::Bezier { a, b, ctrl } => bezier_length_to(&bezier_hull(*a, *b, ctrl), 1.0),
            Segment3::Helix { a, .. } => match self.helix() {
                Some(h) => h.length_to(1.0),
                None => a.dist(self.end()),
            },
        }
    }

    /// The point a distance `s` along the segment from its start (`s` is
    /// clamped to the segment).
    pub fn point_at_length(&self, s: f64) -> Vec3 {
        let len = self.length();
        let u = if len > 0.0 { (s / len).clamp(0.0, 1.0) } else { 0.0 };
        match self {
            Segment3::Line { a, b } => a.lerp(*b, u),
            Segment3::Arc { a, b, via } => match arc3_params(*a, *b, *via) {
                Some(arc) => arc.point(u),
                None => a.lerp(*b, u),
            },
            Segment3::Bezier { a, b, ctrl } => {
                let hull = bezier_hull(*a, *b, ctrl);
                bezier_point(&hull, bezier_param_at_length(&hull, s))
            }
            Segment3::Helix { .. } => self.point_at_param(self.param_at_length(s)),
        }
    }

    /// The parameter at arc length `s` from the start: `s / length` for the
    /// constant-speed kinds (line, arc, cylindrical helix), the inverse of
    /// the quadrature for a Bézier or a tapered helix.
    pub fn param_at_length(&self, s: f64) -> f64 {
        match self {
            Segment3::Bezier { a, b, ctrl } => bezier_param_at_length(&bezier_hull(*a, *b, ctrl), s),
            Segment3::Helix { .. } => self.helix().map(|h| h.param_at_length(s)).unwrap_or(0.0),
            _ => {
                let len = self.length();
                if len > 0.0 { (s / len).clamp(0.0, 1.0) } else { 0.0 }
            }
        }
    }

    /// The point at *parameter* `t` in 0..1: linear along a line, by angle
    /// along an arc or a helix, de Casteljau along a Bézier. This is the
    /// parametrisation the kernel's ruled surface pairs points by, so it is
    /// what the namer plants its probes with.
    pub fn point_at_param(&self, t: f64) -> Vec3 {
        match self {
            Segment3::Line { a, b } => a.lerp(*b, t),
            Segment3::Arc { a, b, via } => match arc3_params(*a, *b, *via) {
                Some(arc) => arc.point(t),
                None => a.lerp(*b, t),
            },
            Segment3::Bezier { a, b, ctrl } => bezier_point(&bezier_hull(*a, *b, ctrl), t),
            Segment3::Helix { a, .. } => match self.helix() {
                Some(h) => h.point(t),
                None => *a,
            },
        }
    }

    /// The piece between arc lengths `s0` and `s1`, as the same kind of curve
    /// on the same geometry — an exact split, in space. `None`
    /// when the piece would be empty or the lengths reversed.
    pub fn sub(&self, s0: f64, s1: f64) -> Option<Segment3> {
        let len = self.length();
        if s1 <= s0 || len <= 0.0 {
            return None;
        }
        let (s0, s1) = (s0.clamp(0.0, len), s1.clamp(0.0, len));
        if s1 <= s0 {
            return None;
        }
        if s0 <= 0.0 && s1 >= len {
            return Some(self.clone());
        }
        Some(match self {
            Segment3::Line { .. } => Segment3::Line { a: self.point_at_length(s0), b: self.point_at_length(s1) },
            Segment3::Arc { a, b, via } => match arc3_params(*a, *b, *via) {
                Some(_) => Segment3::Arc {
                    a: self.point_at_length(s0),
                    b: self.point_at_length(s1),
                    via: self.point_at_length((s0 + s1) / 2.0),
                },
                None => Segment3::Line { a: self.point_at_length(s0), b: self.point_at_length(s1) },
            },
            Segment3::Bezier { a, b, ctrl } => {
                let hull = bezier_hull(*a, *b, ctrl);
                let t0 = bezier_param_at_length(&hull, s0);
                let t1 = bezier_param_at_length(&hull, s1);
                let piece = bezier_sub(&hull, t0, t1);
                Segment3::Bezier { a: piece[0], b: *piece.last().unwrap(), ctrl: piece[1..piece.len() - 1].to_vec() }
            }
            // A piece of a helix is a helix of the same axis: the start moves
            // along it, the centre slides up the axis with it, and the turn,
            // the advance and the taper are the piece's share.
            Segment3::Helix { centre, axis, angle, advance, .. } => match self.helix() {
                Some(h) => {
                    let (t0, t1) = (h.param_at_length(s0), h.param_at_length(s1));
                    let r = |t: f64| h.r0 + (h.r1 - h.r0) * t;
                    Segment3::Helix {
                        a: h.point(t0),
                        centre: *centre + *axis * (advance * t0),
                        axis: *axis,
                        r_end: r(t1),
                        angle: angle * (t1 - t0),
                        advance: advance * (t1 - t0),
                    }
                }
                None => Segment3::Line { a: self.point_at_length(s0), b: self.point_at_length(s1) },
            },
        })
    }

    /// The same segment traversed the other way.
    pub fn reversed(&self) -> Segment3 {
        match self {
            Segment3::Line { a, b } => Segment3::Line { a: *b, b: *a },
            Segment3::Arc { a, b, via } => Segment3::Arc { a: *b, b: *a, via: *via },
            Segment3::Bezier { a, b, ctrl } => {
                Segment3::Bezier { a: *b, b: *a, ctrl: ctrl.iter().rev().copied().collect() }
            }
            // Run backwards, a helix turns the other way and advances the
            // other way about the same axis, from the end's centre.
            Segment3::Helix { a, centre, axis, angle, advance, .. } => {
                let r0 = match self.helix() {
                    Some(h) => h.r0,
                    None => (*a - *centre).length(),
                };
                Segment3::Helix {
                    a: self.end(),
                    centre: *centre + *axis * *advance,
                    axis: *axis,
                    r_end: r0,
                    angle: -angle,
                    advance: -advance,
                }
            }
        }
    }

    /// A 2D segment lifted onto `plane`.
    pub fn from_2d(seg: &Segment, plane: &Plane) -> Segment3 {
        let to3 = |p: P2| plane.to_world(p.x, p.y);
        match seg {
            Segment::Line { a, b } => Segment3::Line { a: to3(*a), b: to3(*b) },
            Segment::Arc { a, b, via } => Segment3::Arc { a: to3(*a), b: to3(*b), via: to3(*via) },
            Segment::Bezier { a, b, ctrl } => {
                Segment3::Bezier { a: to3(*a), b: to3(*b), ctrl: ctrl.iter().map(|c| to3(*c)).collect() }
            }
        }
    }
}

fn unit(v: Vec3) -> Option<Vec3> {
    let l = v.length();
    if l == 0.0 { None } else { Some(v * (1.0 / l)) }
}

/// A right-handed pair `(u, w)` perpendicular to the unit vector `axis`: `u`
/// is the world axis least aligned with it (ties X before Y before Z),
/// orthogonalised, and `w = axis × u`. The same rule as
/// [`crate::naming::canonical_frame`], without its sign normalisation, so a
/// helix about `-Z` starts where one about `+Z` does and turns the other way.
pub fn perpendicular_frame(axis: Vec3) -> (Vec3, Vec3) {
    let axes = [Vec3::X, Vec3::Y, Vec3::Z];
    let mut best = 0usize;
    for (i, a) in axes.iter().enumerate() {
        if axis.dot(*a).abs() < axis.dot(axes[best]).abs() {
            best = i;
        }
    }
    let a = axes[best];
    let u = (a - axis * a.dot(axis)).normalized();
    (u, axis.cross(u))
}

/// A chain of [`Segment3`]s in world space, open until [`Curve::close`]
/// closes it. Immutable: every builder method returns a new curve.
#[derive(Clone, Debug, PartialEq)]
pub struct Curve {
    pub start: Vec3,
    pub segments: Vec<Segment3>,
    pub closed: bool,
}

impl Curve {
    pub fn new(start: Vec3) -> Curve {
        Curve { start, segments: Vec::new(), closed: false }
    }

    /// Where the next segment starts: the end of the last one, or `start`.
    pub fn current(&self) -> Vec3 {
        self.segments.last().map(|s| s.end()).unwrap_or(self.start)
    }

    /// The end of the curve — the seam again, once it is closed.
    pub fn end(&self) -> Vec3 {
        self.current()
    }

    fn open_for(&self, what: &str) -> Result<()> {
        if self.closed {
            return Err(CadpoleError::invalid(format!(
                "{what}: the curve is closed; nothing can be added after close(). Start a new curve(), \
                 or add the segment before closing"
            )));
        }
        Ok(())
    }

    /// Straight segment to `p`.
    pub fn line_to(&self, p: Vec3) -> Result<Curve> {
        self.open_for("line")?;
        let c = self.current();
        if c.dist(p) <= SAME_POINT {
            return Err(CadpoleError::invalid(format!(
                "line: ({}, {}, {}) is where the curve already is, so the segment would have no length",
                p.x, p.y, p.z
            )));
        }
        let mut n = self.clone();
        n.segments.push(Segment3::Line { a: c, b: p });
        Ok(n)
    }

    /// Circular arc to `p` passing through `via`.
    pub fn arc_to(&self, p: Vec3, via: Vec3) -> Result<Curve> {
        self.open_for("arc")?;
        let c = self.current();
        if arc3_params(c, p, via).is_none() {
            return Err(CadpoleError::invalid(format!(
                "arc: the three points ({}, {}, {}), via ({}, {}, {}) and ({}, {}, {}) are on one line or \
                 two of them coincide, so no circle passes through them; move `via` off the chord",
                c.x, c.y, c.z, via.x, via.y, via.z, p.x, p.y, p.z
            )));
        }
        let mut n = self.clone();
        n.segments.push(Segment3::Arc { a: c, b: p, via });
        Ok(n)
    }

    /// Cubic (two controls) or quadratic (one control) Bézier to `p`.
    pub fn bezier_to(&self, p: Vec3, ctrl: Vec<Vec3>) -> Result<Curve> {
        self.open_for("bezier")?;
        if ctrl.is_empty() || ctrl.len() > 2 {
            return Err(CadpoleError::invalid("bezier needs 1 or 2 control points"));
        }
        let mut n = self.clone();
        n.segments.push(Segment3::Bezier { a: self.current(), b: p, ctrl });
        Ok(n)
    }

    /// Smooth Catmull-Rom spline from the current point through `points`, one
    /// cubic Bézier per span, with the ends clamped exactly as the 2D
    /// `Path.spline` clamps them.
    pub fn spline_through(&self, points: &[Vec3], tension: f64) -> Result<Curve> {
        self.open_for("spline")?;
        if points.is_empty() {
            return Err(CadpoleError::invalid("spline needs at least one point"));
        }
        let mut pts = vec![self.current()];
        pts.extend_from_slice(points);
        let mut n = self.clone();
        n.segments.extend(
            catmull_rom_controls(&pts, false, tension)
                .into_iter()
                .map(|(a, c1, c2, b)| Segment3::Bezier { a, b, ctrl: vec![c1, c2] }),
        );
        Ok(n)
    }

    /// The closed curve: a straight segment back to the start is added only
    /// when the end is not already there. Closing a closed curve is a no-op.
    pub fn close(&self) -> Result<Curve> {
        if self.closed {
            return Ok(self.clone());
        }
        if self.segments.is_empty() {
            return Err(CadpoleError::invalid("close: the curve has no segments"));
        }
        let mut n = self.clone();
        let end = self.current();
        if end.dist(self.start) > SAME_POINT {
            n.segments.push(Segment3::Line { a: end, b: self.start });
        }
        if n.segments.len() < 2 {
            return Err(CadpoleError::invalid("a closed curve needs at least two segments"));
        }
        n.closed = true;
        Ok(n)
    }

    /// The same curve traversed the other way. A closed curve keeps its seam
    /// — it still starts where it started — and runs round the other way.
    pub fn reversed(&self) -> Curve {
        let mut segments: Vec<Segment3> = self.segments.iter().rev().map(Segment3::reversed).collect();
        if self.closed && !segments.is_empty() {
            // Reversing [e0, .., e(m-1)] gives [e(m-1)', .., e0'], which starts
            // at e(m-1)'s end: the seam. Nothing to rotate.
            debug_assert!(segments[0].start().dist(self.start) <= 1e-6);
        }
        let start = if self.closed { self.start } else { self.current() };
        if segments.is_empty() {
            segments = Vec::new();
        }
        Curve { start, segments, closed: self.closed }
    }

    /// Total arc length, in mm.
    pub fn length(&self) -> f64 {
        self.segments.iter().map(Segment3::length).sum()
    }

    /// Arc length at the start of each segment, followed by the total: `n + 1`
    /// entries for `n` segments, measured from the seam.
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

    /// The start of every segment, in order — for a closed curve, every
    /// vertex once, beginning at the seam.
    pub fn vertices(&self) -> Vec<Vec3> {
        if self.segments.is_empty() { vec![self.start] } else { self.segments.iter().map(Segment3::start).collect() }
    }

    /// The point a distance `s` from the seam along the curve (clamped).
    pub fn point_at_length(&self, s: f64) -> Vec3 {
        let mut rest = s.max(0.0);
        for seg in &self.segments {
            let len = seg.length();
            if rest <= len {
                return seg.point_at_length(rest);
            }
            rest -= len;
        }
        self.current()
    }

    /// `n` points at uniform fractions of the curve's length, from the seam.
    pub fn sample_fractions(&self, n: usize) -> Vec<Vec3> {
        let total = self.length();
        (0..n).map(|i| self.point_at_length(total * i as f64 / n as f64)).collect()
    }

    /// The best-fit plane, when every control point of the curve lies within
    /// [`PLANAR_TOL`] of it; `None` for a curve that is genuinely
    /// three-dimensional. The normal follows the drawing direction by the
    /// right-hand rule and the origin is the centroid of the control points.
    pub fn plane(&self) -> Option<Plane> {
        let pts: Vec<Vec3> = if self.segments.is_empty() {
            vec![self.start]
        } else {
            self.segments.iter().flat_map(|s| s.control_points().into_iter().skip(1)).collect()
        };
        let pts: Vec<Vec3> = std::iter::once(self.start).chain(pts).collect();
        if pts.len() < 3 {
            return None;
        }
        let centroid = pts.iter().fold(Vec3::ZERO, |a, p| a + *p) * (1.0 / pts.len() as f64);
        // Newell's normal over the control polygon, which is signed by the
        // winding; a straight run of points has none, and is planar in every
        // plane through it — pick one.
        let mut n = Vec3::ZERO;
        for i in 0..pts.len() {
            let (p, q) = (pts[i] - centroid, pts[(i + 1) % pts.len()] - centroid);
            n = n + p.cross(q);
        }
        let scale = pts.iter().map(|p| (*p - centroid).length()).fold(0.0f64, f64::max).max(1.0);
        let n = if n.length() > 1e-12 * scale * scale {
            n.normalized()
        } else {
            let dir = (pts[pts.len() - 1] - pts[0]).normalized();
            let pick = if dir.cross(Vec3::Z).length() > 1e-6 { Vec3::Z } else { Vec3::X };
            pick.cross(dir).normalized()
        };
        let (lo, hi) = pts.iter().fold((f64::MAX, f64::MIN), |(lo, hi), p| {
            let h = (*p - centroid).dot(n);
            (lo.min(h), hi.max(h))
        });
        if hi - lo > PLANAR_TOL {
            return None;
        }
        let x = pts[0] - centroid;
        let x = x - n * x.dot(n);
        let x_dir = if x.length() > 1e-9 {
            x.normalized()
        } else if n.cross(Vec3::X).length() > 1e-6 {
            Vec3::X
        } else {
            Vec3::Y
        };
        Plane::new(centroid, n, x_dir).ok()
    }

    /// A sketch path lifted onto `plane`: `Path.on(plane)`. Open.
    pub fn from_path(path: &Path, plane: &Plane) -> Curve {
        Curve {
            start: plane.to_world(path.start.x, path.start.y),
            segments: path.segments.iter().map(|s| Segment3::from_2d(s, plane)).collect(),
            closed: false,
        }
    }

    /// A sketch loop lifted onto `plane`, as drawn: one of `Sketch.curves()`.
    pub fn from_loop(l: &Loop, plane: &Plane) -> Curve {
        let segments: Vec<Segment3> = l.segments.iter().map(|s| Segment3::from_2d(s, plane)).collect();
        let start = segments.first().map(Segment3::start).unwrap_or(plane.origin);
        Curve { start, segments, closed: true }
    }

    /// The curve cut at the arc lengths in `positions` (ascending, starting at
    /// 0), one segment per entry — [`crate::loft`]'s section matcher applied
    /// to a 3D section. Every piece is an exact piece of the segment it came
    /// from ([`Segment3::sub`]). `what` names the section in diagnostics.
    pub fn split_at(&self, positions: &[f64], what: &str) -> Result<Curve> {
        let cum = self.cumulative_lengths();
        let total = *cum.last().expect("a curve");
        let mut segments = Vec::with_capacity(positions.len());
        for e in 0..positions.len() {
            let s0 = positions[e];
            let s1 = if e + 1 < positions.len() { positions[e + 1] } else { total };
            let j = cum.partition_point(|c| *c <= s0 + BOUNDARY_EPS).saturating_sub(1).min(self.segments.len() - 1);
            if s1 > cum[j + 1] + BOUNDARY_EPS {
                return Err(CadpoleError::invalid(format!(
                    "{what}: matching by arc length produced a piece from {s0:.6} mm to {s1:.6} mm that \
                     spans more than one segment of the curve, so it has no single exact curve. Give \
                     every section the same segment count and match in construction order \
                     (match=\"index\")"
                )));
            }
            let piece = self.segments[j].sub(s0 - cum[j], s1 - cum[j]).ok_or_else(|| {
                CadpoleError::invalid(format!(
                    "{what}: segment {j} cannot be cut between {:.6} mm and {:.6} mm of its length",
                    s0 - cum[j],
                    s1 - cum[j]
                ))
            })?;
            segments.push(piece);
        }
        let start = segments.first().map(Segment3::start).unwrap_or(self.start);
        Ok(Curve { start, segments, closed: self.closed })
    }

    /// The curve with segment `j` cut at the given fractions of its own arc
    /// length (each strictly between 0 and 1, ascending), so that one
    /// segment becomes `fractions.len() + 1`. Used to plant the same vertex on
    /// segment `j` of every section of a loft, where index pairing needs the
    /// counts to agree (the ruled cap).
    pub fn cut_segment(&self, j: usize, fractions: &[f64]) -> Result<Curve> {
        if fractions.is_empty() {
            return Ok(self.clone());
        }
        let seg = &self.segments[j];
        let len = seg.length();
        let mut bounds = vec![0.0];
        bounds.extend(fractions.iter().map(|f| f * len));
        bounds.push(len);
        let mut pieces = Vec::with_capacity(bounds.len() - 1);
        for w in bounds.windows(2) {
            pieces.push(seg.sub(w[0], w[1]).ok_or_else(|| {
                CadpoleError::invalid(format!(
                    "segment {j} cannot be cut between {:.6} mm and {:.6} mm of its {len:.6} mm",
                    w[0], w[1]
                ))
            })?);
        }
        let mut segments = self.segments.clone();
        segments.splice(j..j + 1, pieces);
        Ok(Curve { start: self.start, segments, closed: self.closed })
    }

    /// A helix as an open curve of one segment: `turns` turns of
    /// radius `radius` about the axis through `centre` along `axis`,
    /// advancing `pitch` per turn along it, right-handed (counter-clockwise
    /// about `+axis` while advancing along it) or left-handed, the radius
    /// growing by `taper` from the first turn to the last. It starts at
    /// `centre + radius·x̂` with `x̂` the world axis least aligned with `axis`
    /// (so `+X` for a vertical helix), turned by `phase_rad` about the axis.
    /// Stored analytically; nothing here is approximated.
    pub fn helix(
        centre: Vec3,
        axis: Vec3,
        radius: f64,
        pitch: f64,
        turns: f64,
        right_hand: bool,
        taper: f64,
        phase_rad: f64,
    ) -> Result<Curve> {
        let bad = |what: &str, v: f64| CadpoleError::invalid(format!("helix: {what} must be a positive finite number, got {v}"));
        if !(radius > 0.0) || !radius.is_finite() {
            return Err(bad("radius", radius));
        }
        if !(turns > 0.0) || !turns.is_finite() {
            return Err(bad("turns", turns));
        }
        if !pitch.is_finite() || pitch < 0.0 {
            return Err(CadpoleError::invalid(format!("helix: pitch must be a finite number of mm per turn, at least 0, got {pitch}")));
        }
        if !taper.is_finite() || !(radius + taper > 0.0) {
            return Err(CadpoleError::invalid(format!(
                "helix: taper is the change of radius from the first turn to the last, and {taper} would take a \
                 radius of {radius} to {} — it has to stay positive",
                radius + taper
            )));
        }
        let Some(axis) = unit(axis) else {
            return Err(CadpoleError::invalid("helix: axis must be a non-zero direction"));
        };
        if !phase_rad.is_finite() {
            return Err(CadpoleError::invalid(format!("helix: phase must be finite, got {phase_rad}")));
        }
        let (u, w) = perpendicular_frame(axis);
        let (s, c) = fmath::sin_cos(phase_rad);
        let x_hat = u * c + w * s;
        let a = centre + x_hat * radius;
        let angle = turns * std::f64::consts::TAU * if right_hand { 1.0 } else { -1.0 };
        let seg = Segment3::Helix { a, centre, axis, r_end: radius + taper, angle, advance: pitch * turns };
        Ok(Curve { start: a, segments: vec![seg], closed: false })
    }

    /// A closed, planar curve as a sketch loop on its own plane: what a
    /// `Curve` given as a `sweep_along` profile becomes. Every control point
    /// is written in the plane's frame, which is exact for a curve that lies
    /// in it (the [`Curve::plane`] test). `what` names the caller in the
    /// diagnostic.
    pub fn to_loop(&self, what: &str) -> Result<(Plane, Loop)> {
        if !self.closed {
            return Err(CadpoleError::invalid(format!(
                "{what}: the profile curve is open ({} segments, ending at ({}, {}, {})); close() it first",
                self.segments.len(),
                self.end().x,
                self.end().y,
                self.end().z
            )));
        }
        let plane = self.plane().ok_or_else(|| {
            CadpoleError::invalid(format!(
                "{what}: the profile curve does not lie in a plane (its control points leave the best-fit plane \
                 by more than {PLANAR_TOL} mm); a profile is a planar section. Draw it as a sketch, or with \
                 Path.on(plane)"
            ))
        })?;
        let to2 = |p: Vec3| {
            let d = p - plane.origin;
            P2::new(d.dot(plane.x_dir), d.dot(plane.y_dir()))
        };
        let mut segments = Vec::with_capacity(self.segments.len());
        for (i, seg) in self.segments.iter().enumerate() {
            segments.push(match seg {
                Segment3::Line { a, b } => Segment::Line { a: to2(*a), b: to2(*b) },
                Segment3::Arc { a, b, via } => Segment::Arc { a: to2(*a), b: to2(*b), via: to2(*via) },
                Segment3::Bezier { a, b, ctrl } => {
                    Segment::Bezier { a: to2(*a), b: to2(*b), ctrl: ctrl.iter().map(|c| to2(*c)).collect() }
                }
                Segment3::Helix { .. } => {
                    return Err(CadpoleError::invalid(format!(
                        "{what}: segment {i} of the profile curve is a helix, which a planar profile cannot contain"
                    )));
                }
            });
        }
        Ok((plane, Loop { segments }))
    }

    /// A short description for the feature tree: `3 segments (line, arc, bezier), closed`.
    pub fn describe(&self) -> String {
        let kinds: Vec<&str> = self.segments.iter().map(Segment3::kind).collect();
        format!(
            "{} segment(s) ({}), {}",
            self.segments.len(),
            kinds.join(", "),
            if self.closed { "closed" } else { "open" }
        )
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    fn v(x: f64, y: f64, z: f64) -> Vec3 {
        Vec3::new(x, y, z)
    }

    /// The whole point of `Path.on(plane)`: a 2D path lifted onto XY *is* the
    /// curve drawn directly with z = 0 — the same segments, the same control
    /// points, to the bit — so there is one curve type in the language.
    #[test]
    fn a_path_lifted_onto_xy_is_the_curve_drawn_directly() {
        let path = Path::new(P2::new(0.0, 0.0))
            .line_to(P2::new(10.0, 0.0))
            .arc_to(P2::new(10.0, 4.0), P2::new(12.0, 2.0))
            .bezier_to(P2::new(4.0, 6.0), vec![P2::new(8.0, 9.0), P2::new(6.0, 3.0)])
            .unwrap()
            .spline_through(&[P2::new(2.0, 5.0), P2::new(-1.0, 3.0)], 0.5)
            .unwrap();
        let lifted = Curve::from_path(&path, &Plane::XY);
        let direct = Curve::new(v(0.0, 0.0, 0.0))
            .line_to(v(10.0, 0.0, 0.0))
            .unwrap()
            .arc_to(v(10.0, 4.0, 0.0), v(12.0, 2.0, 0.0))
            .unwrap()
            .bezier_to(v(4.0, 6.0, 0.0), vec![v(8.0, 9.0, 0.0), v(6.0, 3.0, 0.0)])
            .unwrap()
            .spline_through(&[v(2.0, 5.0, 0.0), v(-1.0, 3.0, 0.0)], 0.5)
            .unwrap();
        assert_eq!(lifted, direct);
        // ...and its arc length is the 2D one, because a z of exactly zero
        // adds exactly zero to every distance.
        let len2: f64 = path.segments.iter().map(Segment::length).sum();
        assert_eq!(lifted.length(), len2);
        // The same on a loop: `Sketch.curves()` is `from_loop`.
        let closed_path = path.close().unwrap();
        let from_loop = Curve::from_loop(&closed_path, &Plane::XY);
        assert_eq!(from_loop.segments, direct.close().unwrap().segments);
        assert!(from_loop.closed);
    }

    /// A three-point arc in space is an arc of the circle through its three
    /// points, reached by turning through `via`, and its length is r·angle.
    #[test]
    fn a_three_point_arc_lies_on_its_circle_and_passes_through_via() {
        // Radius 5 about (1, 2, 3) in a tilted plane, drawn the long way round.
        let c = v(1.0, 2.0, 3.0);
        let u = v(1.0, 0.0, 0.0);
        let w = v(0.0, 1.0, 1.0).normalized();
        let at = |t: f64| c + u * (5.0 * fmath::cos(t)) + w * (5.0 * fmath::sin(t));
        let seg = Segment3::Arc { a: at(0.0), b: at(1.0), via: at(4.0) };
        let arc = arc3_params(at(0.0), at(1.0), at(4.0)).unwrap();
        assert!((arc.radius - 5.0).abs() < 1e-9);
        assert!((arc.centre - c).length() < 1e-9, "{:?}", arc.centre);
        // Through `via` means the sweep is the long way round: 2π - 1 of
        // turn (its sign is relative to the frame the arc chose, which puts
        // `via` on the positive side).
        assert!((arc.sweep.abs() - (std::f64::consts::TAU - 1.0)).abs() < 1e-9, "{}", arc.sweep);
        assert!((seg.length() - 5.0 * (std::f64::consts::TAU - 1.0)).abs() < 1e-9);
        let len = seg.length();
        for f in [0.0, 0.1, 0.5, 0.9, 1.0] {
            let p = seg.point_at_length(len * f);
            assert!(((p - c).length() - 5.0).abs() < 1e-9, "left the circle at {f}");
        }
        // Halfway along the long way round is at angle -(2π - 1)/2 from the start.
        let mid = seg.point_at_length(len / 2.0);
        assert!((mid - at(-(std::f64::consts::TAU - 1.0) / 2.0)).length() < 1e-9);
        // The short way, drawn through a point on it.
        let short = Segment3::Arc { a: at(0.0), b: at(1.0), via: at(0.5) };
        assert!((short.length() - 5.0).abs() < 1e-9);
        // Collinear points are not an arc.
        assert!(arc3_params(v(0.0, 0.0, 0.0), v(2.0, 0.0, 0.0), v(1.0, 0.0, 0.0)).is_none());
    }

    /// Splitting keeps an arc on its circle and a Bézier on its curve —
    /// which the ruled cap and `match="arclen"`
    /// on 3D sections both rely on.
    #[test]
    fn splitting_a_3d_segment_stays_on_its_geometry() {
        let c = v(0.0, 0.0, 0.0);
        let at = |t: f64| c + v(4.0 * fmath::cos(t), 4.0 * fmath::sin(t), 2.0 * fmath::sin(t));
        let arc = Segment3::Arc { a: at(0.0), b: at(2.0), via: at(1.0) };
        let len = arc.length();
        let piece = arc.sub(len * 0.3, len * 0.8).unwrap();
        assert!(matches!(piece, Segment3::Arc { .. }));
        let plen = piece.length();
        assert!((plen - len * 0.5).abs() < 1e-9);
        let params = arc3_params(at(0.0), at(2.0), at(1.0)).unwrap();
        for f in [0.0, 0.25, 0.5, 0.75, 1.0] {
            let p = piece.point_at_length(plen * f);
            assert!(((p - params.centre).length() - params.radius).abs() < 1e-9, "left the circle");
        }
        let bez = Segment3::Bezier {
            a: v(0.0, 0.0, 0.0),
            b: v(10.0, 0.0, 5.0),
            ctrl: vec![v(2.0, 8.0, 1.0), v(8.0, -8.0, 9.0)],
        };
        let len = bez.length();
        let piece = bez.sub(len * 0.2, len * 0.6).unwrap();
        let plen = piece.length();
        for t in [0.0, 0.3, 0.6, 1.0] {
            let want = bez.point_at_length(len * (0.2 + 0.4 * t));
            let got = piece.point_at_length(plen * t);
            assert!((want - got).length() < 1e-6, "{want:?} vs {got:?}");
        }
        // The pieces add up to the whole.
        let a = bez.sub(0.0, len * 0.2).unwrap().length();
        let b = bez.sub(len * 0.6, len).unwrap().length();
        assert!((a + plen + b - len).abs() < 1e-6);
    }

    /// A 3D spline passes through its points, is one cubic Bézier per span,
    /// and is tangent-continuous at the joints, like the 2D one.
    #[test]
    fn a_spline_through_3d_points_is_cubic_beziers_through_them() {
        let pts = [v(3.0, 1.0, 2.0), v(6.0, 4.0, 0.0), v(9.0, 2.0, 5.0)];
        let c = Curve::new(v(0.0, 0.0, 0.0)).spline_through(&pts, 0.5).unwrap();
        assert_eq!(c.segments.len(), 3);
        for (seg, p) in c.segments.iter().zip(&pts) {
            assert!(matches!(seg, Segment3::Bezier { ctrl, .. } if ctrl.len() == 2));
            assert_eq!(seg.end(), *p);
        }
        for i in 0..2 {
            let a = c.segments[i].end_direction().unwrap();
            let b = c.segments[i + 1].start_direction().unwrap();
            assert!((a - b).length() < 1e-9, "joint {i}: {a:?} vs {b:?}");
        }
    }

    /// `close()` adds a line only when there is a gap; `reverse()` keeps a
    /// closed curve's seam; a closed curve takes no more segments.
    #[test]
    fn close_and_reverse_behave_like_the_sketch_path() {
        let open = Curve::new(v(0.0, 0.0, 0.0)).line_to(v(4.0, 0.0, 0.0)).unwrap().line_to(v(4.0, 3.0, 1.0)).unwrap();
        let closed = open.close().unwrap();
        assert_eq!(closed.segments.len(), 3, "a line back to the start was added");
        assert!(closed.closed);
        let already = open.line_to(v(0.0, 0.0, 0.0)).unwrap().close().unwrap();
        assert_eq!(already.segments.len(), 3, "no segment added when the end is the start");
        assert!(closed.line_to(v(9.0, 9.0, 9.0)).unwrap_err().to_string().contains("closed"));
        let rev = closed.reversed();
        assert_eq!(rev.start, closed.start, "the seam stays put");
        assert!((rev.length() - closed.length()).abs() < 1e-12);
        assert_eq!(rev.segments[0].start(), closed.start);
        assert_eq!(rev.reversed(), closed);
        // A degenerate line and a collinear arc are refused, with the reason.
        let e = open.line_to(v(4.0, 3.0, 1.0)).unwrap_err().to_string();
        assert!(e.contains("no length"), "{e}");
        let e = open.arc_to(v(4.0, 3.0, 5.0), v(4.0, 3.0, 3.0)).unwrap_err().to_string();
        assert!(e.contains("no circle"), "{e}");
    }

    /// `plane` is the best-fit plane of a planar curve, on any plane, and
    /// `None` for a curve that leaves it by more than the kernel's tolerance.
    #[test]
    fn plane_is_reported_for_planar_curves_only() {
        let tilted = Plane::new(v(1.0, 2.0, 3.0), v(1.0, 1.0, 1.0), v(1.0, -1.0, 0.0)).unwrap();
        let path = Path::new(P2::new(-5.0, -5.0))
            .line_to(P2::new(5.0, -5.0))
            .arc_to(P2::new(5.0, 5.0), P2::new(8.0, 0.0))
            .spline_through(&[P2::new(0.0, 7.0), P2::new(-5.0, 5.0)], 0.5)
            .unwrap();
        let c = Curve::from_path(&path, &tilted).close().unwrap();
        let p = c.plane().expect("planar");
        assert!((p.normal - tilted.normal).length() < 1e-9 || (p.normal + tilted.normal).length() < 1e-9);
        assert!(((p.origin - tilted.origin).dot(tilted.normal)).abs() < 1e-9, "the origin is on the plane");
        // Counter-clockwise as drawn (seen from +normal), so the right-hand
        // rule gives the plane's own normal.
        assert!((p.normal - tilted.normal).length() < 1e-9);
        // Lift one control point a micron and a half off the plane: no longer planar.
        let mut warped = c.clone();
        if let Segment3::Line { b, .. } = &mut warped.segments[0] {
            *b = *b + tilted.normal * 1.5e-6;
        }
        assert!(warped.plane().is_none());
        // A genuinely twisted loop.
        let twisted = Curve::new(v(0.0, 0.0, 0.0))
            .line_to(v(10.0, 0.0, 0.0))
            .unwrap()
            .line_to(v(10.0, 10.0, 3.0))
            .unwrap()
            .line_to(v(0.0, 10.0, 0.0))
            .unwrap()
            .close()
            .unwrap();
        assert!(twisted.plane().is_none());
    }

    /// `split_at` and `cut_segment` cut exactly and keep the total length.
    #[test]
    fn cutting_a_curve_keeps_its_length_and_its_seam() {
        let c = Curve::new(v(0.0, 0.0, 0.0))
            .line_to(v(10.0, 0.0, 0.0))
            .unwrap()
            .arc_to(v(10.0, 10.0, 2.0), v(13.0, 5.0, 1.0))
            .unwrap()
            .spline_through(&[v(4.0, 12.0, 4.0), v(0.0, 10.0, 0.0)], 0.5)
            .unwrap()
            .close()
            .unwrap();
        let total = c.length();
        let cum = c.cumulative_lengths();
        let mut positions = cum[..cum.len() - 1].to_vec();
        positions.push(total * 0.13);
        positions.push(total * 0.61);
        positions.sort_by(|a, b| a.partial_cmp(b).unwrap());
        let split = c.split_at(&positions, "test").unwrap();
        assert_eq!(split.segments.len(), c.segments.len() + 2);
        assert!((split.length() - total).abs() < 1e-6);
        assert_eq!(split.start, c.start);
        assert!(split.closed);
        for w in split.segments.windows(2) {
            assert!((w[0].end() - w[1].start()).length() < 1e-9, "the pieces do not chain");
        }
        let cut = c.cut_segment(1, &[0.25, 0.5]).unwrap();
        assert_eq!(cut.segments.len(), c.segments.len() + 2);
        assert!((cut.length() - total).abs() < 1e-9);
        assert!(cut.segments[1..4].iter().all(|s| matches!(s, Segment3::Arc { .. })));
    }
}
