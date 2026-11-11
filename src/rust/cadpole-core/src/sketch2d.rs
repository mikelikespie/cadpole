//! The 2D toolkit: offset, booleans, corner fillets and chamfers,
//! mirror and patterns on a [`Sketch`].
//!
//! Everything here is built on one planar clipper over **lines and circular
//! arcs**, so that what a sketch is made of is what comes back: a line
//! offsets to a line, an arc to a concentric arc, a rectangle's rounded
//! offset has four exact quarter circles, and an extrude of an offset circle
//! is still a cylinder. Splines (Béziers) are the exception: they are
//! sampled to the sketch tolerance on the way in and re-fitted on the way
//! out, and the note on each operation says so.
//!
//! **The clipper.** The pieces of every operand are cut against each other
//! (line–line, line–arc and arc–arc intersections in closed form), welded
//! into vertices — a point within `1e-9 × extent` of an existing vertex
//! *is* that vertex, and the first point to arrive keeps its coordinates —
//! rebuilt from their welded ends, split where they pass within that
//! distance of a vertex they do not end at, and merged where they coincide.
//! The faces of that planar graph are traced by angular order around each
//! vertex (tangent, then curvature, so a line tangent to an arc sorts).
//! Each face gets one winding number per operand, evaluated at the
//! midpoint of its longest edge by a ray with the half-open endpoint rule
//! (a vertex on the ray counts once, on its `+t` side, so a curve that
//! merely touches the ray counts zero and one that crosses it counts one).
//! The result is the set of faces the operation's rule accepts —
//! `wn > 0` for a fill, `wn_a > 0 || wn_b > 0` for a union, and so on — and
//! its boundary is walked with the material on the left, so outer loops come
//! back counter-clockwise and holes clockwise. Winding numbers rather than
//! even-odd because a union of overlapping regions is then just "covered
//! at least once", and because it is what makes the offset below exact.
//!
//! **The offset** is the Chen–McMains construction: offset every piece,
//! join the pieces at each vertex — a round arc, mitre or bevel where the
//! offset *opens* a gap, and a spike back through the original vertex where
//! the offset pieces *overlap* — and keep the region with positive winding
//! number. The spike is what makes that rule a theorem rather than a
//! heuristic: with it, the raw curve is homologous to the original boundary
//! plus one rectangle per piece and one sector per gap vertex, every term
//! with the sign of the offset, so `wn > 0` is exactly "in the original or
//! within `d` of its boundary" for an outward offset and exactly "in the
//! original and further than `|d|` from its boundary" for an inward one. An
//! arc whose offset radius would cross zero collapses to its centre (a
//! continuous offset would turn it inside out and leave a phantom disc).
//! Where two overlapping offset pieces reach each other they are trimmed to
//! the meeting point instead of spiked: the loop that removes lies inside
//! the two rectangles and winds `±1`, so the accepted region is the same,
//! and the hairline overshoots a tiny turn would leave are gone.
//! Self-intersections — the trailing edge of a thin airfoil offset inward —
//! are the arrangement's ordinary business: the invalid loops have winding
//! number ≤ 0 and vanish, a loop that collapses vanishes with a note naming
//! it, and an offset that splits a loop returns both pieces.
//!
//! **Exactness contract.** Lines and arcs: every vertex of the result is a
//! closed-form intersection or a closed-form offset point, exact to floating
//! point, and consecutive segments share their vertices exactly. Splines:
//! within the sketch tolerance, sampled on the way in and re-fitted on the
//! way out. Nothing here goes through the mesh path.

use std::collections::{BTreeMap, BTreeSet, HashMap};
use std::f64::consts::{PI, TAU};

use crate::fmath;
use crate::geom::{Plane, Vec3};
use crate::naming;
use crate::sketch::{Loop, P2, Segment, Sketch, arc_params, bezier_point};

// ----------------------------------------------------------------- Join

/// How `Sketch.offset` joins two offset pieces at a corner that opens a gap.
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub enum Join {
    /// A circular arc of radius `|d|` about the corner: the true (Minkowski) offset.
    Round,
    /// The two offset pieces extended to their intersection, up to
    /// [`MITER_LIMIT`] offset distances from the corner; a sharper corner is bevelled.
    Miter,
    /// A straight line between the two offset ends.
    Bevel,
}

impl Join {
    pub const ALL: [Join; 3] = [Join::Round, Join::Miter, Join::Bevel];

    pub fn name(self) -> &'static str {
        match self {
            Join::Round => "round",
            Join::Miter => "miter",
            Join::Bevel => "bevel",
        }
    }

    /// Parse the spelling a script passes; the error lists the accepted spellings.
    pub fn parse(s: &str) -> Result<Join, String> {
        Join::ALL
            .iter()
            .copied()
            .find(|j| j.name() == s)
            .ok_or_else(|| {
                let valid: Vec<&str> = Join::ALL.iter().map(|j| j.name()).collect();
                format!("join={s:?} is not a join style; valid: {}", valid.join(", "))
            })
    }
}

/// A mitre point further than this many offset distances from its corner is
/// bevelled instead (SVG's `miterlimit` default: corners sharper than about
/// 29° do not get a spike).
pub const MITER_LIMIT: f64 = 4.0;

/// The 2D booleans.
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub enum BoolOp {
    Union,
    Subtract,
    Intersect,
}

impl BoolOp {
    pub fn name(self) -> &'static str {
        match self {
            BoolOp::Union => "union",
            BoolOp::Subtract => "subtract",
            BoolOp::Intersect => "intersect",
        }
    }
}

/// What a toolkit operation hands back: the sketch, and the notes a script
/// should be able to read (a loop that collapsed, an empty intersection).
#[derive(Clone, Debug)]
pub struct Outcome {
    pub sketch: Sketch,
    pub notes: Vec<String>,
}

// ---------------------------------------------------------- small vectors

#[inline]
fn sub(a: P2, b: P2) -> P2 {
    P2::new(a.x - b.x, a.y - b.y)
}
#[inline]
fn add(a: P2, b: P2) -> P2 {
    P2::new(a.x + b.x, a.y + b.y)
}
#[inline]
fn mul(a: P2, k: f64) -> P2 {
    P2::new(a.x * k, a.y * k)
}
#[inline]
fn dot(a: P2, b: P2) -> f64 {
    a.x * b.x + a.y * b.y
}
#[inline]
fn cross(a: P2, b: P2) -> f64 {
    a.x * b.y - a.y * b.x
}
#[inline]
fn norm(a: P2) -> f64 {
    (a.x * a.x + a.y * a.y).sqrt()
}
#[inline]
fn dist(a: P2, b: P2) -> f64 {
    norm(sub(a, b))
}
fn unit(a: P2) -> Option<P2> {
    let l = norm(a);
    if l > 0.0 { Some(P2::new(a.x / l, a.y / l)) } else { None }
}
/// The right-hand normal of a direction: where an outward offset of a
/// counter-clockwise loop goes.
#[inline]
fn right(t: P2) -> P2 {
    P2::new(t.y, -t.x)
}
#[inline]
fn left(t: P2) -> P2 {
    P2::new(-t.y, t.x)
}
fn polar(c: P2, r: f64, t: f64) -> P2 {
    let (s, co) = fmath::sin_cos(t);
    P2::new(c.x + r * co, c.y + r * s)
}

// ------------------------------------------------------------------ Prim

/// A primitive of the clipper: an oriented line or circular arc.
#[derive(Clone, Copy, Debug, PartialEq)]
enum Prim {
    Line { a: P2, b: P2 },
    /// Centre, radius, start angle and signed sweep (`0 < |sweep| <= π`
    /// inside the arrangement; a longer arc is split before it enters).
    Arc { c: P2, r: f64, t0: f64, sweep: f64 },
}

impl Prim {
    fn start(&self) -> P2 {
        match *self {
            Prim::Line { a, .. } => a,
            Prim::Arc { c, r, t0, .. } => polar(c, r, t0),
        }
    }
    fn end(&self) -> P2 {
        match *self {
            Prim::Line { b, .. } => b,
            Prim::Arc { c, r, t0, sweep } => polar(c, r, t0 + sweep),
        }
    }
    fn point(&self, u: f64) -> P2 {
        match *self {
            Prim::Line { a, b } => P2::new(a.x + (b.x - a.x) * u, a.y + (b.y - a.y) * u),
            Prim::Arc { c, r, t0, sweep } => polar(c, r, t0 + sweep * u),
        }
    }
    /// Unit tangent at parameter `u` (a degenerate line has none).
    fn tangent(&self, u: f64) -> Option<P2> {
        match *self {
            Prim::Line { a, b } => unit(sub(b, a)),
            Prim::Arc { t0, sweep, .. } => {
                let (s, co) = fmath::sin_cos(t0 + sweep * u);
                Some(if sweep > 0.0 { P2::new(-s, co) } else { P2::new(s, -co) })
            }
        }
    }
    /// Signed curvature: zero for a line, `±1/r` for an arc turning left/right.
    fn curvature(&self) -> f64 {
        match *self {
            Prim::Line { .. } => 0.0,
            Prim::Arc { r, sweep, .. } => sweep.signum() / r,
        }
    }
    fn length(&self) -> f64 {
        match *self {
            Prim::Line { a, b } => dist(a, b),
            Prim::Arc { r, sweep, .. } => r * sweep.abs(),
        }
    }
    fn reversed(&self) -> Prim {
        match *self {
            Prim::Line { a, b } => Prim::Line { a: b, b: a },
            Prim::Arc { c, r, t0, sweep } => Prim::Arc { c, r, t0: t0 + sweep, sweep: -sweep },
        }
    }
    /// The piece between parameters `u0 < u1`, on the same carrier.
    fn sub(&self, u0: f64, u1: f64) -> Prim {
        match *self {
            Prim::Line { .. } => Prim::Line { a: self.point(u0), b: self.point(u1) },
            Prim::Arc { c, r, t0, sweep } => Prim::Arc { c, r, t0: t0 + sweep * u0, sweep: sweep * (u1 - u0) },
        }
    }
    /// The parameter of a point on an arc's circle, in the arc's own sense,
    /// normalised so that a point just before the start reads slightly
    /// negative rather than nearly one turn.
    fn arc_param(&self, p: P2) -> f64 {
        match *self {
            Prim::Line { a, b } => {
                let d = sub(b, a);
                let l2 = dot(d, d);
                if l2 == 0.0 { 0.0 } else { dot(sub(p, a), d) / l2 }
            }
            Prim::Arc { c, t0, sweep, .. } => {
                let phi = fmath::atan2(p.y - c.y, p.x - c.x);
                let mut d = ((phi - t0) * sweep.signum()).rem_euclid(TAU);
                if d > TAU - (TAU - sweep.abs()) / 2.0 {
                    d -= TAU;
                }
                d / sweep.abs()
            }
        }
    }
    fn bbox(&self) -> (P2, P2) {
        match *self {
            Prim::Line { a, b } => (P2::new(a.x.min(b.x), a.y.min(b.y)), P2::new(a.x.max(b.x), a.y.max(b.y))),
            Prim::Arc { c, r, .. } => {
                let (a, b) = (self.start(), self.end());
                let mut lo = P2::new(a.x.min(b.x), a.y.min(b.y));
                let mut hi = P2::new(a.x.max(b.x), a.y.max(b.y));
                for (k, q) in [(0.0, P2::new(c.x + r, c.y)), (PI / 2.0, P2::new(c.x, c.y + r)), (PI, P2::new(c.x - r, c.y)), (3.0 * PI / 2.0, P2::new(c.x, c.y - r))] {
                    let u = self.arc_param(polar(c, r, k));
                    if (-1e-9..=1.0 + 1e-9).contains(&u) {
                        lo = P2::new(lo.x.min(q.x), lo.y.min(q.y));
                        hi = P2::new(hi.x.max(q.x), hi.y.max(q.y));
                    }
                }
                (lo, hi)
            }
        }
    }
    /// This piece's share of `∮ (x dy − y dx) / 2`: the signed area of the
    /// loop it belongs to is the sum over its pieces.
    fn area_term(&self) -> f64 {
        match *self {
            Prim::Line { a, b } => (a.x * b.y - b.x * a.y) / 2.0,
            Prim::Arc { c, r, t0, sweep } => {
                let t1 = t0 + sweep;
                let (s0, c0) = fmath::sin_cos(t0);
                let (s1, c1) = fmath::sin_cos(t1);
                (r * r * sweep + r * (c.x * (s1 - s0) - c.y * (c1 - c0))) / 2.0
            }
        }
    }
}

/// The signed area of a loop, exact for lines and arcs (a spline is
/// sampled to `tolerance`): `∮ (x dy − y dx) / 2` piece by piece.
pub fn exact_area(l: &Loop, tolerance: f64) -> f64 {
    l.segments.iter().flat_map(|s| prims_of(s, tolerance)).map(|p| p.area_term()).sum()
}

/// A sketch segment as clipper primitives: one for a line or an arc, several
/// for a spline (sampled) or an arc longer than a half turn (split).
fn prims_of(seg: &Segment, tolerance: f64) -> Vec<Prim> {
    match seg {
        Segment::Line { a, b } => vec![Prim::Line { a: *a, b: *b }],
        Segment::Arc { a, b, via } => match arc_params(*a, *b, *via) {
            Some((c, r, t0, t1)) => split_long_arc(Prim::Arc { c, r, t0, sweep: t1 - t0 }),
            None => vec![Prim::Line { a: *a, b: *b }],
        },
        Segment::Bezier { .. } => {
            let pts = sample_bezier(seg, tolerance);
            pts.windows(2).map(|w| Prim::Line { a: w[0], b: w[1] }).collect()
        }
    }
}

fn split_long_arc(p: Prim) -> Vec<Prim> {
    match p {
        Prim::Arc { sweep, .. } if sweep.abs() > PI + 1e-12 => vec![p.sub(0.0, 0.5), p.sub(0.5, 1.0)],
        _ => vec![p],
    }
}

/// Sample a Bézier segment so the chord error stays under `tolerance`, both
/// ends included. The control polygon bounds the curve, so its deviation from
/// the chord bounds the flattening error, which falls as `1/n²`.
fn sample_bezier(seg: &Segment, tolerance: f64) -> Vec<P2> {
    let Segment::Bezier { a, b, ctrl } = seg else { return vec![seg.start(), seg.end()] };
    let tol = tolerance.max(1e-9);
    let chord = sub(*b, *a);
    let len = norm(chord);
    let dev = ctrl
        .iter()
        .map(|c| if len > 0.0 { cross(chord, sub(*c, *a)).abs() / len } else { dist(*c, *a) })
        .fold(0.0f64, f64::max);
    let steps = ((dev / tol).sqrt().ceil() as usize).clamp(4, 512);
    let mut hull = vec![*a];
    hull.extend_from_slice(ctrl);
    hull.push(*b);
    (0..=steps).map(|i| bezier_point(&hull, i as f64 / steps as f64)).collect()
}

// --------------------------------------------------------- intersections

