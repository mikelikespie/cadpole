"""`viewer_page` and `print_project`: what the site gets from the CLI.

One action per model runs `cadpole publish` on its script and writes the page
into `<name>/<model name>/`; the target's output is those directories. The CLI
is always the optimized build, whatever the mode of the build that asks: a
debug boolean is about ten times slower, and the turbojet alone would take
minutes.
"""

load("//build_support/transitions:defs.bzl", _opt_transition = "opt_transition")

def _viewer_page_impl(ctx):
    out = ctx.actions.declare_directory(ctx.label.name)
    cadpole = ctx.attr.cadpole[0][DefaultInfo].files_to_run
    args = ctx.actions.args()
    args.add("publish")
    args.add(ctx.file.script)
    args.add("--title", ctx.attr.title)
    args.add("--units", ctx.attr.units)
    args.add("--home-url", "../../#" + ctx.attr.back)
    args.add("--home-label", "Cadpole")
    args.add_all(ctx.attr.publish_args)
    args.add("--out-dir", out.path)
    ctx.actions.run(
        executable = cadpole,
        arguments = [args],
        inputs = [ctx.file.script] + ctx.files.data,
        outputs = [out],
        mnemonic = "CadpolePublish",
        progress_message = "Publishing %{label}",
        # The CLI keeps a cache under the home directory; a build action has none.
        env = {"HOME": "/nonexistent", "XDG_CACHE_HOME": out.path + ".cache"},
    )
    return [DefaultInfo(files = depset([out]))]

viewer_page = rule(
    implementation = _viewer_page_impl,
    attrs = {
        "script": attr.label(allow_single_file = [".star"], mandatory = True),
        "data": attr.label_list(allow_files = True, doc = "Scripts it loads and the assets it reads."),
        "title": attr.string(mandatory = True),
        "back": attr.string(mandatory = True, doc = "The section of the site the page's home link returns to."),
        "publish_args": attr.string_list(),
        "units": attr.string(default = "in", doc = "How the page writes a length."),
        "cadpole": attr.label(
            default = "//src/rust/cadpole-cli:cadpole",
            cfg = _opt_transition,
            executable = True,
        ),
        "_allowlist_function_transition": attr.label(
            default = "@bazel_tools//tools/allowlists/function_transition_allowlist",
        ),
    },
    doc = "One model's viewer page, as `cadpole publish` writes it.",
)

def _print_project_impl(ctx):
    out = ctx.actions.declare_file(ctx.label.name + ".3mf")
    cadpole = ctx.attr.cadpole[0][DefaultInfo].files_to_run
    args = ctx.actions.args()
    args.add("plate")
    args.add(ctx.file.script)
    args.add_all(ctx.attr.plate_args)
    args.add("--out", out.path)
    ctx.actions.run(
        executable = cadpole,
        arguments = [args],
        inputs = [ctx.file.script] + ctx.files.data,
        outputs = [out],
        mnemonic = "CadpolePlate",
        progress_message = "Plating %{label}",
        env = {"HOME": "/nonexistent", "XDG_CACHE_HOME": out.path + ".cache"},
    )
    return [DefaultInfo(files = depset([out]))]

print_project = rule(
    implementation = _print_project_impl,
    attrs = {
        "script": attr.label(allow_single_file = [".star"], mandatory = True),
        "data": attr.label_list(allow_files = True, doc = "Scripts it loads and the assets it reads."),
        "plate_args": attr.string_list(doc = "Further `cadpole plate` arguments: the printer, parameters, the layers to plate."),
        "cadpole": attr.label(
            default = "//src/rust/cadpole-cli:cadpole",
            cfg = _opt_transition,
            executable = True,
        ),
        "_allowlist_function_transition": attr.label(
            default = "@bazel_tools//tools/allowlists/function_transition_allowlist",
        ),
    },
    doc = "A model plated for printing, as `cadpole plate` writes it: a Bambu Studio project, every piece oriented and packed by the planner (docs/plating.md).",
)
