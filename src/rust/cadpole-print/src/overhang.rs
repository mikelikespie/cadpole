//! Overhangs: what the nozzle would be asked to print over thin air.
//!
//! **Method.** Purely local, from the triangle normals and the build
//! direction — this is the same quantity a slicer's overhang perimeter logic
//! uses, and it is cheap enough to recompute on every orientation candidate.
//!
//! For a triangle with outward normal `n` and build direction `u` (up), let
//! `s = n · (−u)`. `s ≤ 0` is a wall or an upward face, angle 0. Otherwise the
//! **angle from vertical** is `asin(s)`: a vertical wall is 0°, a 45° slope is
//! 45°, and a flat ceiling is 90°. Past `profile.max_overhang_degrees`
//! (default 45°) the surface needs support.
//!
//! Faces lying *on the bed* are excluded: a flat bottom is a 90° "overhang"
//! that the build plate supports perfectly. A triangle counts as bed contact
//! when all three of its vertices are within a whisker of the lowest point of
//! the model along the build direction.
//!
//! **Bridges.** A flat downward face held up at both ends -- a window
//! or door head, a lintel, the roof of a cavity -- is a *bridge*: a slicer
//! lays straight extrusions across it from one support to the other and
//! prints it without support, up to a length. So a flat face (within
//! [`BRIDGE_FLAT_DEGREES`] of level) is checked for a span: its connected
//! flat region's boundary is split into edges that are **supported** -- the
//! surface across the edge goes *down* from it, a wall under it -- and edges
//! that are not (the surface goes up, or nothing is there); from each
//! triangle's centre a line is cast both ways in the directions across the
//! supported edges and along the plane's axes, and the shortest such line
//! that ends on a supported edge at both ends is the triangle's span. A span
//! no longer than `max_bridge` is a bridge and is not counted as an
//! overhang; a longer one still is, and the warning says it was a span
//! between supports that is too long rather than a face over air. The
//! default, [`DEFAULT_MAX_BRIDGE`], is the slicer setting this models.
//!
//! What this still does **not** do is a real supportability analysis: a
//! sloped downward face 2 mm above a wider face below it is printable in
//! practice and is counted here, and an orientation is scored without the
//! bridge allowance ([`unsupported_area`]). Slicers decide that from the
//! sliced layers; the number here is the honest upper bound.

use std::collections::{HashMap, HashSet};

use cadpole_core::geom::Vec3;
use cadpole_core::render::bvh::Bvh;
use cadpole_core::tessellate::{Mesh, Topology};

use crate::{
    Adjacency, Check, Issue, Profile, Severity, centroid, cluster, face_name, fmt_at, num, scale_of, tri, tri_normal_area,
};

/// How close to level a downward face has to be to be bridged: within this
/// many degrees of flat. A bridge is printed in one layer, and a face tilted
/// more than this rises by more than a layer across a span worth bridging.
pub const BRIDGE_FLAT_DEGREES: f64 = 2.0;

/// The longest span, in mm, a flat downward face held up at both ends may
/// bridge before it counts as an overhang: Bambu Studio's documented
/// "max bridge length" setting, at its default. A
/// profile-independent Cadpole number, like the 45° overhang threshold.
pub const DEFAULT_MAX_BRIDGE: f64 = 10.0;

/// How close to level a downward face held up on one side has to be to be
/// judged as a ledge ([`crate::SupportRules::max_cantilever`]): within this
/// many degrees of flat. Steeper faces are slopes, which the overhang angle
/// and area judge; a ledge is the near-level case, where every layer of it
/// is printed out over air at once.
pub const CANTILEVER_DEGREES: f64 = 70.0;

/// How far past the overhang angle a face has to be to count, in degrees:
/// a face drawn at exactly 45° (a teardrop's top, a chamfer) comes back
/// from f32 positions a few millionths of a degree either side of it.
pub const ANGLE_SLACK: f64 = 1e-3;

/// A downward face no more than this many layers above an upward face of
/// the same part -- the vertical gap of a print-in-place mechanism, a gear
/// standing two layers over its carrier -- is printed onto that face and
/// comes free of it: it rests, and needs no support. The two-layer gap is
/// what print-in-place parts are drawn with (the `layer_gap` rule of
/// [`crate::assembly`] holds such gaps to whole layers); a quarter layer
/// more absorbs the tessellation.
pub const RESTING_LAYERS: f64 = 2.0;

/// The gap, in mm, under which a face rests on the part below it.
pub fn resting_gap(profile: &Profile) -> f64 {
    profile.layer_height * (RESTING_LAYERS + 0.25)
}

/// Whether the point `p`, on a downward face, has an upward face of the same
/// mesh within `gap` straight below it.
pub(crate) fn rests_on(bvh: &Bvh, mesh: &Mesh, p: Vec3, up: Vec3, gap: f64, eps: f64) -> bool {
    let d = -up;
    let Some(h) = bvh.intersect([p.x as f32, p.y as f32, p.z as f32], [d.x as f32, d.y as f32, d.z as f32], eps as f32, gap as f32)
    else {
        return false;
    };
    let t = bvh.tri(h.tri).tri as usize;
    tri_normal_area(&tri(mesh, t)).0.dot(up) > 0.0
}

