//! Swept elliptical beads: an analytic primitive for extruded material.
//!
//! A bead is the solid a round nozzle lays down along a straight move: a
//! capsule between two points whose cross-section is squashed along world Z
//! into an ellipse `2 * rx` wide and `2 * rz` tall. Seen from above it is a
//! stadium with round ends, so consecutive beads of one path join without a
//! notch at any corner; seen end-on it is the ellipse. Formally it is the
//! image of a round capsule of radius `rx` under the scaling
//! `diag(1, 1, rz / rx)`, which is what makes it cheap: a ray is mapped into
//! the round capsule's space, intersected there with a quadratic, and the
//! normal is mapped back with the inverse transpose. The ray parameter `t`
//! is the same in both spaces, because the map is linear.
//!
//! Millions of them are rendered without a mesh: [`BeadSet`] holds the beads
//! and a four-wide bounding volume hierarchy over them. It is built as a
//! binary binned-SAH tree (up to four beads per leaf, the top levels split
//! sequentially and the subtrees below them in parallel, so the tree is the
//! same for any thread count) and then collapsed so each node tests four
//! child boxes at once, laid out for the compiler's vector unit. Long beads
//! are split into pieces no longer than [`BeadSet::new`]'s `max_len` first,
//! because a long diagonal line has a box mostly made of air.
//!
//! Every bead has a class from its material ([`CLASS_OPAQUE`] and the
//! rest), and every child box carries the classes found under it, so a
//! search for one kind of bead (the opaque ones a shadow ray stops at, the
//! see-through ones it is tinted by) never descends into a subtree that has
//! none.
//!
//! A bead overlaps its neighbours: the next bead of the same path at the
//! joint, and the beads beside and above it where the cross-sections meet.
//! For an opaque material that does not matter (a ray from outside meets the
//! union's surface first). Inside a see-through or scattering solid it
//! does, and [`BeadSet::inside_other`] tells a crossing that is the union's
//! real surface from one that is buried in a neighbour.

use super::{add, dot, scale, sub};
use crate::context::Material;

/// Beads per leaf above which a split is attempted.
const LEAF_MAX: usize = 4;
/// Centroid bins per axis for the SAH sweep.
const BINS: usize = 16;
/// Depth limit of the binary build.
const MAX_DEPTH: usize = 64;
/// Traversal stack of the four-wide tree: three entries a level at most.
const STACK: usize = 3 * MAX_DEPTH + 4;
/// Ranges at or below this many beads are built as one task each, in
/// parallel; above it the build splits sequentially.
const PAR_GRAIN: usize = 1 << 14;

/// A bead whose material is not see-through: it stops a shadow ray.
pub const CLASS_OPAQUE: u8 = 1;
/// A see-through bead: it tints a shadow ray.
pub const CLASS_CLEAR: u8 = 2;
/// Light travels inside it (it is see-through, or it scatters under its
/// surface), so where two overlap the buried crossings are not a surface.
pub const CLASS_SOLID: u8 = 4;
/// Not [`CLASS_SOLID`]: every crossing is a surface.
pub const CLASS_PLAIN: u8 = 8;
/// Every bead.
pub const CLASS_ALL: u8 = 15;

/// One straight bead from `a` to `b`. `rx` is half its width, `rz` half its
/// height, both in mm; `material` indexes [`BeadSet::materials`].
#[derive(Clone, Copy, Debug, PartialEq)]
pub struct Bead {
    pub a: [f32; 3],
    pub b: [f32; 3],
    pub rx: f32,
    pub rz: f32,
    pub material: u32,
}

/// The ray's interval inside a sphere of radius² `r2` centred `oc2` behind
/// the ray's origin (`oc2` = origin − centre), direction `dd` with
/// `rdrd = |dd|²`.
#[inline(always)]
fn sphere_interval(oc2: [f32; 3], dd: [f32; 3], rdrd: f32, r2: f32) -> Option<(f32, f32)> {
    let bq = dot(oc2, dd);
    let cq = dot(oc2, oc2) - r2;
    let h = bq * bq - rdrd * cq;
    if h < 0.0 {
        return None;
    }
    let sh = h.sqrt();
    Some(((-bq - sh) / rdrd, (-bq + sh) / rdrd))
}

impl Bead {
    fn bounds(&self) -> ([f32; 3], [f32; 3]) {
        let r = [self.rx, self.rx, self.rz];
        let mut lo = [0.0; 3];
        let mut hi = [0.0; 3];
        for k in 0..3 {
            lo[k] = self.a[k].min(self.b[k]) - r[k];
            hi[k] = self.a[k].max(self.b[k]) + r[k];
        }
        (lo, hi)
    }

    /// Unit direction from `a` to `b`, or +X for a dot.
    pub fn tangent(&self) -> [f32; 3] {
        let d = sub(self.b, self.a);
        let l2 = dot(d, d);
        if l2 > 1e-20 {
            scale(d, 1.0 / l2.sqrt())
        } else {
            [1.0, 0.0, 0.0]
        }
    }

    /// The ray's interval inside the bead, `(t_in, t_out)`, if it meets it.
    ///
    /// The ray is first moved to its closest approach to `a`, so the
    /// quadratics are solved with coordinates the size of the bead rather
    /// than the size of the scene: from a camera half a metre away, `|o|^2`
    /// is 2.5e5 mm^2 against a radius^2 of 0.04, which is past what f32
    /// can subtract.
    ///
    /// The capsule lies inside the infinite cylinder about its axis, so a
    /// ray that misses that cylinder misses the bead, and that is decided
    /// first: it is most of the beads a leaf offers. A ray that meets the
    /// cylinder outside the segment's slab enters (or leaves) through the
    /// end sphere on that side, and only that sphere is solved.
    #[inline]
    pub fn interval(&self, o: [f32; 3], d: [f32; 3]) -> Option<(f32, f32)> {
        let s = self.rx / self.rz.max(1e-9);
        let a = [self.a[0], self.a[1], self.a[2] * s];
        let b = [self.b[0], self.b[1], self.b[2] * s];
        let dd = [d[0], d[1], d[2] * s];
        let oo = [o[0], o[1], o[2] * s];
        let rdrd = dot(dd, dd);
        if rdrd <= 0.0 {
            return None;
        }
        let tc = dot(sub(a, oo), dd) / rdrd;
        let oc = add(oo, scale(dd, tc));
        let r2 = self.rx * self.rx;
        let ba = sub(b, a);
        let baba = dot(ba, ba);
        let oa = sub(oc, a);
        if baba <= 1e-14 {
            return sphere_interval(oa, dd, rdrd, r2).map(|(t0, t1)| (t0 + tc, t1 + tc));
        }
        let bard = dot(ba, dd);
        let baoa = dot(ba, oa);
        let qa = baba * rdrd - bard * bard;
        if qa <= 1e-9 * baba * rdrd {
            // Along the axis: the two end spheres bound it.
            let ea = sphere_interval(oa, dd, rdrd, r2);
            let eb = sphere_interval(sub(oc, b), dd, rdrd, r2);
            return match (ea, eb) {
                (Some(x), Some(y)) => Some((x.0.min(y.0) + tc, x.1.max(y.1) + tc)),
                (Some(x), None) | (None, Some(x)) => Some((x.0 + tc, x.1 + tc)),
                (None, None) => None,
            };
        }
        let qb = baba * dot(dd, oa) - baoa * bard;
        let qc = baba * dot(oa, oa) - baoa * baoa - r2 * baba;
        let h = qb * qb - qa * qc;
        if h < 0.0 {
            return None;
        }
        let sh = h.sqrt();
        let t0 = (-qb - sh) / qa;
        let t1 = (-qb + sh) / qa;
        let y0 = baoa + t0 * bard;
        let y1 = baoa + t1 * bard;
        let t_in = if y0 < 0.0 {
            sphere_interval(oa, dd, rdrd, r2)?.0
        } else if y0 > baba {
            sphere_interval(sub(oc, b), dd, rdrd, r2)?.0
        } else {
            t0
        };
        let t_out = if y1 < 0.0 {
            sphere_interval(oa, dd, rdrd, r2).map_or(t_in, |x| x.1)
        } else if y1 > baba {
            sphere_interval(sub(oc, b), dd, rdrd, r2).map_or(t_in, |x| x.1)
        } else {
            t1
        };
        Some((t_in + tc, t_out.max(t_in) + tc))
    }

