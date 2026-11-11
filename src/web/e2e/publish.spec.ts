// A published page: `cadpole publish examples/cabin_site.star
// --model-unit ft` wrote a directory (playwright.config.ts does it when the
// suite starts), and a static server serves it. The page must open the bake
// with no engine, no /api and no Starlark, show what a viewer shows, and the
// directory must hold nothing of the script.

import { expect, test, type Page } from "@playwright/test";
import { readdirSync, readFileSync } from "node:fs";
import { join } from "node:path";
import { inflateSync } from "node:zlib";

import { PUBLISH_FLAGS_DIR, PUBLISH_OPENING_DIR, SHADING_DIR, publishDir } from "./playwright.config.js";

/** The published bake's file name, as index.html names it (the extension is
 * build_support/file_types.bzl's, so the suite never spells it out). */
function bakeFile(): string {
  const html = readFileSync(join(publishDir, "index.html"), "utf8");
  const m = /<meta name="cadpole-baked" content="\.\/([^"]+)">/.exec(html);
  if (!m) throw new Error("index.html names no bake");
  return m[1];
}

/** Open the page and wait for its one evaluation; collect what went wrong. */
async function open(page: Page, path = "/"): Promise<{ errors: string[]; requests: string[] }> {
  const errors: string[] = [];
  const requests: string[] = [];
  page.on("pageerror", (err) => errors.push(err.message));
  page.on("console", (m) => {
    if (m.type() === "error") errors.push(m.text());
  });
  page.on("request", (r) => requests.push(new URL(r.url()).pathname));
  await page.goto(path);
  await expect(page.locator("html")).toHaveClass(/ready/);
  await expect(page.locator("html")).toHaveAttribute("data-evals", "1");
  return { errors, requests };
}

