//! glTF tests: every file is built here, in the test, from a JSON
//! value and a binary buffer, so each case is exactly the bytes it names
//! and no captured scan is ever a fixture.

use std::collections::BTreeMap;
use std::sync::Arc;

use serde_json::{Value, json};

use crate::gltf;
use crate::mesh::{self, Import, Options, Repair, Resources, Unit, Up};
use crate::prescan::{self, Format};

// ---------------------------------------------------------------- a GLB builder

/// Accessors and buffer views over one growing binary buffer.
#[derive(Default)]
struct Builder {
    bin: Vec<u8>,
    views: Vec<Value>,
    accessors: Vec<Value>,
}

impl Builder {
    fn view(&mut self, bytes: &[u8]) -> usize {
        while self.bin.len() % 4 != 0 {
            self.bin.push(0);
        }
        self.views.push(json!({"buffer": 0, "byteOffset": self.bin.len(), "byteLength": bytes.len()}));
        self.bin.extend_from_slice(bytes);
        self.views.len() - 1
    }

    fn accessor(&mut self, view: usize, ctype: u32, count: usize, kind: &str, normalized: bool) -> usize {
        let mut a = json!({"bufferView": view, "componentType": ctype, "count": count, "type": kind});
        if normalized {
            a["normalized"] = json!(true);
        }
        self.accessors.push(a);
        self.accessors.len() - 1
    }

    fn positions(&mut self, ps: &[[f32; 3]]) -> usize {
        let bytes: Vec<u8> = ps.iter().flatten().flat_map(|x| x.to_le_bytes()).collect();
        let v = self.view(&bytes);
        let a = self.accessor(v, 5126, ps.len(), "VEC3", false);
        let lo = (0..3).map(|k| ps.iter().map(|p| p[k]).fold(f32::MAX, f32::min)).collect::<Vec<_>>();
        let hi = (0..3).map(|k| ps.iter().map(|p| p[k]).fold(f32::MIN, f32::max)).collect::<Vec<_>>();
        self.accessors[a]["min"] = json!(lo);
        self.accessors[a]["max"] = json!(hi);
        a
    }

    fn indices_u8(&mut self, idx: &[u32]) -> usize {
        let bytes: Vec<u8> = idx.iter().map(|&i| i as u8).collect();
        let v = self.view(&bytes);
        self.accessor(v, 5121, idx.len(), "SCALAR", false)
    }

    fn indices_u16(&mut self, idx: &[u32]) -> usize {
        let bytes: Vec<u8> = idx.iter().flat_map(|&i| (i as u16).to_le_bytes()).collect();
        let v = self.view(&bytes);
        self.accessor(v, 5123, idx.len(), "SCALAR", false)
    }

    fn indices_u32(&mut self, idx: &[u32]) -> usize {
        let bytes: Vec<u8> = idx.iter().flat_map(|&i| i.to_le_bytes()).collect();
        let v = self.view(&bytes);
        self.accessor(v, 5125, idx.len(), "SCALAR", false)
    }

    fn colors_u8(&mut self, cs: &[[u8; 4]]) -> usize {
        let bytes: Vec<u8> = cs.iter().flatten().copied().collect();
        let v = self.view(&bytes);
        self.accessor(v, 5121, cs.len(), "VEC4", true)
    }

    fn colors_u16(&mut self, cs: &[[u16; 3]]) -> usize {
        let bytes: Vec<u8> = cs.iter().flatten().flat_map(|c| c.to_le_bytes()).collect();
        let v = self.view(&bytes);
        self.accessor(v, 5123, cs.len(), "VEC3", true)
    }

    fn colors_f32(&mut self, cs: &[[f32; 3]]) -> usize {
        let bytes: Vec<u8> = cs.iter().flatten().flat_map(|x| x.to_le_bytes()).collect();
        let v = self.view(&bytes);
        self.accessor(v, 5126, cs.len(), "VEC3", false)
    }

    /// The document: `rest` supplies scenes, nodes, meshes and materials;
    /// the builder adds the accessors, views and the one buffer.
    fn doc(&self, rest: Value) -> Value {
        let mut d = json!({"asset": {"version": "2.0", "generator": "cadpole gltf_tests"}});
        for (k, v) in rest.as_object().unwrap() {
            d[k] = v.clone();
        }
        d["accessors"] = json!(self.accessors);
        d["bufferViews"] = json!(self.views);
        d["buffers"] = json!([{"byteLength": self.bin.len()}]);
        d
    }

    fn glb(&self, rest: Value) -> Vec<u8> {
        glb_of(&self.doc(rest), &self.bin)
    }
}

/// A GLB: header, JSON chunk padded with spaces, BIN chunk padded with zeros.
fn glb_of(doc: &Value, bin: &[u8]) -> Vec<u8> {
    let mut js = serde_json::to_vec(doc).unwrap();
    while js.len() % 4 != 0 {
        js.push(b' ');
    }
    let mut b = bin.to_vec();
    while b.len() % 4 != 0 {
        b.push(0);
    }
    let total = 12 + 8 + js.len() + if b.is_empty() { 0 } else { 8 + b.len() };
    let mut out = Vec::with_capacity(total);
    out.extend_from_slice(b"glTF");
    out.extend_from_slice(&2u32.to_le_bytes());
    out.extend_from_slice(&(total as u32).to_le_bytes());
    out.extend_from_slice(&(js.len() as u32).to_le_bytes());
    out.extend_from_slice(&0x4E4F_534Au32.to_le_bytes());
    out.extend_from_slice(&js);
    if !b.is_empty() {
        out.extend_from_slice(&(b.len() as u32).to_le_bytes());
        out.extend_from_slice(&0x004E_4942u32.to_le_bytes());
        out.extend_from_slice(&b);
    }
    out
}

fn base64(bytes: &[u8]) -> String {
    const A: &[u8; 64] = b"ABCDEFGHIJKLMNOPQRSTUVWXYZabcdefghijklmnopqrstuvwxyz0123456789+/";
    let mut s = String::new();
    for c in bytes.chunks(3) {
        let n = (c[0] as u32) << 16 | (*c.get(1).unwrap_or(&0) as u32) << 8 | *c.get(2).unwrap_or(&0) as u32;
        for k in 0..4 {
            if k <= c.len() {
                s.push(A[(n >> (18 - 6 * k) & 63) as usize] as char);
            } else {
                s.push('=');
            }
        }
    }
    s
}

