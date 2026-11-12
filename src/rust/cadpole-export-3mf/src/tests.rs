//! What a 3MF file has to be: a readable zip, an OPC package with the right
//! parts, a model that satisfies the core specification's structural rules,
//! and colours that survive being written and read back.

use super::*;
use cadpole_core::geom::Vec3;

// ---------------------------------------------------------------------------
// Fixtures
// ---------------------------------------------------------------------------

pub(crate) fn push_tri(m: &mut Mesh, a: Vec3, b: Vec3, c: Vec3, face: u32) {
    let base = (m.positions.len() / 3) as u32;
    let n = (b - a).cross(c - a).normalized();
    for p in [a, b, c] {
        m.positions.extend_from_slice(&[p.x as f32, p.y as f32, p.z as f32]);
        m.normals.extend_from_slice(&[n.x as f32, n.y as f32, n.z as f32]);
    }
    m.indices.extend_from_slice(&[base, base + 1, base + 2]);
    m.face_ids.push(face);
}

/// A box, written the way a tessellator writes one: every side with its own
/// four vertices, so the writer has to weld 24 down to 8.
pub(crate) fn box_mesh(lo: Vec3, hi: Vec3) -> Mesh {
    let mut m = Mesh::default();
    let v = |i: usize| {
        Vec3::new(
            if i & 1 == 0 { lo.x } else { hi.x },
            if i & 2 == 0 { lo.y } else { hi.y },
            if i & 4 == 0 { lo.z } else { hi.z },
        )
    };
    let sides: [[usize; 4]; 6] =
        [[0, 4, 6, 2], [1, 3, 7, 5], [0, 1, 5, 4], [2, 6, 7, 3], [0, 2, 3, 1], [4, 5, 7, 6]];
    for (f, s) in sides.iter().enumerate() {
        let (a, b, c, d) = (v(s[0]), v(s[1]), v(s[2]), v(s[3]));
        push_tri(&mut m, a, b, c, f as u32);
        push_tri(&mut m, a, c, d, f as u32);
    }
    m
}

/// Two filaments, given the way a script gives them — as sRGB bytes decoded
/// to linear — so the tests below can assert that the bytes written to the
/// file are the bytes the script wrote.
pub(crate) fn palette() -> Vec<Filament> {
    vec![
        Filament { name: "shell".into(), color: cadpole_core::paint::unhex("#C98A6B") },
        Filament { name: String::new(), color: cadpole_core::paint::unhex("#0078BF") },
    ]
}

fn two_body_scene<'a>(a: &'a Mesh, b: &'a Mesh, slicer: Slicer) -> Scene<'a> {
    Scene {
        title: "cadpole test".into(),
        parts: vec![Part::new("base", a, 1), Part::new("lid", b, 2)],
        palette: palette(),
        slicer,
        ..Scene::default()
    }
}

pub(crate) fn open(bytes: &[u8]) -> zip::Archive {
    zip::read(bytes).expect("our own package must read back through a zip reader")
}

fn model_of(bytes: &[u8]) -> String {
    open(bytes).text(MODEL_PART).expect("a 3D model part")
}

// ---------------------------------------------------------------------------
// The package
// ---------------------------------------------------------------------------

#[test]
fn the_package_has_the_parts_opc_requires() {
    let (a, b) = (box_mesh(Vec3::ZERO, Vec3::new(10.0, 10.0, 4.0)), box_mesh(Vec3::new(0.0, 0.0, 4.0), Vec3::new(10.0, 10.0, 6.0)));
    let w = write(&two_body_scene(&a, &b, Slicer::Prusa));
    let z = open(&w.bytes);
    assert!(z.names.contains(&CONTENT_TYPES_PART.to_owned()), "{:?}", z.names);
    assert!(z.names.contains(&RELS_PART.to_owned()));
    assert!(z.names.contains(&MODEL_PART.to_owned()));

    let ct = z.text(CONTENT_TYPES_PART).unwrap();
    assert!(ct.contains("application/vnd.openxmlformats-package.relationships+xml"), "{ct}");
    assert!(ct.contains("application/vnd.ms-package.3dmanufacturing-3dmodel+xml"), "{ct}");

    let rels = z.text(RELS_PART).unwrap();
    assert!(rels.contains("http://schemas.microsoft.com/3dmanufacturing/2013/01/3dmodel"), "{rels}");
    assert!(rels.contains(&format!("Target=\"/{MODEL_PART}\"")), "{rels}");
}

