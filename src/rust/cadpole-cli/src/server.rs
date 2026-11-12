//! `cadpole serve`: the session API and static web UI for one document.
//! Protocol: docs/architecture.md and docs/design.md. Security: docs/security.md
//! (the rules themselves live in `security.rs`, which is where they are
//! tested).

use cadpole_engine::cadpole_proto::file_types::DOCUMENT_EXT;
use std::net::{IpAddr, SocketAddr};
use std::path::{Path, PathBuf};
use std::sync::atomic::{AtomicUsize, Ordering};
use std::sync::{Arc, Mutex};
use std::time::Duration;

use axum::body::Bytes;
use axum::extract::{Query, State};
use axum::http::{Method, StatusCode, header};
use axum::response::sse::{Event as SseEvent, KeepAlive, Sse};
use axum::response::{IntoResponse, Response};
use axum::routing::{get, post};
use axum::{Json, Router};
use clap::{Args, ValueEnum};
use cadpole_engine::cadpole_proto::prost::Message;
use cadpole_engine::cadpole_proto::v1 as pb;
use cadpole_engine::view::{FingerprintView, RefView};
use serde::{Deserialize, Serialize};
use tokio::sync::broadcast;
use tokio_stream::StreamExt;
use tokio_stream::wrappers::BroadcastStream;

use crate::connect;
use crate::reply_stream;
use crate::security::{self, DocumentRoot, OriginVerdict};

/// Concurrent render/sweep jobs, and how many may wait behind them
/// (docs/security.md, "Render and sweep limits").
const RENDER_QUEUE: usize = 16;

/// Operations the restricted listener token may have running on blocking
/// threads at once. Its other calls wait (asynchronously, holding no thread)
/// for one of these, so however hard a listener floods, it cannot fill
/// tokio's blocking pool and stall the owner's operations behind its own.
const LISTENER_OP_SLOTS: usize = 8;

/// Everything `cadpole serve` says to the terminal goes through here.
///
/// One funnel, so a test can hold *all* of it and grep it. That is the only
/// way to make "the token is never logged" a claim rather than a hope:
/// `the_token_is_printed_once_and_never_logged` turns capture on, starts a
/// real server, drives good and bad requests through it and asserts the token
/// occurs exactly once in everything the process said — in the `open:` line of
/// the startup banner, which is deliberate, and nowhere else.
pub mod out {
    use std::sync::{Mutex, OnceLock};

    fn slot() -> &'static Mutex<Option<Vec<String>>> {
        static CAPTURE: OnceLock<Mutex<Option<Vec<String>>>> = OnceLock::new();
        CAPTURE.get_or_init(|| Mutex::new(None))
    }

    /// One line of server output: to the capture buffer if a test is holding
    /// one, otherwise to stderr.
    pub fn line(text: String) {
        {
            let mut g = slot().lock().unwrap();
            if let Some(buf) = g.as_mut() {
                buf.push(text);
                return;
            }
        }
        eprintln!("{text}");
    }

    /// Start collecting instead of printing.
    #[cfg(test)]
    pub fn start_capture() {
        *slot().lock().unwrap() = Some(Vec::new());
    }

    /// Stop collecting and return everything said since [`start_capture`].
    #[cfg(test)]
    pub fn take_capture() -> Vec<String> {
        slot().lock().unwrap().take().unwrap_or_default()
    }
}

/// `eprintln!` for the server: routed through [`out`] so it is greppable.
macro_rules! note {
    ($($arg:tt)*) => { crate::server::out::line(format!($($arg)*)) };
}

#[derive(Copy, Clone, ValueEnum, Debug, PartialEq, Eq)]
pub enum Listen {
    /// 127.0.0.1 only (the default: nothing else can reach it).
    Loopback,
    /// The Tailscale interface (100.64.0.0/10) if present. Opt in explicitly.
    Tailscale,
    /// All interfaces. Only behind a firewall, and never without a token.
    All,
}

#[derive(Args, Clone)]
pub struct ServeArgs {
    /// A `.star` script or a packed document. Omit for an empty document.
    pub input: Option<PathBuf>,
    #[arg(long, default_value_t = 7331)]
    pub port: u16,
    /// Which interface to bind. Loopback unless you ask for more.
    #[arg(long, value_enum, default_value_t = Listen::Loopback)]
    pub listen: Listen,
    /// Serve no TCP port at all: no browser, agents only (over the socket).
    #[arg(long, conflicts_with_all = ["agent_tcp", "no_socket"])]
    pub no_tcp: bool,
    /// Also accept the agent tokens (the ones `cadpole mcp`, `cadpole op`,
    /// `cadpole listen` and the listener read) on the TCP port, for a client
    /// on another machine or one that cannot use a Unix socket. Off by
    /// default: agents use the socket, and the port is the browser's.
    #[arg(long)]
    pub agent_tcp: bool,
    /// Where to put the agent socket (default
    /// `$XDG_RUNTIME_DIR/cadpole/<instance>.sock`). Its directory must be
    /// yours and 0700.
    #[arg(long, value_name = "PATH", conflicts_with = "no_socket")]
    pub socket: Option<PathBuf>,
    /// Serve no agent socket: agents then need `--agent-tcp` and
    /// `--server http://...`.
    #[arg(long)]
    pub no_socket: bool,
    /// Directory with the built web UI (default: the bundled one).
    #[arg(long)]
    pub web_dir: Option<PathBuf>,
    /// Bearer token clients must present. One is minted for this run and
    /// written to the runtime directory unless it is pinned here; pinning it
    /// on a command line puts it in `ps` and in your shell history, so prefer
    /// the file (docs/security.md).
    #[arg(long, env = "CADPOLE_TOKEN")]
    pub token: Option<String>,
    /// Disable authentication (development only; refused off loopback).
    #[arg(long)]
    pub no_auth: bool,
    /// Where `POST /api/session/save` writes the document.
    #[arg(long)]
    pub save_path: Option<PathBuf>,
    /// The one directory `GET /api/documents/<id>` may read from
    /// (default `~/Documents/cadpole`). The HTTP API never takes a path.
    #[arg(long)]
    pub root: Option<PathBuf>,
    /// An extra browser origin allowed to make state-changing requests, e.g.
    /// the web dev server `http://localhost:5173`. Repeatable.
    #[arg(long, value_name = "URL")]
    pub allow_origin: Vec<String>,
    /// Wall-clock budget for one script evaluation, in milliseconds
    /// (0 = no limit).
    #[arg(long, default_value_t = 20_000)]
    pub eval_deadline_ms: u64,
    /// Wall-clock budget for one render or sweep request, in milliseconds.
    /// The caller is released when it passes; see docs/security.md.
    #[arg(long, default_value_t = 60_000)]
    pub render_budget_ms: u64,
    /// Concurrent render jobs (0 = `max(1, cores / 4)`, capped at 2).
    #[arg(long, default_value_t = 0)]
    pub render_jobs: usize,
    /// Largest request body, in bytes.
    #[arg(long, default_value_t = security::MAX_BODY_BYTES)]
    pub max_body: usize,
    /// Optimisation solves run as jobs at once: a solve too long for
    /// one evaluation, or one written `solve(job=True)`, runs in its own
    /// `cadpole` process and the document is evaluated again when it lands. 0
    /// is `max(1, min(4, cores / 4))`.
    #[arg(long, default_value_t = 0)]
    pub solve_jobs: usize,
    /// Run no solve jobs: a solve too long for the evaluation stays pending.
    #[arg(long)]
    pub no_solve_jobs: bool,
    /// A solve job's time limit, in seconds, for solves that give no
    /// `time_limit=` of their own (0 = none).
    #[arg(long, default_value_t = 3600)]
    pub job_time_limit: u64,
    /// A solve job's memory (address space) limit, in MB (0 = none).
    #[arg(long, default_value_t = 8192)]
    pub job_mem_mb: u64,
    /// The longest a solve runs inside an evaluation before it becomes a
    /// job, in milliseconds; never more than half of what the evaluation has
    /// left (`--eval-deadline-ms`).
    #[arg(long, default_value_t = 10_000)]
    pub solve_inline_ms: u64,
}

#[derive(Serialize, Deserialize, Clone, Debug, Default)]
pub struct Selection {
    pub selected: Vec<RefView>,
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub hovered: Option<RefView>,
    #[serde(default)]
    pub note: String,
}

/// A viewport screenshot (or other blob) attached to a message.
#[derive(Serialize, Deserialize, Clone, Debug)]
pub struct Attachment {
    pub kind: String,
    #[serde(default, skip_serializing_if = "String::is_empty")]
    pub png_base64: String,
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub camera: Option<serde_json::Value>,
    #[serde(default)]
    pub annotated: bool,
}

#[derive(Serialize, Deserialize, Clone, Debug)]
pub struct ChatMessage {
    pub seq: u64,
    pub from: String,
    pub text: String,
    pub at: String,
    #[serde(default, skip_serializing_if = "Vec::is_empty")]
    pub attachments: Vec<Attachment>,
    /// Empty for a message posted whole; "streaming", then "done" or
    /// "failed", for one posted with `StreamReply` (reply_stream.rs).
    #[serde(default, skip_serializing_if = "String::is_empty")]
    pub state: String,
    /// What the assistant is doing right now, while the message is streaming.
    #[serde(default, skip_serializing_if = "String::is_empty")]
    pub activity: String,
}

#[derive(Serialize, Clone, Debug)]
pub struct Event {
    pub seq: u64,
    pub at: String,
    pub kind: String,
    pub data: serde_json::Value,
    /// Which client caused it, from `X-Cadpole-Client`. Empty for the server
    /// itself. A client skips its own writes.
    #[serde(default, skip_serializing_if = "String::is_empty")]
    pub by: String,
}

#[derive(Serialize, Clone, Debug)]
pub struct DocumentView {
    pub id: String,
    pub name: String,
    pub display_unit: String,
    pub scripts: Vec<cadpole_engine::view::ScriptIn>,
    pub param_overrides: serde_json::Map<String, serde_json::Value>,
    /// The assets' names, never their bytes: what tells the browser a
    /// document can only be evaluated here.
    pub asset_names: Vec<String>,
}

#[derive(Serialize, Clone, Debug)]
pub struct SessionView {
    pub id: String,
    pub revision: u64,
    pub document: DocumentView,
    pub selection: Selection,
    pub messages: Vec<ChatMessage>,
    pub clients: usize,
    pub last_event_seq: u64,
    /// Whether UI events are forwarded to a connected assistant (the
    /// "Listen" toggle in the UI).
    pub assistant_listening: bool,
    /// The current viewport camera as last reported by a UI, if any.
    #[serde(skip_serializing_if = "Option::is_none")]
    pub camera: Option<serde_json::Value>,
    /// How many front-end crashes this session has recorded. Anything
    /// but 0 is worth reading back with
    /// `WaitEvents(after: 0, kinds: ["client_failure"])` before trusting what
    /// the user is looking at.
    #[serde(default)]
    pub client_failures: u64,
    /// Fingerprint of the web bundle being served, so a page can tell on load
    /// whether it is running the current one. Empty when none is served.
    #[serde(default)]
    pub web_build: String,
}

pub struct SessionState {
    pub id: String,
    pub revision: u64,
    pub document: pb::Document,
    pub selection: Selection,
    pub messages: Vec<ChatMessage>,
    pub events: Vec<Event>,
    pub clients: usize,
    pub save_path: Option<PathBuf>,
    pub assistant_listening: bool,
    pub camera: Option<serde_json::Value>,
    /// Per-client event cursors, server side. A client that
    /// restarts resumes from here instead of losing what it missed.
    pub cursors: std::collections::BTreeMap<String, u64>,
    /// How many front-end failure reports this session has taken, and how
    /// much budget is left.
    pub failures: FailureLimits,
    /// Fingerprint of the served web bundle, kept current by the build watcher
    /// so `session_view` can report it without stat-ing the directory.
    pub web_build: String,
    /// The messages being written a piece at a time (`StreamReply`).
    pub streams: reply_stream::ReplyStreams,
}

/// The rate and volume limits on `ReportClientFailure`, per session.
///
/// A panic in a render loop fires thousands of times a second, and each firing
/// would otherwise be one event in the log every attached assistant reads. Two
/// caps, because they answer different questions: a **token bucket** so a
/// burst is recorded and a storm is not, and a **session total** because a
/// front end that has failed two hundred times is not going to be diagnosed by
/// the two hundred and first.
///
/// Neither cap drops anything silently. A refusal comes back to the caller
/// with the reason, the remaining budget and the running drop count, and the
/// first refusal in each window is itself emitted as a `client_failure` event
/// so the log shows reports being lost rather than appearing to show that the
/// crashing stopped.
#[derive(Debug)]
pub struct FailureLimits {
    /// Reports recorded in this session.
    pub accepted: u32,
    /// Reports refused in this session.
    pub dropped: u64,
    /// Whole tokens left in the bucket, and when it was last topped up.
    tokens: u32,
    refilled: std::time::Instant,
    /// When a throttle notice was last emitted, so the notice that reports are
    /// being dropped cannot itself become the flood.
    noticed: Option<std::time::Instant>,
}

impl Default for FailureLimits {
    fn default() -> Self {
        FailureLimits {
            accepted: 0,
            dropped: 0,
            tokens: security::FAILURE_REPORT_BURST,
            refilled: std::time::Instant::now(),
            noticed: None,
        }
    }
}

impl FailureLimits {
    /// Take one report's budget, or say why it cannot be taken.
    fn admit(&mut self, now: std::time::Instant) -> Result<(), String> {
        if self.accepted >= security::MAX_FAILURE_REPORTS_PER_SESSION {
            return Err(format!(
                "this session has already recorded {} failure reports, which is the per-session cap; \
                 restart `cadpole serve` to clear it",
                security::MAX_FAILURE_REPORTS_PER_SESSION
            ));
        }
        let elapsed = now.saturating_duration_since(self.refilled).as_millis() as u64;
        let earned = (elapsed / security::FAILURE_REPORT_REFILL_MS) as u32;
        if earned > 0 {
            self.tokens = (self.tokens + earned).min(security::FAILURE_REPORT_BURST);
            self.refilled += Duration::from_millis(u64::from(earned) * security::FAILURE_REPORT_REFILL_MS);
        }
        if self.tokens == 0 {
            return Err(format!(
                "failure reports are rate limited to {} at once and one every {} ms after that; \
                 coalesce repeats and send the count in `repeat`",
                security::FAILURE_REPORT_BURST,
                security::FAILURE_REPORT_REFILL_MS
            ));
        }
        self.tokens -= 1;
        Ok(())
    }

    /// How many more reports this session will record.
    fn remaining(&self) -> u32 {
        security::MAX_FAILURE_REPORTS_PER_SESSION.saturating_sub(self.accepted)
    }

    /// True when this refusal should also be announced in the event log. At
    /// most one notice per window, so the throttling is visible without the
    /// notices becoming the thing that needs throttling.
    fn should_announce(&mut self, now: std::time::Instant) -> bool {
        let due = match self.noticed {
            None => true,
            Some(t) => now.saturating_duration_since(t) >= Duration::from_millis(security::FAILURE_THROTTLE_NOTICE_MS),
        };
        if due {
            self.noticed = Some(now);
        }
        due
    }
}

pub struct App {
    pub state: Mutex<SessionState>,
    pub events: broadcast::Sender<Event>,
    /// What a message in progress gained, for the pages showing it.
    ///
    /// Deliberately not the event log. The log is a bounded window that every
    /// reader resumes from by sequence number, and every entry wakes every
    /// long poll; a reply written over a minute would put hundreds of entries
    /// in it, push real events out of the window and wake `cadpole listen`
    /// and each `WaitEvents` caller for text nobody acts on. A frame here has
    /// no sequence number, is kept nowhere and is sent only to an event
    /// stream that asked for it (`/api/events?live=1`). Nothing is owed to a
    /// reader that missed one: the frame says where its text starts, a page
    /// that sees a gap reads the session again, and the finished message is
    /// one ordinary `message` event in the log.
    pub live: broadcast::Sender<LiveFrame>,
    /// The tokens this run accepts and the role each one carries. Empty means
    /// `--no-auth` (loopback only).
    pub tokens: security::TokenSet,
    pub allowed_origins: Vec<String>,
    pub root: DocumentRoot,
    /// How many renders may run at once, and how many may wait.
    pub render_slots: tokio::sync::Semaphore,
    pub render_waiting: AtomicUsize,
    pub render_budget: Duration,
    /// [`LISTENER_OP_SLOTS`]: the listener's share of the blocking pool.
    pub listener_slots: Arc<tokio::sync::Semaphore>,
    /// Largest request body (`--max-body`). The one number every route
    /// checks: the router's `DefaultBodyLimit` and the handlers that read raw
    /// bytes alike.
    pub max_body: usize,
    /// The kernel-op memo: one per session, for its lifetime, read
    /// and filled by every evaluation of its document -- the UI's, an
    /// assistant's, an edit's feedback. Never persisted, never shared with
    /// another engine.
    pub memo: Arc<cadpole_engine::cadpole_core::memo::Memo>,
}

type Shared = Arc<App>;

/// One step of a message in progress, as sent to the pages following it: the
/// message, where the appended text starts in bytes, the text, and the
/// activity line as it now is.
#[derive(Serialize, Clone, Debug)]
pub struct LiveFrame {
    pub seq: u64,
    pub from: String,
    pub at: String,
    pub state: String,
    pub offset: usize,
    pub length: usize,
    pub append: String,
    pub activity: String,
}

fn now() -> String {
    chrono::Utc::now().to_rfc3339_opts(chrono::SecondsFormat::Millis, true)
}

fn random_id(bytes: usize) -> String {
    use rand::RngCore;
    let mut b = vec![0u8; bytes];
    rand::rng().fill_bytes(&mut b);
    b.iter().map(|x| format!("{x:02x}")).collect()
}

impl SessionState {
    fn view(&self, last_event_seq: u64) -> SessionView {
        SessionView {
            id: self.id.clone(),
            revision: self.revision,
            document: DocumentView {
                id: self.document.id.clone(),
                name: self.document.name.clone(),
                display_unit: format!("{:?}", self.document.display_unit()).to_lowercase(),
                scripts: self
                    .document
                    .scripts
                    .iter()
                    .map(|s| cadpole_engine::view::ScriptIn { path: s.path.clone(), source: s.source.clone() })
                    .collect(),
                param_overrides: self
                    .document
                    .param_overrides
                    .iter()
                    .filter_map(|(k, v)| cadpole_engine::cadpole_core::ParamValue::from_proto(v).map(|v| (k.clone(), cadpole_engine::view::param_value_json(&v))))
                    .collect(),
                asset_names: self.document.assets.iter().map(|a| a.name.clone()).collect(),
            },
            selection: self.selection.clone(),
            messages: self.messages.iter().map(|m| ChatMessage { attachments: m.attachments.iter().map(|a| Attachment { png_base64: String::new(), ..a.clone() }).collect(), ..m.clone() }).collect(),
            clients: self.clients,
            last_event_seq,
            assistant_listening: self.assistant_listening,
            camera: self.camera.clone(),
            client_failures: u64::from(self.failures.accepted),
            web_build: self.web_build.clone(),
        }
    }
}

impl App {
    fn emit(&self, st: &mut SessionState, kind: &str, data: serde_json::Value) -> Event {
        self.emit_by(st, kind, data, "")
    }

    /// Append an event, attributed to the client that caused it.
    fn emit_by(&self, st: &mut SessionState, kind: &str, data: serde_json::Value, by: &str) -> Event {
        let seq = st.events.last().map(|e| e.seq).unwrap_or(0) + 1;
        let ev = Event { seq, at: now(), kind: kind.to_owned(), data, by: by.to_owned() };
        st.events.push(ev.clone());
        if st.events.len() > 10_000 {
            st.events.drain(..st.events.len() - 10_000);
        }
        let _ = self.events.send(ev.clone());
        ev
    }

    fn session_view(&self, st: &SessionState) -> SessionView {
        st.view(st.events.last().map(|e| e.seq).unwrap_or(0))
    }

    /// One `message` event for each streamed message that just ended, with
    /// its final text: the only trace a streamed message leaves in the log.
    fn announce_ended(&self, st: &mut SessionState, ended: &[u64], by: &str) {
        for seq in ended {
            if let Some(m) = st.messages.iter().find(|m| m.seq == *seq).cloned() {
                self.emit_by(st, "message", serde_json::to_value(&m).unwrap_or_default(), by);
            }
        }
    }

    /// End the streamed messages whose author went away (reply_stream.rs).
    /// Called once a second by `serve`; the event is the server's own.
    pub fn expire_streams(&self, now: std::time::Instant) -> usize {
        let mut st = self.state.lock().unwrap_or_else(|e| e.into_inner());
        if st.streams.in_progress() == 0 {
            return 0;
        }
        let st = &mut *st;
        let ended = st.streams.expire(&mut st.messages, now);
        self.announce_ended(st, &ended, "");
        ended.len()
    }
}

// ---- auth / origin middleware

/// Everything docs/security.md asks of a request before a handler sees it:
/// the same-origin rules, the JSON content type, the bearer token; and
/// everything it asks of a response afterwards: the security headers.
async fn guard(State(app): State<Shared>, req: axum::extract::Request, next: axum::middleware::Next) -> Response {
    let path = req.uri().path().to_owned();
    // Without a token the Host is the only thing a DNS-rebinding page cannot
    // forge, so it is checked for the page as well as the API.
    if app.tokens.is_open() {
        if let OriginVerdict::Refuse(why) = security::check_host(req.headers(), &app.allowed_origins) {
            let mut response = (StatusCode::FORBIDDEN, why).into_response();
            for (name, value) in security::security_headers() {
                response.headers_mut().insert(name, value);
            }
            return response;
        }
    }
    let mut response = if path.starts_with("/api/") { api_guard(&app, req, next).await } else { next.run(req).await };
    let headers = response.headers_mut();
    for (name, value) in security::security_headers() {
        headers.insert(name, value);
    }
    response
}

async fn api_guard(app: &Shared, req: axum::extract::Request, next: axum::middleware::Next) -> Response {
    let path = req.uri().path().to_owned();
    // A refusal on the Connect route has to *be* a Connect error, or a
    // generated client sees a parse failure where it should see a code. The
    // rules being enforced are identical; only the rendering differs.
    let wire = connect::method_of(&path).map(|_| connect::wire_of_headers(req.headers()));
    let refuse = |code: connect::Code, why: String| -> Response {
        match wire {
            Some(w) => connect::error_response(w, &connect::Failure::new(code, why)),
            None => (code.http_status(), why).into_response(),
        }
    };
    if let OriginVerdict::Refuse(why) = security::check_origin(req.method(), req.headers(), &app.allowed_origins) {
        return refuse(connect::Code::PermissionDenied, why.to_owned());
    }
    // On the Connect route the content type *is* the protocol negotiation, and
    // `post_connect` has the stricter allowlist: only the protocol's own media
    // types, each with its own answer, and 415 with a code for anything else.
    // Checking here as well would replace that answer with a sentence about
    // JSON, and would not refuse anything the handler does not already refuse.
    if wire.is_none() && !security::check_content_type(req.method(), req.headers()) {
        return (
            StatusCode::UNSUPPORTED_MEDIA_TYPE,
            "JSON endpoints require Content-Type: application/json",
        )
            .into_response();
    }
    // The token travels in one place and one place only: the `Authorization`
    // header. There is no `?token=` escape hatch for the event stream any
    // more -- a query string lands in shell history, in a `Referer`, in a
    // proxy log and in the browser's own history, and `EventSource` was the
    // only reason it existed. The web client reads the stream with `fetch`
    // instead, which can set headers.
    // The surface the request came in on: set by the router it reached, and
    // the narrower one when somehow absent.
    let surface = req.extensions().get::<security::Surface>().copied().unwrap_or(security::Surface::Browser);
    let role = if app.tokens.is_open() {
        cadpole_ops::capability::Role::Owner
    } else {
        let presented = req.headers().get(header::AUTHORIZATION).and_then(|v| v.to_str().ok()).and_then(|v| v.strip_prefix("Bearer "));
        match presented.map(|t| app.tokens.admit(t, surface)).unwrap_or(security::Admission::Unknown) {
            security::Admission::Admitted(role) => role,
            // A real token, on the wrong surface: refused before the role is
            // looked at, so which surface a token is admitted on never
            // changes what it may do, only where it may ask.
            security::Admission::NotHere(role) => {
                return refuse(connect::Code::PermissionDenied, surface_refusal(role, surface, req.method(), &path));
            }
            // No detail: "wrong" and "missing" are the same answer, and the
            // body never echoes what was presented.
            security::Admission::Unknown => return refuse(connect::Code::Unauthenticated, "missing or wrong bearer token".to_owned()),
        }
    };
    if let Some(why) = capability_refusal(role, req.method(), &path) {
        note_refusal(role, req.method(), &path);
        return refuse(connect::Code::PermissionDenied, why);
    }
    // The handlers that act on the caller's behalf need to know who it is:
    // only the owner's evaluations may start solve jobs.
    let mut req = req;
    req.extensions_mut().insert(role);
    next.run(req).await
}

/// Say on the server's own output that a token was refused something it does
/// not grant: the person running `cadpole serve` is the one who should know
/// that a listener asked to save, or to post as them.
///
/// At most one line a second, with a count of the ones left out, so a client
/// that keeps asking cannot turn the terminal into its log. The path is the
/// caller's text: only its printable ASCII is shown, and no more than 80
/// bytes of it.
fn note_refusal(role: cadpole_ops::capability::Role, method: &Method, path: &str) {
    static LAST: Mutex<Option<(std::time::Instant, u64)>> = Mutex::new(None);
    let now = std::time::Instant::now();
    let mut last = LAST.lock().unwrap_or_else(|e| e.into_inner());
    let skipped = match *last {
        Some((at, skipped)) if now.saturating_duration_since(at) < Duration::from_secs(1) => {
            *last = Some((at, skipped + 1));
            return;
        }
        Some((_, skipped)) => skipped,
        None => 0,
    };
    *last = Some((now, 0));
    let shown: String = path.chars().filter(|c| c.is_ascii_graphic()).take(80).collect();
    let more = if skipped > 0 { format!(" ({skipped} more refusals not shown)") } else { String::new() };
    note!("403: refused the {} token {method} {shown}{more}", role.as_str());
}

/// The role `api_guard` decided for this request. Every `/api/` route is
/// behind the guard, so it is always there; a request that somehow arrives
/// without one is treated as the listener's, the narrower of the two.
fn caller_role(r: Option<axum::Extension<cadpole_ops::capability::Role>>) -> cadpole_ops::capability::Role {
    r.map(|axum::Extension(r)| r).unwrap_or(cadpole_ops::capability::Role::Listener)
}

/// What a restricted token may not reach, decided before any handler runs.
///
/// The listener's whole surface is the operation registry: an operation
/// `cadpole_ops::capability` grants, called over either of the two routes that
/// reach the registry — `POST /api/ops/<Op>` or the Connect route
/// `POST /api/cadpole.v1.CadpoleOps/<Rpc>` — plus `GET /api/ops` to discover them
/// and the read-only `GET /api/session/document` its local Render/Evaluate/
/// Export need to see the document they are working on. Everything else under
/// `/api/` -- the hand-written session routes, the document store, the raw
/// render endpoint -- is refused, so a new route cannot silently widen the
/// listener's privileges by existing.
///
/// **A second transport is a second route shape, which is a second chance to
/// get this wrong.** It is written as one function over both for that reason:
/// the two routes differ in how a method is *named* — `/api/ops` takes any
/// spelling `MethodInfo::matches` accepts, Connect takes the rpc name and
/// nothing else — and agree in every other respect, including that a name
/// nobody answers to is granted to nobody. `spec/capability.qnt` models both.
fn capability_refusal(role: cadpole_ops::capability::Role, method: &Method, path: &str) -> Option<String> {
    use cadpole_ops::capability::Role;
    // Before anything else, and before the owner is waved through: the router
    // decodes percent-escapes before it matches, so `/api/ops/Post%4dessage`
    // *dispatches* as `PostMessage`, while this sees the raw path and resolves
    // nothing. Today that fails closed -- an unresolvable name is granted to
    // no role but the owner -- but the guard and the dispatcher reading
    // different names is a property nobody should have to re-derive the next
    // time a role is added.
    //
    // Decoding here would work and would leave two decoders to agree. Refusing
    // is better: an operation name is letters, digits and underscores, so a
    // percent-escape in one is never a spelling anybody needs, only a way to
    // make the two disagree.
    if path.contains('%') && (path.starts_with("/api/ops/") || path.starts_with(connect::service_prefix())) {
        return Some("an operation name is not percent-encoded; ask for it by name".to_owned());
    }
    if role == Role::Owner {
        return None;
    }
    if method == Method::GET && (path == "/api/ops" || path == "/api/session/document") {
        return None;
    }
    // Resolve the spelling to the rpc name *before* deciding, so neither an
    // alias nor a framing is a way around the policy. An unresolvable name
    // falls through to its own text, which no role but the owner grants.
    let name = match (method == Method::POST, path.strip_prefix("/api/ops/"), connect::method_of(path)) {
        (true, Some(op), _) => cadpole_ops::method(op).map(|m| m.name.clone()).unwrap_or_else(|| op.to_owned()),
        (true, None, Some(rpc)) => connect::lookup(rpc).map(|m| m.name.clone()).unwrap_or_else(|| rpc.to_owned()),
        _ => {
            return Some(format!(
                "a {} token may only call operations: POST /api/ops/<Operation> or POST {}/<Rpc>",
                role.as_str(),
                connect::service_prefix()
            ));
        }
    };
    if role.may(&name) {
        return None;
    }
    let why = cadpole_ops::capability::denied_reason(&name).unwrap_or("it is not in this token's capability list");
    Some(format!("a {} token may not call {name}: {why}", role.as_str()))
}

/// Why a token of this run is refused on this surface. Names the request but
/// never the token.
fn surface_refusal(role: cadpole_ops::capability::Role, surface: security::Surface, method: &Method, path: &str) -> String {
    match surface {
        security::Surface::Browser => format!(
            "this {} agent token is not accepted on the browser (TCP) port ({method} {path}): agent clients \
             connect over the Unix socket (leave out --server), or start `cadpole serve --agent-tcp`",
            role.as_str()
        ),
        security::Surface::Agent => format!("this {} token is not accepted on the agent socket ({method} {path})", role.as_str()),
    }
}

/// The id a writer gives itself, so an event can say who caused it and a
/// client can skip its own writes.
fn client_id(headers: &axum::http::HeaderMap) -> String {
    headers
        .get("x-cadpole-client")
        .and_then(|v| v.to_str().ok())
        .map(|s| s.chars().filter(|c| !c.is_control()).take(64).collect())
        .unwrap_or_default()
}

// ---- handlers

async fn get_session(State(app): State<Shared>) -> Json<SessionView> {
    let st = app.state.lock().unwrap();
    Json(app.session_view(&st))
}

#[derive(Deserialize)]
struct ScriptBody {
    path: String,
    source: String,
    /// Optimistic concurrency: reject if the session moved on.
    ///
    /// Not optional, and not defaulted: a body that left it out would skip the
    /// check and clobber (A writes at revision 3, B still holding 3 omits the
    /// field, and A's work is gone under a 200). Serde refuses a body without
    /// it, exactly as it refuses one without `source`.
    base_revision: u64,
}

async fn put_script(State(app): State<Shared>, headers: axum::http::HeaderMap, Json(body): Json<ScriptBody>) -> Response {
    let by = client_id(&headers);
    let mut st = app.state.lock().unwrap();
    if body.base_revision != st.revision {
        return (StatusCode::CONFLICT, Json(serde_json::json!({"error": "stale base_revision", "revision": st.revision}))).into_response();
    }
    if let Err(e) = check_script(&body.path, &body.source) {
        return (StatusCode::BAD_REQUEST, e.to_string()).into_response();
    }
    if let Some(d) = cadpole_engine::cadpole_script::check_syntax(&body.path, &body.source) {
        let span = d.span.as_ref().map(|s| format!("{}:{}:{}", s.path, s.start_line, s.start_col)).unwrap_or_default();
        return (StatusCode::UNPROCESSABLE_ENTITY, Json(serde_json::json!({"error": "syntax", "message": d.message, "span": span}))).into_response();
    }
    match st.document.scripts.iter_mut().find(|s| s.path == body.path) {
        Some(s) => s.source = body.source.clone(),
        None => st.document.scripts.push(pb::ScriptFile { path: body.path.clone(), source: body.source.clone() }),
    }
    st.revision += 1;
    let rev = st.revision;
    app.emit_by(&mut st, "script_changed", serde_json::json!({"path": body.path, "revision": rev}), &by);
    Json(app.session_view(&st)).into_response()
}

/// A script path is a bare file name, and a script has a size cap
/// (docs/security.md item 12). Both are checked wherever a script is stored.
fn check_script(path: &str, source: &str) -> anyhow::Result<()> {
    if path.is_empty() || path.contains("..") || path.contains('/') || path.contains('\\') {
        anyhow::bail!("script path must be a bare file name inside the document, e.g. \"main.star\"");
    }
    if source.len() > security::MAX_SCRIPT_BYTES {
        anyhow::bail!("script is {} bytes, over the {} byte limit", source.len(), security::MAX_SCRIPT_BYTES);
    }
    Ok(())
}

#[derive(Deserialize)]
struct ParamsBody {
    overrides: serde_json::Map<String, serde_json::Value>,
    /// Names to remove, as `SetParamsRequest.clear`. Optional: the common
    /// case is a write that names only what it is changing.
    #[serde(default)]
    clear: Vec<String>,
}

async fn put_params(State(app): State<Shared>, headers: axum::http::HeaderMap, Json(body): Json<ParamsBody>) -> Response {
    let by = client_id(&headers);
    let mut st = app.state.lock().unwrap();
    // **Replace**, and deliberately not the merge `SetParams` does.
    //
    // The two look like one operation under two names and are not. The rpc is
    // the primitive -- `overrides` plus `clear`, a delta -- and every
    // client-facing API in the tree is a *replace* built on it: `api.ts`'s
    // `putParams` takes the whole desired set and computes the `clear` list
    // for whatever is going away, and `EngineClient.setParams` says "Replace
    // the parameter overrides" in as many words.
    //
    // This route is one of those client-facing APIs, not a second spelling of
    // the rpc, and it is the one CadpoleKit writes through. Making it merge
    // would take away the only way a stateless client has of
    // removing an override, because `RemoteEngine` holds no session to
    // compute a `clear` list from. Nothing in the Apple app removes one
    // today, so that would be a silent contract change with no visible
    // symptom.
    //
    // The answer is not to align the semantics: it is to delete the route
    // once CadpoleKit moves to the rpc, as the web UI has. Until then it is
    // replace, it says so, and `clear` is accepted so a caller can be explicit.
    // The same bounds `SetParams` applies, checked before anything is written
    // so a refusal leaves the document as it was -- which is why the clear
    // below comes after them and not with the comment that explains it.
    for k in body.overrides.keys() {
        if k.len() > security::MAX_PARAM_KEY_BYTES {
            return (StatusCode::BAD_REQUEST, format!("parameter name is {} bytes, over the {} byte limit", k.len(), security::MAX_PARAM_KEY_BYTES))
                .into_response();
        }
    }
    // The route replaces, so what the document will hold is exactly what was
    // sent -- counting the existing entries too would refuse writes that are
    // about to remove them.
    if body.overrides.len() > security::MAX_PARAM_OVERRIDES {
        return (
            StatusCode::BAD_REQUEST,
            format!("this document already holds {} parameter overrides, which is the limit", security::MAX_PARAM_OVERRIDES),
        )
            .into_response();
    }
    st.document.param_overrides.clear();
    for (k, v) in &body.overrides {
        if let Some(pv) = cadpole_engine::view::param_value_from_json(v) {
            st.document.param_overrides.insert(k.clone(), pv.to_proto());
        }
    }
    for k in &body.clear {
        st.document.param_overrides.remove(k);
    }
    st.revision += 1;
    let ov = body.overrides.clone();
    app.emit_by(&mut st, "params_changed", serde_json::json!({"overrides": ov}), &by);
    Json(app.session_view(&st)).into_response()
}

