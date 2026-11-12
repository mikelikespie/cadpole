import Foundation
import simd

/// One plate's meshes, ready to upload: one flat, non-indexed vertex buffer
/// per filament (positions and normals, three `Float`s each), in the plate's
/// own coordinates (mm, z up, origin at the bed's front-left corner).
public struct PlateGeometry: Sendable {
    public struct Batch: Sendable {
        /// 1-based filament index.
        public let filament: Int
        /// Linear-light-agnostic sRGB components, 0...1.
        public let colour: SIMD4<Float>
        public let positions: Data
        public let normals: Data
        public var vertexCount: Int { positions.count / (3 * MemoryLayout<Float>.size) }
    }

    /// A coarse top-down picture of the plate: `width x height` cells over
    /// the bed's bounds, each the filament index (0 for empty) of the
    /// highest triangle there. For the sidebar's glyph.
    public struct Footprint: Sendable {
        public let width: Int
        public let height: Int
        public let cells: [UInt8]
    }

    public struct Timings: Sendable {
        public var inflate: Duration = .zero
        public var parse: Duration = .zero
        public var normals: Duration = .zero
        public var assemble: Duration = .zero
        public var total: Duration { inflate + parse + normals + assemble }
    }

    public let plateId: Int
    public let batches: [Batch]
    /// Bounds of every vertex; nil for an empty plate.
    public let bounds: (min: SIMD3<Float>, max: SIMD3<Float>)?
    public let triangleCount: Int
    public let footprint: Footprint
    public let timings: Timings
}

/// Builds plates' geometry from a project, reading only the mesh parts a
/// plate uses, and keeping each parsed part and each mesh's normals for
/// later plates. Calls are serialised with a lock; build from one
/// background queue.
public final class PlateBuilder: @unchecked Sendable {
    public let archive: ZipReader
    public let project: PlateProject
    /// Faces meeting at a vertex at less than this angle share a smoothed
    /// normal; sharper edges stay crisp.
    public var creaseAngleDegrees: Float = 30

    private let lock = NSLock()
    private var files: [String: ModelFile] = [:]
    private var normals: [String: [Float]] = [:]
    private var timings = PlateGeometry.Timings()

    public init(archive: ZipReader, project: PlateProject) {
        self.archive = archive
        self.project = project
    }

