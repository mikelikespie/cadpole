//! 3MF writer, with colour.
//!
//! Written fresh from the published specifications and from the observed
//! structure of files two slicers wrote. **No code from Bambu Studio,
//! OrcaSlicer or PrusaSlicer is used or ported** — they are AGPL-3.0 and only
//! their *file formats* are targeted. What was verified
//! and what is best-effort is spelled out under "Compatibility layers" below.
//!
//! # The package
//!
//! A 3MF file is an OPC (zip) package. We write four kinds of part:
//!
//! | part | what |
//! | --- | --- |
//! | `[Content_Types].xml` | the OPC content-type defaults |
//! | `_rels/.rels` | the relationship naming `/3D/3dmodel.model` as the start part |
//! | `3D/3dmodel.model` | the model: `basematerials`, one object per body, per-triangle properties |
//! | `Metadata/…` | the slicer configuration parts, when a compatibility layer asks for them |
//!
//! # Compatibility layers
//!
//! [`Slicer::Core`] writes nothing but the 3MF core specification.
//! [`Slicer::Prusa`] (the default) adds PrusaSlicer's namespaced attributes,
//! which are legal under the core schema (`anyAttribute namespace="##other"`).
//! [`Slicer::Bambu`] adds Bambu Studio / OrcaSlicer's, which are **not**
//! namespaced and therefore technically outside the schema, so that layer is
//! opt-in.
//!
//! **Verified** against the core specification text and against real files
//! written by PrusaSlicer 2.9.4 and Bambu Studio 02.05.00.66:
//!
//! * core: the namespace, `basematerials`/`base` with required `name` and
//!   `displaycolor`, `object/@pid`/`@pindex`, `triangle/@p1` (and that `p2`,
//!   `p3` default to `p1`, and `pid` to the object's), the row-major 12-number
//!   `item/@transform`, counter-clockwise winding, and the OPC content types
//!   and relationship type;
//! * PrusaSlicer: the `slic3rpe` namespace URI, `Version3mf` and
//!   `MmPaintingVersion` as metadata *text*, the `slic3rpe:mmu_segmentation`
//!   attribute, and the `Metadata/Slic3r_PE_model.config` element structure;
//! * Bambu/Orca: the unprefixed `paint_color` attribute, the
//!   `BambuStudio:3mfVersion` metadata and namespace, the XML shape of
//!   `Metadata/model_settings.config` (`<object>`/`<part subtype=…>` with
//!   `key`/`value` metadata), that `Metadata/project_settings.config` is JSON
//!   carrying `filament_colour` as `#RRGGBB` strings, and that
//!   `Metadata/slice_info.config` is XML;
//! * the paint string encoding, including that filaments 1..4 are `4`, `8`,
//!   `0C`, `1C` — see [`paint`].
//!
//! **Best-effort**, marked in the code where it is emitted:
//!
//! * `key="extruder"` inside PrusaSlicer's per-volume config. The
//!   metadata mechanism is verified; that exact key is not.
//! * The *content* of our minimal `project_settings.config` and
//!   `slice_info.config`. Their format is verified; a three-key file of our
//!   own construction is not a file Bambu wrote, and whether Bambu is happy
//!   with a minimal one is the known risk.
//!   [`Scene::bambu_project_settings`] turns them off.
//!
//! **Never written:** PrusaSlicer's print configuration part,
//! `Metadata/Slic3r_PE.config` ([`PRUSA_PRINT_CONFIG`]). In a slicer's own
//! file it is the whole print profile, and a slicer that finds one loads it
//! as such: a stub holding only the filament colours is a broken profile,
//! which Bambu Studio refuses outright ("invalid config"). A complete one
//! would be a profile of our invention. The colours reach every reader
//! without it, through `basematerials` and the paint attributes, and the
//! slicer keeps the user's own profile.
//!
//! **Known, and why we still write the core properties:** none of the three
//! slicers reads per-triangle `pid`/`p1` — they use the paint attributes
//! instead. Microsoft's viewers and `lib3mf` do read them. The two do not
//! conflict, so both go in the same file and every consumer sees colour.

use std::collections::HashMap;
use std::fmt::Write as _;

use cadpole_core::solid::SolidOptions;
use cadpole_core::tessellate::Mesh;

pub mod bambu;
pub mod bambu_settings;
pub mod paint;
pub mod zip;

