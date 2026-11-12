//! What else decides whether a part prints without support, beyond the
//! overhang angle ([`crate::overhang`]): four rules, each a finding with a
//! position and a face, each judged against a number on
//! [`crate::SupportRules`].
//!
//! | check | rule | severity |
//! | --- | --- | --- |
//! | `island` | a region whose lowest point is in mid-air -- a downward tip, a chin, a hanging finger, the underside of a ball off the bed -- deeper than `min_island` | warning |
//! | `horizontal_hole` | a round horizontal hole wider than `max_horizontal_hole` whose top is a round ceiling over air | warning |
//! | `slender` | a feature narrower than `slender_width` and taller than `max_slenderness` times its width | warning |
//! | `bed_contact` | a part taller than `max_height_to_base` times the square root of its first-layer area | warning |
//!
//! **Islands.** The overhang rule judges faces by their angle and their
//! area, and a tip that starts in mid-air can be a speck of either: the end
//! of a cone pointing down has almost no area past 45°, yet its first layer
//! is a dot printed on nothing. So the welded surface is swept bottom up,
//! the way water would fill it from below: vertices in order of height,
//! each joined to the neighbours already passed. A vertex with none is the
//! start of a new region, and a region that starts where the surface faces
//! down (the material is above it) and off the bed is a candidate. When it
//! meets a region that started lower, it is held up from there on, and the
//! height it rose before that is its depth. A candidate deeper than
//! `min_island` -- one layer by default, since a tip within one layer of
//! the rest prints as part of that layer -- is an island. A pocket's floor
//! or the bottom of a horizontal bore starts a region too, but faces up and
//! is left alone; a window head meets its jambs at its own height and has
//! no depth.
//!
//! **Round horizontal holes.** The top of a round bore lying on its side is
//! a ceiling, and past 6-8 mm across a slicer's bridge across it sags into
//! the hole. A face the kernel recognised as a cylinder with a horizontal
//! axis, concave (its normals point at the axis) and covering at least half
//! a turn is a hole; one whose faces still reach past the overhang angle at
//! the top has a round ceiling. Wider than `max_horizontal_hole`, that is a
//! finding naming the fix: a teardrop or diamond top, whose two 45° slopes
//! need no support. Narrower, its top is bridged, and the overhang rule
//! leaves it out. A bare mesh has no face classes and is not judged.
//!
//! **Slender features and the first layer**, from horizontal sections of
//! the welded surface: a section's loops are found by joining the segments
//! each triangle cuts across shared edges, and each loop's area is the
//! shoelace sum of its segments, oriented by the triangle's normal. A loop
//! narrower than `slender_width` (as the diameter of a circle of the same
//! area) is followed up through the sections while it overlaps the one
//! below; its height over that width past `max_slenderness` wobbles under
//! the nozzle. The section half a layer up is the first layer: a part
//! taller than `max_height_to_base` times the square root of its area (the
//! side of a square with that footprint) may tip or lift, and wants a brim
//! or a wider base.

use std::collections::HashMap;

use cadpole_core::geom::Vec3;
use cadpole_core::surface_class::SurfaceClass;
use cadpole_core::tessellate::{Mesh, Topology};

use crate::overhang::{lowest, plane_axes};
use crate::{
    Adjacency, Check, Issue, Profile, Severity, centroid, cluster, face_name, fmt_at, num, scale_of, tri,
    tri_normal_area,
};

/// A cylinder's axis within this many degrees of the plate is horizontal.
const HORIZONTAL_AXIS_DEGREES: f64 = 5.0;

/// The most sections the slender check cuts through a part; finer than a
/// layer only on a part under 300 layers tall.
const MAX_SECTIONS: usize = 300;

/// What [`check`] measured, beside its findings.
#[derive(Clone, Debug, Default)]
pub struct SupportReport {
    /// Islands found deeper than `min_island`, and the deepest, in mm.
    pub islands: usize,
    pub deepest_island: f64,
    /// Round horizontal holes found (a bare mesh has none), how many of them
    /// are wider than `max_horizontal_hole` with a round top, and the widest
    /// such, in mm across.
    pub horizontal_holes: usize,
    pub wide_holes: usize,
    pub widest_hole: f64,
    /// Slender features past `max_slenderness`, and the largest height over
    /// width among them (0 when none).
    pub slender: usize,
    pub slenderness: f64,
    /// The area of the section half a layer above the bed, in mm², and the
    /// part's height over its square root.
    pub first_layer_area: f64,
    pub height_to_base: f64,
}

/// The round horizontal holes [`horizontal_holes`] found.
#[derive(Clone, Debug, Default)]
pub struct Holes {
    pub count: usize,
    pub wide: usize,
    pub widest: f64,
}

/// Islands, slender features and the first layer, adding their findings
/// to `issues`. `holes` is what [`horizontal_holes`] found, carried into
/// the report.
pub fn check(
    mesh: &Mesh,
    adj: &Adjacency,
    topo: Option<&Topology>,
    profile: &Profile,
    holes: Holes,
    issues: &mut Vec<Issue>,
) -> SupportReport {
    let mut r = SupportReport {
        horizontal_holes: holes.count,
        wide_holes: holes.wide,
        widest_hole: holes.widest,
        ..Default::default()
    };
    if mesh.triangle_count() == 0 {
        return r;
    }
    islands(mesh, adj, topo, profile, &mut r, issues);
    sections(adj, profile, scale_of(mesh), &mut r, issues);
    r
}

