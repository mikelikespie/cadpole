//! The settings side of a Bambu project: reading a template, mapping
//! the model's filaments onto it, the version a project claims, and Cadpole's
//! built-in file. The template here is a hand-made stand-in with the *shape*
//! of a saved two-nozzle project — its key names and list lengths — and
//! values of our own; no Bambu profile or saved project is in the tree.

use serde_json::{json, Map, Value};

use super::*;
use crate::Filament;

/// `n` plates, each with its pieces over `footprint` on `machine` and its
/// prime tower where the layout puts a default-width tower for them
/// ([`tower_spot`]).
fn plates(machine: &Machine, n: usize, footprint: [f64; 4]) -> Vec<PlateUse> {
    let spot = tower_spot(machine, TowerRoom::for_width(TOWER_WIDTH_DEFAULT), &[footprint]);
    vec![PlateUse { tower: spot }; n]
}

fn filament(hex: &str) -> Filament {
    Filament { name: String::new(), color: cadpole_core::paint::unhex(hex) }
}

/// A two-nozzle machine with `n` filaments, each filament with two extruder
/// variants: the list shapes of a saved H2D/H2C project (per filament `n`,
/// per filament and variant `2n`, per nozzle 2, per nozzle and variant 4,
/// the flush matrix `2 x n x n`, the `[process, filaments.., printer]`
/// lists `n + 2`, the AMS drying lists `4n`).
fn two_nozzle_settings(n: usize, colours: &[&str]) -> Map<String, Value> {
    let per = |v: &str| Value::Array(vec![Value::String(v.into()); n]);
    let per_variant = |v: &str| Value::Array(vec![Value::String(v.into()); 2 * n]);
    let mut flush = Vec::new();
    for b in 0..2 {
        for i in 0..n {
            for j in 0..n {
                flush.push(Value::String(if i == j { "0".into() } else { format!("{}", 100 * (b + 1) + 10 * i + j) }));
            }
        }
    }
    let mut m = Map::new();
    let pairs: Vec<(&str, Value)> = vec![
        ("from", json!("project")),
        ("name", json!("project_settings")),
        ("version", json!("02.06.00.51")),
        ("printer_model", json!("Test Twin")),
        ("printer_settings_id", json!("Test Twin 0.4 nozzle")),
        ("print_settings_id", json!("0.20mm Test @Twin")),
        ("printable_area", json!(["0x0", "330x0", "330x320", "0x320"])),
        ("printable_height", json!("325")),
        ("bed_exclude_area", json!([])),
        ("extruder_printable_area", json!(["0x0,325x0,325x320,0x320", "25x0,330x0,330x320,25x320"])),
        ("extruder_printable_height", json!(["320", "325"])),
        ("extruder_max_nozzle_count", json!(["1", "6"])),
        ("nozzle_diameter", json!(["0.4", "0.4"])),
        ("outer_wall_speed", json!(["200", "200", "200", "200"])),
        ("printer_extruder_variant", json!(["Standard", "High Flow", "Standard", "High Flow"])),
        ("filament_colour", Value::Array(colours.iter().map(|c| json!(c)).collect())),
        ("filament_settings_id", per("Test PLA @Twin")),
        ("filament_type", per("PLA")),
        ("filament_ids", per("T0001")),
        ("filament_map", per("1")),
        ("filament_extruder_variant", Value::Array((0..n).flat_map(|_| [json!("Standard"), json!("High Flow")]).collect())),
        ("filament_self_index", Value::Array((0..n).flat_map(|i| [json!((i + 1).to_string()), json!((i + 1).to_string())]).collect())),
        ("filament_dev_ams_drying_temperature", Value::Array(vec![json!("45"); 4 * n])),
        ("nozzle_temperature", per_variant("220")),
        ("hot_plate_temp", per("55")),
        ("required_nozzle_HRC", per("3")),
        ("flush_volumes_matrix", Value::Array(flush)),
        ("flush_volumes_vector", per_variant("140")),
        ("different_settings_to_system", Value::Array(std::iter::once(json!("brim_type")).chain((0..n).map(|_| json!(""))).chain(std::iter::once(json!(""))).collect())),
        ("filament_is_mixed", json!(["0"])),
        ("wall_filament", json!("1")),
        ("sparse_infill_filament", json!("1")),
        ("solid_infill_filament", json!("1")),
        ("raft_first_layer_expansion", json!("-1")),
        ("tree_support_wall_count", json!("-1")),
        ("enable_support", json!("0")),
        ("raft_layers", json!("0")),
        ("wipe_tower_x", json!(["165", "165"])),
        ("wipe_tower_y", json!(["250", "250"])),
    ];
    for (k, v) in pairs {
        m.insert(k.into(), v);
    }
    m
}

