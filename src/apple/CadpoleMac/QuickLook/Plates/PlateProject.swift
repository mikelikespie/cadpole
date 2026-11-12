import Foundation
import simd

/// What a 3MF slicer project (Bambu Studio, Orca Slicer, `cadpole export` for
/// a Bambu printer) says about its build plates, read without touching any
/// mesh: which objects sit on each plate, which filament each part prints
/// in, the filaments' colours, the printer's bed, and the build items and
/// components that place each object's meshes.
///
/// Sources: `Metadata/model_settings.config` (plates, objects, parts,
/// extruders), `Metadata/project_settings.config` (filament colours, the
/// bed's `printable_area`) and the root model `3D/3dmodel.model` (build items
/// and components; in a Bambu project it holds no mesh). A plain 3MF without
/// plate settings is one plate holding every build item.
public struct PlateProject: Sendable {
    public struct Filament: Sendable, Equatable {
        /// 1-based, as the project numbers them.
        public let index: Int
        /// `#RRGGBB` or `#RRGGBBAA` as stored; empty when the project has none.
        public let colour: String
        public let type: String

        public init(index: Int, colour: String, type: String) {
            self.index = index
            self.colour = colour
            self.type = type
        }
    }

    /// One object's settings: its name, its extruder, and its parts' (by the
    /// part's mesh object id, which is the component's `objectid`).
    public struct ObjectSettings: Sendable {
        public var name: String?
        public var extruder = 1
        public var partExtruders: [Int: Int] = [:]
        /// Modifiers, negative volumes, support blockers and enforcers: not
        /// printed, so not drawn.
        public var hiddenParts: Set<Int> = []
    }

    public struct Plate: Sendable, Equatable {
        public let id: Int
        public let name: String
        /// Build items on the plate, as indices into `root.items`.
        public let items: [Int]
        /// Object names and how many of each, for the sidebar.
        public let objects: [PlacedObject]

        public var isEmpty: Bool { items.isEmpty }
        public var objectCount: Int { objects.reduce(0) { $0 + $1.count } }
        public var title: String { name.isEmpty ? "Plate \(id)" : "Plate \(id) \u{2014} \(name)" }
    }

    public struct PlacedObject: Sendable, Equatable {
        public let name: String
        public let count: Int

        public init(name: String, count: Int) {
            self.name = name
            self.count = count
        }
    }

    public enum Failure: Error, Equatable {
        case noRootModel
        case nothingToShow
        case unreadableSettings(String)
    }

    public var plates: [Plate]
    public var filaments: [Filament]
    public var objects: [Int: ObjectSettings]
    /// The root model: its objects' components, and its build items.
    public var root: ModelFile
    public var rootPath: String
    public var bed: Bed
    /// True when the plates come from slicer settings, so build items are
    /// already placed on the plate grid; false for a plain 3MF, whose model
    /// is drawn centred on the bed.
    public var laidOut: Bool

    public static let modelSettings = "Metadata/model_settings.config"
    public static let projectSettings = "Metadata/project_settings.config"

    /// The order plates are offered in: plates with objects first, in plate
    /// order, so the first is the lowest-numbered plate that prints
    /// something; empty plates after them.
    public var previewOrder: [Plate] {
        plates.filter { !$0.isEmpty } + plates.filter(\.isEmpty)
    }

    public func filament(_ index: Int) -> Filament? {
        filaments.first { $0.index == index }
    }

    /// Where plate `id` starts in the project's world space: Bambu Studio
    /// lays plates out `ceil(sqrt(N))` to a row, `1.2 x` the bed apart, rows
    /// towards -y. A project without plate settings has its one plate at 0.
    public func plateOrigin(_ id: Int) -> SIMD2<Double> {
        let count = max(plates.map(\.id).max() ?? 1, id, 1)
        var cols = 1
        while cols * cols < count { cols += 1 }
        let i = max(id, 1) - 1
        let size = bed.size
        return SIMD2(Double(i % cols) * 1.2 * size.x, 0 - Double(i / cols) * 1.2 * size.y)
    }

