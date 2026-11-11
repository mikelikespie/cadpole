// The coalescing rule on the client: only the newest evaluation runs.
//
// A slider drag produces a value per frame. Sending them all queues work behind
// the user's finger and the viewport lags further the longer they drag. So:
// one request in flight, the newest pending value replacing any earlier one,
// and everything in between dropped. Remote mode coalesces against the server;
// local mode coalesces the same way and additionally cancels the in-flight
// evaluation (`cancelsInFlight`: the apply's task is cancelled, which
// LocalEngine turns into cadpole_cancel), which is the whole reason the C ABI
// grew cadpole_call_cancellable.
//
// A cancel releases the caller, not the CPU: the engine's abandoned
// evaluation runs to its end on its own thread. So the coalescer never
// cancels two applies in a row. The apply after a cancelled one runs to
// completion whatever arrives meanwhile, which keeps the viewport updating
// during a drag of a slow model and keeps abandoned evaluations to about one
// at a time.
//
// This is a plain actor with no Apple dependency, so the Linux test pins the
// behaviour that is easy to get subtly wrong.

import Foundation
import CadpoleProto

public actor ParamCoalescer {
    public typealias Apply = @Sendable ([String: Cadpole_V1_ParamValue]) async -> Void

    private let apply: Apply
    private let cancelsInFlight: Bool
    private var pending: [String: Cadpole_V1_ParamValue]?
    private var running = false
    /// The apply running now, so a newer submission can cancel it.
    private var inFlight: Task<Void, Never>?
    /// The previous apply was cancelled (or the running one already is), so
    /// the next one is not: at most one cancelled apply in a row.
    private var cancelledLast = false
    /// Woken when the queue drains; only tests and "wait for quiescence" use it.
    private var idleWaiters: [CheckedContinuation<Void, Never>] = []

    /// How many times `apply` actually ran. The point of the type is that this
    /// is much smaller than the number of `submit` calls.
    public private(set) var appliedCount = 0

    /// - Parameter cancelsInFlight: cancel the running apply's task when a
    ///   newer value arrives, instead of letting it finish. For an engine
    ///   whose calls honour task cancellation (LocalEngine); the apply must
    ///   treat cancellation as "superseded", not as a failure.
    public init(cancelsInFlight: Bool = false, apply: @escaping Apply) {
        self.apply = apply
        self.cancelsInFlight = cancelsInFlight
    }

    /// Ask for these overrides. Returns immediately; the newest submission
    /// wins and intermediate ones are dropped, never queued.
    public func submit(_ overrides: [String: Cadpole_V1_ParamValue]) {
        pending = overrides
        guard !running else {
            if cancelsInFlight, !cancelledLast, let task = inFlight {
                task.cancel()
                cancelledLast = true
            }
            return
        }
        running = true
        Task { await self.drain() }
    }

    private func drain() async {
        while let next = pending {
            pending = nil
            appliedCount += 1
            let apply = self.apply
            let task = Task { await apply(next) }
            inFlight = task
            await task.value
            inFlight = nil
            // An apply that ran to its end lets the next one be cancelled.
            if !task.isCancelled { cancelledLast = false }
        }
        running = false
        let waiters = idleWaiters
        idleWaiters.removeAll()
        for w in waiters { w.resume() }
    }

    /// Whether a newer submission is waiting behind the one being applied.
    /// An apply asks this to choose a draft render (more is coming) or a
    /// full one (it is the last).
    public var hasPending: Bool { pending != nil }

    /// Suspends until nothing is in flight and nothing is pending.
    public func waitUntilIdle() async {
        guard running || pending != nil else { return }
        await withCheckedContinuation { (c: CheckedContinuation<Void, Never>) in
            idleWaiters.append(c)
        }
    }
}
