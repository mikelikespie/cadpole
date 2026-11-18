import AppKit
import SceneKit
import simd

/// How the camera answers the mouse and trackpad. One place to tune.
public struct CameraFeel: Sendable {
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
    /// Elevation limits (degrees above the bed), so the camera never goes
    /// under the plate or over the top.
    public var minElevationDegrees: Double = 2
    public var maxElevationDegrees: Double = 89
    /// Turntable: off.
    public var autoRotateDegreesPerSecond: Double = 0
    /// Plate switches and reframing glide for this long (s).
    public var transitionSeconds: Double = 0.25

    public init() {}
}

/// An orbiting camera around a target: azimuth, elevation and distance,
/// driven by drags and one finger (orbit), two-finger scrolls and two
/// fingers (pan), pinches (zoom), a two-finger twist (turn), with gentle inertia, and gliding
/// to a new framing when the plate changes. Updated from the render loop.
final class OrbitCamera: @unchecked Sendable {
    var feel = CameraFeel()
    let node: SCNNode
    private let lock = NSLock()

    // State, in SceneKit's y-up space; angles in degrees.
    private var target = SIMD3<Double>.zero
    private var azimuth = 0.0
    private var elevation = 30.0
    private var distance = 100.0
    private var limits: ClosedRange<Double> = 1 ... 10_000
    private var velocity = SIMD2<Double>.zero
    private var glide: (from: Pose, to: Pose, start: TimeInterval?, seconds: Double)?
    private var lastTime: TimeInterval?

    struct Pose {
        var target: SIMD3<Double>
        var azimuth: Double
        var elevation: Double
        var distance: Double
    }

    init(node: SCNNode) {
        self.node = node
    }

    /// Whether the render loop has work: inertia, a glide or a turntable.
    var isMoving: Bool {
        lock.withLock { glide != nil || simd_length(velocity) > 0 || feel.autoRotateDegreesPerSecond != 0 }
    }

    /// Looks from `position` at `target`, gliding there when `animated`.
    func set(position: SIMD3<Float>, target: SIMD3<Float>, limits: ClosedRange<Float>, animated: Bool) {
        let t = SIMD3<Double>(target)
        let o = SIMD3<Double>(position) - t
        let d = simd_length(o)
        let to = Pose(
            target: t,
            azimuth: atan2(o.x, o.z) * 180 / .pi,
            elevation: asin(max(-1, min(1, o.y / max(d, 1e-9)))) * 180 / .pi,
            distance: d)
        lock.withLock {
            self.limits = Double(limits.lowerBound) ... Double(limits.upperBound)
            velocity = .zero
            if animated {
                var from = pose
                // Take the short way round.
                while to.azimuth - from.azimuth > 180 { from.azimuth += 360 }
                while to.azimuth - from.azimuth < -180 { from.azimuth -= 360 }
                glide = (from, to, nil, feel.transitionSeconds)
            } else {
                glide = nil
                pose = to
            }
        }
        apply()
    }

    private var pose: Pose {
        get { Pose(target: target, azimuth: azimuth, elevation: elevation, distance: distance) }
        set {
            target = newValue.target
            azimuth = newValue.azimuth
            elevation = min(max(newValue.elevation, feel.minElevationDegrees), feel.maxElevationDegrees)
            distance = min(max(newValue.distance, limits.lowerBound), limits.upperBound)
        }
    }

    var targetPoint: SIMD3<Float> { lock.withLock { SIMD3<Float>(target) } }

    // MARK: - Input (main thread)

    private func degreesPerPoint(_ viewWidth: CGFloat) -> Double {
        min(feel.maxDegreesPerPoint, feel.fullWidthDragDegrees / Double(max(viewWidth, 1)))
    }

    /// A drag of `delta` points (y up), `dt` seconds after the last.
    func drag(_ delta: CGSize, dt: Double, viewWidth: CGFloat) {
        let k = degreesPerPoint(viewWidth)
        lock.withLock {
            glide = nil
            azimuth -= Double(delta.width) * k
            pose.elevation = elevation - Double(delta.height) * k
            if dt > 0 {
                // Release velocity, smoothed over the last few events.
                let v = SIMD2(-Double(delta.width) * k / dt, -Double(delta.height) * k / dt)
                velocity = velocity * 0.5 + v * 0.5
            }
        }
        apply()
    }

    /// The drag ended: coast with what is left of its speed.
    func release(stale: Bool) {
        lock.withLock {
            velocity = stale ? .zero : velocity * feel.inertiaVelocityScale
            lastTime = nil
        }
    }

    func stop() {
        lock.withLock { velocity = .zero }
    }

