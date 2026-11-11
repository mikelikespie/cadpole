//! A linear or mixed-integer linear program, and the solver a native host
//! installs to answer it.
//!
//! The model is plain data: columns (variables) with bounds, an objective
//! coefficient and an integrality flag; rows (constraints) with bounds; and
//! the constraint matrix, row by row. `cadpole-script`'s `opt` namespace builds
//! one from a script and asks the installed [`Solver`] to answer it.
//!
//! **The solver is not in this crate and cannot be.** It is HiGHS, which is
//! C++ and builds for the native targets only, while this crate also builds
//! for `wasm32-unknown-unknown` and into the C FFI library the Apple apps
//! link. So a host installs one at start-up with [`install`]: the CLI does,
//! for every command it runs (`cadpole eval`, `cadpole test`, `cadpole serve`, `cadpole
//! mcp`), through `cadpole_opt::install()`. The wasm engine and the FFI never
//! do, and a script that asks them to solve gets [`NOT_INSTALLED`] as an error
//! rather than a different answer: published sites are baked, so no browser
//! ever needs one.
//!
//! **What an answer depends on** is everything here plus the solver's
//! [`Solver::identity`], which names its version and its fixed options, so a
//! memo entry made by one solver is never served for another. Ties are the
//! one thing the model does not decide: among several optimal corners the
//! solver returns one, the same one on every run on one machine, but not
//! necessarily the one another platform's build picks.

use std::sync::OnceLock;

use crate::memo::{KeyBuilder, sha256};

/// Bumped when the meaning of a [`Model`] or [`Options`] changes -- a new
/// field, a different reading of an old one -- so a memo key made under the
/// old meaning is never hit.
pub const MODEL_FORMAT: u32 = 1;

/// What a script that asks for a solve is told when the host has no solver.
pub const NOT_INSTALLED: &str = "solving needs native Cadpole: the CLI (`cadpole eval`, `cadpole test`), `cadpole serve` \
     or `cadpole mcp`. This engine (the browser's wasm engine, or the one inside the Apple apps) builds the \
     problem but has no solver; published sites are baked, so they never need one";

/// Minimise or maximise the objective.
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub enum Sense {
    Minimize,
    Maximize,
}

/// One variable: its bounds (either may be infinite), whether it must be a
/// whole number, and its coefficient in the objective.
#[derive(Clone, Debug, PartialEq)]
pub struct Column {
    pub name: String,
    pub lower: f64,
    pub upper: f64,
    pub integer: bool,
    pub cost: f64,
}

impl Column {
    /// An integer column whose bounds lie inside `[0, 1]`: the only kind a
    /// no-good cut can tell apart.
    pub fn is_binary(&self) -> bool {
        self.integer && self.lower >= 0.0 && self.upper <= 1.0
    }
}

/// One constraint: `lower <= sum(a_j * x_j) <= upper`, either bound possibly
/// infinite. Its coefficients are in [`Model`]'s matrix.
#[derive(Clone, Debug, PartialEq)]
pub struct Row {
    pub name: String,
    pub lower: f64,
    pub upper: f64,
}

/// A linear program, or a mixed-integer one when any column is an integer.
///
/// The matrix is stored row by row: row `r`'s coefficients are
/// `value[start[r]..start[r + 1]]` on the columns `index[start[r]..start[r +
/// 1]]`. Within a row the columns are distinct and in increasing order, and no
/// coefficient is zero ([`Model::validate`] checks it), so one problem has one
/// encoding and one memo key.
#[derive(Clone, Debug, PartialEq)]
pub struct Model {
    pub sense: Sense,
    /// A constant added to the objective.
    pub offset: f64,
    pub columns: Vec<Column>,
    pub rows: Vec<Row>,
    pub start: Vec<usize>,
    pub index: Vec<u32>,
    pub value: Vec<f64>,
}

impl Model {
    /// An empty model with no columns and no rows.
    pub fn new(sense: Sense) -> Model {
        Model { sense, offset: 0.0, columns: Vec::new(), rows: Vec::new(), start: vec![0], index: Vec::new(), value: Vec::new() }
    }

    /// Whether any column is an integer.
    pub fn is_mip(&self) -> bool {
        self.columns.iter().any(|c| c.integer)
    }

    /// Append a row; `terms` must already be canonical (distinct columns in
    /// increasing order, no zeros), which [`Model::validate`] checks.
    pub fn push_row(&mut self, row: Row, terms: &[(u32, f64)]) {
        self.rows.push(row);
        for &(j, a) in terms {
            self.index.push(j);
            self.value.push(a);
        }
        self.start.push(self.index.len());
    }

