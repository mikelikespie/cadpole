// `EngineClient` over the C ABI: macOS self-hosted, no server.
//
// The engine is `libcadpole_ffi.a` linked into the app, and this type is the
// Swift session a server would otherwise supply: revisions, the event log,
// the selection, the parameter overrides. That session is a known liability --
// it is a second implementation of `cadpole-session`'s rules -- and the preferred
// shape is to compile `cadpole-session` into the staticlib and make this a pure
// byte pump (docs/apple-apps.md). Everything above `EngineClient` is
// already written as if that had happened.
//
// Threading, which is the part that goes wrong quietly: a `cadpole_call` can block
// for the whole interactive deadline, and Swift actors run on the cooperative
// pool -- one thread per core -- so blocking one freezes the app rather than
// the viewport. Calls therefore run on a dedicated *serial* DispatchQueue
// (serial because the engine's memoisation cache is shared mutable state) and
// bridge back with withCheckedThrowingContinuation. Raw pointers never leave
// that queue.

import Dispatch
import Foundation
import CadpoleFFI
import CadpoleKit
import CadpoleProto

public final class LocalEngine: EngineClient, @unchecked Sendable {
    /// The version of `cadpole-ffi` these Swift protos were generated from.
    /// Rule 4 of the ABI: a mismatch is fatal, not a warning. Bump together
    /// with CADPOLE_VERSION in //src/rust/cadpole-ffi/BUILD.bazel (which is what
    /// sets CARGO_PKG_VERSION for a Bazel build -- rules_rust does not read
    /// Cargo.toml) and //src/apple:CadpoleVersion.
    public static let expectedEngineVersion = "0.1.0"

    /// What `cadpole_version()` reports from the linked static library.
    public static var engineVersion: String {
        String(cString: cadpole_version())
    }

    public enum LaunchError: Error, Equatable {
        /// The linked engine is not the one the protos were generated from.
        case versionMismatch(linked: String, expected: String)
    }

    /// No `.assistant`: the assistant workflow *is* the session server, and
    /// self-hosted has none. The app says so on opening a local file rather
    /// than greying a button out (docs/apple-apps.md).
    public let capabilities: EngineCapabilities = [.localEvaluation, .pathTrace, .stepExport, .assets]

    /// Every write through this engine is this client's, as every request a
    /// `RemoteEngine` sends carries its `X-Cadpole-Client`. Events are
    /// stamped with it so the screen that made a write skips its echo.
    public let clientID = UUID().uuidString

    private let queue = DispatchQueue(label: "io.github.mikelikespie.cadpole.LocalEngine")
    private let lock = NSLock()
    private var snapshot: SessionSnapshot
    /// The document's assets. `SessionSnapshot` mirrors the server's JSON view,
    /// which names assets without their bytes, so they are kept beside it and
    /// put back into every document this engine evaluates.
    private let assets: [Cadpole_V1_Asset]
    private var log: [EngineEvent] = []
    private var listeners: [UUID: AsyncThrowingStream<EngineEvent, any Error>.Continuation] = [:]
    private var nextToken: UInt64 = 1

    /// - Parameter requiringEngineVersion: what `cadpole_version()` must report.
    ///   `nil` skips the check; nothing in the app passes `nil`.
    /// - Throws: `LaunchError.versionMismatch` when they differ.
    public init(
        document: Cadpole_V1_Document,
        requiringEngineVersion expected: String? = LocalEngine.expectedEngineVersion
    ) throws {
        if let expected {
            let linked = Self.engineVersion
            guard linked == expected else {
                throw LaunchError.versionMismatch(linked: linked, expected: expected)
            }
        }
        snapshot = SessionSnapshot(
            id: document.id.isEmpty ? UUID().uuidString : document.id,
            revision: 1,
            documentName: document.name,
            displayUnit: DisplayUnit(document.displayUnit).rawValue,
            scripts: document.scripts,
            paramOverrides: document.paramOverrides,
            selection: document.selection.selected,
            lastEventSeq: 0
        )
        assets = document.assets
    }

    // MARK: - EngineClient

    public func session() async throws -> SessionSnapshot {
        lock.withLockedValue { snapshot }
    }

    public func document() async throws -> Cadpole_V1_Document {
        var doc = lock.withLockedValue { snapshot }.asDocument()
        doc.assets = assets
        return doc
    }

    public func call(_ request: Cadpole_V1_Request) async throws -> Cadpole_V1_Response {
        // Every call gets a token, so every call is bounded by whatever
        // deadline the request carries and ends when the calling task is
        // cancelled. Only a caller that cancels by token needs to know it.
        try await call(request, cancellationToken: newCancellationToken())
    }

    /// The cancellable form. Pass a token from `newCancellationToken()` and
    /// hand the same token to `cancel(_:)` when a newer request supersedes
    /// this one; that is what a slider drag does.
    ///
    /// Cancelling the calling Swift task cancels the token too, so a caller
    /// that cancels its task (the parameter coalescer, a closed screen) is
    /// released with `EngineError.cancelled` instead of waiting out the
    /// evaluation. A cancel that lands before the call starts still wins.
    public func call(_ request: Cadpole_V1_Request, cancellationToken token: UInt64) async throws -> Cadpole_V1_Response {
        let bytes = try request.serializedData()
        let responseBytes = try await withTaskCancellationHandler {
            try await withCheckedThrowingContinuation { continuation in
                queue.async {
                    do {
                        continuation.resume(returning: try Self.invoke(bytes, token: token))
                    } catch {
                        continuation.resume(throwing: error)
                    }
                }
            }
        } onCancel: {
            self.cancel(token)
        }
        do {
            return try Cadpole_V1_Response(serializedData: responseBytes)
        } catch {
            throw EngineError.decoding("response: \(error)")
        }
    }

