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
    let opts = Options { prefer: Prefer::Plates, ..Options::default() };
    let plan = plan(&bodies, &loose(2), &opts);
    assert_eq!(plan.pieces.len(), 2);
    assert_eq!(plan.plates.len(), 1, "two small pieces share a plate: {}", plan.report(&opts));
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
        assert!(p.tower.is_some(), "a plate of several filaments keeps room for its tower");
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
    assert!(plan.plates[2].tower.is_none(), "one filament, no tower");
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
    // The core is 10 x 5 cells, and the raster has room for the 2 mm it
    // is grown by all round.
    assert_eq!((fp.raster.w, fp.raster.h), (14, 9));
    assert_eq!(fp.area, 50.0);
    assert!(fp.core.get(2, 2) && fp.core.get(11, 6) && !fp.core.get(1, 2) && !fp.core.get(12, 6));
    assert!(fp.raster.get(0, 4) && fp.raster.get(7, 0) && fp.raster.get(13, 4));
    let wide = pack::footprint(&m, 1.0, 4.0, 0.0);
    assert!(wide.raster.get(0, 6) && !wide.raster.get(0, 0), "the corner of the growth is round");
}

/// A truck wheel as modelled on its axle: a hub disc and a tyre ring round
/// it, axis along y, standing on its tread.
fn standing_wheel() -> (Mesh, Mesh) {
    let stand = Transform::rotation(Vec3::ZERO, Vec3::X, std::f64::consts::FRAC_PI_2);
    let hub = revolve(&[(0.0, 0.0), (10.0, 0.0), (10.0, 10.0), (0.0, 10.0)], 48).transformed(&stand);
    let tyre = revolve(&[(10.0, 0.0), (20.0, 0.0), (20.0, 10.0), (10.0, 10.0), (10.0, 0.0)], 48).transformed(&stand);
    (hub, tyre)
}

#[test]
fn a_wheel_standing_on_its_tread_is_laid_on_its_side() {
    let (hub, tyre) = standing_wheel();
    let bodies = [body("hub", &hub, "#FFCE1F"), body("tyre", &tyre, "#161616")];
    let opts = Options::default();
    let plan = plan(&bodies, &loose(2), &opts);
    assert_eq!(plan.pieces.len(), 1, "the hub is a skin of the tyre's bore: one piece");
    let p = &plan.pieces[0];
    let base = &p.candidates[p.baseline.unwrap()];
    assert!(base.support_area > 100.0, "standing, the tread's underside overhangs: {}", p.reason);
    assert!(p.best().down.dot(Vec3::Y).abs() > 0.99, "it lies on a side: {}", p.reason);
    assert_eq!(p.best().support_area, 0.0, "{}", p.reason);
    assert!((p.best().height - 10.0).abs() < 1e-3, "{}", p.reason);

    // And judged as placed, standing is clearly worse.
    let mesh = pieces::without_contacts(&[&hub, &tyre]);
    let v = score::judge(&mesh, None, &Pin::default(), &opts);
    assert!(!v.ok(score::DEFAULT_ORIENTATION_TOLERANCE), "{}", v.words());
    assert!(v.words().contains("as placed it needs") && v.delta > 100.0, "{}", v.words());
}

#[test]
fn a_tall_thin_bracket_lies_down() {
    let m = box_mesh(10.0, 10.0, 80.0);
    let bodies = [body("bracket", &m, "#ff0000")];
    let plan = plan(&bodies, &loose(1), &Options::default());
    let p = &plan.pieces[0];
    assert!(p.best().height <= 10.0 + 1e-6, "{}", p.reason);
    assert!(p.reason.contains("10 mm tall instead of 80") || p.reason.contains("steady"), "{}", p.reason);
}

