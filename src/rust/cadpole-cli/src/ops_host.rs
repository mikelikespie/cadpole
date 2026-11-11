//! `cadpole_ops::Host` over HTTP: how `cadpole` and `cadpole mcp` reach a session.
//!
//! Session operations are defined once in `ops.proto` and implemented once in
//! `cadpole-ops`; the only difference between running inside `cadpole serve` and
//! running beside it is this file, which turns each one into a call to the
//! REST-ish session endpoints documented in `docs/architecture.md`.
//!
//! Those endpoints speak the web UI's hand-written JSON, not proto3 JSON, so
//! the conversions here are explicit. `cadpole.v1.Session` is *defined* to mirror
//! that JSON (see ops.proto), which keeps the mapping mechanical.

use std::path::{Path, PathBuf};
use std::time::Duration;

use anyhow::Context as _;
use cadpole_engine::cadpole_proto::prost;
use cadpole_engine::cadpole_proto::prost::Message as _;
use cadpole_engine::cadpole_proto::v1 as pb;
use cadpole_ops::Host;
use serde_json::Value;

/// A client of one running `cadpole serve`, over its agent socket or over
/// HTTP.
pub struct HttpHost {
    endpoint: std::sync::Arc<crate::runtime::Endpoint>,
    agent: ureq::Agent,
    /// This client's stable id. It is sent as `X-Cadpole-Client` on every write,
    /// so the server can attribute the event and never echo it back to us,
    /// and it is the key the `Ack` cursor is stored under.
    client: String,
    /// Set once when the server turns out not to serve the Connect route, so
    /// a new `cadpole op` keeps working against a `cadpole serve` that predates it.
    legacy_route: std::sync::atomic::AtomicBool,
}

/// Read a bearer token: `--token`, else a token file (`~` expanded), else the
/// one `cadpole serve` wrote for *this run* in the runtime directory.
///
/// The last fallback is what makes the documented `.mcp.json` work without an
/// `env` block at all, and what makes a restart invisible to `cadpole op`: the
/// server rewrites the file every time it starts, so a client that re-reads it
/// picks up the new token with nothing pasted by hand (docs/security.md).
/// The old config-directory location is read last as a fallback.
pub fn token(explicit: Option<String>, file: Option<&Path>) -> Option<String> {
    if let Some(t) = explicit.filter(|t| !t.is_empty()) {
        return Some(t);
    }
    if let Some(p) = file {
        let p = match p.strip_prefix("~") {
            Ok(rest) => std::env::var("HOME").map(|h| PathBuf::from(h).join(rest)).unwrap_or_else(|_| p.to_owned()),
            Err(_) => p.to_owned(),
        };
        if let Some(t) = read_token_file(&p) {
            return Some(t);
        }
    }
    crate::security::token_path()
        .and_then(|p| read_token_file(&p))
        .or_else(|| crate::security::legacy_token_path().and_then(|p| read_token_file(&p)))
}

fn read_token_file(path: &Path) -> Option<String> {
    std::fs::read_to_string(path).ok().map(|s| s.trim().to_owned()).filter(|t| !t.is_empty())
}

impl HttpHost {
    /// A client of the server at `base` (`http://...` or `unix:/path`).
    #[cfg(test)]
    pub fn new(base: &str, token: Option<String>) -> HttpHost {
        HttpHost::with_client(base, token, format!("cli:{}", std::process::id()))
    }

    #[cfg(test)]
    pub fn with_client(base: &str, token: Option<String>, client: String) -> HttpHost {
        let target = crate::runtime::parse_server(base).unwrap_or_else(|_| crate::runtime::Target::Http(base.trim_end_matches('/').to_owned()));
        let tokens = match token {
            Some(t) => crate::runtime::TokenSource::Explicit(t),
            None => crate::runtime::TokenSource::Owner { file: None },
        };
        HttpHost::on(std::sync::Arc::new(crate::runtime::Endpoint::fixed(target, tokens)), client)
    }

    /// A client of whatever `endpoint` resolves to; several clients may share
    /// one, and with it the document a discovering client has settled on.
    pub fn on(endpoint: std::sync::Arc<crate::runtime::Endpoint>, client: String) -> HttpHost {
        HttpHost { endpoint, agent: ureq::Agent::new_with_defaults(), client, legacy_route: std::sync::atomic::AtomicBool::new(false) }
    }

