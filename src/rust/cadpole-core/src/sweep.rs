//! Sweeping a profile along a curve: `sweep_along`.
//!
//! A planar profile — a sketch, as drawn, wherever it is — is carried along
//! an open, tangent-continuous [`Curve`] by a *moving frame*, and the solid
//! it sweeps out is built segment by segment on the exact B-rep kernel with
//! every seam a shared edge (truck connects faces by id,
//! not by position, so each segment's far wire *is* the next segment's
//! start wire).
//!
//! ## The exactness contract, by segment kind
//!
//! Each segment of the path is looked at on its own, with the frame the
//! path has reached by then:
//!
//! * a **straight** segment across which the frame does not turn is a
//!   translation — `builder::tsweep` of the current wire, **exact**; with
//!   `scale=` it is the ruled surface between the two end sections (the
//!   loft's own skinner at degree 1), still exact, because a linear
//!   scale along a line moves every point on a straight line;
//! * a **circular arc** across which the frame turns exactly as the arc
//!   does — a rotation about the arc's own axis — is
//!   `builder::rsweep` about that axis, **exact**: its faces are the same
//!   `RevolutedCurve` surfaces a `revolve` makes, and a swept circle is a
//!   torus that [`crate::surface_class`] recognises;
//! * everything else — a **Bézier or spline** segment, a **helix** (which is
//!   not a rational curve and has no NURBS form), an arc under a frame that
//!   does not follow it, any segment with `twist=` — is a NURBS
//!   **approximation** of the swept surface: profile copies are placed by
//!   the frame at *stations* along the segment and the loft's global
//!   skinner runs one cubic B-spline surface through them
//!   ([`crate::skin`]). The stations are chosen by the bound-driven
//!   division the tessellator uses — the tangent may turn by at most
//!   [`DEFAULT_MAX_NORMAL_DEGREES`] between stations, and the cubic
//!   interpolant's chordal bound must be within the target — and then the
//!   achieved deviation from the true sweep is **measured**, doubled until
//!   it is within the target, and reported in the kernel notes.
//!
//! Whether a straight or circular segment is exact is *checked*, never
//! assumed: the frame at the segment's end is compared with the segment's
//! own rigid motion applied to the frame at its start. Under the
//! rotation-minimising frame that holds by construction for lines and
//! arcs; under `frame.fixed` an arc whose axis is not along `up` fails it
//! and takes the approximated path, honestly.
//!
//! ## Frames
//!
//! * **`rmf`** (the default): the rotation-minimising frame, by the double
//!   reflection method (Wang, Jüttler, Zheng and Liu 2008), which is fourth
//!   order accurate in the step. It is propagated from station to station
//!   with the step halved until the frame at the end of the segment no
//!   longer moves ([`RMF_CONVERGENCE`]), so the twist error is below any
//!   tolerance a document names. Along a straight segment the frame does
//!   not turn; along a circular arc it turns with the arc, exactly — both
//!   are rotation-minimising by definition — so those two are set in
//!   closed form. A square swept along any planar path stays a square, in
//!   the same orientation, at the end.
//! * **`frenet`**: the tangent, the curvature normal and their cross
//!   product. Well defined only where the path curves, so a straight
//!   segment carries the frame it inherits, and an **inflection** — the
//!   curvature normal flipping sides — is refused with the position, as is
//!   a joint where the curvature normal jumps. It is what a thread wants:
//!   on a helix the Frenet frame is the screw motion about the axis, so a
//!   profile drawn in the axial plane stays in it.
//! * **`fixed`**: the normal is `up` projected perpendicular to the
//!   tangent, so the profile keeps one direction pointing up. Refused where
//!   the tangent runs along `up`.
//!
//! The profile is taken **as drawn**: its plane need not be perpendicular
//! to the path, and a profile drawn on the XY plane and swept up +Z is the
//! extrude, bit for bit. The frame at the path's start is the reference;
//! the placement at any station is the rigid motion taking the start frame
//! to the frame there, about the path's start point, followed by the
//! station's share of `twist=` (about the tangent) and `scale=` (about the
//! path point). Because every rotation-minimising frame field along one
//! path differs from every other by a constant turn about the tangent, the
//! choice of the initial normal does not change the geometry under
//! `frame.rmf`; `up=` matters to `frame.fixed` and to the reported
//! `end_plane` only.

use std::f64::consts::TAU;

use truck_meshalgo::prelude::*;
use truck_modeling::{builder, Face, Matrix4, Shell, Solid, Surface, Wire};
use truck_topology::shell::ShellCondition;

use crate::curve::{arc3_params, Curve, Segment3};
use crate::geom::{Plane, Vec3};
use crate::kernel::{self, guarded, trace, KSolid};
use crate::ksurface::DEFAULT_MAX_NORMAL_DEGREES;
use crate::sketch::Sketch;
use crate::{CadpoleError, Result};
use crate::fmath;

/// How the profile is oriented as it moves along the path.
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub enum Frame {
    /// Rotation-minimising: no turning about the tangent anywhere.
    Rmf,
    /// The Frenet–Serret frame: the curvature normal.
    Frenet,
    /// `up` projected perpendicular to the tangent.
    Fixed,
}

impl Frame {
    /// Every variant, in documentation order: the `frame` vocabulary's
    /// members. A new variant fails `vocabulary_tests` until it is
    /// listed here.
    pub const ALL: [Frame; 3] = [Frame::Rmf, Frame::Frenet, Frame::Fixed];

    pub fn name(self) -> &'static str {
        match self {
            Frame::Rmf => "rmf",
            Frame::Frenet => "frenet",
            Frame::Fixed => "fixed",
        }
    }

    pub fn parse(s: &str) -> Result<Frame> {
        Frame::ALL.iter().copied().find(|f| f.name() == s).ok_or_else(|| {
            CadpoleError::invalid(format!(
                "frame={s:?} is not a sweep frame; valid: {}",
                Frame::ALL.iter().map(|f| f.name()).collect::<Vec<_>>().join(", ")
            ))
        })
    }
}

/// What a sweep is asked for.
#[derive(Clone, Copy, Debug)]
pub struct SweepSpec<'a> {
    /// The profile, as drawn: one solid per region (a loop with its holes).
    pub profile: &'a Sketch,
    /// The path: open, tangent-continuous, at least one segment.
    pub path: &'a Curve,
    pub frame: Frame,
    /// The reference direction: `frame.fixed`'s up, and the seed of the
    /// rotation-minimising frame's normal (which changes nothing but the
    /// reported end plane). `None` takes the profile plane's x direction.
    pub up: Option<Vec3>,
    /// Extra turn of the profile about the tangent over the whole length,
    /// radians, shared out by arc length.
    pub twist: f64,
    /// The profile's scale at the end of the path, about the path point;
    /// `1` at the start, linear in arc length between.
    pub scale: f64,
    /// The document's tessellation tolerance, which sets the approximation
    /// target ([`approximation_target`]).
    pub tolerance: f64,
}

/// The approximation target for a skinned segment, from the document
/// tolerance: a fifth of it, and never coarser than 0.01 mm. The mesh a
/// user sees is within the tolerance of the *model*; the model's own
/// departure from the true sweep should be a small part of that, and on
/// the default 0.05 mm it is a hundredth of a millimetre.
pub fn approximation_target(tolerance: f64) -> f64 {
    let t = if tolerance > 0.0 && tolerance.is_finite() { tolerance } else { kernel::DEFAULT_TOLERANCE };
    (t / 5.0).min(0.01)
}

/// The turn of the frame between two consecutive stations of a skinned
/// segment, at most: the tessellator's normal bound, in radians.
fn max_station_turn() -> f64 {
    DEFAULT_MAX_NORMAL_DEGREES.to_radians()
}

/// The double-reflection step is halved until the normal at the end of the
/// segment moves by less than this angle, in radians.
const RMF_CONVERGENCE: f64 = 1e-11;

/// Two unit tangents within this angle of each other at a joint, in
/// radians, are continuous: about 0.06°. A path typed by hand meets this
/// way — an arc through a via point rounded to two decimals is a few
/// hundredths of a degree off the lines it joins — and so does a path
/// computed in floating point; a corner anyone means is far wider. Across
/// such a joint the frame is carried as it is rather than re-based on the
/// next tangent (see [`segment_frames`]), so the profile wires the exact
/// segments carry and the frames the caps and stations are placed by stay
/// one and the same, and the solid closes; the profile is then off
/// perpendicular to the path by the joints' angles so far, which add up, so
/// past this angle in total the path is refused as turning a corner.
const JOINT_TOL: f64 = 1e-3;

/// Below this angle, in radians, a joint is tangent to the arithmetic and
/// the frame is re-based on the next segment's own tangent, as before.
const EXACT_JOINT: f64 = 1e-9;

/// How far a checked frame may be from a segment's rigid motion for the
/// segment to count as exact.
const RIGID_TOL: f64 = 1e-9;

/// How many times a skinned segment's stations are doubled when the
/// measured deviation is over the target, before giving up and reporting
/// what was reached.
const MAX_REFINEMENTS: usize = 5;

/// Fractions along a segment at which the namer plants its points.
const NAMER_FRACTIONS: [f64; 5] = [0.0, 0.25, 0.5, 0.75, 1.0];

/// The most station intervals one skinned face spans. A skinned segment
/// with more stations is built as a chain of faces, each a skin through
/// this many intervals and sharing its end wire with the next, because
/// truck evaluates a B-spline surface by computing *every* basis function
/// of its knot vector — the cost of one point grows with the row count —
/// and the tessellator asks for tens of thousands of points: a two-turn
/// spring as one 91-row face took 16 s to mesh at 0.25 mm in a debug
/// build, and the same spring in 17-row faces a fraction of that. The
/// faces meet on their shared station wires; the tangent across such a
/// seam differs by the interpolation error, which is what the measured
/// deviation bounds, and a run of such faces carries one name with the
/// grammar's `#k` pieces.
const MAX_INTERVALS_PER_FACE: usize = 16;

// ------------------------------------------------------------------ frames

/// The moving frame at one point of the path: the point, the unit tangent
/// and two unit normals with `b = t × n`.
#[derive(Clone, Copy, Debug)]
pub struct FrameAt {
    pub point: Vec3,
    pub t: Vec3,
    pub n: Vec3,
    pub b: Vec3,
}

impl FrameAt {
    fn new(point: Vec3, t: Vec3, n: Vec3) -> FrameAt {
        let n = (n - t * n.dot(t)).normalized();
        FrameAt { point, t, n, b: t.cross(n) }
    }

    /// The frame rotated by `angle` about `axis` through `origin`.
    fn rotated(&self, origin: Vec3, axis: Vec3, angle: f64) -> FrameAt {
        let r = |v: Vec3| rotate(v, axis, angle);
        FrameAt { point: origin + r(self.point - origin), t: r(self.t), n: r(self.n), b: r(self.b) }
    }

    /// The frame moved to `point`, the same axes.
    fn moved(&self, point: Vec3) -> FrameAt {
        FrameAt { point, ..*self }
    }

    /// The frame's normal turned by `angle` about its tangent.
    fn twisted(&self, angle: f64) -> FrameAt {
        if angle == 0.0 {
            return *self;
        }
        let (s, c) = fmath::sin_cos(angle);
        let n = self.n * c + self.b * s;
        FrameAt { point: self.point, t: self.t, n, b: self.t.cross(n) }
    }

    /// The plane perpendicular to the path here: origin the point, normal
    /// the tangent, x direction the frame's normal.
    pub fn plane(&self) -> Plane {
        Plane { origin: self.point, normal: self.t, x_dir: self.n }
    }
}

/// Rodrigues' rotation of `v` by `angle` about the unit vector `axis`.
fn rotate(v: Vec3, axis: Vec3, angle: f64) -> Vec3 {
    let (s, c) = fmath::sin_cos(angle);
    v * c + axis.cross(v) * s + axis * (axis.dot(v) * (1.0 - c))
}

/// One double-reflection step (Wang et al. 2008, algorithm 1): the frame
/// at the next sample from the frame at this one, given the next point and
/// tangent. Two reflections — in the bisector plane of the chord, then in
/// the bisector plane of the reflected and the true tangent — carry the
/// normal with no rotation about the tangent to fourth order in the step.
fn rmf_step(f: &FrameAt, p1: Vec3, t1: Vec3) -> FrameAt {
    let v1 = p1 - f.point;
    let c1 = v1.dot(v1);
    if !(c1 > 0.0) {
        return FrameAt::new(p1, t1, f.n);
    }
    let r_l = f.n - v1 * (2.0 / c1 * v1.dot(f.n));
    let t_l = f.t - v1 * (2.0 / c1 * v1.dot(f.t));
    let v2 = t1 - t_l;
    let c2 = v2.dot(v2);
    let n1 = if c2 > 0.0 { r_l - v2 * (2.0 / c2 * v2.dot(r_l)) } else { r_l };
    FrameAt::new(p1, t1, n1)
}

/// The angle between two unit vectors, radians.
fn angle_between(a: Vec3, b: Vec3) -> f64 {
    fmath::acos(a.dot(b).clamp(-1.0, 1.0))
}

/// The unit tangent of a segment at `t`, or `None` where the derivative
/// vanishes (a Bézier with a cusp; a helix on its axis).
fn tangent_at(seg: &Segment3, t: f64) -> Option<Vec3> {
    let d = seg.derivatives(t).0;
    let l = d.length();
    (l > 0.0 && l.is_finite()).then(|| d * (1.0 / l))
}

