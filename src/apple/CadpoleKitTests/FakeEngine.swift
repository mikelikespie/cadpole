// Fakes for the cheap test layer (docs/apple-apps.md).
//
// Two of them, because there are two seams worth pinning:
//
//   FakeEngine     conforms to EngineClient directly, so the protocol's shared
//                  evaluate/render/apiDoc behaviour is tested without any HTTP
//                  at all.
//   FakeTransport  conforms to HTTPTransport, so RemoteEngine's own logic
//                  (URLs, the bearer header, JSON and protobuf framing, SSE,
//                  status handling) is tested with no server.

import Foundation
import CadpoleKit
import CadpoleProto

// MARK: - A whole engine, faked

/// An in-memory `EngineClient`. Evaluates nothing: it returns the model it was
/// given, which is exactly what a screen test needs.
final class FakeEngine: EngineClient, @unchecked Sendable {
    let capabilities: EngineCapabilities

    private let lock = NSLock()
    private var _snapshot: SessionSnapshot
    private var _model: Cadpole_V1_Model
    private var _png: Data
    private var _events: [EngineEvent]
    private var _calls: [Cadpole_V1_Request] = []

    init(
        snapshot: SessionSnapshot = FakeEngine.defaultSnapshot(),
        model: Cadpole_V1_Model = FakeEngine.defaultModel(),
        png: Data = Data([0x89, 0x50, 0x4E, 0x47]),
        events: [EngineEvent] = [],
        capabilities: EngineCapabilities = [.localEvaluation, .stepExport]
    ) {
        _snapshot = snapshot
        _model = model
        _png = png
        _events = events
        self.capabilities = capabilities
    }

    /// Every `Request` the client sent, in order.
    var calls: [Cadpole_V1_Request] {
        lock.sync { _calls }
    }

    func session() async throws -> SessionSnapshot {
        lock.sync { _snapshot }
    }

    func document() async throws -> Cadpole_V1_Document {
        lock.sync { _snapshot }.asDocument()
    }

    func call(_ request: Cadpole_V1_Request) async throws -> Cadpole_V1_Response {
        lock.sync { _calls.append(request) }
        var response = Cadpole_V1_Response()
        switch request.request {
        case .evaluate:
            var r = Cadpole_V1_EvaluateResponse()
            r.model = lock.sync { _model }
            response.evaluate = r
        case .render:
            var r = Cadpole_V1_RenderResponse()
            r.png = lock.sync { _png }
            r.width = request.render.width
            r.height = request.render.height
            r.model = lock.sync { _model }
            r.diagnostics = lock.sync { _model }.diagnostics
            response.render = r
        case .apiDoc:
            var r = Cadpole_V1_ApiDocResponse()
            var f = Cadpole_V1_ApiFunction()
            f.name = "extrude"
            f.doc = "Extrude a sketch."
            r.functions = [f]
            response.apiDoc = r
        default:
            response.error = "unsupported in the fake: \(String(describing: request.request))"
        }
        return response
    }

    @discardableResult
    func setParams(_ overrides: [String: Cadpole_V1_ParamValue]) async throws -> SessionSnapshot {
        lock.sync {
            _snapshot.paramOverrides = overrides
            _snapshot.revision += 1
            // Keep the returned model's params consistent with the override so
            // a test can assert the round trip end to end.
            for i in _model.params.indices {
                if let v = overrides[_model.params[i].name] {
                    _model.params[i].value = v
                    _model.params[i].overridden = true
                }
            }
            return _snapshot
        }
    }

    @discardableResult
    func setSelection(_ refs: [Cadpole_V1_EntityRef]) async throws -> SessionSnapshot {
        lock.sync {
            _snapshot.selection = refs
            return _snapshot
        }
    }

    func events(after seq: UInt64) -> AsyncThrowingStream<EngineEvent, any Error> {
        let events = lock.sync { _events }.filter { $0.seq > seq }
        return AsyncThrowingStream { continuation in
            for e in events { continuation.yield(e) }
            continuation.finish()
        }
    }

    // MARK: Fixtures

    static func defaultSnapshot() -> SessionSnapshot {
        var script = Cadpole_V1_ScriptFile()
        script.path = "main.star"
        script.source = "wall = param('wall', 2.4)\n"
        return SessionSnapshot(
            id: "sess-1",
            revision: 3,
            documentName: "planter",
            displayUnit: "mm",
            scripts: [script],
            paramOverrides: ["wall": .number(2.4)],
            selection: [],
            lastEventSeq: 7
        )
    }

