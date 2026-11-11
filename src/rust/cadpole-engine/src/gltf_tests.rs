//! GlTF out, and the whole-document export's layers, on the cabin
//! example (drawn in feet, a hidden layer) and on small scripts.
//!
//! Every file is checked by [`validate`], a structural validator written
//! here against the glTF 2.0 specification -- the GLB container, the JSON,
//! the buffer, every buffer view and accessor in bounds, `min`/`max` equal
//! to the data, indices inside their vertex count, unit normals, the node
//! graph a forest the scene reaches -- and then read back by the glTF
//! importer, which is a second implementation of the same format.

use super::*;
use crate::{document_from_scripts, export};
use serde_json::Value;

fn examples() -> std::path::PathBuf {
    std::path::PathBuf::from(std::env::var("CADPOLE_EXAMPLES").unwrap_or_else(|_| "examples".into()))
}

fn cabin() -> pb::Document {
    let dir = examples();
    let site = std::fs::read_to_string(dir.join("cabin_site.star")).expect("examples/cabin_site.star");
    let parts = std::fs::read_to_string(dir.join("cabin_parts.star")).expect("examples/cabin_parts.star");
    document_from_scripts("cabin_site", vec![("main.star".into(), site), ("cabin_parts.star".into(), parts)])
}

fn ask(doc: pb::Document, format: pb::ExportFormat, f: impl FnOnce(&mut pb::ExportRequest)) -> pb::ExportResponse {
    let mut req = pb::ExportRequest { document: Some(doc), format: format as i32, ..Default::default() };
    f(&mut req);
    let resp = export(&req).expect("export");
    let errors: Vec<&str> =
        resp.diagnostics.iter().filter(|d| d.severity() == pb::Severity::Error).map(|d| d.message.as_str()).collect();
    assert!(errors.is_empty(), "{errors:?}");
    resp
}

// ------------------------------------------------------------ the validator

fn u32_at(b: &[u8], at: usize) -> u32 {
    u32::from_le_bytes(b[at..at + 4].try_into().unwrap())
}

fn base64_decode(s: &str) -> Vec<u8> {
    let val = |c: u8| -> u32 {
        match c {
            b'A'..=b'Z' => u32::from(c - b'A'),
            b'a'..=b'z' => u32::from(c - b'a') + 26,
            b'0'..=b'9' => u32::from(c - b'0') + 52,
            b'+' => 62,
            b'/' => 63,
            _ => panic!("not base64: {c}"),
        }
    };
    let bytes: Vec<u8> = s.bytes().filter(|c| *c != b'=').collect();
    let mut out = Vec::with_capacity(bytes.len() * 3 / 4);
    for chunk in bytes.chunks(4) {
        let mut n = 0u32;
        for (i, c) in chunk.iter().enumerate() {
            n |= val(*c) << (18 - 6 * i);
        }
        for i in 0..chunk.len() - 1 {
            out.push((n >> (16 - 8 * i)) as u8);
        }
    }
    out
}

fn arr<'a>(v: &'a Value, key: &str) -> &'a [Value] {
    v.get(key).and_then(Value::as_array).map(Vec::as_slice).unwrap_or(&[])
}

fn idx(v: &Value, key: &str) -> Option<usize> {
    v.get(key).map(|x| x.as_u64().unwrap_or_else(|| panic!("{key} is not an index: {x}")) as usize)
}

