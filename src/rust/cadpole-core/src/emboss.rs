//! Imprint, emboss and wrap: a sketch on a face of a body.
//!
//! **Imprint** is the primitive. It splits one face of a B-rep body along
//! the loops of a sketch laid on that face, keeping the face's own surface:
//! the parent face gets the loops as new inner boundaries, the pieces inside
//! them become faces of their own on the same surface, and the body is
//! rebuilt shell by shell around the new faces. No boolean runs and no
//! surface–surface intersection is computed — the loops *are* the new
//! edges, so the split is exact wherever the loops are.
//! That is the other way round from the chamfer's cutter: a
//! hole's mouth cannot be chamfered because the exact kernel will not
//! split a face along a curve that encloses one of its inner loops; here
//! the face is never handed to the kernel to split, it is *built* with the
//! loops in it.
//!
//! **Emboss** is the imprint with a relief. `raise` extrudes the imprinted
//! pieces along the face's normal by a height, `cut` the same inward, and
//! both are again a rebuild: the piece's loop is shared, edge for edge,
//! between the parent face (as a hole), the walls (as their base) and —
//! through the walls' rungs — the top, so the raised region shares the
//! face's boundary by construction and the flush-contact case never arises.
//! `paint` is the imprint with the pieces named for a `paint` feature;
//! `scribe` strokes the sketch's outlines into a thin closed band and cuts
//! that. On a plane every edge is the sketch's own segment (a line, an arc
//! or a Bézier) and the walls are planes and cylinders, as an extrude's
//! are. On a cylinder or a cone the walls are ruled between the loop on the
//! surface and the same loop on the offset surface — a helicoid for a
//! developed line, a plane for a projected one — and the top is the offset
//! cylinder (radius `r ± h`) or the offset cone (the same half-angle, the
//! apex moved along the axis), trimmed by the offset loops: exact surfaces,
//! the kernel's own kinds. A sphere, a torus or a free-form face has no
//! offset surface the kernel can represent, so a relief there is refused by
//! name; the imprint (and so `paint`) still works on any face.
//!
//! **Wrap** puts a planar sketch on a face. `project` moves every point
//! along the face's normal at the wrap's origin until it meets the face:
//! exact on a plane (an identity for a sketch drawn on it), in closed form
//! on a cylinder, a cone or a sphere (a ray against a quadric), and by
//! Newton on anything else (a loft's skin). `unroll` lays the sketch on the
//! *developed* surface of a cylinder or a cone — an isometry, so distances
//! along the surface are the sketch's own, the way a label wraps a can —
//! and is closed form on both. The curves this produces are exact edges
//! where the kernel has one (a parallel circle, a generator, a plane
//! section that is a line) and otherwise a cubic Hermite spline through
//! samples of the true curve with the true tangents, refined until it is
//! within [`FIT_TOL`] of it, measured; the largest deviation is reported on
//! the feature.
//!
//! Everything a script sees is named by construction
//! ([`naming::Generator::Indexed`]): the parent keeps its name, a piece
//! inside loop `L` is `<feature>/imprint[L]`, an island inside a hole of the
//! sketch `<feature>/island[L]`, a raised or cut top `<feature>/end[L]` and
//! a wall `<feature>/side[l.s]`, with `imprint`, `end` and `side` as group
//! aliases. `L` is the loop's own name when it has one (`text_1/glyph[3]/
//! loop[0]`) and its index otherwise.

use std::collections::{BTreeMap, HashMap};
use std::sync::Arc;

use truck_modeling::{
    builder, BSplineCurve, BoundedCurve, Curve, Edge, Face, Invertible, KnotVec, ParametricCurve, ParametricSurface,
    ParametricSurface3D, Point3, SPHint2D, SearchParameter, Shell, Solid, Surface, Vertex, Wire,
};

use crate::context::{fmt_num, Body, Context, Span};
use crate::curve::{Curve as Curve3, Segment3};
use crate::fmath;
use crate::geom::{Plane, Vec3};
use crate::ids::{BodyId, FeatureId};
use crate::kernel::{self, KSolid};
use crate::memo::{KeyBuilder, Produced};
use crate::naming::{self, full_name, NamedFace, Role};
use crate::sketch::{arc_params, bezier_point, Loop, Path, Segment, Sketch, P2};
use crate::sketch2d;
use crate::surface_class::SurfaceClass;
use crate::context::NameSource;
use crate::{CadpoleError, Result};

/// How far a fitted on-surface edge may leave the true curve, in mm. truck
/// inverts a boundary point onto its face's surface only within `1e-6`, so
/// the edge has to sit an order of magnitude closer than that; a tenth of a
/// micron is also four orders below the finest tessellation anyone asks for.
pub const FIT_TOL: f64 = 1e-7;

/// The largest number of Hermite spans one sketch segment may be fitted
/// with before the fit is called off. A segment that needs more is longer
/// than a full turn of the surface it lies on.
const MAX_SPANS: usize = 1024;

/// Sample points per boundary edge and per loop segment when the loops are
/// checked against the face's boundary in parameter space.
const CHECK_SAMPLES: usize = 24;

// ------------------------------------------------------------ vocabularies

/// What `emboss(kind=)` does to the material. Spelled `raised`, not
/// `raise`: `raise` is a reserved word in Starlark (Python's), so
/// `emboss.raise` cannot be written — and
/// the member is the string, so the string is `"raised"` too; `"raise"`
/// is accepted by the parser as an alias, the way `planar` is for
/// `kind.plane`.
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub enum Kind {
    /// The imprinted pieces stand proud of the face by the height.
    Raise,
    /// The imprinted pieces are sunk into the face by the height.
    Cut,
    /// The imprinted pieces are coloured; no geometry changes.
    Paint,
    /// The sketch's outlines are stroked into a thin band and cut.
    Scribe,
}

impl Kind {
    pub const ALL: [Kind; 4] = [Kind::Raise, Kind::Cut, Kind::Paint, Kind::Scribe];

    pub fn name(self) -> &'static str {
        match self {
            Kind::Raise => "raised",
            Kind::Cut => "cut",
            Kind::Paint => "paint",
            Kind::Scribe => "scribe",
        }
    }

    pub fn parse(s: &str) -> std::result::Result<Kind, String> {
        if s == "raise" {
            return Ok(Kind::Raise);
        }
        Kind::ALL.iter().copied().find(|k| k.name() == s).ok_or_else(|| {
            format!(
                "kind must be one of {}, got {s:?}",
                Kind::ALL.iter().map(|k| format!("\"{}\"", k.name())).collect::<Vec<_>>().join(", ")
            )
        })
    }
}

/// How `wrap(method=)` carries a flat sketch onto a face.
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub enum Method {
    /// Along the face's normal at the wrap's origin: any face.
    Project,
    /// On the developed surface: a cylinder or a cone (a plane trivially).
    Unroll,
}

impl Method {
    pub const ALL: [Method; 2] = [Method::Project, Method::Unroll];

    pub fn name(self) -> &'static str {
        match self {
            Method::Project => "project",
            Method::Unroll => "unroll",
        }
    }

    pub fn parse(s: &str) -> std::result::Result<Method, String> {
        Method::ALL.iter().copied().find(|m| m.name() == s).ok_or_else(|| {
            format!(
                "method must be one of {}, got {s:?}",
                Method::ALL.iter().map(|m| format!("\"{}\"", m.name())).collect::<Vec<_>>().join(", ")
            )
        })
    }
}

#[cfg(test)]
mod vocab_tests {
    use super::*;

    #[test]
    fn every_kind_and_method_is_listed_and_parses() {
        for k in Kind::ALL {
            // Exhaustive, so a new variant fails to compile until it is in `ALL`.
            match k {
                Kind::Raise | Kind::Cut | Kind::Paint | Kind::Scribe => {}
            }
            assert_eq!(Kind::parse(k.name()).unwrap(), k);
        }
        for m in Method::ALL {
            match m {
                Method::Project | Method::Unroll => {}
            }
            assert_eq!(Method::parse(m.name()).unwrap(), m);
        }
        assert!(Kind::parse("engrave").unwrap_err().contains("\"scribe\""));
        // `raise` is Starlark's reserved word; the member is `raised`, and
        // the old spelling still parses.
        assert_eq!(Kind::Raise.name(), "raised");
        assert_eq!(Kind::parse("raise").unwrap(), Kind::Raise);
        assert!(Method::parse("conformal").unwrap_err().contains("\"unroll\""));
    }
}

// ------------------------------------------------------------ the mapping

/// How a point of the frame's plane reaches the surface, and how far along
/// the face's outward normal an offset moves it.
#[derive(Clone, Debug)]
enum Map {
    /// The face is a plane and the frame lies in it.
    Plane,
    /// The developed cylinder: `(x, y)` in the frame is `(circumferential,
    /// axial)` through `xs`/`ys`, the angle is the arc length over the
    /// radius. `sign` is `+1` when the face's outward normal points away
    /// from the axis, `−1` in a bore.
    UnrollCylinder { centre: Vec3, axis: Vec3, radial0: Vec3, circ0: Vec3, r: f64, sign: f64, xs: (f64, f64), ys: (f64, f64) },
    /// The developed cone: a sector about the apex, the origin at slant
    /// distance `s0` on the generator through `radial0`; an angle of `ψ` in
    /// the sector is `ψ / sin α` about the axis.
    UnrollCone { apex: Vec3, axis: Vec3, radial0: Vec3, circ0: Vec3, half_angle: f64, s0: f64, sign: f64, xs: (f64, f64), ys: (f64, f64) },
    /// A ray from the frame's plane along its normal against a quadric.
    ProjectQuadric { class: SurfaceClass, sign: f64 },
    /// A ray against any surface, by Newton from the nearest point.
    ProjectSurface { surface: Arc<Surface> },
}

/// One segment of a loop as it lies on the surface: an exact edge where
/// the kernel has one, else a cubic Hermite spline within [`FIT_TOL`].
#[derive(Clone, Debug)]
enum Piece {
    Line(Vec3, Vec3),
    Arc(Vec3, Vec3, Vec3),
    Bezier(Vec3, Vec3, Vec<Vec3>),
    /// `3n + 1` Bézier control points of `n` cubic spans, C¹ at the joins;
    /// `analytic` is the closed form when there is one (a helix).
    Spline { ctrl: Vec<Vec3>, analytic: Option<Segment3> },
}

impl Piece {
    fn start(&self) -> Vec3 {
        match self {
            Piece::Line(a, _) | Piece::Arc(a, _, _) | Piece::Bezier(a, _, _) => *a,
            Piece::Spline { ctrl, .. } => ctrl[0],
        }
    }

    fn end(&self) -> Vec3 {
        match self {
            Piece::Line(_, b) | Piece::Arc(_, b, _) | Piece::Bezier(_, b, _) => *b,
            Piece::Spline { ctrl, .. } => ctrl[ctrl.len() - 1],
        }
    }

    /// The truck edge between two existing vertices at this piece's ends.
    fn edge(&self, from: &Vertex, to: &Vertex) -> Edge {
        match self {
            Piece::Line(..) => builder::line(from, to),
            Piece::Arc(_, _, via) => builder::circle_arc(from, to, via.point()),
            Piece::Bezier(_, _, ctrl) => builder::bezier(from, to, ctrl.iter().map(|c| c.point()).collect()),
            Piece::Spline { ctrl, .. } => {
                let spans = (ctrl.len() - 1) / 3;
                let mut knots: Vec<f64> = vec![0.0; 4];
                for k in 1..spans {
                    let t = k as f64 / spans as f64;
                    knots.extend([t, t, t]);
                }
                knots.extend([1.0; 4]);
                let points: Vec<Point3> = ctrl.iter().map(|c| c.point()).collect();
                Edge::new(from, to, Curve::BSplineCurve(BSplineCurve::new(KnotVec::from(knots), points)))
            }
        }
    }

    /// Points along the piece for the checks in parameter space.
    fn samples(&self, n: usize) -> Vec<Vec3> {
        match self {
            Piece::Line(a, b) => (0..=n).map(|i| lerp3(*a, *b, i as f64 / n as f64)).collect(),
            Piece::Arc(a, b, via) => match crate::curve::arc3_params(*a, *b, *via) {
                Some(arc) => (0..=n).map(|i| arc.point(i as f64 / n as f64)).collect(),
                None => vec![*a, *b],
            },
            Piece::Bezier(a, b, ctrl) => {
                let mut pts = vec![*a];
                pts.extend_from_slice(ctrl);
                pts.push(*b);
                (0..=n).map(|i| bezier_point(&pts, i as f64 / n as f64)).collect()
            }
            Piece::Spline { ctrl, .. } => {
                let spans = (ctrl.len() - 1) / 3;
                let per = (n / spans).max(2);
                let mut out = Vec::with_capacity(spans * per + 1);
                for k in 0..spans {
                    let c = &ctrl[3 * k..3 * k + 4];
                    for i in 0..per {
                        out.push(bezier_point(c, i as f64 / per as f64));
                    }
                }
                out.push(ctrl[ctrl.len() - 1]);
                out
            }
        }
    }

    /// The piece as segments of a [`Curve3`]: the closed form where there is
    /// one, else one cubic Bézier per span.
    fn segments(&self) -> Vec<Segment3> {
        match self {
            Piece::Line(a, b) => vec![Segment3::Line { a: *a, b: *b }],
            Piece::Arc(a, b, via) => vec![Segment3::Arc { a: *a, b: *b, via: *via }],
            Piece::Bezier(a, b, ctrl) => vec![Segment3::Bezier { a: *a, b: *b, ctrl: ctrl.clone() }],
            Piece::Spline { ctrl, analytic } => match analytic {
                Some(s) => vec![s.clone()],
                None => ctrl
                    .chunks_exact(3)
                    .zip(ctrl.iter().skip(3).step_by(3))
                    .map(|(c, b)| Segment3::Bezier { a: c[0], b: *b, ctrl: vec![c[1], c[2]] })
                    .collect(),
            },
        }
    }
}

fn lerp3(a: Vec3, b: Vec3, t: f64) -> Vec3 {
    a + (b - a) * t
}

fn unit(v: Vec3) -> Option<Vec3> {
    let l = v.length();
    (l > 1e-12 && l.is_finite()).then(|| v * (1.0 / l))
}

/// The point of a 2D segment at `t` in `[0, 1]`, by the segment's own
/// parameter: a line's fraction, an arc's angle, a Bézier's parameter.
fn seg_point(seg: &Segment, t: f64) -> P2 {
    match seg {
        Segment::Line { a, b } => P2::new(a.x + (b.x - a.x) * t, a.y + (b.y - a.y) * t),
        Segment::Arc { a, b, via } => match arc_params(*a, *b, *via) {
            Some((c, r, t0, t1)) => {
                let (s, co) = fmath::sin_cos(t0 + (t1 - t0) * t);
                P2::new(c.x + r * co, c.y + r * s)
            }
            None => P2::new(a.x + (b.x - a.x) * t, a.y + (b.y - a.y) * t),
        },
        Segment::Bezier { a, b, ctrl } => {
            let mut pts = vec![*a];
            pts.extend_from_slice(ctrl);
            pts.push(*b);
            bezier_point(&pts, t)
        }
    }
}

impl Map {
    /// The point of the surface (offset by `off` along the face's outward
    /// normal) that the frame point `p` maps to; `None` when the mapping
    /// has no answer there (a ray that misses, a cone's apex).
    fn at(&self, frame: &Plane, p: P2, off: f64) -> Option<Vec3> {
        match self {
            Map::Plane => Some(frame.to_world(p.x, p.y) + frame.normal * off),
            Map::UnrollCylinder { centre, axis, radial0, circ0, r, sign, xs, ys } => {
                let s = p.x * xs.0 + p.y * ys.0;
                let t = p.x * xs.1 + p.y * ys.1;
                let theta = s / r;
                let (sn, cs) = fmath::sin_cos(theta);
                let radial = *radial0 * cs + *circ0 * sn;
                Some(*centre + *axis * t + radial * (r + sign * off))
            }
            Map::UnrollCone { apex, axis, radial0, circ0, half_angle, s0, sign, xs, ys } => {
                let x = p.x * xs.0 + p.y * ys.0;
                let y = s0 + p.x * xs.1 + p.y * ys.1;
                let rho = fmath::hypot(x, y);
                if !(rho > 1e-9) {
                    return None;
                }
                let psi = fmath::atan2(x, y);
                let (sa, ca) = fmath::sin_cos(*half_angle);
                if psi.abs() > std::f64::consts::PI * sa {
                    return None;
                }
                let phi = psi / sa;
                let (sp, cp) = fmath::sin_cos(phi);
                let radial = *radial0 * cp + *circ0 * sp;
                let on = *apex + *axis * (rho * ca) + radial * (rho * sa);
                let normal = radial * ca - *axis * sa;
                Some(on + normal * (sign * off))
            }
            Map::ProjectQuadric { class, sign } => {
                let p0 = frame.to_world(p.x, p.y);
                let d = -frame.normal;
                let t = ray_quadric(p0, d, class, sign * off)?;
                Some(p0 + d * t)
            }
            Map::ProjectSurface { surface } => {
                if off != 0.0 {
                    return None;
                }
                let p0 = frame.to_world(p.x, p.y);
                ray_surface(surface, p0, -frame.normal)
            }
        }
    }

