//! Bambu Studio project settings: where a project's
//! `Metadata/project_settings.config` comes from, and the machine its plates
//! are laid out for.
//!
//! Learned, like the project format, from what the slicers *do* with files — the
//! official Bambu Studio 2.8.2.61 and OrcaSlicer 2.4.2 command-line slicers,
//! run as binaries on files built for the purpose — and from the bytes of
//! projects Bambu Studio saved. **No slicer source and no file bundled with a
//! slicer was read**, and no Bambu profile or saved project is shipped: what
//! Cadpole writes on its own is a handful of keys whose values are facts about a
//! machine (its bed, its nozzles, the names of its stock presets) and about
//! the model (its filament colours).
//!
//! # Two sources
//!
//! * **A template** ([`Template`]): a project the user saved in Bambu Studio
//!   for their printer, which carries every printer, process and filament
//!   setting they print with — start G-code, speeds, temperatures, flush
//!   volumes. Cadpole keeps all of it and maps the model's filament slots onto
//!   it ([`project_settings`]): the per-filament lists are cut or extended
//!   to the model's slot count, the colours become the model's, and the
//!   three "which filament prints walls and infill" keys become `"0"`, the
//!   part's own. This is the route to a project that prints as the user's
//!   own would.
//! * **Cadpole's built-in file**, when there is no template: the machine's bed,
//!   height, nozzles and the names of its stock printer, process and
//!   filament presets, and per filament its colour and the few properties
//!   the slicers index by filament. Both slicers open it as a project with
//!   its plates, objects, parts and filaments, and slice it — but a slicer
//!   fills every setting the file leaves out with **its own defaults, not the
//!   named preset's** (the Bambu Studio CLI does so: 60 mm/s
//!   walls, a 200 °C nozzle, a two-line start G-code, no filament-change
//!   G-code). So the built-in file is a project to open and check, and
//!   [`project_settings`] says so in a warning; a template is the project
//!   to print.
//!
//! # Why a plain three-key file is not enough
//!
//! A file of `from`, `name` and `filament_colour` and nothing else is one
//! Bambu Studio 2.8.2's CLI segfaults on, as it does on a project with no
//! settings file at all. The keys it needs are the 15 of a
//! saved project's 481 keys without which it crashes or
//! misassigns filaments: the printer and process preset names, the bed and
//! height, the nozzle diameter, and five per-filament lists it indexes by
//! filament number (`filament_diameter`, `filament_ids`,
//! `filament_is_support`, `filament_shrink`, `filament_start_gcode`) — a list
//! left at its default single entry is out of range for filament 2.
//!
//! # The version a project claims
//!
//! The root model's `Application` metadata is what makes both slicers open a
//! file as a project. OrcaSlicer 2.4.2 refuses `BambuStudio-02.05.00.00` and
//! everything newer ("Unsupported 3MF version"), and accepts
//! `BambuStudio-02.04.99.99`, so a project claims at most
//! [`APPLICATION`], a released Bambu Studio version, whatever its template's
//! own. Bambu Studio 2.8.2 slices a 2.6 template's settings under that claim
//! with the same filaments and the same grams.

use serde_json::{Map, Value};

use crate::Filament;

/// The `Application` Cadpole's projects claim: a released Bambu Studio, the
/// newest OrcaSlicer 2.4.2 accepts (it refuses 02.05.00.00 and later). A
/// template's own version is kept when it is older.
pub const APPLICATION: &str = "BambuStudio-02.04.00.70";
/// The newest Bambu Studio major.minor OrcaSlicer 2.4.2 opens.
pub const NEWEST_FOR_ORCA: (u32, u32) = (2, 4);

