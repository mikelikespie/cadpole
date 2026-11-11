// The playground under slow evaluations: every reply from the engine worker
// reaches the page late and out of order, while tabs are switched and sliders
// dragged quickly. Afterwards the view must draw the selected tab's model,
// with that tab's script and sliders, and the same model a calm evaluation of
// that tab draws.
import { context, expect, test } from "./site.js";

type Play = { start(): void; selected(): string; shown(): { key: string; triangles: number } | null; state(): string };

test("playground: a late result never replaces the selected tab's model", async ({ browser, site }) => {
  const ctx = await context(browser, "desk-1440");
  // Hold every message from a worker for 150 to 900 ms, each its own delay,
  // so replies arrive late and can overtake each other.
  await ctx.addInitScript(() => {
    const Real = window.Worker;
    class Slow extends Real {
      override addEventListener(type: string, fn: EventListenerOrEventListenerObject, opts?: boolean | AddEventListenerOptions): void {
        if (type !== "message" || typeof fn !== "function") return super.addEventListener(type, fn, opts);
        super.addEventListener(type, (ev) => { setTimeout(() => fn.call(this, ev), 150 + Math.random() * 750); }, opts);
      }
    }
    window.Worker = Slow as typeof Worker;
  });
  const p = await ctx.newPage();
  const errs: string[] = [];
  p.on("pageerror", (e) => errs.push(String(e).slice(0, 200)));
  await p.goto(site + "?noauto=1", { waitUntil: "load" });
  await p.evaluate(() => { document.querySelector("#play")!.scrollIntoView({ block: "center", behavior: "instant" }); (window as unknown as { __play: Play }).__play.start(); });
  await p.waitForSelector(".play-view.is-live .live canvas", { timeout: 180_000 });
  const tabs = p.locator("#play .tab");
  const n = await tabs.count();
  expect(n).toBeGreaterThan(2);
  const play = () => p.evaluate(() => { const w = (window as unknown as { __play: Play }).__play; return { sel: w.selected(), shown: w.shown(), state: w.state(), live: document.querySelector(".play-view")!.classList.contains("is-live"), file: document.querySelector("#play [data-file]")!.textContent, script: (document.querySelector("#play .editor") as HTMLTextAreaElement).value, src: (document.getElementById(w.selected())?.textContent ?? "") }; });

  // Calm: each tab's own model, one at a time.
  const calm: Record<string, number> = {};
  for (let i = 0; i < n; i++) {
    await tabs.nth(i).click();
    const key = await p.evaluate(() => (window as unknown as { __play: Play }).__play.selected());
    await expect.poll(async () => (await play()).shown?.key ?? "", { timeout: 60_000 }).toBe(key);
    await expect.poll(async () => (await play()).state, { timeout: 60_000 }).toContain("in your browser");
    calm[key] = (await play()).shown!.triangles;
  }

  // Rough: switch tabs and drag sliders faster than the replies come back.
  const slide = async () => {
    const s = p.locator("#play .sliders input").first();
    if (!(await s.count())) return;
    await s.evaluate((el: HTMLInputElement) => {
      const lo = +el.min, hi = +el.max;
      for (let k = 0; k < 5; k++) { el.value = String(lo + (hi - lo) * Math.random()); el.dispatchEvent(new Event("input", { bubbles: true })); }
      el.dispatchEvent(new Event("change", { bubbles: true }));
    });
  };
  const order = [1, 0, 2, 1, 3 % n, 0, 2, 1];
  for (const i of order) { await tabs.nth(i).click(); await slide(); await p.waitForTimeout(40 + Math.random() * 120); }
  await tabs.nth(0).click();
  await tabs.nth(2).click();
  const last = await p.evaluate(() => (window as unknown as { __play: Play }).__play.selected());

  // Settle, then hold still for longer than any reply can be late: nothing may change after.
  await expect.poll(async () => (await play()).shown?.key ?? "", { timeout: 60_000 }).toBe(last);
  await expect.poll(async () => (await play()).state, { timeout: 60_000 }).toContain("in your browser");
  await p.waitForTimeout(2500);
  const end = await play();
  expect(end.sel).toBe(last);
  expect(end.shown?.key).toBe(last);
  expect(end.shown?.triangles).toBe(calm[last]);
  expect(end.live).toBe(true);
  expect(end.script).toBe(end.src);
  expect(errs).toEqual([]);
  await ctx.close();
});
