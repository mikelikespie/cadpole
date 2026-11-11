//! Characterisation tests for the RPC encoding.
//!
//! Every assertion here records what the encoding **does**, and each one
//! that records a known gap says so. Those pass on purpose: a change that
//! closes a gap fails its test, so it cannot land unnoticed, and nobody can
//! make one quietly worse.

use cadpole_proto::prost::Message as _;
use cadpole_proto::v1 as pb;
use serde_json::json;

fn to_json(message: &str, bytes: &[u8]) -> serde_json::Value {
    cadpole_ops::proto_to_json(message, bytes, &cadpole_ops::serialize_options()).unwrap()
}

// ---------------------------------------------------------------------------
// An enum value the schema does not list is refused.
// ---------------------------------------------------------------------------

/// `ExportFormat` has no member called `OBJ`; it is `EXPORT_FORMAT_OBJ`. The
/// CLI accepts the short spelling and expands it, so `cadpole op Export --format
/// obj` writes an OBJ. Over JSON the short word must not decode to
/// `EXPORT_FORMAT_UNSPECIFIED` (0) and fall back to STL: `check_enum_values`
/// walks the JSON against the descriptor before decoding, so unknown enum
/// values are refused while unknown *fields* stay lenient.
#[test]
fn an_enum_value_the_schema_does_not_list_is_refused() {
    let good = cadpole_ops::json_to_proto("cadpole.v1.ExportRequest", &json!({"format": "EXPORT_FORMAT_OBJ"})).unwrap();
    assert_eq!(pb::ExportRequest::decode(good.as_slice()).unwrap().format(), pb::ExportFormat::Obj);

    for wrong in ["OBJ", "obj", "GLB", "Stl", "not-a-format"] {
        let e = cadpole_ops::json_to_proto("cadpole.v1.ExportRequest", &json!({ "format": wrong }))
            .expect_err("a value that is not in the enum must not decode to zero")
            .to_string();
        assert!(e.contains("not a value of cadpole.v1.ExportFormat"), "{wrong:?}: {e}");
        assert!(e.contains("EXPORT_FORMAT_OBJ"), "the refusal names what would have worked: {e}");
    }

    // An out-of-range *number* is refused too.
    assert!(cadpole_ops::json_to_proto("cadpole.v1.ExportRequest", &json!({"format": 99})).is_err());
    // A number that is in range is still a perfectly good way to say it.
    let bytes = cadpole_ops::json_to_proto("cadpole.v1.ExportRequest", &json!({"format": 2})).unwrap();
    assert_eq!(pb::ExportRequest::decode(bytes.as_slice()).unwrap().format(), pb::ExportFormat::Obj);
}

/// ...and the published schema lists `_UNSPECIFIED` among the values an
/// assistant may pick, without saying what it means.
#[test]
fn the_schema_offers_the_unspecified_enum_member() {
    let s = cadpole_ops::json_schema("ExportRequest").unwrap();
    let values: Vec<&str> = s["properties"]["format"]["enum"].as_array().unwrap().iter().map(|v| v.as_str().unwrap()).collect();
    assert_eq!(values[0], "EXPORT_FORMAT_UNSPECIFIED", "known gap: the schema offers the zero value as a choice");
    assert!(values.contains(&"EXPORT_FORMAT_OBJ"));
    assert!(!values.contains(&"OBJ"), "the short spelling the CLI accepts is not in the schema");
}

// ---------------------------------------------------------------------------
// A required field is enforced wherever proto3 can see it
// ---------------------------------------------------------------------------

struct NoHost;
impl cadpole_ops::Host for NoHost {}

/// `(cadpole.field).required` becomes JSON Schema `required` and `cadpole op` turns
/// it into a clap `.required(true)`. `check_required` also runs in `handle`,
/// which is behind every front door, so the CLI, MCP and HTTP give the same
/// answer for protobuf and JSON. Note where it stops: proto3 gives a
/// plain number no presence, so `SetScript.base_revision` cannot be told from
/// a caller that meant zero, and the only thing that catches it is the
/// revision check itself (a session starts at 1, so zero is never current).
#[test]
fn a_required_field_is_enforced_wherever_proto3_can_see_it() {
    for (op, field) in [("Reply", "text"), ("ReportClientFailure", "kind"), ("AddAsset", "name")] {
        let schema = cadpole_ops::input_schema(op).unwrap();
        let required: Vec<&str> =
            schema["required"].as_array().unwrap().iter().map(|v| v.as_str().unwrap()).collect();
        assert!(required.contains(&field), "{op}: the schema says {field} is required");
        let e = cadpole_ops::call_json(op, &json!({}), &NoHost).unwrap_err().to_string();
        assert!(e.contains(&format!("needs `{field}`")), "{op} must name the field it wants: {e}");
    }

    // The decoder still decodes: it is a decoder, and the call is where a
    // request is judged. Both front doors reach that judgement.
    cadpole_ops::json_to_proto("cadpole.v1.ReplyRequest", &json!({})).expect("decoding an empty message is not the check");

    // What proto3 hides. `base_revision` is `required` and arrives as 0
    // whether the caller said nothing or said zero, so this gets past
    // `check_required` and is caught one layer down by the revision itself.
    let e = cadpole_ops::call_json("SetScript", &json!({"source": "b = box(1,1,1)\n"}), &NoHost).unwrap_err().to_string();
    assert!(e.contains("cadpole serve"), "a missing base_revision is still only a zero here: {e}");
}

