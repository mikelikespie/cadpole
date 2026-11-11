//! Assets: the bytes a document refers to that did not come from a script.
//!
//! An asset is **content-addressed and user-named**: the document's asset
//! table maps a name (`"plan.dxf"`) to a SHA-256, a size and a sniffed
//! [`AssetKind`]; the bytes themselves live either inside the packed document
//! (packed), in a content store, or loose on disk next to `main.star`. One
//! [`AssetStore`] trait is behind all three, so scripts never see a path and
//! the sandbox, wasm and the render host all work the same way.
//!
//! This crate is deliberately dependency-free apart from the hash: it knows
//! nothing about protobuf, Starlark or geometry.

use std::collections::BTreeMap;
use std::sync::Arc;

mod sha256;
pub use sha256::sha256_hex;

/// Largest single asset wherever the bytes *travel* — an upload
/// (`AddAsset`), a packed document, `EvaluateRequest.asset_data`, the
/// browser's engine — in bytes. A
/// native run reading `./assets` from disk holds a mesh to
/// [`NATIVE_MESH_ASSET_BYTES`] instead ([`Limits`]).
pub const MAX_ASSET_BYTES: u64 = 32 * 1024 * 1024;
/// Largest total of all assets in one document where the bytes travel, in
/// bytes.
pub const MAX_DOCUMENT_ASSET_BYTES: u64 = 128 * 1024 * 1024;
/// Largest mesh asset — STL, OBJ, 3MF, glTF, and a `.bin` a `.gltf`
/// names — a native run reads from disk by default. A phone's room
/// scan is the one asset routinely larger than [`MAX_ASSET_BYTES`]: 26 to
/// 34 MB as GLB, 55 to 71 MB as OBJ, 166 MB as a textured GLB, and an OBJ
/// at the importer's five-million-triangle cap is about 300 MB.
pub const NATIVE_MESH_ASSET_BYTES: u64 = 512 * 1024 * 1024;
/// Largest total of all assets a native run reads from disk by default.
pub const NATIVE_DOCUMENT_ASSET_BYTES: u64 = 2 * 1024 * 1024 * 1024;

/// The size caps one host holds assets to.
///
/// Two defaults, because the two places bytes come from differ in what a
/// large file costs. Where bytes **travel** ([`Limits::TRANSFER`]) — an
/// upload to `cadpole serve`, a packed document, the browser's wasm engine —
/// every asset is held to 32 MB and a document to 128 MB, as they always
/// were: that is transfer time and browser memory. A **native** run that
/// reads `./assets` from disk ([`Limits::NATIVE`]: `cadpole eval`, `render`,
/// `serve`, `export`, … on a `.star`) holds a mesh to 512 MB and the rest
/// to the same 32 MB, and a document to 2 GB: the bytes never cross a
/// wire, and the importers carry caps of their own (five million
/// triangles, 64 MB of STEP or DXF text). `cadpole --max-asset-mb N` (or
/// `CADPOLE_MAX_ASSET_MB=N`) replaces the native caps with one of N MB for
/// every kind, and a total of four times that ([`Limits::with_max_asset_mb`]).
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub struct Limits {
    /// Largest single asset that is not a mesh, in bytes.
    pub asset: u64,
    /// Largest single mesh asset ([`AssetKind::is_mesh`], or a `.bin`), in
    /// bytes.
    pub mesh_asset: u64,
    /// Largest total of all assets, in bytes.
    pub document: u64,
}

impl Limits {
    /// Where bytes travel: 32 MB each, 128 MB in all.
    pub const TRANSFER: Limits =
        Limits { asset: MAX_ASSET_BYTES, mesh_asset: MAX_ASSET_BYTES, document: MAX_DOCUMENT_ASSET_BYTES };
    /// A native run reading `./assets` from disk: a mesh to 512 MB,
    /// anything else to 32 MB, 2 GB in all.
    pub const NATIVE: Limits =
        Limits { asset: MAX_ASSET_BYTES, mesh_asset: NATIVE_MESH_ASSET_BYTES, document: NATIVE_DOCUMENT_ASSET_BYTES };

