// A login's lifecycle (CadpoleKit/ServerLogin.swift): the token is kept only
// once the server accepts it, and a 401 forgets it and ends the login.

import Foundation
import CadpoleKit
import XCTest

final class MemoryTokenStore: TokenStoring, @unchecked Sendable {
    private let lock = NSLock()
    private var tokens: [String: String] = [:]

    func save(_ token: String, host: String) throws {
        lock.lock()
        defer { lock.unlock() }
        tokens[host] = token
    }

    func load(host: String) -> String? {
        lock.lock()
        defer { lock.unlock() }
        return tokens[host]
    }

    func delete(host: String) {
        lock.lock()
        defer { lock.unlock() }
        tokens[host] = nil
    }
}

final class ServerLoginTests: XCTestCase {
    private func freshRecents() -> RecentServers {
        let defaults = UserDefaults(suiteName: "cadpole-test-\(UUID().uuidString)")!
        defaults.removeObject(forKey: RecentServers.key)
        return RecentServers(defaults: defaults)
    }

    private func states(_ list: [ConnectionState]) -> AsyncStream<ConnectionState> {
        AsyncStream { continuation in
            for s in list { continuation.yield(s) }
            continuation.finish()
        }
    }

    private func login(_ pasted: String, typed: String = "", store: MemoryTokenStore) throws -> ServerLogin {
        try ServerLogin.resolve(pasted: pasted, typed: typed, store: store).get()
    }

    func testResolvingSavesNothing() throws {
        let store = MemoryTokenStore()
        let l = try login("http://cadpole-host:7331/#token=typo", store: store)
        XCTAssertEqual(l.bearer, "typo")
        XCTAssertEqual(l.source, .pastedURL)
        XCTAssertNil(store.load(host: "cadpole-host:7331"), "not saved before the server accepts it")
    }

