//! What moves as one: bodies joined into pieces.
//!
//! A list the script gave is one piece. Loose bodies are joined pairwise,
//! for each pair whose bounding boxes come within `joint_gap`:
//!
//! * **overlap**: they share material ([`clearance::Clearance::overlap`]);
//! * **skin**: they touch, and at least [`SKIN_FRACTION`] of the smaller
//!   one's surface lies on the other -- a stripe, a spot, an inlaid eye;
//! * **captive**: within `joint_gap` and neither can be slid away from the
//!   other in any of 26 directions -- a wheel on an axle with a hub cap, a
//!   planet between carrier plates. This is the non-directional blocking
//!   test of assembly planning, by rays: A is free to leave B along `d`
//!   when no ray from A's sample points along `d` meets B and no ray from
//!   B's along `-d` meets A.
//!
//! Anything else -- a lid resting on its base, which lifts off along +z --
//! stays apart. Vertex rays can miss an edge-on-edge collision, so the
//! captive test leans towards "free"; a script joins such a pair by giving
//! it as a list.

use cadpole_core::clearance::{self, Tree};
use cadpole_core::geom::Vec3;
use cadpole_core::render::bvh::Bvh;
use cadpole_core::tessellate::Mesh;

use super::{Item, Options, PieceKind, Pin, PlanBody};
use crate::{centroid, num, tri, tri_normal_area};

/// The share of the smaller body's surface that must lie on the other for
/// two touching bodies to be one piece.
pub const SKIN_FRACTION: f64 = 0.10;

/// Two surfaces closer than this, in mm, touch.
pub const TOUCH: f64 = 0.02;

/// Sample points per body for the captive test.
const CAPTIVE_SAMPLES: usize = 3000;

/// Triangles per body sampled for the contact share.
const CONTACT_SAMPLES: usize = 20000;

/// Why two bodies are one piece.
#[derive(Clone, Copy, Debug, PartialEq)]
pub enum JointKind {
    Overlap,
    /// The share of the smaller body's surface on the other.
    Skin(f64),
    /// The gap across which they are held, in mm.
    Captive(f64),
}

/// One joint between two bodies (indices into the plan's bodies).
#[derive(Clone, Copy, Debug, PartialEq)]
pub struct Joint {
    pub a: usize,
    pub b: usize,
    pub kind: JointKind,
}

