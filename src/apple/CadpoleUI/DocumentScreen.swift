// The first screen: sidebar (feature tree + parameters), detail (viewport),
// inspector (selection).
//
// `NavigationSplitView` + `.inspector` adapt themselves -- three columns when
// horizontally regular, a sheet when compact -- so this is one implementation
// for Mac, iPad and iPhone (docs/apple-apps.md).

import CadpoleKit
import CadpoleProto
import SwiftUI

public struct DocumentScreen: View {
    @State private var model: DocumentModel
    @State private var showsInspector = true
    /// Lets VoiceOver jump between errors without walking the tree.
    @Namespace private var diagnosticsRotor

    public init(engine: any EngineClient) {
        _model = State(initialValue: DocumentModel(engine: engine))
    }

    public var body: some View {
        NavigationSplitView {
            sidebar
                .navigationSplitViewColumnWidth(min: 260, ideal: 300, max: 420)
        } detail: {
            ViewportView(model: model)
                .inspector(isPresented: $showsInspector) {
                    InspectorView(model: model)
                        .inspectorColumnWidth(min: 240, ideal: 300, max: 420)
                }
                .toolbar {
                    ToolbarItem {
                        Button {
                            showsInspector.toggle()
                        } label: {
                            Label("Inspector", systemImage: "sidebar.trailing")
                        }
                        .help("Show or hide the inspector")
                    }
                }
        }
        .navigationTitle(model.session.documentName.isEmpty ? "Cadpole" : model.session.documentName)
        .task { await model.load() }
        .overlay {
            if case .loading = model.phase {
                ProgressView("Evaluating\u{2026}")
                    .controlSize(.large)
                    .padding(24)
                    .background(.regularMaterial, in: .rect(cornerRadius: 16))
            }
        }
    }

    @ViewBuilder
    private var sidebar: some View {
        List(selection: $model.selection) {
            Section("Features") {
                OutlineGroup(model.tree.roots, children: \.children) { node in
                    FeatureRow(node: node)
                }
            }
            if !model.model.params.isEmpty {
                Section("Parameters") {
                    ParameterList(model: model)
                }
            }
            if !model.model.diagnostics.isEmpty {
                Section("Diagnostics") {
                    ForEach(model.diagnosticEntries) { entry in
                        DiagnosticRow(diagnostic: entry.diagnostic)
                            .accessibilityRotorEntry(id: entry.id, in: diagnosticsRotor)
                    }
                }
            }
        }
        #if os(macOS)
        .listStyle(.sidebar)
        #else
        .listStyle(.insetGrouped)
        #endif
        .accessibilityRotor("Errors", entries: model.errors, entryLabel: \.message)
    }
}

struct FeatureRow: View {
    let node: FeatureNode

    var body: some View {
        Label {
            HStack(spacing: 6) {
                Text(node.title)
                if case .feature(let kind) = node.kind {
                    Text(kind)
                        .font(.caption)
                        .foregroundStyle(.secondary)
                }
                if node.hasError {
                    Image(systemName: "exclamationmark.triangle.fill")
                        .foregroundStyle(.red)
                }
            }
        } icon: {
            Image(systemName: node.systemImage)
        }
        .accessibilityLabel(node.accessibilityLabel)
    }
}

struct DiagnosticRow: View {
    let diagnostic: Cadpole_V1_Diagnostic

    var body: some View {
        Label {
            VStack(alignment: .leading, spacing: 2) {
                Text(diagnostic.message)
                    .font(.callout)
                if diagnostic.hasSpan, diagnostic.span.startLine > 0 {
                    Text("\(diagnostic.span.path):\(diagnostic.span.startLine)")
                        .font(.caption)
                        .foregroundStyle(.secondary)
                        .monospaced()
                }
            }
        } icon: {
            Image(systemName: icon)
                .foregroundStyle(tint)
        }
        .accessibilityLabel("\(severity): \(diagnostic.message)")
    }

    private var severity: String {
        switch diagnostic.severity {
        case .error: return "Error"
        case .warning: return "Warning"
        default: return "Note"
        }
    }

    private var icon: String {
        switch diagnostic.severity {
        case .error: return "xmark.octagon.fill"
        case .warning: return "exclamationmark.triangle.fill"
        default: return "info.circle"
        }
    }

    private var tint: Color {
        switch diagnostic.severity {
        case .error: return .red
        case .warning: return .orange
        default: return .secondary
        }
    }
}