    /// The `--server` value that reaches this server from another command,
    /// or `None` when that command would find it with no flag.
    pub fn server_flag(&self) -> Option<String> {
        self.endpoint.server_flag()
    }

    /// How many different servers this client has been led to
    /// ([`crate::runtime::Endpoint::generation`]).
    pub fn generation(&self) -> u64 {
        self.endpoint.generation()
    }

    /// The id this client sends as `X-Cadpole-Client`.
    pub fn client_id(&self) -> &str {
        &self.client
    }

    /// One request, returning the status and the body whatever the status.
    ///
    /// A client given `--server` keeps the server and the token it started
    /// with; only one that found its server itself looks again, below.
    ///
    /// `follow`: whether the request may go to a new server when the one
    /// this client was using has gone. When the connect fails (the request
    /// never left) or the server turns the token down with `401` (before any
    /// handler ran), the endpoint is resolved again -- the restarted server
    /// of the same document, with that run's token, or a clear error -- and,
    /// with `follow`, the request is sent once more there. Nothing runs
    /// twice: a request that broke after it was sent is reported, never
    /// repeated (spec/transport.qnt, `aCallRunsAtMostOnce`).
    ///
    /// Without `follow` the failure is returned, and only the next request
    /// goes to the new server. That is for the reads that carry a cursor
    /// (`WaitEvents`, `Ack`): a restarted server's log starts over, so the
    /// reader has to learn of the restart and start again itself
    /// (spec/wakeups.qnt), not be handed the new log under the old cursor.
    fn request(&self, method: &str, path: &str, headers: &[(&str, &str)], body: Option<&[u8]>, timeout: Duration, follow: bool) -> anyhow::Result<(u16, Vec<u8>)> {
        let mut retried = false;
        loop {
            let r = self.endpoint.resolve()?;
            let mut hs: Vec<(&str, String)> = headers.iter().map(|(k, v)| (*k, (*v).to_owned())).collect();
            hs.push(("X-Cadpole-Client", self.client.clone()));
            if let Some(t) = &r.token {
                hs.push(("Authorization", format!("Bearer {t}")));
            }
            let outcome = match &r.target {
                crate::runtime::Target::Unix(sock) => {
                    let hs: Vec<(&str, &str)> = hs.iter().map(|(k, v)| (*k, v.as_str())).collect();
                    match crate::runtime::unix_request(sock, method, path, &hs, body, timeout) {
                        Ok(x) => Ok(x),
                        Err(crate::runtime::SendError::Connect(e)) => {
                            if self.endpoint.discovers() {
                                self.endpoint.invalidate();
                                if follow && !retried {
                                    retried = true;
                                    continue;
                                }
                            }
                            Err(anyhow::anyhow!("{method} {path} via {}: cannot connect ({e}); the next request looks for the server again", r.target))
                        }
                        Err(e) => Err(anyhow::anyhow!("{method} {path} via {}: {e}", r.target)),
                    }
                }
                crate::runtime::Target::Http(base) => self.http_request(base, method, path, &hs, body, timeout),
            };
            let (status, bytes) = outcome?;
            if status == 401 && self.endpoint.discovers() {
                self.endpoint.invalidate();
                if follow && !retried {
                    retried = true;
                    continue;
                }
            }
            return Ok((status, bytes));
        }
    }

    fn http_request(&self, base: &str, method: &str, path: &str, headers: &[(&str, String)], body: Option<&[u8]>, timeout: Duration) -> anyhow::Result<(u16, Vec<u8>)> {
        let url = format!("{base}{path}");
        let mut resp = if method == "GET" {
            let mut r = self.agent.get(&url).config().timeout_global(Some(timeout)).http_status_as_error(false).build();
            for (k, v) in headers {
                r = r.header(*k, v.as_str());
            }
            r.call().with_context(|| format!("GET {url}"))?
        } else {
            // 4xx come back as a response rather than an error, so the
            // *body* reaches the caller: a `403` from the capability check
            // carries the reason the operation is denied.
            let mut r = self.agent.post(&url).config().timeout_global(Some(timeout)).http_status_as_error(false).build();
            for (k, v) in headers {
                r = r.header(*k, v.as_str());
            }
            r.send(body.unwrap_or(&[])).with_context(|| format!("POST {url}"))?
        };
        let status = resp.status().as_u16();
        let mut bytes = Vec::new();
        std::io::Read::read_to_end(&mut resp.body_mut().as_reader(), &mut bytes)?;
        Ok((status, bytes))
    }

