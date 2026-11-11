"""Module extension that declares the sha256-pinned Playwright browsers.

    playwright = use_extension("//build_support/playwright:extensions.bzl", "playwright")
    use_repo(
        playwright,
        "playwright_chromium_linux_x86_64",
        "playwright_chromium_macos_aarch64",
    )

The repositories are only fetched when the `select()` in
//build_support/playwright:BUILD.bazel picks them, i.e. one ~120 MB download on
the host that actually runs the tests.
"""

load(
    ":browsers.bzl",
    "CHROMIUM_ARCHIVES",
    "CHROMIUM_BROWSER_DIR",
    "CHROMIUM_BROWSER_VERSION",
    "playwright_browser",
)

def _playwright_impl(_module_ctx):
    for platform, archive in CHROMIUM_ARCHIVES.items():
        playwright_browser(
            name = "playwright_chromium_" + platform.replace("-", "_"),
            archive_path = archive.path,
            browser_dir = CHROMIUM_BROWSER_DIR,
            browser_version = CHROMIUM_BROWSER_VERSION,
            sha256 = archive.sha256,
        )

playwright = module_extension(
    implementation = _playwright_impl,
    doc = "Chromium headless shell matching the pinned @playwright/test version.",
)
