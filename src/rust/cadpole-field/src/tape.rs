//! The SDF tape: a compiled straight-line program over signed distance.
//!
//! An expression graph ([`crate::Field`]) is compiled once into a flat
//! `Vec<Node>` in topological order — node `i` may only reference nodes
//! `< i` — plus a flat table of *spaces*. Evaluation is then a single forward
//! pass over a register file, with no pointer chasing, no recursion and no
//! allocation per sample (Keeter 2020 §3; the structure, not the code, since
//! libfive and fidget are MPL and we may not vendor them).
//!
//! **Spaces are how transforms stay flat.** A `translate` or a `twist` does
//! not become a node: it becomes an entry in [`Tape::spaces`], and every node
//! underneath it records the index of the space it is evaluated in. Space 0
//! is world. Evaluating at a point starts by walking the space table once,
//! turning the world point into one local point per space; evaluating over a
//! box turns the world box into one conservative local *box* per space. Both
//! passes are O(spaces), and the node loop afterwards never has to think
//! about coordinates again. A transform above a shared subtree duplicates the
//! subtree's nodes at the new space, which is what `compile` deduplicates on.
//!
//! **Two evaluators, one tape.** [`Evaluator::eval`] is plain `f64` and is
//! used where the surface is. [`Evaluator::eval_box`] is interval arithmetic
//! ([`crate::interval`]) and is used to throw whole octree cells away: a box
//! whose interval does not straddle zero cannot contain the surface. Every
//! interval rule is an inclusion function, so pruning can only ever be
//! conservative.
//!
//! Everything here is deterministic: no RNG (the noise field is an integer
//! hash of the lattice point), no parallelism, no hash iteration order in the
//! output.

use std::collections::HashMap;
use std::sync::Arc;

use crate::edges::EdgeBlendSet;
use crate::interval::{Box3, Interval};
use crate::mesh_sdf::MeshField;
use cadpole_core::fmath;

/// `1 / sqrt(2)`, the chamfer plane's slope factor.
pub(crate) const INV_SQRT2: f64 = std::f64::consts::FRAC_1_SQRT_2;

/// An affine map plus the bookkeeping the evaluators need.
#[derive(Clone, Copy, Debug)]
pub struct Affine {
    /// Rows of the 3x3 linear part of the *world to local* map.
    pub m: [[f64; 3]; 3],
    /// Translation of the world-to-local map: `local = m * world + t`.
    pub t: [f64; 3],
    /// How a distance measured in this local space converts back to world:
    /// `world_distance = local_distance * value_scale`. 1 for a rigid map.
    pub value_scale: f64,
}

impl Affine {
    pub const IDENTITY: Affine = Affine {
        m: [[1.0, 0.0, 0.0], [0.0, 1.0, 0.0], [0.0, 0.0, 1.0]],
        t: [0.0; 3],
        value_scale: 1.0,
    };

    /// World-to-local for "move the shape by `v`".
    pub fn translation(v: [f64; 3]) -> Affine {
        Affine { t: [-v[0], -v[1], -v[2]], ..Affine::IDENTITY }
    }

    /// World-to-local for "rotate the shape by `angle` radians about `axis`
    /// through the origin": the inverse rotation.
    pub fn rotation(axis: [f64; 3], angle: f64) -> Affine {
        let n = norm3(axis);
        let (s, c) = (fmath::sin(-angle), fmath::cos(-angle));
        let (x, y, z) = (n[0], n[1], n[2]);
        let k = 1.0 - c;
        Affine {
            m: [
                [c + x * x * k, x * y * k - z * s, x * z * k + y * s],
                [y * x * k + z * s, c + y * y * k, y * z * k - x * s],
                [z * x * k - y * s, z * y * k + x * s, c + z * z * k],
            ],
            t: [0.0; 3],
            value_scale: 1.0,
        }
    }

    /// World-to-local for "scale the shape uniformly by `s` about the origin".
    pub fn uniform_scale(s: f64) -> Affine {
        let inv = 1.0 / s;
        Affine {
            m: [[inv, 0.0, 0.0], [0.0, inv, 0.0], [0.0, 0.0, inv]],
            t: [0.0; 3],
            value_scale: s.abs(),
        }
    }

    /// World-to-local for "mirror the shape across the plane through the
    /// origin with normal `n`". An involution, so it is its own inverse.
    pub fn mirror(n: [f64; 3]) -> Affine {
        let n = norm3(n);
        let mut m = [[0.0; 3]; 3];
        for i in 0..3 {
            for j in 0..3 {
                m[i][j] = if i == j { 1.0 } else { 0.0 } - 2.0 * n[i] * n[j];
            }
        }
        Affine { m, t: [0.0; 3], value_scale: 1.0 }
    }

    #[inline]
    fn apply(&self, p: [f64; 3]) -> [f64; 3] {
        [
            self.m[0][0] * p[0] + self.m[0][1] * p[1] + self.m[0][2] * p[2] + self.t[0],
            self.m[1][0] * p[0] + self.m[1][1] * p[1] + self.m[1][2] * p[2] + self.t[1],
            self.m[2][0] * p[0] + self.m[2][1] * p[1] + self.m[2][2] * p[2] + self.t[2],
        ]
    }

    /// The image of an interval box under this affine map, as a box. Exact
    /// for an axis-aligned map, conservative otherwise (the usual
    /// centre-plus-absolute-half-extent rule).
    fn apply_box(&self, b: Box3) -> Box3 {
        let c = b.center();
        let h = [0.5 * b.x.width(), 0.5 * b.y.width(), 0.5 * b.z.width()];
        let cc = self.apply(c);
        let mut lo = [0.0; 3];
        let mut hi = [0.0; 3];
        for i in 0..3 {
            let e = self.m[i][0].abs() * h[0] + self.m[i][1].abs() * h[1] + self.m[i][2].abs() * h[2];
            lo[i] = cc[i] - e;
            hi[i] = cc[i] + e;
        }
        Box3::new(lo, hi)
    }

