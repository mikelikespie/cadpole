//! Crate tests.
//!
//! Every STEP fixture is a file Cadpole itself wrote with `cadpole-export-step`,
//! so the round trip is measured against the solid it came from; the
//! hand-written variants are edits of those files that reach the branches
//! a Cadpole export never takes (an inch unit, a refused surface, a degenerate
//! placement, an open shell, a reversed face). The mesh fixtures go out
//! through `cadpole_core::export` and `cadpole_export_3mf` the same way. The
//! corpus under `testdata/fuzz` and the mutation loop at the end hold the
//! one promise docs/security.md makes: a malformed file is a diagnostic,
//! never a panic.

use std::collections::BTreeMap;
use std::path::PathBuf;

use cadpole_core::geom::{Plane, Vec3};
use cadpole_core::kernel::{self, BoolOp, KSolid};
use cadpole_core::sketch::{Loop, P2, Sketch};
use cadpole_core::tessellate::{self, Mesh};
use cadpole_export_3mf::bambu;
use cadpole_export_step::{Options as StepOptions, Part};

use crate::dxf;
use crate::mesh::{self, Repair, Report};
use crate::prescan::{self, Format};
use crate::step;

/// Tessellation tolerance for the comparison meshes, in mm.
const TOL: f64 = 0.02;

// ---------------------------------------------------------------- fixtures

fn box_solid(w: f64, d: f64, h: f64) -> KSolid {
    let s = Sketch::new(Plane::XY).with_loop(Loop::rect(P2::new(0.0, 0.0), w, d));
    kernel::extrude(&s, h).unwrap().remove(0)
}

fn plate_with_hole(side: f64, r: f64, h: f64) -> KSolid {
    let s = Sketch::new(Plane::XY)
        .with_loop(Loop::rect(P2::new(0.0, 0.0), side, side))
        .with_loop(Loop::circle(P2::new(0.0, 0.0), r));
    kernel::extrude(&s, h).unwrap().remove(0)
}

fn sphere(r: f64) -> KSolid {
    cadpole_core::primitives::sphere(Vec3::ZERO, r).unwrap()
}

/// A real boolean, so the file carries intersection-curve edges.
fn notched_block() -> KSolid {
    let a = box_solid(20.0, 20.0, 10.0);
    let s = Sketch::new(Plane::XY.offset(6.0)).with_loop(Loop::rect(P2::new(0.0, 0.0), 8.0, 30.0));
    let b = kernel::extrude(&s, 10.0).unwrap().remove(0);
    kernel::boolean(BoolOp::Subtract, &a, &b, 0.05).unwrap()
}

fn step_text(name: &str, solid: &KSolid) -> String {
    let opts = StepOptions { tolerance: 1e-4, product_name: name.into(), ..StepOptions::default() };
    cadpole_export_step::write(&[Part { name, solid }], &opts).expect("the fixture must export")
}

fn volume_of(s: &KSolid) -> f64 {
    tessellate::tessellate(s, TOL).unwrap().0.volume()
}

fn classes_of(s: &KSolid) -> Vec<&'static str> {
    let mut c: Vec<&'static str> =
        tessellate::tessellate(s, TOL).unwrap().1.faces.iter().map(|f| f.surface.kind_name()).collect();
    c.sort();
    c
}

fn rel(a: f64, b: f64) -> f64 {
    (a - b).abs() / b.abs().max(1e-12)
}

fn import_step(text: &str) -> step::Import {
    step::import_unbounded(text, &step::Options::default()).unwrap_or_else(|e| panic!("import_step: {e}"))
}

fn box_mesh() -> Mesh {
    tessellate::tessellate(&box_solid(20.0, 10.0, 5.0), TOL).unwrap().0
}

fn mesh_opts(repair: Repair) -> mesh::Options {
    mesh::Options { repair, ..mesh::Options::default() }
}

fn import_mesh(bytes: &[u8], name: &str, repair: Repair) -> mesh::Import {
    mesh::import_unbounded(bytes, name, &mesh_opts(repair)).unwrap_or_else(|e| panic!("import_mesh({name}): {e}"))
}

// ---------------------------------------------------------------- 1. STEP round trips

#[test]
fn a_cadpole_step_round_trips_to_the_same_volume_faces_and_classes() {
    let fixtures: Vec<(&str, KSolid)> = vec![
        ("box", box_solid(20.0, 10.0, 5.0)),
        ("plate", plate_with_hole(30.0, 5.0, 4.0)),
        ("sphere", sphere(7.0)),
        ("notched", notched_block()),
    ];
    for (name, solid) in fixtures {
        let text = step_text(name, &solid);
        let im = import_step(&text);
        assert_eq!(im.solids.len(), 1, "{name}: one solid");
        assert!((im.unit_scale - 1.0).abs() < 1e-12, "{name}: millimetres");
        assert!((im.tolerance - 1e-4).abs() < 1e-12, "{name}: the file's tolerance is read");
        let s = &im.solids[0];
        assert_eq!(s.kind, "MANIFOLD_SOLID_BREP");
        assert_eq!(s.label, name, "{name}: the PRODUCT name is the solid's label");
        assert_eq!(s.faces.len(), solid.face_iter().count(), "{name}: every face comes back, in order");
        assert!(s.faces.iter().all(|f| f.label.is_empty()), "{name}: Cadpole writes '' as every face name");
        assert!(s.faces.iter().all(|f| f.entity > 0), "{name}: every face knows its entity id");
        assert_eq!(s.flipped_faces, 0, "{name}: a Cadpole file is consistently oriented");
        assert!(!s.inverted, "{name}: a Cadpole file is outward");
        let (v0, v1) = (volume_of(&solid), volume_of(&s.solid));
        let d = rel(v1, v0);
        eprintln!("round trip {name}: {v0:.6} -> {v1:.6} mm3 ({:.2e} relative), faces {}", d, s.faces.len());
        assert!(d < 1e-6, "{name}: volume {v0} became {v1} ({d:.2e} relative)");
        assert_eq!(classes_of(&s.solid), classes_of(&solid), "{name}: every face keeps its class");
    }
}

#[test]
fn the_report_of_a_cadpole_step_names_what_it_read() {
    let text = step_text("plate", &plate_with_hole(30.0, 5.0, 4.0));
    let im = import_step(&text);
    assert!(im.schema.starts_with("AUTOMOTIVE_DESIGN"), "schema {:?}", im.schema);
    assert_eq!(im.product, "plate");
    assert!(im.entities > 50);
    assert_eq!(im.unit, "mm");
    assert!(im.notes.is_empty(), "a clean file has nothing to say: {:?}", im.notes);
}

// ---------------------------------------------------------------- 2. hand-written STEP

/// The `k`-th `ADVANCED_FACE` line of `text` and its id.
fn face_line(text: &str, k: usize) -> (u64, String) {
    let line = text.lines().filter(|l| l.contains("= ADVANCED_FACE(")).nth(k).expect("a face").to_owned();
    let id: u64 = line[1..line.find(' ').unwrap()].parse().unwrap();
    (id, line)
}

/// Turn face `k` of a Cadpole file over: its `same_sense` *and* its outer
/// bound's orientation, so the face stays self-consistent and disagrees
/// with its neighbours (what a broken exporter writes).
fn turn_face(text: &str, k: usize) -> String {
    let (_, line) = face_line(text, k);
    // ADVANCED_FACE('', (#bound), #surface, .T.)
    let bound: u64 = {
        let open = line.find("(#").unwrap() + 2;
        let close = line[open..].find(|c: char| !c.is_ascii_digit()).unwrap() + open;
        line[open..close].parse().unwrap()
    };
    let flipped = line.replace(", .T.);", ", .F.);").replace(", .F.);", ", .F.);");
    let flipped = if line.ends_with(".F.);") { line.replace(", .F.);", ", .T.);") } else { flipped };
    let mut out = String::new();
    for l in text.lines() {
        if l == line {
            out.push_str(&flipped);
        } else if l.starts_with(&format!("#{bound} = FACE_OUTER_BOUND(")) || l.starts_with(&format!("#{bound} = FACE_BOUND(")) {
            let t = if l.ends_with(".T.);") { l.replace(".T.);", ".F.);") } else { l.replace(".F.);", ".T.);") };
            out.push_str(&t);
        } else {
            out.push_str(l);
        }
        out.push('\n');
    }
    out
}