    /// Outward unit normal at a point on (or near) the surface.
    #[inline]
    pub fn normal(&self, p: [f32; 3]) -> [f32; 3] {
        let s = self.rx / self.rz.max(1e-9);
        let a = [self.a[0], self.a[1], self.a[2] * s];
        let ba = [
            self.b[0] - self.a[0],
            self.b[1] - self.a[1],
            (self.b[2] - self.a[2]) * s,
        ];
        let ps = [p[0], p[1], p[2] * s];
        let pa = sub(ps, a);
        let baba = dot(ba, ba);
        let h = if baba > 1e-14 {
            (dot(pa, ba) / baba).clamp(0.0, 1.0)
        } else {
            0.0
        };
        let n = sub(pa, scale(ba, h));
        let n = [n[0], n[1], n[2] * s];
        let l2 = dot(n, n);
        if l2 > 1e-24 {
            scale(n, 1.0 / l2.sqrt())
        } else {
            [0.0, 0.0, 1.0]
        }
    }

    /// Whether `p` lies inside the bead by more than `margin` of its radius
    /// (a fraction, so a point on the surface is not inside).
    #[inline]
    pub fn contains(&self, p: [f32; 3], margin: f32) -> bool {
        let s = self.rx / self.rz.max(1e-9);
        let a = [self.a[0], self.a[1], self.a[2] * s];
        let ba = [
            self.b[0] - self.a[0],
            self.b[1] - self.a[1],
            (self.b[2] - self.a[2]) * s,
        ];
        let pa = sub([p[0], p[1], p[2] * s], a);
        let baba = dot(ba, ba);
        let h = if baba > 1e-14 {
            (dot(pa, ba) / baba).clamp(0.0, 1.0)
        } else {
            0.0
        };
        let q = sub(pa, scale(ba, h));
        let r = self.rx * (1.0 - margin);
        dot(q, q) < r * r
    }
}

/// Metal or mica flakes embedded in the plastic: glitter, metal-filled and
/// "galaxy" filaments, and the faint glints of cut carbon fibre. A flake is
/// a small mirror tilted at random from the surface, so it lights up only
/// where it happens to reflect a light toward the camera.
#[derive(Clone, Copy, Debug, PartialEq)]
pub struct Flakes {
    /// Share of the lattice cells that hold a flake, 0..1.
    pub density: f32,
    /// Flake spacing in mm (the lattice cell); a flake is a little under
    /// half of it across.
    pub size: f32,
    /// Largest tilt of a flake from the surface, in degrees.
    pub tilt_degrees: f32,
    /// Microfacet roughness of a flake.
    pub roughness: f32,
    /// Reflectance of a flake, linear RGB (a metal's F0).
    pub color: [f32; 3],
}

/// Particles of another colour suspended in the plastic: the dark grains of
/// a stone-look filament, the fibres of a wood-filled one.
#[derive(Clone, Copy, Debug, PartialEq)]
pub struct Speckle {
    /// Share of the lattice cells that hold a particle, 0..1.
    pub density: f32,
    /// Particle spacing in mm.
    pub size: f32,
    /// The particle's colour, linear RGB.
    pub color: [f32; 3],
}

/// How a set of beads is drawn: a scene [`Material`] plus what only a
/// bead can have.
///
/// On a bead, `anisotropy` follows the bead: the surface is smooth along the
/// direction the nozzle moved and rough across it, which is the satin streak
/// of a silk filament. The geometry already makes the layer lines, so there
/// is no separate layer-direction rule here.
#[derive(Clone, Debug)]
pub struct BeadMaterial {
    pub material: Material,
    /// Mean free path, in mm, of light scattering under the surface of an
    /// opaque material; 0 is off. With it the diffuse part of the surface
    /// is a random walk through the beads instead of a Lambert lobe, so
    /// light that enters one side of a thin wall or a bead's edge comes out
    /// a little way off, which is what makes plastic read as plastic.
    pub subsurface: f32,
    /// Embedded flakes, if any.
    pub flakes: Option<Flakes>,
    /// Suspended particles, if any.
    pub speckle: Option<Speckle>,
    /// How much the tone wanders from layer to layer, 0..1: the banding of
    /// a wood-filled filament, whose colour follows the nozzle temperature.
    pub streak: f32,
}

impl BeadMaterial {
    pub fn new(material: Material) -> BeadMaterial {
        BeadMaterial {
            material,
            subsurface: 0.0,
            flakes: None,
            speckle: None,
            streak: 0.0,
        }
    }

    /// The class of a bead drawn with this material.
    pub fn class(&self) -> u8 {
        let clear = super::trace::material_for(&self.material).transmission > 1e-3;
        let solid = clear || self.subsurface > 0.0;
        (if clear { CLASS_CLEAR } else { CLASS_OPAQUE })
            | (if solid { CLASS_SOLID } else { CLASS_PLAIN })
    }
}

/// What a bead's finish does at one point of its surface.
#[derive(Clone, Copy, Debug, PartialEq)]
pub enum Finish {
    /// The plastic itself, its colour multiplied by `tone`.
    Base { tone: f32 },
    /// A flake: a small tilted mirror with this normal.
    Flake { normal: [f32; 3] },
    /// A particle of the speckle colour.
    Particle,
}

#[inline]
fn mix32(mut x: u32) -> u32 {
    x ^= x >> 16;
    x = x.wrapping_mul(0x7feb_352d);
    x ^= x >> 15;
    x = x.wrapping_mul(0x846c_a68b);
    x ^= x >> 16;
    x
}

#[inline]
fn cell_hash(c: [i32; 3], seed: u32) -> u32 {
    let h = mix32((c[2] as u32) ^ seed ^ 0x9e37_79b9);
    let h = mix32((c[1] as u32).wrapping_add(h));
    mix32((c[0] as u32).wrapping_add(h))
}

#[inline]
fn unit(h: u32) -> f32 {
    (h >> 8) as f32 * (1.0 / 16_777_216.0)
}

/// The particle of the lattice of cell size `size` nearest `p`, if `p` is
/// inside it: `(hash, offset from its centre in cells)`.
fn particle_at(p: [f32; 3], size: f32, density: f32, seed: u32) -> Option<u32> {
    if !(size > 0.0) || !(density > 0.0) {
        return None;
    }
    let u = [p[0] / size, p[1] / size, p[2] / size];
    if !u.iter().all(|v| v.is_finite() && v.abs() < 1e7) {
        return None;
    }
    let c = [
        u[0].floor() as i32,
        u[1].floor() as i32,
        u[2].floor() as i32,
    ];
    let h = cell_hash(c, seed);
    if unit(h) >= density {
        return None;
    }
    let h1 = mix32(h ^ 0x68e3_1da4);
    let h2 = mix32(h1);
    let h3 = mix32(h2);
    // A centre kept away from the cell walls, so a particle is whole.
    let centre = [
        c[0] as f32 + 0.3 + 0.4 * unit(h1),
        c[1] as f32 + 0.3 + 0.4 * unit(h2),
        c[2] as f32 + 0.3 + 0.4 * unit(h3),
    ];
    let q = sub(u, centre);
    (dot(q, q) < 0.09).then_some(mix32(h3))
}

impl BeadMaterial {
    /// The finish at `p`, whose unit surface normal is `n`.
    pub fn finish_at(&self, p: [f32; 3], n: [f32; 3]) -> Finish {
        if let Some(f) = &self.flakes {
            if let Some(h) = particle_at(p, f.size, f.density, 0x51ed_270b) {
                // A tilt up to the limit, uniform over the cap's area, about
                // a random azimuth in the surface's tangent plane.
                let max = f.tilt_degrees.clamp(0.0, 80.0).to_radians();
                let cos_t = 1.0 - unit(h) * (1.0 - max.cos());
                let sin_t = (1.0 - cos_t * cos_t).max(0.0).sqrt();
                let phi = std::f32::consts::TAU * unit(mix32(h));
                let t = if n[0].abs() > 0.9 {
                    [0.0, 1.0, 0.0]
                } else {
                    [1.0, 0.0, 0.0]
                };
                let tx = super::normalize(sub(t, scale(n, dot(t, n))));
                let ty = super::cross(n, tx);
                let w = add(
                    scale(n, cos_t),
                    add(scale(tx, sin_t * phi.cos()), scale(ty, sin_t * phi.sin())),
                );
                return Finish::Flake {
                    normal: super::normalize(w),
                };
            }
        }
        if let Some(s) = &self.speckle {
            if particle_at(p, s.size, s.density, 0x2c1b_3c6d).is_some() {
                return Finish::Particle;
            }
        }
        if self.streak > 0.0 {
            // Bands about a layer or two tall that drift slowly along the
            // layers: the colour follows the temperature, which wanders on
            // the scale of a whole layer, not of a bead.
            let q = [p[0] * 0.08, p[1] * 0.08, p[2] * 1.7];
            let q = q.map(|v| {
                if v.is_finite() {
                    v.clamp(-1e6, 1e6)
                } else {
                    0.0
                }
            });
            let v = super::textures::value_noise(q) * 0.65
                + super::textures::value_noise([q[0] * 2.1 + 3.7, q[1] * 2.1, q[2] * 2.3]) * 0.35;
            return Finish::Base {
                tone: (1.0 + self.streak.clamp(0.0, 1.0) * 2.0 * (v - 0.5)).max(0.0),
            };
        }
        Finish::Base { tone: 1.0 }
    }
}