#[test]
fn the_same_scene_writes_the_same_bytes() {
    let a = box_mesh(Vec3::ZERO, Vec3::new(10.0, 10.0, 10.0));
    let go = || write(&Scene { parts: vec![Part::new("b", &a, 1)], palette: palette(), ..Scene::default() }).bytes;
    assert_eq!(go(), go(), "exports have to be comparable between runs");
}

// ---------------------------------------------------------------------------
// The core specification's structure
// ---------------------------------------------------------------------------

/// Everything the core specification says must hold about a model part that
/// carries base materials, checked by walking the parsed XML.
fn validate_core(xml: &str) {
    let doc = roxmltree::Document::parse(xml).expect("the model part must be well-formed XML");
    let root = doc.root_element();
    assert_eq!(root.tag_name().name(), "model");
    assert_eq!(root.tag_name().namespace(), Some(CORE_NS), "the core namespace");
    assert_eq!(root.attribute("unit"), Some("millimeter"), "Cadpole is millimetres everywhere");
    assert!(
        root.attribute("requiredextensions").is_none(),
        "a required extension makes a conformant consumer refuse the whole file"
    );

    let resources = root.children().find(|n| n.has_tag_name("resources")).expect("<resources>");
    let build = root.children().find(|n| n.has_tag_name("build")).expect("<build>");

    // Resource ids are unique model-wide, and a resource is defined before it
    // is referenced (the spec's forward-only parser rule).
    let mut ids: Vec<u32> = Vec::new();
    let mut material_count = 0usize;
    let mut materials_at: Option<usize> = None;
    for (i, n) in resources.children().filter(|n| n.is_element()).enumerate() {
        let id: u32 = n.attribute("id").expect("every resource has an id").parse().expect("numeric id");
        assert!(id > 0, "resource ids are positive");
        assert!(!ids.contains(&id), "resource id {id} used twice");
        ids.push(id);
        if n.has_tag_name("basematerials") {
            materials_at = Some(i);
            for b in n.children().filter(|c| c.is_element()) {
                assert!(b.has_tag_name("base"));
                let name = b.attribute("name").expect("`name` is required on <base>");
                assert!(!name.is_empty());
                let c = b.attribute("displaycolor").expect("`displaycolor` is required on <base>");
                assert!(c.starts_with('#') && (c.len() == 7 || c.len() == 9), "bad colour {c:?}");
                assert!(c[1..].chars().all(|c| c.is_ascii_hexdigit()), "bad colour {c:?}");
                material_count += 1;
            }
        }
    }

    let mut objects = 0usize;
    for (i, o) in resources.children().filter(|n| n.has_tag_name("object")).enumerate() {
        objects += 1;
        assert_eq!(o.attribute("type").unwrap_or("model"), "model");
        let mesh = o.children().find(|n| n.has_tag_name("mesh")).expect("<mesh>");
        let verts: Vec<()> = mesh
            .children()
            .find(|n| n.has_tag_name("vertices"))
            .expect("<vertices>")
            .children()
            .filter(|n| n.has_tag_name("vertex"))
            .map(|v| {
                for a in ["x", "y", "z"] {
                    v.attribute(a).expect("x/y/z").parse::<f64>().expect("a number");
                }
            })
            .collect();
        let tris: Vec<_> =
            mesh.children().find(|n| n.has_tag_name("triangles")).expect("<triangles>").children().filter(|n| n.has_tag_name("triangle")).collect();
        assert!(!tris.is_empty(), "an object with no triangles is not a solid");

        let has_props = tris.iter().any(|t| t.attribute("p1").is_some());
        if has_props {
            // "If this object contains any triangles with assigned
            // properties, the object MUST specify pid and pindex."
            let pid: u32 = o.attribute("pid").expect("pid on an object with painted triangles").parse().unwrap();
            assert!(ids.contains(&pid), "pid {pid} names no resource");
            let pindex: usize = o.attribute("pindex").expect("pindex").parse().unwrap();
            assert!(pindex < material_count, "pindex {pindex} is past the {material_count} materials");
            assert!(materials_at.is_some_and(|m| m < i + material_count.min(1)), "materials come first");
        }
        for t in &tris {
            let v: Vec<usize> =
                ["v1", "v2", "v3"].iter().map(|a| t.attribute(*a).expect(a).parse().expect("index")).collect();
            assert!(v.iter().all(|&i| i < verts.len()), "vertex index out of range: {v:?}");
            assert!(v[0] != v[1] && v[1] != v[2] && v[0] != v[2], "a triangle's vertices must differ");
            if let Some(p1) = t.attribute("p1") {
                let p: usize = p1.parse().expect("p1 is an index");
                assert!(p < material_count, "p1 {p} is past the {material_count} materials");
                // For a basematerials group p1/p2/p3 must be equal or absent.
                for a in ["p2", "p3"] {
                    assert!(t.attribute(a).is_none_or(|x| x == p1), "{a} must equal p1 for base materials");
                }
            }
        }
    }
    assert!(objects > 0);

    let items: Vec<_> = build.children().filter(|n| n.has_tag_name("item")).collect();
    assert!(!items.is_empty(), "<build> must place something");
    for it in items {
        let id: u32 = it.attribute("objectid").expect("objectid").parse().unwrap();
        assert!(ids.contains(&id), "build item names object {id}, which is not a resource");
        let t: Vec<f64> =
            it.attribute("transform").expect("transform").split_whitespace().map(|n| n.parse().unwrap()).collect();
        assert_eq!(t.len(), 12, "the 3MF transform is 12 row-major numbers");
    }
}

