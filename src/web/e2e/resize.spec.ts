// The viewport through a change of size.
//
// A canvas whose drawing buffer is resized is cleared, and what is shown is
// whatever the buffer holds when the browser next composites. So after every
// change -- the window dragged, turned on its side, a new pixel ratio, the
// lighter picture drawn while the view moves -- two things must hold:
//
//   at every paint   the picture in the buffer was drawn at the buffer's size
//                    (never a cleared buffer waiting for the next frame);
//   once settled     the model is on screen, read back from a screenshot,
//                    with its centre in the middle of the uncovered area.
//
// A translucent outline is kept on screen throughout (the pointer rests on
// the model): a translucent fat line samples a copy of the frame, a texture
// the size of the buffer, and is the one material that can be left bound to
// the copy a resize destroyed.
//
// Two projects run this file: `resize` on the WebGL2 backend, and
// `resize-webgpu` on WebGPU, where a destroyed texture fails the whole frame.

import { expect, test, type CDPSession, type Page } from "@playwright/test";
import { decodePng } from "./png.js";

const URL = "/?mock=1&nosw=1";

interface View {
  backend: "webgpu" | "webgl";
  /** The canvas's box on the page, CSS px. */
  canvas: { x: number; y: number; width: number; height: number };
  buffer: { width: number; height: number };
  drawn: { width: number; height: number };
  insets: { top: number; right: number; bottom: number; left: number };
  /** Where the framed box's centre is drawn, page px. */
  centre: { x: number; y: number } | null;
  /** The framed box's projection clipped to the canvas, page px. */
  box: { x: number; y: number; width: number; height: number } | null;
}

async function open(page: Page): Promise<void> {
  await page.goto(URL);
  await expect(page.locator("html")).toHaveClass(/ready/);
  await expect(page.locator("html")).toHaveAttribute("data-evals", /[1-9]/);
  await settled(page);
  // The renderer falls back to WebGL2 without a word when there is no WebGPU
  // adapter, and a WebGPU project that ran on WebGL2 would prove nothing.
  if (test.info().project.name.includes("webgpu")) {
    const adapter = await page.evaluate(async () => {
      const gpu = (navigator as unknown as { gpu?: { requestAdapter(): Promise<{ info: { vendor: string; architecture: string } } | null> } }).gpu;
      const a = await gpu?.requestAdapter();
      return a ? `${a.info.vendor} ${a.info.architecture}` : "none";
    });
    expect(adapter, "the WebGPU adapter").toContain("swiftshader");
    expect((await view(page)).backend).toBe("webgpu");
  }
}

/**
 * The viewer has gone idle: its frame counter has stopped, across a wait and
 * two animation frames of the page's own. The frames matter on a busy
 * machine, where a page can sit a long while without having laid out or
 * drawn anything at all, which is not the same as having nothing to draw.
 */
async function settled(page: Page, quietMs = 400): Promise<void> {
  const frames = (): Promise<number> =>
    page.evaluate(
      () =>
        new Promise<number>((done) =>
          requestAnimationFrame(() => requestAnimationFrame(() => done(Number(document.documentElement.dataset["frames"] ?? 0)))),
        ),
    );
  let last = await frames();
  const deadline = Date.now() + 30_000;
  while (Date.now() < deadline) {
    await page.waitForTimeout(quietMs);
    const now = await frames();
    if (now === last) return;
    last = now;
  }
}