/// Settings lists that hold one block per filament but whose names do not
/// start with `filament_` (every one that does, bar [`NOT_PER_FILAMENT`], is
/// per filament). Learned from the saved projects: across files with one to
/// six filaments, each of these lists' lengths moved with the filament count.
const PER_FILAMENT: &[&str] = &[
    "activate_air_filtration",
    "additional_cooling_fan_speed",
    "additional_fan_full_speed_layer",
    "chamber_temperatures",
    "circle_compensation_speed",
    "close_additional_fan_first_x_layers",
    "close_fan_the_first_x_layers",
    "complete_print_exhaust_fan_speed",
    "cool_plate_temp",
    "cool_plate_temp_initial_layer",
    "cooling_perimeter_transition_distance",
    "cooling_slowdown_logic",
    "counter_coef_1",
    "counter_coef_2",
    "counter_coef_3",
    "counter_limit_max",
    "counter_limit_min",
    "default_filament_colour",
    "diameter_limit",
    "during_print_exhaust_fan_speed",
    "enable_overhang_bridge_fan",
    "enable_pressure_advance",
    "eng_plate_temp",
    "eng_plate_temp_initial_layer",
    "fan_cooling_layer_time",
    "fan_max_speed",
    "fan_min_speed",
    "first_x_layer_fan_speed",
    "first_x_layer_part_fan_speed",
    "flush_volumes_vector",
    "full_fan_speed_layer",
    "hole_coef_1",
    "hole_coef_2",
    "hole_coef_3",
    "hole_limit_max",
    "hole_limit_min",
    "hot_plate_temp",
    "hot_plate_temp_initial_layer",
    "impact_strength_z",
    "ironing_fan_speed",
    "long_retractions_when_ec",
    "no_slow_down_for_cooling_on_outwalls",
    "nozzle_temperature",
    "nozzle_temperature_initial_layer",
    "nozzle_temperature_range_high",
    "nozzle_temperature_range_low",
    "overhang_fan_speed",
    "overhang_fan_threshold",
    "overhang_threshold_participating_cooling",
    "override_process_overhang_speed",
    "pre_start_fan_time",
    "pressure_advance",
    "reduce_fan_stop_start_freq",
    "required_nozzle_HRC",
    "retraction_distances_when_ec",
    "slow_down_for_layer_cooling",
    "slow_down_layer_time",
    "slow_down_min_speed",
    "supertack_plate_temp",
    "supertack_plate_temp_initial_layer",
    "temperature_vitrification",
    "textured_plate_temp",
    "textured_plate_temp_initial_layer",
    "volumetric_speed_coefficients",
];

/// `filament_`-named lists whose length did *not* move with the filament
/// count in the saved projects.
const NOT_PER_FILAMENT: &[&str] = &[
    "filament_is_mixed",
    "filament_mixed_components",
    "filament_mixed_gradient",
    "filament_mixed_gradient_range",
    "filament_mixed_sublayer_ratios",
];

/// Lists of `[process, filament 1 .. filament N, printer]`.
const PER_PRESET: &[&str] = &["different_settings_to_system", "inherits_group"];

/// Two values Bambu Studio 2.x writes as `-1` and OrcaSlicer 2.4.2 refuses
/// ("Invalid parameter value(s) included in the 3mf file"; either one alone
/// is enough), with the value Bambu Studio itself wrote for each before it
/// adopted `-1` (every 1.x and early 2.x save has `2` and `0`). Both only
/// matter to a raft or to tree supports; Bambu Studio 2.8.2 slices an
/// unsupported kit identically either way.
const BAMBU_ONLY_VALUES: &[(&str, &str)] = &[("raft_first_layer_expansion", "2"), ("tree_support_wall_count", "0")];

/// The three "which filament prints this feature" keys: `"0"` is the part's
/// own filament. A project saved with `"1"` prints every part's walls or
/// infill in filament 1, whatever its part says.
const FEATURE_FILAMENTS: &[&str] = &["wall_filament", "sparse_infill_filament", "solid_infill_filament"];

// ---------------------------------------------------------------------------
// The machine
// ---------------------------------------------------------------------------

/// What one nozzle reaches: a rectangle of the plate and a height. A
/// two-nozzle machine's nozzles each reach less than the bed (the H2D's
/// left X 0–325, right X 25–350), and a part printed by both must lie where
/// both reach.
#[derive(Clone, Debug, PartialEq)]
pub struct Reach {
    /// `[x0, y0, x1, y1]` in mm, plate coordinates.
    pub area: [f64; 4],
    /// Printable height in mm.
    pub height: f64,
    /// Hotends this nozzle position swaps between (the H2C's right side
    /// holds six); 1 otherwise.
    pub hotends: u32,
}

/// The names of a machine's stock Bambu Studio presets, which Cadpole's
/// built-in settings name so that the slicer selects them.
#[derive(Clone, Debug, PartialEq)]
pub struct Presets {
    /// `printer_model`, e.g. "Bambu Lab H2D".
    pub model: String,
    /// `printer_settings_id`, e.g. "Bambu Lab H2D 0.4 nozzle".
    pub printer: String,
    /// `print_settings_id`, e.g. "0.20mm Standard @BBL H2D".
    pub process: String,
    /// `filament_settings_id` for every slot, e.g. "Bambu PLA Basic @BBL H2D".
    pub filament: String,
    /// That filament's `filament_ids` code, e.g. "GFA00".
    pub filament_id: String,
}

/// The machine a project's plates are laid out for: its bed, its height, the
/// corner it cannot print, what each nozzle reaches, and its stock presets.
#[derive(Clone, Debug, PartialEq)]
pub struct Machine {
    /// How messages name it ("Bambu Lab H2D").
    pub name: String,
    /// The printable area `[x0, y0, x1, y1]` in mm, plate coordinates
    /// (Bambu Studio's `printable_area`; every Bambu bed starts at 0, 0).
    pub area: [f64; 4],
    /// Printable height in mm (`printable_height`).
    pub height: f64,
    /// Rectangles of the area nothing may stand in (`bed_exclude_area`: the
    /// X1 and P1 series' 18 x 28 mm front-left corner).
    pub exclude: Vec<[f64; 4]>,
    /// Per nozzle, what it reaches. Empty, or one entry, for a machine whose
    /// one nozzle reaches the whole area.
    pub nozzles: Vec<Reach>,
    /// Nozzle diameter in mm.
    pub nozzle_diameter: f64,
    /// The stock presets the built-in settings name; `None` writes none.
    pub presets: Option<Presets>,
}