/// A node of the four-wide tree: four child boxes in structure-of-arrays
/// form, so one slab test covers all four.
#[derive(Clone, Copy, Debug)]
#[repr(C, align(64))]
struct Node4 {
    /// `lo[axis][child]`.
    lo: [[f32; 4]; 3],
    hi: [[f32; 4]; 3],
    /// An inner child's node index, or a leaf's first bead.
    child: [u32; 4],
    /// Beads in a leaf child; 0 for an inner child.
    count: [u16; 4],
    /// The classes under each child; 0 is an empty slot.
    mask: [u8; 4],
}

impl Node4 {
    const EMPTY: Node4 = Node4 {
        lo: [[f32::INFINITY; 4]; 3],
        hi: [[f32::NEG_INFINITY; 4]; 3],
        child: [0; 4],
        count: [0; 4],
        mask: [0; 4],
    };
}

/// A node of the binary build tree.
#[derive(Clone, Copy, Debug, Default)]
struct Node {
    lo: [f32; 3],
    hi: [f32; 3],
    /// Leaf: first bead. Inner node: index of the left child (the right one
    /// follows it).
    first: u32,
    /// Beads in a leaf; 0 for an inner node.
    count: u32,
}

/// Build and memory figures for a [`BeadSet`].
#[derive(Clone, Copy, Debug, Default)]
pub struct BeadStats {
    /// Beads given to [`BeadSet::new`].
    pub input: usize,
    /// Beads after long ones were split.
    pub beads: usize,
    /// Nodes of the four-wide tree.
    pub nodes: usize,
    pub max_depth: usize,
    pub build_ms: f64,
    /// Bytes held by the beads and the tree.
    pub bytes: usize,
}

/// A hit on a bead: the distance along the ray and which bead.
#[derive(Clone, Copy, Debug)]
pub struct BeadHit {
    pub t: f32,
    pub bead: u32,
}

/// Beads, their materials and the tree over them.
pub struct BeadSet {
    beads: Vec<Bead>,
    /// Per bead slot, its class.
    class: Vec<u8>,
    nodes: Vec<Node4>,
    bounds: ([f32; 3], [f32; 3]),
    /// The classes present at all.
    classes: u8,
    pub materials: Vec<BeadMaterial>,
    /// A plane the beads stand on, if any: subsurface light that reaches
    /// the bottom of the first layer goes into it and does not come back
    /// out.
    pub floor_z: Option<f32>,
    pub stats: BeadStats,
}

impl std::fmt::Debug for BeadSet {
    fn fmt(&self, f: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
        f.debug_struct("BeadSet")
            .field("stats", &self.stats)
            .field("materials", &self.materials.len())
            .finish()
    }
}

/// Per-ray constants of the slab test.
#[derive(Clone, Copy)]
struct RayBox {
    /// `o * inv`, so a plane's distance is one multiply-subtract.
    oi: [f32; 3],
    inv: [f32; 3],
    /// Per axis, whether the direction is negative: which of lo/hi is near.
    neg: [bool; 3],
}

impl RayBox {
    #[inline]
    fn new(o: [f32; 3], d: [f32; 3]) -> RayBox {
        let inv = inv_dir(d);
        RayBox {
            oi: [o[0] * inv[0], o[1] * inv[1], o[2] * inv[2]],
            inv,
            neg: [inv[0] < 0.0, inv[1] < 0.0, inv[2] < 0.0],
        }
    }

    /// Entry distances of the ray into the four child boxes within
    /// `(tmin, tmax)`, and a bit per child that it enters.
    #[inline(always)]
    fn test(&self, n: &Node4, tmin: f32, tmax: f32) -> ([f32; 4], u32) {
        let mut t0 = [tmin; 4];
        let mut t1 = [tmax; 4];
        for k in 0..3 {
            let (near, far) = if self.neg[k] {
                (&n.hi[k], &n.lo[k])
            } else {
                (&n.lo[k], &n.hi[k])
            };
            for c in 0..4 {
                let a = near[c] * self.inv[k] - self.oi[k];
                let b = far[c] * self.inv[k] - self.oi[k];
                // Plain compares rather than `f32::max`, whose NaN rule
                // costs a compare and a blend per lane; an empty slot's
                // infinities still lose every comparison they should.
                t0[c] = if a > t0[c] { a } else { t0[c] };
                t1[c] = if b < t1[c] { b } else { t1[c] };
            }
        }
        let mut bits = 0u32;
        for c in 0..4 {
            // A hair of slack: a bead lying exactly on a box face is still
            // found.
            if t0[c] <= t1[c] * (1.0 + 4.0 * f32::EPSILON) + 1e-7 {
                bits |= 1 << c;
            }
        }
        (t0, bits)
    }
}

/// A child entry on the traversal stack: a node index, or a leaf with the
/// high bit set and its count in the next 15 bits.
const LEAF_BIT: u64 = 1 << 63;

#[inline]
fn pack(child: u32, count: u16) -> u64 {
    if count == 0 {
        child as u64
    } else {
        LEAF_BIT | ((count as u64) << 32) | child as u64
    }
}

impl BeadSet {
    /// Build the set. Beads longer than `max_len` mm (when positive) are
    /// cut into equal pieces no longer than it; beads with a non-finite
    /// coordinate or a non-positive radius are dropped, and a material
    /// index past the end is clamped to the last material.
    pub fn new(beads: Vec<Bead>, materials: Vec<BeadMaterial>, max_len: f32) -> BeadSet {
        let t0 = super::aa::now_ms();
        let input = beads.len();
        let last = materials.len().saturating_sub(1) as u32;
        let mut out: Vec<Bead> = Vec::with_capacity(beads.len() + beads.len() / 4);
        for mut b in beads {
            let finite = b.a.iter().chain(b.b.iter()).all(|v| v.is_finite())
                && b.rx.is_finite()
                && b.rz.is_finite();
            if !finite || b.rx <= 0.0 || b.rz <= 0.0 {
                continue;
            }
            b.material = b.material.min(last);
            let d = sub(b.b, b.a);
            let len = dot(d, d).sqrt();
            let pieces = if max_len > 0.0 && len > max_len {
                (len / max_len).ceil() as usize
            } else {
                1
            };
            if pieces <= 1 {
                out.push(b);
                continue;
            }
            for i in 0..pieces {
                let t0 = i as f32 / pieces as f32;
                let t1 = (i + 1) as f32 / pieces as f32;
                out.push(Bead {
                    a: add(b.a, scale(d, t0)),
                    b: add(b.a, scale(d, t1)),
                    ..b
                });
            }
        }
        let mat_class: Vec<u8> = if materials.is_empty() {
            vec![CLASS_OPAQUE | CLASS_PLAIN]
        } else {
            materials.iter().map(|m| m.class()).collect()
        };
        let threads = super::aa::resolve_threads(0) as usize;
        let (beads, bin, max_depth) = build(out, threads);
        let class: Vec<u8> = beads
            .iter()
            .map(|b| mat_class[(b.material as usize).min(mat_class.len() - 1)])
            .collect();
        let classes = class.iter().fold(0u8, |m, c| m | c);
        let bounds = bin
            .first()
            .filter(|_| !beads.is_empty())
            .map_or(([0.0; 3], [0.0; 3]), |n| (n.lo, n.hi));
        let nodes = collapse(&bin, &class);
        let bytes = beads.capacity() * std::mem::size_of::<Bead>()
            + nodes.capacity() * std::mem::size_of::<Node4>()
            + class.capacity();
        let stats = BeadStats {
            input,
            beads: beads.len(),
            nodes: nodes.len(),
            max_depth,
            build_ms: super::aa::now_ms() - t0,
            bytes,
        };
        BeadSet {
            beads,
            class,
            nodes,
            bounds,
            classes,
            materials,
            floor_z: None,
            stats,
        }
    }