fn template(n: usize, colours: &[&str], app: &str) -> Template {
    let text = serde_json::to_string(&Value::Object(two_nozzle_settings(n, colours))).unwrap();
    Template::from_parts(app, &text).expect("a valid template")
}

fn parse(json: &str) -> Map<String, Value> {
    match serde_json::from_str::<Value>(json).expect("the settings are JSON") {
        Value::Object(m) => m,
        other => panic!("not an object: {other}"),
    }
}

fn texts(v: &Value) -> Vec<String> {
    v.as_array().expect("a list").iter().map(|x| x.as_str().expect("a string").to_owned()).collect()
}

#[test]
fn a_project_claims_at_most_the_newest_bambu_version_orca_opens() {
    assert_eq!(version_of("BambuStudio-02.06.00.51"), Some([2, 6, 0, 51]));
    assert_eq!(version_of("OrcaSlicer-2.4.2"), None);
    assert_eq!(version_of("BambuStudio-2.6"), None);
    assert_eq!(application_for(None), APPLICATION);
    // OrcaSlicer 2.4.2 refuses 02.05.00.00 and later, so a newer template's
    // version is capped; an older one's is kept.
    assert_eq!(application_for(Some(&template(2, &["#000000", "#FFFFFF"], "BambuStudio-02.06.00.51"))), APPLICATION);
    assert_eq!(application_for(Some(&template(2, &["#000000", "#FFFFFF"], "BambuStudio-02.08.02.61"))), APPLICATION);
    assert_eq!(
        application_for(Some(&template(2, &["#000000", "#FFFFFF"], "BambuStudio-02.03.01.51"))),
        "BambuStudio-02.03.01.51"
    );
    assert_eq!(
        application_for(Some(&template(2, &["#000000", "#FFFFFF"], "BambuStudio-02.04.99.99"))),
        "BambuStudio-02.04.99.99"
    );
    let v = version_of(APPLICATION).unwrap();
    assert!((v[0], v[1]) <= NEWEST_FOR_ORCA);
}

#[test]
fn a_template_must_be_a_bambu_studio_project_with_settings() {
    let ok = serde_json::to_string(&Value::Object(two_nozzle_settings(2, &["#000000", "#FFFFFF"]))).unwrap();
    let e = Template::from_parts("OrcaSlicer-2.4.2", &ok).unwrap_err();
    assert!(e.contains("OrcaSlicer-2.4.2") && e.contains("Bambu Studio"), "{e}");
    assert!(Template::from_parts("", &ok).unwrap_err().contains("does not say"));
    assert!(Template::from_parts("BambuStudio-02.04.00.70", "{ nope").unwrap_err().contains("not JSON"));
    assert!(Template::from_parts("BambuStudio-02.04.00.70", "[1, 2]").unwrap_err().contains("not a JSON object"));
    let mut no_colours = two_nozzle_settings(2, &["#000000", "#FFFFFF"]);
    no_colours.remove("filament_colour");
    let e = Template::from_parts("BambuStudio-02.04.00.70", &Value::Object(no_colours).to_string()).unwrap_err();
    assert!(e.contains("filament_colour"), "{e}");
    let mut no_bed = two_nozzle_settings(2, &["#000000", "#FFFFFF"]);
    no_bed.remove("printable_area");
    let e = Template::from_parts("BambuStudio-02.04.00.70", &Value::Object(no_bed).to_string()).unwrap_err();
    assert!(e.contains("printable_area"), "{e}");
}

