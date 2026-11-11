// The agent recording: one 16:10 recording on every window, as wide as its
// column, with Play and the caption under the frame and never over it.
import { VIEWPORTS, context, expect, test } from "./site.js";

for (const name of Object.keys(VIEWPORTS)) {
  test(`agent recording and caption, ${name}`, async ({ browser, site }) => {
    const ctx = await context(browser, name);
    const p = await ctx.newPage();
    await p.goto(site + "?noauto=1", { waitUntil: "load" });
    await p.locator(".agent-vid").scrollIntoViewIfNeeded();
    await p.waitForTimeout(300);
    const geo = await p.evaluate(() => {
      const box = document.querySelector<HTMLElement>(".agent-vid")!, main = box.parentElement!;
      const r = (e: Element) => { const b = e.getBoundingClientRect(); return { l: b.left, t: b.top, r: b.right, b: b.bottom, w: b.width, h: b.height }; };
      return {
        box: r(box), main: r(main), ctl: r(document.querySelector(".agent-ctl")!), cap: r(document.querySelector(".agent-cap")!),
        play: r(document.querySelector("[data-vplay]")!), poster: document.querySelector<HTMLImageElement>(".agent-vid img")!.currentSrc,
      };
    });
    expect(geo.poster).toContain("agent/agent-poster.webp");
    expect(Math.abs(geo.box.w / geo.box.h - 1.6), `aspect ${geo.box.w}x${geo.box.h}`).toBeLessThan(0.02);
    expect(Math.abs(geo.box.w - geo.main.w)).toBeLessThan(1);
    expect(geo.box.r).toBeLessThanOrEqual((await p.evaluate(() => innerWidth)) + 0.5);
    for (const [k, e] of [["controls", geo.ctl], ["caption", geo.cap], ["play", geo.play]] as const) expect(e.t, `${k} under the frame`).toBeGreaterThanOrEqual(geo.box.b - 0.5);
    // Play and the caption share a row.
    expect(geo.cap.l).toBeGreaterThanOrEqual(geo.play.r);
    expect(geo.cap.t).toBeLessThan(geo.play.b);
    // The video it makes is the one recording, whatever the window.
    await p.locator("[data-vplay]").click();
    const srcs = await p.evaluate(() => [...document.querySelectorAll<HTMLSourceElement>(".agent-vid video source")].map((s) => s.getAttribute("src")!));
    expect(srcs.map((s) => s.replace(/\?.*/, ""))).toEqual(["agent/agent.webm", "agent/agent.mp4"]);
    await ctx.close();
  });
}

test("no phone recording on the page", async ({ browser, site }) => {
  const ctx = await context(browser, "phone-390");
  const p = await ctx.newPage();
  await p.goto(site, { waitUntil: "load" });
  expect(await p.content()).not.toContain("agent-phone");
  await ctx.close();
});

test("the pictures first, the reading after: setup and Get started come last, and the link down reaches them", async ({ browser, site }) => {
  const ctx = await context(browser, "desk-1440");
  const p = await ctx.newPage();
  await p.goto(site + "?rm=1", { waitUntil: "load" });
  const ids = await p.evaluate(() => [...document.querySelectorAll("main > section[id]")].map((s) => s.id));
  const at = (id) => { const i = ids.indexOf(id); expect(i, id).toBeGreaterThanOrEqual(0); return i; };
  for (const shown of ["agent", "modes", "play", "can", "draw", "gallery"]) expect(at(shown)).toBeLessThan(at("tools"));
  expect(at("tools")).toBeLessThan(at("connect"));
  expect(at("connect")).toBeLessThan(at("start"));
  expect(await p.locator("#agent .agent-how").count()).toBe(0);
  await p.locator('#agent a[href="#connect"]').click();
  await p.waitForTimeout(300);
  const top = await p.evaluate(() => document.getElementById("connect")!.getBoundingClientRect().top);
  expect(top).toBeGreaterThanOrEqual(0);
  expect(top).toBeLessThan(200);
  expect(new URL(p.url()).hash).toBe("#connect");
  await ctx.close();
});
