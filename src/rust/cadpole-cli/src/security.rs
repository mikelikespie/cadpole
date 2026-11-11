//! The parts of `docs/security.md` that are not about geometry: the bearer
//! token, where it is stored, what counts as a same-origin request, the
//! response headers, and the one directory documents may be read from.
//!
//! Kept out of `server.rs` because every one of these is a rule worth testing
//! on its own, and because "is this request allowed" should be readable
//! without scrolling past forty route handlers.

use cadpole_engine::cadpole_proto::file_types::DOCUMENT_EXT;
use std::io::Write as _;
use std::path::{Component, Path, PathBuf};

use axum::http::{HeaderMap, HeaderValue, Method, header};

// ---------------------------------------------------------------------------
// Size caps (docs/security.md)
// ---------------------------------------------------------------------------

/// Largest HTTP request body, in bytes.
pub const MAX_BODY_BYTES: usize = 8 * 1024 * 1024;
/// Largest script file a session will store.
pub const MAX_SCRIPT_BYTES: usize = cadpole_engine::edit::MAX_SCRIPT_BYTES;
/// Largest packed document read from or written to the document root.
pub const MAX_DOCUMENT_BYTES: u64 = 16 * 1024 * 1024;
/// Most parameter overrides one session document may hold, and the longest a
/// parameter name may be.
///
/// `SetParams` is granted to the restricted listener as "only an override
/// map: no code, one revision", and nothing bounded that map: each call
/// merged more entries into the live session document and they persisted, so
/// every later `GetSession`, `/api/session/document` and `params_changed`
/// event carried the whole of it. Repeated calls drove RSS past 1.5 GB and a single
/// `GetSession` to 88 MB that way. `MAX_DOCUMENT_BYTES` did not help, because
/// it is checked only where a document meets the disk.
///
/// Counted rather than measured: re-encoding the document on every call would
/// put a proto encode in the middle of a slider drag, the one path that has
/// to stay cheap. Overwriting a name that is already there is always allowed,
/// so a drag never meets this at all. 4096 x 256 bytes bounds the map at
/// about 1 MB, comfortably inside the document cap.
pub const MAX_PARAM_OVERRIDES: usize = 4096;
/// See [`MAX_PARAM_OVERRIDES`].
pub const MAX_PARAM_KEY_BYTES: usize = 256;
/// Largest message text.
pub const MAX_MESSAGE_BYTES: usize = 64 * 1024;
/// Largest total of a message's attachments, base64 included.
pub const MAX_ATTACHMENT_BYTES: usize = 6 * 1024 * 1024;

// ---- Client failure reports.
//
// A crashing front end is the one client guaranteed to behave badly: a panic
// inside a render loop fires thousands of times a second, and the report is
// caller-written text that lands in the event log every attached assistant
// reads. So the report is capped twice -- per report and per session -- and
// every cap is *visible*: an over-long field is truncated and the response
// names it, a refused report gets a reason, and the first refusal in a window
// becomes its own `client_failure` event so the log shows that reports are
// being lost rather than that the crashing stopped.

/// Largest `message` a failure report may carry.
pub const MAX_FAILURE_MESSAGE_BYTES: usize = 2_000;
/// Largest `stack` a failure report may carry.
pub const MAX_FAILURE_STACK_BYTES: usize = 8_000;
/// Largest any other free-text field of a report may be (`location`,
/// `operation`, `engine_build`, `engine_kind`, `script_digest`).
pub const MAX_FAILURE_FIELD_BYTES: usize = 300;
/// How many `params` entries a report may carry. The parameter map is the
/// user's own model, but it is still caller-supplied, so it is bounded.
pub const MAX_FAILURE_PARAMS: usize = 200;
/// Reports accepted back-to-back before the rate limit bites (the burst).
pub const FAILURE_REPORT_BURST: u32 = 10;
/// How often the burst refills, in milliseconds per token: 1 report every
/// 2 s in the steady state, which is ample for a human-visible crash and
/// hopeless for a render loop.
pub const FAILURE_REPORT_REFILL_MS: u64 = 2_000;
/// Reports one session will record in total. Past this the session stops
/// accepting them and says so: something is wrong that more reports of the
/// same thing will not diagnose.
pub const MAX_FAILURE_REPORTS_PER_SESSION: u32 = 200;
/// How often a refusal is allowed to emit its own throttle event, so the
/// notice that reports are being dropped cannot itself become the flood.
pub const FAILURE_THROTTLE_NOTICE_MS: u64 = 10_000;

// ---------------------------------------------------------------------------
// Token
// ---------------------------------------------------------------------------

/// `~/.config/cadpole`, honouring `XDG_CONFIG_HOME`. Nothing here creates it; the
/// callers that write do.
///
/// Configuration, not secrets: tokens live in the runtime directory.
/// [`legacy_token_path`] still reads the old location here as a fallback.
pub fn config_dir() -> Option<PathBuf> {
    if let Ok(x) = std::env::var("CADPOLE_CONFIG_DIR") {
        if !x.is_empty() {
            return Some(PathBuf::from(x));
        }
    }
    if let Ok(x) = std::env::var("XDG_CONFIG_HOME") {
        if !x.is_empty() {
            return Some(PathBuf::from(x).join("cadpole"));
        }
    }
    std::env::var("HOME").ok().filter(|h| !h.is_empty()).map(|h| PathBuf::from(h).join(".config").join("cadpole"))
}

