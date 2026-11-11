//! Geometric selection: asking a body for entities by *what they are*
//! (docs/design.md), rather than by an index into the kernel's output order.
//!
//! The grammar of names lives in [`crate::naming`]; this module is the other
//! half — the filters a script composes and the order the results come
//! back in. It knows nothing about Starlark: `cadpole-script` turns keywords into
//! [`Filter`]s and renders [`Outcome::reason`] as a diagnostic.
//!
//! **Order is a pure function of the geometry.** Kernel output order is
//! reproducible but it is not *stable*: a parameter change can
//! renumber it. Every selection is therefore sorted by [`sort_key`] — the
//! entity's quantised centroid, then its lower bounding-box corner, then its
//! size, then its generated name — so the same model always yields the same
//! order, and a small parameter change moves entities smoothly rather than
//! shuffling them. Nothing here ever reads a `Vec` index, a hash iteration
//! order or a tessellation order as data.

use crate::geom::{Plane, Vec3};
use crate::naming::{self, quantize as q};
use crate::tessellate::{EdgeInfo, FaceInfo, Mesh, Topology};
use crate::fmath;

/// Default angular tolerance, in degrees (`within=`).
pub const DEFAULT_WITHIN_DEG: f64 = 5.0;
/// Default length tolerance, in millimetres (`tolerance=`). One micron: the
/// caps and walls a feature generates land on their plane exactly, so this is
/// float slop, not a modelling fudge.
pub const DEFAULT_TOLERANCE_MM: f64 = 1e-3;

/// How many of a face's triangles are sampled for the tests that need points
/// *on* the entity. Planar faces short-circuit to an exact test, so this only
/// bounds the curved case.
const FACE_SAMPLE_TRIS: usize = 256;

/// Relative residual a circle fit must beat before an edge is called an arc.
const CIRCLE_FIT_TOLERANCE: f64 = 1e-4;

/// The two tolerances, kept apart on purpose: `within=` is degrees and
/// `tolerance=` is millimetres, everywhere.
#[derive(Clone, Copy, Debug)]
pub struct Tol {
    pub within_deg: f64,
    pub mm: f64,
}

impl Default for Tol {
    fn default() -> Tol {
        Tol { within_deg: DEFAULT_WITHIN_DEG, mm: DEFAULT_TOLERANCE_MM }
    }
}

/// Which side of the topology a selection is over.
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub enum Which {
    Faces,
    Edges,
}

impl Which {
    pub fn singular(self) -> &'static str {
        match self {
            Which::Faces => "face",
            Which::Edges => "edge",
        }
    }
    pub fn plural(self) -> &'static str {
        match self {
            Which::Faces => "faces",
            Which::Edges => "edges",
        }
    }
}

/// One entity as a filter sees it. Built from the tessellated topology, but
/// carrying nothing kernel-specific, so a test can write one by hand.
#[derive(Clone, Debug, Default)]
pub struct Candidate {
    /// Index into `Topology::faces` / `Topology::edges`.
    pub index: usize,
    pub name: String,
    pub aliases: Vec<String>,
    /// `FaceInfo::surface_kind` or `EdgeInfo::curve_kind`, plus the derived
    /// classes this module can measure for itself (`circle`, `arc`).
    pub class: String,
    /// A face's recognised analytic class (`FaceInfo::surface_class`):
    /// `plane`, `cylinder`, `cone`, `sphere`, `torus` or `other`. Empty for
    /// an edge. `kind=` matches either this or `class`, so the truck-level
    /// string still selects what it always did.
    pub surface_class: String,
    /// Face centroid, or an edge's polyline midpoint.
    pub centroid: Vec3,
    /// A face's outward normal, or a straight edge's unit tangent.
    pub direction: Vec3,
    /// False for an edge that is not straight: it has no single tangent, so
    /// `direction=` must not match it rather than match it by its chord.
    pub straight: bool,
    pub area: f64,
    pub length: f64,
    /// Radius of the circle an edge lies on, or 0 when it is not an arc.
    pub radius: f64,
    /// True when the material is on the inside of the fold (edges only).
    pub convex: bool,
    /// Angle between the surfaces meeting at an edge; `-1` when unknown.
    pub dihedral: f64,
    /// Exact axis-aligned bounds, over every point of the entity.
    pub lo: Vec3,
    pub hi: Vec3,
    /// Points *on* the entity, capped. Exact and complete for edges.
    pub points: Vec<Vec3>,
    /// The other side of the topology: a face's bounding edges, or an edge's
    /// adjacent faces.
    pub neighbours: Vec<u32>,
}

impl Candidate {
    /// The measurement a selection sorts largest-first by: area for a face,
    /// length for an edge.
    pub fn size(&self) -> f64 {
        if self.area > 0.0 { self.area } else { self.length }
    }

    fn planar(&self) -> bool {
        self.class == "plane" || self.surface_class == "plane"
    }

    /// Signed distance range of the entity from `plane`, over the points we
    /// have. Exact for edges and for planar faces; sampled otherwise.
    fn span(&self, plane: &Plane) -> (f64, f64) {
        let n = plane.normal.normalized();
        let mut lo = f64::MAX;
        let mut hi = f64::MIN;
        for p in &self.points {
            let d = (*p - plane.origin).dot(n);
            lo = lo.min(d);
            hi = hi.max(d);
        }
        if self.points.is_empty() {
            let d = (self.centroid - plane.origin).dot(n);
            return (d, d);
        }
        (lo, hi)
    }
}