function view(page: Page): Promise<View> {
  return page.evaluate(() => {
    type P = { x: number; y: number } | null;
    const v = (
      window as unknown as {
        __cadpole: { viewport(): Omit<View, "canvas" | "centre" | "box"> & { screenPoint(p: { x: number; y: number; z: number }): P } };
      }
    ).__cadpole.viewport();
    const canvas = document.querySelector("canvas.viewport-canvas") as HTMLCanvasElement;
    const r = canvas.getBoundingClientRect();
    const [x0, y0, z0, x1, y1, z1] = (document.documentElement.dataset["frameBox"] ?? "").split(",").map(Number) as [number, number, number, number, number, number];
    const c = v.screenPoint({ x: (x0 + x1) / 2, y: (y0 + y1) / 2, z: (z0 + z1) / 2 });
    let left = Infinity;
    let right = -Infinity;
    let top = Infinity;
    let bottom = -Infinity;
    for (let i = 0; i < 8; i++) {
      const p = v.screenPoint({ x: i & 1 ? x1 : x0, y: i & 2 ? y1 : y0, z: i & 4 ? z1 : z0 });
      if (!p) continue;
      left = Math.min(left, p.x);
      right = Math.max(right, p.x);
      top = Math.min(top, p.y);
      bottom = Math.max(bottom, p.y);
    }
    const clamp = (a: number, hi: number): number => Math.max(0, Math.min(hi, a));
    const box =
      left < right && top < bottom
        ? { x: r.left + clamp(left, r.width), y: r.top + clamp(top, r.height), width: clamp(right, r.width) - clamp(left, r.width), height: clamp(bottom, r.height) - clamp(top, r.height) }
        : null;
    return {
      backend: v.backend,
      canvas: { x: r.left, y: r.top, width: r.width, height: r.height },
      buffer: v.buffer,
      drawn: v.drawn,
      insets: v.insets,
      centre: c ? { x: c.x + r.left, y: c.y + r.top } : null,
      box,
    };
  });
}

/**
 * How much of the model's projected box is the model: the mock model is a
 * light grey body on a dark ground, so the pixels well lighter than the
 * ground are counted. A canvas that drew nothing scores zero.
 */
async function modelShare(page: Page, v: View): Promise<number> {
  if (!v.box || v.box.width < 8 || v.box.height < 8) return 0;
  const img = decodePng(await page.screenshot({ clip: v.box }));
  let n = 0;
  for (let i = 0; i < img.rgba.length; i += 4) {
    const luma = 0.2126 * (img.rgba[i] ?? 0) + 0.7152 * (img.rgba[i + 1] ?? 0) + 0.0722 * (img.rgba[i + 2] ?? 0);
    if (luma > 105) n++;
  }
  return n / (img.width * img.height);
}

/**
 * Watch every paint from here on. A `ResizeObserver` created after the
 * viewer's is called after it, once every animation-frame callback of the
 * frame has run and before the page is painted; a sliver whose width changes
 * each frame makes that happen every frame. Whatever the buffer holds then is
 * what is shown, so the picture in it must have been drawn at its size.
 */
async function watchPaints(page: Page): Promise<void> {
  await page.evaluate(() => {
    const w = window as unknown as {
      __cadpole: { viewport(): { buffer: { width: number; height: number }; drawn: { width: number; height: number } } };
      __paints: { stale: string[]; checked: number; leans: string[] };
    };
    w.__paints = { stale: [], checked: 0, leans: [] };
    const sliver = document.createElement("div");
    sliver.style.cssText = "position:fixed;left:0;top:0;width:1px;height:1px;pointer-events:none;opacity:0";
    document.body.appendChild(sliver);
    let wide = false;
    const tick = (): void => {
      wide = !wide;
      sliver.style.width = wide ? "2px" : "1px";
      requestAnimationFrame(tick);
    };
    requestAnimationFrame(tick);
    new ResizeObserver(() => {
      const v = w.__cadpole.viewport();
      w.__paints.checked++;
      if (v.buffer.width !== v.drawn.width || v.buffer.height !== v.drawn.height) {
        w.__paints.stale.push(`buffer ${v.buffer.width}x${v.buffer.height}, picture ${v.drawn.width}x${v.drawn.height}`);
      }
    }).observe(sliver);
    const host = document.querySelector("canvas.viewport-canvas")?.parentElement as HTMLElement;
    new MutationObserver(() => w.__paints.leans.push(host.dataset["lean"] ?? "")).observe(host, { attributes: true, attributeFilter: ["data-lean"] });
  });
}

const paints = (page: Page): Promise<{ stale: string[]; checked: number; leans: string[] }> =>
  page.evaluate(() => (window as unknown as { __paints: { stale: string[]; checked: number; leans: string[] } }).__paints);

