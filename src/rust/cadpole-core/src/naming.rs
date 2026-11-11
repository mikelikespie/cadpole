//! Generator-based topological naming (docs/design.md).
//!
//! A face is named from *how it was made* — the sketch loop and segment it was
//! swept from, the cap it is, the source face a boolean kept it from — never
//! from an index into the kernel's output order. Edges are named by the pair of
//! faces they bound, vertices by the faces that meet there.
//!
//! Nothing in this module touches truck or manifold: the namer sees points,
//! normals and areas ([`FaceProbe`]), plus a description of what the feature
//! did ([`Generator`]) or of the surfaces it inherits from
//! ([`SurfaceCatalogue`]). That is what makes it testable without a kernel, and
//! it is also why the two kernels agree on names: they hand the same shapes to
//! the same code.
//!
//! The naming grammar is in docs/design.md and is restated nowhere. What is
//! implemented here:
//!
//! ```text
//! ref      := feature "/" local
//! local    := "start" | "end" | "top" | "bottom" | "front" | "back"
//!          |  "left" | "right" | "side"
//!          |  "surface" | "outer" | "inner"  # sphere, torus
//!          |  "side[" l "." s "]"          # extrude and loft, loop.segment
//!          |  "seg[" l "." s "]"           # revolve, loop.segment
//!          |  "cut"                        # the face split() left behind
//!          |  "face[" edge "]"             # a chamfer's face, after the edge it
//!          |                               #   replaced; `leg`/`cap` on the
//!          |                               #   cutter alone, which never survive
//!          |  "face[" n "]"                # unmatched fallback, with a warning
//!          |  local "#" k                  # k-th piece, canonical order
//! edge     := ref "|" ref                  # canonically ordered face pair,
//!          |  edge "@" k                   #   shared feature prefix written once
//! vertex   := ref "|" ref "|" ref          # the faces that meet there
//!          |  vertex "@" k
//! ```
//!
//! `face:N` / `edge:N` / `vertex:N` (kernel output order) are *not* part of the
//! grammar; [`resolve`] still accepts them as an addressing form, because the
//! web UI, the MCP tools and existing scripts use them.

use std::collections::{BTreeMap, HashMap};

use crate::curve::Curve;
use crate::geom::{Plane, Vec3};
use crate::loft::{Cap, Layout, LoftPlan, PlannedSection};
use crate::sketch::{Segment, Sketch, arc_params, bezier_point};
use crate::surface_class::SurfaceClass;
use crate::fmath;

/// Angular slop when deciding whether a face is a cap, in degrees.
const CAP_ANGLE_DEG: f64 = 1.0;
/// Share of a result face's samples a *second* source must own before its name
/// is carried over too. This is what makes a merged face carry the union of
/// its sources' names (the `merged` role): two coplanar faces that a
/// union fused into one each own about half, and neither can reach the 50 %
/// the primary match asks for.
const ALIAS_COVERAGE: f64 = 0.2;
/// Share the *best* source must own before it names the face at all. Below it
/// the face lies on no surface either operand contributed — a genuinely new
/// surface — and gets `<feature>/face[n]` plus a warning.
const MATCH_COVERAGE: f64 = 0.2;

/// What kind of element a name addresses.
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub enum Kind {
    Face,
    Edge,
    Vertex,
}

impl Kind {
    /// The kernel-order index form's prefix: `face:`, `edge:`, `vertex:`.
    pub fn index_prefix(self) -> &'static str {
        match self {
            Kind::Face => "face:",
            Kind::Edge => "edge:",
            Kind::Vertex => "vertex:",
        }
    }

    /// `face:3` and friends — the addressing form kept for compatibility.
    pub fn index_name(self, i: usize) -> String {
        format!("{}{i}", self.index_prefix())
    }

    /// The index `s` addresses, when it is in the index form.
    pub fn parse_index(self, s: &str) -> Option<usize> {
        s.strip_prefix(self.index_prefix())?.parse().ok()
    }
}

/// `<feature>/<local>`, the full form of every generated name.
pub fn full_name(feature: &str, local: &str) -> String {
    if local.contains('/') { local.to_owned() } else { format!("{feature}/{local}") }
}

/// The feature part of a full name, or `""`.
pub fn feature_of(name: &str) -> &str {
    name.split_once('/').map(|(f, _)| f).unwrap_or("")
}

/// The local part of a full name (everything after the first `/`).
pub fn local_of(name: &str) -> &str {
    name.split_once('/').map(|(_, l)| l).unwrap_or(name)
}

/// Join face names into an edge or vertex name: sorted, deduped, and with the
/// feature prefix written once when every part shares it
/// (`lid/end|side[0.2]`, but `block/end|hole/side[0.0]`).
pub fn join_names(parts: &[String]) -> String {
    let mut p: Vec<&str> = parts.iter().map(String::as_str).collect();
    p.sort_unstable();
    p.dedup();
    if p.is_empty() {
        return String::new();
    }
    let prefix = feature_of(p[0]);
    let shared = !prefix.is_empty() && p.iter().all(|n| feature_of(n) == prefix);
    if shared && p.len() > 1 {
        let mut out = p[0].to_owned();
        for n in &p[1..] {
            out.push('|');
            out.push_str(local_of(n));
        }
        out
    } else {
        p.join("|")
    }
}

/// An edge name: the canonically ordered pair of faces it bounds. A seam that
/// a single face meets itself along keeps the pair shape (`lid/side|side`).
pub fn edge_name(faces: &[String]) -> String {
    let mut p: Vec<&str> = faces.iter().map(String::as_str).collect();
    p.sort_unstable();
    p.dedup();
    match p.len() {
        0 => String::new(),
        1 => format!("{}|{}", p[0], local_of(p[0])),
        _ => join_names(faces),
    }
}

// ---------------------------------------------------------------- probes

/// A face as the namer sees it. Built from the tessellation, but carrying
/// nothing kernel-specific, so tests can write one by hand.
#[derive(Clone, Debug, Default, PartialEq)]
pub struct FaceProbe {
    pub surface_kind: &'static str,
    pub centroid: Vec3,
    pub normal: Vec3,
    pub area: f64,
    /// Points on the face — triangle centroids, in practice. The namer never
    /// assumes how many there are, only that they are on the face.
    pub samples: Vec<Vec3>,
}

/// The name a face ends up with and how it got there.
#[derive(Clone, Debug, PartialEq)]
pub struct NamedFace {
    /// Full name, `<feature>/<local>`.
    pub name: String,
    /// Other names that also resolve to this face: the generic form of a
    /// primitive's face (`box_1/end` next to `box_1/top`), or the other
    /// sources of a merged face.
    pub aliases: Vec<String>,
    pub role: Role,
}

impl NamedFace {
    pub fn new(name: impl Into<String>, role: Role) -> NamedFace {
        NamedFace { name: name.into(), aliases: Vec::new(), role }
    }
    pub fn with_alias(mut self, a: impl Into<String>) -> NamedFace {
        let a = a.into();
        if a != self.name && !self.aliases.contains(&a) {
            self.aliases.push(a);
        }
        self
    }
}

/// The element map's role column (docs/design.md).
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub enum Role {
    /// Created by this feature's generator.
    New,
    /// One source face, most of its area.
    Kept,
    /// Several sources: the name is the union.
    Merged,
    /// Matched nothing; the name is the `face[n]` fallback.
    Unmatched,
}

// ---------------------------------------------------------------- ordering

/// Canonical in-surface frame for a normal: the world axis least aligned with
/// `n` (ties broken X before Y before Z), orthogonalised. `n` is sign-
/// normalised first so a flipped face gives the same frame.
pub fn canonical_frame(n: Vec3) -> (Vec3, Vec3) {
    let n = sign_normalize(n.normalized());
    let axes = [Vec3::X, Vec3::Y, Vec3::Z];
    let mut best = 0usize;
    for (i, a) in axes.iter().enumerate() {
        if n.dot(*a).abs() < n.dot(axes[best]).abs() {
            best = i;
        }
    }
    let a = axes[best];
    let u = (a - n * a.dot(n)).normalized();
    (u, n.cross(u))
}

/// Flip a direction so that its first significant component is positive.
fn sign_normalize(n: Vec3) -> Vec3 {
    for c in [n.x, n.y, n.z] {
        if c > 1e-9 {
            return n;
        }
        if c < -1e-9 {
            return -n;
        }
    }
    n
}

/// Coordinates are compared at micron resolution, so two values that are
/// nominally equal cannot swap places on float noise. `select`'s result order
/// goes through this one function; the canonical orders of split pieces and
/// coincident edges use [`gapped_order`], which is this idea made robust to
/// noise larger than a micron.
pub fn quantize(v: f64) -> i64 {
    (v / 1e-6).round() as i64
}

fn q(v: f64) -> i64 {
    quantize(v)
}

/// The tie floor for a group of things being ordered whose world extent is
/// `extent`: two key values closer than this are the same value. Ten
/// microns, or a thousandth of the extent when that is more -- three orders
/// of magnitude above the last-bit noise of any arithmetic, and above the
/// half-millimetre a face's centroid can move when a different platform
/// gives it five more triangles, while
/// staying well under the separation of any two pieces a script could tell
/// apart.
pub fn tie_floor(extent: f64) -> f64 {
    (1e-3 * extent).max(10e-6)
}

/// Bounding-box diagonal of a set of points: the extent [`tie_floor`] scales
/// with.
fn extent_of(points: impl Iterator<Item = Vec3>) -> f64 {
    let mut lo = Vec3::new(f64::MAX, f64::MAX, f64::MAX);
    let mut hi = Vec3::new(f64::MIN, f64::MIN, f64::MIN);
    let mut any = false;
    for p in points {
        lo = lo.min(p);
        hi = hi.max(p);
        any = true;
    }
    if any { (hi - lo).length() } else { 0.0 }
}

/// A total order on `0..n` by successive float keys, in which two key values
/// less than `floor` apart are one value.
///
/// Sort by the first key; walk the sorted list and cut it wherever two
/// neighbours are more than `floor` apart; every run between cuts is a tie
/// on that key and is ordered by the next key the same way; a run that is
/// still tied after the last key keeps the order it was given in. The
/// result is deterministic, independent of the order the kernel emitted the
/// items in, and can be perturbed by noise only where a gap between two
/// neighbouring key values is itself within the noise of `floor` -- which is
/// why the floor sits three orders of magnitude above any arithmetic noise
/// and one above tessellation noise. Comparing quantised values instead
/// (`round(v / grid)`) has a boundary every `grid`, so a one-ulp change
/// flips an order whenever a value sits on one; a gap-based tie has no
/// boundaries in the interior of a cluster.
pub fn gapped_order(n: usize, keys: &[&dyn Fn(usize) -> f64], floor: f64) -> Vec<usize> {
    let mut idx: Vec<usize> = (0..n).collect();
    gapped_sort(&mut idx, keys, floor);
    idx
}

fn gapped_sort(idx: &mut [usize], keys: &[&dyn Fn(usize) -> f64], floor: f64) {
    if idx.len() < 2 {
        return;
    }
    let Some((key, rest)) = keys.split_first() else {
        // Tied on every key: the order the items were given in, which is
        // the one thing here no float can move.
        idx.sort_unstable();
        return;
    };
    idx.sort_by(|&a, &b| key(a).total_cmp(&key(b)));
    let mut start = 0;
    for i in 1..=idx.len() {
        let cut = i == idx.len() || key(idx[i]) - key(idx[i - 1]) > floor;
        if cut {
            if i - start > 1 {
                gapped_sort(&mut idx[start..i], rest, floor);
            }
            start = i;
        }
    }
}

/// Largest minus smallest of `key` over `0..n`: how much a component can
/// tell the group apart by.
fn spread(n: usize, key: &dyn Fn(usize) -> f64) -> f64 {
    let mut lo = f64::INFINITY;
    let mut hi = f64::NEG_INFINITY;
    for i in 0..n {
        let v = key(i);
        lo = lo.min(v);
        hi = hi.max(v);
    }
    if n == 0 { 0.0 } else { hi - lo }
}

