//! What a Bambu Studio project has to be: read back through a zip reader and an
//! XML parser, and held to the rules those files share. The same reader and
//! the same rules run over a real Bambu-written file when
//! `CADPOLE_BAMBU_SAMPLE` names one, which is how the rules are known to
//! describe Bambu's output and not only ours.

use std::collections::{BTreeMap, BTreeSet, HashSet};

use cadpole_core::geom::Vec3;
use cadpole_core::tessellate::Mesh;

use super::*;
use crate::tests::{box_mesh, edge_uses, open, palette};

// ---------------------------------------------------------------------------
// The reader
// ---------------------------------------------------------------------------

const PROD: &str = PRODUCTION_NS;

#[derive(Debug, Default)]
pub(crate) struct Structure {
    pub names: Vec<String>,
    /// `(Extension, ContentType)` defaults.
    pub content_types: Vec<(String, String)>,
    /// `(Type, Target)` of `_rels/.rels`.
    pub rels: Vec<(String, String)>,
    /// Targets of `3D/_rels/3dmodel.model.rels`.
    pub model_rels: Vec<String>,
    pub root: RootModel,
    pub files: BTreeMap<String, ObjectFile>,
    pub settings: Settings,
    /// `filament_colour` of `project_settings.config`, when the part exists.
    pub filament_colour: Option<Vec<String>>,
    /// `printable_area` of `project_settings.config`, as `[w, h]`.
    pub printable_area: Option<[f64; 2]>,
    /// The whole `project_settings.config`, when it is a JSON object.
    pub settings_json: Option<serde_json::Map<String, serde_json::Value>>,
    /// Every PNG in the package with its size.
    pub pngs: BTreeMap<String, (u32, u32)>,
    pub has_slice_info: bool,
}

#[derive(Debug, Default)]
pub(crate) struct RootModel {
    pub required_extensions: Option<String>,
    pub metadata: Vec<(String, String)>,
    pub objects: Vec<RootObject>,
    pub build_uuid: Option<String>,
    pub items: Vec<Item>,
}

#[derive(Debug)]
pub(crate) struct RootObject {
    pub id: u32,
    pub uuid: Option<String>,
    pub has_mesh: bool,
    pub components: Vec<Component>,
}

#[derive(Debug)]
pub(crate) struct Component {
    pub path: Option<String>,
    pub objectid: u32,
    pub uuid: Option<String>,
    pub transform: Vec<f64>,
}

#[derive(Debug)]
pub(crate) struct Item {
    pub objectid: u32,
    pub uuid: Option<String>,
    pub transform: Vec<f64>,
    pub printable: Option<String>,
}

#[derive(Debug, Default)]
pub(crate) struct ObjectFile {
    pub required_extensions: Option<String>,
    pub version_metadata: Option<String>,
    pub materials: Vec<(String, String)>,
    pub materials_id: Option<u32>,
    pub meshes: Vec<MeshObject>,
    pub build_items: usize,
    pub xml: String,
}

#[derive(Debug)]
pub(crate) struct MeshObject {
    pub id: u32,
    pub uuid: Option<String>,
    pub pid: Option<u32>,
    pub pindex: Option<u32>,
    pub verts: Vec<[f64; 3]>,
    /// `(v1 v2 v3, p1, paint_color)`.
    pub tris: Vec<([u32; 3], Option<u32>, Option<String>)>,
}

#[derive(Debug, Default)]
pub(crate) struct Settings {
    pub objects: Vec<CfgObject>,
    pub plates: Vec<CfgPlate>,
    /// `(object_id, transform)` of the assembly view.
    pub assemble: Vec<(u32, Vec<f64>)>,
}

#[derive(Debug)]
pub(crate) struct CfgObject {
    pub id: u32,
    pub name: String,
    pub extruder: Option<u32>,
    pub parts: Vec<CfgPart>,
}

#[derive(Debug)]
pub(crate) struct CfgPart {
    pub id: u32,
    pub subtype: Option<String>,
    pub name: String,
    pub matrix: Vec<f64>,
    pub extruder: Option<u32>,
}

#[derive(Debug)]
pub(crate) struct CfgPlate {
    pub id: u32,
    pub name: String,
    pub thumbnail: Option<String>,
    /// `(object_id, instance_id, identify_id)`.
    pub instances: Vec<(u32, u32, Option<String>)>,
}

fn numbers(s: &str) -> Vec<f64> {
    s.split_whitespace().map(|n| n.parse::<f64>().unwrap_or_else(|_| panic!("not a number in {s:?}"))).collect()
}

fn attr_u32(n: roxmltree::Node, name: &str) -> Option<u32> {
    n.attribute(name).map(|v| v.parse().unwrap_or_else(|_| panic!("{name}={v:?} is not an integer")))
}

fn png_size(bytes: &[u8]) -> Option<(u32, u32)> {
    if bytes.len() < 24 || bytes[..8] != [0x89, b'P', b'N', b'G', 0x0d, 0x0a, 0x1a, 0x0a] {
        return None;
    }
    let w = u32::from_be_bytes([bytes[16], bytes[17], bytes[18], bytes[19]]);
    let h = u32::from_be_bytes([bytes[20], bytes[21], bytes[22], bytes[23]]);
    Some((w, h))
}

/// The strings of the JSON array under `key` in `project_settings.config`.
fn json_string_array(settings: &serde_json::Map<String, serde_json::Value>, key: &str) -> Option<Vec<String>> {
    Some(settings.get(key)?.as_array()?.iter().filter_map(|v| v.as_str().map(str::to_owned)).collect())
}

