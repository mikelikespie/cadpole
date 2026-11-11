//! Bambu Studio project files: the `flavor.bambu` 3MF.
//!
//! Reverse-engineered from the *file format* — the 3MF Core and
//! Production-extension specifications and the bytes of project files
//! Bambu Studio 01.08 through 02.06 wrote — and from what the official Bambu Studio and OrcaSlicer
//! command-line slicers do with files built for the purpose.
//! **No slicer source, and no file bundled with a slicer, was read**:
//! Bambu Studio, OrcaSlicer and PrusaSlicer are AGPL and Cadpole is Apache-2.0.
//!
//! # What a project is
//!
//! A standard 3MF with the Production extension's layout on top of it:
//!
//! | part | what |
//! | --- | --- |
//! | `3D/3dmodel.model` | the root model: `Application` = `BambuStudio-…`, which is what makes a slicer open the file *as a project*; one `<object>` per print object, each a `<components>` list pointing into an object file; one `<item>` per object placing it on its plate |
//! | `3D/_rels/3dmodel.model.rels` | a relationship to every object file, as the Production extension requires |
//! | `3D/Objects/object_N.model` | one model per print object holding its parts' meshes, each mesh **centred on its own bounding box**, painted triangles carrying `paint_color`; an empty `<build/>` |
//! | `Metadata/model_settings.config` | Bambu's own XML: object and part names, the filament (`extruder`) of each part, the **plates** and which object sits on which, an assembly view |
//! | `Metadata/project_settings.config` | Bambu's JSON settings: a template's, mapped onto the model's filaments, or Cadpole's built-in file ([`crate::bambu_settings`]) |
//! | `Metadata/slice_info.config` | the header Bambu writes for an unsliced project |
//! | `Metadata/plate_N.png`, `plate_N_small.png` | the plate thumbnails, 512 and 128 px, referenced from `_rels/.rels` as the package thumbnail |
//!
//! # The plate grid
//!
//! Bambu Studio lays its plates out in world space, `ceil(√N)` plates to a
//! row for a project of N plates, each plate `1.2 ×` the bed's size from the
//! next, rows towards −y. Every multi-plate project Bambu Studio writes follows
//! that rule; a piece outside its plate's cell fails Bambu's slice ("One of
//! the plate is empty or has no object fully inside it"). A build
//! item's translation is *plate origin + the object's bounding-box centre*,
//! with z the object's half-height so it stands on the bed. [`plate_origin`]
//! is the grid.
//!
//! **Where on the plate.** A Cadpole model is built around its own origin and a
//! script has no bed to place it on — a house's storeys stand at their own
//! heights, the toy's core is centred on (0, 0) — where a bed's origin is its
//! corner. So a plate's contents are moved *as a group* so that their combined
//! footprint is centred where every nozzle reaches ([`Machine::shared`]; the
//! bed, for a one-nozzle machine), off any corner the machine cannot print,
//! and every object is set down on the bed (its lowest point at z = 0): every
//! body keeps its position relative to every other body of its object, and
//! the objects of a plate keep theirs in plan. A plate whose pieces cannot
//! fit is refused by [`layout_plates`], naming the plate. The assembly view
//! keeps the script's own coordinates.
//!
//! # Objects and parts
//!
//! A Bambu *object* is what the user moves and slices as one; its *parts*
//! are meshes with their own filament that cannot be separated. A
//! print-in-place mechanism is one object with a part per body. Each part's
//! mesh is written centred on its own bounding box, the `<component>`
//! transform carries its offset from the object's centre, and the object's
//! `<item>` carries the object's centre on the plate — exactly the shape the
//! projects Bambu Studio writes have for their multi-part objects.

use std::fmt::Write as _;

use cadpole_core::solid::SolidOptions;
use cadpole_core::tessellate::Mesh;

pub use crate::bambu_settings::{Machine, Presets, Reach, Template};
use crate::{
    append, as_mesh, esc, fmt_item_number, paint, zip, Filament, OutObject, Part, Written, BAMBU_MODEL_CONFIG, BAMBU_NS,
    BAMBU_PROJECT_CONFIG, BAMBU_SLICE_INFO, CONTENT_TYPES_PART, CORE_NS, MODEL_PART, MODEL_REL, RELS_PART,
};

/// The 3MF Production extension namespace (`p:`).
pub const PRODUCTION_NS: &str = "http://schemas.microsoft.com/3dmanufacturing/production/2015/06";
/// OPC's package-thumbnail relationship type (3MF core §"Thumbnails").
pub const THUMBNAIL_REL: &str = "http://schemas.openxmlformats.org/package/2006/relationships/metadata/thumbnail";
/// Bambu's own two cover relationships, as Bambu Studio writes them in `_rels/.rels`.
pub const COVER_MIDDLE_REL: &str = "http://schemas.bambulab.com/package/2021/cover-thumbnail-middle";
pub const COVER_SMALL_REL: &str = "http://schemas.bambulab.com/package/2021/cover-thumbnail-small";
/// The root model's relationships part, which lists every object file.
pub const MODEL_RELS_PART: &str = "3D/_rels/3dmodel.model.rels";
/// The `<build>` UUID Bambu Studio writes, verbatim.
pub const BUILD_UUID: &str = "2c7c17d8-22b5-4d84-8835-1976022ea369";
/// Plate pitch as a multiple of the bed size, on both axes.
pub const PLATE_STRIDE_FACTOR: f64 = 1.2;
/// The bed the grid is laid out for when nothing says otherwise: the X1C,
/// P1S and A1's 256 mm square, the commonest bed size.
pub const DEFAULT_BED: [f64; 2] = [256.0, 256.0];
/// How far apart, in mm, a plate's contents may lie outside what every
/// nozzle reaches before it is worth a warning: rounding, not geometry.
const REACH_SLACK: f64 = 1e-3;
/// The plate thumbnail sizes Bambu Studio writes.
pub const THUMBNAIL_PX: u32 = 512;
pub const THUMBNAIL_SMALL_PX: u32 = 128;

/// `3D/Objects/object_N.model` for print object `id`.
pub fn object_part(id: u32) -> String {
    format!("3D/Objects/object_{id}.model")
}
/// `Metadata/plate_N.png`.
pub fn plate_thumbnail_part(plate: u32) -> String {
    format!("Metadata/plate_{plate}.png")
}
/// `Metadata/plate_N_small.png`.
pub fn plate_thumbnail_small_part(plate: u32) -> String {
    format!("Metadata/plate_{plate}_small.png")
}