/// One test. `cadpole-script` builds the list from keywords; the order of the
/// list does not affect the result, only the wording of an empty-set reason.
#[derive(Clone, Debug)]
pub enum Filter {
    /// A face whose outward normal, or a straight edge whose tangent, is
    /// within `Tol::within_deg` of `dir`. Faces are **signed** — a normal has
    /// a side, and `faces(normal=(0,0,1))` must not return the bottom. Edges
    /// are **unsigned** — an edge running −X is as much "along X" as one
    /// running +X.
    Direction { dir: Vec3, signed: bool },
    /// `surface_kind` / `curve_kind`, a recognised surface class
    /// (`cylinder`, `cone`, `sphere`, `torus`; a `plane` that truck holds as
    /// a B-spline), or an edge class this module measures.
    Class(String),
    /// `*`/`?` glob over the generated name, its aliases, or its local part,
    /// so `name="side[*]"` works without spelling the feature id.
    Name(String),
    /// Every point of the entity is within `Tol::mm` of the plane.
    OnPlane(Plane),
    /// Every point is on the normal side of the plane (`Tol::mm` of slack).
    Above(Plane),
    /// Every point is on the far side of the plane.
    Below(Plane),
    /// The entity's bounds lie inside the box, grown by `Tol::mm`.
    Inside { lo: Vec3, hi: Vec3 },
    MinArea(f64),
    MaxArea(f64),
    MinLength(f64),
    MaxLength(f64),
    MinRadius(f64),
    MaxRadius(f64),
    /// Edges only: material on the inside of the fold.
    Convex(bool),
    MinDihedral(f64),
    MaxDihedral(f64),
    /// Edges bounding this face index (or faces bounded by this edge index).
    Neighbour(u32),
}

/// Why a candidate did not pass, for the message a script gets when a selector
/// comes back empty. Cheap to build — no allocation — because every rejection
/// produces one, not just the ones that end up in a diagnostic.
#[derive(Clone, Copy, Debug, PartialEq)]
pub enum Miss {
    /// It was the right shape for the test and missed by this much, in this
    /// unit: 12.4 degrees, 3 mm, 40 mm².
    By(f64, &'static str),
    /// It was the wrong shape for the test entirely, so there is no distance
    /// to quote: an arc has no tangent, a boundary edge has no dihedral.
    Because(&'static str),
}

impl Miss {
    /// Ranking key for "the closest miss": a measured miss beats an
    /// unmeasurable one, and a smaller measured miss beats a bigger one.
    fn rank(self) -> (u8, i64) {
        match self {
            Miss::By(a, _) => (0, quantize_miss(a)),
            Miss::Because(_) => (1, 0),
        }
    }
}

/// Quantise a miss for comparison without tripping over NaN or infinity.
fn quantize_miss(a: f64) -> i64 {
    if a.is_nan() {
        i64::MAX
    } else {
        (a.clamp(-1e12, 1e12) * 1e6) as i64
    }
}

impl Filter {
    /// How this filter reads in a diagnostic.
    pub fn describe(&self, tol: &Tol) -> String {
        let v = |d: Vec3| format!("({}, {}, {})", num(d.x), num(d.y), num(d.z));
        let pl = |p: &Plane| format!("origin {} normal {}", v(p.origin), v(p.normal));
        match self {
            Filter::Direction { dir, .. } => {
                format!("direction {} within {}°", v(*dir), num(tol.within_deg))
            }
            Filter::Class(k) => format!("kind {k:?}"),
            Filter::Name(g) => format!("name {g:?}"),
            Filter::OnPlane(p) => format!("on the plane {} (within {} mm)", pl(p), num(tol.mm)),
            Filter::Above(p) => format!("above the plane {}", pl(p)),
            Filter::Below(p) => format!("below the plane {}", pl(p)),
            Filter::Inside { lo, hi } => format!("inside the box {} .. {}", v(*lo), v(*hi)),
            Filter::MinArea(a) => format!("area ≥ {} mm²", num(*a)),
            Filter::MaxArea(a) => format!("area ≤ {} mm²", num(*a)),
            Filter::MinLength(l) => format!("length ≥ {} mm", num(*l)),
            Filter::MaxLength(l) => format!("length ≤ {} mm", num(*l)),
            Filter::MinRadius(r) => format!("radius ≥ {} mm", num(*r)),
            Filter::MaxRadius(r) => format!("radius ≤ {} mm", num(*r)),
            Filter::Convex(true) => "convex".into(),
            Filter::Convex(false) => "concave".into(),
            Filter::MinDihedral(d) => format!("dihedral ≥ {}°", num(*d)),
            Filter::MaxDihedral(d) => format!("dihedral ≤ {}°", num(*d)),
            Filter::Neighbour(i) => format!("bordering entity {i}"),
        }
    }

    /// `Ok` when the candidate passes; `Err(miss)` says how far off it was, so
    /// an empty selection can name its nearest miss.
    pub fn test(&self, c: &Candidate, tol: &Tol) -> Result<(), Miss> {
        let over = |value: f64, limit: f64, unit: &'static str| -> Result<(), Miss> {
            if value >= limit { Ok(()) } else { Err(Miss::By(limit - value, unit)) }
        };
        let under = |value: f64, limit: f64, unit: &'static str| -> Result<(), Miss> {
            if value <= limit { Ok(()) } else { Err(Miss::By(value - limit, unit)) }
        };
        match self {
            Filter::Direction { dir, signed } => {
                if !c.straight {
                    return Err(Miss::Because("it is not straight, so it has no one tangent"));
                }
                let d = dir.normalized();
                let mut dot = c.direction.normalized().dot(d);
                if !signed {
                    dot = dot.abs();
                }
                let angle = fmath::acos(dot.clamp(-1.0, 1.0)).to_degrees();
                under(angle, tol.within_deg, "°")
            }
            Filter::Class(k) => {
                if c.class == *k || c.surface_class == *k {
                    Ok(())
                } else {
                    Err(Miss::Because("it is a different class"))
                }
            }
            Filter::Name(g) => {
                let hit = glob_match(g, &c.name)
                    || glob_match(g, naming::local_of(&c.name))
                    || c.aliases.iter().any(|a| glob_match(g, a) || glob_match(g, naming::local_of(a)));
                if hit { Ok(()) } else { Err(Miss::Because("its name does not match")) }
            }
            Filter::OnPlane(p) => {
                let n = p.normal.normalized();
                if c.planar() {
                    // Exact: a planar face is on the plane when it is parallel
                    // to it and its centroid is on it. No sampling involved.
                    let parallel = c.direction.normalized().dot(n).abs();
                    let angle = fmath::acos(parallel.clamp(-1.0, 1.0)).to_degrees();
                    if angle > tol.within_deg {
                        return Err(Miss::By(angle, "° out of parallel"));
                    }
                }
                let (lo, hi) = c.span(p);
                let off = lo.abs().max(hi.abs());
                under(off, tol.mm, " mm")
            }
            Filter::Above(p) => {
                let (lo, _) = c.span(p);
                over(lo, -tol.mm, " mm")
            }
            Filter::Below(p) => {
                let (_, hi) = c.span(p);
                under(hi, tol.mm, " mm")
            }
            Filter::Inside { lo, hi } => {
                let slack = tol.mm;
                let out = (lo.x - slack - c.lo.x)
                    .max(lo.y - slack - c.lo.y)
                    .max(lo.z - slack - c.lo.z)
                    .max(c.hi.x - hi.x - slack)
                    .max(c.hi.y - hi.y - slack)
                    .max(c.hi.z - hi.z - slack);
                under(out, 0.0, " mm outside")
            }
            Filter::MinArea(a) => over(c.area, *a, " mm²"),
            Filter::MaxArea(a) => under(c.area, *a, " mm²"),
            Filter::MinLength(l) => over(c.length, *l, " mm"),
            Filter::MaxLength(l) => under(c.length, *l, " mm"),
            Filter::MinRadius(r) => {
                if c.radius <= 0.0 {
                    return Err(Miss::Because("it is not an arc, so it has no radius"));
                }
                over(c.radius, *r, " mm")
            }
            Filter::MaxRadius(r) => {
                if c.radius <= 0.0 {
                    return Err(Miss::Because("it is not an arc, so it has no radius"));
                }
                under(c.radius, *r, " mm")
            }
            Filter::Convex(want) => {
                if c.dihedral < 0.0 {
                    return Err(Miss::Because("it has no two adjacent faces, so it folds neither way"));
                }
                if c.convex == *want {
                    Ok(())
                } else {
                    Err(Miss::Because("it folds the other way"))
                }
            }
            Filter::MinDihedral(d) => over(c.dihedral, *d, "°"),
            Filter::MaxDihedral(d) => under(c.dihedral, *d, "°"),
            Filter::Neighbour(i) => {
                if c.neighbours.contains(i) {
                    Ok(())
                } else {
                    Err(Miss::Because("it does not border that face"))
                }
            }
        }
    }
}