    /// Compose: `self` applied after `outer` (both world-to-local).
    fn compose(&self, outer: &Affine) -> Affine {
        let mut m = [[0.0; 3]; 3];
        for i in 0..3 {
            for j in 0..3 {
                m[i][j] = (0..3).map(|k| self.m[i][k] * outer.m[k][j]).sum();
            }
        }
        let t = self.apply(outer.t);
        Affine { m, t, value_scale: self.value_scale * outer.value_scale }
    }

    /// Bits, for deduplicating spaces.
    fn key(&self) -> [u64; 13] {
        let mut k = [0u64; 13];
        let mut i = 0;
        for r in &self.m {
            for v in r {
                k[i] = v.to_bits();
                i += 1;
            }
        }
        for v in &self.t {
            k[i] = v.to_bits();
            i += 1;
        }
        k[12] = self.value_scale.to_bits();
        k
    }
}

fn norm3(v: [f64; 3]) -> [f64; 3] {
    let l = (v[0] * v[0] + v[1] * v[1] + v[2] * v[2]).sqrt();
    if l == 0.0 { [0.0, 0.0, 1.0] } else { [v[0] / l, v[1] / l, v[2] / l] }
}

/// One step of the world-to-local chain.
#[derive(Clone, Copy, Debug)]
pub enum SpaceKind {
    Affine(Affine),
    /// Undo a twist of `rate` radians per unit of z about the Z axis.
    Twist { rate: f64 },
    /// Undo a linear taper: xy is divided by `s(z)`, which runs from `s0` at
    /// `z0` to `s1` at `z1` and is held constant outside that range.
    Taper { z0: f64, z1: f64, s0: f64, s1: f64 },
}

impl SpaceKind {
    fn key(&self) -> (u8, [u64; 13]) {
        match self {
            SpaceKind::Affine(a) => (0, a.key()),
            SpaceKind::Twist { rate } => {
                let mut k = [0u64; 13];
                k[0] = rate.to_bits();
                (1, k)
            }
            SpaceKind::Taper { z0, z1, s0, s1 } => {
                let mut k = [0u64; 13];
                k[0] = z0.to_bits();
                k[1] = z1.to_bits();
                k[2] = s0.to_bits();
                k[3] = s1.to_bits();
                (2, k)
            }
        }
    }

    #[inline]
    fn apply_point(&self, p: [f64; 3]) -> [f64; 3] {
        match self {
            SpaceKind::Affine(a) => a.apply(p),
            SpaceKind::Twist { rate } => {
                let a = -rate * p[2];
                let (s, c) = (fmath::sin(a), fmath::cos(a));
                [p[0] * c - p[1] * s, p[0] * s + p[1] * c, p[2]]
            }
            SpaceKind::Taper { z0, z1, s0, s1 } => {
                let s = taper_scale(p[2], *z0, *z1, *s0, *s1);
                [p[0] / s, p[1] / s, p[2]]
            }
        }
    }

    /// A box that is guaranteed to contain the image of `b`.
    fn apply_box(&self, b: Box3) -> Box3 {
        match self {
            SpaceKind::Affine(a) => a.apply_box(b),
            SpaceKind::Twist { rate } => {
                let (a0, a1) = {
                    let (u, v) = (-rate * b.z.lo, -rate * b.z.hi);
                    (u.min(v), u.max(v))
                };
                rotate_box_range(b, a0, a1)
            }
            SpaceKind::Taper { z0, z1, s0, s1 } => {
                let (mut lo, mut hi) = (f64::INFINITY, f64::NEG_INFINITY);
                for z in [b.z.lo, b.z.hi, *z0, *z1] {
                    if z >= b.z.lo && z <= b.z.hi {
                        let s = taper_scale(z, *z0, *z1, *s0, *s1);
                        lo = lo.min(s);
                        hi = hi.max(s);
                    }
                }
                if !lo.is_finite() {
                    lo = taper_scale(b.z.lo, *z0, *z1, *s0, *s1);
                    hi = lo;
                }
                let sx = div_by_positive(b.x, lo, hi);
                let sy = div_by_positive(b.y, lo, hi);
                Box3 { x: sx, y: sy, z: b.z }
            }
        }
    }

}

fn taper_scale(z: f64, z0: f64, z1: f64, s0: f64, s1: f64) -> f64 {
    let t = if (z1 - z0).abs() < 1e-12 { 0.0 } else { ((z - z0) / (z1 - z0)).clamp(0.0, 1.0) };
    let s = s0 + (s1 - s0) * t;
    if s.abs() < 1e-9 { 1e-9f64.copysign(if s == 0.0 { 1.0 } else { s }) } else { s }
}

/// `i / [lo, hi]` where the divisor is known positive.
fn div_by_positive(i: Interval, lo: f64, hi: f64) -> Interval {
    let cands = [i.lo / lo, i.lo / hi, i.hi / lo, i.hi / hi];
    let mut l = f64::INFINITY;
    let mut h = f64::NEG_INFINITY;
    for c in cands {
        l = l.min(c);
        h = h.max(c);
    }
    Interval::new(l, h)
}

