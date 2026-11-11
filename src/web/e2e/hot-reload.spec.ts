// What a rebuilt bundle costs a running page.
//
// A rebuild that reloads the tab throws away everything --
// the message list, the view mode, the hidden bodies, the panel state, any
// work in flight -- for a change that is usually one line of CSS. The
// fingerprint `cadpole serve` publishes names each file separately, so a page can
// tell what actually moved and do the least that suffices. This suite is that
// claim, one case per file:
//
//   styles.css          swapped through the `<link>`. Nothing reloads and
//                       nothing is lost, so the page does it unasked.
//   cadpole_wasm_bg.wasm   a second engine worker is started and proved healthy
//                       before the first is retired, and the model is
//                       recomputed because the triangles on screen came out of
//                       the engine that was just replaced.
//   cadpole_wasm.js        the wasm-bindgen glue alone: the engine is swapped and
//                       the model is *not* recomputed, because the engine did
//                       not change.
//   app.js              the page cannot replace the code it is running, so it
//                       offers and waits -- unless there is provably nothing
//                       to interrupt, in which case it just goes.
//
// The dev server here is the only one started with `--hot`: it serves a
// writable copy of dist and `POST /api/hot/<file>` edits one file of it,
// which is what a real `bazel build //src/web:dist` would do to one file and
// what a read-only bazel-bin cannot be made to do at all.

import { expect, test, type Page } from "@playwright/test";

/** `data-evals` counts finished evaluations; `data-stages` counts every result. */
const evals = (page: Page): Promise<number> =>
  page.evaluate(() => Number(document.documentElement.dataset["evals"] ?? 0));

/** How many engines this page has installed. 1 is the one it started with. */
const engineEpoch = (page: Page): Promise<number> =>
  page.evaluate(() => Number(document.documentElement.dataset["engineEpoch"] ?? 0));

/** The engine epoch that was current when the displayed model was applied. */
const modelEpoch = (page: Page): Promise<number> =>
  page.evaluate(() => Number(document.documentElement.dataset["modelEpoch"] ?? 0));

/**
 * The session revision the page is showing (`rev N` in the status strip).
 *
 * It moves when a write of the page's own comes back with the session, which
 * is the moment `applySession` runs -- and `applySession` is where a page
 * that is behind the server is told so, again, on every refresh.
 */
const revision = (page: Page): Promise<number> =>
  page.evaluate(() => Number((document.querySelector(".status-item.rev")?.textContent ?? "").replace(/\D/g, "")));

/**
 * A value that exists only in this page's JavaScript.
 *
 * The one unambiguous test for "did it reload": every other signal -- the DOM,
 * the store, the engine -- is something a hot swap is *allowed* to replace.
 */
async function markPage(page: Page): Promise<void> {
  await page.evaluate(() => {
    (window as unknown as Record<string, unknown>)["__hotMark"] = "alive";
  });
}

async function stillTheSamePage(page: Page): Promise<boolean> {
  try {
    return await page.evaluate(() => (window as unknown as Record<string, unknown>)["__hotMark"] === "alive");
  } catch {
    // "Execution context was destroyed": the page navigated out from under
    // the evaluate, which is the answer rather than an error.
    return false;
  }
}

async function ready(page: Page): Promise<void> {
  await expect(page.locator("html")).toHaveClass(/ready/);
  await expect.poll(() => evals(page), { timeout: 30_000 }).toBeGreaterThan(0);
}

/** Rebuild one file of the served bundle, the way `bazel build` would. */
async function rebuild(page: Page, what: "styles" | "engine" | "glue" | "worker" | "app"): Promise<string> {
  const res = await page.request.post(`/api/hot/${what}`);
  expect(res.ok(), `rebuilding ${what}`).toBe(true);
  return ((await res.json()) as { build: string }).build;
}

/**
 * Whether the rule a stylesheet rebuild appends is among the document's rules.
 *
 * Reading the rules rather than a computed value: what is being tested is that
 * the page is painted from the rebuilt sheet, and `document.styleSheets` says
 * that without depending on what any one declaration is supposed to do.
 */