/// What a selection produced, and — when it produced nothing — why.
#[derive(Clone, Debug, Default)]
pub struct Outcome {
    /// Indices into `Topology::faces` / `Topology::edges`, in [`sort_key`]
    /// order.
    pub picked: Vec<usize>,
    /// How many entities the filters were offered.
    pub considered: usize,
    /// One sentence naming the filter that rejected everything and the
    /// closest thing to a match, when `picked` is empty and `considered` is
    /// not. `None` when the selection succeeded, or when the body had nothing
    /// to offer in the first place.
    pub reason: Option<String>,
}

/// Run `filters` over `cands` and return the survivors in canonical order.
///
/// An empty result is **not** an error here: `faces(kind="revolved")` on a box
/// legitimately has none, and `len(...) == 0` is a thing a script asserts on.
/// It is the caller's job to decide: `cadpole-script` warns with
/// [`Outcome::reason`], and the *operations* that consume a selection are the
/// ones that refuse an empty one.
pub fn apply(cands: &[Candidate], filters: &[Filter], tol: &Tol) -> Outcome {
    let mut picked: Vec<usize> = Vec::new();
    'next: for (i, c) in cands.iter().enumerate() {
        for f in filters {
            if f.test(c, tol).is_err() {
                continue 'next;
            }
        }
        picked.push(i);
    }
    let reason = if picked.is_empty() && !cands.is_empty() {
        Some(explain(cands, filters, tol))
    } else {
        None
    };
    let mut out = Outcome { picked, considered: cands.len(), reason };
    sort(cands, &mut out.picked);
    out
}

/// Why nothing matched, in one sentence: the filter that rejected the most
/// candidates, how many of them it rejected, and the closest thing to a match.
///
/// This runs only when a selection came back empty, so unlike [`apply`] it can
/// afford to test *every* filter against every candidate rather than stopping
/// at the first failure — which is what makes "rejected 6 of the 6" a fact
/// rather than an artefact of the order the filters happened to be in.
pub fn explain(cands: &[Candidate], filters: &[Filter], tol: &Tol) -> String {
    let total = cands.len();
    let mut best: Option<(usize, usize, Miss, &str)> = None; // filter, count, miss, name
    for (fi, f) in filters.iter().enumerate() {
        let mut count = 0usize;
        let mut nearest: Option<(Miss, &str)> = None;
        for c in cands {
            if let Err(m) = f.test(c, tol) {
                count += 1;
                if nearest.map(|(n, _)| m.rank() < n.rank()).unwrap_or(true) {
                    nearest = Some((m, c.name.as_str()));
                }
            }
        }
        if count == 0 {
            continue;
        }
        if best.map(|(_, n, _, _)| count > n).unwrap_or(true) {
            if let Some((m, name)) = nearest {
                best = Some((fi, count, m, name));
            }
        }
    }
    let Some((fi, count, miss, name)) = best else {
        // Every filter passed everything, yet nothing survived: impossible
        // unless `filters` is empty, which `apply` already handles.
        return format!("no filter rejected anything, yet none of the {total} matched");
    };
    let near = match miss {
        Miss::By(a, unit) => format!("the closest was {name:?}, off by {}{unit}", num(a)),
        Miss::Because(why) => format!("the closest was {name:?}, but {why}"),
    };
    format!("{} rejected {count} of the {total}; {near}", filters[fi].describe(tol))
}

