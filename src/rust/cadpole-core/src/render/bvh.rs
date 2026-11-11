//! Binned-SAH bounding volume hierarchy over every triangle in the scene,
//! built once per path-traced render.
//!
//! The rasterizer never needs this (it walks triangles once, in screen
//! space); the path tracer asks "what does this ray hit" millions of times,
//! so the whole scene goes into one flat, cache-friendly tree.
//!
//! Construction is a plain sequential top-down sweep — 16 centroid bins on
//! each of the three axes, surface-area-heuristic cost, in-place partition —
//! so the tree is a pure function of the triangle list and the same scene
//! always gives the same tree (and therefore the same floating-point
//! traversal results) on every machine and every thread count. The binary
//! tree that produces is then collapsed into four-wide nodes ([`Node4`]),
//! which is what traversal walks: one SSE pass tests a node's four boxes.
//!
//! Traversal is iterative with an explicit stack and front-to-back child
//! ordering, plus [`Bvh::occluded`] which stops at the first hit for shadow
//! rays and [`Bvh::shadow_hits`] which gathers every see-through crossing
//! of one in a single walk.

use super::{Item, cross, dot, sub};


/// Triangles per leaf above which a split is attempted.
const LEAF_MAX: usize = 4;
/// Centroid bins per axis for the SAH sweep.
const BINS: usize = 16;
/// Depth at which the builder stops splitting whatever the heuristic says;
/// a 16-bin SAH tree over a few million triangles never comes close.
const MAX_DEPTH: usize = 62;
/// Traversal stack entries. A four-wide node can leave three children
/// waiting, and the collapsed tree is never deeper than the binary one, so
/// `3 * MAX_DEPTH + 1` is the most a traversal can ever hold.
const MAX_STACK: usize = 3 * MAX_DEPTH + 2;

/// One triangle in the form the intersector wants: an origin and two edge
/// vectors, plus where it came from.
#[derive(Clone, Copy, Debug)]
pub struct Tri {
    pub p0: [f32; 3],
    /// `p1 - p0`
    pub e1: [f32; 3],
    /// `p2 - p0`
    pub e2: [f32; 3],
    /// Index into the scene's item list.
    pub item: u32,
    /// Triangle index within that item's mesh (indexes `face_ids`).
    pub tri: u32,
}

impl Tri {
    #[inline]
    fn centroid(&self) -> [f32; 3] {
        [
            self.p0[0] + (self.e1[0] + self.e2[0]) * (1.0 / 3.0),
            self.p0[1] + (self.e1[1] + self.e2[1]) * (1.0 / 3.0),
            self.p0[2] + (self.e1[2] + self.e2[2]) * (1.0 / 3.0),
        ]
    }

    #[inline]
    fn bounds(&self) -> Aabb {
        let p1 = [self.p0[0] + self.e1[0], self.p0[1] + self.e1[1], self.p0[2] + self.e1[2]];
        let p2 = [self.p0[0] + self.e2[0], self.p0[1] + self.e2[1], self.p0[2] + self.e2[2]];
        let mut b = Aabb::empty();
        b.add(self.p0);
        b.add(p1);
        b.add(p2);
        b
    }

    /// Geometric (unnormalized) normal, `e1 x e2`.
    #[inline]
    pub fn geometric_normal(&self) -> [f32; 3] {
        cross(self.e1, self.e2)
    }

    /// Area of the triangle.
    #[inline]
    pub fn area(&self) -> f32 {
        let n = self.geometric_normal();
        dot(n, n).sqrt() * 0.5
    }

    /// Barycentric point: `p0 + u * e1 + v * e2`.
    #[inline]
    pub fn point(&self, u: f32, v: f32) -> [f32; 3] {
        [
            self.p0[0] + u * self.e1[0] + v * self.e2[0],
            self.p0[1] + u * self.e1[1] + v * self.e2[1],
            self.p0[2] + u * self.e1[2] + v * self.e2[2],
        ]
    }
}

/// What a ray hit: distance, barycentrics and the triangle's slot.
#[derive(Clone, Copy, Debug)]
pub struct Hit {
    pub t: f32,
    pub u: f32,
    pub v: f32,
    /// Slot index; [`Bvh::tri`] turns it back into the triangle.
    pub tri: u32,
}

/// What one traversal of [`Bvh::shadow_hits`] found out about a shadow ray.
#[derive(Clone, Copy, Debug, PartialEq)]
pub enum ShadowHits {
    /// The nearest triangle of an opaque item lies at this distance. The
    /// caller's buffer holds the see-through crossings no farther than it;
    /// nothing beyond it was looked at.
    Blocked(f32),
    /// Only see-through triangles were crossed; the caller's buffer holds
    /// every one of them, in traversal order, not by distance.
    Through,
    /// More see-through crossings than the caller allowed for; the caller
    /// has to find them some other way.
    Overflow,
}

#[derive(Clone, Copy, Debug)]
pub struct Aabb {
    pub lo: [f32; 3],
    pub hi: [f32; 3],
}

impl Aabb {
    #[inline]
    pub fn empty() -> Aabb {
        Aabb { lo: [f32::INFINITY; 3], hi: [f32::NEG_INFINITY; 3] }
    }

    #[inline]
    fn add(&mut self, p: [f32; 3]) {
        for k in 0..3 {
            if p[k] < self.lo[k] {
                self.lo[k] = p[k];
            }
            if p[k] > self.hi[k] {
                self.hi[k] = p[k];
            }
        }
    }

    #[inline]
    fn union(&mut self, o: &Aabb) {
        for k in 0..3 {
            if o.lo[k] < self.lo[k] {
                self.lo[k] = o.lo[k];
            }
            if o.hi[k] > self.hi[k] {
                self.hi[k] = o.hi[k];
            }
        }
    }

    /// Surface area; 0 for an empty box.
    #[inline]
    fn area(&self) -> f32 {
        let d = [self.hi[0] - self.lo[0], self.hi[1] - self.lo[1], self.hi[2] - self.lo[2]];
        if d[0] < 0.0 || d[1] < 0.0 || d[2] < 0.0 {
            return 0.0;
        }
        2.0 * (d[0] * d[1] + d[1] * d[2] + d[2] * d[0])
    }
}

/// A node of the binary tree the builder produces. `count == 0` marks an
/// interior node whose children are at `first` and `first + 1`; otherwise it
/// is a leaf covering `tris[first .. first + count]`. Traversal never sees
/// one of these: the binary tree is collapsed into [`Node4`]s once built.
#[derive(Clone, Copy)]
#[repr(C)]
struct Node {
    lo: [f32; 3],
    hi: [f32; 3],
    first: u32,
    count: u32,
}

/// Four children of one node, laid out so one SSE register holds one
/// coordinate of all four boxes: `lo[axis][slot]`.
///
/// Why four wide. A ray tests a node's children to decide where to go next;
/// with two children that is two box tests and a branch per level, and the
/// impeller's tree is 26 levels deep. With four, one pass over three axes
/// tests all four boxes in twelve SSE multiplies — the same instruction
/// count the two-wide test spent on two — and the tree is half as deep, so a
/// ray takes half as many of the unpredictable "which child" branches, which
/// on this workload is most of what a traversal step costs. Eight wide
/// would want AVX and, at this renderer's scene sizes, mostly cull empty
/// lanes.
///
/// `child[i]` is a `Node4` index when `count[i] == 0` and the index of a
/// leaf's first [`Tri4`] block when it is not; slots past `valid` are
/// unused and their boxes are zero. 128 bytes: two cache lines, aligned, so
/// a node is never three.
#[derive(Clone, Copy)]
#[repr(C, align(64))]
struct Node4 {
    lo: [[f32; 4]; 3],
    hi: [[f32; 4]; 3],
    child: [u32; 4],
    count: [u32; 4],
    /// Bit `i` set when slot `i` holds a child.
    valid: u32,
    _pad: [u32; 3],
}

const EMPTY_NODE4: Node4 = Node4 { lo: [[0.0; 4]; 3], hi: [[0.0; 4]; 3], child: [0; 4], count: [0; 4], valid: 0, _pad: [0; 3] };

/// Four of a leaf's triangles, one per lane: `p0[axis][lane]`, and the two
/// edge vectors likewise, so Möller–Trumbore runs on all four in one pass
/// of SSE arithmetic. A leaf is one of these for every four triangles it
/// holds (usually one: [`LEAF_MAX`] is four, and only a node no split can
/// improve stays whole above that), consecutive in `Bvh::leaves`. A lane
/// past the leaf's count is all zeros, whose determinant is zero, which the
/// test rejects the way it rejects any degenerate triangle. 144 bytes,
/// sixteen-aligned for the loads.
#[derive(Clone, Copy)]
#[repr(C, align(16))]
struct Tri4 {
    p0: [[f32; 4]; 3],
    e1: [[f32; 4]; 3],
    e2: [[f32; 4]; 3],
}

const EMPTY_TRI4: Tri4 = Tri4 { p0: [[0.0; 4]; 3], e1: [[0.0; 4]; 3], e2: [[0.0; 4]; 3] };

/// Where a triangle slot came from. A padding lane has `item == u32::MAX`.
#[derive(Clone, Copy, Debug)]
struct TriRef {
    item: u32,
    tri: u32,
}

/// Build/traversal statistics, reported by `cadpole render --stats`.
#[derive(Clone, Copy, Debug, Default)]
pub struct BvhStats {
    pub triangles: usize,
    pub nodes: usize,
    pub leaves: usize,
    pub max_depth: usize,
    pub build_ms: f64,
}