#[derive(Clone, Debug, Default)]
pub struct OverhangReport {
    /// **Unsupported** angle from vertical in degrees under each triangle, in
    /// `mesh.indices` order: 0 for vertical and upward faces *and for faces
    /// resting on the build plate*, 90 for a flat ceiling over air. This is
    /// the field the `overhang` analysis view colour-maps, and the question it
    /// answers is "what needs support", not "which way does this face point".
    pub per_triangle: Vec<f32>,
    /// Total area in mm² past the threshold and not resting on the bed.
    pub unsupported_area: f64,
    /// Area in mm² lying flat on the build plate.
    pub bed_contact_area: f64,
    /// Steepest overhang that is *not* resting on the bed, in degrees. A box
    /// standing on its base has 0: its underside is 90° from vertical, but the
    /// build plate is holding it up.
    pub max_degrees: f64,
    /// Representative unsupported places: `(position, degrees)`, steepest
    /// first.
    pub spots: Vec<(Vec3, f64)>,
    /// Area in mm² of flat downward faces held up at both ends within
    /// `max_bridge`: bridges, printed without support, and so not in
    /// `unsupported_area`.
    pub bridge_area: f64,
    /// The longest of those spans, in mm; 0 when there is no bridge.
    pub longest_bridge: f64,
    /// How many separate flat regions hold a bridge.
    pub bridges: usize,
    /// Area in mm² of flat faces held up at both ends but over a span longer
    /// than `max_bridge`, which stay in `unsupported_area`; and the shortest
    /// such span, in mm (0 when none).
    pub long_span_area: f64,
    pub shortest_long_span: f64,
    /// The `max_bridge` this was judged against, in mm (0 or less: off).
    pub max_bridge: f64,
    /// What still needs support once every allowance is taken: the area in
    /// mm² of the connected overhang patches larger than
    /// `max_overhang_area`, after bridges, short ledges and the tops of
    /// small horizontal holes are left out. 0 means the overhang rule is
    /// satisfied.
    pub support_area: f64,
    /// The largest connected overhang patch after those allowances, in mm²,
    /// whether or not it is over the limit.
    pub largest_patch: f64,
    /// Area in mm² of near-level ledges held up on one side and sticking out
    /// no further than `max_cantilever`: printed off their perimeters, and
    /// not counted as overhang.
    pub ledge_area: f64,
    /// How far the longest ledge held up on one side sticks out past what
    /// holds it, in mm; 0 when there is none.
    pub longest_cantilever: f64,
    /// Area in mm² of downward faces past the threshold resting on the part
    /// below across a print-in-place gap ([`RESTING_LAYERS`]): not in
    /// `unsupported_area`.
    pub resting_area: f64,
}

/// Check one mesh as if it were alone on the plate: the bed is its own lowest
/// point. Bridges up to [`DEFAULT_MAX_BRIDGE`].
pub fn check(mesh: &Mesh, profile: &Profile, issues: &mut Vec<Issue>) -> OverhangReport {
    check_bridged(mesh, profile, None, DEFAULT_MAX_BRIDGE, issues)
}

/// [`check`], but with the bed given explicitly — the height along the build
/// direction that counts as "resting on the plate".
///
/// The analysis view passes the *scene's* floor, so a part held up by another
/// part shows its underside as the overhang it is; `print_check` on one body
/// passes `None`, because a body checked on its own is a body printed on its
/// own.
pub fn check_at(mesh: &Mesh, profile: &Profile, bed_at: Option<f64>, issues: &mut Vec<Issue>) -> OverhangReport {
    check_bridged(mesh, profile, bed_at, DEFAULT_MAX_BRIDGE, issues)
}

/// [`check_at`] with the longest bridge given, in mm; 0 or less counts every
/// flat downward face past the threshold as needing support.
pub fn check_bridged(
    mesh: &Mesh,
    profile: &Profile,
    bed_at: Option<f64>,
    max_bridge: f64,
    issues: &mut Vec<Issue>,
) -> OverhangReport {
    let adj = Adjacency::new(mesh);
    check_support(mesh, &adj, None, profile, bed_at, max_bridge, &[], issues)
}

