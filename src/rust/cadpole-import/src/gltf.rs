//! glTF 2.0 into [`RawObject`]s: a binary GLB or a JSON `.gltf` whose
//! buffers are `data:` URIs or `.bin` assets beside it.
//!
//! The subset a mesh import needs is small, and it is read here rather than
//! through a crate: a GLB is a twelve-byte header, a JSON chunk and a
//! binary chunk; the JSON is parsed by the `serde_json` already in the
//! graph, and everything else is bounds-checked slicing. What is read:
//!
//! * **The scene graph.** The default `scene`, else the first of the
//!   `scenes`, else every root node (a node nobody lists as a child); a
//!   file with meshes and no nodes at all imports each mesh where it is
//!   defined. `matrix` (column-major) or `translation`/`rotation`/`scale`
//!   compose down the hierarchy; a node whose world transform mirrors
//!   (negative determinant) has its triangles re-wound, as the
//!   specification says a renderer must. A mesh placed by several nodes is
//!   several bodies. The hierarchy is walked with a stack of its own,
//!   [`MAX_NODE_DEPTH`] deep, and a node reached twice — a cycle, or a node
//!   with two parents, both of which the format forbids — refuses the file
//!   by name.
//! * **Triangles.** Primitives of mode 4 (the default), indexed by `u8`,
//!   `u16` or `u32` or not indexed at all; points, lines, strips and fans
//!   are skipped with a warning that counts them. `POSITION` in `float32`
//!   (and, since the reader is generic, the normalised and plain integer
//!   forms `KHR_mesh_quantization` allows). Normals are recomputed, as for
//!   every mesh, so `NORMAL` is not read.
//! * **Names.** A body per mesh-carrying node, named by the node, else its
//!   mesh, else the asset's stem, sanitised, with `[k]` on every body that
//!   shares its name with another. Each primitive's material is a group —
//!   a named face `<feature>/<material>` —
//!   `material[k]` when the material has no name, `default` for a
//!   primitive with none; a body with one group is `<feature>/surface`
//!   unless its one material is named.
//! * **Colour.** `pbrMetallicRoughness.baseColorFactor`, the metallic and
//!   roughness factors, `emissiveFactor` and a `BLEND` material's alpha
//!   become the body's [`Appearance`] — the most-used material's. `COLOR_0`
//!   (`float`, normalised `u8` or `u16`, RGB or RGBA; the alpha is not
//!   used) becomes a colour per vertex, multiplied by its primitive's base
//!   colour; and a body whose primitives' base colours differ carries them
//!   per vertex too, so every face shows its own material. Both are linear
//!   in the file and stored sRGB-encoded ([`linear_to_srgb8`]). Textures
//!   and `KHR_materials_unlit` are said and not applied.
//!
//! Not read, and said once each: textures, skins, morph targets,
//! animations, cameras, lights, sparse accessors (the primitive that needs
//! one is skipped). Required extensions that change geometry — Draco and
//! meshopt compression above all — refuse the file naming the extension;
//! the ones that only change looks are read past.

use std::collections::{BTreeMap, HashMap, HashSet};
use std::sync::{Arc, OnceLock};

use serde_json::{Map, Value};

use crate::Note;
use crate::label;
use crate::mesh::{Appearance, RawObject, Resources};
use crate::prescan::{MAX_GLTF_JSON_BYTES, MAX_NODE_DEPTH, MAX_TRIANGLES, MAX_VERTICES};

const GLB_MAGIC: &[u8; 4] = b"glTF";
const CHUNK_JSON: u32 = 0x4E4F_534A;
const CHUNK_BIN: u32 = 0x004E_4942;

/// Extensions a file may *require* and still be read: each only changes
/// how a surface looks, which this reader either applies or says it does
/// not, or (`KHR_mesh_quantization`) widens the attribute types, which the
/// generic accessor reader already handles.
const TOLERATED_REQUIRED: &[&str] = &[
    "KHR_mesh_quantization",
    "KHR_materials_unlit",
    "KHR_materials_emissive_strength",
    "KHR_materials_ior",
    "KHR_materials_specular",
    "KHR_materials_transmission",
    "KHR_materials_volume",
    "KHR_materials_clearcoat",
    "KHR_materials_sheen",
    "KHR_materials_iridescence",
    "KHR_materials_anisotropy",
    "KHR_materials_dispersion",
    "KHR_materials_variants",
    "KHR_texture_transform",
    "KHR_texture_basisu",
    "EXT_texture_webp",
    "EXT_texture_avif",
    "KHR_lights_punctual",
];

fn u32_at(b: &[u8], at: usize) -> u32 {
    u32::from_le_bytes([b[at], b[at + 1], b[at + 2], b[at + 3]])
}

// ---------------------------------------------------------------- the container

/// A file's JSON and, for a GLB, its binary chunk.
struct Container<'a> {
    json: &'a [u8],
    bin: Option<&'a [u8]>,
    glb: bool,
}

/// Split a GLB into its chunks, or take a `.gltf` as JSON, checking every
/// length against the bytes there are.
fn container<'a>(bytes: &'a [u8], notes: &mut Vec<Note>) -> Result<Container<'a>, String> {
    if !bytes.starts_with(GLB_MAGIC) {
        let json = bytes.strip_prefix(&[0xEF, 0xBB, 0xBF]).unwrap_or(bytes);
        if json.len() > MAX_GLTF_JSON_BYTES {
            return Err(format!(
                "{} bytes of glTF JSON is over the {} MB limit",
                json.len(),
                MAX_GLTF_JSON_BYTES / (1024 * 1024)
            ));
        }
        return Ok(Container { json, bin: None, glb: false });
    }
    if bytes.len() < 12 {
        return Err(format!("a GLB starts with a 12-byte header; this file is {} bytes", bytes.len()));
    }
    let version = u32_at(bytes, 4);
    if version != 2 {
        return Err(format!("GLB version {version}: Cadpole reads glTF 2.0 (version 2)"));
    }
    let declared = u32_at(bytes, 8) as usize;
    if declared > bytes.len() {
        return Err(format!("the GLB header says {declared} bytes but the file has {}: truncated", bytes.len()));
    }
    if declared < bytes.len() {
        notes.push(Note::info(format!(
            "{} bytes after the {declared} the GLB header declares are ignored",
            bytes.len() - declared
        )));
    }
    let end = declared;
    let mut at = 12usize;
    let mut json: Option<&[u8]> = None;
    let mut bin: Option<&[u8]> = None;
    let mut extra = 0usize;
    let mut k = 0usize;
    while at < end {
        if end - at < 8 {
            return Err(format!("chunk {k} at byte {at}: {} bytes left, too few for a chunk header (8)", end - at));
        }
        let len = u32_at(bytes, at) as usize;
        let kind = u32_at(bytes, at + 4);
        let start = at + 8;
        if len > end - start {
            return Err(format!(
                "chunk {k} at byte {at} says it holds {len} bytes, but only {} remain before the end of the GLB ({end})",
                end - start
            ));
        }
        let data = &bytes[start..start + len];
        match (k, kind) {
            (0, CHUNK_JSON) => json = Some(data),
            (0, other) => {
                return Err(format!(
                    "the first chunk (byte 12) is of type 0x{other:08X}, not JSON (0x{CHUNK_JSON:08X}); a GLB's first chunk is its JSON"
                ));
            }
            (_, CHUNK_BIN) if bin.is_none() => bin = Some(data),
            _ => extra += 1,
        }
        // Chunks are padded to four bytes; a writer that did not pad is
        // read anyway.
        at = start + len;
        if at % 4 != 0 && at + (4 - at % 4) <= end {
            at += 4 - at % 4;
        }
        k += 1;
    }
    if extra > 0 {
        notes.push(Note::info(format!("{extra} GLB chunk{} after the binary one ignored", if extra == 1 { "" } else { "s" })));
    }
    let json = json.ok_or("the GLB has no chunks: a GLB holds a JSON chunk at least")?;
    if json.len() > MAX_GLTF_JSON_BYTES {
        return Err(format!("the GLB's JSON chunk is {} bytes, over the {} MB limit", json.len(), MAX_GLTF_JSON_BYTES / (1024 * 1024)));
    }
    Ok(Container { json, bin, glb: true })
}

