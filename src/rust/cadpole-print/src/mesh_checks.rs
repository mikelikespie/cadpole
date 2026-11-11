//! Is the mesh a solid at all?
//!
//! A slicer needs a closed, consistently oriented, non-self-intersecting
//! surface: anything else makes "inside" ambiguous and the slice of a layer
//! stops being a set of closed loops. The measuring is
//! [`cadpole_core::solid::validate`], the one definition of "a solid" the mesh
//! kernel, the exporters and the example suite share; this module turns its
//! report into findings that carry the position to look at, so the viewport
//! can fly there and the assistant can say *where*.
//!
//! Everything runs on the **welded** mesh (vertices merged within
//! [`crate::WELD_EPS`]). A tessellator emits each surface patch with its own
//! vertices, so an unwelded mesh has a boundary edge along every face seam and
//! would report a hole per face.

use std::collections::HashMap;

use cadpole_core::geom::Vec3;
use cadpole_core::solid::{self, SolidOptions, SolidReport};
use cadpole_core::tessellate::Mesh;

use crate::{Check, Issue, Severity, cluster, fmt_at, scale_of};

/// A boundary loop: the rim of a hole in the surface.
#[derive(Clone, Debug)]
pub struct Hole {
    /// Average of the rim vertices — "the hole is around here".
    pub center: Vec3,
    /// Rim length in mm.
    pub perimeter: f64,
    /// Edges in the rim (3 for a single missing triangle).
    pub edges: usize,
    /// Bounding box of the rim, so a viewer can frame it.
    pub bbox: (Vec3, Vec3),
}

/// What the topology checks found. The positions are *representative spots*
/// (clustered, at most a dozen or so); the exact counts are on
/// [`MeshReport::validator`].
#[derive(Clone, Debug, Default)]
pub struct MeshReport {
    /// The verdict of [`cadpole_core::solid::validate`]: closed, consistently
    /// oriented, and free of crossings and doubled faces.
    pub solid: bool,
    /// Closed, orientable and consistently oriented: every edge shared by
    /// exactly two triangles that traverse it in opposite directions. Weaker
    /// than `solid`: an inside-out surface, a doubled face and two skins
    /// through each other are all watertight.
    pub watertight: bool,
    /// Edges belonging to exactly one triangle.
    pub boundary_edges: usize,
    /// The rims those edges form.
    pub holes: Vec<Hole>,
    /// Midpoints of edges shared by three or more triangles.
    pub non_manifold: Vec<Vec3>,
    /// Midpoints of edges whose two triangles traverse them the same way —
    /// one of the pair is wound backwards.
    pub flipped_edges: Vec<Vec3>,
    /// True when the mesh is closed but encloses a negative volume: the whole
    /// surface is inside out.
    pub inverted: bool,
    /// Positions of triangles with no area.
    pub degenerate: Vec<Vec3>,
    /// How many triangles collapsed to a line or a point when welded.
    pub collapsed: usize,
    /// Midway points between pairs of triangles that pass through each other.
    pub self_intersections: Vec<Vec3>,
    /// Spots where two coplanar triangles lie on top of each other: a face
    /// emitted twice, or the wall two fused parts still share.
    pub internal_faces: Vec<Vec3>,
    /// Set when the pairwise checks were skipped because the mesh is too big
    /// for their `O(n·k)` narrow phase.
    pub self_intersection_skipped: bool,
    /// Separate closed shells: 1 for a plain part, 2 for one with a cavity.
    pub components: usize,
    /// Total genus (0 for a ball, 1 for a ring), when the surface is closed.
    pub genus: Option<i64>,
    /// The validator's full report, with every defect's position and the
    /// exact counts.
    pub validator: SolidReport,
}

/// Above this triangle count the pairwise checks (crossings, doubled faces)
/// are skipped and said so, rather than spending a minute on a check the user
/// did not ask for.
const SELF_INTERSECT_LIMIT: usize = 250_000;