/// The whole overhang rule, as `print_check` runs it: the angle, bridges up
/// to `max_bridge`, ledges up to `profile.support.max_cantilever`, and the
/// largest connected patch against `profile.support.max_overhang_area`.
/// `skip` marks triangles another check has already judged -- the tops of
/// round horizontal holes ([`crate::support::horizontal_holes`]) -- which
/// count in `unsupported_area` but not in a patch. `topo` names the faces
/// in the findings.
#[allow(clippy::too_many_arguments)]
pub fn check_support(
    mesh: &Mesh,
    adj: &Adjacency,
    topo: Option<&Topology>,
    profile: &Profile,
    bed_at: Option<f64>,
    max_bridge: f64,
    skip: &[bool],
    issues: &mut Vec<Issue>,
) -> OverhangReport {
    let mut r = OverhangReport { per_triangle: vec![0.0; mesh.triangle_count()], max_bridge, ..Default::default() };
    if mesh.triangle_count() == 0 {
        return r;
    }
    let rules = profile.support;
    let up = profile.up();
    let scale = scale_of(mesh);
    let floor = bed_at.unwrap_or_else(|| lowest(mesh, up));
    let bed_eps = (scale * 1e-4).max(1e-6);
    let threshold = profile.max_overhang_degrees;
    let flat = (90.0 - BRIDGE_FLAT_DEGREES).max(threshold);

    // Every triangle past the threshold and off the bed, with its angle and
    // area; the flat ones are the bridge candidates.
    let mut over: Vec<(usize, f64, f64)> = Vec::new();
    let mut candidates: Vec<usize> = Vec::new();
    for t in 0..mesh.triangle_count() {
        let v = tri(mesh, t);
        let (n, area) = tri_normal_area(&v);
        if area <= 0.0 {
            continue;
        }
        let s = n.dot(-up).clamp(-1.0, 1.0);
        if s <= 0.0 {
            continue;
        }
        let deg = s.asin().to_degrees();
        if v.iter().all(|p| p.dot(up) - floor <= bed_eps) {
            // Resting on the plate: geometrically a 90 degree ceiling, but the
            // bed is holding it up, so the field reads 0 and the picture stays
            // calm. Everything downstream asks "what needs support".
            r.bed_contact_area += area;
            continue;
        }
        r.per_triangle[t] = deg as f32;
        if deg > threshold + ANGLE_SLACK {
            over.push((t, deg, area));
            if deg >= flat {
                candidates.push(t);
            }
        }
    }
    // Faces resting on the part below them across a print-in-place gap.
    let mut resting: HashSet<usize> = HashSet::new();
    if !over.is_empty() {
        let bvh = Bvh::from_mesh(mesh);
        let gap = resting_gap(profile);
        let eps = (scale * 1e-6).max(1e-5);
        over.retain(|&(t, _, area)| {
            if rests_on(&bvh, mesh, centroid(&tri(mesh, t)), up, gap, eps) {
                resting.insert(t);
                r.resting_area += area;
                r.per_triangle[t] = 0.0;
                false
            } else {
                true
            }
        });
        let kept: HashSet<usize> = over.iter().map(|o| o.0).collect();
        candidates.retain(|t| kept.contains(t));
    }
    let spans = if max_bridge > 0.0 && !candidates.is_empty() {
        bridge_spans(adj, up, &candidates, scale)
    } else {
        HashMap::new()
    };

    // What is left once the bridges are out: (triangle, degrees, area).
    let mut left: Vec<(usize, f64, f64)> = Vec::new();
    let mut bridged_regions: HashSet<usize> = HashSet::new();
    let mut long_at: Option<(Vec3, f64)> = None;
    for (t, deg, area) in over {
        match spans.get(&t) {
            Some(&(span, region)) if span <= max_bridge => {
                r.bridge_area += area;
                r.longest_bridge = r.longest_bridge.max(span);
                bridged_regions.insert(region);
                // Held up at both ends: nothing here needs support.
                r.per_triangle[t] = 0.0;
                continue;
            }
            Some(&(span, _)) => {
                r.long_span_area += area;
                if long_at.is_none_or(|(_, s)| span < s) {
                    long_at = Some((centroid(&tri(mesh, t)), span));
                }
            }
            None => {}
        }
        r.unsupported_area += area;
        if !skip.get(t).copied().unwrap_or(false) {
            left.push((t, deg, area));
        }
    }
    r.bridges = bridged_regions.len();
    r.shortest_long_span = long_at.map(|(_, s)| s).unwrap_or(0.0);
    // The steepest face that is not resting on anything, bridges left out.
    r.max_degrees = r.per_triangle.iter().fold(0.0f64, |m, d| m.max(*d as f64));

    // Ledges: near-level patches held up along one side. Within
    // `max_cantilever` they print off their perimeters; past it they are a
    // finding of their own.
    // A ledge within reach of what holds it is printed off it whatever
    // else is true of it -- including a strip along a wall that the span
    // cast above found "held up at both ends" by running along it. A ledge
    // too long that is held up at both ends is a span too long, which the
    // overhang finding says, and not a cantilever.
    let ledge: Vec<usize> =
        left.iter().filter(|(_, d, _)| *d >= CANTILEVER_DEGREES).map(|(t, _, _)| *t).collect();
    let mut exempt: HashSet<usize> = HashSet::new();
    let mut cantilevers: Vec<(Vec3, f64, usize)> = Vec::new();
    for reg in regions(adj, &ledge) {
        let spacing = (rules.max_cantilever / 4.0).max(scale * 1e-3);
        let Some((reach, at, t)) = cantilever_reach(adj, up, &reg, &resting, floor, bed_eps, scale, spacing) else { continue };
        r.longest_cantilever = r.longest_cantilever.max(reach);
        if reach <= rules.max_cantilever {
            for &t in &reg {
                exempt.insert(t);
                r.ledge_area += tri_normal_area(&tri(mesh, t)).1;
            }
        } else if !reg.iter().any(|t| spans.contains_key(t)) {
            cantilevers.push((at, reach, t));
        }
    }
    cantilevers.sort_by(|a, b| b.1.partial_cmp(&a.1).unwrap_or(std::cmp::Ordering::Equal));
    for (at, reach, t) in cantilevers.iter().take(4) {
        issues.push(Issue::new(
            Check::Cantilever,
            Severity::Warning,
            format!(
                "a ledge held up on one side only sticks out {} mm at {} on {}, past max_cantilever {} mm: its far \
                 edge is printed over air and droops; prop it with a 45° slope underneath, shorten it, or support it",
                num(*reach),
                fmt_at(*at),
                face_name(mesh, topo, *t),
                num(rules.max_cantilever)
            ),
            Some(*at),
            *reach,
        ));
    }

    // Patches: what is left, joined across shared edges. A patch no
    // larger than `max_overhang_area` prints on the perimeters round it.
    let rest: Vec<usize> = left.iter().map(|(t, _, _)| *t).filter(|t| !exempt.contains(t)).collect();
    let deg_of: HashMap<usize, f64> = left.iter().map(|(t, d, _)| (*t, *d)).collect();
    let mut spots = Vec::new();
    let mut largest: Option<(f64, usize)> = None;
    let mut big = 0usize;
    let mut small_area = 0.0;
    for reg in regions(adj, &rest) {
        let area: f64 = reg.iter().map(|&t| tri_normal_area(&tri(mesh, t)).1).sum();
        // The patch's steepest triangle is where it is.
        let steepest = *reg
            .iter()
            .max_by(|a, b| deg_of[a].partial_cmp(&deg_of[b]).unwrap_or(std::cmp::Ordering::Equal))
            .expect("a region has a triangle");
        if area > r.largest_patch {
            r.largest_patch = area;
            largest = Some((area, steepest));
        }
        if area > rules.max_overhang_area {
            r.support_area += area;
            big += 1;
            for &t in &reg {
                spots.push((centroid(&tri(mesh, t)), deg_of[&t]));
            }
        } else {
            small_area += area;
        }
    }

    r.spots = cluster(spots, 8, scale * 0.05, false);
    if r.support_area > 0.0 {
        let (p, d) = r.spots.first().copied().unwrap_or((Vec3::ZERO, r.max_degrees));
        let spans_note = match long_at {
            Some((at, span)) if max_bridge > 0.0 => format!(
                "; {} mm² of it is flat and held up at both ends but spans more than max_bridge {} mm (the shortest \
                 such span is {} mm, at {}), which a printer that bridges that far prints without support",
                num(r.long_span_area),
                num(max_bridge),
                num(span),
                fmt_at(at)
            ),
            _ => String::new(),
        };
        let (la, lt) = largest.expect("a patch over the limit is a patch");
        issues.push(Issue::new(
            Check::Overhang,
            Severity::Warning,
            format!(
                "{} mm² of overhang past {}° needs support; the steepest is {}° from vertical at {}; the largest \
                 connected patch is {} mm² on {} at {}{}, over max_overhang_area {} mm²{spans_note}",
                num(r.support_area),
                num(threshold),
                num(d),
                fmt_at(p),
                num(la),
                face_name(mesh, topo, lt),
                fmt_at(centroid(&tri(mesh, lt))),
                if big > 1 { format!(" (one of {big})") } else { String::new() },
                num(rules.max_overhang_area)
            ),
            Some(p),
            la,
        ));
    }
    if r.bridges > 0 {
        issues.push(Issue::new(
            Check::Bridged,
            Severity::Info,
            format!(
                "{} flat span{} held up at both ends, the longest {} mm, {} bridged without support (max_bridge {} \
                 mm) and not counted as overhang: {} mm²",
                r.bridges,
                if r.bridges == 1 { "" } else { "s" },
                num(r.longest_bridge),
                if r.bridges == 1 { "is" } else { "are" },
                num(max_bridge),
                num(r.bridge_area)
            ),
            None,
            r.longest_bridge,
        ));
    }
    if r.support_area == 0.0 && small_area + r.ledge_area > 0.0 {
        issues.push(Issue::new(
            Check::Overhang,
            Severity::Info,
            format!(
                "{} mm² past {}° prints without support: patches no larger than max_overhang_area {} mm² (the \
                 largest {} mm²), ledges sticking out no more than max_cantilever {} mm{}",
                num(small_area + r.ledge_area),
                num(threshold),
                num(rules.max_overhang_area),
                num(r.largest_patch),
                num(rules.max_cantilever),
                if r.longest_cantilever > 0.0 { format!(" (the longest {} mm)", num(r.longest_cantilever)) } else { String::new() }
            ),
            None,
            r.largest_patch,
        ));
    }
    r
}