/// Which compatibility layers to write.
#[derive(Clone, Copy, Debug, PartialEq, Eq, Default)]
pub enum Slicer {
    /// The 3MF core specification and nothing else.
    Core,
    /// Core plus PrusaSlicer's namespaced attributes and per-object config
    /// part (`Slic3r_PE_model.config`; never the print profile). The
    /// default: the extra attributes are valid under the core schema, so the
    /// file stays conformant.
    #[default]
    Prusa,
    /// Core, PrusaSlicer *and* Bambu Studio / OrcaSlicer. Opt-in because
    /// Bambu's `paint_color` attribute carries no namespace and is therefore
    /// outside the core schema.
    Bambu,
}

impl Slicer {
    pub fn parse(s: &str) -> Result<Slicer, String> {
        match s.to_ascii_lowercase().as_str() {
            "" | "core" | "none" | "standard" => Ok(Slicer::Core),
            "prusa" | "prusaslicer" | "slic3r" => Ok(Slicer::Prusa),
            "bambu" | "bambustudio" | "orca" | "orcaslicer" => Ok(Slicer::Bambu),
            other => Err(format!("unknown slicer {other:?}; expected \"core\", \"prusa\" or \"bambu\"")),
        }
    }
    fn prusa(self) -> bool {
        matches!(self, Slicer::Prusa | Slicer::Bambu)
    }
    fn bambu(self) -> bool {
        matches!(self, Slicer::Bambu)
    }
}

/// How bodies become 3MF objects.
#[derive(Clone, Copy, Debug, PartialEq, Eq, Default)]
pub enum Grouping {
    /// One object per body, one build item each: a plate of separate parts.
    #[default]
    Objects,
    /// Every body merged into one object, each body kept as a *volume*
    /// (PrusaSlicer) / *part* (Bambu) with its own filament: one
    /// multi-material part.
    Part,
}

impl Grouping {
    pub fn parse(s: &str) -> Result<Grouping, String> {
        match s.to_ascii_lowercase().as_str() {
            "" | "objects" => Ok(Grouping::Objects),
            "part" => Ok(Grouping::Part),
            other => Err(format!("unknown grouping {other:?}; expected \"objects\" or \"part\"")),
        }
    }
}

/// One filament slot: what the printer would have loaded in that position.
#[derive(Clone, Debug, PartialEq)]
pub struct Filament {
    /// Label. Empty becomes `#rrggbb`.
    pub name: String,
    /// Linear RGB in 0..1, as `Material::base_color` stores it. The file
    /// carries sRGB bytes, so `hex()` encodes on the way out.
    pub color: [f64; 3],
}

impl Filament {
    pub fn hex(&self) -> String {
        cadpole_core::paint::hex(self.color)
    }
    fn label(&self) -> String {
        if self.name.is_empty() { self.hex() } else { self.name.clone() }
    }
}

/// One body on its way out.
pub struct Part<'a> {
    pub name: String,
    pub mesh: &'a Mesh,
    /// Filament slot for the whole body, 1-based, indexing [`Scene::palette`].
    pub slot: u32,
    /// Per-triangle slot override, in `mesh.indices` order. Empty for an
    /// unpainted body; `0` in an entry means "use `slot`".
    pub tri_slots: Vec<u32>,
    /// Where copies of this mesh are built: one 3MF `<item
    /// transform=...>` per entry, the 3×3 rotation row by row then the
    /// translation, twelve numbers. Empty means one item at the identity —
    /// the mesh where it is. An assembly writes each part's mesh once as
    /// an `<object>` and every instance as an item; under
    /// [`Grouping::Part`] the copies are baked into the one object instead,
    /// because that object has no items of its own to carry them.
    pub items: Vec<[f64; 12]>,
    /// The body's display layer, empty for none. Written as
    /// the object's `cadpole:layer` metadata under [`Grouping::Objects`]; a
    /// merged part has no per-volume place for it in the core schema.
    pub layer: String,
}