    pub fn len(&self) -> usize {
        self.beads.len()
    }

    pub fn is_empty(&self) -> bool {
        self.beads.is_empty()
    }

    /// The bead at a slot (the tree's order, not the input's).
    pub fn bead(&self, i: u32) -> &Bead {
        &self.beads[i as usize]
    }

    /// The class of the bead at a slot.
    pub fn class_of(&self, i: u32) -> u8 {
        self.class[i as usize]
    }

    /// Whether any bead has a class in `mask`.
    pub fn has(&self, mask: u8) -> bool {
        self.classes & mask != 0
    }

    /// Bounding box of every bead, or `None` for an empty set.
    pub fn bounds(&self) -> Option<([f32; 3], [f32; 3])> {
        (!self.beads.is_empty()).then_some(self.bounds)
    }

    /// Nearest crossing of a bead surface in `(tmin, tmax)` among the beads
    /// whose class is in `mask`. A crossing is an entry or an exit; a ray
    /// that starts inside a bead meets its exit.
    pub fn intersect_mask(
        &self,
        o: [f32; 3],
        d: [f32; 3],
        tmin: f32,
        tmax: f32,
        mask: u8,
    ) -> Option<BeadHit> {
        if self.classes & mask == 0 {
            return None;
        }
        let rb = RayBox::new(o, d);
        let mut best: Option<BeadHit> = None;
        let mut tbest = tmax;
        let mut stack = [0u64; STACK];
        let mut tstack = [0f32; STACK];
        let mut sp = 0usize;
        let mut node = 0u32;
        loop {
            let n = &self.nodes[node as usize];
            let (tn, mut bits) = rb.test(n, tmin, tbest);
            for c in 0..4 {
                if n.mask[c] & mask == 0 {
                    bits &= !(1 << c);
                }
            }
            // Push the children entered, farthest first, so the nearest is
            // popped next.
            let base = sp;
            while bits != 0 {
                let c = bits.trailing_zeros() as usize;
                bits &= bits - 1;
                let (e, t) = (pack(n.child[c], n.count[c]), tn[c]);
                let mut j = sp;
                while j > base && tstack[j - 1] < t {
                    stack[j] = stack[j - 1];
                    tstack[j] = tstack[j - 1];
                    j -= 1;
                }
                stack[j] = e;
                tstack[j] = t;
                sp += 1;
            }
            loop {
                if sp == 0 {
                    return best;
                }
                sp -= 1;
                if tstack[sp] > tbest {
                    continue;
                }
                let e = stack[sp];
                if e & LEAF_BIT == 0 {
                    node = e as u32;
                    break;
                }
                let first = e as u32;
                let count = ((e >> 32) & 0x7fff_ffff) as u32;
                for i in first..first + count {
                    if self.class[i as usize] & mask == 0 {
                        continue;
                    }
                    if let Some((t0, t1)) = self.beads[i as usize].interval(o, d) {
                        let t = if t0 > tmin { t0 } else { t1 };
                        if t > tmin && t < tbest {
                            tbest = t;
                            best = Some(BeadHit { t, bead: i });
                        }
                    }
                }
            }
        }
    }

    /// [`BeadSet::intersect_mask`] over every bead.
    pub fn intersect(&self, o: [f32; 3], d: [f32; 3], tmin: f32, tmax: f32) -> Option<BeadHit> {
        self.intersect_mask(o, d, tmin, tmax, CLASS_ALL)
    }

    /// Visit every leaf whose box the ray enters within `(tmin, tmax)` and
    /// that holds a class in `mask`, in no particular order; `leaf` returns
    /// true to stop.
    #[inline]
    fn leaves(
        &self,
        o: [f32; 3],
        d: [f32; 3],
        tmin: f32,
        tmax: f32,
        mask: u8,
        mut leaf: impl FnMut(u32, u32) -> bool,
    ) {
        if self.classes & mask == 0 {
            return;
        }
        let rb = RayBox::new(o, d);
        let mut stack = [0u32; STACK];
        let mut sp = 0usize;
        let mut node = 0u32;
        loop {
            let n = &self.nodes[node as usize];
            let (_, mut bits) = rb.test(n, tmin, tmax);
            while bits != 0 {
                let c = bits.trailing_zeros() as usize;
                bits &= bits - 1;
                if n.mask[c] & mask == 0 {
                    continue;
                }
                if n.count[c] > 0 {
                    if leaf(n.child[c], n.count[c] as u32) {
                        return;
                    }
                } else {
                    stack[sp] = n.child[c];
                    sp += 1;
                }
            }
            if sp == 0 {
                return;
            }
            sp -= 1;
            node = stack[sp];
        }
    }

    /// Whether any bead whose class is in `mask` crosses `(tmin, tmax)`.
    pub fn occluded_mask(&self, o: [f32; 3], d: [f32; 3], tmin: f32, tmax: f32, mask: u8) -> bool {
        let mut hit = false;
        self.leaves(o, d, tmin, tmax, mask, |first, count| {
            for i in first..first + count {
                if self.class[i as usize] & mask == 0 {
                    continue;
                }
                if let Some((t0, t1)) = self.beads[i as usize].interval(o, d) {
                    if (t0 > tmin && t0 < tmax) || (t1 > tmin && t1 < tmax) {
                        hit = true;
                        return true;
                    }
                }
            }
            false
        });
        hit
    }

    /// Every bead whose class is in `mask` and whose interval along the
    /// ray overlaps `(tmin, tmax)`, as `(t_in, t_out, bead)`, appended to
    /// `out` in no particular order.
    pub fn intervals_mask(
        &self,
        o: [f32; 3],
        d: [f32; 3],
        tmin: f32,
        tmax: f32,
        mask: u8,
        out: &mut Vec<(f32, f32, u32)>,
    ) {
        self.leaves(o, d, tmin, tmax, mask, |first, count| {
            for i in first..first + count {
                if self.class[i as usize] & mask == 0 {
                    continue;
                }
                if let Some((t0, t1)) = self.beads[i as usize].interval(o, d) {
                    if t1 > tmin && t0 < tmax {
                        out.push((t0, t1, i));
                    }
                }
            }
            false
        });
    }

    /// Whether `p` lies inside some bead other than `skip` whose class is in
    /// `mask` (by more than a thousandth of its radius). Used to tell the
    /// union's real surface from a crossing buried inside a neighbour.
    pub fn inside_other(&self, p: [f32; 3], skip: u32, mask: u8) -> bool {
        if self.beads.is_empty() || self.classes & mask == 0 {
            return false;
        }
        let mut stack = [0u32; STACK];
        let mut sp = 0usize;
        let mut node = 0u32;
        loop {
            let n = &self.nodes[node as usize];
            for c in 0..4 {
                if n.mask[c] & mask == 0
                    || !(0..3).all(|k| p[k] >= n.lo[k][c] && p[k] <= n.hi[k][c])
                {
                    continue;
                }
                if n.count[c] > 0 {
                    let first = n.child[c];
                    for i in first..first + n.count[c] as u32 {
                        if i != skip
                            && self.class[i as usize] & mask != 0
                            && self.beads[i as usize].contains(p, 1e-3)
                        {
                            return true;
                        }
                    }
                } else {
                    stack[sp] = n.child[c];
                    sp += 1;
                }
            }
            if sp == 0 {
                return false;
            }
            sp -= 1;
            node = stack[sp];
        }
    }
}

/// The file form of a list of beads: `CPBEADS1`, a little-endian `u32`
/// count, then per bead nine little-endian values, `f32` `a.x a.y a.z b.x
/// b.y b.z rx rz` and `u32` material (36 bytes).
pub const FILE_MAGIC: &[u8; 8] = b"CPBEADS1";
const RECORD: usize = 36;

/// Read a bead file ([`FILE_MAGIC`]). The count must match the bytes.
pub fn decode(bytes: &[u8]) -> Result<Vec<Bead>, String> {
    if bytes.len() < 12 || &bytes[..8] != FILE_MAGIC {
        return Err("not a bead file (expected the CPBEADS1 header)".into());
    }
    let n = u32::from_le_bytes([bytes[8], bytes[9], bytes[10], bytes[11]]) as usize;
    let body = &bytes[12..];
    if body.len() != n.checked_mul(RECORD).ok_or("bead count overflows")? {
        return Err(format!(
            "bead file declares {n} beads but holds {} bytes of them",
            body.len()
        ));
    }
    let f = |r: &[u8], i: usize| {
        f32::from_le_bytes([r[4 * i], r[4 * i + 1], r[4 * i + 2], r[4 * i + 3]])
    };
    Ok(body
        .chunks_exact(RECORD)
        .map(|r| Bead {
            a: [f(r, 0), f(r, 1), f(r, 2)],
            b: [f(r, 3), f(r, 4), f(r, 5)],
            rx: f(r, 6),
            rz: f(r, 7),
            material: u32::from_le_bytes([r[32], r[33], r[34], r[35]]),
        })
        .collect())
}

