// Staying connected: the retry ladder, and the state the toolbar pill shows.
//
// docs/apple-apps.md designs that pill as `disconnected → resolving →
// connecting → live(revision) → degraded(retrying since T) → unauthorized`.
// `RemoteEngine`'s event-stream loop is the only code that knows which of
// those is true, so it is what drives this; `DocumentModel` subscribes and
// re-reads the session whenever a reconnection succeeds, because a server that
// restarted while we were away has a brand new event log and nothing in the
// stream can say what changed in between.

import Foundation

/// How long to wait before each reconnection attempt.
///
/// 1 s doubling to 10 s is the web client's ladder (`subscribe` in
/// `src/web/src/api.ts`), so a laptop that slept and a phone that changed
/// network behave the same way in both clients.
public struct ReconnectPolicy: Sendable, Equatable {
    /// The wait before the first retry.
    public var initial: Duration
    /// The ceiling the wait doubles towards.
    public var maximum: Duration

    public init(initial: Duration, maximum: Duration) {
        self.initial = initial
        self.maximum = maximum
    }

    public static let `default` = ReconnectPolicy(initial: .seconds(1), maximum: .seconds(10))

    /// Retry at once. The Linux tests use it, so a dozen reconnections cost
    /// microseconds instead of a minute.
    public static let immediate = ReconnectPolicy(initial: .zero, maximum: .zero)

    /// The wait after `failures` consecutive failed connections; `failures` is
    /// 1 for the first retry.
    public func delay(afterFailures failures: Int) -> Duration {
        guard failures > 1 else { return Swift.min(initial, maximum) }
        var delay = initial
        for _ in 1 ..< failures {
            delay = delay * 2
            if delay >= maximum { return maximum }
        }
        return Swift.min(delay, maximum)
    }
}

/// Tells a reconnection apart from the pill merely updating.
///
/// `ConnectionState.live(revision:)` changes on every session read that sees
/// a new revision -- including the one right after this scene's own write --
/// so "a `.live` after the first" is not the same thing as "the connection
/// came back". Only a `.live` that follows a non-live state, after the first
/// time the client was live, is a reconnection; that is the one that has to
/// read the session again.
public struct ReconnectionDetector: Sendable {
    private var wasLive = false
    private var everLive = false

    public init() {}

    /// Feed every state in order; true exactly when this one is a
    /// reconnection.
    public mutating func observe(_ state: ConnectionState) -> Bool {
        let live: Bool
        if case .live = state { live = true } else { live = false }
        defer {
            wasLive = live
            everLive = everLive || live
        }
        return live && !wasLive && everLive
    }
}

/// Broadcasts the newest `ConnectionState` to every observer, and hands a new
/// observer the current state at once so a view that subscribes late still
/// draws the right pill.
///
/// A lock rather than an actor: `send` is called from the stream's task and
/// `stream()` from whatever task draws the UI, and neither should have to be
/// `async` to say what it already knows.
final class ConnectionMonitor: @unchecked Sendable {
    private let lock = NSLock()
    private var state: ConnectionState = .disconnected
    private var revision: UInt64 = 0
    private var observers: [UUID: AsyncStream<ConnectionState>.Continuation] = [:]

    var current: ConnectionState {
        lock.lock()
        defer { lock.unlock() }
        return state
    }

    func send(_ next: ConnectionState) {
        let waiting: [AsyncStream<ConnectionState>.Continuation] = {
            lock.lock()
            defer { lock.unlock() }
            guard next != state else { return [] }
            state = next
            return Array(observers.values)
        }()
        for observer in waiting { observer.yield(next) }
    }

    /// `live`, at the newest revision anyone has read.
    func markLive() {
        lock.lock()
        let seen = revision
        lock.unlock()
        send(.live(revision: seen))
    }

    /// Every session read reports what it saw, so the pill names the
    /// document's revision rather than an event sequence number.
    func noteRevision(_ next: UInt64) {
        let live: Bool = {
            lock.lock()
            defer { lock.unlock() }
            revision = next
            if case .live = state { return true }
            return false
        }()
        if live { send(.live(revision: next)) }
    }

    func stream() -> AsyncStream<ConnectionState> {
        AsyncStream { continuation in
            let id = UUID()
            lock.lock()
            observers[id] = continuation
            let now = state
            lock.unlock()
            continuation.yield(now)
            continuation.onTermination = { [weak self] _ in
                guard let self else { return }
                self.lock.lock()
                self.observers[id] = nil
                self.lock.unlock()
            }
        }
    }
}
