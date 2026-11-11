//! `cadpole bake` and `cadpole publish`: a document evaluated ahead of
//! time, and a static copy of the web app that opens it read-only.
//!
//! **The bake** is a `cadpole.v1.BakedModel` -- the `Model` that `Evaluate`
//! returns, every mesh included, plus the few facts a viewer wants about it
//! -- encoded and zlib-deflated. It is the same `Model` the browser already
//! decodes from `cadpole serve` (`RemoteEngine`) and the Apple viewer draws, so
//! the browser has no second reader to keep in step: what a static page does
//! with a bake is exactly what it does with a remote evaluation, without the
//! server and without Starlark.
//!
//! **The site** is the web dist's page, stylesheet and app bundle, renamed by
//! content hash and pointed at the bake by a `<meta name="cadpole-baked">` in
//! `index.html`. No wasm engine, no workers, no service worker: a published
//! page never evaluates, so it ships none of the machinery that does (the
//! wasm alone is 26 MB). And no sources, unless `--include-sources` asks.
//!
//! This lives in the CLI rather than a lower crate because both halves are
//! the CLI's: the bake is `Evaluate` plus a wrapper, and the site is a copy of
//! the dist the CLI already carries for `cadpole serve`.

use std::collections::BTreeSet;
use std::path::{Path, PathBuf};

use anyhow::Context as _;
use cadpole_engine::cadpole_assets::sha256_hex;
use cadpole_engine::cadpole_proto::prost::Message as _;
use cadpole_engine::cadpole_proto::v1 as pb;

/// The `BakedModel.schema_version` this build writes and the newest it reads.
pub const SCHEMA_VERSION: u32 = 1;
/// The extension of a bake on disk (build_support/file_types.bzl).
pub const MODEL_EXT: &str = cadpole_engine::cadpole_proto::file_types::BAKED_EXT;
/// The manifest a publish leaves beside `index.html`: what it wrote, so the
/// next publish into the same directory can take its own old files away and
/// nothing else.
pub const MANIFEST: &str = "cadpole-publish.json";
/// The largest a bake may inflate to when it is read back. A model of
/// ~1,500 bodies is about 17 MB; this is thirty times that, and a decompression bomb
/// stops here rather than at the machine's memory.
#[cfg(test)]
pub const MAX_INFLATED: usize = 512 << 20;
/// The `<meta>` the web app reads to find the bake (src/web/src/baked.ts).
pub const META_NAME: &str = "cadpole-baked";

/// What a bake is made with, beyond the document and its model.
#[derive(Clone, Debug, Default)]
pub struct BakeOptions {
    /// The viewer's name for the document; the document's own when `None`.
    pub title: Option<String>,
    /// Carry the scripts the entry loads (and every diagnostic, `print()`
    /// output included). Off by default.
    pub include_sources: bool,
    /// The tessellation tolerance the model was evaluated at, mm.
    pub tolerance: f64,
    /// Millimetres in one unit of the script (`--model-unit`): 1 for a
    /// script drawn in millimetres, 304.8 for one drawn in feet. Every
    /// length in the model is multiplied by it, once, at the bake (the
    /// rule: a model drawn in feet scales its bodies in the open, it does
    /// not pretend a millimetre is a foot). 0 reads as 1.
    pub scale: f64,
    /// The display convention to record (`--units`), over the script's own
    /// `units(...)`; `None` keeps the script's.
    pub display_units: Option<pb::DisplayUnits>,
    /// How the page opens: camera, grid, section, every length
    /// already in millimetres. `None` opens it as any other page.
    pub view: Option<pb::PublishedView>,
    /// Layers that start hidden (`--hide-layer`), over the script's own
    /// defaults: a layer the script hides stays hidden either way.
    pub hide_layers: Vec<String>,
    /// Which parameters the page lists, read-only (`--show-params`).
    pub show_params: ShowParams,
}

/// `--show-params`: which of the script's parameters a published
/// page lists. A published page cannot re-evaluate, so a parameter there is
/// a read-out of the value the bake was made with, never a slider; and by
/// default there are none at all -- a parameter's name can say more about
/// the author's options than the publisher meant to show.
#[derive(Clone, Debug, Default, PartialEq)]
pub enum ShowParams {
    /// None: the panel is not shown (the default).
    #[default]
    None,
    /// Every parameter (`--show-params all`).
    All,
    /// These, by name, in the script's order (`--show-params a,b`).
    Only(Vec<String>),
}

impl ShowParams {
    /// `--show-params`'s value: "all", "none", or a comma list of names.
    pub fn parse(s: &str) -> ShowParams {
        match s.trim() {
            "" | "none" => ShowParams::None,
            "all" => ShowParams::All,
            list => ShowParams::Only(list.split(',').map(|n| n.trim().to_owned()).filter(|n| !n.is_empty()).collect()),
        }
    }

    fn keeps(&self, name: &str) -> bool {
        match self {
            ShowParams::None => false,
            ShowParams::All => true,
            ShowParams::Only(names) => names.iter().any(|n| n == name),
        }
    }
}

