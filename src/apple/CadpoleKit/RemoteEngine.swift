// `EngineClient` over `cadpole serve`'s session API (docs/design.md).
//
// Works on both platforms and is the *only* engine on iOS. Three shapes of
// traffic:
//
//   * JSON session state   GET/PUT /api/session*  -- the web UI's view.
//   * protobuf envelope    POST /api/call        -- evaluate, render, api-doc.
//                          Byte-for-byte the same request LocalEngine hands to
//                          cadpole_call, which is what keeps the two equivalent.
//   * SSE                  GET /api/events       -- the change stream.
//
// Every request carries `Authorization: Bearer`, the event stream included.
// docs/security.md's `?token=` exception is a browser limitation (EventSource
// cannot set headers) and must not be used here.

import Foundation
import CadpoleProto

public final class RemoteEngine: EngineClient {
    public let baseURL: URL
    /// Identifies this scene so its own writes are not echoed back to it.
    public let clientID: String

    private let token: String?
    private let transport: any HTTPTransport
    private let eventTransport: any HTTPTransport
    private let reconnect: ReconnectPolicy
    private let monitor = ConnectionMonitor()

    /// The connection pill's state (docs/apple-apps.md), driven by the
    /// event stream's retry loop below.
    public var connection: AsyncStream<ConnectionState> { monitor.stream() }

    public var capabilities: EngineCapabilities {
        // A session server is what makes the assistant workflow possible; it
        // renders and exports for us, and it owns the document. It cannot
        // evaluate *locally*, which is the one thing the phone gives up.
        [.pathTrace, .stepExport, .assistant, .assets]
    }

    public init(
        baseURL: URL,
        token: String?,
        transport: any HTTPTransport,
        eventTransport: (any HTTPTransport)? = nil,
        clientID: String = UUID().uuidString,
        reconnect: ReconnectPolicy = .default
    ) {
        self.baseURL = baseURL
        self.token = token
        self.transport = transport
        self.eventTransport = eventTransport ?? transport
        self.clientID = clientID
        self.reconnect = reconnect
    }

    // MARK: - Requests

    private func makeRequest(
        _ method: String,
        _ path: String,
        query: [String: String] = [:],
        contentType: String? = nil,
        body: Data? = nil,
        accept: String? = nil
    ) -> HTTPRequest {
        var components = URLComponents(
            url: baseURL.appendingPathComponent(path),
            resolvingAgainstBaseURL: false
        )!
        if !query.isEmpty {
            components.queryItems = query.sorted { $0.key < $1.key }
                .map { URLQueryItem(name: $0.key, value: $0.value) }
        }
        var headers = ["X-Cadpole-Client": clientID]
        if let token { headers["Authorization"] = "Bearer \(token)" }
        if let contentType { headers["Content-Type"] = contentType }
        if let accept { headers["Accept"] = accept }
        return HTTPRequest(method: method, url: components.url!, headers: headers, body: body)
    }

    private func perform(_ request: HTTPRequest) async throws -> Data {
        let response = try await transport.send(request)
        switch response.status {
        case 200 ..< 300:
            return response.body
        case 409:
            // A write lost the optimistic-concurrency race.
            struct Conflict: Decodable { var revision: UInt64 }
            let current = (try? JSONDecoder().decode(Conflict.self, from: response.body))?.revision
            throw EngineError.staleRevision(current: current ?? 0)
        default:
            let text = String(data: response.body, encoding: .utf8) ?? ""
            throw EngineError.http(status: response.status, body: text)
        }
    }

    // MARK: - EngineClient

    public func session() async throws -> SessionSnapshot {
        let data = try await perform(makeRequest("GET", "api/session"))
        let snapshot = try SessionWire.decode(data)
        monitor.noteRevision(snapshot.revision)
        return snapshot
    }

