//! End-to-end tests: real Cadpole solids, out to STEP, and back.
//!
//! Two levels of verification, and it is worth being precise about what each
//! one proves.
//!
//! **Structural** ([`crate::validate`]). The text is re-read as a graph and
//! checked for dangling and duplicate ids, cycles, orphans, unclosed shells
//! and mis-oriented voids. This proves the file is well formed. It proves
//! nothing about the shape.
//!
//! **Round trip.** The file is parsed by `truck-stepio`'s reader — a separate
//! implementation, not ours — rebuilt into a truck shell, tessellated, and the
//! resulting mesh's *volume and bounding box* are compared with the original
//! solid's. Volume is a signed integral over the oriented surface, so it
//! catches a wrong surface, a missing face and an inside-out face alike; the
//! bounding box catches a misplaced one. This is the strongest check available
//! without a CAD kernel in the build, and it is what stands in for one.
//!
//! What neither proves: that a *particular* CAD application accepts the file.
//! Nothing in this repository opens FreeCAD.

use cadpole_core::geom::{Plane, Vec3};
use cadpole_core::kernel::{self, BoolOp, KSolid};
use cadpole_core::sketch::{Loop, Sketch, P2};
use truck_meshalgo::prelude::*;

use crate::{validate, write_with_stats, Options, Part, Stats};

/// Tessellation tolerance for the comparison meshes, in mm.
const TOL: f64 = 0.02;

fn opts() -> Options {
    Options { tolerance: 1e-4, ..Options::default() }
}

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

fn notched_block() -> KSolid {
    // A real boolean, so the file carries intersection-curve edges.
    let a = box_solid(20.0, 20.0, 10.0);
    let b = {
        // A slot across the block, open at the top and at both ends: every
        // face of the cutter crosses the block's boundary, which is what
        // truck's boolean needs.
        let s = Sketch::new(Plane::XY.offset(6.0)).with_loop(Loop::rect(P2::new(0.0, 0.0), 8.0, 30.0));
        kernel::extrude(&s, 10.0).unwrap().remove(0)
    };
    kernel::boolean(BoolOp::Subtract, &a, &b, 0.05).unwrap()
}

fn export(name: &str, solid: &KSolid) -> (String, Stats) {
    let (text, stats) = write_with_stats(&[Part { name, solid }], &opts()).expect("the solid must export");
    if let Err(problems) = validate::check(&text) {
        panic!("{name} produced a structurally invalid STEP file:\n{problems}");
    }
    (text, stats)
}

/// Enclosed volume of a polygon mesh, by the divergence theorem. Signed: a
/// mesh whose faces point inward gives a negative number, which is exactly the
/// failure this is here to catch.
fn mesh_volume(poly: &PolygonMesh) -> f64 {
    let pos = poly.positions();
    let mut v = 0.0;
    for tri in poly.faces().triangle_iter() {
        let (a, b, c) = (pos[tri[0].pos], pos[tri[1].pos], pos[tri[2].pos]);
        v += a.to_vec().dot(b.to_vec().cross(c.to_vec())) / 6.0;
    }
    v
}

fn mesh_bbox(poly: &PolygonMesh) -> (Vec3, Vec3) {
    let mut lo = Vec3::new(f64::INFINITY, f64::INFINITY, f64::INFINITY);
    let mut hi = Vec3::new(f64::NEG_INFINITY, f64::NEG_INFINITY, f64::NEG_INFINITY);
    for p in poly.positions() {
        let v = Vec3::new(p.x, p.y, p.z);
        lo = lo.min(v);
        hi = hi.max(v);
    }
    (lo, hi)
}

/// Parse our own output with `truck-stepio`, rebuild the shell and tessellate
/// it. Returns one mesh per `CLOSED_SHELL` in the file.
fn read_back(text: &str) -> Vec<PolygonMesh> {
    let table = truck_stepio::r#in::Table::from_step(text).expect("truck-stepio must parse our output");
    assert!(!table.shell.is_empty(), "the file must contain at least one shell");
    let mut out = Vec::new();
    // `HashMap` iteration order is not stable, so walk the ids in order.
    let mut ids: Vec<u64> = table.shell.keys().copied().collect();
    ids.sort_unstable();
    for id in ids {
        let shell = &table.shell[&id];
        let cshell = table.to_compressed_shell(shell).expect("the shell must rebuild");
        out.push(cshell.robust_triangulation(TOL).to_polygon());
    }
    out
}

/// The truck solid's own tessellation, for comparison.
fn original_mesh(solid: &KSolid) -> cadpole_core::tessellate::Mesh {
    cadpole_core::tessellate::tessellate(solid, TOL).unwrap().0
}

