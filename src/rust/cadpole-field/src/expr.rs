//! `Field`: the expression a script builds, and its compiler to a [`Tape`].
//!
//! A `Field` is a value, not a body: it holds a shared
//! immutable node and every method returns a new one, so `a.union(&b)` never
//! disturbs `a` and a subtree used twice is stored once. Nothing is evaluated
//! until [`Field::compile`], and no geometry exists until the tape is
//! contoured.
//!
//! Bounds are carried up the tree as the expression is built, so a compiled
//! tape already knows the box the solid lives in and the mesher does not have
//! to go looking. `None` means unbounded (a bare half-space, a constant), and
//! meshing one of those is a diagnostic rather than a guess.

use std::sync::Arc;

use crate::edges::EdgeBlendSet;
use crate::interval::{Box3, Interval};
use crate::mesh_sdf::MeshField;
use crate::tape::{Affine, Builder, NoiseSpec, Op, SpaceKind, Tape};
use cadpole_core::fmath;

#[derive(Debug)]
pub(crate) enum Expr {
    Const(f64),
    Sphere(f64),
    Boxed { half: [f64; 3], round: f64 },
    Cylinder { r: f64, h: f64, round: f64 },
    Cone { r0: f64, r1: f64, h: f64 },
    Torus { major: f64, minor: f64 },
    HalfSpace { n: [f64; 3], d: f64 },
    Capsule { a: [f64; 3], b: [f64; 3], r: f64 },
    Mesh(Arc<MeshField>),
    Noise(NoiseSpec),
    Binary { op: Bin, a: Field, b: Field, k: f64 },
    Offset(Field, f64),
    Shell(Field, f64),
    Negate(Field),
    Space { kind: SpaceKind, a: Field },
    Displace { base: Field, source: Field, amplitude: f64 },
    EdgeBlend { base: Field, set: Arc<EdgeBlendSet> },
}

#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub(crate) enum Bin {
    Union,
    Intersect,
    Difference,
    SmoothUnion,
    SmoothIntersect,
    SmoothDifference,
    ChamferUnion,
    ChamferIntersect,
    ChamferDifference,
}

/// A signed distance field expression. Cheap to clone (it is an `Arc`).
#[derive(Clone, Debug)]
pub struct Field {
    pub(crate) node: Arc<Expr>,
    /// Conservative world bounds of `{f < 0}`, or `None` when unbounded.
    pub(crate) bounds: Option<Box3>,
}

fn bx(half: [f64; 3]) -> Box3 {
    Box3::new([-half[0], -half[1], -half[2]], half)
}

impl Field {
    fn leaf(e: Expr, bounds: Option<Box3>) -> Field {
        Field { node: Arc::new(e), bounds }
    }

    /// A constant field. Negative is "everywhere solid", positive "nowhere".
    pub fn constant(v: f64) -> Field {
        Field::leaf(Expr::Const(v), None)
    }

    pub fn sphere(r: f64) -> Field {
        Field::leaf(Expr::Sphere(r.abs()), Some(bx([r.abs(); 3])))
    }

    /// A box of the given **half** extents, with its twelve edges rounded by
    /// `round`. `round` is clamped to the smallest half extent.
    pub fn boxed(half: [f64; 3], round: f64) -> Field {
        let half = [half[0].abs(), half[1].abs(), half[2].abs()];
        let round = round.abs().min(half[0]).min(half[1]).min(half[2]);
        Field::leaf(Expr::Boxed { half, round }, Some(bx(half)))
    }

    /// A Z-axis cylinder of radius `r` and total height `h`, centred on the
    /// origin, with its rims rounded by `round`.
    pub fn cylinder(r: f64, h: f64, round: f64) -> Field {
        let (r, h) = (r.abs(), h.abs());
        let hh = h * 0.5;
        let round = round.abs().min(r).min(hh);
        Field::leaf(Expr::Cylinder { r, h, round }, Some(bx([r, r, hh])))
    }

    /// A Z-axis capped cone: radius `r0` at `z = -h/2`, `r1` at `z = +h/2`.
    pub fn cone(r0: f64, r1: f64, h: f64) -> Field {
        let (r0, r1, h) = (r0.abs(), r1.abs(), h.abs());
        let rm = r0.max(r1);
        Field::leaf(Expr::Cone { r0, r1, h }, Some(bx([rm, rm, h * 0.5])))
    }