/// The connected pieces of a set of triangles (indices into the input
/// mesh), joined across the edges they share once welded.
pub(crate) fn regions(adj: &Adjacency, tris: &[usize]) -> Vec<Vec<usize>> {
    let index: HashMap<usize, usize> = tris.iter().enumerate().map(|(i, &t)| (t, i)).collect();
    let mut parent: Vec<usize> = (0..tris.len()).collect();
    for (i, &t) in tris.iter().enumerate() {
        let Some(w) = adj.welded_of(t) else { continue };
        let wt = adj.wtri(w);
        for k in 0..3 {
            for &x in adj.across(wt[k], wt[(k + 1) % 3]) {
                if let Some(&j) = index.get(&(adj.origin[x] as usize)) {
                    let (a, b) = (find(&mut parent, i), find(&mut parent, j));
                    if a != b {
                        parent[a] = b;
                    }
                }
            }
        }
    }
    let mut out: HashMap<usize, Vec<usize>> = HashMap::new();
    for (i, &t) in tris.iter().enumerate() {
        let root = find(&mut parent, i);
        out.entry(root).or_default().push(t);
    }
    let mut v: Vec<Vec<usize>> = out.into_values().collect();
    v.sort_by_key(|r| r[0]);
    v
}

fn find(p: &mut [usize], mut x: usize) -> usize {
    while p[x] != x {
        p[x] = p[p[x]];
        x = p[x];
    }
    x
}