    /// The whole document, from `GET /api/session/document`.
    ///
    /// Not `session().asDocument()`: the JSON session view is the web UI's
    /// contract and names the assets without their bytes, so a script
    /// that calls `asset("...")` would fail with "asset not found" on every
    /// evaluate and render. The protobuf route carries the document as the
    /// server holds it -- scripts, overrides, assets and the selection.
    public func document() async throws -> Cadpole_V1_Document {
        let data = try await perform(
            makeRequest("GET", "api/session/document", accept: "application/x-protobuf")
        )
        do {
            return try Cadpole_V1_Document(serializedData: data)
        } catch {
            throw EngineError.decoding("document: \(error)")
        }
    }

    public func call(_ cadpoleRequest: Cadpole_V1_Request) async throws -> Cadpole_V1_Response {
        let body = try cadpoleRequest.serializedData()
        let data = try await perform(
            makeRequest(
                "POST",
                "api/call",
                contentType: "application/x-protobuf",
                body: body,
                accept: "application/x-protobuf"
            )
        )
        do {
            return try Cadpole_V1_Response(serializedData: data)
        } catch {
            throw EngineError.decoding("response: \(error)")
        }
    }

    @discardableResult
    public func setParams(_ overrides: [String: Cadpole_V1_ParamValue]) async throws -> SessionSnapshot {
        var json: [String: JSONValue] = [:]
        for (name, value) in overrides {
            if let v = JSONValue(paramValue: value) { json[name] = v }
        }
        let body = try JSONEncoder().encode(["overrides": json])
        let data = try await perform(
            makeRequest("PUT", "api/session/params", contentType: "application/json", body: body)
        )
        return try SessionWire.decode(data)
    }

    @discardableResult
    public func setSelection(_ refs: [Cadpole_V1_EntityRef]) async throws -> SessionSnapshot {
        let body = try JSONEncoder().encode(SelectionWire(refs: refs))
        let data = try await perform(
            makeRequest("PUT", "api/session/selection", contentType: "application/json", body: body)
        )
        return try SessionWire.decode(data)
    }

    /// One attempt at the stream. `Last-Event-ID` is the standard SSE resume
    /// header and is what a client is supposed to send; `cadpole serve` resumes
    /// from `?after=` (`EventsQuery` in `src/rust/cadpole-cli/src/server.rs`), so
    /// both carry the same cursor and a reconnection picks up where the last
    /// one stopped whichever spelling the server reads. The cursor is a
    /// sequence number, never a credential -- the token stays in
    /// `Authorization` on this route as on every other (docs/security.md).
    private func eventsRequest(after cursor: UInt64) -> HTTPRequest {
        var request = makeRequest(
            "GET",
            "api/events",
            query: cursor > 0 ? ["after": String(cursor)] : [:],
            accept: "text/event-stream"
        )
        if cursor > 0 { request.headers["Last-Event-ID"] = String(cursor) }
        return request
    }

    /// Which event log the server is keeping, and how far it has got: the
    /// session id (new per `cadpole serve` process) and `last_event_seq`.
    /// Only those two fields are read, so the check does not depend on the
    /// rest of the view.
    private func logEpoch() async throws -> (id: String, lastEventSeq: UInt64) {
        struct Wire: Decodable {
            var id: String
            var last_event_seq: UInt64
        }
        let data = try await perform(makeRequest("GET", "api/session"))
        do {
            let wire = try JSONDecoder().decode(Wire.self, from: data)
            return (wire.id, wire.last_event_seq)
        } catch {
            throw EngineError.decoding("session: \(error)")
        }
    }

    /// A 401 is the one answer retrying cannot fix: `cadpole serve` mints a new
    /// token every run, so the stored one is dead and the user has to paste a
    /// new URL. Everything else -- a refused socket, a 503, a stream that just
    /// stops -- is the connection, and the connection comes back.
    private static func isTerminal(_ error: EngineError) -> Bool {
        if case .http(let status, _) = error, status == 401 { return true }
        return false
    }