const hotRulePresent = (page: Page): Promise<boolean> =>
  page.evaluate(() =>
    [...document.styleSheets].some((sheet) => {
      try {
        return [...sheet.cssRules].some((r) => r.cssText.includes("--cadpole-hot"));
      } catch {
        // A sheet whose rules cannot be read is not one of ours.
        return false;
      }
    }),
  );

/** Where the page's one `<link rel=stylesheet>` currently points. */
const styleHref = (page: Page): Promise<string> =>
  page.evaluate(() => document.querySelector<HTMLLinkElement>('link[rel="stylesheet"]')?.href ?? "");

/** The bar's message, without the text of its (possibly hidden) buttons. */
const barText = (page: Page) => page.locator(".build-bar-text");

/** Set a number parameter from the panel, the way a user would. */
/** `fill` alone: it fires the one `input` event a typed value fires, and a second, dispatched by hand, commits the edit twice (engine.spec.ts). */
async function setNumber(page: Page, label: string, value: number): Promise<void> {
  await page.getByLabel(label, { exact: true }).fill(String(value));
}

test.describe("a rebuilt bundle", () => {
  test.beforeEach(async ({ page }) => {
    // The dev server's session survives between tests, and so does whatever a
    // previous test left in the parameters.
    await page.goto("/?nosw=1");
    await ready(page);
    const reset = page.getByRole("button", { name: "Reset all" });
    if (await reset.isVisible()) {
      const before = await evals(page);
      await reset.click();
      await expect.poll(() => evals(page), { timeout: 20_000 }).toBeGreaterThan(before);
    }
  });

  test("a stylesheet change is swapped in, and costs nothing", async ({ page }) => {
    await page.goto("/?nosw=1");
    await ready(page);
    await markPage(page);
    const evalsBefore = await evals(page);
    const epochBefore = await engineEpoch(page);
    expect(await hotRulePresent(page)).toBe(false);

    await rebuild(page, "styles");

    // The `<link>` now points at the rebuilt stylesheet...
    await expect.poll(() => styleHref(page), { timeout: 20_000 }).toContain("styles.css?b=");
    // ...and its rules are the ones the document is painted from.
    await expect.poll(() => hotRulePresent(page), { timeout: 10_000 }).toBe(true);
    // ...by the page that was already open. Not a reload with a new stylesheet.
    expect(await stillTheSamePage(page)).toBe(true);
    // Nothing else moved: no engine was replaced and nothing was recomputed,
    // because nothing about the model changed.
    expect(await engineEpoch(page)).toBe(epochBefore);
    expect(await evals(page)).toBe(evalsBefore);
    // And the user is told, once, without being asked for anything.
    await expect(barText(page)).toHaveText("styles updated without reloading");
    await expect(page.locator(".build-bar-reload")).toBeHidden();
  });

  test("an engine change swaps the worker, keeps the page, and recomputes the model", async ({ page }) => {
    await page.goto("/?nosw=1");
    await ready(page);
    await markPage(page);
    // State that a reload would have thrown away, none of which lives on the
    // server: a half-typed message and a display mode.
    await page.getByLabel("Message", { exact: true }).fill("half a thought");
    await page.getByLabel("Display mode").selectOption("wireframe");

    const evalsBefore = await evals(page);
    expect(await engineEpoch(page)).toBe(1);

    await rebuild(page, "engine");

    // A second engine was installed...
    await expect.poll(() => engineEpoch(page), { timeout: 60_000 }).toBe(2);
    // ...and the model was computed again by it, because the triangles on
    // screen had been produced by the engine that was just retired.
    await expect.poll(() => evals(page), { timeout: 60_000 }).toBeGreaterThan(evalsBefore);
    await expect.poll(() => modelEpoch(page), { timeout: 60_000 }).toBe(2);
    await expect(page.locator("html")).toHaveAttribute("data-engine", "worker");
    await expect(page.getByRole("tree", { name: "Feature tree" })).toContainText("plate");

    // The page itself never went away, and neither did anything on it.
    expect(await stillTheSamePage(page)).toBe(true);
    await expect(page.getByLabel("Message", { exact: true })).toHaveValue("half a thought");
    await expect(page.getByLabel("Display mode")).toHaveValue("wireframe");
    await expect(page.locator(".build-bar-reload")).toBeHidden();
  });

  test("an engine change during an evaluation drops the old engine's answer", async ({ page }) => {
    await page.goto("/?nosw=1");
    await ready(page);
    await markPage(page);

    // The in-flight job is put on the *server* deliberately: it is the harder
    // case. Terminating a worker means an abandoned local evaluation can never
    // produce anything, so it proves nothing about dropping a late result. A
    // remote evaluation really does keep running and really does come back --
    // after the swap -- so this is where "a result from before the swap is
    // never displayed as if it came from after it" is actually tested.
    const beforeRemote = await evals(page);
    await page.getByLabel("Where evaluation runs").selectOption("remote");
    await expect.poll(() => evals(page), { timeout: 30_000 }).toBeGreaterThan(beforeRemote);
    const beforeSlow = await evals(page);
    await setNumber(page, "server_slow_ms (ms)", 3000);
    await expect.poll(() => evals(page), { timeout: 30_000 }).toBeGreaterThan(beforeSlow);

    // Start an evaluation that will take three seconds, and rebuild the engine
    // while it is in the air.
    const beforeSwap = await evals(page);
    await setNumber(page, "size (mm)", 42);
    await expect(page.locator("html")).toHaveAttribute("data-evaluating", "1");
    await rebuild(page, "engine");

    await expect.poll(() => engineEpoch(page), { timeout: 60_000 }).toBe(2);
    // Everything settles, and what settles came from the new engine: the
    // displayed model's epoch is the live one, never the retired one.
    await expect
      .poll(() => page.evaluate(() => document.documentElement.dataset["evaluating"]), { timeout: 60_000 })
      .toBe("0");
    await expect.poll(() => evals(page), { timeout: 60_000 }).toBeGreaterThan(beforeSwap);
    expect(await modelEpoch(page)).toBe(2);
    // ...and the value on screen is the one the user last asked for, not
    // whichever evaluation happened to finish last.
    await expect(page.getByLabel("size (mm)")).toHaveValue("42");
    await expect(page.locator(".status-item.eval")).not.toContainText("failed");
    expect(await stillTheSamePage(page)).toBe(true);
  });

  test("glue-only changes swap the engine without recomputing anything", async ({ page }) => {
    await page.goto("/?nosw=1");
    await ready(page);
    await markPage(page);
    const evalsBefore = await evals(page);

    // `cadpole_wasm.js` is wasm-bindgen's marshalling, regenerated around the
    // same engine. The engine is replaced so the page is genuinely on the new
    // build, and nothing is recomputed because the answers cannot have moved.
    await rebuild(page, "glue");

    await expect.poll(() => engineEpoch(page), { timeout: 60_000 }).toBe(2);
    await expect(barText(page)).toHaveText("the engine bindings updated without reloading");
    expect(await stillTheSamePage(page)).toBe(true);
    // Give a stray evaluation a chance to show up before claiming there was none.
    await page.waitForTimeout(1500);
    expect(await evals(page)).toBe(evalsBefore);
  });

  test("a rebuilt engine worker is swapped in without recomputing anything", async ({ page }) => {
    await page.goto("/?nosw=1");
    await ready(page);
    await markPage(page);
    const evalsBefore = await evals(page);

    // `engine-worker.js` is the bundle the engine runs *in*, and the
    // one file a running page can genuinely replace -- replacing it is what
    // `startReplacementEngine` does. It must be part of the engine's
    // cache-busting token, or a build in which only the worker moved would
    // re-fetch the same URL and be handed the copy it was replacing.
    await rebuild(page, "worker");

    await expect.poll(() => engineEpoch(page), { timeout: 60_000 }).toBe(2);
    expect(await stillTheSamePage(page)).toBe(true);
    await expect(page.locator(".build-bar-reload")).toBeHidden();
    // The wasm did not move, so the answers cannot have: nothing is recomputed.
    await page.waitForTimeout(1500);
    expect(await evals(page)).toBe(evalsBefore);
    await expect(page.getByRole("tree", { name: "Feature tree" })).toContainText("plate");
  });

  test("an app bundle change is offered, not taken, and 'not now' sticks", async ({ page }) => {
    await page.goto("/?nosw=1");
    await ready(page);
    await markPage(page);
    // A keypress is all it takes to count as "the user is here"; bare Shift is
    // one the app itself ignores.
    await page.keyboard.press("Shift");

    await rebuild(page, "app");

    // Offered, with both answers on it, and the page still the same page.
    const bar = page.locator(".build-bar");
    await expect(bar).toBeVisible({ timeout: 20_000 });
    await expect(barText(page)).toContainText("app.js changed");
    await expect(page.locator(".build-bar-reload")).toBeVisible();
    await expect(page.locator(".build-bar-later")).toBeVisible();
    expect(await stillTheSamePage(page)).toBe(true);

    // "Not now" hides the interruption and not the fact.
    await page.locator(".build-bar-later").click();
    await expect(bar).toHaveClass(/dismissed/);
    await expect(barText(page)).toHaveText("New build available");
    await expect(page.locator(".build-bar-later")).toBeHidden();
    await expect(page.locator(".build-bar-reload")).toBeVisible();
    await expect(page.locator("html")).toHaveAttribute("data-stale-build", "1");

    // ...and the page goes on working: the offer is not a stop-work order.
    const before = await evals(page);
    const revBefore = await revision(page);
    await setNumber(page, "size (mm)", 33);
    await expect.poll(() => evals(page), { timeout: 30_000 }).toBeGreaterThan(before);
    expect(await stillTheSamePage(page)).toBe(true);

    // "Not now" is still the answer. The parameter write came back with the
    // session, and the session names the build this page is behind -- it does
    // on every refresh, for as long as the offer stands -- and that is not a
    // new offer: the chip stays a chip, the buttons stay away, and the screen
    // reader is not told about the same build a second time.
    await expect.poll(() => revision(page), { timeout: 30_000 }).toBeGreaterThan(revBefore);
    await expect(bar).toHaveClass(/dismissed/);
    await expect(barText(page)).toHaveText("New build available");
    await expect(page.locator(".build-bar-later")).toBeHidden();
    // The same for a selection change, the other write every click makes. A
    // selection is not a document change, so the revision does not move: wait
    // for the write's response instead, and a beat for the refresh it carries
    // to be applied, which is when the offer would come back.
    const selected = page.waitForResponse((r) => r.url().endsWith("/SetSelection") && r.ok(), { timeout: 30_000 });
    await page.getByRole("tree", { name: "Feature tree" }).getByRole("treeitem").first().click();
    await selected;
    await page.waitForTimeout(500);
    await expect(bar).toHaveClass(/dismissed/);
    await expect(page.locator(".build-bar-later")).toBeHidden();

    // A stylesheet rebuilt while the offer stands is still taken, because it
    // still costs nothing...
    await rebuild(page, "styles");
    await expect.poll(() => hotRulePresent(page), { timeout: 20_000 }).toBe(true);
    const href = await styleHref(page);
    // ...and it is taken *once*. The page records what it is actually running,
    // file by file, so the next rebuild of something else diffs against the
    // stylesheet it has rather than the one it had before the offer went up.
    // Without that it would swap the same stylesheet again on every rebuild
    // for as long as the user left the bar alone.
    await rebuild(page, "glue");
    await page.waitForTimeout(1500);
    expect(await styleHref(page)).toBe(href);
    // Still behind, still saying so.
    await expect(page.locator("html")).toHaveAttribute("data-stale-build", "1");
  });

  test("the offer says what the reload will cost, when it costs something the user made", async ({ page }) => {
    await page.goto("/?nosw=1");
    await ready(page);
    await markPage(page);
    // Shift+S queues a snapshot of the viewport for the next message. It is
    // the one thing in the reload's path that the user built by hand and that
    // the reload neither parks nor lists: base64 PNGs, too big to park safely,
    // so the honest thing is to say so before they go.
    await page.keyboard.press("Shift+S");
    await expect(page.locator(".attach-thumb")).toHaveCount(1);

    await rebuild(page, "app");

    await expect(page.locator(".build-bar-reload")).toBeVisible({ timeout: 20_000 });
    await expect(barText(page)).toContainText("1 queued snapshot will be lost");
    expect(await stillTheSamePage(page)).toBe(true);
  });

  test("the reload it cannot avoid puts back more than the camera", async ({ page }) => {
    await page.goto("/?nosw=1");
    await ready(page);
    await markPage(page);
    await page.keyboard.press("Shift");
    await page.getByLabel("Display mode").selectOption("wireframe");
    await page.getByLabel("Message", { exact: true }).fill("kept across the reload");

    await rebuild(page, "app");
    await expect(page.locator(".build-bar-reload")).toBeVisible({ timeout: 20_000 });
    await page.locator(".build-bar-reload").click();

    // It really did reload...
    await expect.poll(() => stillTheSamePage(page), { timeout: 30_000 }).toBe(false);
    await ready(page);
    // ...and came back with the UI state a reload would otherwise lose.
    await expect(page.getByLabel("Display mode")).toHaveValue("wireframe");
    await expect(page.getByLabel("Message", { exact: true })).toHaveValue("kept across the reload");
    // Nothing is left over: the page is on the new build and says nothing.
    await expect(page.locator(".build-bar")).toBeHidden();
    await expect(page.locator("html")).not.toHaveAttribute("data-stale-build", "1");
  });

  test("a page nobody is using takes the reload without asking", async ({ page }) => {
    // Nothing here touches the page after it loads -- no click, no keypress --
    // which is the whole point: asking someone who is not there costs them a
    // stale tab and buys nothing.
    await page.goto("/?nosw=1");
    await ready(page);
    await markPage(page);

    await rebuild(page, "app");

    await expect.poll(() => stillTheSamePage(page), { timeout: 30_000 }).toBe(false);
    await ready(page);
    await expect(page.locator(".build-bar")).toBeHidden();
  });

  /**
   * What each kind of update costs, against the reload it replaces.
   *
   * The numbers go in the log rather than in an assertion, because they are
   * hardware: what is asserted is the claim that motivated the feature, which
   * is that the cheap cases are *cheap* -- a stylesheet change costs
   * milliseconds where a reload costs a page boot and 4.5 MB of compressed
   * wasm. The engine swap is deliberately not asserted to beat the reload on
   * the clock: both fetch the rebuilt engine and both re-evaluate, so what it
   * wins is the page boot and everything on the page, not seconds.
   *
   * `polling: "raf"` rather than `expect.poll`, whose backoff would measure
   * its own sleep.
   */
  test("what each kind of update costs", async ({ page }) => {
    const settleTimeout = 60_000;
    const report: Record<string, number> = {};

    /**
     * Give the local engine real work, when the document can take it.
     *
     * `engine.star` is a box and evaluates in milliseconds, which would make
     * the re-evaluation after an engine swap invisible next to the cost of
     * fetching a 20 MB engine. Its `holes` parameter drills N holes through a
     * pairwise `subtract`, which is quadratic, so it buys seconds
     * cheaply. Skipped when `CADPOLE_E2E_HOT_SCRIPT` points the server at some
     * other document, which is how the base lid gets measured.
     */
    const makeItHeavy = async (): Promise<void> => {
      if ((await page.locator("#param-holes").count()) === 0) {
        console.log("  (no `holes` parameter: measuring this document as it is)");
        return;
      }
      const before = await evals(page);
      await setNumber(page, "holes", 32);
      await expect.poll(() => evals(page), { timeout: settleTimeout }).toBeGreaterThan(before);
    };

    /**
     * A page loaded, pinned to the local engine, and given real work.
     *
     * Pinned, because what is being measured is a *local* engine being
     * replaced. Left on `auto`, a document that takes 900 ms locally is
     * promoted to the server on its first evaluation, and then the
     * re-evaluation after a swap is the server's 7 ms and measures nothing at
     * all about the engine that was swapped.
     */
    const heavyLocalPage = async (): Promise<void> => {
      await page.goto("/?nosw=1");
      await ready(page);
      await page.getByLabel("Where evaluation runs").selectOption("local");
      await makeItHeavy();
      console.log(`  (document: ${await page.locator(".status-item.eval").innerText()})`);
    };

    const measure = async (what: "styles" | "engine" | "glue" | "app", settled: () => Promise<unknown>): Promise<number> => {
      const t0 = Date.now();
      await rebuild(page, what);
      await settled();
      return Date.now() - t0;
    };

    // (1) A stylesheet: the `<link>` is pointed at the new one.
    await page.goto("/?nosw=1");
    await ready(page);
    report["styles"] = await measure("styles", () =>
      page.waitForFunction(
        () => (document.querySelector<HTMLLinkElement>('link[rel="stylesheet"]')?.getAttribute("href") ?? "").includes("?b="),
        null,
        { polling: "raf", timeout: settleTimeout },
      ),
    );

    // (2) The wasm engine: a new worker is live, and the model has been
    // computed again by it. Both halves, because the second is the visible one.
    await heavyLocalPage();
    const evalsBefore = await evals(page);
    report["engine (live)"] = await measure("engine", () =>
      page.waitForFunction(() => document.documentElement.dataset["engineEpoch"] === "2", null, {
        polling: "raf",
        timeout: settleTimeout,
      }),
    );
    const afterLive = Date.now();
    await page.waitForFunction(
      (n) => Number(document.documentElement.dataset["evals"] ?? 0) > n && document.documentElement.dataset["evaluating"] === "0",
      evalsBefore,
      { polling: "raf", timeout: settleTimeout },
    );
    report["engine (+ re-evaluated)"] = report["engine (live)"] + (Date.now() - afterLive);

    // (3) The glue alone: a new engine, and nothing recomputed.
    await page.goto("/?nosw=1");
    await ready(page);
    report["glue"] = await measure("glue", () =>
      page.waitForFunction(() => document.documentElement.dataset["engineEpoch"] === "2", null, {
        polling: "raf",
        timeout: settleTimeout,
      }),
    );

    // (4) The reload this replaces: an untouched page takes it immediately, so
    // this is the whole cost -- page boot, engine load, evaluation. The same
    // document and the same engine as (2), so the two are comparable.
    await heavyLocalPage();
    await markPage(page);
    report["reload"] = await measure("app", async () => {
      await page.waitForFunction(() => (window as unknown as Record<string, unknown>)["__hotMark"] !== "alive", null, {
        polling: "raf",
        timeout: settleTimeout,
      });
      await page.waitForFunction(
        () => document.documentElement.classList.contains("ready") && Number(document.documentElement.dataset["evals"] ?? 0) > 0,
        null,
        { polling: "raf", timeout: settleTimeout },
      );
    });

    console.log(`hot reload, build change to settled:\n${Object.entries(report)
      .map(([k, v]) => `  ${k.padEnd(24)} ${v} ms`)
      .join("\n")}`);

    // The claim: replacing a stylesheet is not in the same class of cost as
    // reloading the page, which is why it happens without asking.
    expect(report["styles"]).toBeLessThan(report["reload"] as number);
    expect(report["styles"]).toBeLessThan(3_000);
  });
});

