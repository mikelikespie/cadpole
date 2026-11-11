//! A constrained Delaunay triangulation in the plane, exact and dependency
//! free: the face step of the watertight tessellator (docs/design.md).
//!
//! **What it is.** Incremental Delaunay insertion with Lawson's edge flips
//! (a point splits the triangle it lands in, or the edge it lands on, and
//! the affected edges are flipped until they pass the incircle test), then
//! constraint recovery by Sloan's method (1993, *A fast algorithm for
//! generating constrained Delaunay triangulations*): the edges a constraint
//! crosses are flipped away one by one — an edge whose quadrilateral is not
//! convex waits its turn — until the constraint is an edge, and the
//! triangulation is made Delaunay again around it with the constraint
//! frozen. Both halves are the textbook ones; what matters here is that
//! nothing is ever *skipped*: a constraint that would have to cross another
//! constraint, or pass through a vertex, is an error that names both, never
//! a leaky boundary.
//!
//! **Why exact predicates by snapping.** `orient2d` and `incircle` decide
//! everything, and a wrong sign from floating-point cancellation is how a
//! triangulation comes back with an overlapping triangle or a flip loop that
//! never ends. Shewchuk's adaptive-precision predicates are the usual answer
//! and several hundred lines; the answer here is smaller. Coordinates are
//! snapped once, by the caller, to an integer grid of [`GRID`] cells across
//! the face's parameter box (a relative resolution of `2^-27`, some
//! `7e-9`), the super-triangle sits at `-GRID .. 4·GRID`, and with every
//! coordinate under `2^29` in magnitude `incircle`'s determinant fits in
//! `i128` with three bits to spare. The predicates are then integer
//! arithmetic: exact, and the same on every target, as anything that
//! decides a name must be.
//!
//! **Why a super-triangle.** Its three vertices carry every fan to
//! completion (no triangulation boundary to walk into), point location by
//! straight walking terminates on a Delaunay triangulation, and the outer
//! region it leaves behind is exactly what the caller's inside/outside
//! classification discards: [`Cdt::classify`] floods from a super-vertex
//! outward, toggling at every constraint edge, so the domain is whatever
//! the constraint loops enclose, however they are oriented, with holes as
//! holes (truck infers loop orientation in parameter space, and can get it
//! wrong).
//!
//! The triangulation is *not* a mesh of the face: the caller maps its
//! vertices to shared 3-D vertex ids afterwards, which is where a pole line
//! collapses and a seam closes (`crate::brepmesh`).

/// The number of grid cells the caller snaps a parameter box to, per axis.
/// Chosen so that the super-triangle's coordinates stay under `2^29` and
/// [`incircle`] fits in `i128`: coordinates in `[-GRID, 4·GRID]` differ by
/// under `2^30`, the lifted terms by under `2^61`, and the three products
/// of the determinant sum to under `2^124`.
pub const GRID: i64 = 1 << 27;

/// A snapped point: integer parameter coordinates.
pub type IPoint = [i64; 2];

const NONE: u32 = u32::MAX;

/// `> 0` when `c` lies to the left of `a → b` (the triangle is
/// counter-clockwise), `0` when collinear.
#[inline]
pub fn orient2d(a: IPoint, b: IPoint, c: IPoint) -> i128 {
    let (bax, bay) = ((b[0] - a[0]) as i128, (b[1] - a[1]) as i128);
    let (cax, cay) = ((c[0] - a[0]) as i128, (c[1] - a[1]) as i128);
    bax * cay - bay * cax
}

/// `> 0` when `d` lies strictly inside the circumcircle of the
/// counter-clockwise triangle `a b c`, `0` when cocircular.
#[inline]
pub fn incircle(a: IPoint, b: IPoint, c: IPoint, d: IPoint) -> i128 {
    let (adx, ady) = ((a[0] - d[0]) as i128, (a[1] - d[1]) as i128);
    let (bdx, bdy) = ((b[0] - d[0]) as i128, (b[1] - d[1]) as i128);
    let (cdx, cdy) = ((c[0] - d[0]) as i128, (c[1] - d[1]) as i128);
    let alift = adx * adx + ady * ady;
    let blift = bdx * bdx + bdy * bdy;
    let clift = cdx * cdx + cdy * cdy;
    alift * (bdx * cdy - cdx * bdy) + blift * (cdx * ady - adx * cdy) + clift * (adx * bdy - bdx * ady)
}