pub(crate) fn read_structure(bytes: &[u8]) -> Structure {
    let z = zip::read(bytes).expect("a readable package");
    let mut s = Structure { names: z.names.clone(), ..Default::default() };

    for n in &z.names {
        if n.ends_with(".png") {
            let size = png_size(z.get(n).unwrap()).unwrap_or_else(|| panic!("{n} is not a PNG"));
            s.pngs.insert(n.clone(), size);
        }
    }
    s.has_slice_info = z.get(BAMBU_SLICE_INFO).is_some();

    let ct = z.text(CONTENT_TYPES_PART).expect("[Content_Types].xml");
    let doc = roxmltree::Document::parse(&ct).unwrap();
    for d in doc.descendants().filter(|n| n.has_tag_name("Default")) {
        s.content_types.push((d.attribute("Extension").unwrap().to_owned(), d.attribute("ContentType").unwrap().to_owned()));
    }
    let rels = z.text(RELS_PART).expect("_rels/.rels");
    let doc = roxmltree::Document::parse(&rels).unwrap();
    for r in doc.descendants().filter(|n| n.has_tag_name("Relationship")) {
        s.rels.push((r.attribute("Type").unwrap().to_owned(), r.attribute("Target").unwrap().to_owned()));
    }
    if let Some(mr) = z.text(MODEL_RELS_PART) {
        let doc = roxmltree::Document::parse(&mr).unwrap();
        for r in doc.descendants().filter(|n| n.has_tag_name("Relationship")) {
            assert_eq!(r.attribute("Type"), Some(MODEL_REL));
            s.model_rels.push(r.attribute("Target").unwrap().to_owned());
        }
    }

    let root = z.text(MODEL_PART).expect("3D/3dmodel.model");
    let doc = roxmltree::Document::parse(&root).expect("the root model is well-formed XML");
    let model = doc.root_element();
    assert_eq!(model.tag_name().namespace(), Some(CORE_NS));
    s.root.required_extensions = model.attribute("requiredextensions").map(str::to_owned);
    for m in model.children().filter(|n| n.has_tag_name("metadata")) {
        s.root.metadata.push((m.attribute("name").unwrap().to_owned(), m.text().unwrap_or("").to_owned()));
    }
    let resources = model.children().find(|n| n.has_tag_name("resources")).expect("<resources>");
    for o in resources.children().filter(|n| n.has_tag_name("object")) {
        let components = o
            .children()
            .find(|n| n.has_tag_name("components"))
            .map(|c| {
                c.children()
                    .filter(|n| n.has_tag_name("component"))
                    .map(|n| Component {
                        path: n.attribute((PROD, "path")).map(str::to_owned),
                        objectid: attr_u32(n, "objectid").expect("objectid"),
                        uuid: n.attribute((PROD, "UUID")).map(str::to_owned),
                        transform: n.attribute("transform").map(numbers).unwrap_or_default(),
                    })
                    .collect()
            })
            .unwrap_or_default();
        s.root.objects.push(RootObject {
            id: attr_u32(o, "id").expect("id"),
            uuid: o.attribute((PROD, "UUID")).map(str::to_owned),
            has_mesh: o.children().any(|n| n.has_tag_name("mesh")),
            components,
        });
    }
    let build = model.children().find(|n| n.has_tag_name("build")).expect("<build>");
    s.root.build_uuid = build.attribute((PROD, "UUID")).map(str::to_owned);
    for it in build.children().filter(|n| n.has_tag_name("item")) {
        s.root.items.push(Item {
            objectid: attr_u32(it, "objectid").expect("objectid"),
            uuid: it.attribute((PROD, "UUID")).map(str::to_owned),
            transform: it.attribute("transform").map(numbers).unwrap_or_default(),
            printable: it.attribute("printable").map(str::to_owned),
        });
    }

    for n in z.names.iter().filter(|n| n.starts_with("3D/Objects/") && n.ends_with(".model")) {
        let xml = z.text(n).unwrap();
        let doc = roxmltree::Document::parse(&xml).unwrap_or_else(|e| panic!("{n}: {e}"));
        let model = doc.root_element();
        let mut f = ObjectFile {
            required_extensions: model.attribute("requiredextensions").map(str::to_owned),
            version_metadata: model
                .children()
                .find(|m| m.has_tag_name("metadata") && m.attribute("name") == Some("BambuStudio:3mfVersion"))
                .and_then(|m| m.text())
                .map(str::to_owned),
            ..Default::default()
        };
        let resources = model.children().find(|c| c.has_tag_name("resources")).expect("<resources>");
        for r in resources.children().filter(|c| c.is_element()) {
            if r.has_tag_name("basematerials") {
                f.materials_id = attr_u32(r, "id");
                for b in r.children().filter(|c| c.has_tag_name("base")) {
                    f.materials.push((b.attribute("name").unwrap().to_owned(), b.attribute("displaycolor").unwrap().to_owned()));
                }
            } else if r.has_tag_name("object") {
                let mesh = r.children().find(|c| c.has_tag_name("mesh")).expect("a mesh object in an object file");
                let verts = mesh
                    .children()
                    .find(|c| c.has_tag_name("vertices"))
                    .expect("<vertices>")
                    .children()
                    .filter(|c| c.has_tag_name("vertex"))
                    .map(|v| {
                        [
                            v.attribute("x").unwrap().parse().unwrap(),
                            v.attribute("y").unwrap().parse().unwrap(),
                            v.attribute("z").unwrap().parse().unwrap(),
                        ]
                    })
                    .collect();
                let tris = mesh
                    .children()
                    .find(|c| c.has_tag_name("triangles"))
                    .expect("<triangles>")
                    .children()
                    .filter(|c| c.has_tag_name("triangle"))
                    .map(|t| {
                        (
                            [attr_u32(t, "v1").unwrap(), attr_u32(t, "v2").unwrap(), attr_u32(t, "v3").unwrap()],
                            attr_u32(t, "p1"),
                            t.attribute("paint_color").map(str::to_owned),
                        )
                    })
                    .collect();
                f.meshes.push(MeshObject {
                    id: attr_u32(r, "id").expect("id"),
                    uuid: r.attribute((PROD, "UUID")).map(str::to_owned),
                    pid: attr_u32(r, "pid"),
                    pindex: attr_u32(r, "pindex"),
                    verts,
                    tris,
                });
            }
        }
        f.build_items = model
            .children()
            .find(|c| c.has_tag_name("build"))
            .map(|b| b.children().filter(|c| c.has_tag_name("item")).count())
            .unwrap_or(0);
        f.xml = xml;
        s.files.insert(format!("/{n}"), f);
    }

    let cfg = z.text(BAMBU_MODEL_CONFIG).expect("Metadata/model_settings.config");
    let doc = roxmltree::Document::parse(&cfg).expect("model_settings.config is well-formed XML");
    let kv = |n: roxmltree::Node, key: &str| -> Option<String> {
        n.children()
            .find(|m| m.has_tag_name("metadata") && m.attribute("key") == Some(key))
            .and_then(|m| m.attribute("value"))
            .map(str::to_owned)
    };
    for o in doc.root_element().children().filter(|n| n.has_tag_name("object")) {
        let parts = o
            .children()
            .filter(|n| n.has_tag_name("part"))
            .map(|p| CfgPart {
                id: attr_u32(p, "id").expect("part id"),
                subtype: p.attribute("subtype").map(str::to_owned),
                name: kv(p, "name").unwrap_or_default(),
                matrix: kv(p, "matrix").map(|m| numbers(&m)).unwrap_or_default(),
                extruder: kv(p, "extruder").map(|e| e.parse().unwrap()),
            })
            .collect();
        s.settings.objects.push(CfgObject {
            id: attr_u32(o, "id").expect("object id"),
            name: kv(o, "name").unwrap_or_default(),
            extruder: kv(o, "extruder").map(|e| e.parse().unwrap()),
            parts,
        });
    }
    for p in doc.root_element().children().filter(|n| n.has_tag_name("plate")) {
        let instances = p
            .children()
            .filter(|n| n.has_tag_name("model_instance"))
            .map(|mi| {
                (
                    kv(mi, "object_id").unwrap().parse().unwrap(),
                    kv(mi, "instance_id").unwrap().parse().unwrap(),
                    kv(mi, "identify_id"),
                )
            })
            .collect();
        s.settings.plates.push(CfgPlate {
            id: kv(p, "plater_id").expect("plater_id").parse().unwrap(),
            name: kv(p, "plater_name").unwrap_or_default(),
            thumbnail: kv(p, "thumbnail_file"),
            instances,
        });
    }
    if let Some(a) = doc.root_element().children().find(|n| n.has_tag_name("assemble")) {
        for it in a.children().filter(|n| n.has_tag_name("assemble_item")) {
            s.settings.assemble.push((attr_u32(it, "object_id").unwrap(), numbers(it.attribute("transform").unwrap())));
        }
    }

    if let Some(text) = z.text(BAMBU_PROJECT_CONFIG) {
        let Ok(serde_json::Value::Object(ps)) = serde_json::from_str::<serde_json::Value>(&text) else {
            panic!("project_settings.config is not a JSON object");
        };
        s.filament_colour = json_string_array(&ps, "filament_colour");
        s.printable_area = json_string_array(&ps, "printable_area").and_then(|corners| {
            let mut w: f64 = 0.0;
            let mut h: f64 = 0.0;
            for c in corners {
                let (x, y) = c.split_once('x')?;
                w = w.max(x.parse().ok()?);
                h = h.max(y.parse().ok()?);
            }
            Some([w, h])
        });
        s.settings_json = Some(ps);
    }
    s
}

// ---------------------------------------------------------------------------
// The rules
// ---------------------------------------------------------------------------

fn is_uuid(s: &str) -> bool {
    let groups: Vec<&str> = s.split('-').collect();
    groups.len() == 5
        && [8, 4, 4, 4, 12].iter().zip(&groups).all(|(n, g)| g.len() == *n && g.chars().all(|c| c.is_ascii_hexdigit()))
}