/// Check `bytes` is a structurally valid glTF 2.0 file and return its JSON
/// and binary buffer. Panics naming the first thing wrong.
pub(crate) fn validate(bytes: &[u8]) -> (Value, Vec<u8>) {
    let (json, bin): (Value, Vec<u8>) = if bytes.starts_with(b"glTF") {
        assert!(bytes.len() >= 20, "a GLB is at least a header and a chunk header");
        assert_eq!(u32_at(bytes, 4), 2, "GLB version");
        assert_eq!(u32_at(bytes, 8) as usize, bytes.len(), "the header's length is the file's");
        assert_eq!(bytes.len() % 4, 0, "a GLB is four-byte aligned");
        let jl = u32_at(bytes, 12) as usize;
        assert_eq!(u32_at(bytes, 16), 0x4E4F_534A, "the first chunk is JSON");
        assert_eq!(jl % 4, 0, "the JSON chunk is padded to four bytes");
        assert!(20 + jl <= bytes.len(), "the JSON chunk fits");
        let json: Value = serde_json::from_slice(&bytes[20..20 + jl]).expect("the JSON chunk parses");
        let mut bin = Vec::new();
        let at = 20 + jl;
        if at < bytes.len() {
            let bl = u32_at(bytes, at) as usize;
            assert_eq!(u32_at(bytes, at + 4), 0x004E_4942, "the second chunk is BIN");
            assert_eq!(at + 8 + bl, bytes.len(), "the BIN chunk ends the file");
            bin = bytes[at + 8..at + 8 + bl].to_vec();
        }
        (json, bin)
    } else {
        let json: Value = serde_json::from_slice(bytes).expect("a .gltf is JSON");
        let uri = arr(&json, "buffers").first().and_then(|b| b.get("uri")).and_then(Value::as_str).unwrap_or("");
        let prefix = "data:application/octet-stream;base64,";
        let bin = if uri.is_empty() {
            Vec::new()
        } else {
            assert!(uri.starts_with(prefix), "the buffer is inline: {}", &uri[..uri.len().min(60)]);
            base64_decode(&uri[prefix.len()..])
        };
        (json, bin)
    };
    assert_eq!(json["asset"]["version"], "2.0");
    let buffers = arr(&json, "buffers");
    assert!(buffers.len() <= 1, "one buffer");
    if let Some(b) = buffers.first() {
        let n = b["byteLength"].as_u64().unwrap() as usize;
        assert!(n <= bin.len() && bin.len() - n < 4, "byteLength {n} against {} bytes of buffer", bin.len());
    }
    let buffer_len = buffers.first().map(|b| b["byteLength"].as_u64().unwrap() as usize).unwrap_or(0);
    let views = arr(&json, "bufferViews");
    for (i, v) in views.iter().enumerate() {
        assert_eq!(idx(v, "buffer"), Some(0), "bufferViews[{i}].buffer");
        let off = idx(v, "byteOffset").unwrap_or(0);
        let len = idx(v, "byteLength").expect("byteLength");
        assert!(len > 0 && off + len <= buffer_len, "bufferViews[{i}] [{off}, {}) is outside {buffer_len}", off + len);
        assert_eq!(off % 4, 0, "bufferViews[{i}] is aligned");
    }
    let accessors = arr(&json, "accessors");
    let comp = |t: u64| match t {
        5120 | 5121 => 1,
        5122 | 5123 => 2,
        5125 | 5126 => 4,
        other => panic!("componentType {other}"),
    };
    let width = |t: &str| match t {
        "SCALAR" => 1,
        "VEC2" => 2,
        "VEC3" => 3,
        "VEC4" => 4,
        "MAT4" => 16,
        other => panic!("type {other}"),
    };
    // Each accessor's values as f64, read straight from the buffer.
    let mut values: Vec<Vec<f64>> = Vec::new();
    for (i, a) in accessors.iter().enumerate() {
        let vi = idx(a, "bufferView").expect("no sparse accessors here");
        let v = &views[vi];
        let ct = a["componentType"].as_u64().unwrap();
        let cs = comp(ct);
        let w = width(a["type"].as_str().unwrap());
        let count = a["count"].as_u64().unwrap() as usize;
        assert!(count > 0, "accessors[{i}].count");
        let off = idx(a, "byteOffset").unwrap_or(0);
        assert_eq!(off % cs, 0, "accessors[{i}] aligned to its component");
        let stride = idx(v, "byteStride").unwrap_or(cs * w);
        let need = off + stride * (count - 1) + cs * w;
        assert!(need <= idx(v, "byteLength").unwrap(), "accessors[{i}] runs past its view");
        let start = idx(v, "byteOffset").unwrap_or(0) + off;
        let mut vals = Vec::with_capacity(count * w);
        for e in 0..count {
            for c in 0..w {
                let at = start + e * stride + c * cs;
                vals.push(match ct {
                    5126 => f64::from(f32::from_le_bytes(bin[at..at + 4].try_into().unwrap())),
                    5125 => f64::from(u32_at(&bin, at)),
                    5123 => f64::from(u16::from_le_bytes(bin[at..at + 2].try_into().unwrap())),
                    _ => f64::from(bin[at]),
                });
            }
        }
        assert!(vals.iter().all(|x| x.is_finite()), "accessors[{i}] holds a NaN or an infinity");
        if let (Some(lo), Some(hi)) = (a.get("min"), a.get("max")) {
            for c in 0..w {
                let col = vals.iter().skip(c).step_by(w);
                let (mn, mx) = col.fold((f64::INFINITY, f64::NEG_INFINITY), |(a, b), x| (a.min(*x), b.max(*x)));
                // Compared as float32, as the specification states them.
                assert_eq!(lo[c].as_f64().unwrap() as f32, mn as f32, "accessors[{i}].min[{c}] is the data's");
                assert_eq!(hi[c].as_f64().unwrap() as f32, mx as f32, "accessors[{i}].max[{c}] is the data's");
            }
        }
        values.push(vals);
    }
    let materials = arr(&json, "materials");
    for (i, m) in materials.iter().enumerate() {
        for c in arr(&m["pbrMetallicRoughness"], "baseColorFactor") {
            let c = c.as_f64().unwrap();
            assert!((0.0..=1.0).contains(&c), "materials[{i}] colour {c}");
        }
    }
    let meshes = arr(&json, "meshes");
    for (mi, m) in meshes.iter().enumerate() {
        for (pi, p) in arr(m, "primitives").iter().enumerate() {
            let attrs = p["attributes"].as_object().unwrap();
            let pos = attrs["POSITION"].as_u64().unwrap() as usize;
            assert!(accessors[pos].get("min").is_some(), "POSITION carries min and max");
            let n = accessors[pos]["count"].as_u64().unwrap();
            for (name, a) in attrs {
                let a = a.as_u64().unwrap() as usize;
                assert!(a < accessors.len(), "meshes[{mi}].primitives[{pi}].{name}");
                assert_eq!(accessors[a]["count"].as_u64().unwrap(), n, "{name} has one value per vertex");
            }
            if let Some(nrm) = attrs.get("NORMAL") {
                for v in values[nrm.as_u64().unwrap() as usize].chunks(3) {
                    let l = (v[0] * v[0] + v[1] * v[1] + v[2] * v[2]).sqrt();
                    assert!((l - 1.0).abs() < 1e-3, "a normal of length {l}");
                }
            }
            let ia = idx(p, "indices").expect("indexed");
            assert_eq!(values[ia].len() % 3, 0, "whole triangles");
            assert!(values[ia].iter().all(|i| (*i as u64) < n), "meshes[{mi}] indexes past its {n} vertices");
            if let Some(mat) = idx(p, "material") {
                assert!(mat < materials.len(), "meshes[{mi}] material");
            }
        }
    }
    let nodes = arr(&json, "nodes");
    let mut parent = vec![None; nodes.len()];
    for (i, n) in nodes.iter().enumerate() {
        if let Some(m) = idx(n, "mesh") {
            assert!(m < meshes.len(), "nodes[{i}].mesh");
        }
        for c in arr(n, "children") {
            let c = c.as_u64().unwrap() as usize;
            assert!(c < nodes.len(), "nodes[{i}] child {c}");
            assert!(parent[c].is_none(), "nodes[{c}] has two parents");
            parent[c] = Some(i);
        }
    }
    let scene = &arr(&json, "scenes")[idx(&json, "scene").unwrap_or(0)];
    let mut seen = vec![false; nodes.len()];
    let mut todo: Vec<usize> = arr(scene, "nodes").iter().map(|n| n.as_u64().unwrap() as usize).collect();
    while let Some(n) = todo.pop() {
        assert!(!seen[n], "nodes[{n}] is reached twice: a cycle");
        seen[n] = true;
        todo.extend(arr(&nodes[n], "children").iter().map(|c| c.as_u64().unwrap() as usize));
    }
    assert!(seen.iter().all(|s| *s), "every node is in the scene");
    (json, bin)
}

