//! Exact chamfers on the B-rep path: a cutter per edge, oversized so nothing
//! of it is coplanar or tangent to the body, the cutters of a chain joined
//! into one tool, subtracted from convex edges and unioned into concave ones.
//!
//! **The cutter.** In the plane normal to the edge a chamfer removes the wedge
//! between the edge point `E` and the two setback points `P_A = E + d₁·a`,
//! `P_B = E + d₂·b`, where `a` and `b` are the in-face directions away from
//! the edge. The cutter's section is a larger triangle around that wedge: the
//! chamfer line `P_A P_B` extended past both setback points until it stands
//! the clearance `h` off each face, and an apex `h` off both faces on the
//! bisector — so its two legs are *parallel* to the faces at distance `h`,
//! outside the body for a convex edge and inside it for a concave one, and the
//! only cutter face that crosses the boundary is the chamfer face itself,
//! transversally (nothing coplanar, nothing tangent). The section is
//! `tsweep`ed along a straight edge, overrunning each end by `h`, or
//! `rsweep`ed about the axis of a circular one, where the chamfer face is a
//! cone. `h` is the distance `contact::extend` grows a tool by: four boolean
//! tolerances, 0.4 mm at the default.
//!
//! **Mitres, and why a chain is one tool.** Consecutive cutters overrun the
//! vertex they share. Subtracting a union is intersecting complements, so two
//! overrun cutters leave two chamfer planes meeting on the straight mitre line
//! and three leave them meeting at one point — the CAD answer with no corner
//! construction. Subtracting them one after another does not work: truck
//! refuses the second, because at a corner where a third,
//! unchamfered edge arrives, the second chamfer plane passes exactly through
//! the vertex the first cut left on that edge — and in the finished part the
//! second chamfer face and the third face genuinely meet at that one point, a
//! four-valent vertex, which a boolean whose operand already has the vertex
//! cannot make. Building the union of the chain's cutters *first* (each union
//! is a generic crossing of two prisms) and subtracting it once does work,
//! bit for bit, because the point is then interior to an edge of each
//! operand. For that union no two cutters may have coplanar legs, so the
//! runs of a chain are coloured (no two that share a vertex alike) and each
//! colour stands a different clearance off the faces. Overrunning is legal
//! only where the overrun lands in air, which is what the end probe checks;
//! a bead added to a concave edge cannot overrun at all, so a concave chain
//! is only built as a closed circular loop.
//!
//! **Stage 2.** A fillet of radius `r` on the same edge is tangent to the
//! faces along this chamfer's setback curves when `d = r·cot(γ/2)`, `γ` the
//! wedge angle between the faces ([`fillet_setback`], [`Setback::Fillet`]).
//! [`EdgeChamfer`] carries the two setback distances and the two setback
//! curves of every edge, so the fillet can be built as "topology by chamfer,
//! geometry by swap" over the boundary this chamfer
//! already has; [`Context::chamfer_plan`] hands the record over without
//! cutting anything.
//!
//! **Free-curve edges** (a cap loop's arc or spline against a flank) are
//! refused. `kernel::sweep_along_wire` is the piece that would carry
//! a section along one, and [`free_curve_cutter`] is the one function that
//! would use it; it says there what filling it in involves and why it is
//! not switched on untested.

use std::collections::BTreeMap;
use std::sync::Arc;

use truck_meshalgo::prelude::*;
use truck_modeling::Curve;

use crate::context::{fmt_num, match_tolerance, Body, NameSource, Span};
use crate::geom::{Plane, Vec3};
use crate::ids::BodyId;
use crate::kernel::{self, BoolOp, KSolid, KernelChoice};
use crate::memo::{Canon, KeyBuilder, Produced};
use crate::naming;
use crate::sketch::{Loop, Segment, Sketch, P2};
use crate::surface_class::SurfaceClass;
use crate::tessellate::{EdgeInfo, FaceInfo, Mesh, Topology};
use crate::{fmath, Context, CadpoleError, Result};

pub(crate) const TAU: f64 = 2.0 * std::f64::consts::PI;

/// Which operation is planning the edge runs: the chamfer, or the fillet
/// that borrows its planner. It decides only the words a diagnostic
/// opens with and the name of the face the operation makes.
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub(crate) enum Op {
    Chamfer,
    Fillet,
}

impl Op {
    /// The operation as a script spells it: the prefix of every diagnostic.
    pub(crate) fn name(self) -> &'static str {
        match self {
            Op::Chamfer => "chamfer",
            Op::Fillet => "fillet",
        }
    }

    /// The same, capitalised, for a sentence that starts with the fix.
    pub(crate) fn verb(self) -> &'static str {
        match self {
            Op::Chamfer => "Chamfer",
            Op::Fillet => "Fillet",
        }
    }

    /// The face the operation puts on an edge.
    pub(crate) fn face(self) -> &'static str {
        match self {
            Op::Chamfer => "chamfer face",
            Op::Fillet => "blend",
        }
    }
}

/// How far back from the edge the chamfer face meets each neighbour.
///
/// "First" and "second" face are the two faces in the order the edge's name
/// spells them (`a|b`: the sorted pair), so an asymmetric
/// chamfer is reproducible from the name alone.
#[derive(Clone, Copy, Debug, PartialEq)]
pub enum Setback {
    /// The same distance along both faces.
    Equal(f64),
    /// `d₁` along the first-named face, `d₂` along the second.
    Two(f64, f64),
    /// `d` along the first-named face; the chamfer face leaves that face at
    /// `angle` radians. The second setback follows from the law of sines in
    /// the section triangle, `d₂ = d·sin θ / sin(γ + θ)`; at a right angle
    /// that is `d·tan θ`.
    Angle { d: f64, angle: f64 },
    /// Stage 2's entry: the setback whose curves are the contact curves of a
    /// fillet of this radius, `r·cot(γ/2)` on both faces.
    Fillet { radius: f64 },
}

impl Setback {
    /// The two distances for an edge whose faces meet at the wedge angle
    /// `wedge` (radians; the interior dihedral of a convex edge, the air-side
    /// angle of a concave one — the angle the chamfer actually spans).
    pub fn per_edge(&self, wedge: f64) -> Result<[f64; 2]> {
        self.per_edge_for(Op::Chamfer, wedge)
    }

    /// [`Setback::per_edge`] with the operation named in a refusal.
    pub(crate) fn per_edge_for(&self, op: Op, wedge: f64) -> Result<[f64; 2]> {
        let op = op.name();
        let out = match *self {
            Setback::Equal(d) => [d, d],
            Setback::Two(d1, d2) => [d1, d2],
            Setback::Angle { d, angle } => {
                let s = fmath::sin(wedge + angle);
                if !(angle > 0.0) || !(s > 1e-9) || wedge + angle >= std::f64::consts::PI {
                    return Err(CadpoleError::invalid(format!(
                        "{op}: an angle of {:.1}° cannot leave a face that meets its neighbour at {:.1}°: the \
                         chamfer face would never reach the other face. The angle must be between 0 and {:.1}°",
                        angle.to_degrees(),
                        wedge.to_degrees(),
                        (std::f64::consts::PI - wedge).to_degrees()
                    )));
                }
                [d, d * fmath::sin(angle) / s]
            }
            Setback::Fillet { radius } => {
                let d = fillet_setback(radius, wedge);
                [d, d]
            }
        };
        for d in out {
            if !(d > 0.0) || !d.is_finite() {
                return Err(CadpoleError::invalid(format!("{op}: the setback must be a positive distance, got {d}")));
            }
        }
        Ok(out)
    }

    /// The call's arguments, for the feature record.
    fn describe(&self, args: &mut BTreeMap<String, String>) {
        match *self {
            Setback::Equal(d) => {
                args.insert("distance".into(), fmt_num(d));
            }
            Setback::Two(d1, d2) => {
                args.insert("distance".into(), fmt_num(d1));
                args.insert("d2".into(), fmt_num(d2));
            }
            Setback::Angle { d, angle } => {
                args.insert("distance".into(), fmt_num(d));
                // Degrees, as the script wrote them: the radians round-trip
                // is off in the last place (`29.999999999999996`).
                args.insert("angle".into(), fmt_num((angle.to_degrees() * 1e9).round() / 1e9));
            }
            Setback::Fillet { radius } => {
                args.insert("fillet_radius".into(), fmt_num(radius));
            }
        }
    }
}

/// The memo key's spelling of a setback: the form and its numbers.
impl Canon for Setback {
    fn canon(&self, k: &mut KeyBuilder) {
        match *self {
            Setback::Equal(d) => {
                k.tag("equal").f64(d);
            }
            Setback::Two(d1, d2) => {
                k.tag("two").f64(d1).f64(d2);
            }
            Setback::Angle { d, angle } => {
                k.tag("angle").f64(d).f64(angle);
            }
            Setback::Fillet { radius } => {
                k.tag("fillet").f64(radius);
            }
        }
    }
}

/// The setback at which a chamfer's setback curves are a fillet's contact
/// curves: `r·cot(γ/2)` for a wedge angle `γ` (`r` at a right angle).
pub fn fillet_setback(radius: f64, wedge: f64) -> f64 {
    // The half-angle identity `cot(γ/2) = (1 + cos γ) / sin γ` rather than
    // `1 / tan(γ/2)`: at a right angle the latter is `1 / 0.9999999999999999`
    // and the setback prints as 5.000000000000001 mm; this form gives `r`
    // exactly there, and the same number everywhere else to the bit that
    // matters.
    let (s, c) = fmath::sin_cos(wedge);
    radius * (1.0 + c) / s
}

/// Where the chamfer face meets one neighbouring face: the curve on that face
/// at the setback distance from the edge, along the whole run of the edge —
/// where two runs mitre, each is trimmed by the other's chamfer face.
#[derive(Clone, Debug, PartialEq)]
pub enum SetbackCurve {
    Line { start: Vec3, end: Vec3 },
    /// A circle about `axis` through `centre`; `sweep` is the arc's angle in
    /// radians (`2π` for a closed loop) counter-clockwise about the axis
    /// from `start`.
    Circle { centre: Vec3, axis: Vec3, radius: f64, start: Vec3, sweep: f64 },
}

/// One chamfered edge run, as stage 2 will read it back.
#[derive(Clone, Debug, PartialEq)]
pub struct EdgeChamfer {
    /// The edge's name; a run of several edges on one line or one circle
    /// (a rim is two arcs) is one chamfer under the first name, with the
    /// others as aliases of its face.
    pub edge: String,
    pub aliases: Vec<String>,
    /// The chamfer face's name on the result, `<feature>/face[<edge>]`.
    pub face: String,
    /// The two neighbouring faces, in the order the edge name spells them.
    pub faces: [String; 2],
    /// The setback along each of `faces`.
    pub setback: [f64; 2],
    /// The wedge angle between the faces, radians.
    pub wedge: f64,
    pub convex: bool,
    /// The chamfer face's boundary on each of `faces`.
    pub curves: [SetbackCurve; 2],
}

/// What [`Context::chamfer`] made.
#[derive(Clone, Debug)]
pub struct Chamfered {
    pub body: BodyId,
    pub edges: Vec<EdgeChamfer>,
}

// ----------------------------------------------------------------- geometry

/// An edge of the exact solid, in the order `tessellate` numbers them.
pub(crate) struct ExactEdge {
    pub(crate) curve: Curve,
    pub(crate) start: Vec3,
    pub(crate) end: Vec3,
}

/// The solid's edges in the order [`crate::tessellate`] walks them — faces
/// in `face_iter` order, their wires, their edges, each edge once — so index
/// `i` here is `Topology::edges[i]`.
pub(crate) fn exact_edges(solid: &KSolid) -> Vec<ExactEdge> {
    let mut seen = std::collections::HashSet::new();
    let mut out = Vec::new();
    for face in solid.face_iter() {
        for wire in face.boundaries() {
            for edge in wire.edge_iter() {
                if !seen.insert(edge.id()) {
                    continue;
                }
                out.push(ExactEdge {
                    curve: edge.oriented_curve(),
                    start: kernel::point_to_vec3(edge.front().point()),
                    end: kernel::point_to_vec3(edge.back().point()),
                });
            }
        }
    }
    out
}

/// The path of one edge: a segment, or an arc of a circle.
#[derive(Clone, Debug)]
pub(crate) enum Path {
    Line { start: Vec3, end: Vec3 },
    /// Counter-clockwise about `axis` from `start` through `sweep` radians.
    Arc { centre: Vec3, axis: Vec3, radius: f64, start: Vec3, sweep: f64 },
}

impl Path {
    pub(crate) fn start(&self) -> Vec3 {
        match self {
            Path::Line { start, .. } | Path::Arc { start, .. } => *start,
        }
    }

    pub(crate) fn end(&self) -> Vec3 {
        match *self {
            Path::Line { end, .. } => end,
            Path::Arc { centre, axis, start, sweep, .. } => rotate_about(start, centre, axis, sweep),
        }
    }

    /// The point `f` of the way along, `0..=1`.
    pub(crate) fn at(&self, f: f64) -> Vec3 {
        match *self {
            Path::Line { start, end } => start + (end - start) * f,
            Path::Arc { centre, axis, start, sweep, .. } => rotate_about(start, centre, axis, sweep * f),
        }
    }

    /// Unit tangent at `f`, in the direction of travel.
    pub(crate) fn tangent(&self, f: f64) -> Vec3 {
        match *self {
            Path::Line { start, end } => (end - start).normalized(),
            Path::Arc { centre, axis, .. } => axis.cross(self.at(f) - centre).normalized(),
        }
    }

    /// A vector attached at `at(f)`, carried to `at(g)`: unchanged along a
    /// line, turned with the arc about its axis.
    pub(crate) fn carry(&self, v: Vec3, f: f64, g: f64) -> Vec3 {
        match *self {
            Path::Line { .. } => v,
            Path::Arc { axis, sweep, .. } => rotate_about(v, Vec3::ZERO, axis, sweep * (g - f)),
        }
    }
}

/// `p` turned by `angle` radians about the axis through `centre`
/// (Rodrigues; `fmath` so native and wasm agree).
pub(crate) fn rotate_about(p: Vec3, centre: Vec3, axis: Vec3, angle: f64) -> Vec3 {
    let (s, c) = fmath::sin_cos(angle);
    let v = p - centre;
    centre + v * c + axis.cross(v) * s + axis * (axis.dot(v) * (1.0 - c))
}

pub(crate) fn parallel(u: Vec3, v: Vec3, eps: f64) -> bool {
    u.cross(v).length() <= eps
}

/// Read the edge as a segment or a circular arc.
///
/// The faces say first: two planes meet on a line, and a plane normal to the
/// axis of a cylinder or a cone meets it on a circle. A boolean's own edge is
/// an `IntersectionCurve` that evaluates by projecting onto both surfaces,
/// so the exact curve is the faces' and the edge is only checked against
/// it. Otherwise the curve itself is measured — a truck arc is a rational
/// NURBS that `curve_kind` calls a `nurbs`, and it is a circle to a
/// nanometre — the way `select::circle_fit` measures the polyline.
fn classify_path(e: &ExactEdge, fa: &SurfaceClass, fb: &SurfaceClass) -> Option<Path> {
    let (t0, t1) = e.curve.range_tuple();
    let samples: Vec<Vec3> = (0..=16).map(|k| kernel::point_to_vec3(e.curve.subs(t0 + (t1 - t0) * k as f64 / 16.0))).collect();
    let chord = (e.end - e.start).length().max((samples[8] - e.start).length());
    if chord <= 1e-9 {
        return None;
    }
    let on_line = |eps: f64| {
        let d = (e.end - e.start).normalized();
        samples.iter().all(|x| {
            let v = *x - e.start;
            (v - d * v.dot(d)).length() <= eps
        })
    };
    let arc_about = |centre: Vec3, axis: Vec3, eps: f64| -> Option<Path> {
        let radius = (e.start - centre).length();
        let on_circle = samples.iter().all(|x| ((*x - centre).length() - radius).abs() <= eps && (*x - centre).dot(axis).abs() <= eps);
        if radius <= 1e-9 || !on_circle {
            return None;
        }
        // The sweep from start to end, counter-clockwise about the axis; the
        // axis is turned over if the arc runs the other way, so `sweep > 0`.
        // Endpoints fix the circle but not which way round: the midpoint says.
        let u = (e.start - centre).normalized();
        let ccw = |w: Vec3| {
            let w = (w - centre).normalized();
            let t = fmath::atan2(u.cross(w).dot(axis), u.dot(w));
            if t <= 1e-9 { t + TAU } else { t }
        };
        let (to_end, to_mid) = (ccw(e.end), ccw(samples[8]));
        let (axis, sweep) = if to_mid > to_end { (-axis, TAU - to_end) } else { (axis, to_end) };
        Some(Path::Arc { centre, axis, radius, start: e.start, sweep })
    };
    let loose = 1e-6 * chord.max(1.0);
    match (fa, fb) {
        (SurfaceClass::Plane { .. }, SurfaceClass::Plane { .. }) => {
            if on_line(loose) {
                return Some(Path::Line { start: e.start, end: e.end });
            }
        }
        (SurfaceClass::Plane { normal, .. }, SurfaceClass::Cylinder { axis_point, axis, .. })
        | (SurfaceClass::Cylinder { axis_point, axis, .. }, SurfaceClass::Plane { normal, .. })
        | (SurfaceClass::Plane { normal, .. }, SurfaceClass::Cone { apex: axis_point, axis, .. })
        | (SurfaceClass::Cone { apex: axis_point, axis, .. }, SurfaceClass::Plane { normal, .. }) => {
            if parallel(*normal, *axis, 1e-9) {
                let centre = *axis_point + *axis * (e.start - *axis_point).dot(*axis);
                if let Some(p) = arc_about(centre, *axis, loose) {
                    return Some(p);
                }
            }
        }
        _ => {}
    }
    if matches!(e.curve, Curve::Line(_)) || on_line(1e-9 * chord.max(1.0)) {
        return Some(Path::Line { start: e.start, end: e.end });
    }
    let (centre, radius, axis) = kernel::circle_through(e.start, samples[8], e.end)?;
    arc_about(centre, axis, 1e-7 * radius.max(1.0))
}

