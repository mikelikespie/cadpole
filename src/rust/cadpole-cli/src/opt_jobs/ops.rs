//! The four job operations of `ops.proto` -- `Solve`, `ListJobs`,
//! `WaitJobs`, `CancelJobs` -- as every host that runs jobs answers them:
//! `cadpole serve`'s `ServerHost`, and [`LocalHost`] for `cadpole solve`, `cadpole
//! jobs` and `cadpole op` without a server. `cadpole mcp` proxies them to the
//! session's server (`ops_host.rs`), so an assistant's jobs are the server's.

use std::collections::BTreeSet;
use std::sync::Arc;
use std::time::{Duration, Instant};

use cadpole_engine::cadpole_proto::v1 as pb;

use super::cache::{Cache, is_job_id};
use super::jobs::{Jobs, Record, Runner, Settings, State};

/// The longest a job operation waits when a network caller asks: the cap
/// `WaitEvents` has, so an MCP tool call or an HTTP request comes back.
pub const MAX_REMOTE_WAIT: Duration = Duration::from_secs(120);

fn rfc3339(ms: u64) -> String {
    if ms == 0 {
        return String::new();
    }
    chrono::DateTime::from_timestamp_millis(ms as i64)
        .map(|t| t.to_rfc3339_opts(chrono::SecondsFormat::Secs, true))
        .unwrap_or_default()
}

/// A record as the operations show it.
pub fn job_proto(r: &Record) -> pb::SolveJob {
    let elapsed_ms = match r.state {
        State::Running => super::jobs::now_ms().saturating_sub(r.started_unix_ms.max(r.queued_unix_ms)) as f64,
        _ => r.elapsed_ms,
    };
    pb::SolveJob {
        id: r.id.clone(),
        state: r.state.as_str().to_owned(),
        origin: r.origin.clone(),
        status: r.status.clone(),
        objective: r.objective,
        gap: r.gap,
        nodes: r.nodes,
        elapsed_s: elapsed_ms / 1000.0,
        time_limit_s: r.time_limit_s.unwrap_or(0.0),
        kept: r.kept,
        message: r.message.clone(),
        owner: r.owner.clone(),
        pid: if r.state == State::Running { r.pid } else { 0 },
        queued_at: rfc3339(r.queued_unix_ms),
        finished_at: rfc3339(r.finished_unix_ms),
        variables: r.variables,
        constraints: r.constraints,
        nonzeros: r.nonzeros,
    }
}

fn check_ids(ids: &[String]) -> anyhow::Result<()> {
    for id in ids {
        anyhow::ensure!(is_job_id(id), "{id:?} is not a job id: twelve hex digits, as a pending solve names it");
    }
    Ok(())
}

/// Whether the default list shows `r`: running or queued, or ended since
/// `since`. A job found lost -- its process killed before it could record
/// an end -- has no end time, and has not ended in any hour: it is shown
/// until the next evaluation runs it again or it is forgotten.
fn recent(r: &Record, since: u64) -> bool {
    r.state.is_active() || r.finished_unix_ms >= since || r.finished_unix_ms == 0
}

/// `ListJobs`.
pub fn list(jobs: &Jobs, cache: &Cache, req: &pb::ListJobsRequest) -> anyhow::Result<pb::ListJobsResponse> {
    check_ids(&req.ids)?;
    let hour_ago = super::jobs::now_ms().saturating_sub(3_600_000);
    let records: Vec<pb::SolveJob> = jobs
        .all()
        .into_iter()
        .map(|r| jobs.current(r))
        .filter(|r| if req.ids.is_empty() { req.all || recent(r, hour_ago) } else { req.ids.contains(&r.id) })
        .map(|r| job_proto(&r))
        .collect();
    let (answers, bytes) = cache.usage();
    Ok(pb::ListJobsResponse {
        jobs: records,
        cache: Some(pb::SolveCache {
            path: cache.dir().display().to_string(),
            answers: answers as u64,
            bytes,
            cap_bytes: cache.cap(),
        }),
    })
}

/// `WaitJobs`; `cap` bounds what a network caller may ask for.
pub fn wait(runner: &Runner, req: &pb::WaitJobsRequest, cap: Option<Duration>) -> anyhow::Result<pb::WaitJobsResponse> {
    check_ids(&req.ids)?;
    let ids: Vec<String> = if req.ids.is_empty() {
        runner.jobs().all().into_iter().map(|r| runner.jobs().current(r)).filter(|r| r.state.is_active()).map(|r| r.id).collect()
    } else {
        req.ids.clone()
    };
    let asked = Duration::from_millis(if req.timeout_ms == 0 { 30_000 } else { req.timeout_ms as u64 });
    let timeout = cap.map_or(asked, |c| asked.min(c));
    let (records, done) = runner.wait(&ids, timeout);
    Ok(pb::WaitJobsResponse { jobs: records.iter().map(job_proto).collect(), done })
}

