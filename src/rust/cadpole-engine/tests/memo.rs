//! The kernel-op memo, tested by counts, never milliseconds.
//! What the memo is *for* is a wall-clock number, and a wall-clock
//! budget on a shared box is how a benchmark becomes a flaky test; what the
//! memo *did* — which ops ran, which were served, which the preview
//! deferred — is a fact about the code that reads the same on any machine.
//! The timings are `cadpole-bench --drag`'s job, on the opt build.
//!
//! Its own binary because the compressor wheel is evaluated cold several
//! times here, which is slow in a debug build.

use std::collections::HashMap;
use std::sync::Arc;

use cadpole_engine::cadpole_assets::AssetStore;
use cadpole_engine::cadpole_core::context::{Context, Severity};
use cadpole_engine::cadpole_core::memo::Memo;
use cadpole_engine::cadpole_core::{BodyId, ParamValue};
use cadpole_engine::cadpole_script::{self, EvalInput, ScriptFile};

fn examples_dir() -> String {
    std::env::var("CADPOLE_EXAMPLES").unwrap_or_else(|_| "examples".into())
}

/// Every runnable example, sorted: not `*_future.star`, not planned-api.
fn examples() -> Vec<std::path::PathBuf> {
    let dir = examples_dir();
    let mut files: Vec<_> = std::fs::read_dir(&dir)
        .expect("the examples directory")
        .flatten()
        .map(|e| e.path())
        .filter(|p| p.extension().and_then(|e| e.to_str()) == Some("star"))
        .filter(|p| !p.file_name().and_then(|n| n.to_str()).is_some_and(|n| n.ends_with("_future.star")))
        .filter(|p| {
            let src = std::fs::read_to_string(p).unwrap_or_default();
            !src.lines().next().unwrap_or("").contains("cadpole: planned-api")
        })
        .collect();
    files.sort();
    files
}

/// The script list the CLI builds for an example: the entry as `main.star`
/// plus every sibling under its own name.
fn scripts_for(entry: &std::path::Path) -> Vec<ScriptFile> {
    let dir = examples_dir();
    let mut scripts = vec![ScriptFile { path: "main.star".into(), source: std::fs::read_to_string(entry).unwrap() }];
    let mut sibs: Vec<_> = std::fs::read_dir(&dir)
        .unwrap()
        .flatten()
        .map(|e| e.path())
        .filter(|p| p != entry && p.extension().and_then(|e| e.to_str()) == Some("star"))
        .collect();
    sibs.sort();
    for p in sibs {
        scripts.push(ScriptFile {
            path: p.file_name().unwrap().to_str().unwrap().to_owned(),
            source: std::fs::read_to_string(&p).unwrap(),
        });
    }
    scripts
}

fn assets() -> Arc<dyn AssetStore> {
    let (store, _) = cadpole_engine::cadpole_assets::load_dir(std::path::Path::new(&examples_dir()).join("assets").as_path());
    Arc::new(store)
}

fn example(name: &str) -> Vec<ScriptFile> {
    scripts_for(&std::path::Path::new(&examples_dir()).join(name))
}

fn evaluate(
    scripts: &[ScriptFile],
    overrides: &[(&str, ParamValue)],
    memo: Option<Arc<Memo>>,
    preview: bool,
) -> Context {
    cadpole_script::evaluate(EvalInput {
        scripts: scripts.to_vec(),
        assets: Some(assets()),
        overrides: overrides.iter().map(|(k, v)| ((*k).to_owned(), v.clone())).collect(),
        memo,
        preview,
        ..Default::default()
    })
}

fn n(v: f64) -> ParamValue {
    ParamValue::Number(v)
}