/// Refuse a `--hide-layer` or `--show-params` name the model does not have,
/// naming the ones it does: a typo'd layer that silently stays visible is
/// the failure this exists to stop.
pub fn check_choices(model: &pb::Model, opts: &BakeOptions) -> anyhow::Result<()> {
    let hide = opts.hide_layers.iter().map(|n| ("--hide-layer", n));
    let frame = opts.view.iter().flat_map(|v| v.frame_layers.iter()).map(|n| ("--frame-layer", n));
    for (flag, name) in hide.chain(frame) {
        if !model.layers.iter().any(|l| &l.name == name) {
            let have: Vec<&str> = model.layers.iter().map(|l| l.name.as_str()).collect();
            anyhow::bail!(
                "{flag} {name:?}: the script declares no such layer ({})",
                if have.is_empty() { "it declares none".to_owned() } else { format!("it declares {}", have.join(", ")) }
            );
        }
    }
    if let ShowParams::Only(names) = &opts.show_params {
        for name in names {
            if !model.params.iter().any(|p| &p.name == name) {
                let have: Vec<&str> = model.params.iter().map(|p| p.name.as_str()).collect();
                anyhow::bail!("--show-params {name:?}: the script has no such parameter (it has {})", have.join(", "));
            }
        }
    }
    Ok(())
}

/// The preset names `--view` accepts for a published page.
pub const VIEW_PRESETS: [&str; 8] = ["home", "iso", "top", "bottom", "front", "back", "left", "right"];

/// The display modes `--mode` accepts for a published page: the web
/// viewer's own names for them (`VIEW_MODES` in src/web/src/viewer.ts).
pub const DISPLAY_MODES: [&str; 5] = ["shaded", "shaded_edges", "wireframe", "xray", "faces"];

/// `--mode` for a published page: one of [`DISPLAY_MODES`], however it was
/// hyphenated (`shaded-edges`, `x-ray`).
pub fn display_mode(s: &str) -> anyhow::Result<String> {
    let m = s.trim().to_ascii_lowercase().replace('-', "_");
    let m = if m == "x_ray" { "xray".to_owned() } else { m };
    anyhow::ensure!(DISPLAY_MODES.contains(&m.as_str()), "--mode {s:?}: one of {}", DISPLAY_MODES.join(", ").replace('_', "-"));
    Ok(m)
}

/// `--home-url` for a published page: an http(s) address or one relative to
/// the page. Anything with another scheme is refused here, and the page
/// checks again before it draws the link: a `javascript:` address in a bake
/// must never become something a reader can click.
pub fn home_url(s: &str) -> anyhow::Result<String> {
    let u = s.trim();
    anyhow::ensure!(!u.is_empty(), "--home-url: an address, as in https://example.org/ or ../");
    anyhow::ensure!(!u.chars().any(|c| c.is_control() || c.is_whitespace()), "--home-url {s:?}: no spaces or control characters");
    let scheme = u.split(['/', '?', '#']).next().and_then(|head| head.split_once(':')).map(|(scheme, _)| scheme.to_ascii_lowercase());
    match scheme.as_deref() {
        None | Some("http") | Some("https") => Ok(u.to_owned()),
        Some(other) => anyhow::bail!("--home-url {s:?}: an http(s) address or a relative one, not {other}:"),
    }
}

/// `--section` for a published page: `cadpole render --section`'s
/// spelling, at most one horizontal (`z=`) and one vertical (`x=` or `y=`)
/// plane, comma-separated, each coordinate in the script's units and
/// multiplied by `scale` to millimetres. Returns the normalised spec.
pub fn section_spec(s: &str, scale: f64) -> anyhow::Result<String> {
    let mut parts: Vec<String> = Vec::new();
    let (mut horizontal, mut vertical) = (false, false);
    for raw in s.split(',').map(str::trim).filter(|p| !p.is_empty()) {
        let (neg, rest) = match raw.strip_prefix('-') {
            Some(r) => ("-", r),
            None => ("", raw),
        };
        let (axis, value) =
            rest.split_once('=').ok_or_else(|| anyhow::anyhow!("--section {raw:?}: expected <axis>=<value>, as in z=4 or -x=12"))?;
        let axis = axis.trim().to_ascii_lowercase();
        let slot = match axis.as_str() {
            "z" => &mut horizontal,
            "x" | "y" => &mut vertical,
            other => anyhow::bail!("--section {raw:?}: the axis must be x, y or z, not {other:?}"),
        };
        anyhow::ensure!(!*slot, "--section {s:?}: at most one horizontal (z=) and one vertical (x= or y=) plane");
        *slot = true;
        let at: f64 = value.trim().parse().map_err(|_| anyhow::anyhow!("--section {raw:?}: {:?} is not a number", value.trim()))?;
        anyhow::ensure!(at.is_finite(), "--section {raw:?}: the coordinate must be finite");
        let k = if scale > 0.0 { scale } else { 1.0 };
        // Rounded to a micron, so a foot is 304.8 and not 304.80000000000001.
        let mm = (at * k * 1000.0).round() / 1000.0;
        parts.push(format!("{neg}{axis}={mm}"));
    }
    Ok(parts.join(","))
}

