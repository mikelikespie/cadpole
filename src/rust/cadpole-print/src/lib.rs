//! Print checks: is this mesh printable, and which way up?
//!
//! Everything here works on a flat [`Mesh`](cadpole_core::tessellate::Mesh), so a
//! B-rep body and a mesh body are checked by exactly the same code (the B-rep
//! is tessellated first, at the document's tolerance). Nothing here knows
//! about Starlark or protobuf; `cadpole-engine` turns a [`Report`] into
//! diagnostics and `cadpole-script` exposes `print_check()`.
//!
//! Five checks, each returning *locations*, never a bare boolean:
//!
//! | module | what it measures |
//! | --- | --- |
//! | [`mesh_checks`] | the solid validator (`cadpole_core::solid`) with positions: boundary holes, non-manifold edges, flipped and inverted normals, self-intersections, doubled and internal faces, degenerate triangles, shell count |
//! | [`thickness`] | wall thickness by ray casting into the solid against the opposite surface |
//! | [`overhang`] | angle from vertical about the build direction, and the unsupported area |
//! | [`bed`] | does it fit the build volume, and by how much it does not |
//! | [`orient`] | which way up to print it, scored and explained |
//! | [`analysis`] | those per-triangle fields as colour ramps with a legend |
//! | [`assembly`] | the rules *between* bodies printed together: gaps that fuse, horizontal gaps that are not whole layers, bodies in the air |
//!
//! The thickness and overhang checks also return a **per-triangle field**
//! (`Vec<f32>`, one entry per triangle of the input mesh, in index order), so
//! the same numbers colour the `thickness` and `overhang` analysis views
//! (docs/views.md) and no analysis is computed twice.

use cadpole_core::geom::Vec3;
use cadpole_core::tessellate::Mesh;

pub mod analysis;
pub mod assembly;
pub mod bed;
pub mod chord;
pub mod mesh_checks;
pub mod orient;
pub mod overhang;
pub mod printers;
pub mod thickness;

pub use analysis::{Analysis, Legend};
pub use bed::BedReport;
pub use chord::{ChordFinding, CHORD_GAP_FRACTION};
pub use mesh_checks::{Hole, MeshReport};
pub use orient::{Candidate, Orientation};
pub use overhang::OverhangReport;
pub use printers::{printer, printer_ids, printer_list, BedShape, Printer, PRINTERS};
pub use thickness::ThicknessReport;

/// Weld tolerance for the topology checks, in mm. The same value the mesh
/// kernel welds with, so "watertight" means the same thing on both sides.
pub const WELD_EPS: f32 = 1e-4;

/// A filament, by name and density.
///
/// Density is here because it is the one number that turns a solid's volume
/// into something a user can act on — how much filament a part will cost —
/// and because "material" without a number attached is decoration. The values
/// are the figures filament manufacturers publish on their technical data
/// sheets; they vary by a percent or two between brands, which is well inside
/// the error of estimating a print from its solid volume anyway.
#[derive(Clone, Copy, Debug, PartialEq)]
pub struct Material {
    pub name: &'static str,
    /// Solid density in g/cm3.
    pub density_g_cm3: f64,
}

impl Material {
    pub const PLA: Material = Material { name: "PLA", density_g_cm3: 1.24 };
    pub const PETG: Material = Material { name: "PETG", density_g_cm3: 1.27 };
    pub const ABS: Material = Material { name: "ABS", density_g_cm3: 1.04 };
    pub const ASA: Material = Material { name: "ASA", density_g_cm3: 1.07 };
    pub const TPU: Material = Material { name: "TPU", density_g_cm3: 1.21 };
    pub const NYLON: Material = Material { name: "PA", density_g_cm3: 1.14 };
    pub const PC: Material = Material { name: "PC", density_g_cm3: 1.20 };

    /// Every material Cadpole knows, in the order a picker would show them.
    pub const ALL: &'static [Material] =
        &[Material::PLA, Material::PETG, Material::ABS, Material::ASA, Material::TPU, Material::NYLON, Material::PC];

    /// Look one up by name, case-insensitively. `"nylon"` and `"pa"` are the
    /// same material.
    pub fn parse(s: &str) -> Option<Material> {
        let want = s.trim().to_ascii_uppercase();
        let want = if want == "NYLON" { "PA".to_owned() } else { want };
        Material::ALL.iter().copied().find(|m| m.name == want)
    }