/**
 * An edit made in the last moment before the page goes away.
 *
 * Parameter overrides are session state on the server, written
 * through a 300 ms trailing debounce. Unless the page flushes it, a slider
 * moved inside that window and followed by "Reload now" or by a navigation
 * is lost, and the page comes back showing the value the user had just
 * changed.
 */
test.describe("an edit inside the debounce window", () => {
  /** Every `SetParams` this page has sent, so "the write has not gone yet" can be asserted. */
  function countWrites(page: Page): () => number {
    let n = 0;
    page.on("request", (r) => {
      if (r.url().endsWith("/SetParams")) n += 1;
    });
    return () => n;
  }

  test("survives Reload now", async ({ page }) => {
    const writes = countWrites(page);
    await page.goto("/?nosw=1");
    await ready(page);
    await markPage(page);
    // A keypress is what makes the build bar ask rather than just reload.
    await page.keyboard.press("Shift");
    await rebuild(page, "app");
    await expect(page.locator(".build-bar-reload")).toBeVisible({ timeout: 20_000 });

    const before = writes();
    await setNumber(page, "size (mm)", 37);
    // The precondition this test is about: the write is still on its timer.
    expect(writes(), "the debounce had already fired; this proves nothing").toBe(before);
    await page.locator(".build-bar-reload").click();

    await expect.poll(() => stillTheSamePage(page), { timeout: 30_000 }).toBe(false);
    await ready(page);
    await expect(page.getByLabel("size (mm)")).toHaveValue("37");
  });

  test("survives a navigation", async ({ page }) => {
    const writes = countWrites(page);
    await page.goto("/?nosw=1");
    await ready(page);

    const before = writes();
    await setNumber(page, "size (mm)", 41);
    expect(writes(), "the debounce had already fired; this proves nothing").toBe(before);
    // Straight out of the page, the way closing a tab does it.
    await page.goto("/?nosw=1");
    await ready(page);
    await expect(page.getByLabel("size (mm)")).toHaveValue("41");
  });
});

