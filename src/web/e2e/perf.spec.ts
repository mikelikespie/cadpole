// What the page does with an evaluation, measured.
//
// It drives the edits that matter -- a first load, a change to one body of
// forty, a change to all forty, and a script edit that shifts every body id
// without moving a triangle -- and prints the phase breakdown the app records
// under `?perf=1`.
//
// **What is asserted and what is only printed.** Wall-clock budgets on a
// shared CI box are how a benchmark becomes a flaky test and then a disabled
// one, so the assertions here are *counts*: how many bodies were reused
// rather than rebuilt, how many GPU-side geometries were created, whether the
// change flash fired. Those are facts about what the code did, identical on a
// fast machine and a loaded one. The times are printed, and there is a single
// deliberately loose ceiling on each case to catch a catastrophe -- a return
// to seconds, not a drift of five milliseconds.
//
// The document is `e2e/perf.star`: 40 bodies and 35 520 triangles, within 1 %
// of the base lid's triangle count but with forty bodies instead of its one,
// which is the case where reusing what did not change can pay at all.

import { expect, test, type Page } from "@playwright/test";
import { appendFileSync, mkdirSync } from "node:fs";
import { dirname, resolve } from "node:path";

interface PerfSpan {
  name: string;
  ms: number;
  at: number;
}

interface PerfLongTask {
  ms: number;
  at: number;
  blame: string;
}

interface PerfReport {
  spans: PerfSpan[];
  longTasks: PerfLongTask[];
  counts: Record<string, number>;
}

/** One case's numbers, as the table prints them. */
interface Case {
  model: string;
  what: string;
  /** The apply task: worker message, parse, normalise, rebuild, panels. */
  applyMs: number;
  /** The frame after it, where the GPU buffers are created and filled. */
  firstFrameMs: number;
  /** The worst single task the browser itself called long (>= 50 ms), if any. */
  longestTaskMs: number;
  counts: Record<string, number>;
  phases: Record<string, number>;
  longTasks: PerfLongTask[];
  spanWindow: [number, number];
  spans: PerfSpan[];
}

const cases: Case[] = [];

const evals = (page: Page): Promise<number> =>
  page.evaluate(() => Number(document.documentElement.dataset["evals"] ?? 0));

const stages = (page: Page): Promise<number> =>
  page.evaluate(() => Number(document.documentElement.dataset["stages"] ?? 0));

/**
 * Forget what the engine policy measured about this document.
 *
 * A document whose last evaluation took 400 ms or more is refined coarse-first,
 * and the benchmark's document is nowhere near that -- except on a
 * loaded machine, where the wall clock of a three-millisecond evaluation can
 * cross it anyway. Two rungs would mean two applies per edit, the second of
 * which rebuilds every body by construction because the tolerance changed, and
 * the counts this suite asserts would depend on how busy the box was. So each
 * edit here starts from an unmeasured document, and `afterAnotherEval` checks
 * that it held: one result per evaluation, never two. Clearing the stored
 * verdicts reaches a page that is already running because the page reads them
 * from storage before every evaluation (`EngineMemory`); while it kept a copy
 * from its load instead, this cleared nothing that mattered.
 */
async function forgetPolicy(page: Page): Promise<void> {
  await page.evaluate(() => {
    try {
      localStorage.removeItem("cadpole.engineVerdicts.v1");
    } catch {
      /* storage disabled: the document was never measured anyway */
    }
  });
}

async function ready(page: Page): Promise<void> {
  await expect(page.locator("html")).toHaveClass(/ready/);
  await expect.poll(() => evals(page), { timeout: 60_000 }).toBeGreaterThan(0);
}

/** Two frames, so the frame that uploads the new geometry has happened. */
async function settle(page: Page): Promise<void> {
  await page.evaluate(
    () =>
      new Promise<void>((r) => {
        requestAnimationFrame(() => requestAnimationFrame(() => r()));
      }),
  );
}

async function takeReport(page: Page): Promise<PerfReport> {
  return page.evaluate(() => {
    const hook = (window as unknown as { __cadpolePerf?: { take(): PerfReport } }).__cadpolePerf;
    if (!hook) throw new Error("?perf=1 did not install the perf hook");
    return hook.take();
  });
}