    /// `at` at both ends and the middle of a segment, for the exact cases.
    fn exact(&self, frame: &Plane, seg: &Segment, off: f64) -> Option<Piece> {
        let a2 = seg.start();
        let b2 = seg.end();
        match self {
            Map::Plane => {
                let plane = frame.offset(off);
                Some(match Segment3::from_2d(seg, &plane) {
                    Segment3::Line { a, b } => Piece::Line(a, b),
                    Segment3::Arc { a, b, via } => Piece::Arc(a, b, via),
                    Segment3::Bezier { a, b, ctrl } => Piece::Bezier(a, b, ctrl),
                    Segment3::Helix { .. } => unreachable!("a planar segment is never a helix"),
                })
            }
            Map::UnrollCylinder { centre, axis, r, sign, xs, ys, .. } => {
                let Segment::Line { .. } = seg else { return None };
                let dev = |q: P2| (q.x * xs.0 + q.y * ys.0, q.x * xs.1 + q.y * ys.1);
                let (sa, ta) = dev(a2);
                let (sb, tb) = dev(b2);
                let scale = r.max((ta - tb).abs()).max(1.0);
                let a = self.at(frame, a2, off)?;
                let b = self.at(frame, b2, off)?;
                if (sa - sb).abs() <= 1e-12 * scale {
                    return Some(Piece::Line(a, b));
                }
                let angle = (sb - sa) / r;
                if angle.abs() >= std::f64::consts::TAU - 1e-9 {
                    return None;
                }
                if (ta - tb).abs() <= 1e-12 * scale {
                    let via = self.at(frame, P2::new((a2.x + b2.x) / 2.0, (a2.y + b2.y) / 2.0), off)?;
                    return Some(Piece::Arc(a, b, via));
                }
                let helix = Segment3::Helix {
                    a,
                    centre: *centre + *axis * ta,
                    axis: *axis,
                    r_end: r + sign * off,
                    angle,
                    advance: tb - ta,
                };
                let f = |t: f64| self.at(frame, seg_point(seg, t), off);
                let (ctrl, _) = fit(&f, FIT_TOL)?;
                Some(Piece::Spline { ctrl, analytic: Some(helix) })
            }
            Map::UnrollCone { s0, xs, ys, .. } => {
                let Segment::Line { .. } = seg else { return None };
                let dev = |q: P2| (q.x * xs.0 + q.y * ys.0, s0 + q.x * xs.1 + q.y * ys.1);
                let (xa, ya) = dev(a2);
                let (xb, yb) = dev(b2);
                let a = self.at(frame, a2, off)?;
                let b = self.at(frame, b2, off)?;
                let scale = s0.max(1.0);
                // Along one generator: the developed line passes through the
                // apex, so its two ends have the same bearing.
                let cross = xa * yb - ya * xb;
                if cross.abs() <= 1e-12 * scale * scale && xa * xb + ya * yb > 0.0 {
                    return Some(Piece::Line(a, b));
                }
                // Along one parallel: the same slant distance at both ends and
                // the developed segment... is a chord, not an arc — only an
                // arc of the sector is a parallel circle, and a sketch line
                // never is. Everything else is fitted.
                None
            }
            Map::ProjectQuadric { class, .. } => {
                let Segment::Line { .. } = seg else { return None };
                let a = self.at(frame, a2, off)?;
                let b = self.at(frame, b2, off)?;
                let mid = self.at(frame, P2::new((a2.x + b2.x) / 2.0, (a2.y + b2.y) / 2.0), off)?;
                // The ray plane of the segment contains the frame's normal
                // and the segment's direction.
                let dir = unit(frame.to_world(b2.x, b2.y) - frame.to_world(a2.x, a2.y))?;
                let ray_plane_normal = unit(dir.cross(frame.normal))?;
                match class {
                    SurfaceClass::Cylinder { axis, .. } | SurfaceClass::Cone { axis, .. } => {
                        // A plane through the axis cuts a generator: a line.
                        if ray_plane_normal.dot(*axis).abs() <= 1e-9 && (mid - a).cross(b - a).length() <= 1e-9 * (b - a).length().max(1.0) {
                            return Some(Piece::Line(a, b));
                        }
                        // A plane normal to the axis cuts a parallel: a circle.
                        if ray_plane_normal.dot(*axis).abs() >= 1.0 - 1e-9 {
                            return Some(Piece::Arc(a, b, mid));
                        }
                        None
                    }
                    // Every plane section of a sphere is a circle.
                    SurfaceClass::Sphere { .. } => Some(Piece::Arc(a, b, mid)),
                    SurfaceClass::Plane { .. } => Some(Piece::Line(a, b)),
                    _ => None,
                }
            }
            Map::ProjectSurface { .. } => None,
        }
    }

    /// The piece for `seg` at offset `off`: exact when [`Map::exact`] says
    /// so, else fitted. Returns the piece and the fit's measured deviation
    /// (zero for an exact one).
    fn piece(&self, frame: &Plane, seg: &Segment, off: f64) -> Result<(Piece, f64)> {
        if let Some(p) = self.exact(frame, seg, off) {
            return Ok((p, 0.0));
        }
        let f = |t: f64| self.at(frame, seg_point(seg, t), off);
        match fit(&f, FIT_TOL) {
            Some((ctrl, dev)) => Ok((Piece::Spline { ctrl, analytic: None }, dev)),
            None => Err(CadpoleError::invalid(
                "the segment cannot be laid on the face: the mapping has no answer along it (a projection \
                 that misses the surface, a cone's apex, or a curve too long to fit)",
            )),
        }
    }
}

/// The ray `p0 + t d` against the quadric `class` offset by `off` along its
/// natural normal, as the `t` nearest zero — the frame's plane touches the
/// surface at the wrap's origin, so the near root is the point under the
/// sketch and the far one the other side of the body.
fn ray_quadric(p0: Vec3, d: Vec3, class: &SurfaceClass, off: f64) -> Option<f64> {
    let nearest = |a: f64, b: f64, c: f64, ok: &dyn Fn(f64) -> bool| -> Option<f64> {
        if a.abs() <= 1e-15 {
            if b.abs() <= 1e-15 {
                return None;
            }
            let t = -c / b;
            return ok(t).then_some(t);
        }
        let disc = b * b - 4.0 * a * c;
        if disc < -1e-9 * (b * b).max(1.0) {
            return None;
        }
        let sq = disc.max(0.0).sqrt();
        let (t1, t2) = ((-b - sq) / (2.0 * a), (-b + sq) / (2.0 * a));
        let mut cands = [t1, t2];
        cands.sort_by(|x, y| x.abs().partial_cmp(&y.abs()).unwrap_or(std::cmp::Ordering::Equal));
        cands.into_iter().find(|t| ok(*t))
    };
    match *class {
        SurfaceClass::Plane { origin, normal } => {
            let denom = d.dot(normal);
            if denom.abs() <= 1e-15 {
                return None;
            }
            Some(((origin + normal * off) - p0).dot(normal) / denom)
        }
        SurfaceClass::Cylinder { axis_point, axis, radius } => {
            let r = radius + off;
            if r <= 0.0 {
                return None;
            }
            let w = p0 - axis_point;
            let wp = w - axis * w.dot(axis);
            let dp = d - axis * d.dot(axis);
            nearest(dp.dot(dp), 2.0 * wp.dot(dp), wp.dot(wp) - r * r, &|_| true)
        }
        SurfaceClass::Cone { apex, axis, half_angle } => {
            let apex = apex - axis * (off / fmath::sin(half_angle));
            let k = fmath::tan(half_angle);
            let k2 = k * k;
            let w = p0 - apex;
            let wa = w.dot(axis);
            let da = d.dot(axis);
            let wp = w - axis * wa;
            let dp = d - axis * da;
            let a = dp.dot(dp) - k2 * da * da;
            let b = 2.0 * (wp.dot(dp) - k2 * wa * da);
            let c = wp.dot(wp) - k2 * wa * wa;
            nearest(a, b, c, &|t| wa + t * da > 0.0)
        }
        SurfaceClass::Sphere { center, radius } => {
            let r = radius + off;
            if r <= 0.0 {
                return None;
            }
            let w = p0 - center;
            nearest(d.dot(d), 2.0 * w.dot(d), w.dot(w) - r * r, &|_| true)
        }
        SurfaceClass::Torus { .. } | SurfaceClass::Other => None,
    }
}

/// The point where the ray `p0 + t d` meets `surface`, by Newton on
/// `(u, v, t)` from the surface point nearest `p0`.
fn ray_surface(surface: &Surface, p0: Vec3, d: Vec3) -> Option<Vec3> {
    let (mut u, mut v) = crate::ksurface::nearest_parameter(surface, p0.point(), SPHint2D::None, 100)?;
    let start = kernel::point_to_vec3(surface.subs(u, v));
    let mut t = (start - p0).dot(d);
    let scale = (p0.length() + start.length()).max(1.0);
    for _ in 0..40 {
        let s = kernel::point_to_vec3(surface.subs(u, v));
        let f = s - p0 - d * t;
        if f.length() <= 1e-12 * scale {
            return Some(p0 + d * t);
        }
        let su = kernel::vector_to_vec3(surface.uder(u, v));
        let sv = kernel::vector_to_vec3(surface.vder(u, v));
        // Solve [su sv -d] δ = -f by Cramer's rule.
        let c0 = su;
        let c1 = sv;
        let c2 = -d;
        let det = c0.dot(c1.cross(c2));
        if det.abs() <= 1e-18 {
            return None;
        }
        let rhs = -f;
        let du = rhs.dot(c1.cross(c2)) / det;
        let dv = c0.dot(rhs.cross(c2)) / det;
        let dt = c0.dot(c1.cross(rhs)) / det;
        if !(du.is_finite() && dv.is_finite() && dt.is_finite()) {
            return None;
        }
        u += du;
        v += dv;
        t += dt;
    }
    let s = kernel::point_to_vec3(surface.subs(u, v));
    ((s - p0 - d * t).length() <= 1e-9 * scale).then(|| p0 + d * t)
}

/// A cubic Hermite spline through samples of `f` on `[0, 1]` with the
/// tangents of `f`, as `3n + 1` Bézier control points, with the span count
/// doubled until every span's interior is within `tol` of `f` — measured at
/// three points per span, against the true curve, so the number reported
/// is what was checked. `None` when `f` has no answer somewhere or the fit
/// does not converge within [`MAX_SPANS`].
fn fit(f: &dyn Fn(f64) -> Option<Vec3>, tol: f64) -> Option<(Vec<Vec3>, f64)> {
    let h = 1e-6;
    let tangent = |t: f64| -> Option<Vec3> {
        let (t0, t1) = ((t - h).max(0.0), (t + h).min(1.0));
        let (p0, p1) = (f(t0)?, f(t1)?);
        Some((p1 - p0) * (1.0 / (t1 - t0)))
    };
    let mut spans = 1usize;
    loop {
        let mut ctrl = Vec::with_capacity(3 * spans + 1);
        for k in 0..spans {
            let (t0, t1) = (k as f64 / spans as f64, (k + 1) as f64 / spans as f64);
            let (p0, p1) = (f(t0)?, f(t1)?);
            let (d0, d1) = (tangent(t0)?, tangent(t1)?);
            let hstep = (t1 - t0) / 3.0;
            if k == 0 {
                ctrl.push(p0);
            }
            ctrl.push(p0 + d0 * hstep);
            ctrl.push(p1 - d1 * hstep);
            ctrl.push(p1);
        }
        let mut worst = 0.0f64;
        for k in 0..spans {
            let c = &ctrl[3 * k..3 * k + 4];
            for q in [0.25, 0.5, 0.75] {
                let t = (k as f64 + q) / spans as f64;
                let on = f(t)?;
                let approx = bezier_point(c, q);
                worst = worst.max((on - approx).length());
            }
        }
        if worst <= tol {
            return Some((ctrl, worst));
        }
        if spans >= MAX_SPANS {
            return None;
        }
        spans *= 2;
    }
}

// --------------------------------------------------------------- Wrapped

/// A planar sketch laid on one face of a body: the frame it was placed in,
/// the sketch in that frame's coordinates, and the curve each loop makes on
/// the surface. What `wrap(...)` returns and what `imprint`/`emboss` take.
#[derive(Clone, Debug)]
pub struct Wrapped {
    pub body: BodyId,
    /// The face's full name.
    pub face: String,
    /// Its index in the body's topology, which is the kernel's face order.
    pub face_index: usize,
    pub method: Method,
    /// The face's recognised surface (`Other` for a free-form face).
    pub class: SurfaceClass,
    /// Origin on the surface, normal the face's outward normal there, `y`
    /// the sketch's up.
    pub frame: Plane,
    /// The sketch in the frame's coordinates: the loops that are imprinted.
    pub sketch: Sketch,
    /// The largest distance any fitted edge was measured to be from the true
    /// curve, in mm; zero when every edge is exact.
    pub deviation: f64,
    /// What the wrap wants the script to know (fit counts, a clipped path).
    pub notes: Vec<String>,
    /// The feature the artwork came from — the sketch, or the `wrap` that
    /// placed it — recorded as an input of the imprint or emboss beside the
    /// body, so the feature tree shows where the letters came from.
    pub source: Option<FeatureId>,
    map: Map,
    /// One entry per loop of `sketch`: its pieces on the surface at offset 0.
    pieces: Vec<Vec<Piece>>,
}

impl Wrapped {
    /// The notes so far, leaving none: a caller that records a feature of
    /// its own for the wrap attaches them there, so the imprint or emboss
    /// that follows does not repeat them.
    pub fn take_notes(&mut self) -> Vec<String> {
        std::mem::take(&mut self.notes)
    }

    /// One closed curve per loop, on the surface: exact segments where the
    /// kernel has them, a helix by its closed form, and cubic Béziers for
    /// the rest. Usable as a `loft` or `sweep_along` section.
    pub fn curves(&self) -> Vec<Curve3> {
        self.pieces
            .iter()
            .map(|pieces| {
                let segments: Vec<Segment3> = pieces.iter().flat_map(Piece::segments).collect();
                let start = segments.first().map(Segment3::start).unwrap_or(self.frame.origin);
                Curve3 { start, segments, closed: true }
            })
            .collect()
    }

    /// The name of loop `i`: its own, or `<index>`.
    pub fn loop_name(&self, i: usize) -> String {
        let own = self.sketch.loop_name(i);
        if own.is_empty() { i.to_string() } else { own.to_owned() }
    }

    /// The pieces of every loop at offset `off`, rebuilt from the map, with
    /// the worst fit deviation. `loops` are the 2D loops to map — the
    /// sketch's own, or their drafted copies.
    fn pieces_at(&self, loops: &[Loop], off: f64) -> Result<(Vec<Vec<Piece>>, f64)> {
        let mut out = Vec::with_capacity(loops.len());
        let mut worst = 0.0f64;
        for (li, l) in loops.iter().enumerate() {
            let mut pieces = Vec::with_capacity(l.segments.len());
            for (si, seg) in l.segments.iter().enumerate() {
                let (p, dev) = self.map.piece(&self.frame, seg, off).map_err(|e| {
                    CadpoleError::invalid(format!("loop {} segment {si}: {e}", self.loop_name(li)))
                })?;
                worst = worst.max(dev);
                pieces.push(p);
            }
            out.push(pieces);
        }
        Ok((out, worst))
    }