/// The Frenet normal of a segment at `t`: the curvature direction, or
/// `None` where the segment is straight there.
fn frenet_normal_at(seg: &Segment3, t: f64) -> Option<Vec3> {
    let (d1, d2) = seg.derivatives(t);
    let l = d1.length();
    if !(l > 0.0) {
        return None;
    }
    let tang = d1 * (1.0 / l);
    let perp = d2 - tang * d2.dot(tang);
    // Curvature below 1e-9 per mm, on a segment whose speed is `l`, is
    // straight: a radius of a thousand kilometres.
    (perp.length() > 1e-9 * l * l).then(|| perp.normalized())
}

/// The frames of one segment, as a table at uniform parameters: entry `i`
/// is the frame at `t = i / (len − 1)`.
#[derive(Clone, Debug)]
struct SegmentFrames {
    table: Vec<FrameAt>,
    /// Arc length from the segment's start at each table entry, scaled so
    /// the last is exactly the segment's `length()`.
    lengths: Vec<f64>,
    /// Which way the frame was reached: `Some` when it is a rigid motion of
    /// the segment's own — a translation along a line, a rotation about an
    /// arc's axis — and the segment may be swept exactly.
    rigid: Option<Rigid>,
    /// Arc length of the segment.
    length: f64,
}

/// The rigid motion a segment's frame follows, when it does.
#[derive(Clone, Copy, Debug)]
enum Rigid {
    Translation(Vec3),
    Rotation { centre: Vec3, axis: Vec3, angle: f64 },
}

impl SegmentFrames {
    fn start(&self) -> &FrameAt {
        &self.table[0]
    }

    fn end(&self) -> &FrameAt {
        &self.table[self.table.len() - 1]
    }

    /// Arc length from the start to parameter `t`, read off the table
    /// (linear between entries, which are dense) and exact at the ends.
    fn length_to(&self, t: f64) -> f64 {
        let n = self.table.len() - 1;
        let t = t.clamp(0.0, 1.0);
        if t >= 1.0 {
            return self.length;
        }
        let x = t * n as f64;
        let i = (x.floor() as usize).min(n - 1);
        let f = x - i as f64;
        self.lengths[i] + (self.lengths[i + 1] - self.lengths[i]) * f
    }

    /// The frame at parameter `t`: the table entry at or before it, carried
    /// the rest of the way by one closed-form or double-reflection step.
    fn at(&self, seg: &Segment3, frame: Frame, up: Vec3, t: f64) -> FrameAt {
        let n = self.table.len() - 1;
        let t = t.clamp(0.0, 1.0);
        let i = ((t * n as f64).floor() as usize).min(n);
        let ti = i as f64 / n as f64;
        let base = &self.table[i];
        if (t - ti).abs() <= 1e-15 {
            return *base;
        }
        match self.rigid {
            Some(Rigid::Translation(_)) => base.moved(seg.point_at_param(t)),
            Some(Rigid::Rotation { centre, axis, angle }) => base.rotated(centre, axis, angle * (t - ti)),
            None => {
                let p = seg.point_at_param(t);
                let tang = tangent_at(seg, t).unwrap_or(base.t);
                match frame {
                    Frame::Rmf => rmf_step(base, p, tang),
                    Frame::Frenet => FrameAt::new(p, tang, frenet_normal_at(seg, t).unwrap_or(base.n)),
                    Frame::Fixed => FrameAt::new(p, tang, up),
                }
            }
        }
    }
}

/// The frames of the whole path, segment by segment, continuous across the
/// joints.
#[derive(Clone, Debug)]
pub struct FrameField {
    frame: Frame,
    up: Vec3,
    segments: Vec<SegmentFrames>,
    /// Cumulative arc length at the start of each segment, then the total.
    cum: Vec<f64>,
}

impl FrameField {
    /// The frames along `path` under `frame`, seeded by `up` (a direction
    /// not along the start tangent; the profile plane's x direction is the
    /// usual choice) and `profile_x`, the fallback seed.
    pub fn new(path: &Curve, frame: Frame, up: Option<Vec3>, profile_x: Vec3) -> Result<FrameField> {
        Self::build(path, frame, up, profile_x, true)
    }

    /// The frames along `path` for a pattern: the same field as
    /// [`FrameField::new`], on a path that may be closed and may turn a
    /// corner. A sweep needs a tangent-continuous open path because its
    /// surface must join up; a pattern only reads the frame at its stations,
    /// so a rivet line round a rectangle or a ring of posts on a circle is
    /// fine. At a corner the normal is carried across as it is when it
    /// stays off the new tangent, and otherwise the binormal is (the frame
    /// turns about it, as it does at a square corner in the plane).
    pub fn lenient(path: &Curve, frame: Frame, up: Option<Vec3>, profile_x: Vec3) -> Result<FrameField> {
        Self::build(path, frame, up, profile_x, false)
    }

    fn build(path: &Curve, frame: Frame, up: Option<Vec3>, profile_x: Vec3, strict: bool) -> Result<FrameField> {
        if path.segments.is_empty() {
            return Err(CadpoleError::invalid("sweep_along: the path has no segments"));
        }
        if path.closed && strict {
            return Err(CadpoleError::invalid(
                "sweep_along: the path is closed; a sweep runs from a start to an end, so give it an open curve \
                 (a closed path would need the profile to meet itself, which is not built)",
            ));
        }
        let segs = &path.segments;
        // Every segment must have a direction at both ends, and consecutive
        // ones must agree there.
        let mut tangents = Vec::with_capacity(segs.len());
        for (i, seg) in segs.iter().enumerate() {
            let (Some(t0), Some(t1)) = (tangent_at(seg, 0.0), tangent_at(seg, 1.0)) else {
                return Err(CadpoleError::invalid(format!(
                    "sweep_along: segment {i} of the path ({}) has no direction at one of its ends",
                    seg.kind()
                )));
            };
            tangents.push((t0, t1));
        }
        for i in 1..segs.len() {
            let (prev, next) = (tangents[i - 1].1, tangents[i].0);
            if !strict {
                continue;
            }
            if angle_between(prev, next) > JOINT_TOL {
                let p = segs[i].start();
                return Err(CadpoleError::invalid(format!(
                    "sweep_along: the path turns a corner of {:.2}° between segments {} and {i} at ({}, {}, {}); \
                     a sweep needs a tangent-continuous path, so round the corner with an arc (the way a bent \
                     pipe is drawn) or split the sweep there",
                    angle_between(prev, next).to_degrees(),
                    i - 1,
                    crate::context::fmt_num(p.x),
                    crate::context::fmt_num(p.y),
                    crate::context::fmt_num(p.z)
                )));
            }
        }
        let t0 = tangents[0].0;
        let up_dir = match up {
            Some(u) => {
                let u = u.normalized();
                if u.length() == 0.0 {
                    return Err(CadpoleError::invalid("sweep_along: up= must be a non-zero direction"));
                }
                u
            }
            None => profile_x,
        };
        // The seed normal: `up` off the tangent; failing that the profile's
        // x direction; failing that anything perpendicular.
        let seed = |dir: Vec3| -> Option<Vec3> {
            let n = dir - t0 * dir.dot(t0);
            (n.length() > 1e-9).then(|| n.normalized())
        };
        let n0 = match frame {
            Frame::Fixed => seed(up_dir).ok_or_else(|| {
                CadpoleError::invalid(format!(
                    "sweep_along: frame.fixed keeps up=({}, {}, {}) pointing up, but the path starts along that \
                     very direction, so there is no way to hold it; pick an up= off the path's tangent",
                    crate::context::fmt_num(up_dir.x),
                    crate::context::fmt_num(up_dir.y),
                    crate::context::fmt_num(up_dir.z)
                ))
            })?,
            Frame::Rmf => seed(up_dir)
                .or_else(|| seed(profile_x))
                .unwrap_or_else(|| crate::curve::perpendicular_frame(t0).0),
            Frame::Frenet => {
                // The first curved segment's normal, carried back over any
                // leading straight run; the seed only when nothing curves.
                let mut n = None;
                for (i, seg) in segs.iter().enumerate() {
                    if matches!(seg, Segment3::Line { .. }) {
                        continue;
                    }
                    n = Some(first_frenet_normal(seg).ok_or_else(|| frenet_flat(i, seg, 0.0))?);
                    break;
                }
                n.or_else(|| seed(up_dir)).or_else(|| seed(profile_x)).unwrap_or_else(|| crate::curve::perpendicular_frame(t0).0)
            }
        };
        let mut current = FrameAt::new(segs[0].start(), t0, n0);
        let mut segments = Vec::with_capacity(segs.len());
        let mut cum = vec![0.0];
        for (i, seg) in segs.iter().enumerate() {
            if !strict && i > 0 {
                // A corner the lenient field allows: the normal carried
                // across it must stay off the new tangent, else the
                // binormal is carried instead (the frame turns about it).
                let t_next = tangents[i].0;
                let n_off = current.n - t_next * current.n.dot(t_next);
                if n_off.length() <= 1e-6 {
                    let b_off = current.b - t_next * current.b.dot(t_next);
                    let n = if b_off.length() > 1e-6 { b_off } else { crate::curve::perpendicular_frame(t_next).0 };
                    current = FrameAt { point: current.point, t: t_next, n: n.normalized(), b: t_next.cross(n.normalized()) };
                }
            }
            let prev_t = (i > 0).then(|| tangents[i - 1].1);
            let sf = segment_frames(seg, i, frame, up_dir, &current, prev_t)?;
            current = *sf.end();
            cum.push(cum[i] + sf.length);
            segments.push(sf);
        }
        Ok(FrameField { frame, up: up_dir, segments, cum })
    }

    /// The frame at the start of the path.
    pub fn start(&self) -> FrameAt {
        *self.segments[0].start()
    }

    /// The frame at the end of the path.
    pub fn end(&self) -> FrameAt {
        *self.segments[self.segments.len() - 1].end()
    }

    /// Total arc length.
    pub fn length(&self) -> f64 {
        self.cum[self.cum.len() - 1]
    }

    /// The frame at parameter `t` of segment `i`.
    pub fn at(&self, path: &Curve, i: usize, t: f64) -> FrameAt {
        self.segments[i].at(&path.segments[i], self.frame, self.up, t)
    }

    /// The fraction of the whole arc length reached at parameter `t` of
    /// segment `i` — what `twist=` and `scale=` are shared out by.
    fn fraction(&self, path: &Curve, i: usize, t: f64) -> f64 {
        let total = self.length();
        if !(total > 0.0) {
            return 0.0;
        }
        let _ = path;
        ((self.cum[i] + self.segments[i].length_to(t)) / total).clamp(0.0, 1.0)
    }
}

/// Arc length from a segment's start at each of `n + 1` uniform parameters,
/// by Gauss–Legendre on each interval, scaled so the last entry is exactly
/// `length` (the segment's own quadrature or closed form).
fn length_table(seg: &Segment3, n: usize, length: f64) -> Vec<f64> {
    let mut out = Vec::with_capacity(n + 1);
    out.push(0.0);
    let mut run = 0.0;
    for k in 0..n {
        let (lo, hi) = (k as f64 / n as f64, (k + 1) as f64 / n as f64);
        let (mid, half) = ((lo + hi) / 2.0, (hi - lo) / 2.0);
        let mut piece = 0.0;
        for (x, w) in crate::sketch::GL5 {
            piece += w * seg.derivatives(mid + half * x).0.length() * half;
        }
        run += piece;
        out.push(run);
    }
    let scale = if run > 0.0 { length / run } else { 1.0 };
    for v in out.iter_mut() {
        *v *= scale;
    }
    if let Some(last) = out.last_mut() {
        *last = length;
    }
    out
}

/// The Frenet normal at the start of a segment, or the first defined one
/// along it when the segment starts straight (a Bézier whose first two
/// control points are collinear with the third); `None` when it never
/// curves.
fn first_frenet_normal(seg: &Segment3) -> Option<Vec3> {
    const N: usize = 256;
    (0..=N).find_map(|k| frenet_normal_at(seg, k as f64 / N as f64))
}

/// The refusal for a Frenet frame on a segment that is straight at `t`.
fn frenet_flat(i: usize, seg: &Segment3, t: f64) -> CadpoleError {
    let p = seg.point_at_param(t);
    CadpoleError::invalid(format!(
        "sweep_along: frame.frenet reads its normal from the path's curvature, and segment {i} ({}) has none \
         at ({}, {}, {}); use frame.rmf, which is defined everywhere",
        seg.kind(),
        crate::context::fmt_num(p.x),
        crate::context::fmt_num(p.y),
        crate::context::fmt_num(p.z)
    ))
}