function sumOf(spans: PerfSpan[], name: string): number {
  let t = 0;
  for (const s of spans) if (s.name === name) t += s.ms;
  return t;
}

function summarize(model: string, what: string, r: PerfReport): Case {
  const phases: Record<string, number> = {};
  for (const s of r.spans) phases[s.name] = (phases[s.name] ?? 0) + s.ms;
  const c: Case = {
    model,
    what,
    applyMs: sumOf(r.spans, "apply") + sumOf(r.spans, "engine.parse") + sumOf(r.spans, "engine.normalize"),
    firstFrameMs: sumOf(r.spans, "render.firstFrame"),
    longestTaskMs: r.longTasks.reduce((m, t) => Math.max(m, t.ms), 0),
    counts: r.counts,
    phases,
    longTasks: r.longTasks,
    spans: r.spans,
    spanWindow: [
      r.spans.reduce((m, x) => Math.min(m, x.at - x.ms), Infinity),
      r.spans.reduce((m, x) => Math.max(m, x.at), 0),
    ],
  };
  cases.push(c);
  return c;
}

/**
 * Set a number parameter from the panel, the way a user would.
 *
 * By id rather than by label: a parameter with a unit gets an accessible name
 * of `one (mm)`, and the benchmark should not break when someone adds a unit.
 *
 * `fill` and nothing after it. On a number input `fill` selects the text and
 * types the value, which fires the one `input` event a typed value fires; a
 * second, dispatched by hand, commits the same edit again. The two usually
 * land inside the 120 ms evaluation debounce and cost nothing, but on a busy
 * machine they do not, and then one edit is two evaluations: the second
 * reuses every body, and whichever report it lands in -- this edit's or the
 * next one's -- counts a whole model too many.
 */
async function setNumber(page: Page, name: string, value: number): Promise<void> {
  await page.locator(`#param-${name}`).fill(String(value));
}

async function afterAnotherEval(page: Page, from: number): Promise<void> {
  await expect.poll(() => evals(page), { timeout: 60_000 }).toBeGreaterThan(from);
  await settle(page);
  await expect.poll(() => stages(page), { timeout: 5_000 }).toBe(await evals(page));
  // `longtask` entries reach the observer a task or two after the task that
  // caused them; without this they land in the next case's report.
  await page.waitForTimeout(100);
}

/// A flash runs for 1.4 s and paints the bodies it marks, so a picture taken
/// during one says nothing about their geometry.
const FLASH_MS = 1400;

async function canvasPixels(page: Page): Promise<Buffer> {
  return page.locator("canvas.viewport-canvas").screenshot();
}

async function resetOverrides(page: Page): Promise<void> {
  const reset = page.getByRole("button", { name: "Reset all" });
  if (await reset.isVisible()) {
    const before = await evals(page);
    await forgetPolicy(page);
    await reset.click();
    await afterAnotherEval(page, before);
  }
}

/**
 * A page that has measured nothing about this document, and will evaluate in
 * the worker whatever it measures.
 *
 * Pinning the engine is not tidiness. `Auto` promotes a document to the server
 * once a local evaluation takes 750 ms or more, and the dev server's
 * `Evaluate` is a stand-in that answers every document with the same
 * 24-triangle box -- so an unpinned benchmark on a heavy model silently stops
 * measuring the model it names. That is the
 * argument for the suite asserting triangle counts as well as times.
 */
async function openFresh(page: Page): Promise<void> {
  await page.addInitScript(() => {
    try {
      localStorage.removeItem("cadpole.engineVerdicts.v1");
      localStorage.setItem("cadpole.enginePreference", "local");
    } catch {
      /* storage disabled */
    }
  });
  await page.goto("/?nosw=1&perf=1");
  await ready(page);
}

/** Move a parameter and wait for the model it produces. */
async function edit(page: Page, name: string, value: number): Promise<void> {
  const before = await evals(page);
  await forgetPolicy(page);
  // Start this case's log here. Screenshots and navigations the harness does
  // between cases are long tasks of their own, and they are not the app's.
  await takeReport(page);
  await setNumber(page, name, value);
  await afterAnotherEval(page, before);
}

test.describe.configure({ mode: "serial" });

