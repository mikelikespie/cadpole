//! The clearance query: how far apart are two bodies, and where.
//!
//! **Why this exists.** A print-in-place mechanism is *made of*
//! near-contacts: does the planet clear the ring, does the pin clear the
//! bore — questions about two bodies 0.15–0.4 mm apart. `intersect` cannot
//! be the fit test, because an empty result is an *error* and a touching
//! pair is a refusal, and a union-volume comparison costs a boolean per
//! pair and inherits the chord error.
//! The question is a distance, not a boolean, and this module answers it
//! from the tessellations both kernels already produce.
//!
//! **Method.** One axis-aligned bounding-box tree per mesh (median split on
//! the widest axis, four triangles a leaf, a pure function of the triangle
//! order so the same pair always gives the same answer), then a best-first
//! traversal of the two trees together, ordered by the distance between the
//! two nodes' boxes: that distance is a lower bound on any triangle pair
//! beneath them, so the first node pair popped whose bound is not below the
//! best exact distance found ends the search. Leaves are compared exactly —
//! the closest points of two triangles that do not cross lie on a vertex and
//! a face or on two edges, so the fifteen vertex–triangle and edge–edge
//! candidates are enumerated, and a segment–triangle crossing is a distance
//! of zero. [`kernel::disposition`](crate::kernel::disposition)'s
//! `boundaries_meet` and [`solid::pair_defects`](crate::solid::pair_defects)
//! do the same broad phase against the renderer's f32 BVH; this one is in
//! f64 and walks two trees against each other, which is what a *closest*
//! pair, rather than an *overlapping* pair, needs.
//!
//! **Uncertainty.** Both operands are the document's tessellations, and a
//! chord lies inside its true surface by up to the tessellation tolerance.
//! Each of the two surfaces can therefore be off by that much, so the true
//! gap is within twice the tolerance of the reported one, and a planar face
//! contributes nothing. [`Clearance::tolerance`] carries the figure so the
//! caller can say so; `export(tolerance=)` per body is the way to tighten it.
//!
//! **Touching versus overlapping.** A distance of zero says the surfaces
//! meet; whether the *material* is shared is a second question, answered by
//! a cheap test rather than a boolean: a pair of triangles that reach past
//! each other's planes is a crossing, and otherwise a handful of probe points
//! just inside one body's surface are tested against the other body's
//! winding number. Material of `a` that is also material of `b` is an
//! overlap; two boxes sharing a face are not.

use std::cmp::Ordering;
use std::collections::{BTreeMap, BinaryHeap};

use crate::geom::Vec3;
use crate::tessellate::Mesh;
use crate::fmath;

/// Triangles per leaf. Four keeps the leaf–leaf narrow phase to sixteen
/// exact tests, which is cheaper than the box tests a deeper tree would add.
const LEAF: usize = 4;

/// Probe points per body for the containment half of the overlap test when
/// the surfaces touch. When they do not, one probe settles it.
const PROBES: usize = 16;

/// The answer to "how far apart are these two bodies".
#[derive(Clone, Debug, PartialEq)]
pub struct Clearance {
    /// Minimum distance between the two tessellated surfaces, in mm. `0.0`
    /// when they touch or overlap — including one body wholly inside the
    /// other, whose surfaces may be far apart; `overlap` says which.
    pub distance: f64,
    /// The two bodies share material: they cross, or one lies inside the
    /// other. False for two bodies that only touch, and for a body sitting
    /// in another's cavity.
    pub overlap: bool,
    /// The closest point on `a`'s surface. For a body inside the other
    /// these are still the closest pair of surface points, `distance` apart
    /// only when there is no overlap.
    pub a_point: Vec3,
    /// The closest point on `b`'s surface.
    pub b_point: Vec3,
    /// Topology face index (`Mesh::face_ids`) the closest point of `a` lies
    /// on; `Topology::faces[face_a].name` is its name.
    pub face_a: u32,
    /// Likewise for `b`.
    pub face_b: u32,
    /// The triangles the closest points lie on, for a caller that wants the
    /// geometry rather than the face.
    pub tri_a: u32,
    pub tri_b: u32,
    /// The tessellation tolerance the operands were meshed at: how far each
    /// surface may be from its chords, so how uncertain `distance` is.
    pub tolerance: f64,
}

impl Clearance {
    /// Zero distance without shared material.
    pub fn touching(&self) -> bool {
        self.distance == 0.0 && !self.overlap
    }
}

/// One place where the gap between two bodies is below a threshold.
#[derive(Clone, Debug, PartialEq)]
pub struct NearPair {
    pub distance: f64,
    pub a_point: Vec3,
    pub b_point: Vec3,
    pub face_a: u32,
    pub face_b: u32,
}

/// Every region where two bodies come within a threshold of each other.
#[derive(Clone, Debug, Default, PartialEq)]
pub struct NearMap {
    /// Ascending by distance, at most the `limit` the caller asked for.
    pub pairs: Vec<NearPair>,
    /// How many regions there were before the cap; more than `pairs.len()`
    /// when the list was capped.
    pub regions: usize,
}

impl NearMap {
    pub fn capped(&self) -> bool {
        self.regions > self.pairs.len()
    }
}

/// Minimum distance between two meshes, with where it is and whether the
/// bodies overlap. `None` when either mesh has no triangles.
///
/// `tolerance` is the tessellation tolerance both were meshed at; it is
/// reported, not used.
pub fn distance(a: &Mesh, b: &Mesh, tolerance: f64) -> Option<Clearance> {
    let ta = Tree::build(a);
    let tb = Tree::build(b);
    closest(&ta, &tb, tolerance)
}

/// Every region where `a` and `b` are closer than `below`: the closest pair
/// of each `spacing`-sized neighbourhood, ascending by distance and capped
/// at `limit` entries. `NearMap::regions` is the count before the cap.
///
/// Bucketing is what keeps the answer bounded: a cylinder in a bore has
/// thousands of triangle pairs within the clearance, and what a script
/// wants is one number per patch of the ring, not one per chord. Pairs are
/// first binned by `spacing`-sized cell, keeping each cell's closest, then
/// any entry within `spacing` of a closer one is dropped, so a cell wall
/// that happens to slice a ring does not report the ring twice.
pub fn near(a: &Mesh, b: &Mesh, below: f64, spacing: f64, limit: usize) -> NearMap {
    let ta = Tree::build(a);
    let tb = Tree::build(b);
    near_trees(&ta, &tb, below, spacing, limit)
}