/// A `--model-unit` / `--units` name as millimetres per unit: the
/// vocabulary `cadpole export`'s glTF shares, defined once in the engine.
pub fn mm_per_unit(name: &str) -> anyhow::Result<f64> {
    if name.trim().is_empty() {
        anyhow::bail!("unknown unit \"\": mm, cm, m, in or ft");
    }
    cadpole_engine::gltf::mm_per_unit(name).map_err(anyhow::Error::msg)
}

/// A `--units` name as the model's display convention.
pub fn display_units(name: &str) -> anyhow::Result<pb::DisplayUnits> {
    let (unit, fraction) = cadpole_engine::gltf::display_units(name).map_err(anyhow::Error::msg)?;
    Ok(pb::DisplayUnits { unit, fraction })
}

/// Wrap an evaluated model as a bake.
///
/// Pure: the caller evaluated `doc` and hands the model over, so this is
/// testable without an engine, and what goes in the file is decided here and
/// nowhere else.
///
/// **Without `include_sources` no text the script's author wrote survives**
/// except names: no diagnostic (a `print()`, a failed check, a
/// warning quoting a value), no source span, no parameter description, no
/// layer description, no feature argument. What is left is geometry, the
/// names of features, bodies, faces, parameters and layers, layer
/// membership and visibility, materials and units.
pub fn bake(doc: &pb::Document, mut model: pb::Model, opts: &BakeOptions) -> pb::BakedModel {
    // The same sources bake to the same bytes: the two fields that measure
    // the run rather than describe the model go.
    model.eval_millis = 0.0;
    model.memo = None;
    if !opts.include_sources {
        strip_author_text(&mut model);
    }
    if opts.scale > 0.0 && opts.scale != 1.0 {
        scale_model(&mut model, opts.scale);
    }
    if let Some(u) = &opts.display_units {
        model.display_units = Some(u.clone());
    }
    // Layers the publisher hides start hidden (the viewer can still
    // show them), and only the parameters the publisher lists survive.
    for l in &mut model.layers {
        if opts.hide_layers.iter().any(|n| n == &l.name) {
            l.visible = false;
        }
    }
    model.params.retain(|p| opts.show_params.keeps(&p.name));
    let document = opts.include_sources.then(|| {
        let keep = reachable_scripts(doc);
        pb::Document {
            id: String::new(),
            name: doc.name.clone(),
            schema_version: doc.schema_version,
            display_unit: doc.display_unit,
            scripts: doc.scripts.iter().filter(|s| keep.contains(&s.path)).cloned().collect(),
            param_overrides: doc.param_overrides.clone(),
            // Names and hashes, never bytes: a scan or a survey is not
            // published because its script was.
            assets: doc.assets.iter().map(|a| pb::Asset { data: Vec::new(), source_path: String::new(), ..a.clone() }).collect(),
            ..Default::default()
        }
    });
    pb::BakedModel {
        schema_version: SCHEMA_VERSION,
        name: opts.title.clone().filter(|t| !t.trim().is_empty()).unwrap_or_else(|| doc.name.clone()),
        model: Some(model),
        tolerance: opts.tolerance,
        generator: format!("cadpole {}", cadpole_engine::VERSION),
        source_sha256: source_sha256(doc),
        // Only the listed parameters' overrides: an unlisted parameter's
        // name is not published through its override either.
        param_overrides: doc
            .param_overrides
            .iter()
            .filter(|(k, _)| opts.include_sources || opts.show_params.keeps(k))
            .map(|(k, v)| (k.clone(), v.clone()))
            .collect(),
        document,
        view: opts.view.clone(),
    }
}

/// Everything in a model that is text the author wrote rather than a name:
/// see [`bake`]. Animations go with it -- a published page cannot evaluate
/// a keyframe -- and a plate keeps its number and name but not the export
/// path it was declared for.
pub fn strip_author_text(model: &mut pb::Model) {
    model.diagnostics.clear();
    for p in &mut model.params {
        p.description.clear();
        p.span = None;
    }
    for f in &mut model.features {
        f.args.clear();
        f.span = None;
        f.diagnostics.clear();
    }
    for l in &mut model.layers {
        l.doc.clear();
    }
    model.animations.clear();
    for p in &mut model.plates {
        p.path.clear();
    }
    if let Some(sun) = model.render_defaults.as_mut().and_then(|r| r.sun.as_mut()) {
        sun.label.clear();
    }
}

