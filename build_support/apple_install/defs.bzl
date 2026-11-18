"""ios_install for Cadpole's iOS app: `bazel run //src/apple/CadpoleiOS:install`
puts it on your own iPhone or iPad, signed on the Mac with your development
identity (docs/apple-apps.md).

install.bzl says what the macro makes; device_app.bzl how the app is built for
a device; install.sh and provision.py are the installer itself.
"""

load(":device_app.bzl", _device_app = "device_app")
load(":install.bzl", _ios_install = "ios_install", _ios_install_profile = "ios_install_profile")

device_app = _device_app
ios_install = _ios_install
ios_install_profile = _ios_install_profile
