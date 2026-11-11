//! Solve jobs: one `cadpole opt-job` process per question too long for
//! an evaluation, and the records every `cadpole` process on the machine reads
//! to share them.
//!
//! **The record is the truth.** `jobs/<id>.job` (JSON, written whole by a
//! rename) says where a job is: `queued`, `running`, `done`, `failed` or
//! `cancelled`, with its pid, its owner, its origin and, once it ends, a
//! summary. The parent writes it when it queues and spawns the job, the child
//! when it ends, a canceller when it stops it, and the parent again if the
//! child died without a word. Its answer goes into the solution cache when
//! it may be kept, and every outcome into `jobs/<id>.result` (`codec`).
//!
//! **One job per question.** The id is the cache key's first twelve hex
//! digits, so every process asking the same question names the same job. The
//! decision to start one is taken under `jobs/<id>.claim`, created
//! exclusively and held for as long as it takes to read the record, write
//! the problem and spawn: whoever claims first runs the job, everyone else
//! finds its record and attaches. A claim older than [`STALE_CLAIM`] belongs
//! to a process that crashed in between, and is broken.
//!
//! **Alive means alive.** `running` is believed while the child's pid is
//! alive and (on Linux) its command line names this job; `queued` while the
//! owner's pid is. A record that says otherwise with no result and no live
//! owner to explain it was a job lost with its parent -- a reboot -- and is
//! forgotten, so the next evaluation runs it again.

use std::collections::{HashMap, HashSet, VecDeque};
use std::io::Write as _;
use std::path::{Path, PathBuf};
use std::process::{Child, Command, Stdio};
use std::sync::{Arc, Condvar, Mutex};
use std::time::{Duration, Instant, SystemTime};

use serde::{Deserialize, Serialize};

use super::cache::{self, Cache, hex, is_job_id, job_id};
use super::codec::{self, Outcome, Problem};
use super::sys;

/// A claim this old is a crashed starter's, not a slow one's.
pub const STALE_CLAIM: Duration = Duration::from_secs(30);

/// Finished records, results and logs older than this are collected.
pub const KEEP_FINISHED: Duration = Duration::from_secs(7 * 24 * 3600);

/// How long past its own time limit a child may run before its parent kills
/// it: HiGHS checks its limit between nodes, and a node can take a while.
pub const GRACE: Duration = Duration::from_secs(120);

/// Where a job is.
#[derive(Serialize, Deserialize, Clone, Copy, Debug, PartialEq, Eq, Default)]
#[serde(rename_all = "snake_case")]
pub enum State {
    #[default]
    Queued,
    Running,
    Done,
    Failed,
    Cancelled,
}

impl State {
    pub fn as_str(self) -> &'static str {
        match self {
            State::Queued => "queued",
            State::Running => "running",
            State::Done => "done",
            State::Failed => "failed",
            State::Cancelled => "cancelled",
        }
    }

    pub fn is_active(self) -> bool {
        matches!(self, State::Queued | State::Running)
    }
}

/// One job's record, `jobs/<id>.job`.
#[derive(Serialize, Deserialize, Clone, Debug, Default, PartialEq)]
pub struct Record {
    pub id: String,
    /// The cache key, 64 hex digits.
    pub key: String,
    pub state: State,
    /// The child's pid, once it runs.
    #[serde(default)]
    pub pid: u32,
    /// Which process started it: "cadpole serve (pid 812)".
    pub owner: String,
    pub owner_pid: u32,
    /// The problem's feature id and the line of its `solve()`.
    #[serde(default)]
    pub origin: String,
    pub queued_unix_ms: u64,
    #[serde(default)]
    pub started_unix_ms: u64,
    #[serde(default)]
    pub finished_unix_ms: u64,
    /// The job's time limit, in seconds; `None` for none.
    #[serde(default)]
    pub time_limit_s: Option<f64>,
    /// The solve asked for that limit, so a limited answer may be kept.
    #[serde(default)]
    pub accept_limited: bool,
    #[serde(default)]
    pub mem_mb: u64,
    #[serde(default)]
    pub variables: u64,
    #[serde(default)]
    pub constraints: u64,
    #[serde(default)]
    pub nonzeros: u64,
    /// The solver's status, once done.
    #[serde(default)]
    pub status: String,
    /// Written as text: serde_json does not read every float back to the
    /// bit, and this is the number a user compares with the answer.
    #[serde(default, with = "exact")]
    pub objective: Option<f64>,
    #[serde(default, with = "exact")]
    pub gap: Option<f64>,
    #[serde(default)]
    pub nodes: u64,
    #[serde(default)]
    pub elapsed_ms: f64,
    /// The answer is in the solution cache.
    #[serde(default)]
    pub kept: bool,
    /// How it ended, or what it waits for.
    #[serde(default)]
    pub message: String,
}