/// The tree. Triangles live in `leaves`, four to a block in the order the
/// build left them, and are addressed by *slot*: `Hit::tri` is
/// `4 * block + lane`, and [`Bvh::tri`] gathers the triangle back out of its
/// block. A leaf's triangles fill consecutive blocks, so its slots are the
/// contiguous range `4 * child .. 4 * child + count`; the slots after that
/// in its last block are padding, which [`Bvh::triangles`] skips.
pub struct Bvh {
    nodes: Vec<Node4>,
    leaves: Vec<Tri4>,
    /// One per slot, `4 * leaves.len()` of them.
    slots: Vec<TriRef>,
    pub stats: BvhStats,
}

impl Bvh {
    /// Number of triangle slots, padding included: the bound on `Hit::tri`.
    #[inline]
    pub fn len(&self) -> usize {
        self.slots.len()
    }

    #[inline]
    pub fn is_empty(&self) -> bool {
        self.stats.triangles == 0
    }

    /// The item the triangle in slot `i` belongs to, without rebuilding the
    /// triangle: what the `_where` traversals' predicates look up. A padding
    /// slot answers `u32::MAX`, but no traversal asks about one: its zero
    /// edges never pass the lane test.
    #[inline]
    pub fn item_of(&self, i: u32) -> u32 {
        self.slots[i as usize].item
    }

    /// The triangle in slot `i`, gathered from its block. A padding slot
    /// comes back with `item == u32::MAX` and zero edges.
    #[inline]
    pub fn tri(&self, i: u32) -> Tri {
        let (b, l) = ((i >> 2) as usize, (i & 3) as usize);
        let t = &self.leaves[b];
        let r = self.slots[i as usize];
        Tri {
            p0: [t.p0[0][l], t.p0[1][l], t.p0[2][l]],
            e1: [t.e1[0][l], t.e1[1][l], t.e1[2][l]],
            e2: [t.e2[0][l], t.e2[1][l], t.e2[2][l]],
            item: r.item,
            tri: r.tri,
        }
    }