/// Everything a caller can observe about an evaluation, as text: the
/// features with their args and outputs, every diagnostic in order, and per
/// visible body its ids, its names in topology order, and its triangles and
/// topology geometry to the bit. Two evaluations that print the same thing
/// produced the *same* model.
fn projection(ctx: &mut Context) -> String {
    let mut out = String::new();
    for f in &ctx.features {
        out.push_str(&format!(
            "feature {} {} {:?} inputs={:?} outputs={:?} args={:?}\n",
            f.id.0,
            f.kind,
            f.name,
            f.inputs.iter().map(|i| i.0.as_str()).collect::<Vec<_>>(),
            f.outputs.iter().map(|b| b.0).collect::<Vec<_>>(),
            f.args
        ));
        for d in &f.diagnostics {
            out.push_str(&format!("  fdiag {:?} {}\n", d.severity, d.message));
        }
    }
    for d in &ctx.diagnostics {
        out.push_str(&format!("diag {:?} {:?} {}\n", d.severity, d.feature.as_ref().map(|f| f.0.as_str()), d.message));
    }
    let ids: Vec<BodyId> = ctx.bodies.iter().filter(|b| b.visible).map(|b| b.id).collect();
    for id in ids {
        let (id_string, name, kind, key) = {
            let b = ctx.body(id).unwrap();
            (b.id_string(), b.name.clone(), b.kind().to_owned(), b.key.hex())
        };
        let (mesh, topo) = ctx.body_mesh(id).unwrap();
        let mut digest: u64 = 0xcbf2_9ce4_8422_2325;
        let mut eat = |bytes: &[u8]| {
            for b in bytes {
                digest ^= *b as u64;
                digest = digest.wrapping_mul(0x1000_0000_01b3);
            }
        };
        for p in &mesh.positions {
            eat(&p.to_bits().to_le_bytes());
        }
        for i in &mesh.indices {
            eat(&i.to_le_bytes());
        }
        for i in &mesh.face_ids {
            eat(&i.to_le_bytes());
        }
        for f in &topo.faces {
            for v in [f.centroid.x, f.centroid.y, f.centroid.z, f.normal.x, f.normal.y, f.normal.z, f.area] {
                eat(&v.to_bits().to_le_bytes());
            }
        }
        for e in &topo.edges {
            for v in [e.start.x, e.start.y, e.start.z, e.end.x, e.end.y, e.end.z, e.length] {
                eat(&v.to_bits().to_le_bytes());
            }
        }
        out.push_str(&format!(
            "body {id_string} {name:?} {kind} key={key} faces={:?} edges={:?} vertices={:?} tris={} digest={digest:016x}\n",
            topo.faces.iter().map(|f| f.name.as_str()).collect::<Vec<_>>(),
            topo.edges.iter().map(|e| e.name.as_str()).collect::<Vec<_>>(),
            topo.vertices.iter().map(|v| v.name.as_str()).collect::<Vec<_>>(),
            mesh.triangle_count()
        ));
    }
    out
}

fn errors(ctx: &Context) -> Vec<String> {
    ctx.diagnostics.iter().filter(|d| d.severity == Severity::Error).map(|d| d.message.clone()).collect()
}

/// Every example, three ways: with no memo, through a cold memo, and
/// through that memo again. All three print the same model — the names,
/// the diagnostics (a hit replays them) and the geometry to the bit — and
/// the warm run ran **zero** kernel ops, hitting every op the cold run
/// ran, in order (and possibly an entry a nested cold op made, when a
/// later top-level request asks for the same tessellation). Split in two
/// by index so each half is well inside the timeout in a debug build.
#[test]
fn every_example_is_identical_cold_warm_and_without_a_memo_and_the_warm_run_runs_nothing_first_half() {
    every_example_three_ways(0);
}

#[test]
fn every_example_is_identical_cold_warm_and_without_a_memo_and_the_warm_run_runs_nothing_second_half() {
    every_example_three_ways(1);
}

/// Is `part` a subsequence of `whole`?
fn in_order(part: &[cadpole_engine::cadpole_core::context::OpRecord], whole: &[cadpole_engine::cadpole_core::context::OpRecord]) -> bool {
    let mut it = whole.iter();
    part.iter().all(|p| it.any(|w| w == p))
}

