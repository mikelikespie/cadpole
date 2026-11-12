//! The Cadpole operation registry.
//!
//! `proto/cadpole/v1/ops.proto` defines `service CadpoleOps`; this crate loads that
//! service out of the descriptor set embedded in the binary
//! (`cadpole_proto::descriptor`, built with `--include_source_info` so the
//! comments are still there) and exposes three things every front-end needs:
//!
//! * [`registry`] — one [`MethodInfo`] per rpc, with its documentation and its
//!   `(cadpole.op)` options (CLI name, idempotence, whether it needs a session,
//!   which response field is a PNG).
//! * [`json_schema`] — JSON Schema for any message, generated from the
//!   descriptor and the `(cadpole.field)` options. This is what MCP publishes as
//!   a tool's `inputSchema`.
//! * [`call_json`] / [`call_proto`] — run an operation: JSON (proto3 mapping)
//!   in, dynamic message, typed request, handler, typed response, JSON out.
//!
//! Engine operations (Evaluate, Render, Sweep, Export, PrintCheck, Inspect,
//! Drawing, RunTests, ApiDoc, ExplainRef, ListAssets, GetAsset) call `cadpole-engine`
//! directly against whatever document
//! the front-end supplies. Session operations (GetSession, GetSelection and
//! the writes) go through [`Host`], which the server implements natively
//! against its own session state and the CLI/MCP implement over HTTP.
//!
//! Adding an operation is: an rpc in `ops.proto` plus one arm in [`handle`].
//!
//! [`capability`] is the other half of the registry: which of those operations
//! a *restricted* caller — the limited-privilege listener — may invoke, and
//! why. A new rpc fails that module's test until it is classified.

pub mod capability;

use std::collections::HashMap;
use std::sync::OnceLock;

use anyhow::{Context as _, anyhow, bail};
use cadpole_proto::prost::Message as _;
use cadpole_proto::v1 as pb;
use cadpole_proto::wkt;
use prost_reflect::{
    ReflectMessage,
    DescriptorPool, DeserializeOptions, DynamicMessage, Kind, MessageDescriptor, MethodDescriptor, SerializeOptions,
};
use serde_json::{Map, Value, json};

/// Full name of the service every front-end is generated from.
pub const SERVICE: &str = "cadpole.v1.CadpoleOps";

// ---------------------------------------------------------------------------
// Descriptor pool
// ---------------------------------------------------------------------------

/// The descriptor pool for `cadpole.v1`, built once from the embedded bytes.
///
/// Panics only if the embedded descriptor set is not a valid
/// `FileDescriptorSet`, which would mean the build is broken.
pub fn pool() -> &'static DescriptorPool {
    static POOL: OnceLock<DescriptorPool> = OnceLock::new();
    POOL.get_or_init(|| {
        DescriptorPool::decode(cadpole_proto::descriptor).expect("the embedded cadpole.v1 descriptor set must parse")
    })
}

/// Leading comments, keyed by (file name, descriptor path).
///
/// `prost-reflect` gives every descriptor its `path()`; `SourceCodeInfo` in
/// the descriptor set maps the same paths to the comments protoc saw. Joining
/// the two is how an rpc's `///` comment becomes an MCP tool description.
fn comments() -> &'static HashMap<(String, Vec<i32>), String> {
    static COMMENTS: OnceLock<HashMap<(String, Vec<i32>), String>> = OnceLock::new();
    COMMENTS.get_or_init(|| {
        let mut out = HashMap::new();
        let Ok(set) = cadpole_proto::prost_types::FileDescriptorSet::decode(cadpole_proto::descriptor) else {
            return out;
        };
        for file in &set.file {
            let Some(info) = &file.source_code_info else { continue };
            for loc in &info.location {
                let text = loc.leading_comments.as_deref().unwrap_or("");
                if text.is_empty() {
                    continue;
                }
                out.insert((file.name().to_owned(), loc.path.clone()), clean_comment(text));
            }
        }
        out
    })
}

/// Turn protoc's raw `leading_comments` into prose: drop the single space
/// protoc leaves after each `//`, and trim the block.
fn clean_comment(raw: &str) -> String {
    raw.lines().map(|l| l.strip_prefix(' ').unwrap_or(l)).collect::<Vec<_>>().join("\n").trim().to_owned()
}

fn comment_at(file: &str, path: &[i32]) -> String {
    comments().get(&(file.to_owned(), path.to_vec())).cloned().unwrap_or_default()
}

/// First paragraph of a doc comment, joined into one line. Used where only a
/// sentence fits (`--help` summaries, `cadpole op --list`).
pub fn summary(doc: &str) -> String {
    doc.split("\n\n").next().unwrap_or("").split('\n').collect::<Vec<_>>().join(" ").trim().to_owned()
}

// ---------------------------------------------------------------------------
// Custom options
// ---------------------------------------------------------------------------

fn ext_message(opts: &DynamicMessage, extension: &str) -> Option<DynamicMessage> {
    let ext = opts.descriptor().parent_pool().get_extension_by_name(extension)?;
    if !opts.has_extension(&ext) {
        return None;
    }
    opts.get_extension(&ext).as_message().cloned()
}

fn str_field(m: &Option<DynamicMessage>, name: &str) -> String {
    m.as_ref().and_then(|m| m.get_field_by_name(name)).and_then(|v| v.as_str().map(str::to_owned)).unwrap_or_default()
}

fn bool_field(m: &Option<DynamicMessage>, name: &str) -> bool {
    m.as_ref().and_then(|m| m.get_field_by_name(name)).and_then(|v| v.as_bool()).unwrap_or(false)
}

fn list_field(m: &Option<DynamicMessage>, name: &str) -> Vec<String> {
    m.as_ref()
        .and_then(|m| m.get_field_by_name(name))
        .and_then(|v| v.as_list().map(|l| l.iter().filter_map(|v| v.as_str().map(str::to_owned)).collect()))
        .unwrap_or_default()
}

/// `(cadpole.field)` on one request or response field, with the defaults applied.
#[derive(Clone, Debug, Default)]
pub struct FieldOpts {
    /// Long CLI flag without dashes. Defaults to the field name with `_` -> `-`.
    pub flag: String,
    pub positional: bool,
    pub example: String,
    pub hidden: bool,
    pub short: String,
    pub required: bool,
}

/// The leading comment on a field, as written in the `.proto`.
pub fn field_doc(f: &prost_reflect::FieldDescriptor) -> String {
    comment_at(f.parent_file().name(), f.path())
}

/// True when the field carries an explicit `(cadpole.field)` option (as opposed
/// to the defaults [`field_opts`] fills in). Front-ends use this to treat
/// "asked for a flag" differently from "got one by default".
pub fn has_field_opts(f: &prost_reflect::FieldDescriptor) -> bool {
    let opts = f.options();
    opts.descriptor()
        .parent_pool()
        .get_extension_by_name("cadpole.field")
        .map(|e| opts.has_extension(&e))
        .unwrap_or(false)
}

/// Read `(cadpole.field)` off a field descriptor.
pub fn field_opts(f: &prost_reflect::FieldDescriptor) -> FieldOpts {
    let m = ext_message(&f.options(), "cadpole.field");
    let flag = str_field(&m, "flag");
    FieldOpts {
        flag: if flag.is_empty() { f.name().replace('_', "-") } else { flag },
        positional: bool_field(&m, "positional"),
        example: str_field(&m, "example"),
        hidden: bool_field(&m, "hidden"),
        short: str_field(&m, "short"),
        required: bool_field(&m, "required"),
    }
}

// ---------------------------------------------------------------------------
// Registry
// ---------------------------------------------------------------------------