#[test]
fn a_template_is_read_from_a_saved_package_and_only_its_settings_and_model() {
    let settings = Value::Object(two_nozzle_settings(3, &["#000000", "#FFFFFF", "#FF0000"])).to_string();
    let root = "<?xml version=\"1.0\" encoding=\"UTF-8\"?>\n<model unit=\"millimeter\" \
                xmlns=\"http://schemas.microsoft.com/3dmanufacturing/core/2015/02\">\n \
                <metadata name=\"Application\">BambuStudio-02.06.00.51</metadata>\n <resources/>\n <build/>\n</model>\n";
    let mut z = crate::zip::ZipWriter::new();
    z.add(crate::MODEL_PART, root.as_bytes());
    z.add(crate::BAMBU_PROJECT_CONFIG, settings.as_bytes());
    z.add("3D/Objects/object_1.model", &vec![b'x'; 10_000]);
    let t = Template::from_3mf(&z.finish()).expect("a template");
    assert_eq!(t.application, "BambuStudio-02.06.00.51");
    assert_eq!(t.filaments(), 3);
    assert_eq!(t.printer_model(), "Test Twin");
    assert_eq!(t.printer_preset(), "Test Twin 0.4 nozzle");

    // A model with no settings is not a project.
    let mut z = crate::zip::ZipWriter::new();
    z.add(crate::MODEL_PART, root.as_bytes());
    let e = Template::from_3mf(&z.finish()).unwrap_err();
    assert!(e.contains("project_settings.config") && e.contains("not a Bambu Studio project"), "{e}");
    assert!(Template::from_3mf(b"not a zip").unwrap_err().contains("not a 3MF package"));
}

#[test]
fn a_template_says_what_each_nozzle_reaches() {
    let m = template(2, &["#000000", "#FFFFFF"], "BambuStudio-02.06.00.51").machine();
    assert_eq!(m.name, "Test Twin");
    assert_eq!(m.area, [0.0, 0.0, 330.0, 320.0]);
    assert_eq!(m.bed(), [330.0, 320.0]);
    assert_eq!(m.height, 325.0);
    assert_eq!(m.nozzles.len(), 2);
    assert_eq!(m.nozzles[0], Reach { area: [0.0, 0.0, 325.0, 320.0], height: 320.0, hotends: 1 });
    assert_eq!(m.nozzles[1], Reach { area: [25.0, 0.0, 330.0, 320.0], height: 325.0, hotends: 6 });
    assert_eq!(m.shared(), [25.0, 0.0, 325.0, 320.0], "the 300 mm both nozzles reach");
    assert_eq!(m.shared_height(), 320.0);
    assert_eq!(m.hotends(), 7, "one hotend on the left, six swapped on the right");
    assert!(m.exclude.is_empty());
}

