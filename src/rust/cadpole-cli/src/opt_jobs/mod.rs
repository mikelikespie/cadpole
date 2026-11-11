//! Long solves without blocking: the solution cache every native
//! evaluation reads before it solves, and -- where the host can run them --
//! solve jobs for what one evaluation cannot finish.
//!
//! **The hook is the solver itself.** `cadpole_core::opt` takes the first
//! [`Solver`] a process installs, and every `opt.problem(...).solve()` of
//! every evaluation in that process -- `cadpole eval`, `cadpole serve`'s, `cadpole
//! mcp`'s -- calls it on a memo miss. So `cadpole` installs [`JobSolver`] rather
//! than HiGHS. It answers from the cache when it can; from a job's record
//! when a job has the question; inline, with `cadpole_opt::solve`, when the
//! solve fits a window of the evaluation's budget; and otherwise with
//! `Status::Pending`, starting a job if this evaluation may. cadpole-script's
//! memo keeps what it returns like any other answer and never keeps a
//! pending one, so the evaluation after a job lands reads the answer here.
//!
//! **Same question, same answer.** The cache key is
//! `cadpole_core::opt::digest` (the model in declaration order, the options
//! that decide the answer, the solver's identity), and a hit returns the bits
//! the solver produced. A job runs the very call an inline solve makes; its
//! own time limit goes in `Options::deadline`, which is not part of the
//! question and only ever decides whether a solve stops.

pub mod cache;
pub mod child;
pub mod codec;
pub mod jobs;
pub mod ops;
pub mod sys;

use std::sync::atomic::{AtomicU64, Ordering};
use std::sync::{Arc, OnceLock};
use std::time::{Instant, SystemTime};

use cadpole_engine::cadpole_core::opt::{self, Model, Options, Solution, Solver, Status};

use cache::Cache;
use codec::{Answer, Problem, Source};
use jobs::{Found, Jobs, Record, Runner, Settings, State};

/// What the installed solver did in this process, for the line `cadpole eval`
/// prints and for tests.
#[derive(Debug, Default)]
pub struct Counts {
    /// Answers read from the cache.
    pub hits: AtomicU64,
    /// Solves that ran here.
    pub solved: AtomicU64,
    /// Answers filed in the cache.
    pub kept: AtomicU64,
    /// Solves answered pending: a job has them, or one would.
    pub pending: AtomicU64,
    /// Jobs this process started.
    pub started: AtomicU64,
}

/// A snapshot of [`Counts`].
#[derive(Debug, Default, Clone, Copy, PartialEq, Eq)]
pub struct Tally {
    pub hits: u64,
    pub solved: u64,
    pub kept: u64,
    pub pending: u64,
    pub started: u64,
}

/// HiGHS's identity, as `cadpole_opt` installs it.
fn highs_identity() -> String {
    cadpole_opt::solver().identity().to_owned()
}

/// The solver `cadpole` installs: HiGHS behind the solution cache and, once a
/// host gives it a runner, solve jobs.
pub struct JobSolver {
    identity: String,
    cache: Option<Arc<Cache>>,
    /// Where the cache lives, which a job's child is told.
    root: Option<std::path::PathBuf>,
    runner: OnceLock<Arc<Runner>>,
    counts: Counts,
}

static SOLVER: OnceLock<JobSolver> = OnceLock::new();

/// Make [`JobSolver`], with the cache the environment names, this process's
/// solver. `cadpole` calls it once, before it evaluates anything; it replaces
/// `cadpole_opt::install()`, which would install HiGHS bare.
pub fn install() -> &'static JobSolver {
    let s = SOLVER.get_or_init(|| {
        let root = cache::location_from(|n| std::env::var(n).ok());
        let cache = root.as_ref().and_then(|r| Cache::open(r, cache::cap_from(|n| std::env::var(n).ok())).ok());
        JobSolver::new(cache.map(Arc::new), root)
    });
    cadpole_engine::cadpole_core::opt::install(s);
    s
}

/// The installed solver, once [`install`] has run.
pub fn installed() -> Option<&'static JobSolver> {
    SOLVER.get()
}

fn unix_ms(t: SystemTime) -> u64 {
    t.duration_since(SystemTime::UNIX_EPOCH).map(|d| d.as_millis() as u64).unwrap_or(0)
}

/// Where a solve may go, worked out from its options and the host.
#[derive(Debug, Clone, Copy, PartialEq)]
pub struct Plan {
    /// The inline attempt's limit, in seconds; `None` for none.
    pub window: Option<f64>,
    /// Skip the inline attempt: the script asked for a job, or for more time
    /// than the window.
    pub straight_to_job: bool,
    /// A job may start for it here.
    pub may_start: bool,
}

