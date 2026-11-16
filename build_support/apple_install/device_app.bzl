"""device_app: an iOS app built for a real device, unsigned, for the laptop to sign.

`bazel run //<app package>:install` (ios_install, ../install/README.md) needs
the app as a device build whatever flags the build was started with, so
this rule depends on the app through a transition:

  --ios_multi_cpus=arm64               one device slice. The transition REPLACES
                                       the list, so the sim_arm64 that
                                       test:apple-remote adds (--ios_multi_cpus
                                       accumulates on the command line;
                                       and in rc files) cannot join it.
  --compilation_mode=opt               an optimised build, as a person uses it.
  --features=disable_legacy_signing    rules_apple 4.5.3 refuses a device build
                                       of an ios_application with no
                                       provisioning_profile ("must be set for
                                       device builds"), except with this
                                       feature, which makes it sign nothing
                                       (apple/internal/codesigning_support.bzl,
                                       _should_sign_bundles). The profiles and
                                       the identity are the laptop's, known only
                                       when the installer runs; it signs there.

  //build_support/apple_install:for_install
                                       rules_apple also wants SOME profile in a
                                       device build, to embed and to check the
                                       entitlements against. An app (or
                                       extension) with no profile of its own
                                       takes ios_install_profile() in its
                                       provisioning_profile, which is the
                                       stand-in install/placeholder.mobileprovision
                                       with this flag. The installer replaces it.
                                       Apps with a checked-in App Store profile
                                       keep theirs: it is replaced the same way.

The output is the app's .ipa, as this target's files and runfiles.
"""

def _device_impl(settings, _attr):
    features = settings["//command_line_option:features"]
    if "disable_legacy_signing" not in features:
        features = features + ["disable_legacy_signing"]
    return {
        "//command_line_option:ios_multi_cpus": ["arm64"],
        "//command_line_option:compilation_mode": "opt",
        "//command_line_option:features": features,
        "//build_support/apple_install:for_install": True,
    }

_device = transition(
    implementation = _device_impl,
    inputs = ["//command_line_option:features"],
    outputs = [
        "//command_line_option:ios_multi_cpus",
        "//command_line_option:compilation_mode",
        "//command_line_option:features",
        "//build_support/apple_install:for_install",
    ],
)

def _device_app_impl(ctx):
    app = ctx.attr.app[0]
    ipas = [f for f in app[DefaultInfo].files.to_list() if f.extension == "ipa"]
    if len(ipas) != 1:
        fail("%s: expected one .ipa from %s, got %s" % (ctx.label, app.label, ipas))

    # A stable name in this package, so the installer finds it at a fixed
    # runfiles path whatever the app's own output is called.
    out = ctx.actions.declare_file(ctx.label.name + ".ipa")
    ctx.actions.symlink(output = out, target_file = ipas[0])
    return [DefaultInfo(
        files = depset([out]),
        runfiles = ctx.runfiles(files = [out]),
    )]

device_app = rule(
    implementation = _device_app_impl,
    doc = "An ios_application built for a device (arm64, optimised), not signed.",
    attrs = {
        "app": attr.label(cfg = _device, mandatory = True),
        "_allowlist_function_transition": attr.label(default = "@bazel_tools//tools/allowlists/function_transition_allowlist"),
    },
)