    pub fn torus(major: f64, minor: f64) -> Field {
        let (major, minor) = (major.abs(), minor.abs());
        Field::leaf(Expr::Torus { major, minor }, Some(bx([major + minor, major + minor, minor])))
    }

    /// The half-space `n . p <= d`. Unbounded, so it only becomes geometry
    /// once it is intersected with something finite.
    pub fn half_space(n: [f64; 3], d: f64) -> Field {
        let l = (n[0] * n[0] + n[1] * n[1] + n[2] * n[2]).sqrt();
        let (n, d) = if l > 0.0 { ([n[0] / l, n[1] / l, n[2] / l], d / l) } else { ([0.0, 0.0, 1.0], d) };
        Field::leaf(Expr::HalfSpace { n, d }, None)
    }

    /// A plane through `origin` with outward normal `n`: the solid is the
    /// side the normal points away from.
    pub fn plane(origin: [f64; 3], n: [f64; 3]) -> Field {
        let d = n[0] * origin[0] + n[1] * origin[1] + n[2] * origin[2];
        Field::half_space(n, d)
    }

    pub fn capsule(a: [f64; 3], b: [f64; 3], r: f64) -> Field {
        let r = r.abs();
        let lo = [a[0].min(b[0]) - r, a[1].min(b[1]) - r, a[2].min(b[2]) - r];
        let hi = [a[0].max(b[0]) + r, a[1].max(b[1]) + r, a[2].max(b[2]) + r];
        Field::leaf(Expr::Capsule { a, b, r }, Some(Box3::new(lo, hi)))
    }

    /// An existing body, imported as a field through its triangles.
    pub fn from_mesh_field(m: Arc<MeshField>) -> Field {
        let (lo, hi) = m.bounds();
        Field::leaf(Expr::Mesh(m), Some(Box3::new(lo, hi)))
    }

    /// A deterministic value-noise field, for `displace`. Not a solid: its
    /// values are in `[-amplitude_total, +amplitude_total]`.
    pub fn noise(scale: f64, amplitude: f64, seed: u32, octaves: u32) -> Field {
        Field::leaf(
            Expr::Noise(NoiseSpec { scale, amplitude, seed, octaves: octaves.max(1) }),
            None,
        )
    }

    // ---- combinators

    fn binary(&self, op: Bin, other: &Field, k: f64) -> Field {
        let k = k.abs();
        let bounds = match op {
            Bin::Union => hull(self.bounds, other.bounds),
            Bin::SmoothUnion | Bin::ChamferUnion => {
                hull(self.bounds, other.bounds).map(|b| b.expanded(k))
            }
            Bin::Intersect | Bin::SmoothIntersect | Bin::ChamferIntersect => {
                meet(self.bounds, other.bounds)
            }
            Bin::Difference | Bin::SmoothDifference | Bin::ChamferDifference => self.bounds,
        };
        Field {
            node: Arc::new(Expr::Binary { op, a: self.clone(), b: other.clone(), k }),
            bounds,
        }
    }

    pub fn union(&self, o: &Field) -> Field {
        self.binary(Bin::Union, o, 0.0)
    }
    pub fn intersect(&self, o: &Field) -> Field {
        self.binary(Bin::Intersect, o, 0.0)
    }
    pub fn subtract(&self, o: &Field) -> Field {
        self.binary(Bin::Difference, o, 0.0)
    }
    /// Union with a constant-radius blend: the fillet that comes for free
    /// when a shape is *built* from fields. Exact on a planar pair.
    pub fn smooth_union(&self, o: &Field, k: f64) -> Field {
        self.binary(Bin::SmoothUnion, o, k)
    }
    pub fn smooth_intersect(&self, o: &Field, k: f64) -> Field {
        self.binary(Bin::SmoothIntersect, o, k)
    }
    pub fn smooth_subtract(&self, o: &Field, k: f64) -> Field {
        self.binary(Bin::SmoothDifference, o, k)
    }
    pub fn chamfer_union(&self, o: &Field, d: f64) -> Field {
        self.binary(Bin::ChamferUnion, o, d)
    }
    pub fn chamfer_intersect(&self, o: &Field, d: f64) -> Field {
        self.binary(Bin::ChamferIntersect, o, d)
    }
    pub fn chamfer_subtract(&self, o: &Field, d: f64) -> Field {
        self.binary(Bin::ChamferDifference, o, d)
    }

