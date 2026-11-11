"""Configuration transitions used across the repo.

`opt_files(name, src)` forwards the files of `src` built with
`--compilation_mode=opt`, whatever the outer build's mode. Used so the web
bundle always ships the optimized wasm engine (the debug build is 4x bigger
and far slower), without forcing `-c opt` on the rest of the build.

`opt_transition` is that transition, for other rules. It also turns off the
debug assertions that .bazelrc keeps on in third-party crates for debug and
test builds, exactly as `--config=opt` does, so what it builds is what
`--config=opt` builds.
"""

_PER_CRATE_RUSTC_FLAG = "@rules_rust//rust/settings:per_crate_rustc_flag"

# Spelled as in .bazelrc's `build:opt`, so both name the same configuration.
_NO_THIRD_PARTY_DEBUG_ASSERTIONS = "external/rules_rust++crate+crates__@-Cdebug-assertions=off"

def _opt_transition_impl(settings, _attr):
    flags = list(settings[_PER_CRATE_RUSTC_FLAG])
    if not flags or flags[-1] != _NO_THIRD_PARTY_DEBUG_ASSERTIONS:
        flags.append(_NO_THIRD_PARTY_DEBUG_ASSERTIONS)
    return {
        "//command_line_option:compilation_mode": "opt",
        _PER_CRATE_RUSTC_FLAG: flags,
    }

opt_transition = transition(
    implementation = _opt_transition_impl,
    inputs = [_PER_CRATE_RUSTC_FLAG],
    outputs = ["//command_line_option:compilation_mode", _PER_CRATE_RUSTC_FLAG],
)

_opt_transition = opt_transition

def _opt_files_impl(ctx):
    dep = ctx.attr.src[0]
    return [DefaultInfo(files = dep[DefaultInfo].files, runfiles = dep[DefaultInfo].default_runfiles)]

opt_files = rule(
    implementation = _opt_files_impl,
    attrs = {
        "src": attr.label(
            cfg = _opt_transition,
            mandatory = True,
            doc = "Target whose default outputs are rebuilt in opt mode.",
        ),
        "_allowlist_function_transition": attr.label(
            default = "@bazel_tools//tools/allowlists/function_transition_allowlist",
        ),
    },
    doc = "Forward `src`'s default outputs built with -c opt.",
)
