//! `import_step`, `import_mesh` and `import_report` (docs/starlark-api.md):
//! a STEP file as exact B-rep bodies, an
//! STL/OBJ/3MF/glTF as mesh bodies with a repair report, through
//! `cadpole-import`.
//!
//! The source is an `asset(...)` or the asset's name as a string — the
//! document-relative path a script would write is the asset's name, since
//! the CLI packs `./assets/<name>` next to the script and names are bare by
//! design (docs/security.md). Each import is one feature whose args record
//! the asset, its hash, the options and the importer's version, and one
//! kernel op through `Context::produce`, keyed by the content hash:
//! on a memo hit nothing is parsed, and the bodies, the notes and the
//! report come back from the entry. Warnings — a mesh that is not closed,
//! a solid that was turned outward — are recorded as feature args inside
//! the op and raised after it returns, so a hit warns too.

use std::collections::BTreeMap;
use std::sync::Arc;

use cadpole_core::context::{Body, Diagnostic, NameSource, Severity, fmt_num};
use cadpole_core::memo::{KeyBuilder, Produced, ProducedBody};
use cadpole_core::{FeatureId, CadpoleError, kernel};
use cadpole_import::mesh::{self, Appearance, Repair, Report, Resources, Unit, Up};
use cadpole_import::{IMPORT_BUDGET_MS, IMPORTER_VERSION, Level, step};
use starlark::environment::GlobalsBuilder;
use starlark::eval::Evaluator;
use starlark::starlark_module;
use starlark::values::{Value, ValueLike};
use starlark::values::float::UnpackFloat;

use crate::globals::{asset_bytes, bodies_value};
use crate::store::{Store, call_span, store};
use crate::values::{KAsset, KBody, KImportReport};

/// The asset a source argument names.
pub(crate) fn source_name(v: Value<'_>, what: &str) -> anyhow::Result<String> {
    if let Some(a) = v.downcast_ref::<KAsset>() {
        return Ok(a.name.clone());
    }
    if let Some(s) = v.unpack_str() {
        return Ok(s.to_owned());
    }
    anyhow::bail!(
        "{what}: the source must be an asset(...) or the asset's name as a string (the file under ./assets), got {}",
        v.get_type()
    )
}

/// The wall-clock budget one import may take: the importer's own cap, or
/// what is left of the evaluation's deadline when that is nearer.
pub(crate) fn budget_ms(st: &Store) -> u64 {
    let total = st.deadline.budget_ms();
    if total == 0 {
        return IMPORT_BUDGET_MS;
    }
    let left = total.saturating_sub(st.deadline.elapsed_ms() as u64).max(1);
    IMPORT_BUDGET_MS.min(left)
}

/// The record and bytes of the asset `name`, checked to be one of `kinds`.
pub(crate) fn asset_of(st: &Store, name: &str, what: &str, kinds: &[cadpole_assets::AssetKind]) -> anyhow::Result<(cadpole_assets::Asset, Arc<[u8]>)> {
    let bytes = asset_bytes(st, name, what)?;
    let record = st.assets.record(name).ok_or_else(|| anyhow::anyhow!("{what}: no asset named {name:?}"))?;
    if !kinds.contains(&record.kind) {
        let other = match record.kind {
            cadpole_assets::AssetKind::Step => "a STEP file: use import_step".to_owned(),
            cadpole_assets::AssetKind::Stl | cadpole_assets::AssetKind::Obj | cadpole_assets::AssetKind::ThreeMf => {
                format!("a {} mesh: use import_mesh", record.kind.name().to_uppercase())
            }
            cadpole_assets::AssetKind::Gltf => "a glTF mesh: use import_mesh".to_owned(),
            cadpole_assets::AssetKind::Dxf => "a DXF drawing: use import_dxf".to_owned(),
            cadpole_assets::AssetKind::Svg => "an SVG: use import_svg".to_owned(),
            k => format!("sniffed as {}, not a file this reads", k.name()),
        };
        anyhow::bail!("{what}: asset {name:?} is {other}");
    }
    Ok((record, bytes))
}