/// One operation: an rpc of `CadpoleOps` with its documentation and options.
#[derive(Clone, Debug)]
pub struct MethodInfo {
    /// The rpc name, e.g. `Render`. This is the canonical name: it is what
    /// `POST /api/ops/<Method>` and `cadpole op <Method>` take.
    pub name: String,
    /// `cadpole.v1.CadpoleOps.Render`.
    pub full_name: String,
    /// snake_case of [`MethodInfo::name`], e.g. `render`. This is the MCP tool
    /// name.
    pub snake: String,
    /// `(cadpole.op).cli_name`, defaulting to [`MethodInfo::snake`].
    pub cli_name: String,
    /// Extra accepted spellings from `(cadpole.op).alias`.
    pub aliases: Vec<String>,
    /// The rpc's leading comment: the MCP tool description and CLI help.
    pub doc: String,
    /// Full name of the request message, e.g. `cadpole.v1.RenderRequest`.
    pub input: String,
    /// Full name of the response message.
    pub output: String,
    /// Read-only and safe to retry.
    pub idempotent: bool,
    /// Only a running `cadpole serve` can answer this.
    pub needs_session: bool,
    /// Response field holding a PNG, or empty.
    pub image_field: String,
    /// Kept out of generated listings.
    pub hidden: bool,
}

impl MethodInfo {
    /// True when `name` is any accepted spelling of this operation.
    pub fn matches(&self, name: &str) -> bool {
        self.name.eq_ignore_ascii_case(name)
            || self.snake == name
            || self.cli_name == name
            || self.aliases.iter().any(|a| a == name)
    }

    /// One-line description, for listings.
    pub fn summary(&self) -> String {
        summary(&self.doc)
    }
}

/// `GetSession` -> `get_session`, `ApiDoc` -> `api_doc`.
pub fn snake_case(name: &str) -> String {
    let mut out = String::with_capacity(name.len() + 4);
    for (i, c) in name.chars().enumerate() {
        if c.is_ascii_uppercase() {
            if i > 0 {
                out.push('_');
            }
            out.push(c.to_ascii_lowercase());
        } else {
            out.push(c);
        }
    }
    out
}

/// Every operation `CadpoleOps` defines, in declaration order.
///
/// This is the single list the MCP tool table, the HTTP route table and the
/// CLI verb table are all built from; `ops_test.rs` asserts they agree.
pub fn registry() -> &'static [MethodInfo] {
    static REGISTRY: OnceLock<Vec<MethodInfo>> = OnceLock::new();
    REGISTRY.get_or_init(|| {
        let service = pool().get_service_by_name(SERVICE).expect("cadpole.v1.CadpoleOps must be in the descriptor pool");
        let file = service.parent_file().name().to_owned();
        service
            .methods()
            .map(|m| {
                let o = ext_message(&m.options(), "cadpole.op");
                let name = m.name().to_owned();
                let snake = snake_case(&name);
                let cli = str_field(&o, "cli_name");
                MethodInfo {
                    cli_name: if cli.is_empty() { snake.clone() } else { cli },
                    doc: comment_at(&file, m.path()),
                    full_name: m.full_name().to_owned(),
                    input: m.input().full_name().to_owned(),
                    output: m.output().full_name().to_owned(),
                    idempotent: bool_field(&o, "idempotent"),
                    needs_session: bool_field(&o, "needs_session"),
                    image_field: str_field(&o, "image_field"),
                    hidden: bool_field(&o, "hidden"),
                    aliases: list_field(&o, "alias"),
                    name,
                    snake,
                }
            })
            .collect()
    })
}

/// Find an operation by rpc name, snake_case name, CLI name or alias.
pub fn method(name: &str) -> Option<&'static MethodInfo> {
    let name = name.rsplit('.').next().unwrap_or(name);
    registry().iter().find(|m| m.matches(name))
}

fn method_or_err(name: &str) -> anyhow::Result<&'static MethodInfo> {
    method(name).ok_or_else(|| {
        let known: Vec<&str> = registry().iter().map(|m| m.name.as_str()).collect();
        anyhow!("unknown operation {name:?}; known operations: {}", known.join(", "))
    })
}

fn descriptor(m: &MethodInfo) -> MethodDescriptor {
    pool()
        .get_service_by_name(SERVICE)
        .and_then(|s| s.methods().find(|d| d.name() == m.name))
        .expect("registry entries come from the pool")
}

// ---------------------------------------------------------------------------
// JSON Schema
// ---------------------------------------------------------------------------

/// JSON Schema (draft 2020-12 shapes, kept to the subset MCP clients read) for
/// a message, by full name (`cadpole.v1.RenderRequest`) or bare name
/// (`RenderRequest`, resolved in `cadpole.v1`).
///
/// Field documentation comes from the `.proto` comments and `examples` from
/// `(cadpole.field).example`; fields marked `(cadpole.field).hidden` are left out
/// entirely, because the front-end fills them in (that is how `document`
/// disappears from every schema).
pub fn json_schema(message: &str) -> anyhow::Result<Value> {
    let md = message_descriptor(message)?;
    let mut stack = Vec::new();
    Ok(message_schema(&md, &mut stack))
}

/// JSON Schema of an operation's request message.
pub fn input_schema(method: &str) -> anyhow::Result<Value> {
    let m = method_or_err(method)?;
    json_schema(&m.input)
}

/// True when this operation's request carries the document it works on.
///
/// Those operations reach a session only through `fill_document`, which asks
/// the host for the document and then works on it locally -- so a host that
/// proxies to a server needs no method of its own for them, and does need one
/// for everything else. `document` is `hidden`, so the published schema
/// cannot answer this and the descriptor has to.
pub fn carries_its_own_document(method: &str) -> bool {
    method_or_err(method)
        .ok()
        .and_then(|m| message_descriptor(&m.input).ok())
        .is_some_and(|md| md.get_field_by_name("document").is_some())
}

fn message_descriptor(message: &str) -> anyhow::Result<MessageDescriptor> {
    pool()
        .get_message_by_name(message)
        .or_else(|| pool().get_message_by_name(&format!("cadpole.v1.{message}")))
        .ok_or_else(|| anyhow!("no message named {message:?} in the cadpole.v1 descriptor pool"))
}

fn message_schema(md: &MessageDescriptor, stack: &mut Vec<String>) -> Value {
    // Self-referential messages are legal protobuf; expand each only once.
    if stack.iter().any(|n| n == md.full_name()) {
        return json!({"type": "object", "description": format!("recursive {}", md.full_name())});
    }
    stack.push(md.full_name().to_owned());

    let file = md.parent_file().name().to_owned();
    let mut props = Map::new();
    let mut required = Vec::new();
    for f in md.fields() {
        let o = field_opts(&f);
        if o.hidden {
            continue;
        }
        let mut s = field_schema(&f, stack);
        let doc = comment_at(&file, f.path());
        if !doc.is_empty() {
            s["description"] = json!(doc);
        }
        if !o.example.is_empty() {
            s["examples"] = json!([o.example]);
        }
        if o.required {
            required.push(json!(f.name()));
        }
        props.insert(f.name().to_owned(), s);
    }

    let type_opts = ext_message(&md.options(), "cadpole.type");
    let title = str_field(&type_opts, "title");
    let doc = comment_at(&file, md.path());
    let mut out = json!({"type": "object", "properties": Value::Object(props)});
    if !title.is_empty() {
        out["title"] = json!(title);
    }
    if !doc.is_empty() {
        out["description"] = json!(doc);
    }
    if !required.is_empty() {
        out["required"] = Value::Array(required);
    }
    stack.pop();
    out
}

fn field_schema(f: &prost_reflect::FieldDescriptor, stack: &mut Vec<String>) -> Value {
    if f.is_map() {
        let entry = match f.kind() {
            Kind::Message(m) => m,
            _ => return json!({"type": "object"}),
        };
        let value = entry.map_entry_value_field();
        return json!({"type": "object", "additionalProperties": scalar_schema(&value, stack)});
    }
    if f.is_list() {
        return json!({"type": "array", "items": scalar_schema(f, stack)});
    }
    scalar_schema(f, stack)
}

