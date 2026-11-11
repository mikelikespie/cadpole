// What one slider apply costs.
//
// The engine echoes every write onto the event stream, attributed to the
// client that made it, and a session read reports the new revision to the
// connection pill. Neither is news to the scene that made the write: it has
// already evaluated and rendered the result. Reacting to them anyway tripled
// the work of every apply (three evaluates, three renders).

import Foundation
import CadpoleKit
import CadpoleProto
import XCTest

@MainActor
final class OwnWriteTests: XCTestCase {
    func testAnApplyCostsOneEvaluateAndOneRender() async {
        let engine = ScriptedEngine()
        engine.echoWrites = true
        let (model, task) = await loadedModel(engine)
        defer { task.cancel(); engine.shutDown() }
        let evaluates = engine.evaluates
        let renders = engine.rendersStarted.count

        model.setParameter("wall", to: 3)
        await eventually("the apply") { engine.overrides["wall"] == .number(3) }
        // The server's revision moved; the pill hears about it.
        engine.sendConnection(.live(revision: 2))
        await settle(.milliseconds(300))

        XCTAssertEqual(engine.evaluates - evaluates, 1, "our own write was evaluated again")
        XCTAssertEqual(engine.rendersStarted.count - renders, 1, "our own write was rendered again")
    }

    func testAnotherWritersChangeIsFollowed() async {
        let engine = ScriptedEngine()
        let (model, task) = await loadedModel(engine)
        defer { task.cancel(); engine.shutDown() }
        let evaluates = engine.evaluates

        engine.externalWrite(kind: "params_changed", by: "assistant")
        await eventually("the other writer's change to be evaluated") { engine.evaluates == evaluates + 1 }
        await settle()
        XCTAssertEqual(engine.evaluates, evaluates + 1)
        _ = model
    }

    func testAServerWriteWithNoAuthorIsFollowed() async {
        let engine = ScriptedEngine()
        let (model, task) = await loadedModel(engine)
        defer { task.cancel(); engine.shutDown() }
        let evaluates = engine.evaluates

        engine.externalWrite(kind: "script_changed", by: "")
        await eventually("the change to be evaluated") { engine.evaluates == evaluates + 1 }
        _ = model
    }

    func testARevisionBumpIsNotAReconnection() async {
        let engine = ScriptedEngine()
        let (model, task) = await loadedModel(engine)
        defer { task.cancel(); engine.shutDown() }
        let evaluates = engine.evaluates

        engine.sendConnection(.live(revision: 7))
        await settle(.milliseconds(300))
        XCTAssertEqual(engine.evaluates, evaluates, "a new revision while live is not a reconnection")
        XCTAssertEqual(model.connection, .live(revision: 7), "the pill still shows it")
    }

    func testAReconnectionReadsEverythingAgain() async {
        let engine = ScriptedEngine()
        let (model, task) = await loadedModel(engine)
        defer { task.cancel(); engine.shutDown() }
        let evaluates = engine.evaluates

        engine.sendConnection(.degraded(since: Date()))
        engine.sendConnection(.live(revision: 8))
        await eventually("the re-read after reconnecting") { engine.evaluates == evaluates + 1 }
        _ = model
    }
}