pub fn check(original: &Mesh, welded: &Mesh, issues: &mut Vec<Issue>) -> MeshReport {
    let collapsed = original.triangle_count().saturating_sub(welded.triangle_count());
    let opts = SolidOptions { weld_eps: crate::WELD_EPS, pair_limit: SELF_INTERSECT_LIMIT };
    let v = solid::validate_welded(welded, collapsed, &opts);
    let scale = scale_of(welded);
    let spots = |ps: &[Vec3], max: usize, radius: f64| -> Vec<Vec3> {
        cluster(ps.iter().map(|p| (*p, 0.0)).collect(), max, radius, false).into_iter().map(|(p, _)| p).collect()
    };
    let pos = |i: u32| {
        let i = i as usize * 3;
        Vec3::new(welded.positions[i] as f64, welded.positions[i + 1] as f64, welded.positions[i + 2] as f64)
    };

    let watertight = v.closed() && v.flipped.is_empty();
    let mut r = MeshReport {
        solid: v.is_solid(),
        watertight,
        boundary_edges: v.boundary.len(),
        holes: rims(&v.boundary, &pos),
        non_manifold: spots(&v.non_manifold, 12, scale * 0.01),
        flipped_edges: spots(&v.flipped, 12, scale * 0.01),
        inverted: watertight && v.volume < 0.0,
        degenerate: spots(&v.degenerate, 8, scale * 0.02),
        collapsed,
        self_intersections: spots(&v.crossings.iter().map(|c| c.at).collect::<Vec<_>>(), 8, scale * 0.02),
        internal_faces: spots(&v.overlaps.iter().map(|c| c.at).collect::<Vec<_>>(), 8, scale * 0.02),
        self_intersection_skipped: v.pairs_skipped,
        components: v.components,
        genus: v.genus,
        validator: SolidReport::default(),
    };
    let plural = |n: usize| if n == 1 { "" } else { "s" };
    let eg = |p: Option<&Vec3>, word: &str| match p {
        Some(p) => format!(", e.g. {word} {}", fmt_at(*p)),
        None => String::new(),
    };

    // ---- the verdict ----------------------------------------------------
    // One error line that says "not a solid" and why, before the findings
    // that say where. A print check is advice, but a body that is not a
    // solid is not advice: nothing downstream can be trusted with it.
    if !r.solid {
        issues.push(Issue::new(
            Check::Solid,
            Severity::Error,
            format!("not a solid: {}", v.problems().join("; ")),
            None,
            v.failing().len() as f64,
        ));
    }

    // ---- degenerate triangles ------------------------------------------
    let degenerate_total = v.degenerate.len() + collapsed;
    if degenerate_total > 0 {
        issues.push(Issue::new(
            Check::Degenerate,
            Severity::Warning,
            format!(
                "{degenerate_total} degenerate triangle{} (no area){}",
                plural(degenerate_total),
                match r.degenerate.first() {
                    Some(p) => format!(", e.g. at {}", fmt_at(*p)),
                    None => " collapsed when welding".to_owned(),
                }
            ),
            r.degenerate.first().copied(),
            degenerate_total as f64,
        ));
    }

    // ---- holes ----------------------------------------------------------
    for h in &r.holes {
        issues.push(Issue::new(
            Check::Watertight,
            Severity::Error,
            format!(
                "hole in the surface at {}: {} boundary edge{}, {:.2} mm around",
                fmt_at(h.center),
                h.edges,
                plural(h.edges),
                h.perimeter
            ),
            Some(h.center),
            h.perimeter,
        ));
    }

    // ---- non-manifold, flipped ------------------------------------------
    if !v.non_manifold.is_empty() {
        let n = v.non_manifold.len();
        issues.push(Issue::new(
            Check::NonManifold,
            Severity::Error,
            format!(
                "{n} non-manifold edge{} (three or more triangles meet along them; an internal wall \
                 between two fused lumps gives four){}",
                plural(n),
                eg(r.non_manifold.first(), "at")
            ),
            r.non_manifold.first().copied(),
            n as f64,
        ));
    }
    if !v.flipped.is_empty() {
        let n = v.flipped.len();
        issues.push(Issue::new(
            Check::Orientation,
            Severity::Error,
            format!(
                "{n} edge{} where the two triangles are wound the same way: a face's normal points \
                 the wrong way{}",
                plural(n),
                eg(r.flipped_edges.first(), "at")
            ),
            r.flipped_edges.first().copied(),
            n as f64,
        ));
    }

    // ---- inside out ------------------------------------------------------
    if r.inverted {
        issues.push(Issue::new(
            Check::Orientation,
            Severity::Error,
            "the surface is inside out: every normal points into the solid, so a slicer would \
             read the model as a void"
                .to_owned(),
            None,
            v.volume.abs(),
        ));
    }

    // ---- crossings and doubled faces -----------------------------------
    if v.pairs_skipped {
        issues.push(Issue::new(
            Check::SelfIntersection,
            Severity::Info,
            format!(
                "self-intersection check skipped: {} triangles is over the {SELF_INTERSECT_LIMIT} limit",
                welded.triangle_count()
            ),
            None,
            welded.triangle_count() as f64,
        ));
    }
    if !v.crossings.is_empty() {
        let n = v.crossings.len();
        let deepest = v.crossings.iter().map(|c| c.depth).fold(0.0, f64::max);
        issues.push(Issue::new(
            Check::SelfIntersection,
            Severity::Error,
            format!(
                "{n} pair{} of triangles pass through each other, up to {deepest:.3} mm deep{}; a union \
                 or a boolean would resolve them",
                plural(n),
                eg(r.self_intersections.first(), "near")
            ),
            r.self_intersections.first().copied(),
            n as f64,
        ));
    }
    if !v.overlaps.is_empty() {
        let n = v.overlaps.len();
        let area: f64 = v.overlaps.iter().map(|c| c.extent).sum();
        issues.push(Issue::new(
            Check::InternalFace,
            Severity::Error,
            format!(
                "{n} pair{} of coplanar triangles lie on top of each other ({area:.3} mm²): a face \
                 emitted twice, or a wall two fused parts still share{}",
                plural(n),
                eg(r.internal_faces.first(), "near")
            ),
            r.internal_faces.first().copied(),
            area,
        ));
    }

    // ---- folds ---------------------------------------------------------
    // Not a defect of the solid — every edge is still two triangles wound
    // against each other — so a warning, in the validator's own words: what
    // it is, where the worst one is, and what it will cost the next boolean.
    if let Some(f) = v.worst_fold() {
        issues.push(Issue::new(
            Check::Fold,
            Severity::Warning,
            v.warnings().into_iter().next().unwrap_or_default(),
            Some(f.at),
            v.folds.len() as f64,
        ));
    }

    // ---- shells ------------------------------------------------------------
    if v.components > 1 {
        issues.push(Issue::new(
            Check::Solid,
            Severity::Info,
            format!(
                "{} separate shells: a cavity inside the part, or pieces that do not touch",
                v.components
            ),
            None,
            v.components as f64,
        ));
    }

    r.validator = v;
    r
}

