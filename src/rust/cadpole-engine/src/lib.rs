//! The Cadpole engine: one `call(request_bytes) -> response_bytes` over the
//! `cadpole.v1.Request` / `cadpole.v1.Response` envelope, plus the JSON "session
//! view" used by the web UI. This is the only crate that knows about the
//! envelope; cadpole-core never sees Starlark and cadpole-script never sees protobuf
//! requests.

pub mod animate;
pub mod edit;
pub mod explain;
pub mod gltf;
pub mod inspect;
pub mod limits;
pub mod modeltest;
pub mod print;
pub mod probe;
pub mod selection;
pub mod step;
pub mod sweep;
pub mod view;

use std::collections::HashMap;
use std::sync::Arc;

use cadpole_assets::{AssetStore, MemoryAssetStore};
use cadpole_core::layers::LayerOverride;
use cadpole_core::context::{Diagnostic, EntityRef};
use cadpole_core::memo::Memo;
use cadpole_core::context::RenderDefaults;
use cadpole_core::render::section::{self, Section};
use cadpole_core::render::{
    Backdrop, Backend, Camera, DebugView, EdgeMode, FaceFrame, Floor, GridMode, Image, Item, Lighting, RenderOptions,
    RenderStats, SunLight, ToneMap,
};
use cadpole_core::sun::SunSpec;
use cadpole_core::{Context, ParamValue, Vec3};
use cadpole_proto::prost::Message;
use cadpole_proto::v1 as pb;
use cadpole_script::{EvalInput, ScriptFile};

pub use cadpole_assets;
pub use cadpole_core;
pub use cadpole_drawing;
pub use cadpole_export_3mf;
pub use cadpole_export_step;
pub use cadpole_print;
pub use cadpole_proto;
pub use cadpole_script;
pub use animate::animate;
pub use explain::explain_ref;
pub use inspect::inspect;
pub use limits::{Limits, RenderLimits};
pub use modeltest::run_tests;
pub use print::print_check;
pub use step::export_step;
pub use selection::resolve_selection;
pub use sweep::sweep;

pub const VERSION: &str = env!("CARGO_PKG_VERSION");

/// Opacity every body is drawn with in the `xray` view mode (docs/views.md).
pub const XRAY_OPACITY: f64 = 0.3;

/// Build a document from loose script files (the CLI's `main.star` case).
pub fn document_from_scripts(name: &str, scripts: Vec<(String, String)>) -> pb::Document {
    pb::Document {
        id: String::new(),
        name: name.to_owned(),
        schema_version: 1,
        display_unit: pb::LengthUnit::Mm as i32,
        scripts: scripts.into_iter().map(|(path, source)| pb::ScriptFile { path, source }).collect(),
        ..Default::default()
    }
}

pub fn overrides_from_proto(m: &HashMap<String, pb::ParamValue>) -> HashMap<String, ParamValue> {
    m.iter().filter_map(|(k, v)| ParamValue::from_proto(v).map(|v| (k.clone(), v))).collect()
}

pub fn selection_from_proto(s: &Option<pb::Selection>) -> Vec<EntityRef> {
    s.as_ref()
        .map(|s| {
            s.selected
                .iter()
                .map(|r| EntityRef {
                    feature_id: r.feature_id.clone(),
                    body_id: r.body_id.clone(),
                    kind: kind_name(r.kind()).to_owned(),
                    topo_name: r.topo_name.clone(),
                })
                .collect()
        })
        .unwrap_or_default()
}

pub fn kind_name(k: pb::EntityKind) -> &'static str {
    match k {
        pb::EntityKind::Unspecified => "",
        pb::EntityKind::Feature => "feature",
        pb::EntityKind::Body => "body",
        pb::EntityKind::Face => "face",
        pb::EntityKind::Edge => "edge",
        pb::EntityKind::Vertex => "vertex",
        pb::EntityKind::Sketch => "sketch",
        pb::EntityKind::SketchEntity => "sketch_entity",
        pb::EntityKind::Param => "param",
        pb::EntityKind::Asset => "asset",
    }
}

pub fn kind_from_name(s: &str) -> pb::EntityKind {
    match s {
        "feature" => pb::EntityKind::Feature,
        "body" => pb::EntityKind::Body,
        "face" => pb::EntityKind::Face,
        "edge" => pb::EntityKind::Edge,
        "vertex" => pb::EntityKind::Vertex,
        "sketch" => pb::EntityKind::Sketch,
        "sketch_entity" => pb::EntityKind::SketchEntity,
        "param" => pb::EntityKind::Param,
        "asset" => pb::EntityKind::Asset,
        _ => pb::EntityKind::Unspecified,
    }
}

/// The asset table of a document, as the store `asset("name")` reads.
///
/// A packed document carries the bytes in `Asset.data`; transport strips them
/// and sends them once in `EvaluateRequest.asset_data`, keyed by hash. Both
/// are merged here, and every hash is *recomputed* from the bytes rather than
/// taken on trust, so a request cannot claim one asset's name for another
/// asset's content.
pub fn assets_from_proto(doc: &pb::Document, extra: &HashMap<String, Vec<u8>>) -> MemoryAssetStore {
    let mut store = MemoryAssetStore::new();
    for bytes in extra.values() {
        store.insert_blob(bytes.clone());
    }
    for a in &doc.assets {
        // Hashed once: `insert_blob` recomputes the hash and hands it back,
        // and hashing the bytes a second time for the record doubled what
        // every evaluation of a document with a scan in it paid (
        // 123 ms a pass for 27 MB).
        let hash = if a.data.is_empty() { a.sha256.clone() } else { store.insert_blob(a.data.clone()) };
        store.insert_record(cadpole_assets::Asset {
            name: a.name.clone(),
            sha256: hash,
            mime: a.mime.clone(),
            size: if a.data.is_empty() { a.size } else { a.data.len() as u64 },
            kind: asset_kind_from_proto(a.kind()),
            source_path: a.source_path.clone(),
        });
    }
    store
}

/// Check a name a caller wants to give an asset.
///
/// Asset names are *names*, never paths: scripts say `asset("plan.dxf")` and
/// the table resolves it. Rejecting separators here is what keeps that true
/// even if a later store decides to write the bytes out under their name
/// (docs/security.md, "Filesystem access").
pub fn check_asset_name(name: &str) -> anyhow::Result<()> {
    if name.is_empty() {
        anyhow::bail!("an asset needs a name");
    }
    if name.len() > 255 {
        anyhow::bail!("asset name is {} bytes; keep it under 255", name.len());
    }
    if name.contains('/') || name.contains('\\') {
        anyhow::bail!("asset names are names, not paths: {name:?} may not contain a separator");
    }
    if name == "." || name == ".." || name.contains("..") {
        anyhow::bail!("asset name {name:?} may not contain \"..\"");
    }
    if name.starts_with('.') {
        anyhow::bail!("asset name {name:?} may not start with a dot");
    }
    if name.chars().any(|c| c.is_control()) {
        anyhow::bail!("asset name {name:?} contains a control character");
    }
    Ok(())
}

/// The document's asset table, without the bytes.
pub fn list_assets(doc: &pb::Document) -> pb::ListAssetsResponse {
    let mut assets: Vec<pb::Asset> = doc.assets.iter().map(|a| pb::Asset { data: Vec::new(), ..a.clone() }).collect();
    assets.sort_by(|a, b| a.name.cmp(&b.name));
    pb::ListAssetsResponse {
        total_bytes: assets.iter().map(|a| a.size).sum(),
        assets,
        max_asset_bytes: cadpole_assets::MAX_ASSET_BYTES,
        max_document_bytes: cadpole_assets::MAX_DOCUMENT_ASSET_BYTES,
    }
}

/// One asset's record and, unless `metadata_only`, its bytes.
pub fn get_asset(doc: &pb::Document, req: &pb::GetAssetRequest) -> anyhow::Result<pb::GetAssetResponse> {
    check_asset_name(&req.name)?;
    let a = doc.assets.iter().find(|a| a.name == req.name).ok_or_else(|| {
        let have: Vec<&str> = doc.assets.iter().map(|a| a.name.as_str()).collect();
        anyhow::anyhow!(
            "no asset named {:?}; this document has {}",
            req.name,
            if have.is_empty() { "none".to_owned() } else { have.join(", ") }
        )
    })?;
    let record = pb::Asset { data: Vec::new(), ..a.clone() };
    if req.metadata_only {
        return Ok(pb::GetAssetResponse { asset: Some(record), data: Vec::new(), bytes_missing: a.data.is_empty() });
    }
    Ok(pb::GetAssetResponse { bytes_missing: a.data.is_empty(), data: a.data.clone(), asset: Some(record) })
}

/// Add or replace one asset in `doc`, hashing and sniffing the bytes. The
/// caller's `mime` is never trusted; the returned record is what is stored.
/// Held to the transfer caps: 32 MB an asset, 128 MB a document.
pub fn add_asset(doc: &mut pb::Document, name: &str, data: Vec<u8>, source_path: &str) -> anyhow::Result<pb::Asset> {
    add_asset_limited(doc, name, data, source_path, &cadpole_assets::Limits::TRANSFER)
}

/// [`add_asset`] under a host's own caps: `cadpole serve` holds an
/// upload to the transfer cap, 32 MB, but the document it adds to to the
/// native total, since the session may have read a scan of hundreds of
/// megabytes from `./assets` itself.
pub fn add_asset_limited(
    doc: &mut pb::Document,
    name: &str,
    data: Vec<u8>,
    source_path: &str,
    limits: &cadpole_assets::Limits,
) -> anyhow::Result<pb::Asset> {
    check_asset_name(name)?;
    if data.is_empty() {
        anyhow::bail!("asset {name:?} has no bytes");
    }
    let cap = limits.cap_for(cadpole_assets::sniff(name, &data), name);
    if data.len() as u64 > cap {
        anyhow::bail!("asset {name:?} is {} bytes, over the {} MB limit", data.len(), cap / (1024 * 1024));
    }
    let total: u64 = doc.assets.iter().filter(|a| a.name != name).map(|a| a.size).sum();
    if total + data.len() as u64 > limits.document {
        anyhow::bail!(
            "this document's assets would total {} bytes, over the {} MB limit",
            total + data.len() as u64,
            limits.document / (1024 * 1024)
        );
    }
    let mut described = cadpole_assets::Asset::describe(name, &data);
    described.source_path = source_path.to_owned();
    let record = asset_to_proto(&described, data);
    match doc.assets.iter_mut().find(|a| a.name == name) {
        Some(slot) => *slot = record.clone(),
        None => doc.assets.push(record.clone()),
    }
    Ok(pb::Asset { data: Vec::new(), ..record })
}

/// A `cadpole.v1.Asset` for bytes the host is packing into a document.
pub fn asset_to_proto(a: &cadpole_assets::Asset, data: Vec<u8>) -> pb::Asset {
    pb::Asset {
        name: a.name.clone(),
        sha256: a.sha256.clone(),
        mime: a.mime.clone(),
        size: a.size,
        kind: asset_kind_to_proto(a.kind) as i32,
        data,
        source_path: a.source_path.clone(),
        added_at: None,
    }
}

pub fn asset_kind_to_proto(k: cadpole_assets::AssetKind) -> pb::AssetKind {
    use cadpole_assets::AssetKind as K;
    match k {
        K::Unknown => pb::AssetKind::Unspecified,
        K::Svg => pb::AssetKind::Svg,
        K::Dxf => pb::AssetKind::Dxf,
        K::Stl => pb::AssetKind::Stl,
        K::Obj => pb::AssetKind::Obj,
        K::ThreeMf => pb::AssetKind::Threemf,
        K::Step => pb::AssetKind::Step,
        K::Image => pb::AssetKind::Image,
        K::Font => pb::AssetKind::Font,
        K::Gltf => pb::AssetKind::Gltf,
    }
}

pub fn asset_kind_from_proto(k: pb::AssetKind) -> cadpole_assets::AssetKind {
    use cadpole_assets::AssetKind as K;
    match k {
        pb::AssetKind::Unspecified => K::Unknown,
        pb::AssetKind::Svg => K::Svg,
        pb::AssetKind::Dxf => K::Dxf,
        pb::AssetKind::Stl => K::Stl,
        pb::AssetKind::Obj => K::Obj,
        pb::AssetKind::Threemf => K::ThreeMf,
        pb::AssetKind::Step => K::Step,
        pb::AssetKind::Image => K::Image,
        pb::AssetKind::Font => K::Font,
        pb::AssetKind::Gltf => K::Gltf,
    }
}

/// Evaluate a document with extra overrides layered on top of its own.
pub fn evaluate_document(
    doc: &pb::Document,
    extra_overrides: &HashMap<String, ParamValue>,
    tolerance: f64,
) -> (Context, f64) {
    evaluate_document_with(doc, extra_overrides, tolerance, 0.0)
}

/// As [`evaluate_document`], with an explicit boolean tolerance (0 = default).
pub fn evaluate_document_with(
    doc: &pb::Document,
    extra_overrides: &HashMap<String, ParamValue>,
    tolerance: f64,
    boolean_tolerance: f64,
) -> (Context, f64) {
    evaluate_document_full(doc, extra_overrides, tolerance, boolean_tolerance, &HashMap::new())
}

/// As [`evaluate_document_with`], plus asset bytes that travelled separately
/// from the document (a session request, a render host, wasm).
pub fn evaluate_document_full(
    doc: &pb::Document,
    extra_overrides: &HashMap<String, ParamValue>,
    tolerance: f64,
    boolean_tolerance: f64,
    asset_data: &HashMap<String, Vec<u8>>,
) -> (Context, f64) {
    let (ctx, _, ms) = evaluate_document_limited(
        doc,
        extra_overrides,
        tolerance,
        boolean_tolerance,
        asset_data,
        limits::default_limits(),
    );
    (ctx, ms)
}

/// As [`evaluate_document_full`], with explicit sandbox limits, and also
/// returning the files the script's `export_svg` / `export_dxf` calls
/// produced.
///
/// Every other entry point routes through here. Most pass
/// [`limits::default_limits`], which is what `cadpole serve` sets once at startup
/// so that a render or an export is bounded too — neither `RenderRequest` nor
/// `ExportRequest` carries a deadline of its own — and most discard the files,
/// because only an export asks for them.
pub fn evaluate_document_limited(
    doc: &pb::Document,
    extra_overrides: &HashMap<String, ParamValue>,
    tolerance: f64,
    boolean_tolerance: f64,
    asset_data: &HashMap<String, Vec<u8>>,
    limits: Limits,
) -> (Context, Vec<cadpole_script::OutputFile>, f64) {
    evaluate_document_options(doc, extra_overrides, tolerance, boolean_tolerance, asset_data, limits, EvalOptions::default())
}

/// What a session adds to an evaluation: the kernel-op memo it
/// keeps across evaluations, and whether this one is a drag preview. The
/// default is neither, which is what every one-shot caller wants.
#[derive(Clone, Default)]
pub struct EvalOptions {
    pub memo: Option<Arc<Memo>>,
    pub preview: bool,
    /// Whether a solve this evaluation cannot finish may be handed to a job
    /// that runs past its deadline: `Limits::solve_jobs`. A host
    /// with a job runner sets it for the evaluations its owner asks for, and
    /// for nothing else.
    pub solve_jobs: bool,
}

/// [`evaluate_document_limited`] with a session's [`EvalOptions`].
#[allow(clippy::too_many_arguments)]
pub fn evaluate_document_options(
    doc: &pb::Document,
    extra_overrides: &HashMap<String, ParamValue>,
    tolerance: f64,
    boolean_tolerance: f64,
    asset_data: &HashMap<String, Vec<u8>>,
    limits: Limits,
    options: EvalOptions,
) -> (Context, Vec<cadpole_script::OutputFile>, f64) {
    // Not `Instant`: it panics on wasm32 (see cadpole_core::time).
    let start = cadpole_core::time::Stopwatch::start();
    let mut input = eval_input(doc, extra_overrides, tolerance, boolean_tolerance, asset_data, limits);
    input.memo = options.memo;
    input.preview = options.preview;
    input.limits.solve_jobs |= options.solve_jobs;
    let out = cadpole_script::evaluate_full(input);
    (out.ctx, out.files, start.elapsed_ms())
}

/// What the script layer evaluates for a document: its files, its own
/// overrides with the caller's on top, its assets and the limits. Every
/// evaluation here is built from this, and `run_tests` builds one per sweep
/// point from it.
pub fn eval_input(
    doc: &pb::Document,
    extra_overrides: &HashMap<String, ParamValue>,
    tolerance: f64,
    boolean_tolerance: f64,
    asset_data: &HashMap<String, Vec<u8>>,
    limits: Limits,
) -> EvalInput {
    let mut overrides = overrides_from_proto(&doc.param_overrides);
    overrides.extend(extra_overrides.iter().map(|(k, v)| (k.clone(), v.clone())));
    let assets: Option<Arc<dyn AssetStore>> = if doc.assets.is_empty() && asset_data.is_empty() {
        None
    } else {
        Some(Arc::new(assets_from_proto(doc, asset_data)))
    };
    EvalInput {
        scripts: doc.scripts.iter().map(|s| ScriptFile { path: s.path.clone(), source: s.source.clone() }).collect(),
        entry: None,
        overrides,
        tolerance,
        boolean_tolerance,
        selection: selection_from_proto(&doc.selection),
        assets,
        limits,
        tests: cadpole_script::TestMode::Skip,
        memo: None,
        preview: false,
        sweep_results: None,
    }
}

pub fn evaluate(req: &pb::EvaluateRequest) -> pb::EvaluateResponse {
    evaluate_with(req, None)
}

/// [`evaluate`] with the caller's memo: `cadpole serve` passes its
/// session's, the CLI passes none. `req.preview` says whether this is a
/// drag preview.
pub fn evaluate_with(req: &pb::EvaluateRequest, memo: Option<Arc<Memo>>) -> pb::EvaluateResponse {
    evaluate_with_options(req, EvalOptions { memo, preview: req.preview, solve_jobs: false })
}

/// [`evaluate_with`] with everything a host adds (`EvalOptions`); the
/// request's `preview` is honoured either way.
pub fn evaluate_with_options(req: &pb::EvaluateRequest, options: EvalOptions) -> pb::EvaluateResponse {
    let options = EvalOptions { preview: options.preview || req.preview, ..options };
    let Some(doc) = &req.document else {
        let mut ctx = Context::new();
        ctx.diagnostics.push(Diagnostic::error("evaluate: no document"));
        return pb::EvaluateResponse { model: Some(ctx.to_model(true, 0.0)) };
    };
    let (mut ctx, _files, ms) = evaluate_document_options(
        doc,
        &HashMap::new(),
        req.tolerance,
        req.boolean_tolerance,
        &req.asset_data,
        limits::limits_from_request(req),
        options,
    );
    pb::EvaluateResponse { model: Some(print::model_with_plates(&mut ctx, req.skip_mesh, ms)) }
}

/// Camera from a proto camera, fitted to the scene when it is a preset.
pub fn camera_from_proto(cam: &Option<pb::Camera>, lo: Vec3, hi: Vec3) -> Camera {
    camera_from_proto_aspect(cam, lo, hi, 4.0 / 3.0)
}

/// As [`camera_from_proto`], framing the model for an image of this aspect
/// ratio (width / height).
pub fn camera_from_proto_aspect(cam: &Option<pb::Camera>, lo: Vec3, hi: Vec3, aspect: f64) -> Camera {
    let c = cam.clone().unwrap_or_default();
    let preset = if c.preset.is_empty() && c.eye.is_none() { "iso" } else { c.preset.as_str() };
    let margin = if c.fit_margin > 0.0 { c.fit_margin } else { Camera::preset_margin(preset) };
    if !preset.is_empty() {
        let mut k = Camera::preset(preset, lo, hi, margin);
        if c.fov_degrees > 0.0 {
            k.fov_degrees = c.fov_degrees;
        }
        k.orthographic = c.orthographic;
        k.fit(lo, hi, margin, aspect);
        return k;
    }
    let v = |o: &Option<pb::Vec3>, d: Vec3| o.as_ref().map(|v| Vec3::new(v.x, v.y, v.z)).unwrap_or(d);
    Camera {
        eye: v(&c.eye, Vec3::new(100.0, -100.0, 80.0)),
        target: v(&c.target, (lo + hi) * 0.5),
        up: v(&c.up, Vec3::Z),
        fov_degrees: if c.fov_degrees > 0.0 { c.fov_degrees } else { 35.0 },
        orthographic: c.orthographic,
    }
}

/// Render the visible bodies of an evaluated context.
pub fn render_context(ctx: &mut Context, cam: &Option<pb::Camera>, opts: &RenderOptions, body_ids: &[String]) -> Image {
    render_context_ex(ctx, cam, None, opts, body_ids, &[])
}

/// `RenderRequest.layers` as the engine's overrides.
pub fn layer_overrides_from_proto(states: &[pb::LayerState]) -> Vec<LayerOverride> {
    states.iter().map(LayerOverride::from_proto).collect()
}

/// One `--layer NAME=on|off|<opacity>` flag as a `LayerState`: what
/// `cadpole render`, `cadpole sweep`, `cadpole animate` and `cadpole op` all parse.
pub fn parse_layer_state(spec: &str) -> anyhow::Result<pb::LayerState> {
    LayerOverride::parse(spec).map(|o| o.to_proto()).map_err(|e| anyhow::anyhow!(e))
}

/// Everything the renderer needs from an evaluated document, as plain data:
/// `Send`, so variants can be evaluated on several threads and rendered with
/// one shared camera afterwards.
pub struct RenderScene {
    pub items: Vec<SceneItem>,
    pub bbox: Option<(Vec3, Vec3)>,
}

impl RenderScene {
    /// Every mesh vertex in the scene, for exact camera framing.
    pub fn points(&self) -> impl Iterator<Item = Vec3> + '_ {
        self.items.iter().flat_map(|i| {
            i.mesh
                .positions
                .chunks_exact(3)
                .map(|p| Vec3::new(p[0] as f64, p[1] as f64, p[2] as f64))
        })
    }

    /// Frame this scene in an image of the given aspect ratio.
    pub fn fit(&self, cam: &mut Camera, margin: f64, aspect: f64) {
        cam.fit_points(|| self.points(), margin, aspect);
    }
}

/// The union of the items' mesh boxes.
fn items_bbox(items: &[SceneItem]) -> Option<(Vec3, Vec3)> {
    let mut bbox: Option<(Vec3, Vec3)> = None;
    for it in items {
        if let Some((lo, hi)) = it.mesh.bbox() {
            bbox = Some(match bbox {
                None => (lo, hi),
                Some((a, b)) => (a.min(lo), b.max(hi)),
            });
        }
    }
    bbox
}

/// Seams folded by less than this shade as one surface (`Mesh::smooth_seams`).
/// It matches the line overlay's default crease angle, so an edge is drawn
/// exactly when it is shaded as one.
pub const SMOOTH_SEAM_DEGREES: f64 = 30.0;

pub struct SceneItem {
    pub mesh: cadpole_core::tessellate::Mesh,
    pub material: cadpole_core::context::Material,
    /// One frame per topology face, for the machined textures
    /// ([`face_frames_for`]).
    pub face_frames: Vec<FaceFrame>,
    /// One colour per triangle, replacing the material's base colour. The
    /// body's face `paint(...)` ([`paint_colors`]) for a normal render, empty
    /// when it has none; replaced by [`apply_analysis`] for the `thickness`
    /// and `overhang` views and by [`apply_debug`] for the debug views.
    pub tri_colors: Vec<[f32; 3]>,
    /// `tri_colors` are the body's paint, which keeps the material's other
    /// properties; false once an analysis or debug view has replaced them.
    pub paint: bool,
    pub edges: Vec<Vec<Vec3>>,
    /// Crease angle per edge, so the overlay can leave tangent seams between
    /// patches of one smooth surface undrawn.
    pub edge_creases: Vec<f64>,
    pub highlight_faces: Vec<u32>,
    pub highlight_edges: Vec<Vec<Vec3>>,
}

