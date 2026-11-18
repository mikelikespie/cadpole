import AppKit
import CadpolePlates
import SceneKit
import simd

/// The SceneKit scene for one project: the printer's bed with a millimetre
/// grid, studio lights, a camera, and the current plate's model on the bed.
///
/// The project's coordinates are z-up millimetres with the bed's front-left
/// corner at the origin; everything hangs off `world`, which turns them to
/// SceneKit's y-up.
public final class PlateScene {
    public let scene = SCNScene()
    public let camera = SCNNode()
    public let world = SCNNode()
    let bed: Bed
    private let bedNode = SCNNode()
    private let gridMinor = SCNNode()
    private let gridMajor = SCNNode()
    private let outline = SCNNode()
    private let keyLight = SCNNode()
    private let underLight = SCNNode()
    public private(set) var model: SCNNode?
    /// How much of the bed shows: all of it from above, a faint sheet from
    /// under it (`updateFade`).
    public private(set) var fade = BedFade.shown
    private var dark = false
    public private(set) var modelBounds: (min: SIMD3<Float>, max: SIMD3<Float>)?

    /// The default view's direction: from the front, a little to the left,
    /// 28 degrees above the bed.
    static let azimuth: Float = -22 * .pi / 180
    static let elevation: Float = 28 * .pi / 180
    static let fieldOfView: CGFloat = 30

    public init(bed: Bed) {
        self.bed = bed
        world.eulerAngles.x = -.pi / 2
        scene.rootNode.addChildNode(world)

        let size = bed.maximum - bed.minimum
        let diagonal = Float(simd_length(size))
        let cam = SCNCamera()
        cam.fieldOfView = Self.fieldOfView
        cam.zNear = Double(max(0.5, diagonal / 2000))
        cam.zFar = Double(diagonal * 20)
        camera.camera = cam
        scene.rootNode.addChildNode(camera)

        world.addChildNode(bedNode)
        world.addChildNode(gridMinor)
        world.addChildNode(gridMajor)
        world.addChildNode(outline)
        buildBed()
        addLights(diagonal: diagonal)
        frame(aspect: 1.5, animated: false)
    }

    // MARK: - Bed

    private func buildBed() {
        let path = NSBezierPath()
        for (i, p) in bed.outline.enumerated() {
            let point = NSPoint(x: p.x, y: p.y)
            if i == 0 { path.move(to: point) } else { path.line(to: point) }
        }
        path.close()
        let shape = SCNShape(path: path, extrusionDepth: 0)
        let fill = SCNMaterial()
        fill.lightingModel = .lambert
        fill.isDoubleSided = true
        shape.materials = [fill]
        bedNode.geometry = shape
        bedNode.position.z = -0.4
        bedNode.castsShadow = false

        let lo = bed.minimum
        let hi = bed.maximum
        func lines(step: Double, skip: Double?) -> SCNGeometry {
            var v: [SIMD3<Float>] = []
            var x = (lo.x / step).rounded(.up) * step
            while x <= hi.x + 1e-6 {
                if skip.map({ abs(x.remainder(dividingBy: $0)) > 1e-6 }) ?? true {
                    v.append(SIMD3(Float(x), Float(lo.y), 0)); v.append(SIMD3(Float(x), Float(hi.y), 0))
                }
                x += step
            }
            var y = (lo.y / step).rounded(.up) * step
            while y <= hi.y + 1e-6 {
                if skip.map({ abs(y.remainder(dividingBy: $0)) > 1e-6 }) ?? true {
                    v.append(SIMD3(Float(lo.x), Float(y), 0)); v.append(SIMD3(Float(hi.x), Float(y), 0))
                }
                y += step
            }
            return lineGeometry(v)
        }
        gridMinor.geometry = lines(step: 10, skip: 50)
        gridMajor.geometry = lines(step: 50, skip: nil)
        var loop: [SIMD3<Float>] = []
        for (i, p) in bed.outline.enumerated() {
            let q = bed.outline[(i + 1) % bed.outline.count]
            loop.append(SIMD3(Float(p.x), Float(p.y), 0))
            loop.append(SIMD3(Float(q.x), Float(q.y), 0))
        }
        outline.geometry = lineGeometry(loop)
        for (node, z) in [(gridMinor, -0.12), (gridMajor, -0.1), (outline, -0.08)] {
            node.position.z = CGFloat(z)
            node.castsShadow = false
            let m = SCNMaterial()
            m.lightingModel = .constant
            m.writesToDepthBuffer = false
            m.blendMode = .alpha
            node.geometry?.materials = [m]
        }
        gridMinor.renderingOrder = 1
        gridMajor.renderingOrder = 2
        outline.renderingOrder = 3
        apply(dark: false)
    }

