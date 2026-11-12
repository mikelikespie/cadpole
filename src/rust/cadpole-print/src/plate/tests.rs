use cadpole_core::geom::{Transform, Vec3};
use cadpole_core::tessellate::Mesh;

use super::*;
use crate::testing::*;

fn body<'a>(name: &str, mesh: &'a Mesh, colour: &str) -> PlanBody<'a> {
    PlanBody { name: name.to_owned(), mesh, topo: None, colours: vec![colour.to_owned()], tri_colours: Vec::new() }
}

/// `n` copies of `m`, 300 mm apart along x, so nothing joins them.
fn spread(m: &Mesh, n: usize) -> Vec<Mesh> {
    (0..n).map(|i| translated(m, Vec3::new(300.0 * i as f64, 0.0, 0.0))).collect()
}

fn loose(n: usize) -> Vec<Item> {
    (0..n).map(|i| Item { bodies: vec![i], ..Item::default() }).collect()
}

/// A lid as it sits on its base: a 2 mm top over 8 mm walls, open below.
fn lid(w: f64, d: f64) -> Mesh {
    joined(&[
        ([0.0, 0.0, 8.0], [w, d, 10.0]),
        ([0.0, 0.0, 0.0], [2.0, d, 8.0]),
        ([w - 2.0, 0.0, 0.0], [w, d, 8.0]),
        ([2.0, 0.0, 0.0], [w - 2.0, 2.0, 8.0]),
        ([2.0, d - 2.0, 0.0], [w - 2.0, d, 8.0]),
    ])
}

fn plate_box(m: &Mesh, t: &Transform) -> (Vec3, Vec3) {
    m.transformed(t).bbox().expect("a mesh")
}