/// Every parameter pair `(u on p, v on q)` where the two pieces meet, to
/// within `eps`: crossings, touches, and the ends of an overlap of collinear
/// lines or co-circular arcs. Closed form in every case.
fn intersect(p: &Prim, q: &Prim, eps: f64) -> Vec<(f64, f64)> {
    let mut out = Vec::new();
    let (lp, lq) = (p.length(), q.length());
    if lp <= eps || lq <= eps {
        return out;
    }
    let (tp, tq) = (eps / lp, eps / lq);
    let on = |u: f64, t: f64| (-t..=1.0 + t).contains(&u);
    match (p, q) {
        (Prim::Line { a, b }, Prim::Line { a: c, b: d }) => {
            let (r, s) = (sub(*b, *a), sub(*d, *c));
            let den = cross(r, s);
            if den.abs() <= 1e-12 * lp * lq {
                // Parallel: an overlap contributes its ends, nothing else.
                if cross(r, sub(*c, *a)).abs() / lp <= eps {
                    for (pt, v) in [(*c, 0.0), (*d, 1.0)] {
                        let u = p.arc_param(pt);
                        if on(u, tp) {
                            out.push((u.clamp(0.0, 1.0), v));
                        }
                    }
                    for (pt, u) in [(*a, 0.0), (*b, 1.0)] {
                        let v = q.arc_param(pt);
                        if on(v, tq) {
                            out.push((u, v.clamp(0.0, 1.0)));
                        }
                    }
                }
            } else {
                let w = sub(*c, *a);
                let u = cross(w, s) / den;
                let v = cross(w, r) / den;
                if on(u, tp) && on(v, tq) {
                    out.push((u.clamp(0.0, 1.0), v.clamp(0.0, 1.0)));
                }
            }
        }
        (Prim::Line { .. }, Prim::Arc { .. }) => {
            for (v, u) in intersect(q, p, eps) {
                out.push((u, v));
            }
        }
        (Prim::Arc { c, r, .. }, Prim::Line { a, b }) => {
            let d = sub(*b, *a);
            let t = unit(d).unwrap();
            let h = cross(t, sub(*c, *a)); // signed distance of the centre from the line
            let foot = dot(sub(*c, *a), t); // along the line, from `a`
            let gap = h.abs() - r;
            let half = if gap.abs() <= eps { 0.0 } else if gap < 0.0 { (r * r - h * h).max(0.0).sqrt() } else { return out };
            let mut cands = vec![foot - half];
            if half > 0.0 {
                cands.push(foot + half);
            }
            for s in cands {
                let v = s / lq;
                if !on(v, tq) {
                    continue;
                }
                let pt = q.point(v.clamp(0.0, 1.0));
                let u = p.arc_param(pt);
                if on(u, tp) {
                    out.push((u.clamp(0.0, 1.0), v.clamp(0.0, 1.0)));
                }
            }
        }
        (Prim::Arc { c: c1, r: r1, .. }, Prim::Arc { c: c2, r: r2, .. }) => {
            let d = dist(*c1, *c2);
            if d <= eps && (r1 - r2).abs() <= eps {
                // Co-circular: the overlap's ends are each arc's ends on the other.
                for (pt, v) in [(q.start(), 0.0), (q.end(), 1.0)] {
                    let u = p.arc_param(pt);
                    if on(u, tp) {
                        out.push((u.clamp(0.0, 1.0), v));
                    }
                }
                for (pt, u) in [(p.start(), 0.0), (p.end(), 1.0)] {
                    let v = q.arc_param(pt);
                    if on(v, tq) {
                        out.push((u, v.clamp(0.0, 1.0)));
                    }
                }
                return out;
            }
            if d <= eps || d > r1 + r2 + eps || d < (r1 - r2).abs() - eps {
                return out;
            }
            let a = (r1 * r1 - r2 * r2 + d * d) / (2.0 * d);
            let h2 = r1 * r1 - a * a;
            let h = if h2 <= eps * eps { 0.0 } else { h2.sqrt() };
            let ex = mul(sub(*c2, *c1), 1.0 / d);
            let m = add(*c1, mul(ex, a));
            let mut cands = vec![add(m, mul(left(ex), h))];
            if h > 0.0 {
                cands.push(add(m, mul(right(ex), h)));
            }
            for pt in cands {
                let (u, v) = (p.arc_param(pt), q.arc_param(pt));
                if on(u, tp) && on(v, tq) {
                    out.push((u.clamp(0.0, 1.0), v.clamp(0.0, 1.0)));
                }
            }
        }
    }
    out
}

// ------------------------------------------------------------ arrangement

/// Where a piece came from, for naming the result and re-fitting splines.
#[derive(Clone, Copy, Debug, PartialEq, Eq, PartialOrd, Ord)]
struct Src {
    operand: u8,
    loop_idx: u32,
    /// The source segment; for a spline, every sample and join carries the
    /// spline's own index so the survivors form one run to re-fit.
    seg_idx: u32,
    bezier: bool,
}

#[derive(Clone, Debug)]
struct Piece {
    prim: Prim,
    src: Src,
}

/// A merged edge of the planar graph: one piece of geometry between two
/// vertices, and the sum of the traversals every operand makes along it.
#[derive(Clone, Debug)]
struct Edge {
    /// Rebuilt from the vertices it joins, so its ends are exactly `v0`, `v1`.
    prim: Prim,
    v0: usize,
    v1: usize,
    contrib: [i32; 2],
    srcs: Vec<Src>,
}

/// Vertex identity: a point within `eps` of an existing vertex *is* that
/// vertex, and the first point to arrive keeps its coordinates, so a
/// sketch's own corners come through bit for bit and every intersection is
/// the number that was computed for it.
struct Welder {
    eps: f64,
    cells: BTreeMap<(i64, i64), Vec<usize>>,
    verts: Vec<P2>,
}

impl Welder {
    fn new(eps: f64) -> Welder {
        Welder { eps, cells: BTreeMap::new(), verts: Vec::new() }
    }
    fn cell(&self, p: P2) -> (i64, i64) {
        ((p.x / self.eps).floor() as i64, (p.y / self.eps).floor() as i64)
    }
    fn find(&self, p: P2) -> Option<usize> {
        let (cx, cy) = self.cell(p);
        let mut best: Option<(f64, usize)> = None;
        for dx in -1..=1 {
            for dy in -1..=1 {
                if let Some(list) = self.cells.get(&(cx + dx, cy + dy)) {
                    for &v in list {
                        let d = dist(self.verts[v], p);
                        if d <= self.eps && best.map_or(true, |(bd, bv)| d < bd || (d == bd && v < bv)) {
                            best = Some((d, v));
                        }
                    }
                }
            }
        }
        best.map(|b| b.1)
    }
    fn weld(&mut self, p: P2) -> usize {
        if let Some(v) = self.find(p) {
            return v;
        }
        let key = self.cell(p);
        self.verts.push(p);
        let id = self.verts.len() - 1;
        self.cells.entry(key).or_default().push(id);
        id
    }
}

/// Vertices bucketed on a square grid of about one vertex per cell, for the
/// hot-pixel pass: which vertices lie in a box.
struct VertexGrid {
    lo: P2,
    cell: f64,
    cells: HashMap<(i64, i64), Vec<usize>>,
}

impl VertexGrid {
    fn new(verts: &[P2], eps: f64) -> VertexGrid {
        let mut lo = P2::new(f64::INFINITY, f64::INFINITY);
        let mut hi = P2::new(f64::NEG_INFINITY, f64::NEG_INFINITY);
        for p in verts {
            lo = P2::new(lo.x.min(p.x), lo.y.min(p.y));
            hi = P2::new(hi.x.max(p.x), hi.y.max(p.y));
        }
        if !lo.x.is_finite() {
            lo = P2::new(0.0, 0.0);
            hi = lo;
        }
        let side = (hi.x - lo.x).max(hi.y - lo.y);
        let cell = (side / (verts.len().max(1) as f64).sqrt()).max(eps * 4.0).max(1e-300);
        let mut grid = VertexGrid { lo, cell, cells: HashMap::new() };
        for (v, p) in verts.iter().enumerate() {
            let k = grid.key(*p);
            grid.cells.entry(k).or_default().push(v);
        }
        grid
    }

    fn key(&self, p: P2) -> (i64, i64) {
        (((p.x - self.lo.x) / self.cell).floor() as i64, ((p.y - self.lo.y) / self.cell).floor() as i64)
    }

    /// Every vertex in the cells the box touches (a superset of the box's).
    fn within(&self, lo: P2, hi: P2, out: &mut Vec<usize>) {
        out.clear();
        let (a, b) = (self.key(lo), self.key(hi));
        let span = (b.0 - a.0 + 1).saturating_mul(b.1 - a.1 + 1);
        if span > self.cells.len() as i64 {
            for list in self.cells.values() {
                out.extend_from_slice(list);
            }
            return;
        }
        for x in a.0..=b.0 {
            for y in a.1..=b.1 {
                if let Some(list) = self.cells.get(&(x, y)) {
                    out.extend_from_slice(list);
                }
            }
        }
    }
}

/// The piece between two vertices, on the carrier of `prim`, with its ends
/// exactly at the vertices.
fn rebuilt(prim: &Prim, a: P2, b: P2) -> Prim {
    match *prim {
        Prim::Line { .. } => Prim::Line { a, b },
        Prim::Arc { c, r, sweep, .. } => {
            let t0 = fmath::atan2(a.y - c.y, a.x - c.x);
            let t1 = fmath::atan2(b.y - c.y, b.x - c.x);
            let sw = if sweep > 0.0 { (t1 - t0).rem_euclid(TAU) } else { -((t0 - t1).rem_euclid(TAU)) };
            Prim::Arc { c, r, t0, sweep: sw }
        }
    }
}

/// Distance from `p` to the nearest point of the piece.
fn distance_to(prim: &Prim, p: P2) -> f64 {
    match *prim {
        Prim::Line { a, b } => {
            let d = sub(b, a);
            let l2 = dot(d, d);
            let u = if l2 > 0.0 { (dot(sub(p, a), d) / l2).clamp(0.0, 1.0) } else { 0.0 };
            dist(p, add(a, mul(d, u)))
        }
        Prim::Arc { c, r, .. } => {
            let u = prim.arc_param(p);
            if (0.0..=1.0).contains(&u) {
                (dist(p, c) - r).abs()
            } else {
                dist(p, prim.start()).min(dist(p, prim.end()))
            }
        }
    }
}

struct Arrangement {
    verts: Vec<P2>,
    edges: Vec<Edge>,
    /// Outgoing half-edges per vertex, counter-clockwise by tangent then curvature.
    out: Vec<Vec<usize>>,
    /// The face to the left of each half-edge (`2*edge + dir`).
    face: Vec<usize>,
    wn: Vec<[i32; 2]>,
    eps: f64,
}

struct HalfEdge {
    edge: usize,
    /// `false` along the edge's own orientation, `true` against it.
    rev: bool,
}
fn he(h: usize) -> HalfEdge {
    HalfEdge { edge: h >> 1, rev: h & 1 == 1 }
}

impl Arrangement {
    fn build(pieces: &[Piece], extent: f64) -> crate::Result<Arrangement> {
        let eps = (extent * 1e-9).max(1e-12);
        let n = pieces.len();
        // Every parameter at which a piece is cut, from every other piece.
        // A sweep along x: only pieces whose boxes overlap in x are tested.
        let boxes: Vec<(P2, P2)> = pieces.iter().map(|p| p.prim.bbox()).collect();
        let mut cuts: Vec<Vec<f64>> = vec![vec![0.0, 1.0]; n];
        let mut by_x: Vec<usize> = (0..n).collect();
        by_x.sort_by(|&a, &b| boxes[a].0.x.total_cmp(&boxes[b].0.x).then(a.cmp(&b)));
        for (k, &p) in by_x.iter().enumerate() {
            for &q in &by_x[k + 1..] {
                if boxes[q].0.x > boxes[p].1.x + eps {
                    break;
                }
                let (i, j) = if p < q { (p, q) } else { (q, p) };
                let (a, b) = (&boxes[i], &boxes[j]);
                if a.1.x < b.0.x - eps || b.1.x < a.0.x - eps || a.1.y < b.0.y - eps || b.1.y < a.0.y - eps {
                    continue;
                }
                for (u, v) in intersect(&pieces[i].prim, &pieces[j].prim, eps) {
                    cuts[i].push(u);
                    cuts[j].push(v);
                }
            }
        }
        // Split, weld, and rebuild each sub-piece from its welded ends.
        let mut welder = Welder::new(eps);
        struct Sub {
            prim: Prim,
            v0: usize,
            v1: usize,
            src: Src,
        }
        let mut subs: Vec<Sub> = Vec::new();
        for (i, piece) in pieces.iter().enumerate() {
            let mut us = cuts[i].clone();
            us.sort_by(|a, b| a.partial_cmp(b).unwrap());
            let mut u_prev = 0.0;
            let mut v_prev = welder.weld(piece.prim.start());
            for &u in us.iter().skip(1) {
                if u <= u_prev {
                    continue;
                }
                let v_next = welder.weld(piece.prim.point(u));
                let piece_prim = piece.prim.sub(u_prev, u);
                u_prev = u;
                if v_next == v_prev {
                    continue;
                }
                subs.push(Sub { prim: rebuilt(&piece_prim, welder.verts[v_prev], welder.verts[v_next]), v0: v_prev, v1: v_next, src: piece.src });
                v_prev = v_next;
            }
        }
        // Hot pixels: a piece running within `eps` of a vertex it does not
        // end at is split there, so the graph has no near-misses.
        // The candidates come from a bucket grid over the vertices, about
        // one vertex per cell, so a short piece looks at a handful of
        // vertices instead of all of them.
        let verts = welder.verts.clone();
        let grid = VertexGrid::new(&verts, eps);
        let mut near: Vec<usize> = Vec::new();
        let mut split_subs: Vec<Sub> = Vec::new();
        for sb in subs {
            let (lo, hi) = sb.prim.bbox();
            let mut hits: Vec<(f64, usize)> = Vec::new();
            grid.within(P2::new(lo.x - eps, lo.y - eps), P2::new(hi.x + eps, hi.y + eps), &mut near);
            for &v in &near {
                let p = &verts[v];
                if v == sb.v0 || v == sb.v1 || p.x < lo.x - eps || p.x > hi.x + eps || p.y < lo.y - eps || p.y > hi.y + eps {
                    continue;
                }
                if distance_to(&sb.prim, *p) <= eps {
                    let u = sb.prim.arc_param(*p).clamp(0.0, 1.0);
                    hits.push((u, v));
                }
            }
            if hits.is_empty() {
                split_subs.push(sb);
                continue;
            }
            hits.sort_by(|a, b| a.0.partial_cmp(&b.0).unwrap().then(a.1.cmp(&b.1)));
            hits.dedup_by_key(|h| h.1);
            let mut prev = sb.v0;
            for (_, v) in hits {
                if v != prev {
                    split_subs.push(Sub { prim: rebuilt(&sb.prim, verts[prev], verts[v]), v0: prev, v1: v, src: sb.src });
                    prev = v;
                }
            }
            if prev != sb.v1 {
                split_subs.push(Sub { prim: rebuilt(&sb.prim, verts[prev], verts[sb.v1]), v0: prev, v1: sb.v1, src: sb.src });
            }
        }
        // Merge coincident sub-pieces: same two vertices, same midpoint.
        let mut edges: Vec<Edge> = Vec::new();
        let mut by_pair: BTreeMap<(usize, usize), Vec<usize>> = BTreeMap::new();
        for sb in split_subs {
            if sb.prim.length() <= 0.0 {
                continue;
            }
            let forward = sb.v0 <= sb.v1;
            let (lo, hi) = if forward { (sb.v0, sb.v1) } else { (sb.v1, sb.v0) };
            let prim = if forward { sb.prim } else { sb.prim.reversed() };
            let mid = prim.point(0.5);
            let sign = if forward { 1 } else { -1 };
            let list = by_pair.entry((lo, hi)).or_default();
            match list.iter().copied().find(|&e| dist(edges[e].prim.point(0.5), mid) <= eps) {
                Some(e) => {
                    edges[e].contrib[sb.src.operand as usize] += sign;
                    edges[e].srcs.push(sb.src);
                }
                None => {
                    let mut contrib = [0i32; 2];
                    contrib[sb.src.operand as usize] = sign;
                    list.push(edges.len());
                    edges.push(Edge { prim, v0: lo, v1: hi, contrib, srcs: vec![sb.src] });
                }
            }
        }
        // Half-edges around each vertex, counter-clockwise.
        let mut out: Vec<Vec<usize>> = vec![Vec::new(); verts.len()];
        for (e, edge) in edges.iter().enumerate() {
            out[edge.v0].push(2 * e);
            out[edge.v1].push(2 * e + 1);
        }
        // The order is a total order on (angle bucket, curvature, index):
        // the angle quantised to a nanoradian, with a full turn folded to
        // zero, so two tangents that agree to roundoff tie and the tie goes
        // to curvature (an arc bending left sorts after the line it is
        // tangent to). A tolerance comparison here is not transitive and
        // once handed `sort_by` an order it could not honour.
        let leaving = |h: usize| -> (i64, f64) {
            let HalfEdge { edge, rev } = he(h);
            let p = &edges[edge].prim;
            let (t, k) = if rev {
                (p.tangent(1.0).map(|t| mul(t, -1.0)), -p.curvature())
            } else {
                (p.tangent(0.0), p.curvature())
            };
            let t = t.unwrap_or(P2::new(1.0, 0.0));
            let a = fmath::atan2(t.y, t.x).rem_euclid(TAU);
            let bucket = (a / 1e-9).round() as i64;
            let full = (TAU / 1e-9).round() as i64;
            (if bucket >= full { 0 } else { bucket }, k)
        };
        for list in out.iter_mut() {
            let mut keyed: Vec<(i64, f64, usize)> = list.iter().map(|&h| {
                let (a, k) = leaving(h);
                (a, k, h)
            }).collect();
            keyed.sort_by(|a, b| a.0.cmp(&b.0).then(a.1.partial_cmp(&b.1).unwrap()).then(a.2.cmp(&b.2)));
            *list = keyed.into_iter().map(|k| k.2).collect();
        }
        let mut arr = Arrangement { verts, edges, out, face: Vec::new(), wn: Vec::new(), eps };
        arr.trace_faces();
        arr.classify_faces();
        Ok(arr)
    }

    fn head(&self, h: usize) -> usize {
        let HalfEdge { edge, rev } = he(h);
        if rev { self.edges[edge].v0 } else { self.edges[edge].v1 }
    }

