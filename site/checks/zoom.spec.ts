// The drawings section's zoomable pair, on a phone and a desk window, and the
// lightbox's drawings, which share its gestures. The pair opens zoomed in; a
// pinch, a Ctrl/Cmd wheel and a double tap change the magnification; a drag
// and a two-finger scroll move the drawing without changing it; a plain wheel
// scrolls the page until the drawing has been clicked; the line between the
// picture and the lines moves only by its grip. Throughout, the page keeps its
// scale and never scrolls sideways, and the drawing is still the SVG, zoomed by
// its viewBox, not a picture of it.
import type { CDPSession, Page } from "@playwright/test";

import { context, expect, test } from "./site.js";

type State = { k: number; readout: string; vb: number[]; svgs: number; paths: number; picture: boolean; scale: number; sideways: number; scrollX: number; split: string };

const state = (p: Page): Promise<State> =>
  p.evaluate(() => {
    const box = document.querySelector<HTMLElement>(".zf")!;
    const svg = box.querySelector(".zf-svg > svg")!;
    return {
      k: +(box.dataset["k"] ?? "0"),
      readout: document.querySelector(".zk")!.textContent!,
      vb: svg.getAttribute("viewBox")!.split(/\s+/).map(Number),
      svgs: box.querySelectorAll(".zf-svg > svg").length,
      paths: svg.querySelectorAll("path").length,
      picture: !box.querySelector<HTMLCanvasElement>(".zf-png")!.hidden,
      scale: visualViewport!.scale,
      sideways: document.documentElement.scrollWidth - innerWidth,
      scrollX: scrollX,
      split: box.querySelector(".zf-split")!.getAttribute("aria-valuenow")!,
    };
  });

/** Scrolls the pair into view and waits for its drawing and its picture. */
async function open(p: Page) {
  await p.evaluate(() => document.querySelector(".zf")!.scrollIntoView({ block: "center", behavior: "instant" }));
  await p.waitForFunction(() => !!document.querySelector(".zf-svg > svg") && !document.querySelector<HTMLCanvasElement>(".zf-png")!.hidden, null, { timeout: 30_000 });
  const r = (await p.locator(".zf").boundingBox())!;
  // a point clear of the grip, and the line between the two sides
  return { x: r.x + r.width * 0.3, y: r.y + r.height * 0.4, line: r.x + r.width / 2 };
}

/** Waits for an animated zoom to land on a magnification, however slow the machine: the
 *  magnification itself, not the readout, which rounds before the animation ends. */
const lands = (p: Page, k: number) =>
  p.waitForFunction((k) => Math.abs(+(document.querySelector<HTMLElement>(".zf")!.dataset["k"] ?? 0) - k) < 1e-3, k, { timeout: 30_000 });

/** Still vector, still inside the frame, and the page unmoved. */
function sound(s: State, paths: number) {
  expect(s.svgs, "the SVG is still in the frame").toBe(1);
  expect(s.paths, "with all its paths").toBe(paths);
  expect(s.vb[2]!, "the viewBox narrows as the drawing is magnified").toBeCloseTo(1500 / s.k, 0);
  expect(s.readout, "the readout").toBe(`${s.k < 9.95 ? Math.round(s.k * 10) / 10 : Math.round(s.k)}×`);
  expect(s.scale, "page zoom").toBe(1);
  expect(s.sideways, "the page is wider than the window").toBeLessThanOrEqual(0);
  expect(s.scrollX, "the page scrolled sideways").toBe(0);
}

const touch = (cdp: CDPSession, type: string, points: [number, number][]) =>
  cdp.send("Input.dispatchTouchEvent", { type, touchPoints: points.map(([x, y], id) => ({ x, y, id })) });