/// How far a ledge sticks out: the largest distance, across the plane
/// square to `up`, from any point of the region (sampled `spacing` apart
/// over each triangle) to its nearest **supported** boundary edge -- an edge where the surface across goes
/// down (the wall it stands on), or that lies on the bed. With the
/// position of that corner and a triangle that has it. `None` when no
/// edge of the region is held up: a ceiling over air is not a ledge.
#[allow(clippy::too_many_arguments)]
fn cantilever_reach(
    adj: &Adjacency,
    up: Vec3,
    region: &[usize],
    resting: &HashSet<usize>,
    floor: f64,
    bed_eps: f64,
    scale: f64,
    spacing: f64,
) -> Option<(f64, Vec3, usize)> {
    let eps = scale * 1e-7;
    let (e1, e2) = plane_axes(up);
    let flat2 = |p: Vec3| [p.dot(e1), p.dot(e2)];
    let in_region: HashSet<usize> = region.iter().filter_map(|&t| adj.welded_of(t)).collect();
    let mut held: Vec<([f64; 2], [f64; 2])> = Vec::new();
    for &w in &in_region {
        let t = adj.wtri(w);
        for k in 0..3 {
            let (a, b) = (t[k], t[(k + 1) % 3]);
            let on = adj.across(a, b);
            if on.iter().filter(|x| in_region.contains(x)).count() != 1 {
                continue;
            }
            let (pa, pb) = (adj.wpos(a), adj.wpos(b));
            let h = (pa.dot(up) + pb.dot(up)) / 2.0;
            let on_bed = h - floor <= bed_eps;
            let down = on.iter().filter(|x| !in_region.contains(x)).any(|&x| {
                resting.contains(&(adj.origin[x] as usize))
                    || adj.wtri(x).iter().any(|&v| v != a && v != b && adj.wpos(v).dot(up) < h - eps)
            });
            if on_bed || down {
                held.push((flat2(pa), flat2(pb)));
            }
        }
    }
    if held.is_empty() {
        return None;
    }
    let mut best: Option<(f64, Vec3, usize)> = None;
    let mut ws: Vec<usize> = in_region.iter().copied().collect();
    ws.sort_unstable();
    for w in ws {
        let t = adj.wtri(w);
        let [a, b, c] = [adj.wpos(t[0]), adj.wpos(t[1]), adj.wpos(t[2])];
        let longest = (b - a).length().max((c - b).length()).max((a - c).length());
        let k = ((longest / spacing).ceil() as usize).clamp(1, 24);
        for i in 0..=k {
            for j in 0..=(k - i) {
                let (u, v) = (i as f64 / k as f64, j as f64 / k as f64);
                let p = a + (b - a) * u + (c - a) * v;
                let q = flat2(p);
                let d = held.iter().map(|(x, y)| seg_dist(q, *x, *y)).fold(f64::INFINITY, f64::min);
                if best.as_ref().is_none_or(|(bd, _, _)| d > *bd) {
                    best = Some((d, p, adj.origin[w] as usize));
                }
            }
        }
    }
    best
}

fn seg_dist(p: [f64; 2], a: [f64; 2], b: [f64; 2]) -> f64 {
    let (dx, dy) = (b[0] - a[0], b[1] - a[1]);
    let l2 = dx * dx + dy * dy;
    let t = if l2 > 0.0 { (((p[0] - a[0]) * dx + (p[1] - a[1]) * dy) / l2).clamp(0.0, 1.0) } else { 0.0 };
    let (x, y) = (a[0] + t * dx - p[0], a[1] + t * dy - p[1]);
    (x * x + y * y).sqrt()
}

/// Two unit axes spanning the plane square to `up`.
pub(crate) fn plane_axes(up: Vec3) -> (Vec3, Vec3) {
    let a = if up.z.abs() < 0.9 { Vec3::Z } else { Vec3::X };
    let e1 = up.cross(a).normalized();
    let e2 = up.cross(e1).normalized();
    (e1, e2)
}

