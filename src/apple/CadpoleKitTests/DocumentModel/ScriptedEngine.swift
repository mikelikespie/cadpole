// An `EngineClient` whose timing the test decides.
//
// `DocumentModel`'s bugs live in the gaps between requests: a second slider
// moving while the first apply is in flight, a slow render finishing after a
// fast one, an event that is our own write coming back. So every operation
// here can be held at the door and released by the test, one at a time, and
// everything the model asked for is recorded.

import Foundation
import CadpoleKit
import CadpoleProto
import XCTest

final class ScriptedEngine: EngineClient, @unchecked Sendable {
    let capabilities: EngineCapabilities
    let clientID = "scene-A"

    struct Render: Equatable, CustomStringConvertible {
        var view: String
        var draft: Bool
        var description: String { "view=\(view) draft=\(draft)" }
        var png: Data { Data(description.utf8) }
    }

    private let lock = NSLock()
    private var snapshot = SessionSnapshot(
        id: "sess-1",
        revision: 1,
        documentName: "planter",
        displayUnit: "mm",
        scripts: [],
        paramOverrides: [:],
        selection: [],
        lastEventSeq: 0
    )
    private var _evaluates = 0
    private var _cancelledEvaluates = 0
    private var _rendersStarted: [Render] = []
    private var _rendersFinished: [Render] = []
    private var _sessionReads = 0
    private var _setParamsCalls: [[String: Cadpole_V1_ParamValue]] = []
    private var _failNextSetParams: EngineError?
    private var _echoWrites = false

    /// Operations the test is holding, by name ("setParams", "evaluate",
    /// "render:front"), each a queue of tickets in arrival order.
    private var holding: Set<String> = []
    private var waiting: [String: [UUID]] = [:]
    private var released: Set<UUID> = []

    private var eventLog: [EngineEvent] = []
    private var eventListeners: [UUID: AsyncThrowingStream<EngineEvent, any Error>.Continuation] = [:]
    private var connectionListeners: [UUID: AsyncStream<ConnectionState>.Continuation] = [:]
    private var connectionState: ConnectionState = .live(revision: 1)

    init(capabilities: EngineCapabilities = [.pathTrace]) {
        self.capabilities = capabilities
    }

    // MARK: What happened

    var evaluates: Int { lock.withLock { _evaluates } }
    var cancelledEvaluates: Int { lock.withLock { _cancelledEvaluates } }
    var rendersStarted: [Render] { lock.withLock { _rendersStarted } }
    var rendersFinished: [Render] { lock.withLock { _rendersFinished } }
    var sessionReads: Int { lock.withLock { _sessionReads } }
    var setParamsCalls: [[String: Cadpole_V1_ParamValue]] { lock.withLock { _setParamsCalls } }
    var overrides: [String: Cadpole_V1_ParamValue] { lock.withLock { snapshot.paramOverrides } }

    // MARK: What the test decides

    /// Send our own writes back on the event stream, attributed to
    /// `clientID`, the way `cadpole serve` and `LocalEngine` do.
    var echoWrites: Bool {
        get { lock.withLock { _echoWrites } }
        set { lock.withLock { _echoWrites = newValue } }
    }

    func failNextSetParams(_ error: EngineError) {
        lock.withLock { _failNextSetParams = error }
    }

    func hold(_ operation: String) {
        lock.withLock { _ = holding.insert(operation) }
    }

    /// How many calls of `operation` are waiting at the door.
    func waitingCount(_ operation: String) -> Int {
        lock.withLock { waiting[operation]?.count ?? 0 }
    }

    /// Let the oldest held call of `operation` through.
    func releaseOne(_ operation: String) {
        lock.withLock {
            guard var queue = waiting[operation], !queue.isEmpty else { return }
            released.insert(queue.removeFirst())
            waiting[operation] = queue
        }
    }

    /// Let every held call through, and stop holding.
    func releaseAll(_ operation: String) {
        lock.withLock {
            holding.remove(operation)
            for t in waiting[operation] ?? [] { released.insert(t) }
            waiting[operation] = []
        }
    }

    /// Another writer -- an assistant -- changing the document.
    func externalWrite(kind: String, by: String) {
        emit(kind: kind, by: by)
    }

    func sendConnection(_ state: ConnectionState) {
        let listeners = lock.withLock { () -> [AsyncStream<ConnectionState>.Continuation] in
            connectionState = state
            return Array(connectionListeners.values)
        }
        for l in listeners { l.yield(state) }
    }

    /// End every stream, so the model's listening task can finish.
    func shutDown() {
        let (events, connections) = lock.withLock {
            (Array(eventListeners.values), Array(connectionListeners.values))
        }
        for e in events { e.finish() }
        for c in connections { c.finish() }
    }

    // MARK: The door

    /// Waits while the test holds `operation`. A cancellable wait throws
    /// `EngineError.cancelled` when the calling task is cancelled, which is
    /// what `LocalEngine` does through `cadpole_cancel`.
    private func door(_ operation: String, cancellable: Bool = false) async throws {
        let ticket: UUID? = lock.withLock {
            guard holding.contains(operation) else { return nil }
            let t = UUID()
            waiting[operation, default: []].append(t)
            return t
        }
        guard let ticket else { return }
        while true {
            let open = lock.withLock { released.contains(ticket) }
            if open { return }
            if cancellable && Task.isCancelled {
                lock.withLock {
                    waiting[operation]?.removeAll { $0 == ticket }
                    _cancelledEvaluates += operation == "evaluate" ? 1 : 0
                }
                throw EngineError.cancelled
            }
            await Task.yield()
            try? await Task.sleep(nanoseconds: 500_000)
        }
    }

