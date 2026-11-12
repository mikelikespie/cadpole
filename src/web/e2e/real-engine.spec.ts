// End-to-end test against the *real* engine: no `?mock=1`, so the UI loads
// `./cadpole_wasm.js` (//src/rust/cadpole-wasm:cadpole_wasm_opt, served out of
// //src/web:dist by devserver.mjs) and the dev server serves
// e2e/real-engine.star, a script written against the actual Starlark API.
//
// The mock engine cannot produce this: it only ever builds a plate with a
// boss, it does not understand `box()` or `revolve()`, and its `api_doc()`
// only lists param/sketch/extrude. So `box` + `extrude` + `revolve` in the
// feature tree, and `revolve(`/`box(` in the API reference, are proof that the
// wasm engine ran.
//
// `Instant::now()` panics on wasm32 ("time not implemented on this platform"),
// so `cadpole_core::time::Stopwatch` reports zero there and the page measures
// evaluation with `performance.now()`; these cases cover that path.

import { expect, test } from "@playwright/test";

test.describe("real wasm engine", () => {
  test("loads the wasm engine rather than falling back to the mock", async ({ page }) => {
    const pageErrors: string[] = [];
    page.on("pageerror", (err) => pageErrors.push(err.message));

    // No ?mock=1: engine.ts imports ./cadpole_wasm.js and instantiates the wasm.
    await page.goto("/?nosw=1");
    await expect(page.locator("html")).toHaveClass(/ready/);

    // The engine runs in a dedicated worker, so "worker" rather
    // than "wasm": same build, off the main thread.
    await expect(page.locator("html")).toHaveAttribute("data-engine", "worker");
    await expect(page.locator("html")).toHaveAttribute("data-local-engine", "worker");
    // Toolbar pill and footer status strip.
    await expect(page.locator(".pill-worker")).toHaveText("worker");
    await expect(page.locator(".pill-mock")).toHaveCount(0);
    await expect(page.locator(".status-item.eval")).toContainText("worker");
    // engine.ts only shows this banner when it fell back to MockEngine.
    await expect(page.locator(".banner")).toBeHidden();

    // The API reference is rendered from the engine's own api_doc(); the mock
    // only ever reports param/sketch/extrude.
    await page.getByRole("tab", { name: "API" }).click();
    const apidoc = page.getByRole("region", { name: "API reference" });
    await expect(apidoc).toContainText("revolve(");
    await expect(apidoc).toContainText("box(");

    // Which renderer backend actually ran. WebGPU needs an adapter; headless
    // Chromium normally has none, so three's WebGPURenderer falls back to its
    // WebGL2 backend. The HUD only writes on a rendered frame, so nudge the
    // camera after showing it.
    await page.keyboard.press("p");
    await page.keyboard.press("f");
    const hud = page.locator(".hud");
    await expect(hud).toContainText(/backend (webgpu|webgl)/);
    const backend = ((await hud.innerText()).match(/backend (webgpu|webgl)/) ?? ["", "?"])[1];
    console.log(`cadpole real-engine e2e: renderer backend = ${backend}`);
    test.info().annotations.push({ type: "renderer-backend", description: backend ?? "?" });

    expect(pageErrors, `page errors: ${pageErrors.join(" | ")}`).toEqual([]);
  });

  test("evaluates the served script and shows its features", async ({ page }) => {
    const pageErrors: string[] = [];
    page.on("pageerror", (err) => pageErrors.push(err.message));

    await page.goto("/?nosw=1");
    await expect(page.locator("html")).toHaveClass(/ready/);
    // main.ts stamps the evaluation count on <html> once a model is in.
    await expect(page.locator("html")).toHaveAttribute("data-evals", /[1-9]/);
    await expect(page.locator("html")).toHaveAttribute("data-engine", "worker");

    // The model the engine built from real-engine.star: three feature kinds
    // the mock does not have.
    const tree = page.getByRole("tree", { name: "Feature tree" });
    await expect(tree.getByRole("treeitem", { name: /^plate box/ })).toBeVisible();
    await expect(tree.getByRole("treeitem", { name: /^boss extrude/ })).toBeVisible();
    await expect(tree.getByRole("treeitem", { name: /^ring revolve/ })).toBeVisible();
    await expect(page.locator(".diag-summary")).toContainText("No issues");
    await expect(page.locator(".status-item.eval")).toContainText(/worker [\d.]+ ms/);

    // Parameters come from the script's own `param()` calls, not from the
    // mock's hardcoded defaults (height is 12 here, 20 in the mock).
    const params = page.getByRole("region", { name: "Parameters" });
    await expect(params.getByLabel("height (mm)")).toHaveValue("12");
    await expect(params.getByLabel("boss_h (mm)")).toHaveValue("14");

    // The HUD credits the evaluation to the wasm engine.
    await page.keyboard.press("p");
    await page.keyboard.press("f");
    await expect(page.locator(".hud")).toContainText(/· worker ·/);
    await page.keyboard.press("p");

    await page.waitForTimeout(500);
    await expect(page).toHaveScreenshot("real-engine.png", { fullPage: true });
    expect(pageErrors, `page errors: ${pageErrors.join(" | ")}`).toEqual([]);
  });

  test("changing a parameter re-evaluates through the wasm engine", async ({ page }) => {
    await page.goto("/?nosw=1");
    await expect(page.locator("html")).toHaveAttribute("data-engine", "worker");
    await expect(page.locator("html")).toHaveAttribute("data-evals", /[1-9]/);
    const evals = async (): Promise<number> => Number(await page.locator("html").getAttribute("data-evals"));
    const before = await evals();

    const width = page.getByRole("region", { name: "Parameters" }).getByLabel("width (mm)");
    await width.fill("120");
    await width.blur();

    await expect(page.locator(".param-row.overridden")).toHaveCount(1);
    await expect.poll(evals, { timeout: 15_000 }).toBeGreaterThan(before);
    await expect(page.locator(".diag-summary")).toContainText("No issues");
    await expect(page.locator(".status-item.eval")).toContainText("worker");
  });

  // Last in the file on purpose: the dev server's session outlives the test,
  // and this replaces its main.star.
  test("a gallery example loaded into the session evaluates on the real engine", async ({ page }) => {
    await page.goto("/?nosw=1");
    await expect(page.locator("html")).toHaveClass(/ready/);
    await expect(page.locator("html")).toHaveAttribute("data-engine", "worker");
    await expect(page.locator("html")).toHaveAttribute("data-evals", /[1-9]/);

    // In session mode this is `SetScript`: the example becomes the document,
    // and the real engine -- not the mock the scripts were once written for
    // -- has to evaluate it. `//src/web:examples_test` checks every example
    // through `cadpole eval`; this is the same claim made through the page.
    const before = await page.locator("html").getAttribute("data-evals");
    await page.getByRole("tab", { name: "Examples" }).click();
    await page.getByRole("button", { name: "Load example Plate with boss" }).click();
    await expect.poll(() => page.locator("html").getAttribute("data-evals"), { timeout: 30_000 }).not.toBe(before);
    await expect(page.locator(".diag-summary")).toContainText("No issues");
    // The tree lives in the Model tab, which the Examples tab replaced.
    await page.getByRole("tab", { name: "Model" }).click();
    const tree = page.getByRole("tree", { name: "Feature tree" });
    await expect(tree.getByRole("treeitem", { name: /^base extrude/ })).toBeVisible();
    await expect(tree.getByRole("treeitem", { name: /^boss extrude/ })).toBeVisible();
  });

  // Also last, for the same reason: it loads the "Print plates" example.
  //
  // The plates view on the real engine: that example writes a script
  // with two Bambu `export()` calls into the document, the wasm engine
  // evaluates it, and `Model.plates` comes back from
  // `cadpole_engine::print::plate_plan` -- the mock only scrapes the text, so
  // the filament labels here are the engine's own.
  test("the plates view shows the engine's plate layout", async ({ page }) => {
    const pageErrors: string[] = [];
    page.on("pageerror", (err) => pageErrors.push(err.message));
    await page.goto("/?nosw=1");
    await expect(page.locator("html")).toHaveClass(/ready/);
    await expect(page.locator("html")).toHaveAttribute("data-evals", /[1-9]/);
    await expect(page.getByRole("group", { name: "View", exact: true })).toBeHidden();
    await page.getByRole("tab", { name: "Examples" }).click();
    await page.getByRole("button", { name: "Load example Print plates" }).click();
    const platesBtn = page.getByRole("button", { name: "Plates", exact: true });
    await expect(platesBtn).toBeVisible({ timeout: 30_000 });
    await expect(page.locator("html")).toHaveAttribute("data-engine", "worker");
    await page.waitForTimeout(1600);
    await platesBtn.click();
    await expect(page.locator("html")).toHaveAttribute("data-view", "plates");
    const tabs = page.getByRole("tablist", { name: "Plates" });
    await expect(tabs.getByRole("tab")).toHaveCount(3);
    await expect(tabs.getByRole("tab", { name: "1 · Boss" })).toBeVisible();
    await expect(tabs.getByRole("tab", { name: "2 · Base" })).toBeVisible();
    // The engine labels a slot with its material's name, `default` for one
    // the script only coloured; the base is pinned to slot 3.
    const legend = page.getByRole("list", { name: "Filaments" });
    await expect(legend.getByRole("listitem")).toHaveCount(2);
    await expect(legend).toContainText("1 · default");
    await expect(legend).toContainText("3 · default");
    await expect(page.locator(".platelabel")).toHaveCount(2);
    await page.waitForTimeout(300);
    await expect(page).toHaveScreenshot("plates-real.png", { fullPage: true });
    expect(pageErrors, `page errors: ${pageErrors.join(" | ")}`).toEqual([]);
  });
});