    /// The terms of row `r`.
    pub fn row_terms(&self, r: usize) -> impl Iterator<Item = (usize, f64)> + '_ {
        let (a, b) = (self.start[r], self.start[r + 1]);
        self.index[a..b].iter().zip(&self.value[a..b]).map(|(&j, &v)| (j as usize, v))
    }

    /// Row `r`'s activity at `x`: its terms summed in column order, so the
    /// same point gives the same bits wherever it is computed.
    pub fn activity(&self, r: usize, x: &[f64]) -> f64 {
        let mut s = 0.0;
        for (j, a) in self.row_terms(r) {
            s += a * x[j];
        }
        s
    }

    /// The objective at `x`, summed in column order after the offset.
    pub fn objective_at(&self, x: &[f64]) -> f64 {
        let mut s = self.offset;
        for (c, &v) in self.columns.iter().zip(x) {
            if c.cost != 0.0 {
                s += c.cost * v;
            }
        }
        s
    }

    /// The structural checks a solver relies on. A bound pair with `lower >
    /// upper` is *not* an error: it is an infeasible problem, and the solver
    /// says so and names it.
    pub fn validate(&self) -> Result<(), String> {
        if self.start.len() != self.rows.len() + 1 || self.start[0] != 0 {
            return Err(format!("the matrix has {} row starts for {} rows", self.start.len(), self.rows.len()));
        }
        if self.index.len() != self.value.len() || *self.start.last().unwrap_or(&0) != self.index.len() {
            return Err("the matrix's index and value arrays disagree with its row starts".into());
        }
        if !self.offset.is_finite() {
            return Err(format!("the objective's constant is {}", self.offset));
        }
        for c in &self.columns {
            if c.lower.is_nan() || c.upper.is_nan() || !c.cost.is_finite() {
                return Err(format!("variable {:?} has a bound or objective coefficient that is not a number", c.name));
            }
            if c.lower == f64::INFINITY || c.upper == f64::NEG_INFINITY {
                return Err(format!("variable {:?} has a bound of {} on the wrong side", c.name, if c.lower == f64::INFINITY { "+inf" } else { "-inf" }));
            }
        }
        for (r, row) in self.rows.iter().enumerate() {
            if row.lower.is_nan() || row.upper.is_nan() {
                return Err(format!("constraint {:?} has a bound that is not a number", row.name));
            }
            if row.lower == f64::INFINITY || row.upper == f64::NEG_INFINITY {
                return Err(format!("constraint {:?} has a bound on the wrong side of infinity", row.name));
            }
            if self.start[r + 1] < self.start[r] {
                return Err(format!("constraint {:?}: row starts go backwards", row.name));
            }
            let mut last: Option<u32> = None;
            for k in self.start[r]..self.start[r + 1] {
                let j = self.index[k];
                if j as usize >= self.columns.len() {
                    return Err(format!("constraint {:?} refers to variable {j}, of {}", row.name, self.columns.len()));
                }
                if last.is_some_and(|l| j <= l) {
                    return Err(format!("constraint {:?}: columns not distinct and increasing", row.name));
                }
                if !self.value[k].is_finite() || self.value[k] == 0.0 {
                    return Err(format!("constraint {:?} has a coefficient of {}", row.name, self.value[k]));
                }
                last = Some(j);
            }
        }
        Ok(())
    }

    /// Write everything that decides the answer into `k`, names included, in
    /// declaration order: the order is part of the problem, because it is
    /// what decides which of several tied optima comes back.
    pub fn canon(&self, k: &mut KeyBuilder) {
        k.tag("model").u64(MODEL_FORMAT as u64);
        k.bool(self.sense == Sense::Maximize).f64(self.offset);
        k.u64(self.columns.len() as u64);
        for c in &self.columns {
            k.str(&c.name).f64(c.lower).f64(c.upper).bool(c.integer).f64(c.cost);
        }
        k.u64(self.rows.len() as u64);
        for (r, row) in self.rows.iter().enumerate() {
            k.str(&row.name).f64(row.lower).f64(row.upper).u64((self.start[r + 1] - self.start[r]) as u64);
            for (j, a) in self.row_terms(r) {
                k.u64(j as u64).f64(a);
            }
        }
    }
}

/// How to solve. Cadpole's fixed options (one thread, no output, a fixed random
/// seed) are the solver's own and part of its [`Solver::identity`].
#[derive(Clone, Debug, PartialEq)]
pub struct Options {
    /// Seconds for the whole call, every solution together, as the script
    /// asked; `None` is no limit. A solve the limit stops is reported as
    /// [`Status::TimeLimit`], with the best point found if there is one.
    pub time_limit: Option<f64>,
    /// What is left of the evaluation's own budget, in seconds: the solver
    /// stops there too, since the deadline cannot interrupt a native call.
    /// It is the caller's circumstance, not part of the question, so it is
    /// not in [`Options::canon`] or [`digest`]: a memo or a cache keyed on
    /// the question serves evaluations with different budgets alike.
    pub deadline: Option<f64>,
    /// The relative gap between the best point and the best bound at which a
    /// MILP counts as solved (HiGHS's `mip_rel_gap`).
    pub mip_rel_gap: f64,
    /// How many solutions: the best, then the best that differs from every
    /// earlier one in [`Options::distinct`], until there are this many or no
    /// other exists.
    pub count: usize,
    /// The binary columns the solutions must differ in. Empty means every
    /// binary column.
    pub distinct: Vec<usize>,
    /// Branch-and-bound nodes a MILP search may visit before it stops with
    /// [`Status::NodeLimit`] and its best point; `None` is no limit. Unlike
    /// the time limit this is deterministic, so an answer it stopped is the
    /// same on every run and may be memoised.
    pub max_nodes: Option<u64>,
    /// A starting point for a MILP search -- some or all columns' values,
    /// by column, in increasing order -- which the solver completes and
    /// starts from. It can decide which of several tied optima comes back, and
    /// what a limit stops at, so it is part of the question.
    pub start: Vec<(usize, f64)>,
    /// The script asked for this solve to run as a job (`solve(job=True)`)
    /// whatever the budget. A circumstance like [`Options::deadline`],
    /// not in the key: a job's answer and an inline one are the same answer.
    pub job: bool,
    /// Whether the host may start a job for this solve: true for an
    /// evaluation its owner asked for (`cadpole serve`'s, `cadpole solve`'s), false
    /// in a drag preview, for the limited listener and in a plain `cadpole
    /// eval`, which read a job's answer and see a running one's state but
    /// never start one. Not in the key.
    pub may_start_job: bool,
    /// Who asked, for a host's list of jobs: the problem's feature id and the
    /// line of the `solve()` call. Not in the key.
    pub origin: String,
    /// Run here, now, under `deadline` as given: never as a job, never
    /// pending, and never cut to a host's inline window. A conflict probe
    /// asks this way, since a probe that came back pending would tell its
    /// filter nothing. Not in the key.
    pub inline_only: bool,
}

