"""Cadpole's protobuf build rules.

One macro, [`cadpole_proto_library`](#cadpole_proto_library), turns a set of `.proto`
files into everything Cadpole needs from them, so adding a proto file is a
one-line edit to `proto/cadpole/v1/BUILD.bazel`:

| Target | What it is |
| --- | --- |
| `<name>_proto` | `proto_library`, the canonical schema |
| `<name>_rust` | `rust_prost_library`: generated prost types (crate `<name>_rust`) |
| `<name>_descriptor_set` | a `FileDescriptorSet` **with source info** (comments) |
| `<name>_descriptor` | `rust_library` embedding those bytes (crate `<name>_descriptor`) |

The descriptor set is the interesting one. Cadpole's front-ends (MCP tools, the
HTTP `/api/ops/*` routes, `cadpole op`) are *generated at run time* from the
service descriptor, and they use the leading comments of rpcs and fields as
documentation (docs/design.md). Bazel's own `proto_library` descriptor
set (`ProtoInfo.direct_descriptor_set`) is produced by protoc **without**
`--include_source_info`, so those comments are stripped. `proto_descriptor_set`
below therefore runs the toolchain-resolved protoc itself with
`--include_source_info --include_imports`, which keeps both the comments and
the transitively imported files (`google/protobuf/descriptor.proto` among
them, which is what makes the custom options in `options.proto` resolvable at
run time).

No genrules: `proto_descriptor_set` and `descriptor_rust_src` are real
Starlark rules, hermetic and cross-platform, using the protoc that proto
toolchain resolution picks (the prebuilt one, per `.bazelrc`).
"""

load("@protobuf//bazel:proto_library.bzl", "proto_library")
load("@protobuf//bazel/common:proto_info.bzl", "ProtoInfo")
load("@rules_rust//rust:defs.bzl", "rust_library")
load("@rules_rust_prost//:defs.bzl", "rust_prost_library")

# Registered unconditionally by the protobuf module; resolves to the prebuilt
# protoc unless `--@protobuf//bazel/flags:prefer_prebuilt_protoc=false`.
_PROTO_TOOLCHAIN = "@protobuf//bazel/private:proto_toolchain_type"

def _include_flags(proto_info):
    """`-I` flags for protoc, most specific path first.

    protoc resolves an import against the first `-I` that contains it and then
    uses the remainder as the file's name, so a shorter path that prefixes a
    longer one must come last or files get the wrong package path. Sorting by
    descending length achieves that; `-I.` (the exec root) always comes last.
    """
    paths = [p for p in proto_info.transitive_proto_path.to_list() if p != "."]
    paths = sorted(paths, key = lambda p: -len(p))
    return ["-I" + p for p in paths] + ["-I."]

def _proto_descriptor_set_impl(ctx):
    proto_info = ctx.attr.proto[ProtoInfo]
    out = ctx.actions.declare_file(ctx.label.name + ".bin")

    args = ctx.actions.args()
    args.use_param_file(param_file_arg = "@%s", use_always = False)
    args.set_param_file_format("multiline")
    args.add_all(_include_flags(proto_info))

    # The two flags that Bazel's own descriptor-set action does not pass:
    # keep the comments, and keep the imported files so the pool is complete
    # and self-contained at run time.
    args.add("--include_source_info")
    args.add("--include_imports")
    args.add(out, format = "--descriptor_set_out=%s")
    args.add_all(proto_info.direct_sources)

    ctx.actions.run(
        executable = ctx.toolchains[_PROTO_TOOLCHAIN].proto.proto_compiler,
        arguments = [args],
        inputs = proto_info.transitive_sources,
        outputs = [out],
        mnemonic = "ProtoDescriptorSetWithSourceInfo",
        progress_message = "Generating descriptor set with source info for %{label}",
        toolchain = _PROTO_TOOLCHAIN,
    )
    return [DefaultInfo(files = depset([out]), runfiles = ctx.runfiles(files = [out]))]