// ---------------------------------------------------------------------------
// `skip_default_fields` erases exactly the alarming value of a bool
// ---------------------------------------------------------------------------

/// `serialize_options()` skips default fields, so a `bool` is present in the
/// JSON only when it is `true`. Every "everything is fine" flag in
/// `PrintCheckResponse` is therefore *absent* when it is not fine.
#[test]
fn a_false_boolean_is_absent_from_the_json_and_a_true_one_is_present() {
    let bad = pb::PrintCheckResponse {
        ok: false,
        bodies: vec![pb::BodyPrintReport {
            body_id: "b#0".into(),
            ok: false,
            watertight: false,
            fits_bed: false,
            inverted_normals: true,
            ..Default::default()
        }],
        ..Default::default()
    };
    let v = to_json("cadpole.v1.PrintCheckResponse", &bad.encode_to_vec());
    let body = &v["bodies"][0];
    for missing in ["ok", "watertight", "fits_bed", "fits_bed_rotated"] {
        assert!(
            body.get(missing).is_none(),
            "known gap: {missing} is false, so it is missing from the JSON rather than `false`: {body}"
        );
    }
    assert!(v.get("ok").is_none(), "known gap: the whole check failed, so `ok` is absent: {v}");
    // The one that *is* alarming when true survives, which is what makes the
    // absence read as "not applicable" rather than "no".
    assert_eq!(body["inverted_normals"], json!(true));

    let good = pb::PrintCheckResponse {
        ok: true,
        bodies: vec![pb::BodyPrintReport { body_id: "b#0".into(), ok: true, watertight: true, fits_bed: true, ..Default::default() }],
        ..Default::default()
    };
    let v = to_json("cadpole.v1.PrintCheckResponse", &good.encode_to_vec());
    assert_eq!(v["ok"], json!(true));
    assert_eq!(v["bodies"][0]["watertight"], json!(true));
}

/// The same erasure hides `REF_STATUS_UNSPECIFIED`, so a resolved reference
/// the engine could not classify comes back with no `status` field at all.
#[test]
fn an_unspecified_ref_status_vanishes_from_the_json() {
    let r = pb::GetSelectionResponse {
        refs: vec![pb::ResolvedRef { status: pb::RefStatus::Unspecified as i32, ..Default::default() }],
        ..Default::default()
    };
    let v = to_json("cadpole.v1.GetSelectionResponse", &r.encode_to_vec());
    assert!(v["refs"][0].get("status").is_none(), "known gap: {v}");

    let r = pb::GetSelectionResponse {
        refs: vec![pb::ResolvedRef { status: pb::RefStatus::Stale as i32, ..Default::default() }],
        ..Default::default()
    };
    let v = to_json("cadpole.v1.GetSelectionResponse", &r.encode_to_vec());
    assert_eq!(v["refs"][0]["status"], json!("REF_STATUS_STALE"));
}

// ---------------------------------------------------------------------------
// `Material.roughness` has no presence, and 0 is a mirror
// ---------------------------------------------------------------------------

/// docs/starlark-api.md: "`roughness` ... 0 is mirror-smooth, 1 is chalk", and
/// the engine's own default is 0.6. `Material.roughness` is a plain `double`,
/// so a deliberate mirror and a material that never named a roughness are the
/// same bytes — and both web (`wire.ts` `materialFrom`) and the old JSON view
/// read the absence as "unset" and substitute their own default.
///
/// `opacity` and `ior` next door are `optional`, which is exactly the fix.
#[test]
fn a_zero_roughness_is_indistinguishable_from_an_unset_one() {
    let mirror = pb::Material { name: "m".into(), roughness: 0.0, metallic: 1.0, ..Default::default() };
    let unset = pb::Material { name: "m".into(), metallic: 1.0, ..Default::default() };
    assert_eq!(mirror.encode_to_vec(), unset.encode_to_vec(), "known gap: the two are the same bytes");

    let v = to_json("cadpole.v1.Material", &mirror.encode_to_vec());
    assert!(v.get("roughness").is_none(), "known gap: a mirror carries no roughness at all: {v}");

    // opacity does have presence, so a fully transparent body survives.
    let clear = pb::Material { name: "m".into(), opacity: Some(0.0), ..Default::default() };
    let v = to_json("cadpole.v1.Material", &clear.encode_to_vec());
    assert_eq!(v["opacity"], json!(0.0), "`optional double opacity` keeps a meaningful zero");
}