impl Default for Options {
    fn default() -> Options {
        Options {
            time_limit: None,
            deadline: None,
            mip_rel_gap: DEFAULT_MIP_REL_GAP,
            count: 1,
            distinct: Vec::new(),
            max_nodes: None,
            start: Vec::new(),
            job: false,
            may_start_job: false,
            origin: String::new(),
            inline_only: false,
        }
    }
}

/// HiGHS's own default, 0.01 %.
pub const DEFAULT_MIP_REL_GAP: f64 = 1e-4;

impl Options {
    /// The limit a solve runs under: the script's, or the deadline's if that
    /// comes first.
    pub fn effective_time_limit(&self) -> Option<f64> {
        match (self.time_limit, self.deadline) {
            (Some(a), Some(b)) => Some(a.min(b)),
            (a, b) => a.or(b),
        }
    }

    /// Everything that decides the answer, for the memo key. The time limit
    /// is in the key although an answer it stopped is never stored, because
    /// it can decide which of the two a solve becomes. The deadline is not,
    /// and neither is anything about where the solve runs (`job`,
    /// `may_start_job`, `origin`).
    pub fn canon(&self, k: &mut KeyBuilder) {
        k.tag("options").put(&self.time_limit).f64(self.mip_rel_gap).u64(self.count as u64);
        k.u64(self.distinct.len() as u64);
        for &j in &self.distinct {
            k.u64(j as u64);
        }
        match self.max_nodes {
            Some(n) => k.bool(true).u64(n),
            None => k.bool(false),
        };
        k.u64(self.start.len() as u64);
        for &(j, v) in &self.start {
            k.u64(j as u64).f64(v);
        }
    }
}

/// How a solve ended.
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub enum Status {
    Optimal,
    Infeasible,
    Unbounded,
    /// One of the two; the solver could not tell which.
    InfeasibleOrUnbounded,
    TimeLimit,
    /// The MILP search visited `max_nodes` nodes; the point, if any, is the
    /// best it found and [`Solution::bound`] says how far from optimal it
    /// can be.
    NodeLimit,
    IterationLimit,
    Interrupted,
    /// The solve is running somewhere else -- a server-side job a host's
    /// solver handed it to -- and has no answer yet. Never kept: the next
    /// evaluation asks again, and gets the answer once there is one.
    Pending,
    /// A numerical failure or a solver error; [`Solution::message`] says what.
    Error,
}

impl Status {
    /// The spelling a script sees as `sol.status`.
    pub fn as_str(self) -> &'static str {
        match self {
            Status::Optimal => "optimal",
            Status::Infeasible => "infeasible",
            Status::Unbounded => "unbounded",
            Status::InfeasibleOrUnbounded => "infeasible_or_unbounded",
            Status::TimeLimit => "time_limit",
            Status::NodeLimit => "node_limit",
            Status::IterationLimit => "iteration_limit",
            Status::Interrupted => "interrupted",
            Status::Pending => "pending",
            Status::Error => "error",
        }
    }

    /// Whether this answer is a function of the problem and its options, so
    /// the same call always gets it and a memo may keep it: optimal,
    /// infeasible and unbounded are, and so is a node or iteration limit,
    /// which counts work rather than time. A time limit, an interrupt or an
    /// error depends on the run (deadline stops and numerical
    /// failures are not cached).
    pub fn is_repeatable(self) -> bool {
        matches!(
            self,
            Status::Optimal
                | Status::Infeasible
                | Status::Unbounded
                | Status::InfeasibleOrUnbounded
                | Status::NodeLimit
                | Status::IterationLimit
        )
    }

    /// Whether this is a property of the problem itself, whatever the
    /// limits: optimal, infeasible or unbounded.
    pub fn is_final(self) -> bool {
        matches!(self, Status::Optimal | Status::Infeasible | Status::Unbounded | Status::InfeasibleOrUnbounded)
    }
}

