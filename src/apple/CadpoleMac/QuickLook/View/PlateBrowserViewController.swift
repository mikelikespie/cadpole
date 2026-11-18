import AppKit
import CadpolePlates
import QuartzCore
import SceneKit
import os
import simd

let log = Logger(subsystem: "io.github.mikelikespie.cadpole.mac", category: "plate-preview")

/// A project's plates, one at a time, on the printer's bed: a sidebar of
/// plates (when there is more than one) and an orbitable 3D view.
///
/// `load(url:)` reads the project's settings on the calling thread (a few
/// ms) and returns; the bed shows at once. The plate's meshes are read,
/// smoothed and uploaded on a background queue and fade in when ready;
/// then the neighbouring plates are built, so moving to them is instant.
public final class PlateBrowserViewController: NSViewController {
    public private(set) var project: PlateProject?
    private var builder: PlateBuilder?
    private var plateScene: PlateScene?
    private let scnView = PlateSCNView(frame: .zero, options: nil)
    private let sidebar = NSVisualEffectView()
    private let table = NSTableView()
    private let spinner = NSProgressIndicator()
    private var order: [PlateProject.Plate] = []
    private var footprints: [Int: PlateGeometry.Footprint] = [:]
    private var cache: [Int: (node: SCNNode, geometry: PlateGeometry)] = [:]
    private var inFlight: Set<Int> = []
    private let queue = DispatchQueue(label: "io.github.mikelikespie.cadpole.plate-build", qos: .userInitiated)
    private var selected: Int?
    private(set) var orbit: OrbitCamera?
    /// Mouse and trackpad rates, inertia, limits.
    public var feel = CameraFeel() {
        didSet { orbit?.feel = feel }
    }

    /// Frame timing, kept by the render delegate.
    let frames = FrameClock()
    /// When `load` started, and what has happened since, for the log.
    private var started: ContinuousClock.Instant?
    private var pendingFirstFrame: ((Duration) -> Void)?
    public var onFirstModelFrame: ((Duration) -> Void)?

    public static let sidebarWidth: CGFloat = 196