/// Multiply every length in `model` by `k` (areas by `k²`): meshes, boxes,
/// topology, poses, the document's camera. Normals, angles and parameter
/// values do not move -- a parameter is the script's number, in the
/// script's unit. Print plates are a bed's millimetres and are left alone.
pub fn scale_model(model: &mut pb::Model, k: f64) {
    fn v(p: &mut Option<pb::Vec3>, k: f64) {
        if let Some(p) = p {
            p.x *= k;
            p.y *= k;
            p.z *= k;
        }
    }
    let kf = k as f32;
    for b in &mut model.bodies {
        if let Some(bb) = &mut b.bbox {
            v(&mut bb.min, k);
            v(&mut bb.max, k);
        }
        if let Some(m) = &mut b.mesh {
            for x in &mut m.positions {
                *x *= kf;
            }
        }
        if let Some(t) = &mut b.transform {
            v(&mut t.translation, k);
        }
        if let Some(t) = &mut b.topology {
            for f in &mut t.faces {
                v(&mut f.centroid, k);
                f.area *= k * k;
            }
            for e in &mut t.edges {
                v(&mut e.start, k);
                v(&mut e.end, k);
                e.length *= k;
                for p in &mut e.polyline {
                    p.x *= k;
                    p.y *= k;
                    p.z *= k;
                }
            }
            for p in &mut t.vertices {
                v(&mut p.position, k);
            }
        }
    }
    for i in &mut model.instances {
        if let Some(t) = &mut i.pose {
            v(&mut t.translation, k);
        }
        v(&mut i.explode, k);
    }
    for a in &mut model.assemblies {
        a.explode_distance *= k;
    }
    if let Some(r) = &mut model.render_defaults {
        v(&mut r.eye, k);
        v(&mut r.target, k);
    }
}

/// The scripts `main.star` reaches through `load()`, transitively, by the
/// key the loader resolves each to. The CLI bundles every sibling `.star` of
/// an entry that loads anything (it cannot know in advance which), so a
/// house directory's twenty unrelated scripts would otherwise ride along.
pub fn reachable_scripts(doc: &pb::Document) -> BTreeSet<String> {
    let entry = doc.scripts.first().map(|s| s.path.clone()).unwrap_or_else(|| "main.star".to_owned());
    let mut seen = BTreeSet::new();
    let mut todo = vec![entry];
    while let Some(key) = todo.pop() {
        if !seen.insert(key.clone()) {
            continue;
        }
        let Some(file) = doc.scripts.iter().find(|s| s.path == key) else { continue };
        for target in crate::input::load_targets(&file.source) {
            if target.starts_with('@') {
                continue;
            }
            let k = cadpole_engine::cadpole_script::resolve_load(&key, &target);
            if !k.is_empty() && !seen.contains(&k) {
                todo.push(k);
            }
        }
    }
    seen.retain(|k| doc.scripts.iter().any(|s| &s.path == k));
    seen
}

/// sha256 over what the model was evaluated from: every script (path and
/// text), every asset (name and hash), every override, each sorted, each
/// field NUL-terminated so no two different documents run together.
pub fn source_sha256(doc: &pb::Document) -> String {
    let mut buf: Vec<u8> = Vec::new();
    let mut put = |b: &[u8]| {
        buf.extend_from_slice(b);
        buf.push(0);
    };
    let mut scripts: Vec<&pb::ScriptFile> = doc.scripts.iter().collect();
    scripts.sort_by(|a, b| a.path.cmp(&b.path));
    for s in scripts {
        put(b"script");
        put(s.path.as_bytes());
        put(s.source.as_bytes());
    }
    let mut assets: Vec<&pb::Asset> = doc.assets.iter().collect();
    assets.sort_by(|a, b| a.name.cmp(&b.name));
    for a in assets {
        put(b"asset");
        put(a.name.as_bytes());
        put(a.sha256.as_bytes());
    }
    let mut keys: Vec<&String> = doc.param_overrides.keys().collect();
    keys.sort();
    for k in keys {
        put(b"param");
        put(k.as_bytes());
        put(&doc.param_overrides[k].encode_to_vec());
    }
    sha256_hex(&buf)
}

/// A bake as the bytes of a baked-model file: the message, zlib-deflated.
pub fn encode(baked: &pb::BakedModel) -> Vec<u8> {
    miniz_oxide::deflate::compress_to_vec_zlib(&baked.encode_to_vec(), 6)
}

/// A baked-model file back as a message, refusing one that inflates past
/// [`MAX_INFLATED`] or was written by a newer schema. The web app has the
/// reader that ships; this one lets the tests round-trip a bake.
#[cfg(test)]
pub fn decode(bytes: &[u8]) -> anyhow::Result<pb::BakedModel> {
    let raw = miniz_oxide::inflate::decompress_to_vec_zlib_with_limit(bytes, MAX_INFLATED)
        .map_err(|e| anyhow::anyhow!("not a .{MODEL_EXT} baked model (zlib: {:?}), or it inflates past {} MB", e.status, MAX_INFLATED >> 20))?;
    let baked = pb::BakedModel::decode(raw.as_slice()).context("not a cadpole.v1.BakedModel")?;
    anyhow::ensure!(
        baked.schema_version >= 1 && baked.schema_version <= SCHEMA_VERSION,
        "this bake is schema {}, and this build reads 1..={SCHEMA_VERSION}",
        baked.schema_version
    );
    Ok(baked)
}

/// A file name's worth of a content hash: the first `n` hex digits.
fn short_hash(bytes: &[u8], n: usize) -> String {
    sha256_hex(bytes)[..n].to_owned()
}

/// `name` made safe for a file name: letters, digits, `-` and `_`, the rest
/// `-`; "model" when nothing is left.
pub fn file_stem(name: &str) -> String {
    let s: String = name.chars().map(|c| if c.is_ascii_alphanumeric() || c == '-' || c == '_' { c } else { '-' }).collect();
    let s = s.trim_matches('-').to_owned();
    if s.is_empty() { "model".to_owned() } else { s }
}