/// The span of every bridge candidate: for each flat downward triangle in
/// `candidates`, the shortest line through its centre, across its flat
/// region, that ends on a supported edge of the region at both ends -- and
/// the region it is in. A triangle with no such line is not in the map.
///
/// The mesh is welded so the regions and their boundary can be found by
/// shared edges. An edge of a region is supported when a triangle across it
/// reaches below it: the wall the bridge rests on. The lines are cast in
/// the plane square to `up`, across each supported edge's direction and
/// along the plane's two axes, so a rectangular head's span between its two
/// jambs is found exactly.
fn bridge_spans(adj: &Adjacency, up: Vec3, candidates: &[usize], scale: f64) -> HashMap<usize, (f64, usize)> {
    let regions = regions(adj, candidates);
    let (e1, e2) = plane_axes(up);
    let flat2 = |p: Vec3| [p.dot(e1), p.dot(e2)];
    let eps = scale * 1e-7;
    let mut out = HashMap::new();
    for (rid, region) in regions.iter().enumerate() {
        let members: Vec<usize> = region.iter().filter_map(|&t| adj.welded_of(t)).collect();
        let in_region: HashSet<usize> = members.iter().copied().collect();
        // Boundary edges, each with whether it is held up from below.
        let mut boundary: Vec<([f64; 2], [f64; 2], bool)> = Vec::new();
        for &w in &members {
            let t = adj.wtri(w);
            for k in 0..3 {
                let (a, b) = (t[k], t[(k + 1) % 3]);
                let on = adj.across(a, b);
                if on.iter().filter(|x| in_region.contains(x)).count() != 1 {
                    continue;
                }
                let (pa, pb) = (adj.wpos(a), adj.wpos(b));
                let h = (pa.dot(up) + pb.dot(up)) / 2.0;
                let supported = on.iter().filter(|x| !in_region.contains(x)).any(|&x| {
                    adj.wtri(x).iter().any(|&v| v != a && v != b && adj.wpos(v).dot(up) < h - eps)
                });
                boundary.push((flat2(pa), flat2(pb), supported));
            }
        }
        if !boundary.iter().any(|(_, _, s)| *s) {
            continue;
        }
        // Directions to cast in: across every supported edge, and the two
        // axes; a direction and its reverse are one.
        let mut dirs: Vec<[f64; 2]> = vec![[1.0, 0.0], [0.0, 1.0]];
        for (p, q, s) in &boundary {
            if !*s || dirs.len() >= 26 {
                continue;
            }
            let (dx, dy) = (q[0] - p[0], q[1] - p[1]);
            let l = (dx * dx + dy * dy).sqrt();
            if l <= eps {
                continue;
            }
            let d = [-dy / l, dx / l];
            if !dirs.iter().any(|e| (e[0] * d[0] + e[1] * d[1]).abs() > 0.9998) {
                dirs.push(d);
            }
        }
        for &w in &members {
            let t = adj.wtri(w);
            let c = flat2((adj.wpos(t[0]) + adj.wpos(t[1]) + adj.wpos(t[2])) * (1.0 / 3.0));
            let mut best = f64::INFINITY;
            for d in &dirs {
                let fwd = cast(c, *d, &boundary, eps);
                let back = cast(c, [-d[0], -d[1]], &boundary, eps);
                if let (Some((t1, true)), Some((t2, true))) = (fwd, back) {
                    best = best.min(t1 + t2);
                }
            }
            if best.is_finite() {
                out.insert(adj.origin[w] as usize, (best, rid));
            }
        }
    }
    out
}

/// The nearest boundary edge a ray from `c` along unit `d` meets: its
/// distance, and whether it is supported (at a vertex shared by two edges,
/// either one being supported is enough).
fn cast(c: [f64; 2], d: [f64; 2], boundary: &[([f64; 2], [f64; 2], bool)], eps: f64) -> Option<(f64, bool)> {
    let mut best: Option<(f64, bool)> = None;
    for (p, q, s) in boundary {
        let (ex, ey) = (q[0] - p[0], q[1] - p[1]);
        let den = d[0] * ey - d[1] * ex;
        if den.abs() <= 1e-15 {
            continue;
        }
        let (wx, wy) = (p[0] - c[0], p[1] - c[1]);
        let t = (wx * ey - wy * ex) / den;
        let u = (wx * d[1] - wy * d[0]) / den;
        if t <= eps || u < -1e-9 || u > 1.0 + 1e-9 {
            continue;
        }
        best = match best {
            Some((bt, bs)) if (t - bt).abs() <= eps.max(bt * 1e-9) => Some((bt.min(t), bs || *s)),
            Some((bt, _)) if bt < t => best,
            _ => Some((t, *s)),
        };
    }
    best
}

/// Height of the lowest point along the build direction.
pub fn lowest(mesh: &Mesh, up: Vec3) -> f64 {
    let mut lo = f64::INFINITY;
    for p in mesh.positions.chunks_exact(3) {
        let h = Vec3::new(p[0] as f64, p[1] as f64, p[2] as f64).dot(up);
        if h < lo {
            lo = h;
        }
    }
    lo
}

