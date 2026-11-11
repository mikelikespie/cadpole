//! The Connect protocol, unary, written out by hand.
//!
//! Cadpole speaks binary protobuf to the browser. Its original transport was
//! a convention of its own: `POST /api/ops/<Rpc>`, `application/x-protobuf`,
//! and an error body that was whatever `anyhow` had to say, so every new
//! client had to be told about it. That route remains as a fallback.
//!
//! This module is the same traffic under a specification other people
//! implement — <https://connectrpc.com/docs/protocol> — so `buf curl`, a
//! `connect-go` client or anything else that speaks it can call `cadpole serve`
//! without being told anything except the URL. It is deliberately small,
//! because the unary half of Connect is small:
//!
//! * `POST <base>/<package>.<Service>/<Method>`;
//! * `Content-Type: application/proto` or `application/json`, and the response
//!   is encoded the same way;
//! * a success is the response message, raw, with HTTP 200;
//! * an error is JSON `{"code": "...", "message": "..."}` with the HTTP status
//!   that Connect assigns to that code;
//! * `Connect-Protocol-Version: 1` and `Connect-Timeout-Ms` on the request.
//!
//! It also frames **gRPC-Web** (unary), which is the same dispatch with a
//! different envelope: a five-byte length prefix on each message and the
//! status folded into a trailing frame instead of the HTTP status. That is
//! what lets a browser speak a gRPC dialect at all, since `fetch` can neither
//! send nor read HTTP trailers. Full gRPC over HTTP/2 is *not* served, mostly
//! because its clients
//! cannot be given a URL prefix and would need a route outside `/api/`, where
//! this server's authentication and capability guard do not run.
//!
//! **What is not here.** Streaming (Cadpole has no streaming rpc), compression
//! (`identity` only, which the protocol permits), and the Connect GET form for
//! side-effect-free methods — that one is a deliberate refusal: a GET with the
//! message in the query string is a *simple* cross-origin request, which is
//! exactly the confused-deputy shape `security::check_origin` exists to stop.
//!
//! The base URL is `/api`, so every Connect path is `/api/cadpole.v1.CadpoleOps/…`.
//! That is a plain configuration choice a Connect client makes (`baseUrl`),
//! and it buys a security property worth more than the four characters:
//! `guard()` authenticates everything under `/api/`, so the new transport is
//! behind the same door as the old one rather than beside it.

use axum::http::{HeaderMap, StatusCode, header};
use axum::response::{IntoResponse, Response};

/// The protocol version this server implements, as `Connect-Protocol-Version`.
pub const PROTOCOL_VERSION: &str = "1";

/// Header names the protocol defines. Lower case: `HeaderMap` is ASCII-case
/// insensitive on lookup, and these are also compared against by name.
pub const VERSION_HEADER: &str = "connect-protocol-version";
pub const TIMEOUT_HEADER: &str = "connect-timeout-ms";

/// The URL prefix Connect clients are given as their base URL.
///
/// Everything under `/api/` is authenticated and origin-checked by `guard()`.
/// Mounting the service here rather than at the root means the Connect
/// transport cannot become a second, unguarded door — and costs a Connect
/// client nothing, because a base URL is a normal thing to configure.
pub const BASE_URL: &str = "/api";

/// `/api/cadpole.v1.CadpoleOps` — the service prefix every method hangs off.
///
/// Derived from the descriptor, not typed out: renaming the service in
/// `ops.proto` moves the route and the guard together.
pub fn service_prefix() -> &'static str {
    static PREFIX: std::sync::OnceLock<String> = std::sync::OnceLock::new();
    PREFIX.get_or_init(|| format!("{BASE_URL}/{}", cadpole_ops::SERVICE))
}

/// The full path of one method, e.g. `/api/cadpole.v1.CadpoleOps/Evaluate`.
pub fn method_path(method: &str) -> String {
    format!("{}/{method}", service_prefix())
}

/// The axum route pattern for the service.
pub fn route_pattern() -> String {
    format!("{}/{{method}}", service_prefix())
}