impl<'a> Part<'a> {
    pub fn new(name: impl Into<String>, mesh: &'a Mesh, slot: u32) -> Part<'a> {
        Part { name: name.into(), mesh, slot: slot.max(1), tri_slots: Vec::new(), items: Vec::new(), layer: String::new() }
    }

    /// The build items this part is written with: its own, or the identity.
    fn build_items(&self) -> Vec<[f64; 12]> {
        if self.items.is_empty() { vec![IDENTITY_ITEM] } else { self.items.clone() }
    }
    fn slot_of(&self, t: usize) -> u32 {
        match self.tri_slots.get(t) {
            Some(0) | None => self.slot,
            Some(s) => *s,
        }
    }
}

/// Everything one 3MF file holds.
pub struct Scene<'a> {
    /// Model title, written as 3MF `Title` metadata.
    pub title: String,
    pub parts: Vec<Part<'a>>,
    /// Filament slots, slot 1 first.
    pub palette: Vec<Filament>,
    pub slicer: Slicer,
    pub group: Grouping,
    /// How many filament slots the target machine has; only used to warn.
    /// 0 means "do not warn".
    pub machine_slots: u32,
    /// Write Bambu's `project_settings.config` and `slice_info.config`.
    /// Their *format* is verified, their minimal content is ours; turn this
    /// off if a Bambu version refuses the project.
    pub bambu_project_settings: bool,
}

impl<'a> Default for Scene<'a> {
    fn default() -> Scene<'a> {
        Scene {
            title: String::new(),
            parts: Vec::new(),
            palette: Vec::new(),
            slicer: Slicer::default(),
            group: Grouping::default(),
            machine_slots: 0,
            bambu_project_settings: true,
        }
    }
}

/// The file, plus anything the user should know about it.
pub struct Written {
    pub bytes: Vec<u8>,
    pub warnings: Vec<String>,
}

/// Part names inside the package.
pub const MODEL_PART: &str = "3D/3dmodel.model";
pub const CONTENT_TYPES_PART: &str = "[Content_Types].xml";
pub const RELS_PART: &str = "_rels/.rels";
pub const PRUSA_MODEL_CONFIG: &str = "Metadata/Slic3r_PE_model.config";
/// PrusaSlicer's print profile part. Named so tests can say it is absent:
/// the writer never emits it (see "Never written" above).
pub const PRUSA_PRINT_CONFIG: &str = "Metadata/Slic3r_PE.config";
pub const BAMBU_MODEL_CONFIG: &str = "Metadata/model_settings.config";
pub const BAMBU_PROJECT_CONFIG: &str = "Metadata/project_settings.config";
pub const BAMBU_SLICE_INFO: &str = "Metadata/slice_info.config";

pub(crate) const CORE_NS: &str = "http://schemas.microsoft.com/3dmanufacturing/core/2015/02";
/// Cadpole's own metadata names (`cadpole:layer`). The core specification
/// requires a metadata name that is not one of its well-known names to be
/// namespace-qualified; a consumer ignores a namespace it does not know.
pub const CADPOLE_NS: &str = "https://cadpole.dev/3mf/2026/10";
const PRUSA_NS: &str = "http://schemas.slic3r.org/3mf/2017/06";
pub(crate) const BAMBU_NS: &str = "http://schemas.bambulab.com/package/2021";
pub(crate) const MODEL_REL: &str = "http://schemas.microsoft.com/3dmanufacturing/2013/01/3dmodel";

/// Resource id of the one `<basematerials>` group. Every other resource is
/// numbered from [`FIRST_OBJECT_ID`], because ids are unique model-wide.
const MATERIALS_ID: u32 = 1;
/// The identity build item: the model where the script put it.
pub(crate) const IDENTITY_ITEM: [f64; 12] = [1.0, 0.0, 0.0, 0.0, 1.0, 0.0, 0.0, 0.0, 1.0, 0.0, 0.0, 0.0];

/// `mesh` with every vertex moved by the affine map `m` (a build item's
/// twelve numbers): what [`Grouping::Part`] does with an instance, since a
/// merged object cannot carry items.
fn moved(mesh: &Mesh, m: &[f64; 12]) -> Mesh {
    let mut out = mesh.clone();
    for p in out.positions.chunks_exact_mut(3) {
        let (x, y, z) = (p[0] as f64, p[1] as f64, p[2] as f64);
        p[0] = (m[0] * x + m[3] * y + m[6] * z + m[9]) as f32;
        p[1] = (m[1] * x + m[4] * y + m[7] * z + m[10]) as f32;
        p[2] = (m[2] * x + m[5] * y + m[8] * z + m[11]) as f32;
    }
    out
}
const FIRST_OBJECT_ID: u32 = 2;

// ---------------------------------------------------------------------------
// The writer
// ---------------------------------------------------------------------------

