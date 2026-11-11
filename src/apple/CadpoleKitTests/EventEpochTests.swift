// The event stream across a server restart, against a fake that answers the
// way `cadpole serve` does.
//
// `get_events` resumes with `filter(|e| e.seq > after)`, and a restarted
// server numbers its log from 1 again. A client that reconnects with its old
// cursor is therefore sent nothing at all -- not even the low sequence numbers
// the client's epoch check is waiting to see -- until the new log passes the old
// cursor. The stream has to find out from somewhere else that the log began
// again; the session view's `last_event_seq` below the cursor says exactly
// that.

import Foundation
import CadpoleKit
import CadpoleProto
import XCTest

/// One `cadpole serve` that can be restarted. Events and the session view are
/// answered from its current log, filtered the way server.rs filters them.
private final class RestartingServer: HTTPTransport, @unchecked Sendable {
    private let lock = NSLock()
    private var boot = 1
    private var log: [(UInt64, String)]
    /// The first connection is cut after its backlog (the server went away);
    /// later ones stay open and silent once the backlog is sent.
    private var streamsOpened = 0
    private var _streamQueries: [String] = []
    private var _sessionReads = 0
    private let restartAfterFirstStream: Bool
    private let logAfterRestart: [(UInt64, String)]

    init(log: [(UInt64, String)], restartTo newLog: [(UInt64, String)]?) {
        self.log = log
        restartAfterFirstStream = newLog != nil
        logAfterRestart = newLog ?? []
    }

    var streamQueries: [String] { lock.withLock { _streamQueries } }
    var sessionReads: Int { lock.withLock { _sessionReads } }

    func send(_ request: HTTPRequest) async throws -> HTTPResponse {
        guard request.url.path == "/api/session" else {
            return HTTPResponse(status: 404, body: Data("no route".utf8))
        }
        let (id, last) = lock.withLock { () -> (String, UInt64) in
            _sessionReads += 1
            return ("boot-\(boot)", log.last?.0 ?? 0)
        }
        let json = """
        {"id":"\(id)","revision":1,
         "document":{"id":"d","name":"bracket","display_unit":"mm","scripts":[],"param_overrides":{},"asset_names":[]},
         "selection":{"selected":[]},"messages":[],"clients":1,"last_event_seq":\(last),"assistant_listening":false}
        """
        return HTTPResponse(status: 200, body: Data(json.utf8))
    }

    func stream(_ request: HTTPRequest) async throws -> AsyncThrowingStream<UInt8, any Error> {
        let after = URLComponents(url: request.url, resolvingAgainstBaseURL: false)?
            .queryItems?.first { $0.name == "after" }?.value.flatMap(UInt64.init) ?? 0
        let (frames, cut) = lock.withLock { () -> (String, Bool) in
            streamsOpened += 1
            _streamQueries.append(request.url.query ?? "")
            let text = log.filter { $0.0 > after }.map { seq, kind in
                "id: \(seq)\nevent: \(kind)\ndata: {\"seq\":\(seq),\"at\":\"t\",\"kind\":\"\(kind)\",\"data\":{}}\n\n"
            }.joined()
            let cut = streamsOpened == 1 && restartAfterFirstStream
            if cut {
                // The process dies after this connection; the next one
                // talks to a new process with a new log.
                boot += 1
                log = logAfterRestart
            }
            return (text, cut)
        }
        let bytes = Array(frames.utf8)
        return AsyncThrowingStream { continuation in
            for b in bytes { continuation.yield(b) }
            if cut { continuation.finish() }
            // Otherwise: open and silent, as a live server between events.
        }
    }
}

final class EventEpochTests: XCTestCase {
    private func engine(_ server: RestartingServer) -> RemoteEngine {
        RemoteEngine(
            baseURL: URL(string: "http://cadpole-host:7331")!,
            token: "s3cret",
            transport: server,
            clientID: "scene-A",
            reconnect: .immediate
        )
    }

    /// Collect up to `count` events, giving up after `timeout` rather than
    /// hanging on a stream that has gone silent.
    private func collect(
        _ count: Int,
        from engine: RemoteEngine,
        after seq: UInt64,
        timeout: Duration = .seconds(3)
    ) async -> [UInt64] {
        await withTaskGroup(of: [UInt64]?.self) { group in
            let box = SeqBox()
            group.addTask {
                do {
                    for try await event in engine.events(after: seq) {
                        await box.append(event.seq)
                        if await box.count == count { break }
                    }
                } catch {}
                return await box.seqs
            }
            group.addTask {
                try? await Task.sleep(for: timeout)
                return nil
            }
            let first = await group.next() ?? nil
            group.cancelAll()
            if let first { return first }
            return await box.seqs
        }
    }

    private actor SeqBox {
        var seqs: [UInt64] = []
        var count: Int { seqs.count }
        func append(_ s: UInt64) { seqs.append(s) }
    }

    /// A restarted log that has already reached the old cursor sends
    /// nothing below it, and `last_event_seq` is not below the cursor either;
    /// only the session id says the log began again.
    func testARestartIsSeenByTheSessionIdWhenTheNewLogHasCaughtUp() async {
        let server = RestartingServer(
            log: [(1, "client_joined"), (2, "params_changed")],
            restartTo: [(1, "client_joined"), (2, "script_changed"), (3, "params_changed")]
        )

        let seqs = await collect(3, from: engine(server), after: 2)
        XCTAssertEqual(seqs, [1, 2, 3], "the restarted server's first events were dropped")
        XCTAssertEqual(server.streamQueries.last, "", "the new log is read from its start")
    }

    func testAfterARestartTheStreamFollowsTheNewLog() async {
        // The old process had 40 events; the client has seen up to 39. It
        // restarts, and the new one has logged two so far.
        let old = (1 ... 40).map { (UInt64($0), "params_changed") }
        let server = RestartingServer(log: old, restartTo: [(1, "client_joined"), (2, "script_changed")])

        let seqs = await collect(3, from: engine(server), after: 39)
        XCTAssertEqual(seqs, [40, 1, 2], "the restarted server's events never arrived")
        XCTAssertEqual(server.streamQueries.last, "", "the new log is read from its start")
    }

    /// Without a restart the reconnection resumes where it stopped and the
    /// session check changes nothing.
    func testAReconnectionToTheSameServerResumesFromTheCursor() async {
        let server = RestartingServer(log: (1 ... 8).map { (UInt64($0), "params_changed") }, restartTo: nil)
        let engine = engine(server)
        let seqs = await collect(2, from: engine, after: 6, timeout: .milliseconds(300))
        XCTAssertEqual(seqs, [7, 8])
        XCTAssertEqual(server.streamQueries.first, "after=6")
    }
}