impl Joint {
    pub fn describe(&self, bodies: &[PlanBody<'_>]) -> String {
        let (a, b) = (&bodies[self.a].name, &bodies[self.b].name);
        match self.kind {
            JointKind::Overlap => format!("{a} overlaps {b}"),
            JointKind::Skin(f) => format!("{a} and {b} touch over {}% of the smaller", (f * 100.0).round()),
            JointKind::Captive(g) => format!("{a} is captive in {b} across {} mm", num(g)),
        }
    }
}

/// A piece before it is oriented.
#[derive(Clone, Debug)]
pub struct Group {
    pub name: String,
    pub bodies: Vec<usize>,
    pub kind: PieceKind,
    pub joints: Vec<Joint>,
    pub pin: Pin,
}

fn find(p: &mut [usize], mut x: usize) -> usize {
    while p[x] != x {
        p[x] = p[p[x]];
        x = p[x];
    }
    x
}

/// Join `items` into pieces, in the order of their first item.
pub fn join(bodies: &[PlanBody<'_>], items: &[Item], opts: &Options, notes: &mut Vec<String>) -> Vec<Group> {
    // Which item each body belongs to; a body in two items stays in the first.
    let mut item_of: Vec<Option<usize>> = vec![None; bodies.len()];
    let mut members: Vec<Vec<usize>> = vec![Vec::new(); items.len()];
    for (i, it) in items.iter().enumerate() {
        for &b in &it.bodies {
            match item_of[b] {
                None => {
                    item_of[b] = Some(i);
                    members[i].push(b);
                }
                Some(_) => notes.push(format!("{} is given twice; it is placed once", bodies[b].name)),
            }
        }
    }
    let boxes: Vec<Option<(Vec3, Vec3)>> = bodies.iter().map(|b| b.mesh.bbox()).collect();
    let mut trees: Vec<Option<Tree>> = (0..bodies.len()).map(|_| None).collect();
    let mut parent: Vec<usize> = (0..items.len()).collect();
    let mut joints: Vec<Joint> = Vec::new();
    let reach = opts.joint_gap.max(TOUCH);
    for i in 0..bodies.len() {
        let Some(ii) = item_of[i] else { continue };
        let Some(bi) = boxes[i] else { continue };
        for j in i + 1..bodies.len() {
            let Some(ij) = item_of[j] else { continue };
            if ii == ij {
                continue;
            }
            let Some(bj) = boxes[j] else { continue };
            if clearance::box_distance(bi, bj) > reach {
                continue;
            }
            for k in [i, j] {
                if trees[k].is_none() {
                    trees[k] = Some(Tree::build(bodies[k].mesh));
                }
            }
            let Some(c) = clearance::closest(trees[i].as_ref().unwrap(), trees[j].as_ref().unwrap(), 0.0) else {
                continue;
            };
            let kind = if c.overlap {
                Some(JointKind::Overlap)
            } else if c.distance > reach {
                None
            } else {
                let skin = if c.distance <= TOUCH { skin_share(bodies[i].mesh, bodies[j].mesh) } else { 0.0 };
                if skin >= SKIN_FRACTION {
                    Some(JointKind::Skin(skin))
                } else if captive(bodies[i].mesh, bodies[j].mesh) {
                    Some(JointKind::Captive(c.distance))
                } else {
                    None
                }
            };
            if let Some(kind) = kind {
                joints.push(Joint { a: i, b: j, kind });
                let (ri, rj) = (find(&mut parent, ii), find(&mut parent, ij));
                if ri != rj {
                    let (lo, hi) = (ri.min(rj), ri.max(rj));
                    parent[hi] = lo;
                }
            }
        }
    }

    let mut out: Vec<Group> = Vec::new();
    let mut root_group: Vec<Option<usize>> = vec![None; items.len()];
    for (i, it) in items.iter().enumerate() {
        if members[i].is_empty() {
            continue;
        }
        let r = find(&mut parent, i);
        let g = match root_group[r] {
            Some(g) => g,
            None => {
                out.push(Group {
                    name: String::new(),
                    bodies: Vec::new(),
                    kind: PieceKind::Single,
                    joints: Vec::new(),
                    pin: Pin::default(),
                });
                root_group[r] = Some(out.len() - 1);
                out.len() - 1
            }
        };
        let grp = &mut out[g];
        let first = grp.bodies.is_empty();
        grp.bodies.extend_from_slice(&members[i]);
        merge_pin(&mut grp.pin, &it.pin, first);
        if it.declared && members[i].len() > 1 {
            grp.kind = PieceKind::Declared;
        }
    }
    for j in &joints {
        let r = find(&mut parent, item_of[j.a].unwrap());
        if let Some(g) = root_group[r] {
            out[g].joints.push(*j);
        }
    }
    for g in out.iter_mut() {
        g.bodies.sort_unstable();
        if g.joints.iter().any(|j| matches!(j.kind, JointKind::Captive(_))) {
            g.kind = PieceKind::PrintInPlace;
        } else if !g.joints.is_empty() {
            g.kind = PieceKind::Fused;
        } else if g.bodies.len() > 1 {
            g.kind = PieceKind::Declared;
        }
        g.name = match &g.pin.name {
            Some(n) => n.clone(),
            None if g.bodies.len() == 1 => bodies[g.bodies[0]].name.clone(),
            None => {
                let n = g.bodies.len() - 1;
                format!("{} and {} other{}", bodies[g.bodies[0]].name, n, if n == 1 { "" } else { "s" })
            }
        };
    }
    // Names are object names in the project: make them unique.
    let mut seen: Vec<String> = Vec::new();
    for g in out.iter_mut() {
        let base = g.name.clone();
        let mut k = 2;
        while seen.contains(&g.name) {
            g.name = format!("{base} #{k}");
            k += 1;
        }
        seen.push(g.name.clone());
    }
    out
}

/// The first item's pins win; a later item fills what it left open.
fn merge_pin(into: &mut Pin, from: &Pin, first: bool) {
    if first {
        *into = from.clone();
        return;
    }
    into.down = into.down.or(from.down);
    into.keep |= from.keep;
    into.along = into.along.or(from.along);
    into.spin = into.spin.or(from.spin);
    into.no_spin |= from.no_spin;
    if into.group.is_none() {
        into.group = from.group.clone();
    }
    into.coupon |= from.coupon;
    into.plate = into.plate.or(from.plate);
}

/// The share of the smaller body's surface that lies on the other: the area
/// of its triangles from whose centroid a ray a hair along the outward
/// normal meets the other body, over its whole area.
pub fn skin_share(a: &Mesh, b: &Mesh) -> f64 {
    let (small, other) = if a.area() <= b.area() { (a, b) } else { (b, a) };
    let total = small.area();
    if total <= 0.0 {
        return 0.0;
    }
    let bvh = Bvh::from_mesh(other);
    let n = small.triangle_count();
    let stride = n.div_ceil(CONTACT_SAMPLES).max(1);
    let eps = TOUCH;
    let (mut on, mut seen) = (0.0, 0.0);
    for t in (0..n).step_by(stride) {
        let v = tri(small, t);
        let (nrm, area) = tri_normal_area(&v);
        if area <= 0.0 {
            continue;
        }
        seen += area;
        let c = centroid(&v);
        let o = c - nrm * eps;
        if bvh.intersect(f3(o), f3(nrm), 0.0, (3.0 * eps) as f32).is_some() {
            on += area;
        }
    }
    if seen > 0.0 { on / seen } else { 0.0 }
}

/// The 26 directions of the cube's neighbours.
fn directions() -> Vec<Vec3> {
    let mut out = Vec::with_capacity(26);
    for x in -1..=1i32 {
        for y in -1..=1i32 {
            for z in -1..=1i32 {
                if x == 0 && y == 0 && z == 0 {
                    continue;
                }
                out.push(Vec3::new(x as f64, y as f64, z as f64).normalized());
            }
        }
    }
    out
}

fn samples(m: &Mesh) -> Vec<Vec3> {
    let mut pts = Vec::new();
    let nv = m.vertex_count();
    let nt = m.triangle_count();
    let total = nv + nt;
    let stride = total.div_ceil(CAPTIVE_SAMPLES).max(1);
    for i in (0..nv).step_by(stride) {
        let p = &m.positions[3 * i..3 * i + 3];
        pts.push(Vec3::new(p[0] as f64, p[1] as f64, p[2] as f64));
    }
    for t in (0..nt).step_by(stride) {
        pts.push(centroid(&tri(m, t)));
    }
    pts
}

fn f3(v: Vec3) -> [f32; 3] {
    [v.x as f32, v.y as f32, v.z as f32]
}

/// Whether neither body can be slid away from the other in any of 26
/// directions without one passing through the other.
pub fn captive(a: &Mesh, b: &Mesh) -> bool {
    let (Some((alo, ahi)), Some((blo, bhi))) = (a.bbox(), b.bbox()) else { return false };
    let reach = ((ahi.max(bhi) - alo.min(blo)).length() * 2.0) as f32;
    let (ba, bb) = (Bvh::from_mesh(a), Bvh::from_mesh(b));
    let (sa, sb) = (samples(a), samples(b));
    let eps = 1e-4f32;
    let blocked = |pts: &[Vec3], other: &Bvh, d: Vec3| pts.iter().any(|p| other.intersect(f3(*p), f3(d), eps, reach).is_some());
    for d in directions() {
        if !blocked(&sa, &bb, d) && !blocked(&sb, &ba, -d) {
            return false;
        }
    }
    true
}