#[test]
fn an_inch_file_is_scaled_and_a_second_data_section_is_a_warning() {
    let solid = box_solid(2.0, 1.0, 0.5);
    let text = step_text("inch_box", &solid);
    let mm_unit = text.lines().find(|l| l.contains("SI_UNIT(.MILLI., .METRE.)")).expect("the mm unit").to_owned();
    let id = &mm_unit[..mm_unit.find(' ').unwrap()];
    let inch = format!(
        "{id} = ( CONVERSION_BASED_UNIT('INCH', #90001) LENGTH_UNIT() NAMED_UNIT(#90002) );\n\
         #90001 = LENGTH_MEASURE_WITH_UNIT(LENGTH_MEASURE(25.4), #90003);\n\
         #90002 = DIMENSIONAL_EXPONENTS(1., 0., 0., 0., 0., 0., 0.);\n\
         #90003 = ( LENGTH_UNIT() NAMED_UNIT(*) SI_UNIT(.MILLI., .METRE.) );"
    );
    let text = text.replace(&mm_unit, &inch);
    let im = import_step(&text);
    assert!((im.unit_scale - 25.4).abs() < 1e-12, "unit {} x{}", im.unit, im.unit_scale);
    assert_eq!(im.unit, "INCH");
    assert!(
        im.notes.iter().all(|n| !n.message.contains("more than one length unit")),
        "the conversion's base unit is not a second unit: {:?}",
        im.notes
    );
    let v = volume_of(&im.solids[0].solid);
    let want = 2.0 * 1.0 * 0.5 * 25.4 * 25.4 * 25.4;
    assert!(rel(v, want) < 1e-6, "{v} mm3, wanted {want}");
    // The tolerance the file declares is in inches too.
    assert!((im.tolerance - 1e-4 * 25.4).abs() < 1e-9, "tolerance {}", im.tolerance);

    let two = text.replace("ENDSEC;\nEND-ISO-10303-21;", "ENDSEC;\nDATA;\nENDSEC;\nEND-ISO-10303-21;");
    let im = import_step(&two);
    assert!(im.notes.iter().any(|n| n.message.contains("2 DATA sections")), "{:?}", im.notes);
    assert_eq!(im.solids.len(), 1);
}

#[test]
fn an_unsupported_surface_refuses_the_solid_naming_the_face_and_the_entity() {
    let text = step_text("box", &box_solid(10.0, 10.0, 10.0));
    let (fid, line) = face_line(&text, 2);
    // ADVANCED_FACE('', (#b), #surface, .T.) -> the surface id
    let sid: u64 = {
        let after = line.rfind(", #").unwrap() + 3;
        let end = line[after..].find(',').unwrap() + after;
        line[after..end].parse().unwrap()
    };
    let plane = text.lines().find(|l| l.starts_with(&format!("#{sid} = PLANE("))).expect("a plane").to_owned();
    let offset = plane.replace("PLANE('', ", "OFFSET_SURFACE('', ").replace(");", ", 1., .F.);");
    let err = step::import_unbounded(&text.replace(&plane, &offset), &step::Options::default()).unwrap_err();
    assert!(err.contains("face[2]"), "{err}");
    assert!(err.contains(&format!("(#{fid})")), "{err}");
    assert!(err.contains(&format!("OFFSET_SURFACE #{sid} is not supported")), "{err}");
    assert!(err.contains("whole solid is refused"), "{err}");
}

#[test]
fn a_placement_whose_ref_direction_is_its_axis_is_refused_by_id() {
    let text = step_text("box", &box_solid(10.0, 10.0, 10.0));
    // The first placement a PLANE uses.
    let plane = text.lines().find(|l| l.contains("= PLANE(")).unwrap();
    let pid: u64 = plane[plane.rfind('#').unwrap() + 1..plane.rfind(')').unwrap()].parse().unwrap();
    let placement = text.lines().find(|l| l.starts_with(&format!("#{pid} = AXIS2_PLACEMENT_3D("))).unwrap().to_owned();
    // AXIS2_PLACEMENT_3D('', #loc, #axis, #ref)
    let ids: Vec<&str> = placement.split('#').skip(2).map(|s| s.trim_end_matches(|c: char| !c.is_ascii_digit())).collect();
    let axis = ids[1].trim_end_matches(|c: char| !c.is_ascii_digit());
    let broken = format!("#{pid} = AXIS2_PLACEMENT_3D('', #{}, #{axis}, #{axis});", ids[0]);
    let err = step::import_unbounded(&text.replace(&placement, &broken), &step::Options::default()).unwrap_err();
    assert!(err.contains(&format!("AXIS2_PLACEMENT_3D #{pid}")), "{err}");
    assert!(err.contains("parallel to axis"), "{err}");
}

#[test]
fn an_open_shell_alone_is_no_solid() {
    let text = step_text("box", &box_solid(10.0, 10.0, 10.0));
    let msb = text.lines().find(|l| l.contains("= MANIFOLD_SOLID_BREP(")).unwrap().to_owned();
    let open = text.replace("= CLOSED_SHELL(", "= OPEN_SHELL(").replace(&msb, "");
    let err = step::import_unbounded(&open, &step::Options::default()).unwrap_err();
    assert!(err.contains("holds no solid"), "{err}");
}

#[test]
fn a_face_turned_against_its_neighbours_is_turned_back_and_a_shell_inside_out_is_turned_out() {
    let solid = box_solid(20.0, 10.0, 5.0);
    let text = step_text("box", &solid);
    let want = volume_of(&solid);

    let one = turn_face(&text, 3);
    let im = import_step(&one);
    let s = &im.solids[0];
    assert_eq!(s.flipped_faces, 1, "the flood fill turned the one face");
    assert!(!s.inverted);
    assert!(rel(volume_of(&s.solid), want) < 1e-6);

    let mut all = text.clone();
    for k in 0..6 {
        all = turn_face(&all, k);
    }
    let im = import_step(&all);
    let s = &im.solids[0];
    assert_eq!(s.flipped_faces, 0, "a consistent shell needs no flood fill");
    assert!(s.inverted, "an inside-out shell is turned outward by its signed volume");
    assert!(rel(volume_of(&s.solid), want) < 1e-6);
}

/// A cylinder the way an OCCT-based exporter writes one: full-circle edges
/// whose start and end are one vertex, and one seam edge.
const OCCT_CYLINDER: &str = "ISO-10303-21;
HEADER;
FILE_DESCRIPTION(('cylinder'),'2;1');
FILE_NAME('cyl.step','2026-09-20T00:00:00',(''),(''),'','','');
FILE_SCHEMA(('AUTOMOTIVE_DESIGN { 1 0 10303 214 1 1 1 1 }'));
ENDSEC;
DATA;
#1=CARTESIAN_POINT('',(0.,0.,0.));
#2=DIRECTION('',(0.,0.,1.));
#3=DIRECTION('',(1.,0.,0.));
#4=AXIS2_PLACEMENT_3D('',#1,#2,#3);
#5=CARTESIAN_POINT('',(0.,0.,10.));
#6=AXIS2_PLACEMENT_3D('',#5,#2,#3);
#7=CARTESIAN_POINT('',(5.,0.,0.));
#8=CARTESIAN_POINT('',(5.,0.,10.));
#9=VERTEX_POINT('',#7);
#10=VERTEX_POINT('',#8);
#11=CIRCLE('',#4,5.);
#12=CIRCLE('',#6,5.);
#13=EDGE_CURVE('',#9,#9,#11,.T.);
#14=EDGE_CURVE('',#10,#10,#12,.T.);
#15=VECTOR('',#2,1.);
#16=LINE('',#7,#15);
#17=EDGE_CURVE('',#9,#10,#16,.T.);
#18=PLANE('',#4);
#19=PLANE('',#6);
#20=CYLINDRICAL_SURFACE('',#4,5.);
#21=ORIENTED_EDGE('',*,*,#13,.T.);
#22=ORIENTED_EDGE('',*,*,#17,.T.);
#23=ORIENTED_EDGE('',*,*,#14,.F.);
#24=ORIENTED_EDGE('',*,*,#17,.F.);
#25=EDGE_LOOP('',(#21,#22,#23,#24));
#26=FACE_OUTER_BOUND('',#25,.T.);
#27=ADVANCED_FACE('side',(#26),#20,.T.);
#28=ORIENTED_EDGE('',*,*,#13,.F.);
#29=EDGE_LOOP('',(#28));
#30=FACE_OUTER_BOUND('',#29,.T.);
#31=ADVANCED_FACE('bottom',(#30),#18,.F.);
#32=ORIENTED_EDGE('',*,*,#14,.T.);
#33=EDGE_LOOP('',(#32));
#34=FACE_OUTER_BOUND('',#33,.T.);
#35=ADVANCED_FACE('top',(#34),#19,.T.);
#36=CLOSED_SHELL('',(#27,#31,#35));
#37=MANIFOLD_SOLID_BREP('cyl',#36);
ENDSEC;
END-ISO-10303-21;
";

#[test]
fn a_full_circle_edge_is_split_and_the_files_face_names_become_aliases() {
    let im = import_step(OCCT_CYLINDER);
    assert!(im.notes.iter().any(|n| n.message.contains("no length unit")), "{:?}", im.notes);
    assert_eq!(im.solids.len(), 1);
    let s = &im.solids[0];
    assert_eq!(s.split_edges, 2, "both rims were one-vertex circles");
    assert_eq!(s.seam_faces, 1, "the side, walked up and down its seam, was split at the opposite meridian");
    assert_eq!(s.faces.len(), 4);
    assert_eq!(
        s.faces.iter().map(|f| f.label.as_str()).collect::<Vec<_>>(),
        ["side", "side#1", "bottom", "top"],
        "the file's own names, in file order, the split face's second half suffixed"
    );
    assert_eq!(s.faces.iter().map(|f| (f.index, f.piece)).collect::<Vec<_>>(), [(0, 0), (0, 1), (1, 0), (2, 0)]);
    assert_eq!(s.faces.iter().map(|f| f.entity).collect::<Vec<_>>(), [27, 27, 31, 35]);
    let v = volume_of(&s.solid);
    let want = std::f64::consts::PI * 25.0 * 10.0;
    assert!(rel(v, want) < 5e-3, "{v} mm3, wanted {want}");
    let mut c = classes_of(&s.solid);
    c.sort();
    assert_eq!(c, ["cylinder", "cylinder", "plane", "plane"]);
}