test.describe("the apply path", () => {
  test("40 bodies, 35 520 triangles: first load, one body, every body", async ({ page }) => {
    await openFresh(page);
    await resetOverrides(page);

    // ---- first load: a fresh page, nothing on screen to reuse.
    await openFresh(page);
    await settle(page);
    const first = summarize("40 bodies / 35 520 tris", "first load", await takeReport(page));
    expect(first.counts["viewer.bodyRebuilt"], "nothing can be reused on the first model").toBe(40);
    expect(first.counts["viewer.bodyReused"] ?? 0).toBe(0);
    expect(first.counts["viewer.bodyFlashed"] ?? 0, "a whole scene lighting up says nothing").toBe(0);
    const beforePixels = await canvasPixels(page);

    // ---- a parameter change that moves exactly one body.
    await edit(page, "one", 2);
    const oneBody = summarize("40 bodies / 35 520 tris", "one body changed", await takeReport(page));
    expect(oneBody.counts["viewer.bodyReused"], "thirty-nine bodies did not move").toBe(39);
    expect(oneBody.counts["viewer.bodyRebuilt"]).toBe(1);
    // The flash is the user's signal that something happened, so it has to
    // fire for what changed and stay off for what did not.
    expect(oneBody.counts["viewer.bodyFlashed"], "exactly the body that moved").toBe(1);

    // ---- a parameter change that moves every body.
    await edit(page, "every", 2);
    const all = summarize("40 bodies / 35 520 tris", "every body changed", await takeReport(page));
    expect(all.counts["viewer.bodyRebuilt"], "every body moved, so every body is rebuilt").toBe(40);
    expect(all.counts["viewer.bodyReused"] ?? 0).toBe(0);
    expect(all.counts["viewer.bodyFlashed"]).toBe(40);

    // Reusing a body's scene node means handing the renderer a new
    // `BufferGeometry` for a `Mesh` it already knows, which is the one way
    // this could go wrong without anything throwing: a stale mesh on screen.
    // So look at the pixels, once the flash has faded so that it is the
    // geometry being compared and not the colour.
    await page.waitForTimeout(FLASH_MS + 300);
    const afterPixels = await canvasPixels(page);
    expect(Buffer.compare(beforePixels, afterPixels), "every body grew; the picture must have changed").not.toBe(0);

    // The same two edits again: whatever a renderer pays once for a shader or
    // a pipeline it has already paid, so a second identical edit separates
    // one-time cost from per-edit cost.
    await edit(page, "one", 4);
    summarize("40 bodies / 35 520 tris", "one body, again", await takeReport(page));
    await edit(page, "every", 4);
    summarize("40 bodies / 35 520 tris", "every body, again", await takeReport(page));

    // A catastrophe ceiling, not a budget: this is about never going back to
    // seconds, and it has to hold on a loaded CI box with a software
    // rasteriser, so it is deliberately an order of magnitude loose.
    for (const c of [first, oneBody, all]) {
      expect(c.applyMs, `${c.what}: apply task`).toBeLessThan(1_000);
    }
  });

  test("a light model costs almost nothing", async ({ page }) => {
    await openFresh(page);
    await resetOverrides(page);

    // Four bodies, 3 552 triangles. Set it, then reload: parameter overrides
    // are session state, so the reload's *first* evaluation is the light one
    // and this is a real first load rather than a rebuild.
    await edit(page, "count", 4);
    await openFresh(page);
    await settle(page);
    const first = summarize("4 bodies / 3 552 tris", "first load", await takeReport(page));
    expect(first.counts["viewer.bodyRebuilt"]).toBe(4);
    expect(first.counts["viewer.bodyFlashed"] ?? 0).toBe(0);

    await edit(page, "one", 2);
    const oneBody = summarize("4 bodies / 3 552 tris", "one body changed", await takeReport(page));
    expect(oneBody.counts["viewer.bodyReused"]).toBe(3);
    expect(oneBody.counts["viewer.bodyFlashed"]).toBe(1);

    await edit(page, "every", 2);
    const all = summarize("4 bodies / 3 552 tris", "every body changed", await takeReport(page));
    expect(all.counts["viewer.bodyRebuilt"]).toBe(4);
    expect(all.counts["viewer.bodyFlashed"]).toBe(4);

    expect(first.applyMs, "a light model's apply").toBeLessThan(500);

    // Leave the session as it was found: the parameters are shared state and
    // the next run's "first load" must be the 40-body one.
    await resetOverrides(page);
  });
});

