//! HiGHS, the LP and MILP solver behind MATLAB's `linprog` and
//! `intlinprog`, SciPy and JuMP, behind a safe API, as Cadpole's
//! [`cadpole_core::opt::Solver`].
//!
//! **Native only.** HiGHS is C++ from the BCR module `highs` 1.15.1 (patched:
//! `build_support/patches/highs_*.patch`). The CLI links this crate and calls
//! [`install`] before it does anything else, so `cadpole eval`, `cadpole test`,
//! `cadpole serve` and `cadpole mcp` can all solve. The wasm engine and the C FFI the
//! Apple apps link never depend on it, which is what keeps C++ out of them,
//! and a script there is told that solving needs native Cadpole.
//!
//! **Determinism.** The same model and options give the same answer, bit for
//! bit, on every run on one machine, and from every front end, because every
//! solve is configured the same way ([`configure`]): one thread, no output, a
//! fixed random seed, HiGHS's defaults for everything else. HiGHS's time
//! limit is the one input that depends on the clock, and it only ever changes
//! a solve it stops: the status is then [`Status::TimeLimit`], which the memo
//! never keeps. The node limit (`Options::max_nodes`, HiGHS's
//! `mip_max_nodes`) counts work instead, so a search it stops stops at the
//! same node with the same point every time. Ties are not decided by the model: between several optimal
//! corners HiGHS picks one by its own pivoting, which is fixed for one build
//! but may differ between the Linux and macOS builds (different compilers,
//! different libm), so a script should assert objectives and slacks, not a
//! tied corner.
//!
//! **Threads.** HiGHS 1.15 keeps its task scheduler per thread, so solves on
//! different threads of one process -- `cadpole serve` evaluates on worker
//! threads -- share nothing; `tests::solves_on_many_threads_agree_with_one`
//! runs them side by side and compares the bits. Nothing is serialised.

pub mod ffi;
pub mod highs;

use std::time::Instant;

use cadpole_core::opt::{Conflict, Model, Options, Solution, Solver, Status};

pub use highs::{Highs, RawSolution};

/// The random seed every solve uses. HiGHS's own default is 0 as well; it is
/// set explicitly so a change of default upstream cannot move an answer.
pub const RANDOM_SEED: i32 = 0;

/// Bumped when this crate changes how it drives HiGHS in a way that can
/// change an answer: the options in [`configure`], the cut used for distinct
/// solutions, the rounding of integer values.
///
/// 2: a `count=` list cut short by a limit ends with that limit's answer
/// (so lists version 1 filed as complete are not served again), an
/// unbounded MILP is told from an infeasible one, and an integer with no
/// whole number in its bounds is a conflict.
pub const DRIVER_VERSION: u32 = 2;

/// The solver as Cadpole installs it.
pub struct HighsSolver {
    identity: String,
}

impl HighsSolver {
    fn new() -> HighsSolver {
        HighsSolver {
            identity: format!("HiGHS {} (threads=1, random_seed={RANDOM_SEED}, cadpole-opt {DRIVER_VERSION})", Highs::version()),
        }
    }
}

impl Solver for HighsSolver {
    fn identity(&self) -> &str {
        &self.identity
    }

    fn solve(&self, model: &Model, options: &Options) -> Result<Vec<Solution>, String> {
        solve(model, options)
    }
}

/// The process's one HiGHS solver, for a host that installs something
/// around it -- `cadpole`'s solution cache and job runner -- rather
/// than HiGHS itself.
pub fn solver() -> &'static HighsSolver {
    static SOLVER: std::sync::OnceLock<HighsSolver> = std::sync::OnceLock::new();
    SOLVER.get_or_init(HighsSolver::new)
}

/// Install HiGHS as this process's solver (`cadpole_core::opt::install`).
/// Idempotent; every native front end calls it at start-up.
pub fn install() {
    cadpole_core::opt::install(solver());
}

