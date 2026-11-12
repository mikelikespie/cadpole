//! Plating, end to end, against the examples' own print layouts.
//!
//! For each example the plan is made from the model as written (or, where
//! the model is not printable as modelled, from the pieces the script
//! exports) and compared with the layout the script places by hand: the
//! plan must need no more support, summed over its pieces, and no more
//! plates. Then a planned project is written and read back as bytes: its
//! plates, the objects on each, every object standing on z = 0 inside its
//! plate, no two overlapping, and the filament each part prints in.
//!
//! Its own binary: it evaluates four examples in a debug build.

use std::collections::{BTreeMap, HashMap};
use std::sync::Arc;

use cadpole_engine::cadpole_assets::AssetStore;
use cadpole_engine::cadpole_core::context::Context;
use cadpole_engine::cadpole_core::{BodyId, ParamValue};
use cadpole_engine::cadpole_print::plate::Pin;
use cadpole_engine::cadpole_script::plating::{self, PlateItem, PlateRequest};
use cadpole_engine::cadpole_script::{self, EvalInput, ScriptFile};

fn examples_dir() -> String {
    std::env::var("CADPOLE_EXAMPLES").unwrap_or_else(|_| "examples".into())
}

fn scripts_for(entry: &str) -> Vec<ScriptFile> {
    let dir = std::path::PathBuf::from(examples_dir());
    let mut scripts = vec![ScriptFile { path: "main.star".into(), source: std::fs::read_to_string(dir.join(entry)).unwrap() }];
    let mut sibs: Vec<_> = std::fs::read_dir(&dir)
        .unwrap()
        .flatten()
        .map(|e| e.path())
        .filter(|p| p.file_name().and_then(|n| n.to_str()) != Some(entry) && p.extension().and_then(|e| e.to_str()) == Some("star"))
        .collect();
    sibs.sort();
    for p in sibs {
        scripts.push(ScriptFile { path: p.file_name().unwrap().to_str().unwrap().to_owned(), source: std::fs::read_to_string(&p).unwrap() });
    }
    scripts
}

fn evaluate(entry: &str, overrides: &[(&str, ParamValue)]) -> Context {
    let (store, _) = cadpole_engine::cadpole_assets::load_dir(std::path::Path::new(&examples_dir()).join("assets").as_path());
    let assets: Arc<dyn AssetStore> = Arc::new(store);
    let ctx = cadpole_script::evaluate(EvalInput {
        scripts: scripts_for(entry),
        assets: Some(assets),
        overrides: overrides.iter().map(|(k, v)| ((*k).to_owned(), v.clone())).collect::<HashMap<_, _>>(),
        ..Default::default()
    });
    let errors: Vec<&str> = ctx
        .diagnostics
        .iter()
        .filter(|d| d.severity == cadpole_engine::cadpole_core::context::Severity::Error)
        .map(|d| d.message.as_str())
        .collect();
    assert!(errors.is_empty(), "{entry}: {errors:?}");
    ctx
}

fn named(ctx: &Context, names: &[&str]) -> Vec<BodyId> {
    names
        .iter()
        .map(|n| {
            ctx.bodies
                .iter()
                .rev()
                .find(|b| b.visible && b.name == *n)
                .unwrap_or_else(|| panic!("no body {n:?}"))
                .id
        })
        .collect()
}

/// The support a hand-made layout needs: each of its pieces as it stands.
fn hand_support(ctx: &mut Context, sets: &[Vec<BodyId>], printer: &str) -> f64 {
    let req = PlateRequest { printer: printer.to_owned(), ..Default::default() };
    let mut total = 0.0;
    for set in sets {
        for j in plating::judge_orientation(ctx, set, &req, None).unwrap() {
            total += j.verdict.placed.support_area;
        }
    }
    total
}

struct Outcome {
    support: f64,
    plates: usize,
    report: String,
}

fn plan(ctx: &mut Context, items: Vec<PlateItem>, printer: &str, ams: u32) -> Outcome {
    let req = PlateRequest { items, printer: printer.to_owned(), ams, ..Default::default() };
    let plated = plating::plate_bodies(ctx, &req).unwrap();
    let support = plated
        .plan
        .placements
        .iter()
        .map(|pl| plated.plan.pieces[pl.piece].candidates[pl.candidate].support_area)
        .sum();
    Outcome { support, plates: plated.plan.plates.len(), report: plated.report }
}