// ---------------------------------------------------------------------------
// Islands
// ---------------------------------------------------------------------------

fn find(p: &mut [u32], mut x: u32) -> u32 {
    while p[x as usize] != x {
        p[x as usize] = p[p[x as usize] as usize];
        x = p[x as usize];
    }
    x
}

fn islands(
    mesh: &Mesh,
    adj: &Adjacency,
    topo: Option<&Topology>,
    profile: &Profile,
    r: &mut SupportReport,
    issues: &mut Vec<Issue>,
) {
    let w = &adj.welded;
    let n = w.positions.len() / 3;
    if n == 0 {
        return;
    }
    let up = profile.up();
    let scale = scale_of(mesh);
    let floor = lowest(mesh, up);
    let bed_eps = (scale * 1e-4).max(1e-6);
    let h: Vec<f64> = (0..n as u32).map(|v| adj.wpos(v).dot(up)).collect();
    let mut nbr: Vec<Vec<u32>> = vec![Vec::new(); n];
    let mut nsum = vec![Vec3::ZERO; n];
    // The welded triangle at each vertex that faces most nearly down: the
    // face an island is reported on.
    let mut low: Vec<(f64, usize)> = vec![(f64::NEG_INFINITY, usize::MAX); n];
    for t in 0..w.triangle_count() {
        let v = adj.wtri(t);
        let (nrm, area) = tri_normal_area(&[adj.wpos(v[0]), adj.wpos(v[1]), adj.wpos(v[2])]);
        let s = nrm.dot(-up);
        for k in 0..3 {
            let (a, b) = (v[k], v[(k + 1) % 3]);
            nbr[a as usize].push(b);
            nbr[b as usize].push(a);
            nsum[a as usize] = nsum[a as usize] + nrm * area;
            if area > 0.0 && s > low[a as usize].0 {
                low[a as usize] = (s, t);
            }
        }
    }
    let mut order: Vec<u32> = (0..n as u32).collect();
    order.sort_by(|a, b| h[*a as usize].partial_cmp(&h[*b as usize]).unwrap_or(std::cmp::Ordering::Equal).then(a.cmp(b)));
    let mut parent: Vec<u32> = (0..n as u32).collect();
    let mut done = vec![false; n];
    // Per region, kept on its root: where it started, whether it faces
    // down there, and whether it starts on the bed.
    let mut birth = vec![0u32; n];
    let mut down = vec![false; n];
    let mut grounded = vec![false; n];
    let min_depth = profile.support.min_island.max(bed_eps);
    let mut found: Vec<(Vec3, f64, usize, bool)> = Vec::new();
    for &v in &order {
        let vi = v as usize;
        let mut roots: Vec<u32> = nbr[vi].iter().filter(|u| done[**u as usize]).map(|&u| find(&mut parent, u)).collect();
        roots.sort_unstable();
        roots.dedup();
        done[vi] = true;
        if roots.is_empty() {
            birth[vi] = v;
            let l = nsum[vi].length();
            down[vi] = l > 0.0 && nsum[vi].dot(up) / l < -0.1;
            grounded[vi] = h[vi] - floor <= bed_eps;
            continue;
        }
        // The region that started lowest (on the bed first) holds the
        // others up from here on.
        let elder = *roots
            .iter()
            .min_by(|a, b| {
                let (a, b) = (**a as usize, **b as usize);
                grounded[b]
                    .cmp(&grounded[a])
                    .then(h[birth[a] as usize].partial_cmp(&h[birth[b] as usize]).unwrap_or(std::cmp::Ordering::Equal))
                    .then(a.cmp(&b))
            })
            .expect("roots is not empty");
        for &rt in &roots {
            if rt == elder {
                continue;
            }
            let ri = rt as usize;
            let depth = h[vi] - h[birth[ri] as usize];
            if down[ri] && !grounded[ri] && depth > min_depth {
                let b = birth[ri] as usize;
                found.push((adj.wpos(birth[ri]), depth, low[b].1, false));
            }
            parent[ri] = elder;
        }
        parent[vi] = elder;
    }
    // A region that never met a lower one is a shell of its own in the air.
    let mut top: HashMap<u32, f64> = HashMap::new();
    for v in 0..n as u32 {
        let rt = find(&mut parent, v);
        let e = top.entry(rt).or_insert(f64::NEG_INFINITY);
        *e = e.max(h[v as usize]);
    }
    let mut roots: Vec<(u32, f64)> = top.into_iter().collect();
    roots.sort_by_key(|(rt, _)| *rt);
    for (rt, hi) in roots {
        let ri = rt as usize;
        let depth = hi - h[birth[ri] as usize];
        if down[ri] && !grounded[ri] && depth > min_depth {
            let b = birth[ri] as usize;
            found.push((adj.wpos(birth[ri]), depth, low[b].1, true));
        }
    }
    if found.is_empty() {
        return;
    }
    // A tip over the part below it, across a print-in-place gap, rests on
    // it ([`crate::overhang::RESTING_LAYERS`]).
    let bvh = cadpole_core::render::bvh::Bvh::from_mesh(mesh);
    let gap = crate::overhang::resting_gap(profile);
    let eps = (scale * 1e-6).max(1e-5);
    found.retain(|f| !crate::overhang::rests_on(&bvh, mesh, f.0, up, gap, eps));
    if found.is_empty() {
        return;
    }
    r.islands = found.len();
    r.deepest_island = found.iter().map(|f| f.1).fold(0.0, f64::max);
    let spots = cluster(found.iter().map(|f| (f.0, f.1)).collect(), 8, scale * 0.01, false);
    for (at, depth) in spots {
        let Some(&(_, _, wt, shell)) = found.iter().find(|f| f.0 == at && f.1 == depth) else { continue };
        let face = if wt == usize::MAX { "face:0".to_owned() } else { face_name(mesh, topo, adj.origin[wt] as usize) };
        let message = if shell {
            format!(
                "a separate shell starts in mid-air at {} on {face}, {} mm tall with nothing of the part under it: \
                 its first layer prints on nothing; join it to the part or put it on the bed",
                fmt_at(at),
                num(depth)
            )
        } else {
            format!(
                "a region starts in mid-air at {} on {face}, {} mm below where it joins the rest of the part: its \
                 first layer prints on nothing; support it, slope it down at 45° to something under it, or turn the \
                 part over",
                fmt_at(at),
                num(depth)
            )
        };
        issues.push(Issue::new(Check::Island, Severity::Warning, message, Some(at), depth));
    }
}

