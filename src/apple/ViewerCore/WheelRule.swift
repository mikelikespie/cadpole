import Foundation

/// What a scroll or a pinch over a 3D view does. One rule for every viewer
/// (the web viewer and the site follow it too: src/web/README.md, "Viewport
/// controls"), so the same hand does the same thing in each:
///
/// - **Option + the scroll wheel always zooms, toward the pointer**, as in
///   Preview (a trackpad's two-finger scroll with Option too), wherever the
///   viewer is.
/// - **The plain wheel scrolls whatever scrolls**: over a viewer that sits
///   in a page that scrolls, the page. Where nothing scrolls (a viewer on
///   its own: a window, Quick Look, a full-screen player) it zooms toward
///   the pointer.
/// - **A pinch zooms**, toward the pointer.
/// - **A two-finger trackpad scroll pans the model** (with shift, orbits it)
///   only where that steals no page's scrolling; in a page it scrolls the
///   page.
public enum WheelRule {
    public enum Input: String, Equatable, Sendable, CaseIterable {
        /// A mouse's notched wheel (AppKit: no precise deltas; UIKit: a
        /// discrete scroll).
        case wheel
        /// A trackpad's or Magic Mouse's two-finger scroll (precise deltas,
        /// a continuous scroll).
        case trackpadScroll
        /// A trackpad pinch.
        case pinch
    }

    public enum Action: String, Equatable, Sendable {
        /// Zoom toward the pointer.
        case zoom
        /// Move the model with the fingers.
        case pan
        /// Turn the model (shift and two fingers).
        case orbit
        /// Leave it to the page the viewer sits in.
        case page
    }

    /// - Parameters:
    ///   - option: Option is held.
    ///   - shift: shift is held.
    ///   - inPage: the viewer sits in a page that scrolls.
    public static func action(_ input: Input, option: Bool, shift: Bool = false, inPage: Bool) -> Action {
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
    public static let pointsPerLine = 10.0

    /// The zoom factor (below 1 comes closer) for a scroll of `deltaY`:
    /// lines for a wheel, points for a precise scroll. Up, away from the
    /// person (AppKit's positive `scrollingDeltaY`), comes closer, as the
    /// wheel always has.
    public static func zoomFactor(deltaY: Double, precise: Bool, perLine: Double) -> Double {
        let lines = precise ? deltaY / pointsPerLine : deltaY
        return pow(1 - perLine, max(-20, min(20, lines)))
    }
}