/// Where this *run's* tokens live: `$XDG_RUNTIME_DIR/cadpole`, else
/// `$XDG_STATE_HOME/cadpole`, else `~/.local/state/cadpole`.
///
/// A token is not configuration. It is per-run state that should not outlive
/// the process that minted it, which is exactly what the runtime directory is
/// for: on a systemd machine it is a private tmpfs at 0700 that is deleted when
/// the user logs out. `CADPOLE_RUNTIME_DIR` overrides everything (the tests use
/// it), and `CADPOLE_CONFIG_DIR` is honoured too so a single environment variable
/// still relocates the whole of Cadpole's state.
pub fn runtime_dir() -> Option<PathBuf> {
    for var in ["CADPOLE_RUNTIME_DIR", "CADPOLE_CONFIG_DIR"] {
        if let Ok(x) = std::env::var(var) {
            if !x.is_empty() {
                return Some(PathBuf::from(x));
            }
        }
    }
    if let Ok(x) = std::env::var("XDG_RUNTIME_DIR") {
        if !x.is_empty() {
            return Some(PathBuf::from(x).join("cadpole"));
        }
    }
    if let Ok(x) = std::env::var("XDG_STATE_HOME") {
        if !x.is_empty() {
            return Some(PathBuf::from(x).join("cadpole"));
        }
    }
    std::env::var("HOME")
        .ok()
        .filter(|h| !h.is_empty())
        .map(|h| PathBuf::from(h).join(".local").join("state").join("cadpole"))
}

/// The file `cadpole serve` writes for this run and every client reads.
pub fn token_path() -> Option<PathBuf> {
    runtime_dir().map(|d| d.join("session-token"))
}

/// The file holding this run's *restricted* token: the one the limited
/// listener gets (`cadpole-ops::capability`).
pub fn listener_token_path() -> Option<PathBuf> {
    runtime_dir().map(|d| d.join("listener-token"))
}

/// The token's old location in the config directory. Read-only, and only as
/// a fallback.
pub fn legacy_token_path() -> Option<PathBuf> {
    config_dir().map(|d| d.join("token"))
}

/// 32 random bytes, base64url, no padding. Unguessable, and safe in a URL
/// fragment and in a shell.
pub fn generate_token() -> String {
    use rand::RngCore;
    let mut b = [0u8; 32];
    rand::rng().fill_bytes(&mut b);
    base64url(&b)
}

fn base64url(bytes: &[u8]) -> String {
    const A: &[u8; 64] = b"ABCDEFGHIJKLMNOPQRSTUVWXYZabcdefghijklmnopqrstuvwxyz0123456789-_";
    let mut out = String::with_capacity(bytes.len().div_ceil(3) * 4);
    for c in bytes.chunks(3) {
        let b = [c[0], *c.get(1).unwrap_or(&0), *c.get(2).unwrap_or(&0)];
        let n = (u32::from(b[0]) << 16) | (u32::from(b[1]) << 8) | u32::from(b[2]);
        let chars = [(n >> 18) & 63, (n >> 12) & 63, (n >> 6) & 63, n & 63];
        for (i, ch) in chars.iter().enumerate() {
            if i <= c.len() {
                out.push(A[*ch as usize] as char);
            }
        }
    }
    out
}

/// The tokens one `cadpole serve` run mints, and where it put them.
#[derive(Clone, Debug)]
pub struct SessionTokens {
    /// The browser's: what the `open:` link carries, for the web UI and the
    /// apps. Accepted on both surfaces. `--token`/`CADPOLE_TOKEN` pins it.
    /// Never written to a token file.
    pub browser: String,
    /// Full authority for the agent clients: `cadpole op`, an interactive
    /// `cadpole mcp`, `cadpole listen`. Accepted on the agent socket, and on
    /// TCP only with `--agent-tcp`.
    pub owner: String,
    /// The limited listener's token (`cadpole_ops::capability::GRANTED`).
    /// Accepted where the owner's agent token is.
    pub listener: String,
    /// The file the owner token was written to, if it could be written.
    pub owner_file: Option<PathBuf>,
    /// The file the listener token was written to, if it could be written.
    pub listener_file: Option<PathBuf>,
    /// Why nothing was stored, when nothing was.
    pub warning: Option<String>,
}

/// Mint this run's tokens and write the agent ones, 0600, into a 0700
/// directory.
///
/// **Every start mints new ones.** A token is a capability that should not
/// outlive the process that issued it: a server that is restarted is a new
/// session with a new event log, and a link handed to a phone three weeks ago
/// should not still open it. The cost is that a bookmarked `#token=` URL stops
/// working after a restart — which is the correct behaviour, and is why the
/// server prints the new URL on every start.
///
/// `pinned` replaces the generated browser token (`--token`/`CADPOLE_TOKEN`).
///
/// The agent tokens go to `session-token` and `listener-token` (the newest
/// run's, for clients that predate discovery) and, with `instance`, to
/// `<instance>.session-token` and `<instance>.listener-token` too, so two
/// servers running at once each keep their own.
///
/// A runtime directory that cannot be written is not fatal: the server runs
/// with tokens that last as long as the process and says so, and a client then
/// needs `CADPOLE_TOKEN`.
pub fn mint_tokens(pinned: Option<String>, instance: Option<&str>) -> SessionTokens {
    mint_run_tokens(runtime_dir().as_deref(), instance, pinned)
}

/// [`mint_tokens`] into a named directory, with no instance. Separate so a
/// test can say where without touching the process environment, which the
/// tests share.
#[cfg(test)]
pub fn mint_tokens_in(dir: Option<&Path>, pinned: Option<String>) -> SessionTokens {
    mint_run_tokens(dir, None, pinned)
}