/// The order in which to consult `components`: by their [`spread`] over
/// the group, largest first, spreads within `floor` of each other keeping
/// the order given. A component the group agrees on to within the
/// noise -- the symmetric axis of a face split in two, whose centroids are
/// nominally equal there -- is then never the one that decides, whatever
/// the noise did to it; the component that separates the pieces by
/// millimetres is.
fn by_spread(n: usize, components: &[&dyn Fn(usize) -> f64], floor: f64) -> Vec<usize> {
    let spreads: Vec<f64> = components.iter().map(|c| spread(n, *c)).collect();
    gapped_order(components.len(), &[&|k| -spreads[k]], floor)
}

/// The canonical order of the pieces a name was split into. Each
/// piece is `(centroid, normal, area)`; the normal is not consulted.
///
/// The rule: the world-space centroid's `x`, `y`, `z`, consulted in
/// order of their spread across the group, largest first ([`by_spread`]);
/// then the larger area; then the order the pieces were given in. Every
/// comparison is [`gapped_order`] at the group's [`tie_floor`]: values
/// closer than the floor are one value and fall through to the next key.
/// So two halves of a cylinder are `#0` on the side of the axis that
/// separates them where the coordinate is lower, three pieces around an
/// axis go by the axis they spread most along, and a component the group
/// agrees on to within the noise never decides. Deterministic, independent
/// of the order the kernel emitted the pieces in, and unmoved by noise
/// smaller than the separation that actually distinguishes the pieces.
///
/// World axes are used rather than a frame of the group's dominant normal,
/// because such a frame needs a seed piece, and among equal pieces "the
/// largest" is decided by ulps of area.
pub fn order_pieces(pieces: &[(Vec3, Vec3, f64)]) -> Vec<usize> {
    let n = pieces.len();
    if n < 2 {
        return (0..n).collect();
    }
    let floor = tie_floor(extent_of(pieces.iter().map(|p| p.0)));
    let x = |i: usize| pieces[i].0.x;
    let y = |i: usize| pieces[i].0.y;
    let z = |i: usize| pieces[i].0.z;
    let axes: [&dyn Fn(usize) -> f64; 3] = [&x, &y, &z];
    let area = |i: usize| -pieces[i].2;
    let mut keys: Vec<&dyn Fn(usize) -> f64> = by_spread(n, &axes, floor).into_iter().map(|k| axes[k]).collect();
    keys.push(&area);
    gapped_order(n, &keys, floor)
}

/// What [`order_edges`] orders: one edge's endpoints, its length and the
/// point half way along it by arc length.
#[derive(Clone, Copy, Debug)]
pub struct EdgeGeom {
    pub start: Vec3,
    pub end: Vec3,
    pub length: f64,
    pub midpoint: Vec3,
}

impl EdgeGeom {
    /// From a polyline: the midpoint is interpolated at half the polyline's
    /// arc length, so two arcs that share both endpoints (a cylinder cut in
    /// two) are told apart by where they go, not by a length that the
    /// tessellation decides.
    pub fn from_polyline(polyline: &[Vec3]) -> EdgeGeom {
        let start = polyline.first().copied().unwrap_or(Vec3::ZERO);
        let end = polyline.last().copied().unwrap_or(start);
        let length: f64 = polyline.windows(2).map(|w| (w[1] - w[0]).length()).sum();
        EdgeGeom { start, end, length, midpoint: polyline_midpoint(polyline, length) }
    }
}

/// The point at half of `length` along `polyline`.
pub fn polyline_midpoint(polyline: &[Vec3], length: f64) -> Vec3 {
    let Some(&first) = polyline.first() else { return Vec3::ZERO };
    let half = length * 0.5;
    let mut walked = 0.0;
    for w in polyline.windows(2) {
        let seg = (w[1] - w[0]).length();
        if walked + seg >= half {
            let t = if seg > 0.0 { (half - walked) / seg } else { 0.0 };
            return w[0] + (w[1] - w[0]) * t;
        }
        walked += seg;
    }
    polyline.last().copied().unwrap_or(first)
}

/// The canonical order of several edges that bound the same pair of faces:
/// by the arc-length midpoint, its three components in order of
/// their spread across the group ([`by_spread`]); then the lowest endpoint,
/// likewise; then the longer edge; then the order given -- each comparison
/// [`gapped_order`] at the group's [`tie_floor`]. The midpoint comes first
/// because the case that produces such edges is one curve cut in two, whose
/// halves share their endpoints and their nominal length and differ only in
/// where they go.
pub fn order_edges(edges: &[EdgeGeom]) -> Vec<usize> {
    let n = edges.len();
    if n < 2 {
        return (0..n).collect();
    }
    let floor = tie_floor(extent_of(edges.iter().flat_map(|e| [e.start, e.end, e.midpoint])));
    let mx = |i: usize| edges[i].midpoint.x;
    let my = |i: usize| edges[i].midpoint.y;
    let mz = |i: usize| edges[i].midpoint.z;
    let lo = |i: usize| edges[i].start.min(edges[i].end);
    let lx = |i: usize| lo(i).x;
    let ly = |i: usize| lo(i).y;
    let lz = |i: usize| lo(i).z;
    let len = |i: usize| -edges[i].length;
    let mid: [&dyn Fn(usize) -> f64; 3] = [&mx, &my, &mz];
    let low: [&dyn Fn(usize) -> f64; 3] = [&lx, &ly, &lz];
    let mut keys: Vec<&dyn Fn(usize) -> f64> = by_spread(n, &mid, floor).into_iter().map(|k| mid[k]).collect();
    keys.extend(by_spread(n, &low, floor).into_iter().map(|k| low[k]));
    keys.push(&len);
    gapped_order(n, &keys, floor)
}

// ---------------------------------------------------------------- generators

/// Meaningful names for the primitives (`box_1/top`, `cylinder_1/side`). The
/// generic `start` / `end` / `side[l.s]` names stay on as aliases, so a script
/// may use either.
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub enum Style {
    Generic,
    Box,
    Cylinder,
    Cone,
    /// A revolved primitive whose one profile segment is the whole surface:
    /// `surface`.
    Sphere,
    /// A revolved primitive whose profile is an outer and an inner arc:
    /// `outer`, `inner`.
    Torus,
}

impl Style {
    pub fn of(feature_kind: &str) -> Style {
        match feature_kind {
            "box" => Style::Box,
            "cylinder" => Style::Cylinder,
            "cone" => Style::Cone,
            "sphere" => Style::Sphere,
            "torus" => Style::Torus,
            _ => Style::Generic,
        }
    }
}

/// What a feature did, in the terms the naming grammar names its output by.
#[derive(Clone, Debug)]
pub enum Generator {
    /// A planar sketch swept along its own normal.
    Extrude {
        plane: Plane,
        distance: f64,
        /// Points along each segment of each loop, in sketch coordinates.
        loops: Vec<Vec<Vec<(f64, f64)>>>,
        style: Style,
    },
    /// A planar sketch revolved about an axis.
    Revolve {
        plane: Plane,
        origin: Vec3,
        axis: Vec3,
        angle_rad: f64,
        loops: Vec<Vec<Vec<(f64, f64)>>>,
        /// `Sphere` and `Torus` give the swept segments their primitive
        /// names beside the generic `seg[l.s]`; `Generic` otherwise.
        style: Style,
    },
    /// A loft through parallel sections. The sections are kept in **script
    /// order**, so band `b` is always "between the section the script listed
    /// b-th and the one it listed next", whichever way the loft runs.
    Loft {
        /// Common section normal.
        normal: Vec3,
        /// A point on the first profile section's plane; offsets measure from
        /// it along `normal`.
        origin: Vec3,
        /// Signed offset of each section, script order.
        offsets: Vec<f64>,
        /// Each section's sample points in world space, script order.
        sections: Vec<LoftRung>,
        /// Whether the kernel built one face per band or one per column. A
        /// skinned loft has no bands to name: its side is a single face
        /// spanning every section.
        ruled: bool,
        style: Style,
    },
    /// A loft through sections in space: list order is the order,
    /// and each end is closed by a plane, by the several faces of a ruled
    /// ribbon, or by a tip.
    LoftOrdered {
        /// Each section's sample points in world space, script order.
        sections: Vec<LoftRung>,
        /// How the start and the end are closed.
        caps: [CapRung; 2],
        /// One face per band (ruled) or one per column (skinned), as for
        /// [`Generator::Loft`].
        ruled: bool,
        style: Style,
    },
    /// The half-space `split()` intersects a body with. Only one of its faces
    /// ever survives into a piece — the one lying on the cutting plane — and
    /// that face is what the piece calls `cut`. The rest of the half-space is
    /// a long way from the body, so `tool[n]` is a name nothing should ever
    /// resolve.
    Cut { plane: Plane },
    /// A profile swept along a path. Every side face comes from one
    /// segment `s` of one loop `l` of the profile along one segment `b` of
    /// the path, so it is `side[l.s]#b` — the loft's band grammar with the
    /// path segment as the band — or the plain `side[l.s]` when the path has
    /// one segment; the two ends are `start` and `end` on the profile's
    /// plane and on that plane carried to the end of the path.
    Sweep {
        /// The points the sweep planted on each face it built: `(path
        /// segment, loop, profile segment, points)`, the profile's segment
        /// samples carried by the frame at several fractions along the path
        /// segment — on the true swept surface, whatever kind it was.
        cells: Vec<(usize, usize, usize, Vec<Vec3>)>,
        /// How many path segments there are: the bands.
        bands: usize,
        /// The profile's plane, and that plane at the end of the path.
        caps: [Plane; 2],
    },
    /// A tool whose faces are known by the analytic surface each lies on: a
    /// chamfer's cutter, whose chamfer face is `face[<edge>]` and
    /// whose legs and caps are `leg[<edge>]` / `cap[<edge>]`. A result face
    /// is named by the first surface every one of its samples lies on within
    /// `tol` (a plane also facing the same way), so the name goes with the
    /// surface into the boolean's catalogue and comes out on whatever face
    /// the boolean left on that surface — `kept`, exactly as a body's own
    /// faces do. The names are full names already.
    Surfaces { faces: Vec<(SurfaceClass, String)>, tol: f64 },
    /// A solid rebuilt face by face, whose maker knows exactly which face is
    /// which: an imprint or an emboss replaces one face of a body
    /// with that face cut by the sketch's loops, the pieces inside them and,
    /// for a relief, their walls and tops, and hands the shell back in an
    /// order it chose. The `i`-th face of the kernel's output is `names[i]`;
    /// nothing is probed, so a piece on the same surface as its parent — the
    /// one case a surface catalogue cannot tell apart — is still named
    /// exactly. A face past the end of the list is `face[i]`, unmatched.
    Indexed { names: Vec<NamedFace> },
}

/// How one end of an ordered loft is closed, as the namer sees it.
#[derive(Clone, Debug)]
pub enum CapRung {
    /// One planar face on this plane: `start` / `end`.
    Plane(Plane),
    /// A ruled ribbon: the points planted on each of its faces, in the order
    /// the kernel built them: `start#k` / `end#k`, with `start` / `end` as
    /// the group alias.
    Ruled(Vec<Vec<Vec3>>),
    /// A point section: no cap on this end.
    Tip,
}

/// One cell the loft namer plants points in: a band's segment, or a piece of
/// a ruled cap. Ordered so a tie between cells goes to the lower one, which
/// is what makes the vote reproducible.
#[derive(Clone, Copy, Debug, PartialEq, Eq, PartialOrd, Ord)]
enum Cell {
    Band { band: usize, l: usize, s: usize },
    Cap { end: usize, piece: usize },
}

/// One loft section as the namer sees it: world-space points on every segment
/// of every loop (`[loop][segment][point]`), or the single point of a tip.
#[derive(Clone, Debug)]
pub enum LoftRung {
    Profile(Vec<Vec<Vec<Vec3>>>),
    Point(Vec3),
}

/// Points per segment when sampling a loft section. Fixed (rather than
/// `segment_points`' per-kind count) because the namer interpolates section
/// *k*'s i-th point with section *k+1*'s i-th point — which is exactly the
/// correspondence the loft was built on — so the two lists have to line up.
const LOFT_SEG_SAMPLES: usize = 9;

/// Fractions along a band at which the namer plants points. The ends are
/// included so a face that hugs one section is still attributed to its band.
const BAND_FRACTIONS: [f64; 5] = [0.0, 0.25, 0.5, 0.75, 1.0];

/// Interior points of a segment, avoiding both ends so a face near a corner is
/// never attributed to its neighbour.
fn segment_points(seg: &Segment) -> Vec<(f64, f64)> {
    let n = match seg {
        Segment::Line { .. } => 5,
        _ => 9,
    };
    segment_points_n(seg, n)
}