    /// The half-edge that continues the face on the left of `h`: at its head,
    /// the clockwise neighbour of its twin.
    fn next(&self, h: usize) -> usize {
        let v = self.head(h);
        let list = &self.out[v];
        let j = list.iter().position(|&g| g == (h ^ 1)).expect("twin is registered at its tail");
        list[(j + list.len() - 1) % list.len()]
    }

    fn trace_faces(&mut self) {
        let nh = 2 * self.edges.len();
        let mut face = vec![usize::MAX; nh];
        let mut count = 0;
        for h0 in 0..nh {
            if face[h0] != usize::MAX {
                continue;
            }
            let mut h = h0;
            loop {
                face[h] = count;
                h = self.next(h);
                if h == h0 {
                    break;
                }
            }
            count += 1;
        }
        self.face = face;
        self.wn = vec![[0; 2]; count];
    }

    /// The winding numbers of every face.
    ///
    /// Crossing an edge from its right to its left adds its `contrib`, so
    /// within one connected piece of the arrangement one face's winding
    /// number gives every other's by a walk across the edges. One face per
    /// connected piece — the one left of the piece's longest half-edge, at
    /// its midpoint — is evaluated by a ray against every edge, and the
    /// rest follow. A ray per face cost faces × edges, which is quartic in
    /// the samples when a large offset of a spline crosses itself
    /// everywhere.
    fn classify_faces(&mut self) {
        let nf = self.wn.len();
        // Connected pieces of the graph, by vertex.
        let mut root: Vec<usize> = (0..self.verts.len()).collect();
        fn find(root: &mut [usize], mut v: usize) -> usize {
            while root[v] != v {
                root[v] = root[root[v]];
                v = root[v];
            }
            v
        }
        for e in &self.edges {
            let (a, b) = (find(&mut root, e.v0), find(&mut root, e.v1));
            if a != b {
                root[a.max(b)] = a.min(b);
            }
        }
        // The seed of each piece: its longest half-edge, lowest index on a tie.
        let mut seed: BTreeMap<usize, (f64, usize)> = BTreeMap::new();
        for h in 0..2 * self.edges.len() {
            let piece = find(&mut root, self.edges[h >> 1].v0);
            let l = self.edges[h >> 1].prim.length();
            let entry = seed.entry(piece).or_insert((l, h));
            if l > entry.0 {
                *entry = (l, h);
            }
        }
        // Across each edge: (face on its left, face on its right, contrib).
        let mut across: Vec<Vec<(usize, [i32; 2])>> = vec![Vec::new(); nf];
        for (e, edge) in self.edges.iter().enumerate() {
            let (l, r) = (self.face[2 * e], self.face[2 * e + 1]);
            let c = edge.contrib;
            across[r].push((l, c));
            across[l].push((r, [-c[0], -c[1]]));
        }
        let mut known = vec![false; nf];
        let mut queue: std::collections::VecDeque<usize> = std::collections::VecDeque::new();
        for &(_, h) in seed.values() {
            let f = self.face[h];
            if known[f] {
                continue;
            }
            let HalfEdge { edge, rev } = he(h);
            let prim = self.edges[edge].prim;
            let m = prim.point(0.5);
            let t = prim.tangent(0.5).unwrap_or(P2::new(1.0, 0.0));
            let t = if rev { mul(t, -1.0) } else { t };
            self.wn[f] = self.winding_at(m, left(t), edge);
            known[f] = true;
            queue.push_back(f);
            while let Some(f) = queue.pop_front() {
                for &(g, c) in &across[f] {
                    if !known[g] {
                        self.wn[g] = [self.wn[f][0] + c[0], self.wn[f][1] + c[1]];
                        known[g] = true;
                        queue.push_back(g);
                    }
                }
            }
        }
    }

    /// Winding numbers per operand at `p`, by a ray in direction `n`, with
    /// `exclude` (the edge `p` sits on) left out. The half-open rule: a piece
    /// crossing the ray line at an interior point counts by the direction it
    /// crosses; a piece with an end on the line counts only if it extends to
    /// the `+t` side (`t = left(n)`), leaving as `+1` and arriving as `-1`.
    fn winding_at(&self, p: P2, n: P2, exclude: usize) -> [i32; 2] {
        let t = left(n);
        let eps = self.eps;
        let mut wn = [0i32; 2];
        for (e, edge) in self.edges.iter().enumerate() {
            if e == exclude || edge.contrib == [0, 0] {
                continue;
            }
            let (lo, hi) = edge.prim.bbox();
            // The ray line's strip, cheaply: every point of the piece must be
            // within the box, and the box must reach the line.
            let corners = [lo, P2::new(lo.x, hi.y), hi, P2::new(hi.x, lo.y)];
            let ts: Vec<f64> = corners.iter().map(|c| dot(sub(*c, p), t)).collect();
            if ts.iter().all(|v| *v > eps) || ts.iter().all(|v| *v < -eps) {
                continue;
            }
            let count = ray_crossings(&edge.prim, p, n, t, eps);
            if count != 0 {
                for k in 0..2 {
                    wn[k] += count * edge.contrib[k];
                }
            }
        }
        wn
    }

    fn face_in(&self, h: usize, rule: &dyn Fn([i32; 2]) -> bool) -> bool {
        rule(self.wn[self.face[h]])
    }

    /// The loops bounding the faces `rule` accepts, walked with those faces
    /// on the left, as sequences of half-edges.
    fn boundary_loops(&self, rule: &dyn Fn([i32; 2]) -> bool) -> crate::Result<Vec<Vec<usize>>> {
        let nh = 2 * self.edges.len();
        let is_boundary = |h: usize| self.face_in(h, rule) && !self.face_in(h ^ 1, rule);
        let mut seen = vec![false; nh];
        let mut loops = Vec::new();
        for h0 in 0..nh {
            if seen[h0] || !is_boundary(h0) {
                continue;
            }
            let mut cycle = Vec::new();
            let mut h = h0;
            loop {
                if seen[h] {
                    return Err(crate::CadpoleError::kernel("2D clipper: boundary walk revisited a half-edge"));
                }
                seen[h] = true;
                cycle.push(h);
                // At the head, sweep clockwise from the twin until the sector
                // beyond an outgoing edge is outside the result.
                let v = self.head(h);
                let list = &self.out[v];
                let k = list.len();
                let j = list.iter().position(|&g| g == (h ^ 1)).unwrap();
                let mut chosen = None;
                for step in 1..=k {
                    let g = list[(j + k - step) % k];
                    let prev = list[(j + 2 * k - step - 1) % k];
                    if self.face_in(g, rule) && !self.face_in(prev, rule) {
                        chosen = Some(g);
                        break;
                    }
                }
                let Some(g) = chosen else {
                    return Err(crate::CadpoleError::kernel("2D clipper: boundary walk found no continuation"));
                };
                if g == h0 {
                    break;
                }
                h = g;
                if cycle.len() > nh {
                    return Err(crate::CadpoleError::kernel("2D clipper: boundary walk did not close"));
                }
            }
            loops.push(cycle);
        }
        Ok(loops)
    }

    /// The geometry along half-edge `h`: its piece, oriented, with the exact
    /// coordinates of the vertices it runs from and to.
    fn along(&self, h: usize) -> (Prim, P2, P2) {
        let HalfEdge { edge, rev } = he(h);
        let e = &self.edges[edge];
        if rev { (e.prim.reversed(), self.verts[e.v1], self.verts[e.v0]) } else { (e.prim, self.verts[e.v0], self.verts[e.v1]) }
    }
}

/// Signed crossings of one piece with the ray from `p` along `n` (see
/// [`Arrangement::winding_at`]).
fn ray_crossings(prim: &Prim, p: P2, n: P2, t: P2, eps: f64) -> i32 {
    let side = |q: P2| dot(sub(q, p), t);
    let ahead = |q: P2| dot(sub(q, p), n) > eps;
    let mut count = 0;
    match *prim {
        Prim::Line { a, b } => {
            let (ta, tb) = (side(a), side(b));
            if ta.abs() <= eps && tb.abs() <= eps {
                return 0;
            }
            if ta.abs() <= eps {
                if tb > 0.0 && ahead(a) {
                    count += 1;
                }
            } else if tb.abs() <= eps {
                if ta > 0.0 && ahead(b) {
                    count -= 1;
                }
            } else if (ta > 0.0) != (tb > 0.0) {
                let u = ta / (ta - tb);
                let q = P2::new(a.x + (b.x - a.x) * u, a.y + (b.y - a.y) * u);
                if ahead(q) {
                    count += if tb > ta { 1 } else { -1 };
                }
            }
        }
        Prim::Arc { c, r, .. } => {
            let (a, b) = (prim.start(), prim.end());
            let (ta, tb) = (side(a), side(b));
            let h = side(c);
            // Ends on the line: does the arc extend to +t there? By the
            // tangent, or when the tangent runs along the line, by which
            // side it bends to.
            let extends = |q_t: P2, s: f64| -> bool {
                let along = dot(q_t, t);
                if along.abs() > 1e-9 { along > 0.0 } else { s > 0.0 }
            };
            let mut skip_lo = false;
            let mut skip_hi = false;
            if ta.abs() <= eps {
                skip_lo = true;
                let tan = prim.tangent(0.0).unwrap();
                if extends(tan, h) && ahead(a) {
                    count += 1;
                }
            }
            if tb.abs() <= eps {
                skip_hi = true;
                let tan = mul(prim.tangent(1.0).unwrap(), -1.0);
                if extends(tan, h) && ahead(b) {
                    count -= 1;
                }
            }
            if h.abs() >= r - eps {
                // Tangent to the line, or clear of it: nothing crosses.
                return count;
            }
            let w = (r * r - h * h).sqrt();
            let foot = add(c, mul(t, -h));
            for q in [add(foot, mul(n, w)), add(foot, mul(n, -w))] {
                let u = prim.arc_param(q);
                let tol = eps / prim.length().max(eps);
                if u <= tol || u >= 1.0 - tol {
                    // An end: handled above, or off the arc.
                    if (u <= tol && skip_lo) || (u >= 1.0 - tol && skip_hi) || u < -tol || u > 1.0 + tol {
                        continue;
                    }
                    if !(-tol..=1.0 + tol).contains(&u) {
                        continue;
                    }
                }
                if !ahead(q) {
                    continue;
                }
                let tan = prim.tangent(u.clamp(0.0, 1.0)).unwrap();
                let along = dot(tan, t);
                if along > 0.0 {
                    count += 1;
                } else if along < 0.0 {
                    count -= 1;
                }
            }
        }
    }
    count
}

// --------------------------------------------------------- operand loops

/// A sketch's loops oriented by their nesting — outer loops counter-clockwise,
/// holes clockwise, islands counter-clockwise again — so that "material" is
/// exactly where the winding number is positive. Indexed like `sketch.loops`.
fn oriented_loops(sketch: &Sketch) -> crate::Result<Vec<Loop>> {
    let mut out: Vec<Option<Loop>> = vec![None; sketch.loops.len()];
    for region in sketch.regions_indexed()? {
        out[region.outer.0] = Some(region.outer.1);
        for (i, hole) in region.holes {
            out[i] = Some(hole);
        }
    }
    Ok(out.into_iter().enumerate().map(|(i, l)| l.unwrap_or_else(|| sketch.loops[i].clone())).collect())
}

/// The name a loop is referred to by: its own, or `<feature>/loop[i]`.
pub fn loop_base_name(feature: &str, sketch: &Sketch, i: usize) -> String {
    let own = sketch.loop_name(i);
    if own.is_empty() { format!("{feature}/loop[{i}]") } else { own.to_owned() }
}

fn extent_of(loops: &[Loop]) -> f64 {
    let mut lo = P2::new(f64::INFINITY, f64::INFINITY);
    let mut hi = P2::new(f64::NEG_INFINITY, f64::NEG_INFINITY);
    for l in loops {
        for p in l.sample() {
            lo = P2::new(lo.x.min(p.x), lo.y.min(p.y));
            hi = P2::new(hi.x.max(p.x), hi.y.max(p.y));
        }
    }
    if lo.x.is_finite() { (hi.x - lo.x).max(hi.y - lo.y).max(1.0) } else { 1.0 }
}

/// The pieces of one operand's loops, tagged.
fn operand_pieces(loops: &[Loop], operand: u8, tolerance: f64) -> Vec<Piece> {
    let mut out = Vec::new();
    for (li, l) in loops.iter().enumerate() {
        for (si, seg) in l.segments.iter().enumerate() {
            let bezier = matches!(seg, Segment::Bezier { .. });
            let src = Src { operand, loop_idx: li as u32, seg_idx: si as u32, bezier };
            for prim in prims_of(seg, tolerance) {
                if prim.length() > 0.0 {
                    out.push(Piece { prim, src });
                }
            }
        }
    }
    out
}

// ----------------------------------------------------------- result loops

/// A loop of the result with the sources that built it.
struct Built {
    segments: Vec<Segment>,
    sources: BTreeSet<Src>,
}

/// Turn the walked boundary cycles into sketch loops: merge pieces that
/// continue on one carrier, re-fit the runs that came from a spline, and
/// drop what is smaller than the weld grid. Segment ends are the
/// arrangement's vertex coordinates, so consecutive segments share them
/// exactly and every loop closes.
fn build_loops(arr: &Arrangement, cycles: &[Vec<usize>], tolerance: f64) -> Vec<Built> {
    let mut out = Vec::new();
    for cycle in cycles {
        // (carrier, from, to, source)
        let mut prims: Vec<(Prim, P2, P2, Src)> = Vec::new();
        let mut sources = BTreeSet::new();
        for &h in cycle {
            let e = &arr.edges[h >> 1];
            let src = e.srcs[0];
            for s in &e.srcs {
                sources.insert(*s);
            }
            let (prim, from, to) = arr.along(h);
            // Continue on the same carrier: a line split by a touch, an arc
            // split at a vertex the result does not use.
            if let Some((last, lfrom, lto, lsrc)) = prims.last_mut() {
                let same_run = lsrc.bezier == src.bezier && (!src.bezier || (lsrc.loop_idx, lsrc.operand) == (src.loop_idx, src.operand));
                let joined = match (*last, prim) {
                    (Prim::Line { .. }, Prim::Line { .. }) => {
                        let (t1, t2) = (sub(*lto, *lfrom), sub(to, from));
                        (cross(t1, t2).abs() <= 1e-9 * norm(t1) * norm(t2) && dot(t1, t2) > 0.0).then_some(Prim::Line { a: *lfrom, b: to })
                    }
                    // Never past a half turn: a circle stays two arcs, as
                    // `Loop::circle` draws it, because an arc's three
                    // points cannot say "all the way round".
                    (Prim::Arc { c: c1, r: r1, t0, sweep: s1 }, Prim::Arc { c: c2, r: r2, sweep: s2, .. })
                        if dist(c1, c2) <= arr.eps * 2.0 && (r1 - r2).abs() <= arr.eps * 2.0 && s1.signum() == s2.signum() && (s1 + s2).abs() <= PI + 1e-9 =>
                    {
                        Some(Prim::Arc { c: c1, r: r1, t0, sweep: s1 + s2 })
                    }
                    _ => None,
                };
                if let (Some(j), true) = (joined, same_run) {
                    *last = j;
                    *lto = to;
                    continue;
                }
            }
            prims.push((prim, from, to, src));
        }
        if prims.len() >= 2 {
            // The seam may fall inside a merged carrier too.
            let (first, ffrom, fto, fsrc) = prims[0];
            let (last, lfrom, lto, lsrc) = *prims.last().unwrap();
            if let (Prim::Line { .. }, Prim::Line { .. }) = (last, first) {
                let (t1, t2) = (sub(lto, lfrom), sub(fto, ffrom));
                if cross(t1, t2).abs() <= 1e-9 * norm(t1) * norm(t2) && dot(t1, t2) > 0.0 && !fsrc.bezier && !lsrc.bezier {
                    prims[0] = (Prim::Line { a: lfrom, b: fto }, lfrom, fto, fsrc);
                    prims.pop();
                }
            }
        }
        // Splines: a run of pieces from one spline chain is one polyline to re-fit.
        let mut segments: Vec<Segment> = Vec::new();
        let mut i = 0;
        while i < prims.len() {
            let (p, from, to, s) = prims[i];
            if !s.bezier {
                segments.push(match p {
                    Prim::Line { .. } => Segment::Line { a: from, b: to },
                    Prim::Arc { .. } => {
                        let via = p.point(0.5);
                        if arc_params(from, to, via).is_none() { Segment::Line { a: from, b: to } } else { Segment::Arc { a: from, b: to, via } }
                    }
                });
                i += 1;
                continue;
            }
            let mut pts = vec![from, to];
            let mut j = i + 1;
            while j < prims.len() {
                let (_, _, qto, sq) = prims[j];
                if sq.bezier && (sq.operand, sq.loop_idx) == (s.operand, s.loop_idx) {
                    pts.push(qto);
                    j += 1;
                } else {
                    break;
                }
            }
            segments.extend(fit_bezier_chain(&pts, tolerance));
            i = j;
        }
        drop_hairlines(&mut segments, (tolerance * 1e-2).max(arr.eps * 100.0));
        let l = Loop { segments };
        let pts = l.sample();
        let (mut lo, mut hi) = (P2::new(f64::INFINITY, f64::INFINITY), P2::new(f64::NEG_INFINITY, f64::NEG_INFINITY));
        for p in &pts {
            lo = P2::new(lo.x.min(p.x), lo.y.min(p.y));
            hi = P2::new(hi.x.max(p.x), hi.y.max(p.y));
        }
        if l.segments.len() < 2 || (hi.x - lo.x).max(hi.y - lo.y) <= arr.eps * 100.0 {
            continue;
        }
        out.push(Built { segments: l.segments, sources });
    }
    out
}

