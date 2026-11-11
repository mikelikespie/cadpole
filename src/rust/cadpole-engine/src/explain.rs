//! `ExplainRef`: turning "what is this thing I clicked?" into an answer that
//! points at a line of script.
//!
//! `GetSelection` already says *which* feature owns a reference. That is not
//! enough to act on: the face a user clicks on a subtracted part was made by
//! an extrude, combined by a boolean, perhaps repeated by a pattern, and the
//! edit they want is in the sketch at the bottom of that stack. This walks the
//! chain and returns every step with its source span, so an assistant can go
//! straight to the call that matters instead of re-reading the whole file.
//!
//! The walk is over `Feature.inputs`, which is what the evaluator records.
//! That means the chain is *feature* provenance, not per-face provenance: it
//! says "this face belongs to a body the subtract produced from the extrude
//! of this sketch", not "this particular face came from segment 3 of loop
//! 0". `Generator.loop` / `Generator.seg` are therefore always 0, and
//! `reason` says when a chain cannot be completed.

use std::collections::{HashMap, HashSet};

use cadpole_core::FeatureId;
use cadpole_core::context::Span;
use cadpole_proto::v1 as pb;

/// Feature kinds that combine several bodies into one: an input of these
/// contributed its geometry to the result rather than passing through.
const BOOLEAN_KINDS: &[&str] = &["union", "subtract", "difference", "intersect", "intersection", "cut", "combine"];

/// Feature kinds that repeat a body. An input of these was copied.
const PATTERN_KINDS: &[&str] = &["pattern", "mirror", "array", "linear_pattern", "circular_pattern", "grid", "copy", "repeat"];

fn role_for(consumer_kind: &str) -> &'static str {
    let k = consumer_kind.to_ascii_lowercase();
    if BOOLEAN_KINDS.contains(&k.as_str()) {
        "merged"
    } else if PATTERN_KINDS.contains(&k.as_str()) {
        "copied"
    } else {
        "kept"
    }
}

/// The entity kind a topological name implies, so a caller can pass just
/// `feature_id` + `topo_name` and still get the right answer.
/// What kind of entity a topological name refers to, when the caller did not
/// say.
///
/// Generated names carry it in their shape: an edge is the
/// two faces it bounds joined by `|`, a vertex is three. The `face:N` forms
/// are the compatibility spelling. Anything else with a `/` is a face, which
/// is the common case an assistant sends.
fn infer_kind(kind: &str, topo_name: &str) -> pb::EntityKind {
    if !kind.is_empty() {
        return crate::kind_from_name(kind);
    }
    if topo_name.is_empty() {
        return pb::EntityKind::Feature;
    }
    match topo_name.matches('|').count() {
        0 => match topo_name.split(':').next().unwrap_or("") {
            "face" => pb::EntityKind::Face,
            "edge" => pb::EntityKind::Edge,
            "vertex" => pb::EntityKind::Vertex,
            // A generated face name: `base/end`, `blk/top`, `ex_1/side[0.2]`.
            _ if topo_name.contains('/') => pb::EntityKind::Face,
            _ => pb::EntityKind::Unspecified,
        },
        1 => pb::EntityKind::Edge,
        _ => pb::EntityKind::Vertex,
    }
}

fn span_text(s: &Option<pb::SourceSpan>) -> String {
    match s {
        Some(s) => format!("{}:{}:{}", s.path, s.start_line, s.start_col),
        None => "an unknown place".to_owned(),
    }
}

fn provenance(f: &cadpole_core::context::Feature, role: &str) -> pb::Provenance {
    pb::Provenance {
        feature_id: f.id.0.clone(),
        kind: f.kind.clone(),
        role: role.to_owned(),
        span: f.span.as_ref().map(Span::to_proto),
        inputs: f.inputs.iter().map(|i| i.0.clone()).collect(),
        args: f.args.clone().into_iter().collect(),
        name: f.name.clone(),
    }
}

/// The reference a request is about: what it asked for, or the session's first
/// selected entity when it asked for nothing.
fn requested_ref(doc: &pb::Document, req: &pb::ExplainRefRequest) -> Option<pb::EntityRef> {
    if !req.feature_id.is_empty() {
        return Some(pb::EntityRef {
            feature_id: req.feature_id.clone(),
            body_id: req.body_id.clone(),
            kind: infer_kind(&req.kind, &req.topo_name) as i32,
            topo_name: req.topo_name.clone(),
                fingerprint: None,
            });
    }
    doc.selection.as_ref().and_then(|s| s.selected.first().cloned())
}