// ---------------------------------------------------------------------------
// google.protobuf.Struct is the only shape an Event payload has
// ---------------------------------------------------------------------------

/// `Event.data` is a `Struct`, whose only number is a `double`. `GET
/// /api/events` serialises the same payload as `serde_json::Value` and keeps
/// the integer. So `cadpole.v1.Event`'s claim that it "mirrors the JSON on the
/// SSE stream exactly, so `cadpole listen` prints the same lines whether it polls
/// or streams" does not hold: the poll prints `9.0` where the stream prints
/// `9`, and a `u64` past 2^53 comes back changed.
#[test]
fn an_event_payload_loses_integers_on_the_way_through_a_struct() {
    let raw = json!({"path": "main.star", "revision": 9u64, "uptime_ms": 9_007_199_254_740_993u64});
    let round_tripped = cadpole_ops::struct_to_json(&cadpole_ops::json_to_struct(&raw));

    assert_eq!(round_tripped["revision"], json!(9.0));
    assert!(round_tripped["revision"].is_f64() && !raw["revision"].is_f64(), "known gap: the integer became a float");
    assert_eq!(
        round_tripped["uptime_ms"].as_f64().unwrap() as u64,
        9_007_199_254_740_992,
        "known gap: 2^53+1 came back as 2^53"
    );
    assert_ne!(round_tripped, raw, "known gap: the payload did not survive the round trip");
}

// ---------------------------------------------------------------------------
// bulk `bytes` responses are not marked hidden
// ---------------------------------------------------------------------------

/// `(cadpole.field).hidden` exists for "bulk data an assistant should not be made
/// to read" and is used on `RenderResponse.model`. It is not used on the four
/// response fields that are always bulk, so `cadpole mcp`'s `export` hands the
/// whole mesh over as one base64 text block (2.2 MB for a tube, measured).
#[test]
fn the_bulk_bytes_responses_are_not_hidden_from_the_json_front_ends() {
    let hidden = |message: &str, field: &str| {
        let md = cadpole_ops::pool().get_message_by_name(message).unwrap();
        cadpole_ops::field_opts(&md.get_field_by_name(field).unwrap()).hidden
    };
    // The one that is done right.
    assert!(hidden("cadpole.v1.RenderResponse", "model"));
    // The ones that are not.
    for (message, field) in [
        ("cadpole.v1.ExportResponse", "data"),
        ("cadpole.v1.ExportFile", "data"),
        ("cadpole.v1.DrawingResponse", "data"),
        ("cadpole.v1.GetAssetResponse", "data"),
    ] {
        assert!(!hidden(message, field), "the gap would be closed if {message}.{field} were hidden");
    }
    // ...and `Drawing` has no `image_field`, so nothing diverts its bytes to
    // a file the way `Render` does. `Export`'s image field is the plate
    // contact sheet of a Bambu project, a picture; the mesh
    // in `data` is still handed over as base64 text.
    assert_eq!(cadpole_ops::method("Export").unwrap().image_field, "plate_sheet");
    assert_eq!(cadpole_ops::method("Drawing").unwrap().image_field, "");
    assert_eq!(cadpole_ops::method("Render").unwrap().image_field, "png");
}

// ---------------------------------------------------------------------------
// a zero component of a Vec3 is not written
// ---------------------------------------------------------------------------

/// A face normal of (0, 0, 1) serialises as `{"z": 1.0}`. Every reader in the
/// tree defaults a missing component to 0, so this is safe today; it is
/// recorded because it is the shape an assistant sees and it makes
/// `"centroid": {}` (the origin) look like a missing value.
#[test]
fn a_vec3_drops_its_zero_components() {
    let v = to_json("cadpole.v1.Vec3", &pb::Vec3 { x: 0.0, y: 0.0, z: 1.0 }.encode_to_vec());
    assert_eq!(v, json!({"z": 1.0}));
    let origin = to_json("cadpole.v1.Vec3", &pb::Vec3::default().encode_to_vec());
    assert_eq!(origin, json!({}), "the origin is an empty object");
}

// ---------------------------------------------------------------------------
// `WaitEvents.timeout_ms` cannot say "do not block", and says so
// ---------------------------------------------------------------------------

