// The wheel on a desktop window. Over the lightbox's live model it zooms (the
// camera comes closer) and the page stays put; over the page outside any
// model it scrolls the page; over the playground's model, which is part of the
// scrolling page, it scrolls the page until a click on the model hands the
// wheel over.
import { context, expect, test } from "./site.js";

type Cam = { d: number } | null;
const cam = (p: import("@playwright/test").Page) => p.evaluate(() => (window as unknown as { __play: { cam(): Cam } }).__play.cam());

test("wheel zooms the opened model and scrolls the page elsewhere", async ({ browser, site }) => {
  const ctx = await context(browser, "desk-1440");
  const p = await ctx.newPage();
  const errs: string[] = [];
  p.on("pageerror", (e) => errs.push(String(e).slice(0, 200)));
  await p.goto(site + "?noauto=1", { waitUntil: "load" });

  // The lightbox: a card's model, then the wheel over its canvas.
  await p.evaluate(() => { const b = document.querySelector<HTMLElement>("button[data-pic][data-sheet3d]")!; b.scrollIntoView({ block: "center", behavior: "instant" }); b.click(); });
  await p.waitForSelector(".sheet-view.is-live .live canvas", { timeout: 60_000 });
  await p.waitForTimeout(400);
  const box = (await p.locator(".sheet-view .live canvas").boundingBox())!;
  const y0 = await p.evaluate(() => scrollY);
  const d0 = (await cam(p))!.d;
  await p.mouse.move(box.x + box.width * 0.6, box.y + box.height * 0.45);
  for (let i = 0; i < 4; i++) { await p.mouse.wheel(0, -120); await p.waitForTimeout(60); }
  await expect.poll(async () => (await cam(p))!.d, { timeout: 5_000 }).toBeLessThan(d0 * 0.8);
  // Out again, smoothly: the springs settle where the wheel left the goal.
  for (let i = 0; i < 4; i++) { await p.mouse.wheel(0, 120); await p.waitForTimeout(60); }
  await expect.poll(async () => (await cam(p))!.d, { timeout: 5_000 }).toBeGreaterThan(d0 * 0.9);
  expect(await p.evaluate(() => scrollY)).toBe(y0);
  await p.evaluate(() => document.querySelector<HTMLElement>("[data-sheet-close]")!.click());
  await p.waitForTimeout(200);

  // The page, outside any model: the wheel scrolls it.
  await p.evaluate(() => scrollTo(0, 0));
  await p.mouse.move(40, 500);
  await p.mouse.wheel(0, 600);
  await expect.poll(() => p.evaluate(() => scrollY), { timeout: 5_000 }).toBeGreaterThan(300);

  // The playground's model, not taken: the wheel scrolls the page, and the model does not move.
  await p.evaluate(() => { document.querySelector("#play")!.scrollIntoView({ block: "center", behavior: "instant" }); (window as unknown as { __play: { start(): void } }).__play.start(); });
  await p.waitForSelector(".play-view.is-live .live canvas", { timeout: 120_000 });
  await p.waitForTimeout(400);
  const pv = (await p.locator(".play-view .live canvas").boundingBox())!;
  const pd0 = (await cam(p))!.d;
  const py0 = await p.evaluate(() => scrollY);
  await p.mouse.move(pv.x + pv.width / 2, pv.y + pv.height / 2);
  await p.mouse.wheel(0, 200);
  await expect.poll(() => p.evaluate(() => scrollY), { timeout: 5_000 }).toBeGreaterThan(py0 + 100);
  expect((await cam(p))!.d).toBeCloseTo(pd0, 3);
  await expect(p.locator(".play-view .cp3d-hint.on")).toHaveCount(1);

  // A click on the model hands it the wheel.
  await p.evaluate(() => document.querySelector("#play .play-view")!.scrollIntoView({ block: "center", behavior: "instant" }));
  await p.waitForTimeout(200);
  const pv2 = (await p.locator(".play-view .live canvas").boundingBox())!;
  await p.mouse.move(pv2.x + pv2.width / 2, pv2.y + pv2.height / 2);
  await p.mouse.down();
  await p.mouse.up();
  const py1 = await p.evaluate(() => scrollY);
  const pd1 = (await cam(p))!.d;
  for (let i = 0; i < 4; i++) { await p.mouse.wheel(0, -120); await p.waitForTimeout(60); }
  await expect.poll(async () => (await cam(p))!.d, { timeout: 5_000 }).toBeLessThan(pd1 * 0.8);
  expect(await p.evaluate(() => scrollY)).toBe(py1);
  expect(errs).toEqual([]);
  await ctx.close();
});