/// The window an inline attempt gets: with jobs to hand it to, at most the
/// runner's cap and half of what the evaluation has, so a long solve becomes
/// a job early and the evaluation keeps time for the rest of the script;
/// without, all but a margin of it -- a tenth, at least a second -- so the
/// deadline never fires inside the solve and kills the evaluation.
pub fn plan(o: &Options, inline_cap: Option<f64>) -> Plan {
    let may_start = inline_cap.is_some() && o.may_start_job;
    let window = match (may_start, o.deadline) {
        (true, Some(b)) => Some((b / 2.0).min(inline_cap.unwrap_or(f64::INFINITY))),
        (true, None) => inline_cap,
        (false, Some(b)) => Some((b - (b / 10.0).max(1.0)).max(b / 2.0)),
        (false, None) => None,
    };
    let straight_to_job = o.job || matches!((o.time_limit, window), (Some(t), Some(w)) if t > w);
    Plan { window, straight_to_job, may_start }
}

/// Whether a solve stopped on `window` rather than on the script's own limit.
fn stopped_by_window(sols: &[Solution], time_limit: Option<f64>, window: Option<f64>) -> bool {
    let Some(w) = window else { return false };
    sols.iter().any(|s| s.status == Status::TimeLimit) && time_limit.is_none_or(|t| t > w)
}

/// A pending answer with this sentence.
fn pending(message: String) -> Vec<Solution> {
    let mut s = Solution::without_point(Status::Pending);
    s.message = message;
    vec![s]
}

fn ago(ms: u64) -> String {
    let s = jobs::now_ms().saturating_sub(ms) / 1000;
    if s < 90 { format!("{s} s") } else { format!("{} min", s / 60) }
}

/// What a script is told about a job that has its question. Always begins
/// "job <id>", which is how a host finds the jobs an evaluation waits on.
fn pending_message(rec: &Record, runner: Option<&Arc<Runner>>) -> String {
    let landing = runner.map(|r| r.settings.when_landed.clone()).unwrap_or_else(|| "evaluate again once it lands".into());
    match rec.state {
        State::Queued => format!("job {} is queued in {}; {landing}", rec.id, rec.owner),
        _ => format!(
            "job {} is solving it in {}, for {} so far{}; {landing}",
            rec.id,
            rec.owner,
            ago(rec.started_unix_ms.max(rec.queued_unix_ms)),
            rec.time_limit_s.map(|t| format!(" of at most {t:.0} s")).unwrap_or_default()
        ),
    }
}

impl JobSolver {
    pub fn new(cache: Option<Arc<Cache>>, root: Option<std::path::PathBuf>) -> JobSolver {
        JobSolver { identity: highs_identity(), cache, root, runner: OnceLock::new(), counts: Counts::default() }
    }

    pub fn cache(&self) -> Option<&Cache> {
        self.cache.as_deref()
    }

    #[cfg_attr(not(test), allow(dead_code))]
    pub fn jobs(&self) -> Option<Jobs> {
        self.cache.as_deref().map(Jobs::of)
    }

    /// Give this process a job runner: from now on its evaluations that may
    /// start jobs do. `None` without a cache, which jobs deliver through.
    pub fn start_runner(&self, settings: Settings) -> Option<Arc<Runner>> {
        let (cache, root) = (self.cache.clone()?, self.root.clone()?);
        Some(self.runner.get_or_init(|| Runner::new(cache, root, jobs::this_exe(), settings)).clone())
    }

    /// [`JobSolver::start_runner`] with a binary of the caller's choosing,
    /// for tests, whose own executable is the test harness.
    #[cfg(test)]
    pub fn start_runner_with(&self, settings: Settings, exe: std::path::PathBuf) -> Option<Arc<Runner>> {
        let (cache, root) = (self.cache.clone()?, self.root.clone()?);
        Some(self.runner.get_or_init(|| Runner::new(cache, root, exe, settings)).clone())
    }

    pub fn runner(&self) -> Option<&Arc<Runner>> {
        self.runner.get()
    }

    pub fn tally(&self) -> Tally {
        let c = &self.counts;
        Tally {
            hits: c.hits.load(Ordering::Relaxed),
            solved: c.solved.load(Ordering::Relaxed),
            kept: c.kept.load(Ordering::Relaxed),
            pending: c.pending.load(Ordering::Relaxed),
            started: c.started.load(Ordering::Relaxed),
        }
    }

