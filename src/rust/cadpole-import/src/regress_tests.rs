//! Regression tests for reader and writer defects, one section per defect,
//! each named for what was wrong.

use cadpole_export_3mf::zip::ZipWriter;

use crate::mesh::RawObject;
use crate::prescan::{MAX_TRIANGLES, MAX_VERTICES};
use crate::threemf;

// ---------------------------------------------------------------- 3MF

/// A package with `model` as its only (root) model part.
fn package(model: &str) -> Vec<u8> {
    let mut z = ZipWriter::new();
    z.add("3D/3dmodel.model", model.as_bytes());
    z.finish()
}

fn model(unit: &str, resources: &str, build: &str) -> String {
    format!(
        "<?xml version=\"1.0\" encoding=\"UTF-8\"?>\n<model unit=\"{unit}\" \
         xmlns=\"http://schemas.microsoft.com/3dmanufacturing/core/2015/02\">\
         <resources>{resources}</resources><build>{build}</build></model>"
    )
}

/// A unit tetrahedron as a 3MF `<object>`.
fn tetra_object(id: u32) -> String {
    format!(
        "<object id=\"{id}\" type=\"model\"><mesh><vertices>\
         <vertex x=\"0\" y=\"0\" z=\"0\"/><vertex x=\"1\" y=\"0\" z=\"0\"/>\
         <vertex x=\"0\" y=\"1\" z=\"0\"/><vertex x=\"0\" y=\"0\" z=\"1\"/></vertices>\
         <triangles><triangle v1=\"0\" v2=\"2\" v3=\"1\"/><triangle v1=\"0\" v2=\"1\" v3=\"3\"/>\
         <triangle v1=\"1\" v2=\"2\" v3=\"3\"/><triangle v1=\"0\" v2=\"3\" v3=\"2\"/></triangles></mesh></object>"
    )
}

fn bbox(o: &RawObject) -> ([f64; 3], [f64; 3]) {
    let mut lo = [f64::INFINITY; 3];
    let mut hi = [f64::NEG_INFINITY; 3];
    for p in &o.positions {
        for k in 0..3 {
            lo[k] = lo[k].min(p[k]);
            hi[k] = hi[k].max(p[k]);
        }
    }
    (lo, hi)
}

fn near3(a: [f64; 3], b: [f64; 3]) -> bool {
    (0..3).all(|k| (a[k] - b[k]).abs() < 1e-9)
}

/// A kilobyte that names a million placements: eight levels of objects,
/// each `fanout` components of the one below, over a mesh with vertices and
/// no triangles. The walk used to visit every leaf (fanout^8) and clone its
/// vertices before noticing the build had no triangles: 16.8 M visits and
/// 3 GB for fanout 8. It must stop at the placement budget instead.
#[test]
fn a_component_bomb_over_an_empty_mesh_is_refused_at_the_placement_budget() {
    let fanout = 6; // 6^8 = 1.68 M leaf placements
    let mut res = String::from(
        "<object id=\"1\" type=\"model\"><mesh><vertices>\
         <vertex x=\"0\" y=\"0\" z=\"0\"/><vertex x=\"1\" y=\"0\" z=\"0\"/>\
         <vertex x=\"0\" y=\"1\" z=\"0\"/><vertex x=\"0\" y=\"0\" z=\"1\"/></vertices>\
         <triangles/></mesh></object>",
    );
    for id in 2..=9 {
        res.push_str(&format!("<object id=\"{id}\" type=\"model\"><components>"));
        for _ in 0..fanout {
            res.push_str(&format!("<component objectid=\"{}\"/>", id - 1));
        }
        res.push_str("</components></object>");
    }
    let bytes = package(&model("millimeter", &res, "<item objectid=\"9\"/>"));
    assert!(bytes.len() < 2048, "{} bytes", bytes.len());
    let err = threemf::parse(&bytes, "bomb.3mf").map(|(o, _)| o.len()).unwrap_err();
    assert!(err.contains("component placements"), "{err}");
}

/// Vertices count too: a leaf with many vertices and no triangles, placed
/// often enough, is memory without a triangle to show for it.
#[test]
fn placed_vertices_are_counted_across_the_build() {
    let n = 200_000; // vertices in the leaf
    let mut verts = String::with_capacity(n * 40);
    for i in 0..n {
        verts.push_str(&format!("<vertex x=\"{i}\" y=\"0\" z=\"0\"/>"));
    }
    let mut res = format!("<object id=\"1\" type=\"model\"><mesh><vertices>{verts}</vertices><triangles/></mesh></object>");
    res.push_str("<object id=\"2\" type=\"model\"><components>");
    for _ in 0..60 {
        res.push_str("<component objectid=\"1\"/>");
    }
    res.push_str("</components></object>");
    let err = threemf::parse(&package(&model("millimeter", &res, "<item objectid=\"2\"/>")), "v.3mf").map(|(o, _)| o.len()).unwrap_err();
    assert!(err.contains(&format!("more than {MAX_VERTICES} vertices in the build")), "{err}");
}