async fn put_selection(State(app): State<Shared>, headers: axum::http::HeaderMap, Json(sel): Json<Selection>) -> Json<SessionView> {
    let by = client_id(&headers);
    let mut st = app.state.lock().unwrap();
    // An event only when the *selected set* or the note
    // changed. Hover PUTs the same resource on every mouse move, and
    // forwarding one notification per face is how an assistant's context is
    // flooded.
    let changed = st.selection.selected != sel.selected || st.selection.note != sel.note;
    st.selection = sel.clone();
    if changed {
        app.emit_by(&mut st, "selection_changed", serde_json::to_value(&sel).unwrap_or_default(), &by);
    }
    Json(app.session_view(&st))
}

#[derive(Deserialize)]
struct MessageBody {
    /// Accepted for compatibility and not believed. See `post_message`.
    #[serde(default)]
    from: String,
    text: String,
    #[serde(default)]
    attachments: Vec<Attachment>,
}

async fn post_message(State(app): State<Shared>, headers: axum::http::HeaderMap, Json(body): Json<MessageBody>) -> Response {
    let by = client_id(&headers);
    if body.text.len() > security::MAX_MESSAGE_BYTES {
        return (StatusCode::PAYLOAD_TOO_LARGE, "message too long").into_response();
    }
    if body.attachments.iter().map(|a| a.png_base64.len()).sum::<usize>() > security::MAX_ATTACHMENT_BYTES {
        return (StatusCode::PAYLOAD_TOO_LARGE, "attachments too large").into_response();
    }
    // The server decides who a message is from, exactly as the `PostMessage`
    // rpc does, which hardcodes "user". Storing the caller's `from` verbatim
    // would let a client holding the owner token post as "assistant", or as
    // "SYSTEM", into the transcript the user and every attached assistant
    // read, and would break the invariant the classification in
    // `capability.rs` argues from ("the only thing that decides who a message
    // is from is which operation posted it").
    //
    // A caller that asked for something else is told, rather than having its
    // message relabelled without a word.
    if !body.from.is_empty() && body.from != "user" {
        return (
            StatusCode::BAD_REQUEST,
            "a message's `from` is set by the server: this route posts as the user, and the assistant posts with Reply",
        )
            .into_response();
    }
    let mut st = app.state.lock().unwrap();
    let seq = st.messages.last().map(|m| m.seq).unwrap_or(0) + 1;
    let msg = ChatMessage { seq, from: "user".to_owned(), text: body.text, at: now(), attachments: body.attachments, state: String::new(), activity: String::new() };
    st.messages.push(msg.clone());
    let excess = st.messages.len().saturating_sub(500);
    if excess > 0 {
        st.messages.drain(..excess);
    }
    // Events carry the message without the image bytes; fetch them with the
    // `GetAttachment` op (or GET /api/session/messages/{seq}/attachment/{i}).
    let mut ev = msg.clone();
    for a in &mut ev.attachments {
        a.png_base64.clear();
    }
    app.emit_by(&mut st, "message", serde_json::to_value(&ev).unwrap_or_default(), &by);
    Json(ev).into_response()
}

fn attachment_bytes(st: &SessionState, seq: u64, i: usize) -> anyhow::Result<(Vec<u8>, Attachment)> {
    let m = st.messages.iter().find(|m| m.seq == seq).ok_or_else(|| {
        let have: Vec<String> = st.messages.iter().rev().take(5).map(|m| m.seq.to_string()).collect();
        anyhow::anyhow!("no message with seq {seq} (recent: {})", have.join(", "))
    })?;
    let a = m.attachments.get(i).ok_or_else(|| {
        anyhow::anyhow!("message {seq} has {} attachment(s), so index {i} does not exist", m.attachments.len())
    })?;
    use base64::Engine as _;
    let bytes = base64::engine::general_purpose::STANDARD
        .decode(a.png_base64.trim())
        .map_err(|e| anyhow::anyhow!("attachment {i} of message {seq} is not valid base64: {e}"))?;
    Ok((bytes, a.clone()))
}

async fn get_attachment(State(app): State<Shared>, axum::extract::Path((seq, i)): axum::extract::Path<(u64, usize)>) -> Response {
    let st = app.state.lock().unwrap();
    match attachment_bytes(&st, seq, i) {
        Ok((bytes, _)) => ([(header::CONTENT_TYPE, "image/png")], bytes).into_response(),
        Err(e) => (StatusCode::NOT_FOUND, e.to_string()).into_response(),
    }
}

#[derive(Deserialize)]
struct AssistantBody {
    #[serde(default)]
    listening: Option<bool>,
}

/// The UI's "Listen" toggle: whether an attached assistant gets UI events.
async fn put_assistant(State(app): State<Shared>, Json(body): Json<AssistantBody>) -> Json<SessionView> {
    let mut st = app.state.lock().unwrap();
    if let Some(l) = body.listening {
        // `was` is the toggle as it stood for every event before this one. A
        // reader that attaches with a backlog knows the toggle only as it is
        // now; the first of these events in the backlog tells it what held
        // for the events in front of it (spec/wakeups.qnt).
        let was = st.assistant_listening;
        st.assistant_listening = l;
        app.emit(&mut st, "assistant_listening", serde_json::json!({"listening": l, "was": was}));
    }
    Json(app.session_view(&st))
}

#[derive(Deserialize)]
struct CameraBody {
    camera: serde_json::Value,
}

/// The UI reports its camera so `cadpole render --view current` and assistant
/// screenshots match what the user sees. Not an event (too chatty).
async fn put_camera(State(app): State<Shared>, Json(body): Json<CameraBody>) -> StatusCode {
    app.state.lock().unwrap().camera = Some(body.camera);
    StatusCode::NO_CONTENT
}

#[derive(Deserialize)]
struct PollQuery {
    #[serde(default)]
    after: u64,
    #[serde(default)]
    timeout_ms: Option<u64>,
    /// When given, `after` defaults to this client's stored cursor and the
    /// client's own writes are filtered out.
    #[serde(default)]
    client: Option<String>,
}

/// JSON long-poll for clients that cannot do SSE (the MCP server, scripts).
async fn poll_events(State(app): State<Shared>, Query(q): Query<PollQuery>) -> Json<serde_json::Value> {
    let timeout = std::time::Duration::from_millis(q.timeout_ms.unwrap_or(30_000).min(120_000));
    let deadline = tokio::time::Instant::now() + timeout;
    let client = q.client.clone().unwrap_or_default();
    let after = {
        let st = app.state.lock().unwrap();
        if q.after == 0 && !client.is_empty() { st.cursors.get(&client).copied().unwrap_or(0) } else { q.after }
    };
    let mut rx = app.events.subscribe();
    loop {
        let backlog: Vec<Event> = {
            let st = app.state.lock().unwrap();
            st.events.iter().filter(|e| e.seq > after && (client.is_empty() || e.by != client)).cloned().collect()
        };
        if !backlog.is_empty() {
            let cursor = backlog.last().map(|e| e.seq).unwrap_or(after);
            return Json(serde_json::json!({"events": backlog, "cursor": cursor}));
        }
        let remaining = deadline.saturating_duration_since(tokio::time::Instant::now());
        if remaining.is_zero() {
            // Nothing for this client, but the log may have moved on with its
            // own writes; hand back the true tail so it does not re-scan them.
            let tail = app.state.lock().unwrap().events.last().map(|e| e.seq).unwrap_or(after);
            return Json(serde_json::json!({"events": [], "cursor": tail.max(after)}));
        }
        match tokio::time::timeout(remaining, rx.recv()).await {
            Ok(Ok(_)) | Ok(Err(broadcast::error::RecvError::Lagged(_))) => continue,
            _ => return Json(serde_json::json!({"events": [], "cursor": after})),
        }
    }
}

async fn post_save(State(app): State<Shared>) -> Response {
    let st = app.state.lock().unwrap();
    let Some(path) = st.save_path.clone() else {
        return (StatusCode::BAD_REQUEST, "server started without --save-path and no packed document input").into_response();
    };
    let doc = saveable_document(&st);
    match crate::input::save(&path, &doc) {
        Ok(()) => Json(serde_json::json!({"path": path})).into_response(),
        Err(e) => (StatusCode::INTERNAL_SERVER_ERROR, e.to_string()).into_response(),
    }
}

fn saveable_document(st: &SessionState) -> pb::Document {
    let mut doc = st.document.clone();
    doc.modified_at = Some(Default::default());
    if let Some(t) = doc.modified_at.as_mut() {
        t.seconds = chrono::Utc::now().timestamp();
    }
    doc.selection = Some(pb::Selection {
        selected: st.selection.selected.iter().map(ref_to_proto).collect(),
        hovered: None,
        note: st.selection.note.clone(),
    });
    doc
}

#[derive(Deserialize)]
struct SaveAsBody {
    /// A *name*, not a path. The server picks the file under `--root`.
    name: String,
    /// Replace a file under that id that holds a different document. Without
    /// it such a save is a 409 (see [`post_save_as`]).
    #[serde(default)]
    overwrite: bool,
}

/// "Save as": the caller names the document, the server decides where it goes.
/// There is no path anywhere in this API (docs/security.md, "Filesystem
/// access"), which is what makes traversal impossible by construction.
///
/// [`slug`] is many-to-one -- case, punctuation and anything past 48
/// characters collapse -- so the id alone cannot tell two documents apart.
/// A file already under the id is replaced without asking only when it is the
/// one this session saves to *and* it holds a document saved under this same
/// name, i.e. when this is plainly a re-save. Anything else (another name that
/// slugs the same way, a file some other session or person put there, a file
/// that does not decode) is a 409 naming the id, and the caller repeats the
/// request with `"overwrite": true` if replacing it is what they meant.
///
/// The write goes to a temporary file beside the target and is renamed over
/// it, so a failure half way leaves the old file whole rather than truncated.
async fn post_save_as(State(app): State<Shared>, Json(body): Json<SaveAsBody>) -> Response {
    let id = slug(&body.name);
    let path = match app.root.resolve(&id) {
        Ok(p) => p,
        Err(e) => return (StatusCode::BAD_REQUEST, e.to_string()).into_response(),
    };
    let doc = {
        let st = app.state.lock().unwrap();
        if !body.overwrite {
            if let Some(why) = save_as_conflict(&path, &body.name, st.save_path.as_deref()) {
                return (
                    StatusCode::CONFLICT,
                    format!("{id:?} already holds {why}; choose another name, or send \"overwrite\": true to replace it"),
                )
                    .into_response();
            }
        }
        let mut doc = saveable_document(&st);
        doc.name = body.name.clone();
        doc
    };
    let bytes = doc.encode_to_vec();
    if bytes.len() as u64 > security::MAX_DOCUMENT_BYTES {
        return (StatusCode::PAYLOAD_TOO_LARGE, format!("document is {} bytes, over the limit", bytes.len())).into_response();
    }
    match security::write_replacing(&path, &bytes) {
        Ok(()) => {
            // The session takes the new name only once it is on disk: a
            // refused or failed save leaves the session as it was.
            let mut st = app.state.lock().unwrap();
            st.document.name = body.name.clone();
            st.save_path = Some(path);
            Json(serde_json::json!({"id": id, "bytes": bytes.len()})).into_response()
        }
        Err(e) => (StatusCode::INTERNAL_SERVER_ERROR, e.to_string()).into_response(),
    }
}

/// Why writing the session's document to `path` under `name` would replace
/// something else, or `None` when it would not: there is no file, or the
/// file is this session's own save target and holds a document of this name.
fn save_as_conflict(path: &Path, name: &str, save_path: Option<&Path>) -> Option<String> {
    let Ok(meta) = std::fs::symlink_metadata(path) else { return None };
    if !meta.is_file() {
        return Some("something that is not a document file".to_owned());
    }
    let same_file = |a: &Path, b: &Path| match (std::fs::canonicalize(a), std::fs::canonicalize(b)) {
        (Ok(a), Ok(b)) => a == b,
        _ => false,
    };
    let held = std::fs::read(path).ok().and_then(|b| pb::Document::decode(&b[..]).ok());
    match held {
        None => Some("a file that is not a readable document".to_owned()),
        Some(d) if d.name != name => Some(format!("a different document, {:?}", d.name)),
        Some(_) if !save_path.is_some_and(|s| same_file(s, path)) => {
            Some(format!("a document named {name:?} that this session did not open or save"))
        }
        Some(_) => None,
    }
}

/// A user-supplied name reduced to a document id: lowercase, letters, digits,
/// `-` and `_`. Anything else becomes `-`. This is the only place user text
/// becomes part of a file name, and it cannot produce a separator or a dot.
fn slug(name: &str) -> String {
    let s: String = name
        .chars()
        .map(|c| if c.is_ascii_alphanumeric() { c.to_ascii_lowercase() } else { '-' })
        .collect::<String>()
        .trim_matches('-')
        .to_owned();
    let s: String = s.chars().take(48).collect();
    if s.is_empty() { format!("untitled-{}", random_id(4)) } else { s }
}

/// `GET /api/documents`: what is in the root, by id. No paths are returned.
async fn get_documents(State(app): State<Shared>) -> Json<serde_json::Value> {
    let docs: Vec<serde_json::Value> =
        app.root.list().into_iter().map(|(id, size)| serde_json::json!({"id": id, "bytes": size})).collect();
    Json(serde_json::json!({"documents": docs}))
}

/// `GET /api/documents/{id}`: the document, as protobuf. The id is validated
/// before the file system is touched and the resolved path is checked against
/// the canonical root, so a traversal attempt is a 400, not a read.
async fn get_document(State(app): State<Shared>, axum::extract::Path(id): axum::extract::Path<String>) -> Response {
    let path = match app.root.resolve(&id) {
        Ok(p) => p,
        Err(e) => return (StatusCode::BAD_REQUEST, e.to_string()).into_response(),
    };
    match std::fs::metadata(&path) {
        Ok(m) if m.len() > security::MAX_DOCUMENT_BYTES => {
            return (StatusCode::PAYLOAD_TOO_LARGE, "document is over the size limit").into_response();
        }
        Ok(_) => {}
        Err(_) => return (StatusCode::NOT_FOUND, "no such document").into_response(),
    }
    match std::fs::read(&path) {
        Ok(bytes) => ([(header::CONTENT_TYPE, "application/x-protobuf")], bytes).into_response(),
        Err(e) => (StatusCode::INTERNAL_SERVER_ERROR, e.to_string()).into_response(),
    }
}

fn ref_to_proto(r: &RefView) -> pb::EntityRef {
    pb::EntityRef {
        feature_id: r.feature_id.clone(),
        body_id: r.body_id.clone(),
        kind: cadpole_engine::kind_from_name(&r.kind) as i32,
        topo_name: r.topo_name.clone(),
        fingerprint: r.fingerprint.as_ref().map(FingerprintView::to_proto),
    }
}

#[derive(Deserialize)]
struct EventsQuery {
    #[serde(default)]
    after: u64,
    /// Also send what messages in progress gain (`App::live`), as
    /// `reply_stream` frames with no id. Asked for by a page that draws
    /// them; a reader that only wants the log never sees one.
    #[serde(default)]
    live: Option<String>,
}

async fn get_events(State(app): State<Shared>, Query(q): Query<EventsQuery>) -> Sse<impl tokio_stream::Stream<Item = Result<SseEvent, std::convert::Infallible>>> {
    let (backlog, rx) = {
        let mut st = app.state.lock().unwrap();
        st.clients += 1;
        let n = st.clients;
        app.emit(&mut st, "client_joined", serde_json::json!({"clients": n}));
        let backlog: Vec<Event> = st.events.iter().filter(|e| e.seq > q.after).cloned().collect();
        (backlog, app.events.subscribe())
    };
    let to_sse = |e: Event| Ok(SseEvent::default().id(e.seq.to_string()).event(e.kind.clone()).data(serde_json::to_string(&e).unwrap_or_default()));
    let last = backlog.last().map(|e| e.seq).unwrap_or(q.after);
    let live = BroadcastStream::new(rx).filter_map(move |r| r.ok().filter(|e| e.seq > last)).map(to_sse);
    // A frame is shaped like an event with sequence number 0, so a reader
    // parses it the same way, and carries no SSE id, so it never moves the
    // position a reconnection resumes from.
    let wanted = wants_live(q.live.as_deref());
    let frames = BroadcastStream::new(app.live.subscribe()).filter_map(move |r| r.ok().filter(|_| wanted)).map(|f| {
        let data = serde_json::json!({"seq": 0, "kind": "reply_stream", "data": f});
        Ok(SseEvent::default().event("reply_stream").data(data.to_string()))
    });
    let stream = tokio_stream::iter(backlog.into_iter().map(to_sse)).chain(live.merge(frames));
    let stream = ClientCounted { inner: Box::pin(stream), app: app.clone() };
    Sse::new(stream).keep_alive(KeepAlive::default())
}

/// `?live=` on the event stream: anything but absent, empty, `0` or `false`.
fn wants_live(v: Option<&str>) -> bool {
    !matches!(v, None | Some("" | "0" | "false"))
}

/// Decrements the client count when the SSE stream is dropped.
struct ClientCounted<S> {
    inner: std::pin::Pin<Box<S>>,
    app: Shared,
}

impl<S: tokio_stream::Stream> tokio_stream::Stream for ClientCounted<S> {
    type Item = S::Item;
    fn poll_next(mut self: std::pin::Pin<&mut Self>, cx: &mut std::task::Context<'_>) -> std::task::Poll<Option<Self::Item>> {
        self.inner.as_mut().poll_next(cx)
    }
}

impl<S> Drop for ClientCounted<S> {
    fn drop(&mut self) {
        if let Ok(mut st) = self.app.state.lock() {
            st.clients = st.clients.saturating_sub(1);
            let n = st.clients;
            self.app.emit(&mut st, "client_left", serde_json::json!({"clients": n}));
        }
    }
}

/// `GET /api/session/document`: the session's document as protobuf.
///
/// The JSON session view is the web UI's contract and carries only scripts and
/// overrides; a client that has to run an *engine* operation (export, explain,
/// the asset ops) needs the whole thing, assets included. Nothing here is a
/// path: the bytes are the document, wherever it happens to live.
async fn get_session_document(State(app): State<Shared>) -> Response {
    let doc = {
        let st = app.state.lock().unwrap();
        ServerHost::document_locked(&st)
    };
    ([(header::CONTENT_TYPE, "application/x-protobuf")], doc.encode_to_vec()).into_response()
}

async fn get_model(State(app): State<Shared>) -> Response {
    let doc = app.state.lock().unwrap().document.clone();
    let memo = app.memo.clone();
    let model = tokio::task::spawn_blocking(move || {
        let (mut ctx, _, ms) = cadpole_engine::evaluate_document_options(
            &doc,
            &Default::default(),
            0.0,
            0.0,
            &Default::default(),
            cadpole_engine::limits::default_limits(),
            // `/api/model` is the owner's alone (`capability_refusal`), so
            // its evaluation may start solve jobs like the UI's.
            cadpole_engine::EvalOptions {
                memo: Some(memo),
                preview: false,
                solve_jobs: crate::opt_jobs::installed().is_some_and(|s| s.runner().is_some()),
            },
        );
        cadpole_engine::print::model_with_plates(&mut ctx, false, ms)
    })
    .await;
    match model {
        Ok(m) => ([(header::CONTENT_TYPE, "application/x-protobuf")], m.encode_to_vec()).into_response(),
        Err(e) => (StatusCode::INTERNAL_SERVER_ERROR, e.to_string()).into_response(),
    }
}

#[derive(Deserialize)]
struct RenderBody {
    #[serde(default)]
    width: u32,
    #[serde(default)]
    height: u32,
    #[serde(default)]
    view: String,
    #[serde(default)]
    params: serde_json::Map<String, serde_json::Value>,
}

/// One render job, subject to the concurrency limit, the bounded queue and the
/// wall-clock budget (docs/security.md, "Render and sweep limits").
///
/// The budget releases the *caller*; the worker thread it abandons is stopped
/// by the evaluation deadline, which is what `--eval-deadline-ms` sets
/// process-wide at startup. That is the honest division of labour: this layer
/// guarantees the HTTP request ends, the sandbox guarantees the CPU is given
/// back.
///
/// The error is a status and a sentence rather than a `Response`, because two
/// transports now render it: `/api/ops` and the JSON routes turn it into
/// plain text, and the Connect route turns it into an error envelope with the
/// code that status means (`connect::Code::of_status`).
async fn render_job<T: Send + 'static>(
    app: &Shared,
    what: &str,
    work: impl FnOnce() -> T + Send + 'static,
) -> Result<T, (StatusCode, String)> {
    let waiting = app.render_waiting.fetch_add(1, Ordering::SeqCst);
    struct Leave<'a>(&'a AtomicUsize);
    impl Drop for Leave<'_> {
        fn drop(&mut self) {
            self.0.fetch_sub(1, Ordering::SeqCst);
        }
    }
    let _leave = Leave(&app.render_waiting);
    if waiting >= RENDER_QUEUE {
        return Err((
            StatusCode::TOO_MANY_REQUESTS,
            format!("{RENDER_QUEUE} render jobs are already queued; try again in a moment"),
        ));
    }
    let Ok(_permit) = app.render_slots.acquire().await else {
        return Err((StatusCode::SERVICE_UNAVAILABLE, "render queue is shutting down".to_owned()));
    };
    match tokio::time::timeout(app.render_budget, tokio::task::spawn_blocking(work)).await {
        Ok(Ok(v)) => Ok(v),
        Ok(Err(e)) => Err((StatusCode::INTERNAL_SERVER_ERROR, e.to_string())),
        Err(_) => Err((
            StatusCode::GATEWAY_TIMEOUT,
            format!(
                "{what} exceeded the {} ms budget and was abandoned; simplify the document or raise --render-budget-ms",
                app.render_budget.as_millis()
            ),
        )),
    }
}

async fn post_render(State(app): State<Shared>, Json(body): Json<RenderBody>) -> Response {
    let doc = app.state.lock().unwrap().document.clone();
    let mut req = pb::RenderRequest {
        document: Some(doc),
        width: body.width,
        height: body.height,
        camera: Some(pb::Camera { preset: if body.view.is_empty() { "iso".into() } else { body.view }, ..Default::default() }),
        ..Default::default()
    };
    for (k, v) in &body.params {
        if let Some(pv) = cadpole_engine::view::param_value_from_json(v) {
            req.param_overrides.insert(k.clone(), pv.to_proto());
        }
    }
    match render_job(&app, "render", move || cadpole_engine::render(&req)).await {
        Ok(Ok(r)) => ([(header::CONTENT_TYPE, "image/png")], r.png).into_response(),
        Ok(Err(e)) => (StatusCode::BAD_REQUEST, e.to_string()).into_response(),
        Err(refusal) => refusal.into_response(),
    }
}

/// The protobuf envelope over HTTP, for non-browser clients and remote render offload.
async fn post_call(State(app): State<Shared>, body: Bytes) -> Response {
    if body.len() > app.max_body {
        return (StatusCode::PAYLOAD_TOO_LARGE, "request too large").into_response();
    }
    let body = lend_session_assets(&app, body);
    match render_job(&app, "call", move || cadpole_engine::call(&body)).await {
        Ok(bytes) => ([(header::CONTENT_TYPE, "application/x-protobuf")], bytes).into_response(),
        Err(refusal) => refusal.into_response(),
    }
}

/// An envelope whose document carries no assets is evaluated against the
/// session's, as `Solve` does.
///
/// A client that rebuilt the document from `GET /api/session` (the Apple
/// RemoteEngine) has asset names but no bytes -- the JSON view never carries
/// them -- so every script that read an asset failed with "not found
/// (available: none)" on this route while the web UI showed the model. A
/// document that brings any assets of its own is left exactly as sent, and
/// bytes that do not decode go through unchanged for the engine to refuse.
fn lend_session_assets(app: &Shared, body: Bytes) -> Bytes {
    use pb::request::Request as R;
    let Ok(mut req) = pb::Request::decode(&body[..]) else { return body };
    let doc = match req.request.as_mut() {
        Some(R::Evaluate(e)) => e.document.as_mut(),
        Some(R::Export(e)) => e.document.as_mut(),
        Some(R::Render(r)) => r.document.as_mut(),
        _ => None,
    };
    let Some(doc) = doc.filter(|d| d.assets.is_empty()) else { return body };
    let assets = app.state.lock().unwrap().document.assets.clone();
    if assets.is_empty() {
        return body;
    }
    doc.assets = assets;
    Bytes::from(req.encode_to_vec())
}

// ---------------------------------------------------------------------------
// The operation registry over HTTP
// ---------------------------------------------------------------------------

/// The operation names this server answers at `POST /api/ops/<name>`: the rpcs
/// of `cadpole.v1.CadpoleOps`, nothing added and nothing left out. Exists so the
/// cross-front-end test in main.rs can compare the three name sets.
#[cfg_attr(not(test), allow(dead_code))]
pub fn op_names() -> Vec<String> {
    cadpole_ops::registry().iter().filter(|m| !m.hidden).map(|m| m.name.clone()).collect()
}

/// `cadpole_ops::Host` served from this process's own session state. The CLI and
/// `cadpole mcp` implement the same trait over HTTP against these endpoints, so
/// every operation behaves the same wherever it runs.
struct ServerHost {
    app: Shared,
    /// `X-Cadpole-Client` of the request being served, so the events this call
    /// emits are attributed to the caller.
    by: String,
    /// Which token is calling. Only the owner's evaluations may start solve
    /// jobs: `Evaluate` is granted to the listener as a pure function
    /// under the deadline, and a job runs past it.
    role: cadpole_ops::capability::Role,
}

impl ServerHost {
    /// `StreamReply` at a given instant: the rules are `reply_stream.rs`, and
    /// this is where their outcome meets the session. A message that ended
    /// becomes one `message` event in the log, as `Reply` makes; a message
    /// that grew becomes a frame on [`App::live`] and no event.
    fn stream_reply_at(&self, req: &pb::StreamReplyRequest, now: std::time::Instant) -> anyhow::Result<pb::StreamReplyResponse> {
        let finish = match req.finish() {
            pb::ReplyFinish::Unspecified => None,
            pb::ReplyFinish::Done => Some(reply_stream::Finish::Done),
            pb::ReplyFinish::Failed => Some(reply_stream::Finish::Failed),
        };
        let author = reply_stream::Author { role: self.role, client: self.by.clone() };
        let update = reply_stream::Update { seq: req.seq, append: &req.append, activity: &req.activity, finish };
        let mut st = self.app.state.lock().unwrap();
        let st = &mut *st;
        let applied = st.streams.apply(&mut st.messages, &author, &update, now, &crate::server::now())?;
        self.app.announce_ended(st, &applied.ended, &self.by);
        if let (Some(offset), Some(m)) = (applied.offset, st.messages.iter().find(|m| m.seq == applied.seq)) {
            // Nobody listening is the ordinary case, not an error.
            let _ = self.app.live.send(LiveFrame {
                seq: m.seq,
                from: m.from.clone(),
                at: m.at.clone(),
                state: m.state.clone(),
                offset,
                length: m.text.len(),
                append: req.append.clone(),
                activity: m.activity.clone(),
            });
        }
        let excess = st.messages.len().saturating_sub(500);
        if excess > 0 {
            st.messages.drain(..excess);
        }
        Ok(pb::StreamReplyResponse {
            seq: applied.seq,
            state: applied.state.to_owned(),
            length: applied.length as u64,
            throttled: applied.throttled,
        })
    }

    /// The owner's evaluations, in a server that runs solve jobs, may start
    /// them.
    fn solve_jobs_allowed(&self) -> bool {
        self.role == cadpole_ops::capability::Role::Owner && crate::opt_jobs::installed().is_some_and(|s| s.runner().is_some())
    }

    fn document_locked(st: &SessionState) -> pb::Document {
        let mut doc = st.document.clone();
        doc.selection = Some(pb::Selection {
            selected: st.selection.selected.iter().map(ref_to_proto).collect(),
            hovered: st.selection.hovered.as_ref().map(ref_to_proto),
            note: st.selection.note.clone(),
        });
        doc
    }

    /// Evaluate the session's document and report what an editing op should
    /// hand back: the diagnostics and the feature ids (one
    /// call is edit *and* feedback).
    fn feedback(&self, doc: &pb::Document) -> (Vec<pb::Diagnostic>, Vec<String>) {
        let (ctx, _, _) = cadpole_engine::evaluate_document_options(
            doc,
            &Default::default(),
            0.0,
            0.0,
            &Default::default(),
            cadpole_engine::limits::default_limits(),
            cadpole_engine::EvalOptions { memo: Some(self.app.memo.clone()), preview: false, solve_jobs: self.solve_jobs_allowed() },
        );
        (
            ctx.diagnostics.iter().map(cadpole_engine::cadpole_core::context::Diagnostic::to_proto).collect(),
            ctx.features.iter().map(|f| f.id.0.clone()).collect(),
        )
    }
}

/// Caller-written text on its way into the event log, the SSE stream and the
/// server's terminal.
///
/// Control characters are the attack: a newline in a "one line" field forges a
/// second line in the server's log, and a bare `\n\n` in an SSE payload would
/// end the frame early. They become spaces (a stack keeps its newlines, which
/// is what makes it readable, but nothing else). The text is then cut to `max`
/// bytes on a character boundary, and the caller is told which fields were
/// cut: a shortened report is worth having, a silently shortened one is not.
fn sanitize(text: &str, max: usize, multiline: bool) -> (String, bool) {
    let mut out = String::with_capacity(text.len().min(max + 4));
    let mut cut = false;
    for c in text.chars() {
        // Stop at the cap rather than copying the whole thing and trimming:
        // this runs on the path a crashing client hammers, and the body limit
        // above it is eight megabytes.
        if out.len() + c.len_utf8() > max {
            cut = true;
            break;
        }
        if c == '\n' && multiline {
            out.push('\n');
        } else if c.is_control() {
            out.push(' ');
        } else {
            out.push(c);
        }
    }
    let mut out = out.trim().to_owned();
    if cut {
        out.push_str(" […truncated]");
    }
    (out, cut)
}

/// `CLIENT_FAILURE_KIND_WASM_PANIC` -> `wasm_panic`: the proto spelling is the
/// wire contract, but the event log and the terminal are read by people.
fn short_enum_name(full: &str, prefix: &str) -> String {
    full.strip_prefix(prefix).unwrap_or(full).to_lowercase()
}

fn session_to_proto(v: &SessionView) -> pb::Session {
    // The view is the JSON contract; cadpole.v1.Session mirrors it, so this goes
    // through serde rather than repeating the field list.
    crate::ops_host::session_from_json(&serde_json::to_value(v).unwrap_or_default())
}

impl cadpole_ops::Host for ServerHost {
    fn document(&self) -> anyhow::Result<pb::Document> {
        let st = self.app.state.lock().unwrap();
        Ok(Self::document_locked(&st))
    }

    fn assets(&self) -> anyhow::Result<Vec<pb::Asset>> {
        let st = self.app.state.lock().unwrap();
        Ok(st.document.assets.clone())
    }

    fn memo(&self) -> Option<Arc<cadpole_engine::cadpole_core::memo::Memo>> {
        Some(self.app.memo.clone())
    }

    fn solve_jobs(&self) -> bool {
        self.solve_jobs_allowed()
    }

    // ---- solve jobs: this server's runner, and the machine's jobs

    fn solve(&self, req: &pb::SolveRequest) -> anyhow::Result<pb::SolveResponse> {
        let runner = server_runner()?;
        anyhow::ensure!(self.solve_jobs_allowed(), "Solve: only the owner's token starts solve jobs");
        let mut doc = match &req.document {
            Some(d) => d.clone(),
            None => cadpole_ops::Host::document(self)?,
        };
        if doc.assets.is_empty() {
            doc.assets = cadpole_ops::Host::assets(self)?;
        }
        let memo = self.app.memo.clone();
        let evaluate = move || -> anyhow::Result<crate::opt_jobs::ops::Evaluated> {
            let (ctx, _, _) = cadpole_engine::evaluate_document_options(
                &doc,
                &Default::default(),
                0.0,
                0.0,
                &Default::default(),
                cadpole_engine::limits::default_limits(),
                cadpole_engine::EvalOptions { memo: Some(memo.clone()), preview: false, solve_jobs: true },
            );
            Ok(crate::opt_jobs::ops::Evaluated::of(&ctx))
        };
        crate::opt_jobs::ops::solve(&runner, req, Some(crate::opt_jobs::ops::MAX_REMOTE_WAIT), &evaluate)
    }

    fn list_jobs(&self, req: &pb::ListJobsRequest) -> anyhow::Result<pb::ListJobsResponse> {
        let solver = crate::opt_jobs::installed().ok_or_else(|| anyhow::anyhow!("ListJobs: no solver in this server"))?;
        let cache = solver.cache().ok_or_else(|| anyhow::anyhow!("ListJobs: this server has no solution cache"))?;
        crate::opt_jobs::ops::list(&crate::opt_jobs::jobs::Jobs::of(cache), cache, req)
    }

    fn wait_jobs(&self, req: &pb::WaitJobsRequest) -> anyhow::Result<pb::WaitJobsResponse> {
        let r = server_runner()?;
        crate::opt_jobs::ops::wait(&r, req, Some(crate::opt_jobs::ops::MAX_REMOTE_WAIT))
    }

    fn cancel_jobs(&self, req: &pb::CancelJobsRequest) -> anyhow::Result<pb::CancelJobsResponse> {
        let r = server_runner()?;
        crate::opt_jobs::ops::cancel(&r, req)
    }

    fn get_session(&self, _req: &pb::GetSessionRequest) -> anyhow::Result<pb::Session> {
        let st = self.app.state.lock().unwrap();
        Ok(session_to_proto(&self.app.session_view(&st)))
    }