/// A body's `paint(...)` features (`cadpole_core::paint`) as the renderer
/// draws them, read in feature order exactly as the 3MF palette
/// (`print::palette_with`) reads them, so the viewport shows paint as it
/// will print.
///
/// A whole-body paint becomes the material's base colour, which keeps the
/// material's character (roughness, metal, texture), and clears the face
/// paint before it. Face paint becomes one colour per triangle
/// (`SceneItem::tri_colors`), with the unpainted triangles in the body's
/// colour; empty when the body has no face paint.
fn paint_colors(
    paints: &[cadpole_core::paint::Paint],
    id_string: &str,
    material: &mut cadpole_core::context::Material,
    mesh: &cadpole_core::tessellate::Mesh,
    topo: &cadpole_core::tessellate::Topology,
) -> Vec<[f32; 3]> {
    // Per topology face: the colour painted on it, if any.
    let mut face_paint: Vec<Option<[f64; 3]>> = Vec::new();
    for p in paints.iter().filter(|p| p.body_id == id_string) {
        if p.faces.is_empty() {
            material.base_color = p.color;
            face_paint.clear();
            continue;
        }
        if face_paint.is_empty() {
            face_paint = vec![None; topo.faces.len()];
        }
        for (i, f) in topo.faces.iter().enumerate() {
            if p.faces.iter().any(|n| *n == f.name) {
                face_paint[i] = Some(p.color);
            }
        }
    }
    if face_paint.iter().all(Option::is_none) {
        return Vec::new();
    }
    let f32s = |c: [f64; 3]| [c[0] as f32, c[1] as f32, c[2] as f32];
    let base = f32s(material.base_color);
    (0..mesh.triangle_count())
        .map(|t| {
            mesh.face_ids
                .get(t)
                .and_then(|f| face_paint.get(*f as usize).copied().flatten())
                .map(f32s)
                .unwrap_or(base)
        })
        .collect()
}

/// Per-triangle colours as one colour per face id, when they are that: every
/// triangle of a face the same colour. `None` when there are none or a face
/// holds more than one.
fn per_face_colors(tri_colors: &[[f32; 3]], face_ids: &[u32]) -> Option<HashMap<u32, [f32; 3]>> {
    if tri_colors.is_empty() || tri_colors.len() != face_ids.len() {
        return None;
    }
    let mut out = HashMap::new();
    for (c, f) in tri_colors.iter().zip(face_ids) {
        if *out.entry(*f).or_insert(*c) != *c {
            return None;
        }
    }
    Some(out)
}

/// Tessellate the visible bodies of an evaluated context into a scene.
///
/// `layers` are the render's display-layer overrides, folded over
/// the document's own `layer(...)` declarations: a body on a hidden layer
/// is left out, and a body on a ghosted layer gets its material's opacity
/// scaled by the layer's, which is how every backend already draws a
/// translucent body. A body on no layer is drawn as it is unless the
/// implicit layer (`other`) is overridden.
pub fn scene_from_context(ctx: &mut Context, body_ids: &[String], highlight: &[EntityRef], layers: &[LayerOverride]) -> RenderScene {
    let layers = cadpole_core::layers::resolve_implicit(layers, &ctx.layers);
    let ids: Vec<_> = ctx
        .bodies
        .iter()
        .filter(|b| b.visible && (body_ids.is_empty() || body_ids.contains(&b.id_string())))
        .filter(|b| cadpole_core::layers::effective(&b.layer, &ctx.layers, &layers).0)
        .map(|b| b.id)
        .collect();
    let paints = cadpole_core::paint::paints(ctx);
    let mut items = Vec::with_capacity(ids.len());
    for id in &ids {
        let (mut material, feature, id_string) = {
            let b = ctx.body(*id).unwrap();
            let (_, layer_opacity) = cadpole_core::layers::effective(&b.layer, &ctx.layers, &layers);
            let mut material = b.material.clone();
            if layer_opacity < 0.999 {
                material.opacity = (material.opacity * layer_opacity).clamp(0.0, 1.0);
            }
            (material, b.feature.0.clone(), b.id_string())
        };
        // An instance body in an exploded assembly is drawn where the view
        // puts it: its cached mesh is at the pose, and the offset is
        // applied here, at scene time, never to the geometry.
        let offset = ctx.body_view_offset(*id).unwrap_or(Vec3::ZERO);
        let Ok((mesh, topo)) = ctx.body_mesh(*id) else { continue };
        // Shading only: a curve drawn as a polyline, or a loft through many
        // sections, otherwise renders every segment as a facet.
        let mut mesh = if offset.length() > 0.0 { mesh.transformed(&cadpole_core::Transform::translation(offset)) } else { mesh.clone() };
        mesh.smooth_seams(SMOOTH_SEAM_DEGREES);
        let face_frames = face_frames_for(topo, mesh.bbox());
        let edges: Vec<Vec<Vec3>> =
            topo.edges.iter().map(|e| e.polyline.iter().map(|p| *p + offset).collect()).collect();
        let edge_creases: Vec<f64> = topo.edges.iter().map(|e| e.dihedral_degrees).collect();
        let tri_colors = paint_colors(&paints, &id_string, &mut material, &mesh, topo);
        let mut faces = Vec::new();
        let mut hl_edges = Vec::new();
        for r in highlight {
            let same_body = r.body_id == id_string || (r.body_id.is_empty() && r.feature_id == feature);
            if !same_body {
                continue;
            }
            match r.kind.as_str() {
                "face" => {
                    if let Some(i) = topo.faces.iter().position(|f| f.name == r.topo_name) {
                        faces.push(i as u32);
                    }
                }
                "edge" => {
                    if let Some(e) = topo.edges.iter().find(|e| e.name == r.topo_name) {
                        hl_edges.push(e.polyline.iter().map(|p| *p + offset).collect());
                    }
                }
                "body" | "feature" | "" => faces.extend(0..topo.faces.len() as u32),
                _ => {}
            }
        }
        items.push(SceneItem {
            mesh,
            material,
            face_frames,
            paint: !tri_colors.is_empty(),
            tri_colors,
            edges,
            edge_creases,
            highlight_faces: faces,
            highlight_edges: hl_edges,
        });
    }
    // The box of what is drawn: a body left out by a hidden layer or by
    // `body_ids` must not move the free camera's default target
    // (`camera_from_proto_aspect`) off the bodies that are shown. The item
    // meshes already carry any exploded-view offset, as
    // `Context::display_bbox` applies it.
    let bbox = items_bbox(&items);
    RenderScene { items, bbox }
}

/// The frame each face's machined texture is laid out in
/// (`cadpole_core::render::FaceFrame`, `textures::machined`). A face of
/// revolution — cylinder, cone, torus — is turned about its own axis; every
/// other face is turned about the body's, which is the axis of its largest
/// face of revolution (so a turned part's end faces and chamfers get the
/// spindle axis), or Z through the bounding-box centre when it has none.
/// The milling direction is the face's own `v` (its ruling), and the
/// scallop phase is anchored at its centroid. A face is `turned` — what
/// `texture="machined"` turns rather than mills — when it is a surface of
/// revolution about the body's axis or a plane square to it: the hub, the
/// bore, the chamfers and the back face of a wheel, and not its blades.
pub fn face_frames_for(topo: &cadpole_core::tessellate::Topology, bbox: Option<(Vec3, Vec3)>) -> Vec<FaceFrame> {
    use cadpole_core::SurfaceClass;
    let f3 = |v: Vec3| [v.x as f32, v.y as f32, v.z as f32];
    let axis_of = |s: &SurfaceClass| match *s {
        SurfaceClass::Cylinder { axis_point, axis, .. } => Some((axis_point, axis)),
        SurfaceClass::Cone { apex, axis, .. } => Some((apex, axis)),
        SurfaceClass::Torus { center, axis, .. } => Some((center, axis)),
        _ => None,
    };
    let center = bbox.map(|(lo, hi)| (lo + hi) * 0.5).unwrap_or(Vec3::ZERO);
    let body_axis = topo
        .faces
        .iter()
        .filter_map(|f| axis_of(&f.surface).map(|a| (f.area, a)))
        .max_by(|a, b| a.0.total_cmp(&b.0))
        .map(|(_, a)| a)
        .unwrap_or((center, Vec3::Z));
    let parallel = |a: Vec3, b: Vec3| a.normalized().dot(b.normalized()).abs() > 0.9999;
    topo.faces
        .iter()
        .map(|f| {
            let own = axis_of(&f.surface);
            let (axis_point, axis) = own.unwrap_or(body_axis);
            let turned = match (&f.surface, own) {
                (_, Some((_, a))) => parallel(a, body_axis.1),
                (SurfaceClass::Plane { normal, .. }, None) => parallel(*normal, body_axis.1),
                _ => false,
            };
            FaceFrame { axis_point: f3(axis_point), axis: f3(axis), origin: f3(f.centroid), dir: f3(f.v_dir), turned }
        })
        .collect()
}

/// Render a prepared scene.
pub fn render_scene(scene: &RenderScene, camera: &Camera, opts: &RenderOptions) -> Image {
    render_scene_stats(scene, camera, opts).0
}

/// Full-control render: an explicit camera (else fitted from `cam`) and a
/// set of entity refs to highlight (selected faces/edges/bodies).
pub fn render_context_ex(
    ctx: &mut Context,
    cam: &Option<pb::Camera>,
    explicit: Option<&Camera>,
    opts: &RenderOptions,
    body_ids: &[String],
    highlight: &[EntityRef],
) -> Image {
    render_context_stats(ctx, cam, explicit, opts, body_ids, highlight, &[]).0
}

/// [`render_context_ex`] plus the renderer's per-stage timings.
pub fn render_context_stats(
    ctx: &mut Context,
    cam: &Option<pb::Camera>,
    explicit: Option<&Camera>,
    opts: &RenderOptions,
    body_ids: &[String],
    highlight: &[EntityRef],
    layers: &[LayerOverride],
) -> (Image, RenderStats) {
    let scene = scene_from_context(ctx, body_ids, highlight, layers);
    let camera = camera_for_scene(&scene, cam, explicit, opts);
    render_scene_stats(&scene, &camera, opts)
}

/// The camera a scene will be drawn with: the explicit one, or the request's
/// preset fitted to the geometry.
pub fn camera_for_scene(
    scene: &RenderScene,
    cam: &Option<pb::Camera>,
    explicit: Option<&Camera>,
    opts: &RenderOptions,
) -> Camera {
    let (lo, hi) = scene.bbox.unwrap_or((Vec3::new(-10.0, -10.0, 0.0), Vec3::new(10.0, 10.0, 10.0)));
    let aspect = opts.width.max(1) as f64 / opts.height.max(1) as f64;
    match explicit {
        Some(c) => c.clone(),
        None => {
            let mut c = camera_from_proto_aspect(cam, lo, hi, aspect);
            // Presets frame the geometry itself; an explicit eye/target is left alone.
            if cam.as_ref().map(|c| !c.preset.is_empty() || c.eye.is_none()).unwrap_or(true) {
                let preset = cam.as_ref().map(|c| c.preset.as_str()).filter(|p| !p.is_empty()).unwrap_or("iso");
                let margin = cam.as_ref().map(|c| c.fit_margin).filter(|m| *m > 0.0).unwrap_or(Camera::preset_margin(preset));
                scene.fit(&mut c, margin, aspect);
                // The product presets sit the subject a little below the
                // middle of the frame: floor in the foreground, air above.
                c.place_subject(Camera::preset_subject_height(preset));
            }
            c
        }
    }
}

/// The look a request asks for (docs/views.md, "The product shot"): which
/// rig, which surround, which floor, which tone curve. Parsed once, so an
/// unknown name is one diagnostic naming the choices, and applied to
/// [`RenderOptions`] in one place, so `Render` and `Sweep` cannot differ.
#[derive(Clone, Debug, Default, PartialEq)]
pub struct Look {
    pub lighting: Lighting,
    pub backdrop: Option<Backdrop>,
    pub floor: Floor,
    pub tonemap: ToneMap,
    /// The request's own sun, resolved to angles; `None` defers to
    /// the document's `render_defaults(sun=)`.
    pub sun: Option<SunSpec>,
    /// The request's north, degrees clockwise from `+y`; 0 defers to the
    /// document's.
    pub north: f64,
    /// The request's section plane, for the image only.
    pub section: Option<Section>,
}

impl Look {
    /// From the request's four look strings; empty means the default of
    /// each. The sun, north and section are [`Look::with_sun`]'s.
    pub fn parse(lighting: &str, backdrop: &str, floor: &str, tonemap: &str) -> anyhow::Result<Look> {
        let choices = |names: &[&str]| names.iter().map(|n| format!("\"{n}\"")).collect::<Vec<_>>().join(", ");
        Ok(Look {
            sun: None,
            north: 0.0,
            section: None,
            lighting: Lighting::parse(lighting)
                .ok_or_else(|| anyhow::anyhow!("unknown lighting {lighting:?}; expected one of {}", choices(Lighting::names())))?,
            backdrop: if backdrop.is_empty() {
                None
            } else {
                Some(
                    Backdrop::parse(backdrop)
                        .ok_or_else(|| anyhow::anyhow!("unknown backdrop {backdrop:?}; expected one of {}", choices(Backdrop::names())))?,
                )
            },
            floor: Floor::parse(floor)
                .ok_or_else(|| anyhow::anyhow!("unknown floor {floor:?}; expected one of {}", choices(Floor::names())))?,
            tonemap: ToneMap::parse(tonemap)
                .ok_or_else(|| anyhow::anyhow!("unknown tonemap {tonemap:?}; expected one of {}", choices(ToneMap::names())))?,
        })
    }

    /// The sun, north and section strings a request carries: a
    /// misspelt sun or section is one error naming the form, before any
    /// evaluation, like a misspelt look.
    pub fn with_sun(mut self, sun: &str, north_degrees: f64, section: &str) -> anyhow::Result<Look> {
        self.sun = cadpole_core::sun::parse_spec(sun).map_err(|e| anyhow::anyhow!(e))?;
        if !north_degrees.is_finite() {
            anyhow::bail!("north must be a finite number of degrees");
        }
        self.north = north_degrees;
        self.section = Section::parse(section).map_err(|e| anyhow::anyhow!(e))?;
        Ok(self)
    }

    pub fn from_render(req: &pb::RenderRequest) -> anyhow::Result<Look> {
        Look::parse(&req.lighting, &req.backdrop, &req.floor, &req.tonemap)?.with_sun(&req.sun, req.north_degrees, &req.section)
    }

    pub fn from_sweep(req: &pb::SweepRequest) -> anyhow::Result<Look> {
        Look::parse(&req.lighting, &req.backdrop, &req.floor, &req.tonemap)?.with_sun(&req.sun, req.north_degrees, "")
    }

    /// Write the look into the options. `keep_background` is whether the
    /// request set its own `background`, which a backdrop then leaves
    /// alone: the backdrop still lights and is reflected, and the pixels
    /// the model does not cover are that colour. Otherwise the backdrop is
    /// shown, and its horizon, through the tone map, is the rasterizer's
    /// background. Must run after `exposure` and the backend are known,
    /// and it does not touch `show_environment` unless a backdrop asks
    /// for it.
    pub fn apply(&self, o: &mut RenderOptions, keep_background: bool) {
        o.lighting = self.lighting;
        o.tonemap = self.tonemap;
        o.backdrop = self.backdrop;
        o.sun = resolve_sun(self.sun.as_ref(), self.north, None);
        o.path.ground = self.floor != Floor::None;
        o.path.ground_roughness = self.floor.roughness();
        if let Some(b) = &self.backdrop {
            o.path.ground_albedo = b.floor_albedo();
            if !keep_background {
                o.path.show_environment = true;
                o.background = b.background(o.exposure, o.tonemap);
            }
        }
    }
}

/// The sun a render draws with: the request's, else the document's
/// `render_defaults(sun=)`; turned into model space by the request's north
/// when it gives one, else the document's, else `+y`. `None` when neither
/// names a sun.
pub fn resolve_sun(request: Option<&SunSpec>, north_degrees: f64, defaults: Option<&RenderDefaults>) -> Option<SunLight> {
    let sun = request.or(defaults.and_then(|d| d.sun.as_ref()))?;
    let north = if north_degrees != 0.0 { north_degrees } else { defaults.and_then(|d| d.north_degrees).unwrap_or(0.0) };
    Some(SunLight::new(sun.direction(north)))
}

/// Fold a document's `render_defaults(...)` into the options a
/// request produced: the sun, where the request named none. Run after the
/// document is evaluated, which is the only time the defaults are known.
pub fn apply_document_defaults(o: &mut RenderOptions, look: &Look, defaults: Option<&RenderDefaults>) {
    o.sun = resolve_sun(look.sun.as_ref(), look.north, defaults);
}

/// The camera a request means: its own, unless it names no preset
/// and no eye, in which case the document's `render_defaults(camera=)`
/// stands in; with neither, the request as it is (which fits `iso`).
pub fn document_camera(cam: &Option<pb::Camera>, defaults: Option<&RenderDefaults>) -> Option<pb::Camera> {
    let unset = cam.as_ref().map(|c| c.preset.is_empty() && c.eye.is_none()).unwrap_or(true);
    match (unset, defaults.and_then(|d| d.camera)) {
        (true, Some(c)) => Some(pb::Camera {
            preset: String::new(),
            eye: Some(c.eye.to_proto()),
            target: Some(c.target.to_proto()),
            up: Some(c.up.to_proto()),
            fov_degrees: c.fov_degrees,
            orthographic: cam.as_ref().map(|c| c.orthographic).unwrap_or(false),
            fit_margin: 0.0,
        }),
        _ => cam.clone(),
    }
}

/// Is this request camera a free one — an eye placed by hand rather than a
/// preset fitted to the model?
pub fn is_free_camera(cam: &Option<pb::Camera>) -> bool {
    cam.as_ref().map(|c| c.preset.is_empty() && c.eye.is_some()).unwrap_or(false)
}

/// Cut every item of a scene by a section plane, for the image only
/// (`cadpole_core::render::section`): the mesh replaced by its capped
/// kept half, the feature edges clipped to the kept side, the bounding box
/// recomputed so the camera frames the cut. Returns one warning per body
/// the mesh kernel could not cut, which is drawn whole.
pub fn section_scene(scene: &mut RenderScene, sec: &Section, tolerance: f64) -> Vec<String> {
    let mut warnings = Vec::new();
    let mut kept = Vec::with_capacity(scene.items.len());
    for (i, mut item) in scene.items.drain(..).enumerate() {
        match section::cut(&item.mesh, sec, tolerance) {
            section::Cut::Gone => continue,
            section::Cut::Untouched => {}
            section::Cut::Refused(why) => warnings.push(format!("section {}: body {i} is drawn whole: {why}", sec.label())),
            section::Cut::Mesh(m) => {
                // The colours were per triangle of the whole mesh, and a cut
                // mesh has other triangles. Colours that are one per face
                // (paint) follow the face ids the cut keeps, with the cap in
                // the body's colour; a field that varies across a face (an
                // analysis) cannot follow, and is dropped.
                let per_face = per_face_colors(&item.tri_colors, &item.mesh.face_ids);
                item.mesh = m;
                item.mesh.smooth_seams(SMOOTH_SEAM_DEGREES);
                item.tri_colors = match per_face {
                    Some(per_face) => {
                        let m = item.material.base_color;
                        let base = [m[0] as f32, m[1] as f32, m[2] as f32];
                        (0..item.mesh.triangle_count())
                            .map(|t| item.mesh.face_ids.get(t).and_then(|f| per_face.get(f)).copied().unwrap_or(base))
                            .collect()
                    }
                    None => Vec::new(),
                };
                let mut edges = Vec::new();
                let mut creases = Vec::new();
                for (e, c) in item.edges.iter().zip(item.edge_creases.iter().chain(std::iter::repeat(&90.0))) {
                    for run in section::clip_polyline(e, sec) {
                        edges.push(run);
                        creases.push(*c);
                    }
                }
                item.edges = edges;
                item.edge_creases = creases;
                item.highlight_edges = item.highlight_edges.iter().flat_map(|e| section::clip_polyline(e, sec)).collect();
            }
        }
        kept.push(item);
    }
    scene.items = kept;
    scene.bbox = items_bbox(&scene.items);
    warnings
}

/// Colour every item of a scene by a measured field instead of its material,
/// and return the legend to draw over the finished image (docs/views.md).
///
/// The per-triangle values come from the same `cadpole-print` checks
/// `print_check` reports, so the picture and the numbers cannot disagree.
pub fn apply_analysis(
    scene: &mut RenderScene,
    analysis: &str,
    profile: &cadpole_print::Profile,
) -> anyhow::Result<cadpole_print::Legend> {
    use cadpole_print::analysis::{Analysis, overhang_colors, overhang_legend, thickness_colors, thickness_legend};
    let kind = Analysis::parse(analysis).map_err(|e| anyhow::anyhow!(e))?;
    // The bed is the *scene's* lowest point, not each body's: a label plate
    // held up by the wall behind it overhangs, even though it is the lowest
    // thing in its own mesh. Thickness stays per body, because a body is what
    // a wall belongs to.
    let up = profile.up();
    let floor = scene
        .items
        .iter()
        .map(|i| cadpole_print::overhang::lowest(&i.mesh, up))
        .fold(f64::INFINITY, f64::min);
    for item in &mut scene.items {
        item.paint = false;
        item.tri_colors = match kind {
            Analysis::Thickness => {
                thickness_colors(&cadpole_print::thickness::check(&item.mesh, profile, &mut Vec::new()), profile)
            }
            Analysis::Overhang => overhang_colors(
                &cadpole_print::overhang::check_at(&item.mesh, profile, Some(floor), &mut Vec::new()),
                profile,
            ),
        };
    }
    Ok(match kind {
        Analysis::Thickness => thickness_legend(profile),
        Analysis::Overhang => overhang_legend(profile),
    })
}

/// Paint a scene for a debug view (`cadpole render --debug`, docs/views.md
/// "Debug views") and return the line the view reports: the face-group
/// count for `faces`, the defect counts and their colours for `defects`,
/// nothing for the rest.
///
/// This is the same per-triangle colour path [`apply_analysis`] uses, with
/// the colours coming from the mesh itself rather than a print check:
/// `faces` gets the palette of `cadpole_core::render::debug`, `defects` the
/// validator's findings, `seams` and `wire` a flat grey for the ink to sit
/// on. `normals` paints nothing here — its colour is per pixel, not per
/// triangle, and the rasterizer writes it from the G-buffer. `sliver_altitude`
/// is the tessellation tolerance: a triangle thinner than what the
/// tessellator was allowed to deviate by is one nothing asked for.
pub fn apply_debug(scene: &mut RenderScene, view: DebugView, sliver_altitude: f64) -> Option<String> {
    use cadpole_core::render::debug;
    match view {
        DebugView::Faces => {
            let (mut faces, mut tris) = (0, 0);
            for (i, item) in scene.items.iter_mut().enumerate() {
                item.tri_colors = debug::face_colors(&item.mesh, i);
                item.paint = false;
                faces += debug::face_count(&item.mesh);
                tris += item.mesh.triangle_count();
            }
            let bodies = scene.items.len();
            Some(format!(
                "faces: {faces} face group{} over {tris} triangles in {bodies} bod{}",
                if faces == 1 { "" } else { "s" },
                if bodies == 1 { "y" } else { "ies" }
            ))
        }
        DebugView::Normals => None,
        DebugView::Defects => {
            let mut counts = debug::DefectCounts::default();
            for item in &mut scene.items {
                let (colors, c) = debug::defect_colors(&item.mesh, sliver_altitude);
                item.tri_colors = colors;
                item.paint = false;
                counts.add(&c);
            }
            Some(counts.summary())
        }
        DebugView::Seams | DebugView::Wire => {
            let grey = if view == DebugView::Wire { debug::WIRE_BODY } else { debug::BODY_GREY };
            for item in &mut scene.items {
                item.tri_colors = vec![grey; item.mesh.triangle_count()];
                item.paint = false;
            }
            None
        }
    }
}

/// Render a prepared scene, returning per-stage timings.
pub fn render_scene_stats(scene: &RenderScene, camera: &Camera, opts: &RenderOptions) -> (Image, RenderStats) {
    // A debug view inspects the mesh, so every body is drawn opaque: a glass
    // cover over the part being inspected would otherwise hide it (or, worse,
    // show it through refraction and look like a defect).
    // `xray` (docs/views.md) draws every body at [`XRAY_OPACITY`] through
    // the same per-item override, so every backend shows the interiors.
    let opacity = match opts.debug {
        Some(_) => Some(1.0),
        None => opts.xray.then_some(XRAY_OPACITY),
    };
    let items = items_of(scene, opacity);
    cadpole_core::render::render_with_stats(&items, camera, opts)
}

/// A scene's items as the renderer's borrowed [`Item`]s, so a caller can
/// add an item of its own (the plate image's bed) before rendering.
pub(crate) fn items_of(scene: &RenderScene, opacity: Option<f64>) -> Vec<Item<'_>> {
    scene
        .items
        .iter()
        .map(|i| Item {
            mesh: &i.mesh,
            material: &i.material,
            face_frames: &i.face_frames,
            edges: &i.edges,
            edge_dihedrals: &i.edge_creases,
            tri_colors: &i.tri_colors,
            paint: i.paint,
            highlight_faces: &i.highlight_faces,
            highlight_edges: &i.highlight_edges,
            opacity,
        })
        .collect()
}