    /// One cap of `mb` megabytes for every kind, and four times that (at
    /// least the transfer total) for the document: what `--max-asset-mb`
    /// and `CADPOLE_MAX_ASSET_MB` set. `0` is read as 1 MB, never "no cap".
    pub fn with_max_asset_mb(mb: u64) -> Limits {
        let one = mb.max(1).saturating_mul(1024 * 1024);
        Limits { asset: one, mesh_asset: one, document: one.saturating_mul(4).max(MAX_DOCUMENT_ASSET_BYTES) }
    }

    /// The native caps, or the override `CADPOLE_MAX_ASSET_MB` names. A value
    /// that is not a whole number of megabytes is ignored, with the reason
    /// in the second element.
    pub fn native_from_env() -> (Limits, Option<String>) {
        match std::env::var("CADPOLE_MAX_ASSET_MB") {
            Ok(v) if !v.trim().is_empty() => match v.trim().parse::<u64>() {
                Ok(mb) => (Limits::with_max_asset_mb(mb), None),
                Err(_) => (
                    Limits::NATIVE,
                    Some(format!("CADPOLE_MAX_ASSET_MB={v:?} is not a whole number of megabytes; the default caps apply")),
                ),
            },
            _ => (Limits::NATIVE, None),
        }
    }

    /// The cap for one asset: the mesh cap for a mesh or a `.bin` (a glTF
    /// buffer, which sniffs as nothing), the other one for the rest.
    pub fn cap_for(&self, kind: AssetKind, name: &str) -> u64 {
        let bin = name.rsplit('.').next().is_some_and(|e| e.eq_ignore_ascii_case("bin")) && name.contains('.');
        if kind.is_mesh() || (kind == AssetKind::Unknown && bin) { self.mesh_asset.max(self.asset) } else { self.asset }
    }

    /// The largest single asset of any kind.
    pub fn largest(&self) -> u64 {
        self.asset.max(self.mesh_asset)
    }
}

/// `n` bytes in whole megabytes, for a diagnostic.
fn mb(n: u64) -> u64 {
    n / (1024 * 1024)
}

/// What an asset's bytes are, sniffed from the bytes themselves. The MIME
/// type a browser or a CLI hands us is advisory and never trusted.
#[derive(Clone, Copy, Debug, PartialEq, Eq, Default)]
pub enum AssetKind {
    #[default]
    Unknown,
    Svg,
    Dxf,
    Stl,
    Obj,
    ThreeMf,
    Step,
    Image,
    Font,
    /// glTF 2.0: binary (GLB, the `glTF` magic) or JSON (`.gltf`).
    Gltf,
}

impl AssetKind {
    /// A mesh `import_mesh` reads: STL, OBJ, 3MF or glTF.
    pub fn is_mesh(self) -> bool {
        matches!(self, AssetKind::Stl | AssetKind::Obj | AssetKind::ThreeMf | AssetKind::Gltf)
    }

    /// The lowercase name used in diagnostics, `asset.kind` and JSON.
    pub fn name(self) -> &'static str {
        match self {
            AssetKind::Unknown => "unknown",
            AssetKind::Svg => "svg",
            AssetKind::Dxf => "dxf",
            AssetKind::Stl => "stl",
            AssetKind::Obj => "obj",
            AssetKind::ThreeMf => "3mf",
            AssetKind::Step => "step",
            AssetKind::Image => "image",
            AssetKind::Font => "font",
            AssetKind::Gltf => "gltf",
        }
    }

    /// A MIME type for the kind, for the table's advisory `mime` field.
    pub fn mime(self) -> &'static str {
        match self {
            AssetKind::Unknown => "application/octet-stream",
            AssetKind::Svg => "image/svg+xml",
            AssetKind::Dxf => "image/vnd.dxf",
            AssetKind::Stl => "model/stl",
            AssetKind::Obj => "model/obj",
            AssetKind::ThreeMf => "model/3mf",
            AssetKind::Step => "model/step",
            AssetKind::Image => "image/png",
            AssetKind::Font => "font/ttf",
            AssetKind::Gltf => "model/gltf-binary",
        }
    }
}