/// An `Option<f64>` as the shortest text that reads back to the same bits
/// (Rust's own float formatting and parsing are exact; serde_json's parsing
/// is not, without a feature this build does not enable).
mod exact {
    use serde::{Deserialize, Deserializer, Serializer};

    pub fn serialize<S: Serializer>(v: &Option<f64>, s: S) -> Result<S::Ok, S::Error> {
        match v {
            Some(x) if x.is_finite() => s.serialize_some(&format!("{x:?}")),
            _ => s.serialize_none(),
        }
    }

    pub fn deserialize<'de, D: Deserializer<'de>>(d: D) -> Result<Option<f64>, D::Error> {
        let v: Option<String> = Option::deserialize(d)?;
        Ok(v.and_then(|t| t.parse().ok()))
    }
}

/// What [`Jobs::lookup`] found for a question.
#[derive(Debug, Clone)]
pub enum Found {
    /// No job, or one lost with its parent: decide afresh.
    Nothing,
    /// Queued or running, in this process or another.
    Active(Record),
    /// Ended; its outcome, when it left one.
    Finished(Record, Option<Outcome>),
}

pub fn now_ms() -> u64 {
    SystemTime::now().duration_since(SystemTime::UNIX_EPOCH).map(|d| d.as_millis() as u64).unwrap_or(0)
}

/// The job directory of a cache.
#[derive(Clone)]
pub struct Jobs {
    dir: PathBuf,
}

/// A held claim; dropping it releases it.
pub struct Claim {
    path: PathBuf,
}

impl Drop for Claim {
    fn drop(&mut self) {
        let _ = std::fs::remove_file(&self.path);
    }
}

impl Jobs {
    pub fn of(cache: &Cache) -> Jobs {
        Jobs { dir: cache.jobs_dir() }
    }

    #[cfg_attr(not(test), allow(dead_code))]
    pub fn dir(&self) -> &Path {
        &self.dir
    }

    fn path(&self, id: &str, ext: &str) -> PathBuf {
        debug_assert!(is_job_id(id), "{id:?}");
        self.dir.join(format!("{id}.{ext}"))
    }

    pub fn problem_path(&self, id: &str) -> PathBuf {
        self.path(id, "problem")
    }
    pub fn result_path(&self, id: &str) -> PathBuf {
        self.path(id, "result")
    }
    pub fn log_path(&self, id: &str) -> PathBuf {
        self.path(id, "log")
    }

    pub fn read(&self, id: &str) -> Option<Record> {
        if !is_job_id(id) {
            return None;
        }
        let bytes = std::fs::read(self.path(id, "job")).ok()?;
        serde_json::from_slice(&bytes).ok()
    }

    /// Write `r` whole, by a rename, so no reader sees half a record.
    pub fn write(&self, cache: &Cache, r: &Record) -> std::io::Result<()> {
        let bytes = serde_json::to_vec_pretty(r).map_err(std::io::Error::other)?;
        cache.write_atomic(&self.path(&r.id, "job"), &bytes)
    }

    pub fn read_outcome(&self, id: &str) -> Option<Outcome> {
        let bytes = std::fs::read(self.result_path(id)).ok()?;
        codec::decode_outcome(&bytes).ok()
    }

    /// Claim the right to decide about job `id`, or `None` if another process
    /// holds it. A stale claim is broken once.
    pub fn claim(&self, id: &str) -> Option<Claim> {
        let path = self.path(id, "claim");
        for _ in 0..2 {
            let mut o = std::fs::OpenOptions::new();
            o.write(true).create_new(true);
            #[cfg(unix)]
            {
                use std::os::unix::fs::OpenOptionsExt as _;
                o.mode(0o600);
            }
            match o.open(&path) {
                Ok(mut f) => {
                    let _ = write!(f, "{}", std::process::id());
                    return Some(Claim { path });
                }
                Err(e) if e.kind() == std::io::ErrorKind::AlreadyExists => {
                    let age = std::fs::metadata(&path)
                        .and_then(|m| m.modified())
                        .map(|t| SystemTime::now().duration_since(t).unwrap_or_default())
                        .unwrap_or_default();
                    if age < STALE_CLAIM {
                        return None;
                    }
                    let _ = std::fs::remove_file(&path);
                }
                Err(_) => return None,
            }
        }
        None
    }