    /// Every real triangle with its slot index, in slot order.
    pub fn triangles(&self) -> impl Iterator<Item = (u32, Tri)> + '_ {
        (0..self.slots.len() as u32).filter(|&i| self.slots[i as usize].item != u32::MAX).map(|i| (i, self.tri(i)))
    }

    /// Collect every triangle of every item and build the tree. Items are
    /// visited in order and their triangles in index order, so the initial
    /// list — and therefore the tree — is fully determined by the scene.
    pub fn build(items: &[Item<'_>]) -> Bvh {
        Bvh::build_where(items, |_| true)
    }

    /// [`Bvh::build`] over the items `keep` accepts; the triangle's `item`
    /// still indexes the full list, so a hit names the same item either way.
    pub fn build_where(items: &[Item<'_>], keep: impl Fn(&Item<'_>) -> bool) -> Bvh {
        let t0 = super::aa::now_ms();
        let mut tris = Vec::new();
        for (ii, it) in items.iter().enumerate() {
            if !keep(it) {
                continue;
            }
            let m = it.mesh;
            for (t, tri) in m.indices.chunks_exact(3).enumerate() {
                let p = |i: u32| {
                    let i = i as usize * 3;
                    [m.positions[i], m.positions[i + 1], m.positions[i + 2]]
                };
                let (p0, p1, p2) = (p(tri[0]), p(tri[1]), p(tri[2]));
                let (e1, e2) = (sub(p1, p0), sub(p2, p0));
                let n = cross(e1, e2);
                if dot(n, n) <= 0.0 || !dot(n, n).is_finite() {
                    continue; // degenerate
                }
                tris.push(Tri { p0, e1, e2, item: ii as u32, tri: t as u32 });
            }
        }
        let mut bvh = Bvh::from_triangles(tris);
        bvh.stats.build_ms = super::aa::now_ms() - t0;
        bvh
    }

    /// Build over a triangle list directly (tests, and the item path above).
    pub fn from_triangles(mut tris: Vec<Tri>) -> Bvh {
        let n = tris.len();
        let mut nodes: Vec<Node> = Vec::with_capacity((2 * n / LEAF_MAX).max(1) + 1);
        let mut stats = BvhStats { triangles: n, ..Default::default() };
        if n == 0 {
            // A single empty node; traversal short-circuits on `is_empty`.
            stats.nodes = 1;
            stats.leaves = 1;
            return Bvh { nodes: vec![EMPTY_NODE4], leaves: Vec::new(), slots: Vec::new(), stats };
        }

        // Per-triangle bounds and centroids, computed once.
        let bounds: Vec<Aabb> = tris.iter().map(|t| t.bounds()).collect();
        let centroids: Vec<[f32; 3]> = tris.iter().map(|t| t.centroid()).collect();
        // `order` is the permutation being sorted; the triangles themselves
        // are rearranged once at the end so `bounds`/`centroids` stay valid.
        let mut order: Vec<u32> = (0..n as u32).collect();

        nodes.push(Node { lo: [0.0; 3], hi: [0.0; 3], first: 0, count: 0 });
        // (node index, range start, range end, depth)
        let mut stack: Vec<(usize, usize, usize, usize)> = vec![(0, 0, n, 0)];
        while let Some((ni, start, end, depth)) = stack.pop() {
            stats.max_depth = stats.max_depth.max(depth);
            let mut bb = Aabb::empty();
            let mut cb = Aabb::empty();
            for &i in &order[start..end] {
                bb.union(&bounds[i as usize]);
                cb.add(centroids[i as usize]);
            }
            nodes[ni].lo = bb.lo;
            nodes[ni].hi = bb.hi;
            let count = end - start;
            if count <= LEAF_MAX || depth >= MAX_DEPTH {
                nodes[ni].first = start as u32;
                nodes[ni].count = count as u32;
                stats.leaves += 1;
                continue;
            }

            // Binned SAH over all three axes.
            let mut best: Option<(usize, usize, f32)> = None; // (axis, split bin, cost)
            let parent_area = bb.area().max(1e-20);
            for axis in 0..3 {
                let lo = cb.lo[axis];
                let hi = cb.hi[axis];
                let extent = hi - lo;
                if !(extent > 1e-12) {
                    continue;
                }
                let k = BINS as f32 / extent;
                let mut bin_box = [Aabb::empty(); BINS];
                let mut bin_n = [0usize; BINS];
                for &i in &order[start..end] {
                    let b = bin_of(centroids[i as usize][axis], lo, k);
                    bin_box[b].union(&bounds[i as usize]);
                    bin_n[b] += 1;
                }
                // Prefix sweep from the left, suffix from the right.
                let mut left_area = [0.0f32; BINS];
                let mut left_n = [0usize; BINS];
                let mut acc = Aabb::empty();
                let mut cnt = 0usize;
                for b in 0..BINS {
                    acc.union(&bin_box[b]);
                    cnt += bin_n[b];
                    left_area[b] = acc.area();
                    left_n[b] = cnt;
                }
                let mut acc = Aabb::empty();
                let mut cnt = 0usize;
                for b in (1..BINS).rev() {
                    acc.union(&bin_box[b]);
                    cnt += bin_n[b];
                    let (la, ln) = (left_area[b - 1], left_n[b - 1]);
                    if ln == 0 || cnt == 0 {
                        continue;
                    }
                    // 1/8 traversal cost relative to one intersection.
                    let cost = 0.125 + (la * ln as f32 + acc.area() * cnt as f32) / parent_area;
                    if best.map(|(_, _, c)| cost < c).unwrap_or(true) {
                        best = Some((axis, b, cost));
                    }
                }
            }

            let leaf_cost = count as f32;
            let split = match best {
                Some((axis, b, cost)) if cost < leaf_cost || count > 16 => (axis, b),
                _ => {
                    nodes[ni].first = start as u32;
                    nodes[ni].count = count as u32;
                    stats.leaves += 1;
                    continue;
                }
            };
            let (axis, split_bin) = split;

            // Stable in-place partition by bin index (the order of equal
            // elements is irrelevant to correctness but is deterministic).
            let lo = cb.lo[axis];
            let k = BINS as f32 / (cb.hi[axis] - lo);
            let mut mid = start;
            {
                let mut scratch: Vec<u32> = Vec::with_capacity(count);
                // `mid <= j` at every step (it only advances when this one
                // is kept), so writing `order[mid]` never clobbers an entry
                // that has not been read yet.
                for j in start..end {
                    let i = order[j];
                    if bin_of(centroids[i as usize][axis], lo, k) < split_bin {
                        order[mid] = i;
                        mid += 1;
                    } else {
                        scratch.push(i);
                    }
                }
                order[mid..end].copy_from_slice(&scratch);
            }
            if mid == start || mid == end {
                mid = start + count / 2; // degenerate split; halve
            }

            let left = nodes.len();
            nodes.push(Node { lo: [0.0; 3], hi: [0.0; 3], first: 0, count: 0 });
            nodes.push(Node { lo: [0.0; 3], hi: [0.0; 3], first: 0, count: 0 });
            nodes[ni].first = left as u32;
            nodes[ni].count = 0;
            // Push right first so the left child is processed next: node
            // indices then follow a deterministic depth-first order.
            stack.push((left + 1, mid, end, depth + 1));
            stack.push((left, start, mid, depth + 1));
        }

        // Apply the permutation to the triangles themselves.
        let permuted: Vec<Tri> = order.iter().map(|&i| tris[i as usize]).collect();
        tris = permuted;
        let (nodes, leaves, slots) = collapse(&nodes, &tris);
        stats.nodes = nodes.len();
        Bvh { nodes, leaves, slots, stats }
    }

    /// Build over the triangles of one mesh, with `item` 0 and `tri` the
    /// triangle's index in `mesh.indices`. This is the entry point analysis
    /// code outside the renderer uses (`cadpole-print`'s thickness ray casts and
    /// its self-intersection broad phase), so a wall-thickness query and a
    /// path trace walk exactly the same tree.
    pub fn from_mesh(mesh: &crate::tessellate::Mesh) -> Bvh {
        let mut tris = Vec::with_capacity(mesh.triangle_count());
        for (t, tri) in mesh.indices.chunks_exact(3).enumerate() {
            let p = |i: u32| {
                let i = i as usize * 3;
                [mesh.positions[i], mesh.positions[i + 1], mesh.positions[i + 2]]
            };
            let (p0, p1, p2) = (p(tri[0]), p(tri[1]), p(tri[2]));
            let (e1, e2) = (sub(p1, p0), sub(p2, p0));
            let n = cross(e1, e2);
            if dot(n, n) <= 0.0 || !dot(n, n).is_finite() {
                continue; // degenerate: it can never be hit anyway
            }
            tris.push(Tri { p0, e1, e2, item: 0, tri: t as u32 });
        }
        Bvh::from_triangles(tris)
    }

    /// Every triangle whose bounds overlap the box, by slot (see
    /// [`Bvh::tri`]). The broad phase of the self-intersection check: a
    /// triangle only has to be tested against the handful of triangles whose
    /// boxes touch its own.
    pub fn overlapping(&self, lo: [f32; 3], hi: [f32; 3], out: &mut Vec<u32>) {
        out.clear();
        if self.is_empty() {
            return;
        }
        let mut stack = [0u32; MAX_STACK];
        let mut sp = 0usize;
        let mut node = 0u32;
        loop {
            let n = &self.nodes[node as usize];
            for slot in 0..4 {
                if n.valid & (1 << slot) == 0 {
                    continue;
                }
                if (0..3).any(|k| n.lo[k][slot] > hi[k] || n.hi[k][slot] < lo[k]) {
                    continue;
                }
                let count = n.count[slot] as usize;
                if count > 0 {
                    let s = n.child[slot] * 4;
                    for i in s..s + count as u32 {
                        let b = self.tri(i).bounds();
                        if (0..3).all(|k| b.lo[k] <= hi[k] && b.hi[k] >= lo[k]) {
                            out.push(i);
                        }
                    }
                } else if sp < MAX_STACK {
                    stack[sp] = n.child[slot];
                    sp += 1;
                }
            }
            if sp == 0 {
                break;
            }
            sp -= 1;
            node = stack[sp];
        }
    }

    /// Closest hit in `(tmin, tmax)`, or `None`. Two-sided: a ray inside a
    /// closed solid hits its far wall, which is what refraction needs.
    ///
    /// The stack holds children, not nodes: a visited node tests its four
    /// boxes at once and pushes the ones the ray enters, farthest first, so
    /// the next pop is always the nearest thing not yet looked at. Each
    /// entry remembers the distance its box was entered at, and an entry
    /// that is farther than the best hit so far is dropped without being
    /// loaded — the standard front-to-back pruning, which the two-wide
    /// traversal could only do after reading the node.
    pub fn intersect(&self, o: [f32; 3], d: [f32; 3], tmin: f32, tmax: f32) -> Option<Hit> {
        self.intersect_where(o, d, tmin, tmax, |_| true)
    }

    /// [`Bvh::intersect`] over the triangles `keep` accepts, by slot
    /// (`item_of` names the item): the closest hit among them, the rest
    /// passed through as if they were not there. How the path tracer's
    /// light transport ignores guide geometry. With `|_| true` it
    /// is `intersect` itself, to the bit.
    #[inline]
    pub fn intersect_where(&self, o: [f32; 3], d: [f32; 3], tmin: f32, tmax: f32, keep: impl Fn(u32) -> bool) -> Option<Hit> {
        if self.is_empty() {
            return None;
        }
        let ray = Ray4::new(o, d);
        let mut best: Option<Hit> = None;
        let mut far = tmax;
        let mut storage = StackStorage::uninit();
        let mut stack = Stack::new(&mut storage);
        let mut cur = Entry { child: 0, count: 0, near: tmin };
        loop {
            // Entered farther away than the best hit so far: nothing in it
            // can win, so it is dropped without being loaded.
            if cur.near <= far {
                if cur.count > 0 {
                    for blk in cur.child..cur.child + cur.count.div_ceil(4) {
                        let (mask, t, u, v) = ray.test_tris(&self.leaves[blk as usize], tmin, far);
                        // Lowest lane wins a tie, as the first of four
                        // triangles tested one after another did.
                        for lane in 0..4 {
                            if mask & (1 << lane) != 0 && t[lane] < far && keep(blk * 4 + lane as u32) {
                                far = t[lane];
                                best = Some(Hit { t: t[lane], u: u[lane], v: v[lane], tri: blk * 4 + lane as u32 });
                            }
                        }
                    }
                } else {
                    let n = &self.nodes[cur.child as usize];
                    let (mask, near) = ray.test(n, tmin, far);
                    if let Some(nearest) = push_ordered(n, mask, near, &mut stack) {
                        cur = nearest;
                        continue;
                    }
                }
            }
            match stack.pop() {
                Some(e) => cur = e,
                None => break,
            }
        }
        best
    }

    /// True when anything blocks `(tmin, tmax)`. Exits on the first hit, so
    /// it is roughly twice as fast as [`Bvh::intersect`] for shadow rays.
    /// Nothing is ordered: any hit ends the search, so the order children
    /// are looked at only changes how soon, not what is found.
    pub fn occluded(&self, o: [f32; 3], d: [f32; 3], tmin: f32, tmax: f32) -> bool {
        self.occluded_where(o, d, tmin, tmax, |_| true)
    }

    /// [`Bvh::occluded`] by the triangles `keep` accepts, by slot; the rest
    /// do not block. With `|_| true` it is `occluded` itself.
    #[inline]
    pub fn occluded_where(&self, o: [f32; 3], d: [f32; 3], tmin: f32, tmax: f32, keep: impl Fn(u32) -> bool) -> bool {
        if self.is_empty() {
            return false;
        }
        let ray = Ray4::new(o, d);
        let mut storage = StackStorage::uninit();
        let mut stack = Stack::new(&mut storage);
        let mut node = 0u32;
        loop {
            let n = &self.nodes[node as usize];
            let (mut mask, _) = ray.test(n, tmin, tmax);
            let mut next: Option<u32> = None;
            while mask != 0 {
                let slot = mask.trailing_zeros() as usize;
                mask &= mask - 1;
                if n.count[slot] > 0 {
                    for blk in n.child[slot]..n.child[slot] + n.count[slot].div_ceil(4) {
                        let mut hit = ray.test_tris(&self.leaves[blk as usize], tmin, tmax).0;
                        while hit != 0 {
                            let lane = hit.trailing_zeros();
                            hit &= hit - 1;
                            if keep(blk * 4 + lane) {
                                return true;
                            }
                        }
                    }
                } else if next.is_none() {
                    next = Some(n.child[slot]);
                } else {
                    stack.push(Entry { child: n.child[slot], count: 0, near: 0.0 });
                }
            }
            match next.or_else(|| stack.pop().map(|e| e.child)) {
                Some(c) => node = c,
                None => break,
            }
        }
        false
    }

    /// A shadow ray through a scene that has see-through geometry, in one
    /// traversal: the nearest triangle whose item is `opaque` in
    /// `(tmin, tmax)`, if there is one, and every see-through triangle
    /// crossed before it, appended to `out` (cleared first) in the order the
    /// tree yields them, up to `limit` of them.
    ///
    /// The alternative is what [`Bvh::intersect`] does when it is called in
    /// a loop — find the nearest crossing, step past it, find the next — and
    /// that is a full traversal from the root for every interface plus one
    /// more to learn there is nothing behind the last. A shadow ray that
    /// leaves a clear part through two walls and then finds the sky pays
    /// three traversals that way; here it pays one. The price is that the
    /// see-through crossings cannot prune each other (a nearer one does not
    /// end the search), so the walk visits every node the ray passes
    /// through before `tmax`; on a scene that is mostly opaque that is what
    /// the last of those loop iterations visited anyway. An opaque crossing
    /// does prune, exactly as a hit prunes the closest-hit search: nothing
    /// farther than it can matter, and the walk is front to back, so a
    /// blocker close to the origin is found before anything beyond it is
    /// looked at.
    ///
    /// Nothing is sorted here. The caller wants the crossings in distance
    /// order and there are usually two of them; sorting a handful of entries
    /// on the stack costs less than keeping the buffer ordered on the way.
    /// `out` is the caller's scratch, reused across rays, rather than a
    /// buffer built here: an array of thirty-two hits zeroed on the stack
    /// for every shadow ray was six per cent of the impeller render — a
    /// `memset` of half a kilobyte against a traversal that touches a few
    /// dozen nodes — and a `Vec` that keeps its capacity costs nothing to
    /// clear.
    pub fn shadow_hits(&self, o: [f32; 3], d: [f32; 3], tmin: f32, tmax: f32, opaque: &[bool], out: &mut Vec<Hit>, limit: usize) -> ShadowHits {
        self.shadow_hits_where(o, d, tmin, tmax, opaque, out, limit, |_| true)
    }

    /// [`Bvh::shadow_hits`] over the triangles `keep` accepts, by slot: the
    /// rest neither block nor count as a crossing. With `|_| true` it is
    /// `shadow_hits` itself.
    #[allow(clippy::too_many_arguments)]
    #[inline]
    pub fn shadow_hits_where(
        &self,
        o: [f32; 3],
        d: [f32; 3],
        tmin: f32,
        tmax: f32,
        opaque: &[bool],
        out: &mut Vec<Hit>,
        limit: usize,
        keep: impl Fn(u32) -> bool,
    ) -> ShadowHits {
        out.clear();
        if self.is_empty() {
            return ShadowHits::Through;
        }
        let ray = Ray4::new(o, d);
        let mut far = tmax;
        let mut blocked: Option<f32> = None;
        let mut storage = StackStorage::uninit();
        let mut stack = Stack::new(&mut storage);
        let mut cur = Entry { child: 0, count: 0, near: tmin };
        loop {
            if cur.near <= far {
                if cur.count > 0 {
                    for blk in cur.child..cur.child + cur.count.div_ceil(4) {
                        let (mut mask, t, u, v) = ray.test_tris(&self.leaves[blk as usize], tmin, far);
                        while mask != 0 {
                            let lane = mask.trailing_zeros() as usize;
                            mask &= mask - 1;
                            let i = blk * 4 + lane as u32;
                            if !keep(i) {
                                continue;
                            }
                            if opaque[self.slots[i as usize].item as usize] {
                                // Nearer than any opaque hit so far, or the
                                // lane test would have rejected it.
                                far = t[lane];
                                blocked = Some(far);
                            } else {
                                if out.len() == limit {
                                    return ShadowHits::Overflow;
                                }
                                out.push(Hit { t: t[lane], u: u[lane], v: v[lane], tri: i });
                            }
                        }
                    }
                } else {
                    let n = &self.nodes[cur.child as usize];
                    let (mask, near) = ray.test(n, tmin, far);
                    if let Some(nearest) = push_ordered(n, mask, near, &mut stack) {
                        cur = nearest;
                        continue;
                    }
                }
            }
            match stack.pop() {
                Some(e) => cur = e,
                None => break,
            }
        }
        match blocked {
            Some(t) => {
                // Crossings collected before the blocker was found may lie
                // beyond it; the caller is promised only what is in front.
                out.retain(|h| h.t <= t);
                ShadowHits::Blocked(t)
            }
            None => ShadowHits::Through,
        }
    }
}