fn scalar_schema(f: &prost_reflect::FieldDescriptor, stack: &mut Vec<String>) -> Value {
    match f.kind() {
        Kind::Double | Kind::Float => json!({"type": "number"}),
        Kind::Int32 | Kind::Sint32 | Kind::Sfixed32 => json!({"type": "integer"}),
        Kind::Uint32 | Kind::Fixed32 => json!({"type": "integer", "minimum": 0}),
        Kind::Int64 | Kind::Sint64 | Kind::Sfixed64 => json!({"type": "integer"}),
        Kind::Uint64 | Kind::Fixed64 => json!({"type": "integer", "minimum": 0}),
        Kind::Bool => json!({"type": "boolean"}),
        Kind::String => json!({"type": "string"}),
        Kind::Bytes => json!({"type": "string", "contentEncoding": "base64"}),
        Kind::Enum(e) => {
            let values: Vec<Value> = e.values().map(|v| json!(v.name())).collect();
            json!({"type": "string", "enum": values})
        }
        Kind::Message(m) => match m.full_name() {
            // Well-known types have their own JSON mapping; describing their
            // fields would be actively misleading.
            "google.protobuf.Struct" => json!({"type": "object"}),
            "google.protobuf.Value" => json!({}),
            "google.protobuf.ListValue" => json!({"type": "array"}),
            "google.protobuf.Timestamp" => json!({"type": "string", "format": "date-time"}),
            "google.protobuf.Duration" => json!({"type": "string"}),
            _ => message_schema(&m, stack),
        },
    }
}

// ---------------------------------------------------------------------------
// Host: everything that needs a live session
// ---------------------------------------------------------------------------

fn needs_session(op: &str) -> anyhow::Error {
    anyhow!("{op} needs a running `cadpole serve`: pass --server URL (or set CADPOLE_SERVER)")
}

/// The session-shaped half of the world.
///
/// Engine operations never touch this; they run in-process. Everything that
/// reads or writes the live session does, so the same operation works
/// natively inside `cadpole serve` and over HTTP from `cadpole` and `cadpole mcp`.
///
/// Every method defaults to "no session here", so a host that only knows how
/// to load a document from disk implements [`Host::document`] and nothing else.
pub trait Host {
    /// The document engine operations run against when the request carries
    /// none: the file the CLI was given, or the session's current document.
    fn document(&self) -> anyhow::Result<pb::Document> {
        Err(anyhow!("no document: pass one with --doc FILE, or --server URL to use a session's"))
    }

    /// The asset table, bytes included, that a document arriving without
    /// one is evaluated against ([`fill_document`]): the document's
    /// own by default. A host that holds large assets answers it without
    /// copying the rest of the document.
    fn assets(&self) -> anyhow::Result<Vec<pb::Asset>> {
        self.document().map(|d| d.assets)
    }

    fn get_session(&self, _req: &pb::GetSessionRequest) -> anyhow::Result<pb::Session> {
        Err(needs_session("GetSession"))
    }

    fn set_script(&self, _req: &pb::SetScriptRequest) -> anyhow::Result<pb::SetScriptResponse> {
        Err(needs_session("SetScript"))
    }

    fn edit_script(&self, _req: &pb::EditScriptRequest) -> anyhow::Result<pb::EditScriptResponse> {
        Err(needs_session("EditScript"))
    }

    fn get_attachment(&self, _req: &pb::GetAttachmentRequest) -> anyhow::Result<pb::GetAttachmentResponse> {
        Err(needs_session("GetAttachment"))
    }

    fn ack(&self, _req: &pb::AckRequest) -> anyhow::Result<pb::AckResponse> {
        Err(needs_session("Ack"))
    }

    fn add_asset(&self, _req: &pb::AddAssetRequest) -> anyhow::Result<pb::AddAssetResponse> {
        Err(needs_session("AddAsset"))
    }

    fn set_params(&self, _req: &pb::SetParamsRequest) -> anyhow::Result<pb::SetParamsResponse> {
        Err(needs_session("SetParams"))
    }

    fn set_selection(&self, _req: &pb::SetSelectionRequest) -> anyhow::Result<pb::SetSelectionResponse> {
        Err(needs_session("SetSelection"))
    }

    fn reply(&self, _req: &pb::ReplyRequest) -> anyhow::Result<pb::ReplyResponse> {
        Err(needs_session("Reply"))
    }

    fn stream_reply(&self, _req: &pb::StreamReplyRequest) -> anyhow::Result<pb::StreamReplyResponse> {
        Err(needs_session("StreamReply"))
    }

    fn post_message(&self, _req: &pb::PostMessageRequest) -> anyhow::Result<pb::PostMessageResponse> {
        Err(needs_session("PostMessage"))
    }

    fn save(&self, _req: &pb::SaveRequest) -> anyhow::Result<pb::SaveResponse> {
        Err(needs_session("Save"))
    }

    fn wait_events(&self, _req: &pb::WaitEventsRequest) -> anyhow::Result<pb::WaitEventsResponse> {
        Err(needs_session("WaitEvents"))
    }

    fn report_client_failure(
        &self,
        _req: &pb::ReportClientFailureRequest,
    ) -> anyhow::Result<pb::ReportClientFailureResponse> {
        Err(needs_session("ReportClientFailure"))
    }

    /// The kernel-op memo evaluations through this host read and fill:
    /// a session's, kept across its evaluations. None for a
    /// one-shot host, which then pays nothing for it.
    fn memo(&self) -> Option<std::sync::Arc<cadpole_engine::cadpole_core::memo::Memo>> {
        None
    }

    /// Whether this host's `Evaluate`s may hand a solve too long for the
    /// evaluation to a job that runs past its deadline. True only for
    /// a host with a job runner, for its owner: `cadpole serve`'s evaluations
    /// with the owner's token, `cadpole solve`'s. Everyone else reads answers
    /// and attaches to running jobs, and never starts one.
    fn solve_jobs(&self) -> bool {
        false
    }

    /// Run a document's pending solves as jobs and evaluate it again as they
    /// land. The request's document, or the host's when it has none.
    fn solve(&self, _req: &pb::SolveRequest) -> anyhow::Result<pb::SolveResponse> {
        Err(no_jobs("Solve"))
    }

    /// This machine's solve jobs.
    fn list_jobs(&self, _req: &pb::ListJobsRequest) -> anyhow::Result<pb::ListJobsResponse> {
        Err(no_jobs("ListJobs"))
    }

    /// Wait for solve jobs to end.
    fn wait_jobs(&self, _req: &pb::WaitJobsRequest) -> anyhow::Result<pb::WaitJobsResponse> {
        Err(no_jobs("WaitJobs"))
    }

    /// Stop solve jobs.
    fn cancel_jobs(&self, _req: &pb::CancelJobsRequest) -> anyhow::Result<pb::CancelJobsResponse> {
        Err(no_jobs("CancelJobs"))
    }
}

/// What a host without solve jobs answers to the job operations: the CLI,
/// `cadpole serve` and `cadpole mcp` have them; a library embedder, the browser's
/// engine and the apps do not.
fn no_jobs(op: &str) -> anyhow::Error {
    anyhow!("{op}: solve jobs need native Cadpole with a solution cache -- `cadpole solve`, `cadpole jobs`, `cadpole serve` or `cadpole mcp`")
}

/// A host with no session and no document: every engine operation must carry
/// its own `document`. Used by tests and by `cadpole-engine`'s own callers.
pub struct NoHost;
impl Host for NoHost {}

/// A host that serves one in-memory document and no session. This is the CLI
/// without `--server`.
pub struct DocumentHost(pub pb::Document);
impl Host for DocumentHost {
    fn document(&self) -> anyhow::Result<pb::Document> {
        Ok(self.0.clone())
    }
}

// ---------------------------------------------------------------------------
// Dispatch
// ---------------------------------------------------------------------------

fn fill_document(slot: &mut Option<pb::Document>, host: &dyn Host) -> anyhow::Result<()> {
    match slot {
        None => *slot = Some(host.document()?),
        // The browser sends its scripts and parameter overrides and never
        // an asset -- it does not hold them (remote-engine.ts) -- so a
        // document that arrives with no asset table is evaluated against
        // the host's. Without this, no document with an import in
        // it evaluated in the web UI at all. A caller that sent a table of
        // its own keeps it; a host with none (a render host, the CLI with
        // no document) leaves the request as it came. Nothing is granted:
        // a listener token may read every asset already (`GetAsset`).
        Some(doc) if doc.assets.is_empty() => {
            if let Ok(assets) = host.assets() {
                doc.assets = assets;
            }
        }
        Some(_) => {}
    }
    Ok(())
}