    public override func loadView() {
        view = NSView(frame: NSRect(x: 0, y: 0, width: 960, height: 640))
        scnView.autoresizingMask = [.width, .height]
        scnView.antialiasingMode = .multisampling4X
        scnView.preferredFramesPerSecond = 120
        // The camera is ours (OrbitCamera), not SceneKit's, so its rates and
        // inertia can be tuned (CameraFeel).
        scnView.allowsCameraControl = false
        scnView.rendersContinuously = false
        scnView.delegate = frames
        scnView.owner = self
        view.addSubview(scnView)

        sidebar.material = .sidebar
        sidebar.blendingMode = .behindWindow
        sidebar.autoresizingMask = [.height]
        let scroll = NSScrollView()
        scroll.autoresizingMask = [.width, .height]
        scroll.hasVerticalScroller = true
        scroll.drawsBackground = false
        let column = NSTableColumn(identifier: .init("plate"))
        table.addTableColumn(column)
        table.headerView = nil
        table.rowHeight = 62
        table.style = .sourceList
        table.backgroundColor = .clear
        table.dataSource = self
        table.delegate = self
        scroll.documentView = table
        sidebar.addSubview(scroll)
        view.addSubview(sidebar)

        spinner.style = .spinning
        spinner.controlSize = .small
        spinner.isDisplayedWhenStopped = false
        view.addSubview(spinner)

        let doubleClick = NSClickGestureRecognizer(target: self, action: #selector(reframe))
        doubleClick.numberOfClicksRequired = 2
        scnView.addGestureRecognizer(doubleClick)

        // Fingers on a touch display (macOS 27): they reach a view only
        // through a gesture recognizer.
        let touches = TouchRecognizer(target: nil, action: nil)
        touches.onChange = { [weak self] before, after, time in self?.touchesChanged(before, after, time) }
        scnView.addGestureRecognizer(touches)
        layout()
    }

    public override func viewDidLayout() {
        super.viewDidLayout()
        layout()
    }

    private func layout() {
        let b = view.bounds
        let showSidebar = order.count > 1
        sidebar.isHidden = !showSidebar
        let w = showSidebar ? Self.sidebarWidth : 0
        sidebar.frame = NSRect(x: 0, y: 0, width: w, height: b.height)
        sidebar.subviews.first?.frame = sidebar.bounds
        scnView.frame = NSRect(x: w, y: 0, width: b.width - w, height: b.height)
        spinner.frame = NSRect(x: b.width - 28, y: b.height - 28, width: 16, height: 16)
    }

    // MARK: - Loading

    /// Reads the project's settings and shows its bed; meshes follow. Throws
    /// for anything that is not a readable 3MF.
    public func load(url: URL) throws {
        let clock = ContinuousClock()
        let t0 = clock.now
        started = t0
        let archive = try ZipReader(url: url)
        let project = try PlateProject(archive: archive)
        if !project.bed.fromProject {
            log.notice("no printable_area in the project; drawing a 256 mm bed")
        }
        self.project = project
        builder = PlateBuilder(archive: archive, project: project)
        order = project.previewOrder
        _ = view
        let scene = PlateScene(bed: project.bed)
        plateScene = scene
        let orbit = OrbitCamera(node: scene.camera)
        orbit.feel = feel
        self.orbit = orbit
        frames.orbit = orbit
        scnView.scene = scene.scene
        scnView.pointOfView = scene.camera
        appearanceChanged()
        layout()
        table.reloadData()
        log.notice("plate preview: settings read in \(String(describing: clock.now - t0), privacy: .public)")
        if let first = order.first {
            select(first.id, animated: false)
            table.selectRowIndexes([0], byExtendingSelection: false)
        }
    }

    /// Shows plate `id`: from the cache at once, else after it is built.
    public func select(_ id: Int, animated: Bool) {
        guard id != selected || plateScene?.model == nil else { return }
        selected = id
        if let cached = cache[id] {
            present(cached.node, cached.geometry, animated: animated)
        } else {
            plateScene?.show(nil, bounds: nil, duration: animated ? 0.25 : 0)
            frameCamera(animated: animated)
            spinner.startAnimation(nil)
            build(id)
        }
    }

    private func build(_ id: Int, then: (() -> Void)? = nil) {
        guard let builder, cache[id] == nil, !inFlight.contains(id) else { then?(); return }
        inFlight.insert(id)
        let view = scnView
        queue.async { [weak self] in
            let clock = ContinuousClock()
            let geometry = builder.build(plateId: id)
            let t0 = clock.now
            let node = PlateScene.makeModel(geometry)
            let made = clock.now - t0
            // Upload the buffers before the node is in the scene, so adding
            // it does not stall a frame.
            view.prepare([node]) { _ in
                let uploaded = clock.now - t0
                DispatchQueue.main.async {
                    guard let self else { return }
                    self.inFlight.remove(id)
                    self.cache[id] = (node, geometry)
                    self.footprints[id] = geometry.footprint
                    if let row = self.order.firstIndex(where: { $0.id == id }) {
                        self.table.reloadData(forRowIndexes: [row], columnIndexes: [0])
                    }
                    let t = geometry.timings
                    log.notice("""
                        plate \(id): \(geometry.triangleCount) triangles; inflate \(String(describing: t.inflate), privacy: .public), \
                        parse \(String(describing: t.parse), privacy: .public), normals \(String(describing: t.normals), privacy: .public), \
                        assemble \(String(describing: t.assemble), privacy: .public), nodes \(String(describing: made), privacy: .public), \
                        upload \(String(describing: uploaded), privacy: .public)
                        """)
                    if self.selected == id { self.present(node, geometry, animated: true) }
                    then?()
                    self.prefetchNeighbours()
                }
            }
        }
    }

    /// Builds the plates either side of the selected one, nearest first.
    private func prefetchNeighbours() {
        guard let selected, let i = order.firstIndex(where: { $0.id == selected }) else { return }
        for j in [i + 1, i - 1, i + 2] where order.indices.contains(j) && !order[j].isEmpty {
            if cache[order[j].id] == nil && !inFlight.contains(order[j].id) {
                build(order[j].id)
                return
            }
        }
    }

    private func present(_ node: SCNNode, _ geometry: PlateGeometry, animated: Bool) {
        spinner.stopAnimation(nil)
        guard let scene = plateScene else { return }
        scene.show(node, bounds: geometry.bounds, duration: animated ? 0.25 : 0)
        frameCamera(animated: animated)
        if let t0 = started {
            started = nil
            frames.onNextFrame = { [weak self] in
                let elapsed = ContinuousClock.now - t0
                log.notice("plate preview: first frame with the model \(String(describing: elapsed), privacy: .public) after load")
                DispatchQueue.main.async { self?.onFirstModelFrame?(elapsed) }
            }
        }
        animate(for: 0.4)
    }

    /// Renders continuously for `seconds`: the view otherwise draws only
    /// when the camera moves, and would not run a fade or a camera move.
    private var animatingUntil: ContinuousClock.Instant?
    private func animate(for seconds: Double) {
        let until = ContinuousClock.now + .milliseconds(Int(seconds * 1000))
        animatingUntil = max(animatingUntil ?? until, until)
        scnView.isPlaying = true
        DispatchQueue.main.asyncAfter(deadline: .now() + seconds) { [weak self] in
            guard let self, let end = self.animatingUntil, ContinuousClock.now >= end else { return }
            self.animatingUntil = nil
            if !self.holdPlaying { self.scnView.isPlaying = false }
        }
    }

    // MARK: - Mouse and trackpad

    private var lastDrag: TimeInterval?

    func orbitMouseDown() {
        orbit?.stop()
        lastDrag = nil
    }

    func orbitDragged(_ event: NSEvent) {
        let dt = lastDrag.map { event.timestamp - $0 } ?? 0
        lastDrag = event.timestamp
        orbit?.drag(CGSize(width: event.deltaX, height: -event.deltaY), dt: dt, viewWidth: scnView.bounds.width)
    }

    func orbitMouseUp(_ event: NSEvent) {
        // A pause before letting go means no flick.
        let stale = lastDrag.map { event.timestamp - $0 > 0.08 } ?? true
        orbit?.release(stale: stale)
        lastDrag = nil
        if !stale { animate(for: feel.inertiaTimeConstant * 5) }
    }

    /// The wheel and the trackpad, as `WheelRule` has them: on its own (a
    /// preview, a window) the wheel zooms toward the pointer, two fingers
    /// pan (shift: orbit), Option + either zooms; inside a scrolling page
    /// the page keeps the plain wheel and two fingers.
    func orbitScroll(_ event: NSEvent) {
        let page = scnView.enclosingScrollView
        guard let orbit, orbit.scrolled(event, in: scnView, inPage: page != nil) else {
            page?.scrollWheel(with: event)
            return
        }
    }

    func orbitMagnify(_ event: NSEvent) {
        orbit?.magnified(event, in: scnView)
    }

    // MARK: - Touch display

    /// The fingers on the view, as a gesture: one finger orbits (and flicks,
    /// like the mouse); two or more pan with their middle, pinch to zoom and
    /// twist to turn, all at once. Once a second finger has come down, the
    /// gesture stays a two-finger one until every finger lifts, so lifting
    /// one of two does not set the model spinning. A quick tap twice
    /// reframes, like a double-click.
    private struct TouchGesture {
        var multi = false
        var lastMove: TimeInterval?
        var tapStart: (time: TimeInterval, at: CGPoint)?
    }

    private var touchGesture = TouchGesture()
    private var lastTap: (time: TimeInterval, at: CGPoint)?

    private static let tapSeconds = 0.25
    private static let tapSlop: CGFloat = 12
    private static let doubleTapSeconds = 0.35

    private func touchesChanged(_ before: [Int: CGPoint], _ after: [Int: CGPoint], _ time: TimeInterval) {
        guard let orbit else { return }
        if before.count != after.count {
            log.debug("touch: \(before.count, privacy: .public) -> \(after.count, privacy: .public) fingers")
        }
        if before.isEmpty {
            orbit.stop()
            touchGesture = TouchGesture()
            if let p = after.values.first, after.count == 1 { touchGesture.tapStart = (time, p) }
        }
        if after.count > 1 {
            touchGesture.multi = true
            touchGesture.tapStart = nil
        }

        // Only the fingers down both before and after move anything; one
        // coming down or lifting changes nothing by itself.
        let ids = before.keys.filter { after[$0] != nil }.sorted()
        if !touchGesture.multi, ids.count == 1, let a = before[ids[0]], let b = after[ids[0]] {
            let delta = CGSize(width: b.x - a.x, height: b.y - a.y)
            if let tap = touchGesture.tapStart, hypot(b.x - tap.at.x, b.y - tap.at.y) > Self.tapSlop {
                touchGesture.tapStart = nil
            }
            if touchGesture.tapStart == nil {
                let dt = touchGesture.lastMove.map { time - $0 } ?? 0
                touchGesture.lastMove = time
                orbit.drag(delta, dt: dt, viewWidth: scnView.bounds.width)
            }
        } else if ids.count >= 2 {
            let old = ids.compactMap { before[$0] }
            let new = ids.compactMap { after[$0] }
            func middle(_ ps: [CGPoint]) -> CGPoint {
                let n = CGFloat(ps.count)
                return ps.reduce(.zero) { CGPoint(x: $0.x + $1.x / n, y: $0.y + $1.y / n) }
            }
            func spread(_ ps: [CGPoint], _ m: CGPoint) -> CGFloat {
                ps.reduce(0) { $0 + hypot($1.x - m.x, $1.y - m.y) } / CGFloat(ps.count)
            }
            let m0 = middle(old), m1 = middle(new)
            // The model follows the fingers' middle (pan takes AppKit's
            // scrolling delta, y down).
            orbit.pan(CGSize(width: m1.x - m0.x, height: -(m1.y - m0.y)), viewHeight: scnView.bounds.height)
            let s0 = spread(old, m0), s1 = spread(new, m1)
            if s0 > 1, s1 > 1 { orbit.zoom(Double(s0 / s1)) }
            // The first two fingers' line turning: clockwise on the screen
            // turns the model clockwise, the way the near side would go
            // under a drag.
            let a0 = atan2(old[1].y - old[0].y, old[1].x - old[0].x)
            let a1 = atan2(new[1].y - new[0].y, new[1].x - new[0].x)
            var d = a1 - a0
            if d > .pi { d -= 2 * .pi } else if d < -.pi { d += 2 * .pi }
            orbit.turn(degrees: -Double(d) * 180 / .pi)
        }

        if after.isEmpty {
            if let tap = touchGesture.tapStart, time - tap.time < Self.tapSeconds {
                if let last = lastTap, time - last.time < Self.doubleTapSeconds,
                   hypot(tap.at.x - last.at.x, tap.at.y - last.at.y) < Self.tapSlop * 3 {
                    lastTap = nil
                    reframe()
                } else {
                    lastTap = (time, tap.at)
                }
            } else if !touchGesture.multi {
                // A pause before lifting means no flick.
                let stale = touchGesture.lastMove.map { time - $0 > 0.08 } ?? true
                orbit.release(stale: stale)
                if !stale { animate(for: feel.inertiaTimeConstant * 5) }
            }
            touchGesture = TouchGesture()
        }
    }

    /// For the harness: keeps the view rendering every frame.
    public var holdPlaying = false {
        didSet { scnView.isPlaying = holdPlaying || animatingUntil != nil }
    }

    private func frameCamera(animated: Bool) {
        guard let scene = plateScene else { return }
        let size = scnView.bounds.size
        let (position, target) = scene.framing(aspect: size.height > 0 ? size.width / size.height : 1.5)
        orbit?.set(position: position, target: target, limits: scene.distanceLimits, animated: animated)
        animate(for: animated ? feel.transitionSeconds + 0.15 : 0.1)
    }

    @objc func reframe() { frameCamera(animated: true) }

    public func step(_ delta: Int) {
        guard let selected, let i = order.firstIndex(where: { $0.id == selected }) else { return }
        let j = i + delta
        guard order.indices.contains(j) else { return }
        table.selectRowIndexes([j], byExtendingSelection: false)
        table.scrollRowToVisible(j)
    }

    func appearanceChanged() {
        let dark = view.effectiveAppearance.bestMatch(from: [.darkAqua, .aqua]) == .darkAqua
        plateScene?.apply(dark: dark)
        scnView.backgroundColor = dark ? NSColor(white: 0.11, alpha: 1) : NSColor(white: 0.97, alpha: 1)
    }

    /// For the harness: the view the plate is drawn in.
    public var sceneView: SCNView { scnView }
    public var currentScene: PlateScene? { plateScene }
    public func isCached(_ id: Int) -> Bool { cache[id] != nil }
}

extension PlateBrowserViewController: NSTableViewDataSource, NSTableViewDelegate {
    public func numberOfRows(in tableView: NSTableView) -> Int { order.count }