/// Write `scene` as a 3MF package.
pub fn write(scene: &Scene<'_>) -> Written {
    let mut warnings = Vec::new();
    if scene.machine_slots > 0 && scene.palette.len() as u32 > scene.machine_slots {
        warnings.push(format!(
            "{} distinct colours but the printer has {} filament slots: merge {} of them, or the \
             slicer will ask you to",
            scene.palette.len(),
            scene.machine_slots,
            scene.palette.len() as u32 - scene.machine_slots
        ));
    }
    let (objects, duplicates) = build_objects(scene);
    if objects.is_empty() {
        warnings.push("nothing to export: no bodies with triangles".to_owned());
    }
    if duplicates > 0 {
        warnings.push(format!(
            "left out {duplicates} triangle{} that repeated another's three vertices: 3MF needs \
             every edge used by exactly two triangles, and a back-to-back pair encloses nothing",
            if duplicates == 1 { "" } else { "s" },
        ));
    }
    // What the core specification asks of the mesh being written, measured on
    // that mesh with the one definition of a solid the tree has
    // (`cadpole_core::solid`). The pairwise search is off: crossing triangles are
    // not one of the structural rules and finding them costs more than every
    // other check here put together.
    //
    // Per body, not per object. Merging bodies into one object
    // (`Grouping::Part`) is how a multi-material part is made, and bodies that
    // share a face are the whole point of it, so the merged surface is
    // legitimately not manifold and only the bodies in it can be judged.
    let opts = SolidOptions { pair_limit: 0, ..Default::default() };
    for o in &objects {
        for (name, _, first, last) in &o.volumes {
            let report = cadpole_core::solid::validate_with(&as_mesh(o, *first, *last), &opts);
            if !report.oriented() {
                warnings.push(format!(
                    "{name:?}: 3MF asks for a closed, consistently wound surface and this is not \
                     one -- {}. The file is written; a slicer will try to repair it, or refuse it",
                    report.problems().join("; "),
                ));
            }
        }
    }

    let mut z = zip::ZipWriter::new();
    z.add(CONTENT_TYPES_PART, content_types(scene).as_bytes());
    z.add(RELS_PART, rels().as_bytes());
    z.add(MODEL_PART, model_xml(scene, &objects).as_bytes());
    if scene.slicer.prusa() {
        z.add(PRUSA_MODEL_CONFIG, prusa_model_config(&objects).as_bytes());
    }
    if scene.slicer.bambu() {
        z.add(BAMBU_MODEL_CONFIG, bambu_model_config(&objects).as_bytes());
        if scene.bambu_project_settings {
            z.add(BAMBU_PROJECT_CONFIG, bambu_project_settings(scene).as_bytes());
            z.add(BAMBU_SLICE_INFO, bambu_slice_info().as_bytes());
        }
    }
    Written { bytes: z.finish(), warnings }
}

/// One `<object>` on its way out, with its vertices welded and its triangles
/// carrying the slot each one prints in.
pub(crate) struct OutObject {
    pub(crate) id: u32,
    pub(crate) name: String,
    /// Object-level filament slot: the one most of its triangles use.
    pub(crate) slot: u32,
    pub(crate) verts: Vec<[f32; 3]>,
    pub(crate) tris: Vec<([u32; 3], u32)>,
    /// `(name, slot, first triangle, last triangle)` — one per source body.
    /// A single-body object has exactly one.
    pub(crate) volumes: Vec<(String, u32, usize, usize)>,
    /// The build items this object is placed by: one per instance, or the
    /// identity.
    pub(crate) items: Vec<[f64; 12]>,
    /// The display layer, empty for none: `cadpole:layer` metadata.
    pub(crate) layer: String,
}

/// The objects to write, and how many triangles were left out as duplicates.
fn build_objects(scene: &Scene<'_>) -> (Vec<OutObject>, usize) {
    let mut dropped = 0;
    match scene.group {
        Grouping::Objects => {
            let objects = scene
                .parts
                .iter()
                .enumerate()
                .filter_map(|(i, p)| {
                    let mut o = OutObject {
                        id: FIRST_OBJECT_ID + i as u32,
                        name: p.name.clone(),
                        slot: p.slot,
                        verts: Vec::new(),
                        tris: Vec::new(),
                        volumes: Vec::new(),
                        items: p.build_items(),
                        layer: p.layer.clone(),
                    };
                    dropped += append(&mut o, p);
                    if o.tris.is_empty() { None } else { Some(o) }
                })
                .collect();
            (objects, dropped)
        }
        Grouping::Part => {
            let mut o = OutObject {
                id: FIRST_OBJECT_ID,
                name: if scene.title.is_empty() { "part".to_owned() } else { scene.title.clone() },
                slot: scene.parts.first().map(|p| p.slot).unwrap_or(1),
                verts: Vec::new(),
                tris: Vec::new(),
                volumes: Vec::new(),
                items: vec![IDENTITY_ITEM],
                layer: String::new(),
            };
            for p in &scene.parts {
                if p.items.is_empty() {
                    dropped += append(&mut o, p);
                } else {
                    // A merged object carries no items: bake each copy.
                    for (k, m) in p.items.iter().enumerate() {
                        let mesh = moved(p.mesh, m);
                        let copy = Part {
                            name: if k == 0 { p.name.clone() } else { format!("{}#{k}", p.name) },
                            mesh: &mesh,
                            slot: p.slot,
                            tri_slots: p.tri_slots.clone(),
                            items: Vec::new(),
                            layer: p.layer.clone(),
                        };
                        dropped += append(&mut o, &copy);
                    }
                }
            }
            (if o.tris.is_empty() { Vec::new() } else { vec![o] }, dropped)
        }
    }
}

