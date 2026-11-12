//! How local clients find a running `cadpole serve`, and how they reach it
//! without a port.
//!
//! Every server keeps its per-run state in the runtime directory
//! ([`security::runtime_dir`]): a private, 0700 directory the user owns. Each
//! run adds, all 0600 and all named after a random instance id:
//!
//! - `<instance>.sock`: the agent socket, the surface `cadpole mcp`,
//!   `cadpole listen`, `cadpole op` and the listener use;
//! - `<instance>.json`: the [`Record`] that says what is there (no secrets);
//! - `<instance>.session-token`, `<instance>.listener-token`: this run's agent
//!   tokens, so that two servers at once do not overwrite each other's;
//! - `<instance>.url`: the browser link with its token, when the server's
//!   output is not a terminal (a token is never printed to a log).
//!
//! A client with no `--server` reads the records, keeps the ones whose files
//! are the user's own and private and whose server answers a connect, and
//! picks one by [`select`]. A client that has picked a document keeps it: when
//! its server goes away it reconnects to a new server *of the same document*,
//! or fails and says so, and never moves to another document on its own.
//!
//! `spec/transport.qnt` models all of this; `spec/replay/transport_replay.mjs`
//! drives the real programs through the model's traces.

use std::io::{Read as _, Write as _};
use std::path::{Path, PathBuf};
use std::time::Duration;

use serde::{Deserialize, Serialize};

use crate::security;

/// What a running server says about itself. Read by clients, so it carries
/// nothing that grants anything: no token, only where things are.
#[derive(Serialize, Deserialize, Clone, Debug, PartialEq, Eq)]
pub struct Record {
    /// Random per run; every file of the run is named after it.
    pub instance: String,
    pub pid: u32,
    /// The agent socket, when the server has one.
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub socket: Option<PathBuf>,
    /// The browser surface, when the server listens on TCP.
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub http: Option<String>,
    /// Whether the TCP surface also accepts the agent tokens (`--agent-tcp`).
    #[serde(default)]
    pub agent_tcp: bool,
    /// The document file it serves, absolute; none for an untitled document.
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub document: Option<PathBuf>,
    pub started: String,
}

/// Where a client sends its requests.
#[derive(Clone, Debug, PartialEq, Eq)]
pub enum Target {
    /// HTTP over a Unix domain socket.
    Unix(PathBuf),
    /// HTTP over TCP, `http://host:port` without a trailing slash.
    Http(String),
}

impl std::fmt::Display for Target {
    fn fmt(&self, f: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
        match self {
            Target::Unix(p) => write!(f, "unix:{}", p.display()),
            Target::Http(u) => f.write_str(u),
        }
    }
}

/// `--server`: `unix:/path/to.sock`, or `http://host:port`.
pub fn parse_server(s: &str) -> anyhow::Result<Target> {
    if let Some(p) = s.strip_prefix("unix:") {
        if p.is_empty() {
            anyhow::bail!("--server unix: needs a path, e.g. unix:/run/user/1000/cadpole/<instance>.sock");
        }
        return Ok(Target::Unix(PathBuf::from(p)));
    }
    if s.starts_with("http://") || s.starts_with("https://") {
        return Ok(Target::Http(s.trim_end_matches('/').to_owned()));
    }
    anyhow::bail!("--server takes unix:/path/to.sock or http://host:port, not {s:?}")
}

// ---------------------------------------------------------------------------
// Owner and mode checks
// ---------------------------------------------------------------------------

#[cfg(unix)]
mod sys {
    unsafe extern "C" {
        fn geteuid() -> u32;
    }
    /// The effective user id. `uid_t` is 32 bits on Linux and macOS.
    pub fn euid() -> u32 {
        // SAFETY: no arguments, cannot fail.
        unsafe { geteuid() }
    }
}

/// The effective user id, where there is one.
#[cfg(unix)]
pub fn euid() -> Option<u32> {
    Some(sys::euid())
}

#[cfg(not(unix))]
pub fn euid() -> Option<u32> {
    None
}

/// `dir` is a real directory (not a link to one), owned by this user, that
/// nobody else can enter: what a socket and a token file need around them.
pub fn check_private_dir(dir: &Path) -> anyhow::Result<()> {
    let meta = std::fs::symlink_metadata(dir).map_err(|e| anyhow::anyhow!("{}: {e}", dir.display()))?;
    if !meta.is_dir() {
        anyhow::bail!("{} is not a directory (a symlink is not accepted)", dir.display());
    }
    #[cfg(unix)]
    {
        use std::os::unix::fs::MetadataExt as _;
        if Some(meta.uid()) != euid() {
            anyhow::bail!("{} belongs to another user (uid {}); refusing to use it", dir.display(), meta.uid());
        }
        if meta.mode() & 0o077 != 0 {
            anyhow::bail!("{} is mode {:o}; it must be 0700 (only you may enter it)", dir.display(), meta.mode() & 0o777);
        }
    }
    Ok(())
}

/// Create `dir` 0700 if it is missing, tighten it if it is this user's, then
/// [`check_private_dir`]. A directory someone else owns is never changed,
/// only refused.
pub fn prepare_private_dir(dir: &Path) -> anyhow::Result<()> {
    std::fs::create_dir_all(dir).map_err(|e| anyhow::anyhow!("cannot create {}: {e}", dir.display()))?;
    #[cfg(unix)]
    {
        use std::os::unix::fs::{MetadataExt as _, PermissionsExt as _};
        if let Ok(meta) = std::fs::symlink_metadata(dir) {
            if meta.is_dir() && Some(meta.uid()) == euid() && meta.mode() & 0o077 != 0 {
                let _ = std::fs::set_permissions(dir, std::fs::Permissions::from_mode(0o700));
            }
        }
    }
    check_private_dir(dir)
}

