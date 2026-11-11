// The coalescing rule: the thing that decides whether a slider drag feels
// immediate or drags a queue behind the user's finger.

import Foundation
import CadpoleKit
import CadpoleProto
import XCTest

final class ParamCoalescerTests: XCTestCase {
    /// Records what actually reached the engine.
    private actor Sink {
        var applied: [Double] = []
        var concurrent = 0
        var maxConcurrent = 0

        func begin(_ v: Double) {
            applied.append(v)
            concurrent += 1
            maxConcurrent = max(maxConcurrent, concurrent)
        }

        func end() { concurrent -= 1 }
    }

    func testOnlyTheNewestValueSurvivesADrag() async {
        let sink = Sink()
        let coalescer = ParamCoalescer { overrides in
            let v = overrides["wall"]?.number ?? 0
            await sink.begin(v)
            // Long enough that the submissions behind it pile up.
            try? await Task.sleep(nanoseconds: 2_000_000)
            await sink.end()
        }

        for i in 1 ... 60 {
            await coalescer.submit(["wall": .number(Double(i))])
        }
        await coalescer.waitUntilIdle()

        let applied = await sink.applied
        XCTAssertEqual(applied.last, 60, "the newest value must always be applied")
        XCTAssertLessThan(applied.count, 60, "intermediate values must be dropped, not queued")
        let ran = await coalescer.appliedCount
        XCTAssertEqual(ran, applied.count)
    }

    func testNeverMoreThanOneRequestInFlight() async {
        let sink = Sink()
        let coalescer = ParamCoalescer { overrides in
            await sink.begin(overrides["wall"]?.number ?? 0)
            try? await Task.sleep(nanoseconds: 1_000_000)
            await sink.end()
        }
        for i in 1 ... 25 {
            await coalescer.submit(["wall": .number(Double(i))])
        }
        await coalescer.waitUntilIdle()
        let peak = await sink.maxConcurrent
        XCTAssertEqual(peak, 1)
    }

    func testASingleSubmissionStillRuns() async {
        let sink = Sink()
        let coalescer = ParamCoalescer { overrides in
            await sink.begin(overrides["wall"]?.number ?? 0)
            await sink.end()
        }
        await coalescer.submit(["wall": .number(3.5)])
        await coalescer.waitUntilIdle()
        let applied = await sink.applied
        XCTAssertEqual(applied, [3.5])
    }

    func testWaitUntilIdleReturnsImmediatelyWhenNothingIsPending() async {
        let coalescer = ParamCoalescer { _ in }
        await coalescer.waitUntilIdle()
        let count = await coalescer.appliedCount
        XCTAssertEqual(count, 0)
    }

    func testDisplayStringsUseTheDocumentsVocabulary() {
        XCTAssertEqual(Cadpole_V1_ParamValue.number(2).displayString, "2")
        XCTAssertEqual(Cadpole_V1_ParamValue.number(2.4).displayString, "2.4")
        XCTAssertEqual(Cadpole_V1_ParamValue.boolean(true).displayString, "on")
        XCTAssertEqual(Cadpole_V1_ParamValue.text("left").displayString, "left")
    }
}
