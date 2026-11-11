//! A binned-SAH bounding volume hierarchy over a triangle soup, with a
//! **closest point** query.
//!
//! The construction follows `cadpole_core::render::bvh`: same 16 centroid
//! bins on each axis, same surface-area-heuristic cost, same in-place
//! partition, so the tree is a pure function of the triangle list and is
//! identical on every machine. That module is private to the renderer and
//! answers a different question — "what does this ray hit" — with triangles
//! stored as an origin plus two edge vectors for Möller–Trumbore, so the
//! ~120 lines of SAH build are repeated here and the traversal is the one
//! this crate needs:
//!
//! * [`Bvh::closest`] — nearest point on the soup, with a distance cap so a
//!   narrow-band field stops descending once it is past the band;
//! * [`Bvh::ray_crossings`] — a parity count along an axis-aligned ray, the
//!   backstop the pseudo-normal sign test falls back on.
//!
//! If the two ever need to be one thing, the merge is mechanical: this file's
//! `Aabb`/`Node`/`build` are the same values under different names.

use crate::edges::{cross, dot, sub};

/// Triangles per leaf above which a split is attempted.
const LEAF_MAX: usize = 4;
/// Centroid bins per axis for the SAH sweep.
const BINS: usize = 16;
/// Traversal stack depth.
const MAX_STACK: usize = 64;

#[derive(Clone, Copy, Debug)]
pub struct Aabb {
    pub lo: [f64; 3],
    pub hi: [f64; 3],
}

impl Aabb {
    pub fn empty() -> Aabb {
        Aabb { lo: [f64::INFINITY; 3], hi: [f64::NEG_INFINITY; 3] }
    }

    fn add(&mut self, p: [f64; 3]) {
        for k in 0..3 {
            if p[k] < self.lo[k] {
                self.lo[k] = p[k];
            }
            if p[k] > self.hi[k] {
                self.hi[k] = p[k];
            }
        }
    }

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

    fn area(&self) -> f64 {
        let d = [self.hi[0] - self.lo[0], self.hi[1] - self.lo[1], self.hi[2] - self.lo[2]];
        if d[0] < 0.0 || d[1] < 0.0 || d[2] < 0.0 {
            return 0.0;
        }
        2.0 * (d[0] * d[1] + d[1] * d[2] + d[2] * d[0])
    }

    /// Squared distance from a point to the box; 0 inside.
    #[inline]
    fn dist2(&self, p: [f64; 3]) -> f64 {
        let mut s = 0.0;
        for k in 0..3 {
            let d = (self.lo[k] - p[k]).max(0.0).max(p[k] - self.hi[k]);
            s += d * d;
        }
        s
    }
}

#[derive(Clone, Copy)]
struct Node {
    lo: [f64; 3],
    hi: [f64; 3],
    first: u32,
    count: u32,
}

impl Node {
    #[inline]
    fn aabb(&self) -> Aabb {
        Aabb { lo: self.lo, hi: self.hi }
    }
}

/// A triangle, stored by its three corners (the closest-point query wants all
/// three, unlike the ray query the renderer's copy serves).
#[derive(Clone, Copy, Debug)]
pub struct Tri {
    pub p: [[f64; 3]; 3],
    /// Index in the source mesh, for provenance.
    pub index: u32,
}

impl Tri {
    fn centroid(&self) -> [f64; 3] {
        [
            (self.p[0][0] + self.p[1][0] + self.p[2][0]) / 3.0,
            (self.p[0][1] + self.p[1][1] + self.p[2][1]) / 3.0,
            (self.p[0][2] + self.p[1][2] + self.p[2][2]) / 3.0,
        ]
    }

    fn bounds(&self) -> Aabb {
        let mut b = Aabb::empty();
        for c in &self.p {
            b.add(*c);
        }
        b
    }

    pub fn normal(&self) -> [f64; 3] {
        cross(sub(self.p[1], self.p[0]), sub(self.p[2], self.p[0]))
    }
}

/// What the nearest-point query found.
#[derive(Clone, Copy, Debug)]
pub struct Closest {
    pub point: [f64; 3],
    pub dist2: f64,
    pub tri: u32,
    /// Barycentric-ish classification of where on the triangle the closest
    /// point landed: which vertex (`Vertex`), which edge (`Edge`), or the
    /// interior (`Face`). The pseudo-normal sign test needs this.
    pub site: Site,
}

#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub enum Site {
    Face,
    /// Edge `i` runs from corner `i` to corner `(i + 1) % 3`.
    Edge(u8),
    Vertex(u8),
}