/// Run one operation over encoded protobuf. The bytes are the request message
/// named by [`MethodInfo::input`]; the result is [`MethodInfo::output`].
pub fn call_proto(method: &str, request: &[u8], host: &dyn Host) -> anyhow::Result<Vec<u8>> {
    handle(method_or_err(method)?, request, host)
}

/// The one place an operation is implemented. Adding an rpc to `ops.proto`
/// means adding an arm here; nothing else in Cadpole changes.
/// Refuse a request that leaves out a field the schema marks `required`.
///
/// `options.proto` says of that option: "Reject the call when the field is
/// absent." Nothing did. It reached the JSON Schema an assistant reads and
/// clap's `required(true)`, and that was all -- so the CLI could not omit
/// one, and every other caller could. `Reply {}` answered 200 and posted an
/// empty message to the user from the assistant.
///
/// Checked here rather than at a decoder, so it holds for protobuf and JSON
/// alike: two encodings of one request must not get two answers.
///
/// **What it can and cannot see.** proto3 gives a plain scalar no presence,
/// so "absent" and "the default" are the same bytes and the only enforceable
/// reading of `required` is "not the default". That is right for a string, a
/// `bytes`, a message and an enum, where the default is not a value anybody
/// means -- an empty name, no text, `..._UNSPECIFIED`. It is wrong for a
/// number whose zero is a real answer: `AckRequest.cursor` is `required` and
/// zero means the start of the log. So numbers are left alone here and get
/// their own checks where zero is meaningless (`base_revision` conflicts
/// against a revision that starts at 1). Giving those fields explicit
/// presence -- `optional uint64` -- is what would close the gap, and it is a
/// schema change with a regenerated Swift tree behind it.
fn check_required(m: &MethodInfo, bytes: &[u8]) -> anyhow::Result<()> {
    let md = message_descriptor(&m.input)?;
    // Almost nothing declares one, and the two heaviest requests -- Evaluate
    // and Render -- declare none, so the hot path stops on this line.
    if !md.fields().any(|f| field_opts(&f).required) {
        return Ok(());
    }
    let dynamic = DynamicMessage::decode(md.clone(), bytes).with_context(|| format!("decoding {}", m.input))?;
    for f in md.fields() {
        if !field_opts(&f).required || f.is_list() || f.is_map() {
            continue;
        }
        let missing = match f.kind() {
            Kind::String | Kind::Bytes | Kind::Message(_) | Kind::Enum(_) => !dynamic.has_field(&f),
            _ => false,
        };
        if missing {
            anyhow::bail!(
                "{} needs `{}`: it is required, and what arrived was the empty default",
                m.name,
                f.name()
            );
        }
    }
    Ok(())
}

fn handle(m: &MethodInfo, bytes: &[u8], host: &dyn Host) -> anyhow::Result<Vec<u8>> {
    check_required(m, bytes)?;
    let ctx = || format!("decoding {}", m.input);
    Ok(match m.name.as_str() {
        "Evaluate" => {
            let mut r = pb::EvaluateRequest::decode(bytes).with_context(ctx)?;
            fill_document(&mut r.document, host)?;
            let options = cadpole_engine::EvalOptions { memo: host.memo(), preview: r.preview, solve_jobs: host.solve_jobs() };
            cadpole_engine::evaluate_with_options(&r, options).encode_to_vec()
        }
        "Render" => {
            let mut r = pb::RenderRequest::decode(bytes).with_context(ctx)?;
            fill_document(&mut r.document, host)?;
            cadpole_engine::render(&r)?.encode_to_vec()
        }
        "Sweep" => {
            let mut r = pb::SweepRequest::decode(bytes).with_context(ctx)?;
            fill_document(&mut r.document, host)?;
            cadpole_engine::sweep(&r)?.encode_to_vec()
        }
        "Animate" => {
            let mut r = pb::AnimateRequest::decode(bytes).with_context(ctx)?;
            fill_document(&mut r.document, host)?;
            cadpole_engine::animate(&r)?.encode_to_vec()
        }
        "Export" => {
            let mut r = pb::ExportRequest::decode(bytes).with_context(ctx)?;
            fill_document(&mut r.document, host)?;
            cadpole_engine::export(&r)?.encode_to_vec()
        }
        "PrintCheck" => {
            let mut r = pb::PrintCheckRequest::decode(bytes).with_context(ctx)?;
            fill_document(&mut r.document, host)?;
            cadpole_engine::print_check(&r)?.encode_to_vec()
        }
        "Plate" => {
            let mut r = pb::PlateRequest::decode(bytes).with_context(ctx)?;
            fill_document(&mut r.document, host)?;
            cadpole_engine::plate(&r)?.encode_to_vec()
        }
        "Inspect" => {
            let mut r = pb::InspectRequest::decode(bytes).with_context(ctx)?;
            fill_document(&mut r.document, host)?;
            cadpole_engine::inspect(&r)?.encode_to_vec()
        }
        "Drawing" => {
            let mut r = pb::DrawingRequest::decode(bytes).with_context(ctx)?;
            fill_document(&mut r.document, host)?;
            cadpole_engine::drawing(&r)?.encode_to_vec()
        }
        "RunTests" => {
            let mut r = pb::RunTestsRequest::decode(bytes).with_context(ctx)?;
            fill_document(&mut r.document, host)?;
            cadpole_engine::run_tests(&r)?.encode_to_vec()
        }
        "ApiDoc" => {
            pb::ApiDocRequest::decode(bytes).with_context(ctx)?;
            cadpole_engine::cadpole_script::api_doc().encode_to_vec()
        }
        "GetSession" => host.get_session(&pb::GetSessionRequest::decode(bytes).with_context(ctx)?)?.encode_to_vec(),
        "GetSelection" => {
            let mut r = pb::GetSelectionRequest::decode(bytes).with_context(ctx)?;
            fill_document(&mut r.document, host)?;
            let doc = r.document.take().expect("filled above");
            cadpole_engine::resolve_selection(&doc, r.skip_geometry, r.tolerance).encode_to_vec()
        }
        "ExplainRef" => {
            let mut r = pb::ExplainRefRequest::decode(bytes).with_context(ctx)?;
            fill_document(&mut r.document, host)?;
            let doc = r.document.take().expect("filled above");
            cadpole_engine::explain_ref(&doc, &r).encode_to_vec()
        }
        "ListAssets" => {
            let mut r = pb::ListAssetsRequest::decode(bytes).with_context(ctx)?;
            fill_document(&mut r.document, host)?;
            cadpole_engine::list_assets(&r.document.take().expect("filled above")).encode_to_vec()
        }
        "GetAsset" => {
            let mut r = pb::GetAssetRequest::decode(bytes).with_context(ctx)?;
            fill_document(&mut r.document, host)?;
            let doc = r.document.take().expect("filled above");
            cadpole_engine::get_asset(&doc, &r)?.encode_to_vec()
        }
        "AddAsset" => host.add_asset(&pb::AddAssetRequest::decode(bytes).with_context(ctx)?)?.encode_to_vec(),
        "EditScript" => host.edit_script(&pb::EditScriptRequest::decode(bytes).with_context(ctx)?)?.encode_to_vec(),
        "GetAttachment" => host.get_attachment(&pb::GetAttachmentRequest::decode(bytes).with_context(ctx)?)?.encode_to_vec(),
        "Ack" => host.ack(&pb::AckRequest::decode(bytes).with_context(ctx)?)?.encode_to_vec(),
        "SetScript" => host.set_script(&pb::SetScriptRequest::decode(bytes).with_context(ctx)?)?.encode_to_vec(),
        "SetParams" => host.set_params(&pb::SetParamsRequest::decode(bytes).with_context(ctx)?)?.encode_to_vec(),
        "SetSelection" => host.set_selection(&pb::SetSelectionRequest::decode(bytes).with_context(ctx)?)?.encode_to_vec(),
        "Reply" => host.reply(&pb::ReplyRequest::decode(bytes).with_context(ctx)?)?.encode_to_vec(),
        "StreamReply" => host.stream_reply(&pb::StreamReplyRequest::decode(bytes).with_context(ctx)?)?.encode_to_vec(),
        "PostMessage" => host.post_message(&pb::PostMessageRequest::decode(bytes).with_context(ctx)?)?.encode_to_vec(),
        "Save" => host.save(&pb::SaveRequest::decode(bytes).with_context(ctx)?)?.encode_to_vec(),
        "WaitEvents" => host.wait_events(&pb::WaitEventsRequest::decode(bytes).with_context(ctx)?)?.encode_to_vec(),
        "ReportClientFailure" => {
            host.report_client_failure(&pb::ReportClientFailureRequest::decode(bytes).with_context(ctx)?)?.encode_to_vec()
        }
        // The solve's document is not filled from the host here: a host that runs the jobs
        // somewhere else (`cadpole mcp`'s, proxying to `cadpole serve`) would
        // otherwise fetch the session's document, assets and all, only to
        // send it straight back. The host fills it where the solve runs.
        "Solve" => host.solve(&pb::SolveRequest::decode(bytes).with_context(ctx)?)?.encode_to_vec(),
        "ListJobs" => host.list_jobs(&pb::ListJobsRequest::decode(bytes).with_context(ctx)?)?.encode_to_vec(),
        "WaitJobs" => host.wait_jobs(&pb::WaitJobsRequest::decode(bytes).with_context(ctx)?)?.encode_to_vec(),
        "CancelJobs" => host.cancel_jobs(&pb::CancelJobsRequest::decode(bytes).with_context(ctx)?)?.encode_to_vec(),
        other => bail!("operation {other} has no handler; add one in cadpole-ops"),
    })
}

