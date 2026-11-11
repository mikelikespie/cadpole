// Pasting a URL is the primary way to connect, so it is the first thing that
// has to be right (docs/apple-apps.md).

import Foundation
import CadpoleKit
import XCTest

final class ServerURLTests: XCTestCase {
    func testThePrintedURLCarriesHostAndToken() throws {
        let address = try XCTUnwrap(ServerAddress.parse("http://cadpole-host:7331/#token=abc123"))
        XCTAssertEqual(address.baseURL.absoluteString, "http://cadpole-host:7331")
        XCTAssertEqual(address.token, "abc123")
        XCTAssertEqual(address.host, "cadpole-host:7331")
    }

    func testABareHostGetsTheDefaultSchemeAndPort() throws {
        let address = try XCTUnwrap(ServerAddress.parse("cadpole-host"))
        XCTAssertEqual(address.baseURL.absoluteString, "http://cadpole-host:7331")
        XCTAssertNil(address.token)
    }

    func testHostAndPortWithoutAScheme() throws {
        let address = try XCTUnwrap(ServerAddress.parse("192.0.2.7:8080"))
        XCTAssertEqual(address.baseURL.absoluteString, "http://192.0.2.7:8080")
    }

    func testSurroundingWhitespaceIsIgnored() throws {
        let address = try XCTUnwrap(ServerAddress.parse("  http://cadpole-host:7331/#token=t  \n"))
        XCTAssertEqual(address.token, "t")
    }

    func testAQueryTokenIsAcceptedOnPasteEvenThoughWeNeverSendOne() throws {
        // security.md's ?token= exception exists for EventSource; a user might
        // still paste a URL shaped like that.
        let address = try XCTUnwrap(ServerAddress.parse("http://cadpole-host:7331/api/events?token=q"))
        XCTAssertEqual(address.token, "q")
        XCTAssertEqual(address.baseURL.absoluteString, "http://cadpole-host:7331", "the path and query are dropped")
    }

    func testPercentEncodedTokensAreDecoded() throws {
        let address = try XCTUnwrap(ServerAddress.parse("http://cadpole-host:7331/#token=a%2Bb%3Dc"))
        XCTAssertEqual(address.token, "a+b=c")
    }

    func testFragmentsWithSeveralKeys() {
        XCTAssertEqual(ServerAddress.bearerToken(inFragment: "doc=1&token=zz&x=2"), "zz")
        XCTAssertNil(ServerAddress.bearerToken(inFragment: "doc=1"))
    }

    func testHttpsAndAnExplicitStandardPort() throws {
        let address = try XCTUnwrap(ServerAddress.parse("https://cadpole.example:443/#token=t"))
        XCTAssertEqual(address.host, "cadpole.example", "a standard port is not part of the Keychain key")
    }

    func testGarbageIsRejected() {
        XCTAssertNil(ServerAddress.parse(""))
        XCTAssertNil(ServerAddress.parse("   "))
        XCTAssertNil(ServerAddress.parse("http://"))
    }
}