/// Why a constraint could not be recovered.
#[derive(Clone, Debug, PartialEq)]
pub enum CdtError {
    /// The segment `a → b` passes through the vertex `through`, so no single
    /// edge can carry it.
    VertexOnConstraint { a: u32, b: u32, through: u32 },
    /// The segment `a → b` crosses the constraint edge `crossed`.
    CrossesConstraint { a: u32, b: u32, crossed: (u32, u32) },
    /// A degenerate constraint: both ends are one vertex.
    Degenerate { a: u32 },
    /// The flip loop did not terminate, which the exact predicates should
    /// make impossible; reported rather than looped on.
    Stuck { a: u32, b: u32, why: &'static str },
}

impl std::fmt::Display for CdtError {
    fn fmt(&self, f: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
        match self {
            CdtError::VertexOnConstraint { a, b, through } => {
                write!(f, "the boundary segment {a}→{b} passes through vertex {through}")
            }
            CdtError::CrossesConstraint { a, b, crossed } => {
                write!(f, "the boundary segment {a}→{b} crosses the boundary segment {}→{}", crossed.0, crossed.1)
            }
            CdtError::Degenerate { a } => write!(f, "the boundary segment at vertex {a} has no length"),
            CdtError::Stuck { a, b, why } => write!(f, "recovering the boundary segment {a}→{b} did not terminate ({why})"),
        }
    }
}

#[derive(Clone, Copy, Debug)]
struct Tri {
    /// Counter-clockwise.
    v: [u32; 3],
    /// `n[i]` is the triangle across the edge opposite `v[i]`, i.e. the edge
    /// `(v[i+1], v[i+2])`; `NONE` outside the super-triangle.
    n: [u32; 3],
    /// `c[i]`: the edge opposite `v[i]` is a constraint.
    c: [bool; 3],
}

/// The triangulation. Vertices `0, 1, 2` are the super-triangle's.
#[derive(Clone, Debug)]
pub struct Cdt {
    pts: Vec<IPoint>,
    tris: Vec<Tri>,
    /// One triangle incident to each vertex.
    vtri: Vec<u32>,
    /// Where the next point location starts walking.
    last: u32,
    stack: Vec<(u32, u8)>,
}

/// Where a point lands.
#[derive(Clone, Copy, Debug, PartialEq)]
enum Location {
    Inside(u32),
    /// On the edge opposite vertex `i` of the triangle.
    OnEdge(u32, u8),
    OnVertex(u32),
}

impl Cdt {
    /// An empty triangulation over the super-triangle, ready for points in
    /// `[0, GRID]²`.
    pub fn new(capacity: usize) -> Cdt {
        let g = GRID;
        let pts = vec![[-g, -g], [4 * g, -g], [-g, 4 * g]];
        let tris = vec![Tri { v: [0, 1, 2], n: [NONE; 3], c: [false; 3] }];
        let mut cdt = Cdt { pts, tris, vtri: vec![0, 0, 0], last: 0, stack: Vec::new() };
        cdt.pts.reserve(capacity);
        cdt.tris.reserve(2 * capacity + 4);
        cdt.vtri.reserve(capacity);
        cdt
    }

    /// The point of vertex `v`.
    pub fn point(&self, v: u32) -> IPoint {
        self.pts[v as usize]
    }

    /// How many vertices, the super-triangle's three included.
    pub fn vertex_count(&self) -> usize {
        self.pts.len()
    }

    /// Whether `v` is one of the super-triangle's vertices.
    pub fn is_super(v: u32) -> bool {
        v < 3
    }

    #[inline]
    fn p(&self, v: u32) -> IPoint {
        self.pts[v as usize]
    }

    /// Insert a point, or find it: the vertex at exactly `p`, if one exists,
    /// is returned with `false`.
    pub fn insert(&mut self, p: IPoint) -> (u32, bool) {
        debug_assert!(p[0] >= 0 && p[0] <= GRID && p[1] >= 0 && p[1] <= GRID, "{p:?} outside the grid");
        match self.locate(p) {
            Location::OnVertex(v) => (v, false),
            Location::Inside(t) => {
                let v = self.push_vertex(p);
                self.split_triangle(t, v);
                self.legalize();
                (v, true)
            }
            Location::OnEdge(t, i) => {
                let v = self.push_vertex(p);
                self.split_edge(t, i, v);
                self.legalize();
                (v, true)
            }
        }
    }

    fn push_vertex(&mut self, p: IPoint) -> u32 {
        self.pts.push(p);
        self.vtri.push(NONE);
        (self.pts.len() - 1) as u32
    }

    /// Straight walk from the last triangle touched. The triangulation is
    /// Delaunay whenever this is called (points go in before constraints),
    /// so the walk terminates; a scan of every triangle is the guard for
    /// the case it does not.
    fn locate(&mut self, p: IPoint) -> Location {
        let mut t = if (self.last as usize) < self.tris.len() { self.last } else { 0 };
        let mut prev = NONE;
        let mut steps = 0usize;
        let limit = self.tris.len() * 2 + 16;
        loop {
            let tri = self.tris[t as usize];
            let mut inside = true;
            let mut zeros = [false; 3];
            let mut next = NONE;
            for i in 0..3 {
                let a = self.p(tri.v[(i + 1) % 3]);
                let b = self.p(tri.v[(i + 2) % 3]);
                let o = orient2d(a, b, p);
                if o < 0 {
                    // Prefer an edge that does not go back where we came from.
                    if tri.n[i] != prev || next == NONE {
                        next = tri.n[i];
                    }
                    inside = false;
                } else if o == 0 {
                    zeros[i] = true;
                }
            }
            if inside {
                self.last = t;
                let z = zeros.iter().filter(|&&z| z).count();
                return match z {
                    0 => Location::Inside(t),
                    1 => Location::OnEdge(t, zeros.iter().position(|&z| z).unwrap() as u8),
                    _ => {
                        // On two edges: the vertex they share, the one
                        // opposite the edge that is *not* zero.
                        let k = zeros.iter().position(|&z| !z).unwrap_or(0);
                        Location::OnVertex(tri.v[k])
                    }
                };
            }
            if next == NONE {
                break;
            }
            prev = t;
            t = next;
            steps += 1;
            if steps > limit {
                break;
            }
        }
        // The guard: scan.
        for (t, tri) in self.tris.iter().enumerate() {
            let a = self.p(tri.v[0]);
            let b = self.p(tri.v[1]);
            let c = self.p(tri.v[2]);
            let o0 = orient2d(b, c, p);
            let o1 = orient2d(c, a, p);
            let o2 = orient2d(a, b, p);
            if o0 >= 0 && o1 >= 0 && o2 >= 0 {
                self.last = t as u32;
                let zeros = [o0 == 0, o1 == 0, o2 == 0];
                let z = zeros.iter().filter(|&&z| z).count();
                return match z {
                    0 => Location::Inside(t as u32),
                    1 => Location::OnEdge(t as u32, zeros.iter().position(|&z| z).unwrap() as u8),
                    _ => Location::OnVertex(tri.v[zeros.iter().position(|&z| !z).unwrap_or(0)]),
                };
            }
        }
        unreachable!("a point in the grid lies in the super-triangle")
    }