/// One thing a traversal still has to look at: a child of a visited node,
/// with the distance the ray entered its box at.
#[derive(Clone, Copy)]
struct Entry {
    /// `Node4` index, or the leaf's first `Tri4` block when `count > 0`.
    child: u32,
    count: u32,
    near: f32,
}

/// Room for the children a traversal puts aside: a local of the traversal,
/// left uninitialised, that a [`Stack`] borrows.
type StackStorage = std::mem::MaybeUninit<[Entry; MAX_STACK]>;

/// The children a traversal has put aside for later.
///
/// The storage is a separate local on purpose, not a field. Two kilobytes
/// of `MaybeUninit` inside this struct, next to `sp: 0`, is a single
/// aggregate store of mostly-undefined bytes, and LLVM may choose any value
/// for undefined bytes — it chose zero and made the store a `memset` of the
/// whole thing before every traversal, 21 % of an opaque scene's render,
/// where a traversal is a dozen nodes and the clear cost more than the
/// walk. A local that is only ever written through `push` has no such
/// store. The entries are only ever read back below the high-water mark,
/// so nothing needs a starting value.
struct Stack<'a> {
    buf: &'a mut StackStorage,
    sp: usize,
}

impl<'a> Stack<'a> {
    #[inline]
    fn new(buf: &'a mut StackStorage) -> Stack<'a> {
        Stack { buf, sp: 0 }
    }

    /// A full stack drops the entry, as the two-wide traversal did: the
    /// bound is three entries per level of a tree the builder caps at
    /// [`MAX_DEPTH`], so it cannot fill.
    #[inline]
    fn push(&mut self, e: Entry) {
        if self.sp < MAX_STACK {
            // SAFETY: `sp < MAX_STACK`, so the write is inside the array.
            unsafe { self.buf.as_mut_ptr().cast::<Entry>().add(self.sp).write(e) };
            self.sp += 1;
        }
    }

    #[inline]
    fn pop(&mut self) -> Option<Entry> {
        if self.sp == 0 {
            return None;
        }
        self.sp -= 1;
        // SAFETY: `sp` only ever grows past an index by writing it in
        // `push`, so every slot below the current `sp` holds an entry.
        Some(unsafe { self.buf.as_ptr().cast::<Entry>().add(self.sp).read() })
    }
}

/// Which of the children `mask` selects to look at next: the nearest, which
/// is returned rather than pushed, while the rest go on the stack farthest
/// first so that the next pop is the nearest of them. Two children entered
/// at the same distance go in slot order, which is the left-to-right order
/// of the binary tree they came from, and so the order the two-wide
/// traversal visited them in.
///
/// Forced inline, and with the one- and two-child cases handled before
/// anything is sorted: this runs once per node a ray visits, the compiler
/// left it out of line when merely asked, and as a call with a frame of its
/// own it was a fifth of the render. Most visits enter one child, and most
/// of the rest two.
#[inline(always)]
fn push_ordered(n: &Node4, mask: u32, near: [f32; 4], stack: &mut Stack) -> Option<Entry> {
    let entry = |slot: usize| Entry { child: n.child[slot], count: n.count[slot], near: near[slot] };
    if mask == 0 {
        return None;
    }
    let a = mask.trailing_zeros() as usize;
    let rest = mask & (mask - 1);
    if rest == 0 {
        return Some(entry(a));
    }
    let b = rest.trailing_zeros() as usize;
    if rest & (rest - 1) == 0 {
        // Two: the earlier slot wins a tie, as in the sort below.
        let (near_first, far_first) = if near[a] <= near[b] { (a, b) } else { (b, a) };
        stack.push(entry(far_first));
        return Some(entry(near_first));
    }
    let mut order = [(0.0f32, 0usize); 4];
    let mut k = 0usize;
    let mut m = mask;
    while m != 0 {
        let slot = m.trailing_zeros() as usize;
        m &= m - 1;
        order[k] = (near[slot], slot);
        k += 1;
    }
    // Insertion sort, descending by distance; an equal key moves past the
    // earlier slot so that the earlier slot ends up nearest.
    for i in 1..k {
        let x = order[i];
        let mut j = i;
        while j > 0 && order[j - 1].0 <= x.0 {
            order[j] = order[j - 1];
            j -= 1;
        }
        order[j] = x;
    }
    for &(_, slot) in &order[..k - 1] {
        stack.push(entry(slot));
    }
    Some(entry(order[k - 1].1))
}

/// Collapse the binary tree into four-wide nodes.
///
/// Each four-wide node starts as a binary node's two children; while it has
/// a free slot, the interior child with the largest box is replaced by its
/// own two children, so the widest subtrees are the ones flattened. Ties
/// go to the earlier slot, and a child's two halves take its place in
/// order, so the slots keep the binary tree's left-to-right order and the
/// result is a pure function of the binary tree, exactly as that tree is of
/// the triangle list.
///
/// Leaves are packed at the same time: each binary leaf's triangles become
/// [`Tri4`] blocks, four to a block, and the four-wide node's `child` for a
/// leaf slot is its first block's index. The returned `TriRef`s say, per
/// slot, which triangle of which item is there.
///
/// Blocks are laid out in the order of the triangles they hold, not in the
/// order this walk reaches the leaves, so the slots hold `tris` in the
/// order the build left it, with padding after each leaf's run. That order
/// is observable: `Tracer::new` sums the emissive triangles' power over
/// [`Bvh::triangles`] to build a sampling table, and a sum of floats taken
/// in another order rounds to another total, which moves every light sample
/// after it. The walk's own order changed the image under an emissive body
/// by a few hundredths of a level over a fifth of its pixels: not wrong, but
/// not the same image, and the same image is the claim this tree makes.
/// (Binary node order is not good enough either — the builder numbers a
/// right child before its left sibling's descendants, so a right child that
/// is a leaf comes before leaves that hold earlier triangles.)
fn collapse(bin: &[Node], tris: &[Tri]) -> (Vec<Node4>, Vec<Tri4>, Vec<TriRef>) {
    let mut out: Vec<Node4> = Vec::with_capacity(bin.len() / 3 + 1);
    // The leaves' ranges tile `tris`, so walking `first` upward visits them
    // in triangle order; `leaf_at` maps a range's start to its node.
    let mut leaf_at = vec![u32::MAX; tris.len()];
    for (i, b) in bin.iter().enumerate() {
        if b.count > 0 {
            leaf_at[b.first as usize] = i as u32;
        }
    }
    // First block of each leaf, by binary node index; interior nodes hold
    // whatever value, never read.
    let mut block_of = vec![0u32; bin.len()];
    let mut n_blocks = 0u32;
    for &i in leaf_at.iter().filter(|&&i| i != u32::MAX) {
        block_of[i as usize] = n_blocks;
        n_blocks += bin[i as usize].count.div_ceil(4);
    }
    let mut leaves: Vec<Tri4> = vec![EMPTY_TRI4; n_blocks as usize];
    let mut slots: Vec<TriRef> = vec![TriRef { item: u32::MAX, tri: 0 }; 4 * n_blocks as usize];
    out.push(EMPTY_NODE4);
    // (binary node whose children fill it, index of the four-wide node)
    let mut stack: Vec<(u32, u32)> = vec![(0, 0)];
    while let Some((bi, oi)) = stack.pop() {
        let b = &bin[bi as usize];
        let mut kids = [0u32; 4];
        let mut k;
        if b.count > 0 {
            // A tree that is one leaf: a node with one slot.
            kids[0] = bi;
            k = 1;
        } else {
            kids[0] = b.first;
            kids[1] = b.first + 1;
            k = 2;
        }
        while k < 4 {
            let mut pick: Option<(usize, f32)> = None;
            for (s, &ki) in kids[..k].iter().enumerate() {
                let c = &bin[ki as usize];
                if c.count == 0 {
                    let area = Aabb { lo: c.lo, hi: c.hi }.area();
                    if pick.map(|(_, a)| area > a).unwrap_or(true) {
                        pick = Some((s, area));
                    }
                }
            }
            let Some((s, _)) = pick else { break };
            let first = bin[kids[s] as usize].first;
            // Open slot `s` into two: shift what follows it right by one.
            for j in (s + 1..k).rev() {
                kids[j + 1] = kids[j];
            }
            kids[s] = first;
            kids[s + 1] = first + 1;
            k += 1;
        }
        let mut n4 = EMPTY_NODE4;
        for (s, &ki) in kids[..k].iter().enumerate() {
            let c = &bin[ki as usize];
            for axis in 0..3 {
                n4.lo[axis][s] = c.lo[axis];
                n4.hi[axis][s] = c.hi[axis];
            }
            if c.count > 0 {
                // A leaf is usually one block, but the builder keeps a node
                // of up to sixteen triangles whole when no split pays for
                // itself, so a leaf is as many consecutive blocks as it
                // needs; only the last one has padding lanes.
                let first_block = block_of[ki as usize];
                n4.child[s] = first_block;
                n4.count[s] = c.count;
                for k in 0..c.count {
                    let t = &tris[(c.first + k) as usize];
                    let (block, lane) = (&mut leaves[(first_block + k / 4) as usize], (k % 4) as usize);
                    for axis in 0..3 {
                        block.p0[axis][lane] = t.p0[axis];
                        block.e1[axis][lane] = t.e1[axis];
                        block.e2[axis][lane] = t.e2[axis];
                    }
                    slots[(4 * first_block + k) as usize] = TriRef { item: t.item, tri: t.tri };
                }
            } else {
                let idx = out.len() as u32;
                out.push(EMPTY_NODE4);
                n4.child[s] = idx;
                stack.push((ki, idx));
            }
        }
        n4.valid = (1u32 << k) - 1;
        out[oi as usize] = n4;
    }
    (out, leaves, slots)
}

/// The ray's origin, direction and reciprocal direction, one register per
/// axis with the value in all four lanes, so a node's four boxes — or a
/// leaf's four triangles — are tested against it in one pass. Built once
/// per traversal.
#[derive(Clone, Copy)]
struct Ray4 {
    #[cfg(not(target_arch = "x86_64"))]
    o: [f32; 3],
    #[cfg(not(target_arch = "x86_64"))]
    d: [f32; 3],
    #[cfg(not(target_arch = "x86_64"))]
    inv: [f32; 3],
    #[cfg(target_arch = "x86_64")]
    o: [std::arch::x86_64::__m128; 3],
    #[cfg(target_arch = "x86_64")]
    d: [std::arch::x86_64::__m128; 3],
    #[cfg(target_arch = "x86_64")]
    inv: [std::arch::x86_64::__m128; 3],
}

impl Ray4 {
    #[inline]
    fn new(o: [f32; 3], d: [f32; 3]) -> Ray4 {
        let inv = [1.0 / d[0], 1.0 / d[1], 1.0 / d[2]];
        #[cfg(not(target_arch = "x86_64"))]
        {
            Ray4 { o, d, inv }
        }
        #[cfg(target_arch = "x86_64")]
        {
            use std::arch::x86_64::*;
            // SAFETY: SSE2, guaranteed on x86-64.
            unsafe {
                Ray4 {
                    o: [_mm_set1_ps(o[0]), _mm_set1_ps(o[1]), _mm_set1_ps(o[2])],
                    d: [_mm_set1_ps(d[0]), _mm_set1_ps(d[1]), _mm_set1_ps(d[2])],
                    inv: [_mm_set1_ps(inv[0]), _mm_set1_ps(inv[1]), _mm_set1_ps(inv[2])],
                }
            }
        }
    }

