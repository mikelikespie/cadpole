"""ios_install for Cadpole's iOS app (docs/apple-apps.md)."""

load(":device_app.bzl", _device_app = "device_app")
load(":install.bzl", _ios_install = "ios_install", _ios_install_profile = "ios_install_profile")

device_app = _device_app
ios_install = _ios_install
ios_install_profile = _ios_install_profile
