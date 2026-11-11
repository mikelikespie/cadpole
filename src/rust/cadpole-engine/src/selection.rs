//! Resolving a stored selection against the current model.
//!
//! Selection references are *names*, not indices, and whether a name is still
//! live is derived on every read rather than remembered: a reference whose
//! feature is removed and later restored (an undo, a rebased assistant write)
//! becomes live again by itself. That is why this is a function of the
//! document and nothing is cached.
//!
//! The ladder, per reference:
//!
//! | step | outcome |
//! | --- | --- |
//! | the feature is not in the model | `stale` |
//! | the name (or an alias, or `face:N`) matches exactly one entity | `ok` |
//! | it matches several — a split face, a group name | `ambiguous`, with the set |
//! | it matches none but the stored fingerprint matches exactly one | `moved` |
//! | otherwise | `stale` |
//!
//! A script's own lookup also takes a short name another generator owns
//! (`top` on a translated box is the box's `top`); a stored reference, which
//! always carries its fingerprint, does not, so the fingerprint decides.
//! Nothing is ever rebound silently, and a stale reference is kept and
//! flagged rather than dropped: dropping loses the user's intent.

use std::collections::HashMap;

use cadpole_core::FeatureId;
use cadpole_core::naming::{self, Fingerprint, Kind, Named, Resolution};
use cadpole_core::tessellate::Topology;
use cadpole_proto::v1 as pb;

use crate::evaluate_document;

/// Resolve one reference against a body's topology, and say in one sentence
/// what happened when it is not a clean hit.
fn resolve_in<T: Named>(
    items: &[T],
    kind: Kind,
    r: &pb::EntityRef,
    diagonal: f64,
) -> (pb::RefStatus, String, Resolution) {
    let fp = r.fingerprint.as_ref().map(Fingerprint::from_proto);
    let res = naming::resolve(items, kind, &r.feature_id, &r.topo_name, fp.as_ref(), diagonal);
    let what = match kind {
        Kind::Face => "face",
        Kind::Edge => "edge",
        Kind::Vertex => "vertex",
    };
    match &res {
        Resolution::Ok(_) => (pb::RefStatus::Ok, String::new(), res),
        Resolution::Moved(i) => (
            pb::RefStatus::Moved,
            format!(
                "no {what} is called {:?} any more; one {what} ({}) still matches the geometry \
                 the selection was made on, so the reference follows it",
                r.topo_name,
                items[*i].name()
            ),
            res,
        ),
        Resolution::Ambiguous(all) => (
            pb::RefStatus::Ambiguous,
            format!(
                "{:?} names {} {what}s ({}); the reference resolves to all of them, pick one",
                r.topo_name,
                all.len(),
                all.iter().map(|i| items[*i].name()).collect::<Vec<_>>().join(", ")
            ),
            res,
        ),
        Resolution::Stale => (
            pb::RefStatus::Stale,
            format!(
                "the body has no {what} named {:?}, and none matches the geometry the selection \
                 was made on{}",
                r.topo_name,
                if r.fingerprint.is_none() {
                    " (the reference carries no fingerprint, so there was nothing to match)"
                } else {
                    ""
                }
            ),
            res,
        ),
    }
}

/// `resolved_name` / `matches` for the reference, once it has been resolved.
fn record<T: Named>(out: &mut pb::ResolvedRef, items: &[T], res: &Resolution) {
    match res {
        Resolution::Ok(i) | Resolution::Moved(i) => out.resolved_name = items[*i].name().to_owned(),
        Resolution::Ambiguous(all) => {
            out.matches = all.iter().map(|i| items[*i].name().to_owned()).collect();
        }
        Resolution::Stale => {}
    }
}

/// The scale the fingerprint's tolerances are relative to.
fn diagonal_of(topo: &Topology) -> f64 {
    topo.diagonal()
}

