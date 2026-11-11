//! The installed solver against a real cache directory. These call
//! [`JobSolver`] directly rather than through `cadpole_core::opt::install`, which
//! is first-wins for the whole test binary; two instances on one directory
//! stand for two `cadpole` processes.

use std::path::{Path, PathBuf};
use std::sync::Arc;

use cadpole_engine::cadpole_core::opt::{Column, Model, Options, Row, Sense, Solution, Solver, Status};

use super::cache::{self, Cache};
use super::{JobSolver, Tally};

const INF: f64 = f64::INFINITY;

pub(crate) fn scratch(name: &str) -> PathBuf {
    let p = std::env::temp_dir().join(format!("cadpole-opt-jobs-test-{name}-{}", std::process::id()));
    let _ = std::fs::remove_dir_all(&p);
    p
}

pub(crate) fn solver_on(root: &Path) -> JobSolver {
    JobSolver::new(Some(Arc::new(Cache::open(root, 1 << 30).unwrap())), Some(root.to_path_buf()))
}

fn t(hits: u64, solved: u64, kept: u64) -> Tally {
    Tally { hits, solved, kept, ..Tally::default() }
}

/// A small knapsack: a MILP with a unique optimum, solved in microseconds.
pub(crate) fn knapsack() -> Model {
    let mut m = Model::new(Sense::Maximize);
    let (value, weight) = ([10.0, 13.0, 7.0, 8.0, 4.0], [5.0, 7.0, 4.0, 5.0, 3.0]);
    for (j, v) in value.iter().enumerate() {
        m.columns.push(Column { name: format!("take{j}"), lower: 0.0, upper: 1.0, integer: true, cost: *v });
    }
    let terms: Vec<(u32, f64)> = weight.iter().enumerate().map(|(j, w)| (j as u32, *w)).collect();
    m.push_row(Row { name: "capacity".into(), lower: -INF, upper: 14.0 }, &terms);
    m
}

/// Cornuéjols and Dawande's market split, as cadpole-opt's tests build it:
/// hard for branch and bound, so a short limit always stops it.
pub(crate) fn market_split(rows: usize, n: usize, seed: u64) -> Model {
    let mut state = seed;
    let mut next = || {
        state = state.wrapping_mul(6364136223846793005).wrapping_add(1442695040888963407);
        ((state >> 33) % 100) as f64
    };
    let mut m = Model::new(Sense::Minimize);
    for j in 0..n {
        m.columns.push(Column { name: format!("x{j}"), lower: 0.0, upper: 1.0, integer: true, cost: 0.0 });
    }
    for i in 0..rows {
        m.columns.push(Column { name: format!("over{i}"), lower: 0.0, upper: INF, integer: false, cost: 1.0 });
        m.columns.push(Column { name: format!("under{i}"), lower: 0.0, upper: INF, integer: false, cost: 1.0 });
    }
    for i in 0..rows {
        let a: Vec<f64> = (0..n).map(|_| next()).collect();
        let d = (a.iter().sum::<f64>() / 2.0).floor();
        let mut terms: Vec<(u32, f64)> = a.iter().enumerate().filter(|(_, v)| **v != 0.0).map(|(j, v)| (j as u32, *v)).collect();
        terms.push(((n + 2 * i) as u32, 1.0));
        terms.push(((n + 2 * i + 1) as u32, -1.0));
        m.push_row(Row { name: format!("market {i}"), lower: d, upper: d }, &terms);
    }
    m
}

pub(crate) fn bits(sols: &[Solution]) -> Vec<(Status, Vec<u64>, Option<u64>, u64, u64)> {
    sols.iter()
        .map(|s| {
            let v = s.values.as_ref().map(|v| v.iter().map(|x| x.to_bits()).collect()).unwrap_or_default();
            (s.status, v, s.objective.map(f64::to_bits), s.nodes, s.iterations)
        })
        .collect()
}