/// [`segment_points`] with the count chosen by the caller.
fn segment_points_n(seg: &Segment, n: usize) -> Vec<(f64, f64)> {
    let lerp = |a: f64, b: f64, t: f64| a + (b - a) * t;
    (0..n)
        .map(|i| {
            let t = (i as f64 + 0.5) / n as f64;
            match seg {
                Segment::Line { a, b } => (lerp(a.x, b.x, t), lerp(a.y, b.y, t)),
                Segment::Arc { a, b, via } => match arc_params(*a, *b, *via) {
                    Some((c, r, t0, t1)) => {
                        let th = t0 + (t1 - t0) * t;
                        (c.x + r * fmath::cos(th), c.y + r * fmath::sin(th))
                    }
                    None => (lerp(a.x, b.x, t), lerp(a.y, b.y, t)),
                },
                Segment::Bezier { a, b, ctrl } => {
                    let mut pts = vec![*a];
                    pts.extend_from_slice(ctrl);
                    pts.push(*b);
                    let p = bezier_point(&pts, t);
                    (p.x, p.y)
                }
            }
        })
        .collect()
}

fn loop_points(s: &Sketch) -> Vec<Vec<Vec<(f64, f64)>>> {
    s.loops.iter().map(|l| l.segments.iter().map(segment_points).collect()).collect()
}

/// [`LOFT_SEG_SAMPLES`] points on every segment of a curve in space, at the
/// same parameters the planar sampler uses — the parametrisation the kernel's
/// ruled surface pairs points by.
fn curve_points(c: &Curve) -> Vec<Vec<Vec3>> {
    c.segments
        .iter()
        .map(|seg| {
            (0..LOFT_SEG_SAMPLES)
                .map(|i| seg.point_at_param((i as f64 + 0.5) / LOFT_SEG_SAMPLES as f64))
                .collect()
        })
        .collect()
}

impl Generator {
    pub fn extrude(sketch: &Sketch, distance: f64, feature_kind: &str) -> Generator {
        Generator::Extrude {
            plane: sketch.plane,
            distance,
            loops: loop_points(sketch),
            style: Style::of(feature_kind),
        }
    }

    /// Describe a loft for the namer. Everything it needs is already in the
    /// plan: the correspondence is by construction order, so the namer can
    /// rebuild the surface of every band by interpolating the two sections'
    /// sample points — the same interpolation the kernel's ruled surface is.
    pub fn loft(plan: &LoftPlan, feature_kind: &str) -> Generator {
        let style = Style::of(feature_kind);
        let sections: Vec<LoftRung> = plan
            .sections
            .iter()
            .map(|s| match s {
                PlannedSection::Point(p) => LoftRung::Point(*p),
                PlannedSection::Curve(c) => LoftRung::Profile(vec![curve_points(c)]),
                PlannedSection::Profile { plane, loops, .. } => LoftRung::Profile(
                    loops
                        .iter()
                        .map(|l| {
                            l.segments
                                .iter()
                                .map(|seg| {
                                    segment_points_n(seg, LOFT_SEG_SAMPLES)
                                        .into_iter()
                                        .map(|(u, v)| plane.to_world(u, v))
                                        .collect()
                                })
                                .collect()
                        })
                        .collect(),
                ),
            })
            .collect();
        match &plan.layout {
            Layout::Stacked { normal, origin, offsets, .. } => Generator::Loft {
                normal: *normal,
                origin: *origin,
                offsets: offsets.clone(),
                sections,
                ruled: plan.ruled,
                style,
            },
            Layout::Ordered => {
                let n = plan.sections.len();
                let cap_of = |e: usize| -> CapRung {
                    let i = if e == 0 { 0 } else { n - 1 };
                    match (&plan.sections[i], plan.caps[e]) {
                        (PlannedSection::Curve(c), Cap::Plane) => match c.plane() {
                            Some(p) => CapRung::Plane(p),
                            None => CapRung::Tip,
                        },
                        (PlannedSection::Curve(_), Cap::Ruled) => match &plan.ruled_caps[e] {
                            Some(rc) => CapRung::Ruled(crate::loft::ruled_cap_points(
                                rc,
                                LOFT_SEG_SAMPLES,
                                &BAND_FRACTIONS,
                            )),
                            None => CapRung::Tip,
                        },
                        _ => CapRung::Tip,
                    }
                };
                Generator::LoftOrdered { sections, caps: [cap_of(0), cap_of(1)], ruled: plan.ruled, style }
            }
        }
    }

    pub fn revolve(sketch: &Sketch, origin: Vec3, axis: Vec3, angle_rad: f64) -> Generator {
        Generator::revolve_styled(sketch, origin, axis, angle_rad, Style::Generic)
    }

    /// Describe a sweep for the namer: the profile's segment samples
    /// carried by the placements the sweep recorded along each path
    /// segment, so every planted point lies on the swept surface whatever
    /// the segment's kind — a translation, a rotation, or a skin.
    pub fn sweep(profile: &Sketch, layout: &crate::sweep::SweepLayout) -> Generator {
        let mut cells = Vec::new();
        for (b, placements) in layout.placements.iter().enumerate() {
            for (l, lp) in profile.loops.iter().enumerate() {
                for (s, seg) in lp.segments.iter().enumerate() {
                    let local = segment_points_n(seg, LOFT_SEG_SAMPLES);
                    let mut pts = Vec::with_capacity(local.len() * placements.len());
                    for pl in placements {
                        for (u, v) in &local {
                            pts.push(pl.apply(profile.plane.to_world(*u, *v)));
                        }
                    }
                    cells.push((b, l, s, pts));
                }
            }
        }
        Generator::Sweep { cells, bands: layout.placements.len(), caps: [layout.start_plane, layout.end_plane] }
    }

    /// [`Generator::revolve`] for a revolved primitive, whose segments have
    /// names of their own (`sphere/surface`, `torus/outer`).
    pub fn revolve_styled(sketch: &Sketch, origin: Vec3, axis: Vec3, angle_rad: f64, style: Style) -> Generator {
        Generator::Revolve {
            plane: sketch.plane,
            origin,
            axis: axis.normalized(),
            angle_rad,
            loops: loop_points(sketch),
            style,
        }
    }
}

/// Names for the faces a generator produced. Returns one entry per probe, in
/// probe order; names are full (`extrude_1/side[0.2]`).
pub fn name_faces(g: &Generator, feature: &str, probes: &[FaceProbe]) -> Vec<NamedFace> {
    match g {
        Generator::Extrude { plane, distance, loops, style } => {
            name_extrude(*plane, *distance, loops, *style, feature, probes)
        }
        Generator::Revolve { plane, origin, axis, angle_rad, loops, style } => {
            name_revolve(*plane, *origin, *axis, *angle_rad, loops, *style, feature, probes)
        }
        Generator::Loft { normal, origin, offsets, sections, ruled, style } => {
            name_loft(*normal, *origin, offsets, sections, *ruled, *style, feature, probes)
        }
        Generator::LoftOrdered { sections, caps, ruled, style } => {
            name_loft_ordered(sections, caps, *ruled, *style, feature, probes)
        }
        Generator::Cut { plane } => name_cut(*plane, feature, probes),
        Generator::Sweep { cells, bands, caps } => name_sweep(cells, *bands, caps, feature, probes),
        Generator::Surfaces { faces, tol } => name_surfaces(faces, *tol, feature, probes),
        Generator::Indexed { names } => probes
            .iter()
            .enumerate()
            .map(|(i, _)| {
                names.get(i).cloned().unwrap_or_else(|| NamedFace::new(full_name(feature, &format!("face[{i}]")), Role::Unmatched))
            })
            .collect(),
    }
}

/// Names for the faces of a sweep (the `sweep_along` row of the
/// naming grammar in docs/design.md). The two caps are decided by the "flat and on the plane" test
/// against the profile's plane and its image at the end of the path; every
/// other face goes to the cell whose planted points it sits nearest —
/// [`nearest_cell`], the loft's own vote — and is `side[l.s]#b` for path
/// segment `b`, with `side[l.s]` as the group alias for the whole run, or
/// the plain `side[l.s]` when the path is one segment.
fn name_sweep(
    cells: &[(usize, usize, usize, Vec<Vec3>)],
    bands: usize,
    caps: &[Plane; 2],
    feature: &str,
    probes: &[FaceProbe],
) -> Vec<NamedFace> {
    let cos_cap = fmath::cos(CAP_ANGLE_DEG.to_radians());
    let extent = cells.iter().flat_map(|c| c.3.iter()).fold(0.0f64, |m, p| m.max(p.length()));
    let eps = cap_eps(extent).max(1e-9);
    let keyed: Vec<(Cell, Vec<Vec3>)> =
        cells.iter().map(|(b, l, s, pts)| (Cell::Band { band: *b, l: *l, s: *s }, pts.clone())).collect();
    let mut out = Vec::with_capacity(probes.len());
    for (i, p) in probes.iter().enumerate() {
        let on_cap = |plane: &Plane| -> bool {
            let flat = p.normal.dot(plane.normal).abs() >= cos_cap;
            let height = |v: Vec3| (v - plane.origin).dot(plane.normal);
            flat
                && !p.samples.is_empty()
                && p.samples.iter().all(|s| height(*s).abs() <= eps)
                && height(p.centroid).abs() <= eps
        };
        if on_cap(&caps[0]) {
            out.push(NamedFace::new(full_name(feature, "start"), Role::New));
            continue;
        }
        if on_cap(&caps[1]) {
            out.push(NamedFace::new(full_name(feature, "end"), Role::New));
            continue;
        }
        let pts: Vec<Vec3> = p.samples.iter().copied().chain(std::iter::once(p.centroid)).collect();
        match nearest_cell(&keyed, &pts) {
            Some(Cell::Band { band: b, l, s: sg }) => {
                out.push(band_name(feature, bands, b, l, sg, Style::Generic, p.normal))
            }
            Some(Cell::Cap { .. }) => unreachable!("a sweep plants no cap cells"),
            None => out.push(NamedFace::new(full_name(feature, &format!("face[{i}]")), Role::Unmatched)),
        }
    }
    out
}

/// Names for the faces of a loft (the `loft` row of the naming grammar).
///
/// The two end sections are `start` and `end`; every other face is swept from
/// one segment of one loop, so it is `side[l.s]` exactly as an extrude's is. A
/// **ruled** loft through more than two sections has one such face *per band*,
/// so the name carries the band as the grammar's split suffix: `side[l.s]#b`
/// is the band between the section the script listed b-th and the next one,
/// and the unsuffixed `side[l.s]` stays on as the group alias for the whole
/// column. Gridfinity's foot therefore selects as `foot/side[0.1]#0` (the 45°
/// top chamfer at corner 1) with `foot/side[0.1]` meaning all three bands of
/// that corner.
///
/// A **skinned** loft (`ruled=False`) has no bands: one face spans every
/// section, so there is nothing for `#b` to distinguish and the name is the
/// plain `side[l.s]` a two-section loft gets. That is not a special case in
/// the namer so much as the truth about what the kernel built — which is why
/// the flag is carried rather than guessed from the face count.
///
/// The band a face belongs to is decided by where the face *is*, against points
/// the loft itself planted on its own ruled surfaces — never by the order the
/// kernel emitted faces in. The two agree today, but only one of them is stable
/// under a kernel change.
#[allow(clippy::too_many_arguments)]
fn name_loft(
    normal: Vec3,
    origin: Vec3,
    offsets: &[f64],
    sections: &[LoftRung],
    ruled: bool,
    style: Style,
    feature: &str,
    probes: &[FaceProbe],
) -> Vec<NamedFace> {
    let n = sections.len();
    let bands = n - 1;
    let named_bands = if ruled { bands } else { 1 };
    let ascending = offsets[n - 1] >= offsets[0];
    let up = if ascending { normal } else { -normal };
    let span = offsets.iter().cloned().fold(f64::MIN, f64::max)
        - offsets.iter().cloned().fold(f64::MAX, f64::min);
    let eps = cap_eps(span).max(1e-9);
    let cos_cap = fmath::cos(CAP_ANGLE_DEG.to_radians());
    let height = |v: Vec3| (v - origin).dot(normal);

    let cells = band_cells(sections, ruled);

    let mut out = Vec::with_capacity(probes.len());
    for (i, p) in probes.iter().enumerate() {
        let flat = p.normal.dot(normal).abs() >= cos_cap;
        let all_at = |h: f64| {
            !p.samples.is_empty() && p.samples.iter().all(|s| (height(*s) - h).abs() <= eps)
        };
        let cap = if flat && matches!(sections[0], LoftRung::Profile(_)) && all_at(offsets[0]) {
            Some(("start", cap_alias(style, false, up)))
        } else if flat && matches!(sections[n - 1], LoftRung::Profile(_)) && all_at(offsets[n - 1]) {
            Some(("end", cap_alias(style, true, up)))
        } else {
            None
        };
        if let Some((generic, meaningful)) = cap {
            out.push(named(feature, generic, meaningful, Role::New));
            continue;
        }
        let pts: Vec<Vec3> = p.samples.iter().copied().chain(std::iter::once(p.centroid)).collect();
        match nearest_cell(&cells, &pts) {
            Some(Cell::Band { band: b, l, s: sg }) => {
                out.push(band_name(feature, named_bands, b, l, sg, style, p.normal))
            }
            Some(Cell::Cap { .. }) => unreachable!("a stacked loft plants no cap cells"),
            None => out.push(NamedFace::new(full_name(feature, &format!("face[{i}]")), Role::Unmatched)),
        }
    }
    out
}