/// A box containing the image of `b`'s xy rectangle swept through every
/// rotation angle in `[a0, a1]` about Z.
fn rotate_box_range(b: Box3, a0: f64, a1: f64) -> Box3 {
    use std::f64::consts::TAU;
    let mut lo = [f64::INFINITY; 2];
    let mut hi = [f64::NEG_INFINITY; 2];
    let mut rmax: f64 = 0.0;
    let corners = [(b.x.lo, b.y.lo), (b.x.hi, b.y.lo), (b.x.lo, b.y.hi), (b.x.hi, b.y.hi)];
    for (cx, cy) in corners {
        rmax = rmax.max(fmath::hypot(cx, cy));
        for a in [a0, a1] {
            let (s, c) = (fmath::sin(a), fmath::cos(a));
            let (x, y) = (cx * c - cy * s, cx * s + cy * c);
            lo[0] = lo[0].min(x);
            hi[0] = hi[0].max(x);
            lo[1] = lo[1].min(y);
            hi[1] = hi[1].max(y);
        }
    }
    // Any cardinal direction the sweep crosses can reach the outer radius.
    if a1 - a0 >= TAU {
        return Box3 { x: Interval::new(-rmax, rmax), y: Interval::new(-rmax, rmax), z: b.z };
    }
    for (cx, cy) in corners {
        let base = fmath::atan2(cy, cx);
        let r = fmath::hypot(cx, cy);
        for k in 0..4 {
            let target = k as f64 * std::f64::consts::FRAC_PI_2;
            // Smallest angle `a` in [a0, a1] with base + a == target (mod tau).
            let mut delta = target - base - a0;
            delta -= TAU * (delta / TAU).floor();
            if a0 + delta <= a1 {
                match k {
                    0 => hi[0] = hi[0].max(r),
                    1 => hi[1] = hi[1].max(r),
                    2 => lo[0] = lo[0].min(-r),
                    _ => lo[1] = lo[1].min(-r),
                }
            }
        }
    }
    Box3 { x: Interval::new(lo[0], hi[0]), y: Interval::new(lo[1], hi[1]), z: b.z }
}

#[derive(Clone, Copy, Debug)]
pub struct Space {
    pub parent: u32,
    pub kind: SpaceKind,
    /// Distance scale accumulated from world down to this space.
    pub accum_scale: f64,
}

/// Deterministic value noise: an integer hash of the lattice point, smoothly
/// interpolated. No RNG, no tables — the same `seed` gives the same field on
/// every machine, which is what a CAD document needs.
#[derive(Clone, Copy, Debug, PartialEq)]
pub struct NoiseSpec {
    /// Feature size in mm.
    pub scale: f64,
    /// Peak amplitude of the first octave, in mm.
    pub amplitude: f64,
    pub seed: u32,
    pub octaves: u32,
}

impl NoiseSpec {
    /// The largest value `|noise|` can take: the geometric sum of the octave
    /// amplitudes. Used as the interval bound and as the Lipschitz slack.
    pub fn bound(&self) -> f64 {
        let n = self.octaves.max(1);
        let mut a = self.amplitude.abs();
        let mut total = 0.0;
        for _ in 0..n {
            total += a;
            a *= 0.5;
        }
        total
    }

    fn eval(&self, p: [f64; 3]) -> f64 {
        let s = if self.scale.abs() < 1e-9 { 1e-9 } else { self.scale };
        let mut freq = 1.0 / s;
        let mut amp = self.amplitude;
        let mut acc = 0.0;
        for o in 0..self.octaves.max(1) {
            acc += amp * value_noise([p[0] * freq, p[1] * freq, p[2] * freq], self.seed.wrapping_add(o * 0x9e37));
            freq *= 2.0;
            amp *= 0.5;
        }
        acc
    }

}

#[inline]
fn hash_lattice(ix: i64, iy: i64, iz: i64, seed: u32) -> f64 {
    // A plain integer avalanche (splitmix-style constants). Deterministic and
    // platform-independent: wrapping integer arithmetic only.
    let mut h = (ix as u64).wrapping_mul(0x9e3779b97f4a7c15);
    h ^= (iy as u64).wrapping_mul(0xbf58476d1ce4e5b9);
    h = h.rotate_left(31);
    h ^= (iz as u64).wrapping_mul(0x94d049bb133111eb);
    h = h.rotate_left(17);
    h ^= seed as u64;
    h ^= h >> 30;
    h = h.wrapping_mul(0xbf58476d1ce4e5b9);
    h ^= h >> 27;
    h = h.wrapping_mul(0x94d049bb133111eb);
    h ^= h >> 31;
    // [0, 1) from the top 53 bits, then to [-1, 1].
    ((h >> 11) as f64 / (1u64 << 53) as f64) * 2.0 - 1.0
}

fn value_noise(p: [f64; 3], seed: u32) -> f64 {
    let f = [p[0].floor(), p[1].floor(), p[2].floor()];
    let i = [f[0] as i64, f[1] as i64, f[2] as i64];
    let t = [p[0] - f[0], p[1] - f[1], p[2] - f[2]];
    let s = |x: f64| x * x * (3.0 - 2.0 * x); // smoothstep
    let (u, v, w) = (s(t[0]), s(t[1]), s(t[2]));
    let mut acc = 0.0;
    for dz in 0..2 {
        for dy in 0..2 {
            for dx in 0..2 {
                let h = hash_lattice(i[0] + dx, i[1] + dy, i[2] + dz, seed);
                let wx = if dx == 0 { 1.0 - u } else { u };
                let wy = if dy == 0 { 1.0 - v } else { v };
                let wz = if dz == 0 { 1.0 - w } else { w };
                acc += h * wx * wy * wz;
            }
        }
    }
    acc
}

/// One instruction of the tape. Operand fields are indices of earlier nodes.
#[derive(Clone, Debug)]
pub enum Op {
    // ---- leaves (distance originates here; scaled by the space)
    Const(f64),
    Sphere { r: f64 },
    /// Axis-aligned box of half-extents `half`, edges rounded by `round`.
    Box { half: [f64; 3], round: f64 },
    /// Z-axis cylinder, radius `r`, half-height `hh`, rim rounded by `round`.
    Cylinder { r: f64, hh: f64, round: f64 },
    /// Z-axis capped cone: radius `r0` at `z = -hh`, `r1` at `z = +hh`.
    Cone { r0: f64, r1: f64, hh: f64 },
    Torus { major: f64, minor: f64 },
    /// Half-space `n . p <= d`; the distance is `n . p - d`.
    Plane { n: [f64; 3], d: f64 },
    Capsule { a: [f64; 3], b: [f64; 3], r: f64 },
    /// Narrow-band signed distance to a triangle soup (BVH + pseudo-normals).
    Mesh(u32),
    Noise(NoiseSpec),