// ---------------------------------------------------------------------------
// Round horizontal holes
// ---------------------------------------------------------------------------

/// Find the round horizontal holes of a body with a topology, report the
/// ones too wide to bridge, and mark (`true`, in `mesh.indices` order) the
/// triangles of every hole's round top: the overhang rule leaves them to
/// this one. Without a topology nothing is found and nothing is marked.
pub fn horizontal_holes(
    mesh: &Mesh,
    topo: Option<&Topology>,
    profile: &Profile,
    issues: &mut Vec<Issue>,
) -> (Holes, Vec<bool>) {
    let mut skip = vec![false; mesh.triangle_count()];
    let mut out = Holes::default();
    let Some(topo) = topo else { return (out, skip) };
    let up = profile.up();
    let sin_axis = HORIZONTAL_AXIS_DEGREES.to_radians().sin();
    let mut by_face: HashMap<u32, Vec<usize>> = HashMap::new();
    for t in 0..mesh.triangle_count() {
        if let Some(&f) = mesh.face_ids.get(t) {
            by_face.entry(f).or_default().push(t);
        }
    }
    // Concave horizontal cylinders: (axis point, unit axis, radius, triangles).
    let mut cyl: Vec<(Vec3, Vec3, f64, Vec<usize>)> = Vec::new();
    let mut ids: Vec<u32> = by_face.keys().copied().collect();
    ids.sort_unstable();
    for f in ids {
        let Some(SurfaceClass::Cylinder { axis_point, axis, radius }) = topo.faces.get(f as usize).map(|x| &x.surface)
        else {
            continue;
        };
        let axis = axis.normalized();
        if axis.length() < 0.5 || axis.dot(up).abs() > sin_axis || *radius <= 0.0 {
            continue;
        }
        let tris = &by_face[&f];
        let mut concave = 0.0;
        for &t in tris {
            let v = tri(mesh, t);
            let (n, area) = tri_normal_area(&v);
            let d = centroid(&v) - *axis_point;
            let radial = d - axis * d.dot(axis);
            concave += if n.dot(radial) < 0.0 { area } else { -area };
        }
        if concave <= 0.0 {
            continue;
        }
        // Two faces of one bore (a cylinder cut in halves) are one hole.
        let same = cyl.iter_mut().find(|(p, a, r, _)| {
            let d = *axis_point - *p;
            (r - radius).abs() <= 1e-3 * radius.max(1.0)
                && a.dot(axis).abs() > 0.9999
                && (d - *a * d.dot(*a)).length() <= 1e-3 * radius.max(1.0)
        });
        match same {
            Some(c) => c.3.extend_from_slice(tris),
            None => cyl.push((*axis_point, axis, *radius, tris.clone())),
        }
    }
    let floor = lowest(mesh, up);
    let bed_eps = (scale_of(mesh) * 1e-4).max(1e-6);
    let threshold = profile.max_overhang_degrees;
    let limit = profile.support.max_horizontal_hole;
    let mut wide: Vec<(Vec3, f64, Vec3, usize)> = Vec::new();
    for (p, axis, radius, tris) in &cyl {
        // How much of a turn the faces cover: a hole goes at least half way
        // round; a fillet in a corner does not.
        let u = (up - *axis * up.dot(*axis)).normalized();
        let wv = axis.cross(u);
        let mut bins = [false; 36];
        for &t in tris {
            let d = centroid(&tri(mesh, t)) - *p;
            let a = d.dot(wv).atan2(d.dot(u)).to_degrees().rem_euclid(360.0);
            bins[((a / 10.0) as usize).min(35)] = true;
        }
        if bins.iter().filter(|b| **b).count() < 18 {
            continue;
        }
        out.count += 1;
        // Its round top: the triangles still past the overhang angle.
        let mut top: Vec<(usize, f64)> = Vec::new();
        for &t in tris {
            let v = tri(mesh, t);
            let (n, area) = tri_normal_area(&v);
            if area <= 0.0 || v.iter().all(|q| q.dot(up) - floor <= bed_eps) {
                continue;
            }
            let s = n.dot(-up).clamp(-1.0, 1.0);
            if s > 0.0 && s.asin().to_degrees() > threshold + crate::overhang::ANGLE_SLACK {
                top.push((t, s));
            }
        }
        if top.is_empty() {
            continue;
        }
        for (t, _) in &top {
            skip[*t] = true;
        }
        let d = 2.0 * radius;
        if d > limit {
            let (t, _) = top.iter().copied().fold((top[0].0, f64::NEG_INFINITY), |b, x| if x.1 > b.1 { x } else { b });
            wide.push((centroid(&tri(mesh, t)), d, *axis, t));
        }
    }
    wide.sort_by(|a, b| b.1.partial_cmp(&a.1).unwrap_or(std::cmp::Ordering::Equal));
    out.wide = wide.len();
    out.widest = wide.first().map(|w| w.1).unwrap_or(0.0);
    for (at, d, axis, t) in wide.iter().take(8) {
        issues.push(Issue::new(
            Check::HorizontalHole,
            Severity::Warning,
            format!(
                "a round horizontal hole {} mm across (axis along {}) has a round ceiling over air at {} on {}: past \
                 max_horizontal_hole {} mm its top sags into the hole; give it a teardrop or diamond top (`teardrop()` \
                 in @cadpole/print.star cuts one) or print the part with the hole standing up",
                num(*d),
                fmt_at(*axis),
                fmt_at(*at),
                face_name(mesh, Some(topo), *t),
                num(limit)
            ),
            Some(*at),
            *d,
        ));
    }
    (out, skip)
}