/// Plates per row of Bambu's world-space grid for a project of `plates`
/// plates: `ceil(√plates)`, at least 1. Bambu Studio lays out
/// projects so (2 plates: 2 to a row; 3 and 4: 2; 5 to 9: 3; 19: 5).
pub fn plate_columns(plates: u32) -> u32 {
    let mut c = 1;
    while c * c < plates {
        c += 1;
    }
    c
}

/// Where plate `plate` (1-based) of a project of `plates` plates starts in
/// Bambu's world space for a bed of `bed` mm: [`plate_columns`] to a row,
/// `1.2 × bed` apart, rows towards −y. In files Bambu Studio writes, a 256 mm
/// bed's nine plates centre at x = 128, 435.2, 742.4 and y = 128, −179.2,
/// −486.4; four plates at x = 128, 435.2 and y = 128, −179.2; a 180 mm bed's
/// plate 2 of 2 at x = 306 for an object centred on it (216 + 90); an H2D's
/// plates 5 and 6 of 6 inside (420, −384) and (840, −384).
pub fn plate_origin(plate: u32, plates: u32, bed: [f64; 2]) -> [f64; 2] {
    let cols = plate_columns(plates.max(plate));
    let i = plate.max(1) - 1;
    let (col, row) = (i % cols, i / cols);
    // `0.0 - y`, not `-y`: the first row is y = 0, never -0.
    [col as f64 * PLATE_STRIDE_FACTOR * bed[0], 0.0 - row as f64 * PLATE_STRIDE_FACTOR * bed[1]]
}

/// The translation that centres a footprint of `lo..hi` (world XY) on a
/// `bed`: what every object of a plate is moved by, together.
pub fn bed_shift(bed: [f64; 2], lo: [f64; 2], hi: [f64; 2]) -> [f64; 2] {
    region_shift([0.0, 0.0, bed[0], bed[1]], lo, hi)
}

/// The translation that centres a footprint of `lo..hi` on the rectangle
/// `region` (`[x0, y0, x1, y1]`, plate coordinates).
pub fn region_shift(region: [f64; 4], lo: [f64; 2], hi: [f64; 2]) -> [f64; 2] {
    [(region[0] + region[2]) * 0.5 - (lo[0] + hi[0]) * 0.5, (region[1] + region[3]) * 0.5 - (lo[1] + hi[1]) * 0.5]
}

/// One print object standing on a plate, for [`layout_plates`]: which plate,
/// its world-XY bounding box where the script put it, and its height once
/// set down on the bed.
#[derive(Clone, Copy, Debug, PartialEq)]
pub struct Footprint {
    /// 1-based plate number.
    pub plate: u32,
    pub lo: [f64; 2],
    pub hi: [f64; 2],
    /// Its height, bottom to top, in mm; 0 when unknown.
    pub height: f64,
}

/// Where one plate's contents go: the plate's corner on Bambu's grid and
/// the shift that centres its contents on the bed. [`Self::translation`]
/// is the whole of what a body on the plate is moved by.
#[derive(Clone, Copy, Debug, PartialEq)]
pub struct PlatePlacement {
    /// 1-based plate number.
    pub plate: u32,
    /// [`plate_origin`]: the bed's corner in world space.
    pub origin: [f64; 2],
    /// [`bed_shift`] of the plate's union footprint; zero for an empty plate.
    pub shift: [f64; 2],
    /// The front-left corner of the plate's prime tower in plate
    /// coordinates (`wipe_tower_x`, `wipe_tower_y`), clear of its pieces
    /// ([`crate::bambu_settings::tower_spot`]); `None` for an empty
    /// plate, a project that prints no tower, or a plate of one filament
    /// with no free spot (it prints no tower).
    pub tower: Option<[f64; 2]>,
}

/// What the layout needs to know about prime towers: the room one
/// takes on the bed -- `None` when the project prints none, its template
/// having turned the tower off -- and how many filaments each plate prints,
/// plate 1 first. A plate of more than one filament prints a tower, so the
/// layout finds it a spot clear of the pieces, moves the pieces aside to
/// make one, or refuses the plate.
#[derive(Clone, Debug, Default, PartialEq)]
pub struct TowerPlan {
    pub room: Option<crate::bambu_settings::TowerRoom>,
    pub filaments: Vec<usize>,
}

impl TowerPlan {
    /// No towers: a layout that only places pieces.
    pub fn none() -> TowerPlan {
        TowerPlan::default()
    }

    /// Does plate `plate` (1-based) print a prime tower?
    pub fn needs(&self, plate: u32) -> bool {
        self.room.is_some() && self.filaments.get(plate.max(1) as usize - 1).is_some_and(|n| *n > 1)
    }
}

impl PlatePlacement {
    /// The translation every body on this plate is moved by: the plate's
    /// origin plus its centring shift, z untouched (z = 0 is the bed in
    /// Cadpole as in Bambu).
    pub fn translation(&self) -> [f64; 3] {
        [self.origin[0] + self.shift[0], self.origin[1] + self.shift[1], 0.0]
    }
}

/// Something wrong with how a plate's pieces sit on the machine.
#[derive(Clone, Debug, PartialEq)]
pub struct PlateProblem {
    /// 1-based plate number.
    pub plate: u32,
    /// The pieces cannot be printed as laid out: the export refuses.
    pub fatal: bool,
    /// What, without the plate's name (the caller has it).
    pub message: String,
}

/// A project's plate layout: one placement per plate, and what is wrong.
#[derive(Clone, Debug, PartialEq)]
pub struct Layout {
    pub placements: Vec<PlatePlacement>,
    pub problems: Vec<PlateProblem>,
}

impl Layout {
    /// The fatal problems, each prefixed by its plate's label from `label`.
    pub fn refusals(&self, label: impl Fn(u32) -> String) -> Vec<String> {
        self.problems.iter().filter(|p| p.fatal).map(|p| format!("{}: {}", label(p.plate), p.message)).collect()
    }
    /// The other problems, likewise.
    pub fn warnings(&self, label: impl Fn(u32) -> String) -> Vec<String> {
        self.problems.iter().filter(|p| !p.fatal).map(|p| format!("{}: {}", label(p.plate), p.message)).collect()
    }
}

/// `"plate 2 (\"Roof\")"`, or `"plate 2"` for a plate only ever numbered.
pub fn plate_label(plate: u32, name: &str) -> String {
    if name.is_empty() { format!("plate {plate}") } else { format!("plate {plate} ({name:?})") }
}

fn mm(v: f64) -> String {
    let r = (v * 10.0).round() / 10.0;
    fmt_item_number(r)
}