/// The method named by a request path, if it is one of ours. Exact rpc names
/// only — see [`lookup`].
pub fn method_of(path: &str) -> Option<&str> {
    let rest = path.strip_prefix(service_prefix())?.strip_prefix('/')?;
    if rest.is_empty() || rest.contains('/') { None } else { Some(rest) }
}

/// Resolve a method name from a Connect path to an operation.
///
/// **Exact rpc names only.** The `/api/ops/<Rpc>` route accepts any spelling
/// `MethodInfo::matches` takes — `eval`, `evaluate`, `EVALUATE` — because a
/// human types those. Connect names a method the way the schema does, and
/// every accepted spelling is another thing the capability model has to reason
/// about, so this route takes one name per operation and nothing else.
pub fn lookup(method: &str) -> Option<&'static cadpole_ops::MethodInfo> {
    cadpole_ops::registry().iter().find(|m| m.name == method)
}

/// How a message is encoded on the wire.
#[derive(Copy, Clone, Debug, PartialEq, Eq)]
pub enum Codec {
    /// `application/proto`: the binary wire format.
    Proto,
    /// `application/json`: the canonical proto3 JSON mapping.
    Json,
}

/// How a message is delimited, and where the status goes.
#[derive(Copy, Clone, Debug, PartialEq, Eq)]
pub enum Framing {
    /// Connect unary: the body *is* the message; the status is the HTTP status
    /// and an error is a JSON envelope.
    Connect,
    /// gRPC-Web unary: each message carries a five-byte prefix and the status
    /// arrives in a trailing frame, because a browser cannot read trailers.
    GrpcWeb,
}

/// The framing and codec one request asked for.
#[derive(Copy, Clone, Debug, PartialEq, Eq)]
pub struct Wire {
    pub framing: Framing,
    pub codec: Codec,
}

impl Wire {
    /// The `Content-Type` a response in this wire carries.
    pub fn content_type(self) -> &'static str {
        match (self.framing, self.codec) {
            (Framing::Connect, Codec::Proto) => "application/proto",
            (Framing::Connect, Codec::Json) => "application/json",
            (Framing::GrpcWeb, Codec::Proto) => "application/grpc-web+proto",
            (Framing::GrpcWeb, Codec::Json) => "application/grpc-web+json",
        }
    }
}

/// The wire a `Content-Type` asks for.
///
/// `application/x-protobuf` and `application/protobuf` are accepted as
/// synonyms for `application/proto` so the pre-Connect clients that already
/// send them keep working on the new path; a Connect client sends
/// `application/proto` and gets it back.
pub fn wire_of(content_type: &str) -> Result<Wire, Failure> {
    let ct = content_type.split(';').next().unwrap_or("").trim().to_ascii_lowercase();
    let connect = |codec| Ok(Wire { framing: Framing::Connect, codec });
    match ct.as_str() {
        "application/proto" | "application/x-protobuf" | "application/protobuf" | "application/octet-stream" => {
            connect(Codec::Proto)
        }
        "application/json" => connect(Codec::Json),
        "application/grpc-web" | "application/grpc-web+proto" => {
            Ok(Wire { framing: Framing::GrpcWeb, codec: Codec::Proto })
        }
        "application/grpc-web+json" => Ok(Wire { framing: Framing::GrpcWeb, codec: Codec::Json }),
        // Named explicitly so the answer is a sentence rather than a shrug:
        // gRPC proper needs HTTP/2 trailers and a route outside `/api/`.
        "application/grpc" | "application/grpc+proto" | "application/grpc+json" => Err(Failure::new(
            Code::Unimplemented,
            "cadpole serve does not speak gRPC over HTTP/2; use the Connect framing \
             (application/proto) or gRPC-Web (application/grpc-web+proto) at this URL",
        )),
        "" => connect(Codec::Proto),
        other => Err(Failure::new(
            Code::Unimplemented,
            format!("unsupported content type {other:?}: use application/proto, application/json or application/grpc-web+proto"),
        )),
    }
}