fn loose(ids: &[BodyId]) -> Vec<PlateItem> {
    ids.iter().map(|id| PlateItem { bodies: vec![*id], ..Default::default() }).collect()
}

/// The plan is at least as good as the hand layout: no more support (to a
/// square millimetre and a percent, for the tessellation), no more plates.
fn at_least_as_good(what: &str, planned: &Outcome, hand_support: f64, hand_plates: usize) {
    eprintln!(
        "{what}: plan {} mm² support on {} plate(s); by hand {} mm² on {}\n{}",
        planned.support, planned.plates, hand_support, hand_plates, planned.report
    );
    assert!(
        planned.support <= hand_support * 1.01 + 1.0,
        "{what}: the plan needs {} mm² of support, the hand layout {}\n{}",
        planned.support,
        hand_support,
        planned.report
    );
    assert!(planned.plates <= hand_plates, "{what}: {} plates against {} by hand\n{}", planned.plates, hand_plates, planned.report);
}

#[test]
fn the_enclosure_plan_matches_its_hand_layout() {
    let mut ctx = evaluate("enclosure.star", &[]);
    let model = ctx.select_bodies(&[], &["base".to_owned(), "lid".to_owned()], true).unwrap().ids;
    let hand = named(&ctx, &["base_print", "lid_print"]);
    let hs = hand_support(&mut ctx, &[vec![hand[0]], vec![hand[1]]], "bambu_h2d");
    let out = plan(&mut ctx, loose(&model), "bambu_h2d", 4);
    // Base and lid on one bed, as the example lays them out.
    at_least_as_good("enclosure", &out, hs, 1);
    assert!(out.report.contains("lid_i/lid: on the"), "the lid is turned over: {}", out.report);
}

#[test]
fn the_gear_toy_plan_matches_its_hand_layout() {
    let mut ctx = evaluate("gear_toy.star", &[]);
    // The pieces of its own project, as loose bodies: the core's are found
    // captive and printed in place, the lid and the crank set re-oriented.
    let specs: Vec<Vec<BodyId>> =
        ctx.exports.iter().filter(|e| e.path == "gear_toy.3mf").map(|e| e.bodies.clone()).collect();
    assert_eq!(specs.len(), 3, "three plates by hand: core, lid, crank");
    let hs = hand_support(&mut ctx, &specs, "bambu_h2d");
    // Each call's bodies as the script would hand them over: a list each,
    // the core kept as modelled (its gaps are on the layer grid). The
    // copies stand in each other's space, one plate apiece by hand.
    let items: Vec<PlateItem> = specs
        .iter()
        .enumerate()
        .map(|(k, ids)| PlateItem {
            bodies: ids.clone(),
            declared: true,
            pin: Pin { keep: k == 0, ..Pin::default() },
        })
        .collect();
    let out = plan(&mut ctx, items, "bambu_h2d", 4);
    at_least_as_good("gear_toy", &out, hs, 3);
}

#[test]
fn the_desk_mascot_plan_matches_its_hand_layout() {
    let mut ctx = evaluate("desk_mascot.star", &[]);
    let model: Vec<BodyId> = ctx.select_bodies(&[], &[], false).unwrap().ids;
    let hand: Vec<Vec<BodyId>> =
        ctx.exports.iter().filter(|e| e.path == "desk_mascot.3mf").map(|e| e.bodies.clone()).collect();
    let hs = hand_support(&mut ctx, &hand, "bambu_h2c");
    // Its hand layout is one plate of five filaments: allow as many.
    let out = plan(&mut ctx, loose(&model), "bambu_h2c", 8);
    at_least_as_good("desk_mascot", &out, hs, 1);
}

