"""`playwright_test`: a hermetic Playwright suite as a Bazel test.

    load("//build_support/playwright:defs.bzl", "playwright_test")

    playwright_test(
        name = "e2e_test",
        config = "e2e/playwright.config.ts",
        data = [":dist", ":node_modules/@playwright/test"] + glob(["e2e/**"]),
        snapshot_dir = "e2e/__screenshots__",
    )

What the macro guarantees:

  * the browser comes from //build_support/playwright:browsers (sha256-pinned,
    fetched by Bazel) and is reached through `PLAYWRIGHT_BROWSERS_PATH` inside
    runfiles -- no ~/.cache/ms-playwright, no `playwright install`;
  * `PLAYWRIGHT_SKIP_BROWSER_DOWNLOAD=1`, so a missing browser fails loudly
    instead of quietly downloading one;
  * the HTML report and the per-test output (failure screenshots, traces) land
    in `$TEST_UNDECLARED_OUTPUTS_DIR`, i.e.
    `bazel-testlogs/<target>/test.outputs/outputs.zip`. The config is expected
    to honour `PLAYWRIGHT_HTML_REPORT` and `PLAYWRIGHT_OUTPUT_DIR`, which
    runner.mjs sets;
  * a companion `<name>_update` binary (`bazel run`) re-runs the suite with
    `--update-snapshots` and, through `PLAYWRIGHT_SNAPSHOT_DIR`, writes the new
    baselines back into the source tree instead of into runfiles.
"""

load("@aspect_rules_js//js:defs.bzl", "js_binary", "js_test")

_RUNNER = "//build_support/playwright:runner"
_BROWSERS = "//build_support/playwright:browsers"
_BROWSERS_MARKER = "//build_support/playwright:browsers_marker"

def _update_name(name):
    return (name[:-len("_test")] if name.endswith("_test") else name) + "_update"

def playwright_test(
        name,
        config,
        data = [],
        env = {},
        args = [],
        chdir = None,
        snapshot_dir = None,
        update_target = True,
        tags = [],
        timeout = "moderate",
        **kwargs):
    """Runs a Playwright suite against a Bazel-fetched browser.

    Args:
        name: test target name.
        config: the Playwright config file (a label, usually package-relative).
        data: runfiles: the config's `testDir`, the app under test,
            `:node_modules/@playwright/test`, screenshot baselines, ...
        env: extra environment for the test.
        args: extra arguments forwarded to `playwright test`.
        chdir: working directory, so that `node_modules` resolves. Defaults to
            the calling package.
        snapshot_dir: package-relative screenshot baseline directory. When set,
            `bazel run //<pkg>:<name>_update` rewrites those baselines in the
            source tree.
        update_target: generate that `<name>_update` binary (default True).
        tags: extra tags.
        timeout: Bazel test timeout (default "moderate"; browsers are slow).
        **kwargs: forwarded to `js_test` / `js_binary`.
    """
    chdir = chdir if chdir != None else native.package_name()

    # `config` is usually also matched by the caller's glob over the test
    # directory; depset() keeps the label list duplicate-free.
    all_data = depset(data + [config, _BROWSERS, _BROWSERS_MARKER]).to_list()

    common_env = dict({
        # Runfiles-relative; runner.mjs resolves them against JS_BINARY__RUNFILES.
        "PLAYWRIGHT_BROWSERS_MARKER": "$(rlocationpath {})".format(_BROWSERS_MARKER),
        "PLAYWRIGHT_CONFIG": "$(rlocationpath {})".format(config),
        # Playwright must never fetch a browser at test time.
        "PLAYWRIGHT_SKIP_BROWSER_DOWNLOAD": "1",
    }, **env)

    js_test(
        name = name,
        chdir = chdir,
        data = all_data,
        entry_point = _RUNNER,
        env = common_env,
        args = args,
        # ~300 MB of browser: keep it in the external repository and let the
        # runfiles symlink to it instead of copying it into bazel-out.
        no_copy_to_bin = [_BROWSERS, _BROWSERS_MARKER],
        tags = tags,
        timeout = timeout,
        **kwargs
    )

    if update_target:
        update_env = dict(common_env)
        if snapshot_dir:
            update_env["PLAYWRIGHT_SNAPSHOT_SOURCE"] = "/".join(
                [p for p in [native.package_name(), snapshot_dir] if p],
            )
        js_binary(
            name = _update_name(name),
            chdir = chdir,
            data = all_data,
            entry_point = _RUNNER,
            env = update_env,
            fixed_args = args + ["--update-snapshots"],
            no_copy_to_bin = [_BROWSERS, _BROWSERS_MARKER],
            tags = tags + ["manual"],
            **kwargs
        )