/// A cylinder whose side is one face bounded by its two rims and no seam
/// edge — what a file may say of a pin or a rod — at radius `r` and
/// height `h`: each rim winds once round the surface's period, so nothing
/// but the tessellator's two-open-loop seam closes the side's domain.
fn band_cylinder(r: f64, h: f64) -> String {
    format!(
        "ISO-10303-21;
HEADER;
FILE_DESCRIPTION(('band'),'2;1');
FILE_NAME('band.step','2026-09-27T00:00:00',(''),(''),'','','');
FILE_SCHEMA(('AUTOMOTIVE_DESIGN {{ 1 0 10303 214 1 1 1 1 }}'));
ENDSEC;
DATA;
#1=CARTESIAN_POINT('',(0.,0.,0.));
#2=DIRECTION('',(0.,0.,1.));
#3=DIRECTION('',(1.,0.,0.));
#4=AXIS2_PLACEMENT_3D('',#1,#2,#3);
#5=CARTESIAN_POINT('',(0.,0.,{h:?}));
#6=AXIS2_PLACEMENT_3D('',#5,#2,#3);
#7=CARTESIAN_POINT('',({r:?},0.,0.));
#8=CARTESIAN_POINT('',({r:?},0.,{h:?}));
#9=VERTEX_POINT('',#7);
#10=VERTEX_POINT('',#8);
#11=CIRCLE('',#4,{r:?});
#12=CIRCLE('',#6,{r:?});
#13=EDGE_CURVE('',#9,#9,#11,.T.);
#14=EDGE_CURVE('',#10,#10,#12,.T.);
#18=PLANE('',#4);
#19=PLANE('',#6);
#20=CYLINDRICAL_SURFACE('',#4,{r:?});
#21=ORIENTED_EDGE('',*,*,#13,.T.);
#22=EDGE_LOOP('',(#21));
#23=FACE_OUTER_BOUND('',#22,.T.);
#24=ORIENTED_EDGE('',*,*,#14,.F.);
#25=EDGE_LOOP('',(#24));
#26=FACE_BOUND('',#25,.T.);
#27=ADVANCED_FACE('side',(#23,#26),#20,.T.);
#28=ORIENTED_EDGE('',*,*,#13,.F.);
#29=EDGE_LOOP('',(#28));
#30=FACE_OUTER_BOUND('',#29,.T.);
#31=ADVANCED_FACE('bottom',(#30),#18,.F.);
#32=ORIENTED_EDGE('',*,*,#14,.T.);
#33=EDGE_LOOP('',(#32));
#34=FACE_OUTER_BOUND('',#33,.T.);
#35=ADVANCED_FACE('top',(#34),#19,.T.);
#36=CLOSED_SHELL('',(#27,#31,#35));
#37=MANIFOLD_SOLID_BREP('band',#36);
ENDSEC;
END-ISO-10303-21;
"
    )
}

/// A full-period wire on a revolved face closes its domain at any size and
/// any tolerance: the band's side, one face
/// between two rims that each wind once round the period, is closed on the
/// ids and a solid for a pin of 0.03 mm at 0.05 and 0.01 and for a post of
/// 10 mm at 0.05 and at 5, where the chord rule alone would have cut each
/// semicircle in two.
#[test]
fn a_band_between_two_full_rims_meshes_closed_at_any_size_and_tolerance() {
    for (r, h, tol) in [(0.03, 0.3, 0.05), (0.03, 0.3, 0.01), (10.0, 30.0, 0.05), (10.0, 30.0, 5.0)] {
        let im = import_step(&band_cylinder(r, h));
        assert_eq!(im.solids.len(), 1);
        let s = &im.solids[0];
        assert_eq!((s.faces.len(), s.seam_faces), (3, 0), "r {r}: the side stays one face, no seam to split it at");
        let meshed = kernel::tessellate(&s.solid, tol).unwrap_or_else(|e| panic!("r {r} at {tol}: {e}"));
        assert_eq!(meshed.unmatched_edges().0, 0, "r {r} at {tol}: open on the ids");
        let (mesh, _) = tessellate::tessellate(&s.solid, tol).unwrap();
        let report = cadpole_core::solid::validate(&mesh);
        assert!(report.is_solid(), "r {r} at {tol}: {}", report.summary());
        let want = std::f64::consts::PI * r * r * h;
        assert!(report.volume > 0.89 * want && report.volume <= want * (1.0 + 1e-9), "r {r} at {tol}: {} of {want}", report.volume);
    }
}

// ---------------------------------------------------------------- 3. mesh round trips

fn ascii_stl(name: &str, m: &Mesh) -> String {
    let mut s = format!("solid {name}\n");
    for t in 0..m.triangle_count() {
        let tri = m.triangle(t).unwrap();
        s.push_str(" facet normal 0 0 0\n  outer loop\n");
        for p in tri {
            s.push_str(&format!("   vertex {} {} {}\n", p.x, p.y, p.z));
        }
        s.push_str("  endloop\n endfacet\n");
    }
    s.push_str(&format!("endsolid {name}\n"));
    s
}

#[test]
fn a_tessellated_box_round_trips_through_every_mesh_format() {
    let m = box_mesh();
    let want = m.volume();
    let n = m.triangle_count();
    let cases: Vec<(&str, Vec<u8>)> = vec![
        ("box.stl", cadpole_core::export::stl(&[&m])),
        ("box_ascii.stl", ascii_stl("box", &m).into_bytes()),
        ("box.obj", cadpole_core::export::obj(&[("box", &m)]).into_bytes()),
        (
            "box.3mf",
            cadpole_export_3mf::write(&cadpole_export_3mf::Scene {
                title: "box".into(),
                parts: vec![cadpole_export_3mf::Part::new("box", &m, 1)],
                ..Default::default()
            })
            .bytes,
        ),
    ];
    for (name, bytes) in cases {
        let im = import_mesh(&bytes, name, Repair::Report);
        assert_eq!(im.bodies.len(), 1, "{name}");
        let b = &im.bodies[0];
        let r = &b.report;
        eprintln!("{name}: {}", r.summary());
        assert_eq!(b.name, "box", "{name}: the file's name (or its stem)");
        assert_eq!(r.triangles, n, "{name}: every triangle");
        assert!(r.solid, "{name}: {}", r.summary());
        assert!(r.closed && r.oriented);
        assert_eq!(r.shells, 1);
        assert_eq!(r.genus, Some(0));
        assert!(rel(r.volume.unwrap(), want) < 1e-6, "{name}: volume {:?} vs {want}", r.volume);
        assert!(rel(b.mesh.volume(), want) < 1e-6, "{name}: the body's own volume");
        assert_eq!(b.topo.faces.len(), 1);
        assert_eq!(b.topo.faces[0].name, "import_mesh/surface");
        assert_eq!(b.topo.faces[0].surface_kind, "group");
        assert_eq!(b.topo.faces[0].tri_count as usize, n);
        assert_eq!(b.mesh.normals.len(), b.mesh.positions.len(), "{name}: a normal per vertex");
        assert!(b.report.bbox.is_some());
        assert_eq!(r.format, prescan::format_of(name, &bytes).unwrap().name());
    }
}

#[test]
fn a_3mf_with_two_items_places_two_bodies_where_the_items_say() {
    let m = box_mesh();
    let mut moved = cadpole_export_3mf::Part::new("far", &m, 1);
    moved.items = vec![[1.0, 0.0, 0.0, 0.0, 1.0, 0.0, 0.0, 0.0, 1.0, 100.0, 0.0, 0.0]];
    let bytes = cadpole_export_3mf::write(&cadpole_export_3mf::Scene {
        title: "two".into(),
        parts: vec![cadpole_export_3mf::Part::new("home", &m, 1), moved],
        ..Default::default()
    })
    .bytes;
    let im = import_mesh(&bytes, "two.3mf", Repair::Report);
    assert_eq!(im.bodies.len(), 2);
    let names: Vec<&str> = im.bodies.iter().map(|b| b.name.as_str()).collect();
    assert_eq!(names, ["home", "far"]);
    let (lo0, _) = im.bodies[0].mesh.bbox().unwrap();
    let (lo1, _) = im.bodies[1].mesh.bbox().unwrap();
    assert!((lo1.x - lo0.x - 100.0).abs() < 1e-3, "the item transform moved the second body: {lo0:?} {lo1:?}");
    for b in &im.bodies {
        assert!(rel(b.mesh.volume(), m.volume()) < 1e-6);
        assert!(b.report.solid);
    }
}