/// The frames along one segment from the frame at its start.
fn segment_frames(seg: &Segment3, i: usize, frame: Frame, up: Vec3, start: &FrameAt, prev_t: Option<Vec3>) -> Result<SegmentFrames> {
    let length = seg.length();
    let t0 = tangent_at(seg, 0.0).unwrap_or(start.t);
    // A joint off tangent by less than JOINT_TOL but more than the
    // arithmetic (a hand-typed via point): the frame the previous segment
    // ended on is carried into this one as it is, because the wire the
    // exact segments sweep from is the previous segment's far wire, which
    // lies in that frame's plane. Re-basing it on this segment's tangent
    // would place the caps and stations a hair off the wires (by the
    // joint's angle times the profile's reach) and the end cap would not
    // lie on its own boundary. At the first segment, and at a joint tangent
    // to the arithmetic, the frame is re-based on this segment's own
    // tangent (`new` re-orthogonalises).
    // (A pattern's lenient field turns real corners; those are re-based.)
    //
    // The joint is the *path's*: the previous segment's end tangent against
    // this one's start. The carried frame's own tangent drifts from the
    // path's by every carried joint so far, and a drifted frame is never
    // re-based (that would put the caps off the wires again); past
    // JOINT_TOL in total the drift is refused by name, like a corner.
    let drift = angle_between(start.t, t0);
    let joint = prev_t.map_or(drift, |p| angle_between(p, t0));
    let carried = i > 0 && joint <= JOINT_TOL && drift > EXACT_JOINT;
    if carried && drift > JOINT_TOL {
        let p = seg.start();
        return Err(CadpoleError::invalid(format!(
            "sweep_along: the path turns a corner of {:.3}° in total over its near-tangent joints, reaching \
             segment {i} at ({}, {}, {}); a sweep needs a tangent-continuous path, so make the joints \
             tangent (an arc's via point on the lines' tangent) or split the sweep there",
            drift.to_degrees(),
            crate::context::fmt_num(p.x),
            crate::context::fmt_num(p.y),
            crate::context::fmt_num(p.z)
        )));
    }
    let start = if carried { start.moved(seg.start()) } else { FrameAt::new(seg.start(), t0, start.n) };
    match seg {
        Segment3::Line { a, b } => {
            let n = match frame {
                Frame::Fixed => up,
                Frame::Rmf | Frame::Frenet => start.n,
            };
            let f0 = if carried { start } else { FrameAt::new(*a, t0, n) };
            let table = vec![f0, f0.moved(*b)];
            Ok(SegmentFrames { table, lengths: vec![0.0, length], rigid: Some(Rigid::Translation(*b - *a)), length })
        }
        Segment3::Arc { a, b, via } => {
            let Some(arc) = arc3_params(*a, *b, *via) else {
                let f0 = FrameAt::new(*a, t0, start.n);
                return Ok(SegmentFrames {
                    table: vec![f0, f0.moved(*b)],
                    lengths: vec![0.0, length],
                    rigid: Some(Rigid::Translation(*b - *a)),
                    length,
                });
            };
            let axis = arc.normal();
            let n_tab = ((arc.sweep.abs() / max_station_turn()).ceil() as usize).clamp(2, 4096);
            let f0 = match frame {
                Frame::Rmf => start,
                Frame::Frenet => FrameAt::new(*a, t0, frenet_normal_at(seg, 0.0).ok_or_else(|| frenet_flat(i, seg, 0.0))?),
                Frame::Fixed => FrameAt::new(*a, t0, up),
            };
            if frame == Frame::Frenet && angle_between(f0.n, start.n) > JOINT_TOL && i > 0 {
                return Err(frenet_jump(i, seg, &start, &f0));
            }
            // Carried across a near-tangent joint: the arc turns the frame
            // the previous segment ended on (a fixed frame that then fails
            // the rigid test below is skinned, which re-places every
            // station from the frames).
            let f0 = if carried { start } else { f0 };
            let rotation = Rigid::Rotation { centre: arc.centre, axis, angle: arc.sweep };
            let table: Vec<FrameAt> = match frame {
                // Rotation-minimising and Frenet both turn with the arc.
                Frame::Rmf | Frame::Frenet => {
                    (0..=n_tab).map(|k| f0.rotated(arc.centre, axis, arc.sweep * k as f64 / n_tab as f64)).collect()
                }
                Frame::Fixed => (0..=n_tab)
                    .map(|k| {
                        let t = k as f64 / n_tab as f64;
                        FrameAt::new(seg.point_at_param(t), tangent_at(seg, t).unwrap_or(t0), up)
                    })
                    .collect::<Vec<_>>(),
            };
            if frame == Frame::Fixed {
                for f in &table {
                    if (up - f.t * up.dot(f.t)).length() <= 1e-9 {
                        return Err(fixed_along(i, seg, f.point));
                    }
                }
            }
            // Exact only when the frame at the end is the rotated start.
            let expect = f0.rotated(arc.centre, axis, arc.sweep);
            let got = table[table.len() - 1];
            let rigid = ((expect.n - got.n).length() <= RIGID_TOL && (expect.b - got.b).length() <= RIGID_TOL)
                .then_some(rotation);
            let lengths = (0..table.len()).map(|k| length * k as f64 / (table.len() - 1) as f64).collect();
            Ok(SegmentFrames { table, lengths, rigid, length })
        }
        Segment3::Bezier { .. } | Segment3::Helix { .. } => {
            let f0 = match frame {
                Frame::Rmf => start,
                Frame::Frenet => FrameAt::new(seg.start(), t0, first_frenet_normal(seg).ok_or_else(|| frenet_flat(i, seg, 0.0))?),
                Frame::Fixed => FrameAt::new(seg.start(), t0, up),
            };
            if frame == Frame::Frenet && i > 0 && angle_between(f0.n, start.n) > JOINT_TOL {
                return Err(frenet_jump(i, seg, &start, &f0));
            }
            let turn = total_turn(seg);
            let base = ((turn / max_station_turn()).ceil() as usize).clamp(8, 4096);
            let table = match frame {
                Frame::Rmf => {
                    // Double reflection at `base` steps, halved until the
                    // end frame settles.
                    let mut n = base;
                    let mut table = rmf_table(seg, &f0, n);
                    for _ in 0..12 {
                        let finer = rmf_table(seg, &f0, n * 2);
                        let moved = angle_between(table[table.len() - 1].n, finer[finer.len() - 1].n);
                        table = finer;
                        n *= 2;
                        if moved <= RMF_CONVERGENCE {
                            break;
                        }
                    }
                    table
                }
                Frame::Frenet => {
                    let n = base.max(64);
                    let mut table = Vec::with_capacity(n + 1);
                    let mut prev: Option<Vec3> = Some(f0.n);
                    for k in 0..=n {
                        let t = k as f64 / n as f64;
                        let p = seg.point_at_param(t);
                        let tang = tangent_at(seg, t).unwrap_or(t0);
                        // A sample with no curvature — the very point of an
                        // inflection, or a straight run — carries the last
                        // normal; the flip is read between defined normals.
                        let normal = match frenet_normal_at(seg, t) {
                            Some(nrm) => nrm,
                            None => prev.unwrap_or(f0.n),
                        };
                        if let Some(pn) = prev {
                            if normal.dot(pn) < 0.0 {
                                return Err(CadpoleError::invalid(format!(
                                    "sweep_along: frame.frenet flips at an inflection of segment {i} ({}) near \
                                     ({}, {}, {}), where the curvature changes side; use frame.rmf, which has no \
                                     inflections",
                                    seg.kind(),
                                    crate::context::fmt_num(p.x),
                                    crate::context::fmt_num(p.y),
                                    crate::context::fmt_num(p.z)
                                )));
                            }
                        }
                        prev = Some(normal);
                        table.push(FrameAt::new(p, tang, normal));
                    }
                    table
                }
                Frame::Fixed => {
                    let n = base.max(64);
                    let mut table = Vec::with_capacity(n + 1);
                    for k in 0..=n {
                        let t = k as f64 / n as f64;
                        let p = seg.point_at_param(t);
                        let tang = tangent_at(seg, t).unwrap_or(t0);
                        if (up - tang * up.dot(tang)).length() <= 1e-9 {
                            return Err(fixed_along(i, seg, p));
                        }
                        table.push(FrameAt::new(p, tang, up));
                    }
                    table
                }
            };
            let lengths = length_table(seg, table.len() - 1, length);
            Ok(SegmentFrames { table, lengths, rigid: None, length })
        }
    }
}

/// The double-reflection table of a segment at `n` steps.
fn rmf_table(seg: &Segment3, f0: &FrameAt, n: usize) -> Vec<FrameAt> {
    let mut table = Vec::with_capacity(n + 1);
    table.push(*f0);
    let mut f = *f0;
    for k in 1..=n {
        let t = k as f64 / n as f64;
        let p = seg.point_at_param(t);
        let tang = tangent_at(seg, t).unwrap_or(f.t);
        f = rmf_step(&f, p, tang);
        table.push(f);
    }
    table
}

fn frenet_jump(i: usize, seg: &Segment3, from: &FrameAt, to: &FrameAt) -> CadpoleError {
    let p = seg.start();
    CadpoleError::invalid(format!(
        "sweep_along: frame.frenet jumps by {:.2}° at the start of segment {i} ({}) at ({}, {}, {}): the \
         curvature normal arriving there and the one leaving do not agree, so the profile would turn \
         abruptly; use frame.rmf, which is continuous",
        angle_between(from.n, to.n).to_degrees(),
        seg.kind(),
        crate::context::fmt_num(p.x),
        crate::context::fmt_num(p.y),
        crate::context::fmt_num(p.z)
    ))
}

fn fixed_along(i: usize, seg: &Segment3, p: Vec3) -> CadpoleError {
    CadpoleError::invalid(format!(
        "sweep_along: frame.fixed cannot hold up= where the path runs along it, which segment {i} ({}) does at \
         ({}, {}, {}); pick an up= off the path everywhere, or use frame.rmf",
        seg.kind(),
        crate::context::fmt_num(p.x),
        crate::context::fmt_num(p.y),
        crate::context::fmt_num(p.z)
    ))
}

/// How far the tangent turns along a segment, radians, summed over 256
/// samples.
fn total_turn(seg: &Segment3) -> f64 {
    const N: usize = 256;
    let mut turn = 0.0;
    let mut prev = tangent_at(seg, 0.0);
    for k in 1..=N {
        let t = tangent_at(seg, k as f64 / N as f64);
        if let (Some(a), Some(b)) = (prev, t) {
            turn += angle_between(a, b);
        }
        prev = t.or(prev);
    }
    turn
}

/// The largest curvature along a segment, per mm, over 256 samples.
fn max_curvature(seg: &Segment3) -> f64 {
    const N: usize = 256;
    (0..=N)
        .map(|k| {
            let (d1, d2) = seg.derivatives(k as f64 / N as f64);
            let l = d1.length();
            if !(l > 0.0) {
                return 0.0;
            }
            d1.cross(d2).length() / (l * l * l)
        })
        .fold(0.0, f64::max)
}

// --------------------------------------------------------------- placement

/// Where the profile sits at one point of the path: the rigid motion from
/// the start frame to the frame there (about the path's start), then the
/// station's scale about the path point.
#[derive(Clone, Copy, Debug)]
pub struct Placement {
    /// The path point: where the anchor lands.
    pub origin: Vec3,
    /// The images of the world axes under the rotation.
    pub x: Vec3,
    pub y: Vec3,
    pub z: Vec3,
    pub scale: f64,
    /// The path's start point, about which the profile is carried.
    pub anchor: Vec3,
}

impl Placement {
    /// The placement carrying `from` (the start frame) to `to`, with the
    /// profile turned by `twist` about the tangent and scaled by `scale`.
    fn between(from: &FrameAt, to: &FrameAt, twist: f64, scale: f64) -> Placement {
        let to = to.twisted(twist);
        // R = [n b t]·[n0 b0 t0]ᵀ, column by column — except that a frame
        // that has not turned at all (a translation) gets the identity to
        // the bit, so a straight sweep is the extrude's own matrix.
        let same = to.n == from.n && to.b == from.b && to.t == from.t;
        let col = |e: Vec3| if same { e } else { to.n * from.n.dot(e) + to.b * from.b.dot(e) + to.t * from.t.dot(e) };
        Placement { origin: to.point, x: col(Vec3::X), y: col(Vec3::Y), z: col(Vec3::Z), scale, anchor: from.point }
    }

    /// The identity at `at`.
    pub fn identity(at: Vec3) -> Placement {
        Placement { origin: at, x: Vec3::X, y: Vec3::Y, z: Vec3::Z, scale: 1.0, anchor: at }
    }

    pub fn direction(&self, d: Vec3) -> Vec3 {
        self.x * d.x + self.y * d.y + self.z * d.z
    }

    pub fn apply(&self, p: Vec3) -> Vec3 {
        self.origin + self.direction(p - self.anchor) * self.scale
    }

    /// A plane carried by the placement.
    pub fn plane(&self, p: &Plane) -> Plane {
        Plane { origin: self.apply(p.origin), normal: self.direction(p.normal), x_dir: self.direction(p.x_dir) }
    }

    /// As truck's 4×4 (columns: the scaled axes, then the translation).
    pub fn matrix(&self) -> Matrix4 {
        let s = self.scale;
        let t = self.origin - self.direction(self.anchor) * s;
        #[rustfmt::skip]
        let m = Matrix4::new(
            self.x.x * s, self.x.y * s, self.x.z * s, 0.0,
            self.y.x * s, self.y.y * s, self.y.z * s, 0.0,
            self.z.x * s, self.z.y * s, self.z.z * s, 0.0,
            t.x,          t.y,          t.z,          1.0,
        );
        m
    }
}

// ------------------------------------------------------------------ report

/// How one segment of the path was swept.
#[derive(Clone, Copy, Debug, PartialEq)]
pub enum Method {
    /// `tsweep`: exact.
    Translation,
    /// `rsweep` about the arc's axis: exact.
    Rotation,
    /// The ruled surface between the two end sections (a straight segment
    /// with `scale=`): exact.
    Ruled,
    /// A cubic B-spline skin through profile copies at stations: an
    /// approximation, measured.
    Skinned,
}

impl Method {
    pub fn is_exact(self) -> bool {
        !matches!(self, Method::Skinned)
    }

    pub fn name(self) -> &'static str {
        match self {
            Method::Translation => "translation",
            Method::Rotation => "rotation",
            Method::Ruled => "ruled",
            Method::Skinned => "skinned",
        }
    }
}

/// One path segment's row of the report.
#[derive(Clone, Debug, PartialEq)]
pub struct SegmentReport {
    pub index: usize,
    /// `line`, `arc`, `bezier`, `helix`.
    pub kind: &'static str,
    pub method: Method,
    /// Stations the skin was run through (2 for an exact segment).
    pub stations: usize,
    /// The largest measured distance from the skinned surface to the true
    /// sweep, in mm; `None` for an exact segment.
    pub deviation: Option<f64>,
    /// Why the segment was not exact, when it could have been.
    pub reason: Option<String>,
}