/// What one publish wrote.
#[derive(Debug)]
pub struct Published {
    pub index: PathBuf,
    /// Relative to the directory, every file written, `index.html` included.
    pub files: Vec<String>,
    /// The bake's file name.
    pub bake: String,
    pub bake_bytes: usize,
}

/// The HTML-escaped form of `s` for an attribute or a text node.
fn html_escape(s: &str) -> String {
    s.replace('&', "&amp;").replace('<', "&lt;").replace('>', "&gt;").replace('"', "&quot;")
}

/// The dist's `index.html` rewritten for a published site: the stylesheet
/// and the bundle by their hashed names, the bake named by a `<meta>`, the
/// title the document's, and no web-app manifest (a published page is a
/// page, not an installable playground). Every edit must find what it
/// replaces: a dist whose page changed shape is an error here, not a site
/// that silently loads the playground.
pub fn rewrite_index(html: &str, styles: &str, app: &str, bake: &str, title: &str) -> anyhow::Result<String> {
    let mut out = html.to_owned();
    let mut swap = |from: &str, to: &str| -> anyhow::Result<()> {
        anyhow::ensure!(out.contains(from), "the web dist's index.html has no `{from}`; it changed shape, and cadpole publish needs updating with it");
        out = out.replacen(from, to, 1);
        Ok(())
    };
    swap("href=\"./styles.css\"", &format!("href=\"./{styles}\""))?;
    swap("src=\"./app.js\"", &format!("src=\"./{app}\""))?;
    swap("<link rel=\"manifest\" href=\"./manifest.webmanifest\">\n", "")?;
    swap("<title>Cadpole</title>", &format!("<title>{} · Cadpole</title>\n<meta name=\"{META_NAME}\" content=\"./{}\">", html_escape(title), html_escape(bake)))?;
    Ok(out)
}

/// The bundle without its trailing `//# sourceMappingURL=` line: the map is
/// not published, and a page that names a map it does not have makes every
/// developer console that opens it ask for one.
fn strip_source_map(js: &[u8]) -> Vec<u8> {
    let text = String::from_utf8_lossy(js);
    match text.rfind("//# sourceMappingURL=") {
        Some(i) if !text[i..].trim_end().contains('\n') => text[..i].trim_end().as_bytes().to_vec(),
        _ => js.to_vec(),
    }
}

/// Write the site: the bake, the page, its stylesheet and its bundle, and
/// [`MANIFEST`]. `out` is created if need be. A directory that has files in
/// it and no manifest is refused unless `force`, so a typo'd `--out-dir`
/// cannot scatter a site through someone's home directory; one that has a
/// manifest has that publish's files taken away first, so content-hashed
/// names do not pile up.
pub fn publish(web_dir: &Path, out: &Path, baked_bytes: &[u8], baked: &pb::BakedModel, force: bool) -> anyhow::Result<Published> {
    let read = |name: &str| -> anyhow::Result<Vec<u8>> {
        std::fs::read(web_dir.join(name)).with_context(|| format!("reading {} from the web dist {}", name, web_dir.display()))
    };
    let html = String::from_utf8(read("index.html")?).context("index.html is not UTF-8")?;
    let styles = read("styles.css")?;
    let app = strip_source_map(&read("app.js")?);

    std::fs::create_dir_all(out).with_context(|| format!("creating {}", out.display()))?;
    let manifest_path = out.join(MANIFEST);
    if manifest_path.exists() {
        // A previous publish: take away exactly what it wrote.
        let old: serde_json::Value = serde_json::from_slice(&std::fs::read(&manifest_path)?).context("reading the previous cadpole-publish.json")?;
        for f in old.get("files").and_then(|v| v.as_array()).into_iter().flatten().filter_map(|v| v.as_str()) {
            // Only plain names it could have written: nothing with a path in it.
            if !f.is_empty() && !f.contains('/') && !f.contains('\\') && f != ".." {
                let _ = std::fs::remove_file(out.join(f));
            }
        }
    } else if !force && std::fs::read_dir(out)?.next().is_some() {
        anyhow::bail!(
            "{} is not empty and holds no {MANIFEST}, so it is not an earlier publish; pick an empty directory or pass --force",
            out.display()
        );
    }

    let stem = file_stem(&baked.name);
    let bake_name = format!("{stem}.{}.{MODEL_EXT}", short_hash(baked_bytes, 16));
    let styles_name = format!("styles.{}.css", short_hash(&styles, 10));
    let app_name = format!("app.{}.js", short_hash(&app, 10));
    let index = rewrite_index(&html, &styles_name, &app_name, &bake_name, &baked.name)?;

    let mut files: Vec<String> = Vec::new();
    let mut write = |name: &str, bytes: &[u8]| -> anyhow::Result<()> {
        std::fs::write(out.join(name), bytes).with_context(|| format!("writing {}", out.join(name).display()))?;
        files.push(name.to_owned());
        Ok(())
    };
    write(&bake_name, baked_bytes)?;
    write(&styles_name, &styles)?;
    write(&app_name, &app)?;
    write("index.html", index.as_bytes())?;
    let manifest = serde_json::json!({
        "generator": baked.generator,
        "name": baked.name,
        "model": bake_name,
        "source_sha256": baked.source_sha256,
        "sources_included": baked.document.is_some(),
        "files": files.iter().chain(std::iter::once(&MANIFEST.to_owned())).collect::<Vec<_>>(),
    });
    std::fs::write(&manifest_path, serde_json::to_vec_pretty(&manifest)?)?;
    files.push(MANIFEST.to_owned());
    Ok(Published { index: out.join("index.html"), files, bake: bake_name, bake_bytes: baked_bytes.len() })
}

