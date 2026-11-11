//! 3MF into [`RawObject`]s: a standard package, or a Bambu Studio project.
//!
//! The package is read by `cadpole_export_3mf::zip::read_with` under the caps
//! of [`crate::prescan`]; the root model is whatever `_rels/.rels` points
//! at with the `3dmodel` relationship type, or `3D/3dmodel.model`. Inside
//! a model: `<model unit>` scales to millimetres; every `<object>` is a
//! `<mesh>` or a `<components>` list; every `<build>/<item>` becomes one
//! body, placed by its transform. A component points at an object in the
//! same model or, with the Production extension's `p:path`, in another
//! model part of the package — which is how a Bambu project keeps every
//! print object in its own `3D/Objects/object_N.model` and the root holds
//! only components. A multi-component object is one body with one group
//! per component, so a print-in-place set of parts imports as the object
//! Bambu Studio moves as a unit, its parts as named faces.
//!
//! Names: a Bambu project's `Metadata/model_settings.config` (the object's
//! `name` metadata for the body, a part's for its group); otherwise the
//! `name` attribute on the object; otherwise the caller's `object[k]` /
//! `part[j]`. Transforms are the 3MF row-vector form, twelve numbers, the
//! rotation rows then the translation — the layout Cadpole's own writer uses.

use std::collections::{HashMap, HashSet};

use cadpole_export_3mf::zip;
use roxmltree::{Document, Node, ParsingOptions};

use crate::Note;
use crate::mesh::RawObject;
use crate::prescan::{self, MAX_COMPONENT_DEPTH, MAX_MODEL_BYTES, MAX_PLACEMENTS, MAX_TRIANGLES, MAX_VERTICES};

type Xf = [f64; 12];
const IDENTITY: Xf = [1.0, 0.0, 0.0, 0.0, 1.0, 0.0, 0.0, 0.0, 1.0, 0.0, 0.0, 0.0];

fn apply(m: &Xf, p: [f64; 3]) -> [f64; 3] {
    let [x, y, z] = p;
    [
        m[0] * x + m[3] * y + m[6] * z + m[9],
        m[1] * x + m[4] * y + m[7] * z + m[10],
        m[2] * x + m[5] * y + m[8] * z + m[11],
    ]
}

/// `a` then `b`: the transform whose effect on a point is `apply(b, apply(a, p))`.
fn compose(a: &Xf, b: &Xf) -> Xf {
    let mut r = [0.0; 12];
    for i in 0..3 {
        for j in 0..3 {
            r[3 * i + j] = (0..3).map(|k| a[3 * i + k] * b[3 * k + j]).sum();
        }
    }
    let t = apply(b, [a[9], a[10], a[11]]);
    r[9] = t[0];
    r[10] = t[1];
    r[11] = t[2];
    r
}

/// `m` with its translation multiplied by `scale`: a transform written in
/// a model part's units, as millimetres. The rotation rows are ratios and
/// stay as they are.
fn in_mm(m: &Xf, scale: f64) -> Xf {
    let mut r = *m;
    for t in &mut r[9..12] {
        *t *= scale;
    }
    r
}

fn parse_transform(s: Option<&str>, what: &str) -> Result<Xf, String> {
    let Some(s) = s else { return Ok(IDENTITY) };
    let v: Vec<f64> = s.split_whitespace().map(|t| t.parse::<f64>().map_err(|_| format!("{what}: {t:?} in the transform is not a number"))).collect::<Result<_, _>>()?;
    if v.len() != 12 {
        return Err(format!("{what}: a transform has 12 numbers, this one has {}", v.len()));
    }
    if v.iter().any(|x| !x.is_finite()) {
        return Err(format!("{what}: the transform is not finite"));
    }
    Ok(v.try_into().expect("twelve"))
}

struct Component {
    path: Option<String>,
    objectid: u32,
    transform: Xf,
}

enum Kind {
    Mesh { positions: Vec<[f64; 3]>, triangles: Vec<[u32; 3]> },
    Components(Vec<Component>),
}

struct Object {
    name: String,
    kind: Kind,
}

struct Item {
    objectid: u32,
    transform: Xf,
}

struct Model {
    scale: f64,
    objects: HashMap<u32, Object>,
    build: Vec<Item>,
}

/// One part of the package, parsed on first use.
struct Package<'a> {
    archive: &'a zip::Archive,
    models: HashMap<String, Model>,
}