fn parse_json(c: &Container<'_>) -> Result<Value, String> {
    let v: Value = serde_json::from_slice(c.json)
        .map_err(|e| format!("the glTF JSON{} is not valid: {e}", if c.glb { " chunk" } else { "" }))?;
    if !v.is_object() {
        return Err("the glTF JSON is not an object".into());
    }
    Ok(v)
}

// ---------------------------------------------------------------- JSON helpers

fn list<'a>(root: &'a Value, key: &str) -> Result<&'a [Value], String> {
    match root.get(key) {
        None | Some(Value::Null) => Ok(&[]),
        Some(Value::Array(a)) => Ok(a.as_slice()),
        Some(_) => Err(format!("`{key}` is not an array")),
    }
}

fn item<'a>(items: &'a [Value], key: &str, i: usize) -> Result<&'a Map<String, Value>, String> {
    match items.get(i) {
        Some(Value::Object(o)) => Ok(o),
        Some(_) => Err(format!("{key}[{i}] is not an object")),
        None => Err(format!("{key}[{i}] does not exist (there are {})", items.len())),
    }
}

fn index(o: &Map<String, Value>, key: &str, path: &str) -> Result<Option<usize>, String> {
    match o.get(key) {
        None | Some(Value::Null) => Ok(None),
        Some(v) => match v.as_u64() {
            Some(n) if n <= u32::MAX as u64 => Ok(Some(n as usize)),
            _ => Err(format!("{path}.{key} is {v}, not an index")),
        },
    }
}

fn number(o: &Map<String, Value>, key: &str, path: &str) -> Result<Option<f64>, String> {
    match o.get(key) {
        None | Some(Value::Null) => Ok(None),
        Some(v) => match v.as_f64() {
            Some(x) if x.is_finite() => Ok(Some(x)),
            _ => Err(format!("{path}.{key} is {v}, not a finite number")),
        },
    }
}

fn numbers<const N: usize>(o: &Map<String, Value>, key: &str, path: &str) -> Result<Option<[f64; N]>, String> {
    let Some(v) = o.get(key) else { return Ok(None) };
    let a = v.as_array().ok_or_else(|| format!("{path}.{key} is not an array"))?;
    if a.len() != N {
        return Err(format!("{path}.{key} has {} numbers, not {N}", a.len()));
    }
    let mut out = [0.0; N];
    for (k, x) in a.iter().enumerate() {
        out[k] = x.as_f64().filter(|x| x.is_finite()).ok_or_else(|| format!("{path}.{key}[{k}] is {x}, not a finite number"))?;
    }
    Ok(Some(out))
}

fn text<'a>(o: &'a Map<String, Value>, key: &str) -> &'a str {
    o.get(key).and_then(Value::as_str).unwrap_or("")
}

fn plural(n: usize, one: &str, many: &str) -> String {
    format!("{n} {}", if n == 1 { one } else { many })
}

// ---------------------------------------------------------------- buffers

/// A buffer's bytes: the GLB's own chunk, a decoded `data:` URI, or an
/// asset beside the `.gltf`.
enum Data<'a> {
    Borrowed(&'a [u8]),
    Owned(Vec<u8>),
    Shared(Arc<[u8]>),
}

impl Data<'_> {
    fn bytes(&self) -> &[u8] {
        match self {
            Data::Borrowed(b) => b,
            Data::Owned(v) => v,
            Data::Shared(a) => a,
        }
    }
}

/// The asset a buffer's `uri` names: its last path segment, percent-decoded.
/// A URL (`http:`, `file:`, anything with a scheme) is refused: an asset is
/// a name in the document's table and never a path or a fetch
/// (docs/security.md).
pub fn asset_name_of_uri(uri: &str) -> Result<String, String> {
    let scheme = uri.split_once(':').map(|(s, _)| s).filter(|s| {
        !s.is_empty() && s.chars().next().is_some_and(|c| c.is_ascii_alphabetic())
            && s.chars().all(|c| c.is_ascii_alphanumeric() || c == '+' || c == '-' || c == '.')
    });
    if let Some(s) = scheme {
        return Err(format!(
            "the buffer URI {} is a {s}: URL; a glTF's buffers are read from the document's assets (a `.bin` beside the `.gltf`) or from data: URIs, never fetched",
            label::quoted(uri)
        ));
    }
    let decoded = percent_decode(uri);
    let last = decoded.rsplit(['/', '\\']).next().unwrap_or("").to_owned();
    if last.is_empty() || last == "." || last == ".." {
        return Err(format!("the buffer URI {} names no file", label::quoted(uri)));
    }
    Ok(last)
}

fn percent_decode(s: &str) -> String {
    String::from_utf8_lossy(&percent_decode_bytes(s)).into_owned()
}

/// `%xx` escapes as the bytes they stand for; anything else as it is.
fn percent_decode_bytes(s: &str) -> Vec<u8> {
    let b = s.as_bytes();
    let mut out = Vec::with_capacity(b.len());
    let mut i = 0;
    while i < b.len() {
        if b[i] == b'%' && i + 2 < b.len() {
            let hex = std::str::from_utf8(&b[i + 1..i + 3]).ok().and_then(|h| u8::from_str_radix(h, 16).ok());
            if let Some(v) = hex {
                out.push(v);
                i += 3;
                continue;
            }
        }
        out.push(b[i]);
        i += 1;
    }
    out
}

/// Standard base64 (RFC 4648 §4), padding optional; `None` on any other
/// character.
fn base64_decode(s: &str) -> Option<Vec<u8>> {
    let s = s.trim_end_matches('=');
    let mut out = Vec::with_capacity(s.len() * 3 / 4 + 3);
    let mut acc = 0u32;
    let mut bits = 0u32;
    for c in s.bytes() {
        let v = match c {
            b'A'..=b'Z' => c - b'A',
            b'a'..=b'z' => c - b'a' + 26,
            b'0'..=b'9' => c - b'0' + 52,
            b'+' | b'-' => 62,
            b'/' | b'_' => 63,
            _ => return None,
        } as u32;
        acc = (acc << 6) | v;
        bits += 6;
        if bits >= 8 {
            bits -= 8;
            out.push((acc >> bits) as u8);
            acc &= (1 << bits) - 1;
        }
    }
    Some(out)
}

/// The bytes of a `data:` URI: base64 when it says so, percent-decoded
/// otherwise.
fn data_uri(uri: &str, what: &str) -> Result<Vec<u8>, String> {
    let rest = &uri[5..];
    let (meta, payload) = rest.split_once(',').ok_or_else(|| format!("{what}: a data: URI without a comma"))?;
    if meta.split(';').any(|p| p.eq_ignore_ascii_case("base64")) {
        base64_decode(payload).ok_or_else(|| format!("{what}: the data: URI is not valid base64"))
    } else {
        Ok(percent_decode_bytes(payload))
    }
}