#[cfg(test)]
mod tests {
    use super::*;

    fn doc() -> pb::Document {
        let mut d = cadpole_engine::document_from_scripts(
            "cabin",
            vec![
                ("main.star".into(), "load(\"shell.star\", \"SHELL\")\nprint(\"secret cost $1\")\n".into()),
                ("shell.star".into(), "load(\"lib/roof.star\", \"ROOF\")\nSHELL = 1\n".into()),
                ("lib/roof.star".into(), "ROOF = 2\n".into()),
                ("unrelated.star".into(), "x = 3\n".into()),
            ],
        );
        d.assets.push(pb::Asset { name: "scan.glb".into(), sha256: "ab".repeat(32), size: 4, data: vec![1, 2, 3, 4], ..Default::default() });
        d
    }

    fn model() -> pb::Model {
        pb::Model {
            eval_millis: 812.5,
            memo: Some(pb::MemoReport { ops_hit: 3, ..Default::default() }),
            diagnostics: vec![
                pb::Diagnostic { severity: pb::Severity::Info as i32, message: "secret cost $1".into(), ..Default::default() },
                pb::Diagnostic { severity: pb::Severity::Warning as i32, message: "a thin wall".into(), ..Default::default() },
            ],
            bodies: vec![pb::Body {
                id: "wall#1".into(),
                mesh: Some(pb::Mesh { positions: vec![0.0, 0.0, 0.0, 1.0, 0.0, 0.0, 0.0, 1.0, 0.0], indices: vec![0, 1, 2], ..Default::default() }),
                ..Default::default()
            }],
            ..Default::default()
        }
    }

    #[test]
    fn a_bake_round_trips_and_is_reproducible() {
        let opts = BakeOptions { tolerance: 0.05, ..Default::default() };
        let a = encode(&bake(&doc(), model(), &opts));
        let mut slower = model();
        slower.eval_millis = 99.0;
        let b = encode(&bake(&doc(), slower, &opts));
        assert_eq!(a, b, "the run's timing must not reach the bytes");
        let back = decode(&a).unwrap();
        assert_eq!(back.schema_version, SCHEMA_VERSION);
        assert_eq!(back.name, "cabin");
        assert_eq!(back.model.as_ref().unwrap().bodies[0].mesh.as_ref().unwrap().indices, vec![0, 1, 2]);
        assert!(back.model.as_ref().unwrap().memo.is_none());
        assert!(back.generator.starts_with("cadpole "));
        assert_eq!(back.source_sha256.len(), 64);
    }

    #[test]
    fn without_sources_a_bake_carries_no_script_and_no_print() {
        let baked = bake(&doc(), model(), &BakeOptions::default());
        assert!(baked.document.is_none());
        assert!(baked.model.as_ref().unwrap().diagnostics.is_empty(), "no diagnostic: a warning can quote the script too");
        let bytes = baked.encode_to_vec();
        assert!(!bytes.windows(6).any(|w| w == b"secret"), "neither the script nor its print() output is in the bake");
    }

    #[test]
    fn with_sources_only_what_main_loads_and_no_asset_bytes() {
        let baked = bake(&doc(), model(), &BakeOptions { include_sources: true, ..Default::default() });
        let d = baked.document.unwrap();
        let paths: Vec<&str> = d.scripts.iter().map(|s| s.path.as_str()).collect();
        assert_eq!(paths, ["main.star", "shell.star", "lib/roof.star"]);
        assert_eq!(d.assets.len(), 1);
        assert!(d.assets[0].data.is_empty());
        assert_eq!(d.assets[0].sha256, "ab".repeat(32));
        assert_eq!(baked.model.unwrap().diagnostics.len(), 2, "the author's own copy keeps its prints");
    }

    #[test]
    fn a_model_in_feet_is_scaled_once_and_reads_in_feet_and_inches() {
        let mut m = model();
        m.bodies[0].bbox = Some(pb::BBox { min: Some(pb::Vec3 { x: 0.0, y: 0.0, z: 0.0 }), max: Some(pb::Vec3 { x: 10.0, y: 1.0, z: 2.0 }) });
        let opts = BakeOptions { scale: mm_per_unit("ft").unwrap(), display_units: Some(display_units("ft-in").unwrap()), ..Default::default() };
        let b = bake(&doc(), m, &opts);
        let m = b.model.unwrap();
        assert_eq!(m.bodies[0].bbox.as_ref().unwrap().max.as_ref().unwrap().x, 3048.0);
        assert_eq!(m.bodies[0].mesh.as_ref().unwrap().positions[3], 304.8_f32);
        assert_eq!(m.display_units.unwrap().unit, "ft-in");
        assert!(mm_per_unit("furlong").is_err());
        assert!(display_units("parsec").is_err());
    }