/// Evaluate a grid of parameter variants in parallel and return each one's
/// scene, in grid order. Evaluation dominates sweep time and is independent
/// per variant, so this is the difference between a sweep that an assistant
/// waits on and one it does not.
pub fn sweep_scenes(
    doc: &pb::Document,
    grid: &[Vec<(String, ParamValue)>],
    fixed: &HashMap<String, ParamValue>,
    tolerance: f64,
    boolean_tolerance: f64,
    threads: usize,
    layers: &[LayerOverride],
) -> Vec<SweepVariant> {
    let threads = threads.clamp(1, grid.len().max(1)).min(available_threads());
    let next = std::sync::atomic::AtomicUsize::new(0);
    let out: Vec<std::sync::Mutex<Option<SweepVariant>>> = (0..grid.len()).map(|_| std::sync::Mutex::new(None)).collect();
    std::thread::scope(|scope| {
        for _ in 0..threads {
            scope.spawn(|| {
                loop {
                    let i = next.fetch_add(1, std::sync::atomic::Ordering::Relaxed);
                    if i >= grid.len() {
                        break;
                    }
                    let combo = &grid[i];
                    let mut ov = fixed.clone();
                    for (k, v) in combo {
                        ov.insert(k.clone(), v.clone());
                    }
                    let (mut ctx, ms) = evaluate_document_with(doc, &ov, tolerance, boolean_tolerance);
                    let label = combo
                        .iter()
                        .map(|(k, v)| format!("{k}={}", value_label(v)))
                        .collect::<Vec<_>>()
                        .join(" ");
                    let diagnostics: Vec<Diagnostic> = ctx.diagnostics.clone();
                    let mut fixed_sorted: Vec<(String, ParamValue)> = fixed.iter().map(|(k, v)| (k.clone(), v.clone())).collect();
                    fixed_sorted.sort_by(|a, b| a.0.cmp(&b.0));
                    let point_warnings = sweep_point_warnings(&ctx, combo.iter().chain(fixed_sorted.iter()));
                    let scene = scene_from_context(&mut ctx, &[], &[], layers);
                    let render_defaults = ctx.render_defaults.clone();
                    *out[i].lock().unwrap() =
                        Some(SweepVariant { label, scene, diagnostics, eval_millis: ms, render_defaults, point_warnings });
                }
            });
        }
    });
    out.into_iter().map(|m| m.into_inner().unwrap().expect("every variant evaluated")).collect()
}

pub struct SweepVariant {
    pub label: String,
    pub scene: RenderScene,
    pub diagnostics: Vec<Diagnostic>,
    pub eval_millis: f64,
    /// The variant's `render_defaults(...)`: a sweep over an hour
    /// parameter is a shadow study, so each cell takes its own sun.
    pub render_defaults: Option<RenderDefaults>,
    /// Where the cell is not the point its label names
    /// ([`sweep_point_warnings`]).
    pub point_warnings: Vec<String>,
}

/// The ways a sweep cell's model is not the point its label names: a
/// parameter the document does not declare (the override is ignored), and
/// a value `param()` did not take as given (clamped into `min`/`max`, or
/// not one of the `choices`, so the default stands). A cell is labelled
/// with what was asked for; these say what was evaluated instead, so two
/// cells that are the same model under different labels are not read as
/// two results.
pub fn sweep_point_warnings<'a>(ctx: &Context, point: impl Iterator<Item = &'a (String, ParamValue)>) -> Vec<String> {
    let mut out = Vec::new();
    for (name, asked) in point {
        match ctx.params.iter().find(|(d, _)| &d.name == name) {
            None => out.push(format!("{UNDECLARED} {name:?} is not a param of this document; it changes nothing")),
            // A value of the wrong kind is already a warning from the param
            // itself, which says why; only a value of the right kind that
            // came back different was moved into the param's range.
            Some((_, got)) if got != asked && std::mem::discriminant(got) == std::mem::discriminant(asked) => {
                out.push(format!(
                    "sweep value {name}={} was evaluated as {name}={} (outside the param's min/max or choices)",
                    value_label(asked),
                    value_label(got)
                ))
            }
            Some(_) => {}
        }
    }
    out
}

/// How [`sweep_point_warnings`] begins a warning about a name the document
/// does not declare.
pub const UNDECLARED: &str = "sweep parameter";

/// Threads to use for engine-level parallelism (1 on wasm).
pub fn available_threads() -> usize {
    if cfg!(target_arch = "wasm32") {
        1
    } else {
        std::thread::available_parallelism().map(|n| n.get()).unwrap_or(1).min(16)
    }
}

pub fn render_options_from_proto(req: &pb::RenderRequest) -> RenderOptions {
    let mut opts = if req.draft { RenderOptions::draft(800, 600) } else { RenderOptions::default() };
    // The same bounds `limits::clamp_render` applies and explains (the
    // G-buffer is ~70 bytes per pixel per pass): a request it already
    // clamped passes untouched, so the size its warning names is the size
    // drawn. A second, smaller cap here once shrank 4096 x 4096 to
    // 4000 x 4000 with no word.
    let l = limits::render_limits();
    if req.width > 0 {
        opts.width = req.width.min(l.max_dimension);
    }
    if req.height > 0 {
        opts.height = req.height.min(l.max_dimension);
    }
    let px = opts.width as u64 * opts.height as u64;
    if px > l.max_pixels {
        let s = (l.max_pixels as f64 / px as f64).sqrt();
        opts.width = ((opts.width as f64 * s).floor() as u32).max(1);
        opts.height = ((opts.height as f64 * s).floor() as u32).max(1);
    }
    if req.samples > 0 {
        opts.samples = req.samples;
    }
    opts.threads = req.threads;
    if let Some(bg) = &req.background {
        opts.background = [bg.r as f32, bg.g as f32, bg.b as f32];
    }
    opts.path.show_environment = req.show_environment;
    opts.transparent = req.transparent;
    opts.shadow_catcher = req.shadow_catcher;
    opts.supersample = req.supersample.clamp(1, 4);
    opts.draw_edges = (req.draw_edges || !req.has_flags()) && !req.no_edges;
    opts.draw_grid = (req.draw_grid || !req.has_flags()) && !req.no_grid;
    if req.edge_crease_degrees > 0.0 {
        opts.edge_crease_degrees = req.edge_crease_degrees as f32;
    }
    opts.edges = match req.edge_mode() {
        pb::EdgeMode::Off => EdgeMode::Off,
        pb::EdgeMode::Silhouette => EdgeMode::Silhouette,
        pb::EdgeMode::All => EdgeMode::All,
        pb::EdgeMode::Creases | pb::EdgeMode::Unspecified => EdgeMode::Creases,
    };
    opts.grid = match req.grid_mode() {
        pb::GridMode::Off => GridMode::Off,
        pb::GridMode::Finite => GridMode::Finite,
        pb::GridMode::Infinite | pb::GridMode::Unspecified => GridMode::Infinite,
    };
    opts.xray = req.xray;
    if req.no_ambient_occlusion {
        opts.ambient_occlusion = false;
    }
    if req.no_contact_shadow {
        opts.contact_shadow = false;
    }
    if req.no_silhouettes {
        opts.draw_silhouettes = false;
    }
    if req.exposure > 0.0 {
        opts.exposure = req.exposure as f32;
    }
    if req.edge_width_px > 0.0 {
        opts.edge_width_px = (req.edge_width_px as f32).clamp(0.2, 16.0);
    }
    opts.backend = match req.backend() {
        pb::RenderBackend::PathTrace => Backend::Path,
        pb::RenderBackend::Raster | pb::RenderBackend::Unspecified => Backend::Raster,
    };
    if opts.backend == Backend::Path {
        // The rasterizer's 1/4/8 anti-aliasing counts are far too few paths
        // to resolve an image, so an unset `samples` means something quite
        // different here.
        if req.samples == 0 {
            opts.samples = if req.draft { 16 } else { 64 };
        }
        if req.max_bounces > 0 {
            opts.path.max_bounces = req.max_bounces.min(64);
        }
    }
    opts.path.dof_f_number = if req.dof_f_number.is_finite() && req.dof_f_number > 0.0 { req.dof_f_number as f32 } else { 0.0 };
    opts.path.edge_break_mm = if req.edge_break_mm.is_finite() && req.edge_break_mm > 0.0 { req.edge_break_mm as f32 } else { 0.0 };
    opts.path.denoise = req.denoise;
    opts.bloom = req.bloom;
    // The look, after exposure and the backend; an unknown name is reported
    // by `render`, which parses it first, so here the default stands in.
    Look::from_render(req).unwrap_or_default().apply(&mut opts, req.background.is_some());
    // Last, because it overrides the backend and quality knobs above; an
    // unknown name is reported by `render`, not here.
    if let Ok(view) = DebugView::parse(&req.debug) {
        opts.set_debug(view);
    }
    opts
}

/// Per-stage timings as a proto message, for `cadpole render --stats`.
pub fn render_stats_to_proto(st: &RenderStats, total_ms: f64) -> pb::RenderStats {
    pb::RenderStats {
        total_ms,
        passes: st.passes,
        threads_used: st.threads_used,
        geometry_ms: st.geometry_ms,
        ssao_ms: st.ssao_ms,
        contact_ms: st.contact_ms,
        shade_ms: st.shade_ms,
        oit_ms: st.oit_ms,
        overlay_ms: st.overlay_ms,
        resolve_ms: st.resolve_ms,
    }
}

trait HasFlags {
    fn has_flags(&self) -> bool;
}
impl HasFlags for pb::RenderRequest {
    // proto3 booleans cannot distinguish "unset" from false; treat the pair
    // both-false as "defaults" so the common case draws edges and a grid.
    fn has_flags(&self) -> bool {
        self.draw_edges || self.draw_grid
    }
}

pub fn render(req: &pb::RenderRequest) -> anyhow::Result<pb::RenderResponse> {
    // A render request is the easiest way to accidentally wedge the machine
    // that also serves the UI, so the size knobs are clamped here rather than
    // in one front-end: every caller, CLI included, gets the same bound and
    // the same explanation (docs/security.md, "Render and sweep limits").
    let mut clamped = req.clone();
    let mut warnings = limits::clamp_render(&mut clamped);
    let req = &clamped;
    let doc = req.document.as_ref().ok_or_else(|| anyhow::anyhow!("render: no document"))?;
    // A misspelt look is an error before any evaluation, like a bad debug
    // view: the options builder cannot report, so it is checked here.
    let look = Look::from_render(req)?;
    let extra = overrides_from_proto(&req.param_overrides);
    let (mut ctx, ms) = evaluate_document_with(doc, &extra, req.tolerance, req.boolean_tolerance);
    let mut opts = render_options_from_proto(req);
    apply_document_defaults(&mut opts, &look, ctx.render_defaults.as_ref());
    let highlight: Vec<EntityRef> = if req.highlight_selection { selection_from_proto(&doc.selection) } else { Vec::new() };
    let t0 = cadpole_core::time::Stopwatch::start();
    let layers = layer_overrides_from_proto(&req.layers);
    let mut scene = scene_from_context(&mut ctx, &req.body_ids, &highlight, &layers);
    if let Some(sec) = &look.section {
        for w in section_scene(&mut scene, sec, ctx.tolerance) {
            warnings.push(Diagnostic::warning(w).to_proto());
        }
    }
    let cam_req = document_camera(&req.camera, ctx.render_defaults.as_ref());
    let debug = if req.debug.is_empty() {
        None
    } else {
        let view = DebugView::parse(&req.debug).map_err(|e| anyhow::anyhow!(e))?;
        if !req.analysis.is_empty() {
            anyhow::bail!("`debug` and `analysis` both paint the surface; ask for one of them");
        }
        Some(view)
    };
    let legend = if req.analysis.is_empty() {
        None
    } else {
        let profile = print::profile("", req.min_wall, 0.0, 0.0, 0.0, "", req.max_overhang_degrees, &[], &[])?;
        Some(apply_analysis(&mut scene, &req.analysis, &profile)?)
    };
    // The view's one-line report goes out as an info diagnostic: `cadpole
    // render` prints it to stderr, and it reaches an assistant next to the
    // image, which a line on stderr would not.
    let report = debug.and_then(|view| apply_debug(&mut scene, view, ctx.tolerance));
    let camera = camera_for_scene(&scene, &cam_req, None, &opts);
    let info = camera_info(&camera, &scene, opts.width, opts.height, 0, 0.0);
    let (mut img, st) = render_scene_stats(&scene, &camera, &opts);
    if let Some(l) = legend {
        cadpole_print::analysis::draw_legend(&mut img, &l);
    }
    let total_ms = t0.elapsed_ms();
    let model = ctx.to_model(true, ms);
    warnings.extend(model.diagnostics.iter().cloned());
    if let Some(line) = report {
        warnings.push(Diagnostic::info(line).to_proto());
    }
    Ok(pb::RenderResponse {
        png: img.to_png(),
        width: img.width,
        height: img.height,
        diagnostics: warnings,
        model: Some(model),
        stats: Some(render_stats_to_proto(&st, total_ms)),
        camera: Some(info),
    })
}

/// What a caller needs to draw over a render (docs/views.md, "The camera
/// sidecar"): the camera as it stood for this image, as a position and as
/// matrices; the ground plane the model stands on; and where the model's
/// bounding box lands in the image.
///
/// The ground is the plane the renderers put their floor, grid and contact
/// shadow on -- the lowest vertex of what was drawn -- with its origin
/// under the world origin and its axes along world x and y, so a grid
/// drawn from it lines up with the model's own coordinates.
pub fn camera_info(camera: &Camera, scene: &RenderScene, width: u32, height: u32, index: u32, t: f64) -> pb::CameraInfo {
    let mut bounds: Option<(Vec3, Vec3)> = None;
    for p in scene.points() {
        bounds = Some(match bounds {
            None => (p, p),
            Some((lo, hi)) => (lo.min(p), hi.max(p)),
        });
    }
    let (lo, hi) = bounds.or(scene.bbox).unwrap_or((Vec3::new(-10.0, -10.0, 0.0), Vec3::new(10.0, 10.0, 10.0)));
    let m = camera.projection(width, height, lo, hi);
    // The box in pixels: the extent of its eight corners, or nothing when
    // one of them is behind the camera and has no pixel.
    let mut px: Option<[f64; 4]> = None;
    let mut all = true;
    for i in 0..8 {
        let c = Vec3::new(if i & 1 == 0 { lo.x } else { hi.x }, if i & 2 == 0 { lo.y } else { hi.y }, if i & 4 == 0 { lo.z } else { hi.z });
        match camera.project(c, width, height) {
            Some((x, y)) => {
                px = Some(match px {
                    None => [x, y, x, y],
                    Some(b) => [b[0].min(x), b[1].min(y), b[2].max(x), b[3].max(y)],
                })
            }
            None => all = false,
        }
    }
    pb::CameraInfo {
        index,
        t,
        width,
        height,
        eye: Some(camera.eye.to_proto()),
        target: Some(camera.target.to_proto()),
        up: Some(camera.up.to_proto()),
        fov_degrees: camera.fov_degrees,
        orthographic: camera.orthographic,
        ortho_height: m.ortho_height,
        view: m.view.to_vec(),
        projection: m.projection.to_vec(),
        near: m.near,
        far: m.far,
        ground_origin: Some(Vec3::new(0.0, 0.0, lo.z).to_proto()),
        ground_normal: Some(Vec3::Z.to_proto()),
        ground_u: Some(Vec3::X.to_proto()),
        ground_v: Some(Vec3::Y.to_proto()),
        bounds_min: Some(lo.to_proto()),
        bounds_max: Some(hi.to_proto()),
        bounds_px: if all { px.map(|b| b.to_vec()).unwrap_or_default() } else { Vec::new() },
    }
}


/// Draw a document as a 2D engineering drawing: the `Drawing` rpc.
///
/// Evaluation errors are diagnostics, not failures — the same contract as
/// `Render` and `Export` — so a broken script comes back with its message and
/// an empty `data` rather than a stack trace.
pub fn drawing(req: &pb::DrawingRequest) -> anyhow::Result<pb::DrawingResponse> {
    let doc = req.document.as_ref().ok_or_else(|| anyhow::anyhow!("drawing: no document"))?;
    let extra = overrides_from_proto(&req.param_overrides);
    if let Some(which) = sheet_view(&req.view) {
        return drawing_sheet(doc, &extra, req, which);
    }
    // A `look` or a `section` asks for a page: the colour look
    // unless `look` says "lines". Neither is the drawing as it always was.
    let page = !req.look.trim().is_empty() || !req.section.trim().is_empty();
    let look = if req.look.trim().is_empty() {
        cadpole_drawing::Look::Color
    } else {
        cadpole_drawing::Look::parse(&req.look)
            .ok_or_else(|| anyhow::anyhow!("drawing: look {:?} must be \"lines\" or \"color\"", req.look))?
    };
    if page {
        // Refuse a bad view or cut before spending an evaluation on it.
        cadpole_drawing::plate::PlateView::parse(&req.view, &req.section).map_err(|e| anyhow::anyhow!("drawing: {e}"))?;
    }
    let (mut ctx, _) = evaluate_document_with(doc, &extra, req.tolerance, 0.0);
    let diagnostics: Vec<pb::Diagnostic> = ctx.diagnostics.iter().map(Diagnostic::to_proto).collect();
    if diagnostics.iter().any(|d| d.severity() == pb::Severity::Error) {
        return Ok(pb::DrawingResponse { diagnostics, ..Default::default() });
    }
    if page {
        return drawing_page(&mut ctx, doc, req, look, diagnostics);
    }
    let mut opts = cadpole_drawing::DrawingOptions::parse(
        &req.view,
        &req.scale,
        &req.sheet,
        &req.dimensions,
        &req.hidden,
        &req.units,
    )
    .map_err(|e| anyhow::anyhow!(e))?;
    opts.title = if req.title.is_empty() { doc.name.clone() } else { req.title.clone() };
    opts.project = doc.name.clone();

    let format = if req.format.trim().is_empty() { "svg".to_owned() } else { req.format.trim().to_ascii_lowercase() };
    let (mesh, topo) =
        cadpole_drawing::model::combined_mesh(&mut ctx, &req.body_ids).map_err(|e| anyhow::anyhow!(e))?;
    let d = cadpole_drawing::model::build(&mesh, &topo, &opts).map_err(|e| anyhow::anyhow!(e))?;
    let data = cadpole_drawing_out::write(&d, &format).map_err(|e| anyhow::anyhow!(e))?;
    let stem = if doc.name.is_empty() { "drawing".to_owned() } else { doc.name.clone() };
    Ok(pb::DrawingResponse {
        data,
        format: format.clone(),
        suggested_filename: format!("{stem}.{format}"),
        scale: d.scale.label(&opts.unit_format),
        items: cadpole_drawing::model::item_count(&d),
        notes: d.notes.clone(),
        diagnostics,
    })
}

/// The `Drawing` rpc's page: one plate -- in the colour look the
/// view painted in the bodies' colours with the script's text labels as
/// text, in the line look cut, hatched and hidden-line-removed -- and the
/// title, legend, scale bar, north arrow and notes around it.
fn drawing_page(
    ctx: &mut Context,
    doc: &pb::Document,
    req: &pb::DrawingRequest,
    look: cadpole_drawing::Look,
    diagnostics: Vec<pb::Diagnostic>,
) -> anyhow::Result<pb::DrawingResponse> {
    // A script drawn in feet is drawn in millimetres, scaled once here as
    // `cadpole bake --model-unit` scales it; its section, crop and
    // label points are in its own units and scale with it.
    let model_unit = match req.model_unit.trim().to_ascii_lowercase().as_str() {
        "" => None,
        u => Some(cadpole_drawing::LengthUnit::parse(u).filter(|l| *l != cadpole_drawing::LengthUnit::FtIn).ok_or_else(|| {
            anyhow::anyhow!("drawing: model_unit {u:?} must be mm, cm, m, in or ft")
        })?),
    };
    let k = model_unit.map(|u| u.mm()).unwrap_or(1.0);
    let view = cadpole_drawing::plate::PlateView::parse(&req.view, &req.section).map_err(|e| anyhow::anyhow!("drawing: {e}"))?.scaled(k);
    // Only what the display layers leave visible, as a render shows it.
    let layers = cadpole_core::layers::resolve_implicit(&layer_overrides_from_proto(&req.layers), &ctx.layers);
    let ids: Vec<cadpole_core::BodyId> = ctx
        .bodies
        .iter()
        .filter(|b| b.visible && (req.body_ids.is_empty() || req.body_ids.contains(&b.id_string())))
        .filter(|b| cadpole_core::layers::effective(&b.layer, &ctx.layers, &layers).0)
        .map(|b| b.id)
        .collect();
    if ids.is_empty() {
        anyhow::bail!("drawing: no visible bodies to draw");
    }
    let mut bodies = Vec::with_capacity(ids.len());
    for id in ids {
        let mut b = cadpole_drawing::painted::view_body(ctx, id, None).map_err(|e| anyhow::anyhow!("drawing: {e}"))?;
        let rec = ctx.body(id)?;
        let (_, layer_opacity) = cadpole_core::layers::effective(&rec.layer, &ctx.layers, &layers);
        b.opacity *= layer_opacity;
        if k != 1.0 {
            b.scale(k);
        }
        bodies.push(b);
    }
    let parsed = cadpole_drawing::DrawingOptions::parse("", &req.scale, "", "", "", &req.units).map_err(|e| anyhow::anyhow!(e))?;
    let unit_format = if req.units.trim().is_empty() {
        document_unit_format(ctx).unwrap_or(if model_unit.is_some_and(|u| u.imperial()) {
            cadpole_drawing::UnitFormat::ft_in()
        } else {
            parsed.unit_format
        })
    } else {
        parsed.unit_format
    };
    let sheet = if req.sheet.trim().is_empty() {
        None
    } else {
        Some(cadpole_drawing::DrawingOptions::parse("", "", &req.sheet, "", "", "").map_err(|e| anyhow::anyhow!(e))?.sheet)
    };
    let north = if req.north_degrees != 0.0 {
        req.north_degrees
    } else {
        ctx.render_defaults.as_ref().and_then(|r| r.north_degrees).unwrap_or(0.0)
    };
    let opts = cadpole_drawing::plate::PlateOptions {
        sheet,
        scale: parsed.scale,
        title: if req.title.is_empty() { doc.name.clone() } else { req.title.clone() },
        subtitle: req.subtitle.clone(),
        note: req.note.clone(),
        legend: req.legend.iter().map(|s| cadpole_drawing::plate::parse_legend(s)).collect::<Result<_, _>>().map_err(|e| anyhow::anyhow!("drawing: {e}"))?,
        labels: req
            .labels
            .iter()
            .map(|s| cadpole_drawing::plate::parse_label(s).map(|(p, t)| (p * k, t)))
            .collect::<Result<_, _>>()
            .map_err(|e| anyhow::anyhow!("drawing: {e}"))?,
        north: view.is_plan().then_some(north),
        scale_bar: true,
        unit_format,
        look,
        crop: if req.crop.trim().is_empty() {
            None
        } else {
            let (a, b) = cadpole_drawing::plate::parse_crop(&req.crop).map_err(|e| anyhow::anyhow!("drawing: {e}"))?;
            Some((cadpole_core::sketch::P2::new(a.x * k, a.y * k), cadpole_core::sketch::P2::new(b.x * k, b.y * k)))
        },
    };
    let d = cadpole_drawing::plate::plate(&bodies, &view, &opts).map_err(|e| anyhow::anyhow!("drawing: {e}"))?;
    let format = if req.format.trim().is_empty() { "svg".to_owned() } else { req.format.trim().to_ascii_lowercase() };
    let data = cadpole_drawing_out::write(&d, &format).map_err(|e| anyhow::anyhow!(e))?;
    let stem = if doc.name.is_empty() { "drawing".to_owned() } else { doc.name.clone() };
    Ok(pb::DrawingResponse {
        data,
        format: format.clone(),
        suggested_filename: format!("{stem}.{format}"),
        scale: d.scale.label(&unit_format),
        items: cadpole_drawing::model::item_count(&d),
        notes: d.notes.clone(),
        diagnostics,
    })
}