/// Which side of a bound takes part in a conflict.
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub enum Side {
    Lower,
    Upper,
    /// Both bounds, or an equality.
    Both,
}

/// An irreducible infeasible subsystem: constraints and variable bounds that
/// cannot all hold, any one of which can be dropped to make the rest
/// feasible. In declaration order.
#[derive(Clone, Debug, Default, PartialEq)]
pub struct Conflict {
    pub rows: Vec<(usize, Side)>,
    pub columns: Vec<(usize, Side)>,
}

/// One answer.
#[derive(Clone, Debug, PartialEq)]
pub struct Solution {
    pub status: Status,
    /// The point, one value per column, when the solver has a feasible one:
    /// always when optimal, sometimes when a limit stopped it. Integer
    /// columns are rounded to whole numbers and no value is `-0.0`.
    pub values: Option<Vec<f64>>,
    /// Each row's activity at [`Solution::values`] ([`Model::activity`]), so
    /// slacks agree with the point a script reads.
    pub activities: Option<Vec<f64>>,
    /// The objective at [`Solution::values`], recomputed from them in column
    /// order, so an integer program with whole coefficients gets a whole
    /// number back.
    pub objective: Option<f64>,
    /// For an LP solved to optimality: per row, the rate at which the
    /// objective changes as the row's binding bound moves up (0 for a row
    /// that does not bind). `None` for a MILP, whose duals mean nothing.
    pub row_duals: Option<Vec<f64>>,
    /// The same for each column's bounds (reduced costs).
    pub reduced_costs: Option<Vec<f64>>,
    /// For a MILP: the relative gap between the point and the best bound.
    pub mip_gap: Option<f64>,
    /// For a MILP: the best bound on the objective the search proved -- no
    /// point is better than this -- which is what a limit's answer is
    /// measured against.
    pub bound: Option<f64>,
    /// Branch-and-bound nodes; 0 for an LP.
    pub nodes: u64,
    /// Simplex iterations, every node included.
    pub iterations: u64,
    /// For an infeasible problem: what cannot all hold. For a MILP it is the
    /// conflict of the LP relaxation, empty when only integrality is to blame.
    pub conflict: Option<Conflict>,
    /// What went wrong, for [`Status::Error`]; otherwise empty.
    pub message: String,
}

impl Solution {
    /// An answer with no point, for a status that has none.
    pub fn without_point(status: Status) -> Solution {
        Solution {
            status,
            values: None,
            activities: None,
            objective: None,
            row_duals: None,
            reduced_costs: None,
            mip_gap: None,
            bound: None,
            nodes: 0,
            iterations: 0,
            conflict: None,
            message: String::new(),
        }
    }

    /// The bytes this answer holds, for the memo's bound.
    pub fn bytes(&self) -> usize {
        let vec = |v: &Option<Vec<f64>>| v.as_ref().map_or(0, |v| v.len() * 8);
        let conflict = self.conflict.as_ref().map_or(0, |c| (c.rows.len() + c.columns.len()) * 16);
        128 + vec(&self.values) + vec(&self.activities) + vec(&self.row_duals) + vec(&self.reduced_costs) + conflict + self.message.len()
    }
}

/// A solver for [`Model`]s.
pub trait Solver: Send + Sync {
    /// Which solver, at which version, with which fixed options: part of
    /// every memo key, and what the `optimize` feature records.
    fn identity(&self) -> &str;

    /// Solve `model`. The first solution is the solve itself, whatever its
    /// status; when `options.count > 1`, each later one is the best that
    /// differs from all before it, and the list stops early when no other
    /// exists or a limit stops a solve. `Err` is for a model the solver
    /// refused outright.
    fn solve(&self, model: &Model, options: &Options) -> Result<Vec<Solution>, String>;
}

/// The identity of one question: the SHA-256 of `model` and `options` in
/// their canonical form (declaration order kept, the deadline left out) and
/// of the solver that answers it. `cadpole-script` keys its memo with this, and
/// a host that keeps answers longer -- a job runner's persistent cache --
/// should key them the same way, so the two can never disagree about what
/// counts as the same problem.
pub fn digest(model: &Model, options: &Options, solver_identity: &str) -> [u8; 32] {
    let mut k = KeyBuilder::new("opt-problem");
    model.canon(&mut k);
    options.canon(&mut k);
    k.str(solver_identity);
    sha256(k.finish().bytes())
}

/// What a deletion filter found: rows that cannot all hold, and how sure
/// that is.
#[derive(Clone, Debug, Default, PartialEq)]
pub struct Deleted {
    /// The rows still in when the filter stopped, in declaration order:
    /// infeasible together, as the last probe that dropped any proved (or the
    /// model itself, when none did).
    pub rows: Vec<usize>,
    /// Whether dropping any one of `rows` makes the rest feasible: the filter
    /// ran to its end and every probe answered. With
    /// [`DeletionLimits::families_only`], the same of whole families.
    pub irreducible: bool,
    pub probes: usize,
    /// Probes that came back without an answer -- a limit stopped them, or
    /// the solver failed -- so the rows they tested stayed in.
    pub unanswered: usize,
    /// Why the filter stopped before its end, if it did.
    pub stop: Option<Stop>,
    /// Whether the clock decided any of it: a time limit or the deadline
    /// stopped the filter or one of its probes. Such an answer may not come
    /// back the same, and a memo must not keep it. A stop at `max_probes`
    /// is a count, and repeats.
    pub clock: bool,
}