    /// The change stream, reconnected for as long as the caller listens.
    ///
    /// A phone sleeps, changes network, or walks out of range; a server is
    /// restarted. Every one of those ends the underlying byte stream, and
    /// without a retry the viewport would silently stop following the
    /// document. So: retry on the `ReconnectPolicy` ladder, resume from the
    /// last sequence number seen, publish what is happening, and stop only on
    /// a 401.
    public func events(after seq: UInt64) -> AsyncThrowingStream<EngineEvent, any Error> {
        let eventTransport = eventTransport
        let monitor = monitor
        let policy = reconnect
        return AsyncThrowingStream { continuation in
            let task = Task {
                var cursor = seq
                var failures = 0
                var attempts = 0
                var degradedSince: Date?
                // The session id the last connection saw. `cadpole serve`
                // mints a new one per process, so a change is a restart even
                // when the new log has already passed our cursor. Read before
                // the first connection too, or the usual restart -- the first
                // reconnection -- would have nothing to compare with. A
                // failed read there leaves it unknown, as before.
                var knownEpoch: String? = try? await self.logEpoch().id

                while !Task.isCancelled {
                    attempts += 1
                    // The restart check only applies to a *re*connection: on the
                    // first one the cursor came from the caller's own session
                    // read, so a low seq there is ordinary backlog.
                    var epochToCheck = attempts > 1
                    monitor.send(failures == 0 ? .connecting : .degraded(since: degradedSince ?? Date()))
                    do {
                        if attempts > 1 {
                            // A restarted server numbers its log from 1 again
                            // and resumes with `seq > after`, so it would send
                            // a stale cursor nothing at all -- not even the low
                            // seq the in-stream check below waits for -- until
                            // its new log passed the old cursor. Ask the
                            // session where its log is before resuming.
                            let epoch = try await self.logEpoch()
                            if epoch.lastEventSeq < cursor || (knownEpoch != nil && epoch.id != knownEpoch) {
                                // A new log: follow it from its start.
                                cursor = 0
                                epochToCheck = false
                            }
                            knownEpoch = epoch.id
                        }
                        let bytes = try await eventTransport.stream(self.eventsRequest(after: cursor))
                        failures = 0
                        degradedSince = nil
                        monitor.markLive()

                        var parser = SSEParser()
                        var chunk = [UInt8]()
                        chunk.reserveCapacity(1024)
                        for try await byte in bytes {
                            chunk.append(byte)
                            // Frames end at a blank line, so flush on every newline
                            // rather than buffering for a size that may never come.
                            guard byte == UInt8(ascii: "\n") else { continue }
                            for frame in parser.push(chunk) {
                                guard let event = try? EngineEvent.decode(sseData: frame.data) else { continue }
                                if epochToCheck {
                                    epochToCheck = false
                                    // The server resumes from our cursor, so the
                                    // first event of a reconnection is above it --
                                    // unless the log started again (the server
                                    // restarted). Follow the new one:
                                    // holding the old cursor would drop everything
                                    // the restarted server will ever send.
                                    if event.seq <= cursor { cursor = 0 }
                                }
                                guard event.seq > cursor else { continue }
                                cursor = event.seq
                                continuation.yield(event)
                            }
                            chunk.removeAll(keepingCapacity: true)
                        }
                        // The stream ended. `cadpole serve` never closes it on
                        // purpose, so this is a dropped connection.
                    } catch let error as EngineError where Self.isTerminal(error) {
                        monitor.send(.unauthorized)
                        continuation.finish(throwing: error)
                        return
                    } catch is CancellationError {
                        break
                    } catch {
                        // The connection, not the document. Retry below.
                    }

                    guard !Task.isCancelled else { break }
                    failures += 1
                    let since = degradedSince ?? Date()
                    degradedSince = since
                    monitor.send(.degraded(since: since))
                    do {
                        try await Task.sleep(for: policy.delay(afterFailures: failures))
                    } catch {
                        break  // cancelled while waiting
                    }
                }
                monitor.send(.disconnected)
                continuation.finish()
            }
            continuation.onTermination = { _ in task.cancel() }
        }
    }
}

// MARK: - The server's JSON session view

