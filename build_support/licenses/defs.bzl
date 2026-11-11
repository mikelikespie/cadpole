"""Generating the attribution file as a Bazel artifact.

`THIRD-PARTY-NOTICES.md` is committed -- it is a document people read and a
thing Cadpole's own licence obliges us to ship -- but it is *derived*, from the
same three inventories the licence test enforces. Committed derived files rot,
so this rule supplies the other half: it regenerates the file into `bazel-bin`,
and `//build_support/licenses:attribution_test` diffs that against the
committed copy. `bazel run //build_support/licenses:attribution_update` copies
it back.

That is the same shape as `//src/apple/CadpoleProto:pb_swift_diff_test`, and for
the same reason: a generated artifact that no test compares is a claim nobody
is checking.

A rule rather than a `genrule` because the inputs are real labels and the tool
is a real `rust_binary`: `bazel build` then knows that touching `crates.tsv`
invalidates the notices, which is the whole point.
"""

def _attribution_impl(ctx):
    out = ctx.actions.declare_file(ctx.attr.out)
    args = ctx.actions.args()
    args.add(ctx.file.crates)
    args.add(ctx.file.npm)
    args.add(ctx.file.fonts)
    args.add(ctx.file.native)
    args.add(ctx.file.allowed)
    args.add(ctx.file.texts_index)
    args.add(out)

    # The licence texts are passed by path rather than read from a directory:
    # the generator matches each one against the file name texts.tsv records,
    # so a text that is listed but not supplied is a build error rather than a
    # silently missing section.
    texts = ctx.files.texts
    args.add_all(texts)

    ctx.actions.run(
        executable = ctx.executable.generator,
        arguments = [args],
        inputs = [
            ctx.file.crates,
            ctx.file.npm,
            ctx.file.fonts,
            ctx.file.native,
            ctx.file.allowed,
            ctx.file.texts_index,
        ] + texts,
        outputs = [out],
        mnemonic = "Attribution",
        progress_message = "Generating %s" % out.short_path,
    )
    return [DefaultInfo(files = depset([out]))]

attribution_file = rule(
    implementation = _attribution_impl,
    doc = "Renders the licence inventories into one attribution document.",
    attrs = {
        "allowed": attr.label(allow_single_file = True, mandatory = True),
        "crates": attr.label(allow_single_file = True, mandatory = True),
        "fonts": attr.label(allow_single_file = True, mandatory = True),
        "generator": attr.label(
            cfg = "exec",
            default = "//build_support/licenses:gen_attribution",
            executable = True,
        ),
        "native": attr.label(
            allow_single_file = True,
            mandatory = True,
            doc = "native.tsv: C and C++ code from Bazel modules.",
        ),
        "npm": attr.label(allow_single_file = True, mandatory = True),
        "out": attr.string(mandatory = True, doc = "Name of the generated file."),
        "texts": attr.label_list(
            allow_files = True,
            doc = "The canonical licence text files texts.tsv names.",
        ),
        "texts_index": attr.label(
            allow_single_file = True,
            mandatory = True,
            doc = "texts.tsv: SPDX id -> file, sha256, source.",
        ),
    },
)