/// The rules every Bambu-written sample satisfies, and so must we. `bed`
/// is the plate grid's bed; `None` takes the file's `printable_area` or the
/// default.
pub(crate) fn check_common(s: &Structure, bed: Option<[f64; 2]>) {
    let bed = bed.or(s.printable_area).unwrap_or(DEFAULT_BED);

    // The package.
    for part in [CONTENT_TYPES_PART, RELS_PART, MODEL_PART, MODEL_RELS_PART, BAMBU_MODEL_CONFIG] {
        assert!(s.names.contains(&part.to_owned()), "missing {part}; have {:?}", s.names);
    }
    for (ext, ty) in [("rels", "application/vnd.openxmlformats-package.relationships+xml"), ("model", "application/vnd.ms-package.3dmanufacturing-3dmodel+xml")] {
        assert!(s.content_types.iter().any(|(e, t)| e == ext && t == ty), "content type for .{ext}: {:?}", s.content_types);
    }
    if !s.pngs.is_empty() {
        assert!(s.content_types.iter().any(|(e, t)| e == "png" && t == "image/png"), "{:?}", s.content_types);
    }
    let model_rels: Vec<&String> = s.rels.iter().filter(|(t, _)| t == MODEL_REL).map(|(_, target)| target).collect();
    assert_eq!(model_rels, vec![&format!("/{MODEL_PART}")], "one start-part relationship");
    for (ty, target) in &s.rels {
        if ty == THUMBNAIL_REL || ty == COVER_MIDDLE_REL || ty == COVER_SMALL_REL {
            assert!(s.pngs.contains_key(target.trim_start_matches('/')), "thumbnail {target} is a PNG in the package");
        }
    }

    // The root model.
    assert!(
        s.root.required_extensions.as_deref().is_some_and(|r| r.split_whitespace().any(|x| x == "p")),
        "requiredextensions names the Production extension: {:?}",
        s.root.required_extensions
    );
    assert!(s.root.metadata.iter().any(|(n, v)| n == "BambuStudio:3mfVersion" && v == "1"), "{:?}", s.root.metadata);
    assert!(s.root.build_uuid.as_deref().is_some_and(is_uuid), "build UUID: {:?}", s.root.build_uuid);
    assert!(!s.root.objects.is_empty(), "a project has objects");
    let mut uuids: HashSet<String> = HashSet::new();
    let mut ids: HashSet<u32> = HashSet::new();
    let mut note = |u: &Option<String>, what: &str| {
        let u = u.as_deref().unwrap_or_else(|| panic!("{what} has no p:UUID"));
        assert!(is_uuid(u), "{what}: {u:?} is not a UUID");
        assert!(uuids.insert(u.to_owned()), "{what}: UUID {u} used twice");
    };
    note(&s.root.build_uuid, "build");
    for o in &s.root.objects {
        assert!(!o.has_mesh, "root object {} is a components object, never a mesh", o.id);
        assert!(!o.components.is_empty(), "root object {} has components", o.id);
        assert!(ids.insert(o.id), "object id {} used twice", o.id);
        note(&o.uuid, &format!("object {}", o.id));
        for c in &o.components {
            note(&c.uuid, &format!("component {} of object {}", c.objectid, o.id));
            assert_eq!(c.transform.len(), 12, "a 12-number component transform");
            let path = c.path.as_deref().unwrap_or_else(|| panic!("component of object {} has no p:path", o.id));
            assert!(s.model_rels.contains(&path.to_owned()), "{path} is listed in {MODEL_RELS_PART}: {:?}", s.model_rels);
            let f = s.files.get(path).unwrap_or_else(|| panic!("{path} is in the package"));
            assert!(f.meshes.iter().any(|m| m.id == c.objectid), "{path} holds object {}", c.objectid);
        }
    }
    for it in &s.root.items {
        note(&it.uuid, &format!("item {}", it.objectid));
        assert_eq!(it.transform.len(), 12);
        assert_eq!(it.printable.as_deref(), Some("1"));
        assert!(ids.contains(&it.objectid), "item names root object {}", it.objectid);
    }
    for o in &s.root.objects {
        assert_eq!(s.root.items.iter().filter(|i| i.objectid == o.id).count(), 1, "object {} is built exactly once", o.id);
    }

    // The object files.
    for (path, f) in &s.files {
        assert!(f.required_extensions.as_deref().is_some_and(|r| r.split_whitespace().any(|x| x == "p")), "{path}");
        assert_eq!(f.version_metadata.as_deref(), Some("1"), "{path} carries BambuStudio:3mfVersion");
        assert_eq!(f.build_items, 0, "{path}: a child model's build is empty");
        for m in &f.meshes {
            assert!(ids.insert(m.id), "{path}: object id {} collides with another resource", m.id);
            note(&m.uuid, &format!("{path} object {}", m.id));
            assert!(m.verts.len() >= 4 && m.tris.len() >= 4, "{path} object {}: a solid", m.id);
            for (v, p1, _) in &m.tris {
                assert!(v.iter().all(|&i| (i as usize) < m.verts.len()), "{path}: index out of range");
                assert!(v[0] != v[1] && v[1] != v[2] && v[0] != v[2], "{path}: distinct vertices");
                if let Some(p) = p1 {
                    assert!((*p as usize) < f.materials.len(), "{path}: p1 {p} past the materials");
                    assert!(m.pid.is_some() && m.pindex.is_some(), "{path}: pid/pindex on a propertied object");
                }
            }
            if let Some(pid) = m.pid {
                assert_eq!(Some(pid), f.materials_id, "{path}: pid names the file's basematerials");
                assert!((m.pindex.unwrap() as usize) < f.materials.len());
            }
            // Bambu centres every part's mesh on its own bounding box; the
            // component and item transforms put it back.
            let (mut lo, mut hi) = ([f64::MAX; 3], [f64::MIN; 3]);
            for v in &m.verts {
                for k in 0..3 {
                    lo[k] = lo[k].min(v[k]);
                    hi[k] = hi[k].max(v[k]);
                }
            }
            for k in 0..3 {
                let c = (lo[k] + hi[k]) * 0.5;
                assert!(c.abs() < 1e-2, "{path} object {}: bounding-box centre {c} on axis {k} is not the origin", m.id);
            }
        }
    }

    // model_settings.config against the model.
    let root_ids: BTreeSet<u32> = s.root.objects.iter().map(|o| o.id).collect();
    let cfg_ids: BTreeSet<u32> = s.settings.objects.iter().map(|o| o.id).collect();
    assert_eq!(cfg_ids, root_ids, "the settings describe exactly the root objects");
    for o in &s.settings.objects {
        let root = s.root.objects.iter().find(|r| r.id == o.id).unwrap();
        let comps: BTreeSet<u32> = root.components.iter().map(|c| c.objectid).collect();
        let parts: BTreeSet<u32> = o.parts.iter().map(|p| p.id).collect();
        assert_eq!(parts, comps, "object {}: a part per component, by the mesh object's id", o.id);
        assert!(o.extruder.is_some_and(|e| e >= 1), "object {} names its extruder", o.id);
        for p in &o.parts {
            // `normal_part` is a mesh that prints; a 01.08 sample also has
            // `modifier_part` (a settings modifier volume). Ours are all
            // normal parts (`check_ours`).
            assert!(
                matches!(p.subtype.as_deref(), Some("normal_part" | "modifier_part" | "negative_part")),
                "object {} part {}: subtype {:?}",
                o.id,
                p.id,
                p.subtype
            );
            assert_eq!(p.matrix.len(), 16, "a 4x4 part matrix");
            assert!(p.extruder.is_none_or(|e| e >= 1));
        }
    }
    let plate_ids: Vec<u32> = s.settings.plates.iter().map(|p| p.id).collect();
    assert_eq!(plate_ids, (1..=s.settings.plates.len() as u32).collect::<Vec<_>>(), "plates are numbered 1..N in order");
    for it in &s.root.items {
        let on: Vec<&CfgPlate> = s.settings.plates.iter().filter(|p| p.instances.iter().any(|(o, _, _)| *o == it.objectid)).collect();
        assert_eq!(on.len(), 1, "object {} is on exactly one plate", it.objectid);
        let plate = on[0];
        let origin = plate_origin(plate.id, s.settings.plates.len() as u32, bed);
        let (x, y) = (it.transform[9] - origin[0], it.transform[10] - origin[1]);
        assert!(
            x >= -1.0 && x <= bed[0] + 1.0 && y >= -1.0 && y <= bed[1] + 1.0,
            "object {} is placed on plate {} of a {}x{} bed: local ({x:.1}, {y:.1})",
            it.objectid,
            plate.id,
            bed[0],
            bed[1],
        );
    }
    for p in &s.settings.plates {
        for (_, inst, _) in &p.instances {
            assert_eq!(*inst, 0);
        }
        if let Some(t) = &p.thumbnail {
            assert!(s.pngs.contains_key(t), "plate {}'s thumbnail {t} is in the package", p.id);
        }
    }
    if let Some(colours) = &s.filament_colour {
        assert!(!colours.is_empty());
        for c in colours {
            assert!(c.len() == 7 && c.starts_with('#') && c[1..].chars().all(|ch| ch.is_ascii_hexdigit()), "{c:?}");
        }
        for o in &s.settings.objects {
            for e in o.parts.iter().filter_map(|p| p.extruder).chain(o.extruder) {
                assert!((e as usize) <= colours.len(), "extruder {e} has a colour in the list of {}", colours.len());
            }
        }
    }
    for (_, size) in s.pngs.iter().filter(|(n, _)| n.starts_with("Metadata/plate_")) {
        assert!(
            *size == (THUMBNAIL_PX, THUMBNAIL_PX) || *size == (THUMBNAIL_SMALL_PX, THUMBNAIL_SMALL_PX),
            "plate thumbnails are 512 or 128 square: {size:?}"
        );
    }
}

/// The stricter rules our own writer keeps on top of [`check_common`]:
/// closed meshes, exact centring, a part matrix that is the component
/// transform, no core colour layer, a Bambu Studio `Application` no
/// newer than OrcaSlicer opens, settings that parse, every piece inside its
/// plate's cell, and the parts we always write.
pub(crate) fn check_ours(s: &Structure, bed: [f64; 2]) {
    check_common(s, Some(bed));
    assert!(s.has_slice_info, "slice_info.config");
    assert!(s.filament_colour.is_some(), "project_settings.config with the colours");
    let settings = s.settings_json.as_ref().expect("project_settings.config parses as a JSON object");
    let app = s.root.metadata.iter().find(|(n, _)| n == "Application").map(|(_, v)| v.clone()).expect("Application");
    let v = crate::bambu_settings::version_of(&app).unwrap_or_else(|| panic!("{app:?} is a Bambu Studio version"));
    assert!((v[0], v[1]) <= crate::bambu_settings::NEWEST_FOR_ORCA, "{app} is newer than OrcaSlicer 2.4.2 opens");
    for k in ["printable_area", "printable_height", "nozzle_diameter", "filament_colour"] {
        assert!(settings.contains_key(k), "the settings carry {k}");
    }
    let n = s.filament_colour.as_ref().unwrap().len();
    for k in ["filament_diameter", "filament_is_support", "filament_shrink", "filament_start_gcode", "filament_type"] {
        if let Some(list) = settings.get(k).and_then(|v| v.as_array()) {
            assert_eq!(list.len() % n, 0, "{k} has a block per filament");
        }
    }
    for (path, f) in &s.files {
        assert!(f.materials.is_empty(), "{path}: no basematerials in a project");
        assert!(edge_uses(&f.xml).iter().all(|&(a, b)| (a, b) == (1, 1)), "{path}: every edge twice, once each way");
        for m in &f.meshes {
            assert!(m.pid.is_none() && m.pindex.is_none(), "{path} object {}: no pid/pindex", m.id);
            assert!(m.tris.iter().all(|(_, p1, _)| p1.is_none()), "{path} object {}: no p1", m.id);
        }
    }
    // Every piece wholly inside its plate's cell: the mesh, moved by its
    // component and its item, within the plate's bed.
    let plates = s.settings.plates.len() as u32;
    for p in &s.settings.plates {
        let origin = plate_origin(p.id, plates, bed);
        for (oid, _, _) in &p.instances {
            let root = s.root.objects.iter().find(|r| r.id == *oid).unwrap();
            let item = s.root.items.iter().find(|i| i.objectid == *oid).unwrap();
            for c in &root.components {
                let m = s.files[c.path.as_deref().unwrap()].meshes.iter().find(|m| m.id == c.objectid).unwrap();
                for v in &m.verts {
                    let w = [0, 1, 2].map(|k| v[k] + c.transform[9 + k] + item.transform[9 + k]);
                    assert!(
                        w[0] >= origin[0] - 1e-3
                            && w[0] <= origin[0] + bed[0] + 1e-3
                            && w[1] >= origin[1] - 1e-3
                            && w[1] <= origin[1] + bed[1] + 1e-3,
                        "object {oid} on plate {}: a vertex at ({:.2}, {:.2}) leaves the plate's cell from ({:.1}, {:.1})",
                        p.id,
                        w[0],
                        w[1],
                        origin[0],
                        origin[1]
                    );
                    assert!(w[2] >= -1e-3, "object {oid}: below the bed at z = {}", w[2]);
                }
            }
        }
    }
    // The prime tower: on a plate of more than one filament, the
    // tower the settings place -- `prime_tower_width` across, at least
    // Cadpole's 45, and as deep -- stands clear of every piece by the margin.
    let numbers = |k: &str| -> Vec<f64> {
        settings
            .get(k)
            .and_then(|v| v.as_array())
            .map(|a| a.iter().filter_map(|x| x.as_str()?.trim().parse().ok()).collect())
            .unwrap_or_default()
    };
    let (tx, ty) = (numbers("wipe_tower_x"), numbers("wipe_tower_y"));
    let width = settings.get("prime_tower_width").and_then(|v| v.as_str()).and_then(|v| v.trim().parse::<f64>().ok());
    let room = crate::bambu_settings::TowerRoom::for_width(width.unwrap_or(crate::bambu_settings::TOWER_WIDTH_DEFAULT));
    for p in &s.settings.plates {
        let mut extruders: Vec<u32> = Vec::new();
        for (oid, _, _) in &p.instances {
            if let Some(o) = s.settings.objects.iter().find(|o| o.id == *oid) {
                for e in o.parts.iter().filter_map(|part| part.extruder) {
                    if !extruders.contains(&e) {
                        extruders.push(e);
                    }
                }
            }
        }
        if extruders.len() < 2 || settings.get("enable_prime_tower").and_then(|v| v.as_str()) == Some("0") {
            continue;
        }
        let i = p.id as usize - 1;
        let (x, y) = (tx[i], ty[i]);
        let tower = [x - room.margin, y - room.margin, x + room.side + room.margin, y + room.side + room.margin];
        let origin = plate_origin(p.id, plates, bed);
        for (oid, _, _) in &p.instances {
            let root = s.root.objects.iter().find(|r| r.id == *oid).unwrap();
            let item = s.root.items.iter().find(|i| i.objectid == *oid).unwrap();
            let (mut lo, mut hi) = ([f64::MAX; 2], [f64::MIN; 2]);
            for c in &root.components {
                let m = s.files[c.path.as_deref().unwrap()].meshes.iter().find(|m| m.id == c.objectid).unwrap();
                for v in &m.verts {
                    for k in 0..2 {
                        let w = v[k] + c.transform[9 + k] + item.transform[9 + k] - origin[k];
                        lo[k] = lo[k].min(w);
                        hi[k] = hi[k].max(w);
                    }
                }
            }
            assert!(
                hi[0] <= tower[0] + 1e-3 || lo[0] >= tower[2] - 1e-3 || hi[1] <= tower[1] + 1e-3 || lo[1] >= tower[3] - 1e-3,
                "plate {}: its prime tower at ({x}, {y}) with its margin, {tower:?}, meets object {oid} over {lo:?}..{hi:?}",
                p.id
            );
        }
    }
    for o in &s.settings.objects {
        let root = s.root.objects.iter().find(|r| r.id == o.id).unwrap();
        for p in &o.parts {
            assert_eq!(p.subtype.as_deref(), Some("normal_part"));
            let c = root.components.iter().find(|c| c.objectid == p.id).unwrap();
            assert_eq!(&p.matrix[..3], &[1.0, 0.0, 0.0]);
            assert_eq!([p.matrix[3], p.matrix[7], p.matrix[11]], [c.transform[9], c.transform[10], c.transform[11]]);
            assert!(p.extruder.is_some(), "every part names its extruder");
        }
        assert_eq!(o.extruder, o.parts[0].extruder, "the object's extruder is its first part's");
    }
    assert_eq!(s.settings.assemble.len(), s.root.objects.len(), "an assembly entry per object");
    if let Some((_, target)) = s.rels.iter().find(|(t, _)| t == THUMBNAIL_REL) {
        assert!(s.root.metadata.iter().any(|(n, v)| n == "Thumbnail_Middle" && format!("/{}", v.trim_start_matches('/')) == *target));
    }
}