    public func build(plateId: Int) -> PlateGeometry {
        lock.lock()
        defer { lock.unlock() }
        timings = .init()
        let plate = project.plates.first { $0.id == plateId }
        let origin = project.plateOrigin(plateId)
        let clock = ContinuousClock()

        // One flat position and normal array per filament, by slot.
        var slotOf: [Int: Int] = [:]
        var slotFilament: [Int] = []
        var positions: [[Float]] = []
        var normalsOut: [[Float]] = []
        func slot(_ filament: Int) -> Int {
            if let s = slotOf[filament] { return s }
            slotOf[filament] = positions.count
            slotFilament.append(filament)
            positions.append([])
            normalsOut.append([])
            return positions.count - 1
        }
        var lo = SIMD3<Float>(repeating: .infinity)
        var hi = SIMD3<Float>(repeating: -.infinity)
        var triangles = 0
        let bed = (min: project.bed.minimum, max: project.bed.maximum)
        let cellsAcross = 48
        let span = max(bed.max.x - bed.min.x, bed.max.y - bed.min.y)
        let cell = span / Double(cellsAcross)
        let fw = max(1, Int(((bed.max.x - bed.min.x) / cell).rounded(.up)))
        let fh = max(1, Int(((bed.max.y - bed.min.y) / cell).rounded(.up)))
        var footprint = [UInt8](repeating: 0, count: fw * fh)
        var footprintZ = [Float](repeating: -.infinity, count: fw * fh)

        for itemIndex in plate?.items ?? [] {
            let item = project.root.items[itemIndex]
            let settings = project.objects[item.objectId]
            var placed: [(path: String, objectId: Int, part: Int, transform: simd_double4x4)] = []
            collect(path: project.rootPath, objectId: item.objectId, transform: item.transform, part: nil, depth: 0, into: &placed)
            for piece in placed {
                if settings?.hiddenParts.contains(piece.part) == true { continue }
                guard let mesh = file(piece.path)?.objects[piece.objectId]?.mesh, mesh.triangleCount > 0 else { continue }
                let key = "\(piece.path)#\(piece.objectId)"
                let corner: [Float]
                if let cached = normals[key] {
                    corner = cached
                } else {
                    let t0 = clock.now
                    corner = Self.creaseNormals(mesh, cosLimit: cos(creaseAngleDegrees * .pi / 180))
                    normals[key] = corner
                    timings.normals += clock.now - t0
                }

                let t0 = clock.now
                let defaultExtruder = settings?.partExtruders[piece.part] ?? settings?.extruder ?? 1
                var m = piece.transform
                m.columns.3.x -= origin.x
                m.columns.3.y -= origin.y
                let mf = simd_float4x4(
                    SIMD4<Float>(m.columns.0), SIMD4<Float>(m.columns.1), SIMD4<Float>(m.columns.2), SIMD4<Float>(m.columns.3))
                let linear = simd_float3x3(
                    SIMD3(mf.columns.0.x, mf.columns.0.y, mf.columns.0.z),
                    SIMD3(mf.columns.1.x, mf.columns.1.y, mf.columns.1.z),
                    SIMD3(mf.columns.2.x, mf.columns.2.y, mf.columns.2.z))
                let normalMatrix = linear.inverse.transpose
                let mirrored = linear.determinant < 0
                let tCount = mesh.triangleCount
                triangles += tCount

                // Transform each vertex once.
                var world = [SIMD3<Float>](repeating: .zero, count: mesh.vertexCount)
                mesh.positions.withUnsafeBufferPointer { pos in
                    for v in 0 ..< mesh.vertexCount {
                        let p = SIMD4<Float>(pos[3 * v], pos[3 * v + 1], pos[3 * v + 2], 1)
                        let w = mf * p
                        world[v] = SIMD3(w.x, w.y, w.z)
                        lo = simd_min(lo, world[v])
                        hi = simd_max(hi, world[v])
                    }
                }

                // Paint state -> slot; state 0 is the part's own filament.
                let slots = (0 ..< 20).map { slot($0 == 0 ? defaultExtruder : $0) }
                let order = mirrored ? [0, 2, 1] : [0, 1, 2]
                for s in Set(slots) {
                    positions[s].reserveCapacity(positions[s].count + tCount * 9)
                    normalsOut[s].reserveCapacity(normalsOut[s].count + tCount * 9)
                }
                mesh.indices.withUnsafeBufferPointer { idx in
                    corner.withUnsafeBufferPointer { cn in
                        for t in 0 ..< tCount {
                            let s = slots[min(Int(mesh.paint[t]), 19)]
                            var top: Float = -.infinity
                            var centroid = SIMD3<Float>.zero
                            for k in order {
                                let v = Int(idx[3 * t + k])
                                let p = v < world.count ? world[v] : .zero
                                positions[s].append(p.x)
                                positions[s].append(p.y)
                                positions[s].append(p.z)
                                let c = 9 * t + 3 * k
                                let n = simd_normalize(normalMatrix * SIMD3(cn[c], cn[c + 1], cn[c + 2]))
                                let safe = n.x.isFinite ? n : SIMD3<Float>(0, 0, 1)
                                normalsOut[s].append(safe.x)
                                normalsOut[s].append(safe.y)
                                normalsOut[s].append(safe.z)
                                top = max(top, p.z)
                                centroid += p / 3
                            }
                            let cx = Int((Double(centroid.x) - bed.min.x) / cell)
                            let cy = Int((Double(centroid.y) - bed.min.y) / cell)
                            if cx >= 0, cy >= 0, cx < fw, cy < fh, top > footprintZ[cy * fw + cx] {
                                footprintZ[cy * fw + cx] = top
                                footprint[cy * fw + cx] = UInt8(clamping: slotFilament[s])
                            }
                        }
                    }
                }
                timings.assemble += clock.now - t0
            }
        }

        let t0 = clock.now
        if !project.laidOut, triangles > 0 {
            // A plain 3MF has no plate: centre it on the bed, standing on it.
            let centre = SIMD2<Float>(Float(bed.min.x + bed.max.x) / 2, Float(bed.min.y + bed.max.y) / 2)
            let shift = SIMD3<Float>(centre.x - (lo.x + hi.x) / 2, centre.y - (lo.y + hi.y) / 2, -lo.z)
            for s in positions.indices {
                positions[s].withUnsafeMutableBufferPointer { p in
                    var i = 0
                    while i < p.count {
                        p[i] += shift.x; p[i + 1] += shift.y; p[i + 2] += shift.z
                        i += 3
                    }
                }
            }
            lo += shift
            hi += shift
            footprint = [UInt8](repeating: 0, count: fw * fh)
        }
        let batches = slotOf.sorted { $0.key < $1.key }.compactMap { filament, s -> PlateGeometry.Batch? in
            guard !positions[s].isEmpty else { return nil }
            return PlateGeometry.Batch(
                filament: filament,
                colour: colour(filament),
                positions: positions[s].withUnsafeBytes { Data($0) },
                normals: normalsOut[s].withUnsafeBytes { Data($0) })
        }
        timings.assemble += clock.now - t0
        return PlateGeometry(
            plateId: plateId,
            batches: batches,
            bounds: triangles > 0 ? (lo, hi) : nil,
            triangleCount: triangles,
            footprint: .init(width: fw, height: fh, cells: footprint),
            timings: timings)
    }

