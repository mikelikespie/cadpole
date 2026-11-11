// The macOS test layer (docs/apple-apps.md): the FFI round trip, memory
// ownership and the cadpole_version match. Runs only where the Rust staticlib
// exists, so it is `manual` and runs on macOS.

import Foundation
import CadpoleKit
import CadpoleLocalEngine
import CadpoleProto
import XCTest

final class LocalEngineTests: XCTestCase {
    private func document(_ source: String = "w = param('w', 4.0)\n") -> Cadpole_V1_Document {
        var script = Cadpole_V1_ScriptFile()
        script.path = "main.star"
        script.source = source
        var doc = Cadpole_V1_Document()
        doc.id = "local-1"
        doc.name = "local"
        doc.schemaVersion = 1
        doc.displayUnit = .mm
        doc.scripts = [script]
        return doc
    }

    func testTheLinkedEngineIsTheOneTheProtosCameFrom() {
        // ABI rule 4: a mismatch is fatal, not a warning.
        XCTAssertEqual(LocalEngine.engineVersion, LocalEngine.expectedEngineVersion)
    }

    func testVersionMismatchRefusesToLaunch() {
        // The protos and the engine must come from the same build or the
        // envelope means different things.
        XCTAssertThrowsError(try LocalEngine(document: document(), requiringEngineVersion: "0.0.1-wrong")) { error in
            XCTAssertEqual(
                error as? LocalEngine.LaunchError,
                .versionMismatch(linked: LocalEngine.engineVersion, expected: "0.0.1-wrong")
            )
        }
        XCTAssertNoThrow(try LocalEngine(document: document()))
    }

    func testApiDocRoundTripsThroughTheCABI() async throws {
        let engine = try LocalEngine(document: document())
        let doc = try await engine.apiDoc()
        XCTAssertFalse(doc.functions.isEmpty, "the engine should describe its Starlark API")
    }

    func testEvaluateProducesAModelAndItsParameters() async throws {
        let engine = try LocalEngine(document: document("w = param('w', 4.0)\n"))
        let model = try await engine.evaluate()
        XCTAssertEqual(model.params.map(\.name), ["w"])
        XCTAssertEqual(model.params.first?.value, .number(4.0))
    }

    func testAParameterChangeIsVisibleInTheNextEvaluation() async throws {
        let engine = try LocalEngine(document: document())
        _ = try await engine.setParams(["w": .number(9.5)])
        let model = try await engine.evaluate()
        XCTAssertEqual(model.params.first?.value, .number(9.5))
        XCTAssertTrue(model.params.first?.overridden ?? false)
    }

    /// A packed document's assets are part of what gets evaluated: a script
    /// that reads `asset("badge.svg")` must find it, as it does in the CLI.
    func testTheDocumentKeepsItsAssets() async throws {
        var doc = document()
        var asset = Cadpole_V1_Asset()
        asset.name = "badge.svg"
        asset.kind = .svg
        asset.data = Data("<svg xmlns='http://www.w3.org/2000/svg'/>".utf8)
        asset.size = UInt64(asset.data.count)
        doc.assets = [asset]
        let engine = try LocalEngine(document: doc)

        let evaluated = try await engine.document()
        XCTAssertEqual(evaluated.assets, [asset])
        // And still after the session moves on.
        _ = try await engine.setParams(["w": .number(5)])
        let after = try await engine.document()
        XCTAssertEqual(after.assets, [asset])
    }

    /// Writes through the engine are this client's, so the screen that made
    /// them can recognise the echo and skip it.
    func testWritesAreAttributedToTheEnginesClient() async throws {
        let engine = try LocalEngine(document: document())
        var events = engine.events(after: 0).makeAsyncIterator()
        _ = try await engine.setParams(["w": .number(5)])
        let event = try await events.next()
        XCTAssertEqual(event?.kind, "params_changed")
        XCTAssertFalse(engine.clientID.isEmpty)
        XCTAssertEqual(event?.isEcho(of: engine.clientID), true)
    }

    func testRenderReturnsPNGBytes() async throws {
        let engine = try LocalEngine(document: document("box(10, 10, 10)\n"))
        let result = try await engine.render(RenderSpec(width: 160, height: 120))
        XCTAssertEqual(Array(result.png.prefix(4)), [0x89, 0x50, 0x4E, 0x47], "PNG magic")
        XCTAssertEqual(result.width, 160)
    }

    func testResponseBuffersAreFreedExactlyOnce() async throws {
        // Rule 2: the response is wrapped with Data(bytesNoCopy:) and freed by
        // its deallocator. Hammering it is how a double free or a leak shows up
        // as a crash under the address sanitiser rather than as a slow leak.
        let engine = try LocalEngine(document: document())
        for _ in 0 ..< 50 {
            _ = try await engine.apiDoc()
        }
    }