    pub fn get(&self, path: &str, timeout: Duration) -> anyhow::Result<Value> {
        let bytes = self.get_bytes(path, timeout)?;
        serde_json::from_slice(&bytes).with_context(|| format!("GET {path}: not JSON"))
    }

    /// Raw bytes from a GET, for endpoints that do not return JSON (an
    /// attachment's PNG, a document).
    pub fn get_bytes(&self, path: &str, timeout: Duration) -> anyhow::Result<Vec<u8>> {
        let (status, bytes) = self.request("GET", path, &[], None, timeout, true)?;
        if status >= 300 {
            let why = String::from_utf8_lossy(&bytes);
            anyhow::bail!("GET {path} -> {status}{}", if why.trim().is_empty() { String::new() } else { format!(": {}", why.trim()) });
        }
        Ok(bytes)
    }

    fn session_json(&self) -> anyhow::Result<Value> {
        self.get("/api/session", Duration::from_secs(10))
    }

    /// POST one operation as a unary Connect rpc in the protobuf wire format.
    ///
    /// **Every** session operation goes through here, never a hand mapping
    /// onto the `/api/session/*` JSON routes: a second mapping is a place for
    /// client and server to disagree, and — since a restricted token's whole
    /// surface is the operation registry — a way for a listener to reach an
    /// operation the policy denies by calling an alias route instead.
    pub fn call_op<Req: prost::Message, Resp: prost::Message + Default>(&self, name: &str, req: &Req) -> anyhow::Result<Resp> {
        self.call_op_within(name, req, Duration::from_secs(300))
    }

    /// [`Self::call_op`] with an explicit deadline, for the long poll.
    ///
    /// The call goes out as unary Connect and falls back, once per process, to
    /// the older `POST /api/ops/<Rpc>` when the server has never heard of the
    /// Connect path: an older `cadpole serve` may still be running. The
    /// fallback is remembered, so the second call does not pay for it.
    pub fn call_op_within<Req: prost::Message, Resp: prost::Message + Default>(
        &self,
        name: &str,
        req: &Req,
        timeout: Duration,
    ) -> anyhow::Result<Resp> {
        let body = req.encode_to_vec();
        if !self.legacy_route.load(std::sync::atomic::Ordering::Relaxed) {
            let (status, bytes) = self.post_op(&crate::connect::method_path(name), &body, timeout, true, follows(name))?;
            match status {
                200 => return Resp::decode(bytes.as_slice()).with_context(|| format!("decoding the {name} response")),
                // 404: no such route at all. 501 with no envelope: something
                // else is answering. Either way this server predates Connect.
                404 | 501 if !looks_like_connect_error(&bytes) => {
                    self.legacy_route.store(true, std::sync::atomic::Ordering::Relaxed);
                }
                _ => anyhow::bail!("{name} -> {}", connect_failure(status, &bytes)),
            }
        }
        let (status, bytes) = self.post_op(&format!("/api/ops/{name}"), &body, timeout, false, follows(name))?;
        if status >= 300 {
            anyhow::bail!("{name} -> HTTP {status}: {}", String::from_utf8_lossy(&bytes));
        }
        Resp::decode(bytes.as_slice()).with_context(|| format!("decoding the {name} response"))
    }

    /// One POST of an encoded request, returning the status and the body.
    fn post_op(&self, path: &str, body: &[u8], timeout: Duration, connect: bool, follow: bool) -> anyhow::Result<(u16, Vec<u8>)> {
        let headers: &[(&str, &str)] = if connect {
            &[
                ("Content-Type", "application/proto"),
                ("Accept", "application/proto"),
                (crate::connect::VERSION_HEADER, crate::connect::PROTOCOL_VERSION),
            ]
        } else {
            &[("Content-Type", "application/x-protobuf")]
        };
        self.request("POST", path, headers, Some(body), timeout, follow)
    }
}