// ---------------------------------------------------------------------------
// JSON front door
// ---------------------------------------------------------------------------

/// proto3 JSON, with Cadpole's two deviations, both chosen so JSON output matches
/// the hand-written session JSON the web UI already speaks: snake_case field
/// names (the repo convention) and 64-bit integers as numbers rather than
/// strings (`seq` and `revision` are small, and every client treats them as
/// numbers).
pub fn serialize_options() -> SerializeOptions {
    SerializeOptions::new().skip_default_fields(true).stringify_64_bit_integers(false).use_proto_field_name(true)
}

/// [`serialize_options`] but emitting every field, including the ones at their
/// default. Used where a stable, complete shape matters more than brevity
/// (`cadpole doc --json`).
pub fn serialize_options_full() -> SerializeOptions {
    serialize_options().skip_default_fields(false)
}

fn deserialize_options() -> DeserializeOptions {
    // Unknown fields are ignored rather than rejected: a newer assistant
    // passing a field this binary does not have yet should degrade, not fail.
    DeserializeOptions::new().deny_unknown_fields(false)
}

/// Reject an enum value this schema does not define.
///
/// proto3 JSON says an unrecognised enum value is an error. prost-reflect
/// gates that on `deny_unknown_fields` -- the same switch that decides what an
/// unknown *field* means -- and those are not the same question. An unknown
/// field from a newer caller should degrade, which is why that switch is off.
/// An unknown enum value must not: the field is then left at its zero value
/// and the answer comes back confidently wrong.
///
/// Read leniently, `{"format": "obj"}` on `Export` would return a binary STL
/// under a 200 -- not a refusal, not an OBJ, and nothing said so. An
/// out-of-range *number* takes the same route by a different road:
/// prost-reflect keeps it, and the handler's conversion falls back.
fn check_enum_values(md: &MessageDescriptor, value: &Value) -> anyhow::Result<()> {
    let Value::Object(fields) = value else { return Ok(()) };
    for (key, v) in fields {
        // An unknown field is still ignored, deliberately.
        let Some(fd) = md.get_field_by_json_name(key).or_else(|| md.get_field_by_name(key)) else { continue };
        check_enum_kind(&fd.kind(), v, &format!("{}.{}", md.name(), fd.name()))?;
    }
    Ok(())
}

fn check_enum_kind(kind: &Kind, v: &Value, at: &str) -> anyhow::Result<()> {
    if let Value::Array(items) = v {
        for item in items {
            check_enum_kind(kind, item, at)?;
        }
        return Ok(());
    }
    match kind {
        Kind::Enum(ed) => {
            let known = match v {
                Value::Null => true,
                Value::String(s) => ed.get_value_by_name(s).is_some(),
                Value::Number(n) => n.as_i64().and_then(|i| i32::try_from(i).ok()).is_some_and(|i| ed.get_value(i).is_some()),
                _ => false,
            };
            if known {
                return Ok(());
            }
            let names: Vec<String> = ed.values().map(|x| x.name().to_owned()).collect();
            anyhow::bail!("{at}: {v} is not a value of {}. Expected one of: {}", ed.full_name(), names.join(", "))
        }
        Kind::Message(m) if m.is_map_entry() => {
            let Value::Object(entries) = v else { return Ok(()) };
            let value_kind = m.map_entry_value_field().kind();
            for (k, mv) in entries {
                check_enum_kind(&value_kind, mv, &format!("{at}[{k}]"))?;
            }
            Ok(())
        }
        Kind::Message(m) => check_enum_values(m, v),
        _ => Ok(()),
    }
}

/// Decode proto3 JSON into a message by full name.
pub fn json_to_proto(message: &str, input: &Value) -> anyhow::Result<Vec<u8>> {
    let md = message_descriptor(message)?;
    check_enum_values(&md, input)?;
    let dynamic = DynamicMessage::deserialize_with_options(md, input.clone(), &deserialize_options())
        .map_err(|e| anyhow!("bad {message}: {e}"))?;
    Ok(dynamic.encode_to_vec())
}

/// Encode a message's bytes as proto3 JSON.
///
/// Fields marked `(cadpole.field).hidden` are left out: on a response that means
/// bulk data the front-ends should not print (see options.proto).
pub fn proto_to_json(message: &str, bytes: &[u8], options: &SerializeOptions) -> anyhow::Result<Value> {
    let md = message_descriptor(message)?;
    let dynamic = DynamicMessage::decode(md.clone(), bytes).with_context(|| format!("decoding {message}"))?;
    let mut v: Value = dynamic.serialize_with_options(serde_json::value::Serializer, options)?;
    if let Some(o) = v.as_object_mut() {
        for f in md.fields() {
            if field_opts(&f).hidden {
                o.remove(f.name());
            }
        }
    }
    Ok(v)
}

/// Run one operation with JSON in and JSON out.
///
/// The round trip is JSON -> `DynamicMessage` -> encoded bytes -> the typed
/// prost request -> the handler -> the typed response -> `DynamicMessage` ->
/// JSON. The dynamic hop is what lets a front-end accept and print any
/// operation without knowing the types.
pub fn call_json(method: &str, input: &Value, host: &dyn Host) -> anyhow::Result<Value> {
    call_json_with(method, input, host, &serialize_options())
}

/// [`call_json`] with explicit output formatting.
pub fn call_json_with(method: &str, input: &Value, host: &dyn Host, options: &SerializeOptions) -> anyhow::Result<Value> {
    let m = method_or_err(method)?;
    let d = descriptor(m);
    let input = if input.is_null() { Value::Object(Map::new()) } else { input.clone() };
    let want = d.input();
    check_enum_values(&want, &input).map_err(|e| anyhow!("{}: {e}", m.name))?;
    let request = DynamicMessage::deserialize_with_options(want, input, &deserialize_options())
        .map_err(|e| anyhow!("{}: bad input: {e}", m.name))?;
    let response = handle(m, &request.encode_to_vec(), host)?;
    proto_to_json(&m.output, &response, options)
}

// ---------------------------------------------------------------------------
// google.protobuf.Struct <-> serde_json
// ---------------------------------------------------------------------------