    func testCancelReleasesTheCaller() async throws {
        let slow = """
        def slow():
            n = 0
            for i in range(2000000):
                n = n + i
            return n

        total = slow()
        """
        let engine = try LocalEngine(document: document(slow))
        let token = engine.newCancellationToken()
        // Cancelling before the call starts is legal and is the race a slider
        // drag actually produces.
        engine.cancel(token)

        var request = Cadpole_V1_Request()
        var evaluate = Cadpole_V1_EvaluateRequest()
        evaluate.document = try await engine.document()
        request.evaluate = evaluate

        do {
            _ = try await engine.call(request, cancellationToken: token)
            XCTFail("expected the cancelled call to throw")
        } catch let error as EngineError {
            XCTAssertEqual(error, .cancelled)
        }
    }

    func testTheDeadlineReleasesTheCaller() async throws {
        let slow = """
        def slow():
            n = 0
            for i in range(2000000):
                n = n + i
            return n

        total = slow()
        """
        let engine = try LocalEngine(document: document(slow))
        let started = Date()
        do {
            let model = try await engine.evaluate(deadline: .milliseconds(50))
            // Not a failure. `deadlineMs` arms *two* mechanisms for the same
            // instant: `deadline_of` in cadpole-ffi bounds the waiter, which
            // returns `CADPOLE_ERR_DEADLINE` and becomes `.cancelled` here, and
            // the same field bounds the Starlark interpreter inside the worker,
            // which stops itself and returns an ordinary response carrying a
            // deadline diagnostic. Which wins depends on machine load (on a Mac
            // the interpreter usually does), so both outcomes are accepted, as
            // in cadpole-ffi's `the_deadline_releases_the_caller`. What is under
            // test is that the caller is released and told a deadline did it.
            let said = model.diagnostics.map(\.message).joined(separator: " | ")
            XCTAssertTrue(
                said.lowercased().contains("deadline") || said.lowercased().contains("budget"),
                "the interpreter stopped first, so the model must say a deadline did it: \(said)"
            )
        } catch let error as EngineError {
            XCTAssertEqual(error, .cancelled)
        }
        XCTAssertLessThan(Date().timeIntervalSince(started), 20, "the deadline must release the caller promptly")
    }

    /// Cancelling the Swift task is what the parameter coalescer does when a
    /// newer value arrives; it must reach cadpole_cancel and release the
    /// caller, not wait out the evaluation.
    func testCancellingTheTaskReleasesTheCaller() async throws {
        let slow = """
        def slow():
            n = 0
            for i in range(50000000):
                n = n + i
            return n

        total = slow()
        """
        let engine = try LocalEngine(document: document(slow))
        let started = Date()
        let task = Task { try await engine.evaluate() }
        try await Task.sleep(for: .milliseconds(50))
        task.cancel()
        do {
            _ = try await task.value
            XCTFail("expected the cancelled evaluation to throw")
        } catch let error as EngineError {
            XCTAssertEqual(error, .cancelled)
        }
        XCTAssertLessThan(Date().timeIntervalSince(started), 5, "cancelling must release the caller promptly")
    }

    func testSelectionAndEventsBehaveLikeASession() async throws {
        let engine = try LocalEngine(document: document())

        var ref = Cadpole_V1_EntityRef()
        ref.featureID = "extrude_1"
        ref.kind = .face
        ref.topoName = "face:top"
        _ = try await engine.setSelection([ref])

        let session = try await engine.session()
        XCTAssertEqual(session.selection.first?.topoName, "face:top")
        // The selection travels in the document, so highlight_selection works.
        let doc = try await engine.document()
        XCTAssertEqual(doc.selection.selected.count, 1)

        // Every mutation appends to the log the UI listens to.
        var kinds: [String] = []
        for try await event in engine.events(after: 0) {
            kinds.append(event.kind)
            if kinds.count == 1 { break }
        }
        XCTAssertEqual(kinds, ["selection_changed"])
    }

    func testStaleRevisionIsRefused() async throws {
        let engine = try LocalEngine(document: document())
        let before = try await engine.session()
        _ = try await engine.setScript(path: "main.star", source: "w = param('w', 1.0)", baseRevision: before.revision)
        do {
            _ = try await engine.setScript(path: "main.star", source: "w = 2", baseRevision: before.revision)
            XCTFail("expected a stale base_revision")
        } catch let error as EngineError {
            XCTAssertEqual(error, .staleRevision(current: before.revision + 1))
        }
    }

    func testSelfHostedHasNoAssistant() throws {
        // The honest consequence of self-hosting: the assistant workflow IS
        // the session server (docs/apple-apps.md).
        let engine = try LocalEngine(document: document())
        XCTAssertTrue(engine.capabilities.contains(.localEvaluation))
        XCTAssertFalse(engine.capabilities.contains(.assistant))
    }
}
