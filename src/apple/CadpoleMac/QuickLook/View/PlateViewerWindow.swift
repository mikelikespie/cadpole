import AppKit
import CadpolePlates
import SceneKit

/// A window around `PlateBrowserViewController`, for running the preview's
/// view outside Quick Look: by hand, or to measure frame times while the
/// camera turns and write a picture of the window.
public enum PlateViewerWindow {
    @MainActor
    public static func run(url: URL, orbitSeconds: Double?, snapshot: URL?, light: Bool = false) {
        let app = NSApplication.shared
        app.setActivationPolicy(.regular)
        if light { app.appearance = NSAppearance(named: .aqua) }
        let browser = PlateBrowserViewController()
        let window = NSWindow(
            contentRect: NSRect(x: 0, y: 0, width: 1100, height: 720),
            styleMask: [.titled, .closable, .resizable, .miniaturizable], backing: .buffered, defer: false)
        window.title = url.lastPathComponent
        window.contentViewController = browser
        window.center()
        let clock = ContinuousClock()
        let t0 = clock.now
        do {
            try browser.load(url: url)
        } catch {
            FileHandle.standardError.write(Data("plate_viewer: \(error)\n".utf8))
            exit(1)
        }
        print("settings read, bed shown: \(clock.now - t0)")
        window.makeKeyAndOrderFront(nil)
        app.activate(ignoringOtherApps: true)
        browser.onFirstModelFrame = { elapsed in
            print("first frame with the model: \(elapsed) after load")
            guard let seconds = orbitSeconds else { return }
            // The default view, then a turn of the camera, then the next plate.
            DispatchQueue.main.asyncAfter(deadline: .now() + 0.4) {
                if let snapshot { write(window: window, browser: browser, to: snapshot) }
                measure(browser, seconds: seconds) {
                    let frames = browser.frames
                    frames.reset()
                    frames.recording = true
                    browser.holdPlaying = true
                    let before = ContinuousClock.now
                    browser.step(1)
                    DispatchQueue.main.asyncAfter(deadline: .now() + 0.8) {
                        browser.holdPlaying = false
                        frames.recording = false
                        let s = frames.samples
                        _ = before
                        print(String(format: "switching to the next plate: %d frames in 0.8 s, worst interval %.2f ms",
                                     s.count, s.max() ?? 0))
                        if let snapshot {
                            let second = snapshot.deletingPathExtension().appendingPathExtension("next.png")
                            write(window: window, browser: browser, to: second)
                        }
                        app.terminate(nil)
                    }
                }
            }
        }
        app.run()
    }

    @MainActor
    private static func measure(_ browser: PlateBrowserViewController, seconds: Double, done: @escaping () -> Void) {
        let view = browser.sceneView
        let frames = browser.frames
        let screen = view.window?.screen
        print("display: \(screen?.localizedName ?? "?"), max \(screen?.maximumFramesPerSecond ?? 0) Hz; view \(Int(view.bounds.width))x\(Int(view.bounds.height)) pt")
        // A turntable at 45 degrees/s, for measuring only; off by default.
        browser.feel.autoRotateDegreesPerSecond = 45
        frames.reset()
        frames.recording = true
        browser.holdPlaying = true
        DispatchQueue.main.asyncAfter(deadline: .now() + seconds) {
            browser.holdPlaying = false
            frames.recording = false
            browser.feel.autoRotateDegreesPerSecond = 0
            let s = frames.samples.sorted()
            guard !s.isEmpty else { print("no frames"); done(); return }
            let mean = s.reduce(0, +) / Double(s.count)
            func pct(_ p: Double) -> Double { s[min(s.count - 1, Int(Double(s.count) * p))] }
            print(String(
                format: "orbit %.1f s: %d frames, mean %.2f ms (%.0f fps), median %.2f ms, p95 %.2f ms, p99 %.2f ms, worst %.2f ms",
                seconds, s.count, mean, 1000 / mean, pct(0.5), pct(0.95), pct(0.99), s.last!))
            done()
        }
    }

    /// The window as drawn: the sidebar from the view hierarchy, the 3D view
    /// from SceneKit's own snapshot.
    @MainActor
    private static func write(window: NSWindow, browser: PlateBrowserViewController, to url: URL) {
        guard let content = window.contentView else { return }
        let scale = window.backingScaleFactor
        let size = content.bounds.size
        guard let rep = content.bitmapImageRepForCachingDisplay(in: content.bounds) else { return }
        content.cacheDisplay(in: content.bounds, to: rep)
        let image = NSImage(size: size)
        image.addRepresentation(rep)
        let composed = NSImage(size: size)
        composed.lockFocus()
        image.draw(in: NSRect(origin: .zero, size: size))
        let v = browser.sceneView
        v.snapshot().draw(in: v.frame)
        composed.unlockFocus()
        guard let tiff = composed.tiffRepresentation, let bitmap = NSBitmapImageRep(data: tiff),
            let png = bitmap.representation(using: .png, properties: [:])
        else { return }
        try? png.write(to: url)
        print("wrote \(url.path) (\(Int(size.width * scale))x\(Int(size.height * scale)) px)")
    }
}
