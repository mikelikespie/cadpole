import Foundation
import ViewerCore

/// The behaviour every orbiting 3D view keeps, from `viewer_conformance.json`,
/// and the checks that hold an implementation to it. The Swift views run
/// these against ViewerCore and against their own cameras; the web viewer
/// runs the same file (src/web/test/viewer-conformance.test.mjs).
///
/// Each check returns what failed, one line per case, so any test framework
/// can report it (`XCTAssertEqual(failures, [])`).
public struct ViewerConformance: Decodable, Sendable {
    public struct Limits: Decodable, Sendable { public let min: Double; public let max: Double }
    public struct CoastStep: Decodable, Sendable {
        public let elevation, velocity, dt, toElevation, toVelocity: Double
    }
    public struct CoastRun: Decodable, Sendable {
        public let elevation, velocity, dt, decaySeconds, seconds, endsAt, within, lastStepUnder: Double
    }
    public struct Fade: Decodable, Sendable {
        public let elevation, above, fill, lines, shadow, underLight: Double
    }
    public struct FadeFromHeight: Decodable, Sendable {
        public let height, distance, elevation, above: Double
    }
    public struct Wheel: Decodable, Sendable {
        public let input: String
        public let option, shift, inPage: Bool
        public let action: String
    }
    public struct Zoom: Decodable, Sendable {
        public let deltaY: Double
        public let precise: Bool
        public let perLine, factor: Double
    }
    public struct Scenario: Decodable, Sendable {
        public let name: String
        public let steps: [Step]
    }
    /// One step of a plate-switching scenario; exactly one action is set.
    public struct Step: Decodable, Sendable {
        public let select: Int?
        public let finish: Int?
        public let fail: Int?
        public let prefetch: [Int]?
        /// After `select`: "model" or "loading".
        public let show: String?
        /// After `select` or `prefetch`: the plate whose load starts (nil: none).
        public let starts: Int??
        /// After `select`: plates whose loads no longer matter.
        public let cancels: [Int]?
        /// After `finish`: "present", "keep" or "drop".
        public let outcome: String?
        public let expectSelected: Int?
        public let expectReady: [Int]?

        enum CodingKeys: String, CodingKey {
            case select, finish, fail, prefetch, show, starts, cancels, outcome, expectSelected, expectReady
        }

        public init(from decoder: any Decoder) throws {
            let c = try decoder.container(keyedBy: CodingKeys.self)
            select = try c.decodeIfPresent(Int.self, forKey: .select)
            finish = try c.decodeIfPresent(Int.self, forKey: .finish)
            fail = try c.decodeIfPresent(Int.self, forKey: .fail)
            prefetch = try c.decodeIfPresent([Int].self, forKey: .prefetch)
            show = try c.decodeIfPresent(String.self, forKey: .show)
            // "starts": null says no load starts; no "starts" says nothing about it.
            starts = c.contains(.starts) ? .some(try c.decodeNil(forKey: .starts) ? nil : c.decode(Int.self, forKey: .starts)) : nil
            cancels = try c.decodeIfPresent([Int].self, forKey: .cancels)
            outcome = try c.decodeIfPresent(String.self, forKey: .outcome)
            expectSelected = try c.decodeIfPresent(Int.self, forKey: .expectSelected)
            expectReady = try c.decodeIfPresent([Int].self, forKey: .expectReady)
        }
    }

    public let elevationLimits: Limits
    public let clamp: [[Double]]
    public let coastSteps: [CoastStep]
    public let coastRuns: [CoastRun]
    public let bedFade: [Fade]
    public let bedFadeFromHeight: [FadeFromHeight]
    public let wheel: [Wheel]
    public let zoomFactor: [Zoom]
    public let plateLoads: [Scenario]

    public static let fileName = "viewer_conformance.json"

    /// The fixture from the test's runfiles (or `VIEWER_CONFORMANCE`, a path).
    public static func load() throws -> ViewerConformance {
        try JSONDecoder().decode(ViewerConformance.self, from: Data(contentsOf: locate()))
    }

    struct NotFound: Error, CustomStringConvertible {
        var description: String { "\(ViewerConformance.fileName) is not in the test's runfiles (add it to data)" }
    }