/// The node called `name`.
fn node<'a>(json: &'a Value, name: &str) -> Option<&'a Value> {
    arr(json, "nodes").iter().find(|n| n["name"] == name)
}

/// The names of `name`'s children.
fn children(json: &Value, name: &str) -> Vec<String> {
    let nodes = arr(json, "nodes");
    arr(node(json, name).unwrap_or_else(|| panic!("no node {name}")), "children")
        .iter()
        .map(|c| nodes[c.as_u64().unwrap() as usize]["name"].as_str().unwrap().to_owned())
        .collect()
}

/// A body node's POSITION extent, in the file's metres.
fn extent(json: &Value, name: &str) -> [f64; 3] {
    let n = node(json, name).unwrap_or_else(|| panic!("no node {name}"));
    let m = &arr(json, "meshes")[idx(n, "mesh").unwrap()];
    let a = &arr(json, "accessors")[arr(m, "primitives")[0]["attributes"]["POSITION"].as_u64().unwrap() as usize];
    std::array::from_fn(|c| a["max"][c].as_f64().unwrap() - a["min"][c].as_f64().unwrap())
}

// ------------------------------------------------------------------ tests

/// The validator on files from outside the tree, by path: a model that
/// cannot live in the repository is checked the same way. Does nothing
/// unless `CADPOLE_GLTF_VALIDATE` names files (colon-separated):
///
/// ```text
/// bazel test //src/rust/cadpole-engine:cadpole_engine_test --test_arg=validate_the_files \
///     --test_env=CADPOLE_GLTF_VALIDATE=/tmp/house.glb --test_output=all
/// ```
#[test]
fn validate_the_files_cadpole_gltf_validate_names() {
    let Ok(paths) = std::env::var("CADPOLE_GLTF_VALIDATE") else { return };
    for p in paths.split(':').filter(|p| !p.is_empty()) {
        let bytes = std::fs::read(p).unwrap_or_else(|e| panic!("{p}: {e}"));
        let (json, bin) = validate(&bytes);
        let (objects, _) = cadpole_import::gltf::parse(&bytes, p, &cadpole_import::mesh::Resources::default())
            .unwrap_or_else(|e| panic!("{p}: the importer refuses it: {e}"));
        let triangles: usize = objects.iter().map(|o| o.triangles.len()).sum();
        println!(
            "{p}: valid; {} nodes, {} meshes, {} materials, {} accessors, {} bytes of buffer; read back as {} bodies, {triangles} triangles",
            arr(&json, "nodes").len(),
            arr(&json, "meshes").len(),
            arr(&json, "materials").len(),
            arr(&json, "accessors").len(),
            bin.len(),
            objects.len(),
        );
    }
}