/// Sort indices into `cands` into the canonical geometric order.
pub fn sort(cands: &[Candidate], picked: &mut [usize]) {
    picked.sort_by(|&a, &b| sort_key(&cands[a]).cmp(&sort_key(&cands[b])));
}

/// The canonical order of a selection: **lowest first, then front to back,
/// then left to right** — the way a part reads on a print bed.
///
/// The key is the centroid as `(z, y, x)`, then the lower bounding-box corner
/// the same way (which separates two entities that share a centroid, such as
/// the two halves of a cylindrical bore), then the larger entity, then the
/// generated name. Coordinates are quantised to a micron by
/// [`naming::quantize`] before they are compared, so two entities that are
/// nominally at the same place cannot swap on float noise. The name is the
/// last resort, and it is itself a pure function of the feature that made the
/// geometry — never of the kernel's output order.
pub fn sort_key(c: &Candidate) -> (i64, i64, i64, i64, i64, i64, i64, String) {
    (
        q(c.centroid.z),
        q(c.centroid.y),
        q(c.centroid.x),
        q(c.lo.z),
        q(c.lo.y),
        q(c.lo.x),
        -q(c.size()),
        c.name.clone(),
    )
}

// ---------------------------------------------------------------- reducers

/// The single entity nearest `point`, by distance from the centroid, ties
/// broken by [`sort_key`]. Returns an index into `cands`.
pub fn nearest(cands: &[Candidate], picked: &[usize], point: Vec3) -> Option<usize> {
    picked
        .iter()
        .copied()
        .min_by(|&a, &b| {
            let da = q((cands[a].centroid - point).length());
            let db = q((cands[b].centroid - point).length());
            da.cmp(&db).then_with(|| sort_key(&cands[a]).cmp(&sort_key(&cands[b])))
        })
}

// ---------------------------------------------------------------- candidates

/// One candidate per face of `topo`.
pub fn face_candidates(topo: &Topology, mesh: &Mesh) -> Vec<Candidate> {
    topo.faces.iter().enumerate().map(|(i, f)| face_candidate(i, f, mesh)).collect()
}

fn face_candidate(index: usize, f: &FaceInfo, mesh: &Mesh) -> Candidate {
    let n = f.tri_count as usize;
    let mut lo = Vec3::new(f64::MAX, f64::MAX, f64::MAX);
    let mut hi = Vec3::new(f64::MIN, f64::MIN, f64::MIN);
    let stride = n.div_ceil(FACE_SAMPLE_TRIS.max(1)).max(1);
    let mut points = Vec::new();
    for k in 0..n {
        let Some(tri) = mesh.triangle(f.tri_start as usize + k) else { continue };
        for v in tri {
            lo = lo.min(v);
            hi = hi.max(v);
        }
        if k % stride == 0 {
            points.extend_from_slice(&tri);
        }
    }
    if points.is_empty() {
        points.push(f.centroid);
        lo = f.centroid;
        hi = f.centroid;
    }
    Candidate {
        index,
        name: f.name.clone(),
        aliases: f.aliases.clone(),
        class: f.surface_kind.to_owned(),
        surface_class: f.surface_class().to_owned(),
        centroid: f.centroid,
        direction: f.normal,
        straight: true,
        area: f.area,
        length: 0.0,
        radius: 0.0,
        convex: false,
        dihedral: -1.0,
        lo,
        hi,
        points,
        neighbours: f.edge_ids.clone(),
    }
}

/// One candidate per edge of `topo`.
pub fn edge_candidates(topo: &Topology) -> Vec<Candidate> {
    topo.edges.iter().enumerate().map(|(i, e)| edge_candidate(i, e)).collect()
}

fn edge_candidate(index: usize, e: &EdgeInfo) -> Candidate {
    let mut points = e.polyline.clone();
    if points.len() < 2 {
        points = vec![e.start, e.end];
    }
    let mut lo = Vec3::new(f64::MAX, f64::MAX, f64::MAX);
    let mut hi = Vec3::new(f64::MIN, f64::MIN, f64::MIN);
    for p in &points {
        lo = lo.min(*p);
        hi = hi.max(*p);
    }
    let straight = is_straight(&points);
    let (radius, closed) = circle_fit(&points);
    let class = if radius > 0.0 {
        if closed { "circle".to_owned() } else { "arc".to_owned() }
    } else {
        e.curve_kind.to_owned()
    };
    Candidate {
        index,
        name: e.name.clone(),
        aliases: Vec::new(),
        class,
        surface_class: String::new(),
        centroid: midpoint(&points),
        direction: if straight { (e.end - e.start).normalized() } else { Vec3::ZERO },
        straight,
        area: 0.0,
        length: e.length,
        radius,
        convex: e.convex,
        dihedral: e.dihedral_degrees,
        lo,
        hi,
        points,
        neighbours: e.face_ids.clone(),
    }
}

/// The point half way along a polyline, which is on the entity — unlike the
/// average of its samples, which is not, and which would move when the
/// tessellation density changed.
fn midpoint(points: &[Vec3]) -> Vec3 {
    let total: f64 = points.windows(2).map(|w| (w[1] - w[0]).length()).sum();
    if total <= 0.0 {
        return points.first().copied().unwrap_or_default();
    }
    let mut walked = 0.0;
    for w in points.windows(2) {
        let step = (w[1] - w[0]).length();
        if walked + step >= total / 2.0 {
            let t = if step > 0.0 { (total / 2.0 - walked) / step } else { 0.0 };
            return w[0] + (w[1] - w[0]) * t;
        }
        walked += step;
    }
    *points.last().unwrap()
}