#[test]
fn a_second_process_reads_the_first_ones_answer_bit_for_bit() {
    let root = scratch("second-process");
    let m = knapsack();
    let o = Options { count: 3, ..Options::default() };
    let first = solver_on(&root);
    let a = first.solve(&m, &o).unwrap();
    assert_eq!(a[0].status, Status::Optimal);
    assert_eq!(first.tally(), t(0, 1, 1));

    let second = solver_on(&root);
    let b = second.solve(&m, &o).unwrap();
    assert_eq!(second.tally(), t(1, 0, 0), "the second process solves nothing");
    assert_eq!(bits(&a), bits(&b));
    assert_eq!(a, b, "every field, the conflict and the duals included");
    let _ = std::fs::remove_dir_all(&root);
}

#[test]
fn the_evaluations_budget_is_not_part_of_the_question() {
    let root = scratch("budget");
    let s = solver_on(&root);
    let m = knapsack();
    s.solve(&m, &Options { deadline: Some(19.5), ..Options::default() }).unwrap();
    s.solve(&m, &Options { deadline: Some(110.0), ..Options::default() }).unwrap();
    s.solve(&m, &Options::default()).unwrap();
    assert_eq!(s.tally(), t(2, 1, 1), "one entry serves every budget");
    // A different question is a different entry.
    s.solve(&m, &Options { mip_rel_gap: 0.0, ..Options::default() }).unwrap();
    s.solve(&m, &Options { time_limit: Some(30.0), ..Options::default() }).unwrap();
    assert_eq!(s.tally().solved, 3);
    let _ = std::fs::remove_dir_all(&root);
}

#[test]
fn a_time_limited_answer_is_never_kept() {
    let root = scratch("limited");
    let s = solver_on(&root);
    let m = market_split(4, 30, 11);
    // The evaluation's budget stopped it: with a cache to deliver through,
    // that solve needs a job, and this process runs none -- pending, saying
    // what would run it, rather than a clock-dependent point.
    let stopped = s.solve(&m, &Options { deadline: Some(0.05), ..Options::default() }).unwrap();
    assert_eq!(stopped[0].status, Status::Pending, "{:?}", stopped[0].status);
    assert!(stopped[0].message.contains("cadpole solve"), "{}", stopped[0].message);
    assert_eq!(s.tally(), Tally { pending: 1, ..t(0, 1, 0) });
    let again = s.solve(&m, &Options { time_limit: Some(0.05), ..Options::default() }).unwrap();
    assert_eq!(again[0].status, Status::TimeLimit, "the script's own limit stops it");
    assert_eq!(s.tally(), Tally { pending: 1, ..t(0, 2, 0) }, "and that answer is not kept either");
    assert_eq!(s.cache().unwrap().usage().0, 0);
    // A node limit counts work, not time: that answer is the
    // question's, and kept.
    let nodes = Options { max_nodes: Some(20), mip_rel_gap: 0.0, ..Options::default() };
    let a = s.solve(&m, &nodes).unwrap();
    assert_eq!(a[0].status, Status::NodeLimit);
    let b = solver_on(&root).solve(&m, &nodes).unwrap();
    assert_eq!(bits(&a), bits(&b));
    assert_eq!(s.cache().unwrap().usage().0, 1);
    let _ = std::fs::remove_dir_all(&root);
}

#[test]
fn an_infeasible_answer_is_kept_with_its_conflict() {
    let root = scratch("infeasible");
    let mut m = Model::new(Sense::Minimize);
    m.columns.push(Column { name: "x".into(), lower: 0.0, upper: 10.0, integer: false, cost: 1.0 });
    m.push_row(Row { name: "low".into(), lower: 20.0, upper: INF }, &[(0, 1.0)]);
    let a = solver_on(&root).solve(&m, &Options::default()).unwrap();
    assert_eq!(a[0].status, Status::Infeasible);
    assert!(a[0].conflict.as_ref().is_some_and(|c| !c.rows.is_empty()), "{:?}", a[0].conflict);
    let s = solver_on(&root);
    let b = s.solve(&m, &Options::default()).unwrap();
    assert_eq!(s.tally().hits, 1);
    assert_eq!(a, b);
    let _ = std::fs::remove_dir_all(&root);
}