/// Triangles `first..=last` of an object as a [`Mesh`], so the validator can
/// read them. Everything it measures comes from the positions and the winding,
/// but `Mesh::welded` carries a normal across for every vertex it keeps and
/// indexes the array to do it, so there has to be one per vertex. Vertices the
/// range does not use are simply unused; the validator counts only the ones a
/// triangle names.
pub(crate) fn as_mesh(o: &OutObject, first: usize, last: usize) -> Mesh {
    Mesh {
        positions: o.verts.iter().flat_map(|v| *v).collect(),
        normals: vec![0.0; o.verts.len() * 3],
        indices: o.tris[first..=last].iter().flat_map(|(v, _)| *v).collect(),
        face_ids: Vec::new(),
        colors: Vec::new(),
    }
}

/// The three vertices in ascending order: one key for a triangle and for the
/// same triangle wound the other way.
fn vertex_set(v: [u32; 3]) -> [u32; 3] {
    let mut k = v;
    k.sort_unstable();
    k
}

/// Does this triangle run round its vertex set in ascending order?
fn forward(v: [u32; 3]) -> bool {
    let k = vertex_set(v);
    v == k || v == [k[1], k[2], k[0]] || v == [k[2], k[0], k[1]]
}

/// Append one body's welded geometry to an object, recording its triangle
/// range as a volume. Returns how many triangles were left out as duplicates.
///
/// Two triangles on the same three vertices are a spec violation either way
/// round: every edge they share ends up used by four triangles instead of two,
/// and the 3MF core specification requires exactly two. The mesh kernel emits
/// such a pair now and then (`examples/filleted_bracket.star` gets a
/// back-to-back pair on its x = -30 face in some runs and not in others).
///
/// So the tally per vertex set is signed: a triangle wound round it in
/// ascending order counts +1, the other way -1. A back-to-back pair sums to
/// zero and neither is written, which is right because between them they
/// enclose no volume and every one of their edges loses two uses, leaving the
/// rest of the surface exactly as it was. A net of +1 or -1 writes one
/// triangle, which also folds away an exact repeat.
pub(crate) fn append(o: &mut OutObject, p: &Part<'_>) -> usize {
    let first = o.tris.len();
    let (verts, remap) = weld(p.mesh);
    let base = o.verts.len() as u32;
    o.verts.extend(verts);

    let mut candidates: Vec<([u32; 3], u32)> = Vec::with_capacity(p.mesh.triangle_count());
    let mut net: HashMap<[u32; 3], i32> = HashMap::new();
    for t in 0..p.mesh.triangle_count() {
        let f = &p.mesh.indices[t * 3..t * 3 + 3];
        let v = [remap[f[0] as usize] + base, remap[f[1] as usize] + base, remap[f[2] as usize] + base];
        // 3MF requires the three vertices of a triangle to be distinct.
        if v[0] == v[1] || v[1] == v[2] || v[0] == v[2] {
            continue;
        }
        *net.entry(vertex_set(v)).or_default() += if forward(v) { 1 } else { -1 };
        candidates.push((v, p.slot_of(t)));
    }

    let mut dropped = 0;
    for (v, slot) in candidates {
        let n = net.get_mut(&vertex_set(v)).expect("counted in the first pass");
        if (*n > 0) != forward(v) || *n == 0 {
            dropped += 1;
            continue;
        }
        *n = 0; // one is written; any further copy of this vertex set is not.
        o.tris.push((v, slot));
    }

    if o.tris.len() > first {
        o.volumes.push((p.name.clone(), p.slot, first, o.tris.len() - 1));
    }
    dropped
}

