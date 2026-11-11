// The cheap layer: protocol behaviour, wire format, SSE, coalescing.
// Foundation only, so it runs on Linux on every commit
// (docs/apple-apps.md). The Simulator target next door runs the same
// sources and proves only that the Simulator path works.

import Foundation
import CadpoleKit
import CadpoleProto
import XCTest

final class EngineClientTests: XCTestCase {
    // MARK: - The protocol's own behaviour, against a fake engine

    func testEvaluateSendsTheSessionDocumentAndReturnsItsModel() async throws {
        let engine = FakeEngine()
        let model = try await engine.evaluate(tolerance: 0.05)

        XCTAssertEqual(model.params.map(\.name), ["wall", "height"])
        XCTAssertEqual(model.features.map(\.id), ["sketch_1", "extrude_1"])

        // The request carried the document from the session, not an empty one:
        // this is what makes LocalEngine and RemoteEngine send the same bytes.
        let request = try XCTUnwrap(engine.calls.first)
        guard case .evaluate(let e) = request.request else {
            return XCTFail("expected an EvaluateRequest, got \(String(describing: request.request))")
        }
        XCTAssertEqual(e.tolerance, 0.05)
        XCTAssertEqual(e.document.scripts.first?.path, "main.star")
        XCTAssertEqual(e.document.paramOverrides["wall"], .number(2.4))
        XCTAssertEqual(e.deadlineMs, 0, "no deadline was asked for")
    }

    func testEvaluateCarriesTheDeadline() async throws {
        let engine = FakeEngine()
        _ = try await engine.evaluate(deadline: .seconds(20))
        guard case .evaluate(let e) = try XCTUnwrap(engine.calls.first).request else {
            return XCTFail("expected an EvaluateRequest")
        }
        XCTAssertEqual(e.deadlineMs, 20_000)
    }

    func testRenderSpellsOutTheNegatives() async throws {
        let engine = FakeEngine()
        var spec = RenderSpec(width: 640, height: 480, view: "front")
        spec.drawGrid = false
        spec.highlightSelection = true

        let result = try await engine.render(spec)
        XCTAssertEqual(result.width, 640)
        XCTAssertEqual(result.height, 480)
        XCTAssertEqual(Array(result.png.prefix(4)), [0x89, 0x50, 0x4E, 0x47])

        guard case .render(let r) = try XCTUnwrap(engine.calls.first).request else {
            return XCTFail("expected a RenderRequest")
        }
        XCTAssertEqual(r.camera.preset, "front")
        XCTAssertTrue(r.highlightSelection)
        // proto3 cannot tell "false" from "unset", so the engine needs the
        // explicit negative to turn the grid off.
        XCTAssertTrue(r.noGrid)
        XCTAssertFalse(r.noEdges)
    }

    func testSettingAParameterIsVisibleInTheNextEvaluation() async throws {
        let engine = FakeEngine()
        let before = try await engine.session()

        let after = try await engine.setParams(["wall": .number(3.2)])
        XCTAssertEqual(after.revision, before.revision + 1)

        let model = try await engine.evaluate()
        let wall = try XCTUnwrap(model.params.first { $0.name == "wall" })
        XCTAssertEqual(wall.value, .number(3.2))
        XCTAssertTrue(wall.overridden)
    }

    func testSelectionRoundTrip() async throws {
        let engine = FakeEngine()
        var ref = Cadpole_V1_EntityRef()
        ref.featureID = "extrude_1"
        ref.bodyID = "extrude_1#0"
        ref.kind = .face
        ref.topoName = "face:top"

        let snapshot = try await engine.setSelection([ref])
        XCTAssertEqual(snapshot.selection, [ref])

        // The selection travels inside the document, so a render can highlight it.
        let document = try await engine.document()
        XCTAssertEqual(document.selection.selected.first?.topoName, "face:top")
    }

    func testApiDocComesBackDecoded() async throws {
        let doc = try await FakeEngine().apiDoc()
        XCTAssertEqual(doc.functions.first?.name, "extrude")
    }