/// Cadpole's fixed options, on a fresh instance: the part of an answer that is
/// not in the model or in [`Options`].
pub fn configure(h: &mut Highs, options: &Options) -> Result<(), String> {
    h.set_bool("output_flag", false)?;
    h.set_bool("log_to_console", false)?;
    h.set_int("threads", 1)?;
    h.set_string("parallel", "off")?;
    h.set_int("random_seed", RANDOM_SEED)?;
    // Tell infeasible from unbounded rather than reporting "one of the two".
    h.set_bool("allow_unbounded_or_infeasible", false)?;
    h.set_double("mip_rel_gap", options.mip_rel_gap)?;
    if let Some(n) = options.max_nodes {
        h.set_int("mip_max_nodes", i32::try_from(n).unwrap_or(i32::MAX))?;
    }
    Ok(())
}

fn check_options(model: &Model, options: &Options) -> Result<Vec<usize>, String> {
    if options.count == 0 {
        return Err("count must be at least 1".into());
    }
    if !(options.mip_rel_gap >= 0.0 && options.mip_rel_gap.is_finite()) {
        return Err(format!("the gap must be a number of at least 0, got {}", options.mip_rel_gap));
    }
    for t in [options.time_limit, options.deadline].into_iter().flatten() {
        if !(t > 0.0 && t.is_finite()) {
            return Err(format!("a time limit must be a positive number of seconds, got {t}"));
        }
    }
    for &(j, v) in &options.start {
        let c = model.columns.get(j).ok_or_else(|| format!("start: variable {j} does not exist"))?;
        if !v.is_finite() || v < c.lower || v > c.upper {
            return Err(format!("start: {:?} = {v} is outside its bounds [{}, {}]", c.name, c.lower, c.upper));
        }
    }
    for &j in &options.distinct {
        let c = model.columns.get(j).ok_or_else(|| format!("distinct: variable {j} does not exist"))?;
        if !c.is_binary() {
            return Err(format!("distinct: {:?} is not a binary variable; solutions can only be told apart by binaries", c.name));
        }
    }
    let distinct: Vec<usize> = if options.distinct.is_empty() {
        model.columns.iter().enumerate().filter(|(_, c)| c.is_binary()).map(|(j, _)| j).collect()
    } else {
        let mut d = options.distinct.clone();
        d.sort_unstable();
        d.dedup();
        d
    };
    if options.count > 1 && distinct.is_empty() {
        return Err("count > 1 needs binary variables for the solutions to differ in, and this problem has none".into());
    }
    Ok(distinct)
}

/// Solve `model`: the best solution first, then, while `options.count`
/// asks for more, the best one that differs from all before it in the
/// binaries `options.distinct` names (every binary when it is empty). Each
/// later one is found by adding a no-good cut over those binaries and solving
/// again, so the list stops early when no other solution exists. A search
/// for the next one that a limit (or anything but "none left") stops ends
/// the list with that answer, with or without a point, so a short list whose
/// statuses are all optimal is always every solution there is.
pub fn solve(model: &Model, options: &Options) -> Result<Vec<Solution>, String> {
    let clock = Instant::now();
    solve_on(model, options, &|| clock.elapsed().as_secs_f64())
}

