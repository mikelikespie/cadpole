//! Named printer profiles: the machines a Cadpole user is actually printing on.
//!
//! [`crate::Profile`] is what the checks read. This module is where the
//! numbers in it come from when a script or a request says `bambu_a1` instead
//! of spelling out a bed size.
//!
//! # Where the numbers come from
//!
//! Each [`Printer`] carries its own [`Printer::source`] string, and the rule
//! for what may go in this table is: **build volume, nozzle diameter and
//! stock layer height are vendor specifications**, quoted from the machine's
//! own specification page and its vendor slicer's stock 0.4 mm profile. They
//! are not guesses and they are not measured here.
//!
//! Two numbers in a profile are *not* vendor specifications, and saying so is
//! the point of this paragraph:
//!
//! * **`max_overhang_degrees` is 45 for every machine.** It is the
//!   rule-of-thumb threshold, not a spec: PrusaSlicer's stock support
//!   threshold and Bambu Studio's are different from each other and change
//!   between releases, and a number copied from one of them would be a
//!   statement about a slicer's defaults dressed up as a statement about a
//!   printer. 45 degrees is what the check has always used and what the
//!   overhang analysis view colours against.
//! * **`perimeters` is 2 for every machine**, for the same reason: it is a
//!   slicer setting, and `min_wall` derives from it. Pass `perimeters=` to
//!   change it.
//! * **`min_gap` is 0.3 mm for every machine**: the gap below which two
//!   bodies printed together fuse is a property of the extrusion width and
//!   the machine's dimensional error, not a published specification
//!   (`crate::Profile::min_gap` says where 0.3 comes from). Pass `min_gap=`
//!   to change it.
//!
//! A profile is a *starting point*: every field can be overridden per call, so
//! `print_check(body, printer="prusa_mk4", nozzle=0.6)` is a 0.6 mm nozzle on
//! an MK4-sized bed.
//!
//! # Bambu machines
//!
//! A Bambu row's build volume is what **Bambu Studio's own printer profile
//! lets you print** — its `printable_area` and `printable_height`, read from
//! projects Bambu Studio saved — wherever that is smaller than the spec
//! page's: the X1 and P1 series' pages give 256 x 256 x 256 mm and footnote
//! that Bambu Studio's default printable height is 250 mm and that the
//! filament cutter takes an 18 x 28 mm corner, which is [`Printer::exclude`].
//! A two-nozzle machine also says what each nozzle reaches
//! ([`Printer::nozzles`]): the spec pages split the volume into "single
//! nozzle", "dual nozzle" and "total", and the saved projects give each
//! nozzle's X range and height. [`Printer::bambu`] names the machine's stock
//! Bambu Studio presets, which a Bambu project without a template names so
//! the slicer selects them.

use crate::{Material, Profile};

/// What one nozzle of a multi-nozzle machine reaches.
#[derive(Clone, Copy, Debug, PartialEq)]
pub struct Nozzle {
    /// `[x0, y0, x1, y1]` of the plate it reaches, in mm.
    pub area: [f64; 4],
    /// How high it prints, in mm.
    pub height: f64,
    /// Hotends it swaps between: the H2C's right-hand Vortek rack holds 6.
    pub hotends: u32,
}

/// A Bambu machine's stock Bambu Studio presets, by name.
#[derive(Clone, Copy, Debug, PartialEq)]
pub struct BambuPresets {
    /// `printer_model`.
    pub model: &'static str,
    /// `printer_settings_id`: the machine with its 0.4 mm nozzle.
    pub printer: &'static str,
    /// `print_settings_id`: the 0.20 mm stock process.
    pub process: &'static str,
    /// `filament_settings_id`: Bambu PLA Basic for this machine.
    pub filament: &'static str,
    /// That filament's `filament_ids` code.
    pub filament_id: &'static str,
}

/// The shape of the build plate seen from above.
#[derive(Clone, Copy, Debug, PartialEq, Eq, Default)]
pub enum BedShape {
    /// A rectangle `bed[0]` by `bed[1]`.
    #[default]
    Rectangular,
    /// A circle of diameter `bed[0]`. `bed[1]` is the same number, so the
    /// bounding-box arithmetic keeps working; the fit test is against the
    /// circle.
    Circular,
}