/// Eight bodies, written so that the test can shift every id under them.
function pins(height: number, lead = ""): string {
  return `${lead}for i in range(8):\n    cylinder(3.0, ${height.toFixed(1)}, at=(i * 9.0, 0.0, 0.0), name="pin_%d" % i)\n`;
}

/** Type a new script into the static playground's editor and apply it. */
async function applyScript(page: Page, source: string): Promise<void> {
  const before = await evals(page);
  await forgetPolicy(page);
  await takeReport(page);
  // The editor lives in the Script tab, and only the open tab is visible.
  await page.getByRole("tab", { name: "Script" }).click();
  await page.getByRole("button", { name: "Edit", exact: true }).click();
  await page.locator(".script-editor").fill(source);
  await page.locator(".script-editor").press("Control+Enter");
  await expect.poll(() => evals(page), { timeout: 60_000 }).toBeGreaterThan(before);
  await settle(page);
  await page.waitForTimeout(100);
}

test.describe("a script edit", () => {
  /**
   * The case a parameter slider cannot reach: an edit that inserts a feature
   * *earlier* in the script, so every body id shifts (`pin_0#0` becomes
   * `pin_0#1`) while the geometry under it is untouched. Getting this wrong
   * either flashes the whole model on every edit -- which is why the change
   * flash keys on the body's name -- or, worse, reuses the wrong geometry.
   *
   * The static playground, because that is where the script editor is: in
   * session mode the script panel is read-only.
   */
  test("shifting every body id keeps the bodies whose triangles did not move", async ({ page }) => {
    await page.addInitScript(() => {
      try {
        localStorage.clear();
        localStorage.setItem("cadpole.enginePreference", "local");
      } catch {
        /* storage disabled */
      }
    });
    await page.goto("/?nosw=1&static=1&perf=1");
    await expect(page.locator("html")).toHaveClass(/ready/);
    await expect.poll(() => evals(page), { timeout: 60_000 }).toBeGreaterThan(0);

    await applyScript(page, pins(10));
    const first = summarize("8 bodies / 7 104 tris", "a new script", await takeReport(page));
    expect(first.counts["viewer.bodyRebuilt"]).toBe(8);

    // One new feature at the top. Every id below it moves; no triangle does.
    await applyScript(page, pins(10, 'cylinder(1.0, 1.0, at=(-40.0, 0.0, 0.0), name="spacer")\n'));
    const shifted = summarize("8 bodies / 7 104 tris", "a feature inserted first", await takeReport(page));
    expect(shifted.counts["viewer.bodyReused"], "the pins did not move, whatever their ids say").toBe(8);
    expect(shifted.counts["viewer.bodyRebuilt"], "only the new body").toBe(1);
    expect(shifted.counts["viewer.bodyFlashed"], "the new one, and nothing else").toBe(1);

    // And an edit that really does change every pin.
    await applyScript(page, pins(13, 'cylinder(1.0, 1.0, at=(-40.0, 0.0, 0.0), name="spacer")\n'));
    const changed = summarize("8 bodies / 7 104 tris", "every body changed", await takeReport(page));
    expect(changed.counts["viewer.bodyRebuilt"]).toBe(8);
    expect(changed.counts["viewer.bodyReused"], "the spacer is the one thing that did not move").toBe(1);
    expect(changed.counts["viewer.bodyFlashed"]).toBe(8);
  });
});

/**
 * The heavy single-body model, opt-in because it is slow.
 *
 * It is here for two reasons beyond the headline number. It is **one body**,
 * so it is the case where reusing unchanged bodies can do nothing at all --
 * what it measures is the part of the apply path that no amount of reuse
 * reaches. And it is heavy enough that the engine policy refines it
 * coarse-first, so its second evaluation applies twice and the
 * second apply rebuilds by construction: that is what a heavy document really
 * costs, and the fast suite deliberately does not.
 */