/// The plate layout of a project, in one place: plate `n`'s
/// corner on the grid ([`plate_origin`]), and its contents moved as a group
/// so that their union footprint is centred where every nozzle of `machine`
/// reaches ([`Machine::shared`]) — on the whole bed when they are wider than
/// that, with a warning — and moved off any corner the machine cannot print
/// when there is room. One entry per plate, `1..=plates`; a plate no
/// footprint names is placed with no shift.
///
/// **The prime tower**: with `towers.room`, every plate with pieces
/// gets a spot for its tower clear of them ([`crate::bambu_settings::tower_spot`],
/// searched beside, between, in front of and behind its pieces). A plate of
/// more than one filament with no such spot has its pieces moved as a
/// group, the least distance that opens one inside the region they were
/// centred in and off any excluded corner, with a warning; when no move
/// does, the plate is refused, naming it and what would make room. A spot
/// is never written over a piece.
///
/// **Refused** (fatal problems): a plate whose pieces, as the script
/// arranged them, are larger than the bed, or an object taller than the
/// machine prints, or a plate of several filaments with no room for its
/// prime tower. **Warned**: pieces beyond what every nozzle reaches, or
/// taller than the lower nozzle prints; pieces that still cover a corner the
/// machine cannot print; pieces moved off centre to make room for the tower.
///
/// The project writer lays its objects out with this and the model view
/// (`Model.plates`) places the bodies it already has with it, so the
/// preview is the export, to the bounding boxes' tessellation.
pub fn layout_plates(machine: &Machine, plates: u32, footprints: &[Footprint], towers: &TowerPlan) -> Layout {
    let bed = machine.bed();
    let area = machine.area;
    let shared = machine.shared();
    let mut problems = Vec::new();
    let placements = (1..=plates)
        .map(|plate| {
            let mine: Vec<&Footprint> = footprints.iter().filter(|f| f.plate == plate).collect();
            let origin = plate_origin(plate, plates, bed);
            if mine.is_empty() {
                return PlatePlacement { plate, origin, shift: [0.0, 0.0], tower: None };
            }
            let (mut lo, mut hi) = ([f64::MAX; 2], [f64::MIN; 2]);
            for f in &mine {
                for k in 0..2 {
                    lo[k] = lo[k].min(f.lo[k]);
                    hi[k] = hi[k].max(f.hi[k]);
                }
            }
            let size = [hi[0] - lo[0], hi[1] - lo[1]];
            let fits = |r: [f64; 4]| size[0] <= r[2] - r[0] + REACH_SLACK && size[1] <= r[3] - r[1] + REACH_SLACK;
            let region = if fits(shared) { shared } else { area };
            let mut shift = region_shift(region, lo, hi);
            if !fits(area) {
                let turned = size[1] <= bed[0] && size[0] <= bed[1];
                problems.push(PlateProblem {
                    plate,
                    fatal: true,
                    message: format!(
                        "its pieces span {} x {} mm as the script places them, and the {} prints {} x {} mm; {}",
                        mm(size[0]),
                        mm(size[1]),
                        machine.name,
                        mm(bed[0]),
                        mm(bed[1]),
                        if mine.len() > 1 {
                            "move them closer together, or give some of them a plate of their own"
                        } else if turned {
                            "turn it 90 degrees about z, or split it"
                        } else {
                            "split it, or print it at a smaller scale"
                        }
                    ),
                });
            } else if region != shared {
                problems.push(PlateProblem {
                    plate,
                    fatal: false,
                    message: format!(
                        "its pieces span {} x {} mm, wider than the {} x {} mm every nozzle of the {} reaches; a \
                         filament the slicer puts on one nozzle cannot print the part of a piece beyond that \
                         nozzle's reach",
                        mm(size[0]),
                        mm(size[1]),
                        mm(shared[2] - shared[0]),
                        mm(shared[3] - shared[1]),
                        machine.name
                    ),
                });
            }
            // Off the corners the machine cannot print, when the plate has
            // room to give: the smallest move along x or y that clears every
            // excluded rectangle and keeps the pieces on the bed.
            let placed = |shift: [f64; 2]| [lo[0] + shift[0], lo[1] + shift[1], hi[0] + shift[0], hi[1] + shift[1]];
            let overlaps = |b: [f64; 4]| {
                machine.exclude.iter().any(|e| b[0] < e[2] && b[2] > e[0] && b[1] < e[3] && b[3] > e[1])
            };
            if overlaps(placed(shift)) {
                let mut best: Option<[f64; 2]> = None;
                for e in &machine.exclude {
                    let b = placed(shift);
                    for cand in [[e[2] - b[0], 0.0], [0.0, e[3] - b[1]], [e[0] - b[2], 0.0], [0.0, e[1] - b[3]]] {
                        let s = [shift[0] + cand[0], shift[1] + cand[1]];
                        let p = placed(s);
                        let on_bed = p[0] >= area[0] - REACH_SLACK
                            && p[1] >= area[1] - REACH_SLACK
                            && p[2] <= area[2] + REACH_SLACK
                            && p[3] <= area[3] + REACH_SLACK;
                        if on_bed && !overlaps(p) && best.is_none_or(|b| cand[0].abs() + cand[1].abs() < b[0].abs() + b[1].abs()) {
                            best = Some(cand);
                        }
                    }
                }
                match best {
                    Some(c) => shift = [shift[0] + c[0], shift[1] + c[1]],
                    None if fits(area) => problems.push(PlateProblem {
                        plate,
                        fatal: false,
                        message: format!(
                            "its pieces cover the part of the {}'s plate it cannot print ({} mm); the slicer \
                             will refuse a part that stands in it",
                            machine.name,
                            machine
                                .exclude
                                .iter()
                                .map(|e| format!("{} x {}", mm(e[2] - e[0]), mm(e[3] - e[1])))
                                .collect::<Vec<_>>()
                                .join(", ")
                        ),
                    }),
                    None => {}
                }
            }
            // The prime tower: a spot clear of the pieces where they
            // stand; for a plate that prints one, the least move of the
            // pieces that opens one; else the plate is refused.
            let mut tower = None;
            if let (Some(room), true) = (towers.room, fits(area)) {
                let boxes = |s: [f64; 2]| -> Vec<[f64; 4]> {
                    mine.iter().map(|f| [f.lo[0] + s[0], f.lo[1] + s[1], f.hi[0] + s[0], f.hi[1] + s[1]]).collect()
                };
                tower = crate::bambu_settings::tower_spot(machine, room, &boxes(shift));
                if tower.is_none() && towers.needs(plate) {
                    match room_for_tower(machine, room, region, lo, hi, shift, &boxes) {
                        Some((s, spot)) => {
                            let moved = [s[0] - shift[0], s[1] - shift[1]];
                            problems.push(PlateProblem {
                                plate,
                                fatal: false,
                                message: format!(
                                    "its pieces are moved {} from the centre of {} to leave room for its {} mm prime \
                                     tower (at x {}, y {}), which has none beside them as centred",
                                    describe_move(moved),
                                    if region == shared && shared != area { "what every nozzle reaches" } else { "the bed" },
                                    mm(room.side),
                                    mm(spot[0]),
                                    mm(spot[1])
                                ),
                            });
                            shift = s;
                            tower = Some(spot);
                        }
                        None => problems.push(PlateProblem {
                            plate,
                            fatal: true,
                            message: no_room_for_tower(machine, room, size),
                        }),
                    }
                }
            }
            // Two objects standing in each other print as one lump: Cadpole
            // keeps the script's arrangement, so say so rather than move them.
            let overlap = |a: &Footprint, b: &Footprint| {
                let w = a.hi[0].min(b.hi[0]) - a.lo[0].max(b.lo[0]);
                let d = a.hi[1].min(b.hi[1]) - a.lo[1].max(b.lo[1]);
                (w > 0.5 && d > 0.5).then_some((w, d))
            };
            let crossing = mine.iter().enumerate().find_map(|(i, a)| mine[i + 1..].iter().find_map(|b| overlap(a, b)));
            if let Some((w, d)) = crossing {
                problems.push(PlateProblem {
                    plate,
                    fatal: false,
                    message: format!(
                        "two of its objects overlap where they stand (their footprints share {} x {} mm) and would \
                         print as one; move them apart, give one a plate of its own, or make them one object with \
                         name=",
                        mm(w),
                        mm(d)
                    ),
                });
            }
            let tallest = mine.iter().map(|f| f.height).fold(0.0, f64::max);
            if tallest > machine.height + REACH_SLACK {
                problems.push(PlateProblem {
                    plate,
                    fatal: true,
                    message: format!(
                        "a piece is {} mm tall and the {} prints {} mm; split it, or lay it on its side",
                        mm(tallest),
                        machine.name,
                        mm(machine.height)
                    ),
                });
            } else if tallest > machine.shared_height() + REACH_SLACK {
                problems.push(PlateProblem {
                    plate,
                    fatal: false,
                    message: format!(
                        "a piece is {} mm tall; only one nozzle of the {} prints above {} mm, so all of its \
                         filaments must go to that nozzle",
                        mm(tallest),
                        machine.name,
                        mm(machine.shared_height())
                    ),
                });
            }
            PlatePlacement { plate, origin, shift, tower }
        })
        .collect();
    Layout { placements, problems }
}