impl BedShape {
    pub fn as_str(self) -> &'static str {
        match self {
            BedShape::Rectangular => "rectangular",
            BedShape::Circular => "circular",
        }
    }

    pub fn parse(s: &str) -> Option<BedShape> {
        match s.to_ascii_lowercase().as_str() {
            "rect" | "rectangular" | "rectangle" | "square" => Some(BedShape::Rectangular),
            "circle" | "circular" | "round" | "delta" => Some(BedShape::Circular),
            _ => None,
        }
    }
}

/// One machine in the table.
#[derive(Clone, Copy, Debug, PartialEq)]
pub struct Printer {
    /// The name a script or a request uses. Lower case, underscores.
    pub id: &'static str,
    /// How the machine is written on the box.
    pub display_name: &'static str,
    /// Build volume in mm: across the plate, across the plate, up.
    pub bed: [f64; 3],
    pub bed_shape: BedShape,
    /// Stock nozzle diameter in mm.
    pub nozzle: f64,
    /// The vendor slicer's stock layer height for that nozzle, in mm.
    pub layer_height: f64,
    /// The filament the profile assumes. Overridable per call.
    pub material: Material,
    /// Where the build volume, nozzle and layer height were read from.
    pub source: &'static str,
    /// Other spellings that resolve to this machine.
    pub aliases: &'static [&'static str],
    /// Rectangles of the plate nothing may stand on, `[x0, y0, x1, y1]` in
    /// mm: the X1 and P1 series' filament-cutter corner. Empty for most.
    pub exclude: &'static [[f64; 4]],
    /// What each nozzle reaches, for a machine with more than one; empty
    /// when its one nozzle reaches the whole bed.
    pub nozzles: &'static [Nozzle],
    /// Its stock Bambu Studio presets, for a Bambu machine.
    pub bambu: Option<BambuPresets>,
}

impl Printer {
    /// The settings the checks read. `min_wall` follows from the nozzle and
    /// the two perimeters [`Profile::default`] assumes.
    pub fn profile(&self) -> Profile {
        Profile {
            name: self.id.to_owned(),
            nozzle: self.nozzle,
            perimeters: 2.0,
            min_wall: self.nozzle * 2.0,
            layer_height: self.layer_height,
            min_gap: 0.3,
            max_overhang_degrees: 45.0,
            bed: self.bed,
            bed_shape: self.bed_shape,
            material: self.material,
            build_direction: crate::Vec3::Z,
            chord_gap_fraction: crate::CHORD_GAP_FRACTION,
            // A tip within one of this machine's layers is not an island.
            support: crate::SupportRules { min_island: self.layer_height, ..crate::SupportRules::default() },
        }
    }
}

/// The X1 and P1 series' filament-cutter corner: 18 x 28 mm at the front
/// left, which Bambu Studio's `bed_exclude_area` keeps clear.
const CUTTER_CORNER: &[[f64; 4]] = &[[0.0, 0.0, 18.0, 28.0]];

