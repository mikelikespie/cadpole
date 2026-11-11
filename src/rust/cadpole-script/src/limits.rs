//! What bounds a script (docs/security.md, "Starlark sandbox").
//!
//! `starlark-rust` is hermetic by construction — [`Globals::standard`] has no
//! I/O, no `import` and no environment access — but hermetic is not the same
//! as bounded: `for i in range(10**9)` is a perfectly well-behaved Starlark
//! program that never finishes. These are the limits that make a runaway
//! script a diagnostic instead of a hung server.
//!
//! | Limit | Mechanism | Default |
//! | --- | --- | --- |
//! | wall clock | [`Deadline`], polled by `Evaluator::set_check_cancelled` and checked by every Cadpole builtin | off in the library; 20 s in `cadpole serve`, 120 s in the CLI |
//! | steps | `Evaluator::set_max_tick_count` — one tick per function call and per loop back-edge | 200 M |
//! | recursion | `Evaluator::set_max_callstack_size` | 200 frames |
//! | memory | `Evaluator::set_max_heap_size` | 512 MB |
//! | `print` | a [`starlark::PrintHandler`] that appends info diagnostics | 200 lines |
//! | nesting | [`crate::nesting`], a scan of the source before it is parsed | 100 levels |
//! | `load()` depth | `Loader::eval_file` refuses a chain longer than the cap | 64 files |
//! | native stack | evaluation runs on its own thread with a 64 MB stack | 64 MB |
//!
//! The last three are about the *native* stack, which none of the others
//! touch: starlark's parser, compiler and evaluator recurse on it once per
//! level of nesting, and a native stack overflow is not a panic but an abort
//! of the whole process. The nesting and load caps make the depth a
//! diagnostic; the big stack makes the CLI, the server's blocking workers and
//! an embedding host all behave the same at that depth.
//!
//! **What the deadline does and does not bound.** starlark-rust reports
//! forward progress on every call and every loop back-edge and runs the
//! cancellation check once per thousand of those, so any *Starlark* loop is
//! interrupted within roughly a thousand iterations. Cadpole's own builtins check
//! it too, at the top of every call, which stops a script that runs a hundred
//! booleans in a loop. What nothing here can do is interrupt one native
//! operation that is already running: a single boolean that takes 15 s runs to
//! completion and the deadline fires at the next check. The honest bound is
//! therefore "budget plus one kernel operation". Interrupting mid-kernel needs
//! either cooperative checks inside `cadpole-core`'s kernel (which is fenced) or
//! an OS-level stop — a child process with rlimits, which is the shape
//! `docs/security.md` records as the long-term answer.
//!
//! **On wasm** [`cadpole_core::time::Stopwatch`] always reads zero (there is no
//! clock in `wasm32-unknown-unknown`), so the deadline never fires there; the
//! tick, heap and callstack limits still do, and the browser's answer to a
//! runaway script is to terminate the Worker.

use std::cell::Cell;

use cadpole_core::time::Stopwatch;

/// Everything that bounds one evaluation.
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub struct Limits {
    /// Wall-clock budget in milliseconds; 0 means no deadline.
    pub deadline_ms: u64,
    /// Function calls plus loop back-edges; 0 means no cap.
    pub max_ticks: u64,
    /// Deepest Starlark call stack, in frames; 0 means the interpreter's own.
    pub max_callstack: usize,
    /// Starlark heap cap in bytes; 0 means no cap.
    pub max_heap_bytes: usize,
    /// `print()` lines kept as diagnostics; further lines are dropped and the
    /// last one says so.
    pub max_print_lines: usize,
    /// Whether a solve this evaluation cannot finish inside the deadline --
    /// or one the script asks to run as a job -- may be handed to a job that
    /// runs past it: set by a host with a job runner for the
    /// evaluations its owner asks for, off for everything else (the limited
    /// listener, a render, a sweep, the browser's engine, a plain `cadpole
    /// eval`). Off, such a solve is reported pending and no job starts.
    pub solve_jobs: bool,
}