/// `CancelJobs`.
pub fn cancel(runner: &Runner, req: &pb::CancelJobsRequest) -> anyhow::Result<pb::CancelJobsResponse> {
    check_ids(&req.ids)?;
    let jobs = runner.jobs();
    let ids: Vec<String> = if req.all {
        jobs.all().into_iter().map(|r| jobs.current(r)).filter(|r| r.state.is_active() || req.forget).map(|r| r.id).collect()
    } else {
        anyhow::ensure!(!req.ids.is_empty(), "CancelJobs: name the jobs (`ids`), or ask for `all`");
        req.ids.clone()
    };
    let mut out = Vec::new();
    for id in &ids {
        let Some(rec) = runner.cancel(id) else {
            anyhow::bail!("no job {id} on this machine (`cadpole jobs --all` lists them)");
        };
        if req.forget {
            jobs.forget(id);
            out.push(job_proto(&Record { message: "cancelled and forgotten: the next evaluation runs it again".into(), ..rec }));
        } else {
            out.push(job_proto(&rec));
        }
    }
    Ok(pb::CancelJobsResponse { jobs: out })
}

/// One evaluation, as `Solve` reads it.
pub struct Evaluated {
    pub diagnostics: Vec<pb::Diagnostic>,
}

impl Evaluated {
    pub fn of(ctx: &cadpole_engine::cadpole_core::context::Context) -> Evaluated {
        Evaluated { diagnostics: ctx.diagnostics.iter().map(cadpole_engine::cadpole_core::context::Diagnostic::to_proto).collect() }
    }

    fn messages(&self) -> impl Iterator<Item = &str> {
        self.diagnostics.iter().map(|d| d.message.as_str())
    }

    fn has_error(&self) -> bool {
        self.diagnostics.iter().any(|d| d.severity == pb::Severity::Error as i32)
    }
}

/// The jobs an evaluation reports as ended badly -- failed, cancelled, or
/// stopped at a host's time limit -- which `rerun` runs again.
pub fn ended_jobs<'a>(messages: impl IntoIterator<Item = &'a str>) -> Vec<String> {
    let mut out = Vec::new();
    for m in messages {
        for (i, _) in m.match_indices("job ") {
            let id: String = m[i + 4..].chars().take(12).collect();
            let rest = &m[(i + 4 + id.len()).min(m.len())..];
            if is_job_id(&id)
                && (rest.starts_with(" failed") || rest.starts_with(" was cancelled") || rest.starts_with(" stopped at its"))
                && !out.contains(&id)
            {
                out.push(id);
            }
        }
    }
    out
}

/// `Solve`: evaluate, wait for the jobs the evaluation is pending on,
/// evaluate again, until nothing is pending or `wait` is spent. `evaluate`
/// is the host's evaluation of the request's document, with jobs allowed.
pub fn solve(
    runner: &Runner,
    req: &pb::SolveRequest,
    cap: Option<Duration>,
    evaluate: &dyn Fn() -> anyhow::Result<Evaluated>,
) -> anyhow::Result<pb::SolveResponse> {
    let asked = if req.wait_ms == u32::MAX { Duration::MAX } else { Duration::from_millis(req.wait_ms as u64) };
    let wait = cap.map_or(asked, |c| asked.min(c));
    let start = Instant::now();
    let deadline = start.checked_add(wait);
    let mut evaluations = 0u32;
    let mut seen: BTreeSet<String> = BTreeSet::new();
    let mut ev = evaluate()?;
    evaluations += 1;
    if req.rerun {
        let ended = ended_jobs(ev.messages());
        if !ended.is_empty() {
            for id in &ended {
                runner.jobs().forget(id);
            }
            ev = evaluate()?;
            evaluations += 1;
        }
    }
    let mut pending;
    // Each round waits for every job the evaluation is pending on, then looks
    // again: a chain (floor by floor) becomes pending one link at a time.
    // A round that ends with nothing landed ends the call.
    loop {
        pending = super::pending_jobs(ev.messages());
        seen.extend(pending.iter().cloned());
        let left = deadline.map(|d| d.saturating_duration_since(Instant::now()));
        if pending.is_empty() || left.is_some_and(|l| l.is_zero()) {
            break;
        }
        let (_, done) = runner.wait(&pending, left.unwrap_or(Duration::MAX).min(Duration::from_secs(365 * 24 * 3600)));
        if !done {
            break;
        }
        ev = evaluate()?;
        evaluations += 1;
    }
    let jobs: Vec<pb::SolveJob> = seen
        .iter()
        .filter_map(|id| runner.jobs().read(id).map(|r| runner.jobs().current(r)))
        .map(|r| job_proto(&r))
        .collect();
    let failed = jobs.iter().any(|j| j.state == "failed" || j.state == "cancelled");
    let ok = pending.is_empty() && !failed && !ev.has_error();
    Ok(pb::SolveResponse { jobs, pending: pending.len() as u32, evaluations, diagnostics: ev.diagnostics, ok })
}

