// Playwright end-to-end tests for the web UI.
//
// Through Bazel (hermetic: the Chromium headless shell is fetched and
// sha256-pinned by //build_support/playwright, so nothing needs installing):
//
//   bazel test //src/web:e2e_test       # compare against e2e/__screenshots__
//   bazel run  //src/web:e2e_update     # regenerate those baselines
//
// Without Bazel (needs `pnpm exec playwright install chromium-headless-shell`):
//
//   bazel build //src/web:dist && cd src/web && pnpm e2e
//
// Environment, all optional, all set by //build_support/playwright:defs.bzl:
//   CADPOLE_DIST                 the built dist directory (default: bazel-bin/...)
//   PLAYWRIGHT_OUTPUT_DIR     per-test output: traces, failure screenshots
//   PLAYWRIGHT_HTML_REPORT    where to write the HTML report
//   PLAYWRIGHT_SNAPSHOT_DIR   where --update-snapshots writes baselines
//   CADPOLE_E2E_*_PORT           pin one server's port (see PORT_VARS below);
//                             unset, each run takes free ones from the kernel
//   CADPOLE_PERF_BASELID=1       also serve examples/gridfinity_baselid.star and
//                             run the benchmark against it (slow: ~13 s an
//                             evaluation)
//   CADPOLE_PERF_TRACE=1         print every span with its timestamp

import { defineConfig, devices } from "@playwright/test";
import { execFileSync } from "node:child_process";
import { existsSync, mkdirSync } from "node:fs";
import { fileURLToPath } from "node:url";
import { tmpdir } from "node:os";
import { dirname, resolve } from "node:path";

const here = dirname(fileURLToPath(import.meta.url));
// src/web, both in the source tree and in a runfiles copy of it.
const webRoot = resolve(here, "..");
const devserver = resolve(webRoot, "devserver.mjs");
// CADPOLE_DIST is relative to src/web (Bazel passes "dist"); resolve it here so
// the dev server's own working directory does not matter.
const distDir = resolve(webRoot, process.env["CADPOLE_DIST"] ?? "../../bazel-bin/src/web/dist");
// Bazel's hermetic Node is not on PATH inside a test.
const node = process.execPath;

/**
 * Every server below listens on a port the kernel picked for this run, so
 * two runs on one machine (two checkouts, or `--runs_per_test`) never ask
 * for the same one. A child process binds them all to port 0 at once, so
 * they are distinct, reports the numbers and lets go; the servers bind them
 * a moment later.
 *
 * The numbers go back into `process.env`, because Playwright loads this file
 * again in every test worker and the workers inherit the runner's
 * environment: a worker reads the ports the runner chose instead of choosing
 * its own. A port already set in the environment is kept, which is also how
 * a run can pin one.
 */
const PORT_VARS = [
  "CADPOLE_E2E_PORT",
  "CADPOLE_E2E_REAL_PORT",
  "CADPOLE_E2E_TOKEN_PORT",
  "CADPOLE_E2E_ENGINE_PORT",
  "CADPOLE_E2E_PERF_PORT",
  "CADPOLE_E2E_BASELID_PORT",
  "CADPOLE_E2E_TOY_PORT",
  "CADPOLE_E2E_HOT_PORT",
  "CADPOLE_E2E_PUBLISH_PORT",
  "CADPOLE_E2E_CRASH_PORT",
] as const;
const ports: Record<string, number> = (() => {
  const missing = PORT_VARS.filter((v) => !process.env[v]);
  if (missing.length > 0) {
    const pick = `
      const net = require("node:net");
      const n = Number(process.argv[1]);
      const servers = Array.from({ length: n }, () => net.createServer());
      Promise.all(servers.map((s) => new Promise((ok, fail) => {
        s.once("error", fail);
        s.listen(0, "127.0.0.1", () => ok(s.address().port));
      }))).then((ports) => {
        process.stdout.write(JSON.stringify(ports));
        for (const s of servers) s.close();
      });
    `;
    const picked = JSON.parse(execFileSync(node, ["-e", pick, String(missing.length)], { encoding: "utf8" })) as number[];
    missing.forEach((v, i) => (process.env[v] = String(picked[i])));
  }
  return Object.fromEntries(PORT_VARS.map((v) => [v, Number(process.env[v])]));
})();