    /// Start the counts again: `cadpole eval --jobs wait` reports what its own
    /// evaluation did, after the rounds that ran the jobs.
    pub fn reset_tally(&self) {
        let c = &self.counts;
        for n in [&c.hits, &c.solved, &c.kept, &c.pending, &c.started] {
            n.store(0, Ordering::Relaxed);
        }
    }

    /// The cache key of one question.
    pub fn key(&self, model: &Model, options: &Options) -> [u8; 32] {
        cache::key_for(&opt::digest(model, options, &self.identity), &cache::platform())
    }

    fn file(&self, key: [u8; 32], source: Source, elapsed_ms: f64, solutions: &[Solution]) {
        let Some(cache) = &self.cache else { return };
        let a = Answer {
            key,
            identity: self.identity.clone(),
            platform: cache::platform(),
            made_unix_ms: unix_ms(SystemTime::now()),
            source,
            limited: None,
            elapsed_ms,
            solutions: solutions.to_vec(),
        };
        // A cache that cannot be written is a slower Cadpole, not a broken one:
        // the answer is still the answer.
        if cache.put(&a).is_ok() {
            self.counts.kept.fetch_add(1, Ordering::Relaxed);
        }
    }

    /// Solve here, with HiGHS, and file the answer if it may be kept.
    fn solve_inline(&self, model: &Model, options: &Options, key: Option<[u8; 32]>) -> Result<Vec<Solution>, String> {
        let clock = Instant::now();
        let sols = cadpole_opt::solve(model, options)?;
        self.counts.solved.fetch_add(1, Ordering::Relaxed);
        if let Some(key) = key
            && !sols.is_empty()
            && sols.iter().all(|s| s.status.is_repeatable())
        {
            self.file(key, Source::Inline, clock.elapsed().as_secs_f64() * 1000.0, &sols);
        }
        Ok(sols)
    }

    /// The answer a finished job left, as a script should see it.
    fn from_record(&self, key: [u8; 32], rec: &Record, outcome: Option<codec::Outcome>) -> Option<Vec<Solution>> {
        match (rec.state, outcome) {
            (State::Done, Some(o)) if !o.solutions.is_empty() => {
                let mut sols = o.solutions;
                if sols.iter().all(|s| s.status.is_repeatable()) {
                    // Its cache entry went (collected); the record still has
                    // it, so it goes back.
                    self.file(key, Source::Job(rec.id.clone()), o.elapsed_ms, &sols);
                } else if !o.message.is_empty() && sols[0].message.is_empty() {
                    sols[0].message = o.message;
                }
                Some(sols)
            }
            // Done, but its outcome is gone: as if it never ran.
            (State::Done, _) => None,
            (State::Failed, _) => Some(error(format!(
                "job {} failed: {}; `cadpole solve --rerun` (or `cadpole jobs cancel --forget {}`) runs it again",
                rec.id, rec.message, rec.id
            ))),
            (State::Cancelled, _) => Some(error(format!(
                "job {} was cancelled; `cadpole solve --rerun` (or `cadpole jobs cancel --forget {}`) runs it again",
                rec.id, rec.id
            ))),
            (State::Queued | State::Running, _) => None,
        }
    }

    fn start_job(&self, runner: &Arc<Runner>, key: [u8; 32], model: &Model, options: &Options) -> Vec<Solution> {
        let question = Options { deadline: None, ..options.clone() };
        let problem = Problem {
            key,
            digest: opt::digest(model, &question, &self.identity),
            identity: self.identity.clone(),
            model: model.clone(),
            options: question,
            // The solve's own limit when it gave one -- a limited answer it
            // accepts -- else the host's.
            time_limit: options.time_limit.or(runner.settings.time_limit),
            accept_limited: options.time_limit.is_some(),
            mem_mb: runner.settings.mem_mb,
            origin: options.origin.clone(),
            owner: runner.settings.owner.clone(),
        };
        match runner.start(&problem) {
            Ok(rec) => {
                if rec.owner_pid == std::process::id() && rec.state.is_active() {
                    self.counts.started.fetch_add(1, Ordering::Relaxed);
                }
                self.counts.pending.fetch_add(1, Ordering::Relaxed);
                match rec.state {
                    State::Queued | State::Running => pending(pending_message(&rec, Some(runner))),
                    // It ended between two looks: the next evaluation reads it.
                    _ => pending(format!("job {} has just ended; evaluate again to read its answer", rec.id)),
                }
            }
            Err(e) => error(e),
        }
    }
}

fn error(message: String) -> Vec<Solution> {
    let mut s = Solution::without_point(Status::Error);
    s.message = message;
    vec![s]
}

impl Solver for JobSolver {
    fn identity(&self) -> &str {
        &self.identity
    }