/// One entry of the document's asset table. The bytes are *not* here: they
/// are fetched from the [`AssetStore`] by name, so the same record describes
/// a packed asset, one in a content store and one loose on disk.
#[derive(Clone, Debug, PartialEq, Eq, Default)]
pub struct Asset {
    pub name: String,
    /// Lowercase hex SHA-256 of the bytes.
    pub sha256: String,
    pub mime: String,
    pub size: u64,
    pub kind: AssetKind,
    /// Where it came from when it was added. Informational only.
    pub source_path: String,
}

impl Asset {
    /// The record for `bytes` named `name`: hashes and sniffs in one pass.
    pub fn describe(name: &str, bytes: &[u8]) -> Asset {
        let kind = sniff(name, bytes);
        Asset {
            name: name.to_owned(),
            sha256: sha256_hex(bytes),
            mime: kind.mime().to_owned(),
            size: bytes.len() as u64,
            kind,
            source_path: String::new(),
        }
    }
}

/// Where a script's `asset("name")` gets its bytes.
///
/// Implementations are cheap to clone behind an `Arc` and must be immutable
/// during one evaluation: an evaluation that saw different bytes half way
/// through would not be reproducible.
pub trait AssetStore: Send + Sync {
    /// The record and bytes for `name`, or `None` when the document has no
    /// such asset *or* has the record but not the bytes. Callers distinguish
    /// the two with [`AssetStore::record`].
    fn get(&self, name: &str) -> Option<(Asset, Arc<[u8]>)>;
    /// The table entry for `name`, even when the bytes are missing.
    fn record(&self, name: &str) -> Option<Asset>;
    /// Every asset name in the document, sorted, for "available: a, b, c".
    fn names(&self) -> Vec<String>;
}

/// An empty store: every lookup fails. The default for a document with no
/// assets, so `asset("x")` is a clean diagnostic rather than a special case.
#[derive(Debug, Default, Clone, Copy)]
pub struct NoAssets;

impl AssetStore for NoAssets {
    fn get(&self, _name: &str) -> Option<(Asset, Arc<[u8]>)> {
        None
    }
    fn record(&self, _name: &str) -> Option<Asset> {
        None
    }
    fn names(&self) -> Vec<String> {
        Vec::new()
    }
}

/// Assets held in memory, keyed by name, with the bytes keyed by hash so two
/// names for identical bytes share one blob. This is what the CLI builds from
/// `./assets/`, what the engine builds from a packed `Document` plus an
/// `EvaluateRequest`'s `asset_data`, and what tests use.
#[derive(Debug, Default, Clone)]
pub struct MemoryAssetStore {
    by_name: BTreeMap<String, Asset>,
    by_hash: BTreeMap<String, Arc<[u8]>>,
}

impl MemoryAssetStore {
    pub fn new() -> MemoryAssetStore {
        MemoryAssetStore::default()
    }

    /// Record `name` -> `bytes`, hashing and sniffing them. Returns the
    /// record. A name that is already present is replaced (the drag-and-drop
    /// "replace" case).
    pub fn insert(&mut self, name: &str, bytes: impl Into<Arc<[u8]>>) -> Asset {
        let bytes: Arc<[u8]> = bytes.into();
        let asset = Asset::describe(name, &bytes);
        self.by_hash.insert(asset.sha256.clone(), bytes);
        self.by_name.insert(name.to_owned(), asset.clone());
        asset
    }

    /// Record a table entry whose bytes may arrive separately
    /// ([`MemoryAssetStore::insert_blob`]). Used when a document carries
    /// names and hashes but the bytes travel in the request.
    pub fn insert_record(&mut self, asset: Asset) {
        self.by_name.insert(asset.name.clone(), asset);
    }

    /// Store bytes under their own hash, for records added with
    /// [`MemoryAssetStore::insert_record`]. The hash is recomputed, never
    /// taken on trust, so a lying request cannot swap an asset's content.
    pub fn insert_blob(&mut self, bytes: impl Into<Arc<[u8]>>) -> String {
        let bytes: Arc<[u8]> = bytes.into();
        let hash = sha256_hex(&bytes);
        self.by_hash.insert(hash.clone(), bytes);
        hash
    }