test.describe("a published page", () => {
  test("opens the bake with no engine, no server and no errors", async ({ page }) => {
    const { errors, requests } = await open(page);
    await expect(page.locator("html")).toHaveAttribute("data-mode", "published");
    await expect(page.locator("html")).toHaveAttribute("data-engine", "baked");
    await expect(page).toHaveTitle("cabin_site · Cadpole");
    await expect(page.locator(".doc-name")).toHaveText("cabin_site");
    await expect(page.locator(".pill-baked")).toHaveText("published");
    // The bodies of both files: the cabin from this script's calls into
    // cabin_parts.star, the lot from its own.
    await expect(page.getByText("cabin_south", { exact: true }).first()).toBeVisible();
    await expect(page.getByText("lot", { exact: true }).first()).toBeVisible();
    // Four files and nothing else: no wasm, no worker, no service worker, no
    // probe of a server that is not there.
    const fetched = requests.filter((p) => p !== "/favicon.ico");
    expect(fetched.some((p) => p.startsWith("/api"))).toBe(false);
    expect(fetched.some((p) => /cadpole_wasm|engine-worker|sw\.js|worker-report/.test(p))).toBe(false);
    expect(fetched.filter((p) => p === "/" + bakeFile())).toHaveLength(1);
    // Nothing that edits or evaluates is offered.
    await expect(page.locator("#tab-examples")).toBeHidden();
    await expect(page.locator("#tab-script")).toBeHidden();
    await expect(page.locator(".right-bottom")).toBeHidden();
    // No parameter is published by default: no panel, no slider.
    await expect(page.locator("html")).toHaveAttribute("data-params", "0");
    await expect(page.locator(".params-panel")).toBeHidden();
    await expect(page.locator(".param-range")).toHaveCount(0);
    // The unit every readout is in, said once.
    await expect(page.locator(".status-item.units")).toHaveText("lengths in feet and inches");
    expect(errors).toEqual([]);
  });

  test("keeps a hidden layer hidden, and shows it without evaluating", async ({ page }) => {
    await open(page);
    const row = page.locator(".layers > *").filter({ hasText: "existing" });
    const toggle = row.locator("input[type=checkbox]");
    await expect(toggle).not.toBeChecked();
    await toggle.check({ force: true });
    await expect(toggle).toBeChecked();
    // A layer is a redraw, never an evaluation: still the one.
    await expect(page.locator("html")).toHaveAttribute("data-evals", "1");
  });

  test("reads in feet and inches: the model was drawn in feet", async ({ page }) => {
    await open(page);
    // The body row (the one with an eye), not the feature row above it.
    const eye = page.getByRole("button", { name: "Hide cabin_floor", exact: true });
    await eye.locator("xpath=..").locator(".tree-label").click();
    await expect(page.locator(".panel-right")).toContainText("Size");
    const inspector = page.locator(".panel-right");
    // 24 ft wide: written as feet and inches, never as 7315.2 mm.
    await expect(inspector).toContainText(`24'`);
    await expect(inspector).not.toContainText("7315");
  });

  test("the directory holds no Starlark: no source, no comment, no print", () => {
    // The other sites live in subdirectories; this is the first's.
    const files = readdirSync(publishDir).filter((f) => ![PUBLISH_FLAGS_DIR, PUBLISH_OPENING_DIR, SHADING_DIR].includes(f));
    expect(files.filter((f) => f.endsWith(".star"))).toEqual([]);
    expect(files.sort()).toEqual(
      expect.arrayContaining(["index.html", "cadpole-publish.json"]),
    );
    expect(files).toHaveLength(5);
    for (const f of files) {
      let bytes = readFileSync(join(publishDir, f));
      if (f === bakeFile()) {
        bytes = inflateSync(bytes);
        // Cadpole's own bundle carries its example gallery, `load(`s and all;
        // the bake carries no script whatever.
        expect(bytes.includes("load(")).toBe(false);
        expect(bytes.includes("def ")).toBe(false);
      }
      // A comment line in cabin_site.star and the text it prints.
      expect(bytes.includes("PUBLISH-PRIVACY-MARKER"), f).toBe(false);
      expect(bytes.includes("the budget is a secret"), f).toBe(false);
      expect(bytes.includes("cabin_parts.star"), f).toBe(false);
    }
  });

  test("frames the drawn bodies only: a hidden layer never moves Home or Fit", async ({ page }) => {
    await open(page);
    // Switch the lot off: what is drawn is the cabin alone, its roof's
    // overhang 1.5 ft west of x = 0. The shed the hidden layer holds stands
    // at x = -14 ft and must not stretch the box to it.
    const row = page.locator(".layers > *").filter({ hasText: "site" });
    await row.locator("input[type=checkbox]").uncheck({ force: true });
    await expect(page.locator("html")).toHaveAttribute("data-scene-box", /^-457,/);
    // Showing the shed's layer does take the box there.
    await page.locator(".layers > *").filter({ hasText: "existing" }).locator("input[type=checkbox]").check({ force: true });
    await expect(page.locator("html")).toHaveAttribute("data-scene-box", /^-4267,/);
    await expect(page.locator("html")).toHaveAttribute("data-evals", "1");
  });

  test("sections client-side: typed in feet, flipped, vertical, cleared, never evaluated", async ({ page }) => {
    const { errors } = await open(page, "/index.html?section=z=1219.2");
    const html = page.locator("html");
    // The address's cut, with caps: the walls cut at 4 ft close into loops.
    await expect(html).toHaveAttribute("data-section", "z=1219.2");
    await expect.poll(async () => Number(await html.getAttribute("data-section-tris"))).toBeGreaterThan(0);
    const panel = page.locator(".section-panel");
    await expect(panel).toBeVisible();
    await expect(panel.locator(".section-horizontal-value")).toHaveValue(`4'-0"`);
    // A height typed the way the readouts write it.
    const field = panel.locator(".section-horizontal-value");
    await field.fill(`2'-6"`);
    await field.press("Enter");
    await expect(html).toHaveAttribute("data-section", "z=762");
    await panel.locator(".section-horizontal-flip").click();
    await expect(html).toHaveAttribute("data-section", "-z=762");
    // A vertical cut along Y beside it, through the middle of what is drawn.
    await panel.locator(".section-axis").selectOption("y");
    await panel.locator(".section-vertical-on").check();
    await expect(html).toHaveAttribute("data-section", /^-z=762,y=/);
    await expect(panel.locator(".section-spec")).toHaveText(/^-z=762,y=/);
    // The address follows, so the link opens on the same cut.
    expect(new URL(page.url()).searchParams.get("section")).toMatch(/^-z=762,y=/);
    await panel.locator(".section-clear").click();
    await expect(html).toHaveAttribute("data-section", "");
    await expect(html).toHaveAttribute("data-section-tris", "0");
    // The close button hides the panel; C brings it back.
    await panel.locator(".section-close").click();
    await expect(panel).toBeHidden();
    await page.locator("canvas").first().click({ position: { x: 5, y: 5 } });
    await page.keyboard.press("c");
    await expect(panel).toBeVisible();
    // All of it a redraw: still the one evaluation.
    await expect(html).toHaveAttribute("data-evals", "1");
    expect(errors).toEqual([]);
  });

  test("a bake that is not there is a page that says so", async ({ page }) => {
    await page.goto("/index.html?baked=missing-model");
    await expect(page.locator(".fatal")).toContainText("Cadpole could not start");
    await expect(page.locator(".fatal")).toContainText("HTTP 404");
  });
});