// ---------------------------------------------------------------------------
// Fixtures
// ---------------------------------------------------------------------------

/// A valid PNG of `w`×`h` opaque pixels, built by hand: the signature, an
/// IHDR, one zlib-deflated IDAT and an IEND, each chunk CRC'd with the zip
/// module's CRC-32. The tests need the size a reader parses, not a picture.
fn tiny_png(w: u32, h: u32) -> Vec<u8> {
    fn chunk(out: &mut Vec<u8>, kind: &[u8; 4], data: &[u8]) {
        out.extend_from_slice(&(data.len() as u32).to_be_bytes());
        let mut body = kind.to_vec();
        body.extend_from_slice(data);
        out.extend_from_slice(&body);
        out.extend_from_slice(&zip::crc32(&body).to_be_bytes());
    }
    let mut out = vec![0x89, b'P', b'N', b'G', 0x0d, 0x0a, 0x1a, 0x0a];
    let mut ihdr = Vec::new();
    ihdr.extend_from_slice(&w.to_be_bytes());
    ihdr.extend_from_slice(&h.to_be_bytes());
    ihdr.extend_from_slice(&[8, 6, 0, 0, 0]); // 8-bit RGBA
    chunk(&mut out, b"IHDR", &ihdr);
    let mut raw = Vec::with_capacity((w as usize * 4 + 1) * h as usize);
    for _ in 0..h {
        raw.push(0); // filter: none
        for _ in 0..w {
            raw.extend_from_slice(&[200, 200, 200, 255]);
        }
    }
    chunk(&mut out, b"IDAT", &miniz_oxide::deflate::compress_to_vec_zlib(&raw, 6));
    chunk(&mut out, b"IEND", &[]);
    out
}

fn moved(m: &Mesh, d: Vec3) -> Mesh {
    let mut out = m.clone();
    for p in out.positions.chunks_exact_mut(3) {
        p[0] += d.x as f32;
        p[1] += d.y as f32;
        p[2] += d.z as f32;
    }
    out
}

/// Two plates: a three-part print-in-place object and a single body on
/// plate 1, one body on plate 2, three filaments, thumbnails on plate 1.
fn toy_like<'a>(meshes: &'a [Mesh]) -> Project<'a> {
    let (ring, sun, planet, lid, crank) = (&meshes[0], &meshes[1], &meshes[2], &meshes[3], &meshes[4]);
    let mut pal = palette();
    pal.push(Filament { name: "planet".into(), color: cadpole_core::paint::unhex("#2F8F8F") });
    Project {
        title: "toy".into(),
        plates: vec![
            Plate {
                name: "Core (print in place)".into(),
                objects: vec![
                    ProjectObject {
                        name: "core".into(),
                        parts: vec![
                            ProjectPart::new("ring", ring, 1),
                            ProjectPart::new("sun", sun, 2),
                            ProjectPart::new("planet", planet, 3),
                        ],
                    },
                    ProjectObject { name: "lid".into(), parts: vec![ProjectPart::new("lid", lid, 2)] },
                ],
                thumbnail: Some(tiny_png(THUMBNAIL_PX, THUMBNAIL_PX)),
                thumbnail_small: Some(tiny_png(THUMBNAIL_SMALL_PX, THUMBNAIL_SMALL_PX)),
            },
            Plate { name: "Crank".into(), objects: vec![ProjectObject { name: "crank".into(), parts: vec![ProjectPart::new("crank", crank, 1)] }], ..Default::default() },
        ],
        palette: pal,
        ..Project::default()
    }
}

fn toy_meshes() -> Vec<Mesh> {
    vec![
        box_mesh(Vec3::new(-50.0, -50.0, 0.0), Vec3::new(50.0, 50.0, 20.0)),
        box_mesh(Vec3::new(-10.0, -10.0, 2.0), Vec3::new(10.0, 10.0, 18.0)),
        box_mesh(Vec3::new(20.0, -8.0, 2.0), Vec3::new(36.0, 8.0, 18.0)),
        box_mesh(Vec3::new(80.0, -30.0, 0.0), Vec3::new(140.0, 30.0, 6.0)),
        box_mesh(Vec3::new(10.0, 10.0, 0.0), Vec3::new(70.0, 22.0, 8.0)),
    ]
}

// ---------------------------------------------------------------------------
// The tests
// ---------------------------------------------------------------------------

#[test]
fn a_project_has_the_parts_and_the_shape_of_a_bambu_project() {
    let meshes = toy_meshes();
    let w = write_project(&toy_like(&meshes)).unwrap();
    assert_eq!(w.warnings.len(), 1, "{:?}", w.warnings);
    assert!(w.warnings[0].contains("no template="), "the built-in settings say what they are: {:?}", w.warnings);
    let s = read_structure(&w.bytes);
    check_ours(&s, DEFAULT_BED);

    // The layout, part by part.
    for part in [
        "[Content_Types].xml",
        "_rels/.rels",
        "3D/3dmodel.model",
        "3D/_rels/3dmodel.model.rels",
        "Metadata/model_settings.config",
        "Metadata/project_settings.config",
        "Metadata/slice_info.config",
        "Metadata/plate_1.png",
        "Metadata/plate_1_small.png",
    ] {
        assert!(s.names.contains(&part.to_owned()), "{part} in {:?}", s.names);
    }
    assert_eq!(s.files.len(), 3, "one object file per print object: {:?}", s.files.keys());
    assert_eq!(s.root.objects.len(), 3);
    assert_eq!(s.root.items.len(), 3);
    assert_eq!(s.settings.plates.len(), 2);
    assert_eq!(s.settings.plates[0].name, "Core (print in place)");
    assert_eq!(s.settings.plates[1].name, "Crank");
    assert_eq!(s.settings.plates[0].instances.len(), 2);
    assert_eq!(s.settings.plates[1].instances.len(), 1);
    assert_eq!(s.settings.plates[0].thumbnail.as_deref(), Some("Metadata/plate_1.png"));
    assert_eq!(s.pngs["Metadata/plate_1.png"], (512, 512));
    assert_eq!(s.pngs["Metadata/plate_1_small.png"], (128, 128));
    // The package thumbnail is plate 1's, three ways, as every sample's .rels has it.
    assert!(s.rels.contains(&(THUMBNAIL_REL.to_owned(), "/Metadata/plate_1.png".to_owned())));
    assert!(s.rels.contains(&(COVER_MIDDLE_REL.to_owned(), "/Metadata/plate_1.png".to_owned())));
    assert!(s.rels.contains(&(COVER_SMALL_REL.to_owned(), "/Metadata/plate_1_small.png".to_owned())));
    assert!(s.root.metadata.contains(&("Thumbnail_Middle".to_owned(), "/Metadata/plate_1.png".to_owned())));
    assert!(
        s.root.metadata.contains(&("Application".to_owned(), crate::bambu_settings::APPLICATION.to_owned())),
        "a Bambu Studio version, so both slicers open it as a project: {:?}",
        s.root.metadata
    );
    assert_eq!(s.root.build_uuid.as_deref(), Some(BUILD_UUID));
}