impl Default for Machine {
    /// A 256 mm square bed, 256 mm tall, one 0.4 mm nozzle, no presets: the
    /// size plates are laid out for by default. The engine passes a real machine.
    fn default() -> Machine {
        Machine {
            name: "a 256 mm bed".to_owned(),
            area: [0.0, 0.0, 256.0, 256.0],
            height: 256.0,
            exclude: Vec::new(),
            nozzles: Vec::new(),
            nozzle_diameter: 0.4,
            presets: None,
        }
    }
}

impl Machine {
    /// The bed's size `[w, d]`: what the plate grid's pitch is made of.
    pub fn bed(&self) -> [f64; 2] {
        [self.area[2] - self.area[0], self.area[3] - self.area[1]]
    }

    /// The part of the bed every nozzle reaches — for one nozzle, the bed.
    /// A plate centred here prints whichever nozzle the slicer gives each of
    /// its filaments.
    pub fn shared(&self) -> [f64; 4] {
        let mut r = self.area;
        for n in &self.nozzles {
            r = [r[0].max(n.area[0]), r[1].max(n.area[1]), r[2].min(n.area[2]), r[3].min(n.area[3])];
        }
        r
    }

    /// The lowest height every nozzle reaches.
    pub fn shared_height(&self) -> f64 {
        self.nozzles.iter().map(|n| n.height).fold(self.height, f64::min)
    }

    /// Hotends in all: a filament per hotend prints without a purge between
    /// them (the H2C's seven). 1 for a single nozzle.
    pub fn hotends(&self) -> u32 {
        if self.nozzles.is_empty() { 1 } else { self.nozzles.iter().map(|n| n.hotends.max(1)).sum() }
    }

    /// The machine a saved project describes: its `printable_area`,
    /// `printable_height`, `bed_exclude_area`, per-nozzle
    /// `extruder_printable_area` / `extruder_printable_height` /
    /// `extruder_max_nozzle_count`, nozzle diameter and preset names.
    pub fn from_settings(ps: &Map<String, Value>) -> Result<Machine, String> {
        let area = polygon_box(&strings(ps.get("printable_area")))
            .ok_or("its project_settings.config has no printable_area")?;
        let height = number(ps.get("printable_height")).unwrap_or(area[3] - area[1]);
        let exclude: Vec<[f64; 4]> = polygon_box(&strings(ps.get("bed_exclude_area"))).into_iter().collect();
        let reach_areas = strings(ps.get("extruder_printable_area"));
        let reach_heights = strings(ps.get("extruder_printable_height"));
        let hotends = strings(ps.get("extruder_max_nozzle_count"));
        let nozzles: Vec<Reach> = if reach_areas.len() > 1 {
            reach_areas
                .iter()
                .enumerate()
                .map(|(i, a)| {
                    let corners: Vec<String> = a.split(',').map(str::to_owned).collect();
                    Ok(Reach {
                        area: polygon_box(&corners).ok_or_else(|| format!("extruder_printable_area {a:?} is not a polygon"))?,
                        height: reach_heights.get(i).and_then(|h| h.trim().parse().ok()).unwrap_or(height),
                        hotends: hotends.get(i).and_then(|h| h.trim().parse().ok()).unwrap_or(1),
                    })
                })
                .collect::<Result<_, String>>()?
        } else {
            Vec::new()
        };
        let nozzle_diameter = strings(ps.get("nozzle_diameter")).first().and_then(|d| d.parse().ok()).unwrap_or(0.4);
        let text = |k: &str| ps.get(k).and_then(Value::as_str).unwrap_or("").to_owned();
        let model = text("printer_model");
        Ok(Machine {
            name: if model.is_empty() { text("printer_settings_id") } else { model.clone() },
            area,
            height,
            exclude,
            nozzles,
            nozzle_diameter,
            presets: None,
        })
    }
}

/// `["0x0", "350x0", "350x320", "0x320"]` → `[x0, y0, x1, y1]`.
fn polygon_box(corners: &[String]) -> Option<[f64; 4]> {
    let mut b = [f64::MAX, f64::MAX, f64::MIN, f64::MIN];
    for c in corners {
        let (x, y) = c.trim().split_once('x')?;
        let (x, y): (f64, f64) = (x.trim().parse().ok()?, y.trim().parse().ok()?);
        b = [b[0].min(x), b[1].min(y), b[2].max(x), b[3].max(y)];
    }
    (b[0] < b[2] && b[1] < b[3]).then_some(b)
}

