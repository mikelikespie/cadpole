//! The process-wide evaluation deadline, end to end through the engine.
//!
//! `set_default_limits` is how a host bounds every evaluation that does not
//! carry a deadline of its own — `cadpole serve` calls it once at startup with
//! `--eval-deadline-ms`, the CLI with `Limits::batch()` — because neither
//! `RenderRequest` nor `ExportRequest` has a deadline field. Two links can
//! break silently: the engine can stop *reading* the global, and the merge
//! rule can stop treating a request as a tightening only. Nothing tested
//! either; `limits.rs`'s own tests deliberately avoid the globals, because
//! they are process-wide and a unit test that set a 200 ms deadline would
//! impose it on every other test evaluating a document in the same binary.
//!
//! So this is its own test *binary*: the globals it sets are private to this
//! process, and it can use a real, short deadline against a script that does
//! not terminate. It runs `--test-threads=1` (see the BUILD file) because the
//! limits are per-process: each test here sets the global it is about to rely
//! on, so they must not overlap with each other either.

use cadpole_engine::cadpole_proto::v1 as pb;
use cadpole_engine::{Limits, document_from_scripts, limits};

/// A script that will not finish on its own: about 10^9 loop back-edges, some
/// seconds of work even in an optimised build. The tick cap (200 M) would also
/// stop it eventually, so every assertion below checks the *deadline* fired,
/// by its message and by the time it took.
const RUNAWAY: &str = "total = 0\nfor i in range(1000000000):\n    total = total + i\n";

fn doc(src: &str) -> pb::Document {
    document_from_scripts("t", vec![("main.star".into(), src.into())])
}

fn errors(ctx: &cadpole_engine::cadpole_core::Context) -> Vec<String> {
    ctx.diagnostics
        .iter()
        .filter(|d| d.severity == cadpole_engine::cadpole_core::context::Severity::Error)
        .map(|d| d.message.clone())
        .collect()
}

/// The deadline a host installs is the one an evaluation that carries none of
/// its own actually runs under — the link `cadpole serve` depends on for every
/// render and export.
#[test]
fn the_process_deadline_stops_an_evaluation_that_carries_none_of_its_own() {
    limits::set_default_limits(Limits { deadline_ms: 250, ..Limits::structural() });
    assert_eq!(limits::default_limits().deadline_ms, 250, "the global is what was set");

    let start = std::time::Instant::now();
    let (ctx, ms) = cadpole_engine::evaluate_document(&doc(RUNAWAY), &Default::default(), 0.0);
    let elapsed = start.elapsed();

    let errs = errors(&ctx);
    assert_eq!(errs.len(), 1, "{errs:?}");
    assert!(errs[0].contains("evaluation stopped after"), "the deadline is what stopped it: {}", errs[0]);
    assert!(errs[0].contains("250 ms budget"), "and it names the budget it was given: {}", errs[0]);
    assert!(
        elapsed < std::time::Duration::from_secs(20),
        "it stopped near the budget rather than running to the tick cap: {elapsed:?}"
    );
    assert!(ms > 0.0, "the evaluation reports how long it took");
}

/// Every entry point that takes no deadline of its own picks up the same
/// global: `Evaluate` with an unset `deadline_ms` is the one the restricted
/// listener is granted.
#[test]
fn an_evaluate_request_without_a_deadline_gets_the_process_one() {
    limits::set_default_limits(Limits { deadline_ms: 250, ..Limits::structural() });

    let req = pb::EvaluateRequest { document: Some(doc(RUNAWAY)), ..Default::default() };
    let start = std::time::Instant::now();
    let resp = cadpole_engine::evaluate(&req);
    let elapsed = start.elapsed();

    let model = resp.model.expect("a model, even when the script was stopped");
    let stopped: Vec<_> = model
        .diagnostics
        .iter()
        .filter(|d| d.severity == pb::Severity::Error as i32)
        .map(|d| d.message.clone())
        .collect();
    assert_eq!(stopped.len(), 1, "{stopped:?}");
    assert!(stopped[0].contains("250 ms budget"), "{}", stopped[0]);
    assert!(elapsed < std::time::Duration::from_secs(20), "{elapsed:?}");
}

/// A request may ask for less and never for more. `capability.rs` grants the
/// listener `Evaluate` on the argument that it runs under the bounds the
/// server was started with, so this is that argument, exercised rather than
/// asserted: a caller asking for ten minutes still stops at the process's
/// quarter second.
#[test]
fn a_request_cannot_buy_itself_more_than_the_process_allows() {
    limits::set_default_limits(Limits { deadline_ms: 250, ..Limits::structural() });

    let greedy = pb::EvaluateRequest {
        document: Some(doc(RUNAWAY)),
        deadline_ms: 10 * 60 * 1000,
        max_ticks: u64::MAX,
        max_heap_bytes: 9_000_000_000,
        ..Default::default()
    };
    let start = std::time::Instant::now();
    let resp = cadpole_engine::evaluate(&greedy);
    let elapsed = start.elapsed();

    let model = resp.model.expect("a model");
    assert!(
        model
            .diagnostics
            .iter()
            .any(|d| d.severity == pb::Severity::Error as i32 && d.message.contains("250 ms budget")),
        "the process default bound it, not the ten minutes it asked for: {:?}",
        model.diagnostics.iter().map(|d| &d.message).collect::<Vec<_>>()
    );
    assert!(elapsed < std::time::Duration::from_secs(20), "{elapsed:?}");

    // ...and asking for less is still honoured.
    let modest = pb::EvaluateRequest { document: Some(doc(RUNAWAY)), deadline_ms: 50, ..Default::default() };
    let resp = cadpole_engine::evaluate(&modest);
    let model = resp.model.expect("a model");
    assert!(
        model.diagnostics.iter().any(|d| d.message.contains("50 ms budget")),
        "{:?}",
        model.diagnostics.iter().map(|d| &d.message).collect::<Vec<_>>()
    );
}

/// A host that sets no deadline is not bounded by one — the library default,
/// and what a test or a one-shot embedding gets. The other structural caps
/// are still read from the same global, and are what stop the same script.
#[test]
fn with_no_process_deadline_the_other_structural_caps_are_what_stop_a_runaway() {
    // A small tick budget rather than the 200 M default, so this is a second
    // and not a minute; the point is which limit fires, not how big it is.
    limits::set_default_limits(Limits { deadline_ms: 0, max_ticks: 2_000_000, ..Limits::structural() });
    let l = limits::default_limits();
    assert_eq!((l.deadline_ms, l.max_ticks), (0, 2_000_000), "every field comes from the global, not just the deadline");

    let (ctx, _) = cadpole_engine::evaluate_document(&doc(RUNAWAY), &Default::default(), 0.0);
    let errs = errors(&ctx);
    assert_eq!(errs.len(), 1, "{errs:?}");
    assert!(
        errs[0].contains("2000000 ticks") || errs[0].contains("ticks has been exceeded"),
        "no deadline fired, the tick budget did: {}",
        errs[0]
    );

    // Leave the process as a library embedder would find it.
    limits::set_default_limits(Limits::structural());
}