// ---------------------------------------------------------------------------
// Sections: slender features and the first layer
// ---------------------------------------------------------------------------

/// One closed loop of a horizontal section: its signed area (positive
/// round material, negative round a hole), its extent in the plane's two
/// axes, and a point on it.
#[derive(Clone, Copy, Debug)]
struct Loop {
    area: f64,
    lo: [f64; 2],
    hi: [f64; 2],
    at: Vec3,
}

/// The loops of the sections at each height in `levels` (along `up`). A
/// level should not fall exactly on a vertex: the callers cut half a step
/// between them.
fn cut(adj: &Adjacency, up: Vec3, levels: &[f64]) -> Vec<Vec<Loop>> {
    let w = &adj.welded;
    let (e1, e2) = plane_axes(up);
    let origin = if w.positions.len() >= 3 { adj.wpos(0) } else { Vec3::ZERO };
    let mut segs: Vec<Vec<((u32, u32), (u32, u32), Vec3, Vec3)>> = vec![Vec::new(); levels.len()];
    let (Some(&first), Some(&last)) = (levels.first(), levels.last()) else { return Vec::new() };
    let step = if levels.len() > 1 { (last - first) / (levels.len() - 1) as f64 } else { 1.0 };
    for t in 0..w.triangle_count() {
        let v = adj.wtri(t);
        let p = [adj.wpos(v[0]), adj.wpos(v[1]), adj.wpos(v[2])];
        let hs = [p[0].dot(up), p[1].dot(up), p[2].dot(up)];
        let (lo, hi) = (hs[0].min(hs[1]).min(hs[2]), hs[0].max(hs[1]).max(hs[2]));
        if hi < first || lo > last {
            continue;
        }
        let (n, area) = tri_normal_area(&p);
        if area <= 0.0 {
            continue;
        }
        let tangent = up.cross(n);
        let k0 = if levels.len() > 1 { (((lo - first) / step).floor().max(0.0)) as usize } else { 0 };
        for (k, &l) in levels.iter().enumerate().skip(k0) {
            if l > hi {
                break;
            }
            if l < lo {
                continue;
            }
            let mut pts: Vec<((u32, u32), Vec3)> = Vec::with_capacity(2);
            for e in 0..3 {
                let (a, b) = (e, (e + 1) % 3);
                let (ha, hb) = (hs[a], hs[b]);
                if (ha < l) != (hb < l) {
                    let s = (l - ha) / (hb - ha);
                    let key = if v[a] < v[b] { (v[a], v[b]) } else { (v[b], v[a]) };
                    pts.push((key, p[a] + (p[b] - p[a]) * s));
                }
            }
            if pts.len() != 2 {
                continue;
            }
            let (mut a, mut b) = (pts[0], pts[1]);
            if (b.1 - a.1).dot(tangent) < 0.0 {
                std::mem::swap(&mut a, &mut b);
            }
            segs[k].push((a.0, b.0, a.1 - origin, b.1 - origin));
        }
    }
    let mut out = Vec::with_capacity(levels.len());
    for level in segs {
        let mut key_ix: HashMap<(u32, u32), usize> = HashMap::with_capacity(level.len());
        for (a, b, _, _) in &level {
            let n = key_ix.len();
            key_ix.entry(*a).or_insert(n);
            let n = key_ix.len();
            key_ix.entry(*b).or_insert(n);
        }
        let mut parent: Vec<u32> = (0..key_ix.len() as u32).collect();
        for (a, b, _, _) in &level {
            let (x, y) = (find(&mut parent, key_ix[a] as u32), find(&mut parent, key_ix[b] as u32));
            if x != y {
                parent[x as usize] = y;
            }
        }
        let mut loops: HashMap<u32, Loop> = HashMap::new();
        for (a, _, p, q) in &level {
            let root = find(&mut parent, key_ix[a] as u32);
            let (pp, qq) = ([p.dot(e1), p.dot(e2)], [q.dot(e1), q.dot(e2)]);
            let l = loops.entry(root).or_insert(Loop {
                area: 0.0,
                lo: [f64::INFINITY; 2],
                hi: [f64::NEG_INFINITY; 2],
                at: *p + origin,
            });
            l.area += 0.5 * up.dot(p.cross(*q));
            for i in 0..2 {
                l.lo[i] = l.lo[i].min(pp[i]).min(qq[i]);
                l.hi[i] = l.hi[i].max(pp[i]).max(qq[i]);
            }
        }
        let mut v: Vec<Loop> = loops.into_values().collect();
        v.sort_by(|a, b| b.area.partial_cmp(&a.area).unwrap_or(std::cmp::Ordering::Equal));
        out.push(v);
    }
    out
}