/// Convert arbitrary JSON into a `google.protobuf.Struct`, for event payloads.
///
/// Non-object input becomes a `Struct` with a single `value` key, because a
/// `Struct` can only be a map at the top level.
pub fn json_to_struct(v: &Value) -> wkt::Struct {
    match v {
        Value::Object(o) => wkt::Struct { fields: o.iter().map(|(k, v)| (k.clone(), json_to_value(v))).collect() },
        other => wkt::Struct { fields: [("value".to_owned(), json_to_value(other))].into_iter().collect() },
    }
}

/// Convert a `google.protobuf.Struct` back into JSON, the inverse of
/// [`json_to_struct`]. Front-ends that accept a `Struct` on the wire (the
/// camera attached to a screenshot) store JSON.
pub fn struct_to_json(s: &wkt::Struct) -> Value {
    Value::Object(s.fields.iter().map(|(k, v)| (k.clone(), value_to_json(v))).collect())
}

fn value_to_json(v: &wkt::Value) -> Value {
    use wkt::value::Kind as K;
    match &v.kind {
        None | Some(K::NullValue(_)) => Value::Null,
        Some(K::BoolValue(b)) => Value::Bool(*b),
        Some(K::NumberValue(n)) => serde_json::Number::from_f64(*n).map(Value::Number).unwrap_or(Value::Null),
        Some(K::StringValue(s)) => Value::String(s.clone()),
        Some(K::ListValue(l)) => Value::Array(l.values.iter().map(value_to_json).collect()),
        Some(K::StructValue(s)) => struct_to_json(s),
    }
}

fn json_to_value(v: &Value) -> wkt::Value {
    use wkt::value::Kind as K;
    let kind = match v {
        Value::Null => K::NullValue(wkt::NullValue::NullValue as i32),
        Value::Bool(b) => K::BoolValue(*b),
        Value::Number(n) => K::NumberValue(n.as_f64().unwrap_or(0.0)),
        Value::String(s) => K::StringValue(s.clone()),
        Value::Array(a) => K::ListValue(wkt::ListValue { values: a.iter().map(json_to_value).collect() }),
        Value::Object(_) => K::StructValue(json_to_struct(v)),
    };
    wkt::Value { kind: Some(kind) }
}

#[cfg(test)]
mod tests {
    use super::*;

    /// An enum value this schema does not define is refused, not read as the
    /// zero value.
    ///
    /// Otherwise `{"format": "obj"}` on `Export` returns a binary STL under a
    /// 200 -- not a refusal and not an OBJ. prost-reflect gates unknown enum
    /// values on `deny_unknown_fields`, which is off here on purpose so that
    /// an unknown *field* from a newer caller degrades rather than failing;
    /// the two questions share one switch, and only one of them should be
    /// lenient.
    #[test]
    fn an_enum_value_the_schema_does_not_define_is_refused() {
        let e = json_to_proto("cadpole.v1.ExportRequest", &serde_json::json!({"format": "obj"}))
            .expect_err("a value that is not in the enum must not decode")
            .to_string();
        assert!(e.contains("not a value of cadpole.v1.ExportFormat"), "{e}");
        assert!(e.contains("EXPORT_FORMAT_OBJ"), "the message names what would have worked: {e}");

        // A number outside the enum took the same road by a different route:
        // prost-reflect keeps it and the handler's conversion falls back.
        assert!(json_to_proto("cadpole.v1.ExportRequest", &serde_json::json!({"format": 99})).is_err());

        // Every spelling that is real still decodes, the number included.
        for good in [serde_json::json!({"format": "EXPORT_FORMAT_OBJ"}), serde_json::json!({"format": 2})] {
            json_to_proto("cadpole.v1.ExportRequest", &good).expect("a defined value decodes");
        }

        // And an unknown *field* still degrades, which is the whole reason
        // `deny_unknown_fields` is off.
        json_to_proto("cadpole.v1.ExportRequest", &serde_json::json!({"format": 2, "from_a_newer_client": 1}))
            .expect("an unknown field is still ignored");
    }

    #[test]
    fn the_registry_has_every_rpc_with_its_comment_and_options() {
        let names: Vec<&str> = registry().iter().map(|m| m.name.as_str()).collect();
        assert_eq!(
            names,
            vec![
                "Evaluate",
                "Render",
                "Sweep",
                "Animate",
                "Export",
                "PrintCheck",
                "Inspect",
                "Drawing",
                "RunTests",
                "ApiDoc",
                "GetSession",
                "GetSelection",
                "SetScript",
                "SetParams",
                "SetSelection",
                "Reply",
                "StreamReply",
                "PostMessage",
                "Save",
                "WaitEvents",
                "ExplainRef",
                "EditScript",
                "GetAttachment",
                "Ack",
                "ListAssets",
                "AddAsset",
                "GetAsset",
                "ReportClientFailure",
                "Solve",
                "ListJobs",
                "WaitJobs",
                "CancelJobs"
            ]
        );
        let render = method("Render").unwrap();
        assert_eq!(render.snake, "render");
        assert_eq!(render.cli_name, "render");
        assert_eq!(render.image_field, "png");
        assert!(render.idempotent && !render.needs_session);
        assert!(render.doc.starts_with("Render one image of a document"), "comments must survive: {:?}", render.doc);

        let eval = method("Evaluate").unwrap();
        assert_eq!(eval.cli_name, "eval", "(cadpole.op).cli_name overrides the snake_case name");
        assert!(method("eval").is_some() && method("evaluate").is_some());

        let session = method("GetSession").unwrap();
        assert!(session.needs_session);
        assert_eq!(session.snake, "get_session");
    }

    #[test]
    fn hidden_fields_are_absent_and_examples_are_published() {
        let s = json_schema("RenderRequest").unwrap();
        let props = s["properties"].as_object().unwrap();
        assert!(!props.contains_key("document"), "(cadpole.field).hidden must remove `document`");
        assert_eq!(props["width"]["type"], "integer");
        assert_eq!(props["width"]["examples"][0], "800");
        assert!(props["width"]["description"].as_str().unwrap().contains("Image width"));
        assert_eq!(props["draw_edges"]["type"], "boolean");
        assert_eq!(props["param_overrides"]["type"], "object");
        assert_eq!(props["body_ids"]["type"], "array");
        assert_eq!(props["backend"]["enum"][1], "RENDER_BACKEND_RASTER");
        assert_eq!(s["title"], "Render one image of a document");
    }

    /// The generated schema is a published interface: MCP clients and the
    /// `/api/ops` listing show it to people. A golden keeps a careless proto
    /// edit from silently changing what assistants are told.
    #[test]
    fn render_request_schema_matches_the_golden() {
        let golden = include_str!("../testdata/render_request.schema.json");
        let actual = serde_json::to_string_pretty(&json_schema("cadpole.v1.RenderRequest").unwrap()).unwrap();
        if actual.trim() != golden.trim() {
            panic!(
                "src/rust/cadpole-ops/testdata/render_request.schema.json is out of date; \
                 update it to:\n<<<ACTUAL\n{actual}\nACTUAL"
            );
        }
    }

    /// The same for `Drawing`: the colour look's fields -- `look`,
    /// `section`, `legend`, `labels`, `crop`, `model_unit` -- are what an
    /// assistant reads to draw a plan.
    #[test]
    fn drawing_request_schema_matches_the_golden() {
        let golden = include_str!("../testdata/drawing_request.schema.json");
        let actual = serde_json::to_string_pretty(&json_schema("cadpole.v1.DrawingRequest").unwrap()).unwrap();
        if actual.trim() != golden.trim() {
            if let Ok(dir) = std::env::var("TEST_UNDECLARED_OUTPUTS_DIR") {
                let _ = std::fs::write(format!("{dir}/drawing_request.schema.json"), &actual);
            }
            panic!(
                "src/rust/cadpole-ops/testdata/drawing_request.schema.json is out of date; the new one is in \
                 bazel-testlogs/src/rust/cadpole-ops/cadpole_ops_test/test.outputs/"
            );
        }
    }