// Two dev servers: the mock-engine suite drives the built-in sample script,
// the real-engine suite needs one the wasm engine can evaluate.
const port = ports["CADPOLE_E2E_PORT"]!;
const realPort = ports["CADPOLE_E2E_REAL_PORT"]!;
// A third dev server that demands a bearer token, for token.spec.ts.
const tokenPort = ports["CADPOLE_E2E_TOKEN_PORT"]!;
const e2eToken = "e2e-secret-token";
// A fourth, for engine.spec.ts: where evaluation runs. It serves
// e2e/engine.star, which the real wasm engine evaluates locally and whose
// `server_fail` / `server_slow_ms` parameters tell the dev server how to
// answer `Evaluate` -- so one server covers the happy path, the fallback and
// the in-flight case, and the test drives all three from the Parameters panel.
const enginePort = ports["CADPOLE_E2E_ENGINE_PORT"]!;
// A fifth, for perf.spec.ts. It serves e2e/perf.star, a document that
// is trivial for the engine and expensive for the viewer, which is what makes
// the benchmark's wall clock the apply path rather than the kernel.
const perfPort = ports["CADPOLE_E2E_PERF_PORT"]!;
// The real thing, opt-in. `examples/gridfinity_baselid.star` takes about
// thirteen seconds per evaluation in the wasm build -- too slow to run on
// every `bazel test`. So:
//
//   bazel test //src/web:e2e_test --test_arg=--project=baselid \
//     --test_env=CADPOLE_PERF_BASELID=1 --test_output=all --nocache_test_results
const baselid = process.env["CADPOLE_PERF_BASELID"] === "1";
const baselidPort = ports["CADPOLE_E2E_BASELID_PORT"]!;
// Copied into this package by `//src/web:perf_examples` under Bazel; straight
// out of the source tree when the suite is run with `pnpm e2e`.
const baselidScript = [
  resolve(webRoot, "perf_examples/gridfinity_baselid.star"),
  resolve(webRoot, "../../examples/gridfinity_baselid.star"),
].find((p) => existsSync(p));

// The gear toy's plates, opt-in like the base lid: the toy takes tens
// of seconds to evaluate in wasm, and the one thing this project produces is
// a picture of its three plates in the plates view, saved to
// CADPOLE_E2E_TOY_SHOT when that is set:
//
//   bazel run //src/web:e2e_update -- --project=toy \
//     --test_env=CADPOLE_E2E_TOY=1  (or CADPOLE_E2E_TOY=1 in the environment)
const toy = process.env["CADPOLE_E2E_TOY"] === "1";
const toyPort = ports["CADPOLE_E2E_TOY_PORT"]!;
const toyScript = [resolve(webRoot, "perf_examples/gear_toy.star"), resolve(webRoot, "../../examples/gear_toy.star")].find((p) =>
  existsSync(p),
);

// A sixth, for hot-reload.spec.ts, and the only one that can *rebuild*.
// `--hot` makes the dev server serve a writable copy of dist and expose
// `POST /api/hot/<file>`, which edits one file of the bundle and emits the
// `build_changed` that `cadpole serve`'s watcher would -- so the suite can change
// a stylesheet, the engine or the app bundle independently and watch the page
// do the least that suffices. It serves e2e/engine.star, because the
// in-flight case needs `server_slow_ms`.
const hotPort = ports["CADPOLE_E2E_HOT_PORT"]!;
// The document that server serves. `engine.star` by default, because every
// assertion in hot-reload.spec.ts is written against it; `CADPOLE_E2E_HOT_SCRIPT`
// points it somewhere else for a one-off measurement against a document whose
// evaluation is measured in seconds rather than milliseconds, e.g.
//
//   bazel run //src/web:e2e_update -- --project=hot --grep "what each kind of update costs"
//
// with CADPOLE_E2E_HOT_SCRIPT=<abs>/examples/gridfinity_baselid.star in the
// environment. Only the measurement survives that swap; the rest of the suite
// drives parameters that only `engine.star` declares.
const hotScript = process.env["CADPOLE_E2E_HOT_SCRIPT"] ?? resolve(here, "engine.star");