/// The wire a request asks for, for code that only has the headers.
pub fn wire_of_headers(headers: &HeaderMap) -> Wire {
    headers
        .get(header::CONTENT_TYPE)
        .and_then(|v| v.to_str().ok())
        .and_then(|ct| wire_of(ct).ok())
        .unwrap_or(Wire { framing: Framing::Connect, codec: Codec::Proto })
}

/// The error codes of the Connect protocol, which are gRPC's.
///
/// All sixteen, including the ones this server never produces: the set is part
/// of the protocol, a caller may be shown any of them by a proxy in between,
/// and `of_status` has to be total. A partial enum would be a dialect.
#[derive(Copy, Clone, Debug, PartialEq, Eq)]
#[allow(dead_code)]
pub enum Code {
    Canceled,
    Unknown,
    InvalidArgument,
    DeadlineExceeded,
    NotFound,
    AlreadyExists,
    PermissionDenied,
    ResourceExhausted,
    FailedPrecondition,
    Aborted,
    OutOfRange,
    Unimplemented,
    Internal,
    Unavailable,
    DataLoss,
    Unauthenticated,
}

impl Code {
    /// The name as it appears in a Connect error envelope.
    pub fn as_str(self) -> &'static str {
        match self {
            Code::Canceled => "canceled",
            Code::Unknown => "unknown",
            Code::InvalidArgument => "invalid_argument",
            Code::DeadlineExceeded => "deadline_exceeded",
            Code::NotFound => "not_found",
            Code::AlreadyExists => "already_exists",
            Code::PermissionDenied => "permission_denied",
            Code::ResourceExhausted => "resource_exhausted",
            Code::FailedPrecondition => "failed_precondition",
            Code::Aborted => "aborted",
            Code::OutOfRange => "out_of_range",
            Code::Unimplemented => "unimplemented",
            Code::Internal => "internal",
            Code::Unavailable => "unavailable",
            Code::DataLoss => "data_loss",
            Code::Unauthenticated => "unauthenticated",
        }
    }

    /// The HTTP status Connect assigns to this code.
    pub fn http_status(self) -> StatusCode {
        match self {
            // 499 is nginx's, and the one the Connect specification names.
            Code::Canceled => StatusCode::from_u16(499).expect("499 is a valid status"),
            Code::Unknown | Code::Internal | Code::DataLoss => StatusCode::INTERNAL_SERVER_ERROR,
            Code::InvalidArgument | Code::OutOfRange => StatusCode::BAD_REQUEST,
            Code::DeadlineExceeded => StatusCode::GATEWAY_TIMEOUT,
            Code::NotFound => StatusCode::NOT_FOUND,
            Code::AlreadyExists | Code::Aborted => StatusCode::CONFLICT,
            Code::PermissionDenied => StatusCode::FORBIDDEN,
            Code::ResourceExhausted => StatusCode::TOO_MANY_REQUESTS,
            Code::FailedPrecondition => StatusCode::PRECONDITION_FAILED,
            Code::Unimplemented => StatusCode::NOT_IMPLEMENTED,
            Code::Unavailable => StatusCode::SERVICE_UNAVAILABLE,
            Code::Unauthenticated => StatusCode::UNAUTHORIZED,
        }
    }

    /// The numeric gRPC status, for a gRPC-Web trailer.
    pub fn grpc_status(self) -> u8 {
        match self {
            Code::Canceled => 1,
            Code::Unknown => 2,
            Code::InvalidArgument => 3,
            Code::DeadlineExceeded => 4,
            Code::NotFound => 5,
            Code::AlreadyExists => 6,
            Code::PermissionDenied => 7,
            Code::ResourceExhausted => 8,
            Code::FailedPrecondition => 9,
            Code::Aborted => 10,
            Code::OutOfRange => 11,
            Code::Unimplemented => 12,
            Code::Internal => 13,
            Code::Unavailable => 14,
            Code::DataLoss => 15,
            Code::Unauthenticated => 16,
        }
    }

    /// The code a status from elsewhere in the server means. Used where an
    /// existing layer (the render queue, the body limit) already decided on an
    /// HTTP status and the Connect answer has to agree with it.
    pub fn of_status(status: StatusCode) -> Code {
        match status {
            StatusCode::BAD_REQUEST => Code::InvalidArgument,
            StatusCode::UNAUTHORIZED => Code::Unauthenticated,
            StatusCode::FORBIDDEN => Code::PermissionDenied,
            StatusCode::NOT_FOUND => Code::Unimplemented,
            StatusCode::CONFLICT => Code::Aborted,
            StatusCode::PRECONDITION_FAILED => Code::FailedPrecondition,
            StatusCode::PAYLOAD_TOO_LARGE | StatusCode::TOO_MANY_REQUESTS => Code::ResourceExhausted,
            StatusCode::NOT_IMPLEMENTED => Code::Unimplemented,
            StatusCode::SERVICE_UNAVAILABLE => Code::Unavailable,
            StatusCode::GATEWAY_TIMEOUT => Code::DeadlineExceeded,
            _ => Code::Unknown,
        }
    }
}

