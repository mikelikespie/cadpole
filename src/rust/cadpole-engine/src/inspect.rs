//! `Inspect`: what the geometry is, and what the kernel did to make it.
//!
//! Three things about one body are otherwise learned only by rebuilding the
//! kernel with trace lines: that a boolean ran on the
//! engine that keeps no face provenance, that the cleanup threw away
//! thousands of good edits over one bad pass, and that the body carries a
//! fold the validator does not report. Every one of those is either a
//! field of [`pb::BodyInspection`] or a kernel note on the feature that did
//! it, and this operation is how they are read: one call, one body at a
//! time, no rebuild.
//!
//! The per-body half ([`inspect_body`]) is a pure function of a mesh and its
//! topology, so it can be tested on a hand-built fixture with a known fold or
//! a known hole; the document half evaluates, walks the bodies, and groups
//! the diagnostics by feature.

use cadpole_core::Context;
use cadpole_core::context::{Diagnostic, Severity};
use cadpole_core::geom::Vec3;
use cadpole_core::mesh_clean::min_altitude;
use cadpole_core::mesh_kernel;
use cadpole_core::solid::{self, SolidOptions, SolidReport};
use cadpole_core::tessellate::{Mesh, Topology};
use cadpole_proto::v1 as pb;

/// The `Inspect` rpc.
pub fn inspect(req: &pb::InspectRequest) -> anyhow::Result<pb::InspectResponse> {
    let doc = req.document.as_ref().ok_or_else(|| anyhow::anyhow!("inspect: no document"))?;
    let extra = crate::overrides_from_proto(&req.param_overrides);
    let (mut ctx, _) = crate::evaluate_document_with(doc, &extra, req.tolerance, req.boolean_tolerance);
    // Bodies first: tessellating one for the report can itself produce a
    // diagnostic (a face refused by name), which then belongs in the
    // diagnostics.
    let bodies = inspect_context(&mut ctx, &req.body_ids)?;
    let (_, instances, _) = ctx.assembly_lists();
    let features = grouped_diagnostics(&ctx);
    let ok = !ctx.diagnostics.iter().any(|d| d.severity == Severity::Error);
    let display_units =
        ctx.display_units.as_ref().map(|u| pb::DisplayUnits { unit: u.unit.clone(), fraction: u.fraction });
    Ok(pb::InspectResponse { bodies, features, ok, instances, display_units })
}

/// Inspect the visible bodies of an evaluated context, or the ones named in
/// `body_ids` (`feature#n`). Naming a body that does not exist is an error
/// that lists the ones that do; a document with no bodies at all (a script
/// that failed before making any) is an empty report, not an error, so the
/// diagnostics that say why still come back.
pub fn inspect_context(ctx: &mut Context, body_ids: &[String]) -> anyhow::Result<Vec<pb::BodyInspection>> {
    let all: Vec<(cadpole_core::BodyId, String)> = ctx.bodies.iter().map(|b| (b.id, b.id_string())).collect();
    let chosen: Vec<cadpole_core::BodyId> = if body_ids.is_empty() {
        ctx.bodies.iter().filter(|b| b.visible).map(|b| b.id).collect()
    } else {
        let mut out = Vec::new();
        for want in body_ids {
            match all.iter().find(|(_, s)| s == want) {
                Some((id, _)) => out.push(*id),
                None => anyhow::bail!(
                    "inspect: no body {want:?}; the document has {}",
                    if all.is_empty() {
                        "none".to_owned()
                    } else {
                        all.iter().map(|(_, s)| s.as_str()).collect::<Vec<_>>().join(", ")
                    }
                ),
            }
        }
        out
    };
    let mut out = Vec::with_capacity(chosen.len());
    for id in chosen {
        let (body_id, name, feature, kind) = {
            let b = ctx.body(id)?;
            (b.id_string(), b.name.clone(), b.feature.0.clone(), b.kind().to_owned())
        };
        let (mesh, topo) = ctx.body_mesh(id)?;
        let mut b = inspect_body(&body_id, &name, &feature, &kind, mesh, topo);
        // What that mesh was made to: the tolerance and its source,
        // the tolerance it was made at, and the chord error measured.
        let (options, source) = ctx.tessellation_for(id, None)?;
        let q = ctx.mesh_quality(id)?;
        b.tolerance = options.tolerance;
        b.tolerance_source = source.as_str().to_owned();
        b.tessellated_at = q.as_ref().map(|q| q.tessellated_at).unwrap_or(options.tolerance);
        b.chord_error = q.as_ref().and_then(|q| q.chord_error());
        b.chord_error_face = q.as_ref().map(|q| q.chord_face.clone()).unwrap_or_default();
        b.chord_error_at = q.and_then(|q| q.chord.map(|c| c.at.to_proto()));
        out.push(b);
    }
    Ok(out)
}