/** GPU complaints: a frame that used something destroyed is a frame not drawn. */
function gpuErrors(page: Page): string[] {
  const errors: string[] = [];
  page.on("console", (m) => {
    if ((m.type() === "error" || m.type() === "warning") && /destroyed|GPUValidationError|context lost|INVALID_/i.test(m.text())) errors.push(m.text().slice(0, 200));
  });
  page.on("pageerror", (e) => errors.push(e.message.slice(0, 200)));
  return errors;
}

/** Once settled: the picture is the buffer's size, the model is on screen, centred in what is uncovered. */
async function expectDrawn(page: Page, what: string): Promise<View> {
  await settled(page);
  const v = await view(page);
  expect(v.backend, "the backend this project is for").toBe(test.info().project.name.includes("webgpu") ? "webgpu" : "webgl");
  expect(v.drawn, `${what}: the picture is the size of its buffer`).toEqual(v.buffer);
  expect(v.buffer.width, `${what}: the buffer follows the canvas`).toBeGreaterThanOrEqual(Math.floor(v.canvas.width));
  expect(v.centre, `${what}: the model's centre is on the canvas`).not.toBeNull();
  if (v.centre) {
    const cx = v.canvas.x + (v.insets.left + v.canvas.width - v.insets.right) / 2;
    const cy = v.canvas.y + (v.insets.top + v.canvas.height - v.insets.bottom) / 2;
    expect(Math.abs(v.centre.x - cx), `${what}: centred across the uncovered area`).toBeLessThan(1.5);
    expect(Math.abs(v.centre.y - cy), `${what}: centred up the uncovered area`).toBeLessThan(1.5);
  }
  expect(await modelShare(page, v), `${what}: the model is drawn`).toBeGreaterThan(0.08);
  return v;
}

/** Rest the pointer on the model, so its translucent hover outline is drawn. */
async function hoverModel(page: Page): Promise<void> {
  const v = await view(page);
  if (!v.centre || !v.box) throw new Error("the model is off the canvas");
  const plain = await page.screenshot({ clip: v.box });
  await page.mouse.move(v.centre.x + 3, v.centre.y + 3);
  await page.mouse.move(v.centre.x, v.centre.y);
  await settled(page);
  expect(plain.equals(await page.screenshot({ clip: v.box })), "the hover highlight is drawn").toBe(false);
}

/**
 * Put the page on a screen of another pixel ratio. The emulation changes
 * `devicePixelRatio` without the media-query change or the resize a real
 * screen change comes with, so the resize is sent by hand.
 */
async function setRatio(cdp: CDPSession, page: Page, ratio: number): Promise<void> {
  const size = page.viewportSize();
  if (!size) throw new Error("no viewport");
  await cdp.send("Emulation.setDeviceMetricsOverride", { width: size.width, height: size.height, deviceScaleFactor: ratio, mobile: false });
  await expect.poll(() => page.evaluate(() => window.devicePixelRatio)).toBe(ratio);
  await page.evaluate(() => window.dispatchEvent(new Event("resize")));
}