#[test]
fn the_model_satisfies_the_core_specification_structure() {
    let (a, b) = (box_mesh(Vec3::ZERO, Vec3::new(10.0, 10.0, 4.0)), box_mesh(Vec3::new(0.0, 0.0, 4.0), Vec3::new(10.0, 10.0, 6.0)));
    for slicer in [Slicer::Core, Slicer::Prusa, Slicer::Bambu] {
        let w = write(&two_body_scene(&a, &b, slicer));
        let xml = model_of(&w.bytes);
        validate_core(&xml);
        // The rule the structure checks above do not reach: every edge is
        // shared by exactly two triangles, once in each direction. (Only under
        // the default grouping -- merging bodies into one object is how a
        // multi-material part is made, and those bodies share faces.)
        assert!(
            edge_uses(&xml).iter().all(|&(f, b)| (f, b) == (1, 1)),
            "{slicer:?}: every edge exactly twice, once each way",
        );
    }
}

/// Every undirected edge of every object in the model, with how many
/// triangles traverse it each way. The 3MF core specification's structural
/// rule: exactly two, once in each direction.
pub(crate) fn edge_uses(xml: &str) -> Vec<(usize, usize)> {
    let doc = roxmltree::Document::parse(xml).expect("well-formed");
    let mut out = Vec::new();
    for o in doc.descendants().filter(|n| n.has_tag_name("object")) {
        let mut edges: std::collections::HashMap<(u32, u32), (usize, usize)> = Default::default();
        for t in o.descendants().filter(|n| n.has_tag_name("triangle")) {
            let v: Vec<u32> =
                ["v1", "v2", "v3"].iter().map(|a| t.attribute(*a).unwrap().parse().unwrap()).collect();
            for k in 0..3 {
                let (a, b) = (v[k], v[(k + 1) % 3]);
                let e = edges.entry(if a < b { (a, b) } else { (b, a) }).or_default();
                if a < b { e.0 += 1 } else { e.1 += 1 }
            }
        }
        out.extend(edges.into_values());
    }
    out
}