/// Two faces' classes spell one surface: the same plane, the same axis and
/// radius, the same apex and angle. `SurfaceClass`'s `PartialEq` compares
/// the fields, and a cylinder's `axis_point` is wherever the face's samples
/// were, so `side#0` and `side#1` of one wall never compare equal by it.
pub(crate) fn same_surface(a: &SurfaceClass, b: &SurfaceClass) -> bool {
    const EPS: f64 = 1e-6;
    match (a, b) {
        (SurfaceClass::Plane { origin: o1, normal: n1 }, SurfaceClass::Plane { origin: o2, normal: n2 }) => {
            parallel(*n1, *n2, EPS) && (*o2 - *o1).dot(*n1).abs() <= EPS
        }
        (
            SurfaceClass::Cylinder { axis_point: p1, axis: a1, radius: r1 },
            SurfaceClass::Cylinder { axis_point: p2, axis: a2, radius: r2 },
        ) => parallel(*a1, *a2, EPS) && (r1 - r2).abs() <= EPS && (*p2 - *p1).cross(*a1).length() <= EPS,
        (SurfaceClass::Cone { apex: p1, axis: a1, half_angle: h1 }, SurfaceClass::Cone { apex: p2, axis: a2, half_angle: h2 }) => {
            (*p1 - *p2).length() <= EPS && (a1.dot(*a2) - 1.0).abs() <= EPS && (h1 - h2).abs() <= EPS
        }
        (SurfaceClass::Sphere { center: c1, radius: r1 }, SurfaceClass::Sphere { center: c2, radius: r2 }) => {
            (*c1 - *c2).length() <= EPS && (r1 - r2).abs() <= EPS
        }
        (
            SurfaceClass::Torus { center: c1, axis: a1, major: m1, minor: n1 },
            SurfaceClass::Torus { center: c2, axis: a2, major: m2, minor: n2 },
        ) => (*c1 - *c2).length() <= EPS && parallel(*a1, *a2, EPS) && (m1 - m2).abs() <= EPS && (n1 - n2).abs() <= EPS,
        _ => false,
    }
}

/// One edge chosen for chamfering, with everything measured on it.
#[derive(Clone)]
pub(crate) struct Picked {
    pub(crate) index: usize,
    pub(crate) name: String,
    pub(crate) path: Path,
    /// Face indices, in the order the edge name spells the faces.
    pub(crate) faces: [usize; 2],
    pub(crate) convex: bool,
}

/// The section frame at one station of a run: the in-face directions away
/// from the edge and the outward normals there.
#[derive(Clone, Copy, Debug)]
pub(crate) struct Frame {
    pub(crate) a: Vec3,
    pub(crate) b: Vec3,
    pub(crate) na: Vec3,
    pub(crate) nb: Vec3,
}

impl Frame {
    /// The wedge angle between the faces, radians.
    pub(crate) fn wedge(&self) -> f64 {
        fmath::acos(self.a.dot(self.b).clamp(-1.0, 1.0))
    }

    pub(crate) fn turned(&self, axis: Vec3, angle: f64) -> Frame {
        let r = |v: Vec3| rotate_about(v, Vec3::ZERO, axis, angle);
        Frame { a: r(self.a), b: r(self.b), na: r(self.na), nb: r(self.nb) }
    }
}

/// A run of edges on one line or one circle, chamfered by one cutter.
#[derive(Clone)]
pub(crate) struct Run {
    pub(crate) path: Path,
    /// A full circle: no ends, no overrun, no mitre.
    pub(crate) closed: bool,
    pub(crate) frame: Frame,
    /// The first member's faces, in the order its edge name spells them.
    pub(crate) faces: [usize; 2],
    /// Every member's face on each side: one surface, several faces when
    /// the wall is `side#0` and `side#1`, or a boolean split it.
    pub(crate) face_sets: [Vec<usize>; 2],
    pub(crate) convex: bool,
    pub(crate) members: Vec<usize>,
    pub(crate) setback: [f64; 2],
    /// The clearance this run's cutter stands off each of its faces by:
    /// the base clearance, stepped by the slot's colour in its chain. The
    /// two differ, and differ from every clearance and overrun of a run
    /// that shares a vertex: with equal clearances the point where one
    /// cutter's apex edge meets a neighbour's chamfer plane lies exactly on
    /// the third cutter's chamfer plane (the clearance cancels out of
    /// `x + y = corner + c`), a four-valent vertex of the union no boolean
    /// can build — measured before this was a rule.
    pub(crate) h: [f64; 2],
    /// How far the cutter runs past the run's start and end, in mm along
    /// the path. Never equal to any clearance in the chain: at a 45°
    /// corner the neighbour's chamfer plane descends one for one, so a cap
    /// `h` past the corner has its lower corner (`h` below the setback
    /// point) exactly on that plane — likewise measured.
    pub(crate) overrun: [f64; 2],
    /// For a circular run, which of `faces` is the plane (the other is the
    /// surface of revolution).
    pub(crate) plane_side: Option<usize>,
    /// Where the members join, as angles from the run's start about its
    /// axis, for a circular run: the body has a seam or a vertex there, and
    /// the ring's own seams are turned away from them ([`place_seams`]).
    pub(crate) joints: Vec<f64>,
    /// A closed ring's revolution starts this far round from the run's
    /// start, radians.
    pub(crate) seam_turn: f64,
}

/// What [`plan_runs`] made of the picked edges: the ones that are corners,
/// grouped into runs, and the smooth seams an all-edges call passed over.
pub(crate) struct Planned {
    pub(crate) picked: Vec<Picked>,
    pub(crate) runs: Vec<Run>,
    pub(crate) seams: Vec<String>,
}

/// The body's triangles, for the in/out and on-face probes.
pub(crate) struct Probe<'a> {
    pub(crate) mesh: &'a Mesh,
    pub(crate) topo: &'a Topology,
    pub(crate) soup: Vec<[Vec3; 3]>,
}

impl Probe<'_> {
    pub(crate) fn inside(&self, p: Vec3) -> bool {
        kernel::encloses(&self.soup, p)
    }

    /// How far a point exactly on a plane may read off that plane's
    /// triangles: their positions are f32, so each coordinate carries up to
    /// half an f32 ulp of rounding, which passes a micrometre once a
    /// coordinate reaches a few tens of millimetres. A fixed nanometre-scale
    /// slack refused a setback of any size on a face 76.2 mm from the origin.
    pub(crate) fn plane_slack(p: Vec3) -> f64 {
        1e-6 + 4.0 * f64::from(f32::EPSILON) * p.x.abs().max(p.y.abs()).max(p.z.abs())
    }

    /// Distance from `p` to face `fi`'s triangles.
    pub(crate) fn distance_to_face(&self, fi: usize, p: Vec3) -> f64 {
        let f = &self.topo.faces[fi];
        let mut best = f64::INFINITY;
        for t in f.tri_start..f.tri_start + f.tri_count {
            if let Some([a, b, c]) = self.mesh.triangle(t as usize) {
                best = best.min(naming::point_triangle_distance(p, a, b, c));
            }
        }
        best
    }

    /// The face nearest `p`, by its triangles.
    pub(crate) fn nearest_face(&self, p: Vec3) -> Option<&FaceInfo> {
        (0..self.topo.faces.len())
            .map(|fi| (fi, self.distance_to_face(fi, p)))
            .min_by(|x, y| x.1.partial_cmp(&y.1).unwrap_or(std::cmp::Ordering::Equal))
            .map(|(fi, _)| &self.topo.faces[fi])
    }

    /// The outward unit normal of face `fi` at `p`, from its analytic class,
    /// with the sign read off the nearest triangle: the class's own normal is
    /// "away from the axis" or the plane's parametric one, which is inward
    /// on a hole's wall and on a cap swept the other way.
    fn outward_normal(&self, fi: usize, p: Vec3) -> Option<Vec3> {
        let f = &self.topo.faces[fi];
        let n = f.surface.normal_at(p)?;
        let mut best: Option<(f64, Vec3)> = None;
        for t in f.tri_start..f.tri_start + f.tri_count {
            if let Some([a, b, c]) = self.mesh.triangle(t as usize) {
                let d = naming::point_triangle_distance(p, a, b, c);
                if best.map(|(bd, _)| d < bd).unwrap_or(true) {
                    best = Some((d, (b - a).cross(c - a)));
                }
            }
        }
        let tri = best?.1;
        Some(if tri.dot(n) < 0.0 { -n } else { n })
    }
}

/// The finest boolean tolerance a chamfer runs at. truck's intersector walks
/// tessellations chorded at the boolean tolerance, and a cutter is a
/// millimetre-scale solid beside a part that may be hundreds: at the
/// document's default 0.1 mm a chamfer plane crossing a cylinder wall came
/// back "not transversal" in every variant tried (clearance 0.2–1.2 mm,
/// overrun 0.4–2 mm), and at 0.03 mm every one of them cut cleanly, and a
/// cylinder's rim meshed at 0.05 and 0.03 where it did not at 0.1. So a
/// chamfer runs its booleans at the document's tolerance or this, whichever
/// is finer; the cutters are small, so the extra work lands on the faces
/// they touch, and the clearance ladder scales with it.
///
/// It is a measurement, not a derivation, and it does not make the curved
/// cases reliable — see the tessellation check at the end of
/// [`Context::chamfer_booleans`] for what remains and why no tolerance
/// fixes it.
pub(crate) const CHAMFER_TOLERANCE: f64 = 0.03;

/// The distance the cutter stands off the faces it must not touch, overruns
/// an end by, and a setback curve must keep from a face's far edge. The
/// same distance `contact::extend` grows a tool by, for the same reason:
/// comfortably more than the tolerance the kernel separates surfaces with,
/// and small enough that the region it adds is easy to prove empty.
pub(crate) fn clearance(boolean_tol: f64) -> f64 {
    (4.0 * boolean_tol).max(0.02)
}

/// The ladder of clearances and overruns a chain's cutters draw from: the
/// base clearance, then half as much again per colour, so any two distinct
/// colours are at least half the base — two boolean tolerances — apart. No
/// leg is then coplanar with another cutter's leg or cap, and no cap corner
/// sits on another cutter's chamfer plane ([`Run::h`], [`Run::overrun`]).
fn coloured_clearance(h: f64, colour: usize) -> f64 {
    h * (1.0 + 0.5 * colour as f64)
}

/// Angular seam threshold: below this the edge is a tessellation seam, not a
/// corner (the same 5° the field path uses).
const SEAM_DEGREES: f64 = 5.0;

fn same_direction(u: Vec3, v: Vec3, eps: f64) -> bool {
    (u - v).length() <= eps
}

/// A position in a diagnostic, to the micron: a boolean's vertex is a
/// numerical intersection, and `(16.000000000000004, …)` says nothing.
pub(crate) fn fmt_pt(v: Vec3) -> String {
    format!("({:.3}, {:.3}, {:.3})", v.x, v.y, v.z)
}

pub(crate) fn naming_list<'a>(names: impl Iterator<Item = &'a str>) -> String {
    let v: Vec<String> = names.map(|n| format!("`{n}`")).collect();
    v.join(", ")
}

// ------------------------------------------------------------------ Context

impl Context {
    /// Chamfer the named edges of `body` on the exact path. `edges`
    /// empty means every edge of the body that is a corner.
    ///
    /// The result is a B-rep body whose chamfer faces are named
    /// `<feature>/face[<edge>]`, whose neighbours keep their names, and
    /// whose new edges follow the face-pair grammar. What the exact path
    /// cannot build is an error naming the edge and the fix — the field path
    /// is never taken on its own.
    pub fn chamfer(
        &mut self,
        body: BodyId,
        edges: &[String],
        setback: Setback,
        name: Option<String>,
        span: Option<Span>,
    ) -> Result<Chamfered> {
        let source = self.body(body)?;
        let inputs = vec![source.feature.clone()];
        let material = source.material.clone();
        let layer = source.layer.clone();
        let source_names = source.names.clone();
        let source_body = source.body.clone();
        let body_name = source.name.clone();
        let solid = match &source_body {
            Body::Brep(s) => s.clone(),
            Body::Mesh(_) => {
                return Err(CadpoleError::invalid(format!(
                    "chamfer: `{body_name}` is a mesh body, which has no exact edges to chamfer; pass \
                     kernel=\"field\" to chamfer it on the field path (the result is a mesh body)"
                )));
            }
        };
        let mut args = BTreeMap::new();
        args.insert("body".into(), body_name.clone());
        setback.describe(&mut args);
        let fid = self.new_feature("chamfer", name.clone(), inputs, args, span.clone())?;

        let (mesh, topo) = self.body_mesh(body)?.clone();
        let probe = Probe {
            mesh: &mesh,
            topo: &topo,
            soup: (0..mesh.triangle_count()).filter_map(|t| mesh.triangle(t)).collect(),
        };
        let exact = exact_edges(&solid);
        let tol = self.boolean_tolerance.min(CHAMFER_TOLERANCE);
        let h = clearance(tol);
        let picked = pick_edges(Op::Chamfer, &body_name, edges, &topo, &exact)?;
        let Planned { picked, mut runs, seams } = plan_runs(Op::Chamfer, picked, &probe, &topo, setback, !edges.is_empty())?;
        check_mixed_corners(Op::Chamfer, &runs, &picked, &topo)?;
        if picked.is_empty() {
            return Err(CadpoleError::invalid(format!(
                "chamfer: `{body_name}` has no edge to chamfer{}",
                if seams.is_empty() {
                    String::new()
                } else {
                    format!(" (its edges are all smooth seams: {})", naming_list(seams.iter().map(String::as_str)))
                }
            )));
        }
        if !seams.is_empty() {
            self.info(
                format!(
                    "chamfer: skipped {} smooth seam{} that {} no corner: {}",
                    seams.len(),
                    if seams.len() == 1 { "" } else { "s" },
                    if seams.len() == 1 { "is" } else { "are" },
                    naming_list(seams.iter().map(String::as_str))
                ),
                &fid,
                span.clone(),
            );
        }
        let chains = chains_of(&mut runs, h);
        for run in &runs {
            check_run(Op::Chamfer, run, &runs, &picked, &probe, &topo, self.tolerance)?;
        }

        // ---- the cutters, named before the boolean
        let mut records = Vec::with_capacity(runs.len());
        for run in &runs {
            let names: Vec<&str> = run.members.iter().map(|&m| picked[m].name.as_str()).collect();
            records.push(EdgeChamfer {
                edge: names[0].to_owned(),
                aliases: names[1..].iter().map(|s| (*s).to_owned()).collect(),
                face: format!("{}/face[{}]", fid.0, names[0]),
                faces: [topo.faces[run.faces[0]].name.clone(), topo.faces[run.faces[1]].name.clone()],
                setback: run.setback,
                wedge: run.frame.wedge(),
                convex: run.convex,
                curves: setback_curves(run),
            });
        }
        let arg_edges: Vec<String> = records
            .iter()
            .map(|r| {
                format!(
                    "{} ({} along `{}`, {} along `{}`)",
                    r.edge,
                    fmt_num(r.setback[0]),
                    r.faces[0],
                    fmt_num(r.setback[1]),
                    r.faces[1]
                )
            })
            .collect();
        self.set_feature_arg(&fid, "edges", arg_edges.join("; "));

        // ---- one tool and one boolean per chain, on the exact kernel only,
        // at the chamfer's tolerance (`boolean_pair` reads the document's),
        // through the memo. The key: the feature the names are
        // minted under, the body, the edges as named and in the order
        // given, the setback, and the routing inputs. The planning above
        // is cheap and pure and runs on a hit too, so the records stage 2
        // reads are always in hand.
        let mut key = KeyBuilder::new("chamfer");
        key.put(&fid).body(&self.body(body)?.key);
        key.tag("edges").u64(edges.len() as u64);
        for e in edges {
            key.str(e);
        }
        key.put(&setback);
        self.routing_key(&mut key);
        let produced = self.memoised("chamfer", &fid, span.clone(), key.finish(), |ctx| {
            let saved = ctx.boolean_tolerance;
            ctx.boolean_tolerance = tol;
            let cut = ctx.chamfer_booleans(&chains, &runs, &picked, source_body, source_names, &fid, span.clone());
            ctx.boolean_tolerance = saved;
            let (acc, acc_names) = cut?;
            Ok(Produced::one(acc, acc_names))
        })?;

        self.consume_body(body)?;
        let id = self.add_produced(&fid, produced, name)?[0];
        self.body_mut(id)?.material = material;
        self.body_mut(id)?.layer = layer;
        Ok(Chamfered { body: id, edges: records })
    }