/// May a call of this operation follow a restart to the new server
/// ([`HttpHost::request`])? Not the two that carry an event-log cursor.
fn follows(op: &str) -> bool {
    !matches!(op, "WaitEvents" | "Ack")
}

/// Is this body a Connect error envelope? Distinguishes "this server does not
/// have the route" from "this server refused the call with `unimplemented`".
fn looks_like_connect_error(body: &[u8]) -> bool {
    serde_json::from_slice::<Value>(body).ok().and_then(|v| v.get("code").and_then(|c| c.as_str()).map(str::to_owned)).is_some()
}

/// A Connect failure as one line: the code and the message the server sent.
///
/// The message is the part that matters — a capability refusal carries the
/// reason the operation is denied, and that sentence is what an assistant
/// repeats back to the user.
fn connect_failure(status: u16, body: &[u8]) -> String {
    match serde_json::from_slice::<Value>(body) {
        Ok(v) if v.get("code").is_some() => {
            let code = v["code"].as_str().unwrap_or("unknown");
            match v["message"].as_str().filter(|m| !m.is_empty()) {
                Some(m) => format!("{code}: {m}"),
                None => code.to_owned(),
            }
        }
        _ => format!("HTTP {status}: {}", String::from_utf8_lossy(body)),
    }
}

fn param_value(v: &Value) -> Option<pb::ParamValue> {
    cadpole_engine::view::param_value_from_json(v).map(|p| p.to_proto())
}

fn session_ref(v: &Value) -> pb::SessionRef {
    pb::SessionRef {
        feature_id: v["feature_id"].as_str().unwrap_or("").to_owned(),
        body_id: v["body_id"].as_str().unwrap_or("").to_owned(),
        kind: v["kind"].as_str().unwrap_or("").to_owned(),
        topo_name: v["topo_name"].as_str().unwrap_or("").to_owned(),
        fingerprint: serde_json::from_value::<cadpole_engine::view::FingerprintView>(v["fingerprint"].clone())
            .ok()
            .map(|f| f.to_proto()),
    }
}

fn entity_ref(v: &Value) -> pb::EntityRef {
    pb::EntityRef {
        feature_id: v["feature_id"].as_str().unwrap_or("").to_owned(),
        body_id: v["body_id"].as_str().unwrap_or("").to_owned(),
        kind: cadpole_engine::kind_from_name(v["kind"].as_str().unwrap_or("")) as i32,
        topo_name: v["topo_name"].as_str().unwrap_or("").to_owned(),
        // The geometric fallback, when the front-end recorded
        // one at pick time.
        fingerprint: serde_json::from_value::<cadpole_engine::view::FingerprintView>(v["fingerprint"].clone())
            .ok()
            .map(|f| f.to_proto()),
    }
}

/// `GET /api/session`'s JSON as a `cadpole.v1.Session`.
pub fn session_from_json(s: &Value) -> pb::Session {
    let doc = &s["document"];
    pb::Session {
        id: s["id"].as_str().unwrap_or("").to_owned(),
        revision: s["revision"].as_u64().unwrap_or(0),
        web_build: s["web_build"].as_str().unwrap_or("").to_owned(),
        document: Some(pb::SessionDocument {
            id: doc["id"].as_str().unwrap_or("").to_owned(),
            name: doc["name"].as_str().unwrap_or("").to_owned(),
            display_unit: doc["display_unit"].as_str().unwrap_or("").to_owned(),
            scripts: doc["scripts"]
                .as_array()
                .map(|a| {
                    a.iter()
                        .map(|s| pb::ScriptFile {
                            path: s["path"].as_str().unwrap_or("").to_owned(),
                            source: s["source"].as_str().unwrap_or("").to_owned(),
                        })
                        .collect()
                })
                .unwrap_or_default(),
            param_overrides: doc["param_overrides"]
                .as_object()
                .map(|o| o.iter().filter_map(|(k, v)| param_value(v).map(|p| (k.clone(), p))).collect())
                .unwrap_or_default(),
            asset_names: doc["asset_names"]
                .as_array()
                .map(|a| a.iter().filter_map(|n| n.as_str().map(str::to_owned)).collect())
                .unwrap_or_default(),
        }),
        selection: Some(pb::SessionSelection {
            selected: s["selection"]["selected"].as_array().map(|a| a.iter().map(session_ref).collect()).unwrap_or_default(),
            hovered: s["selection"]["hovered"].as_object().map(|_| session_ref(&s["selection"]["hovered"])),
            note: s["selection"]["note"].as_str().unwrap_or("").to_owned(),
        }),
        messages: s["messages"]
            .as_array()
            .map(|a| {
                a.iter()
                    .map(|m| pb::SessionMessage {
                        seq: m["seq"].as_u64().unwrap_or(0),
                        from: m["from"].as_str().unwrap_or("").to_owned(),
                        text: m["text"].as_str().unwrap_or("").to_owned(),
                        at: m["at"].as_str().unwrap_or("").to_owned(),
                        attachments: m["attachments"].as_array().map(|a| a.len() as u32).unwrap_or(0),
                        state: m["state"].as_str().unwrap_or("").to_owned(),
                        activity: m["activity"].as_str().unwrap_or("").to_owned(),
                    })
                    .collect()
            })
            .unwrap_or_default(),
        clients: s["clients"].as_u64().unwrap_or(0) as u32,
        last_event_seq: s["last_event_seq"].as_u64().unwrap_or(0),
        assistant_listening: s["assistant_listening"].as_bool().unwrap_or(false),
        client_failures: s["client_failures"].as_u64().unwrap_or(0),
    }
}