/// The least move of a plate's pieces, as a group, that leaves its prime
/// tower a spot: the pieces span `lo..hi` before `shift`, stay
/// inside `region` (where they were centred) and off the machine's excluded
/// corners, and `boxes` gives the objects' rectangles under a shift.
/// Candidates: pushed just far enough from each side of what every nozzle
/// reaches to open a strip one tower wide there, and pushed into each side
/// and corner of the region. Returns the new shift and the tower's spot.
fn room_for_tower(
    machine: &Machine,
    room: crate::bambu_settings::TowerRoom,
    region: [f64; 4],
    lo: [f64; 2],
    hi: [f64; 2],
    shift: [f64; 2],
    boxes: &dyn Fn([f64; 2]) -> Vec<[f64; 4]>,
) -> Option<([f64; 2], [f64; 2])> {
    let r = machine.shared();
    let span = room.span();
    // Per axis: the shifts that put the group against either end of the
    // region, and that open a strip one tower deep at either end of the
    // shared reach.
    let mut along: [Vec<f64>; 2] = Default::default();
    for k in 0..2 {
        along[k] = vec![
            shift[k],
            region[k] - lo[k],
            region[k + 2] - hi[k],
            r[k] + span - lo[k],
            r[k + 2] - span - hi[k],
        ];
    }
    let mut cands: Vec<[f64; 2]> = Vec::new();
    for &x in &along[0] {
        for &y in &along[1] {
            cands.push([x, y]);
        }
    }
    let inside = |s: [f64; 2]| {
        lo[0] + s[0] >= region[0] - REACH_SLACK
            && lo[1] + s[1] >= region[1] - REACH_SLACK
            && hi[0] + s[0] <= region[2] + REACH_SLACK
            && hi[1] + s[1] <= region[3] + REACH_SLACK
    };
    let on_corner = |s: [f64; 2]| {
        let b = [lo[0] + s[0], lo[1] + s[1], hi[0] + s[0], hi[1] + s[1]];
        machine.exclude.iter().any(|e| b[0] < e[2] && b[2] > e[0] && b[1] < e[3] && b[3] > e[1])
    };
    // The distance moved, to a micrometre, so two moves equal but for
    // rounding tie, and a tie goes left (then forward): deterministic.
    let dist = |s: [f64; 2]| (((s[0] - shift[0]).abs() + (s[1] - shift[1]).abs()) * 1e3).round();
    cands.sort_by(|a, b| dist(*a).total_cmp(&dist(*b)).then(a[0].total_cmp(&b[0])).then(a[1].total_cmp(&b[1])));
    cands
        .into_iter()
        .filter(|s| inside(*s) && !on_corner(*s))
        .find_map(|s| crate::bambu_settings::tower_spot(machine, room, &boxes(s)).map(|spot| (s, spot)))
}

/// "5 mm left", "3 mm left and 1.5 mm back".
fn describe_move(d: [f64; 2]) -> String {
    let mut parts = Vec::new();
    if d[0].abs() >= 0.05 {
        parts.push(format!("{} mm {}", mm(d[0].abs()), if d[0] < 0.0 { "left" } else { "right" }));
    }
    if d[1].abs() >= 0.05 {
        parts.push(format!("{} mm {}", mm(d[1].abs()), if d[1] < 0.0 { "forward" } else { "back" }));
    }
    if parts.is_empty() { "less than 0.1 mm".to_owned() } else { parts.join(" and ") }
}