    static func defaultModel() -> Cadpole_V1_Model {
        var wall = Cadpole_V1_ParamInfo()
        wall.name = "wall"
        wall.defaultValue = .number(2.0)
        wall.value = .number(2.4)
        wall.overridden = true
        wall.unit = "mm"
        wall.min = 1.0
        wall.max = 6.0
        wall.step = 0.1
        wall.description_p = "Wall thickness"

        var height = Cadpole_V1_ParamInfo()
        height.name = "height"
        height.defaultValue = .number(60)
        height.value = .number(60)
        height.unit = "mm"
        height.min = 10
        height.max = 200

        var sketch = Cadpole_V1_Feature()
        sketch.id = "sketch_1"
        sketch.kind = "sketch"
        sketch.outputs = []

        var extrude = Cadpole_V1_Feature()
        extrude.id = "extrude_1"
        extrude.kind = "extrude"
        extrude.inputs = ["sketch_1"]
        extrude.outputs = ["extrude_1#0"]

        var face = Cadpole_V1_TopoFace()
        face.name = "face:top"
        face.surfaceKind = "plane"
        face.area = 1764

        var topology = Cadpole_V1_Topology()
        topology.faces = [face]

        var body = Cadpole_V1_Body()
        body.id = "extrude_1#0"
        body.featureID = "extrude_1"
        body.name = "planter"
        body.visible = true
        body.topology = topology

        var model = Cadpole_V1_Model()
        model.params = [wall, height]
        model.features = [sketch, extrude]
        model.bodies = [body]
        model.evalMillis = 12.5
        return model
    }
}

// MARK: - A transport, faked

/// Answers `RemoteEngine`'s requests from a table, and records what it was
/// asked. `handler` gets the whole request so a test can assert on the body.
final class FakeTransport: HTTPTransport, @unchecked Sendable {
    typealias Handler = @Sendable (HTTPRequest) throws -> HTTPResponse

    /// One connection's worth of the event stream. `RemoteEngine` reconnects,
    /// so a test scripts what each successive attempt gets.
    enum StreamEpisode: Sendable, Equatable {
        /// These bytes, and then EOF -- which is what a dropped connection
        /// looks like from inside `URLSession.bytes(for:)`.
        case body(String)
        /// The connection is refused before a byte arrives, the way a 401 or a
        /// closed port does.
        case failure(EngineError)
    }

    private let handler: Handler
    private let episodes: [StreamEpisode]
    private let lock = NSLock()
    private var _requests: [HTTPRequest] = []
    private var _streamAttempts = 0

    init(streamBody: String = "", handler: @escaping Handler) {
        self.handler = handler
        episodes = streamBody.isEmpty ? [] : [.body(streamBody)]
    }

    init(streamEpisodes: [StreamEpisode], handler: @escaping Handler) {
        self.handler = handler
        episodes = streamEpisodes
    }

    var requests: [HTTPRequest] {
        lock.sync { _requests }
    }

    /// How many times the event stream has been opened. One means no reconnect.
    var streamAttempts: Int {
        lock.sync { _streamAttempts }
    }

    /// Only the event-stream requests, in order.
    var streamRequests: [HTTPRequest] {
        lock.sync { _requests }.filter { $0.url.path == "/api/events" }
    }

    func send(_ request: HTTPRequest) async throws -> HTTPResponse {
        lock.sync { _requests.append(request) }
        return try handler(request)
    }

    func stream(_ request: HTTPRequest) async throws -> AsyncThrowingStream<UInt8, any Error> {
        let episode: StreamEpisode? = lock.sync {
            _requests.append(request)
            defer { _streamAttempts += 1 }
            return _streamAttempts < episodes.count ? episodes[_streamAttempts] : nil
        }
        switch episode {
        case .failure(let error):
            throw error
        case .body(let text):
            let bytes = Array(text.utf8)
            return AsyncThrowingStream { continuation in
                for b in bytes { continuation.yield(b) }
                continuation.finish()
            }
        case nil:
            // The script ran out: a connection that stays open and says
            // nothing, so a test stops when it has what it came for rather
            // than racing an endless reconnection loop.
            return AsyncThrowingStream { _ in }
        }
    }
}

extension NSLock {
    func sync<T>(_ body: () throws -> T) rethrows -> T {
        lock()
        defer { unlock() }
        return try body()
    }
}
