import Foundation

/// An invented print project for the viewers' tests and screenshots: a slicer
/// project (3MF, as Bambu Studio and Orca save one) with real-looking parts
/// on a 256 mm bed, so a view can be shot from above and from under the bed.
///
/// - Plate 1, "Workshop": a spur gear with a hub, a knurled knob, an
///   L-bracket with a gusset and a hex spacer, in four filaments.
/// - Plate 2, "Knobs": a row of knobs.
/// - Plate 3, "Spacers": hex spacers and a small gear.
///
/// Foundation only: the meshes are made here, the archive is a stored ZIP.
public enum DemoPlates {
    public struct Mesh: Sendable {
        public var vertices: [SIMD3<Double>] = []
        public var triangles: [SIMD3<Int>] = []

        public init() {}

        mutating func add(_ v: SIMD3<Double>) -> Int {
            vertices.append(v)
            return vertices.count - 1
        }

        /// A closed prism: `outline` (counter-clockwise from above, and
        /// star-shaped about its centroid) from `z0` up to `z1`, through
        /// `place` (outline x, y and height to the part's own space).
        mutating func prism(_ outline: [SIMD2<Double>], _ z0: Double, _ z1: Double,
                            place: (SIMD2<Double>, Double) -> SIMD3<Double> = { SIMD3($0.x, $0.y, $1) }) {
            let n = outline.count
            let c = outline.reduce(SIMD2<Double>.zero, +) / Double(n)
            let bottom = outline.map { add(place($0, z0)) }
            let top = outline.map { add(place($0, z1)) }
            let cb = add(place(c, z0))
            let ct = add(place(c, z1))
            for i in 0 ..< n {
                let j = (i + 1) % n
                triangles.append(SIMD3(cb, bottom[j], bottom[i]))
                triangles.append(SIMD3(ct, top[i], top[j]))
                triangles.append(SIMD3(bottom[i], bottom[j], top[j]))
                triangles.append(SIMD3(bottom[i], top[j], top[i]))
            }
        }

        /// A box from `lo` to `hi`.
        mutating func box(_ lo: SIMD3<Double>, _ hi: SIMD3<Double>) {
            prism([SIMD2(lo.x, lo.y), SIMD2(hi.x, lo.y), SIMD2(hi.x, hi.y), SIMD2(lo.x, hi.y)], lo.z, hi.z)
        }
    }

    // MARK: - Parts (z-up millimetres, standing on z = 0, centred on x, y = 0)

    static func circle(_ r: Double, _ n: Int, _ bump: (Double) -> Double = { _ in 0 }) -> [SIMD2<Double>] {
        (0 ..< n).map { i in
            let a = Double(i) / Double(n) * 2 * .pi
            let rr = r + bump(a)
            return SIMD2(rr * cos(a), rr * sin(a))
        }
    }

    /// A spur gear, `teeth` teeth on a `radius` pitch circle, with a hub.
    public static func gear(teeth: Int = 24, radius: Double = 30, thickness: Double = 7, hub: Double = 9) -> Mesh {
        var m = Mesh()
        let p = 2 * Double.pi / Double(teeth)
        let root = radius - 2.5, tip = radius + 2.2
        var outline: [SIMD2<Double>] = []
        for i in 0 ..< teeth {
            let a = Double(i) * p
            for (r, f) in [(root, -0.5), (root, -0.3), (tip, -0.14), (tip, 0.14), (root, 0.3)] {
                outline.append(SIMD2(r * cos(a + f * p), r * sin(a + f * p)))
            }
        }
        m.prism(outline, 0, thickness)
        m.prism(circle(hub, 40), thickness, thickness + 6)
        return m
    }

    /// A knurled knob with a cap.
    public static func knob(radius: Double = 15, height: Double = 16) -> Mesh {
        var m = Mesh()
        m.prism(circle(radius, 180) { 0.7 * cos(30 * $0) }, 0, height)
        m.prism(circle(radius * 0.62, 48), height, height + 3)
        return m
    }

    /// An L-bracket: a base, an upright and a gusset between them.
    public static func bracket(width: Double = 64, depth: Double = 36, thickness: Double = 5, height: Double = 40) -> Mesh {
        var m = Mesh()
        let x = width / 2, y = depth / 2
        m.box(SIMD3(-x, -y, 0), SIMD3(x, y, thickness))
        m.box(SIMD3(-x, y - thickness, thickness), SIMD3(x, y, height))
        // A triangular gusset across the middle, extruded along x.
        let g = [SIMD2(y - thickness, thickness), SIMD2(y - thickness, height - 6), SIMD2(-y + 6, thickness)]
        m.prism(g, -2, 2) { p, h in SIMD3(h, p.x, p.y) }
        return m
    }

    /// A hex spacer.
    public static func spacer(across: Double = 22, height: Double = 12) -> Mesh {
        var m = Mesh()
        m.prism(circle(across / 2 / cos(.pi / 6), 6), 0, height)
        return m
    }

