//! The mesh side: STL, OBJ, 3MF and glTF into mesh bodies with a report
//! that measures rather than mends.
//!
//! Each format's parser ([`crate::stl`], [`crate::obj`], [`crate::threemf`],
//! [`crate::gltf`]) hands back [`RawObject`]s — positions, triangles, the
//! file's own groups, per-vertex colours and a material's appearance when
//! the file has them — and everything after that is shared: the file's
//! frame turned into Cadpole's (+Z up, millimetres; [`Up`], [`Unit`]), the
//! group order, the mesh, the optional weld, the verdict of
//! `cadpole_core::solid` and the [`Report`] the feature records. The rule the whole module keeps is **never change the
//! shape**: under [`Repair::Report`] (the default) the body is the file's
//! triangles exactly as written, a soup if the file is a soup, and the
//! report says what `cadpole_core::solid` found when it welded a copy to look;
//! [`Repair::Weld`] merges coincident vertices (the one repair the mesh
//! kernel itself performs on the way in) and turns a closed,
//! consistently wound but inside-out mesh outward; [`Repair::Off`] skips
//! the pairwise crossing search on a mesh too big to want it. Nothing here
//! fills a hole, drops a fold or flips one triangle: those are changes
//! a mesh-repair tool makes and Cadpole does not.
//!
//! The report reaches the script two ways: as diagnostics on the feature
//! (an info summary, warnings for what stops the body being a solid) and
//! as feature args `report.*`, which the memo replays on a hit, so
//! `import_report(body)` reads the same numbers whether or not the file
//! was parsed this evaluation.

use std::collections::BTreeMap;
use std::sync::Arc;

use cadpole_core::geom::Vec3;
use cadpole_core::mesh_kernel::WELD_EPS;
use cadpole_core::solid::{self, SolidOptions, SolidReport};
use cadpole_core::tessellate::{FaceInfo, Mesh, Topology};

use crate::prescan::{self, Format, MAX_TRIANGLES, MAX_VERTICES};
use crate::{Note, bounded, gltf, label, obj, stl, threemf};

/// The length unit `import_mesh(units=)` names: the one vocabulary every
/// importer's `units=` reads (`import_dxf`'s `dxf_units`). `Auto`
/// is the format's own convention.
pub use crate::dxf::Unit;

/// Which way is up in a mesh file (`import_mesh(up=)`). Cadpole is +Z
/// up; glTF is +Y up by its specification, and so is much of what a phone
/// or a game engine writes.
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub enum Up {
    /// +Y is up: turned to +Z by (x, y, z) → (x, −z, y), a quarter turn
    /// about +X, so the file's +Z (toward a glTF viewer) becomes Cadpole's
    /// −Y (toward the front view's eye) and the winding is kept.
    Y,
    /// +Z is up: taken as it is.
    Z,
}

impl Up {
    /// Every member, in the order the documentation lists them.
    pub const ALL: [Up; 2] = [Up::Y, Up::Z];

    pub fn name(self) -> &'static str {
        match self {
            Up::Y => "y",
            Up::Z => "z",
        }
    }

    /// `"y"` or `"z"`, in either case, with an optional `+`. Empty means
    /// the format's own.
    pub fn parse(s: &str) -> Result<Option<Up>, String> {
        match s.trim().trim_start_matches('+').to_ascii_lowercase().as_str() {
            "" | "auto" => Ok(None),
            "y" => Ok(Some(Up::Y)),
            "z" => Ok(Some(Up::Z)),
            other => Err(format!(
                "up={other:?} is not an axis Cadpole turns from; valid: y (the file is +Y up, as glTF is), z (+Z up, as Cadpole is)"
            )),
        }
    }
}

/// The files a `.gltf` names beside itself — its external `.bin` buffers —
/// by asset name: the caller looks each one up in the document's
/// asset table, so the importer never touches a path.
#[derive(Clone, Default)]
pub struct Resources(pub BTreeMap<String, Arc<[u8]>>);

impl std::fmt::Debug for Resources {
    fn fmt(&self, f: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
        f.debug_map().entries(self.0.iter().map(|(k, v)| (k, format!("{} bytes", v.len())))).finish()
    }
}

impl Resources {
    pub fn get(&self, name: &str) -> Option<&Arc<[u8]>> {
        self.0.get(name)
    }
}

/// What `import_mesh(repair=)` may do to the triangles.
#[derive(Clone, Copy, Debug, PartialEq, Eq, Default)]
pub enum Repair {
    /// Measure and say; change nothing. The default.
    #[default]
    Report,
    /// Merge coincident vertices, and turn the whole mesh outward when it is
    /// closed, consistently wound and inside out.
    Weld,
    /// Neither weld nor run the pairwise crossing search.
    Off,
}

impl Repair {
    /// Every member, in the order the `repair` vocabulary lists them.
    pub const ALL: [Repair; 3] = [Repair::Report, Repair::Weld, Repair::Off];

    pub fn name(self) -> &'static str {
        match self {
            Repair::Report => "report",
            Repair::Weld => "weld",
            Repair::Off => "off",
        }
    }

    /// The vocabulary's spellings and nothing else. Empty means the
    /// default.
    pub fn parse(s: &str) -> Result<Repair, String> {
        match s {
            "" | "report" => Ok(Repair::Report),
            "weld" => Ok(Repair::Weld),
            "off" => Ok(Repair::Off),
            other => Err(format!("unknown repair {other:?}; valid: report, weld, off")),
        }
    }
}