    /// `"PLA, PETG, ABS, ..."`, for a diagnostic.
    pub fn list() -> String {
        Material::ALL.iter().map(|m| m.name).collect::<Vec<_>>().join(", ")
    }

    /// Mass of `mm3` of this material, in grams.
    pub fn mass_g(&self, mm3: f64) -> f64 {
        mm3 * self.density_g_cm3 / 1000.0
    }
}

impl Default for Material {
    fn default() -> Material {
        Material::PLA
    }
}

/// Printer and process settings the checks are judged against.
///
/// The defaults are a 0.4 mm nozzle, two perimeters, a 0.2 mm layer, a 45°
/// support threshold, PLA and a 256 mm cube of build volume (a Bambu
/// X1/P1-sized machine). [`printers`] fills this in from a named machine;
/// `printer("bambu_a1")` in the script API is the same table.
#[derive(Clone, Debug, PartialEq)]
pub struct Profile {
    /// The machine this came from, or `"default"`. Carried so a report can say
    /// what it judged the model against.
    pub name: String,
    /// Nozzle diameter in mm.
    pub nozzle: f64,
    /// Perimeters the slicer is set to print. `min_wall` defaults to
    /// `nozzle * perimeters`: a wall thinner than the extrusions that have to
    /// fit in it either vanishes or prints as a single squashed bead.
    pub perimeters: f64,
    /// Minimum acceptable wall thickness in mm.
    pub min_wall: f64,
    /// Layer height in mm. Reported, used to say how many layers tall a
    /// part is, and the grid a horizontal gap between two bodies is held to
    /// ([`assembly`]); no check refuses a single body because of it.
    pub layer_height: f64,
    /// The smallest gap between two bodies printed together that still
    /// prints as a gap, in mm. Below it the `fuse` check fires. 0.3 mm for
    /// FDM: one extrusion is 0.42–0.45 mm wide at a 0.4 mm nozzle and two
    /// walls closer than about a third of that fuse, a vertical bore prints
    /// 0.1–0.15 mm undersize and a 30 mm circle is out by ~0.1 mm radially, so 0.3 is the gap that
    /// survives both. A Cadpole number, not a vendor's, like `max_overhang`.
    pub min_gap: f64,
    /// Overhang angle from vertical, in degrees, past which a surface needs
    /// support. 0° is a vertical wall, 90° a flat ceiling.
    pub max_overhang_degrees: f64,
    /// Build volume in mm, in the printer's own X/Y/Z.
    pub bed: [f64; 3],
    /// Rectangular (the usual) or circular (a delta). The bed-fit check tests
    /// the footprint against the right shape.
    pub bed_shape: BedShape,
    /// The filament the estimate assumes.
    pub material: Material,
    /// Which way "up" is: the direction the nozzle moves away from the bed.
    pub build_direction: Vec3,
    /// The chord-error-vs-gap rule ([`chord`]): a body whose export
    /// mesh sits farther off its exact surfaces than this fraction of the
    /// gap to another body gets a warning. [`CHORD_GAP_FRACTION`], a third,
    /// unless a profile says otherwise; no printer does today.
    pub chord_gap_fraction: f64,
}

impl Default for Profile {
    fn default() -> Profile {
        Profile {
            name: "default".to_owned(),
            nozzle: 0.4,
            perimeters: 2.0,
            min_wall: 0.8,
            layer_height: 0.2,
            min_gap: 0.3,
            max_overhang_degrees: 45.0,
            bed: [256.0, 256.0, 256.0],
            bed_shape: BedShape::Rectangular,
            material: Material::PLA,
            build_direction: Vec3::Z,
            chord_gap_fraction: CHORD_GAP_FRACTION,
        }
    }
}

impl Profile {
    /// The settings of a named machine, or an error naming the ones there are.
    pub fn named(name: &str) -> Result<Profile, String> {
        match printers::printer(name) {
            Some(p) => Ok(p.profile()),
            None => Err(format!("unknown printer {name:?}; Cadpole knows {}", printers::printer_list())),
        }
    }