/// Every machine Cadpole knows by name.
///
/// Ordered as a user would pick from it: the common desktop machines first.
/// Adding one is one row plus its source; nothing else in Cadpole has a table of
/// printers in it.
pub const PRINTERS: &[Printer] = &[
    Printer {
        id: "bambu_a1",
        display_name: "Bambu Lab A1",
        bed: [256.0, 256.0, 256.0],
        bed_shape: BedShape::Rectangular,
        nozzle: 0.4,
        layer_height: 0.2,
        material: Material::PLA,
        source: "Bambu Lab A1 specifications (build volume 256 x 256 x 256 mm, stock 0.4 mm hotend), the same \
                 printable area and height in projects Bambu Studio saved; Bambu Studio's stock 0.20 mm \
                 Standard profile for a 0.4 mm nozzle",
        aliases: &["a1", "bambulab_a1"],
        exclude: &[],
        nozzles: &[],
        bambu: Some(BambuPresets {
            model: "Bambu Lab A1",
            printer: "Bambu Lab A1 0.4 nozzle",
            process: "0.20mm Standard @BBL A1",
            filament: "Bambu PLA Basic @BBL A1",
            filament_id: "GFA00",
        }),
    },
    Printer {
        id: "bambu_a1_mini",
        display_name: "Bambu Lab A1 mini",
        bed: [180.0, 180.0, 180.0],
        bed_shape: BedShape::Rectangular,
        nozzle: 0.4,
        layer_height: 0.2,
        material: Material::PLA,
        source: "Bambu Lab A1 mini specifications (build volume 180 x 180 x 180 mm, stock 0.4 mm hotend), the \
                 same in projects Bambu Studio saved; Bambu Studio's stock 0.20 mm Standard profile for a 0.4 mm \
                 nozzle",
        aliases: &["a1_mini", "bambulab_a1_mini"],
        exclude: &[],
        nozzles: &[],
        bambu: Some(BambuPresets {
            model: "Bambu Lab A1 mini",
            printer: "Bambu Lab A1 mini 0.4 nozzle",
            process: "0.20mm Standard @BBL A1M",
            filament: "Bambu PLA Basic @BBL A1M",
            filament_id: "GFA00",
        }),
    },
    Printer {
        id: "bambu_p1s",
        display_name: "Bambu Lab P1S",
        bed: [256.0, 256.0, 250.0],
        bed_shape: BedShape::Rectangular,
        nozzle: 0.4,
        layer_height: 0.2,
        material: Material::PLA,
        source: "Bambu Lab P1S specifications (build volume 256 x 256 x 256 mm, stock 0.4 mm hotend; footnoted: \
                 Bambu Studio's default printable height is 250 mm and the filament cutter takes an 18 x 28 mm \
                 corner, wiki 'print volume limitations'), and those 250 mm and that corner in projects Bambu \
                 Studio saved; Bambu Studio's stock 0.20 mm Standard profile for a 0.4 mm nozzle",
        aliases: &["p1s"],
        exclude: CUTTER_CORNER,
        nozzles: &[],
        bambu: Some(BambuPresets {
            model: "Bambu Lab P1S",
            printer: "Bambu Lab P1S 0.4 nozzle",
            process: "0.20mm Standard @BBL X1C",
            filament: "Bambu PLA Basic @BBL X1C",
            filament_id: "GFA00",
        }),
    },
    Printer {
        id: "bambu_p2s",
        display_name: "Bambu Lab P2S",
        bed: [256.0, 256.0, 256.0],
        bed_shape: BedShape::Rectangular,
        nozzle: 0.4,
        layer_height: 0.2,
        material: Material::PLA,
        source: "Bambu Lab P2S specifications on the official store (build volume 256 x 256 x 256 mm, 0.4 mm \
                 hardened steel nozzle); the printable height is not cross-checked against a saved project. \
                 The printer preset name is the one saved projects list among \
                 their compatible machines; the process and filament names follow the other machines' pattern \
                 and are not confirmed",
        aliases: &["p2s"],
        exclude: &[],
        nozzles: &[],
        bambu: Some(BambuPresets {
            model: "Bambu Lab P2S",
            printer: "Bambu Lab P2S 0.4 nozzle",
            process: "0.20mm Standard @BBL P2S",
            filament: "Bambu PLA Basic @BBL P2S",
            filament_id: "GFA00",
        }),
    },
    Printer {
        id: "bambu_x1c",
        display_name: "Bambu Lab X1 Carbon",
        bed: [256.0, 256.0, 250.0],
        bed_shape: BedShape::Rectangular,
        nozzle: 0.4,
        layer_height: 0.2,
        material: Material::PLA,
        source: "Bambu Lab X1 Carbon specifications (build volume 256 x 256 x 256 mm, stock 0.4 mm hardened \
                 hotend; footnoted: Bambu Studio's default printable height is 250 mm and the filament cutter \
                 takes an 18 x 28 mm corner), and those 250 mm and that corner in projects Bambu Studio saved; \
                 Bambu Studio's stock 0.20 mm Standard profile for a 0.4 mm nozzle",
        aliases: &["x1c", "x1", "bambu_x1", "bambu_x1_carbon"],
        exclude: CUTTER_CORNER,
        nozzles: &[],
        bambu: Some(BambuPresets {
            model: "Bambu Lab X1 Carbon",
            printer: "Bambu Lab X1 Carbon 0.4 nozzle",
            process: "0.20mm Standard @BBL X1C",
            filament: "Bambu PLA Basic @BBL X1C",
            filament_id: "GFA00",
        }),
    },
    Printer {
        id: "bambu_x1e",
        display_name: "Bambu Lab X1E",
        bed: [256.0, 256.0, 250.0],
        bed_shape: BedShape::Rectangular,
        nozzle: 0.4,
        layer_height: 0.2,
        material: Material::PLA,
        source: "Bambu Lab X1E FAQ on the wiki (build volume 256 x 256 x 256 mm) and the X1 series' print volume \
                 limitations (250 mm printable height in Bambu Studio, an 18 x 28 mm cutter corner); a project \
                 Bambu Studio saved for an X1E has that area, height and corner and a 0.4 mm nozzle",
        aliases: &["x1e"],
        exclude: CUTTER_CORNER,
        nozzles: &[],
        bambu: Some(BambuPresets {
            model: "Bambu Lab X1E",
            printer: "Bambu Lab X1E 0.4 nozzle",
            process: "0.20mm Standard @BBL X1C",
            filament: "Bambu PLA Basic @BBL X1C",
            filament_id: "GFA00",
        }),
    },
    Printer {
        id: "bambu_h2d",
        display_name: "Bambu Lab H2D",
        bed: [350.0, 320.0, 325.0],
        bed_shape: BedShape::Rectangular,
        nozzle: 0.4,
        layer_height: 0.2,
        material: Material::PLA,
        source: "Bambu Lab H2D specifications on the official store (total volume for two nozzles 350 x 320 x \
                 325 mm, single nozzle 325 x 320 x 325, dual-nozzle printing 300 x 320 x 325; two 0.4 mm hotends); \
                 projects Bambu Studio saved give the left nozzle X 0-325 up to 320 mm and \
                 the right X 25-350 up to 325 (the store gives no per-side height)",
        aliases: &["h2d", "bambulab_h2d"],
        exclude: &[],
        nozzles: &[
            Nozzle { area: [0.0, 0.0, 325.0, 320.0], height: 320.0, hotends: 1 },
            Nozzle { area: [25.0, 0.0, 350.0, 320.0], height: 325.0, hotends: 1 },
        ],
        bambu: Some(BambuPresets {
            model: "Bambu Lab H2D",
            printer: "Bambu Lab H2D 0.4 nozzle",
            process: "0.20mm Standard @BBL H2D",
            filament: "Bambu PLA Basic @BBL H2D",
            filament_id: "GFA00",
        }),
    },
    Printer {
        id: "bambu_h2c",
        display_name: "Bambu Lab H2C",
        bed: [330.0, 320.0, 325.0],
        bed_shape: BedShape::Rectangular,
        nozzle: 0.4,
        layer_height: 0.2,
        material: Material::PLA,
        source: "Bambu Lab H2C specifications on the official store (total volume for two nozzles 330 x 320 x \
                 325 mm; single nozzle 325 x 320 x 320 left, 305 x 320 x 325 right; dual-nozzle printing 300 x \
                 320 x 325; a left hotend and the right-hand Vortek rack of six, seven hotends that print seven \
                 filaments without a purge); projects Bambu Studio saved agree exactly: \
                 left X 0-325 up to 320 mm with 1 hotend, right X 25-330 up to 325 with 6. The 0.20 mm process \
                 name follows the H2C's other stock processes and is not confirmed",
        aliases: &["h2c", "bambulab_h2c"],
        exclude: &[],
        nozzles: &[
            Nozzle { area: [0.0, 0.0, 325.0, 320.0], height: 320.0, hotends: 1 },
            Nozzle { area: [25.0, 0.0, 330.0, 320.0], height: 325.0, hotends: 6 },
        ],
        bambu: Some(BambuPresets {
            model: "Bambu Lab H2C",
            printer: "Bambu Lab H2C 0.4 nozzle",
            process: "0.20mm Standard @BBL H2C",
            filament: "Bambu PLA Basic @BBL H2C",
            filament_id: "GFA00",
        }),
    },
    Printer {
        id: "bambu_h2s",
        display_name: "Bambu Lab H2S",
        bed: [340.0, 320.0, 340.0],
        bed_shape: BedShape::Rectangular,
        nozzle: 0.4,
        layer_height: 0.2,
        material: Material::PLA,
        source: "Bambu Lab H2S specifications on the official store (build volume 340 x 320 x 340 mm, one 0.4 mm \
                 hardened steel nozzle); not cross-checked against a saved project. \
                 The printer preset name is the one saved projects list among their compatible \
                 machines; the process and filament names follow the other machines' pattern and are not \
                 confirmed",
        aliases: &["h2s", "bambulab_h2s"],
        exclude: &[],
        nozzles: &[],
        bambu: Some(BambuPresets {
            model: "Bambu Lab H2S",
            printer: "Bambu Lab H2S 0.4 nozzle",
            process: "0.20mm Standard @BBL H2S",
            filament: "Bambu PLA Basic @BBL H2S",
            filament_id: "GFA00",
        }),
    },
    Printer {
        id: "prusa_mk4",
        display_name: "Prusa MK4 / MK4S",
        bed: [250.0, 210.0, 220.0],
        bed_shape: BedShape::Rectangular,
        nozzle: 0.4,
        layer_height: 0.2,
        material: Material::PLA,
        source: "Prusa Research MK4/MK4S specifications (build volume 250 x 210 x 220 mm, stock 0.4 mm \
                 nozzle); PrusaSlicer's stock 0.20 mm QUALITY profile",
        aliases: &["mk4", "mk4s", "prusa_mk4s"],
        exclude: &[],
        nozzles: &[],
        bambu: None,
    },
    Printer {
        id: "ender3",
        display_name: "Creality Ender 3",
        bed: [220.0, 220.0, 250.0],
        bed_shape: BedShape::Rectangular,
        nozzle: 0.4,
        layer_height: 0.2,
        material: Material::PLA,
        source: "Creality Ender 3 specifications (build volume 220 x 220 x 250 mm, stock 0.4 mm nozzle); \
                 Creality's and PrusaSlicer's stock 0.20 mm profile for a 0.4 mm nozzle",
        aliases: &["ender3v2", "creality_ender_3"],
        exclude: &[],
        nozzles: &[],
        bambu: None,
    },
];