#[test]
fn a_print_in_place_set_is_one_object_with_a_part_per_body() {
    let meshes = toy_meshes();
    let s = read_structure(&write_project(&toy_like(&meshes)).unwrap().bytes);
    let core = s.settings.objects.iter().find(|o| o.name == "core").expect("the core object");
    assert_eq!(core.parts.iter().map(|p| p.name.as_str()).collect::<Vec<_>>(), ["ring", "sun", "planet"]);
    assert_eq!(core.parts.iter().map(|p| p.extruder).collect::<Vec<_>>(), [Some(1), Some(2), Some(3)]);
    assert_eq!(core.extruder, Some(1));
    let root = s.root.objects.iter().find(|o| o.id == core.id).unwrap();
    assert_eq!(root.components.len(), 3);
    let paths: BTreeSet<&str> = root.components.iter().map(|c| c.path.as_deref().unwrap()).collect();
    assert_eq!(paths.len(), 1, "all three parts live in the object's one file");
    assert_eq!(s.files[*paths.iter().next().unwrap()].meshes.len(), 3);
    assert_eq!(s.root.items.iter().filter(|i| i.objectid == core.id).count(), 1, "one item: the set moves as one");
    // A single body is an object of one part named after the body.
    let lid = s.settings.objects.iter().find(|o| o.name == "lid").unwrap();
    assert_eq!(lid.parts.len(), 1);
    assert_eq!(lid.parts[0].name, "lid");
}

/// The bounding box of a mesh's positions.
fn mesh_bbox(m: &Mesh) -> ([f64; 3], [f64; 3]) {
    let (mut lo, mut hi) = ([f64::MAX; 3], [f64::MIN; 3]);
    for p in m.positions.chunks_exact(3) {
        for k in 0..3 {
            lo[k] = lo[k].min(p[k] as f64);
            hi[k] = hi[k].max(p[k] as f64);
        }
    }
    (lo, hi)
}

#[test]
fn each_part_is_centred_and_a_plate_keeps_the_scripts_layout_centred_on_the_bed() {
    let meshes = toy_meshes();
    let project = toy_like(&meshes);
    let s = read_structure(&write_project(&project).unwrap().bytes);
    // Reconstruct every part's world box from the file: mesh + component +
    // item − plate origin. Against the script's meshes it must have the
    // same size, the same z (the bed is z = 0 on both sides), and the same
    // position relative to every other part of its plate; and the plate's
    // whole footprint must sit centred on the bed.
    let mut seen = 0;
    for (pi, plate) in project.plates.iter().enumerate() {
        let plate_no = pi as u32 + 1;
        let origin = plate_origin(plate_no, project.plates.len() as u32, project.machine.bed());
        let (mut all_lo, mut all_hi) = ([f64::MAX; 2], [f64::MIN; 2]);
        let (mut script_lo, mut script_hi) = ([f64::MAX; 2], [f64::MIN; 2]);
        let mut boxes: Vec<([f64; 3], [f64; 3], [f64; 3], [f64; 3])> = Vec::new(); // (file lo, file hi, script lo, script hi)
        for obj in &plate.objects {
            let cfg = s.settings.objects.iter().find(|o| o.name == obj.name).unwrap();
            let root = s.root.objects.iter().find(|o| o.id == cfg.id).unwrap();
            let item = s.root.items.iter().find(|i| i.objectid == cfg.id).unwrap();
            for (j, part) in obj.parts.iter().enumerate() {
                let c = &root.components[j];
                let f = &s.files[c.path.as_deref().unwrap()];
                let m = f.meshes.iter().find(|m| m.id == c.objectid).unwrap();
                let mut lo = [f64::MAX; 3];
                let mut hi = [f64::MIN; 3];
                for v in &m.verts {
                    for k in 0..3 {
                        let world = v[k] + c.transform[9 + k] + item.transform[9 + k] - if k < 2 { origin[k] } else { 0.0 };
                        lo[k] = lo[k].min(world);
                        hi[k] = hi[k].max(world);
                    }
                }
                let (elo, ehi) = mesh_bbox(part.mesh);
                for k in 0..3 {
                    assert!(((hi[k] - lo[k]) - (ehi[k] - elo[k])).abs() < 1e-3, "{}: size on axis {k}", part.name);
                }
                assert!((lo[2] - elo[2]).abs() < 1e-3, "{}: z is not moved", part.name);
                for k in 0..2 {
                    all_lo[k] = all_lo[k].min(lo[k]);
                    all_hi[k] = all_hi[k].max(hi[k]);
                    script_lo[k] = script_lo[k].min(elo[k]);
                    script_hi[k] = script_hi[k].max(ehi[k]);
                }
                boxes.push((lo, hi, elo, ehi));
                seen += 1;
            }
            // The item's z is the object's half-height: it sits on the bed.
            assert!(item.transform[11] > 0.0);
        }
        // Centred on the bed, as a group.
        for k in 0..2 {
            assert!(((all_lo[k] + all_hi[k]) * 0.5 - project.machine.bed()[k] * 0.5).abs() < 1e-3, "plate {plate_no}: axis {k} centred: {all_lo:?}..{all_hi:?}");
        }
        // Every part where the script had it relative to the others.
        let shift = bed_shift(project.machine.bed(), script_lo, script_hi);
        for (lo, _, elo, _) in &boxes {
            for k in 0..2 {
                assert!((lo[k] - (elo[k] + shift[k])).abs() < 1e-3, "plate {plate_no}: a part moved relative to its plate");
            }
        }
    }
    assert_eq!(seen, 5);
    // Plate 2's one object sits at the centre of plate 2's grid cell.
    let crank = s.settings.objects.iter().find(|o| o.name == "crank").unwrap();
    let item = s.root.items.iter().find(|i| i.objectid == crank.id).unwrap();
    assert!((item.transform[9] - (307.2 + 128.0)).abs() < 1e-3, "{:?}", item.transform);
    assert!((item.transform[10] - 128.0).abs() < 1e-3);
    assert!((item.transform[11] - 4.0).abs() < 1e-3);
}

#[test]
fn a_plates_contents_are_centred_on_the_bed_together() {
    assert_eq!(bed_shift([256.0, 256.0], [0.0, 0.0], [40.0, 20.0]), [108.0, 118.0]);
    assert_eq!(bed_shift([180.0, 180.0], [-50.0, -50.0], [50.0, 50.0]), [90.0, 90.0]);
    assert_eq!(bed_shift([256.0, 256.0], [1200.0, 1200.0], [1256.0, 1256.0]), [-1100.0, -1100.0]);
}

#[test]
fn plates_lay_out_on_bambus_grid() {
    // ceil(sqrt(N)) to a row: the rule every multi-plate sample follows.
    assert_eq!([1, 2, 3, 4, 5, 9, 10, 16, 17, 19].map(plate_columns), [1, 2, 2, 2, 3, 3, 4, 4, 5, 5]);
    assert_eq!(plate_columns(0), 1);
    let b = [256.0, 256.0];
    assert_eq!(plate_origin(1, 1, b), [0.0, 0.0]);
    assert_eq!(plate_origin(2, 2, b), [307.2, 0.0]);
    // Three and four plates: two to a row (three to a row would put plate 3
    // at x = 614.4 and every piece of it outside Bambu's plate 3).
    assert_eq!(plate_origin(3, 3, b), [0.0, -307.2]);
    assert_eq!(plate_origin(3, 4, b), [0.0, -307.2]);
    assert_eq!(plate_origin(4, 4, b), [307.2, -307.2]);
    // Nine plates: three to a row, as in the Gridfinity sample.
    assert_eq!(plate_origin(3, 9, b), [614.4, 0.0]);
    assert_eq!(plate_origin(4, 9, b), [0.0, -307.2]);
    assert_eq!(plate_origin(9, 9, b), [614.4, -614.4]);
    // Ten plates: four to a row.
    let o = plate_origin(4, 10, b);
    assert!((o[0] - 921.6).abs() < 1e-9 && o[1] == 0.0 && o[1].is_sign_positive(), "{o:?}");
    assert_eq!(plate_origin(5, 10, b), [0.0, -307.2]);
    assert_eq!(plate_origin(2, 2, [180.0, 180.0]), [216.0, 0.0]);
    // The H2D's 350 x 320 bed: plate 5 of 6 at (420, -384).
    assert_eq!(plate_origin(5, 6, [350.0, 320.0]), [420.0, -384.0]);
    assert_eq!(plate_origin(6, 6, [350.0, 320.0]), [840.0, -384.0]);
    // The H2C's 330 x 320.
    assert_eq!(plate_origin(2, 7, [330.0, 320.0]), [396.0, 0.0]);
}

/// A 256 mm one-nozzle machine with no corner.
fn plain(w: f64, d: f64) -> Machine {
    Machine { area: [0.0, 0.0, w, d], height: 250.0, ..Machine::default() }
}

/// [`layout_plates`] with no prime towers: the pieces alone.
fn lay(machine: &Machine, plates: u32, footprints: &[Footprint]) -> Layout {
    layout_plates(machine, plates, footprints, &TowerPlan::none())
}