/// The unit cube [0, 1]³, outward, as glTF writes it (counter-clockwise
/// seen from outside).
fn cube() -> (Vec<[f32; 3]>, Vec<u32>) {
    let p = vec![
        [0.0, 0.0, 0.0],
        [1.0, 0.0, 0.0],
        [1.0, 1.0, 0.0],
        [0.0, 1.0, 0.0],
        [0.0, 0.0, 1.0],
        [1.0, 0.0, 1.0],
        [1.0, 1.0, 1.0],
        [0.0, 1.0, 1.0],
    ];
    let t = vec![
        0, 2, 1, 0, 3, 2, // z = 0, facing -z
        4, 5, 6, 4, 6, 7, // z = 1
        0, 1, 5, 0, 5, 4, // y = 0
        3, 7, 6, 3, 6, 2, // y = 1
        0, 4, 7, 0, 7, 3, // x = 0
        1, 2, 6, 1, 6, 5, // x = 1
    ];
    (p, t)
}

fn opts() -> Options {
    Options { repair: Repair::Report, ..Options::default() }
}

fn import(bytes: &[u8], name: &str, o: &Options) -> Import {
    mesh::import_unbounded(bytes, name, o).unwrap_or_else(|e| panic!("import({name}): {e}"))
}

fn one_node_cube(b: &mut Builder, idx_kind: u8) -> Value {
    let (p, t) = cube();
    let pos = b.positions(&p);
    let ind = match idx_kind {
        8 => Some(b.indices_u8(&t)),
        16 => Some(b.indices_u16(&t)),
        32 => Some(b.indices_u32(&t)),
        _ => None,
    };
    let mut prim = json!({"attributes": {"POSITION": pos}, "mode": 4});
    if let Some(i) = ind {
        prim["indices"] = json!(i);
    }
    json!({"scene": 0, "scenes": [{"nodes": [0]}], "nodes": [{"mesh": 0}], "meshes": [{"primitives": [prim]}]})
}

fn bbox(b: &mesh::ImportedBody) -> ([f64; 3], [f64; 3]) {
    let (lo, hi) = b.mesh.bbox().unwrap();
    ([lo.x, lo.y, lo.z], [hi.x, hi.y, hi.z])
}

fn close3(a: [f64; 3], b: [f64; 3], tol: f64) -> bool {
    (0..3).all(|k| (a[k] - b[k]).abs() <= tol)
}

// ---------------------------------------------------------------- the tests

#[test]
fn the_bytes_sniff_as_gltf_and_the_format_is_named() {
    let mut b = Builder::default();
    let rest = one_node_cube(&mut b, 16);
    let bytes = b.glb(rest.clone());
    assert_eq!(cadpole_assets::sniff("anything.bin", &bytes), cadpole_assets::AssetKind::Gltf);
    assert_eq!(prescan::format_of("x", &bytes).unwrap(), Format::Gltf);
    let json = serde_json::to_vec_pretty(&b.doc(rest)).unwrap();
    assert_eq!(cadpole_assets::sniff("room.gltf", &json), cadpole_assets::AssetKind::Gltf);
    assert_eq!(cadpole_assets::sniff("mystery", &json), cadpole_assets::AssetKind::Gltf, "the bytes decide, not the name");
    assert!(cadpole_assets::AssetKind::Gltf.is_mesh());
}

#[test]
fn a_cube_with_a_node_transform_is_placed_scaled_and_solid() {
    let mut b = Builder::default();
    let mut rest = one_node_cube(&mut b, 16);
    // Scale 2, then translate by (1, 2, 3) metres: the cube spans
    // [1, 3] x [2, 4] x [3, 5] in glTF's frame.
    rest["nodes"][0]["translation"] = json!([1.0, 2.0, 3.0]);
    rest["nodes"][0]["scale"] = json!([2.0, 2.0, 2.0]);
    rest["nodes"][0]["name"] = json!("crate");
    let im = import(&b.glb(rest), "room.glb", &opts());
    assert_eq!(im.format, Format::Gltf);
    assert_eq!(im.bodies.len(), 1);
    let body = &im.bodies[0];
    assert_eq!(body.name, "crate", "the node names the body");
    let (lo, hi) = bbox(body);
    // (x, y, z) -> 1000 (x, -z, y)
    assert!(close3(lo, [1000.0, -5000.0, 2000.0], 1e-3), "{lo:?}");
    assert!(close3(hi, [3000.0, -3000.0, 4000.0], 1e-3), "{hi:?}");
    let r = &body.report;
    assert_eq!(r.format, "glTF");
    assert_eq!(r.triangles, 12);
    assert!(r.solid, "{}", r.summary());
    assert!(r.volume.unwrap() > 0.0, "outward after the axis turn");
    assert!((r.volume.unwrap() - 8.0e9).abs() < 1.0, "2 m cube = 8e9 mm3: {:?}", r.volume);
    assert_eq!(r.object, "crate");
    assert!(!r.vertex_colors);
    assert_eq!(body.topo.faces.len(), 1);
    assert_eq!(body.topo.faces[0].name, "import_mesh/surface");
    assert!(im.notes.iter().any(|n| n.message.contains("(x, y, z) → 1000·(x, −z, y)")), "{:?}", im.notes);
    assert!(im.notes.iter().any(|n| n.message.contains("binary, GLB") && n.message.contains("cadpole gltf_tests")), "{:?}", im.notes);
}

#[test]
fn y_up_turns_to_z_up_and_units_scale_by_coordinates() {
    let mut b = Builder::default();
    let pos = b.positions(&[[1.0, 0.0, 0.0], [0.0, 1.0, 0.0], [0.0, 0.0, 1.0]]);
    let rest = json!({"nodes": [{"mesh": 0}], "meshes": [{"primitives": [{"attributes": {"POSITION": pos}}]}]});
    let bytes = b.glb(rest);
    let got = |o: &Options| -> Vec<[f32; 3]> {
        let im = import(&bytes, "tri.glb", o);
        im.bodies[0].mesh.positions.chunks_exact(3).map(|p| [p[0], p[1], p[2]]).collect()
    };
    // The default: +Y up in metres.
    assert_eq!(got(&opts()), vec![[1000.0, 0.0, 0.0], [0.0, 0.0, 1000.0], [0.0, -1000.0, 0.0]]);
    // The file's own frame, taken as it is.
    let asis = Options { up: Some(Up::Z), units: Some(Unit::Mm), ..opts() };
    assert_eq!(got(&asis), vec![[1.0, 0.0, 0.0], [0.0, 1.0, 0.0], [0.0, 0.0, 1.0]]);
    // Z up, but still metres.
    let z_m = Options { up: Some(Up::Z), ..opts() };
    assert_eq!(got(&z_m), vec![[1000.0, 0.0, 0.0], [0.0, 1000.0, 0.0], [0.0, 0.0, 1000.0]]);
    // Y up in inches.
    let y_in = Options { units: Some(Unit::In), ..opts() };
    assert_eq!(got(&y_in), vec![[25.4, 0.0, 0.0], [0.0, 0.0, 25.4], [0.0, -25.4, 0.0]]);
    // The same overrides on an STL, whose own frame is +Z up in mm.
    let stl = cadpole_core::export::stl(&[&cadpole_core::tessellate::Mesh {
        positions: vec![1.0, 0.0, 0.0, 0.0, 1.0, 0.0, 0.0, 0.0, 1.0],
        normals: vec![0.0; 9],
        indices: vec![0, 1, 2],
        face_ids: vec![0],
        colors: Vec::new(),
    }]);
    let stl_pos = |o: &Options| -> Vec<[f32; 3]> {
        let im = import(&stl, "tri.stl", o);
        im.bodies[0].mesh.positions.chunks_exact(3).map(|p| [p[0], p[1], p[2]]).collect()
    };
    assert_eq!(stl_pos(&opts()), vec![[1.0, 0.0, 0.0], [0.0, 1.0, 0.0], [0.0, 0.0, 1.0]], "STL defaults unchanged");
    let stl_m_y = Options { up: Some(Up::Y), units: Some(Unit::M), ..opts() };
    assert_eq!(stl_pos(&stl_m_y), vec![[1000.0, 0.0, 0.0], [0.0, 0.0, 1000.0], [0.0, -1000.0, 0.0]]);
    assert!(Up::parse("q").unwrap_err().contains("valid: y"));
    assert_eq!(Up::parse("+Y").unwrap(), Some(Up::Y));
    assert_eq!(Up::parse("").unwrap(), None);
}