    private func lineGeometry(_ points: [SIMD3<Float>]) -> SCNGeometry {
        let data = points.withUnsafeBytes { Data($0) }
        let source = SCNGeometrySource(
            data: data, semantic: .vertex, vectorCount: points.count, usesFloatComponents: true,
            componentsPerVector: 3, bytesPerComponent: 4, dataOffset: 0, dataStride: MemoryLayout<SIMD3<Float>>.stride)
        let indices = (0 ..< UInt32(points.count)).map { $0 }
        let element = SCNGeometryElement(
            data: indices.withUnsafeBytes { Data($0) }, primitiveType: .line,
            primitiveCount: points.count / 2, bytesPerIndex: 4)
        return SCNGeometry(sources: [source], elements: [element])
    }

    /// Light and dark appearance: the bed's fill and the grid's lines stay
    /// low-contrast against the background so the model reads first.
    public func apply(dark: Bool) {
        self.dark = dark
        let fill = dark ? NSColor(white: 0.19, alpha: 1) : NSColor(white: 0.88, alpha: 1)
        let ink = dark ? NSColor.white : NSColor.black
        bedNode.geometry?.firstMaterial?.diffuse.contents = fill
        gridMinor.geometry?.firstMaterial?.diffuse.contents = ink.withAlphaComponent(dark ? 0.07 : 0.07)
        gridMajor.geometry?.firstMaterial?.diffuse.contents = ink.withAlphaComponent(dark ? 0.16 : 0.16)
        outline.geometry?.firstMaterial?.diffuse.contents = ink.withAlphaComponent(dark ? 0.35 : 0.30)
    }

    /// Whether the bed, its grid and outline are drawn (a picture of the
    /// parts alone, as a slicer saves for each plate, has none).
    public var showsBed = true {
        didSet { for n in [bedNode, gridMinor, gridMajor, outline] { n.isHidden = !showsBed } }
    }

    /// For tests: the bed's fill, its lines and the under light as drawn now.
    var drawnFade: (fill: CGFloat, lines: CGFloat, underLight: CGFloat, shadowAlpha: CGFloat) {
        let shadow = (keyLight.light?.shadowColor as? NSColor)?.alphaComponent ?? 0
        return (bedNode.opacity, gridMajor.opacity, underLight.isHidden ? 0 : underLight.light?.intensity ?? 0, shadow)
    }

    // MARK: - Under the bed

    /// The camera's angle above the bed's plane, seen from `target` (scene
    /// space): under the bed the bed fades, the shadow goes and the light
    /// from below comes up (`BedFade`). Call it each frame; cheap when
    /// nothing changes.
    public func updateFade(target: SIMD3<Float>) {
        let eye = camera.simdWorldPosition
        // The bed's top is the scene's y = 0 (its fill sits just under it).
        let next = BedFade(cameraHeight: Double(eye.y), distance: Double(simd_distance(eye, target)))
        if abs(next.above - fade.above) > 1e-4 || (next.above != fade.above && (next.above == 0 || next.above == 1)) {
            apply(fade: next)
        }
    }

    /// Draws the bed, its shadow and the under light at `fade`.
    public func apply(fade: BedFade) {
        self.fade = fade
        SCNTransaction.begin()
        SCNTransaction.disableActions = true
        bedNode.opacity = CGFloat(fade.fill)
        for n in [gridMinor, gridMajor, outline] { n.opacity = CGFloat(fade.lines) }
        // Faint, the fill must not hide the first layer behind it.
        bedNode.geometry?.firstMaterial?.writesToDepthBuffer = !fade.isFading
        keyLight.light?.shadowColor = NSColor(white: 0, alpha: 0.22 * fade.shadow)
        underLight.light?.intensity = 560 * fade.underLight
        underLight.isHidden = fade.underLight == 0
        SCNTransaction.commit()
    }