/// The triangle cap is the file's, not each build item's: one object of
/// half a million triangles placed by eleven items is 5.5 M triangles, over
/// the cap, exactly as eleven components under one item already were.
#[test]
fn the_triangle_cap_counts_every_build_item() {
    let per = 500_000;
    let items = MAX_TRIANGLES / per + 1;
    // Distinct vertices and triangles, so the part does not deflate past
    // the zip reader's 100:1 ratio cap.
    let mut res = String::with_capacity(per * 80);
    res.push_str("<object id=\"1\" type=\"model\"><mesh><vertices>");
    for i in 0..per + 2 {
        res.push_str(&format!("<vertex x=\"{i}\" y=\"{}\" z=\"0\"/>", i % 7));
    }
    res.push_str("</vertices><triangles>");
    for i in 0..per {
        res.push_str(&format!("<triangle v1=\"{i}\" v2=\"{}\" v3=\"{}\"/>", i + 1, i + 2));
    }
    res.push_str("</triangles></mesh></object>");
    let build: String = (0..items).map(|_| "<item objectid=\"1\"/>").collect();
    let bytes = package(&model("millimeter", &res, &build));
    let err = threemf::parse(&bytes, "items.3mf").map(|(o, _)| o.len()).unwrap_err();
    assert!(err.contains(&format!("more than {MAX_TRIANGLES} triangles in the build")), "{err}");
}

/// `<model unit>` is the unit of every length in the part, a transform's
/// translation included: an inch tetrahedron moved 10 along x by its item
/// sits at 254 mm, not at 10 mm with an inch-sized body.
#[test]
fn the_model_unit_scales_item_and_component_translations() {
    let item = model("inch", &tetra_object(1), "<item objectid=\"1\" transform=\"1 0 0 0 1 0 0 0 1 10 0 0\"/>");
    let comp = model(
        "inch",
        &format!(
            "{}<object id=\"2\" type=\"model\"><components>\
             <component objectid=\"1\" transform=\"1 0 0 0 1 0 0 0 1 10 0 0\"/></components></object>",
            tetra_object(1)
        ),
        "<item objectid=\"2\"/>",
    );
    // Both: a component moved 5 inches and its item another 5.
    let both = model(
        "inch",
        &format!(
            "{}<object id=\"2\" type=\"model\"><components>\
             <component objectid=\"1\" transform=\"1 0 0 0 1 0 0 0 1 5 0 0\"/></components></object>",
            tetra_object(1)
        ),
        "<item objectid=\"2\" transform=\"1 0 0 0 1 0 0 0 1 5 0 0\"/>",
    );
    for (what, m) in [("item", &item), ("component", &comp), ("both", &both)] {
        let (objs, _) = threemf::parse(&package(m), "inch.3mf").unwrap_or_else(|e| panic!("{what}: {e}"));
        let (lo, hi) = bbox(&objs[0]);
        assert!(near3(lo, [254.0, 0.0, 0.0]) && near3(hi, [279.4, 25.4, 25.4]), "{what}: {lo:?} {hi:?}");
    }
    // `units=` replaces the declared unit for translations too.
    let (objs, _) = threemf::parse_with_unit(&package(&item), "inch.3mf", Some(10.0)).unwrap();
    let (lo, hi) = bbox(&objs[0]);
    assert!(near3(lo, [100.0, 0.0, 0.0]) && near3(hi, [110.0, 10.0, 10.0]), "units=: {lo:?} {hi:?}");
    // A rotation is unitless: a quarter turn about z then a 10-inch move.
    let rot = model("inch", &tetra_object(1), "<item objectid=\"1\" transform=\"0 1 0 -1 0 0 0 0 1 10 0 0\"/>");
    let (objs, _) = threemf::parse(&package(&rot), "rot.3mf").unwrap();
    let (lo, hi) = bbox(&objs[0]);
    assert!(near3(lo, [228.6, 0.0, 0.0]) && near3(hi, [254.0, 25.4, 25.4]), "rotated: {lo:?} {hi:?}");
}

// ---------------------------------------------------------------- STEP units