    /// The booleans of [`Context::chamfer`]: each chain's cutters unioned
    /// into one tool, then subtracted (convex) or unioned (concave).
    #[allow(clippy::too_many_arguments)]
    fn chamfer_booleans(
        &mut self,
        chains: &[Vec<usize>],
        runs: &[Run],
        picked: &[Picked],
        source_body: Body,
        source_names: NameSource,
        fid: &crate::ids::FeatureId,
        span: Option<Span>,
    ) -> Result<(Body, NameSource)> {
        let tol = self.boolean_tolerance;
        let mut acc = source_body;
        let mut acc_names = source_names;
        for chain in chains {
            let mut tool: Option<KSolid> = None;
            let mut faces: Vec<(SurfaceClass, String)> = Vec::new();
            for &k in chain {
                let run = &runs[k];
                let edge = picked[run.members[0]].name.as_str();
                let cutter = build_cutter(Op::Chamfer, run, edge)?;
                faces.extend(cutter_faces(run, &cutter, &fid.0, edge));
                tool = Some(match tool {
                    None => cutter.solid,
                    Some(t) => kernel::boolean(BoolOp::Union, &t, &cutter.solid, tol).map_err(|e| {
                        CadpoleError::kernel(format!(
                            "chamfer: the cutter for edge `{edge}` could not be joined to its chain's at the corner they \
                             share: {}. Chamfer the two in separate calls, or pass kernel=\"field\" for a mesh result",
                            e.to_string().trim_end_matches('.')
                        ))
                    })?,
                });
            }
            let tool = tool.expect("a chain has a run");
            // The chamfer faces first: a probe is named by the first surface
            // it lies on, and the legs and caps never survive anyway.
            faces.sort_by_key(|(_, n)| !n.contains("/face["));
            let generator = Arc::new(naming::Generator::Surfaces { faces, tol: match_tolerance(self.tolerance) });
            let tool_names = NameSource::Generator { feature: fid.clone(), generator };
            let convex = runs[chain[0]].convex;
            let op = if convex { BoolOp::Subtract } else { BoolOp::Union };
            let folded = self.boolean_pair(
                op,
                acc,
                &acc_names,
                &Body::Brep(tool),
                &tool_names,
                KernelChoice::Brep,
                &fid,
                span.clone(),
            );
            self.absorb_kernel_notes(&fid, span.clone());
            let (body, body_names) = folded.map_err(|e| {
                let edges: Vec<&str> = chain.iter().map(|&k| picked[runs[k].members[0]].name.as_str()).collect();
                CadpoleError::kernel(format!(
                    "chamfer: the exact kernel could not {} the cutter for {}: {}. The cutter is oversized so that only \
                     its chamfer face crosses the body; a refusal here is a contact the kernel cannot trace on this \
                     body. Pass kernel=\"field\" for a mesh result",
                    op.kind_name(),
                    if edges.len() == 1 { format!("edge `{}`", edges[0]) } else { format!("edges {}", naming_list(edges.into_iter())) },
                    e.to_string().trim_end_matches('.')
                ))
            })?;
            acc = body;
            acc_names = body_names;
        }
        // The result is a solid the tessellator can close, or this call is
        // a refusal that says so — not a body that fails to mesh two
        // features later. truck traces a curved pair's intersection
        // numerically and evaluates the curve by projecting onto both
        // surfaces; on a long circular seam the polyline can come back with
        // points out of order, and the face triangulator then drops the
        // constraints they carry, leaving a hole along that seam. Measured
        // on a cylinder's rim chamfered at 1.2 mm: clean at radius 10 and
        // 20, a hole at 30 and 44, and *not* monotone in the boolean
        // tolerance (at radius 20: a hole at 0.1, clean at 0.05 and 0.03, a
        // hole again at 0.01), so no tolerance choice avoids it and the
        // honest answer is to name it.
        if let Err(e) = acc.to_mesh(self.tolerance) {
            let names: Vec<&str> = chains
                .iter()
                .flatten()
                .map(|&k| picked[runs[k].members[0]].name.as_str())
                .collect();
            return Err(CadpoleError::kernel(format!(
                "chamfer: the cutters for {} were subtracted or unioned cleanly, but the solid that came out cannot be \
                 meshed: {}. On a circular edge the chamfer face meets its neighbours along a curve truck traces \
                 numerically, and on a long seam — a rim of about 30 mm radius or more — the polyline it hands the \
                 tessellator can double back on itself, which leaves a hole there. Chamfer a smaller radius, build \
                 this chamfer into the profile (a loft between the outline and its inset is exact), or pass \
                 kernel=\"field\" for a mesh result",
                if names.len() == 1 { format!("edge `{}`", names[0]) } else { format!("edges {}", naming_list(names.into_iter())) },
                e.to_string().trim_end_matches('.')
            )));
        }
        Ok((acc, acc_names))
    }

    /// The setback data of every edge run without cutting anything: what
    /// stage 2 asks for with `Setback::Fillet`, whose curves are the fillet's
    /// contact curves. `edges` empty means every corner, as for `chamfer`.
    pub fn chamfer_plan(&mut self, body: BodyId, edges: &[String], setback: Setback) -> Result<Vec<EdgeChamfer>> {
        let body_name = self.body(body)?.name.clone();
        let solid = self.body(body)?.solid()?.clone();
        let (mesh, topo) = self.body_mesh(body)?.clone();
        let probe = Probe { mesh: &mesh, topo: &topo, soup: Vec::new() };
        let exact = exact_edges(&solid);
        let picked = pick_edges(Op::Chamfer, &body_name, edges, &topo, &exact)?;
        let Planned { picked, runs, .. } = plan_runs(Op::Chamfer, picked, &probe, &topo, setback, !edges.is_empty())?;
        check_mixed_corners(Op::Chamfer, &runs, &picked, &topo)?;
        Ok(runs
            .iter()
            .map(|run| {
                let names: Vec<&str> = run.members.iter().map(|&m| picked[m].name.as_str()).collect();
                EdgeChamfer {
                    edge: names[0].to_owned(),
                    aliases: names[1..].iter().map(|s| (*s).to_owned()).collect(),
                    face: format!("chamfer/face[{}]", names[0]),
                    faces: [topo.faces[run.faces[0]].name.clone(), topo.faces[run.faces[1]].name.clone()],
                    setback: run.setback,
                    wedge: run.frame.wedge(),
                    convex: run.convex,
                    curves: setback_curves(run),
                }
            })
            .collect())
    }
}

/// The edges to chamfer, each read as a line or a circle.
pub(crate) fn pick_edges(op: Op, body_name: &str, edges: &[String], topo: &Topology, exact: &[ExactEdge]) -> Result<Vec<Picked>> {
    let (face, op) = (op.face(), op.name());
    let explicit = !edges.is_empty();
    let wanted: Vec<usize> = if explicit {
        let mut idx = Vec::new();
        for e in edges {
            match topo.edges.iter().position(|x| &x.name == e) {
                Some(i) => {
                    if !idx.contains(&i) {
                        idx.push(i);
                    }
                }
                None => {
                    return Err(CadpoleError::invalid(format!(
                        "{op}: `{body_name}` has no edge named `{e}`; its edges are {}",
                        naming_list(topo.edges.iter().map(|x| x.name.as_str()))
                    )));
                }
            }
        }
        idx
    } else {
        (0..topo.edges.len()).collect()
    };
    let mut picked = Vec::new();
    for i in wanted {
        let e = &topo.edges[i];
        if e.face_ids.len() != 2 {
            return Err(CadpoleError::invalid(format!(
                "{op}: edge `{}` does not bound two faces, so there is no corner to cut",
                e.name
            )));
        }
        let ex = exact.get(i).ok_or_else(|| CadpoleError::kernel(format!("{op}: the solid has fewer edges than its topology")))?;
        let ends_match = |p: Vec3, q: Vec3| (p - q).length() <= 1e-6;
        if !((ends_match(ex.start, e.start) && ends_match(ex.end, e.end))
            || (ends_match(ex.start, e.end) && ends_match(ex.end, e.start)))
        {
            return Err(CadpoleError::kernel(format!(
                "{op}: edge `{}` of the topology does not match edge {i} of the solid",
                e.name
            )));
        }
        // Faces in the order the name spells them: the sorted pair.
        let (f0, f1) = (e.face_ids[0] as usize, e.face_ids[1] as usize);
        let faces = if topo.faces[f0].name <= topo.faces[f1].name { [f0, f1] } else { [f1, f0] };
        for fi in faces {
            let f = &topo.faces[fi];
            if !f.surface.is_recognised() {
                return Err(CadpoleError::invalid(format!(
                    "{op}: edge `{}` lies on `{}`, a free-form surface ({}), so the angle between its faces \
                     varies along it and the {face} would be a ruled surface between two on-surface \
                     offsets, which the exact path does not build. Build the chamfer \
                     into the profile, or pass kernel=\"field\" for a mesh result",
                    e.name, f.name, f.surface_kind
                )));
            }
        }
        let (fa, fb) = (&topo.faces[faces[0]], &topo.faces[faces[1]]);
        let path = match classify_path(ex, &fa.surface, &fb.surface) {
            Some(p) => p,
            None => return Err(free_curve_cutter(op, e, fa, fb)),
        };
        picked.push(Picked { index: i, name: e.name.clone(), path, faces, convex: e.convex });
    }
    Ok(picked)
}

/// The adapter for edges on neither a line nor a circle — a cap loop's arc
/// or spline against a flank.
///
/// The piece it needs exists: `kernel::sweep_along_wire(profile, path,
/// sweep::Frame::Rmf)`. Filling this in is building the section as
/// a `Sketch` on the plane `sweep::end_planes(&curve, up)` gives at the
/// edge's start, extending the edge's curve by the overrun at both ends,
/// and sweeping it — the same `Section` and the same clearance ladder the
/// straight case uses. It is one function and no new rules, and it is not
/// done: an `IntersectionCurve` edge is where truck's tracing is already
/// least reliable (see the tessellation check in
/// [`Context::chamfer_booleans`]), so it wants its own measurement rather
/// than being switched on untested.
fn free_curve_cutter(op: &str, e: &EdgeInfo, fa: &FaceInfo, fb: &FaceInfo) -> CadpoleError {
    CadpoleError::invalid(format!(
        "{op}: edge `{}` is neither straight nor a circle (a {} curve between `{}` and `{}`), and the exact \
         path carries its cutter only along a line or about an axis. Build the chamfer into the profile (for a \
         whole cap loop, a loft between the outline and its inset is exact), or pass kernel=\"field\" for a mesh \
         result",
        e.name, e.curve_kind, fa.name, fb.name
    ))
}

/// The section frame at the edge's start, verified constant along it; for a
/// smooth seam, `Err` with the degrees the faces are from tangent there.
fn frame_of(op: Op, p: &Picked, probe: &Probe<'_>, topo: &Topology) -> Result<std::result::Result<Frame, f64>> {
    let (face, op) = (op.face(), op.name());
    let e = &topo.edges[p.index];
    let (fa, fb) = (&topo.faces[p.faces[0]], &topo.faces[p.faces[1]]);
    let frame_at = |f: f64| -> Result<Frame> {
        let x = p.path.at(f);
        let t = p.path.tangent(f);
        let (Some(na), Some(nb)) = (probe.outward_normal(p.faces[0], x), probe.outward_normal(p.faces[1], x)) else {
            return Err(CadpoleError::invalid(format!(
                "{op}: edge `{}` touches the axis or apex of `{}` or `{}`, where the surface has no normal",
                e.name, fa.name, fb.name
            )));
        };
        // `a` runs along face A away from the edge: perpendicular to the
        // edge, in the face, and on the side of B's normal that the material
        // is on — under B for a convex edge, over it for a concave one.
        let mut a = t.cross(na).normalized();
        let mut b = t.cross(nb).normalized();
        if a.length() < 0.5 || b.length() < 0.5 {
            return Err(CadpoleError::invalid(format!("{op}: edge `{}` runs along its own face normal", e.name)));
        }
        let want = if p.convex { -1.0 } else { 1.0 };
        if a.dot(nb) * want < 0.0 {
            a = -a;
        }
        if b.dot(na) * want < 0.0 {
            b = -b;
        }
        Ok(Frame { a, b, na, nb })
    };
    let f0 = frame_at(0.0)?;
    // Constant along the edge: the normals carried from the start agree with
    // the normals measured at the middle and the end. A circle on faces of
    // revolution about its axis passes by symmetry; a circle on anything
    // else, or a line along a twisting face, does not.
    for f in [0.5, 1.0] {
        let fx = frame_at(f)?;
        let ok = same_direction(p.path.carry(f0.na, 0.0, f), fx.na, 1e-6)
            && same_direction(p.path.carry(f0.nb, 0.0, f), fx.nb, 1e-6);
        if !ok {
            let (which, other) = match p.path {
                Path::Arc { .. } => ("a circle", "not a surface of revolution about its axis"),
                Path::Line { .. } => ("straight", "not ruled along it"),
            };
            return Err(CadpoleError::invalid(format!(
                "{op}: edge `{}` is {which}, but `{}` or `{}` is {other}, so the angle between the faces varies \
                 along the edge and the {face} is not a constant section; the exact path builds only \
                 constant sections. Build the chamfer into the profile, or pass \
                 kernel=\"field\" for a mesh result",
                e.name, fa.name, fb.name
            )));
        }
    }
    // A smooth seam — a cylinder's wall meeting its other half, a loft band
    // meeting the next — is no corner: `None`, and the caller decides
    // whether that is a refusal or a skip.
    let wedge = f0.wedge().to_degrees();
    if !(SEAM_DEGREES..=180.0 - SEAM_DEGREES).contains(&wedge) {
        return Ok(Err(wedge.min(180.0 - wedge)));
    }
    Ok(Ok(f0))
}

/// Frames for every picked edge (the seams set aside), the concavity check
/// across the chain, and the edges grouped into runs. A seam named
/// outright is a refusal; one an all-edges call swept up is skipped.
pub(crate) fn plan_runs(op: Op, picked: Vec<Picked>, probe: &Probe<'_>, topo: &Topology, setback: Setback, explicit: bool) -> Result<Planned> {
    let (verb, name) = (op.verb(), op.name());
    let mut kept: Vec<Picked> = Vec::with_capacity(picked.len());
    let mut frames: Vec<Frame> = Vec::with_capacity(picked.len());
    let mut seams = Vec::new();
    for p in picked {
        match frame_of(op, &p, probe, topo)? {
            Ok(f) => {
                frames.push(f);
                kept.push(p);
            }
            Err(degrees) => {
                let e = &topo.edges[p.index];
                if explicit {
                    return Err(CadpoleError::invalid(format!(
                        "{name}: edge `{}` is a smooth seam, not a corner: `{}` and `{}` meet at {:.1}° there (a \
                         cylinder's seam between its two halves is one), so there is nothing to {name} and the \
                         cutter's face would be tangent to both",
                        e.name,
                        topo.faces[p.faces[0]].name,
                        topo.faces[p.faces[1]].name,
                        degrees
                    )));
                }
                seams.push(e.name.clone());
            }
        }
    }
    let picked = kept;
    for (i, p) in picked.iter().enumerate() {
        for q in picked.iter().skip(i + 1) {
            if p.convex == q.convex {
                continue;
            }
            let ends = [p.path.start(), p.path.end()];
            let other = [q.path.start(), q.path.end()];
            if let Some(v) = ends.iter().find(|e| other.iter().any(|o| (**e - *o).length() <= 1e-6)) {
                let (cvx, ccv) = if p.convex { (p, q) } else { (q, p) };
                return Err(CadpoleError::invalid(format!(
                    "{name}: the chain changes concavity at {}: `{}` is convex (material is cut away) and `{}` is \
                     concave (material is added), and the two cutters cannot mitre. \
                     {verb} the two in separate calls, or pass kernel=\"field\"",
                    fmt_pt(*v),
                    cvx.name,
                    ccv.name
                )));
            }
        }
    }
    let runs = runs_of(op, &picked, &frames, topo, setback)?;
    Ok(Planned { picked, runs, seams })
}

