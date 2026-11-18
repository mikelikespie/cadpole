import Foundation
import ViewerConformance
import ViewerCore
import XCTest

/// ViewerCore against the shared reference (`viewer_conformance.json`): the
/// elevation limits and coasting, the bed's fade under the horizon, the
/// wheel rule and plate switching. Foundation only, so it runs on Linux too.
final class ViewerCoreTests: XCTestCase {
    private func conformance() throws -> ViewerConformance { try ViewerConformance.load() }

    func testElevationLimits() throws {
        let feel = CameraFeel()
        XCTAssertEqual(
            try conformance().checkClamp(min: feel.minElevationDegrees, max: feel.maxElevationDegrees) { OrbitLimits.clamp($0) }, [])
    }

    func testCoastingEasesOntoTheLimit() throws {
        let c = try conformance()
        XCTAssertEqual(c.checkCoastSteps { e, v, dt in OrbitLimits.coast(elevation: e, velocity: v, dt: dt) }, [])
        // The same flick a camera makes: the step, then its own decay and stop.
        XCTAssertEqual(c.checkCoastRuns { e0, v0, dt, seconds in
            let feel = CameraFeel()
            var e = e0, v = v0, path = [e0], t = 0.0
            while t < seconds, v != 0 {
                (e, v) = OrbitLimits.coast(elevation: e, velocity: v, dt: dt)
                v *= exp(-dt / feel.inertiaTimeConstant)
                if abs(v) < feel.inertiaStopSpeed { v = 0 }
                path.append(e)
                t += dt
            }
            return path
        }, [])
    }

    func testBedFadesUnderTheHorizon() throws {
        XCTAssertEqual(try conformance().checkBedFade({ BedFade(elevationDegrees: $0) }, fromHeight: { BedFade(cameraHeight: $0, distance: $1) }), [])
        // Smooth across the plane: no step bigger than a few percent per tenth of a degree.
        var last = BedFade(elevationDegrees: 5).above
        for i in 1...200 {
            let a = BedFade(elevationDegrees: 5 - Double(i) / 10).above
            XCTAssertLessThan(abs(a - last), 0.03)
            XCTAssertLessThanOrEqual(a, last)
            last = a
        }
    }

    func testWheelRule() throws {
        XCTAssertEqual(try conformance().checkWheel({ input, option, shift, inPage in
            WheelRule.action(WheelRule.Input(rawValue: input)!, option: option, shift: shift, inPage: inPage).rawValue
        }, zoomFactor: { WheelRule.zoomFactor(deltaY: $0, precise: $1, perLine: $2) }), [])
    }

    func testPlateSwitching() throws {
        XCTAssertEqual(try conformance().checkPlateLoads { PlateLoadsAdapter() }, [])
    }

    /// The thread-safe switch answers a loader on another queue: a load for a
    /// plate left behind stops (wants is false) and is dropped.
    func testPlateSwitchAcrossQueues() {
        let s = PlateSwitch<Int>()
        let t2 = s.select(2).start!
        let gate = DispatchSemaphore(value: 0)
        let done = DispatchSemaphore(value: 0)
        let stillWanted = Flag()
        DispatchQueue.global().async {
            gate.wait()
            stillWanted.value = s.wants(t2)
            done.signal()
        }
        let t3 = s.select(3).start!
        gate.signal()
        done.wait()
        XCTAssertFalse(stillWanted.value)
        XCTAssertEqual(s.finished(t3), .present)
        XCTAssertEqual(s.finished(t2), .drop)
        XCTAssertEqual(s.selected, 3)
    }
}

private final class Flag: @unchecked Sendable {
    var value = true
}