    func testEngineErrorSurfacesResponseError() async throws {
        let engine = FakeEngine()
        var request = Cadpole_V1_Request()
        request.format = Cadpole_V1_FormatRequest()  // the fake does not implement it
        let response = try await engine.call(request)
        guard case .error(let message) = response.response else {
            return XCTFail("expected Response.error")
        }
        XCTAssertTrue(message.contains("unsupported"))
    }

    // MARK: - RemoteEngine over a fake transport

    private static let sessionJSON = """
    {
      "id": "sess-9",
      "revision": 12,
      "document": {
        "id": "doc-1",
        "name": "bracket",
        "display_unit": "mm",
        "scripts": [{"path": "main.star", "source": "w = param('w', 4.0)"}],
        "param_overrides": {"w": 4.5, "label": "left", "flip": true, "origin": [1.0, 2.0, 3.0]}
      },
      "selection": {
        "selected": [{"feature_id": "extrude_1", "body_id": "extrude_1#0", "kind": "face", "topo_name": "face:top"}],
        "note": ""
      },
      "messages": [],
      "clients": 1,
      "last_event_seq": 31,
      "assistant_listening": false
    }
    """

    private func remote(
        streamBody: String = "",
        handler: @escaping FakeTransport.Handler
    ) -> (RemoteEngine, FakeTransport) {
        let transport = FakeTransport(streamBody: streamBody, handler: handler)
        // `.immediate`: the stream reconnects now, so a test that stops
        // listening mid-ladder does not wait out a real backoff first.
        let engine = RemoteEngine(
            baseURL: URL(string: "http://cadpole-host:7331")!,
            token: "s3cret",
            transport: transport,
            clientID: "scene-A",
            reconnect: .immediate
        )
        return (engine, transport)
    }

    func testSessionDecodesTheServersJSONView() async throws {
        let (engine, transport) = remote { _ in
            HTTPResponse(status: 200, body: Data(Self.sessionJSON.utf8))
        }
        let session = try await engine.session()

        XCTAssertEqual(session.id, "sess-9")
        XCTAssertEqual(session.revision, 12)
        XCTAssertEqual(session.documentName, "bracket")
        XCTAssertEqual(session.lastEventSeq, 31)
        XCTAssertEqual(session.scripts.first?.source, "w = param('w', 4.0)")
        // All four ParamValue shapes from cadpole_engine::view::param_value_json.
        XCTAssertEqual(session.paramOverrides["w"], .number(4.5))
        XCTAssertEqual(session.paramOverrides["label"], .text("left"))
        XCTAssertEqual(session.paramOverrides["flip"], .boolean(true))
        XCTAssertEqual(session.paramOverrides["origin"]?.vector.y, 2.0)
        XCTAssertEqual(session.selection.first?.kind, .face)
        XCTAssertEqual(session.selection.first?.topoName, "face:top")

        let request = try XCTUnwrap(transport.requests.first)
        XCTAssertEqual(request.url.absoluteString, "http://cadpole-host:7331/api/session")
        XCTAssertEqual(request.headers["Authorization"], "Bearer s3cret")
        XCTAssertEqual(request.headers["X-Cadpole-Client"], "scene-A")
    }

    func testCallPostsTheProtobufEnvelope() async throws {
        var model = Cadpole_V1_Model()
        var param = Cadpole_V1_ParamInfo()
        param.name = "w"
        model.params = [param]
        var evaluateResponse = Cadpole_V1_EvaluateResponse()
        evaluateResponse.model = model
        var response = Cadpole_V1_Response()
        response.evaluate = evaluateResponse
        let responseBytes = try response.serializedData()
        var document = Cadpole_V1_Document()
        document.name = "bracket"
        document.paramOverrides = ["w": .number(4.5)]
        let documentBytes = try document.serializedData()

        let (engine, transport) = remote { request in
            if request.url.path == "/api/session" {
                return HTTPResponse(status: 200, body: Data(Self.sessionJSON.utf8))
            }
            if request.url.path == "/api/session/document" {
                return HTTPResponse(status: 200, body: documentBytes)
            }
            return HTTPResponse(status: 200, body: responseBytes)
        }

        let evaluated = try await engine.evaluate()
        XCTAssertEqual(evaluated.params.map(\.name), ["w"])

        let call = try XCTUnwrap(transport.requests.last)
        XCTAssertEqual(call.method, "POST")
        XCTAssertEqual(call.url.path, "/api/call")
        XCTAssertEqual(call.headers["Content-Type"], "application/x-protobuf")
        XCTAssertEqual(call.headers["Authorization"], "Bearer s3cret")

        // The bytes on the wire are a cadpole.v1.Request -- the same envelope
        // LocalEngine hands to cadpole_call.
        let sent = try Cadpole_V1_Request(serializedData: try XCTUnwrap(call.body))
        guard case .evaluate(let e) = sent.request else {
            return XCTFail("expected an EvaluateRequest on the wire")
        }
        XCTAssertEqual(e.document.name, "bracket")
        XCTAssertEqual(e.document.paramOverrides["w"], .number(4.5))
    }