    #[test]
    fn call_json_round_trips_render() {
        let doc = cadpole_engine::document_from_scripts("t", vec![("main.star".into(), "b = box(10, 20, 5)".into())]);
        let host = DocumentHost(doc);
        let out = call_json("Render", &json!({"width": 64, "height": 48, "camera": {"preset": "front"}}), &host).unwrap();
        assert_eq!(out["width"], 64);
        assert_eq!(out["height"], 48);
        let png = out["png"].as_str().expect("png is base64 in proto3 JSON");
        assert!(png.starts_with("iVBORw0K"), "{}", &png[..16.min(png.len())]);
        assert!(out["stats"]["passes"].as_u64().unwrap() >= 1);
        assert!(out.get("diagnostics").is_none(), "a clean script has no diagnostics: {out}");

        // `(cadpole.field).hidden` keeps the whole B-rep out of the JSON view,
        // while the typed response still carries it.
        assert!(out.get("model").is_none(), "RenderResponse.model must not reach a front-end: {out}");
        let request = json_to_proto("cadpole.v1.RenderRequest", &json!({"width": 32, "height": 24})).unwrap();
        let typed = pb::RenderResponse::decode(call_proto("Render", &request, &host).unwrap().as_slice()).unwrap();
        assert!(typed.model.unwrap().bodies[0].id.starts_with("box_1#"));
    }

    /// `Animate` through the JSON front door (what `cadpole op Animate` and the
    /// MCP tool send): a listing, then two frames of a one-second pose whose
    /// file comes back as base64 in `data` with the per-frame counts beside
    /// it.
    #[test]
    fn call_json_round_trips_animate() {
        let doc = cadpole_engine::document_from_scripts(
            "t",
            vec![(
                "main.star".into(),
                "lift = param(\"lift\", 0.0, min=0, max=20)\n\
                 b = instance(part(box(10, 10, 5, name=\"b\"), name=\"bp\"), name=\"bi\")\n\
                 explode(b, (0, 0, lift))\n\
                 animation(\"drop\", [keyframe(0, lift=20), keyframe(1, lift=0)], fps=1)\n"
                    .into(),
            )],
        );
        let host = DocumentHost(doc);
        let out = call_json("Animate", &json!({"list": true}), &host).unwrap();
        assert_eq!(out["animations"][0]["name"], "drop");
        assert_eq!(out["animations"][0]["frames"], 2);
        assert!(out.get("data").is_none(), "a listing renders nothing: {out}");
        let out = call_json("Animate", &json!({"name": "drop", "width": 32, "height": 24, "draft": true, "format": "gif"}), &host).unwrap();
        assert_eq!(out["frame_count"], 2);
        assert_eq!(out["format"], "gif");
        let data = out["data"].as_str().expect("data is base64 in proto3 JSON");
        assert!(data.starts_with("R0lGODlh"), "GIF89a in base64: {}", &data[..8.min(data.len())]);
        assert_eq!(out["frame_stats"][1]["ops_run"].as_u64(), None, "zero is skipped in the JSON view: a hit-only frame");
        assert_eq!(out["frame_stats"][1]["ops_hit"], 2);
        assert_eq!(out["suggested_filename"], "t_drop.gif");
    }

    #[test]
    fn session_operations_say_what_is_missing() {
        let e = call_json("GetSession", &json!({}), &NoHost).unwrap_err().to_string();
        assert!(e.contains("cadpole serve"), "{e}");
        let e = call_json("Render", &json!({}), &NoHost).unwrap_err().to_string();
        assert!(e.contains("--doc"), "{e}");
        let e = call_json("Nope", &json!({}), &NoHost).unwrap_err().to_string();
        assert!(e.contains("unknown operation"), "{e}");
        // ...and so do the ones added since. These carry `required` fields, so
        // the request has to be a real one: an empty object is now refused for
        // the field it left out, before anything asks whether there is a
        // session, which is a different (and also correct) answer.
        for (op, req) in [
            ("EditScript", json!({"edits": [{"old_str": "a", "new_str": "b"}], "base_revision": 1})),
            ("GetAttachment", json!({"seq": 1})),
            ("Ack", json!({"client": "t", "cursor": 0})),
            ("AddAsset", json!({"name": "a.bin", "data": "AQ=="})),
            ("ReportClientFailure", json!({"kind": "CLIENT_FAILURE_KIND_EXCEPTION", "message": "x"})),
        ] {
            let e = call_json(op, &req, &NoHost).unwrap_err().to_string();
            assert!(e.contains("cadpole serve"), "{op}: {e}");
        }
    }

    /// `required: true` is enforced, not merely published.
    ///
    /// `options.proto` promises "Reject the call when the field is absent",
    /// and nothing kept that promise: the option reached the JSON Schema an
    /// assistant reads and clap's `required(true)`, so the CLI could not omit
    /// one and every other caller could. `Reply {}` answered 200 and posted an
    /// empty message to the user, signed by the assistant.
    #[test]
    fn a_request_that_leaves_out_a_required_field_is_refused() {
        for (op, missing) in [("Reply", "text"), ("PostMessage", "text"), ("AddAsset", "name"), ("GetAsset", "name")] {
            let e = call_json(op, &json!({}), &NoHost).unwrap_err().to_string();
            assert!(e.contains(&format!("needs `{missing}`")), "{op} should name the field it wants: {e}");
        }

        // An enum's `_UNSPECIFIED` is not a value anybody means, so a required
        // enum left out is left out.
        let e = call_json("ReportClientFailure", &json!({"message": "x"}), &NoHost).unwrap_err().to_string();
        assert!(e.contains("needs `kind`"), "{e}");

        // A number is not checked here and must not be: `AckRequest.cursor` is
        // `required` and zero is the start of the log, a real answer. It gets
        // as far as the host, which is the operation's own business.
        let e = call_json("Ack", &json!({"client": "t"}), &NoHost).unwrap_err().to_string();
        assert!(e.contains("cadpole serve"), "a zero cursor is a cursor: {e}");
    }

    /// `Inspect` is an engine operation like `PrintCheck`: a document in,
    /// per-body measurements and the grouped diagnostics out, through the
    /// same JSON front door every other front-end uses.
    #[test]
    fn inspect_reports_every_body_and_groups_the_diagnostics() {
        // Two boxes sharing a face: the B-rep kernel refuses the union and
        // the fallback is an info on `part`, which is what the grouping is
        // there to carry.
        let doc = cadpole_engine::document_from_scripts(
            "t",
            vec![(
                "main.star".into(),
                "a = box(20, 20, 10, name=\"a\")\n\
                 b = box(10, 10, 6, at=(0, 0, 10), name=\"b\")\n\
                 part = union(a, b, name=\"part\")\n"
                    .into(),
            )],
        );
        let host = DocumentHost(doc);
        let out = call_json("Inspect", &json!({}), &host).unwrap();
        assert_eq!(out["ok"], true, "{out}");
        let bodies = out["bodies"].as_array().expect("bodies");
        assert_eq!(bodies.len(), 1, "{out}");
        let part = &bodies[0];
        assert_eq!(part["name"], "part");
        assert_eq!(part["kind"], "mesh");
        assert_eq!(part["solid"]["is_solid"], true, "{part}");
        assert!(part["solid"].get("failing").is_none(), "nothing fails on a clean part: {part}");
        assert!(part["triangles"].as_u64().unwrap() > part["faces"].as_u64().unwrap());
        assert!(part["summary"].as_str().unwrap().starts_with("mesh, "), "{part}");
        assert!(part["bbox"]["max"]["z"].as_f64().unwrap() > 15.9, "the cap stands on the base: {part}");
        // The fallback's info is grouped under `part`, with the union's span.
        let groups = out["features"].as_array().expect("features");
        let part_group = groups.iter().find(|g| g["feature_id"] == "part").expect("a group for part");
        assert_eq!(part_group["kind"], "union");
        assert_eq!(part_group["span"]["start_line"], 3);
        assert!(
            part_group["diagnostics"].as_array().unwrap().iter().any(|d| d["message"].as_str().unwrap().contains("retrying on the mesh kernel")),
            "{part_group}"
        );
        // Overrides and body ids are the request's, like Render's.
        let out = call_json("Inspect", &json!({"body_ids": [part["body_id"]], "param_overrides": {}}), &host).unwrap();
        assert_eq!(out["bodies"].as_array().unwrap().len(), 1);
        let e = call_json("Inspect", &json!({"body_ids": ["nope#1"]}), &host).unwrap_err().to_string();
        assert!(e.contains("nope#1"), "{e}");
        // The schema an assistant sees: the document is the front-end's.
        let s = json_schema("InspectRequest").unwrap();
        let props = s["properties"].as_object().unwrap();
        assert!(!props.contains_key("document"));
        assert_eq!(props["body_ids"]["type"], "array");
        assert_eq!(props["param_overrides"]["type"], "object");
        let m = method("Inspect").unwrap();
        assert!(m.idempotent && !m.needs_session);
        assert_eq!(m.cli_name, "inspect");
        assert!(m.doc.starts_with("Inspect the geometry a document produces"), "{:?}", m.doc);
    }