#[test]
fn indices_of_every_width_and_none_read_the_same_cube() {
    let mut volumes = Vec::new();
    for kind in [8u8, 16, 32, 0] {
        let mut b = Builder::default();
        let rest = if kind == 0 {
            // Not indexed: every triangle's corners written out in order.
            let (p, t) = cube();
            let flat: Vec<[f32; 3]> = t.iter().map(|&i| p[i as usize]).collect();
            let pos = b.positions(&flat);
            json!({"nodes": [{"mesh": 0}], "meshes": [{"primitives": [{"attributes": {"POSITION": pos}}]}]})
        } else {
            one_node_cube(&mut b, kind)
        };
        let im = import(&b.glb(rest), "cube.glb", &Options { repair: Repair::Weld, ..opts() });
        let r = &im.bodies[0].report;
        assert_eq!(r.triangles, 12, "u{kind}");
        assert!(r.solid, "u{kind}: {}", r.summary());
        assert_eq!(r.vertices, 8, "u{kind}: welded to the corners");
        volumes.push(r.volume.unwrap());
    }
    for v in &volumes {
        assert!((v - 1.0e9).abs() < 1e-3, "{volumes:?}");
    }
    // A u32 index past u16's range is read as itself: a strip of 70 000
    // vertices on one line, the last triangle using the last ones.
    let mut b = Builder::default();
    let n = 70_000u32;
    let ps: Vec<[f32; 3]> = (0..n).map(|i| [i as f32 * 1e-3, (i % 2) as f32 * 1e-3, 0.0]).collect();
    let pos = b.positions(&ps);
    let ind = b.indices_u32(&[n - 3, n - 2, n - 1]);
    let rest = json!({"nodes": [{"mesh": 0}], "meshes": [{"primitives": [{"attributes": {"POSITION": pos}, "indices": ind}]}]});
    let im = import(&b.glb(rest), "wide.glb", &opts());
    assert_eq!(im.bodies[0].report.triangles, 1);
    assert_eq!(im.bodies[0].mesh.vertex_count(), 3, "only the vertices the triangles use");
}

#[test]
fn one_mesh_under_two_nodes_is_two_bodies_and_the_hierarchy_composes() {
    let mut b = Builder::default();
    let (p, t) = cube();
    let pos = b.positions(&p);
    let ind = b.indices_u16(&t);
    let rest = json!({
        "scene": 0,
        "scenes": [{"nodes": [0, 1]}],
        "nodes": [
            {"mesh": 0, "translation": [10.0, 0.0, 0.0]},
            // A parent 5 m up, turned a quarter about +Y; its child sits
            // 2 m along the parent's +X, which the turn makes glTF's -Z.
            {"name": "", "translation": [0.0, 5.0, 0.0], "rotation": [0.0, 0.70710678, 0.0, 0.70710678], "children": [2]},
            {"mesh": 0, "translation": [2.0, 0.0, 0.0]}
        ],
        "meshes": [{"name": "chair", "primitives": [{"attributes": {"POSITION": pos}, "indices": ind}]}]
    });
    let im = import(&b.glb(rest), "room.glb", &opts());
    assert_eq!(im.bodies.len(), 2);
    let names: Vec<&str> = im.bodies.iter().map(|b| b.name.as_str()).collect();
    assert_eq!(names, ["chair[0]", "chair[1]"], "unnamed nodes take the mesh's name, with [k] on each");
    // First: [10, 11] x [0, 1] x [0, 1] -> cadpole x [10000, 11000], y [-1000, 0], z [0, 1000].
    let (lo, hi) = bbox(&im.bodies[0]);
    assert!(close3(lo, [10000.0, -1000.0, 0.0], 1e-2) && close3(hi, [11000.0, 0.0, 1000.0], 1e-2), "{lo:?} {hi:?}");
    // Second: the child's cube, local [2, 3] x [0, 1] x [0, 1], turned
    // +90 about Y (x -> -z, z -> x): x [0, 1], y [0, 1], z [-3, -2]; then
    // up 5: y [5, 6]. In Cadpole: x [0, 1000], y = -z [2000, 3000], z = y [5000, 6000].
    let (lo, hi) = bbox(&im.bodies[1]);
    assert!(close3(lo, [0.0, 2000.0, 5000.0], 1e-2) && close3(hi, [1000.0, 3000.0, 6000.0], 1e-2), "{lo:?} {hi:?}");
    for body in &im.bodies {
        assert!(body.report.solid && body.report.volume.unwrap() > 0.0, "{}", body.report.summary());
    }
    // A node's own name wins over its mesh's; a unique name takes no [k].
    let mut b2 = Builder::default();
    let rest2 = {
        let (p, t) = cube();
        let pos = b2.positions(&p);
        let ind = b2.indices_u16(&t);
        json!({"nodes": [{"mesh": 0, "name": "Left chair"}, {"mesh": 0, "translation": [3, 0, 0]}],
               "meshes": [{"name": "chair", "primitives": [{"attributes": {"POSITION": pos}, "indices": ind}]}]})
    };
    let im = import(&b2.glb(rest2), "room.glb", &opts());
    let names: Vec<&str> = im.bodies.iter().map(|b| b.name.as_str()).collect();
    assert_eq!(names, ["Left_chair", "chair"]);
    assert!(im.notes.iter().any(|n| n.message.contains("no scenes")), "{:?}", im.notes);
    assert_eq!(im.bodies[1].report.object, "chair");
}