    public func newCancellationToken() -> UInt64 {
        lock.withLockedValue {
            nextToken += 1
            return nextToken
        }
    }

    /// Abandon the call running under `token`. Safe before it starts, after it
    /// finishes, and for a token that never existed.
    public func cancel(_ token: UInt64) {
        _ = cadpole_cancel(token)
    }

    @discardableResult
    public func setParams(_ overrides: [String: Cadpole_V1_ParamValue]) async throws -> SessionSnapshot {
        let (updated, event) = lock.withLockedValue { () -> (SessionSnapshot, EngineEvent) in
            snapshot.paramOverrides = overrides
            snapshot.revision += 1
            return (snapshot, makeEvent(kind: "params_changed"))
        }
        broadcast(event)
        return updated
    }

    @discardableResult
    public func setSelection(_ refs: [Cadpole_V1_EntityRef]) async throws -> SessionSnapshot {
        let (updated, event) = lock.withLockedValue { () -> (SessionSnapshot, EngineEvent) in
            snapshot.selection = refs
            return (snapshot, makeEvent(kind: "selection_changed"))
        }
        broadcast(event)
        return updated
    }

    /// Replace one script and bump the revision. `baseRevision` is the
    /// optimistic-concurrency check the server does; here the only other writer
    /// is the app itself, but the check keeps undo honest.
    @discardableResult
    public func setScript(path: String, source: String, baseRevision: UInt64?) async throws -> SessionSnapshot {
        let (updated, event) = try lock.withLockedValue { () -> (SessionSnapshot, EngineEvent) in
            if let baseRevision, baseRevision != snapshot.revision {
                throw EngineError.staleRevision(current: snapshot.revision)
            }
            if let i = snapshot.scripts.firstIndex(where: { $0.path == path }) {
                snapshot.scripts[i].source = source
            } else {
                var file = Cadpole_V1_ScriptFile()
                file.path = path
                file.source = source
                snapshot.scripts.append(file)
            }
            snapshot.revision += 1
            return (snapshot, makeEvent(kind: "script_changed"))
        }
        broadcast(event)
        return updated
    }

    public func events(after seq: UInt64) -> AsyncThrowingStream<EngineEvent, any Error> {
        AsyncThrowingStream { continuation in
            let id = UUID()
            let backlog = lock.withLockedValue { () -> [EngineEvent] in
                listeners[id] = continuation
                return log.filter { $0.seq > seq }
            }
            for event in backlog { continuation.yield(event) }
            continuation.onTermination = { [weak self] _ in
                self?.lock.withLockedValue { self?.listeners[id] = nil }
            }
        }
    }

    // MARK: - The C ABI

    /// Runs on `queue`. Everything unsafe is contained here.
    private static func invoke(_ request: Data, token: UInt64) throws -> Data {
        var out: UnsafeMutablePointer<UInt8>?
        var outLen = 0
        let status = request.withUnsafeBytes { raw -> Int32 in
            // Rule 1: the request bytes stay Swift's; Rust only reads them for
            // the duration of the call.
            let base = raw.bindMemory(to: UInt8.self).baseAddress
            return cadpole_call_cancellable(base, raw.count, &out, &outLen, token)
        }
        switch status {
        case CADPOLE_OK:
            guard let out, outLen > 0 else {
                // A zero-length response is not something the engine produces;
                // treat it as a protocol violation rather than decoding it.
                if let out { cadpole_free(out, outLen) }
                throw EngineError.engine("engine returned an empty response")
            }
            // Rule 2: transferred exactly once, freed by the deallocator and
            // never by hand -- which makes a double free structurally
            // impossible.
            return Data(
                bytesNoCopy: out,
                count: outLen,
                deallocator: .custom { pointer, length in
                    cadpole_free(pointer.assumingMemoryBound(to: UInt8.self), length)
                }
            )
        case CADPOLE_ERR_CANCELLED, CADPOLE_ERR_DEADLINE:
            // Rule 3: *out was not written; there is nothing to free.
            throw EngineError.cancelled
        case CADPOLE_ERR_PANIC:
            throw EngineError.engine("the engine panicked; this document's state is suspect")
        default:
            throw EngineError.engine("cadpole_call_cancellable returned \(status)")
        }
    }

    // MARK: - The session a server would own

    /// Caller holds `lock`.
    private func makeEvent(kind: String) -> EngineEvent {
        let seq = (log.last?.seq ?? 0) + 1
        let event = EngineEvent(
            seq: seq,
            at: ISO8601DateFormatter().string(from: Date()),
            kind: kind,
            dataJSON: Data("{}".utf8),
            by: clientID
        )
        log.append(event)
        if log.count > 10_000 { log.removeFirst(log.count - 10_000) }
        snapshot.lastEventSeq = seq
        return event
    }

    private func broadcast(_ event: EngineEvent) {
        let listeners = lock.withLockedValue { Array(self.listeners.values) }
        for listener in listeners { listener.yield(event) }
    }
}

extension NSLock {
    /// `withLock` under a different name: Foundation grew one at different
    /// versions on Darwin and on swift-corelibs, and a clash is a build break
    /// rather than a warning.
    func withLockedValue<T>(_ body: () throws -> T) rethrows -> T {
        lock()
        defer { unlock() }
        return try body()
    }
}