    #[test]
    fn the_source_hash_moves_with_any_source() {
        let base = source_sha256(&doc());
        let mut d = doc();
        d.scripts[2].source.push(' ');
        assert_ne!(source_sha256(&d), base);
        let mut d = doc();
        d.assets[0].sha256 = "cd".repeat(32);
        assert_ne!(source_sha256(&d), base);
        let mut d = doc();
        d.param_overrides.insert("w".into(), pb::ParamValue { value: Some(pb::param_value::Value::Number(2.0)) });
        assert_ne!(source_sha256(&d), base);
        let mut d = doc();
        d.scripts.reverse();
        assert_eq!(source_sha256(&d), base, "order does not matter");
    }

    #[test]
    fn decode_refuses_garbage_and_newer_schemas() {
        assert!(decode(b"not zlib at all").is_err());
        let mut b = bake(&doc(), model(), &BakeOptions::default());
        b.schema_version = SCHEMA_VERSION + 1;
        let err = decode(&encode(&b)).unwrap_err().to_string();
        assert!(err.contains("schema"), "{err}");
    }

    #[test]
    fn publish_choices_hide_layers_list_params_and_carry_the_view() {
        let mut m = model();
        m.layers = vec![
            pb::Layer { name: "site".into(), visible: true, opacity: 1.0, ..Default::default() },
            pb::Layer { name: "existing".into(), visible: false, opacity: 1.0, ..Default::default() },
        ];
        m.params = vec![
            pb::ParamInfo { name: "depth".into(), ..Default::default() },
            pb::ParamInfo { name: "budget_option".into(), ..Default::default() },
        ];
        // By default no parameter is published, and the layers are the script's.
        let b = bake(&doc(), m.clone(), &BakeOptions::default());
        let bm = b.model.as_ref().unwrap();
        assert!(bm.params.is_empty());
        assert_eq!(bm.layers.iter().map(|l| l.visible).collect::<Vec<_>>(), [true, false]);
        assert!(b.view.is_none());

        let opts = BakeOptions {
            hide_layers: vec!["site".into()],
            show_params: ShowParams::parse("depth"),
            view: Some(pb::PublishedView {
                camera: "top".into(),
                grid: "dim".into(),
                section: section_spec("z=4", 304.8).unwrap(),
                ..Default::default()
            }),
            ..Default::default()
        };
        check_choices(&m, &opts).unwrap();
        let b = bake(&doc(), m.clone(), &opts);
        let bm = b.model.as_ref().unwrap();
        assert_eq!(bm.layers.iter().map(|l| l.visible).collect::<Vec<_>>(), [false, false], "a hidden layer stays hidden");
        assert_eq!(bm.params.iter().map(|p| p.name.as_str()).collect::<Vec<_>>(), ["depth"]);
        let bytes = b.encode_to_vec();
        assert!(!bytes.windows(13).any(|w| w == b"budget_option"), "an unlisted parameter's name is not in the bake");
        let v = decode(&encode(&b)).unwrap().view.unwrap();
        assert_eq!((v.camera.as_str(), v.grid.as_str(), v.section.as_str()), ("top", "dim", "z=1219.2"));
        assert_eq!(ShowParams::parse("all"), ShowParams::All);

        let typo = BakeOptions { hide_layers: vec!["sight".into()], ..Default::default() };
        let err = check_choices(&m, &typo).unwrap_err().to_string();
        assert!(err.contains("site, existing"), "{err}");
        let framed = |name: &str| BakeOptions {
            view: Some(pb::PublishedView { frame_layers: vec![name.into()], ..Default::default() }),
            ..Default::default()
        };
        assert!(check_choices(&m, &framed("site")).is_ok());
        assert!(check_choices(&m, &framed("house")).unwrap_err().to_string().contains("--frame-layer"));
        assert!(check_choices(&m, &BakeOptions { show_params: ShowParams::parse("nope"), ..Default::default() }).is_err());
    }

    #[test]
    fn a_display_mode_is_one_of_the_viewers() {
        assert_eq!(display_mode("xray").unwrap(), "xray");
        assert_eq!(display_mode(" X-Ray ").unwrap(), "xray");
        assert_eq!(display_mode("shaded-edges").unwrap(), "shaded_edges");
        assert!(display_mode("ghost").is_err());
        assert!(display_mode("").is_err());
    }

    #[test]
    fn a_home_link_is_http_or_relative() {
        assert_eq!(home_url(" https://example.org/#models ").unwrap(), "https://example.org/#models");
        assert_eq!(home_url("../../#turbojet").unwrap(), "../../#turbojet");
        assert_eq!(home_url("/models?a=b:c").unwrap(), "/models?a=b:c");
        assert!(home_url("javascript:alert(1)").is_err());
        assert!(home_url("JavaScript:alert(1)").is_err());
        assert!(home_url("data:text/html,x").is_err());
        assert!(home_url("https://example.org/a b").is_err());
        assert!(home_url("").is_err());
    }