    private func engine(for l: ServerLogin, sessionStatus: Int, streamEpisodes: [FakeTransport.StreamEpisode] = []) -> RemoteEngine {
        let transport = FakeTransport(streamEpisodes: streamEpisodes) { _ in
            sessionStatus == 200
                ? HTTPResponse(status: 200, body: Data(#"{"id":"s","revision":3,"last_event_seq":0,"selection":{"selected":[]},"document":{"id":"d","name":"d","display_unit":"mm","scripts":[],"param_overrides":{}}}"#.utf8))
                : HTTPResponse(status: sessionStatus, body: Data("missing or wrong bearer token".utf8))
        }
        return RemoteEngine(baseURL: l.address.baseURL, token: l.bearer, transport: transport, reconnect: .immediate)
    }

    func testTheTokenIsKeptOnceTheServerAcceptsIt() async throws {
        let store = MemoryTokenStore()
        let recents = freshRecents()
        let l = try login("http://cadpole-host:7331/#token=good", store: store)
        let verdict = await l.verify(with: engine(for: l, sessionStatus: 200), store: store, recents: recents)
        XCTAssertNil(verdict.failureValue, "\(verdict)")
        XCTAssertEqual(store.load(host: "cadpole-host:7331"), "good")
        XCTAssertEqual(recents.entries.count, 1)
    }

    func testAMistypedTokenIsNeverSaved() async throws {
        let store = MemoryTokenStore()
        let recents = freshRecents()
        let l = try login("http://cadpole-host:7331/#token=typo", store: store)
        let verdict = await l.verify(with: engine(for: l, sessionStatus: 401), store: store, recents: recents)
        XCTAssertEqual(verdict.failureValue, .rejected(host: "cadpole-host:7331"))
        XCTAssertNil(store.load(host: "cadpole-host:7331"))
        XCTAssertTrue(recents.entries.isEmpty, "a rejected login is not a recent server")
    }

    func testARejectedTypoLeavesTheGoodStoredTokenAlone() async throws {
        let store = MemoryTokenStore()
        try store.save("good", host: "cadpole-host:7331")
        let l = try login("cadpole-host:7331", typed: "typo", store: store)
        XCTAssertEqual(l.source, .typed)
        let verdict = await l.verify(with: engine(for: l, sessionStatus: 401), store: store, recents: freshRecents())
        XCTAssertEqual(verdict.failureValue, .rejected(host: "cadpole-host:7331"))
        XCTAssertEqual(store.load(host: "cadpole-host:7331"), "good", "only the rejected token is forgotten")
    }

    func testARecentServerWithADeadTokenForgetsIt() async throws {
        // Relaunch, tap the recent entry: it pastes only the server, and the
        // token comes from the store -- the one the restarted server rotated.
        let store = MemoryTokenStore()
        try store.save("dead", host: "cadpole-host:7331")
        let l = try login("cadpole-host:7331", store: store)
        XCTAssertEqual(l.source, .stored)
        let verdict = await l.verify(with: engine(for: l, sessionStatus: 401), store: store, recents: freshRecents())
        XCTAssertTrue(verdict.failureValue?.message.contains("cadpole-host:7331") == true, "\(verdict)")
        XCTAssertNil(store.load(host: "cadpole-host:7331"), "a 401 clears the stored token")
        // So the next tap asks for a token instead of reusing the dead one.
        XCTAssertEqual(ServerLogin.resolve(pasted: "cadpole-host:7331", typed: "", store: store).failureValue, .noToken)
    }

    func testAServerThatCannotBeReachedSavesNothing() async throws {
        let store = MemoryTokenStore()
        let l = try login("http://cadpole-host:7331/#token=t", store: store)
        let verdict = await l.verify(with: engine(for: l, sessionStatus: 502), store: store, recents: freshRecents())
        guard case .unreachable = verdict.failureValue else { return XCTFail("\(verdict)") }
        XCTAssertNil(store.load(host: "cadpole-host:7331"))
    }

    func testATokenRotatedMidSessionIsForgotten() async throws {
        // Connected and working; then `cadpole serve` restarts. The event
        // stream gets a 401, the monitor goes .unauthorized, and the login
        // forgets the token and ends, which sends the app back to connect.
        let store = MemoryTokenStore()
        let l = try login("http://cadpole-host:7331/#token=t", store: store)
        let remote = engine(for: l, sessionStatus: 200, streamEpisodes: [.failure(.http(status: 401, body: "wrong bearer token"))])
        _ = await l.verify(with: remote, store: store, recents: freshRecents())
        XCTAssertEqual(store.load(host: "cadpole-host:7331"), "t")

        let watching = Task { await l.watch(remote.connection, store: store) }
        do {
            for try await _ in remote.events(after: 0) {}
        } catch {}
        let outcome = await watching.value
        guard case .rejected(let message) = outcome else { return XCTFail("expected a rejection, got \(outcome)") }
        XCTAssertTrue(message.contains("cadpole-host:7331"), message)
        XCTAssertNil(store.load(host: "cadpole-host:7331"))
    }

    func testAStreamThatEndsWithoutA401KeepsTheToken() async throws {
        let store = MemoryTokenStore()
        try store.save("t", host: "cadpole-host:7331")
        let l = try login("cadpole-host:7331", store: store)
        let outcome = await l.watch(states([.connecting, .live(revision: 1), .degraded(since: Date())]), store: store)
        XCTAssertEqual(outcome, .ended)
        XCTAssertEqual(store.load(host: "cadpole-host:7331"), "t")
    }

    func testPastedAndTypedTokensWinOverAStoredOne() throws {
        let store = MemoryTokenStore()
        try store.save("old", host: "cadpole-host:7331")
        XCTAssertEqual(try login("cadpole-host", typed: " new ", store: store).bearer, "new")
        XCTAssertEqual(try login("http://cadpole-host:7331/#token=url", typed: "new", store: store).bearer, "url")
    }

    func testProblems() {
        let store = MemoryTokenStore()
        XCTAssertEqual(ServerLogin.resolve(pasted: "http://", typed: "", store: store).failureValue, .notAnAddress)
        XCTAssertEqual(ServerLogin.resolve(pasted: "cadpole-host", typed: "", store: store).failureValue, .noToken)
    }
}

extension Result {
    fileprivate var failureValue: Failure? {
        if case .failure(let f) = self { return f }
        return nil
    }
}