    /// Point the neighbour across edge `i` of `t` back at `t`.
    fn link(&mut self, t: u32, i: usize) {
        let nb = self.tris[t as usize].n[i];
        if nb == NONE {
            return;
        }
        let (a, b) = (self.tris[t as usize].v[(i + 1) % 3], self.tris[t as usize].v[(i + 2) % 3]);
        let nt = &mut self.tris[nb as usize];
        for j in 0..3 {
            let (c, d) = (nt.v[(j + 1) % 3], nt.v[(j + 2) % 3]);
            if (c == b && d == a) || (c == a && d == b) {
                nt.n[j] = t;
                return;
            }
        }
        unreachable!("neighbour does not share the edge");
    }

    fn set_vtri(&mut self, t: u32) {
        for k in 0..3 {
            let v = self.tris[t as usize].v[k];
            self.vtri[v as usize] = t;
        }
    }

    /// `t = (a, b, c)` becomes `(a, b, v)`, `(b, c, v)`, `(c, a, v)`.
    fn split_triangle(&mut self, t: u32, v: u32) {
        let old = self.tris[t as usize];
        let [a, b, c] = old.v;
        let t1 = self.tris.len() as u32;
        let t2 = t1 + 1;
        // Edge opposite v[2] in each new triangle is the old outer edge.
        self.tris[t as usize] = Tri { v: [a, b, v], n: [t1, t2, old.n[2]], c: [false, false, old.c[2]] };
        self.tris.push(Tri { v: [b, c, v], n: [t2, t, old.n[0]], c: [false, false, old.c[0]] });
        self.tris.push(Tri { v: [c, a, v], n: [t, t1, old.n[1]], c: [false, false, old.c[1]] });
        self.link(t, 2);
        self.link(t1, 2);
        self.link(t2, 2);
        self.set_vtri(t);
        self.set_vtri(t1);
        self.set_vtri(t2);
        self.stack.extend([(t, 2), (t1, 2), (t2, 2)]);
        self.last = t;
    }

    /// `v` lies on the edge opposite `v[i]` of `t`: split `t` and its
    /// neighbour across that edge into two each.
    fn split_edge(&mut self, t: u32, i: u8, v: u32) {
        let i = i as usize;
        let old = self.tris[t as usize];
        let u = old.n[i];
        let apex = old.v[i];
        let a = old.v[(i + 1) % 3];
        let b = old.v[(i + 2) % 3];
        let constrained = old.c[i];
        // t = (apex, a, b) with v on a-b: (apex, a, v) and (apex, v, b).
        let t1 = self.tris.len() as u32;
        // Neighbours of the old t: across (a,b) = u; across (b,apex) = n[(i+1)%3]; across (apex,a) = n[(i+2)%3].
        let n_b_apex = old.n[(i + 1) % 3];
        let c_b_apex = old.c[(i + 1) % 3];
        let n_apex_a = old.n[(i + 2) % 3];
        let c_apex_a = old.c[(i + 2) % 3];
        // (apex, a, v): edges opp apex = (a, v) -> across is u's half (set below); opp a = (v, apex) -> t1; opp v = (apex, a) -> n_apex_a.
        self.tris[t as usize] = Tri { v: [apex, a, v], n: [NONE, t1, n_apex_a], c: [constrained, false, c_apex_a] };
        // (apex, v, b): opp apex = (v, b) -> u's half; opp v = (b, apex) -> n_b_apex; opp b = (apex, v) -> t.
        self.tris.push(Tri { v: [apex, v, b], n: [NONE, n_b_apex, t], c: [constrained, c_b_apex, false] });
        self.link(t, 2);
        self.link(t1, 1);
        if u == NONE {
            self.set_vtri(t);
            self.set_vtri(t1);
            self.stack.extend([(t, 2), (t1, 1)]);
            self.last = t;
            return;
        }
        let ou = self.tris[u as usize];
        // In u the shared edge runs b -> a; find its opposite vertex.
        let j = (0..3).find(|&j| ou.v[(j + 1) % 3] == b && ou.v[(j + 2) % 3] == a).expect("shared edge");
        let q = ou.v[j];
        let n_a_q = ou.n[(j + 1) % 3];
        let c_a_q = ou.c[(j + 1) % 3];
        let n_q_b = ou.n[(j + 2) % 3];
        let c_q_b = ou.c[(j + 2) % 3];
        let u1 = self.tris.len() as u32;
        // u = (q, b, a) with v on b-a: (q, b, v) and (q, v, a).
        // (q, b, v): opp q = (b, v) -> t1; opp b = (v, q) -> u1; opp v = (q, b) -> n_q_b.
        self.tris[u as usize] = Tri { v: [q, b, v], n: [t1, u1, n_q_b], c: [constrained, false, c_q_b] };
        // (q, v, a): opp q = (v, a) -> t; opp v = (a, q) -> n_a_q; opp a = (q, v) -> u.
        self.tris.push(Tri { v: [q, v, a], n: [t, n_a_q, u], c: [constrained, c_a_q, false] });
        self.tris[t as usize].n[0] = u1;
        self.tris[t1 as usize].n[0] = u;
        self.link(u, 2);
        self.link(u1, 1);
        self.set_vtri(t);
        self.set_vtri(t1);
        self.set_vtri(u);
        self.set_vtri(u1);
        self.stack.extend([(t, 2), (t1, 1), (u, 2), (u1, 1)]);
        self.last = t;
    }