    /// A profile with `min_wall` derived from the nozzle and perimeter count
    /// rather than set directly.
    pub fn with_nozzle(nozzle: f64, perimeters: f64) -> Profile {
        Profile { nozzle, perimeters, min_wall: nozzle * perimeters, ..Profile::default() }
    }

    /// Refuse a profile no printer could have, naming the setting: a zero
    /// build direction, a nozzle, perimeter count or layer height that is not
    /// positive, a negative minimum wall or gap, an overhang limit outside
    /// 0..90 degrees, a bed that is not three positive sizes. A front end
    /// calls this once it has applied what the caller passed, so a mistyped
    /// value is an error rather than silently replaced by the default.
    pub fn validate(&self) -> Result<(), String> {
        let positive = |v: f64, what: &str| {
            if v.is_finite() && v > 0.0 { Ok(()) } else { Err(format!("{what} must be a positive number, got {v}")) }
        };
        let non_negative = |v: f64, what: &str| {
            if v.is_finite() && v >= 0.0 { Ok(()) } else { Err(format!("{what} must be 0 or more, got {v}")) }
        };
        positive(self.nozzle, "nozzle")?;
        positive(self.perimeters, "perimeters")?;
        positive(self.layer_height, "layer_height")?;
        non_negative(self.min_wall, "min_wall")?;
        non_negative(self.min_gap, "min_gap")?;
        non_negative(self.chord_gap_fraction, "chord_gap_fraction")?;
        let o = self.max_overhang_degrees;
        if !(o.is_finite() && (0.0..=90.0).contains(&o)) {
            return Err(format!("max_overhang must be between 0 and 90 degrees from vertical, got {o}"));
        }
        for (k, v) in self.bed.iter().enumerate() {
            positive(*v, ["bed x", "bed y", "bed z"][k])?;
        }
        let d = self.build_direction;
        if !(d.x.is_finite() && d.y.is_finite() && d.z.is_finite()) || d.length() == 0.0 {
            return Err(format!("build_direction must be a non-zero vector, got ({}, {}, {})", d.x, d.y, d.z));
        }
        Ok(())
    }

    /// The build direction, normalized, falling back to +Z if it is zero.
    pub fn up(&self) -> Vec3 {
        let u = self.build_direction.normalized();
        if u.length() < 0.5 { Vec3::Z } else { u }
    }
}

/// How bad one finding is. Printing checks are advice, so even `Error` never
/// stops an evaluation; it is what the UI paints red and what
/// `report.ok` reports as false.
#[derive(Clone, Copy, Debug, PartialEq, Eq, PartialOrd, Ord)]
pub enum Severity {
    Info,
    Warning,
    Error,
}

impl Severity {
    pub fn as_str(self) -> &'static str {
        match self {
            Severity::Info => "info",
            Severity::Warning => "warning",
            Severity::Error => "error",
        }
    }
}

/// Which check produced a finding. Stable strings: the CLI, the UI and the
/// assistant all key off them.
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub enum Check {
    /// The verdict of `cadpole_core::solid::validate`: an error saying "not a
    /// solid" and why, or an info line counting the shells.
    Solid,
    Watertight,
    NonManifold,
    Orientation,
    SelfIntersection,
    /// Two coplanar triangles lying on top of each other: a face emitted
    /// twice, or the wall two fused parts still share.
    InternalFace,
    /// Two adjacent triangles facing opposite ways: a zero-thickness fin or
    /// pleat. A warning, not an error — the surface is still a solid — but
    /// the mesh kernel's self-intersection test fires on it, and the next
    /// boolean on the body may then lose its face names.
    Fold,
    Degenerate,
    Thickness,
    Overhang,
    BedFit,
    /// Two bodies printed together are closer than `Profile::min_gap`, or
    /// touch, or overlap: they come off the printer as one part
    /// ([`assembly`]).
    Fuse,
    /// A horizontal gap between two bodies is not a whole number of layers,
    /// so the slicer rounds it, and the smaller rounding may fuse.
    LayerGap,
    /// A body that does not touch the bed has nothing under its lowest
    /// face: it prints in the air.
    Bridge,
    /// Flat downward faces held up at both ends within `max_bridge`: a
    /// slicer bridges them, so they are not counted as overhang (
    /// [`overhang`]). Always information.
    Bridged,
    /// The export mesh sits farther off the body's exact surfaces than the
    /// profile's fraction of its gap to another body ([`chord`]).
    /// Always a warning: the file is still written.
    ChordError,
}