/// Points on every band's ruled surface, keyed by (band, loop, segment): the
/// same interpolation the kernel's ruled surface is, at [`BAND_FRACTIONS`]
/// between the two sections' corresponding sample points. A skinned column
/// is one face, so every band's planted points then belong to the same cell
/// and the same name.
fn band_cells(sections: &[LoftRung], ruled: bool) -> Vec<(Cell, Vec<Vec3>)> {
    let bands = sections.len() - 1;
    let cell_band = |b: usize| if ruled { b } else { 0 };
    let mut cells: Vec<(Cell, Vec<Vec3>)> = Vec::new();
    for b in 0..bands {
        match (&sections[b], &sections[b + 1]) {
            (LoftRung::Profile(a), LoftRung::Profile(c)) => {
                for (l, (la, lc)) in a.iter().zip(c).enumerate() {
                    for (sg, (sa, sc)) in la.iter().zip(lc).enumerate() {
                        let mut pts = Vec::with_capacity(sa.len() * BAND_FRACTIONS.len());
                        for t in BAND_FRACTIONS {
                            for (pa, pc) in sa.iter().zip(sc) {
                                pts.push(*pa + (*pc - *pa) * t);
                            }
                        }
                        cells.push((Cell::Band { band: cell_band(b), l, s: sg }, pts));
                    }
                }
            }
            (LoftRung::Profile(a), LoftRung::Point(tip)) | (LoftRung::Point(tip), LoftRung::Profile(a)) => {
                for (l, la) in a.iter().enumerate() {
                    for (sg, sa) in la.iter().enumerate() {
                        let mut pts = Vec::with_capacity(sa.len() * BAND_FRACTIONS.len());
                        for t in BAND_FRACTIONS {
                            for pa in sa {
                                pts.push(*pa + (*tip - *pa) * t);
                            }
                        }
                        cells.push((Cell::Band { band: cell_band(b), l, s: sg }, pts));
                    }
                }
            }
            (LoftRung::Point(_), LoftRung::Point(_)) => {}
        }
    }
    cells
}

/// `side[l.s]` for a loft with one named band, `side[l.s]#b` with the column
/// alias for more.
fn band_name(feature: &str, named_bands: usize, b: usize, l: usize, sg: usize, style: Style, normal: Vec3) -> NamedFace {
    let generic = if named_bands > 1 { format!("side[{l}.{sg}]#{b}") } else { format!("side[{l}.{sg}]") };
    let mut nf = named(feature, &generic, side_alias(style, normal), Role::New);
    if named_bands > 1 {
        nf = nf.with_alias(full_name(feature, &format!("side[{l}.{sg}]")));
    }
    nf
}

/// Names for the faces of a loft in the ordered layout: the sides as
/// [`name_loft`] names them, with `b` the script's list index and nothing
/// else (and no `#b` at all when skinned, as there); a planar cap `start` /
/// `end` by the same "flat and on the plane" test, against the section's own
/// best-fit plane rather than a common normal; and a ruled cap's faces
/// `start#k` / `end#k`, decided like a band — by the points the plan planted
/// on each piece — with `start` / `end` as the alias for the whole cap.
fn name_loft_ordered(
    sections: &[LoftRung],
    caps: &[CapRung; 2],
    ruled: bool,
    style: Style,
    feature: &str,
    probes: &[FaceProbe],
) -> Vec<NamedFace> {
    let bands = sections.len() - 1;
    let named_bands = if ruled { bands } else { 1 };
    let cos_cap = fmath::cos(CAP_ANGLE_DEG.to_radians());
    let mut cells = band_cells(sections, ruled);
    for (end, cap) in caps.iter().enumerate() {
        if let CapRung::Ruled(pieces) = cap {
            for (piece, pts) in pieces.iter().enumerate() {
                cells.push((Cell::Cap { end, piece }, pts.clone()));
            }
        }
    }
    let extent = sections
        .iter()
        .flat_map(|s| match s {
            LoftRung::Profile(loops) => loops.iter().flatten().flatten().copied().collect::<Vec<_>>(),
            LoftRung::Point(p) => vec![*p],
        })
        .fold(0.0f64, |m, p| m.max(p.length()));
    let eps = cap_eps(extent).max(1e-9);
    let mut out = Vec::with_capacity(probes.len());
    for (i, p) in probes.iter().enumerate() {
        let on_cap = |end: usize| -> bool {
            let CapRung::Plane(plane) = &caps[end] else { return false };
            let flat = p.normal.dot(plane.normal).abs() >= cos_cap;
            let height = |v: Vec3| (v - plane.origin).dot(plane.normal);
            flat && !p.samples.is_empty() && p.samples.iter().all(|s| height(*s).abs() <= eps)
        };
        if on_cap(0) {
            out.push(named(feature, "start", cap_alias(style, false, p.normal), Role::New));
            continue;
        }
        if on_cap(1) {
            out.push(named(feature, "end", cap_alias(style, true, p.normal), Role::New));
            continue;
        }
        let pts: Vec<Vec3> = p.samples.iter().copied().chain(std::iter::once(p.centroid)).collect();
        match nearest_cell(&cells, &pts) {
            Some(Cell::Band { band: b, l, s: sg }) => {
                out.push(band_name(feature, named_bands, b, l, sg, style, p.normal))
            }
            Some(Cell::Cap { end, piece }) => {
                let group = if end == 0 { "start" } else { "end" };
                let nf = named(feature, &format!("{group}#{piece}"), cap_alias(style, end == 1, p.normal), Role::New)
                    .with_alias(full_name(feature, group));
                out.push(nf);
            }
            None => out.push(NamedFace::new(full_name(feature, &format!("face[{i}]")), Role::Unmatched)),
        }
    }
    out
}

/// The cell whose planted points sit closest to `pts`, by majority vote with
/// the summed distance as the tiebreak — the 3-D twin of
/// [`nearest_segment`], and deterministic for the same reason.
fn nearest_cell(cells: &[(Cell, Vec<Vec3>)], pts: &[Vec3]) -> Option<Cell> {
    let mut votes: BTreeMap<Cell, (u32, i64)> = BTreeMap::new();
    for p in pts {
        let mut best: Option<(Cell, f64)> = None;
        for (key, cloud) in cells {
            for c in cloud {
                let d = (*c - *p).length();
                if best.map(|(_, bd)| d < bd).unwrap_or(true) {
                    best = Some((*key, d));
                }
            }
        }
        if let Some((k, d)) = best {
            let e = votes.entry(k).or_insert((0, 0));
            e.0 += 1;
            e.1 += q(d);
        }
    }
    votes
        .into_iter()
        .max_by_key(|(k, (n, dist))| (*n, -*dist, std::cmp::Reverse(*k)))
        .map(|(k, _)| k)
}

/// `cut` for the face on the cutting plane, `tool[n]` for the rest of the
/// half-space.
fn name_cut(plane: Plane, feature: &str, probes: &[FaceProbe]) -> Vec<NamedFace> {
    let cos_cap = fmath::cos(CAP_ANGLE_DEG.to_radians());
    probes
        .iter()
        .enumerate()
        .map(|(i, p)| {
            let height = |v: Vec3| (v - plane.origin).dot(plane.normal);
            let flat = p.normal.dot(plane.normal).abs() >= cos_cap;
            let on_plane = p.samples.iter().all(|s| height(*s).abs() <= 1e-6) && height(p.centroid).abs() <= 1e-6;
            if flat && on_plane {
                NamedFace::new(full_name(feature, "cut"), Role::New)
            } else {
                NamedFace::new(full_name(feature, &format!("tool[{i}]")), Role::New)
            }
        })
        .collect()
}

/// Each probe takes the name of the first listed surface all of its samples
/// lie on; a probe on none of them is the `face[n]` fallback. The names are
/// full names already (a chamfer face's local part carries the edge's own
/// `/`, which `full_name` would mistake for a prefix), so they are used as
/// given.
fn name_surfaces(faces: &[(SurfaceClass, String)], tol: f64, feature: &str, probes: &[FaceProbe]) -> Vec<NamedFace> {
    let cos_cap = fmath::cos(CAP_ANGLE_DEG.to_radians());
    probes
        .iter()
        .enumerate()
        .map(|(i, p)| {
            // The samples, not the centroid: a curved face's centroid is
            // nowhere near it (a half-cylinder's sits 2r/π in from the wall).
            let hit = faces.iter().find(|(s, _)| {
                let on = !p.samples.is_empty() && p.samples.iter().all(|x| s.distance(*x) <= tol);
                let facing = match s {
                    SurfaceClass::Plane { normal, .. } => p.normal.dot(*normal).abs() >= cos_cap,
                    _ => true,
                };
                on && facing
            });
            match hit {
                Some((_, name)) => NamedFace::new(name.clone(), Role::New),
                None => NamedFace::new(full_name(feature, &format!("face[{i}]")), Role::Unmatched),
            }
        })
        .collect()
}

/// The nearest (loop, segment) to a set of sketch-space points, by majority
/// vote over the points; ties go to the lower (loop, segment).
fn nearest_segment(loops: &[Vec<Vec<(f64, f64)>>], pts: &[(f64, f64)]) -> Option<(usize, usize)> {
    let mut votes: BTreeMap<(usize, usize), (u32, i64)> = BTreeMap::new();
    for p in pts {
        let mut best: Option<((usize, usize), f64)> = None;
        for (l, segs) in loops.iter().enumerate() {
            for (s, sp) in segs.iter().enumerate() {
                for c in sp {
                    let d = fmath::powi(c.0 - p.0, 2) + fmath::powi(c.1 - p.1, 2);
                    if best.map(|(_, bd)| d < bd).unwrap_or(true) {
                        best = Some(((l, s), d));
                    }
                }
            }
        }
        if let Some((k, d)) = best {
            let e = votes.entry(k).or_insert((0, 0));
            e.0 += 1;
            e.1 += q(d.sqrt());
        }
    }
    votes.into_iter().max_by_key(|(k, (n, dist))| (*n, -*dist, -(k.0 as i64), -(k.1 as i64))).map(|(k, _)| k)
}

fn cap_eps(scale: f64) -> f64 {
    1e-6 * scale.abs().max(1.0)
}