    // MARK: - Lights

    private func addLights(diagonal: Float) {
        let ambient = SCNLight()
        ambient.type = .ambient
        ambient.intensity = 420
        let ambientNode = SCNNode()
        ambientNode.light = ambient
        scene.rootNode.addChildNode(ambientNode)

        // Key light: high, front-left; it casts the soft ground shadow.
        let key = SCNLight()
        key.type = .directional
        key.intensity = 750
        key.castsShadow = true
        key.shadowMode = .forward
        key.shadowSampleCount = 8
        key.shadowRadius = 3
        key.shadowMapSize = CGSize(width: 2048, height: 2048)
        key.shadowColor = NSColor(white: 0, alpha: 0.22)
        key.automaticallyAdjustsShadowProjection = true
        key.maximumShadowDistance = CGFloat(diagonal * 3)
        keyLight.light = key
        keyLight.simdPosition = SIMD3(0, 0, 0)
        keyLight.simdLook(at: SIMD3(0.35, -1, -0.55), up: SIMD3(0, 1, 0), localFront: SIMD3(0, 0, -1))
        scene.rootNode.addChildNode(keyLight)

        // Fill from behind and to the right, no shadow.
        let fill = SCNLight()
        fill.type = .directional
        fill.intensity = 260
        let fillNode = SCNNode()
        fillNode.light = fill
        fillNode.simdLook(at: SIMD3(-0.6, -0.5, 0.6), up: SIMD3(0, 1, 0), localFront: SIMD3(0, 0, -1))
        scene.rootNode.addChildNode(fillNode)

        // From below, front-left, pointing up: lights the underside, and only
        // while the camera is under the bed (`apply(fade:)`).
        let under = SCNLight()
        under.type = .directional
        under.intensity = 0
        underLight.light = under
        underLight.isHidden = true
        underLight.simdLook(at: SIMD3(0.3, 1, -0.45), up: SIMD3(0, 0, 1), localFront: SIMD3(0, 0, -1))
        scene.rootNode.addChildNode(underLight)
    }

    // MARK: - Model

    /// The model's node for `geometry`: one SCNGeometry per filament colour,
    /// straight from the flat buffers. Safe off the main thread; nothing
    /// here touches the scene.
    public static func makeModel(_ geometry: PlateGeometry) -> SCNNode {
        let node = SCNNode()
        node.name = "plate \(geometry.plateId)"
        for batch in geometry.batches {
            let n = batch.vertexCount
            let positions = SCNGeometrySource(
                data: batch.positions, semantic: .vertex, vectorCount: n, usesFloatComponents: true,
                componentsPerVector: 3, bytesPerComponent: 4, dataOffset: 0, dataStride: 12)
            let normals = SCNGeometrySource(
                data: batch.normals, semantic: .normal, vectorCount: n, usesFloatComponents: true,
                componentsPerVector: 3, bytesPerComponent: 4, dataOffset: 0, dataStride: 12)
            var indices = Data(count: n * 4)
            indices.withUnsafeMutableBytes { raw in
                let p = raw.bindMemory(to: UInt32.self)
                for i in 0 ..< n { p[i] = UInt32(i) }
            }
            let element = SCNGeometryElement(data: indices, primitiveType: .triangles, primitiveCount: n / 3, bytesPerIndex: 4)
            let g = SCNGeometry(sources: [positions, normals], elements: [element])
            let m = SCNMaterial()
            m.lightingModel = .blinn
            let c = batch.colour
            m.diffuse.contents = NSColor(srgbRed: CGFloat(c.x), green: CGFloat(c.y), blue: CGFloat(c.z), alpha: 1)
            m.specular.contents = NSColor(white: 0.18, alpha: 1)
            m.shininess = 0.25
            m.isDoubleSided = false
            g.materials = [m]
            let child = SCNNode(geometry: g)
            child.name = "filament \(batch.filament)"
            node.addChildNode(child)
        }
        return node
    }

