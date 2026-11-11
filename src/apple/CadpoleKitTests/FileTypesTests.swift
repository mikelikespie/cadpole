import XCTest

@testable import CadpoleKit

// The values live in build_support/file_types.bzl; this only checks they are
// usable as two different file extensions.
final class FileTypesTests: XCTestCase {
    func testTheTwoExtensionsAreDistinctPlainNames() {
        let exts = [CadpoleFileTypes.documentExtension, CadpoleFileTypes.bakedExtension]
        XCTAssertNotEqual(exts[0], exts[1])
        for ext in exts {
            XCTAssertFalse(ext.isEmpty)
            XCTAssertFalse(ext.contains("."), ext)
            XCTAssertEqual(ext, ext.lowercased())
        }
        XCTAssertTrue(CadpoleFileTypes.documentTypeIdentifier.hasSuffix(".document"))
    }
}