/// No two placed pieces on a plate come closer than `gap` in plan (by
/// bounding box), and every one stands on z = 0 inside the region.
fn assert_laid_out(plan: &Plan, bodies: &[PlanBody<'_>], opts: &Options, gap: f64) {
    for plate in &plan.plates {
        let boxes: Vec<(Vec3, Vec3)> = plate
            .placements
            .iter()
            .map(|&k| {
                let pl = &plan.placements[k];
                let m = concat(plan.pieces[pl.piece].bodies.iter().map(|&b| bodies[b].mesh));
                plate_box(&m, &pl.transform)
            })
            .collect();
        for (lo, hi) in &boxes {
            assert!(lo.z.abs() < 1e-4, "on the bed: {lo:?}");
            assert!(lo.x >= opts.region[0] - 1e-6 && lo.y >= opts.region[1] - 1e-6, "{lo:?} inside {:?}", opts.region);
            assert!(hi.x <= opts.region[2] + 1e-6 && hi.y <= opts.region[3] + 1e-6, "{hi:?} inside {:?}", opts.region);
        }
        for i in 0..boxes.len() {
            for j in i + 1..boxes.len() {
                let (a, b) = (boxes[i], boxes[j]);
                let dx = (b.0.x - a.1.x).max(a.0.x - b.1.x);
                let dy = (b.0.y - a.1.y).max(a.0.y - b.1.y);
                assert!(dx.max(dy) >= gap, "pieces {i} and {j} on {} are {} apart: {a:?} {b:?}", plate.name, dx.max(dy));
            }
        }
    }
}

#[test]
fn a_lid_is_turned_over_onto_its_top() {
    let m = lid(60.0, 40.0);
    let bodies = [body("lid", &m, "#ff0000")];
    let opts = Options::default();
    let plan = plan(&bodies, &loose(1), &opts);
    let p = &plan.pieces[0];
    let base = &p.candidates[p.baseline.unwrap()];
    assert!(base.support_area > 1000.0, "as modelled the top is a ceiling: {}", base.support_area);
    assert_eq!(p.best().support_area, 0.0, "{}", p.reason);
    assert!(p.best().down.dot(Vec3::Z) > 0.99, "the top faces the bed: {}", p.reason);
    assert!(p.reason.contains("vs"), "the reason compares with as modelled: {}", p.reason);
    let report = plan.report(&opts);
    assert!(report.contains("lid:"), "{report}");
}

#[test]
fn a_box_already_flat_is_left_as_modelled() {
    let m = box_mesh(30.0, 20.0, 10.0);
    let bodies = [body("block", &m, "#ff0000")];
    let plan = plan(&bodies, &loose(1), &Options::default());
    assert_eq!(plan.pieces[0].best().label, score::AS_MODELLED, "{}", plan.pieces[0].reason);
}

#[test]
fn an_along_axis_turns_a_flat_clip_onto_its_edge() {
    // A 30 x 30 x 5 tab whose arm must run along z: flat, it would be
    // across the layers.
    let m = box_mesh(30.0, 30.0, 5.0);
    let bodies = [body("clip", &m, "#ff0000")];
    let free = plan(&bodies, &loose(1), &Options::default());
    assert!(free.pieces[0].best().height < 10.0, "left alone it lies flat: {}", free.pieces[0].reason);
    let items = vec![Item { bodies: vec![0], pin: Pin { along: Some(Vec3::Z), ..Pin::default() }, ..Item::default() }];
    let plan = plan(&bodies, &items, &Options::default());
    let c = plan.pieces[0].best();
    assert!(c.along_degrees.unwrap() < score::ALONG_DEGREES, "{}", plan.pieces[0].reason);
    assert!((c.height - 30.0).abs() < 1e-6, "it stands on its 30 x 5 edge: {}", plan.pieces[0].reason);
}

#[test]
fn a_pinned_direction_is_used_and_said_to_be_pinned() {
    let m = lid(60.0, 40.0);
    let bodies = [body("lid", &m, "#ff0000")];
    let items = vec![Item { bodies: vec![0], pin: Pin { keep: true, ..Pin::default() }, ..Item::default() }];
    let plan = plan(&bodies, &items, &Options::default());
    assert_eq!(plan.pieces[0].best().label, score::AS_MODELLED);
    assert!(plan.pieces[0].reason.contains("keep"), "{}", plan.pieces[0].reason);
    let items = vec![Item { bodies: vec![0], pin: Pin { down: Some(Vec3::X), ..Pin::default() }, ..Item::default() }];
    let plan = super::plan(&bodies, &items, &Options::default());
    assert!(plan.pieces[0].best().pinned);
    assert!(plan.pieces[0].reason.contains("pinned"), "{}", plan.pieces[0].reason);
}

/// An axle with a cap at each end and a square ring round it, 0.5 mm clear.
fn axle_and_wheel() -> (Mesh, Mesh) {
    let axle = joined(&[
        ([-2.0, -2.0, 0.0], [2.0, 2.0, 20.0]),
        ([-8.0, -8.0, 0.0], [8.0, 8.0, 2.0]),
        ([-8.0, -8.0, 18.0], [8.0, 8.0, 20.0]),
    ]);
    let wheel = joined(&[
        ([-12.0, -12.0, 2.5], [-2.5, 12.0, 17.5]),
        ([2.5, -12.0, 2.5], [12.0, 12.0, 17.5]),
        ([-2.5, -12.0, 2.5], [2.5, -2.5, 17.5]),
        ([-2.5, 2.5, 2.5], [2.5, 12.0, 17.5]),
    ]);
    (axle, wheel)
}

#[test]
fn a_wheel_held_on_its_axle_is_one_print_in_place_piece() {
    let (axle, wheel) = axle_and_wheel();
    assert!(pieces::captive(&wheel, &axle), "the caps and the axle hold the wheel every way");
    let bodies = [body("axle", &axle, "#ff0000"), body("wheel", &wheel, "#000000")];
    let plan = plan(&bodies, &loose(2), &Options::default());
    assert_eq!(plan.pieces.len(), 1, "{:?}", plan.pieces.iter().map(|p| &p.name).collect::<Vec<_>>());
    let p = &plan.pieces[0];
    assert_eq!(p.kind, PieceKind::PrintInPlace);
    assert_eq!(p.best().label, score::AS_MODELLED, "never turned: {}", p.reason);
    assert!(p.reason.contains("captive"), "{}", p.reason);
    assert_eq!(p.colours, vec!["#ff0000".to_owned(), "#000000".to_owned()]);
}

#[test]
fn a_lid_resting_on_its_base_is_printed_apart() {
    let base = joined(&[([0.0, 0.0, 0.0], [60.0, 40.0, 2.0]), ([0.0, 0.0, 2.0], [60.0, 2.0, 20.0]), ([0.0, 38.0, 2.0], [60.0, 40.0, 20.0])]);
    let top = translated(&lid(60.0, 40.0), Vec3::new(0.0, 0.0, 20.0));
    assert!(!pieces::captive(&top, &base), "the lid lifts off");
    let bodies = [body("base", &base, "#ffffff"), body("lid", &top, "#ff0000")];
    let plan = plan(&bodies, &loose(2), &Options::default());
    assert_eq!(plan.pieces.len(), 2);
    assert_eq!(plan.plates.len(), 1, "two small pieces share a plate");
}

#[test]
fn a_skin_and_an_overlap_are_fused_into_one_piece() {
    let shell = box_mesh(40.0, 40.0, 10.0);
    let stripe = box_between(Vec3::new(5.0, 0.0, 10.0), Vec3::new(10.0, 40.0, 10.6));
    let eye = box_between(Vec3::new(30.0, 30.0, 8.0), Vec3::new(34.0, 34.0, 12.0));
    let bodies = [body("shell", &shell, "#5b7fa3"), body("stripe", &stripe, "#ffce1f"), body("eye", &eye, "#161616")];
    let plan = plan(&bodies, &loose(3), &Options::default());
    assert_eq!(plan.pieces.len(), 1);
    assert_eq!(plan.pieces[0].kind, PieceKind::Fused);
    assert_eq!(plan.pieces[0].joints.len(), 2);
    assert!(matches!(plan.pieces[0].joints[0].kind, JointKind::Skin(f) if f > 0.4));
    assert_eq!(plan.pieces[0].joints[1].kind, JointKind::Overlap);
    // The stripe's underside lies on the shell and the eye's lower half is
    // inside it: neither is an overhang or an island.
    let best = plan.pieces[0].best();
    assert_eq!((best.support_area, best.islands), (0.0, 0), "{}", plan.pieces[0].reason);
    assert_eq!(best.label, score::AS_MODELLED, "{}", plan.pieces[0].reason);
}

#[test]
fn a_list_is_one_piece_whatever_the_analysis_finds() {
    let a = box_mesh(10.0, 10.0, 10.0);
    let b = translated(&box_mesh(10.0, 10.0, 10.0), Vec3::new(40.0, 0.0, 0.0));
    let bodies = [body("a", &a, "#ff0000"), body("b", &b, "#ff0000")];
    let items = vec![Item { bodies: vec![0, 1], declared: true, ..Item::default() }];
    let plan = plan(&bodies, &items, &Options::default());
    assert_eq!(plan.pieces.len(), 1);
    assert_eq!(plan.pieces[0].kind, PieceKind::Declared);
}

#[test]
fn twelve_blocks_share_one_plate_without_touching() {
    let ms = spread(&box_mesh(50.0, 40.0, 10.0), 12);
    let bodies: Vec<PlanBody<'_>> = ms.iter().enumerate().map(|(i, m)| body(&format!("b{i}"), m, "#ff0000")).collect();
    let opts = Options::default();
    let plan = plan(&bodies, &loose(12), &opts);
    assert_eq!(plan.plates.len(), 1, "{}", plan.report(&opts));
    assert_eq!(plan.placements.len(), 12);
    assert_laid_out(&plan, &bodies, &opts, opts.spacing - 1e-6);
}

#[test]
fn big_blocks_spill_onto_more_plates_four_to_a_plate() {
    let ms = spread(&box_mesh(100.0, 100.0, 10.0), 9);
    let bodies: Vec<PlanBody<'_>> = ms.iter().enumerate().map(|(i, m)| body(&format!("b{i}"), m, "#ff0000")).collect();
    let opts = Options::default();
    let plan = plan(&bodies, &loose(9), &opts);
    assert_eq!(plan.plates.len(), 3, "{}", plan.report(&opts));
    assert_eq!(plan.plates[0].placements.len(), 4);
    assert_eq!(plan.plates[0].name, "Parts 1");
    assert_laid_out(&plan, &bodies, &opts, opts.spacing - 1e-6);
}

#[test]
fn the_same_input_gives_the_same_plan() {
    let ms: Vec<Mesh> = (0..7)
        .map(|i| translated(&if i % 2 == 0 { lid(60.0, 40.0) } else { box_mesh(33.0, 21.0, 7.0) }, Vec3::new(300.0 * i as f64, 0.0, 0.0)))
        .collect();
    let bodies: Vec<PlanBody<'_>> = ms.iter().enumerate().map(|(i, m)| body(&format!("p{i}"), m, if i % 2 == 0 { "#ff0000" } else { "#00ff00" })).collect();
    let opts = Options::default();
    let a = plan(&bodies, &loose(7), &opts);
    let b = plan(&bodies, &loose(7), &opts);
    assert_eq!(a.report(&opts), b.report(&opts));
    for (x, y) in a.placements.iter().zip(&b.placements) {
        assert_eq!(x.footprint, y.footprint);
        assert_eq!(x.plate, y.plate);
    }
}

#[test]
fn a_plate_never_holds_more_filaments_than_the_ams_has_slots() {
    let ms = spread(&box_mesh(20.0, 20.0, 5.0), 6);
    let colours = ["#ff0000", "#00ff00", "#0000ff", "#ffff00", "#00ffff", "#ff00ff"];
    let bodies: Vec<PlanBody<'_>> = colours.iter().enumerate().map(|(i, c)| body(&format!("b{i}"), &ms[i], c)).collect();
    let opts = Options { ams: 4, ..Options::default() };
    let plan = plan(&bodies, &loose(6), &opts);
    assert_eq!(plan.plates.len(), 2, "{}", plan.report(&opts));
    for p in &plan.plates {
        assert!(p.colours.len() <= 4, "{}: {:?}", p.name, p.colours);
        assert!(p.tower, "a plate of several filaments keeps room for its tower");
    }
}

#[test]
fn same_coloured_pieces_go_together() {
    let m = spread(&box_mesh(100.0, 100.0, 5.0), 4);
    // Two red and two blue, room for two a plate: red with red.
    let bodies = [body("r1", &m[0], "#ff0000"), body("b1", &m[1], "#0000ff"), body("r2", &m[2], "#ff0000"), body("b2", &m[3], "#0000ff")];
    let opts = Options { region: [0.0, 0.0, 256.0, 110.0], ..Options::default() };
    let plan = plan(&bodies, &loose(4), &opts);
    assert_eq!(plan.plates.len(), 2, "{}", plan.report(&opts));
    for p in &plan.plates {
        assert_eq!(p.colours.len(), 1, "{}: {:?}", p.name, p.colours);
    }
}

#[test]
fn coupons_print_first_on_their_own_plate_and_variants_get_their_own_plates() {
    let m = spread(&box_mesh(40.0, 40.0, 5.0), 2);
    let c = translated(&box_mesh(15.0, 10.0, 3.0), Vec3::new(900.0, 0.0, 0.0));
    let bodies = [body("part", &m[0], "#ff0000"), body("trim", &m[1], "#000000"), body("coupon", &c, "#ff0000")];
    let mut items = loose(3);
    items[2].pin.coupon = true;
    let opts = Options { variants: vec![Variant::Colour, Variant::Mono], ..Options::default() };
    let plan = plan(&bodies, &items, &opts);
    let names: Vec<&str> = plan.plates.iter().map(|p| p.name.as_str()).collect();
    assert_eq!(names, ["Test coupons", "Colour 1", "Mono 1"], "{}", plan.report(&opts));
    assert_eq!(plan.plates[0].placements.len(), 1);
    assert_eq!(plan.plates[2].variant, Some(Variant::Mono));
    assert!(!plan.plates[2].tower, "one filament, no tower");
    assert_eq!(plan.plates[2].colour_changes, 0);
}

#[test]
fn the_excluded_corner_is_kept_clear() {
    let m = box_mesh(30.0, 30.0, 5.0);
    let bodies = [body("a", &m, "#ff0000")];
    let opts = Options { exclude: vec![[0.0, 0.0, 18.0, 28.0]], ..Options::default() };
    let plan = plan(&bodies, &loose(1), &opts);
    let f = plan.placements[0].footprint;
    assert!(f[0] >= 18.0 || f[1] >= 28.0, "{f:?}");
}

#[test]
fn colour_changes_count_each_layers_filaments() {
    // Two filaments over every one of ten layers: one change a layer.
    assert_eq!(colour::changes(&[(0, 0.0, 2.0), (1, 0.0, 2.0)], 10, 0.2, 2), 10);
    // One above the other: a single change where they meet.
    assert_eq!(colour::changes(&[(0, 0.0, 1.0), (1, 1.0, 2.0)], 10, 0.2, 2), 1);
    // Three on a layer, the middle one carried over: two changes a layer.
    assert_eq!(colour::changes(&[(0, 0.0, 0.2), (1, 0.0, 0.2), (2, 0.0, 0.2)], 1, 0.2, 3), 2);
}

#[test]
fn the_minimum_rectangle_of_a_turned_rectangle_is_found() {
    let m = box_mesh(60.0, 10.0, 5.0).transformed(&Transform::rotation(Vec3::ZERO, Vec3::Z, 30f64.to_radians()));
    let (turn, w, d) = pack::min_rect(&pack::plan_points(&m));
    assert!((w * d - 600.0).abs() < 1e-3, "{w} x {d}");
    assert!((turn - 60.0).abs() < 1e-6, "{turn}");
}

#[test]
fn a_footprint_raster_covers_the_shape_and_its_spacing() {
    let m = box_mesh(10.0, 5.0, 1.0);
    let fp = pack::footprint(&m, 1.0, 2.0, 0.0);
    assert_eq!((fp.raster.w, fp.raster.h), (15, 10));
    // The core is 11 x 6 cells (a box edge on a cell boundary marks both
    // cells), the dilation two cells round it with rounded corners.
    assert!(fp.raster.get(2, 2) && fp.raster.get(12, 7));
    assert!(fp.raster.get(0, 5) && fp.raster.get(7, 0));
    assert!(!fp.raster.get(0, 0), "the corner of a disc dilation is round");
}
