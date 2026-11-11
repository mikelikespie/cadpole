"""Generating SwiftProtobuf message types, and keeping the committed copies honest.

Cadpole **commits** the generated `*.pb.swift` under `src/apple/CadpoleProto/` rather
than producing them with `swift_proto_library`, because
`swift_proto_library` builds SwiftProtobuf's compiler plugin as a host tool, and
under rules_swift 3.6.1 + Bazel 9 the exec configuration resolves no Apple
platform, so that compile gets `-target arm64-apple-macos` with no version and
an empty `-sdk`.

Committed generated code rots, so this file supplies the other half: a rule that
regenerates the same files into `bazel-bin`, which
`//src/apple/CadpoleProto:pb_swift_diff_test` compares against the committed ones.
Regenerate with `bazel run //src/apple/CadpoleProto:regenerate`.

The input is the `FileDescriptorSet` that `//build_support/proto:defs.bzl`
already builds with `--include_imports`, so protoc needs no `-I` flags and no
copy of the well-known types: the descriptor set is a complete, self-contained
pool.
"""

load("@protobuf//bazel/common:proto_info.bzl", "ProtoInfo")

_PROTO_TOOLCHAIN = "@protobuf//bazel/private:proto_toolchain_type"

def _swift_proto_srcs_impl(ctx):
    descriptor_sets = ctx.files.descriptor_set
    if len(descriptor_sets) != 1:
        fail("descriptor_set must produce exactly one file, got %s" % descriptor_sets)
    descriptor_set = descriptor_sets[0]

    proto_info = ctx.attr.proto[ProtoInfo]
    names = [f.short_path for f in proto_info.direct_sources]

    # `proto_library(strip_import_prefix = "/proto")` puts the sources under a
    # `_virtual_imports` tree; the name protoc knows them by is the part after
    # it, which is also the name inside the descriptor set.
    marker = "/_virtual_imports/"
    protoc_names = []
    for n in names:
        if marker in n:
            rest = n.split(marker, 1)[1]
            protoc_names.append(rest.split("/", 1)[1])
        else:
            protoc_names.append(n)

    outs = []
    rels = []
    for n in protoc_names:
        rel = n[:-len(".proto")] + ".pb.swift"
        rels.append(rel)
        outs.append(ctx.actions.declare_file(ctx.attr.out_prefix + "/" + rel))

    # Every declared output shares one root; hand that root to --swift_out and
    # let protoc-gen-swift lay the package path out underneath it.
    root = outs[0].path[:-(len(rels[0]) + 1)]

    args = ctx.actions.args()
    args.add(descriptor_set, format = "--descriptor_set_in=%s")
    args.add(ctx.executable.plugin, format = "--plugin=protoc-gen-swift=%s")
    args.add("--swift_out=" + root)
    # Public: the app modules are separate Swift modules from CadpoleProto.
    args.add("--swift_opt=Visibility=Public")
    args.add_all(protoc_names)

    ctx.actions.run(
        executable = ctx.toolchains[_PROTO_TOOLCHAIN].proto.proto_compiler,
        arguments = [args],
        inputs = [descriptor_set],
        tools = [ctx.executable.plugin],
        outputs = outs,
        mnemonic = "SwiftProtoGen",
        progress_message = "Generating SwiftProtobuf types for %{label}",
        toolchain = _PROTO_TOOLCHAIN,
    )
    # One output group per file, named after its basename with the dots
    # flattened ("document_pb_swift"), so a diff_test can name a single
    # generated file. Rule outputs declared with declare_file have no label of
    # their own, and a whole-directory comparison would not say which file
    # drifted.
    groups = {}
    for i, rel in enumerate(rels):
        groups[rel.split("/")[-1].replace(".", "_")] = depset([outs[i]])

    return [
        DefaultInfo(files = depset(outs), runfiles = ctx.runfiles(files = outs)),
        OutputGroupInfo(**groups),
    ]

swift_proto_srcs = rule(
    implementation = _swift_proto_srcs_impl,
    attrs = {
        "descriptor_set": attr.label(
            mandatory = True,
            allow_files = True,
            doc = "A `proto_descriptor_set` target (built with --include_imports).",
        ),
        "out_prefix": attr.string(
            default = "_generated",
            doc = "Directory under the package to write into. Must differ from " +
                  "where the committed copies live, or Bazel sees a generated " +
                  "file colliding with a source file.",
        ),
        "plugin": attr.label(
            default = "@com_github_apple_swift_protobuf//:ProtoCompilerPlugin",
            cfg = "exec",
            executable = True,
            doc = "protoc-gen-swift, built from the SwiftProtobuf revision " +
                  "rules_swift bundles so the generated code matches the runtime.",
        ),
        "proto": attr.label(
            mandatory = True,
            providers = [ProtoInfo],
            doc = "The `proto_library` whose direct sources to generate.",
        ),
    },
    toolchains = [_PROTO_TOOLCHAIN],
    doc = "Generated `*.pb.swift` for a `proto_library`'s direct sources.",
)