/// Explain one entity reference: what it is, what made it, and where in the
/// script that happened.
pub fn explain_ref(doc: &pb::Document, req: &pb::ExplainRefRequest) -> pb::ExplainRefResponse {
    let Some(r) = requested_ref(doc, req) else {
        return pb::ExplainRefResponse {
            status: pb::RefStatus::Unspecified as i32,
            reason: "no reference given and nothing is selected in the session".to_owned(),
            summary: "Nothing to explain: pass feature_id (and topo_name for a face or an edge), \
                      or select something in the UI first."
                .to_owned(),
            ..Default::default()
        };
    };

    let (mut ctx, _) = crate::evaluate_document(doc, &HashMap::new(), req.tolerance);
    let mut out = pb::ExplainRefResponse { r#ref: Some(r.clone()), ..Default::default() };

    let Ok(head) = ctx.feature(&FeatureId(r.feature_id.clone())).map(Clone::clone) else {
        out.status = pb::RefStatus::Stale as i32;
        out.reason = format!("feature {:?} is not in the current model", r.feature_id);
        let known: Vec<&str> = ctx.features.iter().map(|f| f.id.0.as_str()).take(20).collect();
        out.summary = format!(
            "No feature named {:?} exists in this document any more. \
             The features it does have are: {}.",
            r.feature_id,
            if known.is_empty() { "none".to_owned() } else { known.join(", ") }
        );
        return out;
    };
    out.status = pb::RefStatus::Ok as i32;

    // ---- the lineage: the principal path, newest first.
    //
    // A feature's first input is its base (the body a boolean cuts into, the
    // profile an extrude sweeps), so following it reaches the sketch that
    // started the shape. Everything else reachable is a contributor: the tool
    // bodies, the things merged in along the way.
    let by_id: HashMap<&str, &cadpole_core::context::Feature> = ctx.features.iter().map(|f| (f.id.0.as_str(), f)).collect();
    let mut chain = vec![provenance(&head, "new")];
    let mut on_path: HashSet<String> = HashSet::from([head.id.0.clone()]);
    let mut cur = &head;
    // Bounded: a malformed cycle in `inputs` must not hang the server, and no
    // real feature tree is 256 deep.
    for _ in 0..256 {
        let Some(next_id) = cur.inputs.first() else { break };
        let Some(next) = by_id.get(next_id.0.as_str()).copied() else {
            out.reason = format!("the chain stops at {:?}, whose input {:?} is not in the model", cur.id.0, next_id.0);
            break;
        };
        if !on_path.insert(next.id.0.clone()) {
            out.reason = format!("the chain loops back to {:?}; stopping there", next.id.0);
            break;
        }
        chain.push(provenance(next, role_for(&cur.kind)));
        cur = next;
    }

    // Contributors: everything else the chain consumes, breadth first from the
    // head, in the order found, so the first one named is the nearest tool.
    let mut contributors = Vec::new();
    let mut seen: HashSet<String> = on_path.clone();
    let mut queue: Vec<String> = chain.iter().flat_map(|p| p.inputs.clone()).collect();
    while let Some(id) = queue.first().cloned() {
        queue.remove(0);
        if !seen.insert(id.clone()) {
            continue;
        }
        if let Some(f) = by_id.get(id.as_str()) {
            contributors.push(id.clone());
            queue.extend(f.inputs.iter().map(|i| i.0.clone()));
        }
    }
    out.contributors = contributors;

    // The generator: the sketch the principal path bottoms out at.
    if let Some(p) = chain.iter().rev().find(|p| p.kind == "sketch") {
        out.generator = Some(pb::Generator {
            feature_id: p.feature_id.clone(),
            local_name: p.name.clone(),
            sketch_id: p.feature_id.clone(),
            // Feature provenance only: no loop or segment is known.
            r#loop: 0,
            seg: 0,
            span: p.span.clone(),
        });
    }

    // ---- geometry, resolved the same way GetSelection does it.
    let kind = crate::kind_name(r.kind()).to_owned();
    let body = ctx
        .bodies
        .iter()
        .find(|b| b.id_string() == r.body_id || (r.body_id.is_empty() && b.feature.0 == r.feature_id))
        .map(|b| (b.id, b.id_string()));
    if let Some((id, body_id)) = body {
        if out.r#ref.as_ref().map(|x| x.body_id.is_empty()).unwrap_or(false) {
            if let Some(x) = out.r#ref.as_mut() {
                x.body_id = body_id;
            }
        }
        match ctx.body_mesh(id) {
            Ok((_, topo)) => match kind.as_str() {
                "face" => match topo.faces.iter().find(|f| f.name == r.topo_name) {
                    Some(f) => {
                        out.face = Some(pb::FaceGeometry {
                            surface_kind: f.surface_kind.to_owned(),
                            centroid: Some(f.centroid.to_proto()),
                            normal: Some(f.normal.to_proto()),
                            area: f.area,
                            edges: f.edges.clone(),
                        })
                    }
                    None => {
                        out.status = pb::RefStatus::Moved as i32;
                        out.reason = format!(
                            "the body has no face named {:?} any more; face:N names are kernel-order \
                             and change when the sketch or a boolean changes",
                            r.topo_name
                        );
                    }
                },
                "edge" => match topo.edges.iter().find(|e| e.name == r.topo_name) {
                    Some(e) => {
                        out.edge = Some(pb::EdgeGeometry {
                            curve_kind: e.curve_kind.to_owned(),
                            start: Some(e.start.to_proto()),
                            end: Some(e.end.to_proto()),
                            length: e.length,
                            faces: e.faces.clone(),
                        })
                    }
                    None => {
                        out.status = pb::RefStatus::Moved as i32;
                        out.reason = format!("the body has no edge named {:?} any more", r.topo_name);
                    }
                },
                _ => {}
            },
            Err(e) => {
                out.status = pb::RefStatus::Moved as i32;
                out.reason = format!("the body could not be tessellated: {e}");
            }
        }
    } else if !r.body_id.is_empty() {
        out.status = pb::RefStatus::Moved as i32;
        out.reason = format!("body {:?} is not in the current model", r.body_id);
    }

    out.summary = summary(&out, &chain, &kind, &r);
    out.chain = chain;
    out
}