/// The session's document as a `cadpole.v1.Document`, ready to evaluate: scripts,
/// parameter overrides and the current selection (so `highlight_selection`
/// works from the CLI too).
pub fn document_from_session_json(s: &Value) -> pb::Document {
    let doc = &s["document"];
    let mut d = cadpole_engine::document_from_scripts(
        doc["name"].as_str().unwrap_or("document"),
        doc["scripts"]
            .as_array()
            .map(|a| {
                a.iter()
                    .map(|s| (s["path"].as_str().unwrap_or("main.star").to_owned(), s["source"].as_str().unwrap_or("").to_owned()))
                    .collect()
            })
            .unwrap_or_default(),
    );
    d.id = doc["id"].as_str().unwrap_or("").to_owned();
    if let Some(ov) = doc["param_overrides"].as_object() {
        for (k, v) in ov {
            if let Some(p) = param_value(v) {
                d.param_overrides.insert(k.clone(), p);
            }
        }
    }
    d.selection = Some(pb::Selection {
        selected: s["selection"]["selected"].as_array().map(|a| a.iter().map(entity_ref).collect()).unwrap_or_default(),
        hovered: None,
        note: s["selection"]["note"].as_str().unwrap_or("").to_owned(),
    });
    d
}

impl Host for HttpHost {
    fn document(&self) -> anyhow::Result<pb::Document> {
        // The protobuf endpoint carries everything -- assets included -- which
        // the JSON session view does not; fall back to reconstructing from
        // that view when talking to an older server.
        match self.get_bytes("/api/session/document", Duration::from_secs(30)) {
            Ok(bytes) => Ok(pb::Document::decode(bytes.as_slice())?),
            Err(_) => Ok(document_from_session_json(&self.session_json()?)),
        }
    }

    fn edit_script(&self, req: &pb::EditScriptRequest) -> anyhow::Result<pb::EditScriptResponse> {
        self.call_op("EditScript", req)
    }

    fn get_attachment(&self, req: &pb::GetAttachmentRequest) -> anyhow::Result<pb::GetAttachmentResponse> {
        self.call_op("GetAttachment", req)
    }

    fn ack(&self, req: &pb::AckRequest) -> anyhow::Result<pb::AckResponse> {
        let mut req = req.clone();
        if req.client.is_empty() {
            req.client = self.client.clone();
        }
        self.call_op("Ack", &req)
    }

    fn add_asset(&self, req: &pb::AddAssetRequest) -> anyhow::Result<pb::AddAssetResponse> {
        self.call_op("AddAsset", req)
    }

    fn get_session(&self, req: &pb::GetSessionRequest) -> anyhow::Result<pb::Session> {
        self.call_op("GetSession", req)
    }

    fn set_script(&self, req: &pb::SetScriptRequest) -> anyhow::Result<pb::SetScriptResponse> {
        self.call_op("SetScript", req)
    }

    fn set_params(&self, req: &pb::SetParamsRequest) -> anyhow::Result<pb::SetParamsResponse> {
        self.call_op("SetParams", req)
    }