#[test]
fn the_models_filaments_are_mapped_onto_a_template_with_fewer() {
    let t = template(3, &["#111111", "#EEEEEE", "#CC0000"], "BambuStudio-02.06.00.51");
    let colours = ["#111111", "#EEEEEE", "#00AA00", "#0000CC", "#FFFF00", "#FF00FF", "#00FFFF"];
    let palette: Vec<Filament> = colours.iter().map(|c| filament(c)).collect();
    let out = project_settings(Some((&t, "twin_3.3mf")), &t.machine(), &palette, &plates(&t.machine(), 5, [100.0, 100.0, 200.0, 200.0]));
    let ps = parse(&out.json);
    assert_eq!(out.application, APPLICATION);
    assert_eq!(texts(&ps["filament_colour"]), colours.iter().map(|c| c.to_string()).collect::<Vec<_>>());
    // Every per-filament list now has seven blocks, the new ones copying
    // filament 1's.
    assert_eq!(texts(&ps["filament_settings_id"]), vec!["Test PLA @Twin"; 7]);
    assert_eq!(texts(&ps["filament_type"]).len(), 7);
    assert_eq!(texts(&ps["filament_map"]).len(), 7);
    assert_eq!(texts(&ps["hot_plate_temp"]).len(), 7, "a per-filament list not named filament_*");
    assert_eq!(texts(&ps["required_nozzle_HRC"]).len(), 7);
    assert_eq!(texts(&ps["nozzle_temperature"]).len(), 14, "two variants per filament");
    assert_eq!(texts(&ps["filament_extruder_variant"]).len(), 14);
    assert_eq!(texts(&ps["flush_volumes_vector"]).len(), 14);
    assert_eq!(texts(&ps["filament_dev_ams_drying_temperature"]).len(), 28, "four per filament");
    assert_eq!(
        texts(&ps["filament_self_index"]),
        ["1", "1", "2", "2", "3", "3", "4", "4", "5", "5", "6", "6", "7", "7"].map(String::from).to_vec(),
        "each filament's own number, not filament 1's"
    );
    // The machine's lists keep their length.
    assert_eq!(texts(&ps["nozzle_diameter"]).len(), 2);
    assert_eq!(texts(&ps["outer_wall_speed"]).len(), 4);
    assert_eq!(texts(&ps["printer_extruder_variant"]).len(), 4);
    assert_eq!(texts(&ps["printable_area"]).len(), 4);
    assert_eq!(texts(&ps["filament_is_mixed"]), ["0"], "a filament_ list that is not per filament");
    // [process, filament 1..7, printer].
    let d = texts(&ps["different_settings_to_system"]);
    assert_eq!(d.len(), 9);
    assert_eq!(d[0], "brim_type");
    // The part's own filament prints its walls and infill.
    for k in ["wall_filament", "sparse_infill_filament", "solid_infill_filament"] {
        assert_eq!(ps[k], json!("0"), "{k}");
    }
    // The two values OrcaSlicer refuses, as Bambu wrote them before -1.
    assert_eq!(ps["raft_first_layer_expansion"], json!("2"));
    assert_eq!(ps["tree_support_wall_count"], json!("0"));
    // A prime-tower position for every plate: at the front of the right-hand
    // column of what both nozzles reach, which the pieces leave free.
    assert_eq!(texts(&ps["wipe_tower_x"]), vec!["275"; 5]);
    assert_eq!(texts(&ps["wipe_tower_y"]), vec!["5"; 5]);
    // Said once, naming the template and the preset the new slots take.
    assert!(
        out.warnings.iter().any(|w| w.contains("twin_3.3mf") && w.contains("holds 3 filaments") && w.contains("4-7")),
        "{:?}",
        out.warnings
    );
    assert!(!out.warnings.iter().any(|w| w.contains("tree supports") || w.contains("raft")), "no support, no warning");
}

#[test]
fn the_flush_matrix_keeps_a_pair_only_where_both_colours_are_the_templates() {
    let t = template(3, &["#111111", "#EEEEEE", "#CC0000"], "BambuStudio-02.06.00.51");
    // Slots 1 and 2 keep their colours; slot 3 changes; slot 4 is new.
    let palette: Vec<Filament> = ["#111111", "#eeeeee", "#00AA00", "#0000CC"].iter().map(|c| filament(c)).collect();
    let ps = parse(&project_settings(Some((&t, "t")), &t.machine(), &palette, &plates(&t.machine(), 1, [100.0, 100.0, 200.0, 200.0])).json);
    let m: Vec<f64> = texts(&ps["flush_volumes_matrix"]).iter().map(|v| v.parse().unwrap()).collect();
    assert_eq!(m.len(), 2 * 4 * 4, "per nozzle, 4 x 4");
    for b in 0..2 {
        let at = |i: usize, j: usize| m[b * 16 + i * 4 + j];
        let worst = (100 * (b + 1) + 10 * 2 + 1) as f64; // the template's largest in this block
        for i in 0..4 {
            assert_eq!(at(i, i), 0.0);
        }
        assert_eq!(at(0, 1), (100 * (b + 1) + 1) as f64, "both colours kept: the template's own volume");
        assert_eq!(at(1, 0), (100 * (b + 1) + 10) as f64);
        for (i, j) in [(0, 2), (2, 0), (1, 3), (3, 2), (2, 3)] {
            assert_eq!(at(i, j), worst, "({i}, {j}): a changed or new colour purges the template's worst case");
        }
    }
}