/// Everything `Inspect` reports about one body, from its mesh and topology.
pub fn inspect_body(body_id: &str, name: &str, feature_id: &str, kind: &str, mesh: &Mesh, topo: &Topology) -> pb::BodyInspection {
    // Weld once and keep the welded mesh: the report's boundary edges are
    // welded indices, and the first hole's position needs their coordinates.
    let opts = SolidOptions::default();
    let welded = mesh.welded(opts.weld_eps);
    let collapsed = mesh.triangle_count().saturating_sub(welded.triangle_count());
    let report = solid::validate_welded(&welded, collapsed, &opts);

    let mut min_alt = f64::INFINITY;
    let mut min_at = Vec3::ZERO;
    for t in 0..mesh.triangle_count() {
        if let Some([a, b, c]) = mesh.triangle(t) {
            let h = min_altitude(a, b, c);
            if h < min_alt {
                min_alt = h;
                min_at = (a + b + c) * (1.0 / 3.0);
            }
        }
    }
    let manifold = mesh_kernel::self_intersects(mesh);
    let thin_faces = topo.faces.iter().filter(|f| f.tri_count <= 2).count();
    let surfaces = surface_counts(topo);
    let face_name = |id: u32| topo.faces.get(id as usize).map(|f| f.name.clone()).unwrap_or_else(|| format!("face:{id}"));
    let verdict = solid_to_proto(&report, &welded, &face_name);

    let summary = format!(
        "{kind}, {} face{} ({} of one or two triangles; {}), {} edge{}, {} triangle{}, {:.1} mm³; {}; manifold says {}; thinnest triangle {}",
        topo.faces.len(),
        s(topo.faces.len()),
        thin_faces,
        surfaces_line(&surfaces),
        topo.edges.len(),
        s(topo.edges.len()),
        mesh.triangle_count(),
        s(mesh.triangle_count()),
        mesh.volume(),
        report.summary(),
        match manifold {
            Some(true) => "self-intersecting (its boolean dispatch would use the robust engine)",
            Some(false) => "not self-intersecting",
            None => "nothing (it could not take the mesh)",
        },
        if min_alt.is_finite() {
            format!("{min_alt:.4} mm at ({:.2}, {:.2}, {:.2})", min_at.x, min_at.y, min_at.z)
        } else {
            "none".to_owned()
        }
    );

    pb::BodyInspection {
        body_id: body_id.to_owned(),
        name: name.to_owned(),
        feature_id: feature_id.to_owned(),
        kind: kind.to_owned(),
        faces: topo.faces.len() as u32,
        edges: topo.edges.len() as u32,
        triangles: mesh.triangle_count() as u32,
        thin_faces: thin_faces as u32,
        bbox: mesh.bbox().map(|(lo, hi)| pb::BBox { min: Some(lo.to_proto()), max: Some(hi.to_proto()) }),
        volume_mm3: mesh.volume(),
        area_mm2: mesh.area(),
        solid: Some(verdict),
        min_altitude_mm: if min_alt.is_finite() { min_alt } else { 0.0 },
        min_altitude_at: min_alt.is_finite().then(|| min_at.to_proto()),
        manifold_self_intersecting: manifold.unwrap_or(false),
        manifold_verdict_known: manifold.is_some(),
        summary,
        surfaces,
        // Filled in by `inspect_context`, which has the context the mesh
        // was made in; a bare mesh has no tolerance to report.
        ..Default::default()
    }
}

/// The class vocabulary in report order, `other` last.
const SURFACE_CLASSES: [&str; 6] = ["plane", "cylinder", "cone", "sphere", "torus", "other"];

/// Faces by recognised analytic surface (`cadpole_core::surface_class`), in
/// [`SURFACE_CLASSES`] order, classes with no face left out. On a mesh body
/// every face is a triangle group and reads `other`.
fn surface_counts(topo: &Topology) -> Vec<pb::SurfaceClassCount> {
    SURFACE_CLASSES
        .iter()
        .filter_map(|k| {
            let n = topo.faces.iter().filter(|f| f.surface_class() == *k).count();
            (n > 0).then(|| pb::SurfaceClassCount { kind: (*k).to_owned(), faces: n as u32 })
        })
        .collect()
}