    /// What there is for the question `key`.
    pub fn lookup(&self, key: &[u8; 32]) -> Found {
        let id = job_id(key);
        let Some(rec) = self.read(&id) else { return Found::Nothing };
        if rec.key != hex(key) {
            // Two questions sharing an id's 48 bits: never seen, and the
            // newcomer solves inline rather than take the other's files.
            return Found::Nothing;
        }
        self.classify(rec)
    }

    /// Whether a record is believed: see the module comment.
    pub fn classify(&self, rec: Record) -> Found {
        match rec.state {
            State::Queued if sys::alive(rec.owner_pid) => Found::Active(rec),
            State::Running if rec.pid != 0 && sys::is_job_process(rec.pid, &rec.id) => Found::Active(rec),
            // The child is gone without a final record. If it left a
            // result it finished and only the record write was lost.
            State::Queued | State::Running => match self.read_outcome(&rec.id) {
                Some(o) => Found::Finished(rec, Some(o)),
                // Its parent is alive and will record why in a moment.
                None if rec.owner_pid != 0 && rec.owner_pid != std::process::id() && sys::alive(rec.owner_pid) => {
                    Found::Active(rec)
                }
                None if rec.owner_pid == std::process::id() && rec.state == State::Running => Found::Active(rec),
                None => Found::Nothing,
            },
            State::Done | State::Failed | State::Cancelled => {
                let o = self.read_outcome(&rec.id);
                // A job that answered but whose answer is gone (its result
                // deleted, its cache entry collected) answers nothing: as if
                // it never ran, so the question is asked again.
                if rec.state == State::Done && o.is_none() { Found::Nothing } else { Found::Finished(rec, o) }
            }
        }
    }

    /// Every record, newest first.
    pub fn all(&self) -> Vec<Record> {
        let mut out: Vec<Record> = std::fs::read_dir(&self.dir)
            .into_iter()
            .flatten()
            .flatten()
            .filter_map(|e| {
                let name = e.file_name().into_string().ok()?;
                let id = name.strip_suffix(".job")?;
                self.read(id)
            })
            .collect();
        out.sort_by(|a, b| b.queued_unix_ms.cmp(&a.queued_unix_ms).then(a.id.cmp(&b.id)));
        out
    }

    /// A record as a reader should see it: `running` with a dead child and
    /// no live owner is `failed` (lost), whatever the file says.
    pub fn current(&self, rec: Record) -> Record {
        match self.classify(rec.clone()) {
            Found::Nothing if rec.state.is_active() => Record {
                state: State::Failed,
                message: "lost: its process and the one that started it are both gone (a reboot, or a kill); it runs again \
                          the next time the document is evaluated"
                    .into(),
                ..rec
            },
            Found::Finished(r, _) if r.state.is_active() => Record { state: State::Done, ..r },
            _ => rec,
        }
    }

    /// Delete job `id`'s files: the record, the result, the log and any
    /// problem left behind.
    pub fn forget(&self, id: &str) {
        if !is_job_id(id) {
            return;
        }
        for ext in ["job", "result", "log", "problem", "claim"] {
            let _ = std::fs::remove_file(self.path(id, ext));
        }
    }

    /// Delete finished jobs older than [`KEEP_FINISHED`], and problems that
    /// no live job will read.
    pub fn collect(&self) -> usize {
        let cutoff = now_ms().saturating_sub(KEEP_FINISHED.as_millis() as u64);
        let mut n = 0;
        for rec in self.all() {
            let current = self.current(rec);
            let ended = current.finished_unix_ms.max(current.queued_unix_ms);
            if !current.state.is_active() && ended < cutoff {
                self.forget(&current.id);
                n += 1;
            }
        }
        n
    }
}

// ------------------------------------------------------------------ runner