/// A host with no session that runs its own jobs: `cadpole solve`, `cadpole
/// jobs`, `cadpole op` without `--server`.
pub struct LocalHost {
    pub doc: Option<pb::Document>,
    /// How this process runs jobs, should it need to.
    pub settings: Settings,
    /// A memo across `Solve`'s rounds, so each evaluation after the first
    /// rebuilds the geometry from it.
    memo: Arc<cadpole_engine::cadpole_core::memo::Memo>,
}

impl LocalHost {
    pub fn new(doc: Option<pb::Document>, settings: Settings) -> LocalHost {
        LocalHost { doc, settings, memo: Arc::new(cadpole_engine::cadpole_core::memo::Memo::new()) }
    }

    fn runner(&self) -> anyhow::Result<Arc<Runner>> {
        let solver = super::installed().ok_or_else(|| anyhow::anyhow!("no solver is installed in this process"))?;
        if let Some(r) = solver.runner() {
            return Ok(r.clone());
        }
        solver.start_runner(self.settings.clone()).ok_or_else(|| {
            anyhow::anyhow!("solve jobs need the solution cache, and there is none: CADPOLE_OPT_CACHE=off, or it could not be made")
        })
    }
}

impl cadpole_ops::Host for LocalHost {
    fn document(&self) -> anyhow::Result<pb::Document> {
        self.doc.clone().ok_or_else(|| anyhow::anyhow!("no document: pass one with --doc FILE, or --server URL to use a session's"))
    }

    fn solve(&self, req: &pb::SolveRequest) -> anyhow::Result<pb::SolveResponse> {
        let runner = self.runner()?;
        let doc = match &req.document {
            Some(d) => d.clone(),
            None => self.document()?,
        };
        let memo = self.memo.clone();
        let evaluate = move || -> anyhow::Result<Evaluated> {
            let (ctx, _, _) = cadpole_engine::evaluate_document_options(
                &doc,
                &Default::default(),
                0.0,
                0.0,
                &Default::default(),
                cadpole_engine::limits::default_limits(),
                cadpole_engine::EvalOptions { memo: Some(memo.clone()), preview: false, solve_jobs: true },
            );
            Ok(Evaluated::of(&ctx))
        };
        solve(&runner, req, None, &evaluate)
    }

    fn list_jobs(&self, req: &pb::ListJobsRequest) -> anyhow::Result<pb::ListJobsResponse> {
        let solver = super::installed().ok_or_else(|| anyhow::anyhow!("no solver is installed in this process"))?;
        let cache = solver.cache().ok_or_else(|| anyhow::anyhow!("there is no solution cache: CADPOLE_OPT_CACHE=off, or it could not be made"))?;
        let jobs = Jobs::of(cache);
        // Listing is when a CLI user looks, so it is when week-old records go.
        jobs.collect();
        list(&jobs, cache, req)
    }

    fn wait_jobs(&self, req: &pb::WaitJobsRequest) -> anyhow::Result<pb::WaitJobsResponse> {
        let r = self.runner()?;
        wait(&r, req, None)
    }

    fn cancel_jobs(&self, req: &pb::CancelJobsRequest) -> anyhow::Result<pb::CancelJobsResponse> {
        let r = self.runner()?;
        cancel(&r, req)
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn rerun_finds_the_jobs_that_ended_badly() {
        let msgs = [
            "fit: the solver stopped (error) job 3f9a2c1b7e44 failed: out of its 8,192-MB memory limit; `cadpole solve --rerun` ...",
            "floor: the solver stopped (error) job 0123456789ab was cancelled; ...",
            "roof: stopped at the time limit with a point within 2.1% of the best bound; job abcdefabcdef stopped at its 3600-s time limit (cadpole serve's ...)",
            "kept: stopped at the time limit with a point; kept from job 111111111111, which ran to its 600-s limit",
            "fine: pending: job 222222222222 is solving it",
        ];
        assert_eq!(ended_jobs(msgs), vec!["3f9a2c1b7e44", "0123456789ab", "abcdefabcdef"], "an accepted limit and a running job are not rerun");
    }

    #[test]
    fn a_record_reads_as_the_operations_show_it() {
        let r = Record {
            id: "3f9a2c1b7e44".into(),
            state: State::Done,
            status: "optimal".into(),
            objective: Some(1.0),
            elapsed_ms: 31_200.0,
            time_limit_s: Some(3600.0),
            pid: 77,
            queued_unix_ms: 1_793_000_000_000,
            finished_unix_ms: 1_793_000_031_200,
            ..Default::default()
        };
        let p = job_proto(&r);
        assert_eq!((p.state.as_str(), p.status.as_str(), p.objective, p.elapsed_s), ("done", "optimal", Some(1.0), 31.2));
        assert_eq!(p.pid, 0, "a pid is only shown while it means something");
        assert!(p.finished_at.ends_with('Z') && p.queued_at < p.finished_at, "{} {}", p.queued_at, p.finished_at);
        assert!(check_ids(&["../../x".into()]).is_err());
    }
}