// ---------------------------------------------------------------------------
// The tree
// ---------------------------------------------------------------------------

#[derive(Clone, Copy, Debug)]
struct Node {
    lo: Vec3,
    hi: Vec3,
    /// Children, for an inner node.
    left: u32,
    right: u32,
    /// Slot range in `Tree::order`, for a leaf; `count == 0` is an inner node.
    start: u32,
    count: u32,
}

impl Node {
    fn is_leaf(&self) -> bool {
        self.count > 0
    }
    fn extent(&self) -> f64 {
        (self.hi - self.lo).length()
    }
}

/// An axis-aligned bounding-box tree over one mesh's triangles, in f64.
pub struct Tree {
    nodes: Vec<Node>,
    /// Every triangle, by mesh index.
    tris: Vec<[Vec3; 3]>,
    /// Per-triangle bounds, by mesh index.
    boxes: Vec<(Vec3, Vec3)>,
    /// Leaf slots: triangle indices in the order the build left them.
    order: Vec<u32>,
    /// `Mesh::face_ids`, or empty.
    face_ids: Vec<u32>,
    /// One triangle per connected piece of the surface (vertices joined by
    /// position, not by index, so an unwelded mesh is still one piece per
    /// lump): the largest, lowest index on a tie. A body of several lumps —
    /// `union` of two disjoint bodies, `pattern(merge=True)` with gaps — is
    /// only "all inside or all outside" the other body lump by lump.
    lumps: Vec<u32>,
}

impl Tree {
    /// Build the tree. Deterministic: the split is a median on the widest
    /// axis with the triangle index as the tie-break, and every leaf is
    /// sorted by index, so the same mesh gives the same tree and the same
    /// traversal on every machine.
    pub fn build(mesh: &Mesh) -> Tree {
        let n = mesh.triangle_count();
        let mut tris = Vec::with_capacity(n);
        let mut boxes = Vec::with_capacity(n);
        let mut keys = Vec::with_capacity(n);
        for t in 0..n {
            let v = crate::solid::tri(mesh, t);
            let lo = v[0].min(v[1]).min(v[2]);
            let hi = v[0].max(v[1]).max(v[2]);
            tris.push(v);
            boxes.push((lo, hi));
            keys.push((lo + hi) * 0.5);
        }
        let mut order: Vec<u32> = (0..n as u32).collect();
        let mut nodes = Vec::with_capacity(2 * n / LEAF + 1);
        if n > 0 {
            nodes.push(Node { lo: Vec3::ZERO, hi: Vec3::ZERO, left: 0, right: 0, start: 0, count: 0 });
            // (node, start, end) — an explicit stack keeps a 100k-triangle
            // build off the call stack.
            let mut work = vec![(0usize, 0usize, n)];
            while let Some((node, start, end)) = work.pop() {
                let mut lo = Vec3::new(f64::MAX, f64::MAX, f64::MAX);
                let mut hi = Vec3::new(f64::MIN, f64::MIN, f64::MIN);
                let mut klo = lo;
                let mut khi = hi;
                for &t in &order[start..end] {
                    let (a, b) = boxes[t as usize];
                    lo = lo.min(a);
                    hi = hi.max(b);
                    klo = klo.min(keys[t as usize]);
                    khi = khi.max(keys[t as usize]);
                }
                nodes[node].lo = lo;
                nodes[node].hi = hi;
                let count = end - start;
                if count <= LEAF {
                    order[start..end].sort_unstable();
                    nodes[node].start = start as u32;
                    nodes[node].count = count as u32;
                    continue;
                }
                let spread = khi - klo;
                let axis = if spread.x >= spread.y && spread.x >= spread.z {
                    0
                } else if spread.y >= spread.z {
                    1
                } else {
                    2
                };
                let key = |t: u32| -> f64 {
                    let k = keys[t as usize];
                    match axis {
                        0 => k.x,
                        1 => k.y,
                        _ => k.z,
                    }
                };
                let mid = start + count / 2;
                order[start..end].select_nth_unstable_by(mid - start, |&p, &q| {
                    key(p).partial_cmp(&key(q)).unwrap_or(Ordering::Equal).then(p.cmp(&q))
                });
                let left = nodes.len();
                nodes.push(Node { lo: Vec3::ZERO, hi: Vec3::ZERO, left: 0, right: 0, start: 0, count: 0 });
                let right = nodes.len();
                nodes.push(Node { lo: Vec3::ZERO, hi: Vec3::ZERO, left: 0, right: 0, start: 0, count: 0 });
                nodes[node].left = left as u32;
                nodes[node].right = right as u32;
                work.push((left, start, mid));
                work.push((right, mid, end));
            }
        }
        let lumps = lumps(&tris);
        Tree { nodes, tris, boxes, order, face_ids: mesh.face_ids.clone(), lumps }
    }

    pub fn is_empty(&self) -> bool {
        self.tris.is_empty()
    }

    pub fn triangle_count(&self) -> usize {
        self.tris.len()
    }

    /// The whole mesh's bounds.
    pub fn bounds(&self) -> Option<(Vec3, Vec3)> {
        self.nodes.first().map(|n| (n.lo, n.hi))
    }

    fn face_of(&self, tri: u32) -> u32 {
        self.face_ids.get(tri as usize).copied().unwrap_or(0)
    }

    /// Winding number of `p` with respect to this mesh: ±1 in the material,
    /// 0 outside and 0 in a void, whose triangles face the other way. The
    /// solid-angle form of [`crate::kernel::encloses`], kept as a number so
    /// a point on the surface (½) can be told from either side.
    pub fn winding(&self, p: Vec3) -> f64 {
        let mut sum = 0.0;
        for t in &self.tris {
            let (a, b, c) = (t[0] - p, t[1] - p, t[2] - p);
            let (la, lb, lc) = (a.length(), b.length(), c.length());
            if la == 0.0 || lb == 0.0 || lc == 0.0 {
                continue;
            }
            let num = a.dot(b.cross(c));
            let den = la * lb * lc + a.dot(b) * lc + b.dot(c) * la + c.dot(a) * lb;
            sum += fmath::atan2(num, den);
        }
        sum / (2.0 * std::f64::consts::PI)
    }
}