#[test]
fn the_cabin_is_a_valid_glb_in_metres_with_its_layers_and_without_the_hidden_shed() {
    let resp = ask(cabin(), pb::ExportFormat::Glb, |r| r.model_unit = "ft".into());
    assert_eq!(resp.suggested_filename, "cabin_site.glb");
    let (json, _) = validate(&resp.data);
    // The root is the document; its children are the layers that hold
    // anything, in declaration order. `existing` is hidden: no node.
    assert_eq!(children(&json, "cabin_site"), ["cabin", "site"]);
    assert_eq!(children(&json, "cabin"), ["cabin_floor", "cabin_south", "cabin_north", "cabin_west", "cabin_east", "cabin_roof"]);
    assert_eq!(children(&json, "site"), ["deck", "lot"]);
    assert!(node(&json, "shed_floor").is_none() && node(&json, "existing").is_none());
    assert_eq!(resp.bodies.len(), 8, "the report lists what was written");
    // The lot is 60 x 50 x 0.1 ft: 18.288 x 0.03048 x 15.24 m, +Y up.
    let e = extent(&json, "lot");
    for (got, want) in e.iter().zip([18.288, 0.03048, 15.24]) {
        assert!((got - want).abs() < 1e-5, "lot extent {e:?}");
    }
    let site = &node(&json, "site").unwrap()["extras"]["cadpole"];
    assert_eq!((site["visible"].as_bool(), site["color"].is_array()), (Some(true), true));
    let lot = &node(&json, "lot").unwrap()["extras"]["cadpole"];
    assert_eq!((lot["layer"].as_str(), lot["hidden"].as_bool()), (Some("site"), Some(false)));
    let units = &json["asset"]["extras"]["cadpole"];
    assert_eq!((units["unit"].as_str(), units["model_unit"].as_str(), units["display_units"].as_str()), (Some("m"), Some("ft"), Some("ft-in")));
    assert_eq!(units["mm_per_model_unit"].as_f64(), Some(304.8));
    // Materials come from the document: the lot's green, linear, as Cadpole
    // keeps it; the walls share one material, written once.
    let mats = arr(&json, "materials");
    let lot_mat = &mats[idx(&arr(&arr(&json, "meshes")[idx(node(&json, "lot").unwrap(), "mesh").unwrap()], "primitives")[0], "material").unwrap()];
    let c = &lot_mat["pbrMetallicRoughness"]["baseColorFactor"];
    let linear = |v: u8| {
        let s = f64::from(v) / 255.0;
        if s <= 0.04045 { s / 12.92 } else { ((s + 0.055) / 1.055).powf(2.4) }
    };
    let want = [0x5d, 0x7a, 0x4a].map(linear);
    for k in 0..3 {
        assert!((c[k].as_f64().unwrap() - want[k]).abs() < 2e-3, "lot colour {c} against {want:?}");
    }
    assert_eq!(lot_mat["pbrMetallicRoughness"]["roughnessFactor"].as_f64(), Some(1.0));
    let wall_mats: std::collections::BTreeSet<usize> = ["cabin_south", "cabin_north", "cabin_west", "cabin_east"]
        .iter()
        .map(|n| idx(&arr(&arr(&json, "meshes")[idx(node(&json, n).unwrap(), "mesh").unwrap()], "primitives")[0], "material").unwrap())
        .collect();
    assert_eq!(wall_mats.len(), 1, "one material for four walls");
    // And the same document writes the same bytes.
    let again = ask(cabin(), pb::ExportFormat::Glb, |r| r.model_unit = "ft".into());
    assert_eq!(resp.data, again.data, "deterministic");
}