/// How a process runs jobs.
#[derive(Clone, Debug)]
pub struct Settings {
    /// Jobs at once.
    pub parallel: usize,
    /// Jobs waiting behind them; past this a solve is refused.
    pub max_queued: usize,
    /// A job's time limit when its solve gave none; `None` for none.
    pub time_limit: Option<f64>,
    /// A job's address-space limit, in MB; 0 for none.
    pub mem_mb: u64,
    /// The longest an inline attempt may take before the solve becomes a
    /// job, in seconds (also never more than half what the evaluation has).
    pub inline_cap: f64,
    /// Who runs them, for records and messages: "cadpole serve (pid 812)".
    pub owner: String,
    /// What the pending message promises happens when a job lands.
    pub when_landed: String,
    /// Start each job in a process group of its own, so the terminal's
    /// Ctrl-C to its parent does not reach it: `cadpole serve`'s jobs outlive the
    /// server and still file their answers, while `cadpole solve`'s go with it.
    pub own_process_group: bool,
}

impl Settings {
    /// `max(1, min(4, cores / 4))`: a quarter of the machine at most, so a
    /// handful of fits leaves room for the server and the user.
    pub fn default_parallel() -> usize {
        let cores = std::thread::available_parallelism().map(|n| n.get()).unwrap_or(4);
        (cores / 4).clamp(1, 4)
    }

    pub fn new(owner: impl Into<String>) -> Settings {
        Settings {
            parallel: Settings::default_parallel(),
            max_queued: 32,
            time_limit: None,
            mem_mb: 8192,
            inline_cap: 10.0,
            owner: owner.into(),
            when_landed: "evaluate again once it lands".into(),
            own_process_group: false,
        }
    }
}

/// A job waiting for a slot.
struct Waiting {
    id: String,
    time_limit: Option<f64>,
}

#[derive(Default)]
struct RunState {
    running: HashMap<String, u32>,
    queue: VecDeque<Waiting>,
    /// Other processes' jobs this one's evaluations are waiting on.
    watched: HashSet<String>,
    poller: bool,
}

type Listener = Arc<dyn Fn(&Record) + Send + Sync>;

/// Runs jobs for one process, and tells it when they, or the other
/// processes' jobs it is waiting on, end.
pub struct Runner {
    cache_root: PathBuf,
    cache: Arc<Cache>,
    jobs: Jobs,
    exe: PathBuf,
    pub settings: Settings,
    state: Mutex<RunState>,
    /// Bumped and signalled whenever a job of this process, or one it
    /// watches, changes state; separate from `state` so a waiter reads
    /// records from disk without holding the runner.
    generation: Mutex<u64>,
    changed: Condvar,
    on_started: Mutex<Vec<Listener>>,
    on_finished: Mutex<Vec<Listener>>,
}

/// The binary a job runs: this one. On Linux `/proc/self/exe` is the inode
/// this process was started from, so a binary rebuilt underneath a running
/// server cannot change the solver its jobs use.
pub fn this_exe() -> PathBuf {
    #[cfg(target_os = "linux")]
    {
        PathBuf::from("/proc/self/exe")
    }
    #[cfg(not(target_os = "linux"))]
    {
        std::env::current_exe().unwrap_or_else(|_| PathBuf::from("cadpole"))
    }
}

fn exit_reason(status: std::process::ExitStatus) -> String {
    #[cfg(unix)]
    {
        use std::os::unix::process::ExitStatusExt as _;
        if let Some(sig) = status.signal() {
            return match sig {
                9 => "killed (SIGKILL): out of its memory limit, past its wall-clock limit, or stopped by hand".into(),
                24 => "stopped by its CPU-time limit (SIGXCPU)".into(),
                6 => "aborted (SIGABRT): most often out of its memory limit".into(),
                15 => "terminated (SIGTERM)".into(),
                other => format!("killed by signal {other}"),
            };
        }
    }
    match status.code() {
        Some(c) => format!("exited with status {c}"),
        None => "ended without a status".into(),
    }
}

fn log_tail(path: &Path) -> String {
    let text = std::fs::read_to_string(path).unwrap_or_default();
    let lines: Vec<&str> = text.lines().filter(|l| !l.trim().is_empty()).collect();
    lines[lines.len().saturating_sub(3)..].join(" | ")
}

impl Runner {
    pub fn new(cache: Arc<Cache>, cache_root: PathBuf, exe: PathBuf, settings: Settings) -> Arc<Runner> {
        let jobs = Jobs::of(&cache);
        Arc::new(Runner {
            cache_root,
            cache,
            jobs,
            exe,
            settings,
            state: Mutex::new(RunState::default()),
            generation: Mutex::new(0),
            changed: Condvar::new(),
            on_started: Mutex::new(Vec::new()),
            on_finished: Mutex::new(Vec::new()),
        })
    }