/// The `.bin` buffers a glTF names beside itself, `(uri, asset name)`,
/// remembered by content hash: the list is needed for the memo key before
/// the memo can answer, and a `.gltf` whose buffers are inline is JSON of
/// any size, so each distinct file is parsed for it once per process.
fn gltf_buffers(sha256: &str, bytes: &Arc<[u8]>) -> Result<Vec<(String, String)>, String> {
    use std::sync::{Mutex, OnceLock};
    static SEEN: OnceLock<Mutex<std::collections::HashMap<String, Result<Vec<(String, String)>, String>>>> = OnceLock::new();
    let seen = SEEN.get_or_init(|| Mutex::new(std::collections::HashMap::new()));
    if let Some(hit) = seen.lock().ok().and_then(|m| m.get(sha256).cloned()) {
        return hit;
    }
    let got = cadpole_import::gltf::external_buffers(bytes);
    if let Ok(mut m) = seen.lock() {
        if m.len() >= 256 {
            m.clear();
        }
        m.insert(sha256.to_owned(), got.clone());
    }
    got
}

/// The warnings an op recorded as `warn.<i>` args, raised on the feature
/// after the op — so a memo hit, which replays the args, raises them too.
pub(crate) fn raise_recorded_warnings(ctx: &mut cadpole_core::Context, fid: &FeatureId, span: Option<cadpole_core::context::Span>) {
    let mut messages: Vec<(usize, String)> = Vec::new();
    if let Ok(f) = ctx.feature(fid) {
        for (k, v) in &f.args {
            if let Some(i) = k.strip_prefix("warn.").and_then(|i| i.parse::<usize>().ok()) {
                messages.push((i, v.clone()));
            }
        }
    }
    messages.sort();
    for (_, message) in messages {
        let d = Diagnostic { severity: Severity::Warning, message, span: span.clone(), feature: Some(fid.clone()) };
        if let Ok(f) = ctx.feature_mut(fid) {
            f.diagnostics.push(d.clone());
        }
        ctx.diagnostics.push(d);
    }
}

/// One line for a note at each level, inside the op: an info note goes to
/// the kernel's note channel (attached to the feature as info when the op
/// ends, replayed on a hit); a warning is recorded as an arg.
pub(crate) fn record_note(ctx: &mut cadpole_core::Context, fid: &FeatureId, level: Level, message: &str, warned: &mut usize) {
    match level {
        Level::Info => kernel::notes::push(message.to_owned()),
        Level::Warning => {
            ctx.set_feature_arg(fid, &format!("warn.{warned}"), message);
            *warned += 1;
        }
    }
}