/// Remove the segments of a closed chain shorter than `min_len`, joining
/// their neighbours at the dropped segment's start.
///
/// The arrangement keeps every piece longer than its weld grid (about 1e-9
/// of the sketch's extent), so a trimmed corner can leave a piece a few
/// nanometres to a micron long between two real segments. As geometry it is
/// nothing: it is far below any tolerance the sketch is used at. As topology
/// it is an edge whose two vertices nearly coincide, and every consumer
/// that builds something on each segment (an emboss wall, a sweep) gets a
/// face with two corners at one point, which the tessellator cannot
/// triangulate. Moving the next segment's start onto the dropped segment's
/// start keeps the chain closed and shifts that one end by less than
/// `min_len`.
fn drop_hairlines(segments: &mut Vec<Segment>, min_len: f64) {
    let mut i = 0;
    while i < segments.len() {
        if segments.len() <= 2 {
            return;
        }
        if segments[i].length() >= min_len {
            i += 1;
            continue;
        }
        let start = segments[i].start();
        segments.remove(i);
        let k = if i < segments.len() { i } else { 0 };
        match &mut segments[k] {
            Segment::Line { a, .. } | Segment::Arc { a, .. } | Segment::Bezier { a, .. } => *a = start,
        }
        // An arc whose start moved can no longer pass through its `via`
        // as a proper arc when it was itself barely longer than the shift:
        // it becomes its chord.
        if let Segment::Arc { a, b, via } = segments[k]
            && arc_params(a, b, via).is_none()
        {
            segments[k] = Segment::Line { a, b };
        }
    }
}

/// Name every built loop from its sources, `#k` in canonical order when two
/// results share a name (a loop the offset split in two).
fn name_loops(built: &[Built], plane: &Plane, base: &dyn Fn(Src) -> String, suffix: &str) -> Vec<String> {
    let mut names: Vec<String> = built
        .iter()
        .map(|b| {
            let parts: Vec<String> = b.sources.iter().map(|s| (s.operand, s.loop_idx)).collect::<BTreeSet<_>>().into_iter().map(|(o, l)| base(Src { operand: o, loop_idx: l, seg_idx: 0, bezier: false })).collect();
            format!("{}{suffix}", parts.join("|"))
        })
        .collect();
    let mut groups: BTreeMap<String, Vec<usize>> = BTreeMap::new();
    for (i, n) in names.iter().enumerate() {
        groups.entry(n.clone()).or_default().push(i);
    }
    for (name, members) in groups {
        if members.len() < 2 {
            continue;
        }
        let pieces: Vec<(Vec3, Vec3, f64)> = members
            .iter()
            .map(|&i| {
                let l = Loop { segments: built[i].segments.clone() };
                let pts = l.sample();
                let n = pts.len().max(1) as f64;
                let (sx, sy) = pts.iter().fold((0.0, 0.0), |(x, y), p| (x + p.x, y + p.y));
                (plane.to_world(sx / n, sy / n), plane.normal, l.signed_area().abs())
            })
            .collect();
        for (k, &pi) in naming::order_pieces(&pieces).iter().enumerate() {
            names[members[pi]] = format!("{name}#{k}");
        }
    }
    names
}

fn assemble(plane: Plane, built: Vec<Built>, names: Vec<String>) -> Sketch {
    let mut s = Sketch::new(plane);
    for (b, n) in built.into_iter().zip(names) {
        s.push_named_loop(Loop { segments: b.segments }, n);
    }
    s
}

// ------------------------------------------------------------ Bézier fit

/// Cubic Béziers through `pts` within `tol` (Schneider, Graphics Gems 1990):
/// chord-length parameters, the two handle lengths by least squares, a few
/// Newton reparametrisations, and a split at the worst point when that is
/// not enough. Deterministic: fixed iteration counts, no randomness.
fn fit_bezier_chain(pts: &[P2], tol: f64) -> Vec<Segment> {
    let pts: Vec<P2> = {
        let mut v: Vec<P2> = Vec::new();
        for p in pts {
            if v.last().map_or(true, |l| dist(*l, *p) > 1e-12) {
                v.push(*p);
            }
        }
        v
    };
    if pts.len() < 2 {
        return Vec::new();
    }
    if pts.len() == 2 {
        return vec![Segment::Line { a: pts[0], b: pts[1] }];
    }
    let n = pts.len();
    let t1 = unit(sub(pts[1], pts[0])).unwrap();
    let t2 = unit(sub(pts[n - 2], pts[n - 1])).unwrap();
    let mut out = Vec::new();
    fit_cubic(&pts, t1, t2, tol.max(1e-9), 0, &mut out);
    out
}

fn fit_cubic(pts: &[P2], t1: P2, t2: P2, tol: f64, depth: usize, out: &mut Vec<Segment>) {
    let n = pts.len();
    if n == 2 {
        let d = dist(pts[0], pts[1]) / 3.0;
        out.push(Segment::Bezier { a: pts[0], b: pts[1], ctrl: vec![add(pts[0], mul(t1, d)), add(pts[1], mul(t2, d))] });
        return;
    }
    let mut u = chord_params(pts);
    let mut bez = generate_bezier(pts, &u, t1, t2);
    let (mut err, mut split) = max_error(pts, &bez, &u);
    if err > tol && err <= tol * 4.0 {
        for _ in 0..4 {
            u = reparametrize(pts, &bez, &u);
            bez = generate_bezier(pts, &u, t1, t2);
            let (e, s) = max_error(pts, &bez, &u);
            err = e;
            split = s;
            if err <= tol {
                break;
            }
        }
    }
    if err <= tol || depth >= 24 {
        out.push(Segment::Bezier { a: bez[0], b: bez[3], ctrl: vec![bez[1], bez[2]] });
        return;
    }
    let split = split.clamp(1, n - 2);
    let centre = unit(sub(pts[split - 1], pts[split + 1])).unwrap_or(t1);
    fit_cubic(&pts[..=split], t1, centre, tol, depth + 1, out);
    fit_cubic(&pts[split..], mul(centre, -1.0), t2, tol, depth + 1, out);
}

fn chord_params(pts: &[P2]) -> Vec<f64> {
    let mut u = vec![0.0; pts.len()];
    for i in 1..pts.len() {
        u[i] = u[i - 1] + dist(pts[i], pts[i - 1]);
    }
    let total = u[pts.len() - 1];
    if total > 0.0 {
        for v in u.iter_mut() {
            *v /= total;
        }
    }
    u
}

fn bernstein(t: f64) -> [f64; 4] {
    let s = 1.0 - t;
    [s * s * s, 3.0 * t * s * s, 3.0 * t * t * s, t * t * t]
}

fn generate_bezier(pts: &[P2], u: &[f64], t1: P2, t2: P2) -> [P2; 4] {
    let (p0, p3) = (pts[0], pts[pts.len() - 1]);
    let (mut c11, mut c12, mut c22, mut x1, mut x2) = (0.0, 0.0, 0.0, 0.0, 0.0);
    for (p, &t) in pts.iter().zip(u) {
        let b = bernstein(t);
        let a1 = mul(t1, b[1]);
        let a2 = mul(t2, b[2]);
        c11 += dot(a1, a1);
        c12 += dot(a1, a2);
        c22 += dot(a2, a2);
        let tmp = sub(*p, add(add(mul(p0, b[0] + b[1]), mul(p3, b[2] + b[3])), P2::new(0.0, 0.0)));
        x1 += dot(a1, tmp);
        x2 += dot(a2, tmp);
    }
    let det = c11 * c22 - c12 * c12;
    let (mut a1, mut a2) = if det.abs() > 1e-18 { ((x1 * c22 - x2 * c12) / det, (c11 * x2 - c12 * x1) / det) } else { (0.0, 0.0) };
    let seg = dist(p0, p3);
    let tiny = seg * 1e-6;
    if a1 < tiny || a2 < tiny {
        a1 = seg / 3.0;
        a2 = seg / 3.0;
    }
    [p0, add(p0, mul(t1, a1)), add(p3, mul(t2, a2)), p3]
}

fn max_error(pts: &[P2], bez: &[P2; 4], u: &[f64]) -> (f64, usize) {
    let mut worst = (0.0, pts.len() / 2);
    for (i, (p, &t)) in pts.iter().zip(u).enumerate() {
        let d = dist(bezier_point(bez, t), *p);
        if d > worst.0 {
            worst = (d, i);
        }
    }
    worst
}

fn reparametrize(pts: &[P2], bez: &[P2; 4], u: &[f64]) -> Vec<f64> {
    let d1: Vec<P2> = (0..3).map(|i| mul(sub(bez[i + 1], bez[i]), 3.0)).collect();
    let d2: Vec<P2> = (0..2).map(|i| mul(sub(d1[i + 1], d1[i]), 2.0)).collect();
    pts.iter()
        .zip(u)
        .map(|(p, &t)| {
            let q = bezier_point(bez, t);
            let q1 = bezier_point(&d1, t);
            let q2 = bezier_point(&d2, t);
            let num = dot(sub(q, *p), q1);
            let den = dot(q1, q1) + dot(sub(q, *p), q2);
            if den.abs() < 1e-18 { t } else { (t - num / den).clamp(0.0, 1.0) }
        })
        .collect()
}

// ----------------------------------------------------------------- offset

/// The raw offset curve of one oriented loop (material on its left).
fn raw_offset(l: &Loop, li: usize, d: f64, join: Join, tolerance: f64) -> Vec<Piece> {
    // Elements: the loop's pieces with their source; a spline is its samples.
    let mut elems: Vec<(Prim, Src)> = Vec::new();
    for (si, seg) in l.segments.iter().enumerate() {
        let bezier = matches!(seg, Segment::Bezier { .. });
        let src = Src { operand: 0, loop_idx: li as u32, seg_idx: si as u32, bezier };
        for p in prims_of(seg, tolerance) {
            if p.length() > 0.0 {
                elems.push((p, src));
            }
        }
    }
    let n = elems.len();
    let mut out = Vec::new();
    if n == 0 {
        return out;
    }
    // Each element's offset, or its collapse to a centre.
    let offset_of = |p: &Prim| -> Prim {
        match *p {
            Prim::Line { a, b } => {
                let nr = mul(right(unit(sub(b, a)).unwrap()), d);
                Prim::Line { a: add(a, nr), b: add(b, nr) }
            }
            Prim::Arc { c, r, t0, sweep } => {
                let r2 = if sweep > 0.0 { r + d } else { r - d };
                if r2 <= 1e-12 { Prim::Line { a: c, b: c } } else { Prim::Arc { c, r: r2, t0, sweep } }
            }
        }
    };
    let offs: Vec<Prim> = elems.iter().map(|(p, _)| offset_of(p)).collect();
    // What happens at the vertex after element i.
    #[derive(Clone, Copy, PartialEq)]
    enum At {
        Straight,
        Gap(f64),
        /// The two offset pieces meet: trim both to the crossing (parameters
        /// on the piece before and the piece after).
        Meet(f64, f64),
        /// They overlap without meeting: the spike through the corner.
        Spike,
    }
    let mut at: Vec<At> = Vec::with_capacity(n);
    for i in 0..n {
        let k = (i + 1) % n;
        let (p, _) = elems[i];
        let (q, _) = elems[k];
        let t_in = p.tangent(1.0).unwrap();
        let t_out = q.tangent(0.0).unwrap();
        let turn = cross(t_in, t_out);
        if turn.abs() <= 1e-9 && dot(t_in, t_out) > 0.0 {
            at.push(At::Straight);
            continue;
        }
        // A reversal (a slit tip) turns right for the sign rule.
        let turn = if turn.abs() <= 1e-9 { -1.0 } else { turn };
        let gap = (turn > 0.0 && d > 0.0) || (turn < 0.0 && d < 0.0);
        if gap {
            at.push(At::Gap(turn));
            continue;
        }
        // Overlap: the offset pieces cross where the true offset turns the
        // corner. Trimming to that crossing drops a loop of winding ±1 that
        // lies inside the two pieces' offset rectangles, so the region with
        // positive winding number is unchanged — and the hairline
        // overshoots that would otherwise sit there are gone.
        let (o, ok) = (offs[i], offs[k]);
        let mut best: Option<(f64, f64)> = None;
        if o.length() > 0.0 && ok.length() > 0.0 && n > 1 {
            for (u, v) in intersect(&o, &ok, 1e-12) {
                if best.map_or(true, |(bu, _)| u > bu) {
                    best = Some((u, v));
                }
            }
        }
        at.push(best.map_or(At::Spike, |(u, v)| At::Meet(u, v)));
    }
    // A piece trimmed from both ends must keep some length; one that would
    // not falls back to spikes at both its vertices.
    for i in 0..n {
        let before = (i + n - 1) % n;
        let s0 = match at[before] { At::Meet(_, v) => v, _ => 0.0 };
        let s1 = match at[i] { At::Meet(u, _) => u, _ => 1.0 };
        if s0 >= s1 - 1e-12 {
            if matches!(at[before], At::Meet(..)) {
                at[before] = At::Spike;
            }
            if matches!(at[i], At::Meet(..)) {
                at[i] = At::Spike;
            }
        }
    }
    for i in 0..n {
        let (p, src) = elems[i];
        let k = (i + 1) % n;
        let (q, qsrc) = elems[k];
        let before = (i + n - 1) % n;
        let s0 = match at[before] { At::Meet(_, v) => v, _ => 0.0 };
        let s1 = match at[i] { At::Meet(u, _) => u, _ => 1.0 };
        let o = offs[i].sub(s0, s1);
        if o.length() > 0.0 {
            out.push(Piece { prim: o, src });
        }
        let v = p.end();
        let t_in = p.tangent(1.0).unwrap();
        let t_out = q.tangent(0.0).unwrap();
        let e1 = o.end();
        let e2 = match at[i] { At::Meet(_, v2) => offs[k].point(v2), _ => offs[k].start() };
        // Between two samples of one spline chain the join is the chain's
        // own: a bevel, tagged so the survivors re-fit as one curve.
        let join_src = Src { bezier: src.bezier && qsrc.bezier, ..src };
        match at[i] {
            At::Meet(..) => {}
            At::Straight => {
                if dist(e1, e2) > 1e-12 {
                    out.push(Piece { prim: Prim::Line { a: e1, b: e2 }, src: join_src });
                }
            }
            At::Spike => {
                for prim in [Prim::Line { a: e1, b: v }, Prim::Line { a: v, b: e2 }] {
                    if prim.length() > 0.0 {
                        out.push(Piece { prim, src: join_src });
                    }
                }
            }
            At::Gap(turn) => {
                if dist(e1, e2) <= 1e-12 {
                    continue;
                }
                let style = if join_src.bezier { Join::Bevel } else { join };
                match style {
                    Join::Round => {
                        let a0 = fmath::atan2(e1.y - v.y, e1.x - v.x);
                        let a1 = fmath::atan2(e2.y - v.y, e2.x - v.x);
                        let mut sweep = (a1 - a0).rem_euclid(TAU);
                        if turn < 0.0 {
                            sweep -= TAU;
                        }
                        for prim in split_long_arc(Prim::Arc { c: v, r: d.abs(), t0: a0, sweep }) {
                            out.push(Piece { prim, src: join_src });
                        }
                    }
                    Join::Bevel => out.push(Piece { prim: Prim::Line { a: e1, b: e2 }, src: join_src }),
                    Join::Miter => {
                        // Tangent lines at the two offset ends meet at the mitre point.
                        let ta = o.tangent(1.0).unwrap_or(t_in);
                        let tb = offs[k].tangent(0.0).unwrap_or(t_out);
                        let den = cross(ta, tb);
                        let m = if den.abs() > 1e-12 { Some(add(e1, mul(ta, cross(sub(e2, e1), tb) / den))) } else { None };
                        match m {
                            Some(m) if dist(m, v) <= MITER_LIMIT * d.abs() => {
                                out.push(Piece { prim: Prim::Line { a: e1, b: m }, src: join_src });
                                out.push(Piece { prim: Prim::Line { a: m, b: e2 }, src: join_src });
                            }
                            _ => out.push(Piece { prim: Prim::Line { a: e1, b: e2 }, src: join_src }),
                        }
                    }
                }
            }
        }
    }
    out
}

