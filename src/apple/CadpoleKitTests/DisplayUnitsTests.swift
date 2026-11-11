// Geometry is always millimetres (docs/starlark-api.md) and only
// presentation knows about `display_unit`. These pin the conversion, and pin
// it to the web client's numbers so the browser and the app agree about the
// same body.

import Foundation
import CadpoleKit
import CadpoleProto
import XCTest

final class DisplayUnitsTests: XCTestCase {
    private func vec(_ x: Double, _ y: Double, _ z: Double) -> Cadpole_V1_Vec3 {
        var v = Cadpole_V1_Vec3()
        v.x = x
        v.y = y
        v.z = z
        return v
    }

    /// `UNIT_SCALE` in src/web/src/util.ts.
    func testTheScalesAreTheWebsScales() {
        XCTAssertEqual(DisplayUnit.mm.scale, 1)
        XCTAssertEqual(DisplayUnit.cm.scale, 10)
        XCTAssertEqual(DisplayUnit.m.scale, 1000)
        XCTAssertEqual(DisplayUnit.in.scale, 25.4)
        XCTAssertEqual(DisplayUnit.ft.scale, 304.8)
    }

    func testAnUnknownUnitIsMillimetres() {
        // What the engine actually produced, and what the rest of the client
        // falls back to.
        XCTAssertEqual(DisplayUnit("furlong"), .mm)
        XCTAssertEqual(DisplayUnit(""), .mm)
        XCTAssertEqual(DisplayUnit("in"), .in)
    }

    // MARK: - Conversion

    func testAnInchCubeInAnInchDocumentReadsAsOneInch() {
        // `box(25.4, 25.4, 25.4)` with display_unit "in".
        let size = vec(25.4, 25.4, 25.4)
        XCTAssertEqual(DisplayUnit.in.size(size), "1 \u{00D7} 1 \u{00D7} 1 in")
        XCTAssertEqual(DisplayUnit.in.spokenSize(size), "1 by 1 by 1 inches")
    }

    func testAMillimetreDocumentIsUnchanged() {
        let size = vec(25.4, 25.4, 25.4)
        XCTAssertEqual(DisplayUnit.mm.size(size), "25.4 \u{00D7} 25.4 \u{00D7} 25.4 mm")
        XCTAssertEqual(DisplayUnit.mm.spokenSize(size), "25.4 by 25.4 by 25.4 millimetres")
    }

    func testEveryUnitConvertsAndLabelsItself() {
        let metre = vec(1000, 500, 250)
        XCTAssertEqual(DisplayUnit.cm.size(metre), "100 \u{00D7} 50 \u{00D7} 25 cm")
        XCTAssertEqual(DisplayUnit.m.size(metre), "1 \u{00D7} 0.5 \u{00D7} 0.25 m")
        XCTAssertEqual(DisplayUnit.ft.size(vec(304.8, 304.8, 152.4)), "1 \u{00D7} 1 \u{00D7} 0.5 ft")
    }

    /// A converted value below one must keep its digits, not round to 0.
    func testASmallBodyKeepsItsDigits() {
        XCTAssertEqual(DisplayUnit.in.spokenSize(vec(12.7, 12.7, 12.7)), "0.5 by 0.5 by 0.5 inches")
    }

    func testOneLength() {
        XCTAssertEqual(DisplayUnit.in.length(25.4), "1 in")
        XCTAssertEqual(DisplayUnit.mm.length(2.4), "2.4 mm")
    }

    // MARK: - The vocabulary (one for all three audiences)

    func testSpokenUnitNames() {
        XCTAssertEqual(DisplayUnit.mm.spoken, "millimetres")
        XCTAssertEqual(DisplayUnit.cm.spoken, "centimetres")
        XCTAssertEqual(DisplayUnit.m.spoken, "metres")
        XCTAssertEqual(DisplayUnit.in.spoken, "inches")
        XCTAssertEqual(DisplayUnit.ft.spoken, "feet")
        // A parameter declares its own unit, which is not a length unit at all
        // when it is an angle -- and may be anything the script wants.
        XCTAssertEqual(UnitNames.spoken("deg"), "degrees")
        XCTAssertEqual(UnitNames.spoken("kg"), "kg")
    }

    // MARK: - The digits, against src/web/src/util.ts `fmt`

    func testTheDigitsAreTheWebs() {
        XCTAssertEqual(UnitNames.trimmed(25.4), "25.4")
        XCTAssertEqual(UnitNames.trimmed(1), "1")
        XCTAssertEqual(UnitNames.trimmed(0), "0")
        XCTAssertEqual(UnitNames.trimmed(1.0 / 3.0), "0.333")
        XCTAssertEqual(UnitNames.trimmed(2.0005), "2.001", "rounds at three decimals")
        XCTAssertEqual(UnitNames.trimmed(-4.5), "-4.5")
        XCTAssertEqual(UnitNames.trimmed(1e-12), "0", "rounds to nothing")
        XCTAssertEqual(UnitNames.trimmed(2_000_000), "2.00e+6", "exponential at the top")
        XCTAssertEqual(UnitNames.trimmed(0.00001), "1.00e-5", "and at the bottom")
        XCTAssertEqual(UnitNames.trimmed(.nan), "\u{2013}")
        XCTAssertEqual(UnitNames.trimmed(.infinity), "\u{2013}")
    }

    // MARK: - The proto enum, so there is one table and not four

    func testTheProtoEnumRoundTrips() {
        for unit in DisplayUnit.allCases {
            XCTAssertEqual(DisplayUnit(unit.lengthUnit), unit)
            XCTAssertEqual(DisplayUnit(unit.rawValue), unit)
        }
        XCTAssertEqual(DisplayUnit(Cadpole_V1_LengthUnit.unspecified), .mm)
    }
}