    /// Which of a leaf's four triangles the ray hits in `(tmin, tmax)`, as
    /// a bit per lane, with the distance and barycentrics of each.
    #[inline]
    fn test_tris(&self, t4: &Tri4, tmin: f32, tmax: f32) -> (u32, [f32; 4], [f32; 4], [f32; 4]) {
        #[cfg(not(target_arch = "x86_64"))]
        {
            let mut mask = 0u32;
            let (mut t, mut u, mut v) = ([0.0f32; 4], [0.0f32; 4], [0.0f32; 4]);
            for lane in 0..4 {
                let tri = Tri {
                    p0: [t4.p0[0][lane], t4.p0[1][lane], t4.p0[2][lane]],
                    e1: [t4.e1[0][lane], t4.e1[1][lane], t4.e1[2][lane]],
                    e2: [t4.e2[0][lane], t4.e2[1][lane], t4.e2[2][lane]],
                    item: 0,
                    tri: 0,
                };
                if let Some((tt, uu, vv)) = intersect_tri(&tri, self.o, self.d, tmin, tmax) {
                    mask |= 1 << lane;
                    t[lane] = tt;
                    u[lane] = uu;
                    v[lane] = vv;
                }
            }
            (mask, t, u, v)
        }
        #[cfg(target_arch = "x86_64")]
        {
            tri4_sse(t4, self.o, self.d, tmin, tmax)
        }
    }