/// Why a plate of several filaments is refused for want of room for its
/// prime tower, and what would make some.
fn no_room_for_tower(machine: &Machine, room: crate::bambu_settings::TowerRoom, size: [f64; 2]) -> String {
    let r = machine.shared();
    let (w, d) = (r[2] - r[0], r[3] - r[1]);
    let span = room.span();
    let leaves = |sx: f64, sy: f64| (sx + span <= w + REACH_SLACK && sy <= d + REACH_SLACK) || (sy + span <= d + REACH_SLACK && sx <= w + REACH_SLACK);
    let turn = if !leaves(size[0], size[1]) && leaves(size[1], size[0]) {
        "turn its pieces 90 degrees about z (they then leave the tower room), "
    } else {
        ""
    };
    format!(
        "it prints several filaments, so the slicer stands a prime tower on it, and there is no room for one: a {} mm \
         tower with {} mm clear round it needs a square {} mm across, in the {} x {} mm every nozzle of the {} reaches, \
         that its pieces ({} x {} mm) leave free, wherever they are moved; {}print it in one filament, split it, or \
         narrow the tower (prime_tower_width in the template)",
        mm(room.side),
        mm(room.margin),
        mm(span),
        mm(w),
        mm(d),
        machine.name,
        mm(size[0]),
        mm(size[1]),
        turn
    )
}

/// One part of a print object: a body's mesh in Cadpole's world coordinates
/// and the filament it prints in.
pub struct ProjectPart<'a> {
    pub name: String,
    pub mesh: &'a Mesh,
    /// 1-based filament slot, indexing [`Project::palette`].
    pub slot: u32,
    /// Per-triangle slot override, in `mesh.indices` order; empty or `0`
    /// means `slot`.
    pub tri_slots: Vec<u32>,
}

impl<'a> ProjectPart<'a> {
    pub fn new(name: impl Into<String>, mesh: &'a Mesh, slot: u32) -> ProjectPart<'a> {
        ProjectPart { name: name.into(), mesh, slot: slot.max(1), tri_slots: Vec::new() }
    }
}

/// One print object: what Bambu Studio moves and slices as a unit.
pub struct ProjectObject<'a> {
    pub name: String,
    pub parts: Vec<ProjectPart<'a>>,
}

/// One plate: its tab name, its objects, and its thumbnails (PNG bytes,
/// [`THUMBNAIL_PX`] and [`THUMBNAIL_SMALL_PX`] square) when the caller
/// rendered them.
#[derive(Default)]
pub struct Plate<'a> {
    pub name: String,
    pub objects: Vec<ProjectObject<'a>>,
    pub thumbnail: Option<Vec<u8>>,
    pub thumbnail_small: Option<Vec<u8>>,
}

/// Everything one project file holds. `plates[0]` is plate 1; an empty
/// plate is written empty, so plate numbers stay what the script said.
pub struct Project<'a> {
    pub title: String,
    pub plates: Vec<Plate<'a>>,
    /// Filament slots, slot 1 first: the project's `filament_colour` list.
    pub palette: Vec<Filament>,
    /// The machine the plate grid is laid out for and the pieces are fitted
    /// to: a template's, else the printer's.
    pub machine: Machine,
    /// A project the user saved, whose settings the file carries, mapped onto
    /// `palette`, and how warnings name it. `None` writes Cadpole's
    /// built-in settings for `machine`.
    pub template: Option<(Template, String)>,
}

impl<'a> Default for Project<'a> {
    fn default() -> Project<'a> {
        Project { title: String::new(), plates: Vec::new(), palette: Vec::new(), machine: Machine::default(), template: None }
    }
}

/// A part on its way out: welded, de-duplicated, centred.
struct OutPart {
    id: u32,
    name: String,
    slot: u32,
    /// Vertices relative to `centre`.
    verts: Vec<[f32; 3]>,
    tris: Vec<([u32; 3], u32)>,
    /// Bounding-box centre in world coordinates, as `f32` so that
    /// `verts + centre` gives the original coordinates back to one
    /// rounding.
    centre: [f32; 3],
}

struct OutObj {
    id: u32,
    name: String,
    parts: Vec<OutPart>,
    /// The union bounding box of its parts, world coordinates.
    lo: [f32; 3],
    hi: [f32; 3],
    /// Its centre.
    centre: [f32; 3],
    plate: u32,
}

impl OutObj {
    fn slot(&self) -> u32 {
        self.parts.first().map(|p| p.slot).unwrap_or(1)
    }
    /// A part's offset from the object's centre: the `<component>` transform.
    fn offset(&self, p: &OutPart) -> [f64; 3] {
        [
            (p.centre[0] - self.centre[0]) as f64,
            (p.centre[1] - self.centre[1]) as f64,
            (p.centre[2] - self.centre[2]) as f64,
        ]
    }
}

/// Write `project` as a Bambu Studio project package. Refused, with every
/// reason, when a plate's pieces cannot fit the machine ([`layout_plates`]).
pub fn write_project(project: &Project<'_>) -> Result<Written, String> {
    let mut warnings = Vec::new();
    let (objects, dropped) = build(project, &mut warnings);
    if objects.is_empty() {
        warnings.push("nothing to export: no bodies with triangles".to_owned());
    }
    if dropped > 0 {
        warnings.push(format!(
            "left out {dropped} triangle{} that repeated another's three vertices: 3MF needs \
             every edge used by exactly two triangles, and a back-to-back pair encloses nothing",
            if dropped == 1 { "" } else { "s" },
        ));
    }
    let layout = layout_of(project, &objects);
    let label = |n: u32| plate_label(n, project.plates.get(n as usize - 1).map(|p| p.name.as_str()).unwrap_or(""));
    let refused = layout.refusals(label);
    if !refused.is_empty() {
        return Err(refused.join("; "));
    }
    warnings.extend(layout.warnings(label));
    // Each plate's prime tower where the layout put it.
    let uses: Vec<crate::bambu_settings::PlateUse> =
        layout.placements.iter().map(|pl| crate::bambu_settings::PlateUse { tower: pl.tower }).collect();
    let settings = crate::bambu_settings::project_settings(
        project.template.as_ref().map(|(t, name)| (t, name.as_str())),
        &project.machine,
        &project.palette,
        &uses,
    );
    warnings.extend(settings.warnings.iter().cloned());
    let cover = project
        .plates
        .iter()
        .position(|p| p.thumbnail.is_some())
        .map(|i| (i as u32 + 1, project.plates[i].thumbnail_small.is_some()));

    let mut z = zip::ZipWriter::new();
    z.add(CONTENT_TYPES_PART, content_types().as_bytes());
    z.add(RELS_PART, rels(cover).as_bytes());
    z.add(MODEL_PART, root_model(project, &objects, &layout, &settings.application, cover).as_bytes());
    z.add(MODEL_RELS_PART, model_rels(&objects).as_bytes());
    for o in &objects {
        z.add(&object_part(o.id), object_model(o).as_bytes());
    }
    z.add(BAMBU_MODEL_CONFIG, model_settings(project, &objects).as_bytes());
    z.add(BAMBU_PROJECT_CONFIG, settings.json.as_bytes());
    z.add(BAMBU_SLICE_INFO, slice_info(&settings.application).as_bytes());
    for (i, p) in project.plates.iter().enumerate() {
        let n = i as u32 + 1;
        if let Some(png) = &p.thumbnail {
            z.add(&plate_thumbnail_part(n), png);
        }
        if let Some(png) = &p.thumbnail_small {
            z.add(&plate_thumbnail_small_part(n), png);
        }
    }
    Ok(Written { bytes: z.finish(), warnings })
}