/// Straight to within a micron: every point lies on the chord.
fn is_straight(points: &[Vec3]) -> bool {
    let (a, b) = (points[0], *points.last().unwrap());
    let chord = b - a;
    let len = chord.length();
    if len <= DEFAULT_TOLERANCE_MM {
        return false;
    }
    let d = chord * (1.0 / len);
    points.iter().all(|p| {
        let v = *p - a;
        (v - d * v.dot(d)).length() <= DEFAULT_TOLERANCE_MM
    })
}

/// Fit a circle through three well-spread points of the polyline and check
/// every point against it. Returns `(radius, closed)`, or `(0, _)` when the
/// polyline is not an arc.
///
/// This is a *measurement*, not a guess: an edge is an arc only when all of
/// its points are on the fitted circle. It is the one shape recognition this
/// module does, and it exists because `kernel::curve_kind` calls a circle a
/// `bspline`. Faces do not need it: `crate::surface_class` stamps every face
/// with the analytic surface it lies on at tessellation.
pub fn circle_fit(points: &[Vec3]) -> (f64, bool) {
    let closed = points.len() >= 2 && (points[0] - *points.last().unwrap()).length() <= DEFAULT_TOLERANCE_MM;
    match circle_fit_frame(points) {
        Some((_, _, r)) => (r, closed),
        None => (0.0, closed),
    }
}

/// The circle [`circle_fit`] measures, in full: `(centre, normal, radius)`,
/// or `None` when the polyline is not an arc. The normal follows the
/// polyline's winding (right-hand rule), which is what a frame on the edge
/// needs: the axis of the hole the edge bounds.
pub fn circle_fit_frame(points: &[Vec3]) -> Option<(Vec3, Vec3, f64)> {
    if points.is_empty() {
        return None;
    }
    let closed = points.len() >= 2 && (points[0] - *points.last().unwrap()).length() <= DEFAULT_TOLERANCE_MM;
    let pts: &[Vec3] = if closed && points.len() > 3 { &points[..points.len() - 1] } else { points };
    if pts.len() < 3 {
        return None;
    }
    let (a, b, c) = (pts[0], pts[pts.len() / 3], pts[2 * pts.len() / 3]);
    let (ab, ac) = (b - a, c - a);
    let n = ab.cross(ac);
    let n2 = n.dot(n);
    if n2 <= 1e-18 {
        return None;
    }
    // Circumcentre of the triangle abc, in its own plane.
    let centre = a + (ac * ab.dot(ab) - ab * ac.dot(ac)).cross(n) * (0.5 / n2);
    let r = (a - centre).length();
    if r <= DEFAULT_TOLERANCE_MM {
        return None;
    }
    let normal = n.normalized();
    for p in pts {
        let v = *p - centre;
        if (v.dot(normal)).abs() > r * CIRCLE_FIT_TOLERANCE {
            return None;
        }
        if (v.length() - r).abs() > r * CIRCLE_FIT_TOLERANCE {
            return None;
        }
    }
    Some((centre, normal, r))
}

// ---------------------------------------------------------------- set algebra

#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub enum SetOp {
    Union,
    Intersection,
    Difference,
}

/// Where one member of a combined selection came from.
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub struct Member {
    /// True when the member came from the right-hand selection.
    pub from_b: bool,
    /// Its position within that selection.
    pub at: usize,
}

/// Combine two selections given as their members' **canonical ranks** (see
/// [`sort_key`]) and say which members survive, in canonical order.
///
/// Ranks rather than entities, because rank *is* the order: a union comes back
/// in exactly the order one selector asking for both would have produced, and
/// the caller does not have to re-derive any geometry to sort it. A rank
/// identifies an entity within its body, so a duplicate across the two sides is
/// one member, taken from the left.
pub fn combine(a: &[u32], b: &[u32], op: SetOp) -> Vec<Member> {
    let mut keep: Vec<(u32, Member)> = Vec::new();
    match op {
        SetOp::Union => {
            for (at, r) in a.iter().enumerate() {
                keep.push((*r, Member { from_b: false, at }));
            }
            for (at, r) in b.iter().enumerate() {
                if !a.contains(r) {
                    keep.push((*r, Member { from_b: true, at }));
                }
            }
        }
        SetOp::Intersection => {
            for (at, r) in a.iter().enumerate() {
                if b.contains(r) {
                    keep.push((*r, Member { from_b: false, at }));
                }
            }
        }
        SetOp::Difference => {
            for (at, r) in a.iter().enumerate() {
                if !b.contains(r) {
                    keep.push((*r, Member { from_b: false, at }));
                }
            }
        }
    }
    keep.sort_by_key(|(r, _)| *r);
    keep.into_iter().map(|(_, m)| m).collect()
}

/// The canonical rank of every entity: `rank[i]` is where entity `i` sits in
/// [`sort`]'s order. This is the table [`combine`] and every reducer break ties
/// with, and it is the only place a selection's order is decided.
pub fn ranks(cands: &[Candidate]) -> Vec<u32> {
    let mut order: Vec<usize> = (0..cands.len()).collect();
    sort(cands, &mut order);
    let mut rank = vec![0u32; cands.len()];
    for (r, &i) in order.iter().enumerate() {
        rank[i] = r as u32;
    }
    rank
}

// ---------------------------------------------------------------- sketch plane

/// Why a face cannot carry a sketch.
#[derive(Clone, Debug, PartialEq)]
pub enum PlaneError {
    /// The surface is not a plane; the string is the surface kind, so the
    /// diagnostic can name it.
    NotPlanar(String),
    /// A planar face with no usable normal (a degenerate sliver).
    Degenerate,
}