test.describe("a window", () => {
  test.skip(({ isMobile }) => Boolean(isMobile), "desktop layout");

  test("dragged to other sizes, turned on its side and brought back keeps the model drawn at every paint", async ({ page }) => {
    const errors = gpuErrors(page);
    await open(page);
    await hoverModel(page);
    await expectDrawn(page, "at first");
    await watchPaints(page);

    // A drag: small steps a frame or two apart, and no rest between them.
    for (let i = 1; i <= 12; i++) {
      await page.setViewportSize({ width: 1280 - i * 14, height: 800 - i * 9 });
      await page.waitForTimeout(20);
    }
    await expectDrawn(page, "after a drag smaller");
    for (let i = 11; i >= 0; i--) await page.setViewportSize({ width: 1280 - i * 14, height: 800 - i * 9 });
    await expectDrawn(page, "after a drag back");

    for (const [width, height, what] of [
      [900, 560, "a jump smaller"],
      [800, 1100, "on its side"],
      [1500, 900, "larger than it began"],
      [1280, 800, "back where it began"],
    ] as const) {
      await page.setViewportSize({ width, height });
      await expectDrawn(page, what);
    }

    const seen = await paints(page);
    expect(seen.checked, "paints were watched").toBeGreaterThan(20);
    expect(seen.stale, "paints of a buffer not drawn at its size").toEqual([]);
    expect(errors).toEqual([]);
  });

  test("resized while the camera is easing to a view, and several times in one task, keeps the model drawn", async ({ page }) => {
    const errors = gpuErrors(page);
    await open(page);
    await hoverModel(page);
    await watchPaints(page);

    await page.keyboard.press("Home");
    for (let i = 1; i <= 8; i++) {
      await page.setViewportSize({ width: 1280 - i * 20, height: 800 - i * 12 });
      await page.waitForTimeout(16);
    }
    await expectDrawn(page, "after a drag during a camera move");

    // Three sizes before the browser lays anything out: one resize, one draw.
    await page.evaluate(() => {
      const host = document.querySelector("canvas.viewport-canvas")?.parentElement as HTMLElement;
      for (const w of ["70%", "55%", "80%"]) {
        host.style.width = w;
        void host.offsetWidth;
      }
    });
    await expectDrawn(page, "after three sizes in one task");
    await page.evaluate(() => {
      (document.querySelector("canvas.viewport-canvas")?.parentElement as HTMLElement).style.width = "";
    });
    await expectDrawn(page, "with its width given back");

    expect((await paints(page)).stale).toEqual([]);
    expect(errors).toEqual([]);
  });

  test("with no area keeps its buffer, and is drawn again when it has one", async ({ page }) => {
    const errors = gpuErrors(page);
    await open(page);
    await hoverModel(page);
    const before = await expectDrawn(page, "at first");
    await watchPaints(page);

    const host = "(document.querySelector('canvas.viewport-canvas').parentElement)";
    await page.evaluate(`${host}.style.display = "none"`);
    await settled(page);
    expect((await view(page)).buffer, "hidden: the buffer is kept").toEqual(before.buffer);
    await page.evaluate(`${host}.style.display = ""`);
    expect((await expectDrawn(page, "shown again")).buffer).toEqual(before.buffer);

    await page.evaluate(`${host}.style.height = "0px"`);
    await settled(page);
    expect((await view(page)).buffer, "collapsed: the buffer is kept").toEqual(before.buffer);
    await page.evaluate(`${host}.style.height = ""`);
    await expectDrawn(page, "given its height back");

    // A size changed while nothing was laid out is picked up on the way back.
    await page.evaluate(`${host}.style.display = "none"`);
    await page.setViewportSize({ width: 1000, height: 700 });
    await page.evaluate(`${host}.style.display = ""`);
    const after = await expectDrawn(page, "shown again at another size");
    expect(after.buffer.width).toBeLessThan(before.buffer.width);

    expect((await paints(page)).stale).toEqual([]);
    expect(errors).toEqual([]);
  });

  test("on a screen of another pixel ratio is drawn again at that ratio", async ({ page }) => {
    const errors = gpuErrors(page);
    await open(page);
    await hoverModel(page);
    const one = await expectDrawn(page, "at one device pixel a CSS pixel");
    await watchPaints(page);
    const cdp = await page.context().newCDPSession(page);

    await setRatio(cdp, page, 2);
    const two = await expectDrawn(page, "at two");
    expect(two.buffer.width).toBe(one.buffer.width * 2);
    expect(two.buffer.height).toBe(one.buffer.height * 2);

    await setRatio(cdp, page, 1.5);
    const mid = await expectDrawn(page, "at one and a half");
    expect(mid.buffer.width).toBe(Math.floor(one.buffer.width * 1.5));

    // Past two the picture is not drawn any finer.
    await setRatio(cdp, page, 3);
    expect((await expectDrawn(page, "at three")).buffer).toEqual(two.buffer);

    await setRatio(cdp, page, 1);
    expect((await expectDrawn(page, "back at one")).buffer).toEqual(one.buffer);

    expect((await paints(page)).stale).toEqual([]);
    expect(errors).toEqual([]);
  });

  test("whose graphics context is lost says so, and draws the model again when the context is back", async ({ page }) => {
    await open(page);
    const before = await expectDrawn(page, "at first");
    test.skip(before.backend !== "webgl", "a WebGL context can be lost and restored on request; a WebGPU device cannot");

    await page.evaluate(() => {
      const canvas = document.querySelector("canvas.viewport-canvas") as HTMLCanvasElement;
      const lose = canvas.getContext("webgl2")?.getExtension("WEBGL_lose_context");
      if (!lose) throw new Error("no WEBGL_lose_context");
      (window as unknown as { __lose: WEBGL_lose_context }).__lose = lose;
      lose.loseContext();
    });
    const notice = page.locator(".viewport-notice");
    await expect(notice).toBeVisible();
    await expect(notice).toContainText("graphics device was lost");
    await expect(page.locator("canvas.viewport-canvas").locator("..")).toHaveAttribute("data-graphics", "restoring");

    await page.evaluate(() => (window as unknown as { __lose: WEBGL_lose_context }).__lose.restoreContext());
    await expect(notice).toBeHidden();
    await expect(page.locator("canvas.viewport-canvas").locator("..")).toHaveAttribute("data-graphics", "ok");
    const after = await expectDrawn(page, "with the context back");
    expect(after.buffer).toEqual(before.buffer);

    // And it is a working viewport, not a last picture: a resize is drawn.
    await page.setViewportSize({ width: 1100, height: 720 });
    await expectDrawn(page, "resized after the context came back");
  });

  // A dropped WebGPU device rejects the renderer's pending calls with nobody
  // awaiting them, sometimes without its `lost` promise ever settling. Forced
  // here with the rejection such a device leaves, on either backend: the
  // viewport is brought back, and nothing is left uncaught.
  test("whose device is dropped mid-frame brings the view back, with no uncaught error", async ({ page }) => {
    const pageErrors: string[] = [];
    page.on("pageerror", (e) => pageErrors.push(e.message));
    await open(page);
    const before = await expectDrawn(page, "at first");
    const states = await page.evaluate(async () => {
      const host = document.querySelector("canvas.viewport-canvas")!.parentElement!;
      const seen: string[] = [];
      new MutationObserver(() => seen.push(host.dataset["graphics"] ?? "")).observe(host, { attributes: true, attributeFilter: ["data-graphics"] });
      void Promise.reject(new DOMException("Instance dropped in popErrorScope", "OperationError"));
      for (let i = 0; i < 100 && seen.at(-1) !== "ok"; i++) await new Promise((r) => setTimeout(r, 50));
      return seen;
    });
    expect(states).toEqual(["restoring", "ok"]);
    await expect(page.locator(".viewport-notice")).toHaveCount(0);
    const after = await expectDrawn(page, "after the device came back");
    expect(after.buffer).toEqual(before.buffer);
    expect(pageErrors).toEqual([]);
  });
});

