// The site's page rules, in headless Chromium against a loopback server.
//
//   bazel test //site:checks_test
//   bazel test //site:checks_test --test_env=CADPOLE_SITE_ROOT=/an/assembled/site
import { defineConfig } from "@playwright/test";

export default defineConfig({
  testDir: ".",
  testMatch: "*.spec.ts",
  outputDir: process.env["PLAYWRIGHT_OUTPUT_DIR"] ?? "test-results",
  reporter: [["list"], ["html", { open: "never", outputFolder: process.env["PLAYWRIGHT_HTML_REPORT"] ?? "playwright-report" }]],
  // One browser at a time: the live 3D runs on a software rasteriser.
  workers: 1,
  fullyParallel: false,
  retries: 0,
  timeout: 240_000,
  use: {
    browserName: "chromium",
    launchOptions: { args: ["--use-gl=angle", "--use-angle=swiftshader", "--enable-unsafe-swiftshader"] },
  },
});