#[test]
fn a_mirroring_node_is_rewound_so_the_cube_stays_outward() {
    let mut b = Builder::default();
    let mut rest = one_node_cube(&mut b, 16);
    rest["nodes"][0]["scale"] = json!([-1.0, 1.0, 1.0]);
    let im = import(&b.glb(rest), "mirror.glb", &opts());
    let r = &im.bodies[0].report;
    assert!(r.solid && r.volume.unwrap() > 0.0, "{}", r.summary());
    assert!(im.notes.iter().any(|n| n.message.contains("mirroring")), "{:?}", im.notes);
    // A matrix does the same as TRS.
    let mut b = Builder::default();
    let mut rest = one_node_cube(&mut b, 16);
    rest["nodes"][0]["matrix"] = json!([1, 0, 0, 0, 0, 1, 0, 0, 0, 0, -1, 0, 0, 0, 0, 1]);
    let r = import(&b.glb(rest), "mirror.glb", &opts()).bodies.remove(0).report;
    assert!(r.solid && r.volume.unwrap() > 0.0, "{}", r.summary());
}

#[test]
fn color_0_in_every_encoding_becomes_srgb_vertex_colours() {
    let (p, t) = cube();
    // Linear 0.5 grey is sRGB 188; linear 0.2 is 124; 1.0 is 255.
    let cases: Vec<(&str, Box<dyn Fn(&mut Builder) -> usize>)> = vec![
        ("unorm8 RGBA", Box::new(|b: &mut Builder| b.colors_u8(&[[128, 51, 255, 255]; 8]))),
        ("unorm16 RGB", Box::new(|b: &mut Builder| b.colors_u16(&[[32896, 13107, 65535]; 8]))),
        ("float RGB", Box::new(|b: &mut Builder| b.colors_f32(&[[128.0 / 255.0, 0.2, 1.0]; 8]))),
    ];
    for (what, col) in cases {
        let mut b = Builder::default();
        let pos = b.positions(&p);
        let ind = b.indices_u32(&t);
        let c = col(&mut b);
        let rest = json!({"nodes": [{"mesh": 0, "name": "scan"}], "meshes": [{"primitives": [{"attributes": {"POSITION": pos, "COLOR_0": c}, "indices": ind}]}]});
        let im = import(&b.glb(rest), "scan.glb", &opts());
        let body = &im.bodies[0];
        let colors = body.mesh.vertex_colors().unwrap_or_else(|| panic!("{what}: colours carried"));
        assert_eq!(colors.len(), 8 * 3);
        for c in colors.chunks_exact(3) {
            assert!(c[0].abs_diff(188) <= 1 && c[1].abs_diff(124) <= 1 && c[2] == 255, "{what}: {c:?}");
        }
        assert!(body.report.vertex_colors, "{what}");
        // No material: white underneath, so a viewer shows the colours as they are.
        let look = body.appearance.as_ref().expect("a body with vertex colours has a look");
        assert_eq!(look.base_color, [1.0; 3], "{what}");
        // The colours survive the weld.
        let welded = import(&b.glb(json!({"nodes": [{"mesh": 0}], "meshes": [{"primitives": [{"attributes": {"POSITION": pos, "COLOR_0": c}, "indices": ind}]}]})), "scan.glb", &Options { repair: Repair::Weld, ..opts() });
        assert!(welded.bodies[0].mesh.vertex_colors().is_some(), "{what}: welded");
    }
    // What a phone scanner writes: unorm8 colours, an unlit white material.
    let mut b = Builder::default();
    let pos = b.positions(&p);
    let ind = b.indices_u32(&t);
    let c = b.colors_u8(&[[255, 0, 0, 255]; 8]);
    let rest = json!({
        "extensionsUsed": ["KHR_materials_unlit"],
        "nodes": [{"mesh": 0, "name": "scan"}],
        "materials": [{"pbrMetallicRoughness": {"baseColorFactor": [1, 1, 1, 1], "metallicFactor": 0, "roughnessFactor": 1},
                       "doubleSided": true, "extensions": {"KHR_materials_unlit": {}}}],
        "meshes": [{"primitives": [{"attributes": {"POSITION": pos, "COLOR_0": c}, "indices": ind, "material": 0, "mode": 4}]}]
    });
    let im = import(&b.glb(rest), "scan.glb", &opts());
    let body = &im.bodies[0];
    assert_eq!(&body.mesh.vertex_colors().unwrap()[..3], &[255, 0, 0]);
    assert_eq!(body.topo.faces[0].name, "import_mesh/surface", "one unnamed material is the surface");
    let look = body.appearance.as_ref().unwrap();
    assert_eq!((look.base_color, look.metallic, look.roughness), ([1.0; 3], 0.0, 1.0));
    assert!(im.notes.iter().any(|n| n.message.contains("KHR_materials_unlit") && n.message.contains("lit like any other")), "{:?}", im.notes);
}

