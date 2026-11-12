import AppKit
import CadpolePlates
import Metal
import SceneKit

/// A still of a project's first plate with objects on it, rendered
/// offscreen: the Finder thumbnail.
public enum PlateSnapshot {
    public static func image(url: URL, size: CGSize, dark: Bool = false) throws -> NSImage {
        let archive = try ZipReader(url: url)
        let project = try PlateProject(archive: archive)
        guard let plate = project.previewOrder.first else { throw PlateProject.Failure.nothingToShow }
        let geometry = PlateBuilder(archive: archive, project: project).build(plateId: plate.id)
        let scene = PlateScene(bed: project.bed)
        scene.apply(dark: dark)
        scene.show(PlateScene.makeModel(geometry), bounds: geometry.bounds, duration: 0)
        scene.frame(aspect: size.width / max(size.height, 1), animated: false)
        let renderer = SCNRenderer(device: MTLCreateSystemDefaultDevice(), options: nil)
        renderer.scene = scene.scene
        renderer.pointOfView = scene.camera
        scene.scene.background.contents = NSColor.clear
        return renderer.snapshot(atTime: 0, with: size, antialiasingMode: .multisampling4X)
    }
}
