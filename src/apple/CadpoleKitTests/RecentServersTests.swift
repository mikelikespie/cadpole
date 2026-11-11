// A "Recent servers" entry reconnects to the server it was saved from: the
// entry is re-parsed by the Recent button, so it must keep the scheme, the
// port and an IPv6 literal's brackets.

import Foundation
import CadpoleKit
import XCTest

final class RecentServersTests: XCTestCase {
    private func freshRecents() -> RecentServers {
        let defaults = UserDefaults(suiteName: "cadpole-recents-\(UUID().uuidString)")!
        defaults.removeObject(forKey: RecentServers.key)
        return RecentServers(defaults: defaults)
    }

    private func assertRoundTrips(_ pasted: String, expect base: String, file: StaticString = #filePath, line: UInt = #line) throws {
        let address = try XCTUnwrap(ServerAddress.parse(pasted), file: file, line: line)
        XCTAssertEqual(address.baseURL.absoluteString, base, "first connect", file: file, line: line)
        let recents = freshRecents()
        recents.remember(address)
        let entry = try XCTUnwrap(recents.entries.first, file: file, line: line)
        let again = try XCTUnwrap(ServerAddress.parse(entry), "recent entry \(entry) does not parse", file: file, line: line)
        XCTAssertEqual(again.baseURL, address.baseURL, "recent entry \(entry)", file: file, line: line)
        XCTAssertEqual(again.host, address.host, "the Keychain key is the same", file: file, line: line)
    }

    func testThePrintedURL() throws {
        try assertRoundTrips("http://cadpole-host:7331/#token=t", expect: "http://cadpole-host:7331")
        try assertRoundTrips("cadpole-host", expect: "http://cadpole-host:7331")
        try assertRoundTrips("http://127.0.0.1:7331", expect: "http://127.0.0.1:7331")
    }

    func testHttpsKeepsItsSchemeAndPort() throws {
        try assertRoundTrips("https://cadpole.example:443", expect: "https://cadpole.example:443")
        try assertRoundTrips("https://cadpole.example:8443", expect: "https://cadpole.example:8443")
    }

    func testHttpsWithoutAPortIsTheStandardPortNot7331() throws {
        // A TLS front (a reverse proxy, `tailscale serve`) listens on 443.
        try assertRoundTrips("https://cadpole.example/", expect: "https://cadpole.example")
    }

    func testAnExplicitPort80IsKept() throws {
        try assertRoundTrips("http://cadpole.example:80", expect: "http://cadpole.example:80")
    }

    func testAnIPv6LiteralKeepsItsBrackets() throws {
        try assertRoundTrips("http://[fd7a::1]:7331/#token=t", expect: "http://[fd7a::1]:7331")
        XCTAssertEqual(ServerAddress.parse("http://[fd7a::1]:7331")?.host, "[fd7a::1]:7331")
    }

    func testOldBareHostEntriesStillParse() throws {
        // Entries saved before entries were URLs.
        let address = try XCTUnwrap(ServerAddress.parse("cadpole-host:7331"))
        XCTAssertEqual(address.baseURL.absoluteString, "http://cadpole-host:7331")
    }

    func testNewestFirstNoDuplicatesAtMostEight() throws {
        let recents = freshRecents()
        for i in 0 ..< 10 {
            recents.remember(try XCTUnwrap(ServerAddress.parse("host\(i)")))
        }
        recents.remember(try XCTUnwrap(ServerAddress.parse("host5")))
        XCTAssertEqual(recents.entries.count, RecentServers.limit)
        XCTAssertEqual(recents.entries.first, "http://host5:7331")
        XCTAssertEqual(recents.entries.filter { $0 == "http://host5:7331" }.count, 1)
    }
}