/// What the sweep built and how well.
#[derive(Clone, Debug, PartialEq)]
pub struct SweepReport {
    pub segments: Vec<SegmentReport>,
    /// The approximation target every skinned segment was held to, mm.
    pub target: f64,
    /// Total arc length of the path, mm.
    pub length: f64,
}

impl SweepReport {
    /// The largest deviation over every skinned segment; `None` when the
    /// whole sweep is exact.
    pub fn max_deviation(&self) -> Option<f64> {
        self.segments.iter().filter_map(|s| s.deviation).fold(None, |m, d| Some(m.map_or(d, |m: f64| m.max(d))))
    }

    /// Is every segment exact?
    pub fn is_exact(&self) -> bool {
        self.segments.iter().all(|s| s.method.is_exact())
    }

    /// One line per segment kind for the feature tree:
    /// `line: translation (exact); helix: skinned through 541 stations, within 0.0031 mm`.
    pub fn describe(&self) -> String {
        self.segments
            .iter()
            .map(|s| match s.deviation {
                Some(d) => format!(
                    "{}: {} through {} stations, within {:.4} mm{}",
                    s.kind,
                    s.method.name(),
                    s.stations,
                    d,
                    s.reason.as_ref().map(|r| format!(" ({r})")).unwrap_or_default()
                ),
                None => format!("{}: {} (exact)", s.kind, s.method.name()),
            })
            .collect::<Vec<_>>()
            .join("; ")
    }

    /// The kernel note for a sweep that approximated anything: which
    /// segments, how many stations, how far off, against what target.
    pub fn note(&self) -> Option<String> {
        let skinned: Vec<&SegmentReport> = self.segments.iter().filter(|s| !s.method.is_exact()).collect();
        if skinned.is_empty() {
            return None;
        }
        let parts: Vec<String> = skinned
            .iter()
            .map(|s| {
                format!(
                    "segment {} ({}{}) is a NURBS skin through {} stations, measured within {:.4} mm of the true sweep",
                    s.index,
                    s.kind,
                    s.reason.as_ref().map(|r| format!(", {r}")).unwrap_or_default(),
                    s.stations,
                    s.deviation.unwrap_or(0.0)
                )
            })
            .collect();
        let exact: Vec<String> =
            self.segments.iter().filter(|s| s.method.is_exact()).map(|s| format!("{} ({})", s.index, s.kind)).collect();
        let over = self.max_deviation().is_some_and(|d| d > self.target);
        Some(format!(
            "sweep_along: {}; the target was {:.4} mm{}{}",
            parts.join("; "),
            self.target,
            if over { ", which was not reached after refining the stations" } else { "" },
            if exact.is_empty() {
                String::new()
            } else {
                format!("; segment{} {} exact", if exact.len() == 1 { " " } else { "s " }, exact.join(", "))
            }
        ))
    }
}

/// What the namer needs of a sweep: for every path segment, the
/// placements at [`NAMER_FRACTIONS`] along it, plus the two cap planes.
#[derive(Clone, Debug)]
pub struct SweepLayout {
    pub placements: Vec<Vec<Placement>>,
    pub start_plane: Plane,
    pub end_plane: Plane,
}

/// The solids of a sweep, its report, and the layout the namer reads.
#[derive(Clone, Debug)]
pub struct Swept {
    pub solids: Vec<KSolid>,
    pub report: SweepReport,
    pub layout: SweepLayout,
}

// ------------------------------------------------------------------- build

/// Sweep `spec.profile` along `spec.path`: one solid per region of the
/// profile, built as the module doc says.
pub fn sweep(spec: &SweepSpec<'_>) -> Result<Swept> {
    let _span = trace::Span::start(format!(
        "sweep_along {} segments {} loops frame={}",
        spec.path.segments.len(),
        spec.profile.loops.len(),
        spec.frame.name()
    ));
    if !spec.scale.is_finite() || !(spec.scale > 0.0) {
        return Err(CadpoleError::invalid(format!("sweep_along: scale= must be a positive number, got {}", spec.scale)));
    }
    if !spec.twist.is_finite() {
        return Err(CadpoleError::invalid(format!("sweep_along: twist= must be finite, got {}", spec.twist)));
    }
    let regions = spec.profile.regions()?;
    if regions.is_empty() {
        return Err(CadpoleError::invalid("sweep_along: the profile has no closed regions to sweep"));
    }
    let field = FrameField::new(spec.path, spec.frame, spec.up, spec.profile.plane.x_dir)?;
    let start = field.start();
    let path_len = field.length();
    if !(path_len > 0.0) {
        return Err(CadpoleError::invalid("sweep_along: the path has no length"));
    }
    let placement_at = |i: usize, t: f64| -> Placement {
        let f = field.at(spec.path, i, t);
        let frac = field.fraction(spec.path, i, t);
        Placement::between(&start, &f, spec.twist * frac, 1.0 + (spec.scale - 1.0) * frac)
    };
    // How far any point of the profile sits from the anchor: the widest
    // fibre the sweep carries, for the chordal bound.
    let extent = spec
        .profile
        .loops
        .iter()
        .flat_map(|l| l.sample())
        .map(|p| (spec.profile.plane.to_world(p.x, p.y) - start.point).length())
        .fold(0.0f64, f64::max)
        .max(1e-9);
    let target = approximation_target(spec.tolerance);

    // Decide each segment's method once; every region follows the same plan.
    let mut plans: Vec<(Method, usize, Option<String>)> = Vec::with_capacity(spec.path.segments.len());
    for (i, seg) in spec.path.segments.iter().enumerate() {
        let sf = &field.segments[i];
        let twisted = spec.twist != 0.0;
        let scaled = spec.scale != 1.0;
        let plan = match (seg, sf.rigid) {
            (Segment3::Line { .. }, Some(Rigid::Translation(_))) if !twisted && !scaled => (Method::Translation, 2, None),
            (Segment3::Line { .. }, Some(Rigid::Translation(_))) if !twisted => (Method::Ruled, 2, None),
            (Segment3::Arc { .. }, Some(Rigid::Rotation { .. })) if !twisted && !scaled => (Method::Rotation, 2, None),
            (Segment3::Line { .. } | Segment3::Arc { .. }, _) => {
                let reason = if twisted {
                    "twisted".to_owned()
                } else if scaled {
                    "scaled".to_owned()
                } else {
                    format!("the {} frame does not follow the arc", spec.frame.name())
                };
                (Method::Skinned, initial_stations(seg, spec, &field, i, extent, target), Some(reason))
            }
            (Segment3::Bezier { .. } | Segment3::Helix { .. }, _) => {
                (Method::Skinned, initial_stations(seg, spec, &field, i, extent, target), None)
            }
        };
        plans.push(plan);
    }

    let mut solids = Vec::with_capacity(regions.len());
    let mut report: Option<SweepReport> = None;
    for region in &regions {
        let mut loops = vec![&region.outer];
        loops.extend(region.holes.iter());
        let start_wires: Vec<Wire> =
            loops.iter().map(|l| kernel::loop_to_wire(l, &spec.profile.plane)).collect::<Result<_>>()?;
        let (solid, rep) = guarded(
            "sweep_along",
            std::panic::AssertUnwindSafe(|| build_region(spec, &field, &placement_at, &start_wires, &plans, target)),
        )??;
        solids.push(solid);
        // The report is the same for every region (same path, same plan);
        // the largest deviation over the regions is kept.
        report = Some(match report {
            None => rep,
            Some(mut r) => {
                for (a, b) in r.segments.iter_mut().zip(&rep.segments) {
                    if let (Some(x), Some(y)) = (a.deviation, b.deviation) {
                        a.deviation = Some(x.max(y));
                    }
                    a.stations = a.stations.max(b.stations);
                }
                r
            }
        });
    }
    let report = report.expect("at least one region");
    let placements: Vec<Vec<Placement>> = (0..spec.path.segments.len())
        .map(|i| NAMER_FRACTIONS.iter().map(|&f| placement_at(i, f)).collect())
        .collect();
    let last = spec.path.segments.len() - 1;
    let layout = SweepLayout {
        placements,
        start_plane: spec.profile.plane,
        end_plane: placement_at(last, 1.0).plane(&spec.profile.plane),
    };
    Ok(Swept { solids, report, layout })
}

/// The first station count for a skinned segment, from two bounds:
/// the frame may turn by at most the normal bound between stations, and
/// the cubic interpolant's chordal bound on the widest fibre must be within
/// the target. Cubic spline interpolation of a curve of radius `ρ` at a
/// step of `θ` radians errs by at most `(5/384)·ρ·θ⁴` (the complete cubic
/// spline bound with the fourth derivative `ρ·ω⁴` of a fibre turning at
/// rate `ω`), so `θ ≤ (384·target / (5·ρ))^¼`.
fn initial_stations(seg: &Segment3, spec: &SweepSpec<'_>, field: &FrameField, i: usize, extent: f64, target: f64) -> usize {
    let turn = total_turn(seg) + spec.twist.abs() * (field.segments[i].length / field.length().max(1e-12));
    let kappa = max_curvature(seg);
    let rho = if kappa > 0.0 { 1.0 / kappa } else { 0.0 } + extent;
    let by_turn = (turn / max_station_turn()).ceil() as usize;
    let theta_chord = fmath::powf(384.0 * target / (5.0 * rho.max(1e-9)), 0.25);
    let by_chord = (turn / theta_chord).ceil() as usize;
    by_turn.max(by_chord).clamp(2, 4096)
}

/// One region's solid: the wires swept segment by segment, the far wire of
/// each segment feeding the next, the two ends capped, the shell closed
/// and turned outward by its volume.
fn build_region(
    spec: &SweepSpec<'_>,
    field: &FrameField,
    placement_at: &dyn Fn(usize, f64) -> Placement,
    start_wires: &[Wire],
    plans: &[(Method, usize, Option<String>)],
    target: f64,
) -> Result<(KSolid, SweepReport)> {
    let mut shell = Shell::new();
    // The start cap first, inverted, then the sides, then the end cap: the
    // order truck's own `Face::sweep` lays a solid out in, so a straight
    // sweep is the extrude face for face. The side faces use the start
    // wires forwards and the end wires backwards (`connect_wires`, and the
    // loft's layout), which is what the two caps' directions answer to.
    let start_cap = builder::try_attach_plane(start_wires)
        .map_err(|e| CadpoleError::kernel(format!("sweep_along: the start cap: {e}")))?;
    let start_surface = start_cap.surface();
    shell.push(start_cap.inverse());
    let mut current: Vec<Wire> = start_wires.to_vec();
    let mut segments = Vec::with_capacity(plans.len());
    for (i, seg) in spec.path.segments.iter().enumerate() {
        let (method, stations, reason) = &plans[i];
        let (faces, far, stations_used, deviation) = match method {
            Method::Translation => {
                let Some(Rigid::Translation(v)) = field.segments[i].rigid else { unreachable!() };
                let mut faces = Vec::new();
                let mut far = Vec::with_capacity(current.len());
                for w in &current {
                    let swept: Shell = builder::tsweep(w, v.vector());
                    far.push(far_wire(&swept, w, i)?);
                    faces.extend(swept.face_iter().cloned());
                }
                (faces, far, 2, None)
            }
            Method::Rotation => {
                let Some(Rigid::Rotation { centre, axis, angle }) = field.segments[i].rigid else { unreachable!() };
                // In pieces of at most a quarter turn, each an exact NURBS
                // surface of revolution; see `revolved_faces` for why not
                // truck's `rsweep`.
                let pieces = ((angle.abs() / std::f64::consts::FRAC_PI_2) - 1e-9).ceil().max(1.0) as usize;
                let mut faces = Vec::new();
                let mut from = current.clone();
                for _ in 0..pieces {
                    let (piece_faces, far) = revolved_faces(&from, centre, axis, angle / pieces as f64)
                        .map_err(|e| CadpoleError::kernel(format!("sweep_along: segment {i}: {e}")))?;
                    faces.extend(piece_faces);
                    from = far;
                }
                (faces, from, 2, None)
            }
            Method::Ruled => {
                // Every placement is from the path's start, so every placed
                // wire is the profile *as drawn* under it; only the shared
                // station 0 is the previous segment's far wire.
                let end = placement_at(i, 1.0);
                let far: Vec<Wire> = start_wires.iter().map(|w| builder::transformed(w, end.matrix())).collect();
                let faces = kernel::skin_through_wires(&[current.clone(), far.clone()], 1)
                    .map_err(|e| CadpoleError::kernel(format!("sweep_along: segment {i}: {e}")))?;
                (faces, far, 2, None)
            }
            Method::Skinned => {
                let mut k = (*stations).max(2);
                let mut best: Option<(Vec<Face>, Vec<Wire>, usize, f64)> = None;
                for round in 0..=MAX_REFINEMENTS {
                    // Every placement is from the path's start, so every
                    // placed wire is the profile as drawn under it; only
                    // the shared station 0 is the previous face's far wire.
                    let chunks = k.div_ceil(MAX_INTERVALS_PER_FACE);
                    let mut faces = Vec::new();
                    let mut chunk_start = current.clone();
                    let mut deviation: f64 = 0.0;
                    for c in 0..chunks {
                        let (s0, s1) = (c * k / chunks, (c + 1) * k / chunks);
                        let mut wires: Vec<Vec<Wire>> = Vec::with_capacity(s1 - s0 + 1);
                        wires.push(chunk_start);
                        for st in s0 + 1..=s1 {
                            let p = placement_at(i, st as f64 / k as f64);
                            wires.push(start_wires.iter().map(|w| builder::transformed(w, p.matrix())).collect());
                        }
                        let chunk_faces = kernel::skin_through_wires(&wires, crate::skin::MAX_V_DEGREE)
                            .map_err(|e| CadpoleError::kernel(format!("sweep_along: segment {i}: {e}")))?;
                        deviation = deviation.max(measure_deviation(&chunk_faces, start_wires, i, k, s0, s1, placement_at));
                        chunk_start = wires.pop().expect("the last station");
                        faces.extend(chunk_faces);
                    }
                    let done = deviation <= target || round == MAX_REFINEMENTS;
                    best = Some((faces, chunk_start, k + 1, deviation));
                    if done {
                        break;
                    }
                    k *= 2;
                }
                let (faces, far, used, dev) = best.expect("a skin");
                (faces, far, used, Some(dev))
            }
        };
        shell.extend(faces);
        current = far;
        segments.push(SegmentReport {
            index: i,
            kind: seg.kind(),
            method: *method,
            stations: stations_used,
            deviation,
            reason: reason.clone(),
        });
    }
    // The end cap is the start cap's plane carried to the end of the path
    // — the way truck's sweeps map their ceiling — on the far wires the
    // last segment's faces already use.
    let last = spec.path.segments.len() - 1;
    let end_surface = end_cap_surface(&start_surface, &placement_at(last, 1.0));
    let end_cap = Face::try_new(current.clone(), end_surface)
        .map_err(|e| CadpoleError::kernel(format!("sweep_along: the end cap: {e}")))?;
    shell.push(end_cap);
    if shell.shell_condition() != ShellCondition::Closed {
        return Err(CadpoleError::kernel(format!(
            "sweep_along: the swept shell is {:?}, not closed; the profile may cross itself along the path \
             (a profile wider than the path's radius of curvature turns inside out)",
            shell.shell_condition()
        )));
    }
    let mut solid = Solid::try_new(vec![shell])
        .map_err(|e| CadpoleError::kernel(format!("sweep_along did not close into a solid ({e})")))?;
    // Which way is out. The whole shell is oriented consistently with the
    // profile wires' winding (truck's `connect_wires`, the loft's layout
    // and the two caps all follow it), so one sign decides: the start
    // cap's plane normal, which follows the winding, against the path's
    // direction there. Along the sweep means the un-inverted cap would
    // face into the solid, so the inverted one faces out and the shell is
    // outward as built; against it, the shell is inside out. This is
    // `extrude`'s own rule (a face swept against its normal is inverted
    // first), decided here after the fact, and it costs nothing — the
    // loft's volume sign would tessellate the whole solid.
    let outward = match start_surface {
        Surface::Plane(p) => kernel::vector_to_vec3(p.normal()).dot(field.start().t) > 0.0,
        _ => kernel::soup_volume(&solid, 0.25)? > 0.0,
    };
    if !outward {
        solid.not();
    }
    Ok((solid, SweepReport { segments, target, length: field.length() }))
}

