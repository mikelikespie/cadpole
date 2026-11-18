import Foundation

/// How much of a print bed shows as the camera goes under it. From above the
/// bed is drawn as usual; from below it would hide the part's first layer,
/// so its fill, grid, outline and labels fade to a faint sheet, the shadow
/// it receives goes, and a light from underneath comes up so the part's
/// underside reads. The change runs smoothly across the horizon (no pop at
/// the plane) and is complete a few degrees under it.
///
/// Pure: every view computes the same numbers from the same angle
/// (`conformance.json` holds the reference values).
public struct BedFade: Sendable, Equatable {
    /// 1 with the camera above the bed, 0 once it is well under it.
    public let above: Double

    /// The camera's angle above the bed (degrees) where the fade starts, and
    /// where it is complete.
    public static let startDegrees = 2.0
    public static let endDegrees = -10.0
    /// What is left of each part under the bed.
    public static let fillUnder = 0.12
    public static let linesUnder = 0.2

    public init(above: Double) {
        self.above = min(max(above, 0), 1)
    }

    /// The fade for a camera `degrees` above the bed's plane (negative: under it).
    public init(elevationDegrees degrees: Double) {
        let t = (degrees - Self.endDegrees) / (Self.startDegrees - Self.endDegrees)
        let u = min(max(t, 0), 1)
        self.init(above: u * u * (3 - 2 * u))
    }

    /// The fade for a camera `height` above the bed's top (in any unit, the
    /// same as `distance`) looking at a target `distance` away: the angle at
    /// which its line of sight meets the bed's plane.
    public init(cameraHeight height: Double, distance: Double) {
        let s = distance > 0 ? min(max(height / distance, -1), 1) : (height < 0 ? -1 : 1)
        self.init(elevationDegrees: asin(s) * 180 / .pi)
    }

    /// Fully drawn: a view with no camera under the bed.
    public static let shown = BedFade(above: 1)

    /// The bed's fill and texture (its slab, its sides): opacity, 1 to 0.12.
    public var fill: Double { Self.fillUnder + (1 - Self.fillUnder) * above }
    /// Its grid, outline and labels: a multiplier on their own alpha, 1 to 0.2.
    public var lines: Double { Self.linesUnder + (1 - Self.linesUnder) * above }
    /// The shadow the part casts on it: a multiplier on its alpha, 1 to 0.
    public var shadow: Double { above }
    /// A light from below, for the part's underside: 0 above, 1 under.
    public var underLight: Double { 1 - above }
    /// Whether the camera is under the bed enough for any of it to show.
    public var isFading: Bool { above < 1 }
}