#[test]
fn base_colours_become_the_look_or_per_vertex_colour_when_materials_differ() {
    let (p, t) = cube();
    // One material: its factors are the body's look, and no vertex colour.
    let mut b = Builder::default();
    let pos = b.positions(&p);
    let ind = b.indices_u16(&t);
    let rest = json!({
        "nodes": [{"mesh": 0}],
        "materials": [{"name": "Brass", "pbrMetallicRoughness": {"baseColorFactor": [0.9, 0.6, 0.2, 0.5], "metallicFactor": 1.0, "roughnessFactor": 0.3,
                       "baseColorTexture": {"index": 0}}, "emissiveFactor": [0.1, 0.0, 0.0], "alphaMode": "BLEND"}],
        "meshes": [{"primitives": [{"attributes": {"POSITION": pos}, "indices": ind, "material": 0}]}]
    });
    let im = import(&b.glb(rest), "knob.glb", &opts());
    let body = &im.bodies[0];
    assert!(body.mesh.vertex_colors().is_none());
    let look = body.appearance.clone().unwrap();
    assert_eq!(look.base_color, [0.9, 0.6, 0.2]);
    assert_eq!((look.opacity, look.metallic, look.roughness), (0.5, 1.0, 0.3));
    assert_eq!(look.emissive, [0.1, 0.0, 0.0]);
    assert_eq!(look.material, "Brass");
    assert_eq!(body.topo.faces[0].name, "import_mesh/Brass", "a named material names the face");
    assert!(im.notes.iter().any(|n| n.message.contains("textures are not read") && n.message.contains("Brass")), "{:?}", im.notes);
    // Round trip through the feature args, as the memo replays it.
    let args: BTreeMap<String, String> = look.to_args("look[0].").into_iter().collect();
    assert_eq!(mesh::Appearance::from_args(&args, "look[0]."), Some(look));

    // Two materials that differ: one face each, and the colours per vertex.
    let mut b = Builder::default();
    let pos = b.positions(&p);
    let top = b.indices_u16(&t[..6]);
    let rest_i = b.indices_u16(&t[6..]);
    let rest = json!({
        "nodes": [{"mesh": 0, "name": "box"}],
        "materials": [{"name": "Red", "pbrMetallicRoughness": {"baseColorFactor": [1, 0, 0, 1], "metallicFactor": 0}},
                      {"pbrMetallicRoughness": {"baseColorFactor": [0, 0, 1, 1], "metallicFactor": 0}}],
        "meshes": [{"primitives": [
            {"attributes": {"POSITION": pos}, "indices": top, "material": 0},
            {"attributes": {"POSITION": pos}, "indices": rest_i, "material": 1}
        ]}]
    });
    let im = import(&b.glb(rest), "box.glb", &opts());
    let body = &im.bodies[0];
    let faces: Vec<&str> = body.topo.faces.iter().map(|f| f.name.as_str()).collect();
    assert_eq!(faces, ["import_mesh/Red", "import_mesh/material[1]"]);
    assert_eq!(body.topo.faces[0].tri_count, 2);
    let colors = body.mesh.vertex_colors().expect("the colours are per vertex");
    // The first primitive's vertices are red, the second's blue.
    assert_eq!(&colors[..3], &[255, 0, 0]);
    assert_eq!(&colors[colors.len() - 3..], &[0, 0, 255]);
    assert_eq!(body.appearance.as_ref().unwrap().base_color, [1.0; 3], "white under per-vertex colour");
    // A primitive with no material beside one with: `default`.
    let mut b = Builder::default();
    let pos = b.positions(&p);
    let top = b.indices_u16(&t[..6]);
    let rest_i = b.indices_u16(&t[6..]);
    let rest = json!({
        "nodes": [{"mesh": 0}],
        "materials": [{"name": "Lid", "pbrMetallicRoughness": {"baseColorFactor": [1, 1, 1, 1], "metallicFactor": 0}}],
        "meshes": [{"primitives": [
            {"attributes": {"POSITION": pos}, "indices": top, "material": 0},
            {"attributes": {"POSITION": pos}, "indices": rest_i}
        ]}]
    });
    let body = import(&b.glb(rest), "box.glb", &opts()).bodies.remove(0);
    let faces: Vec<&str> = body.topo.faces.iter().map(|f| f.name.as_str()).collect();
    assert_eq!(faces, ["import_mesh/Lid", "import_mesh/default"]);
    assert!(body.mesh.vertex_colors().is_none(), "white and the default white agree: no per-vertex colour");
}

#[test]
fn a_gltf_with_a_data_uri_buffer_or_a_bin_asset_reads_the_same_cube() {
    let mut b = Builder::default();
    let rest = one_node_cube(&mut b, 16);
    let mut doc = b.doc(rest);
    let bin = b.bin.clone();
    doc["buffers"] = json!([{"byteLength": bin.len(), "uri": format!("data:application/octet-stream;base64,{}", base64(&bin))}]);
    let text = serde_json::to_vec_pretty(&doc).unwrap();
    let im = import(&text, "cube.gltf", &opts());
    assert!(im.notes.iter().any(|n| n.message.contains("(JSON, from")), "{:?}", im.notes);
    let r = &im.bodies[0].report;
    assert!(r.solid && (r.volume.unwrap() - 1.0e9).abs() < 1e-3, "{}", r.summary());
    assert_eq!(im.bodies[0].name, "cube", "the asset's stem names an unnamed mesh");

    // Percent-encoded rather than base64: every byte is itself, 0x80 and
    // above included.
    let pct: String = bin.iter().map(|b| format!("%{b:02X}")).collect();
    doc["buffers"] = json!([{"byteLength": bin.len(), "uri": format!("data:application/octet-stream,{pct}")}]);
    let im = import(&serde_json::to_vec(&doc).unwrap(), "cube.gltf", &opts());
    assert!((im.bodies[0].report.volume.unwrap() - 1.0e9).abs() < 1e-3);

    // The same bytes as a `.bin` beside it: named by the caller, looked up
    // by file name, and nothing else.
    doc["buffers"] = json!([{"byteLength": bin.len(), "uri": "buffers/cube%20data.bin"}]);
    let text = serde_json::to_vec(&doc).unwrap();
    assert_eq!(gltf::external_buffers(&text).unwrap(), vec![("buffers/cube%20data.bin".to_owned(), "cube data.bin".to_owned())]);
    let mut res = Resources::default();
    res.0.insert("cube data.bin".into(), Arc::from(bin.as_slice()));
    let with = Options { resources: res, ..opts() };
    let im = import(&text, "cube.gltf", &with);
    assert!(im.bodies[0].report.solid);
    // Without it: named.
    let e = mesh::import_unbounded(&text, "cube.gltf", &opts()).unwrap_err();
    assert!(e.contains("not an asset of this document") && e.contains("cube data.bin"), "{e}");
    // A URL is never fetched.
    doc["buffers"] = json!([{"byteLength": bin.len(), "uri": "https://example.com/cube.bin"}]);
    let e = gltf::external_buffers(&serde_json::to_vec(&doc).unwrap()).unwrap_err();
    assert!(e.contains("https: URL") && e.contains("never fetched"), "{e}");
}

#[test]
fn quantized_positions_read_as_the_extension_allows() {
    // int16, normalised: 32767 is 1.0.
    let mut b = Builder::default();
    let q: Vec<u8> = [[32767i16, 0, 0], [0, 32767, 0], [0, 0, 32767]].iter().flatten().flat_map(|x| x.to_le_bytes()).collect();
    let v = b.view(&q);
    let pos = b.accessor(v, 5122, 3, "VEC3", true);
    let rest = json!({"extensionsRequired": ["KHR_mesh_quantization"], "extensionsUsed": ["KHR_mesh_quantization"],
                      "nodes": [{"mesh": 0}], "meshes": [{"primitives": [{"attributes": {"POSITION": pos}}]}]});
    let im = import(&b.glb(rest), "q.glb", &opts());
    let p = &im.bodies[0].mesh.positions;
    assert_eq!(&p[..3], &[1000.0, 0.0, 0.0]);
}