/// A list of strings, or a single string as one; anything else is empty.
fn strings(v: Option<&Value>) -> Vec<String> {
    match v {
        Some(Value::Array(a)) => a.iter().filter_map(|x| x.as_str().map(str::to_owned)).collect(),
        Some(Value::String(s)) => vec![s.clone()],
        _ => Vec::new(),
    }
}

/// A number stored, as Bambu stores every number, as a string.
fn number(v: Option<&Value>) -> Option<f64> {
    match v {
        Some(Value::String(s)) => s.trim().parse().ok(),
        Some(Value::Number(n)) => n.as_f64(),
        Some(Value::Array(a)) => number(a.first()),
        _ => None,
    }
}

/// A coordinate as Bambu writes one: `350`, `25.5`.
fn coord(v: f64) -> String {
    crate::fmt_item_number(v)
}

fn corners(b: [f64; 4]) -> [String; 4] {
    [
        format!("{}x{}", coord(b[0]), coord(b[1])),
        format!("{}x{}", coord(b[2]), coord(b[1])),
        format!("{}x{}", coord(b[2]), coord(b[3])),
        format!("{}x{}", coord(b[0]), coord(b[3])),
    ]
}

// ---------------------------------------------------------------------------
// The template
// ---------------------------------------------------------------------------

/// A project the user saved in Bambu Studio, as `export(template=)` reads it:
/// its `Application` and its whole `project_settings.config`.
#[derive(Clone, Debug, PartialEq)]
pub struct Template {
    /// The root model's `Application`, e.g. "BambuStudio-02.06.00.51".
    pub application: String,
    pub settings: Map<String, Value>,
}

/// What a template may be, as a zip read under caps: a saved project is one
/// entry per object plus a few images per plate; its settings are about
/// 70 KB. Only the two parts read are inflated.
const TEMPLATE_CAPS: crate::zip::Caps =
    crate::zip::Caps { max_entries: 4096, max_inflated: 512 * 1024 * 1024, max_ratio: 200 };

impl Template {
    /// Read a saved project: its `Metadata/project_settings.config` and the
    /// `Application` of its root model, nothing else. Refused, saying why:
    /// a file that is not a zip, a 3MF with no settings (not a project), or a
    /// project another application saved.
    pub fn from_3mf(bytes: &[u8]) -> Result<Template, String> {
        let (application, settings) = template_parts(bytes)?;
        Template::from_parts(&application, &settings)
    }

    /// A template from its two parts: the `Application` string and the
    /// settings JSON.
    pub fn from_parts(application: &str, settings: &str) -> Result<Template, String> {
        if !application.starts_with("BambuStudio-") {
            return Err(format!(
                "it was saved by {:?}, not Bambu Studio; save the template in Bambu Studio",
                if application.is_empty() { "an application that does not say" } else { application }
            ));
        }
        let v: Value = serde_json::from_str(settings).map_err(|e| format!("its project_settings.config is not JSON: {e}"))?;
        let Value::Object(settings) = v else {
            return Err("its project_settings.config is not a JSON object".to_owned());
        };
        let t = Template { application: application.to_owned(), settings };
        if t.filaments() == 0 {
            return Err("its project_settings.config lists no filament (filament_colour)".to_owned());
        }
        Machine::from_settings(&t.settings)?;
        Ok(t)
    }

    /// How many filaments the template holds.
    pub fn filaments(&self) -> usize {
        strings(self.settings.get("filament_colour")).len()
    }

    /// The machine the template was saved for.
    pub fn machine(&self) -> Machine {
        Machine::from_settings(&self.settings).expect("checked when the template was read")
    }

    /// `printer_model`, e.g. "Bambu Lab H2C".
    pub fn printer_model(&self) -> &str {
        self.settings.get("printer_model").and_then(Value::as_str).unwrap_or("")
    }

    /// `printer_settings_id`, e.g. "Bambu Lab H2C 0.4 nozzle".
    pub fn printer_preset(&self) -> &str {
        self.settings.get("printer_settings_id").and_then(Value::as_str).unwrap_or("")
    }
}

/// The two parts of a saved project a template is made of, as text: its
/// root model's `Application` and its `project_settings.config`. Only those
/// two entries are inflated. [`Template::from_parts`] checks them.
pub fn template_parts(bytes: &[u8]) -> Result<(String, String), String> {
    let want = |n: &str| n == crate::MODEL_PART || n == crate::BAMBU_PROJECT_CONFIG;
    let z = crate::zip::read_some(bytes, &TEMPLATE_CAPS, &want).map_err(|e| format!("not a 3MF package: {e}"))?;
    let settings = z.text(crate::BAMBU_PROJECT_CONFIG).ok_or(
        "it has no Metadata/project_settings.config: it is a model, not a Bambu Studio project \
         (save one in Bambu Studio with your printer, process and filaments selected)",
    )?;
    let root = z.text(crate::MODEL_PART).ok_or("it has no 3D/3dmodel.model")?;
    Ok((application_of(&root), settings))
}