/// A cone meeting a plane at a corner, refused by name.
///
/// The chamfer runs this on every run it plans; the fillet only on the runs
/// it cuts by chamfer, because two blends that meet *tangentially* — the
/// flat and the corner arc of a rounded outline's rim — share one section
/// arc and are re-wired without any mitre.
pub(crate) fn check_mixed_corners(op: Op, runs: &[Run], picked: &[Picked], topo: &Topology) -> Result<()> {
    let (name, faces, pair) = match op {
        Op::Chamfer => ("chamfer", "chamfer faces", "a cone and a plane"),
        Op::Fillet => ("fillet", "blends", "a torus and a cylinder"),
    };
    // A cone meeting a plane at a corner: their mitre is a curve that leaves
    // the corner on the wall, where the plane's chamfer face touches the
    // curved wall at exactly one point — the four-valent vertex of the
    // finished part. truck builds that vertex where all four faces are
    // planes (a box's corner) and refuses it where one is curved (a flat on
    // a shaft: "near miss" between the chamfer plane and the wall, 15 µm,
    // whatever the tolerance), and every construction has the same point.
    for (i, p) in runs.iter().enumerate() {
        for q in runs.iter().skip(i + 1) {
            let mixed = matches!((&p.path, &q.path), (Path::Line { .. }, Path::Arc { .. }) | (Path::Arc { .. }, Path::Line { .. }));
            if !mixed || p.closed || q.closed {
                continue;
            }
            let ends = [p.path.start(), p.path.end()];
            let other = [q.path.start(), q.path.end()];
            if let Some(v) = ends.iter().find(|e| other.iter().any(|o| (**e - *o).length() <= 1e-6)) {
                let (arc, line) = if matches!(p.path, Path::Arc { .. }) { (p, q) } else { (q, p) };
                return Err(CadpoleError::invalid(format!(
                    "{name}: `{}` (an arc) and `{}` (straight) meet at {}: their {faces}, {pair}, \
                     would mitre along a curve that leaves the corner on `{}`, where the plane touches that wall at a \
                     single point — a vertex of four faces the exact kernel builds only when all four are planes. \
                     Build this chamfer into the profile (a loft between the outline and its inset), or pass \
                     kernel=\"field\" for a mesh result",
                    picked[arc.members[0]].name,
                    picked[line.members[0]].name,
                    fmt_pt(*v),
                    topo.faces[arc.faces[1 - arc.plane_side.unwrap_or(0)]].name
                )));
            }
        }
    }
    Ok(())
}

/// Group the picked edges into runs on one line or one circle.
fn runs_of(op: Op, picked: &[Picked], frames: &[Frame], topo: &Topology, setback: Setback) -> Result<Vec<Run>> {
    let name = op.name();
    let mut used = vec![false; picked.len()];
    let mut runs = Vec::new();
    for i in 0..picked.len() {
        if used[i] {
            continue;
        }
        used[i] = true;
        let mut members = vec![i];
        match picked[i].path {
            Path::Line { start, end } => {
                let dir = (end - start).normalized();
                let on_line = |q: &Picked| -> bool {
                    let Path::Line { start: s, end: e } = q.path else { return false };
                    let v = s - start;
                    parallel((e - s).normalized(), dir, 1e-9) && (v - dir * v.dot(dir)).length() <= 1e-6
                };
                for j in 0..picked.len() {
                    if !used[j] && on_line(&picked[j]) && same_section(&frames[i], &frames[j], &picked[i], &picked[j], topo) {
                        used[j] = true;
                        members.push(j);
                    }
                }
                // The run's extent along the line, and that it is connected.
                let mut spans: Vec<(f64, f64)> = Vec::new();
                for &m in &members {
                    let Path::Line { start: s, end: e } = picked[m].path else { unreachable!() };
                    let (u, v) = ((s - start).dot(dir), (e - start).dot(dir));
                    spans.push((u.min(v), u.max(v)));
                }
                spans.sort_by(|x, y| x.0.partial_cmp(&y.0).unwrap_or(std::cmp::Ordering::Equal));
                for w in spans.windows(2) {
                    if w[1].0 > w[0].1 + 1e-6 {
                        return Err(CadpoleError::invalid(format!(
                            "{name}: `{}` and `{}` lie on one line but do not touch; {name} them in separate calls",
                            picked[members[0]].name,
                            picked[members[1]].name
                        )));
                    }
                }
                let lo = spans[0].0;
                let hi = spans.iter().map(|s| s.1).fold(f64::NEG_INFINITY, f64::max);
                let frame = frames[i];
                runs.push(Run {
                    path: Path::Line { start: start + dir * lo, end: start + dir * hi },
                    closed: false,
                    frame,
                    faces: picked[i].faces,
                    face_sets: face_sets(picked, &members),
                    convex: picked[i].convex,
                    members,
                    setback: setback.per_edge_for(op, frame.wedge())?,
                    h: [0.0; 2],
                    overrun: [0.0; 2],
                    plane_side: None,
                    joints: Vec::new(),
                    seam_turn: 0.0,
                });
            }
            Path::Arc { centre: c, axis: ax, radius: r, .. } => {
                let same_circle = |q: &Picked| -> bool {
                    let Path::Arc { centre: c2, axis: a2, radius: r2, .. } = q.path else { return false };
                    (c - c2).length() <= 1e-6 && parallel(a2, ax, 1e-9) && (r - r2).abs() <= 1e-6
                };
                for j in 0..picked.len() {
                    if !used[j] && same_circle(&picked[j]) && same_section(&frames[i], &frames[j], &picked[i], &picked[j], topo) {
                        used[j] = true;
                        members.push(j);
                    }
                }
                // Angular spans about `ax` from the first member's start,
                // each read counter-clockwise about `ax` whichever way its
                // own arc was written.
                let start0 = picked[i].path.start();
                let u0 = (start0 - c).normalized();
                let angle_of = |p: Vec3| -> f64 {
                    let v = (p - c).normalized();
                    let t = fmath::atan2(u0.cross(v).dot(ax), u0.dot(v));
                    if t < -1e-9 { t + TAU } else { t.max(0.0) }
                };
                let mut spans: Vec<(f64, f64)> = Vec::new();
                for &m in &members {
                    let Path::Arc { axis: a2, sweep, start, .. } = picked[m].path else { unreachable!() };
                    if (sweep - TAU).abs() < 1e-9 {
                        spans.push((0.0, TAU));
                        continue;
                    }
                    let (s, e) = if a2.dot(ax) > 0.0 { (start, picked[m].path.end()) } else { (picked[m].path.end(), start) };
                    let (t0, mut t1) = (angle_of(s), angle_of(e));
                    if t1 <= t0 + 1e-9 {
                        t1 += TAU;
                    }
                    spans.push((t0, t1));
                }
                spans.sort_by(|x, y| x.0.partial_cmp(&y.0).unwrap_or(std::cmp::Ordering::Equal));
                let total: f64 = spans.iter().map(|s| s.1 - s.0).sum();
                let closed = (total - TAU).abs() <= 1e-6;
                // An open run covers all but one gap of the circle, and the
                // gap may wrap through the first member's start: the run
                // begins with the span after it.
                let mut first = 0;
                if !closed {
                    let n = spans.len();
                    let mut gaps = Vec::new();
                    for k in 0..n {
                        let next = if k + 1 < n { spans[k + 1].0 } else { spans[0].0 + TAU };
                        if next - spans[k].1 > 1e-6 {
                            gaps.push((k + 1) % n);
                        }
                    }
                    if gaps.len() != 1 {
                        return Err(CadpoleError::invalid(format!(
                            "{name}: `{}` and `{}` lie on one circle but do not touch; {name} them in separate calls",
                            picked[members[0]].name,
                            picked[members[1]].name
                        )));
                    }
                    first = gaps[0];
                }
                let turn = if closed { 0.0 } else { spans[first].0 };
                let start = rotate_about(start0, c, ax, turn);
                let sweep = if closed { TAU } else { total };
                // Where the members join, from the run's start; an open
                // run's two ends are not joints, they are where it stops.
                let mut joints: Vec<f64> = Vec::new();
                for s in &spans {
                    for t in [s.0, s.1] {
                        let a = (t - turn).rem_euclid(TAU);
                        let is_end = !closed && (a <= 1e-9 || (a - sweep).abs() <= 1e-9 || a >= TAU - 1e-9);
                        if !is_end && !joints.iter().any(|j| (j - a).abs() <= 1e-9) {
                            joints.push(a);
                        }
                    }
                }
                // The frame was measured at the first member's start; carry
                // it to the run's start.
                let frame = frames[i].turned(ax, turn);
                let plane_side = if matches!(topo.faces[picked[i].faces[0]].surface, SurfaceClass::Plane { .. }) { 0 } else { 1 };
                runs.push(Run {
                    path: Path::Arc { centre: c, axis: ax, radius: r, start, sweep },
                    closed,
                    frame,
                    faces: picked[i].faces,
                    face_sets: face_sets(picked, &members),
                    convex: picked[i].convex,
                    members,
                    setback: setback.per_edge_for(op, frame.wedge())?,
                    h: [0.0; 2],
                    overrun: [0.0; 2],
                    plane_side: Some(plane_side),
                    joints,
                    seam_turn: 0.0,
                });
            }
        }
    }
    Ok(runs)
}

/// The distinct faces on each side of a run's members.
fn face_sets(picked: &[Picked], members: &[usize]) -> [Vec<usize>; 2] {
    let mut out: [Vec<usize>; 2] = [Vec::new(), Vec::new()];
    for &m in members {
        for side in 0..2 {
            let f = picked[m].faces[side];
            if !out[side].contains(&f) {
                out[side].push(f);
            }
        }
    }
    out
}

/// Two edges on one line or circle carry one section when their faces meet
/// the same way: same surfaces, same normals in the run's own frame.
fn same_section(f: &Frame, g: &Frame, p: &Picked, q: &Picked, topo: &Topology) -> bool {
    if p.convex != q.convex {
        return false;
    }
    let same_faces = |x: usize, y: usize| same_surface(&topo.faces[x].surface, &topo.faces[y].surface);
    if !(same_faces(p.faces[0], q.faces[0]) && same_faces(p.faces[1], q.faces[1])) {
        return false;
    }
    match (&p.path, &q.path) {
        (Path::Line { .. }, Path::Line { .. }) => {
            same_direction(f.a, g.a, 1e-6)
                && same_direction(f.b, g.b, 1e-6)
                && same_direction(f.na, g.na, 1e-6)
                && same_direction(f.nb, g.nb, 1e-6)
        }
        (Path::Arc { centre, axis, .. }, Path::Arc { .. }) => {
            // Rotation-invariant: components along the radial and the axis.
            let radial = |x: Vec3| (x - *centre - *axis * (x - *centre).dot(*axis)).normalized();
            let (r1, r2) = (radial(p.path.start()), radial(q.path.start()));
            let comps = |v: Vec3, r: Vec3| (v.dot(r), v.dot(*axis));
            let close = |x: (f64, f64), y: (f64, f64)| (x.0 - y.0).abs() <= 1e-6 && (x.1 - y.1).abs() <= 1e-6;
            close(comps(f.a, r1), comps(g.a, r2))
                && close(comps(f.b, r1), comps(g.b, r2))
                && close(comps(f.na, r1), comps(g.na, r2))
                && close(comps(f.nb, r1), comps(g.nb, r2))
        }
        _ => false,
    }
}

/// Runs that share an endpoint are one chain, cut by one tool; a closed
/// loop is a chain of its own. Each chain is ordered so every run after the
/// first touches one before it (the unions are then crossings, never
/// separate lumps), and coloured so no two runs that share a vertex get the
/// same clearance.
pub(crate) fn chains_of(runs: &mut [Run], h: f64) -> Vec<Vec<usize>> {
    let n = runs.len();
    let touch = |a: &Run, b: &Run| -> bool {
        if a.closed || b.closed {
            return false;
        }
        let ea = [a.path.start(), a.path.end()];
        let eb = [b.path.start(), b.path.end()];
        ea.iter().any(|p| eb.iter().any(|q| (*p - *q).length() <= 1e-6))
    };
    let mut seen = vec![false; n];
    let mut chains = Vec::new();
    for i in 0..n {
        if seen[i] {
            continue;
        }
        seen[i] = true;
        let mut chain = vec![i];
        let mut k = 0;
        while k < chain.len() {
            for j in 0..n {
                if !seen[j] && touch(&runs[chain[k]], &runs[j]) {
                    seen[j] = true;
                    chain.push(j);
                }
            }
            k += 1;
        }
        // Greedy colouring in chain order, three slots per run (its two
        // clearances and its overrun), no two alike within a run or
        // between runs that share a vertex.
        let mut colour = vec![[0usize; 3]; chain.len()];
        for (ci, &r) in chain.iter().enumerate() {
            for slot in 0..3 {
                let mut taken: Vec<usize> = colour[ci][..slot].to_vec();
                for (cj, &o) in chain[..ci].iter().enumerate() {
                    if touch(&runs[r], &runs[o]) {
                        taken.extend(colour[cj]);
                    }
                }
                let mut c = 0;
                while taken.contains(&c) {
                    c += 1;
                }
                colour[ci][slot] = c;
            }
        }
        for (ci, &r) in chain.iter().enumerate() {
            let [ca, cb, co] = colour[ci];
            runs[r].h = [coloured_clearance(h, ca), coloured_clearance(h, cb)];
            runs[r].overrun = [coloured_clearance(h, co); 2];
            place_seams(&mut runs[r]);
        }
        chains.push(chain);
    }
    chains
}

/// Turn a ring cutter's seams away from the body's.
///
/// A ring is lofted between sections whose circles are two arcs, so its
/// faces have two meridional seam edges; a seam that shares a meridional
/// plane with a joint of the run — where the wall's own two halves meet, or
/// a boolean left a vertex — crosses the body's edge there at a point, which
/// the kernel cannot trace (measured: the same ring passes turned 30° and
/// fails turned 0°). A closed ring is free to start anywhere, so it starts
/// where its seams are farthest from every joint; an open arc's sections
/// are sectors whose arcs are split at their middle, and its start is its
/// overrun, so the two overruns are stretched, one or the other, to keep
/// that middle clear.
fn place_seams(run: &mut Run) {
    let Path::Arc { radius, sweep, .. } = run.path else { return };
    let apart = |a: f64, b: f64| {
        let d = (a - b).rem_euclid(TAU);
        d.min(TAU - d)
    };
    let score = |seams: &[f64]| -> f64 {
        seams.iter().flat_map(|s| run.joints.iter().map(move |j| apart(*s, *j))).fold(f64::INFINITY, f64::min)
    };
    if run.closed {
        let mut best = (f64::NEG_INFINITY, 0.0);
        for k in 0..180 {
            let t = std::f64::consts::PI * k as f64 / 180.0;
            let s = score(&[t, t + std::f64::consts::PI]);
            if s > best.0 {
                best = (s, t);
            }
        }
        run.seam_turn = best.1;
    } else {
        let o = run.overrun[0];
        let mut best = (f64::NEG_INFINITY, [o, o]);
        for (fs, fe) in [(1.0, 1.0), (1.0, 1.25), (1.25, 1.0), (1.0, 1.5), (1.5, 1.0), (1.25, 1.5), (1.5, 1.25)] {
            let (os, oe) = (o * fs, o * fe);
            let mid = (-os / radius + sweep + oe / radius) / 2.0;
            let s = score(&[mid]);
            if s > best.0 {
                best = (s, [os, oe]);
            }
        }
        run.overrun = best.1;
    }
}

