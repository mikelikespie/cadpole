// JSONValue's accessors return nil for what they cannot represent; they never
// trap on a value a server sent.

import Foundation
import CadpoleKit
import XCTest

final class JSONValueTests: XCTestCase {
    private func decode(_ json: String) throws -> JSONValue {
        try JSONDecoder().decode(JSONValue.self, from: Data(json.utf8))
    }

    func testUInt64InRange() throws {
        XCTAssertEqual(try decode("5").uint64Value, 5)
        XCTAssertEqual(try decode("0").uint64Value, 0)
        XCTAssertEqual(JSONValue.number(9_007_199_254_740_992).uint64Value, 9_007_199_254_740_992)
    }

    func testUInt64OutOfRangeIsNilNotACrash() throws {
        XCTAssertNil(try decode("1e30").uint64Value)
        XCTAssertNil(try decode("18446744073709551616").uint64Value, "2^64")
        XCTAssertNil(JSONValue.number(.infinity).uint64Value)
        XCTAssertNil(JSONValue.number(.nan).uint64Value)
        XCTAssertNil(try decode("-1").uint64Value)
    }

    func testAFractionIsNotACount() {
        // seq and revision are integers; 1.5 is not one of them.
        XCTAssertNil(JSONValue.number(1.5).uint64Value)
    }

    func testNonNumbers() {
        XCTAssertNil(JSONValue.string("5").uint64Value)
        XCTAssertNil(JSONValue.null.uint64Value)
    }
}