#[test]
fn a_back_to_back_triangle_pair_does_not_reach_the_file() {
    // The mesh kernel can emit these: the same three vertices twice, wound
    // opposite ways. Written as they are, two edges of the model would be
    // used by four triangles each, which the 3MF core specification forbids.
    let mut a = box_mesh(Vec3::ZERO, Vec3::new(10.0, 10.0, 10.0));
    let (p, q, r) = (Vec3::ZERO, Vec3::new(10.0, 0.0, 0.0), Vec3::new(10.0, 0.0, 10.0));
    push_tri(&mut a, p, q, r, 7);
    push_tri(&mut a, p, r, q, 7); // the same three corners, the other way round

    let w = write(&Scene { parts: vec![Part::new("b", &a, 1)], palette: palette(), ..Scene::default() });
    let xml = model_of(&w.bytes);
    validate_core(&xml);
    assert_eq!(xml.matches("<triangle ").count(), 12, "the box, and neither of the pair");
    assert!(
        edge_uses(&xml).iter().all(|&(f, b)| (f, b) == (1, 1)),
        "every edge exactly twice, once each way: {:?}",
        edge_uses(&xml).iter().filter(|&&(f, b)| (f, b) != (1, 1)).collect::<Vec<_>>(),
    );
    assert!(
        w.warnings.iter().any(|m| m.contains("repeated another's three vertices")),
        "and it says so: {:?}",
        w.warnings,
    );
}

#[test]
fn an_exact_duplicate_triangle_is_written_once() {
    // Same vertices, same winding. Keeping both gives every one of its edges
    // three uses; one of them is the surface.
    let mut a = box_mesh(Vec3::ZERO, Vec3::new(10.0, 10.0, 10.0));
    let n = a.triangle_count();
    let i = &a.indices[(n - 1) * 3..n * 3].to_vec();
    a.indices.extend_from_slice(i);
    a.face_ids.push(*a.face_ids.last().unwrap());

    let w = write(&Scene { parts: vec![Part::new("b", &a, 1)], palette: palette(), ..Scene::default() });
    let xml = model_of(&w.bytes);
    assert_eq!(xml.matches("<triangle ").count(), 12);
    assert!(edge_uses(&xml).iter().all(|&(f, b)| (f, b) == (1, 1)), "still manifold");
}

#[test]
fn a_body_that_is_not_a_closed_surface_is_reported() {
    // A box with its top two triangles missing: the file is still written --
    // a slicer can often repair one -- but the report says it is holed.
    let mut a = box_mesh(Vec3::ZERO, Vec3::new(10.0, 10.0, 10.0));
    a.indices.truncate((a.triangle_count() - 2) * 3);
    a.face_ids.truncate(a.face_ids.len() - 2);
    let w = write(&Scene { parts: vec![Part::new("lid_off", &a, 1)], palette: palette(), ..Scene::default() });
    assert!(!w.bytes.is_empty(), "the file is still written");
    assert!(
        w.warnings.iter().any(|m| m.contains("lid_off") && m.contains("boundary edge")),
        "{:?}",
        w.warnings,
    );
}

#[test]
fn a_tessellated_box_is_welded_to_eight_vertices_and_twelve_triangles() {
    let a = box_mesh(Vec3::ZERO, Vec3::new(10.0, 10.0, 10.0));
    assert_eq!(a.vertex_count(), 36, "the fixture really does repeat its corners");
    let w = write(&Scene { parts: vec![Part::new("b", &a, 1)], palette: palette(), ..Scene::default() });
    let xml = model_of(&w.bytes);
    assert_eq!(xml.matches("<vertex ").count(), 8);
    assert_eq!(xml.matches("<triangle ").count(), 12);
}

// ---------------------------------------------------------------------------
// Colour
// ---------------------------------------------------------------------------

#[test]
fn per_body_colour_lands_on_the_object_and_in_the_material_group() {
    let (a, b) = (box_mesh(Vec3::ZERO, Vec3::new(10.0, 10.0, 4.0)), box_mesh(Vec3::new(0.0, 0.0, 4.0), Vec3::new(10.0, 10.0, 6.0)));
    let xml = model_of(&write(&two_body_scene(&a, &b, Slicer::Prusa)).bytes);
    let doc = roxmltree::Document::parse(&xml).unwrap();
    let bases: Vec<_> = doc.descendants().filter(|n| n.has_tag_name("base")).collect();
    assert_eq!(bases.len(), 2);
    assert_eq!(bases[0].attribute("name"), Some("shell"));
    assert_eq!(bases[0].attribute("displaycolor"), Some("#C98A6B"));
    // An unnamed filament is named after its colour, because `name` is
    // required and "" is not a name.
    assert_eq!(bases[1].attribute("name"), Some("#0078BF"));

    let objs: Vec<_> = doc.descendants().filter(|n| n.has_tag_name("object")).collect();
    assert_eq!(objs.len(), 2);
    assert_eq!(objs[0].attribute("pindex"), Some("0"), "slot 1 is base index 0");
    assert_eq!(objs[1].attribute("pindex"), Some("1"), "slot 2 is base index 1");
}

