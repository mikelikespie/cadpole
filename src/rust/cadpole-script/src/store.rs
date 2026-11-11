//! Shared mutable state handed to native functions through `Evaluator::extra`.

use std::cell::{Cell, RefCell};
use std::sync::Arc;

use cadpole_assets::AssetStore;
use cadpole_core::Context;
use cadpole_core::context::{Diagnostic, Span};
use starlark::any::ProvidesStaticType;
use starlark::eval::Evaluator;

use crate::constrained::CSketch;
use crate::limits::{Deadline, Limits};
use crate::modeltest::{Registered, TestMode, TestOutcome};
use crate::sheets::{SheetExport, SweepRequest, SweepRow};

#[derive(ProvidesStaticType)]
pub struct Store {
    pub ctx: RefCell<Context>,
    /// Where `asset("name")` gets its bytes. Immutable for the whole
    /// evaluation: a model that saw different bytes half way through would
    /// not be reproducible.
    pub assets: Arc<dyn AssetStore>,
    /// Constrained sketches, by index. The Starlark value is a handle into
    /// this table rather than the sketch itself, because building one is
    /// stateful — `s.line(a, b)` adds to `s` — while Starlark values are
    /// immutable snapshots.
    pub sketches: RefCell<Vec<CSketch>>,
    /// `opt.problem()`s being built, by index: an `OptProblem`
    /// value is a handle into this table, for the same reason a
    /// constrained sketch is.
    pub problems: RefCell<Vec<crate::opt::ProblemState>>,
    /// Files `export_svg` / `export_dxf` asked for, already rendered. The
    /// script never writes them; `evaluate_full` hands them to the host, which
    /// decides where they go.
    pub files: RefCell<Vec<crate::drawing::OutputFile>>,
    /// The evaluation's wall-clock budget (docs/security.md). Checked on the
    /// way into every Cadpole builtin, which is what bounds a script whose time
    /// goes into the kernel rather than into Starlark.
    pub deadline: Deadline,
    pub limits: Limits,
    /// `print()` lines already recorded, so the cap is enforced across files.
    printed: std::cell::Cell<usize>,
    /// The tests `test(name, fn, ...)` declared, in declaration order, and
    /// what this evaluation does with them (`modeltest`). Declarations are
    /// always recorded; whether anything runs is the mode's decision.
    pub tests: RefCell<Vec<Registered>>,
    pub test_mode: TestMode,
    pub outcomes: RefCell<Vec<TestOutcome>>,
    /// Set while a test function runs, so `test()` inside one is refused
    /// rather than registered into a run that is already under way.
    pub in_test: Cell<bool>,
    /// The sheet sets the script built, by index; a `SheetSet`
    /// value is a handle into this table, because placing a view on a
    /// sheet is stateful the way a constrained sketch is.
    pub sheet_sets: RefCell<Vec<cadpole_drawing::SheetSet>>,
    /// `export(set, path)` calls: written by the host under
    /// `--run-exports`, never here.
    pub sheet_exports: RefCell<Vec<SheetExport>>,
    /// `report(name, value)` calls, in order: what a sweep row carries.
    pub reports: RefCell<Vec<(String, cadpole_core::ParamValue)>>,
    /// `sweep_table(...)` calls: the configurations the host is asked to
    /// evaluate the document at.
    pub sweep_requests: RefCell<Vec<SweepRequest>>,
    /// The rows a host that ran those configurations hands back, keyed by
    /// the request's index; `None` in an evaluation with no host support,
    /// where every table is pending.
    pub sweep_results: Option<Arc<Vec<Vec<SweepRow>>>>,
}

