// The shape of the feature tree, without SwiftUI.
//
// `Model.features` is a DAG: a feature names the features (or bodies) it
// consumed and the bodies it produced. The sidebar needs a tree, so each
// feature hangs under the FIRST feature it consumes and the other DAG edges
// are dropped -- the tree is a view of the script, not something the user
// edits, so that loss is presentational. Losing a feature is not:
// every feature in the model is in the outline exactly once.
//
// Lives in CadpoleKit, not CadpoleUI, so the rule runs in the Linux tests.

import Foundation
import CadpoleProto

public struct FeatureOutline: Equatable, Sendable {
    /// How deep the outline nests. A consume chain is as long as the script's
    /// loop (forty sequential unions make a chain forty deep); past this depth
    /// the chain continues as siblings of the last nested level instead of
    /// nesting further, so nothing is lost and the sidebar stays readable.
    public static let defaultMaxDepth = 32

    /// Indices into `features`, top level first, in model order.
    public let roots: [Int]
    /// `children[i]`: indices of the features shown under feature `i`.
    public let children: [[Int]]

    public init(features: [Cadpole_V1_Feature], maxDepth: Int = defaultMaxDepth) {
        let n = features.count
        var indexOf: [String: Int] = [:]
        for (i, f) in features.enumerated() where indexOf[f.id] == nil {
            indexOf[f.id] = i
        }
        var producerOfBody: [String: Int] = [:]
        for (i, f) in features.enumerated() {
            for out in f.outputs where producerOfBody[out] == nil {
                producerOfBody[out] = i
            }
        }

        // The logical parent: the first input that names another feature,
        // either directly or through a body it produced.
        var parent = [Int?](repeating: nil, count: n)
        var logicalChildren = [[Int]](repeating: [], count: n)
        for (i, f) in features.enumerated() {
            for input in f.inputs where !input.isEmpty {
                guard let p = producerOfBody[input] ?? indexOf[input], p != i else { continue }
                parent[i] = p
                logicalChildren[p].append(i)
                break
            }
        }

        // Lay out depth first from the roots. A feature that would land below
        // `maxDepth` is shown beside its parent instead (under the parent's
        // own display parent), so a long chain flattens rather than vanishing.
        var displayChildren = [[Int]](repeating: [], count: n)
        var roots: [Int] = []
        var placed = [Bool](repeating: false, count: n)
        let cap = Swift.max(1, maxDepth)

        func place(_ start: Int, under displayParent: Int?, depth: Int) {
            // Explicit stack: a chain thousands long must not recurse.
            var stack: [(feature: Int, displayParent: Int?, depth: Int)] = [(start, displayParent, depth)]
            while let (f, dp, d) = stack.popLast() {
                guard !placed[f] else { continue }
                placed[f] = true
                if let dp { displayChildren[dp].append(f) } else { roots.append(f) }
                // Children nest one deeper, unless that is past the cap: then
                // they join this feature's own level.
                let (childParent, childDepth) = d + 1 > cap ? (dp, d) : (Optional(f), d + 1)
                for c in logicalChildren[f].reversed() {
                    stack.append((c, childParent, childDepth))
                }
            }
        }

        for i in 0 ..< n where parent[i] == nil {
            place(i, under: nil, depth: 0)
        }
        // Anything not reached hangs off a cycle (a feature whose parent chain
        // loops back on itself). Real Evaluate output is acyclic, but a
        // malformed model must still show every feature: the first unplaced one
        // in model order becomes a root.
        for i in 0 ..< n where !placed[i] {
            place(i, under: nil, depth: 0)
        }

        self.roots = roots
        self.children = displayChildren
    }

    /// The deepest level any feature is shown at (roots are 0).
    public var depth: Int {
        var best = 0
        var stack = roots.map { ($0, 0) }
        while let (f, d) = stack.popLast() {
            best = Swift.max(best, d)
            stack.append(contentsOf: children[f].map { ($0, d + 1) })
        }
        return best
    }
}
