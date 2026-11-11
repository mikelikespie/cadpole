"""Module extension declaring the sha256-pinned Quint Rust evaluator.

    quint = use_extension("//build_support/quint:extensions.bzl", "quint")
    use_repo(
        quint,
        "quint_evaluator_linux_x86_64",
        "quint_evaluator_linux_aarch64",
        "quint_evaluator_macos_aarch64",
        "quint_evaluator_macos_x86_64",
    )

Only the repository the `select()` in //build_support/quint:BUILD.bazel picks
is fetched, i.e. one ~1 MB download on the host that runs //spec:quint_test.
"""

load(":evaluator.bzl", "QUINT_EVALUATOR_ARCHIVES", "quint_evaluator")

def _quint_impl(_module_ctx):
    for platform, archive in QUINT_EVALUATOR_ARCHIVES.items():
        quint_evaluator(
            name = "quint_evaluator_" + platform.replace("-", "_"),
            triple = archive.triple,
            sha256 = archive.sha256,
        )

quint = module_extension(
    implementation = _quint_impl,
    doc = "The Rust evaluator matching the pinned @informalsystems/quint version.",
)