    pub fn jobs(&self) -> &Jobs {
        &self.jobs
    }

    pub fn cache(&self) -> &Cache {
        &self.cache
    }

    fn notify(&self) {
        *self.generation.lock().unwrap() += 1;
        self.changed.notify_all();
    }

    pub fn on_started(&self, f: Listener) {
        self.on_started.lock().unwrap().push(f);
    }

    pub fn on_finished(&self, f: Listener) {
        self.on_finished.lock().unwrap().push(f);
    }

    fn tell(&self, listeners: &Mutex<Vec<Listener>>, rec: &Record) {
        let ls: Vec<Listener> = listeners.lock().unwrap().clone();
        for f in ls {
            f(rec);
        }
    }

    /// Start a job for `problem` -- or, if another process got there first,
    /// attach to its -- and return the job's record as it now stands.
    pub fn start(self: &Arc<Self>, problem: &Problem) -> Result<Record, String> {
        let id = job_id(&problem.key);
        let Some(claim) = self.jobs.claim(&id) else {
            // Someone is deciding about this very job right now; its record
            // appears in a moment, and the poller finds it.
            self.watch(&id);
            return Ok(self.jobs.read(&id).unwrap_or(Record { id, state: State::Queued, ..Default::default() }));
        };
        match self.jobs.lookup(&problem.key) {
            Found::Active(rec) => {
                drop(claim);
                if rec.owner_pid != std::process::id() {
                    self.watch(&rec.id);
                }
                return Ok(rec);
            }
            Found::Finished(rec, _) => {
                // It ended between the caller's look and this one; the next
                // evaluation reads its answer.
                drop(claim);
                return Ok(rec);
            }
            Found::Nothing => {}
        }
        {
            let st = self.state.lock().unwrap();
            if st.queue.len() >= self.settings.max_queued {
                return Err(format!(
                    "{} solve jobs are already waiting in {}; cancel some (`cadpole jobs cancel`) or wait",
                    st.queue.len(),
                    self.settings.owner
                ));
            }
        }
        if let Some(other) = self.jobs.read(&id)
            && other.key != hex(&problem.key)
        {
            return Err(format!("job id {id} is another question's (its first 48 bits match); this one solves inline"));
        }
        // A fresh job: whatever an earlier one left goes first.
        self.jobs.forget_files_but_claim(&id);
        let bytes = codec::encode_problem(problem);
        self.cache.write_atomic(&self.jobs.problem_path(&id), &bytes).map_err(|e| format!("writing job {id}'s problem: {e}"))?;
        let (vars, rows, nz) = (problem.model.columns.len(), problem.model.rows.len(), problem.model.index.len());
        let rec = Record {
            id: id.clone(),
            key: hex(&problem.key),
            state: State::Queued,
            pid: 0,
            owner: self.settings.owner.clone(),
            owner_pid: std::process::id(),
            origin: problem.origin.clone(),
            queued_unix_ms: now_ms(),
            time_limit_s: problem.time_limit,
            accept_limited: problem.accept_limited,
            mem_mb: problem.mem_mb,
            variables: vars as u64,
            constraints: rows as u64,
            nonzeros: nz as u64,
            ..Default::default()
        };
        self.jobs.write(&self.cache, &rec).map_err(|e| format!("writing job {id}'s record: {e}"))?;
        self.state.lock().unwrap().queue.push_back(Waiting { id: id.clone(), time_limit: problem.time_limit });
        drop(claim);
        self.dispatch();
        Ok(self.jobs.read(&id).unwrap_or(rec))
    }

    /// Start queued jobs while there are free slots.
    fn dispatch(self: &Arc<Self>) {
        loop {
            let next = {
                let mut st = self.state.lock().unwrap();
                if st.running.len() >= self.settings.parallel.max(1) {
                    return;
                }
                match st.queue.pop_front() {
                    Some(w) => w,
                    None => return,
                }
            };
            // Cancelled while it waited (by another process, which could
            // not reach this queue): it never starts.
            if self.jobs.read(&next.id).is_some_and(|r| r.state != State::Queued) {
                self.notify();
                continue;
            }
            match self.spawn(&next) {
                Ok((child, rec)) => {
                    self.state.lock().unwrap().running.insert(next.id.clone(), child.id());
                    self.tell(&self.on_started, &rec);
                    let me = self.clone();
                    let limit = next.time_limit;
                    std::thread::Builder::new()
                        .name(format!("opt-job {}", next.id))
                        .spawn(move || me.reap(child, rec, limit))
                        .expect("a thread for the job's waiter");
                }
                Err(e) => {
                    let mut rec = self.jobs.read(&next.id).unwrap_or_default();
                    rec.state = State::Failed;
                    rec.finished_unix_ms = now_ms();
                    rec.message = format!("could not start: {e}");
                    let _ = self.jobs.write(&self.cache, &rec);
                    self.notify();
                    self.tell(&self.on_finished, &rec);
                }
            }
        }
    }