    /// Flip the edge opposite `v[i]` of `t`. Returns the two triangles
    /// (`t`, `u`) that now share the new edge, and the new edge's ends.
    fn flip(&mut self, t: u32, i: usize) -> (u32, u32, u32, u32) {
        let ot = self.tris[t as usize];
        let u = ot.n[i];
        debug_assert!(u != NONE);
        let ou = self.tris[u as usize];
        let p = ot.v[i];
        let a = ot.v[(i + 1) % 3];
        let b = ot.v[(i + 2) % 3];
        let j = (0..3).find(|&j| ou.v[(j + 1) % 3] == b && ou.v[(j + 2) % 3] == a).expect("shared edge");
        let q = ou.v[j];
        // Old neighbours: t across (b,p) = ot.n[(i+1)%3], across (p,a) = ot.n[(i+2)%3];
        //                 u across (a,q) = ou.n[(j+1)%3], across (q,b) = ou.n[(j+2)%3].
        let (n_bp, c_bp) = (ot.n[(i + 1) % 3], ot.c[(i + 1) % 3]);
        let (n_pa, c_pa) = (ot.n[(i + 2) % 3], ot.c[(i + 2) % 3]);
        let (n_aq, c_aq) = (ou.n[(j + 1) % 3], ou.c[(j + 1) % 3]);
        let (n_qb, c_qb) = (ou.n[(j + 2) % 3], ou.c[(j + 2) % 3]);
        // New: t' = (p, a, q): opp p = (a,q) -> n_aq; opp a = (q,p) -> u; opp q = (p,a) -> n_pa.
        //      u' = (p, q, b): opp p = (q,b) -> n_qb; opp q = (b,p) -> n_bp; opp b = (p,q) -> t.
        self.tris[t as usize] = Tri { v: [p, a, q], n: [n_aq, u, n_pa], c: [c_aq, false, c_pa] };
        self.tris[u as usize] = Tri { v: [p, q, b], n: [n_qb, n_bp, t], c: [c_qb, c_bp, false] };
        self.link(t, 0);
        self.link(t, 2);
        self.link(u, 0);
        self.link(u, 1);
        self.set_vtri(t);
        self.set_vtri(u);
        (t, u, p, q)
    }

    /// Lawson's flips over the pending stack: an edge whose far vertex lies
    /// inside its triangle's circumcircle is flipped, and the two edges
    /// that then face the inserted vertex are pushed.
    fn legalize(&mut self) {
        while let Some((t, i)) = self.stack.pop() {
            let i = i as usize;
            let tri = self.tris[t as usize];
            let u = tri.n[i];
            if u == NONE || tri.c[i] {
                continue;
            }
            let a = tri.v[(i + 1) % 3];
            let b = tri.v[(i + 2) % 3];
            let ou = self.tris[u as usize];
            let Some(j) = (0..3).find(|&j| ou.v[(j + 1) % 3] == b && ou.v[(j + 2) % 3] == a) else { continue };
            let q = ou.v[j];
            if incircle(self.p(tri.v[0]), self.p(tri.v[1]), self.p(tri.v[2]), self.p(q)) > 0 {
                let (t2, u2, _, _) = self.flip(t, i);
                // The two edges opposite p in the new triangles: t' = (p, a, q) -> edge (a, q) is opp index 0;
                // u' = (p, q, b) -> edge (q, b) is opp index 0.
                self.stack.push((t2, 0));
                self.stack.push((u2, 0));
            }
        }
    }

    /// Lawson's flips over the pending stack for a triangulation whose
    /// non-Delaunay edges can be anywhere (after a constraint's recovery),
    /// not only around one inserted vertex: after a flip all four edges
    /// around the quad are pushed, not just the two facing a vertex. Every
    /// flip strictly raises the sorted vector of angles, so it ends; the
    /// guard is insurance.
    fn legalize_all(&mut self) {
        let mut guard = 0usize;
        while let Some((t, i)) = self.stack.pop() {
            guard += 1;
            if guard > 64 * (self.tris.len() + 16) {
                self.stack.clear();
                break;
            }
            let i = i as usize;
            let tri = self.tris[t as usize];
            let u = tri.n[i];
            if u == NONE || tri.c[i] {
                continue;
            }
            let a = tri.v[(i + 1) % 3];
            let b = tri.v[(i + 2) % 3];
            let ou = self.tris[u as usize];
            let Some(j) = (0..3).find(|&j| ou.v[(j + 1) % 3] == b && ou.v[(j + 2) % 3] == a) else { continue };
            let q = ou.v[j];
            if incircle(self.p(tri.v[0]), self.p(tri.v[1]), self.p(tri.v[2]), self.p(q)) > 0 && self.flippable(t, i) {
                let (t2, u2, _, _) = self.flip(t, i);
                // t2 = (p, a, q): (a, q) is opposite 0, (p, a) opposite 2;
                // u2 = (p, q, b): (q, b) is opposite 0, (b, p) opposite 1.
                self.stack.push((t2, 0));
                self.stack.push((t2, 2));
                self.stack.push((u2, 0));
                self.stack.push((u2, 1));
            }
        }
    }