impl Store {
    /// A store for one evaluation, with explicit sandbox limits.
    pub fn with_limits(ctx: Context, assets: Arc<dyn AssetStore>, limits: Limits) -> Store {
        let limits = limits.or_defaults();
        Store {
            ctx: RefCell::new(ctx),
            assets,
            sketches: RefCell::new(Vec::new()),
            problems: RefCell::new(Vec::new()),
            files: RefCell::new(Vec::new()),
            deadline: Deadline::new(limits.deadline_ms),
            limits,
            printed: std::cell::Cell::new(0),
            tests: RefCell::new(Vec::new()),
            test_mode: TestMode::Skip,
            outcomes: RefCell::new(Vec::new()),
            in_test: Cell::new(false),
            sheet_sets: RefCell::new(Vec::new()),
            sheet_exports: RefCell::new(Vec::new()),
            reports: RefCell::new(Vec::new()),
            sweep_requests: RefCell::new(Vec::new()),
            sweep_results: None,
        }
    }

    /// The rows of the sweeps a host ran before this evaluation.
    pub fn with_sweep_results(mut self, results: Option<Arc<Vec<Vec<SweepRow>>>>) -> Store {
        self.sweep_results = results;
        self
    }

    /// What this evaluation does with the script's tests.
    pub fn with_tests(mut self, mode: TestMode) -> Store {
        self.test_mode = mode;
        self
    }

    /// Everything an evaluation leaves behind.
    pub fn finish(self) -> Finished {
        Finished {
            ctx: self.ctx.into_inner(),
            files: self.files.into_inner(),
            tests: self.tests.into_inner(),
            outcomes: self.outcomes.into_inner(),
            sheet_sets: self.sheet_sets.into_inner(),
            sheet_exports: self.sheet_exports.into_inner(),
            reports: self.reports.into_inner(),
            sweep_requests: self.sweep_requests.into_inner(),
        }
    }
}

/// What [`Store::finish`] hands back.
pub struct Finished {
    pub ctx: Context,
    pub files: Vec<crate::drawing::OutputFile>,
    pub tests: Vec<Registered>,
    pub outcomes: Vec<TestOutcome>,
    pub sheet_sets: Vec<cadpole_drawing::SheetSet>,
    pub sheet_exports: Vec<SheetExport>,
    pub reports: Vec<(String, cadpole_core::ParamValue)>,
    pub sweep_requests: Vec<SweepRequest>,
}

/// `print()` from a script becomes an info diagnostic rather than a line on
/// the server's stdout: the caller is usually an assistant or a browser, and
/// neither can see stderr. Capped so a `print` in a loop cannot fill memory.
impl starlark::PrintHandler for Store {
    fn println(&self, text: &str) -> starlark::Result<()> {
        let n = self.printed.get();
        self.printed.set(n + 1);
        if n > self.limits.max_print_lines {
            return Ok(());
        }
        if let Ok(mut ctx) = self.ctx.try_borrow_mut() {
            if n == self.limits.max_print_lines {
                ctx.diagnostics.push(Diagnostic::info(format!(
                    "print: more than {} lines; the rest are dropped",
                    self.limits.max_print_lines
                )));
            } else {
                ctx.diagnostics.push(Diagnostic::info(format!("print: {text}")));
            }
        }
        Ok(())
    }
}

/// Fetch the store from an evaluator. Native functions are only ever called
/// through `evaluate`, which always installs one.
///
/// This is also where the wall-clock budget is enforced for scripts whose time
/// goes into the kernel: every Cadpole builtin starts here, so a deadline that
/// passed during the previous boolean stops the next one.
pub fn store<'a>(eval: &Evaluator<'_, 'a, '_>) -> anyhow::Result<&'a Store> {
    let store = eval
        .extra
        .ok_or_else(|| anyhow::anyhow!("cadpole functions need an evaluation context"))?
        .downcast_ref::<Store>()
        .ok_or_else(|| anyhow::anyhow!("evaluation context has the wrong type"))?;
    store.deadline.check()?;
    Ok(store)
}

/// Source span of the innermost Starlark call, for feature bookkeeping.
pub fn call_span(eval: &Evaluator<'_, '_, '_>) -> Option<Span> {
    let fs = eval.call_stack_top_location()?;
    let r = fs.resolve_span();
    Some(Span {
        path: fs.filename().to_owned(),
        start_line: r.begin.line as u32 + 1,
        start_col: r.begin.column as u32 + 1,
        end_line: r.end.line as u32 + 1,
        end_col: r.end.column as u32 + 1,
    })
}