    // MARK: - The project

    public struct Part: Sendable {
        public var name: String
        public var mesh: Mesh
        /// Where its origin stands on the plate (bed millimetres) and its turn.
        public var at: SIMD2<Double>
        public var turnDegrees: Double
        /// The filament it prints in, from 1.
        public var filament: Int
    }

    public static let bedSize = SIMD2<Double>(256, 256)
    /// Orange PETG, charcoal PLA, white PLA, teal PLA.
    public static let filaments = ["#F2862E", "#3B3E44", "#EDEBE6", "#2A9D8F"]

    public static let plates: [(name: String, parts: [Part])] = [
        ("Workshop", [
            Part(name: "gear", mesh: gear(), at: SIMD2(78, 92), turnDegrees: 0, filament: 1),
            Part(name: "knob", mesh: knob(), at: SIMD2(182, 70), turnDegrees: 0, filament: 3),
            Part(name: "bracket", mesh: bracket(), at: SIMD2(160, 178), turnDegrees: -20, filament: 2),
            Part(name: "spacer", mesh: spacer(), at: SIMD2(70, 190), turnDegrees: 10, filament: 4),
        ]),
        ("Knobs", (0 ..< 4).map { i in
            Part(name: "knob", mesh: knob(), at: SIMD2(56 + Double(i) * 48, 128), turnDegrees: 0, filament: 3)
        }),
        ("Spacers", [
            Part(name: "spacer", mesh: spacer(), at: SIMD2(80, 90), turnDegrees: 0, filament: 4),
            Part(name: "spacer", mesh: spacer(), at: SIMD2(176, 90), turnDegrees: 15, filament: 4),
            Part(name: "gear", mesh: gear(teeth: 16, radius: 20, thickness: 6, hub: 7), at: SIMD2(128, 170), turnDegrees: 0, filament: 1),
        ]),
    ]

    /// The project as a 3MF. `only` keeps one plate (as plate 1);
    /// `thumbnails` are written as `Metadata/plate_N.png`.
    public static func project(only: Int? = nil, thumbnails: [Int: Data] = [:]) -> Data {
        let chosen = only.map { [plates[$0 - 1]] } ?? plates
        let count = chosen.count
        var cols = 1
        while cols * cols < count { cols += 1 }
        var objectsXML: [String] = []
        var rootObjects: [String] = []
        var items: [String] = []
        var settingsObjects: [String] = []
        var settingsPlates: [String] = []
        var nextId = 1
        for (p, plate) in chosen.enumerated() {
            let origin = SIMD2(Double(p % cols) * 1.2 * bedSize.x, -Double(p / cols) * 1.2 * bedSize.y)
            var instances: [String] = []
            for part in plate.parts {
                let meshId = nextId, objectId = nextId + 1
                nextId += 2
                objectsXML.append(meshXML(part.mesh, id: meshId))
                rootObjects.append("""
                    <object id="\(objectId)" type="model"><components><component p:path="/3D/Objects/object_1.model" objectid="\(meshId)"/></components></object>
                    """)
                let a = part.turnDegrees * .pi / 180
                let t = [cos(a), sin(a), 0, -sin(a), cos(a), 0, 0, 0, 1, origin.x + part.at.x, origin.y + part.at.y, 0]
                items.append("<item objectid=\"\(objectId)\" transform=\"\(t.map { String(format: "%.6g", $0) }.joined(separator: " "))\" printable=\"1\"/>")
                settingsObjects.append("""
                    <object id="\(objectId)"><metadata key="name" value="\(part.name)"/><metadata key="extruder" value="\(part.filament)"/></object>
                    """)
                instances.append("<model_instance><metadata key=\"object_id\" value=\"\(objectId)\"/><metadata key=\"instance_id\" value=\"0\"/></model_instance>")
            }
            settingsPlates.append("""
                <plate><metadata key="plater_id" value="\(p + 1)"/><metadata key="plater_name" value="\(plate.name)"/>\(instances.joined())</plate>
                """)
        }
        let root = """
            <?xml version="1.0" encoding="UTF-8"?>
            <model unit="millimeter" xmlns="http://schemas.microsoft.com/3dmanufacturing/core/2015/02" \
            xmlns:p="http://schemas.microsoft.com/3dmanufacturing/production/2015/06" requiredextensions="p">
             <resources>
            \(rootObjects.joined(separator: "\n"))
             </resources>
             <build>
            \(items.joined(separator: "\n"))
             </build>
            </model>
            """
        let objectFile = """
            <?xml version="1.0" encoding="UTF-8"?>
            <model unit="millimeter" xmlns="http://schemas.microsoft.com/3dmanufacturing/core/2015/02"><resources>
            \(objectsXML.joined(separator: "\n"))
            </resources><build/></model>
            """
        let settings = "<?xml version=\"1.0\" encoding=\"UTF-8\"?>\n<config>\n\(settingsObjects.joined(separator: "\n"))\n\(settingsPlates.joined(separator: "\n"))\n</config>\n"
        let w = Int(bedSize.x), d = Int(bedSize.y)
        let project = """
            {"filament_colour": [\(filaments.map { "\"\($0)\"" }.joined(separator: ", "))],
             "filament_type": ["PETG", "PLA", "PLA", "PLA"],
             "printable_area": ["0x0", "\(w)x0", "\(w)x\(d)", "0x\(d)"]}
            """
        var entries: [(name: String, data: Data)] = [
            ("[Content_Types].xml", Data("""
                <?xml version="1.0" encoding="UTF-8"?>
                <Types xmlns="http://schemas.openxmlformats.org/package/2006/content-types">\
                <Default Extension="rels" ContentType="application/vnd.openxmlformats-package.relationships+xml"/>\
                <Default Extension="model" ContentType="application/vnd.ms-package.3dmanufacturing-3dmodel+xml"/>\
                <Default Extension="png" ContentType="image/png"/></Types>
                """.utf8)),
            ("_rels/.rels", Data("""
                <?xml version="1.0" encoding="UTF-8"?>
                <Relationships xmlns="http://schemas.openxmlformats.org/package/2006/relationships">\
                <Relationship Target="/3D/3dmodel.model" Id="rel0" Type="http://schemas.microsoft.com/3dmanufacturing/2013/01/3dmodel"/>\
                </Relationships>
                """.utf8)),
            ("3D/3dmodel.model", Data(root.utf8)),
            ("3D/Objects/object_1.model", Data(objectFile.utf8)),
            ("Metadata/model_settings.config", Data(settings.utf8)),
            ("Metadata/project_settings.config", Data(project.utf8)),
        ]
        for (id, png) in thumbnails.sorted(by: { $0.key < $1.key }) {
            entries.append(("Metadata/plate_\(id).png", png))
        }
        return StoredZip.archive(entries)
    }