#[test]
fn fewer_model_filaments_cut_the_templates_lists() {
    let t = template(4, &["#111111", "#EEEEEE", "#CC0000", "#00CC00"], "BambuStudio-02.03.01.51");
    let palette: Vec<Filament> = ["#111111", "#EEEEEE"].iter().map(|c| filament(c)).collect();
    let out = project_settings(Some((&t, "t")), &t.machine(), &palette, &plates(&t.machine(), 2, [100.0, 100.0, 200.0, 200.0]));
    let ps = parse(&out.json);
    assert_eq!(out.application, "BambuStudio-02.03.01.51", "an older template's own version");
    assert_eq!(texts(&ps["filament_settings_id"]).len(), 2);
    assert_eq!(texts(&ps["nozzle_temperature"]).len(), 4);
    assert_eq!(texts(&ps["different_settings_to_system"]).len(), 4);
    assert_eq!(texts(&ps["flush_volumes_matrix"]).len(), 2 * 2 * 2);
    // Four filaments and four per-nozzle-variant entries: the names, not the
    // lengths, say which is which.
    assert_eq!(texts(&ps["outer_wall_speed"]).len(), 4);
    assert_eq!(texts(&ps["printer_extruder_variant"]).len(), 4);
    assert!(out.warnings.is_empty(), "{:?}", out.warnings);
}

#[test]
fn replacing_a_minus_one_is_said_when_the_template_uses_tree_supports() {
    let mut s = two_nozzle_settings(2, &["#000000", "#FFFFFF"]);
    s.insert("enable_support".into(), json!("1"));
    s.insert("support_type".into(), json!("tree(auto)"));
    let t = Template::from_parts("BambuStudio-02.06.00.51", &Value::Object(s).to_string()).unwrap();
    let palette: Vec<Filament> = ["#000000", "#FFFFFF"].iter().map(|c| filament(c)).collect();
    let out = project_settings(Some((&t, "trees.3mf")), &t.machine(), &palette, &plates(&t.machine(), 1, [100.0, 100.0, 200.0, 200.0]));
    assert!(
        out.warnings.iter().any(|w| w.contains("trees.3mf") && w.contains("tree supports") && w.contains("tree_support_wall_count -1 -> 0")),
        "{:?}",
        out.warnings
    );
}

/// An H2D-like machine as the engine builds one from the printer table.
fn twin_machine() -> Machine {
    Machine {
        name: "Test Twin".into(),
        area: [0.0, 0.0, 350.0, 320.0],
        height: 325.0,
        exclude: Vec::new(),
        nozzles: vec![
            Reach { area: [0.0, 0.0, 325.0, 320.0], height: 320.0, hotends: 1 },
            Reach { area: [25.0, 0.0, 350.0, 320.0], height: 325.0, hotends: 1 },
        ],
        nozzle_diameter: 0.4,
        presets: Some(Presets {
            model: "Test Twin".into(),
            printer: "Test Twin 0.4 nozzle".into(),
            process: "0.20mm Test @Twin".into(),
            filament: "Test PLA @Twin".into(),
            filament_id: "T0001".into(),
        }),
    }
}

#[test]
fn the_built_in_settings_name_the_presets_and_carry_the_machine_and_the_filaments() {
    let palette: Vec<Filament> = ["#B5573B", "#1E2124", "#B9B4AA", "#E3A21A"].iter().map(|c| filament(c)).collect();
    let out = project_settings(None, &twin_machine(), &palette, &plates(&twin_machine(), 3, [100.0, 100.0, 200.0, 200.0]));
    let ps = parse(&out.json);
    assert_eq!(out.application, APPLICATION);
    assert_eq!(ps["from"], json!("project"));
    assert_eq!(ps["version"], json!(APPLICATION.strip_prefix("BambuStudio-").unwrap()));
    assert_eq!(ps["printer_model"], json!("Test Twin"));
    assert_eq!(ps["printer_settings_id"], json!("Test Twin 0.4 nozzle"));
    assert_eq!(ps["print_settings_id"], json!("0.20mm Test @Twin"));
    assert_eq!(texts(&ps["printable_area"]), ["0x0", "350x0", "350x320", "0x320"]);
    assert_eq!(ps["printable_height"], json!("325"));
    assert_eq!(texts(&ps["nozzle_diameter"]), ["0.4", "0.4"]);
    assert_eq!(texts(&ps["extruder_printable_area"]), ["0x0,325x0,325x320,0x320", "25x0,350x0,350x320,25x320"]);
    assert_eq!(texts(&ps["extruder_printable_height"]), ["320", "325"]);
    assert_eq!(texts(&ps["filament_colour"]), ["#B5573B", "#1E2124", "#B9B4AA", "#E3A21A"]);
    // The lists a slicer indexes by filament number, one entry each.
    for k in [
        "filament_settings_id",
        "filament_ids",
        "filament_type",
        "filament_diameter",
        "filament_density",
        "filament_is_support",
        "filament_shrink",
        "filament_start_gcode",
    ] {
        assert_eq!(texts(&ps[k]).len(), 4, "{k}");
    }
    for k in ["wall_filament", "sparse_infill_filament", "solid_infill_filament"] {
        assert_eq!(ps[k], json!("0"), "{k}");
    }
    // The one process value, without which OrcaSlicer fails a painted face.
    assert_eq!(ps["outer_wall_line_width"], json!("0.42"));
    // No G-code of ours for the machine, ever.
    assert!(!ps.contains_key("machine_start_gcode") && !ps.contains_key("change_filament_gcode"));
    assert!(texts(&ps["filament_start_gcode"]).iter().all(String::is_empty));
    assert!(out.warnings.iter().any(|w| w.contains("no template=") && w.contains("Test Twin")), "{:?}", out.warnings);
}