    /// The colour filament `index` prints in: the project's, else a stand-in
    /// from a small palette.
    public func colour(_ index: Int) -> SIMD4<Float> {
        if let hex = project.filament(index)?.colour, let c = Self.colour(hex: hex) { return c }
        let palette: [SIMD4<Float>] = [
            SIMD4(0.80, 0.80, 0.82, 1), SIMD4(0.90, 0.45, 0.20, 1), SIMD4(0.25, 0.50, 0.85, 1),
            SIMD4(0.30, 0.70, 0.40, 1), SIMD4(0.85, 0.75, 0.25, 1), SIMD4(0.60, 0.35, 0.70, 1),
        ]
        return palette[(max(index, 1) - 1) % palette.count]
    }

    /// `#RRGGBB` or `#RRGGBBAA`.
    public static func colour(hex: String) -> SIMD4<Float>? {
        var s = Substring(hex.trimmingCharacters(in: .whitespaces))
        if s.hasPrefix("#") { s = s.dropFirst() }
        guard s.count == 6 || s.count == 8, let v = UInt64(s, radix: 16) else { return nil }
        let rgba = s.count == 6 ? (v << 8) | 0xFF : v
        func c(_ shift: UInt64) -> Float { Float((rgba >> shift) & 0xFF) / 255 }
        return SIMD4(c(24), c(16), c(8), c(0))
    }

    /// Resolves an object to its meshes: an object with a mesh is one piece;
    /// one with components is its components', each with its transform
    /// composed. `part` is the first-level component's object id, which is
    /// the part id the plate settings use.
    private func collect(
        path: String, objectId: Int, transform: simd_double4x4, part: Int?, depth: Int,
        into out: inout [(path: String, objectId: Int, part: Int, transform: simd_double4x4)]
    ) {
        guard depth < 16, let object = file(path)?.objects[objectId] else { return }
        if object.mesh != nil {
            out.append((path, objectId, part ?? objectId, transform))
        }
        for c in object.components {
            collect(
                path: c.path ?? path, objectId: c.objectId, transform: transform * c.transform,
                part: part ?? c.objectId, depth: depth + 1, into: &out)
        }
    }

    private func file(_ path: String) -> ModelFile? {
        if path == project.rootPath { return project.root }
        if let f = files[path] { return f }
        let clock = ContinuousClock()
        let t0 = clock.now
        guard let data = try? archive.read(path) else { return nil }
        let t1 = clock.now
        let parsed = ModelFile(parsing: data)
        timings.inflate += t1 - t0
        timings.parse += clock.now - t1
        files[path] = parsed
        return parsed
    }

