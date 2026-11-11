// Reading what the user picked.
//
// A packed document file is a serialised `cadpole.v1.Document`; a `.star` file is a bare
// Starlark script, which is how most documents start life (`cadpole eval
// examples/bracket.star`). Both become a `Document` the engine can evaluate; a
// script is packed with the files it loads (CadpoleKit/LooseScript.swift).

import Foundation
import CadpoleKit
import CadpoleProto
import UniformTypeIdentifiers

public enum CadpoleDocumentFile {
    /// The exported type, declared in both apps' Info.plist. Its prefix is
    /// shared with the Keychain service, so it must not change after shipping.
    /// Set in build_support/file_types.bzl.
    public static let documentTypeIdentifier = CadpoleFileTypes.documentTypeIdentifier

    public static var documentType: UTType {
        UTType(exportedAs: documentTypeIdentifier, conformingTo: .data)
    }

    /// A loose `.star` script: imported in both apps' Info.plist, because the
    /// system has no type for the extension and the dynamic one it would
    /// otherwise invent conforms only to public.data.
    public static var starlarkType: UTType {
        UTType(importedAs: CadpoleFileTypes.starlarkTypeIdentifier, conformingTo: .script)
    }

    /// Every type the open panel offers.
    public static var readableTypes: [UTType] {
        [documentType, starlarkType, .plainText]
    }

    public enum ReadError: Error {
        case unreadable(String)
    }

    public static func read(contentsOf url: URL) throws -> Cadpole_V1_Document {
        // A document picked from Files or a sandboxed open panel needs its
        // security scope taken before the bytes can be read.
        let scoped = url.startAccessingSecurityScopedResource()
        defer { if scoped { url.stopAccessingSecurityScopedResource() } }

        if url.pathExtension.lowercased() == CadpoleFileTypes.documentExtension {
            let data = try Data(contentsOf: url)
            do {
                return try Cadpole_V1_Document(serializedData: data)
            } catch {
                throw ReadError.unreadable("\(url.lastPathComponent) is not a Cadpole document: \(error)")
            }
        }
        // A loose script brings what it `load()`s, as `cadpole eval` does.
        do {
            return try LooseScript.pack(contentsOf: url)
        } catch let error as LooseScript.PackError {
            throw ReadError.unreadable(error.description)
        }
    }

    /// A document with one script, the shape `cadpole eval` uses for a loose file.
    public static func script(named name: String, source: String) -> Cadpole_V1_Document {
        var file = Cadpole_V1_ScriptFile()
        file.path = "main.star"
        file.source = source

        var document = Cadpole_V1_Document()
        document.id = UUID().uuidString
        document.name = name
        document.schemaVersion = 1
        document.displayUnit = .mm
        document.scripts = [file]
        return document
    }

    /// The document a new window starts from, so the app has something to draw
    /// before anyone opens a file.
    public static func starter() -> Cadpole_V1_Document {
        script(
            named: "Untitled",
            source: """
            wall = param("wall", 2.4, min = 1.0, max = 6.0, unit = "mm")
            size = param("size", 42.0, min = 10.0, max = 120.0, unit = "mm")

            box(size, size, size)
            """
        )
    }
}