/// Offset every loop of `sketch` by `d`: positive grows the material (outer
/// loops outward, holes inward), negative shrinks it. See the module doc for
/// the construction and its contract. `feature` names the sketch for the
/// result's loop names (`<loop>/offset`).
pub fn offset(sketch: &Sketch, feature: &str, d: f64, join: Join, tolerance: f64) -> crate::Result<Outcome> {
    if !d.is_finite() {
        return Err(crate::CadpoleError::invalid("offset: distance must be finite"));
    }
    let loops = oriented_loops(sketch)?;
    let base = |s: Src| loop_base_name(feature, sketch, s.loop_idx as usize);
    if d == 0.0 || loops.is_empty() {
        let mut out = Sketch::new(sketch.plane);
        for (i, l) in loops.iter().enumerate() {
            out.push_named_loop(l.clone(), format!("{}/offset", base(Src { operand: 0, loop_idx: i as u32, seg_idx: 0, bezier: false })));
        }
        return Ok(Outcome { sketch: out, notes: Vec::new() });
    }
    let extent = extent_of(&loops) + 2.0 * d.abs();
    let mut pieces = Vec::new();
    for (li, l) in loops.iter().enumerate() {
        pieces.extend(raw_offset(l, li, d, join, tolerance));
    }
    let arr = Arrangement::build(&pieces, extent)?;
    let rule = |wn: [i32; 2]| wn[0] > 0;
    let cycles = arr.boundary_loops(&rule)?;
    let built = build_loops(&arr, &cycles, tolerance);
    let mut notes = Vec::new();
    let survived: BTreeSet<u32> = built.iter().flat_map(|b| b.sources.iter().map(|s| s.loop_idx)).collect();
    for i in 0..loops.len() {
        if !survived.contains(&(i as u32)) {
            notes.push(format!(
                "offset: loop {} collapsed at distance {} and was dropped",
                base(Src { operand: 0, loop_idx: i as u32, seg_idx: 0, bezier: false }),
                crate::context::fmt_num(d)
            ));
        }
    }
    if built.is_empty() {
        notes.push(format!("offset: every loop collapsed at distance {}; the sketch is empty", crate::context::fmt_num(d)));
    }
    let names = name_loops(&built, &sketch.plane, &base, "/offset");
    Ok(Outcome { sketch: assemble(sketch.plane, built, names), notes })
}

// --------------------------------------------------------------- booleans

/// `other`'s loops in `target`'s frame, or why they cannot be: the planes
/// must coincide (a sketch boolean is a plane operation).
fn into_frame(other: &Sketch, target: &Plane, what: &str) -> crate::Result<Vec<Loop>> {
    let (a, b) = (target, &other.plane);
    let same_normal = a.normal.dot(b.normal) > 1.0 - 1e-9;
    let on_plane = (b.origin - a.origin).dot(a.normal).abs() <= 1e-9;
    if !same_normal || !on_plane {
        return Err(crate::CadpoleError::invalid(format!(
            "{what}: the sketches are on different planes; a 2D boolean needs both on one plane"
        )));
    }
    if a == b {
        return Ok(other.loops.clone());
    }
    let (ax, ay) = (a.x_dir, a.y_dir());
    let map = |p: P2| {
        let w = b.to_world(p.x, p.y) - a.origin;
        P2::new(w.dot(ax), w.dot(ay))
    };
    Ok(other.loops.iter().map(|l| transform_loop(l, &map)).collect())
}

fn transform_loop(l: &Loop, map: &dyn Fn(P2) -> P2) -> Loop {
    Loop {
        segments: l
            .segments
            .iter()
            .map(|s| match s {
                Segment::Line { a, b } => Segment::Line { a: map(*a), b: map(*b) },
                Segment::Arc { a, b, via } => Segment::Arc { a: map(*a), b: map(*b), via: map(*via) },
                Segment::Bezier { a, b, ctrl } => Segment::Bezier { a: map(*a), b: map(*b), ctrl: ctrl.iter().map(|c| map(*c)).collect() },
            })
            .collect(),
    }
}

/// A 2D boolean of two sketches on one plane. Loop names are provenance:
/// `a/loop[0]|b/loop[0]` for a loop built from both, the source's own name
/// for one that came through untouched (bit for bit, splines included).
pub fn boolean(a: &Sketch, a_feature: &str, b: &Sketch, b_feature: &str, op: BoolOp, tolerance: f64) -> crate::Result<Outcome> {
    let what = op.name();
    let b_in_a = Sketch { plane: a.plane, loops: into_frame(b, &a.plane, what)?, loop_names: b.loop_names.clone(), paths: Vec::new(), path_names: Vec::new() };
    let la = oriented_loops(a)?;
    let lb = oriented_loops(&b_in_a)?;
    let base = |s: Src| if s.operand == 0 { loop_base_name(a_feature, a, s.loop_idx as usize) } else { loop_base_name(b_feature, b, s.loop_idx as usize) };
    let rule: Box<dyn Fn([i32; 2]) -> bool> = match op {
        BoolOp::Union => Box::new(|wn: [i32; 2]| wn[0] > 0 || wn[1] > 0),
        BoolOp::Subtract => Box::new(|wn: [i32; 2]| wn[0] > 0 && wn[1] <= 0),
        BoolOp::Intersect => Box::new(|wn: [i32; 2]| wn[0] > 0 && wn[1] > 0),
    };
    let outcome = clip(&[la, lb], a.plane, &*rule, &base, "", tolerance)?;
    let mut notes = outcome.notes;
    if outcome.sketch.loops.is_empty() {
        notes.push(match op {
            BoolOp::Intersect => format!("intersect: {a_feature} and {b_feature} do not overlap; the sketch is empty"),
            BoolOp::Subtract => format!("subtract: {b_feature} covers all of {a_feature}; the sketch is empty"),
            BoolOp::Union => format!("union: {a_feature} and {b_feature} are both empty"),
        });
    }
    Ok(Outcome { sketch: outcome.sketch, notes })
}

/// The union of every loop of several sketches on one plane, as one region:
/// what `merge=True` means for a mirror image or a pattern's copies.
fn union_all(sketches: &[Sketch], plane: Plane, base: &dyn Fn(usize, usize) -> String, tolerance: f64) -> crate::Result<Outcome> {
    // One operand holding every loop: covered at least once is material.
    let mut loops = Vec::new();
    let mut index: Vec<(usize, usize)> = Vec::new();
    for (si, s) in sketches.iter().enumerate() {
        for (li, l) in oriented_loops(s)?.into_iter().enumerate() {
            loops.push(l);
            index.push((si, li));
        }
    }
    let name = |s: Src| {
        let (si, li) = index[s.loop_idx as usize];
        base(si, li)
    };
    clip(&[loops], plane, &|wn: [i32; 2]| wn[0] > 0, &name, "", tolerance)
}

/// The clipper proper: operands in, the accepted region's loops out. A source
/// loop nothing touched comes back as it was drawn.
fn clip(operands: &[Vec<Loop>], plane: Plane, rule: &dyn Fn([i32; 2]) -> bool, base: &dyn Fn(Src) -> String, suffix: &str, tolerance: f64) -> crate::Result<Outcome> {
    let all: Vec<Loop> = operands.iter().flatten().cloned().collect();
    let extent = extent_of(&all);
    let mut pieces = Vec::new();
    for (oi, loops) in operands.iter().enumerate() {
        pieces.extend(operand_pieces(loops, oi as u8, tolerance));
    }
    let arr = Arrangement::build(&pieces, extent)?;
    let cycles = arr.boundary_loops(rule)?;
    let mut built = build_loops(&arr, &cycles, tolerance);
    // Untouched loops: every edge of the loop is a boundary in the loop's own
    // direction, nothing else shares its edges, and no edge was cut. Then the
    // original segments replace the rebuilt ones, splines and all.
    let mut touched: BTreeMap<(u8, u32), bool> = BTreeMap::new();
    for e in &arr.edges {
        let mut keys: BTreeSet<(u8, u32)> = BTreeSet::new();
        for s in &e.srcs {
            keys.insert((s.operand, s.loop_idx));
        }
        let clean = keys.len() == 1 && e.srcs.len() == 1;
        for k in keys {
            let t = touched.entry(k).or_insert(false);
            *t |= !clean;
        }
    }
    for b in built.iter_mut() {
        let keys: BTreeSet<(u8, u32)> = b.sources.iter().map(|s| (s.operand, s.loop_idx)).collect();
        if keys.len() != 1 {
            continue;
        }
        let (oi, li) = *keys.iter().next().unwrap();
        if touched.get(&(oi, li)).copied().unwrap_or(true) {
            continue;
        }
        let original = &operands[oi as usize][li as usize];
        let count = original.segments.iter().map(|s| prims_of(s, tolerance).len()).sum::<usize>();
        let used: usize = arr.edges.iter().filter(|e| e.srcs.iter().any(|s| (s.operand, s.loop_idx) == (oi, li))).count();
        if used == count {
            // Same direction as drawn? The rebuilt loop's area sign says.
            let rebuilt = Loop { segments: b.segments.clone() };
            if rebuilt.signed_area().signum() == original.signed_area().signum() {
                b.segments = original.segments.clone();
            } else {
                b.segments = original.reversed().segments;
            }
        }
    }
    let names = name_loops(&built, &plane, base, suffix);
    Ok(Outcome { sketch: assemble(plane, built, names), notes: Vec::new() })
}

// --------------------------------------------------------------- corners

/// A corner of a sketch loop: a vertex where the tangent turns.
#[derive(Clone, Debug, PartialEq)]
pub struct Corner {
    pub loop_idx: usize,
    /// The vertex is the start of segment `vertex` (and the end of the one before).
    pub vertex: usize,
    pub name: String,
    pub at: P2,
    /// Interior angle in degrees (less than 180 at a convex corner of an
    /// outer loop).
    pub angle_deg: f64,
}

/// Every corner of every loop, named `<loop>/corner[k]`. Tangent-continuous
/// joints (a rounded rectangle's, a circle's) are not corners.
pub fn corners(sketch: &Sketch, feature: &str) -> Vec<Corner> {
    let mut out = Vec::new();
    for (li, l) in sketch.loops.iter().enumerate() {
        let n = l.segments.len();
        let ccw = l.signed_area() >= 0.0;
        let name = loop_base_name(feature, sketch, li);
        for k in 0..n {
            let prev = &l.segments[(k + n - 1) % n];
            let next = &l.segments[k];
            let (Some(t_in), Some(t_out)) = (prev.end_direction(), next.start_direction()) else { continue };
            let turn = cross(t_in, t_out);
            if turn.abs() <= 1e-9 && dot(t_in, t_out) > 0.0 {
                continue;
            }
            // The interior angle, with the material on the loop's inside.
            let ext = fmath::atan2(turn, dot(t_in, t_out));
            let signed = if ccw { ext } else { -ext };
            let interior = PI - signed;
            out.push(Corner { loop_idx: li, vertex: k, name: format!("{name}/corner[{k}]"), at: next.start(), angle_deg: interior.to_degrees() });
        }
    }
    out
}

/// Which corners a call means.
#[derive(Clone, Debug)]
pub enum CornerSelect {
    All,
    Some(Vec<(usize, usize)>),
}

fn carrier(seg: &Segment) -> Option<Prim> {
    match seg {
        Segment::Line { a, b } => Some(Prim::Line { a: *a, b: *b }),
        Segment::Arc { a, b, via } => arc_params(*a, *b, *via).map(|(c, r, t0, t1)| Prim::Arc { c, r, t0, sweep: t1 - t0 }),
        Segment::Bezier { .. } => None,
    }
}

/// Every point where the infinite carriers of two primitives meet (the
/// whole line, the whole circle), in closed form.
fn carrier_points(p: &Prim, q: &Prim) -> Vec<P2> {
    let mut out = Vec::new();
    match (*p, *q) {
        (Prim::Line { a, b }, Prim::Line { a: c, b: d }) => {
            let (r, s) = (sub(b, a), sub(d, c));
            let den = cross(r, s);
            if den.abs() > 1e-12 * norm(r) * norm(s) {
                let u = cross(sub(c, a), s) / den;
                out.push(add(a, mul(r, u)));
            }
        }
        (Prim::Line { a, b }, Prim::Arc { c, r, .. }) | (Prim::Arc { c, r, .. }, Prim::Line { a, b }) => {
            if let Some(t) = unit(sub(b, a)) {
                let h = cross(t, sub(c, a));
                let foot = add(a, mul(t, dot(sub(c, a), t)));
                let gap = h.abs() - r;
                if gap.abs() <= 1e-12 {
                    out.push(foot);
                } else if gap < 0.0 {
                    let w = (r * r - h * h).sqrt();
                    out.push(add(foot, mul(t, w)));
                    out.push(add(foot, mul(t, -w)));
                }
            }
        }
        (Prim::Arc { c: c1, r: r1, .. }, Prim::Arc { c: c2, r: r2, .. }) => {
            let d = dist(c1, c2);
            if d > 1e-12 && d <= r1 + r2 + 1e-12 && d >= (r1 - r2).abs() - 1e-12 {
                let a = (r1 * r1 - r2 * r2 + d * d) / (2.0 * d);
                let h = (r1 * r1 - a * a).max(0.0).sqrt();
                let ex = mul(sub(c2, c1), 1.0 / d);
                let m = add(c1, mul(ex, a));
                out.push(add(m, mul(left(ex), h)));
                if h > 0.0 {
                    out.push(add(m, mul(right(ex), h)));
                }
            }
        }
    }
    out
}

/// Where a fillet of radius `r` touches the two carriers meeting at the
/// corner `v`: the trim parameters on each and the arc, or `None` when it
/// does not fit.
fn fillet_at(a: &Prim, b: &Prim, v: P2, r: f64) -> Option<(f64, f64, Segment)> {
    let t_in = a.tangent(1.0)?;
    let t_out = b.tangent(0.0)?;
    let turn = cross(t_in, t_out);
    if turn.abs() <= 1e-12 {
        return None;
    }
    // The centre sits on the inside of the turn, `r` from both carriers.
    let side = if turn > 0.0 { -r } else { r };
    let shift = |p: &Prim| -> Option<Prim> {
        Some(match *p {
            Prim::Line { a, b } => {
                let nr = mul(right(unit(sub(b, a))?), side);
                Prim::Line { a: add(a, nr), b: add(b, nr) }
            }
            Prim::Arc { c, r: ra, t0, sweep } => {
                let r2 = if sweep > 0.0 { ra + side } else { ra - side };
                if r2 <= 1e-12 {
                    return None;
                }
                Prim::Arc { c, r: r2, t0, sweep }
            }
        })
    };
    let (sa, sb) = (shift(a)?, shift(b)?);
    let cands = carrier_points(&sa, &sb);
    // The foot of the centre on each original carrier.
    let foot = |p: &Prim, c: P2| -> (P2, f64) {
        match *p {
            Prim::Line { .. } => {
                let u = p.arc_param(c);
                (p.point(u), u)
            }
            Prim::Arc { c: cc, r: rr, .. } => {
                let dir = unit(sub(c, cc)).unwrap_or(P2::new(1.0, 0.0));
                let q = add(cc, mul(dir, rr));
                (q, p.arc_param(q))
            }
        }
    };
    let mut best: Option<(f64, f64, f64, P2, P2, P2)> = None;
    for c in cands {
        let (fa, ua) = foot(a, c);
        let (fb, ub) = foot(b, c);
        // The tangent points may reach the far ends of their segments (the
        // segment then vanishes, as with a chamfer of half a side) but not
        // the corner itself.
        let ok = ua >= -1e-9 && ua < 1.0 - 1e-12 && ub > 1e-12 && ub <= 1.0 + 1e-9;
        if !ok {
            continue;
        }
        let dv = dist(c, v);
        if best.map_or(true, |b| dv < b.0) {
            best = Some((dv, ua.max(0.0), ub.min(1.0), c, fa, fb));
        }
    }
    let (_, ua, ub, c, fa, fb) = best?;
    let via = add(c, mul(unit(sub(v, c))?, r));
    Some((ua, ub, Segment::Arc { a: fa, b: fb, via }))
}

fn chamfer_at(a: &Prim, b: &Prim, d: f64) -> Option<(f64, f64, Segment)> {
    let (la, lb) = (a.length(), b.length());
    if d > la + 1e-12 || d > lb + 1e-12 || d <= 0.0 {
        return None;
    }
    let ua = 1.0 - d / la;
    let ub = d / lb;
    Some((ua, ub, Segment::Line { a: a.point(ua), b: b.point(ub) }))
}