/// One failed call: a code and a message, which is the whole of a Connect
/// error until details are needed (Cadpole sends none).
#[derive(Clone, Debug)]
pub struct Failure {
    pub code: Code,
    pub message: String,
}

impl Failure {
    pub fn new(code: Code, message: impl Into<String>) -> Self {
        Failure { code, message: message.into() }
    }

    /// The JSON envelope, which is the same in every framing that has one.
    pub fn envelope(&self) -> Vec<u8> {
        serde_json::to_vec(&serde_json::json!({"code": self.code.as_str(), "message": self.message}))
            .unwrap_or_else(|_| br#"{"code":"internal","message":"error"}"#.to_vec())
    }
}

/// What an operation's own error means in Connect's vocabulary.
///
/// Cadpole's handlers return `anyhow::Error`, which carries no code, and the
/// `/api/ops` route answered every one of them with 400. That was right for
/// most of them and wrong for two shapes that a caller can act on, so those
/// two are recognised here by the text their constructors produce:
///
/// * "needs a running `cadpole serve`" — a *session* operation called against a
///   front-end that has no session. The request was fine; the server is not in
///   a state to answer it, which is `failed_precondition`.
/// * "unknown operation" — the registry has never heard of it: `unimplemented`.
///
/// Everything else stays `invalid_argument`, which is what 400 already said.
/// The alternative — threading a code through every `bail!` in `cadpole-ops` — is
/// a change to the crate that holds the operations for the benefit of the one
/// that holds the transport, and this is the layer that knows about HTTP.
pub fn classify(err: &anyhow::Error) -> Code {
    let text = format!("{err:#}");
    if text.contains("needs a running `cadpole serve`") || text.contains("no document:") {
        Code::FailedPrecondition
    } else if text.contains("unknown operation") {
        Code::Unimplemented
    } else {
        Code::InvalidArgument
    }
}

// ---------------------------------------------------------------------------
// Framing
// ---------------------------------------------------------------------------

/// The five-byte gRPC-Web prefix: one flag byte, then a big-endian length.
const FRAME_HEADER: usize = 5;
/// The flag bit that marks a frame as trailers rather than a message.
const TRAILER_FLAG: u8 = 0x80;
/// The flag bit that marks a frame as compressed. We never set it, and a
/// request that does is refused rather than silently mis-read.
const COMPRESSED_FLAG: u8 = 0x01;

/// Take one gRPC-Web message frame off the front of a request body.
pub fn unframe(body: &[u8]) -> Result<&[u8], Failure> {
    if body.len() < FRAME_HEADER {
        return Err(Failure::new(Code::InvalidArgument, "truncated gRPC-Web frame: fewer than five bytes"));
    }
    let flags = body[0];
    if flags & COMPRESSED_FLAG != 0 {
        return Err(Failure::new(
            Code::Unimplemented,
            "compressed gRPC-Web frames are not supported; send identity-encoded messages",
        ));
    }
    if flags & TRAILER_FLAG != 0 {
        return Err(Failure::new(Code::InvalidArgument, "expected a message frame, got a trailers frame"));
    }
    let len = u32::from_be_bytes([body[1], body[2], body[3], body[4]]) as usize;
    let end = FRAME_HEADER + len;
    if body.len() < end {
        return Err(Failure::new(
            Code::InvalidArgument,
            format!("gRPC-Web frame says {len} bytes but the body has {}", body.len() - FRAME_HEADER),
        ));
    }
    Ok(&body[FRAME_HEADER..end])
}

/// Put one frame on the wire: flags, big-endian length, payload.
fn frame(flags: u8, payload: &[u8]) -> Vec<u8> {
    let mut out = Vec::with_capacity(FRAME_HEADER + payload.len());
    out.push(flags);
    out.extend_from_slice(&(payload.len() as u32).to_be_bytes());
    out.extend_from_slice(payload);
    out
}

/// `grpc-message` is percent-encoded: anything outside printable ASCII, and
/// `%` itself, becomes `%XX` (gRPC's own rule, which is not RFC 3986's).
fn percent_encode(message: &str) -> String {
    let mut out = String::with_capacity(message.len());
    for b in message.as_bytes() {
        match b {
            0x20..=0x24 | 0x26..=0x7e => out.push(*b as char),
            _ => out.push_str(&format!("%{b:02X}")),
        }
    }
    out
}

/// The trailing frame of a gRPC-Web response.
fn trailers(code: Code, message: &str) -> Vec<u8> {
    let mut text = format!("grpc-status: {}\r\n", if code.grpc_status() == 0 { 0 } else { code.grpc_status() });
    if !message.is_empty() {
        text.push_str(&format!("grpc-message: {}\r\n", percent_encode(message)));
    }
    frame(TRAILER_FLAG, text.as_bytes())
}

/// A successful response in the requested wire.
pub fn ok_response(wire: Wire, body: Vec<u8>) -> Response {
    match wire.framing {
        Framing::Connect => ([(header::CONTENT_TYPE, wire.content_type())], body).into_response(),
        Framing::GrpcWeb => {
            let mut out = frame(0, &body);
            // grpc-status 0 in the trailing frame. `Code::Unknown` is never
            // sent here: the helper takes the numeric zero path.
            out.extend_from_slice(&frame(TRAILER_FLAG, b"grpc-status: 0\r\n"));
            (
                [
                    (header::CONTENT_TYPE, wire.content_type()),
                    (header::HeaderName::from_static("grpc-encoding"), "identity"),
                ],
                out,
            )
                .into_response()
        }
    }
}

/// A failed response in the requested wire.
///
/// Connect puts the status in HTTP, where a browser and `curl` can see it.
/// gRPC-Web answers 200 and puts the status in the trailing frame, because
/// that is what its clients read; a non-200 would work too, but only by way of
/// the status-code fallback, and this is the shape the specification names.
pub fn error_response(wire: Wire, failure: &Failure) -> Response {
    match wire.framing {
        Framing::Connect => (
            failure.code.http_status(),
            [(header::CONTENT_TYPE, "application/json")],
            failure.envelope(),
        )
            .into_response(),
        Framing::GrpcWeb => (
            StatusCode::OK,
            [(header::CONTENT_TYPE, wire.content_type())],
            trailers(failure.code, &failure.message),
        )
            .into_response(),
    }
}

/// `Connect-Timeout-Ms`, when the caller set one.
///
/// A malformed value is an error rather than an ignored header: a client that
/// asked for a deadline and silently did not get one is worse off than one
/// that was told.
pub fn timeout_of(headers: &HeaderMap) -> Result<Option<std::time::Duration>, Failure> {
    let Some(raw) = headers.get(TIMEOUT_HEADER) else { return Ok(None) };
    let text = raw.to_str().unwrap_or("");
    match text.parse::<u64>() {
        // The protocol caps the header at ten digits.
        Ok(ms) if text.len() <= 10 => Ok(Some(std::time::Duration::from_millis(ms))),
        _ => Err(Failure::new(
            Code::InvalidArgument,
            format!("{TIMEOUT_HEADER} must be up to ten digits of milliseconds, got {text:?}"),
        )),
    }
}

/// Reject a `Connect-Protocol-Version` that is not this one.
///
/// The header is validated when present and not required when absent. Connect
/// lets a server require it, as a defence against a cross-origin form post
/// reaching an rpc — but a form can only send `text/plain`,
/// `application/x-www-form-urlencoded` or `multipart/form-data`, none of which
/// this handler accepts, and `check_origin` refuses a cross-site request
/// before any of this runs. Requiring it would buy nothing and would turn a
/// conforming client that omits it into a 400.
pub fn check_version(headers: &HeaderMap) -> Result<(), Failure> {
    match headers.get(VERSION_HEADER).and_then(|v| v.to_str().ok()) {
        None => Ok(()),
        Some(PROTOCOL_VERSION) => Ok(()),
        Some(other) => Err(Failure::new(
            Code::InvalidArgument,
            format!("{VERSION_HEADER} must be {PROTOCOL_VERSION}, got {other:?}"),
        )),
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn the_service_path_comes_from_the_descriptor() {
        assert_eq!(service_prefix(), "/api/cadpole.v1.CadpoleOps");
        assert_eq!(method_path("Evaluate"), "/api/cadpole.v1.CadpoleOps/Evaluate");
        assert_eq!(method_of("/api/cadpole.v1.CadpoleOps/Evaluate"), Some("Evaluate"));
        assert_eq!(method_of("/api/cadpole.v1.CadpoleOps/"), None);
        assert_eq!(method_of("/api/cadpole.v1.CadpoleOps/a/b"), None);
        assert_eq!(method_of("/api/ops/Evaluate"), None);
    }

    /// The Connect route takes the schema's spelling and no other, which is
    /// the property the capability model leans on.
    #[test]
    fn only_exact_rpc_names_resolve_on_the_connect_route() {
        assert!(lookup("Evaluate").is_some());
        assert!(lookup("SetScript").is_some());
        assert!(lookup("eval").is_none(), "a CLI name is not a Connect method");
        assert!(lookup("evaluate").is_none(), "an alias is not a Connect method");
        assert!(lookup("EVALUATE").is_none(), "case matters here");
        assert!(lookup("cadpole.v1.SetScript").is_none(), "the path already named the service");
    }

    #[test]
    fn content_types_map_to_framings() {
        let w = |ct: &str| wire_of(ct).unwrap();
        assert_eq!(w("application/proto"), Wire { framing: Framing::Connect, codec: Codec::Proto });
        assert_eq!(w("application/json; charset=utf-8"), Wire { framing: Framing::Connect, codec: Codec::Json });
        assert_eq!(w("application/x-protobuf"), Wire { framing: Framing::Connect, codec: Codec::Proto });
        assert_eq!(w("application/grpc-web+proto"), Wire { framing: Framing::GrpcWeb, codec: Codec::Proto });
        assert_eq!(w("application/grpc-web"), Wire { framing: Framing::GrpcWeb, codec: Codec::Proto });
        assert_eq!(wire_of("application/grpc").unwrap_err().code, Code::Unimplemented);
        assert_eq!(wire_of("text/plain").unwrap_err().code, Code::Unimplemented);
    }

    #[test]
    fn codes_carry_the_statuses_the_specification_assigns() {
        assert_eq!(Code::InvalidArgument.http_status(), StatusCode::BAD_REQUEST);
        assert_eq!(Code::Unauthenticated.http_status(), StatusCode::UNAUTHORIZED);
        assert_eq!(Code::PermissionDenied.http_status(), StatusCode::FORBIDDEN);
        assert_eq!(Code::Unimplemented.http_status(), StatusCode::NOT_IMPLEMENTED);
        assert_eq!(Code::ResourceExhausted.http_status(), StatusCode::TOO_MANY_REQUESTS);
        assert_eq!(Code::DeadlineExceeded.http_status(), StatusCode::GATEWAY_TIMEOUT);
        assert_eq!(Code::FailedPrecondition.http_status(), StatusCode::PRECONDITION_FAILED);
        assert_eq!(Code::PermissionDenied.grpc_status(), 7);
        assert_eq!(Code::Unauthenticated.grpc_status(), 16);
        assert_eq!(Code::InvalidArgument.as_str(), "invalid_argument");
    }

    #[test]
    fn an_error_is_the_envelope_the_protocol_names() {
        let f = Failure::new(Code::PermissionDenied, "a listener token may not call SetScript");
        let v: serde_json::Value = serde_json::from_slice(&f.envelope()).unwrap();
        assert_eq!(v["code"], "permission_denied");
        assert_eq!(v["message"], "a listener token may not call SetScript");
    }

    #[test]
    fn frames_round_trip() {
        let body = frame(0, b"hello");
        assert_eq!(body, vec![0, 0, 0, 0, 5, b'h', b'e', b'l', b'l', b'o']);
        assert_eq!(unframe(&body).unwrap(), b"hello");
        assert_eq!(unframe(&frame(0, b"")).unwrap(), b"");
        assert!(unframe(&[0, 0, 0]).is_err(), "a short frame is an error, not an empty message");
        assert!(unframe(&[0, 0, 0, 0, 9, 1, 2]).is_err(), "a frame longer than the body is an error");
        assert_eq!(unframe(&[COMPRESSED_FLAG, 0, 0, 0, 0]).unwrap_err().code, Code::Unimplemented);
    }

    #[test]
    fn trailers_carry_the_status_and_a_percent_encoded_message() {
        let t = trailers(Code::PermissionDenied, "no: \"bad\"\nreason");
        let text = String::from_utf8(t[FRAME_HEADER..].to_vec()).unwrap();
        assert!(text.starts_with("grpc-status: 7\r\n"), "{text}");
        assert!(text.contains("grpc-message: no: \"bad\"%0Areason"), "{text}");
        assert_eq!(t[0], TRAILER_FLAG);
        assert_eq!(u32::from_be_bytes([t[1], t[2], t[3], t[4]]) as usize, t.len() - FRAME_HEADER);
    }

    #[test]
    fn a_percent_sign_is_itself_encoded() {
        assert_eq!(percent_encode("100% sure"), "100%25 sure");
        assert_eq!(percent_encode("plain"), "plain");
    }

    #[test]
    fn the_protocol_version_is_checked_when_present() {
        let mut h = HeaderMap::new();
        assert!(check_version(&h).is_ok(), "absent is fine");
        h.insert(VERSION_HEADER, "1".parse().unwrap());
        assert!(check_version(&h).is_ok());
        h.insert(VERSION_HEADER, "2".parse().unwrap());
        assert_eq!(check_version(&h).unwrap_err().code, Code::InvalidArgument);
    }

    #[test]
    fn a_timeout_header_is_parsed_or_refused() {
        let mut h = HeaderMap::new();
        assert_eq!(timeout_of(&h).unwrap(), None);
        h.insert(TIMEOUT_HEADER, "250".parse().unwrap());
        assert_eq!(timeout_of(&h).unwrap(), Some(std::time::Duration::from_millis(250)));
        h.insert(TIMEOUT_HEADER, "soon".parse().unwrap());
        assert!(timeout_of(&h).is_err());
        h.insert(TIMEOUT_HEADER, "12345678901".parse().unwrap());
        assert!(timeout_of(&h).is_err(), "the protocol caps the header at ten digits");
    }

    #[test]
    fn a_session_operation_without_a_session_is_a_precondition_not_a_bad_request() {
        let e = anyhow::anyhow!("GetSession needs a running `cadpole serve`: pass --server URL (or set CADPOLE_SERVER)");
        assert_eq!(classify(&e), Code::FailedPrecondition);
        assert_eq!(classify(&anyhow::anyhow!("no such asset \"logo\"")), Code::InvalidArgument);
        assert_eq!(classify(&anyhow::anyhow!("unknown operation \"Nope\"")), Code::Unimplemented);
    }
}
