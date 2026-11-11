// What `RemoteEngine` evaluates and renders.
//
// The JSON session view (`GET /api/session`) is the web UI's contract: it
// names the document's assets but never carries their bytes. A
// script that calls `asset("badge.svg")` therefore cannot be evaluated from a
// document rebuilt out of that view; the engine answers `asset "badge.svg"
// not found`. The whole document, assets included, is
// `GET /api/session/document`, and that is what has to go on the wire.

import Foundation
import CadpoleKit
import CadpoleProto
import XCTest

final class RemoteDocumentTests: XCTestCase {
    /// The session view as `cadpole serve` sends it: asset names, no bytes.
    private static let sessionJSON = """
    {
      "id": "sess-4",
      "revision": 2,
      "document": {
        "id": "doc-badge",
        "name": "svg_badge",
        "display_unit": "mm",
        "scripts": [{"path": "main.star", "source": "b = import_svg(asset('badge.svg'))"}],
        "param_overrides": {"depth": 3.0},
        "asset_names": ["badge.svg"]
      },
      "selection": {"selected": [], "note": ""},
      "messages": [],
      "clients": 1,
      "last_event_seq": 5,
      "assistant_listening": false
    }
    """

    /// The same session as `GET /api/session/document` serves it.
    private static func fullDocument() -> Cadpole_V1_Document {
        var doc = Cadpole_V1_Document()
        doc.id = "doc-badge"
        doc.name = "svg_badge"
        doc.schemaVersion = 1
        var script = Cadpole_V1_ScriptFile()
        script.path = "main.star"
        script.source = "b = import_svg(asset('badge.svg'))"
        doc.scripts = [script]
        doc.paramOverrides = ["depth": .number(3.0)]
        var asset = Cadpole_V1_Asset()
        asset.name = "badge.svg"
        asset.mime = "image/svg+xml"
        asset.kind = .svg
        asset.data = Data("<svg xmlns='http://www.w3.org/2000/svg'/>".utf8)
        asset.size = UInt64(asset.data.count)
        doc.assets = [asset]
        var ref = Cadpole_V1_EntityRef()
        ref.featureID = "b"
        ref.kind = .feature
        var selection = Cadpole_V1_Selection()
        selection.selected = [ref]
        doc.selection = selection
        return doc
    }

    /// A transport that answers like `cadpole serve`, and keeps the `/api/call`
    /// bodies so the test can read what was evaluated.
    private func server() throws -> (RemoteEngine, FakeTransport) {
        let documentBytes = try Self.fullDocument().serializedData()
        var evaluate = Cadpole_V1_EvaluateResponse()
        evaluate.model = Cadpole_V1_Model()
        var evaluateResponse = Cadpole_V1_Response()
        evaluateResponse.evaluate = evaluate
        let evaluateBytes = try evaluateResponse.serializedData()
        var render = Cadpole_V1_RenderResponse()
        render.png = Data([0x89, 0x50, 0x4E, 0x47])
        var renderResponse = Cadpole_V1_Response()
        renderResponse.render = render
        let renderBytes = try renderResponse.serializedData()

        let transport = FakeTransport { request in
            switch request.url.path {
            case "/api/session":
                return HTTPResponse(status: 200, body: Data(Self.sessionJSON.utf8))
            case "/api/session/document":
                return HTTPResponse(
                    status: 200,
                    headers: ["Content-Type": "application/x-protobuf"],
                    body: documentBytes
                )
            case "/api/call":
                let sent = try Cadpole_V1_Request(serializedData: request.body ?? Data())
                if case .render = sent.request {
                    return HTTPResponse(status: 200, body: renderBytes)
                }
                return HTTPResponse(status: 200, body: evaluateBytes)
            default:
                return HTTPResponse(status: 404, body: Data("no route".utf8))
            }
        }
        let engine = RemoteEngine(
            baseURL: URL(string: "http://cadpole-host:7331")!,
            token: "s3cret",
            transport: transport,
            clientID: "scene-A",
            reconnect: .immediate
        )
        return (engine, transport)
    }

    private func sentRequests(_ transport: FakeTransport) throws -> [Cadpole_V1_Request] {
        try transport.requests
            .filter { $0.url.path == "/api/call" }
            .map { try Cadpole_V1_Request(serializedData: $0.body ?? Data()) }
    }

    func testTheDocumentCarriesTheSessionsAssets() async throws {
        let (engine, _) = try server()
        let document = try await engine.document()
        XCTAssertEqual(document.assets.map(\.name), ["badge.svg"])
        XCTAssertFalse(document.assets.first?.data.isEmpty ?? true, "the bytes, not just the name")
        // Nothing else is lost on the way.
        XCTAssertEqual(document.scripts.first?.path, "main.star")
        XCTAssertEqual(document.paramOverrides["depth"], .number(3.0))
        XCTAssertEqual(document.selection.selected.first?.featureID, "b")
    }

    func testEvaluateSendsTheAssetsTheScriptReads() async throws {
        let (engine, transport) = try server()
        _ = try await engine.evaluate()

        let sent = try XCTUnwrap(try sentRequests(transport).first)
        guard case .evaluate(let e) = sent.request else {
            return XCTFail("expected an EvaluateRequest on the wire")
        }
        XCTAssertEqual(e.document.assets.map(\.name), ["badge.svg"])
        XCTAssertEqual(e.document.assets.first?.data, Self.fullDocument().assets.first?.data)
    }

    func testRenderSendsTheAssetsTheScriptReads() async throws {
        let (engine, transport) = try server()
        _ = try await engine.render(RenderSpec())

        let sent = try XCTUnwrap(try sentRequests(transport).first)
        guard case .render(let r) = sent.request else {
            return XCTFail("expected a RenderRequest on the wire")
        }
        XCTAssertEqual(r.document.assets.map(\.name), ["badge.svg"])
    }

    /// The document read is protobuf both ways, with the same bearer header as
    /// every other route.
    func testTheDocumentIsReadAsProtobuf() async throws {
        let (engine, transport) = try server()
        _ = try await engine.document()
        let read = try XCTUnwrap(transport.requests.first { $0.url.path == "/api/session/document" })
        XCTAssertEqual(read.method, "GET")
        XCTAssertEqual(read.headers["Accept"], "application/x-protobuf")
        XCTAssertEqual(read.headers["Authorization"], "Bearer s3cret")
    }
}
