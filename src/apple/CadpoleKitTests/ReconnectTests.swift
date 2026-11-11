// The event stream after the connection goes away.
//
// A phone sleeps, changes network, or the server is restarted. Every one of
// those ends the byte stream, and a client that treats that as the end of the
// document stops following it with nothing on screen to say so. `FakeTransport` scripts one
// episode per connection attempt, so the whole ladder -- drop, retry, resume,
// a new epoch, a dead token -- is exercised on Linux with no server.

import Foundation
import CadpoleKit
import CadpoleProto
import XCTest

final class ReconnectTests: XCTestCase {
    private func frame(seq: UInt64, kind: String) -> String {
        """
        id: \(seq)
        event: \(kind)
        data: {"seq":\(seq),"at":"t","kind":"\(kind)","data":{}}


        """
    }

    private func remote(
        _ episodes: [FakeTransport.StreamEpisode],
        reconnect: ReconnectPolicy = .immediate
    ) -> (RemoteEngine, FakeTransport) {
        let transport = FakeTransport(streamEpisodes: episodes) { request in
            // A reconnection first asks the session where its event log is.
            // This server has not restarted: its log is past every cursor
            // these tests resume from. (EventEpochTests covers a restart.)
            if request.url.path == "/api/session" {
                return HTTPResponse(status: 200, body: Data(#"{"id":"sess-1","last_event_seq":1000}"#.utf8))
            }
            return HTTPResponse(status: 200, body: Data("{}".utf8))
        }
        let engine = RemoteEngine(
            baseURL: URL(string: "http://cadpole-host:7331")!,
            token: "s3cret",
            transport: transport,
            clientID: "scene-A",
            reconnect: reconnect
        )
        return (engine, transport)
    }

    /// The state the monitor hands a new subscriber is the current one, so a
    /// test can read it without racing a collector task.
    private func currentState(of engine: RemoteEngine) async -> ConnectionState? {
        var iterator = engine.connection.makeAsyncIterator()
        return await iterator.next()
    }

    /// Wait for a state the test is looking for, and give up rather than hang:
    /// a regression here should fail this suite in seconds, not sit on the
    /// runner until Bazel's timeout.
    private func waitForState(
        of engine: RemoteEngine,
        timeout: Duration = .seconds(5),
        matching predicate: @escaping @Sendable (ConnectionState) -> Bool
    ) async -> ConnectionState? {
        await withTaskGroup(of: ConnectionState?.self) { group in
            group.addTask {
                for await state in engine.connection where predicate(state) { return state }
                return nil
            }
            group.addTask {
                try? await Task.sleep(for: timeout)
                return nil
            }
            let first = await group.next() ?? nil
            group.cancelAll()
            return first
        }
    }

    // MARK: - Reconnecting

    func testTheStreamReconnectsAfterADroppedConnection() async throws {
        // One frame, then EOF -- a dropped connection.
        let (engine, transport) = remote([
            .body(frame(seq: 8, kind: "params_changed")),
            .body(frame(seq: 9, kind: "script_changed")),
        ])

        var seqs: [UInt64] = []
        for try await event in engine.events(after: 7) {
            seqs.append(event.seq)
            if seqs.count == 2 { break }
        }

        XCTAssertEqual(seqs, [8, 9], "the second event only arrives if the client reconnected")
        XCTAssertGreaterThanOrEqual(transport.streamAttempts, 2, "the stream was opened again")
    }

    func testAReconnectionResumesFromTheLastSequenceNumberSeen() async throws {
        let (engine, transport) = remote([
            .body(frame(seq: 8, kind: "params_changed")),
            .body(frame(seq: 9, kind: "script_changed")),
        ])
        var seqs: [UInt64] = []
        for try await event in engine.events(after: 7) {
            seqs.append(event.seq)
            if seqs.count == 2 { break }
        }

        let opens = transport.streamRequests
        XCTAssertGreaterThanOrEqual(opens.count, 2, "the stream was opened again")
        guard opens.count >= 2 else { return }
        XCTAssertEqual(opens[0].url.query, "after=7")
        XCTAssertEqual(opens[0].headers["Last-Event-ID"], "7")
        // Event 8 arrived on the first connection, so the second asks for what
        // comes after it -- not for 7 again.
        XCTAssertEqual(opens[1].url.query, "after=8")
        XCTAssertEqual(opens[1].headers["Last-Event-ID"], "8")
    }

    /// The cursor is a sequence number and belongs in the standard SSE resume
    /// header; the token stays in `Authorization` on this route as on every
    /// other (docs/security.md).
    func testTheResumeCursorTravelsInAHeaderAndTheTokenNeverInAQuery() async throws {
        let (engine, transport) = remote([.body(frame(seq: 1, kind: "client_joined"))])
        for try await _ in engine.events(after: 0) { break }

        let open = try XCTUnwrap(transport.streamRequests.first)
        XCTAssertEqual(open.headers["Authorization"], "Bearer s3cret")
        XCTAssertEqual(open.headers["Accept"], "text/event-stream")
        XCTAssertNil(open.headers["Last-Event-ID"], "nothing to resume from at seq 0")
        XCTAssertFalse(open.url.absoluteString.contains("token="))
        XCTAssertFalse(
            open.url.absoluteString.lowercased().contains("last-event-id"),
            "the resume cursor is a header, not a query parameter"
        )
    }

    func testAConnectionThatFailsOnceIsRetriedAndThenWorks() async throws {
        let (engine, transport) = remote([
            .failure(.http(status: 503, body: "server restarting")),
            .body(frame(seq: 12, kind: "params_changed")),
        ])

        var seqs: [UInt64] = []
        for try await event in engine.events(after: 11) {
            seqs.append(event.seq)
            break
        }
        XCTAssertEqual(seqs, [12], "a refused connection is retried, not fatal")
        XCTAssertGreaterThanOrEqual(transport.streamAttempts, 2)
    }

    /// The restart check, on the far side of a reconnection: the server was
    /// restarted, so its log begins again. Holding the old cursor would drop
    /// every event it will ever send.
    func testARestartedServersNewEpochIsAdopted() async throws {
        let (engine, _) = remote([
            .body(frame(seq: 40, kind: "params_changed")),
            .body(frame(seq: 1, kind: "client_joined") + frame(seq: 2, kind: "script_changed")),
        ])

        var seqs: [UInt64] = []
        for try await event in engine.events(after: 39) {
            seqs.append(event.seq)
            if seqs.count == 3 { break }
        }
        XCTAssertEqual(seqs, [40, 1, 2])
    }

    func testA401EndsTheStreamInsteadOfHammeringTheServer() async throws {
        // `cadpole serve` mints a new token every run, so a 401 is the one answer
        // retrying cannot fix.
        let (engine, transport) = remote([
            .failure(.http(status: 401, body: "missing or wrong bearer token")),
            .body(frame(seq: 5, kind: "params_changed")),
        ])

        do {
            for try await _ in engine.events(after: 4) {
                XCTFail("a 401 must not deliver events")
            }
            XCTFail("expected the 401 to end the stream")
        } catch let error as EngineError {
            guard case .http(let status, _) = error else {
                return XCTFail("expected EngineError.http, got \(error)")
            }
            XCTAssertEqual(status, 401)
        }
        XCTAssertEqual(transport.streamAttempts, 1, "no retry after a 401")
        let state = await currentState(of: engine)
        XCTAssertEqual(state, .unauthorized, "the pill says the token is dead, not that we are retrying")
    }

    // MARK: - What the toolbar pill shows

    func testAnOpenStreamReportsItselfLive() async throws {
        let (engine, _) = remote([.body(frame(seq: 8, kind: "params_changed"))])

        let before = await currentState(of: engine)
        XCTAssertEqual(before, .disconnected, "nothing has connected yet")

        var stream = engine.events(after: 7).makeAsyncIterator()
        let first = try await stream.next()
        XCTAssertEqual(first?.seq, 8)

        // `.immediate` reconnects straight into the silent connection the
        // script leaves behind, so the steady state here is live.
        let live = await waitForState(of: engine) { if case .live = $0 { return true } else { return false } }
        XCTAssertNotNil(live, "a stream that is open is live")
    }

    func testADroppedConnectionShowsAsDegradedWhileItRetries() async throws {
        // A slow ladder, so the client stays in `degraded` long enough to
        // observe instead of racing back to `live`.
        let (engine, _) = remote(
            [.body(frame(seq: 8, kind: "params_changed"))],
            reconnect: ReconnectPolicy(initial: .seconds(60), maximum: .seconds(60))
        )

        var stream = engine.events(after: 7).makeAsyncIterator()
        let first = try await stream.next()
        XCTAssertEqual(first?.seq, 8)

        // EOF is a dropped connection. The pill says so, and keeps saying so
        // until the ladder brings the stream back -- it does not claim to be
        // live and it does not give up.
        let degraded = await waitForState(of: engine) { if case .degraded = $0 { return true } else { return false } }
        XCTAssertNotNil(degraded, "a dropped connection shows as degraded, not as live")
    }

    // MARK: - The ladder itself

    func testTheBackoffLadderDoublesFromOneSecondToTen() {
        let policy = ReconnectPolicy.default
        XCTAssertEqual(policy.delay(afterFailures: 1), .seconds(1))
        XCTAssertEqual(policy.delay(afterFailures: 2), .seconds(2))
        XCTAssertEqual(policy.delay(afterFailures: 3), .seconds(4))
        XCTAssertEqual(policy.delay(afterFailures: 4), .seconds(8))
        XCTAssertEqual(policy.delay(afterFailures: 5), .seconds(10), "capped")
        XCTAssertEqual(policy.delay(afterFailures: 50), .seconds(10), "and stays capped")
        XCTAssertEqual(ReconnectPolicy.immediate.delay(afterFailures: 9), .zero)
    }
}
