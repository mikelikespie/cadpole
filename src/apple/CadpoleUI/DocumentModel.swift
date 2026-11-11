// The state behind one open document, and the only place that talks to an
// `EngineClient`.
//
// Scope: open a document, evaluate it, render it, select geometry, change a
// parameter. Messages, the script editor, exports, sweeps and the wgpu
// viewport are not implemented.

import Foundation
import CadpoleKit
import CadpoleProto
import Observation

@MainActor
@Observable
public final class DocumentModel {
    public enum Phase: Equatable {
        case idle
        case loading
        case ready
        case failed(String)
    }

    private let engine: any EngineClient
    private var coalescer: ParamCoalescer!

    public private(set) var phase: Phase = .idle
    public private(set) var session = SessionSnapshot()
    public private(set) var model = Cadpole_V1_Model()
    /// The last image the engine produced. Stale while a re-evaluation is in
    /// flight, which the viewport dims rather than blanking.
    public private(set) var renderedPNG: Data?
    public private(set) var isStale = false
    public private(set) var capabilities: EngineCapabilities

    /// The toolbar's connection pill. `RemoteEngine` drives it from its event-stream retry
    /// loop; an engine linked in-process reports `.live` and stays there.
    public private(set) var connection: ConnectionState = .disconnected

    /// Live values of the sliders. Separate from `model.params` so a drag stays
    /// smooth while the engine catches up (the UI is never blocked on it).
    public var paramValues: [String: Double] = [:]

    /// The tree's selection, as row ids. `entityRefs` turns it back into the
    /// `EntityRef`s the engine and an assistant speak.
    public var selection: Set<String> = [] {
        didSet {
            guard selection != oldValue else { return }
            Task { await pushSelection() }
        }
    }

    public var view: String = "iso" {
        didSet { Task { await rerender() } }
    }

    public init(engine: any EngineClient) {
        self.engine = engine
        capabilities = engine.capabilities
        // An in-process engine honours task cancellation (cadpole_cancel),
        // so a newer slider value stops the evaluation it supersedes.
        // A server keeps computing whatever it was asked; there, the newest
        // value simply goes next.
        coalescer = ParamCoalescer(cancelsInFlight: engine.capabilities.contains(.localEvaluation)) { [weak self] overrides in
            await self?.applyParams(overrides)
        }
    }

    // MARK: - Loading

    public func load() async {
        phase = .loading
        do {
            session = try await engine.session()
            try await reevaluate()
            phase = .ready
            await rerender()
            await listen()
        } catch {
            phase = .failed(Self.describe(error))
        }
    }

    /// Following the document: the change stream, and the connection under it.
    ///
    /// Two loops, because they answer different questions. The events say what
    /// changed; the connection says whether we are still being told. Both run
    /// until the screen goes away, which cancels this task group.
    private func listen() async {
        await withTaskGroup(of: Void.self) { group in
            group.addTask { [weak self] in
                guard let self else { return }
                await self.watchConnection()
            }
            group.addTask { [weak self] in
                guard let self else { return }
                await self.watchEvents()
            }
        }
    }

    /// The change stream. Another writer -- usually the assistant -- moving the
    /// document is the normal case, not an error.
    private func watchEvents() async {
        do {
            for try await event in engine.events(after: session.lastEventSeq) {
                session.lastEventSeq = event.seq
                // Our own write coming back: applyParams/pushSelection have
                // already evaluated and rendered it.
                if event.isEcho(of: engine.clientID) { continue }
                do {
                    switch event.kind {
                    case "script_changed", "params_changed":
                        session = try await engine.session()
                        try await reevaluate()
                        await rerender()
                    case "selection_changed":
                        session = try await engine.session()
                    default:
                        break
                    }
                } catch {
                    // One failed refresh is not the end of the stream: the next
                    // event, or the next reconnection, tries again.
                    isStale = true
                }
            }
        } catch {
            // The stream reconnects on its own, so arriving here means it gave
            // up -- a rotated token. The model on screen stays usable and
            // read-only.
            isStale = true
        }
    }

    /// The connection, and the re-read that makes a reconnection honest.
    ///
    /// A server restarted while we were away has a new event log, so the
    /// restart check drops its low sequence numbers and nothing in the stream can
    /// say what changed in between. Reading the session again is the only way
    /// to find out.
    ///
    /// Only a real reconnection (live again after not being live) re-reads:
    /// `.live(revision:)` also changes whenever a session read sees a new
    /// revision, which every write -- our own included -- causes.
    private func watchConnection() async {
        var reconnections = ReconnectionDetector()
        for await state in engine.connection {
            connection = state
            if reconnections.observe(state) { await refresh() }
        }
    }

    /// Read everything again, from scratch.
    private func refresh() async {
        do {
            session = try await engine.session()
            try await reevaluate()
            await rerender()
        } catch {
            isStale = true
        }
    }

    // MARK: - The work

    private func reevaluate() async throws {
        // 20 s is `security.md`'s interactive budget. Local mode enforces it
        // at the FFI boundary; remote mode is bounded by the server.
        model = try await engine.evaluate(deadline: .seconds(20))
        // A slider the user has moved since is ahead of this model: leave
        // its thumb where the finger put it, or it jumps back to a value the
        // user has already left until the newer apply lands.
        for param in model.params where param.value.isNumber && pendingEdits[param.name] == nil {
            paramValues[param.name] = param.value.number
        }
        isStale = false
        // The document evaluates again, so whatever failure an earlier apply
        // reported is over: take the overlay down. (`.loading` is load()'s to
        // end, and `.idle`/`.ready` need nothing.)
        if case .failed = phase { phase = .ready }
    }