#[test]
fn the_thumbscrew_plan_matches_its_print_layout() {
    let mut hand_ctx = evaluate("thumbscrew.star", &[("layout", ParamValue::Text("print".into()))]);
    let hand = named(&hand_ctx, &["screw_print", "nut_print"]);
    let hs = hand_support(&mut hand_ctx, &[vec![hand[0]], vec![hand[1]]], "bambu_h2c");
    let mut ctx = evaluate("thumbscrew.star", &[]);
    let model: Vec<BodyId> = ctx.select_bodies(&[], &[], false).unwrap().ids;
    let out = plan(&mut ctx, loose(&model), "bambu_h2c", 4);
    // The nut is only 0.11 mm clear of the screw: printed apart.
    assert!(out.report.contains("printed apart"), "{}", out.report);
    at_least_as_good("thumbscrew", &out, hs, 1);
}

// ---------------------------------------------------------------------------
// The written project, read back
// ---------------------------------------------------------------------------

type M = [f64; 12];

fn matrix(s: &str) -> M {
    let v: Vec<f64> = s.split_whitespace().map(|x| x.parse().unwrap()).collect();
    assert_eq!(v.len(), 12, "{s}");
    let mut m = [0.0; 12];
    m.copy_from_slice(&v);
    m
}

/// 3MF's row-vector affine: p' = p * M, with the translation last.
fn apply(m: &M, p: [f64; 3]) -> [f64; 3] {
    [
        p[0] * m[0] + p[1] * m[3] + p[2] * m[6] + m[9],
        p[0] * m[1] + p[1] * m[4] + p[2] * m[7] + m[10],
        p[0] * m[2] + p[1] * m[5] + p[2] * m[8] + m[11],
    ]
}

fn meta<'a>(n: roxmltree::Node<'a, 'a>, key: &str) -> Option<&'a str> {
    n.children().find(|c| c.has_tag_name("metadata") && c.attribute("key") == Some(key)).and_then(|c| c.attribute("value"))
}