// The layout at every window a desktop browser can be given, an embedding
// frame's included: short and wide, tall and narrow. The grid has a narrow
// form and, for a phone on its side, a short one, each with its own set of
// panels; a window that got one form's grid with the other's panels showed
// the viewport as a thumbnail in a corner under overlapping panels.
test.describe("a window of any size", () => {
  test.skip(({ isMobile }) => Boolean(isMobile), "a mouse, at every size");

  test("keeps the viewport the largest pane, with no pane over another", async ({ page }) => {
    await open(page);
    const widths = [360, 520, 640, 900, 901, 1024, 1204, 1300, 1460, 1920];
    const heights = [440, 500, 501, 540, 600, 743, 900, 1100];
    const wrong: string[] = [];
    for (const width of widths) {
      for (const height of heights) {
        await page.setViewportSize({ width, height });
        const found = await page.evaluate(() => {
          const app = document.querySelector(".app") as HTMLElement;
          const canvas = document.querySelector("canvas.viewport-canvas") as HTMLElement;
          const c = canvas.getBoundingClientRect();
          // The grid's own panes: what is laid out in it and takes room.
          const panes = [...app.children].filter((el) => {
            const cs = getComputedStyle(el);
            const r = el.getBoundingClientRect();
            return cs.display !== "none" && cs.visibility !== "hidden" && cs.position !== "absolute" && cs.position !== "fixed" && r.width > 1 && r.height > 1;
          });
          const out: string[] = [];
          const name = (el: Element): string => `.${el.className.toString().split(" ")[0]}`;
          for (let i = 0; i < panes.length; i++) {
            const a = panes[i]!.getBoundingClientRect();
            if (a.left < -1 || a.top < -1 || a.right > innerWidth + 1 || a.bottom > innerHeight + 1) out.push(`${name(panes[i]!)} runs off the window`);
            if (!panes[i]!.contains(canvas) && a.width * a.height > c.width * c.height) out.push(`${name(panes[i]!)} is larger than the viewport`);
            for (let j = i + 1; j < panes.length; j++) {
              const b = panes[j]!.getBoundingClientRect();
              const w = Math.min(a.right, b.right) - Math.max(a.left, b.left);
              const h = Math.min(a.bottom, b.bottom) - Math.max(a.top, b.top);
              // A bar laid over the viewport, inside its box, is chrome on the picture (the narrow layout's toolbar).
              const inside = (x: DOMRect, y: DOMRect): boolean => x.left >= y.left - 1 && x.top >= y.top - 1 && x.right <= y.right + 1 && x.bottom <= y.bottom + 1;
              const viewport = canvas.parentElement as HTMLElement;
              if ((panes[i] === viewport && inside(b, a)) || (panes[j] === viewport && inside(a, b))) continue;
              if (w > 1 && h > 1) out.push(`${name(panes[i]!)} and ${name(panes[j]!)} overlap by ${Math.round(w)} x ${Math.round(h)}`);
            }
          }
          if (c.height < innerHeight * 0.55) out.push(`the viewport is ${Math.round(c.height)} px of ${innerHeight} high`);
          if (c.width < Math.min(280, innerWidth * 0.8)) out.push(`the viewport is ${Math.round(c.width)} px of ${innerWidth} wide`);
          return out;
        });
        for (const f of found) wrong.push(`${width} x ${height}: ${f}`);
      }
    }
    expect(wrong).toEqual([]);

    // And at the sizes that went wrong, the model is drawn in it.
    for (const [width, height] of [[1460, 480], [1300, 500], [1204, 560], [1204, 743]] as const) {
      await page.setViewportSize({ width, height });
      await expectDrawn(page, `${width} x ${height}`);
    }
  });
});