    /// Render requests issued, and the newest one whose image is on screen.
    /// Renders are independent round trips that finish in any order; an
    /// image only replaces one from an older request, so a slow render of a
    /// view the user has left can never land on top of the current one.
    private var rendersIssued = 0
    private var renderShown = 0

    public func rerender(draft: Bool = false) async {
        var spec = RenderSpec(view: view)
        spec.width = 1200
        spec.height = 900
        spec.draft = draft
        spec.highlightSelection = !selection.isEmpty
        rendersIssued += 1
        let ticket = rendersIssued
        do {
            let result = try await engine.render(spec)
            if !result.png.isEmpty && ticket > renderShown {
                renderShown = ticket
                renderedPNG = result.png
            }
        } catch {
            // Keep the last good image; the diagnostics list says why the new
            // one did not arrive.
            isStale = true
        }
    }

    // MARK: - Parameters

    /// Called on every slider frame. Returns immediately: the request is
    /// coalesced so only the newest value reaches the engine.
    ///
    /// `setParams` replaces the whole override set, so what goes to the
    /// coalescer is the session's overrides with every edit not yet
    /// acknowledged laid over them -- never just the one slider that moved.
    /// `session` lags behind the user by a round trip, and a set built from
    /// it alone would put back the value of any other slider still in flight.
    public func setParameter(_ name: String, to value: Double) {
        paramValues[name] = value
        isStale = true
        pendingEdits[name] = .number(value)
        let overrides = session.paramOverrides.merging(pendingEdits) { _, edit in edit }
        Task { await coalescer.submit(overrides) }
    }

    /// Edits the engine has not acknowledged yet, by parameter name.
    private var pendingEdits: [String: Cadpole_V1_ParamValue] = [:]

    /// The engine has taken (or refused) `applied`: those edits are no longer
    /// pending. An edit the user has moved on from since stays.
    private func settleEdits(_ applied: [String: Cadpole_V1_ParamValue]) {
        for (name, value) in applied where pendingEdits[name] == value {
            pendingEdits[name] = nil
        }
    }

    private func applyParams(_ overrides: [String: Cadpole_V1_ParamValue]) async {
        do {
            session = try await engine.setParams(overrides)
            settleEdits(overrides)
            try await reevaluate()
            // A draft while the drag has more values queued; the full image
            // once this is the last of them, so the viewport never rests on
            // a draft.
            await rerender(draft: await coalescer.hasPending)
        } catch EngineError.cancelled {
            // Superseded by a newer value. Expected during a drag.
        } catch is CancellationError {
            // The same, noticed by Swift before the engine.
        } catch {
            settleEdits(overrides)
            phase = .failed(Self.describe(error))
        }
    }

    // MARK: - Selection

    public var entityRefs: [Cadpole_V1_EntityRef] {
        selection.compactMap { tree.node(id: $0)?.entityRef }
    }

    public var selectedNode: FeatureNode? {
        selection.count == 1 ? selection.first.flatMap { tree.node(id: $0) } : nil
    }

    private func pushSelection() async {
        do {
            session = try await engine.setSelection(entityRefs)
            await rerender()
        } catch {
            isStale = true
        }
    }

    // MARK: - Derived

    public var tree: FeatureTree {
        FeatureTree(model: model, unit: DisplayUnit(session.displayUnit))
    }

    /// Diagnostics with stable ids, so the accessibility rotor can list them.
    /// `Diagnostic` itself is a generated protobuf struct and is not
    /// `Identifiable`; its index in the model's list is the only id it has.
    public struct DiagnosticEntry: Identifiable, Equatable {
        public let id: Int
        public let message: String
        public let diagnostic: Cadpole_V1_Diagnostic
    }

    public var diagnosticEntries: [DiagnosticEntry] {
        model.diagnostics.enumerated().map { index, d in
            DiagnosticEntry(id: index, message: d.message, diagnostic: d)
        }
    }

    public var errors: [DiagnosticEntry] {
        diagnosticEntries.filter { $0.diagnostic.severity == .error }
    }

    public var warnings: [DiagnosticEntry] {
        diagnosticEntries.filter { $0.diagnostic.severity == .warning }
    }

    static func describe(_ error: any Error) -> String {
        guard let error = error as? EngineError else { return String(describing: error) }
        switch error {
        case .http(let status, let body) where status == 401:
            return "The server rotated its token. Paste the URL `cadpole serve` printed. (\(body))"
        case .http(let status, let body):
            return "The server answered \(status). \(body)"
        case .engine(let message): return message
        case .unexpectedResponse: return "The engine answered a different question."
        case .decoding(let what): return "Could not read the engine's answer: \(what)"
        case .staleRevision(let revision):
            return "Someone else changed the document (now at revision \(revision))."
        case .cancelled: return "Superseded by a newer change."
        case .unsupported(let what): return what
        }
    }
}

extension Cadpole_V1_ParamValue {
    var isNumber: Bool {
        if case .number = value { return true }
        return false
    }
}