/// A 10 mm-to-4 mm, 15 mm-high truncated cone whose semi-angle (21.8014
/// degrees) is written in a `DEGREE` unit, the one
/// `GLOBAL_UNIT_ASSIGNED_CONTEXT` assigns, with the SI radian it is defined
/// from at a lower entity id (#8) — the usual layout. The reader used to
/// take the first plane-angle unit by id and read the angle as radians.
const DEGREE_CONE: &str = r#"ISO-10303-21;
HEADER;
FILE_DESCRIPTION((''), '2;1');
FILE_NAME('cone.step', '2026-01-01T00:00:00', (''), (''), '', '', '');
FILE_SCHEMA(('AUTOMOTIVE_DESIGN { 1 0 10303 214 1 1 1 1 }'));
ENDSEC;
DATA;
#10 = ( LENGTH_UNIT() NAMED_UNIT(*) SI_UNIT(.MILLI., .METRE.) );
#8 = ( NAMED_UNIT(*) PLANE_ANGLE_UNIT() SI_UNIT($, .RADIAN.) );
#11 = ( CONVERSION_BASED_UNIT('DEGREE', #902) NAMED_UNIT(#901) PLANE_ANGLE_UNIT() );
#901 = DIMENSIONAL_EXPONENTS(0.,0.,0.,0.,0.,0.,0.);
#902 = PLANE_ANGLE_MEASURE_WITH_UNIT(PLANE_ANGLE_MEASURE(0.0174532925199433), #8);
#12 = ( NAMED_UNIT(*) SI_UNIT($, .STERADIAN.) SOLID_ANGLE_UNIT() );
#13 = UNCERTAINTY_MEASURE_WITH_UNIT(LENGTH_MEASURE(1.0E-6), #10, 'distance_accuracy_value', '');
#14 = ( GEOMETRIC_REPRESENTATION_CONTEXT(3) GLOBAL_UNCERTAINTY_ASSIGNED_CONTEXT((#13)) GLOBAL_UNIT_ASSIGNED_CONTEXT((#10, #11, #12)) REPRESENTATION_CONTEXT('', '') );
#16 = ADVANCED_BREP_SHAPE_REPRESENTATION('cone', (#70), #14);
#20 = CARTESIAN_POINT('', (0., 0., 0.));
#21 = CARTESIAN_POINT('', (0., 0., 15.));
#22 = DIRECTION('', (0., 0., 1.));
#23 = DIRECTION('', (0., 0., -1.));
#24 = DIRECTION('', (1., 0., 0.));
#25 = AXIS2_PLACEMENT_3D('', #20, #22, #24);
#26 = AXIS2_PLACEMENT_3D('', #21, #22, #24);
#27 = AXIS2_PLACEMENT_3D('', #20, #23, #24);
#30 = CARTESIAN_POINT('', (10., 0., 0.));
#31 = CARTESIAN_POINT('', (4.0, 0., 15.));
#32 = VERTEX_POINT('', #30);
#33 = VERTEX_POINT('', #31);
#34 = CIRCLE('', #25, 10.);
#35 = CIRCLE('', #26, 4.0);
#36 = VECTOR('', #37, 1.);
#37 = DIRECTION('', (-0.3713906763541038, 0., 0.9284766908852594));
#38 = LINE('', #30, #36);
#40 = EDGE_CURVE('', #32, #32, #34, .T.);
#41 = EDGE_CURVE('', #33, #33, #35, .T.);
#42 = EDGE_CURVE('', #32, #33, #38, .T.);
#43 = ORIENTED_EDGE('', *, *, #40, .F.);
#44 = EDGE_LOOP('', (#43));
#45 = FACE_OUTER_BOUND('', #44, .T.);
#46 = PLANE('', #27);
#47 = ADVANCED_FACE('', (#45), #46, .T.);
#48 = ORIENTED_EDGE('', *, *, #41, .T.);
#49 = EDGE_LOOP('', (#48));
#50 = FACE_OUTER_BOUND('', #49, .T.);
#51 = PLANE('', #26);
#52 = ADVANCED_FACE('', (#50), #51, .T.);
#53 = CONICAL_SURFACE('', #27, 10., 21.80140948635181);
#54 = ORIENTED_EDGE('', *, *, #40, .T.);
#55 = ORIENTED_EDGE('', *, *, #42, .T.);
#56 = ORIENTED_EDGE('', *, *, #41, .F.);
#57 = ORIENTED_EDGE('', *, *, #42, .F.);
#58 = EDGE_LOOP('', (#54, #55, #56, #57));
#59 = FACE_OUTER_BOUND('', #58, .T.);
#60 = ADVANCED_FACE('', (#59), #53, .T.);
#69 = CLOSED_SHELL('', (#47, #52, #60));
#70 = MANIFOLD_SOLID_BREP('cone', #69);
ENDSEC;
END-ISO-10303-21;
"#;