pub struct Bvh {
    nodes: Vec<Node>,
    pub tris: Vec<Tri>,
}

impl Bvh {
    pub fn build(mut tris: Vec<Tri>) -> Bvh {
        let n = tris.len();
        let mut nodes: Vec<Node> = Vec::with_capacity((2 * n / LEAF_MAX).max(1) + 1);
        if n == 0 {
            nodes.push(Node { lo: [f64::INFINITY; 3], hi: [f64::NEG_INFINITY; 3], first: 0, count: 0 });
            return Bvh { nodes, tris };
        }
        let bounds: Vec<Aabb> = tris.iter().map(|t| t.bounds()).collect();
        let centroids: Vec<[f64; 3]> = tris.iter().map(|t| t.centroid()).collect();
        let mut order: Vec<u32> = (0..n as u32).collect();

        nodes.push(Node { lo: [0.0; 3], hi: [0.0; 3], first: 0, count: 0 });
        let mut stack: Vec<(usize, usize, usize, usize)> = vec![(0, 0, n, 0)];
        while let Some((ni, start, end, depth)) = stack.pop() {
            let mut bb = Aabb::empty();
            let mut cb = Aabb::empty();
            for &i in &order[start..end] {
                bb.union(&bounds[i as usize]);
                cb.add(centroids[i as usize]);
            }
            nodes[ni].lo = bb.lo;
            nodes[ni].hi = bb.hi;
            let count = end - start;
            if count <= LEAF_MAX || depth >= MAX_STACK - 2 {
                nodes[ni].first = start as u32;
                nodes[ni].count = count as u32;
                continue;
            }

            let mut best: Option<(usize, usize, f64)> = None;
            let parent_area = bb.area().max(1e-20);
            for axis in 0..3 {
                let lo = cb.lo[axis];
                let extent = cb.hi[axis] - lo;
                if !(extent > 1e-12) {
                    continue;
                }
                let k = BINS as f64 / extent;
                let mut bin_box = [Aabb::empty(); BINS];
                let mut bin_n = [0usize; BINS];
                for &i in &order[start..end] {
                    let b = bin_of(centroids[i as usize][axis], lo, k);
                    bin_box[b].union(&bounds[i as usize]);
                    bin_n[b] += 1;
                }
                let mut left_area = [0.0f64; BINS];
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
                    let cost = 0.125 + (la * ln as f64 + acc.area() * cnt as f64) / parent_area;
                    if best.map(|(_, _, c)| cost < c).unwrap_or(true) {
                        best = Some((axis, b, cost));
                    }
                }
            }

            let leaf_cost = count as f64;
            let Some((axis, split_bin, _)) = best.filter(|&(_, _, c)| c < leaf_cost || count > 16)
            else {
                nodes[ni].first = start as u32;
                nodes[ni].count = count as u32;
                continue;
            };

            let lo = cb.lo[axis];
            let k = BINS as f64 / (cb.hi[axis] - lo);
            let mut mid = start;
            {
                let mut scratch: Vec<u32> = Vec::with_capacity(count);
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
                mid = start + count / 2;
            }

            let left = nodes.len();
            nodes.push(Node { lo: [0.0; 3], hi: [0.0; 3], first: 0, count: 0 });
            nodes.push(Node { lo: [0.0; 3], hi: [0.0; 3], first: 0, count: 0 });
            nodes[ni].first = left as u32;
            nodes[ni].count = 0;
            stack.push((left + 1, mid, end, depth + 1));
            stack.push((left, start, mid, depth + 1));
        }