    static func meshXML(_ m: Mesh, id: Int) -> String {
        var s = "<object id=\"\(id)\" type=\"model\"><mesh><vertices>"
        for v in m.vertices { s += String(format: "<vertex x=\"%.4f\" y=\"%.4f\" z=\"%.4f\"/>", v.x, v.y, v.z) }
        s += "</vertices><triangles>"
        for t in m.triangles { s += "<triangle v1=\"\(t.x)\" v2=\"\(t.y)\" v3=\"\(t.z)\"/>" }
        return s + "</triangles></mesh></object>"
    }
}

/// A ZIP of stored (uncompressed) entries, with real CRCs.
public enum StoredZip {
    public static func archive(_ entries: [(name: String, data: Data)]) -> Data {
        var out = Data()
        var central = Data()
        for (name, data) in entries {
            let offset = UInt32(out.count)
            let nameBytes = Data(name.utf8)
            let crc = crc32(data)
            var local = Data()
            local.le32(0x0403_4B50); local.le16(20); local.le16(0); local.le16(0)
            local.le16(0); local.le16(0x21)  // 1980-01-01, no time
            local.le32(crc); local.le32(UInt32(data.count)); local.le32(UInt32(data.count))
            local.le16(UInt16(nameBytes.count)); local.le16(0)
            out.append(local); out.append(nameBytes); out.append(data)

            central.le32(0x0201_4B50); central.le16(20); central.le16(20); central.le16(0); central.le16(0)
            central.le16(0); central.le16(0x21); central.le32(crc)
            central.le32(UInt32(data.count)); central.le32(UInt32(data.count))
            central.le16(UInt16(nameBytes.count)); central.le16(0); central.le16(0)
            central.le16(0); central.le16(0); central.le32(0); central.le32(offset)
            central.append(nameBytes)
        }
        let cdOffset = UInt32(out.count)
        out.append(central)
        out.le32(0x0605_4B50); out.le16(0); out.le16(0)
        out.le16(UInt16(entries.count)); out.le16(UInt16(entries.count))
        out.le32(UInt32(central.count)); out.le32(cdOffset); out.le16(0)
        return out
    }

    static let table: [UInt32] = (0 ..< 256).map { n in
        var c = UInt32(n)
        for _ in 0 ..< 8 { c = c & 1 != 0 ? 0xEDB8_8320 ^ (c >> 1) : c >> 1 }
        return c
    }

    public static func crc32(_ data: Data) -> UInt32 {
        var c: UInt32 = 0xFFFF_FFFF
        for b in data { c = table[Int((c ^ UInt32(b)) & 0xFF)] ^ (c >> 8) }
        return c ^ 0xFFFF_FFFF
    }
}

extension Data {
    fileprivate mutating func le16(_ v: UInt16) { append(contentsOf: [UInt8(v & 0xFF), UInt8(v >> 8)]) }
    fileprivate mutating func le32(_ v: UInt32) { le16(UInt16(v & 0xFFFF)); le16(UInt16(v >> 16)) }
}