/// One representative triangle per connected piece of `tris`: union-find
/// over vertices keyed by their exact coordinates, then the largest
/// non-degenerate triangle of each piece, in order of first appearance.
fn lumps(tris: &[[Vec3; 3]]) -> Vec<u32> {
    fn find(parent: &mut [usize], mut i: usize) -> usize {
        while parent[i] != i {
            parent[i] = parent[parent[i]];
            i = parent[i];
        }
        i
    }
    let mut ids: std::collections::HashMap<[u64; 3], usize> = std::collections::HashMap::new();
    let mut parent: Vec<usize> = Vec::new();
    let mut first: Vec<usize> = Vec::with_capacity(tris.len());
    for t in tris {
        let mut root = None;
        for p in t {
            let next = ids.len();
            let id = *ids.entry([p.x.to_bits(), p.y.to_bits(), p.z.to_bits()]).or_insert(next);
            if id == parent.len() {
                parent.push(id);
            }
            let r = find(&mut parent, id);
            match root {
                None => root = Some(r),
                Some(r0) if r0 != r => {
                    let (lo, hi) = (r0.min(r), r0.max(r));
                    parent[hi] = lo;
                    root = Some(lo);
                }
                _ => {}
            }
        }
        first.push(root.unwrap_or(0));
    }
    // (piece root) -> (best area, triangle), in order of first appearance.
    let mut best: Vec<(usize, f64, u32)> = Vec::new();
    let mut slot: std::collections::HashMap<usize, usize> = std::collections::HashMap::new();
    for (k, t) in tris.iter().enumerate() {
        let r = find(&mut parent, first[k]);
        let area = (t[1] - t[0]).cross(t[2] - t[0]).length();
        if !(area > 0.0) || !area.is_finite() {
            continue;
        }
        match slot.get(&r) {
            Some(&i) => {
                if area > best[i].1 {
                    best[i] = (r, area, k as u32);
                }
            }
            None => {
                slot.insert(r, best.len());
                best.push((r, area, k as u32));
            }
        }
    }
    best.into_iter().map(|(_, _, k)| k).collect()
}

// ---------------------------------------------------------------------------
// Closest pair
// ---------------------------------------------------------------------------

/// A node pair waiting in the best-first queue, ordered so the *smallest*
/// lower bound pops first, with the node indices as the tie-break so the
/// order — and therefore which of two equally close pairs is reported — is
/// the same on every run.
#[derive(Clone, Copy, Debug)]
struct Cand {
    lb: f64,
    na: u32,
    nb: u32,
}

impl PartialEq for Cand {
    fn eq(&self, o: &Self) -> bool {
        self.cmp(o) == Ordering::Equal
    }
}
impl Eq for Cand {}
impl PartialOrd for Cand {
    fn partial_cmp(&self, o: &Self) -> Option<Ordering> {
        Some(self.cmp(o))
    }
}
impl Ord for Cand {
    fn cmp(&self, o: &Self) -> Ordering {
        // Reversed: BinaryHeap is a max-heap and we want the minimum bound.
        o.lb.partial_cmp(&self.lb).unwrap_or(Ordering::Equal).then(o.na.cmp(&self.na)).then(o.nb.cmp(&self.nb))
    }
}

/// A second lower bound on two triangles' distance, along the line between
/// their centroids: the gap between their projections onto it. Boxes see a
/// radial gap but not a tangential offset when the two boxes overlap on that
/// axis — a pin in a bore near the X axis is the case — and this bound sees
/// the hypotenuse. Forty flops against the several hundred of an exact test.
fn projected_gap(a: &[Vec3; 3], b: &[Vec3; 3]) -> f64 {
    let ca = (a[0] + a[1] + a[2]) * (1.0 / 3.0);
    let cb = (b[0] + b[1] + b[2]) * (1.0 / 3.0);
    let u = cb - ca;
    let l = u.length();
    if l <= 0.0 || !l.is_finite() {
        return 0.0;
    }
    let u = u * (1.0 / l);
    let max_a = a.iter().map(|p| u.dot(*p)).fold(f64::MIN, f64::max);
    let min_b = b.iter().map(|p| u.dot(*p)).fold(f64::MAX, f64::min);
    (min_b - max_a).max(0.0)
}

/// Euclidean distance between two boxes: zero when they overlap.
pub fn box_distance((alo, ahi): (Vec3, Vec3), (blo, bhi): (Vec3, Vec3)) -> f64 {
    let gap = |a0: f64, a1: f64, b0: f64, b1: f64| (b0 - a1).max(a0 - b1).max(0.0);
    let d = Vec3::new(gap(alo.x, ahi.x, blo.x, bhi.x), gap(alo.y, ahi.y, blo.y, bhi.y), gap(alo.z, ahi.z, blo.z, bhi.z));
    d.length()
}

/// How much two triangles face each other across their closest points:
/// each normal's component along the line joining the points, summed, so
/// two walls squarely across a gap score 2, a wall and the edge of a face
/// at right angles to it score 1, and for touching triangles the negative
/// of the normals' dot product, so a face resting on a face scores 1.
///
/// Why it matters: the closest pair of two cubes 0.4 mm apart is every
/// point of the two facing walls, and the *edges* of the four other faces
/// of each cube lie on those walls at the same distance. Which of the tied
/// pairs is reported decides whether the answer names `a/right` or
/// `a/back`, and the wall is the one a person means.
fn facing(a: &[Vec3; 3], b: &[Vec3; 3], pa: Vec3, pb: Vec3, d: f64) -> f64 {
    let (na, _) = crate::solid::tri_normal_area(a);
    let (nb, _) = crate::solid::tri_normal_area(b);
    if d > 0.0 {
        let u = (pb - pa) * (1.0 / d);
        na.dot(u) - nb.dot(u)
    } else {
        -na.dot(nb)
    }
}

