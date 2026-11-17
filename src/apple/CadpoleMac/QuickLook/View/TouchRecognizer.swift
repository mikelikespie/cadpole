import AppKit

/// Fingers on a Mac's touch display, all of them, as they move.
///
/// On macOS 27 a gesture recognizer is the only way an app is given direct
/// touches: they do not come through `mouseDown` or the responder chain
/// (Apple's TN3212, "Adopting gesture recognizers"). The stock recognizers
/// each boil the fingers down to one gesture, and running a pan, a pinch and
/// a rotation side by side means three recognizers agreeing about who has
/// the touches. The camera wants all of it at once (Maps' two fingers pan,
/// pinch and turn together), so this one reports every finger's move, as the
/// positions before and after, and the camera works the gesture out.
@MainActor
final class TouchRecognizer: NSGestureRecognizer {
    /// The fingers down before and after a change, by identity, in the
    /// view's coordinates (y up), and the event's time. A finger that just
    /// came down is only in `after`; one that just lifted only in `before`.
    var onChange: ((_ before: [Int: CGPoint], _ after: [Int: CGPoint], _ time: TimeInterval) -> Void)?

    private(set) var down: [Int: CGPoint] = [:]

    override init(target: Any?, action: Selector?) {
        super.init(target: target, action: action)
        allowedTouchTypes = [.direct]
        // Never the mouse: the view's own mouse methods have that.
        delaysPrimaryMouseButtonEvents = false
    }

    @available(*, unavailable)
    required init?(coder: NSCoder) { fatalError("no nibs") }

    /// Required of a subclass built with the macOS 27 SDK: the middle of
    /// the fingers that are down.
    override func location(in view: NSView?) -> NSPoint {
        guard !down.isEmpty, let own = self.view else { return .zero }
        let n = CGFloat(down.count)
        let mid = down.values.reduce(CGPoint.zero) { CGPoint(x: $0.x + $1.x / n, y: $0.y + $1.y / n) }
        return own.convert(mid, to: view)
    }

    private func pass(_ event: NSEvent, _ phase: NSTouch.Phase) {
        guard let view else { return }
        let before = down
        for t in event.touches(matching: phase, in: view) where t.type == .direct {
            let id = t.identity.hash
            if phase.contains(.ended) || phase.contains(.cancelled) {
                down[id] = nil
            } else {
                down[id] = t.location(in: view)
            }
        }
        if before != down { onChange?(before, down, event.timestamp) }
    }

    override func touchesBegan(with event: NSEvent) {
        let first = down.isEmpty
        pass(event, .began)
        if !down.isEmpty { state = first ? .began : .changed }
    }

    override func touchesMoved(with event: NSEvent) {
        pass(event, .moved)
        if !down.isEmpty { state = .changed }
    }

    override func touchesEnded(with event: NSEvent) {
        pass(event, .ended)
        // The gesture must end when the last finger lifts, or no other
        // recognizer in the window can start.
        if down.isEmpty { state = .ended }
    }

    override func touchesCancelled(with event: NSEvent) {
        pass(event, .cancelled)
        // Whatever the system did not name is gone too.
        if !down.isEmpty {
            let before = down
            down.removeAll()
            onChange?(before, [:], event.timestamp)
        }
        state = .cancelled
    }

    override func reset() {
        super.reset()
        down.removeAll()
    }
}