#[test]
fn the_plane_angle_unit_is_the_one_the_context_assigns() {
    let im = crate::step::import_unbounded(DEGREE_CONE, &crate::step::Options::default())
        .unwrap_or_else(|e| panic!("the degree cone: {e}"));
    assert_eq!(im.solids.len(), 1);
    // A frustum: pi h (R^2 + R r + r^2) / 3.
    let want = std::f64::consts::PI * 15.0 * (100.0 + 40.0 + 16.0) / 3.0;
    let got = cadpole_core::tessellate::tessellate(&im.solids[0].solid, 0.01).unwrap().0.volume();
    assert!((got - want).abs() / want < 0.01, "volume {got}, want {want}");
}

// ---------------------------------------------------------------- STEP strings

fn step_of(name: &str) -> String {
    let s = cadpole_core::sketch::Sketch::new(cadpole_core::geom::Plane::XY)
        .with_loop(cadpole_core::sketch::Loop::rect(cadpole_core::sketch::P2::new(0.0, 0.0), 10.0, 10.0));
    let solid = cadpole_core::kernel::extrude(&s, 10.0).unwrap().remove(0);
    let opts = cadpole_export_step::Options { tolerance: 1e-4, product_name: name.into(), ..Default::default() };
    cadpole_export_step::write(&[cadpole_export_step::Part { name, solid: &solid }], &opts).unwrap()
}

/// Part 21 writes an apostrophe in a string as `''`, and Cadpole's own
/// writer does; the lexer took the first `'` as the end of the string and
/// refused the file ("Error while tokenizing ... DATA;"), so a body named
/// `it's` did not survive its own export.
#[test]
fn a_doubled_apostrophe_in_a_step_string_reads_as_one_apostrophe() {
    let text = step_of("it's");
    assert!(text.contains("'it''s'"), "the writer escapes it");
    let im = crate::step::import_unbounded(&text, &crate::step::Options::default()).unwrap_or_else(|e| panic!("{e}"));
    assert_eq!(im.solids.len(), 1);
    assert_eq!(im.solids[0].label, "it's");
    assert_eq!(crate::label::decode_step(&im.solids[0].raw_label), "it's");
    // In the header alone, and next to an empty string and a backslash.
    let plain = step_of("plain");
    let at = plain.find("FILE_NAME(").unwrap() + "FILE_NAME(".len();
    let end = plain[at..].find(',').unwrap() + at;
    let hdr = format!("{}'O''Brien \\\\ ''''.step'{}", &plain[..at], &plain[end..]);
    let im = crate::step::import_unbounded(&hdr, &crate::step::Options::default()).unwrap_or_else(|e| panic!("{e}"));
    assert_eq!(im.solids.len(), 1);
    // A comment with an apostrophe is not a string.
    let commented = plain.replacen("DATA;", "DATA;\n/* it's */", 1);
    crate::step::import_unbounded(&commented, &crate::step::Options::default()).unwrap_or_else(|e| panic!("comment: {e}"));
}

// ---------------------------------------------------------------- OBJ

/// A tetrahedron with an unreferenced fifth vertex far away, first line a
/// `v`: with a UTF-8 BOM in front, the first token was `\u{feff}v`, the line
/// was read past as an unknown keyword, and every index shifted by one —
/// silently, because the extra vertex took up the slack.
#[test]
fn a_byte_order_mark_does_not_hide_the_first_obj_line() {
    let plain = "v 0 0 0\nv 10 0 0\nv 0 10 0\nv 0 0 10\nv 20 20 20\nf 1 3 2\nf 1 2 4\nf 2 3 4\nf 1 4 3\n";
    let mut bom = vec![0xEF, 0xBB, 0xBF];
    bom.extend_from_slice(plain.as_bytes());
    let (a, _) = crate::obj::parse(plain.as_bytes(), "plain.obj").unwrap();
    let (b, _) = crate::obj::parse(&bom, "bom.obj").unwrap();
    assert_eq!(a[0].positions, b[0].positions);
    assert_eq!(bbox(&b[0]).1, [10.0, 10.0, 10.0]);
}

/// The STEP pre-scan already allowed a BOM before `ISO-10303-21;`, but the
/// lexer then failed at line 1.
#[test]
fn a_byte_order_mark_before_a_step_file_is_read_past() {
    let text = format!("\u{feff}{}", step_of("plain"));
    let im = crate::step::import_unbounded(&text, &crate::step::Options::default()).unwrap_or_else(|e| panic!("{e}"));
    assert_eq!(im.solids.len(), 1);
}