    /// Puts `node` (from `makeModel`) on the bed in place of the current
    /// model: the old one goes at once (two plates are never seen together),
    /// the new one fades in over `duration`.
    public func show(_ node: SCNNode?, bounds: (min: SIMD3<Float>, max: SIMD3<Float>)?, duration: TimeInterval) {
        model?.removeAllAnimations()
        model?.removeFromParentNode()
        model = node
        modelBounds = bounds
        guard let node else { return }
        node.opacity = duration > 0 ? 0 : 1
        world.addChildNode(node)
        guard duration > 0 else { return }
        // An implicit animation, not an action: it runs without the scene
        // playing, so the view renders only while something changes.
        SCNTransaction.begin()
        SCNTransaction.animationDuration = duration
        SCNTransaction.animationTimingFunction = CAMediaTimingFunction(name: .easeOut)
        node.opacity = 1
        SCNTransaction.commit()
    }

    // MARK: - Camera

    /// Where the camera looks: the model's centre, or the bed's when empty.
    public var target: SIMD3<Float> {
        let (lo, hi) = framedBounds
        return Self.scenePoint((lo + hi) / 2)
    }

    /// The radius of the sphere the framing fits.
    public var radius: Float {
        let (lo, hi) = framedBounds
        return max(simd_length(hi - lo) / 2, 5)
    }

    public var bedRadius: Float { Float(simd_length(bed.maximum - bed.minimum)) / 2 }

    private var framedBounds: (SIMD3<Float>, SIMD3<Float>) {
        if let b = modelBounds { return (b.min, b.max) }
        let lo = bed.minimum
        let hi = bed.maximum
        return (SIMD3(Float(lo.x), Float(lo.y), 0), SIMD3(Float(hi.x), Float(hi.y), 0))
    }

    /// Project z-up millimetres to SceneKit's y-up.
    static func scenePoint(_ p: SIMD3<Float>) -> SIMD3<Float> { SIMD3(p.x, p.z, -p.y) }

    /// Fits the model (or the empty bed) in a view of `aspect` (width /
    /// height), from the default direction.
    public func frame(aspect: CGFloat, animated: Bool, duration: TimeInterval = 0.25) {
        let (position, t) = framing(aspect: aspect)
        SCNTransaction.begin()
        SCNTransaction.animationDuration = animated ? duration : 0
        SCNTransaction.animationTimingFunction = CAMediaTimingFunction(name: .easeInEaseOut)
        camera.simdPosition = position
        camera.simdLook(at: t, up: SIMD3(0, 1, 0), localFront: SIMD3(0, 0, -1))
        SCNTransaction.commit()
    }

    /// The camera position that fits every corner of the framed box, from
    /// the default direction, with a small margin; and the target.
    func framing(aspect: CGFloat) -> (SIMD3<Float>, SIMD3<Float>) {
        let fov = Float(Self.fieldOfView) * .pi / 180
        let tanV = tan(fov / 2)
        let tanH = tanV * Float(max(aspect, 0.2))
        let back = simd_normalize(Self.scenePoint(SIMD3(
            sin(Self.azimuth) * cos(Self.elevation), -cos(Self.azimuth) * cos(Self.elevation), sin(Self.elevation))))
        let right = simd_normalize(simd_cross(SIMD3(0, 1, 0), back))
        let up = simd_cross(back, right)
        let t = target
        let (lo, hi) = framedBounds
        var distance: Float = 1
        for i in 0 ..< 8 {
            let corner = SIMD3(i & 1 == 0 ? lo.x : hi.x, i & 2 == 0 ? lo.y : hi.y, i & 4 == 0 ? lo.z : hi.z)
            let o = Self.scenePoint(corner) - t
            let depth = simd_dot(o, back)
            distance = max(distance, depth + abs(simd_dot(o, right)) / tanH, depth + abs(simd_dot(o, up)) / tanV)
        }
        return (t + back * distance * 1.1, t)
    }

    /// Keeps the camera between just outside the model and a few beds away.
    public var distanceLimits: ClosedRange<Float> {
        let near = radius * 0.6
        return near ... max(near * 2, bedRadius * 6)
    }
}