        let reordered: Vec<Tri> = order.iter().map(|&i| tris[i as usize]).collect();
        tris = reordered;
        Bvh { nodes, tris }
    }

    pub fn bounds(&self) -> Aabb {
        self.nodes.first().map(|n| n.aabb()).unwrap_or_else(Aabb::empty)
    }

    /// Nearest point on the soup to `p`, or `None` when nothing is within
    /// `max_dist`. Children are visited nearest first so the cap tightens
    /// early — this is the narrow band.
    pub fn closest(&self, p: [f64; 3], max_dist: f64) -> Option<Closest> {
        if self.tris.is_empty() {
            return None;
        }
        let mut best: Option<Closest> = None;
        let mut best_d2 = max_dist * max_dist;
        let mut stack: [(u32, f64); MAX_STACK] = [(0, 0.0); MAX_STACK];
        let mut sp = 0usize;
        stack[sp] = (0, self.nodes[0].aabb().dist2(p));
        sp += 1;
        while sp > 0 {
            sp -= 1;
            let (ni, d2) = stack[sp];
            if d2 > best_d2 {
                continue;
            }
            let node = self.nodes[ni as usize];
            if node.count > 0 {
                for t in node.first..node.first + node.count {
                    let tri = &self.tris[t as usize];
                    let (q, site) = closest_on_triangle(p, tri);
                    let d = sub(q, p);
                    let dd = dot(d, d);
                    if dd < best_d2 {
                        best_d2 = dd;
                        best = Some(Closest { point: q, dist2: dd, tri: t, site });
                    }
                }
                continue;
            }
            let (l, r) = (node.first, node.first + 1);
            let dl = self.nodes[l as usize].aabb().dist2(p);
            let dr = self.nodes[r as usize].aabb().dist2(p);
            // Push the farther child first so the nearer one pops next.
            let (first, fd, second, sd) =
                if dl <= dr { (r, dr, l, dl) } else { (l, dl, r, dr) };
            if fd <= best_d2 && sp < MAX_STACK {
                stack[sp] = (first, fd);
                sp += 1;
            }
            if sd <= best_d2 && sp < MAX_STACK {
                stack[sp] = (second, sd);
                sp += 1;
            }
        }
        best
    }

    /// How many triangles the ray from `p` along `+X` crosses. Odd means `p`
    /// is inside a closed surface. Returns `None` when the ray grazes a
    /// triangle edge, where the parity is not trustworthy.
    pub fn ray_crossings(&self, p: [f64; 3]) -> Option<u32> {
        let mut hits = 0u32;
        for t in &self.tris {
            // 2D point-in-triangle in the YZ plane, then check x.
            let (a, b, c) = (t.p[0], t.p[1], t.p[2]);
            let d0 = (b[1] - a[1]) * (p[2] - a[2]) - (b[2] - a[2]) * (p[1] - a[1]);
            let d1 = (c[1] - b[1]) * (p[2] - b[2]) - (c[2] - b[2]) * (p[1] - b[1]);
            let d2 = (a[1] - c[1]) * (p[2] - c[2]) - (a[2] - c[2]) * (p[1] - c[1]);
            let area = (b[1] - a[1]) * (c[2] - a[2]) - (b[2] - a[2]) * (c[1] - a[1]);
            if area.abs() < 1e-18 {
                continue; // edge-on to the ray: contributes nothing
            }
            let eps = 1e-12 * area.abs().max(1.0);
            if d0.abs() < eps || d1.abs() < eps || d2.abs() < eps {
                return None; // grazing
            }
            let inside = if area > 0.0 {
                d0 > 0.0 && d1 > 0.0 && d2 > 0.0
            } else {
                d0 < 0.0 && d1 < 0.0 && d2 < 0.0
            };
            if !inside {
                continue;
            }
            // Barycentric interpolation of x at (p.y, p.z).
            let (w0, w1, w2) = (d1 / area, d2 / area, d0 / area);
            let x = w0 * a[0] + w1 * b[0] + w2 * c[0];
            if x > p[0] {
                hits += 1;
            }
        }
        Some(hits)
    }
}

#[inline]
fn bin_of(v: f64, lo: f64, k: f64) -> usize {
    (((v - lo) * k) as isize).clamp(0, BINS as isize - 1) as usize
}

/// Closest point on a triangle to `p` (Ericson, *Real-Time Collision
/// Detection*, §5.1.5: the Voronoi-region case split), plus which feature of
/// the triangle it landed on.
pub fn closest_on_triangle(p: [f64; 3], t: &Tri) -> ([f64; 3], Site) {
    let (a, b, c) = (t.p[0], t.p[1], t.p[2]);
    let ab = sub(b, a);
    let ac = sub(c, a);
    let ap = sub(p, a);
    let d1 = dot(ab, ap);
    let d2 = dot(ac, ap);
    if d1 <= 0.0 && d2 <= 0.0 {
        return (a, Site::Vertex(0));
    }
    let bp = sub(p, b);
    let d3 = dot(ab, bp);
    let d4 = dot(ac, bp);
    if d3 >= 0.0 && d4 <= d3 {
        return (b, Site::Vertex(1));
    }
    let vc = d1 * d4 - d3 * d2;
    if vc <= 0.0 && d1 >= 0.0 && d3 <= 0.0 {
        let v = d1 / (d1 - d3);
        return ([a[0] + ab[0] * v, a[1] + ab[1] * v, a[2] + ab[2] * v], Site::Edge(0));
    }
    let cp = sub(p, c);
    let d5 = dot(ab, cp);
    let d6 = dot(ac, cp);
    if d6 >= 0.0 && d5 <= d6 {
        return (c, Site::Vertex(2));
    }
    let vb = d5 * d2 - d1 * d6;
    if vb <= 0.0 && d2 >= 0.0 && d6 <= 0.0 {
        let w = d2 / (d2 - d6);
        return ([a[0] + ac[0] * w, a[1] + ac[1] * w, a[2] + ac[2] * w], Site::Edge(2));
    }
    let va = d3 * d6 - d5 * d4;
    if va <= 0.0 && (d4 - d3) >= 0.0 && (d5 - d6) >= 0.0 {
        let w = (d4 - d3) / ((d4 - d3) + (d5 - d6));
        let bc = sub(c, b);
        return ([b[0] + bc[0] * w, b[1] + bc[1] * w, b[2] + bc[2] * w], Site::Edge(1));
    }
    let denom = 1.0 / (va + vb + vc);
    let v = vb * denom;
    let w = vc * denom;
    (
        [
            a[0] + ab[0] * v + ac[0] * w,
            a[1] + ab[1] * v + ac[1] * w,
            a[2] + ab[2] * v + ac[2] * w,
        ],
        Site::Face,
    )
}