/// [`solve`] against `elapsed`, the seconds since the call began: the clock
/// every time limit is measured on, which a test can stop or run out at will.
fn solve_on(model: &Model, options: &Options, elapsed: &dyn Fn() -> f64) -> Result<Vec<Solution>, String> {
    model.validate()?;
    let distinct = check_options(model, options)?;
    if model.columns.is_empty() {
        return Ok(vec![constant_model(model)]);
    }
    // A whole-number column fixed at a whole number is a constant, and solved
    // as a continuous column it is the same column. Solving it so means a
    // model whose integers are all fixed is an LP, with duals and a real
    // conflict: the ones local to that arrangement.
    let constants = |c: &cadpole_core::opt::Column| c.integer && c.lower == c.upper && c.lower.fract() == 0.0;
    let owned;
    let model = if model.columns.iter().any(constants) {
        let mut m = model.clone();
        for c in &mut m.columns {
            if constants(c) {
                c.integer = false;
            }
        }
        owned = m;
        &owned
    } else {
        model
    };
    let mut h = Highs::new();
    configure(&mut h, options)?;
    h.pass_model(model)?;
    if !options.start.is_empty() {
        h.set_start(&options.start)?;
    }
    let mut first = run_once(&mut h, model, options, elapsed, true)?;
    if first.status == Status::InfeasibleOrUnbounded && model.is_mip() {
        first = tell_apart(model, options, elapsed, first)?;
    }
    let mut out = vec![first];
    while out.len() < options.count {
        let last = out.last().expect("one solution at least");
        let (Status::Optimal, Some(x)) = (last.status, last.values.as_ref()) else { break };
        // sum over the zeros of x_j  -  sum over the ones of x_j  >=  1 - ones:
        // at least one of the named binaries flips.
        let mut terms = Vec::with_capacity(distinct.len());
        let mut ones = 0usize;
        for &j in &distinct {
            if x[j] > 0.5 {
                terms.push((j, -1.0));
                ones += 1;
            } else {
                terms.push((j, 1.0));
            }
        }
        h.add_row(1.0 - ones as f64, f64::INFINITY, &terms)?;
        let next = run_once(&mut h, model, options, elapsed, false)?;
        match next.status {
            Status::Optimal => out.push(next),
            // No other solution exists: the list is complete.
            Status::Infeasible | Status::InfeasibleOrUnbounded | Status::Unbounded => break,
            // Anything else stopped the search for the next one -- a limit,
            // an interrupt, an error -- and it goes on the end, point or
            // none. With a point it is still a different solution, and its
            // status says it may not be the best; without one it says the
            // list was cut short rather than exhausted. Either way the list
            // then holds a status that says whether it may be kept, so a
            // run the clock cut short is never kept as all there is.
            _ => {
                out.push(next);
                break;
            }
        }
    }
    Ok(out)
}

/// A model with no variables: every row is the constant 0.
fn constant_model(model: &Model) -> Solution {
    let violated: Vec<usize> = (0..model.rows.len()).filter(|&r| !(model.rows[r].lower <= 0.0 && 0.0 <= model.rows[r].upper)).collect();
    if violated.is_empty() {
        let mut s = Solution::without_point(Status::Optimal);
        s.values = Some(Vec::new());
        s.activities = Some(vec![0.0; model.rows.len()]);
        s.objective = Some(model.offset);
        if !model.is_mip() {
            s.row_duals = Some(vec![0.0; model.rows.len()]);
            s.reduced_costs = Some(Vec::new());
        }
        s
    } else {
        let mut s = Solution::without_point(Status::Infeasible);
        let side = |r: usize| if model.rows[r].lower > 0.0 { cadpole_core::opt::Side::Lower } else { cadpole_core::opt::Side::Upper };
        s.conflict = Some(Conflict { rows: vec![(violated[0], side(violated[0]))], columns: Vec::new() });
        s
    }
}

fn status_of(code: ffi::HighsInt) -> Status {
    match code {
        ffi::MODEL_OPTIMAL => Status::Optimal,
        ffi::MODEL_INFEASIBLE => Status::Infeasible,
        ffi::MODEL_UNBOUNDED => Status::Unbounded,
        ffi::MODEL_UNBOUNDED_OR_INFEASIBLE => Status::InfeasibleOrUnbounded,
        ffi::MODEL_TIME_LIMIT => Status::TimeLimit,
        // Cadpole sets `mip_max_nodes` and none of HiGHS's other solution limits
        // (leaves, improving solutions), so this one is the node limit.
        ffi::MODEL_SOLUTION_LIMIT => Status::NodeLimit,
        ffi::MODEL_ITERATION_LIMIT => Status::IterationLimit,
        ffi::MODEL_INTERRUPT => Status::Interrupted,
        _ => Status::Error,
    }
}