    fn solve(&self, model: &Model, options: &Options) -> Result<Vec<Solution>, String> {
        let Some(cache) = &self.cache else {
            // No cache, so no jobs either: the plain inline solve, bounded so
            // the deadline never fires inside it (a probe's deadline is
            // already what its filter has left).
            if options.inline_only {
                return self.solve_inline(model, options, None);
            }
            let window = plan(options, None).window;
            return self.solve_inline(model, &Options { deadline: window, ..options.clone() }, None);
        };
        let key = self.key(model, options);
        // 1. The cache.
        if let Some(a) = cache.get(&key)
            && a.identity == self.identity
        {
            self.counts.hits.fetch_add(1, Ordering::Relaxed);
            return Ok(a.solutions);
        }
        // A conflict probe runs here, under the deadline it was given: no
        // job, no window, never pending.
        if options.inline_only {
            return self.solve_inline(model, options, Some(key));
        }
        // 2. A job that has, or had, this question -- any process's.
        let jobs = Jobs::of(cache);
        match jobs.lookup(&key) {
            Found::Active(rec) => {
                if let Some(r) = self.runner() {
                    r.watch(&rec.id);
                }
                self.counts.pending.fetch_add(1, Ordering::Relaxed);
                return Ok(pending(pending_message(&rec, self.runner())));
            }
            Found::Finished(rec, outcome) => {
                if let Some(sols) = self.from_record(key, &rec, outcome) {
                    return Ok(sols);
                }
            }
            Found::Nothing => {}
        }
        // 3. Inline within the window, or a job.
        let runner = self.runner();
        let p = plan(options, runner.map(|r| r.settings.inline_cap));
        if !p.straight_to_job {
            let sols = self.solve_inline(model, &Options { deadline: p.window, ..options.clone() }, Some(key))?;
            if !stopped_by_window(&sols, options.time_limit, p.window) {
                return Ok(sols);
            }
        }
        match runner {
            Some(r) if p.may_start => Ok(self.start_job(r, key, model, options)),
            Some(_) => {
                self.counts.pending.fetch_add(1, Ordering::Relaxed);
                Ok(pending(
                    "it needs a job, and only an evaluation its owner asks for starts one -- not a drag preview, a \
                     render, a sweep or the limited listener; it stays pending until one does"
                        .into(),
                ))
            }
            None => {
                self.counts.pending.fetch_add(1, Ordering::Relaxed);
                Ok(pending(match (options.job, options.deadline) {
                    (true, _) => "it asks for a job (job=True), and this command runs none: `cadpole solve` runs it, \
                                  and `cadpole serve` does by itself"
                        .into(),
                    (false, Some(b)) => format!(
                        "it needs longer than this evaluation's {b:.0} s: `cadpole solve` runs it as a job, and `cadpole \
                         serve` does by itself"
                    ),
                    (false, None) => "it needs a job: `cadpole solve` runs it".into(),
                }))
            }
        }
    }
}

/// What `cadpole eval` says about its solves on stderr, or `None` when it made
/// none.
pub fn summary(t: Tally, cache: Option<&Cache>) -> Option<String> {
    if t.hits + t.solved + t.pending == 0 {
        return None;
    }
    let plural = |n: u64, one: &str, many: &str| format!("{n} {}", if n == 1 { one } else { many });
    let mut s = format!("opt: {} from the cache, {} here", plural(t.hits, "answer", "answers"), plural(t.solved, "solve", "solves"));
    match cache {
        Some(c) => s.push_str(&format!(" ({} kept in {})", t.kept, c.dir().display())),
        None => s.push_str(" (no cache: CADPOLE_OPT_CACHE=off, or it could not be made)"),
    }
    if t.pending > 0 {
        s.push_str(&format!("; {} pending: `cadpole solve` runs it", plural(t.pending, "solve", "solves")));
    }
    Some(s)
}

/// The job ids an evaluation's diagnostics say it is waiting on: every
/// pending solve's message begins "job <id>" (cadpole-script prints it after
/// "pending: ").
pub fn pending_jobs<'a>(messages: impl IntoIterator<Item = &'a str>) -> Vec<String> {
    let mut out: Vec<String> = Vec::new();
    for m in messages {
        let mut rest = m;
        while let Some(i) = rest.find("pending: job ") {
            let tail = &rest[i + "pending: job ".len()..];
            let id: String = tail.chars().take(12).collect();
            if cache::is_job_id(&id) && !out.contains(&id) {
                out.push(id);
            }
            rest = tail;
        }
    }
    out
}

#[cfg(test)]
mod tests;

#[cfg(test)]
mod e2e_tests;