#[test]
fn hidden_layers_are_written_when_asked_and_marked_hidden() {
    let resp = ask(cabin(), pb::ExportFormat::Glb, |r| {
        r.model_unit = "ft".into();
        r.include_hidden_layers = true;
    });
    let (json, _) = validate(&resp.data);
    assert_eq!(children(&json, "cabin_site"), ["cabin", "site", "existing"]);
    assert_eq!(children(&json, "existing"), ["shed_floor", "shed_south", "shed_north", "shed_west", "shed_east"]);
    assert_eq!(node(&json, "existing").unwrap()["extras"]["cadpole"]["visible"].as_bool(), Some(false));
    assert_eq!(node(&json, "shed_floor").unwrap()["extras"]["cadpole"]["hidden"].as_bool(), Some(true));
    assert_eq!(node(&json, "lot").unwrap()["extras"]["cadpole"]["hidden"].as_bool(), Some(false));
    assert_eq!(resp.bodies.len(), 13);
}

#[test]
fn a_gltf_is_the_same_scene_as_json_with_its_buffer_inline() {
    let glb = ask(cabin(), pb::ExportFormat::Glb, |r| r.model_unit = "ft".into());
    let text = ask(cabin(), pb::ExportFormat::Gltf, |r| r.model_unit = "ft".into());
    assert_eq!(text.suggested_filename, "cabin_site.gltf");
    let (a, abin) = validate(&glb.data);
    let (b, bbin) = validate(&text.data);
    assert_eq!(abin, bbin, "the same buffer");
    assert_eq!(a["nodes"], b["nodes"]);
    assert_eq!(a["accessors"], b["accessors"]);
    assert_eq!(a["materials"], b["materials"]);
}

