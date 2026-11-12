import AppKit
import CadpolePlateView
import QuickLookUI

/// The space-bar preview of a 3MF project: its plates on the printer's bed
/// in an orbitable 3D view, with a sidebar of plates when there are several.
/// It opens on the first plate with objects on it.
///
/// The project's settings are read before the handler is called (a few
/// ms), so an unreadable file fails here and Quick Look falls back to the
/// next previewer; the meshes load after, in the background.
@objc(CadpolePlatePreviewController)
final class PreviewViewController: NSViewController, QLPreviewingController {
    private let browser = PlateBrowserViewController()

    override var nibName: NSNib.Name? { nil }

    override func loadView() {
        view = NSView(frame: NSRect(x: 0, y: 0, width: 960, height: 640))
        preferredContentSize = NSSize(width: 960, height: 640)
        addChild(browser)
        browser.view.frame = view.bounds
        browser.view.autoresizingMask = [.width, .height]
        view.addSubview(browser.view)
    }

    func preparePreviewOfFile(at url: URL, completionHandler handler: @escaping (Error?) -> Void) {
        _ = view
        do {
            try browser.load(url: url)
            handler(nil)
        } catch {
            handler(error)
        }
    }
}
