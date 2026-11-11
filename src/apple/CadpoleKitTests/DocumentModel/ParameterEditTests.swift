// Two sliders, one round trip.
//
// Every engine's `setParams` REPLACES the override set, so whatever the model
// submits has to be the whole set the user wants. Building it from the last
// session the server acknowledged loses any edit still in flight: move `wall`,
// then `height` before the first apply returns, and `wall` goes back to its
// default.

import Foundation
import CadpoleKit
import CadpoleProto
import XCTest

@MainActor
final class ParameterEditTests: XCTestCase {
    func testASecondSliderDoesNotRevertTheFirst() async {
        let engine = ScriptedEngine()
        let (model, task) = await loadedModel(engine)
        defer { task.cancel(); engine.shutDown() }

        engine.hold("setParams")
        model.setParameter("wall", to: 3)
        await eventually("the first apply to reach the engine") { engine.waitingCount("setParams") == 1 }
        // The first round trip is still out; the user moves a second slider.
        model.setParameter("height", to: 30)
        await settle(.milliseconds(20))
        engine.releaseAll("setParams")

        await eventually("height to be applied") { engine.overrides["height"] == .number(30) }
        await settle()
        XCTAssertEqual(engine.overrides["wall"], .number(3), "the first slider's value was lost")
        XCTAssertEqual(engine.overrides["height"], .number(30))
        XCTAssertEqual(model.paramValues["wall"], 3)
        XCTAssertEqual(model.paramValues["height"], 30)
    }

    /// The mid-drag variant: a newer frame of the first slider is waiting in
    /// the coalescer when the second slider moves. The newest `wall` must
    /// survive, not an earlier frame of it.
    func testASecondSliderKeepsTheFirstSlidersNewestFrame() async {
        let engine = ScriptedEngine()
        let (model, task) = await loadedModel(engine)
        defer { task.cancel(); engine.shutDown() }

        engine.hold("setParams")
        model.setParameter("wall", to: 3)
        await eventually("the first apply to reach the engine") { engine.waitingCount("setParams") == 1 }
        model.setParameter("wall", to: 4)
        model.setParameter("height", to: 30)
        await settle(.milliseconds(20))
        engine.releaseAll("setParams")

        await eventually("height to be applied") { engine.overrides["height"] == .number(30) }
        await settle()
        XCTAssertEqual(engine.overrides["wall"], .number(4))
        XCTAssertEqual(engine.overrides["height"], .number(30))
    }

    /// The override set is the session's plus our edits: an override another
    /// writer set earlier is carried along, not dropped.
    func testAnEditKeepsTheOverridesAlreadyInTheSession() async throws {
        let engine = ScriptedEngine()
        _ = try await engine.setParams(["height": .number(80)])
        let (model, task) = await loadedModel(engine)
        defer { task.cancel(); engine.shutDown() }

        model.setParameter("wall", to: 5)
        await eventually("wall to be applied") { engine.overrides["wall"] == .number(5) }
        XCTAssertEqual(engine.overrides["height"], .number(80))
    }
}
