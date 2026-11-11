// A loose `.star` file opened by itself, packed the way the CLI packs it
// (src/rust/cadpole-cli/src/input.rs `load`): the file becomes `main.star`;
// if it uses `load()`, every other `.star` in its directory comes along, and
// so does every file a bundled script loads from outside that directory,
// transitively, under the key the loader resolves it to.
//
// Assets (`./assets` beside the script) are not packed here yet: that needs
// the SHA-256 and kind sniffing `cadpole_assets` does.

import Foundation
import CadpoleProto

public enum LooseScript {
    public enum PackError: Error, Equatable, CustomStringConvertible {
        /// Files the script loads that exist but could not be read -- on a
        /// sandboxed Mac, files beside the one the user picked.
        case unreadable(script: String, files: [String])
        case notText(String)

        public var description: String {
            switch self {
            case .unreadable(let script, let files):
                return "\(script) loads \(files.joined(separator: ", ")), which Cadpole could not read "
                    + "(a sandboxed app may only read the file you picked). Serve the script with "
                    + "`cadpole serve` and connect to it, or open a packed document."
            case .notText(let name):
                return "\(name) is not UTF-8 text"
            }
        }
    }

    /// Read `url` and everything it loads into one document.
    public static func pack(contentsOf url: URL) throws -> Cadpole_V1_Document {
        return try packScripts(entry: url).document(named: url.deletingPathExtension().lastPathComponent)
    }

    struct Scripts {
        var files: [(path: String, source: String)]

        func document(named name: String) -> Cadpole_V1_Document {
            var document = Cadpole_V1_Document()
            document.id = UUID().uuidString
            document.name = name
            document.schemaVersion = 1
            document.displayUnit = .mm
            document.scripts = files.map { f in
                var file = Cadpole_V1_ScriptFile()
                file.path = f.path
                file.source = f.source
                return file
            }
            return document
        }
    }

    static func packScripts(entry url: URL) throws -> Scripts {
        let data = try Data(contentsOf: url)
        guard let entry = String(data: data, encoding: .utf8) else { throw PackError.notText(url.lastPathComponent) }
        var files: [(path: String, source: String)] = [("main.star", entry)]
        let dir = url.deletingLastPathComponent()
        let usesLoad = entry.contains("load(")

        // Sibling .star files only when the entry can load them; otherwise
        // unrelated scripts would clutter the document.
        // In the App Sandbox the directory beside a picked file can usually
        // not be listed, and there `fileExists` is false for files that are
        // there but outside the grant, so a failed listing is remembered.
        var listingFailed = false
        if usesLoad {
            let names: [String]
            do {
                names = try FileManager.default.contentsOfDirectory(atPath: dir.path)
            } catch {
                names = []
                listingFailed = true
            }
            for name in names.sorted() where name.hasSuffix(".star") && name != url.lastPathComponent {
                if let text = readText(dir.appendingPathComponent(name)) {
                    files.append((name, text))
                }
            }

            // Everything a bundled script loads from outside the directory,
            // transitively, under the key the loader resolves it to.
            var i = 0
            while i < files.count {
                let (key, src) = files[i]
                for target in loadTargets(src) where !target.hasPrefix("@") {
                    let k = resolveLoad(from: key, path: target)
                    if k.isEmpty || files.contains(where: { $0.path == k }) { continue }
                    if let text = readText(dir.appendingPathComponent(k)) {
                        files.append((k, text))
                    }
                }
                i += 1
            }
        }

        // A load that is not there is left for the loader to report, with its
        // line. One that IS there but could not be read is ours to explain:
        // the loader would only say "no script file named ...". When the
        // directory could not be listed, which is which cannot be told, so
        // every load that was not packed is reported as unreadable.
        var unreadable: [String] = []
        for (key, src) in files {
            for target in loadTargets(src) where !target.hasPrefix("@") {
                let k = resolveLoad(from: key, path: target)
                guard !k.isEmpty, !files.contains(where: { $0.path == k }), !unreadable.contains(k) else { continue }
                if listingFailed || FileManager.default.fileExists(atPath: dir.appendingPathComponent(k).path) {
                    unreadable.append(k)
                }
            }
        }
        if !unreadable.isEmpty {
            throw PackError.unreadable(script: url.lastPathComponent, files: unreadable)
        }
        return Scripts(files: files)
    }

    private static func readText(_ url: URL) -> String? {
        (try? Data(contentsOf: url)).flatMap { String(data: $0, encoding: .utf8) }
    }

    /// The first argument of every `load("...")` in `src`, as the CLI's
    /// `load_targets` reads it.
    public static func loadTargets(_ src: String) -> [String] {
        var out: [String] = []
        var rest = Substring(src)
        while let r = rest.range(of: "load(") {
            let before = rest[..<r.lowerBound].last
            rest = rest[r.upperBound...]
            if let c = before, c.isLetter || c.isNumber || c == "_" || c == "." { continue }
            let t = rest.drop(while: { $0.isWhitespace })
            guard let q = t.first, q == "\"" || q == "'" else { continue }
            let body = t.dropFirst()
            if let end = body.firstIndex(of: q) {
                out.append(String(body[..<end]))
            }
        }
        return out
    }

    /// `cadpole_script::resolve_load`: the key a `load(path)` written in the
    /// script `from` resolves to.
    public static func resolveLoad(from: String, path: String) -> String {
        if path.hasPrefix("@") { return path }
        var path = Substring(path)
        while path.hasPrefix("//") { path = path.dropFirst(2) }
        let base = from.lastIndex(of: "/").map { String(from[..<$0]) } ?? ""
        var parts = base.split(separator: "/").map(String.init).filter { !$0.isEmpty && $0 != "." }
        for p in path.split(separator: "/", omittingEmptySubsequences: false).map(String.init) {
            switch p {
            case "", ".": break
            case "..":
                if let last = parts.last, last != ".." { parts.removeLast() } else { parts.append("..") }
            default: parts.append(p)
            }
        }
        return parts.joined(separator: "/")
    }
}