fn attr<'a>(n: &Node<'a, '_>, name: &str) -> Option<&'a str> {
    n.attributes().find(|a| a.name() == name).map(|a| a.value())
}

fn parse_u32(s: Option<&str>, what: &str) -> Result<u32, String> {
    let s = s.ok_or_else(|| format!("{what} has no id"))?;
    s.parse::<u32>().map_err(|_| format!("{what}: {s:?} is not an id"))
}

fn parse_f64(s: Option<&str>, what: &str) -> Result<f64, String> {
    let s = s.ok_or_else(|| format!("{what} is missing a coordinate"))?;
    let v = s.parse::<f64>().map_err(|_| format!("{what}: {s:?} is not a number"))?;
    if !v.is_finite() {
        return Err(format!("{what}: {s:?} is not finite"));
    }
    Ok(v)
}

fn parse_model(text: &str, part: &str) -> Result<Model, String> {
    if text.len() > MAX_MODEL_BYTES {
        return Err(format!("{part}: {} bytes is over the {} MB model limit", text.len(), MAX_MODEL_BYTES / (1024 * 1024)));
    }
    let doc = Document::parse_with_options(text, ParsingOptions { allow_dtd: false, ..ParsingOptions::default() })
        .map_err(|e| format!("{part}: not well-formed XML: {e}"))?;
    let root = doc.root_element();
    if root.tag_name().name() != "model" {
        return Err(format!("{part}: the root element is <{}>, not <model>", root.tag_name().name()));
    }
    let scale = match attr(&root, "unit").unwrap_or("millimeter") {
        "micron" => 1e-3,
        "millimeter" => 1.0,
        "centimeter" => 10.0,
        "inch" => 25.4,
        "foot" => 304.8,
        "meter" => 1000.0,
        other => return Err(format!("{part}: unknown model unit {other:?}")),
    };
    let mut objects = HashMap::new();
    let mut triangles_total = 0usize;
    if let Some(resources) = root.children().find(|n| n.tag_name().name() == "resources") {
        for o in resources.children().filter(|n| n.tag_name().name() == "object") {
            let id = parse_u32(attr(&o, "id"), &format!("{part}: an <object>"))?;
            let what = format!("{part}: object {id}");
            let name = attr(&o, "name").unwrap_or("").to_owned();
            let kind = if let Some(mesh) = o.children().find(|n| n.tag_name().name() == "mesh") {
                let mut positions = Vec::new();
                if let Some(vs) = mesh.children().find(|n| n.tag_name().name() == "vertices") {
                    for v in vs.children().filter(|n| n.tag_name().name() == "vertex") {
                        let p = [
                            parse_f64(attr(&v, "x"), &what)?,
                            parse_f64(attr(&v, "y"), &what)?,
                            parse_f64(attr(&v, "z"), &what)?,
                        ];
                        positions.push(p);
                        if positions.len() > MAX_VERTICES {
                            return Err(format!("{what}: more than {MAX_VERTICES} vertices"));
                        }
                    }
                }
                let mut triangles = Vec::new();
                if let Some(ts) = mesh.children().find(|n| n.tag_name().name() == "triangles") {
                    for t in ts.children().filter(|n| n.tag_name().name() == "triangle") {
                        let mut tri = [0u32; 3];
                        for (k, key) in ["v1", "v2", "v3"].iter().enumerate() {
                            let i = parse_u32(attr(&t, key), &format!("{what}: triangle {}", triangles.len()))?;
                            if i as usize >= positions.len() {
                                return Err(format!(
                                    "{what}: triangle {} refers to vertex {i}, but the mesh has {}",
                                    triangles.len(),
                                    positions.len()
                                ));
                            }
                            tri[k] = i;
                        }
                        triangles.push(tri);
                        triangles_total += 1;
                        if triangles_total > MAX_TRIANGLES {
                            return Err(format!("{part}: more than {MAX_TRIANGLES} triangles"));
                        }
                    }
                }
                Kind::Mesh { positions, triangles }
            } else if let Some(cs) = o.children().find(|n| n.tag_name().name() == "components") {
                let mut list = Vec::new();
                for c in cs.children().filter(|n| n.tag_name().name() == "component") {
                    let objectid = parse_u32(attr(&c, "objectid"), &format!("{what}: a <component>"))?;
                    let path = attr(&c, "path").map(|p| p.trim_start_matches('/').to_owned());
                    let transform = parse_transform(attr(&c, "transform"), &format!("{what}: component {objectid}"))?;
                    list.push(Component { path, objectid, transform });
                }
                Kind::Components(list)
            } else {
                // An object with neither: legal for `other` types; nothing
                // to import.
                Kind::Components(Vec::new())
            };
            if objects.insert(id, Object { name, kind }).is_some() {
                return Err(format!("{what} is declared twice"));
            }
        }
    }
    let mut build = Vec::new();
    if let Some(b) = root.children().find(|n| n.tag_name().name() == "build") {
        for (k, item) in b.children().filter(|n| n.tag_name().name() == "item").enumerate() {
            let objectid = parse_u32(attr(&item, "objectid"), &format!("{part}: build item {k}"))?;
            let transform = parse_transform(attr(&item, "transform"), &format!("{part}: build item {k}"))?;
            build.push(Item { objectid, transform });
        }
    }
    Ok(Model { scale, objects, build })
}