    public func tableView(_ tableView: NSTableView, viewFor tableColumn: NSTableColumn?, row: Int) -> NSView? {
        let cell = (tableView.makeView(withIdentifier: PlateCell.id, owner: nil) as? PlateCell) ?? PlateCell()
        let plate = order[row]
        cell.configure(plate: plate, bed: project?.bed, footprint: footprints[plate.id]) { [weak self] in
            self?.builder?.colour($0) ?? SIMD4(0.7, 0.7, 0.7, 1)
        }
        return cell
    }

    public func tableViewSelectionDidChange(_ notification: Notification) {
        let row = table.selectedRow
        guard order.indices.contains(row) else { return }
        select(order[row].id, animated: true)
    }
}

/// The SCNView: arrow keys page plates, and the appearance follows the
/// system's.
final class PlateSCNView: SCNView {
    weak var owner: PlateBrowserViewController?

    override var acceptsFirstResponder: Bool { true }

    override func keyDown(with event: NSEvent) {
        switch event.specialKey {
        case .leftArrow?, .upArrow?, .pageUp?: owner?.step(-1)
        case .rightArrow?, .downArrow?, .pageDown?: owner?.step(1)
        default: super.keyDown(with: event)
        }
    }

    override func mouseDown(with event: NSEvent) {
        window?.makeFirstResponder(self)
        owner?.orbitMouseDown()
    }

