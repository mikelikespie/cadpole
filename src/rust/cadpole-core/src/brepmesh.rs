//! The watertight tessellator: edge-first, bound-driven, constrained
//! (docs/design.md).
//!
//! **The rule.** The discretisation of an edge is a function of the edge
//! alone — its curve, its range, the tolerance and the normal bound the
//! faces it bounds ask of it — never of a face; and every sample is one
//! 3-D point with **one vertex id**, which both faces of the edge index.
//! Each face is then triangulated in its own parameter domain by a
//! constrained Delaunay triangulation ([`crate::cdt`]) whose constraints are
//! the edges' samples inverted onto the face, so every boundary segment is a
//! triangle edge on both sides of the edge, by construction, and the mesh is
//! closed without a weld. Positions on a face boundary come from the edge
//! table, not from `surface.subs(search_parameter(point))` — the two faces
//! never disagree by a Newton residual because there is nothing to disagree
//! about. There is no ladder of tolerance retries: a face that cannot be
//! triangulated is a diagnostic naming the face, the edges and the
//! parameter-domain reason.
//!
//! **Edges.** A line is its two ends. A polynomial spline is divided per
//! knot span by the Filip–Magedson–Markot chordal bound
//! `⅛·M·h² ≤ tol`, `M` the largest second-difference control point active
//! on the span. A rational curve that is a
//! circular arc — Cadpole's only rational curves — is halved until its
//! sagitta is under the tolerance (truck's rule); any other rational
//! span samples its second derivative with a margin. A boolean's
//! intersection curve is divided between the vertices of truck's tracer
//! polyline by a sag test at the quarter points, each sample projected onto
//! both surfaces from its neighbours' parameters rather than from a
//! whole-surface search (which can send a small cross-drilling in a
//! translated body off to the wrong sheet of its hub).
//! Every division is then verified at the quarter points of each piece and
//! split where the sag exceeds the tolerance — the rational estimate and the
//! intersection curves are not bounds: unverified, a boundary polyline came
//! out coarser than the tolerance on a nut's chamfer cone. On top of
//! all that, the normal-turn pieces
//! [`crate::ksurface::KSurface::edge_pieces`] asked for by the faces, and a
//! floor no bound can go under: a curved edge is at least its share of
//! [`crate::ksurface::MIN_PIECES_PER_TURN`] pieces a whole turn, an arc by
//! its angle and any other curve by its tangent's turning, so a circle
//! smaller than the tolerance is an octagon rather than two points.
//! A polyline that doubles back on itself — what
//! truck's tracer hands over on a long circular seam — is
//! straightened once, here, before either face sees it, and samples closer
//! than the weld distance to their predecessor are dropped.
//!
//! **Faces.** Boundary samples are inverted with the previous sample's
//! parameter as the hint, unrolled by the surface's period against the
//! previous sample (truck's rule), and given a corner where the surface is
//! degenerate between two samples (the pole line, truck's rule again, so
//! the loop runs *along* the pole). A loop whose image is open by one period
//! — a cap around a pole, a full band between two rims — is closed with the
//! seam line sampled *once* and inserted twice, a period apart, under the
//! same vertex ids, and with the pole line under the one id of the pole, so
//! the seam closes by identity rather than by two samplings agreeing. Every
//! triangle whose three vertex ids are not distinct is a pole sliver and is
//! dropped, which leaves a fan whose triangles all hold the pole's one id.
//! Interior points come from the per-direction, per-span division over
//! the loops' bounding box — for a revolved surface truck's, with never
//! fewer lines a direction than its turning asks for at eight a whole turn
//! ([`crate::ksurface::KSurface::grid`]) — inside the loops and clear of the
//! boundary by a twentieth of a cell. Inside and outside is decided by flooding from the
//! outside across constraint edges, so no loop orientation is inferred.
//! Normals are the surface's at each vertex's own parameter, per face
//! (they survive a mesh boolean); at a pole the normal is taken
//! a hair inside the domain.
//!
//! Faces are independent once the edge table exists, and are triangulated
//! in parallel on native targets; the output is assembled in face order and
//! is byte-identical however many threads ran, which the memo relies on.

use std::collections::HashMap;

use truck_geometry::prelude::double_projection;
use truck_modeling::{
    BSplineCurve, BoundedCurve, Curve, InnerSpace, KnotVec, ParametricCurve, ParametricSurface, Point3, SPHint2D,
    SearchParameter, Surface, Vector3,
};
use truck_topology::{EdgeID, VertexID};

use crate::cdt::{Cdt, CdtError, Snap};
use crate::fmath;
use crate::geom::Vec3;
use crate::kernel::{self, point_to_vec3, vector_to_vec3, TessellationSolid};
use crate::ksurface::{cumulative_turning, pieces_for_turning, DivisionBounds, KCurve, KSurface};

// ------------------------------------------------------------------ the output

/// One vertex of a face's mesh: the shared position, the face's parameter of
/// it, and the face's normal there (outward, the face's orientation applied).
#[derive(Clone, Copy, Debug)]
pub struct FaceVertex {
    /// Index into [`MeshedSolid::positions`]. Boundary vertices share theirs
    /// with the neighbouring faces; interior vertices are the face's own.
    pub id: u32,
    pub uv: [f64; 2],
    pub normal: Vector3,
}

/// One use of an edge by a face's wire.
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub struct EdgeUse {
    /// Index into [`MeshedSolid::edges`].
    pub edge: u32,
    /// Whether the wire runs the edge front to back.
    pub forward: bool,
}

/// A face's mesh.
#[derive(Clone, Debug, Default)]
pub struct MeshedFace {
    /// The B-rep face's orientation; `triangles` are already wound outward.
    pub orientation: bool,
    /// What the tessellator saw the surface as (`kernel::surface_kind` of
    /// the surface it triangulated: a recognised cone is `revolved`).
    pub kind: &'static str,
    pub vertices: Vec<FaceVertex>,
    /// Indices into `vertices`, wound so the normal points out of the solid.
    pub triangles: Vec<[u32; 3]>,
    /// The face's wires as edge uses, in the B-rep's order.
    pub wires: Vec<Vec<EdgeUse>>,
}

impl MeshedFace {
    /// The wires as the oriented face traverses them — what truck's
    /// `Face::boundaries` gives: the B-rep's own for a forward face, and for
    /// an inverted one each wire reversed with every use flipped. This is
    /// the walk [`crate::tessellate`] numbers edges and vertices in.
    pub fn oriented_wires(&self) -> Vec<Vec<EdgeUse>> {
        if self.orientation {
            self.wires.clone()
        } else {
            self.wires
                .iter()
                .map(|w| w.iter().rev().map(|u| EdgeUse { edge: u.edge, forward: !u.forward }).collect())
                .collect()
        }
    }
}

/// An edge's polyline, as vertex ids.
#[derive(Clone, Debug, Default)]
pub struct MeshedEdge {
    /// Front to back.
    pub ids: Vec<u32>,
    /// Indices into [`MeshedSolid::vertices`].
    pub front: u32,
    pub back: u32,
    /// truck's curve kind (`kernel::curve_kind`).
    pub kind: &'static str,
    /// A zero-length edge: one id, a pole.
    pub degenerate: bool,
}

/// A tessellated solid: one shared vertex table, a mesh per face, a polyline
/// per edge, all by id. Faces, edges and vertices are in the order a walk of
/// the solid's faces, their wires and their edges first meets them, which is
/// the order [`crate::tessellate`] names them in.
#[derive(Clone, Debug, Default)]
pub struct MeshedSolid {
    pub positions: Vec<Point3>,
    pub faces: Vec<MeshedFace>,
    pub edges: Vec<MeshedEdge>,
    /// Topological vertices, as position ids.
    pub vertices: Vec<u32>,
}

impl MeshedSolid {
    /// The corners of triangle `t` of face `f`, wound outward.
    pub fn triangle_points(&self, f: usize, t: usize) -> [Point3; 3] {
        let face = &self.faces[f];
        let tri = face.triangles[t];
        [
            self.positions[face.vertices[tri[0] as usize].id as usize],
            self.positions[face.vertices[tri[1] as usize].id as usize],
            self.positions[face.vertices[tri[2] as usize].id as usize],
        ]
    }

    /// Every triangle of the solid as outward-wound corners.
    pub fn triangles(&self) -> impl Iterator<Item = [Point3; 3]> + '_ {
        self.faces.iter().enumerate().flat_map(move |(f, face)| (0..face.triangles.len()).map(move |t| self.triangle_points(f, t)))
    }

    /// How many triangles, over every face.
    pub fn triangle_count(&self) -> usize {
        self.faces.iter().map(|f| f.triangles.len()).sum()
    }

    /// How many edges [`MeshedSolid::unmatched_edges`] would report, without
    /// its per-face breakdown: the one pass over the triangles the driver
    /// makes on every tessellation (`kernel::tessellate_reporting_with`).
    pub fn unmatched_edge_count(&self) -> usize {
        let mut edges: HashMap<(u32, u32), i32> = HashMap::with_capacity(self.triangle_count() * 3 / 2 + 1);
        for face in &self.faces {
            for tri in &face.triangles {
                let ids = [face.vertices[tri[0] as usize].id, face.vertices[tri[1] as usize].id, face.vertices[tri[2] as usize].id];
                for k in 0..3 {
                    let (a, b) = (ids[k], ids[(k + 1) % 3]);
                    let key = if a < b { (a, b) } else { (b, a) };
                    *edges.entry(key).or_insert(0) += if a < b { 1 } else { -1 };
                }
            }
        }
        edges.values().filter(|&&c| c != 0).count()
    }

    /// The edges, by vertex id, that are not shared by exactly two
    /// oppositely wound triangles: `0` for a closed mesh. Judged on the ids
    /// themselves, with no weld — this is what "watertight by construction"
    /// means. Per face: the number of such edges on the face's triangles.
    pub fn unmatched_edges(&self) -> (usize, Vec<usize>) {
        let mut edges: HashMap<(u32, u32), (i32, Vec<u32>)> = HashMap::new();
        for (fi, face) in self.faces.iter().enumerate() {
            for tri in &face.triangles {
                let ids = [face.vertices[tri[0] as usize].id, face.vertices[tri[1] as usize].id, face.vertices[tri[2] as usize].id];
                for k in 0..3 {
                    let (a, b) = (ids[k], ids[(k + 1) % 3]);
                    let key = if a < b { (a, b) } else { (b, a) };
                    let e = edges.entry(key).or_insert((0, Vec::new()));
                    e.0 += if a < b { 1 } else { -1 };
                    if !e.1.contains(&(fi as u32)) {
                        e.1.push(fi as u32);
                    }
                }
            }
        }
        let mut per_face = vec![0usize; self.faces.len()];
        let mut total = 0;
        for (_, (count, faces)) in edges {
            if count != 0 {
                total += 1;
                for f in faces {
                    per_face[f as usize] += 1;
                }
            }
        }
        (total, per_face)
    }
}

/// Why a face could not be tessellated: the face, what the tessellator saw
/// it as, where it is, and the exact reason in the parameter domain.
#[derive(Clone, Debug)]
pub struct TessellationError {
    pub face: usize,
    pub kind: &'static str,
    pub bbox: Option<(Vec3, Vec3)>,
    pub reason: String,
}

impl std::fmt::Display for TessellationError {
    fn fmt(&self, f: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
        match self.bbox {
            Some((lo, hi)) => write!(
                f,
                "face {} ({}, box ({:.3}, {:.3}, {:.3})..({:.3}, {:.3}, {:.3})) cannot be triangulated: {}",
                self.face, self.kind, lo.x, lo.y, lo.z, hi.x, hi.y, hi.z, self.reason
            ),
            None => write!(f, "face {} ({}) cannot be triangulated: {}", self.face, self.kind, self.reason),
        }
    }
}

// --------------------------------------------------------------- the edge phase

/// Samples closer than this to their predecessor along an edge are dropped:
/// the weld distance, the closest two vertices the mesh kernel tells apart.
const MIN_STEP: f64 = crate::mesh_kernel::WELD_EPS as f64;

/// A derivative shorter than this times the surface's scale is a degenerate
/// direction: a pole line.
const DEGENERATE_DER: f64 = 1e-7;

/// The most pieces one knot span of an edge is cut into by the bound.
const MAX_PIECES: usize = 1024;

struct EdgeRecord {
    ids: Vec<u32>,
    front: u32,
    back: u32,
    kind: &'static str,
    degenerate: bool,
}

/// Every position of a curve is asked for through this, because an
/// intersection curve's `subs` panics when its projection fails.
fn eval_curve(curve: &Curve, t: f64) -> Option<Point3> {
    match curve {
        Curve::IntersectionCurve(ic) => ic.search_triple(t).map(|x| x.0),
        other => {
            let p = other.subs(t);
            (p.x.is_finite() && p.y.is_finite() && p.z.is_finite()).then_some(p)
        }
    }
}

/// The distinct knots strictly inside `(t0, t1)`, ascending.
fn interior_knots(knots: &KnotVec, (t0, t1): (f64, f64)) -> Vec<f64> {
    let eps = 1e-9 * (t1 - t0).abs().max(1e-300);
    let mut out: Vec<f64> = Vec::new();
    for &k in knots.iter() {
        if k > t0 + eps && k < t1 - eps && out.last().is_none_or(|&l| k > l + eps) {
            out.push(k);
        }
    }
    out
}