/// The whole layout rule at once, as the project writer and the model view
/// both read it: a plate's corner on the grid, its contents centred on the
/// bed as a group, an empty plate placed with no shift, and the same
/// footprints on a 180 mm bed landing on that bed's grid.
#[test]
fn layout_places_every_plate_on_the_grid_with_its_contents_centred() {
    let f = |plate, lo, hi| Footprint { plate, lo, hi, height: 10.0 };
    // Plate 1: one 40 x 20 footprint at the origin. Plate 2: empty. Plate 3:
    // two footprints whose union is x 0..20, y 0..55 -- the engine test's
    // "a and c as a group". Plate 3 starts the second row of two.
    let footprints = [
        f(1, [0.0, 0.0], [40.0, 20.0]),
        f(3, [0.0, 0.0], [20.0, 20.0]),
        f(3, [0.0, 40.0], [15.0, 55.0]),
        f(4, [-50.0, -50.0], [50.0, 50.0]),
    ];
    let layout = lay(&plain(256.0, 256.0), 4, &footprints);
    assert!(layout.problems.is_empty(), "{:?}", layout.problems);
    let placed = layout.placements;
    assert_eq!(placed.len(), 4);
    assert_eq!(placed[0], PlatePlacement { plate: 1, origin: [0.0, 0.0], shift: [108.0, 118.0], tower: None });
    assert_eq!(placed[0].translation(), [108.0, 118.0, 0.0]);
    assert_eq!(placed[1], PlatePlacement { plate: 2, origin: [307.2, 0.0], shift: [0.0, 0.0], tower: None });
    assert_eq!(placed[2], PlatePlacement { plate: 3, origin: [0.0, -307.2], shift: [118.0, 100.5], tower: None });
    assert_eq!(placed[2].translation(), [118.0, -206.7, 0.0]);
    assert_eq!(placed[3], PlatePlacement { plate: 4, origin: [307.2, -307.2], shift: [128.0, 128.0], tower: None });
    assert_eq!(placed[3].translation(), [435.2, -179.2, 0.0]);
    // The 180 mm bed: plate 2 starts at x = 216 and a 100 mm square centred
    // on the script's origin lands centred on the bed, at 216 + 90.
    let placed = lay(&plain(180.0, 180.0), 2, &[f(2, [-50.0, -50.0], [50.0, 50.0])]).placements;
    assert_eq!(placed[1], PlatePlacement { plate: 2, origin: [216.0, 0.0], shift: [90.0, 90.0], tower: None });
    assert_eq!(placed[1].translation(), [306.0, 90.0, 0.0]);
    // No plates at all is no placements, and a footprint past the last
    // plate asked for is simply not laid out.
    assert!(lay(&plain(256.0, 256.0), 0, &footprints).placements.is_empty());
    assert_eq!(lay(&plain(256.0, 256.0), 1, &footprints).placements.len(), 1);
}

/// An H2D-shaped machine: 350 x 320 x 325, the left nozzle X 0-325 up to
/// 320 mm, the right X 25-350 up to 325.
fn twin() -> Machine {
    Machine {
        name: "Twin".into(),
        area: [0.0, 0.0, 350.0, 320.0],
        height: 325.0,
        nozzles: vec![
            Reach { area: [0.0, 0.0, 325.0, 320.0], height: 320.0, hotends: 1 },
            Reach { area: [25.0, 0.0, 350.0, 320.0], height: 325.0, hotends: 1 },
        ],
        ..Machine::default()
    }
}

#[test]
fn a_plate_is_centred_where_every_nozzle_reaches() {
    // On an H2C-shaped bed the shared region (X 25-325) is not centred on
    // the bed (0-330): a plate that fits it is centred on it.
    let mut h2c = twin();
    h2c.area = [0.0, 0.0, 330.0, 320.0];
    h2c.nozzles[1].area = [25.0, 0.0, 330.0, 320.0];
    let f = Footprint { plate: 1, lo: [0.0, 0.0], hi: [280.0, 100.0], height: 30.0 };
    let l = lay(&h2c, 1, &[f]);
    assert!(l.problems.is_empty(), "{:?}", l.problems);
    assert_eq!(l.placements[0].shift, [35.0, 110.0], "x 35..315, inside 25..325");
    // Wider than the shared 300 mm but on the bed: centred on the bed, warned.
    let wide = Footprint { plate: 1, lo: [0.0, 0.0], hi: [320.0, 100.0], height: 30.0 };
    let l = lay(&h2c, 1, &[wide]);
    assert_eq!(l.placements[0].shift, [5.0, 110.0]);
    assert_eq!(l.problems.len(), 1);
    assert!(!l.problems[0].fatal);
    assert!(l.problems[0].message.contains("300 x 320 mm every nozzle"), "{}", l.problems[0].message);
    // Taller than the left nozzle prints: warned; taller than the machine: refused.
    let l = lay(&twin(), 1, &[Footprint { height: 322.0, ..f }]);
    assert!(l.problems.iter().any(|p| !p.fatal && p.message.contains("only one nozzle")), "{:?}", l.problems);
    let l = lay(&twin(), 1, &[Footprint { height: 330.0, ..f }]);
    assert!(l.problems.iter().any(|p| p.fatal && p.message.contains("330 mm tall")), "{:?}", l.problems);
}

#[test]
fn a_plate_whose_pieces_cannot_fit_is_refused_naming_the_plate() {
    let machine = Machine { name: "Bambu Lab X1 Carbon".into(), ..plain(256.0, 256.0) };
    // One piece too big, that would fit turned.
    let one = [Footprint { plate: 2, lo: [0.0, 0.0], hi: [300.0, 200.0], height: 20.0 }];
    let l = lay(&machine, 2, &one);
    let refused = l.refusals(|n| plate_label(n, "Roof"));
    assert_eq!(refused.len(), 1, "{refused:?}");
    assert!(refused[0].starts_with("plate 2 (\"Roof\"): its pieces span 300 x 200 mm"), "{}", refused[0]);
    assert!(refused[0].contains("256 x 256 mm") && refused[0].contains("split it"), "{}", refused[0]);
    let turnable = [Footprint { plate: 1, lo: [0.0, 0.0], hi: [260.0, 100.0], height: 20.0 }];
    let refused = lay(&plain(300.0, 256.0), 1, &turnable).refusals(|n| plate_label(n, ""));
    assert!(refused.is_empty(), "{refused:?}");
    let refused = lay(&plain(256.0, 300.0), 1, &turnable).refusals(|n| plate_label(n, ""));
    assert!(refused[0].contains("turn it 90 degrees"), "{refused:?}");
    // Two pieces placed too far apart.
    let two = [
        Footprint { plate: 1, lo: [0.0, 0.0], hi: [50.0, 50.0], height: 20.0 },
        Footprint { plate: 1, lo: [400.0, 0.0], hi: [450.0, 50.0], height: 20.0 },
    ];
    let refused = lay(&machine, 1, &two).refusals(|n| plate_label(n, ""));
    assert!(refused[0].starts_with("plate 1: its pieces span 450 x 50 mm") && refused[0].contains("closer together"), "{refused:?}");
}

#[test]
fn objects_that_stand_in_each_other_are_warned_not_moved() {
    let a = Footprint { plate: 1, lo: [0.0, 0.0], hi: [50.0, 40.0], height: 20.0 };
    let b = Footprint { plate: 1, lo: [30.0, 10.0], hi: [90.0, 30.0], height: 20.0 };
    let l = lay(&plain(256.0, 256.0), 1, &[a, b]);
    assert_eq!(l.problems.len(), 1, "{:?}", l.problems);
    assert!(!l.problems[0].fatal);
    assert!(l.problems[0].message.contains("share 20 x 20 mm"), "{}", l.problems[0].message);
    // Side by side, touching: no warning.
    let c = Footprint { lo: [50.0, 0.0], hi: [90.0, 40.0], ..a };
    assert!(lay(&plain(256.0, 256.0), 1, &[a, c]).problems.is_empty());
}

#[test]
fn a_plate_is_moved_off_the_corner_the_machine_cannot_print() {
    // The X1 series' 18 x 28 mm front-left corner. A 230 x 230 piece centred
    // on the bed covers it (13..243); moved by the smaller of the two moves
    // that clear it, it stands at x 18..248.
    let x1 = Machine { exclude: vec![[0.0, 0.0, 18.0, 28.0]], ..plain(256.0, 256.0) };
    let f = Footprint { plate: 1, lo: [0.0, 0.0], hi: [230.0, 230.0], height: 20.0 };
    let l = lay(&x1, 1, &[f]);
    assert!(l.problems.is_empty(), "{:?}", l.problems);
    assert_eq!(l.placements[0].shift, [18.0, 13.0]);
    // A small piece never comes near it.
    let small = Footprint { hi: [100.0, 100.0], ..f };
    assert_eq!(lay(&x1, 1, &[small]).placements[0].shift, [78.0, 78.0]);
    // A piece the size of the bed cannot be moved off it: warned.
    let full = Footprint { hi: [250.0, 250.0], ..f };
    let l = lay(&x1, 1, &[full]);
    assert!(l.problems.iter().any(|p| !p.fatal && p.message.contains("18 x 28 mm")), "{:?}", l.problems);
}

/// An H2C-shaped machine: 330 x 320, the left nozzle X 0-325, the right
/// X 25-330; every nozzle reaches X 25-325.
fn h2c_like() -> Machine {
    let mut m = twin();
    m.name = "Twin C".into();
    m.area = [0.0, 0.0, 330.0, 320.0];
    m.nozzles[1].area = [25.0, 0.0, 330.0, 320.0];
    m
}