/// What [`import`] takes beside the bytes.
#[derive(Clone, Debug)]
pub struct Options {
    pub repair: Repair,
    /// The document tolerance, in mm: the distance of the second weld pass,
    /// tried only when the first (at `WELD_EPS`) left open edges.
    pub weld_tolerance: f64,
    /// The feature id the group names are minted under: `<feature>/<group>`.
    pub feature: String,
    /// The wall-clock budget for parse and build, in milliseconds
    /// ([`crate::bounded`]).
    pub budget_ms: u64,
    /// The file's up axis; `None` is the format's own: +Y for glTF, +Z for
    /// STL, OBJ and 3MF.
    pub up: Option<Up>,
    /// The file's length unit; `None` (or [`Unit::Auto`]) is the format's
    /// own: metres for glTF, `<model unit>` for 3MF, millimetres for STL
    /// and OBJ, which declare none.
    pub units: Option<Unit>,
    /// What a `.gltf` names beside itself, by asset name.
    pub resources: Resources,
}

impl Default for Options {
    fn default() -> Options {
        Options {
            repair: Repair::Report,
            weld_tolerance: cadpole_core::kernel::DEFAULT_TOLERANCE,
            feature: "import_mesh".into(),
            budget_ms: crate::IMPORT_BUDGET_MS,
            up: None,
            units: None,
            resources: Resources::default(),
        }
    }
}

impl Options {
    /// The up axis `format` is read with.
    pub fn up_for(&self, format: Format) -> Up {
        self.up.unwrap_or(if format == Format::Gltf { Up::Y } else { Up::Z })
    }

    /// The unit `units=` names, `None` for the format's own.
    pub fn unit_override(&self) -> Option<Unit> {
        self.units.filter(|u| *u != Unit::Auto)
    }
}

/// How a body should look, from the material a file gave it (glTF's
/// `pbrMetallicRoughness`): what `Body.material(...)` would set.
#[derive(Clone, Debug, PartialEq)]
pub struct Appearance {
    /// The material it came from, as the file names it; empty when unnamed.
    pub material: String,
    /// Linear RGB, 0..1: the base colour factor, or white when the body's
    /// colour is per vertex (the factor is then in the vertex colours).
    pub base_color: [f64; 3],
    /// 0..1; below 1 only for a glTF `alphaMode: BLEND`.
    pub opacity: f64,
    pub roughness: f64,
    pub metallic: f64,
    /// Linear RGB added after lighting.
    pub emissive: [f64; 3],
}

impl Default for Appearance {
    fn default() -> Appearance {
        Appearance {
            material: String::new(),
            base_color: [1.0; 3],
            opacity: 1.0,
            roughness: 1.0,
            metallic: 0.0,
            emissive: [0.0; 3],
        }
    }
}

fn rgb_arg(c: [f64; 3]) -> String {
    format!("{} {} {}", c[0], c[1], c[2])
}

impl Appearance {
    /// The appearance as feature args under `prefix` (`look[k].`), which
    /// the memo replays, so a hit colours the body as the run did.
    pub fn to_args(&self, prefix: &str) -> Vec<(String, String)> {
        vec![
            (format!("{prefix}material"), self.material.clone()),
            (format!("{prefix}base_color"), rgb_arg(self.base_color)),
            (format!("{prefix}opacity"), self.opacity.to_string()),
            (format!("{prefix}roughness"), self.roughness.to_string()),
            (format!("{prefix}metallic"), self.metallic.to_string()),
            (format!("{prefix}emissive"), rgb_arg(self.emissive)),
        ]
    }

    /// The appearance back from a feature's args; `None` when none was
    /// recorded under `prefix`.
    pub fn from_args(args: &BTreeMap<String, String>, prefix: &str) -> Option<Appearance> {
        let get = |k: &str| args.get(&format!("{prefix}{k}")).map(|s| s.as_str());
        let base_color = get("base_color").and_then(parse_pos)?;
        let f = |k: &str, d: f64| get(k).and_then(|s| s.parse::<f64>().ok()).unwrap_or(d);
        Some(Appearance {
            material: get("material").unwrap_or("").to_owned(),
            base_color,
            opacity: f("opacity", 1.0),
            roughness: f("roughness", 1.0),
            metallic: f("metallic", 0.0),
            emissive: get("emissive").and_then(parse_pos).unwrap_or([0.0; 3]),
        })
    }
}

/// One object of a file, as its parser read it: the body-to-be.
#[derive(Clone, Debug, Default)]
pub struct RawObject {
    /// The file's own name for it; empty when it gave none.
    pub name: String,
    pub positions: Vec<[f64; 3]>,
    pub triangles: Vec<[u32; 3]>,
    /// The group of each triangle, an index into `groups`. Empty means every
    /// triangle is in the one group.
    pub tri_group: Vec<u32>,
    /// The file's own groups (OBJ `g`, a Bambu object's parts, a glTF
    /// primitive's material), raw.
    pub groups: Vec<String>,
    /// What a group is called when its label sanitises to nothing, by
    /// group: `material[k]` for a glTF material without a name. Empty (or
    /// an empty entry) means `group[j]`, by position.
    pub group_fallbacks: Vec<String>,
    /// One colour per position, sRGB-encoded 8-bit RGB; empty when the file
    /// gave none.
    pub colors: Vec<[u8; 3]>,
    /// How the body looks, when the file says (a glTF material).
    pub appearance: Option<Appearance>,
}