    static func locate() throws -> URL {
        let env = ProcessInfo.processInfo.environment
        if let p = env["VIEWER_CONFORMANCE"] { return URL(fileURLWithPath: p) }
        if let manifest = env["RUNFILES_MANIFEST_FILE"], let text = try? String(contentsOfFile: manifest, encoding: .utf8) {
            for line in text.split(separator: "\n") {
                let parts = line.split(separator: " ", maxSplits: 1)
                if parts.count == 2, parts[0].hasSuffix("/" + fileName) { return URL(fileURLWithPath: String(parts[1])) }
            }
        }
        for root in [env["TEST_SRCDIR"], env["RUNFILES_DIR"]].compactMap({ $0 }) {
            let base = URL(fileURLWithPath: root)
            let fm = FileManager.default
            // Runfiles are symlinks: walk the tree's names, not its targets.
            guard let walk = fm.enumerator(atPath: root) else { continue }
            while let rel = walk.nextObject() as? String {
                if rel.hasSuffix("/" + fileName) || rel == fileName { return base.appendingPathComponent(rel) }
            }
        }
        throw NotFound()
    }

    // MARK: - Checks

    static func near(_ a: Double, _ b: Double, _ tolerance: Double) -> Bool { abs(a - b) <= tolerance }

    /// The elevation limits and the clamp.
    public func checkClamp(min: Double, max: Double, clamp f: (Double) -> Double) -> [String] {
        var out: [String] = []
        if min != elevationLimits.min || max != elevationLimits.max {
            out.append("limits \(min)...\(max), want \(elevationLimits.min)...\(elevationLimits.max)")
        }
        for c in clamp where c.count == 2 && !Self.near(f(c[0]), c[1], 1e-9) {
            out.append("clamp(\(c[0])) = \(f(c[0])), want \(c[1])")
        }
        return out
    }

    /// One coasting step: `(elevation, velocity, dt) -> (elevation, velocity)`.
    public func checkCoastSteps(_ f: (Double, Double, Double) -> (Double, Double)) -> [String] {
        coastSteps.compactMap { c in
            let (e, v) = f(c.elevation, c.velocity, c.dt)
            guard Self.near(e, c.toElevation, 1e-5), Self.near(v, c.toVelocity, 1e-5) else {
                return "coast(\(c.elevation), \(c.velocity), \(c.dt)) = (\(e), \(v)), want (\(c.toElevation), \(c.toVelocity))"
            }
            return nil
        }
    }

    /// A flick that coasts into a limit, run on a camera: it never passes the
    /// limit, comes to rest near it, and its last steps are small.
    /// `run(elevation, velocity, dt, seconds)` returns the elevation after
    /// each frame.
    public func checkCoastRuns(_ run: (Double, Double, Double, Double) -> [Double]) -> [String] {
        coastRuns.compactMap { c in
            let path = run(c.elevation, c.velocity, c.dt, c.seconds)
            guard let last = path.last else { return "run from \(c.elevation): no frames" }
            let beyond = path.contains { c.endsAt < 0 ? $0 < c.endsAt - 1e-9 : $0 > c.endsAt + 1e-9 }
            let steps = zip(path, path.dropFirst()).map { abs($1 - $0) }
            let tail = steps.suffix(3)
            let wrongWay = steps.indices.contains { i in
                let d = path[i + 1] - path[i]
                return c.velocity < 0 ? d > 1e-9 : d < -1e-9
            }
            if beyond { return "run from \(c.elevation) at \(c.velocity)/s passed \(c.endsAt): \(path.min()!)...\(path.max()!)" }
            if wrongWay { return "run from \(c.elevation) at \(c.velocity)/s turned back" }
            if !Self.near(last, c.endsAt, c.within) { return "run from \(c.elevation) at \(c.velocity)/s ended at \(last), want \(c.endsAt) +- \(c.within)" }
            if let big = tail.max(), big >= c.lastStepUnder { return "run from \(c.elevation): last steps \(Array(tail)), want under \(c.lastStepUnder)" }
            return nil
        }
    }

    /// The bed's fade over the camera's angle to it.
    public func checkBedFade(_ f: (Double) -> BedFade, fromHeight g: (Double, Double) -> BedFade) -> [String] {
        var out: [String] = []
        for c in bedFade {
            let b = f(c.elevation)
            let got = [b.above, b.fill, b.lines, b.shadow, b.underLight]
            let want = [c.above, c.fill, c.lines, c.shadow, c.underLight]
            if zip(got, want).contains(where: { !Self.near($0, $1, 1e-5) }) {
                out.append("fade at \(c.elevation): \(got), want \(want)")
            }
        }
        for c in bedFadeFromHeight where !Self.near(g(c.height, c.distance).above, c.above, 1e-5) {
            out.append("fade at height \(c.height), distance \(c.distance): \(g(c.height, c.distance).above), want \(c.above)")
        }
        return out
    }