fn every_example_three_ways(half: usize) {
    // The CLI's solver, so an `opt` example is checked as `cadpole
    // serve` runs it: its solve a hit in the warm evaluation like any op.
    cadpole_opt::install();
    let files: Vec<_> = examples().into_iter().enumerate().filter(|(i, _)| i % 2 == half).map(|(_, p)| p).collect();
    assert!(!files.is_empty());
    let mut checked = 0;
    let mut failures = Vec::new();
    for p in &files {
        let file = p.file_name().unwrap().to_str().unwrap().to_owned();
        let scripts = scripts_for(p);
        let memo = Arc::new(Memo::new());
        let mut plain = evaluate(&scripts, &[], None, false);
        let mut cold = evaluate(&scripts, &[], Some(memo.clone()), false);
        let mut warm = evaluate(&scripts, &[], Some(memo.clone()), false);
        let a = projection(&mut plain);
        let b = projection(&mut cold);
        let c = projection(&mut warm);
        // A body's key is the digest of what made it, and only a memoised
        // evaluation computes one; drop it before comparing against `plain`.
        let unkeyed = |s: &str| s.lines().map(|l| l.split(" key=").next().unwrap_or(l).to_owned()).collect::<Vec<_>>();
        if unkeyed(&a) != unkeyed(&b) {
            failures.push(format!("{file}: a cold evaluation through a memo differs from one without"));
        }
        if b != c {
            failures.push(format!("{file}: the warm evaluation differs from the cold one"));
        }
        if !warm.memo_counts.runs.is_empty() {
            failures.push(format!("{file}: the warm evaluation ran {:?}", warm.memo_counts.runs));
        }
        if !in_order(&cold.memo_counts.runs, &warm.memo_counts.hits) {
            failures.push(format!(
                "{file}: the warm evaluation hit {:?}, which does not cover what the cold one ran, {:?}",
                warm.memo_counts.hits, cold.memo_counts.runs
            ));
        }
        assert_eq!(plain.memo_counts.runs, cold.memo_counts.runs, "{file}: the same ops run with and without a memo");
        eprintln!(
            "{file}: {} ops cold, {} hits warm, {} entries, {} KB",
            cold.memo_counts.runs.len(),
            warm.memo_counts.hits.len(),
            memo.stats().entries,
            memo.stats().bytes / 1024
        );
        checked += 1;
    }
    assert!(failures.is_empty(), "{}", failures.join("\n"));
    assert!(checked > 10, "only {checked} examples checked");
}