fn status_message(code: ffi::HighsInt) -> &'static str {
    match code {
        ffi::MODEL_NOTSET => "HiGHS did not set a status",
        ffi::MODEL_LOAD_ERROR => "HiGHS could not load the model",
        ffi::MODEL_MODEL_ERROR => "HiGHS found the model malformed",
        ffi::MODEL_PRESOLVE_ERROR => "HiGHS failed in presolve",
        ffi::MODEL_SOLVE_ERROR => "HiGHS failed while solving (numerical trouble)",
        ffi::MODEL_POSTSOLVE_ERROR => "HiGHS failed in postsolve",
        ffi::MODEL_EMPTY => "HiGHS was given an empty model",
        ffi::MODEL_OBJECTIVE_BOUND | ffi::MODEL_OBJECTIVE_TARGET => "HiGHS stopped at an objective bound Cadpole never sets",
        ffi::MODEL_UNKNOWN => "HiGHS ended in an unknown state",
        _ => "",
    }
}

/// How far from a whole number a bound may be and still count as one:
/// HiGHS's own `mip_feasibility_tolerance`.
const INTEGRALITY: f64 = 1e-6;

/// `-0.0` is a different bit pattern from `0.0` and prints as `-0`; nothing
/// Cadpole hands a script is ever the former.
fn unsign_zero(v: f64) -> f64 {
    if v == 0.0 { 0.0 } else { v }
}

/// One run of `h` as it stands, read back as a [`Solution`] of `model`.
/// `first` is the solve itself rather than a re-solve with a no-good cut: an
/// infeasible re-solve means "no other solution" and needs no conflict.
fn run_once(h: &mut Highs, model: &Model, options: &Options, elapsed: &dyn Fn() -> f64, first: bool) -> Result<Solution, String> {
    if let Some(limit) = options.effective_time_limit() {
        let left = limit - elapsed();
        if left <= 0.0 {
            return Ok(Solution::without_point(Status::TimeLimit));
        }
        h.set_double("time_limit", left)?;
    }
    h.run()?;
    let code = h.model_status();
    let status = status_of(code);
    let mip = model.is_mip();
    let mut sol = Solution::without_point(status);
    sol.message = if status == Status::Error { status_message(code).to_owned() } else { String::new() };
    sol.nodes = if mip { h.mip_nodes() } else { 0 };
    sol.iterations = h.simplex_iterations();
    let has_point =
        matches!(status, Status::Optimal | Status::TimeLimit | Status::NodeLimit | Status::IterationLimit | Status::Interrupted);
    if has_point && h.has_primal_solution() {
        let raw = h.solution()?;
        let mut x: Vec<f64> = raw.col_value[..model.columns.len()].to_vec();
        for (v, c) in x.iter_mut().zip(&model.columns) {
            if c.integer {
                *v = v.round();
            }
            *v = unsign_zero(*v);
        }
        sol.objective = Some(unsign_zero(model.objective_at(&x)));
        sol.activities = Some((0..model.rows.len()).map(|r| unsign_zero(model.activity(r, &x))).collect());
        if mip {
            sol.mip_gap = Some(unsign_zero(h.mip_gap()));
            sol.bound = h.mip_bound().map(unsign_zero);
        } else if status == Status::Optimal && h.has_dual_solution() {
            // HiGHS reports duals in the problem's own sense: the derivative
            // of the objective with respect to the binding bound, positive
            // when raising that bound raises the objective, whether it
            // minimises or maximises. `duals_are_derivatives_of_the_objective`
            // pins this down against finite differences.
            sol.row_duals = Some(raw.row_dual[..model.rows.len()].iter().map(|&d| unsign_zero(d)).collect());
            sol.reduced_costs = Some(raw.col_dual[..model.columns.len()].iter().map(|&d| unsign_zero(d)).collect());
        }
        sol.values = Some(x);
    }
    if first && status == Status::Infeasible {
        sol.conflict = Some(conflict(h, model, options, elapsed)?);
    }
    Ok(sol)
}