    // ---- combinators (space-independent: operands carry their own space)
    Union(u32, u32),
    Intersect(u32, u32),
    /// `max(a, -b)`.
    Difference(u32, u32),
    /// Exact circular blends: on a pair of planar faces these are the
    /// rolling-ball fillet of radius `k`, not an approximation.
    SmoothUnion(u32, u32, f64),
    SmoothIntersect(u32, u32, f64),
    SmoothDifference(u32, u32, f64),
    ChamferUnion(u32, u32, f64),
    ChamferIntersect(u32, u32, f64),
    ChamferDifference(u32, u32, f64),
    /// `a - d`: grow (`d > 0`) or shrink the solid.
    Offset(u32, f64),
    /// `|a| - t/2`: a shell of wall thickness `t` centred on the surface.
    Shell(u32, f64),
    Negate(u32),
    /// `base - amplitude * source`: push the surface along its own normal.
    Displace { base: u32, source: u32, amplitude: f64 },
    /// `base` corrected near a set of selected edges — the fillet/chamfer
    /// engine. See [`crate::edges`].
    EdgeBlend { base: u32, set: u32 },
}

#[derive(Clone, Debug)]
pub struct Node {
    pub op: Op,
    pub space: u32,
}

/// A compiled field.
pub struct Tape {
    pub(crate) nodes: Vec<Node>,
    pub(crate) spaces: Vec<Space>,
    pub(crate) meshes: Vec<Arc<MeshField>>,
    pub(crate) blends: Vec<Arc<EdgeBlendSet>>,
    pub(crate) root: u32,
    /// Conservative world bounds of the solid, when they are finite.
    pub(crate) bounds: Option<Box3>,
}

impl Tape {
    pub fn node_count(&self) -> usize {
        self.nodes.len()
    }

    pub fn space_count(&self) -> usize {
        self.spaces.len()
    }

    /// A conservative axis-aligned box containing every point where the field
    /// is negative, or `None` when the expression is unbounded (a bare plane,
    /// a constant).
    pub fn bounds(&self) -> Option<Box3> {
        self.bounds
    }

    pub fn evaluator(&self) -> Evaluator<'_> {
        Evaluator {
            tape: self,
            regs: vec![0.0; self.nodes.len()],
            iregs: vec![Interval::point(0.0); self.nodes.len()],
            pts: vec![[0.0; 3]; self.spaces.len()],
            boxes: vec![Box3::point([0.0; 3]); self.spaces.len()],
        }
    }

    /// Convenience single-shot evaluation (tests and diagnostics; the mesher
    /// keeps one [`Evaluator`] and reuses its buffers).
    pub fn eval(&self, p: [f64; 3]) -> f64 {
        self.evaluator().eval(p)
    }

    pub fn eval_box(&self, b: Box3) -> Interval {
        self.evaluator().eval_box(b)
    }
}

/// Reusable scratch for evaluating one tape. Not `Sync`; make one per thread.
pub struct Evaluator<'a> {
    tape: &'a Tape,
    regs: Vec<f64>,
    iregs: Vec<Interval>,
    pts: Vec<[f64; 3]>,
    boxes: Vec<Box3>,
}

impl<'a> Evaluator<'a> {
    pub fn tape(&self) -> &'a Tape {
        self.tape
    }

    /// Signed distance at a point: negative inside the solid.
    pub fn eval(&mut self, p: [f64; 3]) -> f64 {
        let t = self.tape;
        self.pts[0] = p;
        for k in 1..t.spaces.len() {
            let s = &t.spaces[k];
            self.pts[k] = s.kind.apply_point(self.pts[s.parent as usize]);
        }
        for i in 0..t.nodes.len() {
            let n = &t.nodes[i];
            let lp = self.pts[n.space as usize];
            let scale = t.spaces[n.space as usize].accum_scale;
            self.regs[i] = eval_op(&n.op, lp, scale, &self.regs, t);
        }
        self.regs[t.root as usize]
    }

    /// The range of the field over a box. Guaranteed to contain the true
    /// range; a box whose result does not straddle zero has no surface in it.
    pub fn eval_box(&mut self, b: Box3) -> Interval {
        let t = self.tape;
        self.boxes[0] = b;
        for k in 1..t.spaces.len() {
            let s = &t.spaces[k];
            let parent = self.boxes[s.parent as usize];
            self.boxes[k] = s.kind.apply_box(parent);
        }
        for i in 0..t.nodes.len() {
            let n = &t.nodes[i];
            let lb = self.boxes[n.space as usize];
            let scale = t.spaces[n.space as usize].accum_scale;
            self.iregs[i] = eval_op_box(&n.op, lb, scale, &self.iregs, t);
        }
        self.iregs[t.root as usize]
    }

    /// Central-difference gradient, normalized. `h` is chosen from the cell
    /// size the caller is working at so the estimate is stable on the coarse
    /// grids as well as the fine ones.
    pub fn gradient(&mut self, p: [f64; 3], h: f64) -> [f64; 3] {
        let h = h.max(1e-9);
        let mut g = [0.0; 3];
        for k in 0..3 {
            let mut a = p;
            let mut b = p;
            a[k] -= h;
            b[k] += h;
            g[k] = (self.eval(b) - self.eval(a)) / (2.0 * h);
        }
        let l = (g[0] * g[0] + g[1] * g[1] + g[2] * g[2]).sqrt();
        if l < 1e-12 { [0.0, 0.0, 1.0] } else { [g[0] / l, g[1] / l, g[2] / l] }
    }
}

// ---------------------------------------------------------------- scalar ops

#[inline]
fn dot2(x: f64, y: f64) -> f64 {
    x * x + y * y
}

