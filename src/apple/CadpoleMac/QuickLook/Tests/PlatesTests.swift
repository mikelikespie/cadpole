// The plate preview's reader, against small projects built here.
//
//   bazel test //src/apple/CadpoleMac/QuickLook:PlatesTests
//
// Set CADPOLE_3MF_SAMPLE (bazel test --test_env=CADPOLE_3MF_SAMPLE=...) to
// also read a real slicer project.

import CadpolePlates
import Compression
import Foundation
import XCTest
import simd

final class PlatesTests: XCTestCase {
    // MARK: - Fixture

    /// A 3-plate project on a 200 x 180 mm bed. Plate 1 is empty; plate 2
    /// holds two wheels (one mirrored) and a chassis; plate 3 a stand with
    /// one painted face. Plates sit on Bambu's grid: 2 to a row, 1.2 beds
    /// apart, so plate 2 starts at x = 240 and plate 3 at y = -216.
    static let modelSettings = """
        <?xml version="1.0" encoding="UTF-8"?>
        <config>
          <object id="2">
            <metadata key="name" value="wheel"/>
            <metadata key="extruder" value="1"/>
            <part id="1" subtype="normal_part"><metadata key="name" value="tyre"/><metadata key="extruder" value="3"/></part>
            <part id="3" subtype="normal_part"><metadata key="name" value="hub"/></part>
            <part id="4" subtype="modifier_part"><metadata key="extruder" value="4"/></part>
          </object>
          <object id="5">
            <metadata key="name" value="chassis"/>
            <metadata key="extruder" value="2"/>
          </object>
          <object id="7">
            <metadata key="name" value="stand"/>
          </object>
          <plate>
            <metadata key="plater_id" value="3"/>
            <metadata key="plater_name" value=""/>
            <model_instance><metadata key="object_id" value="7"/><metadata key="instance_id" value="0"/></model_instance>
          </plate>
          <plate>
            <metadata key="plater_id" value="1"/>
            <metadata key="plater_name" value="spare"/>
          </plate>
          <plate>
            <metadata key="plater_id" value="2"/>
            <metadata key="plater_name" value="Wheels"/>
            <model_instance><metadata key="object_id" value="2"/><metadata key="instance_id" value="0"/></model_instance>
            <model_instance><metadata key="object_id" value="5"/><metadata key="instance_id" value="0"/></model_instance>
            <model_instance><metadata key="object_id" value="2"/><metadata key="instance_id" value="1"/></model_instance>
          </plate>
        </config>
        """

    static let projectSettings = """
        {"filament_colour": ["#171717", "#FF8800", "#FFFFFFFF", "#00AA00"],
         "filament_type": ["PLA", "PETG", "PLA", "TPU"],
         "printable_area": ["0x0", "200x0", "200x180", "0x180"]}
        """

    static let rootModel = """
        <?xml version="1.0" encoding="UTF-8"?>
        <model unit="millimeter" xmlns="http://schemas.microsoft.com/3dmanufacturing/core/2015/02"
          xmlns:p="http://schemas.microsoft.com/3dmanufacturing/production/2015/06" requiredextensions="p">
         <resources>
          <object id="2" type="model">
           <components>
            <component p:path="/3D/Objects/object_1.model" objectid="1" transform="1 0 0 0 1 0 0 0 1 0 0 0"/>
            <component p:path="/3D/Objects/object_1.model" objectid="3" transform="1 0 0 0 1 0 0 0 1 0 0 1"/>
            <component p:path="/3D/Objects/object_1.model" objectid="4" transform="1 0 0 0 1 0 0 0 1 0 0 2"/>
           </components>
          </object>
          <object id="5" type="model">
           <components><component p:path="/3D/Objects/object_1.model" objectid="6"/></components>
          </object>
          <object id="7" type="model">
           <components><component p:path="/3D/Objects/object_2.model" objectid="8"/></components>
          </object>
         </resources>
         <build>
          <item objectid="2" transform="1 0 0 0 1 0 0 0 1 290 50 0" printable="1"/>
          <item objectid="2" transform="-1 0 0 0 1 0 0 0 1 341 50 0" printable="1"/>
          <item objectid="5" transform="10 0 0 0 10 0 0 0 1 390 100 0" printable="1"/>
          <item objectid="7" transform="1 0 0 0 1 0 0 0 1 20 -186 0" printable="1"/>
         </build>
        </model>
        """

