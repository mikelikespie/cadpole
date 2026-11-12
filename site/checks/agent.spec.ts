// The agent recording: one 16:10 recording on every window, as wide as its
// column, with the round play button and the caption under the frame and never
// over it. It starts by itself, muted, when it comes into view.
import { existsSync } from "node:fs";
import { resolve } from "node:path";

import { VIEWPORTS, context, expect, root, test } from "./site.js";

/** Whether this copy of the site carries the recording itself, or only its poster. */
const hasVideo = existsSync(resolve(root, "agent/agent.webm"));

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
    // The play button is a round 44 px target, and it and the caption share a row.
    expect(Math.round(geo.play.w)).toBe(44);
    expect(Math.round(geo.play.h)).toBe(44);
    expect(geo.cap.l).toBeGreaterThanOrEqual(geo.play.r);
    expect(geo.cap.t).toBeLessThan(geo.play.b);
    await expect(p.locator("[data-vplay]")).toHaveAttribute("aria-label", "Play recording");
    // The video it makes is the one recording, whatever the window.
    await p.locator("[data-vplay]").click();
    const srcs = await p.evaluate(() => [...document.querySelectorAll<HTMLSourceElement>(".agent-vid video source")].map((s) => s.getAttribute("src")!));
    expect(srcs.map((s) => s.replace(/\?.*/, ""))).toEqual(["agent/agent.webm", "agent/agent.mp4"]);
    await ctx.close();
  });
}

test("on a phone the frame, Play, the step and the step row fit above the bottom bar, and a segment goes to its step", async ({ browser, site }) => {
  const ctx = await context(browser, "phone-390");
  const p = await ctx.newPage();
  await p.goto(site + "?noauto=1", { waitUntil: "load" });
  await p.evaluate(() => document.querySelector(".agent-vid")!.scrollIntoView({ block: "start" }));
  await p.waitForTimeout(300);
  const geo = await p.evaluate(() => {
    const r = (s: string) => document.querySelector(s)!.getBoundingClientRect();
    return { vid: r(".agent-vid").top, prog: r(".agent-prog").bottom, bar: r(".bar").top, list: r(".agent-steps").height, n: document.querySelectorAll(".agent-prog button").length, steps: document.querySelectorAll(".agent-steps li").length };
  });
  expect(geo.vid).toBeGreaterThanOrEqual(0);
  expect(geo.prog).toBeLessThanOrEqual(geo.bar);
  expect(geo.list, "the list is for screen readers only").toBeLessThanOrEqual(1);
  expect(geo.n).toBe(geo.steps);
  await p.locator(".agent-prog button").nth(3).click();
  await expect(p.locator(".agent-prog button").nth(3)).toHaveAttribute("aria-current", "step");
  await expect(p.locator(".agent-n")).toHaveText(`4 / ${geo.n}`);
  await expect(p.locator(".agent-cap")).toHaveText((await p.locator(".agent-steps li").nth(3).textContent())!);
  await ctx.close();
});