/// The text of `<metadata name="Application">` in a root model, or "".
pub fn application_of(root_model: &str) -> String {
    let Ok(doc) = roxmltree::Document::parse(root_model) else { return String::new() };
    doc.root_element()
        .children()
        .find(|n| n.has_tag_name("metadata") && n.attribute("name") == Some("Application"))
        .and_then(|n| n.text())
        .unwrap_or("")
        .trim()
        .to_owned()
}

/// `BambuStudio-02.06.00.51` → `[2, 6, 0, 51]`.
pub fn version_of(application: &str) -> Option<[u32; 4]> {
    let v = application.strip_prefix("BambuStudio-")?;
    let parts: Vec<u32> = v.split('.').map(|p| p.trim().parse().ok()).collect::<Option<_>>()?;
    (parts.len() == 4).then(|| [parts[0], parts[1], parts[2], parts[3]])
}

/// The `Application` a project with this template claims: the template's
/// own, unless OrcaSlicer would refuse it, then [`APPLICATION`].
pub fn application_for(template: Option<&Template>) -> String {
    match template.and_then(|t| version_of(&t.application).map(|v| (v, &t.application))) {
        Some((v, app)) if (v[0], v[1]) <= NEWEST_FOR_ORCA => app.clone(),
        _ => APPLICATION.to_owned(),
    }
}

// ---------------------------------------------------------------------------
// The settings
// ---------------------------------------------------------------------------

/// The width, across x, Cadpole keeps clear for a prime tower, at least: a
/// slicer's tower is wider than its `prime_tower_width` says (OrcaSlicer
/// 2.4.2 refused an H2D tower of width 15 at x = 297, past the left nozzle's
/// 325, and took it at 280: its tower is 28 to 45 mm across). Its depth
/// grows towards +y from `wipe_tower_y` the more it purges -- a 20 mm tower
/// purging seven filaments at 0.12 mm layers ran past the H2C's 320 mm from
/// y = 250 and fitted from y = 200 (Bambu Studio 2.8.2) -- so a
/// tower starts at the front of the longest free run of the plate.
pub const TOWER_SIDE: f64 = 45.0;
/// Clearance kept round a prime tower: its brim, and a gap to the pieces.
pub const TOWER_MARGIN: f64 = 5.0;
/// `prime_tower_width` when a template does not say, and for Cadpole's
/// built-in settings: the value Bambu Studio writes by default.
pub const TOWER_WIDTH_DEFAULT: f64 = 35.0;

/// What a plate's prime tower needs on the bed: a square
/// `side` mm across -- the template's `prime_tower_width`, at least
/// [`TOWER_SIDE`] -- and `margin` clear all round it, inside what every
/// nozzle reaches. The square is the least depth Cadpole accepts; the tower
/// grows towards +y from its front the more it purges, so of the spots that
/// have room the one with the longest free run behind it is taken.
#[derive(Clone, Copy, Debug, PartialEq)]
pub struct TowerRoom {
    pub side: f64,
    pub margin: f64,
}

impl TowerRoom {
    /// The room a tower of the slicer's `width` needs.
    pub fn for_width(width: f64) -> TowerRoom {
        TowerRoom { side: width.max(TOWER_SIDE), margin: TOWER_MARGIN }
    }

    /// Across x (and the least depth along y) one tower and its margins
    /// take: `side + 2 margin`.
    pub fn span(&self) -> f64 {
        self.side + 2.0 * self.margin
    }
}

/// The prime tower a project with `template` prints on a plate of more than
/// one filament: `None` when the template turns the prime tower off
/// (`enable_prime_tower` "0"), else the room a tower of its
/// `prime_tower_width` needs. Cadpole's built-in settings leave the slicer's
/// default on, a [`TOWER_WIDTH_DEFAULT`] tower.
pub fn tower_room(template: Option<&Template>) -> Option<TowerRoom> {
    let settings = template.map(|t| &t.settings);
    let text = |k: &str| settings.and_then(|s| s.get(k)).and_then(Value::as_str).map(str::trim).unwrap_or("");
    if text("enable_prime_tower") == "0" {
        return None;
    }
    let width = settings.and_then(|s| number(s.get("prime_tower_width"))).filter(|w| *w > 0.0).unwrap_or(TOWER_WIDTH_DEFAULT);
    Some(TowerRoom::for_width(width))
}