/// The closest pair of two trees.
pub fn closest(a: &Tree, b: &Tree, tolerance: f64) -> Option<Clearance> {
    if a.is_empty() || b.is_empty() {
        return None;
    }
    // Pairs within this of the best are ties, broken by `facing`. A tie is
    // the same geometry reached by different arithmetic (an edge shared by
    // two faces), so the window is rounding, scaled to the model — not
    // f32's quantum, which would admit merely-near pairs and, on a ring of
    // contact, double the work.
    let scale = [a.nodes[0], b.nodes[0]].iter().map(|n| n.extent()).fold(1.0f64, f64::max);
    let tie = scale * 1e-9;
    let mut best = f64::INFINITY;
    let mut best_facing = f64::NEG_INFINITY;
    let mut hit: Option<(Vec3, Vec3, u32, u32)> = None;
    let mut crossing = false;
    let mut heap = BinaryHeap::new();
    heap.push(Cand { lb: box_distance((a.nodes[0].lo, a.nodes[0].hi), (b.nodes[0].lo, b.nodes[0].hi)), na: 0, nb: 0 });
    while let Some(Cand { lb, na, nb }) = heap.pop() {
        if lb > best + tie {
            break;
        }
        let (x, y) = (a.nodes[na as usize], b.nodes[nb as usize]);
        if x.is_leaf() && y.is_leaf() {
            for &ta in &a.order[x.start as usize..(x.start + x.count) as usize] {
                for &tb in &b.order[y.start as usize..(y.start + y.count) as usize] {
                    let (va, vb) = (&a.tris[ta as usize], &b.tris[tb as usize]);
                    if box_distance(a.boxes[ta as usize], b.boxes[tb as usize]) > best + tie
                        || projected_gap(va, vb) > best + tie
                    {
                        continue;
                    }
                    let (d, pa, pb, crossed) = tri_tri(va, vb);
                    if d > best + tie {
                        continue;
                    }
                    let f = facing(va, vb, pa, pb, d);
                    if d < best - tie || f > best_facing {
                        best = best.min(d);
                        best_facing = f;
                        hit = Some((pa, pb, ta, tb));
                        crossing = crossed;
                    }
                }
            }
            continue;
        }
        // Split the inner node, or the bigger of two inner nodes: the bound
        // tightens fastest on the side with the most room in it.
        let split_a = !x.is_leaf() && (y.is_leaf() || x.extent() >= y.extent());
        let children: [(u32, u32); 2] = if split_a { [(x.left, nb), (x.right, nb)] } else { [(na, y.left), (na, y.right)] };
        for (ca, cb) in children {
            let (p, q) = (a.nodes[ca as usize], b.nodes[cb as usize]);
            let lb = box_distance((p.lo, p.hi), (q.lo, q.hi));
            if lb <= best + tie {
                heap.push(Cand { lb, na: ca, nb: cb });
            }
        }
    }
    let (pa, pb, ta, tb) = hit?;
    let overlap = if best > 0.0 {
        // The surfaces never meet, so one probe per lump each way decides:
        // if a point of a lump's material is in b, all of that lump is.
        // One probe per *body* is not enough — a body's first triangle may
        // sit on a lump that is clear of b while another lump is buried.
        contained(a, b, 0) || contained(b, a, 0)
    } else {
        crossing || contained(a, b, PROBES) || contained(b, a, PROBES)
    };
    Some(Clearance {
        // Shared material is no clearance at all, whatever the gap between
        // the surfaces: a part buried in another is 0 from it, not the depth
        // it is buried at.
        distance: if overlap { 0.0 } else { best },
        overlap,
        a_point: pa,
        b_point: pb,
        face_a: a.face_of(ta),
        face_b: b.face_of(tb),
        tri_a: ta,
        tri_b: tb,
        tolerance,
    })
}

/// Is any probe point just inside `inner`'s surface inside `outer`'s
/// material? Probes are triangle centroids pushed a little along the inward
/// normal: one on every lump of `inner` ([`Tree::lumps`]), plus `probes`
/// more spread across the mesh, so a body touching another from outside
/// (its probe lands in its own material, outside the other) and a body
/// sitting in a cavity (winding 0) both answer no.
fn contained(inner: &Tree, outer: &Tree, probes: usize) -> bool {
    let n = inner.tris.len();
    let spread = if probes == 0 { 0..0 } else { 0..n };
    let step = n.div_ceil(probes.max(1)).max(1);
    let picks = inner.lumps.iter().map(|&k| k as usize).chain(spread.step_by(step));
    for k in picks {
        let t = &inner.tris[k];
        let nrm = (t[1] - t[0]).cross(t[2] - t[0]);
        let len = nrm.length();
        if len <= 0.0 || !len.is_finite() {
            continue;
        }
        let edge = (t[1] - t[0]).length().max((t[2] - t[1]).length()).max((t[0] - t[2]).length());
        let p = (t[0] + t[1] + t[2]) * (1.0 / 3.0) - nrm * (1.0 / len) * (edge * 1e-3).max(1e-6);
        // Outside the other body's box the winding number is 0: skip the
        // O(n) sum, which keeps a soup of many small pieces cheap.
        if box_distance((p, p), (outer.nodes[0].lo, outer.nodes[0].hi)) > 0.0 {
            continue;
        }
        if outer.winding(p).abs() > 0.5 {
            return true;
        }
    }
    false
}

// ---------------------------------------------------------------------------
// The near map
// ---------------------------------------------------------------------------

/// Every triangle pair within `below`, reduced to one entry per cell of
/// `spacing`, ascending, capped at `limit`.
pub fn near_trees(a: &Tree, b: &Tree, below: f64, spacing: f64, limit: usize) -> NearMap {
    if a.is_empty() || b.is_empty() || !(below > 0.0) {
        return NearMap::default();
    }
    let cell = if spacing > 0.0 { spacing } else { below.max(1e-3) };
    let mut cells: BTreeMap<(i64, i64, i64), NearPair> = BTreeMap::new();
    let mut stack = vec![(0u32, 0u32)];
    while let Some((na, nb)) = stack.pop() {
        let (x, y) = (a.nodes[na as usize], b.nodes[nb as usize]);
        if box_distance((x.lo, x.hi), (y.lo, y.hi)) >= below {
            continue;
        }
        if x.is_leaf() && y.is_leaf() {
            for &ta in &a.order[x.start as usize..(x.start + x.count) as usize] {
                for &tb in &b.order[y.start as usize..(y.start + y.count) as usize] {
                    if box_distance(a.boxes[ta as usize], b.boxes[tb as usize]) >= below
                        || projected_gap(&a.tris[ta as usize], &b.tris[tb as usize]) >= below
                    {
                        continue;
                    }
                    let (d, pa, pb, _) = tri_tri(&a.tris[ta as usize], &b.tris[tb as usize]);
                    if d >= below {
                        continue;
                    }
                    let mid = (pa + pb) * 0.5;
                    let key = ((mid.x / cell).floor() as i64, (mid.y / cell).floor() as i64, (mid.z / cell).floor() as i64);
                    let entry = NearPair { distance: d, a_point: pa, b_point: pb, face_a: a.face_of(ta), face_b: b.face_of(tb) };
                    match cells.get_mut(&key) {
                        Some(e) if e.distance <= d => {}
                        Some(e) => *e = entry,
                        None => {
                            cells.insert(key, entry);
                        }
                    }
                }
            }
            continue;
        }
        let split_a = !x.is_leaf() && (y.is_leaf() || x.extent() >= y.extent());
        if split_a {
            stack.push((x.left, nb));
            stack.push((x.right, nb));
        } else {
            stack.push((na, y.left));
            stack.push((na, y.right));
        }
    }
    // One entry per cell, then one per `spacing`-radius region: a cell wall
    // that slices a ring leaves the cell on one side holding a second-best
    // pair at a larger distance, so a candidate within `spacing` of a
    // closer one already kept is the same region and is dropped. The cells
    // bound the work; the pass keeps the closest of each neighbourhood.
    let mut candidates: Vec<NearPair> = cells.into_values().collect();
    candidates.sort_by(|p, q| p.distance.partial_cmp(&q.distance).unwrap_or(Ordering::Equal));
    let r2 = cell * cell;
    let mut pairs: Vec<NearPair> = Vec::new();
    for c in candidates {
        let mid = (c.a_point + c.b_point) * 0.5;
        let taken = pairs.iter().any(|k| {
            let d = (k.a_point + k.b_point) * 0.5 - mid;
            d.dot(d) < r2
        });
        if !taken {
            pairs.push(c);
        }
    }
    let regions = pairs.len();
    pairs.truncate(limit);
    NearMap { pairs, regions }
}