    /// Two-finger trackpad scroll: moves the target in the view's plane so
    /// the model follows the fingers, the way a Mac trackpad moves content.
    /// `delta` is AppKit's scrolling delta (y down), in points.
    func pan(_ delta: CGSize, viewHeight: CGFloat) {
        let fov = Double(node.camera?.fieldOfView ?? 30) * .pi / 180
        lock.withLock {
            glide = nil
            velocity = .zero
            // World units per point at the target's depth.
            let k = 2 * distance * tan(fov / 2) / Double(max(viewHeight, 1))
            let (right, up) = screenAxes
            target += (up * Double(delta.height) - right * Double(delta.width)) * k
        }
        apply()
    }

    /// The view's right and up, in the world (under the lock).
    private var screenAxes: (right: SIMD3<Double>, up: SIMD3<Double>) {
        let a = azimuth * .pi / 180
        let e = elevation * .pi / 180
        return (SIMD3(cos(a), 0, -sin(a)), SIMD3(-sin(a) * sin(e), cos(e), -cos(a) * sin(e)))
    }

    /// Shift + two-finger trackpad scroll: a slower orbit.
    func scroll(_ delta: CGSize, viewWidth: CGFloat) {
        let k = degreesPerPoint(viewWidth) * feel.scrollRateFraction
        lock.withLock {
            glide = nil
            velocity = .zero
            azimuth -= Double(delta.width) * k
            pose.elevation = elevation - Double(delta.height) * k
        }
        apply()
    }

    /// Turns the camera about the target's vertical axis (two fingers
    /// twisting on a touch display).
    func turn(degrees: Double) {
        lock.withLock {
            glide = nil
            velocity = .zero
            azimuth += degrees
        }
        apply()
    }

    /// Zoom by `factor` (< 1 closer).
    func zoom(_ factor: Double) {
        lock.withLock {
            glide = nil
            pose.distance = distance * factor
        }
        apply()
    }

    /// Zoom by `factor` (< 1 closer) toward a point of the view: what is
    /// under it, at the target's depth, stays under it. `offset` is the
    /// point's distance from the view's centre in points, x right and y up.
    func zoom(_ factor: Double, toward offset: CGPoint, viewHeight: CGFloat) {
        let fov = Double(node.camera?.fieldOfView ?? 30) * .pi / 180
        lock.withLock {
            glide = nil
            let before = distance
            pose.distance = distance * factor
            // The distance may have stopped at a limit: the target moves by
            // as much as the camera really came.
            let s = distance / before
            let k = 2 * before * tan(fov / 2) / Double(max(viewHeight, 1))
            let (right, up) = screenAxes
            target += (right * Double(offset.x) + up * Double(offset.y)) * k * (1 - s)
        }
        apply()
    }

    func pinch(_ magnification: CGFloat) {
        zoom(max(0.2, 1 - Double(magnification) * feel.pinchZoomRate))
    }

    // MARK: - Render loop

    /// Advances inertia, a glide and the turntable to `time`.
    func step(_ time: TimeInterval) {
        lock.lock()
        let dt = lastTime.map { min(time - $0, 0.1) } ?? 0
        lastTime = time
        if var g = glide {
            if g.start == nil { g.start = time; glide = g }
            let u = min(1, (time - g.start!) / max(g.seconds, 1e-3))
            let e = u * u * (3 - 2 * u)
            func mix(_ a: Double, _ b: Double) -> Double { a + (b - a) * e }
            pose = Pose(
                target: g.from.target + (g.to.target - g.from.target) * e,
                azimuth: mix(g.from.azimuth, g.to.azimuth),
                elevation: mix(g.from.elevation, g.to.elevation),
                distance: mix(g.from.distance, g.to.distance))
            if u >= 1 { glide = nil }
        } else if dt > 0 {
            if simd_length(velocity) > 0 {
                azimuth += velocity.x * dt
                pose.elevation = elevation + velocity.y * dt
                velocity *= exp(-dt / feel.inertiaTimeConstant)
                if simd_length(velocity) < feel.inertiaStopSpeed { velocity = .zero }
            }
            azimuth += feel.autoRotateDegreesPerSecond * dt
        }
        lock.unlock()
        apply()
    }

    private func apply() {
        let (position, t): (SIMD3<Double>, SIMD3<Double>) = lock.withLock {
            let a = azimuth * .pi / 180
            let e = elevation * .pi / 180
            let offset = SIMD3(sin(a) * cos(e), sin(e), cos(a) * cos(e)) * distance
            return (target + offset, target)
        }
        SCNTransaction.begin()
        SCNTransaction.disableActions = true
        node.simdPosition = SIMD3<Float>(position)
        node.simdLook(at: SIMD3<Float>(t), up: SIMD3(0, 1, 0), localFront: SIMD3(0, 0, -1))
        SCNTransaction.commit()
    }
}