    public init(archive: ZipReader) throws {
        let rootPath = Self.rootModelPath(archive)
        guard let rootData = try archive.read(rootPath) else { throw Failure.noRootModel }
        root = ModelFile(parsing: rootData)
        self.rootPath = rootPath

        var filaments: [Filament] = []
        var bed = Bed.fallback
        if let data = try archive.read(Self.projectSettings) {
            guard let json = try? JSONSerialization.jsonObject(with: data) as? [String: Any] else {
                throw Failure.unreadableSettings(Self.projectSettings)
            }
            let colours = json["filament_colour"] as? [String] ?? []
            let types = json["filament_type"] as? [String] ?? []
            filaments = (0 ..< max(colours.count, types.count)).map { i in
                Filament(index: i + 1, colour: i < colours.count ? colours[i] : "", type: i < types.count ? types[i] : "")
            }
            if let area = json["printable_area"] as? [String], let parsed = Bed(printableArea: area) {
                bed = parsed
            }
        }
        if filaments.isEmpty {
            filaments = root.baseColours.enumerated().map { Filament(index: $0.offset + 1, colour: $0.element, type: "") }
        }
        self.filaments = filaments
        self.bed = bed

        // Build items by object, so a plate's (object, instance) pair finds its item.
        var itemsByObject: [Int: [Int]] = [:]
        for (i, item) in root.items.enumerated() { itemsByObject[item.objectId, default: []].append(i) }

        var plates: [Plate] = []
        var objects: [Int: ObjectSettings] = [:]
        if let data = try archive.read(Self.modelSettings) {
            let parsed = try ModelSettingsParser.parse(data)
            objects = parsed.objects
            for raw in parsed.plates.sorted(by: { $0.id < $1.id }) {
                var items: [Int] = []
                for (object, instance) in raw.instances {
                    guard let list = itemsByObject[object], !list.isEmpty else { continue }
                    items.append(list[min(max(instance, 0), list.count - 1)])
                }
                plates.append(Plate(id: raw.id, name: raw.name, items: items, objects: Self.placed(items, root, objects)))
            }
        }
        laidOut = !plates.isEmpty
        if plates.isEmpty {
            guard !root.items.isEmpty else { throw Failure.nothingToShow }
            let all = Array(root.items.indices)
            plates = [Plate(id: 1, name: "", items: all, objects: Self.placed(all, root, objects))]
        }
        self.plates = plates
        self.objects = objects
    }

    private static func placed(_ items: [Int], _ root: ModelFile, _ objects: [Int: ObjectSettings]) -> [PlacedObject] {
        // Copies of an object, and separate objects that share a name (a left
        // and a right part split from one mesh), count as one line.
        var counts: [String: Int] = [:]
        var order: [String] = []
        for i in items {
            let id = root.items[i].objectId
            let name = objects[id]?.name ?? root.objects[id]?.name ?? "Object \(id)"
            if counts[name] == nil { order.append(name) }
            counts[name, default: 0] += 1
        }
        return order.map { PlacedObject(name: $0, count: counts[$0]!) }
    }

    /// The root model part: the package relationship's target, else the
    /// conventional path.
    static func rootModelPath(_ archive: ZipReader) -> String {
        if let rels = try? archive.read("_rels/.rels") {
            let text = String(decoding: rels, as: UTF8.self)
            for rel in ModelSettingsParser.relationships(text) where rel.type.hasSuffix("/3dmodel") {
                let name = rel.target.hasPrefix("/") ? String(rel.target.dropFirst()) : rel.target
                if archive.contains(name) { return name }
            }
        }
        return "3D/3dmodel.model"
    }
}

/// The printer's bed: the printable area's outline in plate coordinates (mm,
/// origin at the bed's front-left corner).
public struct Bed: Sendable, Equatable {
    public var outline: [SIMD2<Double>]
    /// False when the project names no bed and a 256 mm square stands in.
    public var fromProject: Bool

    public static let fallback = Bed(
        outline: [SIMD2(0, 0), SIMD2(256, 0), SIMD2(256, 256), SIMD2(0, 256)], fromProject: false)

    public var minimum: SIMD2<Double> { outline.reduce(SIMD2(repeating: .infinity)) { simd_min($0, $1) } }
    public var maximum: SIMD2<Double> { outline.reduce(SIMD2(repeating: -.infinity)) { simd_max($0, $1) } }
    /// The extent the plate grid is laid out with.
    public var size: SIMD2<Double> { maximum }

    public init(outline: [SIMD2<Double>], fromProject: Bool) {
        self.outline = outline
        self.fromProject = fromProject
    }

    /// Bambu's `printable_area`: `["0x0", "330x0", "330x320", "0x320"]`.
    public init?(printableArea: [String]) {
        let points = printableArea.compactMap { s -> SIMD2<Double>? in
            let xy = s.split(separator: "x")
            guard xy.count == 2, let x = Double(xy[0]), let y = Double(xy[1]) else { return nil }
            return SIMD2(x, y)
        }
        guard points.count >= 3, points.count == printableArea.count else { return nil }
        let size = points.reduce(SIMD2<Double>(repeating: 0)) { simd_max($0, $1) }
        guard size.x > 0, size.y > 0 else { return nil }
        self.init(outline: points, fromProject: true)
    }
}