/// The compressor wheel's parameters for the memo tests: the counterbore
/// on, and the rim scallops and the clamp nut off, so the op graph is the
/// one these tests count (a counterbore cut downstream of nothing, and no
/// cut that depends on the blade count).
fn wheel(extra: &[(&'static str, ParamValue)]) -> Vec<(&'static str, ParamValue)> {
    let mut v = vec![("counterbore", n(16.0)), ("scallops", n(0.0)), ("nut", ParamValue::Bool(false))];
    v.extend(extra.iter().cloned());
    v
}

/// The compressor wheel, by parameter: a parameter
/// that reaches only the counterbore re-runs the counterbore, the union of
/// the cuts and the final subtract, and nothing upstream of them — the
/// blade lofts, the pattern fuse and the hub union are hits. `blades`
/// re-runs exactly the four ops downstream of it and not the lofts, the
/// hub or the cuts.
#[test]
fn the_wheel_reruns_exactly_the_ops_downstream_of_the_changed_parameter() {
    let scripts = example("compressor_wheel.star");
    let memo = Arc::new(Memo::new());
    let cold = evaluate(&scripts, &wheel(&[]), Some(memo.clone()), false);
    assert!(errors(&cold).is_empty(), "{:?}", errors(&cold));
    let ran: Vec<String> = cold.memo_counts.runs.iter().map(|r| format!("{}:{}", r.feature, r.op)).collect();
    eprintln!("cold: {ran:?}");
    for f in ["blade", "splitter", "blade_pair", "blades", "hub", "wheel_solid", "bore_cut", "counterbore_cut", "cuts", "wheel"] {
        assert!(cold.memo_counts.runs.iter().any(|r| r.feature == f), "{f} did not run cold: {ran:?}");
    }

    let deeper = evaluate(&scripts, &wheel(&[("counterbore_depth", n(4.0))]), Some(memo.clone()), false);
    assert!(errors(&deeper).is_empty(), "{:?}", errors(&deeper));
    assert_eq!(deeper.memo_counts.run_features(), ["counterbore_cut", "cuts", "wheel"]);
    let hits = deeper.memo_counts.hit_features();
    for f in ["blade", "splitter", "blade_pair", "blades", "hub", "wheel_solid", "bore_cut"] {
        assert!(hits.contains(&f.to_owned()), "{f} should have hit; hits {hits:?}");
    }
    assert!(deeper.memo_counts.deferred.is_empty());

    let more = evaluate(&scripts, &wheel(&[("blades", n(8.0))]), Some(memo.clone()), false);
    assert!(errors(&more).is_empty(), "{:?}", errors(&more));
    assert_eq!(more.memo_counts.run_features(), ["blade_pair", "blades", "wheel_solid", "wheel"]);
    let hits = more.memo_counts.hit_features();
    for f in ["blade", "splitter", "hub", "bore_cut", "counterbore_cut", "cuts"] {
        assert!(hits.contains(&f.to_owned()), "{f} should have hit; hits {hits:?}");
    }

    // Back to the defaults: everything is in the memo already.
    let mut again = evaluate(&scripts, &wheel(&[]), Some(memo.clone()), false);
    assert!(again.memo_counts.runs.is_empty(), "{:?}", again.memo_counts.runs);
    let mut plain = evaluate(&scripts, &wheel(&[]), None, false);
    let strip = |s: String| s.lines().map(|l| l.split(" key=").next().unwrap_or(l).to_owned()).collect::<Vec<_>>();
    assert_eq!(strip(projection(&mut again)), strip(projection(&mut plain)));
}

/// The `blades` drag: with the memo warm, a preview at a new blade
/// count runs **no** kernel op at all — the lofts and the cuts are hits,
/// the rotation is a rigid copy — and defers exactly the four fuses the
/// parameter reaches, in order; the model still evaluates cleanly (the
/// script's own assertions hold on the operands); and the full evaluation
/// on release runs those four and only those.
#[test]
fn the_blades_drag_preview_defers_the_fuses_and_runs_nothing_then_the_release_runs_the_fuses() {
    let scripts = example("compressor_wheel.star");
    let memo = Arc::new(Memo::new());
    let cold = evaluate(&scripts, &wheel(&[]), Some(memo.clone()), false);
    assert!(errors(&cold).is_empty(), "{:?}", errors(&cold));
    let cold_ops = cold.memo_counts.runs.len();

    let preview = evaluate(&scripts, &wheel(&[("blades", n(8.0))]), Some(memo.clone()), true);
    assert!(errors(&preview).is_empty(), "{:?}", errors(&preview));
    assert!(preview.preview);
    // The operands are shown for the first time, so their display meshes
    // are made (a blade, the splitter, the hub: three tessellations); no
    // kernel op that makes geometry runs.
    let ran: Vec<&str> = preview.memo_counts.runs.iter().map(|r| r.op.as_str()).collect();
    assert!(ran.iter().all(|op| *op == "tessellate"), "a preview of a warm memo runs no kernel op: {ran:?}");
    assert_eq!(preview.memo_counts.deferred_features(), ["blade_pair", "blades", "wheel_solid", "wheel"]);
    let ops: Vec<&str> = preview.memo_counts.deferred.iter().map(|r| r.op.as_str()).collect();
    assert_eq!(ops, ["union", "pattern_circular", "union", "subtract"]);
    // The preview shows the operands: the blade, the splitter, the copies
    // and the hub, all visible, and says so on each deferred feature.
    let visible: Vec<&str> = preview.bodies.iter().filter(|b| b.visible).map(|b| b.name.as_str()).collect();
    assert!(visible.contains(&"splitter_placed"), "{visible:?}");
    assert!(visible.contains(&"blade_pair"), "{visible:?}");
    assert_eq!(visible.iter().filter(|n| **n == "blades").count(), 7, "{visible:?}");
    assert!(visible.contains(&"wheel"), "{visible:?}");
    for f in ["blade_pair", "blades", "wheel_solid", "wheel"] {
        let feature = preview.features.iter().find(|x| x.id.0 == f).unwrap();
        assert!(
            feature.diagnostics.iter().any(|d| d.message.contains("deferred while a parameter is being dragged")),
            "{f} should say it was deferred"
        );
    }
    let stored_before = memo.stats().stores;

    let release = evaluate(&scripts, &wheel(&[("blades", n(8.0))]), Some(memo.clone()), false);
    assert!(errors(&release).is_empty(), "{:?}", errors(&release));
    assert_eq!(release.memo_counts.run_features(), ["blade_pair", "blades", "wheel_solid", "wheel"]);
    assert!(release.memo_counts.deferred.is_empty());
    assert!(memo.stats().stores > stored_before, "the release stored what it ran");
    assert!(release.memo_counts.runs.len() < cold_ops);
    let visible: Vec<&str> = release.bodies.iter().filter(|b| b.visible).map(|b| b.name.as_str()).collect();
    assert_eq!(visible, ["wheel"]);

    // A preview never stored a stand-in: the same preview again is
    // every op a hit against the entries the release made, and nothing
    // deferred.
    let again = evaluate(&scripts, &wheel(&[("blades", n(8.0))]), Some(memo.clone()), true);
    assert!(again.memo_counts.deferred.is_empty(), "{:?}", again.memo_counts.deferred);
    assert!(again.memo_counts.runs.is_empty(), "{:?}", again.memo_counts.runs);
}

/// A memo too small for the model evicts, and what it hands back is still
/// right: with a bound well under the wheel's working set its second run is
/// partly misses, the store never exceeds its bound, and the model is
/// byte-identical to an unmemoised one. The bound is found from the model
/// rather than fixed: half the unbounded working set, halved again while
/// the entries that fit under it still fit without evicting (an entry
/// larger than the bound is never stored, so one large entry can leave the
/// rest under it).
#[test]
fn the_memory_bound_is_respected_and_a_partly_evicted_memo_is_still_correct() {
    let scripts = example("compressor_wheel.star");
    let unbounded = Arc::new(Memo::new());
    let full = evaluate(&scripts, &wheel(&[]), Some(unbounded.clone()), false);
    assert!(errors(&full).is_empty(), "{:?}", errors(&full));
    let total = unbounded.stats().bytes;
    let mut limit = total / 2;
    let (memo, cold) = loop {
        let memo = Arc::new(Memo::with_limit(limit));
        let cold = evaluate(&scripts, &wheel(&[]), Some(memo.clone()), false);
        assert!(errors(&cold).is_empty(), "{:?}", errors(&cold));
        if memo.stats().evictions > 0 || limit < 64 * 1024 {
            break (memo, cold);
        }
        limit /= 2;
    };
    let s = memo.stats();
    eprintln!("bounded: {s:?} (unbounded {total} bytes)");
    assert!(s.bytes <= limit, "{s:?}");
    assert!(s.evictions > 0, "the wheel does not fit in {limit} bytes: {s:?}");
    let mut warm = evaluate(&scripts, &wheel(&[]), Some(memo.clone()), false);
    assert!(!warm.memo_counts.runs.is_empty(), "something was evicted, so something re-ran");
    // Every op is either a hit or a run; with a bound this far under the
    // working set the LRU may thrash (the hub's entry evicts the cuts'
    // before the warm run reaches them), which costs time and never
    // correctness.
    assert_eq!(warm.memo_counts.hits.len() + warm.memo_counts.runs.len(), cold.memo_counts.runs.len());
    assert!(memo.stats().bytes <= limit);
    assert!(memo.stats().evictions > s.evictions, "the warm run evicted too");
    let mut plain = evaluate(&scripts, &wheel(&[]), None, false);
    let strip = |s: String| s.lines().map(|l| l.split(" key=").next().unwrap_or(l).to_owned()).collect::<Vec<_>>();
    assert_eq!(strip(projection(&mut warm)), strip(projection(&mut plain)));
}

/// A hit replays what the op said: the routing notes, the kernel's ladder
/// notes and the mesh-kernel decisions come back on the same features in
/// the same order, and `cached` is true for exactly the features whose ops
/// were all hits.
#[test]
fn a_hit_replays_the_diagnostics_and_marks_the_feature_cached() {
    let scripts = example("gridfinity_bin.star");
    let memo = Arc::new(Memo::new());
    let cold = evaluate(&scripts, &[], Some(memo.clone()), false);
    let warm = evaluate(&scripts, &[], Some(memo.clone()), false);
    let diags = |c: &Context| {
        c.diagnostics
            .iter()
            .map(|d| format!("{:?} {:?} {}", d.severity, d.feature.as_ref().map(|f| f.0.clone()), d.message))
            .collect::<Vec<_>>()
    };
    assert_eq!(diags(&cold), diags(&warm));
    assert!(cold.diagnostics.iter().any(|d| d.message.contains("mesh kernel")), "the bin routes something to the mesh kernel");
    let cached: Vec<&str> = warm.features.iter().filter(|f| warm.feature_cached(&f.id.0)).map(|f| f.id.0.as_str()).collect();
    assert!(!cached.is_empty());
    for f in &cold.features {
        assert!(!cold.feature_cached(&f.id.0), "nothing is cached on a cold run");
    }
    let model = warm.memo_report();
    assert_eq!(model.ops_run, 0);
    assert!(model.ops_hit as usize >= cold.memo_counts.runs.len());
    assert_eq!(model.entries as usize, memo.stats().entries);
}

/// A memo whose kernel version is not this one is emptied on attach.
#[test]
fn a_one_shot_evaluation_pays_nothing_and_a_sweep_shares_one_memo() {
    let scripts = example("bracket.star");
    let plain = evaluate(&scripts, &[], None, false);
    assert!(plain.memo.is_none());
    assert_eq!(plain.memo_report().entries, 0);
    // The sweep: two cells that differ in one parameter share the memo, and
    // the second cell hits everything the parameter does not reach.
    let memo = Arc::new(Memo::new());
    let a = evaluate(&scripts, &[("width", n(60.0))], Some(memo.clone()), false);
    let b = evaluate(&scripts, &[("width", n(70.0))], Some(memo.clone()), false);
    assert!(!a.memo_counts.runs.is_empty());
    assert!(b.memo_counts.runs.len() <= a.memo_counts.runs.len());
    let _ = HashMap::<String, String>::new();
}