/// `path` is this user's, not a link, and nobody else may read or write it.
/// `socket` says which kind of file it must be.
pub fn check_private_file(path: &Path, socket: bool) -> anyhow::Result<()> {
    let meta = std::fs::symlink_metadata(path).map_err(|e| anyhow::anyhow!("{}: {e}", path.display()))?;
    #[cfg(unix)]
    {
        use std::os::unix::fs::{FileTypeExt as _, MetadataExt as _};
        let kind_ok = if socket { meta.file_type().is_socket() } else { meta.is_file() };
        if !kind_ok {
            anyhow::bail!("{} is not a {}", path.display(), if socket { "socket" } else { "regular file" });
        }
        if Some(meta.uid()) != euid() {
            anyhow::bail!("{} belongs to another user (uid {}); refusing to use it", path.display(), meta.uid());
        }
        if meta.mode() & 0o077 != 0 {
            anyhow::bail!("{} is mode {:o}; it must be 0600", path.display(), meta.mode() & 0o777);
        }
    }
    #[cfg(not(unix))]
    {
        let _ = socket;
        if !meta.is_file() {
            anyhow::bail!("{} is not a regular file", path.display());
        }
    }
    Ok(())
}

// ---------------------------------------------------------------------------
// The files of one run
// ---------------------------------------------------------------------------

/// A new instance id: 12 hex digits, so the files of two runs never share a
/// name and a new server never has to decide whether an old socket is live
/// before it can bind its own.
pub fn new_instance() -> String {
    security::generate_token().bytes().take(12).map(|b| format!("{:x}", b % 16)).collect()
}

/// True for a name [`new_instance`] could have made.
fn valid_instance(s: &str) -> bool {
    s.len() == 12 && s.bytes().all(|b| b.is_ascii_hexdigit() && !b.is_ascii_uppercase())
}

/// The paths of one run's files.
#[derive(Clone, Debug)]
pub struct RunFiles {
    pub dir: PathBuf,
    pub instance: String,
}

impl RunFiles {
    pub fn new(dir: &Path, instance: &str) -> RunFiles {
        RunFiles { dir: dir.to_owned(), instance: instance.to_owned() }
    }
    pub fn socket(&self) -> PathBuf {
        self.dir.join(format!("{}.sock", self.instance))
    }
    pub fn record(&self) -> PathBuf {
        self.dir.join(format!("{}.json", self.instance))
    }
    pub fn session_token(&self) -> PathBuf {
        self.dir.join(format!("{}.session-token", self.instance))
    }
    pub fn listener_token(&self) -> PathBuf {
        self.dir.join(format!("{}.listener-token", self.instance))
    }
    pub fn url(&self) -> PathBuf {
        self.dir.join(format!("{}.url", self.instance))
    }
    /// Every file this run may leave behind, socket last.
    fn all(&self) -> [PathBuf; 5] {
        [self.record(), self.session_token(), self.listener_token(), self.url(), self.socket()]
    }
}

/// The longest socket path this code binds: `sun_path` is 108 bytes on Linux
/// and 104 on macOS, the terminator included.
pub const MAX_SOCKET_PATH: usize = 100;

/// Where this run's socket goes: `<instance>.sock` in the runtime directory,
/// or, when that path is too long for a socket (a deep `$TMPDIR`, a test
/// sandbox), in `/tmp/cadpole-<uid>`, a private directory of this user's that
/// the record then names. Either directory must pass [`check_private_dir`].
pub fn default_socket_path(files: &RunFiles) -> anyhow::Result<PathBuf> {
    let p = files.socket();
    if p.as_os_str().len() <= MAX_SOCKET_PATH {
        return Ok(p);
    }
    let uid = euid().map(|u| u.to_string()).unwrap_or_else(|| "user".to_owned());
    let dir = PathBuf::from("/tmp").join(format!("cadpole-{uid}"));
    prepare_private_dir(&dir)?;
    Ok(dir.join(format!("{}.sock", files.instance)))
}

/// Write `bytes` to `path` 0600, all at once: a reader sees the whole record
/// or none. The temporary file is created new, so it cannot be a planted link.
pub fn write_private_atomic(path: &Path, bytes: &[u8]) -> std::io::Result<()> {
    let dir = path.parent().unwrap_or(Path::new("."));
    let name = path.file_name().and_then(|n| n.to_str()).unwrap_or("record");
    let tmp = dir.join(format!(".{name}.{}.tmp", std::process::id()));
    let mut opts = std::fs::OpenOptions::new();
    opts.write(true).create_new(true);
    #[cfg(unix)]
    {
        use std::os::unix::fs::OpenOptionsExt as _;
        opts.mode(0o600);
    }
    let _ = std::fs::remove_file(&tmp);
    let result = (|| {
        let mut f = opts.open(&tmp)?;
        f.write_all(bytes)?;
        f.sync_all()?;
        std::fs::rename(&tmp, path)
    })();
    if result.is_err() {
        let _ = std::fs::remove_file(&tmp);
    }
    result
}

/// Does something accept connections on this socket right now?
///
/// The one liveness test: a socket file outlives a server that crashed, and a
/// pid outlives nothing reliably (it is reused). A connect is answered by the
/// kernel as soon as the server has bound, before it accepts anything.
#[cfg(unix)]
pub fn socket_is_live(path: &Path) -> bool {
    std::os::unix::net::UnixStream::connect(path).is_ok()
}