fn name_extrude(
    plane: Plane,
    distance: f64,
    loops: &[Vec<Vec<(f64, f64)>>],
    style: Style,
    feature: &str,
    probes: &[FaceProbe],
) -> Vec<NamedFace> {
    let eps = cap_eps(distance);
    let cos_cap = fmath::cos(CAP_ANGLE_DEG.to_radians());
    let mut out = Vec::with_capacity(probes.len());
    for (i, p) in probes.iter().enumerate() {
        let height = |v: Vec3| (v - plane.origin).dot(plane.normal);
        let flat = p.normal.dot(plane.normal).abs() >= cos_cap;
        let all_at = |h: f64| p.samples.iter().all(|s| (height(*s) - h).abs() <= eps.max(1e-9));
        let local = if flat && (p.samples.is_empty() || all_at(0.0)) && (height(p.centroid)).abs() <= eps.max(1e-9) {
            Some(("start", cap_alias(style, false, plane.normal * distance.signum())))
        } else if flat && (p.samples.is_empty() || all_at(distance)) && (height(p.centroid) - distance).abs() <= eps.max(1e-9) {
            Some(("end", cap_alias(style, true, plane.normal * distance.signum())))
        } else {
            None
        };
        if let Some((generic, meaningful)) = local {
            out.push(named(feature, generic, meaningful, Role::New));
            continue;
        }
        // A side: attribute it to the loop and segment it was swept from.
        let pts: Vec<(f64, f64)> = p
            .samples
            .iter()
            .chain(std::iter::once(&p.centroid))
            .map(|s| {
                let d = *s - plane.origin;
                (d.dot(plane.x_dir), d.dot(plane.y_dir()))
            })
            .collect();
        match nearest_segment(loops, &pts) {
            Some((l, s)) => {
                let generic = format!("side[{l}.{s}]");
                let meaningful = side_alias(style, p.normal);
                out.push(named(feature, &generic, meaningful, Role::New));
            }
            None => out.push(NamedFace::new(full_name(feature, &format!("face[{i}]")), Role::Unmatched)),
        }
    }
    out
}

/// `top` / `bottom` for a box or cylinder cap, derived from where it points so
/// the name means what it says whichever way the sketch was swept.
fn cap_alias(style: Style, end: bool, sweep: Vec3) -> Option<String> {
    if style == Style::Generic {
        return None;
    }
    let up = if end { sweep } else { -sweep };
    if up.z > 0.9 {
        Some("top".into())
    } else if up.z < -0.9 {
        Some("bottom".into())
    } else {
        None
    }
}

/// `front` / `back` / `left` / `right` for an axis-aligned box wall.
fn side_alias(style: Style, normal: Vec3) -> Option<String> {
    match style {
        Style::Box => {
            let n = normal.normalized();
            if n.x > 0.999 {
                Some("right".into())
            } else if n.x < -0.999 {
                Some("left".into())
            } else if n.y > 0.999 {
                Some("back".into())
            } else if n.y < -0.999 {
                Some("front".into())
            } else {
                None
            }
        }
        // A circle is two arcs in the sketch, so a cylinder (or cone) wall is
        // two faces; `side` names the wall and resolves to both of them.
        Style::Cylinder | Style::Cone => Some("side".into()),
        Style::Generic | Style::Sphere | Style::Torus => None,
    }
}

/// `surface` for a sphere's one swept segment, `outer` / `inner` for the
/// two arcs of a torus's tube. A generic revolve has only `seg[l.s]`.
fn revolve_alias(style: Style, s: usize) -> Option<String> {
    match style {
        Style::Sphere => Some("surface".into()),
        Style::Torus => Some(if s == 0 { "outer" } else { "inner" }.into()),
        Style::Generic | Style::Box | Style::Cylinder | Style::Cone => None,
    }
}

/// Build the name: the meaningful one wins when there is one, and the generic
/// form stays on as an alias.
fn named(feature: &str, generic: &str, meaningful: Option<String>, role: Role) -> NamedFace {
    match meaningful {
        Some(m) => NamedFace::new(full_name(feature, &m), role).with_alias(full_name(feature, generic)),
        None => NamedFace::new(full_name(feature, generic), role),
    }
}

#[allow(clippy::too_many_arguments)]
fn name_revolve(
    plane: Plane,
    origin: Vec3,
    axis: Vec3,
    angle_rad: f64,
    loops: &[Vec<Vec<(f64, f64)>>],
    style: Style,
    feature: &str,
    probes: &[FaceProbe],
) -> Vec<NamedFace> {
    let axis = axis.normalized();
    let full_turn = (angle_rad.abs() - std::f64::consts::TAU).abs() < 1e-9;
    // The direction from the axis towards the profile, at angle 0 and at the
    // end angle: the two cap half-planes of a partial revolve.
    let mut mid = Vec3::ZERO;
    let mut n = 0.0;
    for segs in loops {
        for sp in segs {
            for (u, v) in sp {
                mid = mid + plane.to_world(*u, *v);
                n += 1.0;
            }
        }
    }
    let ref0 = if n > 0.0 {
        let d = mid * (1.0 / n) - origin;
        (d - axis * d.dot(axis)).normalized()
    } else {
        plane.x_dir
    };
    let ref1 = rotate_about(ref0, axis, angle_rad);
    let eps = cap_eps(1.0);

    let cyl = |p: Vec3| {
        let d = p - origin;
        let h = d.dot(axis);
        let r = (d - axis * h).length();
        (r, h)
    };
    let profile: Vec<Vec<Vec<(f64, f64)>>> = loops
        .iter()
        .map(|segs| segs.iter().map(|sp| sp.iter().map(|(u, v)| cyl(plane.to_world(*u, *v))).collect()).collect())
        .collect();

    let mut out = Vec::with_capacity(probes.len());
    for (i, p) in probes.iter().enumerate() {
        let on_half = |r: Vec3| {
            let nh = axis.cross(r);
            p.samples.iter().all(|s| {
                let d = *s - origin;
                d.dot(nh).abs() <= eps && d.dot(r) >= -eps
            })
        };
        if !full_turn && !p.samples.is_empty() && on_half(ref0) {
            out.push(NamedFace::new(full_name(feature, "start"), Role::New));
            continue;
        }
        if !full_turn && !p.samples.is_empty() && on_half(ref1) {
            out.push(NamedFace::new(full_name(feature, "end"), Role::New));
            continue;
        }
        let pts: Vec<(f64, f64)> =
            p.samples.iter().chain(std::iter::once(&p.centroid)).map(|s| cyl(*s)).collect();
        match nearest_segment(&profile, &pts) {
            Some((l, s)) => out.push(named(feature, &format!("seg[{l}.{s}]"), revolve_alias(style, s), Role::New)),
            None => out.push(NamedFace::new(full_name(feature, &format!("face[{i}]")), Role::Unmatched)),
        }
    }
    out
}

fn rotate_about(v: Vec3, axis: Vec3, angle: f64) -> Vec3 {
    let (s, c) = fmath::sin_cos(angle);
    v * c + axis.cross(v) * s + axis * (axis.dot(v) * (1.0 - c))
}

// ---------------------------------------------------- propagation (booleans)

/// The named surfaces a boolean's operands bring in: every source face as the
/// triangles it covers, in a uniform grid, so a face of the result can be
/// attributed to the source surface it lies on.
///
/// This is matching to source faces by surface identity and area
/// overlap, measured where both kernels can measure it — on the triangles.
/// It is the same code for a truck result and a manifold result, which is why
/// the two agree on names.
#[derive(Clone, Debug, Default)]
pub struct SurfaceCatalogue {
    entries: Vec<NamedFace>,
    tris: Vec<[Vec3; 3]>,
    owner: Vec<u32>,
    cells: HashMap<(i32, i32, i32), Vec<u32>>,
    /// Triangles too large to index; checked on every query.
    big: Vec<u32>,
    cell: f64,
    tol: f64,
}

impl SurfaceCatalogue {
    /// Start a catalogue that matches points within `tol` of a source surface.
    pub fn new(tol: f64) -> SurfaceCatalogue {
        SurfaceCatalogue { tol: tol.max(1e-9), cell: 0.0, ..Default::default() }
    }

    pub fn is_empty(&self) -> bool {
        self.tris.is_empty()
    }

    pub fn len(&self) -> usize {
        self.entries.len()
    }

    /// Add one named source face covering `tris`.
    pub fn add_face(&mut self, name: NamedFace, tris: impl IntoIterator<Item = [Vec3; 3]>) {
        let owner = self.entries.len() as u32;
        self.entries.push(name);
        for t in tris {
            self.tris.push(t);
            self.owner.push(owner);
        }
    }

    /// Index the triangles. Must be called before [`SurfaceCatalogue::nearest`].
    pub fn finish(&mut self) {
        self.cells.clear();
        self.big.clear();
        if self.tris.is_empty() {
            return;
        }
        let mut lo = self.tris[0][0];
        let mut hi = lo;
        for t in &self.tris {
            for p in t {
                lo = lo.min(*p);
                hi = hi.max(*p);
            }
        }
        let diag = (hi - lo).length().max(1e-6);
        self.cell = (self.tol * 4.0).max(diag / 32.0);
        for (i, t) in self.tris.iter().enumerate() {
            let (a, b) = tri_cells(t, self.cell);
            let span = (b.0 - a.0 + 1) as i64 * (b.1 - a.1 + 1) as i64 * (b.2 - a.2 + 1) as i64;
            if span > 2048 {
                self.big.push(i as u32);
                continue;
            }
            for x in a.0..=b.0 {
                for y in a.1..=b.1 {
                    for z in a.2..=b.2 {
                        self.cells.entry((x, y, z)).or_default().push(i as u32);
                    }
                }
            }
        }
    }

    /// The source face `p` lies on, if any is within the tolerance.
    pub fn nearest(&self, p: Vec3) -> Option<u32> {
        if self.cell <= 0.0 {
            return None;
        }
        let k = cell_of(p, self.cell);
        let mut best: Option<(u32, f64)> = None;
        let consider = |i: u32, best: &mut Option<(u32, f64)>| {
            let t = &self.tris[i as usize];
            let d = point_triangle_distance(p, t[0], t[1], t[2]);
            if d <= self.tol && best.map(|(_, bd)| d < bd).unwrap_or(true) {
                *best = Some((self.owner[i as usize], d));
            }
        };
        for dx in -1..=1 {
            for dy in -1..=1 {
                for dz in -1..=1 {
                    if let Some(ids) = self.cells.get(&(k.0 + dx, k.1 + dy, k.2 + dz)) {
                        for &i in ids {
                            consider(i, &mut best);
                        }
                    }
                }
            }
        }
        for &i in &self.big {
            consider(i, &mut best);
        }
        best.map(|(o, _)| o)
    }

    /// The name of entry `i`.
    pub fn entry(&self, i: u32) -> &NamedFace {
        &self.entries[i as usize]
    }
}

fn cell_of(p: Vec3, cell: f64) -> (i32, i32, i32) {
    ((p.x / cell).floor() as i32, (p.y / cell).floor() as i32, (p.z / cell).floor() as i32)
}

fn tri_cells(t: &[Vec3; 3], cell: f64) -> ((i32, i32, i32), (i32, i32, i32)) {
    let lo = t[0].min(t[1]).min(t[2]);
    let hi = t[0].max(t[1]).max(t[2]);
    (cell_of(lo, cell), cell_of(hi, cell))
}

/// Distance from `p` to triangle `abc` (Ericson, *Real-Time Collision
/// Detection* §5.1.5).
pub fn point_triangle_distance(p: Vec3, a: Vec3, b: Vec3, c: Vec3) -> f64 {
    let ab = b - a;
    let ac = c - a;
    let ap = p - a;
    let d1 = ab.dot(ap);
    let d2 = ac.dot(ap);
    if d1 <= 0.0 && d2 <= 0.0 {
        return (p - a).length();
    }
    let bp = p - b;
    let d3 = ab.dot(bp);
    let d4 = ac.dot(bp);
    if d3 >= 0.0 && d4 <= d3 {
        return (p - b).length();
    }
    let vc = d1 * d4 - d3 * d2;
    if vc <= 0.0 && d1 >= 0.0 && d3 <= 0.0 {
        let v = if (d1 - d3).abs() > 0.0 { d1 / (d1 - d3) } else { 0.0 };
        return (p - (a + ab * v)).length();
    }
    let cp = p - c;
    let d5 = ab.dot(cp);
    let d6 = ac.dot(cp);
    if d6 >= 0.0 && d5 <= d6 {
        return (p - c).length();
    }
    let vb = d5 * d2 - d1 * d6;
    if vb <= 0.0 && d2 >= 0.0 && d6 <= 0.0 {
        let w = if (d2 - d6).abs() > 0.0 { d2 / (d2 - d6) } else { 0.0 };
        return (p - (a + ac * w)).length();
    }
    let va = d3 * d6 - d5 * d4;
    if va <= 0.0 && (d4 - d3) >= 0.0 && (d5 - d6) >= 0.0 {
        let denom = (d4 - d3) + (d5 - d6);
        let w = if denom.abs() > 0.0 { (d4 - d3) / denom } else { 0.0 };
        return (p - (b + (c - b) * w)).length();
    }
    let denom = va + vb + vc;
    if denom.abs() <= 0.0 {
        return (p - a).length();
    }
    let v = vb / denom;
    let w = vc / denom;
    (p - (a + ab * v + ac * w)).length()
}