/// Everything that must hold before a cutter is built for `run`; `others`
/// are all the runs of the call, for the neighbours at its ends.
pub(crate) fn check_run(
    op: Op,
    run: &Run,
    others: &[Run],
    picked: &[Picked],
    probe: &Probe<'_>,
    topo: &Topology,
    mesh_tol: f64,
) -> Result<()> {
    let (verb, face, op) = (op.verb(), op.face(), op.name());
    let name = &picked[run.members[0]].name;
    let (fa, fb) = (&topo.faces[run.faces[0]], &topo.faces[run.faces[1]]);
    let h = run.h;
    let sect = Section::new(run);

    // A concave chain is a bead that is added, and a bead cannot overrun (it
    // would add material in air) nor stop flush (a coplanar contact): only a
    // closed circular loop has no ends.
    if !run.convex && !run.closed {
        let (where_, what) = match run.path {
            Path::Line { .. } => ("a straight concave edge", "the bead would need to end flush with the faces it runs into"),
            Path::Arc { .. } => ("an open arc of a concave edge", "the bead would need to end flush with the faces at its ends"),
        };
        return Err(CadpoleError::invalid(format!(
            "{op}: edge `{name}` is concave (the {op} adds a bead between `{}` and `{}` rather than cutting), and \
             it is {where_}: {what}, which is a coplanar contact the exact kernel refuses, and overrunning would add \
             material in air. This version builds beads only around a closed circular loop (a boss root, a blind \
             hole's floor). Build the bead into the profile, or pass kernel=\"field\" for a mesh result",
            fa.name, fb.name
        )));
    }

    // A circle that is an inner loop of its planar face — the mouth of a
    // hole in a plate, the root of a boss on one — would have its chamfer's
    // setback circle enclose that loop, and truck's boolean does not split a
    // face along a curve that encloses one of its inner loops: a counterbore
    // cut into an existing hole fails the same way, whatever the tool
    // (measured). The plane's in-face direction says which way the
    // plane extends: toward the axis for a cap or a floor, away from it for
    // a plate around a hole or a boss.
    if let (Path::Arc { centre, axis, .. }, Some(side)) = (&run.path, run.plane_side) {
        let e = run.path.start();
        let outward = (e - *centre - *axis * (e - *centre).dot(*axis)).normalized();
        let in_plane = if side == 0 { run.frame.a } else { run.frame.b };
        if in_plane.dot(outward) > 0.0 {
            let plane = &topo.faces[run.faces[side]];
            let wall = &topo.faces[run.faces[1 - side]];
            return Err(CadpoleError::invalid(format!(
                "{op}: edge `{name}` is an inner loop of `{}` — `{}` is a hole in it, or a boss standing on it — and \
                 its {} would replace that loop of `{}` by a wider circle; the exact kernel does not split a face \
                 along a curve that encloses one of its inner loops (a counterbore cut into an existing hole fails the \
                 same way). Cut the hole and its countersink, or build the boss and its bead, as one tool before the \
                 boolean (a cylinder unioned with a `cone`, as the Gridfinity baseplate does), build it into the \
                 profile, or pass kernel=\"field\" for a mesh result",
                plane.name,
                wall.name,
                if run.convex { "chamfer" } else { "bead" },
                plane.name
            )));
        }
    }

    // The setback curve must lie on its face with a clearance to spare, or
    // the chamfer would swallow the face (Braid's overrun) and its face end
    // on the far edge as a knife edge. "On the face" is exact on a plane and
    // within the chord tolerance of its triangles elsewhere.
    let stations: &[f64] = if run.closed { &[0.0, 0.25, 0.5, 0.75] } else { &[0.0, 0.25, 0.5, 0.75, 1.0] };
    // On any of the side's faces: a wall is `side#0` and `side#1`, one
    // surface, and a station falls on whichever half it falls on.
    let on_face = |side: usize, dir: Vec3, dd: f64| -> bool {
        stations.iter().all(|&f| {
            let e = run.path.at(f);
            let v = run.path.carry(dir, 0.0, f);
            run.face_sets[side].iter().any(|&fi| {
                let p = e + v * dd;
                let slack =
                    if matches!(topo.faces[fi].surface, SurfaceClass::Plane { .. }) { Probe::plane_slack(p) } else { mesh_tol + 1e-6 };
                probe.distance_to_face(fi, p) <= slack
            })
        })
    };
    for (side, dir) in [run.frame.a, run.frame.b].into_iter().enumerate() {
        let d = run.setback[side];
        if !on_face(side, dir, d + h[side]) {
            // The largest setback that does fit, by bisection.
            let (mut lo, mut hi) = (0.0, d);
            for _ in 0..30 {
                let mid = (lo + hi) / 2.0;
                if on_face(side, dir, mid + h[side]) { lo = mid } else { hi = mid }
            }
            let f = &topo.faces[run.faces[side]];
            return Err(CadpoleError::invalid(format!(
                "{op}: {} mm along `{}` at edge `{name}` is wider than the face: the setback curve leaves it, so the \
                 cutter would swallow `{}` and the {face} would end on its far edge. The largest setback that \
                 fits there is about {:.2} mm",
                fmt_num(d),
                f.name,
                f.name,
                lo
            )));
        }
    }

    // Two runs on one face, with no vertex in common (two runs that share a
    // vertex mitre there, and their strips overlap at the corner by
    // design): each setback may fit the face alone while the two together
    // take all of it or more — a face 10 mm wide between two edges each
    // given 6 mm, or a full round at exactly half the width. The face would
    // vanish into a knife edge or the two faces cross, and what the kernel
    // then says is about its cutters' union, or nothing at all; the mistake
    // is the setbacks', so say that. Measured at the run's stations: the
    // width across the face from this edge to the other one, along the
    // other run's in-face direction there.
    let ends = |r: &Run| if r.closed { vec![] } else { vec![r.path.start(), r.path.end()] };
    let mine = ends(run);
    for other in others {
        if std::ptr::eq(other, run) || ends(other).iter().any(|v| mine.iter().any(|w| (*v - *w).length() <= 1e-6)) {
            continue;
        }
        for side in 0..2 {
            for oside in 0..2 {
                let Some(&shared) = run.face_sets[side].iter().find(|f| other.face_sets[oside].contains(f)) else { continue };
                let (d, od) = (run.setback[side], other.setback[oside]);
                let odir0 = if oside == 0 { other.frame.a } else { other.frame.b };
                for &f in stations {
                    let e = run.path.at(f);
                    // The station of `other` across from `e`, and the width
                    // between the two edges there along `other`'s in-face
                    // direction; only a station on `other`'s own extent
                    // counts.
                    let Some(g) = station_of(other, e) else { continue };
                    let oe = other.path.at(g);
                    let odir = other.path.carry(odir0, 0.0, g);
                    let width = (e - oe).dot(odir);
                    // Across the face, not along it: the other edge's
                    // in-face direction must point at this edge.
                    if !(width > 0.0) || (e - oe - odir * width).length() > 1e-3 * width.max(1.0) {
                        continue;
                    }
                    if d + od >= width - 1e-6 {
                        let other_name = &picked[other.members[0]].name;
                        return Err(CadpoleError::invalid(format!(
                            "{op}: the {face}s at edges `{name}` and `{other_name}` both run along `{}`, which is {} mm \
                             apart between them at {}, and they take {} mm and {} mm of it: together that is the whole \
                             face or more, so the two {face}s would meet or cross on it and `{}` would vanish. Keep the \
                             two setbacks under {} mm together (an equal setback under {} mm), {op} one of the edges \
                             alone, or pass kernel=\"field\" for a mesh result",
                            topo.faces[shared].name,
                            fmt_num(round_mm(width)),
                            fmt_pt(e),
                            fmt_num(round_mm(d)),
                            fmt_num(round_mm(od)),
                            topo.faces[shared].name,
                            fmt_num(round_mm(width)),
                            fmt_num(round_mm(width / 2.0))
                        )));
                    }
                }
            }
        }
    }

    // The cutter must touch the body only on its chamfer face: its section's
    // three corners are `h` clear of both faces, so they must be in air
    // (convex) or in the material (concave) all along the run — and so must
    // the two points a ring's loft is cut through beyond the tip.
    let mut corners: Vec<Vec3> = sect.corners().to_vec();
    if let Some(ring) = ring_geom(run, &sect) {
        corners.push(ring.s_leg[1]);
        corners.push(ring.chamfer[1]);
    }
    // At an open run's ends a corner can sit exactly on the plane of the
    // face the edge dies into, where the winding number is a coin flip
    // (it fell "outside" on one tessellation and "inside" on the next);
    // the ends are the check below's, so the corners are probed a hair
    // inside the run.
    let corner_stations: Vec<f64> = stations.iter().map(|&f| if run.closed { f } else { f.clamp(1e-3, 1.0 - 1e-3) }).collect();
    for &f in &corner_stations {
        for (k, q) in corners.iter().enumerate() {
            let p = run.path.at(f) + run.path.carry(*q - run.path.start(), 0.0, f);
            if probe.inside(p) == run.convex {
                let near = probe.nearest_face(p).map(|x| x.name.clone()).unwrap_or_default();
                let which =
                    ["past its first face", "past its second face", "outside the corner"].get(k).copied().unwrap_or("beyond its far tip");
                return Err(CadpoleError::invalid(format!(
                    "{op}: the cutter for edge `{name}` reaches {} mm {which} (the oversize that keeps it clear of \
                     `{}` and `{}`) and meets `{near}` there at {}: a feature that close to the edge is in the \
                     cutter's way. Move it, use a smaller setback, or pass kernel=\"field\" for a mesh result",
                    fmt_num(h[0].max(h[1])),
                    fa.name,
                    fb.name,
                    fmt_pt(p)
                )));
            }
        }
    }

    // Ends: the overrun past a vertex must land in air, or inside the wedge
    // of a neighbouring run that meets the edge there — then the two cutters
    // mitre, and what the overrun takes beyond the mitre the neighbour
    // takes anyway. Otherwise the edge dies into a face: at a concave
    // vertex, or at a convex one whose third face stands obliquely across
    // the edge's line (a flat on a shaft, chamfered alone); a cutter that
    // stopped flush with that face would be a coplanar contact the exact
    // kernel refuses, and one that ran past would notch it.
    if !run.closed {
        let interior: Vec<Vec3> = {
            let g = sect.wedge_centroid();
            vec![g, (sect.e + g) * 0.5, (sect.pa + g) * 0.5, (sect.pb + g) * 0.5]
        };
        for (f, sign, overrun) in [(0.0, -1.0, run.overrun[0]), (1.0, 1.0, run.overrun[1])] {
            let e = run.path.at(f);
            let t = run.path.tangent(f) * sign;
            let neighbours: Vec<&Run> = others
                .iter()
                .filter(|o| !std::ptr::eq(*o, run) && !o.closed && [o.path.start(), o.path.end()].iter().any(|v| (*v - e).length() <= 1e-6))
                .collect();
            for step in [0.5, 1.0] {
                for q in &interior {
                    let p = e + run.path.carry(*q - run.path.start(), 0.0, f) + t * (overrun * step);
                    if probe.inside(p) && !neighbours.iter().any(|o| wedge_contains(o, p)) {
                        // The face the edge dies into: the one at that
                        // vertex that is neither of the edge's own.
                        let near = topo
                            .vertices
                            .iter()
                            .find(|v| (v.position - e).length() <= 1e-6)
                            .and_then(|v| {
                                v.face_ids.iter().map(|&i| i as usize).find(|i| !run.face_sets.iter().any(|set| set.contains(i)))
                            })
                            .map(|i| topo.faces[i].name.clone())
                            .or_else(|| probe.nearest_face(p).map(|x| x.name.clone()))
                            .unwrap_or_default();
                        return Err(CadpoleError::invalid(format!(
                            "{op}: edge `{name}` dies into `{near}` at {}: the cutter would have to stop flush with \
                             that face, which is a coplanar contact the exact kernel refuses, and running past it \
                             would notch it. {verb} the edge along `{near}` as well, so the \
                             two cutters mitre; {op} the closed loop the edge belongs to; build the step into the \
                             profile; or pass kernel=\"field\" for a mesh result",
                            fmt_pt(e)
                        )));
                    }
                }
            }
        }
    }
    Ok(())
}

/// The station of `run` nearest `p`: a fraction of the path, extrapolated
/// past the ends.
fn extrapolated_station(run: &Run, p: Vec3) -> f64 {
    match run.path {
        Path::Line { start, end } => {
            let d = end - start;
            (p - start).dot(d) / d.dot(d)
        }
        Path::Arc { centre, axis, start, sweep, .. } => {
            let u = (start - centre - axis * (start - centre).dot(axis)).normalized();
            let v = (p - centre - axis * (p - centre).dot(axis)).normalized();
            let mut t = fmath::atan2(u.cross(v).dot(axis), u.dot(v));
            if t < -0.5 * (TAU - sweep) {
                t += TAU;
            }
            t / sweep
        }
    }
}

/// The station of `run` across from `p`, when it is on the run's own
/// extent (any station of a closed loop).
fn station_of(run: &Run, p: Vec3) -> Option<f64> {
    let f = extrapolated_station(run, p);
    if run.closed {
        return Some(f.rem_euclid(1.0));
    }
    (-1e-9..=1.0 + 1e-9).contains(&f).then(|| f.clamp(0.0, 1.0))
}

/// A length to the hundredth of a millimetre, for a message.
fn round_mm(x: f64) -> f64 {
    (x * 100.0).round() / 100.0
}

/// Is `p` inside the wedge `run`'s cutter removes (or adds), taken along
/// the run's whole path and past its ends: at the station nearest `p`, the
/// triangle between the edge and its two setback points.
fn wedge_contains(run: &Run, p: Vec3) -> bool {
    let f = extrapolated_station(run, p);
    let e = run.path.at(f);
    let a = run.path.carry(run.frame.a, 0.0, f);
    let b = run.path.carry(run.frame.b, 0.0, f);
    // `p - e` in the (a, b) basis, ignoring what is along the edge.
    let w = p - e;
    let (aa, ab, bb, wa, wb) = (a.dot(a), a.dot(b), b.dot(b), w.dot(a), w.dot(b));
    let det = aa * bb - ab * ab;
    if det.abs() < 1e-12 {
        return false;
    }
    let alpha = (wa * bb - wb * ab) / det;
    let beta = (wb * aa - wa * ab) / det;
    let [d1, d2] = run.setback;
    alpha >= -1e-9 && beta >= -1e-9 && alpha / d1 + beta / d2 <= 1.0 + 1e-9
}

/// The setback curves of a run, on each face.
pub(crate) fn setback_curves(run: &Run) -> [SetbackCurve; 2] {
    let curve = |dir: Vec3, d: f64| -> SetbackCurve {
        match run.path {
            Path::Line { start, end } => SetbackCurve::Line { start: start + dir * d, end: end + dir * d },
            Path::Arc { centre, axis, radius, start, sweep } => {
                let radial = (start - centre).normalized();
                SetbackCurve::Circle {
                    centre: centre + axis * (d * dir.dot(axis)),
                    axis,
                    radius: radius + d * dir.dot(radial),
                    start: start + dir * d,
                    sweep,
                }
            }
        }
    };
    [curve(run.frame.a, run.setback[0]), curve(run.frame.b, run.setback[1])]
}

/// The cutter's section at the run's start: the wedge and the oversized
/// triangle around it, as points in space.
pub(crate) struct Section {
    pub(crate) e: Vec3,
    pub(crate) pa: Vec3,
    pub(crate) pb: Vec3,
    pub(crate) qa: Vec3,
    pub(crate) qb: Vec3,
    pub(crate) apex: Vec3,
    /// A normal of the chamfer plane (sign immaterial: surfaces are matched
    /// unsigned).
    pub(crate) chamfer_normal: Vec3,
}

impl Section {
    pub(crate) fn new(run: &Run) -> Section {
        let Frame { a, b, na, nb } = run.frame;
        let [ha, hb] = run.h;
        let e = run.path.start();
        let [d1, d2] = run.setback;
        let pa = e + a * d1;
        let pb = e + b * d2;
        let along = (pa - pb).normalized();
        // Past each setback point until the chamfer line stands that face's
        // clearance off it: outside it for a convex edge, inside for a
        // concave one — which is where the line is already heading on each
        // side.
        let qa = pa + along * (ha / along.dot(na).abs());
        let qb = pb - along * (hb / along.dot(nb).abs());
        // The apex: `ha` off face A and `hb` off face B, outside for a
        // convex edge and inside the material for a concave one, so the
        // legs through it are parallel to the faces at those distances.
        let sign = if run.convex { 1.0 } else { -1.0 };
        let g = na.dot(nb);
        let alpha = sign * (ha - g * hb) / (1.0 - g * g);
        let beta = sign * (hb - g * ha) / (1.0 - g * g);
        let apex = e + na * alpha + nb * beta;
        let chamfer_normal = along.cross(run.path.tangent(0.0)).normalized();
        Section { e, pa, pb, qa, qb, apex, chamfer_normal }
    }

    fn corners(&self) -> [Vec3; 3] {
        [self.qa, self.qb, self.apex]
    }

    fn wedge_centroid(&self) -> Vec3 {
        (self.e + self.pa + self.pb) * (1.0 / 3.0)
    }
}

/// A cutter solid, the section it was built from, and, for a ring, the
/// section quadrilateral the loft was run through.
pub(crate) struct Cutter {
    pub(crate) solid: KSolid,
    pub(crate) section: Section,
    pub(crate) ring: Option<RingGeom>,
}

