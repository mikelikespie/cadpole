// The one protocol every screen is written against (docs/apple-apps.md).
//
// Two implementations: `RemoteEngine` (the session HTTP/SSE API, both
// platforms) and `LocalEngine` (`cadpole_call` over the C ABI, macOS only, in the
// CadpoleLocalEngine module because it links Rust). Nothing above this line knows
// which one it has.
//
// The protocol is deliberately *small*: one `call` over the
// `cadpole.v1.Request`/`cadpole.v1.Response` envelope, plus the session state a
// server owns and an in-process engine has to synthesise. Evaluate, render and
// api-doc are then default implementations shared by both engines, which is
// what makes them mechanically equivalent rather than two clients that drift.

import Foundation
import CadpoleProto

/// What an engine can do. `LocalEngine` and `RemoteEngine` differ here and
/// nowhere else, so the UI disables a control *with a reason* rather than
/// silently greying it out.
public struct EngineCapabilities: OptionSet, Sendable, Hashable {
    public let rawValue: Int
    public init(rawValue: Int) { self.rawValue = rawValue }

    /// No network needed to see a model change.
    public static let localEvaluation = EngineCapabilities(rawValue: 1 << 0)
    public static let pathTrace = EngineCapabilities(rawValue: 1 << 1)
    /// Needs truck; never in a renderer-only build.
    public static let stepExport = EngineCapabilities(rawValue: 1 << 2)
    /// messages/listen require a session server, so self-hosted macOS lacks it.
    public static let assistant = EngineCapabilities(rawValue: 1 << 3)
    public static let assets = EngineCapabilities(rawValue: 1 << 4)
    /// Cancellable `/api/jobs`. No server implements it yet.
    public static let backgroundJobs = EngineCapabilities(rawValue: 1 << 5)
}

/// The toolbar pill's state machine (docs/apple-apps.md).
public enum ConnectionState: Sendable, Equatable {
    case disconnected
    case resolving
    case connecting
    case live(revision: UInt64)
    case degraded(since: Date)
    case unauthorized
}

public enum EngineError: Error, Sendable, Equatable {
    /// The server said no. 401 means the token rotated: clear it and ask for a
    /// new URL rather than retrying.
    case http(status: Int, body: String)
    /// `Response.error`: the request could not be processed at all. Script
    /// errors are never this -- they are diagnostics inside a good response.
    case engine(String)
    /// A well-formed response of the wrong kind for the request.
    case unexpectedResponse
    case decoding(String)
    /// Optimistic concurrency lost; open a diff sheet, never auto-merge.
    case staleRevision(current: UInt64)
    /// The evaluation deadline passed, or a newer request superseded this one.
    case cancelled
    /// The platform or the engine build cannot do this.
    case unsupported(String)
}

/// A session as this client understands it: the document plus everything
/// transient around it. `RemoteEngine` decodes the server's JSON session view;
/// `LocalEngine` synthesises the same shape from its own state.
public struct SessionSnapshot: @unchecked Sendable, Equatable {
    public var id: String
    public var revision: UInt64
    public var documentName: String
    public var displayUnit: String
    public var scripts: [Cadpole_V1_ScriptFile]
    public var paramOverrides: [String: Cadpole_V1_ParamValue]
    public var selection: [Cadpole_V1_EntityRef]
    public var lastEventSeq: UInt64

    public init(
        id: String = "",
        revision: UInt64 = 0,
        documentName: String = "",
        displayUnit: String = "mm",
        scripts: [Cadpole_V1_ScriptFile] = [],
        paramOverrides: [String: Cadpole_V1_ParamValue] = [:],
        selection: [Cadpole_V1_EntityRef] = [],
        lastEventSeq: UInt64 = 0
    ) {
        self.id = id
        self.revision = revision
        self.documentName = documentName
        self.displayUnit = displayUnit
        self.scripts = scripts
        self.paramOverrides = paramOverrides
        self.selection = selection
        self.lastEventSeq = lastEventSeq
    }
}