/// `cadpole_cli::server::SessionView`, as Swift sees it. Field names are the Rust
/// struct's, because serde serialises them verbatim.
struct SessionWire: Decodable {
    struct DocumentWire: Decodable {
        var id: String
        var name: String
        var display_unit: String
        var scripts: [ScriptWire]
        var param_overrides: [String: JSONValue]
    }

    struct ScriptWire: Decodable {
        var path: String
        var source: String
    }

    struct RefWire: Codable {
        var feature_id: String
        var body_id: String?
        var kind: String
        var topo_name: String?
    }

    struct SelectionView: Decodable {
        var selected: [RefWire]
        var hovered: RefWire?
        var note: String?
    }

    var id: String
    var revision: UInt64
    var document: DocumentWire
    var selection: SelectionView
    var last_event_seq: UInt64

    static func decode(_ data: Data) throws -> SessionSnapshot {
        let wire: SessionWire
        do {
            wire = try JSONDecoder().decode(SessionWire.self, from: data)
        } catch {
            throw EngineError.decoding("session: \(error)")
        }
        var overrides: [String: Cadpole_V1_ParamValue] = [:]
        for (name, value) in wire.document.param_overrides {
            if let v = value.paramValue { overrides[name] = v }
        }
        return SessionSnapshot(
            id: wire.id,
            revision: wire.revision,
            documentName: wire.document.name,
            displayUnit: wire.document.display_unit,
            scripts: wire.document.scripts.map { s in
                var f = Cadpole_V1_ScriptFile()
                f.path = s.path
                f.source = s.source
                return f
            },
            paramOverrides: overrides,
            selection: wire.selection.selected.map(\.entityRef),
            lastEventSeq: wire.last_event_seq
        )
    }
}

struct SelectionWire: Encodable {
    var selected: [SessionWire.RefWire]
    var note: String

    init(refs: [Cadpole_V1_EntityRef], note: String = "") {
        selected = refs.map { r in
            SessionWire.RefWire(
                feature_id: r.featureID,
                body_id: r.bodyID,
                kind: EntityKindNames.name(for: r.kind),
                topo_name: r.topoName
            )
        }
        self.note = note
    }
}

extension SessionWire.RefWire {
    var entityRef: Cadpole_V1_EntityRef {
        var r = Cadpole_V1_EntityRef()
        r.featureID = feature_id
        r.bodyID = body_id ?? ""
        r.kind = EntityKindNames.kind(for: kind)
        r.topoName = topo_name ?? ""
        return r
    }
}

/// The `kind` strings the JSON view uses, mirroring `cadpole_engine::kind_name`.
/// One vocabulary for the web UI, `cadpole selection --json` and VoiceOver.
public enum EntityKindNames {
    public static func name(for kind: Cadpole_V1_EntityKind) -> String {
        switch kind {
        case .feature: return "feature"
        case .body: return "body"
        case .face: return "face"
        case .edge: return "edge"
        case .vertex: return "vertex"
        case .sketch: return "sketch"
        case .sketchEntity: return "sketch_entity"
        case .param: return "param"
        case .asset: return "asset"
        default: return ""
        }
    }

    public static func kind(for name: String) -> Cadpole_V1_EntityKind {
        switch name {
        case "feature": return .feature
        case "body": return .body
        case "face": return .face
        case "edge": return .edge
        case "vertex": return .vertex
        case "sketch": return .sketch
        case "sketch_entity": return .sketchEntity
        case "param": return .param
        case "asset": return .asset
        default: return .unspecified
        }
    }
}

extension SessionSnapshot {
    /// The document both engines evaluate: scripts, overrides and the selection
    /// (which is what `highlight_selection` draws).
    public func asDocument() -> Cadpole_V1_Document {
        var doc = Cadpole_V1_Document()
        doc.id = id
        doc.name = documentName
        doc.schemaVersion = 1
        doc.displayUnit = DisplayUnit(displayUnit).lengthUnit
        doc.scripts = scripts
        doc.paramOverrides = paramOverrides
        var sel = Cadpole_V1_Selection()
        sel.selected = selection
        doc.selection = sel
        return doc
    }
}