/// The document's `units(...)` as a drawing writes lengths.
fn document_unit_format(ctx: &Context) -> Option<cadpole_drawing::UnitFormat> {
    let u = ctx.display_units.as_ref()?;
    let unit = cadpole_drawing::LengthUnit::parse(&u.unit)?;
    let mut f = if unit.imperial() { cadpole_drawing::UnitFormat::ft_in() } else { cadpole_drawing::UnitFormat::mm() };
    f.unit = unit;
    if unit.imperial() && u.fraction > 0 {
        f.denominator = u.fraction;
    }
    Some(f)
}

/// A warning raised while writing a file rather than while evaluating the
/// script, so it has no span: a filament slot pinned twice, a palette wider
/// than the machine. `export_3mf` returns these and every front-end reads
/// `ExportResponse.diagnostics`, so this is how they reach a person.
fn export_warning(message: String) -> pb::Diagnostic {
    pb::Diagnostic { severity: pb::Severity::Warning as i32, message, ..Default::default() }
}

/// `ExportRequest.template`: the saved Bambu Studio project the
/// request names, read from the document's asset table, as the script's
/// `export(..., template=)` reads it.
fn request_template(req: &pb::ExportRequest, name: &str) -> anyhow::Result<cadpole_core::context::BambuTemplate> {
    let doc = req.document.as_ref().ok_or_else(|| anyhow::anyhow!("export: no document"))?;
    let store = assets_from_proto(doc, &HashMap::new());
    let Some((record, bytes)) = store.get(name) else {
        let have = store.names();
        anyhow::bail!(
            "export: template {name:?} is not one of the document's assets (available: {})",
            if have.is_empty() { "none".to_owned() } else { have.join(", ") }
        );
    };
    let (application, settings) = cadpole_export_3mf::bambu_settings::template_parts(&bytes)
        .map_err(|e| anyhow::anyhow!("export: template {name:?} is not a Bambu Studio project: {e}"))?;
    cadpole_export_3mf::bambu_settings::Template::from_parts(&application, &settings)
        .map_err(|e| anyhow::anyhow!("export: template {name:?}: {e}"))?;
    Ok(cadpole_core::context::BambuTemplate { asset: name.to_owned(), sha256: record.sha256, application, settings })
}

/// One body's line of an export report: what `Context` kept beside
/// the mesh it just made — the tolerance and where it came from, the
/// tolerance it was made at (always the requested one), the chord
/// error measured. `exact` is
/// STEP, which writes surfaces and has none of that to report.
fn export_body_report(
    ctx: &Context,
    id: cadpole_core::BodyId,
    call: Option<f64>,
    exact: bool,
    triangles: usize,
) -> anyhow::Result<pb::ExportBodyReport> {
    let b = ctx.body(id)?;
    let (options, source) = ctx.tessellation_for(id, call)?;
    let q = ctx.mesh_quality(id)?;
    Ok(pb::ExportBodyReport {
        body_id: b.id_string(),
        name: b.name.clone(),
        triangles: triangles as u32,
        tolerance: if exact { 0.0 } else { options.tolerance },
        tolerance_source: if exact { String::new() } else { source.as_str().to_owned() },
        tessellated_at: if exact { 0.0 } else { q.as_ref().map(|q| q.tessellated_at).unwrap_or(options.tolerance) },
        chord_error: if exact { None } else { q.as_ref().and_then(|q| q.chord_error()) },
        chord_error_face: if exact { String::new() } else { q.as_ref().map(|q| q.chord_face.clone()).unwrap_or_default() },
        chord_error_at: if exact { None } else { q.and_then(|q| q.chord.map(|c| c.at.to_proto())) },
        exact,
    })
}

/// The chord-error-vs-gap warnings for the meshes as they stand (
/// `cadpole_print::chord`): one per body whose triangles sit farther off its
/// exact surfaces than a third of its gap to another body, on that body's
/// feature. Judged on the meshes the export just made, so a call-level
/// tolerance is judged at that tolerance. `seen` keeps a warning the
/// whole-document export already raised from being raised again by a script
/// export of the same meshes.
fn chord_warnings(ctx: &mut Context, seen: &mut std::collections::HashSet<String>) -> anyhow::Result<Vec<pb::Diagnostic>> {
    let fraction = cadpole_print::Profile::default().chord_gap_fraction;
    let mut out = Vec::new();
    for f in cadpole_print::chord::chord_findings(ctx, fraction)? {
        let (feature, id) = {
            let b = ctx.body(f.body)?;
            (b.feature.clone(), b.id_string())
        };
        let message = format!("export: \"{}\" ({id}): {}", f.body_name, f.message());
        if !seen.insert(message.clone()) {
            continue;
        }
        let span = ctx.feature(&feature).ok().and_then(|f| f.span.clone());
        out.push(Diagnostic::warning(message).with_feature(feature).with_span(span).to_proto());
    }
    Ok(out)
}

pub fn export(req: &pb::ExportRequest) -> anyhow::Result<pb::ExportResponse> {
    let doc = req.document.as_ref().ok_or_else(|| anyhow::anyhow!("export: no document"))?;
    let input = eval_input(doc, &HashMap::new(), req.tolerance, req.boolean_tolerance, &HashMap::new(), limits::default_limits());
    // A script's `sweep_table()` rows cost one evaluation per configuration,
    // worth paying only when its own files are written.
    let ev = if runs_script_exports(req) {
        cadpole_script::evaluate_with_sweeps(input)
    } else {
        cadpole_script::evaluate_full(input)
    };
    let mut ctx = ev.ctx;
    let mut drawn = ev.files;
    let failed = ctx.diagnostics.iter().any(|d| d.severity == cadpole_core::context::Severity::Error);
    if runs_script_exports(req) && !ev.sheet_exports.is_empty() && !failed {
        // The sheet sets are drawn and written here, after evaluation, and
        // only for an export: the pages are the script's, the bytes the host's.
        match cadpole_script::sheet_files(&ev.sheet_sets, &ev.sheet_exports) {
            Ok(files) => drawn.extend(files),
            Err(e) => ctx.diagnostics.push(Diagnostic::error(e)),
        }
    }
    export_evaluated(&mut ctx, &drawn, &doc.name, req)
}

/// `view="sheet:A2.0"` asks for one sheet of the script's sheet sets and
/// `view="sheets"` for every sheet as one PDF; anything else is a
/// view of the model. The number the sheet was drawn under, whatever its
/// case.
fn sheet_view(view: &str) -> Option<&str> {
    let v = view.trim();
    if v.eq_ignore_ascii_case("sheets") {
        return Some("");
    }
    v.strip_prefix("sheet:").or_else(|| v.strip_prefix("SHEET:")).map(str::trim)
}

/// The `Drawing` rpc for a sheet of a `sheet_set()`: the document evaluated
/// with its `sweep_table()` rows filled in, as the export would, and the one
/// page written in the asked-for format -- a preview of exactly what
/// `export(set, "x.pdf")` puts on that page.
fn drawing_sheet(
    doc: &pb::Document,
    extra: &HashMap<String, ParamValue>,
    req: &pb::DrawingRequest,
    which: &str,
) -> anyhow::Result<pb::DrawingResponse> {
    let input = eval_input(doc, extra, req.tolerance, 0.0, &HashMap::new(), limits::default_limits());
    let ev = cadpole_script::evaluate_with_sweeps(input);
    let diagnostics: Vec<pb::Diagnostic> = ev.ctx.diagnostics.iter().map(Diagnostic::to_proto).collect();
    if diagnostics.iter().any(|d| d.severity() == pb::Severity::Error) {
        return Ok(pb::DrawingResponse { diagnostics, ..Default::default() });
    }
    let numbers: Vec<String> = ev.sheet_sets.iter().flat_map(|s| s.sheets.iter().map(|p| p.number.clone())).collect();
    if numbers.is_empty() {
        anyhow::bail!("drawing: view {:?} draws a sheet of the script's sheet_set(), and this document has no sheets", req.view);
    }
    let format = if req.format.trim().is_empty() { "svg".to_owned() } else { req.format.trim().to_ascii_lowercase() };
    let stem = if doc.name.is_empty() { "drawing".to_owned() } else { doc.name.clone() };
    if which.is_empty() {
        // Every sheet of every set, in order: only a PDF has pages.
        if format != "pdf" {
            anyhow::bail!("drawing: view \"sheets\" is every sheet as one PDF; pass --format pdf, or view \"sheet:<number>\" for one sheet as {format}");
        }
        let pages: Vec<cadpole_drawing::Drawing> = ev.sheet_sets.iter().flat_map(|s| s.pages()).collect();
        let refs: Vec<&cadpole_drawing::Drawing> = pages.iter().collect();
        let opts = cadpole_drawing_out::PdfOptions {
            title: ev.sheet_sets.first().map(|s| s.name.clone()).filter(|n| !n.is_empty()),
            ..Default::default()
        };
        let data = cadpole_drawing_out::write_pdf_pages(&refs, &opts);
        return Ok(pb::DrawingResponse {
            data,
            format,
            suggested_filename: format!("{stem}.pdf"),
            scale: "AS NOTED".to_owned(),
            items: pages.iter().map(|p| p.flatten().len() as u32).sum(),
            notes: Vec::new(),
            diagnostics,
        });
    }
    let found = ev
        .sheet_sets
        .iter()
        .find_map(|set| set.find(which).map(|i| (set, i)));
    let Some((set, i)) = found else {
        anyhow::bail!("drawing: no sheet {which:?} in this document's sheet sets (it has {})", numbers.join(", "));
    };
    let files = cadpole_drawing_out::write_set(set, &format!("sheet.{format}"), &format, false, Some(i)).map_err(|e| anyhow::anyhow!("drawing: {e}"))?;
    let data = files.into_iter().next().map(|(_, d)| d).unwrap_or_default();
    let page = set.page(i).ok_or_else(|| anyhow::anyhow!("drawing: sheet {which:?} vanished"))?;
    let sheet = &set.sheets[i];
    Ok(pb::DrawingResponse {
        data,
        format: format.clone(),
        suggested_filename: format!("{stem}_{}.{format}", sheet.number.replace(['/', '\\', ' '], "_")),
        scale: match sheet.scale {
            Some(sc) => sc.label(&set.unit_format),
            None => "AS NOTED".to_owned(),
        },
        items: page.flatten().len() as u32,
        notes: Vec::new(),
        diagnostics,
    })
}

/// The closed-mesh gate at the export: a mesh handed to a slicer format is a closed,
/// consistently oriented surface, or the file is not written and the
/// diagnostic says where the hole is.
///
/// **What is checked, and what is not.** The topology half of
/// [`cadpole_core::solid::validate`] — closed, no non-manifold edge, no edge
/// whose triangles are wound the same way, positive volume — and not the
/// pairwise half. A slicer cannot slice an open or non-manifold surface at
/// all: "inside" has no answer, and the layer stops being a set of closed
/// loops. Triangles that *cross* each other are a different matter: every
/// slicer resolves them by winding, the field path's fillet band produces a
/// few by construction (`examples/filleted_bracket.star`), and
/// refusing them would take away an export people rely on. `cadpole check`
/// reports those, at error level, which is where that argument belongs.
///
/// The tessellator ([`cadpole_core::kernel::tessellate_reporting`])
/// refuses to return an open B-rep mesh and the mesh kernel refuses to import
/// or return one, so this is the last line rather than the first: what reaches
/// it is a mesh body made some other way, or a regression in either. It costs
/// one weld and one edge count per body, and it turns "your slicer
/// auto-repaired this part" into a diagnostic with a position. Before it
/// existed, `cadpole export --tolerance 0.01 examples/showcase.star` wrote an STL
/// with six unbalanced edges and said `wrote`.
fn refuse_open(meshes: &[(String, cadpole_core::tessellate::Mesh)]) -> Vec<pb::Diagnostic> {
    let mut out = Vec::new();
    for (name, mesh) in meshes {
        // `pair_limit: 0` skips the crossing and doubled-face search, which
        // is the half this gate deliberately does not judge (and the half
        // that costs anything).
        let opts = cadpole_core::solid::SolidOptions { pair_limit: 0, ..Default::default() };
        let welded = mesh.welded(opts.weld_eps);
        let collapsed = mesh.triangle_count().saturating_sub(welded.triangle_count());
        let report = cadpole_core::solid::validate_welded(&welded, collapsed, &opts);
        if report.oriented() {
            continue;
        }
        let holes = cadpole_print::mesh_checks::holes(&welded, &report.boundary);
        let where_ = match holes.first() {
            Some(h) => format!(
                " The first is a hole at ({:.2}, {:.2}, {:.2}), {} boundary edge{}, {:.2} mm around{}.",
                h.center.x,
                h.center.y,
                h.center.z,
                h.edges,
                if h.edges == 1 { "" } else { "s" },
                h.perimeter,
                match holes.len() {
                    1 => String::new(),
                    n => format!(", and there are {} more", n - 1),
                }
            ),
            None => String::new(),
        };
        out.push(
            Diagnostic::error(format!(
                "export refused: \"{name}\" is not a closed, consistently oriented surface, so a \
                 slicer would have to guess which side of it is inside — {}.{where_} `cadpole check` \
                 lists every one with its position. On a B-rep body a different --tolerance often \
                 closes a small hole, and `kernel=\"mesh\"` on the last boolean gives a body that \
                 is closed by construction",
                report.problems().join("; ")
            ))
            .to_proto(),
        );
    }
    out
}

/// Whether an export writes the script's own `export(...)` calls:
/// `script_exports_only` asks for them and nothing else.
fn runs_script_exports(req: &pb::ExportRequest) -> bool {
    req.run_script_exports || req.script_exports_only
}

/// [`export`] after evaluation: from the diagnostics gate to the bytes. Split
/// out so a test can hand it a `Context` holding a body no script can make —
/// a mesh with a triangle missing — and watch [`refuse_open`] refuse it.
fn export_evaluated(
    ctx: &mut Context,
    drawn: &[cadpole_script::OutputFile],
    doc_name: &str,
    req: &pb::ExportRequest,
) -> anyhow::Result<pb::ExportResponse> {
    let mut diagnostics: Vec<pb::Diagnostic> = ctx.diagnostics.iter().map(Diagnostic::to_proto).collect();
    if diagnostics.iter().any(|d| d.severity() == pb::Severity::Error) {
        return Ok(pb::ExportResponse { diagnostics, ..Default::default() });
    }
    // The script's own `export(...)` calls, when asked for. These are separate
    // from the whole-document export below: the script names both the format
    // and the path.
    let mut files = Vec::new();
    let mut warned = std::collections::HashSet::new();
    if runs_script_exports(req) {
        // `export_svg` / `export_dxf` already produced their bytes during
        // evaluation; `export(...)` recorded body ids for us to mesh here.
        for f in drawn {
            files.push(pb::ExportFile { path: f.path.clone(), data: f.data.clone(), bodies: Vec::new(), ..Default::default() });
        }
        let specs = ctx.exports.clone();
        // Bambu projects already written: every call to one path is one file.
        let mut projects: Vec<String> = Vec::new();
        for spec in &specs {
            if spec.format == "3mf" && spec.flavor == cadpole_core::export::Flavor::Bambu {
                // A Bambu Studio project: every `export()` call to
                // this path, written once, where the first of them stands.
                if projects.contains(&spec.path) {
                    continue;
                }
                projects.push(spec.path.clone());
                let group: Vec<cadpole_core::context::ExportSpec> =
                    specs.iter().filter(|s| s.path == spec.path).cloned().collect();
                if group.iter().any(|s| s.flavor != cadpole_core::export::Flavor::Bambu) {
                    diagnostics.push(
                        Diagnostic::error(format!(
                            "export: {:?} is written as a Bambu Studio project by one export() call and as a \
                             standard 3MF by another; give every call to it flavor=flavor.bambu, or the \
                             standard one another file name",
                            spec.path
                        ))
                        .to_proto(),
                    );
                    return Ok(pb::ExportResponse { diagnostics, ..Default::default() });
                }
                let mut meshes = Vec::new();
                let mut bodies = Vec::new();
                for s in &group {
                    for id in &s.bodies {
                        let name = ctx.body(*id)?.name.clone();
                        let mesh = ctx.body_mesh_with(*id, s.tolerance)?.0.clone();
                        let n = mesh.triangle_count();
                        meshes.push((name, mesh));
                        bodies.push(export_body_report(ctx, *id, s.tolerance, false, n)?);
                    }
                }
                let refused = refuse_open(&meshes);
                if !refused.is_empty() {
                    diagnostics.extend(refused);
                    return Ok(pb::ExportResponse { diagnostics, ..Default::default() });
                }
                diagnostics.extend(chord_warnings(ctx, &mut warned)?);
                let stem = std::path::Path::new(&spec.path)
                    .file_stem()
                    .map(|s| s.to_string_lossy().into_owned())
                    .unwrap_or_default();
                // The plate images: the script's own `plate_images(...)`
                // declaration for this project, else the request's ask.
                let images = match ctx.plate_images.iter().find(|p| p.path == spec.path) {
                    Some(decl) => Some(print::PlateImageOptions::from_spec(decl)?),
                    None if req.plate_images => Some(print::PlateImageOptions::from_request(req)?),
                    None => None,
                };
                let out = match print::export_bambu_project(ctx, &group, &stem, true, images.as_ref()) {
                    Ok(out) => out,
                    Err(e) => {
                        // A plate whose pieces cannot fit: nothing is
                        // written, and the error names every such plate.
                        diagnostics.push(Diagnostic::error(e.to_string()).to_proto());
                        return Ok(pb::ExportResponse { diagnostics, ..Default::default() });
                    }
                };
                diagnostics.extend(out.warnings.into_iter().map(export_warning));
                files.push(pb::ExportFile {
                    path: spec.path.clone(),
                    data: out.bytes,
                    bodies,
                    plate_images: out.plate_images.iter().map(print::PlateImage::to_proto).collect(),
                    plate_sheet: out.plate_sheet.as_ref().map(|s| s.image.to_png()).unwrap_or_default(),
                });
                continue;
            }
            let exact = spec.format == "step";
            let mut meshes = Vec::new();
            let mut bodies = Vec::new();
            for id in &spec.bodies {
                let name = ctx.body(*id)?.name.clone();
                // The mesh at this call's tolerance for a body without its
                // own; STEP writes surfaces and needs no mesh at all.
                let triangles = if exact {
                    0
                } else {
                    let mesh = ctx.body_mesh_with(*id, spec.tolerance)?.0.clone();
                    let n = mesh.triangle_count();
                    meshes.push((name.clone(), mesh));
                    n
                };
                bodies.push(export_body_report(ctx, *id, spec.tolerance, exact, triangles)?);
            }
            if matches!(spec.format.as_str(), "stl" | "obj" | "3mf") {
                let refused = refuse_open(&meshes);
                if !refused.is_empty() {
                    diagnostics.extend(refused);
                    return Ok(pb::ExportResponse { diagnostics, ..Default::default() });
                }
                diagnostics.extend(chord_warnings(ctx, &mut warned)?);
            }
            let data = match spec.format.as_str() {
                "stl" => cadpole_core::export::stl(&meshes.iter().map(|(_, m)| m).collect::<Vec<_>>()),
                "3mf" => {
                    let stem = std::path::Path::new(&spec.path)
                        .file_stem()
                        .map(|s| s.to_string_lossy().into_owned())
                        .unwrap_or_default();
                    let (bytes, warnings) =
                        print::export_3mf(ctx, &spec.bodies, &stem, &req.slicer, &req.group, spec.name.as_deref())?;
                    diagnostics.extend(warnings.into_iter().map(export_warning));
                    bytes
                }
                "step" => {
                    let stem = std::path::Path::new(&spec.path)
                        .file_stem()
                        .map(|s| s.to_string_lossy().into_owned())
                        .unwrap_or_default();
                    step::export_step(ctx, &spec.bodies, &stem, req.tolerance)?
                }
                "obj" => cadpole_core::export::obj(&meshes.iter().map(|(n, m)| (n.as_str(), m)).collect::<Vec<_>>()).into_bytes(),
                // Never a fallback format: `export(b, "part.step")` must not
                // write an OBJ called .step.
                other => anyhow::bail!("export: unsupported format {other:?}"),
            };
            files.push(pb::ExportFile { path: spec.path.clone(), data, bodies, ..Default::default() });
        }
    }
    if req.script_exports_only {
        // No whole-document export was asked for, so no body is meshed or
        // judged for one: a drawing set's export checks nothing it does not
        // write, and a model of two thousand bodies is not meshed
        // for an STL nobody will read.
        return Ok(pb::ExportResponse { files, diagnostics, ..Default::default() });
    }
    let gltf_format = matches!(req.format(), pb::ExportFormat::Glb | pb::ExportFormat::Gltf);
    // `model_unit` and `units` say how a glTF is scaled and labelled; every
    // other format is written in the millimetres of the script as it always
    // was, so asking one of them for feet is refused rather than ignored.
    if !gltf_format {
        let mm = gltf::mm_per_unit(&req.model_unit).map_err(|e| anyhow::anyhow!("export: model_unit: {e}"))?;
        if mm != 1.0 || !req.units.trim().is_empty() {
            anyhow::bail!(
                "export: model_unit and units apply to glTF (glb, gltf) only; {:?} is written in the \
                 script's own units, as before",
                req.format()
            );
        }
    }
    // The whole-document export writes what a viewer shows: the
    // bodies not hidden one by one, and not on a layer the script declares
    // hidden, unless `include_hidden_layers`; bodies named in `body_ids`
    // (by id or name) and every body on a layer in `layer_names`
    // are written whatever their layer.
    let picked = ctx
        .select_bodies(&req.body_ids, &req.layer_names, req.include_hidden_layers)
        .map_err(|e| anyhow::anyhow!("export: {e}"))?;
    let on_hidden_layers = picked.on_hidden_layers;
    let ids: Vec<_> = picked.ids.iter().filter_map(|id| ctx.body(*id).ok().map(|b| (b.id, b.name.clone()))).collect();
    if ids.is_empty() {
        if !files.is_empty() {
            return Ok(pb::ExportResponse { files, diagnostics, ..Default::default() });
        }
        if on_hidden_layers > 0 {
            anyhow::bail!(
                "export: no visible bodies: all {on_hidden_layers} are on hidden layers (include_hidden_layers, \
                 `cadpole export --include-hidden`, writes them)"
            );
        }
        anyhow::bail!("export: no visible bodies");
    }
    let exact = req.format() == pb::ExportFormat::Step;
    let mut meshes: Vec<(String, cadpole_core::tessellate::Mesh)> = Vec::new();
    let mut bodies = Vec::new();
    for (id, name) in &ids {
        // Each body at its own tolerance or the document's: the
        // whole-document export has no call level.
        let triangles = if exact {
            0
        } else {
            let mesh = ctx.body_mesh(*id)?.0.clone();
            let n = mesh.triangle_count();
            meshes.push((name.clone(), mesh));
            n
        };
        bodies.push(export_body_report(ctx, *id, None, exact, triangles)?);
    }
    if matches!(
        req.format(),
        pb::ExportFormat::Stl | pb::ExportFormat::Unspecified | pb::ExportFormat::Obj | pb::ExportFormat::Threemf
    ) {
        let refused = refuse_open(&meshes);
        if !refused.is_empty() {
            diagnostics.extend(refused);
            return Ok(pb::ExportResponse { files, diagnostics, ..Default::default() });
        }
        diagnostics.extend(chord_warnings(ctx, &mut warned)?);
    }
    let stem = if doc_name.is_empty() { "model".to_owned() } else { doc_name.to_owned() };
    // The standalone plate images of a Bambu project, when asked for.
    let (mut plate_images, mut plate_sheet) = (Vec::new(), Vec::new());
    let (data, ext) = match req.format() {
        pb::ExportFormat::Stl | pb::ExportFormat::Unspecified => {
            let refs: Vec<&cadpole_core::tessellate::Mesh> = meshes.iter().map(|(_, m)| m).collect();
            (cadpole_core::export::stl(&refs), "stl")
        }
        pb::ExportFormat::Obj => {
            // `o <body>` and `g <layer>`: layer membership where OBJ has a
            // place for it.
            let mut layers = Vec::with_capacity(ids.len());
            for (id, _) in &ids {
                let layer = ctx.body(*id)?.layer.clone();
                let hidden = gltf::layer_hidden(ctx, &layer);
                layers.push((layer, hidden));
            }
            let bodies: Vec<cadpole_core::export::ObjBody<'_>> = meshes
                .iter()
                .zip(&layers)
                .map(|((n, m), (layer, hidden))| cadpole_core::export::ObjBody { name: n, layer, hidden: *hidden, mesh: m })
                .collect();
            (cadpole_core::export::obj_layered(&bodies).into_bytes(), "obj")
        }
        f @ (pb::ExportFormat::Glb | pb::ExportFormat::Gltf) => {
            let binary = f == pb::ExportFormat::Glb;
            let bodies: Vec<(cadpole_core::BodyId, cadpole_core::tessellate::Mesh)> =
                ids.iter().map(|(id, _)| *id).zip(meshes.into_iter().map(|(_, m)| m)).collect();
            (gltf::write(ctx, &bodies, &stem, req, binary)?, if binary { "glb" } else { "gltf" })
        }
        pb::ExportFormat::Threemf => {
            let ids: Vec<_> = ids.iter().map(|(id, _)| *id).collect();
            let flavor = cadpole_core::export::Flavor::parse(&req.flavor).map_err(|e| anyhow::anyhow!("export: {e}"))?;
            let (bytes, warnings) = match flavor {
                cadpole_core::export::Flavor::Standard => {
                    print::export_3mf(ctx, &ids, &stem, &req.slicer, &req.group, None)?
                }
                cadpole_core::export::Flavor::Bambu => {
                    // The whole document as one plate: `--group part` makes
                    // it one object named after the document, `objects` one
                    // object per body.
                    let group = cadpole_export_3mf::Grouping::parse(&req.group).map_err(|e| anyhow::anyhow!(e))?;
                    let printer = match req.printer.trim() {
                        "" => String::new(),
                        p => cadpole_print::printers::printer(p)
                            .map(|known| known.id.to_owned())
                            .ok_or_else(|| {
                                anyhow::anyhow!("export: unknown printer {p:?}; valid: {}", cadpole_print::printers::printer_list())
                            })?,
                    };
                    let bed = match req.bed.as_slice() {
                        [] => None,
                        [x, y] if *x > 0.0 && *y > 0.0 => Some([*x, *y, 0.0]),
                        [x, y, z] if *x > 0.0 && *y > 0.0 && *z > 0.0 => Some([*x, *y, *z]),
                        other => anyhow::bail!("export: bed takes x,y or x,y,z in mm, every one positive; got {other:?}"),
                    };
                    let template = match req.template.trim() {
                        "" => None,
                        name => Some(request_template(req, name)?),
                    };
                    let spec = cadpole_core::context::ExportSpec {
                        bodies: ids,
                        path: format!("{stem}.3mf"),
                        format: "3mf".to_owned(),
                        tolerance: None,
                        flavor,
                        plate: Some(cadpole_core::context::PlateRef::Number(req.plate.max(1))),
                        name: (group == cadpole_export_3mf::Grouping::Part).then(|| stem.clone()),
                        extruders: Vec::new(),
                        printer,
                        bed,
                        template,
                    };
                    let images = req.plate_images.then(|| print::PlateImageOptions::from_request(req)).transpose()?;
                    let out = match print::export_bambu_project(ctx, &[spec], &stem, true, images.as_ref()) {
                        Ok(out) => out,
                        Err(e) => {
                            diagnostics.push(Diagnostic::error(e.to_string()).to_proto());
                            return Ok(pb::ExportResponse { diagnostics, ..Default::default() });
                        }
                    };
                    plate_images = out.plate_images.iter().map(print::PlateImage::to_proto).collect();
                    plate_sheet = out.plate_sheet.as_ref().map(|s| s.image.to_png()).unwrap_or_default();
                    (out.bytes, out.warnings)
                }
            };
            diagnostics.extend(warnings.into_iter().map(export_warning));
            (bytes, "3mf")
        }
        pb::ExportFormat::Step => {
            let ids: Vec<_> = ids.iter().map(|(id, _)| *id).collect();
            (step::export_step(ctx, &ids, &stem, req.tolerance)?, "step")
        }
        // The 2D formats export a drawing of the model, not a mesh of it.
        f @ (pb::ExportFormat::Svg | pb::ExportFormat::Pdf | pb::ExportFormat::Dxf) => {
            let name = match f {
                pb::ExportFormat::Svg => "svg",
                pb::ExportFormat::Pdf => "pdf",
                _ => "dxf",
            };
            let opts = cadpole_drawing::DrawingOptions {
                title: doc_name.to_owned(),
                project: doc_name.to_owned(),
                ..Default::default()
            };
            // A drawing keeps its own rule for what it draws when nothing is
            // named; `--body` / `--layer` name
            // the bodies exactly, by id.
            let named: Vec<String> = if req.body_ids.is_empty() && req.layer_names.is_empty() {
                Vec::new()
            } else {
                ids.iter().filter_map(|(id, _)| ctx.body(*id).ok().map(|b| b.id_string())).collect()
            };
            let (mesh, topo) = cadpole_drawing::model::combined_mesh(ctx, &named)
                .map_err(|e| anyhow::anyhow!(e))?;
            let d = cadpole_drawing::model::build(&mesh, &topo, &opts)
                .map_err(|e| anyhow::anyhow!(e))?;
            (cadpole_drawing_out::write(&d, name).map_err(|e| anyhow::anyhow!(e))?, name)
        }
    };
    Ok(pb::ExportResponse {
        data,
        suggested_filename: format!("{stem}.{ext}"),
        files,
        diagnostics,
        bodies,
        plate_images,
        plate_sheet,
    })
}