/// A piece 150 mm wide printing four filaments with a template's 70 mm
/// prime tower. Centred where every nozzle reaches it leaves 75 mm either
/// side and the tower needs 80 (70 and 5 clear each side), so the piece
/// moves the least distance that opens a strip -- 5 mm -- the tower stands
/// in it, and a warning says so; printing one filament it stays centred.
#[test]
fn a_plate_moves_its_pieces_aside_to_make_room_for_its_prime_tower() {
    let m = h2c_like();
    let room = crate::bambu_settings::TowerRoom::for_width(70.0);
    let base = Footprint { plate: 1, lo: [10.0, 20.0], hi: [160.0, 270.0], height: 76.4 };
    let l = layout_plates(&m, 1, &[base], &TowerPlan { room: Some(room), filaments: vec![4] });
    assert!(l.refusals(|n| plate_label(n, "")).is_empty(), "{:?}", l.problems);
    let p = l.placements[0];
    let (x0, x1) = (base.lo[0] + p.shift[0], base.hi[0] + p.shift[0]);
    let (y0, y1) = (base.lo[1] + p.shift[1], base.hi[1] + p.shift[1]);
    assert!(x0 >= 25.0 - 1e-9 && x1 <= 325.0 + 1e-9, "still where every nozzle reaches: {x0}..{x1}");
    assert!((x0 - 95.0).abs() < 1e-9, "5 mm left of centre (100): {x0}");
    assert!((y0 - 35.0).abs() < 1e-9 && (y1 - 285.0).abs() < 1e-9, "centred along y: {y0}..{y1}");
    let spot = p.tower.expect("a spot");
    assert_eq!(spot, [250.0, 5.0], "the right-hand strip, at its front");
    assert!(spot[0] - room.margin >= x1 - 1e-9, "clear of the piece: {spot:?}, piece to {x1}");
    let said = l.warnings(|n| plate_label(n, "Base"));
    assert_eq!(said.len(), 1, "{said:?}");
    assert!(said[0].starts_with("plate 1 (\"Base\"): its pieces are moved 5 mm left") && said[0].contains("70 mm prime tower"), "{said:?}");
    // One filament: no tower is printed, so nothing moves and nothing is said.
    let l = layout_plates(&m, 1, &[base], &TowerPlan { room: Some(room), filaments: vec![1] });
    assert!(l.problems.is_empty(), "{:?}", l.problems);
    assert!((base.lo[0] + l.placements[0].shift[0] - 100.0).abs() < 1e-9);
    assert_eq!(l.placements[0].tower, None, "no free spot, and none needed");
    // The template turned the tower off: the same.
    let l = layout_plates(&m, 1, &[base], &TowerPlan { room: None, filaments: vec![4] });
    assert!(l.problems.is_empty() && l.placements[0].tower.is_none(), "{:?}", l.problems);
    // Room as centred: nothing moves; the tower takes a free strip.
    let small = Footprint { hi: [110.0, 120.0], ..base };
    let l = layout_plates(&m, 1, &[small], &TowerPlan { room: Some(room), filaments: vec![4] });
    assert!(l.problems.is_empty(), "{:?}", l.problems);
    assert_eq!(l.placements[0].shift, [115.0, 90.0]);
    assert_eq!(l.placements[0].tower, Some([250.0, 5.0]));
}

/// A plate of several filaments that no move can make room on is
/// refused, naming the plate, the tower and what would help -- a quarter
/// turn when that leaves room.
#[test]
fn a_plate_with_no_room_for_its_prime_tower_is_refused_naming_it() {
    let m = h2c_like();
    let room = crate::bambu_settings::TowerRoom::for_width(70.0);
    let towers = TowerPlan { room: Some(room), filaments: vec![1, 3] };
    // 230 x 250 leaves 70 mm across and 70 deep; turned (250 x 230) it
    // leaves 90 deep, room for the 80 the tower needs.
    let f = |plate| Footprint { plate, lo: [0.0, 0.0], hi: [230.0, 250.0], height: 30.0 };
    let l = layout_plates(&m, 2, &[f(1), f(2)], &towers);
    let refused = l.refusals(|n| plate_label(n, if n == 2 { "L2" } else { "L1" }));
    assert_eq!(refused.len(), 1, "plate 1 prints one filament: {refused:?}");
    assert!(refused[0].starts_with("plate 2 (\"L2\"): it prints several filaments"), "{refused:?}");
    assert!(refused[0].contains("70 mm tower") && refused[0].contains("a square 80 mm across") && refused[0].contains("Twin C"), "{refused:?}");
    assert!(refused[0].contains("turn its pieces 90 degrees"), "{refused:?}");
    // 250 x 290 leaves no room either way: no turn is suggested.
    let big = Footprint { plate: 1, lo: [0.0, 0.0], hi: [250.0, 290.0], height: 30.0 };
    let refused = layout_plates(&m, 1, &[big], &TowerPlan { room: Some(room), filaments: vec![2] }).refusals(|n| plate_label(n, ""));
    assert_eq!(refused.len(), 1);
    assert!(!refused[0].contains("turn"), "{refused:?}");
}

/// End to end: a project written with a template's 70 mm tower and a
/// piece the tower has no room beside as centred. The structural checker
/// holds every project to "the tower is clear of every piece"; this one
/// also moves the piece and says so.
#[test]
fn a_written_project_never_stands_its_prime_tower_on_a_piece() {
    let settings = serde_json::json!({
        "from": "project",
        "name": "project_settings",
        "printer_model": "Twin C",
        "printable_area": ["0x0", "330x0", "330x320", "0x320"],
        "printable_height": "325",
        "nozzle_diameter": ["0.4", "0.4"],
        "extruder_printable_area": ["0x0,325x0,325x320,0x320", "25x0,330x0,330x320,25x320"],
        "extruder_printable_height": ["320", "325"],
        "filament_colour": ["#000000", "#FFFFFF"],
        "prime_tower_width": "70",
        "wipe_tower_x": ["165"],
        "wipe_tower_y": ["250"]
    });
    let t = Template::from_parts("BambuStudio-02.04.00.70", &settings.to_string()).unwrap();
    let ground = box_mesh(Vec3::ZERO, Vec3::new(150.0, 250.0, 60.0));
    let top = box_mesh(Vec3::new(10.0, 10.0, 60.0), Vec3::new(140.0, 240.0, 76.4));
    let project = Project {
        plates: vec![Plate {
            name: "Base".into(),
            objects: vec![ProjectObject {
                name: "base".into(),
                parts: vec![ProjectPart::new("ground", &ground, 1), ProjectPart::new("top", &top, 2)],
            }],
            ..Default::default()
        }],
        palette: palette(),
        machine: t.machine(),
        template: Some((t, "h2c.3mf".into())),
        ..Project::default()
    };
    let w = write_project(&project).unwrap();
    let s = read_structure(&w.bytes);
    check_ours(&s, [330.0, 320.0]);
    let ps = s.settings_json.as_ref().unwrap();
    assert_eq!(ps["wipe_tower_x"], serde_json::json!(["250"]), "not the template's 165, inside the piece");
    assert_eq!(ps["wipe_tower_y"], serde_json::json!(["5"]));
    assert!(w.warnings.iter().any(|m| m.contains("moved 5 mm left") && m.contains("prime tower")), "{:?}", w.warnings);
}

#[test]
fn filament_slots_reach_the_parts_and_the_colour_list() {
    let meshes = toy_meshes();
    let s = read_structure(&write_project(&toy_like(&meshes)).unwrap().bytes);
    assert_eq!(s.filament_colour.as_deref(), Some(&["#C98A6B".to_owned(), "#0078BF".to_owned(), "#2F8F8F".to_owned()][..]));
    let core = s.settings.objects.iter().find(|o| o.name == "core").unwrap();
    assert_eq!(core.parts.iter().map(|p| p.extruder).collect::<Vec<_>>(), [Some(1), Some(2), Some(3)]);
    let root = s.root.objects.iter().find(|o| o.id == core.id).unwrap();
    let f = &s.files[root.components[0].path.as_deref().unwrap()];
    assert!(f.materials.is_empty(), "no core basematerials in a Bambu project");
    assert!(!f.xml.contains("pid=") && !f.xml.contains("p1="), "no core colour properties");
}

#[test]
fn painted_faces_carry_paint_color_in_the_object_file() {
    let mut a = box_mesh(Vec3::ZERO, Vec3::new(10.0, 10.0, 10.0));
    a.face_ids = a.face_ids.clone();
    let n = a.triangle_count();
    let mut tri_slots = vec![0u32; n];
    tri_slots[n - 2] = 2;
    tri_slots[n - 1] = 2;
    let project = Project {
        plates: vec![Plate {
            name: String::new(),
            objects: vec![ProjectObject {
                name: "b".into(),
                parts: vec![ProjectPart { name: "b".into(), mesh: &a, slot: 1, tri_slots }],
            }],
            ..Default::default()
        }],
        palette: palette(),
        ..Project::default()
    };
    let s = read_structure(&write_project(&project).unwrap().bytes);
    check_ours(&s, DEFAULT_BED);
    let f = s.files.values().next().unwrap();
    let painted: Vec<_> = f.meshes[0].tris.iter().filter(|(_, _, paint)| paint.is_some()).collect();
    assert_eq!(painted.len(), 2);
    for (_, p1, paint) in painted {
        assert_eq!(*p1, None, "no core p1 in a project");
        assert_eq!(paint.as_deref(), Some("8"), "filament 2 is `8`");
    }
}

#[test]
fn every_object_is_set_down_on_the_bed() {
    // A storey modelled at its own height (z 100..130) and a piece reaching
    // below the bed (z -5..5): each stands on the bed, its item z its
    // half-height; the assembly view keeps where the script had them.
    let up = box_mesh(Vec3::new(0.0, 0.0, 100.0), Vec3::new(40.0, 30.0, 130.0));
    let down = box_mesh(Vec3::new(0.0, 0.0, -5.0), Vec3::new(20.0, 20.0, 5.0));
    let project = Project {
        plates: vec![
            Plate { name: "L1".into(), objects: vec![ProjectObject { name: "L1".into(), parts: vec![ProjectPart::new("storey", &up, 1)] }], ..Default::default() },
            Plate { name: "Base".into(), objects: vec![ProjectObject { name: "base".into(), parts: vec![ProjectPart::new("base", &down, 1)] }], ..Default::default() },
        ],
        palette: palette(),
        ..Project::default()
    };
    let s = read_structure(&write_project(&project).unwrap().bytes);
    check_ours(&s, DEFAULT_BED);
    let item = |name: &str| {
        let o = s.settings.objects.iter().find(|o| o.name == name).unwrap();
        s.root.items.iter().find(|i| i.objectid == o.id).unwrap().transform.clone()
    };
    assert_eq!(item("L1")[11], 15.0);
    assert_eq!(item("base")[11], 5.0);
    let l1 = s.settings.objects.iter().find(|o| o.name == "L1").unwrap();
    let assembled = s.settings.assemble.iter().find(|(id, _)| *id == l1.id).unwrap();
    assert_eq!(assembled.1[11], 115.0, "the assembly view keeps the storey at its height");
}