/// Write beads in the form [`decode`] reads.
pub fn encode(beads: &[Bead]) -> Vec<u8> {
    let mut out = Vec::with_capacity(12 + beads.len() * RECORD);
    out.extend_from_slice(FILE_MAGIC);
    out.extend_from_slice(&(beads.len() as u32).to_le_bytes());
    for b in beads {
        for v in [b.a[0], b.a[1], b.a[2], b.b[0], b.b[1], b.b[2], b.rx, b.rz] {
            out.extend_from_slice(&v.to_le_bytes());
        }
        out.extend_from_slice(&b.material.to_le_bytes());
    }
    out
}

/// One stretch of a ray inside a union of beads: entered at `t_in` through
/// bead `b_in`, left at `t_out` through bead `b_out`.
#[derive(Clone, Copy, Debug, PartialEq)]
pub struct Span {
    pub t_in: f32,
    pub t_out: f32,
    pub b_in: u32,
    pub b_out: u32,
}

/// The spans of the union of intervals `(t_in, t_out, bead)`, sorted, written
/// over `spans`. A union of overlapping beads is one solid along the ray:
/// its surface is crossed only where a span begins or ends. `iv` is sorted
/// in place.
pub fn union_spans(iv: &mut [(f32, f32, u32)], spans: &mut Vec<Span>) {
    spans.clear();
    iv.sort_unstable_by(|a, b| {
        a.0.partial_cmp(&b.0)
            .unwrap_or(std::cmp::Ordering::Equal)
            .then(a.2.cmp(&b.2))
    });
    for &(t0, t1, b) in iv.iter() {
        match spans.last_mut() {
            Some(last) if t0 <= last.t_out => {
                if t1 > last.t_out {
                    last.t_out = t1;
                    last.b_out = b;
                }
            }
            _ => spans.push(Span {
                t_in: t0,
                t_out: t1,
                b_in: b,
                b_out: b,
            }),
        }
    }
}

#[inline]
fn inv_dir(d: [f32; 3]) -> [f32; 3] {
    let f = |v: f32| {
        if v.abs() > 1e-30 {
            1.0 / v
        } else {
            1e30f32.copysign(v)
        }
    };
    [f(d[0]), f(d[1]), f(d[2])]
}

// ---------------------------------------------------------------------------
// Build.

#[derive(Clone, Copy)]
struct Prim {
    lo: [f32; 3],
    hi: [f32; 3],
    c: [f32; 3],
    i: u32,
}

#[derive(Clone, Copy)]
struct BinBox {
    lo: [f32; 3],
    hi: [f32; 3],
    n: u32,
}

impl BinBox {
    const EMPTY: BinBox = BinBox {
        lo: [f32::INFINITY; 3],
        hi: [f32::NEG_INFINITY; 3],
        n: 0,
    };
    fn grow(&mut self, lo: [f32; 3], hi: [f32; 3], n: u32) {
        for k in 0..3 {
            self.lo[k] = self.lo[k].min(lo[k]);
            self.hi[k] = self.hi[k].max(hi[k]);
        }
        self.n += n;
    }
    fn area(&self) -> f32 {
        if self.n == 0 {
            return 0.0;
        }
        let e = [
            self.hi[0] - self.lo[0],
            self.hi[1] - self.lo[1],
            self.hi[2] - self.lo[2],
        ];
        2.0 * (e[0] * e[1] + e[1] * e[2] + e[2] * e[0])
    }
}

/// The box of a range of prims, and whether and where to split it (the
/// range is partitioned in place). `None` makes it a leaf.
fn split(prims: &mut [Prim], depth: usize) -> (BinBox, Option<usize>) {
    let mut bb = BinBox::EMPTY;
    let mut cb = BinBox::EMPTY;
    for p in prims.iter() {
        bb.grow(p.lo, p.hi, 1);
        cb.grow(p.c, p.c, 1);
    }
    let n = prims.len();
    if n <= LEAF_MAX || depth + 1 >= MAX_DEPTH {
        return (bb, None);
    }
    // Best split over the three axes.
    let mut best: Option<(f32, usize, usize)> = None; // (cost, axis, bins on the left)
    for axis in 0..3 {
        let ext = cb.hi[axis] - cb.lo[axis];
        if ext <= 1e-9 {
            continue;
        }
        let k = BINS as f32 / ext;
        let mut bins = [BinBox::EMPTY; BINS];
        for p in prims.iter() {
            let b = (((p.c[axis] - cb.lo[axis]) * k) as usize).min(BINS - 1);
            bins[b].grow(p.lo, p.hi, 1);
        }
        let mut right_area = [0.0f32; BINS];
        let mut right_n = [0u32; BINS];
        let mut acc = BinBox::EMPTY;
        for b in (1..BINS).rev() {
            acc.grow(bins[b].lo, bins[b].hi, bins[b].n);
            right_area[b] = acc.area();
            right_n[b] = acc.n;
        }
        let mut left = BinBox::EMPTY;
        for b in 0..BINS - 1 {
            left.grow(bins[b].lo, bins[b].hi, bins[b].n);
            if left.n == 0 || right_n[b + 1] == 0 {
                continue;
            }
            let cost = left.area() * left.n as f32 + right_area[b + 1] * right_n[b + 1] as f32;
            if best.is_none_or(|(c, _, _)| cost < c) {
                best = Some((cost, axis, b + 1));
            }
        }
    }
    let leaf_cost = bb.area() * n as f32;
    let mid = match best {
        // Traversal costs about as much as one bead test; below the
        // leaf's own cost (and while the leaf is small) stop here.
        Some((cost, axis, nl)) if cost + bb.area() < leaf_cost || n > 16 => {
            let ext = cb.hi[axis] - cb.lo[axis];
            let k = BINS as f32 / ext;
            let lo = cb.lo[axis];
            let mut i = 0usize;
            let mut j = prims.len();
            while i < j {
                let b = (((prims[i].c[axis] - lo) * k) as usize).min(BINS - 1);
                if b < nl {
                    i += 1;
                } else {
                    j -= 1;
                    prims.swap(i, j);
                }
            }
            Some(i)
        }
        Some(_) => None,
        // Every centroid in one place: halve the list.
        None if n > 16 => Some(n / 2),
        None => None,
    };
    (bb, mid.filter(|&m| m > 0 && m < n))
}

/// The binary tree over `prims` (which start at slot `offset` of the whole
/// list), root first, child indices local to the returned list.
fn build_sub(prims: &mut [Prim], offset: usize, depth0: usize) -> (Vec<Node>, usize) {
    let mut nodes: Vec<Node> = Vec::with_capacity(2 * prims.len() / LEAF_MAX + 1);
    nodes.push(Node::default());
    let mut max_depth = depth0;
    let mut work: Vec<(u32, usize, usize, usize)> = vec![(0, 0, prims.len(), depth0)];
    while let Some((ni, start, end, depth)) = work.pop() {
        max_depth = max_depth.max(depth);
        let (bb, mid) = split(&mut prims[start..end], depth);
        let Some(mid) = mid else {
            nodes[ni as usize] = Node {
                lo: bb.lo,
                hi: bb.hi,
                first: (offset + start) as u32,
                count: (end - start) as u32,
            };
            continue;
        };
        let left = nodes.len() as u32;
        nodes.push(Node::default());
        nodes.push(Node::default());
        nodes[ni as usize] = Node {
            lo: bb.lo,
            hi: bb.hi,
            first: left,
            count: 0,
        };
        work.push((left + 1, start + mid, end, depth + 1));
        work.push((left, start, start + mid, depth + 1));
    }
    (nodes, max_depth)
}

