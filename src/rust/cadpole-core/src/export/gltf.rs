//! glTF 2.0 out: a binary GLB or a self-contained `.gltf` whose one
//! buffer is a `data:` URI, written by hand as every other format here is.
//!
//! This is the reader run backwards (`cadpole_import::gltf`, which the
//! round-trip tests read these files with):
//!
//! * **Frame and unit.** glTF is +Y up in metres; Cadpole is +Z up in
//!   millimetres. The reader maps a file's `(x, y, z)` to Cadpole's
//!   `1000·(x, −z, y)`, so this writes Cadpole's `(x, y, z)` as
//!   `s·(x, z, −y)`, `s` metres per millimetre times the millimetres in one
//!   of the script's units (`--model-unit`). The map is a
//!   rotation (determinant +1), so winding and normals survive it.
//! * **Structure.** One scene, one root node named for the document, one
//!   child node per display layer that holds any body written, in
//!   declaration order, and one node per body under its layer's node — a
//!   body on the implicit layer directly under the root. A node and its mesh
//!   take the body's name. `extras.cadpole` says on every node what it is: the
//!   layer's name, visibility, opacity and tint; the body's id, layer and
//!   whether its layer is hidden.
//! * **Meshes.** One primitive per body: `POSITION` (with the `min`/`max`
//!   the specification requires), `NORMAL` (unit length), a colour per
//!   vertex as linear `COLOR_0` when the body carries one (an imported scan),
//!   and `u32` indices. A collapsed triangle is left out, as the
//!   STL, OBJ and 3MF writers leave it out.
//! * **Materials.** One per distinct Cadpole material, in order of first use,
//!   named as the script named it -- or, for the many a script colours
//!   without naming (Cadpole calls those `default`), by its colour, `#rrggbb`:
//!   base colour (Cadpole's is linear, as glTF's is) with the opacity as its
//!   alpha and `BLEND` below one, metallic, roughness, and emission —
//!   through `KHR_materials_emissive_strength` when it is brighter than
//!   glTF's 0..1 factor allows. Procedural textures, transmission and the
//!   print-material terms have no core glTF spelling and are not written.
//! * **Units recorded.** `asset.extras.cadpole` names the file's unit
//!   (always metres), the unit the script was drawn in and its size in
//!   millimetres, and the document's display convention, so a
//!   reader that wants feet and inches back can have them.
//!
//! Deterministic: the same bodies write the same bytes — no clock, no map
//! iteration order, every number printed by Rust's shortest round-trip
//! formatting.

use super::{collapsed, corners};
use crate::context::Material;
use crate::tessellate::Mesh;

/// One body on its way into the file.
pub struct GltfBody<'a> {
    /// The node's and the mesh's name.
    pub name: &'a str,
    /// `feature#n`, recorded in the node's extras.
    pub body_id: &'a str,
    /// The display layer, empty for the implicit one.
    pub layer: &'a str,
    /// The body's layer is hidden: written only because the caller asked
    /// for hidden layers too, and marked so.
    pub hidden: bool,
    pub material: &'a Material,
    pub mesh: &'a Mesh,
}

/// A display layer as the document declared it.
#[derive(Clone, Debug, PartialEq)]
pub struct GltfLayer {
    pub name: String,
    pub visible: bool,
    pub opacity: f64,
    /// Linear RGB tint.
    pub color: Option<[f64; 3]>,
}

/// How the file is written.
#[derive(Clone, Debug)]
pub struct GltfOptions {
    /// The scene's and the root node's name.
    pub title: String,
    /// `asset.generator`.
    pub generator: String,
    /// Millimetres in one unit of the script: 1 for a script in mm, 304.8
    /// for one drawn in feet. 0 reads as 1.
    pub mm_per_unit: f64,
    /// The script unit's name as the caller spelt it (`mm`, `ft`, ...).
    pub model_unit: String,
    /// The display convention: a unit name and, for the imperial
    /// ones, the fraction of an inch.
    pub display_unit: String,
    pub display_fraction: u32,
    /// GLB when true, else a `.gltf` with its buffer inline.
    pub binary: bool,
}