pub fn mint_run_tokens(dir: Option<&Path>, instance: Option<&str>, pinned: Option<String>) -> SessionTokens {
    let browser = pinned.filter(|t| !t.is_empty()).unwrap_or_else(generate_token);
    let (owner, listener) = (generate_token(), generate_token());
    let Some(dir) = dir else {
        return SessionTokens {
            browser,
            owner,
            listener,
            owner_file: None,
            listener_file: None,
            warning: Some("no HOME, XDG_RUNTIME_DIR or XDG_STATE_HOME: the tokens are not stored".to_owned()),
        };
    };
    let mut warning = None;
    let mut write = |name: &str, token: &str| {
        let mut first = None;
        let names = match instance {
            Some(i) => vec![format!("{i}.{name}"), name.to_owned()],
            None => vec![name.to_owned()],
        };
        for n in names {
            let path = dir.join(n);
            match write_private(&path, token.as_bytes()) {
                Ok(()) => {
                    first.get_or_insert(path);
                }
                Err(e) => {
                    warning.get_or_insert(format!("could not write {}: {e}", path.display()));
                }
            }
        }
        first
    };
    let owner_file = write("session-token", &owner);
    let listener_file = write("listener-token", &listener);
    SessionTokens { browser, owner, listener, owner_file, listener_file, warning }
}

/// The two ways into one server.
///
/// - `Browser`: loopback TCP (or `--listen`). The web UI, its static files and
///   every route the page uses, behind the origin checks.
/// - `Agent`: the Unix socket in the private runtime directory. The operation
///   registry and the two reads an agent client needs; no web UI.
///
/// The surface decides only *which tokens are admitted* and *which routes
/// exist*. What an admitted token may do is decided by its role alone
/// (`capability_refusal` in server.rs takes no surface), so the same
/// operation gets the same answer on every surface that admits the token
/// (spec/transport.qnt, `aTokensDecisionNeverDependsOnTheSurface`).
#[derive(Clone, Copy, Debug, PartialEq, Eq, Hash)]
pub enum Surface {
    Browser,
    Agent,
}

/// What the guard learns from a bearer token on one surface.
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub enum Admission {
    /// A token of this run, admitted here, carrying this role.
    Admitted(cadpole_ops::capability::Role),
    /// A token of this run that this surface does not admit.
    NotHere(cadpole_ops::capability::Role),
    /// Not a token of this run.
    Unknown,
}

/// The tokens a running server accepts, the role each one carries, and the
/// surfaces each one is admitted on.
///
/// Authorisation is attached to the *token*, server side, because that is the
/// only place a confused or compromised client cannot reach: an agent that
/// holds the listener token cannot promote itself by editing its own allow-list
/// or by calling an endpoint its MCP client did not advertise.
#[derive(Clone, Debug, Default)]
pub struct TokenSet {
    /// (token, role, admitted on Browser, admitted on Agent)
    entries: Vec<(String, cadpole_ops::capability::Role, bool, bool)>,
}

impl TokenSet {
    /// No authentication at all (`--no-auth`, loopback only).
    pub fn open() -> TokenSet {
        TokenSet { entries: Vec::new() }
    }

    /// One owner token and one listener token, both admitted on every
    /// surface: what `--agent-tcp` gives the agent tokens.
    #[cfg(test)]
    pub fn new(owner: &str, listener: &str) -> TokenSet {
        use cadpole_ops::capability::Role;
        TokenSet { entries: vec![(owner.to_owned(), Role::Owner, true, true), (listener.to_owned(), Role::Listener, true, true)] }
    }

    /// A run's tokens: the browser's on both surfaces; the agent owner's and
    /// the listener's on the socket, and on TCP only when `agent_tcp`.
    pub fn for_run(browser: &str, owner: &str, listener: &str, agent_tcp: bool) -> TokenSet {
        use cadpole_ops::capability::Role;
        TokenSet {
            entries: vec![
                (browser.to_owned(), Role::Owner, true, true),
                (owner.to_owned(), Role::Owner, agent_tcp, true),
                (listener.to_owned(), Role::Listener, agent_tcp, true),
            ],
        }
    }

    /// Just an owner token.
    #[cfg(test)]
    pub fn owner_only(owner: &str) -> TokenSet {
        TokenSet { entries: vec![(owner.to_owned(), cadpole_ops::capability::Role::Owner, true, true)] }
    }

    /// True when no token is required at all.
    pub fn is_open(&self) -> bool {
        self.entries.is_empty()
    }

    /// The role `presented` carries, on any surface, or `None`.
    #[cfg(test)]
    pub fn role_of(&self, presented: &str) -> Option<cadpole_ops::capability::Role> {
        match self.admit(presented, Surface::Agent) {
            Admission::Admitted(r) | Admission::NotHere(r) => Some(r),
            Admission::Unknown => match self.admit(presented, Surface::Browser) {
                Admission::Admitted(r) | Admission::NotHere(r) => Some(r),
                Admission::Unknown => None,
            },
        }
    }

    /// What `presented` is on `surface`.
    ///
    /// Every entry is compared, with no early exit, so neither the match nor
    /// *which* token matched is visible in the time this takes.
    pub fn admit(&self, presented: &str, surface: Surface) -> Admission {
        let mut found = Admission::Unknown;
        for (token, role, browser, agent) in &self.entries {
            if constant_time_eq(presented, token) {
                let here = match surface {
                    Surface::Browser => *browser,
                    Surface::Agent => *agent,
                };
                found = if here { Admission::Admitted(*role) } else { Admission::NotHere(*role) };
            }
        }
        found
    }
}