    /// Whether a flip of the edge opposite `v[i]` of `t` is possible: the
    /// quadrilateral is strictly convex.
    fn flippable(&self, t: u32, i: usize) -> bool {
        let ot = self.tris[t as usize];
        let u = ot.n[i];
        if u == NONE {
            return false;
        }
        let ou = self.tris[u as usize];
        let p = ot.v[i];
        let a = ot.v[(i + 1) % 3];
        let b = ot.v[(i + 2) % 3];
        let Some(j) = (0..3).find(|&j| ou.v[(j + 1) % 3] == b && ou.v[(j + 2) % 3] == a) else { return false };
        let q = ou.v[j];
        // p and q on opposite sides of a-b is given; a and b must be on
        // opposite sides of p-q.
        let o1 = orient2d(self.p(p), self.p(q), self.p(a));
        let o2 = orient2d(self.p(p), self.p(q), self.p(b));
        (o1 > 0 && o2 < 0) || (o1 < 0 && o2 > 0)
    }

    /// The triangle with vertex `a` at some index `i`, and that index, for
    /// every triangle around `a` — in rotation order.
    fn fan(&self, a: u32) -> Vec<(u32, usize)> {
        let start = self.vtri[a as usize];
        let mut out = Vec::new();
        if start == NONE {
            return out;
        }
        // Counter-clockwise from `start`: cross the edge opposite v[i+1],
        // which contains a. A super-triangle vertex sits on the boundary,
        // so when that walk runs out the other way round is walked too.
        let mut t = start;
        let mut hit_boundary = false;
        loop {
            let tri = self.tris[t as usize];
            let Some(i) = tri.v.iter().position(|&v| v == a) else { break };
            out.push((t, i));
            let next = tri.n[(i + 1) % 3];
            if next == start {
                return out;
            }
            if next == NONE {
                hit_boundary = true;
                break;
            }
            t = next;
            if out.len() > self.tris.len() {
                return out;
            }
        }
        if hit_boundary {
            // Clockwise from `start`: cross the edge opposite v[i+2].
            let mut t = self.tris[start as usize].n[(out[0].1 + 2) % 3];
            while t != NONE {
                let tri = self.tris[t as usize];
                let Some(i) = tri.v.iter().position(|&v| v == a) else { break };
                out.push((t, i));
                t = tri.n[(i + 2) % 3];
                if out.len() > self.tris.len() {
                    break;
                }
            }
        }
        out
    }

    /// The triangle in which the edge `a → b` (in either direction) is the
    /// edge opposite index `i`, as `(t, i)`, searched around `a`.
    fn find_edge(&self, a: u32, b: u32) -> Option<(u32, usize)> {
        for (t, i) in self.fan(a) {
            let tri = self.tris[t as usize];
            let (x, y) = (tri.v[(i + 1) % 3], tri.v[(i + 2) % 3]);
            // Edges at a are (a, x) opposite y, and (y, a) opposite x.
            if x == b {
                return Some((t, (i + 2) % 3));
            }
            if y == b {
                return Some((t, (i + 1) % 3));
            }
        }
        None
    }

    fn set_constraint(&mut self, t: u32, i: usize, on: bool) {
        self.tris[t as usize].c[i] = on;
        let u = self.tris[t as usize].n[i];
        if u == NONE {
            return;
        }
        let (a, b) = (self.tris[t as usize].v[(i + 1) % 3], self.tris[t as usize].v[(i + 2) % 3]);
        let ou = self.tris[u as usize];
        if let Some(j) = (0..3).find(|&j| ou.v[(j + 1) % 3] == b && ou.v[(j + 2) % 3] == a) {
            self.tris[u as usize].c[j] = on;
        }
    }

    /// Whether the segment `a b` and the segment `c d` cross properly
    /// (interiors intersect, no endpoint shared or on the other).
    fn segments_cross(&self, a: u32, b: u32, c: u32, d: u32) -> bool {
        let (pa, pb, pc, pd) = (self.p(a), self.p(b), self.p(c), self.p(d));
        let o1 = orient2d(pa, pb, pc);
        let o2 = orient2d(pa, pb, pd);
        let o3 = orient2d(pc, pd, pa);
        let o4 = orient2d(pc, pd, pb);
        ((o1 > 0 && o2 < 0) || (o1 < 0 && o2 > 0)) && ((o3 > 0 && o4 < 0) || (o3 < 0 && o4 > 0))
    }