/// Top-down binned SAH. The levels above [`PAR_GRAIN`] beads are split
/// one after another; each range below is then an independent task, built
/// on `threads` threads and stitched in task order, so the tree does not
/// depend on the thread count. Returns the beads in leaf order, the binary
/// nodes (root first) and the depth reached.
fn build(beads: Vec<Bead>, threads: usize) -> (Vec<Bead>, Vec<Node>, usize) {
    if beads.is_empty() {
        return (beads, vec![Node::default()], 0);
    }
    let mut prims: Vec<Prim> = beads
        .iter()
        .enumerate()
        .map(|(i, b)| {
            let (lo, hi) = b.bounds();
            Prim {
                lo,
                hi,
                c: scale(add(lo, hi), 0.5),
                i: i as u32,
            }
        })
        .collect();
    // Sequential top: (slot, start, end, depth) ranges become tasks once
    // small enough.
    let mut nodes: Vec<Node> = vec![Node::default()];
    let mut tasks: Vec<(u32, usize, usize, usize)> = Vec::new();
    let mut work = vec![(0u32, 0usize, prims.len(), 0usize)];
    let mut max_depth = 0usize;
    while let Some((ni, start, end, depth)) = work.pop() {
        if end - start <= PAR_GRAIN {
            tasks.push((ni, start, end, depth));
            continue;
        }
        max_depth = max_depth.max(depth);
        let (bb, mid) = split(&mut prims[start..end], depth);
        let Some(mid) = mid else {
            nodes[ni as usize] = Node {
                lo: bb.lo,
                hi: bb.hi,
                first: start as u32,
                count: (end - start) as u32,
            };
            continue;
        };
        let left = nodes.len() as u32;
        nodes.push(Node::default());
        nodes.push(Node::default());
        nodes[ni as usize] = Node {
            lo: bb.lo,
            hi: bb.hi,
            first: left,
            count: 0,
        };
        work.push((left + 1, start + mid, end, depth + 1));
        work.push((left, start, start + mid, depth + 1));
    }
    tasks.sort_by_key(|t| t.1);
    // Disjoint slices of `prims`, one per task, in order.
    let mut slices: Vec<(usize, &mut [Prim])> = Vec::with_capacity(tasks.len());
    {
        let mut rest: &mut [Prim] = &mut prims;
        let mut at = 0usize;
        for &(_, start, end, _) in &tasks {
            let (_, tail) = std::mem::take(&mut rest).split_at_mut(start - at);
            let (mine, tail) = tail.split_at_mut(end - start);
            slices.push((start, mine));
            rest = tail;
            at = end;
        }
    }
    let depths: Vec<usize> = tasks.iter().map(|t| t.3).collect();
    let mut results: Vec<Option<(Vec<Node>, usize)>> = (0..tasks.len()).map(|_| None).collect();
    let threads = if cfg!(target_arch = "wasm32") {
        1
    } else {
        threads.max(1)
    };
    if threads == 1 || tasks.len() <= 1 {
        for (k, (off, s)) in slices.into_iter().enumerate() {
            results[k] = Some(build_sub(s, off, depths[k]));
        }
    } else {
        let queue = std::sync::Mutex::new(slices.into_iter().zip(results.iter_mut()).enumerate());
        std::thread::scope(|sc| {
            for _ in 0..threads.min(tasks.len()) {
                sc.spawn(|| {
                    loop {
                        let next = queue.lock().expect("build queue").next();
                        let Some((k, ((off, s), slot))) = next else {
                            break;
                        };
                        *slot = Some(build_sub(s, off, depths[k]));
                    }
                });
            }
        });
    }
    // Stitch: a task's root goes into its slot; its other nodes are
    // appended, their inner links moved by where they landed.
    for (k, r) in results.into_iter().enumerate() {
        let (sub_nodes, d) = r.expect("every task built");
        max_depth = max_depth.max(d);
        let base = nodes.len() as u32;
        let fix = |n: Node| {
            if n.count == 0 {
                Node {
                    first: n.first + base - 1,
                    ..n
                }
            } else {
                n
            }
        };
        nodes[tasks[k].0 as usize] = fix(sub_nodes[0]);
        nodes.extend(sub_nodes[1..].iter().map(|&n| fix(n)));
    }
    let ordered: Vec<Bead> = prims.iter().map(|p| beads[p.i as usize]).collect();
    (ordered, nodes, max_depth)
}

/// The four-wide tree from the binary one: each node takes up to four of
/// the binary tree's descendants, opening the largest inner one first.
fn collapse(bin: &[Node], class: &[u8]) -> Vec<Node4> {
    if bin.len() == 1 {
        // One leaf (or none): a root with a single child.
        let n = &bin[0];
        let mut root = Node4::EMPTY;
        if n.count > 0 {
            for ax in 0..3 {
                root.lo[ax][0] = n.lo[ax];
                root.hi[ax][0] = n.hi[ax];
            }
            root.child[0] = n.first;
            root.count[0] = n.count.min(u16::MAX as u32) as u16;
            root.mask[0] = class[n.first as usize..(n.first + n.count) as usize]
                .iter()
                .fold(0, |m, c| m | c);
        }
        return vec![root];
    }
    // Classes under each binary node, children before parents.
    let mut mask = vec![0u8; bin.len()];
    let mut order = Vec::with_capacity(bin.len());
    let mut stack = vec![0u32];
    while let Some(i) = stack.pop() {
        order.push(i);
        let n = &bin[i as usize];
        if n.count == 0 && bin.len() > 1 {
            stack.push(n.first);
            stack.push(n.first + 1);
        }
    }
    for &i in order.iter().rev() {
        let n = &bin[i as usize];
        mask[i as usize] = if n.count > 0 {
            class[n.first as usize..(n.first + n.count) as usize]
                .iter()
                .fold(0, |m, c| m | c)
        } else {
            mask[n.first as usize] | mask[n.first as usize + 1]
        };
    }
    let area = |n: &Node| {
        let e = [n.hi[0] - n.lo[0], n.hi[1] - n.lo[1], n.hi[2] - n.lo[2]];
        e[0] * e[1] + e[1] * e[2] + e[2] * e[0]
    };
    let mut out: Vec<Node4> = vec![Node4::EMPTY];
    // (binary node, four-wide slot it fills)
    let mut work = vec![(0u32, 0u32)];
    while let Some((bi, slot)) = work.pop() {
        let root = &bin[bi as usize];
        let mut kids: Vec<u32> = if root.count > 0 {
            vec![bi]
        } else {
            vec![root.first, root.first + 1]
        };
        while kids.len() < 4 {
            let pick = kids
                .iter()
                .enumerate()
                .filter(|(_, k)| bin[**k as usize].count == 0)
                .max_by(|a, b| {
                    area(&bin[*a.1 as usize])
                        .partial_cmp(&area(&bin[*b.1 as usize]))
                        .unwrap_or(std::cmp::Ordering::Equal)
                });
            let Some((j, &k)) = pick else { break };
            let f = bin[k as usize].first;
            kids[j] = f;
            kids.push(f + 1);
        }
        let mut node = Node4::EMPTY;
        for (c, &k) in kids.iter().enumerate() {
            let n = &bin[k as usize];
            for ax in 0..3 {
                node.lo[ax][c] = n.lo[ax];
                node.hi[ax][c] = n.hi[ax];
            }
            node.mask[c] = mask[k as usize];
            if n.count > 0 {
                node.child[c] = n.first;
                node.count[c] = n.count.min(u16::MAX as u32) as u16;
            } else {
                let s = out.len() as u32;
                out.push(Node4::EMPTY);
                node.child[c] = s;
                work.push((k, s));
            }
        }
        out[slot as usize] = node;
    }
    out
}

#[cfg(test)]
mod tests {
    use super::super::normalize;
    use super::*;

    fn lcg(s: &mut u64) -> f32 {
        *s = s
            .wrapping_mul(6364136223846793005)
            .wrapping_add(1442695040888963407);
        ((*s >> 40) as f32) / ((1u64 << 24) as f32)
    }

    fn bead(a: [f32; 3], b: [f32; 3]) -> Bead {
        Bead {
            a,
            b,
            rx: 0.25,
            rz: 0.1,
            material: 0,
        }
    }