proto_descriptor_set = rule(
    implementation = _proto_descriptor_set_impl,
    attrs = {
        "proto": attr.label(
            mandatory = True,
            providers = [ProtoInfo],
            doc = "The `proto_library` to describe.",
        ),
    },
    toolchains = [_PROTO_TOOLCHAIN],
    doc = """Serialized `FileDescriptorSet` for `proto`, *with* source info.

Unlike `ProtoInfo.direct_descriptor_set` this keeps `SourceCodeInfo` (so
leading comments survive into the binary) and includes every transitively
imported file, so the bytes alone are enough to build a complete
`prost_reflect::DescriptorPool`.""",
)

def _descriptor_rust_src_impl(ctx):
    bins = ctx.files.descriptor_set
    if len(bins) != 1:
        fail("descriptor_set must produce exactly one file, got %s" % bins)
    bin = bins[0]
    out = ctx.actions.declare_file(ctx.label.name + ".rs")

    # `include_bytes!` resolves relative to the .rs file. Both files are
    # declared in this package, so they land side by side in the same output
    # directory and a bare basename is correct in every sandbox.
    if bin.dirname != out.dirname:
        fail("descriptor set %s and generated source %s must share a directory" % (bin.path, out.path))

    ctx.actions.write(
        output = out,
        content = """//! Generated by //build_support/proto:defs.bzl. Do not edit.
//!
//! The `FileDescriptorSet` for `{label}`, compiled with `--include_source_info`
//! so rpc and field comments survive into the binary, and `--include_imports`
//! so the bytes build a complete descriptor pool on their own.

/// Serialized `google.protobuf.FileDescriptorSet`.
pub const FILE_DESCRIPTOR_SET: &[u8] = include_bytes!("{basename}");
""".format(label = ctx.attr.descriptor_set.label, basename = bin.basename),
    )
    return [DefaultInfo(files = depset([out]))]

descriptor_rust_src = rule(
    implementation = _descriptor_rust_src_impl,
    attrs = {
        "descriptor_set": attr.label(
            mandatory = True,
            allow_files = True,
            doc = "A `proto_descriptor_set` target (exactly one output file).",
        ),
    },
    doc = """A one-constant Rust source file embedding a descriptor set.

Split from `proto_descriptor_set` so the `.rs` can be a `rust_library` src
while the `.bin` is its `compile_data`; a single rule would hand both files to
`srcs`.""",
)

def cadpole_proto_library(
        name,
        srcs,
        deps = [],
        strip_import_prefix = "/proto",
        visibility = ["//visibility:public"]):
    """Everything Cadpole needs from a set of `.proto` files.

    Args:
      name: base name. Creates `<name>_proto` (proto_library), `<name>_rust`
        (prost types), `<name>_descriptor_set` (FileDescriptorSet with source
        info) and `<name>_descriptor` (a rust_library embedding it).
      srcs: the `.proto` files.
      deps: `proto_library` targets this one imports, e.g.
        `@protobuf//:timestamp_proto`.
      strip_import_prefix: import root. Defaults to `/proto`, so
        `proto/cadpole/v1/eval.proto` is imported as `cadpole/v1/eval.proto`.
      visibility: visibility for every generated target.
    """
    proto_library(
        name = name + "_proto",
        srcs = srcs,
        strip_import_prefix = strip_import_prefix,
        deps = deps,
        visibility = visibility,
    )

    rust_prost_library(
        name = name + "_rust",
        proto = ":" + name + "_proto",
        visibility = visibility,
    )

    proto_descriptor_set(
        name = name + "_descriptor_set",
        proto = ":" + name + "_proto",
        visibility = visibility,
    )

    descriptor_rust_src(
        name = name + "_descriptor_src",
        descriptor_set = ":" + name + "_descriptor_set",
    )

    rust_library(
        name = name + "_descriptor",
        srcs = [":" + name + "_descriptor_src"],
        compile_data = [":" + name + "_descriptor_set"],
        visibility = visibility,
    )
