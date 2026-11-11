// Cadpole on macOS: self-hosted *and* connected.
//
// Self-hosted links `cadpole-ffi` and evaluates in-process with no server;
// connected uses the same session API as the web UI and the phone. The user
// picks per window, and every screen below the picker is the same code, because
// both engines are `EngineClient`.
//
// Documents open through `.fileImporter` into a `WindowGroup` rather than a
// `DocumentGroup`: `ReadableDocument`/`WritableDocument` need the macOS 27 SDK.
// Not implemented: undo wired to revisions, export, messages and a live wgpu
// viewport.

import CadpoleKit
import CadpoleLocalEngine
import CadpoleProto
import CadpoleUI
import SwiftUI

@main
struct CadpoleMacApp: App {
    var body: some Scene {
        WindowGroup {
            RootView()
        }
        .commands {
            CommandGroup(replacing: .newItem) {
                Button("New Sketch Window") {
                    NSWorkspace.shared.open(URL(string: "cadpole://new")!)
                }
                .keyboardShortcut("n")
                .disabled(true)
                .help("New documents are not implemented yet")
            }
        }
    }
}

struct RootView: View {
    private enum Mode {
        case choosing
        case engine(any EngineClient, subtitle: String)
        case remote(RemoteEngine, ServerLogin)
    }

    @State private var mode: Mode = .choosing
    @State private var importing = false
    @State private var problem: String?

    var body: some View {
        Group {
            switch mode {
            case .choosing:
                chooser
            case .engine(let engine, let subtitle):
                DocumentScreen(engine: engine)
                    .navigationSubtitle(subtitle)
            case .remote(let engine, let login):
                // A rejected token (the server restarted) brings the window
                // back to the chooser, with the connect screen saying why.
                DocumentScreen(engine: engine)
                    .navigationSubtitle(login.address.host)
                    .modifier(ServerLoginWatch(engine: engine, login: login) { message in
                        problem = message
                        mode = .choosing
                    })
            }
        }
        .fileImporter(
            isPresented: $importing,
            allowedContentTypes: CadpoleDocumentFile.readableTypes,
            allowsMultipleSelection: false
        ) { result in
            switch result {
            case .success(let urls):
                guard let url = urls.first else { return }
                openLocally(url)
            case .failure(let error):
                problem = error.localizedDescription
            }
        }
    }

    @ViewBuilder
    private var chooser: some View {
        VStack(spacing: 24) {
            Image(systemName: "cube.transparent")
                .font(.system(size: 56))
                .foregroundStyle(.tint)
            Text("Cadpole")
                .font(.largeTitle.weight(.semibold))

            Grid(horizontalSpacing: 16, verticalSpacing: 16) {
                GridRow {
                    modeCard(
                        title: "Open a file",
                        detail: "Self-hosted: the engine runs in this app. No server, no assistant.",
                        systemImage: "folder"
                    ) { importing = true }

                    modeCard(
                        title: "New document",
                        detail: "Start from a small parametric script and evaluate it locally.",
                        systemImage: "doc.badge.plus"
                    ) { openStarter() }
                }
            }

            Divider().frame(maxWidth: 520)

            ConnectView { engine, login in
                problem = nil
                mode = .remote(engine, login)
            }
            .frame(maxWidth: 520, maxHeight: 320)

            if let problem {
                Label(problem, systemImage: "exclamationmark.triangle")
                    .foregroundStyle(.red)
            }
        }
        .padding(32)
        .frame(minWidth: 620, minHeight: 640)
    }

    private func modeCard(
        title: String,
        detail: String,
        systemImage: String,
        action: @escaping () -> Void
    ) -> some View {
        Button(action: action) {
            VStack(alignment: .leading, spacing: 8) {
                Image(systemName: systemImage).font(.title2)
                Text(title).font(.headline)
                Text(detail)
                    .font(.caption)
                    .foregroundStyle(.secondary)
                    .fixedSize(horizontal: false, vertical: true)
            }
            .frame(width: 230, alignment: .leading)
            .padding(16)
        }
        .buttonStyle(.plain)
        .background(.regularMaterial, in: .rect(cornerRadius: 14))
        .accessibilityLabel("\(title). \(detail)")
    }

    private func openStarter() {
        start(document: CadpoleDocumentFile.starter(), subtitle: "Self-hosted")
    }

    private func openLocally(_ url: URL) {
        do {
            start(document: try CadpoleDocumentFile.read(contentsOf: url), subtitle: url.lastPathComponent)
        } catch {
            problem = String(describing: error)
        }
    }

    private func start(document: Cadpole_V1_Document, subtitle: String) {
        do {
            let engine = try LocalEngine(document: document)
            mode = .engine(engine, subtitle: subtitle)
            problem = nil
        } catch LocalEngine.LaunchError.versionMismatch(let linked, let expected) {
            // ABI rule 4: fatal, not a warning.
            problem = "This build links engine \(linked) but its protocol types came from \(expected)."
        } catch {
            problem = String(describing: error)
        }
    }
}