/// The faces of `wires` turned by `angle` (at most a half turn, in practice
/// a quarter) about the axis through `centre` along the unit vector `axis`,
/// and the turned wires, sharing every vertex and edge the way a loft band
/// does.
///
/// Each face is the **exact NURBS surface of revolution** of its edge's
/// curve (Piegl & Tiller A8.1): the curve's homogeneous control points are
/// each swung into a rational quadratic arc — the point, its tangent
/// intersection at half the angle with weight `cos(angle/2)`, and the point
/// turned by the whole angle — over the knots `[0,0,0,1,1,1]`, so the
/// surface *is* the torus, cone or cylinder to the bit the arithmetic
/// allows, in the same representation the loft's cones use. Not truck's
/// `rsweep`: that hands back `RevolutedCurve` surfaces, and
/// `truck_shapeops` refuses to intersect those with a plane in the very
/// configuration a chamfer cutter meets — a ring revolved about a vertical
/// axis against a horizontal face, "the boundaries meet, but not
/// transversally", for a ring built by Cadpole's own `revolve` just the same
/// (`which_booleans_a_cutter_survives` prints the census). As NURBS patches the same ring subtracts.
///
/// Layout as truck's sweeps and the loft's bands: the curve along `u`, the
/// turn along `v`, the boundary the start edge forwards, the far vertex's
/// arc, the turned edge backwards and the start vertex's arc backwards. The
/// vertex arcs are the surface's own `u = 0` and `u = 1` rows as curves, so
/// the two faces meeting on one are identical there, and they are shared
/// through the vertex pair exactly as [`crate::kernel`]'s `spoke`.
fn revolved_faces(wires: &[Wire], centre: Vec3, axis: Vec3, angle: f64) -> Result<(Vec<Face>, Vec<Wire>)> {
    use std::collections::HashMap;
    use truck_modeling::{BSplineCurve, BSplineSurface, Curve as TCurve, Edge, KnotVec, NurbsCurve, NurbsSurface, Vector4, VertexID};
    let whole = crate::geom::Transform::rotation(centre, axis, angle);
    let half = crate::geom::Transform::rotation(centre, axis, angle / 2.0);
    let (_, w_mid) = fmath::sin_cos(angle / 2.0);
    if !(w_mid > 1e-9) {
        return Err(CadpoleError::kernel("a revolved piece of half a turn or more"));
    }
    // A homogeneous control point swung through the angle: the three
    // control points of its arc, with their weights.
    let swing = |p: Vector4| -> [Vector4; 3] {
        let w = p.w;
        let e = Vec3::new(p.x / w, p.y / w, p.z / w);
        let d = e - centre;
        let along = d.dot(axis);
        let radial = d - axis * along;
        if radial.length() < 1e-12 {
            return [p, p, p];
        }
        let foot = centre + axis * along;
        let q1 = foot + (half.point(e) - foot) * (1.0 / w_mid);
        let q2 = whole.point(e);
        let wm = w * w_mid;
        [p, Vector4::new(q1.x * wm, q1.y * wm, q1.z * wm, wm), Vector4::new(q2.x * w, q2.y * w, q2.z * w, w)]
    };
    let far_wires: Vec<Wire> = wires.iter().map(|w| builder::transformed(w, whole.0)).collect();
    let mut spokes: HashMap<(VertexID, VertexID), Edge> = HashMap::new();
    let mut faces = Vec::new();
    for (w, far) in wires.iter().zip(&far_wires) {
        for (edge, far_edge) in w.edge_iter().zip(far.edge_iter()) {
            let curve = edge.oriented_curve().lift_up();
            let rows: Vec<[Vector4; 3]> = curve.control_points().iter().map(|p| swing(*p)).collect();
            // The surface with the turn along `u` and the profile along `v`
            // — the loft's cone layout, which is what `cone_of` and
            // `revolution_of` read and what truck's boolean trims
            // — so the control net is the swing rows
            // transposed. The boundary is then written counter-clockwise in
            // that `(u, v)` and the face inverted, exactly as truck's own
            // `connect_edges` does for an inverted edge, which leaves the
            // oriented boundary running the start edge forwards.
            let net: Vec<Vec<Vector4>> = (0..3).map(|j| rows.iter().map(|r| r[j]).collect()).collect();
            let surface = BSplineSurface::try_new((KnotVec::bezier_knot(2), curve.knot_vec().clone()), net)
                .map_err(|e| CadpoleError::kernel(format!("revolved face: {e}")))?;
            let mut spoke = |from: &truck_modeling::Vertex, to: &truck_modeling::Vertex, row: [Vector4; 3]| -> Edge {
                spokes
                    .entry((from.id(), to.id()))
                    .or_insert_with(|| {
                        let arc = BSplineCurve::new(KnotVec::bezier_knot(2), row.to_vec());
                        Edge::new(from, to, TCurve::NurbsCurve(NurbsCurve::new(arc)))
                    })
                    .clone()
            };
            let front = spoke(edge.front(), far_edge.front(), rows[0]);
            let back = spoke(edge.back(), far_edge.back(), rows[rows.len() - 1]);
            // Counter-clockwise in (u = turn, v = profile): the front spoke
            // (v = 0, u rising), the far edge (u = 1, v rising), the back
            // spoke backwards, the start edge backwards.
            let ccw: Wire = vec![front.clone(), far_edge.clone(), back.inverse(), edge.inverse()].into();
            let mut face = Face::try_new(vec![ccw], Surface::NurbsSurface(NurbsSurface::new(surface)))
                .map_err(|e| CadpoleError::kernel(format!("revolved face: {e}")))?;
            face.invert();
            faces.push(face);
        }
    }
    Ok((faces, far_wires))
}

/// The start cap's plane under the end placement (truck's `Transformed`),
/// which is what the end of the path is: the profile plane, moved.
fn end_cap_surface(start: &Surface, end: &Placement) -> Surface {
    use truck_modeling::Transformed;
    start.transformed(end.matrix())
}

/// The boundary of a swept shell that is not the wire it was swept from,
/// turned to run the way the start wire does — the wire the next segment
/// is swept from, sharing every edge with the faces just built.
fn far_wire(swept: &Shell, start: &Wire, i: usize) -> Result<Wire> {
    let start_ids: Vec<_> = start.vertex_iter().map(|v| v.id()).collect();
    let boundaries = swept.extract_boundaries();
    let far = boundaries
        .into_iter()
        .find(|w| w.vertex_iter().all(|v| !start_ids.contains(&v.id())))
        .ok_or_else(|| CadpoleError::kernel(format!("sweep_along: segment {i} swept to no far boundary")))?;
    if far.len() != start.len() {
        return Err(CadpoleError::kernel(format!(
            "sweep_along: segment {i}'s far boundary has {} edges against the profile's {}",
            far.len(),
            start.len()
        )));
    }
    // The faces use the far edges inverted, so the boundary came out
    // backwards; the same start vertex ordering is restored by inverting
    // and rotating to the vertex that maps from the start wire's first.
    let far = far.inverse();
    let want = start.front_vertex().expect("a non-empty wire").point();
    let mut best = 0;
    let mut best_d = f64::INFINITY;
    for (k, e) in far.edge_iter().enumerate() {
        let d = (e.front().point() - want).magnitude();
        if d < best_d {
            best_d = d;
            best = k;
        }
    }
    let edges: Vec<truck_modeling::Edge> = far.edge_iter().cloned().collect();
    let rotated: Vec<truck_modeling::Edge> = edges[best..].iter().chain(edges[..best].iter()).cloned().collect();
    Ok(Wire::from(rotated))
}

/// The largest distance from the skinned faces of one segment to the true
/// sweep: each face sampled between its stations, and each sample's
/// distance to the profile (`profile_wires`, as drawn) carried by the exact
/// frame minimised along the path near it. An upper bound on the true
/// distance, measured rather than bounded, on up to 48
/// station intervals of the segment.
fn measure_deviation(
    faces: &[Face],
    profile_wires: &[Wire],
    i: usize,
    stations: usize,
    s0: usize,
    s1: usize,
    placement_at: &dyn Fn(usize, f64) -> Placement,
) -> f64 {
    const U_SAMPLES: [f64; 5] = [0.1, 0.3, 0.5, 0.7, 0.9];
    const V_PER_INTERVAL: usize = 3;
    const MAX_INTERVALS: usize = 48;
    let edges: Vec<truck_modeling::Edge> = profile_wires.iter().flat_map(|w| w.edge_iter().cloned()).collect();
    let span = (s1 - s0).max(1);
    let stride = span.div_ceil(MAX_INTERVALS).max(1);
    let mut worst: f64 = 0.0;
    for (face, edge) in faces.iter().zip(&edges) {
        let surface = face.surface();
        let ((u0, u1), (v0, v1)) = match &surface {
            Surface::NurbsSurface(ns) => ns.range_tuple(),
            Surface::BSplineSurface(bs) => bs.range_tuple(),
            _ => continue,
        };
        let curve = edge.oriented_curve();
        let (c0, c1) = curve.range_tuple();
        for k in (s0..s1).step_by(stride) {
            for j in 0..V_PER_INTERVAL {
                // Where in the face's v range this interval sits (the
                // face spans stations s0..s1), and where along the path.
                let within = (k - s0) as f64 + (j as f64 + 0.5) / V_PER_INTERVAL as f64;
                let v = v0 + (v1 - v0) * within / span as f64;
                let frac = (k as f64 + (j as f64 + 0.5) / V_PER_INTERVAL as f64) / stations as f64;
                for &su in &U_SAMPLES {
                    let u = u0 + (u1 - u0) * su;
                    let q = kernel::point_to_vec3(surface.subs(u, v));
                    let p = kernel::point_to_vec3(curve.subs(c0 + (c1 - c0) * su));
                    let dist = |s: f64| (placement_at(i, s.clamp(0.0, 1.0)).apply(p) - q).length();
                    // Golden-section search over the neighbouring intervals.
                    let (mut lo, mut hi) = ((k as f64 - 1.0) / stations as f64, (k as f64 + 2.0) / stations as f64);
                    lo = lo.max(0.0);
                    hi = hi.min(1.0);
                    let phi = 0.618_033_988_749_895;
                    let (mut a, mut b) = (hi - phi * (hi - lo), lo + phi * (hi - lo));
                    let (mut fa, mut fb) = (dist(a), dist(b));
                    for _ in 0..40 {
                        if fa < fb {
                            hi = b;
                            b = a;
                            fb = fa;
                            a = hi - phi * (hi - lo);
                            fa = dist(a);
                        } else {
                            lo = a;
                            a = b;
                            fa = fb;
                            b = lo + phi * (hi - lo);
                            fb = dist(b);
                        }
                    }
                    let d = fa.min(fb).min(dist(frac));
                    if d.is_finite() && d > worst {
                        worst = d;
                    }
                }
            }
        }
    }
    worst
}