#[test]
fn a_one_nozzle_machine_writes_its_excluded_corner_and_no_nozzle_lists() {
    let m = Machine {
        name: "Test One".into(),
        area: [0.0, 0.0, 256.0, 256.0],
        height: 250.0,
        exclude: vec![[0.0, 0.0, 18.0, 28.0]],
        ..Machine::default()
    };
    let ps = parse(&project_settings(None, &m, &[filament("#FFFFFF")], &plates(&m, 1, [100.0, 100.0, 150.0, 150.0])).json);
    assert_eq!(texts(&ps["bed_exclude_area"]), ["0x0", "18x0", "18x28", "0x28"]);
    assert_eq!(texts(&ps["nozzle_diameter"]), ["0.4"]);
    assert!(!ps.contains_key("extruder_printable_area"));
    assert!(!ps.contains_key("printer_settings_id"), "a machine with no presets names none");
    assert_eq!(m.shared(), m.area);
    assert_eq!(m.hotends(), 1);
}

#[test]
fn a_prime_tower_is_placed_where_both_nozzles_reach_clear_of_the_pieces() {
    let mut settings = two_nozzle_settings(2, &["#000000", "#FFFFFF"]);
    // A template saved with a 15 mm tower at x = 297: Bambu Studio takes it,
    // OrcaSlicer's wider tower crosses the left nozzle's 325 mm.
    settings.insert("wipe_tower_x".into(), json!(["297.425"]));
    settings.insert("wipe_tower_y".into(), json!(["137.09"]));
    settings.insert("prime_tower_width".into(), json!("15"));
    let t = Template::from_parts("BambuStudio-02.04.00.70", &Value::Object(settings).to_string()).unwrap();
    let room = tower_room(Some(&t)).expect("the template prints a tower");
    assert_eq!(room, TowerRoom { side: TOWER_SIDE, margin: TOWER_MARGIN }, "a 15 mm tower is kept 45 mm wide");
    let m = t.machine();
    // Plate 1: the right-hand column (x 275, 325 - 5 - 45) is clear of the
    // pieces from front to back; the tower starts at its front and has the
    // plate's whole depth to grow into.
    let p1 = [[130.0, 126.0, 220.0, 194.0]];
    assert_eq!(tower_spot(&m, room, &p1), Some([275.0, 5.0]));
    // Plate 2: the pieces fill the right column from y 200 back, leaving it
    // 195 mm; the left column is clear all the way: the longer run wins.
    let p2 = [[200.0, 200.0, 320.0, 315.0]];
    assert_eq!(tower_spot(&m, room, &p2), Some([30.0, 5.0]));
    // Plate 3: pieces over everything both nozzles reach: no room.
    assert_eq!(tower_spot(&m, room, &[[20.0, 0.0, 330.0, 320.0]]), None);
    // Every spot found keeps its 45 mm width and 5 mm margin inside the
    // shared reach, and a free run at least 45 mm deep from it.
    for (i, f) in [p1[0], p2[0]].iter().enumerate() {
        let s = tower_spot(&m, room, &[*f]).unwrap();
        let r = [s[0] - 5.0, s[1] - 5.0, s[0] + 50.0, s[1] + 50.0];
        assert!(r[0] >= 25.0 && r[2] <= 325.0 && r[1] >= 0.0 && r[3] <= 320.0, "plate {}: {r:?}", i + 1);
        assert!(r[2] <= f[0] || r[0] >= f[2] || r[3] <= f[1] || r[1] >= f[3], "plate {}: {r:?} meets {f:?}", i + 1);
    }
    // The settings write the layout's spots; a plate without one (it prints
    // a single filament, so no tower) gets the template's saved spot as a
    // placeholder.
    let palette: Vec<Filament> = ["#000000", "#FFFFFF"].iter().map(|c| filament(c)).collect();
    let uses = [PlateUse { tower: Some([275.0, 5.0]) }, PlateUse { tower: Some([30.0, 5.0]) }, PlateUse { tower: None }];
    let out = project_settings(Some((&t, "t")), &m, &palette, &uses);
    let ps = parse(&out.json);
    assert_eq!(texts(&ps["wipe_tower_x"]), ["275", "30", "297.425"]);
    assert_eq!(texts(&ps["wipe_tower_y"]), ["5", "5", "137.09"]);
    assert!(!out.warnings.iter().any(|w| w.contains("prime tower")), "the layout says, not the settings: {:?}", out.warnings);
}

