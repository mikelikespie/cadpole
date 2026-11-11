// The viewer's gestures, input by input: what a trackpad, a notched mouse
// wheel, Safari's gesture events, a touch screen on a Mac (in Safari and in
// Chrome) and a touch screen elsewhere each do to the camera. Mock engine.
//
// A touch screen on a Mac never reaches a page as touch: Safari sends a
// finger as a mouse and two fingers as gesture events plus a one-button drag
// at their centroid; Chrome sends a finger drag as a pixel wheel stream.
// Those event sequences are replayed here as the browsers send them.

import { expect, test, type Page } from "@playwright/test";

const URL = "/?mock=1&nosw=1";

interface Pose {
  theta: number;
  phi: number;
  distance: number;
  target: { x: number; y: number; z: number };
}

async function open(page: Page): Promise<void> {
  await page.goto(URL);
  await expect(page.locator("html")).toHaveClass(/ready/);
  await expect(page.locator("html")).toHaveAttribute("data-evals", /[1-9]/);
  await page.waitForTimeout(300);
}

const pose = (page: Page): Promise<Pose> => page.evaluate(() => (window as unknown as { __cadpole: { viewport(): { pose: Pose } } }).__cadpole.viewport().pose);

async function centre(page: Page): Promise<{ x: number; y: number }> {
  const box = await page.locator("canvas.viewport-canvas").boundingBox();
  if (!box) throw new Error("no canvas");
  return { x: box.x + box.width / 2, y: box.y + box.height / 2 };
}

/** The drawn scene's bounding sphere, from the box the viewer stamps on <html>. */
async function scene(page: Page): Promise<{ c: { x: number; y: number; z: number }; r: number }> {
  const b = ((await page.locator("html").getAttribute("data-scene-box")) ?? "").split(",").map(Number) as number[];
  const [x0, y0, z0, x1, y1, z1] = b as [number, number, number, number, number, number];
  return { c: { x: (x0 + x1) / 2, y: (y0 + y1) / 2, z: (z0 + z1) / 2 }, r: Math.hypot(x1 - x0, y1 - y0, z1 - z0) / 2 };
}

const moved = (a: Pose, b: Pose): number => Math.hypot(a.target.x - b.target.x, a.target.y - b.target.y, a.target.z - b.target.z);

/** Safari's gesture event: an Event with the scale, rotation and point WebKit gives it. */
async function gesture(page: Page, type: string, scale: number, rotation: number, at: { x: number; y: number }): Promise<boolean> {
  return page.evaluate(
    ({ type, scale, rotation, at }) => {
      const c = document.querySelector("canvas.viewport-canvas") as HTMLElement;
      const e = new Event(type, { bubbles: true, cancelable: true });
      Object.assign(e, { scale, rotation, clientX: at.x, clientY: at.y });
      c.dispatchEvent(e);
      return e.defaultPrevented;
    },
    { type, scale, rotation, at },
  );
}