/// A slender run: consecutive sections of one narrow loop.
#[derive(Clone, Copy)]
struct Run {
    start: f64,
    end: f64,
    lo: [f64; 2],
    hi: [f64; 2],
    width: f64,
    top: Vec3,
}

fn sections(adj: &Adjacency, profile: &Profile, scale: f64, r: &mut SupportReport, issues: &mut Vec<Issue>) {
    let rules = profile.support;
    let up = profile.up();
    let w = &adj.welded;
    let (mut floor, mut ceil) = (f64::INFINITY, f64::NEG_INFINITY);
    for v in 0..(w.positions.len() / 3) as u32 {
        let h = adj.wpos(v).dot(up);
        floor = floor.min(h);
        ceil = ceil.max(h);
    }
    let height = ceil - floor;
    if !(height > 0.0) {
        return;
    }
    let layer = profile.layer_height.max(1e-3);

    // The first layer: the section half a layer up (or half way, on a part
    // thinner than a layer).
    let first = cut(adj, up, &[floor + (layer / 2.0).min(height / 2.0)]);
    let loops = first.first().cloned().unwrap_or_default();
    let area: f64 = loops.iter().map(|l| l.area).sum::<f64>().max(0.0);
    r.first_layer_area = area;
    r.height_to_base = if area > 0.0 { height / area.sqrt() } else { f64::INFINITY };
    if rules.max_height_to_base > 0.0 && r.height_to_base > rules.max_height_to_base {
        let at = loops.first().map(|l| l.at).unwrap_or(Vec3::ZERO);
        issues.push(Issue::new(
            Check::BedContact,
            Severity::Warning,
            format!(
                "the first layer is {} mm² under a part {} mm tall, {} times the side of a square with that \
                 footprint, past max_height_to_base {}: it may tip over or come off the plate; add a brim, widen \
                 the base, or lay the part down (the first layer is at {})",
                num(area),
                num(height),
                if r.height_to_base.is_finite() { num(r.height_to_base) } else { "infinitely many".to_owned() },
                num(rules.max_height_to_base),
                fmt_at(at)
            ),
            Some(at),
            if r.height_to_base.is_finite() { r.height_to_base } else { f64::MAX },
        ));
    }

    if rules.max_slenderness <= 0.0 || rules.slender_width <= 0.0 {
        return;
    }
    let step = layer.max(height / MAX_SECTIONS as f64);
    let count = ((height / step).floor() as usize).max(1);
    let levels: Vec<f64> = (0..count).map(|k| floor + (k as f64 + 0.5) * step).collect();
    let all = cut(adj, up, &levels);
    let width_of = |l: &Loop| (4.0 * l.area / std::f64::consts::PI).sqrt();
    let overlaps = |a: &Run, l: &Loop| {
        (0..2).all(|i| a.lo[i] <= l.hi[i] + step && l.lo[i] <= a.hi[i] + step)
    };
    let mut active: Vec<Run> = Vec::new();
    let mut done: Vec<Run> = Vec::new();
    for (k, loops) in all.iter().enumerate() {
        let lv = levels[k];
        let mut next: Vec<Run> = Vec::new();
        let mut used = vec![false; active.len()];
        for l in loops.iter().filter(|l| l.area > 0.0 && width_of(l) < rules.slender_width) {
            let d = width_of(l);
            let run = match active.iter().position(|a| overlaps(a, l)) {
                Some(i) => {
                    used[i] = true;
                    let a = active[i];
                    Run { end: lv + step / 2.0, lo: l.lo, hi: l.hi, width: a.width.max(d), top: l.at, ..a }
                }
                None => Run { start: lv - step / 2.0, end: lv + step / 2.0, lo: l.lo, hi: l.hi, width: d, top: l.at },
            };
            next.push(run);
        }
        for (i, a) in active.iter().enumerate() {
            if !used[i] {
                done.push(*a);
            }
        }
        active = next;
    }
    done.extend(active);
    let mut spots: Vec<(Vec3, f64)> = Vec::new();
    let mut what: Vec<(Vec3, f64, f64, f64)> = Vec::new();
    for run in done {
        let tall = run.end - run.start;
        if run.width <= 0.0 {
            continue;
        }
        let ratio = tall / run.width;
        if ratio > rules.max_slenderness {
            spots.push((run.top, ratio));
            what.push((run.top, ratio, tall, run.width));
        }
    }
    if spots.is_empty() {
        return;
    }
    r.slender = spots.len();
    r.slenderness = spots.iter().map(|s| s.1).fold(0.0, f64::max);
    for (at, ratio) in cluster(spots, 8, scale * 0.02, false) {
        let Some(&(_, _, tall, width)) = what.iter().find(|w| w.0 == at && w.1 == ratio) else { continue };
        issues.push(Issue::new(
            Check::Slender,
            Severity::Warning,
            format!(
                "a feature {} mm wide stands {} mm tall, {} times its width, past max_slenderness {} for features \
                 under slender_width {} mm: it wobbles under the nozzle and may snap; thicken it, brace it, or lay \
                 it down (its top is at {})",
                num(width),
                num(tall),
                num(ratio),
                num(rules.max_slenderness),
                num(rules.slender_width),
                fmt_at(at)
            ),
            Some(at),
            ratio,
        ));
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::testing::*;
    use crate::{SupportRules, check, check_named};

    fn of(r: &crate::Report, c: Check) -> Vec<&Issue> {
        r.issues.iter().filter(|i| i.check == c).collect()
    }

    // ---- islands ---------------------------------------------------

    #[test]
    fn a_finger_hanging_from_an_arm_is_an_island_with_its_depth() {
        // A post, an arm out from its top, and a finger hanging 6 mm down
        // from the arm's far end: the finger's tip starts in mid-air.
        let m = joined(&[
            ([0.0, 0.0, 0.0], [3.0, 3.0, 20.0]),
            ([3.0, 0.0, 17.0], [15.0, 3.0, 20.0]),
            ([12.0, 0.0, 11.0], [15.0, 3.0, 17.0]),
        ]);
        let r = check(&m, &Profile::default());
        assert_eq!(r.support.islands, 1, "{:?}", r.issues);
        assert!((r.support.deepest_island - 6.0).abs() < 1e-4, "{}", r.support.deepest_island);
        let i = of(&r, Check::Island);
        assert_eq!(i.len(), 1, "{:?}", r.issues);
        let at = i[0].at.unwrap();
        assert!((at.z - 11.0).abs() < 1e-4 && at.x >= 12.0 - 1e-6, "at the tip: {at:?}");
        assert!(i[0].message.contains("6 mm below"), "{}", i[0].message);
        // And the overhang under the arm is a finding too, but the tip's
        // own 9 mm² of underside would not have told the two apart.
        assert!(r.support_findings().count() >= 2);
    }

    #[test]
    fn a_ball_on_a_stalk_is_held_up_and_a_ball_beside_it_is_not() {
        // A ball on a stalk is no island: its lowest point is the stalk.
        let lollipop = revolve(&lollipop_profile(10.0, 1.5, 15.0), 48);
        let r = check(&lollipop, &Profile::default());
        assert_eq!(r.support.islands, 0, "{:?}", of(&r, Check::Island));
        // A ball floating beside a block, in the same body: its bottom is.
        let mut m = box_mesh(10.0, 10.0, 10.0);
        let ball = translated(&revolve(&ball_profile(4.0, 24), 32), Vec3::new(20.0, 5.0, 8.0));
        append(&mut m, &ball);
        let r = check(&m, &Profile::default());
        let i = of(&r, Check::Island);
        assert_eq!(i.len(), 1, "{:?}", r.issues);
        assert!(i[0].message.contains("separate shell"), "{}", i[0].message);
        let at = i[0].at.unwrap();
        assert!((at.z - 8.0).abs() < 1e-3 && (at.x - 20.0).abs() < 1e-3, "{at:?}");
        assert!((i[0].value - 8.0).abs() < 1e-3, "the ball's height: {}", i[0].value);
    }

    #[test]
    fn a_pocket_floor_and_a_window_head_are_not_islands() {
        // A box with a pocket in its top: the pocket's floor is a low point
        // of the surface, but it faces up.
        let pocket = joined(&[
            ([0.0, 0.0, 0.0], [20.0, 20.0, 5.0]),
            ([0.0, 0.0, 5.0], [5.0, 20.0, 10.0]),
            ([15.0, 0.0, 5.0], [20.0, 20.0, 10.0]),
            ([5.0, 0.0, 5.0], [15.0, 5.0, 10.0]),
            ([5.0, 15.0, 5.0], [15.0, 20.0, 10.0]),
        ]);
        assert_eq!(check(&pocket, &Profile::default()).support.islands, 0);
        // A window head meets its jambs at its own height.
        let wall = joined(&[
            ([0.0, 0.0, 0.0], [5.0, 1.0, 10.0]),
            ([13.0, 0.0, 0.0], [18.0, 1.0, 10.0]),
            ([5.0, 0.0, 0.0], [13.0, 1.0, 3.0]),
            ([5.0, 0.0, 8.0], [13.0, 1.0, 10.0]),
        ]);
        let r = check(&wall, &Profile::default());
        assert_eq!(r.support.islands, 0, "{:?}", r.issues);
        assert_eq!(r.support_findings().count(), 0, "{:?}", r.issues);
    }

    #[test]
    fn a_part_two_layers_over_another_rests_on_it() {
        // A plate and, in the same mesh, a disc standing 0.4 mm (two 0.2 mm
        // layers) over it: a print-in-place gap. Its underside is neither an
        // island nor an overhang. At 1 mm it is both.
        let plate = box_mesh(30.0, 30.0, 2.0);
        for (gap, resting) in [(0.4, true), (1.0, false)] {
            let mut m = plate.clone();
            append(&mut m, &box_between(Vec3::new(5.0, 5.0, 2.0 + gap), Vec3::new(25.0, 25.0, 6.0)));
            let r = check(&m, &Profile::default());
            assert_eq!(r.support_findings().next().is_none(), resting, "gap {gap}: {:?}", r.issues);
            if resting {
                assert!((r.overhang.resting_area - 400.0).abs() < 1e-6, "{}", r.overhang.resting_area);
                assert_eq!(r.overhang.unsupported_area, 0.0);
            } else {
                assert_eq!(r.support.islands, 1, "{:?}", r.issues);
            }
        }
    }

    #[test]
    fn a_tip_shallower_than_min_island_is_not_an_island() {
        // The finger hangs 0.15 mm: within one 0.2 mm layer of the arm.
        let m = joined(&[
            ([0.0, 0.0, 0.0], [3.0, 3.0, 20.0]),
            ([3.0, 0.0, 17.0], [5.0, 3.0, 20.0]),
            ([4.0, 0.0, 16.85], [5.0, 3.0, 17.0]),
        ]);
        assert_eq!(check(&m, &Profile::default()).support.islands, 0);
        let p = Profile { support: SupportRules { min_island: 0.1, ..SupportRules::default() }, ..Profile::default() };
        assert_eq!(check(&m, &p).support.islands, 1);
    }

    // ---- ledges and patches ----------------------------------------

    #[test]
    fn a_t_shelf_sticking_out_past_max_cantilever_is_a_cantilever() {
        // A wall 2 thick with a shelf on top reaching 6 out to one side.
        let m = joined(&[([0.0, 0.0, 0.0], [10.0, 2.0, 10.0]), ([0.0, 0.0, 10.0], [10.0, 8.0, 11.0])]);
        let r = check(&m, &Profile::default());
        let c = of(&r, Check::Cantilever);
        assert_eq!(c.len(), 1, "{:?}", r.issues);
        assert!((c[0].value - 6.0).abs() < 1e-4, "{}", c[0].value);
        assert!(c[0].message.contains("sticks out 6 mm"), "{}", c[0].message);
        assert!((r.overhang.longest_cantilever - 6.0).abs() < 1e-4);
        // The same shelf reaching 2 out prints off its perimeters.
        let m = joined(&[([0.0, 0.0, 0.0], [10.0, 2.0, 10.0]), ([0.0, 0.0, 10.0], [10.0, 4.0, 11.0])]);
        let r = check(&m, &Profile::default());
        assert!(of(&r, Check::Cantilever).is_empty(), "{:?}", r.issues);
        assert!(r.support_findings().next().is_none(), "{:?}", r.issues);
        assert!((r.overhang.ledge_area - 20.0).abs() < 1e-6, "{}", r.overhang.ledge_area);
        assert!((r.overhang.unsupported_area - 20.0).abs() < 1e-6, "still counted: {}", r.overhang.unsupported_area);
        // And with max_cantilever=0 the short ledge is judged too.
        let p = Profile { support: SupportRules { max_cantilever: 0.0, ..SupportRules::default() }, ..Profile::default() };
        assert_eq!(of(&check(&m, &p), Check::Cantilever).len(), 1);
    }

    #[test]
    fn a_patch_no_larger_than_max_overhang_area_prints_and_a_larger_one_does_not() {
        // A 60 deg lean 1 mm tall and 1.5 deep: 3 mm2 of overhang.
        let small = leaning_prism(10.0, 1.0, 60.0, 1.5);
        let r = check(&small, &Profile::default());
        assert!((r.overhang.unsupported_area - 3.0).abs() < 1e-6);
        assert!(of(&r, Check::Overhang).iter().all(|i| i.severity == Severity::Info), "{:?}", r.issues);
        // 10 mm deep, 20 mm2: over.
        let big = leaning_prism(10.0, 1.0, 60.0, 10.0);
        let r = check(&big, &Profile::default());
        let o: Vec<_> = of(&r, Check::Overhang).into_iter().filter(|i| i.severity == Severity::Warning).collect();
        assert_eq!(o.len(), 1, "{:?}", r.issues);
        assert!(o[0].message.contains("largest connected patch is 20 mm²"), "{}", o[0].message);
        assert!((r.overhang.largest_patch - 20.0).abs() < 1e-6);
        assert!((r.overhang.support_area - 20.0).abs() < 1e-6);
    }

    // ---- horizontal holes ------------------------------------------

    #[test]
    fn a_ten_mm_horizontal_bore_wants_a_teardrop_and_a_teardrop_does_not() {
        let (m, topo) = block_with_bore(20.0, 10.0, 10.0, 64, false);
        let r = check_named(&m, Some(&topo), &Profile::default(), 10.0);
        let h = of(&r, Check::HorizontalHole);
        assert_eq!(h.len(), 1, "{:?}", r.issues);
        assert!(h[0].message.contains("10 mm across"), "{}", h[0].message);
        assert!(h[0].message.contains("teardrop"), "{}", h[0].message);
        assert!(h[0].message.contains("block/bore"), "{}", h[0].message);
        assert_eq!((r.support.horizontal_holes, r.support.wide_holes), (1, 1));
        // Not counted twice: the bore's top is the hole's finding, not an
        // overhang patch.
        assert!(of(&r, Check::Overhang).iter().all(|i| i.severity == Severity::Info), "{:?}", r.issues);

        let (m, topo) = block_with_bore(20.0, 10.0, 10.0, 64, true);
        let r = check_named(&m, Some(&topo), &Profile::default(), 10.0);
        assert_eq!(r.support.horizontal_holes, 1);
        assert!(of(&r, Check::HorizontalHole).is_empty(), "{:?}", r.issues);
        assert!(r.support_findings().next().is_none(), "{:?}", r.issues);
    }

    #[test]
    fn a_small_horizontal_bore_is_bridged() {
        let (m, topo) = block_with_bore(20.0, 10.0, 5.0, 64, false);
        let r = check_named(&m, Some(&topo), &Profile::default(), 10.0);
        assert!(r.support_findings().next().is_none(), "{:?}", r.issues);
        // Without the faces' classes the bore's top is an overhang like any.
        let r = check(&m, &Profile::default());
        assert!(of(&r, Check::Overhang).iter().any(|i| i.severity == Severity::Warning), "{:?}", r.issues);
    }

    // ---- slender features and the first layer ----------------------

    #[test]
    fn a_needle_on_a_plate_is_slender_and_a_post_is_not() {
        let needle = joined(&[([0.0, 0.0, 0.0], [20.0, 20.0, 2.0]), ([9.0, 9.0, 2.0], [11.0, 11.0, 32.0])]);
        let r = check(&needle, &Profile::default());
        let s = of(&r, Check::Slender);
        assert_eq!(s.len(), 1, "{:?}", r.issues);
        // 2 x 2 is 2.26 mm as a circle; 30 mm tall.
        assert!((s[0].value - 30.0 / (16.0f64 / std::f64::consts::PI).sqrt()).abs() < 0.3, "{}", s[0].value);
        assert_eq!(s[0].severity, Severity::Warning);
        let post = joined(&[([0.0, 0.0, 0.0], [20.0, 20.0, 2.0]), ([7.0, 7.0, 2.0], [13.0, 13.0, 32.0])]);
        assert!(of(&check(&post, &Profile::default()), Check::Slender).is_empty());
        let short = joined(&[([0.0, 0.0, 0.0], [20.0, 20.0, 2.0]), ([9.0, 9.0, 2.0], [11.0, 11.0, 12.0])]);
        assert!(of(&check(&short, &Profile::default()), Check::Slender).is_empty());
    }

    #[test]
    fn a_tall_pin_on_its_own_footprint_may_tip_and_a_cube_does_not() {
        let pin = box_mesh(4.0, 4.0, 40.0);
        let r = check(&pin, &Profile::default());
        assert!((r.support.first_layer_area - 16.0).abs() < 1e-6, "{}", r.support.first_layer_area);
        assert!((r.support.height_to_base - 10.0).abs() < 1e-6);
        let b = of(&r, Check::BedContact);
        assert_eq!(b.len(), 1, "{:?}", r.issues);
        assert_eq!(b[0].severity, Severity::Warning);
        assert!(!b[0].check.is_support(), "a tipping risk is not a support finding");
        let cube = box_mesh(20.0, 20.0, 20.0);
        let r = check(&cube, &Profile::default());
        assert!(of(&r, Check::BedContact).is_empty());
        assert!((r.support.height_to_base - 1.0).abs() < 1e-9);
        // A ball resting on the plate touches it at a point.
        let ball = revolve(&ball_profile(10.0, 48), 48);
        assert_eq!(of(&check(&ball, &Profile::default()), Check::BedContact).len(), 1);
    }

    #[test]
    fn the_sections_measure_areas_and_holes() {
        // A 20 x 20 plate with a 10 x 10 hole through it: 300 mm2 a layer.
        let frame = joined(&[
            ([0.0, 0.0, 0.0], [20.0, 5.0, 4.0]),
            ([0.0, 15.0, 0.0], [20.0, 20.0, 4.0]),
            ([0.0, 5.0, 0.0], [5.0, 15.0, 4.0]),
            ([15.0, 5.0, 0.0], [20.0, 15.0, 4.0]),
        ]);
        let r = check(&frame, &Profile::default());
        assert!((r.support.first_layer_area - 300.0).abs() < 1e-6, "{}", r.support.first_layer_area);
    }
}