/// Every buffer of the file that lives in another asset, as `(uri, asset
/// name)`, in buffer order: what a caller must look up in its asset table
/// and pass back as [`Resources`] — and key its memo by — before the file
/// can be read. A GLB's own binary chunk and `data:` URIs need nothing.
pub fn external_buffers(bytes: &[u8]) -> Result<Vec<(String, String)>, String> {
    let mut notes = Vec::new();
    let c = container(bytes, &mut notes)?;
    let root = parse_json(&c)?;
    let mut out = Vec::new();
    for (i, b) in list(&root, "buffers")?.iter().enumerate() {
        let Some(uri) = b.get("uri").and_then(Value::as_str) else { continue };
        if uri.starts_with("data:") {
            continue;
        }
        let name = asset_name_of_uri(uri).map_err(|e| format!("buffers[{i}]: {e}"))?;
        out.push((uri.to_owned(), name));
    }
    Ok(out)
}

// ---------------------------------------------------------------- accessors

#[derive(Clone, Copy, Debug)]
struct Accessor {
    /// `None` for an accessor with no buffer view (all zeros until a
    /// sparse substitution fills it).
    view: Option<usize>,
    offset: u64,
    ctype: u32,
    normalized: bool,
    count: usize,
    ncomp: usize,
    sparse: bool,
}

fn component_size(ctype: u32) -> Option<u64> {
    match ctype {
        5120 | 5121 => Some(1),
        5122 | 5123 => Some(2),
        5125 | 5126 => Some(4),
        _ => None,
    }
}

fn component_name(ctype: u32) -> &'static str {
    match ctype {
        5120 => "int8",
        5121 => "uint8",
        5122 => "int16",
        5123 => "uint16",
        5125 => "uint32",
        5126 => "float32",
        _ => "unknown",
    }
}

struct Reader<'a> {
    root: &'a Value,
    bin: Option<&'a [u8]>,
    resources: &'a Resources,
    buffers: Vec<Option<Data<'a>>>,
    accessors: &'a [Value],
    views: &'a [Value],
    buffer_list: &'a [Value],
}

impl<'a> Reader<'a> {
    fn buffer(&mut self, i: usize) -> Result<&[u8], String> {
        if i >= self.buffer_list.len() {
            return Err(format!("buffers[{i}] does not exist (there are {})", self.buffer_list.len()));
        }
        if self.buffers[i].is_none() {
            let path = format!("buffers[{i}]");
            let b = item(self.buffer_list, "buffers", i)?;
            let declared = index(b, "byteLength", &path)?.ok_or_else(|| format!("{path} has no byteLength"))?;
            let data = match b.get("uri").and_then(Value::as_str) {
                None => match (i, self.bin) {
                    (0, Some(bin)) => Data::Borrowed(bin),
                    (0, None) => return Err(format!("{path} has no uri and the file has no binary chunk")),
                    _ => return Err(format!("{path} has no uri; only the first buffer of a GLB is its binary chunk")),
                },
                Some(uri) if uri.starts_with("data:") => Data::Owned(data_uri(uri, &path)?),
                Some(uri) => {
                    let name = asset_name_of_uri(uri).map_err(|e| format!("{path}: {e}"))?;
                    match self.resources.get(&name) {
                        Some(a) => Data::Shared(a.clone()),
                        None => {
                            return Err(format!(
                                "{path} is the file {}, which is not an asset of this document: put it in ./assets beside the .gltf (the importer resolves a buffer by its file name, {})",
                                label::quoted(uri),
                                label::quoted(&name)
                            ));
                        }
                    }
                }
            };
            if data.bytes().len() < declared {
                return Err(format!("{path} declares {declared} bytes but holds {}", data.bytes().len()));
            }
            self.buffers[i] = Some(data);
        }
        let declared = item(self.buffer_list, "buffers", i).ok().and_then(|b| b.get("byteLength")).and_then(Value::as_u64).unwrap_or(0) as usize;
        let all = self.buffers[i].as_ref().map(Data::bytes).unwrap_or(&[]);
        Ok(&all[..declared.min(all.len())])
    }

    fn accessor(&self, i: usize, path: &str) -> Result<Accessor, String> {
        let a = item(self.accessors, "accessors", i).map_err(|e| format!("{path}: {e}"))?;
        let ap = format!("accessors[{i}]");
        let view = index(a, "bufferView", &ap)?;
        let ctype = index(a, "componentType", &ap)?.ok_or_else(|| format!("{ap} has no componentType"))? as u32;
        if component_size(ctype).is_none() {
            return Err(format!("{ap}.componentType is {ctype}, not one glTF defines (5120-5123, 5125, 5126)"));
        }
        let ncomp = match text(a, "type") {
            "SCALAR" => 1,
            "VEC2" => 2,
            "VEC3" => 3,
            "VEC4" => 4,
            "MAT2" => 4,
            "MAT3" => 9,
            "MAT4" => 16,
            other => return Err(format!("{ap}.type is {}, not SCALAR, VEC2, VEC3, VEC4 or MAT2-4", label::quoted(other))),
        };
        let count = index(a, "count", &ap)?.ok_or_else(|| format!("{ap} has no count"))?;
        Ok(Accessor {
            view,
            offset: index(a, "byteOffset", &ap)?.unwrap_or(0) as u64,
            ctype,
            normalized: a.get("normalized").and_then(Value::as_bool).unwrap_or(false),
            count,
            ncomp,
            sparse: a.contains_key("sparse"),
        })
    }

    /// Why an attribute or index accessor cannot be read here, when that is
    /// a reason to skip its primitive rather than to refuse the file.
    fn unreadable(&self, acc: &Accessor, ai: usize) -> Option<String> {
        if acc.sparse {
            return Some(format!("accessors[{ai}] is sparse, and sparse accessors are not read"));
        }
        if acc.view.is_none() {
            return Some(format!("accessors[{ai}] has no bufferView (all zeros), which only a sparse accessor fills"));
        }
        None
    }

    /// The bytes of an accessor's elements and the stride between them,
    /// every length checked against the buffer.
    fn elements(&mut self, acc: &Accessor, ai: usize) -> Result<(&[u8], usize), String> {
        let ap = format!("accessors[{ai}]");
        let view = acc.view.ok_or_else(|| format!("{ap} has no bufferView"))?;
        let vp = format!("bufferViews[{view}]");
        let v = item(self.views, "bufferViews", view).map_err(|e| format!("{ap}: {e}"))?;
        let buffer = index(v, "buffer", &vp)?.ok_or_else(|| format!("{vp} has no buffer"))?;
        let v_off = index(v, "byteOffset", &vp)?.unwrap_or(0) as u64;
        let v_len = index(v, "byteLength", &vp)?.ok_or_else(|| format!("{vp} has no byteLength"))? as u64;
        let elem = component_size(acc.ctype).unwrap_or(1) * acc.ncomp as u64;
        let stride = match index(v, "byteStride", &vp)? {
            Some(s) if (s as u64) < elem => {
                return Err(format!("{vp}.byteStride is {s}, less than the {elem}-byte element {ap} reads through it"));
            }
            Some(s) => s as u64,
            None => elem,
        };
        let data = self.buffer(buffer).map_err(|e| format!("{vp}: {e}"))?;
        if v_off.checked_add(v_len).is_none_or(|e| e > data.len() as u64) {
            return Err(format!("{vp} spans bytes {v_off}..{} of buffers[{buffer}], which has {}", v_off.saturating_add(v_len), data.len()));
        }
        if acc.offset > v_len {
            return Err(format!("{ap}.byteOffset is {}, past the {v_len} bytes of {vp}", acc.offset));
        }
        if acc.count > 0 {
            let need = acc.offset.checked_add(stride.checked_mul(acc.count as u64 - 1).unwrap_or(u64::MAX)).and_then(|x| x.checked_add(elem));
            if need.is_none_or(|n| n > v_len) {
                return Err(format!(
                    "{ap} reads {} elements of {elem} bytes every {stride} from byte {} of {vp}, past its {v_len} bytes",
                    acc.count, acc.offset
                ));
            }
        }
        let start = (v_off + acc.offset) as usize;
        let end = (v_off + v_len) as usize;
        Ok((&data[start..end], stride as usize))
    }