/// For tests: make `path` the socket file a crashed server leaves behind,
/// one nothing listens on. Dropping the listener is not enough in a test
/// process: a child another test is spawning at that moment holds a copy
/// of every descriptor until it execs, close-on-exec or not, so the
/// "crashed" socket can still accept a connection. The listening socket is
/// unlinked instead, and a datagram socket bound in its place, which no
/// stream connection ever reaches, whoever holds a copy of it.
#[cfg(all(test, unix))]
pub(crate) fn leave_dead_socket(path: &Path) {
    use std::os::unix::fs::PermissionsExt as _;
    std::fs::remove_file(path).unwrap();
    drop(std::os::unix::net::UnixDatagram::bind(path).unwrap());
    std::fs::set_permissions(path, std::fs::Permissions::from_mode(0o600)).unwrap();
}

#[cfg(not(unix))]
pub fn socket_is_live(_path: &Path) -> bool {
    false
}

/// Does a server answer on this `http://host:port`?
fn http_is_live(url: &str) -> bool {
    let authority = url.trim_start_matches("http://").trim_start_matches("https://");
    let authority = authority.split('/').next().unwrap_or("");
    use std::net::ToSocketAddrs as _;
    let Ok(mut addrs) = authority.to_socket_addrs() else { return false };
    addrs.next().is_some_and(|a| std::net::TcpStream::connect_timeout(&a, Duration::from_millis(500)).is_ok())
}

/// Is the server this record describes alive? By its socket when it has one;
/// otherwise its pid must be alive and its HTTP port must answer.
pub fn record_is_live(r: &Record) -> bool {
    match (&r.socket, &r.http) {
        (Some(s), _) => socket_is_live(s),
        (None, Some(h)) => crate::opt_jobs::sys::alive(r.pid) && http_is_live(h),
        (None, None) => false,
    }
}

/// Read a record, if it is a file this user owns and nobody else can write.
pub fn read_record(path: &Path) -> anyhow::Result<Record> {
    check_private_file(path, false)?;
    let bytes = std::fs::read(path)?;
    if bytes.len() > 64 * 1024 {
        anyhow::bail!("{} is too large to be a record", path.display());
    }
    let r: Record = serde_json::from_slice(&bytes).map_err(|e| anyhow::anyhow!("{}: {e}", path.display()))?;
    if !valid_instance(&r.instance) || path.file_stem().and_then(|s| s.to_str()) != Some(r.instance.as_str()) {
        anyhow::bail!("{} does not name its own instance", path.display());
    }
    Ok(r)
}

/// Every record in `dir`, with the reason for each one that cannot be used.
pub fn records(dir: &Path) -> (Vec<Record>, Vec<String>) {
    let (mut ok, mut skipped) = (Vec::new(), Vec::new());
    let Ok(entries) = std::fs::read_dir(dir) else { return (ok, skipped) };
    let mut paths: Vec<PathBuf> = entries
        .flatten()
        .map(|e| e.path())
        .filter(|p| p.extension().and_then(|x| x.to_str()) == Some("json") && p.file_stem().and_then(|s| s.to_str()).is_some_and(valid_instance))
        .collect();
    paths.sort();
    for p in paths {
        match read_record(&p) {
            Ok(r) => ok.push(r),
            Err(e) => skipped.push(format!("{e}")),
        }
    }
    (ok, skipped)
}

/// Remove what crashed servers left in `dir`: a record whose server does not
/// answer, the files named after it, and a socket nobody is listening on.
///
/// Nothing that answers a connect is ever removed, and nothing that is not
/// this user's own private file with an instance name: cleanup is the one
/// place this code deletes, so it deletes only what it can prove is dead.
/// Returns what it removed.
pub fn sweep_stale(dir: &Path) -> Vec<PathBuf> {
    let mut removed = Vec::new();
    let Ok(entries) = std::fs::read_dir(dir) else { return removed };
    let mut stems: Vec<String> = entries
        .flatten()
        .filter_map(|e| {
            let p = e.path();
            let ext = p.extension().and_then(|x| x.to_str())?.to_owned();
            let stem = p.file_stem().and_then(|s| s.to_str())?.to_owned();
            (valid_instance(&stem) && (ext == "json" || ext == "sock")).then_some(stem)
        })
        .collect();
    stems.sort();
    stems.dedup();
    for stem in stems {
        let files = RunFiles::new(dir, &stem);
        let record = read_record(&files.record()).ok();
        let sock = files.socket();
        let sock_present = std::fs::symlink_metadata(&sock).is_ok();
        let live = match &record {
            Some(r) => record_is_live(r) || (sock_present && socket_is_live(&sock)),
            None => sock_present && socket_is_live(&sock),
        };
        if live {
            continue;
        }
        // A record that is not ours (wrong owner or mode) is left alone; so
        // is a socket that is not our own private socket.
        if record.is_none() && std::fs::symlink_metadata(files.record()).is_ok() {
            continue;
        }
        if sock_present && check_private_file(&sock, true).is_err() {
            continue;
        }
        let mut paths = files.all().to_vec();
        // A socket kept elsewhere because the runtime directory's path was
        // too long for one: the record names it.
        if let Some(elsewhere) = record.as_ref().and_then(|r| r.socket.clone()).filter(|p| *p != sock) {
            if !socket_is_live(&elsewhere) && check_private_file(&elsewhere, true).is_ok() {
                paths.push(elsewhere);
            }
        }
        for p in paths {
            let is_socket = p.extension().and_then(|x| x.to_str()) == Some("sock");
            if std::fs::symlink_metadata(&p).is_ok() && check_private_file(&p, is_socket).is_ok() && std::fs::remove_file(&p).is_ok() {
                removed.push(p);
            }
        }
    }
    removed
}

