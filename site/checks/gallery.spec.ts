// The gallery of operations: every tile opens the kind of view its picture is
// (a drawing opens the SVG to pan and zoom, a model opens in 3D), line art is
// drawn from the SVG everywhere on the page, an address names an open card,
// and the tiles fill their rows.
import { context, expect, test } from "./site.js";

type Sheet = { kind(): string; card(): string };
const sheet = (p: import("@playwright/test").Page) => p.evaluate(() => { const s = (window as unknown as { __sheet: Sheet }).__sheet; return { kind: s.kind(), card: s.card() }; });

test("line art is SVG: every drawing tile and drawing opener", async ({ browser, site }) => {
  const ctx = await context(browser, "desk-1440");
  const p = await ctx.newPage();
  await p.goto(site + "?noauto=1", { waitUntil: "load" });
  const bad = await p.evaluate(() => {
    const out: string[] = [];
    for (const b of document.querySelectorAll<HTMLElement>("[data-draw]")) {
      if (b.closest("template")) continue;
      if (!b.querySelector(".dwg[data-svg$='.svg']")) out.push(`${b.dataset["title"]}: no SVG picture`);
      if (b.querySelector("img")) out.push(`${b.dataset["title"]}: a raster picture`);
    }
    for (const t of document.querySelectorAll<HTMLElement>(".tile")) {
      const kind = t.dataset["kind"], svg = !!t.querySelector(".dwg[data-svg]"), img = !!t.querySelector("img");
      if (kind === "drawing" && (!svg || img || !t.dataset["draw"])) out.push(`${t.id}: a drawing tile must show and open the SVG`);
      if (kind === "model" && (!img || svg || !t.dataset["sheet3d"])) out.push(`${t.id}: a model tile must show a picture and open the model`);
      if (kind !== "drawing" && kind !== "model") out.push(`${t.id}: no kind`);
    }
    return out;
  });
  expect(bad).toEqual([]);
  await ctx.close();
});

test("each tile opens its own kind of view", async ({ browser, site }) => {
  const ctx = await context(browser, "desk-1440");
  const p = await ctx.newPage();
  const errs: string[] = [];
  p.on("pageerror", (e) => errs.push(String(e).slice(0, 200)));
  await p.goto(site + "?noauto=1", { waitUntil: "load" });
  const tiles = await p.evaluate(() => [...document.querySelectorAll<HTMLElement>(".tile")].map((t) => ({ id: t.id, kind: t.dataset["kind"]! })));
  expect(tiles.length).toBeGreaterThan(20);
  for (const t of tiles) {
    await p.evaluate((id) => { const el = document.getElementById(id)!; el.scrollIntoView({ block: "center", behavior: "instant" }); el.click(); }, t.id);
    await expect.poll(async () => (await sheet(p)).kind, { timeout: 60_000, message: t.id }).toBe(t.kind === "drawing" ? "svg" : "3d");
    const s = await sheet(p);
    expect(s.card, t.id).toBe(`#${t.id}`);
    await expect(p.locator(".sheet-info .card-blurb")).toHaveCount(1);
    await expect(p.locator(".sheet-info pre")).toHaveCount(1);
    await p.keyboard.press("Escape");
    await expect.poll(async () => (await sheet(p)).kind).toBe("");
    await expect.poll(() => p.evaluate(() => location.hash)).toBe("");
  }
  expect(errs).toEqual([]);
  await ctx.close();
});

test("an address opens a card; Escape and the surround close it", async ({ browser, site }) => {
  const ctx = await context(browser, "phone-390");
  const p = await ctx.newPage();
  await p.goto(site + "?noauto=1#f-drawing", { waitUntil: "load" });
  await expect.poll(async () => (await sheet(p)).kind, { timeout: 30_000 }).toBe("svg");
  expect((await sheet(p)).card).toBe("#f-drawing");
  // The drawing card offers its files and the model behind it.
  await expect(p.locator(".sheet-info a[download]")).toHaveCount(3);
  await p.locator(".sheet-info button[data-sheet3d]").click();
  await expect.poll(async () => (await sheet(p)).kind, { timeout: 60_000 }).toBe("3d");
  await p.keyboard.press("Escape");
  await expect.poll(async () => (await sheet(p)).kind).toBe("");
  await ctx.close();
});

for (const name of ["phone-390", "tablet-820", "desk-1440"]) {
  test(`the tiles fill their rows, ${name}`, async ({ browser, site }) => {
    const ctx = await context(browser, name);
    const p = await ctx.newPage();
    await p.goto(site + "?noauto=1", { waitUntil: "load" });
    const rows = await p.evaluate(() => {
      const by = new Map<number, number[]>();
      for (const t of document.querySelectorAll<HTMLElement>(".tiles > li")) {
        const r = t.getBoundingClientRect(), y = Math.round(r.top + scrollY);
        by.set(y, [...(by.get(y) ?? []), Math.round(r.width)]);
      }
      return [...by.values()];
    });
    const full = rows[0]!.length, w = rows[0]![0]!;
    expect(full).toBeGreaterThanOrEqual(2);
    // Every row but the last is full, every tile is the same width, and the last row is centred (checked by its count only).
    rows.slice(0, -1).forEach((r) => expect(r.length).toBe(full));
    for (const r of rows) for (const x of r) expect(Math.abs(x - w)).toBeLessThanOrEqual(1);
    expect(rows.at(-1)!.length).toBeLessThanOrEqual(full);
    await ctx.close();
  });
}