test.describe("the apply path, base lid", () => {
  test("base lid: one body, 35 742 triangles", async ({ page }) => {
    await openFresh(page);
    await settle(page);
    await page.waitForTimeout(100);
    const first = summarize("base lid / 35 742 tris", "first load", await takeReport(page));
    expect(first.counts["viewer.bodyRebuilt"]).toBe(1);
    expect(first.counts["viewer.triangles"], "the wasm engine evaluated the lid, not a stand-in").toBe(35_742);

    // One body means every edit is an edit to every body. `magnets` is the
    // parameter that moves the most geometry for the least evaluation --
    // without it the model is 807 ms native rather than 6.5 s -- so turning it
    // off and on again is two real edits at two model sizes, and the second
    // one lands the full 35 742 triangles.
    for (const on of [false, true]) {
      const before = await evals(page);
      await takeReport(page);
      await page.locator("#param-magnets").setChecked(on);
      await expect.poll(() => evals(page), { timeout: 400_000 }).toBeGreaterThan(before);
      await settle(page);
      await page.waitForTimeout(100);
      const changed = summarize("base lid / 35 742 tris", `magnets ${on ? "on" : "off"}`, await takeReport(page));
      // Which engine actually answered, and what it cost the user. This line
      // shows when the dev server's 24-triangle stand-in is standing in
      // for the base lid.
      console.log(`  base lid, magnets ${on ? "on" : "off"}: ${await page.locator(".status-item.eval").innerText()}`);
      // A heavy document is refined coarse-first, so one edit is two
      // applies, and the second compares against the coarse mesh the first put
      // up -- which is why nothing is reused here and the lid rebuilds twice.
      expect(changed.counts["viewer.bodyFlashed"], "the body changed, once per edit").toBe(1);
      expect(changed.counts["viewer.bodyReused"] ?? 0).toBe(0);
      if (on) expect(changed.counts["viewer.triangles"]).toBe(35_742);
    }
  });
});

test.afterAll(() => {
  if (cases.length === 0) return;
  const header = ["model", "case", "apply ms", "first frame ms", "longest task ms", "rebuilt", "reused"];
  const rows = cases.map((c) => [
    c.model,
    c.what,
    c.applyMs.toFixed(1),
    c.firstFrameMs.toFixed(1),
    c.longestTaskMs === 0 ? "-" : c.longestTaskMs.toFixed(0),
    String(c.counts["viewer.bodyRebuilt"] ?? 0),
    String(c.counts["viewer.bodyReused"] ?? 0),
  ]);
  const widths = header.map((h, i) => Math.max(h.length, ...rows.map((r) => (r[i] ?? "").length)));
  const line = (cells: string[]): string => cells.map((c, i) => c.padEnd(widths[i] ?? 0)).join("  ");
  const out = ["", "cadpole apply-path benchmark", line(header), line(widths.map((w) => "-".repeat(w)))];
  for (const r of rows) out.push(line(r));
  out.push("");
  for (const c of cases) {
    const phases = Object.entries(c.phases)
      .filter(([, ms]) => ms >= 0.05)
      .sort((a, b) => b[1] - a[1])
      .map(([k, ms]) => `${k} ${ms.toFixed(1)}`)
      .join(", ");
    const counts = Object.entries(c.counts)
      .map(([k, n]) => `${k}=${n}`)
      .join(" ");
    out.push(`  ${c.model} / ${c.what}: ${phases}`);
    out.push(`      counts: ${counts}`);
    if (c.longTasks.length) {
      out.push(
        `      long tasks: ${c.longTasks.map((t) => `${t.ms.toFixed(0)}ms at ${t.at.toFixed(0)} (${t.blame})`).join(", ")}` +
          `; spans ran ${c.spanWindow[0].toFixed(0)}..${c.spanWindow[1].toFixed(0)}`,
      );
    }
    // Every span with the moment it started, which is what tells a long task
    // the page really caused from one where the box was simply busy.
    if (process.env["CADPOLE_PERF_TRACE"]) {
      out.push(`      trace: ${c.spans.map((s) => `${s.name}@${(s.at - s.ms).toFixed(0)}+${s.ms.toFixed(1)}`).join(" ")}`);
    }
  }
  const text = out.join("\n");
  console.log(text);
  const dir = process.env["PLAYWRIGHT_OUTPUT_DIR"] ?? process.env["TEST_UNDECLARED_OUTPUTS_DIR"];
  if (dir) {
    const file = resolve(dir, "perf.txt");
    mkdirSync(dirname(file), { recursive: true });
    appendFileSync(file, `${text}\n`);
  }
});