/// Resolve `doc.selection` against the model `doc` evaluates to.
///
/// With `skip_geometry` the document is not evaluated at all: every reference
/// comes back with `REF_STATUS_UNSPECIFIED` and no geometry, which is enough
/// when the caller only wants the names the user picked.
pub fn resolve_selection(doc: &pb::Document, skip_geometry: bool, tolerance: f64) -> pb::GetSelectionResponse {
    let selection = doc.selection.clone().unwrap_or_default();
    let note = selection.note.clone();
    if skip_geometry {
        let refs = selection.selected.into_iter().map(|r| pb::ResolvedRef { r#ref: Some(r), ..Default::default() }).collect();
        return pb::GetSelectionResponse { refs, note };
    }

    let (mut ctx, _) = evaluate_document(doc, &HashMap::new(), tolerance);
    let mut refs = Vec::new();
    for r in &selection.selected {
        let kind = crate::kind_name(r.kind()).to_owned();
        let mut out = pb::ResolvedRef {
            r#ref: Some(r.clone()),
            status: pb::RefStatus::Stale as i32,
            reason: format!("feature {:?} is not in the current model", r.feature_id),
            ..Default::default()
        };
        if let Ok(f) = ctx.feature(&FeatureId(r.feature_id.clone())) {
            out.feature = Some(pb::FeatureInfo {
                id: f.id.0.clone(),
                kind: f.kind.clone(),
                name: f.name.clone(),
                args: f.args.clone().into_iter().collect(),
                span: f.span.as_ref().map(cadpole_core::context::Span::to_proto),
                inputs: f.inputs.iter().map(|i| i.0.clone()).collect(),
            });
            out.status = pb::RefStatus::Ok as i32;
            out.reason = String::new();
        }
        // The body the reference belongs to: the one it names, or (for a
        // feature-level reference) the first body that feature produced.
        let body = ctx
            .bodies
            .iter()
            .find(|b| b.id_string() == r.body_id || (r.body_id.is_empty() && b.feature.0 == r.feature_id))
            .map(|b| b.id);
        if let Some(id) = body {
            match ctx.body_mesh(id) {
                Ok((mesh, topo)) => {
                    out.bbox = mesh.bbox().map(|(lo, hi)| pb::BBox { min: Some(lo.to_proto()), max: Some(hi.to_proto()) });
                    let diagonal = diagonal_of(topo);
                    match kind.as_str() {
                        "face" => {
                            let (status, reason, res) = resolve_in(&topo.faces, Kind::Face, r, diagonal);
                            out.status = status as i32;
                            out.reason = reason;
                            record(&mut out, &topo.faces, &res);
                            // Geometry for the one entity it landed on; an
                            // ambiguous reference deliberately gets none, so a
                            // client cannot mistake a set for a pick.
                            if let Resolution::Ok(i) | Resolution::Moved(i) = res {
                                let f = &topo.faces[i];
                                out.face = Some(pb::FaceGeometry {
                                    surface_kind: f.surface_kind.to_owned(),
                                    centroid: Some(f.centroid.to_proto()),
                                    normal: Some(f.normal.to_proto()),
                                    area: f.area,
                                    edges: f.edges.clone(),
                                });
                            }
                        }
                        "edge" => {
                            let (status, reason, res) = resolve_in(&topo.edges, Kind::Edge, r, diagonal);
                            out.status = status as i32;
                            out.reason = reason;
                            record(&mut out, &topo.edges, &res);
                            if let Resolution::Ok(i) | Resolution::Moved(i) = res {
                                let e = &topo.edges[i];
                                out.edge = Some(pb::EdgeGeometry {
                                    curve_kind: e.curve_kind.to_owned(),
                                    start: Some(e.start.to_proto()),
                                    end: Some(e.end.to_proto()),
                                    length: e.length,
                                    faces: e.faces.clone(),
                                });
                            }
                        }
                        "vertex" => {
                            let (status, reason, res) = resolve_in(&topo.vertices, Kind::Vertex, r, diagonal);
                            out.status = status as i32;
                            out.reason = reason;
                            record(&mut out, &topo.vertices, &res);
                        }
                        _ => {}
                    }
                }
                Err(e) => {
                    out.status = pb::RefStatus::Moved as i32;
                    out.reason = format!("the body could not be tessellated: {e}");
                }
            }
        } else if !r.body_id.is_empty() && out.status == pb::RefStatus::Ok as i32 {
            out.status = pb::RefStatus::Moved as i32;
            out.reason = format!("body {:?} is not in the current model", r.body_id);
        }
        refs.push(out);
    }
    pb::GetSelectionResponse { refs, note }
}

#[cfg(test)]
mod tests {
    use super::*;

    const BLOCK: &str = "b = box(10, 10, 10, name=\"blk\")";

    fn doc_of(script: &str, refs: Vec<pb::EntityRef>) -> pb::Document {
        let mut d = crate::document_from_scripts("t", vec![("main.star".into(), script.into())]);
        d.selection = Some(pb::Selection { selected: refs, hovered: None, note: "make this thicker".into() });
        d
    }

    fn doc_with(refs: Vec<pb::EntityRef>) -> pb::Document {
        doc_of(BLOCK, refs)
    }

    fn face_ref(feature: &str, name: &str) -> pb::EntityRef {
        pb::EntityRef {
            feature_id: feature.into(),
            kind: pb::EntityKind::Face as i32,
            topo_name: name.into(),
            ..Default::default()
        }
    }

    fn only(d: &pb::Document) -> pb::ResolvedRef {
        let r = resolve_selection(d, false, 0.0);
        assert_eq!(r.refs.len(), 1);
        r.refs[0].clone()
    }

    #[test]
    fn a_live_face_resolves_with_geometry_and_a_span() {
        let r = resolve_selection(&doc_with(vec![face_ref("blk", "blk/top")]), false, 0.0);
        assert_eq!(r.note, "make this thicker");
        assert_eq!(r.refs[0].status(), pb::RefStatus::Ok, "{:?}", r.refs[0]);
        let face = r.refs[0].face.as_ref().expect("face geometry");
        assert!((face.area - 100.0).abs() < 1e-6);
        assert!(face.normal.as_ref().unwrap().z > 0.99);
        assert_eq!(r.refs[0].resolved_name, "blk/top");
        let f = r.refs[0].feature.as_ref().expect("feature");
        assert_eq!(f.kind, "box");
        assert!(f.span.is_some());
    }

    /// Compatibility: `face:N` is kernel output order and not a name, but the
    /// web UI, the MCP tools and existing scripts address with it, so it has
    /// to keep working for a milestone -- and the reply says which real name
    /// it landed on.
    #[test]
    fn the_index_form_still_addresses_and_reports_the_real_name() {
        let r = only(&doc_with(vec![face_ref("blk", "face:0")]));
        assert_eq!(r.status(), pb::RefStatus::Ok, "{r:?}");
        assert!(r.face.is_some());
        assert!(r.resolved_name.starts_with("blk/"), "{}", r.resolved_name);
        // Out of range is stale, not a wrong face.
        let r = only(&doc_with(vec![face_ref("blk", "face:999")]));
        assert_eq!(r.status(), pb::RefStatus::Stale);
        assert!(r.face.is_none());
    }

    /// The local name and the generic alias address the same face as the
    /// meaningful one a primitive gives it.
    #[test]
    fn a_short_local_name_and_an_alias_address_the_same_face() {
        for q in ["top", "blk/top", "blk/end", "end"] {
            let r = only(&doc_with(vec![face_ref("blk", q)]));
            assert_eq!(r.status(), pb::RefStatus::Ok, "{q}: {r:?}");
            assert_eq!(r.resolved_name, "blk/top", "{q}");
        }
    }

    #[test]
    fn a_missing_feature_is_stale() {
        let r = only(&doc_with(vec![face_ref("gone", "gone/top")]));
        assert_eq!(r.status(), pb::RefStatus::Stale);
        assert!(r.reason.contains("gone"));
        assert!(r.face.is_none());
    }

    /// The point of the whole exercise: a selection stored against a script
    /// survives an unrelated feature being inserted *before* the one it names,
    /// which renumbers every kernel face.
    #[test]
    fn a_selection_survives_an_edit_that_renumbers_everything() {
        let before = only(&doc_with(vec![face_ref("blk", "blk/top")]));
        assert_eq!(before.status(), pb::RefStatus::Ok);
        let area = before.face.as_ref().unwrap().area;

        let edited = "a = cylinder(3, 4, name=\"pin\")\nb = box(10, 10, 10, name=\"blk\")\n";
        let after = only(&doc_of(edited, vec![face_ref("blk", "blk/top")]));
        assert_eq!(after.status(), pb::RefStatus::Ok, "{after:?}");
        assert_eq!(after.resolved_name, "blk/top");
        assert!((after.face.as_ref().unwrap().area - area).abs() < 1e-9);

        // The same reference written as a `face:N` index does *not* survive it:
        // the index moved.
        let idx = only(&doc_of(edited, vec![face_ref("blk", "face:0")]));
        assert_eq!(idx.status(), pb::RefStatus::Ok);
        assert_ne!(
            idx.face.as_ref().unwrap().area,
            0.0,
            "an index still resolves to *a* face -- just not necessarily the same one"
        );
    }

    /// A parameter change moves the geometry; the name does not move with it.
    #[test]
    fn a_selection_survives_a_parameter_change() {
        let script = "h = param(\"h\", 10)\nb = box(10, 10, h, name=\"blk\")";
        let mut d = doc_of(script, vec![face_ref("blk", "blk/top")]);
        let thin = only(&d);
        assert_eq!(thin.status(), pb::RefStatus::Ok);
        let z0 = thin.face.as_ref().unwrap().centroid.as_ref().unwrap().z;

        d.param_overrides.insert("h".into(), pb::ParamValue { value: Some(pb::param_value::Value::Number(40.0)) });
        let thick = only(&d);
        assert_eq!(thick.status(), pb::RefStatus::Ok, "{thick:?}");
        assert_eq!(thick.resolved_name, "blk/top");
        let z1 = thick.face.as_ref().unwrap().centroid.as_ref().unwrap().z;
        assert!((z0 - 10.0).abs() < 1e-6 && (z1 - 40.0).abs() < 1e-6, "{z0} -> {z1}");
    }

    /// When the name is gone, the fingerprint is the last rung of the ladder:
    /// exactly one candidate is `moved`, zero or several is `stale`. It never
    /// rebinds silently -- `moved` says so, and says to what.
    #[test]
    fn a_lost_name_is_moved_by_fingerprint_or_stale() {
        let fp = |z: f64, area: f64| pb::EntityFingerprint {
            surface_kind: "plane".into(),
            centroid: Some(pb::Vec3 { x: 5.0, y: 5.0, z }),
            normal: Some(pb::Vec3 { x: 0.0, y: 0.0, z: 1.0 }),
            area,
            length: 0.0,
        };
        let mut r = face_ref("blk", "blk/was_called_this");
        r.fingerprint = Some(fp(10.0, 100.0));
        let moved = only(&doc_with(vec![r.clone()]));
        assert_eq!(moved.status(), pb::RefStatus::Moved, "{moved:?}");
        assert_eq!(moved.resolved_name, "blk/top");
        assert!(moved.face.is_some());
        assert!(moved.reason.contains("blk/top"));

        // Far away: stale, not a wrong match. Under-matching is the point.
        r.fingerprint = Some(fp(400.0, 100.0));
        let stale = only(&doc_with(vec![r.clone()]));
        assert_eq!(stale.status(), pb::RefStatus::Stale, "{stale:?}");
        assert!(stale.face.is_none());

        // No fingerprint at all: stale, and the reason says why.
        r.fingerprint = None;
        let bare = only(&doc_with(vec![r]));
        assert_eq!(bare.status(), pb::RefStatus::Stale);
        assert!(bare.reason.contains("no fingerprint"), "{}", bare.reason);
    }

    /// A group name (a cylinder's wall is two kernel faces) resolves to the
    /// set and is flagged, never to one of them.
    #[test]
    fn a_group_name_is_ambiguous_with_the_set_listed() {
        let d = doc_of(
            "c = cylinder(5, 10, name=\"post\")",
            vec![face_ref("post", "post/side")],
        );
        let r = only(&d);
        assert_eq!(r.status(), pb::RefStatus::Ambiguous, "{r:?}");
        // A circle is two arcs in the sketch, so the wall is two kernel faces:
        // `post/side` is the group and the pieces are `post/side#k`.
        assert_eq!(r.matches.len(), 2, "{:?}", r.matches);
        let mut sorted = r.matches.clone();
        sorted.sort();
        assert_eq!(sorted, vec!["post/side#0".to_owned(), "post/side#1".to_owned()]);
        assert!(r.face.is_none(), "an ambiguous ref gets no geometry: it is a set");
        // Either half of the wall addresses on its own.
        let one = only(&doc_of("c = cylinder(5, 10, name=\"post\")", vec![face_ref("post", &r.matches[0])]));
        assert_eq!(one.status(), pb::RefStatus::Ok);
    }

    /// Edges are named by the faces they bound, so an edge reference survives
    /// the same edits a face reference does.
    #[test]
    fn an_edge_is_named_by_the_faces_it_bounds() {
        let d = doc_with(vec![]);
        let (mut ctx, _) = evaluate_document(&d, &HashMap::new(), 0.0);
        let id = ctx.bodies.iter().find(|b| b.visible).map(|b| b.id).unwrap();
        let names: Vec<String> = ctx.body_mesh(id).unwrap().1.edges.iter().map(|e| e.name.clone()).collect();
        // `blk/front|top`, never `edge:7`: the pair is sorted, and the shared
        // feature prefix is written once.
        assert!(names.iter().all(|n| n.starts_with("blk/") && n.contains('|')), "{names:?}");
        let one = names.iter().find(|n| n.ends_with("|top")).unwrap_or_else(|| panic!("{names:?}"));
        let r = only(&doc_with(vec![pb::EntityRef {
            feature_id: "blk".into(),
            kind: pb::EntityKind::Edge as i32,
            topo_name: one.clone(),
            ..Default::default()
        }]));
        assert_eq!(r.status(), pb::RefStatus::Ok, "{r:?}");
        assert!(r.edge.as_ref().unwrap().length > 0.0);
    }

    #[test]
    fn skip_geometry_returns_the_refs_verbatim() {
        let d = doc_with(vec![pb::EntityRef { feature_id: "blk".into(), kind: pb::EntityKind::Body as i32, ..Default::default() }]);
        let r = resolve_selection(&d, true, 0.0);
        assert_eq!(r.refs.len(), 1);
        assert!(r.refs[0].feature.is_none());
        assert_eq!(r.refs[0].status(), pb::RefStatus::Unspecified);
    }
}