    override func mouseDragged(with event: NSEvent) { owner?.orbitDragged(event) }
    override func mouseUp(with event: NSEvent) { owner?.orbitMouseUp(event) }
    override func scrollWheel(with event: NSEvent) { owner?.orbitScroll(event) }
    override func magnify(with event: NSEvent) { owner?.orbitMagnify(event) }

    override func viewDidChangeEffectiveAppearance() {
        super.viewDidChangeEffectiveAppearance()
        owner?.appearanceChanged()
    }
}

/// The render delegate: frame times, the first frame after a model goes in,
/// and the camera's per-frame step.
final class FrameClock: NSObject, SCNSceneRendererDelegate, @unchecked Sendable {
    private let lock = NSLock()
    private var last: TimeInterval?
    private var intervals: [Double] = []
    var recording = false
    var onNextFrame: (() -> Void)? {
        get { lock.withLock { _onNextFrame } }
        set { lock.withLock { _onNextFrame = newValue } }
    }
    private var _onNextFrame: (() -> Void)?
    /// The camera, stepped once per frame.
    weak var orbit: OrbitCamera?

    func renderer(_ renderer: any SCNSceneRenderer, updateAtTime time: TimeInterval) {
        orbit?.step(time)
    }

    func renderer(_ renderer: any SCNSceneRenderer, didRenderScene scene: SCNScene, atTime time: TimeInterval) {
        let callback: (() -> Void)?
        lock.lock()
        if recording, let last { intervals.append(time - last) }
        callback = _onNextFrame
        _onNextFrame = nil
        lock.unlock()
        last = time
        callback?()
    }