/// The sketch plane of a planar face (`sketch(on=)`).
///
/// * **Normal** is the face's outward normal, so a positive `extrude` leaves
///   the body and a negative one cuts into it.
/// * **Origin** is the face's area-weighted centroid. It is a pure function of
///   the face's outline, moves smoothly with a parameter, and is where a
///   script that writes `rounded_rect(w, h, r)` with no `center=` expects
///   (0, 0) to be. The alternative — projecting the world origin onto the
///   plane — is more stable still, but puts the sketch origin somewhere nobody
///   asked for.
/// * **x direction** is [`naming::canonical_frame`] of the normal: the world
///   axis least aligned with it, sign-normalised first and orthogonalised.
///   That is derived from the *orientation alone*, so it cannot spin when the
///   face's outline changes, when the face moves within its plane, or when the
///   normal flips. It also reproduces `XY` / `XZ` / `YZ` exactly for the
///   axis-aligned cases.
pub fn face_plane(kind: &str, normal: Vec3, centroid: Vec3) -> Result<Plane, PlaneError> {
    if kind != "plane" {
        return Err(PlaneError::NotPlanar(kind.to_owned()));
    }
    if normal.length() <= 0.0 {
        return Err(PlaneError::Degenerate);
    }
    let (x_dir, _) = naming::canonical_frame(normal);
    Plane::new(centroid, normal, x_dir).map_err(|_| PlaneError::Degenerate)
}

// ---------------------------------------------------------------- helpers

/// `*` and `?` globbing. One definition, shared with the DXF layer selector.
pub fn glob_match(pattern: &str, text: &str) -> bool {
    let (p, t): (Vec<char>, Vec<char>) = (pattern.chars().collect(), text.chars().collect());
    let (mut pi, mut ti) = (0usize, 0usize);
    let (mut star, mut mark) = (usize::MAX, 0usize);
    while ti < t.len() {
        if pi < p.len() && (p[pi] == '?' || p[pi] == t[ti]) {
            pi += 1;
            ti += 1;
        } else if pi < p.len() && p[pi] == '*' {
            star = pi;
            mark = ti;
            pi += 1;
        } else if star != usize::MAX {
            pi = star + 1;
            mark += 1;
            ti = mark;
        } else {
            return false;
        }
    }
    while pi < p.len() && p[pi] == '*' {
        pi += 1;
    }
    pi == p.len()
}

fn num(v: f64) -> String {
    crate::context::fmt_num(v)
}

#[cfg(test)]
mod tests {
    use super::*;

    fn face(name: &str, class: &str, centroid: Vec3, normal: Vec3, area: f64, corners: &[Vec3]) -> Candidate {
        let mut lo = Vec3::new(f64::MAX, f64::MAX, f64::MAX);
        let mut hi = Vec3::new(f64::MIN, f64::MIN, f64::MIN);
        for c in corners {
            lo = lo.min(*c);
            hi = hi.max(*c);
        }
        Candidate {
            index: 0,
            name: name.into(),
            class: class.into(),
            centroid,
            direction: normal,
            straight: true,
            area,
            lo,
            hi,
            points: corners.to_vec(),
            dihedral: -1.0,
            ..Default::default()
        }
    }

    /// A 10 x 10 x 4 box, as the tessellator would describe it.
    fn box_faces() -> Vec<Candidate> {
        let p = |x: f64, y: f64, z: f64| Vec3::new(x, y, z);
        let quad = |a: Vec3, b: Vec3| vec![a, p(b.x, a.y, a.z), b, p(a.x, b.y, b.z)];
        let mut v = vec![
            face("b/bottom", "plane", p(5.0, 5.0, 0.0), -Vec3::Z, 100.0, &quad(p(0.0, 0.0, 0.0), p(10.0, 10.0, 0.0))),
            face("b/top", "plane", p(5.0, 5.0, 4.0), Vec3::Z, 100.0, &quad(p(0.0, 0.0, 4.0), p(10.0, 10.0, 4.0))),
            face("b/front", "plane", p(5.0, 0.0, 2.0), -Vec3::Y, 40.0, &quad(p(0.0, 0.0, 0.0), p(10.0, 0.0, 4.0))),
            face("b/right", "plane", p(10.0, 5.0, 2.0), Vec3::X, 40.0, &quad(p(10.0, 0.0, 0.0), p(10.0, 10.0, 4.0))),
            face("b/back", "plane", p(5.0, 10.0, 2.0), Vec3::Y, 40.0, &quad(p(0.0, 10.0, 0.0), p(10.0, 10.0, 4.0))),
            face("b/left", "plane", p(0.0, 5.0, 2.0), -Vec3::X, 40.0, &quad(p(0.0, 0.0, 0.0), p(0.0, 10.0, 4.0))),
        ];
        for (i, f) in v.iter_mut().enumerate() {
            f.index = i;
        }
        v
    }

    fn names(cands: &[Candidate], picked: &[usize]) -> Vec<String> {
        picked.iter().map(|&i| cands[i].name.clone()).collect()
    }

    #[test]
    fn a_normal_selects_one_side_and_not_the_other() {
        let f = box_faces();
        let up = apply(&f, &[Filter::Direction { dir: Vec3::Z, signed: true }], &Tol::default());
        assert_eq!(names(&f, &up.picked), ["b/top"]);
        assert!(up.reason.is_none());
        let down = apply(&f, &[Filter::Direction { dir: -Vec3::Z, signed: true }], &Tol::default());
        assert_eq!(names(&f, &down.picked), ["b/bottom"]);
    }