    #[test]
    fn a_published_section_is_render_spelling_in_millimetres() {
        assert_eq!(section_spec("z=40", 1.0).unwrap(), "z=40");
        assert_eq!(section_spec(" z=4 , -x=10 ", 304.8).unwrap(), "z=1219.2,-x=3048");
        assert_eq!(section_spec("", 1.0).unwrap(), "");
        assert!(section_spec("z=1,z=2", 1.0).is_err(), "one horizontal plane");
        assert!(section_spec("x=1,y=2", 1.0).is_err(), "one vertical plane");
        assert!(section_spec("w=1", 1.0).is_err());
        assert!(section_spec("z=up", 1.0).is_err());
    }

    /// The real page, so a change to src/web/index.html that this rewrite
    /// no longer understands fails here rather than publishing a playground.
    const INDEX_HTML: &str = include_str!("../../../web/index.html");

    #[test]
    fn the_two_file_types_differ() {
        use cadpole_engine::cadpole_proto::file_types::{BAKED_EXT, DOCUMENT_EXT};
        assert_ne!(DOCUMENT_EXT, BAKED_EXT, "build_support/file_types.bzl");
        for e in [DOCUMENT_EXT, BAKED_EXT] {
            assert!(!e.is_empty() && !e.contains('.') && e == e.to_lowercase(), "{e:?}");
        }
    }

    #[test]
    fn the_real_index_html_is_rewritten() {
        let bake_name = format!("cabin.3.{MODEL_EXT}");
        let out = rewrite_index(INDEX_HTML, "styles.1.css", "app.2.js", &bake_name, "Cabin <&> \"one\"").unwrap();
        assert!(out.contains("href=\"./styles.1.css\""));
        assert!(out.contains("src=\"./app.2.js\""));
        assert!(out.contains(&format!("<meta name=\"cadpole-baked\" content=\"./{bake_name}\">")));
        assert!(out.contains("<title>Cabin &lt;&amp;&gt; &quot;one&quot; · Cadpole</title>"));
        assert!(!out.contains("manifest.webmanifest"));
        assert!(rewrite_index("<html></html>", "a", "b", "c", "d").is_err());
    }

    #[test]
    fn a_publish_replaces_its_own_files_and_refuses_a_stranger() {
        let tmp = std::env::temp_dir().join(format!("cadpole-publish-test-{}", std::process::id()));
        let _ = std::fs::remove_dir_all(&tmp);
        let web = tmp.join("dist");
        std::fs::create_dir_all(&web).unwrap();
        std::fs::write(web.join("index.html"), INDEX_HTML).unwrap();
        std::fs::write(web.join("styles.css"), "body{}").unwrap();
        std::fs::write(web.join("app.js"), "console.log(1);\n//# sourceMappingURL=app.js.map\n").unwrap();
        std::fs::write(web.join("cadpole_wasm_bg.wasm"), "big").unwrap();
        let site = tmp.join("site");

        let b1 = bake(&doc(), model(), &BakeOptions::default());
        let p1 = publish(&web, &site, &encode(&b1), &b1, false).unwrap();
        assert!(p1.bake.starts_with("cabin.") && p1.bake.ends_with(&format!(".{MODEL_EXT}")));
        let mut names: Vec<String> = std::fs::read_dir(&site).unwrap().map(|e| e.unwrap().file_name().into_string().unwrap()).collect();
        names.sort();
        assert_eq!(names.len(), 5, "{names:?}");
        assert!(!names.iter().any(|n| n.ends_with(".star") || n.contains("wasm") || n == "sw.js" || n.ends_with(".map")), "{names:?}");
        let app = names.iter().find(|n| n.starts_with("app.")).unwrap();
        assert!(!std::fs::read_to_string(site.join(app)).unwrap().contains("sourceMappingURL"));

        // A second publish of a different model into the same place: the
        // old bake is gone, the new one is there.
        let mut m = model();
        m.bodies[0].id = "wall#2".into();
        let b2 = bake(&doc(), m, &BakeOptions::default());
        let p2 = publish(&web, &site, &encode(&b2), &b2, false).unwrap();
        assert_ne!(p1.bake, p2.bake);
        assert!(!site.join(&p1.bake).exists());
        assert!(site.join(&p2.bake).exists());
        assert_eq!(std::fs::read_dir(&site).unwrap().count(), 5);

        // A directory with someone else's files in it.
        let stranger = tmp.join("home");
        std::fs::create_dir_all(&stranger).unwrap();
        std::fs::write(stranger.join("notes.txt"), "mine").unwrap();
        assert!(publish(&web, &stranger, &encode(&b2), &b2, false).is_err());
        assert!(publish(&web, &stranger, &encode(&b2), &b2, true).is_ok());
        assert!(stranger.join("notes.txt").exists(), "--force adds; it does not delete");
        let _ = std::fs::remove_dir_all(&tmp);
    }
}