/// The largest second-derivative control point active on the span holding
/// `t`, of the second-derivative curve `dd`.
fn second_difference_bound(dd: &BSplineCurve<Vector3>, t: f64) -> f64 {
    let knots = dd.knot_vec();
    let pts = dd.control_points();
    if pts.is_empty() {
        return 0.0;
    }
    let d = dd.degree();
    let n = knots.len();
    let mut s = knots.floor(t).unwrap_or(0).min(n.saturating_sub(2)).min(pts.len() - 1);
    while s > 0 && (s + 1 >= n || knots[s] >= knots[s + 1]) {
        s -= 1;
    }
    let lo = s.saturating_sub(d);
    pts[lo..=s.min(pts.len() - 1)].iter().map(|p| p.magnitude()).fold(0.0, f64::max)
}

fn pieces_for_bound(m: f64, h: f64, tol: f64) -> usize {
    if !(m.is_finite() && m > 0.0) {
        return 1;
    }
    ((m * h * h / (8.0 * tol)).sqrt().ceil() as usize).clamp(1, MAX_PIECES)
}

/// A polynomial B-spline curve's parameters: per span, the pieces the
/// control-net bound asks for.
fn bspline_params(c: &BSplineCurve<Point3>, range: (f64, f64), tol: f64) -> Vec<f64> {
    let dd = c.derivation().derivation();
    let mut breaks = vec![range.0];
    breaks.extend(interior_knots(c.knot_vec(), range));
    breaks.push(range.1);
    let mut out = vec![range.0];
    for w in breaks.windows(2) {
        let (a, b) = (w[0], w[1]);
        let m = second_difference_bound(&dd, 0.5 * (a + b));
        let k = pieces_for_bound(m, b - a, tol);
        for p in 1..=k {
            out.push(a + (b - a) * p as f64 / k as f64);
        }
    }
    out
}

/// A rational curve's parameters: a recognised circular arc by the chord
/// rule, anything else by a sampled second-derivative estimate per span.
fn nurbs_params(c: &truck_modeling::NurbsCurve<truck_modeling::Vector4>, range: (f64, f64), tol: f64) -> (Vec<f64>, bool) {
    let pts = c.control_points();
    if let Some(w0) = pts.first().map(|p| p.w) {
        if w0.abs() > 1e-12 && pts.iter().all(|p| (p.w - w0).abs() <= 1e-12 * w0.abs()) {
            let control_points = pts.iter().map(|p| Point3::new(p.x / p.w, p.y / p.w, p.z / p.w)).collect();
            let bs = BSplineCurve::new_unchecked(c.knot_vec().clone(), control_points);
            return (bspline_params(&bs, range, tol), true);
        }
    }
    let (t0, t1) = range;
    let at = |s: f64| point_to_vec3(c.subs(t0 + (t1 - t0) * s));
    let samples: Vec<Vec3> = (0..=8).map(|i| at(i as f64 / 8.0)).collect();
    // A circular arc: every sample on one circle.
    if let Some((centre, r, axis)) = kernel::circle_through(samples[0], samples[4], samples[8]) {
        let on = samples.iter().all(|p| ((*p - centre).length() - r).abs() <= 1e-9 * r.max(1.0) && (*p - centre).dot(axis).abs() <= 1e-9 * r.max(1.0));
        if on && r > 0.0 {
            let turn: f64 = samples
                .windows(2)
                .map(|w| {
                    let a = (w[0] - centre).normalized();
                    let b = (w[1] - centre).normalized();
                    fmath::acos(a.dot(b).clamp(-1.0, 1.0))
                })
                .sum();
            // truck's own rule for the arc, kept: halve until the sagitta is
            // under the tolerance, so the count is a power of two and the
            // sagitta lands between a quarter of the tolerance and the
            // tolerance. Not the exact chord rule (`2·acos(1 − tol/r)`
            // pieces, sagitta at the tolerance): the corpus' volume
            // assertions were written against the halving — a 3 mm peg at
            // 0.05 mm is 0.6 % under its closed form this way, 2.2 % at the
            // chord rule, past `assert_volume`'s default 1 %. And not a
            // fixed quarter of the tolerance either (`acos(1 − tol/r)`, the
            // revolved interior's rule): that is the fine end of the
            // halving's range on every arc, and a Gridfinity cup's corner
            // bands — 0.8 to 3.75 mm — took 14 to 24 triangles where the
            // halving gives 8 to 16, half as many triangles again on every
            // Gridfinity body and 60 % on the baseplate's evaluation. The
            // midpoint is a sample either way (an even count), so a
            // cylinder's box reads its diameter. The halving starts at the
            // arc's share of `MIN_PIECES_PER_TURN` (a semicircle at four, a
            // quarter at two), not at two whatever the arc: a semicircle of
            // radius under 0.17 mm at 0.05 was two pieces, its middle
            // sample a 90° corner that `straightened` dropped when the
            // rounding went one way, and a thin cylinder a flat strip.
            let sag = |k: usize| r * (1.0 - fmath::cos(0.5 * turn / k as f64));
            let mut k = pieces_for_turning(turn).max(2).next_power_of_two();
            while k < MAX_PIECES && sag(k) > tol {
                k *= 2;
            }
            // Uniform in *angle*, not in the rational parameter, so every
            // chord has the same sagitta: the angle from the start about
            // the axis is monotone in `t`, so each target is found by
            // bisection on it.
            let e0 = (samples[0] - centre).normalized();
            let e1 = axis.cross(e0).normalized();
            let angle_at = |t: f64| -> f64 {
                let d = at((t - t0) / (t1 - t0)) - centre;
                let a = fmath::atan2(d.dot(e1), d.dot(e0));
                if a < -1e-9 { a + std::f64::consts::TAU } else { a }
            };
            let sense = if angle_at(t0 + (t1 - t0) * 0.125) > std::f64::consts::PI { -1.0 } else { 1.0 };
            let unwrapped = |t: f64| -> f64 {
                let a = angle_at(t);
                if sense > 0.0 { a } else { std::f64::consts::TAU - a }
            };
            let mut out = vec![t0];
            for p in 1..k {
                let target = turn * p as f64 / k as f64;
                let (mut lo, mut hi) = (t0, t1);
                for _ in 0..40 {
                    let mid = 0.5 * (lo + hi);
                    if unwrapped(mid) < target {
                        lo = mid;
                    } else {
                        hi = mid;
                    }
                }
                out.push(0.5 * (lo + hi));
            }
            out.push(t1);
            return (out, false);
        }
    }
    // The sampled estimate, per span.
    let mut breaks = vec![t0];
    breaks.extend(interior_knots(c.knot_vec(), range));
    breaks.push(t1);
    let mut out = vec![t0];
    for w in breaks.windows(2) {
        let (a, b) = (w[0], w[1]);
        let m = (0..=8).map(|i| c.der2(a + (b - a) * i as f64 / 8.0).magnitude()).filter(|m| m.is_finite()).fold(0.0, f64::max) * 1.25;
        let k = pieces_for_bound(m, b - a, tol);
        for p in 1..=k {
            out.push(a + (b - a) * p as f64 / k as f64);
        }
    }
    (out, true)
}

/// A bound-driven division merged back while the measured sag stays under
/// `tol`: the bound proposes, the measurement disposes. The second
/// differences active on a knot span include its neighbours', so a span
/// beside an airfoil's leading edge inherits the leading edge's curvature —
/// a 1.6 mm span of the compressor wheel's blade took 31 samples under the
/// bound where its sag asks for six, and the wheel had half as many
/// triangles again. A piece is merged into the next when every sample the
/// merge skips and the merged chord's three quarter points are within
/// `tol` of the chord; the bound's own samples are what make the
/// measurement dense enough to trust, and `verified` re-checks the result.
fn thinned(curve: &Curve, samples: Vec<(f64, Point3)>, tol: f64) -> Vec<(f64, Point3)> {
    if samples.len() <= 2 {
        return samples;
    }
    let mut out = vec![samples[0]];
    let mut i = 0;
    while i < samples.len() - 1 {
        let mut j = i + 1;
        while j + 1 < samples.len() {
            let cand = j + 1;
            let (a, b) = (point_to_vec3(samples[i].1), point_to_vec3(samples[cand].1));
            let skipped_ok = (i + 1..cand).all(|k| sag_of(a, b, point_to_vec3(samples[k].1)) <= tol);
            let quarters_ok = skipped_ok
                && [0.5, 0.25, 0.75].iter().all(|&f| {
                    let t = samples[i].0 + (samples[cand].0 - samples[i].0) * f;
                    eval_curve(curve, t).is_some_and(|p| sag_of(a, b, point_to_vec3(p)) <= tol)
                });
            if quarters_ok {
                j = cand;
            } else {
                break;
            }
        }
        out.push(samples[j]);
        i = j;
    }
    out
}

/// An intersection curve's samples: the tracer polyline's vertices, refined
/// between them by a sag test with each new point projected from its
/// neighbours' surface parameters.
fn intersection_samples(
    ic: &truck_modeling::IntersectionCurve<truck_modeling::Leader, Surface>,
    range: (f64, f64),
    tol: f64,
) -> Vec<(f64, Point3)> {
    let (t0, t1) = range;
    let leader = ic.leader();
    let mut breaks = vec![t0];
    match leader {
        truck_modeling::Leader::Polyline(pl) => {
            let n = pl.len();
            for i in 1..n.saturating_sub(1) {
                let t = i as f64;
                if t > t0 + 1e-9 && t < t1 - 1e-9 {
                    breaks.push(t);
                }
            }
        }
        truck_modeling::Leader::BSpline(bs) => breaks.extend(interior_knots(bs.knot_vec(), range)),
    }
    breaks.push(t1);
    // Each breakpoint with its parameters on both surfaces.
    struct S {
        t: f64,
        p: Point3,
        uv0: (f64, f64),
        uv1: (f64, f64),
    }
    let mut samples: Vec<S> = Vec::new();
    let mut prev: Option<((f64, f64), (f64, f64))> = None;
    for &t in &breaks {
        let (h0, h1) = match prev {
            Some((a, b)) => (Some(a), Some(b)),
            None => (None, None),
        };
        let triple = double_projection(ic.surface0(), h0, ic.surface1(), h1, leader.subs(t), leader.der(t), 100)
            .or_else(|| ic.search_triple(t));
        if let Some((p, a, b)) = triple {
            prev = Some(((a.x, a.y), (b.x, b.y)));
            samples.push(S { t, p, uv0: (a.x, a.y), uv1: (b.x, b.y) });
        }
    }
    if samples.len() < 2 {
        return samples.into_iter().map(|s| (s.t, s.p)).collect();
    }
    // An intersection curve whose points lie on one circle is an arc — a
    // plane through a cylinder, a bore's rim on a plate, a STEP reader's
    // lifted circle — and takes the arc rule the edge sampler gives an
    // arc drawn as one (`nurbs_params`): halved until the sagitta is under
    // the tolerance, uniform in angle. The same circle then has the same
    // pieces whichever way it reached the tessellator, which is what the
    // STEP round trip of a translated cylinder measures (its rims come
    // back lifted onto the cylinder as 64-piece polylines; kept, they
    // were 316 triangles for the source's 124 and 3e-3 more volume).
    if samples.len() >= 5 {
        let pts: Vec<Vec3> = samples.iter().map(|s| point_to_vec3(s.p)).collect();
        let n = pts.len();
        if let Some((centre, r, axis)) = kernel::circle_through(pts[0], pts[n / 2], pts[n - 1]) {
            let eps = 1e-7 * r.max(1.0);
            let on = r > 0.0 && pts.iter().all(|p| ((*p - centre).length() - r).abs() <= eps && (*p - centre).dot(axis).abs() <= eps);
            if on {
                // The angle of every sample from the first, unwrapped and
                // monotone along the curve.
                let e0 = (pts[0] - centre).normalized();
                let e1 = axis.cross(e0).normalized();
                let mut angles: Vec<f64> = Vec::with_capacity(n);
                let mut prev = 0.0f64;
                for p in &pts {
                    let d = *p - centre;
                    let mut a = fmath::atan2(d.dot(e1), d.dot(e0));
                    while a < prev - std::f64::consts::PI {
                        a += std::f64::consts::TAU;
                    }
                    while a > prev + std::f64::consts::PI {
                        a -= std::f64::consts::TAU;
                    }
                    angles.push(a);
                    prev = a;
                }
                let turn = angles[n - 1];
                let monotone = turn.abs() > 1e-9 && angles.windows(2).all(|w| (w[1] - w[0]) * turn.signum() >= -1e-12);
                if monotone {
                    let sag = |k: usize| r * (1.0 - fmath::cos(0.5 * turn.abs() / k as f64));
                    let mut k = pieces_for_turning(turn.abs()).max(2).next_power_of_two();
                    while k < MAX_PIECES && sag(k) > tol {
                        k *= 2;
                    }
                    let mut arc: Vec<(f64, Point3)> = vec![(samples[0].t, samples[0].p)];
                    let mut ok = true;
                    let mut hint = (samples[0].uv0, samples[0].uv1);
                    for piece in 1..k {
                        let target = turn * piece as f64 / k as f64;
                        // The bracket among the samples, and t interpolated
                        // by angle within it; then one hinted projection.
                        let Some(i) = (0..n - 1).find(|&i| (angles[i] - target) * (angles[i + 1] - target) <= 0.0) else { ok = false; break };
                        let span = angles[i + 1] - angles[i];
                        let f = if span.abs() > 1e-15 { (target - angles[i]) / span } else { 0.0 };
                        let t = samples[i].t + (samples[i + 1].t - samples[i].t) * f;
                        match double_projection(ic.surface0(), Some(hint.0), ic.surface1(), Some(hint.1), leader.subs(t), leader.der(t), 100) {
                            Some((p, a, b)) => {
                                hint = ((a.x, a.y), (b.x, b.y));
                                arc.push((t, p));
                            }
                            None => {
                                ok = false;
                                break;
                            }
                        }
                    }
                    if ok {
                        let last = &samples[n - 1];
                        arc.push((last.t, last.p));
                        if kernel::trace::enabled() {
                            eprintln!(
                                "brepmesh edge: intersection curve over {:?}: {} breakpoints on one circle (r {:.4}, turn {:.4}), {} pieces by the arc rule",
                                range,
                                n,
                                r,
                                turn.to_degrees(),
                                k
                            );
                        }
                        return arc;
                    }
                }
            }
        }
    }
    // Refine each interval until the sag at the quarter points is under
    // the tolerance, projecting from the interval's left end.
    let mut out: Vec<(f64, Point3)> = Vec::new();
    for w in samples.windows(2) {
        let (a, b) = (&w[0], &w[1]);
        out.push((a.t, a.p));
        let mut stack: Vec<(f64, Point3, (f64, f64), (f64, f64), f64, Point3, usize)> = vec![(a.t, a.p, a.uv0, a.uv1, b.t, b.p, 0)];
        let mut pieces: Vec<(f64, Point3)> = Vec::new();
        while let Some((ta, pa, ua, va, tb, pb, depth)) = stack.pop() {
            let mut split = false;
            let mut mid: Option<(Point3, (f64, f64), (f64, f64))> = None;
            if depth < 8 {
                let chord = (point_to_vec3(pb) - point_to_vec3(pa)).length();
                for s in [0.5, 0.25, 0.75] {
                    let t = ta + (tb - ta) * s;
                    let Some((p, u0, u1)) = double_projection(ic.surface0(), Some(ua), ic.surface1(), Some(va), leader.subs(t), leader.der(t), 100) else {
                        continue;
                    };
                    let sag = sag_of(point_to_vec3(pa), point_to_vec3(pb), point_to_vec3(p));
                    // A projection that jumped sheets is not a sample.
                    if sag > tol && sag < 2.0 * chord + tol {
                        split = true;
                    }
                    if s == 0.5 {
                        mid = Some((p, (u0.x, u0.y), (u1.x, u1.y)));
                    }
                }
            }
            match (split, mid) {
                (true, Some((pm, um, vm))) => {
                    let tm = 0.5 * (ta + tb);
                    stack.push((tm, pm, um, vm, tb, pb, depth + 1));
                    stack.push((ta, pa, ua, va, tm, pm, depth + 1));
                }
                _ => {
                    if tb < b.t {
                        pieces.push((tb, pb));
                    }
                }
            }
        }
        pieces.sort_by(|x, y| x.0.total_cmp(&y.0));
        out.extend(pieces);
    }
    let last = samples.last().unwrap();
    out.push((last.t, last.p));
    if kernel::trace::enabled() {
        eprintln!(
            "brepmesh edge: intersection curve over {:?}: {} breakpoints, {} projected, {} samples after the sag test",
            range,
            breaks.len(),
            samples.len(),
            out.len()
        );
    }
    out
}