    /// A unit cube's vertices and its twelve outward-facing triangles.
    static func cube(id: Int, size: Double = 1, paint: [Int: String] = [:]) -> String {
        let s = size
        let v = [(0, 0, 0), (s, 0, 0), (s, s, 0), (0, s, 0), (0, 0, s), (s, 0, s), (s, s, s), (0, s, s)]
            .map { "<vertex x=\"\($0.0)\" y=\"\($0.1)\" z=\"\($0.2)\"/>" }.joined()
        let t = [(0, 2, 1), (0, 3, 2), (4, 5, 6), (4, 6, 7), (0, 1, 5), (0, 5, 4),
                 (1, 2, 6), (1, 6, 5), (2, 3, 7), (2, 7, 6), (3, 0, 4), (3, 4, 7)]
            .enumerated().map { i, f in
                let p = paint[i].map { " paint_color=\"\($0)\"" } ?? ""
                return "<triangle v1=\"\(f.0)\" v2=\"\(f.1)\" v3=\"\(f.2)\"\(p)/>"
            }.joined(separator: "\n")
        return "<object id=\"\(id)\" type=\"model\"><mesh><vertices>\(v)</vertices><triangles>\n\(t)\n</triangles></mesh></object>"
    }

    static func objectFile(_ objects: [String]) -> String {
        "<?xml version=\"1.0\"?><model unit=\"millimeter\"><resources>\(objects.joined())</resources></model>"
    }

    static func project() -> Data {
        ZipWriter.archive([
            ("[Content_Types].xml", Data("<Types/>".utf8), true),
            ("3D/3dmodel.model", Data(rootModel.utf8), true),
            ("3D/Objects/object_1.model", Data(objectFile([cube(id: 1), cube(id: 3), cube(id: 4), cube(id: 6)]).utf8), true),
            ("3D/Objects/object_2.model", Data(objectFile([cube(id: 8, size: 10, paint: [2: "8"])]).utf8), false),
            ("Metadata/project_settings.config", Data(projectSettings.utf8), true),
            ("Metadata/model_settings.config", Data(modelSettings.utf8), true),
        ])
    }

    // MARK: - ZIP

    func testReadsStoredAndDeflatedEntries() throws {
        let text = Data(String(repeating: "plate ", count: 500).utf8)
        let zip = try ZipReader(data: ZipWriter.archive([("a.txt", text, false), ("b/c.txt", text, true)]))
        XCTAssertEqual(try zip.read("a.txt"), text)
        XCTAssertEqual(try zip.read("b/c.txt"), text)
        XCTAssertNil(try zip.read("missing"))
    }

    func testRejectsSomethingThatIsNotAZip() {
        XCTAssertThrowsError(try ZipReader(data: Data("solid cube\nendsolid\n".utf8))) {
            XCTAssertEqual($0 as? ZipReader.Failure, .notAZip)
        }
    }

    func testAZipWithoutAModelIsNotShown() throws {
        let zip = try ZipReader(data: ZipWriter.archive([("readme.txt", Data("hi".utf8), false)]))
        XCTAssertThrowsError(try PlateProject(archive: zip)) {
            XCTAssertEqual($0 as? PlateProject.Failure, .noRootModel)
        }
    }

    // MARK: - Settings

    func testReadsPlatesBedAndOrder() throws {
        let project = try PlateProject(archive: ZipReader(data: Self.project()))
        XCTAssertEqual(project.plates.map(\.id), [1, 2, 3])
        XCTAssertEqual(project.previewOrder.map(\.id), [2, 3, 1], "first plate with objects first, empty plates last")
        let wheels = project.plates[1]
        XCTAssertEqual(wheels.title, "Plate 2 \u{2014} Wheels")
        XCTAssertEqual(wheels.objects, [.init(name: "wheel", count: 2), .init(name: "chassis", count: 1)])
        XCTAssertEqual(wheels.items.count, 3)
        XCTAssertTrue(project.plates[0].isEmpty)
        XCTAssertTrue(project.bed.fromProject)
        XCTAssertEqual(project.bed.size, SIMD2(200, 180))
        XCTAssertEqual(project.plateOrigin(1), SIMD2(0, 0))
        XCTAssertEqual(project.plateOrigin(2), SIMD2(240, 0))
        XCTAssertEqual(project.plateOrigin(3), SIMD2(0, -216))
        XCTAssertEqual(project.filament(2)?.type, "PETG")
    }

    func testABedlessProjectFallsBackTo256mm() throws {
        let zip = try ZipReader(data: ZipWriter.archive([
            ("3D/3dmodel.model", Data(Self.objectFile([Self.cube(id: 1)]).replacingOccurrences(
                of: "</resources>", with: "</resources><build><item objectid=\"1\"/></build>").utf8), true),
        ]))
        let project = try PlateProject(archive: zip)
        XCTAssertFalse(project.bed.fromProject)
        XCTAssertEqual(project.bed.size, SIMD2(256, 256))
    }

    // MARK: - Geometry