    #[test]
    fn on_plane_is_exact_for_a_planar_face_and_rejects_a_parallel_one_elsewhere() {
        let f = box_faces();
        let tol = Tol::default();
        let top = apply(&f, &[Filter::OnPlane(Plane::XY.offset(4.0))], &tol);
        assert_eq!(names(&f, &top.picked), ["b/top"]);
        let none = apply(&f, &[Filter::OnPlane(Plane::XY.offset(2.0))], &tol);
        assert!(none.picked.is_empty());
        // ... and it says why, naming the filter and the nearest miss.
        let why = none.reason.unwrap();
        assert!(why.contains("on the plane"), "{why}");
        assert!(why.contains("rejected 6 of the 6"), "{why}");
        assert!(why.contains("the closest was"), "{why}");
    }

    #[test]
    fn half_spaces_and_boxes_take_whole_entities_only() {
        let f = box_faces();
        let tol = Tol::default();
        let above = apply(&f, &[Filter::Above(Plane::XY.offset(4.0))], &tol);
        assert_eq!(names(&f, &above.picked), ["b/top"]);
        // The side walls straddle z = 2, so neither half-space takes them.
        let below = apply(&f, &[Filter::Below(Plane::XY.offset(2.0))], &tol);
        assert_eq!(names(&f, &below.picked), ["b/bottom"]);
        let inside = apply(
            &f,
            &[Filter::Inside { lo: Vec3::new(-1.0, -1.0, 3.0), hi: Vec3::new(11.0, 11.0, 5.0) }],
            &tol,
        );
        assert_eq!(names(&f, &inside.picked), ["b/top"]);
    }

    #[test]
    fn filters_intersect_and_the_algebra_composes() {
        let f = box_faces();
        let tol = Tol::default();
        let rank = ranks(&f);
        let as_ranks = |picked: &[usize]| -> Vec<u32> {
            let mut r: Vec<u32> = picked.iter().map(|&i| rank[i]).collect();
            r.sort_unstable();
            r
        };
        let named = |members: &[Member], a: &[u32], b: &[u32]| -> Vec<String> {
            members
                .iter()
                .map(|m| {
                    let want = if m.from_b { b[m.at] } else { a[m.at] };
                    f.iter().find(|c| rank[c.index] == want).unwrap().name.clone()
                })
                .collect()
        };
        let planar = as_ranks(&apply(&f, &[Filter::Class("plane".into())], &tol).picked);
        assert_eq!(planar.len(), 6);
        let up = as_ranks(&apply(&f, &[Filter::Direction { dir: Vec3::Z, signed: true }], &tol).picked);
        let down = as_ranks(&apply(&f, &[Filter::Direction { dir: -Vec3::Z, signed: true }], &tol).picked);
        let u = combine(&up, &down, SetOp::Union);
        assert_eq!(named(&u, &up, &down), ["b/bottom", "b/top"]);
        assert!(combine(&up, &down, SetOp::Intersection).is_empty());
        assert_eq!(combine(&planar, &up, SetOp::Difference).len(), 5);
        // Union is idempotent and does not duplicate.
        assert_eq!(combine(&up, &up, SetOp::Union).len(), 1);
        // A union comes back in the order one selector asking for both would
        // have produced: the canonical ranks, ascending.
        let ordered: Vec<u32> = u.iter().map(|m| if m.from_b { down[m.at] } else { up[m.at] }).collect();
        let mut sorted = ordered.clone();
        sorted.sort_unstable();
        assert_eq!(ordered, sorted);
    }

    #[test]
    fn a_name_glob_matches_the_local_part_and_the_aliases() {
        let mut f = box_faces();
        f[1].aliases = vec!["b/end".into()];
        let tol = Tol::default();
        assert_eq!(names(&f, &apply(&f, &[Filter::Name("b/top".into())], &tol).picked), ["b/top"]);
        assert_eq!(names(&f, &apply(&f, &[Filter::Name("top".into())], &tol).picked), ["b/top"]);
        assert_eq!(names(&f, &apply(&f, &[Filter::Name("end".into())], &tol).picked), ["b/top"]);
        assert_eq!(apply(&f, &[Filter::Name("b/*".into())], &tol).picked.len(), 6);
    }

    #[test]
    fn the_order_is_geometric_and_survives_a_shuffle() {
        let f = box_faces();
        let all: Vec<usize> = (0..f.len()).collect();
        let mut sorted = all.clone();
        sort(&f, &mut sorted);
        // Reversing the input order must not change the output order.
        let mut reversed: Vec<usize> = all.iter().rev().copied().collect();
        sort(&f, &mut reversed);
        assert_eq!(sorted, reversed);
        // Lowest first, then front to back, then left to right.
        assert_eq!(names(&f, &sorted), ["b/bottom", "b/front", "b/left", "b/right", "b/back", "b/top"]);
    }

    #[test]
    fn a_parameter_change_moves_entities_but_does_not_reorder_them() {
        // The same box, 1 mm taller: every face keeps its place in the order.
        let a = box_faces();
        let mut b = box_faces();
        for f in b.iter_mut() {
            if f.name == "b/top" {
                f.centroid.z = 5.0;
                f.lo.z = 5.0;
                f.hi.z = 5.0;
            } else if f.name != "b/bottom" {
                f.centroid.z = 2.5;
                f.hi.z = 5.0;
            }
        }
        let mut oa: Vec<usize> = (0..a.len()).collect();
        let mut ob: Vec<usize> = (0..b.len()).collect();
        sort(&a, &mut oa);
        sort(&b, &mut ob);
        assert_eq!(names(&a, &oa), names(&b, &ob));
    }

    #[test]
    fn nearest_picks_one_and_breaks_ties_by_the_canonical_order() {
        let f = box_faces();
        let all: Vec<usize> = (0..f.len()).collect();
        assert_eq!(f[nearest(&f, &all, Vec3::new(5.0, 5.0, 99.0)).unwrap()].name, "b/top");
        assert_eq!(f[nearest(&f, &all, Vec3::new(5.0, 5.0, -99.0)).unwrap()].name, "b/bottom");
        // The box centre is 2 mm from both caps and 5 mm from every wall, so
        // top and bottom tie: the canonical order breaks it, lowest first.
        assert_eq!(f[nearest(&f, &all, Vec3::new(5.0, 5.0, 2.0)).unwrap()].name, "b/bottom");
    }