// ---------------------------------------------------------------------------
// Discovery
// ---------------------------------------------------------------------------

/// A live server a client may use, and how to reach it.
#[derive(Clone, Debug)]
pub struct Found {
    pub record: Record,
    pub target: Target,
}

/// The live servers in `dir` that a client can reach as an agent, and why
/// each other record was passed over.
pub fn live_servers(dir: &Path) -> (Vec<Found>, Vec<String>) {
    let (records, mut skipped) = records(dir);
    let mut found = Vec::new();
    for r in records {
        let target = match (&r.socket, &r.http) {
            (Some(s), _) => {
                let dir_ok = s.parent().map(check_private_dir).unwrap_or_else(|| Err(anyhow::anyhow!("no directory")));
                if let Err(e) = dir_ok.and_then(|()| check_private_file(s, true)) {
                    skipped.push(format!("instance {}: {e}", r.instance));
                    continue;
                }
                Target::Unix(s.clone())
            }
            (None, Some(h)) if r.agent_tcp => Target::Http(h.clone()),
            (None, Some(_)) => {
                skipped.push(format!("instance {} has no socket and does not accept agents over TCP", r.instance));
                continue;
            }
            (None, None) => continue,
        };
        if !record_is_live(&r) {
            skipped.push(format!("instance {} (pid {}) is not running", r.instance, r.pid));
            continue;
        }
        found.push(Found { record: r, target });
    }
    (found, skipped)
}

/// Which of several live servers a client with no `--server` uses.
///
/// - `pinned`: once a client has used a server, only servers of the same
///   document are candidates, so a restart reconnects it and nothing moves it
///   to another document.
/// - one candidate: that one;
/// - several: the one whose document is in `cwd` or below it, if exactly one
///   is;
/// - otherwise: an error naming them, asking for `--server`.
///
/// Deterministic: the answer depends on the set of candidates and `cwd`, not
/// on the order the directory lists them in.
pub fn select(mut candidates: Vec<Found>, cwd: Option<&Path>, pinned: Option<&Option<PathBuf>>, dir: &Path) -> anyhow::Result<Found> {
    if let Some(doc) = pinned {
        candidates.retain(|f| &f.record.document == doc);
        if candidates.is_empty() {
            anyhow::bail!(
                "the `cadpole serve` of {} is no longer running; start it again (this client will reconnect), or pass --server",
                describe_document(doc.as_deref())
            );
        }
    }
    candidates.sort_by(|a, b| a.record.instance.cmp(&b.record.instance));
    if candidates.is_empty() {
        anyhow::bail!(
            "no running `cadpole serve` found in {}: start one with `cadpole serve FILE`, or pass --server unix:PATH or --server http://HOST:PORT",
            dir.display()
        );
    }
    if candidates.len() == 1 {
        return Ok(candidates.remove(0));
    }
    if let Some(cwd) = cwd {
        let here: Vec<&Found> = candidates.iter().filter(|f| f.record.document.as_deref().is_some_and(|d| d.starts_with(cwd))).collect();
        if here.len() == 1 {
            return Ok(here[0].clone());
        }
    }
    let list: Vec<String> = candidates
        .iter()
        .map(|f| format!("  --server {}   ({}, pid {})", f.target, describe_document(f.record.document.as_deref()), f.record.pid))
        .collect();
    anyhow::bail!(
        "{} `cadpole serve` instances are running and none is the only one serving a document in this directory; choose one:\n{}",
        candidates.len(),
        list.join("\n")
    )
}

fn describe_document(d: Option<&Path>) -> String {
    match d {
        Some(p) => p.display().to_string(),
        None => "an untitled document".to_owned(),
    }
}

/// [`live_servers`] then [`select`], in the runtime directory.
pub fn discover(pinned: Option<&Option<PathBuf>>) -> anyhow::Result<Found> {
    let dir = security::runtime_dir().ok_or_else(|| anyhow::anyhow!("no runtime directory: set XDG_RUNTIME_DIR or HOME, or pass --server"))?;
    discover_in(&dir, std::env::current_dir().ok().as_deref(), pinned)
}

pub fn discover_in(dir: &Path, cwd: Option<&Path>, pinned: Option<&Option<PathBuf>>) -> anyhow::Result<Found> {
    if std::fs::symlink_metadata(dir).is_err() {
        return select(Vec::new(), cwd, pinned, dir);
    }
    check_private_dir(dir)?;
    let (found, _skipped) = live_servers(dir);
    select(found, cwd, pinned, dir)
}

// ---------------------------------------------------------------------------
// Endpoint: target and token, resolved and re-resolved
// ---------------------------------------------------------------------------

/// Which token a client presents.
#[derive(Clone, Debug)]
pub enum TokenSource {
    /// `--token` / `CADPOLE_TOKEN`: used as given, never re-read.
    Explicit(String),
    /// The owner's agent token: `--token-file`, else the run's own file, else
    /// the newest run's.
    Owner { file: Option<PathBuf> },
    /// The restricted token. Never falls back to the owner's.
    Listener { file: Option<PathBuf> },
}

/// Where a client's requests go and what it presents, resolved lazily and
/// again after the server it was using goes away.
#[derive(Debug)]
pub struct Endpoint {
    fixed: Option<Target>,
    tokens: TokenSource,
    /// Stay with the first run found rather than follow its document to a
    /// new one: the limited listener, which a restart of the server stops
    /// until it is started again (docs/assistant-integration.md, "Restarts").
    stay_with_the_run: bool,
    state: std::sync::Mutex<EndpointState>,
}

