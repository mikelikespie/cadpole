//! `cadpole-bench`: time the pipeline phases for scripts and print one JSON
//! line per script, so numbers can be tracked over time.
//!
//! Usage: cadpole-bench [--repeat N] [--size WxH] [--param k=v] script.star...
//!        cadpole-bench --signature [--long] [--param k=v] script.star...
//!        cadpole-bench --drag k=v1,v2,... [--preview] [--no-memo] [--param k=v] script.star...
//!
//! `--drag` is the kernel-op memo's benchmark: one evaluation at the
//! base parameters, then one per listed value of `k` through the *same*
//! memo — what a slider drag sends a session — and, with `--preview`, each
//! of those as a drag preview followed by one full evaluation at the last
//! value, which is the release. One JSON line per evaluation: the wall
//! clock, and the counts (ops run, hit, deferred) that say why it took
//! that long. `--no-memo` is the same sequence with no memo, the baseline.
//!
//! `--signature` prints, instead of timings, everything a caller can observe
//! about each visible body: its mesh as bytes, its volume, whether it is
//! watertight, and every generated name on it. Two builds that print the same
//! line produced the *same* model, not an equivalent one — which is how an
//! optimisation to the boolean path is shown not to have moved anything.
//! It is deliberately not a golden file: the
//! comparison is between two builds on one machine, so nothing here depends on
//! float formatting agreeing across platforms.

use std::sync::Arc;
use std::time::Instant;

use cadpole_engine::cadpole_core::render::RenderOptions;
use cadpole_engine::cadpole_script::{EvalInput, ScriptFile};

/// FNV-1a over the bytes of whatever is pushed at it. A digest keeps a
/// signature line readable; `--signature --long` prints the material itself
/// when a digest has already said two runs differ.
#[derive(Clone, Copy)]
struct Digest(u64);

impl Digest {
    fn new() -> Digest {
        Digest(0xcbf2_9ce4_8422_2325)
    }
    fn eat(&mut self, bytes: &[u8]) {
        for b in bytes {
            self.0 ^= *b as u64;
            self.0 = self.0.wrapping_mul(0x1000_0000_01b3);
        }
    }
    fn hex(self) -> String {
        format!("{:016x}", self.0)
    }
}

/// `--param n=32`: a number, a bool, or text, the way the CLI reads one.
fn param_value(v: &str) -> cadpole_engine::cadpole_core::ParamValue {
    use cadpole_engine::cadpole_core::ParamValue;
    match v {
        "true" | "True" => ParamValue::Bool(true),
        "false" | "False" => ParamValue::Bool(false),
        _ => match v.parse::<f64>() {
            Ok(n) => ParamValue::Number(n),
            Err(_) => ParamValue::Text(v.to_owned()),
        },
    }
}

/// Everything observable about one body, as four digests and a few numbers.
/// Split rather than lumped so a difference says *what* moved: geometry, the
/// face names selections resolve through, the edge names, or the vertex names.
fn signature(ctx: &mut cadpole_engine::cadpole_core::context::Context, long: bool) -> anyhow::Result<()> {
    let ids: Vec<_> = ctx.bodies.iter().filter(|b| b.visible).map(|b| b.id).collect();
    for id in ids {
        let name = ctx.body(id)?.name.clone();
        let kind = ctx.body(id)?.kind().to_owned();
        let (mesh, topo) = ctx.body_mesh(id)?;
        let (mut geom, mut faces, mut edges, mut verts) =
            (Digest::new(), Digest::new(), Digest::new(), Digest::new());
        for p in &mesh.positions {
            geom.eat(&p.to_bits().to_le_bytes());
        }
        for i in mesh.indices.iter().chain(&mesh.face_ids) {
            geom.eat(&i.to_le_bytes());
        }
        // The same triangles, as a set: each one written from its
        // lexicographically smallest corner so a rotation of the winding reads
        // the same, tagged with the face group it belongs to, and sorted. A
        // model whose `geometry_digest` moved but whose `triangle_set_digest`
        // did not is the *same* surface enumerated in a different order.
        let mut tris: Vec<String> = Vec::with_capacity(mesh.triangle_count());
        for t in 0..mesh.triangle_count() {
            let v = &mesh.indices[t * 3..t * 3 + 3];
            let corner = |k: usize| {
                let o = v[k] as usize * 3;
                (mesh.positions[o].to_bits(), mesh.positions[o + 1].to_bits(), mesh.positions[o + 2].to_bits())
            };
            let c = [corner(0), corner(1), corner(2)];
            let start = (0..3).min_by_key(|&k| c[k]).unwrap_or(0);
            tris.push(format!(
                "{}|{:?}|{:?}|{:?}",
                mesh.face_ids[t],
                c[start],
                c[(start + 1) % 3],
                c[(start + 2) % 3]
            ));
        }
        tris.sort_unstable();
        let mut set = Digest::new();
        for t in &tris {
            set.eat(t.as_bytes());
        }
        for f in &topo.faces {
            faces.eat(f.name.as_bytes());
            for a in &f.aliases {
                faces.eat(b"|");
                faces.eat(a.as_bytes());
            }
            faces.eat(f.surface_kind.as_bytes());
            faces.eat(&f.tri_count.to_le_bytes());
        }
        for e in &topo.edges {
            edges.eat(e.name.as_bytes());
        }
        for v in &topo.vertices {
            verts.eat(v.name.as_bytes());
        }
        let line = serde_json::json!({
            "body": name,
            "kind": kind,
            "triangles": mesh.triangle_count(),
            "faces": topo.faces.len(),
            "edges": topo.edges.len(),
            "vertices": topo.vertices.len(),
            "volume": format!("{:.9}", mesh.volume()),
            "watertight": mesh.welded(1e-4).is_watertight(),
            "solid": cadpole_engine::cadpole_core::solid::validate(mesh).is_solid(),
            "geometry_digest": geom.hex(),
            "triangle_set_digest": set.hex(),
            "face_names_digest": faces.hex(),
            "edge_names_digest": edges.hex(),
            "vertex_names_digest": verts.hex(),
        });
        println!("{line}");
        if long {
            for f in &topo.faces {
                println!("  face {} {:?} {} tris={}", f.name, f.aliases, f.surface_kind, f.tri_count);
            }
            for e in &topo.edges {
                println!("  edge {}", e.name);
            }
            for v in &topo.vertices {
                println!("  vert {}", v.name);
            }
        }
    }
    Ok(())
}