// The publisher's choices: `cadpole publish ... --view top
// --frame-layer cabin --grid off --hide-layer site --show-params width,depth
// --section z=0.25` (playwright.config.ts's PUBLISH_FLAGS).
test.describe("a page published with its opening choices", () => {
  const flags = `/${PUBLISH_FLAGS_DIR}/index.html`;

  test("lists the chosen parameters read-only, hides the chosen layer and the grid", async ({ page }) => {
    const { errors } = await open(page, flags);
    const html = page.locator("html");
    await expect(html).toHaveAttribute("data-params", "2");
    const readout = page.locator(".param-readout");
    await expect(readout.locator("dt")).toHaveText(["width", "depth"]);
    await expect(readout.locator("dd")).toHaveText(["24", "16"]);
    // A read-out, nothing to drag or type into.
    await expect(page.locator(".params-panel input")).toHaveCount(0);
    // The lot's layer starts hidden; the script's own hidden layer stays hidden.
    await expect(page.locator(".layers > *").filter({ hasText: "site" }).locator("input[type=checkbox]")).not.toBeChecked();
    await expect(page.locator(".layers > *").filter({ hasText: "existing" }).locator("input[type=checkbox]")).not.toBeChecked();
    // The grid is off, and the View menu says so (and can turn it back on).
    await page.locator("details.menu summary").click();
    await expect(page.getByRole("menuitemcheckbox", { name: "Grid" })).toHaveAttribute("aria-checked", "false");
    expect(errors).toEqual([]);
  });

  test("frames the cabin layer, opens on the section, and a cut face selects its body", async ({ page }) => {
    await open(page, flags);
    const html = page.locator("html");
    // The framing box is the cabin's (its roof overhangs 1.5 ft), whatever else is drawn.
    await expect(html).toHaveAttribute("data-frame-box", /^-457,-457,/);
    await expect(html).toHaveAttribute("data-section", "z=76.2");
    await expect.poll(async () => Number(await html.getAttribute("data-section-tris"))).toBeGreaterThan(0);
    // Top view, centred on the cabin, cut through its floor slab: the middle
    // of the viewport is the slab's cut face, and clicking it selects the slab.
    const canvas = page.locator("canvas").first();
    const box = await canvas.boundingBox();
    if (!box) throw new Error("no canvas");
    await page.waitForTimeout(600);
    await canvas.click({ position: { x: box.width / 2, y: box.height / 2 } });
    await expect(page.locator(".panel-right")).toContainText("cabin_floor");
    await expect(html).toHaveAttribute("data-evals", "1");
  });
});