/// The distance from `p` to the segment `a b`.
fn sag_of(a: Vec3, b: Vec3, p: Vec3) -> f64 {
    let d = b - a;
    let l2 = d.dot(d);
    if l2 <= 0.0 {
        return (p - a).length();
    }
    let s = ((p - a).dot(d) / l2).clamp(0.0, 1.0);
    (p - (a + d * s)).length()
}

/// The parameters and points of an edge's polyline, front to back, the
/// ends included, at `tol` and with at least `min_pieces` pieces.
fn sample_edge(kc: &KCurve, tol: f64) -> Vec<(f64, Point3)> {
    let curve = kc.curve();
    let (t0, t1) = curve.range_tuple();
    if !(t1 > t0) {
        return Vec::new();
    }
    let (params, bound): (Vec<f64>, bool) = match curve {
        Curve::Line(_) => (vec![t0, t1], false),
        Curve::BSplineCurve(c) => (bspline_params(c, (t0, t1), tol), true),
        Curve::NurbsCurve(c) => nurbs_params(c, (t0, t1), tol),
        Curve::IntersectionCurve(ic) => {
            // Refined by its own hinted sag test; no `verified` pass over
            // it, because every evaluation of an intersection curve away
            // from a known neighbour is an unhinted double projection —
            // a whole-patch presearch on each surface — and three of them
            // per piece for three rounds cost more than the rest of the
            // tessellation (measured: 90 ms of a 93 ms tessellation of the
            // bracket, against truck's 24 ms).
            let mut out = intersection_samples(ic, (t0, t1), tol);
            merge_min_pieces(curve, &mut out, kc.min_pieces(), (t0, t1));
            return out;
        }
    };
    let mut samples: Vec<(f64, Point3)> = params.into_iter().filter_map(|t| eval_curve(curve, t).map(|p| (t, p))).collect();
    let n_bound = samples.len();
    if bound {
        samples = thinned(curve, samples, tol);
    }
    let n_thinned = samples.len();
    merge_min_pieces(curve, &mut samples, kc.min_pieces(), (t0, t1));
    if matches!(curve, Curve::Line(_)) {
        return samples;
    }
    // A recognised arc took its share of the per-turn minimum in the arc
    // rule; any other curve takes it here, by the angle its tangent turns.
    let arc = matches!(curve, Curve::NurbsCurve(_)) && !bound;
    if !arc {
        merge_turning_pieces(curve, &mut samples, (t0, t1));
    }
    let n_min = samples.len();
    let out = verified(curve, samples, tol, 3);
    if kernel::trace::enabled() {
        let kind = match curve {
            Curve::Line(_) => "line".to_string(),
            Curve::BSplineCurve(c) => format!("bspline deg {} knots {}", c.degree(), c.knot_vec().len()),
            Curve::NurbsCurve(c) => format!("nurbs deg {} knots {} w0 {:.4} w1 {:.4}", c.degree(), c.knot_vec().len(), c.control_points().first().map(|p| p.w).unwrap_or(0.0), c.control_points().get(1).map(|p| p.w).unwrap_or(0.0)),
            Curve::IntersectionCurve(_) => "intersection".to_string(),
        };
        eprintln!("brepmesh edge: {kind} range ({t0:.4}, {t1:.4}) bound {n_bound} thinned {n_thinned} min {n_min} (min_pieces {}) verified {}", kc.min_pieces(), out.len());
    }
    out
}

/// The normal bound's pieces (`KCurve::min_pieces`): when the
/// division has fewer pieces than the bound asks, it is *replaced* by the
/// uniform division of that many, and `verified` then restores the sag
/// where the uniform pieces are too long. Not the union of the two: on a
/// corner band whose 8° minimum (12 pieces a quarter turn) does not divide
/// the arc rule's 10, the union put 21 points on every rim and the
/// Gridfinity bodies had half as many triangles again.
fn merge_min_pieces(curve: &Curve, samples: &mut Vec<(f64, Point3)>, min_pieces: usize, (t0, t1): (f64, f64)) {
    if min_pieces <= 1 || samples.len() >= min_pieces + 1 {
        return;
    }
    let uniform: Vec<(f64, Point3)> = (0..=min_pieces)
        .map(|k| t0 + (t1 - t0) * k as f64 / min_pieces as f64)
        .filter_map(|t| eval_curve(curve, t).map(|p| (t, p)))
        .collect();
    if uniform.len() == min_pieces + 1 {
        *samples = uniform;
    }
}

/// The per-turn minimum for a curve that is not a recognised arc:
/// a curved edge is never cut into fewer pieces than
/// its tangent's turning asks for at [`crate::ksurface::MIN_PIECES_PER_TURN`]
/// a whole turn, whatever the chordal bound and the thinning left. A spline
/// profile smaller than the tolerance thinned to a piece or two an edge, and
/// a lens of two such edges revolved was a two-sided loop with no area.
/// Only an edge with fewer pieces in all than its turning asks for is
/// touched, and then only by adding samples: every piece that turns by more
/// than its share is cut into pieces of equal turning (the turning sampled
/// at 33 parameters, interpolated and inverted piecewise-linearly), so the
/// samples the bound and the thinning placed — a knot, a kink — all stay,
/// and `verified` then checks the sag as before.
fn merge_turning_pieces(curve: &Curve, samples: &mut Vec<(f64, Point3)>, (t0, t1): (f64, f64)) {
    const N: usize = 32;
    if samples.len() < 2 || !(t1 > t0) {
        return;
    }
    let n = samples.len() - 1;
    // The common case, cheaply: the tangents at the samples and at the
    // pieces' middles (2n + 1 of them, where a gear's flanks are two pieces
    // an edge) — exact for an arc, a lower bound for any curve, at the
    // resolution the division itself chose. Only an edge they find short
    // is measured on the uniform grid below.
    if 2 * n + 1 <= N + 1 {
        let mut tangents: Vec<Vec3> = Vec::with_capacity(2 * n + 1);
        tangents.push(vector_to_vec3(curve.der(samples[0].0)));
        for w in samples.windows(2) {
            tangents.push(vector_to_vec3(curve.der(0.5 * (w[0].0 + w[1].0))));
            tangents.push(vector_to_vec3(curve.der(w[1].0)));
        }
        if pieces_for_turning(crate::ksurface::turning_of(&tangents)) <= n {
            return;
        }
    }
    let ts: Vec<f64> = (0..=N).map(|i| t0 + (t1 - t0) * i as f64 / N as f64).collect();
    let tangents: Vec<Vec3> = ts.iter().map(|&t| vector_to_vec3(curve.der(t))).collect();
    let cumulative = cumulative_turning(&tangents);
    if pieces_for_turning(cumulative[N]) <= n {
        return;
    }
    // The turning from the start to `t`, and the parameter where it first
    // reaches `c`: one piecewise-linear function and its inverse.
    let turning_at = |t: f64| -> f64 {
        let x = ((t - t0) / (t1 - t0) * N as f64).clamp(0.0, N as f64);
        let i = (x.floor() as usize).min(N - 1);
        cumulative[i] + (cumulative[i + 1] - cumulative[i]) * (x - i as f64)
    };
    let parameter_at = |c: f64| -> f64 {
        let i = (0..N).find(|&i| cumulative[i + 1] >= c).unwrap_or(N - 1);
        let span = cumulative[i + 1] - cumulative[i];
        let f = if span > 0.0 { ((c - cumulative[i]) / span).clamp(0.0, 1.0) } else { 0.0 };
        ts[i] + (ts[i + 1] - ts[i]) * f
    };
    let mut out: Vec<(f64, Point3)> = Vec::with_capacity(2 * samples.len() + 8);
    for w in samples.windows(2) {
        let ((ta, pa), (tb, _)) = (w[0], w[1]);
        out.push((ta, pa));
        let (ca, cb) = (turning_at(ta), turning_at(tb));
        let m = pieces_for_turning(cb - ca);
        for j in 1..m {
            let t = parameter_at(ca + (cb - ca) * j as f64 / m as f64);
            if t > out.last().map_or(ta, |l| l.0) && t < tb {
                if let Some(p) = eval_curve(curve, t) {
                    out.push((t, p));
                }
            }
        }
    }
    out.push(*samples.last().unwrap());
    *samples = out;
}

/// Split every piece whose sag at its quarter points exceeds `tol`, up to
/// `rounds` times.
fn verified(curve: &Curve, mut samples: Vec<(f64, Point3)>, tol: f64, rounds: usize) -> Vec<(f64, Point3)> {
    for _ in 0..rounds {
        let mut out: Vec<(f64, Point3)> = Vec::with_capacity(samples.len() * 2);
        let mut changed = false;
        for w in samples.windows(2) {
            let ((ta, pa), (tb, pb)) = (w[0], w[1]);
            out.push((ta, pa));
            if out.len() > 4 * MAX_PIECES {
                continue;
            }
            let (a, b) = (point_to_vec3(pa), point_to_vec3(pb));
            let chord = (b - a).length();
            let mut worst = 0.0f64;
            let mut mid: Option<Point3> = None;
            for s in [0.5, 0.25, 0.75] {
                let t = ta + (tb - ta) * s;
                let Some(p) = eval_curve(curve, t) else { continue };
                let sag = sag_of(a, b, point_to_vec3(p));
                if sag < 2.0 * chord + tol {
                    worst = worst.max(sag);
                }
                if s == 0.5 {
                    mid = Some(p);
                }
            }
            if worst > tol {
                if let Some(pm) = mid {
                    out.push((0.5 * (ta + tb), pm));
                    changed = true;
                }
            }
        }
        if let Some(last) = samples.last() {
            out.push(*last);
        }
        samples = out;
        if !changed {
            break;
        }
    }
    samples
}