#[derive(Debug, Default)]
struct EndpointState {
    current: Option<Resolved>,
    /// The document of the first server this client used, when it found it
    /// by discovery.
    pinned: Option<Option<PathBuf>>,
    /// That server's instance.
    instance: Option<String>,
    /// Where the last resolution led, and how many different places it has
    /// led to: a reader that keeps a cursor compares this to know that it
    /// is reading another run's log now.
    last: Option<Target>,
    generation: u64,
}

/// One resolution: where to send, and the token to send with it.
#[derive(Clone, Debug)]
pub struct Resolved {
    pub target: Target,
    pub token: Option<String>,
}

impl Endpoint {
    /// `server`: the `--server` value, or `None` to discover.
    pub fn new(server: Option<&str>, tokens: TokenSource) -> anyhow::Result<Endpoint> {
        let fixed = server.filter(|s| !s.is_empty()).map(parse_server).transpose()?;
        Ok(Endpoint { fixed, tokens, stay_with_the_run: false, state: Default::default() })
    }

    /// The same, staying with the first run it finds (the limited listener).
    pub fn staying(mut self) -> Endpoint {
        self.stay_with_the_run = true;
        self
    }

    /// An endpoint for a known target.
    #[cfg(test)]
    pub fn fixed(target: Target, tokens: TokenSource) -> Endpoint {
        Endpoint { fixed: Some(target), tokens, stay_with_the_run: false, state: Default::default() }
    }

    /// How many different servers this endpoint has led to so far: it goes
    /// up when a restart is followed to a new run.
    pub fn generation(&self) -> u64 {
        let _ = self.resolve();
        self.state.lock().unwrap_or_else(|e| e.into_inner()).generation
    }

    /// Whether this endpoint finds its server itself.
    pub fn discovers(&self) -> bool {
        self.fixed.is_none()
    }

    /// The `--server` value to hand another command so it reaches the same
    /// server, or `None` when it would find it the same way.
    pub fn server_flag(&self) -> Option<String> {
        self.fixed.as_ref().map(|t| t.to_string())
    }

    /// Where requests go now, resolving if nothing is cached.
    pub fn resolve(&self) -> anyhow::Result<Resolved> {
        let mut st = self.state.lock().unwrap_or_else(|e| e.into_inner());
        if let Some(r) = &st.current {
            return Ok(r.clone());
        }
        let (target, instance) = match &self.fixed {
            Some(t) => {
                if let Target::Unix(p) = t {
                    check_private_file(p, true)?;
                }
                (t.clone(), instance_of(t))
            }
            None => {
                let found = discover(st.pinned.as_ref())?;
                if self.stay_with_the_run {
                    if let Some(first) = st.instance.as_ref().filter(|i| **i != found.record.instance) {
                        anyhow::bail!(
                            "the `cadpole serve` this listener was attached to (instance {first}) was restarted; \
                             start the listener again to attach to the new run"
                        );
                    }
                }
                if st.pinned.is_none() {
                    st.pinned = Some(found.record.document.clone());
                    st.instance = Some(found.record.instance.clone());
                }
                (found.target, Some(found.record.instance))
            }
        };
        let token = self.token_for(instance.as_deref())?;
        if st.last.as_ref() != Some(&target) {
            st.last = Some(target.clone());
            st.generation += 1;
        }
        let r = Resolved { target, token };
        st.current = Some(r.clone());
        Ok(r)
    }

    /// Forget the cached resolution: the server went away, or turned the
    /// token down. The next [`Self::resolve`] looks again.
    pub fn invalidate(&self) {
        self.state.lock().unwrap_or_else(|e| e.into_inner()).current = None;
    }

    fn token_for(&self, instance: Option<&str>) -> anyhow::Result<Option<String>> {
        let dir = security::runtime_dir();
        let run = |name: fn(&RunFiles) -> PathBuf| -> Option<String> {
            let (dir, inst) = (dir.as_ref()?, instance?);
            read_token_file(&name(&RunFiles::new(dir, inst)))
        };
        Ok(match &self.tokens {
            TokenSource::Explicit(t) => Some(t.clone()),
            TokenSource::Owner { file: Some(f) } => crate::ops_host::token(None, Some(f)),
            TokenSource::Owner { file: None } => run(RunFiles::session_token).or_else(|| crate::ops_host::token(None, None)),
            TokenSource::Listener { file } => {
                let env = std::env::var("CADPOLE_LISTENER_TOKEN").ok().filter(|t| !t.is_empty());
                let t = env
                    .or_else(|| file.as_deref().and_then(read_token_file))
                    .or_else(|| run(RunFiles::listener_token))
                    .or_else(|| security::listener_token_path().and_then(|p| read_token_file(&p)));
                if t.is_none() {
                    // Falling back to the owner token here would silently turn
                    // the limited listener into an unlimited one.
                    anyhow::bail!(
                        "no restricted listener token: `cadpole serve` writes one for each run into its runtime \
                         directory (or set CADPOLE_LISTENER_TOKEN); refusing to fall back to the owner token"
                    );
                }
                t
            }
        })
    }
}