/// Everything the import measured about one body. Every count is on the
/// mesh `cadpole_core::solid` judged: the body's own triangles, welded at the
/// mesh kernel's distance so "closed" means what it means to `to_manifold`.
#[derive(Clone, Debug, Default, PartialEq)]
pub struct Report {
    pub format: String,
    /// Triangles and vertices as the file had them.
    pub triangles_in: usize,
    pub vertices_in: usize,
    /// Triangles and vertices the body carries.
    pub triangles: usize,
    pub vertices: usize,
    /// Groups (the body's named faces).
    pub groups: usize,
    /// Vertices merged by the weld (`repair="weld"` only).
    pub welded: usize,
    /// Which weld pass the body kept: 0 none, 1 at `WELD_EPS`, 2 at the
    /// document tolerance.
    pub weld_pass: u8,
    /// Triangles that collapsed to a line or a point when welded.
    pub collapsed: usize,
    /// Triangles still without area after the weld.
    pub degenerate: usize,
    /// Connected components of the surface.
    pub shells: usize,
    /// Edges used by exactly one triangle.
    pub open_edges: usize,
    /// Edges used by three or more triangles.
    pub non_manifold_edges: usize,
    /// Edges whose two triangles walk them the same way.
    pub flipped_edges: usize,
    /// Pairs of triangles that pass through each other.
    pub crossings: usize,
    /// Pairs of coplanar triangles that share area.
    pub overlaps: usize,
    /// The pairwise search was skipped (`repair="off"`, or over the pair
    /// limit): `crossings` and `overlaps` are unknown, not zero.
    pub pairs_skipped: bool,
    /// Adjacent triangle pairs facing opposite ways.
    pub folds: usize,
    pub closed: bool,
    /// Closed, every edge's two triangles wound against each other, and
    /// the normals pointing out of the material.
    pub oriented: bool,
    /// Closed and consistently wound, but inside out (`oriented` is then
    /// false); `repair="weld"` turns such a mesh outward.
    pub inverted: bool,
    /// Closed, oriented and free of crossings and overlaps.
    pub solid: bool,
    /// Only for a closed surface.
    pub genus: Option<i64>,
    /// Signed volume, only when closed and oriented; positive means the
    /// normals point out of the material.
    pub volume: Option<f64>,
    pub area: f64,
    pub bbox: Option<([f64; 3], [f64; 3])>,
    /// The whole mesh was inside out and was turned (`repair="weld"`).
    pub flipped_to_outward: bool,
    /// Where to look: the midpoint of the first open edge, the first
    /// crossing, the worst fold.
    pub open_at: Option<[f64; 3]>,
    pub crossing_at: Option<[f64; 3]>,
    pub fold_at: Option<[f64; 3]>,
    /// The file's own name for the body (a glTF node or mesh, a 3MF
    /// object, an OBJ `o`), sanitised; `object[k]` when it gave none.
    pub object: String,
    /// The body carries a colour per vertex.
    pub vertex_colors: bool,
}

fn pos_arg(p: [f64; 3]) -> String {
    format!("{} {} {}", p[0], p[1], p[2])
}

fn parse_pos(s: &str) -> Option<[f64; 3]> {
    let v: Vec<f64> = s.split_whitespace().filter_map(|t| t.parse().ok()).collect();
    (v.len() == 3).then(|| [v[0], v[1], v[2]])
}

impl Report {
    /// The report as feature args, `<prefix><field>` (a feature with
    /// several bodies uses `report[k].`): how it is recorded on the feature
    /// and replayed by the memo.
    pub fn to_args(&self, prefix: &str) -> Vec<(String, String)> {
        let mut a: Vec<(String, String)> = Vec::new();
        let mut put = |k: &str, v: String| a.push((format!("{prefix}{k}"), v));
        put("format", self.format.clone());
        put("triangles_in", self.triangles_in.to_string());
        put("vertices_in", self.vertices_in.to_string());
        put("triangles", self.triangles.to_string());
        put("vertices", self.vertices.to_string());
        put("groups", self.groups.to_string());
        put("welded", self.welded.to_string());
        put("weld_pass", self.weld_pass.to_string());
        put("collapsed", self.collapsed.to_string());
        put("degenerate", self.degenerate.to_string());
        put("shells", self.shells.to_string());
        put("open_edges", self.open_edges.to_string());
        put("non_manifold_edges", self.non_manifold_edges.to_string());
        put("flipped_edges", self.flipped_edges.to_string());
        put("crossings", self.crossings.to_string());
        put("overlaps", self.overlaps.to_string());
        put("pairs_skipped", self.pairs_skipped.to_string());
        put("folds", self.folds.to_string());
        put("closed", self.closed.to_string());
        put("oriented", self.oriented.to_string());
        put("inverted", self.inverted.to_string());
        put("solid", self.solid.to_string());
        if let Some(g) = self.genus {
            put("genus", g.to_string());
        }
        if let Some(v) = self.volume {
            put("volume", v.to_string());
        }
        put("area", self.area.to_string());
        if let Some((lo, hi)) = self.bbox {
            put("bbox_min", pos_arg(lo));
            put("bbox_max", pos_arg(hi));
        }
        put("flipped_to_outward", self.flipped_to_outward.to_string());
        if let Some(p) = self.open_at {
            put("open_at", pos_arg(p));
        }
        if let Some(p) = self.crossing_at {
            put("crossing_at", pos_arg(p));
        }
        if let Some(p) = self.fold_at {
            put("fold_at", pos_arg(p));
        }
        put("object", self.object.clone());
        put("vertex_colors", self.vertex_colors.to_string());
        a
    }

