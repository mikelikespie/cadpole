// A Codable "any JSON" value.
//
// The session API is the web UI's JSON view (docs/design.md), and two of
// its fields are schema-free from Swift's point of view: `param_overrides`
// (number | bool | string | [x,y,z], mirroring `cadpole_engine::view`) and an
// event's `data`. Foundation's JSONSerialization would do, but it hands back
// NSNumber, whose bridging differs between Darwin and swift-corelibs; a Codable
// enum behaves identically on both and is what the Linux test exercises.

import Foundation
import CadpoleProto

public enum JSONValue: Codable, Sendable, Equatable {
    case null
    case bool(Bool)
    case number(Double)
    case string(String)
    case array([JSONValue])
    case object([String: JSONValue])

    public init(from decoder: any Decoder) throws {
        let c = try decoder.singleValueContainer()
        if c.decodeNil() {
            self = .null
        } else if let v = try? c.decode(Bool.self) {
            self = .bool(v)
        } else if let v = try? c.decode(Double.self) {
            self = .number(v)
        } else if let v = try? c.decode(String.self) {
            self = .string(v)
        } else if let v = try? c.decode([JSONValue].self) {
            self = .array(v)
        } else if let v = try? c.decode([String: JSONValue].self) {
            self = .object(v)
        } else {
            throw DecodingError.dataCorruptedError(in: c, debugDescription: "not JSON")
        }
    }

    public func encode(to encoder: any Encoder) throws {
        var c = encoder.singleValueContainer()
        switch self {
        case .null: try c.encodeNil()
        case .bool(let v): try c.encode(v)
        case .number(let v): try c.encode(v)
        case .string(let v): try c.encode(v)
        case .array(let v): try c.encode(v)
        case .object(let v): try c.encode(v)
        }
    }

    public var doubleValue: Double? {
        if case .number(let v) = self { return v }
        return nil
    }

    /// The number as a count (a `seq`, a `revision`), or nil when it is not
    /// one exactly: negative, fractional, 2^64 or more, or not finite.
    /// `UInt64(exactly:)` rather than `UInt64(_:)`, which traps on a value out
    /// of range -- and this reads JSON a server sent.
    public var uint64Value: UInt64? {
        guard case .number(let v) = self else { return nil }
        return UInt64(exactly: v)
    }

    public var stringValue: String? {
        if case .string(let v) = self { return v }
        return nil
    }
}

// MARK: - ParamValue bridging

extension JSONValue {
    /// The inverse of `cadpole_engine::view::param_value_json`. A three-element
    /// array is a vector; anything else of the wrong shape is not a parameter
    /// value and is dropped, exactly as the Rust side drops it.
    public var paramValue: Cadpole_V1_ParamValue? {
        var out = Cadpole_V1_ParamValue()
        switch self {
        case .bool(let b):
            out.boolean = b
        case .number(let n):
            out.number = n
        case .string(let s):
            out.text = s
        case .array(let a) where a.count == 3:
            let f = a.compactMap(\.doubleValue)
            guard f.count == 3 else { return nil }
            var v = Cadpole_V1_Vec3()
            v.x = f[0]
            v.y = f[1]
            v.z = f[2]
            out.vector = v
        default:
            return nil
        }
        return out
    }

    /// The forward direction: what `PUT /api/session/params` wants.
    public init?(paramValue: Cadpole_V1_ParamValue) {
        switch paramValue.value {
        case .number(let n): self = .number(n)
        case .boolean(let b): self = .bool(b)
        case .text(let s): self = .string(s)
        case .vector(let v): self = .array([.number(v.x), .number(v.y), .number(v.z)])
        case .none: return nil
        }
    }
}

extension Cadpole_V1_ParamValue {
    /// How the value reads in the document's display unit, for a slider's
    /// `accessibilityValue` and for the text field beside it.
    public var displayString: String {
        switch value {
        case .number(let n):
            return n == n.rounded() && abs(n) < 1e9
                ? String(Int(n)) : String(format: "%g", n)
        case .boolean(let b): return b ? "on" : "off"
        case .text(let s): return s
        case .vector(let v): return String(format: "%g, %g, %g", v.x, v.y, v.z)
        case .none: return ""
        }
    }

    public static func number(_ n: Double) -> Cadpole_V1_ParamValue {
        var v = Cadpole_V1_ParamValue()
        v.number = n
        return v
    }

    public static func text(_ s: String) -> Cadpole_V1_ParamValue {
        var v = Cadpole_V1_ParamValue()
        v.text = s
        return v
    }

    public static func boolean(_ b: Bool) -> Cadpole_V1_ParamValue {
        var v = Cadpole_V1_ParamValue()
        v.boolean = b
        return v
    }
}
