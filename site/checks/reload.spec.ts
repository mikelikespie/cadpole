// A reload lands where the visitor was, on every window; a fresh visit lands
// at the top, or at the section its address names.
import { VIEWPORTS, context, expect, test } from "./site.js";

for (const name of Object.keys(VIEWPORTS)) {
  test(`reload position, ${name}`, async ({ browser, site }) => {
    const ctx = await context(browser, name);
    const p = await ctx.newPage();
    await p.goto(site + "?noauto=1", { waitUntil: "load" });
    await p.waitForTimeout(800);
    const spots: Record<string, number> = await p.evaluate(() => {
      const y = (s: string, f = 0) => { const r = document.querySelector(s)!.getBoundingClientRect(); return Math.round(r.top + scrollY + r.height * f); };
      return { top: 0, "scene1-mid": y("#turbojet", 0.45), "scene3-mid": y("#mug", 0.5), play: y("#play", 0.2), gallery: y("#gallery", 0.1), bottom: document.documentElement.scrollHeight - innerHeight };
    });
    const state = () => p.evaluate(() => ({
      y: Math.round(scrollY),
      scenes: ((window as unknown as { __scenes?: { active: boolean; el: HTMLElement; p: number }[] }).__scenes ?? []).filter((s) => s.active).map((s) => `${s.el.id}@${s.p.toFixed(3)}`).join(" "),
    }));
    const bad: string[] = [];
    for (const [k, y] of Object.entries(spots)) {
      await p.evaluate((y) => scrollTo({ top: y, behavior: "instant" }), y);
      await p.waitForTimeout(900);
      const a = await state();
      await p.reload({ waitUntil: "load" });
      await p.waitForTimeout(1800);
      const c = await state();
      if (Math.abs(c.y - a.y) > 2 || c.scenes !== a.scenes) bad.push(`${k}: before y=${a.y} [${a.scenes}], after y=${c.y} [${c.scenes}]`);
    }
    for (const hash of ["", "#gallery", "#start"]) {
      const q = await ctx.newPage();
      await q.goto(site + hash, { waitUntil: "load" });
      await q.waitForTimeout(1800);
      const r = await q.evaluate((hash) => ({ y: Math.round(scrollY), want: hash ? Math.round(document.querySelector(hash)!.getBoundingClientRect().top) : 0 }), hash);
      if (hash ? r.want < -2 || r.want > 80 : r.y !== 0) bad.push(`fresh ${hash || "(no hash)"}: y=${r.y}, target top at ${r.want}px`);
      await q.close();
    }
    expect(bad).toEqual([]);
    await ctx.close();
  });
}

test("back at the top, the address drops the section it named; a reload stays at the top", async ({ browser, site }) => {
  const ctx = await context(browser, "desk-1440");
  const p = await ctx.newPage();
  await p.goto(site + "?noauto=1", { waitUntil: "load" });
  await p.locator('.top nav a[href="#play"]').click();
  await expect.poll(() => p.evaluate(() => location.hash)).toBe("#play");
  await p.waitForTimeout(600);
  await p.evaluate(() => scrollTo({ top: 0, behavior: "instant" }));
  await p.mouse.wheel(0, -50);
  await expect.poll(() => p.evaluate(() => location.hash)).toBe("");
  await p.reload({ waitUntil: "load" });
  await p.waitForTimeout(1500);
  expect(await p.evaluate(() => Math.round(scrollY))).toBeLessThan(5);
  await ctx.close();
});