    #[test]
    fn ranks_agree_with_the_sort() {
        let f = box_faces();
        let rank = ranks(&f);
        let mut order: Vec<usize> = (0..f.len()).collect();
        sort(&f, &mut order);
        for (r, &i) in order.iter().enumerate() {
            assert_eq!(rank[i], r as u32);
        }
    }

    /// The reason names a count that is true, not the first filter to fail.
    /// `apply` short-circuits; `explain` does not, which is the whole point.
    #[test]
    fn the_reason_counts_every_rejection_not_just_the_first_filters() {
        let f = box_faces();
        let tol = Tol::default();
        // `kind` rejects nothing (all six are planes); `normal` rejects all six.
        let out = apply(
            &f,
            &[Filter::Class("plane".into()), Filter::Direction { dir: Vec3::new(1.0, 1.0, 1.0), signed: true }],
            &tol,
        );
        let why = out.reason.unwrap();
        assert!(why.contains("direction"), "{why}");
        assert!(why.contains("rejected 6 of the 6"), "{why}");
        assert!(why.contains("off by"), "{why}");
    }

    /// A candidate that is the wrong shape for a test gets a clause, not a
    /// nonsense distance.
    #[test]
    fn a_miss_that_cannot_be_measured_says_why_instead() {
        let mut e = Candidate {
            index: 0,
            name: "b/rim".into(),
            class: "bspline".into(),
            straight: false,
            length: 10.0,
            dihedral: -1.0,
            points: vec![Vec3::ZERO, Vec3::new(1.0, 1.0, 0.0), Vec3::ZERO],
            ..Default::default()
        };
        e.hi = Vec3::new(1.0, 1.0, 0.0);
        let tol = Tol::default();
        let out = apply(&[e.clone()], &[Filter::Direction { dir: Vec3::X, signed: false }], &tol);
        let why = out.reason.unwrap();
        assert!(why.contains("not straight"), "{why}");
        let out = apply(&[e.clone()], &[Filter::Convex(true)], &tol);
        assert!(out.reason.unwrap().contains("no two adjacent faces"));
        let out = apply(&[e], &[Filter::MinRadius(2.0)], &tol);
        assert!(out.reason.unwrap().contains("not an arc"));
    }

    #[test]
    fn an_empty_body_has_no_reason_to_give() {
        let out = apply(&[], &[Filter::Class("plane".into())], &Tol::default());
        assert!(out.picked.is_empty());
        assert_eq!(out.considered, 0);
        assert!(out.reason.is_none());
    }

    #[test]
    fn a_circle_fit_recognises_an_arc_and_refuses_a_polyline() {
        let arc: Vec<Vec3> = (0..=16)
            .map(|i| {
                let t = std::f64::consts::FRAC_PI_2 * i as f64 / 16.0;
                Vec3::new(4.0 * fmath::cos(t), 4.0 * fmath::sin(t), 3.0)
            })
            .collect();
        let (r, closed) = circle_fit(&arc);
        assert!((r - 4.0).abs() < 1e-9, "radius {r}");
        assert!(!closed);
        let full: Vec<Vec3> = (0..=32)
            .map(|i| {
                let t = std::f64::consts::TAU * i as f64 / 32.0;
                Vec3::new(2.5 * fmath::cos(t), 2.5 * fmath::sin(t), 0.0)
            })
            .collect();
        let (r, closed) = circle_fit(&full);
        assert!((r - 2.5).abs() < 1e-9);
        assert!(closed);
        let line = vec![Vec3::ZERO, Vec3::new(1.0, 0.0, 0.0), Vec3::new(2.0, 0.0, 0.0)];
        assert_eq!(circle_fit(&line).0, 0.0);
        let bent = vec![Vec3::ZERO, Vec3::new(1.0, 0.0, 0.0), Vec3::new(1.0, 1.0, 0.0), Vec3::new(9.0, 3.0, 0.0)];
        assert_eq!(circle_fit(&bent).0, 0.0);
    }

    #[test]
    fn a_sketch_on_a_face_reproduces_the_world_planes() {
        assert_eq!(face_plane("plane", Vec3::Z, Vec3::ZERO).unwrap(), Plane::XY);
        assert_eq!(face_plane("plane", Vec3::X, Vec3::ZERO).unwrap(), Plane::YZ);
        assert_eq!(face_plane("plane", -Vec3::Y, Vec3::ZERO).unwrap(), Plane::XZ);
        // The x direction comes from the orientation alone, so moving the face
        // within its plane cannot spin it.
        let a = face_plane("plane", Vec3::Z, Vec3::new(3.0, 7.0, 2.0)).unwrap();
        let b = face_plane("plane", Vec3::Z, Vec3::new(-40.0, 11.0, 2.0)).unwrap();
        assert_eq!(a.x_dir, b.x_dir);
        assert_eq!(a.normal, b.normal);
        // Flipping the normal keeps the same x direction, so a lid's underside
        // and its top agree about which way is "across".
        let flipped = face_plane("plane", -Vec3::Z, Vec3::ZERO).unwrap();
        assert_eq!(flipped.x_dir, a.x_dir);
        assert_eq!(
            face_plane("nurbs", Vec3::Z, Vec3::ZERO),
            Err(PlaneError::NotPlanar("nurbs".into()))
        );
    }

    #[test]
    fn globs_behave() {
        assert!(glob_match("side[*]", "side[0.2]"));
        assert!(glob_match("*/top", "blk/top"));
        assert!(glob_match("a?c", "abc"));
        assert!(!glob_match("side[*]", "end"));
    }
}