/// Write a file only the owner can read (0600 on unix), creating the parent
/// directory 0700 so the file is not merely private but unreachable.
pub fn write_private(path: &Path, bytes: &[u8]) -> std::io::Result<()> {
    if let Some(dir) = path.parent() {
        std::fs::create_dir_all(dir)?;
        #[cfg(unix)]
        {
            use std::os::unix::fs::PermissionsExt as _;
            let _ = std::fs::set_permissions(dir, std::fs::Permissions::from_mode(0o700));
        }
    }
    let mut opts = std::fs::OpenOptions::new();
    opts.write(true).create(true).truncate(true);
    #[cfg(unix)]
    {
        use std::os::unix::fs::OpenOptionsExt as _;
        opts.mode(0o600);
    }
    let mut f = opts.open(path)?;
    f.write_all(bytes)?;
    f.write_all(b"\n")?;
    // An existing file keeps its old mode, so set it explicitly too.
    #[cfg(unix)]
    {
        use std::os::unix::fs::PermissionsExt as _;
        let _ = std::fs::set_permissions(path, std::fs::Permissions::from_mode(0o600));
    }
    Ok(())
}

/// Write `bytes` to `path` all at once: into a new temporary file beside it,
/// then renamed over it.
///
/// A reader sees the old file or the new one, never a truncated one, and a
/// failure part way (a full disk) leaves the old file as it was. The
/// temporary file is created with `create_new`, so it can be neither an
/// existing file nor a symlink someone planted, and `rename` replaces a
/// directory entry rather than following it: a symlink at `path` is replaced
/// by the file, never written through.
pub fn write_replacing(path: &Path, bytes: &[u8]) -> std::io::Result<()> {
    let dir = path.parent().filter(|d| !d.as_os_str().is_empty()).unwrap_or(Path::new("."));
    std::fs::create_dir_all(dir)?;
    let name = path.file_name().and_then(|n| n.to_str()).unwrap_or("document");
    let tmp = dir.join(format!(".{name}.{}.{}.tmp", std::process::id(), generate_token().get(..12).unwrap_or("x")));
    let result = (|| {
        let mut f = std::fs::OpenOptions::new().write(true).create_new(true).open(&tmp)?;
        // The rename gives the path a new file; keep the old one's mode, so
        // a document the user made private stays private.
        if let Ok(meta) = std::fs::symlink_metadata(path) {
            if meta.is_file() {
                f.set_permissions(meta.permissions())?;
            }
        }
        f.write_all(bytes)?;
        f.sync_all()?;
        std::fs::rename(&tmp, path)
    })();
    if result.is_err() {
        let _ = std::fs::remove_file(&tmp);
    }
    result
}

/// Constant-time string comparison.
///
/// Hand-written rather than pulling in `subtle`: it is six lines, and a new
/// dependency here would need a licence review to save them. The compiler is
/// not allowed to short-circuit this because every byte feeds the same
/// accumulator and the result is only read at the end.
pub fn constant_time_eq(a: &str, b: &str) -> bool {
    let (a, b) = (a.as_bytes(), b.as_bytes());
    // The length is not a secret (it is fixed by `generate_token`), but the
    // comparison below still must not stop early.
    let mut diff = (a.len() ^ b.len()) as u8;
    for i in 0..a.len().max(b.len()) {
        let x = a.get(i).copied().unwrap_or(0);
        let y = b.get(i).copied().unwrap_or(0);
        diff |= x ^ y;
    }
    diff == 0
}

// ---------------------------------------------------------------------------
// Origin
// ---------------------------------------------------------------------------