/// `smooth_intersect`: exact rolling-ball rounding of a convex edge between
/// two half-spaces. Derived, not copied: the solid eroded by `r` is
/// `{a + r <= 0} n {b + r <= 0}`, whose exact distance is the box formula,
/// and dilating by `r` subtracts `r`.
#[inline]
pub(crate) fn smooth_intersect(a: f64, b: f64, r: f64) -> f64 {
    if r <= 0.0 {
        return a.max(b);
    }
    let (u, v) = (a + r, b + r);
    (fmath::hypot(u.max(0.0), v.max(0.0))) + u.max(v).min(0.0) - r
}

/// The complement identity: rounding a concave edge adds material.
#[inline]
pub(crate) fn smooth_union(a: f64, b: f64, r: f64) -> f64 {
    -smooth_intersect(-a, -b, r)
}

/// Cut a convex edge with the angle-bisecting plane at setback `d` along
/// each face.
#[inline]
pub(crate) fn chamfer_intersect(a: f64, b: f64, d: f64) -> f64 {
    if d <= 0.0 {
        return a.max(b);
    }
    a.max(b).max((a + b + d) * INV_SQRT2)
}

#[inline]
pub(crate) fn chamfer_union(a: f64, b: f64, d: f64) -> f64 {
    -chamfer_intersect(-a, -b, d)
}

fn eval_op(op: &Op, p: [f64; 3], scale: f64, r: &[f64], t: &Tape) -> f64 {
    let s = |i: &u32| r[*i as usize];
    match op {
        Op::Const(v) => *v,
        Op::Sphere { r } => scale * ((p[0] * p[0] + p[1] * p[1] + p[2] * p[2]).sqrt() - r),
        Op::Box { half, round } => {
            let q = [
                p[0].abs() - (half[0] - round),
                p[1].abs() - (half[1] - round),
                p[2].abs() - (half[2] - round),
            ];
            let outside =
                (fmath::powi(q[0].max(0.0), 2) + fmath::powi(q[1].max(0.0), 2) + fmath::powi(q[2].max(0.0), 2)).sqrt();
            let inside = q[0].max(q[1]).max(q[2]).min(0.0);
            scale * (outside + inside - round)
        }
        Op::Cylinder { r, hh, round } => {
            let dxy = (p[0] * p[0] + p[1] * p[1]).sqrt() - (r - round);
            let dz = p[2].abs() - (hh - round);
            let outside = dot2(dxy.max(0.0), dz.max(0.0)).sqrt();
            scale * (outside + dxy.max(dz).min(0.0) - round)
        }
        Op::Cone { r0, r1, hh } => scale * capped_cone(p, *r0, *r1, *hh),
        Op::Torus { major, minor } => {
            let q = (p[0] * p[0] + p[1] * p[1]).sqrt() - major;
            scale * ((q * q + p[2] * p[2]).sqrt() - minor)
        }
        Op::Plane { n, d } => scale * (n[0] * p[0] + n[1] * p[1] + n[2] * p[2] - d),
        Op::Capsule { a, b, r } => {
            let pa = [p[0] - a[0], p[1] - a[1], p[2] - a[2]];
            let ba = [b[0] - a[0], b[1] - a[1], b[2] - a[2]];
            let bb = ba[0] * ba[0] + ba[1] * ba[1] + ba[2] * ba[2];
            let h = if bb <= 0.0 {
                0.0
            } else {
                ((pa[0] * ba[0] + pa[1] * ba[1] + pa[2] * ba[2]) / bb).clamp(0.0, 1.0)
            };
            let d = [pa[0] - ba[0] * h, pa[1] - ba[1] * h, pa[2] - ba[2] * h];
            scale * ((d[0] * d[0] + d[1] * d[1] + d[2] * d[2]).sqrt() - r)
        }
        Op::Mesh(i) => scale * t.meshes[*i as usize].signed_distance(p),
        Op::Noise(spec) => spec.eval(p),
        Op::Union(a, b) => s(a).min(s(b)),
        Op::Intersect(a, b) => s(a).max(s(b)),
        Op::Difference(a, b) => s(a).max(-s(b)),
        Op::SmoothUnion(a, b, k) => smooth_union(s(a), s(b), *k),
        Op::SmoothIntersect(a, b, k) => smooth_intersect(s(a), s(b), *k),
        Op::SmoothDifference(a, b, k) => smooth_intersect(s(a), -s(b), *k),
        Op::ChamferUnion(a, b, k) => chamfer_union(s(a), s(b), *k),
        Op::ChamferIntersect(a, b, k) => chamfer_intersect(s(a), s(b), *k),
        Op::ChamferDifference(a, b, k) => chamfer_intersect(s(a), -s(b), *k),
        Op::Offset(a, d) => s(a) - d,
        Op::Shell(a, th) => s(a).abs() - th * 0.5,
        Op::Negate(a) => -s(a),
        Op::Displace { base, source, amplitude } => s(base) - amplitude * s(source),
        Op::EdgeBlend { base, set } => t.blends[*set as usize].apply(s(base), p),
    }
}

/// iq's capped cone, re-derived: the distance to the nearer of the cap
/// segment and the slant segment, signed by which side of both we are on.
/// An exact (1-Lipschitz) signed distance.
fn capped_cone(p: [f64; 3], r0: f64, r1: f64, hh: f64) -> f64 {
    let q = [(p[0] * p[0] + p[1] * p[1]).sqrt(), p[2]];
    let k1 = [r1, hh];
    let k2 = [r1 - r0, 2.0 * hh];
    let cap_r = if q[1] < 0.0 { r0 } else { r1 };
    let ca = [q[0] - q[0].min(cap_r), q[1].abs() - hh];
    let den = dot2(k2[0], k2[1]).max(1e-30);
    let tclamp = (((k1[0] - q[0]) * k2[0] + (k1[1] - q[1]) * k2[1]) / den).clamp(0.0, 1.0);
    let cb = [q[0] - k1[0] + k2[0] * tclamp, q[1] - k1[1] + k2[1] * tclamp];
    let s = if cb[0] < 0.0 && ca[1] < 0.0 { -1.0 } else { 1.0 };
    s * dot2(ca[0], ca[1]).min(dot2(cb[0], cb[1])).sqrt()
}