#[test]
fn a_flat_part_as_placed_is_judged_optimal() {
    let m = lid(60.0, 40.0).transformed(&Transform::rotation(Vec3::new(30.0, 20.0, 5.0), Vec3::X, std::f64::consts::PI));
    let v = score::judge(&m, None, &Pin::default(), &Options::default());
    assert!(v.ok(score::DEFAULT_ORIENTATION_TOLERANCE), "the lid on its top is the best: {}", v.words());
    let up = lid(60.0, 40.0);
    let v = score::judge(&up, None, &Pin::default(), &Options::default());
    assert!(!v.ok(score::DEFAULT_ORIENTATION_TOLERANCE), "{}", v.words());
}

#[test]
fn a_part_with_no_flat_face_rests_on_a_hull_facet() {
    // A tyre alone, standing: its sides are flat, but a ball has none.
    let ball = revolve(&ball_profile(10.0, 16), 32);
    let facets = score::hull_facets(&ball);
    assert!(!facets.is_empty());
    let (_, tyre) = standing_wheel();
    let f = score::hull_facets(&tyre);
    assert!(f.iter().any(|(n, _)| n.dot(Vec3::Y).abs() > 0.99), "the sides are hull facets: {f:?}");
}

#[test]
fn a_nut_held_tight_on_its_screw_is_printed_apart() {
    // The axle and ring, but only 0.1 mm clear: it would fuse.
    let axle = joined(&[
        ([-2.0, -2.0, 0.0], [2.0, 2.0, 20.0]),
        ([-8.0, -8.0, 0.0], [8.0, 8.0, 2.0]),
        ([-8.0, -8.0, 18.0], [8.0, 8.0, 20.0]),
    ]);
    let ring = joined(&[
        ([-12.0, -12.0, 2.1], [-2.1, 12.0, 17.9]),
        ([2.1, -12.0, 2.1], [12.0, 12.0, 17.9]),
        ([-2.1, -12.0, 2.1], [2.1, -2.1, 17.9]),
        ([-2.1, 2.1, 2.1], [2.1, 12.0, 17.9]),
    ]);
    let bodies = [body("screw", &axle, "#ff0000"), body("nut", &ring, "#000000")];
    let plan = plan(&bodies, &loose(2), &Options::default());
    assert_eq!(plan.pieces.len(), 2);
    assert!(plan.notes.iter().any(|n| n.contains("printed apart")), "{:?}", plan.notes);
}

#[test]
fn a_chassis_printed_on_its_stand_stays_on_it() {
    // A slab one layer over a stand that holds it up: printed together, as
    // modelled, or the slab's underside needs support.
    let stand = box_mesh(40.0, 20.0, 10.0);
    let chassis = translated(&box_mesh(40.0, 20.0, 5.0), Vec3::new(0.0, 0.0, 10.2));
    let bodies = [body("stand", &stand, "#000000"), body("chassis", &chassis, "#ff0000")];
    let plan = plan(&bodies, &loose(2), &Options::default());
    assert_eq!(plan.pieces.len(), 1, "{}", plan.report(&Options::default()));
    assert_eq!(plan.pieces[0].kind, PieceKind::PrintInPlace);
    assert!(matches!(plan.pieces[0].joints[0].kind, JointKind::Resting(g) if (g - 0.2).abs() < 1e-3));
    assert!(plan.pieces[0].reason.contains("resting on another"), "{}", plan.pieces[0].reason);
    // A lid sitting right on its box (no gap) is not that: it lifts off.
    let lidded = translated(&lid(40.0, 20.0), Vec3::new(0.0, 0.0, 10.0));
    let bodies = [body("box", &stand, "#000000"), body("lid", &lidded, "#ff0000")];
    let plan = super::plan(&bodies, &loose(2), &Options::default());
    assert_eq!(plan.pieces.len(), 2);
}

// ---------------------------------------------------------------------------
// The layout engine: what it guarantees, on random piece sets and on the
// cases that used to go wrong.
// ---------------------------------------------------------------------------

