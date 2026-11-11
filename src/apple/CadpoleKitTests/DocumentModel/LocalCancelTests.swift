// Local mode cancels a superseded evaluation: with the engine in-process, the newest slider value
// should not wait behind a full evaluation of one the user has already left.
// Remote mode does not cancel; the server keeps computing whatever it was
// asked, and only the newest value is sent next.

import Foundation
import CadpoleKit
import CadpoleProto
import XCTest

@MainActor
final class LocalCancelTests: XCTestCase {
    func testLocalModeCancelsASupersededEvaluation() async {
        let engine = ScriptedEngine(capabilities: [.localEvaluation])
        let (model, task) = await loadedModel(engine)
        defer { task.cancel(); engine.shutDown() }

        engine.hold("evaluate")
        model.setParameter("wall", to: 3)
        await eventually("the evaluation of 3 to start") { engine.waitingCount("evaluate") == 1 }
        model.setParameter("wall", to: 4)
        await eventually("the evaluation of 3 to be cancelled", timeout: .seconds(2)) {
            engine.cancelledEvaluates == 1
        }
        engine.releaseAll("evaluate")
        await eventually("4 to be applied") { engine.overrides["wall"] == .number(4) }
        await settle()
        XCTAssertEqual(model.paramValues["wall"], 4)
        XCTAssertEqual(model.phase, .ready, "a cancellation is not a failure")
    }

    func testRemoteModeLetsTheInFlightEvaluationFinish() async {
        let engine = ScriptedEngine(capabilities: [.pathTrace])
        let (model, task) = await loadedModel(engine)
        defer { task.cancel(); engine.shutDown() }

        engine.hold("evaluate")
        model.setParameter("wall", to: 3)
        await eventually("the evaluation of 3 to start") { engine.waitingCount("evaluate") == 1 }
        model.setParameter("wall", to: 4)
        await settle(.milliseconds(100))
        XCTAssertEqual(engine.cancelledEvaluates, 0)
        engine.releaseAll("evaluate")
        await eventually("4 to be applied") { engine.overrides["wall"] == .number(4) }
    }
}