    func reset() {
        lock.withLock {
            intervals = []
            last = nil
        }
    }

    /// Frame intervals since `reset`, in ms.
    var samples: [Double] { lock.withLock { intervals.map { $0 * 1000 } } }
}

/// A sidebar row: a small top-down glyph of the plate, its title, and how
/// many objects it holds.
final class PlateCell: NSTableCellView {
    static let id = NSUserInterfaceItemIdentifier("PlateCell")
    private let glyph = PlateGlyph()
    private let title = NSTextField(labelWithString: "")
    private let detail = NSTextField(labelWithString: "")

    init() {
        super.init(frame: NSRect(x: 0, y: 0, width: 180, height: 62))
        identifier = Self.id
        title.font = .systemFont(ofSize: 12, weight: .semibold)
        title.lineBreakMode = .byTruncatingTail
        detail.font = .systemFont(ofSize: 11)
        detail.textColor = .secondaryLabelColor
        detail.lineBreakMode = .byTruncatingTail
        for v in [glyph, title, detail] as [NSView] {
            v.translatesAutoresizingMaskIntoConstraints = false
            addSubview(v)
        }
        NSLayoutConstraint.activate([
            glyph.leadingAnchor.constraint(equalTo: leadingAnchor, constant: 4),
            glyph.centerYAnchor.constraint(equalTo: centerYAnchor),
            glyph.widthAnchor.constraint(equalToConstant: 50),
            glyph.heightAnchor.constraint(equalToConstant: 50),
            title.leadingAnchor.constraint(equalTo: glyph.trailingAnchor, constant: 8),
            title.trailingAnchor.constraint(equalTo: trailingAnchor, constant: -4),
            title.bottomAnchor.constraint(equalTo: centerYAnchor, constant: 1),
            detail.leadingAnchor.constraint(equalTo: title.leadingAnchor),
            detail.trailingAnchor.constraint(equalTo: title.trailingAnchor),
            detail.topAnchor.constraint(equalTo: centerYAnchor, constant: 2),
        ])
    }