    /// The report back from a feature's args under `prefix`; `None` when
    /// the feature recorded none there.
    pub fn from_args(args: &BTreeMap<String, String>, prefix: &str) -> Option<Report> {
        let get = |k: &str| args.get(&format!("{prefix}{k}")).map(|s| s.as_str());
        let format = get("format")?.to_owned();
        let n = |k: &str| get(k).and_then(|s| s.parse::<usize>().ok()).unwrap_or(0);
        let b = |k: &str| get(k) == Some("true");
        let f = |k: &str| get(k).and_then(|s| s.parse::<f64>().ok());
        let bbox = match (get("bbox_min").and_then(parse_pos), get("bbox_max").and_then(parse_pos)) {
            (Some(lo), Some(hi)) => Some((lo, hi)),
            _ => None,
        };
        Some(Report {
            format,
            triangles_in: n("triangles_in"),
            vertices_in: n("vertices_in"),
            triangles: n("triangles"),
            vertices: n("vertices"),
            groups: n("groups"),
            welded: n("welded"),
            weld_pass: n("weld_pass") as u8,
            collapsed: n("collapsed"),
            degenerate: n("degenerate"),
            shells: n("shells"),
            open_edges: n("open_edges"),
            non_manifold_edges: n("non_manifold_edges"),
            flipped_edges: n("flipped_edges"),
            crossings: n("crossings"),
            overlaps: n("overlaps"),
            pairs_skipped: b("pairs_skipped"),
            folds: n("folds"),
            closed: b("closed"),
            oriented: b("oriented"),
            inverted: b("inverted"),
            solid: b("solid"),
            genus: get("genus").and_then(|s| s.parse().ok()),
            volume: f("volume"),
            area: f("area").unwrap_or(0.0),
            bbox,
            flipped_to_outward: b("flipped_to_outward"),
            open_at: get("open_at").and_then(parse_pos),
            crossing_at: get("crossing_at").and_then(parse_pos),
            fold_at: get("fold_at").and_then(parse_pos),
            object: get("object").unwrap_or("").to_owned(),
            vertex_colors: b("vertex_colors"),
        })
    }

    /// One line for `cadpole inspect` and the info diagnostic.
    pub fn summary(&self) -> String {
        let verdict = if self.solid {
            "a solid".to_owned()
        } else if !self.closed {
            format!(
                "not closed ({} open edge{}{})",
                self.open_edges,
                if self.open_edges == 1 { "" } else { "s" },
                if self.non_manifold_edges > 0 {
                    format!(", {} non-manifold", self.non_manifold_edges)
                } else {
                    String::new()
                }
            )
        } else if self.inverted {
            "closed but inside out".to_owned()
        } else if !self.oriented {
            format!("closed but not consistently wound ({} flipped edges)", self.flipped_edges)
        } else if self.pairs_skipped {
            "closed and oriented; crossings not searched".to_owned()
        } else {
            format!("closed but self-intersecting ({} crossings, {} overlaps)", self.crossings, self.overlaps)
        };
        let mut s = format!(
            "{}: {} triangle{}, {} vert{}, {} shell{}, {} group{}; {verdict}",
            self.format,
            self.triangles,
            if self.triangles == 1 { "" } else { "s" },
            self.vertices,
            if self.vertices == 1 { "ex" } else { "ices" },
            self.shells,
            if self.shells == 1 { "" } else { "s" },
            self.groups,
            if self.groups == 1 { "" } else { "s" },
        );
        if self.welded > 0 {
            s.push_str(&format!(
                "; welded {} vert{} (pass {})",
                self.welded,
                if self.welded == 1 { "ex" } else { "ices" },
                self.weld_pass
            ));
        }
        if self.collapsed > 0 {
            s.push_str(&format!("; {} collapsed", self.collapsed));
        }
        if self.flipped_to_outward {
            s.push_str("; turned outward");
        }
        if let Some(v) = self.volume {
            s.push_str(&format!("; volume {v:.3} mm3"));
        }
        s
    }

    /// What the feature says about the body: the summary as info, one
    /// warning per thing that stops it being a solid, and the chord-error
    /// line every export reads.
    pub fn diagnostics(&self) -> Vec<Note> {
        self.diagnostics_under(Repair::Report)
    }