#[test]
fn a_planned_project_reads_back_with_its_plates_objects_and_filaments() {
    let mut ctx = evaluate("print_plan.star", &[]);
    let specs: Vec<_> = ctx.exports.iter().filter(|e| e.path == "print_plan.3mf").cloned().collect();
    let planned_plates: Vec<String> = {
        let mut v: Vec<String> = Vec::new();
        for s in &specs {
            if let Some(cadpole_engine::cadpole_core::context::PlateRef::Name(n)) = &s.plate {
                if !v.contains(n) {
                    v.push(n.clone());
                }
            }
        }
        v
    };
    assert_eq!(planned_plates, ["Test coupons", "Parts"]);
    let out = cadpole_engine::print::export_bambu_project(&mut ctx, &specs, "print_plan", false, None).unwrap();
    let z = cadpole_engine::cadpole_export_3mf::zip::read(&out.bytes).unwrap();

    // Plates and what is on them.
    let settings = z.text("Metadata/model_settings.config").unwrap();
    let sdoc = roxmltree::Document::parse(&settings).unwrap();
    let mut plate_of: BTreeMap<String, (u32, String)> = BTreeMap::new();
    let mut plate_names: Vec<String> = Vec::new();
    for p in sdoc.root_element().children().filter(|n| n.has_tag_name("plate")) {
        let id: u32 = meta(p, "plater_id").unwrap().parse().unwrap();
        let name = meta(p, "plater_name").unwrap().to_owned();
        plate_names.push(name.clone());
        for inst in p.children().filter(|n| n.has_tag_name("model_instance")) {
            plate_of.insert(meta(inst, "object_id").unwrap().to_owned(), (id, name.clone()));
        }
    }
    assert_eq!(plate_names, planned_plates);
    let mut names: BTreeMap<String, String> = BTreeMap::new();
    let mut extruders: BTreeMap<String, Vec<u32>> = BTreeMap::new();
    for o in sdoc.root_element().children().filter(|n| n.has_tag_name("object")) {
        let id = o.attribute("id").unwrap().to_owned();
        names.insert(id.clone(), meta(o, "name").unwrap().to_owned());
        extruders.insert(
            id,
            o.children().filter(|n| n.has_tag_name("part")).map(|p| meta(p, "extruder").unwrap().parse().unwrap()).collect(),
        );
    }
    let on = |plate: &str| -> Vec<&str> {
        plate_of.iter().filter(|(_, (_, n))| n == plate).map(|(id, _)| names[id].as_str()).collect()
    };
    assert_eq!(on("Test coupons"), ["latch coupon"]);
    let mut parts = on("Parts");
    parts.sort();
    assert_eq!(parts, ["axle and wheel", "badge and stripe", "box_body", "latch", "lid"]);
    // Filaments: the badge's stripe and the wheel each in a slot of their
    // own colour, every part with one.
    let slots = |name: &str| extruders[names.iter().find(|(_, n)| *n == name).unwrap().0].clone();
    let (badge, wheel) = (slots("badge and stripe"), slots("axle and wheel"));
    assert_eq!(badge.len(), 2);
    assert_ne!(badge[0], badge[1], "the stripe prints in its own filament");
    assert_ne!(wheel[0], wheel[1], "the wheel prints in its own filament");
    let colours = z.text("Metadata/project_settings.config").unwrap();
    let ps: serde_json::Value = serde_json::from_str(&colours).unwrap();
    let list = ps["filament_colour"].as_array().unwrap();
    assert_eq!(list.len(), 4, "four colours in the model: {list:?}");
    assert!(extruders.values().flatten().all(|s| *s >= 1 && *s as usize <= list.len()));

    // Every object on z = 0, inside its plate, and clear of the others.
    let root = z.text("3D/3dmodel.model").unwrap();
    let rdoc = roxmltree::Document::parse(&root).unwrap();
    let res = rdoc.root_element().children().find(|n| n.has_tag_name("resources")).unwrap();
    let build = rdoc.root_element().children().find(|n| n.has_tag_name("build")).unwrap();
    let bed = [350.0f64, 320.0]; // the H2D the example plates for
    let plates = plate_names.len() as u32;
    let mut boxes: BTreeMap<u32, Vec<(String, [f64; 4])>> = BTreeMap::new();
    for item in build.children().filter(|n| n.has_tag_name("item")) {
        let oid = item.attribute("objectid").unwrap();
        let it = matrix(item.attribute("transform").unwrap());
        let obj = res.children().find(|n| n.has_tag_name("object") && n.attribute("id") == Some(oid)).unwrap();
        let (mut lo, mut hi) = ([f64::MAX; 3], [f64::MIN; 3]);
        for c in obj.descendants().filter(|n| n.has_tag_name("component")) {
            let path = c.attributes().find(|a| a.name() == "path").unwrap().value().trim_start_matches('/').to_owned();
            let ct = matrix(c.attribute("transform").unwrap());
            let text = z.text(&path).unwrap();
            let odoc = roxmltree::Document::parse(&text).unwrap();
            for v in odoc.descendants().filter(|n| n.has_tag_name("vertex")) {
                let p = [
                    v.attribute("x").unwrap().parse().unwrap(),
                    v.attribute("y").unwrap().parse().unwrap(),
                    v.attribute("z").unwrap().parse().unwrap(),
                ];
                let w = apply(&it, apply(&ct, p));
                for k in 0..3 {
                    lo[k] = lo[k].min(w[k]);
                    hi[k] = hi[k].max(w[k]);
                }
            }
        }
        let name = &names[oid];
        assert!(lo[2].abs() < 1e-3, "{name} stands on the bed: lowest z {}", lo[2]);
        let (plate, _) = &plate_of[oid];
        let origin = cadpole_engine::cadpole_export_3mf::bambu::plate_origin(*plate, plates, bed);
        let local = [lo[0] - origin[0], lo[1] - origin[1], hi[0] - origin[0], hi[1] - origin[1]];
        assert!(
            local[0] >= -1e-3 && local[1] >= -1e-3 && local[2] <= bed[0] + 1e-3 && local[3] <= bed[1] + 1e-3,
            "{name} is on plate {plate}: {local:?}"
        );
        boxes.entry(*plate).or_default().push((name.clone(), local));
    }
    for (plate, objs) in &boxes {
        for i in 0..objs.len() {
            for j in i + 1..objs.len() {
                let (a, b) = (objs[i].1, objs[j].1);
                let apart = a[2] <= b[0] || b[2] <= a[0] || a[3] <= b[1] || b[3] <= a[1];
                assert!(apart, "{} and {} overlap on plate {plate}: {a:?} {b:?}", objs[i].0, objs[j].0);
            }
        }
    }
}