fn median(v: &mut [f64]) -> f64 {
    v.sort_by(|a, b| a.partial_cmp(b).unwrap());
    if v.is_empty() { 0.0 } else { v[v.len() / 2] }
}

type Params = std::collections::HashMap<String, cadpole_engine::cadpole_core::ParamValue>;

/// What the command line asked for.
#[derive(Debug)]
struct Args {
    repeat: usize,
    size: (u32, u32),
    files: Vec<String>,
    sig: bool,
    long: bool,
    drag: Option<(String, Vec<String>)>,
    preview: bool,
    no_memo: bool,
    params: Params,
}

const USAGE: &str = "usage: cadpole-bench [--repeat N] [--size WxH] [--param k=v] script.star...\n       \
                     cadpole-bench --signature [--long] [--param k=v] script.star...\n       \
                     cadpole-bench --drag k=v1,v2,... [--preview] [--no-memo] [--param k=v] script.star...";

fn parse_args(mut args: Vec<String>) -> anyhow::Result<Args> {
    let mut a = Args {
        repeat: 3,
        size: (800, 600),
        files: Vec::new(),
        sig: false,
        long: false,
        drag: None,
        preview: false,
        no_memo: false,
        params: Default::default(),
    };
    let mut args = args.drain(..);
    while let Some(arg) = args.next() {
        // A flag's value, or a usage error naming the flag: a trailing
        // `--repeat` used to panic on an empty argument list.
        let mut value = |flag: &str| args.next().ok_or_else(|| anyhow::anyhow!("{flag} needs a value\n{USAGE}"));
        match arg.as_str() {
            "--repeat" => {
                let v = value("--repeat")?;
                a.repeat = v.parse().map_err(|e| anyhow::anyhow!("--repeat {v:?}: {e}"))?;
                // No runs would print timings of 0 for a script never run.
                anyhow::ensure!(a.repeat >= 1, "--repeat is a number of runs of 1 or more, got 0");
            }
            "--signature" => a.sig = true,
            "--long" => a.long = true,
            "--preview" => a.preview = true,
            "--no-memo" => a.no_memo = true,
            "--drag" => {
                let kv = value("--drag")?;
                let (k, v) = kv.split_once('=').ok_or_else(|| anyhow::anyhow!("drag k=v1,v2,..."))?;
                a.drag = Some((k.to_owned(), v.split(',').map(str::to_owned).collect()));
            }
            "--param" => {
                let kv = value("--param")?;
                let (k, v) = kv.split_once('=').ok_or_else(|| anyhow::anyhow!("param k=v"))?;
                a.params.insert(k.to_owned(), param_value(v));
            }
            "--size" => {
                let s = value("--size")?;
                let (w, h) = s.split_once('x').ok_or_else(|| anyhow::anyhow!("size WxH"))?;
                a.size = (w.parse()?, h.parse()?);
            }
            _ => a.files.push(arg),
        }
    }
    if a.files.is_empty() {
        anyhow::bail!("{USAGE}");
    }
    Ok(a)
}