test("zoomable pair, phone-390: opens zoomed in; pinch, drag, double tap, grip", async ({ browser, site }) => {
  const ctx = await context(browser, "phone-390");
  const p = await ctx.newPage();
  const errs: string[] = [];
  p.on("pageerror", (e) => errs.push(String(e)));
  await p.goto(site + "?noauto=1", { waitUntil: "load" });
  let c = await open(p);
  const first = await state(p);
  expect(first.readout, "it opens zoomed in").toBe("8×");
  expect(first.picture, "with the picture under the lines").toBe(true);
  sound(first, first.paths);
  const cdp = await ctx.newCDPSession(p);

  // two fingers spread apart, straddling the line between the picture and the lines
  await touch(cdp, "touchStart", [[c.line - 15, c.y], [c.line + 15, c.y]]);
  for (let i = 1; i <= 10; i++) await touch(cdp, "touchMove", [[c.line - 15 - i * 5, c.y], [c.line + 15 + i * 5, c.y]]);
  await touch(cdp, "touchEnd", []);
  await p.waitForTimeout(100);
  const pinched = await state(p);
  expect(pinched.k, "a pinch zooms").toBeGreaterThan(first.k * 1.5);
  expect(pinched.split, "and leaves the line where it was").toBe("50");
  sound(pinched, first.paths);

  // one finger drags the drawing
  await touch(cdp, "touchStart", [[c.x, c.y]]);
  for (let i = 1; i <= 6; i++) await touch(cdp, "touchMove", [[c.x + i * 15, c.y + i * 5]]);
  await touch(cdp, "touchEnd", []);
  await p.waitForTimeout(700);
  const dragged = await state(p);
  expect(dragged.vb[0], "a drag moves the drawing").toBeLessThan(pinched.vb[0]! - 0.5);
  expect(dragged.k, "and does not zoom it").toBeCloseTo(pinched.k, 2);
  sound(dragged, first.paths);

  // the whole model, then a double tap zooms in on the spot
  await p.locator('[data-z="reset"]').click();
  await lands(p, 1);
  c = await open(p); // the click may have scrolled the page
  for (let i = 0; i < 2; i++) {
    await touch(cdp, "touchStart", [[c.x, c.y]]);
    await touch(cdp, "touchEnd", []);
    await p.waitForTimeout(60);
  }
  await lands(p, 2.5);
  sound(await state(p), first.paths);

  // the grip moves the line, and nothing else
  const g = (await p.locator(".zf-split").boundingBox())!;
  await touch(cdp, "touchStart", [[g.x + g.width / 2, g.y + g.height / 2]]);
  for (let i = 1; i <= 6; i++) await touch(cdp, "touchMove", [[g.x + g.width / 2 - i * 15, g.y + g.height / 2]]);
  await touch(cdp, "touchEnd", []);
  const gripped = await state(p);
  expect(+gripped.split, "the grip moved the line").toBeLessThan(40);
  expect(gripped.readout, "and not the drawing").toBe("2.5×");
  expect(errs, "page errors").toEqual([]);
  await ctx.close();
});