    pub fn is_empty(&self) -> bool {
        self.by_name.is_empty()
    }

    pub fn len(&self) -> usize {
        self.by_name.len()
    }

    /// Total size of the recorded assets, for the document cap.
    pub fn total_size(&self) -> u64 {
        self.by_name.values().map(|a| a.size).sum()
    }
}

impl AssetStore for MemoryAssetStore {
    fn get(&self, name: &str) -> Option<(Asset, Arc<[u8]>)> {
        let a = self.by_name.get(name)?;
        let b = self.by_hash.get(&a.sha256)?;
        Some((a.clone(), b.clone()))
    }
    fn record(&self, name: &str) -> Option<Asset> {
        self.by_name.get(name).cloned()
    }
    fn names(&self) -> Vec<String> {
        self.by_name.keys().cloned().collect()
    }
}

/// Sniff what the bytes are. `name` is only a tie-breaker for text formats
/// that have no magic of their own; the bytes win wherever they can decide.
pub fn sniff(name: &str, bytes: &[u8]) -> AssetKind {
    sniff_head(name, bytes, bytes.len() as u64)
}

/// [`sniff`] from the start of a file and its whole length, for a caller
/// that has not read the rest (the loader deciding whether a large file is
/// under its kind's cap). `head` should be at least the first 1 KB; the
/// length is what a binary STL is recognised by.
pub fn sniff_head(name: &str, bytes: &[u8], len: u64) -> AssetKind {
    // Strip a UTF-8 BOM before looking at text.
    let b = bytes.strip_prefix(&[0xEF, 0xBB, 0xBF]).unwrap_or(bytes);
    let head = &b[..b.len().min(1024)];

    // Binary glTF: the `glTF` magic and version 2.
    if head.len() >= 8 && head.starts_with(b"glTF") && head[4..8] == 2u32.to_le_bytes() {
        return AssetKind::Gltf;
    }

    if head.starts_with(b"\x89PNG\r\n\x1a\n") || head.starts_with(&[0xFF, 0xD8, 0xFF]) {
        return AssetKind::Image;
    }
    if head.starts_with(&[0x00, 0x01, 0x00, 0x00])
        || head.starts_with(b"OTTO")
        || head.starts_with(b"true")
        || head.starts_with(b"ttcf")
    {
        return AssetKind::Font;
    }
    if head.starts_with(b"PK\x03\x04") {
        // Only 3MF is a zip we accept; the reader validates the parts.
        return AssetKind::ThreeMf;
    }
    if head.starts_with(b"AutoCAD Binary DXF") {
        return AssetKind::Dxf;
    }
    if head.starts_with(b"ISO-10303-21;") {
        return AssetKind::Step;
    }

    let text = String::from_utf8_lossy(head);
    let trimmed = text.trim_start();
    // A glTF JSON: an object whose first key is one glTF's top level has.
    // Exporters write `asset` first; the rest are there for the ones that
    // do not.
    if trimmed.starts_with('{') {
        let first_key = trimmed[1..].trim_start();
        const GLTF_KEYS: [&str; 8] =
            ["\"asset\"", "\"accessors\"", "\"bufferViews\"", "\"buffers\"", "\"meshes\"", "\"nodes\"", "\"scene\"", "\"extensionsUsed\""];
        if GLTF_KEYS.iter().any(|k| first_key.starts_with(k)) || text.contains("\"asset\"") && text.contains("\"version\"") {
            return AssetKind::Gltf;
        }
    }
    if trimmed.starts_with("<?xml") || trimmed.starts_with("<svg") || trimmed.starts_with("<!--") {
        if text.contains("<svg") {
            return AssetKind::Svg;
        }
    }
    // An ASCII DXF starts with a group code 0 / SECTION pair, possibly after
    // a comment (code 999).
    if dxf_header(&text) {
        return AssetKind::Dxf;
    }
    if trimmed.starts_with("solid ") {
        return AssetKind::Stl;
    }
    if trimmed.lines().any(|l| l.starts_with("v ") || l.starts_with("f ")) {
        return AssetKind::Obj;
    }
    // Binary STL: an 80-byte header then a u32 triangle count.
    if len > 84 && bytes.len() >= 84 {
        let n = u32::from_le_bytes([bytes[80], bytes[81], bytes[82], bytes[83]]) as u64;
        if len == 84 + 50 * n {
            return AssetKind::Stl;
        }
    }
    // Last resort: the extension the user gave the file.
    match name.rsplit('.').next().unwrap_or("").to_ascii_lowercase().as_str() {
        "svg" => AssetKind::Svg,
        "dxf" => AssetKind::Dxf,
        "stl" => AssetKind::Stl,
        "obj" => AssetKind::Obj,
        "3mf" => AssetKind::ThreeMf,
        "step" | "stp" => AssetKind::Step,
        "png" | "jpg" | "jpeg" => AssetKind::Image,
        "ttf" | "otf" | "ttc" => AssetKind::Font,
        "gltf" | "glb" => AssetKind::Gltf,
        _ => AssetKind::Unknown,
    }
}

