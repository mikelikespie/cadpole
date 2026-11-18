import Foundation

/// How an orbiting camera answers the mouse, trackpad and fingers. One place
/// to tune, shared by every 3D view so the same hand does the same thing in
/// each.
public struct CameraFeel: Sendable, Equatable {
    /// A drag across the whole view turns the model this far...
    public var fullWidthDragDegrees: Double = 180
    /// ...but never faster than this per point, in a wide view.
    public var maxDegreesPerPoint: Double = 0.25
    /// Shift + two-finger trackpad scroll turns the camera at this fraction
    /// of the drag rate. (Without shift, two fingers pan.)
    public var scrollRateFraction: Double = 0.5
    /// After a flick, the camera keeps this fraction of the release speed...
    public var inertiaVelocityScale: Double = 0.5
    /// ...and slows exponentially with this time constant (s): four time
    /// constants, about 0.6 s, to come to rest.
    public var inertiaTimeConstant: Double = 0.15
    /// Below this speed (degrees/s) the camera stops.
    public var inertiaStopSpeed: Double = 2
    /// Pinch: the distance shrinks by this much per unit of magnification.
    public var pinchZoomRate: Double = 1
    /// A mouse wheel's line zooms by this fraction.
    public var wheelZoomPerLine: Double = 0.06
    /// Elevation limits, degrees above the horizon through the target. The
    /// camera goes under a print bed (the bed fades, `BedFade`) to show the
    /// first layer, but never quite to the pole, where the view would spin.
    public var minElevationDegrees: Double = OrbitLimits.minElevationDegrees
    public var maxElevationDegrees: Double = OrbitLimits.maxElevationDegrees
    /// A coasting camera that runs into an elevation limit slows over about
    /// this long (s) as it nears it, and comes to rest on it, never bouncing
    /// or stopping dead.
    public var limitEaseSeconds: Double = OrbitLimits.limitEaseSeconds
    /// Turntable: off.
    public var autoRotateDegreesPerSecond: Double = 0
    /// Plate switches and reframing glide for this long (s).
    public var transitionSeconds: Double = 0.25

    public init() {}
}

/// The elevation limits every orbiting view keeps, and how a flick that
/// coasts into one comes to rest. Pure, so each view's camera and its tests
/// share it (`conformance.json`).
public enum OrbitLimits {
    /// Under the bed, nearly straight up at it.
    public static let minElevationDegrees = -85.0
    /// Over the top, nearly straight down.
    public static let maxElevationDegrees = 89.0
    public static let limitEaseSeconds = 0.12

    /// `elevation` kept within the feel's limits.
    public static func clamp(_ elevation: Double, _ feel: CameraFeel = CameraFeel()) -> Double {
        min(max(elevation, feel.minElevationDegrees), feel.maxElevationDegrees)
    }

    /// One step of coasting in elevation: where a camera at `elevation`
    /// moving at `velocity` (degrees/s) is `dt` seconds later, and its speed
    /// then. Heading for a limit, the speed is held to what would cover the
    /// room left in `limitEaseSeconds`, so the camera eases onto the limit:
    /// it never passes it, and its last steps are small.
    public static func coast(
        elevation: Double, velocity: Double, dt: Double, feel: CameraFeel = CameraFeel()
    ) -> (elevation: Double, velocity: Double) {
        guard dt > 0, velocity != 0 else { return (clamp(elevation, feel), velocity) }
        let limit = velocity < 0 ? feel.minElevationDegrees : feel.maxElevationDegrees
        let room = abs(limit - elevation)
        let cap = room / max(feel.limitEaseSeconds, dt)
        let v = velocity < 0 ? max(velocity, -cap) : min(velocity, cap)
        let e = clamp(elevation + v * dt, feel)
        // On the limit (to a hundredth of a degree): at rest in elevation.
        return (e, abs(limit - e) < 0.01 ? 0 : v)
    }
}