/// Twenty thousand one-triangle objects over two million vertices, most of
/// them unused: each object used to allocate and fill a remap table the
/// size of the file's vertex list, so the cost was objects x vertices
/// (4e10 writes here, about a minute in a debug build). It is linear now.
/// The cost is measured against the same triangles in one object, so the
/// machine's load cancels out: before, the ratio was 50 to 100.
#[test]
fn many_obj_objects_cost_what_they_use_not_objects_times_vertices() {
    let objects = 20_000;
    let vertices = 2_000_000;
    let text = |split: bool| {
        let mut s = String::with_capacity(vertices * 12 + objects * 40);
        for i in 0..vertices {
            s.push_str(&format!("v {} 0 {}\n", i % 1000, i / 1000));
        }
        if !split {
            s.push_str("o all\n");
        }
        for k in 0..objects {
            let a = 3 * k + 1;
            if split {
                s.push_str(&format!("o o{k}\n"));
            }
            s.push_str(&format!("f {a} {} {}\n", a + 1, a + 2));
        }
        s
    };
    let (one, many) = (text(false), text(true));
    let t = std::time::Instant::now();
    let (whole, _) = crate::obj::parse(one.as_bytes(), "one.obj").unwrap();
    let one_object = t.elapsed();
    let t = std::time::Instant::now();
    let (objs, _) = crate::obj::parse(many.as_bytes(), "many.obj").unwrap();
    let many_objects = t.elapsed();
    assert_eq!(whole.len(), 1);
    assert_eq!(whole[0].triangles.len(), objects);
    assert_eq!(objs.len(), objects);
    assert!(objs.iter().all(|o| o.positions.len() == 3 && o.triangles == vec![[0, 1, 2]]));
    assert_eq!(objs[1].positions[0], [3.0, 0.0, 0.0]);
    assert!(
        many_objects.as_secs_f64() < 5.0 * one_object.as_secs_f64(),
        "{objects} objects took {many_objects:?}, one object {one_object:?}"
    );
}

// ---------------------------------------------------------------- STL

const TETRA: [[[f32; 3]; 3]; 4] = [
    [[0.0, 0.0, 0.0], [0.0, 10.0, 0.0], [10.0, 0.0, 0.0]],
    [[0.0, 0.0, 0.0], [10.0, 0.0, 0.0], [0.0, 0.0, 10.0]],
    [[10.0, 0.0, 0.0], [0.0, 10.0, 0.0], [0.0, 0.0, 10.0]],
    [[0.0, 0.0, 0.0], [0.0, 0.0, 10.0], [0.0, 10.0, 0.0]],
];

fn binary_stl(header: &str) -> Vec<u8> {
    let mut b = vec![0u8; 80];
    b[..header.len()].copy_from_slice(header.as_bytes());
    b.extend_from_slice(&(TETRA.len() as u32).to_le_bytes());
    for t in TETRA {
        b.extend_from_slice(&[0u8; 12]);
        for p in t {
            for c in p {
                b.extend_from_slice(&c.to_le_bytes());
            }
        }
        b.extend_from_slice(&[0, 0]);
    }
    b
}

/// The text parser lower-cases every keyword, but the sniff compared the
/// first word with `solid` exactly, so an upper-case ASCII STL went to the
/// binary reader ("1480938578 triangles is over the limit").
#[test]
fn an_upper_case_ascii_stl_is_read_as_text() {
    let mut s = String::from("SOLID T\n");
    for t in TETRA {
        s.push_str("FACET NORMAL 0 0 0\nOUTER LOOP\n");
        for p in t {
            s.push_str(&format!("VERTEX {} {} {}\n", p[0], p[1], p[2]));
        }
        s.push_str("ENDLOOP\nENDFACET\n");
    }
    s.push_str("ENDSOLID T\n");
    let (objs, _) = crate::stl::parse(s.as_bytes(), "upper.stl").unwrap_or_else(|e| panic!("{e}"));
    assert_eq!(objs[0].triangles.len(), 4);
}

/// A binary STL with trailing bytes is read, with a note — unless its
/// header happened to start with `solid`, which sent it to the text parser
/// ("the ASCII STL has no facets"). The module's own rule is that binary is
/// recognised by its length, not its header.
#[test]
fn a_binary_stl_with_a_solid_header_and_trailing_bytes_is_binary() {
    for header in ["exported", "solid exported"] {
        let mut b = binary_stl(header);
        b.push(b'\n');
        let (objs, notes) = crate::stl::parse(&b, "trail.stl").unwrap_or_else(|e| panic!("{header}: {e}"));
        assert_eq!(objs[0].triangles.len(), 4, "{header}");
        assert!(notes.iter().any(|n| n.message.contains("1 trailing byte")), "{header}: {notes:?}");
    }
}