/// Why a deletion filter stopped before its end.
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub enum Stop {
    /// It had run `max_probes` probes.
    Probes,
    /// Its time ran out: its own limit, or the evaluation's deadline.
    Time,
}

/// What bounds a deletion filter, beyond each probe's own limits in
/// [`Options`].
#[derive(Clone, Copy, Debug, Default, PartialEq)]
pub struct DeletionLimits {
    /// Probes to stop after; each is one solve.
    pub max_probes: Option<usize>,
    /// Seconds for the whole filter. `Options::deadline` (the evaluation's)
    /// bounds it as well. Each probe keeps `Options::time_limit` and gets
    /// what is left as its `Options::deadline`, which stops it no later
    /// and is not part of its memo key.
    pub time_limit: Option<f64>,
    /// Stop after the pass over families: whole families that cannot all
    /// hold, without the search for the rules within them.
    pub families_only: bool,
}

/// A conflict found by deletion, for a MILP whose LP relaxation is feasible
/// -- where HiGHS's IIS, an LP's, has nothing to say.
///
/// `model` must be infeasible. Each probe asks `solver` for any feasible
/// point (the objective left out) with some rows' bounds removed, and rows a
/// probe shows are not needed stay out. First the `groups` (families of
/// rules) go, one at a time; every row in no group goes as one group more, so
/// thousands of untagged rows cost one probe, not thousands. Then, within
/// each group that had to stay, its halves, their halves and so on down to
/// single rows, which finds k needed rules among m in about 2k log2(m/k)
/// probes rather than m. What is left cannot all hold, and dropping any one
/// of it makes the rest feasible. Groups go in the order given (the untagged
/// last) and rows in declaration order, so where there is a choice the rules
/// written last are the ones named. Variable bounds are never dropped.
///
/// **Bounded.** It stops at `limits.max_probes` and when its time runs out
/// (`limits.time_limit`, `options.deadline`), checked before every probe;
/// each probe gets what is left as its deadline, so none outlives the
/// budget, and keeps the solve's own time limit, so its question -- its
/// memo and cache key -- is the same on every run.
/// Stopped early, it returns the rows still in: proven infeasible together,
/// but perhaps not irreducible.
pub fn conflict_by_deletion(
    solver: &dyn Solver,
    model: &Model,
    options: &Options,
    groups: &[Vec<usize>],
    limits: DeletionLimits,
) -> Deleted {
    let n = model.rows.len();
    let mut base = model.clone();
    for c in &mut base.columns {
        c.cost = 0.0;
    }
    base.offset = 0.0;
    // The families, each row in the first that names it, then every other
    // row as one group more.
    let mut grouped = vec![false; n];
    let mut all: Vec<Vec<usize>> = Vec::with_capacity(groups.len() + 1);
    for g in groups {
        all.push(g.iter().copied().filter(|&r| r < n && !std::mem::replace(&mut grouped[r], true)).collect());
    }
    all.push((0..n).filter(|&r| !grouped[r]).collect());
    let mut f = Filter {
        solver,
        base,
        options,
        limits,
        clock: crate::time::Stopwatch::start(),
        out: vec![false; n],
        found: Deleted::default(),
    };
    let mut stopped = false;
    let mut kept: Vec<&[usize]> = Vec::new();
    for g in all.iter().filter(|g| !g.is_empty()) {
        match f.drop_if_still_infeasible(g) {
            Some(true) => {}
            Some(false) => kept.push(g),
            None => {
                stopped = true;
                break;
            }
        }
    }
    if !stopped && !limits.families_only {
        // Dropping the whole of a kept group is known to leave the rest
        // feasible (it did with fewer rows out), so each starts at its
        // halves; the first half goes first.
        fn halves<'r>(todo: &mut Vec<&'r [usize]>, rows: &'r [usize]) {
            if rows.len() > 1 {
                let (a, b) = rows.split_at(rows.len() / 2);
                todo.push(b);
                todo.push(a);
            }
        }
        let mut todo: Vec<&[usize]> = Vec::new();
        'groups: for g in kept {
            halves(&mut todo, g);
            while let Some(rows) = todo.pop() {
                match f.drop_if_still_infeasible(rows) {
                    Some(true) => {}
                    Some(false) => halves(&mut todo, rows),
                    None => {
                        stopped = true;
                        break 'groups;
                    }
                }
            }
        }
    }
    let mut found = f.found;
    found.irreducible = !stopped && found.unanswered == 0;
    found.rows = (0..n).filter(|&r| !f.out[r]).collect();
    found
}

/// One deletion filter's state: the rows it has dropped, and its probes.
struct Filter<'a> {
    solver: &'a dyn Solver,
    base: Model,
    options: &'a Options,
    limits: DeletionLimits,
    clock: crate::time::Stopwatch,
    out: Vec<bool>,
    found: Deleted,
}