    /// [`Report::diagnostics`] for the repair the import was asked for.
    /// Under `repair=off` the same findings are information, not warnings:
    /// a mesh read without repair is reference geometry — a room
    /// scan is an open surface by nature — and a warning on every load of
    /// one says nothing its user does not know. Booleans and `cadpole check`
    /// still refuse a non-solid, with their own errors.
    pub fn diagnostics_under(&self, repair: Repair) -> Vec<Note> {
        let mut out = self.diagnostics_as_warnings();
        if repair == Repair::Off {
            for n in &mut out {
                n.level = crate::Level::Info;
            }
        }
        out
    }

    fn diagnostics_as_warnings(&self) -> Vec<Note> {
        let at = |p: Option<[f64; 3]>| match p {
            Some(p) => format!(" at ({:.3}, {:.3}, {:.3})", p[0], p[1], p[2]),
            None => String::new(),
        };
        let mut out = vec![Note::info(self.summary())];
        if !self.closed {
            out.push(Note::warning(format!(
                "the mesh is not closed: {} open edge{}{}{}; it renders and exports, but it is not a solid, \
                 so booleans and cadpole check will refuse it (a missing bottom is for you to close, not for \
                 the importer to guess)",
                self.open_edges,
                if self.open_edges == 1 { "" } else { "s" },
                if self.non_manifold_edges > 0 {
                    format!(" and {} non-manifold", self.non_manifold_edges)
                } else {
                    String::new()
                },
                at(self.open_at)
            )));
        } else if self.inverted {
            out.push(Note::warning(
                "the mesh is closed but inside out (every normal points into the material); repair=\"weld\" \
                 turns it outward, or fix the file's winding",
            ));
        } else if !self.oriented {
            out.push(Note::warning(format!(
                "the mesh is closed but not consistently wound: {} edge{} walked the same way by both \
                 triangles; it is not a solid until the file's winding is fixed",
                self.flipped_edges,
                if self.flipped_edges == 1 { " is" } else { "s are" }
            )));
        }
        if self.crossings > 0 {
            out.push(Note::warning(format!(
                "{} triangle pair{} cross{}{}; as a boolean operand the body takes the mesh kernel's soup \
                 path and loses face provenance, and cadpole check will call it unprintable there",
                self.crossings,
                if self.crossings == 1 { "" } else { "s" },
                if self.crossings == 1 { "es" } else { "" },
                at(self.crossing_at)
            )));
        }
        if self.folds > 0 {
            out.push(Note::warning(format!(
                "{} fold{} (adjacent triangles facing opposite ways){}; a scan's doubled sheet, reported \
                 and left as it is",
                self.folds,
                if self.folds == 1 { "" } else { "s" },
                at(self.fold_at)
            )));
        }
        out.push(Note::info(
            "chord error: not measurable on an imported mesh — the triangles are the geometry",
        ));
        out
    }
}

/// One body an import produced.
#[derive(Clone, Debug)]
pub struct ImportedBody {
    /// The file's own name for it, sanitised; `object[k]` when it gave none.
    pub name: String,
    pub mesh: Mesh,
    pub topo: Topology,
    pub report: Report,
    /// How it should look, when the file says (a glTF material).
    pub appearance: Option<Appearance>,
}

/// What an import produced.
#[derive(Clone, Debug)]
pub struct Import {
    pub format: Format,
    pub bodies: Vec<ImportedBody>,
    pub notes: Vec<Note>,
}

/// Read a mesh file under the pre-scan and the deadline. `name_hint` is the
/// asset's name: the tie-breaker `cadpole_assets::sniff` already uses, and the
/// stem an unnamed single object is called.
pub fn import(bytes: &[u8], name_hint: &str, opts: &Options) -> Result<Import, String> {
    import_shared(Arc::from(bytes), name_hint, opts)
}

/// [`import`] of bytes the caller already shares — an asset store's own
/// `Arc` — so a large scan is not copied on its way to the parse thread.
pub fn import_shared(bytes: Arc<[u8]>, name_hint: &str, opts: &Options) -> Result<Import, String> {
    let format = prescan::format_of(name_hint, &bytes)?;
    if format == Format::Step {
        return Err("this is a STEP file; import_step reads it as an exact B-rep".into());
    }
    if format == Format::Dxf {
        return Err("this is a DXF drawing; import_dxf reads it".into());
    }
    let hint = name_hint.to_owned();
    let opts = opts.clone();
    match bounded::run(opts.budget_ms, move || convert(format, &bytes, &hint, &opts)) {
        Ok(r) => r,
        Err(stopped) => Err(format!("{} import {stopped}", format.name())),
    }
}

/// [`import`] without the deadline or the panic guard: for tests that want a
/// panic to be a failure with a backtrace.
pub fn import_unbounded(bytes: &[u8], name_hint: &str, opts: &Options) -> Result<Import, String> {
    let format = prescan::format_of(name_hint, bytes)?;
    if format == Format::Step {
        return Err("this is a STEP file; import_step reads it as an exact B-rep".into());
    }
    if format == Format::Dxf {
        return Err("this is a DXF drawing; import_dxf reads it".into());
    }
    convert(format, bytes, name_hint, opts)
}

/// The asset name's stem, sanitised: what an unnamed single object is called.
pub(crate) fn stem(hint: &str) -> String {
    let base = hint.rsplit(['/', '\\']).next().unwrap_or(hint);
    let stem = base.rsplit_once('.').map(|(s, _)| s).unwrap_or(base);
    label::sanitise(stem)
}

