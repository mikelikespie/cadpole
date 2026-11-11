// Millimetres in, the document's unit out.
//
// Geometry is always millimetres -- docs/starlark-api.md: "Units are
// fixed: millimetres for length, degrees for angles... The document's
// `display_unit` only affects presentation." So every length that reaches a
// person has to be divided by the unit's scale on the way, and a client that
// prints the raw number with the unit's name beside it is off by that scale:
// a 1 inch cube in an inch document read "25.40 x 25.40 x 25.40 in".
//
// The scales and the formatting are the web client's (`UNIT_SCALE`, `fmt` and
// `fmtLen` in src/web/src/util.ts), so the browser and the app print the same
// string for the same body -- and, through `spoken`, say the same sentence
// (docs/design.md: one vocabulary for the three audiences).

import Foundation
import CadpoleProto

/// A document's `display_unit`.
public enum DisplayUnit: String, Sendable, CaseIterable {
    case mm
    case cm
    case m
    case `in`
    case ft

    /// Anything unknown is millimetres, which is what the rest of the client
    /// falls back to and what the engine actually produced.
    public init(_ name: String) {
        self = DisplayUnit(rawValue: name) ?? .mm
    }

    public init(_ unit: Cadpole_V1_LengthUnit) {
        switch unit {
        case .cm: self = .cm
        case .m: self = .m
        case .in: self = .in
        case .ft: self = .ft
        default: self = .mm
        }
    }

    public var lengthUnit: Cadpole_V1_LengthUnit {
        switch self {
        case .mm: return .mm
        case .cm: return .cm
        case .m: return .m
        case .in: return .in
        case .ft: return .ft
        }
    }

    /// Millimetres per unit. `UNIT_SCALE` in src/web/src/util.ts.
    public var scale: Double {
        switch self {
        case .mm: return 1
        case .cm: return 10
        case .m: return 1000
        case .in: return 25.4
        case .ft: return 304.8
        }
    }

    /// An internal millimetre value, in this unit.
    public func fromMillimetres(_ mm: Double) -> Double {
        mm / scale
    }

    /// How this unit reads aloud.
    public var spoken: String {
        UnitNames.spoken(rawValue)
    }

    // MARK: - What a person sees

    /// One length: "25.4 mm", "1 in".
    public func length(_ mm: Double) -> String {
        "\(UnitNames.trimmed(fromMillimetres(mm))) \(rawValue)"
    }

    /// A bounding box, for the inspector: "25.4 × 25.4 × 25.4 mm".
    public func size(_ size: Cadpole_V1_Vec3) -> String {
        let x = UnitNames.trimmed(fromMillimetres(size.x))
        let y = UnitNames.trimmed(fromMillimetres(size.y))
        let z = UnitNames.trimmed(fromMillimetres(size.z))
        return "\(x) \u{00D7} \(y) \u{00D7} \(z) \(rawValue)"
    }

    /// The same box, for VoiceOver: "25.4 by 25.4 by 25.4 millimetres".
    public func spokenSize(_ size: Cadpole_V1_Vec3) -> String {
        let x = UnitNames.trimmed(fromMillimetres(size.x))
        let y = UnitNames.trimmed(fromMillimetres(size.y))
        let z = UnitNames.trimmed(fromMillimetres(size.z))
        return "\(x) by \(y) by \(z) \(spoken)"
    }
}

public enum UnitNames {
    /// How a unit reads aloud, for a slider's `accessibilityValue` and for a
    /// tree row. A parameter may declare any unit it likes, so an unknown
    /// one reads as itself rather than as nothing.
    public static func spoken(_ unit: String) -> String {
        switch unit {
        case "mm": return "millimetres"
        case "cm": return "centimetres"
        case "m": return "metres"
        case "in": return "inches"
        case "ft": return "feet"
        case "deg": return "degrees"
        default: return unit
        }
    }

    /// `fmt` from src/web/src/util.ts, so both clients print the same digits:
    /// up to three decimals with the trailing zeros taken off, exponential at
    /// the extremes, and a plain "0" for something that rounds to nothing.
    public static func trimmed(_ n: Double, digits: Int = 3) -> String {
        guard n.isFinite else { return "\u{2013}" }
        let magnitude = abs(n)
        if magnitude < 1e-9 { return "0" }
        if magnitude >= 1e6 || magnitude < 1e-4 { return exponential(n) }
        var s = String(format: "%.\(digits)f", n)
        while s.hasSuffix("0") { s.removeLast() }
        if s.hasSuffix(".") { s.removeLast() }
        return s
    }

    /// JavaScript's `toExponential(2)`, which does not pad the exponent.
    private static func exponential(_ n: Double) -> String {
        let s = String(format: "%.2e", n)
        guard let e = s.firstIndex(of: "e") else { return s }
        let mantissa = String(s[s.startIndex ..< e])
        var rest = String(s[s.index(after: e)...])
        var sign = "+"
        if rest.hasPrefix("-") || rest.hasPrefix("+") {
            sign = String(rest.removeFirst())
        }
        while rest.count > 1, rest.hasPrefix("0") { rest.removeFirst() }
        return "\(mantissa)e\(sign)\(rest)"
    }
}