    /// This wrap with the sketch's outlines stroked into closed bands of
    /// `width` (a closed loop is its outward offset less
    /// its inward one, an open path the offset of the path doubled back on
    /// itself with its tips rounded), ready to be cut as a scribe. A loop
    /// that retraces itself — what `Sketch.add(path)` makes of an open path
    /// — is stroked as that path.
    pub fn stroked(&self, width: f64, tolerance: f64) -> Result<Wrapped> {
        if !(width > 0.0) || !width.is_finite() {
            return Err(CadpoleError::invalid(format!("scribe: width must be a positive number of millimetres, got {width}")));
        }
        let half = width / 2.0;
        let plane = self.sketch.plane;
        let mut stroke = Sketch::new(plane);
        let mut notes = self.notes.clone();
        let mut open: Vec<(String, Path)> = Vec::new();
        for (i, l) in self.sketch.loops.iter().enumerate() {
            let name = self.loop_name(i);
            if let Some(p) = retraced_path(l) {
                open.push((name, p));
                continue;
            }
            let one = Sketch::new(plane).with_named_loop(l.clone(), name.clone());
            let outer = sketch2d::offset(&one, "scribe", half, sketch2d::Join::Round, tolerance)?;
            let inner = sketch2d::offset(&one, "scribe", -half, sketch2d::Join::Round, tolerance)?;
            let band = if inner.sketch.loops.is_empty() {
                outer.sketch
            } else {
                sketch2d::boolean(&outer.sketch, "scribe", &inner.sketch, "scribe", sketch2d::BoolOp::Subtract, tolerance)?.sketch
            };
            for (k, b) in band.loops.iter().enumerate() {
                stroke.push_named_loop(b.clone(), format!("{name}/stroke[{k}]"));
            }
        }
        for (i, p) in self.sketch.paths.iter().enumerate() {
            let own = self.sketch.path_name(i);
            let name = if own.is_empty() { format!("path[{i}]") } else { own.to_owned() };
            open.push((name, p.clone()));
        }
        for (name, p) in &open {
            if p.segments.is_empty() {
                continue;
            }
            let mut segs = p.segments.clone();
            segs.extend(p.segments.iter().rev().map(reverse_segment));
            let doubled = Sketch::new(plane).with_named_loop(Loop { segments: segs }, name.clone());
            let band = sketch2d::offset(&doubled, "scribe", half, sketch2d::Join::Round, tolerance)?;
            if band.sketch.loops.is_empty() {
                notes.push(format!("scribe: path {name} produced no stroke and was skipped"));
            }
            // The doubled path reverses at both tips, and the offset
            // counts a reversal as a right turn: a positive offset closes
            // the tip with a butt cap through it. A stroke's end is round.
            let end = p.segments.iter().fold(p.start, |_, s| s.end());
            let open = fmath::hypot(end.x - p.start.x, end.y - p.start.y) > 1e-9;
            let tips = [
                (p.start, p.segments.first().and_then(Segment::start_direction).map(|d| P2::new(-d.x, -d.y))),
                (end, p.end_direction()),
            ];
            for (k, b) in band.sketch.loops.iter().enumerate() {
                let mut l = b.clone();
                if open {
                    for (tip, outward) in tips {
                        if let Some(o) = outward {
                            l = round_cap(l, tip, o, half);
                        }
                    }
                }
                stroke.push_named_loop(l, format!("{name}/stroke[{k}]"));
            }
        }
        if stroke.loops.is_empty() {
            return Err(CadpoleError::invalid("scribe: the sketch has no loop or path to stroke"));
        }
        let (pieces, dev) = {
            let tmp = Wrapped { sketch: stroke.clone(), pieces: Vec::new(), ..self.clone() };
            tmp.pieces_at(&stroke.loops, 0.0)?
        };
        Ok(Wrapped { sketch: stroke, pieces, deviation: self.deviation.max(dev), notes, ..self.clone() })
    }
}

/// The butt cap that the offset leaves at a doubled path's tip (a
/// reversal counts as a right turn there, so a positive offset spikes
/// through the tip instead of rounding it), replaced by the semicircle
/// about the tip: the run of line segments across the tip — perpendicular
/// to `outward`, within `half` of the tip — becomes one arc through
/// `tip + outward · half`. A run that is not the full cap (two strokes'
/// ends overlapping) is left as it is.
fn round_cap(l: Loop, tip: P2, outward: P2, half: f64) -> Loop {
    let eps = 1e-9 * half.max(1.0);
    let on_cap = |p: P2| {
        let (dx, dy) = (p.x - tip.x, p.y - tip.y);
        (dx * outward.x + dy * outward.y).abs() <= eps && fmath::hypot(dx, dy) <= half + eps
    };
    let is_cap = |s: &Segment| matches!(s, Segment::Line { a, b } if on_cap(*a) && on_cap(*b));
    let n = l.segments.len();
    let Some(first) = (0..n).find(|i| !is_cap(&l.segments[*i])) else { return l };
    let cap_arc = |a: P2, b: P2| -> Segment {
        let at_rim = |p: P2| (fmath::hypot(p.x - tip.x, p.y - tip.y) - half).abs() <= eps;
        if at_rim(a) && at_rim(b) && (fmath::hypot(a.x - b.x, a.y - b.y) - 2.0 * half).abs() <= eps {
            Segment::Arc { a, b, via: P2::new(tip.x + outward.x * half, tip.y + outward.y * half) }
        } else {
            Segment::Line { a, b }
        }
    };
    let mut out = Vec::with_capacity(n);
    let mut run: Option<(P2, P2)> = None;
    for k in 0..n {
        let s = &l.segments[(first + k) % n];
        if is_cap(s) {
            let (a, b) = (s.start(), s.end());
            run = Some(match run {
                None => (a, b),
                Some((ra, _)) => (ra, b),
            });
        } else {
            if let Some((a, b)) = run.take() {
                out.push(cap_arc(a, b));
            }
            out.push(s.clone());
        }
    }
    if let Some((a, b)) = run.take() {
        out.push(cap_arc(a, b));
    }
    Loop { segments: out }
}

/// A closed loop that retraces itself — its second half is its first half
/// run backwards, which is what `Sketch.add(path)` makes of an open path —
/// read back as that path; `None` for any other loop.
fn retraced_path(l: &Loop) -> Option<Path> {
    let n = l.segments.len();
    if n == 0 || n % 2 != 0 {
        return None;
    }
    let half = n / 2;
    let same = |a: P2, b: P2| (a.x - b.x).abs() <= 1e-9 && (a.y - b.y).abs() <= 1e-9;
    let reversed = |s: &Segment, r: &Segment| match (s, r) {
        (Segment::Line { a, b }, Segment::Line { a: ra, b: rb }) => same(*a, *rb) && same(*b, *ra),
        (Segment::Arc { a, b, via }, Segment::Arc { a: ra, b: rb, via: rv }) => {
            same(*a, *rb) && same(*b, *ra) && same(*via, *rv)
        }
        (Segment::Bezier { a, b, ctrl }, Segment::Bezier { a: ra, b: rb, ctrl: rc }) => {
            same(*a, *rb) && same(*b, *ra) && ctrl.len() == rc.len() && ctrl.iter().zip(rc.iter().rev()).all(|(c, d)| same(*c, *d))
        }
        _ => false,
    };
    (0..half)
        .all(|i| reversed(&l.segments[i], &l.segments[n - 1 - i]))
        .then(|| Path { start: l.segments[0].start(), segments: l.segments[..half].to_vec() })
}

fn reverse_segment(s: &Segment) -> Segment {
    match s {
        Segment::Line { a, b } => Segment::Line { a: *b, b: *a },
        Segment::Arc { a, b, via } => Segment::Arc { a: *b, b: *a, via: *via },
        Segment::Bezier { a, b, ctrl } => Segment::Bezier { a: *b, b: *a, ctrl: ctrl.iter().rev().copied().collect() },
    }
}

// ------------------------------------------------------------ the face

/// One face of an exact body, with what the wrap and the rebuild need of it.
struct FaceOn {
    index: usize,
    name: String,
    class: SurfaceClass,
    /// The face's surface, oriented outward.
    surface: Surface,
    face: Face,
}

/// The face named `face` of `body`, which must be a B-rep body.
fn face_of(ctx: &mut Context, body: BodyId, face: &str, what: &str) -> Result<FaceOn> {
    let body_name = ctx.body(body)?.name.clone();
    let solid = match &ctx.body(body)?.body {
        Body::Brep(s) => s.clone(),
        Body::Mesh(_) => {
            return Err(CadpoleError::invalid(format!(
                "{what}: `{body_name}` is a mesh body, which has no exact face to split; an imprint rebuilds a \
                 B-rep face around the sketch's loops, and the mesh path (`field.from_sketch`) is not built"
            )));
        }
    };
    let (_, topo) = ctx.body_mesh(body)?;
    let index = topo
        .faces
        .iter()
        .position(|f| f.name == face)
        .or_else(|| {
            let hits: Vec<usize> = topo.faces.iter().enumerate().filter(|(_, f)| f.aliases.iter().any(|a| a == face)).map(|(i, _)| i).collect();
            (hits.len() == 1).then(|| hits[0])
        })
        .ok_or_else(|| {
            let mut names: Vec<&str> = topo.faces.iter().map(|f| f.name.as_str()).collect();
            names.sort_unstable();
            names.truncate(12);
            CadpoleError::invalid(format!("{what}: `{body_name}` has no face named `{face}`; its faces are {}", names.join(", ")))
        })?;
    let class = topo.faces[index].surface.clone();
    let name = topo.faces[index].name.clone();
    let face = solid
        .face_iter()
        .nth(index)
        .cloned()
        .ok_or_else(|| CadpoleError::kernel(format!("{what}: face {index} of `{body_name}` is not in its solid")))?;
    let surface = face.oriented_surface();
    Ok(FaceOn { index, name, class, surface, face })
}

/// The `(u, v)` of `p` on `surface`: truck's inversion from the hint, then
/// from nothing, then the nearest point from each when both refuse. Every
/// answer is held to the surface's parameter range — read one period over
/// when that keeps the point (a seam), refused otherwise — and to the
/// point itself: a hint on the far side of a seam sends Newton out of the
/// range, and on a surface of revolution the profile's extension past the
/// pole is on the same surface, so an out-of-range answer is not off the
/// point and only the range tells it from the right one. A boundary sample
/// of an edge a boolean made can sit a hair off the surface, hence the
/// nearest-point fallback within 1e-4.
fn uv_of(surface: &Surface, p: Vec3, hint: Option<(f64, f64)>) -> Option<(f64, f64)> {
    let (ur, vr) = surface.try_range_tuple();
    // Only a surface of revolution is held to its range: a plane is
    // parametrised on [0, 1]² by three points but extends past them, and a
    // point just off a planar face must reach the containment check, which
    // names the real offence.
    let strict = matches!(surface, Surface::RevolutedCurve(_));
    let fit = |x: f64, r: Option<(f64, f64)>| -> Option<f64> {
        let Some((a, b)) = r.filter(|_| strict) else { return x.is_finite().then_some(x) };
        let (lo, hi) = (a.min(b), a.max(b));
        let len = hi - lo;
        let eps = 1e-9 * len.max(1.0);
        if x >= lo - eps && x <= hi + eps {
            return Some(x.clamp(lo, hi));
        }
        if len > 0.0 {
            let w = lo + (x - lo).rem_euclid(len);
            if w >= lo - eps && w <= hi + eps {
                return Some(w.clamp(lo, hi));
            }
        }
        None
    };
    let accept = |uv: (f64, f64)| -> Option<(f64, f64)> {
        let uv = (fit(uv.0, ur)?, fit(uv.1, vr)?);
        let back = kernel::point_to_vec3(surface.subs(uv.0, uv.1));
        ((back - p).length() <= 1e-4).then_some(uv)
    };
    let hinted = match hint {
        Some((u, v)) => SPHint2D::Parameter(u, v),
        None => SPHint2D::None,
    };
    let hints: &[SPHint2D] = if hint.is_some() { &[hinted, SPHint2D::None] } else { &[SPHint2D::None] };
    for h in hints {
        if let Some(uv) = surface.search_parameter(p.point(), *h, 100).and_then(accept) {
            return Some(uv);
        }
    }
    for h in hints {
        if let Some(uv) = crate::ksurface::nearest_parameter(surface, p.point(), *h, 100).and_then(accept) {
            return Some(uv);
        }
    }
    None
}

/// The surface's outward normal at a point on it.
fn normal_at(surface: &Surface, p: Vec3) -> Option<Vec3> {
    let (u, v) = uv_of(surface, p, None)?;
    unit(kernel::vector_to_vec3(surface.normal(u, v)))
}

/// The point of the face's surface nearest `p`, by the class where it is
/// closed form and by the surface otherwise.
fn nearest_on(class: &SurfaceClass, surface: &Surface, p: Vec3) -> Option<Vec3> {
    match *class {
        SurfaceClass::Plane { origin, normal } => Some(p - normal * (p - origin).dot(normal)),
        SurfaceClass::Cylinder { axis_point, axis, radius } => {
            let w = p - axis_point;
            let along = w.dot(axis);
            let radial = unit(w - axis * along)?;
            Some(axis_point + axis * along + radial * radius)
        }
        SurfaceClass::Cone { apex, axis, half_angle } => {
            let w = p - apex;
            let h = w.dot(axis);
            let radial = unit(w - axis * h)?;
            let (sa, ca) = fmath::sin_cos(half_angle);
            let s = h * ca + (w - axis * h).length() * sa;
            (s > 0.0).then(|| apex + (axis * ca + radial * sa) * s)
        }
        SurfaceClass::Sphere { center, radius } => Some(center + unit(p - center)? * radius),
        SurfaceClass::Torus { .. } | SurfaceClass::Other => {
            let (u, v) = crate::ksurface::nearest_parameter(surface, p.point(), SPHint2D::None, 100)?;
            Some(kernel::point_to_vec3(surface.subs(u, v)))
        }
    }
}

/// The face's boundary in its surface's parameter space: the outer loop
/// and the holes, each a polygon of inverted samples made continuous
/// across the surface's seams ([`unwrap_polygon`]). A boundary that goes
/// right round the surface (a hemisphere's equator, closed through the
/// pole) is refused: truck closes such a face along the pole line when it
/// triangulates it, and cannot once the face has an inner wire.
struct UvBoundary {
    outer: Vec<(f64, f64)>,
    holes: Vec<Vec<(f64, f64)>>,
    /// The length of each parameter range, when the surface is bounded: a
    /// jump of more than half of it between two neighbouring samples is a
    /// seam, and a polygon or a point may be read one period over.
    period: (Option<f64>, Option<f64>),
}

/// `poly` made continuous: whenever two neighbouring samples jump by more
/// than half a period, the rest of the polygon is read one period over.
/// Returns the polygon and the whole periods its closing edge (last to
/// first) still spans — zero for a polygon that merely straddles a seam, a
/// period for a boundary that goes right round the surface, such as a
/// hemisphere's equator.
fn unwrap_polygon(poly: &[(f64, f64)], period: (Option<f64>, Option<f64>)) -> (Vec<(f64, f64)>, (f64, f64)) {
    let mut out: Vec<(f64, f64)> = Vec::with_capacity(poly.len());
    let (mut su, mut sv) = (0.0f64, 0.0f64);
    for &(u, v) in poly {
        if let Some(&(pu, pv)) = out.last() {
            if let Some(p) = period.0 {
                let d = u + su - pu;
                if d > p / 2.0 {
                    su -= p;
                } else if d < -p / 2.0 {
                    su += p;
                }
            }
            if let Some(p) = period.1 {
                let d = v + sv - pv;
                if d > p / 2.0 {
                    sv -= p;
                } else if d < -p / 2.0 {
                    sv += p;
                }
            }
        }
        out.push((u + su, v + sv));
    }
    let net = |a: f64, b: f64, p: Option<f64>| match p {
        Some(p) if (b - a).abs() > p / 2.0 => ((b - a) / p).round() * p,
        _ => 0.0,
    };
    let winding = match (out.first(), out.last()) {
        (Some(a), Some(b)) => (net(a.0, b.0, period.0), net(a.1, b.1, period.1)),
        _ => (0.0, 0.0),
    };
    (out, winding)
}

fn uv_boundary(on: &FaceOn, what: &str) -> Result<UvBoundary> {
    let (ur, vr) = on.surface.try_range_tuple();
    let period = (ur.map(|(a, b)| (b - a).abs()), vr.map(|(a, b)| (b - a).abs()));
    let mut polys: Vec<Vec<(f64, f64)>> = Vec::new();
    for wire in on.face.boundaries() {
        let mut poly = Vec::new();
        let mut hint = None;
        for edge in wire.edge_iter() {
            let curve = edge.oriented_curve();
            let (t0, t1) = curve.range_tuple();
            for i in 0..CHECK_SAMPLES {
                let t = t0 + (t1 - t0) * (i as f64 + 0.5) / CHECK_SAMPLES as f64;
                let p = kernel::point_to_vec3(curve.subs(t));
                let uv = uv_of(&on.surface, p, hint).ok_or_else(|| {
                    CadpoleError::kernel(format!(
                        "{what}: a point of `{}`'s boundary could not be placed on its own surface",
                        on.name
                    ))
                })?;
                hint = Some(uv);
                poly.push(uv);
            }
        }
        let (poly, winding) = unwrap_polygon(&poly, period);
        if winding.0 != 0.0 || winding.1 != 0.0 {
            return Err(CadpoleError::invalid(format!(
                "{what}: the boundary of `{}` goes right round its surface, so the face closes through a pole (a \
                 hemisphere, a cap that reaches the axis); the tessellator cannot triangulate such a face with a loop \
                 cut into it, so an imprint needs a face bounded on every side — revolve the profile short of the \
                 axis, or split the body first",
                on.name
            )));
        }
        polys.push(poly);
    }
    if polys.is_empty() {
        return Err(CadpoleError::kernel(format!("{what}: `{}` has no boundary", on.name)));
    }
    let area = |p: &Vec<(f64, f64)>| -> f64 {
        let n = p.len();
        (0..n).map(|i| p[i].0 * p[(i + 1) % n].1 - p[(i + 1) % n].0 * p[i].1).sum::<f64>().abs()
    };
    let outer_i = (0..polys.len()).max_by(|a, b| area(&polys[*a]).partial_cmp(&area(&polys[*b])).unwrap()).unwrap();
    let outer = polys.remove(outer_i);
    Ok(UvBoundary { outer, holes: polys, period })
}