test.describe("one gesture model", () => {
  test("Chrome on a touch Mac: a finger drag arrives as a pixel wheel stream, which pans smoothly and never zooms or loses the model", async ({ page }) => {
    await open(page);
    const c = await centre(page);
    await page.mouse.move(c.x, c.y);
    const before = await pose(page);
    // A finger flicked sideways, as Chrome 157 delivered it on a touch Mac:
    // small first deltas, then large ones, some landing on whole multiples of 40.
    const deltas = [-1.5, -4.2, -9.8, -17.3, -40, -44.9, -80, -120, -63.1, -40, -21.7, -8.4, -2.2];
    for (const dx of deltas) await page.mouse.wheel(dx, 0);
    const after = await pose(page);
    expect(after.distance / before.distance).toBeCloseTo(1, 2);
    expect(after.theta).toBeCloseTo(before.theta, 6);
    expect(after.phi).toBeCloseTo(before.phi, 6);
    expect(moved(before, after)).toBeGreaterThan(1);
  });

  test("a long scroll stream can pan only so far: the model stays within reach", async ({ page }) => {
    await open(page);
    const c = await centre(page);
    await page.mouse.move(c.x, c.y);
    for (let i = 0; i < 120; i++) await page.mouse.wheel(-60.5, 23.3);
    const p = await pose(page);
    const s = await scene(page);
    const off = Math.hypot(p.target.x - s.c.x, p.target.y - s.c.y, p.target.z - s.c.z);
    expect(off).toBeLessThanOrEqual(s.r * 1.5 + 1e-6);
  });

  test("a notched mouse wheel zooms toward the cursor; a trackpad delta of 40 px inside a stream does not", async ({ page }) => {
    await open(page);
    const c = await centre(page);
    await page.mouse.move(c.x, c.y);
    const d0 = (await pose(page)).distance;
    await page.mouse.wheel(0, -120);
    await page.waitForTimeout(200);
    await page.mouse.wheel(0, -120);
    const d1 = (await pose(page)).distance;
    expect(d1).toBeLessThan(d0 * 0.8);
    await page.waitForTimeout(200);
    // A trackpad stream: it starts small, so a later 40 px step is still a pan.
    await page.mouse.wheel(0, 2.5);
    await page.mouse.wheel(0, 40);
    await page.mouse.wheel(0, 40);
    expect((await pose(page)).distance / d1).toBeCloseTo(1, 3);
  });

  test("ctrl + wheel (a pinch in Chrome) zooms; a two-finger touch's jitter of them does not", async ({ page }) => {
    await open(page);
    const c = await centre(page);
    await page.mouse.move(c.x, c.y);
    const d0 = (await pose(page)).distance;
    await page.keyboard.down("Control");
    for (let i = 0; i < 10; i++) await page.mouse.wheel(0, -5);
    const d1 = (await pose(page)).distance;
    expect(d1).toBeLessThan(d0 * 0.75);
    await page.waitForTimeout(400);
    for (let i = 0; i < 16; i++) await page.mouse.wheel(0, i % 2 ? 1 : -1);
    await page.keyboard.up("Control");
    const d2 = (await pose(page)).distance;
    expect(Math.abs(d2 / d1 - 1)).toBeLessThan(0.03);
  });

  test("Safari: a trackpad pinch (gesture events, no button down) zooms, and the page never does", async ({ page }) => {
    await open(page);
    const c = await centre(page);
    const d0 = (await pose(page)).distance;
    expect(await gesture(page, "gesturestart", 1, 0, c)).toBe(true);
    for (const s of [1.1, 1.25, 1.5]) expect(await gesture(page, "gesturechange", s, 0, c)).toBe(true);
    expect(await gesture(page, "gestureend", 1.5, 0, c)).toBe(true);
    expect((await pose(page)).distance / (d0 / 1.5)).toBeCloseTo(1, 2);
    expect(await page.evaluate(() => window.visualViewport?.scale ?? 1)).toBe(1);
  });

  test("Safari on a touch Mac: two fingers dragged together pan (the centroid drag does not orbit)", async ({ page }) => {
    await open(page);
    const c = await centre(page);
    const before = await pose(page);
    // The first finger's press, cancelled by WebKit as the second lands...
    await page.mouse.move(c.x, c.y);
    await page.mouse.down();
    await page.evaluate(() => {
      const el = document.querySelector("canvas.viewport-canvas") as HTMLElement;
      el.dispatchEvent(new PointerEvent("pointercancel", { pointerId: 1, pointerType: "mouse", bubbles: true }));
    });
    await page.mouse.up();
    // ...then the gesture, and a fresh one-button drag at the centroid.
    await gesture(page, "gesturestart", 1, 0, c);
    await page.mouse.down();
    for (let i = 1; i <= 10; i++) {
      await page.mouse.move(c.x + i * 12, c.y + i * 4);
      await gesture(page, "gesturechange", 1, 0, { x: c.x + i * 12, y: c.y + i * 4 });
    }
    await page.mouse.up();
    await gesture(page, "gestureend", 1, 0, { x: c.x + 120, y: c.y + 40 });
    const after = await pose(page);
    expect(after.theta).toBeCloseTo(before.theta, 3);
    expect(after.phi).toBeCloseTo(before.phi, 3);
    expect(moved(before, after)).toBeGreaterThan(1);
  });

  test("Safari: a twist turns the model about the vertical once past its dead zone; a pinch that wobbles does not", async ({ page }) => {
    await open(page);
    const c = await centre(page);
    const t0 = (await pose(page)).theta;
    await gesture(page, "gesturestart", 1, 0, c);
    for (const r of [4, 8, 12, 20, 30]) await gesture(page, "gesturechange", 1, r, c);
    await gesture(page, "gestureend", 1, 30, c);
    const t1 = (await pose(page)).theta;
    // Engaged at 12 degrees, so the 18 that followed turn the model.
    expect(((t1 - t0) * 180) / Math.PI).toBeCloseTo(18, 0);
    const p1 = await pose(page);
    await gesture(page, "gesturestart", 1, 0, c);
    for (const [s, r] of [[1.2, 2], [1.5, 6], [1.8, 9]] as const) await gesture(page, "gesturechange", s, r, c);
    await gesture(page, "gestureend", 1.8, 9, c);
    expect((await pose(page)).theta).toBeCloseTo(p1.theta, 6);
  });

  test("a right click opens the model's own menu; a right drag pans without one", async ({ page }) => {
    await open(page);
    const c = await centre(page);
    const menu = page.getByRole("menu", { name: "Select entity" });
    await page.mouse.click(c.x, c.y, { button: "right" });
    await expect(menu).toBeVisible();
    await page.keyboard.press("Escape");
    await expect(menu).toBeHidden();
    const before = await pose(page);
    await page.mouse.move(c.x, c.y);
    await page.mouse.down({ button: "right" });
    await page.mouse.move(c.x + 150, c.y + 60, { steps: 8 });
    await page.mouse.up({ button: "right" });
    await page.waitForTimeout(500);
    await expect(menu).toBeHidden();
    const after = await pose(page);
    expect(after.theta).toBeCloseTo(before.theta, 6);
    expect(moved(before, after)).toBeGreaterThan(1);
  });

  test("a press held into a menu and then dragged at once orbits, and the menu goes away (a touch screen on a Mac)", async ({ page }) => {
    await open(page);
    const c = await centre(page);
    const menu = page.getByRole("menu", { name: "Select entity" });
    // AppKit turns a finger held still into a right press...
    await page.mouse.move(c.x, c.y);
    await page.mouse.down({ button: "right" });
    await page.waitForTimeout(500);
    await page.mouse.up({ button: "right" });
    await expect(menu).toBeVisible();
    // ...and the same finger moving on into a left drag.
    const t0 = (await pose(page)).theta;
    await page.mouse.down();
    await page.mouse.move(c.x + 160, c.y, { steps: 8 });
    await page.mouse.up();
    await expect(menu).toBeHidden();
    expect(Math.abs((await pose(page)).theta - t0)).toBeGreaterThan(0.3);
  });

  test("a double-click or a held drag on the model never selects the page's text", async ({ page }) => {
    await open(page);
    const c = await centre(page);
    await page.mouse.dblclick(c.x, c.y);
    await page.mouse.move(c.x, c.y);
    await page.mouse.down();
    await page.waitForTimeout(700);
    await page.mouse.move(c.x + 200, c.y + 80, { steps: 8 });
    await page.mouse.up();
    const s = await page.evaluate(() => ({
      selected: String(getSelection() ?? ""),
      user: getComputedStyle(document.querySelector("canvas.viewport-canvas") as Element).userSelect,
    }));
    expect(s.selected).toBe("");
    expect(s.user).toBe("none");
  });
});