#[starlark_module]
pub(crate) fn import_globals(builder: &mut GlobalsBuilder) {
    /// Import a STEP file (ISO 10303-21, AP203/AP214/AP242) as exact B-rep
    /// bodies: one body per solid in the file, each a boolean operand,
    /// STEP-exportable and classified face by face like any Cadpole body.
    /// `source` is an `asset("part.step")` or the asset's name. Faces are
    /// named `<name>/face[k]` by their position in the file; a face the
    /// file itself names gets that name as an alias. `tolerance` overrides
    /// the vertex-matching distance the file declares, in mm.
    ///
    /// What is refused, each naming the STEP entity: an open shell; a face
    /// whose surface or curve Cadpole cannot convert (offset and trimmed
    /// surfaces, hyperbolas, parabolas, parameter-space-only edges) — the
    /// whole solid is refused, because a shell with a face missing is a
    /// hole every later check would blame on you; a placement whose
    /// reference direction is its axis. Assemblies are not flattened yet:
    /// every solid in the file comes back where the file put it.
    fn import_step<'v>(
        #[starlark(require = pos)] source: Value<'v>,
        #[starlark(require = named)] name: Option<&str>,
        #[starlark(require = named)] layer: Option<Value<'v>>,
        #[starlark(require = named)] tolerance: Option<UnpackFloat>,
        eval: &mut Evaluator<'v, '_, '_>,
    ) -> anyhow::Result<Value<'v>> {
        let asset = source_name(source, "import_step")?;
        let tol = match tolerance {
            Some(t) if t.0.is_finite() && t.0 > 0.0 => Some(t.0),
            Some(t) => anyhow::bail!("import_step: tolerance must be a positive number of millimetres, got {}", t.0),
            None => None,
        };
        let span = call_span(eval);
        let st = store(eval)?;
        let (record, bytes) = asset_of(st, &asset, "import_step", &[cadpole_assets::AssetKind::Step])?;
        let budget = budget_ms(st);
        let mut ctx = st.ctx.borrow_mut();
        let mut args = BTreeMap::new();
        args.insert("asset".into(), asset.clone());
        args.insert("sha256".into(), record.sha256.clone());
        if let Some(t) = tol {
            args.insert("tolerance".into(), fmt_num(t));
        }
        args.insert("importer_version".into(), IMPORTER_VERSION.to_string());
        let fid = ctx.new_feature("import_step", name.map(str::to_owned), vec![], args, span.clone())?;
        let mut key = KeyBuilder::new("import_step");
        key.put(&fid).str(&record.sha256).f64(tol.unwrap_or(0.0)).u64(IMPORTER_VERSION as u64);
        let fid_run = fid.clone();
        let asset_run = asset.clone();
        let ids = ctx.produce("import_step", &fid, span.clone(), key.finish(), name.map(str::to_owned), move |ctx| {
            let im = step::import(&bytes, &step::Options { tolerance: tol, budget_ms: budget })
                .map_err(|e| CadpoleError::invalid(format!("import_step({asset_run:?}): {e}")))?;
            let mut warned = 0;
            for n in &im.notes {
                record_note(ctx, &fid_run, n.level, &n.message, &mut warned);
            }
            let faces: usize = im.solids.iter().map(|s| s.faces.len()).sum();
            let split: usize = im.solids.iter().map(|s| s.split_edges).sum();
            let lifted: usize = im.solids.iter().map(|s| s.lifted_edges).sum();
            let seams: usize = im.solids.iter().map(|s| s.seam_faces).sum();
            let turned: usize = im.solids.iter().map(|s| s.flipped_faces).sum();
            let inverted: usize = im.solids.iter().filter(|s| s.inverted).count();
            ctx.set_feature_arg(&fid_run, "solids", im.solids.len());
            ctx.set_feature_arg(&fid_run, "faces", faces);
            ctx.set_feature_arg(&fid_run, "entities", im.entities);
            ctx.set_feature_arg(&fid_run, "unit", &im.unit);
            ctx.set_feature_arg(&fid_run, "schema", &im.schema);
            ctx.set_feature_arg(&fid_run, "tolerance_used", fmt_num(im.tolerance));
            let labels: Vec<String> = im.solids.iter().map(|s| s.label.clone()).filter(|l| !l.is_empty()).collect();
            if !labels.is_empty() {
                ctx.set_feature_arg(&fid_run, "solids_named", labels.join(", "));
            }
            kernel::notes::push(format!(
                "import_step: {} solid{} with {faces} face{} from {} entities (schema {}, unit {}, tolerance {} mm){}{}{}{}{}",
                im.solids.len(),
                if im.solids.len() == 1 { "" } else { "s" },
                if faces == 1 { "" } else { "s" },
                im.entities,
                if im.schema.is_empty() { "unknown" } else { im.schema.as_str() },
                im.unit,
                fmt_num(im.tolerance),
                if split > 0 { format!("; {split} full-circle edge{} split", if split == 1 { "" } else { "s" }) } else { String::new() },
                if lifted > 0 { format!("; {lifted} edge{} written off {} surfaces lifted onto them", if lifted == 1 { "" } else { "s" }, if lifted == 1 { "its" } else { "their" }) } else { String::new() },
                if seams > 0 { format!("; {seams} seam face{} split in two", if seams == 1 { "" } else { "s" }) } else { String::new() },
                if turned > 0 { format!("; {turned} face{} turned to agree with {} neighbours", if turned == 1 { "" } else { "s" }, if turned == 1 { "its" } else { "their" }) } else { String::new() },
                if inverted > 0 { format!("; {inverted} shell{} turned outward", if inverted == 1 { "" } else { "s" }) } else { String::new() },
            ));
            let bodies = im
                .solids
                .into_iter()
                .map(|s| {
                    let names: Vec<(String, Vec<String>)> = s
                        .faces
                        .iter()
                        .map(|f| {
                            let suffix = if f.piece > 0 { format!("#{}", f.piece) } else { String::new() };
                            let aliases = if f.label.is_empty() { Vec::new() } else { vec![format!("{fid_run}/{}", f.label)] };
                            (format!("{fid_run}/face[{}]{suffix}", f.index), aliases)
                        })
                        .collect();
                    ProducedBody::new(Body::Brep(s.solid), NameSource::Listed { names: Arc::new(names) })
                })
                .collect();
            Ok(Produced::bodies(bodies))
        })?;
        raise_recorded_warnings(&mut ctx, &fid, span);
        crate::layers::tag_ids(&mut ctx, layer, &ids, "import_step")?;
        bodies_value(eval.heap(), &ctx, ids)
    }

    /// Import an STL (binary or ASCII), OBJ, 3MF or glTF (GLB or `.gltf`)
    /// file as mesh bodies: one body per object in the file (per ASCII-STL
    /// `solid`, OBJ `o`, 3MF build item — a Bambu Studio project's print
    /// objects, each with its parts as named faces — and per glTF node that
    /// places a mesh), with the file's groups as faces named
    /// `<name>/<group>` (an OBJ `g`, a 3MF part, a glTF primitive's
    /// material) and `<name>/surface` when it has none. `source` is an
    /// `asset("scan.glb")` or the asset's name; a `.gltf`'s `.bin` buffers
    /// are assets beside it, found by file name.
    ///
    /// The importer measures and never mends: `repair="report"` (the
    /// default) keeps the triangles exactly as written and says what
    /// `cadpole check`'s solid test found (open edges, flipped edges,
    /// crossings, folds, shells, genus, volume); `"weld"` merges coincident
    /// vertices — the one repair the mesh kernel performs itself — and
    /// turns a closed, consistently wound, inside-out mesh outward; `"off"`
    /// skips the pairwise crossing search on a mesh too big to want it. A
    /// hole is never filled and a fold never removed: a mesh that is not
    /// closed renders and exports, warns, and is refused by booleans and
    /// `cadpole check` as every non-solid is. `import_report(body)` returns
    /// the numbers.
    ///
    /// `up` and `units` say what frame the file was written in, when it is
    /// not the format's own: glTF is +Y up in metres by its specification
    /// and is turned into Cadpole's +Z up in millimetres, (x, y, z) →
    /// 1000·(x, −z, y); STL and OBJ are taken as +Z up in millimetres and
    /// 3MF in its `<model unit>`. `up="y"` or `"z"`; `units` is a length
    /// unit (`"mm"`, `"cm"`, `"m"`, `"inch"`, `"ft"`, … — `dxf_units`'
    /// members, and `units.m`, `units.mm` read the same). A glTF material's
    /// base colour, roughness, metallic and emissive factors become the
    /// body's material, and per-vertex colour (glTF `COLOR_0`, an OBJ's
    /// `v x y z r g b`) is carried on the mesh and drawn by the viewer and
    /// `cadpole render`.
    fn import_mesh<'v>(
        #[starlark(require = pos)] source: Value<'v>,
        #[starlark(require = named)] name: Option<&str>,
        #[starlark(require = named)] layer: Option<Value<'v>>,
        #[starlark(require = named)] repair: Option<&str>,
        #[starlark(require = named)] up: Option<&str>,
        #[starlark(require = named)] units: Option<&str>,
        eval: &mut Evaluator<'v, '_, '_>,
    ) -> anyhow::Result<Value<'v>> {
        let asset = source_name(source, "import_mesh")?;
        let repair = Repair::parse(repair.unwrap_or("")).map_err(|e| anyhow::anyhow!("import_mesh: {e}"))?;
        let up = Up::parse(up.unwrap_or("")).map_err(|e| anyhow::anyhow!("import_mesh: {e}"))?;
        let units = match units.map(str::trim) {
            None | Some("") => None,
            Some("ft_in") => anyhow::bail!(
                "import_mesh: units=\"ft_in\" is a way of writing lengths, not a file's unit; a file in feet is units=\"ft\", in inches units=\"inch\""
            ),
            Some(u) => Some(Unit::parse(u).map_err(|e| anyhow::anyhow!("import_mesh: units: {e}"))?).filter(|u| *u != Unit::Auto),
        };
        let span = call_span(eval);
        let st = store(eval)?;
        let (record, bytes) = asset_of(
            st,
            &asset,
            "import_mesh",
            &[
                cadpole_assets::AssetKind::Stl,
                cadpole_assets::AssetKind::Obj,
                cadpole_assets::AssetKind::ThreeMf,
                cadpole_assets::AssetKind::Gltf,
            ],
        )?;
        // A `.gltf`'s buffers beside it: each an asset, found by its file
        // name, hashed into the key so a changed `.bin` is a new import.
        let mut resources = Resources::default();
        let mut buffers: Vec<(String, String)> = Vec::new();
        if record.kind == cadpole_assets::AssetKind::Gltf {
            for (uri, file) in gltf_buffers(&record.sha256, &bytes).map_err(|e| anyhow::anyhow!("import_mesh({asset:?}): {e}"))? {
                let (brec, bbytes) = match (st.assets.record(&file), asset_bytes(st, &file, "import_mesh")) {
                    (Some(r), Ok(b)) => (r, b),
                    _ => anyhow::bail!(
                        "import_mesh({asset:?}): the glTF's buffer {uri:?} is the file {file:?}, which is not an asset of this document; put it in ./assets beside {asset:?}"
                    ),
                };
                buffers.push((file.clone(), brec.sha256.clone()));
                resources.0.insert(file, bbytes);
            }
        }
        let budget = budget_ms(st);
        let mut ctx = st.ctx.borrow_mut();
        let weld_tolerance = ctx.tolerance;
        let mut args = BTreeMap::new();
        args.insert("asset".into(), asset.clone());
        args.insert("sha256".into(), record.sha256.clone());
        args.insert("repair".into(), repair.name().to_owned());
        if let Some(u) = up {
            args.insert("up".into(), u.name().to_owned());
        }
        if let Some(u) = units {
            args.insert("units".into(), u.name().to_owned());
        }
        for (k, (file, sha)) in buffers.iter().enumerate() {
            args.insert(format!("buffer[{k}]"), file.clone());
            args.insert(format!("buffer[{k}].sha256"), sha.clone());
        }
        args.insert("importer_version".into(), IMPORTER_VERSION.to_string());
        let fid = ctx.new_feature("import_mesh", name.map(str::to_owned), vec![], args, span.clone())?;
        let mut key = KeyBuilder::new("import_mesh");
        // The tolerance is an input only to `repair.weld`'s second pass; the
        // other two read the triangles as written. Keying it for them would make
        // the web's coarse-first pass (0.4 mm) re-import a scan it has in the
        // memo at 0.05.
        key.put(&fid)
            .str(&record.sha256)
            .str(repair.name())
            .f64(if repair == Repair::Weld { weld_tolerance } else { 0.0 })
            .str(up.map(Up::name).unwrap_or(""))
            .str(units.map(Unit::name).unwrap_or(""))
            .u64(IMPORTER_VERSION as u64);
        for (file, sha) in &buffers {
            key.str(file).str(sha);
        }
        let fid_run = fid.clone();
        let asset_run = asset.clone();
        let ids = ctx.produce("import_mesh", &fid, span.clone(), key.finish(), name.map(str::to_owned), move |ctx| {
            let opts = mesh::Options {
                repair,
                weld_tolerance,
                feature: fid_run.0.clone(),
                budget_ms: budget,
                up,
                units,
                resources,
            };
            let im = mesh::import_shared(bytes, &asset_run, &opts)
                .map_err(|e| CadpoleError::invalid(format!("import_mesh({asset_run:?}): {e}")))?;
            let mut warned = 0;
            for n in &im.notes {
                record_note(ctx, &fid_run, n.level, &n.message, &mut warned);
            }
            ctx.set_feature_arg(&fid_run, "bodies", im.bodies.len());
            ctx.set_feature_arg(&fid_run, "format", im.format.name());
            ctx.set_feature_arg(&fid_run, "objects", im.bodies.iter().map(|b| b.name.as_str()).collect::<Vec<_>>().join(", "));
            let mut bodies = Vec::with_capacity(im.bodies.len());
            for (k, b) in im.bodies.into_iter().enumerate() {
                for (key, value) in b.report.to_args(&format!("report[{k}].")) {
                    ctx.set_feature_arg(&fid_run, &key, value);
                }
                if let Some(look) = &b.appearance {
                    for (key, value) in look.to_args(&format!("look[{k}].")) {
                        ctx.set_feature_arg(&fid_run, &key, value);
                    }
                }
                let prefix = if k == 0 && bodies.is_empty() { String::new() } else { format!("{}: ", b.name) };
                for n in b.report.diagnostics_under(repair) {
                    record_note(ctx, &fid_run, n.level, &format!("{prefix}{}", n.message), &mut warned);
                }
                bodies.push(ProducedBody::new(Body::Mesh((b.mesh, b.topo)), NameSource::Intrinsic));
            }
            Ok(Produced::bodies(bodies))
        })?;
        raise_recorded_warnings(&mut ctx, &fid, span);
        // How the file says each body looks (a glTF material; white under
        // per-vertex colour): set as `Body.material(...)` would, from the
        // recorded args, so a memo hit looks the same. A later
        // `.material(...)` in the script overrides it.
        let looks: Vec<Option<Appearance>> = {
            let f = ctx.feature(&fid)?;
            (0..ids.len()).map(|k| Appearance::from_args(&f.args, &format!("look[{k}]."))).collect()
        };
        for (id, look) in ids.iter().zip(looks) {
            if let Some(look) = look {
                let m = &mut ctx.body_mut(*id)?.material;
                m.base_color = look.base_color;
                m.opacity = look.opacity;
                m.roughness = look.roughness;
                m.metallic = look.metallic;
                m.emissive = look.emissive;
            }
        }
        crate::layers::tag_ids(&mut ctx, layer, &ids, "import_mesh")?;
        bodies_value(eval.heap(), &ctx, ids)
    }

    /// The repair report of a body `import_mesh` made: every count the
    /// import measured — triangles, vertices, groups, shells, open, flipped
    /// and non-manifold edges, crossings, overlaps, folds, degenerate
    /// triangles, what the weld merged — with the verdicts `closed`,
    /// `oriented`, `inverted`, `solid`, the `genus` and signed `volume`
    /// when the mesh is closed, the `bbox`, and `chord_error`, which is
    /// `None`: on an imported mesh the triangles are the geometry. Reads
    /// the feature's recorded args, so it costs nothing and works on a
    /// memo hit.
    fn import_report<'v>(
        #[starlark(require = pos)] body: &KBody,
        eval: &mut Evaluator<'v, '_, '_>,
    ) -> anyhow::Result<KImportReport> {
        let st = store(eval)?;
        let ctx = st.ctx.borrow();
        let fid = FeatureId(body.feature.clone());
        let f = ctx.feature(&fid)?;
        if f.kind != "import_mesh" {
            anyhow::bail!(
                "import_report: {} was made by {} ({}), not by import_mesh; only an imported mesh carries a repair report",
                body.name,
                f.id,
                f.kind
            );
        }
        let k = f.outputs.iter().position(|b| b.0 == body.id).unwrap_or(0);
        let report = Report::from_args(&f.args, &format!("report[{k}]."))
            .ok_or_else(|| anyhow::anyhow!("import_report: {} recorded no report", f.id))?;
        Ok(KImportReport::new(body.name.clone(), report))
    }
}
