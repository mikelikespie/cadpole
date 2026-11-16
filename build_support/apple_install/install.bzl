"""ios_install: `bazel run //<pkg>:install` puts an iOS app on the owner's own
iPhones and iPads, signed on the Mac with its development identity
(../install/README.md).

    ios_install(
        name = "install",                       # also :doctor and :install_test
        app = "//path/to/app:App",
        bundle_id = "com.example.App",
        display_name = "App",
        entitlements = "//path/to/app:App.entitlements",
        extensions = {"com.example.App.Widgets": None},  # bundle id -> its entitlements file
        families = ["iphone", "ipad"],
    )

makes:

    :<name>              build the app for a device (unsigned), check, sign, install, start
    :<doctor_name>       the same checks, changing nothing, without building the app
    :<name>_ipa          install an app built elsewhere: -- --ipa FILE (no build here)
    :<device_app_name>   the unsigned device build (an .ipa)
    :<name>_test         a dry run of this app's settings on made-up devices (Linux)
    :<name>_mac_test     manual, macOS: the real device build inspected and ad hoc
                         signed on the farm's Mac (bazel test --config=apple-remote-macos)

An app or extension with no provisioning profile of its own (no App Store
profile checked in) needs `provisioning_profile = ios_install_profile()`
so rules_apple builds it for a device at all; see device_app.bzl.
"""

load("@platforms//host:constraints.bzl", "HOST_CONSTRAINTS")
load("@rules_shell//shell:sh_binary.bzl", "sh_binary")
load("@rules_shell//shell:sh_test.bzl", "sh_test")
load(":device_app.bzl", "device_app")

# True on a Linux host: no local Xcode, so a device build only works when the
# command names the farm's Xcode (--config=apple-remote, --config=device).
_NO_LOCAL_XCODE = "@platforms//os:linux" in HOST_CONSTRAINTS

_TOOLS = "//build_support/apple_install"

def ios_install_profile(default = None):
    """provisioning_profile for an app or extension that has none checked in:
    the stand-in when built for the installer, `default` otherwise."""
    return select({
        _TOOLS + ":for_install_build": _TOOLS + ":placeholder.mobileprovision",
        "//conditions:default": default,
    })

def ios_install(
        name,
        app,
        bundle_id,
        display_name,
        entitlements = None,
        extensions = {},
        families = ["iphone", "ipad"],
        min_os = "26.0",
        default_team = "",
        doctor_name = None,
        device_app_name = None,
        note = None,
        visibility = None):
    """The install, doctor and device-build targets for one iOS app.

    Args:
      name: the install target (`bazel run :<name>`).
      app: the ios_application.
      bundle_id: its bundle_id.
      display_name: what the installer calls it ("App is running on ...").
      entitlements: its entitlements file, or None.
      extensions: {bundle id: entitlements file or None} for each extension
        the app embeds. Each is signed with a profile for its own bundle id.
      families: "iphone", "ipad": the devices it may go on.
      min_os: its minimum_os_version.
      default_team: the developer team when the Mac does not say which.
      doctor_name: the checks-only target; "<name>_doctor" if not given.
      device_app_name: the device build; "<name>_device_app" if not given.
      note: something the owner must know before installing this app, shown
        by the checks as a question mark (for example, that a development
        build's push tokens are the sandbox ones).
      visibility: of the install and doctor targets.
    """
    doctor_name = doctor_name or name + "_doctor"
    device = device_app_name or name + "_device_app"
    pkg = native.package_name()
    for f in families:
        if f not in ("iphone", "ipad"):
            fail("ios_install %s: families are iphone and ipad, not %s" % (name, f))

    data = [_TOOLS + ":provision.py"]
    env = {
        "APPLE_INSTALL_BUNDLE_ID": bundle_id,
        "APPLE_INSTALL_CMD": "bazel run //%s:%s" % (pkg, name),
        "APPLE_INSTALL_DEFAULT_TEAM": default_team,
        "APPLE_INSTALL_FAMILIES": " ".join(families),
        "APPLE_INSTALL_MIN_OS": min_os,
        "APPLE_INSTALL_NAME": display_name,
        "APPLE_INSTALL_PROVISION_PY": "$(rootpath %s:provision.py)" % _TOOLS,
    }
    if note:
        env["APPLE_INSTALL_NOTE"] = note
    if entitlements:
        data.append(entitlements)
        env["APPLE_INSTALL_ENTITLEMENTS"] = "$(rootpath %s)" % entitlements
    ext = []
    for bid, ents in extensions.items():
        if ents:
            data.append(ents)
            ext.append("%s=$(rootpath %s)" % (bid, ents))
        else:
            ext.append(bid)
    if ext:
        env["APPLE_INSTALL_EXTENSIONS"] = " ".join(ext)

    device_app(
        name = device,
        app = app,
        tags = ["manual"],
        # Skipped where there is no Xcode (a Linux host that names none).
        target_compatible_with = select({
            _TOOLS + ":worker_xcode": [],
            "//conditions:default": ["@platforms//:incompatible"] if _NO_LOCAL_XCODE else [],
        }),
    )

    # Builds the app, signs it with this Mac's development identity, puts it
    # on the connected devices and starts it.
    install_env = dict(env)
    install_env["APPLE_INSTALL_IPA"] = "$(rootpath :%s)" % device
    sh_binary(
        name = name,
        srcs = [_TOOLS + ":install.sh"],
        data = data + [":" + device],
        env = install_env,
        tags = ["manual"],
        visibility = visibility,
    )

    # The same, for an app built elsewhere (the farm): no build here.
    #   bazel run //<pkg>:<name>_ipa -- --ipa ~/Downloads/<App>Device.ipa
    ipa_env = dict(env)
    ipa_env["APPLE_INSTALL_NEED_IPA"] = "1"
    sh_binary(
        name = name + "_ipa",
        srcs = [_TOOLS + ":install.sh"],
        data = data,
        env = ipa_env,
        visibility = visibility,
    )

    # The checks alone, changing nothing. It does not depend on the app, so it
    # runs, and says what is wrong, even where the app does not build yet.
    sh_binary(
        name = doctor_name,
        srcs = [_TOOLS + ":install.sh"],
        args = ["--dry-run"],
        data = data,
        env = env,
        visibility = visibility,
    )

    # This app's settings, as a dry run on made-up devices with no profiles:
    # the entitlements files read, the bundles named, the plan.
    test_env = dict(env)
    test_env["APPLE_INSTALL_SH"] = "$(rootpath %s:install.sh)" % _TOOLS
    test_env["APPLE_INSTALL_TESTDATA"] = "build_support/apple_install/testdata"
    sh_test(
        name = name + "_test",
        size = "small",
        srcs = [_TOOLS + ":app_test.sh"],
        data = data + [_TOOLS + ":install.sh", _TOOLS + ":testdata"],
        env = test_env,
    )

    # The real device build on the farm's Mac: arm64, the extensions inside,
    # not signed; ad hoc signed by the installer's own commands, with the
    # entitlements each bundle gets; the throwaway project Xcode builds.
    #   bazel test --config=apple-remote-macos //<pkg>:<name>_mac_test
    mac_env = dict(test_env)
    mac_env["APPLE_INSTALL_IPA"] = "$(rootpath :%s)" % device
    sh_test(
        name = name + "_mac_test",
        size = "small",
        srcs = [_TOOLS + ":mac_test.sh"],
        data = data + [_TOOLS + ":install.sh", _TOOLS + ":testdata", ":" + device],
        env = mac_env,
        tags = ["manual"],
        target_compatible_with = ["@platforms//os:macos"],
    )