    /// Which of the node's children the ray enters within `(tmin, tmax)`,
    /// as a bit per slot, and the entry distance of each.
    #[inline]
    fn test(&self, n: &Node4, tmin: f32, tmax: f32) -> (u32, [f32; 4]) {
        #[cfg(not(target_arch = "x86_64"))]
        {
            let mut mask = 0u32;
            let mut near = [0.0f32; 4];
            for slot in 0..4 {
                if n.valid & (1 << slot) == 0 {
                    continue;
                }
                let lo = [n.lo[0][slot], n.lo[1][slot], n.lo[2][slot]];
                let hi = [n.hi[0][slot], n.hi[1][slot], n.hi[2][slot]];
                if let Some(t) = slab(lo, hi, self.o, self.inv, tmin, tmax) {
                    mask |= 1 << slot;
                    near[slot] = t;
                }
            }
            (mask, near)
        }
        #[cfg(target_arch = "x86_64")]
        {
            slab4_sse(n, self.o, self.inv, tmin, tmax)
        }
    }
}

/// SSE2 form of [`slab`] over a node's four boxes at once, lane for lane
/// the same test.
///
/// SSE2 is part of the x86-64 baseline, so this needs no runtime detection
/// and no `target_feature`: the intrinsics are unconditionally available on
/// every machine that can run the binary at all.
///
/// The NaN behaviour the scalar version documents is preserved exactly, and
/// the operand order is what preserves it. `minps(a, b)` computes
/// `a < b ? a : b` and `maxps(b, a)` computes `b > a ? b : a`; an unordered
/// comparison is false, so a NaN in either operand falls through to the
/// same operand the scalar `if a <= b { (a, b) } else { (b, a) }` picks.
/// Folding into the running interval is the same trick: `maxps(near, t0)`
/// yields `t0` when `near` is NaN, which is what `t0.max(near)` does, and
/// `minps(far, t1)` yields `t1` when `far` is NaN.
///
/// The scalar version bails out as soon as `t0 > t1`; this one always does
/// all three axes and compares once at the end. That is the same answer:
/// `t0` only rises and `t1` only falls, so an interval that has crossed
/// stays crossed. Unused slots hold zeros — real, finite floats, never a
/// reinterpreted integer, because a denormal in a lane nothing reads still
/// costs a microcode assist on some processors' efficiency cores, which is
/// the cliff the two-wide test had to mask its way around — and `valid`
/// drops them from the answer.
#[cfg(target_arch = "x86_64")]
#[inline]
fn slab4_sse(n: &Node4, o: [std::arch::x86_64::__m128; 3], inv: [std::arch::x86_64::__m128; 3], tmin: f32, tmax: f32) -> (u32, [f32; 4]) {
    use std::arch::x86_64::*;
    // SAFETY: every intrinsic below is SSE/SSE2, which the x86-64 target
    // guarantees. The aligned loads read `Node4::lo[k]` and `hi[k]`, each a
    // 16-byte array at a multiple of 16 inside a 64-byte-aligned node.
    unsafe {
        let mut t0 = _mm_set1_ps(tmin);
        let mut t1 = _mm_set1_ps(tmax);
        for k in 0..3 {
            let lo = _mm_load_ps(n.lo[k].as_ptr());
            let hi = _mm_load_ps(n.hi[k].as_ptr());
            let a = _mm_mul_ps(_mm_sub_ps(lo, o[k]), inv[k]);
            let b = _mm_mul_ps(_mm_sub_ps(hi, o[k]), inv[k]);
            let near = _mm_min_ps(a, b);
            let far = _mm_max_ps(b, a);
            t0 = _mm_max_ps(near, t0);
            t1 = _mm_min_ps(far, t1);
        }
        let mask = _mm_movemask_ps(_mm_cmple_ps(t0, t1)) as u32 & n.valid;
        let mut near = [0.0f32; 4];
        _mm_storeu_ps(near.as_mut_ptr(), t0);
        (mask, near)
    }
}

/// SSE2 form of [`intersect_tri`] over a leaf's four triangles at once,
/// lane for lane the same arithmetic in the same order — every product,
/// sum and difference of the scalar version, as an `mulps`, `addps` or
/// `subps` on four lanes, with no fused multiply-add anywhere — so the
/// distance and barycentrics it returns are the bits the scalar test
/// returns. The one place the two could disagree is a NaN determinant,
/// which the scalar test lets through and this one rejects; the builder
/// drops degenerate triangles, so neither ever sees one.
#[cfg(target_arch = "x86_64")]
#[inline]
fn tri4_sse(t4: &Tri4, o: [std::arch::x86_64::__m128; 3], d: [std::arch::x86_64::__m128; 3], tmin: f32, tmax: f32) -> (u32, [f32; 4], [f32; 4], [f32; 4]) {
    use std::arch::x86_64::*;
    // SAFETY: every intrinsic below is SSE/SSE2, which the x86-64 target
    // guarantees. The aligned loads read the 16-byte rows of a `Tri4`,
    // which is 16-byte aligned.
    unsafe {
        let ld = |a: &[f32; 4]| _mm_load_ps(a.as_ptr());
        let p0 = [ld(&t4.p0[0]), ld(&t4.p0[1]), ld(&t4.p0[2])];
        let e1 = [ld(&t4.e1[0]), ld(&t4.e1[1]), ld(&t4.e1[2])];
        let e2 = [ld(&t4.e2[0]), ld(&t4.e2[1]), ld(&t4.e2[2])];
        let cross = |a: [__m128; 3], b: [__m128; 3]| {
            [
                _mm_sub_ps(_mm_mul_ps(a[1], b[2]), _mm_mul_ps(a[2], b[1])),
                _mm_sub_ps(_mm_mul_ps(a[2], b[0]), _mm_mul_ps(a[0], b[2])),
                _mm_sub_ps(_mm_mul_ps(a[0], b[1]), _mm_mul_ps(a[1], b[0])),
            ]
        };
        let dot = |a: [__m128; 3], b: [__m128; 3]| {
            _mm_add_ps(_mm_add_ps(_mm_mul_ps(a[0], b[0]), _mm_mul_ps(a[1], b[1])), _mm_mul_ps(a[2], b[2]))
        };
        let pv = cross(d, e2);
        let det = dot(e1, pv);
        let abs = _mm_castsi128_ps(_mm_set1_epi32(0x7fff_ffff));
        let mut ok = _mm_cmpge_ps(_mm_and_ps(det, abs), _mm_set1_ps(1e-18));
        let inv_det = _mm_div_ps(_mm_set1_ps(1.0), det);
        let tv = [_mm_sub_ps(o[0], p0[0]), _mm_sub_ps(o[1], p0[1]), _mm_sub_ps(o[2], p0[2])];
        let u = _mm_mul_ps(dot(tv, pv), inv_det);
        let lo = _mm_set1_ps(-TRI_EPS);
        let hi = _mm_set1_ps(1.0 + TRI_EPS);
        ok = _mm_and_ps(ok, _mm_and_ps(_mm_cmpge_ps(u, lo), _mm_cmple_ps(u, hi)));
        let qv = cross(tv, e1);
        let v = _mm_mul_ps(dot(d, qv), inv_det);
        ok = _mm_and_ps(ok, _mm_and_ps(_mm_cmpge_ps(v, lo), _mm_cmple_ps(_mm_add_ps(u, v), hi)));
        let t = _mm_mul_ps(dot(e2, qv), inv_det);
        ok = _mm_and_ps(ok, _mm_and_ps(_mm_cmpgt_ps(t, _mm_set1_ps(tmin)), _mm_cmplt_ps(t, _mm_set1_ps(tmax))));
        let mask = _mm_movemask_ps(ok) as u32;
        let (mut ts, mut us, mut vs) = ([0.0f32; 4], [0.0f32; 4], [0.0f32; 4]);
        _mm_storeu_ps(ts.as_mut_ptr(), t);
        _mm_storeu_ps(us.as_mut_ptr(), u);
        _mm_storeu_ps(vs.as_mut_ptr(), v);
        (mask, ts, us, vs)
    }
}

#[inline]
fn bin_of(c: f32, lo: f32, k: f32) -> usize {
    let b = ((c - lo) * k) as i32;
    b.clamp(0, BINS as i32 - 1) as usize
}

/// Slab test; returns the entry distance when the box overlaps
/// `(tmin, tmax)`. Infinities from axis-aligned directions work out because
/// `0 * inf` cases are resolved by the min/max ordering below.
#[cfg_attr(target_arch = "x86_64", allow(dead_code))] // the SSE path replaces it there, except in tests
#[inline]
fn slab(lo: [f32; 3], hi: [f32; 3], o: [f32; 3], inv: [f32; 3], tmin: f32, tmax: f32) -> Option<f32> {
    let mut t0 = tmin;
    let mut t1 = tmax;
    for k in 0..3 {
        let a = (lo[k] - o[k]) * inv[k];
        let b = (hi[k] - o[k]) * inv[k];
        // f32::max / f32::min return the non-NaN operand, so a ray origin
        // exactly on a box plane with a zero direction component (which
        // produces 0 * inf = NaN) degrades to "visit the box", never to a
        // wrong answer.
        let (near, far) = if a <= b { (a, b) } else { (b, a) };
        t0 = t0.max(near);
        t1 = t1.min(far);
        if t0 > t1 {
            return None;
        }
    }
    Some(t0)
}

/// The relative epsilon on Möller–Trumbore's barycentric bounds that keeps
/// shared edges from leaking rays through a closed mesh.
const TRI_EPS: f32 = 1e-7;

/// Möller–Trumbore, two-sided. The reference for [`tri4_sse`], and the
/// intersector on targets without SSE.
#[cfg_attr(target_arch = "x86_64", allow(dead_code))] // the SSE path replaces it there, except in tests
#[inline]
fn intersect_tri(tri: &Tri, o: [f32; 3], d: [f32; 3], tmin: f32, tmax: f32) -> Option<(f32, f32, f32)> {
    const EPS: f32 = TRI_EPS;
    let pv = cross(d, tri.e2);
    let det = dot(tri.e1, pv);
    if det.abs() < 1e-18 {
        return None;
    }
    let inv_det = 1.0 / det;
    let tv = sub(o, tri.p0);
    let u = dot(tv, pv) * inv_det;
    if u < -EPS || u > 1.0 + EPS {
        return None;
    }
    let qv = cross(tv, tri.e1);
    let v = dot(d, qv) * inv_det;
    if v < -EPS || u + v > 1.0 + EPS {
        return None;
    }
    let t = dot(tri.e2, qv) * inv_det;
    if t <= tmin || t >= tmax {
        return None;
    }
    Some((t, u, v))
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::render::textures::hash3;

    /// Deterministic pseudo-random floats from the texture hash, so the test
    /// scene is the same everywhere and pulls in no dependency.
    fn rnd(i: u32, k: u32) -> f32 {
        hash3(i as i32, k as i32, 17, 0x51ed_2701) as f32 * 2.328_306_4e-10
    }

    fn random_tris(n: u32) -> Vec<Tri> {
        (0..n)
            .map(|i| {
                let c = [rnd(i, 0) * 100.0 - 50.0, rnd(i, 1) * 100.0 - 50.0, rnd(i, 2) * 100.0 - 50.0];
                let s = 4.0 + rnd(i, 3) * 14.0;
                let v = |k: u32| [c[0] + (rnd(i, k) - 0.5) * s, c[1] + (rnd(i, k + 1) - 0.5) * s, c[2] + (rnd(i, k + 2) - 0.5) * s];
                let (p0, p1, p2) = (v(4), v(7), v(10));
                Tri { p0, e1: sub(p1, p0), e2: sub(p2, p0), item: 0, tri: i }
            })
            .filter(|t| t.area() > 1e-4)
            .collect()
    }

    /// Closest hit over every `(index, triangle)` pair, first index winning
    /// a tie, as the tree's leaf test does.
    fn brute_force<I: IntoIterator<Item = (u32, Tri)>>(tris: I, o: [f32; 3], d: [f32; 3], tmax: f32) -> Option<(f32, u32)> {
        let mut best: Option<(f32, u32)> = None;
        for (i, t) in tris {
            if let Some((t, _, _)) = intersect_tri(&t, o, d, 1e-4, best.map(|b| b.0).unwrap_or(tmax)) {
                best = Some((t, i));
            }
        }
        best
    }

    fn indexed(tris: &[Tri]) -> impl Iterator<Item = (u32, Tri)> + '_ {
        tris.iter().enumerate().map(|(i, t)| (i as u32, *t))
    }