fn point_in_polygon(p: (f64, f64), poly: &[(f64, f64)]) -> bool {
    let n = poly.len();
    let mut inside = false;
    let mut j = n - 1;
    for i in 0..n {
        let (xi, yi) = poly[i];
        let (xj, yj) = poly[j];
        if (yi > p.1) != (yj > p.1) {
            let x = xi + (p.1 - yi) * (xj - xi) / (yj - yi);
            if p.0 < x {
                inside = !inside;
            }
        }
        j = i;
    }
    inside
}

fn segments_cross(a: (f64, f64), b: (f64, f64), c: (f64, f64), d: (f64, f64)) -> bool {
    let orient = |p: (f64, f64), q: (f64, f64), r: (f64, f64)| (q.0 - p.0) * (r.1 - p.1) - (q.1 - p.1) * (r.0 - p.0);
    let (o1, o2, o3, o4) = (orient(a, b, c), orient(a, b, d), orient(c, d, a), orient(c, d, b));
    (o1 > 0.0) != (o2 > 0.0) && (o3 > 0.0) != (o4 > 0.0) && o1 != 0.0 && o2 != 0.0 && o3 != 0.0 && o4 != 0.0
}

/// Every loop lies inside the face, off its boundary and its holes, and on
/// one side of the surface's seam; the offence is named otherwise.
fn check_loops_on_face(on: &FaceOn, w: &Wrapped, what: &str) -> Result<()> {
    let bound = uv_boundary(on, what)?;
    let half = |p: Option<f64>| p.map(|p| 0.5 * p);
    let (hu, hv) = (half(bound.period.0), half(bound.period.1));
    let mut walls: Vec<((f64, f64), (f64, f64))> = Vec::new();
    for poly in std::iter::once(&bound.outer).chain(bound.holes.iter()) {
        let n = poly.len();
        walls.extend((0..n).map(|i| (poly[i], poly[(i + 1) % n])));
    }
    // The sheets a point may be read on: its own, and one period either
    // way in each periodic direction, since the face's polygon was unwrapped
    // from wherever its boundary happened to start.
    let shifts: Vec<(f64, f64)> = {
        let us: Vec<f64> = bound.period.0.map_or(vec![0.0], |p| vec![0.0, p, -p]);
        let vs: Vec<f64> = bound.period.1.map_or(vec![0.0], |p| vec![0.0, p, -p]);
        us.iter().flat_map(|u| vs.iter().map(move |v| (*u, *v))).collect()
    };
    let inside = |p: (f64, f64)| point_in_polygon(p, &bound.outer) && !bound.holes.iter().any(|h| point_in_polygon(p, h));
    for (li, pieces) in w.pieces.iter().enumerate() {
        let name = w.loop_name(li);
        let mut uvs: Vec<(f64, f64)> = Vec::new();
        let mut hint = None;
        for p in pieces.iter().flat_map(|p| p.samples(CHECK_SAMPLES)) {
            let uv = uv_of(&on.surface, p, hint).ok_or_else(|| {
                CadpoleError::invalid(format!(
                    "{what}: loop {name} leaves the surface of `{}` at ({}, {}, {}); it must lie on the face",
                    on.name,
                    fmt_num(p.x),
                    fmt_num(p.y),
                    fmt_num(p.z)
                ))
            })?;
            hint = Some(uv);
            uvs.push(uv);
        }
        for pair in uvs.windows(2) {
            let (a, b) = (pair[0], pair[1]);
            let jump_u = hu.is_some_and(|h| (a.0 - b.0).abs() > h);
            let jump_v = hv.is_some_and(|h| (a.1 - b.1).abs() > h);
            if jump_u || jump_v {
                return Err(CadpoleError::invalid(format!(
                    "{what}: loop {name} crosses the seam of `{}`, where the face's surface starts over; the loops \
                     must sit within one face — move them with origin=, or turn the body so the seam is elsewhere",
                    on.name
                )));
            }
        }
        let Some(first) = uvs.first().copied() else { continue };
        let shift = shifts.iter().copied().find(|(du, dv)| inside((first.0 + du, first.1 + dv)));
        let Some((du, dv)) = shift else {
            return Err(CadpoleError::invalid(format!(
                "{what}: loop {name} is not inside `{}` (its outline starts outside the face or in a hole of it); an \
                 imprint splits one face, so clip the sketch to the face (`intersect2d`) or move it with origin=",
                on.name
            )));
        };
        let uvs: Vec<(f64, f64)> = uvs.into_iter().map(|(u, v)| (u + du, v + dv)).collect();
        for (k, uv) in uvs.iter().enumerate() {
            if !inside(*uv) {
                return Err(CadpoleError::invalid(format!(
                    "{what}: loop {name} is not inside `{}` (point {k} of its outline falls outside the face or in a \
                     hole of it); an imprint splits one face, so clip the sketch to the face (`intersect2d`) or move it \
                     with origin=",
                    on.name
                )));
            }
        }
        let n = uvs.len();
        for i in 0..n {
            let (a, b) = (uvs[i], uvs[(i + 1) % n]);
            if walls.iter().any(|(c, d)| segments_cross(a, b, *c, *d)) {
                return Err(CadpoleError::invalid(format!(
                    "{what}: loop {name} crosses the boundary of `{}`; an imprint splits one face, so clip the sketch \
                     to the face (`intersect2d`) or move it with origin=",
                    on.name
                )));
            }
        }
    }
    Ok(())
}

// --------------------------------------------------------------- wrapping

/// The frame's `x` and `y` expressed in the tangent basis `(e_s, e_t)`.
fn in_basis(frame: &Plane, e_s: Vec3, e_t: Vec3) -> ((f64, f64), (f64, f64)) {
    let y = frame.y_dir();
    ((frame.x_dir.dot(e_s), frame.x_dir.dot(e_t)), (y.dot(e_s), y.dot(e_t)))
}

impl Context {
    /// Lay `sketch` on the face `face` of `body` by `method`.
    ///
    /// `origin` is a point on (or near) the face where the sketch's own
    /// origin lands, with the sketch's `+y` along `up` projected onto the
    /// face: the sketch's coordinates are then read on the face. Without
    /// it, `project` takes the sketch where it is in space — its outline
    /// projected along the face's normal at the surface point nearest the
    /// sketch's origin, an identity for a sketch drawn on a planar face —
    /// and `unroll` places the sketch's origin at the middle of the face,
    /// `up` along the axis. What comes back carries the frame, the sketch in
    /// it, and the curve of every loop on the surface, ready for
    /// [`Context::imprint`] and [`Context::emboss`]; its notes are the
    /// caller's to attach.
    pub fn wrap_sketch(
        &mut self,
        body: BodyId,
        face: &str,
        sketch: &Sketch,
        method: Method,
        origin: Option<Vec3>,
        up: Option<Vec3>,
    ) -> Result<Wrapped> {
        let what = "wrap";
        let on = face_of(self, body, face, what)?;
        let body_name = self.body(body)?.name.clone();
        let (_, topo) = self.body_mesh(body)?;
        let centroid = topo.faces[on.index].centroid;
        let mut notes = Vec::new();

        // ---- what the method can do with this face
        match (method, &on.class) {
            (Method::Unroll, SurfaceClass::Plane { .. } | SurfaceClass::Cylinder { .. } | SurfaceClass::Cone { .. }) => {}
            (Method::Unroll, other) => {
                return Err(CadpoleError::invalid(format!(
                    "{what}: `{}` is a {} face, which does not develop onto a plane (Gauss: no isometry); unroll needs \
                     a cylinder or a cone — pass method=\"project\" for this face",
                    on.name,
                    other.kind_name()
                )));
            }
            (Method::Project, _) => {}
        }

        // ---- the origin on the surface, and the frame
        let placement = origin.is_some() || method == Method::Unroll;
        let seed = match origin {
            Some(o) => o,
            None if method == Method::Unroll => centroid,
            None => sketch.plane.origin,
        };
        let o = nearest_on(&on.class, &on.surface, seed).ok_or_else(|| {
            CadpoleError::invalid(format!(
                "{what}: no point of `{}` is nearest ({}, {}, {}) — it is on the axis or at the apex; pass origin= to say \
                 where on the face the sketch's origin lands",
                on.name,
                fmt_num(seed.x),
                fmt_num(seed.y),
                fmt_num(seed.z)
            ))
        })?;
        let n = normal_at(&on.surface, o).ok_or_else(|| {
            CadpoleError::kernel(format!("{what}: `{}` has no normal at its origin", on.name))
        })?;
        let default_up = match (&on.class, method) {
            (SurfaceClass::Cylinder { axis, .. }, _) => *axis,
            (SurfaceClass::Cone { apex, axis, half_angle }, _) => {
                let radial = unit(o - *apex - *axis * (o - *apex).dot(*axis)).unwrap_or(Vec3::Z);
                let (sa, ca) = fmath::sin_cos(*half_angle);
                *axis * ca + radial * sa
            }
            (_, Method::Project) if !placement => sketch.plane.y_dir(),
            _ => {
                if n.z.abs() < 0.9 {
                    Vec3::Z
                } else {
                    Vec3::Y
                }
            }
        };
        let up_raw = up.unwrap_or(default_up);
        let y_dir = unit(up_raw - n * up_raw.dot(n)).ok_or_else(|| {
            CadpoleError::invalid(format!(
                "{what}: up=({}, {}, {}) is along the normal of `{}` at the origin; it must lie across the face",
                fmt_num(up_raw.x),
                fmt_num(up_raw.y),
                fmt_num(up_raw.z),
                on.name
            ))
        })?;
        let x_dir = y_dir.cross(n);
        let frame = Plane::new(o, n, x_dir)?;

        // ---- the sketch in the frame
        let sketch2 = if placement {
            Sketch { plane: frame, ..sketch.clone() }
        } else {
            // The sketch where it is: its plane must be parallel to the
            // frame's, so its segments stay what they are.
            if sketch.plane.normal.dot(frame.normal).abs() < 1.0 - 1e-9 {
                return Err(CadpoleError::invalid(format!(
                    "{what}: the sketch's plane is not parallel to `{}` at the origin (normals {:.1}° apart); draw \
                     it on a parallel plane, or pass origin= to place it on the face by its own coordinates",
                    on.name,
                    fmath::acos(sketch.plane.normal.dot(frame.normal).abs().min(1.0)).to_degrees()
                )));
            }
            let (ax, ay) = (frame.x_dir, frame.y_dir());
            let map = |p: P2| {
                let w = sketch.plane.to_world(p.x, p.y) - o;
                P2::new(w.dot(ax), w.dot(ay))
            };
            let mut s = Sketch::new(frame);
            for (i, l) in sketch.loops.iter().enumerate() {
                s.push_named_loop(map_loop(l, &map), sketch.loop_name(i));
            }
            for (i, p) in sketch.paths.iter().enumerate() {
                s.push_path(map_path(p, &map), sketch.path_name(i));
            }
            s
        };

        // ---- the map
        let map = match (method, &on.class) {
            (_, SurfaceClass::Plane { .. }) => Map::Plane,
            (Method::Unroll, SurfaceClass::Cylinder { axis_point, axis, radius }) => {
                let radial0 = unit(o - *axis_point - *axis * (o - *axis_point).dot(*axis))
                    .ok_or_else(|| CadpoleError::invalid(format!("{what}: the origin is on the axis of `{}`", on.name)))?;
                let circ0 = axis.cross(radial0);
                let sign = if n.dot(radial0) >= 0.0 { 1.0 } else { -1.0 };
                let centre = *axis_point + *axis * (o - *axis_point).dot(*axis);
                let (xs, ys) = in_basis(&frame, circ0, *axis);
                Map::UnrollCylinder { centre, axis: *axis, radial0, circ0, r: *radius, sign, xs, ys }
            }
            (Method::Unroll, SurfaceClass::Cone { apex, axis, half_angle }) => {
                let w = o - *apex;
                let radial0 = unit(w - *axis * w.dot(*axis))
                    .ok_or_else(|| CadpoleError::invalid(format!("{what}: the origin is on the axis of `{}`", on.name)))?;
                let circ0 = axis.cross(radial0);
                let (sa, ca) = fmath::sin_cos(*half_angle);
                let g0 = *axis * ca + radial0 * sa;
                let natural = radial0 * ca - *axis * sa;
                let sign = if n.dot(natural) >= 0.0 { 1.0 } else { -1.0 };
                let (xs, ys) = in_basis(&frame, circ0, g0);
                Map::UnrollCone { apex: *apex, axis: *axis, radial0, circ0, half_angle: *half_angle, s0: w.length(), sign, xs, ys }
            }
            (Method::Unroll, _) => unreachable!("refused above"),
            (Method::Project, class @ (SurfaceClass::Cylinder { .. } | SurfaceClass::Cone { .. } | SurfaceClass::Sphere { .. })) => {
                let natural = class.normal_at(o).unwrap_or(n);
                let sign = if n.dot(natural) >= 0.0 { 1.0 } else { -1.0 };
                Map::ProjectQuadric { class: class.clone(), sign }
            }
            (Method::Project, _) => Map::ProjectSurface { surface: Arc::new(on.surface.clone()) },
        };

        let mut w = Wrapped {
            body,
            face: on.name.clone(),
            face_index: on.index,
            method,
            class: on.class.clone(),
            frame,
            sketch: sketch2,
            deviation: 0.0,
            notes: Vec::new(),
            source: None,
            map,
            pieces: Vec::new(),
        };
        let loops = w.sketch.loops.clone();
        let (pieces, dev) = w.pieces_at(&loops, 0.0).map_err(|e| CadpoleError::invalid(format!("{what} onto `{}`: {e}", on.name)))?;
        w.pieces = pieces;
        w.deviation = dev;
        let fitted = w.pieces.iter().flatten().filter(|p| matches!(p, Piece::Spline { .. })).count();
        let spans: usize = w
            .pieces
            .iter()
            .flatten()
            .map(|p| match p {
                Piece::Spline { ctrl, .. } => (ctrl.len() - 1) / 3,
                _ => 0,
            })
            .sum();
        let exact = w.pieces.iter().flatten().count() - fitted;
        if fitted > 0 {
            notes.push(format!(
                "{what}: {} loop(s) on `{}` by {}: {exact} exact edge(s), {fitted} fitted as {spans} cubic span(s) within {} mm of the \
                 surface (the largest deviation measured: {:.2e} mm)",
                w.sketch.loops.len(),
                on.name,
                method.name(),
                fmt_num(FIT_TOL),
                w.deviation
            ));
        } else {
            notes.push(format!(
                "{what}: {} loop(s) on `{}` by {}: every edge exact ({} on a {})",
                w.sketch.loops.len(),
                on.name,
                method.name(),
                exact,
                on.class.kind_name()
            ));
        }
        let _ = body_name;
        w.notes = notes;
        Ok(w)
    }
}

fn map_loop(l: &Loop, map: &dyn Fn(P2) -> P2) -> Loop {
    Loop { segments: l.segments.iter().map(|s| map_segment(s, map)).collect() }
}

fn map_path(p: &Path, map: &dyn Fn(P2) -> P2) -> Path {
    Path { start: map(p.start), segments: p.segments.iter().map(|s| map_segment(s, map)).collect() }
}

fn map_segment(s: &Segment, map: &dyn Fn(P2) -> P2) -> Segment {
    match s {
        Segment::Line { a, b } => Segment::Line { a: map(*a), b: map(*b) },
        Segment::Arc { a, b, via } => Segment::Arc { a: map(*a), b: map(*b), via: map(*via) },
        Segment::Bezier { a, b, ctrl } => Segment::Bezier { a: map(*a), b: map(*b), ctrl: ctrl.iter().map(|c| map(*c)).collect() },
    }
}

// ------------------------------------------------------------ the draft