/// Every column beside a piece is tried, so a gap between two
/// pieces, or the strip beside one piece of a pair, takes the tower when
/// the shared reach's edges and middle do not.
#[test]
fn a_prime_tower_is_found_between_and_beside_the_pieces() {
    let m = Template::from_parts("BambuStudio-02.04.00.70", &Value::Object(two_nozzle_settings(2, &["#000000", "#FFFFFF"])).to_string())
        .unwrap()
        .machine();
    let room = TowerRoom::for_width(70.0);
    assert_eq!(room.span(), 80.0);
    // Two pieces at the reach's two edges with a 90 mm gap between them,
    // off the middle: the edges are taken, the middle column (x 135..215
    // with its margins) meets the left piece, and the column beside it --
    // x 175, its margin touching the left piece -- wins.
    let two = [[25.0, 0.0, 170.0, 320.0], [260.0, 0.0, 325.0, 320.0]];
    let s = tower_spot(&m, room, &two).expect("the gap between them");
    assert_eq!(s, [175.0, 5.0]);
    for o in &two {
        assert!(s[0] - 5.0 >= o[2] - 1e-9 || s[0] + 75.0 <= o[0] + 1e-9, "{s:?} meets {o:?}");
    }
    // A tower turned off by the template has no room at all.
    let mut off = two_nozzle_settings(2, &["#000000", "#FFFFFF"]);
    off.insert("enable_prime_tower".into(), json!("0"));
    let t = Template::from_parts("BambuStudio-02.04.00.70", &Value::Object(off).to_string()).unwrap();
    assert_eq!(tower_room(Some(&t)), None);
    // Cadpole's built-in settings leave the slicer's default tower on.
    assert_eq!(tower_room(None), Some(TowerRoom::for_width(TOWER_WIDTH_DEFAULT)));
}

#[test]
fn a_prime_tower_keeps_off_the_corner_the_machine_cannot_print() {
    // An X1-shaped bed: 256 square, the 18 x 28 cutter corner at the front
    // left. Pieces across the middle leave the left and right columns; the
    // right column is clear front to back and wins over the left, whose run
    // starts behind the corner.
    let m = Machine { area: [0.0, 0.0, 256.0, 256.0], height: 250.0, exclude: vec![[0.0, 0.0, 18.0, 28.0]], ..Machine::default() };
    let room = TowerRoom::for_width(TOWER_WIDTH_DEFAULT);
    assert_eq!(tower_spot(&m, room, &[[60.0, 40.0, 196.0, 216.0]]), Some([206.0, 5.0]));
    // Pieces over the right and the middle: the left column, behind the corner.
    assert_eq!(tower_spot(&m, room, &[[60.0, 0.0, 256.0, 256.0]]), Some([5.0, 38.0]), "28 + 5 behind the corner, +5 margin");
}