impl Check {
    pub fn as_str(self) -> &'static str {
        match self {
            Check::Solid => "solid",
            Check::Watertight => "watertight",
            Check::NonManifold => "non_manifold",
            Check::Orientation => "orientation",
            Check::SelfIntersection => "self_intersection",
            Check::InternalFace => "internal_face",
            Check::Fold => "fold",
            Check::Degenerate => "degenerate",
            Check::Thickness => "thickness",
            Check::Overhang => "overhang",
            Check::BedFit => "bed_fit",
            Check::Fuse => "fuse",
            Check::LayerGap => "layer_gap",
            Check::Bridge => "bridge",
            Check::Bridged => "bridged",
            Check::ChordError => "chord_error",
        }
    }
}

/// One finding, with the place to look.
#[derive(Clone, Debug)]
pub struct Issue {
    pub check: Check,
    pub severity: Severity,
    /// One line, already carrying its numbers, e.g.
    /// `"wall is 0.62 mm at (12.5, 0, 8), below the 0.8 mm minimum"`.
    pub message: String,
    /// Where in model space to look. `None` only for findings about the whole
    /// model (bed fit, inside-out normals).
    pub at: Option<Vec3>,
    /// The measured number the message quotes: mm for thickness and bed
    /// excess, degrees for overhangs, mm² for areas, a count otherwise.
    pub value: f64,
}

impl Issue {
    fn new(check: Check, severity: Severity, message: impl Into<String>, at: Option<Vec3>, value: f64) -> Issue {
        Issue { check, severity, message: message.into(), at, value }
    }
}

/// Everything the checks found, plus the measurements they took on the way.
#[derive(Clone, Debug)]
pub struct Report {
    pub triangles: usize,
    /// Enclosed volume in mm³ (negative when the mesh is inside out).
    pub volume_mm3: f64,
    pub area_mm2: f64,
    pub bbox: Option<(Vec3, Vec3)>,
    pub mesh: MeshReport,
    pub thickness: ThicknessReport,
    pub overhang: OverhangReport,
    pub bed: BedReport,
    /// Every finding, worst first.
    pub issues: Vec<Issue>,
    /// The profile the checks judged this against, so a report can say what
    /// machine it is talking about.
    pub profile_name: String,
    /// Solid mass in grams at the profile's material density. A solid
    /// estimate, not a slicer estimate: infill and perimeters make the real
    /// number smaller, so this is the upper bound.
    pub mass_g: f64,
    /// How many layers tall the part is at the profile's layer height,
    /// measured along the build direction.
    pub layers: usize,
}

impl Report {
    /// True when nothing was flagged at `Error`. Warnings (thin walls,
    /// overhangs) still print.
    pub fn ok(&self) -> bool {
        !self.issues.iter().any(|i| i.severity == Severity::Error)
    }

    pub fn errors(&self) -> impl Iterator<Item = &Issue> {
        self.issues.iter().filter(|i| i.severity == Severity::Error)
    }

    pub fn warnings(&self) -> impl Iterator<Item = &Issue> {
        self.issues.iter().filter(|i| i.severity == Severity::Warning)
    }

    /// Add a finding made outside [`check`] — the chord-error-vs-gap rule,
    /// which needs the other bodies — keeping the worst-first order.
    pub fn push_issue(&mut self, issue: Issue) {
        self.issues.push(issue);
        sort_issues(&mut self.issues);
    }

    /// A one-line summary for a CLI or a chat reply.
    pub fn summary(&self) -> String {
        let e = self.errors().count();
        let w = self.warnings().count();
        format!(
            "{} triangles, {:.1} cm³, {}{}{}",
            self.triangles,
            self.volume_mm3 / 1000.0,
            if self.mesh.solid {
                "solid"
            } else if self.mesh.watertight {
                "watertight but NOT a solid"
            } else {
                "NOT watertight"
            },
            if e > 0 { format!(", {e} error{}", plural(e)) } else { String::new() },
            if w > 0 { format!(", {w} warning{}", plural(w)) } else { String::new() },
        )
    }
}

fn plural(n: usize) -> &'static str {
    if n == 1 { "" } else { "s" }
}