    /// Grow (`d > 0`) or shrink the solid by `d` millimetres.
    pub fn offset(&self, d: f64) -> Field {
        Field {
            node: Arc::new(Expr::Offset(self.clone(), d)),
            bounds: self.bounds.map(|b| b.expanded(d.max(0.0))),
        }
    }

    /// Hollow the solid into a wall of thickness `t` centred on its surface.
    pub fn shell(&self, t: f64) -> Field {
        Field {
            node: Arc::new(Expr::Shell(self.clone(), t.abs())),
            bounds: self.bounds.map(|b| b.expanded(t.abs() * 0.5)),
        }
    }

    /// Swap inside for outside.
    pub fn negate(&self) -> Field {
        Field { node: Arc::new(Expr::Negate(self.clone())), bounds: None }
    }

    // ---- transforms

    fn spaced(&self, kind: SpaceKind, bounds: Option<Box3>) -> Field {
        Field { node: Arc::new(Expr::Space { kind, a: self.clone() }), bounds }
    }

    pub fn translate(&self, v: [f64; 3]) -> Field {
        let b = self.bounds.map(|b| {
            Box3::new(
                [b.x.lo + v[0], b.y.lo + v[1], b.z.lo + v[2]],
                [b.x.hi + v[0], b.y.hi + v[1], b.z.hi + v[2]],
            )
        });
        self.spaced(SpaceKind::Affine(Affine::translation(v)), b)
    }

    /// Rotate by `angle` radians about `axis` through the origin.
    pub fn rotate(&self, axis: [f64; 3], angle: f64) -> Field {
        let inv = Affine::rotation(axis, angle);
        // The forward map is the transpose of the inverse's linear part.
        let b = self.bounds.map(|b| rotate_bounds(b, &inv));
        self.spaced(SpaceKind::Affine(inv), b)
    }

    /// Scale uniformly about the origin.
    pub fn scale(&self, s: f64) -> Field {
        let s = if s.abs() < 1e-12 { 1e-12f64.copysign(s) } else { s };
        let b = self.bounds.map(|b| {
            let (lo, hi) = (b.lo(), b.hi());
            let mut l = [0.0; 3];
            let mut h = [0.0; 3];
            for k in 0..3 {
                l[k] = (lo[k] * s).min(hi[k] * s);
                h[k] = (lo[k] * s).max(hi[k] * s);
            }
            Box3::new(l, h)
        });
        self.spaced(SpaceKind::Affine(Affine::uniform_scale(s)), b)
    }

    /// Mirror across the plane through the origin with normal `n`.
    pub fn mirror(&self, n: [f64; 3]) -> Field {
        let a = Affine::mirror(n);
        let b = self.bounds.map(|b| rotate_bounds(b, &a));
        self.spaced(SpaceKind::Affine(a), b)
    }

    /// Twist about Z by `degrees_per_mm`.
    pub fn twist(&self, radians_per_mm: f64) -> Field {
        let b = self.bounds.map(|b| {
            let r = fmath::hypot(b
                .x
                .abs()
                .hi
                , b.y.abs().hi);
            Box3 { x: Interval::new(-r, r), y: Interval::new(-r, r), z: b.z }
        });
        self.spaced(SpaceKind::Twist { rate: radians_per_mm }, b)
    }

    /// Scale xy linearly with z, from `s0` at `z0` to `s1` at `z1`.
    pub fn taper(&self, z0: f64, z1: f64, s0: f64, s1: f64) -> Field {
        let m = s0.abs().max(s1.abs()).max(1.0);
        let b = self.bounds.map(|b| Box3 {
            x: b.x.mul_scalar(m),
            y: b.y.mul_scalar(m),
            z: b.z,
        });
        self.spaced(SpaceKind::Taper { z0, z1, s0, s1 }, b)
    }

