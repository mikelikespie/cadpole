// Recognising this client's own writes on the event stream, and a
// reconnection as opposed to a revision bump.

import Foundation
import CadpoleKit
import CadpoleProto
import XCTest

final class OwnWriteEventTests: XCTestCase {
    func testAnEventSaysWhoCausedIt() throws {
        let event = try EngineEvent.decode(
            sseData: #"{"seq":2,"at":"t","kind":"params_changed","data":{"overrides":{"width":3}},"by":"scene-A"}"#
        )
        XCTAssertEqual(event.by, "scene-A")
        XCTAssertTrue(event.isEcho(of: "scene-A"))
        XCTAssertFalse(event.isEcho(of: "scene-B"))
    }

    func testAServerEventIsNobodysEcho() throws {
        // `by` is omitted when the server itself emitted the event.
        let event = try EngineEvent.decode(sseData: #"{"seq":1,"kind":"client_joined","data":{}}"#)
        XCTAssertEqual(event.by, "")
        XCTAssertFalse(event.isEcho(of: "scene-A"))
        // An engine with no attribution skips nothing.
        XCTAssertFalse(event.isEcho(of: ""))
    }

    func testRemoteEngineEventsCarryTheirAuthor() async throws {
        let sse = """
        data: {"seq":2,"kind":"params_changed","data":{},"by":"scene-A"}

        data: {"seq":3,"kind":"params_changed","data":{},"by":"assistant"}


        """
        let transport = FakeTransport(streamBody: sse) { _ in HTTPResponse(status: 200) }
        let engine = RemoteEngine(
            baseURL: URL(string: "http://cadpole-host:7331")!,
            token: "s3cret",
            transport: transport,
            clientID: "scene-A",
            reconnect: .immediate
        )
        var echoes: [Bool] = []
        for try await event in engine.events(after: 1) {
            echoes.append(event.isEcho(of: engine.clientID))
            if echoes.count == 2 { break }
        }
        XCTAssertEqual(echoes, [true, false])
    }

    func testAnEngineWithoutAttributionHasAnEmptyClientID() {
        XCTAssertEqual(FakeEngine().clientID, "")
    }

    func testOnlyComingBackLiveIsAReconnection() {
        var detector = ReconnectionDetector()
        let states: [ConnectionState] = [
            .disconnected, .connecting,
            .live(revision: 1),  // the first connection: not a reconnection
            .live(revision: 2),  // a write bumped the revision
            .live(revision: 3),
            .degraded(since: Date(timeIntervalSince1970: 0)),
            .live(revision: 3),  // back: a reconnection
            .live(revision: 4),
            .connecting,
            .live(revision: 9),  // back again
            .unauthorized,
        ]
        XCTAssertEqual(
            states.map { detector.observe($0) },
            [false, false, false, false, false, false, true, false, false, true, false]
        )
    }
}
