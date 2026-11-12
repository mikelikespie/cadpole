import Foundation
import simd

/// One 3MF model part (`3D/3dmodel.model`, or a production-extension part
/// such as `3D/Objects/object_7.model`), read by a byte-level scanner.
///
/// Mesh parts run to tens of MB of `<vertex x= y= z=/>` and
/// `<triangle v1= v2= v3=/>`; the scanner reads only those tags' attributes
/// and the few structural tags (object, component, item), straight into flat
/// arrays, with no per-vertex allocation and no DOM.
public struct ModelFile: Sendable {
    public struct Object: Sendable {
        public var id: Int
        public var name: String?
        public var mesh: Mesh?
        public var components: [Component] = []
    }

    public struct Component: Sendable {
        /// The part holding the referenced object (`p:path`, without the
        /// leading slash); nil for this same part.
        public var path: String?
        public var objectId: Int
        public var transform: simd_double4x4
    }

    public struct Item: Sendable {
        public var objectId: Int
        public var transform: simd_double4x4
    }

    /// An indexed triangle mesh, flat: `positions` is x, y, z per vertex,
    /// `indices` three per triangle, `paint` one state per triangle (0 when
    /// unpainted; else the 1-based extruder Bambu Studio's painting set it to).
    public struct Mesh: Sendable {
        public var positions: [Float] = []
        public var indices: [UInt32] = []
        public var paint: [UInt8] = []

        public init() {}

        public var vertexCount: Int { positions.count / 3 }
        public var triangleCount: Int { indices.count / 3 }
    }

    public var objects: [Int: Object] = [:]
    public var items: [Item] = []
    /// `<basematerials>` display colours, in order: what a file without
    /// slicer settings (PrusaSlicer's format) has for filament colours.
    public var baseColours: [String] = []

    public init(parsing data: Data) {
        data.withUnsafeBytes { raw in
            guard let base = raw.baseAddress?.assumingMemoryBound(to: UInt8.self) else { return }
            var scanner = Scanner(p: base, n: raw.count)
            scanner.run(into: &self)
        }
    }
}

// MARK: - Scanner

private let LT = UInt8(ascii: "<")
private let GT = UInt8(ascii: ">")
private let SLASH = UInt8(ascii: "/")
private let QUOTE = UInt8(ascii: "\"")
private let APOS = UInt8(ascii: "'")
private let EQ = UInt8(ascii: "=")

private struct Scanner {
    let p: UnsafePointer<UInt8>
    let n: Int
    var i = 0

    init(p: UnsafePointer<UInt8>, n: Int) {
        self.p = p
        self.n = n
    }

    mutating func run(into model: inout ModelFile) {
        var current: ModelFile.Object?
        var mesh = ModelFile.Mesh()
        var inMesh = false
        while i < n {
            guard let lt = memchr(p + i, Int32(LT), n - i) else { break }
            i = UnsafePointer<UInt8>(lt.assumingMemoryBound(to: UInt8.self)) - p + 1
            guard i < n else { break }
            let first = p[i]
            if first == SLASH {
                i += 1
                if matches("object") {
                    if var o = current {
                        if inMesh || !mesh.indices.isEmpty { o.mesh = mesh }
                        model.objects[o.id] = o
                    }
                    current = nil
                    mesh = ModelFile.Mesh()
                    inMesh = false
                }
                continue
            }
            // Tags are matched on their name followed by whitespace, `/` or `>`.
            if matches("vertex") {
                let (x, y, z) = vertexAttributes()
                mesh.positions.append(x)
                mesh.positions.append(y)
                mesh.positions.append(z)
            } else if matches("triangle") {
                let (a, b, c, paint) = triangleAttributes()
                mesh.indices.append(a)
                mesh.indices.append(b)
                mesh.indices.append(c)
                mesh.paint.append(paint)
            } else if matches("vertices") || matches("triangles") {
                continue
            } else if matches("mesh") {
                inMesh = true
                // Reserve from the part's size: a vertex line is ~60 bytes.
                mesh.positions.reserveCapacity(min(n / 20, 1 << 26))
                mesh.indices.reserveCapacity(min(n / 12, 1 << 26))
            } else if matches("object") {
                let a = attributes()
                current = ModelFile.Object(id: a["id"].flatMap { Int($0) } ?? -1, name: a["name"])
            } else if matches("component") {
                let a = attributes()
                guard let id = a["objectid"].flatMap({ Int($0) }) else { continue }
                var path = a["p:path"] ?? a["path"]
                if let s = path, s.hasPrefix("/") { path = String(s.dropFirst()) }
                current?.components.append(.init(path: path, objectId: id, transform: Self.transform(a["transform"])))
            } else if matches("base") {
                if let colour = attributes()["displaycolor"] { model.baseColours.append(colour) }
            } else if matches("item") {
                let a = attributes()
                guard let id = a["objectid"].flatMap({ Int($0) }) else { continue }
                if a["printable"] == "0" { continue }
                model.items.append(.init(objectId: id, transform: Self.transform(a["transform"])))
            }
        }
    }

