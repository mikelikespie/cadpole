// The feature tree's shape (CadpoleKit/FeatureOutline.swift): every feature
// in the model shows up exactly once, however long the consume chain.

import CadpoleKit
import CadpoleProto
import XCTest

final class FeatureOutlineTests: XCTestCase {
    private func feature(_ id: String, inputs: [String] = [], outputs: [String] = []) -> Cadpole_V1_Feature {
        var f = Cadpole_V1_Feature()
        f.id = id
        f.inputs = inputs
        f.outputs = outputs
        return f
    }

    /// Every index reachable from the roots, in display order.
    private func shown(_ o: FeatureOutline) -> [Int] {
        var out: [Int] = []
        var stack = Array(o.roots.reversed())
        while let f = stack.popLast() {
            out.append(f)
            stack.append(contentsOf: o.children[f].reversed())
        }
        return out
    }

    /// What `Evaluate` returns for `b = union(b, box(...))` forty times: each
    /// union's first input is the previous union.
    private func unionChain(_ count: Int) -> [Cadpole_V1_Feature] {
        var features = [feature("base", outputs: ["base#0"])]
        var previous = "base"
        for i in 0 ..< count {
            features.append(feature("add\(i)", outputs: ["add\(i)#\(2 * i + 1)"]))
            let id = "u\(i)"
            features.append(feature(id, inputs: [previous, "add\(i)"], outputs: ["\(id)#\(2 * i + 2)"]))
            previous = id
        }
        return features
    }

    func testAFortyLinkChainKeepsEveryFeature() {
        let features = unionChain(40)
        let outline = FeatureOutline(features: features)
        let ids = shown(outline).map { features[$0].id }
        XCTAssertEqual(ids.count, features.count, "missing: \(Set(features.map(\.id)).subtracting(ids).sorted())")
        XCTAssertEqual(Set(ids), Set(features.map(\.id)))
        XCTAssertTrue(ids.contains("u39"), "the last union owns the model's only body")
        XCTAssertLessThanOrEqual(outline.depth, FeatureOutline.defaultMaxDepth)
    }

    func testPastTheCapAChainContinuesAsSiblingsInOrder() {
        let features = [
            feature("a"),
            feature("b", inputs: ["a"]),
            feature("c", inputs: ["b"]),
            feature("d", inputs: ["c"]),
            feature("e", inputs: ["d"]),
        ]
        let outline = FeatureOutline(features: features, maxDepth: 2)
        XCTAssertEqual(outline.roots, [0])
        XCTAssertEqual(outline.children[0], [1])
        XCTAssertEqual(outline.children[1], [2, 3, 4], "c, d and e flatten at depth 2, in chain order")
        XCTAssertEqual(outline.depth, 2)
    }

    func testShallowTreesNestAsBefore() {
        let features = [
            feature("sk", outputs: ["sk#0"]),
            feature("ex", inputs: ["sk#0"], outputs: ["ex#1"]),
            feature("fil", inputs: ["ex#1"], outputs: ["fil#2"]),
            feature("other"),
        ]
        let outline = FeatureOutline(features: features)
        XCTAssertEqual(outline.roots, [0, 3])
        XCTAssertEqual(outline.children[0], [1])
        XCTAssertEqual(outline.children[1], [2])
        XCTAssertEqual(outline.children[2], [])
    }

    func testAParentCycleStillShowsItsFeatures() {
        let features = [
            feature("root"),
            feature("x", inputs: ["y"]),
            feature("y", inputs: ["x"]),
        ]
        let outline = FeatureOutline(features: features)
        XCTAssertEqual(Set(shown(outline)), [0, 1, 2])
        XCTAssertEqual(shown(outline).count, 3, "each feature once")
    }

    func testAThousandLinkChainDoesNotRecurse() {
        let features = unionChain(1000)
        let outline = FeatureOutline(features: features)
        XCTAssertEqual(shown(outline).count, features.count)
    }
}