    /// The interval as the union of the two end spheres and the clipped
    /// cylinder, each solved in full: the definition the fast form must
    /// agree with.
    fn reference_interval(bd: &Bead, o: [f32; 3], d: [f32; 3]) -> Option<(f32, f32)> {
        let s = bd.rx / bd.rz.max(1e-9);
        let a = [bd.a[0], bd.a[1], bd.a[2] * s];
        let b = [bd.b[0], bd.b[1], bd.b[2] * s];
        let dd = [d[0], d[1], d[2] * s];
        let oo = [o[0], o[1], o[2] * s];
        let rdrd = dot(dd, dd);
        let tc = dot(sub(a, oo), dd) / rdrd;
        let oc = add(oo, scale(dd, tc));
        let r2 = bd.rx * bd.rx;
        let mut tn = f32::INFINITY;
        let mut tf = f32::NEG_INFINITY;
        for c in [a, b] {
            if let Some((x, y)) = sphere_interval(sub(oc, c), dd, rdrd, r2) {
                tn = tn.min(x);
                tf = tf.max(y);
            }
        }
        let ba = sub(b, a);
        let baba = dot(ba, ba);
        let oa = sub(oc, a);
        let bard = dot(ba, dd);
        let baoa = dot(ba, oa);
        let qa = baba * rdrd - bard * bard;
        if qa > 1e-9 * baba * rdrd {
            let qb = baba * dot(dd, oa) - baoa * bard;
            let qc = baba * dot(oa, oa) - baoa * baoa - r2 * baba;
            let h = qb * qb - qa * qc;
            if h >= 0.0 {
                let sh = h.sqrt();
                let mut t0 = (-qb - sh) / qa;
                let mut t1 = (-qb + sh) / qa;
                let y0 = -baoa / bard;
                let y1 = (baba - baoa) / bard;
                t0 = t0.max(y0.min(y1));
                t1 = t1.min(y0.max(y1));
                if t0 <= t1 {
                    tn = tn.min(t0);
                    tf = tf.max(t1);
                }
            }
        }
        (tn <= tf).then(|| (tn + tc, tf + tc))
    }

    #[test]
    fn the_fast_interval_is_the_union_of_its_parts() {
        let mut s = 11u64;
        let mut hits = 0;
        for _ in 0..200_000 {
            let a = [lcg(&mut s) * 4.0, lcg(&mut s) * 4.0, lcg(&mut s) * 2.0];
            let ang = lcg(&mut s) * 6.283;
            let len = lcg(&mut s) * 3.0;
            let rise = (lcg(&mut s) - 0.5) * 0.3;
            let bd = Bead {
                a,
                b: [a[0] + ang.cos() * len, a[1] + ang.sin() * len, a[2] + rise],
                rx: 0.1 + lcg(&mut s) * 0.4,
                rz: 0.05 + lcg(&mut s) * 0.2,
                material: 0,
            };
            let o = [
                lcg(&mut s) * 20.0 - 8.0,
                lcg(&mut s) * 20.0 - 8.0,
                lcg(&mut s) * 20.0 - 8.0,
            ];
            let tgt = [
                a[0] + lcg(&mut s) - 0.5,
                a[1] + lcg(&mut s) - 0.5,
                a[2] + lcg(&mut s) * 0.4 - 0.2,
            ];
            let d = normalize(sub(tgt, o));
            match (bd.interval(o, d), reference_interval(&bd, o, d)) {
                (Some(x), Some(y)) => {
                    hits += 1;
                    assert!(
                        (x.0 - y.0).abs() < 2e-3 && (x.1 - y.1).abs() < 2e-3,
                        "{bd:?} {o:?} {d:?}: {x:?} vs {y:?}"
                    );
                }
                (None, None) => {}
                // A ray that grazes the surface may land either way.
                (x, y) => {
                    let w = x.or(y).unwrap();
                    assert!(w.1 - w.0 < 2e-2, "{bd:?} {o:?} {d:?}: {x:?} vs {y:?}");
                }
            }
        }
        assert!(hits > 20_000, "{hits}");
    }

    #[test]
    fn a_ray_down_onto_a_bead_meets_its_top() {
        let b = bead([0.0, 0.0, 1.0], [10.0, 0.0, 1.0]);
        let (t0, t1) = b.interval([5.0, 0.0, 100.0], [0.0, 0.0, -1.0]).unwrap();
        assert!((t0 - 98.9).abs() < 1e-4, "{t0}");
        assert!((t1 - 99.1).abs() < 1e-4, "{t1}");
        let n = b.normal([5.0, 0.0, 1.1]);
        assert!((n[2] - 1.0).abs() < 1e-5, "{n:?}");
    }

    #[test]
    fn a_ray_from_the_side_meets_the_full_width_and_a_round_end() {
        let b = bead([0.0, 0.0, 0.0], [10.0, 0.0, 0.0]);
        let (t0, _) = b.interval([5.0, -50.0, 0.0], [0.0, 1.0, 0.0]).unwrap();
        assert!((t0 - 49.75).abs() < 1e-4, "{t0}");
        // Along the axis from beyond `b`: the end cap is a half spheroid.
        let (t0, t1) = b.interval([20.0, 0.0, 0.0], [-1.0, 0.0, 0.0]).unwrap();
        assert!(
            (t0 - 9.75).abs() < 1e-4 && (t1 - 20.25).abs() < 1e-4,
            "{t0} {t1}"
        );
        // Just above the ellipse misses.
        assert!(b.interval([5.0, -50.0, 0.11], [0.0, 1.0, 0.0]).is_none());
        assert!(b.interval([5.0, -50.0, 0.09], [0.0, 1.0, 0.0]).is_some());
        // Past the end, square to the axis: only the round end is there.
        assert!(b.interval([10.2, -50.0, 0.0], [0.0, 1.0, 0.0]).is_some());
        assert!(b.interval([10.3, -50.0, 0.0], [0.0, 1.0, 0.0]).is_none());
    }

    #[test]
    fn normals_are_unit_and_point_out_of_the_ellipse() {
        let b = bead([0.0, 0.0, 0.0], [0.0, 10.0, 0.0]);
        let mut s = 7u64;
        for _ in 0..200 {
            let th = lcg(&mut s) * std::f32::consts::TAU;
            // A point on the ellipse x = rx cos, z = rz sin at the middle.
            let p = [0.25 * th.cos(), 5.0, 0.1 * th.sin()];
            let n = b.normal(p);
            assert!((dot(n, n) - 1.0).abs() < 1e-4);
            // The ellipse's gradient is (x / rx^2, z / rz^2).
            let g = normalize([p[0] / 0.0625, 0.0, p[2] / 0.01]);
            assert!(dot(n, g) > 0.9999, "{n:?} {g:?}");
        }
    }

    fn random_beads(seed: u64, n: usize) -> Vec<Bead> {
        let mut s = seed;
        let mut beads = Vec::new();
        for i in 0..n {
            let a = [
                lcg(&mut s) * 40.0,
                lcg(&mut s) * 40.0,
                (lcg(&mut s) * 50.0).floor() * 0.2,
            ];
            let ang = lcg(&mut s) * 6.283;
            let len = lcg(&mut s) * 12.0;
            let b = [a[0] + ang.cos() * len, a[1] + ang.sin() * len, a[2]];
            beads.push(Bead {
                a,
                b,
                rx: 0.22,
                rz: 0.1,
                material: (i % 3) as u32,
            });
        }
        beads
    }

    /// Three materials: opaque, see-through, and scattering.
    fn three_materials() -> Vec<BeadMaterial> {
        let plain = BeadMaterial::new(Material::default());
        let mut clear = BeadMaterial::new(Material::default());
        clear.material.transmission = 1.0;
        let mut sss = BeadMaterial::new(Material::default());
        sss.subsurface = 0.3;
        vec![plain, clear, sss]
    }