// How a link asks a page to open (src/web/src/opening.ts): `?section=`,
// `?mode=`, `?view=` and `?eye=`/`&target=` in the address, over what the
// publisher chose (`--section`, `--mode`, `--view`).
test.describe("a published page opened by a link to a view", () => {
  /** The camera the page reports, as the link to this view writes it. */
  const link = async (page: Page): Promise<URL> => new URL(await page.evaluate(() => (window as unknown as { __cadpole: { viewLink(): string } }).__cadpole.viewLink()));

  test("opens cut, in x-ray, from the camera in the address, and the toolbar says so", async ({ page }) => {
    const { errors } = await open(page, "/index.html?section=-y=2000&mode=xray&eye=9000,-12000,7000&target=3600,2400,1200");
    const html = page.locator("html");
    await expect(html).toHaveAttribute("data-section", "-y=2000");
    await expect.poll(async () => Number(await html.getAttribute("data-section-tris"))).toBeGreaterThan(0);
    await expect(html).toHaveAttribute("data-view-mode", "xray");
    await expect(page.getByLabel("Display mode")).toHaveValue("xray");
    // The Section button is down and marked as cutting; its panel is open.
    const sectionBtn = page.locator(".section-btn");
    await expect(sectionBtn).toHaveAttribute("aria-pressed", "true");
    await expect(sectionBtn).toHaveClass(/cutting/);
    await expect(page.locator(".section-panel")).toBeVisible();
    // The camera is the address's, to the millimetre the link is written in.
    await expect.poll(async () => (await link(page)).searchParams.get("eye")).toBe("9000,-12000,7000");
    expect((await link(page)).searchParams.get("target")).toBe("3600,2400,1200");
    // Turning the cut off: the picture, the address and the button all follow.
    await page.locator(".section-clear").click();
    await expect(html).toHaveAttribute("data-section", "");
    await expect(sectionBtn).not.toHaveClass(/cutting/);
    expect(new URL(page.url()).searchParams.get("section")).toBeNull();
    await sectionBtn.click();
    await expect(sectionBtn).toHaveAttribute("aria-pressed", "false");
    await expect(page.locator(".section-panel")).toBeHidden();
    await expect(html).toHaveAttribute("data-evals", "1");
    expect(errors).toEqual([]);
  });

  test("the link to this view opens the same view again", async ({ page }) => {
    await open(page, "/index.html?view=front");
    // Arrange a view by hand: a cut, a mode, and wherever the front view put the camera.
    await page.keyboard.press("c");
    await page.getByLabel("Display mode").selectOption("shaded");
    const html = page.locator("html");
    await expect.poll(async () => await html.getAttribute("data-section")).toMatch(/z=/);
    await expect.poll(async () => {
      const a = (await link(page)).search;
      await page.waitForTimeout(400);
      return (await link(page)).search === a;
    }).toBe(true);
    const first = await link(page);
    expect(first.searchParams.get("mode")).toBe("shaded");
    expect(first.searchParams.get("section")).toMatch(/z=/);
    expect(first.searchParams.get("eye")).not.toBeNull();
    // A new tab would have no saved camera; this one forgets its own.
    await page.evaluate(() => sessionStorage.clear());
    await open(page, first.pathname + first.search);
    await expect(html).toHaveAttribute("data-section", first.searchParams.get("section") ?? "");
    await expect(html).toHaveAttribute("data-view-mode", "shaded");
    await expect.poll(async () => (await link(page)).search).toBe(first.search);
  });

  test("a value that does not parse is ignored, not an error", async ({ page }) => {
    const { errors } = await open(page, "/index.html?mode=sparkly&eye=1,2&view=sideways");
    await expect(page.locator("html")).toHaveAttribute("data-view-mode", "shaded_edges");
    expect(errors).toEqual([]);
  });

  test("the publisher's mode and home link; the address's mode over the publisher's", async ({ page }) => {
    const opening = `/${PUBLISH_OPENING_DIR}/index.html`;
    const { errors } = await open(page, opening);
    await expect(page.locator("html")).toHaveAttribute("data-view-mode", "xray");
    const home = page.locator(".home-link");
    await expect(home).toBeVisible();
    await expect(home).toHaveAttribute("aria-label", "Models");
    expect(new URL(await home.evaluate((a: HTMLAnchorElement) => a.href)).pathname).toBe("/");
    // The page without the flag has no such link.
    await page.evaluate(() => sessionStorage.clear());
    await open(page, `${opening}?mode=shaded`);
    await expect(page.locator("html")).toHaveAttribute("data-view-mode", "shaded");
    await open(page, "/index.html");
    await expect(page.locator(".home-link")).toHaveCount(0);
    expect(errors).toEqual([]);
  });
});