    /// Make `a → b` an edge of the triangulation and mark it a constraint.
    pub fn constrain(&mut self, a: u32, b: u32) -> Result<(), CdtError> {
        if a == b {
            return Err(CdtError::Degenerate { a });
        }
        if let Some((t, i)) = self.find_edge(a, b) {
            self.set_constraint(t, i, true);
            return Ok(());
        }
        let (pa, pb) = (self.p(a), self.p(b));
        // The triangle around `a` that the segment leaves through.
        let mut crossing: Vec<(u32, u32)> = Vec::new();
        let mut cur: Option<(u32, usize)> = None;
        for (t, i) in self.fan(a) {
            let tri = self.tris[t as usize];
            let c = tri.v[(i + 1) % 3];
            let d = tri.v[(i + 2) % 3];
            let oc = orient2d(pa, pb, self.p(c));
            let od = orient2d(pa, pb, self.p(d));
            // t = (a, c, d) counter-clockwise: the ray a -> b leaves
            // through (c, d) when c is right of it and d left.
            if oc < 0 && od > 0 {
                cur = Some((t, i));
                break;
            }
            // A fan vertex exactly on the ray, between a and b.
            for (v, o) in [(c, oc), (d, od)] {
                if o == 0 && v != a && v != b {
                    let pv = self.p(v);
                    let dot = (pv[0] - pa[0]) as i128 * (pb[0] - pa[0]) as i128 + (pv[1] - pa[1]) as i128 * (pb[1] - pa[1]) as i128;
                    let len2 = (pb[0] - pa[0]) as i128 * (pb[0] - pa[0]) as i128 + (pb[1] - pa[1]) as i128 * (pb[1] - pa[1]) as i128;
                    if dot > 0 && dot < len2 {
                        return Err(CdtError::VertexOnConstraint { a, b, through: v });
                    }
                }
            }
        }
        let Some((mut t, mut i)) = cur else {
            return Err(CdtError::Stuck { a, b, why: "no fan triangle faces the segment" });
        };
        // Walk to b, collecting the edges crossed.
        let mut guard = 0usize;
        loop {
            let tri = self.tris[t as usize];
            if tri.c[i] {
                return Err(CdtError::CrossesConstraint { a, b, crossed: (tri.v[(i + 1) % 3], tri.v[(i + 2) % 3]) });
            }
            let c = tri.v[(i + 1) % 3];
            let d = tri.v[(i + 2) % 3];
            crossing.push((c, d));
            let u = tri.n[i];
            if u == NONE {
                return Err(CdtError::Stuck { a, b, why: "the segment leaves the triangulation" });
            }
            let ou = self.tris[u as usize];
            let j = (0..3).find(|&j| ou.v[(j + 1) % 3] == d && ou.v[(j + 2) % 3] == c).expect("shared edge");
            let e = ou.v[j];
            if e == b {
                break;
            }
            let oe = orient2d(pa, pb, self.p(e));
            if oe == 0 {
                return Err(CdtError::VertexOnConstraint { a, b, through: e });
            }
            // u = (d, c, e) counter-clockwise, entered through (d, c): c is
            // right of a -> b and d left. If e is left too the segment exits
            // through (c, e), the edge opposite d; if e is right, through
            // (e, d), opposite c. Either way the next edge keeps its right
            // end at index i+1 and its left end at i+2.
            if oe > 0 {
                i = (0..3).find(|&k| ou.v[k] == d).unwrap();
            } else {
                i = (0..3).find(|&k| ou.v[k] == c).unwrap();
            }
            t = u;
            guard += 1;
            if guard > self.tris.len() {
                return Err(CdtError::Stuck { a, b, why: "the walk to b did not end" });
            }
        }
        // Flip the crossing edges away.
        let mut queue: std::collections::VecDeque<(u32, u32)> = crossing.into_iter().collect();
        let mut touched: Vec<u32> = Vec::new();
        let mut idle = 0usize;
        let mut total = 0usize;
        while let Some((c, d)) = queue.pop_front() {
            total += 1;
            if total > 64 * (self.tris.len() + 16) {
                return Err(CdtError::Stuck { a, b, why: "too many flips" });
            }
            let Some((t, i)) = self.find_edge(c, d) else { continue };
            if self.tris[t as usize].c[i] {
                return Err(CdtError::CrossesConstraint { a, b, crossed: (c, d) });
            }
            if !self.flippable(t, i) {
                queue.push_back((c, d));
                idle += 1;
                if idle > queue.len() + 1 {
                    return Err(CdtError::Stuck { a, b, why: "no crossing edge is flippable" });
                }
                continue;
            }
            idle = 0;
            let (ft, fu, p, q) = self.flip(t, i);
            touched.push(ft);
            touched.push(fu);
            if (p == a && q == b) || (p == b && q == a) {
                continue;
            }
            if self.segments_cross(a, b, p, q) {
                queue.push_back((p, q));
            }
        }
        let Some((t, i)) = self.find_edge(a, b) else {
            return Err(CdtError::Stuck { a, b, why: "the segment is still not an edge after the flips" });
        };
        self.set_constraint(t, i, true);
        // Restore the Delaunay property everywhere the flips reached, with
        // the constraint frozen: every edge of every triangle a flip
        // rewrote, and from there Lawson's rule. (Re-testing only the
        // flipped diagonals leaves a letter's outline —
        // hundreds of constraints on one face — with non-Delaunay slivers
        // all along it.)
        touched.sort_unstable();
        touched.dedup();
        for &t in &touched {
            for i in 0..3u8 {
                self.stack.push((t, i));
            }
        }
        self.legalize_all();
        Ok(())
    }

    /// Whether the edge `a b` exists and is a constraint.
    pub fn is_constrained(&self, a: u32, b: u32) -> bool {
        self.find_edge(a, b).is_some_and(|(t, i)| self.tris[t as usize].c[i])
    }

    /// Every triangle's side of the constraint loops: `true` for the
    /// triangles inside an odd number of them. Flooded from the outside
    /// (the super-triangle's corner), toggling at every constraint edge, so
    /// it needs no loop orientation and treats nested loops as holes.
    pub fn classify(&self) -> Vec<bool> {
        let n = self.tris.len();
        let mut state = vec![None::<bool>; n];
        let start = self.vtri[0];
        let mut stack = vec![(start, false)];
        while let Some((t, inside)) = stack.pop() {
            if t == NONE || state[t as usize].is_some() {
                continue;
            }
            state[t as usize] = Some(inside);
            let tri = self.tris[t as usize];
            for i in 0..3 {
                let u = tri.n[i];
                if u != NONE && state[u as usize].is_none() {
                    stack.push((u, inside ^ tri.c[i]));
                }
            }
        }
        state.into_iter().map(|s| s.unwrap_or(false)).collect()
    }