    /// `ExplainRef` is an engine operation: it needs a document, not a
    /// session, so it works from `cadpole op --doc` and from a session alike.
    #[test]
    fn explain_ref_answers_what_made_this_face() {
        let doc = cadpole_engine::document_from_scripts(
            "t",
            vec![(
                "main.star".into(),
                "base = extrude(sketch(XY).rect(40, 20), 10, name=\"base\")\n\
                 hole = extrude(sketch(XY).circle(3), 12, name=\"hole\")\n\
                 part = subtract(base, hole, name=\"part\")\n"
                    .into(),
            )],
        );
        let host = DocumentHost(doc);
        // A generated name, not the `face:N` compatibility
        // form: that one resolves through the fingerprint and so reports
        // MOVED, which is correct but not what this test is about.
        let out = call_json("ExplainRef", &json!({"feature_id": "part", "topo_name": "base/end"}), &host).unwrap();
        assert_eq!(out["status"], "REF_STATUS_OK", "{out}");
        let chain = out["chain"].as_array().expect("a chain");
        assert_eq!(chain[0]["feature_id"], "part");
        assert_eq!(chain[0]["role"], "new");
        assert!(chain.iter().any(|p| p["kind"] == "sketch"), "it reaches the sketch: {out}");
        assert!(chain.iter().all(|p| p["span"]["path"] == "main.star"), "every step has a span: {out}");
        assert_eq!(out["generator"]["sketch_id"], chain.last().unwrap()["feature_id"]);
        assert!(out["summary"].as_str().unwrap().contains("subtract"), "{out}");
        assert!(out["contributors"].as_array().unwrap().iter().any(|c| c == "hole"), "{out}");
    }

    /// The browser sends its scripts and overrides and never an asset
    /// (remote-engine.ts), so a request document that arrives with no asset
    /// table is evaluated against the host's. One that brings its own keeps
    /// it; a host with none leaves the request as it came.
    #[test]
    fn a_document_without_assets_is_evaluated_against_the_hosts() {
        let stl = b"solid t\nfacet normal 0 0 1\nouter loop\nvertex 0 0 0\nvertex 1 0 0\nvertex 0 1 0\nendloop\nendfacet\nendsolid t\n";
        let script = "t = import_mesh(\"tri.stl\", name=\"t\")\n";
        let mut doc = cadpole_engine::document_from_scripts("t", vec![("main.star".into(), script.into())]);
        cadpole_engine::add_asset(&mut doc, "tri.stl", stl.to_vec(), "").unwrap();
        let host = DocumentHost(doc);
        let errors = |out: &serde_json::Value| -> Vec<String> {
            out["model"]["diagnostics"]
                .as_array()
                .map(|d| d.iter().filter(|d| d["severity"] == "SEVERITY_ERROR").map(|d| d["message"].to_string()).collect())
                .unwrap_or_default()
        };
        // What the browser sends: the scripts, no assets.
        let req = json!({"document": {"scripts": [{"path": "main.star", "source": script}]}, "skip_mesh": true});
        let out = call_json("Evaluate", &req, &host).unwrap();
        assert!(errors(&out).is_empty(), "the host's asset table was used: {out}");
        assert_eq!(out["model"]["bodies"].as_array().map(|b| b.len()), Some(1), "{out}");
        // A host with no document: the request is evaluated as it came.
        let out = call_json("Evaluate", &req, &NoHost).unwrap();
        assert!(errors(&out).iter().any(|e| e.contains("no asset named")), "{out}");
    }

    #[test]
    fn the_asset_operations_read_the_documents_table() {
        let mut doc = cadpole_engine::document_from_scripts("t", vec![("main.star".into(), "b = box(1,1,1)".into())]);
        cadpole_engine::add_asset(&mut doc, "plan.dxf", b"0\nSECTION\n2\nHEADER\n".to_vec(), "").unwrap();
        let host = DocumentHost(doc);

        let out = call_json("ListAssets", &json!({}), &host).unwrap();
        assert_eq!(out["assets"][0]["name"], "plan.dxf");
        assert_eq!(out["assets"][0]["kind"], "ASSET_KIND_DXF", "the bytes decide the kind: {out}");
        assert!(out["assets"][0].get("data").is_none(), "a listing never carries bytes: {out}");
        assert_eq!(out["total_bytes"], 19);

        let out = call_json("GetAsset", &json!({"name": "plan.dxf"}), &host).unwrap();
        assert!(out["data"].as_str().unwrap().len() > 8, "the bytes come back base64: {out}");
        let out = call_json("GetAsset", &json!({"name": "plan.dxf", "metadata_only": true}), &host).unwrap();
        assert!(out.get("data").is_none(), "{out}");

        let e = call_json("GetAsset", &json!({"name": "nope.dxf"}), &host).unwrap_err().to_string();
        assert!(e.contains("plan.dxf"), "the error names the assets there are: {e}");
        let e = call_json("GetAsset", &json!({"name": "../escape"}), &host).unwrap_err().to_string();
        assert!(e.contains("not paths"), "an asset name may not be a path: {e}");
    }

    /// The new operations publish schemas an assistant can actually fill in.
    #[test]
    fn the_new_operations_publish_usable_schemas() {
        let s = json_schema("EditScriptRequest").unwrap();
        let props = s["properties"].as_object().unwrap();
        assert_eq!(props["edits"]["type"], "array");
        assert_eq!(props["edits"]["items"]["properties"]["old_str"]["type"], "string");
        assert!(s["required"].as_array().unwrap().iter().any(|r| r == "base_revision"), "{s}");
        // The proto comment is the documentation, verbatim, line breaks and all.
        let old_str_doc = props["edits"]["items"]["properties"]["old_str"]["description"].as_str().unwrap();
        assert!(old_str_doc.contains("unless `replace_all`"), "{old_str_doc:?}");
        assert!(old_str_doc.contains("nothing is written"), "{old_str_doc:?}");

        let s = json_schema("ExplainRefRequest").unwrap();
        assert!(!s["properties"].as_object().unwrap().contains_key("document"), "the front-end supplies it");
        assert_eq!(s["properties"]["feature_id"]["examples"][0], "lid");

        let s = json_schema("AckRequest").unwrap();
        assert!(s["required"].as_array().unwrap().iter().any(|r| r == "client"));

        let attachment = method("GetAttachment").unwrap();
        assert_eq!(attachment.image_field, "png", "the PNG becomes an image block, never base64 in text");
        assert!(attachment.needs_session && attachment.idempotent);
        assert!(method("ExplainRef").unwrap().idempotent && !method("ExplainRef").unwrap().needs_session);
        assert!(!method("EditScript").unwrap().idempotent, "an edit is not safe to retry blindly");
    }

    #[test]
    fn struct_conversion_keeps_shape() {
        let s = json_to_struct(&json!({"path": "main.star", "revision": 8, "tags": ["a"], "deep": {"x": true}}));
        let msg = DynamicMessage::decode(
            pool().get_message_by_name("google.protobuf.Struct").unwrap(),
            s.encode_to_vec().as_slice(),
        )
        .unwrap();
        let back: Value = msg.serialize_with_options(serde_json::value::Serializer, &serialize_options()).unwrap();
        assert_eq!(back, json!({"path": "main.star", "revision": 8.0, "tags": ["a"], "deep": {"x": true}}));
    }
}