// A seventh, for publish.spec.ts: no dev server at all in the sense
// the others are. `cadpole publish` bakes examples/cabin_site.star (which loads
// cabin_parts.star) into a directory, and that directory is served as plain
// files (`--static`: the dev server answers no /api) -- what a static host
// does. Built when the config loads, so the server below only serves.
const publishPort = ports["CADPOLE_E2E_PUBLISH_PORT"]!;
export const publishDir = resolve(process.env["TEST_TMPDIR"] ?? process.env["TMPDIR"] ?? tmpdir(), "cadpole-publish-e2e");
/**
 * The second site: the cabin published with the opening choices --
 * a top view framed on the cabin layer, the grid off, the lot's layer
 * hidden, two parameters listed, and a section through the floor slab
 * (0.25 ft up a 0.5-ft slab, so the cut face covers the cabin's footprint).
 */
export const PUBLISH_FLAGS_DIR = "flags";
export const PUBLISH_FLAGS = "--view top --frame-layer cabin --grid off --hide-layer site --show-params width,depth --section z=0.25";
/**
 * A third: the cabin opening in x-ray with a link back to the site it is
 * part of (`--mode`, `--home-url`, `--home-label`).
 */
export const PUBLISH_OPENING_DIR = "opening";
export const PUBLISH_OPENING = "--mode xray --home-url ../ --home-label Models";
/**
 * And the lighting samples (shading.spec.ts): e2e/shading.star published once
 * per sample with the grid off, beside `cadpole render`'s picture of the same
 * body from the same side on a green key -- the look the viewport is held to.
 */
export const SHADING_DIR = "shading";
export const SHADING_SAMPLES = ["dark_metal", "white_matte", "painted", "dark_matte"] as const;
export const SHADING_REFERENCE = "reference.png";
/** The depth-precision sample of the same script: published, not rendered. */
export const SHADING_FILM = "film";
const shadingScript = resolve(here, "shading.star");
const cabinScript = [resolve(webRoot, "perf_examples/cabin_site.star"), resolve(webRoot, "../../examples/cabin_site.star")].find((p) =>
  existsSync(p),
);

// ---------------------------------------------------------------------------
// The crash suite is the one thing here that does NOT use the dev
// server: failure reports go to `POST /api/ops/ReportClientFailure` and become
// events in a session log, and devserver.mjs implements neither the op
// registry nor an event log. Proving the reporting works therefore means the
// real `cadpole serve`, the real router, the real capability check and the real
// event log -- with the test-hooks wasm in front of it, so a real Rust panic
// can be provoked on demand.
export const crashPort = ports["CADPOLE_E2E_CRASH_PORT"]!;
export const crashToken = "e2e-crash-owner-token";
/**
 * Where `cadpole serve` writes this run's token files; the spec reads them.
 *
 * `TEST_TMPDIR` first, because it is unique per Bazel run and the other two
 * are not. Under Bazel `TMPDIR` can be plain `/tmp`, which made this one
 * directory shared by every concurrent run on the machine -- and two runs
 * minting tokens into it means one of them reads the other's, which surfaces
 * as this suite intermittently getting a 401 where it demanded a 403. Nothing
 * here is wrong about authentication; the tokens were simply someone else's.
 */
export const crashRuntimeDir = resolve(
  process.env["TEST_TMPDIR"] ?? process.env["TMPDIR"] ?? tmpdir(),
  "cadpole-crash-e2e",
);
const crashDist = resolve(webRoot, process.env["CADPOLE_E2E_DIST"] ?? "../../bazel-bin/src/web/e2e_dist");

