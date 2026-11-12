import AppKit
import CadpolePlateView
import QuickLookThumbnailing
import os

private let log = Logger(subsystem: "io.github.mikelikespie.cadpole.mac", category: "plate-thumbnail")

/// A Finder icon for a 3MF project: a render of the first plate with
/// objects on it, the plate the preview opens on.
@objc(CadpolePlateThumbnailProvider)
final class ThumbnailProvider: QLThumbnailProvider {
    override func provideThumbnail(
        for request: QLFileThumbnailRequest, _ handler: @escaping (QLThumbnailReply?, Error?) -> Void
    ) {
        let size = request.maximumSize
        let pixels = CGSize(width: size.width * request.scale, height: size.height * request.scale)
        let image: NSImage
        do {
            image = try PlateSnapshot.image(url: request.fileURL, size: pixels)
        } catch {
            log.error("no plate thumbnail: \(String(describing: error), privacy: .public)")
            handler(nil, error)
            return
        }
        handler(QLThumbnailReply(contextSize: size) {
            image.draw(in: NSRect(origin: .zero, size: size))
            return true
        }, nil)
    }
}