/// True when the text looks like an ASCII DXF: a `0` group code followed by
/// `SECTION`, ignoring leading `999` comment pairs and whitespace.
fn dxf_header(text: &str) -> bool {
    let mut lines = text.lines().map(str::trim);
    while let Some(code) = lines.next() {
        match code {
            "999" => {
                lines.next();
            }
            "0" => return lines.next() == Some("SECTION"),
            "" => continue,
            _ => return false,
        }
    }
    false
}

/// Read every regular file in `dir` as an asset named by its file name,
/// under the native caps ([`Limits::NATIVE`]): what every caller of this —
/// a process reading a directory off its own disk — is.
///
/// This is the loose-mode half of the model: `cadpole eval main.star` looks next
/// to the script for `./assets/`, hashes what it finds and packs it into the
/// document. Reproducibility comes from the file system (git), exactly as it
/// does for the script. A file over its kind's cap is skipped and named in
/// the returned warnings; a missing directory is not an error.
pub fn load_dir(dir: &std::path::Path) -> (MemoryAssetStore, Vec<String>) {
    load_dir_with(dir, &Limits::NATIVE)
}

/// [`load_dir`] under the caps `limits` sets. A file is measured
/// before it is read, and a file over the non-mesh cap is sniffed from its
/// first 4 KB before the rest is read, so a large file that is not a mesh
/// costs one small read. Assets are taken in name order until the
/// document's total would pass `limits.document`; the rest are skipped and
/// named.
pub fn load_dir_with(dir: &std::path::Path, limits: &Limits) -> (MemoryAssetStore, Vec<String>) {
    let mut store = MemoryAssetStore::new();
    let mut warnings = Vec::new();
    let Ok(entries) = std::fs::read_dir(dir) else {
        return (store, warnings);
    };
    let mut paths: Vec<std::path::PathBuf> = entries.flatten().map(|e| e.path()).collect();
    paths.sort();
    let how = "cadpole --max-asset-mb N, or CADPOLE_MAX_ASSET_MB=N, sets one cap for every kind";
    for p in paths {
        if !p.is_file() {
            continue;
        }
        let Some(name) = p.file_name().and_then(|n| n.to_str()) else { continue };
        if name.starts_with('.') || name == "BUILD.bazel" || name == "BUILD" {
            continue;
        }
        let len = match std::fs::metadata(&p) {
            Ok(m) => m.len(),
            Err(e) => {
                warnings.push(format!("asset {name:?} could not be read: {e}"));
                continue;
            }
        };
        let over = |kind: AssetKind| -> Option<String> {
            let cap = limits.cap_for(kind, name);
            (len > cap).then(|| {
                let what = if kind.is_mesh() || cap == limits.mesh_asset && limits.mesh_asset > limits.asset {
                    "a mesh asset".to_owned()
                } else if kind == AssetKind::Unknown {
                    "an asset".to_owned()
                } else {
                    format!("a {} asset", kind.name())
                };
                format!("asset {name:?} is {len} bytes, over the {} MB limit for {what}; not loaded ({how})", mb(cap))
            })
        };
        if len > limits.largest() {
            // Over every cap, but the message names the cap of its kind, so
            // look at its first bytes: a mesh is over the mesh cap, not the
            // smaller one for everything else.
            let kind = sniff_head(name, &read_head(&p, 4096), len);
            warnings.push(over(kind).unwrap_or_else(|| {
                format!("asset {name:?} is {len} bytes, over the {} MB limit; not loaded ({how})", mb(limits.largest()))
            }));
            continue;
        }
        if len > limits.asset {
            // Only a mesh may be this large: look at its first bytes before
            // reading the rest.
            let head = read_head(&p, 4096);
            if let Some(w) = over(sniff_head(name, &head, len)) {
                warnings.push(w);
                continue;
            }
        }
        if store.total_size() + len > limits.document {
            warnings.push(format!(
                "asset {name:?} ({len} bytes) would take this document's assets past the {} MB total; not loaded ({how})",
                mb(limits.document)
            ));
            continue;
        }
        match std::fs::read(&p) {
            Ok(bytes) => {
                if let Some(w) = over(sniff(name, &bytes)) {
                    // The file grew, or its first bytes said something the
                    // rest did not: the kind is the whole file's.
                    warnings.push(w);
                    continue;
                }
                let mut a = store.insert(name, bytes);
                a.source_path = p.display().to_string();
                store.insert_record(a);
            }
            Err(e) => warnings.push(format!("asset {name:?} could not be read: {e}")),
        }
    }
    (store, warnings)
}

