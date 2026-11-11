// Entry point of every `playwright_test` target (build_support/playwright/defs.bzl).
//
// Bazel gives us runfiles-relative paths; Playwright wants absolute ones and an
// environment that never reaches out to the network or to ~/.cache. This script
// is the translation layer:
//
//   PLAYWRIGHT_BROWSERS_MARKER   rlocationpath of a file sitting at the root of
//                                the Bazel-fetched browser cache; its directory
//                                becomes PLAYWRIGHT_BROWSERS_PATH.
//   PLAYWRIGHT_CONFIG            rlocationpath of the playwright config file.
//   PLAYWRIGHT_CLI               path (relative to the chdir'd cwd) of the
//                                @playwright/test CLI.
//   PLAYWRIGHT_SNAPSHOT_SOURCE   workspace-relative snapshot directory; only
//                                used by the `..._update` binary, where it is
//                                resolved against BUILD_WORKSPACE_DIRECTORY and
//                                re-exported as PLAYWRIGHT_SNAPSHOT_DIR so the
//                                config writes new baselines into the source
//                                tree rather than into runfiles.
//
// It also points Playwright's HTML report and its per-test output (traces,
// failure screenshots) at $TEST_UNDECLARED_OUTPUTS_DIR, which Bazel collects
// into bazel-testlogs/<target>/test.outputs/outputs.zip.

import { spawnSync } from "node:child_process";
import { existsSync, mkdirSync } from "node:fs";
import { dirname, isAbsolute, join, resolve } from "node:path";

function die(message) {
  console.error(`playwright_test: ${message}`);
  process.exit(1);
}

const runfiles = process.env["JS_BINARY__RUNFILES"] ?? process.env["RUNFILES_DIR"];
if (!runfiles) die("neither JS_BINARY__RUNFILES nor RUNFILES_DIR is set");

/** Resolve a Bazel rlocationpath ("<repo>/some/file") against the runfiles tree. */
function rlocation(p, what) {
  if (!p) die(`${what} is not set`);
  const abs = isAbsolute(p) ? p : join(runfiles, p);
  if (!existsSync(abs)) die(`${what} does not exist in runfiles: ${abs}`);
  return abs;
}

// ---------------------------------------------------------------- browsers

const browsersPath = dirname(rlocation(process.env["PLAYWRIGHT_BROWSERS_MARKER"], "PLAYWRIGHT_BROWSERS_MARKER"));
process.env["PLAYWRIGHT_BROWSERS_PATH"] = browsersPath;
// Belt and braces: never let Playwright reach for the network.
process.env["PLAYWRIGHT_SKIP_BROWSER_DOWNLOAD"] = "1";

// ------------------------------------------------------------------ paths

const config = rlocation(process.env["PLAYWRIGHT_CONFIG"], "PLAYWRIGHT_CONFIG");

const cli = resolve(process.env["PLAYWRIGHT_CLI"] ?? "node_modules/@playwright/test/cli.js");
if (!existsSync(cli)) die(`@playwright/test CLI not found at ${cli} (cwd ${process.cwd()})`);

// Chromium and Playwright both want somewhere to write. Bazel gives tests
// TEST_TMPDIR; `bazel run` gives neither that nor necessarily a usable HOME.
const tmp = process.env["TEST_TMPDIR"];
if (tmp) {
  process.env["TMPDIR"] = process.env["TMPDIR"] ?? tmp;
  process.env["HOME"] = process.env["HOME"] ?? tmp;
}

// Undeclared outputs: the HTML report and the failure screenshots/traces.
const outputsRoot = process.env["TEST_UNDECLARED_OUTPUTS_DIR"];
if (outputsRoot) {
  process.env["PLAYWRIGHT_OUTPUT_DIR"] = join(outputsRoot, "test-results");
  process.env["PLAYWRIGHT_HTML_REPORT"] = join(outputsRoot, "playwright-report");
  mkdirSync(process.env["PLAYWRIGHT_OUTPUT_DIR"], { recursive: true });
  mkdirSync(process.env["PLAYWRIGHT_HTML_REPORT"], { recursive: true });
}

// `bazel run //...:..._update`: write regenerated baselines into the source
// tree, not into the (throw-away) runfiles copy of it.
const snapshotSource = process.env["PLAYWRIGHT_SNAPSHOT_SOURCE"];
if (snapshotSource) {
  const workspace = process.env["BUILD_WORKSPACE_DIRECTORY"];
  if (!workspace) die("PLAYWRIGHT_SNAPSHOT_SOURCE needs `bazel run`, which sets BUILD_WORKSPACE_DIRECTORY");
  process.env["PLAYWRIGHT_SNAPSHOT_DIR"] = join(workspace, snapshotSource);
  mkdirSync(process.env["PLAYWRIGHT_SNAPSHOT_DIR"], { recursive: true });
  console.error(`playwright_test: writing baselines to ${process.env["PLAYWRIGHT_SNAPSHOT_DIR"]}`);
}

// -------------------------------------------------------------------- run

const argv = [cli, "test", "-c", config, ...process.argv.slice(2)];
const result = spawnSync(process.execPath, argv, { stdio: "inherit", env: process.env });
if (result.error) die(`could not start playwright: ${result.error.message}`);
process.exit(result.status ?? 1);