// ---------------------------------------------------------------------------
// Triangle–triangle distance
// ---------------------------------------------------------------------------

/// Distance between two triangles with the closest points, and whether they
/// cross. Zero for triangles that touch or cross.
///
/// Two triangles that do not cross have their closest points on a vertex and
/// the other triangle, or on an edge of each (Ericson, *Real-Time Collision
/// Detection* §5.1), so those fifteen candidates are enumerated; a triangle
/// whose edge passes through the other is a crossing at distance zero, which
/// the candidates alone would miss when the crossing is interior to both.
pub fn tri_tri(a: &[Vec3; 3], b: &[Vec3; 3]) -> (f64, Vec3, Vec3, bool) {
    let mut best = f64::INFINITY;
    let mut pa = a[0];
    let mut pb = b[0];
    let mut consider = |p: Vec3, q: Vec3| {
        let d = (p - q).length();
        if d < best {
            best = d;
            pa = p;
            pb = q;
        }
    };
    for &v in a {
        consider(v, closest_on_tri(v, b));
    }
    for &v in b {
        consider(closest_on_tri(v, a), v);
    }
    let edges = |t: &[Vec3; 3]| [(t[0], t[1]), (t[1], t[2]), (t[2], t[0])];
    for (p1, q1) in edges(a) {
        for (p2, q2) in edges(b) {
            let (c1, c2) = seg_seg(p1, q1, p2, q2);
            consider(c1, c2);
        }
    }
    // Two triangles can only pass through each other when each reaches past
    // the other's plane, so that cheap test gates the six segment tests.
    let straddling = straddle(a, b);
    if best == 0.0 {
        return (0.0, pa, pb, straddling);
    }
    if straddling {
        for (p, q) in edges(a) {
            if let Some(x) = seg_hits_tri(p, q, b) {
                return (0.0, x, x, true);
            }
        }
        for (p, q) in edges(b) {
            if let Some(x) = seg_hits_tri(p, q, a) {
                return (0.0, x, x, true);
            }
        }
    }
    (best, pa, pb, false)
}

/// Do the two triangles each reach strictly past the other's plane? With a
/// zero distance that is a crossing rather than a touch. The band is scaled
/// to the triangles, as [`crate::solid::pair_defects`]'s is to the mesh.
fn straddle(a: &[Vec3; 3], b: &[Vec3; 3]) -> bool {
    let plane = |t: &[Vec3; 3]| {
        let n = (t[1] - t[0]).cross(t[2] - t[0]);
        let l = n.length();
        if l > 0.0 { Some(n * (1.0 / l)) } else { None }
    };
    let (Some(na), Some(nb)) = (plane(a), plane(b)) else { return false };
    let scale = a.iter().chain(b).map(|p| p.length()).fold(0.0f64, f64::max);
    let eps = (scale * 1e-9).max(1e-9);
    let both_sides = |n: Vec3, origin: Vec3, t: &[Vec3; 3]| {
        let d = [n.dot(t[0] - origin), n.dot(t[1] - origin), n.dot(t[2] - origin)];
        d.iter().any(|v| *v > eps) && d.iter().any(|v| *v < -eps)
    };
    both_sides(na, a[0], b) && both_sides(nb, b[0], a)
}

/// The point of triangle `t` closest to `p` (Ericson §5.1.5): the Voronoi
/// region of `p` against the triangle decides whether the answer is a
/// vertex, a point on an edge or the perpendicular foot.
pub fn closest_on_tri(p: Vec3, t: &[Vec3; 3]) -> Vec3 {
    let (a, b, c) = (t[0], t[1], t[2]);
    let ab = b - a;
    let ac = c - a;
    let ap = p - a;
    let d1 = ab.dot(ap);
    let d2 = ac.dot(ap);
    if d1 <= 0.0 && d2 <= 0.0 {
        return a;
    }
    let bp = p - b;
    let d3 = ab.dot(bp);
    let d4 = ac.dot(bp);
    if d3 >= 0.0 && d4 <= d3 {
        return b;
    }
    let vc = d1 * d4 - d3 * d2;
    if vc <= 0.0 && d1 >= 0.0 && d3 <= 0.0 {
        let denom = d1 - d3;
        let v = if denom != 0.0 { d1 / denom } else { 0.0 };
        return a + ab * v;
    }
    let cp = p - c;
    let d5 = ab.dot(cp);
    let d6 = ac.dot(cp);
    if d6 >= 0.0 && d5 <= d6 {
        return c;
    }
    let vb = d5 * d2 - d1 * d6;
    if vb <= 0.0 && d2 >= 0.0 && d6 <= 0.0 {
        let denom = d2 - d6;
        let w = if denom != 0.0 { d2 / denom } else { 0.0 };
        return a + ac * w;
    }
    let va = d3 * d6 - d5 * d4;
    if va <= 0.0 && (d4 - d3) >= 0.0 && (d5 - d6) >= 0.0 {
        let denom = (d4 - d3) + (d5 - d6);
        let w = if denom != 0.0 { (d4 - d3) / denom } else { 0.0 };
        return b + (c - b) * w;
    }
    let denom = va + vb + vc;
    if denom == 0.0 || !denom.is_finite() {
        // A degenerate triangle: the nearest of its edges.
        let (mut best, mut at) = (f64::INFINITY, a);
        for (u, v) in [(a, b), (b, c), (c, a)] {
            let q = closest_on_seg(p, u, v);
            let d = (p - q).length();
            if d < best {
                best = d;
                at = q;
            }
        }
        return at;
    }
    let v = vb / denom;
    let w = vc / denom;
    a + ab * v + ac * w
}

