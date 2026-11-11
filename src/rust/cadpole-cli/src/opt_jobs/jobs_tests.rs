//! Records, claims and the runner. The runner tests start real
//! `cadpole opt-job` children, so they need the binary in the test's runfiles
//! (`data = [":cadpole"]`) and skip, saying so, without it.

use std::sync::Arc;
use std::time::Duration;

use cadpole_engine::cadpole_core::opt::{Options, Solution, Status};

use super::*;
use crate::opt_jobs::cache::{Cache, key_for, platform};
use crate::opt_jobs::codec::{Outcome, Problem};
use crate::opt_jobs::tests::{bits, cadpole_binary, knapsack, market_split, scratch};
use crate::opt_jobs::{JobSolver, cache};

fn jobs_in(root: &std::path::Path) -> (Arc<Cache>, Jobs) {
    let c = Arc::new(Cache::open(root, 1 << 30).unwrap());
    let j = Jobs::of(&c);
    (c, j)
}

fn record(id: &str, key: &[u8; 32], state: State) -> Record {
    Record {
        id: id.into(),
        key: hex(key),
        state,
        owner: "test".into(),
        owner_pid: std::process::id(),
        queued_unix_ms: now_ms(),
        ..Default::default()
    }
}

/// A pid that is not running: a child that has been waited for.
fn dead_pid() -> u32 {
    let mut c = std::process::Command::new("true").spawn().expect("`true` runs");
    let pid = c.id();
    c.wait().unwrap();
    pid
}

#[test]
fn a_record_round_trips_and_spells_its_state() {
    let k = key_for(&[1; 32], &platform());
    let mut r = record(&job_id(&k), &k, State::Done);
    // A number serde_json's own float parsing gets wrong by an ulp.
    r.objective = Some(0.9999999999999831);
    r.gap = None;
    r.message = "kept".into();
    let json = serde_json::to_string(&r).unwrap();
    assert!(json.contains("\"state\":\"done\""), "{json}");
    assert_eq!(serde_json::from_str::<Record>(&json).unwrap(), r);
    assert_eq!(serde_json::from_str::<Record>(&json).unwrap().objective.unwrap().to_bits(), 0.9999999999999831f64.to_bits());
    for (s, w) in [(State::Queued, "queued"), (State::Running, "running"), (State::Done, "done"), (State::Failed, "failed"), (State::Cancelled, "cancelled")] {
        assert_eq!(s.as_str(), w);
        assert_eq!(s.is_active(), matches!(s, State::Queued | State::Running));
    }
}

#[test]
fn a_claim_is_exclusive_until_it_is_dropped_or_stale() {
    let root = scratch("claims");
    let (_c, j) = jobs_in(&root);
    let id = "3f9a2c1b7e44";
    let first = j.claim(id).expect("the first claim");
    assert!(j.claim(id).is_none(), "a held claim is held");
    drop(first);
    let again = j.claim(id).expect("a released claim can be taken");
    // A crashed starter's claim: old enough to break.
    std::mem::forget(again);
    let path = j.dir().join(format!("{id}.claim"));
    std::fs::File::options().write(true).open(&path).unwrap().set_modified(SystemTime::now() - STALE_CLAIM * 2).unwrap();
    assert!(j.claim(id).is_some(), "a stale claim is broken");
    let _ = std::fs::remove_dir_all(&root);
}