/// The `Curve` attributes `start_plane` / `end_plane`: the planes
/// perpendicular to the path at its two ends under the rotation-minimising
/// frame, x direction the frame's normal, seeded by `up` when given.
pub fn end_planes(path: &Curve, up: Option<Vec3>) -> Result<(Plane, Plane)> {
    let t0 = path.segments.first().and_then(|s| tangent_at(s, 0.0)).unwrap_or(Vec3::Z);
    let seed = crate::curve::perpendicular_frame(t0).0;
    let field = FrameField::new(path, Frame::Rmf, up, seed)?;
    Ok((field.start().plane(), field.end().plane()))
}

/// The number of turns a helix segment makes, for reports.
pub fn helix_turns(seg: &Segment3) -> Option<f64> {
    match seg {
        Segment3::Helix { angle, .. } => Some(angle.abs() / TAU),
        _ => None,
    }
}

/// A recognised surface of revolution among a sweep's faces is what the
/// tests read off an arc segment: nothing here, the classifier is
/// [`crate::surface_class`]; this re-export keeps the test module short.
#[cfg(test)]
pub(crate) fn classes(s: &KSolid) -> Vec<crate::surface_class::SurfaceClass> {
    crate::primitives::classes_of(s)
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::sketch::{Loop, P2};
    use crate::surface_class::SurfaceClass;

    fn v(x: f64, y: f64, z: f64) -> Vec3 {
        Vec3::new(x, y, z)
    }

    fn spec<'a>(profile: &'a Sketch, path: &'a Curve, frame: Frame) -> SweepSpec<'a> {
        SweepSpec { profile, path, frame, up: None, twist: 0.0, scale: 1.0, tolerance: 0.05 }
    }

    /// Volume, watertightness, face count and fold count of a solid, from
    /// the mesh every consumer sees.
    fn measure(s: &KSolid) -> (f64, bool, usize, usize) {
        let (mesh, topo) = crate::tessellate::tessellate(s, 0.01).expect("tessellation");
        let welded = mesh.welded(1e-4);
        let report = crate::solid::validate(&mesh);
        (welded.volume().abs(), welded.is_watertight() && report.is_solid(), topo.faces.len(), report.folds.len())
    }

    /// A rectangle on XY swept straight up +Z is the extrude of the same
    /// sketch, to the bit: the same face count, the same mesh positions and
    /// triangles, the same volume.
    #[test]
    fn a_straight_sweep_is_the_extrude_bit_for_bit() {
        let sk = Sketch::new(Plane::XY)
            .with_loop(Loop::rounded_rect(P2::new(1.0, 2.0), 10.0, 6.0, 1.5))
            .with_loop(Loop::circle(P2::new(1.0, 2.0), 1.0));
        let path = Curve::new(v(0.0, 0.0, 0.0)).line_to(v(0.0, 0.0, 12.0)).unwrap();
        let swept = sweep(&spec(&sk, &path, Frame::Rmf)).unwrap();
        assert_eq!(swept.solids.len(), 1);
        assert!(swept.report.is_exact());
        assert_eq!(swept.report.segments[0].method, Method::Translation);
        let extruded = kernel::extrude(&sk, 12.0).unwrap().remove(0);
        let (ma, ta) = crate::tessellate::tessellate(&swept.solids[0], 0.02).unwrap();
        let (mb, tb) = crate::tessellate::tessellate(&extruded, 0.02).unwrap();
        assert_eq!(ta.faces.len(), tb.faces.len());
        assert_eq!(ma.positions, mb.positions, "the same mesh, to the bit");
        assert_eq!(ma.indices, mb.indices);
        assert_eq!(ma.welded(1e-4).volume(), mb.welded(1e-4).volume());
    }

    /// A circle swept a quarter turn about an axis is a quarter of a torus:
    /// its side faces are recognised as tori of the circle's radius about
    /// the arc's centre, and its volume is `π r² · (π/2) R` (Pappus).
    #[test]
    fn a_quarter_arc_sweep_of_a_circle_is_a_torus_quarter() {
        let (r, big) = (2.0, 20.0);
        // The profile in the XZ plane at (R, 0, 0); the path an arc about +Z.
        let plane = Plane::new(v(big, 0.0, 0.0), v(0.0, 1.0, 0.0), v(1.0, 0.0, 0.0)).unwrap();
        let sk = Sketch::new(plane).with_loop(Loop::circle(P2::new(0.0, 0.0), r));
        let (s, c) = fmath::sin_cos(std::f64::consts::FRAC_PI_4);
        let path = Curve::new(v(big, 0.0, 0.0)).arc_to(v(0.0, big, 0.0), v(big * c, big * s, 0.0)).unwrap();
        for frame in [Frame::Rmf, Frame::Frenet] {
            let swept = sweep(&spec(&sk, &path, frame)).unwrap();
            assert!(swept.report.is_exact(), "{:?}", swept.report);
            assert_eq!(swept.report.segments[0].method, Method::Rotation);
            let (vol, solid, faces, folds) = measure(&swept.solids[0]);
            // The chords of a 0.01 mm tessellation sit inside a 2 mm tube by
            // two thirds of the sag relative to the radius: a third of a
            // percent, measured 0.34 %.
            let want = std::f64::consts::PI * r * r * std::f64::consts::FRAC_PI_2 * big;
            assert!((vol - want).abs() < 0.005 * want, "{frame:?}: volume {vol} vs {want}");
            assert!(solid && folds == 0);
            // Two profile arcs, each one quarter-turn patch, plus two caps.
            assert_eq!(faces, 4);
            let tori: Vec<_> = classes(&swept.solids[0])
                .into_iter()
                .filter(|c| matches!(c, SurfaceClass::Torus { major, minor, .. } if (major - big).abs() < 1e-6 && (minor - r).abs() < 1e-6))
                .collect();
            assert_eq!(tori.len(), 2, "{:?}", classes(&swept.solids[0]));
        }
        // Under a fixed frame whose up is along the arc's axis the frame
        // follows the arc and the sweep is still the rotation; with up in
        // the arc's plane it is not, and the segment is skinned instead.
        let mut fixed = spec(&sk, &path, Frame::Fixed);
        fixed.up = Some(Vec3::Z);
        assert_eq!(sweep(&fixed).unwrap().report.segments[0].method, Method::Rotation);
        // Up in the arc's plane is the rotation again (the only in-plane
        // direction perpendicular to the tangent is the one the arc turns
        // with); up out of the plane and off the axis is not.
        fixed.up = Some(Vec3::new(1.0, 1.0, 0.0));
        assert_eq!(sweep(&fixed).unwrap().report.segments[0].method, Method::Rotation);
        fixed.up = Some(Vec3::new(0.0, 1.0, 1.0));
        let rep = sweep(&fixed).unwrap().report;
        assert_eq!(rep.segments[0].method, Method::Skinned);
        assert!(rep.segments[0].reason.as_deref().unwrap().contains("fixed frame"));
    }

    /// A helix sweep of a circle is skinned: its volume is the profile's
    /// area times the path's length within the approximation target, the
    /// measured deviation is under the target, and the mesh is a clean
    /// solid.
    #[test]
    fn a_helix_sweep_of_a_circle_is_within_its_target() {
        let (wire, coil, pitch, turns) = (1.5, 20.0, 6.0, 2.0);
        let path = Curve::helix(Vec3::ZERO, Vec3::Z, coil / 2.0, pitch, turns, true, 0.0, 0.0).unwrap();
        let (start_plane, _) = end_planes(&path, None).unwrap();
        let sk = Sketch::new(start_plane).with_loop(Loop::circle(P2::new(0.0, 0.0), wire));
        let t = std::time::Instant::now();
        let swept = sweep(&spec(&sk, &path, Frame::Rmf)).unwrap();
        eprintln!("helix sweep: {:?} in {:?}", swept.report.describe(), t.elapsed());
        let rep = &swept.report.segments[0];
        assert_eq!(rep.method, Method::Skinned);
        let dev = rep.deviation.unwrap();
        assert!(dev <= swept.report.target, "deviation {dev} over the target {}", swept.report.target);
        assert!(rep.stations > 8);
        let (vol, solid, _, folds) = measure(&swept.solids[0]);
        let want = std::f64::consts::PI * wire * wire * path.length();
        assert!((vol - want).abs() < 0.005 * want, "volume {vol} vs π r² L = {want}");
        assert!(solid, "a solid");
        assert_eq!(folds, 0);
        assert!(swept.report.note().unwrap().contains("NURBS skin"));
        // Frenet on a helix is the screw motion: the same tube, exactly as
        // good, and a round wire cannot tell the two apart.
        let frenet = sweep(&spec(&sk, &path, Frame::Frenet)).unwrap();
        let (v2, _, _, _) = measure(&frenet.solids[0]);
        assert!((v2 - vol).abs() < 0.002 * vol, "{v2} vs {vol}");
    }

    /// The rotation-minimising frame has no twist on a planar path: the
    /// binormal stays the plane's normal all along an S-shaped spline, and a
    /// square swept along it arrives as a square with the same orientation
    /// relative to the path (to 1e-9).
    #[test]
    fn rmf_has_no_twist_on_a_planar_path() {
        let path = Curve::new(v(0.0, 0.0, 0.0))
            .spline_through(&[v(10.0, 8.0, 0.0), v(20.0, -8.0, 0.0), v(30.0, 6.0, 0.0), v(40.0, 0.0, 0.0)], 0.5)
            .unwrap();
        let field = FrameField::new(&path, Frame::Rmf, None, Vec3::Y).unwrap();
        for (i, sf) in field.segments.iter().enumerate() {
            for f in &sf.table {
                assert!((f.b.dot(Vec3::Z).abs() - 1.0).abs() < 1e-9, "segment {i}: binormal {:?} left the plane", f.b);
                assert!(f.n.z.abs() < 1e-9);
            }
        }
        let end = field.end();
        assert!((end.point - v(40.0, 0.0, 0.0)).length() < 1e-9);
        // A square in the plane perpendicular to the start tangent, its
        // sides along the frame's normal and binormal, ends up with its
        // sides along the end frame's normal and binormal.
        let start = field.start();
        let sk = Sketch::new(start.plane()).with_loop(Loop::rect(P2::new(0.0, 0.0), 4.0, 4.0));
        let swept = sweep(&spec(&sk, &path, Frame::Rmf)).unwrap();
        let end_plane = swept.layout.end_plane;
        assert!((end_plane.normal - end.t).length() < 1e-9);
        assert!((end_plane.x_dir - end.n).length() < 1e-9);
        assert!((end_plane.y_dir() - end.b).length() < 1e-9);
        let (_, solid, _, folds) = measure(&swept.solids[0]);
        assert!(solid && folds == 0);
        // The end cap's vertices are the square's corners in the end frame.
        let last = swept.solids[0].face_iter().last().unwrap();
        let corners: Vec<Vec3> = last.vertex_iter().map(|vx| kernel::point_to_vec3(vx.point())).collect();
        for c in &corners {
            let d = *c - end.point;
            assert!(d.dot(end.t).abs() < 1e-9, "a corner left the end plane by {}", d.dot(end.t));
            assert!((d.dot(end.n).abs() - 2.0).abs() < 1e-9 && (d.dot(end.b).abs() - 2.0).abs() < 1e-9, "{d:?}");
        }
    }

    /// A chamfer cutter's planar case: a triangle swept along a planar
    /// arc and along a planar spline, each subtracted from a plate. The
    /// cutter is a valid, outward solid for both; the spline cutter
    /// subtracts on the exact path at the default boolean tolerance, and
    /// both subtract on the mesh path. The arc cutter's exact subtraction
    /// is truck's `RevolutedCurve`-against-plane intersection, which
    /// refuses this configuration ("the boundaries meet, but not
    /// transversally") for a ring made by `revolve` about the same axis
    /// just the same — `which_booleans_a_cutter_survives` prints the
    /// census — so that one is reported rather than required.
    #[test]
    fn a_triangle_swept_along_a_planar_edge_is_a_valid_cutter() {
        let plate = kernel::extrude(&Sketch::new(Plane::XY).with_loop(Loop::rect(P2::new(20.0, 10.0), 40.0, 20.0)), 6.0)
            .unwrap()
            .remove(0);
        // The paths overrun the plate by a millimetre at each end, so the
        // cutter's caps sit in air and nothing is coplanar with the plate's
        // sides.
        let paths = [
            Curve::new(v(-1.0, 4.0, 6.0)).arc_to(v(41.0, 4.0, 6.0), v(20.0, 12.0, 6.0)).unwrap(),
            Curve::new(v(-1.0, 4.0, 6.0)).spline_through(&[v(13.0, 9.0, 6.0), v(27.0, 1.0, 6.0), v(41.0, 6.0, 6.0)], 0.5).unwrap(),
        ];
        for (k, path) in paths.iter().enumerate() {
            let (start_plane, _) = end_planes(path, Some(Vec3::Z)).unwrap();
            // The chamfer section in the normal plane at the start: a
            // triangle 2 mm into the top face and 2 mm down the side,
            // pushed 0.2 mm out past the surfaces so nothing is coplanar.
            let tri = Loop::polygon(&[P2::new(-2.0, 0.2), P2::new(0.2, 0.2), P2::new(0.2, -2.0)]);
            let sk = Sketch::new(start_plane).with_loop(tri);
            let swept = sweep(&spec(&sk, path, Frame::Rmf)).unwrap();
            let (vol, solid, _, folds) = measure(&swept.solids[0]);
            assert!(solid && folds == 0, "path {k}: the cutter is not a solid");
            assert!(vol > 0.0);
            let (mesh, _) = crate::tessellate::tessellate(&swept.solids[0], 0.05).unwrap();
            assert!(mesh.welded(1e-4).volume() > 0.0, "path {k}: the cutter is inside out");
            let (pv, _, _, _) = measure(&plate);
            match kernel::boolean(kernel::BoolOp::Subtract, &plate, &swept.solids[0], 0.1) {
                Ok(cut) => {
                    // The boolean's trimmed faces are measured at the
                    // document's default tolerance, as every consumer
                    // sees them. The arc cutter's result is the one
                    // truck's face triangulator left open along the
                    // trimmed cylinder and cone; the edge-first
                    // tessellator closes it by construction and, with the
                    // parameter domain scaled towards an isometry before
                    // the Delaunay step, without a chord of the wall
                    // sagging through the cone beside it. So the
                    // exact result meshes as a solid on every path.
                    let (mesh, _) = crate::tessellate::tessellate(&cut, 0.05).unwrap_or_else(|e| panic!("path {k}: the exact result did not mesh: {e}"));
                    let welded = mesh.welded(1e-4);
                    let cv = welded.volume();
                    let report = crate::solid::validate(&mesh);
                    assert!(welded.is_watertight() && report.is_solid(), "path {k}: the chamfered plate is not a solid: {}", report.summary());
                    assert!(cv < pv && cv > 0.9 * pv, "path {k}: {cv} of {pv}");
                }
                Err(e) => {
                    assert_eq!(k, 0, "the spline cutter subtracts on the exact path: {e}");
                    assert!(e.to_string().contains("not transversally"), "{e}");
                }
            }
            let a = crate::tessellate::tessellate(&plate, 0.05).unwrap();
            let b = crate::tessellate::tessellate(&swept.solids[0], 0.05).unwrap();
            let (m, _) = crate::mesh_kernel::boolean(kernel::BoolOp::Subtract, &a, &b, 0.05).unwrap();
            let cv = m.welded(1e-4).volume();
            assert!(cv < pv && cv > 0.9 * pv, "path {k}: mesh subtract gave {cv} of {pv}");
        }
    }

    /// Where the time goes in a helix sweep, stage by stage, on stderr: the
    /// frame field, the station wires, the skin, the measurement, the
    /// solid. Not an assertion; a measurement (`--nocapture` shows it).
    #[test]
    fn where_a_helix_sweep_spends_its_time() {
        use std::time::Instant;
        let path = Curve::helix(Vec3::ZERO, Vec3::Z, 10.0, 6.0, 2.0, true, 0.0, 0.0).unwrap();
        let (start_plane, _) = end_planes(&path, None).unwrap();
        let sk = Sketch::new(start_plane).with_loop(Loop::circle(P2::new(0.0, 0.0), 1.5));
        let t = Instant::now();
        let field = FrameField::new(&path, Frame::Rmf, None, sk.plane.x_dir).unwrap();
        eprintln!("frame field: {:?} ({} table entries)", t.elapsed(), field.segments[0].table.len());
        let start = field.start();
        let placement_at = |i: usize, tt: f64| -> Placement {
            let f = field.at(&path, i, tt);
            let frac = field.fraction(&path, i, tt);
            Placement::between(&start, &f, 0.0, 1.0 + 0.0 * frac)
        };
        let seg = &path.segments[0];
        let t = Instant::now();
        let k = initial_stations(seg, &spec(&sk, &path, Frame::Rmf), &field, 0, 1.5, 0.01);
        eprintln!("stations: {k} in {:?}", t.elapsed());
        let region = &sk.regions().unwrap()[0];
        let start_wires = vec![kernel::loop_to_wire(&region.outer, &sk.plane).unwrap()];
        let t = Instant::now();
        let mut wires: Vec<Vec<Wire>> = vec![start_wires.clone()];
        for s in 1..=k {
            let p = placement_at(0, s as f64 / k as f64);
            wires.push(start_wires.iter().map(|w| builder::transformed(w, p.matrix())).collect());
        }
        eprintln!("station wires: {:?}", t.elapsed());
        let t = Instant::now();
        let faces = kernel::skin_through_wires(&wires, crate::skin::MAX_V_DEGREE).unwrap();
        eprintln!("skin: {:?} ({} faces)", t.elapsed(), faces.len());
        let t = Instant::now();
        let dev = measure_deviation(&faces, &start_wires, 0, k, 0, k, &placement_at);
        eprintln!("measure: {:?} (deviation {dev:.6})", t.elapsed());
        let t = Instant::now();
        let swept = sweep(&spec(&sk, &path, Frame::Rmf)).unwrap();
        eprintln!("whole sweep: {:?}", t.elapsed());
        let t = Instant::now();
        let vol = kernel::soup_volume(&swept.solids[0], 0.25).unwrap();
        eprintln!("soup volume at 0.25: {:?} (signed {vol:.3})", t.elapsed());
        let t = Instant::now();
        let (mesh, _) = crate::tessellate::tessellate(&swept.solids[0], 0.05).unwrap();
        eprintln!("tessellate at 0.05: {:?} ({} triangles)", t.elapsed(), mesh.triangle_count());
    }

    /// Which booleans a chamfer cutter survives, on stderr: the exact path
    /// at three tolerances and the mesh path, for a straight, an arc and a
    /// spline path. A census, not an assertion; the assertion is the test
    /// after it.
    #[test]
    fn which_booleans_a_cutter_survives() {
        let plate = kernel::extrude(&Sketch::new(Plane::XY).with_loop(Loop::rect(P2::new(20.0, 10.0), 40.0, 20.0)), 6.0)
            .unwrap()
            .remove(0);
        let paths = [
            ("line", Curve::new(v(-1.0, 4.0, 6.0)).line_to(v(41.0, 4.0, 6.0)).unwrap()),
            ("arc", Curve::new(v(-1.0, 4.0, 6.0)).arc_to(v(41.0, 4.0, 6.0), v(20.0, 12.0, 6.0)).unwrap()),
            (
                "spline",
                Curve::new(v(-1.0, 4.0, 6.0)).spline_through(&[v(13.0, 9.0, 6.0), v(27.0, 1.0, 6.0), v(41.0, 6.0, 6.0)], 0.5).unwrap(),
            ),
        ];
        for (what, path) in &paths {
            let (start_plane, _) = end_planes(path, Some(Vec3::Z)).unwrap();
            let tri = Loop::polygon(&[P2::new(-2.0, 0.2), P2::new(0.2, 0.2), P2::new(0.2, -2.0)]);
            let sk = Sketch::new(start_plane).with_loop(tri);
            let swept = sweep(&spec(&sk, path, Frame::Rmf)).unwrap();
            let cutter = &swept.solids[0];
            let (mesh, _) = crate::tessellate::tessellate(cutter, 0.05).unwrap();
            let signed = mesh.welded(1e-4).volume();
            eprintln!("{what}: cutter signed volume {signed:.4}, {} faces, {}", cutter.face_iter().count(), swept.report.describe());
            for tol in [0.1, 0.05, 0.01] {
                match kernel::boolean(kernel::BoolOp::Subtract, &plate, cutter, tol) {
                    Ok(s) => {
                        let kinds: Vec<&str> = s.face_iter().map(|f| kernel::tessellation_kind(&f.surface())).collect();
                        let meshed: Vec<String> = [0.005, 0.05, 0.2, 0.5]
                            .iter()
                            .map(|&mt| match crate::tessellate::tessellate(&s, mt) {
                                Ok((m, _)) => format!("{mt}: {} tri", m.triangle_count()),
                                Err(_) => format!("{mt}: open {:?}", kernel::open_edge_faces(&s, mt)),
                            })
                            .collect();
                        eprintln!("  exact tol {tol}: ok, {} faces {kinds:?}; meshed {}", s.face_iter().count(), meshed.join("; "))
                    }
                    Err(e) => eprintln!("  exact tol {tol}: {}", e.to_string().chars().take(90).collect::<String>()),
                }
            }
            let a = crate::tessellate::tessellate(&plate, 0.05).unwrap();
            let b = crate::tessellate::tessellate(cutter, 0.05).unwrap();
            match crate::mesh_kernel::boolean(kernel::BoolOp::Subtract, &a, &b, 0.05) {
                Ok((m, _)) => eprintln!("  mesh: ok, {} triangles, volume {:.3}", m.triangle_count(), m.welded(1e-4).volume()),
                Err(e) => eprintln!("  mesh: {}", e.to_string().chars().take(90).collect::<String>()),
            }
        }
        // Which profile kinds tessellate at 0.01 along the arc: what each
        // face is meshed as, and whether the mesh closes.
        let arc_path = &paths[1].1;
        let shapes: [(&str, Loop); 3] = [
            ("triangle", Loop::polygon(&[P2::new(-2.0, 0.2), P2::new(0.2, 0.2), P2::new(0.2, -2.0)])),
            ("rectangle", Loop::rect(P2::new(0.0, 0.0), 2.0, 1.5)),
            ("circle", Loop::circle(P2::new(0.0, 0.0), 1.0)),
        ];
        for (which, path) in [("arc", arc_path), ("spline", &paths[2].1)] {
            for (what, lp) in &shapes {
                let (start_plane, _) = end_planes(path, Some(Vec3::Z)).unwrap();
                let sk = Sketch::new(start_plane).with_loop(lp.clone());
                let swept = sweep(&spec(&sk, path, Frame::Rmf)).unwrap();
                let kinds: Vec<&str> = swept.solids[0].face_iter().map(|f| kernel::tessellation_kind(&f.surface())).collect();
                let fine = crate::tessellate::tessellate(&swept.solids[0], 0.01).map(|(m, _)| m.triangle_count());
                eprintln!("{what} along the {which}: faces {kinds:?}; at 0.01: {}", match fine {
                    Ok(n) => format!("{n} triangles"),
                    Err(e) => {
                        let open = kernel::open_edge_faces(&swept.solids[0], 0.01);
                        format!("{} — open edges on faces {open:?}", e.to_string().chars().take(40).collect::<String>())
                    }
                });
            }
        }
        // Variants of the arc case, to see what truck objects to: the top
        // leg raised out of the way, the vertical leg tilted into a cone,
        // and a tighter arc.
        let variants: [(&str, [P2; 3]); 3] = [
            ("arc, top leg at +1.0", [P2::new(-2.0, 0.2), P2::new(1.0, 0.2), P2::new(1.0, -2.0)]),
            ("arc, vertical leg tilted", [P2::new(-2.0, 0.6), P2::new(0.2, 0.2), P2::new(0.2, -2.0)]),
            ("arc, symmetric V", [P2::new(-2.0, 0.0), P2::new(0.3, 1.5), P2::new(0.3, -1.5)]),
        ];
        for (what, tri) in &variants {
            let (start_plane, _) = end_planes(arc_path, Some(Vec3::Z)).unwrap();
            let sk = Sketch::new(start_plane).with_loop(Loop::polygon(tri));
            let swept = sweep(&spec(&sk, arc_path, Frame::Rmf)).unwrap();
            for tol in [0.1, 0.05] {
                match kernel::boolean(kernel::BoolOp::Subtract, &plate, &swept.solids[0], tol) {
                    Ok(s) => eprintln!("{what}: exact tol {tol}: ok, {} faces", s.face_iter().count()),
                    Err(e) => eprintln!("{what}: exact tol {tol}: {}", e.to_string().chars().take(70).collect::<String>()),
                }
            }
        }
        let tight = Curve::new(v(10.0, 4.0, 6.0)).arc_to(v(30.0, 4.0, 6.0), v(20.0, 14.0, 6.0)).unwrap();
        let (start_plane, _) = end_planes(&tight, Some(Vec3::Z)).unwrap();
        let sk = Sketch::new(start_plane).with_loop(Loop::polygon(&[P2::new(-2.0, 0.2), P2::new(0.2, 0.2), P2::new(0.2, -2.0)]));
        let swept = sweep(&spec(&sk, &tight, Frame::Rmf)).unwrap();
        match kernel::boolean(kernel::BoolOp::Subtract, &plate, &swept.solids[0], 0.1) {
            Ok(s) => eprintln!("semicircle inside the plate: exact tol 0.1: ok, {} faces", s.face_iter().count()),
            Err(e) => eprintln!("semicircle inside the plate: exact tol 0.1: {}", e.to_string().chars().take(70).collect::<String>()),
        }
        // The same triangular ring made by Cadpole's own `revolve` about the
        // arc's axis: is the arc failure the construction or the kernel?
        let arc = crate::curve::arc3_params(v(-1.0, 4.0, 6.0), v(41.0, 4.0, 6.0), v(20.0, 12.0, 6.0)).unwrap();
        let radial = Plane::new(v(-1.0, 4.0, 6.0), arc.tangent(0.0), Vec3::Z).unwrap();
        let tri = Loop::polygon(&[P2::new(-2.0, 0.2), P2::new(0.2, 0.2), P2::new(0.2, -2.0)]);
        let ring = kernel::revolve(&Sketch::new(radial).with_loop(tri), arc.centre, arc.normal(), arc.sweep).unwrap().remove(0);
        let (mesh, _) = crate::tessellate::tessellate(&ring, 0.05).unwrap();
        eprintln!("revolve-built ring: signed volume {:.4}, {} faces", mesh.welded(1e-4).volume(), ring.face_iter().count());
        for tol in [0.1, 0.05] {
            match kernel::boolean(kernel::BoolOp::Subtract, &plate, &ring, tol) {
                Ok(s) => eprintln!("  exact tol {tol}: ok, {} faces", s.face_iter().count()),
                Err(e) => eprintln!("  exact tol {tol}: {}", e.to_string().chars().take(90).collect::<String>()),
            }
        }
    }

    /// Twist and scale along a straight path: a twisted prism keeps its
    /// base area at every section, so its volume is the prism's; a scaled
    /// one is the frustum of the textbook volume and is still exact.
    #[test]
    fn twist_keeps_the_volume_and_scale_is_the_frustum() {
        let sk = Sketch::new(Plane::XY).with_loop(Loop::rect(P2::new(0.0, 0.0), 6.0, 4.0));
        let path = Curve::new(Vec3::ZERO).line_to(v(0.0, 0.0, 20.0)).unwrap();
        let mut s = spec(&sk, &path, Frame::Rmf);
        s.twist = std::f64::consts::FRAC_PI_2;
        let twisted = sweep(&s).unwrap();
        assert_eq!(twisted.report.segments[0].method, Method::Skinned);
        let (vol, solid, _, folds) = measure(&twisted.solids[0]);
        assert!((vol - 480.0).abs() < 0.01 * 480.0, "{vol}");
        assert!(solid && folds == 0);
        let dev = twisted.report.max_deviation().unwrap();
        assert!(dev <= twisted.report.target, "{dev}");
        let mut s = spec(&sk, &path, Frame::Rmf);
        s.scale = 0.5;
        let scaled = sweep(&s).unwrap();
        assert_eq!(scaled.report.segments[0].method, Method::Ruled);
        let (vol, solid, faces, _) = measure(&scaled.solids[0]);
        let (a, b): (f64, f64) = (24.0, 6.0);
        let want = 20.0 * (a + (a * b).sqrt() + b) / 3.0;
        assert!((vol - want).abs() < 1e-6, "{vol} vs {want}");
        assert!(solid);
        assert_eq!(faces, 6);
    }

    /// A pipe: an annulus along line–arc–line. Every segment is exact, the
    /// faces are shared across the joints (one solid, closed), and the
    /// volume is the annulus times the centreline length.
    #[test]
    fn a_bent_tube_is_exact_and_one_solid() {
        let (ro, ri) = (5.0, 4.0);
        let plane = Plane::new(Vec3::ZERO, Vec3::Z, Vec3::X).unwrap();
        let sk = Sketch::new(plane).with_loop(Loop::circle(P2::new(0.0, 0.0), ro)).with_loop(Loop::circle(P2::new(0.0, 0.0), ri));
        let r = 15.0;
        let (s, c) = fmath::sin_cos(std::f64::consts::FRAC_PI_4);
        let path = Curve::new(Vec3::ZERO)
            .line_to(v(0.0, 0.0, 20.0))
            .unwrap()
            .arc_to(v(r, 0.0, 20.0 + r), v(r - r * c, 0.0, 20.0 + r * s))
            .unwrap()
            .line_to(v(r + 25.0, 0.0, 20.0 + r))
            .unwrap();
        let swept = sweep(&spec(&sk, &path, Frame::Rmf)).unwrap();
        assert!(swept.report.is_exact(), "{}", swept.report.describe());
        assert_eq!(swept.solids.len(), 1, "an annulus is one region");
        let (vol, solid, faces, folds) = measure(&swept.solids[0]);
        let want = std::f64::consts::PI * (ro * ro - ri * ri) * path.length();
        assert!((vol - want).abs() < 0.003 * want, "{vol} vs {want}");
        assert!(solid && folds == 0);
        // 2 loops × (2 + 2 + 2) faces + 2 caps: the quarter-turn arc is
        // one patch per profile edge.
        assert_eq!(faces, 2 * 6 + 2);
        assert!(swept.report.note().is_none(), "nothing approximated, nothing to say");
    }

    /// The reference example's pipe: a line–arc–line route whose arc is
    /// given by a via point typed to two decimals, `(5.86, 0, 54.14)` for
    /// the true `(5.857864…, 0, 54.142135…)`. The arc then meets the lines
    /// 0.02° off tangent, a numerical joint and not a corner: the sweep
    /// builds, stays exact, closes into one solid, and its end cap lies on
    /// the wires the segments carried there. A real corner, however small
    /// to the eye, is still refused, and a near miss names its angle in
    /// digits that are not all zero.
    #[test]
    fn a_hand_typed_tangent_arc_is_a_joint_not_a_corner() {
        let plane = Plane::new(Vec3::ZERO, Vec3::Z, Vec3::X).unwrap();
        let sk = Sketch::new(plane).with_loop(Loop::circle(P2::new(0.0, 0.0), 5.0)).with_loop(Loop::circle(P2::new(0.0, 0.0), 4.0));
        let route = |via: Vec3| {
            Curve::new(Vec3::ZERO)
                .line_to(v(0.0, 0.0, 40.0))
                .unwrap()
                .arc_to(v(20.0, 0.0, 60.0), via)
                .unwrap()
                .line_to(v(60.0, 0.0, 60.0))
                .unwrap()
        };
        for frame in [Frame::Rmf, Frame::Frenet, Frame::Fixed] {
            let path = route(v(5.86, 0.0, 54.14));
            let mut s = spec(&sk, &path, frame);
            if frame == Frame::Fixed {
                s.up = Some(Vec3::Y);
            }
            let swept = sweep(&s).unwrap_or_else(|e| panic!("{}: {e}", frame.name()));
            assert_eq!(swept.solids.len(), 1);
            let (vol, solid, _, folds) = measure(&swept.solids[0]);
            let want = std::f64::consts::PI * (25.0 - 16.0) * path.length();
            assert!((vol - want).abs() < 0.003 * want, "{}: {vol} vs {want}", frame.name());
            assert!(solid && folds == 0, "{}", frame.name());
            if frame == Frame::Rmf {
                assert!(swept.report.is_exact(), "{}", swept.report.describe());
            }
        }
        // A joint a tenth of a degree off is a corner.
        let (s, c) = fmath::sin_cos(0.1f64.to_radians());
        let kink = Curve::new(Vec3::ZERO).line_to(v(0.0, 0.0, 10.0)).unwrap().line_to(v(10.0 * s, 0.0, 10.0 + 10.0 * c)).unwrap();
        let e = sweep(&spec(&sk, &kink, Frame::Rmf)).unwrap_err().to_string();
        assert!(e.contains("corner of 0.1") && e.contains("round the corner"), "{e}");
        let (s, c) = fmath::sin_cos(0.004f64.to_radians() * 20.0);
        let kink = Curve::new(Vec3::ZERO).line_to(v(0.0, 0.0, 10.0)).unwrap().line_to(v(10.0 * s, 0.0, 10.0 + 10.0 * c)).unwrap();
        let e = sweep(&spec(&sk, &kink, Frame::Rmf)).unwrap_err().to_string();
        assert!(e.contains("corner of 0.08°"), "{e}");
        // Near-tangent joints that add up: a via point 0.03° off at each end
        // of the arc, and a polyline of three 0.04° kinks the same way. Each
        // builds one closed solid or is refused as a corner, never as a face
        // the tessellator cannot mesh.
        let (s4, c4) = fmath::sin_cos(0.04f64.to_radians());
        let mut poly = Curve::new(Vec3::ZERO);
        let (mut at, mut dir) = (Vec3::ZERO, Vec3::Z);
        for _ in 0..4 {
            at = at + dir * 10.0;
            poly = poly.line_to(at).unwrap();
            dir = Vec3::new(dir.x * c4 + dir.z * s4, 0.0, dir.z * c4 - dir.x * s4);
        }
        for (name, path) in [("via 0.03° off", route(v(5.861, 0.0, 54.139))), ("three 0.04° kinks", poly)] {
            for frame in [Frame::Rmf, Frame::Frenet, Frame::Fixed] {
                let mut s = spec(&sk, &path, frame);
                if frame == Frame::Fixed {
                    s.up = Some(Vec3::Y);
                }
                match sweep(&s) {
                    Ok(swept) => {
                        assert_eq!(swept.solids.len(), 1, "{name} {}", frame.name());
                        let (_, solid, _, folds) = measure(&swept.solids[0]);
                        assert!(solid && folds == 0, "{name} {}", frame.name());
                    }
                    Err(e) => {
                        let e = e.to_string();
                        assert!(e.contains("sweep_along:") && !e.contains("tessellation") && !e.contains("triangulated"), "{name} {}: {e}", frame.name());
                    }
                }
            }
        }
    }

    /// The refusals: a corner, a closed path, a Frenet inflection, a fixed
    /// up along the path.
    #[test]
    fn the_refusals_name_the_place_and_the_fix() {
        let sk = Sketch::new(Plane::XY).with_loop(Loop::circle(P2::new(0.0, 0.0), 1.0));
        let corner = Curve::new(Vec3::ZERO).line_to(v(0.0, 0.0, 10.0)).unwrap().line_to(v(10.0, 0.0, 10.0)).unwrap();
        let e = sweep(&spec(&sk, &corner, Frame::Rmf)).unwrap_err().to_string();
        assert!(e.contains("corner of 90.00°") && e.contains("round the corner"), "{e}");
        let closed = Curve::new(Vec3::ZERO).line_to(v(0.0, 0.0, 10.0)).unwrap().line_to(v(5.0, 0.0, 5.0)).unwrap().close().unwrap();
        let e = sweep(&spec(&sk, &closed, Frame::Rmf)).unwrap_err().to_string();
        assert!(e.contains("closed"), "{e}");
        // An S-curve has an inflection: Frenet flips there.
        let s_curve = Curve::new(Vec3::ZERO).bezier_to(v(0.0, 0.0, 30.0), vec![v(10.0, 0.0, 10.0), v(-10.0, 0.0, 20.0)]).unwrap();
        let e = sweep(&spec(&sk, &s_curve, Frame::Frenet)).unwrap_err().to_string();
        assert!(e.contains("inflection") && e.contains("frame.rmf"), "{e}");
        assert!(sweep(&spec(&sk, &s_curve, Frame::Rmf)).is_ok());
        // Frenet on a straight path carries the seed and works.
        let line = Curve::new(Vec3::ZERO).line_to(v(0.0, 0.0, 10.0)).unwrap();
        assert!(sweep(&spec(&sk, &line, Frame::Frenet)).is_ok());
        // Fixed with up along the path.
        let mut s = spec(&sk, &line, Frame::Fixed);
        s.up = Some(Vec3::Z);
        let e = sweep(&s).unwrap_err().to_string();
        assert!(e.contains("frame.fixed") && e.contains("off the path"), "{e}");
    }

    /// The double-reflection step converges: on a helix the RMF's twist
    /// relative to the Frenet frame is the torsion times the length, in
    /// closed form, and the propagated frame reproduces it.
    #[test]
    fn the_rmf_on_a_helix_twists_against_frenet_by_the_torsion() {
        let (r, pitch, turns) = (10.0, 5.0, 3.0);
        let path = Curve::helix(Vec3::ZERO, Vec3::Z, r, pitch, turns, true, 0.0, 0.0).unwrap();
        let seg = &path.segments[0];
        let frenet0 = frenet_normal_at(seg, 0.0).unwrap();
        let rmf = FrameField::new(&path, Frame::Rmf, Some(frenet0), Vec3::X).unwrap();
        let frenet = FrameField::new(&path, Frame::Frenet, None, Vec3::X).unwrap();
        let (fr, fe) = (rmf.end(), frenet.end());
        let c = pitch / TAU;
        let torsion = c / (r * r + c * c);
        let want = torsion * path.length();
        let got = fmath::atan2(fr.n.dot(fe.b), fr.n.dot(fe.n)).abs();
        let want_wrapped = (want % TAU).min(TAU - want % TAU);
        assert!((got - want_wrapped).abs() < 1e-7, "twist {got} rad vs τL = {want} ({want_wrapped})");
    }

    /// `Segment3::Helix` in closed form: length, sub-pieces and the
    /// reversal agree with the parametrisation.
    #[test]
    fn a_helix_segment_is_consistent() {
        let path = Curve::helix(v(1.0, 2.0, 3.0), v(0.0, 0.0, 1.0), 4.0, 2.0, 1.5, true, 1.0, 0.0).unwrap();
        let seg = &path.segments[0];
        assert_eq!(seg.kind(), "helix");
        assert!((seg.start() - v(5.0, 2.0, 3.0)).length() < 1e-12);
        let end = seg.end();
        assert!((end.z - 6.0).abs() < 1e-12);
        assert!(((end - v(1.0, 2.0, 6.0)).length() - 5.0).abs() < 1e-9, "the taper reaches 5 at the end");
        let len = seg.length();
        let piece = seg.sub(len * 0.25, len * 0.75).unwrap();
        assert!((piece.length() - len * 0.5).abs() < 1e-6);
        assert!((piece.start() - seg.point_at_length(len * 0.25)).length() < 1e-9);
        assert!((piece.end() - seg.point_at_length(len * 0.75)).length() < 1e-9);
        let back = seg.reversed();
        assert!((back.start() - end).length() < 1e-9);
        assert!((back.end() - seg.start()).length() < 1e-9);
        assert!((back.length() - len).abs() < 1e-9);
        assert!(path.plane().is_none());
        let flat = Curve::helix(Vec3::ZERO, Vec3::Z, 4.0, 0.0, 0.5, false, 0.0, 0.0).unwrap();
        assert!(flat.plane().is_some(), "no advance: a flat coil is planar");
        let cyl = Curve::helix(Vec3::ZERO, Vec3::Z, 4.0, 2.0, 3.0, true, 0.0, 0.0).unwrap();
        let want = 3.0 * fmath::hypot(TAU * 4.0, 2.0);
        assert!((cyl.length() - want).abs() < 1e-9);
    }
}
