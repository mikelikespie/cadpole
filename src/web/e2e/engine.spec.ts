// Where evaluation runs, against the real wasm engine locally and the
// dev server's `Evaluate` remotely.
//
// What each case is actually proving:
//
//   the worker is the default   The engine is not on the main thread, so a
//                               slow model cannot freeze the tab with no
//                               spinner, no camera and no way to give up.
//   the server can be chosen    The remote path really encodes an
//                               `EvaluateRequest`, decodes an
//                               `EvaluateResponse`, and draws *its* triangles
//                               -- the dev server answers with a body called
//                               `blk`, the script's own body is `plate`, so
//                               which engine produced the picture is visible
//                               in the feature tree and not only in a pill.
//   the fallback                A server that will not answer must leave a
//                               working UI, not a dead one.
//   a burst does not queue      Ten slider moves during a slow evaluation
//                               leave one evaluation outstanding, not ten.
//   cancelling                  Stops the work and keeps the picture.
//
// The dev server's behaviour is driven from `server_fail` / `server_slow_ms`
// in `e2e/engine.star`, set from the Parameters panel: the UI does not expose
// script editing in session mode, and moving a slider is the gesture the whole
// design is about anyway.

import { expect, test, type Page } from "@playwright/test";
import { decodePng } from "./png.js";

/** How many pixels of the viewport are warm -- far more red than blue. */
async function warmPixels(page: Page): Promise<number> {
  // Two frames, so the one that uploads a new geometry has been drawn.
  await page.evaluate(() => new Promise<void>((r) => requestAnimationFrame(() => requestAnimationFrame(() => r()))));
  const img = decodePng(await page.getByRole("img", { name: "3D model viewport" }).screenshot());
  let n = 0;
  for (let i = 0; i < img.rgba.length; i += 4) {
    const r = img.rgba[i] ?? 0;
    const b = img.rgba[i + 2] ?? 0;
    if (r > 100 && r > b + 80) n++;
  }
  return n;
}

/** `data-evals` counts finished evaluations; `data-stages` counts every result. */
const evals = (page: Page): Promise<number> =>
  page.evaluate(() => Number(document.documentElement.dataset["evals"] ?? 0));

async function ready(page: Page): Promise<void> {
  await expect(page.locator("html")).toHaveClass(/ready/);
  await expect.poll(() => evals(page), { timeout: 30_000 }).toBeGreaterThan(0);
}

/** Set a number parameter from the panel, the way a user would. */
/**
 * `fill` and nothing after it: on a number input `fill` fires the one `input`
 * event a typed value fires, and a second one dispatched by hand commits the
 * same edit again. On a busy machine the two land more than the 120 ms
 * evaluation debounce apart, and then one edit is two evaluations
 * (perf.spec.ts says the same, and counts them).
 */
async function setNumber(page: Page, label: string, value: number): Promise<void> {
  await page.getByLabel(label, { exact: true }).fill(String(value));
}

async function waitForAnotherEval(page: Page, from: number, timeout = 30_000): Promise<void> {
  await expect.poll(() => evals(page), { timeout }).toBeGreaterThan(from);
}

/**
 * Run `act`, and wait for the parameter write it causes to reach the server.
 *
 * A parameter change is pushed through a 300 ms trailing debounce, and the
 * local re-evaluation (`data-evals`) happens before that -- so "the page
 * re-evaluated" says nothing about whether the server has the value yet. A
 * test that moves on (or navigates) on the strength of `data-evals` alone
 * leaves the write in flight, and the dev server's one session carries it
 * into the next test: that is how `server_fail` stays set and the burst and
 * cancel tests watch every remote evaluation fail for a reason they never
 * asked for.
 */
async function written(page: Page, act: () => Promise<void>, timeout = 20_000): Promise<void> {
  const response = page.waitForResponse((r) => r.url().endsWith("/SetParams") && r.ok(), { timeout });
  await act();
  await response;
}