#[test]
fn an_entry_from_another_solver_is_not_served() {
    let root = scratch("foreign");
    let s = solver_on(&root);
    let m = knapsack();
    let o = Options::default();
    let mut doctored = s.solve(&m, &o).unwrap();
    doctored[0].objective = Some(-1.0);
    // Someone else's file under this key: it says another solver made it.
    let key = s.key(&m, &o);
    let c = s.cache().unwrap();
    let mut a = c.get(&key).unwrap();
    a.identity = "SomeOtherSolver 9".into();
    a.solutions = doctored;
    c.put(&a).unwrap();
    let back = s.solve(&m, &o).unwrap();
    assert_ne!(back[0].objective, Some(-1.0), "the foreign entry was not served");
    assert_eq!(s.tally().solved, 2);
    let _ = std::fs::remove_dir_all(&root);
}

#[test]
fn without_a_cache_every_solve_runs() {
    let s = JobSolver::new(None, None);
    let m = knapsack();
    s.solve(&m, &Options::default()).unwrap();
    s.solve(&m, &Options::default()).unwrap();
    assert_eq!(s.tally(), t(0, 2, 0));
    assert!(super::summary(s.tally(), None).unwrap().contains("no cache"));
    assert_eq!(super::summary(Tally::default(), None), None, "nothing to say about a script that solved nothing");
}

/// A CLI cache hit on a second run: the real binary, twice,
/// on one cache directory.
#[test]
fn a_second_cadpole_eval_is_a_cache_hit() {
    let Some(cadpole) = cadpole_binary() else {
        eprintln!("skipped: the cadpole binary is not in this test's runfiles");
        return;
    };
    let root = scratch("cli");
    std::fs::create_dir_all(&root).unwrap();
    let doc = root.join("main.star");
    std::fs::write(
        &doc,
        "p = opt.problem(name = \"sizes\")\n\
         w = p.var(\"w\", min = 8, max = 20)\n\
         d = p.var(\"d\", min = 8, max = 20)\n\
         pick = p.binary(\"pick\")\n\
         p.constrain(w + d + 4 * pick, max = 30, name = \"perimeter\")\n\
         p.maximize(2 * w + d + 3 * pick)\n\
         sol = p.solve()\n\
         print(\"answer\", sol.status, sol.value(w), sol.value(d), sol.value(pick))\n",
    )
    .unwrap();
    let run = || {
        let out = std::process::Command::new(&cadpole)
            .arg("eval")
            .arg(&doc)
            .env("CADPOLE_OPT_CACHE", root.join("cache"))
            .output()
            .expect("cadpole runs");
        assert!(out.status.success(), "{}", String::from_utf8_lossy(&out.stderr));
        (String::from_utf8_lossy(&out.stdout).into_owned(), String::from_utf8_lossy(&out.stderr).into_owned())
    };
    let (out1, err1) = run();
    let (out2, err2) = run();
    assert!(err1.contains("opt: 0 answers from the cache, 1 solve here (1 kept in"), "{err1}");
    assert!(err2.contains("opt: 1 answer from the cache, 0 solves here"), "{err2}");
    let answer = |s: &str| s.lines().find(|l| l.contains("print: answer")).map(str::to_owned);
    assert_eq!(answer(&out1).or_else(|| answer(&err1)), answer(&out2).or_else(|| answer(&err2)), "the same answer both times");
    let _ = std::fs::remove_dir_all(&root);
}

/// The `cadpole` binary this test was built beside (`data = [":cadpole"]`).
pub(crate) fn cadpole_binary() -> Option<PathBuf> {
    let r = runfiles::Runfiles::create().ok()?;
    let p = runfiles::rlocation!(r, "cadpole/src/rust/cadpole-cli/cadpole")?;
    p.exists().then_some(p)
}