    fn set_script(&self, req: &pb::SetScriptRequest) -> anyhow::Result<pb::SetScriptResponse> {
        let path = if req.path.is_empty() { "main.star".to_owned() } else { req.path.clone() };
        check_script(&path, &req.source)?;
        // `source` is `required: true` in the schema, but nothing enforces
        // `required` at dispatch -- it reaches only the JSON Schema an
        // assistant reads -- and proto3 cannot tell an absent string from an
        // empty one. So the check lives here, and it has to: a request that
        // simply omits the field erased the file and answered with the new
        // revision, which is exactly what `capability.rs` predicts of a
        // whole-file write ("one confused call [can] silently discard work
        // that still matches the revision it read") and why the listener may
        // not call this at all. It cannot go in `check_script`, which
        // `edit_script` calls with an empty source on purpose. A caller that
        // really means to empty a file says so with EditScript.
        if req.source.is_empty() {
            anyhow::bail!("SetScript needs a `source`: it is required, and an empty one would erase {path:?}. To empty a file deliberately, use EditScript.");
        }
        let (doc, rev) = {
            let mut st = self.app.state.lock().unwrap();
            if req.base_revision != st.revision {
                let current = st.document.scripts.iter().find(|f| f.path == path).map(|f| f.source.clone()).unwrap_or_default();
                return Ok(pb::SetScriptResponse {
                    conflict: true,
                    current_revision: st.revision,
                    current_source: current,
                    ..Default::default()
                });
            }
            if let Some(d) = cadpole_engine::cadpole_script::check_syntax(&path, &req.source) {
                let at = d.span.as_ref().map(|s| format!("{}:{}:{} ", s.path, s.start_line, s.start_col)).unwrap_or_default();
                anyhow::bail!("syntax error: {at}{}", d.message);
            }
            match st.document.scripts.iter_mut().find(|s| s.path == path) {
                Some(s) => s.source = req.source.clone(),
                None => st.document.scripts.push(pb::ScriptFile { path: path.clone(), source: req.source.clone() }),
            }
            st.revision += 1;
            let rev = st.revision;
            self.app.emit_by(&mut st, "script_changed", serde_json::json!({"path": path, "revision": rev}), &self.by);
            (ServerHost::document_locked(&st), rev)
        };
        let (diagnostics, features) = self.feedback(&doc);
        Ok(pb::SetScriptResponse {
            // `rev`, not a second read of the state. `feedback` evaluates the
            // whole document with the lock dropped, so re-reading afterwards
            // can hand this writer a revision somebody else produced -- and
            // the writer's next request names it as `base_revision`, the
            // check passes, and the other write is gone. The optimistic
            // concurrency defeats itself. Modelled in `spec/revisions.qnt`.
            revision: rev,
            diagnostics,
            features,
            ..Default::default()
        })
    }

    fn edit_script(&self, req: &pb::EditScriptRequest) -> anyhow::Result<pb::EditScriptResponse> {
        let path = if req.path.is_empty() { "main.star".to_owned() } else { req.path.clone() };
        check_script(&path, "")?;
        // Read the current source and the feature spans first, under the lock
        // but without evaluating: the `feature` edit form needs spans from the
        // document as it is *now*.
        let (before, revision) = {
            let st = self.app.state.lock().unwrap();
            let src = st
                .document
                .scripts
                .iter()
                .find(|f| f.path == path)
                .map(|f| f.source.clone())
                .ok_or_else(|| {
                    let have: Vec<&str> = st.document.scripts.iter().map(|f| f.path.as_str()).collect();
                    anyhow::anyhow!("no script named {path:?} in this document (have: {})", have.join(", "))
                })?;
            (src, st.revision)
        };
        if req.base_revision != revision {
            let diff = cadpole_engine::edit::unified_diff(&before, &before, &path, 200);
            return Ok(pb::EditScriptResponse {
                conflict: true,
                current_revision: revision,
                current_source: before,
                diff,
                ..Default::default()
            });
        }
        let spans = if req.edits.iter().any(|e| !e.feature.is_empty()) {
            let doc = self.document()?;
            let (ctx, _) = cadpole_engine::evaluate_document(&doc, &Default::default(), 0.0);
            cadpole_engine::edit::feature_spans(&ctx)
        } else {
            Default::default()
        };
        let after = cadpole_engine::edit::apply_edits(&before, &req.edits, &spans)?;
        if let Some(d) = cadpole_engine::cadpole_script::check_syntax(&path, &after) {
            let at = d.span.as_ref().map(|s| format!("{}:{}:{} ", s.path, s.start_line, s.start_col)).unwrap_or_default();
            anyhow::bail!("the edit would not parse: {at}{}; nothing was written", d.message);
        }
        let diff = cadpole_engine::edit::unified_diff(&before, &after, &path, 200);
        let (doc, rev) = {
            let mut st = self.app.state.lock().unwrap();
            // Re-check under the lock: another writer may have slipped in
            // between the read above and here.
            if st.revision != revision {
                let current = st.document.scripts.iter().find(|f| f.path == path).map(|f| f.source.clone()).unwrap_or_default();
                return Ok(pb::EditScriptResponse {
                    conflict: true,
                    current_revision: st.revision,
                    diff: cadpole_engine::edit::unified_diff(&before, &current, &path, 200),
                    current_source: current,
                    ..Default::default()
                });
            }
            if let Some(s) = st.document.scripts.iter_mut().find(|s| s.path == path) {
                s.source = after.clone();
            }
            st.revision += 1;
            let rev = st.revision;
            self.app.emit_by(&mut st, "script_changed", serde_json::json!({"path": path, "revision": rev}), &self.by);
            (ServerHost::document_locked(&st), rev)
        };
        let (diagnostics, features) = self.feedback(&doc);
        Ok(pb::EditScriptResponse {
            // `rev`, for the reason spelled out in `set_script`.
            revision: rev,
            diagnostics,
            features,
            diff,
            applied: req.edits.len() as u32,
            ..Default::default()
        })
    }

    fn set_params(&self, req: &pb::SetParamsRequest) -> anyhow::Result<pb::SetParamsResponse> {
        let mut st = self.app.state.lock().unwrap();
        for (k, v) in &req.overrides {
            if k.len() > security::MAX_PARAM_KEY_BYTES {
                anyhow::bail!("parameter name is {} bytes, over the {} byte limit", k.len(), security::MAX_PARAM_KEY_BYTES);
            }
            if !st.document.param_overrides.contains_key(k) && st.document.param_overrides.len() >= security::MAX_PARAM_OVERRIDES {
                anyhow::bail!(
                    "this document already holds {} parameter overrides, which is the limit; clear some before setting more",
                    security::MAX_PARAM_OVERRIDES
                );
            }
            st.document.param_overrides.insert(k.clone(), v.clone());
        }
        for k in &req.clear {
            st.document.param_overrides.remove(k);
        }
        st.revision += 1;
        let overrides: serde_json::Map<String, serde_json::Value> = st
            .document
            .param_overrides
            .iter()
            .filter_map(|(k, v)| cadpole_engine::cadpole_core::ParamValue::from_proto(v).map(|v| (k.clone(), cadpole_engine::view::param_value_json(&v))))
            .collect();
        self.app.emit_by(&mut st, "params_changed", serde_json::json!({"overrides": overrides}), &self.by);
        Ok(pb::SetParamsResponse { revision: st.revision, param_overrides: st.document.param_overrides.clone() })
    }

    fn set_selection(&self, req: &pb::SetSelectionRequest) -> anyhow::Result<pb::SetSelectionResponse> {
        let view = |r: &pb::SessionRef| RefView {
            feature_id: r.feature_id.clone(),
            body_id: r.body_id.clone(),
            kind: r.kind.clone(),
            topo_name: r.topo_name.clone(),
            fingerprint: r.fingerprint.as_ref().map(FingerprintView::from_proto),
        };
        let sel = Selection {
            selected: req.selected.iter().map(view).collect(),
            hovered: req.hovered.as_ref().map(view),
            note: req.note.clone(),
        };
        let mut st = self.app.state.lock().unwrap();
        let changed = st.selection.selected != sel.selected || st.selection.note != sel.note;
        st.selection = sel.clone();
        if changed {
            self.app.emit_by(&mut st, "selection_changed", serde_json::to_value(&sel).unwrap_or_default(), &self.by);
        }
        Ok(pb::SetSelectionResponse { revision: st.revision, selected: req.selected.len() as u32 })
    }

    /// A front end saying it crashed.
    ///
    /// Everything here is caller-written text on its way into the event log an
    /// assistant reads, so it is sanitised before it is stored, not before it
    /// is printed: control characters go (a report must not be able to forge a
    /// second log line or a second SSE frame), every field is cut to its cap
    /// with the cut recorded, and the rate and session caps decide whether the
    /// report is recorded at all. A refusal is still an answer: it says which
    /// cap, how many have been dropped and how much budget is left.
    fn report_client_failure(&self, req: &pb::ReportClientFailureRequest) -> anyhow::Result<pb::ReportClientFailureResponse> {
        if req.message.trim().is_empty() {
            anyhow::bail!("a failure report needs `message`: the one line saying what went wrong");
        }
        let mut truncated: Vec<String> = Vec::new();
        let mut field = |name: &str, text: &str, max: usize, multiline: bool| -> String {
            let (out, cut) = sanitize(text, max, multiline);
            if cut {
                truncated.push(name.to_owned());
            }
            out
        };
        let message = field("message", &req.message, security::MAX_FAILURE_MESSAGE_BYTES, false);
        let stack = field("stack", &req.stack, security::MAX_FAILURE_STACK_BYTES, true);
        let location = field("location", &req.location, security::MAX_FAILURE_FIELD_BYTES, false);
        let operation = field("operation", &req.operation, security::MAX_FAILURE_FIELD_BYTES, false);
        let engine_build = field("engine_build", &req.engine_build, security::MAX_FAILURE_FIELD_BYTES, false);
        let engine_kind = field("engine_kind", &req.engine_kind, security::MAX_FAILURE_FIELD_BYTES, false);
        let script_digest = field("script_digest", &req.script_digest, security::MAX_FAILURE_FIELD_BYTES, false);

        let mut params = serde_json::Map::new();
        for (name, value) in req.params.iter().take(security::MAX_FAILURE_PARAMS) {
            let Some(v) = cadpole_engine::cadpole_core::ParamValue::from_proto(value) else { continue };
            let (name, _) = sanitize(name, security::MAX_FAILURE_FIELD_BYTES, false);
            params.insert(name, cadpole_engine::view::param_value_json(&v));
        }
        if req.params.len() > security::MAX_FAILURE_PARAMS {
            truncated.push("params".to_owned());
        }

        let kind = short_enum_name(req.kind().as_str_name(), "CLIENT_FAILURE_KIND_");
        let surface = short_enum_name(req.surface().as_str_name(), "CLIENT_SURFACE_");
        let now = std::time::Instant::now();
        let mut st = self.app.state.lock().unwrap();

        if let Err(why) = st.failures.admit(now) {
            st.failures.dropped += 1;
            let dropped = st.failures.dropped;
            let remaining = st.failures.remaining();
            // The cap is visible or it is a silent drop. One notice per
            // window carries the *dropped* report's own message, so the log
            // still says what is crashing, just not how many times.
            let seq = if st.failures.should_announce(now) {
                let data = serde_json::json!({
                    "throttled": true,
                    "dropped": dropped,
                    "kind": kind,
                    "surface": surface,
                    "message": message,
                    "reason": why,
                });
                let ev = self.app.emit_by(&mut st, "client_failure", data, &self.by);
                drop(st);
                note!("client failure reports throttled ({dropped} dropped so far); last: [{kind}] {message}");
                ev.seq
            } else {
                0
            };
            return Ok(pb::ReportClientFailureResponse {
                seq,
                accepted: false,
                refused: why,
                dropped,
                remaining,
                truncated,
            });
        }

        st.failures.accepted += 1;
        let remaining = st.failures.remaining();
        let dropped = st.failures.dropped;
        let data = serde_json::json!({
            "kind": kind,
            "surface": surface,
            "message": message,
            "location": location,
            "stack": stack,
            "operation": operation,
            "revision": req.revision,
            "params": params,
            "engine_build": engine_build,
            "engine_kind": engine_kind,
            "script_digest": script_digest,
            "uptime_ms": req.uptime_ms,
            "repeat": req.repeat.max(1),
            "truncated": truncated,
        });
        let ev = self.app.emit_by(&mut st, "client_failure", data, &self.by);
        drop(st);
        // The other half of "nothing recorded it": the server's own log.
        let at = if location.is_empty() { String::new() } else { format!(" at {location}") };
        let doing = if operation.is_empty() { String::new() } else { format!(", during {operation}") };
        let times = if req.repeat > 1 { format!(" (x{})", req.repeat) } else { String::new() };
        note!(
            "client failure [{kind}/{surface}]{times} {message}{at}{doing}, revision {}, engine {engine_build} ({engine_kind})",
            req.revision
        );
        Ok(pb::ReportClientFailureResponse { seq: ev.seq, accepted: true, refused: String::new(), dropped, remaining, truncated })
    }

    fn reply(&self, req: &pb::ReplyRequest) -> anyhow::Result<pb::ReplyResponse> {
        if req.text.len() > security::MAX_MESSAGE_BYTES {
            anyhow::bail!("message is {} bytes, over the {} byte limit", req.text.len(), security::MAX_MESSAGE_BYTES);
        }
        let mut st = self.app.state.lock().unwrap();
        let seq = st.messages.last().map(|m| m.seq).unwrap_or(0) + 1;
        let msg = ChatMessage { seq, from: "assistant".into(), text: req.text.clone(), at: now(), attachments: Vec::new(), state: String::new(), activity: String::new() };
        st.messages.push(msg.clone());
        self.app.emit_by(&mut st, "message", serde_json::to_value(&msg).unwrap_or_default(), &self.by);
        Ok(pb::ReplyResponse { seq })
    }

    fn stream_reply(&self, req: &pb::StreamReplyRequest) -> anyhow::Result<pb::StreamReplyResponse> {
        self.stream_reply_at(req, std::time::Instant::now())
    }

    /// The user's own client speaking. `from` is set here, not by the caller,
    /// for the same reason `Reply` sets it: the only thing that decides who a
    /// message is from is which operation posted it (capability.rs).
    fn post_message(&self, req: &pb::PostMessageRequest) -> anyhow::Result<pb::PostMessageResponse> {
        if req.text.len() > security::MAX_MESSAGE_BYTES {
            anyhow::bail!("message is {} bytes, over the {} byte limit", req.text.len(), security::MAX_MESSAGE_BYTES);
        }
        use base64::Engine as _;
        let attachments: Vec<Attachment> = req
            .attachments
            .iter()
            .map(|a| Attachment {
                kind: if a.kind.is_empty() { "screenshot".to_owned() } else { a.kind.clone() },
                png_base64: base64::engine::general_purpose::STANDARD.encode(&a.png),
                camera: a.camera.as_ref().map(cadpole_ops::struct_to_json),
                annotated: a.annotated,
            })
            .collect();
        // The cap is on the stored form, so it is the same number whether the
        // bytes arrived as protobuf or as base64 JSON.
        let total: usize = attachments.iter().map(|a| a.png_base64.len()).sum();
        if total > security::MAX_ATTACHMENT_BYTES {
            anyhow::bail!("attachments are {total} bytes, over the {} byte limit", security::MAX_ATTACHMENT_BYTES);
        }
        let mut st = self.app.state.lock().unwrap();
        let seq = st.messages.last().map(|m| m.seq).unwrap_or(0) + 1;
        let msg = ChatMessage { seq, from: "user".into(), text: req.text.clone(), at: now(), attachments, state: String::new(), activity: String::new() };
        st.messages.push(msg.clone());
        let excess = st.messages.len().saturating_sub(500);
        if excess > 0 {
            st.messages.drain(..excess);
        }
        // Events carry the message without the image bytes, exactly as the
        // hand-written route does; the picture comes from `GetAttachment`.
        let mut ev = msg.clone();
        for a in &mut ev.attachments {
            a.png_base64.clear();
        }
        self.app.emit_by(&mut st, "message", serde_json::to_value(&ev).unwrap_or_default(), &self.by);
        Ok(pb::PostMessageResponse {
            message: Some(pb::SessionMessage {
                seq: msg.seq,
                from: msg.from.clone(),
                text: msg.text.clone(),
                at: msg.at.clone(),
                attachments: msg.attachments.len() as u32,
                state: String::new(),
                activity: String::new(),
            }),
        })
    }

    fn save(&self, _req: &pb::SaveRequest) -> anyhow::Result<pb::SaveResponse> {
        let st = self.app.state.lock().unwrap();
        let path = st
            .save_path
            .clone()
            .ok_or_else(|| anyhow::anyhow!("this server has nowhere to save: it was started without --save-path and without a packed document"))?;
        let doc = saveable_document(&st);
        crate::input::save(&path, &doc)?;
        Ok(pb::SaveResponse { path: path.to_string_lossy().into_owned() })
    }

    fn get_attachment(&self, req: &pb::GetAttachmentRequest) -> anyhow::Result<pb::GetAttachmentResponse> {
        let st = self.app.state.lock().unwrap();
        let (png, a) = attachment_bytes(&st, req.seq, req.index as usize)?;
        let (width, height) = png_size(&png);
        Ok(pb::GetAttachmentResponse {
            png,
            kind: a.kind.clone(),
            annotated: a.annotated,
            width,
            height,
            camera: a.camera.as_ref().map(cadpole_ops::json_to_struct),
        })
    }

    fn ack(&self, req: &pb::AckRequest) -> anyhow::Result<pb::AckResponse> {
        if req.client.is_empty() {
            anyhow::bail!("ack needs a client id: pass the same one you send as X-Cadpole-Client");
        }
        let mut st = self.app.state.lock().unwrap();
        let last = st.events.last().map(|e| e.seq).unwrap_or(0);
        // A cursor past the end of the log means the log restarted under the
        // client: clamp and say so rather than silently
        // skipping everything that has happened since.
        let epoch_reset = req.cursor > last;
        let cursor = req.cursor.min(last);
        let slot = st.cursors.entry(req.client.clone()).or_insert(0);
        *slot = (*slot).max(cursor);
        let cursor = *slot;
        Ok(pb::AckResponse { client: req.client.clone(), cursor, last_event_seq: last, epoch_reset })
    }

    fn add_asset(&self, req: &pb::AddAssetRequest) -> anyhow::Result<pb::AddAssetResponse> {
        let mut st = self.app.state.lock().unwrap();
        if req.base_revision != 0 && req.base_revision != st.revision {
            return Ok(pb::AddAssetResponse { conflict: true, current_revision: st.revision, ..Default::default() });
        }
        // An upload is held to the transfer cap, the document to the native
        // total the session loaded its own `./assets` under.
        let limits = cadpole_engine::cadpole_assets::Limits {
            document: crate::input::asset_limits().document.max(cadpole_engine::cadpole_assets::MAX_DOCUMENT_ASSET_BYTES),
            ..cadpole_engine::cadpole_assets::Limits::TRANSFER
        };
        let asset = cadpole_engine::add_asset_limited(&mut st.document, &req.name, req.data.clone(), &req.source_path, &limits)?;
        st.revision += 1;
        let rev = st.revision;
        let name = req.name.clone();
        let kind = asset.kind().as_str_name().to_owned();
        self.app.emit_by(
            &mut st,
            "asset_changed",
            serde_json::json!({"name": name, "revision": rev, "kind": kind, "size": asset.size}),
            &self.by,
        );
        Ok(pb::AddAssetResponse { asset: Some(asset), revision: rev, ..Default::default() })
    }

    fn wait_events(&self, req: &pb::WaitEventsRequest) -> anyhow::Result<pb::WaitEventsResponse> {
        // The server's own routes never come here: `run_op` answers
        // `WaitEvents` with [`wait_events_async`], which holds no thread while
        // it waits. This is the trait's synchronous spelling of the same poll,
        // for a caller that already owns a thread.
        let poll = EventPoll::start(&self.app, req, &self.by);
        loop {
            if let Some(r) = poll.ready(&self.app, req, std::time::Instant::now() >= poll.deadline) {
                return Ok(r);
            }
            std::thread::sleep(std::time::Duration::from_millis(50));
        }
    }
}

/// One `WaitEvents` long poll: where it resumes, whose writes it skips, and
/// when it gives up.
struct EventPoll {
    after: u64,
    client: String,
    deadline: std::time::Instant,
}

impl EventPoll {
    fn start(app: &App, req: &pb::WaitEventsRequest, by: &str) -> EventPoll {
        let timeout = std::time::Duration::from_millis(u64::from(if req.timeout_ms == 0 { 30_000 } else { req.timeout_ms }).min(120_000));
        let deadline = std::time::Instant::now() + timeout;
        let client = if req.client.is_empty() { by.to_owned() } else { req.client.clone() };
        let after = {
            let st = app.state.lock().unwrap();
            if req.after == 0 && !client.is_empty() { st.cursors.get(&client).copied().unwrap_or(0) } else { req.after }
        };
        EventPoll { after, client, deadline }
    }

    /// The answer, if the poll is over: something matched, or `expired`.
    fn ready(&self, app: &App, req: &pb::WaitEventsRequest, expired: bool) -> Option<pb::WaitEventsResponse> {
        let (after, client) = (self.after, &self.client);
        let backlog: Vec<Event> = {
            let st = app.state.lock().unwrap();
            st.events.iter().filter(|e| e.seq > after && (client.is_empty() || &e.by != client)).cloned().collect()
        };
        // The filter decides whether this poll is over, not just what it
        // carries. Applying `kinds` after would release a caller waiting
        // for `client_failure` the moment *any* event existed, with an
        // empty list -- a long poll that returns immediately, and a busy
        // loop. Nothing matching means keep waiting.
        let matching: Vec<&Event> = backlog.iter().filter(|e| req.kinds.is_empty() || req.kinds.contains(&e.kind)).collect();
        if matching.is_empty() && !expired {
            return None;
        }
        // The cursor covers everything examined, matching or not: it
        // means "considered up to here", so a filtered reader does not
        // re-examine the same rejects for ever. A reader that wants
        // those events back asks for them with an explicit `after`.
        let cursor = backlog.last().map(|e| e.seq).unwrap_or(after);
        // The log is a window and drops from the front, so a reader
        // resuming from older than the window gets a stream that
        // begins later than it asked for. Saying so is the whole of
        // `eventlog.qnt`'s `nothingIsLostSilently`: not seeing an
        // event is survivable, believing you have seen it is not.
        let (first_seq, truncated) = {
            let st = app.state.lock().unwrap();
            let first = st.events.first().map(|e| e.seq).unwrap_or(0);
            (first, first > 0 && after > 0 && after + 1 < first)
        };
        let events: Vec<pb::Event> = matching
            .iter()
            .map(|e| pb::Event {
                seq: e.seq,
                at: e.at.clone(),
                kind: e.kind.clone(),
                data: Some(cadpole_ops::json_to_struct(&e.data)),
                by: e.by.clone(),
            })
            .collect();
        Some(pb::WaitEventsResponse { events, cursor, truncated, first_seq })
    }
}

/// `WaitEvents` as the server runs it: awaiting the broadcast channel, the way
/// `poll_events` always has, rather than parking a blocking thread for up to
/// two minutes. The listener is granted this call, and a few hundred parked
/// polls on blocking threads used to fill tokio's pool and stall every other
/// operation, the owner's included.
async fn wait_events_async(app: &Shared, req: &pb::WaitEventsRequest, by: &str) -> pb::WaitEventsResponse {
    // Subscribe before the first look, so an event emitted in between wakes us.
    let mut rx = app.events.subscribe();
    let poll = EventPoll::start(app, req, by);
    loop {
        let now = std::time::Instant::now();
        if let Some(r) = poll.ready(app, req, now >= poll.deadline) {
            return r;
        }
        let remaining = poll.deadline.saturating_duration_since(now);
        match tokio::time::timeout(remaining, rx.recv()).await {
            // Woken, lagged or timed out: look again; the deadline check above
            // ends the poll.
            Ok(Ok(_)) | Ok(Err(broadcast::error::RecvError::Lagged(_))) | Err(_) => continue,
            Ok(Err(broadcast::error::RecvError::Closed)) => {
                return poll.ready(app, req, true).unwrap_or_default();
            }
        }
    }
}

/// [`wait_events_async`] in either codec `run_op` speaks.
async fn wait_events_op(app: &Shared, m: &cadpole_ops::MethodInfo, codec: connect::Codec, body: &[u8], by: &str) -> anyhow::Result<Vec<u8>> {
    let req = match codec {
        connect::Codec::Proto => pb::WaitEventsRequest::decode(body)?,
        connect::Codec::Json => {
            let input: serde_json::Value = if body.is_empty() { serde_json::json!({}) } else { serde_json::from_slice(body)? };
            // As `cadpole_ops::call_json` reads its input: `null` is `{}`.
            let input = if input.is_null() { serde_json::json!({}) } else { input };
            let bytes = cadpole_ops::json_to_proto(&m.input, &input).map_err(|e| anyhow::anyhow!("{}: bad input: {e}", m.name))?;
            pb::WaitEventsRequest::decode(&bytes[..])?
        }
    };
    let out = wait_events_async(app, &req, by).await.encode_to_vec();
    match codec {
        connect::Codec::Proto => Ok(out),
        connect::Codec::Json => Ok(serde_json::to_vec(&cadpole_ops::proto_to_json(&m.output, &out, &cadpole_ops::serialize_options())?)?),
    }
}

/// Width and height from a PNG's IHDR, without decoding the image.
fn png_size(png: &[u8]) -> (u32, u32) {
    if png.len() < 24 || !png.starts_with(b"\x89PNG\r\n\x1a\n") {
        return (0, 0);
    }
    let w = u32::from_be_bytes([png[16], png[17], png[18], png[19]]);
    let h = u32::from_be_bytes([png[20], png[21], png[22], png[23]]);
    (w, h)
}

/// `GET /api/ops`: the registry, with the same JSON Schemas MCP publishes.
async fn get_ops() -> Json<serde_json::Value> {
    let ops: Vec<serde_json::Value> = cadpole_ops::registry()
        .iter()
        .filter(|m| !m.hidden)
        .map(|m| {
            serde_json::json!({
                "name": m.name,
                "cli_name": m.cli_name,
                "mcp_name": m.snake,
                "summary": m.summary(),
                "description": m.doc,
                "idempotent": m.idempotent,
                "needs_session": m.needs_session,
                "image_field": m.image_field,
                "input": m.input,
                "output": m.output,
                "input_schema": cadpole_ops::json_schema(&m.input).unwrap_or_default(),
            })
        })
        .collect();
    Json(serde_json::json!({"service": cadpole_ops::SERVICE, "ops": ops}))
}

/// Run one operation, whichever transport asked for it.
///
/// Both front doors end here: the `/api/ops/<Rpc>` route this server has
/// always had, and the Connect route beside it. One dispatch through
/// `cadpole_ops`, one render queue, one body limit, one set of errors — the
/// framings differ in how the bytes are wrapped and in nothing else. That is
/// the property that makes a second transport cheap and a third one thinkable:
/// there is no per-transport table of methods to keep in step, because there
/// is no table at all.
async fn run_op(
    app: &Shared,
    m: &'static cadpole_ops::MethodInfo,
    codec: connect::Codec,
    body: Bytes,
    by: String,
    role: cadpole_ops::capability::Role,
) -> Result<Vec<u8>, connect::Failure> {
    let name = m.name.clone();
    // The long poll waits on the event channel, not on a thread.
    if name == "WaitEvents" {
        return wait_events_op(app, m, codec, &body, &by)
            .await
            .map_err(|e| connect::Failure::new(connect::classify(&e), format!("{e:#}")));
    }
    // Rendering is the expensive shape; it goes through the same gate as
    // /api/render so a loop of tool calls cannot peg the machine.
    let heavy = matches!(name.as_str(), "Render" | "Sweep" | "Animate");
    let work = {
        let app = app.clone();
        move || -> anyhow::Result<Vec<u8>> {
            let host = ServerHost { app, by, role };
            match codec {
                connect::Codec::Proto => cadpole_ops::call_proto(&name, &body, &host),
                connect::Codec::Json => {
                    let input: serde_json::Value =
                        if body.is_empty() { serde_json::json!({}) } else { serde_json::from_slice(&body)? };
                    Ok(serde_json::to_vec(&cadpole_ops::call_json(&name, &input, &host)?)?)
                }
            }
        }
    };
    let res: anyhow::Result<Vec<u8>> = if heavy {
        match render_job(app, "the operation", work).await {
            Ok(v) => v,
            Err((status, why)) => return Err(connect::Failure::new(connect::Code::of_status(status), why)),
        }
    } else {
        // The listener's calls take a slot first, waiting without a thread.
        // The permit moves into the blocking task, so it is held as long as
        // the thread is: a caller that stops waiting (a Connect timeout, a
        // closed connection) does not free a slot while the work runs on.
        let slot = match role {
            cadpole_ops::capability::Role::Listener => match app.listener_slots.clone().acquire_owned().await {
                Ok(p) => Some(p),
                Err(_) => return Err(connect::Failure::new(connect::Code::Unavailable, "the server is shutting down")),
            },
            _ => None,
        };
        let work = move || {
            let _slot = slot;
            work()
        };
        match tokio::task::spawn_blocking(work).await {
            Ok(v) => v,
            Err(e) => return Err(connect::Failure::new(connect::Code::Internal, e.to_string())),
        }
    };
    res.map_err(|e| connect::Failure::new(connect::classify(&e), format!("{e:#}")))
}

/// `POST /api/ops/{method}`: proto3 JSON in and out by default,
/// `application/x-protobuf` when the caller asks for the wire format.
///
/// Cadpole's own convention, kept as an alias for the clients that already speak
/// it — `cadpole op` against an older server, the listener's e2e test, a `curl`
/// in someone's notes — and for its lenient spellings, which the Connect route
/// deliberately does not take.
async fn post_op(
    State(app): State<Shared>,
    axum::extract::Path(method): axum::extract::Path<String>,
    role: Option<axum::Extension<cadpole_ops::capability::Role>>,
    headers: axum::http::HeaderMap,
    body: Bytes,
) -> Response {
    let Some(m) = cadpole_ops::method(&method) else {
        return (StatusCode::NOT_FOUND, Json(serde_json::json!({"error": format!("unknown operation {method:?}")}))).into_response();
    };
    if body.len() > app.max_body {
        return (StatusCode::PAYLOAD_TOO_LARGE, "request too large").into_response();
    }
    let content_type = headers.get(header::CONTENT_TYPE).and_then(|v| v.to_str().ok()).unwrap_or("").to_owned();
    let wants_proto = content_type.starts_with("application/x-protobuf")
        || content_type.starts_with("application/protobuf")
        // Connect's spelling, accepted here too so one content type works on
        // both routes. The default is still JSON when nothing is said, which
        // is what this route's existing callers rely on.
        || content_type.starts_with("application/proto");
    let codec = if wants_proto { connect::Codec::Proto } else { connect::Codec::Json };
    let output = m.output.clone();

    match run_op(&app, m, codec, body, client_id(&headers), caller_role(role)).await {
        Ok(bytes) if wants_proto => ([(header::CONTENT_TYPE, "application/x-protobuf")], bytes).into_response(),
        Ok(bytes) => ([(header::CONTENT_TYPE, "application/json")], bytes).into_response(),
        // This route's error shape predates Connect and is left alone: its
        // callers read `error`, and the whole point of the alias is that it
        // does not change under them.
        Err(f) => {
            (StatusCode::BAD_REQUEST, Json(serde_json::json!({"error": f.message, "op": method, "output": output}))).into_response()
        }
    }
}

/// `POST /api/cadpole.v1.CadpoleOps/{method}`: the Connect protocol, unary.
///
/// The whole of the transport is here and in `connect.rs`: decide the framing
/// from the content type, check the protocol version and the deadline, unwrap
/// the request, run the same operation `/api/ops` would have run, and wrap the
/// answer back up. Everything that decides *whether* the call may happen —
/// the origin rules, the bearer token, the capability guard — has already run
/// in `guard()`, because this route lives under `/api/` like every other one.
async fn post_connect(
    State(app): State<Shared>,
    axum::extract::Path(method): axum::extract::Path<String>,
    role: Option<axum::Extension<cadpole_ops::capability::Role>>,
    headers: axum::http::HeaderMap,
    body: Bytes,
) -> Response {
    let content_type = headers.get(header::CONTENT_TYPE).and_then(|v| v.to_str().ok()).unwrap_or("");
    let wire = match connect::wire_of(content_type) {
        Ok(w) => w,
        // No framing was agreed, so the error goes out in the one every
        // Connect client can read, with the status HTTP assigns to it.
        Err(f) => {
            return (StatusCode::UNSUPPORTED_MEDIA_TYPE, [(header::CONTENT_TYPE, "application/json")], f.envelope())
                .into_response();
        }
    };
    let fail = |f: connect::Failure| connect::error_response(wire, &f);
    if let Err(f) = connect::check_version(&headers) {
        return fail(f);
    }
    let timeout = match connect::timeout_of(&headers) {
        Ok(t) => t,
        Err(f) => return fail(f),
    };
    let Some(m) = connect::lookup(&method) else {
        return fail(connect::Failure::new(
            connect::Code::Unimplemented,
            format!("{} has no method {method:?}", cadpole_ops::SERVICE),
        ));
    };
    if body.len() > app.max_body {
        return fail(connect::Failure::new(connect::Code::ResourceExhausted, "request too large"));
    }
    let body = match wire.framing {
        connect::Framing::Connect => body,
        connect::Framing::GrpcWeb => match connect::unframe(&body) {
            Ok(message) => Bytes::copy_from_slice(message),
            Err(f) => return fail(f),
        },
    };

    let call = run_op(&app, m, wire.codec, body, client_id(&headers), caller_role(role));
    let res = match timeout {
        None => call.await,
        Some(budget) => match tokio::time::timeout(budget, call).await {
            Ok(v) => v,
            // The caller is released; the blocking worker it abandons is
            // stopped by the evaluation deadline, exactly as `render_job`
            // documents for its own budget.
            Err(_) => Err(connect::Failure::new(
                connect::Code::DeadlineExceeded,
                format!("{} exceeded the caller's {} ms deadline", m.name, budget.as_millis()),
            )),
        },
    };
    match res {
        Ok(bytes) => connect::ok_response(wire, bytes),
        Err(f) => fail(f),
    }
}

/// The files the page can tell apart, in the order they appear in the
/// fingerprint.
///
/// What each one costs to replace is what the page's plan is made of,
/// so this list is a contract with `src/web/src/hot-reload.ts` rather than an
/// implementation detail: a name added here that the page does not recognise
/// makes that page reload rather than guess.
const WEB_BUILD_FILES: [&str; 5] = ["index.html", "app.js", "styles.css", "cadpole_wasm_bg.wasm", "cadpole_wasm.js"];