    /// Per-corner normals (nine floats per triangle), smoothed across faces
    /// that meet at a vertex position within the crease angle, area-weighted.
    /// Vertices are matched by position through a spatial hash, so a mesh
    /// whose shared vertices are duplicated still smooths.
    public static func creaseNormals(_ mesh: ModelFile.Mesh, cosLimit: Float) -> [Float] {
        let vCount = mesh.vertexCount
        let tCount = mesh.triangleCount
        var out = [Float](repeating: 0, count: tCount * 9)
        guard tCount > 0 else { return out }

        // Vertex -> position group.
        var groupOf = [Int32](repeating: 0, count: vCount)
        var groups: [SIMD3<Int32>: Int32] = [:]
        groups.reserveCapacity(vCount)
        mesh.positions.withUnsafeBufferPointer { pos in
            for v in 0 ..< vCount {
                let key = SIMD3<Int32>(
                    Int32(clamping: Int((pos[3 * v] * 1e4).rounded())),
                    Int32(clamping: Int((pos[3 * v + 1] * 1e4).rounded())),
                    Int32(clamping: Int((pos[3 * v + 2] * 1e4).rounded())))
                if let g = groups[key] {
                    groupOf[v] = g
                } else {
                    let g = Int32(groups.count)
                    groups[key] = g
                    groupOf[v] = g
                }
            }
        }
        let gCount = groups.count

        // Face normals: area-weighted and unit.
        var area = [SIMD3<Float>](repeating: .zero, count: tCount)
        var unit = [SIMD3<Float>](repeating: .zero, count: tCount)
        func position(_ v: Int) -> SIMD3<Float> {
            guard v < vCount else { return .zero }
            return SIMD3(mesh.positions[3 * v], mesh.positions[3 * v + 1], mesh.positions[3 * v + 2])
        }
        for t in 0 ..< tCount {
            let a = position(Int(mesh.indices[3 * t]))
            let b = position(Int(mesh.indices[3 * t + 1]))
            let c = position(Int(mesh.indices[3 * t + 2]))
            let n = simd_cross(b - a, c - a)
            area[t] = n
            let len = simd_length(n)
            unit[t] = len > 0 ? n / len : .zero
        }

        // Group -> incident faces (CSR).
        var start = [Int32](repeating: 0, count: gCount + 1)
        for t in 0 ..< tCount {
            for k in 0 ..< 3 {
                let v = Int(mesh.indices[3 * t + k])
                if v < vCount { start[Int(groupOf[v]) + 1] += 1 }
            }
        }
        for g in 0 ..< gCount { start[g + 1] += start[g] }
        var fill = start
        var faces = [Int32](repeating: 0, count: Int(start[gCount]))
        for t in 0 ..< tCount {
            for k in 0 ..< 3 {
                let v = Int(mesh.indices[3 * t + k])
                guard v < vCount else { continue }
                let g = Int(groupOf[v])
                faces[Int(fill[g])] = Int32(t)
                fill[g] += 1
            }
        }

        for t in 0 ..< tCount {
            let own = unit[t]
            for k in 0 ..< 3 {
                let v = Int(mesh.indices[3 * t + k])
                var sum = SIMD3<Float>.zero
                if v < vCount, own != .zero {
                    let g = Int(groupOf[v])
                    for j in Int(start[g]) ..< Int(start[g + 1]) {
                        let f = Int(faces[j])
                        if simd_dot(unit[f], own) >= cosLimit { sum += area[f] }
                    }
                }
                let len = simd_length(sum)
                let n = len > 0 ? sum / len : (own != .zero ? own : SIMD3(0, 0, 1))
                out[9 * t + 3 * k] = n.x
                out[9 * t + 3 * k + 1] = n.y
                out[9 * t + 3 * k + 2] = n.z
            }
        }
        return out
    }
}