    func testSetParamsPutsTheOverridesAsJSON() async throws {
        let (engine, transport) = remote { _ in
            HTTPResponse(status: 200, body: Data(Self.sessionJSON.utf8))
        }
        _ = try await engine.setParams(["w": .number(6.25), "flip": .boolean(false)])

        let put = try XCTUnwrap(transport.requests.last)
        XCTAssertEqual(put.method, "PUT")
        XCTAssertEqual(put.url.path, "/api/session/params")
        XCTAssertEqual(put.headers["Content-Type"], "application/json")

        struct Body: Decodable { var overrides: [String: JSONValue] }
        let body = try JSONDecoder().decode(Body.self, from: try XCTUnwrap(put.body))
        XCTAssertEqual(body.overrides["w"], .number(6.25))
        XCTAssertEqual(body.overrides["flip"], .bool(false))
    }

    func testUnauthorizedIsReportedAsSuch() async throws {
        let (engine, _) = remote { _ in
            HTTPResponse(status: 401, body: Data("missing or wrong bearer token".utf8))
        }
        do {
            _ = try await engine.session()
            XCTFail("expected a 401")
        } catch let error as EngineError {
            guard case .http(let status, _) = error else {
                return XCTFail("expected EngineError.http, got \(error)")
            }
            XCTAssertEqual(status, 401)
        }
    }

    func testConflictBecomesStaleRevision() async throws {
        let (engine, _) = remote { _ in
            HTTPResponse(status: 409, body: Data(#"{"error":"stale base_revision","revision":18}"#.utf8))
        }
        do {
            _ = try await engine.setParams([:])
            XCTFail("expected a 409")
        } catch let error as EngineError {
            XCTAssertEqual(error, .staleRevision(current: 18))
        }
    }

    func testEventStreamUsesTheAuthorizationHeaderNotAQueryToken() async throws {
        let sse = """
        id: 8\r
        event: params_changed\r
        data: {"seq":8,"at":"t","kind":"params_changed","data":{"overrides":{"w":5}}}\r
        \r
        id: 9\r
        event: eval_finished\r
        data: {"seq":9,"at":"t","kind":"eval_finished","data":{}}\r
        \r

        """
        let (engine, transport) = remote(streamBody: sse) { _ in
            HTTPResponse(status: 200)
        }

        var kinds: [String] = []
        for try await event in engine.events(after: 7) {
            kinds.append(event.kind)
            // The stream reconnects rather than ending, so stop at the frames
            // this fixture holds.
            if kinds.count == 2 { break }
        }
        XCTAssertEqual(kinds, ["params_changed", "eval_finished"])

        let request = try XCTUnwrap(transport.streamRequests.first)
        XCTAssertEqual(request.url.path, "/api/events")
        XCTAssertEqual(request.url.query, "after=7")
        XCTAssertEqual(request.headers["Authorization"], "Bearer s3cret")
        XCTAssertFalse(
            request.url.absoluteString.contains("token="),
            "security.md's ?token= exception is for EventSource only"
        )
    }

    func testEventStreamDropsAStaleEpoch() async throws {
        // A seq at or below the cursor means the server restarted.
        let sse = """
        data: {"seq":2,"kind":"client_joined"}

        data: {"seq":40,"kind":"script_changed"}


        """
        let (engine, _) = remote(streamBody: sse) { _ in HTTPResponse(status: 200) }
        var seqs: [UInt64] = []
        for try await event in engine.events(after: 31) {
            seqs.append(event.seq)
            break
        }
        XCTAssertEqual(seqs, [40])
    }
}
