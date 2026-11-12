//! The Cadpole Starlark API.
//!
//! `evaluate` runs a set of script files against a fresh [`cadpole_core::Context`]
//! and returns it, with every problem recorded as a diagnostic. Nothing here
//! panics on bad scripts.

mod animation;
mod apidoc;
mod assembly;
mod chamfer;
mod check;
mod clearance;
mod constrained;
mod convert;
mod emboss;
mod field;
mod fillet;
mod heightfield;
pub mod drawing;
mod globals;
mod import2d;
mod import3d;
mod layers;
mod mathns;
mod sketch2d;
pub mod limits;
mod modeltest;
pub mod nesting;
mod opt;
mod pattern;
pub mod plating;
mod primitives;
mod selection;
pub mod sheets;
mod fmtspec;
mod sightline;
pub mod stdlib;
mod store;
mod sun;
mod sweep;
mod terrain;
mod trig_lint;
mod units;
mod values;
mod vocab;

use std::cell::RefCell;
use std::collections::HashMap;
use std::sync::Arc;

use cadpole_assets::{AssetStore, NoAssets};
use cadpole_core::context::{Diagnostic, EntityRef, Span};
use cadpole_core::memo::Memo;
use cadpole_core::{Context, ParamValue};
use starlark::environment::{FrozenModule, Globals, GlobalsBuilder, Module};
use starlark::eval::{Evaluator, FileLoader};
use starlark::syntax::{AstModule, Dialect, DialectTypes};

pub use apidoc::api_doc;
pub use limits::{Deadline, Limits};
pub use drawing::OutputFile;
pub use modeltest::{Monotone, TestDecl, TestMode, TestOutcome, TestRun, point_label, run_tests};
pub use nesting::TooDeep;
pub use sheets::{resolve_load, sheet_files, SheetExport, SweepRequest, SweepRow};
pub use vocab::Vocabulary;
use store::Store;

/// A script that nests deeper than [`Limits::MAX_NESTING`], with the file it
/// is in. Raised by the loader before the file is parsed; `evaluate_full`
/// turns it into a diagnostic at the offending line.
#[derive(Debug)]
struct NestingError {
    path: String,
    inner: TooDeep,
}

impl std::fmt::Display for NestingError {
    fn fmt(&self, f: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
        write!(f, "{}:{}:{}: {}", self.path, self.inner.line, self.inner.col, self.inner)
    }
}

impl std::error::Error for NestingError {}

fn nesting_span(path: &str, e: &TooDeep) -> Span {
    Span { path: path.to_owned(), start_line: e.line, start_col: e.col, end_line: e.line, end_col: e.col + 1 }
}

#[derive(Clone, Debug)]
pub struct ScriptFile {
    pub path: String,
    pub source: String,
}

#[derive(Clone, Default)]
pub struct EvalInput {
    pub scripts: Vec<ScriptFile>,
    /// Entry point; defaults to "main.star" or the first script.
    pub entry: Option<String>,
    pub overrides: HashMap<String, ParamValue>,
    pub tolerance: f64,
    /// Boolean tolerance in mm; 0 means the kernel default. Independent of
    /// `tolerance` so a fine export mesh does not slow booleans down.
    pub boolean_tolerance: f64,
    pub selection: Vec<EntityRef>,
    /// Where `asset("name")` gets its bytes. Absent means the document has no
    /// assets, so every `asset(...)` call is a clean diagnostic.
    pub assets: Option<Arc<dyn AssetStore>>,
    /// What bounds this evaluation: wall clock, steps, recursion, heap
    /// (docs/security.md). `Limits::default()` is the structural caps with no
    /// deadline; `cadpole serve` and the CLI pass a deadline of their own.
    pub limits: Limits,
    /// What to do with the tests the script declares (`test(name, fn)`):
    /// nothing, which is every ordinary evaluation, or run some of them
    /// after the model is built. `run_tests` is the caller that sets it.
    pub tests: TestMode,
    /// The kernel-op memo this evaluation reads and fills: the
    /// session's or the wasm engine's, shared across evaluations and across
    /// sweep cells; `None` for a one-shot evaluation, which pays nothing.
    pub memo: Option<Arc<Memo>>,
    /// The drag preview: while a parameter is being dragged, a fuse
    /// or boolean whose inputs changed is skipped and its operands shown
    /// unfused; the evaluation after the drag is full.
    pub preview: bool,
    /// The rows of the document's `sweep_table(...)` calls, one list per
    /// call in order, as a host that evaluated the document at those
    /// configurations hands them back. `None` leaves every table
    /// pending, which is what an ordinary evaluation gets.
    pub sweep_results: Option<Arc<Vec<Vec<SweepRow>>>>,
}

impl std::fmt::Debug for EvalInput {
    fn fmt(&self, f: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
        f.debug_struct("EvalInput")
            .field("scripts", &self.scripts)
            .field("entry", &self.entry)
            .field("overrides", &self.overrides)
            .field("tolerance", &self.tolerance)
            .field("boolean_tolerance", &self.boolean_tolerance)
            .field("selection", &self.selection)
            .field("assets", &self.assets.as_ref().map(|a| a.names()))
            .field("limits", &self.limits)
            .field("tests", &self.tests)
            .field("memo", &self.memo.as_ref().map(|m| m.stats()))
            .field("preview", &self.preview)
            .finish()
    }
}

/// The dialect scripts are parsed with: standard Starlark plus f-strings,
/// keyword-only arguments and type annotations (checked at parse time only).
pub fn dialect() -> Dialect {
    Dialect {
        enable_f_strings: true,
        enable_types: DialectTypes::ParseOnly,
        enable_keyword_only_arguments: true,
        enable_positional_only_arguments: true,
        enable_load: true,
        enable_load_reexport: true,
        enable_top_level_stmt: true,
        ..Dialect::Standard
    }
}

/// The globals a Cadpole script sees: standard Starlark (no I/O, no `import`, no
/// environment), Cadpole's own API, and `print`.
///
/// `print` is the *only* extension enabled, and it is not a hole: it goes
/// through the [`starlark::PrintHandler`] on the store, which turns it into
/// capped info diagnostics instead of writing to this process's stderr — the
/// caller is a browser or an assistant, and neither can see stderr. The
/// extensions deliberately left out are `Debug`, `Internal` and `Breakpoint`
/// (docs/security.md, "Starlark sandbox"); adding any of them would hand a
/// script a way to inspect or stop the host.
pub fn globals() -> Globals {
    GlobalsBuilder::extended_by(&[starlark::environment::LibraryExtension::Print])
        .with(globals::cadpole_globals)
        .with(animation::animation_globals)
        .with(assembly::assembly_globals)
        .with(chamfer::chamfer_globals)
        .with(emboss::emboss_globals)
        .with(fillet::fillet_globals)
        .with(field::field_globals)
        .with(heightfield::heightfield_globals)
        .with(pattern::pattern_globals)
        .with(import2d::import2d_globals)
        .with(import3d::import_globals)
        .with(layers::layer_globals)
        .with(vocab::vocab_globals)
        .with(primitives::primitive_globals)
        .with(sketch2d::sketch2d_globals)
        .with(sun::sun_globals)
        .with(sightline::sightline_globals)
        .with(sweep::sweep_globals)
        .with(terrain::terrain_globals)
        .with(sheets::sheet_globals)
        .with(fmtspec::format_globals)
        .with(units::units_globals)
        .with_namespace("field", field::field_namespace)
        .with_namespace("math", mathns::math_namespace)
        .with_namespace("opt", opt::opt_namespace)
        .build()
}

/// Loads sibling script files for `load("lib.star", "helper")`. Each file is
/// evaluated once, against the same context, so features defined in libraries
/// join the same tree.
struct Loader<'a> {
    files: HashMap<String, String>,
    globals: &'a Globals,
    store: &'a Store,
    cache: RefCell<HashMap<String, FrozenModule>>,
    stack: RefCell<Vec<String>>,
}

fn normalize(path: &str) -> String {
    let p = path.trim_start_matches("//").trim_start_matches("./");
    p.to_owned()
}

impl<'a> Loader<'a> {
    fn eval_file(&self, path: &str) -> starlark::Result<FrozenModule> {
        // A relative path is taken from the loading file's directory:
        // `load("../lib/sheets.star")` from a sheet script finds
        // the library beside its model, and a bare name keeps resolving at
        // the document's root as it always has.
        let key = {
            let stack = self.stack.borrow();
            let from = stack.last().map(String::as_str).unwrap_or("");
            let relative = resolve_load(from, path);
            if self.files.contains_key(&relative) || stdlib::is_library(&relative) {
                relative
            } else {
                normalize(path)
            }
        };
        if let Some(m) = self.cache.borrow().get(&key) {
            return Ok(m.clone());
        }
        if self.stack.borrow().contains(&key) {
            return Err(starlark::Error::new_other(anyhow::anyhow!(
                "load cycle: {} -> {}",
                self.stack.borrow().join(" -> "),
                key
            )));
        }
        // A library label (`@cadpole/...`) resolves in the embedded standard
        // library and nowhere else; a bare name is a document file.
        let is_library = stdlib::is_library(&key);
        let source: String = if is_library {
            match stdlib::source(&key) {
                Some(s) => s.to_owned(),
                None => return Err(starlark::Error::new_other(anyhow::anyhow!("{}", stdlib::missing(&key)))),
            }
        } else {
            match self.files.get(&key) {
                Some(s) => s.clone(),
                None => {
                    let mut have: Vec<String> = self.files.keys().cloned().collect();
                    have.sort();
                    return Err(starlark::Error::new_other(anyhow::anyhow!(
                        "no script file named {key:?} (have: {}; the standard library is {})",
                        have.join(", "),
                        stdlib::names().join(", ")
                    )));
                }
            }
        };
        // Each level of this chain is a whole `Evaluator` on the native
        // stack, and the callstack cap does not see across files, so a
        // document of 1500 scripts each loading the next aborted the server.
        {
            let stack = self.stack.borrow();
            if stack.len() >= Limits::MAX_LOAD_DEPTH {
                let shown: Vec<&str> = stack.iter().take(3).map(String::as_str).collect();
                return Err(starlark::Error::new_other(anyhow::anyhow!(
                    "load nesting too deep: {} files deep at {key:?} (the limit is {}): {} -> ... -> {}",
                    stack.len() + 1,
                    Limits::MAX_LOAD_DEPTH,
                    shown.join(" -> "),
                    stack.last().map(String::as_str).unwrap_or("")
                )));
            }
        }
        // Before the parser, which recurses per level of nesting on the
        // native stack (see `nesting`).
        if let Err(inner) = nesting::check(&source, Limits::MAX_NESTING) {
            return Err(starlark::Error::new_other(NestingError { path: key, inner }));
        }
        // Radians handed to the degree trigonometry: a warning, on the text,
        // since the call itself cannot tell (`trig_lint`).
        let trig = trig_lint::check(&key, &source);
        if !trig.is_empty() {
            self.store.ctx.borrow_mut().diagnostics.extend(trig);
        }
        let ast = AstModule::parse(&key, source, &dialect())?;
        // The entry file is the one nothing loaded. Its body is the model,
        // so the tests -- every file's -- run once it has returned, against
        // the finished context and while its module is still live.
        let is_entry = self.stack.borrow().is_empty();
        let first_test = self.store.tests.borrow().len();
        self.stack.borrow_mut().push(key.clone());
        let store = self.store;
        let before = stdlib::counts(&store.ctx.borrow());
        let exports_before =
            (store.ctx.borrow().exports.len(), store.files.borrow().len(), store.sheet_exports.borrow().len());
        let result = Module::with_temp_heap(|module| -> starlark::Result<FrozenModule> {
            {
                let mut eval = Evaluator::new(&module);
                eval.extra = Some(store);
                eval.set_loader(self);
                apply_limits(&mut eval, store);
                eval.eval_module(ast, self.globals)?;
                // Loading a library records nothing: no feature, parameter
                // or export. A part file (`load("nightstand.star")`)
                // builds on load, on purpose, and is not under `@`.
                if is_library && let Some(msg) = stdlib::impurity(&key, before, &store.ctx.borrow()) {
                    return Err(starlark::Error::new_other(anyhow::anyhow!("{msg}")));
                }
                if is_entry {
                    modeltest::run_declared(&mut eval, store);
                }
            }
            Ok(module.freeze()?)
        });
        // A part file builds on load, but its `export()` calls are its own
        // document's business: a sheet script that loads
        // the model must not re-write the model's STEP files. What the
        // loaded file asked for is dropped, and said once.
        if !is_entry && !is_library {
            let mut ctx = store.ctx.borrow_mut();
            let dropped = ctx.exports.len().saturating_sub(exports_before.0)
                + store.files.borrow().len().saturating_sub(exports_before.1)
                + store.sheet_exports.borrow().len().saturating_sub(exports_before.2);
            if dropped > 0 {
                ctx.exports.truncate(exports_before.0);
                store.files.borrow_mut().truncate(exports_before.1);
                store.sheet_exports.borrow_mut().truncate(exports_before.2);
                ctx.diagnostics.push(Diagnostic::info(format!(
                    "load: {key} asked for {dropped} export{}; a loaded file's exports are not written from \
                     this document (only the entry script's export() calls are)",
                    if dropped == 1 { "" } else { "s" }
                )));
            }
        }
        self.stack.borrow_mut().pop();
        let frozen = result?;
        if !is_entry {
            modeltest::adopt_frozen(store, first_test, &frozen);
        }
        self.cache.borrow_mut().insert(key, frozen.clone());
        Ok(frozen)
    }
}

impl FileLoader for Loader<'_> {
    fn load(&self, path: &str) -> starlark::Result<FrozenModule> {
        self.eval_file(path)
    }
}

/// The error for a script that filled its heap cap, with the way to
/// raise it: `bytes` is the cap it ran under.
fn heap_message(bytes: usize) -> String {
    format!(
        "the script filled its {} MB Starlark heap: the values one file holds while it runs, each file it loads \
         counted on its own (docs/security.md, \"Starlark sandbox\"). A model of a few thousand bodies loaded by a \
         script that is itself loaded can need more: `cadpole --max-heap-mb N` (or CADPOLE_MAX_HEAP_MB=N) raises it for a \
         native run, `cadpole serve` included; the browser's engine and the apps keep 512 MB",
        bytes.div_ceil(1024 * 1024)
    )
}

/// Install the sandbox limits on one evaluator (docs/security.md).
///
/// Every loaded file gets its own `Evaluator`, so this runs once per file
/// while the [`Deadline`] and the print budget live on the shared [`Store`]
/// and therefore span the whole evaluation. The tick and heap caps are
/// per-file, which is the conservative direction: a hundred files cannot each
/// spend the whole budget, because the wall clock is shared.
fn apply_limits<'a>(eval: &mut Evaluator<'_, 'a, '_>, store: &'a Store) {
    let limits = store.limits;
    // `print` goes into the diagnostics, never to stdout: the caller is a
    // browser or an assistant, and neither can see this process's stderr.
    eval.set_print_handler(store);
    if limits.max_callstack > 0 {
        let _ = eval.set_max_callstack_size(limits.max_callstack);
    }
    if limits.max_heap_bytes > 0 {
        let _ = eval.set_max_heap_size(limits.max_heap_bytes);
    }
    if limits.max_ticks > 0 {
        let _ = eval.set_max_tick_count(limits.max_ticks);
    }
    if limits.deadline_ms > 0 {
        // starlark-rust polls this every 1000 "ticks" — function calls and
        // loop back-edges — so a Starlark loop is interrupted within about a
        // thousand iterations of the budget passing. This is what makes the
        // deadline real rather than advisory.
        eval.set_check_cancelled(Box::new(move || store.deadline.expired()));
    }
}

fn span_of(e: &starlark::Error) -> Option<Span> {
    let fs = e.span()?;
    let r = fs.resolve_span();
    Some(Span {
        path: fs.filename().to_owned(),
        start_line: r.begin.line as u32 + 1,
        start_col: r.begin.column as u32 + 1,
        end_line: r.end.line as u32 + 1,
        end_col: r.end.column as u32 + 1,
    })
}

/// Parse-only check, for validating a script before storing it. Returns the
/// syntax error as a diagnostic, or `None` when the file parses.
///
/// The nesting cap runs first, for the same reason it does in the loader:
/// the parse itself is what overflows on a deeply nested script, and this is
/// called on the server's request thread.
pub fn check_syntax(path: &str, source: &str) -> Option<Diagnostic> {
    if let Err(e) = nesting::check(source, Limits::MAX_NESTING) {
        return Some(Diagnostic::error(e.to_string()).with_span(Some(nesting_span(path, &e))));
    }
    match AstModule::parse(path, source.to_owned(), &dialect()) {
        Ok(_) => None,
        Err(e) => Some(Diagnostic::error(e.kind().to_string()).with_span(span_of(&e))),
    }
}

/// The native stack one evaluation runs on, off the caller's thread.
///
/// A tokio blocking worker has 2 MB and the CLI's main thread 8 MB, so on the
/// caller's thread the same script could succeed in one and abort the other. The nesting and
/// load caps bound what a script can ask for; this is the headroom that
/// makes the bound comfortable everywhere — the worst measured shape is
/// under a megabyte at the cap — and it is address space, not memory: only
/// the pages a stack touches are ever committed.
#[cfg(not(target_arch = "wasm32"))]
const EVAL_STACK_BYTES: usize = 64 << 20;

/// Run `f` on a thread with [`EVAL_STACK_BYTES`] of stack and hand back its
/// result. A panic in `f` resumes on the caller's thread, so a host's
/// `catch_unwind` still sees it. If the thread cannot be started at all, `f`
/// runs on the caller's thread as before.
#[cfg(not(target_arch = "wasm32"))]
fn on_eval_stack<T: Send, F: FnOnce() -> T + Send>(f: F) -> T {
    let slot = std::sync::Mutex::new(Some(f));
    let slot_ref = &slot;
    let ran = std::thread::scope(|s| {
        let handle = std::thread::Builder::new()
            .name("cadpole-eval".into())
            .stack_size(EVAL_STACK_BYTES)
            .spawn_scoped(s, move || (slot_ref.lock().unwrap().take().unwrap())());
        match handle {
            Ok(h) => Some(h.join().unwrap_or_else(|p| std::panic::resume_unwind(p))),
            Err(_) => None,
        }
    });
    match ran {
        Some(v) => v,
        None => (slot.into_inner().unwrap().take().unwrap())(),
    }
}

/// wasm32 has no threads; its stack overflow is a trap the host catches.
#[cfg(target_arch = "wasm32")]
fn on_eval_stack<T, F: FnOnce() -> T>(f: F) -> T {
    f()
}

/// What one evaluation produced: the model, and the files the script asked to
/// be written.
///
/// `export(...)` records its intent on the context (the host builds the mesh);
/// `export_svg` / `export_dxf` have already produced bytes, because a drawing
/// is cheap to build and would be expensive to describe. Either way the script
/// itself never touches the file system.
pub struct Evaluation {
    pub ctx: Context,
    pub files: Vec<OutputFile>,
    /// Every `test(...)` the script declared, whether or not it ran.
    pub tests: Vec<TestDecl>,
    /// The results of the tests [`EvalInput::tests`] asked for, in
    /// declaration order. Empty for an ordinary evaluation.
    pub outcomes: Vec<TestOutcome>,
    /// The sheet sets the script built, in order.
    pub sheet_sets: Vec<cadpole_drawing::SheetSet>,
    /// `export(set, path)` calls, for the host to write with
    /// `cadpole_drawing_out::write_set`.
    pub sheet_exports: Vec<SheetExport>,
    /// `report(name, value)` calls, in order.
    pub reports: Vec<(String, ParamValue)>,
    /// `sweep_table(...)` calls: what a host evaluates the document at
    /// before the export runs.
    pub sweep_requests: Vec<SweepRequest>,
}

impl Evaluation {
    /// An evaluation that never ran: the context alone.
    fn bare(ctx: Context) -> Evaluation {
        Evaluation {
            ctx,
            files: Vec::new(),
            tests: Vec::new(),
            outcomes: Vec::new(),
            sheet_sets: Vec::new(),
            sheet_exports: Vec::new(),
            reports: Vec::new(),
            sweep_requests: Vec::new(),
        }
    }
}

/// Evaluate scripts. Never fails: errors become diagnostics on the context.
pub fn evaluate(input: EvalInput) -> Context {
    evaluate_full(input).ctx
}

/// As [`evaluate`], also returning the files `export_svg` / `export_dxf`
/// produced.
pub fn evaluate_full(input: EvalInput) -> Evaluation {
    on_eval_stack(move || evaluate_here(input))
}

/// [`evaluate_full`], and when the script asked for `sweep_table(...)`
/// rows, the document evaluated once per configuration first --
/// outside the evaluation, which cannot re-enter itself -- and once more
/// with the rows handed in. A configuration's row carries every `report()`
/// that evaluation made, and `ok`/`error` from its diagnostics.
///
/// Every run shares one kernel memo: the caller's, or one made
/// here, so the last run rebuilds nothing the first built, and two
/// configurations share every op whose inputs they share. A document with
/// no `sweep_table()` is evaluated exactly once. Hosts call this where the
/// rows are worth their cost -- `--run-exports` and the `Drawing` rpc's
/// sheet preview; the viewer evaluates with `evaluate_full` and shows every
/// row pending.
pub fn evaluate_with_sweeps(mut input: EvalInput) -> Evaluation {
    if input.memo.is_none() {
        input.memo = Some(Arc::new(Memo::new()));
    }
    input.sweep_results = None;
    let first = evaluate_full(input.clone());
    let failed = first.ctx.diagnostics.iter().any(|d| d.severity == cadpole_core::context::Severity::Error);
    if first.sweep_requests.is_empty() || failed {
        return first;
    }
    let mut results: Vec<Vec<SweepRow>> = Vec::with_capacity(first.sweep_requests.len());
    for request in &first.sweep_requests {
        let mut rows = Vec::with_capacity(request.points.len());
        for (point, label) in request.points.iter().zip(&request.labels) {
            let mut at = input.clone();
            at.tests = TestMode::Skip;
            for (k, v) in point {
                at.overrides.insert(k.clone(), v.clone());
            }
            let ev = evaluate_full(at);
            let error = ev
                .ctx
                .diagnostics
                .iter()
                .find(|d| d.severity == cadpole_core::context::Severity::Error)
                .map(|d| d.message.clone());
            rows.push(SweepRow {
                point: point.clone(),
                label: label.clone(),
                ok: error.is_none(),
                error: error.unwrap_or_default(),
                values: ev.reports,
            });
        }
        results.push(rows);
    }
    input.sweep_results = Some(Arc::new(results));
    evaluate_full(input)
}

/// An override no `param()` declared changes nothing, and a typo (`-p
/// widht=80`) otherwise looks like a parameter with no effect. Said once the
/// script has run to the end (a script that stopped early may not have
/// reached the declaration), as a warning naming what the script does
/// declare.
fn warn_undeclared_overrides(ctx: &mut Context) {
    let mut unknown: Vec<&String> =
        ctx.overrides.keys().filter(|k| !ctx.params.iter().any(|(d, _)| &d.name == *k)).collect();
    if unknown.is_empty() {
        return;
    }
    unknown.sort();
    let mut declared: Vec<&str> = ctx.params.iter().map(|(d, _)| d.name.as_str()).collect();
    declared.sort_unstable();
    let declared = if declared.is_empty() { "none".to_owned() } else { declared.join(", ") };
    let messages: Vec<String> = unknown
        .into_iter()
        .map(|k| format!("override for {k:?} matches no param() in this script, so it has no effect (params: {declared})"))
        .collect();
    for m in messages {
        ctx.diagnostics.push(Diagnostic::warning(m));
    }
}

/// [`evaluate_full`] on the current thread.
fn evaluate_here(input: EvalInput) -> Evaluation {
    // The boolean tolerance ladder's finer rungs are one evaluation's.
    let _ladder = cadpole_core::kernel::ladder::scope();
    let mut ctx = Context::new();
    if input.tolerance > 0.0 {
        ctx.tolerance = input.tolerance;
    }
    if input.boolean_tolerance > 0.0 {
        ctx.boolean_tolerance = input.boolean_tolerance;
    }
    ctx.overrides = input.overrides;
    ctx.selection = input.selection;
    ctx.use_memo(input.memo.clone(), input.preview);

    let entry = input
        .entry
        .clone()
        .or_else(|| input.scripts.iter().find(|s| s.path == "main.star").map(|s| s.path.clone()))
        .or_else(|| input.scripts.first().map(|s| s.path.clone()));
    let Some(entry) = entry else {
        ctx.diagnostics.push(Diagnostic::error("document has no script files"));
        return Evaluation::bare(ctx);
    };

    // `@` names the libraries the engine carries; a document file
    // under it could only be an attempt to shadow one.
    if let Some(s) = input.scripts.iter().find(|s| stdlib::is_library(&normalize(&s.path))) {
        ctx.diagnostics.push(Diagnostic::error(format!(
            "script file {:?}: the `@` prefix is reserved for libraries (`{}` is the standard library, \
             built into the engine) and a document's own files cannot use it",
            s.path,
            stdlib::PREFIX
        )));
        return Evaluation::bare(ctx);
    }

    let assets: Arc<dyn AssetStore> = input.assets.clone().unwrap_or_else(|| Arc::new(NoAssets));
    let store = Store::with_limits(ctx, assets, input.limits)
        .with_tests(input.tests)
        .with_sweep_results(input.sweep_results.clone());
    let globals = globals();
    let loader = Loader {
        files: input.scripts.iter().map(|s| (normalize(&s.path), s.source.clone())).collect(),
        globals: &globals,
        store: &store,
        cache: RefCell::new(HashMap::new()),
        stack: RefCell::new(Vec::new()),
    };
    if let Err(e) = loader.eval_file(&entry) {
        let mut span = span_of(&e);
        // starlark reports a cancelled evaluation as "Evaluation cancelled",
        // which on its own tells nobody anything. When our deadline is what
        // cancelled it, say so, and say what to do about it.
        let msg = if store.deadline.fired() {
            store.deadline.message()
        } else if let starlark::ErrorKind::Other(inner) = e.kind()
            && let Some(n) = inner.downcast_ref::<NestingError>()
            && span.is_none()
        {
            // The entry file was too deep: the loader raised this before
            // anything had a span, so the span is the scan's.
            span = Some(nesting_span(&n.path, &n.inner));
            n.inner.to_string()
        } else if let Some(msg) = vocab::missing_member_message(&e) {
            // `kind.cylnder`: starlark's "no attribute" names only the type,
            // which is `vocabulary` for all of them; say which, and list the
            // members the way the string parser does.
            msg
        } else if e.kind().to_string().contains("Heap memory limit of") {
            // starlark's own words say nothing of what to do.
            heap_message(store.limits.max_heap_bytes)
        } else {
            e.kind().to_string()
        };
        // An error inside a library function: say where in the library and
        // where in the document, and point the span at the document.
        let (msg, span) = match stdlib::attribute(&e, &msg, span.as_ref()) {
            Some((m, s)) => (m, Some(s)),
            None => (msg, span),
        };
        store.ctx.borrow_mut().diagnostics.push(Diagnostic::error(msg).with_span(span));
    } else {
        warn_undeclared_overrides(&mut store.ctx.borrow_mut());
    }
    drop(loader);
    let f = store.finish();
    Evaluation {
        ctx: f.ctx,
        files: f.files,
        tests: f.tests.into_iter().map(|r| r.decl).collect(),
        outcomes: f.outcomes,
        sheet_sets: f.sheet_sets,
        sheet_exports: f.sheet_exports,
        reports: f.reports,
        sweep_requests: f.sweep_requests,
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use cadpole_core::fmath;

    /// `pub(crate)` so sibling modules' tests can use it: `mathns` tests the
    /// `math` namespace through Starlark, which is the only thing that proves
    /// the namespace is reachable and its argument conventions are real.
    pub(crate) fn run(src: &str) -> Context {
        // The CLI's solver, so a test script can `solve()` the way
        // `cadpole eval` does. Idempotent and process-wide.
        cadpole_opt::install();
        evaluate(EvalInput {
            scripts: vec![ScriptFile { path: "main.star".into(), source: src.into() }],
            ..Default::default()
        })
    }

    pub(crate) fn errors(ctx: &Context) -> Vec<String> {
        of_severity(ctx, cadpole_core::context::Severity::Error)
            .into_iter()
            .map(|(m, _)| m)
            .collect()
    }

    /// Messages of one severity, with the line each was reported on.
    fn of_severity(ctx: &Context, severity: cadpole_core::context::Severity) -> Vec<(String, u32)> {
        ctx.diagnostics
            .iter()
            .filter(|d| d.severity == severity)
            .map(|d| (d.message.clone(), d.span.as_ref().map(|s| s.start_line).unwrap_or(0)))
            .collect()
    }

    /// `Edge.radius` and `Edge.center`: the circle the edge's
    /// polyline fits, by the same fit `frame(edge)` uses, and `None` for an
    /// edge that is not an arc. A cylinder's rims answer; a box's edges do
    /// not; a hole a boolean cut answers too, whatever the kernel calls the
    /// curve.
    #[test]
    fn an_edge_reports_the_radius_and_centre_of_the_circle_it_fits() {
        let ctx = run(concat!(
            "c = cylinder(5, 10, name=\"c\")\n",
            "rims = [e for e in c.edges() if e.radius != None]\n",
            "assert_true(len(rims) >= 2)  # a rim is two arcs when the side is split at its seam\n",
            "top = [e for e in rims if abs(e.center[2] - 10) < 1e-6]\n",
            "assert_true(len(top) >= 1)\n",
            "assert_close(top[0].radius, 5, tol=1e-3)\n",
            "assert_close(top[0].center, (0, 0, 10), tol=1e-3)\n",
            "b = box(10, 10, 10, name=\"b\")\n",
            "assert_true(all([e.radius == None and e.center == None for e in b.edges()]))\n",
            "holed = subtract(box(20, 20, 5, name=\"plate\"), cylinder(3, 20, at=(5, 5, -5), name=\"tool\"), name=\"holed\")\n",
            "hole = [e for e in holed.edges() if e.radius != None]\n",
            "assert_true(len(hole) >= 2)\n",
            "assert_close(hole[0].radius, 3, tol=1e-3)\n",
            "assert_close(hole[0].center[0], 5, tol=1e-3)\n",
            "assert_close(hole[0].center[1], 5, tol=1e-3)\n",
            "assert_true(\"radius\" in dir(hole[0]) and \"center\" in dir(hole[0]))\n",
        ));
        assert_eq!(errors(&ctx), Vec::<String>::new());
    }

    /// The three tolerance levels: `body.tessellation(tolerance=)` beats
    /// `export(..., tolerance=)`, which beats the document's tolerance; the
    /// angle rides along on the body; and the export call records its own.
    #[test]
    fn a_body_tessellation_beats_the_export_call_which_beats_the_document() {
        let mut ctx = run(concat!(
            "own = cylinder(10, 10, name=\"own\").tessellation(tolerance=0.01, angle=4)\n",
            "other = cylinder(10, 10, at=(40, 0, 0), name=\"other\")\n",
            "export([own, other], \"x.stl\", tolerance=0.02)\n",
        ));
        assert!(errors(&ctx).is_empty(), "{:?}", errors(&ctx));
        let id_of = |ctx: &Context, n: &str| ctx.bodies.iter().find(|b| b.name == n).unwrap().id;
        let (own, other) = (id_of(&ctx, "own"), id_of(&ctx, "other"));
        assert_eq!(ctx.exports[0].tolerance, Some(0.02));
        use cadpole_core::context::ToleranceSource as S;
        let (o, src) = ctx.tessellation_for(own, Some(0.02)).unwrap();
        assert_eq!((o.tolerance, o.max_normal_degrees, src), (0.01, 4.0, S::Body));
        let (o, src) = ctx.tessellation_for(other, Some(0.02)).unwrap();
        assert_eq!((o.tolerance, src), (0.02, S::Call));
        let (o, src) = ctx.tessellation_for(other, None).unwrap();
        assert_eq!((o.tolerance, src), (cadpole_core::kernel::DEFAULT_TOLERANCE, S::Document));
        // The body's own level is what its mesh is made to, and finer is
        // more triangles.
        let fine = ctx.body_mesh(own).unwrap().0.triangle_count();
        let q = ctx.mesh_quality(own).unwrap().expect("a tessellated B-rep has a quality");
        assert_eq!((q.options.tolerance, q.source), (0.01, S::Body));
        let coarse = ctx.body_mesh(other).unwrap().0.triangle_count();
        assert!(fine > coarse, "{fine} vs {coarse}");
        // Asking for a call-level mesh replaces the cache with one made to
        // that level; asking again without one puts the document's back.
        let at_call = ctx.body_mesh_with(other, Some(0.02)).unwrap().0.triangle_count();
        assert!(at_call > coarse, "{at_call} vs {coarse}");
        assert_eq!(ctx.mesh_quality(other).unwrap().unwrap().source, S::Call);
        assert_eq!(ctx.body_mesh(other).unwrap().0.triangle_count(), coarse);
    }

    /// The chord error is measured, not assumed: on a cylinder it is the
    /// sagitta of the chord rule's step, under the tolerance and well above
    /// zero, and it shrinks with the tolerance while the triangle count
    /// grows.
    #[test]
    fn a_finer_tolerance_makes_more_triangles_and_the_measured_chord_error_stays_under_it() {
        let mut ctx = run(concat!(
            "fine = cylinder(10, 10, name=\"fine\").tessellation(tolerance=0.01)\n",
            "coarse = cylinder(10, 10, at=(40, 0, 0), name=\"coarse\").tessellation(tolerance=0.1)\n",
        ));
        assert!(errors(&ctx).is_empty(), "{:?}", errors(&ctx));
        let mut measured = Vec::new();
        for (name, tol) in [("fine", 0.01), ("coarse", 0.1)] {
            let id = ctx.bodies.iter().find(|b| b.name == name).unwrap().id;
            let n = ctx.body_mesh(id).unwrap().0.triangle_count();
            let q = ctx.mesh_quality(id).unwrap().unwrap();
            let e = q.chord_error().expect("measured");
            assert!(e <= tol, "{name}: chord error {e} over the tolerance {tol}");
            assert!(e >= tol / 5.0, "{name}: chord error {e} is not a real sagitta at {tol}");
            assert!(q.chord_face.contains("side"), "on the curved face: {}", q.chord_face);
            measured.push((n, e));
        }
        assert!(measured[0].0 > measured[1].0, "triangles {:?}", measured);
        assert!(measured[0].1 < measured[1].1, "chord errors {:?}", measured);
    }

    /// The chord-error-vs-gap rule reaches a model test the way every print
    /// finding does: `print_check` reports it as a warning and
    /// `assert_printable` fails on it. Two cylinders 0.3 mm apart at 0.5 mm
    /// warn (the arc rule's sagitta is a quarter of the tolerance:
    /// 0.12 mm, over a third of the gap); at 0.02 mm they do not.
    #[test]
    fn print_check_and_assert_printable_see_the_chord_error_against_the_gap() {
        let script = |tol: &str, check: &str| {
            format!(
                "a = cylinder(10, 10, name=\"a\").tessellation(tolerance={tol})\n\
                 b = cylinder(10, 10, at=(20.3, 0, 0), name=\"b\").tessellation(tolerance={tol})\n\
                 {check}\n"
            )
        };
        let ctx = run(&script("0.5", "r = print_check(a)\nassert_true(len([w for w in r.warnings if w.startswith(\"chord error\")]) == 1)\nassert_true(r.ok)"));
        assert!(errors(&ctx).is_empty(), "{:?}", errors(&ctx));
        let warnings = of_severity(&ctx, cadpole_core::context::Severity::Warning);
        let w = warnings.iter().find(|(m, _)| m.contains("[chord_error]")).map(|(m, _)| m.clone()).expect("a print_check warning");
        assert!(w.starts_with("print_check(a): [chord_error] chord error 0."), "{w}");
        assert!(w.contains("the gap to \"b\""), "{w}");

        let ctx = run(&script("0.5", "assert_printable(a)"));
        let e = errors(&ctx);
        assert_eq!(e.len(), 1, "{e:?}");
        assert!(e[0].contains("assert_printable failed: a: [chord_error]"), "{}", e[0]);

        let ctx = run(&script("0.02", "assert_printable(a)\nassert_printable(b)"));
        assert!(errors(&ctx).is_empty(), "{:?}", errors(&ctx));
        assert!(of_severity(&ctx, cadpole_core::context::Severity::Warning).is_empty());
    }

    /// A copy shares its source's mesh and a non-uniformly scaled body keeps
    /// the mesh `scale` remapped for it, so `tessellation()` on
    /// either is said to do nothing rather than silently doing nothing — and
    /// the scaled body's given mesh stays as given (its top's centroid is on
    /// the axis to the micron, which a fresh tessellation of the squashed
    /// cylinder is not); and a tolerance has to be a positive length.
    #[test]
    fn tessellation_on_a_copy_warns_and_bad_tolerances_are_refused() {
        let mut ctx = run(concat!(
            "a = cylinder(10, 10, name=\"a\")\n",
            "c = copy(a, offset=(40, 0, 0), name=\"c\").tessellation(tolerance=0.01)\n",
            "s = scale(cylinder(5, 12, name=\"c2\"), (2, 1, 1), name=\"s\").tessellation(tolerance=0.01)\n",
            "assert_close(s.face(\"c2/top\").centroid, (0, 0, 12))\n",
        ));
        assert!(errors(&ctx).is_empty(), "{:?}", errors(&ctx));
        let w = of_severity(&ctx, cadpole_core::context::Severity::Warning);
        assert_eq!(w.len(), 2, "{w:?}");
        assert!(w[0].0.contains("tessellation(c): a copy shares its source's tessellation"), "{}", w[0].0);
        assert_eq!(w[0].1, 2, "on the call");
        assert!(w[1].0.contains("tessellation(s): a scaled body remaps its source's tessellation"), "{}", w[1].0);
        let s = ctx.bodies.iter().find(|b| b.name == "s").unwrap().id;
        assert!(ctx.body(s).unwrap().has_given_mesh());
        let c2 = ctx.bodies.iter().find(|b| b.name == "c2").unwrap().id;
        let source = ctx.body_mesh(c2).unwrap().0.triangle_count();
        assert_eq!(ctx.body_mesh(s).unwrap().0.triangle_count(), source, "the remapped mesh, not a fresh one");
        assert!(ctx.mesh_quality(s).unwrap().is_none(), "nothing was measured on a given mesh");

        for (call, want) in [
            ("a.tessellation(tolerance=0)", "tolerance must be positive"),
            ("a.tessellation(angle=-1)", "angle must be zero (off) or positive"),
            ("a.tessellation()", "pass tolerance="),
            ("export(a, \"a.stl\", tolerance=-0.1)", "export: tolerance must be positive"),
        ] {
            let ctx = run(&format!("a = cylinder(10, 10, name=\"a\")\n{call}\n"));
            let e = errors(&ctx);
            assert_eq!(e.len(), 1, "{call}: {e:?}");
            assert!(e[0].contains(want), "{call}: {}", e[0]);
        }
    }

    #[test]
    fn an_export_path_needs_a_file_name_and_not_only_an_extension() {
        // The host writes `file_name()` under --out-dir, so `".stl"` asked for
        // a hidden dotfile and got one.
        let ctx = run("a = box(10, 10, 10)\nexport(a, \".stl\")\n");
        let e = errors(&ctx);
        assert_eq!(e.len(), 1, "{e:?}");
        assert!(e[0].contains("no file name in front of the extension"), "{}", e[0]);
        assert!(ctx.exports.is_empty());

        // A directory part is still fine: the host drops it.
        let ctx = run("a = box(10, 10, 10)\nexport(a, \"sub/part.stl\")\n");
        assert!(errors(&ctx).is_empty(), "{:?}", errors(&ctx));
        assert_eq!(ctx.exports.len(), 1);
        assert_eq!(ctx.exports[0].path, "sub/part.stl");
    }

    /// Volume and face count of a named body, from its tessellation. The two
    /// numbers together are a good "same solid?" test: volume catches a
    /// different shape, the face count catches the same volume built wrong.
    fn solid(ctx: &mut Context, name: &str) -> (f64, usize) {
        let id = ctx
            .bodies
            .iter()
            .find(|b| b.name == name)
            .unwrap_or_else(|| panic!("no body named {name:?}"))
            .id;
        let (mesh, topo) = ctx.body_mesh(id).unwrap();
        (mesh.volume(), topo.faces.len())
    }

    /// Run a script with one parameter overridden.
    fn run_with(src: &str, param: &str, value: f64) -> Context {
        let mut overrides = HashMap::new();
        overrides.insert(param.to_owned(), ParamValue::Number(value));
        evaluate(EvalInput {
            scripts: vec![ScriptFile { path: "main.star".into(), source: src.into() }],
            overrides,
            ..Default::default()
        })
    }

    /// Evaluate and return both halves: the model and the files the script
    /// asked for.
    fn run_full(src: &str) -> Evaluation {
        evaluate_full(EvalInput {
            scripts: vec![ScriptFile { path: "main.star".into(), source: src.into() }],
            ..Default::default()
        })
    }

    // ------------------------------------------------------------- loft, cone

    /// The property the foot rests on: a loft of two identical sections is a
    /// prism, so its volume is area x height however the sections were written.
    #[test]
    fn a_loft_of_two_identical_sections_is_a_prism() {
        let mut ctx = run(r#"
a = sketch(XY).rect(12, 12)
b = sketch(XY.offset(7)).rect(12, 12)
p = loft([a, b], name="prism")
"#);
        assert!(errors(&ctx).is_empty(), "{:?}", errors(&ctx));
        let (v, faces) = solid(&mut ctx, "prism");
        assert!((v - 12.0 * 12.0 * 7.0).abs() < 1e-6, "volume {v}");
        assert_eq!(faces, 6);
    }

    /// ...and a loft of a square to a point is a pyramid.
    #[test]
    fn a_loft_to_a_point_is_a_pyramid() {
        let mut ctx = run(r#"
p = loft([sketch(XY).rect(10, 10), (0, 0, 12)], name="tip")
"#);
        assert!(errors(&ctx).is_empty(), "{:?}", errors(&ctx));
        let (v, faces) = solid(&mut ctx, "tip");
        assert!((v - 100.0 * 12.0 / 3.0).abs() < 1e-6, "volume {v}");
        assert_eq!(faces, 5, "4 triangles + the base");
    }

    /// The Gridfinity foot, from the script. Its lateral surface is the exact
    /// reference profile: every section is the set of points a distance
    /// `4 - inset` from one 17 mm half-square, because `21 - inset` minus
    /// `4 - inset` is 17 whatever the inset. A stepped foot misses that by up
    /// to a whole step.
    #[test]
    fn the_lofted_gridfinity_foot_lies_on_the_reference_profile() {
        let mut ctx = run(r#"
def ring(inset, z):
    size = 42.0 - 2 * inset
    return sketch(XY.offset(z)).rounded_rect(size, size, 4.0 - inset)

foot = loft([ring(3.2, -5.0), ring(2.4, -4.2), ring(2.4, -2.4), ring(0.0, 0.0)], name="foot")
"#);
        assert!(errors(&ctx).is_empty(), "{:?}", errors(&ctx));
        let id = ctx.bodies.iter().find(|b| b.name == "foot").unwrap().id;
        let (mesh, topo) = ctx.body_mesh(id).unwrap();
        assert_eq!(topo.faces.len(), 26, "3 bands x 8 segments + 2 caps");
        let inset_at = |z: f64| if z >= -2.4 { -z } else if z >= -4.2 { 2.4 } else { 2.4 + (-4.2 - z) };
        let mut worst: f64 = 0.0;
        let mut checked = 0;
        for q in mesh.positions.chunks_exact(3) {
            let (x, y, z) = (q[0] as f64, q[1] as f64, q[2] as f64);
            if z > -1e-6 || z < -5.0 + 1e-6 {
                continue;
            }
            let (dx, dy) = ((x.abs() - 17.0).max(0.0), (y.abs() - 17.0).max(0.0));
            worst = worst.max(((dx * dx + dy * dy).sqrt() - (4.0 - inset_at(z))).abs());
            checked += 1;
        }
        // A cone band meshes as a strip and a flat flank as a boundary-only
        // polygon, so every lateral vertex is on one of the two interior rims.
        assert!(checked > 60, "{checked} lateral vertices");
        assert!(worst < 5e-6, "worst deviation {worst} mm (f32 mesh precision is ~1.3e-6)");
        // Between the rims the mesh has nothing to sample; the ruling lines do.
        // A rung's polyline is its two ends, in f64, so a point partway up it
        // is the exact surface, not the mesh.
        let mut rungs = 0;
        let mut worst_rung: f64 = 0.0;
        for e in &topo.edges {
            let [a, b] = e.polyline.as_slice() else { continue };
            if (a.z - b.z).abs() < 1e-9 {
                continue;
            }
            for t in [0.25, 0.5, 0.75] {
                let p = *a + (*b - *a) * t;
                let (dx, dy) = ((p.x.abs() - 17.0).max(0.0), (p.y.abs() - 17.0).max(0.0));
                worst_rung = worst_rung.max(((dx * dx + dy * dy).sqrt() - (4.0 - inset_at(p.z))).abs());
            }
            rungs += 1;
        }
        assert_eq!(rungs, 24, "8 ruling lines per band, three bands");
        assert!(worst_rung < 1e-9, "a ruling line leaves the true profile by {worst_rung} mm");
    }

    /// A side face of every band is selectable by name, and the unsuffixed name
    /// is the whole column.
    #[test]
    fn a_script_can_select_one_band_of_a_loft() {
        let ctx = run(r#"
def ring(inset, z):
    size = 42.0 - 2 * inset
    return sketch(XY.offset(z)).rounded_rect(size, size, 4.0 - inset)

foot = loft([ring(3.2, -5.0), ring(2.4, -4.2), ring(2.4, -2.4), ring(0.0, 0.0)], name="foot")
chamfer = foot.face("foot/side[0.1]#2")
if chamfer.name != "foot/side[0.1]#2":
    fail("got " + chamfer.name)
if "foot/side[0.1]" not in chamfer.aliases:
    fail("the column alias is missing: " + str(chamfer.aliases))
cap = foot.face("start")
if cap.name != "foot/start":
    fail("got " + cap.name)
"#);
        assert!(errors(&ctx).is_empty(), "{:?}", errors(&ctx));
    }

    /// Two evaluations of the same script agree, and nudging a parameter moves
    /// the surface instead of re-pairing the sections (which would twist the
    /// solid and jump the volume).
    #[test]
    fn a_loft_is_reproducible_and_a_parameter_nudge_does_not_twist_it() {
        let src = r#"
k = param("k", 0.0)
def ring(inset, z):
    size = 42.0 - 2 * (inset + k)
    return sketch(XY.offset(z)).rounded_rect(size, size, 4.0 - inset - k)

foot = loft([ring(3.2, -5.0), ring(2.4, -4.2), ring(2.4, -2.4), ring(0.0, 0.0)], name="foot")
"#;
        let mut a = run(src);
        let mut b = run(src);
        assert_eq!(solid(&mut a, "foot"), solid(&mut b, "foot"));
        let mut prev = solid(&mut a, "foot");
        for k in [0.01, 0.02, 0.03] {
            let mut ctx = run_with(src, "k", k);
            assert!(errors(&ctx).is_empty(), "{:?}", errors(&ctx));
            let now = solid(&mut ctx, "foot");
            assert_eq!(now.1, prev.1, "the face count must not move");
            assert!((now.0 - prev.0).abs() / prev.0 < 0.01, "volume jumped {} -> {}", prev.0, now.0);
            prev = now;
        }
    }

    /// A bossed rim shell, from a script: a plain rounded-rectangle wall
    /// ring lofted 15 mm up to a ring that detours in a U around each of four
    /// magnet bosses. 8 segments against 24.
    ///
    /// `boss` is the boss radius, so a nudge can be dialled in.
    fn brim_script(boss: f64) -> String {
        format!(
            r#"
h = 18.4          # the detoured ring's half-width (inset 2.6 of a 42 mm cell)
r = 4.9           # its corner radius
rb = {boss}       # magnet boss radius
d = 5.15          # how far in from the wall the boss centre sits
k = 0.7071067811865476

def rot(p, q):
    """`p` turned `q` quarter-turns counter-clockwise."""
    x = p[0]
    y = p[1]
    for _ in range(q):
        x, y = -y, x
    return (x, y)

def ring():
    """Four walls, each: wall, flank, over the boss, flank, wall, corner arc."""
    p = path((-h + r, -h))
    for q in range(4):
        p = p.line(rot((-rb, -h), q))
        p = p.line(rot((-rb, -h + d), q))
        p = p.arc(rot((rb, -h + d), q), via=rot((0.0, -h + d + rb), q))
        p = p.line(rot((rb, -h), q))
        p = p.line(rot((h - r, -h), q))
        p = p.arc(rot((h, -h + r), q), via=rot((h - r + r * k, -h + r - r * k), q))
    return p

plain = sketch(XY).rounded_rect(39.6, 39.6, 6.3)
detoured = sketch(XY.offset(15)).add(ring())
brim = loft([plain, detoured], match="arclen", name="brim")
"#
        )
    }

    /// The acceptance case: it builds, it is one watertight solid of positive
    /// volume with no self-intersections, and two evaluations agree bit for
    /// bit. 30 faces, against the 362 a discretised pairing would leave.
    #[test]
    fn the_base_brim_rim_shell_lofts_with_arc_length_matching() {
        let src = brim_script(4.025);
        let mut ctx = run(&src);
        assert!(errors(&ctx).is_empty(), "{:?}", errors(&ctx));
        assert!(
            of_severity(&ctx, cadpole_core::context::Severity::Warning).is_empty(),
            "{:?}",
            of_severity(&ctx, cadpole_core::context::Severity::Warning)
        );
        let (v, faces) = solid(&mut ctx, "brim");
        assert!(v > 0.0, "volume {v} mm^3");
        assert_eq!(faces, 30, "28 matched bands + 2 caps, against 362 for 360 chords");

        // Watertight, non-inverted, and nothing passing through anything else,
        // on the same checks `cadpole check` runs.
        let id = ctx.bodies.iter().find(|b| b.name == "brim").unwrap().id;
        let (mesh, topo1) = {
            let (m, t) = ctx.body_mesh(id).unwrap();
            (m.clone(), t.faces.iter().map(|f| f.name.clone()).collect::<Vec<_>>())
        };
        let welded = mesh.welded(cadpole_print::WELD_EPS);
        let mut issues = Vec::new();
        let report = cadpole_print::mesh_checks::check(&mesh, &welded, &mut issues);
        assert!(report.watertight, "not watertight: {} boundary edges", report.boundary_edges);
        assert!(!report.inverted, "the shell is inside out");
        assert!(report.non_manifold.is_empty(), "{:?}", report.non_manifold);
        assert!(!report.self_intersection_skipped, "the self-intersection check was skipped");
        assert!(
            report.self_intersections.is_empty(),
            "{} self-intersecting triangle pairs",
            report.self_intersections.len()
        );

        // Byte for byte reproducible.
        let mut again = run(&src);
        let id2 = again.bodies.iter().find(|b| b.name == "brim").unwrap().id;
        let (mesh2, topo2) = again.body_mesh(id2).unwrap();
        assert_eq!(mesh.positions, mesh2.positions, "two evaluations disagree");
        assert_eq!(mesh.indices, mesh2.indices);
        let n2: Vec<String> = topo2.faces.iter().map(|f| f.name.clone()).collect();
        assert_eq!(topo1, n2);
    }

    /// ...and growing the bosses moves the surface rather than re-pairing the
    /// rings: the face count holds and the volume follows smoothly.
    #[test]
    fn nudging_the_brim_bosses_does_not_re_pair_the_rings() {
        let mut base = run(&brim_script(4.025));
        let (v0, f0) = solid(&mut base, "brim");
        for boss in [4.075, 4.125, 4.225] {
            let mut ctx = run(&brim_script(boss));
            assert!(errors(&ctx).is_empty(), "{:?}", errors(&ctx));
            let (v, f) = solid(&mut ctx, "brim");
            assert_eq!(f, f0, "the face count moved at rb = {boss}");
            assert!((v - v0).abs() / v0.abs() < 0.05, "volume jumped {v0} -> {v}");
        }
    }

    /// `match=` is opt-in: without it the mismatch is still an error, and the
    /// error says which argument to reach for.
    #[test]
    fn unequal_sections_are_still_refused_unless_the_matcher_is_asked_for() {
        let ctx = run(r#"
a = sketch(XY).rounded_rect(20, 20, 3)
b = sketch(XY.offset(5)).ngon(12, 6)
loft([a, b], name="mismatched")
"#);
        let errs = errors(&ctx);
        assert!(errs.iter().any(|e| e.contains("match=\"arclen\"")), "{errs:?}");

        // ...and with it, the same two sections build.
        let mut ok = run(r#"
a = sketch(XY).rounded_rect(20, 20, 3)
b = sketch(XY.offset(5)).ngon(12, 6)
loft([a, b], match="arclen", name="matched")
"#);
        assert!(errors(&ok).is_empty(), "{:?}", errors(&ok));
        let (v, faces) = solid(&mut ok, "matched");
        assert!(v > 0.0, "volume {v}");
        // 8 vertices and 12, of which four fall at the same fraction round the
        // loop (the square's corners and every third vertex of the 12-gon).
        assert_eq!(faces, 16 + 2, "16 matched bands + 2 caps");
    }

    /// An unknown matching rule names the ones that exist.
    #[test]
    fn an_unknown_match_argument_is_a_diagnostic() {
        let ctx = run(r#"loft([sketch(XY).rect(10, 10), sketch(XY.offset(4)).rect(6, 6)], match="nearest")"#);
        let errs = errors(&ctx);
        assert!(errs.iter().any(|e| e.contains("\"arclen\"")), "{errs:?}");
    }

    /// Sections the first version cannot match are diagnostics that name the
    /// section and say what to do, never panics.
    #[test]
    fn bad_loft_sections_are_diagnostics_with_a_span() {
        for (src, want) in [
            ("loft([sketch(XY).rect(10, 10), sketch(XZ).rect(10, 10)])", "not parallel"),
            ("loft([sketch(XY).rounded_rect(10, 10, 2), sketch(XY.offset(4)).rect(6, 6)])", "segment to segment"),
            ("loft([sketch(XY).rect(10, 10), sketch(XY).rect(6, 6)])", "same height"),
            ("loft([sketch(XY).rect(10, 10)])", "at least two sections"),
            (
                "loft([sketch(XY).rect(10, 10), sketch(XY.offset(9)).rect(4, 4), sketch(XY.offset(3)).rect(2, 2)])",
                "reverses the direction",
            ),
        ] {
            let ctx = run(src);
            let errs = errors(&ctx);
            assert!(errs.iter().any(|e| e.contains(want)), "{src}\nwanted {want:?}, got {errs:?}");
        }
    }

    /// Sections whose loops start at different corners still build in
    /// construction order — that is the contract — but the script is warned.
    #[test]
    fn a_twisted_loft_warns_rather_than_guessing() {
        let ctx = run(r#"
a = sketch(XY).polygon([(-5, -5), (5, -5), (5, 5), (-5, 5)])
b = sketch(XY.offset(6)).polygon([(5, 5), (-5, 5), (-5, -5), (5, -5)])
loft([a, b], name="twisted")
"#);
        assert!(errors(&ctx).is_empty(), "{:?}", errors(&ctx));
        let warnings = of_severity(&ctx, cadpole_core::context::Severity::Warning);
        assert!(
            warnings.iter().any(|(m, _)| m.contains("probably twisted")),
            "{warnings:?}"
        );
    }

    #[test]
    fn a_cone_is_a_solid_of_the_textbook_volume_and_names_its_faces() {
        let mut ctx = run(r#"
c = cone(5, 0, 9, name="tip")
f = cone(3.275, 3.075, 0.2, at=(20, 0, 0), name="mouth")
if f.face("mouth/bottom").name != "mouth/bottom":
    fail("no bottom")
"#);
        assert!(errors(&ctx).is_empty(), "{:?}", errors(&ctx));
        let (v, faces) = solid(&mut ctx, "tip");
        let want = std::f64::consts::PI * 25.0 * 9.0 / 3.0;
        assert!((v.abs() - want).abs() < 0.02 * want, "volume {v}, expected {want}");
        assert_eq!(faces, 3, "two wall faces + the base");
    }

    /// FNV-1a over `bytes`, folded into a running hash. Cheap, dependency-free
    /// and stable across platforms, which is all a fingerprint needs.
    fn fnv(h: u64, bytes: &[u8]) -> u64 {
        bytes.iter().fold(h, |h, b| (h ^ *b as u64).wrapping_mul(0x0000_0100_0000_01b3))
    }

    /// One line per body a `loft` or `cone` feature produced in the example:
    /// its face count, a hash of its face names in mesh order, a hash of every
    /// vertex position and triangle index in mesh order, and its volume.
    /// Bit-identical geometry gives a bit-identical line.
    fn loft_fingerprints(example: &str) -> Vec<String> {
        let dir = std::env::var("CADPOLE_EXAMPLES").unwrap_or_else(|_| "examples".into());
        let src = std::fs::read_to_string(std::path::Path::new(&dir).join(example)).expect(example);
        let mut ctx = run(&src);
        assert!(errors(&ctx).is_empty(), "{example}: {:?}", errors(&ctx));
        let lofts: Vec<(String, Vec<cadpole_core::BodyId>)> = ctx
            .features
            .iter()
            .filter(|f| f.kind == "loft" || f.kind == "cone")
            .map(|f| (f.id.0.clone(), f.outputs.clone()))
            .collect();
        let mut out = Vec::new();
        for (feature, bodies) in lofts {
            for id in bodies {
                let (mesh, topo) = ctx.body_mesh(id).unwrap();
                let mut names = 0xcbf2_9ce4_8422_2325u64;
                for f in &topo.faces {
                    names = fnv(names, f.name.as_bytes());
                    names = fnv(names, b"|");
                }
                let mut geom = 0xcbf2_9ce4_8422_2325u64;
                for p in &mesh.positions {
                    geom = fnv(geom, &p.to_le_bytes());
                }
                for i in &mesh.indices {
                    geom = fnv(geom, &i.to_le_bytes());
                }
                out.push(format!(
                    "{feature}#{} faces={} names={names:016x} geom={geom:016x} volume={:.6}",
                    id.0,
                    topo.faces.len(),
                    mesh.volume()
                ));
            }
        }
        out
    }

    /// Every loft that built before `Curve` sections existed builds the
    /// identical solid with identical names afterwards. The
    /// fingerprints below were recorded on `main` before the 3D-section path
    /// was added: planar sections still take exactly the code they took then,
    /// so a changed line here means the planar path moved, which it must not.
    /// `geom` and `volume` also move with the tessellator (how a twisted or
    /// spline patch is triangulated, the last bits of `cadpole_core::fmath`),
    /// and split pieces carry `#k` suffixes ordered by their tessellated
    /// centroids, so a tessellator change may swap two of them; the faces are
    /// the same faces.
    ///
    /// Set `CADPOLE_LOFT_FINGERPRINTS=print` to print the current lines instead
    /// of comparing them (for re-recording after a *deliberate* change). An
    /// example whose script is redesigned is re-recorded from the new script,
    /// still through planar sections only.
    #[test]
    fn existing_lofts_are_unchanged_by_the_curve_section_path() {
        let recorded: &[(&str, &[&str])] = &[
            (
                "knurled_knob.star",
                &[
                    "knob#0 faces=2486 names=9058ea4dda1cab3c geom=82a9c5ea3c99db9e volume=11085.035817",
                    "cap_disc#1 faces=6 names=5f0082f0ab12606e geom=d04b8604ce2dc5f1 volume=474.826012",
                ],
            ),
            (
                "charging_stand.star",
                &[
                    "disc#0 faces=8 names=ae8fbe9078f0627d geom=98730f9fbd79a8b4 volume=145476.888108",
                    "pad#1 faces=4 names=d0c8d563711f61a5 geom=446c6a5041044935 volume=19314.711345",
                    "arm#2 faces=42 names=8c1d59a0f9918c08 geom=b39eed7aa9d85d25 volume=11980.046695",
                    "watch_stem#3 faces=4 names=a443f1dc4a839401 geom=311c459cd0bdcb7d volume=1436.206117",
                ],
            ),
            (
                "table_lamp.star",
                &[
                    "shade#0 faces=6 names=3d86f9f052b7e202 geom=b514f28630382c41 volume=270239.018892",
                    "shade_rim#1 faces=6 names=8686bffd004438d2 geom=c7d751714c57e565 volume=36782.562032",
                    "base#2 faces=8 names=3616e1c6f1680425 geom=d9c6aa56c71c8738 volume=1967129.588737",
                ],
            ),
            (
                "nightstand.star",
                &[
                    "top#0 faces=14 names=517a555e0c8168d6 geom=673190f0da4812dd volume=4246449.000000",
                    "shelf#8 faces=14 names=d69e9eeb503df2c2 geom=5be4e95f2ff72285 volume=2944393.000000",
                    "drawer_front#9 faces=14 names=968ec5533f3059ce geom=763230be03c29c59 volume=784071.000000",
                ],
            ),
            (
                "gridfinity_baseplate.star",
                &[
                    "slab#0 faces=18 names=c4700cd2f784abe8 geom=a8e0114afc1c10af volume=52283.180361",
                    "cup#1 faces=34 names=3a0b8dc70e2c87b6 geom=d002335c89b184be volume=8378.611441",
                    "foot#4 faces=26 names=14d31ef76be1dce8 geom=5a02c0f247dca5ea volume=7296.130456",
                ],
            ),
            (
                "gridfinity_bin.star",
                &[
                    "socket#1 faces=18 names=46d2ed07e1736326 geom=84d98ecf06eac39d volume=54488.674224",
                    "foot#4 faces=34 names=c68e999c3c3c8608 geom=a265332692240742 volume=8171.102019",
                ],
            ),
            (
                "impeller.star",
                &[
                    "shroud#0 faces=54 names=bee8b9b801100d8e geom=26e3a6b9cd18a362 volume=39581.229405",
                    "vane#1 faces=28 names=b414e5acb82e8059 geom=0ff2522a648061fa volume=5672.861028",
                    "passage#3 faces=30 names=be8f94cade90129a geom=c5a13ca62b151624 volume=203595.115934",
                    "hub_dome#6 faces=22 names=1750390ce4f49cac geom=bfdf4596ef836057 volume=27501.465602",
                    "nut_chamfer#16 faces=6 names=bfc263ec89ec085a geom=bcbe2ce38e96e0c5 volume=3555.464878",
                ],
            ),
        ];
        let print = std::env::var("CADPOLE_LOFT_FINGERPRINTS").is_ok_and(|v| v == "print");
        let mut failures = Vec::new();
        for (example, lines) in recorded {
            let now = loft_fingerprints(example);
            if print {
                println!("--- {example}");
                for l in &now {
                    println!("{l}");
                }
                continue;
            }
            assert!(!now.is_empty(), "{example} has no loft bodies to fingerprint");
            let want: Vec<String> = lines.iter().map(|s| s.to_string()).collect();
            if now != want {
                failures.push(format!("{example}:\n  recorded: {want:#?}\n  now:      {now:#?}"));
            }
        }
        assert!(failures.is_empty(), "{}", failures.join("\n"));
    }

    // --------------------------------------------- curves in space

    /// The line each error was reported on: every refusal below has to point
    /// at the call that caused it, never at nothing.
    fn error_lines(ctx: &Context) -> Vec<(String, u32)> {
        of_severity(ctx, cadpole_core::context::Severity::Error)
    }

    /// A blade-like closed section in space, as a script would draw one:
    /// `n` stations along a chord, a round nose, a square trailing edge, drawn
    /// from the middle of the trailing edge; turned by `twist` degrees,
    /// lifted to `z`, warped out of its plane by `warp`.
    fn blade_script(n: usize, chord: f64, t: f64, z: f64, twist: f64, warp: f64, name: &str) -> String {
        format!(
            r#"
def place_{name}(x, y):
    s = math.sin({twist})
    c = math.cos({twist})
    return (x * c - y * s, x * s + y * c, {z} + {warp} * math.sin(x / {chord} * 180.0))

def h_{name}(x):
    # Quadratic, so the flanks curve: a linear taper is a straight flank, and
    # a seam in the middle of a straight flank has no corner face at all.
    return {t} / 2.0 * (1.0 - 0.4 * (x / {chord}) * (x / {chord}))

xs_{name} = [{chord} * i / ({n} - 1.0) for i in range({n})]
press_{name} = [place_{name}(x, h_{name}(x)) for x in xs_{name}]
suct_{name} = [place_{name}(x, -h_{name}(x)) for x in xs_{name}]
te_{name} = place_{name}({chord}, 0.0)
nose_{name} = place_{name}(-h_{name}(0.0), 0.0)
{name} = (curve(te_{name}, name="{name}")
    .line(press_{name}[-1])
    .spline([press_{name}[i] for i in range({n} - 2, -1, -1)])
    .arc(suct_{name}[0], via=nose_{name})
    .spline(suct_{name}[1:])
    .line(te_{name})
    .close())
"#
        )
    }

    /// `Path.on(XY)` of a 2D path is the curve drawn directly with z = 0:
    /// same segment count, same length, same ends — one curve type in the
    /// language, not two. And `curve()` is a feature of kind `curve`, named
    /// like every other, that records what was drawn on it.
    #[test]
    fn a_path_lifted_onto_xy_is_the_curve_drawn_directly() {
        let ctx = run(r#"
p = path((0, 0)).line((10, 0)).arc((10, 4), via=(12, 2)).bezier((4, 6), controls=[(8, 9), (6, 3)]).spline([(2, 5), (-1, 3)])
lifted = p.on(XY)
direct = (curve((0, 0, 0), name="direct")
          .line((10, 0, 0))
          .arc((10, 4, 0), via=(12, 2, 0))
          .bezier((4, 6, 0), controls=[(8, 9, 0), (6, 3, 0)])
          .spline([(2, 5, 0), (-1, 3, 0)]))
if lifted.segments != direct.segments:
    fail("segments %s vs %s" % (lifted.segments, direct.segments))
if lifted.length != direct.length:
    fail("length %s vs %s" % (lifted.length, direct.length))
if lifted.end != direct.end or lifted.start != direct.start:
    fail("ends differ")
if lifted.closed or direct.closed:
    fail("an open path lifts to an open curve")
if direct.id != "direct":
    fail("the name is the feature id: " + direct.id)
if not lifted.id.startswith("curve_"):
    fail("Path.on records a curve feature: " + lifted.id)
c = direct.close()
if not c.closed or c.segments != direct.segments + 1:
    fail("close() adds the one straight segment back to the start")
if c.plane == None:
    fail("a curve drawn in z = 0 is planar")
if abs(c.plane.normal[2]) < 0.999:
    fail("...with the normal along z: %s" % (c.plane.normal,))
"#);
        assert!(errors(&ctx).is_empty(), "{:?}", errors(&ctx));
        let f = ctx.features.iter().find(|f| f.id.0 == "direct").expect("the curve feature");
        assert_eq!(f.kind, "curve");
        // Five drawn, and `close()` recorded the sixth on the same feature.
        assert_eq!(f.args.get("segments").map(String::as_str), Some("6"));
        assert_eq!(f.args.get("closed").map(String::as_str), Some("true"));
        assert_eq!(f.args.get("kinds").map(String::as_str), Some("line, arc, bezier(2) x3, line"));
    }

    /// `Sketch.curves()` is one closed curve per loop, as drawn, carrying the
    /// sketch's id; a 3D spline is one cubic per span through its points.
    #[test]
    fn sketch_curves_and_3d_splines_are_what_they_say() {
        let ctx = run(r#"
s = sketch(XZ, name="profile").rect(10, 6).circle(2)
cs = s.curves()
if len(cs) != 2:
    fail("two loops, two curves: %s" % len(cs))
for c in cs:
    if not c.closed:
        fail("a loop is a closed curve")
    if c.id != "profile":
        fail("a sketch's curves carry the sketch's id: " + c.id)
    if c.plane == None:
        fail("a sketch loop is planar")
if cs[0].segments != 4 or cs[1].segments != 2:
    fail("rect is 4 lines, circle 2 arcs: %s %s" % (cs[0].segments, cs[1].segments))
if abs(cs[0].plane.normal[1]) < 0.999:
    fail("on XZ the loop's plane normal is along y: %s" % (cs[0].plane.normal,))
sp = curve((0, 0, 0)).spline([(3, 1, 2), (6, 4, 0), (9, 2, 5)])
if sp.segments != 3:
    fail("one cubic per span: %s" % sp.segments)
if sp.end != (9, 2, 5):
    fail("the spline ends on its last point: %s" % (sp.end,))
if sp.plane != None:
    fail("those points are not coplanar")
"#);
        assert!(errors(&ctx).is_empty(), "{:?}", errors(&ctx));
    }

    /// The acceptance shape: two closed splines in space, neither planar,
    /// lofted into one solid whose ends are closed by the ruled cap. Solid,
    /// the face count the construction predicts, the cap's pieces named
    /// `end#k` under the alias `end`, and no warning — the seam is at the
    /// trailing edge where it belongs.
    #[test]
    fn a_loft_between_two_closed_3d_splines_is_a_solid() {
        let src = format!(
            "{}{}\nblade = loft([root, tip], ruled=True, cap=\"ruled\", name=\"blade\")\nassert_solid(blade)\n\
             if root.plane != None or tip.plane != None:\n    fail(\"the sections must be genuinely 3D\")\n\
             tip_faces = blade.faces(name=\"end\")\n\
             if len(tip_faces) < 2:\n    fail(\"the ruled cap is several faces under one alias: %s\" % len(tip_faces))\n\
             if blade.face(\"end#0\").name != \"blade/end#0\":\n    fail(\"the first piece of the cap is end#0\")\n",
            blade_script(6, 30.0, 2.0, 0.0, 0.0, 0.8, "root"),
            blade_script(6, 26.0, 1.4, 20.0, 25.0, -0.6, "tip"),
        );
        let mut ctx = run(&src);
        assert!(errors(&ctx).is_empty(), "{:?}", errors(&ctx));
        let warnings = of_severity(&ctx, cadpole_core::context::Severity::Warning);
        assert!(warnings.is_empty(), "{warnings:?}");
        let (v, faces) = solid(&mut ctx, "blade");
        assert!(v > 0.0, "volume {v}");
        // 2n + 1 segments drawn, so 2n + 1 bands (the cap's half-way cut is
        // a vertex of the capped wire only); each ribbon n - 1 ruled pieces
        // and one flat nose piece.
        assert_eq!(faces, (2 * 6 + 1) + 2 * 6);
    }

    /// Two planar but non-parallel sketches — refused as sketches —
    /// loft once they are passed as curves, and the sketch form still says
    /// so, naming the way through.
    #[test]
    fn two_planar_non_parallel_sections_loft_as_curves() {
        let ctx = run(r#"
a = sketch(XY).rect(10, 10)
b = sketch(plane(origin=(0, 0, 12), normal=(0, 0.5, 1))).rect(10, 10)
loft([a, b], name="refused")
"#);
        let errs = errors(&ctx);
        assert!(errs.iter().any(|e| e.contains("not parallel") && e.contains("curves()")), "{errs:?}");
        let mut ctx = run(r#"
a = sketch(XY).rect(10, 10)
b = sketch(plane(origin=(0, 0, 12), normal=(0, 0.5, 1))).rect(10, 10)
wedge = loft([a.curves()[0], b.curves()[0]], name="wedge")
assert_solid(wedge)
if wedge.face("start").name != "wedge/start" or wedge.face("end").name != "wedge/end":
    fail("planar ends are still start and end")
"#);
        assert!(errors(&ctx).is_empty(), "{:?}", errors(&ctx));
        let (v, faces) = solid(&mut ctx, "wedge");
        assert!(v > 100.0 * 10.0 && v < 100.0 * 14.0, "volume {v}");
        assert_eq!(faces, 6);
    }

    /// `cap="plane"` on an end that lies in no plane is a diagnostic with a
    /// span, naming the section and pointing at `cap="ruled"`; an unknown
    /// spelling lists the two that exist; a 2-tuple closes the ends
    /// differently.
    #[test]
    fn cap_plane_on_a_non_planar_end_is_a_diagnostic_and_a_tuple_closes_each_end() {
        let base = blade_script(6, 30.0, 2.0, 0.0, 0.0, 0.0, "flat");
        let warped = blade_script(6, 26.0, 1.4, 20.0, 25.0, -0.6, "warped");
        let ctx = run(&format!("{base}{warped}\nloft([flat, warped], name=\"b\")\n"));
        let errs = error_lines(&ctx);
        assert_eq!(errs.len(), 1, "{errs:?}");
        assert!(errs[0].0.contains("section 1 (the end) does not lie in a plane"), "{}", errs[0].0);
        assert!(errs[0].0.contains("cap=\"ruled\""), "{}", errs[0].0);
        assert!(errs[0].1 > 0, "the diagnostic has a span");

        let ctx = run(&format!("{base}{warped}\nloft([flat, warped], cap=\"flat\", name=\"b\")\n"));
        let errs = errors(&ctx);
        assert!(errs.iter().any(|e| e.contains("\"plane\"") && e.contains("\"ruled\"")), "{errs:?}");

        // A planar start closed by a plane, a warped end by the ribbon.
        let mut ctx = run(&format!(
            "{base}{warped}\nb = loft([flat, warped], cap=(\"plane\", \"ruled\"), name=\"b\")\nassert_solid(b)\n\
             if b.face(\"start\").name != \"b/start\":\n    fail(\"a planar start is start\")\n"
        ));
        assert!(errors(&ctx).is_empty(), "{:?}", errors(&ctx));
        let (v, faces) = solid(&mut ctx, "b");
        assert!(v > 0.0);
        assert_eq!(faces, (2 * 6 + 1) + 1 + 6, "bands + one planar start + a six-piece ruled end");
    }

    /// The ruled cap's seam warning: the same section drawn from the middle
    /// of a flank instead of the trailing edge rules the loop against itself
    /// lengthwise, and the plan says so — naming the section and where the
    /// loop should have started — while still building what was asked for.
    /// (Planar sections, so that a seam between two spline pieces still has
    /// a flat corner to close; on a warped section the same seam is refused
    /// before any warning, because the corner has no exact surface.)
    #[test]
    fn a_ruled_cap_warns_when_the_seam_is_in_the_middle_of_a_flank() {
        let src = format!(
            "{}{}\nloft([root, tip], cap=\"ruled\", name=\"ok\")\n\
             # The tip again, started a third of the way along the pressure flank.\n\
             k = 2\n\
             bad = (curve(press_tip[k], name=\"bad\")\n\
             \x20   .spline([press_tip[i] for i in range(k - 1, -1, -1)])\n\
             \x20   .arc(suct_tip[0], via=nose_tip)\n\
             \x20   .spline(suct_tip[1:])\n\
             \x20   .line(te_tip)\n\
             \x20   .line(press_tip[-1])\n\
             \x20   .spline([press_tip[i] for i in range(4, k - 1, -1)])\n\
             \x20   .close())\n\
             badroot = (curve(press_root[k], name=\"badroot\")\n\
             \x20   .spline([press_root[i] for i in range(k - 1, -1, -1)])\n\
             \x20   .arc(suct_root[0], via=nose_root)\n\
             \x20   .spline(suct_root[1:])\n\
             \x20   .line(te_root)\n\
             \x20   .line(press_root[-1])\n\
             \x20   .spline([press_root[i] for i in range(4, k - 1, -1)])\n\
             \x20   .close())\n\
             loft([badroot, bad], cap=(\"plane\", \"ruled\"), name=\"skewed\")\n",
            blade_script(6, 30.0, 2.0, 0.0, 0.0, 0.0, "root"),
            blade_script(6, 26.0, 1.4, 20.0, 25.0, 0.0, "tip"),
        );
        let ctx = run(&src);
        assert!(errors(&ctx).is_empty(), "{:?}", errors(&ctx));
        let warnings = of_severity(&ctx, cadpole_core::context::Severity::Warning);
        let seam: Vec<_> = warnings.iter().filter(|(m, _)| m.contains("seam is in the wrong place")).collect();
        assert_eq!(seam.len(), 1, "{warnings:?}");
        assert!(seam[0].0.contains("section 1"), "{}", seam[0].0);
        assert!(seam[0].0.contains("x wider"), "{}", seam[0].0);
    }

    /// Reversing the list, or drawing both sections the other way round,
    /// gives the same outward solid: "out" is read from the volume, not
    /// guessed from an up direction. The face counts agree exactly; the
    /// volumes only roughly, because the same surfaces parametrised the
    /// other way round are chorded differently at the display tolerance,
    /// which on a 2 mm blade is a few percent of its thickness — the kernel
    /// test compares them at a fine tolerance and holds them to half a
    /// percent.
    #[test]
    fn reversing_the_list_or_the_curves_gives_the_same_outward_solid() {
        let src = format!(
            "{}{}\nfwd = loft([root, tip], cap=\"ruled\", name=\"fwd\")\n\
             bwd = loft([tip, root], cap=\"ruled\", name=\"bwd\")\n\
             rev = loft([root.reverse(), tip.reverse()], cap=\"ruled\", name=\"rev\")\n\
             for b in [fwd, bwd, rev]:\n    assert_solid(b)\n\
             v = fwd.volume()\n\
             if v <= 0:\n    fail(\"volume %s\" % v)\n\
             for b in [bwd, rev]:\n\
             \x20   if abs(b.volume() - v) > 0.1 * v:\n        fail(\"volumes differ: %s vs %s\" % (b.volume(), v))\n\
             \x20   if len(b.faces()) != len(fwd.faces()):\n        fail(\"face counts differ: %s vs %s\" % (len(b.faces()), len(fwd.faces())))\n",
            blade_script(6, 30.0, 2.0, 0.0, 0.0, 0.8, "root"),
            blade_script(6, 26.0, 1.4, 20.0, 25.0, -0.6, "tip"),
        );
        let ctx = run(&src);
        assert!(errors(&ctx).is_empty(), "{:?}", errors(&ctx));
        assert!(of_severity(&ctx, cadpole_core::context::Severity::Warning).is_empty());
    }

    /// The twist warning in space: a section drawn the opposite way round
    /// from its neighbour is called out (with `.reverse()` as the fix), and
    /// one started from a different corner is called out as before. Both
    /// still build in construction order.
    #[test]
    fn a_twisted_3d_loft_warns_rather_than_guessing() {
        let ctx = run(r#"
a = curve((-5, -5, 0)).line((5, -5, 1)).line((5, 5, 0)).line((-5, 5, 1)).close()
b = curve((-5, -5, 8)).line((-5, 5, 9)).line((5, 5, 8)).line((5, -5, 9)).close()
loft([a, b], cap="ruled", name="backwards")
c = curve((5, 5, 16)).line((-5, 5, 17)).line((-5, -5, 16)).line((5, -5, 17)).close()
loft([a, c], cap="ruled", name="rotated")
"#);
        assert!(errors(&ctx).is_empty(), "{:?}", errors(&ctx));
        let warnings = of_severity(&ctx, cadpole_core::context::Severity::Warning);
        assert!(warnings.iter().any(|(m, _)| m.contains("opposite way round") && m.contains(".reverse()")), "{warnings:?}");
        assert!(warnings.iter().any(|(m, _)| m.contains("rotated by 2 segment(s)")), "{warnings:?}");
    }

    /// Point sections still work next to a curve, sketches and curves mix in
    /// one list, and the count mismatch is the diagnostic it always was.
    #[test]
    fn curve_sections_with_points_sketches_and_mismatches() {
        let mut ctx = run(r#"
base = curve((-5, -5, 0)).line((5, -5, 1)).line((5, 5, 0)).line((-5, 5, 1)).close()
pyr = loft([base, (0, 0, 12)], cap="ruled", name="pyr")
assert_solid(pyr)
sq = sketch(XY).rounded_rect(20, 12, 3)
top = sketch(XY.offset(15)).rounded_rect(14, 8, 2).curves()[0]
mixed = loft([sq, top], name="mixed")
assert_solid(mixed)
"#);
        assert!(errors(&ctx).is_empty(), "{:?}", errors(&ctx));
        assert_eq!(solid(&mut ctx, "pyr").1, 6, "four triangles + two closing the twisted base");
        assert_eq!(solid(&mut ctx, "mixed").1, 10);

        let ctx = run(r#"
a = curve((0, 0, 0)).line((10, 0, 0)).line((10, 10, 0)).line((0, 10, 0)).close()
b = curve((0, 0, 8)).line((10, 0, 8)).line((10, 10, 9)).line((5, 12, 9)).line((0, 10, 8)).close()
loft([a, b], name="mismatched")
"#);
        let errs = error_lines(&ctx);
        assert_eq!(errs.len(), 1, "{errs:?}");
        assert!(errs[0].0.contains("section 1's loop 0 has 5 segment(s)") && errs[0].0.contains("match=\"arclen\""), "{}", errs[0].0);
        assert_eq!(errs[0].1, 4, "the diagnostic points at the loft call");
    }

    /// The refusals a script can run into with curves, each a diagnostic on
    /// the line that caused it: an open section, drawing after `close()`, a
    /// collinear arc, a sketch with a hole next to a curve, and a ruled cap
    /// whose corner has no flat surface.
    #[test]
    fn curve_refusals_are_diagnostics_with_spans() {
        for (src, want) in [
            ("open = curve((0, 0, 0)).line((10, 0, 0)).line((10, 10, 3))\nloft([open, open], name=\"x\")", "open curve"),
            ("c = curve((0, 0, 0)).line((10, 0, 0)).line((10, 10, 3)).close()\nc.line((20, 20, 20))", "closed"),
            ("curve((0, 0, 0)).arc((10, 0, 0), via=(5, 0, 0))", "no circle"),
            (
                "s = sketch(XY).rect(20, 20).circle(3)\nc = sketch(XY.offset(5)).rect(10, 10).curves()[0]\nloft([s, c], name=\"x\")",
                "sketch with 2 loops",
            ),
            (
                "a = curve((0, 0, 0)).spline([(10, 0, 2), (10, 10, 0), (0, 10, 2)]).close()\n\
                 b = curve((0, 0, 8)).spline([(10, 0, 10), (10, 10, 8), (0, 10, 10)]).close()\n\
                 loft([a, b], cap=\"ruled\", name=\"x\")",
                "do not share a plane",
            ),
        ] {
            let ctx = run(src);
            let errs = error_lines(&ctx);
            assert!(errs.iter().any(|(m, _)| m.contains(want)), "{src}\nwanted {want:?}, got {errs:?}");
            assert!(errs.iter().all(|(_, line)| *line > 0), "{src}: a diagnostic without a span: {errs:?}");
        }
    }

    // ---------------------------------------------------------- pattern merge

    #[test]
    fn a_merging_pattern_is_one_body_and_a_plain_one_is_a_list() {
        let mut ctx = run(r#"
tile = box(10, 10, 4, name="tile")
row = pattern_linear(tile, 3, (30, 0, 0), merge=True, name="row")
if type(row) != "Body":
    fail("merge=True should give one Body, got " + type(row))
"#);
        assert!(errors(&ctx).is_empty(), "{:?}", errors(&ctx));
        let (v, _) = solid(&mut ctx, "row");
        assert!((v.abs() - 3.0 * 400.0).abs() < 1e-3, "volume {v}");

        let ctx = run(r#"
tile = box(10, 10, 4, name="tile")
row = pattern_linear(tile, 3, (30, 0, 0), name="row")
if type(row) != "list" or len(row) != 3:
    fail("without merge it is still a list of 3")
"#);
        assert!(errors(&ctx).is_empty(), "{:?}", errors(&ctx));
    }

    /// The Gridfinity case: copies that only touch. The exact kernel refuses
    /// them, so the merge says it is going to the mesh kernel and the result is
    /// one watertight body of the right volume.
    #[test]
    fn a_merging_pattern_fuses_copies_that_touch_and_says_which_kernel() {
        let mut ctx = run(r#"
tile = box(10, 10, 4, name="tile")
row = pattern_linear(tile, 4, (10, 0, 0), merge=True, name="row")
"#);
        assert!(errors(&ctx).is_empty(), "{:?}", errors(&ctx));
        let infos = of_severity(&ctx, cadpole_core::context::Severity::Info);
        assert!(infos.iter().any(|(m, _)| m.contains("mesh kernel")), "{infos:?}");
        let id = ctx.bodies.iter().find(|b| b.name == "row").unwrap().id;
        assert_eq!(ctx.body(id).unwrap().kind(), "mesh");
        let (mesh, _) = ctx.body_mesh(id).unwrap();
        let welded = mesh.welded(1e-4);
        assert!(welded.is_watertight());
        assert!((welded.volume().abs() - 1600.0).abs() < 1e-3, "volume {}", welded.volume());
    }

    #[test]
    fn a_circular_pattern_merges_too() {
        let mut ctx = run(r#"
post = cylinder(2, 10, at=(10, 0, 0), name="post")
ring = pattern_circular(post, 4, merge=True, name="ring")
"#);
        assert!(errors(&ctx).is_empty(), "{:?}", errors(&ctx));
        let (v, _) = solid(&mut ctx, "ring");
        let one = std::f64::consts::PI * 4.0 * 10.0;
        // A 2 mm cylinder at 0.05 is a 16-gon under the arc rule (
        // halved until the sagitta is under the tolerance), 2.55 % under
        // its circle; the point here is the merge, not the density.
        assert!((v.abs() - 4.0 * one).abs() < 0.03 * 4.0 * one, "volume {v}");
    }

    /// A script addresses a face by its generated name, the short local name,
    /// an alias or the `face:N` compatibility form.
    #[test]
    fn a_script_can_name_a_face_four_ways() {
        let ctx = run(r#"
base = box(20, 20, 10, name="base")
picks = [base.face("top"), base.face("base/top"), base.face("end"), base.face("face:0")]
for p in picks[:3]:
    if p.name != "base/top":
        fail("expected base/top, got " + p.name)
if "base/end" not in picks[0].aliases:
    fail("the generic name should stay on as an alias: " + str(picks[0].aliases))
if "/" not in picks[3].name:
    fail("face:0 should resolve to a generated name, got " + picks[3].name)
if abs(picks[0].area - 400.0) > 1e-6:
    fail("wrong face: area " + str(picks[0].area))
"#);
        assert!(errors(&ctx).is_empty(), "{:?}", errors(&ctx));
    }

    #[test]
    fn a_face_name_that_stands_for_several_faces_is_an_error() {
        // A cylinder's wall is two kernel faces, so `side` is a group.
        let ctx = run(r#"
post = cylinder(5, 10, name="post")
wall = post.face("side")
"#);
        let e = errors(&ctx);
        assert_eq!(e.len(), 1, "{e:?}");
        assert!(e[0].contains("post/side#0") && e[0].contains("post/side#1"), "{}", e[0]);

        // And the error for a name that matches nothing lists what there is.
        let ctx = run(r#"
b = box(10, 10, 10, name="b")
f = b.face("nope")
"#);
        let e = errors(&ctx);
        assert_eq!(e.len(), 1, "{e:?}");
        assert!(e[0].contains("b/top"), "{}", e[0]);
    }

    /// Generated names, end to end through the script layer: the name of the
    /// top face does not move when an unrelated feature is inserted before it.
    #[test]
    fn names_from_a_script_survive_an_insertion_and_a_parameter_change() {
        let names = |src: &str| {
            let mut ctx = run(src);
            let id = ctx.bodies.iter().find(|b| b.visible && b.name == "blk").map(|b| b.id).expect("blk");
            let mut n: Vec<String> = ctx.body_mesh(id).unwrap().1.faces.iter().map(|f| f.name.clone()).collect();
            n.sort();
            n
        };
        let plain = names("blk = box(10, 10, 10, name=\"blk\")\n");
        assert!(plain.contains(&"blk/top".to_owned()), "{plain:?}");
        assert_eq!(plain, names("pin = cylinder(2, 3, at=(50,0,0), name=\"pin\")\nblk = box(10, 10, 10, name=\"blk\")\n"));
        assert_eq!(plain, names("blk = box(10, 10, 40, name=\"blk\")\n"));
        // A sketch that gains a loop does not renumber the first loop's sides.
        let one = names("blk = extrude(sketch(XY).rect(20, 20), 5, name=\"blk\")\n");
        let two = names("blk = extrude(sketch(XY).rect(20, 20).circle(3), 5, name=\"blk\")\n");
        assert!(one.iter().all(|n| two.contains(n)), "{one:?} is not a subset of {two:?}");
    }

    #[test]
    fn a_drawing_of_a_part_is_dimensioned_and_exportable() {
        let out = run_full(
            r#"
b = box(60, 30, 6, center=True, name="plate")
d = drawing(b, view="three-view", sheet="A4", title="Plate", name="sheet")
export_svg(d, "plate.svg")
export_dxf(d, "plate.dxf")
export_pdf(d, "plate.pdf")
assert_eq(d.sheet, "A4")
assert_eq(d.scale, "2.5:1")
assert_true("M-VIEW" in d.layers, msg="the view layer exists")
"#,
        );
        assert!(errors(&out.ctx).is_empty(), "{:?}", errors(&out.ctx));
        let paths: Vec<&str> = out.files.iter().map(|f| f.path.as_str()).collect();
        assert_eq!(paths, vec!["plate.svg", "plate.dxf", "plate.pdf"]);

        let svg = String::from_utf8(out.files[0].data.clone()).unwrap();
        assert!(svg.contains("width=\"297mm\""), "A4 landscape");
        // The figured sizes are the model's, not the paper's.
        for want in [">60</text>", ">30</text>", ">6</text>"] {
            assert!(svg.contains(want), "{want} missing");
        }
        assert!(String::from_utf8_lossy(&out.files[1].data).contains("LWPOLYLINE"));
        assert!(out.files[2].data.starts_with(b"%PDF-"));
    }

    #[test]
    fn a_drawing_in_feet_and_inches_says_so() {
        let out = run_full(
            r#"
b = box(3810, 2438.4, 100, center=True, name="slab")
d = drawing(b, view="top", units="ft-in", sheet="A3", name="plan")
export_svg(d, "plan.svg")
"#,
        );
        assert!(errors(&out.ctx).is_empty(), "{:?}", errors(&out.ctx));
        let svg = String::from_utf8(out.files[0].data.clone()).unwrap();
        assert!(svg.contains("12&apos;-6&quot;"), "width in feet and inches");
        assert!(svg.contains("8&apos;-0&quot;"), "depth in feet and inches");
    }

    #[test]
    fn export_svg_takes_a_sketch_and_keeps_its_loop_names() {
        let out = run_full(
            r#"
s = sketch(XY).rect(40, 20).circle(4)
export_svg(s, "plate.svg")
box(1, 1, 1, name="anything")
"#,
        );
        assert!(errors(&out.ctx).is_empty(), "{:?}", errors(&out.ctx));
        let svg = String::from_utf8(out.files[0].data.clone()).unwrap();
        assert!(svg.contains("viewBox"));
        assert_eq!(svg.matches(" A ").count(), 2, "the circle stayed two arcs");
    }

    #[test]
    fn the_drawing_builtins_refuse_what_they_cannot_draw() {
        let out = run_full("drawing(None, view=\"sideways\")");
        assert!(errors(&out.ctx).iter().any(|e| e.contains("three-view")), "{:?}", errors(&out.ctx));
        let out = run_full("box(1, 1, 1, name=\"b\")\nexport_svg(\"not geometry\", \"x.svg\")");
        assert!(errors(&out.ctx).iter().any(|e| e.contains("expected a Drawing")), "{:?}", errors(&out.ctx));
        let out = run_full("b = box(1, 1, 1, name=\"b\")\ndrawing(b, units=\"furlong\")");
        assert!(errors(&out.ctx).iter().any(|e| e.contains("ft-in")), "{:?}", errors(&out.ctx));
    }

    #[test]
    fn box_with_hole() {
        let ctx = run(r#"
w = param("w", 40, unit="mm", min=10, max=100)
base = extrude(sketch(XY).rect(w, 20), 10, name="base")
hole = extrude(sketch(XY.offset(-1)).circle(3), 12, name="hole")
part = subtract(base, hole, name="part")
"#);
        assert!(errors(&ctx).is_empty(), "{:?}", errors(&ctx));
        assert_eq!(ctx.params.len(), 1);
        let kinds: Vec<_> = ctx.features.iter().map(|f| (f.id.0.as_str(), f.kind.as_str())).collect();
        assert_eq!(
            kinds,
            vec![
                ("sketch_1", "sketch"),
                ("base", "extrude"),
                ("sketch_2", "sketch"),
                ("hole", "extrude"),
                ("part", "subtract")
            ]
        );
        let visible: Vec<_> = ctx.bodies.iter().filter(|b| b.visible).collect();
        assert_eq!(visible.len(), 1);
        assert_eq!(visible[0].name, "part");
        // Spans point into main.star.
        let f = ctx.feature(&"base".into()).unwrap();
        assert_eq!(f.span.as_ref().unwrap().path, "main.star");
        assert_eq!(f.span.as_ref().unwrap().start_line, 3);
    }

    #[test]
    fn overrides_apply_and_queries_work() {
        let mut overrides = HashMap::new();
        overrides.insert("h".to_owned(), ParamValue::Number(7.0));
        let ctx = evaluate(EvalInput {
            scripts: vec![ScriptFile {
                path: "main.star".into(),
                source: r#"
h = param("h", 5)
b = box(10, 10, h, name="b")
top = b.faces(normal=(0, 0, 1))
assert_eq(len(top), 1)
assert_eq(top[0].centroid[2], h)
assert_eq(len(b.faces()), 6)
assert_eq(len(b.edges()), 12)
"#
                .into(),
            }],
            overrides,
            ..Default::default()
        });
        assert!(errors(&ctx).is_empty(), "{:?}", errors(&ctx));
        assert_eq!(ctx.params[0].1, ParamValue::Number(7.0));
    }

    /// The query algebra: filters, set operations and
    /// the reducers, on a body whose every face has a generated name.
    #[test]
    fn selectors_filter_by_geometry_and_compose_as_sets() {
        let ctx = run(r#"
b = box(20, 20, 10, name="b")
top = b.faces(normal=(0, 0, 1))
assert_eq(top.count(), 1)
assert_eq(top.one().name, "b/top")

# Whole faces on a plane, and whole faces to one side of it.
assert_eq(b.faces(on_plane=XY.offset(10)).names(), ["b/top"])
assert_eq(b.faces(above=XY.offset(10)).names(), ["b/top"])
assert_eq(b.faces(below=XY).names(), ["b/bottom"])

# A glob over the generated name, matching the local part too.
assert_eq(b.faces(name="side[*]").count(), 4)

# inside= takes bbox()'s own shape, so a body can be used as the box.
assert_eq(b.faces(inside=b.bbox()).count(), 6)
assert_eq(b.faces(inside=((0, 0, 9), (20, 20, 11))).names(), ["b/top"])

# Set algebra over two selections of the same body.
caps = b.faces(normal=(0, 0, 1)) | b.faces(normal=(0, 0, -1))
assert_eq(caps.names(), ["b/bottom", "b/top"])
walls = b.faces() - caps
assert_eq(walls.count(), 4)
assert_eq((caps & walls).count(), 0)
assert_eq((caps | walls).count(), 6)

# Edges: a tangent-free box gives four straight edges along each axis, and
# the four around the top cap lie on one plane.
assert_eq(b.edges(direction=(0, 0, 1)).count(), 4)
assert_eq(b.edges(on_plane=XY.offset(10)).count(), 4)
assert_eq(b.edges(of_face="b/top").count(), 4)
assert_eq(b.edges(min_length=19.9).count(), 8)   # 8 of 20 mm, 4 verticals of 10
assert_eq(b.edges(convex=True).count(), 12)

# Reducers pick exactly one, and never silently.
assert_eq(b.faces().max("z").name, "b/top")
assert_eq(b.faces().min("z").name, "b/bottom")
assert_eq(b.faces().nearest((0, 0, 99)).name, "b/top")
assert_eq(b.faces(name="side[*]").largest().area, 200.0)
assert_true("b/top" in b.faces())
"#);
        assert!(errors(&ctx).is_empty(), "{:?}", errors(&ctx));
    }

    /// An empty selection is legal and assertable, but it warns with the
    /// filter that rejected everything and the nearest miss, because
    /// a selector that quietly returns nothing is the trap.
    #[test]
    fn an_empty_selection_warns_with_the_reason_unless_it_was_asked_for() {
        let ctx = run(r#"
b = box(20, 20, 10, name="b")
none = b.faces(normal=(1, 1, 1))
assert_eq(none.count(), 0)
"#);
        assert!(errors(&ctx).is_empty(), "{:?}", errors(&ctx));
        let warns: Vec<&str> = ctx
            .diagnostics
            .iter()
            .filter(|d| d.severity == cadpole_core::context::Severity::Warning)
            .map(|d| d.message.as_str())
            .collect();
        assert!(warns.iter().any(|m| m.contains("matched nothing") && m.contains("direction")), "{warns:?}");
        assert!(warns.iter().any(|m| m.contains("the closest was")), "{warns:?}");

        // allow_empty=True is the script saying "nothing is the answer".
        let ctx = run("b = box(20, 20, 10, name=\"b\")\nn = b.faces(normal=(1, 1, 1), allow_empty=True)\n");
        assert!(
            !ctx.diagnostics.iter().any(|d| d.severity == cadpole_core::context::Severity::Warning),
            "{:?}",
            ctx.diagnostics
        );
    }

    /// `within=` is degrees and `tolerance=` is millimetres. An angular
    /// `tolerance=` is an error that names the fix, never a
    /// number quietly reinterpreted as a length.
    #[test]
    fn an_angular_tolerance_must_be_spelled_within() {
        let ctx = run("b = box(10, 10, 10, name=\"b\")\nt = b.faces(normal=(0, 0, 1), tolerance=5)\n");
        assert!(errors(&ctx)[0].contains("within="), "{:?}", errors(&ctx));
        // With a length filter present, `tolerance=` is unambiguous and fine.
        let ctx =
            run("b = box(10, 10, 10, name=\"b\")\nt = b.faces(normal=(0, 0, 1), on_plane=XY.offset(10), tolerance=0.01)\n");
        assert!(errors(&ctx).is_empty(), "{:?}", errors(&ctx));
    }

    /// `Face.kind` reports the analytic surface a face was recognised as:
    /// a box's faces are planes, an extruded circle's wall is a
    /// cylinder, a two-circle loft's band a cone, a revolved circle a torus,
    /// and the flat sides of a loft — NURBS patches to truck — are planes.
    /// `faces(kind=)` selects by that class, and the truck-level string is
    /// still an alias that selects what it always did.
    #[test]
    fn a_face_reports_the_analytic_surface_it_lies_on() {
        let ctx = run(r#"
b = box(20, 20, 10, name="b")
assert_eq(b.faces(kind="plane").count(), 6)
for f in b.faces():
    assert_eq(f.kind, "plane")

rod = extrude(sketch(XY).circle(5), 12, name="rod")
walls = rod.faces(kind="cylinder")
assert_eq(walls.count(), rod.faces().count() - 2)
assert_eq(rod.faces(kind="nurbs").count(), walls.count())
for f in walls:
    assert_eq(f.kind, "cylinder")
for f in rod.faces(kind="plane"):
    assert_eq(f.kind, "plane")

c = cone(10, 4, 6, name="c")
assert_true(c.faces(kind="cone").count() >= 1)
assert_eq(c.faces(kind="plane").count(), 2)
assert_eq(c.faces(kind="cylinder", allow_empty=True).count(), 0)

ring = revolve(sketch(XZ).circle(2, center=(10, 0)), axis=(0, 0, 1), name="ring")
assert_true(ring.faces(kind="torus").count() >= 1)
assert_eq(ring.faces(kind="torus").count(), ring.faces().count())

prism = loft([sketch(XY).rect(12, 12), sketch(XY.offset(7)).rect(12, 12)], name="prism")
assert_eq(prism.faces(kind="plane").count(), 6)
assert_eq(prism.faces(kind="nurbs").count(), 4)   # the four sides are NURBS patches to truck, planes to us
"#);
        assert!(errors(&ctx).is_empty(), "{:?}", errors(&ctx));
    }

    /// An unknown face class is still an error listing the valid ones,
    /// and `cylinder` is no longer among the unknown.
    #[test]
    fn an_unknown_face_class_lists_the_valid_ones() {
        let ctx = run("b = box(10, 10, 10, name=\"b\")\nf = b.faces(kind=\"banana\")\n");
        assert!(errors(&ctx)[0].contains("valid") && errors(&ctx)[0].contains("cylinder"), "{:?}", errors(&ctx));
    }

    /// The order a selection comes back in is geometric, so it is the same
    /// before and after a parameter change that only moves things.
    #[test]
    fn a_parameter_change_does_not_reorder_a_selection() {
        let src = r#"
h = param("h", 10)
b = box(20, 20, h, name="b")
print(",".join(b.faces().names()))
print(",".join(b.edges().names()))
"#;
        let mut orders = Vec::new();
        for h in [10.0, 10.5, 40.0] {
            let ctx = run_with(src, "h", h);
            assert!(errors(&ctx).is_empty(), "{:?}", errors(&ctx));
            // `print()` lands in the diagnostics as info lines.
            orders.push(
                ctx.diagnostics
                    .iter()
                    .filter(|d| d.severity == cadpole_core::context::Severity::Info)
                    .map(|d| d.message.clone())
                    .collect::<Vec<_>>(),
            );
        }
        assert_eq!(orders[0], orders[1], "a small change must not reorder");
        assert_eq!(orders[0], orders[2], "nor a large one, while nothing crosses");
    }

    /// `sketch(on=<face>)`: the face's normal, its centroid as the origin, and
    /// the canonical frame as the x direction, so a parameter change moves the
    /// sketch without spinning it.
    #[test]
    fn a_sketch_on_a_face_takes_its_plane_from_the_face() {
        let ctx = run(r#"
b = box(20, 30, 10, at=(5, 7, 0), name="b")
s = sketch(on=b.face("b/top"), name="lid")

def near(p, q):
    return abs(p[0] - q[0]) < 1e-6 and abs(p[1] - q[1]) < 1e-6 and abs(p[2] - q[2]) < 1e-6

# The origin is the face centroid, so a `rounded_rect` with no center= lands
# on the middle of the face the script asked for.
assert_true(near(s.plane.origin, (15.0, 22.0, 10.0)), msg=str(s.plane.origin))
assert_true(near(s.plane.normal, (0.0, 0.0, 1.0)))
# The x direction comes from the normal alone, so it is XY's.
assert_true(near(s.plane.x_dir, XY.x_dir))
# A sketch on a face extrudes away from the body by default, and into it
# with a negative distance.
boss = extrude(sketch(on=b.face("b/top")).circle(3), 5, name="boss")
assert_true(boss.bbox()[1][2] > 14.9)
# One-face selections work too, so a query can feed a sketch directly.
s2 = sketch(on=b.faces(normal=(0, 0, 1)))
assert_true(near(s2.plane.origin, (15.0, 22.0, 10.0)))
"#);
        assert!(errors(&ctx).is_empty(), "{:?}", errors(&ctx));
        let f = ctx.features.iter().find(|f| f.id.0 == "lid").unwrap();
        assert_eq!(f.args.get("on").map(String::as_str), Some("b/top"));
    }

    /// A non-planar face is refused with a diagnostic naming the surface kind,
    /// and an ambiguous selection is refused with the names it found.
    #[test]
    fn a_sketch_refuses_a_face_that_is_not_a_plane() {
        let ctx = run("c = cylinder(5, 10, name=\"c\")\ns = sketch(on=c.face(\"c/side#0\"))\n");
        let e = errors(&ctx);
        assert!(e[0].contains("not a plane"), "{e:?}");
        // The wall is named by what it was recognised as, not by
        // the NURBS patch truck holds it in.
        assert!(e[0].contains("cylinder"), "{e:?}");
        let ctx = run("b = box(10, 10, 10, name=\"b\")\ns = sketch(on=b.faces(name=\"side[*]\"))\n");
        assert!(errors(&ctx)[0].contains("matched 4 faces"), "{:?}", errors(&ctx));
        let ctx = run("b = box(10, 10, 10, name=\"b\")\ns = sketch(XY, on=b.face(\"b/top\"))\n");
        assert!(errors(&ctx)[0].contains("not both"), "{:?}", errors(&ctx));
    }

    /// The worked path the faithful Gridfinity script takes: select the floor
    /// edges of a rounded-rect pocket by the plane they lie on, and round the
    /// whole tangent chain in one call.
    #[test]
    fn the_floor_edges_of_a_rounded_pocket_select_as_one_chain() {
        let ctx = run(r#"
cut = extrude(sketch(XY.offset(1)).rounded_rect(30, 40, 3), 20, name="cut")
floor = cut.edges(on_plane=XY.offset(1))
assert_eq(floor.count(), 8)                 # 4 straight runs + 4 corner arcs
assert_eq(floor.faces().count(), 9)         # the cap plus the 8 walls it meets
assert_eq(cut.edges(on_plane=XY.offset(1), direction=(1, 0, 0)).count(), 2)
assert_eq(cut.edges(on_plane=XY.offset(1), kind="arc").count(), 4)
rounded = fillet(floor, 2.5, name="rounded")
assert_eq(rounded.kind, "brep")             # exact: one tangent chain, re-wired
"#);
        assert!(errors(&ctx).is_empty(), "{:?}", errors(&ctx));
        let id = ctx.bodies.iter().find(|b| b.name == "rounded").unwrap().id;
        let mut ctx = ctx;
        let (_, topo) = ctx.body_mesh(id).unwrap();
        let blends: Vec<&cadpole_core::tessellate::FaceInfo> = topo.faces.iter().filter(|f| f.name.starts_with("rounded/blend[")).collect();
        assert_eq!(blends.len(), 8, "{:?}", topo.faces.iter().map(|f| &f.name).collect::<Vec<_>>());
        assert_eq!(blends.iter().filter(|f| matches!(f.surface, cadpole_core::SurfaceClass::Cylinder { .. })).count(), 4);
        assert_eq!(blends.iter().filter(|f| matches!(f.surface, cadpole_core::SurfaceClass::Torus { .. })).count(), 4);
    }

    #[test]
    fn script_errors_become_diagnostics_with_spans() {
        let ctx = run("x = 1\ny = extrude(3, 4)\n");
        let errs = errors(&ctx);
        assert_eq!(errs.len(), 1, "{errs:?}");
        let d = &ctx.diagnostics[0];
        assert_eq!(d.span.as_ref().map(|s| s.start_line), Some(2));
    }

    #[test]
    fn load_shares_the_feature_tree() {
        let ctx = evaluate(EvalInput {
            scripts: vec![
                ScriptFile {
                    path: "main.star".into(),
                    source: "load(\"lib.star\", \"peg\")\np = peg(4)\nq = translate(p, (10, 0, 0))\n".into(),
                },
                ScriptFile {
                    path: "lib.star".into(),
                    source: "def peg(r):\n    return extrude(sketch(XY).circle(r), 8)\n".into(),
                },
            ],
            ..Default::default()
        });
        assert!(errors(&ctx).is_empty(), "{:?}", errors(&ctx));
        assert_eq!(ctx.features.len(), 3);
        assert_eq!(ctx.features[2].kind, "translate");
    }

    #[test]
    fn paths_slots_and_rounded_rects_extrude() {
        let ctx = run(r#"
# An L-bracket profile with a filleted inner corner, via a tangent arc.
p = path((0, 0)).h(30).v(5).h(8).tangent_arc((5, 8)).v(30).h(0)
s = sketch(XZ).add(p)
b = extrude(s, 10, name="bracket")
s2 = sketch(XY).rounded_rect(20, 10, 2).slot(12, 3)
plate = extrude(s2, 3, name="plate")
assert_eq(len(plate.faces()), 14)  # 8 sides + 4 slot walls + top + bottom
blob = extrude(sketch(XY).spline([(10, 0), (3, 9), (-8, 4), (-6, -7), (4, -8)]), 4, name="blob")
assert_eq(len(blob.faces()), 7)
wave = path((0, 0)).spline([(10, 4), (20, -4), (30, 0)]).v(-10).h(0)
w = extrude(sketch(XY).add(wave), 2, name="wave")
"#);
        assert!(errors(&ctx).is_empty(), "{:?}", errors(&ctx));
        assert_eq!(ctx.bodies.iter().filter(|b| b.visible).count(), 4);
    }

    #[test]
    fn check_syntax_reports_parse_errors_with_spans() {
        assert!(check_syntax("main.star", "x = 1\n").is_none());
        let d = check_syntax("main.star", "x = (1\ny = 2\n").unwrap();
        assert!(d.span.is_some());
    }

    #[test]
    fn booleans_accept_lists_of_bodies() {
        let ctx = run(r#"
base = box(30, 30, 10, name="base")
holes = [cylinder(2, 12, at=(x, 15, -1)) for x in [8, 15, 22]]
part = subtract(base, holes, name="part")
assert_eq(len(part.faces()), 6 + 3 * 2)
"#);
        assert!(errors(&ctx).is_empty(), "{:?}", errors(&ctx));
    }

    #[test]
    fn kernel_keyword_chooses_the_geometry_kernel() {
        // "mesh" forces the mesh kernel and the body says so.
        let ctx = run(r#"
a = box(20, 20, 10, name="a")
b = box(10, 10, 6, at=(0, 0, 10), name="b")
u = union(a, b, kernel="mesh", name="u")
assert_eq(u.kind, "mesh")
"#);
        assert!(errors(&ctx).is_empty(), "{:?}", errors(&ctx));
        // The same coincident-face union on "brep" is a diagnostic, not a
        // silent demotion; on the default "auto" it succeeds as a mesh body.
        let ctx = run(r#"
a = box(20, 20, 10, name="a")
b = box(10, 10, 6, at=(0, 0, 10), name="b")
u = union(a, b, kernel="brep", name="u")
"#);
        assert!(!errors(&ctx).is_empty(), "kernel=\"brep\" must not fall back");
        let ctx = run(r#"
a = box(20, 20, 10, name="a")
b = box(10, 10, 6, at=(0, 0, 10), name="b")
u = union(a, b, name="u")
assert_eq(u.kind, "mesh")
"#);
        assert!(errors(&ctx).is_empty(), "{:?}", errors(&ctx));
        assert!(
            ctx.diagnostics
                .iter()
                .any(|d| d.severity == cadpole_core::context::Severity::Info && d.message.contains("mesh kernel")),
            "the fallback must say why: {:?}",
            ctx.diagnostics.iter().map(|d| d.message.clone()).collect::<Vec<_>>()
        );
        // A plain transversal union stays exact.
        let ctx = run(r#"
a = box(20, 20, 10, name="a")
b = box(10, 10, 6, at=(3, 3, 6), name="b")
u = union(a, b, name="u")
assert_eq(u.kind, "brep")
"#);
        assert!(errors(&ctx).is_empty(), "{:?}", errors(&ctx));
        // An unknown kernel name is a diagnostic naming the choices.
        let ctx = run("u = union(box(1,1,1), box(1,1,1, at=(0.5,0,0)), kernel=\"magic\")\n");
        assert!(errors(&ctx)[0].contains("\"auto\""), "{:?}", errors(&ctx));
    }

    /// `glue` is the same operation `union` routes to on its own; what asking
    /// for it by name buys is the refusal.
    #[test]
    fn glue_joins_touching_bodies_and_refuses_everything_else() {
        // A boss standing on the middle of a plate: the two touch on faces,
        // one wholly inside the other. `union` finds that by itself ...
        let script = |call: &str| {
            format!(
                "plate = box(20, 20, 5, name=\"plate\")\n\
                 boss = box(4, 4, 6, at=(8, 8, 5), name=\"boss\")\n\
                 p = {call}\n\
                 assert_eq(p.kind, \"brep\")\n"
            )
        };
        for call in ["union(plate, boss, kernel=\"brep\", name=\"p\")", "glue(plate, boss, name=\"p\")"] {
            let ctx = run(&script(call));
            assert!(errors(&ctx).is_empty(), "{call}: {:?}", errors(&ctx));
        }
        // ... and where they only overlap, `union` is a kernel boolean and
        // `glue` is an error saying so.
        let overlapping = "a = box(20, 20, 10, name=\"a\")\n\
                           b = box(6, 6, 6, at=(3, 3, 6), name=\"b\")\n";
        let ctx = run(&format!("{overlapping}u = union(a, b, kernel=\"brep\", name=\"u\")\n"));
        assert!(errors(&ctx).is_empty(), "{:?}", errors(&ctx));
        let ctx = run(&format!("{overlapping}u = glue(a, b, name=\"u\")\n"));
        assert!(errors(&ctx)[0].contains("do not share a face"), "{:?}", errors(&ctx));
        // Two bodies that are nowhere near each other are not a glue either.
        let ctx = run("u = glue(box(4,4,4), box(4,4,4, at=(40,0,0)))\n");
        assert!(errors(&ctx)[0].contains("do not touch at all"), "{:?}", errors(&ctx));
        // One body is not a join.
        let ctx = run("u = glue(box(4,4,4))\n");
        assert!(errors(&ctx)[0].contains("at least two"), "{:?}", errors(&ctx));
    }

    /// `union(..., weld=)` joins a box-built story -- boxes touching
    /// along an edge and at a corner, one a hair off its neighbour, a pane
    /// thinner than the weld -- into one solid, every face named after the
    /// box side it lies on, on the first operand's layer, and says what it
    /// bridged and what it closed.
    #[test]
    fn a_welded_union_joins_boxes_that_touch_along_edges_and_at_corners() {
        let ctx = run(concat!(
            "walls = layer(\"walls\")\n",
            "slab = box(30, 30, 1, at=(-5, -5, -1), name=\"slab\", layer=walls)\n",
            "a = box(10, 10, 10, name=\"a\")\n",
            "b = box(10, 10, 10, at=(10, 10, 0), name=\"b\")\n",          // a's edge
            "c = box(4, 4, 4, at=(20, 20, 10), name=\"c\")\n",            // b's corner
            "d = box(10, 0.5, 10, at=(0, -0.504, 0), name=\"d\")\n",      // a hair off a
            "glass = box(6, 0.002, 4, at=(2, 10, 3), name=\"glass\")\n",  // thinner than the weld
            "u = union(slab, a, b, c, d, glass, weld=0.01, name=\"story\")\n",
            "r = print_check(u)\n",
            "assert_true(r.solid, msg=r.summary)\n",
            "assert_eq(u.kind, \"mesh\")\n",
            "assert_eq(u.layer(), \"walls\")\n",
            "assert_true(len(u.faces(name=\"a/*\")) > 0)\n",
            "assert_true(len(u.faces(name=\"story/weld*\")) > 0)\n",
            "assert_close(u.volume(), 900 + 2000 + 64 + 50, tol=1)\n",
            "assert_true(not a.visible() and not glass.visible())\n",
        ));
        assert!(errors(&ctx).is_empty(), "{:?}", errors(&ctx));
        let notes: Vec<String> = ctx.diagnostics.iter().map(|d| d.message.clone()).collect();
        assert!(
            notes.iter().any(|m| m.contains("6 boxes joined exactly")
                && m.contains("1 contact along an edge and 1 at a corner bridged 0.01 wide")),
            "{notes:?}"
        );
        assert!(
            ctx.diagnostics.iter().any(|d| d.severity == cadpole_core::context::Severity::Warning
                && d.message.contains("closed to nothing (the first is glass)")),
            "{notes:?}"
        );
        // The same boxes without the weld are refused.
        let ctx = run(concat!(
            "a = box(10, 10, 10, name=\"a\")\n",
            "b = box(10, 10, 10, at=(10, 10, 0), name=\"b\")\n",
            "u = union(a, b, name=\"u\")\n",
        ));
        assert!(errors(&ctx).iter().any(|e| e.contains("not a solid")), "{:?}", errors(&ctx));
    }

    /// A body that is not an axis-aligned box is unioned exactly onto the
    /// joined boxes; the weld does not reach it, and the note says so. The
    /// refusals: a weld that is not a positive length, and the exact kernel.
    #[test]
    fn a_welded_union_takes_other_bodies_exactly_and_refuses_what_it_cannot_do() {
        let ctx = run(concat!(
            "slab = box(20, 20, 2, name=\"slab\")\n",
            "post = cylinder(2, 6, at=(10, 10, 1), name=\"post\")\n",
            "wall = box(20, 1, 8, at=(0, 0, 2), name=\"wall\")\n",
            "u = union(slab, post, wall, weld=0.01, name=\"u\")\n",
            "assert_true(print_check(u).solid)\n",
            "one = union(box(3, 3, 3, name=\"lone\"), weld=0.01, name=\"one\")\n",
            "assert_eq(one.kind, \"mesh\")\n",
        ));
        assert!(errors(&ctx).is_empty(), "{:?}", errors(&ctx));
        assert!(
            ctx.diagnostics.iter().any(|d| d.message.contains("1 body that is not an axis-aligned box unioned after")),
            "{:?}",
            ctx.diagnostics.iter().map(|d| &d.message).collect::<Vec<_>>()
        );
        for (src, needle) in [
            ("u = union(box(1, 1, 1), box(1, 1, 1, at=(1, 0, 0)), weld=0)", "weld= is a positive length"),
            ("u = union(box(1, 1, 1), box(1, 1, 1, at=(1, 0, 0)), weld=-1)", "weld= is a positive length"),
            ("u = union(box(1, 1, 1), box(1, 1, 1, at=(1, 0, 0)), weld=0.1, kernel=\"brep\")", "kernel=\"brep\" cannot"),
        ] {
            let ctx = run(src);
            assert!(errors(&ctx).iter().any(|e| e.contains(needle)), "{src}: {:?}", errors(&ctx));
        }
    }

    /// A 40-degree corbel under a frame's lip, modelled against the
    /// wall's face and the lip's underside, joins the welded boxes exactly
    /// wherever it stands within the weld of their planes. Its back 10 nm
    /// in front of the wall -- a float rounding -- was refused by the union
    /// gate ("passes through itself"), and 1 to 5 um off gave three
    /// different answers; the weld now moves its vertices onto the planes,
    /// so every one of these is the same solid.
    #[test]
    fn a_wedge_within_the_weld_of_the_boxes_joins_them_exactly() {
        let mut volumes = Vec::new();
        for off in ["0", "0.00001", "-0.00001", "0.0001", "-0.0001", "0.005", "-0.005"] {
            let ctx = run(&format!(
                "wall = box(10, 1.6, 30, name=\"wall\")\n\
                 frame = box(6, 1.9, 2, at=(2, -0.3, 20), name=\"frame\")\n\
                 slab = box(12, 6, 2.4, at=(-1, -3, -2.4), name=\"slab\")\n\
                 corbel = extrude(sketch(YZ.offset(2), name=\"cs\").polygon([({off}, 20), (-0.3, 20), ({off}, 19.6425)]), 6, name=\"corbel\")\n\
                 u = union(slab, wall, frame, corbel, weld=0.01, name=\"u\")\n\
                 r = print_check(u)\n\
                 assert_true(r.solid, msg=r.summary)\n\
                 print(\"V %s\" % u.volume())\n\
                 print(\"F %s\" % len(u.faces()))\n"
            ));
            assert!(errors(&ctx).is_empty(), "off {off}: {:?}", errors(&ctx));
            let said = |p: &str| {
                let p = format!("print: {p}");
                ctx.diagnostics.iter().find_map(|d| d.message.strip_prefix(p.as_str()).map(str::to_owned))
            };
            volumes.push((off, said("V ").unwrap(), said("F ").unwrap()));
            if off != "0" {
                // The move in a unit, not as a float's digits.
                let furthest = if off.ends_with("0.005") { "(the furthest by 5.0 µm)" } else { "(the furthest by " };
                assert!(
                    ctx.diagnostics.iter().any(|d| d.message.contains("corbel is not an axis-aligned box;")
                        && d.message.contains("moved onto them")
                        && d.message.contains(furthest)),
                    "off {off}: {:?}",
                    ctx.diagnostics.iter().map(|d| &d.message).collect::<Vec<_>>()
                );
            }
        }
        assert!(volumes.iter().all(|v| v.1 == volumes[0].1 && v.2 == volumes[0].2), "one solid whatever the offset: {volumes:?}");
    }

    /// The same corbel at exactly the wall's and the lip's planes, at
    /// depths from 0.1 to 1 mm (thinner is under the tolerance, which the
    /// mesh cleanup removes). As an exact tie with the wall's face the
    /// mesh union left that face whole under the corbel -- a fold -- at 0.2
    /// and 0.5 mm and not at 0.3; resting faces now reach half a weld into
    /// the boxes, and every depth is a solid whose corbel carries the lip.
    #[test]
    fn a_corbel_resting_on_the_boxes_joins_them_at_every_depth() {
        for depth in [0.1, 0.2, 0.3, 0.4, 0.5, 0.6, 1.0] {
            let h = depth / fmath::tan(40f64.to_radians());
            let ctx = run(&format!(
                "wall = box(10, 1.6, 30, name=\"wall\")\n\
                 frame = box(6, 1.6 + {depth}, 2, at=(2, -{depth}, 20), name=\"frame\")\n\
                 slab = box(12, 6, 2.4, at=(-1, -3, -2.4), name=\"slab\")\n\
                 corbel = extrude(sketch(YZ.offset(2), name=\"cs\").polygon([(0, 20), (-{depth}, 20), (0, 20 - {h})]), 6, name=\"corbel\")\n\
                 u = union(slab, wall, frame, corbel, weld=0.01, name=\"u\")\n\
                 r = print_check(u)\n\
                 assert_true(r.solid, msg=r.summary)\n\
                 assert_close(r.unsupported_area, 0, tol=0.05, msg=\"the corbel carries the lip: \" + r.summary)\n\
                 assert_close(u.volume(), 12 * 6 * 2.4 + 10 * 1.6 * 30 + 6 * 2 * {depth} + 3 * {depth} * {h}, tol={tol})\n",
                // A corbel twice the tessellation tolerance deep is cleaned to
                // that tolerance: its volume, not the boxes', is what moves.
                tol = (0.6 * 3.0 * depth * h).max(0.01)
            ));
            assert!(errors(&ctx).is_empty(), "depth {depth}: {:?}", errors(&ctx));
        }
    }

    /// What the weld cannot fix -- a roof slab that meets the wall
    /// top's outer edge only along a line -- is refused saying what to
    /// change, after the kernel's own reason.
    #[test]
    fn a_welded_union_that_cannot_hold_says_what_to_change() {
        let ctx = run(concat!(
            "wall = box(10, 1, 10, name=\"wall\")\n",
            "slab = box(12, 4, 1, at=(-1, -1.5, -1), name=\"slab\")\n",
            "roof = extrude(sketch(YZ.offset(0), name=\"rs\").polygon([(1, 10), (5, 12.3), (5, 13.3), (1, 11)]), 10, name=\"roof\")\n",
            "u = union(slab, wall, roof, weld=0.01, name=\"u\")\n",
        ));
        let e = errors(&ctx);
        assert!(
            e.iter().any(|m| m.contains("roof is not an axis-aligned box")
                && m.contains("pinched")
                && m.contains("To join it: make the faces where it rests on or against the boxes lie on their planes")
                && m.contains("build it from boxes")),
            "{e:?}"
        );
    }

    /// `print_check` and `assert_printable` take `max_bridge=`: a window
    /// head 9.5 wide over a 1-thick wall is a bridge at the default (10 mm),
    /// an overhang at `max_bridge=9`, and the report says which.
    #[test]
    fn a_window_head_is_a_bridge_up_to_max_bridge() {
        let wall = concat!(
            "p1 = box(5, 1, 10, name=\"p1\")\n",
            "p2 = box(5, 1, 10, at=(14.5, 0, 0), name=\"p2\")\n",
            "sill = box(9.5, 1, 3, at=(5, 0, 0), name=\"sill\")\n",
            "head = box(9.5, 1, 2, at=(5, 0, 8), name=\"head\")\n",
            "w = union(p1, p2, sill, head, weld=0.001, name=\"w\")\n",
        );
        let ctx = run(&format!(
            "{wall}r = print_check(w)\n\
             assert_eq(r.unsupported_area, 0)\n\
             assert_close(r.bridge_area, 9.5, tol=1e-6)\n\
             assert_close(r.longest_bridge, 9.5, tol=1e-4)\n\
             assert_eq(r.bridges, 1)\n\
             assert_printable(w)\n\
             r9 = print_check(w, max_bridge=9)\n\
             assert_close(r9.unsupported_area, 9.5, tol=1e-6)\n\
             assert_eq(r9.bridges, 0)\n"
        ));
        assert!(errors(&ctx).is_empty(), "{:?}", errors(&ctx));
        let ctx = run(&format!("{wall}assert_printable(w, max_bridge=9)\n"));
        assert!(errors(&ctx).iter().any(|e| e.contains("spans more than max_bridge 9 mm")), "{:?}", errors(&ctx));
        let ctx = run(&format!("{wall}r = print_check(w, max_bridge=-1)\n"));
        assert!(errors(&ctx).iter().any(|e| e.contains("max_bridge is a length")), "{:?}", errors(&ctx));
    }

    /// `assert_support_free` fails with one line per finding, each naming
    /// its check, its position and its face; the support rules are
    /// keywords of `print_check` too, refused when they are negative.
    #[test]
    fn assert_support_free_lists_what_needs_support() {
        let hook = concat!(
            "post = box(4, 4, 30, name=\"post\")\n",
            "arm = box(14, 4, 4, at=(4, 0, 26), name=\"arm\")\n",
            "finger = box(2, 4, 8, at=(14, 0, 18), name=\"finger\")\n",
            "hook = union(post, arm, finger, name=\"hook\")\n",
        );
        let ctx = run(&format!("{hook}assert_support_free(hook, msg=\"the hook\")\n"));
        let e = errors(&ctx);
        let m = e.iter().find(|m| m.contains("assert_support_free failed")).unwrap_or_else(|| panic!("{e:?}"));
        assert!(m.contains("(the hook)"), "{m}");
        assert!(m.contains("\n  - hook: [island] a region starts in mid-air at (14"), "{m}");
        assert!(m.contains("8 mm below where it joins"), "{m}");
        assert!(m.contains("on finger/") || m.contains("on hook/"), "a face name: {m}");
        // Off by keywords: an island must hang deeper than 9 mm, and the
        // finger's 8 mm2 tip is a patch within a 10 mm2 allowance.
        let ctx = run(&format!(
            "{hook}r = print_check(hook, min_island=9, max_overhang_area=10)\nassert_eq(r.islands, 0)\n\
             assert_true(r.support_free, msg=str(r.support))\n\
             assert_support_free(hook, min_island=9, max_overhang_area=10)\n"
        ));
        assert!(errors(&ctx).is_empty(), "{:?}", errors(&ctx));
        // A printer value carries the rules.
        let ctx = run("p = printer(\"bambu_a1\")\nassert_eq(p.max_overhang_area, 4.0)\nassert_eq(p.max_cantilever, 3.0)\nassert_eq(p.min_island, p.layer_height)\n");
        assert!(errors(&ctx).is_empty(), "{:?}", errors(&ctx));
        let ctx = run(&format!("{hook}r = print_check(hook, max_cantilever=-1)\n"));
        assert!(errors(&ctx).iter().any(|e| e.contains("max_cantilever must be 0 or more")), "{:?}", errors(&ctx));
    }

    #[test]
    fn duplicate_names_are_rejected() {
        let ctx = run("a = box(1,1,1, name=\"x\")\nb = box(1,1,1, name=\"x\")\n");
        assert!(errors(&ctx)[0].contains("used twice"));
    }


    // ---- printing (docs/design.md "3D-printing checks", docs/starlark-api.md 6.1) ------

    #[test]
    fn print_check_returns_a_report_a_script_can_assert_on() {
        let ctx = run(
            "b = box(20, 20, 20, name=\"b\")\n\
             r = print_check(b)\n\
             assert_true(r.ok)\n\
             assert_true(r.watertight)\n\
             assert_eq(r.triangles, 12)\n\
             assert_true(r.min_wall > 19.9)\n\
             assert_eq(r.unsupported_area, 0.0)\n\
             assert_true(r.fits_bed)\n\
             assert_eq(r.holes, 0)\n\
             assert_eq(len(r.errors), 0)\n\
             assert_eq(r.size, (20.0, 20.0, 20.0))\n",
        );
        assert!(errors(&ctx).is_empty(), "{:?}", errors(&ctx));
    }

    #[test]
    fn print_check_never_fails_the_script_even_when_the_model_is_hopeless() {
        // 400 mm cube on a 256 mm bed: an error-severity finding, but the
        // evaluation still succeeds and the report says so.
        let ctx = run(
            "b = box(400, 400, 400, name=\"b\")\n\
             r = print_check(b)\n\
             assert_true(not r.ok)\n\
             assert_true(not r.fits_bed)\n\
             assert_eq(r.bed_excess, (144.0, 144.0, 144.0))\n\
             assert_true(len(r.errors) == 1)\n",
        );
        assert!(errors(&ctx).is_empty(), "printing checks are advice: {:?}", errors(&ctx));
        let warnings: Vec<&str> =
            ctx.diagnostics.iter().filter(|d| d.severity == cadpole_core::context::Severity::Warning).map(|d| d.message.as_str()).collect();
        assert!(warnings.iter().any(|w| w.contains("[bed_fit]")), "{warnings:?}");
        assert!(warnings.iter().any(|w| w.contains("144 mm on X")), "{warnings:?}");
    }

    #[test]
    fn a_thin_wall_is_a_warning_with_a_position() {
        let ctx = run(
            "outer = box(20, 20, 20)\n\
             inner = box(18.8, 18.8, 18.8, at=(0.6, 0.6, 0.6))\n\
             shell = subtract(outer, inner, name=\"shell\")\n\
             r = print_check(shell)\n\
             assert_true(shell.kind == \"brep\")\n\
             assert_true(r.min_wall < 0.61)\n\
             assert_true(r.thin_area > 2000)\n",
        );
        assert!(errors(&ctx).is_empty(), "{:?}", errors(&ctx));
        let w: Vec<&str> =
            ctx.diagnostics.iter().filter(|d| d.severity == cadpole_core::context::Severity::Warning).map(|d| d.message.as_str()).collect();
        assert!(w.iter().any(|m| m.contains("[thickness]") && m.contains("0.8 mm minimum")), "{w:?}");
    }

    #[test]
    fn orient_for_print_returns_a_rotated_body_and_says_why() {
        // A 20 x 20 x 100 post prints lying down: same supports, ten times the
        // bed contact, a fifth of the height.
        let ctx = run(
            "b = box(20, 20, 100, name=\"b\")\n\
             flat = orient_for_print(b, name=\"flat\")\n",
        );
        assert!(errors(&ctx).is_empty(), "{:?}", errors(&ctx));
        let f = ctx.features.iter().find(|f| f.id.0 == "flat").expect("the feature is in the tree");
        assert_eq!(f.kind, "orient_for_print");
        assert!(f.args.contains_key("reasoning"), "{:?}", f.args);
        let body = ctx.bodies.iter().find(|b| b.feature.0 == "flat").expect("a rotated body");
        let (lo, hi) = ctx.body(body.id).unwrap().body.to_mesh(0.1).unwrap().0.bbox().unwrap();
        assert!((hi.z - lo.z - 20.0).abs() < 1e-6, "it should be lying down: {lo:?} {hi:?}");
        assert!(lo.z.abs() < 1e-6, "and sitting on the bed: {lo:?}");
        let info: Vec<&str> =
            ctx.diagnostics.iter().filter(|d| d.severity == cadpole_core::context::Severity::Info).map(|d| d.message.as_str()).collect();
        assert!(info.iter().any(|m| m.contains("orient_for_print") && m.contains("mm tall")), "{info:?}");
    }

    #[test]
    fn a_named_printer_supplies_the_bed_and_the_call_site_still_wins() {
        let ctx = run(
            "p = printer(\"prusa_mk4\")\n\
             assert_eq(p.name, \"prusa_mk4\")\n\
             assert_eq(p.bed, (250.0, 210.0, 220.0))\n\
             assert_eq(p.nozzle, 0.4)\n\
             assert_eq(p.layer_height, 0.2)\n\
             assert_eq(p.material, \"PLA\")\n\
             assert_true(\"Prusa\" in p.source)\n\
             b = box(240, 200, 100, name=\"b\")\n\
             r = print_check(b, printer=p)\n\
             assert_true(r.fits_bed)\n\
             assert_eq(r.profile, \"prusa_mk4\")\n\
             assert_eq(r.layers, 500)\n\
             # A name works as well as a value, and an argument beats both.\n\
             r2 = print_check(b, printer=\"prusa_mk4\", nozzle=0.6, material=\"PETG\")\n\
             assert_true(r2.mass > r.mass)\n",
        );
        assert!(errors(&ctx).is_empty(), "{:?}", errors(&ctx));
    }

    #[test]
    fn the_same_part_fits_one_printer_and_not_another() {
        let ctx = run(
            "b = box(200, 170, 200, name=\"b\")\n\
             assert_true(print_check(b, printer=\"bambu_x1c\").fits_bed)\n\
             assert_true(not print_check(b, printer=\"bambu_a1_mini\").fits_bed)\n",
        );
        assert!(errors(&ctx).is_empty(), "{:?}", errors(&ctx));
    }

    #[test]
    fn an_unknown_printer_lists_the_ones_there_are() {
        let ctx = run("p = printer(\"makerbot_cupcake\")\n");
        let e = errors(&ctx);
        assert!(e[0].contains("bambu_a1") && e[0].contains("ender3"), "{e:?}");
    }

    #[test]
    fn a_part_too_tall_for_the_bed_is_told_where_to_cut_it() {
        let ctx = run(
            "b = box(100, 100, 400, name=\"tall\")\n\
             r = print_check(b, printer=\"bambu_a1\")\n\
             assert_true(not r.fits_bed)\n\
             assert_eq(r.split_axis, \"z\")\n\
             assert_eq(r.split_pieces, 2)\n\
             assert_true(\"split(body, plane(\" in r.split_advice)\n",
        );
        assert!(errors(&ctx).is_empty(), "{:?}", errors(&ctx));
        let w: Vec<&str> = ctx
            .diagnostics
            .iter()
            .filter(|d| d.severity == cadpole_core::context::Severity::Warning)
            .map(|d| d.message.as_str())
            .collect();
        assert!(w.iter().any(|m| m.contains("[bed_fit]") && m.contains("split(body, plane(")), "{w:?}");
    }

    // ---- split (docs/design.md "3D-printing checks", docs/starlark-api.md 6.1) ---------

    #[test]
    fn split_cuts_a_body_in_two_along_the_plane_normal() {
        let ctx = run(
            "b = box(20, 20, 40, name=\"tall\")\n\
             lower, upper = split(b, plane(origin=(0, 0, 25), normal=(0, 0, 1)), kernel=\"brep\", name=\"half\")\n\
             assert_true(abs(print_check(lower).volume - 10000.0) < 1.0)\n\
             assert_true(abs(print_check(upper).volume - 6000.0) < 1.0)\n",
        );
        assert!(errors(&ctx).is_empty(), "{:?}", errors(&ctx));
        let names: Vec<&str> = ctx.bodies.iter().filter(|b| b.visible).map(|b| b.name.as_str()).collect();
        assert_eq!(names, vec!["half", "half#1"], "{names:?}");
    }

    #[test]
    fn the_pieces_of_a_split_carry_a_cut_face_and_keep_their_old_names() {
        let ctx = run(
            "b = box(20, 20, 40, name=\"tall\")\n\
             pieces = split(b, plane(origin=(0, 0, 25), normal=(0, 0, 1)), kernel=\"brep\")\n\
             for p in pieces:\n\
             \x20   names = [f.name for f in p.faces()]\n\
             \x20   assert_true(\"split_1/cut\" in names)\n\
             \x20   assert_true(len([n for n in names if n.startswith(\"tall/\")]) >= 4)\n",
        );
        assert!(errors(&ctx).is_empty(), "{:?}", errors(&ctx));
    }

    #[test]
    fn split_by_another_body_keeps_the_cutter() {
        let ctx = run(
            "b = box(20, 20, 20, name=\"b\")\n\
             slab = box(40, 40, 13, at=(-10, -10, -5), name=\"slab\")\n\
             inside, outside = split(b, by=slab, kernel=\"brep\", name=\"part\")\n\
             assert_true(abs(print_check(inside).volume - 3200.0) < 1.0)\n\
             assert_true(abs(print_check(outside).volume - 4800.0) < 1.0)\n",
        );
        assert!(errors(&ctx).is_empty(), "{:?}", errors(&ctx));
        // The cutter is still there; the source body is not.
        let visible: Vec<&str> = ctx.bodies.iter().filter(|b| b.visible).map(|b| b.name.as_str()).collect();
        assert!(visible.contains(&"slab"), "{visible:?}");
        assert!(!visible.contains(&"b"), "{visible:?}");
    }

    /// `keep=` returns one piece as a Body and consumes the other, so
    /// an export leaves it out; a word that does not fit the cut is refused.
    #[test]
    fn split_keeps_one_piece_when_asked() {
        let ctx = run(
            "b = box(20, 20, 40, name=\"tall\")\n\
             low = split(b, plane(origin=(0, 0, 25)), keep=\"below\", name=\"low\")\n\
             assert_eq(type(low), \"Body\")\n\
             assert_close(low.volume(), 10000, tol=1e-6)\n\
             c = box(20, 20, 20, at=(50, 0, 0), name=\"c\")\n\
             slab = box(40, 40, 13, at=(40, -10, -5), name=\"slab\")\n\
             out = split(c, by=slab, keep=\"outside\", name=\"out\")\n\
             assert_close(out.volume(), 4800, tol=1e-6)\n",
        );
        assert!(errors(&ctx).is_empty(), "{:?}", errors(&ctx));
        let visible: Vec<&str> = ctx.bodies.iter().filter(|b| b.visible).map(|b| b.name.as_str()).collect();
        assert_eq!(visible, ["low", "slab", "out#1"], "{visible:?}");
        for (src, needle) in [
            ("split(box(1, 1, 2), plane(origin=(0, 0, 1)), keep=\"inside\")", "\"both\", \"below\" or \"above\""),
            ("split(box(1, 1, 2), by=box(4, 4, 1), keep=\"below\")", "\"both\", \"inside\" or \"outside\""),
        ] {
            let ctx = run(src);
            assert!(errors(&ctx).iter().any(|e| e.contains(needle)), "{src}: {:?}", errors(&ctx));
        }
    }

    #[test]
    fn a_split_that_misses_the_body_says_so() {
        let ctx = run(
            "b = box(20, 20, 20, name=\"b\")\n\
             split(b, plane(origin=(0, 0, 500), normal=(0, 0, 1)))\n",
        );
        assert!(errors(&ctx)[0].contains("does not cross the body"), "{:?}", errors(&ctx));
    }

    #[test]
    fn split_needs_a_plane_or_a_body_and_not_both() {
        let ctx = run("b = box(10, 10, 10, name=\"b\")\nsplit(b)\n");
        assert!(errors(&ctx)[0].contains("pass a plane"), "{:?}", errors(&ctx));
        let ctx = run(
            "b = box(10, 10, 10, name=\"b\")\n\
             c = box(4, 4, 4, name=\"c\")\n\
             split(b, XY, by=c)\n",
        );
        assert!(errors(&ctx)[0].contains("not both"), "{:?}", errors(&ctx));
    }

    #[test]
    fn paint_records_a_feature_and_rejects_a_name_that_is_not_there() {
        let ctx = run(
            "b = box(10, 10, 10, name=\"b\")\n\
             paint(b, faces=b.faces(normal=(0, 0, 1)), color=\"#FF8800\", slot=2, name=\"top\")\n",
        );
        assert!(errors(&ctx).is_empty(), "{:?}", errors(&ctx));
        let paints = cadpole_core::paint::paints(&ctx);
        assert_eq!(paints.len(), 1);
        assert_eq!(paints[0].feature, "top");
        assert_eq!(paints[0].hex, "#FF8800");
        assert_eq!(paints[0].slot, Some(2));
        assert_eq!(paints[0].faces.len(), 1, "one upward face on a box: {:?}", paints[0].faces);

        let bad = run("b = box(10, 10, 10, name=\"b\")\npaint(b, faces=[\"face:99\"], color=\"#FF0000\")\n");
        assert!(errors(&bad)[0].contains("face:99"), "{:?}", errors(&bad));
        let no_color = run("b = box(10, 10, 10, name=\"b\")\npaint(b)\n");
        assert!(errors(&no_color)[0].contains("color="), "{:?}", errors(&no_color));
    }

    #[test]
    fn painting_a_whole_body_needs_no_faces() {
        let ctx = run("b = box(10, 10, 10, name=\"b\")\npaint(b, color=(0.1, 0.2, 0.3))\n");
        assert!(errors(&ctx).is_empty(), "{:?}", errors(&ctx));
        let paints = cadpole_core::paint::paints(&ctx);
        assert!(paints[0].faces.is_empty(), "the whole body");
        // A tuple is linear; the feature records it as sRGB bytes, so linear
        // 0.1 is byte 0x59, not 0x1A (paint::hex).
        assert_eq!(paints[0].hex, "#597C95");
    }

    #[test]
    fn export_accepts_3mf() {
        let ctx = run("b = box(10, 10, 10, name=\"b\")\nexport(b, \"b.3mf\")\n");
        assert!(errors(&ctx).is_empty(), "{:?}", errors(&ctx));
        assert_eq!(ctx.exports[0].format, "3mf");
        assert_eq!(ctx.exports[0].flavor, cadpole_core::export::Flavor::Standard);
        assert!(ctx.exports[0].plate.is_none() && ctx.exports[0].name.is_none() && ctx.exports[0].extruders.is_empty());
        let bad = run("b = box(10, 10, 10)\nexport(b, \"b.gcode\")\n");
        assert!(errors(&bad)[0].contains(".3mf"), "{:?}", errors(&bad));
    }

    /// `plate_images(path, size=, background=, backend=)`
    /// records what the host renders after the project's own export calls,
    /// and refuses everything else at the call.
    #[test]
    fn plate_images_are_declared_after_the_project_and_refused_otherwise() {
        use cadpole_core::context::PlateImageSpec;
        let ctx = run(concat!(
            "a = box(10, 10, 10, name=\"a\")\n",
            "export(a, \"toy.3mf\", flavor=flavor.bambu, plate=\"Core\")\n",
            "export(a, \"other.3mf\", flavor=flavor.bambu)\n",
            "plate_images(\"toy.3mf\")\n",
            "plate_images(\"other.3mf\", size=(800, 600), background=\"studio\", backend=\"path\")\n",
        ));
        assert!(errors(&ctx).is_empty(), "{:?}", errors(&ctx));
        assert_eq!(
            ctx.plate_images,
            vec![
                PlateImageSpec {
                    path: "toy.3mf".into(),
                    width: 1600,
                    height: 1200,
                    background: "transparent".into(),
                    backend: "raster".into(),
                },
                PlateImageSpec {
                    path: "other.3mf".into(),
                    width: 800,
                    height: 600,
                    background: "studio".into(),
                    backend: "path".into(),
                },
            ]
        );
        assert!(ctx.features.iter().all(|f| f.kind != "plate_images"), "a declaration is not a feature");

        // Each refusal is one error at the call, and records nothing (the
        // second of two declarations leaves the first standing).
        let refused = |call: &str, needle: &str| -> usize {
            let src = format!("a = box(10, 10, 10, name=\"a\")\nexport(a, \"toy.3mf\", flavor=flavor.bambu)\n{call}\n");
            let ctx = run(&src);
            let e = errors(&ctx);
            assert_eq!(e.len(), 1, "{call}: {e:?}");
            assert!(e[0].contains(needle), "{call}: {}", e[0]);
            ctx.plate_images.len()
        };
        assert_eq!(refused("plate_images(\"lid.3mf\")", "no export(..., \"lid.3mf\", flavor=flavor.bambu) has been declared yet"), 0);
        assert_eq!(refused("plate_images(\"toy.3mf\")\nplate_images(\"toy.3mf\")", "declared twice"), 1);
        assert_eq!(refused("plate_images(\"toy.3mf\", size=800)", "takes (width, height)"), 0);
        assert_eq!(refused("plate_images(\"toy.3mf\", size=(800, 600, 2))", "got 3 values"), 0);
        assert_eq!(refused("plate_images(\"toy.3mf\", size=(800, 8))", "16 to 8192 pixels a side, got 8"), 0);
        assert_eq!(refused("plate_images(\"toy.3mf\", size=(1.5, 600))", "takes whole pixels"), 0);
        assert_eq!(refused("plate_images(\"toy.3mf\", background=\"neon\")", "\"transparent\", \"studio\", \"white\", \"black\" or \"#rrggbb\""), 0);
        assert_eq!(refused("plate_images(\"toy.3mf\", backend=\"gpu\")", "not \"raster\" or \"path\""), 0);
        // A plain 3MF has no plates to picture.
        let ctx = run("a = box(10, 10, 10)\nexport(a, \"plain.3mf\")\nplate_images(\"plain.3mf\")\n");
        assert!(errors(&ctx)[0].contains("flavor=flavor.bambu"), "{:?}", errors(&ctx));
    }

    /// `export(flavor=flavor.bambu, plate=, name=, extruder=, printer=)`:
    /// what each call records, and what it refuses at the call.
    #[test]
    fn export_takes_the_bambu_flavor_its_plates_names_extruders_and_printer() {
        use cadpole_core::context::PlateRef;
        use cadpole_core::export::Flavor;
        let ctx = run(concat!(
            "a = box(10, 10, 10, name=\"a\")\n",
            "b = box(10, 10, 10, at=(20, 0, 0), name=\"b\")\n",
            "export([a, b], \"toy.3mf\", flavor=flavor.bambu, plate=\"Core\", name=\"core\", extruder=[2, 3])\n",
            "export(a, \"toy.3mf\", flavor=\"bambu\", plate=2, extruder=1, printer=\"a1 mini\")\n",
            "export(b, \"toy.3mf\", flavor=flavor.bambu)\n",
        ));
        assert!(errors(&ctx).is_empty(), "{:?}", errors(&ctx));
        assert_eq!(ctx.exports.len(), 3);
        let e = &ctx.exports[0];
        assert_eq!(e.flavor, Flavor::Bambu);
        assert_eq!(e.plate, Some(PlateRef::Name("Core".into())));
        assert_eq!(e.name.as_deref(), Some("core"));
        assert_eq!(e.extruders, [2, 3]);
        let e = &ctx.exports[1];
        assert_eq!(e.plate, Some(PlateRef::Number(2)));
        assert_eq!(e.extruders, [1], "one number for every body of the call");
        assert_eq!(e.printer, "bambu_a1_mini", "the printer table's own spelling");
        assert!(ctx.exports[2].plate.is_none(), "no plate= is plate 1, resolved by the host");

        let refused = |src: &str, what: &str| {
            let ctx = run(&format!("a = box(10, 10, 10, name=\"a\")\nb = box(10, 10, 10, at=(20, 0, 0), name=\"b\")\n{src}\n"));
            let errs = errors(&ctx);
            assert!(errs.iter().any(|e| e.contains(what)), "{src:?}: expected {what:?} in {errs:?}");
        };
        refused("export(a, \"a.3mf\", plate=1)", "plate= is a Bambu Studio project's plate");
        refused("export(a, \"a.3mf\", flavor=flavor.bambu, plate=0)", "plate must be 1 or more");
        refused("export(a, \"a.3mf\", flavor=flavor.bambu, plate=\"\")", "plate name must not be empty");
        refused("export(a, \"a.3mf\", flavor=flavor.bambu, plate=1.5)", "takes a number or a name");
        refused("export(a, \"a.stl\", flavor=flavor.bambu)", "is not a .3mf");
        refused("export(a, \"a.3mf\", flavor=\"orca\")", "valid: standard, bambu");
        refused("export(a, \"a.3mf\", flavor=flavor.orca)", "flavor.orca is not a member of flavor");
        refused("export([a, b], \"a.3mf\", flavor=flavor.bambu, extruder=[1, 2, 3])", "names 3 slots for 2 bodies");
        refused("export(a, \"a.3mf\", flavor=flavor.bambu, extruder=0)", "numbered from 1");
        refused("export(a, \"a.3mf\", flavor=flavor.bambu, extruder=\"AMS\")", "takes a number or a list of numbers");
        refused("export(a, \"a.3mf\", flavor=flavor.bambu, printer=\"replicator\")", "unknown printer");
        refused("export(a, \"a.3mf\", printer=\"bambu_x1c\")", "printer= lays out a Bambu Studio project's plates");
    }

    /// Bodies the examples produce that are not solids, as of this writing:
    /// `(example, kernel, body, failing check, why)`. `kernel` is the value
    /// of the script's `kernel` parameter for the run (`"default"` for a
    /// script without one; `"*"` matches any), and the check is a name from
    /// `cadpole_core::solid::SolidReport::failing`. A new example that is not a
    /// solid fails the build until it is fixed or listed here with its
    /// reason; an entry that stops failing that way fails it until removed.
    const KNOWN_NON_SOLID_EXAMPLES: &[(&str, &str, &str, &str, &str)] = &[
        // The wing's registration skin: "N1234" painted on the section 1 skin
        // is 558 edges of 0.04–0.5 mm on a curved patch whose cells
        // are millimetres, and its tessellation carries slivers a chord long
        // on a base a step wide whose planes pass through each other by
        // 0.3–0.8 µm, against the crossing test's 0.2 µm (eight f32 ulps of
        // the span). Watertight by id, the mark on the skin to 1e-7 mm; the
        // example's own tests judge it watertight ("the wing's mark").
        (
            "wing.star",
            "*",
            "s1_r/s1_registration",
            "crossings",
            "sub-micron crossings between slivers of the painted registration's outline on a curved skin",
        ),
        (
            "wing.star",
            "*",
            "s1_l/s1_mirrored",
            "crossings",
            "the mirror image of the same skin",
        ),
        // A room scan is reference geometry and an open surface by nature:
        // the synthetic room has no ceiling and a doorway through its front
        // wall, as a phone's scan of a room has holes wherever the phone did
        // not look. Imported with `repair.off`, which says so as information;
        // the walls drawn over it are the example's solids.
        (
            "scan_underlay.star",
            "*",
            "scan",
            "holes",
            "a room scan: an open surface, imported as reference with repair.off",
        ),
    ];

    fn example_files() -> (String, Vec<std::path::PathBuf>) {
        // The examples are what `cadpole eval` runs, and the CLI has a solver;
        // so do the gates that evaluate them.
        cadpole_opt::install();
        let dir = std::env::var("CADPOLE_EXAMPLES").unwrap_or_else(|_| "examples".into());
        let mut files: Vec<_> = std::fs::read_dir(&dir)
            .expect("the examples directory")
            .flatten()
            .map(|e| e.path())
            .filter(|p| p.extension().and_then(|e| e.to_str()) == Some("star"))
            // `*_future.star` examples are written against planned API.
            .filter(|p| !p.file_name().and_then(|n| n.to_str()).map(|n| n.ends_with("_future.star")).unwrap_or(false))
            .collect();
        files.sort();
        (dir, files)
    }

    /// The script list for one example, the way `cadpole eval examples/x.star`
    /// builds it (`cadpole-cli/src/input.rs`): the example itself as the entry
    /// `main.star`, plus every other `.star` in the directory under its own
    /// name so that `load("sibling.star", ...)` resolves. Loads are lazy, so
    /// an example that loads nothing is evaluated exactly as before; without
    /// this an example may not be a multi-file document, which is a
    /// restriction the CLI does not have and the UI does not have either.
    fn example_scripts(dir: &str, entry: &std::path::Path) -> Vec<ScriptFile> {
        let src = std::fs::read_to_string(entry).expect("the example");
        let mut scripts = vec![ScriptFile { path: "main.star".into(), source: src }];
        let mut sibs: Vec<_> = std::fs::read_dir(dir)
            .expect("the examples directory")
            .flatten()
            .map(|e| e.path())
            .filter(|p| p != entry && p.extension().and_then(|e| e.to_str()) == Some("star"))
            .collect();
        sibs.sort();
        for p in sibs {
            let (Some(name), Ok(source)) = (p.file_name().and_then(|n| n.to_str()), std::fs::read_to_string(&p)) else {
                continue;
            };
            scripts.push(ScriptFile { path: name.to_owned(), source });
        }
        scripts
    }

    fn example_assets(dir: &str) -> Arc<dyn AssetStore> {
        let (assets, warnings) = cadpole_assets::load_dir(std::path::Path::new(dir).join("assets").as_path());
        assert!(warnings.is_empty(), "{warnings:?}");
        Arc::new(assets)
    }

    /// A body that is not a solid: `(example, kernel, body, failing checks,
    /// summary)`.
    type NonSolid = (String, String, String, Vec<&'static str>, String);

    /// Evaluate one example, `kernel` overridden or not, and validate every
    /// visible body with `cadpole_core::solid`. A kernel the script offers may
    /// refuse a boolean — the exact kernel declines tangent and partly
    /// coincident pairs by design — and that is a diagnostic, not a
    /// non-solid; the bodies it did return are still checked.
    fn validate_example(
        scripts: &[ScriptFile],
        assets: &Arc<dyn AssetStore>,
        file: &str,
        kernel: Option<&str>,
        label: &str,
    ) -> (Context, Vec<NonSolid>) {
        let mut overrides = HashMap::new();
        if let Some(k) = kernel {
            overrides.insert("kernel".to_owned(), ParamValue::Text(k.to_owned()));
        }
        let mut ctx = evaluate(EvalInput {
            scripts: scripts.to_vec(),
            assets: Some(assets.clone()),
            overrides,
            ..Default::default()
        });
        let errs = errors(&ctx);
        if !errs.is_empty() {
            eprintln!("{file} kernel={label}: refused: {}", errs.join(" | "));
        }
        let ids: Vec<(cadpole_core::BodyId, String)> =
            ctx.bodies.iter().filter(|b| b.visible).map(|b| (b.id, b.name.clone())).collect();
        let mut out = Vec::new();
        for (id, name) in &ids {
            let report = match ctx.body_mesh(*id) {
                Ok((mesh, _)) => cadpole_core::solid::validate(mesh),
                Err(e) => {
                    out.push((file.to_owned(), label.to_owned(), name.clone(), vec!["untessellatable"], e.to_string()));
                    continue;
                }
            };
            if !report.is_solid() {
                out.push((file.to_owned(), label.to_owned(), name.clone(), report.failing(), report.summary()));
            }
        }
        eprintln!(
            "{file} kernel={label}: {} bod{}, {} not solid",
            ids.len(),
            if ids.len() == 1 { "y" } else { "ies" },
            out.len()
        );
        (ctx, out)
    }

    /// New failures and stale allowlist entries both fail, so the list is
    /// edited on purpose in both directions. `ran` is every (example, kernel)
    /// pair the test evaluated.
    fn judge_solids(found: Vec<NonSolid>, ran: &[(String, String)]) {
        let matches = |e: &(&str, &str, &str, &str, &str), f: &NonSolid| {
            e.0 == f.0 && (e.1 == f.1 || e.1 == "*") && e.2 == f.2 && f.3.contains(&e.3)
        };
        let new: Vec<&NonSolid> =
            found.iter().filter(|f| !KNOWN_NON_SOLID_EXAMPLES.iter().any(|e| matches(e, f))).collect();
        let stale: Vec<_> = KNOWN_NON_SOLID_EXAMPLES
            .iter()
            .filter(|e| ran.iter().any(|(file, k)| *file == e.0 && (e.1 == "*" || e.1 == k)))
            .filter(|e| !found.iter().any(|f| matches(e, f)))
            .collect();
        let mut msg = String::new();
        if !new.is_empty() {
            msg.push_str(&format!(
                "{} example bod{} not a solid and not in KNOWN_NON_SOLID_EXAMPLES:\n",
                new.len(),
                if new.len() == 1 { "y is" } else { "ies are" }
            ));
            for (file, kernel, body, failing, summary) in &new {
                msg.push_str(&format!("- {file} kernel={kernel} body {body:?} {failing:?}: {summary}\n"));
            }
        }
        if !stale.is_empty() {
            msg.push_str("KNOWN_NON_SOLID_EXAMPLES entries that no longer fail that way; edit them:\n");
            for (file, kernel, body, check, why) in &stale {
                msg.push_str(&format!("- {file} kernel={kernel} body {body:?} ({check}): {why}\n"));
            }
        }
        if !msg.is_empty() {
            eprintln!("{msg}");
        }
        assert!(msg.is_empty(), "{msg}");
    }

    /// Does the script declare a `kernel` parameter? Decided from the source,
    /// so the solid gate knows which half of the corpus a script is in before
    /// evaluating it, and confirmed against the declaration after evaluation.
    fn offers_a_kernel(src: &str) -> bool {
        src.contains("param(\"kernel\"")
    }

    /// The impeller's cap nut — a hex prism intersected with a chamfer
    /// loft — measured 0.176 mm of chord error at 0.05 and the same at 0.01
    /// under the old face step (the cone ∩ flat intersection edge
    /// divided coarser than the tolerance, which no interior division could
    /// mend). With every edge divided by its
    /// own sag test the nut's chord error follows the tolerance, and the nut
    /// is closed by id at 0.01.
    #[test]
    fn the_impeller_nut_meshes_closed_at_the_export_tolerance_with_its_chord_error_under_it() {
        let (dir, _) = example_files();
        let src = std::fs::read_to_string(std::path::Path::new(&dir).join("impeller.star")).unwrap();
        let ctx = run(&src);
        let nut = ctx.bodies.iter().find(|b| b.name == "nut").expect("the nut");
        let solid = nut.solid().unwrap();
        for tol in [0.05, 0.01] {
            let t = cadpole_core::kernel::tessellate_reporting(solid, tol).unwrap_or_else(|e| panic!("at {tol}: {e}"));
            assert_eq!(t.meshed.unmatched_edges().0, 0, "at {tol}: open by id");
            let chord = t.chord.expect("a cone to measure against").max;
            assert!(chord <= 1.5 * tol, "at {tol}: chord error {chord}");
        }
    }

    /// `assert_solid` is `assert_true(print_check(b).solid)` without the
    /// print profile: it passes a solid and fails with the validator's reason
    /// on anything else.
    #[test]
    fn assert_solid_passes_a_solid_and_names_what_is_wrong_otherwise() {
        let ctx = run("b = box(10, 10, 10, name=\"b\")\nassert_solid(b)\nassert_solid(b, msg=\"twice\")\n");
        assert!(errors(&ctx).is_empty(), "{:?}", errors(&ctx));

        // The negative case needs a body that is not a solid, and nothing
        // that builds geometry makes one any more: every boolean result is
        // validated (`Context::check_mesh_result`), and the field mesher
        // measures its own output and refuses a non-solid. What is left is
        // an import: the example room scan is an open surface, imported with
        // `repair.off`.
        let dir = std::env::var("CADPOLE_EXAMPLES").unwrap_or_else(|_| "examples".into());
        let ctx = evaluate(EvalInput {
            scripts: vec![ScriptFile {
                path: "main.star".into(),
                source: "scan = import_mesh(\"scan_room.glb\", name=\"scan\", repair=repair.off)\n\
                         assert_solid(scan, msg=\"the scan\")\n"
                    .into(),
            }],
            assets: Some(example_assets(&dir)),
            ..Default::default()
        });
        let e = errors(&ctx);
        assert_eq!(e.len(), 1, "{e:?}");
        assert!(e[0].contains("assert_solid failed: scan is not a solid"), "{}", e[0]);
        assert!(e[0].contains("(the scan)"), "{}", e[0]);
    }

    /// Contacts the mesh kernel would answer with a body that is not a
    /// solid are refused by name.
    ///
    /// Two boxes meeting along one edge came back as both lumps with the
    /// shared edge on four triangles — a surface pinched to nothing there,
    /// with no inside for a slicer to find — and `intersect` of two boxes
    /// sharing a face came back as a zero-volume sheet of four triangles,
    /// because the only empty result `finish` refused was one with no
    /// triangles at all. The exact kernel refuses both contacts by name, so
    /// these are the two kernels agreeing about what a body is.
    #[test]
    fn the_mesh_kernel_refuses_a_result_that_is_not_a_solid() {
        let ctx = run(
            "a = box(10, 10, 10)\nb = box(10, 10, 10, at=(10, 10, 0))\n\
             u = union(a, b, kernel=\"mesh\", name=\"touching\")\n",
        );
        let e = errors(&ctx);
        assert_eq!(e.len(), 1, "{e:?}");
        assert!(e[0].contains("not a solid"), "{}", e[0]);
        assert!(e[0].contains("non-manifold edge"), "{}", e[0]);
        assert!(e[0].contains("0.5 mm"), "the fix has to be a number: {}", e[0]);

        let ctx = run(
            "a = box(10, 10, 10)\nb = box(10, 10, 10, at=(10, 0, 0))\n\
             i = intersect(a, b, kernel=\"mesh\", name=\"sheet\")\n",
        );
        let e = errors(&ctx);
        assert_eq!(e.len(), 1, "{e:?}");
        assert!(e[0].contains("a surface, not a solid"), "{}", e[0]);

        // Overlapping by more than nothing, both are ordinary booleans.
        let ctx = run(
            "a = box(10, 10, 10)\nb = box(10, 10, 10, at=(9, 9, 0))\n\
             u = union(a, b, kernel=\"mesh\", name=\"lapped\")\n\
             assert_solid(u)\n",
        );
        assert!(errors(&ctx).is_empty(), "{:?}", errors(&ctx));
    }

    /// Run a script against an in-memory asset table, the way every front end
    /// does: the script names assets, never paths.
    fn run_with_assets(src: &str, files: &[(&str, &[u8])]) -> Context {
        let mut store = cadpole_assets::MemoryAssetStore::new();
        for (name, bytes) in files {
            store.insert(name, bytes.to_vec());
        }
        evaluate(EvalInput {
            scripts: vec![ScriptFile { path: "main.star".into(), source: src.into() }],
            assets: Some(Arc::new(store)),
            ..Default::default()
        })
    }

    const BADGE_SVG: &[u8] = br#"<svg xmlns="http://www.w3.org/2000/svg" width="40mm" height="40mm" viewBox="0 0 40 40">
  <g id="badge"><path d="M0 0 H40 V40 H0 Z M12 12 H28 V28 H12 Z"/></g>
  <circle id="dot" cx="20" cy="20" r="3"/>
</svg>"#;

    #[test]
    fn an_svg_asset_imports_extrudes_and_keeps_its_names() {
        let ctx = run_with_assets(
            r#"
logo = asset("badge.svg")
assert_eq(logo.kind, "svg")
s = import_svg(logo, ids=["badge"], name="badge")
assert_eq(s.loops, 2)
plate = extrude(s, 3, name="plate")
"#,
            &[("badge.svg", BADGE_SVG)],
        );
        assert!(errors(&ctx).is_empty(), "{:?}", errors(&ctx));
        let f = ctx.feature(&"badge".into()).unwrap();
        assert_eq!(f.kind, "import_svg");
        assert_eq!(f.args.get("asset").map(String::as_str), Some("badge.svg"));
        assert_eq!(f.args.get("loops").map(String::as_str), Some("2"));
        assert!(f.args.get("sha256").is_some_and(|s| s.len() == 64));
        // A box with a square hole: six outer faces plus four hole walls.
        let body = ctx.bodies.iter().find(|b| b.name == "plate").unwrap();
        assert!(body.visible);
        assert_eq!(ctx.features.iter().filter(|f| f.kind == "extrude").count(), 1);
        let _ = body;
    }

    #[test]
    fn imported_loops_carry_names_that_trace_back_to_the_file() {
        let ctx = run_with_assets(
            "s = import_svg(asset(\"badge.svg\"))\n",
            &[("badge.svg", BADGE_SVG)],
        );
        assert!(errors(&ctx).is_empty(), "{:?}", errors(&ctx));
        // The names are in the sketch the feature produced; check them
        // through a second evaluation that reads `s.names`.
        let ctx2 = run_with_assets(
            r#"
s = import_svg(asset("badge.svg"), name="art")
assert_eq(s.names[0], "art/badge/path[0]/loop[0]")
assert_eq(s.names[1], "art/badge/path[0]/loop[1]")
assert_eq(s.names[2], "art/dot/loop[0]")
b = extrude(s, 1)
"#,
            &[("badge.svg", BADGE_SVG)],
        );
        assert!(errors(&ctx2).is_empty(), "{:?}", errors(&ctx2));
        let _ = ctx;
    }

    #[test]
    fn a_missing_asset_names_the_ones_that_exist() {
        let ctx = run_with_assets("a = asset(\"nope.svg\")\n", &[("badge.svg", BADGE_SVG)]);
        let errs = errors(&ctx);
        assert_eq!(errs.len(), 1, "{errs:?}");
        assert!(errs[0].contains("\"nope.svg\"") && errs[0].contains("badge.svg"), "{errs:?}");
        // With no assets at all the message is still a diagnostic, not a panic.
        let ctx = run("a = asset(\"x.svg\")\n");
        assert!(errors(&ctx)[0].contains("available: none"), "{:?}", errors(&ctx));
    }

    #[test]
    fn a_hash_assertion_warns_but_does_not_fail() {
        let ctx = run_with_assets(
            "a = asset(\"badge.svg\", sha256=\"deadbeef\")\ns = import_svg(a)\nextrude(s, 1)\n",
            &[("badge.svg", BADGE_SVG)],
        );
        assert!(errors(&ctx).is_empty(), "{:?}", errors(&ctx));
        assert!(
            ctx.diagnostics.iter().any(|d| d.message.contains("has changed since")),
            "{:?}",
            ctx.diagnostics.iter().map(|d| &d.message).collect::<Vec<_>>()
        );
    }

    #[test]
    fn a_dxf_asset_imports_with_its_units_and_joins() {
        // Four unconnected inch-unit lines forming a 2 x 2 inch square.
        let dxf = b"0\nSECTION\n2\nHEADER\n9\n$INSUNITS\n70\n1\n0\nENDSEC\n\
0\nSECTION\n2\nENTITIES\n\
0\nLINE\n8\nA-WALL\n10\n0\n20\n0\n11\n2\n21\n0\n\
0\nLINE\n8\nA-WALL\n10\n2\n20\n0\n11\n2\n21\n2\n\
0\nLINE\n8\nA-WALL\n10\n2\n20\n2\n11\n0\n21\n2\n\
0\nLINE\n8\nA-WALL\n10\n0\n20\n2\n11\n0\n21\n0\n\
0\nENDSEC\n0\nEOF\n";
        let ctx = run_with_assets(
            r#"
plan = asset("plan.dxf")
assert_eq(plan.kind, "dxf")
walls = import_dxf(plan, layers=["A-WALL"], name="walls")
assert_eq(type(walls), "Drawing2D")
assert_eq(walls.units, "inch")
assert_eq(walls.unit_source, "$INSUNITS")
assert_eq(walls.layer_names, ["A-WALL"])
assert_eq(walls.layers(), [("A-WALL", 4, (255, 255, 255))])
s = walls.sketch()
assert_eq(s.loops, 1)
assert_eq(s.names, ["walls/A-WALL/loop[0]"])
# A Drawing2D stands in for a Sketch: the old `extrude(import_dxf(...))`.
slab = extrude(walls, 10, name="slab")
bb = slab.bbox()
assert_true(bb[1][0] - bb[0][0] > 50.5 and bb[1][0] - bb[0][0] < 51.5, msg="2 inches is 50.8 mm")
# Too tight a tolerance and nothing joins; closed=True then has nothing.
assert_eq(walls.sketch(tolerance=1e-9).loops, 1)
"#,
            &[("plan.dxf", dxf)],
        );
        assert!(errors(&ctx).is_empty(), "{:?}", errors(&ctx));
        let f = ctx.feature(&"walls".into()).unwrap();
        assert_eq!(f.kind, "import_dxf");
        assert_eq!(f.args.get("unit").map(String::as_str), Some("inch"));
        assert_eq!(f.args.get("layers").map(String::as_str), Some("A-WALL"));
        assert_eq!(f.args.get("entities").map(String::as_str), Some("4"));
        assert_eq!(f.args.get("sketch[0].loops").map(String::as_str), Some("1"));
        assert_eq!(f.args.get("sketch[1].loops").map(String::as_str), Some("1"));
    }

    /// `Body.volume()` and `Body.area()` measure the tessellation, so they are
    /// exact on a polyhedron and carry the chord error on a curved one. Both
    /// facts matter to a script that compares two bodies, which is what the
    /// Gridfinity baseplate's drop test does.
    #[test]
    fn volume_and_area_measure_the_body() {
        let ctx = run(r#"
b = box(10, 20, 3, name="b")
assert_true(abs(b.volume() - 600.0) < 1e-9, msg="a box is exact")
assert_true(abs(b.area() - 2 * (200.0 + 30.0 + 60.0)) < 1e-9, msg="six faces")

# A cut removes exactly what it removes.
slab = box(20, 20, 4, name="slab")
hole = cylinder(3.0, 10, at=(10, 10, -1), name="hole")
holed = subtract(slab, hole, name="holed")
bore = 1600.0 - holed.volume()
# The mesh is inscribed in the cylinder, so the bore comes out a little small:
# at tolerance 0.05 a 3 mm circle is 32 chords, the bore wall is a strip on
# them, and a 32-gon has 0.9936 of its circle's area.
assert_true(bore < 4 * 3.141592653589793 * 9.0, msg="inscribed, so under the true bore")
assert_true(bore > 0.99 * 4 * 3.141592653589793 * 9.0, msg="within the chord error")

# Subtracting a body that is entirely inside a void removes nothing: the
# containment test the baseplate example is built on. The two volumes agree
# to what f32 mesh positions can promise, not bit for bit: an ulp at 20 mm is
# 1.9e-6 mm, times the body's 1139 mm^2 of surface is 2e-3 mm^3, and the peg
# this guards against is 75 mm^3.
peg = cylinder(2.0, 6, at=(10, 10, -0.5), name="peg")
again = subtract(holed, peg, name="again")
assert_true(abs(again.volume() - (1600.0 - bore)) < 5e-3, msg="nothing to remove")
"#);
        assert!(errors(&ctx).is_empty(), "{:?}", errors(&ctx));
    }

    #[test]
    fn text_makes_a_sketch_that_extrudes() {
        let ctx = run(r#"
label = text("Kern", size=8, align="center", name="label")
assert_eq(label.loops, 5)
raised = extrude(label, 0.6, name="raised")
"#);
        assert!(errors(&ctx).is_empty(), "{:?}", errors(&ctx));
        let f = ctx.feature(&"label".into()).unwrap();
        assert_eq!(f.kind, "text");
        assert_eq!(f.args.get("font").map(String::as_str), Some("builtin"));
        assert_eq!(f.args.get("text").map(String::as_str), Some("Kern"));
        assert!(f.args.contains_key("cap_height"));
        assert!(ctx.bodies.iter().any(|b| b.name == "raised" && b.visible));
    }

    #[test]
    fn a_font_asset_is_used_and_a_bad_font_name_is_a_diagnostic() {
        let inter = cadpole_font::BUILTIN_SANS;
        let ctx = run_with_assets(
            "s = text(\"Hi\", font=asset(\"MyFont.ttf\"), size=10)\nextrude(s, 1)\n",
            &[("MyFont.ttf", inter)],
        );
        assert!(errors(&ctx).is_empty(), "{:?}", errors(&ctx));
        assert_eq!(ctx.features[0].args.get("font").map(String::as_str), Some("MyFont.ttf"));

        let ctx = run("text(\"Hi\", font=\"Comic Sans\")\n");
        let errs = errors(&ctx);
        assert_eq!(errs.len(), 1, "{errs:?}");
        assert!(errs[0].contains("\"builtin\""), "{errs:?}");
    }

    #[test]
    fn importer_options_are_validated_with_useful_messages() {
        let ctx = run_with_assets(
            "import_svg(asset(\"badge.svg\"), fill_rule=\"maybe\")\n",
            &[("badge.svg", BADGE_SVG)],
        );
        assert!(errors(&ctx)[0].contains("fill_rule"), "{:?}", errors(&ctx));
        let ctx = run_with_assets(
            "import_svg(asset(\"badge.svg\"), align=\"topleft\")\n",
            &[("badge.svg", BADGE_SVG)],
        );
        assert!(errors(&ctx)[0].contains("align"), "{:?}", errors(&ctx));
    }
    #[test]
    fn material_preset_is_applied_first_and_overridden() {
        let ctx = run(r#"
b = box(10, 10, 10, name="b")
b.material(preset="glass", opacity=0.4)
c = box(10, 10, 10, at=(20, 0, 0), name="c")
c.material(preset="wood_oak", scale=12, label="oak")
"#);
        assert!(errors(&ctx).is_empty(), "{:?}", errors(&ctx));
        let m = &ctx.bodies.iter().find(|b| b.name == "b").unwrap().material;
        // The kwarg wins, the rest of the preset stays.
        assert_eq!(m.opacity, 0.4);
        assert_eq!(m.ior, 1.5);
        assert_eq!(m.roughness, 0.05);
        assert_eq!(m.name, "glass");
        let w = &ctx.bodies.iter().find(|b| b.name == "c").unwrap().material;
        assert_eq!(w.texture, "wood");
        assert_eq!(w.texture_scale, 12.0);
        assert_eq!(w.name, "oak");
    }

    /// `shadow=False` is material state, so it goes wherever the
    /// material goes — through a transform, a boolean (the first operand's,
    /// as every other field), `paint` and `Body.material` — and a body
    /// nobody flagged casts shadows.
    #[test]
    fn a_guide_body_keeps_its_shadow_flag_through_paint_transform_and_boolean() {
        let ctx = run(r##"
b = box(10, 10, 10, name="b")
b.material(color="#ffcc00", opacity=0.3, shadow=False)
t = translate(b, (20, 0, 0), name="t")
c = box(4, 4, 4, at=(22, 3, 3), name="c")
u = union(t, c, name="u")
p = paint(box(10, 10, 10, at=(40, 0, 0), name="p"), shadow=False)
q = paint(box(10, 10, 10, at=(60, 0, 0), name="q"), color="#ff0000", shadow=False)
plain = box(10, 10, 10, at=(80, 0, 0), name="plain")
back = paint(box(10, 10, 10, at=(100, 0, 0), name="back"), shadow=False)
back.material(shadow=True)
glass = box(10, 10, 10, at=(120, 0, 0), name="glass")
glass.material(preset="glass", shadow=False)
"##);
        assert!(errors(&ctx).is_empty(), "{:?}", errors(&ctx));
        let mut ctx = ctx;
        let model = ctx.to_model(true, 0.0);
        let m = |name: &str| &ctx.bodies.iter().find(|b| b.name == name).unwrap_or_else(|| panic!("no body {name}")).material;
        assert!(!m("t").shadow, "a transform carries the material, flag included");
        assert!(!m("u").shadow, "a boolean takes the first operand's material, flag included");
        assert!(!m("p").shadow, "paint(shadow=False) alone sets it");
        assert!(!m("q").shadow, "paint with a colour sets it too");
        assert!(m("plain").shadow, "on by default");
        assert!(m("back").shadow, "and switched back on");
        assert!(!m("glass").shadow && m("glass").name == "glass", "a preset is applied first, the flag after it");
        // The colour is still a feature and the flag never is: one paint
        // feature, `q`'s.
        let paints = cadpole_core::paint::paints(&ctx);
        assert_eq!(paints.len(), 1, "{paints:?}");
        assert_eq!(paints[0].hex, "#FF0000");
        // Round trip: a guide writes the field, a plain body does not.
        let pm = |name: &str| model.bodies.iter().find(|b| b.name == name).unwrap().material.clone().unwrap();
        assert_eq!(pm("p").shadow, Some(false));
        assert_eq!(pm("plain").shadow, None);

        let bad = run("b = box(10, 10, 10, name=\"b\")\npaint(b, faces=b.faces(normal=(0, 0, 1)), shadow=False)\n");
        assert!(errors(&bad)[0].contains("faces="), "the flag is the whole body's: {:?}", errors(&bad));
    }

    #[test]
    fn material_values_are_clamped_and_emissive_may_exceed_one() {
        let ctx = run(r#"
b = box(10, 10, 10, name="b")
b.material(opacity=2.5, ior=9, emissive=(4, 4, 3), roughness=-1)
"#);
        assert!(errors(&ctx).is_empty(), "{:?}", errors(&ctx));
        let m = &ctx.bodies[0].material;
        assert_eq!(m.opacity, 1.0);
        assert_eq!(m.ior, 2.5);
        assert_eq!(m.roughness, 0.0);
        assert_eq!(m.emissive, [4.0, 4.0, 3.0]);
    }

    #[test]
    fn unknown_preset_and_texture_names_are_diagnostics() {
        let ctx = run("box(10, 10, 10, name=\"b\").material(preset=\"brasss\")\n");
        let errs = errors(&ctx);
        assert_eq!(errs.len(), 1, "{errs:?}");
        assert!(errs[0].contains("unknown material preset \"brasss\""), "{errs:?}");
        assert!(errs[0].contains("\"brass\""), "the error lists the valid names: {errs:?}");
        // The body keeps its default material.
        assert_eq!(ctx.bodies[0].material, cadpole_core::context::Material::default());

        let ctx = run("box(10, 10, 10, name=\"b\").material(texture=\"tartan\")\n");
        let errs = errors(&ctx);
        assert_eq!(errs.len(), 1, "{errs:?}");
        assert!(errs[0].contains("unknown texture \"tartan\""), "{errs:?}");
        assert!(errs[0].contains("\"checker\""), "the error lists the valid names: {errs:?}");
        assert_eq!(ctx.bodies[0].material.texture, "");
    }

    // ---- constrained sketches ----------------------------------------

    /// A rectangle the solver worked out and a rectangle written down are the
    /// same solid: same volume, same faces. Nothing downstream of the sketch
    /// can tell which way it was built, which is the whole point.
    #[test]
    fn a_rectangle_from_constraints_extrudes_like_rect() {
        let mut ctx = run(r#"
literal = extrude(sketch(XY).rect(40, 25), 5, name="literal")

s = constrained_sketch(XY, name="frame")
a = s.point(0, 0, name="a", fix=True)
# Deliberately wrong starting points: the dimensions decide, not these.
b = s.point(37, 2, name="b")
c = s.point(39, 24, name="c")
d = s.point(-2, 26, name="d")
s.line(a, b, name="bottom")
s.line(b, c, name="right")
s.line(c, d, name="top")
s.line(d, a, name="left")
s.constrain("horizontal", "bottom")
s.constrain("horizontal", "top")
s.constrain("vertical", "left")
s.constrain("vertical", "right")
s.constrain("distance", a, b, 40, name="width")
s.constrain("distance", b, c, 25, name="height")
assert_eq(s.dof(), 0)
assert_eq(s.sketch().loops, 1)
solved = extrude(s, 5, name="solved")
"#);
        assert!(errors(&ctx).is_empty(), "{:?}", errors(&ctx));
        // Fully constrained, so there is nothing to report at all.
        assert!(of_severity(&ctx, cadpole_core::context::Severity::Info).is_empty());
        assert!(of_severity(&ctx, cadpole_core::context::Severity::Warning).is_empty());
        let (lv, lf) = solid(&mut ctx, "literal");
        let (sv, sf) = solid(&mut ctx, "solved");
        assert!((sv - 40.0 * 25.0 * 5.0).abs() < 1e-6, "solved volume {sv}");
        assert!((lv - sv).abs() < 1e-6, "{lv} vs {sv}");
        assert_eq!((lf, sf), (6, 6));
    }

    /// The parametric part of a parametric sketch: a dimension driven by
    /// `param()` re-solves when the parameter changes, and everything measured
    /// from an edge follows the edge. Sweeping the width gives a family of
    /// correctly solved sketches, not one sketch and two wrong ones.
    #[test]
    fn a_param_dimension_moves_the_geometry_it_dimensions() {
        const PLATE: &str = r#"
w = param("width", 60, unit="mm", min=30, max=200)

def near(a, b):
    return a - b < 1e-6 and b - a < 1e-6

s = constrained_sketch(XY, name="plate_profile")
a = s.point(0, 0, name="a", fix=True)
b = s.point(50, 0, name="b")
c = s.point(50, 20, name="c")
d = s.point(0, 20, name="d")
s.line(a, b, name="bottom")
s.line(b, c, name="right")
s.line(c, d, name="top")
s.line(d, a, name="left")
s.constrain("horizontal", "bottom")
s.constrain("horizontal", "top")
s.constrain("vertical", "left")
s.constrain("vertical", "right")
s.constrain("distance", a, b, w, name="width")
s.constrain("distance", b, c, 20, name="height")

# A hole measured from the right-hand edge and the bottom edge.
hc = s.point(40, 10, name="hole_center")
hole = s.circle(hc, 3, name="hole")
s.constrain("distance", hc, "right", 8, name="hole from the right edge")
s.constrain("distance", hc, "bottom", 10, name="hole from the bottom")
s.constrain("radius", hole, 3, name="hole radius")

assert_eq(s.dof(), 0)
# The plate is `w` wide and the hole is 8 mm in from its right-hand edge,
# wherever that edge ended up.
assert_true(near(s.at(b)[0], w), msg="the width dimension drives the corner")
assert_true(near(s.at(hc)[0], w - 8), msg="the hole follows the edge")
assert_true(near(s.at(hc)[1], 10))
plate = extrude(s, 4, name="plate")
"#;
        // A family of three, each solved from the same script.
        let mut volumes = Vec::new();
        for w in [40.0, 60.0, 120.0] {
            let mut ctx = run_with(PLATE, "width", w);
            assert!(errors(&ctx).is_empty(), "width {w}: {:?}", errors(&ctx));
            volumes.push((w, solid(&mut ctx, "plate").0));
        }
        // Only the plate grew; the hole is the same hole in each.
        for (w, v) in &volumes {
            let (w0, v0) = volumes[0];
            let grew = (w - w0) * 20.0 * 4.0;
            // A millionth of a cubic millimetre: the circle's tessellation
            // moves by that much when the plate around it changes size.
            assert!(
                (v - (v0 + grew)).abs() < 1e-3,
                "width {w}: volume {v}, expected {}",
                v0 + grew
            );
        }
        assert!(volumes[2].1 > volumes[0].1, "the widest plate is the biggest");
    }

    /// An extra constraint that agrees with the others is a warning that names
    /// it and what already implied it — and points at the line it is on.
    #[test]
    fn a_redundant_constraint_is_a_warning_that_names_it() {
        let ctx = run(r#"
s = constrained_sketch(XY, name="over")
a = s.point(0, 0, name="a", fix=True)
b = s.point(40, 0, name="b")
s.line(a, b, name="bottom")
s.constrain("horizontal", "bottom")
s.constrain("distance", a, b, 40, name="width")
s.constrain("distance", a, b, 40, name="width again")
s.solve()
"#);
        assert!(errors(&ctx).is_empty(), "a redundancy is not an error: {:?}", errors(&ctx));
        let warnings = of_severity(&ctx, cadpole_core::context::Severity::Warning);
        let (msg, line) = warnings
            .iter()
            .find(|(m, _)| m.starts_with("redundant constraint"))
            .unwrap_or_else(|| panic!("no redundancy reported: {warnings:?}"));
        assert!(msg.contains("redundant constraint: \"width again\""), "{msg}");
        assert!(msg.contains("already implied by \"width\""), "{msg}");
        // The span is the offending `constrain(...)` call, not the sketch.
        assert_eq!(*line, 8, "{msg}");
    }

    /// Constraints that cannot both hold are an error naming both sides and
    /// the size of the contradiction, reported on the constraint that caused
    /// it; the operation that needed the geometry fails too, rather than
    /// extruding a least-squares compromise.
    #[test]
    fn conflicting_constraints_name_both_sides_and_the_gap() {
        let ctx = run(r#"
s = constrained_sketch(XY, name="bad")
a = s.point(0, 0, name="a", fix=True)
b = s.point(40, 0, name="b")
c = s.point(40, 25, name="c")
d = s.point(0, 25, name="d")
s.line(a, b, name="bottom")
s.line(b, c, name="right")
s.line(c, d, name="top")
s.line(d, a, name="left")
s.constrain("horizontal", "bottom")
s.constrain("horizontal", "top")
s.constrain("vertical", "left")
s.constrain("vertical", "right")
s.constrain("distance", a, b, 40, name="width")
s.constrain("distance", d, c, 45, name="top width")
part = extrude(s, 3, name="part")
"#);
        let errs = of_severity(&ctx, cadpole_core::context::Severity::Error);
        let conflict = errs
            .iter()
            .find(|(m, _)| m.starts_with("conflicting constraints"))
            .unwrap_or_else(|| panic!("no conflict reported: {errs:?}"));
        assert!(conflict.0.contains("\"top width\""), "{}", conflict.0);
        assert!(conflict.0.contains("\"width\""), "{}", conflict.0);
        assert!(conflict.0.contains("by 5 mm"), "the contradiction in mm: {}", conflict.0);
        assert_eq!(conflict.1, 16, "the span is the constrain() call: {}", conflict.0);
        // And the extrude that tripped over it says so as well, so evaluation
        // stops instead of producing a shape that is neither 40 nor 45 wide.
        assert!(
            errs.iter().any(|(m, _)| m.contains("extrude") && m.contains("cannot be solved")),
            "{errs:?}"
        );
        assert!(!ctx.bodies.iter().any(|b| b.name == "part"));
    }

    /// Under-constrained is how modelling starts, so it is allowed: the sketch
    /// still solves and extrudes, and the leftover freedom is reported as info
    /// naming the entities that still have it.
    #[test]
    fn an_under_constrained_sketch_evaluates_and_reports_its_dof() {
        let ctx = run(r#"
s = constrained_sketch(XY, name="loose")
a = s.point(0, 0, name="a", fix=True)
b = s.point(40, 0, name="b")
c = s.point(40, 25, name="c")
d = s.point(0, 25, name="d")
s.line(a, b, name="bottom")
s.line(b, c, name="right")
s.line(c, d, name="top")
s.line(d, a, name="left")
s.constrain("horizontal", "bottom")
s.constrain("horizontal", "top")
s.constrain("vertical", "left")
s.constrain("vertical", "right")
s.constrain("distance", a, b, 40, name="width")
# No height: the rectangle is a rectangle, but not yet a particular one.
assert_eq(s.dof(), 1)
part = extrude(s, 3, name="part")
"#);
        assert!(errors(&ctx).is_empty(), "{:?}", errors(&ctx));
        let info = of_severity(&ctx, cadpole_core::context::Severity::Info);
        let (msg, _) = info
            .iter()
            .find(|(m, _)| m.contains("under-constrained"))
            .unwrap_or_else(|| panic!("no DoF report: {info:?}"));
        assert!(msg.contains("1 degree of freedom"), "{msg}");
        assert!(msg.contains("free: "), "{msg}");
        assert!(ctx.bodies.iter().any(|b| b.name == "part" && b.visible));

        // The extreme of the same case: no constraints at all is a sketch that
        // is exactly the coordinates the script typed, every one of them free.
        let mut ctx = run(r#"
s = constrained_sketch(XY, name="free")
a = s.point(0, 0, name="a")
b = s.point(10, 0, name="b")
c = s.point(10, 6, name="c")
s.line(a, b, name="one")
s.line(b, c, name="two")
s.line(c, a, name="three")
assert_eq(s.dof(), 6)
part = extrude(s, 2, name="triangle")
"#);
        assert!(errors(&ctx).is_empty(), "{:?}", errors(&ctx));
        let (v, faces) = solid(&mut ctx, "triangle");
        assert_eq!(faces, 5);
        assert!((v - 10.0 * 6.0 / 2.0 * 2.0).abs() < 1e-9, "volume {v}");
    }

    /// Construction geometry shapes the solve and never becomes material: the
    /// centreline here decides where the hole sits and contributes no loop, no
    /// face and no volume.
    #[test]
    fn construction_geometry_shapes_the_solve_but_not_the_solid() {
        let mut ctx = run(r#"
def near(a, b):
    return a - b < 1e-6 and b - a < 1e-6

s = constrained_sketch(XY, name="plate_profile")
a = s.point(0, 0, name="a", fix=True)
b = s.point(40, 0, name="b")
c = s.point(40, 20, name="c")
d = s.point(0, 20, name="d")
s.line(a, b, name="bottom")
s.line(b, c, name="right")
s.line(c, d, name="top")
s.line(d, a, name="left")
s.constrain("horizontal", "bottom")
s.constrain("horizontal", "top")
s.constrain("vertical", "left")
s.constrain("vertical", "right")
s.constrain("distance", a, b, 40, name="width")
s.constrain("distance", b, c, 20, name="height")

# A centreline across the plate. It is construction geometry, so it is part
# of the system and not part of the profile.
ml = s.point(0, 9, name="ml", construction=True)
mr = s.point(40, 11, name="mr", construction=True)
axis = s.line(ml, mr, name="centreline", construction=True)
s.constrain("midpoint", ml, "left")
s.constrain("midpoint", mr, "right")

hc = s.point(12, 4, name="hole_center")
hole = s.circle(hc, 3, name="hole")
s.constrain("on_curve", hc, axis, name="hole on the centreline")
s.constrain("distance", hc, "left", 12, name="hole from the left edge")
s.constrain("radius", hole, 3, name="hole radius")

assert_eq(s.dof(), 0)
# Two loops: the outline and the hole. The centreline is neither.
assert_eq(s.sketch().loops, 2)
assert_true(near(s.at(hc)[1], 10), msg="the centreline placed the hole")
plate = extrude(s, 4, name="plate")
"#);
        assert!(errors(&ctx).is_empty(), "{:?}", errors(&ctx));
        let (v, faces) = solid(&mut ctx, "plate");
        // Six faces for the plate and two for the (two-arc) hole. A centreline
        // that became geometry would show up here.
        assert_eq!(faces, 8);
        let expected = 40.0 * 20.0 * 4.0 - std::f64::consts::PI * 9.0 * 4.0;
        assert!((v - expected).abs() < 2.0, "volume {v}, expected about {expected}");
    }

    /// The parts of the surface a script is most likely to get wrong, each
    /// answered with a message that says what to write instead.
    #[test]
    fn constrained_sketch_mistakes_are_diagnostics_that_teach() {
        let ctx = run("s = constrained_sketch(XY)\ns.point(0, 0, name=\"a\")\ns.constrain(\"perpendicualr\", \"a\")\n");
        assert!(errors(&ctx)[0].contains("unknown constraint \"perpendicualr\""), "{:?}", errors(&ctx));
        assert!(errors(&ctx)[0].contains("perpendicular"), "it lists the kinds: {:?}", errors(&ctx));

        let ctx = run("s = constrained_sketch(XY)\ns.point(0, 0, name=\"a\")\ns.point(1, 1, name=\"a\")\n");
        assert!(errors(&ctx)[0].contains("name \"a\" used twice"), "{:?}", errors(&ctx));

        let ctx = run("s = constrained_sketch(XY)\ns.point(0, 0, name=\"a\")\ns.constrain(\"horizontal\", \"a\")\n");
        assert!(errors(&ctx)[0].contains("must be a line"), "{:?}", errors(&ctx));

        let ctx = run("s = constrained_sketch(XY)\ns.constrain(\"fix\", \"nope\")\n");
        assert!(errors(&ctx)[0].contains("no entity named \"nope\""), "{:?}", errors(&ctx));

        // An unclosed chain cannot bound a face, so it becomes a construction
        // path — with a warning saying how far from closing it was.
        let ctx = run(r#"
s = constrained_sketch(XY, name="open")
a = s.point(0, 0, name="a", fix=True)
b = s.point(10, 0, name="b")
c = s.point(10, 10, name="c")
s.line(a, b, name="one")
s.line(b, c, name="two")
s.constrain("horizontal", "one")
s.constrain("vertical", "two")
s.constrain("distance", a, b, 10, name="run")
s.constrain("distance", b, c, 10, name="rise")
assert_eq(s.sketch().loops, 0)
assert_eq(s.sketch().paths, 1)
"#);
        assert!(errors(&ctx).is_empty(), "{:?}", errors(&ctx));
        let warnings = of_severity(&ctx, cadpole_core::context::Severity::Warning);
        assert!(
            warnings.iter().any(|(m, _)| m.contains("does not close") && m.contains("construction path")),
            "{warnings:?}"
        );
    }

    /// A constrained sketch survives being defined in a library: the value is
    /// a handle into the evaluation store, so it still works after the
    /// library's module is frozen, and the feature joins the same tree.
    #[test]
    fn a_library_can_build_a_constrained_sketch() {
        let mut ctx = evaluate(EvalInput {
            scripts: vec![
                ScriptFile {
                    path: "main.star".into(),
                    source: "load(\"lib.star\", \"square\")\nb = extrude(square(12), 3, name=\"tile\")\n".into(),
                },
                ScriptFile {
                    path: "lib.star".into(),
                    source: r#"
def square(side):
    s = constrained_sketch(XY)
    a = s.point(0, 0, name="a", fix=True)
    b = s.point(side, 1, name="b")
    c = s.point(side, side, name="c")
    d = s.point(1, side, name="d")
    s.line(a, b, name="bottom")
    s.line(b, c, name="right")
    s.line(c, d, name="top")
    s.line(d, a, name="left")
    s.constrain("horizontal", "bottom")
    s.constrain("vertical", "right")
    s.constrain("perpendicular", "top", "right")
    s.constrain("perpendicular", "left", "bottom")
    s.constrain("distance", a, b, side, name="side")
    s.constrain("equal", "bottom", "right", name="square")
    return s
"#
                    .into(),
                },
            ],
            ..Default::default()
        });
        assert!(errors(&ctx).is_empty(), "{:?}", errors(&ctx));
        let (v, faces) = solid(&mut ctx, "tile");
        assert!((v - 12.0 * 12.0 * 3.0).abs() < 1e-6, "volume {v}");
        assert_eq!(faces, 6);
    }

    /// The worked example, swept. `constrained_bracket.star` asserts its own
    /// dimensions — the plate is `width` wide, the right-hand hole is
    /// `edge_margin` in from the right-hand edge — so evaluating it at several
    /// widths is a family of correctly solved sketches, and only the plate
    /// grows: the holes keep their size and their distance from the ends.
    #[test]
    fn sweeping_a_dimension_of_the_worked_example_stays_true() {
        let dir = std::env::var("CADPOLE_EXAMPLES").unwrap_or_else(|_| "examples".into());
        let src = std::fs::read_to_string(std::path::Path::new(&dir).join("constrained_bracket.star"))
            .expect("the worked example");
        let mut volumes = Vec::new();
        for w in [50.0, 80.0, 150.0] {
            let mut ctx = run_with(&src, "width", w);
            assert!(errors(&ctx).is_empty(), "width {w}: {:?}", errors(&ctx));
            volumes.push((w, solid(&mut ctx, "plate").0));
        }
        for (w, v) in &volumes {
            let (w0, v0) = volumes[0];
            let grew = (w - w0) * 40.0 * 6.0; // depth × thickness, both default
            assert!((v - (v0 + grew)).abs() < 1e-3, "width {w}: volume {v}");
        }

        // The other example is the harder solve — a fillet held on by two
        // tangencies — so sweep its radius too. The fillet is a *re-entrant*
        // corner, so rounding it off adds `(1 − π/4) r²` of material.
        let src = std::fs::read_to_string(std::path::Path::new(&dir).join("constrained_lprofile.star"))
            .expect("the L-profile example");
        for r in [2.0, 6.0, 12.0] {
            let mut ctx = run_with(&src, "fillet", r);
            assert!(errors(&ctx).is_empty(), "fillet {r}: {:?}", errors(&ctx));
            let (v, _) = solid(&mut ctx, "angle");
            let area = 60.0 * 8.0 + (45.0 - 8.0) * 8.0 + (1.0 - std::f64::consts::FRAC_PI_4) * r * r;
            let expected = area * 20.0;
            assert!((v - expected).abs() < 0.002 * expected, "fillet {r}: volume {v}, expected {expected}");
        }
    }

    /// Arcs, tangency and coincidence: the slot that is tedious to write as
    /// coordinates. Its ends are semicircles tangent to both flanks, and the
    /// only numbers in it are the two dimensions a drawing would carry.
    #[test]
    fn a_slot_from_arcs_and_tangency_closes_into_one_loop() {
        let mut ctx = run(r#"
def near(a, b):
    return a - b < 1e-6 and b - a < 1e-6

length = 30.0   # between the arc centres
width = 8.0     # across the flanks

s = constrained_sketch(XY, name="slot_profile")
left_c = s.point(0, 0, name="left_center", fix=True)
right_c = s.point(28, 1, name="right_center")

bl = s.point(0, -4, name="bl")
br = s.point(28, -4, name="br")
tr = s.point(28, 4, name="tr")
tl = s.point(0, 4, name="tl")

bottom = s.line(bl, br, name="bottom")
top = s.line(tr, tl, name="top")
right = s.arc(right_c, width / 2, start=-90, end=90, name="right_end")
left = s.arc(left_c, width / 2, start=90, end=270, name="left_end")

s.constrain("horizontal", bottom)
s.constrain("distance", left_c, right_c, length, name="centres")
s.constrain("coincident", bl, left.end)
s.constrain("coincident", br, right.start)
s.constrain("coincident", tr, right.end)
s.constrain("coincident", tl, left.start)
s.constrain("tangent", bottom, right, br, name="bottom meets the right end")
s.constrain("tangent", bottom, left, bl, name="bottom meets the left end")
s.constrain("tangent", top, right, tr, name="top meets the right end")
s.constrain("tangent", top, left, tl, name="top meets the left end")
s.constrain("radius", right, width / 2, name="width")
s.constrain("equal", left, right, name="both ends match")

assert_eq(s.sketch().loops, 1)
assert_true(near(s.at(right_c)[0], length), msg="the centres are `length` apart")
assert_true(near(s.radius_of(left), width / 2))
slot = extrude(s, 3, name="slot")
"#);
        assert!(errors(&ctx).is_empty(), "{:?}", errors(&ctx));
        let (v, faces) = solid(&mut ctx, "slot");
        // Two flanks, two ends (each arc is split in two so the three-point
        // circle through it stays well conditioned), a top and a bottom.
        assert!(faces >= 6, "{faces} faces");
        let expected = (30.0 * 8.0 + std::f64::consts::PI * 16.0) * 3.0;
        // Within a tenth of a percent: the ends are tessellated, not exact.
        assert!((v - expected).abs() < 0.002 * expected, "volume {v}, expected about {expected}");
    }

    // -----------------------------------------------------------------------
    // The sandbox (docs/security.md, "Starlark sandbox")
    // -----------------------------------------------------------------------

    fn run_limited(src: &str, limits: Limits) -> Context {
        evaluate(EvalInput {
            scripts: vec![ScriptFile { path: "main.star".into(), source: src.into() }],
            limits,
            ..Default::default()
        })
    }

    /// The headline claim of docs/security.md's sandbox section: a runaway
    /// script is a diagnostic, not a hung server. If this test ever hangs,
    /// the deadline is not being checked and the claim is false.
    #[test]
    fn a_runaway_loop_is_stopped_by_the_deadline_with_a_diagnostic() {
        let start = std::time::Instant::now();
        let ctx = run_limited(
            "total = 0\nfor i in range(1000000000):\n    total = total + i\nbox(total, 1, 1)\n",
            Limits { deadline_ms: 250, ..Limits::structural() },
        );
        let elapsed = start.elapsed();
        let errs = errors(&ctx);
        assert_eq!(errs.len(), 1, "exactly one diagnostic, not a panic: {errs:?}");
        assert!(errs[0].contains("250 ms budget"), "the diagnostic says what was exceeded: {errs:?}");
        assert!(errs[0].contains("cadpole serve --eval-deadline-ms"), "and where the budget is set: {errs:?}");
        assert!(ctx.bodies.is_empty(), "nothing after the loop ran");
        // starlark polls the cancellation check once every 1000 ticks, so the
        // overshoot is bounded by a thousand loop iterations.
        assert!(elapsed < std::time::Duration::from_secs(20), "the deadline must actually bound CPU, took {elapsed:?}");
    }

    /// The same runaway, bounded deterministically instead of by a clock. This
    /// is what a test on a loaded machine (or wasm, where there is no clock at
    /// all) can rely on.
    #[test]
    fn a_runaway_loop_is_stopped_by_the_tick_cap() {
        let ctx = run_limited(
            "total = 0\nfor i in range(1000000000):\n    total = total + i\n",
            Limits { deadline_ms: 0, max_ticks: 50_000, ..Limits::structural() },
        );
        let errs = errors(&ctx);
        assert_eq!(errs.len(), 1, "{errs:?}");
        assert!(errs[0].to_lowercase().contains("tick"), "the tick limit names itself: {errs:?}");
    }

    #[test]
    fn a_script_within_the_budget_is_untouched_by_it() {
        let ctx = run_limited(
            "total = 0\nfor i in range(1000):\n    total = total + i\nbox(10, 10, 10, name=\"b\")\n",
            Limits { deadline_ms: 30_000, ..Limits::structural() },
        );
        assert!(errors(&ctx).is_empty(), "{:?}", errors(&ctx));
        assert_eq!(ctx.bodies.len(), 1);
    }

    #[test]
    fn runaway_recursion_hits_the_callstack_limit() {
        let ctx = run_limited(
            "def f(n):\n    return f(n + 1)\n\nf(0)\n",
            Limits { max_callstack: 32, ..Limits::structural() },
        );
        let errs = errors(&ctx);
        assert_eq!(errs.len(), 1, "a stack overflow must be a diagnostic, never a crash: {errs:?}");
        assert!(!errs[0].is_empty());
    }

    #[test]
    fn a_greedy_allocation_hits_the_heap_cap() {
        let ctx = run_limited(
            "xs = []\nfor i in range(10000000):\n    xs = xs + [i, i, i, i]\n",
            Limits { max_heap_bytes: 4 * 1024 * 1024, max_ticks: 0, deadline_ms: 0, ..Limits::structural() },
        );
        let errs = errors(&ctx);
        assert_eq!(errs.len(), 1, "{errs:?}");
        assert!(errs[0].to_lowercase().contains("memory") || errs[0].to_lowercase().contains("heap"), "{errs:?}");
        // It says the cap it ran under and how a native run raises it.
        assert!(errs[0].contains("filled its 4 MB Starlark heap") && errs[0].contains("--max-heap-mb"), "{errs:?}");
    }

    /// `print` must not reach the process's stdout: the caller is a browser or
    /// an assistant, and neither can see it. It becomes an info diagnostic,
    /// capped so a print in a loop cannot fill memory.
    #[test]
    fn print_becomes_a_capped_diagnostic() {
        let ctx = run_limited(
            "for i in range(10):\n    print(\"hello\", i)\n",
            Limits { max_print_lines: 3, ..Limits::structural() },
        );
        assert!(errors(&ctx).is_empty(), "{:?}", errors(&ctx));
        let infos: Vec<&str> = ctx
            .diagnostics
            .iter()
            .filter(|d| d.severity == cadpole_core::context::Severity::Info)
            .map(|d| d.message.as_str())
            .collect();
        assert!(infos[0].starts_with("print: hello 0"), "{infos:?}");
        assert_eq!(infos.len(), 4, "three lines plus the note that the rest were dropped: {infos:?}");
        assert!(infos.last().unwrap().contains("the rest are dropped"), "{infos:?}");
    }

    /// The standard globals have no I/O of any kind. This is a property of
    /// `Globals::standard()` rather than of our code, which is exactly why it
    /// is worth a test: enabling `LibraryExtension::Debug` one day would break
    /// it silently.
    #[test]
    fn there_is_no_way_out_of_the_sandbox() {
        for attempt in ["open(\"/etc/passwd\")", "import os", "__import__(\"os\")", "eval(\"1\")", "exec(\"1\")", "getattr(1, \"x\")"] {
            let ctx = run(attempt);
            assert!(!errors(&ctx).is_empty(), "{attempt:?} must not work");
        }
    }

    // ---------------------------------------------------- fields and fillets

    #[test]
    fn a_field_is_a_value_until_it_is_meshed() {
        let ctx = run(r#"
f = field.box(20, 20, 6, round=1.5)
assert_eq(str(type(f)), "Field")
lo, hi = f.bounds
assert_eq(lo[0], -10.0)
assert_eq(hi[2], 3.0)
b = mesh(f, resolution=0.5, name="slab")
assert_eq(b.kind, "mesh")
"#);
        assert!(errors(&ctx).is_empty(), "{:?}", errors(&ctx));
        // One feature, of kind `mesh`, recording how it was built so it can
        // be re-meshed at another resolution.
        let f = ctx.feature(&"slab".into()).unwrap();
        assert_eq!(f.kind, "mesh");
        assert_eq!(f.args.get("resolution").map(String::as_str), Some("0.5"));
        assert!(f.args.get("field").is_some_and(|v| v.contains("box")));
        assert!(f.args.get("triangles").is_some_and(|v| v.parse::<usize>().unwrap() > 100));
        let body = ctx.bodies.iter().find(|b| b.name == "slab").unwrap();
        assert_eq!(body.kind(), "mesh");
    }

    #[test]
    fn a_meshed_field_has_the_volume_the_field_says_it_has() {
        let mut ctx = run(r#"
f = field.sphere(8)
b = mesh(f, resolution=0.3, simplify=False, name="ball")
"#);
        assert!(errors(&ctx).is_empty(), "{:?}", errors(&ctx));
        let id = ctx.bodies.iter().find(|b| b.name == "ball").unwrap().id;
        let (mesh, _) = ctx.body_mesh(id).unwrap();
        let want = 4.0 / 3.0 * std::f64::consts::PI * 512.0;
        let got = mesh.volume();
        assert!((got - want).abs() / want < 0.01, "{got} vs {want}");
        assert!(mesh.welded(1e-4).is_watertight());
    }

    #[test]
    fn an_unbounded_field_is_refused_with_advice() {
        let ctx = run("b = mesh(field.half_space(normal=(0,0,1)), resolution=1)\n");
        let errs = errors(&ctx);
        assert_eq!(errs.len(), 1, "{errs:?}");
        assert!(errs[0].contains("unbounded"), "{errs:?}");
        assert!(errs[0].contains("intersect"), "the message says what to do: {errs:?}");
    }

    #[test]
    fn fillet_rounds_the_selected_edges_and_says_it_is_approximate() {
        let ctx = run(r#"
part = box(30, 20, 10, center=True, name="part")
top = [f for f in part.faces(normal=(0, 0, 1)) if f.normal[2] > 0.99][0]
rounded = fillet(part, edges=top.edges, radius=2, resolution=0.4, kernel="field", name="rounded")
assert_eq(rounded.kind, "mesh")
"#);
        assert!(errors(&ctx).is_empty(), "{:?}", errors(&ctx));
        let f = ctx.feature(&"rounded".into()).unwrap();
        assert_eq!(f.kind, "fillet");
        assert_eq!(f.args.get("radius").map(String::as_str), Some("2"));
        assert_eq!(f.args.get("edges").map(String::as_str), Some("4"));
        let info: Vec<&str> = ctx.diagnostics.iter().map(|d| d.message.as_str()).collect();
        assert!(
            info.iter().any(|m| m.contains("fillet of 4 edges") && m.contains("mesh body")),
            "the diagnostic states the approximation: {info:?}"
        );
        assert!(
            info.iter().any(|m| m.contains("Measured surface error")),
            "and reports the error it measured: {info:?}"
        );
        // The original body is consumed, the new one is a mesh body.
        assert!(!ctx.bodies.iter().find(|b| b.name == "part").unwrap().visible);
        assert_eq!(ctx.bodies.iter().find(|b| b.name == "rounded").unwrap().kind(), "mesh");
    }

    #[test]
    fn a_filleted_body_still_takes_a_boolean() {
        let mut ctx = run(r#"
part = box(30, 20, 10, center=True, name="part")
rounded = fillet(part, radius=2, resolution=0.5, kernel="field", name="rounded")
hole = cylinder(3, 20, at=(0, 0, -5), name="hole")
out = subtract(rounded, hole, name="out")
"#);
        assert!(errors(&ctx).is_empty(), "{:?}", errors(&ctx));
        let id = ctx.bodies.iter().find(|b| b.name == "out").unwrap().id;
        let (mesh, _) = ctx.body_mesh(id).unwrap();
        assert!(mesh.welded(1e-4).is_watertight(), "the result of the boolean is closed");
        let v = mesh.volume();
        // The box, minus the twelve rounded edges, minus the bore.
        assert!(v > 4000.0 && v < 6000.0, "volume {v}");
    }

    /// `fillet` is exact by default: a B-rep body whose blends are
    /// cylinders named `<feature>/blend[<edge>]`, the neighbours kept under
    /// their names, and the feature recording the radius.
    #[test]
    fn fillet_is_exact_by_default_and_names_its_blends() {
        let mut ctx = run(r#"
part = box(30, 20, 10, center=True, name="part")
top = part.faces(normal=(0, 0, 1)).one()
rounded = fillet(part, edges=top.edges, radius=2, name="rounded")
assert_eq(rounded.kind, "brep")
"#);
        assert!(errors(&ctx).is_empty(), "{:?}", errors(&ctx));
        let f = ctx.feature(&"rounded".into()).unwrap();
        assert_eq!(f.kind, "fillet");
        assert_eq!(f.args.get("radius").map(String::as_str), Some("2"));
        let id = ctx.bodies.iter().find(|b| b.name == "rounded").unwrap().id;
        let (_, topo) = ctx.body_mesh(id).unwrap();
        let names: Vec<&str> = topo.faces.iter().map(|f| f.name.as_str()).collect();
        assert_eq!(names.iter().filter(|n| n.starts_with("rounded/blend[")).count(), 4, "{names:?}");
        assert!(names.contains(&"part/top") && names.contains(&"part/bottom"), "{names:?}");
        let kinds: Vec<(&str, &str)> = topo.faces.iter().map(|f| (f.name.as_str(), f.surface_kind)).collect();
        assert!(
            topo.faces.iter().filter(|f| f.name.starts_with("rounded/")).all(|f| matches!(f.surface, cadpole_core::SurfaceClass::Cylinder { .. })),
            "{kinds:?}"
        );
        // No diagnostic calls it approximate: nothing was.
        assert!(!ctx.diagnostics.iter().any(|d| d.message.contains("mesh body")), "{:?}", ctx.diagnostics);
    }

    /// The exact fillet's result is an ordinary B-rep: a later boolean takes
    /// it on the exact kernel and stays exact.
    #[test]
    fn a_filleted_brep_still_takes_an_exact_boolean() {
        let mut ctx = run(r#"
part = box(30, 20, 10, center=True, name="part")
rounded = fillet(part, radius=2, name="rounded")
hole = cylinder(3, 20, at=(0, 0, -5), name="hole")
out = subtract(rounded, hole, name="out")
assert_eq(out.kind, "brep")
"#);
        assert!(errors(&ctx).is_empty(), "{:?}", errors(&ctx));
        let id = ctx.bodies.iter().find(|b| b.name == "out").unwrap().id;
        let (mesh, topo) = ctx.body_mesh(id).unwrap();
        assert!(mesh.welded(1e-4).is_watertight(), "the result of the boolean is closed");
        assert_eq!(topo.faces.iter().filter(|f| f.name.starts_with("rounded/vertex[")).count(), 8);
        // The rounded box in closed form, less the bore, to the display mesh's chord error.
        let (w, d, h, r) = (30.0f64, 20.0f64, 10.0f64, 2.0f64);
        let (iw, id_, ih) = (w - 2.0 * r, d - 2.0 * r, h - 2.0 * r);
        let rounded = iw * id_ * ih
            + 2.0 * r * (iw * id_ + iw * ih + id_ * ih)
            + std::f64::consts::PI * r * r * (iw + id_ + ih)
            + 4.0 / 3.0 * std::f64::consts::PI * r * r * r;
        let expected = rounded - std::f64::consts::PI * 9.0 * h;
        let v = mesh.volume();
        assert!((v - expected).abs() < 0.005 * expected, "volume {v}, expected {expected}");
    }

    /// The exact path refuses by name, and its options are checked before
    /// anything is built.
    #[test]
    fn an_exact_fillet_that_does_not_fit_is_an_error_naming_the_edge_and_the_fix() {
        let ctx = run("plate = box(40, 30, 3, center=True, name=\"plate\")\nout = fillet(plate, radius=4, name=\"out\")\n");
        let e = errors(&ctx);
        assert_eq!(e.len(), 1, "{e:?}");
        assert!(e[0].starts_with("fillet: 4 mm along `plate/") && e[0].contains("is wider than the face"), "{}", e[0]);
        assert!(e[0].contains("The largest setback that fits there is about"), "{}", e[0]);
        let ctx = run("out = fillet(box(10, 10, 10, name=\"b\"), radius=1, resolution=0.3)\n");
        assert!(errors(&ctx)[0].contains("resolution= is the field path's"), "{:?}", errors(&ctx));
        let ctx = run("out = fillet(box(10, 10, 10, name=\"b\"), radius=1, kernel=\"mesh\")\n");
        assert!(errors(&ctx)[0].contains("kernel must be \"brep\""), "{:?}", errors(&ctx));
    }

    /// `kernel="field"` is the field path `fillet` runs on: the same
    /// selectors, a mesh body, the same diagnostics.
    #[test]
    fn chamfer_on_the_field_path_takes_the_same_selectors_as_fillet() {
        let ctx = run(r#"
part = box(20, 20, 8, center=True, name="part")
top = [f for f in part.faces(normal=(0, 0, 1)) if f.normal[2] > 0.99][0]
out = chamfer(part, edges=top.edges, distance=1.5, resolution=0.3, kernel="field", name="out")
assert_eq(out.kind, "mesh")
"#);
        assert!(errors(&ctx).is_empty(), "{:?}", errors(&ctx));
        assert_eq!(ctx.feature(&"out".into()).unwrap().kind, "chamfer");
        assert_eq!(
            ctx.feature(&"out".into()).unwrap().args.get("distance").map(String::as_str),
            Some("1.5")
        );
    }

    /// The names of a body's faces, sorted.
    fn face_names(ctx: &mut Context, name: &str) -> Vec<String> {
        let id = ctx.bodies.iter().find(|b| b.name == name).unwrap_or_else(|| panic!("no body named {name:?}")).id;
        let mut names: Vec<String> = ctx.body_mesh(id).unwrap().1.faces.iter().map(|f| f.name.clone()).collect();
        names.sort();
        names
    }

    /// `chamfer` is exact by default: a B-rep body, one planar face
    /// per edge named after it, the neighbours kept, the closed-form volume
    /// (four wedges less the four mitred corners).
    #[test]
    fn chamfer_is_exact_by_default_and_names_its_faces_after_the_edges() {
        let mut ctx = run(r#"
part = box(30, 20, 10, center=True, name="part")
top = part.faces(normal=(0, 0, 1)).one()
out = chamfer(part, 1, edges=top.edges, name="out")
assert_eq(out.kind, "brep")
assert_eq(len(out.faces(kind="plane")), 10)
assert_eq(len(out.faces(name="out/face[*]")), 4)
assert_eq(len(out.edges(name="out/face[*]|face[*]")), 4)
"#);
        assert!(errors(&ctx).is_empty(), "{:?}", errors(&ctx));
        let (v, faces) = solid(&mut ctx, "out");
        let expect = 30.0 * 20.0 * 10.0 - 0.5 * 2.0 * (30.0 + 20.0) + 4.0 / 3.0;
        assert!((v - expect).abs() <= 1e-6 * expect, "volume {v}, expected {expect}");
        assert_eq!(faces, 10);
        let names = face_names(&mut ctx, "out");
        for n in ["part/top", "part/bottom", "part/front", "part/back", "part/left", "part/right"] {
            assert!(names.contains(&n.to_owned()), "{names:?}");
        }
        for e in ["part/front|top", "part/back|top", "part/left|top", "part/right|top"] {
            assert!(names.contains(&format!("out/face[{e}]")), "{names:?}");
        }
        let f = ctx.feature(&"out".into()).unwrap();
        assert_eq!(f.kind, "chamfer");
        assert_eq!(f.args.get("distance").map(String::as_str), Some("1"));
        assert!(f.args.get("edges").unwrap().contains("part/front|top (1 along `part/front`, 1 along `part/top`)"), "{:?}", f.args);
        assert!(f.args.get("kind").is_none(), "an exact body records no kind: {:?}", f.args);
    }

    /// `d2` sets the second face's setback outright; `angle` sets it by the
    /// angle the flat leaves the first face at. The faces are the two in the
    /// order the edge name spells them: `part/front` then `part/top`.
    #[test]
    fn chamfer_d2_and_angle_set_the_second_setback() {
        let mut ctx = run(r#"
part = box(30, 20, 10, center=True, name="part")
a = chamfer(part, 1, edges="part/front|top", d2=2, name="a")
part2 = box(30, 20, 10, center=True, name="part2")
b = chamfer(part2, 2, edges="part2/front|top", angle=30, name="b")
"#);
        assert!(errors(&ctx).is_empty(), "{:?}", errors(&ctx));
        let (va, _) = solid(&mut ctx, "a");
        let ea = 6000.0 - 0.5 * 1.0 * 2.0 * 30.0;
        assert!((va - ea).abs() <= 1e-6 * ea, "volume {va}, expected {ea}");
        let (vb, _) = solid(&mut ctx, "b");
        let d2 = 2.0 * cadpole_core::fmath::tan(30f64.to_radians());
        let eb = 6000.0 - 0.5 * 2.0 * d2 * 30.0;
        assert!((vb - eb).abs() <= 1e-6 * eb, "volume {vb}, expected {eb}");
        assert_eq!(ctx.feature(&"a".into()).unwrap().args.get("d2").map(String::as_str), Some("2"));
        assert_eq!(ctx.feature(&"b".into()).unwrap().args.get("angle").map(String::as_str), Some("30"));
        let ctx = run(r#"
part = box(30, 20, 10, center=True, name="part")
a = chamfer(part, 1, edges="part/front|top", d2=2, angle=30)
"#);
        assert_eq!(errors(&ctx), ["chamfer: pass d2= or angle=, not both: each fixes the second setback"]);
    }

    /// The exact path never changes representation on its own: what
    /// it cannot build is an error that names the edge and the fix, and the
    /// fix that always exists is to ask for the field path.
    #[test]
    fn chamfer_refuses_by_name_and_offers_the_field_path() {
        let ctx = run(r#"
m = mesh(field.box(10, 10, 10), resolution=1, name="m")
out = chamfer(m, 1)
"#);
        assert_eq!(
            errors(&ctx),
            ["chamfer: `m` is a mesh body, which has no exact edges to chamfer; pass kernel=\"field\" to chamfer it on \
              the field path (the result is a mesh body)"]
        );
        // A hole's mouth: an inner loop of the plate's top.
        let ctx = run(r#"
plate = box(30, 30, 8, name="plate")
bore = cylinder(4, 10, at=(15, 15, 3), name="bore")
part = subtract(plate, bore, name="part")
mouth = part.edges(on_plane=XY.offset(8), max_length=26)
assert_eq(len(mouth), 2)
out = chamfer(part, 1, edges=mouth, name="sink")
"#);
        let errs = errors(&ctx);
        assert_eq!(errs.len(), 1, "{errs:?}");
        assert!(errs[0].starts_with("chamfer: edge `bore/side#"), "{errs:?}");
        assert!(
            errs[0].contains("is an inner loop of `plate/top`")
                && errs[0].ends_with("build it into the profile, or pass kernel=\"field\" for a mesh result"),
            "{errs:?}"
        );
        // The same call on the field path is the mesh chamfer.
        let ctx = run(r#"
plate = box(30, 30, 8, name="plate")
bore = cylinder(4, 10, at=(15, 15, 3), name="bore")
part = subtract(plate, bore, name="part")
out = chamfer(part, 1, edges=part.edges(on_plane=XY.offset(8), max_length=26), kernel="field", name="sink")
assert_eq(out.kind, "mesh")
"#);
        assert!(errors(&ctx).is_empty(), "{:?}", errors(&ctx));
        // An edge that dies into a face.
        let ctx = run(r#"
plate = box(40, 30, 4, name="plate")
flange = box(8, 36, 20, at=(36, -3, 2), name="flange")
l = union(plate, flange, name="l")
front = l.edges(name="plate/front|top")
assert_eq(len(front), 1)
out = chamfer(l, 1, edges=front)
"#);
        let errs = errors(&ctx);
        assert_eq!(errs.len(), 1, "{errs:?}");
        assert!(errs[0].starts_with("chamfer: edge `plate/front|top` dies into `flange/left` at (36.000, 0.000, 4.000): "), "{errs:?}");
        // The arguments the two paths do not share.
        let ctx = run("out = chamfer(box(10, 10, 10, name=\"b\"), 1, kernel=\"mesh\")\n");
        assert_eq!(errors(&ctx), ["chamfer: kernel must be \"brep\" (exact, the default) or \"field\", got \"mesh\""]);
        let ctx = run("out = chamfer(box(10, 10, 10, name=\"b\"), 1, resolution=0.3)\n");
        assert_eq!(errors(&ctx), ["chamfer: resolution= is the field path's (pass kernel=\"field\"); the exact path has no grid"]);
        let ctx = run("out = chamfer(box(10, 10, 10, name=\"b\"), 1, d2=2, kernel=\"field\")\n");
        assert_eq!(errors(&ctx), ["chamfer: d2= and angle= are the exact path's; the field path cuts the same distance along both faces"]);
    }

    /// A chamfer drawn in the profile and one cut by `chamfer()` agree:
    /// the compressor wheel's hub carries its nose chamfer in the loft
    /// profile, and `chamfer()` on the plain hub — a ring cutter on that
    /// rim — gives the same solid to the last part in 10⁶.
    ///
    /// The hub's *other* chamfer, on the 44 mm exducer rim, is one the
    /// exact chamfer refuses: the booleans run, and the solid they make is one
    /// truck cannot mesh. So the profile keeps it, which is what
    /// `examples/compressor_wheel.star` does and why its header says the
    /// chamfers are free.
    #[test]
    fn the_compressor_hubs_nose_chamfer_by_profile_and_by_cutter_agree() {
        let mut ctx = run(&format!(
            "{HUB_HEAD}
a = hub(True, \"by_profile\")
plain = hub(False, \"plain\")
b = chamfer(plain, c * 0.7, edges=plain.edges(on_plane=XY.offset(z_nose + 2.0)), name=\"by_cutter\")
assert_eq(b.kind, \"brep\")
assert_eq(len(b.faces(name=\"by_cutter/face[*]\")), 2)
assert_eq(len(b.faces(kind=\"cone\")), len(a.faces(kind=\"cone\")))
"
        ));
        assert!(errors(&ctx).is_empty(), "{:?}", errors(&ctx));
        let (va, fa) = solid(&mut ctx, "by_profile");
        let (vb, fb) = solid(&mut ctx, "by_cutter");
        assert_eq!(fa, fb, "the same faces: {fa} vs {fb}");

        // The two chamfer *surfaces*, compared exactly. This is the real
        // statement that the routes agree: the cone the loft's profile
        // segment sweeps and the cone the ring cutter left are the same
        // cone, to the last few bits, with no tessellation in between.
        let cone_at = |ctx: &mut Context, body: &str, z: f64| -> cadpole_core::SurfaceClass {
            let id = ctx.bodies.iter().find(|b| b.name == body).unwrap().id;
            let topo = ctx.body_mesh(id).unwrap().1.clone();
            topo.faces
                .iter()
                .filter(|f| matches!(f.surface, cadpole_core::SurfaceClass::Cone { .. }) && (f.centroid.z - z).abs() < 1.0)
                .map(|f| f.surface.clone())
                .next()
                .unwrap_or_else(|| panic!("no cone near z={z} on {body}"))
        };
        let (ca, cb) = (cone_at(&mut ctx, "by_profile", 43.6), cone_at(&mut ctx, "by_cutter", 43.6));
        let cadpole_core::SurfaceClass::Cone { apex: pa, axis: xa, half_angle: ha } = ca else { unreachable!() };
        let cadpole_core::SurfaceClass::Cone { apex: pb, axis: xb, half_angle: hb } = cb else { unreachable!() };
        assert!((pa - pb).length() < 1e-9, "apex {pa:?} vs {pb:?}");
        assert!((xa - xb).length() < 1e-9, "axis {xa:?} vs {xb:?}");
        assert!((ha - hb).abs() < 1e-12, "half angle {ha} vs {hb}");

        // And the volumes, which can only be compared on the tessellation:
        // 3.4e-5 of the body apart (9.1e-6 under truck's face step), all
        // of it the chord error of two differently-triangulated solids of
        // revolution — the cone's rim is one arc on one route and two on
        // the other, and the arc rule divides them by their own sagitta.
        assert!((va - vb).abs() <= 5e-5 * va, "by profile {va} mm³, by cutter {vb} mm³: {:.2e} apart", (va - vb).abs() / va);

        // And the exducer rim, the 44 mm circle, is chamfered and closed by
        // construction.
        let mut ctx = run(&format!("{}\n{}", HUB_HEAD, "plain = hub(False, \"plain\")\nb = chamfer(plain, 1.2, edges=plain.edges(on_plane=XY.offset(-back_t)), name=\"rim\")\n"));
        assert!(errors(&ctx).is_empty(), "{:?}", errors(&ctx));
        let rim = ctx.bodies.iter().find(|b| b.name == "rim").expect("the chamfered hub").id;
        let (mesh, _) = ctx.body_mesh(rim).unwrap().clone();
        assert!(mesh.welded(1e-4).is_watertight(), "the chamfered exducer rim is open");
    }

    /// The compressor wheel's hub, as `examples/compressor_wheel.star`
    /// draws it: a quarter-ellipse bell between the exducer rim and the
    /// nose, lofted through circles.
    const HUB_HEAD: &str = r#"
r_tip = 44.0
r_nose = 10.0
z_nose = 42.0
back_t = 5.0
c = 1.2
PHI0 = 6.0

def quarter(r_start, z_start, r_end, z_end, n):
    s0 = math.sin(PHI0)
    c0 = math.cos(PHI0)
    a = (r_start - r_end) / (1.0 - s0)
    b = (z_end - z_start) / c0
    rc = r_end + a
    zc = z_end
    pts = []
    for k in range(n + 1):
        phi = PHI0 + (90.0 - PHI0) * k / n
        pts.append((rc - a * math.sin(phi), zc - b * math.cos(phi)))
    return pts

n = 8
bell = quarter(r_tip, 0.0, r_nose, z_nose, n)

def hub(with_chamfer, name):
    prof = [(-back_t, r_tip), (0.0, r_tip)]
    prof += [(bell[k][1], bell[k][0]) for k in range(1, n + 1)]
    if with_chamfer:
        prof += [(z_nose + 2.0 - c * 0.7, r_nose), (z_nose + 2.0, r_nose - c * 0.7)]
    else:
        prof += [(z_nose + 2.0, r_nose)]
    return loft([sketch(XY.offset(z)).circle(r) for (z, r) in prof], ruled=True, name=name)
"#;

    #[test]
    fn a_radius_that_does_not_fit_is_a_warning_naming_the_edge() {
        let ctx = run(r#"
plate = box(40, 30, 3, center=True, name="plate")
out = fillet(plate, radius=4, resolution=0.5, kernel="field", name="out")
"#);
        assert!(errors(&ctx).is_empty(), "{:?}", errors(&ctx));
        let warns: Vec<&str> = ctx
            .diagnostics
            .iter()
            .filter(|d| d.severity == cadpole_core::context::Severity::Warning)
            .map(|d| d.message.as_str())
            .collect();
        assert!(
            warns.iter().any(|m| m.contains("does not fit at edge") && m.contains("largest size")),
            "{warns:?}"
        );
    }

    #[test]
    fn fillet_diagnoses_a_selection_it_cannot_resolve() {
        // An Edge value carries the body it came from, so this resolves on its
        // own -- it is a bare *name* that says nothing about a body.
        let ctx = run("out = fillet(box(10, 10, 10).edges()[0], 1)\n");
        assert!(errors(&ctx).is_empty(), "{:?}", errors(&ctx));
        let ctx = run("out = fillet([\"edge:0\"], radius=1)\n");
        assert!(errors(&ctx)[0].contains("which body"), "{:?}", errors(&ctx));
        let ctx = run("out = fillet(box(10, 10, 10, name=\"b\"), radius=0)\n");
        assert!(errors(&ctx)[0].contains("positive"), "{:?}", errors(&ctx));
        let ctx = run("out = fillet(box(10, 10, 10, name=\"b\"), edges=[\"edge:99\"], radius=1)\n");
        assert!(errors(&ctx)[0].contains("edge:99"), "{:?}", errors(&ctx));
    }

    #[test]
    fn a_smooth_union_of_fields_adds_material_at_the_seam() {
        let mut ctx = run(r#"
a = field.sphere(6).translate((-4, 0, 0))
b = field.sphere(6).translate((4, 0, 0))
sharp = mesh(a.union(b), resolution=0.4, simplify=False, name="sharp")
blend = mesh(a.smooth_union(b, 2), resolution=0.4, simplify=False, name="blend")
"#);
        assert!(errors(&ctx).is_empty(), "{:?}", errors(&ctx));
        let sharp = ctx.bodies.iter().find(|b| b.name == "sharp").unwrap().id;
        let blend = ctx.bodies.iter().find(|b| b.name == "blend").unwrap().id;
        let vs = ctx.body_mesh(sharp).unwrap().0.volume();
        let vb = ctx.body_mesh(blend).unwrap().0.volume();
        assert!(vb > vs, "the blend fills the groove: {vb} vs {vs}");
        assert!((vb - vs) / vs < 0.1, "but only near the seam: {vb} vs {vs}");
    }

    #[test]
    fn a_body_can_be_imported_as_a_field_and_meshed_back() {
        let mut ctx = run(r#"
part = box(20, 12, 8, center=True, name="part")
f = field.from_body(part)
out = mesh(f.offset(1), resolution=0.3, simplify=False, name="grown")
"#);
        assert!(errors(&ctx).is_empty(), "{:?}", errors(&ctx));
        let id = ctx.bodies.iter().find(|b| b.name == "grown").unwrap().id;
        let v = ctx.body_mesh(id).unwrap().0.volume();
        // Offsetting a box by 1 mm is the Steiner volume of a rounded box.
        let (w, d, h, r) = (20.0, 12.0, 8.0, 1.0);
        let pi = std::f64::consts::PI;
        let want = w * d * h
            + 2.0 * (w * d + d * h + h * w) * r
            + 4.0 * (w + d + h) * pi * r * r / 4.0
            + 4.0 / 3.0 * pi * r * r * r;
        assert!((v - want).abs() / want < 0.01, "{v} vs {want}");
    }

    #[test]
    fn meshing_a_field_is_deterministic_across_evaluations() {
        let src = r#"
f = field.cylinder(8, 20, round=1).twist(3).displace(field.noise(scale=5, amplitude=0.6, seed=4))
b = mesh(f, resolution=0.6, name="vase")
"#;
        let mut a = run(src);
        let mut b = run(src);
        assert!(errors(&a).is_empty(), "{:?}", errors(&a));
        let ia = a.bodies.iter().find(|x| x.name == "vase").unwrap().id;
        let ib = b.bodies.iter().find(|x| x.name == "vase").unwrap().id;
        let ma = a.body_mesh(ia).unwrap().0.clone();
        let mb = b.body_mesh(ib).unwrap().0.clone();
        assert_eq!(ma.indices, mb.indices);
        assert_eq!(ma.positions, mb.positions);
    }

    // ------------------------------------------------------- process safety

    /// The error diagnostics of a document, with their spans.
    fn spanned_errors(ctx: &Context) -> Vec<(String, Option<Span>)> {
        ctx.diagnostics
            .iter()
            .filter(|d| d.severity == cadpole_core::context::Severity::Error)
            .map(|d| (d.message.clone(), d.span.clone()))
            .collect()
    }

    /// starlark parses, compiles and evaluates recursively, and a native
    /// stack overflow aborts the process. Every one of these shapes used to
    /// do exactly that to `cadpole serve` (a 10 KB script of parentheses, 800
    /// nested `if`s, 800 `elif`s at one indent); the test harness runs on a
    /// 2 MB thread like the server's blocking workers, so without the cap
    /// this test does not fail, it kills the test binary.
    #[test]
    fn deeply_nested_syntax_is_a_diagnostic_not_a_stack_overflow() {
        let shapes: Vec<(&str, String)> = vec![
            ("parens", format!("x = {}1{}\n", "(".repeat(5000), ")".repeat(5000))),
            ("lists", format!("x = {}1{}\n", "[".repeat(5000), "]".repeat(5000))),
            ("calls", format!("x = {}1{}\n", "str(".repeat(5000), ")".repeat(5000))),
            ("unary", format!("x = {}1\n", "-".repeat(5000))),
            ("binop", format!("x = 1{}\n", " + 1".repeat(5000))),
            ("lambda", format!("x = {}1\n", "lambda: ".repeat(2000))),
            ("ternary", format!("x = {}1\n", "1 if True else ".repeat(5000))),
            ("ifs", (0..3200).map(|i| format!("{}if True:\n", " ".repeat(i))).collect::<String>() + &" ".repeat(3200) + "pass\n"),
            ("elifs", format!("if False:\n    pass\n{}x = 1\n", "elif False:\n    pass\n".repeat(3000))),
            ("fstring", format!("x = f\"{{{}1{}}}\"\n", "(".repeat(5000), ")".repeat(5000))),
        ];
        for (what, src) in shapes {
            let ctx = run(&src);
            let errs = spanned_errors(&ctx);
            assert_eq!(errs.len(), 1, "{what}: {errs:?}");
            let (msg, span) = &errs[0];
            assert!(msg.contains("nests") && msg.contains("the limit is 100"), "{what}: {msg}");
            let span = span.as_ref().unwrap_or_else(|| panic!("{what}: the diagnostic has a span"));
            assert_eq!(span.path, "main.star", "{what}");
            assert!(span.start_line >= 1 && span.start_col >= 1, "{what}: {span:?}");
        }
        // A syntax error after the depth check still reads as a syntax error.
        let ctx = run("x = (1\n");
        let errs = errors(&ctx);
        assert_eq!(errs.len(), 1, "{errs:?}");
        assert!(!errs[0].contains("nests"), "{errs:?}");
    }

    /// The scan runs before the parse in `check_syntax` too: the server
    /// calls it under its state lock, on the request thread.
    #[test]
    fn check_syntax_refuses_deep_nesting_before_parsing() {
        let src = format!("x = {}1{}\n", "(".repeat(5000), ")".repeat(5000));
        let d = check_syntax("main.star", &src).expect("a diagnostic");
        assert!(d.message.contains("nests"), "{}", d.message);
        let span = d.span.expect("with a span");
        assert_eq!((span.path.as_str(), span.start_line, span.start_col), ("main.star", 1, 105));
        assert!(check_syntax("main.star", "x = ((1))\n").is_none());
    }

    /// A chain of files each loading the next is one native `Evaluator` per
    /// level, which nothing in the Starlark limits sees; 1500 of them aborted
    /// `cadpole serve` from a 180 KB request, listener token included.
    #[test]
    fn a_deep_load_chain_is_a_diagnostic_not_a_stack_overflow() {
        let n = 2000;
        let mut scripts = vec![ScriptFile { path: "main.star".into(), source: "load(\"s0.star\", \"v\")\nb = box(v, 10, 10)\n".into() }];
        for i in 0..n {
            let source = if i + 1 < n { format!("load(\"s{}.star\", \"v\")\n", i + 1) } else { "v = 10\n".into() };
            scripts.push(ScriptFile { path: format!("s{i}.star"), source });
        }
        let ctx = evaluate(EvalInput { scripts, ..Default::default() });
        let errs = errors(&ctx);
        assert_eq!(errs.len(), 1, "{errs:?}");
        assert!(errs[0].contains("load nesting too deep") && errs[0].contains("the limit is 64"), "{}", errs[0]);
        assert!(errs[0].contains("main.star -> s0.star -> s1.star -> ... -> s62.star"), "{}", errs[0]);

        // A chain under the cap is fine, and so is a wide fan of loads.
        let n = 40;
        let mut scripts = vec![ScriptFile { path: "main.star".into(), source: "load(\"s0.star\", \"v\")\nb = box(v, 10, 10)\n".into() }];
        for i in 0..n {
            let source = if i + 1 < n { format!("load(\"s{}.star\", \"v\")\n", i + 1) } else { "v = 10\n".into() };
            scripts.push(ScriptFile { path: format!("s{i}.star"), source });
        }
        let ctx = evaluate(EvalInput { scripts, ..Default::default() });
        assert!(errors(&ctx).is_empty(), "{:?}", errors(&ctx));
        let mut scripts = vec![];
        let mut main = String::new();
        for i in 0..200 {
            main.push_str(&format!("load(\"w{i}.star\", v{i}=\"v\")\n"));
            scripts.push(ScriptFile { path: format!("w{i}.star"), source: "v = 1\n".into() });
        }
        main.push_str("b = box(v0 + v199, 10, 10)\n");
        scripts.insert(0, ScriptFile { path: "main.star".into(), source: main });
        let ctx = evaluate(EvalInput { scripts, ..Default::default() });
        assert!(errors(&ctx).is_empty(), "{:?}", errors(&ctx));
    }

    /// A file at the end of an allowed chain is still measured, and the
    /// diagnostic names that file, not the `load` that reached it.
    #[test]
    fn nesting_in_a_loaded_file_is_reported_in_that_file() {
        let deep = format!("v = {}1{}\n", "(".repeat(5000), ")".repeat(5000));
        let ctx = evaluate(EvalInput {
            scripts: vec![
                ScriptFile { path: "main.star".into(), source: "load(\"lib.star\", \"v\")\nb = box(v, 10, 10)\n".into() },
                ScriptFile { path: "lib.star".into(), source: deep },
            ],
            ..Default::default()
        });
        let errs = errors(&ctx);
        assert_eq!(errs.len(), 1, "{errs:?}");
        assert!(errs[0].contains("lib.star:1:105") && errs[0].contains("nests"), "{}", errs[0]);
    }

    /// A count the script chooses sizes a Rust `Vec` before any sandbox limit
    /// runs: `pattern_linear(b, 2147483647, ...)` was a 274 GB allocation and
    /// `ngon(2000000000, r)` a 32 GB one, and a failed allocation aborts the
    /// process. Without the cap this test does not fail, it kills the binary
    /// (or, for the ngon, fills the machine's memory first).
    #[test]
    fn script_chosen_counts_are_capped_before_allocation() {
        for (what, src) in [
            ("pattern_linear", "b = box(10, 10, 10)\np = pattern_linear(b, 2147483647, (20, 0, 0))\n"),
            ("pattern_circular", "b = box(10, 10, 10)\np = pattern_circular(b, 2147483647)\n"),
            ("ngon", "s = sketch(XY).ngon(2000000000, 5)\n"),
            ("pattern_linear", "b = box(10, 10, 10)\np = pattern_linear(b, 10001, (20, 0, 0))\n"),
            ("ngon", "s = sketch(XY).ngon(10001, 5)\n"),
        ] {
            let ctx = run(src);
            let errs = spanned_errors(&ctx);
            assert_eq!(errs.len(), 1, "{what}: {errs:?}");
            assert!(errs[0].0.contains(what) && errs[0].0.contains("over the limit of 10000"), "{what}: {}", errs[0].0);
            assert!(errs[0].1.as_ref().is_some_and(|s| s.start_line == 2 || s.start_line == 1), "{what}: {:?}", errs[0].1);
        }
        // The old checks still hold, and a sane count still works.
        let ctx = run("b = box(10, 10, 10)\np = pattern_linear(b, 0, (20, 0, 0))\n");
        assert!(errors(&ctx)[0].contains("at least 1"), "{:?}", errors(&ctx));
        let ctx = run("s = sketch(XY).ngon(2, 5)\n");
        assert!(errors(&ctx)[0].contains("at least 3"), "{:?}", errors(&ctx));
        let ctx = run("b = box(10, 10, 10)\np = pattern_linear(b, 4, (20, 0, 0))\ns = sketch(XY).ngon(64, 5)\n");
        assert!(errors(&ctx).is_empty(), "{:?}", errors(&ctx));
        assert_eq!(ctx.bodies.len(), 4);
    }

    /// NaN and infinity are refused at every geometry builtin, with a spanned
    /// diagnostic naming the argument.
    ///
    /// Two different failures come of letting them through. The first
    /// three never return at all: truck's rotational sweep spins forever on a
    /// NaN angle or axis, and a sweep is one native operation the deadline
    /// cannot interrupt, so a single `revolve` pinned ten cores of a server
    /// until it was restarted. **Without the guards this test does not fail,
    /// it hangs**, which is the bug. The rest build a body whose every vertex
    /// is NaN — `bbox()` inverted, `volume()` NaN, the OBJ written and exit 0.
    #[test]
    fn nan_and_inf_arguments_are_refused_at_every_builtin() {
        let sketch = "s = sketch(XZ).rect(10, 10, center=(10, 5))\n";
        let cases: Vec<(&str, String)> = vec![
            // The hangs.
            ("revolve: angle", format!("{sketch}r = revolve(s, angle=float(\"nan\"))\n")),
            ("revolve: axis", format!("{sketch}r = revolve(s, axis=(float(\"nan\"), 0, 1))\n")),
            ("revolve: origin", format!("{sketch}r = revolve(s, origin=(float(\"inf\"), 0, 0))\n")),
            ("cylinder: h", "c = cylinder(5, float(\"inf\"))\n".into()),
            ("cylinder: r", "c = cylinder(float(\"nan\"), 5)\n".into()),
            // The silent NaN geometry.
            ("translate: offset", "b = box(10, 10, 10)\nt = translate(b, (float(\"nan\"), 0, 0))\n".into()),
            ("rotate: angle", "b = box(10, 10, 10)\nt = rotate(b, float(\"nan\"))\n".into()),
            ("rotate: axis", "b = box(10, 10, 10)\nt = rotate(b, 45, axis=(float(\"inf\"), 0, 1))\n".into()),
            ("translate: offset", "b = box(10, 10, 10)\nt = b.translate((0, float(\"inf\"), 0))\n".into()),
            ("rotate: angle", "b = box(10, 10, 10)\nt = b.rotate(float(\"nan\"))\n".into()),
            ("copy: offset", "b = box(10, 10, 10)\nc = copy(b, offset=(float(\"nan\"), 0, 0))\n".into()),
            (
                "pattern_linear: spacing",
                "b = box(10, 10, 10)\np = pattern_linear(b, 3, (float(\"nan\"), 0, 0))\n".into(),
            ),
            ("pattern_circular: angle", "b = box(10, 10, 10)\np = pattern_circular(b, 3, angle=float(\"nan\"))\n".into()),
            // The rest of the surface.
            ("extrude: distance", "s = sketch(XY).rect(10, 10)\nb = extrude(s, float(\"nan\"))\n".into()),
            ("extrude: distance", "s = sketch(XY).rect(10, 10)\nb = extrude(s, float(\"inf\"))\n".into()),
            ("box: w", "b = box(float(\"nan\"), 10, 10)\n".into()),
            ("box: at", "b = box(10, 10, 10, at=(float(\"inf\"), 0, 0))\n".into()),
            ("cone: r0", "c = cone(float(\"nan\"), 2, 5)\n".into()),
            ("rect: w", "s = sketch(XY).rect(float(\"nan\"), 10)\n".into()),
            ("circle: r", "s = sketch(XY).circle(float(\"inf\"))\n".into()),
            ("rect: center", "s = sketch(XY).rect(10, 10, center=(float(\"nan\"), 0))\n".into()),
            ("ngon: r", "s = sketch(XY).ngon(6, float(\"nan\"))\n".into()),
            ("rounded_rect: r", "s = sketch(XY).rounded_rect(10, 10, float(\"nan\"))\n".into()),
            ("slot: length", "s = sketch(XY).slot(float(\"inf\"), 5)\n".into()),
            ("polygon: point", "s = sketch(XY).polygon([(0, 0), (10, 0), (float(\"nan\"), 10)])\n".into()),
            ("plane: origin", "p = plane(origin=(float(\"nan\"), 0, 0))\n".into()),
            ("offset: distance", "p = XY.offset(float(\"inf\"))\n".into()),
            ("to_world: x", "v = XY.to_world(float(\"nan\"), 1)\n".into()),
            ("text: size", "t = text(\"hi\", size=float(\"nan\"))\n".into()),
            ("material: roughness", "b = box(10, 10, 10)\nb.material(roughness=float(\"nan\"))\n".into()),
            ("faces: within", "b = box(10, 10, 10)\nf = b.faces(normal=(0, 0, 1), within=float(\"nan\"))\n".into()),
            ("field.sphere radius", "f = field.sphere(float(\"nan\"))\n".into()),
            ("offset d", "f = field.sphere(5).offset(float(\"nan\"))\n".into()),
            ("point: x", "cs = constrained_sketch(XY)\np = cs.point(float(\"nan\"), 0, name=\"a\")\n".into()),
        ];
        for (what, src) in cases {
            let ctx = run(&src);
            let errs = errors(&ctx);
            assert_eq!(errs.len(), 1, "{what}: expected one diagnostic, got {errs:?}");
            assert!(
                errs[0].contains(what) && (errs[0].contains("NaN") || errs[0].contains("infinite")),
                "{what}: {}",
                errs[0]
            );
            // And nothing non-finite was built along the way.
            let mut ctx = ctx;
            let ids: Vec<_> = ctx.bodies.iter().map(|b| b.id).collect();
            for id in ids {
                if let Ok((mesh, _)) = ctx.body_mesh(id) {
                    assert!(
                        mesh.positions.iter().all(|p| p.is_finite()),
                        "{what}: a body reached the mesh with a non-finite vertex"
                    );
                }
            }
        }
    }

    /// Four `param()` declarations that were accepted and then quietly meant
    /// something else. Each is now refused where it is written, except the
    /// fractional bound on an int, which is common and harmless and tightens
    /// to a whole number instead.
    #[test]
    fn a_param_declaration_that_cannot_be_honoured_is_refused_where_it_is_written() {
        // A 2-element vector default became (x, y, 0).
        let ctx = run("a = param(\"a\", [1, 2])\n");
        assert!(errors(&ctx)[0].contains("needs 3 numbers"), "{:?}", errors(&ctx));
        assert!(errors(&run("a = param(\"a\", [1, 2, 3])\n")).is_empty());

        // min > max clamped to max, so 50 came out as 10.
        let ctx = run("w = param(\"w\", 50, min=200, max=10)\n");
        assert!(errors(&ctx)[0].contains("min 200 is greater than max 10"), "{:?}", errors(&ctx));

        // choices on a non-string default was accepted and ignored.
        let ctx = run("n = param(\"n\", 3, choices=[\"a\", \"b\"])\n");
        assert!(errors(&ctx)[0].contains("choices only apply to a string param"), "{:?}", errors(&ctx));
        assert!(errors(&ctx)[0].contains("the default is number"), "{:?}", errors(&ctx));

        // A non-finite bound.
        let ctx = run("w = param(\"w\", 50, min=float(\"nan\"))\n");
        assert!(errors(&ctx)[0].contains("min is not a number"), "{:?}", errors(&ctx));
    }

    /// An int param stays an int whatever the override:
    /// clamping it to a fractional bound would hand the script a
    /// float, and the `range(n)` two lines later would fail with a type error
    /// that pointed at neither.
    #[test]
    fn an_int_param_with_a_fractional_bound_stays_an_int() {
        let src = "n = param(\"n\", 4, min=2.5, max=9.5)\nprint(type(n), n)\nb = box(n, 10, 10)\n";
        // Under the min: the tightest whole number inside it, not 2.5.
        let ctx = run_with(src, "n", 1.0);
        assert!(errors(&ctx).is_empty(), "{:?}", errors(&ctx));
        assert!(ctx.diagnostics.iter().any(|d| d.message == "print: int 3"), "{:?}", ctx.diagnostics.iter().map(|d| &d.message).collect::<Vec<_>>());
        // Over the max: likewise.
        let ctx = run_with(src, "n", 99.0);
        assert!(ctx.diagnostics.iter().any(|d| d.message == "print: int 9"));
        // In range: untouched, still an int.
        let ctx = run_with(src, "n", 5.0);
        assert!(ctx.diagnostics.iter().any(|d| d.message == "print: int 5"));
        // A float param is unaffected by any of this.
        let ctx = run_with("w = param(\"w\", 4.0, min=2.5)\nprint(type(w), w)\n", "w", 1.0);
        assert!(ctx.diagnostics.iter().any(|d| d.message == "print: float 2.5"));
        // And bounds with no whole number between them are refused outright.
        let ctx = run("n = param(\"n\", 4, min=2.2, max=2.8)\n");
        assert!(errors(&ctx)[0].contains("no whole number lies between"), "{:?}", errors(&ctx));
    }

    /// `paint(faces=)` and `Body.face()` are two builtins that take "a face
    /// name", and they disagreed about what one is: paint compared the string
    /// to the generated name and rejected the local name, the aliases and the
    /// `face:N` form its own documentation uses as the example.
    #[test]
    fn paint_takes_every_face_name_body_face_takes() {
        let paint = |faces: &str| {
            run(&format!(
                "b = box(10, 10, 10, name=\"b\")\npaint(b, color=\"#ff0000\", faces={faces})\n"
            ))
        };
        for faces in [
            "[\"b/top\"]",          // the full generated name
            "[\"top\"]",            // the local name inside the feature
            "[\"face:3\"]",         // the compatibility form
            "[b.face(\"top\")]",    // a Face value
            "b.faces(normal=(0, 0, 1))", // a Selection
            "[\"top\", \"bottom\"]",
        ] {
            let ctx = paint(faces);
            assert!(errors(&ctx).is_empty(), "faces={faces}: {:?}", errors(&ctx));
            let f = ctx.features.iter().find(|f| f.kind == cadpole_core::paint::KIND).expect("a paint feature");
            // Whatever spelling came in, the feature records the resolved
            // name, so export sees a name the body actually has.
            let recorded = f.args.get("faces").expect("the faces argument");
            assert!(recorded.contains("b/"), "faces={faces} recorded {recorded:?}");
        }
        // A name no face has is still a diagnostic at the paint call, and it
        // says what the body does have.
        let ctx = paint("[\"nosuchface\"]");
        let errs = errors(&ctx);
        assert_eq!(errs.len(), 1, "{errs:?}");
        assert!(errs[0].contains("paint:") && errs[0].contains("no face named \"nosuchface\""), "{}", errs[0]);
        assert!(errs[0].contains("b/top"), "the message lists the names it does have: {}", errs[0]);
    }

    /// `text()` is one native operation the deadline cannot interrupt, and it
    /// is superlinear: 2 000 characters cost 2.4 s of CPU, 8 000 cost 55 s,
    /// 32 000 ran for minutes — from a string a 1 MB script carries easily.
    #[test]
    fn a_text_string_is_capped_at_a_length_that_still_returns() {
        let over = Limits::MAX_TEXT_CHARS + 1;
        let ctx = run(&format!("t = text(\"W\" * {over}, size=5)\n"));
        let errs = errors(&ctx);
        assert_eq!(errs.len(), 1, "{errs:?}");
        assert!(errs[0].contains(&format!("{over} characters")), "{}", errs[0]);
        assert!(errs[0].contains("over the limit of 2048"), "{}", errs[0]);
        // Counted in characters, not bytes: this string is 2 bytes per
        // character, and the count in the message is of characters. (The
        // accepted side of the boundary is not exercised here on purpose —
        // shaping two thousand glyphs is exactly the seconds of work the cap
        // exists to bound, and a unit test should not spend them.)
        let ctx = run(&format!("t = text(\"é\" * {over}, size=5)\n"));
        let errs = errors(&ctx);
        assert_eq!(errs.len(), 1, "{errs:?}");
        assert!(errs[0].contains(&format!("{over} characters")), "characters, not bytes: {}", errs[0]);
        // And an ordinary label is untouched.
        let ctx = run("t = text(\"Kern\", size=5)\nprint(t.loops)\n");
        assert!(errors(&ctx).is_empty(), "{:?}", errors(&ctx));
        assert!(ctx.diagnostics.iter().any(|d| d.message == "print: 5"), "4 letters, 5 loops (the e has two)");
    }

    /// `choices` is the allowed set on every path a value can arrive by, not
    /// only on the declared default. An override outside it returned verbatim
    /// would send a script branching on the value
    /// (`if finish == "matte": ... else: ...`) down the wrong branch, and tell
    /// the UI and assistants an allowed set that held nobody to it.
    #[test]
    fn a_param_override_outside_choices_warns_and_uses_the_default() {
        let src = r#"
f = param("finish", "matte", choices=["matte", "gloss"])
print("finish =", f)
"#;
        let mut overrides = HashMap::new();
        overrides.insert("finish".to_owned(), ParamValue::Text("bogus".into()));
        let ctx = evaluate(EvalInput {
            scripts: vec![ScriptFile { path: "main.star".into(), source: src.into() }],
            overrides,
            ..Default::default()
        });
        assert!(errors(&ctx).is_empty(), "a bad override is a warning, not an error: {:?}", errors(&ctx));
        let warnings: Vec<_> = ctx
            .diagnostics
            .iter()
            .filter(|d| d.severity == cadpole_core::context::Severity::Warning)
            .map(|d| (d.message.clone(), d.span.clone()))
            .collect();
        assert_eq!(warnings.len(), 1, "{warnings:?}");
        let (msg, span) = &warnings[0];
        assert!(msg.contains("\"finish\"") && msg.contains("\"bogus\""), "{msg}");
        assert!(msg.contains("[\"matte\", \"gloss\"]") && msg.contains("using the default \"matte\""), "{msg}");
        assert!(span.as_ref().is_some_and(|s| s.start_line == 2), "the warning points at the param call: {span:?}");
        // The script saw the default, and so does anything reading the model.
        assert!(
            ctx.diagnostics.iter().any(|d| d.message == "print: finish = matte"),
            "{:?}",
            ctx.diagnostics.iter().map(|d| &d.message).collect::<Vec<_>>()
        );
        let (_, value) = ctx.params.iter().find(|(d, _)| d.name == "finish").expect("the param is recorded");
        assert_eq!(*value, ParamValue::Text("matte".into()));

        // A value that is one of the choices still overrides.
        let mut overrides = HashMap::new();
        overrides.insert("finish".to_owned(), ParamValue::Text("gloss".into()));
        let ctx = evaluate(EvalInput {
            scripts: vec![ScriptFile { path: "main.star".into(), source: src.into() }],
            overrides,
            ..Default::default()
        });
        assert!(ctx.diagnostics.iter().any(|d| d.message == "print: finish = gloss"));
        assert!(
            !ctx.diagnostics.iter().any(|d| d.severity == cadpole_core::context::Severity::Warning),
            "an allowed override is silent"
        );
        // ...and a param with no choices takes anything, as before.
        let ctx = run_with_text("f = param(\"finish\", \"matte\")\nprint(\"finish =\", f)\n", "finish", "anything");
        assert!(ctx.diagnostics.iter().any(|d| d.message == "print: finish = anything"));
    }

    /// A NaN or infinity can arrive from outside the script — `cadpole eval -p
    /// w=nan` parses as a number, and a proto double carries either — and a
    /// script cannot defend itself against a value it was handed.
    #[test]
    fn a_non_finite_param_override_warns_and_uses_the_default() {
        for bad in [f64::NAN, f64::INFINITY, f64::NEG_INFINITY] {
            let ctx = run_with("w = param(\"w\", 50, min=1, max=100)\nb = box(w, 10, 10)\n", "w", bad);
            assert!(errors(&ctx).is_empty(), "{:?}", errors(&ctx));
            let warnings: Vec<_> = ctx
                .diagnostics
                .iter()
                .filter(|d| d.severity == cadpole_core::context::Severity::Warning)
                .map(|d| d.message.clone())
                .collect();
            assert_eq!(warnings.len(), 1, "{bad}: {warnings:?}");
            assert!(warnings[0].contains("\"w\"") && warnings[0].contains("finite"), "{}", warnings[0]);
            let (_, value) = ctx.params.iter().find(|(d, _)| d.name == "w").unwrap();
            assert_eq!(*value, ParamValue::Number(50.0), "{bad}: the default is used");
            assert_eq!(ctx.bodies.len(), 1, "and the body is built from it");
        }
        // A finite override is untouched, clamp and all.
        let ctx = run_with("w = param(\"w\", 50, min=1, max=100)\n", "w", 1000.0);
        let (_, value) = ctx.params.iter().find(|(d, _)| d.name == "w").unwrap();
        assert_eq!(*value, ParamValue::Number(100.0));
    }

    /// Run a script with one text parameter overridden.
    fn run_with_text(src: &str, param: &str, value: &str) -> Context {
        let mut overrides = HashMap::new();
        overrides.insert(param.to_owned(), ParamValue::Text(value.to_owned()));
        evaluate(EvalInput {
            scripts: vec![ScriptFile { path: "main.star".into(), source: src.into() }],
            overrides,
            ..Default::default()
        })
    }

    /// The two kernel entry points refuse it too, so a host that reaches them
    /// another way cannot hang either. This is the guard the script-level
    /// checks sit in front of, not a substitute for them.
    #[test]
    fn the_kernel_refuses_a_non_finite_sweep() {
        use cadpole_core::geom::{Plane, Vec3};
        use cadpole_core::sketch::{Loop, P2, Sketch};
        let s = Sketch::new(Plane::XZ).with_loop(Loop::rect(P2::new(10.0, 5.0), 10.0, 10.0));
        let e = cadpole_core::kernel::revolve(&s, Vec3::ZERO, Vec3::Z, f64::NAN).unwrap_err().to_string();
        assert!(e.contains("finite"), "{e}");
        assert!(cadpole_core::kernel::revolve(&s, Vec3::ZERO, Vec3::new(f64::NAN, 0.0, 1.0), 1.0).is_err());
        assert!(cadpole_core::kernel::extrude(&s, f64::INFINITY).unwrap_err().to_string().contains("finite"));
        // A real sweep still works.
        assert!(cadpole_core::kernel::revolve(&s, Vec3::ZERO, Vec3::Z, std::f64::consts::PI).is_ok());
    }

    // ------------------------------------------------------------ model tests

    fn input_of(src: &str) -> EvalInput {
        EvalInput { scripts: vec![ScriptFile { path: "main.star".into(), source: src.into() }], ..Default::default() }
    }

    /// Run a script's tests the way `cadpole test` does.
    fn test_run(src: &str) -> TestRun {
        cadpole_opt::install();
        run_tests(input_of(src), &[], 1)
    }

    fn outcome<'a>(run: &'a TestRun, name: &str) -> &'a TestOutcome {
        run.outcomes.iter().find(|o| o.name == name).unwrap_or_else(|| {
            panic!("no outcome named {name:?}; have {:?}", run.outcomes.iter().map(|o| &o.name).collect::<Vec<_>>())
        })
    }

    const TWO_TESTS: &str = "\
b = box(10, 20, 30, name=\"b\")

def fits():
    assert_true(b.bbox()[1][2] <= 30, msg=\"under the shelf\")

def too_tall():
    assert_true(b.bbox()[1][2] <= 25, msg=\"too tall\")

test(\"fits the shelf\", fits)
test(\"clears the lid\", too_tall)
";

    /// The report a failing test produces is its name, the assertion's own
    /// message and the line that raised, and a passing one produces nothing
    /// but its name. Neither is a diagnostic: the model is fine.
    #[test]
    fn a_failing_test_reports_its_name_and_message_and_a_passing_one_is_silent() {
        let run = test_run(TWO_TESTS);
        assert!(errors(&run.base.ctx).is_empty(), "{:?}", errors(&run.base.ctx));
        assert_eq!(run.base.tests.len(), 2);
        assert_eq!(run.outcomes.len(), 2);
        let ok = outcome(&run, "fits the shelf");
        assert!(ok.ok && ok.message.is_empty() && ok.point.is_empty(), "{ok:?}");
        let bad = outcome(&run, "clears the lid");
        assert!(!bad.ok);
        assert_eq!(bad.message, "assert_true failed: too tall", "{bad:?}");
        assert_eq!(bad.span.as_ref().map(|s| s.start_line), Some(7), "the raising line, not the test() call");
        assert!(!run.ok());

        // An ordinary evaluation records the declarations and runs nothing:
        // the viewer never pays for a test.
        let ev = evaluate_full(input_of(TWO_TESTS));
        assert_eq!(ev.tests.iter().map(|t| t.name.as_str()).collect::<Vec<_>>(), ["fits the shelf", "clears the lid"]);
        assert!(ev.outcomes.is_empty());
        assert!(errors(&ev.ctx).is_empty());
    }

    /// A test that raises something other than an assertion -- a script
    /// error, `fail()` -- fails with that error, and the tests after it
    /// still run.
    #[test]
    fn a_script_error_inside_a_test_fails_only_that_test() {
        let run = test_run(
            "b = box(10, 10, 10)\n\
             def broken():\n    return b.face(\"no/such/face\")\n\
             def fine():\n    assert_solid(b)\n\
             def gives_up():\n    fail(\"not today\")\n\
             test(\"broken\", broken)\ntest(\"fine\", fine)\ntest(\"gives up\", gives_up)\n",
        );
        assert_eq!(run.outcomes.len(), 3);
        assert!(!outcome(&run, "broken").ok);
        assert!(outcome(&run, "broken").message.contains("no/such/face"), "{:?}", outcome(&run, "broken").message);
        assert!(outcome(&run, "fine").ok);
        assert!(!outcome(&run, "gives up").ok);
        assert!(outcome(&run, "gives up").message.contains("not today"), "{:?}", outcome(&run, "gives up").message);
    }

    /// A sweep runs the test once per value with the document re-evaluated
    /// at that value, and the failure names the value.
    #[test]
    fn a_sweep_reports_the_parameter_value_that_failed() {
        let run = test_run(
            "n = param(\"n\", 1, min=1, max=5)\n\
             b = box(10 * n, 10, 10, name=\"b\")\n\
             def narrow():\n    assert_true(b.bbox()[1][0] <= 20, msg=\"wider than the slot\")\n\
             test(\"fits the slot\", narrow, sweep={\"n\": [1, 2, 3]})\n",
        );
        assert_eq!(run.base.tests.len(), 1);
        let points: Vec<(bool, String)> = run.outcomes.iter().map(|o| (o.ok, point_label(&o.point))).collect();
        assert_eq!(
            points,
            [(true, "n=1".to_owned()), (true, "n=2".to_owned()), (false, "n=3".to_owned())],
            "{:?}",
            run.outcomes
        );
        let bad = &run.outcomes[2];
        assert_eq!(bad.message, "assert_true failed: wider than the slot");
        assert_eq!(bad.point, vec![("n".to_owned(), ParamValue::Number(3.0))]);
        assert!(bad.eval_ms > 0.0, "a sweep point reports the time its evaluation took");
        // The unswept base run does not run a swept test at the defaults.
        assert!(run.base.outcomes.is_empty());
    }

    /// The point at which the model itself refuses to build is a failure at
    /// that point, not of the run -- finding it is what a sweep is for.
    #[test]
    fn a_sweep_point_where_the_model_does_not_build_fails_at_that_point() {
        let run = test_run(
            "w = param(\"w\", 10)\n\
             b = box(w, 10, 10, name=\"b\")\n\
             def solid():\n    assert_solid(b)\n\
             test(\"still a box\", solid, sweep={\"w\": [10, 0]})\n",
        );
        assert_eq!(run.outcomes.len(), 2);
        assert!(run.outcomes[0].ok);
        let bad = &run.outcomes[1];
        assert!(!bad.ok);
        assert_eq!(point_label(&bad.point), "w=0");
        assert!(bad.message.starts_with("the model did not build: "), "{}", bad.message);
        assert!(bad.message.contains("positive"), "{}", bad.message);
        assert!(bad.span.is_some(), "the model's own error carries its line");
    }

    /// Two axes are the cartesian product, first axis slowest, so a report
    /// reads like the nested loops the author would have written.
    #[test]
    fn a_two_axis_sweep_is_the_cartesian_product_in_declaration_order() {
        let run = test_run(
            "a = param(\"a\", 1)\nb = param(\"b\", 1)\n\
             x = box(a, b, 1)\n\
             def any():\n    pass\n\
             test(\"grid\", any, sweep={\"a\": [1, 2], \"b\": [10, 20, 30]})\n",
        );
        let labels: Vec<String> = run.outcomes.iter().map(|o| point_label(&o.point)).collect();
        assert_eq!(labels, ["a=1 b=10", "a=1 b=20", "a=1 b=30", "a=2 b=10", "a=2 b=20", "a=2 b=30"]);
        assert!(run.ok());
    }

    /// `monotone=` reads the number each point returned and checks the
    /// sequence; a violation names both points and their values.
    #[test]
    fn monotone_checks_the_returned_number_along_the_sweep() {
        const SRC: &str = "\
w = param(\"w\", 10)
b = box(w, 10, 10, name=\"b\")

def vol():
    assert_solid(b)
    return b.volume()

test(\"more width, more volume\", vol, sweep={\"w\": [5, 10, 20]}, monotone=\"increasing\")
test(\"less width, less volume?\", vol, sweep={\"w\": [5, 10, 20]}, monotone=\"decreasing\")
";
        let run = test_run(SRC);
        let up: Vec<&TestOutcome> = run.outcomes.iter().filter(|o| o.name == "more width, more volume").collect();
        assert_eq!(up.len(), 3, "three points and no verdict entry when the claim holds: {up:?}");
        assert!(up.iter().all(|o| o.ok));
        assert_eq!(up.iter().map(|o| o.value.map(|v| v.round())).collect::<Vec<_>>(), [Some(500.0), Some(1000.0), Some(2000.0)]);

        let down: Vec<&TestOutcome> = run.outcomes.iter().filter(|o| o.name == "less width, less volume?").collect();
        assert_eq!(down.len(), 4, "three points that pass, then the verdict: {down:?}");
        assert!(down[..3].iter().all(|o| o.ok));
        let verdict = down[3];
        assert!(!verdict.ok);
        assert_eq!(verdict.message, "not decreasing: 500 at w=5, then 1000 at w=10", "{verdict:?}");
        assert_eq!(point_label(&verdict.point), "w=10", "the verdict sits at the point that broke the claim");

        // A test that returns nothing cannot carry a monotone claim.
        let run = test_run(
            "w = param(\"w\", 10)\nb = box(w, 10, 10)\n\
             def nothing():\n    assert_solid(b)\n\
             test(\"silent\", nothing, sweep={\"w\": [5, 10]}, monotone=\"increasing\")\n",
        );
        let verdict = run.outcomes.last().unwrap();
        assert!(!verdict.ok);
        assert!(verdict.message.contains("needs the test to return a number"), "{}", verdict.message);
        assert!(verdict.message.contains("w=5"), "{}", verdict.message);
    }

    /// Every new assertion says what it measured against what, so the report
    /// is enough to act on without re-running anything.
    #[test]
    fn the_new_assertions_say_what_they_measured() {
        // Passing forms first: a 10 x 20 x 30 box is a polyhedron, so its
        // volume and box are exact.
        let ctx = run("\
b = box(10, 20, 30, name=\"b\")
assert_close(1.0, 1.0)
assert_close(1.0, 1.0 + 1e-9)
assert_close(100.0, 101.0, rel=0.02)
assert_close(b.bbox(), ((0, 0, 0), (10, 20, 30)))
assert_close([1, 2, 3], (1, 2, 3))
assert_volume(b, 6000)
assert_volume(b, 6000, rel=0, tol=1e-6)
assert_bbox(b, (0, 0, 0), (10, 20, 30))
assert_printable(b)
assert_printable(b, printer=\"bambu_a1\", min_wall=1.0)
");
        assert!(errors(&ctx).is_empty(), "{:?}", errors(&ctx));

        let first_error = |src: &str| {
            let ctx = run(src);
            let e = errors(&ctx);
            assert_eq!(e.len(), 1, "{e:?}");
            e[0].clone()
        };
        let e = first_error("assert_close(1.0, 1.5, msg=\"the gap\")\n");
        assert!(e.starts_with("assert_close failed: 1.0 != 1.5"), "{e}");
        assert!(e.contains("differ by 0.5, allowed 0.000001") && e.ends_with("(the gap)"), "{e}");

        let e = first_error("assert_close((1, 2, 3), (1, 2, 4))\n");
        assert!(e.contains("at index 2: 3 vs 4"), "{e}");

        let e = first_error("assert_close((1, 2), (1, 2, 3))\n");
        assert!(e.contains("has 2 numbers") && e.contains("has 3"), "{e}");

        let e = first_error("assert_close(\"a\", 1)\n");
        assert!(e.contains("assert_close: a must be a number or a list/tuple of numbers, got string"), "{e}");

        let e = first_error("b = box(10, 20, 30, name=\"b\")\nassert_volume(b, 5000)\n");
        assert_eq!(e, "assert_volume failed: b is 6000 mm³, expected 5000 ± 50");

        let e = first_error("b = box(10, 20, 30, name=\"b\")\nassert_bbox(b, (0, 0, 0), (10, 20, 31), msg=\"the shelf\")\n");
        assert_eq!(e, "assert_bbox failed: b spans (0, 0, 0)..(10, 20, 30), expected (0, 0, 0)..(10, 20, 31) within 0.1 mm (the shelf)");

        // A 0.3 mm wall is thinner than any nozzle's two perimeters.
        let e = first_error(
            "b = box(20, 20, 20, name=\"b\")\nc = box(19.4, 19.4, 30, at=(0.3, 0.3, 0.3))\n\
             shell = subtract(b, c, name=\"shell\")\nassert_printable(shell)\n",
        );
        assert!(e.starts_with("assert_printable failed: shell: [thin_wall]") || e.starts_with("assert_printable failed: shell: ["), "{e}");
        assert!(e.contains("mm"), "the message carries the measurement: {e}");
        // ...and unlike `print_check`, nothing was recorded as a warning.
        let ctx = run(
            "b = box(20, 20, 20, name=\"b\")\nc = box(19.4, 19.4, 30, at=(0.3, 0.3, 0.3))\n\
             shell = subtract(b, c, name=\"shell\")\nassert_printable(shell)\n",
        );
        assert!(!ctx.diagnostics.iter().any(|d| d.severity == cadpole_core::context::Severity::Warning), "{:?}", ctx.diagnostics);
    }

    /// Everything `test()` refuses at the declaration, so a mistake in the
    /// test is an error at its line rather than a test that quietly passes.
    #[test]
    fn a_test_declaration_is_checked_where_it_is_made() {
        let e = |src: &str| {
            let ctx = run(&format!("w = param(\"w\", 1)\nb = box(w, 1, 1)\ndef t():\n    pass\n{src}"));
            let e = errors(&ctx);
            assert_eq!(e.len(), 1, "{e:?}");
            e[0].clone()
        };
        assert!(e("test(\"a\", t)\ntest(\"a\", t)\n").contains("test \"a\" declared twice"));
        assert!(e("test(\"\", t)\n").contains("name is empty"));
        assert!(e("test(\"a\", t, sweep={\"w\": []})\n").contains("\"w\" has no values"));
        assert!(e("test(\"a\", t, sweep={})\n").contains("sweep= is empty"));
        assert!(e("test(\"a\", t, sweep=[1, 2])\n").contains("sweep= is a dict"));
        assert!(e("test(\"a\", t, sweep={\"w\": 3})\n").contains("needs a list of values"));
        assert!(e("test(\"a\", t, sweep={\"w\": [box]})\n").contains("must be numbers, bools, strings or (x, y, z)"));
        assert!(e("test(\"a\", t, monotone=\"increasing\")\n").contains("sweep over exactly one parameter"));
        assert!(e("test(\"a\", t, sweep={\"w\": [1], \"h\": [1]}, monotone=\"increasing\")\n").contains("exactly one parameter"));
        assert!(e("test(\"a\", t, sweep={\"w\": [1]}, monotone=\"sideways\")\n").contains("\"increasing\" or \"decreasing\""));
        let big = (0..=Limits::MAX_SWEEP_POINTS).map(|i| i.to_string()).collect::<Vec<_>>().join(", ");
        assert!(e(&format!("test(\"a\", t, sweep={{\"w\": [{big}]}})\n")).contains("more than 256 points"));
        // A test that declares a test: the run is already under way.
        let run = test_run("def inner():\n    pass\ndef outer():\n    test(\"inner\", inner)\ntest(\"outer\", outer)\n");
        assert_eq!(run.outcomes.len(), 1);
        assert!(run.outcomes[0].message.contains("cannot declare another test"), "{}", run.outcomes[0].message);

        // A sweep over a parameter the document does not declare would pass
        // for the wrong reason, so the runner refuses it by name.
        let run = test_run("w = param(\"w\", 1)\nb = box(w, 1, 1)\ndef t():\n    pass\ntest(\"a\", t, sweep={\"h\": [1, 2]})\n");
        assert_eq!(run.outcomes.len(), 1);
        assert!(run.outcomes[0].message.contains("\"h\" is not a param of this document (it declares w)"), "{}", run.outcomes[0].message);
        let run = test_run("w = param(\"w\", 1)\nb = box(w, 1, 1)\ndef t():\n    pass\ntest(\"a\", t, sweep={\"w\": [\"wide\"]})\n");
        assert!(run.outcomes[0].message.contains("\"w\" is a number but the sweep gives it a string (wide)"), "{}", run.outcomes[0].message);
    }

    /// `names` picks tests; a name nothing declares is itself a failure,
    /// because a CI line that runs zero tests must not pass.
    #[test]
    fn tests_can_be_selected_by_name() {
        let run = run_tests(input_of(TWO_TESTS), &["clears the lid".to_owned(), "no such test".to_owned()], 1);
        assert_eq!(run.outcomes.len(), 2);
        assert!(!outcome(&run, "clears the lid").ok);
        let missing = outcome(&run, "no such test");
        assert!(!missing.ok);
        assert_eq!(missing.message, "no test named \"no such test\"; the document declares fits the shelf, clears the lid");
        assert!(run.outcomes.iter().all(|o| o.name != "fits the shelf"), "an unselected test does not run");
    }

    /// A library may declare tests too; they run after the *entry* file has
    /// built the model, like the entry's own, and see what it built.
    #[test]
    fn tests_declared_in_a_loaded_library_run_after_the_entry() {
        let input = EvalInput {
            scripts: vec![
                ScriptFile {
                    path: "main.star".into(),
                    source: "load(\"checks.star\", \"remember\")\nb = box(10, 10, 10, name=\"b\")\nremember(b)\n\
                             def own():\n    assert_solid(b)\ntest(\"own\", own)\n"
                        .into(),
                },
                ScriptFile {
                    path: "checks.star".into(),
                    // `remember` runs after this module is frozen, so it closes
                    // over the body rather than appending it to a list.
                    source: "def lib_level():\n    assert_true(True)\ntest(\"from the library\", lib_level)\n\
                             def remember(b):\n    test(\"the library saw the body\", lambda: assert_volume(b, 1000))\n"
                        .into(),
                },
            ],
            ..Default::default()
        };
        let run = run_tests(input, &[], 1);
        assert!(errors(&run.base.ctx).is_empty(), "{:?}", errors(&run.base.ctx));
        let names: Vec<&str> = run.outcomes.iter().map(|o| o.name.as_str()).collect();
        assert_eq!(names, ["from the library", "the library saw the body", "own"], "declaration order across files");
        assert!(run.ok(), "{:?}", run.outcomes);
    }

    /// Sweep points evaluate in parallel when asked; the report is the same.
    #[test]
    fn parallel_sweep_points_report_in_sweep_order() {
        const SRC: &str = "n = param(\"n\", 1)\nb = box(n, 1, 1)\n\
             def t():\n    assert_true(n != 3, msg=\"three\")\n    return b.volume()\n\
             test(\"grid\", t, sweep={\"n\": [1, 2, 3, 4, 5]})\n";
        let serial = run_tests(input_of(SRC), &[], 1);
        let parallel = run_tests(input_of(SRC), &[], 4);
        let view = |r: &TestRun| r.outcomes.iter().map(|o| (o.ok, point_label(&o.point), o.message.clone())).collect::<Vec<_>>();
        assert_eq!(view(&serial), view(&parallel));
        assert_eq!(view(&serial)[2], (false, "n=3".to_owned(), "assert_true failed: three".to_owned()));
    }

    /// A model that does not build has no tests to run, and the run says so
    /// through the base evaluation's diagnostics rather than a failed test.
    #[test]
    fn a_model_that_does_not_build_runs_no_tests() {
        let run = test_run("def t():\n    pass\ntest(\"a\", t)\nb = box(-1, 1, 1)\n");
        assert!(!errors(&run.base.ctx).is_empty());
        assert!(run.outcomes.is_empty());
        assert!(run.ok(), "vacuously; the caller reads the diagnostics");
    }

    /// Whether an example declares tests, decided from the source.
    fn declares_tests(p: &std::path::Path) -> bool {
        std::fs::read_to_string(p).unwrap().contains("\ntest(")
    }

    /// The examples gate, for the example `name`, on one evaluation:
    ///
    /// - it evaluates cleanly, exactly as `cadpole eval examples/<name>.star`
    ///   does, and makes a visible body;
    /// - every body it makes at the default tolerance is a solid: closed,
    ///   consistently oriented, no crossings, no doubled or internal faces
    ///   (`cadpole_core::solid`). A script with a `kernel` parameter
    ///   (`choices=["auto", "brep", "mesh"]`) is checked on the default and on
    ///   every other choice, so the exact and the mesh kernel are held to the
    ///   same standard on the same models;
    /// - its exact bodies tessellate closed by id (`closed_by_id`) and carry
    ///   recognised faces (`faces_are_recognised`).
    ///
    /// Its own `test(...)` declarations are `example_tests_pass`.
    fn example_gate(name: &str) {
        let (dir, files) = example_files();
        let assets = example_assets(&dir);
        let p = files
            .iter()
            .find(|p| p.file_stem().and_then(|s| s.to_str()) == Some(name))
            .unwrap_or_else(|| panic!("no example {name}.star"));
        let file = format!("{name}.star");
        let src = std::fs::read_to_string(p).unwrap();
        let scripts = example_scripts(&dir, p);
        // The default run first: for a script with a kernel parameter it is
        // where the declaration, and so the list of choices, comes from.
        let (mut ctx, bad) = validate_example(&scripts, &assets, &file, None, "default");
        assert!(errors(&ctx).is_empty(), "{file}: {:?}", errors(&ctx));
        assert!(ctx.bodies.iter().any(|b| b.visible), "{file} produced no bodies");
        let mut found = Vec::new();
        let mut ran = Vec::new();
        let kernel = ctx.params.iter().find(|(d, _)| d.name == "kernel").cloned();
        if offers_a_kernel(&src) {
            let (decl, value) = kernel.unwrap_or_else(|| panic!("{file} mentions a kernel parameter but declares none"));
            let default = match value {
                ParamValue::Text(s) => s,
                other => panic!("{file}: the kernel parameter is not text: {other:?}"),
            };
            found.extend(bad.into_iter().map(|(f, _, b, c, s)| (f, default.clone(), b, c, s)));
            ran.push((file.clone(), default.clone()));
            for choice in decl.choices.clone() {
                if choice == default {
                    continue;
                }
                let (_, bad) = validate_example(&scripts, &assets, &file, Some(&choice), &choice);
                found.extend(bad);
                ran.push((file.clone(), choice));
            }
        } else {
            assert!(kernel.is_none(), "{file} declares a kernel parameter in a form offers_a_kernel does not recognise");
            found.extend(bad);
            ran.push((file.clone(), "default".to_owned()));
        }
        judge_solids(found, &ran);

        let open = closed_by_id(&file, &ctx);
        assert!(open.is_empty(), "{}", open.join("\n"));
        let impeller_checked = faces_are_recognised(&file, &mut ctx);
        if name == "impeller" {
            assert_eq!(impeller_checked, (true, true), "the impeller's shaft and nut were not both inspected");
        }
    }

    /// Every exact body of one evaluated example tessellates closed *by
    /// vertex id* — no weld — at the default tolerance and at 0.01, the
    /// export range, which is what the edge-first tessellator promises.
    /// Judged on `MeshedSolid::unmatched_edges`, the raw output of the
    /// tessellator, not on the welded mesh the solid gate judges. Every body
    /// that can be tessellated at all must be; a refusal is a failure too.
    fn closed_by_id(file: &str, ctx: &Context) -> Vec<String> {
        let mut open = Vec::new();
        let mut here = 0;
        for b in ctx.bodies.iter().filter(|b| b.visible) {
            let Ok(solid) = b.solid() else { continue };
            for tol in [cadpole_core::kernel::DEFAULT_TOLERANCE, 0.01] {
                match cadpole_core::kernel::tessellate(solid, tol) {
                    Ok(meshed) => {
                        let (n, _) = meshed.unmatched_edges();
                        if n > 0 {
                            open.push(format!("{file} `{}` at {tol}: {n} unmatched edges by id", b.name));
                        }
                        if meshed.faces.len() != solid.face_iter().count() {
                            open.push(format!("{file} `{}` at {tol}: {} of {} faces", b.name, meshed.faces.len(), solid.face_iter().count()));
                        }
                    }
                    Err(e) => open.push(format!("{file} `{}` at {tol}: refused: {e}", b.name)),
                }
            }
            here += 1;
        }
        eprintln!("{file}: {here} exact bod{} judged by id at 0.05 and 0.01", if here == 1 { "y" } else { "ies" });
        open
    }

    /// An example's tests are run in this many slices, each its own test, so
    /// one example with many sweep points is not one long test.
    const TEST_SLICES: usize = 4;

    /// The tests an example declares, as its source reads: one entry per
    /// top-level `test(`, its first argument when that is a string literal.
    /// A name the script computes (`"... %s ..." % x`) reads as the literal,
    /// which `example_tests_pass` notices and corrects.
    fn declared_test_names(src: &str) -> Vec<String> {
        src.match_indices("\ntest(")
            .map(|(i, m)| {
                let rest = src[i + m.len()..].trim_start();
                rest.strip_prefix('"').and_then(|r| r.find('"').map(|end| r[..end].to_owned())).unwrap_or_default()
            })
            .collect()
    }

    /// The examples' own tests are CI: every `test(...)` the example `name`
    /// declares passes, sweeps included, the way `cadpole test
    /// examples/<name>.star` runs them. This is slice `slice` of them: every
    /// `TEST_SLICES`-th test in declaration order.
    fn example_tests_pass(name: &str, slice: usize) {
        let (dir, files) = example_files();
        let p = files
            .iter()
            .find(|p| p.file_stem().and_then(|s| s.to_str()) == Some(name))
            .unwrap_or_else(|| panic!("no example {name}.star"));
        let file = format!("{name}.star");
        let read = declared_test_names(&std::fs::read_to_string(p).unwrap());
        let mut mine: Vec<String> = read.iter().skip(slice).step_by(TEST_SLICES).cloned().collect();
        if mine.is_empty() {
            return;
        }
        let input = EvalInput { scripts: example_scripts(&dir, p), assets: Some(example_assets(&dir)), ..Default::default() };
        let mut run = run_tests(input.clone(), &mine, 0);
        let errs = errors(&run.base.ctx);
        assert!(errs.is_empty(), "{file}: {errs:?}");
        // The declarations are what counts; the source only saves an
        // evaluation when its names are the declared ones.
        let declared: Vec<String> = run.base.tests.iter().map(|t| t.name.clone()).collect();
        assert_eq!(
            declared.len(),
            read.len(),
            "{file}: {} tests declared but {} top-level test( calls, so slicing by the source would skip some",
            declared.len(),
            read.len()
        );
        let wanted: Vec<String> = declared.iter().skip(slice).step_by(TEST_SLICES).cloned().collect();
        if wanted != mine {
            mine = wanted;
            run = run_tests(input, &mine, 0);
        }
        eprintln!(
            "{file} slice {slice}: {} of {} tests, {} result{}, {} failed",
            mine.len(),
            declared.len(),
            run.outcomes.len(),
            if run.outcomes.len() == 1 { "" } else { "s" },
            run.outcomes.iter().filter(|o| !o.ok).count()
        );
        let failures: Vec<String> = run
            .outcomes
            .iter()
            .filter(|o| !o.ok)
            .map(|o| {
                let at = if o.point.is_empty() { String::new() } else { format!(" at {}", point_label(&o.point)) };
                format!("- {file}: {:?}{at}: {}", o.name, o.message)
            })
            .collect();
        assert!(failures.is_empty(), "example tests failed:\n{}", failures.join("\n"));
    }

    /// The face census of one evaluated example: its exact bodies carry at
    /// least one recognised face, no face truck holds as a plane is called
    /// anything else, and the two faces whose class is known by construction
    /// — the impeller's shaft (an extruded circle) and its cap nut's chamfer
    /// band (a loft between two circles of different radius, cut to the hex)
    /// — come out as a cylinder of the shaft's radius and a cone of the
    /// taper's half-angle. Returns whether the shaft and the nut were seen.
    /// The counts go to stderr so a run reads as a census of the corpus.
    fn faces_are_recognised(file: &str, ctx: &mut Context) -> (bool, bool) {
        use cadpole_core::SurfaceClass;
        let file = file.to_owned();
        let mut impeller_checked = (false, false);
        let bodies: Vec<(cadpole_core::BodyId, String, bool)> =
            ctx.bodies.iter().filter(|b| b.visible).map(|b| (b.id, b.name.clone(), b.kind() == "brep")).collect();
        let mut counts: std::collections::BTreeMap<&'static str, usize> = std::collections::BTreeMap::new();
        let mut exact = 0usize;
        for (id, name, is_brep) in &bodies {
            let Ok((_, topo)) = ctx.body_mesh(*id) else { continue };
            for f in &topo.faces {
                // A mesh body's groups inherit the source's `surface_kind`
                // as provenance but carry no exact surface to classify.
                if *is_brep && f.surface_kind == "plane" {
                    assert!(
                        matches!(f.surface, SurfaceClass::Plane { .. }),
                        "{file} {name} {}: truck's plane classified as {:?}",
                        f.name,
                        f.surface
                    );
                }
                *counts.entry(f.surface_class()).or_default() += 1;
                exact += usize::from(*is_brep);
            }
            if file == "impeller.star" && name == "shaft" {
                let walls: Vec<&cadpole_core::tessellate::FaceInfo> =
                    topo.faces.iter().filter(|f| f.surface_kind != "plane").collect();
                assert!(!walls.is_empty());
                for f in walls {
                    match &f.surface {
                        SurfaceClass::Cylinder { axis, radius, axis_point } => {
                            // bore / 2 - 0.05 with the script's default bore of 12.
                            assert!((radius - 5.95).abs() < 1e-6, "{}: radius {radius}", f.name);
                            assert!(axis.cross(cadpole_core::Vec3::Z).length() < 1e-9, "{}: axis {axis:?}", f.name);
                            assert!(axis_point.x.abs() < 1e-6 && axis_point.y.abs() < 1e-6, "{}: {axis_point:?}", f.name);
                        }
                        other => panic!("{}: the shaft's wall is a cylinder, got {other:?}", f.name),
                    }
                }
                impeller_checked.0 = true;
            }
            if file == "impeller.star" && name == "nut" {
                let cones: Vec<&SurfaceClass> =
                    topo.faces.iter().map(|f| &f.surface).filter(|s| matches!(s, SurfaceClass::Cone { .. })).collect();
                assert!(!cones.is_empty(), "the nut's chamfer band survives the intersection");
                // r_c + 0.5 at z = nose_h + 5.5 down to across_flats / 2 - 0.3 at
                // z = nose_h + 8.1, with across_flats = 2 hub_r - 3 and hub_r = 11.
                let across_flats = 2.0 * 11.0 - 3.0;
                let r_c = across_flats / 2.0 / fmath::cos(30f64.to_radians());
                let want = fmath::atan2((r_c + 0.5) - (across_flats / 2.0 - 0.3), 2.6);
                for c in cones {
                    let SurfaceClass::Cone { half_angle, axis, .. } = c else { unreachable!() };
                    assert!((half_angle - want).abs() < 1e-6, "half angle {half_angle}, expected {want}");
                    assert!(axis.cross(cadpole_core::Vec3::Z).length() < 1e-9, "axis {axis:?}");
                }
                impeller_checked.1 = true;
            }
        }
        let recognised: usize = counts.iter().filter(|(k, _)| **k != "other").map(|(_, n)| n).sum();
        eprintln!("{file}: {} bod{}, faces by surface {counts:?}", bodies.len(), if bodies.len() == 1 { "y" } else { "ies" });
        if exact > 0 {
            assert!(recognised > 0, "{file}: {exact} exact faces and not one recognised");
        }
        impeller_checked
    }

    /// Tests per example (`examples::<name>::gate` and `::tests_<k>`), so
    /// the examples gate runs in parallel and spreads over the target's
    /// shards; `the_example_gates_cover_every_example` keeps the list equal to
    /// the examples directory.
    // The macro writes one `tests_<k>` per slice.
    const _: () = assert!(TEST_SLICES == 4);
    macro_rules! example_gates {
        ($($name:ident),* $(,)?) => {
            const EXAMPLES: &[&str] = &[$(stringify!($name)),*];
            mod examples {
                $(
                    mod $name {
                        use super::super::{example_gate, example_tests_pass};
                        #[test]
                        fn gate() {
                            example_gate(stringify!($name));
                        }
                        #[test]
                        fn tests_0() {
                            example_tests_pass(stringify!($name), 0);
                        }
                        #[test]
                        fn tests_1() {
                            example_tests_pass(stringify!($name), 1);
                        }
                        #[test]
                        fn tests_2() {
                            example_tests_pass(stringify!($name), 2);
                        }
                        #[test]
                        fn tests_3() {
                            example_tests_pass(stringify!($name), 3);
                        }
                    }
                )*
            }
        };
    }

    example_gates!(
        bearing_608,
        bracket,
        cabin_parts,
        cabin_plan,
        cabin_site,
        charging_stand,
        chess_rook,
        color_plan,
        compressor_wheel,
        constrained_bracket,
        constrained_lprofile,
        desk_mascot,
        dice,
        drawing_sheet,
        effusion_liner,
        embossed_mug,
        enclosure,
        filleted_bracket,
        gear_toy,
        gears,
        gridfinity_baselid,
        gridfinity_baseplate,
        gridfinity_bin,
        gridfinity_showcase,
        hex_coaster,
        hillside_lot,
        house_set,
        impeller,
        import_demo,
        iris,
        knurled_knob,
        lamp_shade,
        layers_demo,
        lithophane_lamp,
        material_bar,
        materials,
        nightstand,
        nightstand_scene,
        part,
        peg_board,
        pillow_block,
        plan_parti_compare,
        plan_parti_fits,
        plan_toy_house,
        planetary,
        planter,
        print_plan,
        print_ready,
        radial_engine,
        room_layout_opt,
        scan_underlay,
        shadow_study,
        showcase,
        site_survey,
        soap_dish,
        split_for_bed,
        spring_and_bolt,
        storey_kit,
        story_kit,
        svg_badge,
        table_lamp,
        text_label,
        thumbscrew,
        turbojet,
        vase,
        vendor_bearing,
        wing,
    );

    /// Every example has its gate above and nothing else does; and the gate
    /// cannot pass by running nothing: both halves of the solid gate (with
    /// and without a kernel parameter) and the examples' own tests have
    /// examples to run.
    #[test]
    fn the_example_gates_cover_every_example() {
        let (dir, files) = example_files();
        let mut on_disk: Vec<String> = files.iter().map(|p| p.file_stem().unwrap().to_str().unwrap().to_owned()).collect();
        on_disk.sort();
        let mut listed: Vec<String> = EXAMPLES.iter().map(|s| (*s).to_owned()).collect();
        listed.sort();
        assert_eq!(on_disk, listed, "example_gates! must name exactly the examples in {dir}");
        let sources: Vec<String> = files.iter().map(|p| std::fs::read_to_string(p).unwrap()).collect();
        assert!(sources.iter().any(|s| offers_a_kernel(s)), "no examples with a kernel parameter found in {dir}");
        assert!(sources.iter().any(|s| !offers_a_kernel(s)), "no examples without a kernel parameter found in {dir}");
        let with_tests = files.iter().filter(|p| declares_tests(p)).count();
        assert!(with_tests >= 3, "only {with_tests} examples declare tests; the gate would be vacuous");
    }

    // ---- clearance and the multi-body print checks ----------------

    /// `distance` measures the gap on the facing faces and names them: the
    /// planetary's fit test is a distance, not a boolean.
    #[test]
    fn clearance_between_two_cubes_is_the_gap_on_the_facing_faces() {
        let ctx = run(
            "a = box(10, 10, 10, name=\"a\")\n\
             b = translate(box(10, 10, 10, name=\"b\"), (10.4, 0, 0))\n\
             c = distance(a, b)\n\
             assert_close(c.distance, 0.4, tol=1e-4)\n\
             assert_true(not c.overlap)\n\
             assert_true(not c.touching)\n\
             assert_eq(c.face_a, \"a/right\")\n\
             assert_eq(c.face_b, \"b/left\")\n\
             pa, pb = c.at\n\
             assert_close(pa[0], 10.0, tol=1e-4)\n\
             assert_close(pb[0], 10.4, tol=1e-4)\n\
             assert_eq(c.a_point, pa)\n\
             assert_eq(c.bodies[0], \"a\")\n\
             assert_true(c.tolerance > 0)\n\
             print(c)\n",
        );
        assert!(errors(&ctx).is_empty(), "{:?}", errors(&ctx));
    }

    /// Zero distance says the surfaces meet; `overlap` says whether the
    /// material is shared, which is what a print-in-place part cares about.
    #[test]
    fn clearance_tells_touching_from_overlapping() {
        let ctx = run(
            "a = box(10, 10, 10, name=\"a\")\n\
             t = translate(box(10, 10, 10, name=\"t\"), (10, 0, 0))\n\
             o = translate(box(10, 6, 6, name=\"o\"), (8, 2, 2))\n\
             ct = distance(a, t)\n\
             assert_eq(ct.distance, 0.0)\n\
             assert_true(ct.touching)\n\
             assert_true(not ct.overlap)\n\
             co = distance(a, o)\n\
             assert_eq(co.distance, 0.0)\n\
             assert_true(co.overlap)\n\
             assert_true(not co.touching)\n",
        );
        assert!(errors(&ctx).is_empty(), "{:?}", errors(&ctx));
    }

    /// A pin in a bore with 0.15 mm radial clearance: the gap is 0.15 to
    /// within the chord error both surfaces carry, and `clearance_map`
    /// reports the whole ring, once per millimetre of it.
    #[test]
    fn clearance_of_a_pin_in_a_bore_is_the_radial_gap_all_round() {
        let ctx = run(
            "blk = translate(box(20, 20, 20, name=\"blk\"), (-10, -10, 0))\n\
             bored = subtract(blk, cylinder(5.15, 30, at=(0, 0, -5), name=\"bore\"), name=\"housing\")\n\
             pin = cylinder(5, 20, name=\"pin\")\n\
             c = distance(pin, bored)\n\
             assert_true(abs(c.distance - 0.15) <= 2 * c.tolerance, msg=\"gap %s\" % c.distance)\n\
             assert_true(not c.overlap, msg=\"a pin in a bore shares no material\")\n\
             assert_true(c.face_a.startswith(\"pin/\"), msg=c.face_a)\n\
             assert_true(c.face_b != \"\", msg=c.face_b)\n\
             ring = clearance_map(pin, bored, below=0.3)\n\
             assert_true(len(ring) >= 12, msg=\"ring of %d\" % len(ring))\n\
             assert_true(all([d < 0.3 for d, pa, pb, fa, fb in ring]), msg=\"%s\" % ring)\n\
             assert_true(all([fa.startswith(\"pin/\") for d, pa, pb, fa, fb in ring]), msg=\"%s\" % ring)\n\
             assert_close(ring[0][0], c.distance, tol=1e-9)\n\
             assert_true(len([d for d, pa, pb, fa, fb in ring if abs(d - 0.15) <= 2 * c.tolerance]) >= len(ring) // 2, msg=\"%s\" % ring)\n\
             ys = [pa[1] for d, pa, pb, fa, fb in ring]\n\
             assert_true(min(ys) < -3 and max(ys) > 3, msg=\"both sides of the ring: %s\" % ys)\n\
             assert_true(ring[0][0] <= ring[-1][0], msg=\"ascending\")\n\
             assert_eq(len(clearance_map(pin, bored, below=0.3, limit=4)), 4)\n\
             assert_eq(len(clearance_map(pin, bored, below=0.05)), 0)\n",
        );
        assert!(errors(&ctx).is_empty(), "{:?}", errors(&ctx));
        // The cap says so.
        let warnings: Vec<&str> =
            ctx.diagnostics.iter().filter(|d| d.severity == cadpole_core::context::Severity::Warning).map(|d| d.message.as_str()).collect();
        assert!(warnings.iter().any(|w| w.starts_with("clearance_map(pin, housing): 4 of ")), "{warnings:?}");
    }

    /// `Clearance.tolerance` is the uncertainty of the answer, so it follows
    /// the meshes the answer was measured on: a body has its own
    /// tessellation level, and a gear at 0.01 mm measured against a housing
    /// at the document's 0.05 carries 0.05 (the coarser of the two — each
    /// surface is off by its own chords), two bodies at 0.01 carry 0.01, and
    /// a rigid copy carries its source's. The fuse finding's ± says the same
    /// figure. Before this the value was the document's whatever the bodies
    /// were meshed at, which made `gap - 2 * c.tolerance >= min_gap` ask a
    /// finely tessellated running fit for 0.1 mm it did not need.
    #[test]
    fn clearance_tolerance_is_the_coarser_of_the_two_bodies_own() {
        let ctx = run(
            "fine = cylinder(5, 10, name=\"fine\").tessellation(tolerance=0.01)\n\
             other = cylinder(5, 10, at=(10.35, 0, 0), name=\"other\").tessellation(tolerance=0.01)\n\
             coarse = cylinder(5, 10, at=(-10.35, 0, 0), name=\"coarse\")\n\
             moved = translate(copy(other, name=\"moved\"), (0, 30, 0))\n\
             near = cylinder(5, 10, at=(10.35, 30, 0), name=\"near\").tessellation(tolerance=0.01)\n\
             assert_close(distance(fine, other).tolerance, 0.01)\n\
             assert_close(distance(fine, coarse).tolerance, 0.05)\n\
             assert_close(distance(coarse, fine).tolerance, 0.05)\n\
             assert_close(distance(near, moved).tolerance, 0.01)\n\
             c = distance(fine, other)\n\
             assert_true(c.distance - 2 * c.tolerance >= 0.3, msg=\"a 0.35 mm running fit at 0.01 mm proves 0.3: %s\" % c)\n\
             r = print_check([fine, other], min_gap=0.4)\n\
             assert_true(any([\"±0.01 mm\" in w for w in r[0].errors]), msg=\"%s\" % r[0].errors)\n\
             r = print_check([fine, coarse], min_gap=0.4)\n\
             assert_true(any([\"±0.05 mm\" in w for w in r[0].errors]), msg=\"%s\" % r[0].errors)\n",
        );
        assert!(errors(&ctx).is_empty(), "{:?}", errors(&ctx));
    }

    /// The fuse rule: two bodies checked together, 0.2 mm apart, are one
    /// part off the printer; 0.4 mm apart they are two. The finding is on
    /// the first body's report and in the diagnostics with its faces.
    #[test]
    fn clearance_fuse_check_fires_at_two_tenths_and_not_at_four() {
        let ctx = run(
            "a = box(10, 10, 10, name=\"a\")\n\
             b = box(10, 10, 10, at=(10.2, 0, 0), name=\"b\")\n\
             r = print_check([a, b])\n\
             assert_eq(len(r), 2)\n\
             assert_true(not r[0].ok)\n\
             assert_true(r[1].ok)\n\
             assert_eq(len(r[0].errors), 1)\n\
             assert_true(\"0.2 mm\" in r[0].errors[0] and \"a/right and b/left\" in r[0].errors[0], msg=r[0].errors[0])\n\
             assert_true(print_check(a).ok, msg=\"alone it is fine\")\n",
        );
        assert!(errors(&ctx).is_empty(), "{:?}", errors(&ctx));
        let warnings: Vec<&str> =
            ctx.diagnostics.iter().filter(|d| d.severity == cadpole_core::context::Severity::Warning).map(|d| d.message.as_str()).collect();
        assert!(warnings.iter().any(|w| w.starts_with("print_check(a): [fuse] gap of 0.2 mm between a and b")), "{warnings:?}");

        let ctx = run(
            "a = box(10, 10, 10, name=\"a\")\n\
             b = translate(box(10, 10, 10, name=\"b\"), (10.4, 0, 0))\n\
             r = print_check([a, b])\n\
             assert_true(r[0].ok and r[1].ok)\n\
             assert_eq(len(r[0].errors) + len(r[0].warnings), 0)\n\
             assert_printable([a, b])\n\
             # The threshold is the profile's, and the printer knows it.\n\
             assert_eq(printer(\"bambu_a1\").min_gap, 0.3)\n\
             assert_true(not print_check([a, b], min_gap=0.5)[0].ok)\n",
        );
        assert!(errors(&ctx).is_empty(), "{:?}", errors(&ctx));
        let ctx = run(
            "a = box(10, 10, 10, name=\"a\")\n\
             b = box(10, 10, 10, at=(10.2, 0, 0), name=\"b\")\n\
             assert_printable([a, b])\n",
        );
        let e = errors(&ctx);
        assert_eq!(e.len(), 1, "{e:?}");
        assert!(e[0].contains("assert_printable failed: a: [fuse] gap of 0.2 mm"), "{}", e[0]);
    }

    /// The whole-layer rule: a 0.5 mm horizontal gap at 0.2 mm layers is
    /// two and a half layers, which the slicer rounds; 0.4 mm is two.
    #[test]
    fn clearance_layer_check_fires_for_half_a_layer_and_not_for_a_whole_one() {
        let ctx = run(
            "base = box(20, 20, 5, name=\"base\")\n\
             plate = box(10, 10, 5, at=(5, 5, 5.5), name=\"plate\")\n\
             r = print_check([base, plate])\n\
             assert_true(r[0].ok, msg=\"a rounding risk is a warning\")\n\
             assert_eq(len(r[0].warnings), 1)\n\
             assert_true(\"2.5 layers at 0.2 mm\" in r[0].warnings[0], msg=r[0].warnings[0])\n\
             assert_true(\"plate/bottom and base/top\" in r[0].warnings[0], msg=r[0].warnings[0])\n\
             whole = translate(box(10, 10, 5, name=\"w\"), (5, 5, 5.4))\n\
             r2 = print_check([base, whole])\n\
             assert_eq(len(r2[0].warnings) + len(r2[1].warnings), 0)\n\
             assert_printable([base, whole])\n\
             # At 0.25 mm layers the 0.5 mm gap is two whole layers.\n\
             assert_printable([base, plate], layer_height=0.25)\n",
        );
        assert!(errors(&ctx).is_empty(), "{:?}", errors(&ctx));
        let warnings: Vec<&str> =
            ctx.diagnostics.iter().filter(|d| d.severity == cadpole_core::context::Severity::Warning).map(|d| d.message.as_str()).collect();
        assert!(warnings.iter().any(|w| w.starts_with("print_check(base): [layer_gap] gap of 0.5 mm")), "{warnings:?}");
    }

    /// A body in the air over nothing is a bridge warning; a body a whole
    /// number of layers over another is bridged, and not judged further.
    #[test]
    fn clearance_bridge_check_sees_a_body_the_overhang_check_cannot() {
        let ctx = run(
            "base = box(20, 20, 5, name=\"base\")\n\
             air = translate(box(10, 10, 5, name=\"air\"), (40, 40, 5))\n\
             r = print_check([base, air])\n\
             assert_true(r[1].ok)\n\
             assert_eq(len(r[1].warnings), 1)\n\
             assert_true(\"prints in the air\" in r[1].warnings[0], msg=r[1].warnings[0])\n\
             assert_true(print_check(air).unsupported_area == 0, msg=\"alone, its underside is the bed\")\n",
        );
        assert!(errors(&ctx).is_empty(), "{:?}", errors(&ctx));
    }

    // ---- import_step / import_mesh / import_report -------------

    /// A cylinder the way an OCCT-based exporter writes one: full-circle
    /// rims on one vertex each and a side face walked up and down its
    /// seam, with the file's own face names.
    const OCCT_CYLINDER_STEP: &[u8] = b"ISO-10303-21;
HEADER;
FILE_DESCRIPTION(('cylinder'),'2;1');
FILE_NAME('cyl.step','2026-09-20T00:00:00',(''),(''),'','','');
FILE_SCHEMA(('AUTOMOTIVE_DESIGN { 1 0 10303 214 1 1 1 1 }'));
ENDSEC;
DATA;
#1=CARTESIAN_POINT('',(0.,0.,0.));
#2=DIRECTION('',(0.,0.,1.));
#3=DIRECTION('',(1.,0.,0.));
#4=AXIS2_PLACEMENT_3D('',#1,#2,#3);
#5=CARTESIAN_POINT('',(0.,0.,10.));
#6=AXIS2_PLACEMENT_3D('',#5,#2,#3);
#7=CARTESIAN_POINT('',(5.,0.,0.));
#8=CARTESIAN_POINT('',(5.,0.,10.));
#9=VERTEX_POINT('',#7);
#10=VERTEX_POINT('',#8);
#11=CIRCLE('',#4,5.);
#12=CIRCLE('',#6,5.);
#13=EDGE_CURVE('',#9,#9,#11,.T.);
#14=EDGE_CURVE('',#10,#10,#12,.T.);
#15=VECTOR('',#2,1.);
#16=LINE('',#7,#15);
#17=EDGE_CURVE('',#9,#10,#16,.T.);
#18=PLANE('',#4);
#19=PLANE('',#6);
#20=CYLINDRICAL_SURFACE('',#4,5.);
#21=ORIENTED_EDGE('',*,*,#13,.T.);
#22=ORIENTED_EDGE('',*,*,#17,.T.);
#23=ORIENTED_EDGE('',*,*,#14,.F.);
#24=ORIENTED_EDGE('',*,*,#17,.F.);
#25=EDGE_LOOP('',(#21,#22,#23,#24));
#26=FACE_OUTER_BOUND('',#25,.T.);
#27=ADVANCED_FACE('side',(#26),#20,.T.);
#28=ORIENTED_EDGE('',*,*,#13,.F.);
#29=EDGE_LOOP('',(#28));
#30=FACE_OUTER_BOUND('',#29,.T.);
#31=ADVANCED_FACE('bottom',(#30),#18,.F.);
#32=ORIENTED_EDGE('',*,*,#14,.T.);
#33=EDGE_LOOP('',(#32));
#34=FACE_OUTER_BOUND('',#33,.T.);
#35=ADVANCED_FACE('top',(#34),#19,.T.);
#36=CLOSED_SHELL('',(#27,#31,#35));
#37=MANIFOLD_SOLID_BREP('cyl',#36);
ENDSEC;
END-ISO-10303-21;
";

    /// An ASCII STL of a 20 x 10 x 5 box, optionally with its last
    /// triangle left out (an open mesh).
    fn box_stl(open: bool) -> Vec<u8> {
        use cadpole_core::geom::Plane;
        use cadpole_core::sketch::{Loop, P2, Sketch};
        let s = Sketch::new(Plane::XY).with_loop(Loop::rect(P2::new(0.0, 0.0), 20.0, 10.0));
        let solid = cadpole_core::kernel::extrude(&s, 5.0).unwrap().remove(0);
        let (mesh, _) = cadpole_core::tessellate::tessellate(&solid, 0.02).unwrap();
        let n = mesh.triangle_count() - if open { 1 } else { 0 };
        let mut out = String::from("solid box\n");
        for t in 0..n {
            let tri = mesh.triangle(t).unwrap();
            out.push_str(" facet normal 0 0 0\n  outer loop\n");
            for p in tri {
                out.push_str(&format!("   vertex {} {} {}\n", p.x, p.y, p.z));
            }
            out.push_str("  endloop\n endfacet\n");
        }
        out.push_str("endsolid box\n");
        out.into_bytes()
    }

    #[test]
    fn a_step_asset_imports_as_an_exact_body_named_by_face_index() {
        let mut ctx = run_with_assets(
            r#"
cyl = import_step("cyl.step", name="cyl")
assert_eq(cyl.kind, "brep")
assert_solid(cyl)
assert_eq(cyl.faces().count(), 4)
assert_eq(cyl.faces(kind=kind.cylinder).count(), 2)
assert_eq(cyl.faces(kind=kind.plane).count(), 2)
assert_close(cyl.volume(), math.pi * 25 * 10, rel=1e-2)   # chorded at the display tolerance
names = cyl.faces().names()
assert_true("cyl/face[0]" in names, msg=str(names))
assert_true("cyl/face[0]#1" in names, msg=str(names))
assert_true("cyl/face[1]" in names, msg=str(names))
assert_true("cyl/face[2]" in names, msg=str(names))
# The same file by its asset value, as a second feature.
again = import_step(asset("cyl.step"), name="again", tolerance=0.001)
assert_eq(again.faces().count(), 4)
"#,
            &[("cyl.step", OCCT_CYLINDER_STEP)],
        );
        assert!(errors(&ctx).is_empty(), "{:?}", errors(&ctx));
        let f = ctx.features.iter().find(|f| f.id.0 == "cyl").expect("the feature").clone();
        assert_eq!(f.kind, "import_step");
        assert_eq!(f.args["asset"], "cyl.step");
        assert_eq!(f.args["sha256"].len(), 64);
        assert_eq!(f.args["importer_version"], cadpole_import::IMPORTER_VERSION.to_string());
        assert_eq!(f.args["solids"], "1");
        assert_eq!(f.args["faces"], "4");
        assert_eq!(f.args["solids_named"], "cyl");
        assert!(f.args.get("tolerance").is_none(), "no override was asked for");
        let again = ctx.features.iter().find(|f| f.id.0 == "again").unwrap();
        assert_eq!(again.args["tolerance"], "0.001");
        // The info line names what was read and what was done to it.
        let infos: Vec<&str> = f.diagnostics.iter().map(|d| d.message.as_str()).collect();
        assert!(
            infos.iter().any(|m| m.contains("import_step: 1 solid with 4 faces") && m.contains("2 full-circle edges split") && m.contains("1 seam face split")),
            "{infos:?}"
        );
        assert!(infos.iter().any(|m| m.contains("no length unit")), "{infos:?}");
        // The file's own face names are aliases of the indexed names.
        let id = f.outputs[0];
        let topo = ctx.body_mesh(id).unwrap().1.clone();
        let by_name = |n: &str| topo.faces.iter().find(|x| x.name == n).unwrap_or_else(|| panic!("{n} among {:?}", topo.faces.iter().map(|x| &x.name).collect::<Vec<_>>()));
        assert_eq!(by_name("cyl/face[0]").aliases, vec!["cyl/side".to_string()]);
        assert_eq!(by_name("cyl/face[0]#1").aliases, vec!["cyl/side#1".to_string()]);
        assert_eq!(by_name("cyl/face[1]").aliases, vec!["cyl/bottom".to_string()]);
        assert_eq!(by_name("cyl/face[2]").aliases, vec!["cyl/top".to_string()]);
        assert_eq!(by_name("cyl/face[1]").kind(), "plane");
        assert_eq!(by_name("cyl/face[0]").kind(), "cylinder");
    }

    #[test]
    fn a_mesh_asset_imports_with_a_report_the_memo_replays() {
        let src = r#"
scan = import_mesh("scan.stl", name="scan")
assert_eq(scan.kind, "mesh")
r = import_report(scan)
assert_eq(r.format, "STL")
assert_eq(r.triangles, 11)
assert_eq(r.vertices, 33)          # report keeps the soup
assert_true(not r.closed)
assert_true(not r.solid)
assert_eq(r.open_edges, 3)
assert_eq(r.shells, 1)
assert_eq(r.volume, None)
assert_eq(r.chord_error, None)
assert_eq(r.genus, None)
assert_true(r.bbox != None)
assert_eq(scan.faces().count(), 1)
assert_eq(scan.faces().names(), ["scan/surface"])
assert_true("open edge" in r.summary, msg=r.summary)
w = import_mesh(asset("scan.stl"), name="welded", repair=repair.weld)
wr = import_report(w)
assert_eq(wr.vertices, 8)
assert_eq(wr.weld_pass, 1)
assert_eq(wr.welded, 25)
assert_true(hasattr(repair, "off"))
"#;
        let files: &[(&str, &[u8])] = &[("scan.stl", &box_stl(true))];
        let memo = Arc::new(cadpole_core::memo::Memo::new());
        let run = |memo: Option<Arc<cadpole_core::memo::Memo>>| {
            let mut store = cadpole_assets::MemoryAssetStore::new();
            for (name, bytes) in files {
                store.insert(name, bytes.to_vec());
            }
            evaluate(EvalInput {
                scripts: vec![ScriptFile { path: "main.star".into(), source: src.into() }],
                assets: Some(Arc::new(store)),
                memo,
                ..Default::default()
            })
        };
        let first = run(Some(memo.clone()));
        assert!(errors(&first).is_empty(), "{:?}", errors(&first));
        assert!(first.memo_counts.runs.iter().any(|r| r.op == "import_mesh" && r.feature == "scan"), "{:?}", first.memo_counts.runs);
        let warns = |ctx: &Context| -> Vec<String> {
            ctx.diagnostics.iter().filter(|d| d.severity == cadpole_core::context::Severity::Warning).map(|d| d.message.clone()).collect()
        };
        let w1 = warns(&first);
        assert!(w1.iter().any(|m| m.contains("not closed") && m.contains("3 open edges")), "{w1:?}");
        let scan = first.features.iter().find(|f| f.id.0 == "scan").unwrap();
        assert_eq!(scan.args["repair"], "report");
        assert_eq!(scan.args["report[0].open_edges"], "3");
        assert_eq!(scan.args["bodies"], "1");
        assert!(scan.diagnostics.iter().any(|d| d.message.contains("chord error: not measurable")), "{:?}", scan.diagnostics);
        // The second evaluation parses nothing and says the same things.
        let second = run(Some(memo));
        assert!(errors(&second).is_empty(), "{:?}", errors(&second));
        assert!(second.memo_counts.hits.iter().any(|r| r.op == "import_mesh" && r.feature == "scan"), "{:?}", second.memo_counts.hits);
        assert!(!second.memo_counts.runs.iter().any(|r| r.op == "import_mesh"), "{:?}", second.memo_counts.runs);
        assert_eq!(warns(&second), w1, "a memo hit warns exactly as the run did");
        let scan2 = second.features.iter().find(|f| f.id.0 == "scan").unwrap();
        assert_eq!(scan2.args["report[0].open_edges"], "3");
    }

    /// A GLB the way a phone scanner writes one: one node `scan`,
    /// a cube [0, 1]³ in metres with unorm8 `COLOR_0` (red on the four
    /// low-y corners, blue on the rest), `uint32` indices and an unlit white
    /// material. Built here so the fixture is synthetic.
    fn scan_glb() -> Vec<u8> {
        let p: [[f32; 3]; 8] = [
            [0.0, 0.0, 0.0], [1.0, 0.0, 0.0], [1.0, 1.0, 0.0], [0.0, 1.0, 0.0],
            [0.0, 0.0, 1.0], [1.0, 0.0, 1.0], [1.0, 1.0, 1.0], [0.0, 1.0, 1.0],
        ];
        let t: [u32; 36] = [0, 2, 1, 0, 3, 2, 4, 5, 6, 4, 6, 7, 0, 1, 5, 0, 5, 4, 3, 7, 6, 3, 6, 2, 0, 4, 7, 0, 7, 3, 1, 2, 6, 1, 6, 5];
        let mut bin: Vec<u8> = p.iter().flatten().flat_map(|x| x.to_le_bytes()).collect();
        let col_at = bin.len();
        for q in &p {
            bin.extend_from_slice(if q[1] == 0.0 { &[255, 0, 0, 255] } else { &[0, 0, 255, 255] });
        }
        let idx_at = bin.len();
        bin.extend(t.iter().flat_map(|i| i.to_le_bytes()));
        let json = format!(
            r#"{{"asset":{{"version":"2.0","generator":"cadpole test"}},"extensionsUsed":["KHR_materials_unlit"],"scene":0,"scenes":[{{"nodes":[0]}}],"nodes":[{{"mesh":0,"name":"scan"}}],"materials":[{{"pbrMetallicRoughness":{{"baseColorFactor":[1,1,1,1],"metallicFactor":0,"roughnessFactor":1}},"extensions":{{"KHR_materials_unlit":{{}}}}}}],"meshes":[{{"primitives":[{{"attributes":{{"POSITION":0,"COLOR_0":1}},"indices":2,"material":0,"mode":4}}]}}],"accessors":[{{"bufferView":0,"componentType":5126,"count":8,"type":"VEC3"}},{{"bufferView":1,"componentType":5121,"normalized":true,"count":8,"type":"VEC4"}},{{"bufferView":2,"componentType":5125,"count":36,"type":"SCALAR"}}],"bufferViews":[{{"buffer":0,"byteOffset":0,"byteLength":{col_at}}},{{"buffer":0,"byteOffset":{col_at},"byteLength":32}},{{"buffer":0,"byteOffset":{idx_at},"byteLength":144}}],"buffers":[{{"byteLength":{blen}}}]}}"#,
            blen = bin.len()
        );
        let mut js = json.into_bytes();
        while js.len() % 4 != 0 {
            js.push(b' ');
        }
        let total = 12 + 8 + js.len() + 8 + bin.len();
        let mut out = b"glTF".to_vec();
        out.extend_from_slice(&2u32.to_le_bytes());
        out.extend_from_slice(&(total as u32).to_le_bytes());
        out.extend_from_slice(&(js.len() as u32).to_le_bytes());
        out.extend_from_slice(b"JSON");
        out.extend_from_slice(&js);
        out.extend_from_slice(&(bin.len() as u32).to_le_bytes());
        out.extend_from_slice(b"BIN\0");
        out.extend_from_slice(&bin);
        out
    }

    #[test]
    fn a_glb_scan_imports_z_up_in_millimetres_with_its_colours_and_the_memo_replays_them() {
        let src = r#"
scan = layer("scan", opacity=0.5)
room = scan.add(import_mesh("room.glb", name="room", repair=repair.off))
assert_eq(room.kind, "mesh")
r = import_report(room)
assert_eq(r.format, "glTF")
assert_eq(r.object, "scan")
assert_true(r.vertex_colors)
assert_true(r.pairs_skipped)
assert_eq(r.triangles, 12)
lo, hi = r.bbox
# glTF (x, y, z) in metres is Cadpole 1000 (x, -z, y).
assert_close(lo[0], 0.0, tol=1e-6)
assert_close(lo[1], -1000.0, tol=1e-6)
assert_close(hi[2], 1000.0, tol=1e-6)
assert_eq(room.faces().names(), ["room/surface"])
asis = import_mesh(asset("room.glb"), name="asis", up="z", units=dxf_units.mm)
lo2, hi2 = import_report(asis).bbox
assert_close(hi2[1], 1.0, tol=1e-9)
feet = import_mesh("room.glb", name="feet", units="ft")
assert_close(import_report(feet).bbox[1][2], 304.8, tol=1e-3)
"#;
        let memo = Arc::new(cadpole_core::memo::Memo::new());
        let run = |memo: Option<Arc<cadpole_core::memo::Memo>>| {
            let mut store = cadpole_assets::MemoryAssetStore::new();
            store.insert("room.glb", scan_glb());
            evaluate(EvalInput {
                scripts: vec![ScriptFile { path: "main.star".into(), source: src.into() }],
                assets: Some(Arc::new(store)),
                memo,
                ..Default::default()
            })
        };
        let check = |ctx: &mut Context| {
            assert!(errors(ctx).is_empty(), "{:?}", errors(ctx));
            let f = ctx.features.iter().find(|f| f.id.0 == "room").unwrap().clone();
            assert_eq!(f.args["format"], "glTF");
            assert_eq!(f.args["report[0].vertex_colors"], "true");
            assert_eq!(f.args["look[0].base_color"], "1 1 1");
            let id = f.outputs[0];
            let body = ctx.body(id).unwrap();
            assert_eq!(body.layer, "scan");
            assert_eq!(body.material.base_color, [1.0, 1.0, 1.0], "white under the scan's own colours");
            assert_eq!(body.material.metallic, 0.0);
            let (mesh, _) = ctx.body_mesh(id).unwrap();
            let c = mesh.vertex_colors().expect("the colours reach the body");
            assert_eq!(&c[..3], &[255, 0, 0]);
            let infos: Vec<&str> = f.diagnostics.iter().map(|d| d.message.as_str()).collect();
            assert!(infos.iter().any(|m| m.contains("KHR_materials_unlit")), "{infos:?}");
            assert!(infos.iter().any(|m| m.contains("1000·(x, −z, y)")), "{infos:?}");
            let asis = ctx.features.iter().find(|f| f.id.0 == "asis").unwrap();
            assert_eq!((asis.args["up"].as_str(), asis.args["units"].as_str()), ("z", "mm"));
        };
        let mut first = run(Some(memo.clone()));
        check(&mut first);
        assert!(first.memo_counts.runs.iter().any(|r| r.op == "import_mesh" && r.feature == "room"), "{:?}", first.memo_counts.runs);
        let mut second = run(Some(memo));
        check(&mut second);
        assert!(second.memo_counts.hits.iter().any(|r| r.op == "import_mesh" && r.feature == "room"), "{:?}", second.memo_counts.hits);
        assert!(!second.memo_counts.runs.iter().any(|r| r.op == "import_mesh"), "{:?}", second.memo_counts.runs);
    }

    #[test]
    fn a_gltf_finds_its_bin_beside_it_and_says_when_it_is_missing() {
        let glb = scan_glb();
        // The same document as a .gltf whose buffer is a .bin asset.
        let json_len = u32::from_le_bytes([glb[12], glb[13], glb[14], glb[15]]) as usize;
        let json = std::str::from_utf8(&glb[20..20 + json_len]).unwrap().trim_end().to_owned();
        let bin_at = 20 + json_len + 8;
        let bin = glb[bin_at..].to_vec();
        let gltf = json.replace(r#""buffers":[{"byteLength""#, r#""buffers":[{"uri":"room%20scan.bin","byteLength""#);
        assert!(gltf.contains("room%20scan.bin"));
        let src = "room = import_mesh(\"room.gltf\", name=\"room\")\nassert_eq(import_report(room).triangles, 12)\n";
        let ctx = run_with_assets(src, &[("room.gltf", gltf.as_bytes()), ("room scan.bin", &bin)]);
        assert!(errors(&ctx).is_empty(), "{:?}", errors(&ctx));
        let f = ctx.features.iter().find(|f| f.id.0 == "room").unwrap();
        assert_eq!(f.args["buffer[0]"], "room scan.bin");
        assert_eq!(f.args["buffer[0].sha256"].len(), 64);
        // Without the .bin: an error at the call, naming the file to add.
        let ctx = run_with_assets("\n\nroom = import_mesh(\"room.gltf\")\n", &[("room.gltf", gltf.as_bytes())]);
        let errs = of_severity(&ctx, cadpole_core::context::Severity::Error);
        assert!(errs.iter().any(|(m, line)| m.contains("\"room scan.bin\"") && m.contains("put it in ./assets") && *line == 3), "{errs:?}");
    }

    #[test]
    fn a_broken_glb_is_an_error_at_the_call_never_a_panic() {
        let mut glb = scan_glb();
        let n = glb.len();
        glb.truncate(n - 40);
        let cases: Vec<(&str, Vec<u8>, &str)> = vec![
            ("truncated", glb, "truncated"),
            ("bad JSON", b"glTF\x02\x00\x00\x00\x1c\x00\x00\x00\x08\x00\x00\x00JSON{\"asset\"".to_vec(), "not valid"),
        ];
        for (what, bytes, want) in cases {
            let ctx = run_with_assets("x = 1\nm = import_mesh(\"bad.glb\")\n", &[("bad.glb", &bytes)]);
            let errs = of_severity(&ctx, cadpole_core::context::Severity::Error);
            assert!(errs.iter().any(|(m, line)| m.contains("import_mesh(\"bad.glb\")") && m.contains(want) && *line == 2), "{what}: {errs:?}");
        }
        let bad_args: &[(&str, &str)] = &[
            ("import_mesh(\"room.glb\", up=\"x\")", "valid: y"),
            ("import_mesh(\"room.glb\", units=\"furlong\")", "unknown unit \"furlong\""),
            ("import_mesh(\"room.glb\", units=units.ft_in)", "not a file's unit"),
        ];
        for (call, want) in bad_args {
            let ctx = run_with_assets(call, &[("room.glb", &scan_glb())]);
            let errs = errors(&ctx);
            assert!(errs.iter().any(|e| e.contains(want)), "{call}: {errs:?}");
        }
    }

    #[test]
    fn the_importers_refuse_the_wrong_file_kind_and_the_wrong_body() {
        let cases: &[(&str, &str)] = &[
            ("import_step(\"scan.stl\")", "is a STL mesh: use import_mesh"),
            ("import_mesh(\"cyl.step\")", "is a STEP file: use import_step"),
            ("import_step(42)", "must be an asset(...) or the asset's name"),
            ("import_mesh(\"scan.stl\", repair=\"auto\")", "valid: report, weld, off"),
            ("import_step(\"nothere.step\")", "no asset named"),
            ("import_report(extrude(sketch(XY).rect(4, 4), 2, name=\"blk\"))", "not by import_mesh"),
            ("import_step(\"cyl.step\", tolerance=-1)", "must be a positive number"),
        ];
        for (call, want) in cases {
            let ctx = run_with_assets(call, &[("scan.stl", &box_stl(false)), ("cyl.step", OCCT_CYLINDER_STEP)]);
            let errs = errors(&ctx);
            assert!(errs.iter().any(|e| e.contains(want)), "{call}: {errs:?}\n  wanted {want:?}");
        }
    }

    /// A synthetic site survey in feet: lot lines as four LINEs on PL,
    /// contours every foot on CONTOUR with their elevation as z, thirty
    /// spot shots as TEXT on SPOT beside two labels that are not
    /// elevations, two trees as INSERTs of a block, and one INSERT of a
    /// block the file never defines (a warning).
    fn survey_dxf() -> Vec<u8> {
        let mut e = String::new();
        for (a, b) in [((0, 0), (100, 0)), ((100, 0), (100, 60)), ((100, 60), (0, 60)), ((0, 60), (0, 0))] {
            e.push_str(&format!("0\nLINE\n8\nPL\n10\n{}\n20\n{}\n30\n0\n11\n{}\n21\n{}\n31\n0\n", a.0, a.1, b.0, b.1));
        }
        for k in 0..8 {
            let z = 150.0 + k as f64;
            let (y, y2) = (5.0 + 7.0 * k as f64, 8.0 + 7.0 * k as f64);
            e.push_str(&format!(
                "0\nLWPOLYLINE\n8\nCONTOUR\n38\n{z}\n70\n0\n90\n3\n10\n0\n20\n{y}\n10\n50\n20\n{y2}\n10\n100\n20\n{y}\n"
            ));
        }
        for i in 0..30 {
            let (x, y) = (3.0 + (i % 6) as f64 * 16.0, 4.0 + (i / 6) as f64 * 11.0);
            let z = 150.0 + i as f64 * 0.27;
            let label = match i % 3 {
                0 => format!("{z:.2} BK"),
                1 => format!("TC {z:.2}"),
                _ => format!("{z:.2}"),
            };
            e.push_str(&format!("0\nTEXT\n8\nSPOT\n10\n{x}\n20\n{y}\n30\n0\n40\n1.5\n1\n{label}\n"));
        }
        e.push_str("0\nTEXT\n8\nSPOT\n10\n1\n20\n1\n30\n0\n40\n1.5\n1\nLOT 12\n");
        e.push_str("0\nMTEXT\n8\nSPOT\n10\n2\n20\n2\n30\n0\n40\n1.5\n1\n\\A1;SEE NOTE 4.1 BELOW\n");
        e.push_str("0\nINSERT\n8\nTREE\n2\nTREE\n10\n20\n20\n30\n30\n0\n0\nINSERT\n8\nTREE\n2\nTREE\n10\n70\n20\n40\n30\n0\n");
        e.push_str("0\nINSERT\n8\nTREE\n2\nSHRUB\n10\n50\n20\n50\n30\n0\n");
        format!(
            "0\nSECTION\n2\nHEADER\n9\n$ACADVER\n1\nAC1015\n9\n$INSUNITS\n70\n2\n0\nENDSEC\n\
             0\nSECTION\n2\nBLOCKS\n0\nBLOCK\n8\n0\n2\nTREE\n10\n0\n20\n0\n30\n0\n0\nCIRCLE\n8\n0\n10\n0\n20\n0\n30\n0\n40\n3\n0\nENDBLK\n0\nENDSEC\n\
             0\nSECTION\n2\nENTITIES\n{e}0\nENDSEC\n0\nEOF\n"
        )
        .into_bytes()
    }

    #[test]
    fn a_survey_dxf_yields_spot_elevations_contours_lot_lines_and_trees_and_the_memo_replays() {
        let src = r##"
site = import_dxf("site.dxf", name="site")
assert_eq(type(site), "Drawing2D")
assert_eq(site.units, "ft")
assert_eq(site.unit_source, "$INSUNITS")
assert_close(site.scale, 304.8)
assert_eq(site.version, "AC1015 (2000)")
assert_eq([l[0] for l in site.layers()], ["PL", "CONTOUR", "SPOT", "TREE"])
assert_eq([l[1] for l in site.layers()], [4, 8, 32, 2])
spots = site.spot_elevations(layer="SPOT")
assert_eq(len(spots), 30, msg="the two other labels are not elevations")
assert_close(spots[0][0], 3.0 * 304.8)
assert_close(spots[0][2], 150.0 * 304.8)
assert_close(spots[1][2], 150.27 * 304.8)
assert_eq(len(site.spot_elevations(layer="SPOT", pattern="# BK")), 10)
assert_eq(len(site.spot_elevations(layer="SPOT", pattern="TC #")), 10)
contours = site.contours(layer="CONTOUR")
assert_eq(len(contours), 8)
assert_close(contours[0][0], 150.0 * 304.8)
assert_eq(len(contours[0][1]), 3)
assert_close(contours[7][0], 157.0 * 304.8)
lot = site.sketch(layer="PL", closed=True)
assert_eq(lot.loops, 1)
assert_eq(lot.paths, 0)
assert_eq(lot.names, ["site/PL/loop[0]"])
pts = site.points(layer="CONTOUR")
assert_eq(len(pts), 24)
assert_close(pts[0][2], 150.0 * 304.8)
texts = site.texts(layer="SPOT")
assert_eq(len(texts), 32)
assert_eq(texts[0][2], "150.00 BK")
assert_close(texts[0][3], 1.5 * 304.8)
assert_eq(texts[31][2], "SEE NOTE 4.1 BELOW")
curves = site.curves(layer="CONTOUR")
assert_eq(len(curves), 8)
assert_close(curves[0].start[2], 150.0 * 304.8)
assert_eq(curves[0].id, "site/CONTOUR/lwpolyline[0]")
trees = site.sketch(layer=["TREE"])
assert_eq(trees.loops, 2)
assert_eq(trees.names, ["site/TREE/loop[0]", "site/TREE/loop[1]"])
bb = site.bbox()
assert_close(bb[0][0], 0.0)
assert_close(bb[1][0], 100.0 * 304.8)
assert_eq(site.extents, None)
everything = site.sketch()
assert_eq(everything.loops, 3)
pad = extrude(site.sketch(layer="PL"), 100, name="pad")
assert_close(pad.volume(), 100 * 60 * 304.8 * 304.8 * 100, rel=1e-9)
"##;
        let files: &[(&str, &[u8])] = &[("site.dxf", &survey_dxf())];
        let memo = Arc::new(cadpole_core::memo::Memo::new());
        let run = |memo: Option<Arc<cadpole_core::memo::Memo>>| {
            let mut store = cadpole_assets::MemoryAssetStore::new();
            for (name, bytes) in files {
                store.insert(name, bytes.to_vec());
            }
            evaluate(EvalInput {
                scripts: vec![ScriptFile { path: "main.star".into(), source: src.into() }],
                assets: Some(Arc::new(store)),
                memo,
                ..Default::default()
            })
        };
        let first = run(Some(memo.clone()));
        assert!(errors(&first).is_empty(), "{:?}", errors(&first));
        assert!(first.memo_counts.runs.iter().any(|r| r.op == "import_dxf" && r.feature == "site"), "{:?}", first.memo_counts.runs);
        let warns = |ctx: &Context| -> Vec<String> {
            ctx.diagnostics.iter().filter(|d| d.severity == cadpole_core::context::Severity::Warning).map(|d| d.message.clone()).collect()
        };
        let w1 = warns(&first);
        assert!(w1.iter().any(|m| m.contains("\"SHRUB\"") && m.contains("does not define")), "{w1:?}");
        let site = first.features.iter().find(|f| f.id.0 == "site").unwrap();
        assert_eq!(site.kind, "import_dxf");
        assert_eq!(site.args["unit"], "ft");
        assert_eq!(site.args["entities"], "46");
        assert_eq!(site.args["layer_names"], "PL, CONTOUR, SPOT, TREE");
        assert_eq!(site.args["texts"], "32");
        assert_eq!(site.args["blocks"], "1");
        assert_eq!(site.args["inserts"], "2");
        let infos: Vec<&str> = site.diagnostics.iter().map(|d| d.message.as_str()).collect();
        assert!(infos.iter().any(|m| m.contains("import_dxf: 46 entities on 4 layers") && m.contains("unit ft ($INSUNITS)")), "{infos:?}");
        assert!(infos.iter().any(|m| m.contains("$INSUNITS says feet")), "{infos:?}");
        // The second evaluation parses nothing and says the same things.
        let second = run(Some(memo));
        assert!(errors(&second).is_empty(), "{:?}", errors(&second));
        assert!(second.memo_counts.hits.iter().any(|r| r.op == "import_dxf" && r.feature == "site"), "{:?}", second.memo_counts.hits);
        assert!(!second.memo_counts.runs.iter().any(|r| r.op == "import_dxf"), "{:?}", second.memo_counts.runs);
        assert_eq!(warns(&second), w1, "a memo hit warns exactly as the run did");
        let site2 = second.features.iter().find(|f| f.id.0 == "site").unwrap();
        assert_eq!(site2.args["entities"], "46");
        assert_eq!(site2.args["texts"], "32");
    }

    #[test]
    fn import_dxf_refuses_by_name() {
        let cases: &[(&str, &str)] = &[
            ("import_dxf(\"scan.stl\")", "is a STL mesh: use import_mesh"),
            ("import_step(\"site.dxf\")", "is a DXF drawing: use import_dxf"),
            ("import_mesh(\"site.dxf\")", "is a DXF drawing: use import_dxf"),
            ("import_dxf(42)", "must be an asset(...) or the asset's name"),
            ("import_dxf(\"nothere.dxf\")", "no asset named"),
            ("import_dxf(\"site.dxf\", units=\"furlong\")", "valid: auto, mm, cm, m, inch, ft, yd, survey_ft"),
            ("import_dxf(\"site.dxf\", layers=[\"ROADS\"])", "matches no layer; the file has: \"PL\""),
            ("import_dxf(\"site.dxf\", layers=3)", "layer= takes a layer name"),
            ("import_dxf(\"site.dxf\").sketch(layer=\"NOPE\")", "matches no imported layer; the drawing has: PL, CONTOUR, SPOT, TREE"),
            ("import_dxf(\"site.dxf\", layers=[\"PL\"]).contours(layer=\"CONTOUR\")", "matches no imported layer; the drawing has: PL"),
            ("import_dxf(\"site.dxf\").spot_elevations(pattern=\"EL\")", "has no `#`"),
            ("import_dxf(\"site.dxf\").sketch(align=\"top\")", "must be \"none\", \"min\" or \"center\""),
            ("import_dxf(\"site.dxf\").sketch(tolerance=-1)", "must be a positive number"),
            ("import_dxf(\"site.dxf\", units=dxf_units.parsecs)", "not a member of dxf_units"),
            ("loft([import_dxf(\"site.dxf\"), 3], name=\"x\")", "section 1"),
        ];
        for (call, want) in cases {
            let ctx = run_with_assets(call, &[("scan.stl", &box_stl(false)), ("site.dxf", &survey_dxf())]);
            let errs = errors(&ctx);
            assert!(errs.iter().any(|e| e.contains(want)), "{call}: {errs:?}\n  wanted {want:?}");
        }
        // A unitless file is a warning that says how to settle it, and
        // `units=` settles it with no unit warning (the file's undefined
        // SHRUB block still warns, as it should).
        let unitless = String::from_utf8(survey_dxf()).unwrap().replace("70\n2\n", "70\n0\n");
        let ctx = run_with_assets("s = import_dxf(\"site.dxf\", name=\"s\")\nassert_eq(s.units, \"mm\")\nassert_eq(s.unit_source, \"assumed\")", &[("site.dxf", unitless.as_bytes())]);
        assert!(errors(&ctx).is_empty(), "{:?}", errors(&ctx));
        assert!(ctx.diagnostics.iter().any(|d| d.severity == cadpole_core::context::Severity::Warning && d.message.contains("units=dxf_units.ft")), "{:?}", ctx.diagnostics);
        let ctx = run_with_assets(
            "s = import_dxf(\"site.dxf\", units=dxf_units.survey_ft, name=\"s\")\nassert_eq(s.units, \"survey_ft\")\nassert_eq(s.unit_source, \"units=\")\nassert_close(s.scale, 304.8006096, tol=1e-6)",
            &[("site.dxf", unitless.as_bytes())],
        );
        assert!(errors(&ctx).is_empty(), "{:?}", errors(&ctx));
        assert!(
            !ctx.diagnostics.iter().any(|d| d.severity == cadpole_core::context::Severity::Warning && d.message.contains("unit")),
            "{:?}",
            ctx.diagnostics
        );
        // The string form of every member is accepted too, `"in"` included.
        let ctx = run_with_assets("s = import_dxf(\"site.dxf\", units=\"in\", name=\"s\")\nassert_eq(s.units, \"inch\")\nassert_close(s.scale, 25.4)", &[("site.dxf", unitless.as_bytes())]);
        assert!(errors(&ctx).is_empty(), "{:?}", errors(&ctx));
    }

    // ------------------------------------------------- names through moves

    /// Through Starlark: a short local name keeps resolving after a move,
    /// a fillet and a boolean (docs/starlark-api.md: "a script can write
    /// `block.face("end")` before and after booleans"), and a moved tool's
    /// wall keeps the tool's name through the boolean.
    #[test]
    fn short_face_names_and_moved_operand_names_survive_derivation() {
        let ctx = run(r#"
b = box(20, 20, 10, name="blk")
for r in [translate(copy(b), (1, 2, 3)), rotate(copy(b), 30), fillet(copy(b).edges(), 1)]:
    assert_eq(r.face("top").name, "blk/top")
    assert_eq(r.face("end").name, "blk/top")
drill = translate(cylinder(3, 12, at=(0, 0, -1), name="drill"), (2, 1, 0))
part = subtract(b, drill, name="part")
assert_eq(part.face("top").name, "blk/top")
assert_eq(part.face("drill/side#0").name, "drill/side#0")
assert_eq(sorted([f.name for f in part.faces() if f.name.startswith("drill/")]), ["drill/side#0", "drill/side#1"])
"#);
        assert!(errors(&ctx).is_empty(), "{:?}", errors(&ctx));
    }

    // ---------------------------------------------------- float printing

    /// `str()`, `repr()`, `print` and `%`-formatting of a float in scientific
    /// form round the mantissa as a whole. starlark 0.14.2's
    /// `write_scientific` wrote the leading digit before rounding the six
    /// after it, so a tail that rounded up to 1000000 lost its carry and
    /// 1999999.7 printed as `1e+06`, half its value
    /// (build_support/patches/starlark_float_carry.patch).
    #[test]
    fn a_float_prints_with_its_rounding_carry() {
        let ctx = run(r#"
assert_eq(str(1999999.7), "2e+06")
assert_eq(repr(1999999.9), "2e+06")
assert_eq(str(9999999.7), "1e+07")
assert_eq(str(99999999.0), "1e+08")
assert_eq(str(2999999.9999999995), "3e+06")
assert_eq(str(-1999999.7), "-2e+06")
assert_eq(str(1.9999999e20), "2e+20")
assert_eq(str(0.0000099999999), "1e-05")
assert_eq("%s" % 1999999.7, "2e+06")
assert_eq("%g" % 1999999.7, "2e+06")
assert_eq("%e" % 1999999.7, "2.000000e+06")
assert_eq("{}".format(1999999.7), "2e+06")
# What printed right before still does.
assert_eq(str(1999999.4), "1.999999e+06")
assert_eq(str(1234567.0), "1.234567e+06")
assert_eq(str(123456789.0), "1.234568e+08")
assert_eq(str(2000000.0), "2e+06")
assert_eq(str(999999.96), "999999.96")
assert_eq(str(0.0), "0.0")
assert_eq("%e" % 0.0, "0.000000e+00")
assert_eq("%e" % 1.5e-7, "1.500000e-07")
assert_eq(str(1e300), "1e+300")
assert_eq(str(-3.14e-145), "-3.14e-145")
assert_eq("%e" % -0.0, "-0.000000e+00")
assert_eq(str(1.23e45), "1.23e+45")
"#);
        assert!(errors(&ctx).is_empty(), "{:?}", errors(&ctx));
    }

    // ------------------------------------------------------ param overrides

    fn warnings(ctx: &Context) -> Vec<String> {
        of_severity(ctx, cadpole_core::context::Severity::Warning).into_iter().map(|(m, _)| m).collect()
    }

    /// `-p nosuch=3` is a typo or a stale name; it used to vanish without a
    /// word. A declared one is quiet, and so is the model.
    #[test]
    fn an_override_no_param_declares_is_a_warning() {
        let src = "w = param(\"w\", 10)\nh = param(\"h\", 5)\nbox(w, w, h)\n";
        let ctx = run_with(src, "widht", 80.0);
        assert!(errors(&ctx).is_empty(), "{:?}", errors(&ctx));
        let w = warnings(&ctx);
        assert_eq!(w.len(), 1, "{w:?}");
        assert!(w[0].contains("\"widht\"") && w[0].contains("params: h, w"), "{w:?}");
        let ctx = run_with(src, "w", 80.0);
        assert!(warnings(&ctx).is_empty(), "{:?}", warnings(&ctx));
    }

    /// Through the script: `-p s=2` on a string param is the text `2`.
    #[test]
    fn a_string_param_takes_a_numeric_looking_override() {
        let ctx = run_with("s = param(\"s\", \"1\", choices=[\"1\", \"2\"])\nassert_eq(s, \"2\")\n", "s", 2.0);
        assert!(errors(&ctx).is_empty(), "{:?}", errors(&ctx));
        assert!(warnings(&ctx).is_empty(), "{:?}", warnings(&ctx));
    }

    /// The redeclaration error is one sentence: the source literal carried a
    /// run of indentation spaces in the middle of it.
    #[test]
    fn a_layer_redeclaration_error_reads_as_one_sentence() {
        let ctx = run("layer(\"x\")\nlayer(\"x\", visible=False)\n");
        let errs = errors(&ctx);
        assert_eq!(errs.len(), 1, "{errs:?}");
        assert!(errs[0].contains("a layer is declared once, and a second declaration must repeat"), "{errs:?}");
        assert!(!errs[0].contains("  "), "no run of spaces: {errs:?}");
    }

    /// `print_plan` plates the model as it is written: the lid sitting on
    /// its box is turned over onto its top beside it, the copies go on a
    /// hidden `print` layer with their paint, and the plates are recorded
    /// as Bambu exports under their names.
    #[test]
    fn print_plan_plates_a_lid_and_its_box() {
        let ctx = run(
            "base = subtract(box(60, 40, 20), box(56, 36, 20, at=(2, 2, 2)), name=\"base\")\n\
             lid = subtract(box(60, 40, 6, at=(0, 0, 20)), box(56, 36, 4, at=(2, 2, 20)), name=\"lid\")\n\
             base.material(color=\"#2F6F8F\")\n\
             paint(lid, faces=lid.faces(normal=(0, 0, 1)), color=\"#FFCE1F\")\n\
             plan = print_plan([base, lid], \"box.3mf\", printer=\"bambu_a1\")\n\
             assert_eq([p[\"name\"] for p in plan[\"plates\"]], [\"Parts\"])\n\
             assert_eq(sorted([p[\"name\"] for p in plan[\"pieces\"]]), [\"base\", \"lid\"])\n\
             lp = [p for p in plan[\"pieces\"] if p[\"name\"] == \"lid\"][0]\n\
             assert_eq(lp[\"support_area\"], 0.0)\n\
             assert_true(lp[\"modelled_support_area\"] > 1000, msg=lp[\"reason\"])\n\
             assert_eq(len(plan[\"bodies\"]), 2)\n\
             assert_true(\"lid:\" in plan[\"report\"])\n",
        );
        assert!(errors(&ctx).is_empty(), "{:?}", errors(&ctx));
        let specs: Vec<_> = ctx.exports.iter().filter(|e| e.path == "box.3mf").collect();
        assert_eq!(specs.len(), 2);
        assert!(specs.iter().all(|e| e.flavor == cadpole_core::export::Flavor::Bambu && e.printer == "bambu_a1"));
        let print = ctx.layers.iter().find(|l| l.name == "print").expect("the print layer is declared");
        assert!(!print.visible, "and hidden");
        for e in &specs {
            for id in &e.bodies {
                assert_eq!(ctx.body(*id).unwrap().layer, "print");
            }
        }
        // The lid's copy carries its paint.
        let lid_copy = specs.iter().find(|e| e.name.as_deref() == Some("lid")).unwrap().bodies[0];
        let id = ctx.body(lid_copy).unwrap().id_string();
        assert!(cadpole_core::paint::paints(&ctx).iter().any(|p| p.body_id == id && p.hex == "#FFCE1F"));
    }

    /// The pins a piece can carry are checked at the call.
    #[test]
    fn print_part_refuses_what_it_cannot_mean() {
        for (call, want) in [
            ("print_part(b, orient=\"sideways\")", "orient= is \"auto\" or \"keep\""),
            ("print_part(b, orient=\"keep\", down=\"-x\")", "pass one"),
            ("print_part(b, down=\"sideways\")", "not a direction"),
            ("print_part(b, down=(0, 0, 0))", "has no direction"),
            ("print_part(b, spin=\"a lot\")", "spin= is an angle"),
            ("print_part(b, plate=0)", "plate= is 1 or more"),
            ("print_plan([b], \"x.stl\")", "is not a .3mf"),
            ("print_plan([b], variants=[\"sepia\"])", "is not \"colour\" or \"mono\""),
            ("print_plan([b], printer=\"nope\")", "unknown printer"),
        ] {
            let ctx = run(&format!("b = box(10, 10, 10)\n{call}\n"));
            let e = errors(&ctx);
            assert!(e.iter().any(|m| m.contains(want)), "{call}: {e:?}");
        }
    }

    /// `assert_orientation_optimal` passes a part lying flat and fails one
    /// standing on its end, naming the better way and the saving.
    #[test]
    fn assert_orientation_optimal_names_the_better_orientation() {
        let ctx = run("assert_orientation_optimal(box(80, 10, 10, name=\"bar\"))\n");
        assert!(errors(&ctx).is_empty(), "{:?}", errors(&ctx));
        let ctx = run("assert_orientation_optimal(box(10, 10, 80, name=\"post\"), msg=\"the post\")\n");
        let e = errors(&ctx);
        let m = e.iter().find(|m| m.contains("assert_orientation_optimal failed")).unwrap_or_else(|| panic!("{e:?}"));
        assert!(m.contains("(the post)") && m.contains("post: as placed it needs") && m.contains("better"), "{m}");
        let ctx = run("assert_orientation_optimal(box(10, 10, 80, name=\"post\"), tolerance=1000)\n");
        assert!(errors(&ctx).is_empty(), "{:?}", errors(&ctx));
    }
}