#[test]
fn a_bambu_project_imports_its_objects_by_name_and_their_parts_as_groups() {
    let shell = tessellate::tessellate(&box_solid(20.0, 20.0, 10.0), TOL).unwrap().0;
    let core = tessellate::tessellate(&kernel::transformed(
        &box_solid(4.0, 4.0, 4.0),
        &cadpole_core::geom::Transform::translation(Vec3::new(30.0, 0.0, 0.0)),
    ), TOL)
    .unwrap()
    .0;
    let knob = tessellate::tessellate(&sphere(3.0), TOL).unwrap().0;
    let project = bambu::Project {
        title: "assembly".into(),
        plates: vec![bambu::Plate {
            name: "one".into(),
            objects: vec![
                bambu::ProjectObject {
                    name: "Housing".into(),
                    parts: vec![bambu::ProjectPart::new("shell", &shell, 1), bambu::ProjectPart::new("core", &core, 1)],
                },
                bambu::ProjectObject { name: "Knob".into(), parts: vec![bambu::ProjectPart::new("knob", &knob, 1)] },
            ],
            ..Default::default()
        }],
        ..Default::default()
    };
    let written = bambu::write_project(&project).expect("the parts fit the default bed");
    // The warnings: no template= (the built-in settings), and the
    // knob standing inside the housing's footprint (an import fixture, not a
    // print: two objects there would print as one).
    assert!(
        written.warnings.iter().all(|w| w.contains("no template=") || w.contains("overlap where they stand")),
        "{:?}",
        written.warnings
    );
    let im = import_mesh(&written.bytes, "assembly.3mf", Repair::Report);
    assert!(im.notes.iter().any(|n| n.message.contains("Bambu Studio project")), "{:?}", im.notes);
    assert_eq!(im.bodies.len(), 2);
    let housing = &im.bodies[0];
    assert_eq!(housing.name, "Housing");
    let groups: Vec<&str> = housing.topo.faces.iter().map(|f| f.name.as_str()).collect();
    assert_eq!(groups, ["import_mesh/shell", "import_mesh/core"]);
    assert_eq!(housing.report.groups, 2);
    assert_eq!(housing.report.shells, 2, "two parts, two shells");
    let want = shell.volume() + core.volume();
    assert!(rel(housing.report.volume.unwrap(), want) < 1e-4, "{:?} vs {want}", housing.report.volume);
    // The parts keep their offset from each other: the writer centres the
    // object and places it, which moves both together.
    let (c0, c1) = (housing.topo.faces[0].centroid, housing.topo.faces[1].centroid);
    assert!((c1.x - c0.x - 30.0).abs() < 1e-2, "{c0:?} {c1:?}");
    let knob_body = &im.bodies[1];
    assert_eq!(knob_body.name, "Knob");
    assert_eq!(knob_body.topo.faces.len(), 1);
    assert_eq!(knob_body.topo.faces[0].name, "import_mesh/surface");
    assert!(knob_body.report.solid);
}

#[test]
fn an_obj_with_groups_names_a_face_per_group() {
    let m = box_mesh();
    let mut s = String::from("o lid\n");
    for p in m.positions.chunks_exact(3) {
        s.push_str(&format!("v {} {} {}\n", p[0], p[1], p[2]));
    }
    // The first four triangles in `top`, the rest in `rest`, one polygon
    // (a quad from two of the box's triangles is not guaranteed planar, so
    // a quad is written as its own two triangles' corners).
    for (t, tri) in m.indices.chunks_exact(3).enumerate() {
        if t == 0 {
            s.push_str("g top\n");
        }
        if t == 4 {
            s.push_str("g rest\n");
        }
        s.push_str(&format!("f {} {} {}\n", tri[0] + 1, tri[1] + 1, tri[2] + 1));
    }
    let im = import_mesh(s.as_bytes(), "lid.obj", Repair::Weld);
    let b = &im.bodies[0];
    assert_eq!(b.name, "lid");
    let names: Vec<&str> = b.topo.faces.iter().map(|f| f.name.as_str()).collect();
    assert_eq!(names, ["import_mesh/top", "import_mesh/rest"]);
    assert_eq!(b.topo.faces[0].tri_count, 4);
    assert_eq!(b.topo.faces[1].tri_count as usize, m.triangle_count() - 4);
    assert!(b.report.solid);
    assert_eq!(b.report.vertices, 8, "welded to the box's corners");
    // Negative indices count back from the last vertex.
    let neg = "v 0 0 0\nv 1 0 0\nv 0 1 0\nf -3 -2 -1\n";
    let im = import_mesh(neg.as_bytes(), "tri.obj", Repair::Report);
    assert_eq!(im.bodies[0].report.triangles, 1);
    assert_eq!(im.bodies[0].name, "tri", "an unnamed single object takes the file's stem");
}

// ---------------------------------------------------------------- 4. broken meshes

fn stl_of(m: &Mesh) -> Vec<u8> {
    cadpole_core::export::stl(&[m])
}

#[test]
fn a_broken_mesh_is_reported_not_mended() {
    let m = box_mesh().welded(1e-4);
    assert_eq!(m.vertex_count(), 8);

    // One triangle removed: three open edges, no volume.
    let mut open = m.clone();
    open.indices.truncate(open.indices.len() - 3);
    open.face_ids.truncate(open.face_ids.len() - 1);
    let r = import_mesh(&stl_of(&open), "open.stl", Repair::Weld).bodies.remove(0).report;
    assert!(!r.closed && !r.solid);
    assert_eq!(r.open_edges, 3);
    assert_eq!(r.volume, None);
    assert!(r.open_at.is_some());
    let notes = r.diagnostics();
    assert!(notes.iter().any(|n| n.level == crate::Level::Warning && n.message.contains("not closed")), "{notes:?}");
    assert!(notes.iter().any(|n| n.message.contains("chord error: not measurable")), "{notes:?}");

    // One triangle flipped: its three edges walked the same way twice.
    let mut flipped = m.clone();
    flipped.indices.swap(1, 2);
    let r = import_mesh(&stl_of(&flipped), "flipped.stl", Repair::Weld).bodies.remove(0).report;
    assert!(r.closed && !r.oriented && !r.solid);
    assert_eq!(r.flipped_edges, 3);
    assert_eq!(r.volume, None);
    assert!(!r.flipped_to_outward, "a partly flipped mesh is not turned");

    // A sliver with three distinct collinear corners: degenerate after the weld.
    let mut sliver = m.clone();
    let base = sliver.vertex_count() as u32;
    sliver.positions.extend_from_slice(&[0.0, 0.0, 0.0, 5.0, 0.0, 0.0, 10.0, 0.0, 0.0]);
    sliver.normals.extend_from_slice(&[0.0; 9]);
    sliver.indices.extend_from_slice(&[base, base + 1, base + 2]);
    sliver.face_ids.push(0);
    let r = import_mesh(&stl_of(&sliver), "sliver.stl", Repair::Report).bodies.remove(0).report;
    assert_eq!(r.degenerate, 1, "{}", r.summary());

    // Two boxes through each other: crossings, two shells.
    let a = box_mesh();
    let b = tessellate::tessellate(
        &kernel::transformed(&box_solid(20.0, 10.0, 5.0), &cadpole_core::geom::Transform::translation(Vec3::new(7.0, 3.0, 2.0))),
        TOL,
    )
    .unwrap()
    .0;
    let bytes = cadpole_core::export::stl(&[&a, &b]);
    let r = import_mesh(&bytes, "crossing.stl", Repair::Report).bodies.remove(0).report;
    assert_eq!(r.shells, 2);
    assert!(r.crossings > 0, "{}", r.summary());
    assert!(r.closed && r.oriented && !r.solid);
    assert!(r.crossing_at.is_some());
    assert!(r.diagnostics().iter().any(|n| n.message.contains("cross")), "{:?}", r.diagnostics());

    // `off` skips the search and says so.
    let r = import_mesh(&bytes, "crossing.stl", Repair::Off).bodies.remove(0).report;
    assert!(r.pairs_skipped);
    assert_eq!(r.crossings, 0);

    // An inside-out closed mesh is turned outward under `weld` only.
    let mut inside_out = m.clone();
    for t in inside_out.indices.chunks_exact_mut(3) {
        t.swap(1, 2);
    }
    assert!(inside_out.volume() < 0.0);
    let r = import_mesh(&stl_of(&inside_out), "inside_out.stl", Repair::Weld).bodies.remove(0).report;
    assert!(r.flipped_to_outward && r.solid);
    assert!(r.volume.unwrap() > 0.0);
    let body = import_mesh(&stl_of(&inside_out), "inside_out.stl", Repair::Report).bodies.remove(0);
    assert!(!body.report.flipped_to_outward);
    assert!(body.report.inverted && !body.report.oriented && !body.report.solid);
    assert!(body.report.diagnostics().iter().any(|n| n.message.contains("inside out")));
    assert!(body.mesh.volume() < 0.0, "report leaves the winding alone");
    assert!(body.report.volume.unwrap() < 0.0);
}