/// The largest `x` in `(0, hi]` for which `fits(x)`, by bisection (60 steps).
fn largest_fitting(hi: f64, fits: &dyn Fn(f64) -> bool) -> f64 {
    let (mut lo, mut hi) = (0.0, hi);
    for _ in 0..60 {
        let mid = (lo + hi) / 2.0;
        if fits(mid) { lo = mid } else { hi = mid }
    }
    lo
}

fn corner_op(sketch: &Sketch, feature: &str, size: f64, which: &CornerSelect, what: &str, build: &dyn Fn(&Prim, &Prim, P2, f64) -> Option<(f64, f64, Segment)>) -> crate::Result<Sketch> {
    if !(size > 0.0) || !size.is_finite() {
        return Err(crate::CadpoleError::invalid(format!("{what}: the size must be positive (got {})", crate::context::fmt_num(size))));
    }
    let all = corners(sketch, feature);
    let selected: Vec<&Corner> = match which {
        CornerSelect::All => all.iter().collect(),
        CornerSelect::Some(list) => {
            let mut v = Vec::new();
            for (li, k) in list {
                match all.iter().find(|c| c.loop_idx == *li && c.vertex == *k) {
                    Some(c) => v.push(c),
                    None => {
                        return Err(crate::CadpoleError::invalid(format!(
                            "{what}: loop {li} has no corner at vertex {k}; its corners are {}",
                            all.iter().filter(|c| c.loop_idx == *li).map(|c| c.name.as_str()).collect::<Vec<_>>().join(", ")
                        )));
                    }
                }
            }
            v
        }
    };
    if selected.is_empty() {
        return Err(crate::CadpoleError::invalid(format!("{what}: the sketch has no corners to work on")));
    }
    // Per corner: the two trims and the inserted segment.
    let mut plan: BTreeMap<(usize, usize), (f64, f64, Segment)> = BTreeMap::new();
    for c in &selected {
        let l = &sketch.loops[c.loop_idx];
        let n = l.segments.len();
        let (sa, sb) = (&l.segments[(c.vertex + n - 1) % n], &l.segments[c.vertex]);
        let (Some(pa), Some(pb)) = (carrier(sa), carrier(sb)) else {
            return Err(crate::CadpoleError::invalid(format!(
                "{what}: {} joins a spline; only corners between lines and arcs can be {what}ed",
                c.name
            )));
        };
        match build(&pa, &pb, c.at, size) {
            Some(p) => {
                plan.insert((c.loop_idx, c.vertex), p);
            }
            None => {
                let max = largest_fitting(size, &|x| build(&pa, &pb, c.at, x).is_some());
                return Err(crate::CadpoleError::invalid(format!(
                    "{what}: {} does not fit at {} (segments {} and {} long); the largest that fits is {}",
                    crate::context::fmt_num(size),
                    c.name,
                    crate::context::fmt_num(pa.length()),
                    crate::context::fmt_num(pb.length()),
                    crate::context::fmt_num(max)
                )));
            }
        }
    }
    // Two corners on one segment must leave room for each other.
    for c in &selected {
        let l = &sketch.loops[c.loop_idx];
        let n = l.segments.len();
        let next = (c.vertex + 1) % n;
        let (Some(here), Some(after)) = (plan.get(&(c.loop_idx, c.vertex)), plan.get(&(c.loop_idx, next))) else { continue };
        // `here` trims the start of segment `vertex` to `ub`; `after` trims its end to `ua`.
        if here.1 > after.0 + 1e-12 {
            let (sa, sb, sc) = (&l.segments[(c.vertex + n - 1) % n], &l.segments[c.vertex], &l.segments[(c.vertex + 1) % n]);
            let (pa, pb, pc) = (carrier(sa).unwrap(), carrier(sb).unwrap(), carrier(sc).unwrap());
            let other = all.iter().find(|o| o.loop_idx == c.loop_idx && o.vertex == next).unwrap();
            let max = largest_fitting(size, &|x| match (build(&pa, &pb, c.at, x), build(&pb, &pc, other.at, x)) {
                (Some(h), Some(a)) => h.1 <= a.0 + 1e-12,
                _ => false,
            });
            return Err(crate::CadpoleError::invalid(format!(
                "{what}: {} does not fit: {} and {} overlap on the segment between them ({} long); the largest that fits both is {}",
                crate::context::fmt_num(size),
                c.name,
                other.name,
                crate::context::fmt_num(pb.length()),
                crate::context::fmt_num(max)
            )));
        }
    }
    let mut out = Sketch::new(sketch.plane);
    for (li, l) in sketch.loops.iter().enumerate() {
        let n = l.segments.len();
        let mut segs = Vec::new();
        for k in 0..n {
            if let Some((_, _, ins)) = plan.get(&(li, k)) {
                segs.push(ins.clone());
            }
            let seg = &l.segments[k];
            let len = seg.length();
            let s0 = plan.get(&(li, k)).map_or(0.0, |p| p.1 * len);
            let s1 = plan.get(&(li, (k + 1) % n)).map_or(len, |p| p.0 * len);
            match seg.sub(s0, s1) {
                Some(s) => segs.push(s),
                None => {}
            }
        }
        out.push_named_loop(Loop { segments: segs }, loop_base_name(feature, sketch, li));
    }
    for (p, n) in sketch.paths.iter().zip(&sketch.path_names) {
        out.push_path(p.clone(), n.clone());
    }
    Ok(out)
}

/// Round the selected corners with arcs of radius `r`, each tangent to both
/// segments it joins. Refuses a radius that does not fit, naming the largest
/// that would.
pub fn fillet2d(sketch: &Sketch, feature: &str, r: f64, which: &CornerSelect) -> crate::Result<Sketch> {
    corner_op(sketch, feature, r, which, "fillet2d", &fillet_at)
}

/// Cut the selected corners back by `d` along both segments, joined by a line.
pub fn chamfer2d(sketch: &Sketch, feature: &str, d: f64, which: &CornerSelect) -> crate::Result<Sketch> {
    corner_op(sketch, feature, d, which, "chamfer2d", &|a, b, _v, d| chamfer_at(a, b, d))
}

// ------------------------------------------------------ mirror and pattern

/// A rigid or reflecting map of the sketch plane.
#[derive(Clone, Copy, Debug)]
pub struct Xf2 {
    /// Row-major 2×2 linear part.
    pub m: [f64; 4],
    pub t: P2,
}

impl Xf2 {
    pub fn apply(&self, p: P2) -> P2 {
        P2::new(self.m[0] * p.x + self.m[1] * p.y + self.t.x, self.m[2] * p.x + self.m[3] * p.y + self.t.y)
    }
    /// Reflection across the line through `origin` along `axis`.
    pub fn reflection(origin: P2, axis: P2) -> Option<Xf2> {
        let u = unit(axis)?;
        let m = [u.x * u.x - u.y * u.y, 2.0 * u.x * u.y, 2.0 * u.x * u.y, u.y * u.y - u.x * u.x];
        let lin = P2::new(m[0] * origin.x + m[1] * origin.y, m[2] * origin.x + m[3] * origin.y);
        Some(Xf2 { m, t: sub(origin, lin) })
    }
    pub fn translation(v: P2) -> Xf2 {
        Xf2 { m: [1.0, 0.0, 0.0, 1.0], t: v }
    }
    /// Rotation by `angle` radians about `center`.
    pub fn rotation(center: P2, angle: f64) -> Xf2 {
        let (s, c) = fmath::sin_cos(angle);
        let m = [c, -s, s, c];
        let lin = P2::new(m[0] * center.x + m[1] * center.y, m[2] * center.x + m[3] * center.y);
        Xf2 { m, t: sub(center, lin) }
    }
}

fn mapped_sketch(sketch: &Sketch, feature: &str, xf: &Xf2, suffix: &str) -> Sketch {
    let mut out = Sketch::new(sketch.plane);
    for (i, l) in sketch.loops.iter().enumerate() {
        out.push_named_loop(transform_loop(l, &|p| xf.apply(p)), format!("{}{suffix}", loop_base_name(feature, sketch, i)));
    }
    out
}

fn with_own_names(sketch: &Sketch, feature: &str) -> Sketch {
    let mut out = Sketch::new(sketch.plane);
    for (i, l) in sketch.loops.iter().enumerate() {
        out.push_named_loop(l.clone(), loop_base_name(feature, sketch, i));
    }
    out
}

fn concat(sketches: &[Sketch], plane: Plane) -> Sketch {
    let mut out = Sketch::new(plane);
    for s in sketches {
        for (i, l) in s.loops.iter().enumerate() {
            out.push_named_loop(l.clone(), s.loop_name(i).to_owned());
        }
    }
    out
}

/// Mirror every loop across the line through `origin` along `axis`. With
/// `keep` the originals stay; with `merge` the image is united with them
/// (a half that ends on the axis becomes one loop). Image loops are
/// `<loop>/mirror`; a merged loop is named by provenance.
pub fn mirror2d(sketch: &Sketch, feature: &str, origin: P2, axis: P2, keep: bool, merge: bool, tolerance: f64) -> crate::Result<Outcome> {
    let xf = Xf2::reflection(origin, axis).ok_or_else(|| crate::CadpoleError::invalid("mirror2d: axis must be non-zero"))?;
    let image = mapped_sketch(sketch, feature, &xf, "/mirror");
    if !keep {
        return Ok(Outcome { sketch: image, notes: Vec::new() });
    }
    let original = with_own_names(sketch, feature);
    if !merge {
        return Ok(Outcome { sketch: concat(&[original, image], sketch.plane), notes: Vec::new() });
    }
    let parts = [original, image];
    let base = |si: usize, li: usize| parts[si].loop_name(li).to_owned();
    union_all(&parts, sketch.plane, &base, tolerance)
}

/// `count` copies of every loop under `transforms[k]` (the identity first).
/// Copies are `<loop>/copy[k]`; with `merge` the copies are united.
pub fn pattern2d(sketch: &Sketch, feature: &str, transforms: &[Xf2], merge: bool, tolerance: f64) -> crate::Result<Outcome> {
    let mut parts: Vec<Sketch> = Vec::with_capacity(transforms.len());
    for (k, xf) in transforms.iter().enumerate() {
        parts.push(if k == 0 { with_own_names(sketch, feature) } else { mapped_sketch(sketch, feature, xf, &format!("/copy[{k}]")) });
    }
    if !merge {
        return Ok(Outcome { sketch: concat(&parts, sketch.plane), notes: Vec::new() });
    }
    let base = |si: usize, li: usize| parts[si].loop_name(li).to_owned();
    union_all(&parts, sketch.plane, &base, tolerance)
}

#[cfg(test)]
mod tests {
    use super::*;

    fn xy(l: Loop) -> Sketch {
        Sketch::new(Plane::XY).with_loop(l)
    }
    /// Material area of a sketch, exact for lines and arcs.
    fn area(s: &Sketch) -> f64 {
        s.regions().unwrap().iter().map(|r| exact_area(&r.outer, 1e-4) + r.holes.iter().map(|h| exact_area(h, 1e-4)).sum::<f64>()).sum()
    }
    fn arcs(l: &Loop) -> usize {
        l.segments.iter().filter(|s| matches!(s, Segment::Arc { .. })).count()
    }
    fn lines(l: &Loop) -> usize {
        l.segments.iter().filter(|s| matches!(s, Segment::Line { .. })).count()
    }
    /// Distance from `p` to the nearest point of the loop's boundary, by
    /// sampling finely: the check every offset test rests on.
    fn dist_to(l: &Loop, p: P2) -> f64 {
        let pts = l.sample_tolerance(1e-4);
        let n = pts.len();
        let mut best = f64::INFINITY;
        for i in 0..n {
            let (a, b) = (pts[i], pts[(i + 1) % n]);
            let d = sub(b, a);
            let l2 = dot(d, d);
            let u = if l2 > 0.0 { (dot(sub(p, a), d) / l2).clamp(0.0, 1.0) } else { 0.0 };
            best = best.min(dist(p, P2::new(a.x + d.x * u, a.y + d.y * u)));
        }
        best
    }

    /// A NACA 4-digit section as a polygon: `n` points per surface, a blunt
    /// trailing edge two points wide, counter-clockwise (lower surface from
    /// the nose, then the upper surface back to it). The shape the wing
    /// demo's sections are, and the shape whose inward offset pinches.
    fn naca(code: (f64, f64, f64), chord: f64, n: usize) -> Loop {
        let (m, p, t) = (code.0 / 100.0, code.1 / 10.0, code.2 / 100.0);
        let mut upper = Vec::new();
        let mut lower = Vec::new();
        for i in 0..=n {
            // Cosine spacing packs points at the nose.
            let x = (1.0 - fmath::cos(PI * i as f64 / n as f64)) / 2.0;
            let yt = 5.0 * t * (0.2969 * x.sqrt() - 0.1260 * x - 0.3516 * x * x + 0.2843 * x * x * x - 0.1015 * x * x * x * x);
            let (yc, dyc) = if x < p {
                (m / (p * p) * (2.0 * p * x - x * x), 2.0 * m / (p * p) * (p - x))
            } else {
                (m / ((1.0 - p) * (1.0 - p)) * ((1.0 - 2.0 * p) + 2.0 * p * x - x * x), 2.0 * m / ((1.0 - p) * (1.0 - p)) * (p - x))
            };
            let th = fmath::atan(dyc);
            let (s, c) = fmath::sin_cos(th);
            upper.push(P2::new((x - yt * s) * chord, (yc + yt * c) * chord));
            lower.push(P2::new((x + yt * s) * chord, (yc - yt * c) * chord));
        }
        // Blunt trailing edge: the polynomial leaves 0.0021·t·chord of gap at x = 1.
        let mut pts: Vec<P2> = lower[1..].to_vec();
        pts.extend(upper[1..].iter().rev().copied());
        pts.push(upper[0]);
        pts.dedup_by(|a, b| dist(*a, *b) < 1e-9);
        Loop::polygon(&pts)
    }

    /// A spline outline offset by more than its curves' radius: every
    /// sample's offset crosses many others, so the raw offset holds
    /// thousands of crossings. The arrangement used to test every piece
    /// against every vertex and every face against every edge — quartic in
    /// the samples, half a minute for one glyph — and must stay near-linear
    /// in the crossings. The bound is generous for a debug build.
    #[test]
    #[ignore = "wall-clock timing; flaky on a loaded host. Run with --ignored on a quiet machine"]
    fn a_large_offset_of_a_wavy_spline_is_fast() {
        let pts: Vec<P2> = (0..96)
            .map(|i| {
                let t = TAU * i as f64 / 96.0;
                let r = 10.0 + 2.5 * fmath::sin(8.0 * t);
                P2::new(r * fmath::cos(t), r * fmath::sin(t))
            })
            .collect();
        let s = xy(Loop::spline(&pts, 0.5).unwrap());
        let t0 = std::time::Instant::now();
        for d in [8.0, 4.0, -3.0] {
            let out = offset(&s, "s", d, Join::Round, 0.001).unwrap();
            if d > 0.0 {
                // Outward: one loop round the whole outline, clear of it.
                assert_eq!(out.sketch.loops.len(), 1, "offset {d}");
                let l = &out.sketch.loops[0];
                assert!(s.loops[0].sample().iter().all(|p| l.contains_point(*p)), "offset {d}");
                assert!(area(&out.sketch) > PI * (12.5 + d) * (12.5 + d) * 0.8, "offset {d}: {}", area(&out.sketch));
            } else {
                // Inward by more than the lobes' radius: the core survives.
                assert!(!out.sketch.loops.is_empty(), "offset {d}");
            }
        }
        let took = t0.elapsed().as_secs_f64();
        assert!(took < 10.0, "two offsets took {took:.1} s");
    }

    /// Two outlines whose edges miss lining up by a micron leave a step a
    /// micron long in their union: longer than the arrangement's weld grid,
    /// so it used to survive as a segment, and a wall built on it (emboss)
    /// had two corners at one point. The result has no segment below a
    /// hundredth of the tolerance, and the area is unchanged to that order.
    #[test]
    fn a_union_drops_a_hairline_step() {
        let a = xy(Loop::polygon(&[P2::new(0.0, 0.0), P2::new(10.0, 0.0), P2::new(10.0, 10.0), P2::new(0.0, 10.0)]));
        let b = xy(Loop::polygon(&[P2::new(5.0, 5.0), P2::new(10.0 + 1e-6, 5.0), P2::new(10.0 + 1e-6, 15.0), P2::new(5.0, 15.0)]));
        let tol = 0.05;
        let u = boolean(&a, "a", &b, "b", BoolOp::Union, tol).unwrap().sketch;
        assert_eq!(u.loops.len(), 1);
        let shortest = u.loops[0].segments.iter().map(Segment::length).fold(f64::INFINITY, f64::min);
        assert!(shortest >= tol * 1e-2, "shortest segment {shortest:e}: {:?}", u.loops[0].segments);
        assert!(u.loops[0].is_closed(1e-12));
        assert!((area(&u) - 125.0).abs() < 1e-4, "{}", area(&u));
    }