/// Weld vertices within 1e-4 mm, the tolerance the mesh kernel uses. Returns
/// the unique positions and, for every input vertex, which one it became.
/// A spatial hash with a cell of one tolerance, searching the 27 cells around
/// a point, so two vertices either side of a cell boundary still merge.
fn weld(mesh: &Mesh) -> (Vec<[f32; 3]>, Vec<u32>) {
    const EPS: f32 = 1e-4;
    let q = |v: f32| (v / EPS).floor() as i64;
    let mut cells: HashMap<(i64, i64, i64), Vec<u32>> = HashMap::new();
    let mut verts: Vec<[f32; 3]> = Vec::new();
    let mut remap = Vec::with_capacity(mesh.vertex_count());
    for p in mesh.positions.chunks_exact(3) {
        let key = (q(p[0]), q(p[1]), q(p[2]));
        let mut found = None;
        'search: for dx in -1..=1 {
            for dy in -1..=1 {
                for dz in -1..=1 {
                    if let Some(ids) = cells.get(&(key.0 + dx, key.1 + dy, key.2 + dz)) {
                        for &id in ids {
                            let v = verts[id as usize];
                            let d = (v[0] - p[0]).powi(2) + (v[1] - p[1]).powi(2) + (v[2] - p[2]).powi(2);
                            if d <= EPS * EPS {
                                found = Some(id);
                                break 'search;
                            }
                        }
                    }
                }
            }
        }
        let id = found.unwrap_or_else(|| {
            verts.push([p[0], p[1], p[2]]);
            let id = verts.len() as u32 - 1;
            cells.entry(key).or_default().push(id);
            id
        });
        remap.push(id);
    }
    (verts, remap)
}

// ---------------------------------------------------------------------------
// The parts
// ---------------------------------------------------------------------------

fn content_types(scene: &Scene<'_>) -> String {
    let mut s = String::from(
        "<?xml version=\"1.0\" encoding=\"UTF-8\"?>\n\
         <Types xmlns=\"http://schemas.openxmlformats.org/package/2006/content-types\">\n\
         \x20<Default Extension=\"rels\" ContentType=\"application/vnd.openxmlformats-package.relationships+xml\"/>\n\
         \x20<Default Extension=\"model\" ContentType=\"application/vnd.ms-package.3dmanufacturing-3dmodel+xml\"/>\n",
    );
    if scene.slicer != Slicer::Core {
        // Every part extension in an OPC package needs a content type.
        s.push_str(" <Default Extension=\"config\" ContentType=\"application/octet-stream\"/>\n");
    }
    s.push_str("</Types>\n");
    s
}

fn rels() -> String {
    format!(
        "<?xml version=\"1.0\" encoding=\"UTF-8\"?>\n\
         <Relationships xmlns=\"http://schemas.openxmlformats.org/package/2006/relationships\">\n\
         \x20<Relationship Id=\"rel-1\" Target=\"/{MODEL_PART}\" Type=\"{MODEL_REL}\"/>\n\
         </Relationships>\n"
    )
}

