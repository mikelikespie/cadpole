// The live 3D, on a phone-sized touch window: a card's model comes up in the
// lightbox with a "Full viewer" link that names it and carries the camera, a
// scene's model opens in place, the playground's engine starts and evaluates
// the script, and a viewer page opens its own model.
import type { Page } from "@playwright/test";

import { context, expect, test } from "./site.js";

test("lightbox, scene, playground and a viewer page", async ({ browser, site }) => {
  const ctx = await context(browser, "phone-390");
  const p = await ctx.newPage();
  const errs: string[] = [];
  p.on("pageerror", (e) => errs.push(String(e).slice(0, 200)));
  await p.goto(site + "?noauto=1", { waitUntil: "load" });
  const link = (scope: string) => p.evaluate((scope) => {
    const a = document.querySelector<HTMLAnchorElement>(scope + " a[data-full]")!;
    a.dispatchEvent(new PointerEvent("pointerdown"));
    return { href: a.getAttribute("href") ?? "", target: a.target, rel: a.rel };
  }, scope);

  // Three lightboxes in one page: a feature card, a gallery card, the cabin.
  const pics = await p.evaluate(() => [...document.querySelectorAll<HTMLElement>("button[data-pic][data-sheet3d]")].map((el) => el.dataset["sheet3d"]!.replace(/^.*\//, "").replace(/\.cpmesh$/, "")));
  expect(pics.length).toBeGreaterThan(25);
  for (const model of ["feat-loft", "mascot", "plan-cabin"]) {
    const i = pics.indexOf(model);
    expect(i, model).toBeGreaterThanOrEqual(0);
    await p.evaluate((i) => { const el = document.querySelectorAll<HTMLElement>("button[data-pic][data-sheet3d]")[i]!; el.scrollIntoView({ block: "center", behavior: "instant" }); el.click(); }, i);
    await p.waitForSelector(".sheet-view.is-live .live canvas", { timeout: 60_000 });
    await expect(p.locator(".sheet-foot")).toContainText("triangles");
    const l = await link("dialog.sheet");
    expect(l.href).toMatch(new RegExp(`^view/${model}/\\?eye=[-\\d.,]+&target=[-\\d.,]+$`));
    expect(l.target).toBe("_blank");
    expect(l.rel).toContain("noopener");
    await p.evaluate(() => document.querySelector<HTMLElement>("[data-sheet-close]")!.click());
    await p.waitForTimeout(150);
  }

  // A drawing opens as the vector file itself.
  await p.evaluate(() => { const b = document.querySelector<HTMLElement>("[data-draw]")!; b.scrollIntoView({ block: "center", behavior: "instant" }); b.click(); });
  await p.waitForSelector(".sheet-draw svg", { timeout: 20_000 });
  await p.evaluate(() => document.querySelector<HTMLElement>("[data-sheet-close]")!.click());

  // The first scene, in place.
  await p.evaluate(() => { const s = document.querySelector(".scene")!, r = s.getBoundingClientRect(); scrollTo(0, r.top + scrollY + 0.15 * (r.height - innerHeight)); });
  await p.waitForTimeout(600);
  await p.evaluate(() => document.querySelector<HTMLElement>("[data-open3d]")!.click());
  await p.waitForSelector(".vis.is-live .live canvas", { timeout: 60_000 });
  expect((await link(".scene")).href).toMatch(/^view\/turbojet_cut\/\?eye=/);
  await p.evaluate(() => document.querySelector<HTMLElement>(".scene [data-close3d]")!.click());

  // The playground: the real engine, compiled and run in the page.
  await p.evaluate(() => { document.querySelector("#play")!.scrollIntoView({ behavior: "instant" }); (window as unknown as { __play: { start(): void } }).__play.start(); });
  await p.waitForSelector(".play-view.is-live .live canvas", { timeout: 120_000 });
  await expect(p.locator("#play .status")).toContainText("in your browser");
  expect(await p.locator("#play .sliders input").count()).toBeGreaterThan(0);
  expect(await p.evaluate(() => visualViewport!.scale)).toBe(1);
  expect(errs).toEqual([]);

  // A viewer page: its own model, and a way back to the section it came from.
  const v = await ctx.newPage();
  const got: [string, number][] = [];
  v.on("response", (r) => { if (/\.cadpole$/.test(r.url())) got.push([r.url().replace(/^.*\/view\//, ""), r.status()]); });
  await v.goto(site + "view/part/", { waitUntil: "load" });
  await v.waitForSelector("canvas", { timeout: 60_000 });
  await expect.poll(() => got.length, { timeout: 30_000 }).toBeGreaterThan(0);
  expect(got.every(([u, s]) => u.startsWith("part/") && s === 200)).toBe(true);
  expect(await v.evaluate(() => [...document.querySelectorAll<HTMLAnchorElement>("a[href]")].some((a) => /\/#play$/.test(a.href)))).toBe(true);
  await ctx.close();
});

/**
 * How much of a live canvas is a model: the share of its pixels, as the page
 * shows them, that are well lighter than the page's dark ground. A canvas
 * that drew nothing, and a section with no canvas at all, score zero.
 */
async function drawn(p: Page, scope: string): Promise<number> {
  const canvas = p.locator(`${scope} .live canvas`);
  if ((await canvas.count()) === 0) return 0;
  // Two frames, so what was asked for last has been drawn.
  await p.evaluate(() => new Promise<void>((r) => requestAnimationFrame(() => requestAnimationFrame(() => r()))));
  const png = [...(await canvas.screenshot())];
  return p.evaluate(async (bytes) => {
    const bmp = await createImageBitmap(new Blob([new Uint8Array(bytes)], { type: "image/png" }));
    const g = new OffscreenCanvas(bmp.width, bmp.height).getContext("2d")!;
    g.drawImage(bmp, 0, 0);
    const d = g.getImageData(0, 0, bmp.width, bmp.height).data;
    let n = 0;
    for (let i = 0; i < d.length; i += 4) if (0.2126 * d[i]! + 0.7152 * d[i + 1]! + 0.0722 * d[i + 2]! > 70) n++;
    return n / (d.length / 4);
  }, png);
}

// The page has one WebGL context, which moves between the playground, a
// scene and the lightbox. Whoever has it must show a model, at any window
// size; and the playground, which stays on the page, must get it back.
test("the playground and the lightbox draw the model, through a resize and through lending the stage", async ({ browser, site }) => {
  const ctx = await context(browser, "desk-1440");
  const p = await ctx.newPage();
  const errs: string[] = [];
  p.on("pageerror", (e) => errs.push(String(e).slice(0, 200)));
  await p.goto(site + "?noauto=1", { waitUntil: "load" });
  const toPlay = () => p.evaluate(() => document.querySelector(".play-view")!.scrollIntoView({ block: "center", behavior: "instant" }));
  const live = () => p.waitForSelector(".play-view.is-live .live canvas", { timeout: 120_000 });

  await p.evaluate(() => (window as unknown as { __play: { start(): void } }).__play.start());
  await toPlay();
  await live();
  await expect(p.locator("#play .status")).toContainText("in your browser");
  expect(await drawn(p, ".play-view"), "after the first evaluation").toBeGreaterThan(0.03);

  // The window dragged smaller a step at a time, a jump, and back.
  for (let i = 1; i <= 8; i++) await p.setViewportSize({ width: 1440 - i * 30, height: 900 - i * 15 });
  await toPlay();
  expect(await drawn(p, ".play-view"), "after a drag smaller").toBeGreaterThan(0.03);
  for (const [width, height] of [[820, 1100], [1440, 900]] as const) {
    await p.setViewportSize({ width, height });
    await toPlay();
    expect(await drawn(p, ".play-view"), `at ${width} x ${height}`).toBeGreaterThan(0.03);
  }

  // A slider moved, so what comes back later is this model and not the first.
  const slider = p.locator("#play .sliders input").first();
  await slider.evaluate((el: HTMLInputElement) => { el.value = el.max; el.dispatchEvent(new Event("input", { bubbles: true })); el.dispatchEvent(new Event("change", { bubbles: true })); });
  const moved = await p.locator("#play .sliders output").first().textContent();
  await expect.poll(() => drawn(p, ".play-view")).toBeGreaterThan(0.03);

  // The lightbox takes the stage, draws its own model, through a resize too.
  await p.evaluate(() => { const el = document.querySelector<HTMLElement>("button[data-pic][data-sheet3d]")!; el.scrollIntoView({ block: "center", behavior: "instant" }); el.click(); });
  await p.waitForSelector(".sheet-view.is-live .live canvas", { timeout: 60_000 });
  expect(await drawn(p, ".sheet-view"), "in the lightbox").toBeGreaterThan(0.03);
  await p.setViewportSize({ width: 1100, height: 760 });
  expect(await drawn(p, ".sheet-view"), "in the lightbox, resized").toBeGreaterThan(0.03);
  // Meanwhile the playground shows its picture, not an empty box.
  expect(await p.evaluate(() => document.querySelector(".play-view")!.classList.contains("is-live"))).toBe(false);
  await p.evaluate(() => document.querySelector<HTMLElement>("[data-sheet-close]")!.click());
  await p.setViewportSize({ width: 1440, height: 900 });

  // Back at the playground the model is there again, with the slider where it was.
  await toPlay();
  await live();
  await expect.poll(() => drawn(p, ".play-view"), { message: "after the lightbox gave the stage back" }).toBeGreaterThan(0.03);
  expect(await p.locator("#play .sliders output").first().textContent()).toBe(moved);

  // And when the browser takes the context away: a new one, and the model again.
  await p.evaluate(() => {
    const c = document.querySelector<HTMLCanvasElement>(".play-view .live canvas")!;
    (c.getContext("webgl2") ?? c.getContext("webgl"))!.getExtension("WEBGL_lose_context")!.loseContext();
  });
  await live();
  await expect.poll(() => drawn(p, ".play-view"), { message: "after a lost context" }).toBeGreaterThan(0.03);
  expect(await p.locator(".play-view .live canvas").count()).toBe(1);

  expect(errs).toEqual([]);
  await ctx.close();
});