/// Why a request was refused by the same-origin check.
#[derive(Debug, PartialEq, Eq)]
pub enum OriginVerdict {
    Allow,
    Refuse(&'static str),
}

/// Is this request allowed to reach the API at all?
///
/// The browser is the confused deputy: any page the user visits can `fetch`
/// `http://127.0.0.1:7331/...`, and on a tailnet it can reach every other
/// machine's Cadpole too. It cannot *read* the response (no CORS headers are
/// sent), but without this it could still write. Two signals, either of which
/// is enough to refuse:
///
/// * `Sec-Fetch-Site`, which browsers set and scripts cannot forge. Anything
///   other than `same-origin` or `none` (a typed URL) is refused.
/// * `Origin`, refused unless it matches this server's own `Host` or an
///   explicitly allowed one (`--allow-origin`, for the web dev server).
///
/// **Reads are checked too, not just writes.** The earlier version exempted
/// `GET`, on the grounds that a cross-origin read cannot be read back. That
/// was true only as long as the SSE stream took its token in the query string,
/// where a cross-origin `EventSource` or `<img>` could carry it; now that the
/// stream takes an `Authorization` header like everything else, refusing
/// cross-site `GET` outright costs nothing and removes the whole class.
/// `HEAD` and `OPTIONS` are left alone: they carry no data.
///
/// A CLI sends neither header and is allowed: it is not a browser, and it
/// already holds the bearer token.
pub fn check_origin(method: &Method, headers: &HeaderMap, allowed: &[String]) -> OriginVerdict {
    if method == Method::HEAD || method == Method::OPTIONS {
        return OriginVerdict::Allow;
    }
    let get = |n: &str| headers.get(n).and_then(|v| v.to_str().ok()).map(str::to_owned);
    if let Some(site) = get("sec-fetch-site") {
        let origin_ok = get("origin").map(|o| allowed.iter().any(|a| a == &o)).unwrap_or(false);
        if site != "same-origin" && site != "none" && !origin_ok {
            return OriginVerdict::Refuse("cross-site request refused (Sec-Fetch-Site)");
        }
    }
    if let Some(origin) = get("origin") {
        if allowed.iter().any(|a| a == &origin) {
            return OriginVerdict::Allow;
        }
        let host = headers.get(header::HOST).and_then(|v| v.to_str().ok()).unwrap_or("");
        let same = origin
            .strip_prefix("http://")
            .or_else(|| origin.strip_prefix("https://"))
            .map(|o| o == host)
            .unwrap_or(false);
        if !same {
            return OriginVerdict::Refuse("cross-origin request refused (Origin)");
        }
    }
    OriginVerdict::Allow
}

/// Does this request name the server by a loopback name?
///
/// The defence against DNS rebinding for a server with no token. A page on
/// `evil.example` can point its own name at 127.0.0.1; the browser then talks
/// to this server believing it is `evil.example`, so `Origin` matches `Host`,
/// `Sec-Fetch-Site` says `same-origin`, and [`check_origin`] has nothing to
/// object to. The `Host` header is the one thing that still says which site
/// the browser thinks it is on, and a rebinding page cannot change it.
///
/// Accepted: `localhost` and any `*.localhost` (browsers resolve those to
/// loopback themselves, never through DNS), a loopback IP literal, and the
/// host of any `--allow-origin`. The port is not checked: rebinding changes
/// the name, not the port, and a forwarded port is still the user's own.
/// A request with no `Host` at all is not from a browser and is let through.
///
/// Only `--no-auth` servers need this. With a token, the rebound page does not
/// have it, and a tailnet name the server cannot know in advance must work.
pub fn check_host(headers: &HeaderMap, allowed_origins: &[String]) -> OriginVerdict {
    let Some(host) = headers.get(header::HOST) else { return OriginVerdict::Allow };
    let Ok(host) = host.to_str() else { return OriginVerdict::Refuse("unreadable Host header") };
    let name = host_name(host).to_ascii_lowercase();
    let loopback_ip = name.trim_start_matches('[').trim_end_matches(']').parse::<std::net::IpAddr>().is_ok_and(|ip| ip.is_loopback());
    let allowed = allowed_origins.iter().any(|o| {
        let o = o.strip_prefix("http://").or_else(|| o.strip_prefix("https://")).unwrap_or(o);
        host_name(o).eq_ignore_ascii_case(&name)
    });
    if name == "localhost" || name.ends_with(".localhost") || loopback_ip || allowed {
        OriginVerdict::Allow
    } else {
        OriginVerdict::Refuse("this server has no token and answers only to a loopback name (Host); use http://127.0.0.1 or http://localhost")
    }
}

/// `host[:port]` or `[v6]:port` -> the host part, brackets kept for v6.
fn host_name(authority: &str) -> &str {
    let authority = authority.split('/').next().unwrap_or("");
    if authority.starts_with('[') {
        return authority.find(']').map(|i| &authority[..=i]).unwrap_or(authority);
    }
    authority.rsplit_once(':').map(|(h, _)| h).unwrap_or(authority)
}

/// JSON endpoints require `Content-Type: application/json`, which a plain HTML
/// form cannot set without a preflight the browser would then block. An empty
/// body carries no content type and is fine.
pub fn check_content_type(method: &Method, headers: &HeaderMap) -> bool {
    if method == Method::GET || method == Method::HEAD || method == Method::OPTIONS {
        return true;
    }
    match headers.get(header::CONTENT_TYPE).and_then(|v| v.to_str().ok()) {
        None => true,
        Some(ct) => {
            let ct = ct.split(';').next().unwrap_or("").trim().to_ascii_lowercase();
            ct.is_empty()
                || ct == "application/json"
                || ct == "application/x-protobuf"
                || ct == "application/protobuf"
                || ct == "application/octet-stream"
                // The Connect protocol's own media types. Like the
                // ones above them, none of these is a type an HTML form can
                // post: a form may only send `application/x-www-form-
                // urlencoded`, `multipart/form-data` or `text/plain`, so
                // accepting them adds no cross-origin write the browser would
                // not have had to preflight.
                || ct == "application/proto"
                || ct == "application/grpc-web"
                || ct == "application/grpc-web+proto"
                || ct == "application/grpc-web+json"
        }
    }
}

/// The headers every response carries (docs/security.md, "Origin and CORS").
///
/// `Cross-Origin-Embedder-Policy: require-corp` is here because the web UI
/// wants wasm threads, which need cross-origin isolation; it also happens to
/// stop another origin embedding anything served here. The CSP allows
/// `wasm-unsafe-eval` because instantiating a WebAssembly module counts as
/// eval, and `'unsafe-inline'` for styles only, which bundlers still emit.
pub fn security_headers() -> [(header::HeaderName, HeaderValue); 6] {
    [
        (header::X_CONTENT_TYPE_OPTIONS, HeaderValue::from_static("nosniff")),
        (header::REFERRER_POLICY, HeaderValue::from_static("no-referrer")),
        (
            header::CONTENT_SECURITY_POLICY,
            HeaderValue::from_static(
                "default-src 'self'; connect-src 'self'; worker-src 'self' blob:; \
                 img-src 'self' data: blob:; script-src 'self' 'wasm-unsafe-eval'; \
                 style-src 'self' 'unsafe-inline'; font-src 'self' data:; \
                 base-uri 'none'; form-action 'none'; frame-ancestors 'none'; object-src 'none'",
            ),
        ),
        (header::HeaderName::from_static("cross-origin-opener-policy"), HeaderValue::from_static("same-origin")),
        (header::HeaderName::from_static("cross-origin-embedder-policy"), HeaderValue::from_static("require-corp")),
        (header::HeaderName::from_static("cross-origin-resource-policy"), HeaderValue::from_static("same-origin")),
    ]
}

// ---------------------------------------------------------------------------
// Documents by id under one root
// ---------------------------------------------------------------------------

/// The one directory the HTTP API may read documents from.
///
/// The API never takes a path. A document is addressed by *id*, the id is
/// validated before it is allowed anywhere near the file system, and the
/// resolved path is checked against the canonical root afterwards as well —
/// belt and braces, because a symlink inside the root would otherwise escape
/// it (docs/security.md, "Filesystem access").
#[derive(Clone, Debug)]
pub struct DocumentRoot {
    root: PathBuf,
}

impl DocumentRoot {
    /// `~/Documents/cadpole` unless told otherwise.
    pub fn new(explicit: Option<PathBuf>) -> DocumentRoot {
        let root = explicit.unwrap_or_else(|| {
            std::env::var("HOME")
                .map(|h| PathBuf::from(h).join("Documents").join("cadpole"))
                .unwrap_or_else(|_| PathBuf::from("."))
        });
        DocumentRoot { root }
    }