fn model_xml(scene: &Scene<'_>, objects: &[OutObject]) -> String {
    let mut s = String::with_capacity(objects.iter().map(|o| o.tris.len() * 64 + o.verts.len() * 48).sum());
    s.push_str("<?xml version=\"1.0\" encoding=\"UTF-8\"?>\n");
    // `unit` defaults to millimeter, but every consumer expects to see it and
    // a file that says what it means is worth two attributes.
    let _ = write!(s, "<model unit=\"millimeter\" xml:lang=\"en-US\" xmlns=\"{CORE_NS}\"");
    if scene.slicer.prusa() {
        let _ = write!(s, " xmlns:slic3rpe=\"{PRUSA_NS}\"");
    }
    if scene.slicer.bambu() {
        let _ = write!(s, " xmlns:BambuStudio=\"{BAMBU_NS}\"");
    }
    // Declared only when something uses it, so a file without layers is
    // byte for byte what it always was.
    if objects.iter().any(|o| !o.layer.is_empty()) {
        let _ = write!(s, " xmlns:cadpole=\"{CADPOLE_NS}\"");
    }
    s.push_str(">\n");
    s.push_str(" <metadata name=\"Application\">Cadpole</metadata>\n");
    if !scene.title.is_empty() {
        let _ = writeln!(s, " <metadata name=\"Title\">{}</metadata>", esc(&scene.title));
    }
    if scene.slicer.prusa() {
        // Verified: the value is the element's text, not a `value=` attribute.
        s.push_str(" <metadata name=\"slic3rpe:Version3mf\">1</metadata>\n");
        s.push_str(" <metadata name=\"slic3rpe:MmPaintingVersion\">1</metadata>\n");
    }
    if scene.slicer.bambu() {
        s.push_str(" <metadata name=\"BambuStudio:3mfVersion\">1</metadata>\n");
    }

    s.push_str(" <resources>\n");
    // Resources must be defined before they are referenced, so materials go
    // first.
    if !scene.palette.is_empty() {
        let _ = writeln!(s, "  <basematerials id=\"{MATERIALS_ID}\">");
        for f in &scene.palette {
            // Both `name` and `displaycolor` are required by the core schema.
            let _ = writeln!(s, "   <base name=\"{}\" displaycolor=\"{}\"/>", esc(&f.label()), f.hex());
        }
        s.push_str("  </basematerials>\n");
    }

    let paint_attrs = scene.slicer != Slicer::Core;
    for o in objects {
        let _ = write!(s, "  <object id=\"{}\" type=\"model\" name=\"{}\"", o.id, esc(&o.name));
        if !scene.palette.is_empty() {
            // An object with any per-triangle properties must itself carry
            // `pid` and `pindex`; indices are 0-based into `<base>` order.
            let _ = write!(s, " pid=\"{MATERIALS_ID}\" pindex=\"{}\"", index_of(o.slot));
        }
        s.push_str(">\n");
        if !o.layer.is_empty() {
            // Core 1.2's object-level metadata group, before the mesh.
            let _ = writeln!(
                s,
                "   <metadatagroup>\n    <metadata name=\"cadpole:layer\">{}</metadata>\n   </metadatagroup>",
                esc(&o.layer)
            );
        }
        s.push_str("   <mesh>\n    <vertices>\n");
        for v in &o.verts {
            let _ = writeln!(s, "     <vertex x=\"{}\" y=\"{}\" z=\"{}\"/>", v[0], v[1], v[2]);
        }
        s.push_str("    </vertices>\n    <triangles>\n");
        for (v, slot) in &o.tris {
            let _ = write!(s, "     <triangle v1=\"{}\" v2=\"{}\" v3=\"{}\"", v[0], v[1], v[2]);
            if !scene.palette.is_empty() && *slot != o.slot {
                // p2 and p3 default to p1, and for a basematerials group they
                // have to be equal anyway: one attribute says it all.
                let _ = write!(s, " p1=\"{}\"", index_of(*slot));
            }
            if paint_attrs && *slot != o.slot {
                if let Some(p) = paint::whole_triangle(*slot) {
                    if scene.slicer.prusa() {
                        let _ = write!(s, " slic3rpe:mmu_segmentation=\"{p}\"");
                    }
                    if scene.slicer.bambu() {
                        // Unprefixed, which is why this layer is opt-in.
                        let _ = write!(s, " paint_color=\"{p}\"");
                    }
                }
            }
            s.push_str("/>\n");
        }
        s.push_str("    </triangles>\n   </mesh>\n  </object>\n");
    }
    s.push_str(" </resources>\n <build>\n");
    for o in objects {
        // Row-major, 12 numbers, translation last: the identity leaves the
        // model where the script put it; an instance's item carries its
        // pose, and the same object is built once per instance.
        for m in &o.items {
            let t = m.iter().map(|v| fmt_item_number(*v)).collect::<Vec<_>>().join(" ");
            let _ = writeln!(s, "  <item objectid=\"{}\" transform=\"{t}\"/>", o.id);
        }
    }
    s.push_str(" </build>\n</model>\n");
    s
}

/// A build-item number: an integer prints as one (`1`, not `1.0`), so the
/// identity reads exactly as it always has.
pub(crate) fn fmt_item_number(v: f64) -> String {
    if v == v.trunc() && v.abs() < 1e15 { format!("{}", v as i64) } else { format!("{v}") }
}

/// 1-based filament slot to the 0-based `<base>` index it refers to.
pub(crate) fn index_of(slot: u32) -> u32 {
    slot.saturating_sub(1)
}