#[test]
fn weld_closes_a_soup_and_report_leaves_it_a_soup() {
    let m = box_mesh();
    let bytes = stl_of(&m);
    let n = m.triangle_count();
    let soup = import_mesh(&bytes, "box.stl", Repair::Report).bodies.remove(0);
    assert_eq!(soup.report.vertices, 3 * n, "an STL is a soup and `report` keeps it one");
    assert_eq!(soup.report.welded, 0);
    assert_eq!(soup.report.weld_pass, 0);
    assert!(soup.report.solid, "the verdict is on a welded copy either way");
    let welded = import_mesh(&bytes, "box.stl", Repair::Weld).bodies.remove(0);
    assert_eq!(welded.report.vertices, 8);
    assert_eq!(welded.report.welded, 3 * n - 8);
    assert_eq!(welded.report.weld_pass, 1);
    assert_eq!(welded.mesh.vertex_count(), 8);
    assert!(welded.report.solid);
}

#[test]
fn the_report_survives_the_feature_args() {
    let m = box_mesh();
    let r = import_mesh(&stl_of(&m), "box.stl", Repair::Weld).bodies.remove(0).report;
    let args: BTreeMap<String, String> = r.to_args("report[0].").into_iter().collect();
    assert!(args.keys().all(|k| k.starts_with("report[0].")));
    let back = Report::from_args(&args, "report[0].").expect("a report");
    assert_eq!(back, r);
    assert_eq!(Report::from_args(&args, "report[1]."), None);
    assert_eq!(Report::from_args(&BTreeMap::new(), "report[0]."), None);
    for (r, s) in Repair::ALL.iter().zip(["report", "weld", "off"]) {
        assert_eq!(r.name(), s);
        assert_eq!(Repair::parse(s).unwrap(), *r);
    }
    assert!(Repair::parse("auto").unwrap_err().contains("valid: report, weld, off"));
}

// ---------------------------------------------------------------- 5. the corpus

fn corpus_dir() -> PathBuf {
    match std::env::var("CADPOLE_IMPORT_TESTDATA") {
        Ok(d) => PathBuf::from(d).join("fuzz"),
        Err(_) => PathBuf::from(env!("CARGO_MANIFEST_DIR")).join("testdata/fuzz"),
    }
}

/// Every reader on `bytes`, through the bounded entry points (what a
/// script reaches), so a panic is a diagnostic and a hang is a deadline.
fn import_any(name: &str, bytes: &[u8], budget_ms: u64) -> Result<String, String> {
    match prescan::format_of(name, bytes) {
        Ok(Format::Step) => {
            step::import(bytes, &step::Options { budget_ms, ..Default::default() }).map(|im| format!("{} solid(s)", im.solids.len()))
        }
        Ok(Format::Dxf) => dxf::import(bytes, &dxf::Options { budget_ms, ..Default::default() }).map(|d| {
            // The derived views run outside the bounded thread, from a
            // script: they must not panic on anything the parser let
            // through either.
            let s = d.sketch(&[], None, 0.01).map(|s| s.loops.len() + s.paths.len()).unwrap_or(0);
            let c = d.curves(&[], 0.01).map(|c| c.len()).unwrap_or(0);
            let z = d.contours(&[], 0.01).map(|c| c.len()).unwrap_or(0);
            let e = d.spot_elevations(&[], None).map(|e| e.len()).unwrap_or(0);
            let _ = (d.points(&[]), d.texts(&[]), d.bbox());
            format!("{} entit(ies), {s} loops+paths, {c} curves, {z} contours, {e} elevations", d.entities.len())
        }),
        Ok(_) => mesh::import(bytes, name, &mesh::Options { budget_ms, ..Default::default() })
            .map(|im| format!("{} bod(ies)", im.bodies.len())),
        Err(e) => Err(e),
    }
}

#[test]
fn every_file_in_the_corpus_is_a_diagnostic_never_a_panic() {
    let dir = corpus_dir();
    let mut files: Vec<PathBuf> = std::fs::read_dir(&dir)
        .unwrap_or_else(|e| panic!("{}: {e}", dir.display()))
        .flatten()
        .map(|e| e.path())
        .collect();
    files.sort();
    assert!(files.len() >= 10, "the corpus has {} files", files.len());
    // What each file must be refused *for*, where the reason is the point.
    let expect: &[(&str, &str)] = &[
        ("no_data.step", "no DATA"),
        ("deep_parens.step", "nests parentheses"),
        ("long_number.step", "digits"),
        ("unclosed_string.step", "not closed"),
        ("bad_knot_multiplicities.step", "face[0] (#14)"),
        ("one_control_point.step", "face[0] (#14)"),
        ("solid_points_at_product.step", "PRODUCT"),
        ("two_data_sections_no_solid.step", "no solid"),
        ("count_larger_than_file.stl", "truncated"),
        ("count_max.stl", "limit"),
        ("two_vertices.stl", "not three"),
        ("nan_vertex.stl", "not finite"),
        ("index_past_end.obj", "past the end"),
        ("index_zero.obj", "index 0"),
        ("negative_past_start.obj", "past the first"),
        ("not_a_3mf.3mf", "3dmodel.model"),
        ("component_cycle.3mf", "cycle"),
        ("triangle_past_vertices.3mf", "vertex 7"),
        ("binary_sentinel.dxf", "binary DXF"),
        ("dwg_stamp.dxf", "DWG"),
        ("bad_code.dxf", "group code"),
        ("no_section.dxf", "0 / SECTION"),
        ("odd_lines.dxf", "without its value"),
        ("unknown_insunits.dxf", "units="),
        ("huge_minsert.dxf", "expands to more than"),
    ];
    let mut seen = 0;
    let mut panics = Vec::new();
    for p in &files {
        let name = p.file_name().unwrap().to_str().unwrap().to_owned();
        let bytes = std::fs::read(p).unwrap();
        let t = std::time::Instant::now();
        let r = import_any(&name, &bytes, 20_000);
        let ms = t.elapsed().as_millis();
        eprintln!("{name}: {ms} ms: {r:?}");
        match &r {
            Ok(_) => {}
            Err(e) => {
                assert!(!e.is_empty(), "{name}: an empty message");
                assert!(!e.contains("budget"), "{name}: hit the deadline");
                if e.contains("the parser failed") {
                    panics.push(name.clone());
                }
            }
        }
        if let Some((_, want)) = expect.iter().find(|(f, _)| *f == name) {
            seen += 1;
            let e = r.as_ref().err().unwrap_or_else(|| panic!("{name}: expected a refusal mentioning {want:?}"));
            assert!(e.contains(want), "{name}: {e}\n  wanted {want:?}");
        }
    }
    assert_eq!(seen, expect.len(), "every expected file is in the corpus");
    // A panic inside truck's own parser is a diagnostic, and this line says
    // which files still reach one — the fewer the better, none is the aim.
    eprintln!("caught panics (third-party parser): {panics:?}");
}

#[test]
fn a_line_over_the_cap_is_refused_before_it_is_read() {
    let mut obj = b"v 0 0 0\nv 1 0 0\nv 0 1 0\nf 1 2 3\n# ".to_vec();
    obj.extend(std::iter::repeat_n(b'x', prescan::MAX_LINE_BYTES + 100));
    obj.push(b'\n');
    let err = mesh::import(&obj, "long.obj", &mesh::Options::default()).unwrap_err();
    assert!(err.contains("longer than"), "{err}");
    let empty = mesh::import(b"", "x.stl", &mesh::Options::default()).unwrap_err();
    assert!(empty.contains("empty"), "{empty}");
    let png = mesh::import(b"\x89PNG\r\n\x1a\n....", "x.png", &mesh::Options::default()).unwrap_err();
    assert!(png.contains("not a STEP, STL, OBJ, 3MF, glTF or DXF"), "{png}");
    let step_bytes = step_text("box", &box_solid(1.0, 1.0, 1.0)).into_bytes();
    let wrong = mesh::import(&step_bytes, "box.step", &mesh::Options::default()).unwrap_err();
    assert!(wrong.contains("import_step"), "{wrong}");
    let wrong = step::import(b"solid x\nendsolid x\n", &step::Options::default()).unwrap_err();
    assert!(wrong.contains("ISO-10303-21"), "{wrong}");
}

/// splitmix64, so the loop is reproducible from its seed.
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

fn mutate(rng: &mut Rng, bytes: &[u8]) -> (Vec<u8>, &'static str) {
    let mut b = bytes.to_vec();
    if b.is_empty() {
        return (b, "empty");
    }
    match rng.below(6) {
        0 => {
            let i = rng.below(b.len());
            b[i] ^= 1 << rng.below(8);
            (b, "flip")
        }
        1 => {
            let i = rng.below(b.len());
            let n = rng.below(64).min(b.len() - i);
            b.drain(i..i + n);
            (b, "delete")
        }
        2 => {
            let i = rng.below(b.len());
            let n = rng.below(256).min(b.len() - i);
            let span = b[i..i + n].to_vec();
            b.splice(i..i, span);
            (b, "duplicate")
        }
        3 => {
            let i = rng.below(b.len());
            let n = 1 + rng.below(200);
            b.splice(i..i, std::iter::repeat_n(b'(', n));
            (b, "parens")
        }
        _ => {
            // Replace a number with something a lexer dislikes.
            let start = rng.below(b.len());
            if let Some(off) = b[start..].iter().position(|c| c.is_ascii_digit()) {
                let i = start + off;
                let end = i + b[i..].iter().take_while(|c| c.is_ascii_digit() || **c == b'.').count();
                let with: &[u8] = match rng.below(4) {
                    0 => b"1e400",
                    1 => b"NaN",
                    2 => b"1234567890123456789012345678901234567890",
                    _ => b"-0",
                };
                b.splice(i..end, with.iter().copied());
            }
            (b, "number")
        }
    }
}