/// `6 plane, 1 cylinder, 2 other`, or `no faces`.
fn surfaces_line(counts: &[pb::SurfaceClassCount]) -> String {
    if counts.is_empty() {
        return "no faces".to_owned();
    }
    counts.iter().map(|c| format!("{} {}", c.faces, c.kind)).collect::<Vec<_>>().join(", ")
}

/// The validator's report as the rpc carries it: each failing check with
/// its count and first position, and the folds.
fn solid_to_proto(r: &SolidReport, welded: &Mesh, face_name: &dyn Fn(u32) -> String) -> pb::SolidVerdict {
    let pos = |i: u32| {
        let i = i as usize * 3;
        Vec3::new(welded.positions[i] as f64, welded.positions[i + 1] as f64, welded.positions[i + 2] as f64)
    };
    // `failing()` and `problems()` walk the same conditions in the same
    // order, so the i-th name has the i-th sentence.
    let problems = r.problems();
    let failing = r
        .failing()
        .into_iter()
        .enumerate()
        .map(|(i, check)| {
            let (count, at): (usize, Option<Vec3>) = match check {
                "holes" => (r.boundary.len(), r.boundary.first().map(|&(a, b)| (pos(a) + pos(b)) * 0.5)),
                "non-manifold" => (r.non_manifold.len(), r.non_manifold.first().copied()),
                "flipped" => (r.flipped.len(), r.flipped.first().copied()),
                "crossings" => (r.crossings.len(), r.crossings.first().map(|c| c.at)),
                "internal-faces" => (r.overlaps.len(), r.overlaps.first().map(|c| c.at)),
                // "empty" and "inside-out" are about the whole surface.
                _ => (1, None),
            };
            pb::SolidCheck {
                check: check.to_owned(),
                count: count as u32,
                at: at.map(|p| p.to_proto()),
                message: problems.get(i).cloned().unwrap_or_default(),
            }
        })
        .collect();
    pb::SolidVerdict {
        is_solid: r.is_solid(),
        summary: r.summary(),
        failing,
        folds: r.folds.len() as u32,
        worst_fold: r.worst_fold().map(|f| pb::FoldReport {
            at: Some(f.at.to_proto()),
            tip_gap_mm: f.tip_gap,
            dot: f.dot,
            face_a: face_name(f.faces.0),
            face_b: face_name(f.faces.1),
        }),
        pairs_skipped: r.pairs_skipped,
        components: r.components as u32,
        genus: r.genus.map(|g| g as i32).unwrap_or(-1),
        degenerate: r.degenerate.len() as u32,
    }
}

/// The evaluation's diagnostics grouped by the feature they belong to, in
/// the order the features were first mentioned; the ones with no feature
/// first, under an empty id.
pub fn grouped_diagnostics(ctx: &Context) -> Vec<pb::FeatureDiagnostics> {
    let mut groups: Vec<pb::FeatureDiagnostics> = Vec::new();
    let mut index: std::collections::HashMap<String, usize> = std::collections::HashMap::new();
    let mut push = |d: &Diagnostic, groups: &mut Vec<pb::FeatureDiagnostics>| {
        let id = d.feature.as_ref().map(|f| f.0.clone()).unwrap_or_default();
        let i = match index.get(&id) {
            Some(&i) => i,
            None => {
                let (kind, span) = ctx
                    .features
                    .iter()
                    .find(|f| f.id.0 == id)
                    .map(|f| (f.kind.clone(), f.span.as_ref().map(|s| s.to_proto())))
                    .unwrap_or_default();
                groups.push(pb::FeatureDiagnostics { feature_id: id.clone(), kind, span, diagnostics: Vec::new() });
                index.insert(id, groups.len() - 1);
                groups.len() - 1
            }
        };
        groups[i].diagnostics.push(d.to_proto());
    };
    // Feature-less diagnostics first, whatever order they were raised in.
    for d in ctx.diagnostics.iter().filter(|d| d.feature.is_none()) {
        push(d, &mut groups);
    }
    for d in ctx.diagnostics.iter().filter(|d| d.feature.is_some()) {
        push(d, &mut groups);
    }
    groups
}

fn s(n: usize) -> &'static str {
    if n == 1 { "" } else { "s" }
}