// A published page on a phone: what someone sent a link sees. The bar is the
// model's name and one button, the tabs are the ones a viewer has a use for,
// and nothing is wider than the screen.
test.describe("a published page on a phone", () => {
  test.use({ viewport: { width: 390, height: 844 }, hasTouch: true, isMobile: true });

  test("shows a viewer's tabs only, and nothing runs off the side", async ({ page }) => {
    const { errors } = await open(page);
    const tabbar = page.locator(".tabbar");
    await expect(tabbar).toBeVisible();
    // With nothing to report there is no Issues tab, and there is never a
    // Script or a Chat one: a published page evaluates nothing.
    await expect(tabbar.getByRole("button", { name: "Issues" })).toBeHidden();
    await expect(tabbar.getByRole("button", { name: "Script" })).toBeHidden();
    await expect(tabbar.getByRole("button", { name: "Chat" })).toBeHidden();
    await expect(tabbar.getByRole("button", { name: "Model", exact: true })).toBeVisible();
    await expect(tabbar.getByRole("button", { name: "View", exact: true })).toBeVisible();
    await expect(page.locator(".toolbar button:visible")).toHaveCount(1);
    for (const width of [390, 360]) {
      await page.setViewportSize({ width, height: 800 });
      const w = await page.evaluate(() => [document.documentElement.scrollWidth, innerWidth]);
      expect(w[0], `nothing wider than a ${width} px screen`).toBeLessThanOrEqual(w[1]);
      for (const b of await tabbar.locator("button:visible").all()) {
        const box = await b.boundingBox();
        expect(box?.width ?? 0, "a tab is a 44 px target").toBeGreaterThanOrEqual(44);
        expect((box?.x ?? 0) + (box?.width ?? 0)).toBeLessThanOrEqual(width + 0.5);
      }
    }
    expect(errors).toEqual([]);
  });

  test("a link to a cut opens on the cut, with the panel a tap away and off the model", async ({ page }) => {
    const { errors } = await open(page, "/index.html?section=-y=2000");
    const html = page.locator("html");
    await expect(html).toHaveAttribute("data-section", "-y=2000");
    // No panel over the lower half of the model; a button by the view buttons instead.
    const panel = page.locator(".section-panel");
    await expect(panel).toBeHidden();
    const fab = page.locator(".fab-section");
    await expect(fab).toBeVisible();
    await expect(fab).toHaveAttribute("aria-pressed", "false");
    await fab.tap();
    await expect(panel).toBeVisible();
    await expect(fab).toHaveAttribute("aria-pressed", "true");
    await panel.locator(".section-clear").tap();
    await expect(html).toHaveAttribute("data-section", "");
    await panel.locator(".section-close").tap();
    // Nothing is cut and the panel is shut: the button has nothing to say.
    await expect(fab).toBeHidden();
    expect(errors).toEqual([]);
  });
});