// ---------------------------------------------------------------- coordinate range

/// A finite double past f32's range became an infinite vertex in the stored
/// mesh: the import succeeded with an infinite bbox, a NaN volume and the
/// verdict "closed but not consistently wound". The finiteness check ran
/// on the f64 before the narrowing (and, for a placed mesh, before the
/// transform); it must hold on what is stored.
#[test]
fn a_coordinate_past_f32_range_is_refused_naming_the_vertex() {
    let opts = crate::mesh::Options::default();
    let obj = "v 0 0 0\nv 1e39 0 0\nv 0 10 0\nv 0 0 10\nf 1 3 2\nf 1 2 4\nf 2 3 4\nf 1 4 3\n";
    let err = crate::mesh::import_unbounded(obj.as_bytes(), "big.obj", &opts).map(|i| i.bodies.len()).unwrap_err();
    assert!(err.contains("at (1e39, 0.0, 0.0) mm") && err.contains("single-precision"), "{err}");
    // In range in the file, out of it once the item transform places it.
    let m = model("millimeter", &tetra_object(1), "<item objectid=\"1\" transform=\"1e300 0 0 0 1 0 0 0 1 0 0 0\"/>");
    let err = crate::mesh::import_unbounded(&package(&m), "big.3mf", &opts).map(|i| i.bodies.len()).unwrap_err();
    assert!(err.contains("single-precision"), "{err}");
}

// ---------------------------------------------------------------- the example scan

/// `examples/tools/scan_room_glb.py`, transcribed: the same arithmetic in
/// the same order, Python's round-half-to-even included, so it writes the
/// same bytes. Kept beside the script by its CRC (below).
mod scan_room {
    const W: f64 = 4.0;
    const D: f64 = 3.0;
    const H: f64 = 2.5;
    const DOOR: (f64, f64, f64) = (1.4, 2.3, 2.05);
    const WINDOW: (f64, f64, f64, f64) = (1.2, 2.8, 1.0, 2.0);
    const JITTER: f64 = 0.003;
    pub const GENERATOR: &str = "cadpole examples/tools/scan_room_glb.py";

    struct Lcg(u32);
    impl Lcg {
        fn unit(&mut self) -> f64 {
            self.0 = self.0.wrapping_mul(1664525).wrapping_add(1013904223);
            self.0 as f64 / 4294967296.0 * 2.0 - 1.0
        }
    }

    fn srgb_to_linear(c: f64) -> f64 {
        if c <= 0.04045 { c / 12.92 } else { cadpole_core::fmath::powf((c + 0.055) / 1.055, 2.4) }
    }

    fn colour(surface: &str, p: [f64; 3]) -> [f64; 3] {
        let [x, y, _] = p;
        if surface == "floor" {
            let plank = (x / 0.18) as i64;
            let shade = 0.9 + 0.1 * (((plank * 37) % 7) as f64) / 6.0;
            return [0.62 * shade, 0.43 * shade, 0.26 * shade];
        }
        if y < 0.1 {
            return [0.35, 0.33, 0.31];
        }
        if surface == "back" && WINDOW.0 <= x && x <= WINDOW.1 && WINDOW.2 <= y && y <= WINDOW.3 {
            return [0.62, 0.76, 0.9];
        }
        let k = 0.9 + 0.08 * (y / H).min(1.0);
        [0.93 * k, 0.9 * k, 0.84 * k]
    }

    struct Surface {
        name: &'static str,
        o: [f64; 3],
        ua: [i32; 3],
        va: [i32; 3],
        ul: f64,
        vl: f64,
        normal: [i32; 3],
        nu: usize,
        nv: usize,
        door: bool,
    }

    fn surfaces(n: f64) -> Vec<Surface> {
        let cu = |l: f64| ((l * n).round_ties_even() as usize).max(1);
        let s = |name, o, ua, va, ul: f64, vl: f64, normal, door| Surface { name, o, ua, va, ul, vl, normal, nu: cu(ul), nv: cu(vl), door };
        vec![
            s("floor", [0.0, 0.0, 0.0], [1, 0, 0], [0, 0, 1], W, D, [0, 1, 0], false),
            s("back", [0.0, 0.0, 0.0], [1, 0, 0], [0, 1, 0], W, H, [0, 0, 1], false),
            s("left", [0.0, 0.0, 0.0], [0, 0, 1], [0, 1, 0], D, H, [1, 0, 0], false),
            s("right", [W, 0.0, 0.0], [0, 0, 1], [0, 1, 0], D, H, [-1, 0, 0], false),
            s("front", [0.0, 0.0, D], [1, 0, 0], [0, 1, 0], W, H, [0, 0, -1], true),
        ]
    }