#[test]
fn scenes_modes_and_the_triangle_cap_are_honoured() {
    let (p, t) = cube();
    // The default scene is the one imported.
    let mut b = Builder::default();
    let pos = b.positions(&p);
    let ind = b.indices_u16(&t);
    let rest = json!({
        "scene": 1,
        "scenes": [{"nodes": [0]}, {"nodes": [1]}],
        "nodes": [{"mesh": 0, "name": "a"}, {"mesh": 0, "name": "b"}],
        "meshes": [{"primitives": [{"attributes": {"POSITION": pos}, "indices": ind}]}]
    });
    let im = import(&b.glb(rest), "s.glb", &opts());
    assert_eq!(im.bodies.len(), 1);
    assert_eq!(im.bodies[0].name, "b");
    // Lines are skipped, with a warning; a mesh with only lines leaves nothing.
    let mut b = Builder::default();
    let pos = b.positions(&p);
    let ind = b.indices_u16(&t);
    let rest = json!({"nodes": [{"mesh": 0}], "meshes": [{"primitives": [
        {"attributes": {"POSITION": pos}, "indices": ind},
        {"attributes": {"POSITION": pos}, "mode": 1},
        {"attributes": {"POSITION": pos}, "mode": 5}
    ]}]});
    let im = import(&b.glb(rest), "m.glb", &opts());
    assert_eq!(im.bodies[0].report.triangles, 12);
    let warns: Vec<&str> = im.notes.iter().filter(|n| n.level == crate::Level::Warning).map(|n| n.message.as_str()).collect();
    assert!(warns.iter().any(|m| m.contains("of lines (mode 1) skipped")), "{warns:?}");
    assert!(warns.iter().any(|m| m.contains("of triangle strips (mode 5) skipped")), "{warns:?}");
    let rest = json!({"nodes": [{"mesh": 0}], "meshes": [{"primitives": [{"attributes": {"POSITION": pos}, "mode": 0}]}]});
    let e = mesh::import_unbounded(&b.glb(rest), "m.glb", &opts()).unwrap_err();
    assert!(e.contains("no triangles Cadpole reads"), "{e}");
    // A mesh placed often enough to pass the cap is refused before any
    // body is built: 60 000 nodes of a 12-triangle cube is 720 000; the cap
    // is counted over the placements, so 5 M / 12 + 1 nodes would pass it.
    let n = prescan::MAX_TRIANGLES / 12 + 1;
    let nodes: Vec<Value> = (0..n).map(|_| json!({"mesh": 0})).collect();
    let mut b = Builder::default();
    let pos = b.positions(&p);
    let ind = b.indices_u16(&t);
    let rest = json!({"nodes": nodes, "meshes": [{"primitives": [{"attributes": {"POSITION": pos}, "indices": ind}]}]});
    let t0 = std::time::Instant::now();
    let e = mesh::import_unbounded(&b.glb(rest), "many.glb", &opts()).unwrap_err();
    assert!(e.contains("more than 5000000 triangles"), "{e}");
    assert!(t0.elapsed().as_secs() < 20, "refused from the counts, not after building");
    // No nodes at all: every mesh where it is defined, with a warning.
    let mut b = Builder::default();
    let pos = b.positions(&p);
    let ind = b.indices_u16(&t);
    let rest = json!({"meshes": [{"name": "loose", "primitives": [{"attributes": {"POSITION": pos}, "indices": ind}]}]});
    let im = import(&b.glb(rest), "loose.glb", &opts());
    assert_eq!(im.bodies[0].name, "loose");
    assert!(im.notes.iter().any(|n| n.level == crate::Level::Warning && n.message.contains("no nodes")), "{:?}", im.notes);
}

/// Hand-broken files: each is refused with a message that says where,
/// never a panic.
#[test]
fn malformed_files_are_located_diagnostics_never_panics() {
    let mut b = Builder::default();
    let good_rest = one_node_cube(&mut b, 16);
    let good = b.glb(good_rest.clone());
    let doc = b.doc(good_rest.clone());
    let bin = b.bin.clone();
    let with = |f: &dyn Fn(&mut Value)| -> Vec<u8> {
        let mut d = doc.clone();
        f(&mut d);
        glb_of(&d, &bin)
    };
    let mut cases: Vec<(&str, Vec<u8>, &str)> = vec![
        ("header only", good[..10].to_vec(), "12-byte header"),
        ("version 1", { let mut g = good.clone(); g[4] = 1; g }, "GLB version 1"),
        ("truncated", good[..good.len() - 8].to_vec(), "truncated"),
        ("chunk past the end", { let mut g = good.clone(); g[12..16].copy_from_slice(&100_000u32.to_le_bytes()); g }, "chunk 0 at byte 12"),
        ("first chunk not JSON", { let mut g = good.clone(); g[16..20].copy_from_slice(&0x004E_4942u32.to_le_bytes()); g }, "not JSON"),
        ("bad JSON", { let mut g = good.clone(); g[20] = b'['; g[21] = b'}'; g }, "line 1 column"),
        ("no asset", with(&|d| { d.as_object_mut().unwrap().remove("asset"); }), "no `asset`"),
        ("glTF 1.0", with(&|d| d["asset"]["version"] = json!("1.0")), "Cadpole reads glTF 2.0"),
        ("Draco required", with(&|d| d["extensionsRequired"] = json!(["KHR_draco_mesh_compression"])), "KHR_draco_mesh_compression"),
        ("index past the vertices", with(&|d| d["accessors"][0]["count"] = json!(4)), "refers to vertex"),
        ("accessor past its view", with(&|d| d["accessors"][1]["count"] = json!(400)), "past its"),
        ("view past its buffer", with(&|d| d["bufferViews"][0]["byteLength"] = json!(100_000)), "spans bytes"),
        ("offset past its view", with(&|d| d["accessors"][1]["byteOffset"] = json!(100_000)), "past the"),
        ("stride below the element", with(&|d| d["bufferViews"][0]["byteStride"] = json!(4)), "byteStride is 4"),
        ("unknown component type", with(&|d| d["accessors"][0]["componentType"] = json!(5130)), "componentType is 5130"),
        ("indices as floats", with(&|d| d["accessors"][1]["componentType"] = json!(5126)), "indices are uint8"),
        ("buffer shorter than declared", with(&|d| d["buffers"][0]["byteLength"] = json!(100_000)), "declares 100000 bytes"),
        ("mesh past the meshes", with(&|d| d["nodes"][0]["mesh"] = json!(7)), "nodes[0].mesh is 7"),
        ("a cycle", with(&|d| { d["nodes"][0]["children"] = json!([1]); d["nodes"] = json!([d["nodes"][0].clone(), {"children": [0]}]); }), "reached twice"),
        ("two parents", with(&|d| { d["scenes"][0]["nodes"] = json!([1, 2]); d["nodes"] = json!([{"mesh": 0}, {"children": [0]}, {"children": [0]}]); }), "reached twice"),
        ("a child that is not a node", with(&|d| d["nodes"][0]["children"] = json!([9])), "not a node"),
        ("a scene past the scenes", with(&|d| d["scene"] = json!(3)), "scenes[3] does not exist"),
        ("a matrix of 15", with(&|d| d["nodes"][0]["matrix"] = json!(vec![1; 15])), "matrix has 15 numbers"),
        ("a NaN-ish translation", with(&|d| d["nodes"][0]["translation"] = json!(["x", 0, 0])), "translation[0]"),
        ("a material past the materials", with(&|d| d["meshes"][0]["primitives"][0]["material"] = json!(4)), "materials[4] does not exist"),
        ("no POSITION", with(&|d| { d["meshes"][0]["primitives"][0]["attributes"] = json!({"NORMAL": 0}); }), "has no POSITION"),
        ("a buffer uri that is a file:", with(&|d| d["buffers"][0]["uri"] = json!("file:///etc/passwd")), "file: URL"),
        ("a bad data uri", with(&|d| d["buffers"][0]["uri"] = json!("data:application/octet-stream;base64,@@@@")), "not valid base64"),
        ("an empty file", Vec::new(), "empty"),
    ];
    // Depth past the cap.
    let deep: Vec<Value> = (0..prescan::MAX_NODE_DEPTH + 5)
        .map(|i| if i + 1 < prescan::MAX_NODE_DEPTH + 5 { json!({"children": [i + 1]}) } else { json!({"mesh": 0}) })
        .collect();
    cases.push(("too deep", with(&|d| { d["nodes"] = json!(deep.clone()); }), "deep"));
    // JSON nested past serde_json's own limit.
    let nested = format!("{{\"asset\":{{\"version\":\"2.0\"}},\"extras\":{}{}}}", "[".repeat(200), "]".repeat(200));
    cases.push(("JSON nested 200 deep", nested.into_bytes(), "recursion limit"));
    for (what, bytes, want) in &cases {
        let got = mesh::import(bytes, "broken.glb", &opts());
        let e = match got {
            Ok(im) => panic!("{what}: imported {} bodies", im.bodies.len()),
            Err(e) => e,
        };
        assert!(!e.contains("the parser failed"), "{what}: a panic reached the guard: {e}");
        assert!(e.contains(want), "{what}: {e}\n  wanted {want:?}");
    }
    // A sparse accessor skips its primitive with a warning; a sparse
    // POSITION in the only primitive leaves nothing.
    let sparse = with(&|d| d["accessors"][0]["sparse"] = json!({"count": 1, "indices": {"bufferView": 1, "componentType": 5123}, "values": {"bufferView": 0}}));
    let e = mesh::import(&sparse, "sparse.glb", &opts()).unwrap_err();
    assert!(e.contains("no triangles Cadpole reads"), "{e}");
}