/// Unsupported area alone, without building a report — what
/// [`crate::orient`] scores candidate orientations with.
pub(crate) fn unsupported_area(mesh: &Mesh, up: Vec3, threshold_degrees: f64, bed_eps: f64) -> (f64, f64) {
    let floor = lowest(mesh, up);
    let (mut unsupported, mut contact) = (0.0, 0.0);
    for t in 0..mesh.triangle_count() {
        let v = tri(mesh, t);
        let (n, area) = tri_normal_area(&v);
        if area <= 0.0 {
            continue;
        }
        let s = n.dot(-up).clamp(-1.0, 1.0);
        if s <= 0.0 {
            continue;
        }
        if v.iter().all(|p| p.dot(up) - floor <= bed_eps) {
            contact += area;
            continue;
        }
        if s.asin().to_degrees() > threshold_degrees {
            unsupported += area;
        }
    }
    (unsupported, contact)
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::testing::*;

    fn run(m: &Mesh) -> (OverhangReport, Vec<Issue>) {
        let mut issues = Vec::new();
        let r = check(m, &Profile::default(), &mut issues);
        (r, issues)
    }

    #[test]
    fn a_sixty_degree_lean_is_flagged_with_its_area() {
        // 10 x 10 footprint, 10 tall, 10 deep, leaning 60 deg off vertical.
        // The overhanging face is 10 / cos(60) = 20 mm long and 10 deep.
        let (r, issues) = run(&leaning_prism(10.0, 10.0, 60.0, 10.0));
        assert!((r.max_degrees - 60.0).abs() < 1e-4, "max {}", r.max_degrees);
        assert!((r.unsupported_area - 200.0).abs() < 1e-4, "area {}", r.unsupported_area);
        assert_eq!(issues.len(), 1);
        let m = &issues[0].message;
        assert!(m.contains("200 mm²"), "{m}");
        assert!(m.contains("60° from vertical"), "{m}");
    }

    #[test]
    fn a_thirty_degree_lean_is_not_flagged() {
        let (r, issues) = run(&leaning_prism(10.0, 10.0, 30.0, 10.0));
        assert!((r.max_degrees - 30.0).abs() < 1e-4, "max {}", r.max_degrees);
        assert_eq!(r.unsupported_area, 0.0);
        assert!(issues.is_empty(), "{issues:?}");
    }

    #[test]
    fn the_threshold_is_where_it_says_it_is() {
        // 44 deg passes the 45 deg default, 46 deg does not.
        let (under, over) = (leaning_prism(10.0, 10.0, 44.0, 10.0), leaning_prism(10.0, 10.0, 46.0, 10.0));
        assert_eq!(run(&under).0.unsupported_area, 0.0);
        assert!(run(&over).0.unsupported_area > 0.0);
        // and the threshold moves: 40 deg flags the 44 deg lean.
        let p = Profile { max_overhang_degrees: 40.0, ..Profile::default() };
        assert!(check(&under, &p, &mut Vec::new()).unsupported_area > 0.0);
    }

    #[test]
    fn the_flat_bottom_is_bed_contact_not_an_overhang() {
        let (r, issues) = run(&box_mesh(10.0, 10.0, 10.0));
        assert!((r.bed_contact_area - 100.0).abs() < 1e-6, "contact {}", r.bed_contact_area);
        assert_eq!(r.unsupported_area, 0.0);
        assert_eq!(r.max_degrees, 0.0, "the underside is on the bed, not over air");
        assert!(issues.is_empty());
        // And nothing in the field either: the analysis view must not paint a
        // flat bottom red for resting on the plate.
        assert!(r.per_triangle.iter().all(|d| *d == 0.0), "{:?}", r.per_triangle);
    }

    #[test]
    fn a_ceiling_over_air_is_a_ninety_degree_overhang() {
        // A box lifted off the bed: its underside is unsupported.
        let m = box_between(Vec3::new(0.0, 0.0, 5.0), Vec3::new(10.0, 10.0, 15.0));
        let mut lifted = m;
        // Add a thin pillar so the lowest point is below the underside.
        let p = box_between(Vec3::new(4.0, 4.0, 0.0), Vec3::new(6.0, 6.0, 5.0));
        let base = (lifted.positions.len() / 3) as u32;
        lifted.positions.extend_from_slice(&p.positions);
        lifted.normals.extend_from_slice(&p.normals);
        lifted.indices.extend(p.indices.iter().map(|i| i + base));
        lifted.face_ids.extend(p.face_ids.iter().map(|f| f + 6));
        let (r, issues) = run(&lifted);
        assert!((r.max_degrees - 90.0).abs() < 1e-6);
        assert!((r.unsupported_area - 100.0).abs() < 1e-6, "the whole underside: {}", r.unsupported_area);
        assert!(issues[0].message.contains("90°"), "{}", issues[0].message);
    }

    #[test]
    fn a_body_held_up_by_another_overhangs_once_the_bed_is_the_scenes() {
        // A slab floating 5 mm up. Checked alone its underside is "on the bed";
        // checked against a bed at 0 it is a 100 mm2 ceiling over air.
        let slab = box_between(Vec3::new(0.0, 0.0, 5.0), Vec3::new(10.0, 10.0, 7.0));
        let alone = check(&slab, &Profile::default(), &mut Vec::new());
        assert_eq!(alone.unsupported_area, 0.0);
        assert!((alone.bed_contact_area - 100.0).abs() < 1e-6);
        let in_scene = check_at(&slab, &Profile::default(), Some(0.0), &mut Vec::new());
        assert!((in_scene.unsupported_area - 100.0).abs() < 1e-6, "{}", in_scene.unsupported_area);
        assert_eq!(in_scene.bed_contact_area, 0.0);
    }

    #[test]
    fn the_per_triangle_field_lines_up_with_the_mesh() {
        let m = leaning_prism(10.0, 10.0, 60.0, 10.0);
        let (r, _) = run(&m);
        assert_eq!(r.per_triangle.len(), m.triangle_count());
        let over: Vec<f32> = r.per_triangle.iter().copied().filter(|d| *d > 45.0).collect();
        // The two triangles of the leaning face, at 60 degrees. The flat
        // bottom is on the bed, so it is 0 here.
        assert_eq!(over.len(), 2, "{:?}", r.per_triangle);
        assert!(over.iter().all(|d| (*d - 60.0).abs() < 1e-3), "{over:?}");
    }

    // ---- bridges ---------------------------------------------------

    /// A wall 1 thick with a window `w` wide: two piers, a sill and a head.
    fn wall_with_window(w: f64) -> Mesh {
        joined(&[
            ([0.0, 0.0, 0.0], [5.0, 1.0, 10.0]),
            ([5.0 + w, 0.0, 0.0], [10.0 + w, 1.0, 10.0]),
            ([5.0, 0.0, 0.0], [5.0 + w, 1.0, 3.0]),
            ([5.0, 0.0, 8.0], [5.0 + w, 1.0, 10.0]),
        ])
    }

    #[test]
    fn a_window_head_held_up_by_its_jambs_is_a_bridge_not_an_overhang() {
        let m = wall_with_window(9.5);
        let mut issues = Vec::new();
        let r = check_bridged(&m, &Profile::default(), None, 10.0, &mut issues);
        assert_eq!(r.unsupported_area, 0.0, "{issues:?}");
        assert!((r.bridge_area - 9.5).abs() < 1e-6, "the head's underside, 9.5 x 1: {}", r.bridge_area);
        assert!((r.longest_bridge - 9.5).abs() < 1e-4, "the span between the jambs: {}", r.longest_bridge);
        assert_eq!(r.bridges, 1);
        assert!(!issues.iter().any(|i| i.check == Check::Overhang), "{issues:?}");
        let info = issues.iter().find(|i| i.check == Check::Bridged).expect("a bridged note");
        assert_eq!(info.severity, Severity::Info);
        assert!(info.message.contains("the longest 9.5 mm"), "{}", info.message);
        // The analysis field calls nothing under the head unsupported.
        assert!(r.per_triangle.iter().all(|d| *d == 0.0), "{:?}", r.per_triangle);
        // `check` and `check_at` bridge at the default, which is 10 mm.
        assert_eq!(check(&m, &Profile::default(), &mut Vec::new()).unsupported_area, 0.0);
    }

    #[test]
    fn a_span_longer_than_max_bridge_is_an_overhang_and_says_it_is_a_span() {
        let m = wall_with_window(9.5);
        let mut issues = Vec::new();
        let r = check_bridged(&m, &Profile::default(), None, 9.0, &mut issues);
        assert!((r.unsupported_area - 9.5).abs() < 1e-6, "{}", r.unsupported_area);
        assert_eq!(r.bridge_area, 0.0);
        assert!((r.long_span_area - 9.5).abs() < 1e-6);
        assert!((r.shortest_long_span - 9.5).abs() < 1e-4);
        let w = issues.iter().find(|i| i.check == Check::Overhang).expect("an overhang");
        assert!(w.message.contains("spans more than max_bridge 9 mm"), "{}", w.message);
        assert!(w.message.contains("the shortest such span is 9.5 mm"), "{}", w.message);
        // And no bridging at all when it is off.
        let off = check_bridged(&m, &Profile::default(), None, 0.0, &mut Vec::new());
        assert!((off.unsupported_area - 9.5).abs() < 1e-6);
        assert_eq!((off.bridge_area, off.long_span_area), (0.0, 0.0));
    }

    #[test]
    fn a_cantilever_held_up_on_one_side_only_is_not_a_bridge() {
        // A wall with a slab on top sticking out 4 to one side: the
        // underside rests on the wall along one edge and nowhere else.
        let m = joined(&[([0.0, 0.0, 0.0], [10.0, 1.0, 10.0]), ([0.0, 0.0, 10.0], [10.0, 5.0, 11.0])]);
        let mut issues = Vec::new();
        let r = check_bridged(&m, &Profile::default(), None, 100.0, &mut issues);
        assert!((r.unsupported_area - 40.0).abs() < 1e-6, "{}", r.unsupported_area);
        assert_eq!(r.bridge_area, 0.0);
        assert_eq!(r.long_span_area, 0.0, "not held up at both ends: no span at all");
    }

    #[test]
    fn a_slab_on_two_walls_bridges_across_the_short_way() {
        // Two parallel walls 8 apart with a roof slab across them, longer
        // (20) than it is wide: the span is the 8 between the walls.
        let m = joined(&[
            ([0.0, 0.0, 0.0], [20.0, 1.0, 6.0]),
            ([0.0, 9.0, 0.0], [20.0, 10.0, 6.0]),
            ([0.0, 0.0, 6.0], [20.0, 10.0, 7.0]),
        ]);
        let mut issues = Vec::new();
        let r = check_bridged(&m, &Profile::default(), None, 10.0, &mut issues);
        assert_eq!(r.unsupported_area, 0.0, "{issues:?}");
        assert!((r.bridge_area - 160.0).abs() < 1e-6, "{}", r.bridge_area);
        assert!((r.longest_bridge - 8.0).abs() < 1e-4, "{}", r.longest_bridge);
    }
}