/// The instance a socket in the runtime directory belongs to, from its name.
fn instance_of(t: &Target) -> Option<String> {
    let Target::Unix(p) = t else { return None };
    let stem = p.file_stem().and_then(|s| s.to_str())?;
    if valid_instance(stem) && p.parent().zip(security::runtime_dir()).is_some_and(|(a, b)| a == b) {
        return Some(stem.to_owned());
    }
    // An explicit `--socket` elsewhere: the record that names it.
    let dir = security::runtime_dir()?;
    records(&dir).0.into_iter().find(|r| r.socket.as_deref() == Some(p.as_path())).map(|r| r.instance)
}

fn read_token_file(path: &Path) -> Option<String> {
    std::fs::read_to_string(path).ok().map(|s| s.trim().to_owned()).filter(|t| !t.is_empty())
}

// ---------------------------------------------------------------------------
// HTTP/1.1 over a Unix socket
// ---------------------------------------------------------------------------

/// Why a request over the socket failed, split by whether it was sent.
#[derive(Debug)]
pub enum SendError {
    /// Nothing was sent: the socket is missing or nobody is listening. Safe
    /// to retry anywhere, because the server never saw the request.
    Connect(std::io::Error),
    /// The connection broke after the request started out. The server may
    /// have run it, so it is reported, never retried.
    Io(std::io::Error),
}

impl std::fmt::Display for SendError {
    fn fmt(&self, f: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
        match self {
            SendError::Connect(e) => write!(f, "cannot connect: {e}"),
            SendError::Io(e) => write!(f, "connection lost: {e}"),
        }
    }
}

/// One request, one connection: `Connection: close`, a `Content-Length`
/// body, and the response read to the end (`Content-Length` or chunked).
///
/// Hand-written rather than a ureq transport: it is the only thing the agent
/// clients need, it is a few dozen lines, and one request per connection is
/// what makes "sent or not" exact, which is the line between a retry and a
/// reported failure.
#[cfg(unix)]
pub fn unix_request(
    socket: &Path,
    method: &str,
    path: &str,
    headers: &[(&str, &str)],
    body: Option<&[u8]>,
    timeout: Duration,
) -> Result<(u16, Vec<u8>), SendError> {
    let mut s = std::os::unix::net::UnixStream::connect(socket).map_err(SendError::Connect)?;
    s.set_read_timeout(Some(timeout)).map_err(SendError::Io)?;
    s.set_write_timeout(Some(timeout)).map_err(SendError::Io)?;
    let mut head = format!("{method} {path} HTTP/1.1\r\nHost: localhost\r\nConnection: close\r\n");
    for (k, v) in headers {
        head.push_str(&format!("{k}: {v}\r\n"));
    }
    if let Some(b) = body {
        head.push_str(&format!("Content-Length: {}\r\n", b.len()));
    }
    head.push_str("\r\n");
    // A server may answer before it has read the whole request -- a route
    // that does not exist, a refused token, a body over the limit -- and,
    // the connection being `Connection: close`, close it at once. The rest
    // of the request then fails with EPIPE (or ECONNRESET), but the answer
    // is already in this socket's receive buffer: read it. Only when there
    // is no complete answer is the write error the result.
    let sent = s.write_all(head.as_bytes()).and_then(|()| body.map_or(Ok(()), |b| s.write_all(b)));
    let sent = match sent {
        Err(e) if !matches!(e.kind(), std::io::ErrorKind::BrokenPipe | std::io::ErrorKind::ConnectionReset) => {
            return Err(SendError::Io(e));
        }
        other => other,
    };
    let mut raw = Vec::new();
    let read = s.read_to_end(&mut raw);
    match (sent, read, parse_response(&raw)) {
        (_, Ok(_), Ok(answer)) => Ok(answer),
        (Err(write), _, _) => Err(SendError::Io(write)),
        (Ok(()), Err(e), _) => Err(SendError::Io(e)),
        (Ok(()), Ok(_), Err(e)) => Err(SendError::Io(std::io::Error::new(std::io::ErrorKind::InvalidData, e))),
    }
}

#[cfg(not(unix))]
pub fn unix_request(
    _socket: &Path,
    _method: &str,
    _path: &str,
    _headers: &[(&str, &str)],
    _body: Option<&[u8]>,
    _timeout: Duration,
) -> Result<(u16, Vec<u8>), SendError> {
    Err(SendError::Connect(std::io::Error::other("Unix sockets are not available on this platform; pass --server http://HOST:PORT")))
}

/// Status and body of a complete HTTP/1.1 response.
fn parse_response(raw: &[u8]) -> Result<(u16, Vec<u8>), String> {
    let end = raw.windows(4).position(|w| w == b"\r\n\r\n").ok_or("no end of headers")?;
    let head = std::str::from_utf8(&raw[..end]).map_err(|_| "headers are not UTF-8")?;
    let mut lines = head.split("\r\n");
    let status_line = lines.next().ok_or("empty response")?;
    let status: u16 = status_line.split_whitespace().nth(1).and_then(|s| s.parse().ok()).ok_or("no status code")?;
    let (mut length, mut chunked) = (None, false);
    for l in lines {
        let Some((k, v)) = l.split_once(':') else { continue };
        let (k, v) = (k.trim().to_ascii_lowercase(), v.trim());
        if k == "content-length" {
            length = v.parse::<usize>().ok();
        } else if k == "transfer-encoding" && v.to_ascii_lowercase().contains("chunked") {
            chunked = true;
        }
    }
    let rest = &raw[end + 4..];
    let body = if chunked {
        dechunk(rest)?
    } else if let Some(n) = length {
        if rest.len() < n {
            return Err(format!("body cut short: {} of {n} bytes", rest.len()));
        }
        rest[..n].to_vec()
    } else {
        rest.to_vec()
    };
    Ok((status, body))
}