/// The single entry point used by wasm, the FFI and the server.
pub fn call(request: &[u8]) -> Vec<u8> {
    let resp = match pb::Request::decode(request) {
        Err(e) => pb::Response { response: Some(pb::response::Response::Error(format!("bad request: {e}"))) },
        Ok(req) => dispatch(req),
    };
    resp.encode_to_vec()
}

pub fn dispatch(req: pb::Request) -> pb::Response {
    use pb::request::Request as R;
    use pb::response::Response as S;
    let response = match req.request {
        None => S::Error("empty request".into()),
        Some(R::Evaluate(e)) => S::Evaluate(evaluate(&e)),
        Some(R::Export(e)) => match export(&e) {
            Ok(r) => S::Export(r),
            Err(e) => S::Error(e.to_string()),
        },
        Some(R::ApiDoc(_)) => S::ApiDoc(cadpole_script::api_doc()),
        Some(R::Format(f)) => S::Format(pb::FormatResponse { source: f.source }),
        Some(R::Render(r)) => match render(&r) {
            Ok(r) => S::Render(r),
            Err(e) => S::Error(e.to_string()),
        },
    };
    pb::Response { response: Some(response) }
}

/// Parse `name=value` overrides as used by the CLI and sweeps.
pub fn parse_override(s: &str) -> anyhow::Result<(String, ParamValue)> {
    let (k, v) = s.split_once('=').ok_or_else(|| anyhow::anyhow!("expected name=value, got {s:?}"))?;
    Ok((k.trim().to_owned(), parse_value(v.trim())))
}

pub fn parse_value(v: &str) -> ParamValue {
    if v == "true" {
        return ParamValue::Bool(true);
    }
    if v == "false" {
        return ParamValue::Bool(false);
    }
    if let Ok(n) = v.parse::<f64>() {
        return ParamValue::Number(n);
    }
    let t = v.trim_matches(|c| c == '(' || c == ')');
    let parts: Vec<_> = t.split(',').map(|p| p.trim().parse::<f64>()).collect();
    if parts.len() == 3 && parts.iter().all(|p| p.is_ok()) {
        let p: Vec<f64> = parts.into_iter().map(|p| p.unwrap()).collect();
        return ParamValue::Vector(Vec3::new(p[0], p[1], p[2]));
    }
    ParamValue::Text(v.to_owned())
}

/// How many cells [`sweep_grid`] would build for `specs`, from the value
/// counts alone, or `None` when the product does not fit in a `usize`.
/// A caller checks this against its cell limit *before* building the grid:
/// the grid is one `Vec` per cell, so a request of a few kilobytes (three
/// parameters of 300 values) would otherwise allocate gigabytes only to be
/// refused (docs/security.md, "Render and sweep limits").
pub fn sweep_cell_count(specs: &[(String, Vec<ParamValue>)]) -> Option<usize> {
    specs.iter().try_fold(1usize, |acc, (_, values)| acc.checked_mul(values.len()))
}

/// Cartesian product of `name=v1,v2,...` specs for sweeps, with the **first**
/// spec varying fastest.
///
/// That order is what makes a contact sheet a grid rather than a list. A sheet
/// is `cols` wide, and `cols` defaults to the number of values of the first
/// parameter, so varying the first parameter fastest puts it across the
/// columns and the second down the rows -- `width=1,2,3 x height=3,6` reads as
/// three widths across and two heights down. Varying the *last* parameter
/// fastest instead (the usual way to write a product) gives the cell order
/// (1,3) (1,6) (2,3) (2,6) (3,3) (3,6), which in a three-column sheet is a
/// jumble: row one would hold two different heights and two different widths.
///
/// Each combination still lists its parameters in `specs` order, so labels and
/// `SweepCell.params` are unaffected.
///
/// The grid is materialised: check [`sweep_cell_count`] against a limit
/// first. A product that overflows `usize` panics rather than wrapping to a
/// small (or empty) grid that would silently stand for the real one.
pub fn sweep_grid(specs: &[(String, Vec<ParamValue>)]) -> Vec<Vec<(String, ParamValue)>> {
    if specs.iter().any(|(_, values)| values.is_empty()) {
        return Vec::new();
    }
    let total = sweep_cell_count(specs).expect("sweep_grid: the cell count overflows; check sweep_cell_count first");
    let mut out = Vec::with_capacity(total);
    for n in 0..total {
        let mut combo = Vec::with_capacity(specs.len());
        let mut rest = n;
        for (name, values) in specs {
            combo.push((name.clone(), values[rest % values.len()].clone()));
            rest /= values.len();
        }
        out.push(combo);
    }
    out
}

pub fn value_label(v: &ParamValue) -> String {
    match v {
        ParamValue::Number(n) => cadpole_core::context::fmt_num(*n),
        ParamValue::Bool(b) => b.to_string(),
        ParamValue::Text(s) => s.clone(),
        ParamValue::Vector(v) => cadpole_core::context::fmt_vec(*v),
    }
}

#[cfg(test)]
mod render_tests;

#[cfg(test)]
mod tests {
    use super::*;

    fn doc(src: &str) -> pb::Document {
        document_from_scripts("t", vec![("main.star".into(), src.into())])
    }

    /// This engine is the one the wasm build and the C FFI compile,
    /// and it has no solver -- only the CLI installs HiGHS -- so `solve()` is
    /// one clear error that says where solving happens, while building the
    /// problem works and records its feature.
    #[test]
    fn without_a_solver_solve_says_it_needs_native_cadpole() {
        assert!(cadpole_core::opt::installed().is_none(), "nothing in this test binary installs a solver");
        let (ctx, _) = evaluate_document(
            &doc("p = opt.problem(name=\"plan\")\nx = p.var(\"x\", min=0, max=1)\np.constrain(x, max=0.5)\np.maximize(x)\nsol = p.solve()\n"),
            &HashMap::new(),
            0.0,
        );
        let errors: Vec<&str> =
            ctx.diagnostics.iter().filter(|d| d.severity == cadpole_core::context::Severity::Error).map(|d| d.message.as_str()).collect();
        assert_eq!(errors.len(), 1, "{errors:?}");
        assert!(errors[0].contains("plan: solve: solving needs native Cadpole: the CLI"), "{errors:?}");
        assert!(errors[0].contains("`cadpole serve`") && errors[0].contains("`cadpole mcp`"), "{errors:?}");
        let f = ctx.features.iter().find(|f| f.id.0 == "plan").expect("the problem's feature");
        assert_eq!((f.kind.as_str(), f.args.get("variables").map(String::as_str)), ("optimize", Some("1")));
        assert!(!f.args.contains_key("status"), "nothing was solved");
    }

    /// At the export, a mesh with a hole in it is refused,
    /// with the hole's position, rather than written and called `wrote`.
    ///
    /// The body is one no script can now build — the tessellator and
    /// the mesh kernel both refuse an open surface — so the test makes one by
    /// hand, by dropping a triangle from a box's mesh. That is the shape of a
    /// real defect: the plate ∪ boss of
    /// `examples/showcase.star` lost one triangle on the intersection curve
    /// at `--tolerance 0.01` and its STL went out with six unbalanced edges.
    #[test]
    fn export_refuses_a_mesh_with_a_hole_in_it_and_says_where() {
        use cadpole_core::context::Body;

        let holed = |ctx: &mut Context| {
            let id = ctx.bodies[0].id;
            let (mesh, topo) = ctx.body_mesh(id).unwrap().clone();
            let mut m = mesh;
            m.indices.truncate(m.indices.len() - 3);
            m.face_ids.truncate(m.face_ids.len() - 1);
            ctx.body_mut(id).unwrap().body = Body::Mesh((m, topo));
        };
        let req = pb::ExportRequest { format: pb::ExportFormat::Stl as i32, ..Default::default() };

        // Whole, it exports.
        let (mut ctx, _) = evaluate_document(&doc("box(20, 20, 10, name=\"b\")"), &HashMap::new(), 0.0);
        let drawn: Vec<cadpole_script::OutputFile> = Vec::new();
        let resp = export_evaluated(&mut ctx, &drawn, "b", &req).expect("export");
        assert!(!resp.data.is_empty());
        assert!(!resp.diagnostics.iter().any(|d| d.severity() == pb::Severity::Error), "{:?}", resp.diagnostics);

        // One triangle short, it is refused, and the message says where.
        let (mut ctx, _) = evaluate_document(&doc("box(20, 20, 10, name=\"b\")"), &HashMap::new(), 0.0);
        let drawn: Vec<cadpole_script::OutputFile> = Vec::new();
        holed(&mut ctx);
        let resp = export_evaluated(&mut ctx, &drawn, "b", &req).expect("no hard failure");
        assert!(resp.data.is_empty(), "a holed mesh was written anyway");
        let errs: Vec<&str> = resp
            .diagnostics
            .iter()
            .filter(|d| d.severity() == pb::Severity::Error)
            .map(|d| d.message.as_str())
            .collect();
        assert_eq!(errs.len(), 1, "{errs:?}");
        assert!(errs[0].contains("export refused"), "{}", errs[0]);
        assert!(errs[0].contains("\"b\""), "the body is named: {}", errs[0]);
        assert!(errs[0].contains("boundary edge"), "{}", errs[0]);
        assert!(errs[0].contains("hole at ("), "the hole is placed: {}", errs[0]);

        // STEP is an exact-surface format and does not go through this gate;
        // it refuses a mesh body for its own reason, which is a different
        // message and a hard error.
        let (mut ctx, _) = evaluate_document(&doc("box(20, 20, 10, name=\"b\")"), &HashMap::new(), 0.0);
        let drawn: Vec<cadpole_script::OutputFile> = Vec::new();
        holed(&mut ctx);
        let step = pb::ExportRequest { format: pb::ExportFormat::Step as i32, ..Default::default() };
        let e = export_evaluated(&mut ctx, &drawn, "b", &step).unwrap_err().to_string();
        assert!(e.contains("exact surfaces"), "{e}");
    }

    #[test]
    fn the_drawing_rpc_draws_a_sheet_and_names_its_scale() {
        let doc = pb::Document {
            name: "plate".into(),
            scripts: vec![pb::ScriptFile {
                path: "main.star".into(),
                source: "box(60, 30, 6, center=True, name=\"plate\")".into(),
            }],
            ..Default::default()
        };
        let resp = drawing(&pb::DrawingRequest {
            document: Some(doc.clone()),
            view: "three-view".into(),
            format: "svg".into(),
            sheet: "A4".into(),
            ..Default::default()
        })
        .expect("drawing");
        assert_eq!(resp.format, "svg");
        assert_eq!(resp.suggested_filename, "plate.svg");
        assert_eq!(resp.scale, "2.5:1");
        assert!(resp.items > 20, "{} items", resp.items);
        let svg = String::from_utf8(resp.data).unwrap();
        assert!(svg.contains("width=\"297mm\""));
        assert!(svg.contains(">60</text>"), "the overall width is figured");

        // DXF and PDF come out of the same drawing.
        for (fmt, magic) in [("dxf", "LWPOLYLINE"), ("pdf", "%PDF-")] {
            let r = drawing(&pb::DrawingRequest {
                document: Some(doc.clone()),
                format: fmt.into(),
                ..Default::default()
            })
            .expect(fmt);
            assert!(String::from_utf8_lossy(&r.data).contains(magic), "{fmt}");
        }
    }

    #[test]
    fn a_broken_script_gives_diagnostics_and_no_drawing() {
        let resp = drawing(&pb::DrawingRequest {
            document: Some(pb::Document {
                scripts: vec![pb::ScriptFile {
                    path: "main.star".into(),
                    source: "box(-1, -1, -1, name=\"bad\")".into(),
                }],
                ..Default::default()
            }),
            ..Default::default()
        })
        .expect("no hard failure");
        assert!(resp.data.is_empty());
        assert!(!resp.diagnostics.is_empty());
    }

    /// A small document with a two-sheet set whose cover reads its
    /// `sweep_table()` rows.
    const SHEET_DOC: &str = r#"w = param("w", 3000)
house = box(w, 4000, 3000, name="house")
report("width", w)
s = sheet_set("s", units="mm", scale="1:50")
cover = s.sheet("G0.0", "COVER")
rows = sweep_table([{"w": 2000}, {"w": 5000}])
cover.table([["WIDTH"]] + [[format(r["values"].get("width", 0), ".0f")] for r in rows], at=(50, 500), col_widths=[30])
plan = s.sheet("A1.0", "PLAN")
plan.plan_view(house, cut=1500, at=(100, 100))
export(s, "set.pdf")
export(plan, "plan.dxf")
"#;