/// One entry of the change stream (`GET /api/events`, or the local log).
public struct EngineEvent: Sendable, Equatable {
    public var seq: UInt64
    public var at: String
    public var kind: String
    /// The event's `data` object, still encoded. Callers that care decode it;
    /// most only need `kind` and `seq`.
    public var dataJSON: Data
    /// The `X-Cadpole-Client` id of the client whose write caused this
    /// event; empty when the server itself did. A client skips its own
    /// writes: it has already applied them.
    public var by: String

    public init(seq: UInt64, at: String = "", kind: String, dataJSON: Data = Data(), by: String = "") {
        self.seq = seq
        self.at = at
        self.kind = kind
        self.dataJSON = dataJSON
        self.by = by
    }

    /// Whether this is `clientID`'s own write coming back.
    public func isEcho(of clientID: String) -> Bool {
        !clientID.isEmpty && by == clientID
    }
}

/// What the viewport asks for. A narrow face of `cadpole.v1.RenderRequest`: the
/// rest of that message is for the CLI and for sweeps.
public struct RenderSpec: Sendable, Equatable {
    public var width: UInt32
    public var height: UInt32
    /// "iso", "front", "top", ... (docs/views.md). Empty means explicit camera.
    public var view: String
    public var drawEdges: Bool
    public var drawGrid: Bool
    /// Draw the session's selection in the highlight colour, and return its
    /// pixel boxes -- which is also what makes the viewport accessible.
    public var highlightSelection: Bool
    /// Fast preview while a slider is moving.
    public var draft: Bool

    public init(
        width: UInt32 = 800,
        height: UInt32 = 600,
        view: String = "iso",
        drawEdges: Bool = true,
        drawGrid: Bool = true,
        highlightSelection: Bool = true,
        draft: Bool = false
    ) {
        self.width = width
        self.height = height
        self.view = view
        self.drawEdges = drawEdges
        self.drawGrid = drawGrid
        self.highlightSelection = highlightSelection
        self.draft = draft
    }
}

public struct RenderResult: @unchecked Sendable {
    public var png: Data
    public var width: UInt32
    public var height: UInt32
    public var diagnostics: [Cadpole_V1_Diagnostic]
    /// The evaluated model (meshes omitted), when the engine returned one.
    public var model: Cadpole_V1_Model?

    public init(
        png: Data,
        width: UInt32,
        height: UInt32,
        diagnostics: [Cadpole_V1_Diagnostic] = [],
        model: Cadpole_V1_Model? = nil
    ) {
        self.png = png
        self.width = width
        self.height = height
        self.diagnostics = diagnostics
        self.model = model
    }
}

public protocol EngineClient: Sendable {
    var capabilities: EngineCapabilities { get }

    /// The id this client's writes are attributed to (`X-Cadpole-Client`),
    /// which is how a screen recognises its own writes coming back on the
    /// event stream (`EngineEvent.isEcho(of:)`). Empty: no attribution, so
    /// nothing is skipped.
    var clientID: String { get }

    /// The session and everything transient around it.
    func session() async throws -> SessionSnapshot

    /// The document to evaluate: scripts, overrides and the current selection.
    /// Both engines build requests from this, so both send the same bytes.
    func document() async throws -> Cadpole_V1_Document

    /// One request over the `cadpole.v1.Request` / `cadpole.v1.Response` envelope.
    /// `RemoteEngine` POSTs it to `/api/call`; `LocalEngine` hands the same
    /// bytes to `cadpole_call`.
    func call(_ request: Cadpole_V1_Request) async throws -> Cadpole_V1_Response

    /// Replace the parameter overrides. Coalesced while a slider is moving
    /// (see `ParamCoalescer`): only the newest value runs.
    @discardableResult
    func setParams(_ overrides: [String: Cadpole_V1_ParamValue]) async throws -> SessionSnapshot

    /// Store the selection. This is what an assistant reads to understand
    /// "the thing I'm pointing at", and what `highlight_selection` draws.
    @discardableResult
    func setSelection(_ refs: [Cadpole_V1_EntityRef]) async throws -> SessionSnapshot