/// A polyline's interior straightened: a sample that steps backward along
/// the polyline (the direction from its predecessor opposes the direction
/// to its successor) is dropped, and so is one within [`MIN_STEP`] of its
/// predecessor or of the end. The ends are never touched.
fn straightened(points: Vec<Point3>) -> Vec<Point3> {
    let n = points.len();
    if n <= 2 {
        return points;
    }
    // One pass with the kept points as a stack: a candidate is dropped
    // when it is within `MIN_STEP` of the last kept point, or steps
    // backward from it towards the next; dropping it re-tests the next
    // against the same last kept point, which is what the one-at-a-time
    // removal did in quadratic time.
    let last = points[n - 1];
    let mut kept: Vec<Point3> = Vec::with_capacity(n);
    kept.push(points[0]);
    for i in 1..n - 1 {
        let a = point_to_vec3(*kept.last().unwrap());
        let b = point_to_vec3(points[i]);
        let c = point_to_vec3(if i + 1 < n - 1 { points[i + 1] } else { last });
        if (b - a).length() < MIN_STEP || (i + 1 == n - 1 && (c - b).length() < MIN_STEP) {
            continue;
        }
        if (b - a).dot(c - b) < 0.0 {
            continue;
        }
        kept.push(points[i]);
    }
    // A kept point that the end now steps back from is dropped too.
    while kept.len() >= 2 {
        let a = point_to_vec3(kept[kept.len() - 2]);
        let b = point_to_vec3(kept[kept.len() - 1]);
        let c = point_to_vec3(last);
        if (c - b).length() < MIN_STEP || (b - a).dot(c - b) < 0.0 {
            kept.pop();
        } else {
            break;
        }
    }
    kept.push(last);
    kept
}

// --------------------------------------------------------------- the face phase

/// A boundary point of a face in its parameter domain.
#[derive(Clone, Copy, Debug)]
struct LoopPt {
    id: VertexRef,
    uv: [f64; 2],
}

/// A vertex of a face's mesh: shared (an edge sample) or the face's own
/// (an interior grid point, a seam or pole line sample).
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
enum VertexRef {
    Global(u32),
    Local(u32),
}

struct FaceMesh {
    vertices: Vec<(VertexRef, [f64; 2], Vector3)>,
    triangles: Vec<[u32; 3]>,
    local_positions: Vec<Point3>,
}

struct FaceInput<'a> {
    index: usize,
    surface: &'a KSurface,
    orientation: bool,
    wires: &'a [Vec<EdgeUse>],
    edges: &'a [EdgeRecord],
    positions: &'a [Point3],
    tol: f64,
}

fn unit_normal(surface: &Surface, u: f64, v: f64) -> Option<Vec3> {
    let n = vector_to_vec3(surface.uder(u, v).cross(surface.vder(u, v)));
    let len = n.length();
    (len.is_finite() && len > 1e-12).then(|| n * (1.0 / len))
}

fn mindiff(x: f64, x0: f64, period: f64) -> f64 {
    let mut best = x;
    let mut d = (x - x0).abs();
    for k in [-2.0, -1.0, 1.0, 2.0] {
        let c = x + k * period;
        let dc = (c - x0).abs();
        if dc < d {
            d = dc;
            best = c;
        }
    }
    best
}

/// Inversion of a boundary point: truck's search with the nearest point
/// within the weld distance as the fallback, from the hint first and from
/// nothing if that fails.
///
/// **Inside the patch.** A polynomial patch continues past its domain, and
/// a skin through sections that change sharply overshoots enough for that
/// continuation to pass through a point the patch itself also holds. Newton
/// from a hint near a corner can walk off the domain and land there — a
/// parameter a whole span outside, whose image is the right point and whose
/// loop then fails to close. So a parameter off the domain (in a direction
/// that is not periodic) is only kept if no search finds one on it: the
/// hint, no hint, then a 5 × 5 grid of hints across the domain.
fn invert(surface: &KSurface, p: Point3, hint: Option<[f64; 2]>) -> Option<[f64; 2]> {
    let raw = surface.surface();
    let (ur, vr) = raw.try_range_tuple();
    let on = |x: f64, range: Option<(f64, f64)>, periodic: bool| -> bool {
        periodic
            || range.is_none_or(|(a, b)| {
                let m = 1e-7 * (b - a).abs().max(1e-12);
                x >= a.min(b) - m && x <= a.max(b) + m
            })
    };
    let inside = |uv: [f64; 2]| on(uv[0], ur, raw.u_period().is_some()) && on(uv[1], vr, raw.v_period().is_some());
    let search = |h: SPHint2D| -> Option<[f64; 2]> {
        let f = surface.search_parameter(p, h, 100)?;
        (f.0.is_finite() && f.1.is_finite()).then_some([f.0, f.1])
    };
    let mut off_domain = None;
    let mut first = vec![];
    if let Some(h) = hint {
        first.push(SPHint2D::Parameter(h[0], h[1]));
    }
    first.push(SPHint2D::None);
    for h in first {
        if let Some(uv) = search(h) {
            if inside(uv) {
                return Some(uv);
            }
            off_domain.get_or_insert(uv);
        }
    }
    if let (Some((u0, u1)), Some((v0, v1))) = (ur, vr) {
        for i in 0..5 {
            for j in 0..5 {
                let h = SPHint2D::Parameter(u0 + (u1 - u0) * i as f64 / 4.0, v0 + (v1 - v0) * j as f64 / 4.0);
                if let Some(uv) = search(h) {
                    if inside(uv) {
                        return Some(uv);
                    }
                }
            }
        }
    }
    off_domain
}

/// The scale of a surface: the largest coordinate magnitude over its corners.
fn surface_scale(surface: &Surface) -> f64 {
    let (Some((u0, u1)), Some((v0, v1))) = surface.try_range_tuple() else { return 1.0 };
    [(u0, v0), (u1, v0), (u0, v1), (u1, v1)]
        .into_iter()
        .map(|(u, v)| {
            let p = surface.subs(u, v);
            p.x.abs().max(p.y.abs()).max(p.z.abs())
        })
        .filter(|s| s.is_finite())
        .fold(1.0, f64::max)
}

/// The points of a straight segment in parameter space, sampled so the
/// surface along it sags under `tol` between consecutive points, ends
/// included.
fn sample_pcurve(surface: &Surface, a: [f64; 2], b: [f64; 2], tol: f64) -> Vec<[f64; 2]> {
    let at = |s: f64| [a[0] + (b[0] - a[0]) * s, a[1] + (b[1] - a[1]) * s];
    let ev = |s: f64| point_to_vec3(surface.subs(at(s)[0], at(s)[1]));
    let mut out = vec![0.0f64];
    let mut stack = vec![(0.0f64, 1.0f64, 0usize)];
    let mut pieces: Vec<f64> = Vec::new();
    while let Some((s0, s1, depth)) = stack.pop() {
        let (p0, p1) = (ev(s0), ev(s1));
        let mut split = false;
        if depth < 12 {
            for f in [0.5, 0.25, 0.75] {
                let sm = s0 + (s1 - s0) * f;
                if sag_of(p0, p1, ev(sm)) > tol {
                    split = true;
                    break;
                }
            }
        }
        if split {
            let sm = 0.5 * (s0 + s1);
            stack.push((sm, s1, depth + 1));
            stack.push((s0, sm, depth + 1));
        } else {
            pieces.push(s1);
        }
    }
    pieces.sort_by(f64::total_cmp);
    pieces.dedup();
    out.extend(pieces);
    out.into_iter().map(at).collect()
}

/// Whether the surface is (numerically) one point along the parameter
/// segment `a b`.
fn segment_is_pole(surface: &Surface, a: [f64; 2], b: [f64; 2], scale: f64) -> bool {
    let p0 = point_to_vec3(surface.subs(a[0], a[1]));
    (1..=4).all(|i| {
        let s = i as f64 / 4.0;
        let p = point_to_vec3(surface.subs(a[0] + (b[0] - a[0]) * s, a[1] + (b[1] - a[1]) * s));
        (p - p0).length() <= 1e-6 * scale
    })
}

/// One wire's image in the parameter domain, plus whether and how it is
/// open by a period.
struct LoopImage {
    pts: Vec<LoopPt>,
    /// `Some((axis, sign))` when the image ends a period away from where
    /// it started along `axis` (0 = u, 1 = v), in the direction `sign`.
    open: Option<(usize, f64)>,
}

fn build_loop(input: &FaceInput<'_>, wire: &[EdgeUse], scale: f64) -> Result<LoopImage, String> {
    let surface = input.surface;
    let raw = surface.surface();
    let (up, vp) = (raw.u_period(), raw.v_period());
    // The wire's samples, as ids, each edge without its last (the next
    // edge's first).
    let mut ids: Vec<(u32, usize, usize)> = Vec::new(); // (id, edge index, sample index)
    for use_ in wire {
        let rec = &input.edges[use_.edge as usize];
        let n = rec.ids.len();
        if n == 0 {
            continue;
        }
        if rec.degenerate {
            ids.push((rec.ids[0], use_.edge as usize, 0));
            continue;
        }
        if use_.forward {
            for k in 0..n - 1 {
                ids.push((rec.ids[k], use_.edge as usize, k));
            }
        } else {
            for k in (1..n).rev() {
                ids.push((rec.ids[k], use_.edge as usize, k));
            }
        }
    }
    if ids.is_empty() {
        return Err("the wire has no samples".into());
    }
    // Invert each in turn, hinted by the last well-conditioned parameter.
    let mut pts: Vec<LoopPt> = Vec::with_capacity(ids.len() + 4);
    let mut hint: Option<[f64; 2]> = None;
    let mut prev: Option<[f64; 2]> = None;
    let degenerate_at = |uv: [f64; 2]| -> (bool, bool) {
        let du = raw.uder(uv[0], uv[1]).magnitude();
        let dv = raw.vder(uv[0], uv[1]).magnitude();
        (!(du > DEGENERATE_DER * scale), !(dv > DEGENERATE_DER * scale))
    };
    let place = |_id: u32, p: Point3, edge: usize, sample: usize, pts: &mut Vec<LoopPt>, hint: &mut Option<[f64; 2]>, prev: &mut Option<[f64; 2]>| -> Result<[f64; 2], String> {
        let Some(mut uv) = invert(surface, p, *hint) else {
            return Err(format!(
                "sample {sample} of edge {edge}, the point ({:.4}, {:.4}, {:.4}), does not lie on the face's surface within {} mm (its parameter could not be found)",
                p.x, p.y, p.z, MIN_STEP
            ));
        };
        if let (Some(period), Some(pv)) = (up, *prev) {
            uv[0] = mindiff(uv[0], pv[0], period);
        }
        if let (Some(period), Some(pv)) = (vp, *prev) {
            uv[1] = mindiff(uv[1], pv[1], period);
        }
        // A corner where the previous point sits on a pole line and this
        // one has moved along it: the loop must run along the pole.
        if let (Some(pv), Some(last)) = (*prev, pts.last().copied()) {
            let (du0, dv0) = degenerate_at(pv);
            let eps = 1e-9;
            if du0 && (uv[0] - pv[0]).abs() > eps {
                pts.push(LoopPt { id: last.id, uv: [uv[0], pv[1]] });
            } else if dv0 && (uv[1] - pv[1]).abs() > eps {
                pts.push(LoopPt { id: last.id, uv: [pv[0], uv[1]] });
            }
        }
        let (du, dv) = degenerate_at(uv);
        if !(du || dv) {
            *hint = Some(uv);
        } else if hint.is_none() {
            *hint = Some(uv);
        }
        *prev = Some(uv);
        Ok(uv)
    };
    for &(id, edge, sample) in &ids {
        let p = input.positions[id as usize];
        let uv = place(id, p, edge, sample, &mut pts, &mut hint, &mut prev)?;
        pts.push(LoopPt { id: VertexRef::Global(id), uv });
    }
    // Close: the first point again, seen from the end of the loop.
    let (id0, edge0, sample0) = ids[0];
    let first = pts[0].uv;
    let close = place(id0, input.positions[id0 as usize], edge0, sample0, &mut pts, &mut hint, &mut prev)?;
    let du = close[0] - first[0];
    let dv = close[1] - first[1];
    // The closing point is the first point inverted again from the other
    // end of the loop, so the two differ by the inversion's residual —
    // up to about 1e-6 in parameter, more on a slow surface — and never
    // by anything a period test could confuse: a thousandth of the
    // period or of the image's extent (truck's own closure test), with
    // an absolute floor for a face whose image is tiny.
    let extent = |k: usize| (pts.iter().map(|p| p.uv[k]).fold(f64::MIN, f64::max) - pts.iter().map(|p| p.uv[k]).fold(f64::MAX, f64::min)).abs();
    let eps_u = (1e-3 * up.unwrap_or(0.0).abs().max(extent(0))).max(1e-5);
    let eps_v = (1e-3 * vp.unwrap_or(0.0).abs().max(extent(1))).max(1e-5);
    let open = if du.abs() <= eps_u && dv.abs() <= eps_v {
        None
    } else if up.is_some_and(|p| (du.abs() - p).abs() <= 1e-3 * p && dv.abs() <= eps_v) {
        Some((0, du.signum()))
    } else if vp.is_some_and(|p| (dv.abs() - p).abs() <= 1e-3 * p && du.abs() <= eps_u) {
        Some((1, dv.signum()))
    } else {
        return Err(format!(
            "the wire's image in the parameter domain does not close: it starts at (u, v) = ({:.6}, {:.6}) and returns to ({:.6}, {:.6})",
            first[0], first[1], close[0], close[1]
        ));
    };
    // A corner inserted at the closure belongs to the loop; the closing
    // point itself does not (it is the first point).
    Ok(LoopImage { pts, open })
}

