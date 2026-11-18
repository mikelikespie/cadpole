import AppKit

/// What a scroll or a pinch over a 3D view does. The web viewer and the
/// site follow the same rule (src/web/README.md, "Viewport controls"):
///
/// - Option + the scroll wheel always zooms, toward the pointer (a
///   trackpad's two-finger scroll with Option too), as in Preview.
/// - The plain wheel scrolls whatever scrolls: over a view that sits in a
///   page that scrolls, the page. Where nothing scrolls (a viewer on its
///   own, as Quick Look is) it zooms toward the pointer.
/// - A pinch zooms, toward the pointer.
/// - A two-finger trackpad scroll pans the model (with shift, orbits it)
///   only where that steals no page's scrolling; in a page it scrolls the
///   page.
enum WheelRule {
    enum Input: Equatable {
        /// A mouse's notched wheel (no precise deltas).
        case wheel
        /// A trackpad's or Magic Mouse's two-finger scroll (precise deltas).
        case trackpadScroll
        /// A trackpad pinch.
        case pinch
    }

    enum Action: Equatable {
        case zoom
        case pan
        case orbit
        /// Left to the page the view sits in.
        case page
    }

    static func action(_ input: Input, option: Bool, shift: Bool = false, inPage: Bool) -> Action {
        switch input {
        case .pinch:
            return .zoom
        case _ where option:
            return .zoom
        case .wheel:
            return inPage ? .page : .zoom
        case .trackpadScroll:
            return inPage ? .page : shift ? .orbit : .pan
        }
    }

    /// Points of a precise scroll to one line of a notched wheel (about
    /// AppKit's own conversion).
    static let pointsPerLine = 10.0

    /// The zoom factor (below 1 comes closer) for a scroll of `deltaY`:
    /// lines for a wheel, points for a precise scroll. AppKit's positive
    /// `scrollingDeltaY` comes closer, as the wheel always has.
    static func zoomFactor(deltaY: Double, precise: Bool, perLine: Double) -> Double {
        let lines = precise ? deltaY / pointsPerLine : deltaY
        return pow(1 - perLine, max(-20, min(20, lines)))
    }
}

extension OrbitCamera {
    /// A scroll over `view`, as `WheelRule` has it. Returns false when the
    /// scroll belongs to the page the view sits in.
    func scrolled(_ event: NSEvent, in view: NSView, inPage: Bool) -> Bool {
        let precise = event.hasPreciseScrollingDeltas
        let flags = event.modifierFlags
        switch WheelRule.action(precise ? .trackpadScroll : .wheel, option: flags.contains(.option),
                                shift: flags.contains(.shift), inPage: inPage) {
        case .page:
            return false
        case .zoom:
            let factor = WheelRule.zoomFactor(deltaY: Double(event.scrollingDeltaY), precise: precise, perLine: feel.wheelZoomPerLine)
            zoom(factor, toward: Self.offset(of: event, in: view), viewHeight: view.bounds.height)
        case .orbit:
            scroll(CGSize(width: -event.scrollingDeltaX, height: event.scrollingDeltaY), viewWidth: view.bounds.width)
        case .pan:
            pan(CGSize(width: event.scrollingDeltaX, height: event.scrollingDeltaY), viewHeight: view.bounds.height)
        }
        return true
    }

    /// A trackpad pinch over `view`: a zoom toward the pointer.
    func magnified(_ event: NSEvent, in view: NSView) {
        zoom(max(0.2, 1 - Double(event.magnification) * feel.pinchZoomRate), toward: Self.offset(of: event, in: view),
             viewHeight: view.bounds.height)
    }

    /// Where an event is, from the view's centre, x right and y up.
    static func offset(of event: NSEvent, in view: NSView) -> CGPoint {
        let p = view.convert(event.locationInWindow, from: nil)
        return CGPoint(x: p.x - view.bounds.midX, y: view.isFlipped ? view.bounds.midY - p.y : p.y - view.bounds.midY)
    }
}
