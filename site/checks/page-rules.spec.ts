// Page-wide rules on every window: the page is not zoomed and cannot scroll
// sideways, no form control has text under 16 px on a touch window (a phone
// zooms into those), nothing visible is wider than the window, tap targets
// on touch windows are 44 px (but for links inside text), and nothing throws.
import { VIEWPORTS, context, expect, test } from "./site.js";

for (const [name, v] of Object.entries(VIEWPORTS)) {
  test(`page rules, ${name}`, async ({ browser, site }) => {
    const ctx = await context(browser, name);
    const p = await ctx.newPage();
    const errs: string[] = [];
    p.on("pageerror", (e) => errs.push(String(e).slice(0, 160)));
    await p.goto(site + "?noauto=1", { waitUntil: "load" });
    await p.waitForTimeout(600);
    const H = await p.evaluate(() => document.documentElement.scrollHeight);
    for (let y = 0; y < H; y += v.height * 2) {
      await p.evaluate((y) => scrollTo(0, y), y);
      await p.waitForTimeout(40);
    }
    await p.waitForTimeout(500);
    const r = await p.evaluate((touch) => {
      const vis = (e: Element) => {
        const s = getComputedStyle(e);
        if (s.display === "none" || s.visibility === "hidden" || +s.opacity === 0) return false;
        const r = e.getBoundingClientRect();
        return r.width > 0 && r.height > 0 && !e.closest("dialog:not([open]), [hidden], details:not([open]) > :not(summary)");
      };
      const tag = (e: Element) => e.tagName.toLowerCase() + (e.id ? "#" + e.id : "") + (typeof e.className === "string" && e.className ? "." + e.className.trim().split(/\s+/).slice(0, 2).join(".") : "");
      const out = { scale: visualViewport!.scale, sideways: document.documentElement.scrollWidth - innerWidth, scrolledX: 0, small: [] as string[], wide: [] as string[], taps: [] as string[] };
      scrollTo(9999, scrollY);
      out.scrolledX = scrollX;
      scrollTo(0, scrollY);
      for (const e of document.querySelectorAll("input, select, textarea, [contenteditable]")) {
        const f = parseFloat(getComputedStyle(e).fontSize);
        if (touch && f < 16) out.small.push(`${tag(e)} ${f}px`);
      }
      for (const e of document.querySelectorAll("body *")) {
        if (!vis(e)) continue;
        const r = e.getBoundingClientRect();
        if (r.width <= innerWidth + 1 || getComputedStyle(e).position === "fixed" || e.closest(".seq, .vis, svg, pre, .cmd, [data-scroll-x], .tabs, .frow")) continue;
        let clipped = false;
        for (let a = e.parentElement; a && a !== document.body; a = a.parentElement) if (/(hidden|clip|auto|scroll)/.test(getComputedStyle(a).overflowX)) clipped = true;
        if (!clipped) out.wide.push(`${tag(e)} ${Math.round(r.width)}`);
      }
      if (touch) {
        for (const e of document.querySelectorAll("a[href], button, summary, input, select, [role=button], [role=tab]")) {
          // A link in running text, or in a command or request to copy, is a word in a line: exempt.
          if (!vis(e) || e.closest("p, li, figcaption, .foot, footer, code")) continue;
          const r = e.getBoundingClientRect();
          if (r.height < 43.5 || r.width < 43.5) out.taps.push(`${tag(e)} ${Math.round(r.width)}x${Math.round(r.height)}`);
        }
      }
      return out;
    }, v.touch);
    expect(r.scale, "page zoom").toBe(1);
    expect(r.sideways, "the page is wider than the window").toBeLessThanOrEqual(0);
    expect(r.scrolledX, "the page scrolls sideways").toBe(0);
    expect(r.small, "controls with text under 16 px").toEqual([]);
    expect([...new Set(r.wide)], "wider than the window").toEqual([]);
    expect([...new Set(r.taps)], "tap targets under 44 px").toEqual([]);
    expect(errs, "page errors").toEqual([]);
    await ctx.close();
  });
}