    #[test]
    fn traversal_matches_brute_force_on_a_random_scene() {
        let tris = random_tris(2000);
        let bvh = Bvh::from_triangles(tris.clone());
        // The tree permutes the triangles, so compare distances, not indices.
        let mut checked = 0;
        let mut hits = 0;
        for r in 0..3000u32 {
            let o = [rnd(r, 20) * 200.0 - 100.0, rnd(r, 21) * 200.0 - 100.0, rnd(r, 22) * 200.0 - 100.0];
            let dir = [rnd(r, 23) * 2.0 - 1.0, rnd(r, 24) * 2.0 - 1.0, rnd(r, 25) * 2.0 - 1.0];
            let len = dot(dir, dir).sqrt();
            if len < 1e-3 {
                continue;
            }
            let d = [dir[0] / len, dir[1] / len, dir[2] / len];
            let want = brute_force(indexed(&tris), o, d, 1e9);
            let got = bvh.intersect(o, d, 1e-4, 1e9);
            match (want, got) {
                (None, None) => {}
                (Some((t, _)), Some(h)) => {
                    assert!((t - h.t).abs() <= 1e-3 * t.abs().max(1.0), "ray {r}: brute {t} vs bvh {}", h.t);
                    hits += 1;
                }
                (w, g) => panic!("ray {r}: brute {w:?} vs bvh {:?}", g.map(|h| (h.t, h.tri))),
            }
            checked += 1;
        }
        assert!(checked > 2500, "only {checked} rays tested");
        assert!(hits > 200, "only {hits} rays hit anything; the test scene is too sparse");
    }

    #[test]
    fn shadow_rays_agree_with_closest_hit() {
        let tris = random_tris(800);
        let bvh = Bvh::from_triangles(tris);
        for r in 0..2000u32 {
            let o = [rnd(r, 30) * 160.0 - 80.0, rnd(r, 31) * 160.0 - 80.0, rnd(r, 32) * 160.0 - 80.0];
            let dir = [rnd(r, 33) * 2.0 - 1.0, rnd(r, 34) * 2.0 - 1.0, rnd(r, 35) * 2.0 - 1.0];
            let len = dot(dir, dir).sqrt();
            if len < 1e-3 {
                continue;
            }
            let d = [dir[0] / len, dir[1] / len, dir[2] / len];
            let tmax = 20.0 + rnd(r, 36) * 120.0;
            let closest = bvh.intersect(o, d, 1e-4, tmax).is_some();
            assert_eq!(closest, bvh.occluded(o, d, 1e-4, tmax), "ray {r}");
        }
    }

    #[test]
    fn build_is_deterministic() {
        let tris = random_tris(1500);
        let a = Bvh::from_triangles(tris.clone());
        let b = Bvh::from_triangles(tris);
        assert_eq!(a.stats.nodes, b.stats.nodes);
        assert_eq!(a.stats.leaves, b.stats.leaves);
        assert_eq!(a.stats.max_depth, b.stats.max_depth);
        assert_eq!(a.len(), b.len());
        for ((i, x), (j, y)) in a.triangles().zip(b.triangles()) {
            assert_eq!(i, j);
            assert_eq!(x.p0, y.p0);
            assert_eq!(x.tri, y.tri);
        }
    }

    /// The SSE box test must be the *same* test, not a similar one: the
    /// whole claim that traversal is untouched rests on it, and the cases
    /// that could differ are exactly the ones that are hard to hit by
    /// accident — a ray parallel to an axis (`inv` is an infinity) starting
    /// exactly on a box plane, which makes `0 * inf` and so a NaN, in either
    /// the near or the far term, with the direction positive and negative.
    #[cfg(target_arch = "x86_64")]
    #[test]
    fn the_sse_box_test_is_the_scalar_one() {
        let boxes = [
            ([0.0f32, 0.0, 0.0], [1.0f32, 1.0, 1.0]),
            ([-3.0, 2.0, -1.5], [4.0, 2.0, 9.0]), // zero extent on y
            ([f32::INFINITY; 3], [f32::NEG_INFINITY; 3]), // an empty node
            ([-1e18, -1e18, -1e18], [1e18, 1e18, 1e18]),
        ];
        // Directions that give a zero (and a negative zero) component, so
        // `inv` is +inf and -inf, together with ordinary ones.
        let dirs = [
            [1.0f32, 0.0, 0.0],
            [-1.0, 0.0, 0.0],
            [0.0, 1.0, 0.0],
            [0.0, -0.0, 1.0],
            [0.0, 0.0, -0.0],
            [0.577, 0.577, 0.577],
            [-0.2, 0.9, -0.38],
        ];
        // Origins on a box plane (the NaN case), inside, and outside.
        let origins = [
            [0.0f32, 0.0, 0.0],
            [1.0, 1.0, 1.0],
            [0.5, 0.5, 0.5],
            [-5.0, 0.3, 0.7],
            [2.0, 2.0, -1.5],
            [-3.0, 2.0, 9.0],
        ];
        // The one difference the two are allowed is the *sign* of a zero
        // entry distance, which happens when `tmin` is 0 and a slab's near
        // plane is at -0.0: `f32::max` may keep either, and `maxps` keeps
        // the second operand. It cannot change anything downstream, because
        // -0.0 and 0.0 compare equal — the miss test is `t0 > t1` and the
        // child ordering is `ta <= tb`, and neither can tell them apart —
        // and the renderer never passes `tmin = 0` anyway.
        let bits = |v: Option<f32>| v.map(|t| if t == 0.0 { 0.0f32 } else { t }.to_bits());
        // All four boxes in one node, one per lane.
        let mut n = EMPTY_NODE4;
        for (slot, (lo, hi)) in boxes.iter().enumerate() {
            for axis in 0..3 {
                n.lo[axis][slot] = lo[axis];
                n.hi[axis][slot] = hi[axis];
            }
        }
        n.valid = 0b1111;
        let mut checked = 0;
        for d in dirs {
            for o in origins {
                for (tmin, tmax) in [(1e-4f32, 1e9f32), (0.0, 1.0), (2.0, 3.0), (1e-4, 0.5)] {
                    let (mask, near) = Ray4::new(o, d).test(&n, tmin, tmax);
                    for (slot, (lo, hi)) in boxes.iter().enumerate() {
                        let want = slab(*lo, *hi, o, [1.0 / d[0], 1.0 / d[1], 1.0 / d[2]], tmin, tmax);
                        let got = (mask & (1 << slot) != 0).then_some(near[slot]);
                        assert_eq!(
                            bits(want),
                            bits(got),
                            "box {lo:?}..{hi:?} ray {o:?} -> {d:?} in ({tmin}, {tmax}): scalar {want:?} sse {got:?}"
                        );
                        checked += 1;
                    }
                }
            }
        }
        assert!(checked > 600, "only {checked} cases");
    }

    /// And over a real tree, with real rays: same hits, same distances, same
    /// triangle indices, from both box tests.
    #[test]
    fn traversal_is_unchanged_by_the_box_test() {
        let tris = random_tris(2000);
        let bvh = Bvh::from_triangles(tris);
        let mut hits = 0;
        for r in 0..4000u32 {
            let o = [rnd(r, 40) * 200.0 - 100.0, rnd(r, 41) * 200.0 - 100.0, rnd(r, 42) * 200.0 - 100.0];
            // Every eighth ray is axis aligned, which is the case the box
            // test's infinities and NaNs come from.
            let dir = match r % 8 {
                0 => [1.0, 0.0, 0.0],
                4 => [0.0, 0.0, -1.0],
                _ => [rnd(r, 43) * 2.0 - 1.0, rnd(r, 44) * 2.0 - 1.0, rnd(r, 45) * 2.0 - 1.0],
            };
            let len = dot(dir, dir).sqrt();
            if len < 1e-3 {
                continue;
            }
            let d = [dir[0] / len, dir[1] / len, dir[2] / len];
            let got = bvh.intersect(o, d, 1e-4, 1e9);
            let want = brute_force(bvh.triangles(), o, d, 1e9);
            match (want, got) {
                (None, None) => {}
                (Some((t, i)), Some(h)) => {
                    assert_eq!(t.to_bits(), h.t.to_bits(), "ray {r}");
                    assert_eq!(i, h.tri, "ray {r}");
                    hits += 1;
                }
                (w, g) => panic!("ray {r}: brute {w:?} vs bvh {:?}", g.map(|h| (h.t, h.tri))),
            }
        }
        assert!(hits > 300, "only {hits} rays hit anything");
    }

    #[test]
    fn empty_scene_never_hits() {
        let bvh = Bvh::from_triangles(Vec::new());
        assert!(bvh.intersect([0.0; 3], [0.0, 0.0, 1.0], 1e-4, 1e9).is_none());
        assert!(!bvh.occluded([0.0; 3], [0.0, 0.0, 1.0], 1e-4, 1e9));
    }

    #[test]
    fn tree_is_shallow_and_leaves_are_small() {
        let tris = random_tris(4000);
        let n = tris.len();
        let bvh = Bvh::from_triangles(tris);
        // A SAH tree over n triangles is O(log n) deep, not O(n).
        assert!(bvh.stats.max_depth < 40, "depth {}", bvh.stats.max_depth);
        assert!(bvh.stats.leaves * LEAF_MAX >= n / 4, "{} leaves for {n} triangles", bvh.stats.leaves);
    }