impl<'a> Package<'a> {
    fn model(&mut self, part: &str) -> Result<&Model, String> {
        if !self.models.contains_key(part) {
            let text = self
                .archive
                .text(part)
                .ok_or_else(|| format!("the package has no model part {part:?} (it has: {})", self.list()))?;
            let m = parse_model(&text, part)?;
            self.models.insert(part.to_owned(), m);
        }
        Ok(&self.models[part])
    }

    fn list(&self) -> String {
        let mut names: Vec<&str> = self.archive.names.iter().map(|s| s.as_str()).take(12).collect();
        names.sort();
        let more = self.archive.names.len().saturating_sub(names.len());
        let mut s = names.join(", ");
        if more > 0 {
            s.push_str(&format!(", … and {more} more"));
        }
        s
    }
}

/// The root model's part name, from the package relationships.
fn root_part(archive: &zip::Archive) -> String {
    if let Some(rels) = archive.text("_rels/.rels")
        && let Ok(doc) = Document::parse_with_options(&rels, ParsingOptions { allow_dtd: false, ..ParsingOptions::default() })
    {
        for r in doc.root_element().children().filter(|n| n.tag_name().name() == "Relationship") {
            if attr(&r, "Type").is_some_and(|t| t.ends_with("/3dmodel"))
                && let Some(target) = attr(&r, "Target")
            {
                return target.trim_start_matches('/').to_owned();
            }
        }
    }
    cadpole_export_3mf::MODEL_PART.to_owned()
}

/// Bambu's `model_settings.config`: object id → name and part id → name.
fn bambu_names(archive: &zip::Archive) -> (HashMap<u32, String>, HashMap<u32, String>) {
    let mut objects = HashMap::new();
    let mut parts = HashMap::new();
    let Some(text) = archive.text(cadpole_export_3mf::BAMBU_MODEL_CONFIG) else { return (objects, parts) };
    let Ok(doc) = Document::parse_with_options(&text, ParsingOptions { allow_dtd: false, ..ParsingOptions::default() })
    else {
        return (objects, parts);
    };
    let name_of = |n: &Node<'_, '_>| -> Option<String> {
        n.children()
            .find(|m| m.tag_name().name() == "metadata" && attr(m, "key") == Some("name"))
            .and_then(|m| attr(&m, "value").map(str::to_owned))
    };
    for o in doc.root_element().children().filter(|n| n.tag_name().name() == "object") {
        if let Some(id) = attr(&o, "id").and_then(|s| s.parse::<u32>().ok()) {
            if let Some(name) = name_of(&o) {
                objects.insert(id, name);
            }
            for p in o.children().filter(|n| n.tag_name().name() == "part") {
                if let Some(pid) = attr(&p, "id").and_then(|s| s.parse::<u32>().ok())
                    && let Some(name) = name_of(&p)
                {
                    parts.insert(pid, name);
                }
            }
        }
    }
    (objects, parts)
}

/// A group of a body-to-be: one mesh, placed.
struct Piece {
    name: String,
    positions: Vec<[f64; 3]>,
    triangles: Vec<[u32; 3]>,
}