fn closest_on_seg(p: Vec3, a: Vec3, b: Vec3) -> Vec3 {
    let ab = b - a;
    let l2 = ab.dot(ab);
    if l2 <= 0.0 {
        return a;
    }
    let t = ((p - a).dot(ab) / l2).clamp(0.0, 1.0);
    a + ab * t
}

/// Closest points of two segments (Ericson §5.1.9).
pub fn seg_seg(p1: Vec3, q1: Vec3, p2: Vec3, q2: Vec3) -> (Vec3, Vec3) {
    let d1 = q1 - p1;
    let d2 = q2 - p2;
    let r = p1 - p2;
    let a = d1.dot(d1);
    let e = d2.dot(d2);
    let f = d2.dot(r);
    let tiny = 1e-18;
    let (s, t);
    if a <= tiny && e <= tiny {
        return (p1, p2);
    }
    if a <= tiny {
        s = 0.0;
        t = (f / e).clamp(0.0, 1.0);
    } else {
        let c = d1.dot(r);
        if e <= tiny {
            t = 0.0;
            s = (-c / a).clamp(0.0, 1.0);
        } else {
            let b = d1.dot(d2);
            let denom = a * e - b * b;
            let mut s0 = if denom != 0.0 { ((b * f - c * e) / denom).clamp(0.0, 1.0) } else { 0.0 };
            let mut t0 = (b * s0 + f) / e;
            if t0 < 0.0 {
                t0 = 0.0;
                s0 = (-c / a).clamp(0.0, 1.0);
            } else if t0 > 1.0 {
                t0 = 1.0;
                s0 = ((b - c) / a).clamp(0.0, 1.0);
            }
            s = s0;
            t = t0;
        }
    }
    (p1 + d1 * s, p2 + d2 * t)
}