    fn file<'a>(resp: &'a pb::ExportResponse, path: &str) -> &'a pb::ExportFile {
        resp.files.iter().find(|f| f.path == path).unwrap_or_else(|| {
            panic!("{path} not in {:?}", resp.files.iter().map(|f| &f.path).collect::<Vec<_>>())
        })
    }

    /// The Export rpc writes a script's sheet set under
    /// `run_script_exports` -- one PDF of every page and one sheet as DXF --
    /// with its `sweep_table()` rows filled from one evaluation per
    /// configuration; without the flag nothing of the set is written.
    #[test]
    fn export_writes_a_sheet_set_with_its_sweep_rows() {
        let doc = document_from_scripts("t", vec![("main.star".into(), SHEET_DOC.into())]);
        let req = pb::ExportRequest { document: Some(doc.clone()), run_script_exports: true, ..Default::default() };
        let resp = export(&req).expect("export");
        let errors: Vec<&str> =
            resp.diagnostics.iter().filter(|d| d.severity() == pb::Severity::Error).map(|d| d.message.as_str()).collect();
        assert!(errors.is_empty(), "{errors:?}");
        let pdf = &file(&resp, "set.pdf").data;
        let s = String::from_utf8_lossy(pdf);
        assert!(s.contains("/Count 2"), "two pages");
        let text = cadpole_drawing_out::pdf::extract_text(pdf);
        for want in ["COVER", "PLAN", "WIDTH", "2000", "5000"] {
            assert!(text.iter().any(|t| t == want), "{want} not in {text:?}");
        }
        assert!(String::from_utf8_lossy(&file(&resp, "plan.dxf").data).contains("SECTION"));
        let plain = export(&pb::ExportRequest { document: Some(doc), ..Default::default() }).expect("export");
        assert!(plain.files.is_empty(), "{:?}", plain.files.iter().map(|f| &f.path).collect::<Vec<_>>());
    }

    /// An export of the script's own files alone meshes and judges
    /// no body for a whole-document file nobody asked for. Two cylinders
    /// whose chords eat most of their gap warn when the whole document is
    /// written as well, and say nothing when only the set
    /// is: a drawing set made from a model of two thousand bodies otherwise prints 21 of
    /// them, for bodies on no sheet.
    #[test]
    fn a_set_export_alone_judges_no_body() {
        let src = "a = cylinder(10, 10, name=\"a\")\n\
                   b = cylinder(10, 10, at=(20.3, 0, 0), name=\"b\")\n\
                   s = sheet_set(\"s\", scale=\"1:1\")\n\
                   s.sheet(\"A1\", \"PLAN\").plan_view([a, b], cut=5, at=(100, 100))\n\
                   export(s, \"set.pdf\")\n";
        let ask = |only: bool| {
            export(&pb::ExportRequest {
                document: Some(doc(src)),
                tolerance: 0.2,
                run_script_exports: !only,
                script_exports_only: only,
                ..Default::default()
            })
            .expect("export")
        };
        let chords = |r: &pb::ExportResponse| r.diagnostics.iter().filter(|d| d.message.contains("chord error")).count();
        let both = ask(false);
        assert_eq!(chords(&both), 2, "the whole-document STL judges its meshes: {:?}", both.diagnostics);
        assert!(!both.data.is_empty() && both.bodies.len() == 2);
        let only = ask(true);
        assert_eq!(chords(&only), 0, "{:?}", only.diagnostics);
        assert!(only.data.is_empty() && only.bodies.is_empty() && only.suggested_filename.is_empty());
        assert_eq!(only.files.iter().map(|f| f.path.as_str()).collect::<Vec<_>>(), ["set.pdf"]);
        assert!(String::from_utf8_lossy(&only.files[0].data).contains("/Count 1"), "one page");
    }

    /// `cadpole op Drawing --view sheet:A1.0` is one page of the set,
    /// exactly as the export draws it, sweep rows and all; `--view sheets`
    /// is every page as one PDF.
    #[test]
    fn the_drawing_rpc_previews_one_sheet_or_the_whole_set() {
        let doc = document_from_scripts("t", vec![("main.star".into(), SHEET_DOC.into())]);
        let ask = |view: &str, format: &str| {
            drawing(&pb::DrawingRequest {
                document: Some(doc.clone()),
                view: view.into(),
                format: format.into(),
                ..Default::default()
            })
        };
        let r = ask("sheet:a1.0", "svg").expect("one sheet");
        assert_eq!((r.format.as_str(), r.scale.as_str(), r.suggested_filename.as_str()), ("svg", "1:50", "t_A1.0.svg"));
        let svg = String::from_utf8_lossy(&r.data);
        assert!(svg.starts_with("<?xml") && svg.contains("<title>A1.0 PLAN</title>"), "{}", &svg[..200.min(svg.len())]);
        assert!(r.items > 20, "{} items", r.items);
        let cover = ask("sheet:G0.0", "pdf").expect("the cover");
        let text = cadpole_drawing_out::pdf::extract_text(&cover.data);
        assert!(text.iter().any(|t| t == "5000"), "the preview fills the sweep rows too: {text:?}");
        let all = ask("sheets", "pdf").expect("the set");
        assert!(String::from_utf8_lossy(&all.data).contains("/Count 2"));
        let e = ask("sheet:Z9", "svg").unwrap_err().to_string();
        assert!(e.contains("G0.0, A1.0"), "{e}");
        let e = ask("sheets", "svg").unwrap_err().to_string();
        assert!(e.contains("--format pdf"), "{e}");
    }

    /// The `Drawing` rpc's colour look on `examples/color_plan.star`
    /// -- a plan cut at 1200 whose room names, title and legend are text,
    /// the bundled face embedded -- and a section; `pdffonts` and
    /// `pdftotext` agree where the machine has them.
    #[test]
    fn a_colour_plan_is_a_vector_page_whose_words_are_embedded_text() {
        let dir = std::env::var("CADPOLE_EXAMPLES").unwrap_or_else(|_| "examples".into());
        let src = std::fs::read_to_string(std::path::Path::new(&dir).join("color_plan.star")).expect("examples/color_plan.star");
        let doc = document_from_scripts("color_plan", vec![("main.star".into(), src)]);
        let ask = |section: &str, extra: &dyn Fn(&mut pb::DrawingRequest)| {
            let mut req = pb::DrawingRequest {
                document: Some(doc.clone()),
                look: "color".into(),
                section: section.into(),
                format: "pdf".into(),
                title: "THE STUDIO".into(),
                legend: vec!["#c9774f=walls".into()],
                ..Default::default()
            };
            extra(&mut req);
            drawing(&req)
        };
        let resp = ask("z=1200", &|_| {}).expect("a colour plan");
        assert!(resp.diagnostics.iter().all(|d| d.severity() != pb::Severity::Error), "{:?}", resp.diagnostics);
        assert_eq!(resp.scale, "1/4\" = 1'-0\"", "the document's feet and inches pick an architect's scale");
        let s = String::from_utf8_lossy(&resp.data);
        assert!(s.contains("/FontFile2") && s.contains("+Inter-Bold") && s.contains("+Inter-Regular"), "both weights embedded");
        let text = cadpole_drawing_out::pdf::extract_text(&resp.data);
        for want in ["THE STUDIO", "walls", "Studio", "Bath", "189 sf", "Deck", "N"] {
            assert!(text.iter().any(|t| t == want), "{want:?} not in {text:?}");
        }
        // The labels layer off: the room names are not drawn.
        let off = ask("z=1200", &|r| r.layers = vec![parse_layer_state("labels=off").unwrap()]).unwrap();
        assert!(!cadpole_drawing_out::pdf::extract_text(&off.data).iter().any(|t| t == "Studio"));
        // A section, and the line look refusing a colour cut.
        let sec = ask("x=3000", &|_| {}).unwrap();
        assert!(sec.items > 10 && !sec.data.is_empty());
        // The line look on the same page: the cut hatched, the words text.
        let lines = ask("-y=2000", &|r| r.look = "lines".into()).expect("a line-look section");
        assert!(cadpole_drawing_out::pdf::extract_text(&lines.data).iter().any(|t| t == "THE STUDIO"));
        let e = ask("z=1200", &|r| r.look = "pastel".into()).unwrap_err().to_string();
        assert!(e.contains("\"lines\" or \"color\""), "{e}");
        let e = ask("w=1", &|_| {}).unwrap_err().to_string();
        assert!(e.contains("x, y or z"), "{e}");
        // A reader that is not ours, where the box has one.
        let path = std::env::temp_dir().join(format!("cadpole-color-plan-{}.pdf", std::process::id()));
        std::fs::write(&path, &resp.data).unwrap();
        if let Ok(out) = std::process::Command::new("pdffonts").arg(&path).output() {
            if out.status.success() {
                let fonts = String::from_utf8_lossy(&out.stdout);
                assert!(fonts.contains("Inter-Regular") && fonts.contains("CID TrueType"), "{fonts}");
                assert!(fonts.lines().filter(|l| l.contains("Inter")).all(|l| l.contains("yes yes yes")), "embedded, subset, unicode:\n{fonts}");
            }
        }
        if let Ok(out) = std::process::Command::new("pdftotext").arg(&path).arg("-").output() {
            if out.status.success() {
                let read = String::from_utf8_lossy(&out.stdout);
                for want in ["THE STUDIO", "Studio", "Bath", "189 sf"] {
                    assert!(read.contains(want), "pdftotext does not find {want:?}:\n{read}");
                }
            }
        }
        let _ = std::fs::remove_file(&path);
    }

    /// End to end: `examples/house_set.star` is four ARCH D
    /// pages from one `export()`, the cover's table filled, and every
    /// sheet's name is text a PDF reader finds -- by `pdftotext` where the
    /// machine has it, and always by reading the content streams back.
    #[test]
    fn the_house_set_is_four_arch_d_pages_whose_names_a_reader_finds() {
        let dir = std::env::var("CADPOLE_EXAMPLES").unwrap_or_else(|_| "examples".into());
        let src = std::fs::read_to_string(std::path::Path::new(&dir).join("house_set.star")).expect("examples/house_set.star");
        let doc = document_from_scripts("house_set", vec![("main.star".into(), src)]);
        let resp = export(&pb::ExportRequest { document: Some(doc), run_script_exports: true, ..Default::default() }).expect("export");
        let errors: Vec<&str> =
            resp.diagnostics.iter().filter(|d| d.severity() == pb::Severity::Error).map(|d| d.message.as_str()).collect();
        assert!(errors.is_empty(), "{errors:?}");
        let pdf = &file(&resp, "house_set.pdf").data;
        let s = String::from_utf8_lossy(pdf);
        assert!(s.contains("/Count 4"), "four pages");
        assert_eq!(s.matches("/MediaBox [0 0 2592 1728]").count(), 4, "every page is ARCH D, 36 x 24 in");
        let names = ["COVER SHEET", "SITE PLAN", "FLOOR PLANS", "SECTION AND ELEVATION"];
        let text = cadpole_drawing_out::pdf::extract_text(pdf);
        for want in names.iter().chain(["G0.0", "A1.0", "A2.0", "A3.0", "SHEET INDEX"].iter()) {
            assert!(text.iter().any(|t| t == want), "{want} not in the PDF's text");
        }
        assert!(!text.iter().any(|t| t == "PENDING"), "the cover's setback table is filled");
        // A reader that is not ours, where the box has one.
        let path = std::env::temp_dir().join(format!("cadpole-house-set-{}.pdf", std::process::id()));
        std::fs::write(&path, pdf).unwrap();
        match std::process::Command::new("pdftotext").arg(&path).arg("-").output() {
            Ok(out) if out.status.success() => {
                let read = String::from_utf8_lossy(&out.stdout);
                for want in names {
                    assert!(read.contains(want), "pdftotext does not find {want:?}:\n{read}");
                }
            }
            _ => eprintln!("pdftotext is not installed here; the names were read back from the content streams only"),
        }
        let _ = std::fs::remove_file(&path);
    }

    #[test]
    fn export_writes_the_2d_formats_and_the_scripts_own_drawing_files() {
        let doc = pb::Document {
            name: "plate".into(),
            scripts: vec![pb::ScriptFile {
                path: "main.star".into(),
                source: concat!(
                    "b = box(60, 30, 6, center=True, name=\"plate\")\n",
                    "export_svg(drawing(b, view=\"front\"), \"front.svg\")\n"
                )
                .into(),
            }],
            ..Default::default()
        };
        let resp = export(&pb::ExportRequest {
            document: Some(doc.clone()),
            format: pb::ExportFormat::Svg as i32,
            run_script_exports: true,
            ..Default::default()
        })
        .expect("export");
        assert_eq!(resp.suggested_filename, "plate.svg", "{:?}", resp.diagnostics);
        assert!(String::from_utf8_lossy(&resp.data).starts_with("<?xml"));
        assert_eq!(resp.files.len(), 1);
        assert_eq!(resp.files[0].path, "front.svg");
        assert!(String::from_utf8_lossy(&resp.files[0].data).contains("<svg"));
    }

    /// A rigid copy shares its source's mesh, so it reports the source's
    /// own level: `copy(b).translate(...)` derives the moved body from the
    /// copy, and an instance from a part body that is a copy. Reading one link
    /// up the chain would say `0.05 mm (document; made at 0.0100 mm)` for a
    /// mesh made at the source's own 0.01; every derived body names the level
    /// the mesh was actually made at, in the export report and in `Inspect`.
    #[test]
    fn a_rigid_copy_reports_the_tolerance_its_source_owns() {
        let src = concat!(
            "b = box(20, 10, 5, name=\"src\")\n",
            "b.tessellation(tolerance=0.01)\n",
            "c1 = copy(b, name=\"c1\").translate((30, 0, 0), name=\"c1_moved\")\n",
            "c2 = copy(c1, name=\"c2\").translate((0, 30, 0), name=\"c2_moved\")\n",
            "p = part([copy(b, name=\"pb\")], name=\"p\")\n",
            "i1 = instance(p, at=(60, 0, 0), name=\"i1\")\n",
            "plain = box(5, 5, 5, at=(0, -20, 0), name=\"plain\")\n",
            "pc = copy(plain, name=\"pc\").translate((0, -40, 0), name=\"pc_moved\")\n",
        );
        let req = pb::ExportRequest { format: pb::ExportFormat::Stl as i32, ..Default::default() };
        let (mut ctx, drawn, _) = evaluate_document_limited(
            &doc(src),
            &HashMap::new(),
            0.05,
            0.0,
            &HashMap::new(),
            limits::default_limits(),
        );
        let resp = export_evaluated(&mut ctx, &drawn, "t", &req).expect("export");
        assert!(!resp.diagnostics.iter().any(|d| d.severity() == pb::Severity::Error), "{:?}", resp.diagnostics);
        let levels: Vec<(String, f64, String, f64)> = resp
            .bodies
            .iter()
            .map(|b| (b.name.clone(), b.tolerance, b.tolerance_source.clone(), b.tessellated_at))
            .collect();
        for want in ["src", "c1_moved", "c2_moved", "i1/pb"] {
            let got = levels.iter().find(|l| l.0 == want).unwrap_or_else(|| panic!("{want} in {levels:?}"));
            assert_eq!((got.1, got.2.as_str(), got.3), (0.01, "body", 0.01), "{want}: {got:?}");
        }
        // A copy of a body with no tessellation of its own is still the document's.
        let pc = levels.iter().find(|l| l.0 == "pc_moved").unwrap();
        assert_eq!((pc.1, pc.2.as_str(), pc.3), (0.05, "document", 0.05), "{pc:?}");
        // And `Inspect` says the same.
        let inspection = inspect::inspect_context(&mut ctx, &[]).expect("inspect");
        let i1 = inspection.iter().find(|b| b.name == "i1/pb").expect("the instance body is inspected");
        assert_eq!((i1.tolerance, i1.tolerance_source.as_str(), i1.tessellated_at), (0.01, "body", 0.01));
    }

    /// The three levels of tolerance on the export: a body's own
    /// `tessellation()` beats `export(..., tolerance=)`, which beats the
    /// document's `--tolerance`; each body of a script export reports the
    /// tolerance it got, where it came from, its triangle count and a chord
    /// error no larger than the tolerance; finer means more triangles; and
    /// STEP takes none of it.
    #[test]
    fn export_reports_each_body_at_its_own_level_of_tolerance() {
        let src = concat!(
            "own = cylinder(10, 10, name=\"own\").tessellation(tolerance=0.01)\n",
            "call = cylinder(10, 10, at=(40, 0, 0), name=\"call\")\n",
            "doc = cylinder(10, 10, at=(80, 0, 0), name=\"doc\")\n",
            "export([own, call], \"fine.stl\", tolerance=0.02)\n",
            "export(doc, \"coarse.stl\")\n",
            "export([own, call, doc], \"all.step\")\n",
        );
        let req = pb::ExportRequest {
            format: pb::ExportFormat::Stl as i32,
            run_script_exports: true,
            ..Default::default()
        };
        let (mut ctx, drawn, _) = evaluate_document_limited(
            &doc(src),
            &HashMap::new(),
            0.1,
            0.0,
            &HashMap::new(),
            limits::default_limits(),
        );
        let resp = export_evaluated(&mut ctx, &drawn, "t", &req).expect("export");
        assert!(!resp.diagnostics.iter().any(|d| d.severity() == pb::Severity::Error), "{:?}", resp.diagnostics);
        assert_eq!(resp.files.len(), 3);

        let fine = &resp.files[0];
        assert_eq!(fine.path, "fine.stl");
        let by_name = |f: &pb::ExportFile, n: &str| f.bodies.iter().find(|b| b.name == n).cloned().unwrap();
        let own = by_name(fine, "own");
        let call = by_name(fine, "call");
        assert_eq!((own.tolerance, own.tolerance_source.as_str()), (0.01, "body"));
        assert_eq!((call.tolerance, call.tolerance_source.as_str()), (0.02, "call"));
        let coarse = by_name(&resp.files[1], "doc");
        assert_eq!((coarse.tolerance, coarse.tolerance_source.as_str()), (0.1, "document"));
        // Finer is more triangles, and the measured error is under what
        // was asked for, at every level.
        assert!(own.triangles > call.triangles && call.triangles > coarse.triangles, "{} {} {}", own.triangles, call.triangles, coarse.triangles);
        for b in [&own, &call, &coarse] {
            let e = b.chord_error.expect("a B-rep body has a measured chord error");
            assert!(e > 0.0 && e <= b.tolerance, "{}: chord error {e} at tolerance {}", b.name, b.tolerance);
            assert!(!b.chord_error_face.is_empty(), "the face is named");
            assert_eq!(b.tessellated_at, b.tolerance, "made at the requested tolerance");
            assert!(!b.exact);
        }
        // STEP: surfaces, not triangles.
        let step = &resp.files[2];
        assert_eq!(step.bodies.len(), 3);
        for b in &step.bodies {
            assert!(b.exact && b.triangles == 0 && b.chord_error.is_none(), "{b:?}");
        }
        // The whole-document export has no call level: the body's own, else
        // the document's.
        let whole: Vec<(String, f64, String)> =
            resp.bodies.iter().map(|b| (b.name.clone(), b.tolerance, b.tolerance_source.clone())).collect();
        assert_eq!(
            whole,
            vec![
                ("own".to_owned(), 0.01, "body".to_owned()),
                ("call".to_owned(), 0.1, "document".to_owned()),
                ("doc".to_owned(), 0.1, "document".to_owned()),
            ]
        );
    }

    /// The chord-error-vs-gap warning: two cylinders 0.3 mm apart
    /// exported at 0.2 mm warn — the chords eat most of the gap — naming the
    /// body, the error, the gap and a tolerance to pass; at 0.02 mm they do
    /// not. A warning, never an error: the file is still written.
    #[test]
    fn a_coarse_export_of_two_close_bodies_warns_and_a_fine_one_does_not() {
        let src = concat!(
            "a = cylinder(10, 10, name=\"a\")\n",
            "b = cylinder(10, 10, at=(20.3, 0, 0), name=\"b\")\n",
        );
        let warnings = |tol: f64| {
            let resp = export(&pb::ExportRequest {
                document: Some(doc(src)),
                format: pb::ExportFormat::Stl as i32,
                tolerance: tol,
                ..Default::default()
            })
            .expect("export");
            assert!(!resp.data.is_empty(), "the file is still written at {tol}");
            assert!(!resp.diagnostics.iter().any(|d| d.severity() == pb::Severity::Error), "{:?}", resp.diagnostics);
            resp.diagnostics
                .iter()
                .filter(|d| d.severity() == pb::Severity::Warning && d.message.contains("chord error"))
                .map(|d| d.message.clone())
                .collect::<Vec<_>>()
        };
        let coarse = warnings(0.2);
        assert_eq!(coarse.len(), 2, "both bodies are at fault: {coarse:?}");
        let w = &coarse[0];
        assert!(w.starts_with("export: \"a\" (a#0): chord error 0."), "{w}");
        assert!(w.contains("at tolerance 0.2 mm is more than a third of the gap to \"b\""), "{w}");
        assert!(w.contains("apart between a/side") && w.contains("and b/side"), "names the faces: {w}");
        assert!(w.contains("the exact surfaces could be anywhere from touching to"), "{w}");
        assert!(w.contains("`a.tessellation(tolerance=0.0"), "names the fix: {w}");
        assert!(w.contains(""), "{w}");
        assert!(warnings(0.02).is_empty());
    }

    #[test]
    fn a_filament_slot_pinned_to_two_colours_is_reported_on_both_export_paths() {
        // `palette()` builds the warning and `export_3mf` returns it; a
        // caller that takes `.0` and drops it leaves a user who asked for two
        // colours on slot 1 with three filaments, the second colour quietly
        // moved to slot 3, and nothing said.
        let doc = pb::Document {
            name: "two".into(),
            scripts: vec![pb::ScriptFile {
                path: "main.star".into(),
                source: concat!(
                    "a = box(10, 10, 10, name=\"a\")\n",
                    "b = box(10, 10, 10, at=(20, 0, 0), name=\"b\")\n",
                    "paint(a, color=\"#ff0000\", slot=1)\n",
                    "paint(b, color=\"#00ff00\", slot=1)\n",
                    "export([a, b], \"two.3mf\")\n",
                )
                .into(),
            }],
            ..Default::default()
        };
        let pinned = |d: &pb::Diagnostic| {
            d.severity() == pb::Severity::Warning && d.message.contains("filament slot 1 is pinned to both")
        };

        // The whole-document export.
        let resp = export(&pb::ExportRequest {
            document: Some(doc.clone()),
            format: pb::ExportFormat::Threemf as i32,
            ..Default::default()
        })
        .expect("export");
        assert!(!resp.data.is_empty());
        assert!(resp.diagnostics.iter().any(pinned), "{:?}", resp.diagnostics);

        // And the script's own `export(...)` call, which writes its own file.
        let resp = export(&pb::ExportRequest {
            document: Some(doc),
            format: pb::ExportFormat::Threemf as i32,
            run_script_exports: true,
            ..Default::default()
        })
        .expect("export");
        assert_eq!(resp.files.len(), 1);
        assert!(resp.diagnostics.iter().any(pinned), "{:?}", resp.diagnostics);
    }

    /// The Bambu flavour: three `export()` calls to one path are one
    /// project file with three plates, the named call one object with a
    /// part per body, the colours one filament list, a thumbnail per plate,
    /// and the plain 3MF the same script also asks for untouched beside it.
    #[test]
    fn a_bambu_project_accumulates_every_export_call_to_one_path() {
        let doc = pb::Document {
            name: "toy".into(),
            scripts: vec![pb::ScriptFile {
                path: "main.star".into(),
                source: concat!(
                    "a = box(20, 20, 10, name=\"a\")\n",
                    "a.material(color=\"#e8b84a\")\n",
                    "b = box(10, 10, 8, at=(30, 0, 0), name=\"b\")\n",
                    "b.material(color=\"#2f8f8f\")\n",
                    "c = box(15, 15, 5, at=(0, 40, 0), name=\"c\")\n",
                    "c.material(color=\"#d94a3d\")\n",
                    "export([a, b], \"proj.3mf\", flavor=flavor.bambu, plate=\"Core\", name=\"core\")\n",
                    "export(c, \"proj.3mf\", flavor=flavor.bambu, plate=\"Lid\")\n",
                    "export([a, c], \"proj.3mf\", flavor=flavor.bambu, plate=3, extruder=[4, 1])\n",
                    "export([a, b, c], \"plain.3mf\")\n",
                )
                .into(),
            }],
            ..Default::default()
        };
        let resp = export(&pb::ExportRequest {
            document: Some(doc),
            format: pb::ExportFormat::Threemf as i32,
            run_script_exports: true,
            ..Default::default()
        })
        .expect("export");
        assert!(!resp.diagnostics.iter().any(|d| d.severity() == pb::Severity::Error), "{:?}", resp.diagnostics);
        assert_eq!(resp.files.iter().map(|f| f.path.as_str()).collect::<Vec<_>>(), ["proj.3mf", "plain.3mf"]);
        let project = &resp.files[0];
        assert_eq!(project.bodies.len(), 5, "a report line per body per call");

        let z = cadpole_export_3mf::zip::read(&project.data).unwrap();
        let settings = z.text(cadpole_export_3mf::BAMBU_MODEL_CONFIG).unwrap();
        for expected in [
            "<metadata key=\"plater_name\" value=\"Core\"/>",
            "<metadata key=\"plater_name\" value=\"Lid\"/>",
            "<metadata key=\"plater_id\" value=\"3\"/>",
            "<metadata key=\"name\" value=\"core\"/>",
        ] {
            assert!(settings.contains(expected), "{expected}\n{settings}");
        }
        assert_eq!(settings.matches("<plate>").count(), 3);
        assert_eq!(settings.matches("<object id=").count(), 4, "core, c, a, c");
        assert_eq!(z.names.iter().filter(|n| n.starts_with("3D/Objects/")).count(), 4);
        // The slots: a, b, c by first use (each is free in some call), then
        // a's pin opens slot 4 with a's colour; c's pin onto slot 1 prints it
        // in a's colour and says nothing, because a pin is an instruction.
        let colours = filament_colours(&z);
        assert_eq!(colours, ["#E8B84A", "#2F8F8F", "#D94A3D", "#E8B84A"]);
        assert!(!resp.diagnostics.iter().any(|d| d.message.contains("extruder")), "{:?}", resp.diagnostics);
        // The core keeps a in slot 1: the pin was the third call's.
        let core = settings.split("<object id=").nth(1).unwrap();
        assert!(core.contains("<metadata key=\"name\" value=\"a\"/>\n      <metadata key=\"matrix\" value=\"1 0 0 -10 0 1 0 0 0 0 1 0 0 0 0 1\"/>\n      <metadata key=\"extruder\" value=\"1\"/>"), "{core}");
        // Plates 2 and 3 sit on Bambu's grid -- two to a row for three
        // plates, so plate 2 at x = 307.2 and plate 3 starting the second
        // row at y = -307.2 -- each plate's contents centred on its
        // 256 mm bed: c alone at the centre of plate 2; a and c (x 0..20,
        // y 0..55) as a group on 3.
        let root = z.text(cadpole_export_3mf::MODEL_PART).unwrap();
        let items: Vec<Vec<f64>> = root
            .split("<item ")
            .skip(1)
            .map(|it| {
                let t = it.split("transform=\"").nth(1).unwrap().split('"').next().unwrap();
                t.split_whitespace().map(|v| v.parse().unwrap()).collect::<Vec<f64>>()[9..].to_vec()
            })
            .collect();
        let near = |a: &[f64], b: [f64; 3]| (0..3).all(|k| (a[k] - b[k]).abs() < 1e-6);
        for want in [[128.0, 128.0, 5.0], [435.2, 128.0, 2.5], [128.0, -307.2 + 110.5, 5.0], [125.5, -307.2 + 148.0, 2.5]] {
            assert!(items.iter().any(|t| near(t, want)), "{want:?} in {items:?}");
        }
        // A thumbnail pair per plate, 512 and 128 px PNGs.
        for n in 1..=3 {
            for (name, px) in [(format!("Metadata/plate_{n}.png"), 512u32), (format!("Metadata/plate_{n}_small.png"), 128)] {
                let png = z.get(&name).unwrap_or_else(|| panic!("{name}"));
                assert_eq!(&png[1..4], b"PNG");
                assert_eq!(u32::from_be_bytes([png[16], png[17], png[18], png[19]]), px, "{name}");
            }
        }
        // And the plain file is the standard writer's: one object per body.
        let plain = cadpole_export_3mf::zip::read(&resp.files[1].data).unwrap();
        assert!(plain.get(cadpole_export_3mf::BAMBU_MODEL_CONFIG).is_none());
        assert_eq!(plain.text(cadpole_export_3mf::MODEL_PART).unwrap().matches("<object ").count(), 3);
    }

    /// A body pinned everywhere it appears claims no colour slot: the toy's
    /// lid, crank and knob print in the core's four slots and the project
    /// lists four filaments.
    #[test]
    fn a_body_pinned_in_every_call_claims_no_slot_of_its_own() {
        let resp = export(&pb::ExportRequest {
            document: Some(doc(concat!(
                "a = box(10, 10, 10, name=\"a\")\n",
                "a.material(color=\"#e8b84a\")\n",
                "b = box(10, 10, 10, at=(20, 0, 0), name=\"b\")\n",
                "b.material(color=\"#2f8f8f\")\n",
                "lid = box(10, 10, 4, at=(40, 0, 0), name=\"lid\")\n",
                "lid.material(color=\"#4a7fd6\")\n",
                "export([a, b], \"t.3mf\", flavor=flavor.bambu, plate=1, name=\"core\")\n",
                "export(lid, \"t.3mf\", flavor=flavor.bambu, plate=2, extruder=2)\n",
            ))),
            format: pb::ExportFormat::Threemf as i32,
            run_script_exports: true,
            ..Default::default()
        })
        .expect("export");
        // The one warning is the built-in settings' own (no template=).
        assert!(
            !resp.diagnostics.iter().any(|d| d.severity() >= pb::Severity::Warning && !d.message.contains("no template=")),
            "{:?}",
            resp.diagnostics
        );
        let z = cadpole_export_3mf::zip::read(&resp.files[0].data).unwrap();
        assert_eq!(filament_colours(&z), ["#E8B84A", "#2F8F8F"]);
        let settings = z.text(cadpole_export_3mf::BAMBU_MODEL_CONFIG).unwrap();
        let lid = settings.split("<object id=").nth(2).unwrap();
        assert!(lid.contains("<metadata key=\"extruder\" value=\"2\"/>"), "{lid}");
    }

    #[test]
    fn a_path_written_as_both_flavors_is_refused_by_name() {
        let resp = export(&pb::ExportRequest {
            document: Some(doc(concat!(
                "a = box(10, 10, 10, name=\"a\")\n",
                "export(a, \"x.3mf\", flavor=flavor.bambu)\n",
                "export(a, \"x.3mf\")\n",
            ))),
            format: pb::ExportFormat::Threemf as i32,
            run_script_exports: true,
            ..Default::default()
        })
        .expect("export");
        assert!(resp.files.is_empty());
        let err = resp.diagnostics.iter().find(|d| d.severity() == pb::Severity::Error).expect("an error");
        assert!(err.message.contains("\"x.3mf\"") && err.message.contains("flavor=flavor.bambu"), "{}", err.message);
    }

    /// `cadpole export --format 3mf --flavor bambu --plate 2 --group part`: the
    /// whole document as one object on plate 2.
    #[test]
    fn the_whole_document_export_takes_the_bambu_flavor_and_a_plate() {
        let resp = export(&pb::ExportRequest {
            document: Some(doc("a = box(10, 10, 10, name=\"a\")\nb = box(10, 10, 10, at=(20, 0, 0), name=\"b\")\n")),
            format: pb::ExportFormat::Threemf as i32,
            flavor: "bambu".into(),
            plate: 2,
            group: "part".into(),
            printer: "bambu_a1_mini".into(),
            ..Default::default()
        })
        .expect("export");
        assert!(!resp.diagnostics.iter().any(|d| d.severity() == pb::Severity::Error), "{:?}", resp.diagnostics);
        assert_eq!(resp.suggested_filename, "t.3mf", "the test document is named t");
        let z = cadpole_export_3mf::zip::read(&resp.data).unwrap();
        let settings = z.text(cadpole_export_3mf::BAMBU_MODEL_CONFIG).unwrap();
        assert_eq!(settings.matches("<plate>").count(), 2, "plate 1 empty, the model on plate 2");
        assert_eq!(settings.matches("<object id=").count(), 1);
        assert_eq!(settings.matches("<part id=").count(), 2);
        // The A1 mini's 180 mm bed: plate 2 starts at x = 216, and the
        // object (x 0..30, y 0..10) is centred on it.
        let root = z.text(cadpole_export_3mf::MODEL_PART).unwrap();
        assert!(root.contains("transform=\"1 0 0 0 1 0 0 0 1 306 90 5\""), "{root}");

        let bad = export(&pb::ExportRequest {
            document: Some(doc("a = box(10, 10, 10)\n")),
            format: pb::ExportFormat::Threemf as i32,
            flavor: "orca".into(),
            ..Default::default()
        });
        assert!(bad.unwrap_err().to_string().contains("valid: standard, bambu"));
    }

    /// A Bambu project's `filament_colour`, read as the JSON it is.
    fn filament_colours(z: &cadpole_export_3mf::zip::Archive) -> Vec<String> {
        let text = z.text(cadpole_export_3mf::BAMBU_PROJECT_CONFIG).expect("project_settings.config");
        let v: serde_json::Value = serde_json::from_str(&text).expect("the settings are JSON");
        v["filament_colour"].as_array().expect("a list").iter().map(|c| c.as_str().unwrap().to_owned()).collect()
    }

    /// The PNG header's width and height.
    fn png_size(png: &[u8]) -> (u32, u32) {
        assert_eq!(&png[1..4], b"PNG");
        (u32::from_be_bytes([png[16], png[17], png[18], png[19]]), u32::from_be_bytes([png[20], png[21], png[22], png[23]]))
    }

    fn pixel(img: &cadpole_core::render::Image, x: u32, y: u32) -> [u8; 4] {
        let i = ((y * img.width + x) * 4) as usize;
        [img.rgba[i], img.rgba[i + 1], img.rgba[i + 2], img.rgba[i + 3]]
    }

    const PLATES: &str = concat!(
        "a = box(20, 20, 10, name=\"a\")\n",
        "a.material(color=\"#e8b84a\")\n",
        "b = box(10, 10, 8, at=(30, 0, 0), name=\"b\")\n",
        "b.material(color=\"#2f8f8f\")\n",
        "c = box(15, 15, 5, at=(0, 40, 0), name=\"c\")\n",
        "c.material(color=\"#d94a3d\")\n",
        "export([a, b], \"proj.3mf\", flavor=flavor.bambu, plate=\"Core (print in place)\", name=\"core\")\n",
        "export(c, \"proj.3mf\", flavor=flavor.bambu, plate=\"Lid\")\n",
        "export([a, c], \"proj.3mf\", flavor=flavor.bambu, plate=4)\n",
    );

    /// The standalone plate images: one PNG per plate that
    /// holds anything, named by number and name, at the asked size; a
    /// contact sheet tiling them three to a row with a label strip; and
    /// the same bytes on a second run.
    #[test]
    fn plate_images_are_one_per_plate_named_sized_and_tiled_into_a_sheet() {
        let req = pb::ExportRequest {
            document: Some(doc(PLATES)),
            format: pb::ExportFormat::Threemf as i32,
            run_script_exports: true,
            plate_images: true,
            plate_image_width: 320,
            plate_image_height: 240,
            ..Default::default()
        };
        let resp = export(&req).expect("export");
        assert!(!resp.diagnostics.iter().any(|d| d.severity() == pb::Severity::Error), "{:?}", resp.diagnostics);
        assert!(resp.plate_images.is_empty() && resp.plate_sheet.is_empty(), "the whole-document export asked for none");
        let f = &resp.files[0];
        assert_eq!(
            f.plate_images.iter().map(|p| p.path.as_str()).collect::<Vec<_>>(),
            ["proj_plate_1_Core_print_in_place.png", "proj_plate_2_Lid.png", "proj_plate_4.png"],
            "plate 3 is empty and gets no image"
        );
        assert_eq!(f.plate_images.iter().map(|p| p.plate).collect::<Vec<_>>(), [1, 2, 4]);
        assert_eq!(
            f.plate_images.iter().map(|p| p.plate_name.as_str()).collect::<Vec<_>>(),
            ["Core (print in place)", "Lid", ""]
        );
        for p in &f.plate_images {
            assert_eq!((p.width, p.height), (320, 240), "{}", p.path);
            assert_eq!(png_size(&p.png), (320, 240), "{}", p.path);
        }
        // The sheet: half-size tiles (160 x 120), ceil(sqrt(3)) = 2 to a row
        // as Bambu lays plates out, a 16 px label strip and 6 px
        // gutters at the font scale the tile height gives (1 here):
        // 2 * (160 + 6) + 6 wide, 2 * (120 + 16 + 6) + 6 tall.
        assert_eq!(png_size(&f.plate_sheet), (338, 290));
        // The thumbnails in the file are what they were: 512 and 128 px.
        let z = cadpole_export_3mf::zip::read(&f.data).unwrap();
        assert_eq!(png_size(z.get("Metadata/plate_1.png").unwrap()), (512, 512));
        assert_eq!(png_size(z.get("Metadata/plate_1_small.png").unwrap()), (128, 128));

        // Deterministic: a second export gives the same bytes, images included.
        let again = export(&req).expect("export");
        assert_eq!(again.files[0].data, f.data);
        for (x, y) in again.files[0].plate_images.iter().zip(&f.plate_images) {
            assert_eq!(x.png, y.png, "{}", x.path);
        }
        assert_eq!(again.files[0].plate_sheet, f.plate_sheet);
    }

    /// Each part is drawn in the colour of the filament slot it prints in:
    /// a red part pinned to the teal slot comes out teal, as it will off
    /// the printer. Around the bed the image is transparent by default and
    /// opaque under a backdrop.
    #[test]
    fn plate_images_show_filament_colours_and_key_their_background() {
        let src = concat!(
            "a = box(20, 20, 10, name=\"a\")\n",
            "a.material(color=\"#2f8f8f\")\n",
            "c = box(20, 20, 10, at=(0, 40, 0), name=\"c\")\n",
            "c.material(color=\"#d94a3d\")\n",
            "export(a, \"proj.3mf\", flavor=flavor.bambu, plate=1)\n",
            "export(c, \"proj.3mf\", flavor=flavor.bambu, plate=2, extruder=1)\n",
        );
        let (mut ctx, _) = evaluate_document_with(&doc(src), &HashMap::new(), 0.0, 0.0);
        let specs = ctx.exports.clone();
        let mut render = |background: &str| {
            let opts = print::PlateImageOptions::parse(240, 180, background, "").unwrap();
            print::export_bambu_project(&mut ctx, &specs, "proj", false, Some(&opts)).unwrap()
        };
        let out = render("transparent");
        assert_eq!(out.plate_images.len(), 2);
        // The camera looks at the centre of the bed and the part stands
        // there, so the middle pixel is on the part.
        for p in &out.plate_images {
            let img = &p.image;
            let [r, g, b, a] = pixel(img, img.width / 2, img.height / 2);
            assert_eq!(a, 255, "plate {}: the part is under the middle pixel", p.plate);
            assert!(g > r + 30 && b > r + 30, "plate {}: ({r}, {g}, {b}) is not the teal of slot 1", p.plate);
            assert_eq!(pixel(img, 0, 0)[3], 0, "plate {}: transparent at the corner", p.plate);
            assert_eq!(pixel(img, img.width - 1, img.height - 1)[3], 0);
        }
        let sheet = out.plate_sheet.expect("a sheet");
        assert_eq!(sheet.path, "proj_plates.png");
        assert_eq!(pixel(&sheet.image, 0, 0)[3], 0, "the sheet is transparent where the tiles are");

        let studio = render("studio");
        let corner = pixel(&studio.plate_images[0].image, 0, 0);
        assert_eq!(corner[3], 255, "a backdrop is opaque");
        assert!(corner[0] < 200 && corner[0] > 60, "the studio grey: {corner:?}");
        assert_eq!(pixel(&studio.plate_sheet.unwrap().image, 0, 0), corner, "the sheet stands in the same backdrop");
    }

    /// The whole-document export (`cadpole export --flavor bambu --plate-images
    /// DIR`) gets its images on the response, named after the document;
    /// a script's own `plate_images(...)` declaration governs its project's
    /// size and background whether or not the request asked.
    #[test]
    fn plate_images_come_from_the_request_or_from_the_scripts_declaration() {
        let resp = export(&pb::ExportRequest {
            document: Some(doc("a = box(10, 10, 10, name=\"a\")\nb = box(10, 10, 10, at=(20, 0, 0), name=\"b\")\n")),
            format: pb::ExportFormat::Threemf as i32,
            flavor: "bambu".into(),
            plate: 2,
            plate_images: true,
            plate_image_width: 200,
            plate_image_height: 150,
            ..Default::default()
        })
        .expect("export");
        assert_eq!(resp.plate_images.iter().map(|p| p.path.as_str()).collect::<Vec<_>>(), ["t_plate_2.png"]);
        assert_eq!(png_size(&resp.plate_images[0].png), (200, 150));
        assert_eq!(png_size(&resp.plate_sheet), (1 * (100 + 6) + 6, 75 + 16 + 6 + 6));

        let declared = concat!(
            "a = box(10, 10, 10, name=\"a\")\n",
            "export(a, \"d.3mf\", flavor=flavor.bambu)\n",
            "plate_images(\"d.3mf\", size=(160, 120), background=\"white\")\n",
        );
        let resp = export(&pb::ExportRequest {
            document: Some(doc(declared)),
            format: pb::ExportFormat::Threemf as i32,
            run_script_exports: true,
            ..Default::default()
        })
        .expect("export");
        let f = &resp.files[0];
        assert_eq!(f.plate_images.len(), 1, "declared, so rendered without the request asking");
        assert_eq!(f.plate_images[0].path, "d_plate_1.png");
        assert_eq!(png_size(&f.plate_images[0].png), (160, 120), "the declaration's size, not the default");
        assert!(!f.plate_sheet.is_empty());

        // An unknown background on the request is an error naming the choices.
        let bad = export(&pb::ExportRequest {
            document: Some(doc("a = box(10, 10, 10)\nexport(a, \"x.3mf\", flavor=flavor.bambu)\n")),
            format: pb::ExportFormat::Threemf as i32,
            run_script_exports: true,
            plate_images: true,
            plate_image_background: "sky".into(),
            ..Default::default()
        });
        assert!(bad.unwrap_err().to_string().contains("\"transparent\", \"studio\""));
    }

    /// The plate thumbnail is the plate's bodies on a transparent
    /// background at 512 px: the corners are clear, the middle is not.
    #[test]
    fn a_plate_thumbnail_is_transparent_around_the_model() {
        let (mut ctx, _) = evaluate_document_with(&doc("a = box(20, 20, 10, name=\"a\")\n"), &HashMap::new(), 0.0, 0.0);
        let id = ctx.bodies[0].id_string();
        let img = print::plate_thumbnail_image(&mut ctx, &[id]);
        assert_eq!((img.width, img.height), (512, 512));
        let alpha = |x: u32, y: u32| img.rgba[((y * img.width + x) * 4 + 3) as usize];
        assert_eq!(alpha(0, 0), 0, "a corner is clear");
        assert_eq!(alpha(511, 511), 0);
        assert_eq!(alpha(256, 256), 255, "the model is there");
        let small = print::downscale(&img, 128);
        assert_eq!((small.width, small.height), (128, 128));
        assert_eq!(small.rgba[3], 0);
        assert_eq!(small.rgba[((64 * 128 + 64) * 4 + 3) as usize], 255);
    }

    /// A plate shows what it will print, whatever layer the bodies are on:
    /// a print layout kept on a hidden layer, or on a ghosted one, beside
    /// the assembled model still fills its plate's picture and thumbnail,
    /// opaque.
    #[test]
    fn a_plate_draws_its_bodies_on_a_hidden_or_ghosted_layer() {
        let src = concat!(
            "kit = layer(\"kit\", visible=False)\n",
            "ghost = layer(\"ghost\", opacity=0.3)\n",
            "a = box(20, 20, 10, name=\"a\", layer=kit)\n",
            "g = box(20, 20, 10, at=(40, 0, 0), name=\"g\", layer=ghost)\n",
        );
        let (mut ctx, _) = evaluate_document_with(&doc(src), &HashMap::new(), 0.0, 0.0);
        let a = ctx.bodies.iter().find(|b| b.name == "a").unwrap().id;
        let g = ctx.bodies.iter().find(|b| b.name == "g").unwrap().id;
        // The display leaves the hidden one out and ghosts the other.
        assert_eq!(scene_from_context(&mut ctx, &[], &[], &[]).items.len(), 1);
        let scene = print::plate_scene(&mut ctx, &[(a, None, Vec3::ZERO), (g, None, Vec3::ZERO)]).unwrap();
        assert_eq!(scene.items.len(), 2, "both bodies are on the plate");
        assert!(scene.items.iter().all(|i| i.material.opacity == 1.0), "a plate is drawn opaque");
        let a_id = ctx.body(a).unwrap().id_string();
        let img = print::plate_thumbnail_image(&mut ctx, &[a_id]);
        assert_eq!(img.rgba[((256 * img.width + 256) * 4 + 3) as usize], 255, "the hidden layer's body is in the thumbnail");
    }

    #[test]
    fn evaluate_roundtrips_through_the_envelope() {
        let req = pb::Request {
            request: Some(pb::request::Request::Evaluate(pb::EvaluateRequest {
                document: Some(doc("b = box(10, 10, 10)")),
                ..Default::default()
            })),
        };
        let resp = pb::Response::decode(call(&req.encode_to_vec()).as_slice()).unwrap();
        let Some(pb::response::Response::Evaluate(e)) = resp.response else { panic!("{resp:?}") };
        let m = e.model.unwrap();
        assert_eq!(m.bodies.len(), 1);
        assert_eq!(m.bodies[0].mesh.as_ref().unwrap().indices.len(), 36);
        assert!(m.diagnostics.is_empty());
    }

    /// A document's `layer(...)` declarations reach the model in
    /// order with their settings, a body names its layer, the JSON view
    /// mirrors the proto's names and the proto round-trips into it -- and a
    /// document without layers has neither key.
    #[test]
    fn the_model_carries_layers_and_a_plain_model_does_not_mention_them() {
        use crate::view::{EvalIn, ScriptIn, evaluate_json, model_view_from_proto};
        let src = "env = layer(\"envelope\", doc=\"the zoning envelope\", opacity=0.3)\n\
                   b = box(10, 20, 5, name=\"b\", layer=env)\n\
                   c = box(5, 5, 5, at=(30, 0, 0), name=\"c\")\n";
        let r = evaluate(&pb::EvaluateRequest { document: Some(doc(src)), ..Default::default() });
        let m = r.model.expect("a model");
        assert_eq!(m.layers.len(), 1);
        let l = &m.layers[0];
        assert_eq!((l.name.as_str(), l.doc.as_str(), l.visible, l.opacity, l.order, l.color), ("envelope", "the zoning envelope", true, 0.3, 0, None));
        let by = |n: &str| m.bodies.iter().find(|b| b.name == n).unwrap().layer.clone();
        assert_eq!((by("b"), by("c")), ("envelope".to_owned(), String::new()));
        let back = model_view_from_proto(&m);
        assert_eq!(back.layers.len(), 1);
        assert_eq!(back.bodies.iter().find(|b| b.name == "b").unwrap().layer, "envelope");
        let (view, _) = evaluate_json(&EvalIn { scripts: vec![ScriptIn { path: "main.star".into(), source: src.into() }], ..Default::default() });
        let json = serde_json::to_string(&view).unwrap();
        assert!(json.contains("\"layers\":[{\"name\":\"envelope\"") && json.contains("\"layer\":\"envelope\""), "{json}");
        let (plain, _) = evaluate_json(&EvalIn { scripts: vec![ScriptIn { path: "main.star".into(), source: "b = box(1, 2, 3)".into() }], ..Default::default() });
        let json = serde_json::to_string(&plain).unwrap();
        assert!(!json.contains("layer"), "{json}");
    }

    /// A layer's own defaults and a render's overrides decide what a
    /// scene holds and how opaque each item is -- a hidden layer's bodies
    /// are left out, a ghosted layer's material carries the product opacity
    /// every backend already draws -- and the request path honours them.
    #[test]
    fn a_layer_override_hides_and_ghosts_at_render_time() {
        let src = "env = layer(\"envelope\", opacity=0.5)\nhid = layer(\"hidden\", visible=False)\n\
                   b = box(10, 20, 5, name=\"b\", layer=env)\n\
                   c = box(5, 5, 5, at=(30, 0, 0), name=\"c\")\n\
                   d = box(5, 5, 5, at=(-30, 0, 0), name=\"d\", layer=hid)\n";
        let (mut ctx, _) = evaluate_document_with(&doc(src), &HashMap::new(), 0.0, 0.0);
        let opacities = |scene: &RenderScene| -> Vec<f64> { scene.items.iter().map(|i| i.material.opacity).collect() };
        let parse = |s: &str| LayerOverride::parse(s).unwrap();
        // The declarations alone: the hidden layer is out, the envelope ghosted.
        assert_eq!(opacities(&scene_from_context(&mut ctx, &[], &[], &[])), vec![0.5, 1.0]);
        // Overrides: off hides, on shows a layer declared hidden.
        assert_eq!(opacities(&scene_from_context(&mut ctx, &[], &[], &[parse("envelope=off"), parse("hidden=on")])), vec![1.0, 1.0]);
        // An opacity ghosts, and `other` is the untagged bodies.
        assert_eq!(opacities(&scene_from_context(&mut ctx, &[], &[], &[parse("envelope=0.2"), parse("other=off")])), vec![0.2]);
        // The same through the request, as `cadpole render --layer` sends it.
        let req = |layers: Vec<pb::LayerState>| pb::RenderRequest { document: Some(doc(src)), width: 64, height: 48, layers, ..Default::default() };
        let shown = render(&req(Vec::new())).unwrap();
        let hidden = render(&req(vec![parse_layer_state("envelope=off").unwrap()])).unwrap();
        let ghosted = render(&req(vec![parse_layer_state("envelope=0.1").unwrap()])).unwrap();
        assert_ne!(shown.png, hidden.png);
        assert_ne!(shown.png, ghosted.png);
        assert_eq!(hidden.model.unwrap().layers.len(), 2, "the model still lists every layer");
        assert!(parse_layer_state("envelope=maybe").is_err());
    }

    #[test]
    fn render_produces_png() {
        let r = render(&pb::RenderRequest {
            document: Some(doc("b = box(10, 20, 5)")),
            width: 64,
            height: 48,
            ..Default::default()
        })
        .unwrap();
        assert_eq!(&r.png[1..4], b"PNG");
        assert_eq!((r.width, r.height), (64, 48));
    }

    /// Every debug view renders a box through the request path, forces the
    /// rasterizer at one sample whatever the request asked for, and the two
    /// that count something say so in an info diagnostic.
    #[test]
    fn render_debug_views_report_and_force_the_rasterizer() {
        for view in DebugView::all() {
            let req = pb::RenderRequest {
                document: Some(doc("b = box(10, 20, 5)")),
                width: 64,
                height: 48,
                debug: view.as_str().to_owned(),
                backend: pb::RenderBackend::PathTrace as i32,
                samples: 32,
                xray: true,
                ..Default::default()
            };
            let o = render_options_from_proto(&req);
            assert_eq!(o.debug, Some(view));
            assert_eq!((o.backend, o.samples, o.tonemap, o.xray), (Backend::Raster, 1, ToneMap::None, false), "{view:?}");
            let r = render(&req).unwrap();
            assert_eq!(&r.png[1..4], b"PNG");
            let infos: Vec<&str> = r.diagnostics.iter().map(|d| d.message.as_str()).collect();
            match view {
                DebugView::Faces => assert!(infos.iter().any(|m| m.starts_with("faces: 6 face groups")), "{infos:?}"),
                DebugView::Defects => {
                    assert!(infos.iter().any(|m| m.starts_with("defects: 0 crossing triangles (red), 0 slivers")), "{infos:?}")
                }
                _ => assert!(infos.is_empty(), "{view:?} has nothing to report: {infos:?}"),
            }
        }
        let bad = render(&pb::RenderRequest { document: Some(doc("b = box(1, 1, 1)")), debug: "zebra".into(), ..Default::default() });
        assert!(bad.unwrap_err().to_string().contains("unknown debug view"));
        let both = render(&pb::RenderRequest {
            document: Some(doc("b = box(1, 1, 1)")),
            debug: "faces".into(),
            analysis: "thickness".into(),
            ..Default::default()
        });
        assert!(both.unwrap_err().to_string().contains("one of them"));
    }

    #[test]
    fn render_options_map_samples_threads_and_draft() {
        let o = render_options_from_proto(&pb::RenderRequest { width: 64, height: 48, ..Default::default() });
        assert_eq!((o.samples, o.threads, o.ambient_occlusion), (4, 0, true));
        let o = render_options_from_proto(&pb::RenderRequest { samples: 8, threads: 3, draft: true, ..Default::default() });
        assert_eq!((o.samples, o.threads, o.ambient_occlusion, o.contact_shadow), (8, 3, false, false));
        let o = render_options_from_proto(&pb::RenderRequest { draft: true, ..Default::default() });
        assert_eq!(o.samples, 1);
        let o = render_options_from_proto(&pb::RenderRequest { width: 8000, height: 8000, ..Default::default() });
        assert!(o.width as u64 * o.height as u64 <= limits::RenderLimits::DEFAULT.max_pixels);
    }

    /// `cadpole render --background` and `--environment` reach the renderer:
    /// the colour lands in `RenderOptions::background` verbatim (it is
    /// written to uncovered pixels bit-exactly, so nothing may touch it on
    /// the way), and the flag lands in the path tracer's option, which is
    /// the only backend that reads it. Both requests carry them.
    #[test]
    fn render_options_map_background_and_environment() {
        let o = render_options_from_proto(&pb::RenderRequest::default());
        assert_eq!(o.background, RenderOptions::default().background);
        assert!(!o.path.show_environment, "off by default, so the two backends share a backdrop");

        let bg = pb::Color { r: 1.0, g: 0.5, b: 0.25 };
        let o = render_options_from_proto(&pb::RenderRequest {
            background: Some(bg.clone()),
            show_environment: true,
            ..Default::default()
        });
        assert_eq!(o.background, [1.0, 0.5, 0.25]);
        assert!(o.path.show_environment);

        let o = crate::sweep::options(&pb::SweepRequest { background: Some(bg), show_environment: true, ..Default::default() });
        assert_eq!(o.background, [1.0, 0.5, 0.25]);
        assert!(o.path.show_environment);
    }

    /// The transparent background, the shadow catcher and the supersample
    /// factor reach the renderer from `render` and `sweep` alike, the
    /// factor inside its range.
    #[test]
    fn render_options_map_transparency_and_supersampling() {
        let o = render_options_from_proto(&pb::RenderRequest::default());
        assert!(!o.transparent && !o.shadow_catcher);
        assert_eq!(o.supersample, 1);
        let o = render_options_from_proto(&pb::RenderRequest { transparent: true, shadow_catcher: true, supersample: 9, ..Default::default() });
        assert!(o.transparent && o.shadow_catcher);
        assert_eq!(o.supersample, 4);
        let o = crate::sweep::options(&pb::SweepRequest { transparent: true, shadow_catcher: true, supersample: 2, ..Default::default() });
        assert!(o.transparent && o.shadow_catcher);
        assert_eq!(o.supersample, 2);
    }

    /// The camera a render reports is the camera it was drawn with: a
    /// small block away from the origin, rendered on a transparent
    /// background, has its alpha centroid where the reported matrices put
    /// the block's centre (under a parallel projection, where a block's
    /// outline is symmetric about it), and under perspective its reported
    /// pixel box is the box of the pixels it covers. The ground is the
    /// plane under the model.
    #[test]
    fn the_reported_camera_projects_a_world_point_onto_its_rendered_pixel() {
        let script = "a = box(2, 2, 2).translate((30, -10, 12))\nb = box(1, 1, 1).translate((-20, 15, 0))\n";
        // One box for the point, two far apart for the box of a model.
        let one = "a = box(4, 4, 4).translate((30, -10, 12))\n";
        for (src, view, ortho) in [(one, "az=35,el=25", true), (one, "iso", false), (script, "iso", false)] {
            let r = render(&pb::RenderRequest {
                document: Some(doc(src)),
                width: 160,
                height: 120,
                camera: Some(pb::Camera { preset: view.into(), orthographic: ortho, ..Default::default() }),
                transparent: true,
                no_grid: true,
                no_edges: true,
                ..Default::default()
            })
            .unwrap();
            let cam = r.camera.as_ref().expect("a render reports its camera");
            assert_eq!((cam.width, cam.height), (160, 120));
            let img = cadpole_core::render::Image::from_png(&r.png).unwrap();
            let (mut sx, mut sy, mut sa) = (0.0, 0.0, 0.0);
            let (mut x0, mut y0, mut x1, mut y1) = (f64::MAX, f64::MAX, f64::MIN, f64::MIN);
            for y in 0..120u32 {
                for x in 0..160u32 {
                    let a = img.rgba[((y * 160 + x) * 4 + 3) as usize] as f64;
                    sx += (x as f64 + 0.5) * a;
                    sy += (y as f64 + 0.5) * a;
                    sa += a;
                    if a > 0.0 {
                        x0 = x0.min(x as f64);
                        y0 = y0.min(y as f64);
                        x1 = x1.max(x as f64 + 1.0);
                        y1 = y1.max(y as f64 + 1.0);
                    }
                }
            }
            assert!(sa > 0.0);
            let px = |p: [f64; 3]| {
                let mul = |m: &[f64], v: [f64; 4]| -> Vec<f64> { (0..4).map(|r| (0..4).map(|c| m[r * 4 + c] * v[c]).sum()).collect() };
                let e = mul(&cam.view, [p[0], p[1], p[2], 1.0]);
                let c = mul(&cam.projection, [e[0], e[1], e[2], e[3]]);
                ((c[0] / c[3] + 1.0) * 0.5 * 160.0, (1.0 - c[1] / c[3]) * 0.5 * 120.0)
            };
            let b = &cam.bounds_px;
            assert_eq!(b.len(), 4, "the model is in front of the camera");
            if ortho {
                let (cx, cy) = px([32.0, -8.0, 14.0]);
                assert!((cx - sx / sa).abs() < 1.0 && (cy - sy / sa).abs() < 1.0, "projected ({cx:.2}, {cy:.2}), drawn ({:.2}, {:.2})", sx / sa, sy / sa);
                let lo = cam.bounds_min.clone().unwrap();
                assert_eq!((lo.x, lo.y, lo.z), (30.0, -10.0, 12.0));
                let g = cam.ground_origin.clone().unwrap();
                assert_eq!(g.z, 12.0, "the ground is the plane the model stands on");
                let n = cam.ground_normal.clone().unwrap();
                assert_eq!((n.x, n.y, n.z), (0.0, 0.0, 1.0));
            }
            // The projected corners enclose every covered pixel, closely:
            // each side falls in the outermost pixel the model touches.
            assert!(b[0] <= x0 + 1.0 && b[1] <= y0 + 1.0 && b[2] >= x1 - 1.0 && b[3] >= y1 - 1.0, "{b:?} vs drawn ({x0}, {y0})..({x1}, {y1})");
            // Of one block they are the block's own outline; of two apart,
            // the corners of the box around both, which nothing need fill.
            assert!(src != one || b[0] > x0 - 3.0 && b[1] > y0 - 3.0 && b[2] < x1 + 3.0 && b[3] < y1 + 3.0, "{b:?} vs drawn ({x0}, {y0})..({x1}, {y1})");
        }
    }

    /// The product-shot look (docs/views.md, "The product shot") reaches
    /// the renderer through the four request strings and the two knobs: an
    /// empty request is the studio look every image had; the recipe sets
    /// the rig, the surround (which also turns the environment on, colours
    /// the floor and derives the rasterizer's background through the tone
    /// curve — unless a `background` was given), the glossy floor, Neutral,
    /// the lens and the bloom. A misspelling is one error naming the
    /// choices, from `render` and `sweep` alike, before any evaluation.
    /// The frames the machined textures are laid out in come from the
    /// topology: a cylinder's side is turned about its own axis, its flat
    /// ends about the body's, which is that same axis (the largest face of
    /// revolution); a box, having none, is turned about Z through its
    /// centre; and an extruded side face's `v` runs along the extrusion —
    /// the ruling a `milled` finish scallops along.
    #[test]
    fn face_frames_come_from_the_topology() {
        let (mut ctx, _) = evaluate_document_with(&doc("c = cylinder(10, 20, at=(5, 0, 0))"), &HashMap::new(), 0.0, 0.0);
        let scene = scene_from_context(&mut ctx, &[], &[], &[]);
        let item = &scene.items[0];
        assert_eq!(item.face_frames.len(), item.mesh.face_ids.iter().max().map(|f| *f as usize + 1).unwrap_or(0));
        for f in &item.face_frames {
            assert!((f.axis[2].abs() - 1.0).abs() < 1e-5, "every face of a cylinder is turned about z: {f:?}");
            assert!((f.axis_point[0] - 5.0).abs() < 1e-3 && f.axis_point[1].abs() < 1e-3, "about the cylinder's own axis: {f:?}");
            assert!(f.turned, "every face of a cylinder is one a lathe made: {f:?}");
        }
        let (mut ctx, _) = evaluate_document_with(&doc("b = box(10, 20, 5)"), &HashMap::new(), 0.0, 0.0);
        let scene = scene_from_context(&mut ctx, &[], &[], &[]);
        let item = &scene.items[0];
        assert_eq!(item.face_frames.len(), 6);
        let (lo, hi) = item.mesh.bbox().unwrap();
        let c = (lo + hi) * 0.5;
        let mut sides_along_z = 0;
        let mut turned = 0;
        for f in &item.face_frames {
            assert_eq!(f.axis, [0.0, 0.0, 1.0], "a box has no axis of its own: {f:?}");
            assert!((f.axis_point[0] - c.x as f32).abs() < 1e-3 && (f.axis_point[1] - c.y as f32).abs() < 1e-3, "{f:?}");
            let side = f.origin[2] > lo.z as f32 + 0.1 && f.origin[2] < hi.z as f32 - 0.1;
            if side && f.dir[2].abs() > 0.999 {
                sides_along_z += 1;
            }
            assert_eq!(f.turned, !side, "only the two faces square to the axis are lathe-made: {f:?}");
            turned += usize::from(f.turned);
        }
        assert_eq!(sides_along_z, 4, "the four extruded sides run their v along the extrusion");
        assert_eq!(turned, 2);
    }

    #[test]
    fn render_options_map_the_product_look() {
        use cadpole_core::render::{Backdrop, Floor, Lighting, ToneMap};
        let o = render_options_from_proto(&pb::RenderRequest::default());
        assert_eq!((o.lighting, o.backdrop, o.tonemap, o.bloom), (Lighting::Studio, None, ToneMap::Aces, false));
        assert_eq!((o.path.ground, o.path.ground_roughness, o.path.dof_f_number), (true, 0.85, 0.0));
        assert_eq!(o.path.ground_albedo, cadpole_core::render::PathTraceOptions::default().ground_albedo);

        let req = pb::RenderRequest {
            lighting: "product".into(),
            backdrop: "studio".into(),
            floor: "gloss".into(),
            tonemap: "neutral".into(),
            dof_f_number: 8.0,
            bloom: true,
            ..Default::default()
        };
        let o = render_options_from_proto(&req);
        assert_eq!((o.lighting, o.backdrop, o.tonemap, o.bloom), (Lighting::Product, Some(Backdrop::Studio), ToneMap::Neutral, true));
        assert_eq!((o.path.ground, o.path.ground_roughness, o.path.dof_f_number), (true, 0.35, 8.0));
        assert_eq!(o.path.ground_albedo, [0.40; 3]);
        assert!(o.path.show_environment, "a backdrop is something to show");
        assert_eq!(o.background, Backdrop::Studio.background(1.0, ToneMap::Neutral), "the rasterizer's backdrop is the horizon, tone-mapped");
        let o = render_options_from_proto(&pb::RenderRequest { background: Some(pb::Color { r: 1.0, g: 1.0, b: 1.0 }), ..req.clone() });
        assert_eq!(o.background, [1.0; 3], "an explicit background wins over the backdrop's");
        assert!(!o.path.show_environment, "and is what shows behind the model, edge pixels included");
        let o = render_options_from_proto(&pb::RenderRequest { floor: "none".into(), backdrop: "#ff8800".into(), ..req.clone() });
        assert!(!o.path.ground);
        assert!(matches!(o.backdrop, Some(Backdrop::Color(c)) if c[0] > 0.99 && c[1] > 0.2 && c[1] < 0.3 && c[2] == 0.0), "{:?}", o.backdrop);
        assert_eq!(Floor::parse("gloss"), Some(Floor::Gloss));

        // The sweep takes the same four.
        let s = crate::sweep::options(&pb::SweepRequest { lighting: "product".into(), tonemap: "neutral".into(), ..Default::default() });
        assert_eq!((s.lighting, s.tonemap), (Lighting::Product, ToneMap::Neutral));

        // Misspellings are diagnostics, and nothing is evaluated first.
        for (field, req) in [
            ("lighting", pb::RenderRequest { lighting: "sunny".into(), ..Default::default() }),
            ("backdrop", pb::RenderRequest { backdrop: "#12".into(), ..Default::default() }),
            ("floor", pb::RenderRequest { floor: "carpet".into(), ..Default::default() }),
            ("tonemap", pb::RenderRequest { tonemap: "filmic".into(), ..Default::default() }),
        ] {
            let err = render(&pb::RenderRequest { document: Some(doc("b = box(1, 1, 1)")), ..req }).unwrap_err().to_string();
            assert!(err.contains(&format!("unknown {field}")) && err.contains("expected one of"), "{field}: {err}");
        }
        let err = Look::from_sweep(&pb::SweepRequest { floor: "carpet".into(), ..Default::default() }).unwrap_err().to_string();
        assert!(err.contains("unknown floor"), "{err}");
    }

    /// The sun: a request's `sun` becomes a `SunLight` along the
    /// compass direction turned by `north`; a document's
    /// `render_defaults(sun=, north=)` stands in where the request is
    /// silent, and the request wins field by field; a misspelt sun or
    /// section is refused before evaluation; a section cuts the scene.
    #[test]
    fn render_options_map_the_sun_and_the_document_defaults() {
        let o = render_options_from_proto(&pb::RenderRequest::default());
        assert_eq!(o.sun, None);
        // A hand-placed sun: due south (az 180), 45° up, in a model whose
        // north is +y, points along −y and +z equally.
        let o = render_options_from_proto(&pb::RenderRequest { sun: "az=180,el=45".into(), ..Default::default() });
        let s = o.sun.unwrap();
        assert!(s.dir[0].abs() < 1e-6 && (s.dir[1] + 0.7071).abs() < 1e-3 && (s.dir[2] - 0.7071).abs() < 1e-3, "{s:?}");
        // North along +x: the same sun points along −x.
        let o = render_options_from_proto(&pb::RenderRequest { sun: "az=180,el=45".into(), north_degrees: 90.0, ..Default::default() });
        let s = o.sun.unwrap();
        assert!((s.dir[0] + 0.7071).abs() < 1e-3 && s.dir[1].abs() < 1e-6, "{s:?}");
        // A place, a date and a time: midsummer noon at 38 N is
        // high in the south.
        let o = render_options_from_proto(&pb::RenderRequest { sun: "38,-121,2026-06-21,13:00,-7".into(), ..Default::default() });
        let s = o.sun.unwrap();
        assert!(s.dir[2] > 0.95 && s.dir[1] < 0.0, "{s:?}");

        // The document's defaults.
        let defaults = RenderDefaults {
            sun: Some(SunSpec::angles(90.0, 30.0)),
            north_degrees: Some(90.0),
            camera: Some(cadpole_core::context::CameraSpec { eye: Vec3::new(1.0, 2.0, 3.0), target: Vec3::ZERO, up: Vec3::Z, fov_degrees: 50.0 }),
        };
        let look = Look::from_render(&pb::RenderRequest::default()).unwrap();
        let mut o = RenderOptions::default();
        apply_document_defaults(&mut o, &look, Some(&defaults));
        // East (az 90) in a model whose north is +x is −y.
        let s = o.sun.unwrap();
        assert!(s.dir[1] < -0.8 && s.dir[0].abs() < 1e-6, "{s:?}");
        // The request's north wins over the document's; its sun too.
        let look = Look::from_render(&pb::RenderRequest { north_degrees: 180.0, ..Default::default() }).unwrap();
        apply_document_defaults(&mut o, &look, Some(&defaults));
        assert!(o.sun.unwrap().dir[0] < -0.8, "{:?}", o.sun);
        let look = Look::from_render(&pb::RenderRequest { sun: "az=0,el=10".into(), ..Default::default() }).unwrap();
        apply_document_defaults(&mut o, &look, Some(&defaults));
        assert!(o.sun.unwrap().dir[0] > 0.9, "the request's sun, under the document's north: {:?}", o.sun);
        // The camera: an unset request takes the document's free camera; a
        // preset does not.
        let c = document_camera(&None, Some(&defaults)).unwrap();
        assert!(c.preset.is_empty() && c.eye.is_some() && c.fov_degrees == 50.0);
        assert!(is_free_camera(&Some(c)));
        let iso = Some(pb::Camera { preset: "iso".into(), ..Default::default() });
        assert_eq!(document_camera(&iso, Some(&defaults)), iso);
        assert!(!is_free_camera(&iso));
        assert_eq!(document_camera(&None, None), None);

        // Refusals, before evaluation.
        for (bad, needle) in [
            (pb::RenderRequest { sun: "az=10".into(), ..Default::default() }, "az= and el="),
            (pb::RenderRequest { sun: "37,0".into(), ..Default::default() }, "lat,lon,YYYY-MM-DD,HH:MM"),
            (pb::RenderRequest { section: "w=1".into(), ..Default::default() }, "x, y or z"),
        ] {
            let err = render(&pb::RenderRequest { document: Some(doc("b = box(1, 1, 1)")), ..bad }).unwrap_err().to_string();
            assert!(err.contains(needle), "{err}");
        }

        // A section: the box cut at z = 4 keeps its bottom 4 mm, so the
        // scene's bounding box ends there and the image differs.
        let whole = render(&pb::RenderRequest { document: Some(doc("b = box(10, 10, 10)")), width: 64, height: 48, draft: true, ..Default::default() }).unwrap();
        let cut = render(&pb::RenderRequest { document: Some(doc("b = box(10, 10, 10)")), width: 64, height: 48, draft: true, section: "z=4".into(), ..Default::default() }).unwrap();
        assert_ne!(whole.png, cut.png);
        assert!(!cut.diagnostics.iter().any(|d| d.severity() == pb::Severity::Warning), "{:?}", cut.diagnostics);
        let (mut ctx, _) = evaluate_document_with(&doc("b = box(10, 10, 10)"), &HashMap::new(), 0.0, 0.0);
        let mut scene = scene_from_context(&mut ctx, &[], &[], &[]);
        let warnings = section_scene(&mut scene, &Section::parse("z=4").unwrap().unwrap(), ctx.tolerance);
        assert!(warnings.is_empty());
        let (lo, hi) = scene.bbox.unwrap();
        assert!((hi.z - 4.0).abs() < 1e-4 && lo.z.abs() < 1e-4, "{lo:?} {hi:?}");
        assert!(scene.items[0].edges.iter().all(|e| e.iter().all(|p| p.z <= 4.0 + 1e-6)), "edges are clipped");

        // A sun the document declares reaches the render: the image differs
        // from the plain one, and the sweep's variants carry the defaults.
        let src = "b = box(10, 10, 10)\nrender_defaults(sun=sun(az=200, el=50))\n";
        let lit = render(&pb::RenderRequest { document: Some(doc(src)), width: 64, height: 48, draft: true, ..Default::default() }).unwrap();
        assert!(!lit.diagnostics.iter().any(|d| d.severity() == pb::Severity::Error), "{:?}", lit.diagnostics);
        assert_ne!(whole.png, lit.png);
        let m = lit.model.unwrap();
        let d = m.render_defaults.unwrap();
        assert_eq!((d.sun.as_ref().unwrap().azimuth, d.sun.as_ref().unwrap().elevation), (200.0, 50.0));
    }

    #[test]
    fn parallel_sweep_matches_sequential_and_keeps_order() {
        let doc = doc("w = param(\"w\", 10)\nh = param(\"h\", 5)\nb = box(w, 20, h)");
        let grid = sweep_grid(&[
            ("w".into(), vec![ParamValue::Number(10.0), ParamValue::Number(20.0), ParamValue::Number(30.0)]),
            ("h".into(), vec![ParamValue::Number(4.0), ParamValue::Number(8.0)]),
        ]);
        let fixed = HashMap::new();
        let one = sweep_scenes(&doc, &grid, &fixed, 0.0, 0.0, 1, &[]);
        let many = sweep_scenes(&doc, &grid, &fixed, 0.0, 0.0, 4, &[]);
        assert_eq!(one.len(), 6);
        for (a, b) in one.iter().zip(many.iter()) {
            assert_eq!(a.label, b.label, "variants stay in grid order");
            assert_eq!(a.scene.items.len(), b.scene.items.len());
            assert_eq!(a.scene.items[0].mesh.positions, b.scene.items[0].mesh.positions);
            assert_eq!(a.scene.bbox.map(|(l, _)| l.x), b.scene.bbox.map(|(l, _)| l.x));
        }
        // The grid really varies the model.
        let widths: Vec<f64> = one.iter().filter_map(|v| v.scene.bbox.map(|(lo, hi)| hi.x - lo.x)).collect();
        assert!(widths.iter().any(|w| (w - 30.0).abs() < 1e-6), "{widths:?}");
    }

    #[test]
    fn sweep_grid_is_a_cartesian_product() {
        let g = sweep_grid(&[
            ("a".into(), vec![ParamValue::Number(1.0), ParamValue::Number(2.0)]),
            ("b".into(), vec![ParamValue::Number(3.0), ParamValue::Number(4.0), ParamValue::Number(5.0)]),
        ]);
        assert_eq!(g.len(), 6);
        // The first spec varies fastest, so the first three cells are one row
        // of a two-column sheet only if `a` is the column axis -- a=1,b=3 then
        // a=2,b=3 then a=1,b=4.
        let cells: Vec<(f64, f64)> = g
            .iter()
            .map(|c| match (&c[0].1, &c[1].1) {
                (ParamValue::Number(a), ParamValue::Number(b)) => (*a, *b),
                _ => unreachable!(),
            })
            .collect();
        assert_eq!(cells, vec![(1.0, 3.0), (2.0, 3.0), (1.0, 4.0), (2.0, 4.0), (1.0, 5.0), (2.0, 5.0)]);
        // Every combination still names its parameters in spec order.
        assert!(g.iter().all(|c| c[0].0 == "a" && c[1].0 == "b"));
    }

    #[test]
    fn sweep_grid_of_no_specs_is_one_empty_combination() {
        assert_eq!(sweep_grid(&[]), vec![Vec::new()]);
        assert!(sweep_grid(&[("a".into(), vec![])]).is_empty());
    }

    #[test]
    fn parse_values() {
        assert_eq!(parse_value("3.5"), ParamValue::Number(3.5));
        assert_eq!(parse_value("true"), ParamValue::Bool(true));
        assert_eq!(parse_value("(1,2,3)"), ParamValue::Vector(Vec3::new(1.0, 2.0, 3.0)));
        assert_eq!(parse_value("wood"), ParamValue::Text("wood".into()));
    }

    const SVG: &[u8] = br#"<svg xmlns="http://www.w3.org/2000/svg" width="20mm" height="20mm" viewBox="0 0 20 20"><rect x="0" y="0" width="20" height="20"/></svg>"#;

    fn doc_with_asset(pack: bool) -> pb::Document {
        let a = cadpole_assets::Asset::describe("logo.svg", SVG);
        let mut doc = document_from_scripts(
            "t",
            vec![(
                "main.star".to_owned(),
                "s = import_svg(asset(\"logo.svg\"))\nb = extrude(s, 2, name=\"plate\")\n".to_owned(),
            )],
        );
        doc.assets = vec![asset_to_proto(&a, if pack { SVG.to_vec() } else { Vec::new() })];
        doc
    }

    #[test]
    fn a_packed_asset_round_trips_through_the_protobuf_and_evaluates() {
        let doc = doc_with_asset(true);
        let bytes = doc.encode_to_vec();
        let back = pb::Document::decode(bytes.as_slice()).unwrap();
        assert_eq!(back.assets.len(), 1);
        let a = &back.assets[0];
        assert_eq!(a.name, "logo.svg");
        assert_eq!(a.size, SVG.len() as u64);
        assert_eq!(a.kind(), pb::AssetKind::Svg);
        assert_eq!(a.mime, "image/svg+xml");
        assert_eq!(a.sha256, cadpole_assets::sha256_hex(SVG));
        assert_eq!(a.data, SVG);

        // The document alone is enough to evaluate: the bytes travelled with it.
        let resp = evaluate(&pb::EvaluateRequest { document: Some(back), skip_mesh: true, ..Default::default() });
        let model = resp.model.unwrap();
        let errors: Vec<&str> = model
            .diagnostics
            .iter()
            .filter(|d| d.severity() == pb::Severity::Error)
            .map(|d| d.message.as_str())
            .collect();
        assert!(errors.is_empty(), "{errors:?}");
        assert!(model.features.iter().any(|f| f.kind == "import_svg"));
        assert_eq!(model.bodies.len(), 1);
    }

    #[test]
    fn an_unpacked_asset_needs_its_bytes_in_the_request() {
        // Names and hashes only: this is what a share link or a render-host
        // request carries.
        let doc = doc_with_asset(false);
        assert!(doc.assets[0].data.is_empty());
        let resp = evaluate(&pb::EvaluateRequest {
            document: Some(doc.clone()),
            skip_mesh: true,
            ..Default::default()
        });
        let msgs: Vec<String> =
            resp.model.unwrap().diagnostics.iter().map(|d| d.message.clone()).collect();
        assert!(msgs.iter().any(|m| m.contains("is not loaded")), "{msgs:?}");

        // Feeding the bytes separately, keyed by hash, makes it evaluate.
        let mut asset_data = HashMap::new();
        asset_data.insert(cadpole_assets::sha256_hex(SVG), SVG.to_vec());
        let resp = evaluate(&pb::EvaluateRequest {
            document: Some(doc),
            skip_mesh: true,
            asset_data,
            ..Default::default()
        });
        let model = resp.model.unwrap();
        assert!(
            model.diagnostics.iter().all(|d| d.severity() != pb::Severity::Error),
            "{:?}",
            model.diagnostics
        );
        assert_eq!(model.bodies.len(), 1);
    }

    #[test]
    fn an_assets_hash_is_recomputed_from_its_bytes_not_believed() {
        // A document that claims one hash and carries different bytes must
        // resolve to the bytes it actually has, not to whatever else shares
        // the claimed hash.
        let mut doc = doc_with_asset(true);
        doc.assets[0].sha256 = "0".repeat(64);
        let store = assets_from_proto(&doc, &HashMap::new());
        let (record, bytes) = store.get("logo.svg").expect("the packed bytes resolve");
        assert_eq!(record.sha256, cadpole_assets::sha256_hex(SVG));
        assert_eq!(bytes.as_ref(), SVG);
    }

    #[test]
    fn asset_kinds_round_trip_both_ways() {
        use cadpole_assets::AssetKind as K;
        for k in [K::Unknown, K::Svg, K::Dxf, K::Stl, K::Obj, K::ThreeMf, K::Step, K::Image, K::Font] {
            assert_eq!(asset_kind_from_proto(asset_kind_to_proto(k)), k);
        }
        assert_eq!(kind_from_name("asset"), pb::EntityKind::Asset);
        assert_eq!(kind_name(pb::EntityKind::Asset), "asset");
    }
}