/// `l` moved inward (to its left, the material's side for a loop oriented
/// by nesting) by `d`, corner by corner: a line stays parallel, an arc
/// concentric, and each corner is the mitre of its two offset pieces.
/// Refused when the loop turns back on itself at a corner sharper than the
/// offset can mitre, or when the result crosses itself or vanishes.
fn drafted(l: &Loop, d: f64, name: &str) -> Result<Loop> {
    let n = l.segments.len();
    if n == 0 {
        return Ok(l.clone());
    }
    let left = |t: P2| P2::new(-t.y, t.x);
    // The offset corner at the start of segment i (between i-1 and i).
    let mut corners = Vec::with_capacity(n);
    for i in 0..n {
        let prev = &l.segments[(i + n - 1) % n];
        let cur = &l.segments[i];
        if matches!(cur, Segment::Bezier { .. }) {
            return Err(CadpoleError::invalid(format!(
                "draft: loop {name} has a spline segment, whose parallel curve is not a spline; draft needs lines and \
                 arcs (text is polygonal), or pass draft=0"
            )));
        }
        let t1 = prev.end_direction().unwrap_or(P2::new(1.0, 0.0));
        let t2 = cur.start_direction().unwrap_or(t1);
        let (n1, n2) = (left(t1), left(t2));
        let dotn = n1.x * n2.x + n1.y * n2.y;
        if 1.0 + dotn < 0.05 {
            return Err(CadpoleError::invalid(format!(
                "draft: loop {name} turns back on itself at corner {i}; a drafted wall cannot mitre there"
            )));
        }
        let v = cur.start();
        let k = d / (1.0 + dotn);
        corners.push(P2::new(v.x + k * (n1.x + n2.x), v.y + k * (n1.y + n2.y)));
    }
    let mut segments = Vec::with_capacity(n);
    for i in 0..n {
        let a = corners[i];
        let b = corners[(i + 1) % n];
        segments.push(match &l.segments[i] {
            Segment::Line { .. } => Segment::Line { a, b },
            Segment::Arc { a: a0, b: b0, via } => match arc_params(*a0, *b0, *via) {
                Some((c, r, t0, t1)) => {
                    // A left turn keeps its centre on the left: inward is
                    // toward it.
                    let r2 = if t1 > t0 { r - d } else { r + d };
                    if r2 <= 0.0 {
                        return Err(CadpoleError::invalid(format!(
                            "draft: loop {name} has an arc of radius {} that a draft of {} collapses",
                            fmt_num(r),
                            fmt_num(d)
                        )));
                    }
                    let tm = (t0 + t1) / 2.0;
                    let (s, co) = fmath::sin_cos(tm);
                    Segment::Arc { a, b, via: P2::new(c.x + r2 * co, c.y + r2 * s) }
                }
                None => Segment::Line { a, b },
            },
            Segment::Bezier { .. } => unreachable!("refused above"),
        });
    }
    let out = Loop { segments };
    let (a0, a1) = (l.signed_area(), out.signed_area());
    if a0.signum() != a1.signum() || a1.abs() < 1e-9 * a0.abs().max(1e-9) {
        return Err(CadpoleError::invalid(format!(
            "draft: loop {name} vanishes at an inset of {} — a stroke thinner than twice the inset; use less draft or \
             less height",
            fmt_num(d)
        )));
    }
    let pts = out.sample();
    let m = pts.len();
    for i in 0..m {
        for j in i + 2..m {
            if i == 0 && j == m - 1 {
                continue;
            }
            let (a, b, c, d2) = (pts[i], pts[i + 1], pts[j], pts[(j + 1) % m]);
            if segments_cross((a.x, a.y), (b.x, b.y), (c.x, c.y), (d2.x, d2.y)) {
                return Err(CadpoleError::invalid(format!(
                    "draft: loop {name} crosses itself at an inset of {}; use less draft or less height",
                    fmt_num(d)
                )));
            }
        }
    }
    Ok(out)
}

// ------------------------------------------------------------- the relief

/// What an emboss does to the imprinted pieces.
#[derive(Clone, Copy, Debug, PartialEq)]
enum Relief {
    /// Split only.
    Imprint,
    /// Extrude the pieces by `height` along the face's outward normal (a
    /// raise) or against it (a cut), the walls drafted inward by
    /// `tan(draft) · height`.
    Extrude { height: f64, draft: f64 },
}

/// What [`Context::imprint`] and [`Context::emboss`] hand back.
#[derive(Clone, Debug)]
pub struct Embossed {
    pub body: BodyId,
    pub feature: FeatureId,
    /// The artwork faces: the pieces inside the loops (imprint, paint), or
    /// the tops of the raised or cut regions.
    pub faces: Vec<String>,
}

/// A face of the rebuilt solid with its name, in the order the shell lists
/// them.
struct Built {
    face: Face,
    name: NamedFace,
}

/// A wire from pieces, sharing vertices with its neighbours and closing on
/// its first vertex.
fn wire_of(pieces: &[Piece]) -> Result<Wire> {
    if pieces.is_empty() {
        return Err(CadpoleError::invalid("empty loop"));
    }
    let n = pieces.len();
    let vertices: Vec<Vertex> = pieces.iter().map(|p| builder::vertex(p.start().point())).collect();
    let mut wire = Wire::new();
    for (i, p) in pieces.iter().enumerate() {
        let to = &vertices[(i + 1) % n];
        let gap = (p.end() - kernel::point_to_vec3(to.point())).length();
        if gap > 1e-6 {
            return Err(CadpoleError::invalid(format!("a loop does not close on the surface (gap {:.2e} mm at segment {i})", gap)));
        }
        wire.push_back(p.edge(&vertices[i], to));
    }
    Ok(wire)
}

/// The wire traversed the other way: the same edges, inverted.
fn inverse(w: &Wire) -> Wire {
    w.inverse()
}

/// The ruled faces between `base` and `top`, one per edge pair, sharing the
/// rung between neighbours: a plane between two lines (the extrude's own
/// wall), a homotopy otherwise. The face normal is `tangent × up`.
fn ruled_walls(base: &Wire, top: &Wire, rungs: &mut HashMap<(usize, usize), Edge>, id_of: &dyn Fn(&Vertex) -> usize) -> Result<Vec<Face>> {
    if base.len() != top.len() {
        return Err(CadpoleError::kernel("the base and the top of a wall have different edge counts"));
    }
    let mut faces = Vec::with_capacity(base.len());
    for (e0, e1) in base.edge_iter().zip(top.edge_iter()) {
        let rung = |a: &Vertex, b: &Vertex, rungs: &mut HashMap<(usize, usize), Edge>| -> Edge {
            rungs.entry((id_of(a), id_of(b))).or_insert_with(|| builder::line(a, b)).clone()
        };
        let front = rung(e0.front(), e1.front(), rungs);
        let back = rung(e0.back(), e1.back(), rungs);
        let wire: Wire = vec![e0.clone(), back, e1.inverse(), front.inverse()].into();
        let c0 = e0.oriented_curve();
        let c1 = e1.oriented_curve();
        let surface = match (&c0, &c1) {
            (Curve::Line(l0), Curve::Line(l1)) => {
                let (p0, p1) = (l0.0, l0.1);
                let up = l1.0 - l0.0;
                Surface::Plane(truck_modeling::Plane::new(p0, p1, p0 + up))
            }
            _ => Surface::NurbsSurface(truck_modeling::NurbsSurface::new(truck_modeling::BSplineSurface::homotopy(
                c0.lift_up(),
                c1.lift_up(),
            ))),
        };
        faces.push(Face::try_new(vec![wire], surface).map_err(|e| CadpoleError::kernel(format!("wall: {e}")))?);
    }
    Ok(faces)
}

/// The offset surface a raised or cut top lies on, covering the pieces
/// with a margin: the plane offset, a cylinder of the offset radius swept
/// from an arc, a cone of the same half-angle lofted between two arcs.
fn offset_surface(w: &Wrapped, off: f64, top: &[Vec<Piece>]) -> Result<Surface> {
    match &w.map {
        Map::Plane => {
            let n = w.frame.normal;
            let o = w.frame.origin + n * off;
            let p = o + w.frame.x_dir;
            let q = o + w.frame.y_dir();
            Ok(Surface::Plane(truck_modeling::Plane::new(o.point(), p.point(), q.point())))
        }
        _ => {
            let pts: Vec<Vec3> = top.iter().flatten().flat_map(|p| p.samples(8)).collect();
            cylinder_or_cone_patch(w, off, &pts)
        }
    }
}

/// [`offset_surface`] for the quadrics: the class gives axis and radius,
/// the samples the angular and axial extent.
fn cylinder_or_cone_patch(w: &Wrapped, off: f64, pts: &[Vec3]) -> Result<Surface> {
    let n = w.frame.normal;
    let natural = w.class.normal_at(w.frame.origin).unwrap_or(n);
    let sign = if n.dot(natural) >= 0.0 { 1.0 } else { -1.0 };
    let h = sign * off;
    match &w.class {
        SurfaceClass::Cylinder { axis_point, axis, radius } => {
            let r = radius + h;
            if r <= 0.0 {
                return Err(CadpoleError::invalid(format!("a cut of {} mm reaches the axis of a cylinder of radius {}", fmt_num(off), fmt_num(*radius))));
            }
            let radial0 = unit(w.frame.origin - *axis_point - *axis * (w.frame.origin - *axis_point).dot(*axis)).unwrap_or(Vec3::X);
            let circ0 = axis.cross(radial0);
            let mut lo_t = f64::INFINITY;
            let mut hi_t = f64::NEG_INFINITY;
            let mut lo_a = f64::INFINITY;
            let mut hi_a = f64::NEG_INFINITY;
            for p in pts {
                let q = *p - *axis_point;
                let t = q.dot(*axis);
                let rad = q - *axis * t;
                let ang = fmath::atan2(rad.dot(circ0), rad.dot(radial0));
                lo_t = lo_t.min(t);
                hi_t = hi_t.max(t);
                lo_a = lo_a.min(ang);
                hi_a = hi_a.max(ang);
            }
            let margin_a = ((hi_a - lo_a) * 0.1).max(0.02);
            let (a0, a1) = (lo_a - margin_a, hi_a + margin_a);
            if a1 - a0 >= std::f64::consts::TAU * 0.97 {
                return Err(CadpoleError::invalid("the sketch wraps almost all the way round the cylinder; keep it within one face"));
            }
            let margin_t = ((hi_t - lo_t) * 0.1).max(0.1);
            let (t0, t1) = (lo_t - margin_t, hi_t + margin_t);
            let at = |ang: f64, t: f64| {
                let (s, c) = fmath::sin_cos(ang);
                (*axis_point + *axis * t + (radial0 * c + circ0 * s) * r).point()
            };
            let v0 = builder::vertex(at(a0, t0));
            let v1 = builder::vertex(at(a1, t0));
            let arc = builder::circle_arc(&v0, &v1, at((a0 + a1) / 2.0, t0));
            let face = builder::tsweep(&arc, (*axis * (t1 - t0)).vector());
            let s = face.surface();
            Ok(if sign > 0.0 { s } else { s.inverse() })
        }
        SurfaceClass::Cone { apex, axis, half_angle } => {
            let (sa, ca) = fmath::sin_cos(*half_angle);
            let apex2 = *apex - *axis * (h / sa);
            let radial0 = unit(w.frame.origin - *apex - *axis * (w.frame.origin - *apex).dot(*axis)).unwrap_or(Vec3::X);
            let circ0 = axis.cross(radial0);
            let mut lo_s = f64::INFINITY;
            let mut hi_s = f64::NEG_INFINITY;
            let mut lo_a = f64::INFINITY;
            let mut hi_a = f64::NEG_INFINITY;
            for p in pts {
                let q = *p - apex2;
                let s = q.dot(*axis) / ca;
                let rad = q - *axis * q.dot(*axis);
                let ang = fmath::atan2(rad.dot(circ0), rad.dot(radial0));
                lo_s = lo_s.min(s);
                hi_s = hi_s.max(s);
                lo_a = lo_a.min(ang);
                hi_a = hi_a.max(ang);
            }
            let margin_a = ((hi_a - lo_a) * 0.1).max(0.02);
            let (a0, a1) = (lo_a - margin_a, hi_a + margin_a);
            if a1 - a0 >= std::f64::consts::TAU * 0.97 {
                return Err(CadpoleError::invalid("the sketch wraps almost all the way round the cone; keep it within one face"));
            }
            let margin_s = ((hi_s - lo_s) * 0.1).max(0.1);
            let (s0, s1) = ((lo_s - margin_s).max(1e-3), hi_s + margin_s);
            if s0 <= 0.0 || lo_s <= 0.0 {
                return Err(CadpoleError::invalid("the relief reaches the apex of the cone"));
            }
            let at = |ang: f64, s: f64| {
                let (sn, c) = fmath::sin_cos(ang);
                (apex2 + *axis * (s * ca) + (radial0 * c + circ0 * sn) * (s * sa)).point()
            };
            let arc = |s: f64| {
                let v0 = builder::vertex(at(a0, s));
                let v1 = builder::vertex(at(a1, s));
                builder::circle_arc(&v0, &v1, at((a0 + a1) / 2.0, s))
            };
            let face = builder::homotopy(&arc(s0), &arc(s1));
            let s = face.surface();
            Ok(if sign > 0.0 { s } else { s.inverse() })
        }
        other => Err(CadpoleError::invalid(format!(
            "a raised or cut relief needs an offset surface, and a {} face has none the exact kernel can represent; \
             `paint` and `imprint` work on it, and a relief there is the field path's (not built)",
            other.kind_name()
        ))),
    }
}

/// The name of the first face of the source body the relief's new faces
/// (its walls and its top) pass through or lie on, other than the face it
/// stands on; `None` when it stays clear.
///
/// An emboss is a construction, not a boolean: the parent face gets a hole
/// and the relief is closed off by walls and a top at the offset surface.
/// That shell is closed whatever the height, so a cut deeper than the
/// material under it, or a raise across a gap into another wall, makes a
/// body that passes through itself and nothing downstream says so. Meshing
/// only the new faces and testing them against the source body's mesh
/// (which the wrap has already made) catches it at the cost of the new
/// faces alone. A relief swallowing a whole feature without touching any of
/// its faces (a void entirely inside a pocket) is not seen; every other way
/// for the relief to leave the material crosses a face.
///
/// When the probe mesh cannot be made, the check is skipped: the body's own
/// tessellation meets the same faces and refuses them by name.
fn relief_collision(source: &(crate::tessellate::Mesh, crate::tessellate::Topology), parent: usize, relief: &[Face], tol: f64) -> Option<String> {
    use crate::solid::{coplanar_overlap, crosses, pair_eps, tri, tri_normal_area};
    let (mesh, topo) = source;
    if relief.is_empty() || mesh.triangle_count() == 0 {
        return None;
    }
    let probe = KSolid::new_unchecked(vec![relief.iter().cloned().collect::<Shell>()]);
    let bounds = crate::ksurface::DivisionBounds::from_degrees(crate::ksurface::DEFAULT_MAX_NORMAL_DEGREES);
    let meshed = std::panic::catch_unwind(std::panic::AssertUnwindSafe(|| {
        crate::brepmesh::tessellate(&kernel::with_cones_revolved(&probe, bounds), tol, bounds)
    }))
    .ok()?
    .ok()?;
    let bvh = crate::render::bvh::Bvh::from_mesh(mesh);
    let eps = pair_eps(mesh);
    let p = |id: u32| kernel::point_to_vec3(meshed.positions[id as usize]);
    let mut near = Vec::new();
    for face in &meshed.faces {
        for t in &face.triangles {
            let va = [p(face.vertices[t[0] as usize].id), p(face.vertices[t[1] as usize].id), p(face.vertices[t[2] as usize].id)];
            let (na, area_a) = tri_normal_area(&va);
            if area_a == 0.0 {
                continue;
            }
            let lo = va[0].min(va[1]).min(va[2]);
            let hi = va[0].max(va[1]).max(va[2]);
            bvh.overlapping(
                [(lo.x - eps) as f32, (lo.y - eps) as f32, (lo.z - eps) as f32],
                [(hi.x + eps) as f32, (hi.y + eps) as f32, (hi.z + eps) as f32],
                &mut near,
            );
            near.sort_unstable_by_key(|&i| bvh.tri(i).tri);
            for &i in &near {
                let b = bvh.tri(i).tri as usize;
                let fb = mesh.face_ids.get(b).copied().unwrap_or(u32::MAX) as usize;
                if fb == parent {
                    continue;
                }
                let vb = tri(mesh, b);
                let (nb, area_b) = tri_normal_area(&vb);
                if area_b == 0.0 {
                    continue;
                }
                let da = [na.dot(vb[0] - va[0]), na.dot(vb[1] - va[0]), na.dot(vb[2] - va[0])];
                let db = [nb.dot(va[0] - vb[0]), nb.dot(va[1] - vb[0]), nb.dot(va[2] - vb[0])];
                let hit = if da.iter().chain(&db).all(|d| d.abs() <= eps) {
                    coplanar_overlap(&va, na, &vb, eps).is_some()
                } else {
                    crosses(&va, na, &da, &vb, nb, &db, eps).is_some()
                };
                if hit {
                    return Some(topo.faces.get(fb).map_or_else(|| format!("face {fb}"), |f| f.name.clone()));
                }
            }
        }
    }
    None
}

impl Context {
    /// Split the face of `wrapped` along its loops: the primitive.
    pub fn imprint(&mut self, wrapped: &Wrapped, name: Option<String>, span: Option<Span>) -> Result<Embossed> {
        self.relief("imprint", wrapped, Relief::Imprint, BTreeMap::new(), name, span)
    }