// -------------------------------------------------------------- interval ops

fn smooth_intersect_iv(a: Interval, b: Interval, r: f64) -> Interval {
    if r <= 0.0 {
        return a.max(b);
    }
    let (u, v) = (a.add_scalar(r), b.add_scalar(r));
    Interval::length2(u.max0(), v.max0()).add(u.max(v).min0()).add_scalar(-r)
}

fn chamfer_intersect_iv(a: Interval, b: Interval, d: f64) -> Interval {
    if d <= 0.0 {
        return a.max(b);
    }
    a.max(b).max(a.add(b).add_scalar(d).mul_scalar(INV_SQRT2))
}

/// The interval of a 1-Lipschitz (exact) signed distance over a box, from one
/// sample at the centre: `[f(c) - R, f(c) + R]` where `R` is the box's half
/// diagonal. Correct for any exact SDF, and what the primitives without a
/// clean interval extension (the capped cone, the capsule, an imported mesh)
/// use.
#[inline]
fn lipschitz_iv(v: f64, radius: f64) -> Interval {
    Interval::new(v - radius, v + radius)
}

fn eval_op_box(op: &Op, b: Box3, scale: f64, r: &[Interval], t: &Tape) -> Interval {
    let s = |i: &u32| r[*i as usize];
    let rad = b.radius();
    match op {
        Op::Const(v) => Interval::point(*v),
        Op::Sphere { r } => Interval::length3(b.x, b.y, b.z).add_scalar(-r).mul_scalar(scale),
        Op::Box { half, round } => {
            let q = [
                b.x.abs().add_scalar(-(half[0] - round)),
                b.y.abs().add_scalar(-(half[1] - round)),
                b.z.abs().add_scalar(-(half[2] - round)),
            ];
            let outside = Interval::length3(q[0].max0(), q[1].max0(), q[2].max0());
            let inside = q[0].max(q[1]).max(q[2]).min0();
            outside.add(inside).add_scalar(-round).mul_scalar(scale)
        }
        Op::Cylinder { r, hh, round } => {
            let dxy = Interval::length2(b.x, b.y).add_scalar(-(r - round));
            let dz = b.z.abs().add_scalar(-(hh - round));
            let outside = Interval::length2(dxy.max0(), dz.max0());
            outside.add(dxy.max(dz).min0()).add_scalar(-round).mul_scalar(scale)
        }
        Op::Cone { r0, r1, hh } => {
            lipschitz_iv(capped_cone(b.center(), *r0, *r1, *hh), rad).mul_scalar(scale)
        }
        Op::Torus { major, minor } => {
            let q = Interval::length2(b.x, b.y).add_scalar(-major);
            Interval::length2(q, b.z).add_scalar(-minor).mul_scalar(scale)
        }
        Op::Plane { n, d } => b
            .x
            .mul_scalar(n[0])
            .add(b.y.mul_scalar(n[1]))
            .add(b.z.mul_scalar(n[2]))
            .add_scalar(-d)
            .mul_scalar(scale),
        Op::Capsule { .. } => {
            let c = b.center();
            lipschitz_iv(eval_op(op, c, 1.0, &[], t), rad).mul_scalar(scale)
        }
        Op::Mesh(i) => {
            let c = b.center();
            lipschitz_iv(t.meshes[*i as usize].signed_distance(c), rad).mul_scalar(scale)
        }
        // Value noise is bounded by the sum of its octave amplitudes. A
        // tighter bound would need the trilinear hull of the lattice cells the
        // box touches; the constant one is always valid and the displacement
        // node is the only consumer.
        Op::Noise(spec) => Interval::new(-spec.bound(), spec.bound()),
        Op::Union(a, b2) => s(a).min(s(b2)),
        Op::Intersect(a, b2) => s(a).max(s(b2)),
        Op::Difference(a, b2) => s(a).max(s(b2).neg()),
        Op::SmoothUnion(a, b2, k) => smooth_intersect_iv(s(a).neg(), s(b2).neg(), *k).neg(),
        Op::SmoothIntersect(a, b2, k) => smooth_intersect_iv(s(a), s(b2), *k),
        Op::SmoothDifference(a, b2, k) => smooth_intersect_iv(s(a), s(b2).neg(), *k),
        Op::ChamferUnion(a, b2, k) => chamfer_intersect_iv(s(a).neg(), s(b2).neg(), *k).neg(),
        Op::ChamferIntersect(a, b2, k) => chamfer_intersect_iv(s(a), s(b2), *k),
        Op::ChamferDifference(a, b2, k) => chamfer_intersect_iv(s(a), s(b2).neg(), *k),
        Op::Offset(a, d) => s(a).add_scalar(-d),
        Op::Shell(a, th) => s(a).abs().add_scalar(-th * 0.5),
        Op::Negate(a) => s(a).neg(),
        Op::Displace { base, source, amplitude } => s(base).sub(s(source).mul_scalar(*amplitude)),
        Op::EdgeBlend { base, set } => s(base).add(t.blends[*set as usize].correction_bound(b)),
    }
}

// -------------------------------------------------------------- the compiler

/// Builds a [`Tape`] from an expression tree, deduplicating identical
/// (op, operands, space) triples so a shared subtree is emitted once per
/// space it is used in.
pub(crate) struct Builder {
    nodes: Vec<Node>,
    spaces: Vec<Space>,
    meshes: Vec<Arc<MeshField>>,
    blends: Vec<Arc<EdgeBlendSet>>,
    dedup: HashMap<String, u32>,
    space_dedup: HashMap<(u32, u8, [u64; 13]), u32>,
}