/// [`layout_plates`] over the welded objects: one footprint per object, its
/// box where the script put it and its height; and the prime towers the
/// plates print, from the template and each plate's filaments.
fn layout_of(project: &Project<'_>, objects: &[OutObj]) -> Layout {
    let footprints: Vec<Footprint> = objects
        .iter()
        .map(|o| Footprint {
            plate: o.plate,
            lo: [o.lo[0] as f64, o.lo[1] as f64],
            hi: [o.hi[0] as f64, o.hi[1] as f64],
            height: (o.hi[2] - o.lo[2]) as f64,
        })
        .collect();
    let plates = objects.iter().map(|o| o.plate).max().unwrap_or(0).max(project.plates.len() as u32);
    let filaments = (1..=plates)
        .map(|plate| {
            let mut slots: Vec<u32> = Vec::new();
            for o in objects.iter().filter(|o| o.plate == plate) {
                for p in &o.parts {
                    for s in std::iter::once(p.slot).chain(p.tris.iter().map(|(_, s)| *s)) {
                        if s > 0 && !slots.contains(&s) {
                            slots.push(s);
                        }
                    }
                }
            }
            slots.len()
        })
        .collect();
    let towers = TowerPlan {
        room: crate::bambu_settings::tower_room(project.template.as_ref().map(|(t, _)| t)),
        filaments,
    };
    layout_plates(&project.machine, plates, &footprints, &towers)
}

/// `Metadata/slice_info.config` for an unsliced project, naming the version
/// the root model claims, as Bambu's own do.
fn slice_info(application: &str) -> String {
    let version = application.strip_prefix("BambuStudio-").unwrap_or(application);
    format!(
        "<?xml version=\"1.0\" encoding=\"UTF-8\"?>\n\
         <config>\n\
         \x20<header>\n\
         \x20 <header_item key=\"X-BBL-Client-Type\" value=\"slicer\"/>\n\
         \x20 <header_item key=\"X-BBL-Client-Version\" value=\"{}\"/>\n\
         \x20</header>\n\
         </config>\n",
        esc(version)
    )
}

/// Weld, de-duplicate and centre every part; number parts and objects in
/// one sequence so every id is unique package-wide (a part's mesh object
/// sits in its object's file, but a reader that merges files must not see
/// two resources with one id). Parts take their ids first, then the object.
fn build(project: &Project<'_>, warnings: &mut Vec<String>) -> (Vec<OutObj>, usize) {
    let mut dropped = 0;
    let mut next_id = 1u32;
    let mut out = Vec::new();
    let opts = SolidOptions { pair_limit: 0, ..Default::default() };
    for (pi, plate) in project.plates.iter().enumerate() {
        for obj in &plate.objects {
            let mut parts = Vec::new();
            for p in &obj.parts {
                let mut o = OutObject {
                    id: 0,
                    name: p.name.clone(),
                    slot: p.slot,
                    verts: Vec::new(),
                    tris: Vec::new(),
                    volumes: Vec::new(),
                    items: Vec::new(),
                    layer: String::new(),
                };
                let part = Part {
                    name: p.name.clone(),
                    mesh: p.mesh,
                    slot: p.slot,
                    tri_slots: p.tri_slots.clone(),
                    items: Vec::new(),
                    layer: String::new(),
                };
                dropped += append(&mut o, &part);
                if o.tris.is_empty() {
                    continue;
                }
                let report = cadpole_core::solid::validate_with(&as_mesh(&o, 0, o.tris.len() - 1), &opts);
                if !report.oriented() {
                    warnings.push(format!(
                        "{:?}: 3MF asks for a closed, consistently wound surface and this is not \
                         one -- {}. The file is written; a slicer will try to repair it, or refuse it",
                        p.name,
                        report.problems().join("; "),
                    ));
                }
                let (lo, hi) = bbox(&o.verts);
                let centre = [(lo[0] + hi[0]) * 0.5, (lo[1] + hi[1]) * 0.5, (lo[2] + hi[2]) * 0.5];
                let verts = o.verts.iter().map(|v| [v[0] - centre[0], v[1] - centre[1], v[2] - centre[2]]).collect();
                parts.push(OutPart { id: next_id, name: p.name.clone(), slot: p.slot.max(1), verts, tris: o.tris, centre });
                next_id += 1;
            }
            if parts.is_empty() {
                continue;
            }
            // The object's box is the union of its parts' boxes, which are
            // each `centre ± half-size` of the centred vertices.
            let mut lo = [f32::MAX; 3];
            let mut hi = [f32::MIN; 3];
            for p in &parts {
                let (plo, phi) = bbox(&p.verts);
                for k in 0..3 {
                    lo[k] = lo[k].min(p.centre[k] + plo[k]);
                    hi[k] = hi[k].max(p.centre[k] + phi[k]);
                }
            }
            let centre = [(lo[0] + hi[0]) * 0.5, (lo[1] + hi[1]) * 0.5, (lo[2] + hi[2]) * 0.5];
            out.push(OutObj { id: next_id, name: obj.name.clone(), parts, lo, hi, centre, plate: pi as u32 + 1 });
            next_id += 1;
        }
    }
    (out, dropped)
}

fn bbox(verts: &[[f32; 3]]) -> ([f32; 3], [f32; 3]) {
    let mut lo = [f32::MAX; 3];
    let mut hi = [f32::MIN; 3];
    for v in verts {
        for k in 0..3 {
            lo[k] = lo[k].min(v[k]);
            hi[k] = hi[k].max(v[k]);
        }
    }
    if verts.is_empty() {
        return ([0.0; 3], [0.0; 3]);
    }
    (lo, hi)
}

// ---------------------------------------------------------------------------
// UUIDs
// ---------------------------------------------------------------------------
//
// The Production extension wants an RFC 4122 UUID on the build, every item,
// every object and every component. Bambu Studio's are patterned rather than
// random -- the object's number in the first group, a fixed suffix per
// element kind, version-4 and variant bits in place -- so ours follow the
// same pattern with our ids, which keeps the file reproducible and, should a
// reader ever look inside them, indistinguishable in shape from Bambu's own.