/// The pole-line and seam-line points a face adds to close an open loop
/// image: the loop's own samples plus, for one open loop, the far side and
/// two seam copies, for two open loops the seam copies between them.
struct Closure {
    /// Every loop of the face as a closed polygon in parameter space.
    loops: Vec<Vec<LoopPt>>,
    /// The face's own points (seam and pole samples), by local index.
    local: Vec<Point3>,
}

fn close_loops(input: &FaceInput<'_>, mut images: Vec<LoopImage>, scale: f64) -> Result<Closure, String> {
    let raw = input.surface.surface();
    let mut local: Vec<Point3> = Vec::new();
    let open_count = images.iter().filter(|l| l.open.is_some()).count();
    let mut loops: Vec<Vec<LoopPt>> = Vec::new();
    match open_count {
        0 => {
            for l in images {
                loops.push(l.pts);
            }
        }
        1 | 2 => {
            let axis = images.iter().find_map(|l| l.open.map(|(a, _)| a)).unwrap();
            let period = if axis == 0 { raw.u_period() } else { raw.v_period() }.ok_or("an open loop on a surface without a period")?;
            if images.iter().any(|l| l.open.is_some_and(|(a, _)| a != axis)) {
                return Err("two loops wind round different periods of the surface".into());
            }
            let other = 1 - axis;
            let (Some(urange), Some(vrange)) = raw.try_range_tuple() else {
                return Err("a periodic surface with no parameter range".into());
            };
            let range_other = if other == 0 { urange } else { vrange };
            // The loop traversed in +axis is `a`.
            let ia = images.iter().position(|l| l.open.is_some_and(|(_, s)| s > 0.0));
            let ib = images.iter().position(|l| l.open.is_some_and(|(_, s)| s < 0.0));
            let (ia, ib) = match (ia, ib, open_count) {
                (Some(a), Some(b), 2) => (a, Some(b)),
                (Some(a), None, 1) => (a, None),
                (None, Some(b), 1) => (b, None),
                _ => return Err("two loops wind round the period the same way".into()),
            };
            let a_first = images[ia].pts[0].uv;
            let a_sign = images[ia].open.unwrap().1;
            let a_end = {
                let mut e = a_first;
                e[axis] += a_sign * period;
                e
            };
            // The open loop's closing point — its first vertex again, a
            // period on — is a corner of the polygon: without it the seam
            // would leave the last sample by a diagonal on one copy and
            // arrive at the first by the seam line on the other, and the
            // sliver between them would be a hole (measured: a sphere cap
            // left two unmatched edges at the seam). With it, both copies
            // of the seam end on the same global id.
            let a_pts: Vec<LoopPt> = {
                let mut v = images[ia].pts.clone();
                v.push(LoopPt { id: v[0].id, uv: a_end });
                v
            };
            // Shift every other loop into the window the open loop `a` spans.
            let a_lo = a_first[axis].min(a_end[axis]);
            for (k, l) in images.iter_mut().enumerate() {
                if k == ia {
                    continue;
                }
                let c: f64 = l.pts.iter().map(|p| p.uv[axis]).sum::<f64>() / l.pts.len() as f64;
                let shift = ((c - a_lo) / period).floor() * period;
                if shift != 0.0 {
                    for p in &mut l.pts {
                        p.uv[axis] -= shift;
                    }
                }
            }
            let sample_seam = |from: [f64; 2], to: [f64; 2], local: &mut Vec<Point3>| -> Vec<(VertexRef, [f64; 2])> {
                // Interior points only; the ends are the loops' own vertices.
                let pts = sample_pcurve(raw, from, to, input.tol);
                pts[1..pts.len() - 1]
                    .iter()
                    .map(|&uv| {
                        local.push(raw.subs(uv[0], uv[1]));
                        (VertexRef::Local((local.len() - 1) as u32), uv)
                    })
                    .collect()
            };
            match ib {
                None => {
                    // One open loop: the far side of the patch must be a pole.
                    let far = if (axis == 1 && a_sign > 0.0) || (axis == 0 && a_sign < 0.0) { range_other.0 } else { range_other.1 };
                    let mut far_end = a_end;
                    far_end[other] = far;
                    let mut far_start = a_first;
                    far_start[other] = far;
                    if !segment_is_pole(raw, far_start, far_end, scale) {
                        return Err(format!(
                            "the wire's image winds once round the surface's period and the far side of the patch ({} = {:.6}) is not a pole, so nothing closes the domain",
                            if other == 0 { "u" } else { "v" },
                            far
                        ));
                    }
                    let seam = sample_seam(a_end, far_end, &mut local);
                    let pole = raw.subs(far_end[0], far_end[1]);
                    local.push(pole);
                    let pole_id = VertexRef::Local((local.len() - 1) as u32);
                    let mut poly = a_pts.clone();
                    poly.extend(seam.iter().map(|&(id, uv)| LoopPt { id, uv }));
                    poly.push(LoopPt { id: pole_id, uv: far_end });
                    poly.push(LoopPt { id: pole_id, uv: far_start });
                    // The seam again, a period back, under the same ids.
                    poly.extend(seam.iter().rev().map(|&(id, uv)| {
                        let mut uv2 = uv;
                        uv2[axis] -= a_sign * period;
                        LoopPt { id, uv: uv2 }
                    }));
                    loops.push(poly);
                }
                Some(ib) => {
                    // Two open loops: rotate `b` so it starts nearest the
                    // seam level of `a`'s end, then join a's end to b's start
                    // and b's end to a's start with the same seam samples.
                    let b = &images[ib].pts;
                    let target = a_end[axis];
                    let start = (0..b.len())
                        .min_by(|&i, &j| (b[i].uv[axis] - target).abs().total_cmp(&(b[j].uv[axis] - target).abs()))
                        .unwrap();
                    let mut b_rot: Vec<LoopPt> = b[start..].to_vec();
                    b_rot.extend_from_slice(&b[..start]);
                    // b's image is open by -period: its closing point is its
                    // first, a period lower.
                    let b_first = b_rot[0].uv;
                    let mut b_end = b_first;
                    b_end[axis] -= period;
                    let seam = sample_seam(a_end, b_first, &mut local);
                    let mut poly = a_pts.clone();
                    poly.extend(seam.iter().map(|&(id, uv)| LoopPt { id, uv }));
                    poly.extend(b_rot.iter().copied());
                    // b's closing point, for the same reason as a's.
                    poly.push(LoopPt { id: b_rot[0].id, uv: b_end });
                    // From b's end back to a's start: the seam shifted by a
                    // period, reversed.
                    poly.extend(seam.iter().rev().map(|&(id, uv)| {
                        let mut uv2 = uv;
                        uv2[axis] -= period;
                        LoopPt { id, uv: uv2 }
                    }));
                    loops.push(poly);
                }
            }
            for (k, l) in images.into_iter().enumerate() {
                if k != ia && Some(k) != ib {
                    loops.push(l.pts);
                }
            }
        }
        n => return Err(format!("{n} wires wind round the surface's period; at most two can")),
    }
    Ok(Closure { loops, local })
}

/// Sort-and-dedup helper for a division.
fn tidy(mut d: Vec<f64>) -> Vec<f64> {
    d.retain(|x| x.is_finite());
    d.sort_by(f64::total_cmp);
    d.dedup_by(|a, b| (*a - *b).abs() <= 1e-12 * (a.abs() + b.abs() + 1.0));
    d
}

/// Insert intermediate points on every segment whose two ends share an id
/// (a pole line), at the division's lines crossing it, under the same id.
fn expand_pole_lines(loops: &mut [Vec<LoopPt>], udiv: &[f64], vdiv: &[f64]) {
    for lp in loops.iter_mut() {
        let n = lp.len();
        if n < 2 {
            continue;
        }
        let mut out: Vec<LoopPt> = Vec::with_capacity(n + 8);
        for k in 0..n {
            let a = lp[k];
            let b = lp[(k + 1) % n];
            out.push(a);
            if a.id != b.id {
                continue;
            }
            let (lo, hi) = (a.uv, b.uv);
            let axis = if (hi[0] - lo[0]).abs() >= (hi[1] - lo[1]).abs() { 0 } else { 1 };
            let div = if axis == 0 { udiv } else { vdiv };
            let (x0, x1) = (lo[axis], hi[axis]);
            let (mn, mx) = (x0.min(x1), x0.max(x1));
            let span = mx - mn;
            if span <= 0.0 {
                continue;
            }
            let mut inner: Vec<f64> = div.iter().copied().filter(|&x| x > mn + 1e-3 * span && x < mx - 1e-3 * span).collect();
            if x0 > x1 {
                inner.reverse();
            }
            for x in inner {
                let s = (x - x0) / (x1 - x0);
                let mut uv = lo;
                uv[axis] = x;
                uv[1 - axis] = lo[1 - axis] + (hi[1 - axis] - lo[1 - axis]) * s;
                out.push(LoopPt { id: a.id, uv });
            }
        }
        *lp = out;
    }
}

/// Which grid points lie inside the loops and clear of them: the crossing
/// number along each grid row, and a distance test against the boundary
/// segments in a bucket grid.
fn grid_points(loops: &[Vec<LoopPt>], udiv: &[f64], vdiv: &[f64]) -> Vec<[f64; 2]> {
    if udiv.len() < 3 || vdiv.len() < 3 {
        return Vec::new();
    }
    // Segments.
    let mut segs: Vec<([f64; 2], [f64; 2])> = Vec::new();
    for lp in loops {
        let n = lp.len();
        for k in 0..n {
            let (a, b) = (lp[k].uv, lp[(k + 1) % n].uv);
            if a != b {
                segs.push((a, b));
            }
        }
    }
    if segs.is_empty() {
        return Vec::new();
    }
    let (u0, u1) = (udiv[0], udiv[udiv.len() - 1]);
    let (v0, v1) = (vdiv[0], vdiv[vdiv.len() - 1]);
    let spacing_u: Vec<f64> = udiv.windows(2).map(|w| w[1] - w[0]).collect();
    let spacing_v: Vec<f64> = vdiv.windows(2).map(|w| w[1] - w[0]).collect();
    let local_h = |i: usize, j: usize| -> f64 {
        let hu = spacing_u[i.saturating_sub(1)].min(spacing_u[i.min(spacing_u.len() - 1)]);
        let hv = spacing_v[j.saturating_sub(1)].min(spacing_v[j.min(spacing_v.len() - 1)]);
        hu.min(hv)
    };
    let band_max = 0.05 * spacing_u.iter().chain(spacing_v.iter()).copied().fold(0.0, f64::max);
    // Buckets over the box, each segment in every bucket its box touches
    // (expanded by the widest band).
    const B: usize = 48;
    let ext_u = (u1 - u0).max(1e-300);
    let ext_v = (v1 - v0).max(1e-300);
    let bucket_of = |u: f64, v: f64| -> (usize, usize) {
        let i = (((u - u0) / ext_u) * B as f64).floor().clamp(0.0, (B - 1) as f64) as usize;
        let j = (((v - v0) / ext_v) * B as f64).floor().clamp(0.0, (B - 1) as f64) as usize;
        (i, j)
    };
    let mut buckets: Vec<Vec<u32>> = vec![Vec::new(); B * B];
    for (k, (a, b)) in segs.iter().enumerate() {
        let (lo_u, hi_u) = (a[0].min(b[0]) - band_max, a[0].max(b[0]) + band_max);
        let (lo_v, hi_v) = (a[1].min(b[1]) - band_max, a[1].max(b[1]) + band_max);
        let (i0, j0) = bucket_of(lo_u, lo_v);
        let (i1, j1) = bucket_of(hi_u, hi_v);
        for i in i0..=i1 {
            for j in j0..=j1 {
                buckets[i * B + j].push(k as u32);
            }
        }
    }
    let dist2 = |p: [f64; 2], (a, b): ([f64; 2], [f64; 2])| -> f64 {
        let d = [b[0] - a[0], b[1] - a[1]];
        let l2 = d[0] * d[0] + d[1] * d[1];
        let s = if l2 > 0.0 { (((p[0] - a[0]) * d[0] + (p[1] - a[1]) * d[1]) / l2).clamp(0.0, 1.0) } else { 0.0 };
        let q = [a[0] + d[0] * s, a[1] + d[1] * s];
        (p[0] - q[0]) * (p[0] - q[0]) + (p[1] - q[1]) * (p[1] - q[1])
    };
    let mut out = Vec::new();
    for (j, &v) in vdiv.iter().enumerate() {
        // Crossings of the row v: the u at which each segment straddling it
        // crosses (half-open in v, so a vertex on the row counts once).
        let mut xs: Vec<f64> = segs
            .iter()
            .filter(|(a, b)| (a[1] > v) != (b[1] > v))
            .map(|(a, b)| a[0] + (b[0] - a[0]) * (v - a[1]) / (b[1] - a[1]))
            .collect();
        if xs.is_empty() {
            continue;
        }
        xs.sort_by(f64::total_cmp);
        for (i, &u) in udiv.iter().enumerate() {
            let crossings = xs.partition_point(|&x| x < u);
            if crossings % 2 == 0 {
                continue;
            }
            let band = 0.05 * local_h(i, j);
            let (bi, bj) = bucket_of(u, v);
            let near = buckets[bi * B + bj].iter().any(|&k| dist2([u, v], segs[k as usize]) < band * band);
            if !near {
                out.push([u, v]);
            }
        }
    }
    out
}