    /// The triangles, counter-clockwise, as vertex indices; `None` for none
    /// (the super-triangle's vertices are included where they occur).
    pub fn triangles(&self) -> impl Iterator<Item = [u32; 3]> + '_ {
        self.tris.iter().map(|t| t.v)
    }

    /// The number of triangles, the super-triangle's included.
    pub fn triangle_count(&self) -> usize {
        self.tris.len()
    }

    /// The triangle `t`'s vertices.
    pub fn triangle(&self, t: usize) -> [u32; 3] {
        self.tris[t].v
    }

    /// Whether the edge opposite `v[i]` of triangle `t` is a constraint.
    pub fn edge_constrained(&self, t: usize, i: usize) -> bool {
        self.tris[t].c[i]
    }

    /// Every triangle's edges' constraint flags, for callers that walk the
    /// triangulation: `(vertices, constrained-opposite-each)`.
    pub fn triangle_with_flags(&self, t: usize) -> ([u32; 3], [bool; 3]) {
        (self.tris[t].v, self.tris[t].c)
    }

    /// Check every invariant the structure has: neighbours point back,
    /// triangles are counter-clockwise, every vertex's recorded triangle
    /// contains it. For tests.
    #[cfg(test)]
    pub fn check(&self) {
        for (t, tri) in self.tris.iter().enumerate() {
            let o = orient2d(self.p(tri.v[0]), self.p(tri.v[1]), self.p(tri.v[2]));
            assert!(o > 0, "triangle {t} is not counter-clockwise: {:?}", tri.v);
            for i in 0..3 {
                let u = tri.n[i];
                if u == NONE {
                    continue;
                }
                let ou = &self.tris[u as usize];
                let (a, b) = (tri.v[(i + 1) % 3], tri.v[(i + 2) % 3]);
                let j = (0..3).find(|&j| ou.v[(j + 1) % 3] == b && ou.v[(j + 2) % 3] == a);
                let j = j.unwrap_or_else(|| panic!("triangle {t} edge {i} neighbour {u} does not share it"));
                assert_eq!(ou.n[j], t as u32, "neighbour {u} does not point back at {t}");
                assert_eq!(ou.c[j], tri.c[i], "constraint flag disagrees across {t}/{u}");
            }
        }
        for (v, &t) in self.vtri.iter().enumerate() {
            assert!(t != NONE, "vertex {v} has no triangle");
            assert!(self.tris[t as usize].v.contains(&(v as u32)), "vertex {v}'s triangle {t} does not contain it");
        }
    }

    /// Whether every non-constrained edge passes the incircle test. For tests.
    #[cfg(test)]
    pub fn is_delaunay(&self) -> bool {
        for tri in &self.tris {
            for i in 0..3 {
                let u = tri.n[i];
                if u == NONE || tri.c[i] {
                    continue;
                }
                let ou = &self.tris[u as usize];
                let (a, b) = (tri.v[(i + 1) % 3], tri.v[(i + 2) % 3]);
                let Some(j) = (0..3).find(|&j| ou.v[(j + 1) % 3] == b && ou.v[(j + 2) % 3] == a) else { return false };
                let q = ou.v[j];
                if incircle(self.p(tri.v[0]), self.p(tri.v[1]), self.p(tri.v[2]), self.p(q)) > 0 {
                    return false;
                }
            }
        }
        true
    }
}

/// Maps a real parameter box onto the integer grid, uniformly in both
/// directions so the triangulation's notion of "well shaped" is the
/// parameter domain's own.
#[derive(Clone, Copy, Debug)]
pub struct Snap {
    pub min: [f64; 2],
    pub scale: f64,
}

impl Snap {
    /// A snap over the box `lo .. hi`, with a margin so nothing lands on
    /// the grid's edge.
    pub fn over(lo: [f64; 2], hi: [f64; 2]) -> Snap {
        let ext = (hi[0] - lo[0]).max(hi[1] - lo[1]).max(1e-300);
        let margin = ext * 1e-3;
        let scale = (GRID as f64 - 2.0) / (ext + 2.0 * margin);
        Snap { min: [lo[0] - margin, lo[1] - margin], scale }
    }

    /// The grid point nearest `p`.
    pub fn snap(&self, p: [f64; 2]) -> IPoint {
        let x = ((p[0] - self.min[0]) * self.scale).round() as i64;
        let y = ((p[1] - self.min[1]) * self.scale).round() as i64;
        [x.clamp(0, GRID), y.clamp(0, GRID)]
    }