    fn spawn(&self, w: &Waiting) -> std::io::Result<(Child, Record)> {
        let log = std::fs::File::create(self.jobs.log_path(&w.id))?;
        let mut cmd = Command::new(&self.exe);
        cmd.arg("opt-job").arg(&self.cache_root).arg(&w.id).stdin(Stdio::null()).stdout(log.try_clone()?).stderr(log);
        #[cfg(unix)]
        if self.settings.own_process_group {
            use std::os::unix::process::CommandExt as _;
            cmd.process_group(0);
        }
        let child = cmd.spawn()?;
        let mut rec = self.jobs.read(&w.id).unwrap_or_default();
        rec.state = State::Running;
        rec.pid = child.id();
        rec.started_unix_ms = now_ms();
        self.jobs.write(&self.cache, &rec)?;
        Ok((child, rec))
    }

    /// Wait for a child, kill it if it outlives its limit by [`GRACE`], and
    /// record how it ended if it could not say so itself.
    fn reap(self: Arc<Self>, mut child: Child, rec: Record, limit: Option<f64>) {
        let started = Instant::now();
        let deadline = limit.map(|t| Duration::from_secs_f64(t.max(0.0)) + GRACE);
        let mut killed = false;
        let status = loop {
            match child.try_wait() {
                Ok(Some(s)) => break Some(s),
                Ok(None) => {}
                Err(_) => break None,
            }
            if let Some(d) = deadline
                && !killed
                && started.elapsed() > d
            {
                sys::kill_now(child.id());
                killed = true;
            }
            std::thread::sleep(Duration::from_millis(200));
        };
        // A child that finished deleted its problem; one that did not will
        // never read it again (a rerun writes it afresh).
        let _ = std::fs::remove_file(self.jobs.problem_path(&rec.id));
        let mut now = self.jobs.read(&rec.id).unwrap_or(rec.clone());
        if now.state.is_active() {
            // The child ended without writing its final record.
            now.state = State::Failed;
            now.finished_unix_ms = now_ms();
            now.elapsed_ms = ran_ms(&now, now.finished_unix_ms);
            let why = status.map(exit_reason).unwrap_or_else(|| "its exit could not be read".into());
            let why = if killed { format!("killed {} s past its time limit", GRACE.as_secs()) } else { why };
            let tail = log_tail(&self.jobs.log_path(&rec.id));
            now.message = if tail.is_empty() { why } else { format!("{why}; its log ends: {tail}") };
            let _ = self.jobs.write(&self.cache, &now);
        }
        self.state.lock().unwrap().running.remove(&rec.id);
        self.notify();
        self.tell(&self.on_finished, &now);
        self.dispatch();
    }

    /// Follow another process's job: when it ends, this runner's listeners
    /// hear of it as of their own.
    pub fn watch(self: &Arc<Self>, id: &str) {
        let start = {
            let mut st = self.state.lock().unwrap();
            if st.running.contains_key(id) || st.queue.iter().any(|w| w.id == id) {
                return;
            }
            st.watched.insert(id.to_owned());
            !std::mem::replace(&mut st.poller, true)
        };
        if start {
            let me = self.clone();
            std::thread::Builder::new().name("opt-job watcher".into()).spawn(move || me.poll()).expect("a thread for the watcher");
        }
    }