    /// Push the surface along its own normal by `amplitude * source(p)`.
    ///
    /// The bounds grow by the largest displacement the source can apply near
    /// the base (see [`displaced_bounds`]); a displacement that outgrows any
    /// box leaves the result unbounded, which meshing reports, rather than
    /// bounds that clip the surface.
    pub fn displace(&self, source: &Field, amplitude: f64) -> Field {
        Field {
            node: Arc::new(Expr::Displace {
                base: self.clone(),
                source: source.clone(),
                amplitude,
            }),
            bounds: self.bounds.and_then(|b| displaced_bounds(b, source, amplitude)),
        }
    }

    /// Fillet or chamfer the selected edges. See [`crate::edges`].
    pub fn edge_blend(&self, set: Arc<EdgeBlendSet>) -> Field {
        Field {
            node: Arc::new(Expr::EdgeBlend { base: self.clone(), set }),
            bounds: self.bounds,
        }
    }

    pub fn bounds(&self) -> Option<Box3> {
        self.bounds
    }

    /// Flatten the expression into a tape.
    pub fn compile(&self) -> Tape {
        let mut b = Builder::new();
        let root = emit(&mut b, self, 0);
        b.finish(root, self.bounds)
    }
}

/// Bounds of `base - amplitude * source` given the base's bounds `b`.
///
/// Outside `b` grown by `s` the base is at least `s` (it is a distance field,
/// or bounded below by one), so the displaced surface stays inside that box
/// once `|amplitude| * max|source|` over the box is at most `s`. The source's
/// range comes from interval evaluation of its own tape, which is exact for
/// value noise (a constant range however it is moved, scaled or offset) and
/// grows with the box for a distance field (a half-space, a sphere). The
/// smallest such `s` is found by fixed-point iteration from zero; when it does
/// not settle (the displacement grows at least as fast as the box) there is no
/// finite bound and the result is `None`.
fn displaced_bounds(b: Box3, source: &Field, amplitude: f64) -> Option<Box3> {
    let a = amplitude.abs();
    if a == 0.0 {
        return Some(b);
    }
    let tape = source.compile();
    let mut ev = tape.evaluator();
    let mut need = |s: f64| {
        let r = ev.eval_box(b.expanded(s));
        a * r.lo.abs().max(r.hi.abs())
    };
    let mut s = 0.0f64;
    for _ in 0..200 {
        let next = need(s);
        if !next.is_finite() {
            return None;
        }
        if next <= s {
            return Some(b.expanded(s));
        }
        // Step a little past the iterate so a geometric approach to the fixed
        // point ends in a few dozen steps instead of creeping up on it.
        s = next * 1.001 + 1e-9;
        if s > 1e12 {
            return None;
        }
    }
    None
}

fn rotate_bounds(b: Box3, inv: &Affine) -> Box3 {
    // Forward linear map = transpose of the world-to-local linear part (both
    // rotations and mirrors are orthogonal).
    let c = b.center();
    let h = [0.5 * b.x.width(), 0.5 * b.y.width(), 0.5 * b.z.width()];
    let fwd = |p: [f64; 3]| {
        [
            inv.m[0][0] * p[0] + inv.m[1][0] * p[1] + inv.m[2][0] * p[2],
            inv.m[0][1] * p[0] + inv.m[1][1] * p[1] + inv.m[2][1] * p[2],
            inv.m[0][2] * p[0] + inv.m[1][2] * p[1] + inv.m[2][2] * p[2],
        ]
    };
    let cc = fwd(c);
    let mut lo = [0.0; 3];
    let mut hi = [0.0; 3];
    for i in 0..3 {
        let e = inv.m[0][i].abs() * h[0] + inv.m[1][i].abs() * h[1] + inv.m[2][i].abs() * h[2];
        lo[i] = cc[i] - e;
        hi[i] = cc[i] + e;
    }
    Box3::new(lo, hi)
}

fn hull(a: Option<Box3>, b: Option<Box3>) -> Option<Box3> {
    match (a, b) {
        (Some(a), Some(b)) => Some(Box3 { x: a.x.hull(b.x), y: a.y.hull(b.y), z: a.z.hull(b.z) }),
        _ => None,
    }
}