#[test]
fn the_glb_reads_back_through_the_importer_where_it_was_drawn() {
    let resp = ask(cabin(), pb::ExportFormat::Glb, |r| r.model_unit = "ft".into());
    let (objects, notes) =
        cadpole_import::gltf::parse(&resp.data, "cabin_site.glb", &cadpole_import::mesh::Resources::default()).expect("the importer reads it");
    assert!(notes.iter().all(|n| !n.message.contains("skipped")), "{:?}", notes.iter().map(|n| &n.message).collect::<Vec<_>>());
    let names: Vec<&str> = objects.iter().map(|o| o.name.as_str()).collect();
    assert_eq!(names.len(), 8, "{names:?}");
    for want in ["cabin_floor", "cabin_roof", "deck", "lot"] {
        assert!(names.contains(&want), "{want} in {names:?}");
    }
    // The importer's frame: file (x, y, z) m -> Cadpole 1000 (x, -z, y) mm; in
    // feet, the lot is where the script put it, (-20, -15, -0.1) to (40, 35, 0).
    let lot = objects.iter().find(|o| o.name == "lot").unwrap();
    let to_ft = |p: &[f64; 3]| [p[0] * 1000.0 / 304.8, -p[2] * 1000.0 / 304.8, p[1] * 1000.0 / 304.8];
    let pts: Vec<[f64; 3]> = lot.positions.iter().map(to_ft).collect();
    let lo: [f64; 3] = std::array::from_fn(|c| pts.iter().map(|p| p[c]).fold(f64::INFINITY, f64::min));
    let hi: [f64; 3] = std::array::from_fn(|c| pts.iter().map(|p| p[c]).fold(f64::NEG_INFINITY, f64::max));
    for (got, want) in lo.iter().chain(&hi).zip([-20.0, -15.0, -0.1, 40.0, 35.0, 0.0]) {
        assert!((got - want).abs() < 1e-4, "lot {lo:?}..{hi:?}");
    }
    let look = lot.appearance.as_ref().expect("its material");
    assert_eq!(look.roughness, 1.0);
}

#[test]
fn a_millimetre_model_is_written_a_thousandth_the_size_and_reads_in_mm() {
    let src = "a = box(1000, 2000, 500, name=\"slab\")\na.material(color=\"#ff0000\", metallic=1.0, roughness=0.25)\n";
    let resp = ask(document_from_scripts("t", vec![("main.star".into(), src.into())]), pb::ExportFormat::Glb, |_| {});
    let (json, _) = validate(&resp.data);
    assert_eq!(extent(&json, "slab"), [1.0, 0.5, 2.0]);
    assert_eq!(json["asset"]["extras"]["cadpole"]["display_units"], "mm");
    assert_eq!(json["asset"]["extras"]["cadpole"]["model_unit"], "mm");
    let m = &arr(&json, "materials")[0]["pbrMetallicRoughness"];
    assert_eq!((m["metallicFactor"].as_f64(), m["roughnessFactor"].as_f64()), (Some(1.0), Some(0.25)));
    assert_eq!(children(&json, "t"), ["slab"], "a body on no layer hangs off the root");
    // `units` overrides what is recorded, and never moves the geometry.
    let resp = ask(document_from_scripts("t", vec![("main.star".into(), src.into())]), pb::ExportFormat::Glb, |r| r.units = "in".into());
    let (json, _) = validate(&resp.data);
    assert_eq!(json["asset"]["extras"]["cadpole"]["display_units"], "in");
    assert_eq!(extent(&json, "slab"), [1.0, 0.5, 2.0]);
}

#[test]
fn model_unit_is_refused_where_it_would_be_ignored() {
    for format in [pb::ExportFormat::Stl, pb::ExportFormat::Obj, pb::ExportFormat::Threemf] {
        let e = export(&pb::ExportRequest {
            document: Some(cabin()),
            format: format as i32,
            model_unit: "ft".into(),
            ..Default::default()
        })
        .unwrap_err()
        .to_string();
        assert!(e.contains("glTF (glb, gltf) only"), "{format:?}: {e}");
    }
    let e = export(&pb::ExportRequest {
        document: Some(cabin()),
        format: pb::ExportFormat::Glb as i32,
        model_unit: "furlong".into(),
        ..Default::default()
    })
    .unwrap_err()
    .to_string();
    assert!(e.contains("mm, cm, m, in or ft"), "{e}");
}