impl Builder {
    pub(crate) fn new() -> Builder {
        Builder {
            nodes: Vec::new(),
            spaces: vec![Space {
                parent: 0,
                kind: SpaceKind::Affine(Affine::IDENTITY),
                accum_scale: 1.0,
            }],
            meshes: Vec::new(),
            blends: Vec::new(),
            dedup: HashMap::new(),
            space_dedup: HashMap::new(),
        }
    }

    /// Push a space under `parent`. Consecutive affine steps are composed so
    /// a chain of translates costs one space, not five.
    pub(crate) fn space(&mut self, parent: u32, kind: SpaceKind) -> u32 {
        let (parent, kind) = match (&self.spaces[parent as usize].kind, kind) {
            (SpaceKind::Affine(outer), SpaceKind::Affine(inner)) if parent != 0 => {
                (self.spaces[parent as usize].parent, SpaceKind::Affine(inner.compose(outer)))
            }
            (_, k) => (parent, k),
        };
        let (tag, bits) = kind.key();
        if let Some(&i) = self.space_dedup.get(&(parent, tag, bits)) {
            return i;
        }
        let extra = match &kind {
            SpaceKind::Affine(a) => a.value_scale,
            _ => 1.0,
        };
        let accum_scale = self.spaces[parent as usize].accum_scale * extra;
        self.spaces.push(Space { parent, kind, accum_scale });
        let i = self.spaces.len() as u32 - 1;
        self.space_dedup.insert((parent, tag, bits), i);
        i
    }

    pub(crate) fn mesh(&mut self, m: Arc<MeshField>) -> u32 {
        for (i, e) in self.meshes.iter().enumerate() {
            if Arc::ptr_eq(e, &m) {
                return i as u32;
            }
        }
        self.meshes.push(m);
        self.meshes.len() as u32 - 1
    }

    pub(crate) fn blend(&mut self, s: Arc<EdgeBlendSet>) -> u32 {
        for (i, e) in self.blends.iter().enumerate() {
            if Arc::ptr_eq(e, &s) {
                return i as u32;
            }
        }
        self.blends.push(s);
        self.blends.len() as u32 - 1
    }

    pub(crate) fn push(&mut self, op: Op, space: u32) -> u32 {
        let key = format!("{space}|{op:?}");
        if let Some(&i) = self.dedup.get(&key) {
            return i;
        }
        self.nodes.push(Node { op, space });
        let i = self.nodes.len() as u32 - 1;
        self.dedup.insert(key, i);
        i
    }