    /// The chain is rejoined where the short segment was, closed exactly,
    /// whichever position (first, middle, last) it held.
    #[test]
    fn drop_hairlines_keeps_the_chain_closed() {
        let p = [P2::new(0.0, 0.0), P2::new(10.0, 0.0), P2::new(10.0, 1e-7), P2::new(10.0, 10.0), P2::new(0.0, 10.0)];
        for rot in 0..p.len() {
            let pts: Vec<P2> = (0..p.len()).map(|i| p[(i + rot) % p.len()]).collect();
            let mut segs = Loop::polygon(&pts).segments;
            drop_hairlines(&mut segs, 1e-4);
            assert_eq!(segs.len(), 4, "rotation {rot}");
            let l = Loop { segments: segs };
            assert!(l.is_closed(1e-12), "rotation {rot}: {:?}", l.segments);
            for w in l.segments.windows(2) {
                assert_eq!(w[0].end(), w[1].start(), "rotation {rot}");
            }
        }
    }

    /// A round offset of a rectangle is four lines and four exact quarter
    /// circles centred on the original corners; a mitred one is a bigger
    /// rectangle with sharp corners; a bevelled one has eight lines.
    #[test]
    fn rect_offsets_have_exact_corner_geometry() {
        let s = xy(Loop::rect(P2::new(0.0, 0.0), 20.0, 10.0));
        let r = offset(&s, "s", 2.0, Join::Round, 0.05).unwrap();
        assert!(r.notes.is_empty(), "{:?}", r.notes);
        assert_eq!(r.sketch.loops.len(), 1);
        let l = &r.sketch.loops[0];
        assert_eq!((lines(l), arcs(l)), (4, 4), "{:?}", l.segments);
        for seg in &l.segments {
            if let Segment::Arc { a, b, via } = seg {
                let (c, rad, t0, t1) = arc_params(*a, *b, *via).unwrap();
                assert!((rad - 2.0).abs() < 1e-9, "radius {rad}");
                assert!((c.x.abs() - 10.0).abs() < 1e-9 && (c.y.abs() - 5.0).abs() < 1e-9, "centre {c:?}");
                assert!(((t1 - t0).abs() - PI / 2.0).abs() < 1e-9, "sweep {}", t1 - t0);
            }
        }
        let expect = 24.0 * 14.0 - (4.0 - PI) * 4.0;
        assert!((area(&r.sketch) - expect).abs() < 1e-3, "{} vs {expect}", area(&r.sketch));
        assert_eq!(r.sketch.loop_names, vec!["s/loop[0]/offset"]);

        let m = offset(&s, "s", 2.0, Join::Miter, 0.05).unwrap();
        let l = &m.sketch.loops[0];
        assert_eq!((lines(l), arcs(l)), (4, 0), "{:?}", l.segments);
        let pts = l.sample();
        for p in &pts {
            assert!((p.x.abs() - 12.0).abs() < 1e-9 && (p.y.abs() - 7.0).abs() < 1e-9, "{p:?}");
        }
        assert!((area(&m.sketch) - 24.0 * 14.0).abs() < 1e-9);

        let b = offset(&s, "s", 2.0, Join::Bevel, 0.05).unwrap();
        assert_eq!((lines(&b.sketch.loops[0]), arcs(&b.sketch.loops[0])), (8, 0));
        assert!((area(&b.sketch) - (24.0 * 14.0 - 2.0 * 4.0)).abs() < 1e-9);

        // Inward: the overlaps at the convex corners are trimmed away, and
        // what is left is the smaller rectangle with sharp corners, for
        // every join style (a join only applies where a gap opens).
        for j in Join::ALL {
            let i = offset(&s, "s", -2.0, j, 0.05).unwrap();
            let l = &i.sketch.loops[0];
            assert_eq!((lines(l), arcs(l)), (4, 0), "{j:?}: {:?}", l.segments);
            for p in l.sample() {
                assert!((p.x.abs() - 8.0).abs() < 1e-9 && (p.y.abs() - 3.0).abs() < 1e-9, "{j:?}: {p:?}");
            }
            assert!(l.signed_area() > 0.0, "the result keeps the material on its left");
        }
    }

    /// An offset circle is a circle of the new radius: two arcs on one
    /// centre, so an extrude of it is a cylinder. A collapse is empty with a note.
    #[test]
    fn circle_offsets_are_circles_and_collapse_with_a_note() {
        let s = xy(Loop::circle(P2::new(3.0, -1.0), 5.0));
        for (d, want) in [(2.0, 7.0), (-2.0, 3.0)] {
            let o = offset(&s, "s", d, Join::Round, 0.05).unwrap();
            let l = &o.sketch.loops[0];
            assert_eq!((lines(l), arcs(l)), (0, 2), "{:?}", l.segments);
            for seg in &l.segments {
                let Segment::Arc { a, b, via } = seg else { unreachable!() };
                let (c, r, _, _) = arc_params(*a, *b, *via).unwrap();
                assert!((r - want).abs() < 1e-9 && dist(c, P2::new(3.0, -1.0)) < 1e-9, "d={d}: {c:?} r={r}");
            }
        }
        let gone = offset(&s, "s", -5.0, Join::Round, 0.05).unwrap();
        assert!(gone.sketch.loops.is_empty());
        assert!(gone.notes.iter().any(|n| n.contains("s/loop[0] collapsed")), "{:?}", gone.notes);
        let gone = offset(&s, "s", -7.5, Join::Round, 0.05).unwrap();
        assert!(gone.sketch.loops.is_empty(), "past the radius is still empty, not a phantom: {:?}", gone.sketch.loops);
    }

    /// A hole shrinks when the material grows and grows when it shrinks.
    #[test]
    fn a_holes_offset_goes_the_right_way() {
        let s = xy(Loop::rect(P2::new(0.0, 0.0), 20.0, 20.0)).with_loop(Loop::circle(P2::new(0.0, 0.0), 3.0));
        let grow = offset(&s, "s", 1.0, Join::Round, 0.05).unwrap();
        assert_eq!(grow.sketch.loops.len(), 2, "{:?}", grow.sketch.loop_names);
        let regions = grow.sketch.regions().unwrap();
        assert_eq!(regions.len(), 1);
        assert_eq!(regions[0].holes.len(), 1);
        let Segment::Arc { a, b, via } = &regions[0].holes[0].segments[0] else { panic!() };
        assert!((arc_params(*a, *b, *via).unwrap().1 - 2.0).abs() < 1e-9);
        assert!((exact_area(&regions[0].outer, 1e-4) - (22.0 * 22.0 - (4.0 - PI))).abs() < 1e-9);
        let shrink = offset(&s, "s", -1.0, Join::Round, 0.05).unwrap();
        let regions = shrink.sketch.regions().unwrap();
        let Segment::Arc { a, b, via } = &regions[0].holes[0].segments[0] else { panic!() };
        assert!((arc_params(*a, *b, *via).unwrap().1 - 4.0).abs() < 1e-9);
        assert!((regions[0].outer.signed_area() - 18.0 * 18.0).abs() < 1e-9);
        // A hole grown into the wall takes the wall with it: the ring vanishes.
        let ring = xy(Loop::circle(P2::new(0.0, 0.0), 5.0)).with_loop(Loop::circle(P2::new(0.0, 0.0), 3.0));
        let gone = offset(&ring, "s", -1.0, Join::Round, 0.05).unwrap();
        assert!(gone.sketch.loops.is_empty(), "{:?}", gone.sketch.loop_names);
        // And a hole shrunk to nothing leaves a plain disc, no phantom.
        let filled = offset(&ring, "s", 3.5, Join::Round, 0.05).unwrap();
        assert_eq!(filled.sketch.loops.len(), 1);
        assert!((exact_area(&filled.sketch.loops[0], 1e-4) - PI * 8.5 * 8.5).abs() < 1e-9);
    }

    /// The wing's use: an inward offset of a thin airfoil pinches off at the
    /// trailing edge. Every result vertex is at least `|d|` from the original
    /// boundary, the result is one loop shorter than the chord, and the
    /// spike-trimmed self-intersection leaves no sliver behind.
    #[test]
    fn an_airfoils_inward_offset_pinches_cleanly_at_the_trailing_edge() {
        let foil = naca((2.0, 4.0, 12.0), 100.0, 60);
        assert!(foil.signed_area() > 0.0);
        let s = xy(foil.clone());
        let inner = offset(&s, "wing", -0.8, Join::Miter, 0.05).unwrap();
        assert!(inner.notes.is_empty(), "{:?}", inner.notes);
        assert_eq!(inner.sketch.loops.len(), 1, "{:?}", inner.sketch.loop_names);
        let l = &inner.sketch.loops[0];
        assert!(l.is_closed(1e-9));
        assert!(l.signed_area() > 0.0);
        let pts = l.sample();
        let max_x = pts.iter().map(|p| p.x).fold(f64::NEG_INFINITY, f64::max);
        assert!(max_x < 100.0 - 0.8 - 3.0, "the pinch removes the thin tail, ends at x={max_x}");
        for p in &pts {
            let d = dist_to(&foil, *p);
            assert!(d >= 0.8 - 1e-3, "vertex {p:?} is only {d} from the original");
            assert!(foil.contains_point(*p), "vertex {p:?} is outside");
        }
        // The original kept for reference: the inner area is well inside.
        assert!(area(&inner.sketch) < foil.signed_area() - 0.8 * foil.length() * 0.8);
        // Outward, nothing pinches and the round joins keep the nose smooth.
        let outer = offset(&s, "wing", 0.8, Join::Round, 0.05).unwrap();
        assert_eq!(outer.sketch.loops.len(), 1);
        for p in outer.sketch.loops[0].sample() {
            assert!(dist_to(&foil, p) >= 0.8 - 1e-3 && !foil.contains_point(p));
        }
    }

    /// An inward offset that cuts a neck splits the loop in two, `#0` and `#1`.
    #[test]
    fn an_offset_that_splits_a_loop_returns_both_pieces() {
        // Two discs joined by a 1.5-wide neck.
        let neck = Loop::polygon(&[P2::new(-5.0, -0.75), P2::new(5.0, -0.75), P2::new(5.0, 0.75), P2::new(-5.0, 0.75)]);
        let a = xy(Loop::circle(P2::new(-8.0, 0.0), 4.0));
        let b = xy(Loop::circle(P2::new(8.0, 0.0), 4.0));
        let dumbbell = boolean(&boolean(&a, "a", &xy(neck), "n", BoolOp::Union, 0.05).unwrap().sketch, "u", &b, "b", BoolOp::Union, 0.05).unwrap().sketch;
        assert_eq!(dumbbell.loops.len(), 1, "{:?}", dumbbell.loop_names);
        let cut = offset(&dumbbell, "d", -1.0, Join::Round, 0.05).unwrap();
        assert_eq!(cut.sketch.loops.len(), 2, "{:?}", cut.sketch.loop_names);
        let mut names = cut.sketch.loop_names.clone();
        names.sort();
        assert!(names[0].ends_with("/offset#0") && names[1].ends_with("/offset#1"), "{names:?}");
        // Each piece is the disc shrunk to radius 3 plus a small lens at the
        // mouth of the neck, where the boundary is the neck's edge rather
        // than the circle and the material reaches past the smaller circle:
        // 9π plus about 0.09. Every vertex keeps the offset distance.
        for l in &cut.sketch.loops {
            let a = exact_area(l, 1e-4);
            assert!(a > PI * 9.0 && a < PI * 9.0 + 0.2, "{a}");
            for p in l.sample_tolerance(0.01) {
                let d = dist_to(&dumbbell.loops[0], p);
                assert!(d >= 1.0 - 1e-3, "{p:?} is {d} from the dumbbell");
            }
        }
    }

    /// Two overlapping rectangles unite into one eight-vertex loop with the
    /// right area; a subtraction makes a hole; an empty intersection is an
    /// empty sketch with a note, never an error.
    #[test]
    fn booleans_on_rectangles() {
        let a = xy(Loop::rect(P2::new(0.0, 0.0), 10.0, 10.0));
        let b = xy(Loop::rect(P2::new(5.0, 5.0), 10.0, 10.0));
        let u = boolean(&a, "a", &b, "b", BoolOp::Union, 0.05).unwrap();
        assert_eq!(u.sketch.loops.len(), 1);
        let l = &u.sketch.loops[0];
        assert_eq!(l.segments.len(), 8, "{:?}", l.segments);
        assert!((l.signed_area() - (100.0 + 100.0 - 25.0)).abs() < 1e-9);
        assert_eq!(u.sketch.loop_names, vec!["a/loop[0]|b/loop[0]"]);

        let i = boolean(&a, "a", &b, "b", BoolOp::Intersect, 0.05).unwrap();
        assert_eq!(i.sketch.loops.len(), 1);
        assert!((i.sketch.loops[0].signed_area() - 25.0).abs() < 1e-9);
        assert_eq!(i.sketch.loops[0].segments.len(), 4);

        let d = boolean(&a, "a", &b, "b", BoolOp::Subtract, 0.05).unwrap();
        assert_eq!(d.sketch.loops.len(), 1);
        assert!((d.sketch.loops[0].signed_area() - 75.0).abs() < 1e-9);
        assert_eq!(d.sketch.loops[0].segments.len(), 6);

        // A hole: subtract a disc that sits inside.
        let disc = xy(Loop::circle(P2::new(0.0, 0.0), 2.0));
        let h = boolean(&a, "a", &disc, "c", BoolOp::Subtract, 0.05).unwrap();
        assert_eq!(h.sketch.loops.len(), 2, "{:?}", h.sketch.loop_names);
        let regions = h.sketch.regions().unwrap();
        assert_eq!((regions.len(), regions[0].holes.len()), (1, 1));
        assert!((exact_area(&regions[0].holes[0], 1e-4) + PI * 4.0).abs() < 1e-9);
        // The untouched square came through bit for bit, and the hole is
        // the disc's own two arcs.
        assert_eq!(regions[0].outer, a.loops[0]);
        assert_eq!(arcs(&regions[0].holes[0]), 2);
        assert!(h.sketch.loop_names.contains(&"a/loop[0]".to_owned()) && h.sketch.loop_names.contains(&"c/loop[0]".to_owned()), "{:?}", h.sketch.loop_names);

        let far = xy(Loop::rect(P2::new(50.0, 0.0), 4.0, 4.0));
        let e = boolean(&a, "a", &far, "f", BoolOp::Intersect, 0.05).unwrap();
        assert!(e.sketch.loops.is_empty());
        assert!(e.notes.iter().any(|n| n.contains("do not overlap")), "{:?}", e.notes);
        let both = boolean(&a, "a", &far, "f", BoolOp::Union, 0.05).unwrap();
        assert_eq!(both.sketch.loops.len(), 2);
        assert_eq!(both.sketch.loop_names, vec!["a/loop[0]", "f/loop[0]"]);
    }

    /// Touching is not overlapping: two squares sharing an edge unite into
    /// one rectangle, and the shared edge is gone.
    #[test]
    fn a_shared_edge_is_absorbed_by_a_union() {
        let a = xy(Loop::rect(P2::new(0.0, 0.0), 10.0, 10.0));
        let b = xy(Loop::rect(P2::new(10.0, 0.0), 10.0, 10.0));
        let u = boolean(&a, "a", &b, "b", BoolOp::Union, 0.05).unwrap();
        assert_eq!(u.sketch.loops.len(), 1);
        assert!((u.sketch.loops[0].signed_area() - 200.0).abs() < 1e-9);
        assert_eq!(u.sketch.loops[0].segments.len(), 4, "{:?}", u.sketch.loops[0].segments);
    }