/// FNV-1a over eight bytes at a time, with the tail and the length folded in.
///
/// Not a cryptographic hash and not trying to be: this answers "did these
/// bytes change", against a build system rather than an adversary. Eight bytes
/// per round is what keeps 19 MB of wasm down to a couple of milliseconds,
/// which is what makes hashing affordable at all.
fn content_hash(bytes: &[u8]) -> u64 {
    const PRIME: u64 = 0x0000_0100_0000_01b3;
    let mut h: u64 = 0xcbf2_9ce4_8422_2325;
    let mut chunks = bytes.chunks_exact(8);
    for c in &mut chunks {
        h ^= u64::from_le_bytes(c.try_into().unwrap_or([0; 8]));
        h = h.wrapping_mul(PRIME);
    }
    for &b in chunks.remainder() {
        h ^= u64::from(b);
        h = h.wrapping_mul(PRIME);
    }
    // The length too, so a tail of zero bytes cannot collide with no tail.
    h ^= bytes.len() as u64;
    h.wrapping_mul(PRIME)
}

/// What one file's hash was last taken over.
#[derive(Clone, Copy, PartialEq, Eq)]
struct FileStamp {
    len: u64,
    mtime_ns: u128,
    hash: u64,
}

/// How long a file's modification time must have stood still before it is
/// trusted as a cache key.
///
/// A timestamp is not a version number. A file system may record `st_mtime` at
/// the kernel's tick granularity, not at nanosecond resolution: rewriting a file
/// to the same length immediately after reading it produces the *same* mtime,
/// to the nanosecond, and a cache keyed on (size, mtime) never notices.
///
/// So a file whose timestamp is younger than this is re-hashed rather than
/// trusted. In steady state nothing is that young and every poll is five
/// `stat` calls; around a rebuild the bundle is hashed once or twice. It is
/// the standard build-tool answer to coarse timestamps, and it is portable,
/// which an inode check is not.
const MTIME_SETTLED: Duration = Duration::from_secs(2);

/// A fingerprint of the built web UI: one field per file, each naming that
/// file's size and a hash of its **contents**.
///
/// ```text
/// index.html:1417:9f3a…|app.js:1216260:…|styles.css:30209:…|cadpole_wasm_bg.wasm:19884615:…|cadpole_wasm.js:18872:…
/// ```
///
/// Not size and *modification time*, which is cheap and wrong once the page
/// reads the fields separately. `//src/web:dist` is one `copy_to_directory`
/// action, so editing one line of CSS re-copies all five files and stamps all
/// five mtimes: every rebuild would *look* like every file had changed, and a
/// page trying to do the least that suffices would have nothing to go on.
/// Hashing the bytes says what actually moved — and a rebuild that reproduces
/// identical output tells open pages nothing at all.
///
/// The cost is bounded by only reading what a `stat` says might have moved:
/// the watcher polls every two seconds, a poll is five `stat` calls, and the
/// 21 MB of bytes are hashed on the handful of occasions an hour that
/// something was really rebuilt.
struct BuildFingerprint {
    dir: PathBuf,
    stamps: std::collections::HashMap<&'static str, FileStamp>,
}

impl BuildFingerprint {
    fn new(dir: PathBuf) -> Self {
        Self { dir, stamps: std::collections::HashMap::new() }
    }

    /// The fingerprint as of now, re-reading only files whose size or
    /// modification time moved since the last call.
    fn current(&mut self) -> String {
        let mut parts: Vec<String> = Vec::with_capacity(WEB_BUILD_FILES.len());
        for name in WEB_BUILD_FILES {
            let p = self.dir.join(name);
            let Ok(m) = std::fs::metadata(&p) else {
                self.stamps.remove(name);
                parts.push(format!("{name}:-"));
                continue;
            };
            let len = m.len();
            let modified = m.modified().ok();
            let mtime_ns = modified
                .and_then(|t| t.duration_since(std::time::UNIX_EPOCH).ok())
                .map(|d| d.as_nanos())
                .unwrap_or(0);
            // A timestamp younger than the clock's own granularity proves
            // nothing; re-read instead of trusting it.
            let settled = modified
                .and_then(|t| std::time::SystemTime::now().duration_since(t).ok())
                .is_some_and(|age| age >= MTIME_SETTLED);
            let hash = match self.stamps.get(name) {
                Some(s) if settled && s.len == len && s.mtime_ns == mtime_ns => s.hash,
                _ => match std::fs::read(&p) {
                    Ok(bytes) => content_hash(&bytes),
                    // Present but unreadable, or half-written by a build that
                    // is still running. Say so in-band rather than inventing a
                    // hash: the page sees a field it cannot classify and
                    // reloads instead of guessing.
                    Err(_) => {
                        self.stamps.remove(name);
                        parts.push(format!("{name}:?"));
                        continue;
                    }
                },
            };
            self.stamps.insert(name, FileStamp { len, mtime_ns, hash });
            parts.push(format!("{name}:{len}:{hash:016x}"));
        }
        parts.join("|")
    }
}

/// Watch the built web UI and tell every open page when it changes.
///
/// Without this, a browser goes on running the old bundle after a rebuild
/// until someone hard-reloads it. A
/// served bundle that has changed under a page is a bug the page cannot see,
/// so the server says so and the page reloads itself.
///
/// Polling rather than an inotify watch: it is five stats every couple of
/// seconds against one directory, it behaves the same on every platform, and
/// it cannot miss a rename the way a watch on a replaced file can — which is
/// exactly how Bazel writes its outputs.
///
/// The page decides what to *do* about it, and most of the time
/// that is not a reload: the fingerprint is per file, so a stylesheet change
/// swaps a `<link>`, an engine change swaps the worker, and only the UI bundle
/// itself still costs a page load. Hashing contents rather than mtimes is what
/// makes that possible, and it also keeps this quiet when a rebuild produced
/// the same bytes.
fn spawn_build_watcher(app: Arc<App>, mut fingerprint: BuildFingerprint) {
    tokio::spawn(async move {
        let mut current = fingerprint.current();
        let mut ticker = tokio::time::interval(std::time::Duration::from_secs(2));
        ticker.set_missed_tick_behavior(tokio::time::MissedTickBehavior::Delay);
        loop {
            ticker.tick().await;
            let next = fingerprint.current();
            if next == current {
                continue;
            }
            let moved = changed_web_files(&current, &next);
            current = next.clone();
            let mut st = app.state.lock().unwrap_or_else(|e| e.into_inner());
            st.web_build = next.clone();
            app.emit(&mut st, "build_changed", serde_json::json!({ "build": next }));
            drop(st);
            out::line(format!("web ui rebuilt ({}); told {} open page(s)", moved.join(", "), app.events.receiver_count()));
        }
    });
}

/// Which files differ between two fingerprints, for the log line.
///
/// The page runs the same comparison for real, in `src/web/src/hot-reload.ts`;
/// this exists so the terminal says `styles.css` rather than "something".
fn changed_web_files(from: &str, to: &str) -> Vec<String> {
    let field = |s: &str| -> std::collections::HashMap<String, String> {
        s.split('|')
            .filter_map(|p| p.split_once(':').map(|(name, rest)| (name.to_owned(), rest.to_owned())))
            .collect()
    };
    let (a, b) = (field(from), field(to));
    let mut out: Vec<String> = Vec::new();
    for name in WEB_BUILD_FILES {
        if a.get(name) != b.get(name) {
            out.push(name.to_owned());
        }
    }
    if out.is_empty() {
        out.push("unknown".to_owned());
    }
    out
}

pub(crate) fn web_dir(explicit: Option<PathBuf>) -> Option<PathBuf> {
    if let Some(p) = explicit {
        return Some(p);
    }
    if let Ok(r) = runfiles::Runfiles::create() {
        let p = runfiles::rlocation!(r, "cadpole/src/web/dist");
        if let Some(p) = p {
            if p.join("index.html").exists() {
                return Some(p);
            }
        }
    }
    None
}

fn tailscale_ip() -> Option<IpAddr> {
    // Without pulling in a netlink crate: parse `ip -4 addr` output.
    let out = std::process::Command::new("ip").args(["-4", "-o", "addr"]).output().ok()?;
    let text = String::from_utf8_lossy(&out.stdout);
    for line in text.lines() {
        if line.contains("tailscale") {
            if let Some(cidr) = line.split_whitespace().find(|w| w.contains('/')) {
                if let Ok(ip) = cidr.split('/').next().unwrap_or("").parse::<IpAddr>() {
                    if is_tailnet(&ip) {
                        return Some(ip);
                    }
                }
            }
        }
    }
    None
}

/// Tailscale hands out addresses in the CGNAT range 100.64.0.0/10. Checking it
/// is what stops `--listen tailscale` binding whatever else happens to be
/// called `tailscale0` on the box (docs/security.md, "Network binding").
fn is_tailnet(ip: &IpAddr) -> bool {
    match ip {
        IpAddr::V4(v4) => {
            let o = v4.octets();
            o[0] == 100 && (64..128).contains(&o[1])
        }
        IpAddr::V6(_) => false,
    }
}

/// How many render jobs run at once: `min(2, max(1, cores / 4))`, so an
/// assistant hammering the render endpoint still leaves the machine usable for
/// the person sitting at it.
fn render_jobs(explicit: usize) -> usize {
    if explicit > 0 {
        return explicit;
    }
    (cadpole_engine::available_threads() / 4).clamp(1, 2)
}

pub async fn serve(args: ServeArgs) -> anyhow::Result<()> {
    let (document, save_path) = match &args.input {
        Some(p) => {
            let d = crate::input::load(p)?;
            let save = args.save_path.clone().or_else(|| (p.extension().and_then(|e| e.to_str()) == Some(DOCUMENT_EXT)).then(|| p.clone()));
            (d, save)
        }
        None => (cadpole_engine::document_from_scripts("untitled", vec![("main.star".into(), "# Cadpole script\nb = box(20, 20, 10)\n".into())]), args.save_path.clone()),
    };

    // ---- binding and authentication (docs/security.md, "Network binding" and "Authentication")
    let ip: IpAddr = match args.listen {
        Listen::Loopback => "127.0.0.1".parse().unwrap(),
        Listen::All => "0.0.0.0".parse().unwrap(),
        Listen::Tailscale => match tailscale_ip() {
            Some(ip) => ip,
            None => {
                note!("no tailscale interface in 100.64.0.0/10 found; listening on loopback");
                "127.0.0.1".parse().unwrap()
            }
        },
    };
    let loopback = ip.is_loopback();
    if !loopback && args.no_auth {
        anyhow::bail!(
            "--no-auth is only allowed on loopback: binding {ip} with no token would expose this \
             document, and every render job on this machine, to the whole network"
        );
    }
    if args.listen == Listen::All {
        note!("WARNING: binding 0.0.0.0 exposes Cadpole to every network this machine is on.");
        note!("         A bearer token is required; anyone who has it can change the document.");
    }

    // This run's name: every file it puts in the runtime directory carries it.
    let instance = crate::runtime::new_instance();
    // The runtime directory holds the socket, the record and the agent tokens.
    // A socket needs it to be private, so a directory that is not this user's
    // own 0700 directory stops the server (docs/security.md, "Runtime
    // directory"); with no socket it is only a warning, as before.
    let runtime = crate::security::runtime_dir();
    let want_socket = cfg!(unix) && !args.no_socket;
    if want_socket {
        let dir = runtime.as_ref().ok_or_else(|| anyhow::anyhow!("no runtime directory (XDG_RUNTIME_DIR, XDG_STATE_HOME or HOME); pass --no-socket"))?;
        crate::runtime::prepare_private_dir(dir)?;
    }

    // Three tokens, minted for this run: the browser's, the agent owner's, and
    // the restricted one the limited listener gets.
    let minted = if args.no_auth { None } else { Some(security::mint_tokens(args.token.clone(), Some(&instance))) };
    let tokens = match &minted {
        None => security::TokenSet::open(),
        Some(m) => security::TokenSet::for_run(&m.browser, &m.owner, &m.listener, args.agent_tcp),
    };

    let (tx, _) = broadcast::channel(1024);
    let jobs = render_jobs(args.render_jobs);
    let app: Shared = Arc::new(App {
        state: Mutex::new(SessionState {
            // Filled in below once the web directory is known; empty means no
            // bundle is being served.
            web_build: String::new(),
            id: random_id(8),
            revision: 1,
            document,
            selection: Selection::default(),
            messages: Vec::new(),
            events: Vec::new(),
            clients: 0,
            save_path,
            assistant_listening: false,
            camera: None,
            cursors: Default::default(),
            failures: FailureLimits::default(),
            streams: Default::default(),
        }),
        events: tx,
        live: broadcast::channel(256).0,
        tokens,
        allowed_origins: args.allow_origin.clone(),
        root: DocumentRoot::new(args.root.clone()),
        render_slots: tokio::sync::Semaphore::new(jobs),
        memo: Arc::new(cadpole_engine::cadpole_core::memo::Memo::new()),
        render_waiting: AtomicUsize::new(0),
        render_budget: Duration::from_millis(args.render_budget_ms.max(1)),
        listener_slots: Arc::new(tokio::sync::Semaphore::new(LISTENER_OP_SLOTS)),
        max_body: args.max_body,
    });

    // Every evaluation in this process is bounded from here on: a render or an
    // export carries no deadline of its own, so the process default is what
    // stops a runaway script (docs/security.md, "Starlark sandbox").
    // The heap is the process's: 512 MB, or `cadpole --max-heap-mb`.
    cadpole_engine::limits::set_default_limits(cadpole_engine::Limits {
        deadline_ms: args.eval_deadline_ms,
        max_heap_bytes: crate::heap_bytes(),
        ..cadpole_engine::Limits::structural()
    });

    let solve_jobs = start_solve_jobs(&app, &args);

    // A streamed message whose author went away is ended as failed rather
    // than left in progress (reply_stream.rs). A lock and a counter read per
    // second while nothing is streaming.
    {
        let app = app.clone();
        tokio::spawn(async move {
            let mut tick = tokio::time::interval(Duration::from_secs(1));
            loop {
                tick.tick().await;
                app.expire_streams(std::time::Instant::now());
            }
        });
    }

    let router = build_router(app.clone(), args.web_dir.clone());
    if let Some(dir) = web_dir(args.web_dir.clone()) {
        // One fingerprint, hashed once here and handed to the watcher, so the
        // 21 MB of bundle is read at start-up and then only when a `stat`
        // says something moved.
        let mut fingerprint = BuildFingerprint::new(dir);
        {
            let mut st = app.state.lock().unwrap_or_else(|e| e.into_inner());
            st.web_build = fingerprint.current();
        }
        spawn_build_watcher(app.clone(), fingerprint);
    }

    // ---- the two surfaces
    let stop = StopSignal::install();
    if let Some(dir) = &runtime {
        for p in crate::runtime::sweep_stale(dir) {
            note!("removed {} (left by a server that is no longer running)", p.display());
        }
    }
    let files = runtime.as_ref().map(|d| crate::runtime::RunFiles::new(d, &instance));
    let mut cleanup = RunCleanup { files: files.clone(), socket: None };
    #[cfg(unix)]
    let unix = if want_socket {
        let path = match &args.socket {
            Some(p) => p.clone(),
            None => crate::runtime::default_socket_path(files.as_ref().expect("a socket implies a runtime directory"))?,
        };
        let l = bind_agent_socket(&path)?;
        cleanup.socket = Some(path.clone());
        Some((l, path))
    } else {
        None
    };
    #[cfg(not(unix))]
    let unix: Option<((), PathBuf)> = None;
    let tcp = if args.no_tcp { None } else { Some(tokio::net::TcpListener::bind(SocketAddr::new(ip, args.port)).await?) };
    // The *bound* address, not the requested one: with `--port 0` the kernel
    // picks, and a banner that says `:0` is a URL nobody can open.
    let addr = tcp.as_ref().map(|l| l.local_addr()).transpose()?;
    let socket_path = unix.as_ref().map(|(_, p)| p.clone());

    // The record goes last, once both surfaces answer: a client that reads it
    // can connect at once, and a record never describes a server that is
    // not yet there.
    let document = args.input.as_ref().and_then(|p| std::fs::canonicalize(p).ok());
    let record = crate::runtime::Record {
        instance: instance.clone(),
        pid: std::process::id(),
        socket: socket_path.clone(),
        http: addr.map(|a| format!("http://{a}")),
        agent_tcp: args.agent_tcp,
        document,
        started: now(),
    };
    if let Some(f) = &files {
        if let Err(e) = crate::runtime::write_private_atomic(&f.record(), &serde_json::to_vec_pretty(&record)?) {
            note!("warning: could not write {}: {e}; clients will need --server", f.record().display());
        }
    }
    // A token is never printed to something that is not a terminal: there it
    // is a log. The link goes to a private file instead.
    let url_file = match (&minted, &files, addr) {
        (Some(m), Some(f), Some(a)) if !std::io::IsTerminal::is_terminal(&std::io::stderr()) => {
            match security::write_private(&f.url(), format!("http://{a}/#token={}", m.browser).as_bytes()) {
                Ok(()) => Some(f.url()),
                Err(e) => {
                    note!("warning: could not write {}: {e}", f.url().display());
                    None
                }
            }
        }
        _ => None,
    };
    let addr_text = addr.map(|a| a.to_string());
    for l in startup_banner(addr_text.as_deref(), socket_path.as_deref(), minted.as_ref(), url_file.as_deref()) {
        out::line(l);
    }
    note!(
        "limits: eval {} ms, render budget {} ms, {jobs} render job(s) + {RENDER_QUEUE} queued, body {} MB",
        args.eval_deadline_ms,
        args.render_budget_ms,
        args.max_body / (1024 * 1024)
    );
    note!("documents: {}", app.root.path().display());
    note!("{solve_jobs}");

    // One stop signal for both surfaces. The files go first, the moment it
    // arrives, so no client discovers a server that is shutting down.
    let (stop_tx, stop_rx) = tokio::sync::watch::channel(false);
    {
        let cleanup = cleanup.clone();
        tokio::spawn(async move {
            stop.wait().await;
            cleanup.remove();
            let _ = stop_tx.send(true);
            // Graceful, but bounded: a client's long poll (`WaitEvents` waits
            // up to 30 s) or an open event stream would otherwise hold the
            // process for as long as it likes. Nothing new can find this
            // server any more; what is still open after the grace period is
            // cut, and its client reports the failure and reconnects.
            tokio::time::sleep(STOP_GRACE).await;
            out::line(format!("stopping: requests still open after {} s are cut off", STOP_GRACE.as_secs()));
            std::process::exit(0);
        });
    }
    let stopped = |mut rx: tokio::sync::watch::Receiver<bool>| async move {
        let _ = rx.wait_for(|s| *s).await;
    };
    let web = tcp.map(|l| {
        let s = axum::serve(l, router).with_graceful_shutdown(stopped(stop_rx.clone()));
        tokio::spawn(async move { s.await })
    });
    #[cfg(unix)]
    let agents = unix.map(|(l, _)| {
        let s = axum::serve(l, build_agent_router(app.clone())).with_graceful_shutdown(stopped(stop_rx.clone()));
        tokio::spawn(async move { s.await })
    });
    #[cfg(not(unix))]
    let agents: Option<tokio::task::JoinHandle<std::io::Result<()>>> = {
        let _ = unix;
        None
    };
    if let Some(h) = web {
        h.await??;
    }
    if let Some(h) = agents {
        h.await??;
    }
    cleanup.remove();
    Ok(())
}

/// How long a stopping server waits for open requests before it exits.
const STOP_GRACE: Duration = Duration::from_secs(3);

/// Ctrl-C, or SIGTERM where there is one: the two ways a server is asked to
/// stop. A SIGKILL leaves its files behind, which is what
/// [`crate::runtime::sweep_stale`] and the connect test in discovery are for.
///
/// Installed before the server binds or writes anything: a handler installed
/// later leaves a window in which a SIGTERM takes the default action and
/// kills the process with its socket, record and tokens still on disk. A
/// SIGTERM sent the moment the banner appeared did exactly that.
struct StopSignal {
    #[cfg(unix)]
    signals: Option<(tokio::signal::unix::Signal, tokio::signal::unix::Signal)>,
}

impl StopSignal {
    fn install() -> StopSignal {
        #[cfg(unix)]
        {
            use tokio::signal::unix::{SignalKind, signal};
            StopSignal { signals: signal(SignalKind::terminate()).ok().zip(signal(SignalKind::interrupt()).ok()) }
        }
        #[cfg(not(unix))]
        {
            StopSignal {}
        }
    }

    async fn wait(self) {
        #[cfg(unix)]
        if let Some((mut term, mut int)) = self.signals {
            tokio::select! {
                _ = term.recv() => {}
                _ = int.recv() => {}
            }
            return;
        }
        let _ = tokio::signal::ctrl_c().await;
    }
}

/// The files of this run, removed on the way out (and by `Drop`, for an
/// error return). The socket is removed only while it is still the one this
/// run bound: a path is a name, and the name could have been taken over.
#[derive(Clone)]
struct RunCleanup {
    files: Option<crate::runtime::RunFiles>,
    socket: Option<PathBuf>,
}

impl RunCleanup {
    fn remove(&self) {
        if let Some(f) = &self.files {
            for p in [f.record(), f.url(), f.session_token(), f.listener_token()] {
                let _ = std::fs::remove_file(p);
            }
        }
        if let Some(s) = &self.socket {
            // Only a socket that is still ours: never a file someone else put
            // at the path, never a socket of another live server.
            if crate::runtime::check_private_file(s, true).is_ok() {
                let _ = std::fs::remove_file(s);
            }
        }
    }
}

impl Drop for RunCleanup {
    fn drop(&mut self) {
        self.remove();
    }
}

/// Bind the agent socket at `path`, 0600, in a private directory.
///
/// A file already at `path` is a socket some server left. If anything answers
/// a connect there, another server is live and this one refuses to start; if
/// nothing does and it is this user's private socket, it is stale and is
/// removed. Anything else at the path is refused, never deleted.
#[cfg(unix)]
fn bind_agent_socket(path: &Path) -> anyhow::Result<tokio::net::UnixListener> {
    let dir = path.parent().filter(|d| !d.as_os_str().is_empty()).unwrap_or(Path::new("."));
    crate::runtime::check_private_dir(dir).map_err(|e| anyhow::anyhow!("the agent socket's directory: {e}"))?;
    if path.as_os_str().len() > crate::runtime::MAX_SOCKET_PATH {
        anyhow::bail!("{} is too long for a Unix socket path; pass a shorter --socket", path.display());
    }
    if std::fs::symlink_metadata(path).is_ok() {
        if crate::runtime::socket_is_live(path) {
            anyhow::bail!("another `cadpole serve` is listening on {}; stop it or pass another --socket", path.display());
        }
        crate::runtime::check_private_file(path, true)
            .map_err(|e| anyhow::anyhow!("{} is in the way and is not a stale socket of yours: {e}", path.display()))?;
        std::fs::remove_file(path)?;
        note!("removed a stale socket at {}", path.display());
    }
    let l = tokio::net::UnixListener::bind(path).map_err(|e| anyhow::anyhow!("cannot bind {}: {e}", path.display()))?;
    use std::os::unix::fs::PermissionsExt as _;
    std::fs::set_permissions(path, std::fs::Permissions::from_mode(0o600))?;
    crate::runtime::check_private_file(path, true)?;
    Ok(l)
}

/// Give this server a solve-job runner, and say what it is: a solve
/// too long for one evaluation, or one written `solve(job=True)`, runs in its
/// own `cadpole opt-job` process, and when it lands -- or a job another process
/// runs that this server's evaluations are waiting on -- the session's
/// document is evaluated again, which reads the answer from the cache, warms
/// the memo and starts whatever that answer unblocks; then `solve_finished`
/// tells every client, and the viewer evaluates and shows it.
fn start_solve_jobs(app: &Shared, args: &ServeArgs) -> String {
    use crate::opt_jobs::jobs::{Record, Settings};
    if args.no_solve_jobs {
        return "solve jobs: off (--no-solve-jobs); a solve too long for an evaluation stays pending".into();
    }
    let Some(solver) = crate::opt_jobs::installed() else { return "solve jobs: off (no solver)".into() };
    let mut settings = Settings::new(format!("cadpole serve (pid {})", std::process::id()));
    if args.solve_jobs > 0 {
        settings.parallel = args.solve_jobs;
    }
    settings.time_limit = (args.job_time_limit > 0).then_some(args.job_time_limit as f64);
    settings.mem_mb = args.job_mem_mb;
    settings.inline_cap = (args.solve_inline_ms as f64 / 1000.0).max(0.001);
    settings.when_landed = "cadpole serve evaluates the document again when it lands".into();
    // A job outlives a Ctrl-C to the server and still files its answer,
    // which the next `cadpole serve` of the document reads.
    settings.own_process_group = true;
    let Some(runner) = solver.start_runner(settings) else {
        return "solve jobs: off (no solution cache to deliver answers through: CADPOLE_OPT_CACHE=off, or it could not be made)"
            .into();
    };
    runner.jobs().collect();
    runner.cache().collect();
    {
        let app = app.clone();
        runner.on_started(Arc::new(move |r: &Record| {
            let mut st = app.state.lock().unwrap_or_else(|e| e.into_inner());
            app.emit(
                &mut st,
                "solve_started",
                serde_json::json!({"job": r.id, "origin": r.origin, "time_limit_s": r.time_limit_s, "owner": r.owner}),
            );
        }));
    }
    let (tx, rx) = std::sync::mpsc::channel::<Record>();
    runner.on_finished(Arc::new(move |r: &Record| {
        let _ = tx.send(r.clone());
    }));
    let worker = app.clone();
    std::thread::Builder::new()
        .name("solve landings".into())
        .spawn(move || solve_landings(worker, rx))
        .expect("a thread for solve landings");
    format!(
        "solve jobs: {} at once, {} each, {} MB, inline up to {} ms; cache {}",
        runner.settings.parallel,
        runner.settings.time_limit.map(|t| format!("{t:.0} s")).unwrap_or_else(|| "no time limit".into()),
        runner.settings.mem_mb,
        args.solve_inline_ms,
        runner.cache().dir().display()
    )
}

/// This server's solve-job runner, or why it has none.
fn server_runner() -> anyhow::Result<Arc<crate::opt_jobs::jobs::Runner>> {
    crate::opt_jobs::installed()
        .and_then(|s| s.runner().cloned())
        .ok_or_else(|| anyhow::anyhow!("this cadpole serve runs no solve jobs (--no-solve-jobs, or no solution cache)"))
}

/// The thread that answers landings: gather the jobs that end close
/// together, evaluate the session's document once for all of them, and tell
/// every client.
fn solve_landings(app: Shared, rx: std::sync::mpsc::Receiver<crate::opt_jobs::jobs::Record>) {
    while let Ok(first) = rx.recv() {
        let mut landed = vec![first];
        while let Ok(r) = rx.recv_timeout(Duration::from_millis(200)) {
            landed.push(r);
        }
        let doc = {
            let st = app.state.lock().unwrap_or_else(|e| e.into_inner());
            ServerHost::document_locked(&st)
        };
        let (ctx, _, _) = cadpole_engine::evaluate_document_options(
            &doc,
            &Default::default(),
            0.0,
            0.0,
            &Default::default(),
            cadpole_engine::limits::default_limits(),
            cadpole_engine::EvalOptions { memo: Some(app.memo.clone()), preview: false, solve_jobs: true },
        );
        let pending = crate::opt_jobs::pending_jobs(ctx.diagnostics.iter().map(|d| d.message.as_str()));
        let mut st = app.state.lock().unwrap_or_else(|e| e.into_inner());
        let revision = st.revision;
        for r in &landed {
            app.emit(
                &mut st,
                "solve_finished",
                serde_json::json!({
                    "job": r.id,
                    "state": r.state.as_str(),
                    "status": r.status,
                    "objective": r.objective,
                    "gap": r.gap,
                    "elapsed_ms": r.elapsed_ms,
                    "origin": r.origin,
                    "message": r.message,
                    "pending": pending,
                    "revision": revision,
                }),
            );
        }
    }
}

/// What the server says on start. **The only place a token is ever formatted
/// into text.**
///
/// The owner token is printed once, in the URL, because the user has to be
/// able to open the UI; the fragment keeps it out of the request line, out of
/// access logs and out of any `Referer`. The listener token is never printed —
/// only the path to its file — because the thing that reads it is a process,
/// not a person, and a token on a terminal is a token in a scrollback buffer.
fn startup_banner(
    addr: Option<&str>,
    socket: Option<&Path>,
    minted: Option<&security::SessionTokens>,
    url_file: Option<&Path>,
) -> Vec<String> {
    let mut lines = Vec::new();
    match addr {
        Some(addr) if minted.is_none() => lines.push(format!("cadpole serve listening on http://{addr}/ (NO AUTH -- loopback only)")),
        Some(addr) => lines.push(format!("cadpole serve listening on http://{addr}/")),
        None => lines.push("cadpole serve: no TCP port (--no-tcp); agents only".to_owned()),
    }
    if let Some(s) = socket {
        lines.push(format!("agents: unix:{} (0600) -- `cadpole mcp`, `cadpole op` and `cadpole listen` find it with no flags", s.display()));
    }
    let Some(m) = minted else { return lines };
    if let Some(addr) = addr {
        lines.push(match url_file {
            // Not a terminal: a log. The link goes to a private file instead.
            Some(f) => format!("open:  the link with this run's token is in {} (0600)", f.display()),
            None => format!("open:  http://{addr}/#token={}", m.browser),
        });
    }
    lines.push(match &m.owner_file {
        Some(p) => format!("token: {} (0600), new for this run; `cadpole op` and `cadpole mcp` read it automatically", p.display()),
        None => "token: not stored; pass CADPOLE_TOKEN=<the token in the URL above> to clients".to_owned(),
    });
    lines.push(match &m.listener_file {
        Some(p) => format!("listener: {} (0600) -- a restricted token; {} operations, no others", p.display(), cadpole_ops::capability::GRANTED.len()),
        None => "listener: not stored; the limited listener cannot start without CADPOLE_LISTENER_TOKEN".to_owned(),
    });
    if let Some(w) = &m.warning {
        lines.push(format!("warning: {w}"));
    }
    lines
}

/// Every route, the static UI and the guard, assembled. Separate from
/// [`serve`] so a test can drive the real router without a real network
/// interface or a real token file.
fn build_router(app: Shared, web: Option<PathBuf>) -> Router {
    let max_body = app.max_body;
    let api = agent_routes()
        .route("/api/session/script", axum::routing::put(put_script))
        .route("/api/session/params", axum::routing::put(put_params))
        .route("/api/session/selection", axum::routing::put(put_selection))
        .route("/api/session/messages", post(post_message))
        .route("/api/session/messages/{seq}/attachment/{i}", get(get_attachment))
        .route("/api/session/assistant", axum::routing::put(put_assistant))
        .route("/api/session/camera", axum::routing::put(put_camera))
        .route("/api/session/save", post(post_save))
        .route("/api/session/save-as", post(post_save_as))
        .route("/api/documents", get(get_documents))
        .route("/api/documents/{id}", get(get_document))
        .route("/api/events", get(get_events))
        .route("/api/events/poll", get(poll_events))
        .route("/api/model", get(get_model))
        .route("/api/render", post(post_render))
        .route("/api/call", post(post_call));

    let mut router = api;
    match web_dir(web) {
        Some(dir) => {
            let index = dir.join("index.html");
            // Compressed, because the wasm engine is 19.7 MB on the wire and
            // 5.3 MB gzipped. That is not a micro-optimisation: over a
            // tailnet to a phone the uncompressed version is the difference
            // between a slow load and one the user reports as "hangs
            // forever". Brotli first where the client offers it, gzip
            // otherwise; `ServeDir` still answers a precompressed `.br` or
            // `.gz` beside the file if a build ever produces one, and this
            // layer compresses on the fly when it does not.
            let files = tower_http::services::ServeDir::new(&dir)
                .precompressed_br()
                .precompressed_gzip()
                .not_found_service(tower_http::services::ServeFile::new(index));
            router = router.fallback_service(files).layer(
                tower_http::compression::CompressionLayer::new().br(true).gzip(true),
            );
            note!("web ui: {}", dir.display());
        }
        None => {
            router = router.fallback(|| async { (StatusCode::NOT_FOUND, "web UI not bundled; pass --web-dir") });
            note!("web ui: not found (pass --web-dir)");
        }
    }
    router
        .layer(axum::middleware::from_fn_with_state(app.clone(), guard))
        .layer(axum::Extension(security::Surface::Browser))
        .layer(axum::extract::DefaultBodyLimit::max(max_body))
        .with_state(app)
}

/// What the agent socket serves, and the browser port serves too: the
/// operation registry under both of its route shapes, its list, and the two
/// reads an agent client makes outside it (the session, for `cadpole op`'s
/// own session view, and the packed document a listener's local Render
/// needs). //spec:names_test diffs this list against `AGENT_ROUTES` in
/// spec/transport.qnt.
fn agent_routes() -> Router<Shared> {
    Router::new()
        .route("/api/session", get(get_session))
        .route("/api/session/document", get(get_session_document))
        // The generated front door. The browser router adds the hand-written
        // aliases kept for the web UI; new work goes here.
        .route("/api/ops", get(get_ops))
        .route("/api/ops/{method}", post(post_op))
        // ...and the same registry under a protocol other people implement:
        // `POST /api/cadpole.v1.CadpoleOps/<Rpc>`, Connect unary or gRPC-Web, with
        // `/api` as the base URL so it is behind the same guard. The
        // path comes from the service descriptor, so renaming the service in
        // ops.proto moves the route.
        .route(&connect::route_pattern(), post(post_connect))
}

/// The agent socket's router: [`agent_routes`] and nothing else -- no web UI,
/// no static files, no browser-only route -- behind the same guard, which
/// reads the surface from the request.
fn build_agent_router(app: Shared) -> Router {
    let max_body = app.max_body;
    agent_routes()
        .fallback(|| async {
            (StatusCode::NOT_FOUND, "not served on the agent socket: the web UI and its routes are on the TCP port")
        })
        .layer(axum::middleware::from_fn_with_state(app.clone(), guard))
        .layer(axum::Extension(security::Surface::Agent))
        .layer(axum::extract::DefaultBodyLimit::max(max_body))
        .with_state(app)
}

#[cfg(test)]
mod tests {
    use super::*;

    fn state(source: &str) -> SessionState {
        SessionState {
            web_build: String::new(),
            id: "t".into(),
            revision: 1,
            document: cadpole_engine::document_from_scripts("t", vec![("main.star".into(), source.into())]),
            selection: Selection::default(),
            messages: Vec::new(),
            events: Vec::new(),
            clients: 0,
            save_path: None,
            assistant_listening: false,
            camera: None,
            cursors: Default::default(),
            failures: FailureLimits::default(),
            streams: Default::default(),
        }
    }

    fn host(source: &str) -> ServerHost {
        let (tx, _) = broadcast::channel(64);
        ServerHost {
            app: Arc::new(App {
                state: Mutex::new(state(source)),
                events: tx,
                live: broadcast::channel(256).0,
                tokens: security::TokenSet::open(),
                allowed_origins: Vec::new(),
                root: DocumentRoot::new(Some(std::env::temp_dir().join("cadpole-test-docs"))),
                render_slots: tokio::sync::Semaphore::new(1),
                render_waiting: AtomicUsize::new(0),
                render_budget: Duration::from_secs(60),
                listener_slots: Arc::new(tokio::sync::Semaphore::new(LISTENER_OP_SLOTS)),
                max_body: security::MAX_BODY_BYTES,
                memo: Arc::new(cadpole_engine::cadpole_core::memo::Memo::new()),
            }),
            by: "test".into(),
            role: cadpole_ops::capability::Role::Owner,
        }
    }