#[test]
fn per_face_paint_survives_the_round_trip() {
    let mut a = box_mesh(Vec3::ZERO, Vec3::new(10.0, 10.0, 10.0));
    a.face_ids = a.face_ids.clone();
    // Paint the two triangles of the +Z face (the last two) with slot 2.
    let n = a.triangle_count();
    let mut tri_slots = vec![0u32; n];
    tri_slots[n - 2] = 2;
    tri_slots[n - 1] = 2;
    let scene = Scene {
        parts: vec![Part { name: "b".into(), mesh: &a, slot: 1, tri_slots, items: Vec::new(), layer: String::new() }],
        palette: palette(),
        slicer: Slicer::Bambu,
        ..Scene::default()
    };
    let xml = model_of(&write(&scene).bytes);
    validate_core(&xml);
    let doc = roxmltree::Document::parse(&xml).unwrap();
    let tris: Vec<_> = doc.descendants().filter(|n| n.has_tag_name("triangle")).collect();
    assert_eq!(tris.len(), 12);
    let painted: Vec<_> = tris.iter().filter(|t| t.attribute("p1").is_some()).collect();
    assert_eq!(painted.len(), 2, "exactly the two triangles we painted");
    for t in &painted {
        // Core: the material index.
        assert_eq!(t.attribute("p1"), Some("1"));
        // PrusaSlicer: namespaced, so it stays inside the core schema.
        assert_eq!(t.attribute((PRUSA_NS, "mmu_segmentation")), Some("8"));
        // Bambu/Orca: the same string under an unprefixed name.
        assert_eq!(t.attribute("paint_color"), Some("8"));
        assert_eq!(paint::decode_whole_triangle("8"), Some(2));
    }
    for t in tris.iter().filter(|t| t.attribute("p1").is_none()) {
        assert!(t.attribute("paint_color").is_none(), "an unpainted triangle carries no paint data");
    }
}

#[test]
fn the_layers_are_exactly_what_the_slicer_argument_asks_for() {
    let a = box_mesh(Vec3::ZERO, Vec3::new(10.0, 10.0, 10.0));
    let b = box_mesh(Vec3::new(0.0, 0.0, 10.0), Vec3::new(10.0, 10.0, 12.0));

    let core = write(&two_body_scene(&a, &b, Slicer::Core));
    let z = open(&core.bytes);
    assert_eq!(z.len(), 3, "core only: content types, rels, model — {:?}", z.names);
    let xml = z.text(MODEL_PART).unwrap();
    assert!(!xml.contains("slic3rpe"), "no vendor namespace in a core file");
    assert!(!xml.contains("paint_color"));
    assert!(xml.contains("<base "), "but the colours are still there");

    let prusa = write(&two_body_scene(&a, &b, Slicer::Prusa));
    let z = open(&prusa.bytes);
    assert!(z.get(PRUSA_MODEL_CONFIG).is_some());
    assert!(z.get(PRUSA_PRINT_CONFIG).is_none(), "never the print profile");
    assert!(z.get(BAMBU_MODEL_CONFIG).is_none(), "bambu is opt-in");
    let xml = z.text(MODEL_PART).unwrap();
    assert!(xml.contains("slic3rpe:Version3mf"), "the project marker");
    assert!(!xml.contains("paint_color"));

    let bambu = write(&two_body_scene(&a, &b, Slicer::Bambu));
    let z = open(&bambu.bytes);
    assert!(z.get(BAMBU_MODEL_CONFIG).is_some());
    assert!(z.get(BAMBU_PROJECT_CONFIG).is_some());
    assert!(z.get(BAMBU_SLICE_INFO).is_some());
    assert!(z.get(PRUSA_MODEL_CONFIG).is_some(), "bambu keeps the prusa layer: one file, both slicers");
    assert!(z.text(MODEL_PART).unwrap().contains("BambuStudio:3mfVersion"));
}