/// Export, validate, read back and compare. Returns the tally and the
/// round-tripped solid's signed volume.
fn assert_round_trips(name: &str, solid: &KSolid) -> (Stats, f64) {
    let (text, stats) = export(name, solid);
    let meshes = read_back(&text);
    assert_eq!(meshes.len(), 1, "{name}: expected one shell");
    let got = &meshes[0];

    let want = original_mesh(solid);
    let want_volume = want.volume();
    let got_volume = mesh_volume(got);
    let rel = (got_volume - want_volume).abs() / want_volume.abs().max(1.0);
    assert!(
        rel < 0.02,
        "{name}: the round-tripped solid encloses {got_volume:.3} mm3, the original {want_volume:.3} mm3 \
         ({:.2}% apart)",
        rel * 100.0
    );
    // The signed comparison above already fails loudly if the round trip
    // inverted the solid: a flipped shell changes the sign, not the size.

    let (wlo, whi) = want.bbox().expect("the original has a bounding box");
    let (glo, ghi) = mesh_bbox(got);
    for (a, b, axis) in [(wlo, glo, "min"), (whi, ghi, "max")] {
        assert!(
            (a - b).length() < 0.05,
            "{name}: the {axis} corner moved from {a:?} to {b:?}"
        );
    }
    (stats, got_volume)
}

#[test]
fn a_box_round_trips_through_a_separate_step_reader() {
    let (stats, volume) = assert_round_trips("box", &box_solid(20.0, 30.0, 10.0));
    assert!(volume > 0.0, "a box must come back facing outward, not inside out (volume {volume})");
    assert_eq!(stats.solids, 1);
    assert_eq!(stats.faces, 6);
    assert_eq!(stats.planes, 6, "every face of a box is a plane");
    assert_eq!(stats.lines, 12);
    assert_eq!(stats.bspline_surfaces + stats.rational_surfaces, 0, "a box needs no freeform surface");
}

#[test]
fn a_drilled_plate_round_trips_and_its_hole_is_a_cylinder() {
    let (stats, volume) = assert_round_trips("plate", &plate_with_hole(20.0, 4.0, 6.0));
    assert!(volume > 0.0, "volume {volume}");
    assert_eq!(stats.cylinders, 2, "a circular hole is two half cylinders in truck's topology");
    assert_eq!(stats.circles, 4, "two arcs on each rim, and an arc is a CIRCLE");
    assert_eq!(stats.rational_surfaces, 0, "recognition must have replaced the NURBS patches");
}

#[test]
fn a_boolean_result_round_trips() {
    // The case `truck-stepio`'s own writer documents as unsupported.
    let (stats, volume) = assert_round_trips("notch", &notched_block());
    assert!(volume > 0.0, "volume {volume}");
    assert!(stats.faces >= 8, "a notched block has at least eight faces, got {}", stats.faces);
}

#[test]
fn a_straight_boolean_edge_is_a_line_and_not_a_three_point_polyline() {
    // Where two planes meet, truck's intersection walker returns three or four
    // collinear points. A cubic cannot be fitted through three points, so the
    // refit declines; the edge must still go out as a LINE, not as a
    // `POLYLINE` with a redundant midpoint -- valid AP214, but read back as a
    // degree-1 B-spline a CAD user cannot select, fillet or dimension as one.
    let (stats, _) = assert_round_trips("notch", &notched_block());
    let (text, _) = export("notch", &notched_block());
    assert_eq!(stats.polyline_intersections, 0, "every edge of this solid is straight");
    assert!(!text.contains("POLYLINE"), "no polyline survives:\n{}", polylines(&text).join("\n"));
    // And they are real LINEs with a direction, not something else that
    // happens not to be a polyline.
    assert!(stats.lines >= 12, "a notched block has at least twelve straight edges: {stats:?}");
    // The round trip above is what says the lines are in the right places: a
    // straightened edge that should have been curved moves the volume.
}

/// Every POLYLINE record in the file, for a failure message.
fn polylines(text: &str) -> Vec<String> {
    text.lines().filter(|l| l.contains("POLYLINE")).map(|l| l.to_owned()).collect()
}