impl Default for GltfOptions {
    fn default() -> Self {
        GltfOptions {
            title: "model".into(),
            generator: "cadpole".into(),
            mm_per_unit: 1.0,
            model_unit: "mm".into(),
            display_unit: "mm".into(),
            display_fraction: 0,
            binary: true,
        }
    }
}

// ------------------------------------------------------------------ JSON

/// Just enough JSON to write one: objects keep their insertion order, so
/// the output is whatever order the writer chose, every time.
enum J {
    Bool(bool),
    Int(u64),
    F32(f32),
    F64(f64),
    Str(String),
    Arr(Vec<J>),
    Obj(Vec<(&'static str, J)>),
}

fn s(v: impl Into<String>) -> J {
    J::Str(v.into())
}

fn write_json(j: &J, out: &mut String) {
    use std::fmt::Write;
    match j {
        J::Bool(b) => out.push_str(if *b { "true" } else { "false" }),
        J::Int(i) => {
            let _ = write!(out, "{i}");
        }
        // Rust prints the shortest text that reads back as the same float,
        // never an exponent: valid JSON, and the `min`/`max` a validator
        // compares against the data are the data's own values.
        J::F32(f) => {
            let _ = if f.is_finite() { write!(out, "{f}") } else { write!(out, "0") };
        }
        J::F64(f) => {
            let _ = if f.is_finite() { write!(out, "{f}") } else { write!(out, "0") };
        }
        J::Str(t) => {
            out.push('"');
            for c in t.chars() {
                match c {
                    '"' => out.push_str("\\\""),
                    '\\' => out.push_str("\\\\"),
                    '\n' => out.push_str("\\n"),
                    '\r' => out.push_str("\\r"),
                    '\t' => out.push_str("\\t"),
                    c if (c as u32) < 0x20 => {
                        let _ = write!(out, "\\u{:04x}", c as u32);
                    }
                    c => out.push(c),
                }
            }
            out.push('"');
        }
        J::Arr(items) => {
            out.push('[');
            for (i, v) in items.iter().enumerate() {
                if i > 0 {
                    out.push(',');
                }
                write_json(v, out);
            }
            out.push(']');
        }
        J::Obj(fields) => {
            out.push('{');
            for (i, (k, v)) in fields.iter().enumerate() {
                if i > 0 {
                    out.push(',');
                }
                write_json(&J::Str((*k).to_owned()), out);
                out.push(':');
                write_json(v, out);
            }
            out.push('}');
        }
    }
}

const BASE64: &[u8; 64] = b"ABCDEFGHIJKLMNOPQRSTUVWXYZabcdefghijklmnopqrstuvwxyz0123456789+/";

/// Standard base64 with padding, for the `.gltf` buffer's `data:` URI.
fn base64(bytes: &[u8]) -> String {
    let mut out = String::with_capacity(bytes.len().div_ceil(3) * 4);
    for chunk in bytes.chunks(3) {
        let b = [chunk[0], *chunk.get(1).unwrap_or(&0), *chunk.get(2).unwrap_or(&0)];
        let n = (u32::from(b[0]) << 16) | (u32::from(b[1]) << 8) | u32::from(b[2]);
        for k in 0..4 {
            if k <= chunk.len() {
                out.push(BASE64[((n >> (18 - 6 * k)) & 63) as usize] as char);
            } else {
                out.push('=');
            }
        }
    }
    out
}

// ------------------------------------------------------------------ the writer

const FLOAT: u64 = 5126;
const UNSIGNED_INT: u64 = 5125;
const ARRAY_BUFFER: u64 = 34962;
const ELEMENT_ARRAY_BUFFER: u64 = 34963;

/// The binary chunk and the views and accessors into it.
#[derive(Default)]
struct Bin {
    bytes: Vec<u8>,
    views: Vec<J>,
    accessors: Vec<J>,
}

impl Bin {
    /// Append `data` (already little-endian, a multiple of four bytes) as a
    /// view of its own and an accessor over all of it; the accessor's index.
    fn push(&mut self, data: &[u8], target: u64, accessor: Vec<(&'static str, J)>) -> u64 {
        debug_assert_eq!(data.len() % 4, 0);
        let offset = self.bytes.len();
        self.bytes.extend_from_slice(data);
        self.views.push(J::Obj(vec![
            ("buffer", J::Int(0)),
            ("byteOffset", J::Int(offset as u64)),
            ("byteLength", J::Int(data.len() as u64)),
            ("target", J::Int(target)),
        ]));
        let view = (self.views.len() - 1) as u64;
        let mut fields = vec![("bufferView", J::Int(view))];
        fields.extend(accessor);
        self.accessors.push(J::Obj(fields));
        (self.accessors.len() - 1) as u64
    }
}

fn f32s(v: &[f32]) -> Vec<u8> {
    v.iter().flat_map(|x| x.to_le_bytes()).collect()
}

/// The material's identity for de-duplication: everything the file says
/// about it, bit for bit.
fn material_key(m: &Material) -> Vec<u64> {
    let mut k: Vec<u64> = m.name.bytes().map(u64::from).collect();
    k.push(u64::MAX);
    for v in m.base_color.iter().chain(&m.emissive).chain([m.roughness, m.metallic, m.opacity].iter()) {
        k.push(v.to_bits());
    }
    k
}

/// What a material is called in the file. Cadpole calls every material a
/// script colours without naming `default`, so a house is fifty materials
/// all called that: one of those is named for its colour instead,
/// `#rrggbb` in sRGB as the script wrote it. A name another, different
/// material already took gets `[k]`.
fn material_name(m: &Material, taken: &mut Vec<String>) -> String {
    let d = Material::default();
    let base = match m.name.trim() {
        "" | "default" if m.base_color != d.base_color => {
            let c = m.base_color.map(|v| crate::render::to_srgb8(v as f32));
            format!("#{:02x}{:02x}{:02x}", c[0], c[1], c[2])
        }
        "" => "default".to_owned(),
        n => n.to_owned(),
    };
    let mut name = base.clone();
    let mut k = 1;
    while taken.contains(&name) {
        name = format!("{base}[{k}]");
        k += 1;
    }
    taken.push(name.clone());
    name
}

fn material_json(m: &Material, name: String) -> (J, bool) {
    let c = |v: f64| J::F64(if v.is_finite() { v.clamp(0.0, 1.0) } else { 0.0 });
    let alpha = m.opacity.clamp(0.0, 1.0);
    let mut pbr = vec![(
        "baseColorFactor",
        J::Arr(vec![c(m.base_color[0]), c(m.base_color[1]), c(m.base_color[2]), c(alpha)]),
    )];
    pbr.push(("metallicFactor", c(m.metallic)));
    pbr.push(("roughnessFactor", c(m.roughness)));
    let mut fields = vec![("name", s(name)), ("pbrMetallicRoughness", J::Obj(pbr))];
    let mut strength_used = false;
    let peak = m.emissive.iter().copied().filter(|v| v.is_finite()).fold(0.0f64, f64::max);
    if peak > 0.0 {
        // glTF's factor stops at 1; a brighter emitter is the factor scaled
        // into range and the extension's strength, which the reader
        // multiplies back.
        let (scale, strength) = if peak > 1.0 { (1.0 / peak, peak) } else { (1.0, 1.0) };
        fields.push((
            "emissiveFactor",
            J::Arr(m.emissive.iter().map(|v| c(v.max(0.0) * scale)).collect()),
        ));
        if strength > 1.0 {
            strength_used = true;
            fields.push((
                "extensions",
                J::Obj(vec![(
                    "KHR_materials_emissive_strength",
                    J::Obj(vec![("emissiveStrength", J::F64(strength))]),
                )]),
            ));
        }
    }
    if alpha < 0.999 {
        fields.push(("alphaMode", s("BLEND")));
    }
    (J::Obj(fields), strength_used)
}

/// Write `bodies` as glTF: a GLB, or a `.gltf` with its buffer inline, as
/// `opts.binary` says. `layers` are the document's declarations in order;
/// a body on a layer they do not name gets a node for it all the same.
pub fn gltf(bodies: &[GltfBody<'_>], layers: &[GltfLayer], opts: &GltfOptions) -> Vec<u8> {
    let mm = if opts.mm_per_unit > 0.0 && opts.mm_per_unit.is_finite() { opts.mm_per_unit } else { 1.0 };
    let k = mm / 1000.0;
    // In f64, then rounded once; `+ 0.0` turns a -0 into 0, which reads
    // the same and prints as `0`.
    let to_file = |v: f32| (f64::from(v) * k) as f32 + 0.0;
    let mut bin = Bin::default();
    let mut meshes: Vec<J> = Vec::new();
    let mut materials: Vec<J> = Vec::new();
    let mut material_keys: Vec<Vec<u64>> = Vec::new();
    let mut material_names: Vec<String> = Vec::new();
    let mut strength_used = false;
    // nodes[0] is the root; the layer nodes and body nodes follow, and the
    // children lists are filled once every node has its index.
    let mut nodes: Vec<J> = Vec::new();
    let mut body_nodes: Vec<(usize, &str)> = Vec::new();

    for b in bodies {
        let m = b.mesh;
        let nv = m.vertex_count();
        // Cadpole (x, y, z) mm -> glTF (x, z, -y) m, in the script's unit.
        let mut pos = Vec::with_capacity(nv * 3);
        let (mut lo, mut hi) = ([f32::INFINITY; 3], [f32::NEG_INFINITY; 3]);
        for p in m.positions.chunks_exact(3) {
            let q = [to_file(p[0]), to_file(p[2]), to_file(-p[1])];
            for a in 0..3 {
                lo[a] = lo[a].min(q[a]);
                hi[a] = hi[a].max(q[a]);
            }
            pos.extend_from_slice(&q);
        }
        let tris: Vec<u32> = m
            .indices
            .chunks_exact(3)
            .filter(|t| t.iter().all(|i| (*i as usize) < nv) && !collapsed(&corners(m, t)))
            .flatten()
            .copied()
            .collect();
        let mut node = vec![("name", s(b.name))];
        if nv > 0 && !tris.is_empty() {
            let mut attrs = Vec::new();
            let p = bin.push(
                &f32s(&pos),
                ARRAY_BUFFER,
                vec![
                    ("componentType", J::Int(FLOAT)),
                    ("count", J::Int(nv as u64)),
                    ("type", s("VEC3")),
                    ("min", J::Arr(lo.iter().map(|v| J::F32(*v)).collect())),
                    ("max", J::Arr(hi.iter().map(|v| J::F32(*v)).collect())),
                ],
            );
            attrs.push(("POSITION", J::Int(p)));
            if m.normals.len() == m.positions.len() {
                let mut nrm = Vec::with_capacity(nv * 3);
                for n in m.normals.chunks_exact(3) {
                    let q = [n[0] + 0.0, n[2] + 0.0, -n[1] + 0.0];
                    let l = (q[0] * q[0] + q[1] * q[1] + q[2] * q[2]).sqrt();
                    // A vertex with no direction still needs a unit normal.
                    if l > 1e-12 && l.is_finite() {
                        nrm.extend_from_slice(&[q[0] / l, q[1] / l, q[2] / l]);
                    } else {
                        nrm.extend_from_slice(&[0.0, 1.0, 0.0]);
                    }
                }
                let n = bin.push(
                    &f32s(&nrm),
                    ARRAY_BUFFER,
                    vec![("componentType", J::Int(FLOAT)), ("count", J::Int(nv as u64)), ("type", s("VEC3"))],
                );
                attrs.push(("NORMAL", J::Int(n)));
            }
            if let Some(colors) = m.vertex_colors() {
                // Stored sRGB-encoded; glTF's COLOR_0 is linear.
                let lin: Vec<f32> = colors.iter().map(|c| crate::render::from_srgb8(*c)).collect();
                let c = bin.push(
                    &f32s(&lin),
                    ARRAY_BUFFER,
                    vec![("componentType", J::Int(FLOAT)), ("count", J::Int(nv as u64)), ("type", s("VEC3"))],
                );
                attrs.push(("COLOR_0", J::Int(c)));
            }
            let idx: Vec<u8> = tris.iter().flat_map(|i| i.to_le_bytes()).collect();
            let ia = bin.push(
                &idx,
                ELEMENT_ARRAY_BUFFER,
                vec![("componentType", J::Int(UNSIGNED_INT)), ("count", J::Int(tris.len() as u64)), ("type", s("SCALAR"))],
            );
            let key = material_key(b.material);
            let mi = match material_keys.iter().position(|x| *x == key) {
                Some(i) => i,
                None => {
                    let name = material_name(b.material, &mut material_names);
                    let (j, used) = material_json(b.material, name);
                    strength_used |= used;
                    materials.push(j);
                    material_keys.push(key);
                    materials.len() - 1
                }
            };
            meshes.push(J::Obj(vec![
                ("name", s(b.name)),
                (
                    "primitives",
                    J::Arr(vec![J::Obj(vec![
                        ("attributes", J::Obj(attrs)),
                        ("indices", J::Int(ia)),
                        ("material", J::Int(mi as u64)),
                        ("mode", J::Int(4)),
                    ])]),
                ),
            ]));
            node.push(("mesh", J::Int((meshes.len() - 1) as u64)));
        }
        node.push((
            "extras",
            J::Obj(vec![(
                "cadpole",
                J::Obj(vec![
                    ("body", s(b.body_id)),
                    ("layer", s(b.layer)),
                    ("hidden", J::Bool(b.hidden)),
                ]),
            )]),
        ));
        nodes.push(J::Obj(node));
        body_nodes.push((nodes.len() - 1, b.layer));
    }

    // The layer nodes: the declared ones that hold a body, in declaration
    // order, then any layer a body names that nothing declared.
    let mut layer_order: Vec<GltfLayer> = layers
        .iter()
        .filter(|l| !l.name.is_empty() && body_nodes.iter().any(|(_, bl)| *bl == l.name))
        .cloned()
        .collect();
    for (_, bl) in &body_nodes {
        if !bl.is_empty() && !layer_order.iter().any(|l| l.name == *bl) {
            layer_order.push(GltfLayer { name: (*bl).to_owned(), visible: true, opacity: 1.0, color: None });
        }
    }
    // Node indices: root 0, layers 1..=L, bodies after. Bodies were pushed
    // first, so shift them.
    let shift = 1 + layer_order.len();
    let mut all: Vec<J> = Vec::with_capacity(nodes.len() + shift);
    let mut root_children: Vec<J> = Vec::new();
    let mut layer_nodes: Vec<J> = Vec::new();
    for (li, l) in layer_order.iter().enumerate() {
        root_children.push(J::Int((1 + li) as u64));
        let children: Vec<J> =
            body_nodes.iter().filter(|(_, bl)| *bl == l.name).map(|(i, _)| J::Int((i + shift) as u64)).collect();
        let mut extras = vec![
            ("layer", s(l.name.clone())),
            ("visible", J::Bool(l.visible)),
            ("opacity", J::F64(l.opacity.clamp(0.0, 1.0))),
        ];
        if let Some(c) = l.color {
            extras.push(("color", J::Arr(c.iter().map(|v| J::F64(*v)).collect())));
        }
        layer_nodes.push(J::Obj(vec![
            ("name", s(l.name.clone())),
            ("children", J::Arr(children)),
            ("extras", J::Obj(vec![("cadpole", J::Obj(extras))])),
        ]));
    }
    root_children.extend(body_nodes.iter().filter(|(_, bl)| bl.is_empty()).map(|(i, _)| J::Int((i + shift) as u64)));
    let mut root = vec![("name", s(opts.title.clone()))];
    if !root_children.is_empty() {
        root.push(("children", J::Arr(root_children)));
    }
    all.push(J::Obj(root));
    all.extend(layer_nodes);
    all.extend(nodes);

    let asset_extras = J::Obj(vec![(
        "cadpole",
        J::Obj(vec![
            ("title", s(opts.title.clone())),
            ("unit", s("m")),
            ("up", s("+Y")),
            ("model_unit", s(opts.model_unit.clone())),
            ("mm_per_model_unit", J::F64(mm)),
            ("display_units", s(opts.display_unit.clone())),
            ("display_fraction", J::Int(u64::from(opts.display_fraction))),
            ("frame", s("Cadpole (x, y, z) in the model unit -> glTF (x, z, -y) in metres")),
        ]),
    )]);
    let mut root_json: Vec<(&'static str, J)> = vec![(
        "asset",
        J::Obj(vec![
            ("version", s("2.0")),
            ("generator", s(opts.generator.clone())),
            ("extras", asset_extras),
        ]),
    )];
    if strength_used {
        root_json.push(("extensionsUsed", J::Arr(vec![s("KHR_materials_emissive_strength")])));
    }
    root_json.push(("scene", J::Int(0)));
    root_json.push((
        "scenes",
        J::Arr(vec![J::Obj(vec![("name", s(opts.title.clone())), ("nodes", J::Arr(vec![J::Int(0)]))])]),
    ));
    root_json.push(("nodes", J::Arr(all)));
    if !meshes.is_empty() {
        root_json.push(("meshes", J::Arr(meshes)));
        root_json.push(("materials", J::Arr(materials)));
        root_json.push(("accessors", J::Arr(std::mem::take(&mut bin.accessors))));
        root_json.push(("bufferViews", J::Arr(std::mem::take(&mut bin.views))));
        let mut buffer = vec![("byteLength", J::Int(bin.bytes.len() as u64))];
        if !opts.binary {
            buffer.insert(0, ("uri", s(format!("data:application/octet-stream;base64,{}", base64(&bin.bytes)))));
        }
        root_json.push(("buffers", J::Arr(vec![J::Obj(buffer)])));
    }
    let mut text = String::new();
    write_json(&J::Obj(root_json), &mut text);
    if !opts.binary {
        text.push('\n');
        return text.into_bytes();
    }
    // GLB: a twelve-byte header, the JSON chunk padded with spaces and the
    // binary chunk with zeros, each to four bytes.
    let mut json = text.into_bytes();
    while json.len() % 4 != 0 {
        json.push(b' ');
    }
    let mut data = bin.bytes;
    while data.len() % 4 != 0 {
        data.push(0);
    }
    let total = 12 + 8 + json.len() + if data.is_empty() { 0 } else { 8 + data.len() };
    let mut out = Vec::with_capacity(total);
    out.extend_from_slice(b"glTF");
    out.extend_from_slice(&2u32.to_le_bytes());
    out.extend_from_slice(&(total as u32).to_le_bytes());
    out.extend_from_slice(&(json.len() as u32).to_le_bytes());
    out.extend_from_slice(&0x4E4F_534Au32.to_le_bytes());
    out.extend_from_slice(&json);
    if !data.is_empty() {
        out.extend_from_slice(&(data.len() as u32).to_le_bytes());
        out.extend_from_slice(&0x004E_4942u32.to_le_bytes());
        out.extend_from_slice(&data);
    }
    out
}

#[cfg(test)]
mod tests {
    use super::*;

    fn tri(colors: bool) -> Mesh {
        Mesh {
            positions: vec![0.0, 0.0, 0.0, 1000.0, 0.0, 0.0, 0.0, 2000.0, 3000.0],
            normals: vec![0.0, 0.0, 1.0, 0.0, 0.0, 1.0, 0.0, 0.0, 2.0],
            indices: vec![0, 1, 2],
            face_ids: vec![0],
            colors: if colors { vec![255, 0, 0, 0, 255, 0, 0, 0, 255] } else { Vec::new() },
        }
    }

    fn json_of(glb: &[u8]) -> String {
        assert_eq!(&glb[0..4], b"glTF");
        assert_eq!(u32::from_le_bytes(glb[4..8].try_into().unwrap()), 2);
        assert_eq!(u32::from_le_bytes(glb[8..12].try_into().unwrap()) as usize, glb.len());
        let n = u32::from_le_bytes(glb[12..16].try_into().unwrap()) as usize;
        assert_eq!(&glb[16..20], b"JSON");
        assert_eq!(n % 4, 0);
        String::from_utf8(glb[20..20 + n].to_vec()).unwrap()
    }

    #[test]
    fn base64_matches_the_rfc_vectors() {
        for (i, o) in [("", ""), ("f", "Zg=="), ("fo", "Zm8="), ("foo", "Zm9v"), ("foob", "Zm9vYg=="), ("fooba", "Zm9vYmE="), ("foobar", "Zm9vYmFy")] {
            assert_eq!(base64(i.as_bytes()), o);
        }
    }

    #[test]
    fn a_glb_has_its_header_chunks_and_the_frame_turned() {
        let m = tri(false);
        let mat = Material { name: "oak \"quarter\"".into(), ..Material::default() };
        let b = [GltfBody { name: "slab", body_id: "slab#1", layer: "", hidden: false, material: &mat, mesh: &m }];
        let glb = gltf(&b, &[], &GltfOptions::default());
        let json = json_of(&glb);
        // (1000, 0, 0) mm -> (1, 0, 0) m; (0, 2000, 3000) -> (0, 3, -2).
        assert!(json.contains(r#""min":[0,0,-2],"max":[1,3,0]"#), "{json}");
        assert!(json.contains(r#""name":"oak \"quarter\"""#), "names are escaped: {json}");
        assert!(json.contains(r#""unit":"m""#));
        // 3 positions + 3 normals (36 bytes each) + 3 indices (12).
        assert!(json.contains(r#""buffers":[{"byteLength":84}]"#), "{json}");
        let n = u32::from_le_bytes(glb[12..16].try_into().unwrap()) as usize;
        let at = 20 + n;
        assert_eq!(u32::from_le_bytes(glb[at..at + 4].try_into().unwrap()), 84);
        assert_eq!(&glb[at + 4..at + 8], b"BIN\0");
        // The normal (0, 0, 2) is (0, 2, 0) turned, then made unit length.
        let normal_at = at + 8 + 36 + 24;
        let y = f32::from_le_bytes(glb[normal_at + 4..normal_at + 8].try_into().unwrap());
        assert_eq!(y, 1.0);
    }

    #[test]
    fn feet_scale_and_the_display_units_are_recorded() {
        let m = tri(false);
        let mat = Material::default();
        let b = [GltfBody { name: "wall", body_id: "w#1", layer: "", hidden: false, material: &mat, mesh: &m }];
        let opts = GltfOptions { mm_per_unit: 304.8, model_unit: "ft".into(), display_unit: "ft-in".into(), display_fraction: 16, ..Default::default() };
        let json = json_of(&gltf(&b, &[], &opts));
        // 1000 ft is 304.8 m.
        assert!(json.contains(r#""max":[304.8,914.4,0]"#), "{json}");
        assert!(json.contains(r#""model_unit":"ft","mm_per_model_unit":304.8,"display_units":"ft-in","display_fraction":16"#), "{json}");
    }

    #[test]
    fn layers_become_parent_nodes_in_declaration_order() {
        let m = tri(true);
        let mat = Material::default();
        let glass = Material { name: "glass".into(), opacity: 0.3, emissive: [2.0, 0.0, 0.0], ..Material::default() };
        let b = [
            GltfBody { name: "a", body_id: "a#1", layer: "site", hidden: true, material: &mat, mesh: &m },
            GltfBody { name: "b", body_id: "b#2", layer: "", hidden: false, material: &glass, mesh: &m },
            GltfBody { name: "c", body_id: "c#3", layer: "house", hidden: false, material: &mat, mesh: &m },
        ];
        let layers = [
            GltfLayer { name: "house".into(), visible: true, opacity: 1.0, color: None },
            GltfLayer { name: "unused".into(), visible: true, opacity: 1.0, color: None },
            GltfLayer { name: "site".into(), visible: false, opacity: 0.5, color: Some([0.1, 0.2, 0.3]) },
        ];
        let text = String::from_utf8(gltf(&b, &layers, &GltfOptions { binary: false, ..Default::default() })).unwrap();
        // Root, house, site, then the bodies a, b, c at 3, 4, 5.
        assert!(text.contains(r#"{"name":"model","children":[1,2,4]}"#), "{text}");
        assert!(text.contains(r#"{"name":"house","children":[5],"extras":{"cadpole":{"layer":"house","visible":true,"opacity":1}}}"#), "{text}");
        assert!(text.contains(r#""layer":"site","visible":false,"opacity":0.5,"color":[0.1,0.2,0.3]"#), "{text}");
        assert!(text.contains(r#""body":"a#1","layer":"site","hidden":true"#), "{text}");
        assert!(!text.contains("unused"), "an empty layer gets no node");
        assert!(text.contains("COLOR_0"));
        assert!(text.contains(r#""alphaMode":"BLEND""#));
        assert!(text.contains(r#""emissiveStrength":2"#) && text.contains(r#""extensionsUsed":["KHR_materials_emissive_strength"]"#));
        // Two materials for three bodies: the default is written once.
        assert_eq!(text.matches(r#""pbrMetallicRoughness""#).count(), 2);
        assert!(text.contains(r#""name":"default""#) && text.contains(r#""name":"glass""#));
        assert!(text.contains(r#""uri":"data:application/octet-stream;base64,"#));
        // And the bytes do not move between two runs.
        assert_eq!(text.into_bytes(), gltf(&b, &layers, &GltfOptions { binary: false, ..Default::default() }));
    }

    #[test]
    fn a_collapsed_triangle_is_left_out_and_an_empty_body_has_no_mesh() {
        let mut m = tri(false);
        m.indices.extend_from_slice(&[0, 0, 1]);
        let empty = Mesh { positions: Vec::new(), normals: Vec::new(), indices: Vec::new(), face_ids: Vec::new(), colors: Vec::new() };
        let mat = Material::default();
        let b = [
            GltfBody { name: "t", body_id: "t#1", layer: "", hidden: false, material: &mat, mesh: &m },
            GltfBody { name: "e", body_id: "e#2", layer: "", hidden: false, material: &mat, mesh: &empty },
        ];
        let json = json_of(&gltf(&b, &[], &GltfOptions::default()));
        assert!(json.contains(r#""componentType":5125,"count":3,"type":"SCALAR""#), "{json}");
        assert_eq!(json.matches(r#""mesh":"#).count(), 1, "{json}");
    }
}