    fn set_selection(&self, req: &pb::SetSelectionRequest) -> anyhow::Result<pb::SetSelectionResponse> {
        self.call_op("SetSelection", req)
    }

    fn reply(&self, req: &pb::ReplyRequest) -> anyhow::Result<pb::ReplyResponse> {
        self.call_op("Reply", req)
    }

    fn stream_reply(&self, req: &pb::StreamReplyRequest) -> anyhow::Result<pb::StreamReplyResponse> {
        self.call_op("StreamReply", req)
    }

    fn report_client_failure(&self, req: &pb::ReportClientFailureRequest) -> anyhow::Result<pb::ReportClientFailureResponse> {
        self.call_op("ReportClientFailure", req)
    }

    fn post_message(&self, req: &pb::PostMessageRequest) -> anyhow::Result<pb::PostMessageResponse> {
        self.call_op("PostMessage", req)
    }

    fn save(&self, req: &pb::SaveRequest) -> anyhow::Result<pb::SaveResponse> {
        self.call_op("Save", req)
    }

    fn wait_events(&self, req: &pb::WaitEventsRequest) -> anyhow::Result<pb::WaitEventsResponse> {
        let mut req = req.clone();
        if req.client.is_empty() {
            req.client = self.client.clone();
        }
        self.call_op_within("WaitEvents", &req, wait_events_budget(req.timeout_ms))
    }

    // Solve jobs run in the session's server, so an assistant's jobs
    // are the server's: re-evaluated and pushed to the viewer when they land.
    // A missing `document` stays missing, and the server uses its own.

    fn solve(&self, req: &pb::SolveRequest) -> anyhow::Result<pb::SolveResponse> {
        // The server caps the wait at 120 s; each evaluation it runs has its
        // own deadline on top.
        let budget = Duration::from_millis(u64::from(req.wait_ms).min(120_000) + 120_000);
        self.call_op_within("Solve", req, budget)
    }

    fn list_jobs(&self, req: &pb::ListJobsRequest) -> anyhow::Result<pb::ListJobsResponse> {
        self.call_op("ListJobs", req)
    }

    fn wait_jobs(&self, req: &pb::WaitJobsRequest) -> anyhow::Result<pb::WaitJobsResponse> {
        let asked = if req.timeout_ms == 0 { 30_000 } else { u64::from(req.timeout_ms).min(120_000) };
        self.call_op_within("WaitJobs", req, Duration::from_millis(asked + 10_000))
    }

    fn cancel_jobs(&self, req: &pb::CancelJobsRequest) -> anyhow::Result<pb::CancelJobsResponse> {
        self.call_op("CancelJobs", req)
    }
}

/// How long a `WaitEvents` call may take over HTTP: the server's own wait
/// (0 is its 30 s default, and it caps at 120 s) plus a margin for the round
/// trip, as [`HttpHost::wait_jobs`] does for `WaitJobs`.
fn wait_events_budget(timeout_ms: u32) -> Duration {
    let asked = if timeout_ms == 0 { 30_000 } else { u64::from(timeout_ms).min(120_000) };
    Duration::from_millis(asked + 10_000)
}

#[cfg(test)]
mod tests {
    use super::*;
    use serde_json::json;

    /// The server reads `timeout_ms: 0` as its 30 s default (ops.proto), so
    /// the HTTP budget has to outlast that too; a budget computed from the
    /// raw 0 gave up after 10 s with "timeout: global" while the server was
    /// still waiting.
    #[test]
    fn the_wait_events_budget_outlasts_the_servers_wait() {
        let server_wait = |t: u32| Duration::from_millis(u64::from(if t == 0 { 30_000 } else { t }).min(120_000));
        for t in [0, 1, 500, 30_000, 120_000, u32::MAX] {
            assert!(wait_events_budget(t) > server_wait(t), "timeout_ms {t}: {:?}", wait_events_budget(t));
        }
    }