/// A ring cutter's section as its loft sees it: the plane-side leg at `z1`
/// (the section's apex and `q_p` lie on it), a cut across the section's
/// wall-side tip at `z2`, and the two points of each band on those planes.
pub(crate) struct RingGeom {
    pub(crate) z1: f64,
    pub(crate) z2: f64,
    /// The wall-side leg at `z1` (the apex) and at `z2`.
    pub(crate) s_leg: [Vec3; 2],
    /// The chamfer line at `z1` (`q_p`) and at `z2`.
    pub(crate) chamfer: [Vec3; 2],
    /// An open run's sector, from the overrun before its start to the
    /// overrun past its end, radians from the run's start.
    pub(crate) sector: Option<(f64, f64)>,
}

/// A ring cutter's loft sections: the plane-side leg's level for one (the
/// apex and `q_p` lie on it), and for the other a cut across the section
/// half way between its wall-side tip `q_s` and the point `p_s` where the
/// chamfer line meets the wall. Between those two the strip from the leg to
/// the chamfer line lies entirely on the leg's side of the wall — in air
/// for a cut, in the material for a bead — so the cut's annulus touches
/// nothing. Nearer the apex the chamfer line has crossed the wall and the
/// annulus would be a ledge in the result; beyond the tip the leg and the
/// chamfer line have crossed each other and the loft's loops nest the other
/// way round. Both were measured before the rule.
pub(crate) fn ring_geom(run: &Run, s: &Section) -> Option<RingGeom> {
    let Path::Arc { centre, axis, radius, sweep, .. } = run.path else { return None };
    let z = |p: Vec3| (p - centre).dot(axis);
    let plane_side = run.plane_side.unwrap_or(0);
    let (q_p, q_s, p_s) = if plane_side == 0 { (s.qa, s.qb, s.pb) } else { (s.qb, s.qa, s.pa) };
    let z1 = z(q_p);
    let z2 = (z(q_s) + z(p_s)) / 2.0;
    let along = |from: Vec3, to: Vec3| from + (to - from) * ((z2 - z(from)) / (z(to) - z(from)));
    Some(RingGeom {
        z1,
        z2,
        s_leg: [s.apex, along(s.apex, q_s)],
        chamfer: [q_p, along(q_p, q_s)],
        sector: (!run.closed).then(|| (-run.overrun[0] / radius, sweep + run.overrun[1] / radius)),
    })
}

/// The section triangle swept along the run: `tsweep` past both ends of a
/// line; for a circle, a **loft** between two annular (or sector) sections,
/// one on the leg parallel to the plane and one cutting the section's tip
/// on the wall side, whose two bands are the exact cone and the offset of
/// the wall. truck's boolean takes the loft's surfaces; it refuses a
/// `revolve`'s `RevolutedCurve`s, a plain rectangular ring included
/// (measured).
pub(crate) fn build_cutter(op: Op, run: &Run, edge: &str) -> Result<Cutter> {
    let op = op.name();
    let [o_start, o_end] = run.overrun;
    let section = Section::new(run);
    let corners = section.corners();
    match run.path {
        Path::Line { start, end } => {
            let t = (end - start).normalized();
            let plane = Plane::new(start - t * o_start, t, run.frame.a)?;
            let to2 = |p: Vec3| {
                let v = p - plane.origin;
                P2::new(v.dot(plane.x_dir), v.dot(plane.y_dir()))
            };
            let sketch = Sketch::new(plane).with_loop(Loop::polygon(&corners.map(to2)));
            let solid = kernel::extrude(&sketch, (end - start).length() + o_start + o_end)?
                .into_iter()
                .next()
                .ok_or_else(|| CadpoleError::kernel(format!("{op}: the cutter has no solid")))?;
            Ok(Cutter { solid, section, ring: None })
        }
        Path::Arc { centre, axis, radius, start, sweep } => {
            let radial0 = (start - centre).normalized();
            let rz = |p: Vec3| ((p - centre).dot(radial0), (p - centre).dot(axis));
            for c in corners {
                if (c - centre).dot(axis.cross(radial0)).abs() > 1e-6 {
                    return Err(CadpoleError::kernel(format!("{op}: the section of a circular edge left its meridional plane")));
                }
                if rz(c).0 <= 0.0 {
                    return Err(CadpoleError::invalid(format!(
                        "{op}: the cutter for edge `{edge}` would cross the axis: the circle's radius ({} mm) is \
                         smaller than the {} mm the cutter stands clear of the faces by, so the ring cannot be built. \
                         Use a larger hole, or pass kernel=\"field\" for a mesh result",
                        fmt_num(radius),
                        fmt_num(run.h[0].max(run.h[1]))
                    )));
                }
            }
            let ring = ring_geom(run, &section).expect("a circular run");
            for c in [ring.s_leg[1], ring.chamfer[1]] {
                if rz(c).0 <= 0.0 {
                    return Err(CadpoleError::invalid(format!(
                        "{op}: the cutter for edge `{edge}` would cross the axis: the circle's radius ({} mm) is \
                         smaller than the {} mm the cutter stands clear of the faces by, so the ring cannot be built. \
                         Use a larger hole, or pass kernel=\"field\" for a mesh result",
                        fmt_num(radius),
                        fmt_num(run.h[0].max(run.h[1]))
                    )));
                }
            }
            let (z1, z2, q_p, s2, c2) = (ring.z1, ring.z2, ring.chamfer[0], ring.s_leg[1], ring.chamfer[1]);
            let _ = sweep;
            // The sections' arcs start at the seam turn of a closed ring, and
            // at the run's own start for a sector.
            let x_dir = rotate_about(radial0, Vec3::ZERO, axis, run.seam_turn);
            let section_at = |z: f64, r_s: f64, r_c: f64| -> Result<crate::loft::Section> {
                let plane = Plane::new(centre + axis * z, axis, x_dir)?;
                let sketch = match ring.sector {
                    None => Sketch::new(plane)
                        .with_loop(Loop::circle(P2::new(0.0, 0.0), r_s))
                        .with_loop(Loop::circle(P2::new(0.0, 0.0), r_c)),
                    Some((a0, a1)) => Sketch::new(plane).with_loop(sector_loop(r_s, r_c, a0, a1)),
                };
                Ok(crate::loft::Section::Profile(sketch))
            };
            let sections = [
                section_at(z1, rz(section.apex).0, rz(q_p).0)?,
                section_at(z2, rz(s2).0, rz(c2).0)?,
            ];
            let plan = crate::loft::LoftPlan::new(&sections, true, crate::loft::SectionMatch::Index)?;
            let solid = kernel::loft(&plan)?
                .into_iter()
                .next()
                .ok_or_else(|| CadpoleError::kernel(format!("{op}: the cutter has no solid")))?;
            Ok(Cutter { solid, section, ring: Some(ring) })
        }
    }
}

/// A sector between the wall-side radius `r_s` and the chamfer-side radius
/// `r_c`, from angle `a0` to `a1`, each arc split at its middle so the
/// loft's two bands per side match a full ring's two arcs: the segments are
/// the two S arcs, a radial line, the two C arcs, a radial line.
fn sector_loop(r_s: f64, r_c: f64, a0: f64, a1: f64) -> Loop {
    let at = |r: f64, a: f64| {
        let (s, c) = fmath::sin_cos(a);
        P2::new(r * c, r * s)
    };
    let (m, q1, q3) = ((a0 + a1) / 2.0, a0 + (a1 - a0) * 0.25, a0 + (a1 - a0) * 0.75);
    Loop {
        segments: vec![
            Segment::Arc { a: at(r_s, a0), b: at(r_s, m), via: at(r_s, q1) },
            Segment::Arc { a: at(r_s, m), b: at(r_s, a1), via: at(r_s, q3) },
            Segment::Line { a: at(r_s, a1), b: at(r_c, a1) },
            Segment::Arc { a: at(r_c, a1), b: at(r_c, m), via: at(r_c, q3) },
            Segment::Arc { a: at(r_c, m), b: at(r_c, a0), via: at(r_c, q1) },
            Segment::Line { a: at(r_c, a0), b: at(r_s, a0) },
        ],
    }
}

/// The analytic surface of every cutter face, with its name: the chamfer
/// face `face[<edge>]`, the legs `leg[<edge>]`, the caps `cap[<edge>]`
/// (the legs and caps never survive the boolean).
pub(crate) fn cutter_faces(run: &Run, cutter: &Cutter, feature: &str, edge: &str) -> Vec<(SurfaceClass, String)> {
    let s = &cutter.section;
    let local = |what: &str| format!("{feature}/{what}[{edge}]");
    let mut out = Vec::new();
    match run.path {
        Path::Line { start, end } => {
            let t = (end - start).normalized();
            out.push((SurfaceClass::Plane { origin: s.pa, normal: s.chamfer_normal }, local("face")));
            out.push((SurfaceClass::Plane { origin: s.qa, normal: run.frame.na }, local("leg")));
            out.push((SurfaceClass::Plane { origin: s.qb, normal: run.frame.nb }, local("leg")));
            out.push((SurfaceClass::Plane { origin: start - t * run.overrun[0], normal: t }, local("cap")));
            out.push((SurfaceClass::Plane { origin: end + t * run.overrun[1], normal: t }, local("cap")));
        }
        Path::Arc { centre, axis, start, .. } => {
            let ring = cutter.ring.as_ref().expect("a ring cutter records its loft");
            let radial = (start - centre).normalized();
            out.push((revolved_class(centre, axis, radial, ring.chamfer[0], ring.chamfer[1]), local("face")));
            out.push((revolved_class(centre, axis, radial, ring.s_leg[0], ring.s_leg[1]), local("leg")));
            out.push((SurfaceClass::Plane { origin: centre + axis * ring.z1, normal: axis }, local("leg")));
            out.push((SurfaceClass::Plane { origin: centre + axis * ring.z2, normal: axis }, local("cap")));
            if let Some((a0, a1)) = ring.sector {
                for angle in [a0, a1] {
                    let r = rotate_about(radial, Vec3::ZERO, axis, angle);
                    out.push((SurfaceClass::Plane { origin: centre, normal: axis.cross(r).normalized() }, local("cap")));
                }
            }
        }
    }
    out
}