    /// Drop the spaces nothing ends up referring to (composition leaves the
    /// intermediate steps of a transform chain orphaned) and renumber, so
    /// every evaluation walks exactly the spaces it needs.
    pub(crate) fn finish(mut self, root: u32, bounds: Option<Box3>) -> Tape {
        let mut used = vec![false; self.spaces.len()];
        used[0] = true;
        for n in &self.nodes {
            used[n.space as usize] = true;
        }
        // Spaces are stored parent-before-child, so one backward sweep marks
        // every ancestor of a used space.
        for i in (1..self.spaces.len()).rev() {
            if used[i] {
                used[self.spaces[i].parent as usize] = true;
            }
        }
        let mut remap = vec![0u32; self.spaces.len()];
        let mut kept: Vec<Space> = Vec::with_capacity(self.spaces.len());
        for (i, s) in self.spaces.iter().enumerate() {
            if !used[i] {
                continue;
            }
            remap[i] = kept.len() as u32;
            kept.push(Space { parent: remap[s.parent as usize], ..*s });
        }
        for n in self.nodes.iter_mut() {
            n.space = remap[n.space as usize];
        }
        Tape {
            nodes: self.nodes,
            spaces: kept,
            meshes: self.meshes,
            blends: self.blends,
            root,
            bounds,
        }
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::Field;

    fn brute_force_range(t: &Tape, b: Box3, n: usize) -> (f64, f64) {
        let mut ev = t.evaluator();
        let (mut lo, mut hi) = (f64::INFINITY, f64::NEG_INFINITY);
        for i in 0..=n {
            for j in 0..=n {
                for k in 0..=n {
                    let p = [
                        b.x.lo + b.x.width() * i as f64 / n as f64,
                        b.y.lo + b.y.width() * j as f64 / n as f64,
                        b.z.lo + b.z.width() * k as f64 / n as f64,
                    ];
                    let v = ev.eval(p);
                    lo = lo.min(v);
                    hi = hi.max(v);
                }
            }
        }
        (lo, hi)
    }

    /// The property the mesher's correctness rests on: the interval
    /// evaluation must never be narrower than the true range, or pruning
    /// would drop geometry.
    #[test]
    fn interval_evaluation_encloses_the_sampled_range() {
        let cases: Vec<(&str, Field)> = vec![
            ("sphere", Field::sphere(4.0)),
            ("rounded box", Field::boxed([6.0, 3.0, 2.0], 0.8)),
            ("cylinder", Field::cylinder(3.0, 7.0, 0.4)),
            ("cone", Field::cone(4.0, 1.0, 5.0)),
            ("torus", Field::torus(5.0, 1.5)),
            ("plane", Field::half_space([0.3, 0.7, 0.2], 1.0)),
            ("capsule", Field::capsule([-2.0, 0.0, 0.0], [2.0, 1.0, 1.0], 1.2)),
            (
                "smooth union",
                Field::sphere(3.0).translate([-2.0, 0.0, 0.0]).smooth_union(
                    &Field::sphere(3.0).translate([2.0, 0.0, 0.0]),
                    1.0,
                ),
            ),
            (
                "difference",
                Field::boxed([4.0, 4.0, 4.0], 0.0).subtract(&Field::sphere(4.5)),
            ),
            ("shell", Field::sphere(5.0).shell(1.0)),
            ("offset", Field::boxed([3.0, 3.0, 3.0], 0.0).offset(0.7)),
            (
                "chamfer",
                Field::boxed([3.0, 3.0, 3.0], 0.0).chamfer_intersect(&Field::sphere(4.0), 0.9),
            ),
            ("twist", Field::boxed([4.0, 1.0, 5.0], 0.2).twist(0.15)),
            ("taper", Field::cylinder(3.0, 5.0, 0.0).taper(-5.0, 5.0, 1.0, 0.4)),
            (
                "displaced",
                Field::sphere(5.0).displace(&Field::noise(3.0, 1.0, 7, 2), 1.0),
            ),
            (
                "rotated + scaled",
                Field::boxed([3.0, 1.0, 1.0], 0.0).rotate([0.0, 0.0, 1.0], 0.6).scale(1.7),
            ),
        ];
        let boxes = [
            Box3::new([-1.0, -1.0, -1.0], [1.0, 1.0, 1.0]),
            Box3::new([2.0, -3.0, 0.5], [4.0, -1.0, 3.5]),
            Box3::new([-8.0, -8.0, -8.0], [8.0, 8.0, 8.0]),
            Box3::new([3.9, 0.0, 0.0], [4.1, 0.2, 0.2]),
        ];
        for (name, f) in &cases {
            let t = f.compile();
            for b in boxes {
                let iv = t.eval_box(b);
                let (lo, hi) = brute_force_range(&t, b, 9);
                assert!(
                    iv.lo <= lo + 1e-9 && iv.hi >= hi - 1e-9,
                    "{name}: interval {iv:?} does not enclose sampled range [{lo}, {hi}] over {b:?}"
                );
            }
        }
    }

    #[test]
    fn the_exact_smooth_intersection_rounds_a_right_angle_by_r() {
        // Solid {x >= 0, y >= 0}, so a = -x and b = -y. Rounding the convex
        // corner by r puts the surface on the arc of radius r centred at
        // (r, r), where the exact signed distance is |p - c| - r.
        let r = 2.0;
        for (x, y) in [(0.0, 0.0), (0.5, 0.5), (1.0, 1.9), (1.9, 1.0)] {
            let got = smooth_intersect(-x, -y, r);
            let want = fmath::hypot(r - x, r - y) - r;
            assert!((got - want).abs() < 1e-12, "({x},{y}): {got} != {want}");
        }
        // Points on the arc itself are exactly on the surface.
        for k in 0..=8 {
            let a = std::f64::consts::FRAC_PI_2 * k as f64 / 8.0;
            let (x, y) = (r - r * fmath::cos(a), r - r * fmath::sin(a));
            assert!(smooth_intersect(-x, -y, r).abs() < 1e-12, "({x},{y}) is on the fillet arc");
        }
        // Beyond the rounded region, and deep inside, it is the plain max.
        assert!((smooth_intersect(-3.0, -0.5, r) - (-0.5)).abs() < 1e-12);
        assert!((smooth_intersect(-5.0, -3.0, 1.0) - (-3.0)).abs() < 1e-12);
    }

    #[test]
    fn the_chamfer_cuts_at_the_requested_setback() {
        let d = 3.0;
        // Solid {x >= 0, y >= 0}: a = -x, b = -y.
        // The chamfer plane must pass through (0, d) and (d, 0).
        for (x, y) in [(0.0, d), (d, 0.0), (d / 2.0, d / 2.0)] {
            let v = chamfer_intersect(-x, -y, d);
            assert!(v.abs() < 1e-12, "({x},{y}) should be on the chamfer: {v}");
        }
        // The old corner is gone.
        assert!(chamfer_intersect(0.0, 0.0, d) > 0.0);
    }

    #[test]
    fn spaces_compose_so_a_chain_of_translates_is_one_space() {
        let f = Field::sphere(1.0).translate([1.0, 0.0, 0.0]).translate([0.0, 2.0, 0.0]);
        let t = f.compile();
        // World + one composed affine.
        assert_eq!(t.space_count(), 2, "{} spaces", t.space_count());
        assert!((t.eval([1.0, 2.0, 0.0]) + 1.0).abs() < 1e-12);
    }

    #[test]
    fn identical_subtrees_are_emitted_once() {
        let s = Field::sphere(2.0);
        let f = s.union(&s).union(&s);
        let t = f.compile();
        // One sphere node plus two unions.
        assert_eq!(t.node_count(), 3, "{:?}", t.nodes);
    }

    #[test]
    fn value_noise_is_deterministic_and_bounded() {
        let spec = NoiseSpec { scale: 4.0, amplitude: 1.0, seed: 11, octaves: 3 };
        let p = [1.25, -3.5, 0.75];
        let a = spec.eval(p);
        let b = spec.eval(p);
        assert_eq!(a.to_bits(), b.to_bits());
        for i in 0..200 {
            let q = [i as f64 * 0.37, i as f64 * -0.21, i as f64 * 0.11];
            assert!(spec.eval(q).abs() <= spec.bound() + 1e-12);
        }
    }

    #[test]
    fn the_twist_box_bound_contains_the_twisted_points() {
        let k = SpaceKind::Twist { rate: 0.2 };
        let b = Box3::new([2.0, -1.0, 0.0], [5.0, 3.0, 9.0]);
        let out = k.apply_box(b);
        let n = 12;
        for i in 0..=n {
            for j in 0..=n {
                for l in 0..=n {
                    let p = [
                        b.x.lo + b.x.width() * i as f64 / n as f64,
                        b.y.lo + b.y.width() * j as f64 / n as f64,
                        b.z.lo + b.z.width() * l as f64 / n as f64,
                    ];
                    let q = k.apply_point(p);
                    assert!(q[0] >= out.x.lo - 1e-9 && q[0] <= out.x.hi + 1e-9, "{q:?} vs {out:?}");
                    assert!(q[1] >= out.y.lo - 1e-9 && q[1] <= out.y.hi + 1e-9, "{q:?} vs {out:?}");
                }
            }
        }
    }
}