    /// Emboss `wrapped` on its face: `raise`, `cut`, `paint` (the imprint,
    /// for the caller to paint) or `scribe` (the outlines stroked `width`
    /// wide and cut). `draft` is in degrees, positive narrowing the relief
    /// away from the face.
    #[allow(clippy::too_many_arguments)]
    pub fn emboss(
        &mut self,
        wrapped: &Wrapped,
        kind: Kind,
        height: f64,
        draft_deg: f64,
        width: Option<f64>,
        name: Option<String>,
        span: Option<Span>,
    ) -> Result<Embossed> {
        if !height.is_finite() || height < 0.0 {
            return Err(CadpoleError::invalid(format!("emboss: height must be a non-negative number of millimetres, got {height}")));
        }
        if !draft_deg.is_finite() || draft_deg.abs() >= 90.0 {
            return Err(CadpoleError::invalid(format!("emboss: draft must be an angle between -90 and 90 degrees, got {draft_deg}")));
        }
        let mut args = BTreeMap::new();
        args.insert("kind".into(), kind.name().to_owned());
        args.insert("height".into(), fmt_num(height));
        if draft_deg != 0.0 {
            args.insert("draft".into(), fmt_num(draft_deg));
        }
        let draft = fmath::tan(draft_deg.to_radians());
        match kind {
            Kind::Paint => self.relief("emboss", wrapped, Relief::Imprint, args, name, span),
            Kind::Raise => {
                if !(height > 0.0) {
                    return Err(CadpoleError::invalid("emboss: a raise needs a positive height"));
                }
                self.relief("emboss", wrapped, Relief::Extrude { height, draft }, args, name, span)
            }
            Kind::Cut => {
                if !(height > 0.0) {
                    return Err(CadpoleError::invalid("emboss: a cut needs a positive height"));
                }
                self.relief("emboss", wrapped, Relief::Extrude { height: -height, draft }, args, name, span)
            }
            Kind::Scribe => {
                if !(height > 0.0) {
                    return Err(CadpoleError::invalid("emboss: a scribe needs a positive height (its depth)"));
                }
                let width = width.unwrap_or(2.0 * height);
                args.insert("width".into(), fmt_num(width));
                let stroked = wrapped.stroked(width, self.tolerance).map_err(|e| CadpoleError::invalid(format!("emboss: {e}")))?;
                self.relief("emboss", &stroked, Relief::Extrude { height: -height, draft }, args, name, span)
            }
        }
    }

    /// The one construction behind imprint and emboss: the face rebuilt
    /// around the loops, the body rebuilt around the face, every face named
    /// by construction, through the memo.
    fn relief(
        &mut self,
        kind: &str,
        w: &Wrapped,
        relief: Relief,
        mut args: BTreeMap<String, String>,
        name: Option<String>,
        span: Option<Span>,
    ) -> Result<Embossed> {
        let source = self.body(w.body)?;
        let mut inputs = vec![source.feature.clone()];
        if let Some(from) = &w.source {
            inputs.push(from.clone());
        }
        let material = source.material.clone();
        let layer = source.layer.clone();
        let body_name = source.name.clone();
        args.insert("body".into(), body_name.clone());
        args.insert("face".into(), w.face.clone());
        args.insert("method".into(), w.method.name().to_owned());
        args.insert("loops".into(), w.sketch.loops.len().to_string());
        if w.deviation > 0.0 {
            args.insert("deviation".into(), format!("{:.2e}", w.deviation));
        }
        let fid = self.new_feature(kind, name.clone(), inputs, args, span.clone())?;
        for note in &w.notes {
            self.info(note.clone(), &fid, span.clone());
        }
        let mut key = KeyBuilder::new(kind);
        key.put(&fid).body(&self.body(w.body)?.key).str(&w.face).str(w.method.name()).put(&w.frame).put(&w.sketch);
        match relief {
            Relief::Imprint => {
                key.tag("imprint");
            }
            Relief::Extrude { height, draft } => {
                key.tag("extrude").f64(height).f64(draft);
            }
        }
        self.routing_key(&mut key);
        let mut artwork: Vec<String> = Vec::new();
        let produced = self.memoised(kind, &fid, span.clone(), key.finish(), |ctx| {
            let (solid, names, art) = build_relief(ctx, w, relief, &fid)?;
            artwork = art;
            let generator = Arc::new(naming::Generator::Indexed { names });
            Ok(Produced::one(Body::Brep(solid), NameSource::Generator { feature: fid.clone(), generator }))
        })?;
        // A hit replays no artwork list: derive it from the names the hit's
        // generator carries. Only this feature's own: a body embossed before
        // still carries the earlier relief's faces under that feature's name.
        if artwork.is_empty()
            && let Some(NameSource::Generator { generator, .. }) = produced.bodies.first().map(|b| &b.names)
            && let naming::Generator::Indexed { names } = &**generator
        {
            let tag = format!("{}{}", fid.0, if matches!(relief, Relief::Imprint) { "/imprint[" } else { "/end[" });
            artwork = names.iter().filter(|n| n.name.starts_with(&tag)).map(|n| n.name.clone()).collect();
        }
        self.consume_body(w.body)?;
        let id = self.add_produced(&fid, produced, name)?[0];
        self.body_mut(id)?.material = material;
        self.body_mut(id)?.layer = layer;
        Ok(Embossed { body: id, feature: fid, faces: artwork })
    }
}

