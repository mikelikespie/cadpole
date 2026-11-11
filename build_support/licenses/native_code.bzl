"""Which external repositories' C and C++ code a Cadpole artifact links.

The licence policy covers what a lockfile lists: crates (Cargo.lock), npm
packages (pnpm-lock.yaml), and the fonts reviewed by hand. C and C++ code
that a *Bazel module* puts into a binary is in no lockfile -- HiGHS, the
first, came from the BCR -- so `native.tsv` records it, and this is what keeps
that record complete: an aspect walks a binary's dependency edges and names
every external repository that contributes a `cc_library` (or another C-family
library rule), and `license_test` fails when one is linked that `native.tsv`
does not record, or recorded that nothing links.

It follows the edges that put code into a binary -- `deps`, rules_rust's
`link_deps`, a test's `crate` -- and not `data`, which ships files rather than
linking them, nor toolchains, whose runtime pieces `gen_attribution.rs`'s
toolchain notices name by hand. A crate's own C code (a `-sys` crate's build
script) is a crate, and crates.tsv already covers it.
"""

NativeCodeInfo = provider(
    doc = "The canonical names of the external repositories whose C or C++ libraries a target links.",
    fields = {"repos": "depset of canonical repository names, e.g. `highs+`"},
)

_EDGES = ["deps", "implementation_deps", "link_deps", "crate", "dynamic_deps"]

_C_LIBRARIES = ["cc_library", "cc_import", "cc_shared_library", "objc_library"]

def _native_code_aspect_impl(target, ctx):
    direct = []
    repo = target.label.repo_name
    if repo and ctx.rule.kind in _C_LIBRARIES:
        direct.append(repo)
    transitive = []
    for edge in _EDGES:
        value = getattr(ctx.rule.attr, edge, None)
        if value == None:
            continue
        for dep in (value if type(value) == "list" else [value]):
            if type(dep) == "Target" and NativeCodeInfo in dep:
                transitive.append(dep[NativeCodeInfo].repos)
    return [NativeCodeInfo(repos = depset(direct, transitive = transitive))]

native_code_aspect = aspect(
    implementation = _native_code_aspect_impl,
    attr_aspects = _EDGES,
    doc = "Collects the external repositories whose C or C++ libraries a target links.",
)

def _native_code_report_impl(ctx):
    repos = {}
    for t in ctx.attr.targets:
        for r in t[NativeCodeInfo].repos.to_list():
            repos[r] = True
    out = ctx.actions.declare_file(ctx.label.name + ".txt")
    ctx.actions.write(out, "".join([r + "\n" for r in sorted(repos.keys())]))
    return [DefaultInfo(files = depset([out]))]

native_code_report = rule(
    implementation = _native_code_report_impl,
    doc = "Writes one line per external repository whose C or C++ code `targets` link, sorted.",
    attrs = {
        "targets": attr.label_list(
            aspects = [native_code_aspect],
            doc = "The artifacts Cadpole ships.",
        ),
    },
)