test.describe("one gesture model, by touch", () => {
  test.use({ hasTouch: true });

  test("two touch points twisting turn the model; pinching zooms about their middle", async ({ page }) => {
    await open(page);
    const c = await centre(page);
    const cdp = await page.context().newCDPSession(page);
    const pts = (deg: number, sep: number) => {
      const a = (deg * Math.PI) / 180;
      const dx = (Math.cos(a) * sep) / 2;
      const dy = (Math.sin(a) * sep) / 2;
      return [
        { x: Math.round(c.x - dx), y: Math.round(c.y - dy), id: 1 },
        { x: Math.round(c.x + dx), y: Math.round(c.y + dy), id: 2 },
      ];
    };
    const t0 = (await pose(page)).theta;
    const d0 = (await pose(page)).distance;
    await cdp.send("Input.dispatchTouchEvent", { type: "touchStart", touchPoints: pts(0, 200) });
    for (let deg = 3; deg <= 30; deg += 3) await cdp.send("Input.dispatchTouchEvent", { type: "touchMove", touchPoints: pts(deg, 200) });
    await cdp.send("Input.dispatchTouchEvent", { type: "touchEnd", touchPoints: [] });
    const p1 = await pose(page);
    const turned = ((p1.theta - t0) * 180) / Math.PI;
    // The dead zone takes the first ~10 degrees; the rest turn the model.
    expect(turned).toBeGreaterThan(15);
    expect(turned).toBeLessThan(23);
    expect(p1.distance / d0).toBeCloseTo(1, 2);
    await cdp.send("Input.dispatchTouchEvent", { type: "touchStart", touchPoints: pts(0, 120) });
    for (let s = 140; s <= 240; s += 20) await cdp.send("Input.dispatchTouchEvent", { type: "touchMove", touchPoints: pts(0, s) });
    await cdp.send("Input.dispatchTouchEvent", { type: "touchEnd", touchPoints: [] });
    const p2 = await pose(page);
    expect(p2.distance / p1.distance).toBeCloseTo(0.5, 1);
    expect(p2.theta).toBeCloseTo(p1.theta, 6);
    await cdp.detach();
  });
});