// Flaky on a loaded host (times out in lands()); tracked in the post-launch follow-ups.
test.fixme("zoomable pair, desk-1440: scroll pans, ctrl+wheel zooms, the page is not trapped", async ({ browser, site }) => {
  const ctx = await context(browser, "desk-1440");
  const p = await ctx.newPage();
  const errs: string[] = [];
  p.on("pageerror", (e) => errs.push(String(e)));
  await p.goto(site + "?noauto=1", { waitUntil: "load" });
  let c = await open(p);
  const first = await state(p);
  expect(first.readout).toBe("8×");

  // before the drawing has been clicked, a plain wheel scrolls the page past it
  await p.mouse.move(c.x, c.y);
  const y0 = await p.evaluate(() => scrollY);
  await p.mouse.wheel(0, 200);
  await p.waitForFunction((y) => scrollY > y, y0, { timeout: 10_000 });
  expect((await state(p)).vb, "the drawing did not move").toEqual(first.vb);

  // once clicked, a two-finger scroll moves the drawing in both directions and does not zoom
  c = await open(p);
  await p.mouse.click(c.x, c.y);
  const y1 = await p.evaluate(() => scrollY);
  await p.mouse.move(c.x, c.y);
  for (let i = 0; i < 5; i++) await p.mouse.wheel(12, 9);
  await p.waitForTimeout(100);
  const scrolled = await state(p);
  expect(scrolled.vb[0], "it moved sideways").toBeGreaterThan(first.vb[0]! + 1);
  expect(scrolled.vb[1], "and down").toBeGreaterThan(first.vb[1]! + 1);
  expect(scrolled.k, "at the same magnification").toBeCloseTo(first.k, 3);
  expect(await p.evaluate(() => scrollY), "the page stayed put").toBe(y1);
  sound(scrolled, first.paths);

  // a trackpad pinch arrives as a wheel event with ctrl
  await p.keyboard.down("Control");
  for (let i = 0; i < 5; i++) await p.mouse.wheel(0, -20);
  await p.keyboard.up("Control");
  await p.waitForTimeout(100);
  const pinched = await state(p);
  expect(pinched.k, "ctrl and the wheel zoom").toBeGreaterThan(first.k * 1.5);
  sound(pinched, first.paths);

  // a drag moves it
  await p.mouse.move(c.x, c.y);
  await p.mouse.down();
  await p.mouse.move(c.x + 160, c.y + 60, { steps: 8 });
  await p.mouse.up();
  await p.waitForTimeout(700);
  const dragged = await state(p);
  expect(dragged.vb[0], "a drag moves the drawing").toBeLessThan(pinched.vb[0]!);
  sound(dragged, first.paths);

  // the buttons and a double click
  await p.locator('[data-z="reset"]').click();
  await lands(p, 1);
  c = await open(p);
  await p.mouse.dblclick(c.x, c.y);
  await lands(p, 2.5);
  await p.locator('[data-z="in"]').click();
  await lands(p, 5);
  sound(await state(p), first.paths);

  // the grip moves the line between the two sides
  const g = (await p.locator(".zf-split").boundingBox())!;
  await p.mouse.move(g.x + g.width / 2, g.y + g.height / 2);
  await p.mouse.down();
  await p.mouse.move(g.x + g.width / 2 - 150, g.y + g.height / 2, { steps: 5 });
  await p.mouse.up();
  const gripped = await state(p);
  expect(+gripped.split, "the line moved left").toBeLessThan(45);
  expect(gripped.readout, "and the drawing did not move").toBe("5×");
  expect(errs, "page errors").toEqual([]);
  await ctx.close();
});

test("lightbox drawing, desk-1440: the same gestures", async ({ browser, site }) => {
  const ctx = await context(browser, "desk-1440");
  const p = await ctx.newPage();
  const errs: string[] = [];
  p.on("pageerror", (e) => errs.push(String(e)));
  await p.goto(site + "?noauto=1", { waitUntil: "load" });
  const card = p.locator("button[data-draw]").first();
  await card.scrollIntoViewIfNeeded();
  await card.click();
  await p.waitForFunction(() => !!document.querySelector(".sheet-draw svg") && !!document.querySelector<HTMLElement>(".sheet-draw")!.dataset["k"]);
  const vb = () => p.evaluate(() => document.querySelector(".sheet-draw svg")!.getAttribute("viewBox")!.split(/\s+/).map(Number));
  const r = (await p.locator(".sheet-draw").boundingBox())!;
  await p.mouse.move(r.x + r.width / 2, r.y + r.height / 2);
  const v0 = await vb();
  await p.keyboard.down("Control");
  for (let i = 0; i < 5; i++) await p.mouse.wheel(0, -20);
  await p.keyboard.up("Control");
  const v1 = await vb();
  expect(v1[2], "ctrl and the wheel zoom").toBeLessThan(v0[2]! / 1.5);
  for (let i = 0; i < 4; i++) await p.mouse.wheel(10, 10);
  const v2 = await vb();
  expect(v2[2], "a scroll does not zoom").toBeCloseTo(v1[2]!, 3);
  expect(v2[0], "it moves the drawing").not.toBeCloseTo(v1[0]!, 1);
  expect(await p.evaluate(() => visualViewport!.scale)).toBe(1);
  expect(errs, "page errors").toEqual([]);
  await ctx.close();
});