/// Worst first, then by the size of the number so the biggest hole or the
/// thinnest wall leads.
fn sort_issues(issues: &mut [Issue]) {
    issues.sort_by(|a, b| {
        b.severity.cmp(&a.severity).then(b.value.partial_cmp(&a.value).unwrap_or(std::cmp::Ordering::Equal))
    });
}

/// Run every check against one mesh.
///
/// The mesh is welded once (at [`WELD_EPS`]) for the topology checks, because
/// a tessellator emits each face's triangles with its own vertices and an
/// unwelded mesh has a boundary edge at every face seam. The thickness,
/// overhang and bed checks run on the mesh **as given**, so the per-triangle
/// fields line up with `mesh.indices` for the analysis views.
pub fn check(mesh: &Mesh, profile: &Profile) -> Report {
    check_with(mesh, profile, overhang::DEFAULT_MAX_BRIDGE)
}

/// [`check`], with the longest bridge a flat downward face held up at both
/// ends may span, in mm, before it counts as an overhang; 0 or less
/// counts every such face. Not a [`Profile`] field because the printer table
/// builds profiles field by field and the rule is not a machine's.
pub fn check_with(mesh: &Mesh, profile: &Profile, max_bridge: f64) -> Report {
    let mut issues = Vec::new();
    let welded = mesh.welded(WELD_EPS);

    let mesh_report = mesh_checks::check(mesh, &welded, &mut issues);
    let thickness = thickness::check(mesh, profile, &mut issues);
    let overhang = overhang::check_bridged(mesh, profile, None, max_bridge, &mut issues);
    let bed = bed::check(mesh, profile, &mut issues);

    sort_issues(&mut issues);

    let volume_mm3 = mesh.volume();
    // `BedReport::size` is measured in the printer's frame, so its third
    // component is the height along the build direction whatever `up` is.
    let height = bed.size.z;
    Report {
        triangles: mesh.triangle_count(),
        volume_mm3,
        area_mm2: mesh.area(),
        bbox: mesh.bbox(),
        mesh: mesh_report,
        thickness,
        overhang,
        bed,
        issues,
        profile_name: profile.name.clone(),
        mass_g: profile.material.mass_g(volume_mm3.abs()),
        layers: if profile.layer_height > 0.0 { (height / profile.layer_height).ceil() as usize } else { 0 },
    }
}

// ---------------------------------------------------------------------------
// Shared helpers
// ---------------------------------------------------------------------------

/// The three corners of triangle `t`.
pub(crate) fn tri(mesh: &Mesh, t: usize) -> [Vec3; 3] {
    let i = t * 3;
    let p = |k: usize| {
        let v = mesh.indices[i + k] as usize * 3;
        Vec3::new(mesh.positions[v] as f64, mesh.positions[v + 1] as f64, mesh.positions[v + 2] as f64)
    };
    [p(0), p(1), p(2)]
}

/// Outward unit normal of triangle `t` from its winding, and its area.
/// A degenerate triangle gives `(Vec3::ZERO, 0.0)`.
pub(crate) fn tri_normal_area(v: &[Vec3; 3]) -> (Vec3, f64) {
    let n = (v[1] - v[0]).cross(v[2] - v[0]);
    let l = n.length();
    if l <= 0.0 || !l.is_finite() { (Vec3::ZERO, 0.0) } else { (n * (1.0 / l), l / 2.0) }
}

pub(crate) fn centroid(v: &[Vec3; 3]) -> Vec3 {
    (v[0] + v[1] + v[2]) * (1.0 / 3.0)
}

/// Longest diagonal of the mesh's bounding box, or 1 mm for an empty mesh.
/// Used to scale every "how close is close" epsilon so the checks behave the
/// same on a 4 mm pin and a 400 mm planter.
pub(crate) fn scale_of(mesh: &Mesh) -> f64 {
    match mesh.bbox() {
        Some((lo, hi)) => (hi - lo).length().max(1e-6),
        None => 1.0,
    }
}