    // MARK: EngineClient

    func session() async throws -> SessionSnapshot {
        lock.withLock {
            _sessionReads += 1
            return snapshot
        }
    }

    func document() async throws -> Cadpole_V1_Document {
        lock.withLock { snapshot }.asDocument()
    }

    func call(_ request: Cadpole_V1_Request) async throws -> Cadpole_V1_Response {
        var response = Cadpole_V1_Response()
        switch request.request {
        case .evaluate(let e):
            lock.withLock { _evaluates += 1 }
            try await door("evaluate", cancellable: true)
            var r = Cadpole_V1_EvaluateResponse()
            r.model = Self.model(overrides: e.document.paramOverrides)
            response.evaluate = r
        case .render(let r):
            let render = Render(view: r.camera.preset, draft: r.draft)
            lock.withLock { _rendersStarted.append(render) }
            try await door("render:\(render.view)")
            var out = Cadpole_V1_RenderResponse()
            out.png = render.png
            out.width = r.width
            out.height = r.height
            response.render = out
            lock.withLock { _rendersFinished.append(render) }
        default:
            response.error = "not scripted"
        }
        return response
    }

    @discardableResult
    func setParams(_ overrides: [String: Cadpole_V1_ParamValue]) async throws -> SessionSnapshot {
        lock.withLock { _setParamsCalls.append(overrides) }
        try await door("setParams")
        let (failure, updated, echo) = lock.withLock { () -> (EngineError?, SessionSnapshot, Bool) in
            if let f = _failNextSetParams {
                _failNextSetParams = nil
                return (f, snapshot, false)
            }
            snapshot.paramOverrides = overrides
            snapshot.revision += 1
            return (nil, snapshot, _echoWrites)
        }
        if let failure { throw failure }
        if echo { emit(kind: "params_changed", by: clientID) }
        return updated
    }

    @discardableResult
    func setSelection(_ refs: [Cadpole_V1_EntityRef]) async throws -> SessionSnapshot {
        lock.withLock {
            snapshot.selection = refs
            return snapshot
        }
    }

    func events(after seq: UInt64) -> AsyncThrowingStream<EngineEvent, any Error> {
        AsyncThrowingStream { continuation in
            let id = UUID()
            let backlog = lock.withLock { () -> [EngineEvent] in
                eventListeners[id] = continuation
                return eventLog.filter { $0.seq > seq }
            }
            for e in backlog { continuation.yield(e) }
            continuation.onTermination = { [weak self] _ in
                self?.lock.withLock { self?.eventListeners[id] = nil }
            }
        }
    }

    var connection: AsyncStream<ConnectionState> {
        AsyncStream { continuation in
            let id = UUID()
            let now = lock.withLock { () -> ConnectionState in
                connectionListeners[id] = continuation
                return connectionState
            }
            continuation.yield(now)
            continuation.onTermination = { [weak self] _ in
                self?.lock.withLock { self?.connectionListeners[id] = nil }
            }
        }
    }

    private func emit(kind: String, by: String) {
        let (event, listeners) = lock.withLock { () -> (EngineEvent, [AsyncThrowingStream<EngineEvent, any Error>.Continuation]) in
            let seq = (eventLog.last?.seq ?? 0) + 1
            let event = EngineEvent(seq: seq, at: "t", kind: kind, dataJSON: Data("{}".utf8), by: by)
            eventLog.append(event)
            snapshot.lastEventSeq = seq
            return (event, Array(eventListeners.values))
        }
        for l in listeners { l.yield(event) }
    }

    // MARK: The model it "evaluates"

    static func model(overrides: [String: Cadpole_V1_ParamValue]) -> Cadpole_V1_Model {
        func param(_ name: String, _ fallback: Double) -> Cadpole_V1_ParamInfo {
            var p = Cadpole_V1_ParamInfo()
            p.name = name
            p.defaultValue = .number(fallback)
            p.value = overrides[name] ?? .number(fallback)
            p.overridden = overrides[name] != nil
            p.min = 0
            p.max = 1000
            return p
        }
        var model = Cadpole_V1_Model()
        model.params = [param("wall", 2.4), param("height", 60)]
        return model
    }
}

// MARK: - Helpers

extension XCTestCase {
    /// Wait for a condition the model reaches asynchronously, and fail rather
    /// than hang when it never does.
    @MainActor
    func eventually(
        _ what: String,
        timeout: Duration = .seconds(5),
        file: StaticString = #filePath,
        line: UInt = #line,
        _ condition: @MainActor () -> Bool
    ) async {
        let deadline = ContinuousClock.now + timeout
        while !condition() {
            if ContinuousClock.now >= deadline {
                XCTFail("timed out waiting for \(what)", file: file, line: line)
                return
            }
            try? await Task.sleep(nanoseconds: 1_000_000)
        }
    }

    /// Let whatever the model has in flight run out, so a count taken next is
    /// final rather than a snapshot of work still on its way.
    @MainActor
    func settle(_ duration: Duration = .milliseconds(150)) async {
        try? await Task.sleep(for: duration)
    }

    /// A model over `engine`, loaded, with its listening loops running.
    @MainActor
    func loadedModel(_ engine: ScriptedEngine) async -> (DocumentModel, Task<Void, Never>) {
        let model = DocumentModel(engine: engine)
        let task = Task { await model.load() }
        await eventually("the document to load") { model.phase == .ready && model.renderedPNG != nil }
        await settle()
        return (model, task)
    }
}