    /// The change stream, resumed after `seq`. A `seq` at or below the stored
    /// cursor means the server restarted: discard and re-read.
    ///
    /// The stream reconnects for as long as the caller listens, so it ends
    /// only when the caller stops or when retrying cannot help (a 401).
    func events(after seq: UInt64) -> AsyncThrowingStream<EngineEvent, any Error>

    /// The connection pill's state (docs/apple-apps.md). Watching it is how
    /// a screen learns that a reconnection succeeded and that it should read
    /// the session again -- a server that restarted while we were away has a
    /// new event log, so the stream itself cannot say what was missed.
    var connection: AsyncStream<ConnectionState> { get }
}

extension EngineClient {
    public var clientID: String { "" }
}

extension EngineClient {
    /// An engine linked in-process has no connection to lose, so it is live
    /// from the start and stays that way. `RemoteEngine` overrides this.
    public var connection: AsyncStream<ConnectionState> {
        AsyncStream { continuation in
            continuation.yield(.live(revision: 0))
            continuation.finish()
        }
    }
}

extension EngineClient {
    /// Evaluate the document into a model: the feature tree, parameters,
    /// bodies and diagnostics behind the whole first screen.
    public func evaluate(
        tolerance: Double = 0,
        skipMesh: Bool = false,
        deadline: Duration? = nil
    ) async throws -> Cadpole_V1_Model {
        var evaluate = Cadpole_V1_EvaluateRequest()
        evaluate.document = try await document()
        evaluate.tolerance = tolerance
        evaluate.skipMesh = skipMesh
        if let deadline {
            evaluate.deadlineMs = UInt64(max(0, deadline.milliseconds))
        }
        var request = Cadpole_V1_Request()
        request.evaluate = evaluate
        let response = try await call(request)
        switch response.response {
        case .evaluate(let r): return r.model
        case .error(let e): throw EngineError.engine(e)
        default: throw EngineError.unexpectedResponse
        }
    }

    /// One image of the current document, plus the diagnostics that produced it.
    public func render(_ spec: RenderSpec) async throws -> RenderResult {
        var render = Cadpole_V1_RenderRequest()
        render.document = try await document()
        render.width = spec.width
        render.height = spec.height
        var camera = Cadpole_V1_Camera()
        camera.preset = spec.view
        render.camera = camera
        render.highlightSelection = spec.highlightSelection
        render.draft = spec.draft
        // proto3 cannot tell "false" from "unset", so the engine reads
        // draw_edges/draw_grid as "defaults on" and needs the explicit
        // negatives to turn either off (see engine.proto's `no_edges`).
        render.drawEdges = spec.drawEdges
        render.noEdges = !spec.drawEdges
        render.drawGrid = spec.drawGrid
        render.noGrid = !spec.drawGrid

        var request = Cadpole_V1_Request()
        request.render = render
        let response = try await call(request)
        switch response.response {
        case .render(let r):
            return RenderResult(
                png: r.png,
                width: r.width,
                height: r.height,
                diagnostics: r.diagnostics,
                model: r.hasModel ? r.model : nil
            )
        case .error(let e): throw EngineError.engine(e)
        default: throw EngineError.unexpectedResponse
        }
    }

    /// The machine-readable Starlark API, for script-editor completion.
    public func apiDoc() async throws -> Cadpole_V1_ApiDocResponse {
        var request = Cadpole_V1_Request()
        request.apiDoc = Cadpole_V1_ApiDocRequest()
        let response = try await call(request)
        switch response.response {
        case .apiDoc(let r): return r
        case .error(let e): throw EngineError.engine(e)
        default: throw EngineError.unexpectedResponse
        }
    }
}

extension Duration {
    /// Whole milliseconds, rounded down. `components` is (seconds, attoseconds).
    var milliseconds: Int64 {
        let (seconds, attoseconds) = components
        return seconds * 1_000 + attoseconds / 1_000_000_000_000_000
    }
}