/// splitmix64.
struct Rng(u64);

impl Rng {
    fn next(&mut self) -> u64 {
        self.0 = self.0.wrapping_add(0x9E37_79B9_7F4A_7C15);
        let mut z = self.0;
        z = (z ^ (z >> 30)).wrapping_mul(0xBF58_476D_1CE4_E5B9);
        z = (z ^ (z >> 27)).wrapping_mul(0x94D0_49BB_1331_11EB);
        z ^ (z >> 31)
    }
    fn below(&mut self, n: usize) -> usize {
        (self.next() % n.max(1) as u64) as usize
    }
}

#[test]
fn three_hundred_mutations_of_a_glb_and_a_gltf_are_bodies_or_diagnostics_never_panics() {
    let (p, t) = cube();
    let mut b = Builder::default();
    let pos = b.positions(&p);
    let ind = b.indices_u16(&t);
    let col = b.colors_u8(&[[10, 200, 30, 255]; 8]);
    let rest = json!({
        "scene": 0, "scenes": [{"nodes": [0]}],
        "nodes": [{"name": "root", "children": [1], "rotation": [0, 0.3826834, 0, 0.9238795]}, {"mesh": 0, "translation": [1, 2, 3]}],
        "materials": [{"name": "m", "pbrMetallicRoughness": {"baseColorFactor": [0.5, 0.5, 0.5, 1]}}],
        "meshes": [{"primitives": [{"attributes": {"POSITION": pos, "COLOR_0": col}, "indices": ind, "material": 0}]}]
    });
    let glb = b.glb(rest.clone());
    let mut doc = b.doc(rest);
    doc["buffers"] = json!([{"byteLength": b.bin.len(), "uri": format!("data:application/octet-stream;base64,{}", base64(&b.bin))}]);
    let gltf_text = serde_json::to_vec(&doc).unwrap();
    assert!(mesh::import(&glb, "m.glb", &opts()).is_ok());
    assert!(mesh::import(&gltf_text, "m.gltf", &opts()).is_ok());
    let mut rng = Rng(0x5EED_D210);
    let (mut ok, mut refused) = (0, 0);
    for (name, bytes) in [("m.glb", &glb), ("m.gltf", &gltf_text)] {
        for k in 0..150 {
            let mut m = bytes.clone();
            for _ in 0..=rng.below(3) {
                match rng.below(4) {
                    0 => {
                        let i = rng.below(m.len());
                        m[i] ^= 1 << rng.below(8);
                    }
                    1 => {
                        let i = rng.below(m.len());
                        let n = rng.below(16).min(m.len() - i);
                        m.drain(i..i + n);
                    }
                    2 => {
                        // A digit in the JSON becomes a big number.
                        if let Some(i) = (0..64).map(|_| rng.below(m.len())).find(|&i| m[i].is_ascii_digit()) {
                            m.splice(i..i + 1, b"4294967295".iter().copied());
                        }
                    }
                    _ => {
                        let i = rng.below(m.len());
                        m[i] = rng.next() as u8;
                    }
                }
            }
            match mesh::import(&m, name, &Options { budget_ms: 20_000, ..opts() }) {
                Ok(_) => ok += 1,
                Err(e) => {
                    assert!(!e.contains("the parser failed"), "{name} mutation {k}: a panic: {e}");
                    assert!(!e.contains("budget"), "{name} mutation {k}: {e}");
                    refused += 1;
                }
            }
        }
    }
    eprintln!("glTF mutations: {ok} imported, {refused} refused, 0 panics");
    assert_eq!(ok + refused, 300);
}