/// Where a prime tower of `room` can stand on a plate whose pieces cover
/// `obstacles` (rectangles `[x0, y0, x1, y1]` in plate coordinates, one per
/// object), or `None` when nowhere has room.
///
/// The spot is the tower's front-left corner. The tower and `room.margin`
/// round it lie inside what every nozzle reaches ([`Machine::shared`]),
/// clear of every obstacle and of the machine's excluded corners by the
/// margin, with a free run at least `room.side` deep along +y from its
/// front. Every column that can hold it is tried -- the shared reach's two
/// edges, its middle, and beside each obstacle's left and right sides, so a
/// gap between two pieces, a strip beside them, in front of them, behind
/// them and a corner are all found -- and the spot at the front of the
/// longest run wins; among equals, the right edge first, then the left,
/// then the middle, then the rest left to right. A spot a template saved is
/// never used: it was chosen for another model.
pub fn tower_spot(machine: &Machine, room: TowerRoom, obstacles: &[[f64; 4]]) -> Option<[f64; 2]> {
    let r = machine.shared();
    let (side, m) = (room.side, room.margin);
    let mut xs = vec![r[2] - m - side, r[0] + m, (r[0] + r[2] - side) * 0.5];
    let mut beside: Vec<f64> = obstacles.iter().chain(machine.exclude.iter()).flat_map(|o| [o[0] - m - side, o[2] + m]).collect();
    beside.sort_by(f64::total_cmp);
    xs.extend(beside);
    let mut best: Option<([f64; 2], f64)> = None;
    for x in xs {
        let (x0, x1) = (x - m, x + side + m);
        if x0 < r[0] - 1e-9 || x1 > r[2] + 1e-9 {
            continue;
        }
        let mut blocks: Vec<(f64, f64)> = obstacles
            .iter()
            .chain(machine.exclude.iter())
            .filter(|b| x0 < b[2] - 1e-9 && x1 > b[0] + 1e-9)
            .map(|b| (b[1] - m, b[3] + m))
            .collect();
        blocks.sort_by(|a, b| a.0.total_cmp(&b.0));
        let mut y = r[1];
        let mut runs = Vec::new();
        for (b0, b1) in blocks {
            if b0 > y {
                runs.push((y, b0));
            }
            y = y.max(b1);
        }
        if r[3] > y {
            runs.push((y, r[3]));
        }
        for (a, b) in runs {
            let depth = b - a - 2.0 * m;
            if depth >= side - 1e-9 && best.is_none_or(|(_, d)| depth > d + 1e-9) {
                best = Some(([x, a + m], depth));
            }
        }
    }
    best.map(|(spot, _)| spot)
}

/// One plate as the settings see it: where the layout put its prime tower
/// ([`crate::bambu::layout_plates`]), `None` for a plate that has
/// none -- empty, or printing one filament with no free spot.
#[derive(Clone, Copy, Debug, Default, PartialEq)]
pub struct PlateUse {
    pub tower: Option<[f64; 2]>,
}

/// `wipe_tower_x` / `wipe_tower_y` for every plate: the layout's spot. A
/// plate without one prints no tower (it is empty or prints one filament:
/// the layout refuses a plate of several filaments with no room), and gets
/// the template's saved spot, or the front middle of the shared reach, as a
/// placeholder the slicer does not use.
fn place_towers(ps: &mut Map<String, Value>, machine: &Machine, plates: &[PlateUse]) {
    let room = TowerRoom::for_width(number(ps.get("prime_tower_width")).unwrap_or(TOWER_WIDTH_DEFAULT));
    let xs = strings(ps.get("wipe_tower_x"));
    let ys = strings(ps.get("wipe_tower_y"));
    let saved: Vec<[f64; 2]> =
        xs.iter().zip(&ys).filter_map(|(x, y)| Some([x.trim().parse().ok()?, y.trim().parse().ok()?])).collect();
    let fallback = saved.first().copied().unwrap_or([
        (machine.shared()[0] + machine.shared()[2] - room.side) * 0.5,
        machine.shared()[1] + room.margin,
    ]);
    let mut x = Vec::with_capacity(plates.len());
    let mut y = Vec::with_capacity(plates.len());
    for p in plates {
        let at = p.tower.unwrap_or(fallback);
        x.push(Value::String(coord((at[0] * 1000.0).round() / 1000.0)));
        y.push(Value::String(coord((at[1] * 1000.0).round() / 1000.0)));
    }
    ps.insert("wipe_tower_x".into(), Value::Array(x));
    ps.insert("wipe_tower_y".into(), Value::Array(y));
}

/// A project's settings file and what the user should know about it.
pub struct ProjectSettings {
    /// `Metadata/project_settings.config`.
    pub json: String,
    /// The root model's `Application`.
    pub application: String,
    pub warnings: Vec<String>,
}