/** The `cadpole` binary, from runfiles under Bazel or bazel-bin outside it. */
function cadpoleBinary(): string {
  for (const p of [
    process.env["CADPOLE_BIN"] ? resolve(process.env["CADPOLE_BIN"]) : "",
    resolve(webRoot, "../rust/cadpole-cli/cadpole"),
    resolve(webRoot, "../../bazel-bin/src/rust/cadpole-cli/cadpole"),
  ]) {
    if (p && existsSync(p)) return p;
  }
  throw new Error("cannot find the cadpole binary; `bazel build //src/rust/cadpole-cli:cadpole`");
}

// The token directory has to exist and be private before `cadpole serve` starts.
// Deliberately *not* cleaned here: crash.spec.ts imports this module to learn
// the path, which re-runs it in every test worker, and a delete there would
// remove the tokens out from under the running server. Every `cadpole serve`
// start mints and rewrites both token files anyway, so a leftover from a
// previous run cannot be read by mistake.
mkdirSync(crashRuntimeDir, { recursive: true, mode: 0o700 });

const outputDir = process.env["PLAYWRIGHT_OUTPUT_DIR"] ?? process.env["TEST_UNDECLARED_OUTPUTS_DIR"] ?? resolve(here, "../test-results");
const htmlReport = process.env["PLAYWRIGHT_HTML_REPORT"] ?? resolve(outputDir, "playwright-report");

// Baselines live next to the tests; `--update-snapshots` under Bazel points
// PLAYWRIGHT_SNAPSHOT_DIR at the source tree so the new PNGs are not written
// into a runfiles directory that is about to be thrown away.
const snapshotDir = process.env["PLAYWRIGHT_SNAPSHOT_DIR"] ?? `${here}/__screenshots__`;

// Rendering backend for the tests. By default headless Chromium gets no
// WebGPU adapter, so three's WebGPURenderer falls back to its WebGL2 backend
// on SwiftShader, which is deterministic and shows up in screenshots.
// CADPOLE_E2E_WEBGPU=1 enables WebGPU on SwiftShader instead; with the
// Chromium 153 headless shell the WebGPU canvas renders but is captured blank
// by page screenshots, so it is opt-in.
const chromeArgs = [
  // Chromium's own sandbox needs user namespaces, which a Bazel test sandbox
  // (and most CI containers) do not hand out.
  "--no-sandbox",
  "--disable-dev-shm-usage",
  ...(process.env["CADPOLE_E2E_WEBGPU"]
    ? [
        "--enable-unsafe-webgpu",
        "--enable-features=Vulkan,WebGPU",
        "--use-angle=swiftshader",
        "--use-webgpu-adapter=swiftshader",
        "--ignore-gpu-blocklist",
        "--disable-gpu-sandbox",
      ]
    : ["--use-angle=swiftshader", "--ignore-gpu-blocklist"]),
];

// WebGPU for the projects that are about it: Dawn on SwiftShader's Vulkan,
// which the headless shell both draws and shows in a screenshot (with
// `--use-webgpu-adapter=swiftshader` and no Vulkan compositor it draws and
// the screenshot is blank). SwiftShader's adapter is blocklisted, so there
// is none without `--enable-unsafe-webgpu`; the spec checks it got one.
const webgpuArgs = [
  "--no-sandbox",
  "--disable-dev-shm-usage",
  "--use-angle=swiftshader",
  "--enable-unsafe-swiftshader",
  "--ignore-gpu-blocklist",
  "--enable-unsafe-webgpu",
  "--enable-features=Vulkan,WebGPU",
  "--use-vulkan=swiftshader",
];