    /// An accessor's values as `f32`, `ncomp` per element, with glTF's
    /// normalisation for integer components.
    fn floats(&mut self, acc: Accessor, ai: usize, path: &str, max_count: usize) -> Result<Vec<f32>, String> {
        if acc.count > max_count {
            return Err(format!("{path}: accessors[{ai}] has {} elements, over the {max_count} limit", acc.count));
        }
        let (bytes, stride) = self.elements(&acc, ai)?;
        let n = acc.ncomp;
        let size = component_size(acc.ctype).unwrap_or(4) as usize;
        let mut out = Vec::with_capacity(acc.count * n);
        for e in 0..acc.count {
            let base = e * stride;
            for c in 0..n {
                let at = base + c * size;
                let b = &bytes[at..at + size];
                let x = match (acc.ctype, acc.normalized) {
                    // The specification requires integer colours to be
                    // normalised; a writer that forgot is read as if not.
                    (5126, _) => f32::from_le_bytes([b[0], b[1], b[2], b[3]]),
                    (5121, true) => b[0] as f32 / 255.0,
                    (5121, false) => b[0] as f32,
                    (5120, true) => (b[0] as i8 as f32 / 127.0).max(-1.0),
                    (5120, false) => b[0] as i8 as f32,
                    (5123, true) => u16::from_le_bytes([b[0], b[1]]) as f32 / 65535.0,
                    (5123, false) => u16::from_le_bytes([b[0], b[1]]) as f32,
                    (5122, true) => (i16::from_le_bytes([b[0], b[1]]) as f32 / 32767.0).max(-1.0),
                    (5122, false) => i16::from_le_bytes([b[0], b[1]]) as f32,
                    (5125, _) => u32::from_le_bytes([b[0], b[1], b[2], b[3]]) as f32,
                    _ => 0.0,
                };
                if !x.is_finite() {
                    return Err(format!("{path}: accessors[{ai}] element {e} is not finite"));
                }
                out.push(x);
            }
        }
        Ok(out)
    }

    /// An index accessor: `uint8`, `uint16` or `uint32` scalars.
    fn indices(&mut self, acc: Accessor, ai: usize, path: &str) -> Result<Vec<u32>, String> {
        if acc.ncomp != 1 || !matches!(acc.ctype, 5121 | 5123 | 5125) {
            return Err(format!(
                "{path}: accessors[{ai}] is {} {}, but indices are uint8, uint16 or uint32 scalars",
                component_name(acc.ctype),
                if acc.ncomp == 1 { "SCALAR" } else { "not SCALAR" }
            ));
        }
        if acc.count > 3 * MAX_TRIANGLES {
            return Err(format!("{path}: accessors[{ai}] has {} indices, over the {} limit", acc.count, 3 * MAX_TRIANGLES));
        }
        let (bytes, stride) = self.elements(&acc, ai)?;
        let mut out = Vec::with_capacity(acc.count);
        for e in 0..acc.count {
            let b = &bytes[e * stride..];
            out.push(match acc.ctype {
                5121 => b[0] as u32,
                5123 => u16::from_le_bytes([b[0], b[1]]) as u32,
                _ => u32::from_le_bytes([b[0], b[1], b[2], b[3]]),
            });
        }
        Ok(out)
    }
}

// ---------------------------------------------------------------- materials

#[derive(Clone, Debug)]
struct Material {
    name: String,
    base: [f64; 4],
    metallic: f64,
    roughness: f64,
    emissive: [f64; 3],
    blend: bool,
    unlit: bool,
    textured: bool,
}

fn material(root: &Value, i: usize) -> Result<Material, String> {
    let mats = list(root, "materials")?;
    let m = item(mats, "materials", i)?;
    let path = format!("materials[{i}]");
    let pbr = m.get("pbrMetallicRoughness").and_then(Value::as_object);
    let empty = Map::new();
    let pbr_o = pbr.unwrap_or(&empty);
    let pp = format!("{path}.pbrMetallicRoughness");
    let base = numbers::<4>(pbr_o, "baseColorFactor", &pp)?.unwrap_or([1.0; 4]).map(|c| c.clamp(0.0, 1.0));
    let ext = m.get("extensions").and_then(Value::as_object);
    let strength = ext
        .and_then(|e| e.get("KHR_materials_emissive_strength"))
        .and_then(|s| s.get("emissiveStrength"))
        .and_then(Value::as_f64)
        .filter(|s| s.is_finite() && *s >= 0.0)
        .unwrap_or(1.0);
    let emissive = numbers::<3>(m, "emissiveFactor", &path)?.unwrap_or([0.0; 3]).map(|c| c.clamp(0.0, 1.0) * strength);
    Ok(Material {
        name: text(m, "name").to_owned(),
        base,
        // The specification's defaults: a material that names no factor is
        // fully metallic and fully rough.
        metallic: number(pbr_o, "metallicFactor", &pp)?.unwrap_or(1.0).clamp(0.0, 1.0),
        roughness: number(pbr_o, "roughnessFactor", &pp)?.unwrap_or(1.0).clamp(0.0, 1.0),
        emissive,
        blend: text(m, "alphaMode") == "BLEND",
        unlit: ext.is_some_and(|e| e.contains_key("KHR_materials_unlit")),
        textured: pbr_o.contains_key("baseColorTexture")
            || pbr_o.contains_key("metallicRoughnessTexture")
            || ["normalTexture", "occlusionTexture", "emissiveTexture"].iter().any(|k| m.contains_key(*k)),
    })
}

// ---------------------------------------------------------------- the scene graph

/// A 4x4 transform, column-major as glTF writes it.
type Mat4 = [f64; 16];

const IDENTITY: Mat4 = [1.0, 0.0, 0.0, 0.0, 0.0, 1.0, 0.0, 0.0, 0.0, 0.0, 1.0, 0.0, 0.0, 0.0, 0.0, 1.0];

fn mul(a: &Mat4, b: &Mat4) -> Mat4 {
    let mut r = [0.0; 16];
    for c in 0..4 {
        for row in 0..4 {
            r[c * 4 + row] = (0..4).map(|k| a[k * 4 + row] * b[c * 4 + k]).sum();
        }
    }
    r
}