    /// The wheel rule: `(input, option, shift, inPage) -> action`, by name.
    public func checkWheel(_ f: (String, Bool, Bool, Bool) -> String, zoomFactor z: (Double, Bool, Double) -> Double) -> [String] {
        var out = wheel.compactMap { c -> String? in
            let a = f(c.input, c.option, c.shift, c.inPage)
            return a == c.action ? nil : "\(c.input) option \(c.option) shift \(c.shift) in page \(c.inPage): \(a), want \(c.action)"
        }
        for c in zoomFactor where !Self.near(z(c.deltaY, c.precise, c.perLine), c.factor, 1e-5) {
            out.append("zoom factor for \(c.deltaY) (precise \(c.precise)): \(z(c.deltaY, c.precise, c.perLine)), want \(c.factor)")
        }
        return out
    }

    /// Plate switching, against anything that keeps `PlateLoads`' promises.
    public func checkPlateLoads(_ make: () -> any PlateLoadsUnderTest) -> [String] {
        var out: [String] = []
        for s in plateLoads {
            let t = make()
            for (i, step) in s.steps.enumerated() {
                let at = "\(s.name), step \(i + 1)"
                if let id = step.select {
                    let (show, started, cancelled) = t.select(id)
                    if let want = step.show, show != want { out.append("\(at): shows \(show), want \(want)") }
                    if let want = step.starts, started != want { out.append("\(at): starts \(started.map(String.init) ?? "nothing"), want \(want.map(String.init) ?? "nothing")") }
                    if let want = step.cancels, Set(cancelled) != Set(want) { out.append("\(at): cancels \(cancelled), want \(want)") }
                } else if let id = step.finish {
                    let o = t.finish(id)
                    if let want = step.outcome, o != want { out.append("\(at): finishing \(id) is \(o), want \(want)") }
                } else if let id = step.fail {
                    t.fail(id)
                } else if let candidates = step.prefetch {
                    let started = t.prefetch(candidates)
                    if let want = step.starts, started != want { out.append("\(at): prefetch starts \(started.map(String.init) ?? "nothing"), want \(want.map(String.init) ?? "nothing")") }
                }
                if let want = step.expectSelected, t.selected != want { out.append("\(at): selected \(t.selected.map(String.init) ?? "none"), want \(want)") }
                if let want = step.expectReady, t.ready != Set(want) { out.append("\(at): ready \(t.ready.sorted()), want \(want)") }
            }
        }
        return out
    }
}

/// A plate switcher under test. `select` reports what it shows ("model" or
/// "loading"), the plate whose load it started (nil: none) and the plates
/// whose loads it cancelled; `finish` finishes the latest load started for
/// a plate and reports "present", "keep" or "drop".
public protocol PlateLoadsUnderTest: AnyObject {
    func select(_ id: Int) -> (show: String, started: Int?, cancelled: [Int])
    func finish(_ id: Int) -> String
    func fail(_ id: Int)
    func prefetch(_ candidates: [Int]) -> Int?
    var selected: Int? { get }
    var ready: Set<Int> { get }
}

/// `PlateLoads` itself, as a `PlateLoadsUnderTest`.
public final class PlateLoadsAdapter: PlateLoadsUnderTest {
    var loads = PlateLoads<Int>()
    var tickets: [Int: PlateLoads<Int>.Ticket] = [:]

    public init() {}

    public func select(_ id: Int) -> (show: String, started: Int?, cancelled: [Int]) {
        let wanted = tickets.filter { loads.wants($0.value) }.map(\.key)
        let (show, start) = loads.select(id)
        if let start { tickets[id] = start }
        let cancelled = wanted.filter { tickets[$0].map { !loads.wants($0) } ?? false }
        switch show {
        case .model: return ("model", start?.plate, cancelled.sorted())
        case .loading: return ("loading", start?.plate, cancelled.sorted())
        }
    }

    public func finish(_ id: Int) -> String {
        guard let t = tickets[id] else { return "drop" }
        switch loads.finished(t) {
        case .present: return "present"
        case .keep: return "keep"
        case .drop: return "drop"
        }
    }

    public func fail(_ id: Int) {
        if let t = tickets[id] { loads.failed(t) }
    }

    public func prefetch(_ candidates: [Int]) -> Int? {
        guard let t = loads.prefetch(candidates) else { return nil }
        tickets[t.plate] = t
        return t.plate
    }

    public var selected: Int? { loads.selected }
    public var ready: Set<Int> { loads.ready }
}