impl Limits {
    pub const DEFAULT_CALLSTACK: usize = 200;
    pub const DEFAULT_HEAP_BYTES: usize = 512 * 1024 * 1024;
    /// A tick is a call or a loop back-edge. `examples/gridfinity_bin.star`
    /// uses about 40 000, so this is four orders of magnitude of headroom and
    /// still stops an infinite loop in seconds.
    pub const DEFAULT_TICKS: u64 = 200_000_000;
    pub const DEFAULT_PRINT_LINES: usize = 200;
    /// How deeply one script may nest, as [`crate::nesting`] measures it:
    /// brackets, operators, lambdas, `elif` chains and indented blocks
    /// combined. Every level costs the parser, the compiler and the evaluator
    /// a few kilobytes of native stack each — up to 8 KB for a nested
    /// `lambda` — so a hundred levels is under a megabyte on the worst shape,
    /// which fits a 2 MB thread, while real scripts sit under twenty.
    pub const MAX_NESTING: usize = 100;
    /// The longest chain of `load()`s: each level is a whole `Evaluator` on
    /// the native stack (about 5 KB), and a chain of 1500 aborted the server.
    pub const MAX_LOAD_DEPTH: usize = 64;
    /// Evaluations one `test(..., sweep=)` may ask for. Every sweep point
    /// rebuilds the whole model, so this is the same kind of bound as the
    /// sweep-cell cap on `cadpole sweep`: a study bigger than this is a job,
    /// not a test.
    pub const MAX_SWEEP_POINTS: usize = 256;
    /// Configurations the `sweep_table()` calls of one document may ask the
    /// host to evaluate, every call together. Each is a whole
    /// evaluation of the document before its sheets are written, so a cover
    /// table is a handful of rows, not a study.
    pub const MAX_SWEEP_TABLE_POINTS: usize = 64;
    /// Most copies one `pattern_linear` / `pattern_circular` may make. The
    /// transforms are sized by this number before anything else runs, and
    /// `count=2147483647` was a 274 GB allocation that aborted the server;
    /// ten thousand copies is ten thousand bodies, which is already a model
    /// nothing can render.
    pub const MAX_PATTERN_COUNT: usize = 10_000;
    /// Most sides of one `ngon`: `ngon(2000000000, r)` was a 32 GB point
    /// list. Past a few hundred sides a polygon is a circle.
    pub const MAX_NGON_SIDES: usize = 10_000;
    /// Most characters one `text()` call shapes. Shaping plus the union of
    /// every glyph outline is one native operation the deadline cannot
    /// interrupt, and its cost grows faster than the string: 2 000
    /// characters took 2.4 s of CPU, 8 000 took 55 s, 32 000 ran for
    /// minutes. A label is a few dozen characters; a paragraph is a few
    /// hundred.
    pub const MAX_TEXT_CHARS: usize = 2_048;
    /// Most variables, constraints and nonzero coefficients one
    /// `opt.problem()` may hold. A floor-plan MILP is thousands of
    /// variables and tens of thousands of rows; these leave two orders of
    /// magnitude above that and still bound what a runaway loop can
    /// allocate outside the Starlark heap (the model is Rust memory, which
    /// the heap cap does not see): 2.5 M coefficients is ~30 MB per copy.
    pub const MAX_OPT_VARIABLES: usize = 250_000;
    pub const MAX_OPT_CONSTRAINTS: usize = 500_000;
    pub const MAX_OPT_NONZEROS: usize = 2_500_000;
    /// Most solutions one `solve(count=)` may ask for: each is another
    /// solve of the whole problem.
    pub const MAX_OPT_SOLUTIONS: usize = 1_000;

    /// No deadline, but the structural caps on. This is the library default:
    /// a caller that has not thought about time (a test, a one-shot `cadpole
    /// eval`) still cannot blow the heap or recurse forever.
    pub fn structural() -> Limits {
        Limits {
            deadline_ms: 0,
            max_ticks: Limits::DEFAULT_TICKS,
            max_callstack: Limits::DEFAULT_CALLSTACK,
            max_heap_bytes: Limits::DEFAULT_HEAP_BYTES,
            max_print_lines: Limits::DEFAULT_PRINT_LINES,
            solve_jobs: false,
        }
    }

    /// What `cadpole serve` uses: somebody is waiting at a keyboard.
    pub fn interactive() -> Limits {
        Limits { deadline_ms: 20_000, ..Limits::structural() }
    }

    /// What the CLI and sweeps use: slower is acceptable, forever is not.
    pub fn batch() -> Limits {
        Limits { deadline_ms: 120_000, ..Limits::structural() }
    }

    /// Nothing is bounded. Only for benchmarks that mean it.
    pub fn unbounded() -> Limits {
        Limits { deadline_ms: 0, max_ticks: 0, max_callstack: 0, max_heap_bytes: 0, max_print_lines: usize::MAX, solve_jobs: false }
    }

    /// `self` with any zero field filled in from [`Limits::structural`], so a
    /// request that sets only `deadline_ms` still gets the structural caps.
    pub fn or_defaults(self) -> Limits {
        let d = Limits::structural();
        Limits {
            deadline_ms: self.deadline_ms,
            max_ticks: if self.max_ticks == 0 { d.max_ticks } else { self.max_ticks },
            max_callstack: if self.max_callstack == 0 { d.max_callstack } else { self.max_callstack },
            max_heap_bytes: if self.max_heap_bytes == 0 { d.max_heap_bytes } else { self.max_heap_bytes },
            max_print_lines: if self.max_print_lines == 0 { d.max_print_lines } else { self.max_print_lines },
            solve_jobs: self.solve_jobs,
        }
    }
}