fn apply(m: &Mat4, p: [f64; 3]) -> [f64; 3] {
    [
        m[0] * p[0] + m[4] * p[1] + m[8] * p[2] + m[12],
        m[1] * p[0] + m[5] * p[1] + m[9] * p[2] + m[13],
        m[2] * p[0] + m[6] * p[1] + m[10] * p[2] + m[14],
    ]
}

fn det3(m: &Mat4) -> f64 {
    m[0] * (m[5] * m[10] - m[9] * m[6]) - m[4] * (m[1] * m[10] - m[9] * m[2]) + m[8] * (m[1] * m[6] - m[5] * m[2])
}

/// A node's own transform: its `matrix`, or its translation, rotation (a
/// unit quaternion, x y z w) and scale, applied scale first.
fn local(n: &Map<String, Value>, path: &str) -> Result<Mat4, String> {
    if let Some(m) = numbers::<16>(n, "matrix", path)? {
        return Ok(m);
    }
    let t = numbers::<3>(n, "translation", path)?.unwrap_or([0.0; 3]);
    let s = numbers::<3>(n, "scale", path)?.unwrap_or([1.0; 3]);
    let mut q = numbers::<4>(n, "rotation", path)?.unwrap_or([0.0, 0.0, 0.0, 1.0]);
    let len = (q[0] * q[0] + q[1] * q[1] + q[2] * q[2] + q[3] * q[3]).sqrt();
    if len > 0.0 {
        q = q.map(|c| c / len);
    } else {
        q = [0.0, 0.0, 0.0, 1.0];
    }
    let [x, y, z, w] = q;
    // Rotation columns, from the unit quaternion.
    let r = [
        [1.0 - 2.0 * (y * y + z * z), 2.0 * (x * y + z * w), 2.0 * (x * z - y * w)],
        [2.0 * (x * y - z * w), 1.0 - 2.0 * (x * x + z * z), 2.0 * (y * z + x * w)],
        [2.0 * (x * z + y * w), 2.0 * (y * z - x * w), 1.0 - 2.0 * (x * x + y * y)],
    ];
    Ok([
        r[0][0] * s[0],
        r[0][1] * s[0],
        r[0][2] * s[0],
        0.0,
        r[1][0] * s[1],
        r[1][1] * s[1],
        r[1][2] * s[1],
        0.0,
        r[2][0] * s[2],
        r[2][1] * s[2],
        r[2][2] * s[2],
        0.0,
        t[0],
        t[1],
        t[2],
        1.0,
    ])
}

/// One mesh placed by one node: what becomes a body.
struct Instance {
    mesh: usize,
    world: Mat4,
    /// The node's name, else the mesh's; raw.
    label: String,
}

/// Walk the scene graph: which meshes are placed, by which nodes, where.
fn instances(root: &Value, notes: &mut Vec<Note>) -> Result<Vec<Instance>, String> {
    let nodes = list(root, "nodes")?;
    let meshes = list(root, "meshes")?;
    let scenes = list(root, "scenes")?;
    let mesh_name = |m: usize| -> String {
        meshes.get(m).and_then(|v| v.get("name")).and_then(Value::as_str).unwrap_or("").to_owned()
    };
    if nodes.is_empty() {
        if !meshes.is_empty() {
            notes.push(Note::warning(format!(
                "the glTF has no nodes; its {} imported where {} defined",
                plural(meshes.len(), "mesh is", "meshes are"),
                if meshes.len() == 1 { "it is" } else { "they are" }
            )));
        }
        return Ok((0..meshes.len()).map(|m| Instance { mesh: m, world: IDENTITY, label: mesh_name(m) }).collect());
    }
    let roots: Vec<usize> = match index(root.as_object().expect("an object"), "scene", "the file")? {
        Some(s) => {
            let sc = item(scenes, "scenes", s).map_err(|e| format!("scene: {e}"))?;
            scene_roots(sc, s)?
        }
        None if !scenes.is_empty() => {
            if scenes.len() > 1 {
                notes.push(Note::info(format!(
                    "the glTF names no default scene; the first of its {} scenes is imported",
                    scenes.len()
                )));
            }
            scene_roots(item(scenes, "scenes", 0)?, 0)?
        }
        None => {
            // Every node nobody lists as a child.
            let mut child = vec![false; nodes.len()];
            for (i, n) in nodes.iter().enumerate() {
                if let Some(cs) = n.get("children").and_then(Value::as_array) {
                    for c in cs {
                        match c.as_u64() {
                            Some(c) if (c as usize) < nodes.len() => child[c as usize] = true,
                            _ => return Err(format!("nodes[{i}].children lists {c}, not a node")),
                        }
                    }
                }
            }
            notes.push(Note::info("the glTF has no scenes; every root node is imported"));
            (0..nodes.len()).filter(|&i| !child[i]).collect()
        }
    };
    let mut out = Vec::new();
    let mut seen: HashSet<usize> = HashSet::new();
    // (node, parent's world transform, depth), depth-first in file order.
    let mut stack: Vec<(usize, Mat4, usize)> = roots.iter().rev().map(|&r| (r, IDENTITY, 0)).collect();
    let (mut skinned, mut morphs) = (0usize, 0usize);
    while let Some((i, parent, depth)) = stack.pop() {
        let path = format!("nodes[{i}]");
        let n = item(nodes, "nodes", i)?;
        if !seen.insert(i) {
            return Err(format!(
                "{path} is reached twice in the scene graph: a glTF node has at most one parent and the graph has no cycles"
            ));
        }
        if depth > MAX_NODE_DEPTH {
            return Err(format!("{path} is more than {MAX_NODE_DEPTH} nodes deep"));
        }
        let world = mul(&parent, &local(n, &path)?);
        if let Some(m) = index(n, "mesh", &path)? {
            if m >= meshes.len() {
                return Err(format!("{path}.mesh is {m}, but there are {}", plural(meshes.len(), "mesh", "meshes")));
            }
            if n.contains_key("skin") {
                skinned += 1;
            }
            if meshes[m].get("weights").is_some() || n.contains_key("weights") {
                morphs += 1;
            }
            let label = match text(n, "name") {
                "" => mesh_name(m),
                s => s.to_owned(),
            };
            out.push(Instance { mesh: m, world, label });
        }
        if let Some(cs) = n.get("children") {
            let cs = cs.as_array().ok_or_else(|| format!("{path}.children is not an array"))?;
            for c in cs.iter().rev() {
                let c = c.as_u64().filter(|c| (*c as usize) < nodes.len()).ok_or_else(|| format!("{path}.children lists {c}, not a node"))?;
                stack.push((c as usize, world, depth + 1));
            }
        }
    }
    if skinned > 0 {
        notes.push(Note::info(format!(
            "{} skinned: skins are not read, so each mesh is where its node puts it, in its bind pose",
            plural(skinned, "node is", "nodes are")
        )));
    }
    if morphs > 0 {
        notes.push(Note::info(format!(
            "{} morph weights: morph targets are not read, so the base shape is imported",
            plural(morphs, "mesh has", "meshes have")
        )));
    }
    Ok(out)
}

fn scene_roots(sc: &Map<String, Value>, s: usize) -> Result<Vec<usize>, String> {
    let Some(ns) = sc.get("nodes") else { return Ok(Vec::new()) };
    let ns = ns.as_array().ok_or_else(|| format!("scenes[{s}].nodes is not an array"))?;
    ns.iter()
        .map(|v| v.as_u64().map(|v| v as usize).ok_or_else(|| format!("scenes[{s}].nodes lists {v}, not a node")))
        .collect()
}

// ---------------------------------------------------------------- primitives