    const SRC: &str = "w = param(\"w\", 40)\nb = box(w, 20, 10, name=\"blk\")\n";

    #[test]
    fn a_script_path_must_be_a_bare_name_and_fit_the_cap() {
        assert!(check_script("main.star", "b = 1").is_ok());
        for bad in ["../evil.star", "a/b.star", "a\\b.star", "", "..", "x/../y"] {
            assert!(check_script(bad, "b = 1").is_err(), "{bad:?} must be refused");
        }
        let huge = "#".repeat(security::MAX_SCRIPT_BYTES + 1);
        let e = check_script("main.star", &huge).unwrap_err().to_string();
        assert!(e.contains("byte limit"), "{e}");
    }

    #[test]
    fn edit_script_applies_a_targeted_change_and_reports_a_diff() {
        use cadpole_ops::Host as _;
        let h = host(SRC);
        let r = h
            .edit_script(&pb::EditScriptRequest {
                path: "main.star".into(),
                base_revision: 1,
                edits: vec![pb::ScriptEdit { old_str: "param(\"w\", 40)".into(), new_str: "param(\"w\", 55)".into(), ..Default::default() }],
            })
            .unwrap();
        assert!(!r.conflict);
        assert_eq!(r.revision, 2);
        assert_eq!(r.applied, 1);
        assert!(r.diff.contains("-w = param(\"w\", 40)"), "{}", r.diff);
        assert!(r.diff.contains("+w = param(\"w\", 55)"), "{}", r.diff);
        assert!(r.features.contains(&"blk".to_owned()), "{:?}", r.features);
        assert!(r.diagnostics.is_empty(), "{:?}", r.diagnostics);
        let st = h.app.state.lock().unwrap();
        assert!(st.document.scripts[0].source.contains("55"));
        assert_eq!(st.events.last().unwrap().kind, "script_changed");
        assert_eq!(st.events.last().unwrap().by, "test", "the event names the writer");
    }

    #[test]
    fn a_stale_base_revision_is_refused_and_hands_back_the_current_source() {
        use cadpole_ops::Host as _;
        let h = host(SRC);
        let r = h
            .edit_script(&pb::EditScriptRequest {
                path: "main.star".into(),
                base_revision: 999,
                edits: vec![pb::ScriptEdit { old_str: "40".into(), new_str: "50".into(), ..Default::default() }],
            })
            .unwrap();
        assert!(r.conflict, "a stale base revision must not write");
        assert_eq!(r.current_revision, 1);
        assert_eq!(r.current_source, SRC);
        assert_eq!(r.revision, 0);
        assert_eq!(h.app.state.lock().unwrap().document.scripts[0].source, SRC, "nothing was written");

        // ...and SetScript agrees.
        let r = h.set_script(&pb::SetScriptRequest { path: "main.star".into(), source: "b = box(1,1,1)".into(), base_revision: 7 }).unwrap();
        assert!(r.conflict);
        assert_eq!(h.app.state.lock().unwrap().revision, 1);
    }

    /// Zero is not a free pass on these two routes.
    ///
    /// `base_revision` is read three ways across the API and revisions.qnt is
    /// about exactly that: `add_asset` treats zero as "no check", while
    /// SetScript and EditScript compare the number -- and the session starts
    /// at revision 1, so a zero can only mean "I read nothing". No Rust test
    /// ever sent a zero here, so the difference between the two readings was
    /// invisible in this crate and only a regex in //spec:names_test stood
    /// behind it.
    #[test]
    fn a_write_that_names_no_base_revision_is_a_conflict_rather_than_a_free_write() {
        use cadpole_ops::Host as _;
        let h = host(SRC);
        let r = h
            .edit_script(&pb::EditScriptRequest {
                path: "main.star".into(),
                base_revision: 0,
                edits: vec![pb::ScriptEdit { old_str: "40".into(), new_str: "50".into(), ..Default::default() }],
            })
            .unwrap();
        assert!(r.conflict, "an edit that read no revision must not be applied");
        assert_eq!(r.current_revision, 1);
        assert_eq!(r.revision, 0, "a conflict carries no new revision");

        let r = h
            .set_script(&pb::SetScriptRequest { path: "main.star".into(), source: "b = box(1, 1, 1)\n".into(), base_revision: 0 })
            .unwrap();
        assert!(r.conflict, "...and neither is a whole-file write");

        let st = h.app.state.lock().unwrap();
        assert_eq!(st.document.scripts[0].source, SRC, "nothing was written");
        assert_eq!(st.revision, 1, "and the revision did not move");
    }

    /// A `SetScript` that omits `source` used to erase the file and answer
    /// with the new revision. `source` is `required: true`, but that option
    /// reaches only the JSON Schema an assistant reads, and over proto3 an
    /// omitted string arrives as an empty one -- so the request was
    /// well-formed, destructive, and reported as success. Sending
    /// `{"path": "main.star", "base_revision": N}` to a real server reproduces it.
    #[test]
    fn set_script_will_not_erase_a_file_because_the_source_was_left_out() {
        use cadpole_ops::Host as _;
        let h = host(SRC);
        let e = h
            .set_script(&pb::SetScriptRequest { path: "main.star".into(), source: String::new(), base_revision: 1 })
            .expect_err("an empty source must be refused, not written");
        assert!(e.to_string().contains("would erase"), "{e}");
        let st = h.app.state.lock().unwrap();
        assert_eq!(st.document.scripts[0].source, SRC, "nothing was written");
        assert_eq!(st.revision, 1, "and the revision did not move");
    }

    #[test]
    fn an_edit_that_would_not_parse_is_refused_before_it_is_stored() {
        use cadpole_ops::Host as _;
        let h = host(SRC);
        let e = h
            .edit_script(&pb::EditScriptRequest {
                path: "main.star".into(),
                base_revision: 1,
                edits: vec![pb::ScriptEdit { old_str: "box(w, 20, 10, name=\"blk\")".into(), new_str: "box(w, 20,".into(), ..Default::default() }],
            })
            .unwrap_err()
            .to_string();
        assert!(e.contains("would not parse"), "{e}");
        assert!(e.contains("nothing was written"), "{e}");
        assert_eq!(h.app.state.lock().unwrap().revision, 1);
    }

    #[test]
    fn ack_stores_a_cursor_and_flags_a_new_epoch() {
        use cadpole_ops::Host as _;
        let h = host(SRC);
        {
            let mut st = h.app.state.lock().unwrap();
            for _ in 0..3 {
                h.app.emit(&mut st, "message", serde_json::json!({}));
            }
        }
        let r = h.ack(&pb::AckRequest { client: "mcp:1".into(), cursor: 2 }).unwrap();
        assert_eq!((r.cursor, r.last_event_seq, r.epoch_reset), (2, 3, false));
        // A cursor from a previous run of the server is clamped and flagged.
        let r = h.ack(&pb::AckRequest { client: "mcp:1".into(), cursor: 900 }).unwrap();
        assert!(r.epoch_reset, "a cursor past the log means the server restarted");
        assert_eq!(r.cursor, 3);
        // Cursors never go backwards.
        let r = h.ack(&pb::AckRequest { client: "mcp:1".into(), cursor: 1 }).unwrap();
        assert_eq!(r.cursor, 3);
        assert!(h.ack(&pb::AckRequest { client: String::new(), cursor: 1 }).is_err(), "a cursor needs a client id");
    }

    #[test]
    fn wait_events_resumes_from_the_stored_cursor_and_skips_the_callers_own_writes() {
        use cadpole_ops::Host as _;
        let h = host(SRC);
        {
            let mut st = h.app.state.lock().unwrap();
            h.app.emit_by(&mut st, "message", serde_json::json!({"n": 1}), "ui");
            h.app.emit_by(&mut st, "message", serde_json::json!({"n": 2}), "test");
            h.app.emit_by(&mut st, "message", serde_json::json!({"n": 3}), "ui");
        }
        // `by` is "test", so event 2 is this client's own and is skipped.
        let r = h.wait_events(&pb::WaitEventsRequest { after: 0, timeout_ms: 1, ..Default::default() }).unwrap();
        let seqs: Vec<u64> = r.events.iter().map(|e| e.seq).collect();
        assert_eq!(seqs, vec![1, 3], "a client never sees its own writes echoed");

        h.ack(&pb::AckRequest { client: "test".into(), cursor: 1 }).unwrap();
        let r = h.wait_events(&pb::WaitEventsRequest { after: 0, timeout_ms: 1, ..Default::default() }).unwrap();
        assert_eq!(r.events.iter().map(|e| e.seq).collect::<Vec<_>>(), vec![3], "after: 0 resumes from the stored cursor");
    }

    /// The event log is a bounded window, so a reader whose cursor fell below
    /// it has missed events for good. `truncated` and `first_seq` are the
    /// protocol's only way of saying so -- ops.proto calls `truncated` "this
    /// is the telling", and it is the fix for eventlog.qnt's
    /// `nothingIsLostSilently`. Neither field had a test: replacing the
    /// computation with a flat `false` passed every test in the tree.
    #[test]
    fn a_reader_whose_cursor_fell_below_the_window_is_told_so() {
        use cadpole_ops::Host as _;
        let h = host(SRC);
        {
            let mut st = h.app.state.lock().unwrap();
            // Five past the cap, so the window has moved and by how much is
            // known: 1 through 5 are gone and the log begins at 6.
            for n in 0..10_005 {
                h.app.emit_by(&mut st, "message", serde_json::json!({ "n": n }), "ui");
            }
            assert_eq!(st.events.len(), 10_000, "the log is a window, not a history");
            assert_eq!(st.events.first().unwrap().seq, 6, "and it drops from the front");
        }
        let ask = |after: u64| h.wait_events(&pb::WaitEventsRequest { after, timeout_ms: 1, ..Default::default() }).unwrap();

        // A reader holding cursor 2 asks for 3 onwards; 3, 4 and 5 are gone.
        let r = ask(2);
        assert!(r.truncated, "a reader resuming below the window has to be told");
        assert_eq!(r.first_seq, 6, "...and told where the window begins now");
        assert_eq!(r.events.first().map(|e| e.seq), Some(6), "the stream itself begins later than it asked");

        // The boundary: cursor 5 asks for 6 onwards, and 6 is exactly what
        // the window starts with. Nothing was lost, so nothing is claimed.
        let r = ask(5);
        assert!(!r.truncated, "resuming at the first event still held is not a truncation");
        assert_eq!(r.first_seq, 6, "the window's start is reported either way");

        // ...and a reader with no cursor at all is not told it lost events it
        // never had: `after: 0` means "whatever you have".
        assert!(!ask(0).truncated, "after: 0 is not a claim to have seen anything");
    }

    #[test]
    fn an_attachment_comes_back_as_bytes_with_its_size() {
        use cadpole_ops::Host as _;
        use base64::Engine as _;
        let h = host(SRC);
        // A 1x1 PNG.
        let png: Vec<u8> = vec![
            0x89, b'P', b'N', b'G', 0x0d, 0x0a, 0x1a, 0x0a, 0, 0, 0, 13, b'I', b'H', b'D', b'R', 0, 0, 0, 1, 0, 0, 0, 1,
        ];
        {
            let mut st = h.app.state.lock().unwrap();
            st.messages.push(ChatMessage {
                seq: 1,
                from: "user".into(),
                text: "look".into(),
                at: now(),
                attachments: vec![Attachment {
                    kind: "screenshot".into(),
                    png_base64: base64::engine::general_purpose::STANDARD.encode(&png),
                    camera: Some(serde_json::json!({"preset": "iso"})),
                    annotated: true,
                }],
                state: String::new(),
                activity: String::new(),
            });
        }
        let r = h.get_attachment(&pb::GetAttachmentRequest { seq: 1, index: 0 }).unwrap();
        assert_eq!(r.png, png);
        assert_eq!((r.width, r.height), (1, 1));
        assert!(r.annotated);
        assert_eq!(r.kind, "screenshot");
        assert!(r.camera.is_some(), "the camera travels with the picture");

        let e = h.get_attachment(&pb::GetAttachmentRequest { seq: 9, index: 0 }).unwrap_err().to_string();
        assert!(e.contains("no message with seq 9"), "{e}");
        let e = h.get_attachment(&pb::GetAttachmentRequest { seq: 1, index: 4 }).unwrap_err().to_string();
        assert!(e.contains("does not exist"), "{e}");
    }

    #[test]
    fn an_asset_is_added_sniffed_and_capped() {
        use cadpole_ops::Host as _;
        let h = host(SRC);
        let r = h
            .add_asset(&pb::AddAssetRequest {
                name: "plan.bin".into(),
                data: b"0\nSECTION\n2\nHEADER\n".to_vec(),
                base_revision: 1,
                ..Default::default()
            })
            .unwrap();
        let a = r.asset.unwrap();
        assert_eq!(a.kind(), pb::AssetKind::Dxf, "the bytes decide the kind, not the name");
        assert_eq!(a.data.len(), 0, "the listing never carries bytes");
        assert_eq!(r.revision, 2);

        let doc = h.document().unwrap();
        let list = cadpole_engine::list_assets(&doc);
        assert_eq!(list.assets.len(), 1);
        assert_eq!(list.assets[0].name, "plan.bin");
        let got = cadpole_engine::get_asset(&doc, &pb::GetAssetRequest { name: "plan.bin".into(), ..Default::default() }).unwrap();
        assert_eq!(got.data, b"0\nSECTION\n2\nHEADER\n");

        for bad in ["../escape", "a/b", "", "..", ".hidden"] {
            let e = h.add_asset(&pb::AddAssetRequest { name: bad.into(), data: vec![1, 2, 3], ..Default::default() });
            assert!(e.is_err(), "asset name {bad:?} must be refused");
        }
        let r = h
            .add_asset(&pb::AddAssetRequest { name: "x.bin".into(), data: vec![1], base_revision: 999, ..Default::default() })
            .unwrap();
        assert!(r.conflict, "a stale base revision must not write");
    }

    #[test]
    fn a_selection_that_did_not_change_emits_no_event() {
        use cadpole_ops::Host as _;
        let h = host(SRC);
        let sel = pb::SetSelectionRequest {
            selected: vec![pb::SessionRef { feature_id: "blk".into(), kind: "face".into(), topo_name: "face:0".into(), ..Default::default() }],
            ..Default::default()
        };
        h.set_selection(&sel).unwrap();
        assert_eq!(h.app.state.lock().unwrap().events.len(), 1);
        // Hover moves the mouse, not the selection: the same set again is not
        // an event.
        h.set_selection(&sel).unwrap();
        assert_eq!(h.app.state.lock().unwrap().events.len(), 1, "an unchanged selection must not be forwarded");
    }

    // ---- Front-end failure reports -------------------------------

    fn a_panic_report() -> pb::ReportClientFailureRequest {
        pb::ReportClientFailureRequest {
            kind: pb::ClientFailureKind::WasmPanic as i32,
            message: "time not implemented on this platform".into(),
            location: "src/rust/cadpole-core/src/boolean.rs:412:9".into(),
            stack: "at wasm://wasm/0019bc1a\nat Engine.evaluate".into(),
            surface: pb::ClientSurface::Page as i32,
            operation: "evaluate".into(),
            revision: 7,
            params: [("wall".to_owned(), cadpole_engine::cadpole_core::ParamValue::Number(2.4).to_proto())].into_iter().collect(),
            engine_build: "cadpole-wasm 0.1.0 opt".into(),
            engine_kind: "wasm".into(),
            script_digest: "9f2b1c4d5e6f7a8b".into(),
            uptime_ms: 41_000,
            repeat: 1,
        }
    }

    /// The whole point: a crash in the browser becomes an event in the log,
    /// with the fields that let someone act on it without asking the user.
    #[test]
    fn a_failure_report_becomes_an_event_with_everything_needed_to_act_on_it() {
        use cadpole_ops::Host as _;
        let h = host(SRC);
        let out = h.report_client_failure(&a_panic_report()).unwrap();
        assert!(out.accepted && out.refused.is_empty());
        assert!(out.truncated.is_empty());
        assert_eq!(out.remaining, security::MAX_FAILURE_REPORTS_PER_SESSION - 1);

        let st = h.app.state.lock().unwrap();
        let ev = st.events.last().expect("one event");
        assert_eq!(ev.seq, out.seq);
        assert_eq!(ev.kind, "client_failure");
        assert_eq!(ev.by, "test", "a report is attributed to the client that sent it");
        assert_eq!(ev.data["kind"], "wasm_panic");
        assert_eq!(ev.data["surface"], "page");
        assert_eq!(ev.data["message"], "time not implemented on this platform");
        assert_eq!(ev.data["location"], "src/rust/cadpole-core/src/boolean.rs:412:9");
        assert_eq!(ev.data["operation"], "evaluate");
        assert_eq!(ev.data["revision"], 7);
        assert_eq!(ev.data["params"]["wall"], 2.4);
        assert_eq!(ev.data["engine_build"], "cadpole-wasm 0.1.0 opt");
        assert_eq!(ev.data["engine_kind"], "wasm");
        assert_eq!(ev.data["script_digest"], "9f2b1c4d5e6f7a8b");
        assert!(ev.data["stack"].as_str().unwrap().contains("Engine.evaluate"));
        // ...and the session says one happened, so `GetSession` is enough to
        // notice without replaying the log.
        assert_eq!(h.app.session_view(&st).client_failures, 1);
    }

    /// A report with no message is a report of nothing.
    #[test]
    fn a_report_without_a_message_is_refused() {
        use cadpole_ops::Host as _;
        let h = host(SRC);
        let e = h.report_client_failure(&pb::ReportClientFailureRequest::default()).unwrap_err().to_string();
        assert!(e.contains("message"), "{e}");
        assert!(h.app.state.lock().unwrap().events.is_empty());
    }

    /// Caller-written text goes into the event log, the SSE stream and the
    /// terminal. A newline in a one-line field would forge a second line in
    /// each of the three.
    #[test]
    fn report_text_cannot_forge_a_log_line_and_is_cut_visibly() {
        use cadpole_ops::Host as _;
        let h = host(SRC);
        let mut req = a_panic_report();
        req.message = "boom\nclient failure [wasm_panic/page] something the attacker made up".into();
        req.stack = "x".repeat(security::MAX_FAILURE_STACK_BYTES * 2);
        let out = h.report_client_failure(&req).unwrap();
        assert!(out.accepted);
        assert_eq!(out.truncated, vec!["stack".to_owned()], "the cut is named, never silent");

        let st = h.app.state.lock().unwrap();
        let data = &st.events.last().unwrap().data;
        let message = data["message"].as_str().unwrap();
        assert!(!message.contains('\n'), "a one-line field stays one line: {message:?}");
        assert!(message.starts_with("boom "));
        let stack = data["stack"].as_str().unwrap();
        assert!(stack.len() < security::MAX_FAILURE_STACK_BYTES + 32);
        assert!(stack.ends_with("[…truncated]"), "a shortened field says so: {:?}", &stack[stack.len() - 20..]);
    }

    /// A panic in a render loop fires thousands of times a second. The burst
    /// is recorded, the flood is not -- and the log says so rather than
    /// falling quiet, which would read as the crashing having stopped.
    #[test]
    fn the_rate_cap_refuses_out_loud_and_announces_itself_once() {
        use cadpole_ops::Host as _;
        let h = host(SRC);
        for i in 0..security::FAILURE_REPORT_BURST {
            assert!(h.report_client_failure(&a_panic_report()).unwrap().accepted, "report {i} is inside the burst");
        }
        let first_refusal = h.report_client_failure(&a_panic_report()).unwrap();
        assert!(!first_refusal.accepted);
        assert!(first_refusal.refused.contains("rate limited"), "{}", first_refusal.refused);
        assert_eq!(first_refusal.dropped, 1);
        assert!(first_refusal.seq > 0, "the first refusal in a window is announced in the log");

        let second_refusal = h.report_client_failure(&a_panic_report()).unwrap();
        assert!(!second_refusal.accepted);
        assert_eq!(second_refusal.dropped, 2, "every drop is counted, even when it is not announced");
        assert_eq!(second_refusal.seq, 0, "the notice is itself rate limited");

        let st = h.app.state.lock().unwrap();
        let notices: Vec<&Event> =
            st.events.iter().filter(|e| e.kind == "client_failure" && e.data["throttled"].as_bool().unwrap_or(false)).collect();
        assert_eq!(notices.len(), 1, "exactly one throttle notice per window");
        assert_eq!(notices[0].data["dropped"], 1);
        assert_eq!(
            notices[0].data["message"], "time not implemented on this platform",
            "the notice still says what is crashing"
        );
        assert_eq!(st.events.iter().filter(|e| e.kind == "client_failure").count(), usize::from(security::FAILURE_REPORT_BURST as u16) + 1);
    }

    /// The session cap: past it the answer is "stop", with a reason, and the
    /// remaining budget reaches 0 so a client knows not to retry.
    #[test]
    fn the_session_cap_is_final_and_says_so() {
        let mut limits = FailureLimits::default();
        let t0 = std::time::Instant::now();
        limits.accepted = security::MAX_FAILURE_REPORTS_PER_SESSION;
        let why = limits.admit(t0).unwrap_err();
        assert!(why.contains("per-session cap"), "{why}");
        assert_eq!(limits.remaining(), 0);

        // And the bucket really does refill, so a crash an hour later is
        // still reported.
        let mut limits = FailureLimits::default();
        for _ in 0..security::FAILURE_REPORT_BURST {
            limits.admit(t0).unwrap();
        }
        assert!(limits.admit(t0).is_err());
        let later = t0 + Duration::from_millis(security::FAILURE_REPORT_REFILL_MS + 1);
        assert!(limits.admit(later).is_ok(), "the bucket refills");
    }

    #[test]
    fn png_size_reads_the_header_and_tolerates_rubbish() {
        assert_eq!(png_size(b"not a png"), (0, 0));
        let mut png = vec![0x89, b'P', b'N', b'G', 0x0d, 0x0a, 0x1a, 0x0a];
        png.extend([0, 0, 0, 13, b'I', b'H', b'D', b'R']);
        png.extend(800u32.to_be_bytes());
        png.extend(600u32.to_be_bytes());
        assert_eq!(png_size(&png), (800, 600));
    }

    #[test]
    fn a_name_becomes_a_safe_document_id() {
        assert_eq!(slug("Planter v2"), "planter-v2");
        assert_eq!(slug("../../etc/passwd"), "etc-passwd");
        assert_eq!(slug("a/b"), "a-b");
        assert!(DocumentRoot::valid_id(&slug("../../etc/passwd")));
        assert!(DocumentRoot::valid_id(&slug("!!!")), "an unusable name still becomes a valid id");
    }

    #[test]
    fn only_a_cgnat_address_counts_as_the_tailnet() {
        assert!(is_tailnet(&"100.64.0.1".parse().unwrap()));
        assert!(is_tailnet(&"100.127.255.254".parse().unwrap()));
        assert!(!is_tailnet(&"100.63.0.1".parse().unwrap()));
        assert!(!is_tailnet(&"100.128.0.1".parse().unwrap()));
        assert!(!is_tailnet(&"192.168.1.5".parse().unwrap()));
    }

    #[test]
    fn render_jobs_leave_the_machine_usable() {
        assert_eq!(render_jobs(5), 5);
        let auto = render_jobs(0);
        assert!((1..=2).contains(&auto), "auto picked {auto}");
    }

    /// An App with the render gate set to whatever the test needs. The two
    /// other builders here fix one slot and a 60 s budget, which is the one
    /// configuration in which neither refusal can happen.
    fn app_with_render_gate(slots: usize, budget: Duration) -> Shared {
        let (tx, _) = broadcast::channel(64);
        Arc::new(App {
            state: Mutex::new(state(SRC)),
            events: tx,
            live: broadcast::channel(256).0,
            tokens: security::TokenSet::open(),
            allowed_origins: Vec::new(),
            root: DocumentRoot::new(Some(std::env::temp_dir().join("cadpole-test-docs"))),
            render_slots: tokio::sync::Semaphore::new(slots),
            render_waiting: AtomicUsize::new(0),
            render_budget: budget,
            listener_slots: Arc::new(tokio::sync::Semaphore::new(LISTENER_OP_SLOTS)),
            max_body: security::MAX_BODY_BYTES,
            memo: Arc::new(cadpole_engine::cadpole_core::memo::Memo::new()),
        })
    }

    /// Both of the render gate's refusals, which had no test at all: the
    /// queue's 429 and the budget's 504. `render_jobs_leave_the_machine_usable`
    /// covers the job-count heuristic and nothing else, and every HTTP test
    /// builds the app with one slot and a 60 s budget -- the one configuration
    /// where neither refusal can be reached. `spec/lifecycle.qnt` models
    /// exactly this code.
    #[test]
    fn the_render_gate_refuses_a_full_queue_and_abandons_an_overrunning_job() {
        let rt = tokio::runtime::Builder::new_multi_thread().worker_threads(4).enable_all().build().unwrap();

        // A job that outlives its budget is abandoned with a 504 that says
        // what to do about it, rather than holding the caller for ever.
        // The job runs until the test lets it go, so it overruns whatever the
        // machine's load.
        let app = app_with_render_gate(1, Duration::from_millis(100));
        let (release, held) = std::sync::mpsc::channel::<()>();
        let (status, why) = rt
            .block_on(render_job(&app, "render", move || {
                let _ = held.recv();
            }))
            .expect_err("a job that never finishes inside a 100 ms budget must not succeed");
        assert_eq!(status, StatusCode::GATEWAY_TIMEOUT);
        assert!(why.contains("100 ms budget"), "{why}");
        assert!(why.contains("--render-budget-ms"), "the refusal says which knob to turn: {why}");

        // The slot is released even though the work was abandoned and is
        // still running, so the next caller is not stuck behind a job nobody
        // is waiting for. (Checked on the semaphore itself: a second job
        // would have to finish inside the same 100 ms budget, which a loaded
        // machine does not promise.)
        assert_eq!(app.render_slots.available_permits(), 1, "the gate reopens while the abandoned job still runs");
        assert_eq!(app.render_waiting.load(Ordering::SeqCst), 0, "the waiting count comes back to zero");
        release.send(()).unwrap();

        // And with the queue full, a further caller is refused rather than
        // added to it -- the work never starts.
        let app = app_with_render_gate(1, Duration::from_secs(60));
        app.render_waiting.store(RENDER_QUEUE, Ordering::SeqCst);
        let ran = Arc::new(AtomicUsize::new(0));
        let (status, why) = {
            let ran = ran.clone();
            rt.block_on(render_job(&app, "render", move || ran.fetch_add(1, Ordering::SeqCst)))
                .expect_err("a full queue must be refused")
        };
        assert_eq!(status, StatusCode::TOO_MANY_REQUESTS);
        assert!(why.contains("already queued"), "{why}");
        assert_eq!(ran.load(Ordering::SeqCst), 0, "a refused render does no work");
        assert_eq!(
            app.render_waiting.load(Ordering::SeqCst),
            RENDER_QUEUE,
            "a refusal leaves the count where it found it"
        );

        // One below the limit is still admitted, so the bound is the queue
        // being full and not the counter being non-zero.
        app.render_waiting.store(RENDER_QUEUE - 1, Ordering::SeqCst);
        assert_eq!(rt.block_on(render_job(&app, "render", || 9u8)).expect("room for one more"), 9);
    }

    // -----------------------------------------------------------------------
    // The real router over a real socket. These are the security properties that
    // only mean anything end to end: a token is required, a traversal attempt
    // is refused, an oversized body is refused, the headers are present.
    // -----------------------------------------------------------------------

    /// Start the real router on an ephemeral loopback port. Returns the base
    /// URL; the runtime is leaked, which is what a test server is for.
    fn serve_for_test(token: Option<&str>, root: PathBuf) -> (String, Shared) {
        let tokens = match token {
            Some(t) => security::TokenSet::owner_only(t),
            None => security::TokenSet::open(),
        };
        serve_tokens_for_test(tokens, root)
    }

    fn serve_tokens_for_test(tokens: security::TokenSet, root: PathBuf) -> (String, Shared) {
        serve_on_pool_for_test(tokens, root, 512)
    }

    /// [`serve_tokens_for_test`] with tokio's blocking pool cut to
    /// `max_blocking` threads, so a test can show what happens when it is
    /// full without starting five hundred of anything.
    fn serve_on_pool_for_test(tokens: security::TokenSet, root: PathBuf, max_blocking: usize) -> (String, Shared) {
        serve_custom_for_test(tokens, root, max_blocking, 64 * 1024)
    }

    /// The most general of these: the blocking pool and `--max-body` too.
    fn serve_custom_for_test(tokens: security::TokenSet, root: PathBuf, max_blocking: usize, max_body: usize) -> (String, Shared) {
        let (tx, _) = broadcast::channel(64);
        let app: Shared = Arc::new(App {
            state: Mutex::new(state(SRC)),
            events: tx,
            live: broadcast::channel(256).0,
            tokens,
            allowed_origins: Vec::new(),
            root: DocumentRoot::new(Some(root)),
            render_slots: tokio::sync::Semaphore::new(1),
            render_waiting: AtomicUsize::new(0),
            render_budget: Duration::from_secs(60),
            // Scaled with the pool, as the real constant is against tokio's
            // 512: the listener always leaves the owner threads to run on.
            listener_slots: Arc::new(tokio::sync::Semaphore::new(LISTENER_OP_SLOTS.min(max_blocking / 2).max(1))),
            max_body,
            memo: Arc::new(cadpole_engine::cadpole_core::memo::Memo::new()),
        });
        let router = build_router(app.clone(), Some(PathBuf::from("/nonexistent")));
        let rt = tokio::runtime::Builder::new_multi_thread()
            .worker_threads(2)
            .max_blocking_threads(max_blocking)
            .enable_all()
            .build()
            .unwrap();
        let listener = rt.block_on(async { tokio::net::TcpListener::bind("127.0.0.1:0").await.unwrap() });
        let addr = listener.local_addr().unwrap();
        rt.spawn(async move {
            let _ = axum::serve(listener, router).await;
        });
        std::mem::forget(rt);
        (format!("http://{addr}"), app)
    }

    fn agent() -> ureq::Agent {
        ureq::Agent::new_with_defaults()
    }

    /// An agent that hands back 4xx and 5xx responses instead of turning them
    /// into errors. The Connect tests are about the *body* of a failure.
    fn reading_agent() -> ureq::Agent {
        let config = ureq::Agent::config_builder().http_status_as_error(false).build();
        ureq::Agent::new_with_config(config)
    }

    fn status_of(r: Result<ureq::http::Response<ureq::Body>, ureq::Error>) -> u16 {
        match r {
            Ok(r) => r.status().as_u16(),
            Err(ureq::Error::StatusCode(c)) => c,
            Err(e) => panic!("request failed: {e}"),
        }
    }

    #[test]
    fn the_api_needs_a_token_and_the_headers_are_always_there() {
        let (base, _app) = serve_for_test(Some("s3cret"), std::env::temp_dir().join("cadpole-http-test-1"));
        let a = agent();

        assert_eq!(status_of(a.get(&format!("{base}/api/session")).call()), 401, "no token");
        assert_eq!(
            status_of(a.get(&format!("{base}/api/session")).header("Authorization", "Bearer wrong").call()),
            401,
            "wrong token"
        );
        let r = a.get(&format!("{base}/api/session")).header("Authorization", "Bearer s3cret").call().unwrap();
        assert_eq!(r.status().as_u16(), 200);
        // All six, at the HTTP level: `security_headers()` being right is one
        // thing, the middleware putting every one of them on a real response
        // is another.
        for h in [
            "x-content-type-options",
            "referrer-policy",
            "content-security-policy",
            "cross-origin-opener-policy",
            "cross-origin-embedder-policy",
            "cross-origin-resource-policy",
        ] {
            assert!(r.headers().get(h).is_some(), "missing {h}");
        }
        // The token travels in the header and nowhere else. `?token=` is not
        // accepted on any route, the event stream included: a query string
        // ends up in history, in a Referer and in every proxy log.
        for path in ["/api/model", "/api/events", "/api/session"] {
            assert_eq!(status_of(a.get(&format!("{base}{path}?token=s3cret")).call()), 401, "{path} must not take ?token=");
        }
    }

    /// The negative case the whole scheme rests on: a link from the server
    /// run *before* this one does not open this one.
    #[test]
    fn a_token_from_a_previous_server_run_is_refused() {
        let dir = std::env::temp_dir().join(format!("cadpole-run-token-{}", std::process::id()));
        let _ = std::fs::remove_dir_all(&dir);
        let previous_run = security::mint_tokens_in(Some(&dir), None);
        let this_run = security::mint_tokens_in(Some(&dir), None);

        let (base, _app) = serve_tokens_for_test(
            security::TokenSet::new(&this_run.owner, &this_run.listener),
            std::env::temp_dir().join("cadpole-http-test-runs"),
        );
        let a = agent();
        let get = |t: &str| status_of(a.get(&format!("{base}/api/session")).header("Authorization", &format!("Bearer {t}")).call());
        assert_eq!(get(&this_run.owner), 200, "this run's token works");
        assert_eq!(get(&previous_run.owner), 401, "the previous run's owner token must not work");
        assert_eq!(get(&previous_run.listener), 401, "nor its listener token");
        // ...and the file now holds this run's token, so a client that
        // re-reads it recovers with nothing pasted by hand.
        assert_eq!(std::fs::read_to_string(this_run.owner_file.as_ref().unwrap()).unwrap().trim(), this_run.owner);
        let _ = std::fs::remove_dir_all(&dir);
    }

    /// The event stream is authenticated like everything else, which is what
    /// lets the query-string escape hatch go away.
    #[test]
    fn the_event_stream_needs_the_header_too() {
        let (base, _app) = serve_for_test(Some("s3cret"), std::env::temp_dir().join("cadpole-http-test-sse"));
        let a = agent();
        assert_eq!(status_of(a.get(&format!("{base}/api/events?after=0")).call()), 401, "no token");
        assert_eq!(
            status_of(a.get(&format!("{base}/api/events?after=0")).header("Authorization", "Bearer nope").call()),
            401,
            "wrong token"
        );
        let r = a
            .get(&format!("{base}/api/events?after=0"))
            .header("Authorization", "Bearer s3cret")
            .config()
            .timeout_global(Some(Duration::from_secs(5)))
            .build()
            .call()
            .unwrap();
        assert_eq!(r.status().as_u16(), 200);
        assert!(
            r.headers().get("content-type").and_then(|v| v.to_str().ok()).unwrap_or("").starts_with("text/event-stream"),
            "the stream is still SSE, it just takes a header now"
        );
    }

