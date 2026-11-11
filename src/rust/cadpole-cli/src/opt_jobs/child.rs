//! `cadpole opt-job <root> <id>`: the process a solve job is. Hidden
//! from `--help`; a parent `cadpole` starts it.
//!
//! It reads the problem its parent wrote, checks it -- the solver it would
//! use is the parent's, the problem hashes to the key it is filed under --
//! puts its limits on itself, solves with `cadpole_opt::solve`, and leaves three
//! things: the outcome (`jobs/<id>.result`), the answer in the cache if it
//! may be kept, and the record's final state. It runs no Starlark: the model
//! is data the parent's sandboxed evaluation built.

use std::path::Path;
use std::process::ExitCode;
use std::time::Instant;

use cadpole_engine::cadpole_core::opt::{self, Options, Solution, Status};

use super::cache::{self, is_job_id};
use super::codec::{self, Answer, Outcome, Source};
use super::jobs::{Jobs, State, now_ms, open_root};
use super::sys::{self, JobLimits};

/// What a job's outcome may become in the cache.
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub enum Keep {
    /// Nothing: it depends on the clock, or on a failure.
    No,
    /// An answer like any other: every solution repeatable.
    Answer,
    /// A limited answer the solve accepted by asking for `time_limit=`:
    /// stopped by that limit, with a point.
    Limited,
}

pub fn keepable(sols: &[Solution], accept_limited: bool) -> Keep {
    if sols.is_empty() {
        return Keep::No;
    }
    if sols.iter().all(|s| s.status.is_repeatable()) {
        return Keep::Answer;
    }
    let limited = sols.iter().all(|s| s.status.is_repeatable() || s.status == Status::TimeLimit) && sols[0].values.is_some();
    if accept_limited && limited { Keep::Limited } else { Keep::No }
}

pub fn run(root: &Path, id: &str) -> ExitCode {
    let clock = Instant::now();
    if !is_job_id(id) {
        eprintln!("opt-job: {id:?} is not a job id");
        return ExitCode::from(2);
    }
    let cache = match open_root(root) {
        Ok(c) => c,
        Err(e) => {
            eprintln!("opt-job {id}: the cache at {}: {e}", root.display());
            return ExitCode::from(2);
        }
    };
    let jobs = Jobs::of(&cache);
    let fail = |message: String| -> ExitCode {
        eprintln!("opt-job {id}: {message}");
        if let Some(mut rec) = jobs.read(id) {
            rec.state = State::Failed;
            rec.finished_unix_ms = now_ms();
            rec.elapsed_ms = clock.elapsed().as_secs_f64() * 1000.0;
            rec.message = message;
            let _ = jobs.write(&cache, &rec);
        }
        ExitCode::from(1)
    };
    let bytes = match std::fs::read(jobs.problem_path(id)) {
        Ok(b) => b,
        Err(e) => return fail(format!("its problem file: {e}")),
    };
    let problem = match codec::decode_problem(&bytes) {
        Ok(p) => p,
        Err(e) => return fail(format!("its problem file does not decode: {e}")),
    };
    // The solver here must be the one the parent keyed the question by, or
    // the answer would be filed under a key no evaluation will look up.
    let solver = super::JobSolver::new(None, None);
    let identity = opt::Solver::identity(&solver).to_owned();
    if problem.identity != identity {
        return fail(format!(
            "its parent solves with {:?} and this binary with {identity:?}; restart the process that started it",
            problem.identity
        ));
    }
    let digest = opt::digest(&problem.model, &problem.options, &identity);
    if digest != problem.digest || cache::key_for(&digest, &cache::platform()) != problem.key || cache::job_id(&problem.key) != id {
        return fail("its problem does not hash to its key: a codec bug, or a file that is not this job's".into());
    }
    let cpu = problem.time_limit.map(|t| t.ceil() as u64 + 60);
    for p in sys::limit_self(JobLimits { cpu_seconds: cpu, mem_mb: problem.mem_mb }) {
        eprintln!("opt-job {id}: a limit was not set: {p}");
    }
    // The question exactly as asked; the job's own limit goes in `deadline`,
    // which is not part of it, so an unstopped solve is the inline answer.
    let options = Options { deadline: problem.time_limit, ..problem.options.clone() };
    let solved = std::panic::catch_unwind(|| cadpole_opt::solve(&problem.model, &options));
    let sols = match solved {
        Ok(Ok(s)) if !s.is_empty() => s,
        Ok(Ok(_)) => return fail("the solver returned nothing".into()),
        Ok(Err(e)) => return fail(format!("the solver refused it: {e}")),
        Err(_) => return fail("the solver panicked".into()),
    };
    let elapsed_ms = clock.elapsed().as_secs_f64() * 1000.0;
    let first = &sols[0];
    let limited_by_job = first.status == Status::TimeLimit && problem.options.time_limit.is_none();
    let message = if limited_by_job {
        format!(
            "job {id} stopped at its {}-s time limit ({}'s, not the solve's); give the solve a time_limit= to keep a \
             limited answer, or a longer job limit to finish",
            problem.time_limit.map(|t| format!("{t:.0}")).unwrap_or_default(),
            problem.owner
        )
    } else {
        String::new()
    };
    let outcome = Outcome { key: problem.key, elapsed_ms, solutions: sols.clone(), message: message.clone() };
    if let Err(e) = cache.write_atomic(&jobs.result_path(id), &codec::encode_outcome(&outcome)) {
        return fail(format!("writing its result: {e}"));
    }
    let kept = match keepable(&sols, problem.accept_limited) {
        keep @ (Keep::Answer | Keep::Limited) => {
            let limited = keep == Keep::Limited;
            let mut solutions = sols.clone();
            if limited {
                let note = format!(
                    "kept from job {id}, which ran to its {}-s limit",
                    problem.options.time_limit.map(|t| format!("{t:.0}")).unwrap_or_default()
                );
                for s in &mut solutions {
                    if s.status == Status::TimeLimit {
                        s.message = note.clone();
                    }
                }
            }
            let a = Answer {
                key: problem.key,
                identity,
                platform: cache::platform(),
                made_unix_ms: now_ms(),
                source: Source::Job(id.to_owned()),
                limited: if limited { problem.options.time_limit } else { None },
                elapsed_ms,
                solutions,
            };
            cache.put(&a).is_ok()
        }
        Keep::No => false,
    };
    let mut rec = jobs.read(id).unwrap_or_default();
    rec.id = id.to_owned();
    rec.state = State::Done;
    rec.finished_unix_ms = now_ms();
    rec.elapsed_ms = elapsed_ms;
    rec.status = first.status.as_str().to_owned();
    rec.objective = first.objective;
    rec.gap = first.mip_gap;
    rec.nodes = first.nodes;
    rec.kept = kept;
    rec.message = message;
    if let Err(e) = jobs.write(&cache, &rec) {
        eprintln!("opt-job {id}: writing its record: {e}");
    }
    let _ = std::fs::remove_file(jobs.problem_path(id));
    eprintln!(
        "opt-job {id}: {} in {:.1} s{}",
        first.status.as_str(),
        elapsed_ms / 1000.0,
        if kept { ", kept" } else { "" }
    );
    ExitCode::SUCCESS
}