/// Millimetres per unit of `u` and of `v` over the box `lo..hi`, from the
/// surface's chord lengths along five lines in each direction: the scale
/// that makes the parameter domain nearly isometric. `1` for a direction
/// the surface does not move along.
fn metric_scale(surface: &Surface, lo: [f64; 2], hi: [f64; 2]) -> (f64, f64) {
    const N: usize = 5;
    let at = |i: usize, j: usize| {
        let u = lo[0] + (hi[0] - lo[0]) * i as f64 / (N - 1) as f64;
        let v = lo[1] + (hi[1] - lo[1]) * j as f64 / (N - 1) as f64;
        point_to_vec3(surface.subs(u, v))
    };
    let pts: Vec<Vec<Vec3>> = (0..N).map(|i| (0..N).map(|j| at(i, j)).collect()).collect();
    let mut lu = 0.0;
    let mut lv = 0.0;
    for j in 0..N {
        lu += (0..N - 1).map(|i| (pts[i + 1][j] - pts[i][j]).length()).sum::<f64>();
    }
    for i in 0..N {
        lv += (0..N - 1).map(|j| (pts[i][j + 1] - pts[i][j]).length()).sum::<f64>();
    }
    let per = |length: f64, extent: f64| {
        let s = length / (N as f64) / extent;
        if s.is_finite() && s > 1e-12 { s } else { 1.0 }
    };
    (per(lu, (hi[0] - lo[0]).max(1e-300)), per(lv, (hi[1] - lo[1]).max(1e-300)))
}

/// Whether every point lies on one line (or there are fewer than three):
/// the line through the two points farthest apart, found by two sweeps.
fn collinear(points: &[Vec3]) -> bool {
    let Some(&p0) = points.first() else { return true };
    let far = |from: Vec3| points.iter().copied().max_by(|a, b| (*a - from).length().total_cmp(&(*b - from).length())).unwrap_or(from);
    let a = far(p0);
    let b = far(a);
    let d = b - a;
    let len = d.length();
    let eps = 1e-8 * (1.0 + len);
    if len <= eps {
        return true;
    }
    let dir = d * (1.0 / len);
    points.iter().all(|&p| {
        let r = p - a;
        (r - dir * r.dot(dir)).length() <= eps
    })
}

/// The most triangles one face's interior grid may ask for: about 4 GB of
/// mesh and CDT, past which a tessellation is refused rather than started
/// (docs/security.md's triangle cap, per face).
const MAX_FACE_TRIANGLES: usize = 8_000_000;

/// `x` rounded up to two significant figures, for a suggested tolerance.
fn round_up_2(x: f64) -> f64 {
    if !(x > 0.0) || !x.is_finite() {
        return x;
    }
    let p = fmath::powi(10.0, fmath::log10(x).floor() as i32 - 1);
    (x / p).ceil() * p
}

fn tessellate_face(input: &FaceInput<'_>) -> Result<FaceMesh, String> {
    let surface = input.surface;
    let raw = surface.surface();
    let scale = surface_scale(raw);
    // 0. A degenerate face — every boundary sample on one line — has no
    // area and no triangles. truck's closed `rsweep` makes one per sector
    // from a profile edge that lies on the axis (a cylinder or a sphere by
    // `revolve`): its edges are the axis line twice and two zero-length
    // arcs, all shared with the real faces around it, so nothing is open.
    let boundary: Vec<Vec3> = input
        .wires
        .iter()
        .flatten()
        .flat_map(|u| input.edges[u.edge as usize].ids.iter().map(|&id| point_to_vec3(input.positions[id as usize])))
        .collect();
    if collinear(&boundary) {
        return Ok(FaceMesh { vertices: Vec::new(), triangles: Vec::new(), local_positions: Vec::new() });
    }
    // 1. The wires' images.
    let mut images = Vec::with_capacity(input.wires.len());
    for (wi, wire) in input.wires.iter().enumerate() {
        let img = build_loop(input, wire, scale).map_err(|e| format!("wire {wi}: {e}"))?;
        images.push(img);
    }
    // 2. Closed polygons, seams and poles included.
    let Closure { mut loops, mut local } = close_loops(input, images, scale)?;
    // 3. The interior division over the loops' box.
    let mut lo = [f64::MAX; 2];
    let mut hi = [f64::MIN; 2];
    for lp in &loops {
        for p in lp {
            for k in 0..2 {
                lo[k] = lo[k].min(p.uv[k]);
                hi[k] = hi[k].max(p.uv[k]);
            }
        }
    }
    if !(lo[0].is_finite() && hi[0].is_finite() && lo[1].is_finite() && hi[1].is_finite()) {
        return Err("the boundary's image has no finite extent".into());
    }
    let (udiv, vdiv) = if hi[0] > lo[0] && hi[1] > lo[1] {
        let (u, v) = surface.grid(((lo[0], hi[0]), (lo[1], hi[1])), input.tol);
        (tidy(u), tidy(v))
    } else {
        (vec![lo[0], hi[0]], vec![lo[1], hi[1]])
    };
    // The budget: the grid is sized by the sag bound alone, and the
    // tolerance is absolute, so a large enough curved face (a sphere a
    // kilometre across at 0.05 mm) asks for billions of triangles. Refuse
    // it before building any of them.
    let cells = (udiv.len().saturating_sub(1)).saturating_mul(vdiv.len().saturating_sub(1));
    let wanted = cells.saturating_mul(2);
    if wanted > MAX_FACE_TRIANGLES {
        // Triangle count goes as 1 / tolerance (sag ∝ h², count ∝ 1 / h²).
        let fits = input.tol * wanted as f64 / MAX_FACE_TRIANGLES as f64;
        return Err(format!(
            "at a tolerance of {} mm this face needs about {} triangles, more than the {} one face may have; mesh it \
             at a coarser tolerance (roughly {} mm), or check the model's size",
            crate::context::fmt_num(input.tol),
            wanted,
            MAX_FACE_TRIANGLES,
            crate::context::fmt_num(round_up_2(fits))
        ));
    }
    expand_pole_lines(&mut loops, &udiv, &vdiv);
    let interior = grid_points(&loops, &udiv, &vdiv);
    let tracing = kernel::trace::enabled();
    if tracing {
        eprintln!(
            "brepmesh face {}: {} loops {:?}, box ({:.4}, {:.4})..({:.4}, {:.4}), grid {} x {}, interior {}",
            input.index,
            loops.len(),
            loops.iter().map(Vec::len).collect::<Vec<_>>(),
            lo[0], lo[1], hi[0], hi[1],
            udiv.len(),
            vdiv.len(),
            interior.len()
        );
    }
    // 4. The triangulation.
    for p in &interior {
        for k in 0..2 {
            lo[k] = lo[k].min(p[k]);
            hi[k] = hi[k].max(p[k]);
        }
    }
    // The metric the triangulation is Delaunay in is the division's own:
    // one grid step per unit in each direction, and a direction the
    // division leaves in one piece — a cylinder's ruling, a cone's — half a
    // unit, so a strip between two rims triangulates as the ladder. No
    // planar metric can see a chord leave the surface, and the two wrong
    // ones were both measured: in raw parameters a groove wall 2 mm tall
    // and 35 mm along its arc is a near-square whose Delaunay fanned chords
    // across ten millimetres of arc and 0.4 mm through the cone beside it;
    // in millimetres a bore's wall is 21 mm by 5.8, its
    // rim bows away from the wall in (u, v), and the empty-circle rule
    // fanned triangles across the bow whose chords ran 170° across the
    // bore (a chord error of 1.69 mm at 0.01, and `distance()` to a pin in
    // it reading 0). In steps, a chord along the curved direction is long
    // exactly when it spans many pieces, which is when it sags. A plane or
    // a bilinear patch, divided in one piece each way, keeps millimetres.
    // The map is affine, so which segments cross and what is inside are
    // untouched.
    let pieces = |d: &[f64]| d.len().saturating_sub(1).max(1);
    let (n_u, n_v) = (pieces(&udiv), pieces(&vdiv));
    let (su, sv) = if n_u > 1 || n_v > 1 {
        let per = |n: usize, extent: f64| {
            let units = if n > 1 { n as f64 } else { 0.5 };
            if extent > 1e-300 { units / extent } else { 1.0 }
        };
        (per(n_u, hi[0] - lo[0]), per(n_v, hi[1] - lo[1]))
    } else {
        metric_scale(raw, lo, hi)
    };
    let scaled = |uv: [f64; 2]| [uv[0] * su, uv[1] * sv];
    let snap = Snap::over(scaled(lo), scaled(hi));
    let total: usize = loops.iter().map(Vec::len).sum::<usize>() + interior.len();
    let mut cdt = Cdt::new(total);
    // CDT vertex -> (ref, uv)
    let mut verts: Vec<(VertexRef, [f64; 2])> = vec![(VertexRef::Local(u32::MAX), [0.0; 2]); 3];
    let mut loop_cdt: Vec<Vec<u32>> = Vec::with_capacity(loops.len());
    let mut by_point: HashMap<[i64; 2], u32> = HashMap::new();
    for lp in &loops {
        let mut ids = Vec::with_capacity(lp.len());
        for p in lp {
            let mut ip = snap.snap(scaled(p.uv));
            // Two boundary samples on one grid point: the same vertex when
            // they are the same 3-D vertex, a nudge otherwise.
            let mut tries = 0;
            let v = loop {
                match by_point.get(&ip) {
                    Some(&v) if verts[v as usize].0 == p.id => break v,
                    Some(_) => {
                        tries += 1;
                        if tries > 64 {
                            return Err(format!(
                                "two distinct boundary vertices map to one point of the parameter domain at (u, v) = ({:.6}, {:.6})",
                                p.uv[0], p.uv[1]
                            ));
                        }
                        let (dx, dy) = [(1, 0), (0, 1), (-1, 0), (0, -1), (1, 1), (-1, -1), (1, -1), (-1, 1)][tries % 8];
                        ip = [ip[0] + dx * ((tries / 8) as i64 + 1), ip[1] + dy * ((tries / 8) as i64 + 1)];
                        ip = [ip[0].clamp(0, crate::cdt::GRID), ip[1].clamp(0, crate::cdt::GRID)];
                    }
                    None => {
                        let (v, new) = cdt.insert(ip);
                        if new {
                            verts.push((p.id, p.uv));
                            by_point.insert(ip, v);
                            break v;
                        }
                        // A vertex exists there that by_point did not know
                        // (cannot happen; treat as occupied).
                        by_point.insert(ip, v);
                    }
                }
            };
            ids.push(v);
        }
        loop_cdt.push(ids);
    }
    for p in &interior {
        let ip = snap.snap(scaled(*p));
        if by_point.contains_key(&ip) {
            continue;
        }
        let (v, new) = cdt.insert(ip);
        if new {
            verts.push((VertexRef::Local(u32::MAX - 1), *p));
            by_point.insert(ip, v);
        }
    }
    // Constraints: every loop segment.
    for (li, ids) in loop_cdt.iter().enumerate() {
        let n = ids.len();
        for k in 0..n {
            let (a, b) = (ids[k], ids[(k + 1) % n]);
            if a == b {
                continue;
            }
            if let Err(e) = cdt.constrain(a, b) {
                let describe = |v: u32| -> String {
                    let (r, uv) = verts[v as usize];
                    let what = match r {
                        VertexRef::Global(id) => format!("boundary vertex {id}"),
                        VertexRef::Local(_) => "a seam or pole point".to_string(),
                    };
                    format!("{what} at (u, v) = ({:.6}, {:.6})", uv[0], uv[1])
                };
                let reason = match e {
                    CdtError::VertexOnConstraint { a, b, through } => {
                        format!("the boundary segment from {} to {} passes through {}", describe(a), describe(b), describe(through))
                    }
                    CdtError::CrossesConstraint { a, b, crossed } => format!(
                        "the boundary segment from {} to {} crosses the boundary segment from {} to {}: the boundary's image in the parameter domain intersects itself",
                        describe(a),
                        describe(b),
                        describe(crossed.0),
                        describe(crossed.1)
                    ),
                    other => other.to_string(),
                };
                return Err(format!("loop {li}: {reason}"));
            }
        }
    }
    // 5. The triangles inside, by id.
    let inside = cdt.classify();
    if tracing {
        let n_in = inside.iter().filter(|&&b| b).count();
        eprintln!("brepmesh face: cdt {} vertices, {} triangles, {} inside", cdt.vertex_count(), inside.len(), n_in);
    }
    let mut used: Vec<u32> = vec![u32::MAX; verts.len()];
    let mut vertices: Vec<(VertexRef, [f64; 2], Vector3)> = Vec::new();
    let mut triangles: Vec<[u32; 3]> = Vec::new();
    let mut interior_ids: HashMap<u32, u32> = HashMap::new();
    for (t, &inn) in inside.iter().enumerate() {
        if !inn {
            continue;
        }
        let tri = cdt.triangle(t);
        if tri.iter().any(|&v| Cdt::is_super(v)) {
            continue;
        }
        // Resolve refs: interior grid points become local positions now.
        let mut refs = [VertexRef::Local(0); 3];
        for k in 0..3 {
            let v = tri[k];
            let (r, uv) = verts[v as usize];
            refs[k] = match r {
                VertexRef::Local(x) if x == u32::MAX - 1 => {
                    let id = *interior_ids.entry(v).or_insert_with(|| {
                        local.push(raw.subs(uv[0], uv[1]));
                        (local.len() - 1) as u32
                    });
                    VertexRef::Local(id)
                }
                other => other,
            };
        }
        if refs[0] == refs[1] || refs[1] == refs[2] || refs[0] == refs[2] {
            continue; // a pole sliver
        }
        let mut out = [0u32; 3];
        for k in 0..3 {
            let v = tri[k];
            if used[v as usize] == u32::MAX {
                used[v as usize] = vertices.len() as u32;
                vertices.push((refs[k], verts[v as usize].1, Vector3::new(0.0, 0.0, 0.0)));
            }
            out[k] = used[v as usize];
        }
        if !input.orientation {
            out.swap(1, 2);
        }
        triangles.push(out);
    }
    // 6. Normals: the surface's at the vertex, a hair inside at a pole.
    let position_of = |r: VertexRef| -> Point3 {
        match r {
            VertexRef::Global(id) => input.positions[id as usize],
            VertexRef::Local(k) => local[k as usize],
        }
    };
    let mut fallback: Vec<Vec3> = vec![Vec3::ZERO; vertices.len()];
    let mut centroid_uv: Vec<[f64; 3]> = vec![[0.0; 3]; vertices.len()];
    for tri in &triangles {
        let p = [position_of(vertices[tri[0] as usize].0), position_of(vertices[tri[1] as usize].0), position_of(vertices[tri[2] as usize].0)];
        let (a, b, c) = (point_to_vec3(p[0]), point_to_vec3(p[1]), point_to_vec3(p[2]));
        let n = (b - a).cross(c - a);
        let uvs = [vertices[tri[0] as usize].1, vertices[tri[1] as usize].1, vertices[tri[2] as usize].1];
        let cu = (uvs[0][0] + uvs[1][0] + uvs[2][0]) / 3.0;
        let cv = (uvs[0][1] + uvs[1][1] + uvs[2][1]) / 3.0;
        for &k in tri {
            fallback[k as usize] = fallback[k as usize] + n;
            let c = &mut centroid_uv[k as usize];
            c[0] += cu;
            c[1] += cv;
            c[2] += 1.0;
        }
    }
    for (k, (_, uv, normal)) in vertices.iter_mut().enumerate() {
        let mut n = unit_normal(raw, uv[0], uv[1]);
        if n.is_none() && centroid_uv[k][2] > 0.0 {
            let c = [centroid_uv[k][0] / centroid_uv[k][2], centroid_uv[k][1] / centroid_uv[k][2]];
            for step in [1e-4, 1e-3, 1e-2] {
                n = unit_normal(raw, uv[0] + (c[0] - uv[0]) * step, uv[1] + (c[1] - uv[1]) * step);
                if n.is_some() {
                    break;
                }
            }
        }
        let n = n.unwrap_or_else(|| {
            let f = fallback[k];
            if f.length() > 1e-18 {
                // Triangles are already wound outward; undo that for the
                // sign convention below.
                let f = f.normalized();
                if input.orientation { f } else { -f }
            } else {
                Vec3::ZERO
            }
        });
        let n = if input.orientation { n } else { -n };
        *normal = Vector3::new(n.x, n.y, n.z);
    }
    if tracing {
        eprintln!("brepmesh face: kept {} triangles over {} vertices", triangles.len(), vertices.len());
    }
    Ok(FaceMesh { vertices, triangles, local_positions: local })
}