/// A polygon in plan, `(x, y)` counter-clockwise, `h` tall, standing on the
/// bed.
fn slab(pts: &[(f64, f64)], cap: &[[usize; 3]], h: f64) -> Mesh {
    let mirrored: Vec<(f64, f64)> = pts.iter().map(|p| (p.0, -p.1)).collect();
    prism(&mirrored, cap, h).transformed(&Transform::rotation(Vec3::ZERO, Vec3::X, 90f64.to_radians()))
}

/// A small deterministic generator (xorshift), so a failing set can be run
/// again from its seed.
struct Rng(u64);

impl Rng {
    fn next(&mut self) -> u64 {
        self.0 ^= self.0 << 13;
        self.0 ^= self.0 >> 7;
        self.0 ^= self.0 << 17;
        self.0
    }
    fn below(&mut self, n: u64) -> u64 {
        self.next() % n
    }
    fn range(&mut self, lo: f64, hi: f64) -> f64 {
        lo + (hi - lo) * (self.below(10_000) as f64 / 10_000.0)
    }
}

/// A random piece: a block, an L, a thin bar at an angle, a triangle or a
/// low wide panel.
fn random_piece(r: &mut Rng) -> Mesh {
    match r.below(5) {
        0 => box_mesh(r.range(8.0, 70.0), r.range(8.0, 70.0), r.range(2.0, 30.0)),
        1 => {
            let (arm, thick) = (r.range(30.0, 110.0), r.range(5.0, 14.0));
            let pts = [(0.0, 0.0), (arm, 0.0), (arm, thick), (thick, thick), (thick, arm), (0.0, arm)];
            slab(&pts, &[[0, 1, 2], [0, 2, 3], [0, 3, 4], [0, 4, 5]], r.range(2.0, 12.0))
        }
        2 => box_mesh(r.range(60.0, 240.0), r.range(3.0, 9.0), r.range(2.0, 6.0))
            .transformed(&Transform::rotation(Vec3::ZERO, Vec3::Z, r.range(0.0, 180.0).to_radians())),
        3 => {
            let (a, b) = (r.range(20.0, 120.0), r.range(20.0, 120.0));
            slab(&[(0.0, 0.0), (a, 0.0), (0.0, b)], &[[0, 1, 2]], r.range(2.0, 20.0))
        }
        _ => box_mesh(r.range(60.0, 160.0), r.range(30.0, 90.0), 0.4),
    }
}

const PALETTE: [&str; 5] = ["#ff0000", "#00ff00", "#0000ff", "#ffffff", "#000000"];

fn random_set(seed: u64) -> (Vec<Mesh>, Vec<&'static str>, Vec<Item>, Options) {
    let mut r = Rng(seed.wrapping_mul(0x9E37_79B9_7F4A_7C15) | 1);
    let n = 2 + r.below(27) as usize;
    let colours = 1 + r.below(5) as usize;
    let meshes: Vec<Mesh> = (0..n).map(|i| translated(&random_piece(&mut r), Vec3::new(400.0 * i as f64, 0.0, 0.0))).collect();
    let cols: Vec<&'static str> = (0..n).map(|_| PALETTE[r.below(colours as u64) as usize]).collect();
    let mut items = loose(n);
    for it in items.iter_mut() {
        it.pin.keep = true;
        match r.below(12) {
            0 => it.pin.spin = Some(r.range(0.0, 360.0)),
            1 => it.pin.no_spin = true,
            2 => it.pin.plate = Some(1 + r.below(2) as u32),
            3 => it.pin.at = Some((r.range(20.0, 230.0), r.range(20.0, 230.0))),
            4 => it.pin.group = Some("a".to_owned()),
            _ => {}
        }
    }
    let mut opts = Options::default();
    match r.below(4) {
        0 => {
            // A two-nozzle machine's reach, off the bed's corner.
            opts.region = [25.0, 0.0, 325.0, 320.0];
        }
        1 => opts.exclude = vec![[0.0, 0.0, 18.0, 28.0]],
        _ => {}
    }
    opts.spacing = [0.0, 2.0, 4.0, 6.5][r.below(4) as usize];
    opts.ams = [0, 2, 4][r.below(3) as usize];
    opts.prefer = [Prefer::Balanced, Prefer::Plates, Prefer::Waste][r.below(3) as usize];
    opts.centre_bias = [1.0, 1.0, 0.0, 3.0][r.below(4) as usize];
    (meshes, cols, items, opts)
}