/// PrusaSlicer's per-object configuration. Structure verified against a
/// PrusaSlicer 2.9.4 file; `key="extruder"` itself is **best-effort** (the
/// mechanism is verified, that key is not).
fn prusa_model_config(objects: &[OutObject]) -> String {
    let mut s = String::from("<?xml version=\"1.0\" encoding=\"UTF-8\"?>\n<config>\n");
    for o in objects {
        let _ = writeln!(s, " <object id=\"{}\" instances_count=\"{}\">", o.id, o.items.len().max(1));
        let _ = writeln!(s, "  <metadata type=\"object\" key=\"name\" value=\"{}\"/>", esc(&o.name));
        let _ = writeln!(s, "  <metadata type=\"object\" key=\"extruder\" value=\"{}\"/>", o.slot);
        for (name, slot, first, last) in &o.volumes {
            // `firstid`/`lastid` are inclusive triangle indices into the
            // object's mesh.
            let _ = writeln!(s, "  <volume firstid=\"{first}\" lastid=\"{last}\">");
            let _ = writeln!(s, "   <metadata type=\"volume\" key=\"name\" value=\"{}\"/>", esc(name));
            s.push_str("   <metadata type=\"volume\" key=\"volume_type\" value=\"ModelPart\"/>\n");
            let _ = writeln!(s, "   <metadata type=\"volume\" key=\"extruder\" value=\"{slot}\"/>");
            s.push_str("   <metadata type=\"volume\" key=\"matrix\" value=\"1 0 0 0 0 1 0 0 0 0 1 0 0 0 0 1\"/>\n");
            s.push_str("  </volume>\n");
        }
        s.push_str(" </object>\n");
    }
    s.push_str("</config>\n");
    s
}

/// Bambu Studio / OrcaSlicer per-object configuration. Verified shape:
/// `<object>` with `key`/`value` metadata (no `type=`) and `<part>` children
/// with a `subtype`.
fn bambu_model_config(objects: &[OutObject]) -> String {
    let mut s = String::from("<?xml version=\"1.0\" encoding=\"UTF-8\"?>\n<config>\n");
    let mut part_id = 1u32;
    for o in objects {
        let _ = writeln!(s, " <object id=\"{}\">", o.id);
        let _ = writeln!(s, "  <metadata key=\"name\" value=\"{}\"/>", esc(&o.name));
        let _ = writeln!(s, "  <metadata key=\"extruder\" value=\"{}\"/>", o.slot);
        for (name, slot, _, _) in &o.volumes {
            let _ = writeln!(s, "  <part id=\"{part_id}\" subtype=\"normal_part\">");
            let _ = writeln!(s, "   <metadata key=\"name\" value=\"{}\"/>", esc(name));
            let _ = writeln!(s, "   <metadata key=\"extruder\" value=\"{slot}\"/>");
            s.push_str("   <metadata key=\"matrix\" value=\"1 0 0 0 0 1 0 0 0 0 1 0 0 0 0 1\"/>\n");
            s.push_str("  </part>\n");
            part_id += 1;
        }
        s.push_str(" </object>\n");
    }
    s.push_str("</config>\n");
    s
}

/// A minimal Bambu project settings part: enough for the preview to colour the
/// filaments, nothing that would fight the user's own printer profile. The
/// format is verified; this minimal content is ours, hence
/// [`Scene::bambu_project_settings`].
fn bambu_project_settings(scene: &Scene<'_>) -> String {
    let colors =
        scene.palette.iter().map(|f| format!("\"{}\"", f.hex())).collect::<Vec<_>>().join(", ");
    format!(
        "{{\n  \"from\": \"project\",\n  \"name\": \"project_settings\",\n  \"filament_colour\": [{colors}]\n}}\n"
    )
}

pub(crate) fn bambu_slice_info() -> String {
    "<?xml version=\"1.0\" encoding=\"UTF-8\"?>\n\
     <config>\n\
     \x20<header>\n\
     \x20 <header_item key=\"X-BBL-Client-Type\" value=\"slicer\"/>\n\
     \x20 <header_item key=\"X-BBL-Client-Version\" value=\"cadpole\"/>\n\
     \x20</header>\n\
     </config>\n"
        .to_owned()
}

/// XML attribute and text escaping. Attribute values here are always
/// double-quoted, so the single quote is left alone.
pub(crate) fn esc(s: &str) -> String {
    let mut out = String::with_capacity(s.len());
    for c in s.chars() {
        match c {
            '&' => out.push_str("&amp;"),
            '<' => out.push_str("&lt;"),
            '>' => out.push_str("&gt;"),
            '"' => out.push_str("&quot;"),
            c if (c as u32) < 0x20 && c != '\t' && c != '\n' && c != '\r' => out.push(' '),
            c => out.push(c),
        }
    }
    out
}

#[cfg(test)]
mod tests;