/// Where segment `p→q` passes through triangle `t` (Möller–Trumbore, with the
/// parameter held to the segment). A segment parallel to the plane is not a
/// crossing here: coplanar contact is found by the vertex and edge candidates.
pub fn seg_hits_tri(p: Vec3, q: Vec3, t: &[Vec3; 3]) -> Option<Vec3> {
    let dir = q - p;
    let e1 = t[1] - t[0];
    let e2 = t[2] - t[0];
    let h = dir.cross(e2);
    let det = e1.dot(h);
    let scale = e1.length() * e2.length() * dir.length();
    if det.abs() <= scale * 1e-12 {
        return None;
    }
    let inv = 1.0 / det;
    let s = p - t[0];
    let u = s.dot(h) * inv;
    if !(0.0..=1.0).contains(&u) {
        return None;
    }
    let qv = s.cross(e1);
    let v = dir.dot(qv) * inv;
    if v < 0.0 || u + v > 1.0 {
        return None;
    }
    let k = e2.dot(qv) * inv;
    if !(0.0..=1.0).contains(&k) {
        return None;
    }
    Some(p + dir * k)
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::solid::fixtures::{box_between, box_mesh};

    fn v(x: f64, y: f64, z: f64) -> Vec3 {
        Vec3::new(x, y, z)
    }

    /// A closed polygonal tube from `r_in` to `r_out`, `n` sides, height `h`,
    /// so a pin in a bore has an exactly known clearance: for two regular
    /// n-gons in phase the nearest approach is a vertex of the inner one to
    /// the edge of the outer one beside it, whose line lies `cos(π/n)` of the
    /// radial gap away: `(r_out − r_in)·cos(π/n)`. `slices` divides the
    /// walls along the height, the way the tessellator divides a spline
    /// patch in both directions, so a 100k-triangle wall is a grid
    /// of sensible triangles rather than slivers.
    fn tube_grid(r_in: f64, r_out: f64, n: usize, slices: usize, h: f64) -> Mesh {
        use crate::solid::fixtures::push_tri;
        let mut m = Mesh::default();
        let ring = |r: f64, k: usize, z: f64| {
            let a = 2.0 * std::f64::consts::PI * k as f64 / n as f64;
            v(r * fmath::cos(a), r * fmath::sin(a), z)
        };
        let dz = h / slices as f64;
        for k in 0..n {
            let j = (k + 1) % n;
            for s in 0..slices {
                let (z0, z1) = (s as f64 * dz, (s + 1) as f64 * dz);
                // outer wall, outward
                push_tri(&mut m, ring(r_out, k, z0), ring(r_out, j, z0), ring(r_out, j, z1), 0);
                push_tri(&mut m, ring(r_out, k, z0), ring(r_out, j, z1), ring(r_out, k, z1), 0);
                // inner wall, facing the axis
                push_tri(&mut m, ring(r_in, k, z0), ring(r_in, j, z1), ring(r_in, j, z0), 1);
                push_tri(&mut m, ring(r_in, k, z0), ring(r_in, k, z1), ring(r_in, j, z1), 1);
            }
            // top, up
            push_tri(&mut m, ring(r_in, k, h), ring(r_out, k, h), ring(r_out, j, h), 2);
            push_tri(&mut m, ring(r_in, k, h), ring(r_out, j, h), ring(r_in, j, h), 2);
            // bottom, down
            push_tri(&mut m, ring(r_in, k, 0.0), ring(r_out, j, 0.0), ring(r_out, k, 0.0), 3);
            push_tri(&mut m, ring(r_in, k, 0.0), ring(r_in, j, 0.0), ring(r_out, j, 0.0), 3);
        }
        m
    }

    fn tube(r_in: f64, r_out: f64, n: usize, h: f64) -> Mesh {
        tube_grid(r_in, r_out, n, 1, h)
    }

    /// A solid polygonal cylinder, `n` sides, its wall in `slices` bands.
    fn pin_grid(r: f64, n: usize, slices: usize, h: f64) -> Mesh {
        use crate::solid::fixtures::push_tri;
        let mut m = Mesh::default();
        let ring = |k: usize, z: f64| {
            let a = 2.0 * std::f64::consts::PI * k as f64 / n as f64;
            v(r * fmath::cos(a), r * fmath::sin(a), z)
        };
        let dz = h / slices as f64;
        for k in 0..n {
            let j = (k + 1) % n;
            for s in 0..slices {
                let (z0, z1) = (s as f64 * dz, (s + 1) as f64 * dz);
                push_tri(&mut m, ring(k, z0), ring(j, z0), ring(j, z1), 0);
                push_tri(&mut m, ring(k, z0), ring(j, z1), ring(k, z1), 0);
            }
            push_tri(&mut m, v(0.0, 0.0, h), ring(k, h), ring(j, h), 1);
            push_tri(&mut m, v(0.0, 0.0, 0.0), ring(j, 0.0), ring(k, 0.0), 2);
        }
        m
    }

    fn pin(r: f64, n: usize, h: f64) -> Mesh {
        pin_grid(r, n, 1, h)
    }

    #[test]
    fn two_cubes_a_gap_apart_measure_the_gap_on_the_facing_faces() {
        // 10 mm cubes, the second starting 0.4 mm past the first along X.
        let a = box_mesh(10.0, 10.0, 10.0);
        let b = box_between(v(10.4, 0.0, 0.0), v(20.4, 10.0, 10.0));
        let c = distance(&a, &b, 0.05).unwrap();
        // Mesh positions are f32, so 10.4 is 10.3999996: within a micron.
        assert!((c.distance - 0.4).abs() < 1e-5, "{c:?}");
        assert!(!c.overlap);
        assert!(!c.touching());
        assert!((c.a_point.x - 10.0).abs() < 1e-5 && (c.b_point.x - 10.4).abs() < 1e-5, "{c:?}");
        // The faces are the +X face of a and the -X face of b — the walls
        // that face each other, not one of the four faces whose edges lie
        // on those walls at the same distance.
        let (na, _) = crate::solid::tri_normal_area(&crate::solid::tri(&a, c.tri_a as usize));
        let (nb, _) = crate::solid::tri_normal_area(&crate::solid::tri(&b, c.tri_b as usize));
        assert!(na.x > 0.99, "a's +X wall, got normal {na:?}");
        assert!(nb.x < -0.99, "b's -X wall, got normal {nb:?}");
        assert_eq!((c.face_a, c.face_b), (a.face_ids[c.tri_a as usize], b.face_ids[c.tri_b as usize]));
        assert_eq!(c.tolerance, 0.05);
    }

    #[test]
    fn touching_cubes_are_zero_apart_and_do_not_overlap() {
        let a = box_mesh(10.0, 10.0, 10.0);
        let b = box_between(v(10.0, 0.0, 0.0), v(20.0, 10.0, 10.0));
        let c = distance(&a, &b, 0.05).unwrap();
        assert_eq!(c.distance, 0.0);
        assert!(!c.overlap, "sharing a face is touching, not overlapping: {c:?}");
        assert!(c.touching());
    }

    #[test]
    fn overlapping_cubes_are_zero_apart_and_overlap() {
        let a = box_mesh(10.0, 10.0, 10.0);
        let b = box_between(v(8.0, 2.0, 2.0), v(18.0, 8.0, 8.0));
        let c = distance(&a, &b, 0.05).unwrap();
        assert_eq!(c.distance, 0.0);
        assert!(c.overlap, "{c:?}");
    }

    #[test]
    fn a_cube_inside_another_overlaps_even_with_a_gap_to_its_surface() {
        // Fully embedded: no surface contact, but shared material. The
        // distance is 0, as for any overlap — a clearance check written as
        // `distance >= gap` must not pass for a part buried in another — and
        // the two points are still the closest pair of surface points, the
        // gap to the enclosing surface, 5 mm apart.
        let outer = box_mesh(20.0, 20.0, 20.0);
        let inner = box_between(v(5.0, 5.0, 5.0), v(15.0, 15.0, 15.0));
        let c = distance(&outer, &inner, 0.05).unwrap();
        assert_eq!(c.distance, 0.0, "{c:?}");
        assert!(c.overlap, "{c:?}");
        assert!(!c.touching(), "{c:?}");
        assert!(((c.a_point - c.b_point).length() - 5.0).abs() < 1e-9, "{c:?}");
        let c = distance(&inner, &outer, 0.05).unwrap();
        assert!(c.overlap, "symmetric: {c:?}");
        assert_eq!(c.distance, 0.0, "symmetric: {c:?}");
    }

    /// A body of two lumps, one 40 mm clear of a big box and the other buried
    /// in it. The surfaces never meet, so overlap is decided by probes; one
    /// probe per body used to land on whichever lump's triangle came first,
    /// and with the clear lump first the buried one was never asked about.
    #[test]
    fn a_lump_buried_in_the_other_body_overlaps_whatever_the_lump_order() {
        use crate::solid::fixtures::append;
        let big = box_between(v(40.0, -10.0, -10.0), v(70.0, 20.0, 20.0));
        let near = box_mesh(10.0, 10.0, 10.0);
        let far = box_between(v(50.0, 0.0, 0.0), v(60.0, 10.0, 10.0));
        for (first, second) in [(&near, &far), (&far, &near)] {
            let mut two = first.clone();
            append(&mut two, second);
            let c = distance(&two, &big, 0.05).unwrap();
            assert!(c.overlap, "{c:?}");
            let c = distance(&big, &two, 0.05).unwrap();
            assert!(c.overlap, "swapped: {c:?}");
        }
        // And two lumps both clear of the box are still clear.
        let mut two = near.clone();
        append(&mut two, &box_between(v(0.0, 20.0, 0.0), v(10.0, 30.0, 10.0)));
        let c = distance(&two, &big, 0.05).unwrap();
        assert!(!c.overlap && (c.distance - 30.0).abs() < 1e-9, "{c:?}");
    }

    #[test]
    fn a_pin_in_a_cavity_is_not_an_overlap() {
        // A tube around a pin: the pin sits in the bore, material nowhere
        // shared, and the winding number in the bore is zero.
        let bore = tube(5.15, 10.0, 64, 10.0);
        let p = pin(5.0, 64, 10.0);
        let c = distance(&p, &bore, 0.05).unwrap();
        assert!(!c.overlap, "{c:?}");
        let expect = 0.15 * fmath::cos(std::f64::consts::PI / 64.0);
        assert!((c.distance - expect).abs() < 1e-5, "got {} want {expect}", c.distance);
        assert_eq!(c.face_a, 0, "the pin's side");
        assert_eq!(c.face_b, 1, "the bore wall");
    }

    #[test]
    fn the_near_map_returns_the_whole_ring_ascending_and_capped() {
        let bore = tube(5.15, 10.0, 64, 10.0);
        let p = pin(5.0, 64, 10.0);
        let m = near(&p, &bore, 0.3, 1.0, 1000);
        // 64 vertices of the pin, each nearest an outer edge: one region per
        // ~1 mm cell around a 31 mm circumference, over the full height.
        assert!(m.regions >= 32, "the ring: {} regions", m.regions);
        assert!(!m.capped());
        assert!(m.pairs.windows(2).all(|w| w[0].distance <= w[1].distance), "ascending");
        assert!(m.pairs.iter().all(|p| p.distance < 0.3));
        // Every side of the ring is represented.
        let angles: Vec<f64> = m.pairs.iter().map(|p| fmath::atan2(p.a_point.y, p.a_point.x)).collect();
        assert!(angles.iter().any(|a| *a > 2.0) && angles.iter().any(|a| *a < -2.0), "{angles:?}");
        let capped = near(&p, &bore, 0.3, 1.0, 8);
        assert_eq!(capped.pairs.len(), 8);
        assert!(capped.capped());
        assert_eq!(capped.regions, m.regions);
        // Nothing within 0.1: the map is empty and says so.
        let none = near(&p, &bore, 0.1, 1.0, 8);
        assert_eq!(none.regions, 0);
    }

    #[test]
    fn triangle_distance_covers_every_configuration() {
        let t = [v(0.0, 0.0, 0.0), v(4.0, 0.0, 0.0), v(0.0, 4.0, 0.0)];
        // Vertex above the interior: the perpendicular.
        let u = [v(1.0, 1.0, 2.0), v(2.0, 1.0, 3.0), v(1.0, 2.0, 3.0)];
        let (d, pa, pb, x) = tri_tri(&t, &u);
        assert!((d - 2.0).abs() < 1e-12 && !x, "{d} {pa:?} {pb:?}");
        // Edge over edge, neither vertex nearest.
        let u = [v(-1.0, 5.0, 1.0), v(5.0, -1.0, 1.0), v(5.0, 5.0, 3.0)];
        let (d, _, _, _) = tri_tri(&t, &u);
        assert!((d - 1.0).abs() < 1e-12, "{d}");
        // Passing through: an edge of u pierces t's interior.
        let u = [v(1.0, 1.0, -1.0), v(1.0, 1.0, 1.0), v(1.5, 1.5, 1.0)];
        let (d, _, _, x) = tri_tri(&t, &u);
        assert_eq!(d, 0.0);
        assert!(x, "a crossing");
        // Resting on: a vertex of u on t's face is a touch, not a crossing.
        let u = [v(1.0, 1.0, 0.0), v(1.0, 1.0, 1.0), v(1.5, 1.5, 1.0)];
        let (d, _, _, x) = tri_tri(&t, &u);
        assert_eq!(d, 0.0);
        assert!(!x);
        // Coplanar overlap: distance zero through a vertex inside.
        let u = [v(1.0, 1.0, 0.0), v(9.0, 1.0, 0.0), v(1.0, 9.0, 0.0)];
        let (d, _, _, _) = tri_tri(&t, &u);
        assert_eq!(d, 0.0);
    }

    #[test]
    fn the_tree_and_the_answer_are_deterministic() {
        let a = pin(5.0, 128, 20.0);
        let b = tube(5.2, 9.0, 96, 20.0);
        let first = distance(&a, &b, 0.05).unwrap();
        for _ in 0..3 {
            assert_eq!(distance(&a, &b, 0.05).unwrap(), first);
        }
    }

    #[test]
    fn empty_meshes_have_no_distance() {
        assert!(distance(&Mesh::default(), &box_mesh(1.0, 1.0, 1.0), 0.05).is_none());
        assert_eq!(near(&Mesh::default(), &box_mesh(1.0, 1.0, 1.0), 1.0, 1.0, 8).regions, 0);
    }

    /// Two 100k-triangle bodies, twice: a ring of contact and a point of
    /// contact. The bounds are generous because `bazel test` runs a debug
    /// build.
    #[test]
    fn two_hundred_thousand_triangle_bodies_are_quick() {
        // A pin in a bore, both walls tessellated as grids: 1000 chords by 49
        // bands plus the caps, and 500 chords by 49 bands on both walls of
        // the tube plus its rings. A ring of contact all the way round, which
        // is the planetary's case and the hardest for a closest-pair search:
        // every pin vertex column is at the minimum against the bore edge in
        // front of it, so no bound can prune those pairs and every one is
        // tested exactly.
        let a = pin_grid(5.0, 1000, 49, 20.0);
        let b = tube_grid(5.2, 9.0, 500, 49, 20.0);
        assert_eq!(a.triangle_count(), 100_000);
        assert_eq!(b.triangle_count(), 100_000);
        let t0 = std::time::Instant::now();
        let c = distance(&a, &b, 0.05).unwrap();
        let ring_ms = t0.elapsed().as_secs_f64() * 1e3;
        assert!(!c.overlap);
        let expect = 5.2 * fmath::cos(std::f64::consts::PI / 500.0) - 5.0;
        assert!((c.distance - expect).abs() < 1e-5, "{} want {expect}", c.distance);

        // Two tori side by side, 0.4 mm apart at one point of their equators.
        let t = crate::solid::fixtures::torus_mesh(20.0, 5.0, 250, 200);
        assert_eq!(t.triangle_count(), 100_000);
        let mut u = t.clone();
        for p in u.positions.chunks_exact_mut(3) {
            p[0] += 50.4;
        }
        let t1 = std::time::Instant::now();
        let d = distance(&t, &u, 0.05).unwrap();
        let point_ms = t1.elapsed().as_secs_f64() * 1e3;
        assert!(!d.overlap);
        assert!((d.distance - 0.4).abs() < 1e-3, "{}", d.distance);
        eprintln!("clearance: two 100k-triangle bodies: ring contact {ring_ms:.1} ms, point contact {point_ms:.1} ms");
        // A bound an order of magnitude above a quiet debug run (ring ~0.6 s
        // opt / ~6 s debug; point ~60 ms / ~0.6 s): it catches the traversal
        // going quadratic, not a loaded machine. The times are printed for
        // the record.
        assert!(ring_ms < 120_000.0 && point_ms < 30_000.0, "{ring_ms} / {point_ms} ms: the closest-pair traversal has regressed by more than an order of magnitude");
    }
}