    func testBuildsAPlateInItsOwnCoordinates() throws {
        let zip = try ZipReader(data: Self.project())
        let project = try PlateProject(archive: zip)
        let g = PlateBuilder(archive: zip, project: project).build(plateId: 2)
        // Two wheels of tyre + hub (the modifier is not drawn), and the chassis.
        XCTAssertEqual(g.triangleCount, 2 * 24 + 12)
        XCTAssertEqual(g.batches.map(\.filament), [1, 2, 3], "hub inherits the wheel's 1, chassis 2, tyre 3")
        XCTAssertEqual(g.batches.reduce(0) { $0 + $1.vertexCount }, 3 * g.triangleCount)
        XCTAssertEqual(g.batches[1].colour, SIMD4(1, 0x88 / 255.0, 0, 1))
        let b = try XCTUnwrap(g.bounds)
        // Plate 2 starts at x = 240: the wheels at 50 (one mirrored, 100 - 1 ... 101 - 1).
        XCTAssertEqual(b.min.x, 50, accuracy: 1e-4)
        XCTAssertEqual(b.max.x, 160, accuracy: 1e-4)
        XCTAssertEqual(b.min.y, 50, accuracy: 1e-4)
        XCTAssertEqual(b.max.z, 2, accuracy: 1e-4)
        // From above: the hubs sit on the tyres, so the wheels show 1; the chassis 2.
        XCTAssertEqual(Set(g.footprint.cells), [0, 1, 2])
    }

    /// Every triangle's winding and its stored normals agree, mirrored
    /// instance included, and a cube's corners stay sharp.
    func testNormalsFaceOutwardAndCreasesStaySharp() throws {
        let zip = try ZipReader(data: Self.project())
        let project = try PlateProject(archive: zip)
        let g = PlateBuilder(archive: zip, project: project).build(plateId: 2)
        for batch in g.batches {
            let p = batch.positions.withUnsafeBytes { Array($0.bindMemory(to: Float.self)) }
            let n = batch.normals.withUnsafeBytes { Array($0.bindMemory(to: Float.self)) }
            for t in 0 ..< batch.vertexCount / 3 {
                func v(_ a: [Float], _ k: Int) -> SIMD3<Float> { SIMD3(a[9 * t + 3 * k], a[9 * t + 3 * k + 1], a[9 * t + 3 * k + 2]) }
                let face = simd_normalize(simd_cross(v(p, 1) - v(p, 0), v(p, 2) - v(p, 0)))
                for k in 0 ..< 3 {
                    XCTAssertEqual(simd_dot(face, v(n, k)), 1, accuracy: 1e-4, "filament \(batch.filament) triangle \(t)")
                }
            }
        }
    }

    func testShallowFoldsAreSmoothed() {
        // Two triangles sharing an edge, folded by 10 degrees: one normal at
        // the shared vertices; at 90 degrees they would stay apart.
        let a = Float(10 * Double.pi / 180)
        var mesh = ModelFile.Mesh()
        mesh.positions = [0, 0, 0, 1, 0, 0, 0, 1, 0, 0, -cos(a), sin(a)]
        mesh.indices = [0, 1, 2, 0, 3, 1]
        mesh.paint = [0, 0]
        let n = PlateBuilder.creaseNormals(mesh, cosLimit: cos(30 * .pi / 180))
        // Vertex 0 in triangle 0 and in triangle 1.
        XCTAssertEqual(n[0], n[9], accuracy: 1e-6)
        XCTAssertEqual(n[1], n[10], accuracy: 1e-6)
        XCTAssertEqual(n[2], n[11], accuracy: 1e-6)
        let sharp = PlateBuilder.creaseNormals(mesh, cosLimit: cos(5 * .pi / 180))
        XCTAssertNotEqual(sharp[1], sharp[10], accuracy: 1e-3)
    }

    func testPaintedFacesTakeTheirFilament() throws {
        let zip = try ZipReader(data: Self.project())
        let project = try PlateProject(archive: zip)
        let g = PlateBuilder(archive: zip, project: project).build(plateId: 3)
        XCTAssertEqual(g.batches.map(\.filament), [1, 2])
        XCTAssertEqual(g.batches.map(\.vertexCount), [11 * 3, 3])
        let b = try XCTUnwrap(g.bounds)
        XCTAssertEqual(b.min.y, 30, accuracy: 1e-4, "plate 3 starts at y = -216")
    }

    func testPaintEncoding() {
        func state(_ s: String) -> UInt8 {
            let m = ModelFile(parsing: Data(Self.objectFile(["""
                <object id="1"><mesh><vertices><vertex x="0" y="0" z="0"/></vertices>
                <triangles><triangle v1="0" v2="0" v3="0" paint_color="\(s)"/></triangles></mesh></object>
                """]).utf8))
            return m.objects[1]?.mesh?.paint.first ?? 255
        }
        XCTAssertEqual(state("4"), 1)
        XCTAssertEqual(state("8"), 2)
        XCTAssertEqual(state("1C"), 4, "state 3 + the next digit")
        XCTAssertEqual(state("84483"), 1, "split in four: states 2, 1, 1, 2; the first commonest wins")
    }