    fn poll(self: Arc<Self>) {
        loop {
            std::thread::sleep(Duration::from_secs(1));
            let ids: Vec<String> = self.state.lock().unwrap().watched.iter().cloned().collect();
            for id in ids {
                let ended = match self.jobs.read(&id) {
                    None => Some(None),
                    Some(rec) => match self.jobs.classify(rec) {
                        Found::Active(_) => None,
                        Found::Finished(r, _) => Some(Some(r)),
                        Found::Nothing => Some(None),
                    },
                };
                if let Some(rec) = ended {
                    self.state.lock().unwrap().watched.remove(&id);
                    self.notify();
                    let rec = rec.unwrap_or(Record { id: id.clone(), state: State::Failed, message: "lost".into(), ..Default::default() });
                    self.tell(&self.on_finished, &rec);
                }
            }
            let mut st = self.state.lock().unwrap();
            if st.watched.is_empty() {
                st.poller = false;
                return;
            }
        }
    }

    /// Wait until none of `ids` is queued or running, or `timeout` passes;
    /// then their records. Other processes' jobs are read from disk, so this
    /// wakes at least twice a second.
    pub fn wait(&self, ids: &[String], timeout: Duration) -> (Vec<Record>, bool) {
        let end = Instant::now() + timeout;
        loop {
            let seen = *self.generation.lock().unwrap();
            let records: Vec<Record> = ids.iter().filter_map(|id| self.jobs.read(id).map(|r| self.jobs.current(r))).collect();
            let done = records.iter().all(|r| !r.state.is_active());
            let now = Instant::now();
            if done || now >= end {
                return (records, done);
            }
            let g = self.generation.lock().unwrap();
            if *g == seen {
                let _ = self.changed.wait_timeout(g, (end - now).min(Duration::from_millis(500))).unwrap();
            }
        }
    }

    /// Stop jobs: this process's queued ones are dropped, running ones (any
    /// process's) get SIGTERM, then SIGKILL; each is recorded `cancelled`.
    pub fn cancel(&self, id: &str) -> Option<Record> {
        let rec = self.jobs.read(id)?;
        let rec = self.jobs.current(rec);
        if !rec.state.is_active() {
            return Some(rec);
        }
        let was_queued_here = {
            let mut st = self.state.lock().unwrap();
            let before = st.queue.len();
            st.queue.retain(|w| w.id != id);
            st.queue.len() != before
        };
        if was_queued_here {
            let _ = std::fs::remove_file(self.jobs.problem_path(id));
        }
        let now = now_ms();
        let cancelled = Record {
            state: State::Cancelled,
            finished_unix_ms: now,
            // The child records its time only when it finishes itself, so
            // a cancel records how long it ran.
            elapsed_ms: ran_ms(&rec, now),
            message: "cancelled".into(),
            ..rec.clone()
        };
        let _ = self.jobs.write(&self.cache, &cancelled);
        // This process's own child is signalled by the pid it spawned: until
        // its reaper has waited for it, that pid cannot belong to anyone
        // else. Another process's job only after its command line says it
        // is that job (`sys::is_job_process`).
        let own = self.state.lock().unwrap().running.get(id).copied();
        if !was_queued_here && rec.state == State::Running {
            match own {
                // Ours until its reaper has waited for it.
                Some(pid) => {
                    let _ = sys::terminate(pid, || self.state.lock().unwrap().running.contains_key(id));
                }
                None if sys::is_job_process(rec.pid, id) => {
                    let _ = sys::terminate(rec.pid, || sys::is_job_process(rec.pid, id));
                }
                None => {}
            }
        }
        // A running job's problem file is never read again (a rerun writes
        // it afresh); another process's job has no reaper here to remove it.
        let _ = std::fs::remove_file(self.jobs.problem_path(id));
        self.notify();
        if was_queued_here {
            self.tell(&self.on_finished, &cancelled);
        }
        Some(cancelled)
    }
}

/// How long a job ran until `end`, from its start (0 if it never
/// started): what a record ended by someone other than its own child says
/// it took.
fn ran_ms(rec: &Record, end: u64) -> f64 {
    if rec.started_unix_ms == 0 {
        return 0.0;
    }
    end.saturating_sub(rec.started_unix_ms) as f64
}

impl Jobs {
    /// Delete what an earlier job of this id left, except the claim the
    /// caller holds.
    fn forget_files_but_claim(&self, id: &str) {
        for ext in ["job", "result", "log", "problem"] {
            let _ = std::fs::remove_file(self.path(id, ext));
        }
    }
}

/// Where a root's cache and jobs are, for a child given only the root.
pub fn open_root(root: &Path) -> std::io::Result<Cache> {
    Cache::open(root, cache::cap_from(|n| std::env::var(n).ok()))
}

#[cfg(test)]
#[path = "jobs_tests.rs"]
mod tests;