#[test]
fn an_obj_with_vertex_colours_carries_them() {
    let s = "o tri\nv 0 0 0 1 0 0\nv 1 0 0 0 1 0\nv 0 1 0 0 0 1\nf 1 2 3\n";
    let im = import(s.as_bytes(), "tri.obj", &opts());
    let body = &im.bodies[0];
    assert_eq!(body.mesh.vertex_colors().unwrap(), &[255, 0, 0, 0, 255, 0, 0, 0, 255]);
    assert_eq!(body.appearance.as_ref().unwrap().base_color, [1.0; 3]);
    assert!(body.report.vertex_colors);
    assert!(im.notes.iter().any(|n| n.message.contains("0..1")), "{:?}", im.notes);
    // 0..255, and a vertex without a colour is white.
    let s = "v 0 0 0 255 128 0\nv 1 0 0\nv 0 1 0 0 0 255\nf 1 2 3\n";
    let im = import(s.as_bytes(), "tri.obj", &opts());
    assert_eq!(im.bodies[0].mesh.vertex_colors().unwrap(), &[255, 128, 0, 255, 255, 255, 0, 0, 255]);
    assert!(im.notes.iter().any(|n| n.message.contains("0..255") && n.message.contains("white")), "{:?}", im.notes);
    // `v x y z w` is still a weight, and three numbers that are not a
    // colour are read past, never an error.
    let s = "v 0 0 0 1\nv 1 0 0 1\nv 0 1 0 1\nf 1 2 3\n";
    assert!(import(s.as_bytes(), "tri.obj", &opts()).bodies[0].mesh.vertex_colors().is_none());
    let s = "v 0 0 0 -0.5 0.2 0.9\nv 1 0 0\nv 0 1 0\nf 1 2 3\n";
    let im = import(s.as_bytes(), "tri.obj", &opts());
    assert!(im.bodies[0].mesh.vertex_colors().is_none());
    assert!(im.notes.iter().any(|n| n.message.contains("not a colour")), "{:?}", im.notes);
    // In metres and +Y up, as a phone's OBJ is.
    let s = "v 1 0 0 0.5 0.5 0.5\nv 0 1 0 0.5 0.5 0.5\nv 0 0 1 0.5 0.5 0.5\nf 1 2 3\n";
    let im = import(s.as_bytes(), "scan.obj", &Options { up: Some(Up::Y), units: Some(Unit::M), ..opts() });
    let p = &im.bodies[0].mesh.positions;
    assert_eq!(p.as_slice(), &[1000.0, 0.0, 0.0, 0.0, 0.0, 1000.0, 0.0, -1000.0, 0.0]);
}

#[test]
fn a_3mf_unit_is_replaced_by_units_and_said() {
    let m = cadpole_core::tessellate::tessellate(
        &cadpole_core::kernel::extrude(
            &cadpole_core::sketch::Sketch::new(cadpole_core::geom::Plane::XY)
                .with_loop(cadpole_core::sketch::Loop::rect(cadpole_core::sketch::P2::new(0.0, 0.0), 10.0, 10.0)),
            10.0,
        )
        .unwrap()
        .remove(0),
        0.02,
    )
    .unwrap()
    .0;
    let bytes = cadpole_export_3mf::write(&cadpole_export_3mf::Scene {
        title: "box".into(),
        parts: vec![cadpole_export_3mf::Part::new("box", &m, 1)],
        ..Default::default()
    })
    .bytes;
    let mm = import(&bytes, "box.3mf", &opts());
    let inch = import(&bytes, "box.3mf", &Options { units: Some(Unit::In), ..opts() });
    let (v0, v1) = (mm.bodies[0].report.volume.unwrap(), inch.bodies[0].report.volume.unwrap());
    assert!((v1 / v0 - 25.4 * 25.4 * 25.4).abs() < 1e-3, "{v0} {v1}");
    assert!(inch.notes.iter().any(|n| n.message.contains("replacing the <model unit>")), "{:?}", inch.notes);
}

#[test]
fn repair_off_says_what_it_found_as_information() {
    // An open surface, as a scan is.
    let mut b = Builder::default();
    let pos = b.positions(&[[0.0, 0.0, 0.0], [1.0, 0.0, 0.0], [0.0, 1.0, 0.0]]);
    let bytes = b.glb(json!({"nodes": [{"mesh": 0}], "meshes": [{"primitives": [{"attributes": {"POSITION": pos}}]}]}));
    for (repair, warns) in [(Repair::Report, true), (Repair::Weld, true), (Repair::Off, false)] {
        let r = import(&bytes, "tri.glb", &Options { repair, ..opts() }).bodies.remove(0).report;
        let notes = r.diagnostics_under(repair);
        assert!(notes.iter().any(|n| n.message.contains("not closed")), "{repair:?}: {notes:?}");
        assert_eq!(notes.iter().any(|n| n.level == crate::Level::Warning), warns, "{repair:?}: {notes:?}");
    }
}

/// `examples/assets/scan_room.glb`, which `examples/tools/scan_room_glb.py`
/// writes, read back as the script's docstring describes it: one node
/// `scan`, a 4 x 3 x 2.5 m room open at the top with a doorway, uint32
/// indices and unorm8 colours, an unlit white material -- so the example's
/// assertions and the generator cannot drift apart unnoticed.
#[test]
fn the_example_scan_is_the_room_its_generator_describes() {
    let dir = match std::env::var("CADPOLE_EXAMPLES") {
        Ok(d) => std::path::PathBuf::from(d),
        Err(_) => std::path::PathBuf::from(env!("CARGO_MANIFEST_DIR")).join("../../../examples"),
    };
    let bytes = std::fs::read(dir.join("assets/scan_room.glb")).expect("the example asset");
    assert!(bytes.len() < 100_000, "the example asset stays small: {} bytes", bytes.len());
    let im = import(&bytes, "scan_room.glb", &Options { repair: Repair::Off, ..opts() });
    assert_eq!(im.bodies.len(), 1);
    let b = &im.bodies[0];
    assert_eq!(b.name, "scan");
    assert_eq!(b.report.triangles, 2036);
    assert!(b.mesh.vertex_colors().is_some());
    assert!(!b.report.closed, "open at the top and through the doorway");
    let (lo, hi) = bbox(b);
    assert!(close3(lo, [0.0, -3000.0, 0.0], 5.0) && close3(hi, [4000.0, 0.0, 2500.0], 5.0), "{lo:?} {hi:?}");
    assert!(im.notes.iter().any(|n| n.message.contains("scan_room_glb.py")), "{:?}", im.notes);
    // The floor is oak: the lowest vertices are brown, not grey.
    let c = b.mesh.vertex_colors().unwrap();
    let floor = (0..b.mesh.vertex_count()).find(|&v| b.mesh.positions[v * 3 + 2].abs() < 1.0 && b.mesh.positions[v * 3 + 1] < -1500.0).unwrap();
    let rgb = &c[floor * 3..floor * 3 + 3];
    assert!(rgb[0] > rgb[2] + 40, "an oak floor: {rgb:?}");
}