impl Filter<'_> {
    /// Seconds left for the whole filter, if anything bounds it.
    fn left(&self) -> Option<f64> {
        let spent = self.clock.elapsed_ms() / 1000.0;
        [self.limits.time_limit, self.options.deadline].into_iter().flatten().map(|t| t - spent).reduce(f64::min)
    }

    /// Drop `rows` if the model stays infeasible without them (and the rows
    /// already out): `Some(true)` if they went, `Some(false)` if they stay --
    /// needed, or the probe could not tell -- and `None`, nothing probed, if
    /// the filter must stop first.
    fn drop_if_still_infeasible(&mut self, rows: &[usize]) -> Option<bool> {
        if self.limits.max_probes.is_some_and(|m| self.found.probes >= m) {
            self.found.stop = Some(Stop::Probes);
            return None;
        }
        let left = self.left();
        if left.is_some_and(|t| t <= 0.0) {
            self.found.stop = Some(Stop::Time);
            self.found.clock = true;
            return None;
        }
        for &r in rows {
            self.out[r] = true;
        }
        let mut m = self.base.clone();
        for (row, &o) in m.rows.iter_mut().zip(&self.out) {
            if o {
                row.lower = f64::NEG_INFINITY;
                row.upper = f64::INFINITY;
            }
        }
        // A probe runs where it is asked, never as a job: a filter is many
        // solves, and one that came back pending would tell it nothing. Its
        // time limit is the solve's own, part of the question; what is left
        // of the filter's time is its deadline, a circumstance that is not
        // in the key, so the same probe is the same question on every run
        // and a cache can answer it.
        let opts = Options {
            time_limit: self.options.time_limit,
            deadline: left.map(|t| t.max(1e-3)),
            mip_rel_gap: self.options.mip_rel_gap,
            count: 1,
            distinct: Vec::new(),
            max_nodes: self.options.max_nodes,
            start: Vec::new(),
            job: false,
            may_start_job: false,
            origin: format!("{} (a conflict probe)", self.options.origin),
            inline_only: true,
        };
        self.found.probes += 1;
        let gone = match self.solver.solve(&m, &opts).map(|s| s.into_iter().next()) {
            Ok(Some(s)) if s.status == Status::Infeasible => true,
            Ok(Some(s)) if s.values.is_some() => false,
            other => {
                // No answer: the rows stay, and the conflict may be loose.
                self.found.unanswered += 1;
                if matches!(&other, Ok(Some(s)) if !s.status.is_repeatable()) {
                    self.found.clock = true;
                }
                false
            }
        };
        if !gone {
            for &r in rows {
                self.out[r] = false;
            }
        }
        Some(gone)
    }
}

static SOLVER: OnceLock<&'static dyn Solver> = OnceLock::new();

/// Install the process's solver. The first call wins; it returns whether
/// this one did. A host calls it once, before it evaluates anything.
pub fn install(solver: &'static dyn Solver) -> bool {
    SOLVER.set(solver).is_ok()
}

/// The installed solver, if this process has one.
pub fn installed() -> Option<&'static dyn Solver> {
    SOLVER.get().copied()
}

#[cfg(test)]
mod tests {
    use super::*;

    fn two_rows() -> Model {
        let mut m = Model::new(Sense::Maximize);
        for name in ["x", "y"] {
            m.columns.push(Column { name: name.into(), lower: 0.0, upper: f64::INFINITY, integer: false, cost: 1.0 });
        }
        m.push_row(Row { name: "a".into(), lower: f64::NEG_INFINITY, upper: 4.0 }, &[(0, 1.0), (1, 2.0)]);
        m.push_row(Row { name: "b".into(), lower: f64::NEG_INFINITY, upper: 6.0 }, &[(0, 3.0), (1, 1.0)]);
        m
    }

    #[test]
    fn a_canonical_model_validates_and_a_broken_one_says_why() {
        let m = two_rows();
        m.validate().unwrap();
        assert_eq!(m.activity(0, &[2.0, 1.0]), 4.0);
        assert_eq!(m.objective_at(&[2.0, 1.0]), 3.0);
        let mut bad = m.clone();
        bad.index[1] = 0;
        assert!(bad.validate().unwrap_err().contains("distinct and increasing"));
        let mut bad = m.clone();
        bad.value[0] = 0.0;
        assert!(bad.validate().unwrap_err().contains("coefficient of 0"));
        let mut bad = m.clone();
        bad.columns[0].upper = f64::NEG_INFINITY;
        assert!(bad.validate().unwrap_err().contains("wrong side"));
        // Crossed bounds are a problem to answer, not a model to refuse.
        let mut crossed = m;
        crossed.rows[0].lower = 9.0;
        crossed.validate().unwrap();
    }

    #[test]
    fn the_key_sees_order_names_and_bits() {
        let key = |m: &Model| {
            let mut k = KeyBuilder::new("t");
            m.canon(&mut k);
            k.finish().bytes().to_vec()
        };
        let m = two_rows();
        let mut renamed = m.clone();
        renamed.rows[1].name = "c".into();
        let mut nudged = m.clone();
        nudged.value[3] = 1.0 + f64::EPSILON;
        let mut swapped = m.clone();
        swapped.rows.swap(0, 1);
        assert_eq!(key(&m), key(&m.clone()));
        for other in [renamed, nudged, swapped] {
            assert_ne!(key(&m), key(&other));
        }
    }

