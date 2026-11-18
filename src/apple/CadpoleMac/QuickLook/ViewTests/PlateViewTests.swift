import AppKit
import CadpolePlates
import SceneKit
import ViewerConformance
import ViewerFixtures
import XCTest
import simd
@testable import CadpolePlateView

/// The preview's camera and bed against the shared reference
/// (viewer_conformance.json), plate switching with a slow plate left behind,
/// and pictures of the invented project from under the bed and while a plate
/// loads (written to the test's outputs, `under-plate/`).
@MainActor
final class PlateViewTests: XCTestCase {
    private func conformance() throws -> ViewerConformance { try ViewerConformance.load() }

    /// A camera at `elevation` degrees, 300 mm from a target.
    private func camera(elevation: Double, azimuth: Double = -22) -> OrbitCamera {
        let node = SCNNode()
        node.camera = SCNCamera()
        node.camera?.fieldOfView = 30
        let cam = OrbitCamera(node: node)
        place(cam, elevation: elevation, azimuth: azimuth, target: SIMD3(10, 20, -5), distance: 300)
        return cam
    }

    private func place(_ cam: OrbitCamera, elevation: Double, azimuth: Double, target: SIMD3<Float>, distance: Float) {
        let e = elevation * .pi / 180, a = azimuth * .pi / 180
        let dir = SIMD3<Float>(Float(sin(a) * cos(e)), Float(sin(e)), Float(cos(a) * cos(e)))
        cam.set(position: target + dir * distance, target: target, limits: 20 ... 4000, animated: false)
    }

    // MARK: - Camera

    func testTheCameraKeepsTheSharedLimits() throws {
        let feel = CameraFeel()
        XCTAssertEqual(try conformance().checkClamp(min: feel.minElevationDegrees, max: feel.maxElevationDegrees) { e in
            // From the horizon, one drag as far as `e`: the camera stops at a limit.
            let cam = self.camera(elevation: 0)
            let k = min(feel.maxDegreesPerPoint, feel.fullWidthDragDegrees / 800)
            cam.drag(CGSize(width: 0, height: -e / k), dt: 0, viewWidth: 800)
            return cam.elevationDegrees
        }, [])
        // A long drag down goes under the bed and stops at the limit.
        let cam = camera(elevation: 20)
        for _ in 0 ..< 40 { cam.drag(CGSize(width: 0, height: 60), dt: 0.016, viewWidth: 800) }
        XCTAssertEqual(cam.elevationDegrees, feel.minElevationDegrees, accuracy: 1e-9)
        for _ in 0 ..< 80 { cam.drag(CGSize(width: 0, height: -60), dt: 0.016, viewWidth: 800) }
        XCTAssertEqual(cam.elevationDegrees, feel.maxElevationDegrees, accuracy: 1e-9)
    }

    func testAFlickEasesOntoTheLimit() throws {
        XCTAssertEqual(try conformance().checkCoastRuns { e0, v0, dt, seconds in
            let cam = self.camera(elevation: e0)
            cam.fling(elevation: v0)
            var path = [cam.elevationDegrees], t = 0.0
            cam.step(t)
            while t < seconds, cam.isMoving {
                t += dt
                cam.step(t)
                path.append(cam.elevationDegrees)
            }
            return path
        }, [])
    }

    // MARK: - The bed from below

    func testTheBedFadesUnderTheHorizon() throws {
        let scene = PlateScene(bed: Bed(printableArea: ["0x0", "256x0", "256x256", "0x256"])!)
        let target = SIMD3<Float>(128, 10, -128)
        let cam = OrbitCamera(node: scene.camera)
        cam.feel = CameraFeel()
        var last = CGFloat(1)
        for e in stride(from: 40.0, through: -84.0, by: -4) {
            place(cam, elevation: e, azimuth: -22, target: target, distance: 600)
            scene.updateFade(target: target)
            let eye = scene.camera.simdWorldPosition
            let want = BedFade(cameraHeight: Double(eye.y), distance: Double(simd_distance(eye, target)))
            XCTAssertEqual(scene.fade.above, want.above, accuracy: 1e-3, "at \(e)")
            let drawn = scene.drawnFade
            XCTAssertEqual(Double(drawn.fill), want.fill, accuracy: 1e-3)
            XCTAssertEqual(Double(drawn.lines), want.lines, accuracy: 1e-3)
            XCTAssertEqual(Double(drawn.shadowAlpha), 0.22 * want.shadow, accuracy: 1e-3)
            XCTAssertLessThanOrEqual(drawn.fill, last + 1e-6, "fades monotonically")
            last = drawn.fill
        }
        // Well under: a faint sheet, no shadow, the light from below on.
        XCTAssertEqual(Double(scene.drawnFade.fill), BedFade.fillUnder, accuracy: 1e-6)
        XCTAssertEqual(scene.drawnFade.shadowAlpha, 0, accuracy: 1e-6)
        XCTAssertGreaterThan(scene.drawnFade.underLight, 400)
        // Back above: all of it, the under light off.
        place(cam, elevation: 30, azimuth: -22, target: target, distance: 600)
        scene.updateFade(target: target)
        XCTAssertEqual(scene.drawnFade.fill, 1)
        XCTAssertEqual(scene.drawnFade.underLight, 0)
    }

