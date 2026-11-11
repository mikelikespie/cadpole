// Which result the screen shows when results arrive out of order.
//
// Remote renders are independent HTTP round trips, so they finish in any
// order. The screen must end on the newest request's image, the slider must
// not jump back to a value the user has already left, and the last apply of a
// drag must leave a full-quality image rather than the draft.

import Foundation
import CadpoleKit
import CadpoleProto
import XCTest

@MainActor
final class RenderOrderTests: XCTestCase {
    private func png(_ view: String, draft: Bool) -> Data {
        ScriptedEngine.Render(view: view, draft: draft).png
    }

    func testASlowRenderOfAnOldViewDoesNotReplaceTheNewOne() async {
        let engine = ScriptedEngine()
        let (model, task) = await loadedModel(engine)
        defer { task.cancel(); engine.shutDown() }

        engine.hold("render:front")
        model.view = "front"
        await eventually("the front render to start") { engine.waitingCount("render:front") == 1 }
        model.view = "top"
        await eventually("the top image") { model.renderedPNG == self.png("top", draft: false) }

        // The front render finishes last.
        engine.releaseAll("render:front")
        await eventually("the front render to finish") {
            engine.rendersFinished.contains(.init(view: "front", draft: false))
        }
        await settle()
        XCTAssertEqual(model.view, "top")
        XCTAssertEqual(
            model.renderedPNG.map { String(decoding: $0, as: UTF8.self) },
            "view=top draft=false",
            "the screen shows a view the user has left"
        )
    }

    func testTheSliderDoesNotJumpBackWhileAnOlderValueIsApplied() async {
        let engine = ScriptedEngine()
        let (model, task) = await loadedModel(engine)
        defer { task.cancel(); engine.shutDown() }

        engine.hold("setParams")
        model.setParameter("wall", to: 3)
        await eventually("the first apply to reach the engine") { engine.waitingCount("setParams") == 1 }
        model.setParameter("wall", to: 9)
        await settle(.milliseconds(20))

        let evaluatesBefore = engine.evaluates
        engine.releaseOne("setParams")  // the apply of 3 finishes; 9 is still to come
        await eventually("the re-evaluation of 3") { engine.evaluates > evaluatesBefore }
        await settle(.milliseconds(50))
        XCTAssertEqual(model.paramValues["wall"], 9, "the thumb jumped back to a superseded value")

        engine.releaseAll("setParams")
        await eventually("9 to be applied") { engine.overrides["wall"] == .number(9) }
        await settle()
        XCTAssertEqual(model.paramValues["wall"], 9)
    }

    func testTheLastApplyLeavesAFullQualityImage() async {
        let engine = ScriptedEngine()
        let (model, task) = await loadedModel(engine)
        defer { task.cancel(); engine.shutDown() }

        model.setParameter("wall", to: 3)
        await eventually("the apply") { engine.overrides["wall"] == .number(3) }
        await settle()
        XCTAssertEqual(
            model.renderedPNG.map { String(decoding: $0, as: UTF8.self) },
            "view=iso draft=false",
            "a draft is for while the slider moves, not the resting image"
        )
    }

    func testADragRendersDraftsWhileMoreIsPending() async {
        let engine = ScriptedEngine()
        let (model, task) = await loadedModel(engine)
        defer { task.cancel(); engine.shutDown() }
        let rendersBefore = engine.rendersStarted.count

        engine.hold("setParams")
        model.setParameter("wall", to: 3)
        await eventually("the first apply to reach the engine") { engine.waitingCount("setParams") == 1 }
        model.setParameter("wall", to: 4)
        await settle(.milliseconds(20))
        engine.releaseAll("setParams")
        await eventually("4 to be applied") { engine.overrides["wall"] == .number(4) }
        await settle()

        let renders = Array(engine.rendersStarted.dropFirst(rendersBefore))
        XCTAssertEqual(renders.first?.draft, true, "a value with another behind it renders a draft")
        XCTAssertEqual(renders.last?.draft, false)
    }
}