#[test]
fn a_project_the_machine_cannot_hold_is_refused() {
    let big = box_mesh(Vec3::ZERO, Vec3::new(300.0, 100.0, 10.0));
    let project = Project {
        plates: vec![Plate { name: "Lot".into(), objects: vec![ProjectObject { name: "lot".into(), parts: vec![ProjectPart::new("lot", &big, 1)] }], ..Default::default() }],
        palette: palette(),
        machine: Machine { name: "Bambu Lab X1 Carbon".into(), ..plain(256.0, 256.0) },
        ..Project::default()
    };
    let Err(e) = write_project(&project) else { panic!("a 300 mm lot on a 256 mm bed is refused") };
    assert!(e.contains("plate 1 (\"Lot\")") && e.contains("300 x 100 mm") && e.contains("X1 Carbon"), "{e}");
}

#[test]
fn a_template_supplies_the_settings_the_machine_and_the_version() {
    let settings = serde_json::json!({
        "from": "project",
        "name": "project_settings",
        "printer_model": "Test Twin",
        "printer_settings_id": "Test Twin 0.4 nozzle",
        "print_settings_id": "0.20mm Test @Twin",
        "printable_area": ["0x0", "350x0", "350x320", "0x320"],
        "printable_height": "325",
        "nozzle_diameter": ["0.4", "0.4"],
        "extruder_printable_area": ["0x0,325x0,325x320,0x320", "25x0,350x0,350x320,25x320"],
        "extruder_printable_height": ["320", "325"],
        "filament_colour": ["#000000", "#FFFFFF"],
        "filament_settings_id": ["Test PLA @Twin", "Test PLA @Twin"],
        "machine_start_gcode": "; the user's own start G-code, kept as saved",
        "wall_filament": "1"
    });
    let t = Template::from_parts("BambuStudio-02.06.00.51", &settings.to_string()).unwrap();
    let meshes = toy_meshes();
    let mut project = toy_like(&meshes);
    project.machine = t.machine();
    project.template = Some((t, "twin.3mf".into()));
    let w = write_project(&project).unwrap();
    assert!(!w.warnings.iter().any(|m| m.contains("no template=")), "{:?}", w.warnings);
    assert!(w.warnings.iter().any(|m| m.contains("twin.3mf") && m.contains("holds 2 filaments")), "{:?}", w.warnings);
    let s = read_structure(&w.bytes);
    check_ours(&s, [350.0, 320.0]);
    assert!(s.root.metadata.contains(&("Application".to_owned(), crate::bambu_settings::APPLICATION.to_owned())));
    let ps = s.settings_json.as_ref().unwrap();
    assert_eq!(ps["machine_start_gcode"], "; the user's own start G-code, kept as saved");
    assert_eq!(ps["wall_filament"], "0", "the part's own filament");
    assert_eq!(ps["filament_settings_id"].as_array().unwrap().len(), 3);
    assert_eq!(s.printable_area, Some([350.0, 320.0]));
    // Plate 2 of 2 on the template's 350 mm bed.
    let crank = s.settings.objects.iter().find(|o| o.name == "crank").unwrap();
    let item = s.root.items.iter().find(|i| i.objectid == crank.id).unwrap();
    assert!((item.transform[9] - (420.0 + 175.0)).abs() < 1e-3, "{:?}", item.transform);
}

#[test]
fn an_empty_plate_keeps_its_number_and_a_part_with_no_triangles_is_left_out() {
    let a = box_mesh(Vec3::ZERO, Vec3::new(10.0, 10.0, 10.0));
    let empty = Mesh::default();
    let project = Project {
        plates: vec![
            Plate { name: "one".into(), objects: vec![ProjectObject { name: "a".into(), parts: vec![ProjectPart::new("a", &a, 1)] }], ..Default::default() },
            Plate { name: "two (empty)".into(), ..Default::default() },
            Plate {
                name: "three".into(),
                objects: vec![
                    ProjectObject { name: "nothing".into(), parts: vec![ProjectPart::new("nothing", &empty, 1)] },
                    ProjectObject { name: "b".into(), parts: vec![ProjectPart::new("b", &a, 1)] },
                ],
                ..Default::default()
            },
        ],
        palette: palette(),
        ..Project::default()
    };
    let w = write_project(&project).unwrap();
    let s = read_structure(&w.bytes);
    check_ours(&s, DEFAULT_BED);
    assert_eq!(s.settings.plates.len(), 3);
    assert!(s.settings.plates[1].instances.is_empty());
    assert_eq!(s.settings.plates[1].name, "two (empty)");
    assert_eq!(s.root.objects.len(), 2, "the object with no triangles is not written");
    let b = s.settings.objects.iter().find(|o| o.name == "b").unwrap();
    let item = s.root.items.iter().find(|i| i.objectid == b.id).unwrap();
    assert!((item.transform[9] - 128.0).abs() < 1e-3, "plate 3 of 3 starts the second row of two: {:?}", item.transform);
    assert!((item.transform[10] - (-307.2 + 128.0)).abs() < 1e-3, "{:?}", item.transform);
}

#[test]
fn the_same_project_writes_the_same_bytes() {
    let meshes = toy_meshes();
    let a = write_project(&toy_like(&meshes)).unwrap().bytes;
    let b = write_project(&toy_like(&meshes)).unwrap().bytes;
    assert_eq!(a, b);
}

#[test]
fn names_with_xml_metacharacters_survive_the_settings_and_the_model() {
    let a = box_mesh(Vec3::ZERO, Vec3::new(5.0, 5.0, 5.0));
    let project = Project {
        title: "a & b".into(),
        plates: vec![Plate {
            name: "<plate> \"1\"".into(),
            objects: vec![ProjectObject { name: "core & co".into(), parts: vec![ProjectPart::new("<lid>", &a, 1)] }],
            ..Default::default()
        }],
        palette: palette(),
        ..Project::default()
    };
    let s = read_structure(&write_project(&project).unwrap().bytes);
    check_ours(&s, DEFAULT_BED);
    assert_eq!(s.settings.plates[0].name, "<plate> \"1\"");
    assert_eq!(s.settings.objects[0].name, "core & co");
    assert_eq!(s.settings.objects[0].parts[0].name, "<lid>");
    assert!(s.root.metadata.contains(&("Title".to_owned(), "a & b".to_owned())));
}

#[test]
fn a_body_that_is_not_closed_is_reported_and_still_written() {
    let mut a = box_mesh(Vec3::ZERO, Vec3::new(10.0, 10.0, 10.0));
    a.indices.truncate((a.triangle_count() - 2) * 3);
    a.face_ids.truncate(a.face_ids.len() - 2);
    let project = Project {
        plates: vec![Plate { objects: vec![ProjectObject { name: "holed".into(), parts: vec![ProjectPart::new("holed", &a, 1)] }], ..Default::default() }],
        palette: palette(),
        ..Project::default()
    };
    let w = write_project(&project).unwrap();
    assert!(w.warnings.iter().any(|m| m.contains("holed") && m.contains("boundary edge")), "{:?}", w.warnings);
    let z = open(&w.bytes);
    assert!(z.get(&object_part(2)).is_some());
}

#[test]
fn a_moved_copy_of_a_mesh_lands_where_it_was_moved_to() {
    // Two bodies from one mesh, one moved: two objects at two places, each
    // centred in its own file, the item carrying the difference.
    let a = box_mesh(Vec3::ZERO, Vec3::new(10.0, 10.0, 10.0));
    let b = moved(&a, Vec3::new(100.0, 0.0, 0.0));
    let project = Project {
        plates: vec![Plate {
            objects: vec![
                ProjectObject { name: "a".into(), parts: vec![ProjectPart::new("a", &a, 1)] },
                ProjectObject { name: "b".into(), parts: vec![ProjectPart::new("b", &b, 1)] },
            ],
            ..Default::default()
        }],
        palette: palette(),
        ..Project::default()
    };
    let s = read_structure(&write_project(&project).unwrap().bytes);
    check_ours(&s, DEFAULT_BED);
    // The pair spans x = 0..110: centred on the bed it is 73..183.
    let ia = s.root.items.iter().find(|i| i.objectid == s.settings.objects[0].id).unwrap();
    let ib = s.root.items.iter().find(|i| i.objectid == s.settings.objects[1].id).unwrap();
    assert_eq!(&ia.transform[9..], &[78.0, 128.0, 5.0]);
    assert_eq!(&ib.transform[9..], &[178.0, 128.0, 5.0]);
    let fa = &s.files[s.root.objects[0].components[0].path.as_deref().unwrap()];
    let fb = &s.files[s.root.objects[1].components[0].path.as_deref().unwrap()];
    assert_eq!(fa.meshes[0].verts, fb.meshes[0].verts, "the same centred mesh in both files");
}

/// The rules above against a file Bambu Studio itself wrote. The sample
/// files are not in the repository (their licences are the uploaders'), so
/// this runs only when `CADPOLE_BAMBU_SAMPLE` names one:
///
/// ```text
/// bazel test //src/rust/cadpole-export-3mf:cadpole_export_3mf_test \
///   --test_env=CADPOLE_BAMBU_SAMPLE=/path/to/sample.3mf \
///   --test_filter=a_real_bambu_sample_satisfies_the_rules --test_output=all
/// ```
#[test]
fn a_real_bambu_sample_satisfies_the_rules() {
    let Ok(path) = std::env::var("CADPOLE_BAMBU_SAMPLE") else {
        eprintln!("CADPOLE_BAMBU_SAMPLE is not set: skipping the sample comparison");
        return;
    };
    for path in path.split(':') {
        let bytes = std::fs::read(path).unwrap_or_else(|e| panic!("{path}: {e}"));
        let s = read_structure(&bytes);
        check_common(&s, None);
        eprintln!(
            "{path}: {} objects, {} object files, {} plates, bed {:?}, colours {:?}: the rules hold",
            s.root.objects.len(),
            s.files.len(),
            s.settings.plates.len(),
            s.printable_area,
            s.filament_colour
        );
    }
}