#[test]
fn obj_leaves_out_hidden_layers_and_groups_each_body_by_its_layer() {
    let text = String::from_utf8(ask(cabin(), pb::ExportFormat::Obj, |_| {}).data).unwrap();
    assert!(text.contains("o lot\ng site\n"), "{}", &text[..300.min(text.len())]);
    assert!(text.contains("o cabin_floor\ng cabin\n"));
    assert!(!text.contains("shed_"), "the hidden layer is left out");
    let all = String::from_utf8(ask(cabin(), pb::ExportFormat::Obj, |r| r.include_hidden_layers = true).data).unwrap();
    assert!(all.contains("o shed_floor\n# hidden layer\ng existing\n"));
}

#[test]
fn threemf_leaves_out_hidden_layers_and_names_each_objects_layer() {
    let model = |include: bool| {
        let resp = ask(cabin(), pb::ExportFormat::Threemf, |r| r.include_hidden_layers = include);
        let zip = cadpole_export_3mf::zip::read(&resp.data).expect("a zip");
        zip.text(cadpole_export_3mf::MODEL_PART).expect("the model part")
    };
    let m = model(false);
    assert!(m.contains(&format!("xmlns:cadpole=\"{}\"", cadpole_export_3mf::CADPOLE_NS)));
    assert!(m.contains("name=\"lot\""));
    assert!(m.contains("<metadata name=\"cadpole:layer\">site</metadata>"));
    assert!(!m.contains("shed_"), "the hidden layer is left out");
    let all = model(true);
    assert!(all.contains("name=\"shed_floor\""));
    assert!(all.contains("<metadata name=\"cadpole:layer\">existing</metadata>"));
}

/// `cadpole export --layer NAME` writes that layer's bodies -- a hidden
/// layer too, when it is the one named -- and `--body` adds bodies by id or
/// name; an unknown layer is an error listing the declared ones.
#[test]
fn layer_names_and_body_names_pick_what_an_export_writes() {
    let obj = |f: &dyn Fn(&mut pb::ExportRequest)| {
        String::from_utf8(ask(cabin(), pb::ExportFormat::Obj, |r| f(r)).data).unwrap()
    };
    let objects = |text: &str| -> Vec<String> {
        text.lines().filter_map(|l| l.strip_prefix("o ")).map(str::to_owned).collect()
    };
    let shed = objects(&obj(&|r| r.layer_names = vec!["existing".into()]));
    assert!(!shed.is_empty() && shed.iter().all(|n| n.starts_with("shed_")), "{shed:?}");
    let site = objects(&obj(&|r| r.layer_names = vec!["site".into()]));
    assert!(site.contains(&"lot".to_owned()) && !site.iter().any(|n| n.starts_with("shed_") || n.starts_with("cabin")), "{site:?}");
    let both = objects(&obj(&|r| {
        r.layer_names = vec!["existing".into()];
        r.body_ids = vec!["lot".into()];
    }));
    assert_eq!(both.len(), shed.len() + 1, "{both:?}");
    let e = export(&pb::ExportRequest {
        document: Some(cabin()),
        format: pb::ExportFormat::Obj as i32,
        layer_names: vec!["roofs".into()],
        ..Default::default()
    })
    .unwrap_err()
    .to_string();
    assert!(e.contains("no layer \"roofs\"") && e.contains("cabin, site, existing"), "{e}");
}

#[test]
fn named_bodies_are_written_whatever_their_layer_and_an_all_hidden_document_says_why_it_is_empty() {
    let src = "l = layer(\"ghost\", visible=False)\nl.add(box(10, 10, 10, name=\"a\"))\n";
    let doc = || document_from_scripts("t", vec![("main.star".into(), src.into())]);
    let e = export(&pb::ExportRequest { document: Some(doc()), format: pb::ExportFormat::Stl as i32, ..Default::default() })
        .unwrap_err()
        .to_string();
    assert!(e.contains("all 1 are on hidden layers") && e.contains("include_hidden_layers"), "{e}");
    let id = {
        let r = ask(doc(), pb::ExportFormat::Stl, |r| r.include_hidden_layers = true);
        r.bodies[0].body_id.clone()
    };
    let r = ask(doc(), pb::ExportFormat::Stl, |r| r.body_ids = vec![id.clone()]);
    assert_eq!(r.bodies.len(), 1, "a body asked for by id is written");
}