    /// A restricted token cannot stall the owner by parking long polls.
    ///
    /// `WaitEvents` is granted to the listener as "a read-only long poll", and
    /// it used to hold one of tokio's blocking threads for its whole timeout
    /// (up to 120 s), sleeping in 50 ms steps. 512 of them filled the pool and
    /// every other operation, the owner's included, queued behind them. Here
    /// the pool is four threads and the listener parks twelve polls.
    #[test]
    fn listener_long_polls_do_not_starve_the_owners_operations() {
        let (base, _app) = serve_on_pool_for_test(
            security::TokenSet::new("owner-tok", "listener-tok"),
            std::env::temp_dir().join("cadpole-http-test-pool"),
            4,
        );
        let polls: Vec<_> = (0..12)
            .map(|_| {
                let base = base.clone();
                std::thread::spawn(move || {
                    let a = agent();
                    let r = a
                        .post(&format!("{base}/api/ops/WaitEvents"))
                        .header("Authorization", "Bearer listener-tok")
                        .send_json(serde_json::json!({"after": 999_999, "timeout_ms": 4000}));
                    status_of(r)
                })
            })
            .collect();
        std::thread::sleep(Duration::from_millis(500));
        let started = std::time::Instant::now();
        let code = status_of(
            agent().post(&format!("{base}/api/ops/GetSession")).header("Authorization", "Bearer owner-tok").send_json(serde_json::json!({})),
        );
        let took = started.elapsed();
        assert_eq!(code, 200);
        assert!(took < Duration::from_millis(1500), "the owner's GetSession waited {took:?} behind the listener's polls");
        for p in polls {
            assert_eq!(p.join().unwrap(), 200, "the long polls themselves still answer");
        }
    }

    /// ...and neither can a flood of its ordinary operations. A listener's
    /// `Evaluate` is a pure function under the evaluation deadline, but each
    /// one held a blocking thread, with no limit on how many.
    #[test]
    fn listener_evaluations_leave_the_owner_threads_to_run_on() {
        let (base, _app) = serve_on_pool_for_test(
            security::TokenSet::new("owner-tok", "listener-tok"),
            std::env::temp_dir().join("cadpole-http-test-pool-eval"),
            4,
        );
        let slow = "def spin():\n    n = 0\n    for i in range(6000000):\n        n += i\n    return n\nx = spin()\nb = box(1, 1, 1)\n";
        let evaluate = |token: &str| {
            let started = std::time::Instant::now();
            let code = status_of(
                agent()
                    .post(&format!("{base}/api/ops/Evaluate"))
                    .header("Authorization", &format!("Bearer {token}"))
                    .send_json(serde_json::json!({"document": {"scripts": [{"path": "main.star", "source": slow}]}})),
            );
            (code, started.elapsed())
        };
        let (code, one) = evaluate("owner-tok");
        assert_eq!(code, 200);
        let flood: Vec<_> = (0..12)
            .map(|_| {
                let base = base.clone();
                let slow = slow.to_owned();
                std::thread::spawn(move || {
                    status_of(
                        agent()
                            .post(&format!("{base}/api/ops/Evaluate"))
                            .header("Authorization", "Bearer listener-tok")
                            .send_json(serde_json::json!({"document": {"scripts": [{"path": "main.star", "source": slow}]}})),
                    )
                })
            })
            .collect();
        std::thread::sleep(one.min(Duration::from_millis(300)) / 2);
        let started = std::time::Instant::now();
        let code = status_of(
            agent().post(&format!("{base}/api/ops/GetSession")).header("Authorization", "Bearer owner-tok").send_json(serde_json::json!({})),
        );
        let took = started.elapsed();
        assert_eq!(code, 200);
        // Without a cap, GetSession queues behind three rounds of four.
        assert!(took < one / 2, "the owner's GetSession waited {took:?} behind the listener's evaluations (one takes {one:?})");
        for f in flood {
            assert_eq!(f.join().unwrap(), 200, "the listener's evaluations all still run");
        }
    }

    /// ...nor by giving up on its calls. A listener that sets a 1 ms Connect
    /// deadline is answered at once, but its evaluation keeps the blocking
    /// thread until it ends; the slot has to stay taken that long, or a loop
    /// of abandoned calls fills the pool exactly as an unlimited flood did.
    #[test]
    fn abandoned_listener_calls_keep_their_slot_while_the_work_runs() {
        let (base, _app) = serve_on_pool_for_test(
            security::TokenSet::new("owner-tok", "listener-tok"),
            std::env::temp_dir().join("cadpole-http-test-pool-abandon"),
            4,
        );
        let slow = "def spin():\n    n = 0\n    for i in range(6000000):\n        n += i\n    return n\nx = spin()\nb = box(1, 1, 1)\n";
        let body = serde_json::json!({"document": {"scripts": [{"path": "main.star", "source": slow}]}}).to_string();
        let url = format!("{base}{}", connect::method_path("Evaluate"));
        let started = std::time::Instant::now();
        let (code, one) = {
            let code = status_of(
                agent()
                    .post(&format!("{base}/api/ops/Evaluate"))
                    .header("Authorization", "Bearer owner-tok")
                    .send_json(serde_json::from_str::<serde_json::Value>(&body).unwrap()),
            );
            (code, started.elapsed())
        };
        assert_eq!(code, 200);
        for _ in 0..12 {
            let r = agent()
                .post(&url)
                .header("Authorization", "Bearer listener-tok")
                .content_type("application/json")
                .header("connect-timeout-ms", "1")
                .send(body.as_str());
            assert_eq!(status_of(r), 504, "a 1 ms deadline is not met");
        }
        let started = std::time::Instant::now();
        let code = status_of(
            agent().post(&format!("{base}/api/ops/GetSession")).header("Authorization", "Bearer owner-tok").send_json(serde_json::json!({})),
        );
        let took = started.elapsed();
        assert_eq!(code, 200);
        assert!(took < one / 2, "the owner's GetSession waited {took:?} behind abandoned listener evaluations (one takes {one:?})");
    }

    /// The listener's privilege is attached to its token, server side.
    /// A client-side allow-list would be a suggestion; this is not.
    #[test]
    fn a_listener_token_may_only_call_granted_operations() {
        let (base, app) = serve_tokens_for_test(
            security::TokenSet::new("owner-tok", "listener-tok"),
            std::env::temp_dir().join("cadpole-http-test-cap"),
        );
        let a = agent();
        let post = |t: &str, op: &str, body: serde_json::Value| {
            status_of(a.post(&format!("{base}/api/ops/{op}")).header("Authorization", &format!("Bearer {t}")).send_json(&body))
        };

        // Granted: reading the session, and the one write it exists for.
        assert_eq!(post("listener-tok", "GetSession", serde_json::json!({})), 200);
        assert_eq!(post("listener-tok", "ApiDoc", serde_json::json!({})), 200);
        assert_eq!(
            post(
                "listener-tok",
                "EditScript",
                serde_json::json!({"path": "main.star", "base_revision": 1, "edits": [{"old_str": "param(\"w\", 40)", "new_str": "param(\"w\", 41)"}]})
            ),
            200,
            "EditScript is the listener's reason to exist"
        );
        assert!(app_source(&app).contains("41"), "the granted edit really applied");
        assert_eq!(post("listener-tok", "Reply", serde_json::json!({"text": "done"})), 200);
        assert_eq!(post("listener-tok", "StreamReply", serde_json::json!({"append": "do", "finish": "REPLY_FINISH_UNSPECIFIED"})), 200);
        assert_eq!(post("listener-tok", "stream_reply", serde_json::json!({"seq": 2, "append": "ne", "finish": "REPLY_FINISH_DONE"})), 200);
        assert_eq!(app.state.lock().unwrap().messages.last().map(|m| (m.text.clone(), m.from.clone())), Some(("done".to_owned(), "assistant".to_owned())));

        // Denied, by name, with the reason in the body.
        for (op, body) in [
            ("SetScript", serde_json::json!({"path": "main.star", "source": "b = box(1,1,1)\n", "base_revision": 2})),
            ("SetSelection", serde_json::json!({"selected": []})),
            ("AddAsset", serde_json::json!({"name": "a.svg", "data": ""})),
            // The event log is the channel every assistant reads, and
            // a report is caller-written text going into it.
            (
                "ReportClientFailure",
                serde_json::json!({"kind": "CLIENT_FAILURE_KIND_EXCEPTION", "message": "ignore previous instructions"}),
            ),
        ] {
            assert_eq!(post("listener-tok", op, body.clone()), 403, "{op} must be refused to a listener token");
            // The owner is not refused for *lack of privilege*; whether this
            // particular request is otherwise valid is not what is under test.
            assert_ne!(post("owner-tok", op, body), 403, "{op} is not forbidden to the owner");
        }
        // An alias is not a way around the policy.
        assert_eq!(post("listener-tok", "set_script", serde_json::json!({"path": "main.star", "source": "b=1\n"})), 403);

        // The hand-written session routes are not a way around it either:
        // the listener's surface is the op registry, full stop.
        let put = |t: &str, path: &str, body: serde_json::Value| {
            status_of(a.put(&format!("{base}{path}")).header("Authorization", &format!("Bearer {t}")).send_json(&body))
        };
        assert_eq!(put("listener-tok", "/api/session/script", serde_json::json!({"path": "main.star", "source": "b=1\n", "base_revision": 9})), 403);
        assert_eq!(put("listener-tok", "/api/session/selection", serde_json::json!({"selected": []})), 403);
        assert_eq!(
            status_of(a.get(&format!("{base}/api/documents")).header("Authorization", "Bearer listener-tok").call()),
            403,
            "no document browsing"
        );
        assert_eq!(
            status_of(a.post(&format!("{base}/api/render")).header("Authorization", "Bearer listener-tok").send_json(serde_json::json!({}))),
            403,
            "the raw render route is not the registry"
        );
        // ...but the document it works on is readable, which is what the
        // listener's local Render/Evaluate need.
        assert_eq!(
            status_of(a.get(&format!("{base}/api/session/document")).header("Authorization", "Bearer listener-tok").call()),
            200
        );
        assert_eq!(status_of(a.get(&format!("{base}/api/ops")).header("Authorization", "Bearer listener-tok").call()), 200);
    }

    /// "The token is never logged" is a claim, so here is the grep that backs
    /// it: everything the server says, over a run that includes failures,
    /// contains the browser token exactly once -- in the URL it prints on
    /// purpose, to a terminal -- and no agent token at all. To anything that
    /// is not a terminal it prints no token whatever.
    #[test]
    fn the_token_is_printed_once_and_never_logged() {
        let dir = std::env::temp_dir().join(format!("cadpole-log-grep-{}", std::process::id()));
        let _ = std::fs::remove_dir_all(&dir);
        let minted = security::mint_tokens_in(Some(&dir), None);

        out::start_capture();
        for l in startup_banner(Some("127.0.0.1:7331"), Some(Path::new("/run/x.sock")), Some(&minted), None) {
            out::line(l);
        }
        for l in startup_banner(Some("127.0.0.1:7331"), Some(Path::new("/run/x.sock")), Some(&minted), Some(Path::new("/run/x.url"))) {
            assert!(!l.contains(&minted.browser), "a banner for a log carries no token: {l}");
        }
        let (base, _app) = serve_tokens_for_test(
            security::TokenSet::for_run(&minted.browser, &minted.owner, &minted.listener, true),
            std::env::temp_dir().join("cadpole-http-test-log"),
        );
        let a = agent();
        let auth = format!("Bearer {}", minted.owner);
        // A successful call, a 401, a 403, a 404 and a handler error: every
        // path that could put what it was given into a message.
        let mut bodies: Vec<String> = Vec::new();
        let mut record = |r: Result<ureq::http::Response<ureq::Body>, ureq::Error>| {
            if let Ok(mut r) = r {
                bodies.push(format!("{:?}", r.headers()));
                bodies.push(r.body_mut().read_to_string().unwrap_or_default());
            }
        };
        record(a.get(&format!("{base}/api/session")).header("Authorization", &auth).call());
        record(a.get(&format!("{base}/api/session")).call());
        record(a.get(&format!("{base}/api/session?token={}", minted.owner)).call());
        record(a.post(&format!("{base}/api/ops/SetScript")).header("Authorization", &format!("Bearer {}", minted.listener)).send_json(serde_json::json!({})));
        record(a.post(&format!("{base}/api/ops/NoSuchOp")).header("Authorization", &auth).send_json(serde_json::json!({})));
        record(a.post(&format!("{base}/api/ops/EditScript")).header("Authorization", &auth).send_json(serde_json::json!({"path": "nope.star", "base_revision": 1, "edits": []})));
        record(a.get(&format!("{base}/api/documents/{}", minted.owner)).header("Authorization", &auth).call());

        let said = out::take_capture();
        let all = said.join("\n");
        assert_eq!(
            all.matches(&minted.browser).count(),
            1,
            "the browser token must appear exactly once in everything the server said:\n{all}"
        );
        assert!(
            said.iter().filter(|l| l.contains(&minted.browser)).all(|l| l.starts_with("open:  http://")),
            "the one occurrence must be the URL the user is told to open:\n{all}"
        );
        assert_eq!(all.matches(&minted.owner).count(), 0, "the agent token is never printed at all:\n{all}");
        assert_eq!(all.matches(&minted.listener).count(), 0, "the listener token is never printed at all:\n{all}");
        let responses = bodies.join("\n");
        for (what, t) in [("browser", &minted.browser), ("owner", &minted.owner), ("listener", &minted.listener)] {
            assert!(!responses.contains(t.as_str()), "the {what} token came back in a response body or header:\n{responses}");
        }
        let _ = std::fs::remove_dir_all(&dir);
    }

    #[test]
    fn a_path_traversal_attempt_is_refused_without_touching_the_disk() {
        let dir = std::env::temp_dir().join("cadpole-http-test-2");
        let _ = std::fs::create_dir_all(&dir);
        std::fs::write(dir.join(format!("real.{DOCUMENT_EXT}")), pb::Document::default().encode_to_vec()).unwrap();
        // The file a traversal would be reaching for.
        let secret = dir.parent().unwrap().join(format!("cadpole-traversal-secret.{DOCUMENT_EXT}"));
        std::fs::write(&secret, b"you should not be able to read this").unwrap();

        let (base, _app) = serve_for_test(None, dir);
        let a = agent();
        for attempt in [
            "../cadpole-traversal-secret",
            "..%2Fcadpole-traversal-secret",
            "%2e%2e%2f%2e%2e%2fetc%2fpasswd",
            "..",
            "a/b",
            "%00",
        ] {
            let code = status_of(a.get(&format!("{base}/api/documents/{attempt}")).call());
            assert!(code == 400 || code == 404 || code == 405, "{attempt:?} returned {code}, expected a refusal");
        }
        // The legitimate id still works, so the check is not just "refuse
        // everything".
        assert_eq!(status_of(a.get(&format!("{base}/api/documents/real")).call()), 200);
        let _ = std::fs::remove_file(&secret);
    }

    #[test]
    fn an_oversized_request_is_refused() {
        let (base, _app) = serve_for_test(None, std::env::temp_dir().join("cadpole-http-test-3"));
        let a = agent();
        // The router was built with a 64 KB cap.
        let big = serde_json::json!({"path": "main.star", "source": "#".repeat(200_000), "base_revision": 1});
        // The server refuses on the declared length, before reading the body,
        // and closes. A client still writing the body can see that close
        // (EPIPE, or a reset that discards the 413 over TCP) before it reads
        // the answer; either way the request was refused.
        match a.put(&format!("{base}/api/session/script")).send_json(&big) {
            Ok(r) => panic!("an oversized body was accepted: {}", r.status()),
            Err(ureq::Error::StatusCode(code)) => assert!(code == 413 || code == 400, "an oversized body returned {code}"),
            Err(ureq::Error::Io(e)) if matches!(e.kind(), std::io::ErrorKind::BrokenPipe | std::io::ErrorKind::ConnectionReset) => {}
            Err(e) => panic!("an oversized body failed some other way: {e}"),
        }
        assert_eq!(
            app_source(&_app),
            SRC,
            "the document is untouched"
        );

        // ...and a body under the HTTP cap but over the script cap is refused
        // by the handler with an explanation.
        let over_script = serde_json::json!({"path": "main.star", "source": "#".repeat(40_000), "base_revision": 1});
        let code = status_of(a.put(&format!("{base}/api/session/script")).send_json(&over_script));
        assert_eq!(code, 200, "40 KB is a perfectly ordinary script: {code}");
    }

    /// Who a message is from is the server's to say, on both spellings.
    ///
    /// `PostMessage` hardcodes "user"; the hand-written route stored whatever
    /// the caller sent, so a client could put words in the user's mouth or
    /// sign as the assistant, in the transcript every attached assistant
    /// reads. Not an escalation -- the listener cannot reach this route --
    /// but it is the invariant `capability.rs` argues from, and it was untrue
    /// on one of the two routes.
    #[test]
    fn a_message_is_from_whoever_the_server_says() {
        let (base, app) = serve_for_test(None, std::env::temp_dir().join("cadpole-http-test-from"));
        let a = reading_agent();
        let url = format!("{base}/api/session/messages");

        let code = status_of(a.post(&url).send_json(serde_json::json!({"from": "assistant", "text": "not me"})));
        assert_eq!(code, 400, "claiming to be somebody else is refused, not relabelled");

        assert_eq!(status_of(a.post(&url).send_json(serde_json::json!({"text": "hello"}))), 200);
        assert_eq!(status_of(a.post(&url).send_json(serde_json::json!({"from": "user", "text": "also me"}))), 200);

        let st = app.state.lock().unwrap();
        assert_eq!(st.messages.len(), 2, "only the two honest posts landed");
        assert!(st.messages.iter().all(|m| m.from == "user"), "{:?}", st.messages.iter().map(|m| &m.from).collect::<Vec<_>>());
    }

    // ---- StreamReply (reply_stream.rs, spec/reply_stream.qnt)

    /// A caller of `StreamReply`: the listener token under a client id.
    fn streamer(h: &ServerHost, client: &str) -> ServerHost {
        ServerHost { app: h.app.clone(), by: client.to_owned(), role: cadpole_ops::capability::Role::Listener }
    }

    fn piece(seq: u64, append: &str) -> pb::StreamReplyRequest {
        pb::StreamReplyRequest { seq, append: append.into(), ..Default::default() }
    }

    fn ending(seq: u64, append: &str, finish: pb::ReplyFinish) -> pb::StreamReplyRequest {
        pb::StreamReplyRequest { seq, append: append.into(), finish: finish as i32, ..Default::default() }
    }

    fn refusal(r: anyhow::Result<pb::StreamReplyResponse>) -> reply_stream::Refusal {
        r.expect_err("the call must be refused").downcast::<reply_stream::Refusal>().expect("a refusal from the stream rules")
    }

    fn message(h: &ServerHost, seq: u64) -> ChatMessage {
        h.app.state.lock().unwrap().messages.iter().find(|m| m.seq == seq).cloned().expect("the message")
    }

    fn message_events(h: &ServerHost) -> Vec<serde_json::Value> {
        h.app.state.lock().unwrap().events.iter().filter(|e| e.kind == "message").map(|e| e.data.clone()).collect()
    }

    /// `onlyTheAuthorWritesToItsMessage`: another client id under the same
    /// token, and the owner's token under the same id, are both somebody else.
    #[test]
    fn only_the_client_that_started_a_streamed_message_may_continue_it() {
        let h = host(SRC);
        let now = std::time::Instant::now();
        let author = streamer(&h, "listener:1");
        let seq = author.stream_reply_at(&piece(0, "Thick"), now).unwrap().seq;

        let other = streamer(&h, "listener:2");
        assert_eq!(refusal(other.stream_reply_at(&piece(seq, " and wrong"), now)), reply_stream::Refusal::NotTheAuthor(seq));
        assert_eq!(
            refusal(other.stream_reply_at(&ending(seq, "", pb::ReplyFinish::Done), now)),
            reply_stream::Refusal::NotTheAuthor(seq),
            "ending it is writing to it"
        );
        let owner = ServerHost { app: h.app.clone(), by: "listener:1".into(), role: cadpole_ops::capability::Role::Owner };
        assert_eq!(refusal(owner.stream_reply_at(&piece(seq, "!"), now)), reply_stream::Refusal::NotTheAuthor(seq));

        let r = author.stream_reply_at(&piece(seq, "er now."), now).unwrap();
        assert_eq!((r.state.as_str(), r.length), ("streaming", 12));
        assert_eq!(message(&h, seq).text, "Thicker now.");
    }

    /// `aFinishedMessageNeverChanges`, and a message posted whole is not one
    /// `StreamReply` can reach at all.
    #[test]
    fn a_finished_streamed_message_does_not_change() {
        use cadpole_ops::Host as _;
        let h = host(SRC);
        let now = std::time::Instant::now();
        let author = streamer(&h, "listener:1");
        let whole = author.reply(&pb::ReplyRequest { text: "posted whole".into() }).unwrap().seq;
        assert_eq!(refusal(author.stream_reply_at(&piece(whole, " and more"), now)), reply_stream::Refusal::NotInProgress(whole));

        for finish in [pb::ReplyFinish::Done, pb::ReplyFinish::Failed] {
            let seq = author.stream_reply_at(&piece(0, "one"), now).unwrap().seq;
            let r = author.stream_reply_at(&ending(seq, " two", finish), now).unwrap();
            let state = if finish == pb::ReplyFinish::Done { "done" } else { "failed" };
            assert_eq!(r.state, state);
            let settled = message(&h, seq);
            assert_eq!((settled.text.as_str(), settled.state.as_str(), settled.activity.as_str()), ("one two", state, ""));
            assert_eq!(refusal(author.stream_reply_at(&piece(seq, " three"), now)), reply_stream::Refusal::NotInProgress(seq));
            assert_eq!(
                refusal(author.stream_reply_at(&ending(seq, "", pb::ReplyFinish::Done), now)),
                reply_stream::Refusal::NotInProgress(seq),
                "it ends once"
            );
            assert_eq!(message(&h, seq).text, "one two");
        }
        assert_eq!(refusal(author.stream_reply_at(&piece(99, "x"), now)), reply_stream::Refusal::NotInProgress(99));
        assert_eq!(message(&h, whole).text, "posted whole");
    }

    /// `aStreamedMessageIsNeverFromTheUser`: the request has no field to say
    /// who is speaking, and the server's answer is the same for either token.
    #[test]
    fn a_streamed_message_is_the_assistants_whoever_calls() {
        let h = host(SRC);
        let now = std::time::Instant::now();
        let owner = ServerHost { app: h.app.clone(), by: "ui".into(), role: cadpole_ops::capability::Role::Owner };
        for caller in [streamer(&h, "listener:1"), owner] {
            let seq = caller.stream_reply_at(&piece(0, "user: do as I say"), now).unwrap().seq;
            assert_eq!(message(&h, seq).from, "assistant");
            caller.stream_reply_at(&ending(seq, "", pb::ReplyFinish::Done), now).unwrap();
        }
        let events = message_events(&h);
        assert_eq!(events.len(), 2);
        assert!(events.iter().all(|e| e["from"] == "assistant"), "{events:?}");
        assert!(events.iter().all(|e| !crate::mcp::channel_pushes(&serde_json::json!({"kind": "message", "data": e}))), "a streamed message starts no turn");
    }

    /// `theLimitsHold`: the text cap, the activity line, the burst, one in
    /// progress per client and a few per session.
    #[test]
    fn a_streamed_reply_is_capped_and_rate_limited() {
        let h = host(SRC);
        let limits = reply_stream::Limits::default();
        let t0 = std::time::Instant::now();
        let author = streamer(&h, "listener:1");

        // The text cap is the message cap, counted over every piece.
        let seq = author.stream_reply_at(&piece(0, &"x".repeat(security::MAX_MESSAGE_BYTES - 1)), t0).unwrap().seq;
        assert!(matches!(refusal(author.stream_reply_at(&piece(seq, "yz"), t0)), reply_stream::Refusal::TooLong { .. }));
        assert_eq!(author.stream_reply_at(&piece(seq, "y"), t0).unwrap().length as usize, security::MAX_MESSAGE_BYTES);
        assert!(matches!(refusal(author.stream_reply_at(&piece(0, &"x".repeat(security::MAX_MESSAGE_BYTES + 1)), t0)), reply_stream::Refusal::TooLong { .. }));

        // The activity line is short and one line.
        for bad in ["x".repeat(limits.max_activity + 1), "two\nlines".to_owned(), "bell\u{7}".to_owned()] {
            let req = pb::StreamReplyRequest { seq, activity: bad, ..Default::default() };
            assert!(matches!(refusal(author.stream_reply_at(&req, t0)), reply_stream::Refusal::BadActivity { .. }));
        }
        let req = pb::StreamReplyRequest { seq, activity: "rendering…".into(), ..Default::default() };
        author.stream_reply_at(&req, t0).unwrap();
        assert_eq!(message(&h, seq).activity, "rendering…");

        // The burst, then one call per refill period. A throttled call
        // changes nothing; the call that ends the message is never throttled.
        let seq = author.stream_reply_at(&piece(0, ""), t0).unwrap().seq;
        for i in 0..limits.burst {
            assert!(!author.stream_reply_at(&piece(seq, "a"), t0).unwrap().throttled, "call {i} is inside the burst");
        }
        let r = author.stream_reply_at(&piece(seq, "dropped"), t0).unwrap();
        assert!(r.throttled && r.length == u64::from(limits.burst));
        assert_eq!(message(&h, seq).text.len(), limits.burst as usize);
        let later = t0 + limits.refill;
        assert!(!author.stream_reply_at(&piece(seq, "b"), later).unwrap().throttled, "one refill period earns one call");
        assert!(author.stream_reply_at(&piece(seq, "c"), later).unwrap().throttled);
        let r = author.stream_reply_at(&ending(seq, ".", pb::ReplyFinish::Done), later).unwrap();
        assert!(!r.throttled && r.state == "done");

        // One in progress per client: the earlier one (the oversized one
        // above) was ended when this client started its next.
        let first = message(&h, 1);
        assert_eq!(first.state, "failed");
        assert!(first.text.ends_with(reply_stream::ABANDONED_NOTE));

        // And a few per session, whoever they belong to.
        for i in 0..limits.max_open {
            streamer(&h, &format!("extra:{i}")).stream_reply_at(&piece(0, ""), later).unwrap();
        }
        assert_eq!(
            refusal(streamer(&h, "one-too-many").stream_reply_at(&piece(0, ""), later)),
            reply_stream::Refusal::TooManyInProgress(limits.max_open)
        );
    }

    /// `everyMessageInProgressEndsInBoundedTime`: an author that goes away,
    /// and one that keeps its message open for ever.
    #[test]
    fn an_abandoned_streamed_message_is_ended_as_failed() {
        let h = host(SRC);
        let limits = reply_stream::Limits::default();
        let t0 = std::time::Instant::now();
        let author = streamer(&h, "listener:1");
        let req = pb::StreamReplyRequest { seq: 0, append: "Making the plate".into(), activity: "editing the script…".into(), ..Default::default() };
        let seq = author.stream_reply_at(&req, t0).unwrap().seq;

        assert_eq!(h.app.expire_streams(t0 + limits.idle - Duration::from_millis(1)), 0, "not yet");
        assert_eq!(message(&h, seq).state, "streaming");
        assert_eq!(h.app.expire_streams(t0 + limits.idle), 1);
        let m = message(&h, seq);
        assert_eq!(m.state, "failed");
        assert_eq!(m.activity, "", "nothing is left looking busy");
        assert_eq!(m.text, format!("Making the plate\n\n{}", reply_stream::ABANDONED_NOTE));
        assert_eq!(refusal(author.stream_reply_at(&piece(seq, " thicker"), t0 + limits.idle)), reply_stream::Refusal::NotInProgress(seq));
        let events = message_events(&h);
        assert_eq!(events.len(), 1, "one event, the end: {events:?}");
        assert_eq!((events[0]["seq"].as_u64(), events[0]["state"].as_str()), (Some(seq), Some("failed")));
        assert_eq!(h.app.expire_streams(t0 + limits.idle * 3), 0, "it is ended once");

        // Fed every few seconds, a message is still bounded by its age.
        let seq = author.stream_reply_at(&piece(0, ""), t0).unwrap().seq;
        let mut now = t0;
        while now < t0 + limits.life - Duration::from_secs(10) {
            now += Duration::from_secs(10);
            assert_eq!(h.app.expire_streams(now), 0);
            author.stream_reply_at(&piece(seq, ""), now).unwrap();
        }
        assert_eq!(h.app.expire_streams(t0 + limits.life), 1);
        assert_eq!(message(&h, seq).state, "failed");
    }

    /// `anUpdateIsNeverAnEvent`, `noStreamedMessageStartsATurn`: pieces go to
    /// the pages that asked for them and nowhere else. A long poll for
    /// messages, which is what `cadpole listen --next` and the channel run,
    /// sees nothing until the message ends, and then one event that is the
    /// assistant's.
    #[test]
    fn the_pieces_of_a_streamed_reply_are_frames_and_only_its_end_is_an_event() {
        let h = host(SRC);
        let now = std::time::Instant::now();
        let author = streamer(&h, "listener:1");
        let mut frames = h.app.live.subscribe();
        let mut woken = h.app.events.subscribe();
        let watch = pb::WaitEventsRequest { after: 0, timeout_ms: 1, kinds: vec!["message".into(), "selection_changed".into()], client: "listen".into() };
        let poll = EventPoll::start(&h.app, &watch, "listen");

        let req = pb::StreamReplyRequest { seq: 0, append: "Mak".into(), activity: "editing the script…".into(), ..Default::default() };
        let seq = author.stream_reply_at(&req, now).unwrap().seq;
        author.stream_reply_at(&piece(seq, "ing it thicker"), now).unwrap();
        assert!(h.app.state.lock().unwrap().events.is_empty(), "no event while it grows");
        assert!(woken.try_recv().is_err(), "and nothing woke a long poll");
        assert!(poll.ready(&h.app, &watch, false).is_none(), "a watcher of messages is still waiting");

        let a = frames.try_recv().unwrap();
        assert_eq!((a.seq, a.offset, a.length, a.append.as_str(), a.activity.as_str()), (seq, 0, 3, "Mak", "editing the script…"));
        assert_eq!((a.from.as_str(), a.state.as_str()), ("assistant", "streaming"));
        let b = frames.try_recv().unwrap();
        assert_eq!((b.offset, b.length, b.append.as_str(), b.activity.as_str()), (3, 17, "ing it thicker", ""));

        // The session read shows it in progress, for a page that joins now.
        let view = h.app.session_view(&h.app.state.lock().unwrap());
        let shown = view.messages.iter().find(|m| m.seq == seq).unwrap();
        assert_eq!((shown.state.as_str(), shown.text.as_str()), ("streaming", "Making it thicker"));

        author.stream_reply_at(&ending(seq, ".", pb::ReplyFinish::Done), now).unwrap();
        assert!(frames.try_recv().is_err(), "the end is the event, not another frame");
        let r = poll.ready(&h.app, &watch, false).expect("the end releases the poll");
        assert_eq!(r.events.len(), 1);
        let data = cadpole_ops::struct_to_json(r.events[0].data.as_ref().unwrap());
        assert_eq!((data["from"].as_str(), data["text"].as_str(), data["state"].as_str()), (Some("assistant"), Some("Making it thicker."), Some("done")));
        assert!(!crate::mcp::channel_pushes(&serde_json::json!({"kind": "message", "data": data})), "and it starts no turn");

        // A throttled call is neither.
        let seq = author.stream_reply_at(&piece(0, ""), now).unwrap().seq;
        let _ = frames.try_recv();
        let mut throttled = 0;
        for _ in 0..=reply_stream::Limits::default().burst {
            if author.stream_reply_at(&piece(seq, "x"), now).unwrap().throttled {
                throttled += 1;
                assert!(frames.try_recv().is_err());
            } else {
                frames.try_recv().unwrap();
            }
        }
        assert_eq!(throttled, 1);
    }

    /// The event stream carries frames only to a reader that asked, and a
    /// frame has no id, so it never moves the position a reader resumes from.
    #[test]
    fn the_event_stream_sends_frames_only_when_asked() {
        use std::io::Read as _;
        let (base, app) = serve_for_test(None, std::env::temp_dir().join("cadpole-http-test-live"));
        // Each reader says when its response has started: the handler has
        // returned by then, so both of its subscriptions exist. It reads
        // until it has every whole event it is waiting for; the timeout is
        // only a bound on a broken server. Events and frames come through two
        // channels, merged, so neither is assumed to arrive first.
        let (connected, ready) = std::sync::mpsc::channel::<()>();
        let read_until = |query: &str, kinds: &'static [&'static str]| {
            let url = format!("{base}/api/events?after=0{query}");
            let connected = connected.clone();
            std::thread::spawn(move || {
                let r = agent().get(&url).config().timeout_global(Some(Duration::from_secs(60))).build().call().unwrap();
                connected.send(()).unwrap();
                let mut body = r.into_body().into_reader();
                let mut text = Vec::new();
                let mut buf = [0u8; 4096];
                let has_all = |t: &[u8]| {
                    let t = String::from_utf8_lossy(t);
                    let whole = t.rsplit_once("\n\n").map_or("", |(done, _)| done);
                    kinds.iter().all(|k| whole.split("\n\n").any(|e| e.lines().any(|l| l.strip_prefix("event: ") == Some(*k))))
                };
                while !has_all(&text) {
                    match body.read(&mut buf) {
                        Ok(0) | Err(_) => break,
                        Ok(n) => text.extend_from_slice(&buf[..n]),
                    }
                }
                String::from_utf8_lossy(&text).into_owned()
            })
        };
        // The plain reader waits for an event sent after the live reader has
        // seen everything, so a frame wrongly sent to it has long arrived.
        let (plain, live) = (read_until("", &["message", "test_done"]), read_until("&live=1", &["message", "reply_stream"]));
        ready.recv().unwrap();
        ready.recv().unwrap();
        let author = ServerHost { app: app.clone(), by: "listener:1".into(), role: cadpole_ops::capability::Role::Listener };
        let now = std::time::Instant::now();
        let seq = author.stream_reply_at(&piece(0, "Mak"), now).unwrap().seq;
        author.stream_reply_at(&ending(seq, "ing.", pb::ReplyFinish::Done), now).unwrap();
        let live = live.join().unwrap();
        {
            let mut st = app.state.lock().unwrap();
            app.emit(&mut st, "test_done", serde_json::json!({}));
        }
        let plain = plain.join().unwrap();

