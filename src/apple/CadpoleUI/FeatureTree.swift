// The feature tree, as a tree.
//
// `Model.features` is a DAG: a feature names the features it consumed and the
// bodies it produced. A `List` + `OutlineGroup` needs a tree, so each feature is
// attached to its FIRST producer and the duplicate edges of the DAG are dropped
// -- the tree is a view of the script, not a structure the user edits, so
// the loss is presentational. No feature is ever lost: a consume chain deeper
// than `FeatureOutline.defaultMaxDepth` continues as siblings at that depth
// (CadpoleKit/FeatureOutline.swift, tested on Linux).
//
// The row labels are the sentences the web UI announces and `cadpole selection
// --json` prints, so VoiceOver, the browser and an assistant share one
// vocabulary (docs/design.md).

import Foundation
import CadpoleKit
import CadpoleProto

public struct FeatureNode: Identifiable, Hashable, Sendable {
    public enum Kind: Hashable, Sendable {
        case feature(kind: String)
        case body
    }

    public var id: String
    public var title: String
    public var kind: Kind
    public var entityRef: Cadpole_V1_EntityRef
    public var children: [FeatureNode]?
    /// What VoiceOver reads, and what the inspector's header repeats.
    public var accessibilityLabel: String
    public var hasError: Bool

    public var systemImage: String {
        switch kind {
        case .body: return "cube"
        case .feature(let k):
            switch k {
            case "sketch": return "scribble"
            case "extrude": return "arrow.up.square"
            case "revolve": return "arrow.triangle.2.circlepath"
            case "union", "difference", "intersection": return "square.on.square"
            case "fillet", "chamfer": return "square.righthalf.filled"
            default: return "square.stack.3d.up"
            }
        }
    }
}

public struct FeatureTree {
    public let roots: [FeatureNode]
    private let index: [String: FeatureNode]

    /// - Parameter unit: the document's `display_unit`. Sizes reach this type
    ///   in millimetres and are spoken in the document's unit, the same way
    ///   the web UI prints them.
    public init(model: Cadpole_V1_Model, unit: DisplayUnit = .mm) {
        var bodiesByFeature: [String: [Cadpole_V1_Body]] = [:]
        for body in model.bodies {
            bodiesByFeature[body.featureID, default: []].append(body)
        }

        // The shape (who nests under whom, flattening past the depth cap and
        // surviving a malformed parent cycle) is CadpoleKit's FeatureOutline,
        // so the rule that every feature appears exactly once is tested on
        // Linux.
        let features = model.features
        let outline = FeatureOutline(features: features)

        var index: [String: FeatureNode] = [:]

        // Recursion depth is bounded by FeatureOutline.defaultMaxDepth.
        func node(for featureIndex: Int) -> FeatureNode {
            let feature = features[featureIndex]
            var children: [FeatureNode] = outline.children[featureIndex].map { node(for: $0) }
            children += (bodiesByFeature[feature.id] ?? []).map { body -> FeatureNode in
                var ref = Cadpole_V1_EntityRef()
                ref.featureID = feature.id
                ref.bodyID = body.id
                ref.kind = .body
                let name = body.name.isEmpty ? body.id : body.name
                let node = FeatureNode(
                    id: "body:" + body.id,
                    title: name,
                    kind: .body,
                    entityRef: ref,
                    children: nil,
                    accessibilityLabel: Self.bodyLabel(body, name: name, unit: unit),
                    hasError: false
                )
                index[node.id] = node
                return node
            }

            var ref = Cadpole_V1_EntityRef()
            ref.featureID = feature.id
            ref.kind = .feature
            let title = feature.name.isEmpty ? feature.id : feature.name
            let node = FeatureNode(
                id: "feature:" + feature.id,
                title: title,
                kind: .feature(kind: feature.kind),
                entityRef: ref,
                children: children.isEmpty ? nil : children,
                accessibilityLabel: Self.featureLabel(feature, title: title),
                hasError: feature.diagnostics.contains { $0.severity == .error }
            )
            index[node.id] = node
            return node
        }

        self.roots = outline.roots.map { node(for: $0) }
        self.index = index
    }

    public func node(id: String) -> FeatureNode? { index[id] }

    static func featureLabel(_ feature: Cadpole_V1_Feature, title: String) -> String {
        var parts = ["\(feature.kind) \(title)"]
        if feature.hasSpan, feature.span.startLine > 0 {
            parts.append("from line \(feature.span.startLine)")
        }
        if feature.suppressed { parts.append("suppressed") }
        let errors = feature.diagnostics.filter { $0.severity == .error }.count
        if errors > 0 { parts.append("\(errors) error\(errors == 1 ? "" : "s")") }
        return parts.joined(separator: ", ")
    }

    static func bodyLabel(_ body: Cadpole_V1_Body, name: String, unit: DisplayUnit) -> String {
        var parts = ["body \(name)"]
        if body.hasBbox {
            // Millimetres in, the document's unit out.
            parts.append(unit.spokenSize(body.bbox.size))
        }
        let faces = body.topology.faces.count
        if faces > 0 { parts.append("\(faces) face\(faces == 1 ? "" : "s")") }
        if !body.visible { parts.append("hidden") }
        return parts.joined(separator: ", ")
    }
}

extension Cadpole_V1_BBox {
    public var size: Cadpole_V1_Vec3 {
        var v = Cadpole_V1_Vec3()
        v.x = max.x - min.x
        v.y = max.y - min.y
        v.z = max.z - min.z
        return v
    }
}