/// No layer writes PrusaSlicer's print profile part. A stub of it (a couple
/// of `;` lines naming the colours) is what made Bambu Studio refuse a
/// default export as "invalid config": a slicer loads that part as the whole
/// print profile. Every `.config` part that is written is one we fill
/// completely: XML for the per-object configs, JSON for Bambu's project.
#[test]
fn no_layer_writes_a_stub_print_profile() {
    let a = box_mesh(Vec3::ZERO, Vec3::new(10.0, 10.0, 10.0));
    let b = box_mesh(Vec3::new(0.0, 0.0, 10.0), Vec3::new(10.0, 10.0, 12.0));
    for slicer in [Slicer::Core, Slicer::Prusa, Slicer::Bambu, Slicer::default()] {
        let z = open(&write(&two_body_scene(&a, &b, slicer)).bytes);
        assert!(z.get(PRUSA_PRINT_CONFIG).is_none(), "{slicer:?}: {:?}", z.names);
        for name in z.names.iter().filter(|n| n.ends_with(".config")) {
            let text = z.text(name).unwrap();
            if name.as_str() == BAMBU_PROJECT_CONFIG {
                assert!(text.trim_start().starts_with('{'), "{slicer:?} {name}: JSON: {text}");
            } else {
                roxmltree::Document::parse(&text).unwrap_or_else(|e| panic!("{slicer:?} {name}: not XML ({e}): {text}"));
            }
            assert!(!text.lines().all(|l| l.trim().is_empty() || l.starts_with(';')), "{slicer:?} {name}: a comment-only stub");
        }
    }
}

#[test]
fn the_slicer_configs_carry_the_filament_of_each_body() {
    let (a, b) = (box_mesh(Vec3::ZERO, Vec3::new(10.0, 10.0, 4.0)), box_mesh(Vec3::new(0.0, 0.0, 4.0), Vec3::new(10.0, 10.0, 6.0)));
    let z = open(&write(&two_body_scene(&a, &b, Slicer::Bambu)).bytes);

    let prusa = z.text(PRUSA_MODEL_CONFIG).unwrap();
    assert!(prusa.contains("<volume firstid=\"0\" lastid=\"11\">"), "{prusa}");
    assert!(prusa.contains("key=\"extruder\" value=\"2\""), "{prusa}");
    assert!(prusa.contains("value=\"ModelPart\""), "{prusa}");

    let bambu = z.text(BAMBU_MODEL_CONFIG).unwrap();
    assert!(bambu.contains("<part id=\"1\" subtype=\"normal_part\">"), "{bambu}");
    assert!(bambu.contains("<metadata key=\"extruder\" value=\"2\"/>"), "{bambu}");

    let project = z.text(BAMBU_PROJECT_CONFIG).unwrap();
    assert!(project.contains("\"filament_colour\": [\"#C98A6B\", \"#0078BF\"]"), "{project}");
}

#[test]
fn grouping_part_makes_one_object_with_one_volume_per_body() {
    let (a, b) = (box_mesh(Vec3::ZERO, Vec3::new(10.0, 10.0, 4.0)), box_mesh(Vec3::new(0.0, 0.0, 4.0), Vec3::new(10.0, 10.0, 6.0)));
    let scene = Scene { group: Grouping::Part, ..two_body_scene(&a, &b, Slicer::Bambu) };
    let w = write(&scene);
    let xml = model_of(&w.bytes);
    validate_core(&xml);
    let doc = roxmltree::Document::parse(&xml).unwrap();
    assert_eq!(doc.descendants().filter(|n| n.has_tag_name("object")).count(), 1);
    assert_eq!(doc.descendants().filter(|n| n.has_tag_name("item")).count(), 1);
    assert_eq!(doc.descendants().filter(|n| n.has_tag_name("triangle")).count(), 24);
    // The second body's triangles carry the second filament, which is how a
    // slicer that ignores `p1` still prints it in the right colour.
    let painted =
        doc.descendants().filter(|n| n.has_tag_name("triangle") && n.attribute("paint_color") == Some("8")).count();
    assert_eq!(painted, 12);

    let z = open(&w.bytes);
    let prusa = z.text(PRUSA_MODEL_CONFIG).unwrap();
    assert!(prusa.contains("firstid=\"0\" lastid=\"11\""), "{prusa}");
    assert!(prusa.contains("firstid=\"12\" lastid=\"23\""), "{prusa}");
}