/// `Metadata/model_settings.config`:
///
/// ```xml
/// <config>
///   <object id="2">
///     <metadata key="name" value="bracket"/>
///     <metadata key="extruder" value="1"/>
///     <part id="1" subtype="normal_part"><metadata key="extruder" value="3"/></part>
///   </object>
///   <plate>
///     <metadata key="plater_id" value="1"/>
///     <metadata key="plater_name" value=""/>
///     <model_instance>
///       <metadata key="object_id" value="2"/>
///       <metadata key="instance_id" value="0"/>
///     </model_instance>
///   </plate>
/// </config>
/// ```
///
/// A few hundred KB at most, so Foundation's XMLParser is quick enough.
enum ModelSettingsParser {
    struct RawPlate {
        var id = 0
        var name = ""
        var instances: [(object: Int, instance: Int)] = []
    }

    struct Result {
        var objects: [Int: PlateProject.ObjectSettings] = [:]
        var plates: [RawPlate] = []
    }

    static func parse(_ data: Data) throws -> Result {
        let delegate = Delegate()
        let parser = XMLParser(data: data)
        parser.delegate = delegate
        guard parser.parse() else {
            throw PlateProject.Failure.unreadableSettings(PlateProject.modelSettings)
        }
        return delegate.result
    }

    private final class Delegate: NSObject, XMLParserDelegate {
        var result = Result()
        private var stack: [String] = []
        private var objectId: Int?
        private var object = PlateProject.ObjectSettings()
        private var partId: Int?
        private var plate: RawPlate?
        private var instance: (object: Int, instance: Int)?

        func parser(
            _ parser: XMLParser, didStartElement element: String, namespaceURI: String?,
            qualifiedName: String?, attributes: [String: String] = [:]
        ) {
            let parent = stack.last
            stack.append(element)
            switch (parent, element) {
            case ("config", "object"):
                objectId = attributes["id"].flatMap(Int.init)
                object = PlateProject.ObjectSettings()
            case ("object", "part"):
                partId = attributes["id"].flatMap(Int.init)
                if let id = partId, (attributes["subtype"] ?? "normal_part") != "normal_part" {
                    object.hiddenParts.insert(id)
                }
            case ("config", "plate"):
                plate = RawPlate()
            case ("plate", "model_instance"):
                instance = (object: -1, instance: 0)
            case (_, "metadata"):
                metadata(in: parent, key: attributes["key"], value: attributes["value"] ?? "")
            default:
                break
            }
        }

        func parser(
            _ parser: XMLParser, didEndElement element: String, namespaceURI: String?, qualifiedName: String?
        ) {
            stack.removeLast()
            switch (stack.last, element) {
            case ("config", "object"):
                if let id = objectId { result.objects[id] = object }
                objectId = nil
            case ("object", "part"):
                partId = nil
            case ("plate", "model_instance"):
                if let i = instance, i.object >= 0 { plate?.instances.append(i) }
                instance = nil
            case ("config", "plate"):
                if let p = plate { result.plates.append(p) }
                plate = nil
            default:
                break
            }
        }

        private func metadata(in parent: String?, key: String?, value: String) {
            guard let key else { return }
            switch (parent, key) {
            case ("object", "name"): object.name = value
            case ("object", "extruder"): if let n = Int(value), n > 0 { object.extruder = n }
            case ("part", "extruder"): if let n = Int(value), n > 0, let id = partId { object.partExtruders[id] = n }
            case ("plate", "plater_id"): plate?.id = Int(value) ?? 0
            case ("plate", "plater_name"): plate?.name = value
            case ("model_instance", "object_id"): instance?.object = Int(value) ?? -1
            case ("model_instance", "instance_id"): instance?.instance = Int(value) ?? 0
            default: break
            }
        }
    }

    struct Relationship {
        var type: String
        var target: String
    }

    static func relationships(_ xml: String) -> [Relationship] {
        final class Rels: NSObject, XMLParserDelegate {
            var found: [Relationship] = []
            func parser(
                _ parser: XMLParser, didStartElement element: String, namespaceURI: String?,
                qualifiedName: String?, attributes: [String: String] = [:]
            ) {
                if element == "Relationship", let t = attributes["Type"], let target = attributes["Target"] {
                    found.append(Relationship(type: t, target: target))
                }
            }
        }
        let rels = Rels()
        let parser = XMLParser(data: Data(xml.utf8))
        parser.delegate = rels
        parser.parse()
        return rels.found
    }
}
