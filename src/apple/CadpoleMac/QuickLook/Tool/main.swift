// Loads a 3MF project the way the Quick Look preview does, without
// installing anything.
//
//   bazel run //src/apple/CadpoleMac/QuickLook:plate_viewer -- stats in.3mf
//   bazel run //src/apple/CadpoleMac/QuickLook:plate_viewer -- window in.3mf [--orbit SECONDS] [--snapshot out.png] [--light]
//   bazel run //src/apple/CadpoleMac/QuickLook:plate_viewer -- thumbnail in.3mf out.png
//
// `stats` prints each plate's triangle count and the time spent inflating,
// parsing, smoothing and assembling it. `window` opens the preview's view
// in a window; with --orbit it turns the camera for that long and prints
// frame times, then exits (after writing --snapshot, if given).
// `thumbnail` writes the Finder thumbnail's render at 512 px.

import CadpolePlates
import CadpolePlateView
import AppKit
import Foundation

func fail(_ message: String) -> Never {
    FileHandle.standardError.write(Data("plate_viewer: \(message)\n".utf8))
    exit(1)
}

func ms(_ d: Duration) -> String {
    let c = d.components
    return String(format: "%.1f ms", Double(c.seconds) * 1000 + Double(c.attoseconds) / 1e15)
}

setvbuf(stdout, nil, _IOLBF, 0)
let args = Array(CommandLine.arguments.dropFirst())
guard args.count >= 2 else { fail("usage: plate_viewer stats|window <project.3mf> [--orbit S] [--snapshot out.png]") }
let cwd = ProcessInfo.processInfo.environment["BUILD_WORKING_DIRECTORY"].map { URL(fileURLWithPath: $0, isDirectory: true) }
let url = URL(fileURLWithPath: args[1], relativeTo: cwd)
func option(_ name: String) -> String? {
    args.firstIndex(of: name).flatMap { $0 + 1 < args.count ? args[$0 + 1] : nil }
}

switch args[0] {
case "stats":
    let clock = ContinuousClock()
    let t0 = clock.now
    do {
        let archive = try ZipReader(url: url)
        let t1 = clock.now
        let project = try PlateProject(archive: archive)
        let t2 = clock.now
        print("central directory: \(ms(t1 - t0)), settings + root model: \(ms(t2 - t1))")
        print("bed: \(project.bed.outline.map { "\(Int($0.x))x\(Int($0.y))" }.joined(separator: ",")) \(project.bed.fromProject ? "" : "(fallback)")")
        let builder = PlateBuilder(archive: archive, project: project)
        for plate in project.previewOrder {
            let g = builder.build(plateId: plate.id)
            let t = g.timings
            let size = g.bounds.map { b in String(format: "%.0f..%.0f x %.0f..%.0f x %.0f..%.0f", b.min.x, b.max.x, b.min.y, b.max.y, b.min.z, b.max.z) } ?? "empty"
            print("\(plate.title): \(plate.objectCount) objects, \(g.triangleCount) triangles, \(g.batches.count) colours, bounds \(size)")
            print("  inflate \(ms(t.inflate)), parse \(ms(t.parse)), normals \(ms(t.normals)), assemble \(ms(t.assemble)); total \(ms(t.total))")
        }
        print("all plates: \(ms(clock.now - t0))")
    } catch {
        fail("\(error)")
    }
case "window":
    let orbit = option("--orbit").flatMap(Double.init)
    let snapshot = option("--snapshot").map { URL(fileURLWithPath: $0, relativeTo: cwd) }
    MainActor.assumeIsolated {
        PlateViewerWindow.run(url: url, orbitSeconds: orbit, snapshot: snapshot, light: args.contains("--light"))
    }
case "thumbnail":
    guard args.count >= 3 else { fail("usage: plate_viewer thumbnail <project.3mf> <out.png>") }
    let clock = ContinuousClock()
    let t0 = clock.now
    do {
        let image = try PlateSnapshot.image(url: url, size: CGSize(width: 512, height: 512))
        guard let tiff = image.tiffRepresentation, let rep = NSBitmapImageRep(data: tiff),
            let png = rep.representation(using: .png, properties: [:])
        else { fail("could not encode the render") }
        try png.write(to: URL(fileURLWithPath: args[2], relativeTo: cwd))
        print("thumbnail in \(ms(clock.now - t0))")
    } catch {
        fail("\(error)")
    }
default:
    fail("unknown command \(args[0])")
}