struct Walk<'p, 'a> {
    pkg: &'p mut Package<'a>,
    part_names: &'p HashMap<u32, String>,
    /// `import_mesh(units=)`: millimetres per unit, replacing every model
    /// part's `<model unit>`.
    unit_mm: Option<f64>,
    /// Declared units the override replaced, in mm per unit.
    replaced: Vec<f64>,
    pieces: Vec<Piece>,
    /// `(part, object id)` on the current path, for the cycle check.
    stack: Vec<(String, u32)>,
    /// Triangles placed so far in the build, every item included.
    total: usize,
    /// Vertices placed so far: a mesh with vertices and no triangles still
    /// costs its vertices at every placement.
    vertices: usize,
    /// Objects visited so far, meshes and component lists alike. Shared
    /// sub-objects are walked once per reference, so nested components
    /// multiply: eight levels of eight is 16.8 M visits from a kilobyte,
    /// and an empty leaf mesh never moves the triangle count.
    placements: usize,
}

impl Walk<'_, '_> {
    fn object(&mut self, part: &str, id: u32, xf: &Xf, depth: usize, top: bool) -> Result<(), String> {
        if depth > MAX_COMPONENT_DEPTH {
            return Err(format!("components nest more than {MAX_COMPONENT_DEPTH} deep at {part} object {id}"));
        }
        if self.stack.iter().any(|(p, i)| p == part && *i == id) {
            return Err(format!("{part} object {id} is a component of itself (a component cycle)"));
        }
        self.placements += 1;
        if self.placements > MAX_PLACEMENTS {
            return Err(format!(
                "more than {MAX_PLACEMENTS} component placements in the build (at {part} object {id}); \
                 components that reference shared objects multiply at every level"
            ));
        }
        let declared = self.pkg.model(part)?.scale;
        let scale = match self.unit_mm {
            Some(u) => {
                if declared != u && !self.replaced.contains(&declared) {
                    self.replaced.push(declared);
                }
                u
            }
            None => declared,
        };
        let model = self.pkg.model(part)?;
        let Some(obj) = model.objects.get(&id) else {
            return Err(format!("{part} has no object {id}"));
        };
        match &obj.kind {
            Kind::Mesh { positions, triangles } => {
                self.total += triangles.len();
                if self.total > MAX_TRIANGLES {
                    return Err(format!("more than {MAX_TRIANGLES} triangles in the build"));
                }
                self.vertices += positions.len();
                if self.vertices > MAX_VERTICES {
                    return Err(format!("more than {MAX_VERTICES} vertices in the build"));
                }
                // `xf` is in millimetres; the mesh's coordinates are in its
                // own part's unit until they are scaled.
                let positions: Vec<[f64; 3]> =
                    positions.iter().map(|&p| apply(xf, [p[0] * scale, p[1] * scale, p[2] * scale])).collect();
                let name = if top {
                    "surface".to_owned()
                } else {
                    self.part_names.get(&id).cloned().unwrap_or_else(|| obj.name.clone())
                };
                self.pieces.push(Piece { name, positions, triangles: triangles.clone() });
                Ok(())
            }
            Kind::Components(list) => {
                if list.is_empty() {
                    return Err(format!("{part} object {id} has neither a mesh nor components"));
                }
                let comps: Vec<(Option<String>, u32, Xf)> =
                    list.iter().map(|c| (c.path.clone(), c.objectid, c.transform)).collect();
                self.stack.push((part.to_owned(), id));
                for (path, cid, cxf) in comps {
                    let child_part = path.unwrap_or_else(|| part.to_owned());
                    // The component's translation is in this part's unit
                    // (`<model unit>` covers every length in the part).
                    let composed = compose(&in_mm(&cxf, scale), xf);
                    self.object(&child_part, cid, &composed, depth + 1, false)?;
                }
                self.stack.pop();
                Ok(())
            }
        }
    }
}

/// Parse `bytes` as a 3MF package. `hint` names the file, for diagnostics.
pub fn parse(bytes: &[u8], hint: &str) -> Result<(Vec<RawObject>, Vec<Note>), String> {
    parse_with_unit(bytes, hint, None)
}