/// The one-paragraph version. Everything in here is also a structured field;
/// this exists because a sentence is what a person (or a model composing a
/// reply) actually wants to read.
fn summary(out: &pb::ExplainRefResponse, chain: &[pb::Provenance], kind: &str, r: &pb::EntityRef) -> String {
    let what = match (&out.face, &out.edge) {
        (Some(f), _) => format!(
            "{} {} is a {} of {:.0} mm², normal ({:.2}, {:.2}, {:.2})",
            kind,
            r.topo_name,
            f.surface_kind,
            f.area,
            f.normal.as_ref().map(|n| n.x).unwrap_or(0.0),
            f.normal.as_ref().map(|n| n.y).unwrap_or(0.0),
            f.normal.as_ref().map(|n| n.z).unwrap_or(0.0),
        ),
        (_, Some(e)) => format!("{} {} is a {} {:.2} mm long", kind, r.topo_name, e.curve_kind, e.length),
        _ if r.topo_name.is_empty() => format!("{kind} {}", r.feature_id),
        _ => format!("{kind} {}", r.topo_name),
    };
    let mut s = match chain.first() {
        Some(p) => format!("{what}. It was made by `{}` ({}) at {}", p.feature_id, p.kind, span_text(&p.span)),
        None => what,
    };
    for p in chain.iter().skip(1) {
        s.push_str(&format!(
            ", {} from `{}` ({}) at {}",
            match p.role.as_str() {
                "merged" => "merged",
                "copied" => "copied",
                _ => "built",
            },
            p.feature_id,
            p.kind,
            span_text(&p.span)
        ));
    }
    s.push('.');
    if !out.contributors.is_empty() {
        s.push_str(&format!(" Also involved: {}.", out.contributors.join(", ")));
    }
    if !out.reason.is_empty() {
        s.push_str(&format!(" Note: {}.", out.reason));
    }
    s.push_str(" Edit the first span to change this entity.");
    s
}

#[cfg(test)]
mod tests {
    use super::*;

    fn doc(src: &str) -> pb::Document {
        crate::document_from_scripts("t", vec![("main.star".into(), src.to_owned())])
    }