test.describe("where evaluation runs", () => {
  // One dev server serves every test here, and its session survives between
  // them: the `server_fail` a fallback test ticks is still ticked when the
  // next test starts, and that next test would then watch every remote
  // evaluation fail for a reason it never asked for. Parameters are session
  // state, so they are reset like session state.
  test.beforeEach(async ({ page }) => {
    await page.goto("/?nosw=1");
    await ready(page);
    // Hidden when nothing is overridden, which is the already-clean case.
    const reset = page.getByRole("button", { name: "Reset all" });
    if (await reset.isVisible()) {
      const before = await evals(page);
      // The write itself, not just the local re-evaluation: the test body's
      // first line navigates, and a navigation inside the debounce window
      // would leave the server holding the previous test's parameters.
      await written(page, () => reset.click());
      await waitForAnotherEval(page, before, 20_000);
    }
    await expect(page.locator("#param-server_fail")).not.toBeChecked();
  });

  test("evaluates in a worker by default, and says which engine ran", async ({ page }) => {
    // The default is for a document not yet measured. `beforeEach` evaluated
    // this one, and on a loaded machine that evaluation can pass the
    // promotion threshold, which would send the reload to the server.
    await page.evaluate(() => localStorage.removeItem("cadpole.engineVerdicts.v1"));
    await page.goto("/?nosw=1");
    await ready(page);

    // Not "wasm": the engine is in a dedicated worker, so a thirteen-second
    // model does not take the main thread with it.
    await expect(page.locator("html")).toHaveAttribute("data-engine", "worker");
    await expect(page.locator("html")).toHaveAttribute("data-local-engine", "worker");
    await expect(page.locator(".pill-worker")).toHaveText("worker");
    // The status line names the engine and the time, because "why is this
    // slow" has to be answerable from the screen.
    await expect(page.locator(".status-item.eval")).toContainText("worker");

    // The real engine evaluated the real script.
    await expect(page.getByRole("tree", { name: "Feature tree" })).toContainText("plate");
  });

  test("`?engine=main` still runs the engine on this thread, and admits it", async ({ page }) => {
    // The fallback path for a browser with no Worker, kept reachable so the
    // difference can be demonstrated rather than asserted.
    await page.goto("/?nosw=1&engine=main");
    await ready(page);
    await expect(page.locator("html")).toHaveAttribute("data-engine", "wasm");
    await expect(page.locator(".pill-wasm")).toHaveText("main thread");
    await expect(page.locator(".pill-wasm")).toHaveAttribute("title", /freeze/);
  });

  test("choosing the server evaluates there, and the server's model is what is drawn", async ({ page }) => {
    await page.goto("/?nosw=1");
    await ready(page);
    await expect(page.getByRole("tree", { name: "Feature tree" })).toContainText("plate");

    const before = await evals(page);
    await page.getByLabel("Where evaluation runs").selectOption("remote");
    await waitForAnotherEval(page, before);

    await expect(page.locator("html")).toHaveAttribute("data-engine", "remote");
    await expect(page.locator(".pill-remote")).toHaveText("server");
    await expect(page.locator(".status-item.eval")).toContainText("server");
    // The dev server's stand-in model, not the script's: proof the triangles
    // on screen came off the wire and were decoded, not produced locally.
    await expect(page.getByRole("tree", { name: "Feature tree" })).toContainText("blk");
    await expect(page.getByRole("tree", { name: "Feature tree" })).not.toContainText("plate");
  });

  test("a mesh that carries a colour per vertex is drawn in its own colours", async ({ page }) => {
    // An imported scan reaches the browser this way: `cadpole serve` evaluates
    // it and sends `Mesh.colors`. The dev server's stand-in box carries them
    // when `server_colors` is on, red below and amber above, over the grey
    // the viewer draws a body in by default.
    await page.goto("/?nosw=1");
    await ready(page);
    const before = await evals(page);
    await page.getByLabel("Where evaluation runs").selectOption("remote");
    await waitForAnotherEval(page, before);
    await expect(page.locator("html")).toHaveAttribute("data-engine", "remote");
    const plain = await warmPixels(page);

    const beforeColors = await evals(page);
    await written(page, () => page.locator("#param-server_colors").check());
    await waitForAnotherEval(page, beforeColors);
    await expect(page.locator("html")).toHaveAttribute("data-engine", "remote");
    await expect.poll(() => warmPixels(page), { timeout: 20_000 }).toBeGreaterThan(Math.max(400, plain * 4));

    // The faces mode paints the same attribute with a colour per face; the
    // mesh's own colours come back when it is left.
    await page.getByLabel("Display mode").selectOption("faces");
    await page.evaluate(() => new Promise<void>((r) => requestAnimationFrame(() => requestAnimationFrame(() => r()))));
    await page.getByLabel("Display mode").selectOption("shaded_edges");
    await expect.poll(() => warmPixels(page), { timeout: 20_000 }).toBeGreaterThan(Math.max(400, plain * 4));
  });

  test("a server that will not answer falls back to the local engine and says so", async ({ page }) => {
    await page.goto("/?nosw=1");
    await ready(page);
    const before = await evals(page);
    await page.getByLabel("Where evaluation runs").selectOption("remote");
    await waitForAnotherEval(page, before);
    await expect(page.locator("html")).toHaveAttribute("data-engine", "remote");

    // Tell the dev server to fail, then make it evaluate again.
    const beforeFail = await evals(page);
    await page.locator("#param-server_fail").check();
    await waitForAnotherEval(page, beforeFail);

    // Fallen back, with a working model on screen and a visible reason.
    await expect(page.locator("html")).toHaveAttribute("data-engine", "worker");
    await expect(page.locator(".status-item.engine")).toContainText("Remote evaluation failed");
    await expect(page.getByRole("tree", { name: "Feature tree" })).toContainText("plate");
    // And it does not keep trying: the document was demoted, so the next
    // evaluation is local too, without a second round trip to fail.
    const afterFallback = await evals(page);
    // The last write of this test has to land before the next test starts,
    // or it lands *after* that test's reset.
    await written(page, () => setNumber(page, "size (mm)", 25));
    await waitForAnotherEval(page, afterFallback);
    await expect(page.locator("html")).toHaveAttribute("data-engine", "worker");
  });

  test("a burst of slider moves leaves one evaluation outstanding, not ten", async ({ page }) => {
    const requests: number[] = [];
    page.on("request", (r) => {
      if (r.url().includes("CadpoleOps/Evaluate")) requests.push(Date.now());
    });
    await page.goto("/?nosw=1");
    await ready(page);
    const before = await evals(page);
    await page.getByLabel("Where evaluation runs").selectOption("remote");
    await waitForAnotherEval(page, before);

    // Make the server slow, so there is reliably something in flight.
    const beforeSlow = await evals(page);
    await setNumber(page, "server_slow_ms (ms)", 1200);
    await waitForAnotherEval(page, beforeSlow);

    requests.length = 0;
    const beforeBurst = await evals(page);
    // Ten moves in well under the time one evaluation takes. When each one
    // landed is kept, because the bound below is about their spacing.
    const moves: number[] = [];
    for (let i = 0; i < 10; i++) {
      await setNumber(page, "size (mm)", 20 + i);
      moves.push(Date.now());
      await page.waitForTimeout(30);
    }
    // While the burst is in flight the UI says so, and offers a way out.
    //
    // Wait for the request to have actually gone out before looking at the
    // flag. Asserting on the flag alone is a race
    // under load -- not because the window is too short
    // (the server is held at `server_slow_ms` for exactly that reason) but
    // because the evaluation has not *started*: the burst debounces, and a
    // busy machine pushes that past the assertion's patience. `data-evaluating`
    // reads "0" because nothing is evaluating yet.
    await expect.poll(() => requests.length, { timeout: 30_000 }).toBeGreaterThan(0);
    await expect(page.locator("html")).toHaveAttribute("data-evaluating", "1", { timeout: 30_000 });
    await expect(page.locator(".eval-cancel")).toBeVisible({ timeout: 30_000 });

    await waitForAnotherEval(page, beforeBurst, 60_000);
    await expect.poll(() => page.evaluate(() => document.documentElement.dataset["evaluating"]), { timeout: 30_000 }).toBe("0");

    // The slot runs one job at a time and keeps at most one waiting, and an
    // edit waits 120 ms for the next one before it evaluates at all, so ten
    // moves inside that window cost at most a handful of evaluations -- never
    // one per move. Only a loaded box spaces the moves further apart than the
    // debounce (a `fill` can take 100 ms and a move 330 ms, and every
    // move then honestly supersedes the previous move's request), and each
    // gap wider than the debounce buys the app one more request. On a quiet
    // box there is no such gap and the bound is the four it always was.
    let wideGaps = 0;
    for (let i = 1; i < moves.length; i++) if ((moves[i] ?? 0) - (moves[i - 1] ?? 0) > 120) wideGaps++;
    expect(requests.length).toBeLessThanOrEqual(4 + wideGaps);
    expect(requests.length).toBeGreaterThan(0);

    // And what is on screen is the *last* value, not whichever evaluation
    // happened to finish last.
    await expect(page.getByLabel("size (mm)")).toHaveValue("29");
    await expect(page.locator(".status-item.eval")).not.toContainText("failed");
  });

  test("cancelling stops the evaluation and keeps what is on screen", async ({ page }) => {
    const requests: number[] = [];
    page.on("request", (r) => {
      if (r.url().includes("CadpoleOps/Evaluate")) requests.push(Date.now());
    });
    await page.goto("/?nosw=1");
    await ready(page);
    const before = await evals(page);
    await page.getByLabel("Where evaluation runs").selectOption("remote");
    await waitForAnotherEval(page, before);
    const beforeSlow = await evals(page);
    await setNumber(page, "server_slow_ms (ms)", 4000);
    await waitForAnotherEval(page, beforeSlow, 30_000);

    const finished = await evals(page);
    requests.length = 0;
    await setNumber(page, "size (mm)", 55);
    // Same race as the burst above: look once the evaluation has been asked
    // for, not once the slider has been moved.
    await expect.poll(() => requests.length, { timeout: 30_000 }).toBeGreaterThan(0);
    await expect(page.locator(".eval-cancel")).toBeVisible({ timeout: 30_000 });
    await page.locator(".eval-cancel").click();

    await expect(page.locator(".status-item.engine")).toContainText("cancelled");
    await expect.poll(() => page.evaluate(() => document.documentElement.dataset["evaluating"]), { timeout: 10_000 }).toBe("0");
    // Cancelling is "stop computing", not "throw away what I am looking at":
    // no new model was applied, and the old one is still drawn.
    expect(await evals(page)).toBe(finished);
    await expect(page.getByRole("tree", { name: "Feature tree" })).toContainText("blk");
  });

  test("the static playground has no server to offload to, and says so", async ({ page }) => {
    await page.goto("/?nosw=1&static=1");
    await expect(page.locator("html")).toHaveClass(/ready/);
    // Still the worker: the main thread stays free everywhere, not only where
    // there is a server.
    await expect(page.locator("html")).toHaveAttribute("data-engine", "worker");
    const remote = page.locator('#engine-mode option[value="remote"]');
    await expect(remote).toBeDisabled();
    await expect(remote).toHaveAttribute("title", /no server/i);
  });
});