/// The server reads `timeout_ms` 0 as 30 000 ms, and the wire cannot tell
/// "use the default" from "do not block", because proto3 gives a `uint32` no
/// presence. The hand-written `GET /api/events/poll` route can only because
/// serde hands it an `Option<u64>`. The field's comment has to state the
/// limitation; this test holds the comment to that, because the encoding
/// cannot.
#[test]
fn the_wait_events_timeout_says_it_cannot_express_do_not_block() {
    let doc = {
        let md = cadpole_ops::pool().get_message_by_name("cadpole.v1.WaitEventsRequest").unwrap();
        cadpole_ops::field_doc(&md.get_field_by_name("timeout_ms").unwrap())
    };
    assert!(doc.contains("0 = 30000"), "{doc}");
    assert!(
        doc.contains("no way to say") && !doc.contains("without blocking"),
        "the comment must own the limitation rather than promise past it: {doc}"
    );

    // Nothing distinguishes the two intents on the wire, which is why.
    let dont_block = pb::WaitEventsRequest { after: 5, timeout_ms: 0, ..Default::default() };
    let use_default = pb::WaitEventsRequest { after: 5, ..Default::default() };
    assert_eq!(dont_block.encode_to_vec(), use_default.encode_to_vec());
}

// ---------------------------------------------------------------------------
// Every entity kind the proto has, every client knows.
// ---------------------------------------------------------------------------

/// The web and Swift clients index entity kinds by number (`normalize.ts`,
/// `RemoteEngine.swift`'s `EntityKindNames`), and a kind a client's table
/// lacks reads as a *different valid kind*, which nothing downstream can
/// notice. `//spec:names_test` diffs the clients' tables against the enum;
/// this keeps the enum's own shape pinned.
#[test]
fn every_entity_kind_is_numbered_as_the_clients_index_them() {
    let e = cadpole_ops::pool().get_enum_by_name("cadpole.v1.EntityKind").unwrap();
    let names: Vec<String> = e.values().map(|v| v.name().to_owned()).collect();
    assert_eq!(names.last().unwrap(), "ENTITY_KIND_ASSET");
    assert_eq!(e.get_value_by_name("ENTITY_KIND_ASSET").unwrap().number(), 9);
    // The clients index by number, so the values have to be dense from 0.
    for (i, v) in e.values().enumerate() {
        assert_eq!(v.number() as usize, i, "{} is out of order; the clients index by position", v.name());
    }
}

// ---------------------------------------------------------------------------
// Things that are fine, pinned so they stay fine
// ---------------------------------------------------------------------------

#[test]
fn a_script_error_is_a_successful_response_with_diagnostics() {
    let doc = cadpole_engine::document_from_scripts("t", vec![("main.star".into(), "b = box(".into())]);
    let out = cadpole_ops::call_json("Evaluate", &json!({"skip_mesh": true}), &cadpole_ops::DocumentHost(doc)).unwrap();
    let diags = out["model"]["diagnostics"].as_array().expect("diagnostics, not an Err");
    assert_eq!(diags[0]["severity"], "SEVERITY_ERROR");
    assert!(diags[0]["span"]["path"] == "main.star", "{out}");
}

#[test]
fn sixty_four_bit_fields_accept_both_json_shapes_and_come_back_as_numbers() {
    for shape in [json!({"cursor": 42}), json!({"cursor": "42"})] {
        let bytes = cadpole_ops::json_to_proto("cadpole.v1.AckRequest", &shape).unwrap();
        assert_eq!(pb::AckRequest::decode(bytes.as_slice()).unwrap().cursor, 42);
    }
    let out = to_json("cadpole.v1.AckResponse", &pb::AckResponse { cursor: 42, ..Default::default() }.encode_to_vec());
    assert_eq!(out["cursor"], json!(42), "Cadpole deliberately emits numbers, not proto3 JSON's strings");
}

#[test]
fn bytes_accept_standard_and_url_safe_base64() {
    for b64 in ["+/8=", "-_8="] {
        let bytes = cadpole_ops::json_to_proto("cadpole.v1.AddAssetRequest", &json!({"name": "x", "data": b64})).unwrap();
        assert_eq!(pb::AddAssetRequest::decode(bytes.as_slice()).unwrap().data, vec![0xfb, 0xff]);
    }
}

#[test]
fn an_unknown_request_field_is_ignored_rather_than_refused() {
    // Deliberate (`deny_unknown_fields(false)`), and the reason a *misspelled*
    // field silently takes the default instead of being reported.
    let bytes = cadpole_ops::json_to_proto("cadpole.v1.RenderRequest", &json!({"widht": 999, "height": 64})).unwrap();
    let r = pb::RenderRequest::decode(bytes.as_slice()).unwrap();
    assert_eq!((r.width, r.height), (0, 64), "the typo is gone and `width` is 0, i.e. the 800 default");
}