    const PART: &str = "\
base = extrude(sketch(XY).rect(40, 20), 10, name=\"base\")
hole = extrude(sketch(XY.offset(-1)).circle(3), 12, name=\"hole\")
part = subtract(base, hole, name=\"part\")
";

    #[test]
    fn a_face_is_traced_back_through_the_boolean_to_the_sketch() {
        let r = explain_ref(
            &doc(PART),
            // A generated name, not `face:0`: this test is about lineage, and
            // index names are a compatibility addressing form that resolves
            // through the fingerprint rather than by name.
            &pb::ExplainRefRequest { feature_id: "part".into(), topo_name: "base/end".into(), ..Default::default() },
        );
        assert_eq!(r.status(), pb::RefStatus::Ok, "{}", r.reason);
        assert!(r.face.is_some(), "a face resolves to geometry: {r:?}");

        let ids: Vec<&str> = r.chain.iter().map(|p| p.feature_id.as_str()).collect();
        assert_eq!(ids.first(), Some(&"part"), "newest first: {ids:?}");
        assert!(ids.contains(&"base"), "the chain walks into the boolean's base: {ids:?}");
        assert!(ids.last().unwrap().starts_with("sketch_"), "it bottoms out at a sketch: {ids:?}");

        assert_eq!(r.chain[0].role, "new");
        assert_eq!(r.chain[1].role, "merged", "a subtract merges its inputs");
        assert!(r.chain.iter().all(|p| p.span.is_some()), "every step carries a span: {:?}", r.chain);

        let g = r.generator.as_ref().expect("a generator");
        assert!(g.feature_id.starts_with("sketch_"));
        assert!(g.span.as_ref().unwrap().start_line >= 1);

        assert!(r.contributors.contains(&"hole".to_owned()), "the tool body is a contributor: {:?}", r.contributors);
        assert!(r.summary.contains("subtract"), "{}", r.summary);
        assert!(r.summary.contains("main.star:"), "the summary names the span: {}", r.summary);
    }

    #[test]
    fn the_kind_is_inferred_from_the_topological_name() {
        let r = explain_ref(
            &doc(PART),
            &pb::ExplainRefRequest { feature_id: "part".into(), topo_name: "edge:0".into(), ..Default::default() },
        );
        assert_eq!(r.r#ref.as_ref().unwrap().kind(), pb::EntityKind::Edge);
        assert!(r.edge.is_some() || r.status() == pb::RefStatus::Moved, "{r:?}");
        assert!(!r.r#ref.as_ref().unwrap().body_id.is_empty(), "the body is filled in");
    }

    #[test]
    fn a_missing_feature_says_what_does_exist() {
        let r = explain_ref(&doc(PART), &pb::ExplainRefRequest { feature_id: "nope".into(), ..Default::default() });
        assert_eq!(r.status(), pb::RefStatus::Stale);
        assert!(r.summary.contains("part"), "the summary lists the features there are: {}", r.summary);
        assert!(r.chain.is_empty());
    }

    #[test]
    fn a_missing_face_has_moved_and_says_why() {
        let r = explain_ref(
            &doc(PART),
            &pb::ExplainRefRequest { feature_id: "part".into(), topo_name: "face:999".into(), ..Default::default() },
        );
        assert_eq!(r.status(), pb::RefStatus::Moved);
        assert!(r.reason.contains("kernel-order"), "the reason teaches why it moved: {}", r.reason);
        // The lineage is still useful even when the name no longer resolves.
        assert!(!r.chain.is_empty());
    }

    #[test]
    fn with_no_reference_it_explains_the_selection() {
        let mut d = doc(PART);
        d.selection = Some(pb::Selection {
            selected: vec![pb::EntityRef {
                feature_id: "part".into(),
                kind: pb::EntityKind::Face as i32,
                topo_name: "base/end".into(),
                ..Default::default()
            }],
            ..Default::default()
        });
        let r = explain_ref(&d, &pb::ExplainRefRequest::default());
        assert_eq!(r.r#ref.as_ref().unwrap().feature_id, "part");
        assert_eq!(r.status(), pb::RefStatus::Ok, "{}", r.reason);

        let empty = explain_ref(&doc(PART), &pb::ExplainRefRequest::default());
        assert!(empty.summary.contains("Nothing to explain"), "{}", empty.summary);
    }
}