/// The surface the segment `p q` of a section sweeps about the axis through
/// `centre` — a cylinder, a plane, or a cone — with `radial` the section's
/// own radial direction.
pub(crate) fn revolved_class(centre: Vec3, axis: Vec3, radial: Vec3, p: Vec3, q: Vec3) -> SurfaceClass {
    let rz = |x: Vec3| ((x - centre).dot(radial), (x - centre).dot(axis));
    let ((r0, z0), (r1, z1)) = (rz(p), rz(q));
    if (r1 - r0).abs() <= 1e-9 {
        SurfaceClass::Cylinder { axis_point: centre, axis, radius: r0.abs() }
    } else if (z1 - z0).abs() <= 1e-9 {
        SurfaceClass::Plane { origin: centre + axis * z0, normal: axis }
    } else {
        let z_apex = z0 - r0 * (z1 - z0) / (r1 - r0);
        let far = if (z0 - z_apex).abs() > (z1 - z_apex).abs() { z0 } else { z1 };
        SurfaceClass::Cone {
            apex: centre + axis * z_apex,
            axis: axis * (far - z_apex).signum(),
            half_angle: fmath::atan2((r1 - r0).abs(), (z1 - z0).abs()),
        }
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::sketch::{Loop, Sketch, P2};
    use crate::solid;
    use std::f64::consts::PI;

    fn a_box(ctx: &mut Context, w: f64, d: f64, h: f64, name: &str) -> BodyId {
        let s = Sketch::new(Plane::XY).with_loop(Loop::rect(P2::new(0.0, 0.0), w, d));
        ctx.extrude("box", &s, None, h, BTreeMap::new(), Some(name.into()), None).unwrap()[0]
    }

    fn a_cylinder(ctx: &mut Context, at: P2, z: f64, r: f64, h: f64, name: &str) -> BodyId {
        let s = Sketch::new(Plane::XY.offset(z)).with_loop(Loop::circle(at, r));
        ctx.extrude("cylinder", &s, None, h, BTreeMap::new(), Some(name.into()), None).unwrap()[0]
    }

    /// A box built where it stands, so its faces carry names: a copy made by
    /// `transform` before its source was ever meshed has none to inherit.
    fn a_block(ctx: &mut Context, at: P2, w: f64, d: f64, z0: f64, h: f64, name: &str) -> BodyId {
        let s = Sketch::new(Plane::XY.offset(z0)).with_loop(Loop::rect(at, w, d));
        ctx.extrude("box", &s, None, h, BTreeMap::new(), Some(name.into()), None).unwrap()[0]
    }

    /// A D-shaped post: a cylinder with a flat cut across it at y = 5.
    fn a_d_post(ctx: &mut Context) -> BodyId {
        let post = a_cylinder(ctx, P2::new(0.0, 0.0), 0.0, 10.0, 12.0, "post");
        let flat = a_block(ctx, P2::new(0.0, 10.0), 30.0, 10.0, -2.0, 20.0, "flat");
        ctx.boolean(BoolOp::Subtract, post, &[flat], KernelChoice::Brep, Some("d".into()), None).unwrap()
    }

    fn volume(ctx: &mut Context, id: BodyId) -> f64 {
        ctx.body_mesh(id).unwrap().0.volume()
    }

    /// The closed form against the tessellation's volume. A mesh holds `f32`
    /// positions, so a 6000 mm³ body is measured to about a microlitre; the
    /// planar cases are exact beyond that.
    fn assert_volume(v: f64, expect: f64) {
        assert!((v - expect).abs() <= 1e-6 * expect, "volume {v}, expected {expect}");
    }

    fn names_of(ctx: &mut Context, id: BodyId) -> Vec<String> {
        let mut n: Vec<String> = ctx.body_mesh(id).unwrap().1.faces.iter().map(|f| f.name.clone()).collect();
        n.sort();
        n
    }

    fn edges_named(ctx: &mut Context, id: BodyId, pred: impl Fn(&EdgeInfo) -> bool) -> Vec<String> {
        ctx.body_mesh(id).unwrap().1.edges.iter().filter(|e| pred(e)).map(|e| e.name.clone()).collect()
    }

    fn at_z(e: &EdgeInfo, z: f64) -> bool {
        (e.start.z - z).abs() < 1e-6 && (e.end.z - z).abs() < 1e-6
    }

    fn top_rim(ctx: &mut Context, id: BodyId, z: f64) -> Vec<String> {
        edges_named(ctx, id, |e| at_z(e, z))
    }

    /// The gate every result is held to: a solid by every test, no fold,
    /// every face a recognised surface, no fallback name, still exact.
    fn assert_clean(ctx: &mut Context, id: BodyId) {
        let (mesh, topo) = ctx.body_mesh(id).unwrap().clone();
        let report = solid::validate(&mesh);
        assert!(report.is_solid(), "{}", report.problems().join("; "));
        assert!(report.folds.is_empty(), "{} folds", report.folds.len());
        for f in &topo.faces {
            assert!(f.surface.is_recognised(), "{} is {}", f.name, f.surface_kind);
            // `face[3]` is the unmatched fallback; a chamfer face carries an
            // edge name in the brackets, never a bare number.
            let local = naming::local_of(&f.name);
            let fallback =
                local.strip_prefix("face[").map(|r| r.trim_end_matches(']').chars().all(|c| c.is_ascii_digit())).unwrap_or(false);
            assert!(!fallback, "a fallback name: {}", f.name);
        }
        assert_eq!(ctx.body(id).unwrap().kind(), "brep");
    }

    /// A box's top rim: four planar chamfer faces named after the edges they
    /// replaced, the neighbours kept, and the volume the closed form gives —
    /// four wedges of `½c²` per unit length, less the corner each pair of
    /// wedges removed twice, which is the pyramid `c³/3` at each of the four
    /// corners (the mitre is exact: `(c/2, c/2)` on the diagonal).
    #[test]
    fn a_box_top_rim_is_four_planar_faces_with_the_mitred_volume() {
        let (w, d, h, c) = (30.0, 20.0, 10.0, 1.0);
        let mut ctx = Context::new();
        let blk = a_box(&mut ctx, w, d, h, "blk");
        let rim = top_rim(&mut ctx, blk, h);
        assert_eq!(rim.len(), 4, "{rim:?}");
        let out = ctx.chamfer(blk, &rim, Setback::Equal(c), Some("cham".into()), None).unwrap();
        assert_clean(&mut ctx, out.body);

        let v = volume(&mut ctx, out.body);
        assert_volume(v, w * d * h - 0.5 * c * c * 2.0 * (w + d) + 4.0 * c * c * c / 3.0);

        let names = names_of(&mut ctx, out.body);
        assert_eq!(names.len(), 10, "{names:?}");
        for e in &rim {
            assert!(names.contains(&format!("cham/face[{e}]")), "{names:?}");
        }
        for kept in ["blk/top", "blk/bottom", "blk/front", "blk/back", "blk/left", "blk/right"] {
            assert!(names.contains(&kept.to_owned()), "{names:?}");
        }
        let topo = ctx.body_mesh(out.body).unwrap().1.clone();
        for f in topo.faces.iter().filter(|f| f.name.starts_with("cham/")) {
            assert!(matches!(f.surface, SurfaceClass::Plane { .. }), "{}: {:?}", f.name, f.surface);
            // A 45° plane: its normal is halfway between the two faces'.
            assert!((f.normal.z - 0.5f64.sqrt()).abs() < 1e-6, "{}: {:?}", f.name, f.normal);
        }
        // The mitres: four edges between two chamfer faces, each the
        // corner's diagonal, and no sliver anywhere.
        let mitres: Vec<&EdgeInfo> = topo.edges.iter().filter(|e| e.faces.iter().all(|n| n.starts_with("cham/"))).collect();
        assert_eq!(mitres.len(), 4, "four mitre lines");
        for m in &mitres {
            // From `(c, c, 0)` in from the corner on the top to `(0, 0, c)`
            // down the vertical edge: a body diagonal of a `c` cube.
            assert!((m.length - c * 3f64.sqrt()).abs() < 1e-6, "a mitre runs the corner's diagonal: {}", m.length);
        }
        assert!(topo.edges.iter().all(|e| e.length > 1.0), "no sliver edge");
        // The record stage 2 reads: the setback per side, and the curves.
        assert_eq!(out.edges.len(), 4);
        for r in &out.edges {
            assert_eq!(r.setback, [c, c]);
            assert!((r.wedge - PI / 2.0).abs() < 1e-9);
            assert!(r.convex);
            assert!(matches!(r.curves[0], SetbackCurve::Line { .. }));
            assert!(r.faces[0] < r.faces[1]);
        }
        // A chamfer face is a plane: the tessellation's chord error is zero.
        let s = ctx.body(out.body).unwrap().solid().unwrap().clone();
        let t = kernel::tessellate_reporting(&s, 0.05).unwrap();
        assert_eq!(t.chord.map(|e| e.max).unwrap_or(0.0), 0.0, "{:?}", t.chord);
    }

    /// All twelve edges: one tool of twelve cutters, three crossing at every
    /// corner, and one subtraction. Volume by inclusion–exclusion
    /// over the twelve wedges: `½c²` per unit length, and the pairwise and
    /// triple corner overlaps sum to `+6c³`.
    #[test]
    fn all_twelve_edges_of_a_box_mitre_at_every_corner() {
        let (w, d, h, c) = (24.0, 16.0, 8.0, 1.5);
        let mut ctx = Context::new();
        let blk = a_box(&mut ctx, w, d, h, "blk");
        let out = ctx.chamfer(blk, &[], Setback::Equal(c), Some("cham".into()), None).unwrap();
        assert_clean(&mut ctx, out.body);
        let v = volume(&mut ctx, out.body);
        assert_volume(v, w * d * h - 0.5 * c * c * 4.0 * (w + d + h) + 6.0 * c * c * c);
        let names = names_of(&mut ctx, out.body);
        assert_eq!(names.len(), 18, "{names:?}");
        assert_eq!(names.iter().filter(|n| n.starts_with("cham/face[")).count(), 12);
        let topo = ctx.body_mesh(out.body).unwrap().1.clone();
        // Three chamfer faces meet at each of the eight corners.
        let corners = topo.vertices.iter().filter(|v| v.faces.len() == 3 && v.faces.iter().all(|n| n.starts_with("cham/"))).count();
        assert_eq!(corners, 8, "{:?}", topo.vertices.iter().map(|v| &v.name).collect::<Vec<_>>());
    }

    /// Two chamfers on one face whose setbacks meet or overlap: each fits the
    /// face alone, together they take all of it or more. The refusal names
    /// both edges, the face and the width between them — on a cube's twelve
    /// edges (which used to fail inside the cutters' union) and on four
    /// parallel edges (which used to be accepted, the overlap silently
    /// eating the narrow faces).
    #[test]
    fn two_setbacks_that_overlap_on_a_face_are_refused_with_the_width_between_them() {
        let mut ctx = Context::new();
        let cube = a_box(&mut ctx, 10.0, 10.0, 10.0, "cube");
        let msg = ctx.chamfer(cube, &[], Setback::Equal(6.0), None, None).unwrap_err().to_string();
        assert!(msg.starts_with("chamfer: the chamfer faces at edges `cube/"), "{msg}");
        assert!(msg.contains("10 mm apart") && msg.contains("6 mm and 6 mm"), "{msg}");
        assert!(msg.contains("under 5 mm"), "{msg}");
        let mut ctx = Context::new();
        let blk = a_box(&mut ctx, 10.0, 20.0, 30.0, "blk");
        let verticals = edges_named(&mut ctx, blk, |e| (e.start.x - e.end.x).abs() < 1e-6 && (e.start.y - e.end.y).abs() < 1e-6);
        assert_eq!(verticals.len(), 4);
        let msg = ctx.chamfer(blk, &verticals, Setback::Equal(6.0), None, None).unwrap_err().to_string();
        assert!(msg.contains("10 mm apart") && msg.contains("both run along `blk/"), "{msg}");
        // On the 20 mm faces 6 + 6 fits; on the 10 mm faces 4 + 4 does.
        let out = ctx.chamfer(blk, &verticals, Setback::Equal(4.0), None, None).unwrap();
        assert_clean(&mut ctx, out.body);
    }

    /// An open chain of two edges turning a convex corner mitres at that
    /// corner and overruns into air at the other two ends.
    #[test]
    fn a_chain_that_turns_a_corner_mitres_there_and_overruns_its_free_ends() {
        let (w, d, h, c) = (30.0, 20.0, 10.0, 2.0);
        let mut ctx = Context::new();
        let blk = a_box(&mut ctx, w, d, h, "blk");
        let two: Vec<String> = edges_named(&mut ctx, blk, |e| {
            let front = e.start.y.min(e.end.y) < -d / 2.0 + 1e-6 && (e.start.y - e.end.y).abs() < 1e-6;
            let right = e.start.x.min(e.end.x) > w / 2.0 - 1e-6 && (e.start.x - e.end.x).abs() < 1e-6;
            at_z(e, h) && (front || right)
        });
        assert_eq!(two.len(), 2, "{two:?}");
        let out = ctx.chamfer(blk, &two, Setback::Equal(c), None, None).unwrap();
        assert_clean(&mut ctx, out.body);
        let v = volume(&mut ctx, out.body);
        assert_volume(v, w * d * h - 0.5 * c * c * (w + d) + c * c * c / 3.0);
        assert_eq!(names_of(&mut ctx, out.body).len(), 8);
        let topo = ctx.body_mesh(out.body).unwrap().1.clone();
        assert_eq!(topo.edges.iter().filter(|e| e.faces.iter().all(|n| n.starts_with("chamfer_1/"))).count(), 1);
    }

    /// One edge whose ends are convex vertices: a single wedge, asymmetric,
    /// with `d₁` along the first-named face.
    #[test]
    fn a_single_edge_ending_at_convex_vertices_is_one_asymmetric_wedge() {
        let (w, d, h) = (30.0, 20.0, 10.0);
        let mut ctx = Context::new();
        let blk = a_box(&mut ctx, w, d, h, "blk");
        let front: Vec<String> = edges_named(&mut ctx, blk, |e| at_z(e, h) && e.start.y < 0.0 && (e.start.y - e.end.y).abs() < 1e-6);
        assert_eq!(front.len(), 1);
        let out = ctx.chamfer(blk, &front, Setback::Two(1.0, 2.0), None, None).unwrap();
        assert_clean(&mut ctx, out.body);
        let v = volume(&mut ctx, out.body);
        assert_volume(v, w * d * h - 0.5 * 1.0 * 2.0 * w);
        let r = &out.edges[0];
        assert_eq!(r.faces, ["blk/front".to_owned(), "blk/top".to_owned()]);
        assert_eq!(r.setback, [1.0, 2.0]);
        // 1 mm down the front, 2 mm back across the top.
        let SetbackCurve::Line { start, end } = &r.curves[0] else { panic!() };
        assert!((start.z - (h - 1.0)).abs() < 1e-9 && (end.z - (h - 1.0)).abs() < 1e-9, "{:?}", r.curves);
        let SetbackCurve::Line { start, .. } = &r.curves[1] else { panic!() };
        assert!((start.y - (-d / 2.0 + 2.0)).abs() < 1e-9, "{:?}", r.curves);
    }

    /// The distance–angle form: `d` on the first face and the chamfer face
    /// leaving it at 30°, so the second setback is `d·tan 30°` at a right
    /// angle.
    #[test]
    fn the_angle_form_sets_the_second_setback_by_the_law_of_sines() {
        let (w, d, h) = (30.0, 20.0, 10.0);
        let mut ctx = Context::new();
        let blk = a_box(&mut ctx, w, d, h, "blk");
        let front: Vec<String> = edges_named(&mut ctx, blk, |e| at_z(e, h) && e.start.y < 0.0 && (e.start.y - e.end.y).abs() < 1e-6);
        let out = ctx.chamfer(blk, &front, Setback::Angle { d: 2.0, angle: 30f64.to_radians() }, None, None).unwrap();
        let d2 = 2.0 * fmath::tan(30f64.to_radians());
        assert!((out.edges[0].setback[1] - d2).abs() < 1e-12, "{:?}", out.edges[0].setback);
        let v = volume(&mut ctx, out.body);
        assert_volume(v, w * d * h - 0.5 * 2.0 * d2 * w);
    }

    /// Stage 2's entry point: the setback of a fillet is `r·cot(γ/2)`, and
    /// `chamfer_plan` returns the curves without cutting anything.
    #[test]
    fn the_fillet_setback_is_r_cot_half_the_wedge_and_the_plan_cuts_nothing() {
        assert!((fillet_setback(2.0, PI / 2.0) - 2.0).abs() < 1e-12);
        assert!((fillet_setback(1.0, PI / 3.0) - 3f64.sqrt()).abs() < 1e-12);
        let mut ctx = Context::new();
        let blk = a_box(&mut ctx, 30.0, 20.0, 10.0, "blk");
        let rim = top_rim(&mut ctx, blk, 10.0);
        let plan = ctx.chamfer_plan(blk, &rim, Setback::Fillet { radius: 3.0 }).unwrap();
        assert_eq!(plan.len(), 4);
        for r in &plan {
            assert!((r.setback[0] - 3.0).abs() < 1e-12 && (r.setback[1] - 3.0).abs() < 1e-12);
            let SetbackCurve::Line { start, end } = &r.curves[0] else { panic!() };
            assert!((*start - *end).length() > 1.0);
        }
        assert!(ctx.body(blk).unwrap().visible, "the plan consumes nothing");
        assert_eq!(ctx.features.len(), 1);
    }

    /// A cylinder's rim: the cutter is a ring lofted about the axis, the
    /// chamfer face a cone (two pieces under one name, as the wall is), and
    /// the volume Pappus's rule gives for the revolved wedge — which the
    /// same chamfer drawn into a revolved profile also gives.
    #[test]
    fn a_cylinder_rim_is_one_conical_face_with_the_revolved_wedge_volume() {
        let (r, h, c) = (10.0, 12.0, 1.5);
        let mut ctx = Context::new();
        let post = a_cylinder(&mut ctx, P2::new(0.0, 0.0), 0.0, r, h, "post");
        let rim = top_rim(&mut ctx, post, h);
        assert_eq!(rim.len(), 2, "a circle is two arcs: {rim:?}");
        let out = ctx.chamfer(post, &rim, Setback::Equal(c), Some("cham".into()), None).unwrap();
        assert_clean(&mut ctx, out.body);
        assert_eq!(out.edges.len(), 1, "one run for the whole circle");
        assert_eq!(out.edges[0].aliases.len(), 1);
        // `post/side#1` sorts before `post/top`, so the wall is the first face.
        assert_eq!(out.edges[0].faces, ["post/side#1".to_owned(), "post/top".to_owned()]);
        let SetbackCurve::Circle { radius, sweep, .. } = out.edges[0].curves[0] else { panic!("{:?}", out.edges[0].curves) };
        assert!((radius - r).abs() < 1e-9 && (sweep - TAU).abs() < 1e-9, "down the wall: the same circle, lower");
        let SetbackCurve::Circle { radius, .. } = out.edges[0].curves[1] else { panic!() };
        assert!((radius - (r - c)).abs() < 1e-9, "across the top: a smaller circle");
        let names = names_of(&mut ctx, out.body);
        // The ring is lofted through two-arc circles, so the cone is two
        // pieces under the one name, as the wall is.
        let cones: Vec<&String> = names.iter().filter(|n| n.starts_with("cham/face[")).collect();
        assert_eq!(cones.len(), 2, "{names:?}");
        let topo = ctx.body_mesh(out.body).unwrap().1.clone();
        for f in topo.faces.iter().filter(|f| f.name.starts_with("cham/")) {
            let SurfaceClass::Cone { half_angle, .. } = f.surface else { panic!("{}: {:?}", f.name, f.surface) };
            assert!((half_angle - PI / 4.0).abs() < 1e-6);
            assert!(f.aliases.iter().any(|a| a == &format!("cham/face[{}]", rim[0])), "{:?}", f.aliases);
        }
        let expect = PI * r * r * h - 0.5 * c * c * TAU * (r - c / 3.0);
        let v = volume(&mut ctx, out.body);
        // Measured on a tessellation of a cylinder and a cone, so within the
        // chord band rather than exact: the wall and the cone's rims are
        // 32-gons at 0.05 (an arc is halved until its sagitta is under the
        // tolerance, and so is an intersection curve that is a circle), 0.64 % under the
        // closed form — the same deficit a 3 mm peg has under the halving.
        assert!((v - expect).abs() / expect < 1e-2, "volume {v}, expected {expect}");
        let profile = Sketch::new(Plane::XZ).with_loop(Loop::polygon(&[
            P2::new(0.0, 0.0),
            P2::new(r, 0.0),
            P2::new(r, h - c),
            P2::new(r - c, h),
            P2::new(0.0, h),
        ]));
        let rev = ctx.revolve("revolve", &profile, None, Vec3::ZERO, Vec3::Z, TAU, Some("rev".into()), None).unwrap()[0];
        let vr = volume(&mut ctx, rev);
        // Both are chorded tessellations of one solid: the post's wall is a
        // 32-gon of two half-turn arcs halved to the tolerance, the
        // revolve's rims 48-gons of three third-turn arcs, and the chamfer's own rims are the boolean's
        // intersection curves — so they agree to a few thousandths, not to
        // the last digit.
        assert!((v - vr).abs() / expect < 5e-3, "chamfer {v} vs profile {vr}");
    }

    /// A boss on a plate: its root is an inner loop of the plate's top, and
    /// the bead's outer circle would enclose it — the one configuration
    /// truck's boolean does not split (a counterbore cut into an existing
    /// hole fails the same way), so it is refused by name with the two
    /// constructions that do work.
    #[test]
    fn a_boss_root_is_an_inner_loop_and_is_refused_by_name() {
        let (w, d, t, r, hb, c) = (40.0, 40.0, 5.0, 8.0, 10.0, 1.0);
        let mut ctx = Context::new();
        let plate = a_box(&mut ctx, w, d, t, "plate");
        let boss = a_cylinder(&mut ctx, P2::new(0.0, 0.0), t - 1.0, r, hb + 1.0, "boss");
        let part = ctx.boolean(BoolOp::Union, plate, &[boss], KernelChoice::Brep, Some("part".into()), None).unwrap();
        // The root reads as concave.
        let root: Vec<String> = edges_named(&mut ctx, part, |e| at_z(e, t) && !e.convex);
        assert_eq!(root.len(), 2, "{root:?}");
        let msg = refusal(&mut ctx, part, &root, Setback::Equal(c));
        assert_eq!(
            msg,
            format!(
                "chamfer: edge `{}` is an inner loop of `plate/top` — `boss/side#0` is a hole in it, or a boss standing \
                 on it — and its bead would replace that loop of `plate/top` by a wider circle; the exact kernel does \
                 not split a face along a curve that encloses one of its inner loops (a counterbore cut into an \
                 existing hole fails the same way). Cut the hole and its countersink, or build the boss and its bead, \
                 as one tool before the boolean (a cylinder unioned with a `cone`, as the Gridfinity baseplate does), \
                 build it into the profile, or pass kernel=\"field\" for a mesh result",
                root[0]
            )
        );
    }

    /// A blind hole: its mouth is an inner loop of the plate's top and is
    /// refused (as the boss root is), but its floor's rim is the floor's
    /// own outer loop, and the concave bead there is a ring oversized
    /// *inward* and unioned, with the volume Pappus gives.
    #[test]
    fn a_hole_mouth_is_refused_and_a_blind_floor_takes_a_bead() {
        let (w, d, t, r, c) = (30.0, 30.0, 8.0, 4.0, 1.0);
        let mut ctx = Context::new();
        let plate = a_box(&mut ctx, w, d, t, "plate");
        let bore = a_cylinder(&mut ctx, P2::new(0.0, 0.0), 3.0, r, 10.0, "bore");
        let part = ctx.boolean(BoolOp::Subtract, plate, &[bore], KernelChoice::Brep, Some("part".into()), None).unwrap();
        let rim: Vec<String> = edges_named(&mut ctx, part, |e| at_z(e, t) && e.length < TAU * r);
        assert_eq!(rim.len(), 2, "{rim:?}");
        assert!(ctx.body_mesh(part).unwrap().1.edges.iter().filter(|e| rim.contains(&e.name)).all(|e| e.convex));
        let msg = refusal(&mut ctx, part, &rim, Setback::Equal(c));
        assert!(msg.starts_with(&format!("chamfer: edge `{}` is an inner loop of `plate/top` — `bore/side#", rim[0])), "{msg}");
        assert!(msg.contains("and its chamfer would replace that loop of `plate/top` by a wider circle"), "{msg}");

        let floor: Vec<String> = edges_named(&mut ctx, part, |e| at_z(e, 3.0));
        assert_eq!(floor.len(), 2, "{floor:?}");
        assert!(ctx.body_mesh(part).unwrap().1.edges.iter().filter(|e| floor.contains(&e.name)).all(|e| !e.convex));
        let out = ctx.chamfer(part, &floor, Setback::Equal(c), Some("bead".into()), None).unwrap();
        assert_clean(&mut ctx, out.body);
        assert!(!out.edges[0].convex);
        assert_eq!(out.edges.len(), 1);
        let v = volume(&mut ctx, out.body);
        let expect = w * d * t - PI * r * r * (t - 3.0) + 0.5 * c * c * TAU * (r - c / 3.0);
        assert!((v - expect).abs() / expect < 2e-3, "volume {v}, expected {expect}");
        let names = names_of(&mut ctx, out.body);
        assert!(names.iter().any(|n| n == "plate/top"), "{names:?}");
        assert_eq!(names.iter().filter(|n| n.starts_with("bead/face[")).count(), 2, "{names:?}");
        let topo = ctx.body_mesh(out.body).unwrap().1.clone();
        for f in topo.faces.iter().filter(|f| f.name.starts_with("bead/")) {
            assert!(matches!(f.surface, SurfaceClass::Cone { .. }), "{}: {:?}", f.name, f.surface);
        }
    }

    /// A D-shaped post: the rim is an arc and a straight edge meeting at
    /// two corners on the wall. Together they are refused — the cone–plane
    /// mitre leaves the corner on the curved wall, the one four-valent
    /// vertex truck does not build; alone, each dies into the other's face
    /// obliquely and is refused for that.
    #[test]
    fn an_arc_meeting_a_line_on_a_curved_wall_is_refused_by_name() {
        let mut ctx = Context::new();
        let d = a_d_post(&mut ctx);
        let rim = top_rim(&mut ctx, d, 12.0);
        assert_eq!(rim.len(), 4, "three arcs (the wall's halves, one split by the flat) and the flat's edge: {rim:?}");
        let flat: Vec<String> = rim.iter().filter(|n| n.starts_with("flat/")).cloned().collect();
        let arcs: Vec<String> = rim.iter().filter(|n| !n.starts_with("flat/")).cloned().collect();
        assert_eq!(flat.len(), 1, "{rim:?}");

        let msg = refusal(&mut ctx, d, &rim, Setback::Equal(1.0));
        assert!(msg.starts_with("chamfer: `post/side#"), "{msg}");
        assert!(
            msg.contains("(an arc) and `flat/front|post/top` (straight) meet at (")
                && msg.ends_with(
                    "where the plane touches that wall at a single point — a vertex of four faces the exact kernel \
                     builds only when all four are planes. Build this chamfer into the profile (a loft between the \
                     outline and its inset), or pass kernel=\"field\" for a mesh result"
                ),
            "{msg}"
        );
        // The flat's edge alone: its ends run into the wall, which bulges
        // past them.
        let msg = refusal(&mut ctx, d, &flat, Setback::Equal(1.0));
        assert!(msg.starts_with("chamfer: edge `flat/front|post/top` dies into `post/side#"), "{msg}");
        // The arcs alone: they end against the flat.
        let msg = refusal(&mut ctx, d, &arcs, Setback::Equal(1.0));
        assert!(msg.starts_with("chamfer: edge `post/side#") && msg.contains("` dies into `flat/front` at ("), "{msg}");
    }

    // ------------------------------------------------------------ refusals

    fn refusal(ctx: &mut Context, body: BodyId, edges: &[String], setback: Setback) -> String {
        match ctx.chamfer(body, edges, setback, None, None) {
            Ok(_) => panic!("expected a refusal"),
            Err(e) => e.to_string(),
        }
    }

    /// An L: a 4 mm plate (x ±20, y ±15) with an 8 mm flange standing at
    /// its end, x = 16..24, sunk 2 mm into it and 3 mm deeper than it on
    /// both sides, so the union is a crossing with no flush face and the
    /// plate's top stays one face; the plate-top edges die into the flange.
    fn an_l(ctx: &mut Context) -> BodyId {
        let plate = a_box(ctx, 40.0, 30.0, 4.0, "plate");
        let s = Sketch::new(Plane::XY.offset(2.0)).with_loop(Loop::rect(P2::new(20.0, 0.0), 8.0, 36.0));
        let flange = ctx.extrude("box", &s, None, 20.0, BTreeMap::new(), Some("flange".into()), None).unwrap()[0];
        ctx.boolean(BoolOp::Union, plate, &[flange], KernelChoice::Brep, Some("l".into()), None).unwrap()
    }

    /// The plate's top-front edge, from x=-20 to the flange at x=16.
    fn plate_front_edge(ctx: &mut Context, l: BodyId) -> Vec<String> {
        edges_named(ctx, l, |e| at_z(e, 4.0) && e.start.y < -14.0 && e.end.y < -14.0 && e.start.x.max(e.end.x) > 10.0)
    }

    /// The concave root edge where the plate's top meets the flange's wall.
    fn root_edge(ctx: &mut Context, l: BodyId) -> Vec<String> {
        edges_named(ctx, l, |e| at_z(e, 4.0) && (e.start.x - 16.0).abs() < 1e-6 && (e.end.x - 16.0).abs() < 1e-6)
    }

    #[test]
    fn an_edge_dying_into_a_face_at_a_concave_vertex_is_refused_by_name() {
        let mut ctx = Context::new();
        let l = an_l(&mut ctx);
        let edge = plate_front_edge(&mut ctx, l);
        assert_eq!(edge.len(), 1, "{edge:?}");
        let msg = refusal(&mut ctx, l, &edge, Setback::Equal(1.0));
        assert_eq!(
            msg,
            format!(
                "chamfer: edge `{}` dies into `flange/left` at (16.000, -15.000, 4.000): the cutter would have to stop \
                 flush with that face, which is a coplanar contact the exact kernel refuses, and running past it \
                 would notch it. Chamfer the edge along `flange/left` as well, so the two \
                 cutters mitre; chamfer the closed loop the edge belongs to; build the step into the profile; or pass \
                 kernel=\"field\" for a mesh result",
                edge[0]
            )
        );
    }

    #[test]
    fn a_chain_that_changes_concavity_is_refused() {
        let mut ctx = Context::new();
        let l = an_l(&mut ctx);
        let mut pair = plate_front_edge(&mut ctx, l);
        pair.extend(root_edge(&mut ctx, l));
        assert_eq!(pair.len(), 2, "{pair:?}");
        let msg = refusal(&mut ctx, l, &pair, Setback::Equal(1.0));
        assert_eq!(
            msg,
            format!(
                "chamfer: the chain changes concavity at (16.000, -15.000, 4.000): `{}` is convex (material is cut \
                 away) and `{}` is concave (material is added), and the two cutters cannot mitre. \
                 Chamfer the two in separate calls, or pass kernel=\"field\"",
                pair[0], pair[1]
            )
        );
    }

    #[test]
    fn a_setback_wider_than_a_neighbour_is_refused_with_the_width_that_fits() {
        let mut ctx = Context::new();
        let blk = a_box(&mut ctx, 30.0, 20.0, 4.0, "blk");
        let front: Vec<String> = edges_named(&mut ctx, blk, |e| at_z(e, 4.0) && e.start.y < 0.0 && (e.start.y - e.end.y).abs() < 1e-6);
        let msg = refusal(&mut ctx, blk, &front, Setback::Equal(5.0));
        // 5 mm fits across the 20 mm top but not down the 4 mm front; what
        // fits is the height less the clearance the curve must keep (four
        // chamfer tolerances, 0.12 mm).
        assert_eq!(
            msg,
            "chamfer: 5 mm along `blk/front` at edge `blk/front|top` is wider than the face: the setback curve leaves \
             it, so the cutter would swallow `blk/front` and the chamfer face would end on its far edge. The largest \
             setback that fits there is about 3.88 mm"
        );
    }

    #[test]
    fn a_concave_straight_edge_is_refused_until_beads_can_end() {
        let mut ctx = Context::new();
        let l = an_l(&mut ctx);
        let root = root_edge(&mut ctx, l);
        assert_eq!(root.len(), 1, "{root:?}");
        let msg = refusal(&mut ctx, l, &root, Setback::Equal(1.0));
        assert_eq!(
            msg,
            format!(
                "chamfer: edge `{}` is concave (the chamfer adds a bead between `flange/left` and `plate/top` rather \
                 than cutting), and it is a straight concave edge: the bead would need to end flush with the faces it \
                 runs into, which is a coplanar contact the exact kernel refuses, and overrunning would add material \
                 in air. This version builds beads only around a closed circular loop (a boss root, a blind hole's \
                 floor). Build the bead into the profile, or pass kernel=\"field\" for a mesh result",
                root[0]
            )
        );
    }

    #[test]
    fn an_edge_on_a_free_form_face_is_refused() {
        let mut ctx = Context::new();
        let pts = [P2::new(10.0, 0.0), P2::new(0.0, 8.0), P2::new(-10.0, 0.0), P2::new(0.0, -8.0)];
        let s = Sketch::new(Plane::XY).with_loop(Loop::spline(&pts, 0.5).unwrap());
        let blob = ctx.extrude("extrude", &s, None, 5.0, BTreeMap::new(), Some("blob".into()), None).unwrap()[0];
        let rim = top_rim(&mut ctx, blob, 5.0);
        assert!(!rim.is_empty());
        let msg = refusal(&mut ctx, blob, &rim[..1], Setback::Equal(0.5));
        assert!(msg.starts_with(&format!("chamfer: edge `{}` lies on `blob/side[", rim[0])), "{msg}");
        assert!(
            msg.ends_with(
                "a free-form surface (bspline), so the angle between its faces varies along it and the chamfer face \
                 would be a ruled surface between two on-surface offsets, which the exact path does not build. \
                 Build the chamfer into the profile, or pass kernel=\"field\" for a mesh result"
            ),
            "{msg}"
        );
    }

    /// A cylinder cut by an oblique plane: the rim is an ellipse between two
    /// recognised faces, and the cutter has no line or axis to run along.
    #[test]
    fn a_free_curve_edge_is_refused_by_name() {
        let mut ctx = Context::new();
        let post = a_cylinder(&mut ctx, P2::new(0.0, 0.0), 0.0, 6.0, 20.0, "post");
        let plane = Plane::new(Vec3::new(0.0, 0.0, 12.0), Vec3::new(0.0, 0.5, 1.0), Vec3::X).unwrap();
        let pieces = ctx.split(post, &crate::SplitTool::Plane(plane), KernelChoice::Brep, Some("cut".into()), None).unwrap();
        let lower = pieces[0];
        let rim: Vec<String> = edges_named(&mut ctx, lower, |e| e.faces.iter().any(|f| f == "cut/cut") && e.curve_kind != "line");
        assert!(!rim.is_empty());
        let msg = refusal(&mut ctx, lower, &rim[..1], Setback::Equal(0.5));
        assert!(msg.starts_with(&format!("chamfer: edge `{}` is neither straight nor a circle (a ", rim[0])), "{msg}");
        assert!(
            msg.ends_with(
                "and the exact path carries its cutter only along a line or about an axis. Build the chamfer into \
                 the profile (for a whole cap loop, a loft between the outline and its inset is exact), or pass \
                 kernel=\"field\" for a mesh result"
            ),
            "{msg}"
        );
    }

    #[test]
    fn a_smooth_seam_is_refused_when_asked_for_and_skipped_otherwise() {
        let mut ctx = Context::new();
        let post = a_cylinder(&mut ctx, P2::new(0.0, 0.0), 0.0, 6.0, 10.0, "post");
        // A circle is two arcs, so the wall has two seams, `@0` and `@1`.
        let seams: Vec<String> = edges_named(&mut ctx, post, |e| e.name.starts_with("post/side#0|side#1"));
        assert_eq!(seams.len(), 2, "{seams:?}");
        let msg = refusal(&mut ctx, post, &seams[..1], Setback::Equal(0.5));
        assert_eq!(
            msg,
            format!(
                "chamfer: edge `{}` is a smooth seam, not a corner: `post/side#0` and `post/side#1` meet at 0.0° there \
                 (a cylinder's seam between its two halves is one), so there is nothing to chamfer and the cutter's \
                 face would be tangent to both",
                seams[0]
            )
        );
        // `chamfer(post, d)` with no edges skips the seams with a note and
        // chamfers both rims.
        let out = ctx.chamfer(post, &[], Setback::Equal(0.5), None, None).unwrap();
        assert_clean(&mut ctx, out.body);
        assert_eq!(out.edges.len(), 2);
        let note = format!("chamfer: skipped 2 smooth seams that are no corner: `{}`, `{}`", seams[0], seams[1]);
        assert!(ctx.diagnostics.iter().any(|d| d.message == note), "{:?}", ctx.diagnostics);
    }

    #[test]
    fn a_mesh_body_and_an_unknown_edge_are_refused_with_the_fix() {
        let mut ctx = Context::new();
        let blk = a_box(&mut ctx, 10.0, 10.0, 10.0, "blk");
        let msg = refusal(&mut ctx, blk, &["blk/nowhere".into()], Setback::Equal(1.0));
        assert!(msg.starts_with("chamfer: `blk` has no edge named `blk/nowhere`; its edges are `blk/"), "{msg}");
        let mesh = ctx.body_mesh(blk).unwrap().clone();
        let fid = ctx.new_feature("mesh", Some("m".into()), vec![], BTreeMap::new(), None).unwrap();
        let m = ctx.add_body(&fid, Body::Mesh(mesh), Some("m".into())).unwrap();
        let msg = refusal(&mut ctx, m, &[], Setback::Equal(1.0));
        assert_eq!(
            msg,
            "chamfer: `m` is a mesh body, which has no exact edges to chamfer; pass kernel=\"field\" to chamfer it on \
             the field path (the result is a mesh body)"
        );
    }

    /// A rim of 30 mm chamfered at 1.2: truck's tracer's polyline doubles
    /// back on the long circular seam, which truck's own face triangulator
    /// leaves as a hole. The edge-first tessellator straightens the
    /// shared polyline once and the faces close by construction, so
    /// the chamfer is built, is a solid, and has Pappus's volume. The 44 mm
    /// rim is the same case at four times the boolean's cost, and is left
    /// out of this test for that cost.
    #[test]
    fn a_rim_truck_could_not_trace_is_chamfered_and_meshes_closed() {
        for r in [30.0] {
            let (h, c) = (12.0, 1.2);
            let mut ctx = Context::new();
            let post = a_cylinder(&mut ctx, P2::new(0.0, 0.0), 0.0, r, h, "post");
            let rim = top_rim(&mut ctx, post, h);
            let out = ctx.chamfer(post, &rim, Setback::Equal(c), Some("cham".into()), None).unwrap_or_else(|e| panic!("r={r}: {e}"));
            assert_clean(&mut ctx, out.body);
            let (mesh, _) = ctx.body_mesh(out.body).unwrap().clone();
            assert!(mesh.welded(1e-4).is_watertight(), "r={r}: open");
            let expect = PI * r * r * h - 0.5 * c * c * TAU * (r - c / 3.0);
            let v = mesh.volume();
            assert!((v - expect).abs() / expect < 5e-3, "r={r}: volume {v}, expected {expect}");
        }
    }

    /// The angle form refuses an angle the chamfer face could never close.
    #[test]
    fn an_impossible_angle_is_refused() {
        let msg = Setback::Angle { d: 1.0, angle: 100f64.to_radians() }.per_edge(PI / 2.0).unwrap_err().to_string();
        assert_eq!(
            msg,
            "chamfer: an angle of 100.0° cannot leave a face that meets its neighbour at 90.0°: the chamfer face \
             would never reach the other face. The angle must be between 0 and 90.0°"
        );
    }
}