#[cfg(test)]
mod tests {
    use super::*;
    use cadpole_core::fmath;

    fn unit_tri() -> Tri {
        Tri { p: [[0.0, 0.0, 0.0], [1.0, 0.0, 0.0], [0.0, 1.0, 0.0]], index: 0 }
    }

    #[test]
    fn closest_point_hits_every_voronoi_region() {
        let t = unit_tri();
        let cases: [([f64; 3], [f64; 3], Site); 7] = [
            ([0.2, 0.2, 5.0], [0.2, 0.2, 0.0], Site::Face),
            ([-1.0, -1.0, 0.0], [0.0, 0.0, 0.0], Site::Vertex(0)),
            ([3.0, -1.0, 0.0], [1.0, 0.0, 0.0], Site::Vertex(1)),
            ([-1.0, 3.0, 0.0], [0.0, 1.0, 0.0], Site::Vertex(2)),
            ([0.5, -2.0, 0.0], [0.5, 0.0, 0.0], Site::Edge(0)),
            ([1.0, 1.0, 0.0], [0.5, 0.5, 0.0], Site::Edge(1)),
            ([-2.0, 0.5, 0.0], [0.0, 0.5, 0.0], Site::Edge(2)),
        ];
        for (p, want, site) in cases {
            let (q, s) = closest_on_triangle(p, &t);
            for k in 0..3 {
                assert!((q[k] - want[k]).abs() < 1e-12, "{p:?} -> {q:?}, wanted {want:?}");
            }
            assert_eq!(s, site, "{p:?}");
        }
    }

    /// The tree must agree with brute force on every query, or the field is
    /// wrong in a way that is invisible until a mesh comes out dented.
    #[test]
    fn the_tree_agrees_with_brute_force() {
        // A coarse sphere: enough triangles to make the tree branch.
        let mut tris = Vec::new();
        let n = 12;
        for i in 0..n {
            for j in 0..n {
                let f = |a: usize, b: usize| {
                    let th = std::f64::consts::PI * a as f64 / n as f64;
                    let ph = std::f64::consts::TAU * b as f64 / n as f64;
                    [fmath::sin(th) * fmath::cos(ph) * 3.0, fmath::sin(th) * fmath::sin(ph) * 3.0, fmath::cos(th) * 3.0]
                };
                tris.push(Tri { p: [f(i, j), f(i + 1, j), f(i + 1, j + 1)], index: tris.len() as u32 });
                tris.push(Tri { p: [f(i, j), f(i + 1, j + 1), f(i, j + 1)], index: tris.len() as u32 });
            }
        }
        let flat = tris.clone();
        let bvh = Bvh::build(tris);
        for i in 0..40 {
            let a = i as f64 * 0.7;
            let p = [fmath::sin(a) * 5.0, fmath::cos(a * 1.3) * 4.0, fmath::sin(a * 0.5) * 6.0];
            let want = flat
                .iter()
                .map(|t| {
                    let (q, _) = closest_on_triangle(p, t);
                    let d = sub(q, p);
                    dot(d, d)
                })
                .fold(f64::INFINITY, f64::min);
            let got = bvh.closest(p, f64::INFINITY).expect("a hit").dist2;
            assert!((got - want).abs() < 1e-9, "{p:?}: {got} != {want}");
        }
        // The distance cap really caps.
        assert!(bvh.closest([100.0, 0.0, 0.0], 1.0).is_none());
    }
}