/// Names for the faces of a boolean's result, taken from the surfaces they lie
/// on. `feature` only supplies the `face[n]` fallback for a face that matched
/// nothing (a genuinely new surface, which today means a diagnostic).
pub fn name_inherited(cat: &SurfaceCatalogue, feature: &str, probes: &[FaceProbe]) -> Vec<NamedFace> {
    let mut out = Vec::with_capacity(probes.len());
    for (i, p) in probes.iter().enumerate() {
        let mut votes: BTreeMap<u32, u32> = BTreeMap::new();
        let total = p.samples.len().max(1) as f64;
        for s in &p.samples {
            if let Some(e) = cat.nearest(*s) {
                *votes.entry(e).or_insert(0) += 1;
            }
        }
        let best = votes.iter().max_by_key(|(e, n)| (**n, std::cmp::Reverse(**e))).map(|(e, n)| (*e, *n));
        match best {
            Some((e, n)) if n as f64 / total >= MATCH_COVERAGE => {
                let src = cat.entry(e);
                let mut face = NamedFace { name: src.name.clone(), aliases: src.aliases.clone(), role: Role::Kept };
                let mut merged = false;
                for (o, c) in &votes {
                    if *o != e && *c as f64 / total >= ALIAS_COVERAGE {
                        face = face.with_alias(cat.entry(*o).name.clone());
                        merged = true;
                    }
                }
                if merged {
                    face.role = Role::Merged;
                }
                out.push(face);
            }
            _ => out.push(NamedFace::new(full_name(feature, &format!("face[{i}]")), Role::Unmatched)),
        }
    }
    out
}

// ---------------------------------------------------------------- resolution

/// The geometric fallback (docs/design.md), captured when a selection is made.
#[derive(Clone, Debug, Default, PartialEq)]
pub struct Fingerprint {
    pub surface_kind: String,
    pub centroid: Vec3,
    pub normal: Vec3,
    /// Square millimetres, for a face.
    pub area: f64,
    /// Millimetres, for an edge.
    pub length: f64,
}

impl Fingerprint {
    pub fn to_proto(&self) -> cadpole_proto::v1::EntityFingerprint {
        cadpole_proto::v1::EntityFingerprint {
            surface_kind: self.surface_kind.clone(),
            centroid: Some(self.centroid.to_proto()),
            normal: Some(self.normal.to_proto()),
            area: self.area,
            length: self.length,
        }
    }

    pub fn from_proto(p: &cadpole_proto::v1::EntityFingerprint) -> Fingerprint {
        let v = |o: &Option<cadpole_proto::v1::Vec3>| o.as_ref().map(|v| Vec3::new(v.x, v.y, v.z)).unwrap_or(Vec3::ZERO);
        Fingerprint {
            surface_kind: p.surface_kind.clone(),
            centroid: v(&p.centroid),
            normal: v(&p.normal),
            area: p.area,
            length: p.length,
        }
    }

    /// Same kind, centroid within `max(1 mm, 2 % of the body
    /// diagonal)`, normal within 5°, area (or length) within 25 %. It
    /// deliberately under-matches — a wrong silent match is worse than an
    /// honest "stale".
    pub fn matches(&self, other: &Fingerprint, diagonal: f64) -> bool {
        if !self.surface_kind.is_empty()
            && !other.surface_kind.is_empty()
            && self.surface_kind != other.surface_kind
        {
            return false;
        }
        let slop = (diagonal * 0.02).max(1.0);
        if (self.centroid - other.centroid).length() > slop {
            return false;
        }
        let (a, b) = (self.normal.normalized(), other.normal.normalized());
        if a.length() > 0.5 && b.length() > 0.5 && a.dot(b) < fmath::cos(5.0f64.to_radians()) {
            return false;
        }
        for (x, y) in [(self.area, other.area), (self.length, other.length)] {
            if x > 0.0 || y > 0.0 {
                let big = x.max(y);
                if big > 0.0 && (x - y).abs() / big > 0.25 {
                    return false;
                }
            }
        }
        true
    }
}

/// Something a stored reference can name.
pub trait Named {
    fn name(&self) -> &str;
    fn aliases(&self) -> &[String];
    fn fingerprint(&self) -> Fingerprint;
}

/// What resolving a stored reference produced.
#[derive(Clone, Debug, PartialEq)]
pub enum Resolution {
    /// Exactly one entity, found by name (or by the `face:N` index form).
    Ok(usize),
    /// The name resolves to several entities — a face that was split, or a
    /// group name such as `cylinder_1/side`. Never a silent pick.
    Ambiguous(Vec<usize>),
    /// Not found by name, but exactly one entity matches the fingerprint.
    Moved(usize),
    /// Not found. The reference is kept and flagged, never dropped.
    Stale,
}

