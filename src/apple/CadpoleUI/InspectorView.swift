// The inspector: what is selected, what made it, and what is wrong with it.

import CadpoleKit
import CadpoleProto
import SwiftUI

struct InspectorView: View {
    @Bindable var model: DocumentModel

    var body: some View {
        Form {
            if let node = model.selectedNode {
                Section("Selection") {
                    LabeledContent("Name", value: node.title)
                    LabeledContent("Kind", value: kindName(node))
                    if !node.entityRef.featureID.isEmpty {
                        LabeledContent("From feature", value: node.entityRef.featureID)
                    }
                    if !node.entityRef.topoName.isEmpty {
                        LabeledContent("Topological name", value: node.entityRef.topoName)
                    }
                }
                if let body = body(for: node) {
                    Section("Measurements") {
                        if body.hasBbox {
                            LabeledContent("Size", value: sizeText(body.bbox))
                        }
                        LabeledContent("Faces", value: "\(body.topology.faces.count)")
                        LabeledContent("Edges", value: "\(body.topology.edges.count)")
                        LabeledContent("Vertices", value: "\(body.topology.vertices.count)")
                    }
                }
                if let feature = feature(for: node) {
                    if !feature.args.isEmpty {
                        Section("Arguments") {
                            ForEach(feature.args.sorted(by: { $0.key < $1.key }), id: \.key) { key, value in
                                LabeledContent(key, value: value)
                            }
                        }
                    }
                    if feature.hasSpan, feature.span.startLine > 0 {
                        Section("Source") {
                            LabeledContent("Line", value: "\(feature.span.path):\(feature.span.startLine)")
                                .monospaced()
                        }
                    }
                    if !feature.diagnostics.isEmpty {
                        Section("Diagnostics") {
                            ForEach(Array(feature.diagnostics.enumerated()), id: \.offset) { _, d in
                                DiagnosticRow(diagnostic: d)
                            }
                        }
                    }
                }
            } else if model.selection.count > 1 {
                ContentUnavailableView(
                    "\(model.selection.count) selected",
                    systemImage: "square.on.square",
                    description: Text("Select one item to inspect it.")
                )
            } else {
                ContentUnavailableView(
                    "Nothing selected",
                    systemImage: "cursorarrow.rays",
                    description: Text("Pick a feature or a body in the sidebar.")
                )
            }

            Section("Document") {
                LabeledContent("Revision", value: "\(model.session.revision)")
                LabeledContent("Units", value: model.session.displayUnit)
                LabeledContent("Evaluated in", value: String(format: "%.0f ms", model.model.evalMillis))
                LabeledContent("Engine", value: model.capabilities.contains(.localEvaluation) ? "In this app" : "On the server")
            }
            if !model.capabilities.contains(.assistant) {
                Section {
                    Label(
                        "Assistant unavailable \u{2014} this document is not on a server.",
                        systemImage: "bubble.left.slash"
                    )
                    .font(.callout)
                    .foregroundStyle(.secondary)
                }
            }
        }
        .formStyle(.grouped)
    }

    private func kindName(_ node: FeatureNode) -> String {
        switch node.kind {
        case .body: return "Body"
        case .feature(let kind): return kind.capitalized
        }
    }

    private func body(for node: FeatureNode) -> Cadpole_V1_Body? {
        guard case .body = node.kind else { return nil }
        return model.model.bodies.first { $0.id == node.entityRef.bodyID }
    }

    private func feature(for node: FeatureNode) -> Cadpole_V1_Feature? {
        model.model.features.first { $0.id == node.entityRef.featureID }
    }

    /// The bbox is millimetres, always (docs/starlark-api.md), so it is
    /// converted before it is labelled with the document's unit.
    private func sizeText(_ bbox: Cadpole_V1_BBox) -> String {
        DisplayUnit(model.session.displayUnit).size(bbox.size)
    }
}