/// The scripts an evaluation of `path` is given, as `cadpole eval` gives
/// them (cadpole-cli's `input::load`): the file itself as `main.star`, and,
/// when it uses `load()`, every other `.star` beside it and whatever those
/// load from elsewhere, transitively, under the key the loader resolves.
/// Without them a script that loads a sibling was timed failing at its
/// first line.
fn scripts_for(path: &str) -> anyhow::Result<Vec<ScriptFile>> {
    let path = std::path::Path::new(path);
    let entry = std::fs::read_to_string(path)?;
    let uses_load = entry.contains("load(");
    let mut scripts = vec![ScriptFile { path: "main.star".into(), source: entry }];
    if !uses_load {
        return Ok(scripts);
    }
    let dir = path.parent().filter(|d| !d.as_os_str().is_empty()).unwrap_or(std::path::Path::new("."));
    let mut sibs: Vec<_> = std::fs::read_dir(dir)?
        .flatten()
        .map(|e| e.path())
        .filter(|p| p.file_name() != path.file_name() && p.extension().and_then(|e| e.to_str()) == Some("star"))
        .collect();
    sibs.sort();
    for p in sibs {
        if let (Some(name), Ok(source)) = (p.file_name().and_then(|n| n.to_str()), std::fs::read_to_string(&p)) {
            scripts.push(ScriptFile { path: name.to_owned(), source });
        }
    }
    // What the bundled scripts load from outside the directory.
    let mut i = 0;
    while i < scripts.len() {
        let (key, src) = (scripts[i].path.clone(), scripts[i].source.clone());
        for target in load_targets(&src) {
            if target.starts_with('@') {
                continue;
            }
            let k = cadpole_engine::cadpole_script::resolve_load(&key, &target);
            if k.is_empty() || scripts.iter().any(|s| s.path == k) {
                continue;
            }
            if let Ok(source) = std::fs::read_to_string(dir.join(&k)) {
                scripts.push(ScriptFile { path: k, source });
            }
        }
        i += 1;
    }
    Ok(scripts)
}

/// The first argument of every `load("...")` in `src`.
fn load_targets(src: &str) -> Vec<String> {
    let mut out = Vec::new();
    let mut rest = src;
    while let Some(i) = rest.find("load(") {
        let before = rest[..i].chars().last();
        rest = &rest[i + "load(".len()..];
        if before.is_some_and(|c| c.is_alphanumeric() || c == '_' || c == '.') {
            continue;
        }
        let t = rest.trim_start();
        let Some(q) = t.chars().next().filter(|c| *c == '"' || *c == '\'') else { continue };
        if let Some(end) = t[1..].find(q) {
            out.push(t[1..1 + end].to_owned());
        }
    }
    out
}

