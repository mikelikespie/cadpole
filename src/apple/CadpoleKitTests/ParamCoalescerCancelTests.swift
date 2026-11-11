// The coalescer in local mode: a newer value cancels the apply
// in flight instead of waiting for it.

import Foundation
import CadpoleKit
import CadpoleProto
import XCTest

final class ParamCoalescerCancelTests: XCTestCase {
    private actor Log {
        var started: [Double] = []
        var cancelled: [Double] = []
        var finished: [Double] = []
        func start(_ v: Double) { started.append(v) }
        func cancel(_ v: Double) { cancelled.append(v) }
        func finish(_ v: Double) { finished.append(v) }
    }

    /// An apply that runs until it is cancelled, or for `limit` at most.
    private func slowApply(_ log: Log, limit: Duration = .seconds(2)) -> ParamCoalescer.Apply {
        { overrides in
            let v = overrides["wall"]?.number ?? 0
            await log.start(v)
            let deadline = ContinuousClock.now + limit
            while ContinuousClock.now < deadline {
                if Task.isCancelled {
                    await log.cancel(v)
                    return
                }
                try? await Task.sleep(nanoseconds: 1_000_000)
                if v != 1 { break }  // only the first value is slow
            }
            await log.finish(v)
        }
    }

    private func waitForStart(_ log: Log) async {
        let deadline = ContinuousClock.now + .seconds(2)
        while await log.started.isEmpty, ContinuousClock.now < deadline {
            try? await Task.sleep(nanoseconds: 1_000_000)
        }
    }

    func testANewerValueCancelsTheApplyInFlight() async {
        let log = Log()
        let coalescer = ParamCoalescer(cancelsInFlight: true, apply: slowApply(log))
        await coalescer.submit(["wall": .number(1)])
        await waitForStart(log)
        await coalescer.submit(["wall": .number(2)])
        await coalescer.waitUntilIdle()

        let cancelled = await log.cancelled
        let finished = await log.finished
        XCTAssertEqual(cancelled, [1], "the superseded apply was left to run")
        XCTAssertEqual(finished, [2], "the newest value is still applied")
    }

    func testByDefaultTheApplyInFlightFinishes() async {
        let log = Log()
        let coalescer = ParamCoalescer(apply: slowApply(log, limit: .milliseconds(50)))
        await coalescer.submit(["wall": .number(1)])
        await waitForStart(log)
        await coalescer.submit(["wall": .number(2)])
        await coalescer.waitUntilIdle()

        let cancelled = await log.cancelled
        let finished = await log.finished
        XCTAssertEqual(cancelled, [])
        XCTAssertEqual(finished, [1, 2])
    }

    /// Submissions faster than an apply finishes: cancelling every apply
    /// would starve the viewport (nothing completes while the finger moves)
    /// and pile up abandoned evaluations. Some intermediate value must
    /// complete, and no two applies in a row are cancelled.
    func testAStreamOfSubmissionsNeverCancelsTwoAppliesInARow() async {
        actor Order {
            var events: [(Double, Bool)] = []  // (value, cancelled), in end order
            func end(_ v: Double, cancelled: Bool) { events.append((v, cancelled)) }
        }
        let order = Order()
        let coalescer = ParamCoalescer(cancelsInFlight: true) { overrides in
            let v = overrides["wall"]?.number ?? 0
            let deadline = ContinuousClock.now + .milliseconds(40)
            while ContinuousClock.now < deadline {
                if Task.isCancelled {
                    await order.end(v, cancelled: true)
                    return
                }
                try? await Task.sleep(nanoseconds: 1_000_000)
            }
            await order.end(v, cancelled: false)
        }
        let last = 60.0
        for i in 1...Int(last) {
            await coalescer.submit(["wall": .number(Double(i))])
            try? await Task.sleep(nanoseconds: 5_000_000)
        }
        await coalescer.waitUntilIdle()

        let events = await order.events
        XCTAssertEqual(events.last?.0, last, "the newest value is applied last")
        XCTAssertEqual(events.last?.1, false)
        XCTAssertTrue(events.dropLast().contains { !$0.1 }, "an intermediate value completed during the stream: \(events)")
        for (a, b) in zip(events, events.dropFirst()) {
            XCTAssertFalse(a.1 && b.1, "two applies in a row were cancelled: \(events)")
        }
    }
}