    // MARK: - Switching plates

    private var dir: URL!

    override func setUp() async throws {
        dir = FileManager.default.temporaryDirectory.appendingPathComponent("plate-view-\(UUID().uuidString)")
        try FileManager.default.createDirectory(at: dir, withIntermediateDirectories: true)
    }

    override func tearDown() async throws {
        try? FileManager.default.removeItem(at: dir)
    }

    /// The invented project on disk, with each plate's picture as a slicer
    /// saves it (the parts alone, from the default view).
    private func project() throws -> URL {
        let bare = dir.appendingPathComponent("bare.3mf")
        try DemoPlates.project().write(to: bare)
        let archive = try ZipReader(url: bare)
        let p = try PlateProject(archive: archive)
        let builder = PlateBuilder(archive: archive, project: p)
        var thumbnails: [Int: Data] = [:]
        for plate in p.plates {
            let g = builder.build(plateId: plate.id)
            let scene = PlateScene(bed: p.bed)
            scene.showsBed = false
            scene.show(PlateScene.makeModel(g), bounds: g.bounds, duration: 0)
            scene.frame(aspect: 1, animated: false)
            scene.scene.background.contents = NSColor.clear
            let r = SCNRenderer(device: MTLCreateSystemDefaultDevice(), options: nil)
            r.scene = scene.scene
            r.pointOfView = scene.camera
            thumbnails[plate.id] = try Self.png(r.snapshot(atTime: 0, with: CGSize(width: 512, height: 512), antialiasingMode: .multisampling4X))
        }
        let url = dir.appendingPathComponent("workshop.3mf")
        try DemoPlates.project(thumbnails: thumbnails).write(to: url)
        return url
    }

    private struct Harness {
        let window: NSWindow
        let browser: PlateBrowserViewController
    }

    private func open(_ url: URL, dark: Bool = false, slow: ((Int, () -> Bool) -> Void)? = nil,
                      presented: @escaping (Int) -> Void) throws -> Harness {
        _ = NSApplication.shared
        let browser = PlateBrowserViewController()
        let window = NSWindow(contentRect: NSRect(x: 0, y: 0, width: 1100, height: 720),
                              styleMask: [.titled, .resizable], backing: .buffered, defer: false)
        // Closed by the test, released by Swift: not by AppKit too.
        window.isReleasedWhenClosed = false
        window.appearance = NSAppearance(named: dark ? .darkAqua : .aqua)
        window.contentViewController = browser
        window.setContentSize(NSSize(width: 1100, height: 720))
        window.orderFrontRegardless()
        browser.afterBuild = slow
        browser.onPresent = presented
        try browser.load(url: url)
        browser.view.layoutSubtreeIfNeeded()
        return Harness(window: window, browser: browser)
    }

    /// Spins the main run loop until `done` or `seconds` pass.
    private func spin(until done: () -> Bool, seconds: Double) {
        let end = Date().addingTimeInterval(seconds)
        while !done(), Date() < end { RunLoop.main.run(until: Date().addingTimeInterval(0.01)) }
    }

    private func spin(_ seconds: Double) { spin(until: { false }, seconds: seconds) }

    /// Plate 2 is slow and its build cannot stop: switching 1, 2, 3 quickly
    /// shows plate 3, and plate 2's model, finishing after, never replaces it.
    func testASlowPlateLeftBehindNeverReplacesTheOneShown() throws {
        let url = try project()
        var presented: [Int] = []
        let lock = NSLock()
        let h = try open(url, slow: { plate, _ in if plate == 2 { Thread.sleep(forTimeInterval: 0.8) } }) { id in
            lock.withLock { presented.append(id) }
        }
        spin(until: { lock.withLock { presented == [1] } }, seconds: 20)
        XCTAssertEqual(presented, [1])
        h.browser.select(2, animated: true)
        // The old model is gone at once; plate 2's own picture stands in.
        XCTAssertNil(h.browser.currentScene?.model)
        XCTAssertTrue(h.browser.isShowingPlaceholder)
        XCTAssertTrue(h.browser.isLoading)
        h.browser.select(3, animated: true)
        XCTAssertNil(h.browser.currentScene?.model)
        spin(until: { lock.withLock { presented.contains(3) } }, seconds: 20)
        // Long enough for plate 2's late model to have arrived.
        spin(1.5)
        XCTAssertEqual(lock.withLock { presented }, [1, 3])
        XCTAssertEqual(h.browser.currentScene?.model?.name, "plate 3")
        XCTAssertEqual(h.browser.shownPlate, 3)
        XCTAssertFalse(h.browser.isLoading)
        h.window.close()
    }