impl Default for Limits {
    fn default() -> Limits {
        Limits::structural()
    }
}

/// One evaluation's clock. Cheap to poll: a monotonic read and a compare.
///
/// `fired` is sticky so the caller can tell a deadline from any other error
/// after the fact — starlark reports a cancellation as "Evaluation cancelled",
/// which would be a mystery on its own.
#[derive(Debug)]
pub struct Deadline {
    clock: Stopwatch,
    budget_ms: u64,
    fired: Cell<bool>,
}

impl Deadline {
    /// A deadline `budget_ms` from now; 0 means "never expires".
    pub fn new(budget_ms: u64) -> Deadline {
        Deadline { clock: Stopwatch::start(), budget_ms, fired: Cell::new(false) }
    }

    pub fn budget_ms(&self) -> u64 {
        self.budget_ms
    }

    pub fn elapsed_ms(&self) -> f64 {
        self.clock.elapsed_ms()
    }

    /// True once the budget has passed. Sticky: it stays true afterwards.
    pub fn expired(&self) -> bool {
        if self.fired.get() {
            return true;
        }
        if self.budget_ms == 0 {
            return false;
        }
        if self.clock.elapsed_ms() >= self.budget_ms as f64 {
            self.fired.set(true);
            return true;
        }
        false
    }

    /// True when this deadline is what stopped the evaluation.
    pub fn fired(&self) -> bool {
        self.fired.get()
    }

    /// The message a user or an assistant reads. Says what to do next, since
    /// "it timed out" on its own is not actionable.
    ///
    /// It names where each budget comes from rather than one flag: the budget
    /// is raisable on `cadpole serve` (`--eval-deadline-ms`) and in an
    /// `EvaluateRequest`, but `cadpole eval`, `render`, `export` and `sweep` have
    /// a fixed 120 s and no flag at all, so the old advice to "raise the
    /// budget with --eval-deadline-ms" could not be followed by the reader
    /// most likely to see it ('unexpected argument' is what they got).
    pub fn message(&self) -> String {
        format!(
            "evaluation stopped after {:.0} ms: over the {} ms budget. \
             Simplify the script (fewer booleans, a coarser --boolean-tolerance). \
             The budget is `cadpole serve --eval-deadline-ms` or EvaluateRequest.deadline_ms; \
             the other CLI commands have a fixed one.",
            self.elapsed_ms(),
            self.budget_ms
        )
    }

    /// `Err` with [`Deadline::message`] once the budget has passed. This is
    /// what every Cadpole builtin calls before it does real work.
    pub fn check(&self) -> anyhow::Result<()> {
        if self.expired() {
            anyhow::bail!("{}", self.message());
        }
        Ok(())
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn a_zero_budget_never_expires() {
        let d = Deadline::new(0);
        assert!(!d.expired());
        assert!(!d.fired());
        assert!(d.check().is_ok());
    }

    #[test]
    fn an_elapsed_budget_is_sticky_and_explains_itself() {
        let d = Deadline::new(1);
        let mut acc = 0u64;
        while !d.expired() {
            acc = acc.wrapping_add(1);
            if acc > 5_000_000_000 {
                panic!("a 1 ms deadline must expire");
            }
        }
        assert!(d.fired(), "expiry is remembered");
        let e = d.check().unwrap_err().to_string();
        assert!(e.contains("1 ms budget"), "{e}");
        // Where the budget comes from, and that it is not a flag the reader
        // necessarily has: `cadpole eval` has no --eval-deadline-ms, so the old
        // advice to raise it there answered 'unexpected argument'.
        assert!(e.contains("cadpole serve --eval-deadline-ms"), "the message says where the budget is set: {e}");
        assert!(e.contains("the other CLI commands have a fixed one"), "{e}");
    }

    #[test]
    fn zero_fields_fall_back_to_the_structural_defaults() {
        let l = Limits { deadline_ms: 5_000, max_ticks: 0, max_callstack: 0, max_heap_bytes: 0, max_print_lines: 0, solve_jobs: false }
            .or_defaults();
        assert_eq!(l.deadline_ms, 5_000);
        assert_eq!(l.max_callstack, Limits::DEFAULT_CALLSTACK);
        assert_eq!(l.max_heap_bytes, Limits::DEFAULT_HEAP_BYTES);
        assert_eq!(l.max_ticks, Limits::DEFAULT_TICKS);
        assert_eq!(Limits::interactive().deadline_ms, 20_000);
        assert_eq!(Limits::batch().deadline_ms, 120_000);
        assert_eq!(Limits::unbounded().max_ticks, 0);
    }
}