#[cfg(test)]
mod tests {
    use super::*;
    use cadpole_core::solid::fixtures::{box_mesh, holed_box, slit_box};
    use cadpole_core::tessellate::FaceInfo;

    /// A topology with one face per distinct face id, so the report can name
    /// the faces a fold belongs to.
    fn topo_for(mesh: &Mesh) -> Topology {
        let n = mesh.face_ids.iter().max().map(|f| *f as usize + 1).unwrap_or(0);
        let faces = (0..n)
            .map(|i| FaceInfo {
                name: format!("fixture/face[{i}]"),
                tri_count: mesh.face_ids.iter().filter(|&&f| f as usize == i).count() as u32,
                ..Default::default()
            })
            .collect();
        Topology { faces, ..Default::default() }
    }

    /// A body that is a solid by every
    /// test and carries one fold. Every field needed to diagnose it
    /// is filled in, manifold's own verdict beside ours.
    #[test]
    fn a_known_fold_is_reported_with_its_faces_and_manifolds_verdict() {
        let mesh = slit_box(0.05);
        let topo = topo_for(&mesh);
        let b = inspect_body("slit#0", "slit", "slit", "mesh", &mesh, &topo);
        assert_eq!((b.faces, b.triangles), (9, 24));
        assert_eq!(b.thin_faces, 7, "every face but the notched front and back is one or two triangles");
        let v = b.solid.as_ref().unwrap();
        assert!(v.is_solid, "{}", v.summary);
        assert!(v.failing.is_empty(), "{:?}", v.failing);
        assert_eq!(v.folds, 1);
        let f = v.worst_fold.as_ref().unwrap();
        assert!((f.tip_gap_mm - 0.05).abs() < 1e-6, "{f:?}");
        assert!(f.dot < -0.999, "{f:?}");
        let at = f.at.as_ref().unwrap();
        assert!((at.x - 5.0).abs() < 1e-6 && (at.y - 5.0).abs() < 1e-6 && (at.z - 1.0).abs() < 1e-6, "{at:?}");
        assert_eq!((f.face_a.as_str(), f.face_b.as_str()), ("fixture/face[7]", "fixture/face[8]"));
        assert!(b.manifold_verdict_known, "manifold takes a closed manifold mesh");
        assert!(b.summary.contains("warning: 1 fold"), "{}", b.summary);
        assert_eq!(b.surfaces.len(), 1, "a hand-built mesh has no recognised surfaces: {:?}", b.surfaces);
        assert_eq!((b.surfaces[0].kind.as_str(), b.surfaces[0].faces), ("other", 9));
        assert!(b.summary.contains("9 other"), "{}", b.summary);
        assert!(b.summary.contains("manifold says "), "{}", b.summary);
        assert!(b.min_altitude_mm > 0.0 && b.min_altitude_at.is_some());
        assert!(b.bbox.is_some() && b.volume_mm3 > 0.0 && b.area_mm2 > 0.0);
        assert!((v.components, v.genus) == (1, 0), "{}", v.summary);
    }

    /// A known non-solid: the check that fails is named, counted and located.
    #[test]
    fn a_known_hole_is_a_failing_check_with_a_position() {
        let mesh = holed_box(20.0, 10.0, 6.0);
        let topo = topo_for(&mesh);
        let b = inspect_body("b#1", "b", "b", "mesh", &mesh, &topo);
        let v = b.solid.as_ref().unwrap();
        assert!(!v.is_solid);
        assert_eq!(v.failing.len(), 1, "{:?}", v.failing);
        let holes = &v.failing[0];
        assert_eq!(holes.check, "holes");
        assert_eq!(holes.count, 4, "the missing top's four rim edges");
        let at = holes.at.as_ref().expect("the first boundary edge's midpoint");
        assert!((at.z - 6.0).abs() < 1e-6, "on the top: {at:?}");
        assert!(holes.message.contains("boundary edge"), "{}", holes.message);
        assert_eq!(v.folds, 0);
        assert_eq!(v.genus, -1, "undefined on an open surface");
        assert!(!b.manifold_verdict_known, "manifold cannot take an open mesh");
        assert!(b.summary.starts_with("mesh, "), "{}", b.summary);
        assert!(b.summary.contains("not a solid"), "{}", b.summary);

        // And a plain box is clean on every count.
        let mesh = box_mesh(10.0, 10.0, 10.0);
        let b = inspect_body("c#2", "c", "c", "brep", &mesh, &topo_for(&mesh));
        let v = b.solid.as_ref().unwrap();
        assert!(v.is_solid && v.failing.is_empty() && v.folds == 0 && v.worst_fold.is_none());
        assert_eq!((v.components, v.genus), (1, 0));
        assert!(b.manifold_verdict_known && !b.manifold_self_intersecting);
        assert_eq!(b.thin_faces, 6, "a box's faces are two triangles each");
    }