/// One triangle primitive, read: its own vertices (those its indices use,
/// in the file's order), its triangles over them, its colours.
struct Prim {
    positions: Vec<[f32; 3]>,
    triangles: Vec<[u32; 3]>,
    /// Linear RGB per vertex, from `COLOR_0`.
    colors: Option<Vec<[f32; 3]>>,
    material: Option<usize>,
    /// Its index (or vertex) count was not a multiple of three; the
    /// remainder was dropped.
    ragged: bool,
}

/// What reading a mesh's primitives had to leave out.
#[derive(Default)]
struct Skipped {
    /// Primitives of a mode other than triangles, by mode.
    modes: BTreeMap<u64, usize>,
    /// Primitives refused, with the first reason.
    refused: Vec<String>,
    /// Primitives whose index or vertex count was not a multiple of three.
    ragged: usize,
    /// Primitives with no whole triangle.
    empty: usize,
}

fn mode_name(m: u64) -> &'static str {
    match m {
        0 => "points",
        1 => "lines",
        2 => "line loops",
        3 => "line strips",
        5 => "triangle strips",
        6 => "triangle fans",
        _ => "unknown modes",
    }
}

fn read_mesh(r: &mut Reader<'_>, m: usize, skipped: &mut Skipped) -> Result<Vec<Prim>, String> {
    let meshes = list(r.root, "meshes")?;
    let mesh = item(meshes, "meshes", m)?;
    let path = format!("meshes[{m}]");
    let prims = mesh.get("primitives").and_then(Value::as_array).ok_or_else(|| format!("{path} has no primitives"))?;
    let mut out = Vec::with_capacity(prims.len());
    for (p, prim) in prims.iter().enumerate() {
        let pp = format!("{path}.primitives[{p}]");
        let prim = prim.as_object().ok_or_else(|| format!("{pp} is not an object"))?;
        let mode = match prim.get("mode") {
            None => 4,
            Some(v) => v.as_u64().ok_or_else(|| format!("{pp}.mode is {v}, not a mode"))?,
        };
        if mode != 4 {
            *skipped.modes.entry(mode).or_default() += 1;
            continue;
        }
        if prim.get("extensions").and_then(|e| e.get("KHR_draco_mesh_compression")).is_some() {
            return Err(format!("{pp} is Draco-compressed (KHR_draco_mesh_compression), which Cadpole does not decode; re-export without compression"));
        }
        // A primitive that cannot be read — a sparse accessor, an attribute
        // of a type the format does not allow there — is skipped and said;
        // a structural fault (an index past the vertices, a view past its
        // buffer) refuses the file.
        match read_prim(r, prim, &pp)? {
            PrimRead::Read(mut got) => {
                got.material = index(prim, "material", &pp)?;
                skipped.ragged += got.ragged as usize;
                out.push(got);
            }
            PrimRead::Empty => skipped.empty += 1,
            PrimRead::Skipped(why) => skipped.refused.push(why),
        }
    }
    Ok(out)
}

/// What reading one primitive gave.
enum PrimRead {
    Read(Prim),
    /// No whole triangle.
    Empty,
    /// Not readable here, and why.
    Skipped(String),
}

/// Read one mode-4 primitive.
fn read_prim(r: &mut Reader<'_>, prim: &Map<String, Value>, pp: &str) -> Result<PrimRead, String> {
    let attrs = prim.get("attributes").and_then(Value::as_object).ok_or_else(|| format!("{pp} has no attributes"))?;
    let ap = format!("{pp}.attributes");
    let pos_i = index(attrs, "POSITION", &ap)?.ok_or_else(|| format!("{pp} has no POSITION"))?;
    let pos_acc = r.accessor(pos_i, &format!("{ap}.POSITION"))?;
    if let Some(why) = r.unreadable(&pos_acc, pos_i) {
        return Ok(PrimRead::Skipped(format!("{pp}: POSITION: {why}")));
    }
    if pos_acc.ncomp != 3 {
        return Ok(PrimRead::Skipped(format!("{pp}: POSITION is accessors[{pos_i}], which is not VEC3")));
    }
    let idx_acc = match index(prim, "indices", pp)? {
        Some(ai) => {
            let acc = r.accessor(ai, &format!("{pp}.indices"))?;
            if let Some(why) = r.unreadable(&acc, ai) {
                return Ok(PrimRead::Skipped(format!("{pp}: indices: {why}")));
            }
            Some((ai, acc))
        }
        None => None,
    };
    let col = match index(attrs, "COLOR_0", &ap)? {
        Some(ci) => {
            let mut acc = r.accessor(ci, &format!("{ap}.COLOR_0"))?;
            if let Some(why) = r.unreadable(&acc, ci) {
                return Ok(PrimRead::Skipped(format!("{pp}: COLOR_0: {why}")));
            }
            if acc.ncomp != 3 && acc.ncomp != 4 {
                return Ok(PrimRead::Skipped(format!("{pp}: COLOR_0 is accessors[{ci}], which is neither VEC3 nor VEC4")));
            }
            // Integer colours are normalised by definition.
            acc.normalized = true;
            Some((ci, acc))
        }
        None => None,
    };
    let pos = r.floats(pos_acc, pos_i, &format!("{ap}.POSITION"), MAX_VERTICES)?;
    let nv = pos.len() / 3;
    let idx: Vec<u32> = match idx_acc {
        Some((ai, acc)) => r.indices(acc, ai, &format!("{pp}.indices"))?,
        None => (0..nv as u32).collect(),
    };
    if let Some(at) = idx.iter().position(|&i| i as usize >= nv) {
        return Err(format!("{pp}: index {at} refers to vertex {}, but POSITION has {nv}", idx[at]));
    }
    let ntri = idx.len() / 3;
    if ntri == 0 {
        return Ok(PrimRead::Empty);
    }
    let colors = match col {
        Some((ci, acc)) => {
            let n = acc.ncomp;
            let c = r.floats(acc, ci, &format!("{ap}.COLOR_0"), MAX_VERTICES)?;
            if c.len() / n != nv {
                return Err(format!("{pp}: COLOR_0 has {} colours for {nv} positions", c.len() / n));
            }
            Some(c.chunks_exact(n).map(|c| [c[0], c[1], c[2]]).collect::<Vec<_>>())
        }
        None => None,
    };
    // Only the vertices the triangles use, in the file's order: a
    // primitive that indexes a slice of a shared vertex buffer takes that
    // slice, and one that uses every vertex keeps the file's numbering.
    let mut used = vec![u32::MAX; nv];
    for &i in &idx[..ntri * 3] {
        used[i as usize] = 0;
    }
    let mut positions = Vec::new();
    let mut kept_colors = colors.as_ref().map(|_| Vec::new());
    for v in 0..nv {
        if used[v] == 0 {
            used[v] = positions.len() as u32;
            positions.push([pos[v * 3], pos[v * 3 + 1], pos[v * 3 + 2]]);
            if let (Some(k), Some(c)) = (kept_colors.as_mut(), colors.as_ref()) {
                k.push(c[v]);
            }
        }
    }
    let triangles = idx[..ntri * 3].chunks_exact(3).map(|t| [used[t[0] as usize], used[t[1] as usize], used[t[2] as usize]]).collect();
    Ok(PrimRead::Read(Prim { positions, triangles, colors: kept_colors, material: None, ragged: idx.len() % 3 != 0 }))
}

// ---------------------------------------------------------------- colour

