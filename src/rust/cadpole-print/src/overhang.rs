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
use cadpole_core::tessellate::Mesh;

use crate::{Check, Issue, Profile, Severity, centroid, cluster, fmt_at, num, scale_of, tri, tri_normal_area};

/// How close to level a downward face has to be to be bridged: within this
/// many degrees of flat. A bridge is printed in one layer, and a face tilted
/// more than this rises by more than a layer across a span worth bridging.
pub const BRIDGE_FLAT_DEGREES: f64 = 2.0;

/// The longest span, in mm, a flat downward face held up at both ends may
/// bridge before it counts as an overhang: Bambu Studio's documented
/// "max bridge length" setting, at its default. A
/// profile-independent Cadpole number, like the 45° overhang threshold.
pub const DEFAULT_MAX_BRIDGE: f64 = 10.0;

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
    let mut r = OverhangReport { per_triangle: vec![0.0; mesh.triangle_count()], max_bridge, ..Default::default() };
    if mesh.triangle_count() == 0 {
        return r;
    }
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
        if deg > threshold {
            over.push((t, deg, area));
            if deg >= flat {
                candidates.push(t);
            }
        }
    }
    let spans = if max_bridge > 0.0 && !candidates.is_empty() {
        bridge_spans(mesh, up, &candidates, scale)
    } else {
        HashMap::new()
    };

    let mut spots = Vec::new();
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
        spots.push((centroid(&tri(mesh, t)), deg));
    }
    r.bridges = bridged_regions.len();
    r.shortest_long_span = long_at.map(|(_, s)| s).unwrap_or(0.0);
    // The steepest face that is not resting on anything, bridges left out.
    r.max_degrees = r.per_triangle.iter().fold(0.0f64, |m, d| m.max(*d as f64));

    r.spots = cluster(spots, 8, scale * 0.05, false);
    if r.unsupported_area > 0.0 {
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
        issues.push(Issue::new(
            Check::Overhang,
            Severity::Warning,
            format!(
                "{} mm² of overhang past {}° needs support; the steepest is {}° from vertical at {}{spans_note}",
                num(r.unsupported_area),
                num(threshold),
                num(d),
                fmt_at(p)
            ),
            Some(p),
            r.unsupported_area,
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
    r
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
fn bridge_spans(mesh: &Mesh, up: Vec3, candidates: &[usize], scale: f64) -> HashMap<usize, (f64, usize)> {
    let (welded, origin) = mesh.welded_with_map(crate::WELD_EPS);
    let wpos = |v: u32| {
        let i = v as usize * 3;
        Vec3::new(welded.positions[i] as f64, welded.positions[i + 1] as f64, welded.positions[i + 2] as f64)
    };
    let wtri = |w: usize| [welded.indices[3 * w], welded.indices[3 * w + 1], welded.indices[3 * w + 2]];
    let mut welded_of: HashMap<usize, usize> = HashMap::with_capacity(origin.len());
    for (w, &t) in origin.iter().enumerate() {
        welded_of.insert(t as usize, w);
    }
    let cand: Vec<usize> = candidates.iter().filter_map(|t| welded_of.get(t).copied()).collect();
    let mut edges: HashMap<(u32, u32), Vec<usize>> = HashMap::with_capacity(welded.triangle_count() * 2);
    for w in 0..welded.triangle_count() {
        let t = wtri(w);
        for k in 0..3 {
            let (a, b) = (t[k], t[(k + 1) % 3]);
            edges.entry(if a < b { (a, b) } else { (b, a) }).or_default().push(w);
        }
    }
    // Regions: candidates joined across shared edges.
    let index: HashMap<usize, usize> = cand.iter().enumerate().map(|(i, &w)| (w, i)).collect();
    let mut parent: Vec<usize> = (0..cand.len()).collect();
    fn find(p: &mut [usize], mut x: usize) -> usize {
        while p[x] != x {
            p[x] = p[p[x]];
            x = p[x];
        }
        x
    }
    for tris in edges.values() {
        let cs: Vec<usize> = tris.iter().filter_map(|w| index.get(w).copied()).collect();
        for pair in cs.windows(2) {
            let (a, b) = (find(&mut parent, pair[0]), find(&mut parent, pair[1]));
            if a != b {
                parent[a] = b;
            }
        }
    }
    let mut regions: HashMap<usize, Vec<usize>> = HashMap::new();
    for (i, &w) in cand.iter().enumerate() {
        let root = find(&mut parent, i);
        regions.entry(root).or_default().push(w);
    }
    // The plane square to `up`.
    let e1 = {
        let a = if up.z.abs() < 0.9 { Vec3::Z } else { Vec3::X };
        up.cross(a).normalized()
    };
    let e2 = up.cross(e1).normalized();
    let flat2 = |p: Vec3| [p.dot(e1), p.dot(e2)];
    let eps = scale * 1e-7;
    let mut out = HashMap::new();
    let mut roots: Vec<usize> = regions.keys().copied().collect();
    roots.sort_unstable();
    for (rid, root) in roots.iter().enumerate() {
        let members = &regions[root];
        let in_region: HashSet<usize> = members.iter().copied().collect();
        // Boundary edges, each with whether it is held up from below.
        let mut boundary: Vec<([f64; 2], [f64; 2], bool)> = Vec::new();
        for &w in members {
            let t = wtri(w);
            for k in 0..3 {
                let (a, b) = (t[k], t[(k + 1) % 3]);
                let key = if a < b { (a, b) } else { (b, a) };
                let on = &edges[&key];
                if on.iter().filter(|x| in_region.contains(x)).count() != 1 {
                    continue;
                }
                let (pa, pb) = (wpos(a), wpos(b));
                let h = (pa.dot(up) + pb.dot(up)) / 2.0;
                let supported = on.iter().filter(|x| !in_region.contains(x)).any(|&x| {
                    wtri(x).iter().any(|&v| v != a && v != b && wpos(v).dot(up) < h - eps)
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
        for &w in members {
            let t = wtri(w);
            let c = flat2((wpos(t[0]) + wpos(t[1]) + wpos(t[2])) * (1.0 / 3.0));
            let mut best = f64::INFINITY;
            for d in &dirs {
                let fwd = cast(c, *d, &boundary, eps);
                let back = cast(c, [-d[0], -d[1]], &boundary, eps);
                if let (Some((t1, true)), Some((t2, true))) = (fwd, back) {
                    best = best.min(t1 + t2);
                }
            }
            if best.is_finite() {
                out.insert(origin[w] as usize, (best, rid));
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

    /// Boxes joined exactly (`cadpole_core::gridjoin`), as one closed mesh with
    /// shared edges -- the shape a story of a box-built model has.
    fn joined(boxes: &[([f64; 3], [f64; 3])]) -> Mesh {
        let gb: Vec<cadpole_core::gridjoin::GridBox> = boxes
            .iter()
            .map(|(lo, hi)| cadpole_core::gridjoin::GridBox { lo: *lo, hi: *hi, sides: [0, 1, 2, 3, 4, 5] })
            .collect();
        let g = cadpole_core::gridjoin::union_boxes(&gb, 1e-3, 6).expect("a union of boxes");
        let mut m = Mesh::default();
        for p in &g.positions {
            m.positions.extend_from_slice(&[p.x as f32, p.y as f32, p.z as f32]);
            m.normals.extend_from_slice(&[0.0, 0.0, 1.0]);
        }
        for t in &g.triangles {
            m.indices.extend_from_slice(t);
        }
        m.face_ids = g.provenance.clone();
        m
    }

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
