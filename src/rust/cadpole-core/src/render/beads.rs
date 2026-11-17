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
//! and a binary binned-SAH bounding volume hierarchy over them (32-byte
//! nodes, up to four beads per leaf). Long beads are split into pieces no
//! longer than [`BeadSet::new`]'s `max_len` first, because a long diagonal
//! line has a box mostly made of air.
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
/// Traversal stack depth; the builder never goes deeper than this.
const MAX_DEPTH: usize = 64;

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
        let mut tn = f32::INFINITY;
        let mut tf = f32::NEG_INFINITY;
        // The two end spheres.
        for c in [a, b] {
            let oc2 = sub(oc, c);
            let bq = dot(oc2, dd);
            let cq = dot(oc2, oc2) - r2;
            let h = bq * bq - rdrd * cq;
            if h >= 0.0 {
                let sh = h.sqrt();
                tn = tn.min((-bq - sh) / rdrd);
                tf = tf.max((-bq + sh) / rdrd);
            }
        }
        // The cylinder between them, clipped to the slab of the segment.
        let ba = sub(b, a);
        let baba = dot(ba, ba);
        if baba > 1e-14 {
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
                    if bard.abs() > 1e-20 {
                        let y0 = -baoa / bard;
                        let y1 = (baba - baoa) / bard;
                        t0 = t0.max(y0.min(y1));
                        t1 = t1.min(y0.max(y1));
                    } else if baoa < 0.0 || baoa > baba {
                        t1 = f32::NEG_INFINITY;
                    }
                    if t0 <= t1 {
                        tn = tn.min(t0);
                        tf = tf.max(t1);
                    }
                }
            }
        }
        (tn <= tf).then(|| (tn + tc, tf + tc))
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
}