    /// A parameter distance in grid units.
    pub fn units(&self, d: f64) -> f64 {
        d * self.scale
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    /// A small deterministic generator (xorshift), so the tests need no crate.
    struct Rng(u64);
    impl Rng {
        fn next(&mut self) -> u64 {
            let mut x = self.0;
            x ^= x << 13;
            x ^= x >> 7;
            x ^= x << 17;
            self.0 = x;
            x
        }
        fn below(&mut self, n: i64) -> i64 {
            (self.next() % n as u64) as i64
        }
    }

    fn inside_count(cdt: &Cdt) -> usize {
        cdt.classify().iter().filter(|&&b| b).count()
    }

    #[test]
    fn random_points_make_a_delaunay_triangulation() {
        let mut rng = Rng(0x9e3779b97f4a7c15);
        let mut cdt = Cdt::new(500);
        let mut inserted = 0;
        for _ in 0..500 {
            let p = [rng.below(GRID + 1), rng.below(GRID + 1)];
            let (_, new) = cdt.insert(p);
            inserted += new as usize;
        }
        cdt.check();
        assert!(cdt.is_delaunay());
        // Euler: a triangulation of n points (plus the 3 super-vertices) as
        // the super-triangle's interior has 2n + 1 triangles.
        assert_eq!(cdt.triangle_count(), 2 * inserted + 1);
    }

    #[test]
    fn a_regular_grid_with_its_cocircular_quads_and_collinear_points() {
        let mut cdt = Cdt::new(121);
        let step = GRID / 10;
        for i in 0..=10 {
            for j in 0..=10 {
                cdt.insert([i * step, j * step]);
            }
        }
        cdt.check();
        assert!(cdt.is_delaunay());
        assert_eq!(cdt.triangle_count(), 2 * 121 + 1);
        // The same point again is found, not inserted.
        assert_eq!(cdt.insert([3 * step, 4 * step]).1, false);
        // A point on an edge splits it.
        let before = cdt.triangle_count();
        let (_, new) = cdt.insert([3 * step + step / 2, 4 * step]);
        assert!(new);
        assert_eq!(cdt.triangle_count(), before + 2);
        cdt.check();
        assert!(cdt.is_delaunay());
    }

    #[test]
    fn a_square_with_a_hole_is_recovered_and_classified() {
        let mut cdt = Cdt::new(64);
        let g = GRID;
        let outer: Vec<u32> = [[0, 0], [g, 0], [g, g], [0, g]].iter().map(|&p| cdt.insert(p).0).collect();
        let q = g / 4;
        let hole: Vec<u32> = [[q, q], [3 * q, q], [3 * q, 3 * q], [q, 3 * q]].iter().map(|&p| cdt.insert(p).0).collect();
        // A few interior points, some in the hole.
        let mut rng = Rng(7);
        for _ in 0..40 {
            cdt.insert([rng.below(g + 1), rng.below(g + 1)]);
        }
        for k in 0..4 {
            cdt.constrain(outer[k], outer[(k + 1) % 4]).unwrap();
            cdt.constrain(hole[k], hole[(k + 1) % 4]).unwrap();
        }
        cdt.check();
        assert!(cdt.is_delaunay(), "Delaunay away from the constraints");
        for k in 0..4 {
            assert!(cdt.is_constrained(outer[k], outer[(k + 1) % 4]));
            assert!(cdt.is_constrained(hole[(k + 1) % 4], hole[k]));
        }
        let inside = cdt.classify();
        // Every inside triangle lies in the square and outside the hole.
        let mut area = 0i128;
        for (t, &inn) in inside.iter().enumerate() {
            let v = cdt.triangle(t);
            let (a, b, c) = (cdt.point(v[0]), cdt.point(v[1]), cdt.point(v[2]));
            let cx = (a[0] + b[0] + c[0]) / 3;
            let cy = (a[1] + b[1] + c[1]) / 3;
            let in_square = cx > 0 && cx < g && cy > 0 && cy < g;
            let in_hole = cx > q && cx < 3 * q && cy > q && cy < 3 * q;
            assert_eq!(inn, in_square && !in_hole, "triangle {t} at ({cx}, {cy})");
            if inn {
                area += orient2d(a, b, c);
            }
        }
        // Twice the area: the square less the hole.
        assert_eq!(area, 2 * (g as i128 * g as i128 - (2 * q) as i128 * (2 * q) as i128));
        assert!(inside_count(&cdt) > 0);
    }

    #[test]
    fn a_constraint_through_a_vertex_and_one_crossing_another_are_refused_by_name() {
        let mut cdt = Cdt::new(16);
        let g = GRID;
        let a = cdt.insert([0, g / 2]).0;
        let m = cdt.insert([g / 2, g / 2]).0;
        let b = cdt.insert([g, g / 2]).0;
        assert_eq!(cdt.constrain(a, b), Err(CdtError::VertexOnConstraint { a, b, through: m }));
        // A vertical constraint, then a horizontal one across it.
        let c = cdt.insert([g / 3, 0]).0;
        let d = cdt.insert([g / 3, g]).0;
        cdt.constrain(c, d).unwrap();
        let e = cdt.insert([0, g / 4]).0;
        let f = cdt.insert([g, g / 4]).0;
        match cdt.constrain(e, f) {
            Err(CdtError::CrossesConstraint { crossed, .. }) => {
                assert!(crossed == (c, d) || crossed == (d, c), "{crossed:?}");
            }
            other => panic!("{other:?}"),
        }
        cdt.check();
    }

    #[test]
    fn a_thin_zigzag_boundary_is_recovered_through_many_flips() {
        // A polygon whose edges cross many Delaunay edges of a dense
        // point cloud: every edge must come back as a constraint.
        let mut cdt = Cdt::new(1200);
        let mut rng = Rng(42);
        for _ in 0..1000 {
            cdt.insert([rng.below(GRID + 1), rng.below(GRID + 1)]);
        }
        let g = GRID;
        let poly: Vec<IPoint> = (0..20)
            .map(|k| {
                let x = g / 20 + (g - g / 10) * k / 19;
                let y = if k % 2 == 0 { g / 10 } else { g - g / 10 };
                [x, y]
            })
            .collect();
        let ids: Vec<u32> = poly.iter().map(|&p| cdt.insert(p).0).collect();
        for k in 0..ids.len() {
            let (a, b) = (ids[k], ids[(k + 1) % ids.len()]);
            if cdt.constrain(a, b).is_err() {
                // A random point may sit exactly on the zigzag: allowed in
                // this test only where it does.
                continue;
            }
        }
        cdt.check();
        assert!(cdt.is_delaunay());
    }
}