    /// True, consuming the name, when the tag at `i` is `name`.
    mutating func matches(_ name: StaticString) -> Bool {
        let len = name.utf8CodeUnitCount
        guard i + len < n else { return false }
        let s = name.utf8Start
        for k in 0 ..< len where p[i + k] != s[k] { return false }
        let after = p[i + len]
        guard after == 0x20 || after == 0x09 || after == 0x0A || after == 0x0D || after == SLASH || after == GT else {
            return false
        }
        i += len
        return true
    }

    /// `x="1.5" y="-2" z="3e-1"`, in any order; anything else is skipped.
    mutating func vertexAttributes() -> (Float, Float, Float) {
        var x: Float = 0, y: Float = 0, z: Float = 0
        while i < n, p[i] != GT {
            let c = p[i]
            if (c == UInt8(ascii: "x") || c == UInt8(ascii: "y") || c == UInt8(ascii: "z")), i + 2 < n,
                p[i + 1] == EQ, p[i + 2] == QUOTE || p[i + 2] == APOS, i == 0 || isSpace(p[i - 1])
            {
                i += 3
                let v = Float(number())
                if c == UInt8(ascii: "x") { x = v } else if c == UInt8(ascii: "y") { y = v } else { z = v }
            } else {
                i += 1
            }
        }
        return (x, y, z)
    }

    /// `v1= v2= v3=`, and Bambu Studio's `paint_color` (or PrusaSlicer's
    /// `slic3rpe:mmu_segmentation`, the same encoding).
    mutating func triangleAttributes() -> (UInt32, UInt32, UInt32, UInt8) {
        var v: (UInt32, UInt32, UInt32) = (0, 0, 0)
        var paint: UInt8 = 0
        while i < n, p[i] != GT {
            guard isSpace(p[i - 1]) else { i += 1; continue }
            if p[i] == UInt8(ascii: "v"), i + 3 < n, p[i + 2] == EQ {
                let which = p[i + 1]
                i += 4
                let value = UInt32(clamping: integer())
                switch which {
                case UInt8(ascii: "1"): v.0 = value
                case UInt8(ascii: "2"): v.1 = value
                case UInt8(ascii: "3"): v.2 = value
                default: break
                }
            } else if let start = valueStart(after: "paint_color") ?? valueStart(after: "slic3rpe:mmu_segmentation") {
                i = start
                var end = i
                while end < n, p[end] != QUOTE, p[end] != APOS { end += 1 }
                paint = Self.paintState(p + i, end - i)
                i = end
            } else {
                i += 1
            }
        }
        return (v.0, v.1, v.2, paint)
    }

    /// When the attribute at `i` is `name="`, where its value starts.
    func valueStart(after name: StaticString) -> Int? {
        let len = name.utf8CodeUnitCount
        guard i + len + 2 < n else { return nil }
        let s = name.utf8Start
        for k in 0 ..< len where p[i + k] != s[k] { return nil }
        guard p[i + len] == EQ, p[i + len + 1] == QUOTE || p[i + len + 1] == APOS else { return nil }
        return i + len + 2
    }

    /// The tag's attributes, as strings: for the few structural tags only.
    mutating func attributes() -> [String: String] {
        var out: [String: String] = [:]
        while i < n, p[i] != GT {
            if isSpace(p[i]) || p[i] == SLASH { i += 1; continue }
            let nameStart = i
            while i < n, p[i] != EQ, p[i] != GT, !isSpace(p[i]) { i += 1 }
            let nameEnd = i
            while i < n, isSpace(p[i]) { i += 1 }
            guard i < n, p[i] == EQ else { continue }
            i += 1
            while i < n, isSpace(p[i]) { i += 1 }
            guard i < n, p[i] == QUOTE || p[i] == APOS else { continue }
            let q = p[i]
            i += 1
            let valueStart = i
            while i < n, p[i] != q { i += 1 }
            let name = String(decoding: UnsafeBufferPointer(start: p + nameStart, count: nameEnd - nameStart), as: UTF8.self)
            out[name] = Self.unescape(String(decoding: UnsafeBufferPointer(start: p + valueStart, count: i - valueStart), as: UTF8.self))
            i += 1
        }
        return out
    }

    func isSpace(_ c: UInt8) -> Bool { c == 0x20 || c == 0x09 || c == 0x0A || c == 0x0D }