fn object_uuid(id: u32) -> String {
    format!("{id:08x}-61cb-4c03-9d28-80fed5dfa1dc")
}
fn component_uuid(object: u32, part_index: usize) -> String {
    format!("{:04x}{:04x}-b206-40ff-9872-83e8017abed1", object & 0xffff, part_index & 0xffff)
}
fn mesh_uuid(object: u32, part_index: usize) -> String {
    format!("{:04x}{:04x}-81cb-4c03-9d28-80fed5dfa1dc", object & 0xffff, part_index & 0xffff)
}
fn item_uuid(id: u32) -> String {
    format!("{id:08x}-b1ec-4553-aec9-835e5b724bb4")
}

// ---------------------------------------------------------------------------
// The parts
// ---------------------------------------------------------------------------

fn content_types() -> String {
    // Bambu Studio declares `rels`, `model`, `png` (and `gcode`, which we never
    // write) and leaves `.config` undeclared; OPC wants every extension
    // declared, so `config` is, as the standard writer does.
    "<?xml version=\"1.0\" encoding=\"UTF-8\"?>\n\
     <Types xmlns=\"http://schemas.openxmlformats.org/package/2006/content-types\">\n\
     \x20<Default Extension=\"rels\" ContentType=\"application/vnd.openxmlformats-package.relationships+xml\"/>\n\
     \x20<Default Extension=\"model\" ContentType=\"application/vnd.ms-package.3dmanufacturing-3dmodel+xml\"/>\n\
     \x20<Default Extension=\"png\" ContentType=\"image/png\"/>\n\
     \x20<Default Extension=\"config\" ContentType=\"application/octet-stream\"/>\n\
     </Types>\n"
        .to_owned()
}

/// `_rels/.rels`: the model, and — when a plate has a thumbnail — that
/// thumbnail as the OPC package thumbnail and as Bambu's two covers, the
/// way Bambu Studio's `.rels` names `plate_1.png` and `plate_1_small.png`.
fn rels(cover: Option<(u32, bool)>) -> String {
    let mut s = String::from(
        "<?xml version=\"1.0\" encoding=\"UTF-8\"?>\n\
         <Relationships xmlns=\"http://schemas.openxmlformats.org/package/2006/relationships\">\n",
    );
    let _ = writeln!(s, " <Relationship Target=\"/{MODEL_PART}\" Id=\"rel-1\" Type=\"{MODEL_REL}\"/>");
    if let Some((plate, small)) = cover {
        let big = plate_thumbnail_part(plate);
        let _ = writeln!(s, " <Relationship Target=\"/{big}\" Id=\"rel-2\" Type=\"{THUMBNAIL_REL}\"/>");
        let _ = writeln!(s, " <Relationship Target=\"/{big}\" Id=\"rel-3\" Type=\"{COVER_MIDDLE_REL}\"/>");
        if small {
            let _ = writeln!(
                s,
                " <Relationship Target=\"/{}\" Id=\"rel-4\" Type=\"{COVER_SMALL_REL}\"/>",
                plate_thumbnail_small_part(plate)
            );
        }
    }
    s.push_str("</Relationships>\n");
    s
}

fn model_header(s: &mut String) {
    s.push_str("<?xml version=\"1.0\" encoding=\"UTF-8\"?>\n");
    // `requiredextensions="p"`: the Production extension says a package that
    // references objects in other model files MUST list it, and every
    // project Bambu Studio writes does. A consumer without the extension refuses the file, which
    // is the specification's intent -- it could not find the meshes anyway.
    let _ = writeln!(
        s,
        "<model unit=\"millimeter\" xml:lang=\"en-US\" xmlns=\"{CORE_NS}\" xmlns:BambuStudio=\"{BAMBU_NS}\" \
         xmlns:p=\"{PRODUCTION_NS}\" requiredextensions=\"p\">"
    );
}

fn transform(t: [f64; 3]) -> String {
    format!("1 0 0 0 1 0 0 0 1 {} {} {}", fmt_item_number(t[0]), fmt_item_number(t[1]), fmt_item_number(t[2]))
}

/// `3D/3dmodel.model`: every object as a `<components>` list into its file,
/// every object placed on its plate by one `<item>`. `application` is the
/// Bambu Studio version the project claims: what makes both slicers open it
/// as a project rather than as "a 3mf from another vendor".
fn root_model(
    project: &Project<'_>,
    objects: &[OutObj],
    layout: &Layout,
    application: &str,
    cover: Option<(u32, bool)>,
) -> String {
    let mut s = String::new();
    model_header(&mut s);
    let _ = writeln!(s, " <metadata name=\"Application\">{}</metadata>", esc(application));
    s.push_str(" <metadata name=\"BambuStudio:3mfVersion\">1</metadata>\n");
    if let Some((plate, small)) = cover {
        // Unprefixed custom names, as Bambu writes them (the core
        // specification would want a namespace; its files do not have one).
        let _ = writeln!(s, " <metadata name=\"Thumbnail_Middle\">/{}</metadata>", plate_thumbnail_part(plate));
        if small {
            let _ = writeln!(s, " <metadata name=\"Thumbnail_Small\">/{}</metadata>", plate_thumbnail_small_part(plate));
        }
    }
    let _ = writeln!(s, " <metadata name=\"Title\">{}</metadata>", esc(&project.title));
    s.push_str(" <resources>\n");
    for o in objects {
        let _ = writeln!(s, "  <object id=\"{}\" p:UUID=\"{}\" type=\"model\">", o.id, object_uuid(o.id));
        s.push_str("   <components>\n");
        for (j, p) in o.parts.iter().enumerate() {
            let _ = writeln!(
                s,
                "    <component p:path=\"/{}\" objectid=\"{}\" p:UUID=\"{}\" transform=\"{}\"/>",
                object_part(o.id),
                p.id,
                component_uuid(o.id, j),
                transform(o.offset(p)),
            );
        }
        s.push_str("   </components>\n  </object>\n");
    }
    s.push_str(" </resources>\n");
    let _ = writeln!(s, " <build p:UUID=\"{BUILD_UUID}\">");
    // The one layout rule, shared with the model view: every object of a
    // plate is moved by that plate's placement, and its item then carries
    // its own centre on top -- with z its half-height, so it stands on the
    // bed wherever the script had it.
    for o in objects {
        let moved = layout.placements[o.plate as usize - 1].translation();
        let t = [moved[0] + o.centre[0] as f64, moved[1] + o.centre[1] as f64, (o.centre[2] - o.lo[2]) as f64];
        let _ = writeln!(
            s,
            "  <item objectid=\"{}\" p:UUID=\"{}\" transform=\"{}\" printable=\"1\"/>",
            o.id,
            item_uuid(o.id),
            transform(t)
        );
    }
    s.push_str(" </build>\n</model>\n");
    s
}