#[test]
fn the_key_changes_with_the_platform_and_the_digest_only() {
    let s = JobSolver::new(None, None);
    let m = knapsack();
    let k = s.key(&m, &Options::default());
    assert_eq!(k, s.key(&m, &Options { deadline: Some(3.0), ..Options::default() }));
    assert_ne!(k, s.key(&m, &Options { count: 2, ..Options::default() }));
    let d = cadpole_engine::cadpole_core::opt::digest(&m, &Options::default(), s.identity());
    assert_eq!(k, cache::key_for(&d, &cache::platform()));
}

// ------------------------------------------------- where a solve runs

#[test]
fn the_window_leaves_the_evaluation_time_and_a_job_its_chance() {
    use super::plan;
    let o = |deadline: Option<f64>, time_limit: Option<f64>, job: bool, may: bool| Options {
        deadline,
        time_limit,
        job,
        may_start_job: may,
        ..Options::default()
    };
    // cadpole serve, the owner's evaluation: half of what is left, at most the
    // runner's cap.
    let p = plan(&o(Some(19.5), None, false, true), Some(10.0));
    assert_eq!((p.window, p.straight_to_job, p.may_start), (Some(9.75), false, true));
    assert_eq!(plan(&o(Some(4.0), None, false, true), Some(10.0)).window, Some(2.0));
    assert_eq!(plan(&o(Some(119.0), None, false, true), Some(10.0)).window, Some(10.0), "cadpole solve: the cap");
    assert_eq!(plan(&o(None, None, false, true), Some(10.0)).window, Some(10.0));
    // job=True, and a time_limit= past the window, skip the attempt.
    assert!(plan(&o(Some(19.5), None, true, true), Some(10.0)).straight_to_job);
    assert!(plan(&o(Some(19.5), Some(600.0), false, true), Some(10.0)).straight_to_job);
    assert!(!plan(&o(Some(19.5), Some(5.0), false, true), Some(10.0)).straight_to_job, "a 5-s limit fits");
    // No runner, or no permission: all but a margin, so the deadline never
    // fires inside the solve.
    let p = plan(&o(Some(120.0), None, false, false), Some(10.0));
    assert_eq!((p.window, p.may_start), (Some(108.0), false));
    assert_eq!(plan(&o(Some(20.0), None, false, true), None).window, Some(18.0), "no runner: like no permission");
    assert_eq!(plan(&o(Some(1.5), None, false, false), None).window, Some(0.75), "never less than half");
    assert_eq!(plan(&o(None, None, false, false), None).window, None, "no deadline, no window: the plain inline solve");
    assert!(plan(&o(Some(120.0), Some(600.0), false, false), None).straight_to_job, "pending at once, not after 108 s");
}

#[test]
fn the_jobs_an_evaluation_waits_on_are_read_from_its_diagnostics() {
    let msgs = [
        "plan_fit: pending: job 3f9a2c1b7e44 is solving it in cadpole serve (pid 812), for 4 s so far; ...",
        "floor_l0: pending: job 0123456789ab is queued in cadpole serve (pid 812); ...",
        "again: pending: job 3f9a2c1b7e44 is solving it",
        "sizes: pending: it needs a job, and only an evaluation its owner asks for starts one",
        "broken: pending: job 3F9A2C1B7E44 is not an id",
    ];
    assert_eq!(super::pending_jobs(msgs), vec!["3f9a2c1b7e44".to_owned(), "0123456789ab".to_owned()]);
}

/// A solver with a job runner on `root`, running the real binary.
fn with_runner(root: &Path, inline_cap: f64) -> Option<(JobSolver, Arc<super::jobs::Runner>)> {
    let exe = cadpole_binary()?;
    let s = solver_on(root);
    let mut settings = super::jobs::Settings::new(format!("test (pid {})", std::process::id()));
    settings.inline_cap = inline_cap;
    settings.time_limit = Some(300.0);
    settings.mem_mb = 4096;
    let r = s.start_runner_with(settings, exe)?;
    Some((s, r))
}