/// Millimetres per file unit and the note that says what was assumed or
/// asked for; 3MF's own unit is applied by its parser.
fn frame(format: Format, opts: &Options) -> (f64, Up, Option<String>) {
    let up = opts.up_for(format);
    let asked = opts.unit_override();
    let scale = match (format, asked) {
        (Format::ThreeMf, _) => 1.0,
        (_, Some(u)) => u.mm(),
        (Format::Gltf, None) => 1000.0,
        _ => 1.0,
    };
    let unit = match (format, asked) {
        (Format::ThreeMf, Some(u)) => format!("units={} (every model part's <model unit> replaced)", u.name()),
        (Format::ThreeMf, None) => "<model unit>".to_owned(),
        (_, Some(u)) => format!("units={}", u.name()),
        (Format::Gltf, None) => "metres, as glTF specifies".to_owned(),
        _ => "millimetres (the format declares no unit; units= says otherwise)".to_owned(),
    };
    let s = |v: f64| if v == 1.0 { String::new() } else { format!("{v}·") };
    let map = match up {
        Up::Y => format!("(x, y, z) → {}(x, −z, y)", s(scale)),
        Up::Z if scale != 1.0 => format!("(x, y, z) → {}(x, y, z)", s(scale)),
        Up::Z => String::new(),
    };
    let note = match (format, up, map.is_empty()) {
        (_, _, true) if asked.is_none() && opts.up.is_none() => None,
        _ => Some(format!(
            "{}: read as +{} up in {unit}; into Cadpole's +Z up in millimetres{}",
            format.name(),
            if up == Up::Y { "Y" } else { "Z" },
            if map.is_empty() { " as it is".to_owned() } else { format!(": {map}") }
        )),
    };
    (scale, up, note)
}

/// Turn one object from the file's frame into Cadpole's: scaled to
/// millimetres, and a quarter turn about +X for a +Y-up file. A rotation
/// keeps the winding, so nothing else changes.
fn to_cadpole_frame(o: &mut RawObject, scale: f64, up: Up) {
    if scale == 1.0 && up == Up::Z {
        return;
    }
    for p in &mut o.positions {
        let [x, y, z] = *p;
        *p = match up {
            Up::Y => [scale * x, -scale * z, scale * y],
            Up::Z => [scale * x, scale * y, scale * z],
        };
    }
}

fn convert(format: Format, bytes: &[u8], hint: &str, opts: &Options) -> Result<Import, String> {
    let (mut objects, mut notes) = match format {
        Format::Stl => stl::parse(bytes, hint)?,
        Format::Obj => obj::parse(bytes, hint)?,
        Format::ThreeMf => threemf::parse_with_unit(bytes, hint, opts.unit_override().map(Unit::mm))?,
        Format::Gltf => gltf::parse(bytes, hint, &opts.resources)?,
        Format::Step => return Err("this is a STEP file; import_step reads it as an exact B-rep".into()),
        Format::Dxf => return Err("this is a DXF drawing; import_dxf reads it".into()),
    };
    if objects.is_empty() {
        return Err(format!("the {} file holds no triangles", format.name()));
    }
    let (scale, up, frame_note) = frame(format, opts);
    for o in &mut objects {
        to_cadpole_frame(o, scale, up);
    }
    if let Some(n) = frame_note {
        notes.push(Note::info(n));
    }
    let single = objects.len() == 1;
    let names: Vec<String> = if format == Format::Gltf {
        // Named by the reader already — node, mesh, stem, sanitised, with
        // `[k]` on every body sharing a name — so they are taken
        // as they are.
        objects.iter().map(|o| o.name.clone()).collect()
    } else {
        let mut names: Vec<String> = objects.iter().map(|o| label::sanitise(&o.name)).collect();
        for (k, n) in names.iter_mut().enumerate() {
            if n.is_empty() {
                *n = if single { stem(hint) } else { String::new() };
            }
            if n.is_empty() {
                *n = format!("object[{k}]");
            }
        }
        label::dedupe(&mut names);
        names
    };
    let mut bodies = Vec::with_capacity(objects.len());
    for (o, name) in objects.into_iter().zip(names) {
        bodies.push(build_body(format, o, name, opts, &mut notes)?);
    }
    Ok(Import { format, bodies, notes })
}

/// A linear colour component, 0..1, as the 8-bit sRGB encoding a mesh's
/// vertex colours are stored in (IEC 61966-2-1).
pub fn linear_to_srgb8(c: f64) -> u8 {
    let c = if c.is_finite() { c.clamp(0.0, 1.0) } else { 0.0 };
    let s = if c <= 0.003_130_8 { 12.92 * c } else { 1.055 * cadpole_core::fmath::powf(c, 1.0 / 2.4) - 0.055 };
    (s * 255.0).round().clamp(0.0, 255.0) as u8
}