#[test]
fn a_record_is_believed_only_while_its_processes_are_alive() {
    let root = scratch("classify");
    let (c, j) = jobs_in(&root);
    let k = key_for(&[2; 32], &platform());
    let id = job_id(&k);
    let dead = dead_pid();

    // Queued by a live owner (this process): active.
    j.write(&c, &record(&id, &k, State::Queued)).unwrap();
    assert!(matches!(j.lookup(&k), Found::Active(_)));
    // Queued by a dead one: lost, so nothing.
    j.write(&c, &Record { owner_pid: dead, ..record(&id, &k, State::Queued) }).unwrap();
    assert!(matches!(j.lookup(&k), Found::Nothing));
    // Running, but the "child" is no job (a dead pid), and the owner is
    // gone too: lost -- a reader sees it failed, the next solve reruns it.
    let lost = Record { pid: dead, owner_pid: dead, ..record(&id, &k, State::Running) };
    j.write(&c, &lost).unwrap();
    assert!(matches!(j.lookup(&k), Found::Nothing));
    let seen = j.current(lost.clone());
    assert_eq!(seen.state, State::Failed);
    assert!(seen.message.starts_with("lost"), "{}", seen.message);
    // ...unless it left a result: then it finished and only the record
    // write was lost.
    let o = Outcome { key: k, elapsed_ms: 1.0, solutions: vec![Solution::without_point(Status::Optimal)], message: String::new() };
    c.write_atomic(&j.result_path(&id), &codec::encode_outcome(&o)).unwrap();
    assert!(matches!(j.lookup(&k), Found::Finished(_, Some(_))));
    let _ = std::fs::remove_file(j.result_path(&id));
    // Running with a dead child, but its live owner is this process, whose
    // reaper is about to say why: still active, never restarted under it.
    j.write(&c, &Record { pid: dead, ..record(&id, &k, State::Running) }).unwrap();
    assert!(matches!(j.lookup(&k), Found::Active(_)));
    // Finished states are finished, whatever the pids...
    for s in [State::Failed, State::Cancelled] {
        j.write(&c, &Record { owner_pid: dead, ..record(&id, &k, s) }).unwrap();
        assert!(matches!(j.lookup(&k), Found::Finished(_, None)), "{s:?}");
    }
    // ...but one that answered and whose answer is gone answers nothing.
    j.write(&c, &Record { owner_pid: dead, ..record(&id, &k, State::Done) }).unwrap();
    assert!(matches!(j.lookup(&k), Found::Nothing));
    c.write_atomic(&j.result_path(&id), &codec::encode_outcome(&o)).unwrap();
    assert!(matches!(j.lookup(&k), Found::Finished(_, Some(_))));
    let _ = std::fs::remove_file(j.result_path(&id));
    // Another question's job under the same id is not this one's.
    let other = key_for(&[3; 32], &platform());
    j.write(&c, &Record { key: hex(&other), ..record(&id, &k, State::Done) }).unwrap();
    assert!(matches!(j.lookup(&k), Found::Nothing));
    let _ = std::fs::remove_dir_all(&root);
}

#[test]
fn finished_jobs_are_collected_after_a_week_and_active_ones_never() {
    let root = scratch("collect-jobs");
    let (c, j) = jobs_in(&root);
    let old = now_ms() - KEEP_FINISHED.as_millis() as u64 - 60_000;
    let keys: Vec<[u8; 32]> = (10..13u8).map(|i| key_for(&[i; 32], &platform())).collect();
    let ids: Vec<String> = keys.iter().map(job_id).collect();
    j.write(&c, &Record { queued_unix_ms: old, finished_unix_ms: old, ..record(&ids[0], &keys[0], State::Done) }).unwrap();
    std::fs::write(j.log_path(&ids[0]), b"old log").unwrap();
    j.write(&c, &Record { queued_unix_ms: old, ..record(&ids[1], &keys[1], State::Queued) }).unwrap();
    j.write(&c, &record(&ids[2], &keys[2], State::Failed)).unwrap();
    assert_eq!(j.collect(), 1);
    assert!(j.read(&ids[0]).is_none() && !j.log_path(&ids[0]).exists(), "the old finished job and its log went");
    assert!(j.read(&ids[1]).is_some(), "an active job is never collected");
    assert!(j.read(&ids[2]).is_some(), "a recent failure stays, so it is not rerun behind the user's back");
    assert_eq!(j.all().len(), 2);
    let _ = std::fs::remove_dir_all(&root);
}

// ------------------------------------------------------------- the runner

fn runner_in(root: &std::path::Path, parallel: usize) -> Option<Arc<Runner>> {
    let exe = cadpole_binary()?;
    let c = Arc::new(Cache::open(root, 1 << 30).unwrap());
    let mut s = Settings::new(format!("test (pid {})", std::process::id()));
    s.parallel = parallel;
    s.mem_mb = 4096;
    Some(Runner::new(c, root.to_path_buf(), exe, s))
}

fn problem_for(model: &cadpole_engine::cadpole_core::opt::Model, options: &Options, time_limit: Option<f64>) -> Problem {
    let solver = JobSolver::new(None, None);
    let identity = cadpole_engine::cadpole_core::opt::Solver::identity(&solver).to_owned();
    let digest = cadpole_engine::cadpole_core::opt::digest(model, options, &identity);
    Problem {
        key: cache::key_for(&digest, &platform()),
        digest,
        identity,
        model: model.clone(),
        options: options.clone(),
        time_limit,
        accept_limited: options.time_limit.is_some(),
        mem_mb: 4096,
        origin: "test (jobs_tests.rs)".into(),
        owner: "test".into(),
    }
}