    required init?(coder: NSCoder) { nil }

    func configure(
        plate: PlateProject.Plate, bed: Bed?, footprint: PlateGeometry.Footprint?, colour: @escaping (Int) -> SIMD4<Float>
    ) {
        title.stringValue = plate.title
        let n = plate.objectCount
        detail.stringValue = plate.isEmpty ? "Empty" : n == 1 ? "1 object" : "\(n) objects"
        glyph.bed = bed
        glyph.footprint = footprint
        glyph.colour = colour
        glyph.needsDisplay = true
    }
}

/// The plate seen from above: the bed's outline and, once the plate is
/// built, its footprint in filament colours. Vector drawing, no images.
final class PlateGlyph: NSView {
    var bed: Bed?
    var footprint: PlateGeometry.Footprint?
    var colour: ((Int) -> SIMD4<Float>)?

    override func draw(_ dirtyRect: NSRect) {
        guard let bed else { return }
        let lo = bed.minimum
        let size = bed.maximum - lo
        let scale = min((bounds.width - 4) / size.x, (bounds.height - 4) / size.y)
        let w = size.x * scale
        let h = size.y * scale
        let ox = (bounds.width - w) / 2
        let oy = (bounds.height - h) / 2
        let path = NSBezierPath()
        for (i, p) in bed.outline.enumerated() {
            let q = NSPoint(x: ox + (p.x - lo.x) * scale, y: oy + (p.y - lo.y) * scale)
            if i == 0 { path.move(to: q) } else { path.line(to: q) }
        }
        path.close()
        NSColor.quaternaryLabelColor.setFill()
        path.fill()
        NSColor.tertiaryLabelColor.setStroke()
        path.lineWidth = 0.75
        path.stroke()
        guard let f = footprint, let colour else { return }
        let cw = w / CGFloat(f.width)
        let ch = h / CGFloat(f.height)
        for y in 0 ..< f.height {
            for x in 0 ..< f.width {
                let v = f.cells[y * f.width + x]
                guard v > 0 else { continue }
                let c = colour(Int(v))
                NSColor(srgbRed: CGFloat(c.x), green: CGFloat(c.y), blue: CGFloat(c.z), alpha: 1).setFill()
                NSRect(x: ox + CGFloat(x) * cw, y: oy + CGFloat(y) * ch, width: cw + 0.3, height: ch + 0.3).fill()
            }
        }
    }
}