/// Pick at most `max` representative spots out of `(position, value)` pairs:
/// worst value first, and never two closer together than `radius`. Without
/// this a thin wall reports once per triangle and buries everything else.
pub(crate) fn cluster(mut spots: Vec<(Vec3, f64)>, max: usize, radius: f64, worst_is_low: bool) -> Vec<(Vec3, f64)> {
    spots.sort_by(|a, b| {
        if worst_is_low {
            a.1.partial_cmp(&b.1).unwrap_or(std::cmp::Ordering::Equal)
        } else {
            b.1.partial_cmp(&a.1).unwrap_or(std::cmp::Ordering::Equal)
        }
    });
    let r2 = radius * radius;
    let mut out: Vec<(Vec3, f64)> = Vec::new();
    for (p, v) in spots {
        if out.len() >= max {
            break;
        }
        if out.iter().any(|(q, _)| (p - *q).dot(p - *q) < r2) {
            continue;
        }
        out.push((p, v));
    }
    out
}

/// `(12.5, 0, 8)` — positions inside diagnostic messages, 2 decimals, no
/// trailing zeros, so the text is stable enough for a golden test.
pub(crate) fn fmt_at(p: Vec3) -> String {
    format!("({}, {}, {})", num(p.x), num(p.y), num(p.z))
}

pub(crate) fn num(v: f64) -> String {
    let s = format!("{:.2}", v);
    let s = s.trim_end_matches('0').trim_end_matches('.').to_owned();
    if s == "-0" { "0".to_owned() } else { s }
}

#[cfg(test)]
pub(crate) mod testing;

#[cfg(test)]
mod tests {
    use super::*;
    use crate::testing::*;

    #[test]
    fn a_solid_box_passes_every_check() {
        let m = box_mesh(20.0, 20.0, 20.0);
        let r = check(&m, &Profile::default());
        assert!(r.mesh.watertight, "a box must be watertight");
        assert!(r.mesh.solid, "and a solid");
        assert!(r.ok(), "unexpected errors: {:?}", r.errors().collect::<Vec<_>>());
        assert!((r.volume_mm3 - 8000.0).abs() < 1e-6, "volume {}", r.volume_mm3);
        assert!((r.area_mm2 - 2400.0).abs() < 1e-6, "area {}", r.area_mm2);
    }

    #[test]
    fn summary_counts_errors_and_warnings() {
        let m = box_mesh(10.0, 10.0, 10.0);
        let r = check(&m, &Profile::default());
        assert!(r.summary().contains("solid"), "{}", r.summary());
        assert!(!r.summary().contains("NOT"), "{}", r.summary());
    }

    #[test]
    fn cluster_keeps_the_worst_and_spreads_out() {
        let spots = vec![
            (Vec3::new(0.0, 0.0, 0.0), 0.2),
            (Vec3::new(0.1, 0.0, 0.0), 0.3), // within the radius of the first
            (Vec3::new(50.0, 0.0, 0.0), 0.5),
        ];
        let out = cluster(spots, 4, 1.0, true);
        assert_eq!(out.len(), 2);
        assert!((out[0].1 - 0.2).abs() < 1e-9);
        assert!((out[1].1 - 0.5).abs() < 1e-9);
    }

    #[test]
    fn a_degenerate_profile_value_is_an_error_not_a_default() {
        assert!(Profile::default().validate().is_ok());
        for p in printers::printer_ids() {
            Profile::named(p).unwrap().validate().unwrap_or_else(|e| panic!("{p}: {e}"));
        }
        let cases: [(&str, Profile); 7] = [
            ("build_direction", Profile { build_direction: Vec3::ZERO, ..Profile::default() }),
            ("min_wall", Profile { min_wall: -1.0, ..Profile::default() }),
            ("nozzle", Profile { nozzle: 0.0, ..Profile::default() }),
            ("max_overhang", Profile { max_overhang_degrees: -10.0, ..Profile::default() }),
            ("layer_height", Profile { layer_height: 0.0, ..Profile::default() }),
            ("perimeters", Profile { perimeters: -2.0, ..Profile::default() }),
            ("min_gap", Profile { min_gap: f64::NAN, ..Profile::default() }),
        ];
        for (what, p) in cases {
            let e = p.validate().expect_err(what);
            assert!(e.contains(what), "{what}: {e}");
        }
    }

    #[test]
    fn num_never_prints_negative_zero() {
        assert_eq!(num(-0.001), "0");
        assert_eq!(num(12.5), "12.5");
        assert_eq!(num(8.0), "8");
    }
}