/// Area-weighted vertex normals over the mesh as it is: a soup gets each
/// triangle's own normal on its three private vertices (flat), a shared
/// vertex the weighted mean of its triangles (smooth); `Mesh::smooth_seams`
/// in the renderer decides what shades as a crease.
fn vertex_normals(mesh: &mut Mesh) {
    let nv = mesh.vertex_count();
    let mut acc = vec![Vec3::ZERO; nv];
    for t in mesh.indices.chunks_exact(3) {
        let p = |i: u32| {
            let i = i as usize * 3;
            Vec3::new(mesh.positions[i] as f64, mesh.positions[i + 1] as f64, mesh.positions[i + 2] as f64)
        };
        let (a, b, c) = (p(t[0]), p(t[1]), p(t[2]));
        let n = (b - a).cross(c - a);
        for &v in t {
            acc[v as usize] = acc[v as usize] + n;
        }
    }
    mesh.normals.clear();
    mesh.normals.reserve(nv * 3);
    for n in acc {
        let n = if n.length() > 0.0 { n.normalized() } else { Vec3::new(0.0, 0.0, 1.0) };
        mesh.normals.extend_from_slice(&[n.x as f32, n.y as f32, n.z as f32]);
    }
}

fn midpoint(mesh: &Mesh, a: u32, b: u32) -> [f64; 3] {
    let p = |i: u32| {
        let i = i as usize * 3;
        [mesh.positions[i] as f64, mesh.positions[i + 1] as f64, mesh.positions[i + 2] as f64]
    };
    let (p, q) = (p(a), p(b));
    [(p[0] + q[0]) / 2.0, (p[1] + q[1]) / 2.0, (p[2] + q[2]) / 2.0]
}