/// The rims of the holes in an already-welded mesh, from the boundary edges
/// [`cadpole_core::solid::validate_welded`] found: the *where* to go with the
/// validator's *what*, for a caller that has to refuse an open surface and
/// say where — the exporters — without running the whole print check.
pub fn holes(welded: &Mesh, boundary: &[(u32, u32)]) -> Vec<Hole> {
    let pos = |i: u32| {
        let i = i as usize * 3;
        Vec3::new(welded.positions[i] as f64, welded.positions[i + 1] as f64, welded.positions[i + 2] as f64)
    };
    rims(boundary, &pos)
}

/// Chain directed boundary edges into rims. Each rim is walked by following
/// `end -> next edge starting there`; a vertex where several boundary edges
/// start (a pinched hole) just ends that rim early rather than looping.
fn rims(boundary: &[(u32, u32)], pos: &impl Fn(u32) -> Vec3) -> Vec<Hole> {
    let mut next: HashMap<u32, Vec<u32>> = HashMap::new();
    for &(a, b) in boundary {
        next.entry(a).or_default().push(b);
    }
    // Deterministic order: rims are reported starting from the lowest vertex
    // index, and successors are followed in index order.
    for v in next.values_mut() {
        v.sort_unstable();
    }
    let mut starts: Vec<u32> = next.keys().copied().collect();
    starts.sort_unstable();

    let mut used: HashMap<(u32, u32), bool> = HashMap::new();
    let mut out = Vec::new();
    for &s in &starts {
        loop {
            // Is there an unused edge leaving `s`?
            let Some(&first) = next.get(&s).and_then(|v| v.iter().find(|&&b| !used.contains_key(&(s, b)))) else {
                break;
            };
            let mut ring = vec![s];
            let mut cur = s;
            let mut nxt = first;
            let mut perimeter = 0.0;
            let mut edges = 0usize;
            loop {
                used.insert((cur, nxt), true);
                perimeter += (pos(nxt) - pos(cur)).length();
                edges += 1;
                if nxt == s || edges > boundary.len() {
                    break;
                }
                ring.push(nxt);
                cur = nxt;
                match next.get(&cur).and_then(|v| v.iter().find(|&&b| !used.contains_key(&(cur, b)))) {
                    Some(&b) => nxt = b,
                    None => break,
                }
            }
            let mut c = Vec3::ZERO;
            let mut lo = Vec3::new(f64::MAX, f64::MAX, f64::MAX);
            let mut hi = Vec3::new(f64::MIN, f64::MIN, f64::MIN);
            for &v in &ring {
                let p = pos(v);
                c = c + p;
                lo = lo.min(p);
                hi = hi.max(p);
            }
            out.push(Hole { center: c * (1.0 / ring.len() as f64), perimeter, edges, bbox: (lo, hi) });
        }
    }
    // Biggest rim first: that is the one worth looking at.
    out.sort_by(|a, b| b.perimeter.partial_cmp(&a.perimeter).unwrap_or(std::cmp::Ordering::Equal));
    out.truncate(16);
    out
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::WELD_EPS;
    use crate::testing::*;

    fn run(m: &Mesh) -> (MeshReport, Vec<Issue>) {
        let w = m.welded(WELD_EPS);
        let mut issues = Vec::new();
        let r = check(m, &w, &mut issues);
        (r, issues)
    }

    #[test]
    fn a_box_is_watertight_and_quiet() {
        let (r, issues) = run(&box_mesh(10.0, 10.0, 10.0));
        assert!(r.solid);
        assert!(r.watertight);
        assert_eq!(r.boundary_edges, 0);
        assert!(r.holes.is_empty());
        assert!(!r.inverted);
        assert_eq!(r.components, 1);
        assert_eq!(r.genus, Some(0));
        assert!(issues.is_empty(), "{issues:?}");
    }

    #[test]
    fn a_hollow_box_is_a_solid_of_two_shells() {
        let (r, issues) = run(&hollow_box(20.0, 2.0));
        assert!(r.solid, "{issues:?}");
        assert_eq!(r.components, 2);
        let i = issues.iter().find(|i| i.check == Check::Solid).expect("a shells line");
        assert_eq!(i.severity, Severity::Info);
        assert!(i.message.contains("2 separate shells"), "{}", i.message);
    }

    #[test]
    fn a_shell_nested_the_wrong_way_round_is_not_a_solid() {
        // A 10 mm cube with a 4 mm cube inside, both facing out: the inner
        // region is covered twice and the volume reads 1064, not 1000.
        let mut nested = Mesh::default();
        add_box(&mut nested, Vec3::ZERO, Vec3::new(10.0, 10.0, 10.0), false, 0);
        add_box(&mut nested, Vec3::new(3.0, 3.0, 3.0), Vec3::new(7.0, 7.0, 7.0), false, 6);
        // A cube beside it, wholly inside out, outweighed by the 10 mm one.
        let mut beside = Mesh::default();
        add_box(&mut beside, Vec3::ZERO, Vec3::new(10.0, 10.0, 10.0), false, 0);
        add_box(&mut beside, Vec3::new(20.0, 0.0, 0.0), Vec3::new(24.0, 4.0, 4.0), true, 6);
        for (name, m) in [("nested", &nested), ("beside", &beside)] {
            let (r, issues) = run(m);
            assert!(!r.solid, "{name}: {issues:?}");
            let i = issues.iter().find(|i| i.check == Check::Solid && i.severity == Severity::Error);
            assert!(i.is_some_and(|i| i.message.contains("nested")), "{name}: {issues:?}");
        }
        // The legitimate cavity, inner shell facing in, is still a solid.
        let (r, issues) = run(&hollow_box(20.0, 2.0));
        assert!(r.solid, "{issues:?}");
    }

    #[test]
    fn a_missing_top_is_reported_as_one_hole_at_the_top() {
        // 20x10 top removed: the rim is 4 edges, 60 mm around, centred on the
        // middle of the top face.
        let (r, issues) = run(&holed_box(20.0, 10.0, 6.0));
        assert!(!r.watertight, "a box with no lid is not watertight");
        assert!(!r.solid);
        assert_eq!(r.boundary_edges, 4, "the rim of the missing top");
        assert_eq!(r.holes.len(), 1);
        let h = &r.holes[0];
        assert_eq!(h.edges, 4);
        assert!((h.perimeter - 60.0).abs() < 1e-6, "perimeter {}", h.perimeter);
        assert!((h.center - Vec3::new(10.0, 5.0, 6.0)).length() < 1e-6, "center {:?}", h.center);
        let m = &issues.iter().find(|i| i.check == Check::Watertight).expect("a watertight issue").message;
        assert!(m.contains("(10, 5, 6)"), "{m}");
        assert!(m.contains("4 boundary edges"), "{m}");
        let v = issues.iter().find(|i| i.check == Check::Solid).expect("the verdict");
        assert_eq!(v.severity, Severity::Error);
        assert!(v.message.starts_with("not a solid: 4 boundary edges"), "{}", v.message);
    }

    #[test]
    fn an_inside_out_box_is_flagged() {
        let mut m = Mesh::default();
        add_box(&mut m, Vec3::ZERO, Vec3::new(5.0, 5.0, 5.0), true, 0);
        let (r, issues) = run(&m);
        assert!(r.watertight, "flipping every triangle keeps it closed");
        assert!(r.inverted);
        assert!(!r.solid);
        assert!(issues.iter().any(|i| i.message.contains("inside out")), "{issues:?}");
    }

    #[test]
    fn one_backwards_face_is_an_orientation_error() {
        let (r, issues) = run(&flipped_face_box(8.0, 8.0, 8.0));
        assert!(!r.watertight);
        // The +Z face has four boundary edges with the rest of the box, and
        // its own diagonal stays consistent.
        assert_eq!(r.flipped_edges.len(), 4, "{:?}", r.flipped_edges);
        assert!(issues.iter().any(|i| i.check == Check::Orientation), "{issues:?}");
    }

    #[test]
    fn a_zero_area_triangle_is_found_where_it_is() {
        let (r, issues) = run(&box_with_degenerate(10.0, 10.0, 10.0));
        assert_eq!(r.degenerate.len(), 1);
        assert!((r.degenerate[0] - Vec3::new(5.0, 0.0, 0.0)).length() < 1e-9, "{:?}", r.degenerate[0]);
        assert!(issues.iter().any(|i| i.check == Check::Degenerate), "{issues:?}");
    }

    #[test]
    fn two_boxes_run_through_each_other() {
        let (r, issues) = run(&overlapping_boxes());
        assert!(!r.self_intersections.is_empty(), "the skins cross");
        assert!(r.watertight, "each skin is closed on its own");
        assert!(!r.solid, "but together they are not a solid");
        let i = issues.iter().find(|i| i.check == Check::SelfIntersection).expect("a self-intersection issue");
        assert_eq!(i.severity, Severity::Error);
        assert!(i.message.contains("mm deep"), "{}", i.message);
    }

    #[test]
    fn a_wall_two_fused_boxes_still_share_is_an_internal_face() {
        // Two closed boxes side by side, the shared face emitted by both:
        // what a boolean that left the seam in looks like.
        let mut m = box_mesh(10.0, 10.0, 10.0);
        append(&mut m, &box_between(Vec3::new(10.0, 0.0, 0.0), Vec3::new(20.0, 10.0, 10.0)));
        let (r, issues) = run(&m);
        assert!(!r.solid);
        assert!(!r.watertight, "the seam's edges are on four triangles");
        assert!(!r.internal_faces.is_empty());
        let i = issues.iter().find(|i| i.check == Check::InternalFace).expect("an internal-face issue");
        assert_eq!(i.severity, Severity::Error);
        assert!(i.message.contains("lie on top of each other"), "{}", i.message);
        assert!(issues.iter().any(|i| i.check == Check::NonManifold), "{issues:?}");
    }
}
