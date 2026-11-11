// The two apps' Info.plists, as built (the expanded `:info_plist` outputs),
// checked on any host: what the system reads before a line of the app runs.
//
//   bazel test //src/apple/CadpoleMac:InfoPlistTests

import CadpoleKit
import Foundation
import XCTest

final class InfoPlistTests: XCTestCase {
    private func plist(_ app: String) throws -> [String: Any] {
        let root = ProcessInfo.processInfo.environment["TEST_SRCDIR"] ?? "."
        let url = URL(fileURLWithPath: root).appendingPathComponent("_main/src/apple/\(app)/Info.plist")
        let data = try Data(contentsOf: url)
        let value = try PropertyListSerialization.propertyList(from: data, format: nil)
        return try XCTUnwrap(value as? [String: Any], "\(app) Info.plist is not a dictionary")
    }

    private static let apps = ["CadpoleMac", "CadpoleiOS"]

    // MARK: - Loose .star scripts

    /// Without a declaration, `.star` gets a dynamic type that conforms only
    /// to public.data, so the open panel (which offers the document type and
    /// plain text) greys every script out.
    func testDotStarIsDeclaredAsAPlainTextScript() throws {
        for app in Self.apps {
            let imported = try XCTUnwrap(plist(app)["UTImportedTypeDeclarations"] as? [[String: Any]], app)
            let star = try XCTUnwrap(
                imported.first { ($0["UTTypeIdentifier"] as? String) == CadpoleFileTypes.starlarkTypeIdentifier },
                "\(app) does not declare \(CadpoleFileTypes.starlarkTypeIdentifier)"
            )
            let conforms = star["UTTypeConformsTo"] as? [String] ?? []
            XCTAssertTrue(conforms.contains("public.plain-text") || conforms.contains("public.script"), "\(app): \(conforms)")
            let tags = star["UTTypeTagSpecification"] as? [String: Any]
            XCTAssertEqual(tags?["public.filename-extension"] as? [String], [CadpoleFileTypes.starlarkExtension], app)
        }
    }

    func testTheMacOpensStarlarkScripts() throws {
        let types = try XCTUnwrap(plist("CadpoleMac")["CFBundleDocumentTypes"] as? [[String: Any]])
        let handled = types.flatMap { $0["LSItemContentTypes"] as? [String] ?? [] }
        XCTAssertTrue(handled.contains(CadpoleFileTypes.starlarkTypeIdentifier), "\(handled)")
    }

    // MARK: - App Transport Security

    /// `cadpole serve` speaks plain HTTP (loopback or a tailnet). ATS
    /// exempts IP addresses and unqualified names, but a full MagicDNS name
    /// (`host.<tailnet>.ts.net`, what Tailscale's copy button gives) failed with
    /// NSURLErrorDomain -1022 until the apps carried an exception for it.
    func testPlainHttpToAFullMagicDNSNameIsAllowed() throws {
        for app in Self.apps {
            let ats = try XCTUnwrap(plist(app)["NSAppTransportSecurity"] as? [String: Any], "\(app) has no NSAppTransportSecurity")
            let domains = try XCTUnwrap(ats["NSExceptionDomains"] as? [String: Any], app)
            let tsnet = try XCTUnwrap(domains["ts.net"] as? [String: Any], "\(app): no ts.net exception")
            XCTAssertEqual(tsnet["NSExceptionAllowsInsecureHTTPLoads"] as? Bool, true, app)
            XCTAssertEqual(tsnet["NSIncludesSubdomains"] as? Bool, true, app)
            XCTAssertEqual(ats["NSAllowsLocalNetworking"] as? Bool, true, "\(app): .local names")
            XCTAssertNil(ats["NSAllowsArbitraryLoads"], "\(app): cleartext stays limited to the tailnet and the local network")
        }
    }
}