/// Linear 0..1 to sRGB-encoded 8-bit, through a table (4096 steps: under
/// one step of the 8-bit output everywhere) built once with `fmath`.
fn srgb8(c: f32) -> u8 {
    static LUT: OnceLock<Vec<u8>> = OnceLock::new();
    let lut = LUT.get_or_init(|| (0..=4096).map(|i| crate::mesh::linear_to_srgb8(i as f64 / 4096.0)).collect());
    let c = if c.is_finite() { c.clamp(0.0, 1.0) } else { 0.0 };
    lut[(c * 4096.0).round() as usize]
}

// ---------------------------------------------------------------- the reader

/// Parse `bytes` — a GLB or a `.gltf` — into one [`RawObject`] per placed
/// mesh, in the file's frame (metres, +Y up: [`crate::mesh`] turns it into
/// Cadpole's). `hint` is the asset's name, whose stem names an unnamed mesh;
/// `resources` holds the `.bin` assets a `.gltf` names
/// ([`external_buffers`]).
pub fn parse(bytes: &[u8], hint: &str, resources: &Resources) -> Result<(Vec<RawObject>, Vec<Note>), String> {
    let mut notes = Vec::new();
    let c = container(bytes, &mut notes)?;
    let root = parse_json(&c)?;
    let asset = root.get("asset").and_then(Value::as_object).ok_or("the glTF has no `asset` object (every glTF names its version there)")?;
    let version = text(asset, "version");
    if !version.starts_with("2.") {
        return Err(format!("glTF {}: Cadpole reads glTF 2.0", label::quoted(version)));
    }
    let names = |key: &str| -> Result<Vec<String>, String> {
        Ok(list(&root, key)?.iter().filter_map(Value::as_str).map(str::to_owned).collect())
    };
    let required = names("extensionsRequired")?;
    if let Some(bad) = required.iter().find(|e| !TOLERATED_REQUIRED.contains(&e.as_str())) {
        let why = match bad.as_str() {
            "KHR_draco_mesh_compression" | "EXT_meshopt_compression" | "KHR_meshopt_compression" => {
                " (compressed geometry; re-export the file without compression)"
            }
            _ => "",
        };
        return Err(format!("the glTF requires the extension {}, which Cadpole does not read{why}", label::quoted(bad)));
    }
    let generator = text(asset, "generator");
    let lists = |k: &str| list(&root, k).map(|l| l.len()).unwrap_or(0);
    notes.push(Note::info(format!(
        "glTF {version} ({}{}): {}, {}, {}",
        if c.glb { "binary, GLB" } else { "JSON" },
        if generator.is_empty() { String::new() } else { format!(", from {}", label::quoted(generator)) },
        plural(lists("nodes"), "node", "nodes"),
        plural(lists("meshes"), "mesh", "meshes"),
        plural(lists("materials"), "material", "materials"),
    )));
    let ignored: Vec<&str> = ["animations", "cameras", "skins"].into_iter().filter(|k| lists(k) > 0).collect();
    if !ignored.is_empty() {
        notes.push(Note::info(format!("not read: {}", ignored.join(", "))));
    }

    let places = instances(&root, &mut notes)?;
    if places.is_empty() {
        return Err("the glTF places no mesh: no node of its scene has one".into());
    }
    let accessors = list(&root, "accessors")?;
    let views = list(&root, "bufferViews")?;
    let buffer_list = list(&root, "buffers")?;
    let mut reader = Reader {
        root: &root,
        bin: c.bin,
        resources,
        buffers: (0..buffer_list.len()).map(|_| None).collect(),
        accessors,
        views,
        buffer_list,
    };

    // The whole build's size, from the accessors' counts, before any mesh
    // is read: a mesh placed by a thousand nodes is a thousand bodies.
    let meshes = list(&root, "meshes")?;
    let mut per_mesh: HashMap<usize, (usize, usize)> = HashMap::new();
    let (mut total_t, mut total_v) = (0usize, 0usize);
    for inst in &places {
        let (t, v) = match per_mesh.get(&inst.mesh) {
            Some(tv) => *tv,
            None => {
                let tv = mesh_size(&reader, meshes, inst.mesh)?;
                per_mesh.insert(inst.mesh, tv);
                tv
            }
        };
        total_t = total_t.saturating_add(t);
        total_v = total_v.saturating_add(v);
        if total_t > MAX_TRIANGLES {
            return Err(format!("the scene places more than {MAX_TRIANGLES} triangles (over every node that places a mesh)"));
        }
        if total_v > MAX_VERTICES {
            return Err(format!("the scene places more than {MAX_VERTICES} vertices (over every node that places a mesh)"));
        }
    }

    // Materials, read once.
    let mut materials: HashMap<usize, Material> = HashMap::new();
    let mut textured: Vec<String> = Vec::new();
    let mut unlit: Vec<String> = Vec::new();
    let mut skipped = Skipped::default();
    let mut cache: HashMap<usize, Vec<Prim>> = HashMap::new();
    // How many places each mesh has left, so its primitives are dropped
    // after the last.
    let mut left: HashMap<usize, usize> = HashMap::new();
    for inst in &places {
        *left.entry(inst.mesh).or_default() += 1;
    }

    // Body names: node, else mesh, else the asset's stem, then `[k]` on
    // every body that shares its name.
    let stem = crate::mesh::stem(hint);
    let base_names: Vec<String> = places
        .iter()
        .map(|p| {
            let n = label::sanitise(&p.label);
            if !n.is_empty() {
                n
            } else if !stem.is_empty() {
                stem.clone()
            } else {
                "object".to_owned()
            }
        })
        .collect();
    let mut counts: HashMap<&str, usize> = HashMap::new();
    for n in &base_names {
        *counts.entry(n.as_str()).or_default() += 1;
    }
    let mut ordinal: HashMap<&str, usize> = HashMap::new();
    let body_names: Vec<String> = base_names
        .iter()
        .map(|n| {
            if counts[n.as_str()] > 1 {
                let k = ordinal.entry(n.as_str()).or_default();
                let named = format!("{n}[{k}]");
                *k += 1;
                named
            } else {
                n.clone()
            }
        })
        .collect();

    let mut out = Vec::with_capacity(places.len());
    let mut mirrored = 0usize;
    for (inst, body_name) in places.iter().zip(body_names) {
        if !cache.contains_key(&inst.mesh) {
            let prims = read_mesh(&mut reader, inst.mesh, &mut skipped)?;
            for p in &prims {
                if let Some(mi) = p.material {
                    if let std::collections::hash_map::Entry::Vacant(e) = materials.entry(mi) {
                        let m = material(&root, mi)?;
                        let shown = if m.name.is_empty() { format!("materials[{mi}]") } else { label::quoted(&m.name) };
                        if m.textured {
                            textured.push(shown.clone());
                        }
                        if m.unlit {
                            unlit.push(shown);
                        }
                        e.insert(m);
                    }
                }
            }
            cache.insert(inst.mesh, prims);
        }
        let prims = &cache[&inst.mesh];
        let flip = det3(&inst.world) < 0.0;
        if flip {
            mirrored += 1;
        }
        out.push(body(prims, &materials, &inst.world, flip, body_name));
        let n = left.get_mut(&inst.mesh).expect("counted");
        *n -= 1;
        if *n == 0 {
            cache.remove(&inst.mesh);
        }
    }
    let out: Vec<RawObject> = out.into_iter().filter(|o| !o.triangles.is_empty()).collect();

    // What was left out, said once each.
    for (mode, n) in &skipped.modes {
        notes.push(Note::warning(format!(
            "{} of {} (mode {mode}) skipped: only triangles (mode 4) are imported",
            plural(*n, "primitive", "primitives"),
            mode_name(*mode)
        )));
    }
    if !skipped.refused.is_empty() {
        notes.push(Note::warning(format!(
            "{} skipped: {}",
            plural(skipped.refused.len(), "primitive", "primitives"),
            skipped.refused[0]
        )));
    }
    if skipped.empty > 0 {
        notes.push(Note::info(format!("{} with no whole triangle skipped", plural(skipped.empty, "primitive", "primitives"))));
    }
    if skipped.ragged > 0 {
        notes.push(Note::warning(format!(
            "{} an index (or vertex) count that is not a multiple of three; the last one or two are dropped",
            plural(skipped.ragged, "primitive has", "primitives have")
        )));
    }
    if mirrored > 0 {
        notes.push(Note::info(format!(
            "{} a mirroring transform: {} triangles are re-wound so the surface stays outward",
            plural(mirrored, "node has", "nodes have"),
            if mirrored == 1 { "its" } else { "their" }
        )));
    }
    if !textured.is_empty() {
        notes.push(Note::info(format!(
            "textures are not read ({}): each surface takes its material's base colour factor{}",
            textured.join(", "),
            " and any COLOR_0"
        )));
    }
    if !unlit.is_empty() {
        notes.push(Note::info(format!(
            "KHR_materials_unlit on {}: Cadpole has no unlit shading, so those colours are lit like any other surface",
            unlit.join(", ")
        )));
    }
    if out.is_empty() {
        return Err("the glTF has no triangles Cadpole reads (every primitive was skipped)".into());
    }
    Ok((out, notes))
}