    #[test]
    fn the_tree_finds_what_brute_force_finds() {
        for (seed, n, max_len) in [
            (1u64, 3000usize, 2.0f32),
            (5, 60_000, 3.0),
            (9, 7, 0.0),
            (13, 1, 0.0),
        ] {
            let beads = random_beads(seed, n);
            let mats = three_materials();
            let classes: Vec<u8> = mats.iter().map(|m| m.class()).collect();
            let set = BeadSet::new(beads.clone(), mats, max_len);
            if max_len > 0.0 && n > 100 {
                assert!(set.len() > beads.len(), "long beads are split");
            }
            let mut s = seed ^ 0xabc;
            let mut hits = 0;
            for _ in 0..3000 {
                let o = [
                    lcg(&mut s) * 60.0 - 10.0,
                    lcg(&mut s) * 60.0 - 10.0,
                    30.0 + lcg(&mut s) * 10.0,
                ];
                let tgt = [lcg(&mut s) * 40.0, lcg(&mut s) * 40.0, lcg(&mut s) * 10.0];
                let d = normalize(sub(tgt, o));
                for mask in [
                    CLASS_ALL,
                    CLASS_OPAQUE,
                    CLASS_CLEAR,
                    CLASS_SOLID,
                    CLASS_PLAIN,
                ] {
                    let brute = beads
                        .iter()
                        .filter(|b| classes[b.material as usize] & mask != 0)
                        .filter_map(|b| {
                            b.interval(o, d)
                                .map(|(t0, t1)| if t0 > 1e-4 { t0 } else { t1 })
                        })
                        .filter(|t| *t > 1e-4)
                        .fold(f32::INFINITY, f32::min);
                    let tree = set
                        .intersect_mask(o, d, 1e-4, f32::INFINITY, mask)
                        .map_or(f32::INFINITY, |h| h.t);
                    if brute.is_finite() {
                        hits += 1;
                        assert!(
                            (brute - tree).abs() < 2e-3,
                            "seed {seed} mask {mask}: brute {brute} tree {tree}"
                        );
                    } else {
                        assert!(tree.is_infinite(), "tree found {tree} where nothing is");
                    }
                    assert_eq!(
                        set.occluded_mask(o, d, 1e-4, 1e9, mask),
                        brute.is_finite(),
                        "seed {seed} mask {mask}"
                    );
                    let mut iv = Vec::new();
                    set.intervals_mask(o, d, 1e-4, 1e9, mask, &mut iv);
                    let want = beads
                        .iter()
                        .filter(|b| classes[b.material as usize] & mask != 0)
                        .filter(|b| b.interval(o, d).is_some_and(|(_, t1)| t1 > 1e-4))
                        .count();
                    if max_len == 0.0 {
                        assert_eq!(iv.len(), want, "seed {seed} mask {mask}");
                    } else {
                        assert!(iv.len() >= want, "seed {seed} mask {mask}");
                    }
                }
            }
            assert!(
                n < 100 || hits > 300,
                "the test rays should hit something: {hits}"
            );
        }
    }

    #[test]
    fn the_tree_is_the_same_for_any_thread_count() {
        let beads = random_beads(3, 80_000);
        let (b1, n1, d1) = build(beads.clone(), 1);
        let (b8, n8, d8) = build(beads, 8);
        assert_eq!(b1, b8);
        assert_eq!(d1, d8);
        assert_eq!(n1.len(), n8.len());
        assert!(n1.iter().zip(&n8).all(|(a, b)| a.lo == b.lo
            && a.hi == b.hi
            && a.first == b.first
            && a.count == b.count));
    }

    #[test]
    fn overlapping_intervals_merge_into_one_span() {
        let mut iv = vec![(3.0, 5.0, 1), (0.0, 2.0, 0), (1.5, 3.2, 2), (7.0, 8.0, 3)];
        let mut spans = Vec::new();
        union_spans(&mut iv, &mut spans);
        let flat: Vec<_> = spans
            .iter()
            .map(|s| (s.t_in, s.t_out, s.b_in, s.b_out))
            .collect();
        assert_eq!(flat, vec![(0.0, 5.0, 0, 1), (7.0, 8.0, 3, 3)]);
        // The tree gathers what brute force does.
        let set = BeadSet::new(
            (0..20)
                .map(|i| bead([i as f32 * 0.45, 0.0, 0.0], [i as f32 * 0.45, 10.0, 0.0]))
                .collect(),
            vec![BeadMaterial::new(Material::default())],
            0.0,
        );
        let mut got = Vec::new();
        set.intervals_mask(
            [-5.0, 5.0, 0.0],
            [1.0, 0.0, 0.0],
            0.0,
            100.0,
            CLASS_ALL,
            &mut got,
        );
        assert_eq!(got.len(), 20);
        union_spans(&mut got, &mut spans);
        assert_eq!(spans.len(), 1, "touching lines are one solid: {spans:?}");
        assert!(
            (spans[0].t_in - 4.75).abs() < 1e-4
                && (spans[0].t_out - (5.0 + 19.0 * 0.45 + 0.25)).abs() < 1e-3
        );
    }

    #[test]
    fn the_file_form_round_trips_and_refuses_a_short_file() {
        let beads = vec![
            bead([0.0, 1.0, 2.0], [3.0, 4.0, 5.0]),
            Bead {
                material: 3,
                ..bead([1.0; 3], [2.0; 3])
            },
        ];
        let bytes = encode(&beads);
        assert_eq!(decode(&bytes).unwrap(), beads);
        assert!(decode(&bytes[..bytes.len() - 1]).is_err());
        assert!(decode(b"CPBEADS2\0\0\0\0").is_err());
    }

    #[test]
    fn decoding_arbitrary_bytes_never_panics_and_a_lying_count_is_refused() {
        let mut s = 3u64;
        for n in 0..2000usize {
            let mut b: Vec<u8> = (0..n % 120).map(|_| (lcg(&mut s) * 256.0) as u8).collect();
            if n % 2 == 0 && b.len() >= 12 {
                b[..8].copy_from_slice(FILE_MAGIC);
            }
            let _ = decode(&b);
        }
        let mut b = encode(&[bead([0.0; 3], [1.0; 3])]);
        b[8] = 200;
        assert!(decode(&b).is_err());
        // A set built from garbage beads drops the ones it cannot draw.
        let bad = Bead {
            a: [f32::NAN, 0.0, 0.0],
            ..bead([0.0; 3], [1.0; 3])
        };
        let set = BeadSet::new(
            vec![
                bad,
                Bead {
                    rx: 0.0,
                    ..bead([0.0; 3], [1.0; 3])
                },
            ],
            vec![],
            1.0,
        );
        assert!(set.is_empty() && set.intersect([0.0; 3], [1.0, 0.0, 0.0], 0.0, 1.0).is_none());
        assert!(!set.occluded_mask([0.0; 3], [1.0, 0.0, 0.0], 0.0, 1.0, CLASS_ALL));
    }

    #[test]
    fn a_crossing_buried_in_a_neighbour_is_inside_it() {
        // Two beads of one path, meeting at x = 5.
        let set = BeadSet::new(
            vec![
                bead([0.0, 0.0, 0.0], [5.0, 0.0, 0.0]),
                bead([5.0, 0.0, 0.0], [10.0, 0.0, 0.0]),
            ],
            vec![BeadMaterial::new(Material::default())],
            0.0,
        );
        // A ray along the axis from inside the first bead meets its end cap
        // at x = 5.25, which is inside the second.
        let h = set
            .intersect([1.0, 0.0, 0.0], [1.0, 0.0, 0.0], 1e-4, 100.0)
            .unwrap();
        let p = [1.0 + h.t, 0.0, 0.0];
        assert!(set.inside_other(p, h.bead, CLASS_ALL));
        // The far end of the second bead is the union's real surface.
        let p = [10.25, 0.0, 0.0];
        let last = (0..2).find(|&i| set.bead(i).b[0] > 9.0).unwrap();
        assert!(!set.inside_other(p, last, CLASS_ALL));
    }

    #[test]
    fn flakes_and_particles_cover_about_their_density() {
        let mut m = BeadMaterial::new(Material::default());
        m.flakes = Some(Flakes {
            density: 0.5,
            size: 0.1,
            tilt_degrees: 30.0,
            roughness: 0.15,
            color: [0.9; 3],
        });
        let mut s = 21u64;
        let (mut flakes, mut tilt) = (0, 0.0f32);
        let n = 40_000;
        for _ in 0..n {
            let p = [lcg(&mut s) * 30.0, lcg(&mut s) * 30.0, lcg(&mut s) * 30.0];
            if let Finish::Flake { normal } = m.finish_at(p, [0.0, 0.0, 1.0]) {
                flakes += 1;
                assert!((dot(normal, normal) - 1.0).abs() < 1e-4);
                assert!(normal[2] >= 30f32.to_radians().cos() - 1e-4, "{normal:?}");
                tilt += normal[2].acos();
            }
        }
        // A sphere of radius 0.3 cells is 11.3 % of a cell.
        let share = flakes as f32 / n as f32;
        assert!((share - 0.5 * 0.113).abs() < 0.01, "{share}");
        assert!(tilt / flakes as f32 > 0.2, "flakes are tilted");
        // Streaks average to the base tone.
        let mut w = BeadMaterial::new(Material::default());
        w.streak = 0.3;
        let mean: f32 = (0..20_000)
            .map(|_| {
                match w.finish_at(
                    [lcg(&mut s) * 200.0, lcg(&mut s) * 200.0, lcg(&mut s) * 30.0],
                    [1.0, 0.0, 0.0],
                ) {
                    Finish::Base { tone } => tone,
                    _ => panic!("no particles"),
                }
            })
            .sum::<f32>()
            / 20_000.0;
        assert!((mean - 1.0).abs() < 0.03, "{mean}");
    }
}