/// Resolve `query` against `items`. `feature` lets a short local name (`"end"`)
/// address a face of that feature first, and failing that the one face of
/// any feature with that local name; `fingerprint` is the geometric fallback, and
/// `kind` enables the `face:N` / `edge:N` compatibility form.
pub fn resolve<T: Named>(
    items: &[T],
    kind: Kind,
    feature: &str,
    query: &str,
    fingerprint: Option<&Fingerprint>,
    diagonal: f64,
) -> Resolution {
    if query.is_empty() {
        return Resolution::Stale;
    }
    let qualified = if query.contains('/') { None } else { Some(full_name(feature, query)) };
    let hit = |pred: &dyn Fn(&T) -> bool| -> Vec<usize> {
        items.iter().enumerate().filter(|(_, it)| pred(it)).map(|(i, _)| i).collect()
    };
    for want in [Some(query.to_owned()), qualified].into_iter().flatten() {
        let by_name = hit(&|it: &T| it.name() == want);
        let found = if by_name.is_empty() {
            hit(&|it: &T| it.aliases().iter().any(|a| *a == want))
        } else {
            by_name
        };
        match found.len() {
            0 => {}
            1 => return Resolution::Ok(found[0]),
            _ => return Resolution::Ambiguous(found),
        }
    }
    // A short name the body's own feature does not own: faces keep their
    // generator's names through moves and booleans, so
    // on `translate(blk)` or `subtract(blk, hole)` the `top` the script
    // means is `blk/top`. Match it against the local part of every name,
    // then of every alias; two generators owning it is ambiguous. Only for
    // a script's lookup: a stored reference carries a fingerprint, and one
    // whose own face is gone is decided by it (`moved` or `stale`), never
    // rebound to another generator's face of the same local name.
    if fingerprint.is_none() && !query.contains('/') {
        let local = |n: &str| n.contains('/') && local_of(n) == query;
        let by_name = hit(&|it: &T| local(it.name()));
        let found = if by_name.is_empty() { hit(&|it: &T| it.aliases().iter().any(|a| local(a))) } else { by_name };
        match found.len() {
            0 => {}
            1 => return Resolution::Ok(found[0]),
            _ => return Resolution::Ambiguous(found),
        }
    }
    // Compatibility: the kernel-order index form.
    if let Some(i) = kind.parse_index(query) {
        if i < items.len() {
            return Resolution::Ok(i);
        }
    }
    if let Some(fp) = fingerprint {
        let cand: Vec<usize> = items
            .iter()
            .enumerate()
            .filter(|(_, it)| fp.matches(&it.fingerprint(), diagonal))
            .map(|(i, _)| i)
            .collect();
        if cand.len() == 1 {
            return Resolution::Moved(cand[0]);
        }
    }
    Resolution::Stale
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::sketch::{Loop, P2};

    /// SplitMix64, so a failing perturbation can be reproduced.
    struct Rng(u64);

    impl Rng {
        fn next(&mut self) -> f64 {
            self.0 = self.0.wrapping_add(0x9E37_79B9_7F4A_7C15);
            let mut z = self.0;
            z = (z ^ (z >> 30)).wrapping_mul(0xBF58_476D_1CE4_E5B9);
            z = (z ^ (z >> 27)).wrapping_mul(0x94D0_49BB_1331_11EB);
            z ^= z >> 31;
            (z >> 11) as f64 * (1.0 / (1u64 << 53) as f64) * 2.0 - 1.0
        }
        fn jitter(&mut self, amount: f64) -> Vec3 {
            Vec3::new(self.next() * amount, self.next() * amount, self.next() * amount)
        }
    }

    /// A gap-based tie is a total order: the same items in any input order
    /// come out the same way, and a value within the floor of its neighbour
    /// is the same value.
    #[test]
    fn gapped_order_is_a_total_order_with_ties() {
        let a = [0.0, 1.0, 1.005, 5.0];
        let b = [3.0, 2.0, 1.0, 0.0];
        let k0 = |i: usize| a[i];
        let k1 = |i: usize| b[i];
        // 1.0 and 1.005 tie on the first key at a floor of 0.01 and fall to
        // the second, which puts index 2 before index 1.
        assert_eq!(gapped_order(4, &[&k0, &k1], 0.01), vec![0, 2, 1, 3]);
        // At a floor of 0.001 they do not tie.
        assert_eq!(gapped_order(4, &[&k0, &k1], 0.001), vec![0, 1, 2, 3]);
        // Ties are transitive along a chain: 0, 0.008, 0.016 are one tie at
        // 0.01 even though the ends are 0.016 apart, so the second key decides.
        let c = [0.016, 0.0, 0.008];
        let d = [2.0, 0.0, 1.0];
        assert_eq!(gapped_order(3, &[&|i| c[i], &|i| d[i]], 0.01), vec![1, 2, 0]);
        // Nothing left to compare: the given order stands, not the noisy key's.
        assert_eq!(gapped_order(3, &[&|i| c[i]], 0.01), vec![0, 1, 2]);
        // The same items in another order come out the same way.
        let c2 = [0.008, 0.016, 0.0];
        let d2 = [1.0, 2.0, 0.0];
        let o = gapped_order(3, &[&|i| c2[i], &|i| d2[i]], 0.01);
        assert_eq!(o.iter().map(|&i| d2[i]).collect::<Vec<_>>(), vec![0.0, 1.0, 2.0]);
    }

    /// Two pieces symmetric about the frame's first axis -- the compressor
    /// wheel's `bore_cut/side[0.0]` -- are ordered by the component that
    /// separates them, and half a millimetre of noise on every component
    /// (the shift a face's centroid took when the other platform gave it
    /// five more triangles) never swaps them. Nor does a tenth of a percent
    /// on the areas, nor which piece is the larger.
    #[test]
    fn order_pieces_survives_tessellation_noise() {
        let mut rng = Rng(0xD185);
        let n = Vec3::new(0.0, 1.0, 0.0);
        // Nominally x = 0 for both; separated along z (the frame's w) by 6 mm.
        let base = [(Vec3::new(-0.0006, 10.0, 3.17), n, 720.9), (Vec3::new(-0.0002, 10.0, -3.17), n, 721.4)];
        let want = order_pieces(&base);
        for trial in 0..2000 {
            let mut pieces = base;
            for p in pieces.iter_mut() {
                p.0 = p.0 + rng.jitter(0.5);
                p.2 *= 1.0 + rng.next() * 1e-3;
            }
            assert_eq!(order_pieces(&pieces), want, "trial {trial}: {pieces:?}");
            // And in the other input order the answer names the same piece first.
            let swapped = [pieces[1], pieces[0]];
            let o = order_pieces(&swapped);
            assert_eq!(swapped[o[0]].0.z.signum(), pieces[want[0]].0.z.signum(), "trial {trial} swapped");
        }
    }

    /// Three pieces along the frame's first axis, 20 mm apart, keep their
    /// order under the same noise.
    #[test]
    fn order_pieces_keeps_a_clear_separation_under_noise() {
        let mut rng = Rng(0x5EED);
        let n = Vec3::new(0.0, 0.0, 1.0);
        let base = [(Vec3::new(40.0, 0.0, 0.0), n, 100.0), (Vec3::new(0.0, 0.0, 0.0), n, 100.0), (Vec3::new(20.0, 0.0, 0.0), n, 100.0)];
        assert_eq!(order_pieces(&base), vec![1, 2, 0]);
        for trial in 0..2000 {
            let mut pieces = base;
            for p in pieces.iter_mut() {
                p.0 = p.0 + rng.jitter(0.5);
                p.2 *= 1.0 + rng.next() * 1e-3;
            }
            assert_eq!(order_pieces(&pieces), vec![1, 2, 0], "trial {trial}: {pieces:?}");
        }
    }

    /// Three equal pieces 120 degrees apart around an axis -- the vase's
    /// `seg[0.9]`, whose normals summed to nothing useful and whose "largest"
    /// piece was a matter of ulps -- are ordered by the axis that spreads
    /// them most, and neither the normals nor which area is a hair larger
    /// can move that.
    #[test]
    fn order_pieces_orders_a_symmetric_group_by_the_axis_that_separates_it() {
        let r = 2.4659;
        let at = |deg: f64| Vec3::new(r * fmath::cos(deg.to_radians()), r * fmath::sin(deg.to_radians()), 2.0);
        let want = vec![1, 2, 0]; // y: -2.14, 0, +2.14 for pieces at 60, 180, -60 degrees
        let mut rng = Rng(0xA5E);
        for trial in 0..500 {
            let mut pieces = [(at(60.0), at(60.0), 25.06), (at(-60.0), at(-60.0), 25.06), (at(180.0), at(180.0), 25.06)];
            for p in pieces.iter_mut() {
                p.0 = p.0 + rng.jitter(0.05);
                p.2 += rng.next() * 1e-9;
            }
            assert_eq!(order_pieces(&pieces), want, "trial {trial}: {pieces:?}");
            let rotated = [pieces[2], pieces[0], pieces[1]];
            let o = order_pieces(&rotated);
            assert!((rotated[o[0]].0.y - pieces[1].0.y).abs() < 0.2, "trial {trial} rotated input");
        }
    }

    /// Two halves of one circle share both endpoints and, nominally, their
    /// length; the arc-length midpoints are a diameter apart, and that is
    /// what orders them, whatever the tessellation did to the lengths.
    #[test]
    fn order_edges_tells_two_halves_of_a_circle_apart_by_midpoint() {
        let half = |sign: f64, segs: usize| -> Vec<Vec3> {
            (0..=segs)
                .map(|i| {
                    let t = std::f64::consts::PI * i as f64 / segs as f64;
                    Vec3::new(10.0 * fmath::cos(t), sign * 10.0 * fmath::sin(t), 3.0)
                })
                .collect()
        };
        let want = order_edges(&[EdgeGeom::from_polyline(&half(1.0, 37)), EdgeGeom::from_polyline(&half(-1.0, 42))]);
        assert_eq!(want, vec![1, 0], "the y-negative half has the lower midpoint");
        // The same two arcs at other resolutions -- lengths move, order does not.
        for (a, b) in [(9usize, 40usize), (100, 12), (37, 37), (5, 6)] {
            let got = order_edges(&[EdgeGeom::from_polyline(&half(1.0, a)), EdgeGeom::from_polyline(&half(-1.0, b))]);
            assert_eq!(got, want, "{a} vs {b} segments");
        }
        let mid = polyline_midpoint(&half(1.0, 8), EdgeGeom::from_polyline(&half(1.0, 8)).length);
        assert!((mid - Vec3::new(0.0, 10.0, 3.0)).length() < 0.2, "{mid:?}");
    }

    fn probe(kind: &'static str, c: Vec3, n: Vec3, area: f64, samples: &[Vec3]) -> FaceProbe {
        FaceProbe { surface_kind: kind, centroid: c, normal: n, area, samples: samples.to_vec() }
    }

    /// A unit box extruded from a rect sketch, described the way the
    /// tessellator would describe it — but with no kernel anywhere.
    fn box_probes(h: f64) -> Vec<FaceProbe> {
        let s = |x: f64, y: f64, z: f64| Vec3::new(x, y, z);
        vec![
            probe("plane", s(0.0, 0.0, 0.0), -Vec3::Z, 100.0, &[s(-3.0, -3.0, 0.0), s(3.0, 3.0, 0.0)]),
            probe("plane", s(0.0, 0.0, h), Vec3::Z, 100.0, &[s(-3.0, -3.0, h), s(3.0, 3.0, h)]),
            probe("plane", s(0.0, -5.0, h / 2.0), -Vec3::Y, 50.0, &[s(-2.0, -5.0, 1.0), s(2.0, -5.0, 2.0)]),
            probe("plane", s(5.0, 0.0, h / 2.0), Vec3::X, 50.0, &[s(5.0, -2.0, 1.0), s(5.0, 2.0, 2.0)]),
            probe("plane", s(0.0, 5.0, h / 2.0), Vec3::Y, 50.0, &[s(-2.0, 5.0, 1.0), s(2.0, 5.0, 2.0)]),
            probe("plane", s(-5.0, 0.0, h / 2.0), -Vec3::X, 50.0, &[s(-5.0, -2.0, 1.0), s(-5.0, 2.0, 2.0)]),
        ]
    }

    #[test]
    fn an_extrude_names_its_caps_and_one_side_per_segment() {
        let sk = Sketch::new(Plane::XY).with_loop(Loop::rect(P2::new(0.0, 0.0), 10.0, 10.0));
        let g = Generator::extrude(&sk, 4.0, "extrude");
        let names: Vec<String> = name_faces(&g, "extrude_1", &box_probes(4.0)).into_iter().map(|n| n.name).collect();
        assert_eq!(names[0], "extrude_1/start");
        assert_eq!(names[1], "extrude_1/end");
        // Loop::rect runs -Y, +X, +Y, -X, so the walls are segments 0..3.
        assert_eq!(names[2], "extrude_1/side[0.0]");
        assert_eq!(names[3], "extrude_1/side[0.1]");
        assert_eq!(names[4], "extrude_1/side[0.2]");
        assert_eq!(names[5], "extrude_1/side[0.3]");
    }

    #[test]
    fn a_box_primitive_gets_meaningful_names_and_keeps_the_generic_ones() {
        let sk = Sketch::new(Plane::XY).with_loop(Loop::rect(P2::new(0.0, 0.0), 10.0, 10.0));
        let g = Generator::extrude(&sk, 4.0, "box");
        let named = name_faces(&g, "box_1", &box_probes(4.0));
        assert_eq!(named[1].name, "box_1/top");
        assert!(named[1].aliases.contains(&"box_1/end".to_owned()));
        assert_eq!(named[0].name, "box_1/bottom");
        assert_eq!(named[2].name, "box_1/front");
        assert_eq!(named[3].name, "box_1/right");
        assert_eq!(named[4].name, "box_1/back");
        assert_eq!(named[5].name, "box_1/left");
        assert!(named[2].aliases.contains(&"box_1/side[0.0]".to_owned()));
    }

    #[test]
    fn a_side_keeps_its_name_when_an_earlier_loop_gains_a_segment() {
        // The same wall, in a sketch whose *other* loop changed: the loop and
        // segment indices of loop 0 do not move, so neither does the name.
        let one = Sketch::new(Plane::XY).with_loop(Loop::rect(P2::new(0.0, 0.0), 10.0, 10.0));
        let two = one.with_loop(Loop::circle(P2::new(0.0, 0.0), 2.0));
        let p = box_probes(4.0);
        let a = name_faces(&Generator::extrude(&one, 4.0, "extrude"), "e", &p);
        let b = name_faces(&Generator::extrude(&two, 4.0, "extrude"), "e", &p);
        assert_eq!(
            a.iter().map(|n| n.name.clone()).collect::<Vec<_>>(),
            b.iter().map(|n| n.name.clone()).collect::<Vec<_>>()
        );
    }

    #[test]
    fn names_are_joined_canonically_and_share_a_prefix() {
        assert_eq!(join_names(&["lid/side[0.2]".into(), "lid/end".into()]), "lid/end|side[0.2]");
        assert_eq!(join_names(&["hole/side[0.0]".into(), "block/end".into()]), "block/end|hole/side[0.0]");
        assert_eq!(edge_name(&["a/end".into()]), "a/end|end");
        assert_eq!(join_names(&[]), "");
    }

    #[test]
    fn pieces_are_ordered_by_their_place_on_the_surface_not_by_area() {
        // Three coplanar pieces of one z = 0 face, handed over out of order.
        let pieces = [
            (Vec3::new(5.0, 0.0, 0.0), Vec3::Z, 1.0),
            (Vec3::new(-5.0, 0.0, 0.0), Vec3::Z, 9.0),
            (Vec3::new(0.0, 0.0, 0.0), Vec3::Z, 4.0),
        ];
        let order = order_pieces(&pieces);
        let xs: Vec<f64> = order.iter().map(|&i| pieces[i].0.x).collect();
        assert_eq!(xs, vec![-5.0, 0.0, 5.0]);
        // Flipping the normal must not change the order.
        let flipped: Vec<(Vec3, Vec3, f64)> = pieces.iter().map(|p| (p.0, -p.1, p.2)).collect();
        assert_eq!(order_pieces(&flipped), order);
    }

    #[test]
    fn a_catalogue_carries_a_source_name_onto_a_face_that_lies_on_it() {
        let mut cat = SurfaceCatalogue::new(0.1);
        let z = |x: f64, y: f64| Vec3::new(x, y, 0.0);
        cat.add_face(
            NamedFace::new("a/end", Role::New),
            [[z(-10.0, -10.0), z(10.0, -10.0), z(10.0, 10.0)], [z(-10.0, -10.0), z(10.0, 10.0), z(-10.0, 10.0)]],
        );
        cat.add_face(
            NamedFace::new("b/start", Role::New),
            [[
                Vec3::new(-10.0, -10.0, 5.0),
                Vec3::new(10.0, -10.0, 5.0),
                Vec3::new(10.0, 10.0, 5.0),
            ]],
        );
        cat.finish();
        let on_a = probe("plane", z(0.0, 0.0), Vec3::Z, 400.0, &[z(1.0, 1.0), z(-4.0, 2.0), z(3.0, -6.0)]);
        let nowhere = probe(
            "plane",
            Vec3::new(0.0, 0.0, 50.0),
            Vec3::Z,
            1.0,
            &[Vec3::new(0.0, 0.0, 50.0), Vec3::new(1.0, 0.0, 50.0)],
        );
        let named = name_inherited(&cat, "sub_1", &[on_a, nowhere]);
        assert_eq!(named[0].name, "a/end");
        assert_eq!(named[0].role, Role::Kept);
        assert_eq!(named[1].name, "sub_1/face[1]");
        assert_eq!(named[1].role, Role::Unmatched);
    }

    #[test]
    fn a_face_covering_two_sources_carries_both_names() {
        let mut cat = SurfaceCatalogue::new(0.1);
        let z = |x: f64, y: f64| Vec3::new(x, y, 0.0);
        cat.add_face(NamedFace::new("a/end", Role::New), [[z(-10.0, -10.0), z(0.0, -10.0), z(0.0, 10.0)]]);
        cat.add_face(NamedFace::new("b/end", Role::New), [[z(0.0, -10.0), z(10.0, -10.0), z(10.0, 10.0)]]);
        cat.finish();
        let merged = probe(
            "plane",
            z(0.0, 0.0),
            Vec3::Z,
            400.0,
            &[z(-5.0, -5.0), z(-3.0, 0.0), z(5.0, -5.0), z(8.0, -2.0)],
        );
        let named = name_inherited(&cat, "u_1", &[merged]);
        assert_eq!(named[0].role, Role::Merged);
        assert!(named[0].name == "a/end" || named[0].name == "b/end");
        assert_eq!(named[0].aliases.len(), 1);
    }

    struct Item {
        name: String,
        aliases: Vec<String>,
        fp: Fingerprint,
    }
    impl Named for Item {
        fn name(&self) -> &str {
            &self.name
        }
        fn aliases(&self) -> &[String] {
            &self.aliases
        }
        fn fingerprint(&self) -> Fingerprint {
            self.fp.clone()
        }
    }

    fn item(name: &str, aliases: &[&str], c: Vec3, area: f64) -> Item {
        Item {
            name: name.into(),
            aliases: aliases.iter().map(|s| s.to_string()).collect(),
            fp: Fingerprint {
                surface_kind: "plane".into(),
                centroid: c,
                normal: Vec3::Z,
                area,
                length: 0.0,
            },
        }
    }

    /// A chamfer's cutter names its faces by the surfaces they lie on: the
    /// probe on the 45° plane is `face[<edge>]`, the ones on the legs are
    /// `leg[<edge>]`, and a probe on none of them is the fallback.
    /// The edge grammar then reads a chamfer face's feature prefix off the
    /// front of its name, in front of the edge name it carries.
    #[test]
    fn a_surfaces_generator_names_a_probe_by_the_surface_it_lies_on() {
        let s = 0.5f64.sqrt();
        let faces = vec![
            (
                SurfaceClass::Plane { origin: Vec3::new(1.0, 0.0, 0.0), normal: Vec3::new(s, 0.0, s) },
                "c/face[b/right|top]".to_owned(),
            ),
            (SurfaceClass::Plane { origin: Vec3::new(0.0, 0.0, 0.4), normal: Vec3::Z }, "c/leg[b/right|top]".to_owned()),
            (
                SurfaceClass::Cylinder { axis_point: Vec3::ZERO, axis: Vec3::Z, radius: 3.0 },
                "c/leg[b/right|top]".to_owned(),
            ),
        ];
        let g = Generator::Surfaces { faces, tol: 1e-6 };
        let probe = |c: Vec3, n: Vec3, samples: Vec<Vec3>| FaceProbe {
            surface_kind: "plane",
            centroid: c,
            normal: n,
            area: 1.0,
            samples,
        };
        let probes = vec![
            // On the chamfer plane x + z = 1, facing the other way: still that plane.
            probe(
                Vec3::new(0.5, 0.0, 0.5),
                Vec3::new(-s, 0.0, -s),
                vec![Vec3::new(0.2, 1.0, 0.8), Vec3::new(0.9, -2.0, 0.1)],
            ),
            probe(Vec3::new(5.0, 5.0, 0.4), Vec3::Z, vec![Vec3::new(4.0, 6.0, 0.4)]),
            probe(Vec3::new(3.0, 0.0, 2.0), Vec3::X, vec![Vec3::new(0.0, 3.0, -1.0), Vec3::new(-3.0, 0.0, 5.0)]),
            // On the leg's plane by centroid but not by every sample: on none.
            probe(Vec3::new(5.0, 5.0, 0.4), Vec3::Z, vec![Vec3::new(4.0, 6.0, 0.4), Vec3::new(4.0, 6.0, 1.4)]),
        ];
        let named = name_faces(&g, "c", &probes);
        assert_eq!(named[0], NamedFace::new("c/face[b/right|top]", Role::New));
        assert_eq!(named[1], NamedFace::new("c/leg[b/right|top]", Role::New));
        assert_eq!(named[2], NamedFace::new("c/leg[b/right|top]", Role::New));
        assert_eq!(named[3], NamedFace::new("c/face[3]", Role::Unmatched));
        assert_eq!(feature_of("c/face[b/right|top]"), "c");
        assert_eq!(local_of("c/face[b/right|top]"), "face[b/right|top]");
        assert_eq!(
            edge_name(&["c/face[b/right|top]".into(), "c/face[b/front|top]".into()]),
            "c/face[b/front|top]|face[b/right|top]"
        );
        assert_eq!(edge_name(&["c/face[b/right|top]".into(), "b/top".into()]), "b/top|c/face[b/right|top]");
    }

    #[test]
    fn resolution_reports_ok_ambiguous_moved_and_stale() {
        let items = vec![
            item("box_1/top", &["box_1/end"], Vec3::new(0.0, 0.0, 10.0), 100.0),
            item("cyl_1/side[0.0]", &["cyl_1/side"], Vec3::new(1.0, 0.0, 5.0), 50.0),
            item("cyl_1/side[0.1]", &["cyl_1/side"], Vec3::new(-1.0, 0.0, 5.0), 50.0),
        ];
        let d = 40.0;
        assert_eq!(resolve(&items, Kind::Face, "box_1", "box_1/top", None, d), Resolution::Ok(0));
        // A short local name, and the generic alias.
        assert_eq!(resolve(&items, Kind::Face, "box_1", "top", None, d), Resolution::Ok(0));
        assert_eq!(resolve(&items, Kind::Face, "box_1", "box_1/end", None, d), Resolution::Ok(0));
        // A group name is ambiguous, never a silent pick.
        assert_eq!(resolve(&items, Kind::Face, "cyl_1", "cyl_1/side", None, d), Resolution::Ambiguous(vec![1, 2]));
        // The compatibility index form still addresses.
        assert_eq!(resolve(&items, Kind::Face, "", "face:2", None, d), Resolution::Ok(2));
        assert_eq!(resolve(&items, Kind::Face, "", "face:9", None, d), Resolution::Stale);
        // Renamed but geometrically the same face: moved, with the fingerprint.
        let fp = Fingerprint {
            surface_kind: "plane".into(),
            centroid: Vec3::new(0.0, 0.0, 10.2),
            normal: Vec3::Z,
            area: 105.0,
            length: 0.0,
        };
        assert_eq!(resolve(&items, Kind::Face, "old", "old/end", Some(&fp), d), Resolution::Moved(0));
        // Too far to be the same face: stale, not a wrong match.
        let far = Fingerprint { centroid: Vec3::new(0.0, 0.0, 30.0), ..fp };
        assert_eq!(resolve(&items, Kind::Face, "old", "old/end", Some(&far), d), Resolution::Stale);
    }

    /// Names are paths from the generator, never from the last transform or
    /// boolean, so a short local name has to keep resolving on a body whose
    /// own feature (`translate_1`, `part`) made none of its faces: it
    /// matches the local part of the names, and two features owning the same
    /// local name is ambiguous rather than a pick.
    #[test]
    fn a_short_name_resolves_on_a_body_its_generator_did_not_make_last() {
        let items = vec![
            item("blk/top", &["blk/end"], Vec3::new(0.0, 0.0, 10.0), 100.0),
            item("blk/bottom", &["blk/start"], Vec3::new(0.0, 0.0, 0.0), 100.0),
            item("hole/side#0", &["hole/side"], Vec3::new(1.0, 0.0, 5.0), 50.0),
            item("hole/side#1", &["hole/side"], Vec3::new(-1.0, 0.0, 5.0), 50.0),
            item("lid/top", &[], Vec3::new(0.0, 0.0, 20.0), 100.0),
        ];
        let d = 40.0;
        assert_eq!(resolve(&items, Kind::Face, "translate_1", "bottom", None, d), Resolution::Ok(1));
        assert_eq!(resolve(&items, Kind::Face, "part", "start", None, d), Resolution::Ok(1), "the generic alias");
        assert_eq!(resolve(&items, Kind::Face, "part", "side#1", None, d), Resolution::Ok(3));
        assert_eq!(resolve(&items, Kind::Face, "part", "side", None, d), Resolution::Ambiguous(vec![2, 3]));
        // Two generators own a `top`: say so.
        assert_eq!(resolve(&items, Kind::Face, "part", "top", None, d), Resolution::Ambiguous(vec![0, 4]));
        // The body's own feature still wins when it has the name.
        assert_eq!(resolve(&items, Kind::Face, "lid", "top", None, d), Resolution::Ok(4));
        assert_eq!(resolve(&items, Kind::Face, "part", "nosuch", None, d), Resolution::Stale);
        // A stored reference (it has a fingerprint) to `lid/end` when the lid
        // no longer has an `end` is not rebound to `blk/end`: the
        // fingerprint decides, and a face nowhere near it is stale.
        let far = Fingerprint {
            surface_kind: "plane".into(),
            centroid: Vec3::new(500.0, 500.0, 500.0),
            normal: Vec3::Z,
            area: 1.0,
            length: 0.0,
        };
        assert_eq!(resolve(&items, Kind::Face, "lid", "end", Some(&far), d), Resolution::Stale);
    }

    /// The fallback has four criteria — kind, centroid, normal, area — and
    /// `matches` deliberately under-matches on every one of them. The test
    /// above moves a face that differs only in *centroid*. A
    /// wrong silent match is worse than an honest stale, so each criterion
    /// gets the case where it is the only thing standing between the two.
    #[test]
    fn a_fingerprint_that_differs_only_in_normal_area_or_kind_is_stale() {
        let items = vec![item("box_1/top", &["box_1/end"], Vec3::new(0.0, 0.0, 10.0), 100.0)];
        let d = 40.0;
        let at = |deg: f64| Vec3::new(0.0, fmath::sin(deg.to_radians()), fmath::cos(deg.to_radians()));
        // The control: same kind, same normal, area within 5 %, centroid
        // within the slop. This one has to move, or the negatives below prove
        // nothing.
        let same = Fingerprint {
            surface_kind: "plane".into(),
            centroid: Vec3::new(0.0, 0.0, 10.2),
            normal: Vec3::Z,
            area: 105.0,
            length: 0.0,
        };
        let against = |fp: &Fingerprint| resolve(&items, Kind::Face, "old", "old/end", Some(fp), d);
        assert_eq!(against(&same), Resolution::Moved(0), "the control must match");

        // A normal 30° away: the same patch of space, not the same face.
        let tilted = Fingerprint { normal: at(30.0), ..same.clone() };
        assert_eq!(against(&tilted), Resolution::Stale, "a normal 30° away is not within 5°");
        // ...and 4° still matches, so this is the 5° tolerance
        // and not an equality test that happens to reject 30°.
        let nudged = Fingerprint { normal: at(4.0), ..same.clone() };
        assert_eq!(against(&nudged), Resolution::Moved(0), "4° is inside the 5° tolerance");

        // Twice the area: over the 25 % tolerance.
        assert_eq!(
            against(&Fingerprint { area: 200.0, ..same.clone() }),
            Resolution::Stale,
            "twice the area is not the same face"
        );
        // ...and 20 % under it is still the same face.
        assert_eq!(
            against(&Fingerprint { area: 120.0, ..same.clone() }),
            Resolution::Moved(0),
            "20 % is inside the 25 % tolerance"
        );

        // A cylinder is not a plane, however well the numbers line up.
        assert_eq!(
            against(&Fingerprint { surface_kind: "cylinder".into(), ..same.clone() }),
            Resolution::Stale,
            "a cylinder is not the plane that used to be there"
        );
        // An empty kind is "unknown", not "different": a fingerprint stored
        // before the field existed still matches on the other three.
        assert_eq!(
            against(&Fingerprint { surface_kind: String::new(), ..same.clone() }),
            Resolution::Moved(0),
            "an unknown kind must not block a match"
        );
    }

    /// `face:2` addresses a face and nothing else. The index form is a
    /// compatibility shim, and one that accepted any prefix
    /// would let a stored `face:2` address *edge* 2 the moment a caller asked
    /// for an edge — a silent rebinding onto a different entity entirely.
    #[test]
    fn the_index_form_only_parses_its_own_kind() {
        assert_eq!(Kind::Face.parse_index("face:2"), Some(2));
        assert_eq!(Kind::Edge.parse_index("edge:2"), Some(2));
        assert_eq!(Kind::Vertex.parse_index("vertex:2"), Some(2));
        assert_eq!(Kind::Edge.parse_index("face:2"), None, "a face index must not address an edge");
        assert_eq!(Kind::Face.parse_index("edge:2"), None);
        assert_eq!(Kind::Vertex.parse_index("face:2"), None);
        // Neither a generated name nor a malformed index is an index.
        assert_eq!(Kind::Face.parse_index("blk/top"), None);
        assert_eq!(Kind::Face.parse_index("face:"), None);
        assert_eq!(Kind::Face.parse_index("face:-1"), None);
        assert_eq!(Kind::Face.parse_index("face:x"), None);
        // ...and the round trip with `index_name` holds for each kind.
        for k in [Kind::Face, Kind::Edge, Kind::Vertex] {
            assert_eq!(k.parse_index(&k.index_name(7)), Some(7));
        }
    }

    #[test]
    fn point_triangle_distance_is_right_in_every_region() {
        let a = Vec3::ZERO;
        let b = Vec3::new(1.0, 0.0, 0.0);
        let c = Vec3::new(0.0, 1.0, 0.0);
        assert!(point_triangle_distance(Vec3::new(0.2, 0.2, 3.0), a, b, c) - 3.0 < 1e-9);
        assert!((point_triangle_distance(Vec3::new(-1.0, -1.0, 0.0), a, b, c) - 2.0f64.sqrt()).abs() < 1e-9);
        assert!((point_triangle_distance(Vec3::new(0.5, -1.0, 0.0), a, b, c) - 1.0).abs() < 1e-9);
        assert!(point_triangle_distance(Vec3::new(0.25, 0.25, 0.0), a, b, c) < 1e-9);
    }
}