    func testAPlain3MFIsOnePlateCentredOnTheBed() throws {
        let model = """
            <model unit="millimeter"><resources>
            <basematerials id="9"><base name="a" displaycolor="#5B7FA3"/><base name="b" displaycolor="#F4F3EE"/></basematerials>
            \(Self.cube(id: 1, size: 20).replacingOccurrences(of: "x=\"0", with: "x=\"-50"))
            </resources><build><item objectid="1"/></build></model>
            """
        let zip = try ZipReader(data: ZipWriter.archive([("3D/3dmodel.model", Data(model.utf8), true)]))
        let project = try PlateProject(archive: zip)
        XCTAssertFalse(project.laidOut)
        XCTAssertEqual(project.filaments.map(\.colour), ["#5B7FA3", "#F4F3EE"])
        let g = PlateBuilder(archive: zip, project: project).build(plateId: 1)
        let b = try XCTUnwrap(g.bounds)
        XCTAssertEqual((b.min.x + b.max.x) / 2, 128, accuracy: 1e-3)
        XCTAssertEqual(b.min.z, 0, accuracy: 1e-4)
        XCTAssertEqual(g.batches.first?.colour.x ?? 0, 0x5B / 255.0, accuracy: 1e-4)
    }

    // MARK: - A real project, when one is given

    func testARealProject() throws {
        guard let path = ProcessInfo.processInfo.environment["CADPOLE_3MF_SAMPLE"], !path.isEmpty else {
            throw XCTSkip("CADPOLE_3MF_SAMPLE is not set")
        }
        let zip = try ZipReader(url: URL(fileURLWithPath: path))
        let project = try PlateProject(archive: zip)
        let builder = PlateBuilder(archive: zip, project: project)
        for plate in project.previewOrder where !plate.isEmpty {
            let g = builder.build(plateId: plate.id)
            XCTAssertGreaterThan(g.triangleCount, 0, plate.title)
            let b = try XCTUnwrap(g.bounds)
            let bed = project.bed.maximum
            XCTAssertGreaterThanOrEqual(Double(b.min.x), -1, "\(plate.title) on its bed")
            XCTAssertLessThanOrEqual(Double(b.max.x), bed.x + 1, "\(plate.title) on its bed")
            XCTAssertGreaterThanOrEqual(Double(b.min.y), -1, "\(plate.title) on its bed")
            XCTAssertLessThanOrEqual(Double(b.max.y), bed.y + 1, "\(plate.title) on its bed")
            print("\(plate.title): \(g.triangleCount) triangles, \(g.batches.count) colours, \(g.timings.total)")
        }
    }
}

enum ZipWriter {
    static func archive(_ entries: [(name: String, data: Data, deflate: Bool)]) -> Data {
        var out = Data()
        var central = Data()
        for (name, data, deflate) in entries {
            let body = deflate ? compress(data) : data
            let method: UInt16 = deflate ? 8 : 0
            let offset = UInt32(out.count)
            let nameBytes = Data(name.utf8)
            var local = Data()
            local.le32(0x0403_4B50); local.le16(20); local.le16(0); local.le16(method)
            local.le16(0); local.le16(0x21)  // time, date (1980-01-01)
            local.le32(0)  // CRC: the reader does not check it
            local.le32(UInt32(body.count)); local.le32(UInt32(data.count))
            local.le16(UInt16(nameBytes.count)); local.le16(0)
            out.append(local); out.append(nameBytes); out.append(body)

            central.le32(0x0201_4B50); central.le16(20); central.le16(20); central.le16(0); central.le16(method)
            central.le16(0); central.le16(0x21); central.le32(0)
            central.le32(UInt32(body.count)); central.le32(UInt32(data.count))
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

    private static func compress(_ data: Data) -> Data {
        var out = Data(count: data.count + 1024)
        let n = out.withUnsafeMutableBytes { dst in
            data.withUnsafeBytes { src in
                compression_encode_buffer(
                    dst.bindMemory(to: UInt8.self).baseAddress!, dst.count,
                    src.bindMemory(to: UInt8.self).baseAddress!, data.count, nil, COMPRESSION_ZLIB)
            }
        }
        precondition(n > 0)
        return out.prefix(n)
    }
}

extension Data {
    fileprivate mutating func le16(_ v: UInt16) { append(contentsOf: [UInt8(v & 0xFF), UInt8(v >> 8)]) }
    fileprivate mutating func le32(_ v: UInt32) { le16(UInt16(v & 0xFFFF)); le16(UInt16(v >> 16)) }
}