#[test]
fn a_job_answers_exactly_as_an_inline_solve_and_files_its_answer() {
    let root = scratch("runner-answer");
    let Some(r) = runner_in(&root, 2) else {
        eprintln!("skipped: no cadpole binary in the runfiles");
        return;
    };
    let m = market_split(3, 20, 11);
    let o = Options::default();
    let p = problem_for(&m, &o, Some(300.0));
    let started = r.start(&p).unwrap();
    assert!(started.state.is_active(), "{started:?}");
    let (recs, done) = r.wait(std::slice::from_ref(&started.id), Duration::from_secs(120));
    assert!(done, "{recs:?}");
    assert_eq!(recs[0].state, State::Done, "{:?}", recs[0]);
    assert!(recs[0].kept);
    assert_eq!(recs[0].status, "optimal");
    let inline = cadpole_opt::solve(&m, &o).unwrap();
    let filed = r.cache().get(&p.key).expect("the job filed its answer");
    assert_eq!(bits(&filed.solutions), bits(&inline), "same question, same answer, bit for bit");
    assert_eq!(filed.source, codec::Source::Job(started.id.clone()));
    assert!(!r.jobs().problem_path(&started.id).exists(), "the child cleans up its problem");
    // Asking again attaches to the finished record rather than starting
    // another job.
    let again = r.start(&p).unwrap();
    assert_eq!(again.state, State::Done);
    let _ = std::fs::remove_dir_all(&root);
}

#[test]
fn jobs_beyond_the_parallel_limit_wait_their_turn_and_can_be_cancelled() {
    let root = scratch("runner-queue");
    let Some(r) = runner_in(&root, 1) else {
        eprintln!("skipped: no cadpole binary in the runfiles");
        return;
    };
    let hard = problem_for(&market_split(4, 30, 11), &Options::default(), Some(600.0));
    let easy = problem_for(&knapsack(), &Options::default(), Some(60.0));
    let finished = Arc::new(std::sync::Mutex::new(Vec::new()));
    let seen = finished.clone();
    r.on_finished(Arc::new(move |rec: &Record| seen.lock().unwrap().push((rec.id.clone(), rec.state))));
    let a = r.start(&hard).unwrap();
    let b = r.start(&easy).unwrap();
    assert_eq!(r.jobs().read(&a.id).unwrap().state, State::Running);
    assert_eq!(r.jobs().read(&b.id).unwrap().state, State::Queued, "one at a time");
    // Cancelling the running one frees the slot for the queued one.
    let c = r.cancel(&a.id).unwrap();
    assert_eq!(c.state, State::Cancelled);
    let (recs, done) = r.wait(std::slice::from_ref(&b.id), Duration::from_secs(120));
    let ra = r.jobs().read(&a.id).unwrap();
    assert!(
        done && recs[0].state == State::Done,
        "{recs:?}\nthe cancelled one: {ra:?}, alive {}, its job {}",
        sys::alive(ra.pid),
        sys::is_job_process(ra.pid, &ra.id)
    );
    let pid = r.jobs().read(&a.id).unwrap().pid;
    assert!(!sys::is_job_process(pid, &a.id), "the cancelled child is gone");
    assert_eq!(r.jobs().read(&a.id).unwrap().state, State::Cancelled, "and stays cancelled");
    // The listeners hear of both, from the reapers, a moment after the
    // records say so.
    let heard = |want: &(String, State)| {
        let end = std::time::Instant::now() + Duration::from_secs(10);
        while std::time::Instant::now() < end {
            if finished.lock().unwrap().contains(want) {
                return true;
            }
            std::thread::sleep(Duration::from_millis(50));
        }
        false
    };
    assert!(heard(&(a.id.clone(), State::Cancelled)), "{:?}", finished.lock().unwrap());
    assert!(heard(&(b.id.clone(), State::Done)), "{:?}", finished.lock().unwrap());
    let _ = std::fs::remove_dir_all(&root);
}