/// Up to `n` bytes from the start of the file at `p`; fewer, or none, if it
/// cannot be read.
fn read_head(p: &std::path::Path, n: usize) -> Vec<u8> {
    use std::io::Read;
    let mut head = Vec::with_capacity(n);
    if let Ok(f) = std::fs::File::open(p) {
        let _ = f.take(n as u64).read_to_end(&mut head);
    }
    head
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn hashing_matches_the_known_vectors() {
        assert_eq!(
            sha256_hex(b""),
            "e3b0c44298fc1c149afbf4c8996fb92427ae41e4649b934ca495991b7852b855"
        );
        assert_eq!(
            sha256_hex(b"abc"),
            "ba7816bf8f01cfea414140de5dae2223b00361a396177a9cb410ff61f20015ad"
        );
        // Longer than one block, to exercise the padding path.
        let long = b"abcdbcdecdefdefgefghfghighijhijkijkljklmklmnlmnomnopnopq";
        assert_eq!(
            sha256_hex(long),
            "248d6a61d20638b8e5c026930c3e6039a33ce45964ff2167f6ecedd419db06c1"
        );
        let million = vec![b'a'; 1_000_000];
        assert_eq!(
            sha256_hex(&million),
            "cdc76e5c9914fb9281a1c7e284d73e67f1809a48a497200e046d39ccc7112cd0"
        );
    }

    #[test]
    fn sniffing_reads_the_bytes_not_the_name() {
        assert_eq!(sniff("mystery.bin", b"<?xml version=\"1.0\"?><svg></svg>"), AssetKind::Svg);
        assert_eq!(sniff("mystery.bin", b"0\nSECTION\n2\nHEADER\n"), AssetKind::Dxf);
        assert_eq!(sniff("plan.svg", b"999\nmade by cadpole\n0\nSECTION\n"), AssetKind::Dxf);
        assert_eq!(sniff("f", b"\x00\x01\x00\x00garbage"), AssetKind::Font);
        assert_eq!(sniff("f", b"OTTOgarbage"), AssetKind::Font);
        assert_eq!(sniff("x.png", b"\x89PNG\r\n\x1a\n"), AssetKind::Image);
        assert_eq!(sniff("x.stl", b"solid cube\nfacet normal 0 0 1\n"), AssetKind::Stl);
        // Nothing decisive in the bytes: the extension is the last resort.
        assert_eq!(sniff("x.step", b"\n\n\n"), AssetKind::Step);
        assert_eq!(sniff("x.unknown", b"\n\n\n"), AssetKind::Unknown);
    }

    #[test]
    fn a_store_shares_one_blob_between_two_names() {
        let mut s = MemoryAssetStore::new();
        let a = s.insert("logo.svg", b"<svg/>".to_vec());
        let b = s.insert("copy.svg", b"<svg/>".to_vec());
        assert_eq!(a.sha256, b.sha256);
        assert_eq!(s.by_hash.len(), 1, "identical bytes share one blob");
        assert_eq!(s.names(), vec!["copy.svg".to_owned(), "logo.svg".to_owned()]);
        assert_eq!(s.get("logo.svg").unwrap().1.as_ref(), b"<svg/>");
        assert!(s.get("nope.svg").is_none());
        assert_eq!(s.total_size(), 12);
    }

    /// A directory of its own under the test's temporary directory.
    fn scratch(name: &str) -> std::path::PathBuf {
        let base = std::env::var("TEST_TMPDIR").map(std::path::PathBuf::from).unwrap_or_else(|_| std::env::temp_dir());
        let d = base.join(format!("cadpole-assets-{name}-{}", std::process::id()));
        let _ = std::fs::remove_dir_all(&d);
        std::fs::create_dir_all(&d).unwrap();
        d
    }

    /// A file of `n` bytes that sniffs as `head` says.
    fn write_sized(p: &std::path::Path, head: &[u8], n: usize) {
        let mut b = head.to_vec();
        b.resize(n, 0);
        std::fs::write(p, b).unwrap();
    }

    const GLB_HEAD: &[u8] = b"glTF\x02\x00\x00\x00";

    #[test]
    fn a_mesh_over_32_mb_is_read_natively_and_refused_where_bytes_travel() {
        // A phone's room scan is routinely past 32 MB.
        let d = scratch("caps");
        let big = 40 * 1024 * 1024;
        write_sized(&d.join("scan.glb"), GLB_HEAD, big);
        write_sized(&d.join("scan.bin"), b"\x00\x01\x02", big);
        write_sized(&d.join("poster.svg"), b"<svg xmlns='http://www.w3.org/2000/svg'>", big);
        std::fs::write(d.join("small.stl"), b"solid s\nendsolid s\n").unwrap();

        // Where bytes travel: every one of the large ones refused, named.
        let (store, warnings) = load_dir_with(&d, &Limits::TRANSFER);
        assert_eq!(store.names(), vec!["small.stl".to_owned()], "{warnings:?}");
        assert_eq!(warnings.len(), 3, "{warnings:?}");
        assert!(warnings.iter().all(|w| w.contains("over the 32 MB limit") && w.contains("--max-asset-mb")), "{warnings:?}");

        // A native run: the mesh and its buffer are read, the SVG is not.
        let (store, warnings) = load_dir(&d);
        assert_eq!(store.names(), vec!["scan.bin".to_owned(), "scan.glb".to_owned(), "small.stl".to_owned()], "{warnings:?}");
        assert_eq!(store.record("scan.glb").unwrap().kind, AssetKind::Gltf);
        assert_eq!(store.get("scan.glb").unwrap().1.len(), big);
        assert_eq!(warnings.len(), 1, "{warnings:?}");
        assert!(warnings[0].contains("poster.svg") && warnings[0].contains("32 MB limit for a svg asset"), "{warnings:?}");

        // `--max-asset-mb 32` holds a native run to a 32 MB cap...
        let (store, warnings) = load_dir_with(&d, &Limits::with_max_asset_mb(32));
        assert_eq!(store.names(), vec!["small.stl".to_owned()], "{warnings:?}");
        // ...and 64 admits every kind.
        let (store, warnings) = load_dir_with(&d, &Limits::with_max_asset_mb(64));
        assert_eq!(store.names().len(), 4, "{warnings:?}");
        assert!(warnings.is_empty(), "{warnings:?}");

        // The document total binds too: two 40 MB files under a 100 MB total.
        let (store, warnings) = load_dir_with(&d, &Limits { asset: 64 << 20, mesh_asset: 64 << 20, document: 100 << 20 });
        assert_eq!(store.names().len(), 3, "{warnings:?}");
        assert!(warnings.iter().any(|w| w.contains("past the 100 MB total")), "{warnings:?}");
        let _ = std::fs::remove_dir_all(&d);
    }

    /// A mesh over even the mesh cap is refused naming the mesh cap -- the
    /// one it is over -- not the smaller cap for everything else, which a
    /// mesh between the two would then be seen to pass.
    #[test]
    fn a_mesh_over_the_mesh_cap_is_refused_naming_that_cap() {
        let d = scratch("overcap");
        let limits = Limits { asset: 1 << 20, mesh_asset: 4 << 20, document: 100 << 20 };
        write_sized(&d.join("scan.glb"), GLB_HEAD, 6 << 20);
        write_sized(&d.join("mid.glb"), GLB_HEAD, 3 << 20);
        write_sized(&d.join("notes.txt"), b"plain words\n", 6 << 20);
        let (store, warnings) = load_dir_with(&d, &limits);
        assert_eq!(store.names(), vec!["mid.glb".to_owned()], "{warnings:?}");
        let scan = warnings.iter().find(|w| w.contains("scan.glb")).expect("scan.glb refused");
        assert!(scan.contains("over the 4 MB limit for a mesh asset"), "{scan}");
        let notes = warnings.iter().find(|w| w.contains("notes.txt")).expect("notes.txt refused");
        assert!(notes.contains("over the 1 MB limit"), "{notes}");
        let _ = std::fs::remove_dir_all(&d);
    }

    #[test]
    fn the_caps_read_their_setting() {
        assert_eq!(Limits::TRANSFER.largest(), MAX_ASSET_BYTES);
        assert_eq!(Limits::NATIVE.cap_for(AssetKind::Obj, "scan.obj"), NATIVE_MESH_ASSET_BYTES);
        assert_eq!(Limits::NATIVE.cap_for(AssetKind::Unknown, "scan.bin"), NATIVE_MESH_ASSET_BYTES);
        assert_eq!(Limits::NATIVE.cap_for(AssetKind::Unknown, "notes"), MAX_ASSET_BYTES);
        assert_eq!(Limits::NATIVE.cap_for(AssetKind::Step, "part.step"), MAX_ASSET_BYTES);
        let l = Limits::with_max_asset_mb(300);
        assert_eq!((l.asset, l.mesh_asset, l.document), (300 << 20, 300 << 20, 1200 << 20));
        assert_eq!(Limits::with_max_asset_mb(0).asset, 1 << 20, "0 is not \"no cap\"");
        assert_eq!(Limits::with_max_asset_mb(8).document, MAX_DOCUMENT_ASSET_BYTES, "never below the transfer total");
        // The sniff needs only the head and the length.
        assert_eq!(sniff_head("x", GLB_HEAD, 1 << 30), AssetKind::Gltf);
        let mut stl = vec![0u8; 84];
        stl[80..84].copy_from_slice(&2u32.to_le_bytes());
        assert_eq!(sniff_head("x", &stl, 84 + 100), AssetKind::Stl);
        assert_eq!(sniff("scan.gltf", b"{\n  \"asset\": {\"version\": \"2.0\"}}"), AssetKind::Gltf);
        assert_eq!(sniff("x.json", b"{\"accessors\": []}"), AssetKind::Gltf);
        assert_eq!(sniff("x.json", b"{\"name\": \"not a model\"}"), AssetKind::Unknown);
    }

    #[test]
    fn a_record_without_its_bytes_is_visible_but_not_readable() {
        let mut s = MemoryAssetStore::new();
        s.insert_record(Asset {
            name: "plan.dxf".into(),
            sha256: "ab12".into(),
            size: 10,
            kind: AssetKind::Dxf,
            ..Default::default()
        });
        assert!(s.record("plan.dxf").is_some());
        assert!(s.get("plan.dxf").is_none(), "the bytes are not loaded");
        // Supplying the bytes out of band makes it readable, under their real hash.
        let mut s2 = MemoryAssetStore::new();
        let bytes = b"0\nSECTION\n".to_vec();
        let hash = sha256_hex(&bytes);
        s2.insert_record(Asset { name: "plan.dxf".into(), sha256: hash.clone(), ..Default::default() });
        assert_eq!(s2.insert_blob(bytes), hash);
        assert!(s2.get("plan.dxf").is_some());
    }
}