fn meet(a: Option<Box3>, b: Option<Box3>) -> Option<Box3> {
    match (a, b) {
        (Some(a), Some(b)) => Some(Box3 {
            x: Interval::new(a.x.lo.max(b.x.lo), a.x.hi.min(b.x.hi)),
            y: Interval::new(a.y.lo.max(b.y.lo), a.y.hi.min(b.y.hi)),
            z: Interval::new(a.z.lo.max(b.z.lo), a.z.hi.min(b.z.hi)),
        }),
        (Some(a), None) => Some(a),
        (None, Some(b)) => Some(b),
        (None, None) => None,
    }
}

fn emit(b: &mut Builder, f: &Field, space: u32) -> u32 {
    match &*f.node {
        Expr::Const(v) => b.push(Op::Const(*v), space),
        Expr::Sphere(r) => b.push(Op::Sphere { r: *r }, space),
        Expr::Boxed { half, round } => b.push(Op::Box { half: *half, round: *round }, space),
        Expr::Cylinder { r, h, round } => {
            b.push(Op::Cylinder { r: *r, hh: h * 0.5, round: *round }, space)
        }
        Expr::Cone { r0, r1, h } => b.push(Op::Cone { r0: *r0, r1: *r1, hh: h * 0.5 }, space),
        Expr::Torus { major, minor } => b.push(Op::Torus { major: *major, minor: *minor }, space),
        Expr::HalfSpace { n, d } => b.push(Op::Plane { n: *n, d: *d }, space),
        Expr::Capsule { a, b: bb, r } => b.push(Op::Capsule { a: *a, b: *bb, r: *r }, space),
        Expr::Mesh(m) => {
            let i = b.mesh(m.clone());
            b.push(Op::Mesh(i), space)
        }
        Expr::Noise(spec) => b.push(Op::Noise(*spec), space),
        Expr::Binary { op, a, b: rhs, k } => {
            let x = emit(b, a, space);
            let y = emit(b, rhs, space);
            let node = match op {
                Bin::Union => Op::Union(x, y),
                Bin::Intersect => Op::Intersect(x, y),
                Bin::Difference => Op::Difference(x, y),
                Bin::SmoothUnion => Op::SmoothUnion(x, y, *k),
                Bin::SmoothIntersect => Op::SmoothIntersect(x, y, *k),
                Bin::SmoothDifference => Op::SmoothDifference(x, y, *k),
                Bin::ChamferUnion => Op::ChamferUnion(x, y, *k),
                Bin::ChamferIntersect => Op::ChamferIntersect(x, y, *k),
                Bin::ChamferDifference => Op::ChamferDifference(x, y, *k),
            };
            b.push(node, space)
        }
        Expr::Offset(a, d) => {
            let x = emit(b, a, space);
            b.push(Op::Offset(x, *d), space)
        }
        Expr::Shell(a, t) => {
            let x = emit(b, a, space);
            b.push(Op::Shell(x, *t), space)
        }
        Expr::Negate(a) => {
            let x = emit(b, a, space);
            b.push(Op::Negate(x), space)
        }
        Expr::Space { kind, a } => {
            let s = b.space(space, *kind);
            emit(b, a, s)
        }
        Expr::Displace { base, source, amplitude } => {
            let x = emit(b, base, space);
            let y = emit(b, source, space);
            b.push(Op::Displace { base: x, source: y, amplitude: *amplitude }, space)
        }
        Expr::EdgeBlend { base, set } => {
            let x = emit(b, base, space);
            let i = b.blend(set.clone());
            b.push(Op::EdgeBlend { base: x, set: i }, space)
        }
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    fn inside(f: &Field, p: [f64; 3]) -> bool {
        f.compile().eval(p) < 0.0
    }

    #[test]
    fn primitives_put_their_surface_where_they_say() {
        let t = Field::sphere(3.0).compile();
        assert!((t.eval([3.0, 0.0, 0.0])).abs() < 1e-12);
        assert!((t.eval([0.0, 0.0, 0.0]) + 3.0).abs() < 1e-12);

        let t = Field::cylinder(2.0, 10.0, 0.0).compile();
        assert!(t.eval([2.0, 0.0, 0.0]).abs() < 1e-12);
        assert!(t.eval([0.0, 0.0, 5.0]).abs() < 1e-12);
        assert!(t.eval([0.0, 0.0, 6.0]) > 0.9);

        let t = Field::torus(5.0, 1.0).compile();
        assert!(t.eval([6.0, 0.0, 0.0]).abs() < 1e-12);
        assert!(t.eval([0.0, 0.0, 0.0]) > 0.0);

        let t = Field::cone(3.0, 1.0, 4.0).compile();
        assert!(t.eval([3.0, 0.0, -2.0]).abs() < 1e-9);
        assert!(t.eval([1.0, 0.0, 2.0]).abs() < 1e-9);
    }

    #[test]
    fn transforms_move_the_solid_not_the_query() {
        let f = Field::boxed([1.0, 1.0, 1.0], 0.0).translate([10.0, 0.0, 0.0]);
        assert!(inside(&f, [10.0, 0.0, 0.0]));
        assert!(!inside(&f, [0.0, 0.0, 0.0]));
        let b = f.bounds().unwrap();
        assert!((b.x.lo - 9.0).abs() < 1e-12 && (b.x.hi - 11.0).abs() < 1e-12);

        let r = Field::boxed([3.0, 0.5, 0.5], 0.0).rotate([0.0, 0.0, 1.0], std::f64::consts::FRAC_PI_2);
        assert!(inside(&r, [0.0, 2.5, 0.0]), "the long axis is now y");
        assert!(!inside(&r, [2.5, 0.0, 0.0]));
        let b = r.bounds().unwrap();
        assert!(b.y.hi > 2.9 && b.x.hi < 0.6, "{b:?}");

        let s = Field::sphere(1.0).scale(3.0);
        assert!((s.compile().eval([3.0, 0.0, 0.0])).abs() < 1e-9);
        // Scaling keeps it a distance field: 1 mm outside is 1 mm.
        assert!((s.compile().eval([4.0, 0.0, 0.0]) - 1.0).abs() < 1e-9);

        let m = Field::sphere(1.0).translate([5.0, 0.0, 0.0]).mirror([1.0, 0.0, 0.0]);
        assert!(inside(&m, [-5.0, 0.0, 0.0]));
        assert!(!inside(&m, [5.0, 0.0, 0.0]));
    }

    #[test]
    fn booleans_do_what_they_say() {
        let a = Field::sphere(2.0).translate([-1.0, 0.0, 0.0]);
        let b = Field::sphere(2.0).translate([1.0, 0.0, 0.0]);
        assert!(inside(&a.union(&b), [-2.5, 0.0, 0.0]));
        assert!(!inside(&a.intersect(&b), [-2.5, 0.0, 0.0]));
        assert!(inside(&a.intersect(&b), [0.0, 0.0, 0.0]));
        assert!(!inside(&a.subtract(&b), [0.5, 0.0, 0.0]));
        assert!(inside(&a.subtract(&b), [-2.5, 0.0, 0.0]));
    }

    #[test]
    fn a_shell_is_hollow() {
        let f = Field::sphere(10.0).shell(2.0);
        assert!(!inside(&f, [0.0, 0.0, 0.0]), "the middle is gone");
        assert!(inside(&f, [10.0, 0.0, 0.0]), "the wall is there");
        assert!(!inside(&f, [12.0, 0.0, 0.0]));
        assert!(inside(&f, [9.2, 0.0, 0.0]));
    }

    #[test]
    fn bounds_are_conservative_for_every_construction() {
        let cases = vec![
            Field::sphere(3.0),
            Field::boxed([2.0, 3.0, 4.0], 0.5),
            Field::cylinder(2.0, 6.0, 0.5),
            Field::cone(3.0, 0.0, 5.0),
            Field::torus(4.0, 1.0),
            Field::capsule([-3.0, 0.0, 0.0], [3.0, 2.0, 1.0], 1.0),
            Field::sphere(2.0).translate([4.0, -2.0, 1.0]),
            Field::boxed([3.0, 1.0, 1.0], 0.0).rotate([0.0, 1.0, 0.0], 0.7),
            Field::sphere(2.0).union(&Field::sphere(1.0).translate([0.0, 5.0, 0.0])),
            Field::sphere(3.0).smooth_union(&Field::sphere(3.0).translate([4.0, 0.0, 0.0]), 1.5),
            Field::boxed([3.0, 3.0, 3.0], 0.0).offset(1.0),
            Field::sphere(4.0).shell(1.0),
            Field::sphere(5.0).displace(&Field::noise(2.0, 0.8, 1, 3), 1.0),
            Field::boxed([2.0, 2.0, 6.0], 0.3).twist(0.2),
            Field::cylinder(2.0, 8.0, 0.0).taper(-4.0, 4.0, 1.0, 2.0),
        ];
        for f in &cases {
            let bb = f.bounds().expect("bounded");
            let t = f.compile();
            let probe = bb.expanded(3.0);
            let n = 17;
            for i in 0..=n {
                for j in 0..=n {
                    for k in 0..=n {
                        let p = [
                            probe.x.lo + probe.x.width() * i as f64 / n as f64,
                            probe.y.lo + probe.y.width() * j as f64 / n as f64,
                            probe.z.lo + probe.z.width() * k as f64 / n as f64,
                        ];
                        if t.eval(p) < 0.0 {
                            let e = 1e-6;
                            assert!(
                                p[0] >= bb.x.lo - e && p[0] <= bb.x.hi + e
                                    && p[1] >= bb.y.lo - e && p[1] <= bb.y.hi + e
                                    && p[2] >= bb.z.lo - e && p[2] <= bb.z.hi + e,
                                "{p:?} is inside but outside the declared bounds {bb:?}"
                            );
                        }
                    }
                }
            }
        }
    }

    /// Every sampled inside point of `f` lies in its declared bounds.
    fn assert_bounds_hold(f: &Field, probe: Box3, n: usize) {
        let bb = f.bounds().expect("bounded");
        let t = f.compile();
        for i in 0..=n {
            for j in 0..=n {
                for k in 0..=n {
                    let p = [
                        probe.x.lo + probe.x.width() * i as f64 / n as f64,
                        probe.y.lo + probe.y.width() * j as f64 / n as f64,
                        probe.z.lo + probe.z.width() * k as f64 / n as f64,
                    ];
                    if t.eval(p) < 0.0 {
                        let e = 1e-6;
                        assert!(
                            p[0] >= bb.x.lo - e && p[0] <= bb.x.hi + e
                                && p[1] >= bb.y.lo - e && p[1] <= bb.y.hi + e
                                && p[2] >= bb.z.lo - e && p[2] <= bb.z.hi + e,
                            "{p:?} is inside but outside the declared bounds {bb:?}"
                        );
                    }
                }
            }
        }
    }

    #[test]
    fn displace_bounds_hold_for_any_source_not_just_bare_noise() {
        let base = Field::sphere(15.0);
        let noise = Field::noise(6.0, 3.0, 1, 1);
        let probe = Box3::new([-25.0; 3], [25.0; 3]);
        // A transformed noise has the same value range as the bare one.
        for src in [
            noise.translate([0.0, 0.0, 0.0]),
            noise.rotate([1.0, 1.0, 0.0], 0.4),
            noise.scale(2.0),
            noise.offset(0.5),
        ] {
            let f = base.displace(&src, 1.0);
            assert!(f.bounds().unwrap().x.hi > 16.0, "{:?}", f.bounds());
            assert_bounds_hold(&f, probe, 40);
        }
        // A source whose range grows with distance: sphere(15) - 0.2 z reaches
        // z = 18.75 on the axis.
        let f = base.displace(&Field::half_space([0.0, 0.0, 1.0], 0.0), 0.2);
        assert!(f.bounds().unwrap().z.hi >= 18.75, "{:?}", f.bounds());
        assert_bounds_hold(&f, probe, 40);
        // A displacement that outgrows any box is unbounded, not clipped.
        let f = base.displace(&Field::half_space([0.0, 0.0, 1.0], 0.0), 1.5);
        assert!(f.bounds().is_none(), "{:?}", f.bounds());
    }

    #[test]
    fn an_unbounded_expression_says_so() {
        assert!(Field::half_space([0.0, 0.0, 1.0], 0.0).bounds().is_none());
        // ... until it is bounded by something.
        assert!(
            Field::half_space([0.0, 0.0, 1.0], 0.0)
                .intersect(&Field::sphere(2.0))
                .bounds()
                .is_some()
        );
    }
}