/**
 * The same swaps with the service worker in charge of the page's fetches.
 *
 * Everything above runs with `?nosw=1`, so it never sees what the worker does
 * to a hot swap. On 127.0.0.1 the context is secure and
 * the worker registers, so this is the case a developer on the dev box gets.
 * The worker precaches the bundle and answers static assets
 * stale-while-revalidate; a hot swap fetches `styles.css?b=<token>`, and a
 * cache match that ignored the query would answer it with the *precached*
 * sheet -- the old one -- while the page printed "styles updated without
 * reloading".
 */
test.describe("a rebuilt bundle, with the service worker in charge", () => {
  test.beforeEach(async ({ page }) => {
    // The same reset as above: the server's one session outlives every test.
    await page.goto("/?nosw=1");
    await ready(page);
    const reset = page.getByRole("button", { name: "Reset all" });
    if (await reset.isVisible()) {
      const before = await evals(page);
      const written = page.waitForResponse((r) => r.url().endsWith("/SetParams") && r.ok(), { timeout: 20_000 });
      await reset.click();
      await written;
      await expect.poll(() => evals(page), { timeout: 20_000 }).toBeGreaterThan(before);
    }
  });

  /** The `--cadpole-hot` markers a stylesheet carries, in order: one per rebuild the server has done. */
  const marks = (css: string): number[] => [...css.matchAll(/--cadpole-hot: "(\d+)"/g)].map((m) => Number(m[1]));

  /** The markers in the sheet the document is actually painted from. */
  const documentMarks = (page: Page): Promise<number[]> =>
    page.evaluate(() => {
      const out: number[] = [];
      for (const sheet of document.styleSheets) {
        if (!(sheet.href ?? "").includes("styles.css")) continue;
        for (const r of sheet.cssRules) {
          const m = /--cadpole-hot: "(\d+)"/.exec(r.cssText);
          if (m) out.push(Number(m[1]));
        }
      }
      return out;
    });

  /** Load the page without `?nosw=1` and wait until the worker controls it, precache filled. */
  async function controlled(page: Page): Promise<void> {
    await page.goto("/");
    await ready(page);
    // Registered after the first evaluation; it precaches on install and
    // claims every open page on activate, so "controlled" means both.
    await page.waitForFunction(() => navigator.serviceWorker.controller !== null, null, { timeout: 30_000 });
  }

  test("the app shell loads offline at a URL with a query string", async ({ page }) => {
    await controlled(page);
    await page.context().setOffline(true);

    // `?mock=1` and `?static=1` are in the README, and a bookmark or an
    // installed shortcut can carry any query at all. The precache holds `./`
    // and `./index.html` only, and `networkFirst`'s cache fallback matched
    // the request URL exactly -- so bare `/` came up offline while every one
    // of these failed outright with ERR_FAILED. `staleWhileRevalidate` has
    // had the navigation fallback all along; this is the same page.
    await page.goto("/?mock=1");
    await ready(page);
    await expect(page.locator(".pill-conn")).toHaveText("static");

    await page.context().setOffline(false);
  });

  test("a stylesheet swap is served the rebuilt sheet, not the precached one", async ({ page }) => {
    await controlled(page);
    await markPage(page);

    await rebuild(page, "styles");
    await expect.poll(() => styleHref(page), { timeout: 20_000 }).toContain("styles.css?b=");
    await expect(barText(page)).toHaveText("styles updated without reloading");
    expect(await stillTheSamePage(page)).toBe(true);

    // What the server serves now, read past the browser and its worker...
    const server = marks(await (await page.request.get("/styles.css")).text());
    expect(server.length).toBeGreaterThan(0);
    // ...is what the document is painted from. A worker that answers the
    // `?b=` request out of its cache leaves the document on the precached
    // sheet, and the receipt above is then a lie.
    await expect.poll(() => documentMarks(page), { timeout: 10_000 }).toEqual(server);
  });
});