    /// Plate 2 is slow but stops when left: plate 3 does not wait for it.
    func testASwitchCancelsTheSlowPlate() throws {
        let url = try project()
        var presented: [Int] = []
        let lock = NSLock()
        let h = try open(url, slow: { plate, wanted in
            guard plate == 2 else { return }
            let end = Date().addingTimeInterval(5)
            while wanted(), Date() < end { Thread.sleep(forTimeInterval: 0.01) }
        }) { id in lock.withLock { presented.append(id) } }
        spin(until: { lock.withLock { presented == [1] } }, seconds: 20)
        h.browser.select(2, animated: true)
        spin(0.1)
        let switched = Date()
        h.browser.select(3, animated: true)
        spin(until: { lock.withLock { presented.contains(3) } }, seconds: 20)
        XCTAssertLessThan(Date().timeIntervalSince(switched), 2.5, "plate 3 did not wait out plate 2")
        spin(0.5)
        XCTAssertEqual(lock.withLock { presented }, [1, 3])
        XCTAssertEqual(h.browser.currentScene?.model?.name, "plate 3")
        h.window.close()
    }

    // MARK: - Pictures

    private static func png(_ image: NSImage) throws -> Data {
        guard let tiff = image.tiffRepresentation, let rep = NSBitmapImageRep(data: tiff),
              let data = rep.representation(using: .png, properties: [:]) else { throw CocoaError(.fileWriteUnknown) }
        return data
    }

    private func write(_ image: NSImage, _ name: String) throws {
        let env = ProcessInfo.processInfo.environment
        let base = URL(fileURLWithPath: env["TEST_UNDECLARED_OUTPUTS_DIR"] ?? NSTemporaryDirectory())
            .appendingPathComponent("under-plate")
        try FileManager.default.createDirectory(at: base, withIntermediateDirectories: true)
        try Self.png(image).write(to: base.appendingPathComponent(name))
    }

    /// The 3D view as drawn: rendered off screen, with the views over it
    /// (the plate's picture, the loading mark) on top.
    private func shot(_ h: Harness) -> NSImage {
        let scale: CGFloat = 2
        let b = h.browser
        if let orbit = b.orbit { b.currentScene?.updateFade(target: orbit.targetPoint) }
        let v = b.sceneView
        let r = SCNRenderer(device: MTLCreateSystemDefaultDevice(), options: nil)
        r.scene = v.scene
        r.pointOfView = v.pointOfView
        let background = v.scene?.background.contents
        v.scene?.background.contents = v.backgroundColor
        let stage = r.snapshot(atTime: CACurrentMediaTime(), with: CGSize(width: v.bounds.width * scale, height: v.bounds.height * scale),
                               antialiasingMode: .multisampling4X)
        v.scene?.background.contents = background
        let out = NSImage(size: v.frame.size)
        out.lockFocus()
        stage.draw(in: NSRect(origin: .zero, size: v.frame.size))
        for sub in b.view.subviews where sub !== v && !sub.isHidden && sub.alphaValue > 0 && v.frame.contains(sub.frame) {
            guard let rep = sub.bitmapImageRepForCachingDisplay(in: sub.bounds) else { continue }
            sub.cacheDisplay(in: sub.bounds, to: rep)
            let img = NSImage(size: sub.bounds.size)
            img.addRepresentation(rep)
            img.draw(in: sub.frame.offsetBy(dx: -v.frame.minX, dy: -v.frame.minY), from: .zero, operation: .sourceOver, fraction: sub.alphaValue)
        }
        out.unlockFocus()
        return out
    }

    /// The invented plate from under the bed, at -30 and -80 degrees, light
    /// and dark; and the moment a plate is loading: its picture over the
    /// empty bed, with the loading mark.
    func testPictures() throws {
        let url = try project()
        for dark in [false, true] {
            var presented: [Int] = []
            let h = try open(url, dark: dark, slow: { plate, wanted in
                guard plate == 2 else { return }
                let end = Date().addingTimeInterval(5)
                while wanted(), Date() < end { Thread.sleep(forTimeInterval: 0.01) }
            }) { presented.append($0) }
            spin(until: { presented == [1] }, seconds: 20)
            spin(0.6)
            let b = h.browser
            let orbit = try XCTUnwrap(b.orbit)
            let scene = try XCTUnwrap(b.currentScene)
            let (position, target) = scene.framing(aspect: b.sceneView.bounds.width / b.sceneView.bounds.height)
            let distance = simd_distance(position, target)
            let mode = dark ? "dark" : "light"
            try write(shot(h), "\(mode)-above.png")
            for e in [-30.0, -80.0] {
                place(orbit, elevation: e, azimuth: -22, target: target, distance: distance)
                try write(shot(h), "\(mode)-under-\(Int(-e)).png")
            }
            // Back to the default view, then the next plate (as the arrow key
            // does): its picture and the loading mark until it is built.
            b.reframe()
            spin(0.5)
            b.step(1)
            spin(0.3)
            XCTAssertEqual(b.shownPlate, 2)
            XCTAssertTrue(b.isShowingPlaceholder)
            try write(shot(h), "\(mode)-switching-to-plate-2.png")
            b.step(1)
            spin(until: { presented.contains(3) }, seconds: 20)
            spin(0.6)
            try write(shot(h), "\(mode)-plate-3.png")
            h.window.close()
        }
    }
}