    /// Through the document: the bodies come back in model order, unknown
    /// ids are refused with the known ones, and the diagnostics are grouped
    /// under the feature that raised them, with that feature's span.
    #[test]
    fn a_document_is_inspected_body_by_body_with_diagnostics_by_feature() {
        // Two boxes sharing a face: the B-rep kernel refuses the union and
        // the context says so on the feature, which is the kind of
        // diagnostic the grouping is for.
        let doc = crate::document_from_scripts(
            "t",
            vec![(
                "main.star".into(),
                "a = box(20, 20, 10, name=\"a\")\n\
                 b = box(10, 10, 6, at=(0, 0, 10), name=\"b\")\n\
                 part = union(a, b, name=\"part\")\n"
                    .into(),
            )],
        );
        let resp = inspect(&pb::InspectRequest { document: Some(doc.clone()), ..Default::default() }).unwrap();
        assert!(resp.ok, "{:?}", resp.features);
        assert_eq!(resp.bodies.len(), 1, "one visible body: {:?}", resp.bodies.iter().map(|b| &b.body_id).collect::<Vec<_>>());
        let part = &resp.bodies[0];
        assert!(part.body_id.starts_with("part#"), "{}", part.body_id);
        assert_eq!((part.name.as_str(), part.feature_id.as_str(), part.kind.as_str()), ("part", "part", "mesh"));
        let v = part.solid.as_ref().unwrap();
        assert!(v.is_solid, "{}", v.summary);
        assert_eq!((v.components, v.genus), (1, 0), "{}", v.summary);
        assert!((part.volume_mm3 - 4600.0).abs() < 1e-3, "{}", part.volume_mm3);
        assert!(part.faces < part.triangles, "the exact engine kept provenance: {} faces for {} triangles", part.faces, part.triangles);
        // Every face of two unioned boxes is a plane, and the report says so
        // in the vocabulary a script would use — the recognised class, not
        // truck's type.
        assert_eq!(part.surfaces.len(), 1, "{:?}", part.surfaces);
        assert_eq!((part.surfaces[0].kind.as_str(), part.surfaces[0].faces), ("plane", part.faces), "{:?}", part.surfaces);
        assert!(part.summary.contains(&format!("{} plane", part.faces)), "{}", part.summary);

        // A named body, and a named body that is not there.
        let one = inspect(&pb::InspectRequest { document: Some(doc.clone()), body_ids: vec![part.body_id.clone()], ..Default::default() }).unwrap();
        assert_eq!(one.bodies.len(), 1);
        let e = inspect(&pb::InspectRequest { document: Some(doc.clone()), body_ids: vec!["nope#9".into()], ..Default::default() }).unwrap_err().to_string();
        assert!(e.contains("nope#9") && e.contains(&part.body_id), "{e}");

        // The fallback's info lands under `part`, with the union's span.
        let part_group = resp.features.iter().find(|f| f.feature_id == "part").expect("a group for part");
        assert_eq!(part_group.kind, "union");
        assert_eq!(part_group.span.as_ref().map(|s| s.start_line), Some(3));
        assert!(part_group.diagnostics.iter().all(|d| d.feature_id == "part"));
        assert!(
            part_group.diagnostics.iter().any(|d| d.message.contains("retrying on the mesh kernel")),
            "{:?}",
            part_group.diagnostics
        );

        // A script error is a feature-less group, first, and `ok` is false;
        // what was built before it is still inspected. (A *runtime* error:
        // an unresolved name fails before anything runs, and then there is
        // nothing to inspect but the error.)
        let broken = crate::document_from_scripts("t", vec![("main.star".into(), "b = box(1, 1, 1)\nfail(\"stop\")\n".into())]);
        let resp = inspect(&pb::InspectRequest { document: Some(broken), ..Default::default() }).unwrap();
        assert!(!resp.ok);
        assert_eq!(resp.bodies.len(), 1, "what was built before the error is still inspected");
        assert_eq!(resp.features[0].feature_id, "");
        assert!(resp.features[0].diagnostics.iter().any(|d| d.severity() == pb::Severity::Error));
    }
}