fn main() -> anyhow::Result<()> {
    // The CLI's solver, so `opt` scripts are measured as `cadpole eval`
    // runs them, solve included.
    cadpole_opt::install();
    let Args { repeat, size, files, sig, long, drag, preview, no_memo, params } = parse_args(std::env::args().skip(1).collect())?;
    for f in files {
        let scripts = scripts_for(&f)?;
        // Scripts that call `asset("logo.svg")` read from `assets/` next to
        // themselves, the same "loose" layout the CLI uses. Without this the
        // harness timed a script that had already failed: `svg_badge.star`
        // reported one error, no bodies and a 0.1 ms evaluation, which reads
        // like the fastest script in the set rather than a broken measurement.
        let dir = std::path::Path::new(&f).parent().unwrap_or(std::path::Path::new(".")).join("assets");
        let (store, warnings) = cadpole_engine::cadpole_assets::load_dir(&dir);
        for w in warnings {
            eprintln!("warning: {w}");
        }
        let assets: Option<Arc<dyn cadpole_engine::cadpole_assets::AssetStore>> = Some(Arc::new(store));
        if let Some((name, values)) = &drag {
            let memo = if no_memo { None } else { Some(Arc::new(cadpole_engine::cadpole_core::memo::Memo::new())) };
            let run = |step: &str,
                           overrides: std::collections::HashMap<String, cadpole_engine::cadpole_core::ParamValue>,
                           preview: bool|
             -> anyhow::Result<()> {
                let shown = overrides.get(name).map(cadpole_engine::value_label).unwrap_or_else(|| "default".into());
                let t0 = Instant::now();
                let mut ctx = cadpole_engine::cadpole_script::evaluate(EvalInput {
                    scripts: scripts.clone(),
                    assets: assets.clone(),
                    overrides,
                    memo: memo.clone(),
                    preview,
                    ..Default::default()
                });
                let eval_ms = t0.elapsed().as_secs_f64() * 1000.0;
                let errors = ctx
                    .diagnostics
                    .iter()
                    .filter(|d| d.severity == cadpole_engine::cadpole_core::context::Severity::Error)
                    .count();
                // What the UI does next: the display mesh of every visible body.
                let ids: Vec<_> = ctx.bodies.iter().filter(|b| b.visible).map(|b| b.id).collect();
                let t1 = Instant::now();
                let mut tris = 0usize;
                for id in &ids {
                    if let Ok((m, _)) = ctx.body_mesh(*id) {
                        tris += m.triangle_count();
                    }
                }
                let tessellate_ms = t1.elapsed().as_secs_f64() * 1000.0;
                let report = ctx.memo_report();
                let line = serde_json::json!({
                    "script": f,
                    "step": step,
                    "param": format!("{name}={shown}"),
                    "preview": preview,
                    "eval_ms": (eval_ms * 10.0).round() / 10.0,
                    "tessellate_ms": (tessellate_ms * 10.0).round() / 10.0,
                    "total_ms": ((eval_ms + tessellate_ms) * 10.0).round() / 10.0,
                    "ops_run": report.ops_run,
                    "ops_hit": report.ops_hit,
                    "ops_deferred": report.ops_deferred,
                    "memo_entries": report.entries,
                    "memo_kb": report.bytes / 1024,
                    "bodies": ids.len(),
                    "triangles": tris,
                    "errors": errors,
                    "profile": if cfg!(debug_assertions) { "debug" } else { "opt" },
                });
                println!("{line}");
                Ok(())
            };
            run("base", params.clone(), false)?;
            let mut last = params.clone();
            for v in values {
                let mut p = params.clone();
                p.insert(name.clone(), param_value(v));
                last = p.clone();
                run("drag", p, preview)?;
            }
            if preview {
                run("release", last, false)?;
            }
            continue;
        }
        if sig {
            let mut ctx = cadpole_engine::cadpole_script::evaluate(EvalInput {
                scripts: scripts.clone(),
                assets: assets.clone(),
                overrides: params.clone(),
                ..Default::default()
            });
            let errs: Vec<_> = ctx
                .diagnostics
                .iter()
                .filter(|d| d.severity == cadpole_engine::cadpole_core::context::Severity::Error)
                .map(|d| d.message.clone())
                .collect();
            if !errs.is_empty() {
                anyhow::bail!("{f}: {errs:?}");
            }
            println!("# {f}");
            signature(&mut ctx, long)?;
            continue;
        }
        let mut t_eval = Vec::new();
        let mut t_tess = Vec::new();
        let mut t_render = Vec::new();
        let mut tris = 0usize;
        let mut bodies = 0usize;
        let mut errors = 0usize;
        for _ in 0..repeat {
            let t0 = Instant::now();
            let mut ctx = cadpole_engine::cadpole_script::evaluate(EvalInput {
                scripts: scripts.clone(),
                assets: assets.clone(),
                overrides: params.clone(),
                ..Default::default()
            });
            t_eval.push(t0.elapsed().as_secs_f64() * 1000.0);
            errors = ctx.diagnostics.iter().filter(|d| d.severity == cadpole_engine::cadpole_core::context::Severity::Error).count();
            let ids: Vec<_> = ctx.bodies.iter().filter(|b| b.visible).map(|b| b.id).collect();
            bodies = ids.len();
            let t1 = Instant::now();
            tris = 0;
            for id in &ids {
                if let Ok((m, _)) = ctx.body_mesh(*id) {
                    tris += m.triangle_count();
                }
            }
            t_tess.push(t1.elapsed().as_secs_f64() * 1000.0);
            let t2 = Instant::now();
            let opts = RenderOptions { width: size.0, height: size.1, ..Default::default() };
            let img = cadpole_engine::render_context(&mut ctx, &None, &opts, &[]);
            std::hint::black_box(&img);
            t_render.push(t2.elapsed().as_secs_f64() * 1000.0);
        }
        let mut shown: Vec<(String, String)> =
            params.iter().map(|(k, v)| (k.clone(), cadpole_engine::value_label(v))).collect();
        shown.sort();
        let line = serde_json::json!({
            "script": f,
            "params": shown.iter().map(|(k, v)| format!("{k}={v}")).collect::<Vec<_>>().join(","),
            "bodies": bodies,
            "triangles": tris,
            "errors": errors,
            "eval_ms": median(&mut t_eval),
            "tessellate_ms": median(&mut t_tess),
            "render_ms": median(&mut t_render),
            "render_size": format!("{}x{}", size.0, size.1),
            "repeat": repeat,
            "profile": if cfg!(debug_assertions) { "debug" } else { "opt" },
        });
        println!("{line}");
        if errors > 0 {
            eprintln!("warning: {f} reported {errors} error(s); its timings measure a failed run");
        }
    }
    Ok(())
}

#[cfg(test)]
mod tests;