/// [`parse`], with `unit_mm` millimetres per unit in place of every model
/// part's own `<model unit>` when it is given (`import_mesh(units=)`).
pub fn parse_with_unit(bytes: &[u8], _hint: &str, unit_mm: Option<f64>) -> Result<(Vec<RawObject>, Vec<Note>), String> {
    let archive = zip::read_with(bytes, &prescan::zip_caps())?;
    let root = root_part(&archive);
    let mut pkg = Package { archive: &archive, models: HashMap::new() };
    let (object_names, part_names) = bambu_names(&archive);
    let mut notes = Vec::new();
    let bambu = !object_names.is_empty() || !part_names.is_empty();
    if bambu {
        notes.push(Note::info("a Bambu Studio project: object and part names are read from Metadata/model_settings.config"));
    }
    if archive.get(&root).is_none() {
        return Err(format!("not a 3MF package: no {root} model part (the zip has: {})", pkg.list()));
    }
    let root_model = pkg.model(&root)?;
    // Item transforms are in the root part's unit (or `units=`); the walk
    // takes them in millimetres.
    let root_scale = unit_mm.unwrap_or(root_model.scale);
    let mut items: Vec<(u32, Xf)> =
        root_model.build.iter().map(|i| (i.objectid, in_mm(&i.transform, root_scale))).collect();
    if items.is_empty() {
        // No build: every mesh object where it is, with a note.
        let mut ids: Vec<u32> =
            root_model.objects.iter().filter(|(_, o)| matches!(o.kind, Kind::Mesh { .. })).map(|(id, _)| *id).collect();
        ids.sort_unstable();
        if ids.is_empty() {
            return Err("the 3MF has no build items and no mesh objects".into());
        }
        notes.push(Note::warning(format!(
            "the 3MF has no <build> items; its {} mesh object{} imported where {} defined",
            ids.len(),
            if ids.len() == 1 { " is" } else { "s are" },
            if ids.len() == 1 { "it is" } else { "they are" }
        )));
        items = ids.into_iter().map(|id| (id, IDENTITY)).collect();
    }
    let mut out = Vec::with_capacity(items.len());
    let mut seen: HashSet<u32> = HashSet::new();
    let mut replaced: Vec<f64> = Vec::new();
    // Triangles, vertices and placements, counted over the whole build:
    // every item places its object again, so a per-item count would let
    // N items of one object carry N times the cap.
    let (mut total, mut vertices, mut placements) = (0usize, 0usize, 0usize);
    for (k, (id, xf)) in items.iter().enumerate() {
        let mut walk = Walk {
            pkg: &mut pkg,
            part_names: &part_names,
            unit_mm,
            replaced: Vec::new(),
            pieces: Vec::new(),
            stack: Vec::new(),
            total,
            vertices,
            placements,
        };
        walk.object(&root, *id, xf, 0, true).map_err(|e| format!("build item {k}: {e}"))?;
        (total, vertices, placements) = (walk.total, walk.vertices, walk.placements);
        for r in walk.replaced {
            if !replaced.contains(&r) {
                replaced.push(r);
            }
        }
        let pieces = walk.pieces;
        if pieces.iter().all(|p| p.triangles.is_empty()) {
            return Err(format!("build item {k} (object {id}) has no triangles"));
        }
        let obj_name = object_names
            .get(id)
            .cloned()
            .or_else(|| pkg.models.get(&root).and_then(|m| m.objects.get(id)).map(|o| o.name.clone()))
            .unwrap_or_default();
        let mut o = RawObject { name: obj_name, ..Default::default() };
        let multi = pieces.len() > 1;
        for (j, p) in pieces.into_iter().enumerate() {
            let base = o.positions.len() as u32;
            let g = o.groups.len() as u32;
            o.groups.push(if p.name.is_empty() { format!("part[{j}]") } else { p.name });
            o.positions.extend(p.positions);
            for t in p.triangles {
                o.triangles.push([t[0] + base, t[1] + base, t[2] + base]);
                o.tri_group.push(g);
            }
        }
        if !multi {
            // One mesh: the group is `surface`, unnamed here, so the caller
            // names it as every single-group body is named.
            o.groups.clear();
            o.tri_group.clear();
        }
        if !seen.insert(*id) {
            notes.push(Note::info(format!("object {id} is built more than once; each item is its own body")));
        }
        out.push(o);
    }
    if let (Some(u), false) = (unit_mm, replaced.is_empty()) {
        let was: Vec<String> = replaced.iter().map(|r| format!("{r} mm")).collect();
        notes.push(Note::info(format!(
            "units= says one unit is {u} mm, replacing the <model unit> the package declared ({} per unit)",
            was.join(", ")
        )));
    }
    Ok((out, notes))
}