/// Triangles and vertices a mesh will contribute, from its accessors'
/// counts alone.
fn mesh_size(r: &Reader<'_>, meshes: &[Value], m: usize) -> Result<(usize, usize), String> {
    let mesh = item(meshes, "meshes", m)?;
    let path = format!("meshes[{m}]");
    let prims = mesh.get("primitives").and_then(Value::as_array).ok_or_else(|| format!("{path} has no primitives"))?;
    let (mut t, mut v) = (0usize, 0usize);
    for (p, prim) in prims.iter().enumerate() {
        let pp = format!("{path}.primitives[{p}]");
        let Some(prim) = prim.as_object() else { return Err(format!("{pp} is not an object")) };
        if prim.get("mode").and_then(Value::as_u64).unwrap_or(4) != 4 {
            continue;
        }
        let Some(attrs) = prim.get("attributes").and_then(Value::as_object) else { continue };
        let Some(pos) = index(attrs, "POSITION", &pp)? else { continue };
        let nv = r.accessor(pos, &pp).map(|a| a.count).unwrap_or(0);
        let ni = match index(prim, "indices", &pp)? {
            Some(i) => r.accessor(i, &pp).map(|a| a.count).unwrap_or(0),
            None => nv,
        };
        t = t.saturating_add(ni / 3);
        v = v.saturating_add(nv);
    }
    Ok((t, v))
}

/// One body from a mesh's primitives placed by `world`.
fn body(prims: &[Prim], materials: &HashMap<usize, Material>, world: &Mat4, flip: bool, name: String) -> RawObject {
    let mut o = RawObject { name, ..Default::default() };
    // Groups, one per material in order of first use; a primitive with no
    // material is `default`.
    let mut group_of: Vec<Option<usize>> = Vec::new();
    for p in prims {
        if !group_of.contains(&p.material) {
            group_of.push(p.material);
        }
    }
    // Per-vertex colour when the file has any, or when the body's
    // materials disagree on colour: each face then shows its own.
    let base_of = |p: &Prim| -> [f64; 3] {
        p.material.and_then(|m| materials.get(&m)).map(|m| [m.base[0], m.base[1], m.base[2]]).unwrap_or([1.0; 3])
    };
    let mut distinct: Vec<[f64; 3]> = Vec::new();
    for p in prims {
        let b = base_of(p);
        if !distinct.contains(&b) {
            distinct.push(b);
        }
    }
    let per_vertex = prims.iter().any(|p| p.colors.is_some()) || distinct.len() > 1;
    // The most-used material: its look is the body's.
    let mut weight: BTreeMap<usize, usize> = BTreeMap::new();
    for p in prims {
        if let Some(m) = p.material {
            *weight.entry(m).or_default() += p.triangles.len();
        }
    }
    let dominant = weight.iter().max_by(|a, b| a.1.cmp(b.1).then(b.0.cmp(a.0))).map(|(m, _)| *m);
    o.appearance = match dominant.and_then(|m| materials.get(&m)) {
        Some(m) => Some(Appearance {
            material: m.name.clone(),
            base_color: if per_vertex { [1.0; 3] } else { [m.base[0], m.base[1], m.base[2]] },
            opacity: if m.blend { m.base[3] } else { 1.0 },
            roughness: m.roughness,
            metallic: m.metallic,
            emissive: m.emissive,
        }),
        // No material at all: Cadpole's own look, unless the colour is per
        // vertex, which a white base must carry unchanged.
        None if per_vertex => Some(Appearance::default()),
        None => None,
    };
    let single = group_of.len() == 1;
    for g in &group_of {
        match g {
            Some(m) => {
                let label = materials.get(m).map(|x| x.name.clone()).unwrap_or_default();
                o.groups.push(label);
                o.group_fallbacks.push(if single { "surface".to_owned() } else { format!("material[{m}]") });
            }
            None => {
                o.groups.push(if single { String::new() } else { "default".to_owned() });
                o.group_fallbacks.push(String::new());
            }
        }
    }
    if single && o.groups[0].is_empty() {
        // One group without a name of its own is the body's `surface`,
        // as every single-group mesh body's is.
        o.groups.clear();
        o.group_fallbacks.clear();
    }
    let total_v: usize = prims.iter().map(|p| p.positions.len()).sum();
    let total_t: usize = prims.iter().map(|p| p.triangles.len()).sum();
    o.positions.reserve(total_v);
    o.triangles.reserve(total_t);
    if !o.groups.is_empty() {
        o.tri_group.reserve(total_t);
    }
    if per_vertex {
        o.colors.reserve(total_v);
    }
    for p in prims {
        let base = o.positions.len() as u32;
        for q in &p.positions {
            o.positions.push(apply(world, [q[0] as f64, q[1] as f64, q[2] as f64]));
        }
        let g = group_of.iter().position(|x| *x == p.material).unwrap_or(0) as u32;
        for t in &p.triangles {
            let t = [t[0] + base, t[1] + base, t[2] + base];
            o.triangles.push(if flip { [t[0], t[2], t[1]] } else { t });
            if !o.groups.is_empty() {
                o.tri_group.push(g);
            }
        }
        if per_vertex {
            let b = base_of(p).map(|c| c as f32);
            match &p.colors {
                Some(cs) => o.colors.extend(cs.iter().map(|c| [srgb8(c[0] * b[0]), srgb8(c[1] * b[1]), srgb8(c[2] * b[2])])),
                None => {
                    let c = [srgb8(b[0]), srgb8(b[1]), srgb8(b[2])];
                    o.colors.extend(std::iter::repeat_n(c, p.positions.len()));
                }
            }
        }
    }
    o
}