#[test]
fn a_revolved_body_round_trips() {
    let s = Sketch::new(Plane::XZ).with_loop(Loop::rect(P2::new(10.0, 5.0), 4.0, 10.0));
    let solid = kernel::revolve(&s, Vec3::ZERO, Vec3::Z, std::f64::consts::TAU).unwrap().remove(0);
    // NOTE: `cadpole_core::kernel::revolve` of an XZ profile about +Z hands back
    // an inward-facing solid today (its own tessellation measures a negative
    // volume). That is a kernel-binding bug, not an export one, and the point
    // of this test is that the export *preserves* whatever truck had: the
    // comparison is signed.
    let (stats, volume) = assert_round_trips("ring", &solid);
    let want = std::f64::consts::PI * (12.0f64.powi(2) - 8.0f64.powi(2)) * 10.0;
    assert!((volume.abs() - want).abs() / want < 0.02, "a 8..12 x 10 ring is {want} mm3, got {volume}");
    assert!(stats.cylinders + stats.planes + stats.cones > 0, "a revolved rectangle is quadrics: {stats:?}");
}

#[test]
fn turning_recognition_off_still_round_trips_and_writes_freeform_surfaces() {
    let solid = plate_with_hole(20.0, 4.0, 6.0);
    let options = Options { analytic_surfaces: false, analytic_curves: false, ..opts() };
    let (text, stats) = write_with_stats(&[Part { name: "plate", solid: &solid }], &options).unwrap();
    validate::check(&text).expect("still structurally valid");
    assert_eq!(stats.cylinders, 0);
    assert_eq!(stats.circles, 0);
    assert!(stats.rational_surfaces >= 2, "the hole must fall back to NURBS: {stats:?}");
    let meshes = read_back(&text);
    let v = mesh_volume(&meshes[0]);
    let want = original_mesh(&solid).volume();
    assert!((v - want).abs() / want < 0.02, "exact NURBS export: {v} vs {want}");
}

#[test]
fn two_bodies_go_into_one_file_as_two_solids() {
    let a = box_solid(10.0, 10.0, 10.0);
    let b = box_solid(4.0, 4.0, 4.0);
    let (text, stats) =
        write_with_stats(&[Part { name: "big", solid: &a }, Part { name: "small", solid: &b }], &opts()).unwrap();
    validate::check(&text).expect("valid");
    assert_eq!(stats.solids, 2);
    assert_eq!(text.matches("MANIFOLD_SOLID_BREP").count(), 2);
    assert!(text.contains("MANIFOLD_SOLID_BREP('big'"));
    assert!(text.contains("MANIFOLD_SOLID_BREP('small'"));
    assert_eq!(read_back(&text).len(), 2);
}

#[test]
fn a_solid_with_an_internal_void_round_trips_as_two_shells() {
    use truck_modeling::*;
    let outer = {
        let v = builder::vertex(Point3::new(0.0, 0.0, 0.0));
        let e = builder::tsweep(&v, Vector3::new(20.0, 0.0, 0.0));
        let f = builder::tsweep(&e, Vector3::new(0.0, 20.0, 0.0));
        builder::tsweep(&f, Vector3::new(0.0, 0.0, 20.0))
    };
    let mut inner = {
        let v = builder::vertex(Point3::new(6.0, 6.0, 6.0));
        let e = builder::tsweep(&v, Vector3::new(8.0, 0.0, 0.0));
        let f = builder::tsweep(&e, Vector3::new(0.0, 8.0, 0.0));
        builder::tsweep(&f, Vector3::new(0.0, 0.0, 8.0))
    };
    // A solid's boundary always faces away from the material, so a void's
    // faces point into the void.
    inner.not();
    let solid = Solid::new(outer.into_boundaries().into_iter().chain(inner.into_boundaries()).collect());

    let (text, stats) = write_with_stats(&[Part { name: "hollow", solid: &solid }], &opts()).unwrap();
    validate::check(&text).expect("a solid with a void must still be structurally valid");
    assert_eq!(stats.solids, 1);
    assert_eq!(stats.voids, 1);
    assert_eq!(stats.shells, 2);
    assert!(text.contains("BREP_WITH_VOIDS('hollow'"), "{text}");

    // Both shells come back, and the material between them is
    // 20^3 - 8^3 = 7488 mm3. `robust_triangulation` gives each shell its own
    // outward mesh, so the void's mesh carries the sign the STEP file gave it.
    let meshes = read_back(&text);
    assert_eq!(meshes.len(), 2, "one CLOSED_SHELL each");
    let mut volumes: Vec<f64> = meshes.iter().map(mesh_volume).collect();
    volumes.sort_by(|a, b| a.abs().partial_cmp(&b.abs()).unwrap());
    let (void, outer) = (volumes[0], volumes[1]);
    assert!((outer - 8000.0).abs() < 1.0, "the outer shell encloses {outer}, expected 8000");
    // The void's `CLOSED_SHELL` is written **inverted** so that the
    // `ORIENTED_CLOSED_SHELL(.F.)` that ISO 10303-42's WR1 insists on turns it
    // back into a boundary of the solid. As written, therefore, the void shell
    // faces out of the void and encloses a positive 8^3.
    assert!((void - 512.0).abs() < 1.0, "the void shell encloses {void}, expected +512 as written");
}