#[test]
fn a_child_refuses_a_problem_that_is_not_what_it_claims() {
    let root = scratch("runner-refuse");
    let Some(r) = runner_in(&root, 1) else {
        eprintln!("skipped: no cadpole binary in the runfiles");
        return;
    };
    // A digest that is not the model's.
    let mut forged = problem_for(&knapsack(), &Options::default(), Some(60.0));
    forged.digest[0] ^= 1;
    let a = r.start(&forged).unwrap();
    let (recs, _) = r.wait(std::slice::from_ref(&a.id), Duration::from_secs(60));
    assert_eq!(recs[0].state, State::Failed);
    assert!(recs[0].message.contains("does not hash to its key"), "{}", recs[0].message);
    assert!(r.cache().get(&forged.key).is_none(), "nothing filed under a key it does not answer");
    // A parent whose solver is not this binary's.
    let mut foreign = problem_for(&market_split(3, 12, 5), &Options::default(), Some(60.0));
    foreign.identity = "HiGHS 0.1 (an older build)".into();
    let b = r.start(&foreign).unwrap();
    let (recs, _) = r.wait(std::slice::from_ref(&b.id), Duration::from_secs(60));
    assert_eq!(recs[0].state, State::Failed);
    assert!(recs[0].message.contains("restart the process that started it"), "{}", recs[0].message);
    let _ = std::fs::remove_dir_all(&root);
}

#[test]
fn a_limited_answer_is_kept_only_when_the_solve_asked_for_the_limit() {
    let root = scratch("runner-limited");
    let Some(r) = runner_in(&root, 2) else {
        eprintln!("skipped: no cadpole binary in the runfiles");
        return;
    };
    let m = market_split(4, 30, 11);
    // The solve asked for 2 s: a limited answer it accepted.
    let asked = problem_for(&m, &Options { time_limit: Some(2.0), ..Options::default() }, Some(2.0));
    // No limit of its own; the host's 2-s job limit stopped it.
    let host = problem_for(&m, &Options::default(), Some(2.0));
    let ids = [r.start(&asked).unwrap().id, r.start(&host).unwrap().id];
    let (recs, done) = r.wait(&ids, Duration::from_secs(120));
    assert!(done, "{recs:?}");
    assert!(recs.iter().all(|x| x.state == State::Done && x.status == "time_limit"), "{recs:?}");
    let kept = r.cache().get(&asked.key).expect("an accepted limit is kept");
    assert_eq!(kept.limited, Some(2.0));
    assert!(kept.solutions[0].message.contains("kept from job"), "{}", kept.solutions[0].message);
    assert!(r.cache().get(&host.key).is_none(), "a host's limit is never kept");
    assert!(!recs[1].kept && recs[1].message.contains("time_limit="), "{}", recs[1].message);
    let o = r.jobs().read_outcome(&ids[1]).expect("its outcome is still the record's");
    assert!(o.solutions[0].values.is_some(), "with the point it stopped at");
    let _ = std::fs::remove_dir_all(&root);
}

/// A job whose process was killed is in the default list -- it has not
/// ended in any hour, and it is waiting to run again -- not only under
/// `--all`. A cancelled one keeps the time it ran, and leaves no problem
/// file behind.
#[test]
fn a_lost_job_is_listed_and_a_cancelled_one_keeps_its_time() {
    use cadpole_engine::cadpole_proto::v1 as pb;
    let root = scratch("lost-listed");
    let (c, j) = jobs_in(&root);
    let dead = dead_pid();
    let started = now_ms() - 20_000;
    let k = key_for(&[20; 32], &platform());
    let lost = job_id(&k);
    j.write(&c, &Record { pid: dead, owner_pid: dead, started_unix_ms: started, ..record(&lost, &k, State::Running) }).unwrap();
    let listed = super::super::ops::list(&j, &c, &pb::ListJobsRequest::default()).unwrap();
    let shown: Vec<(String, String)> = listed.jobs.iter().map(|x| (x.id.clone(), x.state.clone())).collect();
    assert!(shown.contains(&(lost.clone(), "failed".to_owned())), "{shown:?}");

    // Running for 20 s under this process (its reaper would be elsewhere),
    // then cancelled.
    let k = key_for(&[21; 32], &platform());
    let id = job_id(&k);
    j.write(&c, &Record { pid: dead, started_unix_ms: started, ..record(&id, &k, State::Running) }).unwrap();
    std::fs::write(j.problem_path(&id), b"the problem").unwrap();
    let r = Runner::new(c.clone(), root.clone(), root.join("no-such-binary"), Settings::new("test"));
    let rec = r.cancel(&id).expect("a record");
    assert_eq!(rec.state, State::Cancelled);
    assert!(rec.elapsed_ms >= 20_000.0, "it ran 20 s, not {} ms", rec.elapsed_ms);
    assert_eq!(j.read(&id).unwrap().elapsed_ms, rec.elapsed_ms, "as recorded");
    assert!(!j.problem_path(&id).exists(), "its problem file went with it");
    let _ = std::fs::remove_dir_all(&root);
}