fn build_body(
    format: Format,
    o: RawObject,
    name: String,
    opts: &Options,
    notes: &mut Vec<Note>,
) -> Result<ImportedBody, String> {
    let nv = o.positions.len();
    let nt = o.triangles.len();
    if nt == 0 {
        return Err(format!("object {name:?} has no triangles"));
    }
    if nt > MAX_TRIANGLES {
        return Err(format!("object {name:?} has {nt} triangles, over the {MAX_TRIANGLES} limit"));
    }
    if nv > MAX_VERTICES {
        return Err(format!("object {name:?} has {nv} vertices, over the {MAX_VERTICES} limit"));
    }
    for (i, p) in o.positions.iter().enumerate() {
        if !(p[0].is_finite() && p[1].is_finite() && p[2].is_finite()) {
            return Err(format!("object {name:?}: vertex {i} is not finite"));
        }
        // The mesh stores single precision: a finite double past f32's
        // range (from the file, or from its transforms and unit) would be
        // stored as an infinity. Checked here, after the frame and every
        // transform, on the value that is kept.
        if p.iter().any(|&c| !(c as f32).is_finite()) {
            return Err(format!(
                "object {name:?}: vertex {i} at ({:?}, {:?}, {:?}) mm is beyond the largest single-precision \
                 coordinate a mesh holds ({:e})",
                p[0],
                p[1],
                p[2],
                f32::MAX
            ));
        }
    }
    for (t, tri) in o.triangles.iter().enumerate() {
        for &i in tri {
            if i as usize >= nv {
                return Err(format!("object {name:?}: triangle {t} refers to vertex {i}, but there are {nv}"));
            }
        }
    }
    if !o.colors.is_empty() && o.colors.len() != nv {
        return Err(format!("object {name:?}: {} vertex colours for {nv} vertices", o.colors.len()));
    }
    // Groups: the file's, sanitised, de-duplicated; one called `surface`
    // when the file has none (the alias for a mesh body's whole
    // surface).
    let mut groups: Vec<String> =
        if o.groups.is_empty() { vec!["surface".into()] } else { o.groups.iter().map(|g| label::sanitise(g)).collect() };
    for (j, g) in groups.iter_mut().enumerate() {
        if g.is_empty() {
            *g = match o.group_fallbacks.get(j) {
                Some(f) if !f.is_empty() => f.clone(),
                _ => format!("group[{j}]"),
            };
        }
    }
    label::dedupe(&mut groups);
    let tri_group: Vec<u32> = if o.tri_group.len() == nt { o.tri_group } else { vec![0; nt] };
    if let Some(&g) = tri_group.iter().find(|&&g| g as usize >= groups.len()) {
        return Err(format!("object {name:?}: a triangle is in group {g}, but there are {}", groups.len()));
    }
    // Triangles ordered by group, stably, so each group is one contiguous
    // run — what `FaceInfo::tri_start`/`tri_count` describe. The weld keeps
    // the order.
    let mut order: Vec<usize> = (0..nt).collect();
    order.sort_by_key(|&t| tri_group[t]);

    let mut mesh = Mesh::default();
    mesh.positions.reserve(nv * 3);
    for p in &o.positions {
        mesh.positions.extend_from_slice(&[p[0] as f32, p[1] as f32, p[2] as f32]);
    }
    mesh.indices.reserve(nt * 3);
    mesh.face_ids.reserve(nt);
    for &t in &order {
        mesh.indices.extend_from_slice(&o.triangles[t]);
        mesh.face_ids.push(tri_group[t]);
    }
    mesh.colors = o.colors.iter().flatten().copied().collect();
    vertex_normals(&mut mesh);

    let mut report = Report { format: format.name().into(), triangles_in: nt, vertices_in: nv, ..Default::default() };
    let judge = SolidOptions {
        weld_eps: WELD_EPS,
        pair_limit: if opts.repair == Repair::Off { 0 } else { SolidOptions::default().pair_limit },
    };
    // The verdict is always on a welded copy; under `weld` the copy becomes
    // the body.
    let (welded, mut sr): (Mesh, SolidReport) = match opts.repair {
        Repair::Weld => {
            let w1 = mesh.welded(WELD_EPS);
            let r1 = solid::validate_welded(&w1, nt - w1.triangle_count(), &judge);
            let mut best = (w1, r1, 1u8);
            if !best.1.boundary.is_empty() && opts.weld_tolerance > WELD_EPS as f64 {
                let w2 = mesh.welded(opts.weld_tolerance as f32);
                let r2 = solid::validate_welded(&w2, nt - w2.triangle_count(), &judge);
                if r2.boundary.len() < best.1.boundary.len() {
                    best = (w2, r2, 2);
                }
            }
            report.weld_pass = best.2;
            report.welded = nv.saturating_sub(best.0.vertex_count());
            mesh = best.0.clone();
            (best.0, best.1)
        }
        Repair::Report | Repair::Off => {
            let w = mesh.welded(WELD_EPS);
            let r = solid::validate_welded(&w, nt - w.triangle_count(), &judge);
            (w, r)
        }
    };
    if opts.repair == Repair::Weld && sr.closed() && sr.flipped.is_empty() && sr.volume < 0.0 {
        for t in mesh.indices.chunks_exact_mut(3) {
            t.swap(1, 2);
        }
        sr.volume = -sr.volume;
        report.flipped_to_outward = true;
    }
    if opts.repair == Repair::Weld {
        vertex_normals(&mut mesh);
    }

    report.triangles = mesh.triangle_count();
    report.vertices = mesh.vertex_count();
    report.collapsed = sr.collapsed;
    report.degenerate = sr.degenerate.len();
    report.shells = sr.components;
    report.open_edges = sr.boundary.len();
    report.non_manifold_edges = sr.non_manifold.len();
    report.flipped_edges = sr.flipped.len();
    report.crossings = sr.crossings.len();
    report.overlaps = sr.overlaps.len();
    report.pairs_skipped = sr.pairs_skipped;
    report.folds = sr.folds.len();
    report.closed = sr.closed();
    report.oriented = sr.oriented();
    report.solid = sr.is_solid();
    report.genus = sr.genus;
    report.volume = (report.closed && report.flipped_edges == 0).then_some(sr.volume);
    report.inverted = report.closed && report.flipped_edges == 0 && sr.volume < 0.0;
    report.area = mesh.area();
    report.bbox = mesh.bbox().map(|(lo, hi)| ([lo.x, lo.y, lo.z], [hi.x, hi.y, hi.z]));
    report.open_at = sr.boundary.first().map(|&(a, b)| midpoint(&welded, a, b));
    report.crossing_at = sr.crossings.first().map(|c| [c.at.x, c.at.y, c.at.z]);
    report.fold_at = sr.worst_fold().map(|f| [f.at.x, f.at.y, f.at.z]);

    // Topology: one face per group that still has triangles, in group
    // order, with the aggregate every mesh body's group faces carry.
    let mut faces: Vec<FaceInfo> = Vec::new();
    let mut remap: Vec<Option<u32>> = vec![None; groups.len()];
    let mut t = 0usize;
    let ntri = mesh.triangle_count();
    while t < ntri {
        let g = mesh.face_ids[t] as usize;
        let start = t;
        while t < ntri && mesh.face_ids[t] as usize == g {
            t += 1;
        }
        let mut area = 0.0;
        let mut centroid = Vec3::ZERO;
        let mut normal = Vec3::ZERO;
        for k in start..t {
            if let Some(tri) = mesh.triangle(k) {
                let n = (tri[1] - tri[0]).cross(tri[2] - tri[0]);
                let a = n.length() / 2.0;
                area += a;
                centroid = centroid + (tri[0] + tri[1] + tri[2]) * (a / 3.0);
                normal = normal + n;
            }
        }
        remap[g] = Some(faces.len() as u32);
        faces.push(FaceInfo {
            name: format!("{}/{}", opts.feature, groups[g]),
            surface_kind: "group",
            centroid: if area > 0.0 { centroid * (1.0 / area) } else { centroid },
            normal: if normal.length() > 0.0 { normal.normalized() } else { Vec3::ZERO },
            area,
            tri_start: start as u32,
            tri_count: (t - start) as u32,
            ..Default::default()
        });
    }
    for f in &mut mesh.face_ids {
        *f = remap[*f as usize].unwrap_or(0);
    }
    if faces.len() < groups.len() {
        notes.push(Note::info(format!(
            "object {name:?}: {} group{} had no triangles left and {} not named",
            groups.len() - faces.len(),
            if groups.len() - faces.len() == 1 { "" } else { "s" },
            if groups.len() - faces.len() == 1 { "is" } else { "are" }
        )));
    }
    report.groups = faces.len();
    report.object = name.clone();
    report.vertex_colors = mesh.vertex_colors().is_some();
    let topo = Topology { faces, edges: Vec::new(), vertices: Vec::new() };
    Ok(ImportedBody { name, mesh, topo, report, appearance: o.appearance })
}
