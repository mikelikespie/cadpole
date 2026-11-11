"""Generating TypeScript/JavaScript message types for the web UI.

The browser talks to `cadpole serve` in the wire format, not in a second,
hand-maintained JSON shape of the same messages. These are the types
it does it with: `protoc` plus `protoc-gen-es` (protobuf-es v2), run as a
normal Bazel action from the same `FileDescriptorSet` every other front-end is
built from, so the schema has exactly one source.

Unlike the Swift types next door in `swift.bzl`, these are **generated at build
time and never committed**. The Swift ones are committed because
`swift_proto_library` cannot build SwiftProtobuf's compiler plugin under
rules_swift 3.6.1 + Bazel 9 (see that file); nothing is broken here --
`protoc-gen-es` is a Node program and Bazel's hermetic Node toolchain runs it
like any other tool -- so there is no reason to keep a copy in the tree and a
diff test to stop it rotting. Generated code that is never committed cannot
drift from its schema.

Output is `target=js+dts`: plain ESM JavaScript plus its `.d.ts`, so the
generated code needs no `tsc` pass of its own and drops straight into
`ts_project`'s deps, `esbuild`'s inputs and the Node dev server.
`import_extension=js` writes `./document_pb.js` rather than `./document_pb`,
which Node's ESM loader (the dev server) requires and bundlers accept.

The input is the descriptor set `//build_support/proto:defs.bzl` already builds
with `--include_imports`, so protoc needs no `-I` flags and no copy of the
well-known types; `protoc-gen-es` maps `google.protobuf.*` onto
`@bufbuild/protobuf/wkt` in the runtime package rather than generating it.
"""

load("@protobuf//bazel/common:proto_info.bzl", "ProtoInfo")

_PROTO_TOOLCHAIN = "@protobuf//bazel/private:proto_toolchain_type"

def _proto_names(proto_info):
    """The names protoc knows the direct sources by, as they are in the set.

    `proto_library(strip_import_prefix = "/proto")` puts the sources under a
    `_virtual_imports` tree; the name inside the descriptor set is the part
    after it.
    """
    marker = "/_virtual_imports/"
    names = []
    for f in proto_info.direct_sources:
        n = f.short_path
        if marker in n:
            rest = n.split(marker, 1)[1]
            names.append(rest.split("/", 1)[1])
        else:
            names.append(n)
    return names

def _es_proto_srcs_impl(ctx):
    descriptor_sets = ctx.files.descriptor_set
    if len(descriptor_sets) != 1:
        fail("descriptor_set must produce exactly one file, got %s" % descriptor_sets)
    descriptor_set = descriptor_sets[0]

    protoc_names = _proto_names(ctx.attr.proto[ProtoInfo])

    outs = []
    rels = []
    for n in protoc_names:
        base = n[:-len(".proto")] + "_pb"
        for ext in (".js", ".d.ts"):
            rels.append(base + ext)
            outs.append(ctx.actions.declare_file(ctx.attr.out_prefix + "/" + base + ext))

    # Every declared output shares one root; hand that root to --es_out and let
    # protoc-gen-es lay the package path out underneath it.
    root = outs[0].path[:-(len(rels[0]) + 1)]

    args = ctx.actions.args()
    args.add(descriptor_set, format = "--descriptor_set_in=%s")
    args.add(ctx.executable.plugin, format = "--plugin=protoc-gen-es=%s")
    args.add("--es_out=" + root)
    # js+dts, not ts: the generated code is already type-complete, so it needs
    # no tsc pass and can be a plain js_library. import_extension=js keeps the
    # emitted imports loadable by Node's ESM resolver.
    args.add("--es_opt=target=js+dts,import_extension=js")
    args.add_all(protoc_names)

    ctx.actions.run(
        executable = ctx.toolchains[_PROTO_TOOLCHAIN].proto.proto_compiler,
        arguments = [args],
        inputs = [descriptor_set],
        tools = [ctx.executable.plugin],
        outputs = outs,
        mnemonic = "EsProtoGen",
        progress_message = "Generating protobuf-es types for %{label}",
        toolchain = _PROTO_TOOLCHAIN,
        # protoc spawns the plugin as a child process, so it inherits this
        # action's environment -- and a rules_js launcher refuses to start
        # without knowing where the bin directory is.
        env = {"BAZEL_BINDIR": ctx.bin_dir.path},
    )

    return [DefaultInfo(files = depset(outs), runfiles = ctx.runfiles(files = outs))]

es_proto_srcs = rule(
    implementation = _es_proto_srcs_impl,
    attrs = {
        "descriptor_set": attr.label(
            mandatory = True,
            allow_files = True,
            doc = "A `proto_descriptor_set` target (built with --include_imports).",
        ),
        "out_prefix": attr.string(
            default = "gen",
            doc = "Directory under the package to write into.",
        ),
        "plugin": attr.label(
            mandatory = True,
            cfg = "exec",
            executable = True,
            doc = "protoc-gen-es, as a rules_js `js_binary` over the npm package.",
        ),
        "proto": attr.label(
            mandatory = True,
            providers = [ProtoInfo],
            doc = "The `proto_library` whose direct sources to generate.",
        ),
    },
    toolchains = [_PROTO_TOOLCHAIN],
    doc = "Generated protobuf-es `*_pb.js` + `*_pb.d.ts` for a `proto_library`.",
)