/// `3D/_rels/3dmodel.model.rels`: "non-root model files MUST be included in
/// the .rels file from the referencing model file" (Production extension).
fn model_rels(objects: &[OutObj]) -> String {
    let mut s = String::from(
        "<?xml version=\"1.0\" encoding=\"UTF-8\"?>\n\
         <Relationships xmlns=\"http://schemas.openxmlformats.org/package/2006/relationships\">\n",
    );
    for (i, o) in objects.iter().enumerate() {
        let _ = writeln!(s, " <Relationship Target=\"/{}\" Id=\"rel-{}\" Type=\"{MODEL_REL}\"/>", object_part(o.id), i + 1);
    }
    s.push_str("</Relationships>\n");
    s
}

/// One object file: its parts as mesh objects, each centred, with Bambu's
/// `paint_color` on any triangle painted a different filament from its part.
/// No core `basematerials`, `pid` or `p1`: Bambu's own projects
/// never carry them, and
/// in a 3MF Bambu Studio 2.5+ reads as standard they start its "Standard
/// 3MF Color Parsing" dialog, which turns colours into surface paint. A
/// part's filament is its `extruder` in `model_settings.config`. The
/// `<build/>` is empty: the Production extension says a consumer ignores a
/// child model's build anyway.
fn object_model(o: &OutObj) -> String {
    let mut s =
        String::with_capacity(o.parts.iter().map(|p| p.tris.len() * 64 + p.verts.len() * 48).sum::<usize>() + 1024);
    model_header(&mut s);
    s.push_str(" <metadata name=\"BambuStudio:3mfVersion\">1</metadata>\n");
    s.push_str(" <resources>\n");
    for (j, p) in o.parts.iter().enumerate() {
        let _ = write!(s, "  <object id=\"{}\" p:UUID=\"{}\" type=\"model\"", p.id, mesh_uuid(o.id, j));
        s.push_str(">\n   <mesh>\n    <vertices>\n");
        for v in &p.verts {
            let _ = writeln!(s, "     <vertex x=\"{}\" y=\"{}\" z=\"{}\"/>", v[0], v[1], v[2]);
        }
        s.push_str("    </vertices>\n    <triangles>\n");
        for (v, slot) in &p.tris {
            let _ = write!(s, "     <triangle v1=\"{}\" v2=\"{}\" v3=\"{}\"", v[0], v[1], v[2]);
            if *slot != p.slot {
                if let Some(code) = paint::whole_triangle(*slot) {
                    let _ = write!(s, " paint_color=\"{code}\"");
                }
            }
            s.push_str("/>\n");
        }
        s.push_str("    </triangles>\n   </mesh>\n  </object>\n");
    }
    s.push_str(" </resources>\n <build/>\n</model>\n");
    s
}

/// A part's 4×4 row-major matrix for `model_settings.config`, translation in
/// the last column: what Bambu Studio's `<metadata key="matrix">` holds.
fn matrix16(t: [f64; 3]) -> String {
    format!(
        "1 0 0 {} 0 1 0 {} 0 0 1 {} 0 0 0 1",
        fmt_item_number(t[0]),
        fmt_item_number(t[1]),
        fmt_item_number(t[2])
    )
}

/// `Metadata/model_settings.config`, in the shape Bambu Studio writes: objects
/// with their parts, then the plates with the objects on each, then the
/// assembly view. Only the keys every version from 01.08 to 02.06 writes,
/// plus a part's `extruder` (which multi-filament projects carry).
fn model_settings(project: &Project<'_>, objects: &[OutObj]) -> String {
    let mut s = String::from("<?xml version=\"1.0\" encoding=\"UTF-8\"?>\n<config>\n");
    for o in objects {
        let _ = writeln!(s, "  <object id=\"{}\">", o.id);
        let _ = writeln!(s, "    <metadata key=\"name\" value=\"{}\"/>", esc(&o.name));
        let _ = writeln!(s, "    <metadata key=\"extruder\" value=\"{}\"/>", o.slot());
        for p in &o.parts {
            let _ = writeln!(s, "    <part id=\"{}\" subtype=\"normal_part\">", p.id);
            let _ = writeln!(s, "      <metadata key=\"name\" value=\"{}\"/>", esc(&p.name));
            let _ = writeln!(s, "      <metadata key=\"matrix\" value=\"{}\"/>", matrix16(o.offset(p)));
            let _ = writeln!(s, "      <metadata key=\"extruder\" value=\"{}\"/>", p.slot);
            s.push_str("    </part>\n");
        }
        s.push_str("  </object>\n");
    }
    for (i, plate) in project.plates.iter().enumerate() {
        let n = i as u32 + 1;
        s.push_str("  <plate>\n");
        let _ = writeln!(s, "    <metadata key=\"plater_id\" value=\"{n}\"/>");
        let _ = writeln!(s, "    <metadata key=\"plater_name\" value=\"{}\"/>", esc(&plate.name));
        s.push_str("    <metadata key=\"locked\" value=\"false\"/>\n");
        if plate.thumbnail.is_some() {
            let _ = writeln!(s, "    <metadata key=\"thumbnail_file\" value=\"{}\"/>", plate_thumbnail_part(n));
        }
        for o in objects.iter().filter(|o| o.plate == n) {
            s.push_str("    <model_instance>\n");
            let _ = writeln!(s, "      <metadata key=\"object_id\" value=\"{}\"/>", o.id);
            s.push_str("      <metadata key=\"instance_id\" value=\"0\"/>\n");
            // A number Bambu Studio writes per instance and nothing here
            // reads; unique is all a reader can ask of it.
            let _ = writeln!(s, "      <metadata key=\"identify_id\" value=\"{}\"/>", o.id);
            s.push_str("    </model_instance>\n");
        }
        s.push_str("  </plate>\n");
    }
    // The assembly view: the objects where the script put them, which for a
    // Cadpole export is the assembled position, plate offsets left out.
    s.push_str("  <assemble>\n");
    for o in objects {
        let _ = writeln!(
            s,
            "   <assemble_item object_id=\"{}\" instance_id=\"0\" transform=\"{}\" offset=\"0 0 0\"/>",
            o.id,
            transform([o.centre[0] as f64, o.centre[1] as f64, o.centre[2] as f64]),
        );
    }
    s.push_str("  </assemble>\n</config>\n");
    s
}

#[cfg(test)]
#[path = "bambu_tests.rs"]
mod tests;