    #[test]
    fn session_json_maps_onto_the_proto_field_for_field() {
        let s = json!({
            "id": "abc", "revision": 7, "clients": 2, "last_event_seq": 42, "assistant_listening": true,
            "document": {"id": "d1", "name": "planter", "display_unit": "mm",
                          "scripts": [{"path": "main.star", "source": "b = box(1,1,1)"}],
                          "param_overrides": {"wall": 2.4, "solid": true}},
            "selection": {"selected": [{"feature_id": "lid", "kind": "face", "topo_name": "face:3", "body_id": ""}], "note": "thicker"},
            "messages": [{"seq": 1, "from": "user", "text": "hi", "at": "2026-01-01T00:00:00Z"}]
        });
        let p = session_from_json(&s);
        assert_eq!((p.id.as_str(), p.revision, p.clients, p.last_event_seq), ("abc", 7, 2, 42));
        assert!(p.assistant_listening);
        let d = p.document.unwrap();
        assert_eq!(d.scripts[0].path, "main.star");
        assert_eq!(d.param_overrides["wall"].value, Some(pb::param_value::Value::Number(2.4)));
        let sel = p.selection.unwrap();
        assert_eq!(sel.selected[0].kind, "face", "SessionRef keeps the UI's lowercase kind");
        assert_eq!(sel.note, "thicker");
        assert_eq!(p.messages[0].text, "hi");

        // ...and the same JSON becomes an evaluatable document.
        let doc = document_from_session_json(&s);
        assert_eq!(doc.scripts.len(), 1);
        assert_eq!(doc.selection.unwrap().selected[0].kind(), pb::EntityKind::Face);
    }

    /// Every session operation crosses the wire as one call to the registry —
    /// `POST /api/cadpole.v1.CadpoleOps/<Rpc>`, or `POST /api/ops/<Rpc>` against a
    /// server too old to have the first — which is what makes a restricted
    /// token's surface exactly the registry. A Host method that
    /// quietly used an alias route instead would be a hole in that, so the
    /// list is asserted rather than assumed.
    #[test]
    fn every_session_operation_goes_through_the_op_registry() {
        let src = include_str!("ops_host.rs");
        let body = src.split("impl Host for HttpHost {").nth(1).expect("the Host impl");
        let body = body.split("\n#[cfg(test)]").next().unwrap_or(body);
        for route in ["/api/session/script", "/api/session/params", "/api/session/selection", "/api/session/messages", "/api/events/poll"] {
            assert!(!body.contains(route), "the Host impl still reaches {route} directly instead of POST /api/ops/<Rpc>");
        }
        // The one read that is not an operation: the document itself, which
        // the local Render/Evaluate need and which has no rpc of its own.
        assert!(body.contains("/api/session/document"));
    }

    /// ...and it goes through it for *every* session operation.
    ///
    /// The test above only asked that no method reach a legacy route. Nothing
    /// asked that every method be here at all, so two were not: `Save` and
    /// `PostMessage` fell through to the trait's default and answered "needs a
    /// running `cadpole serve`: pass --server URL" -- to a caller that had passed
    /// exactly that. `cadpole op Save` and the MCP `save` tool could never work,
    /// and `save` is in `tools/list`, so an assistant was offered a tool that
    /// refuses every time it is used.
    ///
    /// Read off the registry rather than listed here, so a new session rpc
    /// fails this until it is proxied.
    #[test]
    fn every_session_operation_is_actually_proxied() {
        let src = include_str!("ops_host.rs");
        let body = src.split("impl Host for HttpHost {").nth(1).expect("the Host impl");
        let body = body.split("\n#[cfg(test)]").next().unwrap_or(body);
        // A `needs_session` operation whose request carries a `document` does
        // not need a Host method of its own: `handle` fills that field from
        // `host.document()`, which HttpHost answers from the server, and then
        // works on it locally. `GetSelection` and `ExplainRef` are those, and
        // they are correct as they stand. What has to be proxied is everything
        // else, which reaches the session only through a method here.
        let missing: Vec<&str> = cadpole_ops::registry()
            .iter()
            .filter(|m| m.needs_session)
            .filter(|m| !cadpole_ops::carries_its_own_document(&m.name))
            .map(|m| m.name.as_str())
            .filter(|name| !body.contains(&format!("\"{name}\"")))
            .collect();
        assert!(
            missing.is_empty(),
            "these session operations are not proxied by HttpHost, so `cadpole op` and `cadpole mcp` \
             answer \"needs a running cadpole serve\" to a caller that passed one: {missing:?}"
        );
    }
}