/// Infeasible or unbounded, for a MILP HiGHS left undecided between the
/// two. `allow_unbounded_or_infeasible = false` settles it for an LP, but
/// HiGHS's MIP solver still answers "one of the two" when the relaxation is
/// unbounded. A search for any feasible point (the objective left out) of
/// the same model decides: a point means the objective is what grows
/// without limit, so the answer is unbounded; none means infeasible, and
/// its conflict is found as for any infeasible MILP. If that search cannot
/// finish -- a limit stops it -- the answer stays undecided.
fn tell_apart(model: &Model, options: &Options, elapsed: &dyn Fn() -> f64, undecided: Solution) -> Result<Solution, String> {
    let mut feasibility = model.clone();
    for c in &mut feasibility.columns {
        c.cost = 0.0;
    }
    feasibility.offset = 0.0;
    let mut h = Highs::new();
    configure(&mut h, options)?;
    h.pass_model(&feasibility)?;
    let probe = run_once(&mut h, &feasibility, options, elapsed, false)?;
    let mut sol = match probe.status {
        Status::Optimal => Solution::without_point(Status::Unbounded),
        Status::Infeasible => {
            let mut s = Solution::without_point(Status::Infeasible);
            s.conflict = Some(conflict(&mut h, model, options, elapsed)?);
            s
        }
        // The probe could not settle it either, deterministically (a node or
        // iteration limit, its own "infeasible or unbounded"): the undecided
        // answer stands, and is as repeatable as before.
        s if s.is_repeatable() => return Ok(undecided),
        // Stopped by the clock, an interrupt or an error: whether the model
        // is unbounded or infeasible depended on how much time was left, so
        // the answer must not be kept as if it were the problem's.
        s => {
            let mut stopped = Solution::without_point(s);
            stopped.message = probe.message.clone();
            stopped.nodes = undecided.nodes + probe.nodes;
            stopped.iterations = undecided.iterations + probe.iterations;
            return Ok(stopped);
        }
    };
    sol.nodes = undecided.nodes + probe.nodes;
    sol.iterations = undecided.iterations + probe.iterations;
    Ok(sol)
}

/// The IIS of an infeasible model, within whatever is left of the time
/// limit: HiGHS's elastic filter, then its deletion filter.
///
/// HiGHS's IIS search is an LP search -- it starts from rows it assumes are
/// infeasible as an LP, and asserts as much -- so a MILP's conflict is its LP
/// relaxation's, found on an instance of its own, and only when that
/// relaxation is infeasible. When it is not, integrality is to blame: an
/// integer whose bounds hold no whole number is the conflict by itself, and
/// otherwise the conflict is empty (a script finds one by deletion). A
/// conflict HiGHS cannot finish is empty too.
fn conflict(h: &mut Highs, model: &Model, options: &Options, elapsed: &dyn Fn() -> f64) -> Result<Conflict, String> {
    let time_left = |h: &mut Highs| -> Result<(), String> {
        if let Some(limit) = options.effective_time_limit() {
            let left = (limit - elapsed()).max(1e-3);
            h.set_double("time_limit", left)?;
            h.set_double("iis_time_limit", left)?;
        }
        Ok(())
    };
    let strategy = ffi::IIS_FROM_LP | ffi::IIS_IRREDUCIBLE;
    if !model.is_mip() {
        h.set_int("iis_strategy", strategy)?;
        time_left(h)?;
        return Ok(h.iis(model.rows.len()).unwrap_or_default());
    }
    let mut relaxed = model.clone();
    for c in &mut relaxed.columns {
        c.integer = false;
    }
    let mut lp = Highs::new();
    configure(&mut lp, options)?;
    lp.pass_model(&relaxed)?;
    time_left(&mut lp)?;
    lp.run()?;
    if lp.model_status() != ffi::MODEL_INFEASIBLE {
        // An integer whose bounds hold no whole number cannot be satisfied
        // whatever the rules say: its two bounds are a conflict on their
        // own, and an irreducible one. The first such variable is named.
        let empty = model.columns.iter().position(|c| c.integer && (c.lower - INTEGRALITY).ceil() > (c.upper + INTEGRALITY).floor());
        return Ok(match empty {
            Some(j) => Conflict { rows: Vec::new(), columns: vec![(j, cadpole_core::opt::Side::Both)] },
            None => Conflict::default(),
        });
    }
    lp.set_int("iis_strategy", strategy)?;
    Ok(lp.iis(model.rows.len()).unwrap_or_default())
}

#[cfg(test)]
mod tests;