/// The settings of a project of `plates` (one [`PlateUse`] each, plate 1
/// first) printing `palette` (slot 1 first) on `machine`: the template's,
/// mapped onto the model's filaments, or Cadpole's built-in file, with the
/// layout's prime tower spot for every plate. A template comes with how
/// warnings name it.
pub fn project_settings(
    template: Option<(&Template, &str)>,
    machine: &Machine,
    palette: &[Filament],
    plates: &[PlateUse],
) -> ProjectSettings {
    let application = application_for(template.map(|(t, _)| t));
    let colours: Vec<String> = if palette.is_empty() {
        vec!["#BFBFBF".to_owned()]
    } else {
        palette.iter().map(Filament::hex).collect()
    };
    let mut warnings = Vec::new();
    let mut ps = match template {
        Some((t, name)) => from_template(t, name, &colours, &mut warnings),
        None => {
            warnings.push(format!(
                "no template=: the project carries Cadpole's minimal settings -- the {}'s bed, nozzles and preset \
                 names, and the filament colours. Bambu Studio and OrcaSlicer open it as a project, but a slicer \
                 fills what it leaves out (start G-code, speeds, temperatures) with its own defaults: pick your \
                 printer, process and filament presets before slicing, or pass template= a project you saved \
                 for this printer",
                machine.name
            ));
            built_in(machine, &colours, &application)
        }
    };
    let one = [PlateUse::default()];
    place_towers(&mut ps, machine, if plates.is_empty() { &one } else { plates });
    let json = serde_json::to_string_pretty(&Value::Object(ps)).expect("a JSON map serialises");
    ProjectSettings { json: json + "\n", application, warnings }
}

/// Cadpole's own settings file: the keys both slicers need to open the file as a
/// project and slice it, with values Cadpole knows.
fn built_in(machine: &Machine, colours: &[String], application: &str) -> Map<String, Value> {
    let n = colours.len();
    let s = |v: &str| Value::String(v.to_owned());
    let list = |v: &str| Value::Array(vec![Value::String(v.to_owned()); n]);
    let strs = |v: Vec<String>| Value::Array(v.into_iter().map(Value::String).collect());
    let mut ps = Map::new();
    ps.insert("from".into(), s("project"));
    ps.insert("name".into(), s("project_settings"));
    ps.insert("version".into(), s(application.strip_prefix("BambuStudio-").unwrap_or(application)));
    if let Some(p) = &machine.presets {
        ps.insert("printer_model".into(), s(&p.model));
        ps.insert("printer_settings_id".into(), s(&p.printer));
        ps.insert("print_settings_id".into(), s(&p.process));
        ps.insert("filament_settings_id".into(), list(&p.filament));
        ps.insert("filament_ids".into(), list(&p.filament_id));
    }
    ps.insert("printer_variant".into(), s(&coord(machine.nozzle_diameter)));
    // The one process value: OrcaSlicer 2.4.2's default outer-wall line
    // width gives a painted face a negative line spacing ("Flow::spacing()
    // produced negative spacing") and the slice fails; 1.05 x the nozzle is
    // the stock 0.20 mm processes' own (0.42 for a 0.4 mm nozzle).
    ps.insert(
        "outer_wall_line_width".into(),
        s(&coord((machine.nozzle_diameter * 1.05 * 100.0).round() / 100.0)),
    );
    ps.insert("printable_area".into(), strs(corners(machine.area).to_vec()));
    ps.insert("printable_height".into(), s(&coord(machine.height)));
    ps.insert(
        "bed_exclude_area".into(),
        strs(machine.exclude.first().map(|b| corners(*b).to_vec()).unwrap_or_default()),
    );
    let nozzles = machine.nozzles.len().max(1);
    ps.insert("nozzle_diameter".into(), Value::Array(vec![s(&coord(machine.nozzle_diameter)); nozzles]));
    if machine.nozzles.len() > 1 {
        ps.insert(
            "extruder_printable_area".into(),
            strs(machine.nozzles.iter().map(|r| corners(r.area).join(",")).collect()),
        );
        ps.insert("extruder_printable_height".into(), strs(machine.nozzles.iter().map(|r| coord(r.height)).collect()));
        ps.insert(
            "extruder_max_nozzle_count".into(),
            strs(machine.nozzles.iter().map(|r| r.hotends.max(1).to_string()).collect()),
        );
    }
    ps.insert("filament_colour".into(), strs(colours.to_vec()));
    ps.insert("filament_type".into(), list("PLA"));
    ps.insert("filament_diameter".into(), list("1.75"));
    // The density of PLA as Bambu's own PLA presets give it, so a slice
    // estimates grams rather than zero.
    ps.insert("filament_density".into(), list("1.26"));
    ps.insert("filament_is_support".into(), list("0"));
    ps.insert("filament_shrink".into(), list("100%"));
    ps.insert("filament_start_gcode".into(), list(""));
    for k in FEATURE_FILAMENTS {
        ps.insert((*k).into(), s("0"));
    }
    ps
}

/// Is the list `key` one block per filament?
fn per_filament(key: &str) -> bool {
    (key.starts_with("filament_") && !NOT_PER_FILAMENT.contains(&key)) || PER_FILAMENT.contains(&key)
}