    /// A solver that calls a model infeasible while rows 1 and 4 both keep a
    /// bound: the conflict the filter must find, whatever the groups.
    struct OnlyOneAndFour;
    impl Solver for OnlyOneAndFour {
        fn identity(&self) -> &str {
            "mock"
        }
        fn solve(&self, m: &Model, o: &Options) -> Result<Vec<Solution>, String> {
            assert!(m.columns.iter().all(|c| c.cost == 0.0) && o.count == 1, "a probe asks for any point");
            let bound = |r: usize| m.rows[r].lower.is_finite() || m.rows[r].upper.is_finite();
            Ok(vec![if bound(1) && bound(4) {
                Solution::without_point(Status::Infeasible)
            } else {
                Solution { values: Some(vec![0.0; m.columns.len()]), ..Solution::without_point(Status::Optimal) }
            }])
        }
    }

    fn rows(n: usize) -> Model {
        let mut m = Model::new(Sense::Maximize);
        m.columns.push(Column { name: "x".into(), lower: 0.0, upper: 1.0, integer: true, cost: 1.0 });
        for r in 0..n {
            m.push_row(Row { name: format!("r{r}"), lower: f64::NEG_INFINITY, upper: 1.0 }, &[(0, 1.0)]);
        }
        m
    }

    #[test]
    fn deletion_drops_whole_families_then_halves_of_the_rest() {
        let m = rows(6);
        let all = DeletionLimits::default();
        let groups = vec![vec![0, 1, 2], vec![3], vec![4, 5]];
        let d = conflict_by_deletion(&OnlyOneAndFour, &m, &Options::default(), &groups, all);
        assert_eq!(d.rows, vec![1, 4]);
        assert!(d.irreducible && d.stop.is_none() && !d.clock);
        // Three family probes; then [0] and [1, 2] and its halves in the
        // first family kept, and [4] and [5] in the second.
        assert_eq!(d.probes, 3 + 4 + 2);
        // With no families, the untagged rows are one group: one probe for
        // all of them, then halves.
        let d = conflict_by_deletion(&OnlyOneAndFour, &m, &Options::default(), &[], all);
        assert_eq!((d.rows, d.irreducible), (vec![1, 4], true));
        assert_eq!(d.probes, 1 + 5 + 5);
        // Families only: the two families that cannot all go, whole.
        let fam = DeletionLimits { families_only: true, ..all };
        let d = conflict_by_deletion(&OnlyOneAndFour, &m, &Options::default(), &groups, fam);
        assert_eq!((d.rows, d.probes, d.irreducible), (vec![0, 1, 2, 4, 5], 3, true));
    }

    /// The trap a model of thousands of untagged rules sets: a probe
    /// for each. One probe drops all the rows that do not matter, and
    /// halving finds the two that do in a few dozen.
    #[test]
    fn thousands_of_untagged_rules_cost_a_few_dozen_probes() {
        let m = rows(5000);
        let d = conflict_by_deletion(&OnlyOneAndFour, &m, &Options::default(), &[], DeletionLimits::default());
        assert_eq!((d.rows, d.irreducible), (vec![1, 4], true));
        // One for all 5,000, then two at each of the ten levels the two rows
        // share and four below where they part: 29, where one at a time took
        // 5,000.
        assert_eq!(d.probes, 1 + 2 * 10 + 4 + 4);
    }

    #[test]
    fn a_bounded_search_returns_what_it_has_proven() {
        let m = rows(6);
        let groups = vec![vec![0, 1, 2], vec![3], vec![4, 5]];
        let two = DeletionLimits { max_probes: Some(2), ..DeletionLimits::default() };
        let d = conflict_by_deletion(&OnlyOneAndFour, &m, &Options::default(), &groups, two);
        // The first family had to stay, the second could go; the third was
        // never asked about. Still infeasible together, but not irreducible.
        assert_eq!((d.rows, d.probes), (vec![0, 1, 2, 4, 5], 2));
        assert_eq!((d.stop, d.irreducible, d.clock), (Some(Stop::Probes), false, false));
        // Out of time before the first probe: the whole model, nothing proven
        // beyond it, and an answer the clock decided.
        let spent = Options { deadline: Some(0.0), ..Options::default() };
        let d = conflict_by_deletion(&OnlyOneAndFour, &m, &spent, &groups, DeletionLimits::default());
        assert_eq!((d.rows.len(), d.probes, d.stop, d.clock), (6, 0, Some(Stop::Time), true));
        let own = DeletionLimits { time_limit: Some(0.0), ..DeletionLimits::default() };
        let d = conflict_by_deletion(&OnlyOneAndFour, &m, &Options::default(), &groups, own);
        assert_eq!((d.probes, d.stop), (0, Some(Stop::Time)));
    }