#[test]
fn two_hundred_mutations_of_every_fixture_are_diagnostics_or_bodies_never_panics() {
    let m = box_mesh();
    let fixtures: Vec<(&str, Vec<u8>)> = vec![
        ("box.step", step_text("box", &box_solid(20.0, 10.0, 5.0)).into_bytes()),
        ("plate.step", step_text("plate", &plate_with_hole(30.0, 5.0, 4.0)).into_bytes()),
        ("sphere.step", step_text("sphere", &sphere(7.0)).into_bytes()),
        ("box.stl", cadpole_core::export::stl(&[&m])),
        ("box_ascii.stl", ascii_stl("box", &m).into_bytes()),
        ("box.obj", cadpole_core::export::obj(&[("box", &m)]).into_bytes()),
        (
            "box.3mf",
            cadpole_export_3mf::write(&cadpole_export_3mf::Scene {
                title: "box".into(),
                parts: vec![cadpole_export_3mf::Part::new("box", &m, 1)],
                ..Default::default()
            })
            .bytes,
        ),
        ("plate.dxf", dxf_of(&drawing_specimen())),
    ];
    let mut rng = Rng(0x5EED_D200);
    let (mut ok, mut refused, mut caught) = (0, 0, 0);
    let per = 200 / fixtures.len() + 1;
    for (name, bytes) in &fixtures {
        for k in 0..per {
            let mut b = bytes.clone();
            let mut what = Vec::new();
            for _ in 0..=rng.below(3) {
                let (nb, w) = mutate(&mut rng, &b);
                b = nb;
                what.push(w);
            }
            let t = std::time::Instant::now();
            let r = import_any(name, &b, 30_000);
            let ms = t.elapsed().as_millis();
            assert!(ms < 30_000, "{name} mutation {k} ({what:?}) took {ms} ms");
            match r {
                Ok(_) => ok += 1,
                Err(e) => {
                    assert!(!e.is_empty(), "{name} mutation {k}: empty message");
                    assert!(!e.contains("budget"), "{name} mutation {k} ({what:?}): {e}");
                    if e.contains("the parser failed") {
                        caught += 1;
                        eprintln!("{name} mutation {k} ({what:?}): caught: {e}");
                    } else {
                        refused += 1;
                    }
                }
            }
        }
    }
    eprintln!("mutations: {ok} imported, {refused} refused, {caught} caught panics (third-party parser)");
    assert!(ok + refused + caught >= 200);
}

// ---------------------------------------------------------------- 6. the deadline

/// `text` with its DATA entities repeated `copies` times, ids shifted, so
/// the file is big and still parses.
fn repeated(text: &str, copies: usize) -> String {
    let data_start = text.find("DATA;\n").unwrap() + 6;
    let data_end = text.rfind("ENDSEC;").unwrap();
    let body = &text[data_start..data_end];
    let max_id: u64 = body
        .lines()
        .filter_map(|l| l.strip_prefix('#'))
        .filter_map(|l| l.split(|c: char| !c.is_ascii_digit()).next())
        .filter_map(|s| s.parse().ok())
        .max()
        .unwrap_or(0);
    let offset = max_id + 1;
    let mut out = String::with_capacity(text.len() * copies);
    out.push_str(&text[..data_start]);
    for c in 0..copies as u64 {
        let shift = c * offset;
        let mut i = 0;
        let b = body.as_bytes();
        while i < b.len() {
            if b[i] == b'#' {
                let start = i + 1;
                let mut end = start;
                while end < b.len() && b[end].is_ascii_digit() {
                    end += 1;
                }
                if end > start {
                    let n: u64 = body[start..end].parse().unwrap();
                    out.push_str(&format!("#{}", n + shift));
                    i = end;
                    continue;
                }
            }
            out.push(b[i] as char);
            i += 1;
        }
    }
    out.push_str(&text[data_end..]);
    out
}

#[test]
fn a_file_that_outlives_its_budget_is_abandoned_with_a_diagnostic() {
    let one = step_text("sphere", &sphere(7.0));
    let big = repeated(&one, (2_000_000 / one.len()).max(2));
    assert!(big.len() > 1_000_000, "{} bytes", big.len());
    let err = step::import(big.as_bytes(), &step::Options { budget_ms: 1, ..Default::default() }).unwrap_err();
    assert!(err.contains("budget"), "{err}");
    // The same file, given time, is a legitimate multi-solid import.
    let im = step::import_unbounded(&big, &step::Options::default()).unwrap();
    assert!(im.solids.len() > 1);
}




/// The OCCT cylinder with its two rim circles written a hair larger than
/// the cylinder they bound — what a refitted intersection curve looks
/// like to the reader — so the rims are off the surface by 1e-5 mm, which
/// truck's boundary mapping refuses. The reader lifts them onto both faces
/// as intersection curves, and the solid tessellates to its volume.
#[test]
fn an_edge_written_off_its_surface_is_lifted_onto_it() {
    let clean = import_step(OCCT_CYLINDER);
    assert_eq!(clean.solids[0].lifted_edges, 0, "a file whose edges lie on their surfaces is left alone");
    let off = OCCT_CYLINDER.replace("#11=CIRCLE('',#4,5.);", "#11=CIRCLE('',#4,5.00001);").replace("#12=CIRCLE('',#6,5.);", "#12=CIRCLE('',#6,5.00001);");
    assert_ne!(off, OCCT_CYLINDER);
    let im = import_step(&off);
    let s = &im.solids[0];
    assert_eq!(s.lifted_edges, 4, "two rims, each split in two, all four off the cylinder: {}", s.lifted_edges);
    let v = volume_of(&s.solid);
    let want = std::f64::consts::PI * 25.0 * 10.0;
    assert!(rel(v, want) < 5e-3, "{v} mm3, wanted {want}");
    assert_eq!(classes_of(&s.solid), ["cylinder", "cylinder", "plane", "plane"]);
}

// ---------------------------------------------------------------- 7. DXF: the writer's file read back, and the deadline

/// A sketch with everything the DXF writer has to get right and the reader
/// has to get back: straight lines, a full circle (two half arcs), a
/// rounded rectangle (quarter arcs), and an open construction path.
fn drawing_specimen() -> Sketch {
    Sketch::new(Plane::XY)
        .with_named_loop(Loop::rect(P2::new(60.0, 40.0), 120.0, 80.0), "plate")
        .with_named_loop(Loop::circle(P2::new(30.0, 40.0), 12.0), "bore")
        .with_named_loop(Loop::rounded_rect(P2::new(90.0, 40.0), 40.0, 30.0, 6.0), "window")
        .with_path(cadpole_core::sketch::Path::new(P2::new(0.0, 0.0)).line_to(P2::new(120.0, 0.0)), "datum")
}

/// What `cadpole export --format dxf` writes for a sketch: the drawing IR
/// through Cadpole's own DXF writer.
fn dxf_of(s: &Sketch) -> Vec<u8> {
    let d = cadpole_drawing::from_sketches(&[("plate".to_owned(), s.clone())], &cadpole_drawing::SketchOptions::default());
    cadpole_drawing_out::dxf::write(&d, &cadpole_drawing_out::dxf::DxfOptions::default()).expect("write")
}

fn same_point(a: P2, b: P2) -> bool {
    (a.x - b.x).abs() < 1e-9 && (a.y - b.y).abs() < 1e-9
}

/// Two segments describe the same curve: the same ends, and for arcs the
/// same centre and radius (a bulge is read back through its sagitta, so
/// the `via` point is not the one written).
fn same_segment(a: &cadpole_core::sketch::Segment, b: &cadpole_core::sketch::Segment) -> bool {
    use cadpole_core::sketch::{Segment, arc_params};
    match (a, b) {
        (Segment::Line { a: a0, b: a1 }, Segment::Line { a: b0, b: b1 }) => same_point(*a0, *b0) && same_point(*a1, *b1),
        (Segment::Arc { a: a0, b: a1, via: av }, Segment::Arc { a: b0, b: b1, via: bv }) => {
            same_point(*a0, *b0)
                && same_point(*a1, *b1)
                && match (arc_params(*a0, *a1, *av), arc_params(*b0, *b1, *bv)) {
                    (Some((ca, ra, _, _)), Some((cb, rb, _, _))) => same_point(ca, cb) && (ra - rb).abs() < 1e-9,
                    _ => false,
                }
        }
        _ => false,
    }
}

