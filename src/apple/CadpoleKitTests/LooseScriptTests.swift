// Opening a loose `.star` file in the apps packs what it loads, as the CLI
// does (CadpoleKit/LooseScript.swift).

import Foundation
import CadpoleKit
import XCTest

final class LooseScriptTests: XCTestCase {
    private var root: URL!

    override func setUpWithError() throws {
        root = FileManager.default.temporaryDirectory.appendingPathComponent("cadpole-loose-\(UUID().uuidString)")
        try FileManager.default.createDirectory(at: root.appendingPathComponent("scene"), withIntermediateDirectories: true)
        try FileManager.default.createDirectory(at: root.appendingPathComponent("lib"), withIntermediateDirectories: true)
    }

    override func tearDownWithError() throws {
        try? FileManager.default.removeItem(at: root)
    }

    private func write(_ path: String, _ text: String) throws {
        try text.write(to: root.appendingPathComponent(path), atomically: true, encoding: .utf8)
    }

    private func paths(_ url: URL) throws -> [String] {
        try LooseScript.pack(contentsOf: url).scripts.map(\.path)
    }

    func testASceneThatLoadsASiblingBringsIt() throws {
        // examples/nightstand_scene.star does `load("nightstand.star", ...)`.
        try write("scene/scene.star", "load(\"nightstand.star\", \"stand\")\nstand()\n")
        try write("scene/nightstand.star", "def stand():\n    return box(1, 1, 1)\n")
        try write("scene/other.star", "box(2, 2, 2)\n")
        let doc = try LooseScript.pack(contentsOf: root.appendingPathComponent("scene/scene.star"))
        XCTAssertEqual(doc.name, "scene")
        XCTAssertEqual(doc.scripts.map(\.path), ["main.star", "nightstand.star", "other.star"])
        XCTAssertEqual(doc.scripts[0].source, "load(\"nightstand.star\", \"stand\")\nstand()\n")
    }

    func testAScriptWithoutLoadsIsJustItself() throws {
        try write("scene/plain.star", "box(1, 1, 1)\n")
        try write("scene/other.star", "box(2, 2, 2)\n")
        XCTAssertEqual(try paths(root.appendingPathComponent("scene/plain.star")), ["main.star"])
    }

    func testALoadOutsideTheDirectoryIsBundledTransitively() throws {
        try write("scene/a.star", "load(\"../lib/site.star\", \"house\")\nhouse\n")
        try write("lib/site.star", "load(\"dims.star\", \"W\")\nhouse = box(W, 1, 1)\n")
        try write("lib/dims.star", "W = 42\n")
        try write("scene/b.star", "load('@cadpole/site.star', 'ft')\n")
        XCTAssertEqual(
            try paths(root.appendingPathComponent("scene/a.star")),
            ["main.star", "b.star", "../lib/site.star", "../lib/dims.star"]
        )
    }

    func testAMissingLoadIsLeftForTheLoaderToReport() throws {
        try write("scene/a.star", "load(\"gone.star\", \"x\")\n")
        XCTAssertEqual(try paths(root.appendingPathComponent("scene/a.star")), ["main.star"])
    }

    func testALoadThatExistsButCannotBeReadIsAClearError() throws {
        // What a sandboxed Mac does to a file next to the one the user picked.
        try write("scene/a.star", "load(\"parts.star\", \"x\")\n")
        try write("scene/parts.star", "x = 1\n")
        let parts = root.appendingPathComponent("scene/parts.star")
        try FileManager.default.setAttributes([.posixPermissions: 0], ofItemAtPath: parts.path)
        defer { try? FileManager.default.setAttributes([.posixPermissions: 0o644], ofItemAtPath: parts.path) }
        guard (try? Data(contentsOf: parts)) == nil else { throw XCTSkip("running as a user who can read mode 000 files") }

        XCTAssertThrowsError(try LooseScript.pack(contentsOf: root.appendingPathComponent("scene/a.star"))) { error in
            XCTAssertEqual(error as? LooseScript.PackError, .unreadable(script: "a.star", files: ["parts.star"]))
        }
    }

    /// The sandboxed Mac usually cannot list the directory beside a picked
    /// file, and there `fileExists` says false for a file outside the grant,
    /// so "missing" and "unreadable" cannot be told apart. A directory whose
    /// entries can be read by path but not listed models that: what is read
    /// by path is packed, and anything else is reported, not left to the
    /// loader's "no script file named".
    func testALoadInADirectoryThatCannotBeListedIsReported() throws {
        try FileManager.default.createDirectory(at: root.appendingPathComponent("locked"), withIntermediateDirectories: true)
        try write("locked/a.star", "load(\"parts.star\", \"x\")\nload(\"other.star\", \"y\")\n")
        try write("locked/parts.star", "x = 1\n")
        let dir = root.appendingPathComponent("locked")
        try FileManager.default.setAttributes([.posixPermissions: 0o100], ofItemAtPath: dir.path)
        defer { try? FileManager.default.setAttributes([.posixPermissions: 0o755], ofItemAtPath: dir.path) }
        guard (try? FileManager.default.contentsOfDirectory(atPath: dir.path)) == nil else {
            throw XCTSkip("running as a user who can list a mode 100 directory")
        }

        XCTAssertThrowsError(try LooseScript.pack(contentsOf: dir.appendingPathComponent("a.star"))) { error in
            XCTAssertEqual(error as? LooseScript.PackError, .unreadable(script: "a.star", files: ["other.star"]))
        }
    }

    func testLoadTargetsAndResolution() {
        let src = "load(\"@cadpole/site.star\", \"ft\")\nload( '../lib/sheets.star', \"a\")\nx = reload(\"no.star\")\n# load(\"c.star\")\n"
        XCTAssertEqual(LooseScript.loadTargets(src), ["@cadpole/site.star", "../lib/sheets.star", "c.star"])
        XCTAssertEqual(LooseScript.resolveLoad(from: "main.star", path: "../lib/x.star"), "../lib/x.star")
        XCTAssertEqual(LooseScript.resolveLoad(from: "../lib/x.star", path: "y.star"), "../lib/y.star")
        XCTAssertEqual(LooseScript.resolveLoad(from: "a/b.star", path: "../c.star"), "c.star")
        XCTAssertEqual(LooseScript.resolveLoad(from: "main.star", path: "@cadpole/x.star"), "@cadpole/x.star")
        XCTAssertEqual(LooseScript.resolveLoad(from: "main.star", path: "//x/y.star"), "x/y.star")
    }
}