export default defineConfig({
  testDir: here,
  outputDir,
  snapshotPathTemplate: `${snapshotDir}/{testFileName}/{arg}-{projectName}{ext}`,
  timeout: 90_000,
  fullyParallel: false,
  workers: 1,
  retries: 0,
  reporter: [["list"], ["html", { outputFolder: htmlReport, open: "never" }]],
  expect: {
    // One capture of the software-rasterised canvas is a few hundred
    // milliseconds on an idle box and several seconds on one that is running
    // four other Bazel sandboxes -- past the 5 s default, so a screenshot
    // assertion timed out while *taking* its first picture, not comparing
    // it. The same timeout governs every other expect, which only waits
    // longer before failing.
    timeout: 20_000,
    toHaveScreenshot: { maxDiffPixelRatio: 0.03, animations: "disabled", caret: "hide" },
  },
  use: {
    baseURL: `http://127.0.0.1:${port}`,
    colorScheme: "dark",
    reducedMotion: "reduce",
    deviceScaleFactor: 1,
    trace: "retain-on-failure",
    screenshot: "only-on-failure",
  },
  webServer: [
    {
      command: `${node} ${devserver} ${distDir} --port ${port}`,
      url: `http://127.0.0.1:${port}/index.html`,
      cwd: webRoot,
      reuseExistingServer: false,
      stdout: "pipe",
      stderr: "pipe",
      timeout: 30_000,
    },
    {
      command: `${node} ${devserver} ${distDir} --port ${realPort} --script ${resolve(here, "real-engine.star")}`,
      url: `http://127.0.0.1:${realPort}/index.html`,
      cwd: webRoot,
      reuseExistingServer: false,
      stdout: "pipe",
      stderr: "pipe",
      timeout: 30_000,
    },
    {
      // `--token` makes /api behave like `cadpole serve`: a bearer header or a
      // 401, and no `?token=` anywhere. index.html is static, so the health
      // check below does not need the token.
      command: `${node} ${devserver} ${distDir} --port ${tokenPort} --token ${e2eToken}`,
      url: `http://127.0.0.1:${tokenPort}/index.html`,
      cwd: webRoot,
      reuseExistingServer: false,
      stdout: "pipe",
      stderr: "pipe",
      timeout: 30_000,
    },
    {
      command: `${node} ${devserver} ${distDir} --port ${enginePort} --script ${resolve(here, "engine.star")}`,
      url: `http://127.0.0.1:${enginePort}/index.html`,
      cwd: webRoot,
      reuseExistingServer: false,
      stdout: "pipe",
      stderr: "pipe",
      timeout: 30_000,
    },
    {
      command: `${node} ${devserver} ${distDir} --port ${perfPort} --script ${resolve(here, "perf.star")}`,
      url: `http://127.0.0.1:${perfPort}/index.html`,
      cwd: webRoot,
      reuseExistingServer: false,
      stdout: "pipe",
      stderr: "pipe",
      timeout: 30_000,
    },
    {
      command: `${node} ${devserver} ${distDir} --port ${hotPort} --hot --script ${hotScript}`,
      url: `http://127.0.0.1:${hotPort}/index.html`,
      cwd: webRoot,
      reuseExistingServer: false,
      stdout: "pipe",
      stderr: "pipe",
      timeout: 60_000,
    },
    ...(baselid
      ? [
          {
            command: `${node} ${devserver} ${distDir} --port ${baselidPort} --script ${baselidScript}`,
            url: `http://127.0.0.1:${baselidPort}/index.html`,
            cwd: webRoot,
            reuseExistingServer: false,
            stdout: "pipe" as const,
            stderr: "pipe" as const,
            timeout: 30_000,
          },
        ]
      : []),
    ...(toy
      ? [
          {
            command: `${node} ${devserver} ${distDir} --port ${toyPort} --script ${toyScript}`,
            url: `http://127.0.0.1:${toyPort}/index.html`,
            cwd: webRoot,
            reuseExistingServer: false,
            stdout: "pipe" as const,
            stderr: "pipe" as const,
            timeout: 30_000,
          },
        ]
      : []),
    {
      // A published site, served as a static host would serve it.
      command:
        `${cadpoleBinary()} publish ${cabinScript} --model-unit ft --out-dir ${publishDir} --force --web-dir ${distDir} && ` +
        // The same cabin with every page option the publisher has, served
        // from a subdirectory of the same static root.
        `${cadpoleBinary()} publish ${cabinScript} --model-unit ft --out-dir ${publishDir}/${PUBLISH_FLAGS_DIR} --force --web-dir ${distDir} ` +
        `${PUBLISH_FLAGS} && ` +
        `${cadpoleBinary()} publish ${cabinScript} --model-unit ft --out-dir ${publishDir}/${PUBLISH_OPENING_DIR} --force --web-dir ${distDir} ` +
        `${PUBLISH_OPENING} && ` +
        SHADING_SAMPLES.map((sample) => {
          const out = `${publishDir}/${SHADING_DIR}/${sample}`;
          return (
            `${cadpoleBinary()} publish ${shadingScript} -p sample=${sample} --grid off --out-dir ${out} --force --web-dir ${distDir} && ` +
            `${cadpoleBinary()} render ${shadingScript} -p sample=${sample} --size 640x400 --view iso --no-grid --no-shadow --no-ao ` +
            `--background '#00ff00' -o ${out}/${SHADING_REFERENCE} && `
          );
        }).join("") +
        `${cadpoleBinary()} publish ${shadingScript} -p sample=${SHADING_FILM} --grid off --out-dir ${publishDir}/${SHADING_DIR}/${SHADING_FILM} --force --web-dir ${distDir} && ` +
        `${node} ${devserver} ${publishDir} --static --port ${publishPort}`,
      url: `http://127.0.0.1:${publishPort}/index.html`,
      cwd: webRoot,
      reuseExistingServer: false,
      stdout: "pipe",
      stderr: "pipe",
      timeout: 60_000,
    },
    {
      // The real server, for crash.spec.ts. `--token` pins the owner token so
      // the page can be opened with it; the restricted listener token is still
      // minted into CADPOLE_RUNTIME_DIR, which is how the capability test gets
      // one. `--root` keeps the document store inside the test's own tmpdir.
      command:
        `${cadpoleBinary()} serve ${resolve(here, "real-engine.star")} --port ${crashPort} ` +
        `--web-dir ${crashDist} --token ${crashToken} --root ${resolve(crashRuntimeDir, "documents")}`,
      url: `http://127.0.0.1:${crashPort}/index.html`,
      cwd: webRoot,
      env: { CADPOLE_RUNTIME_DIR: crashRuntimeDir },
      reuseExistingServer: false,
      stdout: "pipe",
      stderr: "pipe",
      timeout: 60_000,
    },
  ],
  projects: [
    {
      name: "desktop",
      testMatch: /cadpole\.spec\.ts$/,
      use: {
        ...devices["Desktop Chrome"],
        viewport: { width: 1280, height: 800 },
        launchOptions: { args: chromeArgs },
      },
    },
    {
      name: "phone",
      testMatch: /cadpole\.spec\.ts$/,
      use: {
        ...devices["Pixel 7"],
        deviceScaleFactor: 1,
        launchOptions: { args: chromeArgs },
      },
    },
    // The viewport through a change of size (resize.spec.ts), in both
    // layouts and on both backends: a buffer left undrawn shows on either,
    // and a frame that used a destroyed texture is dropped only by WebGPU.
    ...(["resize", "resize-webgpu"] as const).flatMap((name) => {
      const launchOptions = { args: name === "resize" ? chromeArgs : webgpuArgs };
      return [
        { name, testMatch: /resize\.spec\.ts$/, use: { ...devices["Desktop Chrome"], viewport: { width: 1280, height: 800 }, launchOptions } },
        { name: `${name}-phone`, testMatch: /resize\.spec\.ts$/, use: { ...devices["Pixel 7"], deviceScaleFactor: 1, launchOptions } },
      ];
    }),
    {
      // Authentication, against the dev server that requires a token. No
      // screenshots here: this is about headers, storage and the URL.
      name: "token",
      testMatch: /token\.spec\.ts$/,
      use: {
        ...devices["Desktop Chrome"],
        baseURL: `http://127.0.0.1:${tokenPort}`,
        viewport: { width: 1280, height: 800 },
        launchOptions: { args: chromeArgs },
      },
    },
    {
      // The real wasm engine, against the dev server that serves
      // e2e/real-engine.star.
      name: "real",
      testMatch: /real-engine\.spec\.ts$/,
      use: {
        ...devices["Desktop Chrome"],
        baseURL: `http://127.0.0.1:${realPort}`,
        viewport: { width: 1280, height: 800 },
        launchOptions: { args: chromeArgs },
      },
    },
    {
      // Where evaluation runs: the worker by default, the server when
      // asked, and local again when the server will not answer. No
      // screenshots: this is about which engine produced what is on screen.
      name: "engine",
      // Anchored on the separator: `/engine\.spec\.ts$/` alone also matches
      // `real-engine.spec.ts`, which belongs to the `real` project.
      testMatch: /[/\\]engine\.spec\.ts$/,
      use: {
        ...devices["Desktop Chrome"],
        baseURL: `http://127.0.0.1:${enginePort}`,
        viewport: { width: 1280, height: 800 },
        launchOptions: { args: chromeArgs },
      },
    },
    {
      // What the page does with an evaluation: the apply path's phase
      // breakdown, and how much of a model is reused rather than rebuilt. No
      // screenshots: this is about time and counts.
      name: "perf",
      testMatch: /perf\.spec\.ts$/,
      // The base lid is the `baselid` project's, against its own server.
      grepInvert: /base lid/,
      // A dozen evaluations, three reloads and a software rasteriser.
      timeout: 180_000,
      use: {
        ...devices["Desktop Chrome"],
        baseURL: `http://127.0.0.1:${perfPort}`,
        viewport: { width: 1280, height: 800 },
        launchOptions: { args: chromeArgs },
      },
    },
    {
      // A published page: `cadpole publish`'s directory on a static
      // server, opened with no engine, no /api and no Starlark.
      name: "publish",
      testMatch: /(publish|shading)\.spec\.ts$/,
      use: {
        ...devices["Desktop Chrome"],
        baseURL: `http://127.0.0.1:${publishPort}`,
        viewport: { width: 1280, height: 800 },
        launchOptions: { args: chromeArgs },
      },
    },
    {
      // Hot reload: what a rebuilt bundle costs, file by file. No
      // screenshots -- this is about what is replaced and what survives.
      name: "hot",
      testMatch: /hot-reload\.spec\.ts$/,
      use: {
        ...devices["Desktop Chrome"],
        baseURL: `http://127.0.0.1:${hotPort}`,
        viewport: { width: 1280, height: 800 },
        launchOptions: { args: chromeArgs },
      },
    },
    ...(baselid
      ? [
          {
            // The base lid itself: one body, 35 742 triangles, ~13 s per
            // evaluation in wasm. Opt-in, and its own project so the fast
            // suite is not held up by it.
            name: "baselid",
            testMatch: /perf\.spec\.ts$/,
            grep: /base lid/,
            timeout: 600_000,
            use: {
              ...devices["Desktop Chrome"],
              baseURL: `http://127.0.0.1:${baselidPort}`,
              viewport: { width: 1280, height: 800 },
              launchOptions: { args: chromeArgs },
            },
          },
        ]
      : []),
    ...(toy
      ? [
          {
            // The gear toy's three plates on the real engine, for
            // the picture: opt-in, its own project, its own server.
            name: "toy",
            testMatch: /toy\.spec\.ts$/,
            timeout: 600_000,
            use: {
              ...devices["Desktop Chrome"],
              baseURL: `http://127.0.0.1:${toyPort}`,
              viewport: { width: 1280, height: 800 },
              launchOptions: { args: chromeArgs },
            },
          },
        ]
      : []),
    {
      // Crash reporting, against a real `cadpole serve` and the test-hooks wasm.
      // No screenshots: this is about what reaches the session event log.
      name: "crash",
      testMatch: /crash\.spec\.ts$/,
      use: {
        ...devices["Desktop Chrome"],
        baseURL: `http://127.0.0.1:${crashPort}`,
        viewport: { width: 1280, height: 800 },
        launchOptions: { args: chromeArgs },
      },
    },
  ],
});