    /// A decimal number with optional sign, fraction and exponent, up to the
    /// closing quote.
    mutating func number() -> Double {
        var neg = false
        if i < n, p[i] == UInt8(ascii: "-") { neg = true; i += 1 } else if i < n, p[i] == UInt8(ascii: "+") { i += 1 }
        var mantissa: UInt64 = 0
        var exp = 0
        var digits = 0
        while i < n, p[i] &- 48 < 10 {
            if digits < 18 { mantissa = mantissa * 10 + UInt64(p[i] &- 48); digits += 1 } else { exp += 1 }
            i += 1
        }
        if i < n, p[i] == UInt8(ascii: ".") {
            i += 1
            while i < n, p[i] &- 48 < 10 {
                if digits < 18 { mantissa = mantissa * 10 + UInt64(p[i] &- 48); digits += 1; exp -= 1 }
                i += 1
            }
        }
        if i < n, p[i] == UInt8(ascii: "e") || p[i] == UInt8(ascii: "E") {
            i += 1
            var eneg = false
            if i < n, p[i] == UInt8(ascii: "-") { eneg = true; i += 1 } else if i < n, p[i] == UInt8(ascii: "+") { i += 1 }
            var e = 0
            while i < n, p[i] &- 48 < 10 { e = min(e * 10 + Int(p[i] &- 48), 400); i += 1 }
            exp += eneg ? -e : e
        }
        var value = Double(mantissa)
        if exp != 0 { value *= Self.pow10(exp) }
        return neg ? -value : value
    }

    static func pow10(_ e: Int) -> Double {
        let table: [Double] = [1, 1e1, 1e2, 1e3, 1e4, 1e5, 1e6, 1e7, 1e8, 1e9, 1e10, 1e11, 1e12, 1e13, 1e14, 1e15, 1e16, 1e17, 1e18, 1e19, 1e20, 1e21, 1e22]
        if e >= 0, e < table.count { return table[e] }
        if e < 0, -e < table.count { return 1 / table[-e] }
        return Foundation.pow(10, Double(e))
    }

    mutating func integer() -> Int {
        var v = 0
        while i < n, p[i] &- 48 < 10 { v = v &* 10 &+ Int(p[i] &- 48); i += 1 }
        return v
    }

    /// 3MF's `m00 m01 m02 m10 m11 m12 m20 m21 m22 m30 m31 m32`, row vectors
    /// (`p' = p M`), as a column-vector matrix.
    static func transform(_ s: String?) -> simd_double4x4 {
        guard let s else { return matrix_identity_double4x4 }
        let m = s.split(whereSeparator: { $0 == " " || $0 == "\t" || $0 == "\n" }).compactMap { Double($0) }
        guard m.count == 12 else { return matrix_identity_double4x4 }
        return simd_double4x4(columns: (
            SIMD4(m[0], m[1], m[2], 0),
            SIMD4(m[3], m[4], m[5], 0),
            SIMD4(m[6], m[7], m[8], 0),
            SIMD4(m[9], m[10], m[11], 1)))
    }

    static func unescape(_ s: String) -> String {
        guard s.contains("&") else { return s }
        return s.replacingOccurrences(of: "&quot;", with: "\"").replacingOccurrences(of: "&apos;", with: "'")
            .replacingOccurrences(of: "&lt;", with: "<").replacingOccurrences(of: "&gt;", with: ">")
            .replacingOccurrences(of: "&amp;", with: "&")
    }

    /// The colour a painted triangle mostly takes. The string is a
    /// subdivision tree, one hex digit per node, read from the end: the low
    /// two bits are the number of split sides (0 for a leaf); a leaf's state
    /// is the high two bits, or 3 plus the next digit when those are 3. Each
    /// leaf counts once; the commonest state wins.
    static func paintState(_ s: UnsafePointer<UInt8>, _ count: Int) -> UInt8 {
        guard count > 0 else { return 0 }
        var tally = [Int](repeating: 0, count: 20)
        var k = count - 1
        var pending = 1
        func nibble() -> Int? {
            while k >= 0 {
                let c = s[k]
                k -= 1
                switch c {
                case 48 ... 57: return Int(c - 48)
                case 65 ... 70: return Int(c - 55)
                case 97 ... 102: return Int(c - 87)
                default: continue
                }
            }
            return nil
        }
        while pending > 0, let code = nibble() {
            pending -= 1
            let split = code & 3
            if split != 0 {
                pending += split + 1
            } else {
                var state = code >> 2
                if state == 3, let more = nibble() { state = 3 + more }
                tally[min(state, tally.count - 1)] += 1
            }
        }
        var best = 0
        for s in 1 ..< tally.count where tally[s] > tally[best] { best = s }
        return UInt8(best)
    }
}