test.describe("a phone", () => {
  test.skip(({ isMobile }) => !isMobile, "phone layout");
  // Two device pixels a CSS pixel, so the lighter picture is a smaller buffer.
  test.use({ deviceScaleFactor: 2 });

  test("turned on its side and back with a sheet over the viewport keeps the model drawn in what is uncovered", async ({ page }) => {
    const errors = gpuErrors(page);
    await open(page);
    await page.getByRole("navigation", { name: "Panels" }).getByRole("button", { name: "Chat" }).click();
    await expect(page.getByRole("dialog")).toHaveAttribute("data-tab", "messages");
    const covered = await expectDrawn(page, "with the sheet open");
    expect(covered.insets.bottom).toBeGreaterThan(100);
    await watchPaints(page);

    const size = page.viewportSize();
    if (!size) throw new Error("no viewport");
    for (let i = 1; i <= 8; i++) {
      await page.setViewportSize({ width: size.width - i * 6, height: size.height - i * 10 });
      await page.waitForTimeout(20);
    }
    await expectDrawn(page, "after a drag smaller");
    await page.setViewportSize({ width: size.height, height: size.width });
    await expectDrawn(page, "on its side");
    await page.setViewportSize(size);
    const back = await expectDrawn(page, "upright again");
    expect(back.insets.bottom).toBeGreaterThan(100);

    expect((await paints(page)).stale).toEqual([]);
    expect(errors).toEqual([]);
  });

  test("drawing the lighter picture while the view moves, and the full one at rest, never shows an undrawn buffer", async ({ page }) => {
    const errors = gpuErrors(page);
    await open(page);
    const cdp = await page.context().newCDPSession(page);
    await hoverModel(page);
    const rest = await expectDrawn(page, "at rest");
    expect(rest.buffer.width).toBe(Math.floor(rest.canvas.width) * 2);
    await watchPaints(page);

    // Frames that come slowly while a finger turns the model: every frame
    // is held up, as a phone that cannot keep up holds them.
    await page.evaluate(() => {
      const w = window as unknown as { __slow: boolean };
      w.__slow = true;
      const burn = (): void => {
        const t = performance.now();
        while (performance.now() - t < 34) {
          /* busy */
        }
        if (w.__slow) requestAnimationFrame(burn);
      };
      requestAnimationFrame(burn);
    });
    const at = { x: rest.canvas.x + rest.canvas.width / 2, y: rest.canvas.y + rest.canvas.height * 0.3 };
    const point = (x: number, y: number): { x: number; y: number; id: number }[] => [{ x: Math.round(x), y: Math.round(y), id: 1 }];
    await cdp.send("Input.dispatchTouchEvent", { type: "touchStart", touchPoints: point(at.x, at.y) });
    // The first move is well past a tap's slop: a finger that lingers is a long press, not a drag.
    for (let i = 1; i <= 24; i++) {
      await cdp.send("Input.dispatchTouchEvent", { type: "touchMove", touchPoints: point(at.x + 40 + i * 4, at.y + i * 2) });
      await page.waitForTimeout(30);
    }
    const moving = await view(page);
    expect(moving.buffer.width, `the lighter picture is one device pixel a CSS pixel (levels drawn: ${(await paints(page)).leans.join(" ") || "none"})`).toBe(Math.floor(moving.canvas.width));
    expect(moving.drawn).toEqual(moving.buffer);
    await cdp.send("Input.dispatchTouchEvent", { type: "touchEnd", touchPoints: [] });
    await page.evaluate(() => {
      (window as unknown as { __slow: boolean }).__slow = false;
    });

    // At rest the full picture is back.
    await expect.poll(async () => (await view(page)).buffer.width).toBe(rest.buffer.width);
    const after = await view(page);
    expect(after.drawn).toEqual(after.buffer);

    const seen = await paints(page);
    expect(seen.leans.some((l) => l !== "0"), "the lighter picture was drawn").toBe(true);
    expect(seen.leans[seen.leans.length - 1]).toBe("0");
    expect(seen.stale, "paints of a buffer not drawn at its size").toEqual([]);
    expect(errors).toEqual([]);
  });

  test("a sheet opening and closing does not drop the picture's resolution", async ({ page }) => {
    await open(page);
    await page.emulateMedia({ reducedMotion: "no-preference" });
    const rest = await expectDrawn(page, "at rest");
    await watchPaints(page);
    await page.evaluate(() => {
      const w = window as unknown as { __slow: boolean };
      w.__slow = true;
      const burn = (): void => {
        const t = performance.now();
        while (performance.now() - t < 34) {
          /* busy */
        }
        if (w.__slow) requestAnimationFrame(burn);
      };
      requestAnimationFrame(burn);
    });
    const chat = page.getByRole("navigation", { name: "Panels" }).getByRole("button", { name: "Chat" });
    await chat.click();
    await expect(page.getByRole("dialog")).toHaveAttribute("data-tab", "messages");
    await settled(page, 700);
    await page.getByRole("dialog").getByRole("button", { name: "Close panel" }).click();
    await settled(page, 700);
    await page.evaluate(() => {
      (window as unknown as { __slow: boolean }).__slow = false;
    });
    const seen = await paints(page);
    expect(seen.leans, "the picture slid out from under the sheet at full resolution").toEqual([]);
    expect(seen.stale).toEqual([]);
    expect((await expectDrawn(page, "with the sheet closed again")).buffer).toEqual(rest.buffer);
  });
});