/// The guarantees, measured here and not by the planner: on every plate,
/// every pair of pieces at least the spacing apart by their real triangles,
/// every piece inside the region, none in an excluded corner or the tower's
/// square; filaments within the AMS's slots; every piece placed once.
fn assert_guarantees(plan: &Plan, bodies: &[PlanBody<'_>], opts: &Options, what: &str) {
    assert!(plan.breaches.is_empty(), "{what}: {:?}\n{}", plan.breaches, plan.report(opts));
    let shapes = verify::shapes(bodies, plan);
    let mut seen = vec![0usize; plan.pieces.len()];
    for plate in &plan.plates {
        for (n, &i) in plate.placements.iter().enumerate() {
            seen[plan.placements[i].piece] += 1;
            let s = &shapes[i];
            for &j in &plate.placements[n + 1..] {
                let d = verify::separation(s, &shapes[j], opts.spacing + 1.0);
                assert!(d >= opts.spacing - verify::SLACK, "{what}: {} and {} are {d} apart on {}", i, j, plate.name);
            }
            if plate.oversize {
                continue;
            }
            let r = opts.region;
            assert!(
                s.lo.0 >= r[0] - 1e-3 && s.lo.1 >= r[1] - 1e-3 && s.hi.0 <= r[2] + 1e-3 && s.hi.1 <= r[3] + 1e-3,
                "{what}: piece {i} at {:?}..{:?} leaves {r:?}",
                s.lo,
                s.hi
            );
            for keep in opts.exclude.iter().chain(plate.tower.iter()) {
                assert!(verify::separation(s, &verify::Shape::inside_of(*keep), 1.0) > 0.0, "{what}: piece {i} stands in {keep:?}");
            }
        }
        if plate.placements.len() > 1 && opts.ams > 0 {
            assert!(plate.colours.len() as u32 <= opts.ams, "{what}: {} filaments on {}", plate.colours.len(), plate.name);
        }
        if plate.colours.len() > 1 {
            assert!(plate.tower.is_some(), "{what}: {} mixes filaments and keeps no square for the tower", plate.name);
        }
    }
    assert!(seen.iter().all(|n| *n == 1), "{what}: every piece once: {seen:?}");
}

#[test]
fn random_piece_sets_keep_every_guarantee_and_plan_the_same_twice() {
    for seed in 1..=24u64 {
        let (meshes, cols, items, opts) = random_set(seed);
        let bodies: Vec<PlanBody<'_>> = meshes.iter().zip(&cols).enumerate().map(|(i, (m, c))| body(&format!("p{i}"), m, c)).collect();
        let a = plan(&bodies, &items, &opts);
        assert_guarantees(&a, &bodies, &opts, &format!("seed {seed}"));
        // A pin is honoured, or the piece is named in a note.
        for pl in &a.placements {
            let piece = &a.pieces[pl.piece];
            if a.unpinned.contains(&pl.piece) {
                assert!(a.notes.iter().any(|n| n.contains(&piece.name)), "seed {seed}: {} lost its pin silently", piece.name);
                continue;
            }
            if let Some(n) = piece.pin.plate {
                assert_eq!(a.plates[pl.plate].number, n, "seed {seed}: {}", piece.name);
            }
            if let Some((x, y)) = piece.pin.at {
                let f = pl.footprint;
                assert!(((f[0] + f[2]) / 2.0 - x).abs() <= 1.0 && ((f[1] + f[3]) / 2.0 - y).abs() <= 1.0, "seed {seed}: {} at {f:?}", piece.name);
            }
            if let Some(s) = piece.pin.spin {
                assert!((pl.spin - s).abs() < 1e-9, "seed {seed}: {}", piece.name);
            }
        }
        let b = plan(&bodies, &items, &opts);
        assert_eq!(a.report(&opts), b.report(&opts), "seed {seed}");
        for (x, y) in a.placements.iter().zip(&b.placements) {
            assert_eq!((x.plate, x.footprint, x.spin, x.candidate), (y.plate, y.footprint, y.spin, y.candidate), "seed {seed}");
        }
    }
}

#[test]
fn the_exact_separation_agrees_with_every_pair_of_triangles() {
    let mut r = Rng(77);
    for _ in 0..200 {
        let mut soup = |n: usize, ox: f64| -> verify::Shape {
            verify::Shape::from_tris(
                (0..n)
                    .map(|_| {
                        let (x, y) = (ox + r.range(0.0, 30.0), r.range(0.0, 30.0));
                        [(x, y), (x + r.range(-6.0, 6.0), y + r.range(-6.0, 6.0)), (x + r.range(-6.0, 6.0), y + r.range(-6.0, 6.0))]
                    })
                    .collect(),
            )
        };
        let (a, b) = (soup(12, 0.0), soup(12, 28.0));
        let mut brute = f64::INFINITY;
        for s in &a.tris {
            for t in &b.tris {
                brute = brute.min(verify::triangle_distance(s, t));
            }
        }
        let got = verify::separation(&a, &b, 8.0);
        if brute < 8.0 {
            assert!((got - brute).abs() < 1e-9, "{got} vs {brute}");
        } else {
            assert!(got.is_infinite(), "{got} vs {brute}");
        }
    }
    // Two unit squares 3 apart, and one inside a larger one.
    let sq = |x: f64, s: f64| verify::Shape::rect([x, 0.0, x + s, s]);
    assert!((verify::separation(&sq(0.0, 1.0), &sq(4.0, 1.0), 10.0) - 3.0).abs() < 1e-12);
    assert_eq!(verify::separation(&verify::Shape::rect([2.0, 2.0, 3.0, 3.0]), &sq(0.0, 10.0), 10.0), 0.0);
}

#[test]
fn a_bar_longer_than_the_plate_goes_on_its_diagonal() {
    let m = box_mesh(320.0, 8.0, 3.0);
    let bodies = [body("bar", &m, "#ff0000")];
    let opts = Options::default();
    let plan = plan(&bodies, &loose(1), &opts);
    assert!(!plan.plates[0].oversize, "{}", plan.report(&opts));
    assert_guarantees(&plan, &bodies, &opts, "bar");
    let spin = plan.placements[0].spin.rem_euclid(90.0);
    assert!((spin - 45.0).abs() < 1.0, "along the diagonal of a square plate: {}", plan.placements[0].spin);
}

#[test]
fn pieces_nest_in_each_others_hollows_and_beside_a_diagonal() {
    // Two L brackets with 100 mm arms on a plate 120 mm square: side by
    // side they need 204 mm, nested they fit.
    let l = |h: f64| {
        let pts = [(0.0, 0.0), (100.0, 0.0), (100.0, 12.0), (12.0, 12.0), (12.0, 100.0), (0.0, 100.0)];
        slab(&pts, &[[0, 1, 2], [0, 2, 3], [0, 3, 4], [0, 4, 5]], h)
    };
    let ms = [l(4.0), translated(&l(4.0), Vec3::new(400.0, 0.0, 0.0))];
    let bodies = [body("a", &ms[0], "#ff0000"), body("b", &ms[1], "#ff0000")];
    let opts = Options { region: [0.0, 0.0, 120.0, 120.0], ..Options::default() };
    let p = plan(&bodies, &loose(2), &opts);
    assert_eq!(p.plates.len(), 1, "{}", p.report(&opts));
    assert_guarantees(&p, &bodies, &opts, "two Ls");
    // A bar on the diagonal leaves two triangles; blocks go in both.
    let mut ms = vec![box_mesh(330.0, 10.0, 3.0)];
    ms.extend((1..7).map(|i| translated(&box_mesh(50.0, 50.0, 5.0), Vec3::new(400.0 * i as f64, 0.0, 0.0))));
    let bodies: Vec<PlanBody<'_>> = ms.iter().enumerate().map(|(i, m)| body(&format!("p{i}"), m, "#ff0000")).collect();
    let opts = Options::default();
    let p = plan(&bodies, &loose(7), &opts);
    assert_eq!(p.plates.len(), 1, "{}", p.report(&opts));
    assert_guarantees(&p, &bodies, &opts, "bar and blocks");
}

#[test]
fn a_piece_too_large_for_the_plate_is_alone_on_its_plate() {
    let ms = [box_mesh(500.0, 40.0, 5.0), translated(&box_mesh(30.0, 30.0, 5.0), Vec3::new(900.0, 0.0, 0.0)), translated(&box_mesh(30.0, 30.0, 5.0), Vec3::new(1300.0, 0.0, 0.0))];
    let bodies = [body("beam", &ms[0], "#ff0000"), body("a", &ms[1], "#ff0000"), body("b", &ms[2], "#ff0000")];
    let opts = Options::default();
    let p = plan(&bodies, &loose(3), &opts);
    assert_eq!(p.plates.len(), 2, "{}", p.report(&opts));
    let big = p.plates.iter().find(|q| q.oversize).expect("the beam's plate");
    assert_eq!(big.placements.len(), 1, "nothing shares the plate of a piece that does not fit: {}", p.report(&opts));
    assert!(p.notes.iter().any(|n| n.contains("beam does not fit")), "{:?}", p.notes);
    assert_guarantees(&p, &bodies, &opts, "beam");
}

#[test]
fn a_pinned_position_is_kept_and_packed_round() {
    let ms = spread(&box_mesh(40.0, 40.0, 5.0), 6);
    let bodies: Vec<PlanBody<'_>> = ms.iter().enumerate().map(|(i, m)| body(&format!("b{i}"), m, "#ff0000")).collect();
    let mut items = loose(6);
    items[3].pin.at = Some((128.0, 128.0));
    items[4].pin.at = Some((60.0, 200.0));
    // Two pieces cannot stand in one place: the second is placed elsewhere
    // and the plan says so.
    items[5].pin.at = Some((130.0, 130.0));
    for it in items.iter_mut() {
        it.pin.plate = Some(1);
    }
    let opts = Options::default();
    let p = plan(&bodies, &items, &opts);
    assert_guarantees(&p, &bodies, &opts, "pinned");
    let at = |i: usize| {
        let f = p.placements.iter().find(|pl| pl.piece == i).unwrap().footprint;
        ((f[0] + f[2]) / 2.0, (f[1] + f[3]) / 2.0)
    };
    assert_eq!(at(3), (128.0, 128.0));
    assert_eq!(at(4), (60.0, 200.0));
    assert_eq!(p.unpinned, vec![5]);
    assert!(p.notes.iter().any(|n| n.contains("b5 is pinned at (130, 130) on plate 1")), "{:?}", p.notes);
}

#[test]
fn a_pinned_plate_is_kept() {
    let ms = spread(&box_mesh(40.0, 40.0, 5.0), 3);
    let bodies: Vec<PlanBody<'_>> = ms.iter().enumerate().map(|(i, m)| body(&format!("b{i}"), m, "#ff0000")).collect();
    let mut items = loose(3);
    items[1].pin.plate = Some(2);
    let opts = Options::default();
    let p = plan(&bodies, &items, &opts);
    assert_eq!(p.plates.len(), 2, "{}", p.report(&opts));
    assert_eq!(p.plates[p.placements.iter().find(|pl| pl.piece == 1).unwrap().plate].number, 2);
    assert_guarantees(&p, &bodies, &opts, "pinned plate");
}

#[test]
fn the_piece_likeliest_to_warp_takes_the_middle_and_the_tower_a_corner() {
    // A long two-layer panel, and chunky blocks in another filament.
    let mut ms = vec![box_mesh(200.0, 60.0, 0.4)];
    ms.extend((1..9).map(|i| translated(&box_mesh(25.0, 25.0, 0.4), Vec3::new(400.0 * i as f64, 0.0, 0.0))));
    let bodies: Vec<PlanBody<'_>> =
        ms.iter().enumerate().map(|(i, m)| body(&format!("p{i}"), m, if i == 0 { "#ffffff" } else { "#000000" })).collect();
    let opts = Options { prefer: Prefer::Plates, ..Options::default() };
    let p = plan(&bodies, &loose(9), &opts);
    assert_eq!(p.plates.len(), 1, "{}", p.report(&opts));
    assert_guarantees(&p, &bodies, &opts, "panel");
    let off = |pl: &Placement| {
        let f = pl.footprint;
        (((f[0] + f[2]) / 2.0 - 128.0).powi(2) + ((f[1] + f[3]) / 2.0 - 128.0).powi(2)).sqrt()
    };
    let panel = p.placements.iter().find(|pl| pl.piece == 0).unwrap();
    assert!(panel.need > 0.5, "a long flat panel wants the middle: {}", panel.need);
    let over_middle = |f: [f64; 4]| f[0] < 128.0 && f[2] > 128.0 && f[1] < 128.0 && f[3] > 128.0;
    assert!(over_middle(panel.footprint) && off(panel) < 30.0, "the panel lies over the middle of the bed: {:?}", panel.footprint);
    for pl in p.placements.iter().filter(|pl| pl.piece != 0) {
        assert!(pl.need < 0.2, "a small block does not: {}", pl.need);
        assert!(!over_middle(pl.footprint), "{:?}", pl.footprint);
    }
    let e = p.plates[0].extent;
    assert!(((e[0] + e[2]) / 2.0 - 128.0).abs() <= 1.0 && ((e[1] + e[3]) / 2.0 - 128.0).abs() <= 1.0, "the group is centred on the bed: {e:?}");
    let t = p.plates[0].tower.expect("two filaments");
    assert!(t[0] >= 256.0 - 56.0 && t[1] >= 256.0 - 56.0, "the tower stands in the back right corner: {t:?}");
    // With the pull off the pieces are packed from the front left and
    // centred as a group.
    let flat = Options { centre_bias: 0.0, ..opts.clone() };
    let q = plan(&bodies, &loose(9), &flat);
    assert_guarantees(&q, &bodies, &flat, "panel, no pull");
    assert!(off(q.placements.iter().find(|pl| pl.piece == 0).unwrap()) > 10.0, "{}", q.report(&flat));
}

#[test]
fn filaments_keep_to_their_own_plates_when_mixing_costs_more_than_a_plate() {
    // Two tall blocks, one white, one black: together every layer changes
    // colour; apart neither plate needs a tower.
    let ms = spread(&box_mesh(40.0, 40.0, 40.0), 2);
    let bodies = [body("white", &ms[0], "#ffffff"), body("black", &ms[1], "#000000")];
    let opts = Options::default();
    let p = plan(&bodies, &loose(2), &opts);
    assert_eq!(p.plates.len(), 2, "{}", p.report(&opts));
    assert!(p.plates.iter().all(|q| q.tower.is_none() && q.colour_changes == 0));
    assert!(p.notes.iter().any(|n| n.contains("saves about 200 colour changes")), "{:?}", p.notes);
    assert!(p.report(&opts).contains("one filament: no prime tower, no purge"), "{}", p.report(&opts));
    // Asked for the fewest plates, they share one and its report counts
    // the changes and the purge.
    let dense = Options { prefer: Prefer::Plates, ..Options::default() };
    let q = plan(&bodies, &loose(2), &dense);
    assert_eq!(q.plates.len(), 1);
    assert_eq!(q.plates[0].colour_changes, 200);
    assert!((q.plates[0].tower_height - 40.0).abs() < 1e-9);
    assert!(q.report(&dense).contains("40 mm tall, ~60 cm³ purged"), "{}", q.report(&dense));
    // A machine with a hotend for each filament changes without a purge:
    // for blocks half as tall, 100 swaps are cheaper than a second plate,
    // and 100 purges are not.
    let half = spread(&box_mesh(40.0, 40.0, 20.0), 2);
    let halves = [body("white", &half[0], "#ffffff"), body("black", &half[1], "#000000")];
    assert_eq!(plan(&halves, &loose(2), &opts).plates.len(), 2);
    let two = Options { hotends: 2, ..Options::default() };
    let s = plan(&halves, &loose(2), &two);
    assert_eq!(s.plates.len(), 1, "{}", s.report(&two));
    assert!(s.report(&two).contains("each on a hotend of their own: changes without a purge"), "{}", s.report(&two));
    // Two thin panels cost two changes together: not worth a plate.
    let thin = spread(&box_mesh(40.0, 40.0, 0.4), 2);
    let bodies = [body("white", &thin[0], "#ffffff"), body("black", &thin[1], "#000000")];
    assert_eq!(plan(&bodies, &loose(2), &opts).plates.len(), 1);
}

#[test]
fn a_plate_pin_that_costs_colour_changes_is_said() {
    let ms = spread(&box_mesh(40.0, 40.0, 40.0), 2);
    let bodies = [body("white", &ms[0], "#ffffff"), body("black", &ms[1], "#000000")];
    let mut items = loose(2);
    items[0].pin.plate = Some(1);
    items[1].pin.plate = Some(1);
    let opts = Options::default();
    let p = plan(&bodies, &items, &opts);
    assert_eq!(p.plates.len(), 1, "the pins are kept: {}", p.report(&opts));
    assert!(p.notes.iter().any(|n| n.contains("the plate= pins cost about 200 colour changes")), "{:?}", p.notes);
}

#[test]
fn a_group_that_does_not_fit_one_plate_is_said() {
    let ms = spread(&box_mesh(150.0, 150.0, 5.0), 3);
    let bodies: Vec<PlanBody<'_>> = ms.iter().enumerate().map(|(i, m)| body(&format!("b{i}"), m, "#ff0000")).collect();
    let mut items = loose(3);
    for it in items.iter_mut() {
        it.pin.group = Some("set".to_owned());
    }
    let opts = Options::default();
    let p = plan(&bodies, &items, &opts);
    assert!(p.plates.len() > 1);
    assert!(p.notes.iter().any(|n| n.contains("group \"set\" does not fit one plate")), "{:?}", p.notes);
}

#[test]
fn two_pieces_each_pinned_their_own_way_up_are_not_joined() {
    // The wheel held on its axle is one print-in-place piece when left to
    // the analysis; given as two parts with their own `down=`, it is two.
    let (axle, wheel) = axle_and_wheel();
    let bodies = [body("axle", &axle, "#ff0000"), body("wheel", &wheel, "#000000")];
    let items = vec![
        Item { bodies: vec![0], pin: Pin { down: Some(-Vec3::Z), ..Pin::default() }, ..Item::default() },
        Item { bodies: vec![1], pin: Pin { down: Some(Vec3::X), ..Pin::default() }, ..Item::default() },
    ];
    let opts = Options::default();
    let p = plan(&bodies, &items, &opts);
    assert_eq!(p.pieces.len(), 2, "{}", p.report(&opts));
    assert!(p.notes.iter().any(|n| n.contains("each is pinned its own way up")), "{:?}", p.notes);
    assert!(p.pieces[1].best().pinned);
}