/// The template's settings with the model's filaments mapped onto them.
fn from_template(t: &Template, name: &str, colours: &[String], warnings: &mut Vec<String>) -> Map<String, Value> {
    let mut ps = t.settings.clone();
    let had: Vec<String> = strings(ps.get("filament_colour"));
    let (nt, n) = (had.len(), colours.len());
    if n > nt {
        let preset = strings(ps.get("filament_settings_id")).first().cloned().unwrap_or_default();
        warnings.push(format!(
            "template {name:?} holds {nt} filament{} and the model uses {n}: filament{} {} take{} filament 1's \
             preset ({preset:?}) in the model's colours; save the template with {n} filaments to choose each",
            if nt == 1 { "" } else { "s" },
            if n - nt == 1 { "" } else { "s" },
            if n - nt == 1 { format!("{n}") } else { format!("{}-{n}", nt + 1) },
            if n - nt == 1 { "s" } else { "" },
        ));
    }
    let mut skipped = Vec::new();
    for (key, value) in ps.iter_mut() {
        let Value::Array(list) = value else { continue };
        if key == "flush_volumes_matrix" {
            if let Some(m) = flush_matrix(list, &had, colours) {
                *list = m;
            } else if n != nt {
                skipped.push(key.clone());
            }
            continue;
        }
        if PER_PRESET.contains(&key.as_str()) {
            if list.len() == nt + 2 {
                let mut out = vec![list[0].clone()];
                out.extend((0..n).map(|i| list[1 + if i < nt { i } else { 0 }].clone()));
                out.push(list[nt + 1].clone());
                *list = out;
            }
            continue;
        }
        if !per_filament(key) || n == nt {
            continue;
        }
        if list.is_empty() || list.len() % nt != 0 {
            skipped.push(key.clone());
            continue;
        }
        let block = list.len() / nt;
        let mut out = Vec::with_capacity(block * n);
        for i in 0..n {
            let from = if i < nt { i } else { 0 };
            out.extend_from_slice(&list[from * block..(from + 1) * block]);
        }
        if key == "filament_self_index" {
            out = (0..n).flat_map(|i| std::iter::repeat(Value::String((i + 1).to_string())).take(block)).collect();
        }
        *list = out;
    }
    if !skipped.is_empty() {
        warnings.push(format!(
            "template {name:?}: {} could not be fitted to {n} filaments and {} left as saved",
            skipped.join(", "),
            if skipped.len() == 1 { "is" } else { "are" }
        ));
    }
    ps.insert("filament_colour".into(), Value::Array(colours.iter().cloned().map(Value::String).collect()));
    for k in FEATURE_FILAMENTS {
        if ps.contains_key(*k) {
            ps.insert((*k).into(), Value::String("0".into()));
        }
    }
    let mut replaced = Vec::new();
    for (k, older) in BAMBU_ONLY_VALUES {
        if ps.get(*k).and_then(Value::as_str) == Some("-1") {
            ps.insert((*k).into(), Value::String((*older).into()));
            replaced.push(format!("{k} -1 -> {older}"));
        }
    }
    let text = |k: &str| ps.get(k).and_then(Value::as_str).unwrap_or("").to_owned();
    let tree = text("enable_support") == "1" && text("support_type").starts_with("tree");
    let raft = text("raft_layers").parse::<u32>().unwrap_or(0) > 0;
    if !replaced.is_empty() && (tree || raft) {
        warnings.push(format!(
            "template {name:?} prints with {}: {} so OrcaSlicer opens the project (it refuses -1); check \
             them in Bambu Studio's support settings",
            if raft { "a raft" } else { "tree supports" },
            replaced.join(", ")
        ));
    }
    ps
}

/// The flush matrix — per nozzle, `n x n` purge volumes in mm³ from one
/// filament to the next — for the model's colours. A pair whose two colours
/// are the template's own keeps the template's volume; any other pair gets
/// the template's largest, so no change of colour purges less than the user's
/// worst case. `None` when the list is not whole `nt x nt` blocks.
fn flush_matrix(list: &[Value], had: &[String], colours: &[String]) -> Option<Vec<Value>> {
    let (nt, n) = (had.len(), colours.len());
    if nt == 0 || list.is_empty() || list.len() % (nt * nt) != 0 {
        return None;
    }
    let kept = |i: usize| i < nt && had[i].eq_ignore_ascii_case(&colours[i]);
    let blocks = list.len() / (nt * nt);
    let mut out = Vec::with_capacity(blocks * n * n);
    for b in 0..blocks {
        let block = &list[b * nt * nt..(b + 1) * nt * nt];
        let value = |v: &Value| v.as_str().and_then(|s| s.trim().parse::<f64>().ok()).unwrap_or(0.0);
        let worst = (0..nt * nt).filter(|k| k / nt != k % nt).map(|k| value(&block[k])).fold(0.0, f64::max);
        for i in 0..n {
            for j in 0..n {
                out.push(if i == j {
                    Value::String("0".into())
                } else if kept(i) && kept(j) {
                    block[i * nt + j].clone()
                } else {
                    Value::String(coord(worst))
                });
            }
        }
    }
    Some(out)
}

#[cfg(test)]
#[path = "bambu_settings_tests.rs"]
mod tests;