#[test]
fn more_colours_than_slots_is_a_warning_not_a_failure() {
    let a = box_mesh(Vec3::ZERO, Vec3::new(10.0, 10.0, 10.0));
    let mut palette = palette();
    palette.push(Filament { name: "third".into(), color: [0.1, 0.8, 0.2] });
    let w = write(&Scene {
        parts: vec![Part::new("b", &a, 1)],
        palette,
        machine_slots: 2,
        ..Scene::default()
    });
    assert_eq!(w.warnings.len(), 1);
    assert!(w.warnings[0].contains("3 distinct colours"), "{:?}", w.warnings);
    assert!(!w.bytes.is_empty(), "the file is still written");
}

#[test]
fn names_with_xml_metacharacters_are_escaped() {
    let a = box_mesh(Vec3::ZERO, Vec3::new(5.0, 5.0, 5.0));
    let w = write(&Scene {
        title: "a & b".into(),
        parts: vec![Part::new("<lid> \"1\"", &a, 1)],
        palette: palette(),
        ..Scene::default()
    });
    let xml = model_of(&w.bytes);
    validate_core(&xml);
    let doc = roxmltree::Document::parse(&xml).unwrap();
    let o = doc.descendants().find(|n| n.has_tag_name("object")).unwrap();
    assert_eq!(o.attribute("name"), Some("<lid> \"1\""));
}

/// A body's display layer is its object's `cadpole:layer` metadata, in
/// core 1.2's object-level `<metadatagroup>` before the mesh, under a
/// namespace declared only when a layer is written.
#[test]
fn a_layer_is_object_metadata_in_its_own_namespace() {
    let a = box_mesh(Vec3::ZERO, Vec3::new(5.0, 5.0, 5.0));
    let mut on = Part::new("deck", &a, 1);
    on.layer = "site & yard".into();
    let w = write(&Scene { parts: vec![on, Part::new("loose", &a, 1)], palette: palette(), ..Scene::default() });
    let xml = model_of(&w.bytes);
    validate_core(&xml);
    let doc = roxmltree::Document::parse(&xml).unwrap();
    let objects: Vec<_> = doc.descendants().filter(|n| n.has_tag_name("object")).collect();
    let group = objects[0].first_element_child().expect("a first child");
    assert!(group.has_tag_name("metadatagroup"), "the group comes before the mesh");
    let m = group.first_element_child().unwrap();
    assert_eq!(m.attribute("name"), Some("cadpole:layer"));
    assert_eq!(m.text(), Some("site & yard"));
    assert_eq!(doc.root_element().lookup_namespace_uri(Some("cadpole")), Some(crate::CADPOLE_NS));
    assert!(objects[1].children().all(|n| !n.has_tag_name("metadatagroup")), "no layer, no group");
    // A file with no layers does not declare the namespace at all.
    let plain = model_of(&write(&Scene { parts: vec![Part::new("b", &a, 1)], palette: palette(), ..Scene::default() }).bytes);
    assert!(!plain.contains("xmlns:cadpole"));
}

#[test]
fn parsing_the_slicer_and_grouping_arguments() {
    assert_eq!(Slicer::parse("bambu").unwrap(), Slicer::Bambu);
    assert_eq!(Slicer::parse("OrcaSlicer").unwrap(), Slicer::Bambu);
    assert_eq!(Slicer::parse("prusa").unwrap(), Slicer::Prusa);
    assert_eq!(Slicer::parse("core").unwrap(), Slicer::Core);
    assert!(Slicer::parse("cura").unwrap_err().contains("cura"));
    assert_eq!(Grouping::parse("part").unwrap(), Grouping::Part);
    assert!(Grouping::parse("volume").is_err());
}