// ------------------------------------------------------------------ the driver

/// Tessellate `s` at `tol`: every edge once, every face against its edges'
/// shared samples, closed by construction. `Err` names the face and the
/// reason. `bounds` is what `with_cones_revolved` already applied to the
/// surfaces and edges; it is not needed here beyond the record.
pub fn tessellate(s: &TessellationSolid, tol: f64, _bounds: DivisionBounds) -> Result<MeshedSolid, TessellationError> {
    let _span = kernel::trace::Span::start(format!("brepmesh {}f tol={tol}", s.face_iter().count()));
    // -- edges, once each, in walk order
    let mut positions: Vec<Point3> = Vec::new();
    let mut vertices: Vec<u32> = Vec::new();
    let mut vmap: HashMap<VertexID<Point3>, u32> = HashMap::new();
    let mut by_position: HashMap<[i64; 3], u32> = HashMap::new();
    let mut edges: Vec<EdgeRecord> = Vec::new();
    let mut emap: HashMap<EdgeID<KCurve>, u32> = HashMap::new();
    let mut face_wires: Vec<Vec<Vec<EdgeUse>>> = Vec::new();
    let mut faces: Vec<&truck_topology::Face<Point3, KCurve, KSurface>> = Vec::new();
    {
        let _span = kernel::trace::Span::start("brepmesh edges".to_string());
        for face in s.face_iter() {
            let mut wires = Vec::new();
            for wire in face.absolute_boundaries() {
                let mut uses = Vec::with_capacity(wire.len());
                for edge in wire.edge_iter() {
                    let idx = match emap.get(&edge.id()) {
                        Some(&i) => i,
                        None => {
                            let (front, back) = (edge.absolute_front(), edge.absolute_back());
                            let mut vid = |v: &truck_topology::Vertex<Point3>| -> u32 {
                                *vmap.entry(v.id()).or_insert_with(|| {
                                    // Two topological vertices at one point
                                    // are one position: truck's closed
                                    // `rsweep` turns a vertex on the axis
                                    // into three, and the zero-length arcs
                                    // between them must be one id for the
                                    // pole fans around them to close.
                                    let p = v.point();
                                    let key = coincidence_key(p);
                                    let pid = match by_position.get(&key) {
                                        Some(&pid) if same_point(positions[pid as usize], p) => pid,
                                        _ => {
                                            positions.push(p);
                                            let pid = (positions.len() - 1) as u32;
                                            by_position.insert(key, pid);
                                            pid
                                        }
                                    };
                                    vertices.push(pid);
                                    (vertices.len() - 1) as u32
                                })
                            };
                            let (vf, vb) = (vid(front), vid(back));
                            let (pf, pb) = (positions[vertices[vf as usize] as usize], positions[vertices[vb as usize] as usize]);
                            let kc = edge.curve();
                            let kind = kernel::curve_kind(kc.curve());
                            let samples = sample_edge(&kc, tol);
                            let mut pts: Vec<Point3> = vec![pf];
                            pts.extend(samples.iter().skip(1).take(samples.len().saturating_sub(2)).map(|s| s.1));
                            pts.push(pb);
                            let pts = straightened(pts);
                            let length: f64 = pts.windows(2).map(|w| (point_to_vec3(w[1]) - point_to_vec3(w[0])).length()).sum();
                            let degenerate = vertices[vf as usize] == vertices[vb as usize] && length <= 1e-9 * (1.0 + point_to_vec3(pf).length());
                            let ids: Vec<u32> = if degenerate {
                                vec![vertices[vf as usize]]
                            } else {
                                let mut ids = vec![vertices[vf as usize]];
                                for p in &pts[1..pts.len() - 1] {
                                    positions.push(*p);
                                    ids.push((positions.len() - 1) as u32);
                                }
                                ids.push(vertices[vb as usize]);
                                ids
                            };
                            edges.push(EdgeRecord { ids, front: vf, back: vb, kind, degenerate });
                            let i = (edges.len() - 1) as u32;
                            emap.insert(edge.id(), i);
                            i
                        }
                    };
                    uses.push(EdgeUse { edge: idx, forward: edge.orientation() });
                }
                wires.push(uses);
            }
            face_wires.push(wires);
            faces.push(face);
        }
    }
    // -- faces, independently
    let surfaces: Vec<KSurface> = faces.iter().map(|f| f.surface()).collect();
    let inputs: Vec<FaceInput<'_>> = faces
        .iter()
        .enumerate()
        .map(|(fi, f)| FaceInput { index: fi, surface: &surfaces[fi], orientation: f.orientation(), wires: &face_wires[fi], edges: &edges, positions: &positions, tol })
        .collect();
    let results: Vec<Result<FaceMesh, String>> = {
        let _span = kernel::trace::Span::start(format!("brepmesh faces ({})", inputs.len()));
        run_faces(&inputs)
    };
    // -- assemble, in face order
    let _span = kernel::trace::Span::start("brepmesh assemble".to_string());
    let mut out = MeshedSolid { positions, faces: Vec::with_capacity(faces.len()), edges: Vec::new(), vertices };
    for (fi, r) in results.into_iter().enumerate() {
        let kind = match surfaces[fi].surface() {
            Surface::Plane(_) => "plane",
            Surface::RevolutedCurve(_) => "revolved",
            other => kernel::surface_kind(other),
        };
        let fm = match r {
            Ok(fm) => fm,
            Err(reason) => {
                let bbox = face_bbox(&face_wires[fi], &out.positions, &edges);
                if kernel::trace::enabled() {
                    eprintln!("brepmesh: face {fi} ({kind}) refused: {reason}");
                    for (wi, wire) in face_wires[fi].iter().enumerate() {
                        for u in wire {
                            let e = &edges[u.edge as usize];
                            let (pf, pb) = (out.positions[e.ids[0] as usize], out.positions[*e.ids.last().unwrap() as usize]);
                            eprintln!(
                                "  wire {wi}: edge {} {} {} samples{} ({:.4}, {:.4}, {:.4}) -> ({:.4}, {:.4}, {:.4}) front v{} back v{}",
                                u.edge,
                                if u.forward { "fwd" } else { "rev" },
                                e.ids.len(),
                                if e.degenerate { " degenerate" } else { "" },
                                pf.x, pf.y, pf.z, pb.x, pb.y, pb.z, e.front, e.back
                            );
                        }
                    }
                }
                return Err(TessellationError { face: fi, kind, bbox, reason });
            }
        };
        let base = out.positions.len() as u32;
        out.positions.extend_from_slice(&fm.local_positions);
        let vertices = fm
            .vertices
            .into_iter()
            .map(|(r, uv, normal)| FaceVertex {
                id: match r {
                    VertexRef::Global(id) => id,
                    VertexRef::Local(k) => base + k,
                },
                uv,
                normal,
            })
            .collect();
        out.faces.push(MeshedFace { orientation: faces[fi].orientation(), kind, vertices, triangles: fm.triangles, wires: face_wires[fi].clone() });
    }
    out.edges = edges.into_iter().map(|e| MeshedEdge { ids: e.ids, front: e.front, back: e.back, kind: e.kind, degenerate: e.degenerate }).collect();
    Ok(out)
}

/// A key that two coincident points share (up to rounding at the cell's
/// edge, which `same_point` settles).
fn coincidence_key(p: Point3) -> [i64; 3] {
    let q = |x: f64| (x * 1e6).round() as i64;
    [q(p.x), q(p.y), q(p.z)]
}

fn same_point(a: Point3, b: Point3) -> bool {
    (point_to_vec3(a) - point_to_vec3(b)).length() <= 1e-9 * (1.0 + point_to_vec3(a).length())
}

/// The box of a face's boundary samples, for the diagnostic.
fn face_bbox(wires: &[Vec<EdgeUse>], positions: &[Point3], edges: &[EdgeRecord]) -> Option<(Vec3, Vec3)> {
    let mut lo = Vec3::new(f64::MAX, f64::MAX, f64::MAX);
    let mut hi = Vec3::new(f64::MIN, f64::MIN, f64::MIN);
    let mut any = false;
    for use_ in wires.iter().flatten() {
        for &id in &edges[use_.edge as usize].ids {
            let p = point_to_vec3(positions[id as usize]);
            lo = lo.min(p);
            hi = hi.max(p);
            any = true;
        }
    }
    any.then_some((lo, hi))
}