    fn cross(a: [i32; 3], b: [i32; 3]) -> [i32; 3] {
        [a[1] * b[2] - a[2] * b[1], a[2] * b[0] - a[0] * b[2], a[0] * b[1] - a[1] * b[0]]
    }

    fn build(n: f64) -> (Vec<f64>, Vec<f64>, Vec<u8>, Vec<u32>) {
        let mut rng = Lcg(210);
        let (mut pos, mut nrm, mut col, mut idx) = (Vec::new(), Vec::new(), Vec::new(), Vec::new());
        for s in surfaces(n) {
            let base = (pos.len() / 3) as u32;
            for j in 0..=s.nv {
                for i in 0..=s.nu {
                    let (u, v) = (s.ul * i as f64 / s.nu as f64, s.vl * j as f64 / s.nv as f64);
                    let mut p = [0.0; 3];
                    for k in 0..3 {
                        p[k] = s.o[k] + s.ua[k] as f64 * u + s.va[k] as f64 * v;
                    }
                    let interior = 0 < i && i < s.nu && 0 < j && j < s.nv;
                    let e = if interior { JITTER * rng.unit() } else { 0.0 };
                    for k in 0..3 {
                        p[k] += s.normal[k] as f64 * e;
                    }
                    pos.extend_from_slice(&p);
                    nrm.extend(s.normal.iter().map(|&c| c as f64));
                    for c in colour(s.name, p) {
                        col.push((255.0 * srgb_to_linear(c)).round_ties_even() as u8);
                    }
                    col.push(255);
                }
            }
            let right_handed = cross(s.ua, s.va) == s.normal;
            for j in 0..s.nv {
                for i in 0..s.nu {
                    if s.door {
                        let (u0, v0) = (s.ul * i as f64 / s.nu as f64, s.vl * j as f64 / s.nv as f64);
                        let (u1, v1) = (s.ul * (i + 1) as f64 / s.nu as f64, s.vl * (j + 1) as f64 / s.nv as f64);
                        let mid = (u0 + u1) / 2.0;
                        if DOOR.0 <= mid && mid <= DOOR.1 && (v0 + v1) / 2.0 <= DOOR.2 {
                            continue;
                        }
                    }
                    let a = base + (j * (s.nu + 1) + i) as u32;
                    let (b, c, d) = (a + 1, a + s.nu as u32 + 2, a + s.nu as u32 + 1);
                    if right_handed {
                        idx.extend_from_slice(&[a, b, c, a, c, d]);
                    } else {
                        idx.extend_from_slice(&[a, c, b, a, d, c]);
                    }
                }
            }
        }
        (pos, nrm, col, idx)
    }

    /// Python's `repr` of a float, for the range the bounds fall in.
    fn py_float(x: f64) -> String {
        assert!(x == 0.0 || (1e-4..1e16).contains(&x.abs()), "{x} needs Python's exponent form");
        format!("{x:?}")
    }