#[test]
fn the_file_frame_is_ap214_in_millimetres() {
    let (text, _) = export("box", &box_solid(1.0, 1.0, 1.0));
    assert!(text.starts_with("ISO-10303-21;\n"), "{}", &text[..40]);
    assert!(text.ends_with("END-ISO-10303-21;\n"));
    assert!(text.contains("FILE_SCHEMA(('AUTOMOTIVE_DESIGN { 1 0 10303 214 1 1 1 1 }'));"), "{text}");
    assert!(text.contains("SI_UNIT(.MILLI., .METRE.)"), "units must be millimetres");
    assert!(text.contains("ADVANCED_BREP_SHAPE_REPRESENTATION"));
    assert!(text.contains("SHAPE_DEFINITION_REPRESENTATION"));
    assert!(text.contains("PRODUCT_RELATED_PRODUCT_CATEGORY('part'"));
}

#[test]
fn the_same_document_exports_byte_identically_twice() {
    let solid = plate_with_hole(20.0, 4.0, 6.0);
    let a = write_with_stats(&[Part { name: "plate", solid: &solid }], &opts()).unwrap().0;
    let b = write_with_stats(&[Part { name: "plate", solid: &solid }], &opts()).unwrap().0;
    assert_eq!(a, b, "the writer must be deterministic");
}

#[test]
fn a_name_with_a_quote_in_it_survives() {
    let solid = box_solid(1.0, 1.0, 1.0);
    let (text, _) =
        write_with_stats(&[Part { name: "maker's part", solid: &solid }], &opts()).unwrap();
    validate::check(&text).expect("valid");
    assert!(text.contains("MANIFOLD_SOLID_BREP('maker''s part'"), "{text}");
}

#[test]
fn exporting_nothing_is_an_error_rather_than_an_empty_file() {
    let e = write_with_stats(&[], &opts()).unwrap_err();
    assert!(e.to_string().contains("no bodies"), "{e}");
}

#[test]
fn a_nonsense_tolerance_is_refused() {
    let solid = box_solid(1.0, 1.0, 1.0);
    let e = write_with_stats(&[Part { name: "x", solid: &solid }], &Options { tolerance: 0.0, ..opts() })
        .unwrap_err();
    assert!(e.to_string().contains("tolerance"), "{e}");
}

/// A ring of `n` equal arcs of radius `r` on `plane`, as a loft section.
fn arc_ring(plane: Plane, r: f64, n: usize) -> Sketch {
    let at = |t: f64| P2::new(r * t.cos(), r * t.sin());
    let step = std::f64::consts::TAU / n as f64;
    let segments = (0..n)
        .map(|i| {
            let (t0, t1) = (step * i as f64, step * (i + 1) as f64);
            cadpole_core::sketch::Segment::Arc { a: at(t0), b: at(t1), via: at(0.5 * (t0 + t1)) }
        })
        .collect();
    Sketch::new(plane).with_loop(Loop { segments })
}

/// A shallow cone cut into narrow bands -- a loft between two rings of short
/// coaxial arcs, the shoulder of a knob -- is within the document tolerance
/// of a plane band by band, but its edges are exact arcs that leave any such
/// plane by about ten microns. Written as `PLANE`, the file's edges do not lie
/// on their faces and a reader cannot trim them; each band must stay the
/// exact patch.
#[test]
fn a_narrow_band_of_a_shallow_cone_is_not_written_as_a_plane() {
    use cadpole_core::loft::{LoftPlan, Section, SectionMatch};
    let sections = [
        Section::Profile(arc_ring(Plane::XY, 15.34, 100)),
        Section::Profile(arc_ring(Plane::XY.offset(0.4), 14.0, 100)),
    ];
    let solid = kernel::loft(&LoftPlan::new(&sections, true, SectionMatch::Index).unwrap()).unwrap().remove(0);
    let (text, stats) = write_with_stats(&[Part { name: "shoulder", solid: &solid }], &Options::default()).unwrap();
    validate::check(&text).expect("structurally valid");
    assert_eq!(stats.planes, 2, "only the two caps are planes: {stats:?}");
    let meshes = read_back(&text);
    let v = mesh_volume(&meshes[0]);
    let want = original_mesh(&solid).volume();
    assert!((v - want).abs() / want.abs() < 0.02, "the shoulder round-trips: {v} vs {want}");
}