#[cfg(not(target_arch = "wasm32"))]
fn run_faces(inputs: &[FaceInput<'_>]) -> Vec<Result<FaceMesh, String>> {
    use std::sync::atomic::{AtomicUsize, Ordering};
    use std::sync::Mutex;
    let n = inputs.len();
    // Up to sixteen threads, one per face when there are fewer. A body of
    // under eight faces that are all planes or ruled by lines — a box, a
    // peg — runs serially: its faces cost less than the spawns (a box's
    // six threads were 0.3 ms against 0.1 ms of triangulation, a fifth of
    // the nightstand's evaluation). A thread per eight faces was tried for
    // the same case and cost the chess rook two thirds of its time: one
    // curved face is worth far more than a spawn, so a body with any is
    // spread out whatever its face count.
    let threads = std::thread::available_parallelism().map(|t| t.get()).unwrap_or(1).min(n.max(1)).min(16);
    let cheap = |f: &FaceInput<'_>| match f.surface.surface() {
        Surface::Plane(_) => true,
        Surface::RevolutedCurve(p) => matches!(p.entity().entity_curve(), Curve::Line(_)),
        _ => false,
    };
    if threads <= 1 || n < 4 || (n < 8 && inputs.iter().all(cheap)) {
        return inputs.iter().map(tessellate_face).collect();
    }
    let next = AtomicUsize::new(0);
    let results: Mutex<Vec<Option<Result<FaceMesh, String>>>> = Mutex::new((0..n).map(|_| None).collect());
    // A caller inside `kernel::guarded` catches a face's panic and reports
    // it; the panic happens on these threads, so they are quiet too.
    let quiet = crate::kernel::is_quiet();
    std::thread::scope(|scope| {
        for _ in 0..threads {
            scope.spawn(|| {
                let work = || loop {
                    let i = next.fetch_add(1, Ordering::Relaxed);
                    if i >= n {
                        break;
                    }
                    let r = tessellate_face(&inputs[i]);
                    results.lock().unwrap()[i] = Some(r);
                };
                if quiet { crate::kernel::quietly(work) } else { work() }
            });
        }
    });
    results.into_inner().unwrap().into_iter().map(|r| r.unwrap_or_else(|| Err("not tessellated".into()))).collect()
}

#[cfg(target_arch = "wasm32")]
fn run_faces(inputs: &[FaceInput<'_>]) -> Vec<Result<FaceMesh, String>> {
    inputs.iter().map(tessellate_face).collect()
}

#[cfg(test)]
mod tests {
    //! Thin parts: a valid solid whose curved parts are smaller than the
    //! tolerance — or a normal part at an absurdly coarse one — still
    //! meshes as a closed, outward, positive-volume shell. The chord rule
    //! alone cuts a semicircle under 0.17 mm at 0.05 into two pieces, which
    //! can leave a cylinder flat, open or refused.

    use super::*;
    use crate::curve::Curve as Curve3;
    use crate::geom::{Plane, Vec3};
    use crate::kernel::KSolid;
    use crate::sketch::{Loop, Segment, Sketch, P2};
    use std::f64::consts::{PI, TAU};

    /// A sphere 10 km across at 0.05 mm would need billions of triangles;
    /// the tessellator used to start building them and run out of memory
    /// minutes later. It refuses at once, naming the face, the count and
    /// the way out, and a sphere of ordinary size is untouched.
    #[test]
    fn a_face_past_the_triangle_budget_is_refused_by_name() {
        let huge = crate::primitives::sphere(Vec3::ZERO, 1e7).unwrap();
        let t0 = std::time::Instant::now();
        let err = match crate::kernel::tessellate(&huge, 0.05) {
            Ok(m) => panic!("meshed {} faces", m.faces.len()),
            Err(e) => e.to_string(),
        };
        assert!(t0.elapsed().as_secs_f64() < 20.0, "took {:?}", t0.elapsed());
        assert!(err.contains("triangles") && err.contains("tolerance"), "{err}");
        let fine = crate::primitives::sphere(Vec3::ZERO, 10.0).unwrap();
        assert!(crate::kernel::tessellate(&fine, 0.05).is_ok());
    }

    /// Mesh `solid` at `tol` and hold it to what the tessellator promises
    /// any valid solid: closed on the ids with no weld, and as a mesh a
    /// closed, outward, positive-volume shell that nothing crosses. The
    /// volume, for the caller's shape check.
    fn solid_volume(what: &str, solid: &KSolid, tol: f64) -> f64 {
        checked_volume(what, solid, tol).unwrap_or_else(|e| panic!("{e}"))
    }

    /// [`solid_volume`] as a `Result`, for a test that lists every failure.
    fn checked_volume(what: &str, solid: &KSolid, tol: f64) -> Result<f64, String> {
        let meshed = kernel::tessellate(solid, tol).map_err(|e| format!("{what} at {tol}: {e}"))?;
        if meshed.unmatched_edges().0 != 0 {
            return Err(format!("{what} at {tol}: open on the ids"));
        }
        let (mesh, _) = crate::tessellate::tessellate(solid, tol).map_err(|e| format!("{what} at {tol}: {e}"))?;
        let report = crate::solid::validate(&mesh);
        if !report.oriented() {
            return Err(format!("{what} at {tol}: not a closed, outward shell: {}", report.summary()));
        }
        if !report.is_solid() || !(report.volume > 0.0) {
            return Err(format!("{what} at {tol}: {} (volume {})", report.summary(), report.volume));
        }
        Ok(report.volume)
    }

    fn cylinder(r: f64, h: f64, at: Vec3) -> KSolid {
        let sketch = Sketch::new(Plane::XY.offset(at.z)).with_loop(Loop::circle(P2::new(at.x, at.y), r));
        kernel::extrude(&sketch, h).unwrap().remove(0)
    }

    fn revolved(profile: Loop) -> KSolid {
        kernel::revolve(&Sketch::new(Plane::XZ).with_loop(profile), Vec3::ZERO, Vec3::Z, TAU).unwrap().remove(0)
    }

    #[test]
    fn a_whole_turn_is_never_fewer_than_eight_pieces() {
        assert_eq!(pieces_for_turning(TAU), 8);
        assert_eq!(pieces_for_turning(PI), 4);
        assert_eq!(pieces_for_turning(PI / 2.0), 2);
        assert_eq!(pieces_for_turning(TAU / 3.0), 3);
        assert_eq!(pieces_for_turning(PI / 2.0 * (1.0 + 1e-12)), 2, "a rounded quarter turn is still two pieces");
        assert_eq!(pieces_for_turning(0.1), 1);
        assert_eq!(pieces_for_turning(0.0), 1);
        assert_eq!(pieces_for_turning(f64::NAN), 1);
        // A semicircle of 0.03 mm at 0.05: the chord rule's two pieces
        // become four, uniform in angle, and none of the five samples is
        // dropped on the way to the edge table.
        let wire = kernel::loop_to_wire(&Loop::circle(P2::new(0.0, 0.0), 0.03), &Plane::XY).unwrap();
        for edge in wire.edge_iter() {
            let samples = sample_edge(&KCurve::new(edge.curve(), 1), 0.05);
            assert_eq!(samples.len(), 5, "a semicircle of 0.03 mm at 0.05");
            let pts = straightened(samples.iter().map(|s| s.1).collect());
            assert_eq!(pts.len(), 5, "straightened kept every sample");
        }
    }

    /// Cylinders of 0.01 to 0.08 mm radius, a few tenths tall, at the
    /// default tolerance and around it: standing in several places, lying
    /// down, and as bars along Y centred on the X axis — where the 90°
    /// corner fell depended on the rounding of the coordinates, and the
    /// three together meet all three of the failures (of these 96 at 0.05
    /// without the floor: 37 flat, 5 refused, 1 open, and the other 53
    /// squares, solids a third short of the octagon's volume floor). Every
    /// failure is listed.
    #[test]
    fn a_cylinder_thinner_than_the_tolerance_is_a_solid() {
        let places = [Vec3::new(0.0, 0.0, 0.0), Vec3::new(1.3, 0.0, 0.0), Vec3::new(123.4, -7.7, 2.25), Vec3::new(-5.5, 3.3, 1.1)];
        let turned = |s: &KSolid, about: Vec3| crate::primitives::transformed(s, &crate::geom::Transform::rotation(about, Vec3::X, PI / 2.0));
        let mut failures: Vec<String> = Vec::new();
        let mut cases = 0;
        for r in [0.01, 0.02, 0.029, 0.03, 0.04, 0.05, 0.06, 0.08] {
            let mut bodies: Vec<(String, KSolid, f64)> = Vec::new();
            for at in places {
                let standing = cylinder(r, 0.3, at);
                bodies.push((format!("cylinder r {r} at {at:?}, lying"), turned(&standing, at), 0.3));
                bodies.push((format!("cylinder r {r} at {at:?}"), standing, 0.3));
            }
            for (x, len) in [(1.0, 0.6), (0.0, 0.6), (2.5, 0.4), (-3.25, 1.0)] {
                let bar = turned(&cylinder(r, len, Vec3::new(x, 0.0, -0.5 * len)), Vec3::new(x, 0.0, 0.0));
                bodies.push((format!("bar r {r} at x {x}, {len} long"), bar, len));
            }
            for (what, solid, h) in &bodies {
                for tol in [0.1, 0.05, 0.01] {
                    cases += 1;
                    match checked_volume(what, solid, tol) {
                        // At least the octagon: 2·√2·r² of the circle's π·r².
                        Ok(v) if v > 0.89 * PI * r * r * h && v < PI * r * r * h * (1.0 + 1e-9) => {}
                        Ok(v) => failures.push(format!("{what} at {tol}: volume {v} of {}", PI * r * r * h)),
                        Err(e) => failures.push(e),
                    }
                }
            }
        }
        assert!(failures.is_empty(), "{} of {cases} thin cylinders failed:\n{}", failures.len(), failures.join("\n"));
    }

    /// A normal part at a tolerance larger than its radius: the same failure
    /// by the same mechanism (a semicircle of 3 to 15 mm at 5 is two pieces
    /// by the chord rule), and at 5 the radii that happened to fall the bad
    /// way were 3, 6, 12, 13 and 15.
    #[test]
    fn a_coarse_tolerance_does_not_flatten_a_cylinder() {
        for r in [3.0, 6.0, 10.0, 12.0, 13.0, 15.0] {
            let solid = cylinder(r, 30.0, Vec3::new(40.0 * r, 0.0, 0.0));
            for tol in [2.0, 5.0, 10.0] {
                let v = solid_volume(&format!("cylinder r {r}"), &solid, tol);
                let exact = PI * r * r * 30.0;
                assert!(v > 0.89 * exact, "cylinder r {r} at {tol}: volume {v} of {exact}");
            }
        }
    }

    /// Small revolved parts: a knob on the axis, a ring off it, a tube
    /// (a circle profile), a lens of two Béziers — the one a turning-blind
    /// division thinned to a two-sided loop with no area, on either
    /// tessellator — and the torus and sphere primitives; then the same
    /// shapes at a normal size and a tolerance of 5.
    #[test]
    fn a_small_revolved_part_is_a_solid() {
        let polygon = |pts: &[(f64, f64)]| Loop::polygon(&pts.iter().map(|&(x, y)| P2::new(x, y)).collect::<Vec<_>>());
        let lens = |x0: f64, w: f64, h: f64| Loop {
            segments: vec![
                Segment::Bezier { a: P2::new(x0, 0.0), b: P2::new(x0 + w, 0.0), ctrl: vec![P2::new(x0, h), P2::new(x0 + w, h)] },
                Segment::Bezier { a: P2::new(x0 + w, 0.0), b: P2::new(x0, 0.0), ctrl: vec![P2::new(x0 + w, -h), P2::new(x0, -h)] },
            ],
        };
        for (scale, tol) in [(1.0, 0.05), (1.0, 0.01), (250.0, 5.0)] {
            let s = |x: f64| x * scale;
            let parts: Vec<(&str, KSolid)> = vec![
                ("knob", revolved(polygon(&[(0.0, 0.0), (s(0.03), 0.0), (s(0.03), s(0.2)), (0.0, s(0.2))]))),
                ("ring", revolved(polygon(&[(s(0.02), 0.0), (s(0.04), 0.0), (s(0.04), s(0.1)), (s(0.02), s(0.1))]))),
                ("tube", revolved(Loop::circle(P2::new(s(0.1), 0.0), s(0.02)))),
                ("lens", revolved(lens(s(0.1), s(0.04), s(0.03)))),
                ("torus", crate::primitives::torus(Vec3::ZERO, Vec3::Z, s(0.1), s(0.02)).unwrap()),
                ("sphere", crate::primitives::sphere(Vec3::ZERO, s(0.03)).unwrap()),
            ];
            for (what, solid) in &parts {
                solid_volume(&format!("{what} at scale {scale}"), solid, tol);
            }
        }
    }

    /// A thin tube swept along a line, an arc and a line — a bent rod —
    /// and the same tube at a normal size at a tolerance of 5.
    #[test]
    fn a_thin_swept_tube_is_a_solid() {
        for (scale, tol) in [(0.01, 0.05), (1.0, 5.0)] {
            let s = |x: f64| x * scale;
            let q = std::f64::consts::FRAC_1_SQRT_2;
            let route = Curve3::new(Vec3::ZERO)
                .line_to(Vec3::new(0.0, 0.0, s(40.0)))
                .and_then(|c| c.arc_to(Vec3::new(s(20.0), 0.0, s(60.0)), Vec3::new(s(20.0 - 20.0 * q), 0.0, s(40.0 + 20.0 * q))))
                .and_then(|c| c.line_to(Vec3::new(s(60.0), 0.0, s(60.0))))
                .unwrap();
            let profile = Sketch::new(Plane::XY).with_loop(Loop::circle(P2::new(0.0, 0.0), s(3.0)));
            let solid = kernel::sweep_along_wire(&profile, &route, crate::sweep::Frame::Rmf).unwrap().remove(0);
            solid_volume(&format!("tube at scale {scale}"), &solid, tol);
        }
    }
}