    pub fn path(&self) -> &Path {
        &self.root
    }

    /// True for an id the file system may see: hex, digits, dashes and
    /// underscores only, 1..=64 characters. A UUID passes; `..`, `/`, a
    /// percent escape, a NUL and an absolute path do not.
    pub fn valid_id(id: &str) -> bool {
        !id.is_empty()
            && id.len() <= 64
            && id.chars().all(|c| c.is_ascii_alphanumeric() || c == '-' || c == '_')
    }

    /// The file `id` names, or an error. Never touches the disk for an invalid
    /// id, so a traversal attempt is refused before it can be a `stat`.
    pub fn resolve(&self, id: &str) -> anyhow::Result<PathBuf> {
        if !Self::valid_id(id) {
            anyhow::bail!("{id:?} is not a document id: ids are letters, digits, '-' and '_', at most 64 characters");
        }
        let candidate = self.root.join(format!("{id}.{DOCUMENT_EXT}"));
        // The id cannot contain a separator, so this cannot be anything but a
        // direct child; assert it rather than trust it.
        if candidate.components().any(|c| matches!(c, Component::ParentDir)) {
            anyhow::bail!("refusing a path with '..' in it");
        }
        // `symlink_metadata`, not `exists()`: `exists()` follows a link and is
        // false for a dangling one, which used to hand back the unchecked path
        // for the caller to write through. Anything at all under the name --
        // a file, or a link whether or not its target exists -- is
        // canonicalised and must land inside the root; a link that resolves
        // to nothing cannot be shown to, so it is refused.
        if std::fs::symlink_metadata(&candidate).is_ok() {
            let real = std::fs::canonicalize(&candidate)
                .map_err(|e| anyhow::anyhow!("{id:?} is a link that does not resolve inside the document root: {e}"))?;
            let root = std::fs::canonicalize(&self.root).unwrap_or_else(|_| self.root.clone());
            if !real.starts_with(&root) {
                anyhow::bail!("{id:?} resolves outside the document root");
            }
            return Ok(real);
        }
        Ok(candidate)
    }

