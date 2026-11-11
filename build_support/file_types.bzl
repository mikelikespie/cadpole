"""Cadpole's two file types, each named once.

Everything that writes, reads or declares one of these files takes its
extension (and the document's UTType) from here: the Rust crates through
`cadpole_proto::file_types` (rustc_env below), the Swift code through the
generated `CadpoleFileTypes`, the apps' Info.plists through `file_types_expand`.
Docs and tests never spell the extensions out, so swapping them is an edit to
this file.
"""

# A packed document: a serialised cadpole.v1.Document (scripts, assets, state).
DOCUMENT_EXT = "cadpoledoc"
DOCUMENT_UTTYPE = "io.github.mikelikespie.cadpole.document"

# A baked model: an evaluated BakedModel, zlib-deflated (`cadpole bake`,
# `cadpole publish`, the web viewer's ?baked=).
BAKED_EXT = "cadpole"

FILE_TYPE_ENV = {
    "CADPOLE_BAKED_EXT": BAKED_EXT,
    "CADPOLE_DOCUMENT_EXT": DOCUMENT_EXT,
    "CADPOLE_DOCUMENT_UTTYPE": DOCUMENT_UTTYPE,
}

def file_types_expand(name, src, out):
    """Copies src to out with @DOCUMENT_EXT@, @DOCUMENT_UTTYPE@ and @BAKED_EXT@ filled in."""
    native.genrule(
        name = name,
        srcs = [src],
        outs = [out],
        cmd = "sed -e 's/@DOCUMENT_EXT@/%s/g' -e 's/@DOCUMENT_UTTYPE@/%s/g' -e 's/@BAKED_EXT@/%s/g' $< > $@" % (
            DOCUMENT_EXT,
            DOCUMENT_UTTYPE,
            BAKED_EXT,
        ),
    )