impl BeadMaterial {
    pub fn new(material: Material) -> BeadMaterial {
        BeadMaterial {
            material,
            subsurface: 0.0,
        }
    }
}

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
    nodes: Vec<Node>,
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
        let (beads, nodes, max_depth) = build(out);
        let bytes = beads.capacity() * std::mem::size_of::<Bead>()
            + nodes.capacity() * std::mem::size_of::<Node>();
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
            nodes,
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

    /// Bounding box of every bead, or `None` for an empty set.
    pub fn bounds(&self) -> Option<([f32; 3], [f32; 3])> {
        self.nodes
            .first()
            .filter(|_| !self.beads.is_empty())
            .map(|n| (n.lo, n.hi))
    }

    /// Nearest crossing of a bead surface in `(tmin, tmax)` for which
    /// `accept` is true. A crossing is an entry or an exit; a ray that
    /// starts inside a bead meets its exit.
    pub fn intersect_where(
        &self,
        o: [f32; 3],
        d: [f32; 3],
        tmin: f32,
        tmax: f32,
        accept: impl Fn(u32) -> bool,
    ) -> Option<BeadHit> {
        if self.beads.is_empty() {
            return None;
        }
        let inv = inv_dir(d);
        let mut best: Option<BeadHit> = None;
        let mut tbest = tmax;
        let mut stack = [0u32; MAX_DEPTH * 2];
        let mut sp = 0usize;
        let mut node = 0u32;
        loop {
            let n = &self.nodes[node as usize];
            if n.count > 0 {
                for i in n.first..n.first + n.count {
                    if !accept(i) {
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
            } else {
                let (l, r) = (n.first, n.first + 1);
                let hl = slab(&self.nodes[l as usize], o, inv, tmin, tbest);
                let hr = slab(&self.nodes[r as usize], o, inv, tmin, tbest);
                match (hl, hr) {
                    (Some(a), Some(b)) => {
                        let (near, far) = if a <= b { (l, r) } else { (r, l) };
                        stack[sp] = far;
                        sp += 1;
                        node = near;
                        continue;
                    }
                    (Some(_), None) => {
                        node = l;
                        continue;
                    }
                    (None, Some(_)) => {
                        node = r;
                        continue;
                    }
                    (None, None) => {}
                }
            }
            // Pop, skipping boxes the best hit so far has closed off.
            loop {
                if sp == 0 {
                    return best;
                }
                sp -= 1;
                let c = stack[sp];
                if slab(&self.nodes[c as usize], o, inv, tmin, tbest).is_some() {
                    node = c;
                    break;
                }
            }
        }
    }

    /// [`BeadSet::intersect_where`] over every bead.
    pub fn intersect(&self, o: [f32; 3], d: [f32; 3], tmin: f32, tmax: f32) -> Option<BeadHit> {
        self.intersect_where(o, d, tmin, tmax, |_| true)
    }

    /// Whether any bead for which `accept` holds crosses `(tmin, tmax)`.
    pub fn occluded_where(
        &self,
        o: [f32; 3],
        d: [f32; 3],
        tmin: f32,
        tmax: f32,
        accept: impl Fn(u32) -> bool,
    ) -> bool {
        if self.beads.is_empty() {
            return false;
        }
        let inv = inv_dir(d);
        let mut stack = [0u32; MAX_DEPTH * 2];
        let mut sp = 0usize;
        let mut node = 0u32;
        if slab(&self.nodes[0], o, inv, tmin, tmax).is_none() {
            return false;
        }
        loop {
            let n = &self.nodes[node as usize];
            if n.count > 0 {
                for i in n.first..n.first + n.count {
                    if !accept(i) {
                        continue;
                    }
                    if let Some((t0, t1)) = self.beads[i as usize].interval(o, d) {
                        if (t0 > tmin && t0 < tmax) || (t1 > tmin && t1 < tmax) {
                            return true;
                        }
                    }
                }
            } else {
                let (l, r) = (n.first, n.first + 1);
                let hl = slab(&self.nodes[l as usize], o, inv, tmin, tmax).is_some();
                let hr = slab(&self.nodes[r as usize], o, inv, tmin, tmax).is_some();
                if hl && hr {
                    stack[sp] = r;
                    sp += 1;
                    node = l;
                    continue;
                } else if hl {
                    node = l;
                    continue;
                } else if hr {
                    node = r;
                    continue;
                }
            }
            if sp == 0 {
                return false;
            }
            sp -= 1;
            node = stack[sp];
        }
    }

    /// Every bead for which `accept` holds whose interval along the ray
    /// overlaps `(tmin, tmax)`, as `(t_in, t_out, bead)`, appended to `out`
    /// in no particular order.
    pub fn intervals_where(
        &self,
        o: [f32; 3],
        d: [f32; 3],
        tmin: f32,
        tmax: f32,
        accept: impl Fn(u32) -> bool,
        out: &mut Vec<(f32, f32, u32)>,
    ) {
        if self.beads.is_empty() {
            return;
        }
        let inv = inv_dir(d);
        if slab(&self.nodes[0], o, inv, tmin, tmax).is_none() {
            return;
        }
        let mut stack = [0u32; MAX_DEPTH * 2];
        let mut sp = 0usize;
        let mut node = 0u32;
        loop {
            let n = &self.nodes[node as usize];
            if n.count > 0 {
                for i in n.first..n.first + n.count {
                    if !accept(i) {
                        continue;
                    }
                    if let Some((t0, t1)) = self.beads[i as usize].interval(o, d) {
                        if t1 > tmin && t0 < tmax {
                            out.push((t0, t1, i));
                        }
                    }
                }
            } else {
                let (l, r) = (n.first, n.first + 1);
                let hl = slab(&self.nodes[l as usize], o, inv, tmin, tmax).is_some();
                let hr = slab(&self.nodes[r as usize], o, inv, tmin, tmax).is_some();
                if hl && hr {
                    stack[sp] = r;
                    sp += 1;
                    node = l;
                    continue;
                } else if hl {
                    node = l;
                    continue;
                } else if hr {
                    node = r;
                    continue;
                }
            }
            if sp == 0 {
                return;
            }
            sp -= 1;
            node = stack[sp];
        }
    }

    /// Whether `p` lies inside some bead other than `skip` (by more than a
    /// thousandth of its radius). Used to tell the union's real surface
    /// from a crossing buried inside a neighbour.
    pub fn inside_other(&self, p: [f32; 3], skip: u32, accept: impl Fn(u32) -> bool) -> bool {
        if self.beads.is_empty() {
            return false;
        }
        let mut stack = [0u32; MAX_DEPTH * 2];
        let mut sp = 0usize;
        let mut node = 0u32;
        let inside = |n: &Node| (0..3).all(|k| p[k] >= n.lo[k] && p[k] <= n.hi[k]);
        if !inside(&self.nodes[0]) {
            return false;
        }
        loop {
            let n = &self.nodes[node as usize];
            if n.count > 0 {
                for i in n.first..n.first + n.count {
                    if i != skip && accept(i) && self.beads[i as usize].contains(p, 1e-3) {
                        return true;
                    }
                }
            } else {
                let (l, r) = (n.first, n.first + 1);
                let hl = inside(&self.nodes[l as usize]);
                let hr = inside(&self.nodes[r as usize]);
                if hl && hr {
                    stack[sp] = r;
                    sp += 1;
                    node = l;
                    continue;
                } else if hl {
                    node = l;
                    continue;
                } else if hr {
                    node = r;
                    continue;
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
    iv.sort_by(|a, b| a.0.partial_cmp(&b.0).unwrap_or(std::cmp::Ordering::Equal));
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

/// Entry distance of the ray into a node's box within `(tmin, tmax)`.
#[inline]
fn slab(n: &Node, o: [f32; 3], inv: [f32; 3], tmin: f32, tmax: f32) -> Option<f32> {
    let mut t0 = tmin;
    let mut t1 = tmax;
    for k in 0..3 {
        let a = (n.lo[k] - o[k]) * inv[k];
        let b = (n.hi[k] - o[k]) * inv[k];
        let (lo, hi) = if a <= b { (a, b) } else { (b, a) };
        t0 = t0.max(lo);
        t1 = t1.min(hi);
    }
    // A hair of slack: a bead lying exactly on a box face is still found.
    (t0 <= t1 * (1.0 + 4.0 * f32::EPSILON) + 1e-7).then_some(t0)
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

/// Sequential, deterministic top-down binned SAH. Returns the beads in leaf
/// order, the nodes (root first) and the depth reached.
fn build(beads: Vec<Bead>) -> (Vec<Bead>, Vec<Node>, usize) {
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
    let mut nodes: Vec<Node> = Vec::with_capacity(2 * beads.len() / LEAF_MAX + 1);
    nodes.push(Node::default());
    let mut max_depth = 0usize;
    // (node, start, end, depth)
    let mut work: Vec<(u32, usize, usize, usize)> = vec![(0, 0, prims.len(), 0)];
    while let Some((ni, start, end, depth)) = work.pop() {
        max_depth = max_depth.max(depth);
        let mut bb = BinBox::EMPTY;
        let mut cb = BinBox::EMPTY;
        for p in &prims[start..end] {
            bb.grow(p.lo, p.hi, 1);
            cb.grow(p.c, p.c, 1);
        }
        let n = end - start;
        let make_leaf = |nodes: &mut Vec<Node>| {
            nodes[ni as usize] = Node {
                lo: bb.lo,
                hi: bb.hi,
                first: start as u32,
                count: n as u32,
            };
        };
        if n <= LEAF_MAX || depth + 1 >= MAX_DEPTH {
            make_leaf(&mut nodes);
            continue;
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
            for p in &prims[start..end] {
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
        let split = match best {
            // Traversal costs about as much as one bead test; below the
            // leaf's own cost (and while the leaf is small) stop here.
            Some((cost, axis, nl)) if cost + bb.area() < leaf_cost || n > 16 => {
                let ext = cb.hi[axis] - cb.lo[axis];
                let k = BINS as f32 / ext;
                let lo = cb.lo[axis];
                let slice = &mut prims[start..end];
                let mut i = 0usize;
                let mut j = slice.len();
                while i < j {
                    let b = (((slice[i].c[axis] - lo) * k) as usize).min(BINS - 1);
                    if b < nl {
                        i += 1;
                    } else {
                        j -= 1;
                        slice.swap(i, j);
                    }
                }
                Some(start + i)
            }
            Some(_) => None,
            // Every centroid in one place: halve the list.
            None if n > 16 => Some(start + n / 2),
            None => None,
        };
        let Some(mid) = split.filter(|&m| m > start && m < end) else {
            make_leaf(&mut nodes);
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
        work.push((left + 1, mid, end, depth + 1));
        work.push((left, start, mid, depth + 1));
    }
    let ordered: Vec<Bead> = prims.iter().map(|p| beads[p.i as usize]).collect();
    nodes.shrink_to_fit();
    (ordered, nodes, max_depth)
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

    #[test]
    fn the_tree_finds_what_brute_force_finds() {
        let mut s = 1u64;
        let mut beads = Vec::new();
        for _ in 0..3000 {
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
                material: 0,
            });
        }
        let set = BeadSet::new(
            beads.clone(),
            vec![BeadMaterial::new(Material::default())],
            2.0,
        );
        assert!(set.len() > beads.len(), "long beads are split");
        let mut hits = 0;
        for _ in 0..3000 {
            let o = [
                lcg(&mut s) * 60.0 - 10.0,
                lcg(&mut s) * 60.0 - 10.0,
                30.0 + lcg(&mut s) * 10.0,
            ];
            let tgt = [lcg(&mut s) * 40.0, lcg(&mut s) * 40.0, lcg(&mut s) * 10.0];
            let d = normalize(sub(tgt, o));
            let brute = beads
                .iter()
                .filter_map(|b| {
                    b.interval(o, d)
                        .map(|(t0, t1)| if t0 > 1e-4 { t0 } else { t1 })
                })
                .filter(|t| *t > 1e-4)
                .fold(f32::INFINITY, f32::min);
            let tree = set
                .intersect(o, d, 1e-4, f32::INFINITY)
                .map_or(f32::INFINITY, |h| h.t);
            if brute.is_finite() {
                hits += 1;
                assert!((brute - tree).abs() < 2e-3, "brute {brute} tree {tree}");
            } else {
                assert!(tree.is_infinite(), "tree found {tree} where nothing is");
            }
            assert_eq!(
                set.occluded_where(o, d, 1e-4, 1e9, |_| true),
                brute.is_finite()
            );
        }
        assert!(hits > 300, "the test rays should hit something: {hits}");
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
        set.intervals_where(
            [-5.0, 5.0, 0.0],
            [1.0, 0.0, 0.0],
            0.0,
            100.0,
            |_| true,
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
        let bad = Bead { a: [f32::NAN, 0.0, 0.0], ..bead([0.0; 3], [1.0; 3]) };
        let set = BeadSet::new(vec![bad, Bead { rx: 0.0, ..bead([0.0; 3], [1.0; 3]) }], vec![], 1.0);
        assert!(set.is_empty() && set.intersect([0.0; 3], [1.0, 0.0, 0.0], 0.0, 1.0).is_none());
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
            .intersect_where([1.0, 0.0, 0.0], [1.0, 0.0, 0.0], 1e-4, 100.0, |_| true)
            .unwrap();
        let p = [1.0 + h.t, 0.0, 0.0];
        assert!(set.inside_other(p, h.bead, |_| true));
        // The far end of the second bead is the union's real surface.
        let p = [10.25, 0.0, 0.0];
        let last = (0..2).find(|&i| set.bead(i).b[0] > 9.0).unwrap();
        assert!(!set.inside_other(p, last, |_| true));
    }
}