    /// Every packed document file directly under the root, as (id, file name).
    pub fn list(&self) -> Vec<(String, u64)> {
        let mut out = Vec::new();
        let Ok(entries) = std::fs::read_dir(&self.root) else { return out };
        for e in entries.flatten() {
            let p = e.path();
            if p.extension().and_then(|x| x.to_str()) != Some(DOCUMENT_EXT) {
                continue;
            }
            let Some(id) = p.file_stem().and_then(|s| s.to_str()) else { continue };
            if !Self::valid_id(id) {
                continue;
            }
            let size = e.metadata().map(|m| m.len()).unwrap_or(0);
            out.push((id.to_owned(), size));
        }
        out.sort();
        out
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn a_token_is_unguessable_and_url_safe() {
        let a = generate_token();
        let b = generate_token();
        assert_ne!(a, b);
        assert!(a.len() >= 40, "32 bytes of base64url: {}", a.len());
        assert!(a.chars().all(|c| c.is_ascii_alphanumeric() || c == '-' || c == '_'), "{a}");
    }

    #[test]
    fn base64url_matches_the_known_vectors() {
        assert_eq!(base64url(b""), "");
        assert_eq!(base64url(b"f"), "Zg");
        assert_eq!(base64url(b"fo"), "Zm8");
        assert_eq!(base64url(b"foo"), "Zm9v");
        assert_eq!(base64url(b"foob"), "Zm9vYg");
        assert_eq!(base64url(b"fooba"), "Zm9vYmE");
        assert_eq!(base64url(b"foobar"), "Zm9vYmFy");
        assert_eq!(base64url(&[0xfb, 0xff, 0xfe]), "-__-", "'-' and '_', never '+' or '/'");
    }

    #[test]
    fn the_token_comparison_is_constant_time_and_correct() {
        assert!(constant_time_eq("abc", "abc"));
        assert!(!constant_time_eq("abc", "abd"));
        assert!(!constant_time_eq("abc", "abcd"));
        assert!(!constant_time_eq("", "a"));
        assert!(constant_time_eq("", ""));
    }

    #[test]
    fn a_token_file_is_private_and_so_is_its_directory() {
        let dir = std::env::temp_dir().join(format!("cadpole-token-test-{}", std::process::id()));
        let _ = std::fs::remove_dir_all(&dir);
        let path = dir.join("session-token");
        write_private(&path, b"a-token").unwrap();
        assert_eq!(std::fs::read_to_string(&path).unwrap().trim(), "a-token");
        #[cfg(unix)]
        {
            use std::os::unix::fs::PermissionsExt as _;
            let mode = std::fs::metadata(&path).unwrap().permissions().mode() & 0o777;
            assert_eq!(mode, 0o600, "the token must not be world readable");
            let dir_mode = std::fs::metadata(&dir).unwrap().permissions().mode() & 0o777;
            assert_eq!(dir_mode, 0o700, "the directory must not be world traversable either");
        }
        // Rewriting over an existing, wide-open file tightens it rather than
        // keeping the old mode.
        #[cfg(unix)]
        {
            use std::os::unix::fs::PermissionsExt as _;
            std::fs::set_permissions(&path, std::fs::Permissions::from_mode(0o666)).unwrap();
            write_private(&path, b"another-token").unwrap();
            let mode = std::fs::metadata(&path).unwrap().permissions().mode() & 0o777;
            assert_eq!(mode, 0o600);
        }
        let _ = std::fs::remove_dir_all(&dir);
    }

    /// Every start mints new tokens. A link from a previous run must
    /// not open this one.
    #[test]
    fn every_run_mints_new_tokens_and_stores_them_privately() {
        let dir = std::env::temp_dir().join(format!("cadpole-mint-test-{}", std::process::id()));
        let _ = std::fs::remove_dir_all(&dir);

        let first = mint_tokens_in(Some(&dir), None);
        assert!(first.warning.is_none(), "{:?}", first.warning);
        assert_ne!(first.owner, first.listener, "the roles must not share a token");
        assert_eq!(std::fs::read_to_string(first.owner_file.as_ref().unwrap()).unwrap().trim(), first.owner);
        assert_eq!(std::fs::read_to_string(first.listener_file.as_ref().unwrap()).unwrap().trim(), first.listener);
        assert!(first.owner_file.as_ref().unwrap().ends_with("session-token"));
        assert!(first.listener_file.as_ref().unwrap().ends_with("listener-token"));

        let second = mint_tokens_in(Some(&dir), None);
        assert_ne!(second.owner, first.owner, "a restart must invalidate the previous run's link");
        assert_ne!(second.listener, first.listener);
        // ...and the file now holds the new one, so a client that re-reads it
        // recovers without the user pasting anything.
        assert_eq!(std::fs::read_to_string(second.owner_file.as_ref().unwrap()).unwrap().trim(), second.owner);

        // The default path is the runtime directory, which is where every
        // client looks.
        assert!(token_path().unwrap().ends_with("session-token"));
        assert!(listener_token_path().unwrap().ends_with("listener-token"));
        let _ = std::fs::remove_dir_all(&dir);
    }

    #[test]
    fn a_token_carries_its_role_and_an_unknown_token_carries_none() {
        use cadpole_ops::capability::Role;
        let set = TokenSet::new("owner-token", "listener-token");
        assert_eq!(set.role_of("owner-token"), Some(Role::Owner));
        assert_eq!(set.role_of("listener-token"), Some(Role::Listener));
        assert_eq!(set.role_of("owner-toke"), None);
        assert_eq!(set.role_of(""), None);
        assert_eq!(set.role_of("listener-tokenn"), None);
        assert!(!set.is_open());
        assert!(TokenSet::open().is_open());
        assert_eq!(TokenSet::owner_only("t").role_of("t"), Some(Role::Owner));
        // A listener token is not an owner token by any spelling.
        assert_ne!(set.role_of("listener-token"), Some(Role::Owner));
    }

    #[test]
    fn a_cross_site_write_is_refused_and_a_cli_write_is_not() {
        let mut h = HeaderMap::new();
        h.insert(header::HOST, HeaderValue::from_static("127.0.0.1:7331"));
        // A CLI: no Origin, no Sec-Fetch-Site.
        assert_eq!(check_origin(&Method::POST, &h, &[]), OriginVerdict::Allow);

        // The UI itself.
        let mut same = h.clone();
        same.insert(header::ORIGIN, HeaderValue::from_static("http://127.0.0.1:7331"));
        same.insert("sec-fetch-site", HeaderValue::from_static("same-origin"));
        assert_eq!(check_origin(&Method::POST, &same, &[]), OriginVerdict::Allow);

        // A page on the internet that the user happens to have open.
        let mut evil = h.clone();
        evil.insert(header::ORIGIN, HeaderValue::from_static("https://evil.example"));
        evil.insert("sec-fetch-site", HeaderValue::from_static("cross-site"));
        assert!(matches!(check_origin(&Method::POST, &evil, &[]), OriginVerdict::Refuse(_)));

        // ...even if it strips Sec-Fetch-Site (it cannot, but assume it did).
        let mut evil2 = h.clone();
        evil2.insert(header::ORIGIN, HeaderValue::from_static("https://evil.example"));
        assert!(matches!(check_origin(&Method::POST, &evil2, &[]), OriginVerdict::Refuse(_)));

        // The dev server, when explicitly allowed.
        let allowed = vec!["http://localhost:5173".to_owned()];
        let mut dev = h.clone();
        dev.insert(header::ORIGIN, HeaderValue::from_static("http://localhost:5173"));
        dev.insert("sec-fetch-site", HeaderValue::from_static("cross-site"));
        assert_eq!(check_origin(&Method::POST, &dev, &allowed), OriginVerdict::Allow);

        // Reads are refused cross-site too: the event stream is a GET,
        // and a `<img src=...>` or a cross-origin `fetch` is still a browser
        // being pointed at this server by a page from somewhere else.
        assert!(matches!(check_origin(&Method::GET, &evil, &[]), OriginVerdict::Refuse(_)));
        assert_eq!(check_origin(&Method::GET, &same, &[]), OriginVerdict::Allow);
        // A typed URL ("none") and a CLI (no headers at all) still read.
        let mut typed = h.clone();
        typed.insert("sec-fetch-site", HeaderValue::from_static("none"));
        assert_eq!(check_origin(&Method::GET, &typed, &[]), OriginVerdict::Allow);
        assert_eq!(check_origin(&Method::GET, &h, &[]), OriginVerdict::Allow);
    }

    #[test]
    fn a_form_post_content_type_is_refused() {
        let mut h = HeaderMap::new();
        h.insert(header::CONTENT_TYPE, HeaderValue::from_static("application/x-www-form-urlencoded"));
        assert!(!check_content_type(&Method::POST, &h));
        h.insert(header::CONTENT_TYPE, HeaderValue::from_static("text/plain"));
        assert!(!check_content_type(&Method::POST, &h));
        h.insert(header::CONTENT_TYPE, HeaderValue::from_static("application/json; charset=utf-8"));
        assert!(check_content_type(&Method::POST, &h));
        assert!(check_content_type(&Method::GET, &h));
    }

    #[test]
    fn document_ids_are_validated_before_the_file_system_sees_them() {
        let root = DocumentRoot::new(Some(PathBuf::from("/tmp/cadpole-docs")));
        for bad in [
            "../../etc/passwd",
            "..",
            ".",
            "/etc/passwd",
            "a/b",
            "a\\b",
            "%2e%2e%2fetc",
            "a\0b",
            "",
            &"x".repeat(65),
        ] {
            assert!(root.resolve(bad).is_err(), "{bad:?} must be refused");
            assert!(!DocumentRoot::valid_id(bad), "{bad:?} must not be a valid id");
        }
        let ok = root.resolve("3f2b9c1a-0000-4000-8000-000000000001").unwrap();
        assert_eq!(ok, PathBuf::from(format!("/tmp/cadpole-docs/3f2b9c1a-0000-4000-8000-000000000001.{DOCUMENT_EXT}")));
        assert!(DocumentRoot::valid_id("main_2"));
    }

    /// A link planted in the root cannot lead out of it, dangling or not.
    ///
    /// `resolve` used to check only `if candidate.exists()`, which follows the
    /// link and is false when its target is missing: the unchecked path came
    /// back, and Save As wrote through it, creating the target outside the
    /// root.
    #[cfg(unix)]
    #[test]
    fn a_symlink_in_the_root_cannot_lead_out_of_it() {
        let base = std::env::temp_dir().join(format!("cadpole-resolve-link-{}", std::process::id()));
        let _ = std::fs::remove_dir_all(&base);
        let (dir, outside) = (base.join("root"), base.join("outside"));
        std::fs::create_dir_all(&dir).unwrap();
        std::fs::create_dir_all(&outside).unwrap();
        let root = DocumentRoot::new(Some(dir.clone()));

        std::os::unix::fs::symlink(outside.join("missing.txt"), dir.join(format!("dangle.{DOCUMENT_EXT}"))).unwrap();
        assert!(root.resolve("dangle").is_err(), "a dangling link is refused");

        std::fs::write(outside.join("there.txt"), b"x").unwrap();
        std::os::unix::fs::symlink(outside.join("there.txt"), dir.join(format!("there.{DOCUMENT_EXT}"))).unwrap();
        assert!(root.resolve("there").is_err(), "a link to an existing file outside is refused");

        std::fs::write(dir.join(format!("real.{DOCUMENT_EXT}")), b"x").unwrap();
        std::os::unix::fs::symlink(dir.join(format!("real.{DOCUMENT_EXT}")), dir.join(format!("alias.{DOCUMENT_EXT}"))).unwrap();
        assert!(root.resolve("alias").is_ok(), "a link that stays inside is fine");
        assert!(root.resolve("absent").is_ok(), "a name with nothing under it is a new document");

        // And the write path never follows a link it is handed.
        write_replacing(&dir.join(format!("dangle.{DOCUMENT_EXT}")), b"doc").unwrap();
        assert!(!outside.join("missing.txt").exists(), "nothing was created outside the root");

        // A replaced file keeps its mode: a private document stays private.
        use std::os::unix::fs::PermissionsExt;
        let private = dir.join(format!("private.{DOCUMENT_EXT}"));
        std::fs::write(&private, b"old").unwrap();
        std::fs::set_permissions(&private, std::fs::Permissions::from_mode(0o600)).unwrap();
        write_replacing(&private, b"new").unwrap();
        assert_eq!(std::fs::read(&private).unwrap(), b"new");
        assert_eq!(std::fs::metadata(&private).unwrap().permissions().mode() & 0o777, 0o600);
        let _ = std::fs::remove_dir_all(&base);
    }

    /// Every header docs/security.md promises, by name, and the whole set.
    ///
    /// By name, not by array index: an index-based check keeps passing when a
    /// header is removed and the CSP happens to stay at its index. The
    /// length is asserted too: a header added here has to be named here.
    #[test]
    fn the_security_headers_are_all_present() {
        let headers = security_headers();
        let names: Vec<&str> = headers.iter().map(|(n, _)| n.as_str()).collect();
        for expected in [
            "x-content-type-options",
            "referrer-policy",
            "content-security-policy",
            "cross-origin-opener-policy",
            "cross-origin-embedder-policy",
            "cross-origin-resource-policy",
        ] {
            assert!(names.contains(&expected), "missing {expected}: {names:?}");
        }
        assert_eq!(names.len(), 6, "a header was added without being named in this test: {names:?}");

        let value = |name: &str| -> &str {
            headers
                .iter()
                .find(|(n, _)| n.as_str() == name)
                .map(|(_, v)| v.to_str().unwrap())
                .unwrap_or_else(|| panic!("missing {name}"))
        };
        let csp = value("content-security-policy");
        assert!(csp.contains("default-src 'self'"), "{csp}");
        assert!(csp.contains("frame-ancestors 'none'"), "{csp}");
        assert!(csp.contains("object-src 'none'"), "{csp}");
        assert!(csp.contains("base-uri 'none'"), "{csp}");
        assert_eq!(value("x-content-type-options"), "nosniff");
        assert_eq!(value("referrer-policy"), "no-referrer");
        assert_eq!(value("cross-origin-opener-policy"), "same-origin");
        assert_eq!(value("cross-origin-embedder-policy"), "require-corp");
        assert_eq!(value("cross-origin-resource-policy"), "same-origin");
    }
}