for (const name of ["phone-390", "desk-1440"]) {
  test(`the recording starts by itself in view, the button pauses it, and the steps follow it, ${name}`, async ({ browser, site }) => {
    test.skip(!hasVideo, "this copy of the site has no recording");
    const ctx = await context(browser, name);
    const p = await ctx.newPage();
    await p.goto(site, { waitUntil: "load" });
    const btn = p.locator("[data-vplay]");
    expect(await p.locator(".agent-vid video").count(), "nothing is fetched before it is near").toBe(0);
    await p.evaluate(() => document.querySelector(".agent-vid")!.scrollIntoView({ block: "center" }));
    await expect(btn).toHaveAttribute("aria-label", "Pause recording");
    await expect.poll(() => p.evaluate(() => { const v = document.querySelector<HTMLVideoElement>(".agent-vid video")!; return !v.paused && v.muted && v.loop && v.playsInline; })).toBe(true);
    // The highlighted step is the one the recording is in.
    const first = await p.evaluate(() => +document.querySelector<HTMLElement>(".agent-steps li")!.dataset["t"]!);
    await expect.poll(() => p.evaluate(() => document.querySelector<HTMLVideoElement>(".agent-vid video")!.currentTime), { timeout: 20_000 }).toBeGreaterThan(first + 0.3);
    const at = await p.evaluate(() => {
      const t = document.querySelector<HTMLVideoElement>(".agent-vid video")!.currentTime;
      const li = [...document.querySelectorAll<HTMLElement>(".agent-steps li")];
      const cur = li.filter((l) => +l.dataset["t"]! <= t + 0.05).pop()!;
      return { i: li.indexOf(cur), text: cur.textContent, cap: document.querySelector(".agent-cap")!.textContent, on: li.findIndex((l) => l.classList.contains("on")) };
    });
    expect(at.on).toBe(at.i);
    expect(at.cap).toBe(at.text);
    // The button pauses it, and it stays paused when it leaves and comes back.
    await btn.click();
    await expect(btn).toHaveAttribute("aria-label", "Play recording");
    expect(await p.evaluate(() => document.querySelector<HTMLVideoElement>(".agent-vid video")!.paused)).toBe(true);
    await p.evaluate(() => scrollTo(0, 0));
    await p.waitForTimeout(300);
    await p.evaluate(() => document.querySelector(".agent-vid")!.scrollIntoView({ block: "center" }));
    await p.waitForTimeout(500);
    expect(await p.evaluate(() => document.querySelector<HTMLVideoElement>(".agent-vid video")!.paused)).toBe(true);
    await btn.click();
    await expect(btn).toHaveAttribute("aria-label", "Pause recording");
    // Out of view it pauses by itself, and back in view it plays again.
    await p.evaluate(() => scrollTo(0, 0));
    await expect.poll(() => p.evaluate(() => document.querySelector<HTMLVideoElement>(".agent-vid video")!.paused)).toBe(true);
    await p.evaluate(() => document.querySelector(".agent-vid")!.scrollIntoView({ block: "center" }));
    await expect.poll(() => p.evaluate(() => document.querySelector<HTMLVideoElement>(".agent-vid video")!.paused)).toBe(false);
    await ctx.close();
  });
}

test("a visitor who asked for less motion gets the poster and the play button, not a moving recording", async ({ browser, site }) => {
  const v = VIEWPORTS["phone-390"]!;
  const ctx = await browser.newContext({ viewport: { width: v.width, height: v.height }, deviceScaleFactor: v.dpr, hasTouch: true, isMobile: true, reducedMotion: "reduce" });
  const p = await ctx.newPage();
  await p.goto(site, { waitUntil: "load" });
  await p.evaluate(() => document.querySelector(".agent-vid")!.scrollIntoView({ block: "center" }));
  await p.waitForTimeout(1000);
  expect(await p.evaluate(() => { const v = document.querySelector<HTMLVideoElement>(".agent-vid video"); return !v || v.paused; })).toBe(true);
  await expect(p.locator("[data-vplay]")).toHaveAttribute("aria-label", "Play recording");
  await p.locator("[data-vplay]").click();
  await expect(p.locator("[data-vplay]")).toHaveAttribute("aria-label", hasVideo ? "Pause recording" : /recording/);
  await ctx.close();
});

test("no phone recording on the page",async ({ browser, site }) => {
  const ctx = await context(browser, "phone-390");
  const p = await ctx.newPage();
  await p.goto(site, { waitUntil: "load" });
  expect(await p.content()).not.toContain("agent-phone");
  await ctx.close();
});

test("the pictures first, the reading after: setup and Get started come last, and nothing skips down to them", async ({ browser, site }) => {
  const ctx = await context(browser, "desk-1440");
  const p = await ctx.newPage();
  await p.goto(site + "?rm=1", { waitUntil: "load" });
  const ids = await p.evaluate(() => [...document.querySelectorAll("main > section[id]")].map((s) => s.id));
  const at = (id) => { const i = ids.indexOf(id); expect(i, id).toBeGreaterThanOrEqual(0); return i; };
  for (const shown of ["agent", "modes", "play", "can", "draw", "gallery"]) expect(at(shown)).toBeLessThan(at("tools"));
  expect(at("tools")).toBeLessThan(at("connect"));
  expect(at("connect")).toBeLessThan(at("start"));
  expect(await p.locator("#agent .agent-how").count()).toBe(0);
  // Only the skip link and the header nav jump within the page; the hero, the
  // quick-action bar, the agent section and the mode cards go to GitHub or nowhere.
  expect(await p.locator('.hero-cta a[href^="#"], .bar a[href^="#"], #agent a[href^="#"], #modes a[href^="#"]').count()).toBe(0);
  expect(await p.locator(".hero-cta a").count()).toBe(1);
  expect(await p.locator(".hero-cta a.primary").textContent()).toContain("GitHub");
  expect(await p.locator(".bar a").count()).toBe(1);
  expect(await p.locator(".bar a.primary").textContent()).toContain("GitHub");
  await ctx.close();
});