/// Look up a machine by id or alias. Case and `-`/`_`/space are not
/// significant, so `"Bambu A1"`, `"bambu_a1"` and `"a1"` are the same machine.
pub fn printer(name: &str) -> Option<&'static Printer> {
    let want = normalise(name);
    PRINTERS
        .iter()
        .find(|p| normalise(p.id) == want || p.aliases.iter().any(|a| normalise(a) == want) || normalise(p.display_name) == want)
}

/// Every id, for a diagnostic that has to list the choices.
pub fn printer_ids() -> impl Iterator<Item = &'static str> {
    PRINTERS.iter().map(|p| p.id)
}

/// `"bambu_a1, bambu_a1_mini, ..."`, for an error message.
pub fn printer_list() -> String {
    printer_ids().collect::<Vec<_>>().join(", ")
}

fn normalise(s: &str) -> String {
    s.chars()
        .filter(|c| !matches!(c, '-' | '_' | ' '))
        .flat_map(|c| c.to_lowercase())
        .collect()
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn every_supported_printer_is_in_the_table() {
        for want in [
            "bambu_a1",
            "bambu_a1_mini",
            "bambu_p1s",
            "bambu_p2s",
            "bambu_x1c",
            "bambu_x1e",
            "bambu_h2d",
            "bambu_h2c",
            "bambu_h2s",
            "prusa_mk4",
            "ender3",
        ] {
            assert!(printer(want).is_some(), "{want} is missing from the printer table");
        }
        for (short, id) in [("H2D", "bambu_h2d"), ("h2c", "bambu_h2c"), ("Bambu Lab H2S", "bambu_h2s"), ("x1e", "bambu_x1e")] {
            assert_eq!(printer(short).map(|p| p.id), Some(id), "{short}");
        }
    }

    /// The two-nozzle machines say what each nozzle reaches, inside
    /// the bed and overlapping, and the numbers are the spec pages' and the
    /// saved projects'.
    #[test]
    fn a_two_nozzle_machine_says_what_each_nozzle_reaches() {
        let h2d = printer("bambu_h2d").unwrap();
        assert_eq!(h2d.bed, [350.0, 320.0, 325.0]);
        assert_eq!(h2d.nozzles.len(), 2);
        assert_eq!((h2d.nozzles[0].area[2] - h2d.nozzles[0].area[0], h2d.nozzles[0].height), (325.0, 320.0));
        assert_eq!((h2d.nozzles[1].area[0], h2d.nozzles[1].area[2]), (25.0, 350.0));
        let h2c = printer("bambu_h2c").unwrap();
        assert_eq!(h2c.bed, [330.0, 320.0, 325.0]);
        assert_eq!(h2c.nozzles.iter().map(|n| n.hotends).sum::<u32>(), 7, "1 left + 6 Vortek");
        // The spec pages' "single nozzle" sizes: left 325 x 320 x 320, right 305 x 320 x 325.
        assert_eq!(h2c.nozzles[0].area, [0.0, 0.0, 325.0, 320.0]);
        assert_eq!(h2c.nozzles[0].height, 320.0);
        assert_eq!(h2c.nozzles[1].area[2] - h2c.nozzles[1].area[0], 305.0);
        assert_eq!(h2c.nozzles[1].height, 325.0);
        for p in PRINTERS {
            for n in p.nozzles {
                assert!(n.area[0] >= 0.0 && n.area[2] <= p.bed[0] && n.area[1] >= 0.0 && n.area[3] <= p.bed[1], "{}", p.id);
                assert!(n.height <= p.bed[2] && n.hotends >= 1, "{}", p.id);
            }
            if p.nozzles.len() > 1 {
                // Dual-nozzle printing: 300 mm across on both H2 machines.
                let shared = p.nozzles.iter().map(|n| n.area[2]).fold(f64::MAX, f64::min)
                    - p.nozzles.iter().map(|n| n.area[0]).fold(f64::MIN, f64::max);
                assert_eq!(shared, 300.0, "{}", p.id);
            }
            for e in p.exclude {
                assert!(e[2] <= p.bed[0] && e[3] <= p.bed[1], "{}", p.id);
            }
        }
    }

    #[test]
    fn every_bambu_machine_names_its_stock_presets() {
        for p in PRINTERS {
            assert_eq!(p.bambu.is_some(), p.id.starts_with("bambu_"), "{}", p.id);
            if let Some(b) = p.bambu {
                assert!(b.printer.starts_with(b.model) && b.printer.ends_with("0.4 nozzle"), "{}: {}", p.id, b.printer);
                assert!(b.process.starts_with("0.20mm Standard @BBL "), "{}: {}", p.id, b.process);
                assert!(b.filament.starts_with("Bambu PLA Basic @BBL "), "{}: {}", p.id, b.filament);
                assert_eq!(b.filament_id, "GFA00");
            }
        }
        // The X1 and P1 series keep the cutter corner clear and print 250 mm.
        for id in ["bambu_x1c", "bambu_x1e", "bambu_p1s"] {
            let p = printer(id).unwrap();
            assert_eq!(p.exclude, &[[0.0, 0.0, 18.0, 28.0]], "{id}");
            assert_eq!(p.bed[2], 250.0, "{id}");
        }
    }

    #[test]
    fn lookup_ignores_case_dashes_underscores_and_spaces() {
        let a1 = printer("bambu_a1").unwrap();
        for spelling in ["Bambu_A1", "bambu-a1", "BAMBU A1", "a1", "Bambu Lab A1"] {
            assert_eq!(printer(spelling), Some(a1), "{spelling} should resolve to the A1");
        }
    }

    #[test]
    fn an_unknown_printer_is_none_and_the_list_can_be_shown() {
        assert!(printer("makerbot_cupcake").is_none());
        assert!(printer_list().contains("prusa_mk4"), "{}", printer_list());
    }

    #[test]
    fn no_two_printers_share_an_id_or_an_alias() {
        let mut seen: Vec<String> = Vec::new();
        for p in PRINTERS {
            for name in std::iter::once(p.id).chain(p.aliases.iter().copied()) {
                let n = normalise(name);
                assert!(!seen.contains(&n), "{name} is claimed twice");
                seen.push(n);
            }
        }
    }

    #[test]
    fn every_printer_states_where_its_numbers_came_from() {
        for p in PRINTERS {
            assert!(p.source.len() > 30, "{} has no real source note", p.id);
            assert!(p.bed.iter().all(|v| *v > 0.0), "{} has a nonsense bed", p.id);
            assert!(p.nozzle > 0.0 && p.layer_height > 0.0, "{} has a nonsense nozzle or layer", p.id);
            assert!(p.layer_height < p.nozzle, "{}: a layer taller than the nozzle does not stick", p.id);
        }
    }

    #[test]
    fn a_profile_derives_min_wall_from_the_nozzle() {
        let p = printer("prusa_mk4").unwrap().profile();
        assert_eq!(p.bed, [250.0, 210.0, 220.0]);
        assert!((p.min_wall - 0.8).abs() < 1e-12);
        assert_eq!(p.name, "prusa_mk4");
        assert_eq!(p.material, Material::PLA);
    }

    #[test]
    fn bed_shapes_parse_the_words_people_use() {
        assert_eq!(BedShape::parse("Round"), Some(BedShape::Circular));
        assert_eq!(BedShape::parse("rectangle"), Some(BedShape::Rectangular));
        assert_eq!(BedShape::parse("hexagon"), None);
    }
}
