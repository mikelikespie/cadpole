// A failed apply is reported, and the next good evaluation takes the report
// down again. The "Cannot evaluate" overlay must not outlive the failure that
// raised it.

import Foundation
import CadpoleKit
import CadpoleProto
import XCTest

@MainActor
final class FailureRecoveryTests: XCTestCase {
    func testTheNextGoodApplyClearsAFailedOne() async {
        let engine = ScriptedEngine()
        let (model, task) = await loadedModel(engine)
        defer { task.cancel(); engine.shutDown() }

        engine.failNextSetParams(.http(status: 503, body: "server restarting"))
        model.setParameter("wall", to: 3)
        await eventually("the failure to be reported") {
            if case .failed = model.phase { return true } else { return false }
        }

        model.setParameter("wall", to: 4)
        await eventually("4 to be applied") { engine.overrides["wall"] == .number(4) }
        await settle()
        XCTAssertEqual(model.phase, .ready, "a document that evaluates again is not failed")
    }

    func testAnotherWritersChangeAlsoClearsIt() async {
        let engine = ScriptedEngine()
        let (model, task) = await loadedModel(engine)
        defer { task.cancel(); engine.shutDown() }

        engine.failNextSetParams(.http(status: 503, body: "server restarting"))
        model.setParameter("wall", to: 3)
        await eventually("the failure to be reported") {
            if case .failed = model.phase { return true } else { return false }
        }

        engine.externalWrite(kind: "script_changed", by: "assistant")
        await eventually("the document to be ready again") { model.phase == .ready }
    }
}