/// The rebuilt solid, its face names in the kernel's order, and the artwork
/// faces' names.
fn build_relief(ctx: &mut Context, w: &Wrapped, relief: Relief, fid: &FeatureId) -> Result<(KSolid, Vec<NamedFace>, Vec<String>)> {
    let what = fid.0.as_str();
    let on = face_of(ctx, w.body, &w.face, what)?;
    let solid = match &ctx.body(w.body)?.body {
        Body::Brep(s) => s.clone(),
        Body::Mesh(_) => unreachable!("face_of refused a mesh body"),
    };
    let (_, topo) = ctx.body_mesh(w.body)?;
    let existing: Vec<NamedFace> = topo
        .faces
        .iter()
        .map(|f| NamedFace { name: f.name.clone(), aliases: f.aliases.clone(), role: Role::Kept })
        .collect();

    // ---- the loops, self-unioned when they may overlap (kerned glyphs)
    let mut sketch = w.sketch.clone();
    let has_spline = sketch.loops.iter().any(|l| l.segments.iter().any(|s| matches!(s, Segment::Bezier { .. })));
    if sketch.loops.len() > 1 && !has_spline {
        let before = sketch.loops.len();
        let empty = Sketch::new(sketch.plane);
        let out = sketch2d::boolean(&sketch, what, &empty, what, sketch2d::BoolOp::Union, ctx.tolerance)?;
        if out.sketch.loops.len() != before {
            ctx.info(
                format!(
                    "{what}: the sketch's {before} loops overlap and were united into {} before the imprint, so no part of \
                     the face is claimed twice",
                    out.sketch.loops.len()
                ),
                fid,
                None,
            );
        }
        sketch = out.sketch;
    }
    if sketch.loops.is_empty() {
        return Err(CadpoleError::invalid(format!("{what}: the sketch has no closed loop to imprint")));
    }
    let w = &Wrapped { sketch: sketch.clone(), ..w.clone() };
    let loops = sketch.loops.clone();
    let (pieces, _) = w.pieces_at(&loops, 0.0)?;
    let w = &Wrapped { pieces, ..w.clone() };
    if matches!(relief, Relief::Extrude { .. })
        && !matches!(w.class, SurfaceClass::Plane { .. } | SurfaceClass::Cylinder { .. } | SurfaceClass::Cone { .. })
    {
        return Err(CadpoleError::invalid(format!(
            "{what}: a raised or cut relief needs an offset surface, and a {} face (`{}`) has none the exact kernel \
             can represent; `paint` and `imprint` work on it, and a relief there is the field path's (not built)",
            w.class.kind_name(),
            on.name
        )));
    }
    check_loops_on_face(&on, w, what)?;

    // ---- the regions: outers counter-clockwise about the normal, holes clockwise
    let regions = sketch.regions_indexed()?;
    let oriented: Vec<Loop> = {
        let mut v: Vec<Option<Loop>> = vec![None; loops.len()];
        for r in &regions {
            v[r.outer.0] = Some(r.outer.1.clone());
            for (i, h) in &r.holes {
                v[*i] = Some(h.clone());
            }
        }
        v.into_iter().enumerate().map(|(i, l)| l.unwrap_or_else(|| loops[i].clone())).collect()
    };
    // Which hole (of which region) a region's outer sits inside, for the
    // islands: a region nested two deep is a hole of an island.
    let hole_containing = |ri: usize| -> Option<usize> {
        let probe = regions[ri].outer.1.sample()[0];
        let mut best: Option<(usize, f64)> = None;
        for r in &regions {
            for (hi, h) in &r.holes {
                if h.contains_point(probe) {
                    let a = h.signed_area().abs();
                    if best.map_or(true, |(_, b)| a < b) {
                        best = Some((*hi, a));
                    }
                }
            }
        }
        best.map(|(hi, _)| hi)
    };
    let name_of = |i: usize| w.loop_name(i);

    // ---- base wires, at offset 0, oriented as the regions are
    let (base_pieces, _) = w.pieces_at(&oriented, 0.0)?;
    let base: Vec<Wire> = base_pieces
        .iter()
        .enumerate()
        .map(|(i, p)| wire_of(p).map_err(|e| CadpoleError::invalid(format!("{what}: loop {}: {e}", name_of(i)))))
        .collect::<Result<_>>()?;

    // ---- the parent: its own boundaries plus every top-level outer, inverted
    let mut parent_wires = on.face.boundaries();
    let top_level: Vec<usize> = (0..regions.len()).filter(|ri| hole_containing(*ri).is_none()).collect();
    for ri in &top_level {
        parent_wires.push(inverse(&base[regions[*ri].outer.0]));
    }
    let parent = Face::try_new(parent_wires, on.surface.clone())
        .map_err(|e| CadpoleError::kernel(format!("{what}: rebuilding `{}` with the loops as holes: {e}", on.name)))?;

    // ---- the islands: one face per hole loop, on the parent's surface
    let mut built: Vec<Built> = Vec::new();
    for r in &regions {
        for (hi, _) in &r.holes {
            let mut wires = vec![inverse(&base[*hi])];
            for ri in 0..regions.len() {
                if hole_containing(ri) == Some(*hi) {
                    wires.push(inverse(&base[regions[ri].outer.0]));
                }
            }
            let face = Face::try_new(wires, on.surface.clone())
                .map_err(|e| CadpoleError::kernel(format!("{what}: the island inside loop {}: {e}", name_of(*hi))))?;
            let local = format!("island[{}]", name_of(*hi));
            built.push(Built { face, name: NamedFace::new(format!("{what}/{local}"), Role::New).with_alias(full_name(what, "island")) });
        }
    }

    // ---- the pieces themselves, or their relief
    let mut artwork = Vec::new();
    let relief_from = built.len();
    match relief {
        Relief::Imprint => {
            for r in &regions {
                let mut wires = vec![base[r.outer.0].clone()];
                for (hi, _) in &r.holes {
                    wires.push(base[*hi].clone());
                }
                let face = Face::try_new(wires, on.surface.clone())
                    .map_err(|e| CadpoleError::kernel(format!("{what}: the piece inside loop {}: {e}", name_of(r.outer.0))))?;
                let local = format!("imprint[{}]", name_of(r.outer.0));
                let full = format!("{what}/{local}");
                artwork.push(full.clone());
                built.push(Built { face, name: NamedFace::new(full, Role::New).with_alias(full_name(what, "imprint")) });
            }
        }
        Relief::Extrude { height, draft } => {
            let inset = draft * height.abs();
            let top_loops: Vec<Loop> = if inset != 0.0 {
                oriented.iter().enumerate().map(|(i, l)| drafted(l, inset, &name_of(i))).collect::<Result<_>>()?
            } else {
                oriented.clone()
            };
            let (top_pieces, dev) = w.pieces_at(&top_loops, height)?;
            if dev > 0.0 {
                ctx.info(format!("{what}: the relief's top edges fitted within {:.2e} mm of the offset surface", dev), fid, None);
            }
            let surface = offset_surface(w, height, &top_pieces).map_err(|e| CadpoleError::invalid(format!("{what}: {e}")))?;
            let top: Vec<Wire> = top_pieces
                .iter()
                .enumerate()
                .map(|(i, p)| wire_of(p).map_err(|e| CadpoleError::invalid(format!("{what}: loop {} at the top: {e}", name_of(i)))))
                .collect::<Result<_>>()?;
            let mut rungs: HashMap<(usize, usize), Edge> = HashMap::new();
            let ids: std::cell::RefCell<HashMap<truck_modeling::VertexID, usize>> = std::cell::RefCell::new(HashMap::new());
            let id_of = |v: &Vertex| -> usize {
                let mut m = ids.borrow_mut();
                let n = m.len();
                *m.entry(v.id()).or_insert(n)
            };
            for r in &regions {
                let ri = r.outer.0;
                let mut all: Vec<usize> = vec![ri];
                all.extend(r.holes.iter().map(|(hi, _)| *hi));
                for li in all {
                    let walls = ruled_walls(&base[li], &top[li], &mut rungs, &id_of)
                        .map_err(|e| CadpoleError::kernel(format!("{what}: the walls of loop {}: {e}", name_of(li))))?;
                    for (si, face) in walls.into_iter().enumerate() {
                        let local = format!("side[{li}.{si}]");
                        built.push(Built {
                            face,
                            name: NamedFace::new(format!("{what}/{local}"), Role::New).with_alias(full_name(what, "side")),
                        });
                    }
                }
                let mut wires = vec![top[ri].clone()];
                for (hi, _) in &r.holes {
                    wires.push(top[*hi].clone());
                }
                let face = match &surface {
                    Surface::Plane(_) => builder::try_attach_plane(&wires)
                        .map_err(|e| CadpoleError::kernel(format!("{what}: the top of loop {}: {e}", name_of(ri))))?,
                    s => Face::try_new(wires, s.clone())
                        .map_err(|e| CadpoleError::kernel(format!("{what}: the top of loop {}: {e}", name_of(ri))))?,
                };
                let local = format!("end[{}]", name_of(ri));
                // The loop's own name may carry `/` (`label/glyph[3]/loop[0]`),
                // which `full_name` would read as a full name already.
                let full = format!("{what}/{local}");
                artwork.push(full.clone());
                built.push(Built { face, name: NamedFace::new(full, Role::New).with_alias(full_name(what, "end")) });
            }
        }
    }

    // ---- the relief must stay clear of the rest of the body
    if let Relief::Extrude { height, .. } = relief {
        let faces: Vec<Face> = built[relief_from..].iter().map(|b| b.face.clone()).collect();
        let tol = ctx.tolerance;
        let source = ctx.body_mesh(w.body)?;
        if let Some(hit) = relief_collision(source, on.index, &faces, tol) {
            let body_name = ctx.body(w.body)?.name.clone();
            let deep = height.abs();
            return Err(CadpoleError::invalid(if height < 0.0 {
                format!(
                    "{what}: a cut {} mm deep reaches `{hit}` of `{body_name}`: the body is not that thick under the \
                     sketch. An emboss is built on the face, not subtracted, so its floor must stay inside the material; \
                     make it shallower, or subtract a body for a cut that goes through",
                    fmt_num(deep)
                )
            } else {
                format!(
                    "{what}: a raise {} mm high runs into `{hit}` of `{body_name}`. An emboss is built on the face, not \
                     united, so the relief must stand clear of the rest of the body; make it lower, or unite a body",
                    fmt_num(deep)
                )
            }));
        }
    }

    // ---- the solid, shell by shell, the target face replaced in place and
    // the new faces appended to its shell
    let mut shells: Vec<Shell> = Vec::new();
    let mut names: Vec<NamedFace> = Vec::new();
    let mut fi = 0usize;
    for shell in solid.boundaries() {
        let mut out = Shell::new();
        let mut holds_parent = false;
        for face in shell.face_iter() {
            if fi == on.index {
                out.push(parent.clone());
                holds_parent = true;
            } else {
                out.push(face.clone());
            }
            names.push(existing.get(fi).cloned().unwrap_or_else(|| NamedFace::new(full_name(what, &format!("face[{fi}]")), Role::Unmatched)));
            fi += 1;
        }
        if holds_parent {
            for b in built.drain(..) {
                out.push(b.face);
                names.push(b.name);
            }
        }
        shells.push(out);
    }
    if !built.is_empty() {
        return Err(CadpoleError::kernel(format!("{what}: face {} of `{}` was not found in its shells", on.index, on.name)));
    }
    let solid = Solid::try_new(shells).map_err(|e| {
        CadpoleError::kernel(format!(
            "{what}: the rebuilt body is not a closed solid ({e}); the loops on `{}` must not touch each other or the face's \
             boundary",
            on.name
        ))
    })?;
    // A surface of revolution with a curved profile (a sphere, a torus, a
    // revolved arc) is where truck's tessellator inverts a loop's points
    // onto the profile's *extension* — on the same surface, outside the
    // face's parameter range, which nothing holds it to — and drops the
    // face. The geometry above is right; the mesh would have a hole. Try
    // it now, so the refusal names the face instead of a render failing
    // later. (Planes, cylinders, cones and spline patches never go through
    // that path, so they are not tried twice.)
    if matches!(on.surface, Surface::RevolutedCurve(_))
        && let Err(e) = kernel::tessellate_reporting(&solid, ctx.tolerance)
    {
        return Err(CadpoleError::invalid(format!(
            "{what}: `{}` is a {} face on a surface of revolution, which the tessellator cannot triangulate with a loop \
             cut into it ({e}); the imprint was built exactly but its mesh would have a hole, so it is refused — a plane, \
             a cylinder, a cone or a spline face takes the same sketch",
            on.name,
            w.class.kind_name(),
            e = e.to_string().trim_start_matches("kernel operation failed: ")
        )));
    }
    Ok((solid, names, artwork))
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::kernel::{BoolOp, KernelChoice};
    use crate::sketch::{Loop, Sketch, P2};
    use crate::solid;
    use std::f64::consts::PI;

    fn a_box(ctx: &mut Context, w: f64, d: f64, h: f64, name: &str) -> BodyId {
        let s = Sketch::new(Plane::XY).with_loop(Loop::rect(P2::new(0.0, 0.0), w, d));
        ctx.extrude("box", &s, None, h, BTreeMap::new(), Some(name.into()), None).unwrap()[0]
    }

    fn a_cylinder(ctx: &mut Context, r: f64, h: f64, name: &str) -> BodyId {
        let s = Sketch::new(Plane::XY).with_loop(Loop::circle(P2::new(0.0, 0.0), r));
        ctx.extrude("cylinder", &s, None, h, BTreeMap::new(), Some(name.into()), None).unwrap()[0]
    }

    fn volume(ctx: &mut Context, id: BodyId) -> f64 {
        ctx.body_mesh(id).unwrap().0.volume()
    }

    fn face_names(ctx: &mut Context, id: BodyId) -> Vec<String> {
        let mut n: Vec<String> = ctx.body_mesh(id).unwrap().1.faces.iter().map(|f| f.name.clone()).collect();
        n.sort();
        n
    }

    fn face_area(ctx: &mut Context, id: BodyId, name: &str) -> f64 {
        ctx.body_mesh(id).unwrap().1.faces.iter().find(|f| f.name == name).unwrap_or_else(|| panic!("no face {name}")).area
    }

    fn face_class(ctx: &mut Context, id: BodyId, name: &str) -> String {
        ctx.body_mesh(id).unwrap().1.faces.iter().find(|f| f.name == name).unwrap_or_else(|| panic!("no face {name}")).surface_class().to_owned()
    }

    /// The gate every result is held to: a closed, oriented, embedded
    /// surface by the validator, and watertight after a weld.
    fn assert_solid(ctx: &mut Context, id: BodyId) {
        let (mesh, _) = ctx.body_mesh(id).unwrap().clone();
        let report = solid::validate(&mesh);
        assert!(report.is_solid(), "{}", report.summary());
        assert!(mesh.welded(1e-4).is_watertight());
    }

    fn ops_run(ctx: &Context) -> Vec<String> {
        ctx.memo_counts.run_features()
    }

    fn ops_named(ctx: &Context) -> Vec<String> {
        ctx.memo_counts.runs.iter().map(|r| r.op.clone()).collect()
    }

    /// A circle imprinted on a box top: two faces on that plane with the
    /// right names and areas, the rest untouched, and the body watertight.
    #[test]
    fn a_circle_imprinted_on_a_box_top_splits_it_into_two_named_faces() {
        let mut ctx = Context::new();
        let plate = a_box(&mut ctx, 40.0, 30.0, 5.0, "plate");
        let circle = Sketch::new(Plane::XY.offset(5.0)).with_named_loop(Loop::circle(P2::new(5.0, -3.0), 6.0), "ring");
        let w = ctx.wrap_sketch(plate, "plate/top", &circle, Method::Project, None, None).unwrap();
        assert_eq!(w.class.kind_name(), "plane");
        assert_eq!(w.deviation, 0.0);
        let out = ctx.imprint(&w, Some("imp".into()), None).unwrap();
        assert_eq!(out.faces, ["imp/imprint[ring]"]);
        let names = face_names(&mut ctx, out.body);
        assert_eq!(names, ["imp/imprint[ring]", "plate/back", "plate/bottom", "plate/front", "plate/left", "plate/right", "plate/top"]);
        let disc = face_area(&mut ctx, out.body, "imp/imprint[ring]");
        let rest = face_area(&mut ctx, out.body, "plate/top");
        // Areas are measured on the tessellation: the disc's chords lose a
        // fraction of a percent of π r², and the parent's hole loses exactly
        // the same, because both are the same edges.
        assert!((disc - PI * 36.0).abs() < 0.01 * PI * 36.0, "{disc}");
        assert!((disc + rest - 1200.0).abs() < 1e-6, "{disc} + {rest}");
        assert!((volume(&mut ctx, out.body) - 6000.0).abs() < 1e-3);
        assert_solid(&mut ctx, out.body);
        assert_eq!(face_class(&mut ctx, out.body, "imp/imprint[ring]"), "plane");
        assert_eq!(ops_named(&ctx), ["extrude", "tessellate", "imprint", "tessellate"]);
    }

    /// A square raised by 2 mm on a plate has the volume of the plate plus
    /// the block — `union(plate, box)` — and no boolean ran to get it.
    #[test]
    fn a_raised_square_is_the_union_with_a_box_and_no_boolean_runs() {
        let mut ctx = Context::new();
        let plate = a_box(&mut ctx, 40.0, 30.0, 5.0, "plate");
        let sq = Sketch::new(Plane::XY.offset(5.0)).with_loop(Loop::rect(P2::new(3.0, 2.0), 10.0, 8.0));
        let w = ctx.wrap_sketch(plate, "plate/top", &sq, Method::Project, None, None).unwrap();
        let out = ctx.emboss(&w, Kind::Raise, 2.0, 0.0, None, Some("r".into()), None).unwrap();
        let v = volume(&mut ctx, out.body);
        assert!((v - (6000.0 + 160.0)).abs() <= 1e-9 * 6160.0, "{v}");
        assert_solid(&mut ctx, out.body);
        assert_eq!(out.faces, ["r/end[0]"]);
        let names = face_names(&mut ctx, out.body);
        assert_eq!(names.len(), 6 + 5);
        for n in ["r/end[0]", "r/side[0.0]", "r/side[0.1]", "r/side[0.2]", "r/side[0.3]", "plate/top"] {
            assert!(names.contains(&n.to_owned()), "{names:?}");
        }
        assert_eq!(face_class(&mut ctx, out.body, "r/end[0]"), "plane");
        assert_eq!(face_class(&mut ctx, out.body, "r/side[0.0]"), "plane");
        assert!((face_area(&mut ctx, out.body, "plate/top") - (1200.0 - 80.0)).abs() < 1e-6);
        assert!(!ops_named(&ctx).iter().any(|o| o == "union" || o == "subtract" || o == "boolean"), "{:?}", ops_named(&ctx));
        // The same union, done by the boolean engine, agrees.
        let plate2 = a_box(&mut ctx, 40.0, 30.0, 5.0, "plate2");
        let block = {
            let s = Sketch::new(Plane::XY.offset(5.0)).with_loop(Loop::rect(P2::new(3.0, 2.0), 10.0, 8.0));
            ctx.extrude("box", &s, None, 2.0, BTreeMap::new(), Some("block".into()), None).unwrap()[0]
        };
        let u = ctx.boolean(BoolOp::Union, plate2, &[block], KernelChoice::Auto, Some("u".into()), None).unwrap();
        let vu = volume(&mut ctx, u);
        assert!((v - vu).abs() <= 1e-6 * vu, "{v} vs {vu}");
        let _ = ops_run(&ctx);
    }

    /// A cut is built on the face, not subtracted, so one deeper than the
    /// material under it would hang its floor out through the far face: a
    /// closed shell that passes through itself, returned silently as a body
    /// with the wrong volume. It is refused, naming the face it reaches;
    /// so is one that ends exactly on that face (a floor of zero thickness).
    #[test]
    fn a_cut_deeper_than_the_wall_is_refused_by_the_face_it_reaches() {
        for depth in [20.0, 10.0] {
            let mut ctx = Context::new();
            let b = a_box(&mut ctx, 40.0, 30.0, 10.0, "b");
            let sq = Sketch::new(Plane::XY.offset(10.0)).with_loop(Loop::rect(P2::new(0.0, 0.0), 10.0, 6.0));
            let w = ctx.wrap_sketch(b, "b/top", &sq, Method::Project, None, None).unwrap();
            let err = ctx.emboss(&w, Kind::Cut, depth, 0.0, None, Some("thru".into()), None).unwrap_err().to_string();
            assert!(err.contains("`b/bottom`"), "depth {depth}: {err}");
        }
        // The control: a pocket that stays inside the material is built.
        let mut ctx = Context::new();
        let b = a_box(&mut ctx, 40.0, 30.0, 10.0, "b");
        let sq = Sketch::new(Plane::XY.offset(10.0)).with_loop(Loop::rect(P2::new(0.0, 0.0), 10.0, 6.0));
        let w = ctx.wrap_sketch(b, "b/top", &sq, Method::Project, None, None).unwrap();
        let out = ctx.emboss(&w, Kind::Cut, 5.0, 0.0, None, Some("ok".into()), None).unwrap();
        assert!((volume(&mut ctx, out.body) - (12000.0 - 300.0)).abs() < 1e-6);
        assert_solid(&mut ctx, out.body);
    }

    /// A raise inside a U whose height spans the gap runs into the opposite
    /// arm: the same construction, the same refusal, naming that arm's face.
    #[test]
    fn a_raise_across_a_gap_into_another_face_is_refused() {
        let mut ctx = Context::new();
        // A U open towards +y: arms x 0..5 and 15..20, base y 0..5, 20 deep.
        let u = Loop::polygon(&[
            P2::new(0.0, 0.0),
            P2::new(20.0, 0.0),
            P2::new(20.0, 20.0),
            P2::new(15.0, 20.0),
            P2::new(15.0, 5.0),
            P2::new(5.0, 5.0),
            P2::new(5.0, 20.0),
            P2::new(0.0, 20.0),
        ]);
        let b = ctx
            .extrude("u", &Sketch::new(Plane::XY).with_loop(u), None, 10.0, BTreeMap::new(), Some("u".into()), None)
            .unwrap()[0];
        // The left arm's inner face is x = 5, facing +x; its loop index in
        // the polygon is the edge (5,5) -> (5,20), segment 5.
        let inner = "u/side[0.5]";
        let topo = ctx.body_mesh(b).unwrap().1.clone();
        assert!(topo.faces.iter().any(|f| f.name == inner), "{:?}", topo.faces.iter().map(|f| &f.name).collect::<Vec<_>>());
        let sq = Sketch::new(Plane::YZ.offset(5.0)).with_loop(Loop::rect(P2::new(12.0, 5.0), 4.0, 4.0));
        let w = ctx.wrap_sketch(b, inner, &sq, Method::Project, None, None).unwrap();
        let err = ctx.emboss(&w, Kind::Raise, 12.0, 0.0, None, Some("r".into()), None).unwrap_err().to_string();
        assert!(err.contains("`u/side[0.3]`"), "{err}");
    }

    /// A cut is the subtraction, and a cut through a ring leaves the island
    /// standing at the face's level.
    #[test]
    fn a_cut_ring_removes_the_band_and_keeps_the_island() {
        let mut ctx = Context::new();
        let plate = a_box(&mut ctx, 40.0, 30.0, 5.0, "plate");
        let ring = Sketch::new(Plane::XY.offset(5.0))
            .with_named_loop(Loop::circle(P2::new(0.0, 0.0), 8.0), "o")
            .with_named_loop(Loop::circle(P2::new(0.0, 0.0), 5.0), "i");
        let w = ctx.wrap_sketch(plate, "plate/top", &ring, Method::Project, None, None).unwrap();
        let out = ctx.emboss(&w, Kind::Cut, 1.5, 0.0, None, Some("c".into()), None).unwrap();
        let v = volume(&mut ctx, out.body);
        let expect = 6000.0 - 1.5 * PI * (64.0 - 25.0);
        assert!((v - expect).abs() < 0.01 * (6000.0 - expect), "{v} vs {expect}");
        assert_solid(&mut ctx, out.body);
        let names = face_names(&mut ctx, out.body);
        for n in ["c/end[o]", "c/island[i]", "c/side[0.0]", "c/side[0.1]", "c/side[1.0]", "c/side[1.1]", "plate/top"] {
            assert!(names.contains(&n.to_owned()), "{names:?}");
        }
        assert_eq!(face_class(&mut ctx, out.body, "c/side[0.0]"), "cylinder");
        assert!((face_area(&mut ctx, out.body, "c/island[i]") - PI * 25.0).abs() < 0.01 * PI * 25.0);
        let floor = face_area(&mut ctx, out.body, "c/end[o]");
        assert!((floor - PI * (64.0 - 25.0)).abs() < 0.01 * PI * 39.0, "{floor}");
    }

    /// A drafted raise is a frustum: the volume of a truncated pyramid.
    #[test]
    fn a_drafted_raise_narrows_upward() {
        let mut ctx = Context::new();
        let plate = a_box(&mut ctx, 40.0, 30.0, 5.0, "plate");
        let sq = Sketch::new(Plane::XY.offset(5.0)).with_loop(Loop::rect(P2::new(0.0, 0.0), 10.0, 10.0));
        let w = ctx.wrap_sketch(plate, "plate/top", &sq, Method::Project, None, None).unwrap();
        let out = ctx.emboss(&w, Kind::Raise, 2.0, 45.0, None, Some("r".into()), None).unwrap();
        // top 6 x 6, bottom 10 x 10, height 2: V = h/3 (A1 + A2 + sqrt(A1 A2))
        let frustum = 2.0 / 3.0 * (100.0 + 36.0 + 60.0);
        let v = volume(&mut ctx, out.body);
        assert!((v - (6000.0 + frustum)).abs() < 1e-6, "{v}");
        assert_solid(&mut ctx, out.body);
        assert!((face_area(&mut ctx, out.body, "r/end[0]") - 36.0).abs() < 1e-9);
    }

    /// Scribing a straight path cuts a rounded-end slot of the stroke's
    /// width: the closed form of a stadium times the depth.
    #[test]
    fn a_scribed_path_is_a_stadium_groove() {
        let mut ctx = Context::new();
        let plate = a_box(&mut ctx, 40.0, 30.0, 5.0, "plate");
        let path = Path::new(P2::new(-8.0, 0.0)).line_to(P2::new(8.0, 0.0));
        let s = Sketch::new(Plane::XY.offset(5.0)).with_path(path, "line");
        let w = ctx.wrap_sketch(plate, "plate/top", &s, Method::Project, None, None).unwrap();
        let out = ctx.emboss(&w, Kind::Scribe, 0.5, 0.0, Some(1.0), Some("s".into()), None).unwrap();
        let v = volume(&mut ctx, out.body);
        let groove = 0.5 * (16.0 * 1.0 + PI * 0.25);
        // Within a percent of the groove (its round ends are tessellated);
        // a butt-capped slot would be 0.39 mm³ short.
        assert!((v - (6000.0 - groove)).abs() < 0.01 * groove, "{v} vs {}", 6000.0 - groove);
        assert_solid(&mut ctx, out.body);
    }

    /// A word unrolled onto a cylinder keeps its width as arc length, its
    /// raised tops are exact cylinders of `r + h`, and the body is solid.
    #[test]
    fn a_square_unrolled_and_raised_on_a_cylinder_is_exact() {
        let mut ctx = Context::new();
        let cyl = a_cylinder(&mut ctx, 20.0, 40.0, "cyl");
        let names = face_names(&mut ctx, cyl);
        assert_eq!(names, ["cyl/bottom", "cyl/side#0", "cyl/side#1", "cyl/top"], "{names:?}");
        // A 12 mm wide, 6 mm tall rectangle, placed at the middle of the
        // half-cylinder `side[0.0]` names (the alias of one `side#k`), up
        // along the axis.
        let sq = Sketch::new(Plane::XY).with_named_loop(Loop::rect(P2::new(0.0, 0.0), 12.0, 6.0), "k");
        let w = ctx.wrap_sketch(cyl, "cyl/side[0.0]", &sq, Method::Unroll, None, None).unwrap();
        assert_eq!(w.class.kind_name(), "cylinder");
        let curves = w.curves();
        assert_eq!(curves.len(), 1);
        // The two circumferential edges are arcs of 12 mm, the axial ones lines of 6.
        let kinds: Vec<&str> = curves[0].segments.iter().map(|s| s.kind()).collect();
        assert_eq!(kinds, ["arc", "line", "arc", "line"]);
        for s in &curves[0].segments {
            let l = s.length();
            let want = if s.kind() == "arc" { 12.0 } else { 6.0 };
            assert!((l - want).abs() < 1e-9, "{} is {l}, expected {want}", s.kind());
        }
        assert_eq!(w.deviation, 0.0);
        let out = ctx.emboss(&w, Kind::Raise, 1.5, 0.0, None, Some("r".into()), None).unwrap();
        assert_solid(&mut ctx, out.body);
        assert_eq!(face_class(&mut ctx, out.body, "r/end[k]"), "cylinder");
        let (_, topo) = ctx.body_mesh(out.body).unwrap();
        let top = topo.faces.iter().find(|f| f.name == "r/end[k]").unwrap();
        match &top.surface {
            SurfaceClass::Cylinder { radius, .. } => assert!((radius - 21.5).abs() < 1e-9, "{radius}"),
            other => panic!("{other:?}"),
        }
        // The walls: two planes (the axial edges swept radially lie in a
        // plane through the axis) and two ruled bands along the arcs.
        assert_eq!(face_class(&mut ctx, out.body, "r/side[0.1]"), "plane");
        // Volume: a sector of the annulus 20..21.5 over 12/20 rad, 6 tall.
        // The faces are exact: the top is the 12 mm × 6 mm rectangle read
        // on the r + h cylinder (12 · 21.5 / 20 wide), the axial walls are
        // 1.5 × 6 planes, the circumferential walls are ruled bands between
        // the 12 mm arc and its 12.9 mm offset, and the parent lost the
        // 12 × 6 hole. (The body's *volume* is not compared: truck's
        // Delaunay in the parent's anisotropic (u, v) fans the hole's corners
        // to the far rows over 30° chords, ~0.5 % of the cylinder; the
        // tessellator's to fix, the geometry is unaffected.)
        let theta = 12.0 / 20.0;
        let top = face_area(&mut ctx, out.body, "r/end[k]");
        assert!((top - theta * 21.5 * 6.0).abs() < 0.01 * top, "{top}");
        for i in [1, 3] {
            let wall = face_area(&mut ctx, out.body, &format!("r/side[0.{i}]"));
            assert!((wall - 9.0).abs() < 1e-6, "{wall}");
        }
        for i in [0, 2] {
            let wall = face_area(&mut ctx, out.body, &format!("r/side[0.{i}]"));
            let want = 1.5 * (12.0 + theta * 21.5) / 2.0;
            assert!((wall - want).abs() < 0.01 * want, "{wall} vs {want}");
        }
        let parent = face_area(&mut ctx, out.body, &w.face);
        let whole = face_area(&mut ctx, cyl, &w.face);
        assert!((whole - parent - 72.0).abs() < 0.1 * 72.0, "{whole} - {parent}");
    }

    /// Helices: a diagonal line unrolled is a helix in closed form, and its
    /// edge sits within FIT_TOL of the true curve.
    #[test]
    fn a_diagonal_unrolled_on_a_cylinder_is_a_helix() {
        let mut ctx = Context::new();
        let cyl = a_cylinder(&mut ctx, 20.0, 40.0, "cyl");
        let tri = Sketch::new(Plane::XY).with_loop(Loop::polygon(&[P2::new(-5.0, -4.0), P2::new(5.0, -4.0), P2::new(5.0, 4.0)]));
        let w = ctx.wrap_sketch(cyl, "cyl/side[0.0]", &tri, Method::Unroll, None, None).unwrap();
        let kinds: Vec<&str> = w.curves()[0].segments.iter().map(|s| s.kind()).collect();
        assert_eq!(kinds, ["arc", "line", "helix"]);
        assert!(w.deviation <= FIT_TOL, "{}", w.deviation);
        let out = ctx.emboss(&w, Kind::Cut, 1.0, 0.0, None, Some("c".into()), None).unwrap();
        assert_solid(&mut ctx, out.body);
        assert_eq!(face_class(&mut ctx, out.body, "c/end[0]"), "cylinder");
    }

    /// On a cone the unroll is a sector: a rectangle's horizontal edges are
    /// fitted (a developed line is not a parallel), and the raise is a cone
    /// of the same half-angle.
    #[test]
    fn a_rectangle_unrolled_and_raised_on_a_cone() {
        let mut ctx = Context::new();
        let cone = ctx.cone(Vec3::ZERO, 30.0, 10.0, 40.0, Some("cone".into()), None).unwrap();
        let names = face_names(&mut ctx, cone);
        let side = names.iter().find(|n| n.contains("side")).unwrap().clone();
        // The rectangle's left edge lies on the frame's y axis, which is the
        // generator through the origin: that edge is a line. The other
        // three are developed lines off the apex, fitted span by span.
        let sq = Sketch::new(Plane::XY).with_named_loop(Loop::rect(P2::new(4.0, 0.0), 8.0, 6.0), "k");
        let w = ctx.wrap_sketch(cone, &side, &sq, Method::Unroll, None, None).unwrap();
        assert_eq!(w.class.kind_name(), "cone");
        let curve = &w.curves()[0];
        let kinds: Vec<&str> = curve.segments.iter().map(|s| s.kind()).collect();
        assert_eq!(kinds.iter().filter(|k| **k == "line").count(), 1, "{kinds:?}");
        assert_eq!(kinds.last(), Some(&"line"));
        assert!(kinds.iter().all(|k| *k == "line" || *k == "bezier"), "{kinds:?}");
        assert!(w.deviation <= FIT_TOL);
        // The isometry keeps every length: the generator edge is 6, the
        // perimeter 28.
        let generator = curve.segments.iter().find(|s| s.kind() == "line").unwrap();
        assert!((generator.length() - 6.0).abs() < 1e-9, "{}", generator.length());
        let perimeter: f64 = curve.segments.iter().map(|s| s.length()).sum();
        assert!((perimeter - 28.0).abs() < 1e-6, "{perimeter}");
        let out = ctx.emboss(&w, Kind::Raise, 1.0, 0.0, None, Some("r".into()), None).unwrap();
        assert_solid(&mut ctx, out.body);
        assert_eq!(face_class(&mut ctx, out.body, "r/end[k]"), "cone");
        let (_, topo) = ctx.body_mesh(out.body).unwrap();
        let top = topo.faces.iter().find(|f| f.name == "r/end[k]").unwrap();
        let (SurfaceClass::Cone { half_angle: a0, .. }, SurfaceClass::Cone { half_angle: a1, .. }) = (&w.class, &top.surface) else {
            panic!()
        };
        assert!((a0 - a1).abs() < 1e-9);
    }

    /// Projection onto a cylinder: a square's vertical edges are lines, its
    /// horizontal ones circles, and the raised walls are planes.
    #[test]
    fn a_square_projected_onto_a_cylinder_has_planar_walls() {
        let mut ctx = Context::new();
        let cyl = a_cylinder(&mut ctx, 20.0, 40.0, "cyl");
        let sq = Sketch::new(Plane::XY).with_named_loop(Loop::rect(P2::new(0.0, 0.0), 10.0, 6.0), "k");
        // `Loop::circle`'s first arc runs from +x through +y to −x, so
        // `side[0.0]` is the half at y > 0; (0, 20, 20) is its middle.
        let w = ctx.wrap_sketch(cyl, "cyl/side[0.0]", &sq, Method::Project, Some(Vec3::new(0.0, 20.0, 20.0)), None).unwrap();
        let kinds: Vec<&str> = w.curves()[0].segments.iter().map(|s| s.kind()).collect();
        assert_eq!(kinds, ["arc", "line", "arc", "line"]);
        // Projected, the arc is shorter than the flat width would be
        // unrolled: the chord of 10 on a radius of 20.
        let arc = w.curves()[0].segments.iter().find(|s| s.kind() == "arc").unwrap().length();
        assert!((arc - 2.0 * 20.0 * fmath::asin(5.0 / 20.0)).abs() < 1e-9, "{arc}");
        let out = ctx.emboss(&w, Kind::Raise, 1.0, 0.0, None, Some("r".into()), None).unwrap();
        assert_solid(&mut ctx, out.body);
        assert_eq!(face_class(&mut ctx, out.body, "r/end[k]"), "cylinder");
        for i in 0..4 {
            assert_eq!(face_class(&mut ctx, out.body, &format!("r/side[0.{i}]")), "plane", "side {i}");
        }
    }

    /// What is refused, by name: a mesh body, a sphere's relief, unroll on a
    /// sphere, a loop off the face, a loop across the seam.
    #[test]
    fn refusals_name_the_face_and_the_fix() {
        let mut ctx = Context::new();
        let ball = ctx.sphere(Vec3::ZERO, 10.0, Some("ball".into()), None).unwrap();
        // The northern hemisphere; the square goes at 45° north on the +y
        // meridian — off the pole (where every meridian, and so the seam,
        // converges), off the seam meridian at +x, off the equator.
        let cap = {
            let (_, topo) = ctx.body_mesh(ball).unwrap();
            topo.faces.iter().find(|f| f.centroid.z > 0.0).unwrap().name.clone()
        };
        let sq = Sketch::new(Plane::XY).with_loop(Loop::rect(P2::new(0.0, 0.0), 3.0, 3.0));
        let e = ctx.wrap_sketch(ball, &cap, &sq, Method::Unroll, None, None).unwrap_err().to_string();
        assert!(e.contains("sphere face, which does not develop"), "{e}");
        let k = std::f64::consts::FRAC_1_SQRT_2 * 10.0;
        let w = ctx.wrap_sketch(ball, &cap, &sq, Method::Project, Some(Vec3::new(0.0, k, k)), None).unwrap();
        // Every projected edge is a plane section of the sphere: a circle.
        assert!(w.curves()[0].segments.iter().all(|s| s.kind() == "arc"));
        let e = ctx.emboss(&w, Kind::Raise, 1.0, 0.0, None, None, None).unwrap_err().to_string();
        assert!(e.contains("sphere face") && e.contains("none the exact kernel can represent"), "{e}");
        // A hemisphere closes through its pole: the imprint is refused by name.
        let e = ctx.imprint(&w, Some("i".into()), None).unwrap_err().to_string();
        assert!(e.contains("closes through a pole"), "{e}");
        // On a spherical band a revolve made — bounded on every side — the
        // imprint works, and every projected edge is a circle.
        let band = {
            let plane = Plane::new(Vec3::ZERO, -Vec3::Y, Vec3::X).unwrap();
            let profile = Loop {
                segments: vec![
                    Segment::Arc { a: P2::new(6.0, -8.0), b: P2::new(6.0, 8.0), via: P2::new(10.0, 0.0) },
                    Segment::Line { a: P2::new(6.0, 8.0), b: P2::new(6.0, -8.0) },
                ],
            };
            let s = Sketch::new(plane).with_loop(profile);
            ctx.revolve("revolve", &s, None, Vec3::ZERO, Vec3::Z, std::f64::consts::TAU, Some("band".into()), None).unwrap()[0]
        };
        let face = {
            let (_, topo) = ctx.body_mesh(band).unwrap();
            let mut spheres: Vec<_> = topo.faces.iter().filter(|f| matches!(f.surface, SurfaceClass::Sphere { .. })).collect();
            assert!(!spheres.is_empty(), "{:?}", topo.faces.iter().map(|f| (&f.name, f.surface.kind_name())).collect::<Vec<_>>());
            spheres.sort_by(|a, b| b.centroid.y.partial_cmp(&a.centroid.y).unwrap());
            spheres[0].name.clone()
        };
        let w = ctx.wrap_sketch(band, &face, &sq, Method::Project, Some(Vec3::new(0.0, 10.0, 0.0)), None).unwrap();
        assert_eq!(w.class.kind_name(), "sphere");
        assert!(w.curves()[0].segments.iter().all(|s| s.kind() == "arc"));
        // The band is bounded on every side and the imprint's solid is
        // built and closed — and meshes closed too: the edge-first
        // tessellator inverts every boundary sample through
        // `KSurface::search_parameter`, which holds a revolved surface's
        // inversions to the generator's range in the face's own
        // parameter order (an inverted `Processor` swaps `(u, v)`), so no
        // sample lands on the profile's extension, and the gate inside the
        // op passes the face.
        let out = ctx.imprint(&w, Some("ib".into()), None).unwrap();
        let (mesh, _) = ctx.body_mesh(out.body).unwrap().clone();
        let report = crate::solid::validate(&mesh);
        assert!(report.is_solid(), "the imprinted band: {}", report.summary());

        let plate = a_box(&mut ctx, 40.0, 30.0, 5.0, "plate");
        let off = Sketch::new(Plane::XY.offset(5.0)).with_loop(Loop::rect(P2::new(18.0, 0.0), 10.0, 6.0));
        let w = ctx.wrap_sketch(plate, "plate/top", &off, Method::Project, None, None).unwrap();
        let e = ctx.imprint(&w, None, None).unwrap_err().to_string();
        assert!(e.contains("crosses the boundary of `plate/top`") || e.contains("is not inside `plate/top`"), "{e}");
        let e = ctx.wrap_sketch(plate, "plate/nowhere", &off, Method::Project, None, None).unwrap_err().to_string();
        assert!(e.contains("has no face named `plate/nowhere`"), "{e}");

        let cyl = a_cylinder(&mut ctx, 20.0, 40.0, "cyl");
        let wide = Sketch::new(Plane::XY).with_loop(Loop::rect(P2::new(0.0, 0.0), 70.0, 6.0));
        let w = ctx.wrap_sketch(cyl, "cyl/side[0.0]", &wide, Method::Unroll, None, None).unwrap();
        let e = ctx.imprint(&w, None, None).unwrap_err().to_string();
        assert!(e.contains("cyl/side#"), "{e}");
    }

    /// The memo: the same emboss twice is one run and one hit, and
    /// the hit names its faces identically.
    #[test]
    fn an_emboss_is_memoised() {
        let memo = Arc::new(crate::memo::Memo::new());
        let mut names_by_run = Vec::new();
        for _ in 0..2 {
            let mut ctx = Context::new();
            ctx.use_memo(Some(memo.clone()), false);
            let plate = a_box(&mut ctx, 40.0, 30.0, 5.0, "plate");
            let sq = Sketch::new(Plane::XY.offset(5.0)).with_named_loop(Loop::rect(P2::new(3.0, 2.0), 10.0, 8.0), "q");
            let w = ctx.wrap_sketch(plate, "plate/top", &sq, Method::Project, None, None).unwrap();
            let out = ctx.emboss(&w, Kind::Raise, 2.0, 0.0, None, Some("r".into()), None).unwrap();
            assert_eq!(out.faces, ["r/end[q]"]);
            names_by_run.push((face_names(&mut ctx, out.body), ctx.memo_counts.hit_features(), ctx.memo_counts.run_features()));
        }
        assert_eq!(names_by_run[0].0, names_by_run[1].0);
        assert!(names_by_run[0].2.contains(&"r".to_owned()), "{:?}", names_by_run[0]);
        assert!(names_by_run[1].1.contains(&"r".to_owned()), "{:?}", names_by_run[1]);
    }

    #[test]
    fn the_hermite_fit_meets_its_tolerance_on_a_helix() {
        let f = |t: f64| {
            let (s, c) = fmath::sin_cos(t * 1.2);
            Some(Vec3::new(20.0 * c, 20.0 * s, 5.0 * t))
        };
        let (ctrl, dev) = fit(&f, FIT_TOL).unwrap();
        assert!(dev <= FIT_TOL);
        assert_eq!((ctrl.len() - 1) % 3, 0);
        let spans = (ctrl.len() - 1) / 3;
        // Check at points the fit did not: a fine sweep.
        for i in 0..=200 {
            let t = i as f64 / 200.0;
            let k = ((t * spans as f64) as usize).min(spans - 1);
            let u = t * spans as f64 - k as f64;
            let p = bezier_point(&ctrl[3 * k..3 * k + 4], u);
            assert!((p - f(t).unwrap()).length() <= 2.0 * FIT_TOL, "t={t}");
        }
    }

    #[test]
    fn a_ray_meets_the_quadrics_where_the_closed_form_says() {
        let cyl = SurfaceClass::Cylinder { axis_point: Vec3::ZERO, axis: Vec3::Z, radius: 10.0 };
        let t = ray_quadric(Vec3::new(10.0, 3.0, 1.0), -Vec3::X, &cyl, 0.0).unwrap();
        let hit = Vec3::new(10.0 - t, 3.0, 1.0);
        assert!((fmath::hypot(hit.x, hit.y) - 10.0).abs() < 1e-12);
        assert!(t > 0.0 && t < 1.0);
        let sph = SurfaceClass::Sphere { center: Vec3::ZERO, radius: 10.0 };
        let t = ray_quadric(Vec3::new(10.0, 3.0, 1.0), -Vec3::X, &sph, 2.0).unwrap();
        let hit = Vec3::new(10.0 - t, 3.0, 1.0);
        assert!((hit.length() - 12.0).abs() < 1e-12);
        let cone = SurfaceClass::Cone { apex: Vec3::ZERO, axis: Vec3::Z, half_angle: PI / 6.0 };
        let t = ray_quadric(Vec3::new(20.0, 0.0, 30.0), -Vec3::X, &cone, 0.0).unwrap();
        let hit = Vec3::new(20.0 - t, 0.0, 30.0);
        assert!((hit.x - 30.0 * fmath::tan(PI / 6.0)).abs() < 1e-12);
    }
}
