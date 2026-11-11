// The viewport.
//
// A server- or locally-rendered PNG from `Render`, full-bleed, with a floating
// Liquid Glass cluster for the named views. A live wgpu/Metal viewport
// (`CAMetalLayer`, id-buffer picking, a gesture map) would replace only
// this file, because everything above it is written against `DocumentModel`.
//
// Two deliberate details from docs/apple-apps.md: the geometry is
// full-bleed with `.ignoresSafeArea()` and the controls float inside the safe
// area, and there is NO `.backgroundExtensionEffect()` on the viewport -- it
// mirrors and blurs the view under the safe area, which is right for a banner
// and wrong for geometry.

import CadpoleKit
import SwiftUI

#if canImport(AppKit)
    import AppKit
#endif
#if canImport(UIKit)
    import UIKit
#endif

struct ViewportView: View {
    @Bindable var model: DocumentModel
    @Namespace private var glass

    /// docs/views.md's named views, in the order the number keys use.
    private static let views: [(id: String, label: String, symbol: String)] = [
        ("iso", "Isometric", "cube"),
        ("front", "Front", "square"),
        ("top", "Top", "square.dashed"),
        ("right", "Right", "square.righthalf.filled"),
    ]

    var body: some View {
        ZStack {
            Color.clear
                .background(.background)
            image
                .accessibilityElement()
                .accessibilityLabel(summary)
            if case .failed(let message) = model.phase {
                failure(message)
            }
        }
        .ignoresSafeArea()
        .overlay(alignment: .bottom) { controls }
        .overlay(alignment: .topTrailing) { staleBadge }
    }

    @ViewBuilder
    private var image: some View {
        if let png = model.renderedPNG, let platformImage = PlatformImage(data: png) {
            platformImage.swiftUIImage
                .resizable()
                .aspectRatio(contentMode: .fit)
                .opacity(model.isStale ? 0.45 : 1)
                .animation(.easeOut(duration: 0.15), value: model.isStale)
        } else {
            ContentUnavailableView(
                "No render yet",
                systemImage: "cube.transparent",
                description: Text("The engine has not produced an image for this document.")
            )
        }
    }

    @ViewBuilder
    private var controls: some View {
        GlassEffectContainer(spacing: 12) {
            HStack(spacing: 12) {
                ForEach(Self.views, id: \.id) { view in
                    Button {
                        model.view = view.id
                    } label: {
                        Label(view.label, systemImage: view.symbol)
                            .labelStyle(.iconOnly)
                            .frame(width: 34, height: 34)
                    }
                    .buttonStyle(.plain)
                    .glassEffect(.regular.interactive(), in: .capsule)
                    .glassEffectID(view.id, in: glass)
                    .help(view.label)
                    .accessibilityLabel(view.label)
                    .accessibilityAddTraits(model.view == view.id ? [.isSelected] : [])
                }
            }
        }
        .padding(.bottom, 20)
        .safeAreaPadding(.horizontal, 16)
    }

    /// What the viewport is waiting for, if anything. The connection comes
    /// first: "Re-evaluating" over a stream that dropped an hour ago would
    /// hide that the document is no longer being followed.
    private var badge: (text: String, spoken: String, spinning: Bool)? {
        switch model.connection {
        case .degraded:
            return ("Reconnecting\u{2026}", "Reconnecting to the server", true)
        case .unauthorized:
            // Shown briefly: the app's ServerLoginWatch forgets the token and returns to
            // the connect screen, which asks for the new URL.
            return ("Token expired", "The server rotated its token; returning to the connect screen", false)
        default:
            return model.isStale ? ("Re-evaluating", "Re-evaluating the model", true) : nil
        }
    }

    @ViewBuilder
    private var staleBadge: some View {
        if let badge {
            HStack(spacing: 6) {
                if badge.spinning {
                    ProgressView().controlSize(.small)
                } else {
                    Image(systemName: "exclamationmark.triangle.fill")
                        .foregroundStyle(.orange)
                }
                Text(badge.text)
            }
            .font(.caption)
            .padding(.horizontal, 12)
            .padding(.vertical, 6)
            .glassEffect(.regular, in: .capsule)
            .padding(16)
            .accessibilityLabel(badge.spoken)
        }
    }

    private func failure(_ message: String) -> some View {
        ContentUnavailableView {
            Label("Cannot evaluate", systemImage: "exclamationmark.triangle")
        } description: {
            Text(message)
        }
        .background(.regularMaterial)
    }

    /// The summary VoiceOver reads before exploring. Per-entity elements
    /// (docs/apple-apps.md) need a live viewport.
    private var summary: String {
        let bodies = model.model.bodies.count
        let name = Self.views.first { $0.id == model.view }?.label ?? model.view
        return "\(bodies) \(bodies == 1 ? "body" : "bodies"), \(name) view"
    }
}

/// `Image(nsImage:)` and `Image(uiImage:)` under one name, so the screens do
/// not carry `#if os(...)` of their own.
struct PlatformImage {
    #if canImport(AppKit)
        let native: NSImage
    #else
        let native: UIImage
    #endif

    init?(data: Data) {
        #if canImport(AppKit)
            guard let image = NSImage(data: data) else { return nil }
        #else
            guard let image = UIImage(data: data) else { return nil }
        #endif
        native = image
    }

    var swiftUIImage: Image {
        #if canImport(AppKit)
            Image(nsImage: native)
        #else
            Image(uiImage: native)
        #endif
    }
}
