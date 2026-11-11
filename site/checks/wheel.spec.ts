// The wheel on a desktop window. Over the lightbox's live model a notched
// mouse wheel zooms (the camera comes closer), and a two-finger scroll -- or a
// finger dragged on a Mac's touch screen in Chrome, which arrives as the same
// stream of pixel wheel events -- moves the model and never zooms it; the
// page stays put either way. Over the page outside any model the wheel
// scrolls the page; over the playground's model, which is part of the
// scrolling page, it scrolls the page until a click on the model hands the
// wheel over.
import { context, expect, test } from "./site.js";

type Cam = { x: number; y: number; z: number; d: number } | null;
const cam = (p: import("@playwright/test").Page) => p.evaluate(() => (window as unknown as { __play: { cam(): Cam } }).__play.cam());

test("a notched wheel zooms the opened model and scrolls the page elsewhere", async ({ browser, site }) => {
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

test("a two-finger scroll, or a finger on a Mac's touch screen in Chrome, moves the opened model and never zooms it off the screen", async ({ browser, site }) => {
  const ctx = await context(browser, "desk-1440");
  const p = await ctx.newPage();
  const errs: string[] = [];
  p.on("pageerror", (e) => errs.push(String(e).slice(0, 200)));
  await p.goto(site + "?noauto=1", { waitUntil: "load" });
  await p.evaluate(() => { const b = document.querySelector<HTMLElement>("button[data-pic][data-sheet3d]")!; b.scrollIntoView({ block: "center", behavior: "instant" }); b.click(); });
  await p.waitForSelector(".sheet-view.is-live .live canvas", { timeout: 60_000 });
  await p.waitForTimeout(400);
  const box = (await p.locator(".sheet-view .live canvas").boundingBox())!;
  const y0 = await p.evaluate(() => scrollY);
  // The opening frame eases in: start from where it comes to rest.
  let c0 = (await cam(p))!;
  for (let i = 0; i < 40; i++) {
    await p.waitForTimeout(150);
    const c = (await cam(p))!;
    if (Math.abs(c.d - c0.d) < 1e-6 * c0.d && c.x === c0.x) break;
    c0 = c;
  }
  // A finger dragged up and across, as Chrome delivers it: a pixel stream that
  // starts small, with steps that land on whole multiples of 40.
  await p.mouse.move(box.x + box.width * 0.7, box.y + box.height * 0.3);
  for (const [dx, dy] of [[0, -1.5], [-2, -6.3], [-8.4, -17], [-40, 0], [-44.9, -55], [0, -80], [0, -120], [-63.1, -40], [-21.7, -8.4], [-2.2, -1]]) await p.mouse.wheel(dx, dy);
  await p.waitForTimeout(600);
  const c1 = (await cam(p))!;
  expect(c1.d / c0.d).toBeCloseTo(1, 2);
  expect(Math.hypot(c1.x - c0.x, c1.y - c0.y, c1.z - c0.z)).toBeGreaterThan(0.5);
  // However long the scroll goes on, the model stays in reach.
  for (let i = 0; i < 150; i++) await p.mouse.wheel(-60.5, 23.3);
  await p.waitForTimeout(800);
  const c2 = (await cam(p))!;
  expect(c2.d / c0.d).toBeCloseTo(1, 2);
  expect(Math.hypot(c2.x - c0.x, c2.y - c0.y, c2.z - c0.z)).toBeLessThan(c0.d * 1.5);
  expect(await p.evaluate(() => scrollY)).toBe(y0);
  // Safari's pinch (gesture events, no wheel): the model zooms and the page does not.
  const zoomed = await p.evaluate(() => {
    const c = document.querySelector(".sheet-view .live canvas")!;
    const r = c.getBoundingClientRect();
    let prevented = true;
    for (const [type, scale] of [["gesturestart", 1], ["gesturechange", 1.25], ["gesturechange", 1.5], ["gestureend", 1.5]] as const) {
      const e = new Event(type, { bubbles: true, cancelable: true });
      Object.assign(e, { scale, rotation: 0, clientX: r.left + r.width / 2, clientY: r.top + r.height / 2 });
      c.dispatchEvent(e);
      prevented &&= e.defaultPrevented;
    }
    return prevented;
  });
  expect(zoomed).toBe(true);
  await expect.poll(async () => (await cam(p))!.d, { timeout: 5_000 }).toBeLessThan(c2.d * 0.8);
  expect(await p.evaluate(() => window.visualViewport?.scale ?? 1)).toBe(1);
  expect(errs).toEqual([]);
  await ctx.close();
});