fn dechunk(mut rest: &[u8]) -> Result<Vec<u8>, String> {
    let mut out = Vec::new();
    loop {
        let eol = rest.windows(2).position(|w| w == b"\r\n").ok_or("chunk size line cut short")?;
        let size_text = std::str::from_utf8(&rest[..eol]).map_err(|_| "bad chunk size")?;
        let size = usize::from_str_radix(size_text.split(';').next().unwrap_or("").trim(), 16).map_err(|_| "bad chunk size")?;
        rest = &rest[eol + 2..];
        if size == 0 {
            return Ok(out);
        }
        if rest.len() < size + 2 {
            return Err("chunk cut short".to_owned());
        }
        out.extend_from_slice(&rest[..size]);
        rest = &rest[size + 2..];
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    fn tmp(name: &str) -> PathBuf {
        let d = std::env::temp_dir().join(format!("cadpole-rt-{name}-{}", std::process::id()));
        let _ = std::fs::remove_dir_all(&d);
        d
    }

    #[test]
    fn server_flags_parse() {
        assert_eq!(parse_server("unix:/a/b.sock").unwrap(), Target::Unix("/a/b.sock".into()));
        assert_eq!(parse_server("http://127.0.0.1:7331/").unwrap(), Target::Http("http://127.0.0.1:7331".into()));
        assert!(parse_server("unix:").is_err());
        assert!(parse_server("127.0.0.1:7331").is_err());
        assert_eq!(Target::Unix("/x.sock".into()).to_string(), "unix:/x.sock");
    }

    #[test]
    fn instance_ids_are_fresh_and_checked() {
        let (a, b) = (new_instance(), new_instance());
        assert_ne!(a, b);
        assert!(valid_instance(&a), "{a}");
        assert!(!valid_instance("../../etc"));
        assert!(!valid_instance("ABCDEF012345"));
    }

    #[test]
    fn responses_parse_with_length_chunks_or_neither() {
        let r = b"HTTP/1.1 200 OK\r\ncontent-length: 5\r\n\r\nhello";
        assert_eq!(parse_response(r).unwrap(), (200, b"hello".to_vec()));
        let r = b"HTTP/1.1 403 Forbidden\r\ntransfer-encoding: chunked\r\n\r\n3\r\nabc\r\n2\r\nde\r\n0\r\n\r\n";
        assert_eq!(parse_response(r).unwrap(), (403, b"abcde".to_vec()));
        let r = b"HTTP/1.1 404 Not Found\r\n\r\nno";
        assert_eq!(parse_response(r).unwrap(), (404, b"no".to_vec()));
        assert!(parse_response(b"HTTP/1.1 200 OK\r\ncontent-length: 9\r\n\r\nshort").is_err());
    }

    /// The directory checks a server and a client both rely on: a missing
    /// directory is made 0700, a loose one of ours is tightened, and anything
    /// that is not a real directory is refused.
    #[cfg(unix)]
    #[test]
    fn the_runtime_directory_must_be_private_and_ours() {
        use std::os::unix::fs::PermissionsExt as _;
        let base = tmp("dir");
        let dir = base.join("cadpole");
        prepare_private_dir(&dir).unwrap();
        assert_eq!(std::fs::metadata(&dir).unwrap().permissions().mode() & 0o777, 0o700);
        std::fs::set_permissions(&dir, std::fs::Permissions::from_mode(0o755)).unwrap();
        assert!(check_private_dir(&dir).is_err(), "a world-readable directory is refused");
        prepare_private_dir(&dir).unwrap();
        assert_eq!(std::fs::metadata(&dir).unwrap().permissions().mode() & 0o777, 0o700, "ours, so tightened");
        let link = base.join("link");
        std::os::unix::fs::symlink(&dir, &link).unwrap();
        assert!(check_private_dir(&link).is_err(), "a symlink to a directory is refused");
        // Another user's directory: `/` is root's, and this test never runs as root.
        if euid() != Some(0) {
            let e = check_private_dir(Path::new("/")).unwrap_err().to_string();
            assert!(e.contains("another user"), "{e}");
        }
        let f = dir.join("x.json");
        std::fs::write(&f, b"{}").unwrap();
        std::fs::set_permissions(&f, std::fs::Permissions::from_mode(0o644)).unwrap();
        assert!(check_private_file(&f, false).is_err(), "a readable record is refused");
        std::fs::set_permissions(&f, std::fs::Permissions::from_mode(0o600)).unwrap();
        check_private_file(&f, false).unwrap();
        assert!(check_private_file(&f, true).is_err(), "a regular file is not a socket");
        let _ = std::fs::remove_dir_all(&base);
    }

    fn record(dir: &Path, instance: &str, socket: Option<PathBuf>, doc: Option<&str>) -> Record {
        let r = Record {
            instance: instance.to_owned(),
            pid: std::process::id(),
            socket,
            http: None,
            agent_tcp: false,
            document: doc.map(PathBuf::from),
            started: "2026-01-01T00:00:00Z".into(),
        };
        write_private_atomic(&RunFiles::new(dir, instance).record(), &serde_json::to_vec(&r).unwrap()).unwrap();
        r
    }

    #[cfg(unix)]
    fn bind(path: &Path) -> std::os::unix::net::UnixListener {
        use std::os::unix::fs::PermissionsExt as _;
        let l = std::os::unix::net::UnixListener::bind(path).unwrap();
        std::fs::set_permissions(path, std::fs::Permissions::from_mode(0o600)).unwrap();
        l
    }

    /// A server that answers from the head alone and closes, leaving most of
    /// the body unread, is heard: the client's write fails with EPIPE, and
    /// the answer it already has is the result. The body is far larger than
    /// a socket buffer, so the close always lands mid-write.
    #[cfg(unix)]
    #[test]
    fn an_answer_sent_before_the_body_is_read_is_still_the_answer() {
        let dir = tmp("early");
        prepare_private_dir(&dir).unwrap();
        let path = dir.join("early.sock");
        let l = bind(&path);
        let server = std::thread::spawn(move || {
            let (mut c, _) = l.accept().unwrap();
            let (mut seen, mut buf) = (Vec::new(), [0u8; 4096]);
            while !seen.windows(4).any(|w| w == b"\r\n\r\n") {
                let n = c.read(&mut buf).unwrap();
                assert!(n > 0, "the head arrives");
                seen.extend_from_slice(&buf[..n]);
            }
            c.write_all(b"HTTP/1.1 413 Payload Too Large\r\ncontent-length: 8\r\nconnection: close\r\n\r\ntoo long").unwrap();
        });
        let body = vec![b'x'; 16 << 20];
        let got = unix_request(&path, "POST", "/api/ops/SetScript", &[], Some(&body), Duration::from_secs(30));
        server.join().unwrap();
        assert_eq!(got.unwrap(), (413, b"too long".to_vec()));
        // A server that closes without answering is still a lost connection.
        let l = {
            let _ = std::fs::remove_file(&path);
            bind(&path)
        };
        let server = std::thread::spawn(move || drop(l.accept().unwrap()));
        let got = unix_request(&path, "POST", "/x", &[], Some(&body), Duration::from_secs(30));
        server.join().unwrap();
        assert!(matches!(got, Err(SendError::Io(_))), "{:?}", got.map_err(|e| e.to_string()));
        let _ = std::fs::remove_dir_all(&dir);
    }

    /// Zero, one and two servers, and a crashed one, by discovery alone.
    #[cfg(unix)]
    #[test]
    fn discovery_finds_zero_one_or_two_servers_and_never_a_dead_one() {
        let dir = tmp("disc");
        prepare_private_dir(&dir).unwrap();
        let none = discover_in(&dir, None, None).unwrap_err().to_string();
        assert!(none.contains("no running `cadpole serve`"), "{none}");

        let a = RunFiles::new(&dir, "aaaaaaaaaaaa");
        let _la = bind(&a.socket());
        record(&dir, "aaaaaaaaaaaa", Some(a.socket()), Some("/work/one/a.star"));
        let one = discover_in(&dir, None, None).unwrap();
        assert_eq!(one.target, Target::Unix(a.socket()));

        let b = RunFiles::new(&dir, "bbbbbbbbbbbb");
        let lb = bind(&b.socket());
        record(&dir, "bbbbbbbbbbbb", Some(b.socket()), Some("/work/two/b.star"));
        let two = discover_in(&dir, None, None).unwrap_err().to_string();
        assert!(two.contains("2 `cadpole serve` instances") && two.contains("--server unix:"), "{two}");
        // ...unless the current directory settles it, whatever order the
        // directory lists them in.
        let here = discover_in(&dir, Some(Path::new("/work/two")), None).unwrap();
        assert_eq!(here.record.instance, "bbbbbbbbbbbb");
        assert!(discover_in(&dir, Some(Path::new("/work")), None).is_err(), "both are below /work: ambiguous");

        // b crashes: its socket file stays, nothing answers it.
        drop(lb);
        leave_dead_socket(&b.socket());
        assert!(b.socket().exists());
        let after = discover_in(&dir, Some(Path::new("/work/two")), None).unwrap();
        assert_eq!(after.record.instance, "aaaaaaaaaaaa", "a dead server is never chosen");
        // A client pinned to b's document does not move to a.
        let pinned = Some(PathBuf::from("/work/two/b.star"));
        let e = discover_in(&dir, None, Some(&pinned)).unwrap_err().to_string();
        assert!(e.contains("no longer running"), "{e}");

        // Cleanup removes b's files and nothing of a's.
        let removed = sweep_stale(&dir);
        assert!(removed.contains(&b.socket()) && removed.contains(&b.record()), "{removed:?}");
        assert!(a.socket().exists() && a.record().exists(), "a live server's files are never removed");
        let _ = std::fs::remove_dir_all(&dir);
    }

    /// A record or socket that is not the user's own private file is passed
    /// over, never followed and never deleted.
    #[cfg(unix)]
    #[test]
    fn a_loose_record_or_socket_is_not_followed_or_removed() {
        use std::os::unix::fs::PermissionsExt as _;
        let dir = tmp("loose");
        prepare_private_dir(&dir).unwrap();
        let a = RunFiles::new(&dir, "cccccccccccc");
        let _l = bind(&a.socket());
        record(&dir, "cccccccccccc", Some(a.socket()), None);
        std::fs::set_permissions(a.socket(), std::fs::Permissions::from_mode(0o666)).unwrap();
        let (found, skipped) = live_servers(&dir);
        assert!(found.is_empty() && skipped.iter().any(|s| s.contains("0600")), "{skipped:?}");
        std::fs::set_permissions(a.socket(), std::fs::Permissions::from_mode(0o600)).unwrap();
        std::fs::set_permissions(a.record(), std::fs::Permissions::from_mode(0o644)).unwrap();
        let (found, _) = live_servers(&dir);
        assert!(found.is_empty(), "a record others could have written is not trusted");
        drop(_l);
        sweep_stale(&dir);
        assert!(a.record().exists(), "and not deleted either");
        let _ = std::fs::remove_dir_all(&dir);
    }
}
