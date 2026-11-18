import AppKit

// The rule itself is ViewerCore's `WheelRule`: one rule for every viewer.

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