    /// A fillet is an arc tangent to both edges: its centre is `r` from each,
    /// its ends are on them, and the loop's tangent is continuous through it.
    /// A radius that does not fit is refused with the largest that would.
    #[test]
    fn fillet2d_is_tangent_and_refuses_with_the_max() {
        let s = xy(Loop::rect(P2::new(0.0, 0.0), 20.0, 10.0));
        let cs = corners(&s, "s");
        assert_eq!(cs.len(), 4);
        assert_eq!(cs[1].name, "s/loop[0]/corner[1]");
        assert!((cs[1].angle_deg - 90.0).abs() < 1e-9);
        let f = fillet2d(&s, "s", 3.0, &CornerSelect::Some(vec![(0, 1)])).unwrap();
        let l = &f.loops[0];
        assert_eq!(l.segments.len(), 5);
        let Segment::Arc { a, b, via } = &l.segments[1] else { panic!("{:?}", l.segments) };
        let (c, r, _, _) = arc_params(*a, *b, *via).unwrap();
        assert!((r - 3.0).abs() < 1e-9);
        assert!(dist(c, P2::new(7.0, -2.0)) < 1e-9, "{c:?}");
        assert!(dist(*a, P2::new(7.0, -5.0)) < 1e-9 && dist(*b, P2::new(10.0, -2.0)) < 1e-9);
        for i in 0..l.segments.len() {
            let t0 = l.segments[i].end_direction().unwrap();
            let t1 = l.segments[(i + 1) % l.segments.len()].start_direction().unwrap();
            if i == 0 || i == 1 {
                assert!(dist(t0, t1) < 1e-9, "tangent continuity at joint {i}: {t0:?} vs {t1:?}");
            }
        }
        assert!(l.is_closed(1e-9));
        assert!((exact_area(l, 1e-4) - (200.0 - (4.0 - PI) * 9.0 / 4.0)).abs() < 1e-9);
        assert_eq!(f.loop_names, vec!["s/loop[0]"]);

        let all = fillet2d(&s, "s", 2.0, &CornerSelect::All).unwrap();
        assert_eq!((lines(&all.loops[0]), arcs(&all.loops[0])), (4, 4));
        assert!(corners(&all, "s").is_empty(), "a filleted rectangle has no corners left");

        let err = fillet2d(&s, "s", 6.0, &CornerSelect::All).unwrap_err().to_string();
        assert!(err.contains("does not fit") && err.contains("largest that fits both is 5"), "{err}");
        let err = fillet2d(&s, "s", 12.0, &CornerSelect::Some(vec![(0, 0)])).unwrap_err().to_string();
        assert!(err.contains("does not fit at s/loop[0]/corner[0]") && err.contains("largest that fits is 10"), "{err}");
    }

    /// A fillet between a line and an arc, and between a spline and anything
    /// (refused). The slot's ends are tangent already, so a slot has no corners.
    #[test]
    fn fillet2d_on_a_line_arc_corner() {
        assert!(corners(&xy(Loop::slot(P2::new(0.0, 0.0), 10.0, 4.0)), "s").is_empty());
        // A quarter disc: two radii and a quarter arc; fillet the corner
        // between the horizontal radius and the arc.
        let quarter = Loop {
            segments: vec![
                Segment::Line { a: P2::new(0.0, 0.0), b: P2::new(10.0, 0.0) },
                Segment::Arc { a: P2::new(10.0, 0.0), b: P2::new(0.0, 10.0), via: polar(P2::new(0.0, 0.0), 10.0, PI / 4.0) },
                Segment::Line { a: P2::new(0.0, 10.0), b: P2::new(0.0, 0.0) },
            ],
        };
        let s = xy(quarter);
        let f = fillet2d(&s, "s", 2.0, &CornerSelect::Some(vec![(0, 1)])).unwrap();
        let l = &f.loops[0];
        assert_eq!(l.segments.len(), 4);
        let Segment::Arc { a, b, via } = &l.segments[1] else { panic!() };
        let (c, r, _, _) = arc_params(*a, *b, *via).unwrap();
        assert!((r - 2.0).abs() < 1e-9);
        // The centre is 2 above the x axis and 2 inside the big circle.
        assert!((c.y - 2.0).abs() < 1e-9 && (norm(c) - 8.0).abs() < 1e-9, "{c:?}");
        assert!(dist(*b, mul(unit(c).unwrap(), 10.0)) < 1e-9, "tangent point on the arc");
        let t0 = l.segments[1].end_direction().unwrap();
        let t1 = l.segments[2].start_direction().unwrap();
        assert!(dist(t0, t1) < 1e-9);

        let spl = xy(Loop::spline(&[P2::new(0.0, 0.0), P2::new(10.0, 0.0), P2::new(5.0, 8.0)], 0.5).unwrap());
        let sp = Sketch::new(Plane::XY).with_loop(Loop { segments: vec![spl.loops[0].segments[0].clone(), Segment::Line { a: P2::new(10.0, 0.0), b: P2::new(0.0, 0.0) }] });
        let err = fillet2d(&sp, "s", 1.0, &CornerSelect::All).unwrap_err().to_string();
        assert!(err.contains("joins a spline"), "{err}");
    }

    /// A chamfer cuts back exactly `d` along both edges, and two chamfers on
    /// one edge must fit together.
    #[test]
    fn chamfer2d_cuts_back_by_the_distance() {
        let s = xy(Loop::rect(P2::new(0.0, 0.0), 20.0, 10.0));
        let c = chamfer2d(&s, "s", 3.0, &CornerSelect::Some(vec![(0, 2)])).unwrap();
        let l = &c.loops[0];
        assert_eq!(l.segments.len(), 5);
        let Segment::Line { a, b } = &l.segments[2] else { panic!("{:?}", l.segments) };
        assert!(dist(*a, P2::new(10.0, 2.0)) < 1e-9 && dist(*b, P2::new(7.0, 5.0)) < 1e-9, "{a:?} {b:?}");
        assert!((l.signed_area() - (200.0 - 4.5)).abs() < 1e-9);
        let all = chamfer2d(&s, "s", 4.0, &CornerSelect::All).unwrap();
        assert_eq!(all.loops[0].segments.len(), 8);
        assert!((all.loops[0].signed_area() - (200.0 - 32.0)).abs() < 1e-9);
        // Exactly half the short side from both ends is allowed: the side
        // vanishes and the loop is a hexagon.
        let hex = chamfer2d(&s, "s", 5.0, &CornerSelect::All).unwrap();
        assert_eq!(hex.loops[0].segments.len(), 6);
        assert!((hex.loops[0].signed_area() - 150.0).abs() < 1e-9);
        let err = chamfer2d(&s, "s", 6.0, &CornerSelect::All).unwrap_err().to_string();
        assert!(err.contains("largest that fits both is 5"), "{err}");
        let err = chamfer2d(&s, "s", 11.0, &CornerSelect::Some(vec![(0, 0)])).unwrap_err().to_string();
        assert!(err.contains("largest that fits is 10"), "{err}");
    }

    /// Mirror: the image is named, a half on the axis merges into one loop,
    /// `keep=False` is the image alone, `merge=False` is both.
    #[test]
    fn mirror2d_names_and_merges() {
        let half = xy(Loop::rect(P2::new(5.0, 0.0), 10.0, 6.0)); // x from 0 to 10
        let m = mirror2d(&half, "h", P2::new(0.0, 0.0), P2::new(0.0, 1.0), true, true, 0.05).unwrap();
        assert_eq!(m.sketch.loops.len(), 1, "{:?}", m.sketch.loop_names);
        assert!((m.sketch.loops[0].signed_area() - 120.0).abs() < 1e-9);
        assert_eq!(m.sketch.loops[0].segments.len(), 4);
        assert_eq!(m.sketch.loop_names, vec!["h/loop[0]|h/loop[0]/mirror"]);
        let both = mirror2d(&half, "h", P2::new(0.0, 0.0), P2::new(0.0, 1.0), true, false, 0.05).unwrap();
        assert_eq!(both.sketch.loop_names, vec!["h/loop[0]", "h/loop[0]/mirror"]);
        let img = mirror2d(&half, "h", P2::new(0.0, 0.0), P2::new(0.0, 1.0), false, true, 0.05).unwrap();
        assert_eq!(img.sketch.loop_names, vec!["h/loop[0]/mirror"]);
        for p in img.sketch.loops[0].sample() {
            assert!(p.x <= 1e-9);
        }
        // A disc off the axis: the image is a second disc, and its arcs are still arcs.
        let disc = xy(Loop::circle(P2::new(4.0, 0.0), 2.0));
        let two = mirror2d(&disc, "d", P2::new(0.0, 0.0), P2::new(0.0, 1.0), true, true, 0.05).unwrap();
        assert_eq!(two.sketch.loops.len(), 2);
        assert_eq!(arcs(&two.sketch.loops[1]), 2);
    }

    /// Patterns: copies are named `/copy[k]`, and `merge` unites overlapping
    /// copies into one loop.
    #[test]
    fn pattern2d_names_copies_and_merges_overlaps() {
        let hole = xy(Loop::circle(P2::new(0.0, 0.0), 1.0));
        let xfs: Vec<Xf2> = (0..3).map(|k| Xf2::translation(P2::new(5.0 * k as f64, 0.0))).collect();
        let p = pattern2d(&hole, "h", &xfs, false, 0.05).unwrap();
        assert_eq!(p.sketch.loop_names, vec!["h/loop[0]", "h/loop[0]/copy[1]", "h/loop[0]/copy[2]"]);
        let rot: Vec<Xf2> = (0..4).map(|k| Xf2::rotation(P2::new(0.0, 0.0), TAU * k as f64 / 4.0)).collect();
        let off = xy(Loop::circle(P2::new(6.0, 0.0), 1.0));
        let r = pattern2d(&off, "h", &rot, false, 0.05).unwrap();
        assert_eq!(r.sketch.loops.len(), 4);
        let c3 = r.sketch.loops[3].sample();
        assert!(c3.iter().all(|p| (norm(sub(*p, P2::new(0.0, -6.0))) - 1.0).abs() < 1e-6), "{:?}", c3[0]);
        // Overlapping slots merge into one long slot.
        let slot = xy(Loop::slot(P2::new(0.0, 0.0), 6.0, 2.0));
        let steps: Vec<Xf2> = (0..3).map(|k| Xf2::translation(P2::new(4.0 * k as f64, 0.0))).collect();
        let m = pattern2d(&slot, "s", &steps, true, 0.05).unwrap();
        assert_eq!(m.sketch.loops.len(), 1, "{:?}", m.sketch.loop_names);
        assert!((exact_area(&m.sketch.loops[0], 1e-4) - (12.0 * 2.0 + PI)).abs() < 1e-9, "{}", exact_area(&m.sketch.loops[0], 1e-4));
        assert_eq!(arcs(&m.sketch.loops[0]), 2);
    }

    /// Splines survive: an offset spline loop is spline segments again, at
    /// the right distance, and a boolean that does not touch a spline loop
    /// hands it back exactly.
    #[test]
    fn splines_are_refitted_on_offset_and_kept_when_untouched() {
        let pts = [P2::new(10.0, 0.0), P2::new(0.0, 8.0), P2::new(-10.0, 0.0), P2::new(0.0, -8.0)];
        let egg = Loop::spline(&pts, 0.5).unwrap();
        let s = xy(egg.clone());
        let o = offset(&s, "s", 1.5, Join::Round, 0.02).unwrap();
        assert_eq!(o.sketch.loops.len(), 1);
        let l = &o.sketch.loops[0];
        assert!(l.segments.iter().all(|s| matches!(s, Segment::Bezier { .. })), "{:?}", l.segments.iter().map(|s| match s { Segment::Line { .. } => "line", Segment::Arc { .. } => "arc", Segment::Bezier { .. } => "bezier" }).collect::<Vec<_>>());
        assert!(l.segments.len() <= 60, "{} segments", l.segments.len());
        for p in l.sample_tolerance(0.01) {
            let d = dist_to(&egg, p);
            assert!((d - 1.5).abs() < 0.05, "{p:?} is {d} from the egg");
        }
        let far = xy(Loop::rect(P2::new(40.0, 0.0), 4.0, 4.0));
        let u = boolean(&s, "s", &far, "f", BoolOp::Union, 0.05).unwrap();
        assert_eq!(u.sketch.loops[0], egg);
        // Cut by a rectangle: the spline part is re-fitted, the cut is a line.
        let half = xy(Loop::rect(P2::new(20.0, 0.0), 40.0, 40.0));
        let cut = boolean(&s, "s", &half, "h", BoolOp::Subtract, 0.02).unwrap();
        assert_eq!(cut.sketch.loops.len(), 1);
        assert!((cut.sketch.loops[0].signed_area() - egg.signed_area() / 2.0).abs() < 0.2);
        assert!(cut.sketch.loops[0].segments.iter().any(|s| matches!(s, Segment::Bezier { .. })));
        assert!(cut.sketch.loops[0].segments.iter().any(|s| matches!(s, Segment::Line { .. })));
    }

    /// Two loops meeting at a single vertex stay two loops.
    #[test]
    fn a_corner_touch_keeps_two_loops() {
        let a = xy(Loop::rect(P2::new(0.0, 0.0), 10.0, 10.0));
        let b = xy(Loop::rect(P2::new(10.0, 10.0), 10.0, 10.0));
        let u = boolean(&a, "a", &b, "b", BoolOp::Union, 0.05).unwrap();
        assert_eq!(u.sketch.loops.len(), 2, "{:?}", u.sketch.loop_names);
        assert!(u.sketch.regions().unwrap().len() == 2);
    }

    /// Sketches on different planes are refused; a coincident plane with a
    /// different frame is mapped.
    #[test]
    fn booleans_need_one_plane() {
        let a = xy(Loop::rect(P2::new(0.0, 0.0), 10.0, 10.0));
        let b = Sketch::new(Plane::XZ).with_loop(Loop::rect(P2::new(0.0, 0.0), 10.0, 10.0));
        let err = boolean(&a, "a", &b, "b", BoolOp::Union, 0.05).unwrap_err().to_string();
        assert!(err.contains("different planes"), "{err}");
        let shifted = Plane::new(Vec3::new(5.0, 5.0, 0.0), Vec3::Z, Vec3::Y).unwrap();
        let c = Sketch::new(shifted).with_loop(Loop::rect(P2::new(0.0, 0.0), 10.0, 10.0));
        let i = boolean(&a, "a", &c, "c", BoolOp::Intersect, 0.05).unwrap();
        assert!((i.sketch.loops[0].signed_area() - 25.0).abs() < 1e-9, "{}", i.sketch.loops[0].signed_area());
    }

    /// The winding-number ray with the half-open rule: vertices on the ray
    /// count once, touches count zero.
    #[test]
    fn ray_crossings_follow_the_half_open_rule() {
        let p = P2::new(0.0, 0.0);
        let (n, t) = (P2::new(1.0, 0.0), P2::new(0.0, 1.0));
        // A vertical line crossing the ray, going up: +1; going down: -1.
        assert_eq!(ray_crossings(&Prim::Line { a: P2::new(2.0, -1.0), b: P2::new(2.0, 1.0) }, p, n, t, 1e-9), 1);
        assert_eq!(ray_crossings(&Prim::Line { a: P2::new(2.0, 1.0), b: P2::new(2.0, -1.0) }, p, n, t, 1e-9), -1);
        // Behind the point: nothing.
        assert_eq!(ray_crossings(&Prim::Line { a: P2::new(-2.0, -1.0), b: P2::new(-2.0, 1.0) }, p, n, t, 1e-9), 0);
        // A vertex on the ray: the piece above counts, the piece below does not.
        assert_eq!(ray_crossings(&Prim::Line { a: P2::new(2.0, 0.0), b: P2::new(3.0, 1.0) }, p, n, t, 1e-9), 1);
        assert_eq!(ray_crossings(&Prim::Line { a: P2::new(2.0, 0.0), b: P2::new(3.0, -1.0) }, p, n, t, 1e-9), 0);
        // A circle around a point ahead, as two halves whose ends sit on the
        // ray line: the upper half leaves up and arrives down (net 0), the
        // lower half touches from below (0); the circle crosses nothing.
        assert_eq!(ray_crossings(&Prim::Arc { c: P2::new(5.0, 0.0), r: 1.0, t0: 0.0, sweep: PI }, p, n, t, 1e-9), 0);
        assert_eq!(ray_crossings(&Prim::Arc { c: P2::new(5.0, 0.0), r: 1.0, t0: PI, sweep: PI }, p, n, t, 1e-9), 0);
        // Rotated so the ray cuts it: in on the left going down, out on the right going up.
        assert_eq!(ray_crossings(&Prim::Arc { c: P2::new(5.0, 0.0), r: 1.0, t0: PI / 2.0, sweep: PI }, p, n, t, 1e-9), -1);
        assert_eq!(ray_crossings(&Prim::Arc { c: P2::new(5.0, 0.0), r: 1.0, t0: -PI / 2.0, sweep: PI }, p, n, t, 1e-9), 1);
        // A circle enclosing the point: the ray leaves it once, going up on the right side.
        assert_eq!(ray_crossings(&Prim::Arc { c: P2::new(0.0, 0.0), r: 1.0, t0: -PI / 2.0, sweep: PI }, p, n, t, 1e-9), 1);
        // Tangent to the ray line: zero.
        assert_eq!(ray_crossings(&Prim::Arc { c: P2::new(5.0, 1.0), r: 1.0, t0: 0.0, sweep: PI }, p, n, t, 1e-9), 0);
    }
}