    pub fn glb() -> Vec<u8> {
        let area = W * D + W * H + 2.0 * D * H + W * H - (DOOR.1 - DOOR.0) * DOOR.2;
        let n = (2000.0 / (2.0 * area)).sqrt();
        let (pos, nrm, col, idx) = build(n);
        let mut blob: Vec<u8> = Vec::new();
        let mut views: Vec<String> = Vec::new();
        let mut view = |blob: &mut Vec<u8>, data: &[u8], target: u32| {
            while blob.len() % 4 != 0 {
                blob.push(0);
            }
            views.push(format!(
                "{{\"buffer\":0,\"byteLength\":{},\"byteOffset\":{},\"target\":{target}}}",
                data.len(),
                blob.len()
            ));
            blob.extend_from_slice(data);
        };
        let f32s = |xs: &[f64]| -> Vec<u8> { xs.iter().flat_map(|&x| (x as f32).to_le_bytes()).collect() };
        let nv = pos.len() / 3;
        let lo: Vec<String> = (0..3).map(|k| py_float(pos[k..].iter().step_by(3).copied().fold(f64::INFINITY, f64::min) as f32 as f64)).collect();
        let hi: Vec<String> = (0..3).map(|k| py_float(pos[k..].iter().step_by(3).copied().fold(f64::NEG_INFINITY, f64::max) as f32 as f64)).collect();
        view(&mut blob, &f32s(&pos), 34962);
        view(&mut blob, &f32s(&nrm), 34962);
        view(&mut blob, &col, 34962);
        let ib: Vec<u8> = idx.iter().flat_map(|i| i.to_le_bytes()).collect();
        view(&mut blob, &ib, 34963);
        while blob.len() % 4 != 0 {
            blob.push(0);
        }
        let accessors = format!(
            "[{{\"bufferView\":0,\"componentType\":5126,\"count\":{nv},\"max\":[{}],\"min\":[{}],\"type\":\"VEC3\"}},\
             {{\"bufferView\":1,\"componentType\":5126,\"count\":{nv},\"type\":\"VEC3\"}},\
             {{\"bufferView\":2,\"componentType\":5121,\"count\":{nv},\"normalized\":true,\"type\":\"VEC4\"}},\
             {{\"bufferView\":3,\"componentType\":5125,\"count\":{},\"type\":\"SCALAR\"}}]",
            hi.join(","),
            lo.join(","),
            idx.len()
        );
        let mut js = format!(
            "{{\"accessors\":{accessors},\"asset\":{{\"generator\":\"{GENERATOR}\",\"version\":\"2.0\"}},\
             \"bufferViews\":[{}],\"buffers\":[{{\"byteLength\":{}}}],\"extensionsUsed\":[\"KHR_materials_unlit\"],\
             \"materials\":[{{\"doubleSided\":true,\"extensions\":{{\"KHR_materials_unlit\":{{}}}},\
             \"pbrMetallicRoughness\":{{\"baseColorFactor\":[1,1,1,1],\"metallicFactor\":0,\"roughnessFactor\":1}}}}],\
             \"meshes\":[{{\"primitives\":[{{\"attributes\":{{\"COLOR_0\":2,\"NORMAL\":1,\"POSITION\":0}},\"indices\":3,\
             \"material\":0,\"mode\":4}}]}}],\"nodes\":[{{\"mesh\":0,\"name\":\"scan\"}}],\"scene\":0,\
             \"scenes\":[{{\"nodes\":[0]}}]}}",
            views.join(","),
            blob.len()
        )
        .into_bytes();
        while js.len() % 4 != 0 {
            js.push(b' ');
        }
        let total = 12 + 8 + js.len() + 8 + blob.len();
        let mut out = Vec::with_capacity(total);
        out.extend_from_slice(b"glTF");
        out.extend_from_slice(&2u32.to_le_bytes());
        out.extend_from_slice(&(total as u32).to_le_bytes());
        out.extend_from_slice(&(js.len() as u32).to_le_bytes());
        out.extend_from_slice(b"JSON");
        out.extend_from_slice(&js);
        out.extend_from_slice(&(blob.len() as u32).to_le_bytes());
        out.extend_from_slice(b"BIN\0");
        out.extend_from_slice(&blob);
        out
    }
}

/// The committed `examples/assets/scan_room.glb` is byte for byte what its
/// generator writes. A binary asset does not follow an edit to its script:
/// change the generator string there and the asset keeps the string it was
/// written with, which is what every `cadpole eval
/// examples/scan_underlay.star` then reports. No Python runs
/// under Bazel here, so the test carries a transcription of the script and
/// pins the script by CRC: an edit to the script fails this test until the
/// asset is regenerated (`python3 -I examples/tools/scan_room_glb.py`), the
/// transcription follows the edit and the CRC is updated.
#[test]
fn the_example_scan_is_byte_for_byte_what_its_generator_writes() {
    let root = match std::env::var("CADPOLE_EXAMPLES") {
        Ok(d) => std::path::PathBuf::from(d),
        Err(_) => std::path::PathBuf::from(env!("CARGO_MANIFEST_DIR")).join("../../../examples"),
    };
    let script = std::fs::read(root.join("tools/scan_room_glb.py")).expect("the generator");
    let crc = cadpole_export_3mf::zip::crc32(&script);
    assert_eq!(
        crc, 2176296751,
        "examples/tools/scan_room_glb.py changed: regenerate examples/assets/scan_room.glb with it, mirror the \
         change in this test's transcription, then update this CRC"
    );
    assert!(String::from_utf8_lossy(&script).contains(&format!("\"generator\": \"{}\"", scan_room::GENERATOR)));
    let asset = std::fs::read(root.join("assets/scan_room.glb")).expect("the asset");
    let want = scan_room::glb();
    let first = asset.iter().zip(&want).position(|(a, b)| a != b);
    assert!(
        asset == want,
        "examples/assets/scan_room.glb is stale: {} bytes against the generator's {}, first difference at byte {first:?}; \
         regenerate it with `python3 -I examples/tools/scan_room_glb.py`",
        asset.len(),
        want.len()
    );
}
