import AppKit
import SceneKit
import XCTest
import simd
@testable import CadpolePlateView

/// The wheel-and-pinch rule (View/WheelRule.swift) the preview follows, as
/// the web viewer does: the rule itself, the zoom toward the pointer, and
/// the camera answering real scroll events on its own and inside a page.
@MainActor
final class WheelTests: XCTestCase {
    func testTheRule() {
        typealias R = WheelRule
        for inPage in [false, true] {
            for shift in [false, true] {
                // Option + wheel, Option + two fingers and a pinch zoom, wherever the view is.
                XCTAssertEqual(R.action(.wheel, option: true, shift: shift, inPage: inPage), .zoom)
                XCTAssertEqual(R.action(.trackpadScroll, option: true, shift: shift, inPage: inPage), .zoom)
                XCTAssertEqual(R.action(.pinch, option: false, shift: shift, inPage: inPage), .zoom)
            }
        }
        XCTAssertEqual(R.action(.wheel, option: false, inPage: true), .page)
        XCTAssertEqual(R.action(.wheel, option: false, inPage: false), .zoom)
        XCTAssertEqual(R.action(.trackpadScroll, option: false, inPage: false), .pan)
        XCTAssertEqual(R.action(.trackpadScroll, option: false, shift: true, inPage: false), .orbit)
        XCTAssertEqual(R.action(.trackpadScroll, option: false, inPage: true), .page)
        XCTAssertEqual(R.action(.trackpadScroll, option: false, shift: true, inPage: true), .page)
        XCTAssertEqual(R.zoomFactor(deltaY: 1, precise: false, perLine: 0.06), 0.94, accuracy: 1e-9)
        XCTAssertEqual(R.zoomFactor(deltaY: 10, precise: true, perLine: 0.06), 0.94, accuracy: 1e-9)
    }

    private func camera() -> OrbitCamera {
        let node = SCNNode()
        node.camera = SCNCamera()
        node.camera?.fieldOfView = 30
        let cam = OrbitCamera(node: node)
        cam.set(position: SIMD3(60, 50, 120), target: SIMD3(5, 2, -3), limits: 20 ... 400, animated: false)
        return cam
    }

    private func distance(_ cam: OrbitCamera) -> Float { simd_length(cam.node.simdWorldPosition - cam.targetPoint) }

    /// Where a world point lands on screen, from the centre in points, y up.
    private func project(_ p: SIMD3<Float>, _ node: SCNNode, viewHeight: Double) -> SIMD2<Double> {
        let c = node.simdWorldTransform.inverse * SIMD4(p, 1)
        let k = viewHeight / 2 / tan(Double(node.camera!.fieldOfView) * .pi / 360)
        return SIMD2(Double(c.x) / Double(-c.z) * k, Double(c.y) / Double(-c.z) * k)
    }

    func testZoomTowardThePointerKeepsWhatIsUnderIt() {
        let cam = camera()
        let h = 600.0
        let pointer = SIMD2(220.0, -130.0)
        let k = 2 * Double(distance(cam)) * tan(30 * .pi / 360) / h
        let under = cam.targetPoint + cam.node.simdWorldRight * Float(pointer.x * k) + cam.node.simdWorldUp * Float(pointer.y * k)
        let d0 = distance(cam)
        cam.zoom(0.5, toward: CGPoint(x: pointer.x, y: pointer.y), viewHeight: h)
        XCTAssertEqual(Double(distance(cam) / d0), 0.5, accuracy: 1e-4)
        let after = project(under, cam.node, viewHeight: h)
        XCTAssertEqual(after.x, pointer.x, accuracy: 0.05, "what was under the pointer is still under it")
        XCTAssertEqual(after.y, pointer.y, accuracy: 0.05)
        // Past the far limit the camera stops there; the point still stays.
        let k2 = 2 * Double(distance(cam)) * tan(30 * .pi / 360) / h
        let under2 = cam.targetPoint + cam.node.simdWorldRight * Float(pointer.x * k2) + cam.node.simdWorldUp * Float(pointer.y * k2)
        cam.zoom(100, toward: CGPoint(x: pointer.x, y: pointer.y), viewHeight: h)
        XCTAssertEqual(distance(cam), 400, accuracy: 0.01)
        let after2 = project(under2, cam.node, viewHeight: h)
        XCTAssertEqual(after2.x, pointer.x, accuracy: 0.05)
        XCTAssertEqual(after2.y, pointer.y, accuracy: 0.05)
    }

    /// A scroll: a notched wheel (lines) or a trackpad's (precise pixels).
    private func scroll(continuous: Bool, flags: CGEventFlags = []) throws -> NSEvent {
        let e = try XCTUnwrap(CGEvent(scrollWheelEvent2Source: nil, units: continuous ? .pixel : .line, wheelCount: 1,
                                      wheel1: continuous ? 20 : 2, wheel2: 0, wheel3: 0))
        e.setIntegerValueField(.scrollWheelEventIsContinuous, value: continuous ? 1 : 0)
        e.flags = flags
        let event = try XCTUnwrap(NSEvent(cgEvent: e))
        XCTAssertEqual(event.hasPreciseScrollingDeltas, continuous)
        return event
    }

    func testOnItsOwnTheWheelZoomsAndTwoFingersPan() throws {
        let cam = camera()
        let view = NSView(frame: CGRect(x: 0, y: 0, width: 800, height: 600))
        var d = distance(cam)
        XCTAssertTrue(cam.scrolled(try scroll(continuous: false), in: view, inPage: false))
        XCTAssertLessThan(distance(cam), d, "the wheel zoomed")

        d = distance(cam)
        let t = cam.targetPoint
        XCTAssertTrue(cam.scrolled(try scroll(continuous: true), in: view, inPage: false))
        XCTAssertEqual(distance(cam), d, accuracy: 1e-3)
        XCTAssertGreaterThan(simd_distance(cam.targetPoint, t), 0, "two fingers panned")

        d = distance(cam)
        XCTAssertTrue(cam.scrolled(try scroll(continuous: true, flags: .maskAlternate), in: view, inPage: false))
        XCTAssertLessThan(distance(cam), d, "Option + two fingers zoomed")
    }

    func testInAPageThePageKeepsThePlainWheelAndTwoFingers() throws {
        let cam = camera()
        let view = NSView(frame: CGRect(x: 0, y: 0, width: 800, height: 600))
        let p = cam.node.simdWorldPosition
        XCTAssertFalse(cam.scrolled(try scroll(continuous: false), in: view, inPage: true), "the page's")
        XCTAssertFalse(cam.scrolled(try scroll(continuous: true), in: view, inPage: true), "the page's")
        XCTAssertEqual(cam.node.simdWorldPosition, p, "the model stayed")
        let d = distance(cam)
        XCTAssertTrue(cam.scrolled(try scroll(continuous: false, flags: .maskAlternate), in: view, inPage: true))
        XCTAssertLessThan(distance(cam), d, "Option + wheel zoomed")
    }
}