    /// Traversal micro-benchmark over a real mesh, single threaded, so a
    /// change to the tree can be measured without the whole pipeline and
    /// without the other threads' worth of machine noise:
    ///
    ///   CADPOLE_BVH_STL=/path/to/mesh.stl bazel run --config=opt \
    ///     //src/rust/cadpole-core:cadpole_core_test -- --ignored bvh_bench --nocapture
    ///
    /// Three ray sets that stand in for what the path tracer casts: camera
    /// rays from an isometric direction over the whole bounding box, one
    /// bounce ray in a random direction from every primary hit, and one
    /// shadow ray toward a key light from every primary hit. Prints ns/ray
    /// and a checksum of the hits so two trees can be shown to agree.
    #[test]
    #[ignore]
    fn bvh_bench() {
        let Ok(path) = std::env::var("CADPOLE_BVH_STL") else {
            eprintln!("CADPOLE_BVH_STL not set; skipping");
            return;
        };
        let bytes = std::fs::read(&path).expect("read stl");
        let count = u32::from_le_bytes(bytes[80..84].try_into().unwrap()) as usize;
        let f = |o: usize| f32::from_le_bytes(bytes[o..o + 4].try_into().unwrap());
        let mut tris = Vec::with_capacity(count);
        for i in 0..count {
            let b = 84 + i * 50 + 12;
            let v = |k: usize| [f(b + k * 12), f(b + k * 12 + 4), f(b + k * 12 + 8)];
            let (p0, p1, p2) = (v(0), v(1), v(2));
            let (e1, e2) = (sub(p1, p0), sub(p2, p0));
            let n = cross(e1, e2);
            if dot(n, n) > 0.0 && dot(n, n).is_finite() {
                tris.push(Tri { p0, e1, e2, item: 0, tri: i as u32 });
            }
        }
        let input = tris.clone();
        let t0 = std::time::Instant::now();
        let bvh = Bvh::from_triangles(tris);
        let build_ms = t0.elapsed().as_secs_f64() * 1e3;
        let s = bvh.stats;
        eprintln!(
            "bvh_bench: {} tris, {} nodes, {} leaves ({:.2} tris/leaf), depth {}, build {:.1} ms",
            s.triangles,
            s.nodes,
            s.leaves,
            s.triangles as f64 / s.leaves.max(1) as f64,
            s.max_depth,
            build_ms
        );

        let mut lo = [f32::INFINITY; 3];
        let mut hi = [f32::NEG_INFINITY; 3];
        for (_, t) in bvh.triangles() {
            let b = t.bounds();
            for k in 0..3 {
                lo[k] = lo[k].min(b.lo[k]);
                hi[k] = hi[k].max(b.hi[k]);
            }
        }
        let c = [(lo[0] + hi[0]) * 0.5, (lo[1] + hi[1]) * 0.5, (lo[2] + hi[2]) * 0.5];
        let diag = dot(sub(hi, lo), sub(hi, lo)).sqrt();
        let eps = diag * 1e-4;
        let far = diag * 1e4;
        let norm = |v: [f32; 3]| {
            let l = dot(v, v).sqrt();
            [v[0] / l, v[1] / l, v[2] / l]
        };
        // Isometric camera: looking down at 35 degrees from the front-left.
        let d = norm([0.5, 0.5, -0.57]);
        let right = norm(cross(d, [0.0, 0.0, 1.0]));
        let up = cross(right, d);
        let eye = sub(c, [d[0] * diag * 2.0, d[1] * diag * 2.0, d[2] * diag * 2.0]);
        let n_rays = 400_000u32;
        let side = (n_rays as f32).sqrt() as u32;
        let mut primary = Vec::with_capacity(n_rays as usize);
        for i in 0..n_rays {
            let (gx, gy) = ((i % side) as f32 + rnd(i, 50), (i / side) as f32 + rnd(i, 51));
            let u = (gx / side as f32 - 0.5) * diag;
            let v = (gy / side as f32 - 0.5) * diag;
            let o = [
                eye[0] + right[0] * u + up[0] * v,
                eye[1] + right[1] * u + up[1] * v,
                eye[2] + right[2] * u + up[2] * v,
            ];
            primary.push((o, d));
        }
        fn time<F: FnMut([f32; 3], [f32; 3]) -> (bool, f32)>(name: &str, rays: &[([f32; 3], [f32; 3])], mut f: F) {
            let t0 = std::time::Instant::now();
            let mut hits = 0u32;
            let mut sum = 0.0f64;
            for &(o, d) in rays {
                let (h, t) = f(o, d);
                if h {
                    hits += 1;
                    sum += t as f64;
                }
            }
            let ns = t0.elapsed().as_nanos() as f64 / rays.len() as f64;
            eprintln!("bvh_bench: {name:>8}: {ns:8.1} ns/ray, {hits} hits of {}, checksum {sum:.3}", rays.len());
        }
        // Primary rays, and collect the hit points for the bounce and shadow sets.
        let mut secondary = Vec::new();
        let mut shadow = Vec::new();
        let key = norm([-0.4, -0.6, 0.7]);
        for (i, &(o, d)) in primary.iter().enumerate() {
            if let Some(h) = bvh.intersect(o, d, eps, far) {
                let tri = bvh.tri(h.tri);
                let p = tri.point(h.u, h.v);
                let mut n = norm(tri.geometric_normal());
                if dot(n, d) > 0.0 {
                    n = [-n[0], -n[1], -n[2]];
                }
                let po = [p[0] + n[0] * eps * 4.0, p[1] + n[1] * eps * 4.0, p[2] + n[2] * eps * 4.0];
                let i = i as u32;
                // Uniform direction on the hemisphere about `n`.
                let z = rnd(i, 60);
                let phi = rnd(i, 61) * std::f32::consts::TAU;
                let r = (1.0 - z * z).max(0.0).sqrt();
                let t = norm(cross(n, if n[0].abs() < 0.9 { [1.0, 0.0, 0.0] } else { [0.0, 1.0, 0.0] }));
                let b = cross(n, t);
                let dir = norm([
                    t[0] * r * phi.cos() + b[0] * r * phi.sin() + n[0] * z,
                    t[1] * r * phi.cos() + b[1] * r * phi.sin() + n[1] * z,
                    t[2] * r * phi.cos() + b[2] * r * phi.sin() + n[2] * z,
                ]);
                secondary.push((po, dir));
                // Toward the key light, jittered inside a 14 degree cone.
                let ct = 1.0 - rnd(i, 62) * (1.0 - 14f32.to_radians().cos());
                let st = (1.0 - ct * ct).max(0.0).sqrt();
                let ph = rnd(i, 63) * std::f32::consts::TAU;
                let kt = norm(cross(key, [0.0, 0.0, 1.0]));
                let kb = cross(key, kt);
                let sd = norm([
                    kt[0] * st * ph.cos() + kb[0] * st * ph.sin() + key[0] * ct,
                    kt[1] * st * ph.cos() + kb[1] * st * ph.sin() + key[1] * ct,
                    kt[2] * st * ph.cos() + kb[2] * st * ph.sin() + key[2] * ct,
                ]);
                let so = [p[0] + sd[0] * eps * 4.0, p[1] + sd[1] * eps * 4.0, p[2] + sd[2] * eps * 4.0];
                shadow.push((so, sd));
            }
        }
        // With `CADPOLE_BVH_CHECK` set, every hundredth primary ray is also
        // traced by brute force over the triangles that went *into* the
        // build — not the ones the tree holds, which is what catches a
        // packing that silently drops the fifth triangle of a leaf.
        if std::env::var("CADPOLE_BVH_CHECK").is_ok() {
            assert_eq!(bvh.triangles().count(), input.len(), "the tree does not hold every input triangle");
            let mut bad = 0usize;
            for (ri, &(o, d)) in primary.iter().enumerate().step_by(97) {
                let got = bvh.intersect(o, d, eps, far).map(|h| (h.t.to_bits(), h.u.to_bits(), h.v.to_bits()));
                let want = brute_force(indexed(&input), o, d, far).and_then(|(_, i)| {
                    intersect_tri(&input[i as usize], o, d, 1e-4, far).map(|(t, u, v)| (t.to_bits(), u.to_bits(), v.to_bits()))
                });
                if got != want {
                    bad += 1;
                    if bad <= 5 {
                        eprintln!("ray {ri}: o={o:?} d={d:?} want {want:?} got {got:?}");
                    }
                }
            }
            eprintln!("bvh_bench: {bad} of {} checked primary rays disagree with brute force", primary.len().div_ceil(97));
            assert_eq!(bad, 0);
        }
        for _round in 0..2 {
            time("primary", &primary, |o, d| bvh.intersect(o, d, eps, far).map(|h| (true, h.t)).unwrap_or((false, 0.0)));
            time("bounce", &secondary, |o, d| bvh.intersect(o, d, eps, far).map(|h| (true, h.t)).unwrap_or((false, 0.0)));
            time("occluded", &shadow, |o, d| (bvh.occluded(o, d, eps, far), 1.0));
            let opaque = [false];
            let mut buf = Vec::with_capacity(32);
            time("shadow", &shadow, |o, d| match bvh.shadow_hits(o, d, eps, far, &opaque, &mut buf, 32) {
                ShadowHits::Through => (!buf.is_empty(), buf.iter().map(|h| h.t).sum()),
                _ => (true, -1.0),
            });
        }
    }
}