/// The same loop up to where it starts and which way it runs.
fn same_loop(a: &Loop, b: &Loop) -> bool {
    if a.segments.len() != b.segments.len() {
        return false;
    }
    let n = a.segments.len();
    for dir in [false, true] {
        let bs: Vec<cadpole_core::sketch::Segment> = if dir { b.reversed().segments } else { b.segments.clone() };
        for shift in 0..n {
            if (0..n).all(|i| same_segment(&a.segments[i], &bs[(i + shift) % n])) {
                return true;
            }
        }
    }
    false
}

#[test]
fn a_dxf_cadpole_wrote_reads_back_to_the_same_loops_within_1e_9() {
    let s = drawing_specimen();
    let bytes = dxf_of(&s);
    let d = dxf::import(&bytes, &dxf::Options::default()).expect("Cadpole's own DXF parses");
    assert_eq!(d.unit, dxf::Unit::Mm);
    assert_eq!(d.unit_source, dxf::UnitSource::Header);
    assert!(d.skipped.is_empty(), "{:?}", d.skipped);
    assert_eq!(d.malformed + d.tilted, 0);
    // The layer the writer named.
    let names: Vec<&str> = d.layers.iter().map(|l| l.name.as_str()).collect();
    assert!(names.contains(&"plate"), "{names:?}");
    let sk = d.sketch(&[], None, 0.01).unwrap();
    assert_eq!(sk.loops.len(), 3, "{:?}", sk.loops.iter().map(|(n, _)| n).collect::<Vec<_>>());
    assert_eq!(sk.paths.len(), 1, "the datum is an open chain");
    for (i, want) in s.loops.iter().enumerate() {
        assert!(
            sk.loops.iter().any(|(_, got)| same_loop(want, got)),
            "loop {i} ({}) did not come back within 1e-9",
            s.loop_names[i]
        );
    }
    let datum = &sk.paths[0].1;
    assert!(same_point(datum.start, P2::new(0.0, 0.0)) && same_point(datum.current(), P2::new(120.0, 0.0)));
    // Six arcs: two halves of the bore and four fillets, exact.
    let arcs = sk.loops.iter().flat_map(|(_, l)| l.segments.iter()).filter(|s| matches!(s, cadpole_core::sketch::Segment::Arc { .. })).count();
    assert_eq!(arcs, 6);
    // Every loop is `<layer>/loop[k]`.
    assert!(sk.loops.iter().all(|(n, _)| n.starts_with("plate/loop[")), "{:?}", sk.loops.iter().map(|(n, _)| n).collect::<Vec<_>>());
}

#[test]
fn a_dimensioned_sheet_reads_back_with_its_text_and_arrowheads() {
    use cadpole_drawing::{Arrow, Dim, DimStyle, Drawing, Item, Style, layer};
    let mut d = Drawing::new();
    let dl = d.layer(layer::DIMS);
    // Filled arrowheads, which the writer emits as SOLIDs (the
    // architectural tick is two lines).
    let ds = DimStyle { format: cadpole_drawing::UnitFormat::ft_in(), arrow: Arrow::Filled, ..DimStyle::architectural() };
    d.push(Item::Dim(Dim::linear(Style::on(dl), P2::new(0.0, 0.0), P2::new(3810.0, 0.0), P2::new(1.0, 0.0), -20.0, ds)));
    let bytes = cadpole_drawing_out::dxf::write(&d, &cadpole_drawing_out::dxf::DxfOptions::default()).expect("write");
    let back = dxf::import(&bytes, &dxf::Options::default()).expect("parses");
    let texts = back.texts(&[]).unwrap();
    assert!(texts.iter().any(|t| t.text.contains("12'-6\"")), "{texts:?}");
    assert!(back.layers.iter().any(|l| l.raw == layer::DIMS), "{:?}", back.layers.iter().map(|l| &l.raw).collect::<Vec<_>>());
    // The arrowheads are SOLIDs, which are closed loops; the dimension and
    // extension lines are chains.
    let sk = back.sketch(&[], None, 0.01).unwrap();
    assert!(back.entities.iter().any(|e| e.kind == dxf::Kind::Solid), "{:?}", back.entities.iter().map(|e| e.kind).collect::<Vec<_>>());
    assert!(sk.loops.len() >= 2, "{}", sk.loops.len());
    assert!(!sk.paths.is_empty());
    // The figured text is not a spot elevation.
    assert!(back.spot_elevations(&[], None).unwrap().is_empty());
}

#[test]
fn a_dxf_that_outlives_its_budget_is_abandoned_with_a_diagnostic() {
    let one = "0\nLWPOLYLINE\n8\nCONTOUR\n38\n150\n70\n0\n90\n3\n10\n0\n20\n5\n10\n50\n20\n8\n10\n100\n20\n5\n";
    let mut big = String::from("0\nSECTION\n2\nENTITIES\n");
    while big.len() < 4_000_000 {
        big.push_str(one);
    }
    big.push_str("0\nENDSEC\n0\nEOF\n");
    let err = dxf::import(big.as_bytes(), &dxf::Options { budget_ms: 1, ..Default::default() }).unwrap_err();
    assert!(err.contains("budget"), "{err}");
    // The same file, given time, is a legitimate drawing.
    let d = dxf::import_unbounded(big.as_bytes(), &dxf::Options::default()).unwrap();
    assert!(d.entities.len() > 10_000);
}


// ---------------------------------------------------------------- 8. the survey fixture

/// The directory `examples/` is at, from Bazel's `CADPOLE_EXAMPLES` or the
/// workspace.
fn examples_dir() -> PathBuf {
    match std::env::var("CADPOLE_EXAMPLES") {
        Ok(d) => PathBuf::from(d),
        Err(_) => PathBuf::from(env!("CARGO_MANIFEST_DIR")).join("../../../examples"),
    }
}

/// The lot's five corners, in feet: a surveyor's plat rather than a box.
const LOT: [(f64, f64); 5] = [(0.0, 0.0), (110.0, 0.0), (120.0, 45.0), (100.0, 75.0), (0.0, 70.0)];

/// The ground the survey measured, in feet: a hillside rising to the
/// north-east, the one surface every contour and spot shot below agrees
/// with.
fn ground(x: f64, y: f64) -> f64 {
    150.0 + 0.052 * y + 0.011 * x + 0.9 * cadpole_core::fmath::sin(x / 22.0)
}

/// A site survey the way a surveyor's CAD writes one, through Cadpole's own
/// drawing IR and DXF writer, in feet: the property lines as one closed
/// polyline on `PL` with two bearing labels on `PL-ANNO`; seven contour
/// polylines, one per layer named for its elevation (`CONTOUR-150` …
/// `CONTOUR-156`), because the writer draws in the plane and the reader's
/// layer-name rule is how a 2D contour sheet carries its elevations; twenty
/// spot shots as `TEXT` on `SPOT` in the four spellings a survey uses
/// (`152.37`, `TC 152.37`, `152.37 BK`, `EG 152.37`); a bench mark on
/// `CONTROL`; three tree canopies as circles on `TREE`; a dashed building
/// setback as an open polyline on `BLDG-SETBACK`; and the lot and scale
/// notes on `NOTES`, which carry numbers that are not elevations.
fn survey_drawing() -> cadpole_drawing::Drawing {
    use cadpole_drawing::ir::{Anchor, Dash, FontRef, HAlign, Item, Rgb, Style, TextItem, VAlign};
    use cadpole_drawing::{Drawing, Layer};
    let mut d = Drawing::new();
    d.unit_format = cadpole_drawing::UnitFormat::ft_in();
    let layer = |d: &mut Drawing, name: &str, color: Rgb, dash: Dash| {
        let id = d.layer(name);
        d.layers[id] = Layer::new(name, 0.25).with_color(color).with_dash(dash);
        id
    };
    let text = |d: &mut Drawing, layer: usize, at: P2, s: &str| {
        d.push(Item::Text(TextItem {
            style: Style::on(layer),
            at,
            text: s.to_owned(),
            height_mm: 2.0,
            rotation_deg: 0.0,
            anchor: Anchor::new(HAlign::Left, VAlign::Baseline),
            font: FontRef::Builtin,
            model_height: false,
        }));
    };

    // The property lines: one closed polyline, the way a plat is drawn.
    let pl = layer(&mut d, "PL", Rgb(255, 0, 0), Dash::Solid);
    let corners: Vec<P2> = LOT.iter().map(|&(x, y)| P2::new(x, y)).collect();
    d.named_stroke(pl, polyline_path(&corners), true, "lot");
    let anno = layer(&mut d, "PL-ANNO", Rgb(255, 0, 0), Dash::Solid);
    text(&mut d, anno, P2::new(40.0, 1.5), "N 89°30'00\" E  110.00'");
    text(&mut d, anno, P2::new(1.5, 30.0), "N 00°15'00\" W  70.00'");

    // Contours every foot, each on the layer named for its elevation.
    for k in 0..7 {
        let z = 150.0 + k as f64;
        let l = layer(&mut d, &format!("CONTOUR-{z:.0}"), Rgb(120, 90, 40), Dash::Solid);
        let pts: Vec<P2> = (0..=12)
            .map(|i| {
                let x = -5.0 + 11.0 * i as f64;
                // Where the ground crosses `z` at this x, solved for y.
                let y = (z - 150.0 - 0.011 * x - 0.9 * cadpole_core::fmath::sin(x / 22.0)) / 0.052;
                P2::new(x, y)
            })
            .collect();
        d.named_stroke(l, polyline_path(&pts), false, &format!("contour[{k}]"));
    }

    // Spot shots on a rough grid, labelled the four ways a survey does.
    let spot = layer(&mut d, "SPOT", Rgb(0, 0, 0), Dash::Solid);
    for j in 0..4 {
        for i in 0..5 {
            let (x, y) = (10.0 + 24.0 * i as f64 + 3.0 * j as f64, 8.0 + 18.0 * j as f64);
            let z = ground(x, y);
            let label = match (i + j) % 4 {
                0 => format!("{z:.2}"),
                1 => format!("TC {z:.2}"),
                2 => format!("{z:.2} BK"),
                _ => format!("EG {z:.2}"),
            };
            text(&mut d, spot, P2::new(x, y), &label);
        }
    }
    let control = layer(&mut d, "CONTROL", Rgb(0, 0, 255), Dash::Solid);
    text(&mut d, control, P2::new(2.0, 2.5), "BM 147.52");

    // Trees, as the canopy circles a survey draws them.
    let tree = layer(&mut d, "TREE", Rgb(0, 128, 0), Dash::Solid);
    for (k, (cx, cy, r)) in [(30.0, 50.0, 6.0), (85.0, 25.0, 8.0), (60.0, 62.0, 5.0)].into_iter().enumerate() {
        d.named_loop_stroke(tree, &Loop::circle(P2::new(cx, cy), r), format!("tree[{k}]"));
    }

    // The building setback, dashed and open: a construction path on import.
    let setback = layer(&mut d, "BLDG-SETBACK", Rgb(90, 90, 90), Dash::Hidden);
    d.named_stroke(setback, polyline_path(&[P2::new(15.0, 15.0), P2::new(98.0, 15.0), P2::new(103.0, 45.0)]), false, "setback");

    // Notes with numbers in them that are not elevations.
    let notes = layer(&mut d, "NOTES", Rgb(0, 0, 0), Dash::Solid);
    text(&mut d, notes, P2::new(45.0, 40.0), "LOT 12");
    text(&mut d, notes, P2::new(45.0, 36.0), "BLOCK 4");
    text(&mut d, notes, P2::new(2.0, 78.0), "SCALE 1\" = 20'");
    d
}