fn owner(deadline: f64) -> Options {
    Options { deadline: Some(deadline), may_start_job: true, origin: "fit (main.star:9)".into(), ..Options::default() }
}

fn job_of(sols: &[Solution]) -> String {
    assert_eq!(sols[0].status, Status::Pending, "{:?}: {}", sols[0].status, sols[0].message);
    let ids = super::pending_jobs([format!("x: pending: {}", sols[0].message).as_str()]);
    assert_eq!(ids.len(), 1, "{}", sols[0].message);
    ids[0].clone()
}

#[test]
fn a_solve_too_long_for_its_window_becomes_a_job_whose_answer_is_the_inline_one() {
    let root = scratch("policy-job");
    let Some((s, r)) = with_runner(&root, 0.05) else {
        eprintln!("skipped: no cadpole binary in the runfiles");
        return;
    };
    let m = market_split(3, 20, 11);
    let first = s.solve(&m, &owner(20.0)).unwrap();
    let id = job_of(&first);
    assert!(first[0].message.contains("is solving it in test"), "{}", first[0].message);
    assert_eq!(s.tally().started, 1);
    // Asked again while it runs: the same job, and no second inline attempt.
    let solved = s.tally().solved;
    assert_eq!(job_of(&s.solve(&m, &owner(20.0)).unwrap()), id);
    assert_eq!(s.tally().solved, solved, "a question a job has is not solved again inline");
    let (recs, done) = r.wait(std::slice::from_ref(&id), std::time::Duration::from_secs(240));
    assert!(done && recs[0].state == super::jobs::State::Done, "{recs:?}");
    // It landed: the next evaluation reads the answer, and it is the one an
    // evaluation with time enough gets inline.
    let landed = s.solve(&m, &owner(20.0)).unwrap();
    assert_eq!(landed[0].status, Status::Optimal);
    let inline = cadpole_opt::solve(&m, &Options::default()).unwrap();
    assert_eq!(bits(&landed), bits(&inline), "same question, same answer, bit for bit");
    let _ = std::fs::remove_dir_all(&root);
}

#[test]
fn job_true_skips_the_inline_attempt_and_changes_nothing_but_where() {
    let root = scratch("policy-job-true");
    let Some((s, r)) = with_runner(&root, 10.0) else {
        eprintln!("skipped: no cadpole binary in the runfiles");
        return;
    };
    let m = knapsack();
    let o = Options { job: true, ..owner(20.0) };
    let id = job_of(&s.solve(&m, &o).unwrap());
    assert_eq!(s.tally().solved, 0, "not even tried inline");
    r.wait(std::slice::from_ref(&id), std::time::Duration::from_secs(120));
    // The job's answer serves the same question asked without job=: the
    // option is not part of it.
    let plain = s.solve(&m, &owner(20.0)).unwrap();
    assert_eq!(s.tally().hits, 1);
    assert_eq!(bits(&plain), bits(&cadpole_opt::solve(&m, &Options::default()).unwrap()));
    let _ = std::fs::remove_dir_all(&root);
}

#[test]
fn an_evaluation_that_may_not_start_a_job_never_does_but_attaches_to_one() {
    let root = scratch("policy-listener");
    let Some((s, r)) = with_runner(&root, 0.05) else {
        eprintln!("skipped: no cadpole binary in the runfiles");
        return;
    };
    let m = market_split(3, 20, 11);
    // The listener, a preview, a render: no permission. The window is long
    // (all but a margin), so make the budget short to force the question.
    let listener = Options { may_start_job: false, ..owner(0.1) };
    let sols = s.solve(&m, &listener).unwrap();
    assert_eq!(sols[0].status, Status::Pending);
    assert!(sols[0].message.contains("only an evaluation its owner asks for"), "{}", sols[0].message);
    assert_eq!(s.tally().started, 0);
    assert!(s.jobs().unwrap().all().is_empty(), "no job was started");
    // The owner's evaluation starts it; the listener's next one attaches.
    let id = job_of(&s.solve(&m, &owner(20.0)).unwrap());
    assert_eq!(job_of(&s.solve(&m, &listener).unwrap()), id);
    r.wait(std::slice::from_ref(&id), std::time::Duration::from_secs(240));
    assert_eq!(s.solve(&m, &listener).unwrap()[0].status, Status::Optimal, "and reads its answer");
    let _ = std::fs::remove_dir_all(&root);
}