    /// A solver whose every probe stops at its time limit: nothing is
    /// learned, every row stays, and the answer is the clock's.
    struct AlwaysOutOfTime;
    impl Solver for AlwaysOutOfTime {
        fn identity(&self) -> &str {
            "mock"
        }
        fn solve(&self, _: &Model, o: &Options) -> Result<Vec<Solution>, String> {
            assert_eq!(o.time_limit, Some(5.0), "a probe keeps the solve's own limit");
            assert!(o.deadline.is_some_and(|t| t <= 60.0), "and gets what is left as its deadline: {:?}", o.deadline);
            Ok(vec![Solution::without_point(Status::TimeLimit)])
        }
    }

    #[test]
    fn a_probe_that_runs_out_of_time_keeps_its_rows() {
        let m = rows(4);
        let o = Options { time_limit: Some(5.0), ..Options::default() };
        let fam = DeletionLimits { families_only: true, time_limit: Some(60.0), ..DeletionLimits::default() };
        let d = conflict_by_deletion(&AlwaysOutOfTime, &m, &o, &[vec![0, 1], vec![2, 3]], fam);
        assert_eq!((d.rows, d.probes, d.unanswered), (vec![0, 1, 2, 3], 2, 2));
        assert!(!d.irreducible && d.clock && d.stop.is_none());
    }

    /// A solver that answers like [`OnlyOneAndFour`] and writes down the
    /// options of every probe it is asked.
    struct Recording(std::sync::Mutex<Vec<Options>>);
    impl Solver for Recording {
        fn identity(&self) -> &str {
            "mock"
        }
        fn solve(&self, m: &Model, o: &Options) -> Result<Vec<Solution>, String> {
            self.0.lock().unwrap().push(o.clone());
            OnlyOneAndFour.solve(m, o)
        }
    }

    /// A probe is a question a cache can answer: the time the filter has
    /// left goes in its deadline, which is no part of the key, and its time
    /// limit is the solve's own. Folded into the time limit, it made every
    /// probe of every run a new question that no cache ever hit.
    #[test]
    fn a_probe_asks_the_same_question_whatever_time_is_left() {
        let m = rows(6);
        let groups = vec![vec![0, 1, 2], vec![3], vec![4, 5]];
        let probes = |o: &Options| {
            let r = Recording(std::sync::Mutex::new(Vec::new()));
            let d = conflict_by_deletion(&r, &m, o, &groups, DeletionLimits { time_limit: Some(50.0), ..Default::default() });
            assert_eq!(d.rows, vec![1, 4]);
            r.0.into_inner().unwrap()
        };
        for asked in [None, Some(7.0)] {
            let a = probes(&Options { time_limit: asked, deadline: Some(100.0), ..Options::default() });
            let b = probes(&Options { time_limit: asked, deadline: Some(30.0), ..Options::default() });
            assert_eq!(a.len(), b.len());
            for (pa, pb) in a.iter().zip(&b) {
                assert_eq!(pa.time_limit, asked, "the probe's limit is the solve's own");
                assert!(pa.deadline.is_some_and(|t| t > 0.0 && t <= 50.0), "what is left bounds it: {:?}", pa.deadline);
                assert!(pb.deadline.is_some_and(|t| t > 0.0 && t <= 30.0), "{:?}", pb.deadline);
            }
            let keys = |ps: &[Options]| ps.iter().map(|o| digest(&m, o, "mock")).collect::<Vec<_>>();
            assert_eq!(keys(&a), keys(&b), "the same probes are the same questions");
        }
    }

    #[test]
    fn the_digest_is_the_question_and_not_the_deadline() {
        let m = two_rows();
        let o = Options::default();
        let d = digest(&m, &o, "s");
        assert_eq!(d, digest(&m, &Options { deadline: Some(3.0), ..o.clone() }, "s"), "the budget left is not the question");
        let elsewhere = Options { job: true, may_start_job: true, origin: "p (main.star:3)".into(), ..o.clone() };
        assert_eq!(d, digest(&m, &elsewhere, "s"), "nor is where the solve runs");
        assert_ne!(d, digest(&m, &Options { time_limit: Some(3.0), ..o.clone() }, "s"));
        assert_ne!(d, digest(&m, &Options { start: vec![(0, 1.0)], ..o.clone() }, "s"), "a start can decide a tie");
        assert_ne!(d, digest(&m, &o, "another solver"));
        assert_eq!(Options { time_limit: Some(5.0), deadline: Some(2.0), ..o.clone() }.effective_time_limit(), Some(2.0));
        assert_eq!(Options { deadline: Some(2.0), ..o }.effective_time_limit(), Some(2.0));
        assert!(!Status::Pending.is_repeatable() && Status::Pending.as_str() == "pending");
    }

    #[test]
    fn only_answers_that_do_not_depend_on_the_clock_are_repeatable() {
        assert!(Status::Optimal.is_repeatable() && Status::Infeasible.is_repeatable());
        assert!(Status::NodeLimit.is_repeatable(), "a node limit counts work, not time");
        assert!(!Status::TimeLimit.is_repeatable() && !Status::Error.is_repeatable());
        assert!(Status::Optimal.is_final() && !Status::NodeLimit.is_final());
        assert_eq!(Status::InfeasibleOrUnbounded.as_str(), "infeasible_or_unbounded");
        assert_eq!(Status::NodeLimit.as_str(), "node_limit");
    }
}