fn polyline_path(pts: &[P2]) -> cadpole_core::sketch::Path {
    let mut p = cadpole_core::sketch::Path::new(pts[0]);
    for q in &pts[1..] {
        p = p.line_to(*q);
    }
    p
}

/// What Cadpole's DXF writer writes for the survey, in feet.
fn survey_dxf_bytes() -> Vec<u8> {
    let opts = cadpole_drawing_out::dxf::DxfOptions::default().with_units(cadpole_drawing::LengthUnit::Ft);
    cadpole_drawing_out::dxf::write(&survey_drawing(), &opts).expect("write")
}

/// `examples/assets/site_survey.dxf` is byte for byte what the writer
/// writes for [`survey_drawing`], so the example imports a file Cadpole made
/// and the fixture cannot drift from the drawing that documents it. When it
/// does not match, the fresh bytes are left under the test's undeclared
/// outputs to copy over.
#[test]
fn the_survey_fixture_is_the_writers_own_file() {
    let bytes = survey_dxf_bytes();
    let path = examples_dir().join("assets").join("site_survey.dxf");
    let committed = std::fs::read(&path).unwrap_or_default();
    if committed != bytes {
        let out = std::env::var("TEST_UNDECLARED_OUTPUTS_DIR").map(PathBuf::from).unwrap_or_else(|_| std::env::temp_dir());
        let fresh = out.join("site_survey.dxf");
        std::fs::write(&fresh, &bytes).expect("write the fresh fixture");
        panic!(
            "{} is not what cadpole_drawing_out::dxf writes for survey_drawing() ({} vs {} bytes); the fresh file is at {}",
            path.display(),
            committed.len(),
            bytes.len(),
            fresh.display()
        );
    }
}

/// The fixture read back by the DXF reader: feet from `$INSUNITS`, the lot
/// as one loop, seven contours from their layer names, twenty spot shots
/// plus the bench mark and none of the notes or bearings, the trees as
/// circles, the setback as an open chain — the numbers
/// `examples/site_survey.star` asserts, checked here against the drawing
/// they came from.
#[test]
fn the_survey_fixture_reads_back_as_a_survey() {
    let bytes = survey_dxf_bytes();
    let d = dxf::import(&bytes, &dxf::Options::default()).expect("Cadpole's own survey parses");
    assert_eq!((d.unit, d.unit_source), (dxf::Unit::Ft, dxf::UnitSource::Header));
    assert!((d.scale - 304.8).abs() < 1e-12);
    assert!(d.skipped.is_empty(), "{:?}", d.skipped);
    assert_eq!(d.malformed + d.tilted, 0);
    let ft = d.scale;

    // The lot: one closed loop on PL, its corners where the plat put them.
    let lot = d.sketch(&["PL".to_owned()], Some(true), 0.01).unwrap();
    assert_eq!(lot.loops.len(), 1);
    assert_eq!(lot.loops[0].0, "PL/loop[0]");
    assert_eq!(lot.loops[0].1.segments.len(), 5);
    let area_ft2 = lot.loops[0].1.signed_area().abs() / (ft * ft);
    // The shoelace area of LOT.
    let mut shoelace = 0.0;
    for i in 0..LOT.len() {
        let (x0, y0) = LOT[i];
        let (x1, y1) = LOT[(i + 1) % LOT.len()];
        shoelace += x0 * y1 - x1 * y0;
    }
    assert!((area_ft2 - shoelace.abs() / 2.0).abs() < 1e-6, "{area_ft2} ft²");

    // Contours by layer name, in file order, rising a foot at a time, every
    // vertex on the ground.
    let contours = d.contours(&["CONTOUR*".to_owned()], 0.01).unwrap();
    assert_eq!(contours.len(), 7);
    for (k, (z, pts)) in contours.iter().enumerate() {
        assert!((z - (150.0 + k as f64) * ft).abs() < 1e-9, "contour {k} at {z}");
        assert_eq!(pts.len(), 13);
        for p in pts {
            let g = ground(p.x / ft, p.y / ft);
            assert!((g * ft - z).abs() < 1e-6 * ft, "contour {k}: ({}, {}) is at {g}, not {}", p.x / ft, p.y / ft, z / ft);
        }
    }

    // Spot elevations: twenty on SPOT, the bench mark on CONTROL, nothing
    // from the notes or the bearings.
    let spots = d.spot_elevations(&["SPOT".to_owned()], None).unwrap();
    assert_eq!(spots.len(), 20);
    for s in &spots {
        let g = ground(s[0] / ft, s[1] / ft);
        assert!((s[2] / ft - g).abs() < 0.006, "shot at ({}, {}) reads {} against ground {g}", s[0] / ft, s[1] / ft, s[2] / ft);
    }
    assert_eq!(d.spot_elevations(&[], None).unwrap().len(), 21, "the bench mark and the shots");
    assert_eq!(d.spot_elevations(&["NOTES".to_owned(), "PL-ANNO".to_owned()], None).unwrap().len(), 0);
    assert_eq!(d.spot_elevations(&["SPOT".to_owned()], Some("TC #")).unwrap().len(), 5);
    let texts = d.texts(&[]).unwrap();
    assert_eq!(texts.len(), 20 + 1 + 2 + 3);
    assert!(texts.iter().all(|t| (t.height - 2.0 * ft).abs() < 1e-9));

    // Trees are circles (two arcs each), the setback an open chain, and
    // every layer the drawing declared is there with its colour.
    let trees = d.sketch(&["TREE".to_owned()], Some(true), 0.01).unwrap();
    assert_eq!(trees.loops.len(), 3);
    assert!(trees.loops.iter().all(|(_, l)| l.segments.iter().all(|s| matches!(s, cadpole_core::sketch::Segment::Arc { .. }))));
    let setback = d.sketch(&["BLDG-SETBACK".to_owned()], None, 0.01).unwrap();
    assert_eq!((setback.loops.len(), setback.paths.len()), (0, 1));
    let names: Vec<&str> = d.layers.iter().map(|l| l.name.as_str()).collect();
    for want in ["PL", "PL-ANNO", "CONTOUR-150", "CONTOUR-156", "SPOT", "CONTROL", "TREE", "BLDG-SETBACK", "NOTES"] {
        assert!(names.contains(&want), "{want} in {names:?}");
    }
    let pl = d.layers.iter().find(|l| l.name == "PL").unwrap();
    assert_eq!(pl.color, [255, 0, 0]);
    let (lo, hi) = d.bbox().unwrap();
    assert!((lo[0] - (-5.0 * ft)).abs() < 1e-9 && (hi[0] - 127.0 * ft).abs() < 1e-9, "{lo:?} {hi:?}");
}
