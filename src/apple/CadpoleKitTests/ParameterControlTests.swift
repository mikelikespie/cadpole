// A parameter row's number and control (CadpoleKit/ParameterControl.swift).

import CadpoleKit
import CadpoleProto
import XCTest

final class ParameterControlTests: XCTestCase {
    // MARK: - The printed value

    func testTheValueReadsAsTheWebPrintsIt() {
        // The value the model is evaluated with, not three significant digits.
        XCTAssertEqual(ParameterControl.format(152.4), "152.4")
        XCTAssertEqual(ParameterControl.format(152.37), "152.37")
        XCTAssertEqual(ParameterControl.format(12.75), "12.75")
        XCTAssertEqual(ParameterControl.format(1234.5), "1234.5")
        XCTAssertEqual(ParameterControl.format(2500.5), "2500.5")
        XCTAssertEqual(ParameterControl.format(999.96), "999.96")
        XCTAssertEqual(ParameterControl.format(25.4), "25.4")
        XCTAssertEqual(ParameterControl.format(1500), "1500")
        XCTAssertEqual(ParameterControl.format(-5), "-5")
        XCTAssertEqual(ParameterControl.format(0), "0")
    }

    func testItIsUnitNamesTrimmedSoBothClientsAgree() {
        for v in [152.4, 0.01234, 1234.5, 2e7, -3.25] {
            XCTAssertEqual(ParameterControl.format(v), UnitNames.trimmed(v), "\(v)")
        }
    }

    // MARK: - The slider

    private func param(
        _ value: Double, default def: Double? = nil, min: Double? = nil, max: Double? = nil, step: Double? = nil
    ) -> Cadpole_V1_ParamInfo {
        var p = Cadpole_V1_ParamInfo()
        p.name = "p"
        p.value = .number(value)
        p.defaultValue = .number(def ?? value)
        if let min { p.min = min }
        if let max { p.max = max }
        if let step { p.step = step }
        return p
    }

    func testADeclaredRangeIsTheSlidersRange() {
        XCTAssertEqual(ParameterControl.sliderRange(param(42, min: 10, max: 120)), 10 ... 120)
        XCTAssertEqual(ParameterControl.sliderRange(param(100, default: 42, min: 10, max: 120)), 10 ... 120)
    }

    func testAnUnboundedRangeDoesNotFollowTheValueBeingDragged() {
        // A drag writes the live value on every frame; a range derived from it
        // doubled under the finger (42 -> 0...84, then 84 -> 0...168).
        XCTAssertEqual(
            ParameterControl.sliderRange(param(42)),
            ParameterControl.sliderRange(param(84, default: 42))
        )
    }

    func testANegativeValueIsInsideItsOwnRange() {
        // -5 used to get -10...-9: a slider handed a value outside its bounds.
        if let range = ParameterControl.sliderRange(param(-5)) {
            XCTAssertTrue(range.contains(-5), "\(range)")
        }
    }

    func testAnUnboundedParameterGetsAFieldAndAStepperNotASlider() {
        // As the web does (src/web/src/ui/params.ts: a slider only "when it
        // has a range"): there is no honest scale to invent.
        XCTAssertNil(ParameterControl.sliderRange(param(42)))
        XCTAssertNil(ParameterControl.sliderRange(param(42, min: 0)))
        XCTAssertNil(ParameterControl.sliderRange(param(42, max: 100)))
        XCTAssertNil(ParameterControl.sliderRange(param(5, min: 5, max: 5)), "an empty range is no range")
    }

    func testTheStepIsTheWebsNiceStep() {
        XCTAssertEqual(ParameterControl.step(param(1, step: 0.25)), 0.25)
        XCTAssertEqual(ParameterControl.step(param(42, min: 10, max: 120)), 1, "span 110 -> 1.1 -> 1")
        XCTAssertEqual(ParameterControl.step(param(2.4, min: 1, max: 6)), 0.05, accuracy: 1e-12, "span 5 -> 0.05")
        XCTAssertEqual(ParameterControl.step(param(500, min: 0, max: 2000)), 20, "span 2000 -> 20")
        XCTAssertEqual(ParameterControl.step(param(42)), 1, "unbounded: one unit a tap")
    }
}