#[test]
fn a_job_that_failed_is_reported_and_not_rerun_behind_the_users_back() {
    let root = scratch("policy-failed");
    let s = solver_on(&root);
    let m = knapsack();
    let key = s.key(&m, &Options::default());
    let jobs = s.jobs().unwrap();
    let id = cache::job_id(&key);
    let rec = super::jobs::Record {
        id: id.clone(),
        key: cache::hex(&key),
        state: super::jobs::State::Failed,
        owner: "cadpole serve (pid 1)".into(),
        message: "out of its 8,192-MB memory limit".into(),
        queued_unix_ms: super::jobs::now_ms(),
        ..Default::default()
    };
    jobs.write(s.cache().unwrap(), &rec).unwrap();
    let sols = s.solve(&m, &owner(20.0)).unwrap();
    assert_eq!(sols[0].status, Status::Error);
    assert!(sols[0].message.contains(&format!("job {id} failed: out of its")) && sols[0].message.contains("--rerun"), "{}", sols[0].message);
    assert_eq!(s.tally().solved, 0, "not solved again on its own");
    // Forgotten, it runs again.
    jobs.forget(&id);
    assert_eq!(s.solve(&m, &owner(20.0)).unwrap()[0].status, Status::Optimal);
    let _ = std::fs::remove_dir_all(&root);
}

/// A conflict probe runs where it is asked, through the real solver: a solve
/// whose time limit is longer than the evaluation has left used to send each
/// probe to the job plan, which turned it pending, so the conflict came back
/// with every rule in it and phantom pending solves. The probes are cached
/// questions too: a second filter answers every one from the cache.
#[test]
fn conflict_probes_run_inline_and_are_cached() {
    use cadpole_engine::cadpole_core::opt::{DeletionLimits, conflict_by_deletion};
    let root = scratch("probes");
    let s = solver_on(&root);
    // Two integers with 2x + 2y = 3 (no integer answer), and two rules that
    // hold anyway.
    let mut m = Model::new(Sense::Minimize);
    for name in ["x", "y"] {
        m.columns.push(Column { name: name.into(), lower: 0.0, upper: 10.0, integer: true, cost: 1.0 });
    }
    m.push_row(Row { name: "odd".into(), lower: 3.0, upper: 3.0 }, &[(0, 2.0), (1, 2.0)]);
    m.push_row(Row { name: "cap".into(), lower: -INF, upper: 20.0 }, &[(0, 1.0), (1, 1.0)]);
    m.push_row(Row { name: "lead".into(), lower: -INF, upper: 10.0 }, &[(0, 1.0)]);
    let options = Options { time_limit: Some(200.0), deadline: Some(100.0), may_start_job: false, ..Options::default() };
    let d = conflict_by_deletion(&s, &m, &options, &[], DeletionLimits::default());
    assert_eq!(d.rows, vec![0], "the conflict is the one rule: {d:?}");
    assert_eq!(d.unanswered, 0, "{d:?}");
    assert_eq!(s.tally().pending, 0, "{:?}", s.tally());
    let probes = d.probes as u64;
    let again = solver_on(&root);
    let d2 = conflict_by_deletion(&again, &m, &options, &[], DeletionLimits::default());
    assert_eq!(d2.rows, vec![0]);
    assert_eq!(again.tally().hits, probes, "every probe answered from the cache: {:?}", again.tally());
    assert_eq!(again.tally().solved, 0);
    let _ = std::fs::remove_dir_all(&root);
}