        assert!(!plain.contains("reply_stream"), "a reader that did not ask sees no frame: {plain}");
        assert!(plain.contains("event: message") && plain.contains("Making."), "it sees the finished message: {plain}");
        let frame = live.split("\n\n").find(|f| f.contains("event: reply_stream")).unwrap_or_else(|| panic!("no frame in {live}"));
        assert!(!frame.contains("\nid:") && !frame.starts_with("id:"), "a frame has no id: {frame}");
        let data: serde_json::Value = serde_json::from_str(frame.lines().find_map(|l| l.strip_prefix("data: ")).unwrap()).unwrap();
        assert_eq!((data["seq"].as_u64(), data["kind"].as_str()), (Some(0), Some("reply_stream")));
        assert_eq!((data["data"]["seq"].as_u64(), data["data"]["append"].as_str(), data["data"]["offset"].as_u64()), (Some(seq), Some("Mak"), Some(0)));
        assert!(live.contains("event: message") && live.contains("Making."), "and the finished message as an event: {live}");
    }

    /// The traces of `spec/reply_stream.qnt` against the handler: every step
    /// of every trace is made as the call it stands for, under the model's
    /// limits and a clock the test moves, and after each one the outcome, the
    /// messages, the `message` events and the turns a watcher would start
    /// must be what the model says.
    #[test]
    fn the_model_traces_replay_against_the_handler() {
        use cadpole_ops::Host as _;
        let r = runfiles::Runfiles::create().expect("runfiles");
        let path = runfiles::rlocation!(r, "cadpole/spec/reply_stream_traces.json").expect("the traces are a data dependency");
        let bundle: serde_json::Value = serde_json::from_slice(&std::fs::read(path).unwrap()).unwrap();
        // The constants of reply_stream.qnt, a tick being one second.
        let limits = reply_stream::Limits {
            max_text: 3,
            max_activity: 120,
            burst: 2,
            refill: Duration::from_secs(1),
            idle: Duration::from_secs(2),
            life: Duration::from_secs(5),
            max_open: 2,
        };
        let int = |v: &serde_json::Value| -> u64 { v["#bigint"].as_str().map(|s| s.parse().unwrap()).or(v.as_u64()).unwrap_or_else(|| panic!("not a number: {v}")) };
        // A model text as the handler stores it: the pieces, and the server's
        // closing line on a line of its own.
        let text = |v: &serde_json::Value| -> String {
            let mut out = String::new();
            for t in v.as_array().unwrap() {
                match t.as_str().unwrap() {
                    "!" => {
                        if !out.is_empty() {
                            out.push_str("\n\n");
                        }
                        out.push_str(reply_stream::ABANDONED_NOTE);
                    }
                    piece => out.push_str(piece),
                }
            }
            out
        };

        let traces = bundle["traces"].as_array().unwrap();
        let mut steps = 0usize;
        let mut outcomes = std::collections::BTreeMap::<String, usize>::new();
        for (t, trace) in traces.iter().enumerate() {
            let h = host(SRC);
            h.app.state.lock().unwrap().streams = reply_stream::ReplyStreams::with_limits(limits.clone());
            let mut frames = h.app.live.subscribe();
            let mut now = std::time::Instant::now();
            for (i, state) in trace.as_array().unwrap().iter().enumerate().skip(1) {
                let at = format!("trace {t}, step {i}");
                let last = &state["last"];
                let (op, client) = (last["op"].as_str().unwrap(), last["client"].as_str().unwrap());
                let finish = match last["finish"].as_str().unwrap() {
                    "done" => pb::ReplyFinish::Done,
                    "failed" => pb::ReplyFinish::Failed,
                    _ => pb::ReplyFinish::Unspecified,
                };
                let outcome = match op {
                    "user_post" => {
                        ServerHost { app: h.app.clone(), by: "ui".into(), role: cadpole_ops::capability::Role::Owner }
                            .post_message(&pb::PostMessageRequest { text: "u".into(), attachments: Vec::new() })
                            .unwrap();
                        "ok"
                    }
                    "tick" => {
                        now += Duration::from_secs(1);
                        h.app.expire_streams(now);
                        "ok"
                    }
                    "start" | "update" => {
                        let req = pb::StreamReplyRequest {
                            seq: if op == "start" { 0 } else { int(&last["seq"]) },
                            append: last["chunk"].as_str().unwrap().into(),
                            activity: last["activity"].as_str().unwrap().into(),
                            finish: finish as i32,
                        };
                        match streamer(&h, client).stream_reply_at(&req, now) {
                            Ok(r) if r.throttled => "throttled",
                            Ok(r) => {
                                assert_eq!(r.seq, int(&last["seq"]), "{at}: the message the call was about");
                                "ok"
                            }
                            Err(e) => match e.downcast_ref::<reply_stream::Refusal>().unwrap_or_else(|| panic!("{at}: {e:#}")) {
                                reply_stream::Refusal::NotInProgress(_) => "not_in_progress",
                                reply_stream::Refusal::NotTheAuthor(_) => "not_author",
                                reply_stream::Refusal::TooLong { .. } => "too_long",
                                reply_stream::Refusal::TooManyInProgress(_) => "too_many",
                                reply_stream::Refusal::BadActivity { .. } => "bad_activity",
                            },
                        }
                    }
                    other => panic!("{at}: the model took a step this test does not know: {other}"),
                };
                assert_eq!(outcome, last["outcome"].as_str().unwrap(), "{at}: {last}");
                *outcomes.entry(format!("{op}/{outcome}")).or_default() += 1;

                // A frame for exactly the calls that left a message growing.
                let grew = matches!(op, "start" | "update") && outcome == "ok" && finish == pb::ReplyFinish::Unspecified;
                assert_eq!(frames.try_recv().is_ok(), grew, "{at}: a frame");
                assert!(frames.try_recv().is_err(), "{at}: at most one frame a call");

                let st = h.app.state.lock().unwrap();
                let mut want: Vec<(u64, String, String, String, String)> = state["msgs"]["#map"]
                    .as_array()
                    .unwrap()
                    .iter()
                    .map(|kv| {
                        let m = &kv[1];
                        let field = |k: &str| m[k].as_str().unwrap().to_owned();
                        (int(&kv[0]), field("from"), field("state"), text(&m["text"]), field("activity"))
                    })
                    .collect();
                want.sort();
                let have: Vec<(u64, String, String, String, String)> =
                    st.messages.iter().map(|m| (m.seq, m.from.clone(), m.state.clone(), m.text.clone(), m.activity.clone())).collect();
                assert_eq!(have, want, "{at}: the messages");
                assert_eq!(st.streams.in_progress(), want.iter().filter(|m| m.2 == "streaming").count(), "{at}: in progress");

                assert!(st.events.iter().all(|e| e.kind == "message"), "{at}: only `message` events");
                let log: Vec<(u64, String)> = st.events.iter().map(|e| (e.data["seq"].as_u64().unwrap(), e.data["from"].as_str().unwrap().to_owned())).collect();
                let want_log: Vec<(u64, String)> =
                    state["log"].as_array().unwrap().iter().map(|e| (int(&e["seq"]), e["from"].as_str().unwrap().to_owned())).collect();
                assert_eq!(log, want_log, "{at}: the event log");
                let wakes = st.events.iter().filter(|e| crate::mcp::channel_pushes(&serde_json::to_value(e).unwrap())).count() as u64;
                assert_eq!(wakes, int(&state["wakes"]), "{at}: turns a watcher starts");
                steps += 1;
            }
        }
        // What the replay covered, so a generator that quietly produced less
        // fails here rather than passing on nothing.
        assert!(traces.len() >= 300 && steps >= 9_000, "{} traces, {steps} steps", traces.len());
        for outcome in ["start/ok", "start/too_many", "update/ok", "update/throttled", "update/not_in_progress", "update/not_author", "update/too_long", "tick/ok", "user_post/ok"] {
            assert!(outcomes.get(outcome).copied().unwrap_or(0) > 0, "no trace reached {outcome}: {outcomes:?}");
        }
        println!("replayed {} traces, {steps} steps: {outcomes:?}", traces.len());
    }

    /// An operation is asked for by name, never by percent-escape.
    ///
    /// The router decodes before it matches, so `/api/ops/Post%4dessage`
    /// dispatches as `PostMessage` while the capability guard sees the raw
    /// path and resolves nothing. That failed closed, because an unresolvable
    /// name is granted to no role but the owner -- but the guard and the
    /// dispatcher naming different operations is not a property to leave for
    /// the next role to rediscover.
    #[test]
    fn an_operation_may_not_be_asked_for_by_percent_escape() {
        let (base, _app) = serve_for_test(None, std::env::temp_dir().join("cadpole-http-test-pct"));
        let a = reading_agent();
        for path in ["/api/ops/Post%4dessage", "/api/ops/Get%53ession", "/api/cadpole.v1.CadpoleOps/Get%53ession"] {
            let code = status_of(a.post(&format!("{base}{path}")).send_json(serde_json::json!({})));
            assert_eq!(code, 403, "{path} should be refused before anything dispatches it");
        }
        // ...and the name itself still works, so this refuses a spelling and
        // not the operation.
        assert_eq!(status_of(a.post(&format!("{base}/api/ops/GetSession")).send_json(serde_json::json!({}))), 200);
    }

    /// The two spellings of "set a parameter" are *different operations*, and
    /// this pins which is which so neither drifts into the other.
    ///
    /// `SetParams` is a delta -- `overrides` plus `clear` -- and this route is
    /// a replace, which is what every client-facing API in the tree presents:
    /// `api.ts` computes the `clear` list to build replace on the rpc, and
    /// `EngineClient.setParams` documents replace outright. A merge here would
    /// look consistent with the rpc, and would quietly
    /// take away the only way `RemoteEngine`, which caches no session, has of
    /// removing an override.
    /// A reader that attaches with a backlog knows the Listen toggle only as
    /// it is now. Each change in the log says what the toggle was before it,
    /// so the reader can judge the events in front of the change by the
    /// toggle they were sent under (spec/wakeups.qnt).
    #[test]
    fn a_listen_toggle_event_says_what_the_toggle_was() {
        let (base, app) = serve_for_test(None, std::env::temp_dir().join("cadpole-http-test-listen-toggle"));
        let a = agent();
        let url = format!("{base}/api/session/assistant");
        for on in [true, true, false] {
            assert_eq!(status_of(a.put(&url).send_json(serde_json::json!({"listening": on}))), 200);
        }
        let st = app.state.lock().unwrap();
        let toggles: Vec<(bool, bool)> = st
            .events
            .iter()
            .filter(|e| e.kind == "assistant_listening")
            .map(|e| (e.data["was"].as_bool().unwrap(), e.data["listening"].as_bool().unwrap()))
            .collect();
        assert_eq!(toggles, vec![(false, true), (true, true), (true, false)], "(was, listening), a repeat included");
    }

    #[test]
    fn the_params_route_replaces_and_the_rpc_merges() {
        use cadpole_ops::Host as _;
        let (base, app) = serve_for_test(None, std::env::temp_dir().join("cadpole-http-test-params"));
        let a = agent();
        let url = format!("{base}/api/session/params");
        let names = |app: &Shared| {
            let st = app.state.lock().unwrap();
            let mut n: Vec<String> = st.document.param_overrides.keys().cloned().collect();
            n.sort();
            n
        };
        assert_eq!(status_of(a.put(&url).send_json(serde_json::json!({"overrides": {"w": 40, "h": 10}}))), 200);

        // The route: what you send is what the document holds afterwards.
        assert_eq!(status_of(a.put(&url).send_json(serde_json::json!({"overrides": {"w": 41}}))), 200);
        assert_eq!(names(&app), vec!["w".to_owned()], "the route replaces");

        // The rpc, on the same session: a name it does not mention survives.
        let h = ServerHost { app: app.clone(), by: "test".into(), role: cadpole_ops::capability::Role::Owner };
        h.set_params(&pb::SetParamsRequest {
            overrides: [("h".to_owned(), pb::ParamValue { value: Some(pb::param_value::Value::Number(10.0)) })].into(),
            ..Default::default()
        })
        .unwrap();
        assert_eq!(names(&app), vec!["h".to_owned(), "w".to_owned()], "the rpc merges");

        // `clear` is accepted on the route too, so a caller can be explicit
        // rather than relying on omission.
        assert_eq!(
            status_of(a.put(&url).send_json(serde_json::json!({"overrides": {"w": 41, "h": 10}, "clear": ["h"]}))),
            200
        );
        assert_eq!(names(&app), vec!["w".to_owned()]);

        // Both carry the same caps, and a refusal writes nothing.
        let long = "k".repeat(security::MAX_PARAM_KEY_BYTES + 1);
        assert_eq!(status_of(a.put(&url).send_json(serde_json::json!({"overrides": {long: 1}}))), 400);
        assert_eq!(names(&app), vec!["w".to_owned()], "a refused write left the document alone");
    }

    /// A long poll for one kind is not released by an event of another.
    ///
    /// `kinds` was applied to the response after the loop had already decided
    /// to return, so any traffic at all released a caller waiting for
    /// `client_failure` and handed it an empty list. An assistant following
    /// the documented recipe got a busy loop where it had asked to be woken.
    #[test]
    fn waiting_for_one_kind_is_not_woken_by_another() {
        use cadpole_ops::Host as _;
        let h = Arc::new(host(SRC));
        {
            // Something in the log already, of a kind nobody here wants.
            let mut st = h.app.state.lock().unwrap();
            h.app.emit_by(&mut st, "params_changed", serde_json::json!({"n": 1}), "someone-else");
        }
        let waiter = {
            let h = h.clone();
            std::thread::spawn(move || {
                h.wait_events(&pb::WaitEventsRequest {
                    after: 0,
                    timeout_ms: 2_000,
                    kinds: vec!["client_failure".into()],
                    client: "watcher".into(),
                })
                .unwrap()
            })
        };
        // Long enough that a poll released by the wrong kind has certainly
        // returned, and far short of the two-second budget.
        std::thread::sleep(Duration::from_millis(300));
        {
            let mut st = h.app.state.lock().unwrap();
            h.app.emit_by(&mut st, "client_failure", serde_json::json!({"message": "boom"}), "someone-else");
        }
        let r = waiter.join().unwrap();
        assert_eq!(r.events.len(), 1, "the wait must end on the kind that was asked for: {:?}", r.events);
        assert_eq!(r.events[0].kind, "client_failure");
        assert!(r.cursor >= r.events[0].seq, "the cursor covers everything examined");
    }

    /// A writer is told the revision *its own write* produced, not whatever
    /// the session reached while its response was being built.
    ///
    /// `feedback` evaluates the whole document with the lock dropped, and the
    /// response used to re-read `st.revision` afterwards. So a writer whose
    /// document is slow to evaluate got back a number another writer had
    /// produced -- and sent it as `base_revision` on its next request, where
    /// it matched, the check passed, and the other write was overwritten with
    /// no conflict reported anywhere. The optimistic concurrency defeated
    /// itself. `spec/revisions.qnt` states the rule over what
    /// the writer *observed* rather than over what it sent.
    ///
    /// The interfering write waits for the commit's `script_changed` event,
    /// not for a fixed time, so it always lands after the commit; and the
    /// evaluation it must land inside is millions of interpreter steps, so a
    /// loaded machine makes that window wider rather than the test flakier.
    /// What is asserted is a number, never a duration.
    #[test]
    fn a_writer_is_told_its_own_revision_not_one_another_writer_produced() {
        use cadpole_ops::Host as _;
        let h = Arc::new(host(SRC));
        let slow = "t = 0\nfor i in range(1500):\n    for j in range(1500):\n        t = t + 1\nb = box(1,1,1)\n";
        let before = h.app.state.lock().unwrap().revision;
        let mut events = h.app.events.subscribe();

        let writer = {
            let h = h.clone();
            let slow = slow.to_owned();
            std::thread::spawn(move || {
                h.set_script(&pb::SetScriptRequest { path: "main.star".into(), source: slow, base_revision: before }).unwrap()
            })
        };

        // Let the write commit, then move the session on underneath it while
        // it is out of the lock evaluating.
        loop {
            let ev = events.blocking_recv().expect("the session's events");
            if ev.kind == "script_changed" {
                break;
            }
        }
        h.set_params(&pb::SetParamsRequest {
            overrides: [("w".to_owned(), pb::ParamValue { value: Some(pb::param_value::Value::Number(41.0)) })].into(),
            ..Default::default()
        })
        .unwrap();

        let r = writer.join().unwrap();
        assert!(!r.conflict);
        assert_eq!(r.revision, before + 1, "the writer must be told the revision it produced, not {}", h.app.state.lock().unwrap().revision);
        assert!(h.app.state.lock().unwrap().revision > before + 1, "the interfering write really did land");
    }

    /// The lost update this route used to allow, as the two writers that hit
    /// it: A writes against the revision it read, and B -- which never saw
    /// A's write -- omits `base_revision` and clobbers under a 200. The op
    /// route refused exactly this; the hand-written one did not, which is the
    /// real cost of answering the same write in two places.
    #[test]
    fn a_script_write_that_names_no_base_revision_is_refused_rather_than_clobbering() {
        let (base, app) = serve_for_test(None, std::env::temp_dir().join("cadpole-http-test-lost-update"));
        let a = agent();
        let url = format!("{base}/api/session/script");
        let read = app.state.lock().unwrap().revision;

        let mine = serde_json::json!({"path": "main.star", "source": "# A\n", "base_revision": read});
        assert_eq!(status_of(a.put(&url).send_json(&mine)), 200, "an honest write still works");
        assert_eq!(app_source(&app), "# A\n");

        // B never read past `read`, and says nothing about what it is based on.
        let theirs = serde_json::json!({"path": "main.star", "source": "# B never saw A\n"});
        let code = status_of(a.put(&url).send_json(&theirs));
        assert!(code == 400 || code == 422, "a write with no base revision returned {code}");
        assert_eq!(app_source(&app), "# A\n", "A's work survived");

        // And naming the revision it actually read is a conflict, not a write.
        let stale = serde_json::json!({"path": "main.star", "source": "# B\n", "base_revision": read});
        assert_eq!(status_of(a.put(&url).send_json(&stale)), 409);
        assert_eq!(app_source(&app), "# A\n", "a stale write is refused too");
    }

    fn app_source(app: &Shared) -> String {
        app.state.lock().unwrap().document.scripts[0].source.clone()
    }

    /// Two names that slug to the same id are two documents, not one.
    ///
    /// `slug` is many-to-one by design (case, punctuation and anything past
    /// 48 characters all collapse), so the id alone cannot say whether the
    /// file under it is the document being saved. Save As used to write
    /// straight over it: "My Bracket v1.0" and then "MY BRACKET V1/0" left one
    /// file, holding only the second.
    #[test]
    fn save_as_will_not_overwrite_a_different_document_with_the_same_id() {
        let dir = std::env::temp_dir().join(format!("cadpole-http-test-saveas-{}", std::process::id()));
        let _ = std::fs::remove_dir_all(&dir);
        std::fs::create_dir_all(&dir).unwrap();
        let (base, app) = serve_for_test(None, dir.clone());
        let a = reading_agent();
        let save_as = |body: serde_json::Value| a.post(&format!("{base}/api/session/save-as")).send_json(body).unwrap();
        let file = dir.join(format!("my-bracket-v1-0.{DOCUMENT_EXT}"));

        assert_eq!(save_as(serde_json::json!({"name": "My Bracket v1.0"})).status().as_u16(), 200);
        let first = std::fs::read(&file).unwrap();
        // Saving again under the same name is an ordinary save.
        assert_eq!(save_as(serde_json::json!({"name": "My Bracket v1.0"})).status().as_u16(), 200);

        let rev = app.state.lock().unwrap().revision;
        let edit = serde_json::json!({"path": "main.star", "source": "b = box(1, 2, 3)\n", "base_revision": rev});
        assert_eq!(status_of(a.put(&format!("{base}/api/session/script")).send_json(&edit)), 200);

        // A different name with the same slug is somebody else's file.
        let mut r = save_as(serde_json::json!({"name": "MY BRACKET V1/0"}));
        assert_eq!(r.status().as_u16(), 409, "a colliding name must be refused");
        let why = r.body_mut().read_to_string().unwrap();
        assert!(why.contains("my-bracket-v1-0") && why.contains("overwrite"), "the refusal says what and how: {why}");
        let after = std::fs::read(&file).unwrap();
        assert_eq!(
            pb::Document::decode(&after[..]).unwrap().name,
            pb::Document::decode(&first[..]).unwrap().name,
            "the first document is still there"
        );
        assert_eq!(app.state.lock().unwrap().document.name, "My Bracket v1.0", "a refused save does not rename the session");

        // ...unless the caller says so.
        assert_eq!(save_as(serde_json::json!({"name": "MY BRACKET V1/0", "overwrite": true})).status().as_u16(), 200);
        assert_eq!(pb::Document::decode(&std::fs::read(&file).unwrap()[..]).unwrap().name, "MY BRACKET V1/0");

        // A file this session never wrote is not this session's to replace,
        // even under the same name.
        let other = dir.join(format!("elsewhere.{DOCUMENT_EXT}"));
        let foreign = pb::Document { id: "another".into(), name: "Elsewhere".into(), ..Default::default() };
        std::fs::write(&other, foreign.encode_to_vec()).unwrap();
        assert_eq!(save_as(serde_json::json!({"name": "Elsewhere"})).status().as_u16(), 409);
        assert_eq!(std::fs::read(&other).unwrap(), foreign.encode_to_vec());
        let _ = std::fs::remove_dir_all(&dir);
    }

    /// The content-type guard in `api_guard`, at the level it actually runs.
    ///
    /// `check_content_type` has a unit test, and a form body is refused on
    /// `PUT /api/session/script` -- but that route's `Json<T>` extractor
    /// answers 415 all by itself, so deleting the guard changes nothing there
    /// and the wiring went unverified. `POST /api/ops/<Op>` takes raw `Bytes`
    /// and sniffs the content type only to choose JSON or protobuf, so it is
    /// where the guard is the only thing between a cross-origin form post and
    /// an operation -- and a form is exactly the request a browser will send
    /// to another origin without a preflight.
    #[test]
    fn a_form_content_type_is_refused_on_the_operation_route() {
        let (base, app) = serve_for_test(None, std::env::temp_dir().join("cadpole-http-test-formct"));
        let a = agent();
        let body = serde_json::json!({"path": "main.star", "source": "b = box(2, 2, 2)\n", "base_revision": 1}).to_string();
        for ct in ["application/x-www-form-urlencoded", "multipart/form-data; boundary=x", "text/plain"] {
            let code = status_of(a.post(&format!("{base}/api/ops/SetScript")).content_type(ct).send(body.as_str()));
            assert_eq!(code, 415, "{ct} must be refused on /api/ops");
        }
        assert_eq!(app_source(&app), SRC, "nothing was written");
        // The same bytes with a content type a form cannot set are accepted,
        // so the refusal is about the type and not about the body.
        let code = status_of(a.post(&format!("{base}/api/ops/SetScript")).content_type("application/json").send(body.as_str()));
        assert_eq!(code, 200, "an honest JSON post still works");
        assert!(app_source(&app).contains("box(2, 2, 2)"), "...and wrote");
    }

    /// The override cap on the hand-written route, not just on the rpc.
    ///
    /// `set_params_cannot_grow_the_session_document_without_bound` covers
    /// `SetParams`. `PUT /api/session/params` re-implements the same bounds by
    /// hand and is the spelling a CLI or CadpoleKit caller still reaches, so its
    /// copy of the check needs its own test: deleting it left every test in
    /// the tree green.
    #[test]
    fn the_legacy_params_route_enforces_the_override_cap() {
        let (base, app) = serve_for_test(None, std::env::temp_dir().join("cadpole-http-test-params"));
        let a = reading_agent();
        let url = format!("{base}/api/session/params");
        // Serialised here rather than by `send_json`, which streams a large
        // map without a Content-Length and can lose it to the router's body
        // limit before the route's own check is ever reached. Integer values
        // keep 4097 overrides inside that limit.
        let body = |n: usize| {
            let m: serde_json::Map<String, serde_json::Value> = (0..n).map(|i| (format!("{i}"), serde_json::json!(1))).collect();
            serde_json::to_string(&serde_json::json!({ "overrides": m })).unwrap()
        };
        let put = |text: &str| {
            let r = a.put(&url).content_type("application/json").send(text);
            match r {
                // Cut: on a failure this is the answer to a 4097-key write,
                // and the whole session view in a panic message helps nobody.
                Ok(mut resp) => (
                    resp.status().as_u16(),
                    resp.body_mut().read_to_string().unwrap_or_default().chars().take(200).collect::<String>(),
                ),
                Err(e) => panic!("request failed: {e}"),
            }
        };

        // An ordinary write first, so the refusal below has something it could
        // damage.
        assert_eq!(put(&body(3)).0, 200);

        let (code, text) = put(&body(security::MAX_PARAM_OVERRIDES + 1));
        assert_eq!(code, 400, "{} overrides is over the cap: {text}", security::MAX_PARAM_OVERRIDES + 1);
        assert!(text.contains("parameter overrides"), "the refusal says what the limit is: {text}");
        // The route *replaces* rather than merges, so the check has to come
        // before the clear or a refused write empties the document.
        assert_eq!(
            app.state.lock().unwrap().document.param_overrides.len(),
            3,
            "a refused write leaves the overrides as they were"
        );
    }

    /// `X-Cadpole-Client` is caller-controlled text that reaches the event log's
    /// `by` field, every SSE subscriber and the server's `tracing` lines.
    /// `client_id` strips control characters and cuts it at 64; dropping the
    /// filter passed every test in the tree.
    /// `report_text_cannot_forge_a_log_line_and_is_cut_visibly` covers report
    /// text, which is a different field on a different route.
    #[test]
    fn a_client_id_cannot_carry_control_characters_or_grow_without_bound() {
        let (base, app) = serve_for_test(None, std::env::temp_dir().join("cadpole-http-test-clientid"));
        let a = agent();
        let last_by = || app.state.lock().unwrap().events.last().expect("an event").by.clone();
        let write_as = |who: &str| {
            status_of(
                a.put(&format!("{base}/api/session/params"))
                    .header("X-Cadpole-Client", who)
                    .send_json(serde_json::json!({"overrides": {"w": 41.0}})),
            )
        };

        // A tab is the one control character an HTTP header value may carry,
        // so it is the one a client can actually send -- and it is enough to
        // split a log line into columns that are not there.
        assert_eq!(write_as("we\tb"), 200);
        assert_eq!(last_by(), "web", "the tab is stripped rather than carried into the log");

        // ...and the length is bounded, so no caller writes an unbounded
        // string into every event it causes.
        assert_eq!(write_as(&"z".repeat(200)), 200);
        assert_eq!(last_by(), "z".repeat(64), "a client id is cut at 64 characters");

        // An ordinary id is untouched, so this is a filter and not a mangle.
        assert_eq!(write_as("cadpole-cli"), 200);
        assert_eq!(last_by(), "cadpole-cli");
    }

    #[test]
    fn a_cross_origin_write_is_refused_over_http() {
        let (base, app) = serve_for_test(None, std::env::temp_dir().join("cadpole-http-test-4"));
        let a = agent();
        let body = serde_json::json!({"path": "main.star", "source": "b = box(1,1,1)\n", "base_revision": 1});
        let code = status_of(
            a.put(&format!("{base}/api/session/script"))
                .header("Origin", "https://evil.example")
                .header("Sec-Fetch-Site", "cross-site")
                .send_json(&body),
        );
        assert_eq!(code, 403, "a page on another origin must not be able to write");
        assert_eq!(app_source(&app), SRC, "nothing was written");

        // A form-encoded post cannot set application/json, so it is refused
        // even without an Origin.
        let code = status_of(a.put(&format!("{base}/api/session/script")).content_type("text/plain").send("x=1"));
        assert_eq!(code, 415);

        // Reads are refused cross-site too, the event stream included: a page
        // on another origin is a browser being aimed at this server by
        // somebody else's code, whether it is reading or writing.
        for path in ["/api/session", "/api/events?after=0", "/api/model"] {
            let code = status_of(
                a.get(&format!("{base}{path}"))
                    .header("Origin", "https://evil.example")
                    .header("Sec-Fetch-Site", "cross-site")
                    .call(),
            );
            assert_eq!(code, 403, "a cross-site GET of {path} must be refused");
        }
        // The UI's own reads are not.
        let code = status_of(
            a.get(&format!("{base}/api/session"))
                .header("Origin", &format!("http://{}", base.trim_start_matches("http://")))
                .header("Sec-Fetch-Site", "same-origin")
                .call(),
        );
        assert_eq!(code, 200);
    }

    /// DNS rebinding against `--no-auth`.
    ///
    /// A page on evil.example re-points its own name at 127.0.0.1; from then
    /// on the browser calls this server *as* evil.example, so `Origin` and
    /// `Host` agree and `Sec-Fetch-Site` says same-origin. Every origin check
    /// passes; only the Host says which site the browser thinks it is talking
    /// to. With no token there is nothing else in the way, so a no-auth
    /// server answers only to its loopback names.
    #[test]
    fn a_no_auth_server_refuses_a_rebound_host() {
        let (base, app) = serve_for_test(None, std::env::temp_dir().join("cadpole-http-test-rebind"));
        let port = base.rsplit(':').next().unwrap().to_owned();
        let a = reading_agent();
        let rebound = |path: &str, body: Option<serde_json::Value>| {
            let host = format!("evil.example.com:{port}");
            let origin = format!("http://{host}");
            let r = match body {
                Some(b) => a
                    .post(&format!("{base}{path}"))
                    .header("Host", &host)
                    .header("Origin", &origin)
                    .header("Sec-Fetch-Site", "same-origin")
                    .send_json(b),
                None => a.get(&format!("{base}{path}")).header("Host", &host).header("Origin", &origin).header("Sec-Fetch-Site", "same-origin").call(),
            };
            status_of(r)
        };
        assert_eq!(rebound("/api/ops/GetSession", Some(serde_json::json!({}))), 403, "a rebound read");
        let edit = serde_json::json!({"path": "main.star", "source": "box(1,2,3)\n", "base_revision": 1});
        assert_eq!(rebound("/api/ops/SetScript", Some(edit)), 403, "a rebound write");
        assert_eq!(app_source(&app), SRC, "nothing was written");
        assert_eq!(rebound("/", None), 403, "the page itself is not served under a foreign name");

        // The loopback names still work, with and without a port.
        for host in [format!("127.0.0.1:{port}"), format!("localhost:{port}"), "localhost".to_owned(), format!("[::1]:{port}")] {
            let code = status_of(a.post(&format!("{base}/api/ops/GetSession")).header("Host", &host).send_json(serde_json::json!({})));
            assert_eq!(code, 200, "Host {host}");
        }

        // With a token the bearer is the defence, and a tailnet name the
        // server cannot know in advance must keep working.
        let (base, _app) = serve_for_test(Some("tok"), std::env::temp_dir().join("cadpole-http-test-rebind-2"));
        let code = status_of(
            a.post(&format!("{base}/api/ops/GetSession"))
                .header("Host", "box.tailnet.example:7331")
                .header("Authorization", "Bearer tok")
                .send_json(serde_json::json!({})),
        );
        assert_eq!(code, 200);
    }

    /// `--max-body` is the request limit on every route, up as well as down.
    ///
    /// The router honoured it, but `post_op`, `post_connect` and `post_call`
    /// each re-checked the compiled-in 8 MB default, so raising the flag was
    /// announced in the banner and then refused with a 413.
    #[test]
    fn a_raised_body_limit_is_honoured_by_every_route() {
        let limit = security::MAX_BODY_BYTES + 4 * 1024 * 1024;
        let (base, _app) =
            serve_custom_for_test(security::TokenSet::open(), std::env::temp_dir().join("cadpole-http-test-maxbody"), 512, limit);
        let a = reading_agent();
        // 10 MB: over the default, under the flag. An unknown field is ignored
        // by the operation, so it is pure size.
        let pad = "x".repeat(security::MAX_BODY_BYTES + 2 * 1024 * 1024);
        let body = serde_json::to_vec(&serde_json::json!({"feature_id": "blk", "padding": pad})).unwrap();
        let code = status_of(a.post(&format!("{base}/api/ops/ExplainRef")).content_type("application/json").send(&body[..]));
        assert_eq!(code, 200, "/api/ops refused a body the server was told to accept");
        let code = status_of(
            a.post(&format!("{base}{}/ExplainRef", connect::service_prefix())).content_type("application/json").send(&body[..]),
        );
        assert_eq!(code, 200, "the Connect route refused a body the server was told to accept");
        // /api/call: an envelope this size is refused by the engine for what
        // it is, not by the server for its size.
        let code = status_of(a.post(&format!("{base}/api/call")).content_type("application/x-protobuf").send(&body[..]));
        assert_ne!(code, 413, "/api/call refused a body the server was told to accept");

        // ...and over the flag is still a refusal.
        let over = vec![b' '; limit + 1];
        let code = status_of(a.post(&format!("{base}/api/ops/ExplainRef")).content_type("application/json").send(&over[..]));
        assert_eq!(code, 413);
    }

    /// `/api/call` evaluates against the session's assets when the posted
    /// document carries none.
    ///
    /// The Apple RemoteEngine rebuilds the document from the session's JSON
    /// view, which names assets but never carries their bytes, so every
    /// script that read one failed with `asset "badge.svg" not found
    /// (available: none)` while the web UI showed the model. `Solve` already
    /// filled an asset-less document from the session; this route did not.
    #[test]
    fn the_envelope_route_lends_the_session_assets_to_an_asset_less_document() {
        use cadpole_ops::Host as _;
        let (base, app) = serve_for_test(None, std::env::temp_dir().join("cadpole-http-test-call-assets"));
        let svg = b"<svg xmlns='http://www.w3.org/2000/svg' width='20' height='20'><rect id='r' x='0' y='0' width='20' height='20'/></svg>";
        let host = ServerHost { app: app.clone(), by: "test".into(), role: cadpole_ops::capability::Role::Owner };
        host.add_asset(&pb::AddAssetRequest { name: "badge.svg".into(), data: svg.to_vec(), base_revision: 1, ..Default::default() }).unwrap();

        let script = "art = import_svg(asset(\"badge.svg\"), name = \"art\")\nb = extrude(art, 2, name = \"b\")\n";
        let doc = cadpole_engine::document_from_scripts("t", vec![("main.star".into(), script.into())]);
        assert!(doc.assets.is_empty());
        let req = pb::Request {
            request: Some(pb::request::Request::Evaluate(pb::EvaluateRequest { document: Some(doc), skip_mesh: true, ..Default::default() })),
        };
        let mut r = agent()
            .post(&format!("{base}/api/call"))
            .content_type("application/x-protobuf")
            .send(&req.encode_to_vec()[..])
            .unwrap();
        let bytes = r.body_mut().read_to_vec().unwrap();
        let resp = pb::Response::decode(&bytes[..]).unwrap();
        let Some(pb::response::Response::Evaluate(ev)) = resp.response else { panic!("not an evaluation: {resp:?}") };
        let model = ev.model.unwrap_or_default();
        let errors: Vec<String> = model.diagnostics.iter().map(|d| d.message.clone()).collect();
        assert!(!errors.iter().any(|m| m.contains("not found")), "the session's asset was not lent: {errors:?}");
        assert!(!model.bodies.is_empty(), "the model evaluated: {errors:?}");
    }

    #[test]
    fn the_generated_ops_answer_over_http() {
        let (base, _app) = serve_for_test(None, std::env::temp_dir().join("cadpole-http-test-5"));
        let a = agent();
        let mut r = a.post(&format!("{base}/api/ops/ExplainRef")).send_json(serde_json::json!({"feature_id": "blk"})).unwrap();
        let v: serde_json::Value = r.body_mut().read_json().unwrap();
        assert_eq!(v["status"], "REF_STATUS_OK", "{v}");
        assert_eq!(v["chain"][0]["feature_id"], "blk");

        let mut r = a
            .post(&format!("{base}/api/ops/EditScript"))
            .send_json(serde_json::json!({
                "path": "main.star",
                "base_revision": 1,
                "edits": [{"old_str": "param(\"w\", 40)", "new_str": "param(\"w\", 60)"}]
            }))
            .unwrap();
        let v: serde_json::Value = r.body_mut().read_json().unwrap();
        assert_eq!(v["revision"], 2, "{v}");
        assert!(v["diff"].as_str().unwrap().contains("+w = param(\"w\", 60)"), "{v}");

        // The same edit again is now stale.
        let mut r = a
            .post(&format!("{base}/api/ops/EditScript"))
            .send_json(serde_json::json!({
                "path": "main.star",
                "base_revision": 1,
                "edits": [{"old_str": "param(\"w\", 60)", "new_str": "param(\"w\", 70)"}]
            }))
            .unwrap();
        let v: serde_json::Value = r.body_mut().read_json().unwrap();
        assert_eq!(v["conflict"], true, "{v}");
        assert_eq!(v["current_revision"], 2, "{v}");
        assert!(v["current_source"].as_str().unwrap().contains("60"), "{v}");

        let mut r = a.post(&format!("{base}/api/ops/Ack")).send_json(serde_json::json!({"client": "t", "cursor": 1})).unwrap();
        let v: serde_json::Value = r.body_mut().read_json().unwrap();
        assert_eq!(v["client"], "t");

        assert_eq!(status_of(a.post(&format!("{base}/api/ops/NoSuchOp")).send_json(serde_json::json!({}))), 404);
    }

    // -----------------------------------------------------------------------
    // Security properties of the RPC surface, each asserting the behaviour
    // the docs *claim*.
    // -----------------------------------------------------------------------

    /// docs/security.md ("Capabilities") says the listener's
    /// granted `Evaluate`/`Render`/`Sweep`/`Export` are "bounded by the
    /// Starlark sandbox, the evaluation deadline ... the same bounds every
    /// other caller gets", and `capability.rs` grants `Evaluate` because it is
    /// "under the server's evaluation deadline". An `EvaluateRequest` carries
    /// `deadline_ms`, `max_ticks` and `max_heap_bytes`; taken unclamped, they
    /// would let a restricted listener token run an evaluation far longer,
    /// with more ticks and a larger heap, than `cadpole serve
    /// --eval-deadline-ms` was told to allow. `limits_over` takes whichever of
    /// the request and the process binds first.
    #[test]
    fn a_request_cannot_raise_the_evaluation_limits_above_the_server_maximum() {
        // The default limits are process-wide and Bazel runs every test in
        // this crate as threads of one binary, so the write below is scoped:
        // whatever was there before goes back when this test ends, panic or
        // not. Left set -- which is what it did -- a 20 s deadline this test
        // invented reached every other test in cadpole_cli_test that evaluates a
        // document, including the 2.25M-iteration script in
        // `a_writer_is_told_its_own_revision_not_one_another_writer_produced`.
        // limits.rs's own test avoids the globals for exactly this reason and
        // says so in as many words.
        struct Restore(cadpole_engine::Limits);
        impl Drop for Restore {
            fn drop(&mut self) {
                cadpole_engine::limits::set_default_limits(self.0);
            }
        }
        let _restore = Restore(cadpole_engine::limits::default_limits());

        // What `cadpole serve` installs at startup (its --eval-deadline-ms
        // default and the 512 MB structural heap cap).
        cadpole_engine::limits::set_default_limits(cadpole_engine::Limits {
            deadline_ms: 20_000,
            ..cadpole_engine::Limits::structural()
        });
        let server_default = cadpole_engine::limits::default_limits();

        // A request that asks for far more than the server allows.
        let req = pb::EvaluateRequest {
            deadline_ms: 999_999,
            max_ticks: u64::MAX,
            max_heap_bytes: 64u64 * 1024 * 1024 * 1024,
            ..Default::default()
        };
        let got = cadpole_engine::limits::limits_from_request(&req);

        assert!(
            got.deadline_ms <= server_default.deadline_ms,
            "a caller raised the evaluation deadline to {} ms, above the server's {} ms cap",
            got.deadline_ms,
            server_default.deadline_ms
        );
        assert!(
            got.max_ticks <= server_default.max_ticks,
            "a caller raised the tick cap to {}, above the server's {}",
            got.max_ticks,
            server_default.max_ticks
        );
        assert!(
            got.max_heap_bytes <= server_default.max_heap_bytes,
            "a caller raised the heap cap to {} bytes, above the server's {} bytes",
            got.max_heap_bytes,
            server_default.max_heap_bytes
        );
    }

    /// `SetParams` is granted to the listener as "only an override
    /// map: no code, one revision". With the *size* of that map uncapped, each
    /// call would merge more entries into the live session document and they
    /// would persist, so a listener could grow the in-memory document without
    /// bound. The 16 MB document cap is only enforced when
    /// the document is written to disk (`save-as`) or read back (`GET
    /// /api/documents/{id}`), never on the session document `SetParams` grows,
    /// and every subsequent `GET /api/session`, `GET /api/session/document`
    /// and `params_changed` event would carry the whole thing.
    ///
    /// The override map is bounded by count and by name length, and refusing
    /// is the mechanism -- so the loop below ends in an error rather than in
    /// 24 MB of document.
    #[test]
    fn set_params_cannot_grow_the_session_document_without_bound() {
        use cadpole_ops::Host as _;
        let h = host(SRC);
        // ~24 MB of override keys, well over MAX_DOCUMENT_BYTES (16 MB),
        // spread over many calls the way a loop of tool calls would.
        let mut refused = false;
        for round in 0..300u32 {
            let overrides: std::collections::HashMap<String, pb::ParamValue> = (0..400u32)
                .map(|i| {
                    let key = format!("r{round}_{i}_{}", "k".repeat(200));
                    (key, pb::ParamValue { value: Some(pb::param_value::Value::Number(1.0)) })
                })
                .collect();
            if h.set_params(&pb::SetParamsRequest { overrides, clear: Vec::new() }).is_err() {
                refused = true;
                break;
            }
        }
        assert!(refused, "growing the document without bound was never refused");
        let encoded = h.app.state.lock().unwrap().document.encode_to_vec().len() as u64;
        assert!(
            encoded <= security::MAX_DOCUMENT_BYTES,
            "the session document grew to {encoded} bytes via SetParams, over the {} byte cap, with no refusal",
            security::MAX_DOCUMENT_BYTES
        );
    }

    // The Connect protocol. These are the conformance tests: a stock
    // Connect client interoperates with the route, and a protocol nobody
    // checks in CI is a protocol that rots.
    // -----------------------------------------------------------------------

    /// The same operation, in both codecs, on the path the protocol names.
    #[test]
    fn the_connect_route_answers_proto_and_json() {
        let (base, _app) = serve_for_test(None, std::env::temp_dir().join("cadpole-connect-1"));
        let a = agent();
        let url = |m: &str| format!("{base}{}", connect::method_path(m));

        // application/proto: the body is the message, nothing wrapped round it.
        let mut r = a
            .post(&url("Evaluate"))
            .content_type("application/proto")
            .header("connect-protocol-version", "1")
            .send(pb::EvaluateRequest { skip_mesh: true, ..Default::default() }.encode_to_vec())
            .unwrap();
        assert_eq!(r.status().as_u16(), 200);
        assert_eq!(r.headers().get("content-type").unwrap(), "application/proto");
        let res = pb::EvaluateResponse::decode(&r.body_mut().read_to_vec().unwrap()[..]).unwrap();
        assert!(res.model.is_some(), "the response is the response message itself");

        // application/json: the canonical proto3 JSON mapping, in and out.
        let mut r = a.post(&url("ApiDoc")).content_type("application/json").send("{}").unwrap();
        assert_eq!(r.status().as_u16(), 200);
        assert_eq!(r.headers().get("content-type").unwrap(), "application/json");
        let v: serde_json::Value = r.body_mut().read_json().unwrap();
        assert!(v["functions"].as_array().map(|f| !f.is_empty()).unwrap_or(false), "{v}");
    }

    /// An error is the envelope the protocol names, with the status Connect
    /// assigns to the code -- not a bare 400 with prose in it.
    #[test]
    fn a_connect_error_is_a_code_and_a_message() {
        let (base, _app) = serve_for_test(None, std::env::temp_dir().join("cadpole-connect-2"));
        let a = reading_agent();
        let url = |m: &str| format!("{base}{}", connect::method_path(m));

        let mut r = a
            .post(&url("EditScript"))
            .content_type("application/json")
            .send(r#"{"path": "nope.star", "base_revision": 1, "edits": []}"#)
            .unwrap();
        assert_eq!(r.status().as_u16(), 400);
        assert_eq!(r.headers().get("content-type").unwrap(), "application/json");
        let v: serde_json::Value = r.body_mut().read_json().unwrap();
        assert_eq!(v["code"], "invalid_argument", "{v}");
        assert!(v["message"].as_str().unwrap().contains("nope.star"), "{v}");

        // A method the service does not have is `unimplemented`, which is 501.
        let r = a.post(&url("NoSuchOp")).content_type("application/json").send("{}").unwrap();
        assert_eq!(r.status().as_u16(), 501);

        // A CLI spelling is not a Connect method: the schema's name or nothing.
        assert_eq!(status_of(a.post(&url("eval")).content_type("application/json").send("{}")), 501);
        assert_eq!(status_of(a.post(&url("set_script")).content_type("application/json").send("{}")), 501);

        // A content type nobody agreed on, and the one we deliberately refuse.
        let mut r = a.post(&url("ApiDoc")).content_type("application/grpc+proto").send(&[][..]).unwrap();
        assert_eq!(r.status().as_u16(), 415);
        let v: serde_json::Value = r.body_mut().read_json().unwrap();
        assert_eq!(v["code"], "unimplemented");
        assert!(v["message"].as_str().unwrap().contains("HTTP/2"), "{v}");

        // A protocol version we do not implement is refused rather than guessed at.
        let r = a
            .post(&url("ApiDoc"))
            .content_type("application/json")
            .header("connect-protocol-version", "2")
            .send("{}")
            .unwrap();
        assert_eq!(r.status().as_u16(), 400);
    }

    /// The new transport is behind the old door: same token, same capability
    /// policy, same refusals -- rendered as Connect errors so a generated
    /// client sees a code rather than a parse failure.
    #[test]
    fn the_connect_route_is_behind_the_same_guard() {
        let (base, _app) = serve_tokens_for_test(
            security::TokenSet::new("owner-tok", "listener-tok"),
            std::env::temp_dir().join("cadpole-connect-3"),
        );
        let a = reading_agent();
        let url = |m: &str| format!("{base}{}", connect::method_path(m));
        let post = |t: Option<&str>, m: &str, body: &'static str| {
            let r = a.post(&url(m)).content_type("application/json");
            let r = match t {
                Some(t) => r.header("Authorization", &format!("Bearer {t}")),
                None => r,
            };
            r.send(body)
        };

        // No token is 401 with `unauthenticated`, and the body still never
        // says which half was wrong.
        let mut r = post(None, "GetSession", "{}").unwrap();
        assert_eq!(r.status().as_u16(), 401);
        let v: serde_json::Value = r.body_mut().read_json().unwrap();
        assert_eq!(v["code"], "unauthenticated", "{v}");
        assert_eq!(v["message"], "missing or wrong bearer token");

        // A listener may do on this route exactly what it may do on the other.
        assert_eq!(status_of(post(Some("listener-tok"), "GetSession", "{}")), 200);
        let mut r = post(Some("listener-tok"), "SetScript", r#"{"path":"main.star","source":"b=1\n"}"#).unwrap();
        assert_eq!(r.status().as_u16(), 403, "a denied operation is denied on every route");
        let v: serde_json::Value = r.body_mut().read_json().unwrap();
        assert_eq!(v["code"], "permission_denied", "{v}");
        assert!(v["message"].as_str().unwrap().contains("replaces a whole file"), "the reason travels: {v}");
        assert_ne!(status_of(post(Some("owner-tok"), "SetScript", r#"{"path":"main.star","source":"b=1\n"}"#)), 403);

        // ...and a spelling this route does not answer to is refused for the
        // same reason it is on the other one: it resolves to nothing.
        assert_eq!(status_of(post(Some("listener-tok"), "set_script", "{}")), 403);
        assert_eq!(status_of(post(Some("listener-tok"), "DeleteEverything", "{}")), 403);
    }

    /// gRPC-Web is the same dispatch with a different envelope: a five-byte
    /// prefix on the message and the status in a trailing frame, which is the
    /// only way a browser can read a gRPC status at all.
    #[test]
    fn the_connect_route_also_frames_grpc_web() {
        let (base, _app) = serve_for_test(None, std::env::temp_dir().join("cadpole-connect-4"));
        let a = agent();
        let url = format!("{base}{}", connect::method_path("ApiDoc"));

        let mut framed = vec![0u8, 0, 0, 0, 0];
        let mut r = a.post(&url).content_type("application/grpc-web+proto").send(framed.clone()).unwrap();
        assert_eq!(r.status().as_u16(), 200);
        assert_eq!(r.headers().get("content-type").unwrap(), "application/grpc-web+proto");
        let body = r.body_mut().read_to_vec().unwrap();
        let message = connect::unframe(&body).unwrap().to_vec();
        let doc = pb::ApiDocResponse::decode(&message[..]).unwrap();
        assert!(!doc.functions.is_empty());
        let trailer = &body[5 + message.len()..];
        assert_eq!(trailer[0], 0x80, "the last frame is the trailers");
        assert!(String::from_utf8_lossy(&trailer[5..]).contains("grpc-status: 0"));

        // An error keeps HTTP 200 and says so in the trailers, which is what a
        // gRPC-Web client reads.
        framed = vec![0, 0, 0, 0, 3, 0xff, 0xff, 0xff];
        let mut r = a.post(&url).content_type("application/grpc-web+proto").send(framed).unwrap();
        assert_eq!(r.status().as_u16(), 200);
        let body = r.body_mut().read_to_vec().unwrap();
        assert_eq!(body[0], 0x80, "trailers only: there is no message");
        let text = String::from_utf8_lossy(&body[5..]).to_string();
        assert!(text.contains("grpc-status: 3"), "invalid argument is 3: {text}");
        assert!(text.contains("grpc-message: "), "{text}");
    }

    /// The deadline the caller asked for is the deadline it gets.
    #[test]
    fn a_connect_deadline_is_honoured() {
        let (base, _app) = serve_for_test(None, std::env::temp_dir().join("cadpole-connect-5"));
        let a = reading_agent();
        let url = format!("{base}{}", connect::method_path("ApiDoc"));
        // Zero milliseconds cannot be met by anything, which makes it a stable
        // test of the mechanism rather than a race on how fast ApiDoc is.
        let mut r = a
            .post(&url)
            .content_type("application/json")
            .header("connect-timeout-ms", "0")
            .send("{}")
            .unwrap();
        assert_eq!(r.status().as_u16(), 504);
        let v: serde_json::Value = r.body_mut().read_json().unwrap();
        assert_eq!(v["code"], "deadline_exceeded", "{v}");

        let r = a
            .post(&url)
            .content_type("application/json")
            .header("connect-timeout-ms", "not-a-number")
            .send("{}")
            .unwrap();
        assert_eq!(r.status().as_u16(), 400);
    }

    // ---------------------------------------------------------------------
    // The web-bundle fingerprint.

    fn fingerprint_dir(tag: &str) -> PathBuf {
        let dir = std::env::temp_dir().join(format!("cadpole-build-id-{tag}-{}", std::process::id()));
        let _ = std::fs::remove_dir_all(&dir);
        std::fs::create_dir_all(&dir).expect("temp dir");
        for name in WEB_BUILD_FILES {
            std::fs::write(dir.join(name), format!("contents of {name}\n")).expect("write");
        }
        dir
    }

    /// The reason the fingerprint hashes bytes instead of modification times.
    ///
    /// `//src/web:dist` is one `copy_to_directory` action: a css-only edit
    /// re-copies all five files and stamps all five mtimes, so an mtime-based
    /// id reported "everything changed" on every rebuild and the page had
    /// nothing to be selective with. Re-stamping every file here must not move
    /// the id at all.
    #[test]
    fn touching_every_file_without_changing_a_byte_leaves_the_build_id_alone() {
        let dir = fingerprint_dir("touch");
        let mut fp = BuildFingerprint::new(dir.clone());
        let before = fp.current();

        // Rewrite each file with identical bytes, which is what a re-copy is.
        for name in WEB_BUILD_FILES {
            let p = dir.join(name);
            let bytes = std::fs::read(&p).expect("read");
            std::fs::remove_file(&p).expect("remove");
            std::fs::write(&p, bytes).expect("rewrite");
        }

        assert_eq!(fp.current(), before, "a re-copy of identical bytes is not a new build");
        // And a second reader, with a cold cache, agrees: the id is a function
        // of the contents and nothing else.
        assert_eq!(BuildFingerprint::new(dir.clone()).current(), before);
        let _ = std::fs::remove_dir_all(&dir);
    }

    /// One file changing moves that file's field and no other. This is the
    /// whole basis of the page's plan: if `styles.css` moved and nothing else
    /// did, a `<link>` swap is the entire update.
    #[test]
    fn one_changed_file_moves_exactly_one_field() {
        let dir = fingerprint_dir("one");
        let mut fp = BuildFingerprint::new(dir.clone());
        let before = fp.current();

        std::fs::write(dir.join("styles.css"), "body { color: red }\n").expect("write");
        let after = fp.current();
        assert_ne!(after, before);
        assert_eq!(changed_web_files(&before, &after), vec!["styles.css".to_owned()]);

        // ...and the wasm, the field that decides whether the engine is
        // swapped, moves on its own too.
        let before = after;
        std::fs::write(dir.join("cadpole_wasm_bg.wasm"), "\0asm-ish\n").expect("write");
        let after = fp.current();
        assert_eq!(changed_web_files(&before, &after), vec!["cadpole_wasm_bg.wasm".to_owned()]);
        let _ = std::fs::remove_dir_all(&dir);
    }

    /// Same length, different bytes: the case an mtime-and-size id could miss
    /// entirely if a build system preserved timestamps.
    #[test]
    fn a_same_length_edit_still_changes_the_build_id() {
        let dir = fingerprint_dir("len");
        let mut fp = BuildFingerprint::new(dir.clone());
        let before = fp.current();
        let p = dir.join("app.js");
        let bytes = std::fs::read(&p).expect("read");
        let mut flipped = bytes.clone();
        flipped[0] ^= 0x20;
        assert_eq!(flipped.len(), bytes.len());
        std::fs::write(&p, &flipped).expect("write");
        assert_ne!(fp.current(), before);
        let _ = std::fs::remove_dir_all(&dir);
    }

    /// The shape the page parses: `name:size:hash`, five fields, `|`-joined,
    /// with a missing file spelled `name:-` so a page can tell "absent" from
    /// "unchanged".
    #[test]
    fn the_build_id_has_one_field_per_file_and_names_missing_ones() {
        let dir = fingerprint_dir("shape");
        std::fs::remove_file(dir.join("cadpole_wasm.js")).expect("remove");
        let id = BuildFingerprint::new(dir.clone()).current();
        let fields: Vec<&str> = id.split('|').collect();
        assert_eq!(fields.len(), WEB_BUILD_FILES.len());
        for (f, name) in fields.iter().zip(WEB_BUILD_FILES) {
            assert!(f.starts_with(&format!("{name}:")), "{f} should name {name}");
        }
        assert!(id.contains("cadpole_wasm.js:-"), "a missing file is named, not omitted: {id}");
        let _ = std::fs::remove_dir_all(&dir);
    }

    /// Hashing 21 MB must not be something the two-second poll notices. What
    /// that means about the code is that the cost is *linear* in the bytes,
    /// so that is what this measures: hashing one 21 MB buffer against
    /// hashing twenty-one 1 MB buffers. Both do the same 21 MB of work, so a
    /// linear hash takes about the same time either way and a quadratic one
    /// takes twenty-one times longer on the single large buffer.
    ///
    /// An absolute bound (`ms < 400`) is a statement about the machine, and
    /// fails on a loaded one. So is wall time at all: a sample the scheduler
    /// takes the CPU away from in the middle grows by however long it waited,
    /// which says nothing about the hash. Each sample is this thread's CPU
    /// time, which only counts the time it actually ran; best of two, since
    /// contention (cache, memory bandwidth) can only ever inflate a sample.
    #[test]
    fn hashing_the_whole_bundle_is_cheap_enough_to_poll() {
        const MB: usize = 1024 * 1024;
        /// This thread's CPU time, in ms.
        #[cfg(any(target_os = "linux", target_os = "macos"))]
        fn cpu_ms() -> f64 {
            #[repr(C)]
            struct Timespec {
                sec: i64,
                nsec: i64,
            }
            unsafe extern "C" {
                fn clock_gettime(clock: i32, ts: *mut Timespec) -> i32;
            }
            #[cfg(target_os = "linux")]
            const CLOCK_THREAD_CPUTIME_ID: i32 = 3;
            #[cfg(target_os = "macos")]
            const CLOCK_THREAD_CPUTIME_ID: i32 = 16;
            let mut ts = Timespec { sec: 0, nsec: 0 };
            // SAFETY: `ts` is a valid `struct timespec` (64-bit `time_t` and
            // `long` on both platforms) for the duration of the call.
            assert_eq!(unsafe { clock_gettime(CLOCK_THREAD_CPUTIME_ID, &mut ts) }, 0);
            ts.sec as f64 * 1000.0 + ts.nsec as f64 / 1e6
        }
        #[cfg(not(any(target_os = "linux", target_os = "macos")))]
        fn cpu_ms() -> f64 {
            thread_local!(static T0: std::time::Instant = std::time::Instant::now());
            T0.with(|t| t.elapsed().as_secs_f64() * 1000.0)
        }
        fn best_of_two_ms(mut run: impl FnMut()) -> f64 {
            (0..2)
                .map(|_| {
                    let t0 = cpu_ms();
                    run();
                    cpu_ms() - t0
                })
                .fold(f64::INFINITY, f64::min)
        }

        let one_big = vec![0x5au8; 21 * MB];
        let many_small: Vec<Vec<u8>> = (0..21).map(|i| vec![0x5au8.wrapping_add(i); MB]).collect();
        let big_ms = best_of_two_ms(|| {
            std::hint::black_box(content_hash(std::hint::black_box(&one_big)));
        });
        let small_ms = best_of_two_ms(|| {
            for b in &many_small {
                std::hint::black_box(content_hash(std::hint::black_box(b)));
            }
        });
        assert!(
            big_ms < small_ms * 4.0 + 2.0,
            "hashing one 21 MB buffer took {big_ms:.1} ms against {small_ms:.1} ms for the same 21 MB \
             in 1 MB pieces; a hash the build poll runs every two seconds has to stay linear in the bytes"
        );

        // ...and the part that is not about time at all: the same bytes hash
        // the same, one changed byte does not, and the length is folded in.
        let a = vec![0x5au8; 4096];
        let mut b = a.clone();
        b[1234] = 0x5b;
        assert_ne!(content_hash(&a), 0);
        assert_eq!(content_hash(&a), content_hash(&a));
        assert_ne!(content_hash(&a), content_hash(&b));
        assert_ne!(content_hash(&a), content_hash(&a[..4095]), "the length is folded in");
    }

    // -----------------------------------------------------------------------
    // The two surfaces: the browser's TCP port and the agents' Unix socket,
    // one server, one guard (spec/transport.qnt).
    // -----------------------------------------------------------------------

    /// A private directory of this test's own, as the runtime directory is.
    #[cfg(unix)]
    fn private_dir(name: &str) -> PathBuf {
        let d = std::env::temp_dir().join(format!("cadpole-surf-{name}-{}", std::process::id()));
        let _ = std::fs::remove_dir_all(&d);
        crate::runtime::prepare_private_dir(&d).unwrap();
        d
    }

    /// The real browser router on a loopback port and the real agent router
    /// on a socket, both over one `App`, as `serve` builds them.
    #[cfg(unix)]
    fn serve_both_for_test(tokens: security::TokenSet, dir: &Path) -> (String, PathBuf, Shared) {
        let (tx, _) = broadcast::channel(64);
        let app: Shared = Arc::new(App {
            state: Mutex::new(state(SRC)),
            events: tx,
            live: broadcast::channel(256).0,
            tokens,
            allowed_origins: Vec::new(),
            root: DocumentRoot::new(Some(dir.join("docs"))),
            render_slots: tokio::sync::Semaphore::new(1),
            render_waiting: AtomicUsize::new(0),
            render_budget: Duration::from_secs(60),
            listener_slots: Arc::new(tokio::sync::Semaphore::new(LISTENER_OP_SLOTS)),
            max_body: 64 * 1024,
            memo: Arc::new(cadpole_engine::cadpole_core::memo::Memo::new()),
        });
        let rt = tokio::runtime::Builder::new_multi_thread().worker_threads(2).enable_all().build().unwrap();
        let sock = dir.join("aaaaaaaaaaaa.sock");
        let (tcp, unix) = rt.block_on(async { (tokio::net::TcpListener::bind("127.0.0.1:0").await.unwrap(), bind_agent_socket(&sock).unwrap()) });
        let addr = tcp.local_addr().unwrap();
        let (web, agents) = (build_router(app.clone(), Some(PathBuf::from("/nonexistent"))), build_agent_router(app.clone()));
        rt.spawn(async move {
            let _ = axum::serve(tcp, web).await;
        });
        rt.spawn(async move {
            let _ = axum::serve(unix, agents).await;
        });
        std::mem::forget(rt);
        (format!("http://{addr}"), sock, app)
    }

    /// One request on one surface: status and body.
    #[cfg(unix)]
    fn on(surface: &(String, PathBuf), tcp: bool, method: &str, path: &str, token: Option<&str>, body: Option<&[u8]>) -> (u16, String) {
        let mut headers: Vec<(&str, String)> = vec![("Content-Type", "application/json".to_owned())];
        if let Some(t) = token {
            headers.push(("Authorization", format!("Bearer {t}")));
        }
        if tcp {
            let a = reading_agent();
            let url = format!("{}{path}", surface.0);
            let r = if method == "GET" {
                let mut r = a.get(&url);
                for (k, v) in &headers {
                    r = r.header(*k, v.as_str());
                }
                r.call()
            } else {
                let mut r = a.post(&url);
                for (k, v) in &headers {
                    r = r.header(*k, v.as_str());
                }
                r.send(body.unwrap_or(b"{}"))
            };
            let mut r = r.unwrap();
            (r.status().as_u16(), r.body_mut().read_to_string().unwrap_or_default())
        } else {
            let hs: Vec<(&str, &str)> = headers.iter().map(|(k, v)| (*k, v.as_str())).collect();
            let body = if method == "GET" { None } else { Some(body.unwrap_or(b"{}")) };
            let (status, bytes) = crate::runtime::unix_request(&surface.1, method, path, &hs, body, Duration::from_secs(30)).unwrap();
            (status, String::from_utf8_lossy(&bytes).into_owned())
        }
    }

    /// The same request gets the same answer on both surfaces, for a token
    /// both admit -- the agent socket is the same server, not a different
    /// one.
    #[cfg(unix)]
    #[test]
    fn the_same_request_is_answered_the_same_on_both_surfaces() {
        let dir = private_dir("same");
        let (base, sock, _app) = serve_both_for_test(security::TokenSet::for_run("browser-tok", "agent-tok", "listener-tok", true), &dir);
        let s = (base, sock);
        for (method, path, body) in [
            ("GET", "/api/ops", None),
            ("POST", "/api/ops/GetSelection", Some(&b"{}"[..])),
            ("POST", "/api/ops/GetSession", Some(&b"{}"[..])),
            ("POST", "/api/ops/NoSuchOperation", Some(&b"{}"[..])),
        ] {
            for token in ["browser-tok", "agent-tok"] {
                let tcp = on(&s, true, method, path, Some(token), body);
                let unix = on(&s, false, method, path, Some(token), body);
                assert_eq!(tcp, unix, "{method} {path} with {token}");
            }
        }
        let tcp = on(&s, true, "GET", "/api/session", Some("agent-tok"), None);
        let unix = on(&s, false, "GET", "/api/session", Some("agent-tok"), None);
        assert_eq!(tcp.0, 200);
        assert_eq!(tcp, unix, "the session view, byte for byte");
        let _ = std::fs::remove_dir_all(&dir);
    }

    /// What each token is admitted for, on each surface, by default
    /// (`--agent-tcp` off): the agent tokens only on the socket, the browser's
    /// on both, no token and a wrong one on neither. And where a token is
    /// admitted, a denied operation is denied the same way.
    #[cfg(unix)]
    #[test]
    fn agent_tokens_stay_off_the_tcp_port_and_a_denial_is_the_same_everywhere() {
        let dir = private_dir("admit");
        let (base, sock, app) = serve_both_for_test(security::TokenSet::for_run("browser-tok", "agent-tok", "listener-tok", false), &dir);
        let s = (base, sock);
        let set_script = br#"{"path": "main.star", "source": "b = box(1, 1, 1)\n"}"#;
        // An agent call over TCP is refused by default, before any handler.
        for t in ["agent-tok", "listener-tok"] {
            let (status, body) = on(&s, true, "POST", "/api/ops/GetSelection", Some(t), Some(b"{}"));
            assert_eq!(status, 403, "{t} over TCP: {body}");
            assert!(body.contains("--agent-tcp") && !body.contains(t), "{body}");
        }
        // ...and admitted on the socket.
        assert_eq!(on(&s, false, "POST", "/api/ops/GetSelection", Some("agent-tok"), Some(b"{}")).0, 200);
        assert_eq!(on(&s, false, "POST", "/api/ops/GetSelection", Some("listener-tok"), Some(b"{}")).0, 200);
        // The browser's token works on both.
        assert_eq!(on(&s, true, "POST", "/api/ops/GetSelection", Some("browser-tok"), Some(b"{}")).0, 200);
        assert_eq!(on(&s, false, "POST", "/api/ops/GetSelection", Some("browser-tok"), Some(b"{}")).0, 200);
        // No token, or a wrong one, is refused on both.
        for t in [None, Some("wrong")] {
            assert_eq!(on(&s, true, "POST", "/api/ops/GetSelection", t, Some(b"{}")).0, 401);
            assert_eq!(on(&s, false, "POST", "/api/ops/GetSelection", t, Some(b"{}")).0, 401);
        }
        // A denied operation is denied to the listener wherever it is
        // admitted -- here the socket -- with the same reason a TCP refusal
        // gives when `--agent-tcp` admits it there (the next test).
        let (status, body) = on(&s, false, "POST", "/api/ops/SetScript", Some("listener-tok"), Some(set_script));
        assert_eq!(status, 403, "{body}");
        assert!(body.contains("may not call SetScript"), "{body}");
        let revision = app.state.lock().unwrap().revision;
        assert_eq!(revision, 1, "nothing ran");

        let dir2 = private_dir("admit-tcp");
        let (base2, sock2, _app2) = serve_both_for_test(security::TokenSet::for_run("browser-tok", "agent-tok", "listener-tok", true), &dir2);
        let s2 = (base2, sock2);
        let tcp = on(&s2, true, "POST", "/api/ops/SetScript", Some("listener-tok"), Some(set_script));
        let unix = on(&s2, false, "POST", "/api/ops/SetScript", Some("listener-tok"), Some(set_script));
        assert_eq!(tcp, unix, "the listener's denial does not depend on the surface");
        assert_eq!(tcp, (status, body), "nor on whether --agent-tcp is on");
        assert_eq!(on(&s2, true, "POST", "/api/ops/GetSelection", Some("agent-tok"), Some(b"{}")).0, 200, "--agent-tcp admits it");
        let _ = std::fs::remove_dir_all(&dir);
        let _ = std::fs::remove_dir_all(&dir2);
    }

    /// The socket serves the agent routes and nothing else: no web UI, no
    /// static file, no browser-only route, whatever the token.
    #[cfg(unix)]
    #[test]
    fn the_web_ui_and_browser_routes_are_not_served_on_the_socket() {
        let dir = private_dir("routes");
        let (base, sock, _app) = serve_both_for_test(security::TokenSet::for_run("browser-tok", "agent-tok", "listener-tok", true), &dir);
        let s = (base, sock);
        for path in ["/", "/index.html", "/app.js", "/api/events", "/api/events/poll", "/api/documents", "/api/model", "/api/session/script"] {
            let (status, body) = on(&s, false, "GET", path, Some("browser-tok"), None);
            assert_eq!(status, 404, "{path}: {body}");
        }
        for path in ["/api/render", "/api/call", "/api/session/messages", "/api/session/save"] {
            assert_eq!(on(&s, false, "POST", path, Some("browser-tok"), Some(b"{}")).0, 404, "{path}");
        }
        // The same paths exist on TCP (here the static UI is missing, so the
        // fallback says so rather than 404ing a route).
        assert_eq!(on(&s, true, "GET", "/api/documents", Some("browser-tok"), None).0, 200);
        let _ = std::fs::remove_dir_all(&dir);
    }

    /// The socket is created 0600 in a private directory, never in a loose
    /// one; a live socket is never replaced and a stale one is.
    #[cfg(unix)]
    #[test]
    fn the_socket_is_private_and_a_stale_one_is_replaced_but_a_live_one_is_not() {
        use std::os::unix::fs::PermissionsExt as _;
        let rt = tokio::runtime::Builder::new_current_thread().enable_all().build().unwrap();
        let dir = private_dir("sock");
        let path = dir.join("bbbbbbbbbbbb.sock");
        let live = rt.block_on(async { bind_agent_socket(&path) }).unwrap();
        assert_eq!(std::fs::metadata(&path).unwrap().permissions().mode() & 0o777, 0o600);
        let e = rt.block_on(async { bind_agent_socket(&path) }).unwrap_err().to_string();
        assert!(e.contains("another `cadpole serve` is listening"), "{e}");
        assert!(path.exists(), "a live server's socket is never removed");
        drop(live);
        assert!(path.exists(), "a dropped listener leaves its file, as a crash would");
        crate::runtime::leave_dead_socket(&path);
        let again = rt.block_on(async { bind_agent_socket(&path) });
        assert!(again.is_ok(), "a stale socket of ours is replaced: {:?}", again.err());
        drop(again);
        // Something that is not a socket is in the way: refused, not deleted.
        std::fs::remove_file(&path).unwrap();
        std::fs::write(&path, b"not a socket").unwrap();
        std::fs::set_permissions(&path, std::fs::Permissions::from_mode(0o600)).unwrap();
        assert!(rt.block_on(async { bind_agent_socket(&path) }).is_err());
        assert_eq!(std::fs::read(&path).unwrap(), b"not a socket");
        // A loose directory is refused outright.
        let loose = dir.join("loose");
        std::fs::create_dir(&loose).unwrap();
        std::fs::set_permissions(&loose, std::fs::Permissions::from_mode(0o755)).unwrap();
        let e = rt.block_on(async { bind_agent_socket(&loose.join("cccccccccccc.sock")) }).unwrap_err().to_string();
        assert!(e.contains("0700"), "{e}");
        let _ = std::fs::remove_dir_all(&dir);
    }
}
