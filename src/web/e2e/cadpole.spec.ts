// End-to-end screenshot tests, mock engine (?mock=1), service worker off.

import { expect, test, type Locator, type Page } from "@playwright/test";

const URL = "/?mock=1&nosw=1";

/**
 * A document that reads in feet and inches, for the length readouts:
 * `width` is declared in millimetres and `span` in feet, and both read in the
 * document's convention beside the value they were declared in. Applied in
 * static mode (`&static=1`), where the page edits its own script and nothing
 * is left in the dev server's session for the tests after it.
 */
const FT_IN_SCRIPT = `units(units.ft_in)

width = param("width", 60, min=20, max=200, step=1, unit="mm", doc="Plate width (X)")
span = param("span", 12.5, min=4, max=40, step=0.5, unit="ft", doc="Deck span")

base = extrude(sketch(XY).rect(width, 40), 20, name="base")
`;

/** Replace the static document's script from the Script panel, and wait for the evaluation. */
async function applyScript(page: Page, panel: Page | Locator, source: string): Promise<void> {
  const script = panel.getByRole("region", { name: "Script" });
  await script.getByRole("button", { name: "Edit" }).click();
  const before = Number(await page.locator("html").getAttribute("data-evals"));
  await script.getByRole("textbox", { name: "Script editor" }).fill(source);
  await script.getByRole("button", { name: "Apply" }).click();
  await expect.poll(async () => Number(await page.locator("html").getAttribute("data-evals"))).toBeGreaterThan(before);
}

async function open(page: Page, url = URL): Promise<void> {
  await page.goto(url);
  await expect(page.locator("html")).toHaveClass(/ready/);
  // First evaluation done (main.ts stamps the count on <html>).
  await expect(page.locator("html")).toHaveAttribute("data-evals", /[1-9]/);
  // Let the first (fit) frame render.
  await page.waitForTimeout(300);
}

/**
 * Wait for the viewer to go idle: its frame counter (`data-frames`) has not
 * moved for a while. Rendering is on demand, so a moving counter is a change
 * flash still fading, and on a loaded box each of those frames is slow enough
 * that a screenshot taken meanwhile times out before two of them agree.
 */
async function settled(page: Page, quietMs = 500): Promise<void> {
  const frames = (): Promise<number> => page.evaluate(() => Number(document.documentElement.dataset["frames"] ?? 0));
  let last = await frames();
  const deadline = Date.now() + 30_000;
  while (Date.now() < deadline) {
    await page.waitForTimeout(quietMs);
    const now = await frames();
    if (now === last) return;
    last = now;
  }
}

test.describe("desktop", () => {
  test.skip(({ isMobile }) => Boolean(isMobile), "desktop layout only");

  test("sections the model in the viewer without evaluating", async ({ page }) => {
    await open(page);
    const html = page.locator("html");
    const evals = await html.getAttribute("data-evals");
    // The toolbar's Section button opens the panel with a cut at half height.
    await page.getByRole("button", { name: "Section" }).click();
    const panel = page.locator(".section-panel");
    await expect(panel).toBeVisible();
    await expect(html).toHaveAttribute("data-section", /^z=/);
    await expect.poll(async () => Number(await html.getAttribute("data-section-tris"))).toBeGreaterThan(0);
    // A typed height in the document's unit (millimetres here).
    const field = panel.locator(".section-horizontal-value");
    await field.fill("10");
    await field.press("Enter");
    await expect(html).toHaveAttribute("data-section", "z=10");
    // The slider moves it too.
    await panel.locator(".section-horizontal-range").fill("900");
    await expect(html).not.toHaveAttribute("data-section", "z=10");
    // A vertical cut along X, flipped: `cadpole render --section`'s -x=.
    await panel.locator(".section-vertical-on").check();
    await panel.locator(".section-vertical-flip").click();
    await expect(html).toHaveAttribute("data-section", /,-x=/);
    await settled(page);
    await expect(html).toHaveAttribute("data-evals", evals ?? "");
    await panel.locator(".section-clear").click();
    await expect(html).toHaveAttribute("data-section", "");
  });

  test("renders the mock model with tree, params and viewport", async ({ page }) => {
    await open(page);
    const tree = page.getByRole("tree", { name: "Feature tree" });
    await expect(tree.getByRole("treeitem", { name: /^base extrude/ })).toBeVisible();
    await expect(tree.getByRole("treeitem", { name: /^boss 3 faces/ })).toBeVisible();
    await expect(page.getByRole("region", { name: "Parameters" }).getByLabel("width (mm)")).toHaveValue("60");
    await expect(page.locator(".pill-mock")).toHaveText("mock");
    await expect(page.locator(".status-item.eval")).toContainText(/mock engine [\d.]+ ms/);
    await expect(page).toHaveScreenshot("home.png", { fullPage: true });
  });

  test("clicking geometry selects a face and shows it in the inspector", async ({ page }) => {
    await open(page);
    const canvas = page.locator("canvas.viewport-canvas");
    const box = await canvas.boundingBox();
    if (!box) throw new Error("no canvas");
    // The centre of the home view is on the model.
    await page.mouse.click(box.x + box.width / 2, box.y + box.height / 2);
    const inspector = page.getByRole("region", { name: "Inspector" });
    await expect(inspector.locator(".chip")).toHaveCount(1);
    await expect(inspector.locator(".chip-kind")).toHaveText(/face|edge/);
    // The picker popover lists the alternatives under the cursor.
    await expect(page.getByRole("menu", { name: "Select entity" })).toBeVisible();
    await expect(page.locator(".vlabel")).toHaveCount(1);
    await expect(page).toHaveScreenshot("face-selected.png", { fullPage: true });
    // Escape closes the picker, a second Escape clears the selection.
    await page.keyboard.press("Escape");
    await page.keyboard.press("Escape");
    await expect(inspector).toContainText("Nothing selected");
  });

  test("tree selection, shift-click and keyboard navigation", async ({ page }) => {
    await open(page);
    const tree = page.getByRole("tree", { name: "Feature tree" });
    const base = tree.getByRole("treeitem", { name: /^base extrude/ });
    await base.click();
    const inspector = page.getByRole("region", { name: "Inspector" });
    await expect(inspector).toContainText("extrude");
    await expect(base).toHaveAttribute("aria-selected", "true");
    // Shift-click adds the boss body.
    await tree.getByRole("treeitem", { name: /^boss 3 faces/ }).click({ modifiers: ["Shift"] });
    await expect(inspector.locator(".chip")).toHaveCount(2);
    // Arrow keys move focus between rows.
    await base.focus();
    await page.keyboard.press("ArrowDown");
    await expect(page.locator(":focus")).toHaveAttribute("role", "treeitem");
    await expect(page.locator(":focus")).toHaveAttribute("data-key", "body|extrude_1|body_1|");
  });

  test("a body can be hidden and shown from the keyboard", async ({ page }) => {
    await open(page);
    const tree = page.getByRole("tree", { name: "Feature tree" });
    const boss = tree.getByRole("treeitem", { name: /^boss 3 faces/ });
    // The eye button is deliberately not a tab stop (one roving stop per
    // tree), so the row itself has to answer for it: Delete hides, Delete
    // again shows, and focus stays on the row across the re-render.
    await boss.focus();
    await page.keyboard.press("Delete");
    await expect(boss).toHaveClass(/is-hidden/);
    await expect(boss.getByRole("button", { name: "Show boss" })).toBeAttached();
    await expect(page.locator(":focus")).toHaveAttribute("data-key", "body|extrude_2|body_2|");
    await page.keyboard.press("Backspace");
    await expect(boss).not.toHaveClass(/is-hidden/);
    await expect(boss.getByRole("button", { name: "Hide boss" })).toBeAttached();
  });

  test("the plates view lays the model out on its print plates and leaves the model view untouched", async ({ page }) => {
    await open(page);
    // The default script exports nothing to Bambu Studio, so there is no
    // plates view to offer: the toggle is not there rather than disabled.
    const viewGroup = page.getByRole("group", { name: "View", exact: true });
    await expect(viewGroup).toBeHidden();
    await page.getByRole("tab", { name: "Examples" }).click();
    await page.getByRole("button", { name: "Load example Print plates" }).click();
    const platesBtn = page.getByRole("button", { name: "Plates", exact: true });
    await expect(platesBtn).toBeVisible();
    await expect(platesBtn).toHaveAttribute("aria-pressed", "false");
    // Let the change flash of the new model fade, and keep the pointer off
    // the canvas, so the model view can be compared byte for byte after the
    // round trip: a hover highlight is a difference too.
    const canvas = page.locator("canvas.viewport-canvas");
    await page.mouse.move(40, 400);
    await page.waitForTimeout(1600);
    const before = await canvas.screenshot();

    await platesBtn.click();
    await expect(page.locator("html")).toHaveAttribute("data-view", "plates");
    await expect(platesBtn).toHaveAttribute("aria-pressed", "true");
    // One tab per plate, after "All plates", named as the export names them.
    const tabs = page.getByRole("tablist", { name: "Plates" });
    await expect(tabs.getByRole("tab")).toHaveCount(3);
    await expect(tabs.getByRole("tab", { name: "All plates" })).toHaveAttribute("aria-selected", "true");
    await expect(tabs.getByRole("tab", { name: "1 · Boss" })).toBeVisible();
    await expect(tabs.getByRole("tab", { name: "2 · Base" })).toBeVisible();
    // The legend lists every slot the plates print in: the boss takes slot 1
    // by its colour, the base is pinned to slot 3.
    const legend = page.getByRole("list", { name: "Filaments" });
    await expect(legend.getByRole("listitem")).toHaveCount(2);
    await expect(legend).toContainText("Slot 1");
    await expect(legend).toContainText("Slot 3");
    // The beds are labelled in the viewport.
    await expect(page.locator(".platelabel")).toHaveCount(2);
    await page.waitForTimeout(300);
    await expect(page).toHaveScreenshot("plates.png", { fullPage: true });
    const all = await canvas.screenshot();

    // Framing one plate moves the camera: plate 1 holds the boss alone,
    // centred on its bed, so the middle of the view is then on the boss.
    await tabs.getByRole("tab", { name: "1 · Boss" }).click();
    await expect(tabs.getByRole("tab", { name: "1 · Boss" })).toHaveAttribute("aria-selected", "true");
    await page.waitForTimeout(300);
    const one = await canvas.screenshot();
    expect(Buffer.compare(all, one), "framing one plate moves the camera").not.toBe(0);
    // A placed body is still picked where it is drawn: the pointer over the
    // boss -- which is only under the middle of the view because it was
    // moved to its plate -- is the hand, and off it is not. Hover, not a
    // click: the selection is session state that outlives this test, and
    // the picture is compared against itself below.
    const box = await canvas.boundingBox();
    if (!box) throw new Error("no canvas");
    await page.mouse.move(box.x + box.width / 2, box.y + box.height / 2);
    await expect(canvas).toHaveCSS("cursor", "pointer");
    await page.mouse.move(box.x + 30, box.y + box.height - 30);
    await expect(canvas).not.toHaveCSS("cursor", "pointer");

    // Back to the model view: the toggle, the camera and the picture are
    // exactly what they were.
    await page.getByRole("button", { name: "Model", exact: true }).click();
    await expect(page.locator("html")).toHaveAttribute("data-view", "model");
    await expect(tabs).toBeHidden();
    await expect(page.locator(".platelabel")).toHaveCount(0);
    await page.mouse.move(40, 400);
    await page.waitForTimeout(300);
    const after = await canvas.screenshot();
    expect(Buffer.compare(before, after), "the model view is exactly what it was").toBe(0);

    // The dev server's one session carries the document into the next test
    // and the next project: put the default script back, so a later test
    // that names its "Model" button finds one, not this view toggle too.
    await page.getByRole("tab", { name: "Examples" }).click();
    await page.getByRole("button", { name: "Load example Plate with boss" }).click();
    await expect(viewGroup).toBeHidden();
  });

  test("Parameters precede the feature tree; few choices are segments, many a picker", async ({ page }) => {
    await open(page);
    // Six printers are too many for one line of segments: a native picker.
    await page.getByRole("tab", { name: "Examples" }).click();
    await page.getByRole("button", { name: "Load example Print plates" }).click();
    await page.getByRole("tab", { name: "Model" }).click();
    const panel = page.locator("#tabpanel-model");
    const paramsRegion = panel.getByRole("region", { name: "Parameters" });
    const printer = paramsRegion.getByRole("combobox", { name: "printer" });
    await expect(printer.locator("option")).toHaveCount(6);
    await printer.selectOption("prusa_mk4");
    await expect(paramsRegion.locator(".param-row.overridden")).toHaveCount(1);
    await paramsRegion.getByRole("button", { name: "Reset all" }).click();
    await expect(paramsRegion.locator(".param-row.overridden")).toHaveCount(0);
    // The plate: three finishes are segments, the boss is a switch, and on a
    // fine pointer a number is a slider beside a field, with no stepper.
    await page.getByRole("tab", { name: "Examples" }).click();
    await page.getByRole("button", { name: "Load example Plate with boss" }).click();
    await page.getByRole("tab", { name: "Model" }).click();
    const params = await paramsRegion.boundingBox();
    const tree = await panel.getByRole("region", { name: "Features" }).boundingBox();
    if (!params || !tree) throw new Error("the Model tab is missing a section");
    expect(params.y + params.height, "Parameters come before the tree").toBeLessThanOrEqual(tree.y + 1);
    await expect(paramsRegion.getByRole("radiogroup", { name: "finish" }).getByRole("radio")).toHaveCount(3);
    await expect(paramsRegion.getByRole("switch", { name: "with_boss" })).toBeChecked();
    await expect(paramsRegion.getByRole("button", { name: "Increase width" })).toHaveCount(0);
    await expect(paramsRegion.getByRole("slider", { name: "width slider" })).toBeVisible();
    await expect(paramsRegion.getByLabel("width (mm)")).toHaveValue("60");
  });

  test("a length reads in the document's feet and inches beside its unit", async ({ page }) => {
    await open(page, `${URL}&static=1`);
    await page.getByRole("tab", { name: "Script" }).click();
    await applyScript(page, page, FT_IN_SCRIPT);
    await page.getByRole("tab", { name: "Model" }).click();
    const params = page.getByRole("region", { name: "Parameters" });
    const width = params.locator(".param-row", { has: page.getByLabel("width (mm)") });
    await expect(width.locator(".param-readout")).toHaveText(`= 2 3/8"`);
    const field = params.getByLabel("span (ft)");
    const span = params.locator(".param-row", { has: page.getByLabel("span (ft)") });
    const readout = span.locator(".param-readout");
    await expect(readout).toHaveText(`= 12'-6"`);
    // On a fine pointer the readout is in the row's head, after the unit.
    const [name, unit, text] = await Promise.all([
      span.locator(".param-name").boundingBox(),
      span.locator(".param-unit:not(.param-readout)").boundingBox(),
      readout.boundingBox(),
    ]);
    if (!name || !unit || !text) throw new Error("the span row is missing a part");
    expect(text.x, "after the unit").toBeGreaterThanOrEqual(unit.x + unit.width);
    expect(Math.abs(text.y + text.height / 2 - (name.y + name.height / 2)), "on the name's line").toBeLessThan(4);
    await field.fill("20.25");
    await expect(readout).toHaveText(`= 20'-3"`);
  });

  test("a layer is switched off and ghosted in the viewer without an evaluation, and the state outlives one", async ({ page }) => {
    await open(page);
    // The default script declares no layers, so there is no panel to show.
    const layers = page.getByRole("region", { name: "Layers" });
    await expect(layers).toBeHidden();
    await page.getByRole("tab", { name: "Examples" }).click();
    await page.getByRole("button", { name: "Load example Layers demo" }).click();
    await page.getByRole("tab", { name: "Model" }).click();
    await expect(layers).toBeVisible();
    // Under the feature tree; Parameters are above.
    const [treeBox, layersBox] = await Promise.all([page.getByRole("region", { name: "Features" }).boundingBox(), layers.boundingBox()]);
    if (!treeBox || !layersBox) throw new Error("the Model tab is missing a section");
    expect(layersBox.y, "Layers come after the tree").toBeGreaterThanOrEqual(treeBox.y + treeBox.height - 1);
    const rows = layers.getByRole("listitem");
    await expect(rows).toHaveCount(2);
    await expect(rows.nth(0)).toContainText("part");
    await expect(rows.nth(1)).toContainText("envelope");
    // The script ghosts the envelope at 50 % by default; the tree says
    // which layer the boss is on.
    await expect(rows.nth(1).locator(".layer-pct")).toHaveText("50 %");
    const tree = page.getByRole("tree", { name: "Feature tree" });
    await expect(tree.getByRole("treeitem", { name: /^boss/ }).locator(".tree-layer")).toHaveText("envelope");
    // <html> counts the bodies drawn and the evaluations run.
    const html = page.locator("html");
    await expect(html).toHaveAttribute("data-bodies-drawn", "2");
    const evals = (await html.getAttribute("data-evals")) as string;
    const canvas = page.locator("canvas.viewport-canvas");
    await page.mouse.move(40, 400);
    await page.waitForTimeout(1600);
    const ghosted = await canvas.screenshot();

    // Off: one body drawn, no evaluation, the row marked as overridden.
    const envelope = layers.getByRole("switch", { name: "Show envelope" });
    await envelope.click();
    await expect(html).toHaveAttribute("data-bodies-drawn", "1");
    await expect(rows.nth(1)).toHaveClass(/overridden/);
    await page.waitForTimeout(400);
    await expect(page.locator("html")).toHaveAttribute("data-evals", evals);
    // On again, opaque: the picture is not the ghosted one, and still no
    // evaluation.
    await envelope.click();
    await expect(html).toHaveAttribute("data-bodies-drawn", "2");
    const slider = layers.getByRole("slider", { name: "envelope opacity" });
    await slider.evaluate((el) => {
      const input = el as HTMLInputElement;
      input.value = "100";
      input.dispatchEvent(new Event("input", { bubbles: true }));
    });
    await expect(rows.nth(1).locator(".layer-pct")).toHaveText("100 %");
    await page.waitForTimeout(300);
    const opaque = await canvas.screenshot();
    expect(Buffer.compare(ghosted, opaque), "the opacity slider changes the picture").not.toBe(0);
    await expect(page.locator("html")).toHaveAttribute("data-evals", evals);
    // Solo: the name shows that layer alone; again, every layer.
    await layers.getByRole("button", { name: "part", exact: true }).click();
    await expect(html).toHaveAttribute("data-bodies-drawn", "1");
    await expect(layers.getByRole("button", { name: "part", exact: true })).toHaveAttribute("aria-pressed", "true");
    await layers.getByRole("button", { name: "part", exact: true }).click();
    await expect(html).toHaveAttribute("data-bodies-drawn", "2");
    // Reset puts the script's own state back, byte for byte.
    await layers.getByRole("button", { name: "Reset to script" }).click();
    await expect(rows.nth(1)).not.toHaveClass(/overridden/);
    await expect(rows.nth(1).locator(".layer-pct")).toHaveText("50 %");
    await page.waitForTimeout(300);
    expect(Buffer.compare(ghosted, await canvas.screenshot()), "reset restores the script's picture").toBe(0);
    await expect(page.locator("html")).toHaveAttribute("data-evals", evals);

    // The state survives a re-evaluation of the document: switch the
    // envelope off, change a parameter, and it is still off afterwards.
    await envelope.click();
    await expect(html).toHaveAttribute("data-bodies-drawn", "1");
    const width = page.getByRole("region", { name: "Parameters" }).getByLabel("width (mm)");
    await width.fill("80");
    await width.blur();
    await expect(html).not.toHaveAttribute("data-evals", evals);
    await expect(envelope).not.toBeChecked();
    await expect(html).toHaveAttribute("data-bodies-drawn", "1");
    // ...and a reload of the same document, from this browser's storage.
    await page.reload();
    await expect(page.locator("html")).toHaveAttribute("data-evals", /[1-9]/);
    await page.getByRole("tab", { name: "Model" }).click();
    await expect(layers.getByRole("switch", { name: "Show envelope" })).not.toBeChecked();

    // Leave the session as it was found: the layers reset and the default
    // script back, for the tests after this one.
    await layers.getByRole("button", { name: "Reset to script" }).click();
    await page.getByRole("tab", { name: "Examples" }).click();
    await page.getByRole("button", { name: "Load example Plate with boss" }).click();
    await expect(layers).toBeHidden();
  });

  test("changing a parameter re-evaluates and the diagnostics demo reports errors", async ({ page }) => {
    await open(page);
    const width = page.getByRole("region", { name: "Parameters" }).getByLabel("width (mm)");
    await width.fill("120");
    await width.blur();
    await expect(page.locator(".param-row.overridden")).toHaveCount(1);
    await expect(page.locator(".diag-summary")).toContainText("No issues");
    await page.getByRole("tab", { name: "Examples" }).click();
    await page.getByRole("button", { name: "Load example Diagnostics demo" }).click();
    await expect(page.locator(".diag-summary")).toContainText(/2 errors/);
    await expect(page.locator(".diag-list")).toContainText("boss diameter");
    await settled(page);
    await expect(page).toHaveScreenshot("diagnostics.png", { fullPage: true });
    // Clicking a diagnostic reveals the script line.
    await page.locator(".diag-loc").first().click();
    await expect(page.getByRole("tab", { name: "Script" })).toHaveAttribute("aria-selected", "true");
    await expect(page.locator(".script .line.has-error").first()).toBeVisible();
  });

  test("messages are posted to the session and answered by the dev server", async ({ page }) => {
    await open(page);
    const input = page.getByRole("textbox", { name: "Message", exact: true });
    await input.fill("make it thicker");
    await input.press("Enter");
    const log = page.getByRole("log");
    await expect(log).toContainText("make it thicker");
    await expect(log).toContainText("(dev server) I received");
  });

  test("the Listen switch in the Chat panel turns the assistant's input on and off", async ({ page }) => {
    await open(page);
    const listen = page.getByRole("button", { name: "Listen" });
    await expect(listen).toHaveAttribute("aria-pressed", "false");
    await listen.click();
    await expect(listen).toHaveAttribute("aria-pressed", "true");
    // The server holds it: a page loaded now starts with it on.
    await open(page);
    await expect(page.getByRole("button", { name: "Listen" })).toHaveAttribute("aria-pressed", "true");
    await page.getByRole("button", { name: "Listen" }).click();
    await expect(page.getByRole("button", { name: "Listen" })).toHaveAttribute("aria-pressed", "false");
  });

  test("a message typed while the last one is being sent is sent after it", async ({ page }) => {
    await open(page);
    const input = page.getByRole("textbox", { name: "Message", exact: true });
    await input.fill("one [no reply]");
    await input.press("Enter");
    await input.fill("two [no reply]");
    await input.press("Enter");
    await expect(input).toHaveValue("");
    const log = page.getByRole("log");
    await expect(log.locator(".msg.from-user").filter({ hasText: "two [no reply]" })).toHaveCount(1);
    const texts = await log.locator(".msg.from-user .msg-text").allTextContents();
    expect(texts.indexOf("two [no reply]")).toBe(texts.indexOf("one [no reply]") + 1);
  });

  test("a reply written a piece at a time grows in place and settles", async ({ page }) => {
    await open(page);
    // The test is the assistant: each call is one StreamReply.
    const stream = async (body: Record<string, unknown>): Promise<number> => {
      const res = await page.request.post("/api/dev/stream-reply", { data: body });
      expect(res.ok(), await res.text()).toBe(true);
      return ((await res.json()) as { seq: number }).seq;
    };
    const log = page.getByRole("log");
    const seq = await stream({ activity: "thinking…" });
    const msg = log.locator(`.msg[data-seq="${seq}"]`);
    await expect(msg).toHaveCount(1);
    await expect(msg).toHaveClass(/from-assistant/);
    await expect(msg).toHaveClass(/streaming/);
    await expect(msg).toHaveAttribute("aria-busy", "true");
    await expect(msg.locator(".msg-activity")).toHaveText("thinking…");

    await stream({ seq, append: "Making the plate ", activity: "editing the script…" });
    await expect(msg.locator(".msg-text")).toHaveText("Making the plate");
    await expect(msg.locator(".msg-activity")).toHaveText("editing the script…");
    // The element on screen is the one that grows: a mark put on it now is
    // still there when the reply has settled.
    await msg.evaluate((el) => el.setAttribute("data-mark", "kept"));
    await stream({ seq, append: "2 mm thicker" });
    await expect(msg.locator(".msg-text")).toHaveText("Making the plate 2 mm thicker");
    await expect(msg.locator(".msg-activity")).toHaveText("");
    const growing = await msg.boundingBox();

    await stream({ seq, append: ".", finish: "done" });
    await expect(msg).not.toHaveClass(/streaming/);
    await expect(msg).not.toHaveAttribute("aria-busy", "true");
    await expect(msg.locator(".msg-text")).toHaveText("Making the plate 2 mm thicker.");
    await expect(msg).toHaveAttribute("data-mark", "kept");
    await expect(log.locator(`.msg[data-seq="${seq}"]`)).toHaveCount(1);
    // Settling moves nothing: the message is where it was and as tall.
    const settled = await msg.boundingBox();
    expect(settled?.height).toBe(growing?.height);
    expect(settled?.y).toBe(growing?.y);

    // A reply that ends early says so, once, and is not left looking busy.
    const second = await stream({ append: "Rendering the result", activity: "rendering…" });
    const failed = log.locator(`.msg[data-seq="${second}"]`);
    await expect(failed).toHaveClass(/streaming/);
    await stream({ seq: second, append: "\n\nThe listener could not finish this: it stopped.", finish: "failed" });
    await expect(failed).toHaveClass(/failed/);
    await expect(failed).not.toHaveClass(/streaming/);
    await expect(failed.locator(".msg-text")).toContainText("could not finish this");
    await expect(failed.locator(".msg-activity")).toHaveText("");
  });

  test("a growing reply keeps the list at its end only for a reader who is there", async ({ page }) => {
    await open(page);
    const stream = async (body: Record<string, unknown>): Promise<number> => {
      const res = await page.request.post("/api/dev/stream-reply", { data: body });
      return ((await res.json()) as { seq: number }).seq;
    };
    const log = page.getByRole("log");
    // Enough finished replies that the list scrolls.
    const before = await log.locator(".msg").count();
    for (let i = 0; i < 14; i++) await stream({ append: `Earlier reply ${i}\nwith a second line`, finish: "done" });
    await expect(log.locator(".msg")).toHaveCount(before + 14);
    const gap = (): Promise<number> => log.evaluate((el) => el.scrollHeight - el.scrollTop - el.clientHeight);
    expect(await log.evaluate((el) => el.scrollHeight > el.clientHeight)).toBe(true);
    await expect.poll(gap).toBeLessThanOrEqual(1);

    // At the end: the list follows the reply as it grows.
    const seq = await stream({ activity: "thinking…" });
    for (let i = 0; i < 6; i++) await stream({ seq, append: `Line ${i} of a reply that keeps growing\n` });
    const growing = log.locator(`.msg[data-seq="${seq}"]`);
    await expect(growing.locator(".msg-text")).toContainText("Line 5");
    await expect.poll(gap).toBeLessThanOrEqual(1);

    // Scrolled up to read something: the reply grows and settles below
    // without moving what is being read.
    await log.evaluate((el) => (el.scrollTop = 0));
    for (let i = 6; i < 12; i++) await stream({ seq, append: `Line ${i} of a reply that keeps growing\n` });
    await expect(growing.locator(".msg-text")).toContainText("Line 11");
    expect(await log.evaluate((el) => el.scrollTop)).toBe(0);
    await stream({ seq, append: "Done.", finish: "done" });
    await expect(growing).not.toHaveClass(/streaming/);
    expect(await log.evaluate((el) => el.scrollTop)).toBe(0);
  });

  test("snapshot queues a screenshot attachment for the next message", async ({ page }) => {
    await open(page);
    await page.getByRole("button", { name: /^Snapshot/ }).click();
    await expect(page.locator(".attach-thumb")).toHaveCount(1);
    const input = page.getByRole("textbox", { name: "Message", exact: true });
    await input.fill("here is the view");
    await input.press("Enter");
    await expect(page.locator(".attach-thumb")).toHaveCount(0);
    await expect(page.getByRole("log").locator(".msg-img")).toHaveCount(1);
  });

  test("display modes, projection and overlays are keyboard toggles", async ({ page }) => {
    await open(page);
    await page.keyboard.press("5");
    // The "View" menu mirrors the state; open it to check (a closed <details> is not in the a11y tree).
    await page.locator(".toolbar .menu > summary").click();
    await expect(page.getByRole("menuitemcheckbox", { name: "Orthographic" })).toHaveAttribute("aria-checked", "true");
    await page.keyboard.press("Escape");
    await page.keyboard.press("b");
    await expect(page.locator(".dimlabel")).toHaveCount(3);
    await page.keyboard.press("x");
    await expect(page.getByLabel("Display mode")).toHaveValue("xray");
    await expect(page).toHaveScreenshot("xray-ortho-bbox.png", { fullPage: true });
  });

  test("the stylesheet applies as written: nothing nested, and the build bar is fixed", async ({ page }) => {
    await open(page);
    // Chromium parses everything after an unclosed `{` as CSS Nesting inside
    // the rule that was never closed, and `document.styleSheets` shows it that
    // way: the trailing rules are present, but as children of a style rule.
    // They then apply only under that rule's selector, and no screenshot
    // notices, because the baselines were made from the same sheet. This
    // stylesheet does not use nesting, so a nested style rule is a parse
    // accident by definition.
    const nested = await page.evaluate(() => {
      const sheet = [...document.styleSheets].find((s) => (s.href ?? "").includes("styles.css"));
      if (!sheet) return ["no styles.css stylesheet on the page"];
      const out: string[] = [];
      const walk = (rules: CSSRuleList, insideStyleRule: boolean): void => {
        for (const r of rules) {
          if (r instanceof CSSStyleRule) {
            if (insideStyleRule) out.push(r.selectorText);
            walk(r.cssRules, true);
          } else if (r instanceof CSSGroupingRule) {
            walk(r.cssRules, insideStyleRule);
          }
        }
      };
      walk(sheet.cssRules, false);
      return out;
    });
    expect(nested, "style rules parsed as nested inside another style rule").toEqual([]);
    // The last rules of the file are the build bar. It is `hidden` until
    // there is news, but its computed style is what the sheet says either way.
    expect(await page.locator(".build-bar").evaluate((el) => getComputedStyle(el).position)).toBe("fixed");
  });

  test("static mode works without /api", async ({ page }) => {
    await open(page, "/?mock=1&nosw=1&static=1");
    await expect(page.locator(".pill-conn")).toHaveText("static");
    await expect(page.getByRole("region", { name: "Notes" })).toBeVisible();
    await page.getByRole("tab", { name: "API" }).click();
    await expect(page.getByRole("region", { name: "API reference" })).toContainText("extrude(");
  });
});

test.describe("phone", () => {
  test.skip(({ isMobile }) => !isMobile, "phone layout only");

  test("every control meets the 44 px touch target", async ({ page }) => {
    await open(page);
    // `preventPageZoom` takes pinch-zoom away from the page, and its stated
    // justification is this: "every control meets the 44px touch target".
    // The coarse-pointer block set a minimum height and no minimum width, so
    // the icon-only toolbar buttons were 30-35 px wide -- the smallest
    // targets on the screen were the ones the claim was made about.
    expect(await page.evaluate(() => matchMedia("(pointer: coarse)").matches)).toBe(true);
    const small = await page.evaluate(() => {
      const out: { name: string; w: number; h: number }[] = [];
      // The controls that are on screen and tappable without opening
      // anything: the toolbar's buttons and selects, and the tab bar. Items
      // inside the View menu's panel are laid out as a list and are excluded
      // -- a menu row is a different target shape from a 35 px icon button,
      // and it is not what pwa.ts's claim is about.
      const controls = document.querySelectorAll<HTMLElement>(
        ".toolbar .btn, .toolbar .icon-btn, .toolbar select, .tabbar button",
      );
      for (const el of controls) {
        if (el.closest(".menu-panel")) continue;
        const r = el.getBoundingClientRect();
        if (r.width === 0 && r.height === 0) continue; // not displayed in this layout
        if (r.width < 44 || r.height < 44) {
          out.push({ name: (el.getAttribute("aria-label") ?? el.textContent ?? "?").trim(), w: Math.round(r.width), h: Math.round(r.height) });
        }
      }
      return out;
    });
    expect(small, "controls under the 44 px touch target").toEqual([]);
  });

  test("Layers is the tab after Params while the document has layers, and its controls meet the 44 px touch target", async ({ page }) => {
    await open(page);
    const bar = page.getByRole("navigation", { name: "Panels" });
    const layersTab = bar.getByRole("button", { name: "Layers", exact: true });
    await page.getByRole("button", { name: "Model", exact: true }).click();
    const sheet = page.getByRole("dialog");
    await expect(sheet).toBeVisible();
    await sheet.getByRole("tab", { name: "Examples" }).click();
    // The dev server's one session may already hold this example (a
    // desktop test loads it too); its button is then "Loaded" and disabled.
    const load = sheet.getByRole("button", { name: "Load example Layers demo" });
    if (await load.isEnabled()) {
      await load.click();
      // Loading an example closes the sheet on a phone.
      await expect(sheet).toBeHidden();
    } else {
      await page.keyboard.press("Escape");
      await expect(sheet).toBeHidden();
    }
    // Layers is a tab of its own, the one after Params, and not the
    // tail of the Model sheet.
    await expect(bar.getByRole("button").nth(0)).toHaveAccessibleName("Params");
    await expect(bar.getByRole("button").nth(1)).toHaveAccessibleName("Layers");
    await page.getByRole("button", { name: "Model", exact: true }).click();
    await expect(sheet.getByRole("tree", { name: "Feature tree" })).toBeVisible();
    await expect(sheet.getByRole("region", { name: "Layers" })).toHaveCount(0);
    await sheet.getByRole("tab", { name: "Layers", exact: true }).click();
    const layers = sheet.getByRole("region", { name: "Layers" });
    await expect(layers).toBeVisible();
    await expect(layers.getByRole("listitem")).toHaveCount(2);
    // The switch is the parameters' switch, and its label is a thumb's
    // target in both dimensions.
    await expect(layers.getByRole("switch", { name: "Show envelope" })).toHaveClass(/param-switch/);
    const small = await layers.evaluate((root) => {
      const out: { name: string; w: number; h: number }[] = [];
      for (const el of root.querySelectorAll<HTMLElement>(".layer-name, .layer-switch-hit, .layer-range, .btn")) {
        const r = el.getBoundingClientRect();
        if (r.width === 0 && r.height === 0) continue;
        const wide = el.classList.contains("layer-switch-hit") || el.classList.contains("btn");
        if (r.height < 44 || (wide && r.width < 44)) {
          const named = el.matches("[aria-label]") ? el : el.querySelector("[aria-label]");
          out.push({ name: (named?.getAttribute("aria-label") ?? el.textContent ?? "?").trim(), w: Math.round(r.width), h: Math.round(r.height) });
        }
      }
      return out;
    });
    expect(small, "layer controls under the 44 px touch target").toEqual([]);
    // A tap on the switch is a redraw, not an evaluation.
    const evals = (await page.locator("html").getAttribute("data-evals")) as string;
    await layers.getByRole("switch", { name: "Show envelope" }).click();
    await expect(layers.getByRole("switch", { name: "Show envelope" })).not.toBeChecked();
    await page.waitForTimeout(400);
    await expect(page.locator("html")).toHaveAttribute("data-evals", evals);
    await layers.getByRole("button", { name: "Reset to script" }).click();
    await sheet.getByRole("tab", { name: "Examples" }).click();
    await sheet.getByRole("button", { name: "Load example Plate with boss" }).click();
    await expect(sheet).toBeHidden();
    // A document without layers has no tab for them.
    await expect(layersTab).toBeHidden();
  });

  test("narrow layout: tab bar opens panels in a bottom sheet", async ({ page }) => {
    await open(page);
    await expect(page.getByRole("navigation", { name: "Panels" })).toBeVisible();
    await expect(page.getByRole("dialog")).toBeHidden();
    await page.getByRole("button", { name: "Model", exact: true }).click();
    const sheet = page.getByRole("dialog");
    await expect(sheet).toBeVisible();
    await expect(sheet.getByRole("tree", { name: "Feature tree" })).toBeVisible();
    await expect(page).toHaveScreenshot("phone-sheet.png");
    await page.keyboard.press("Escape");
    await expect(sheet).toBeHidden();
    await expect(page).toHaveScreenshot("phone-home.png");
  });

  // The controls on a phone, at an iPhone-sized viewport.
  test.describe("controls on a phone", () => {
    test.use({ viewport: { width: 390, height: 844 } });

    const evals = async (page: Page): Promise<number> => Number(await page.locator("html").getAttribute("data-evals"));

    /** Drag the sheet's handle by `dy` pixels (negative is up). */
    async function dragHandle(page: Page, dy: number): Promise<void> {
      const hb = await page.locator(".sheet-handle").boundingBox();
      if (!hb) throw new Error("no sheet handle");
      const x = hb.x + hb.width / 2;
      const y = hb.y + hb.height / 2;
      await page.mouse.move(x, y);
      await page.mouse.down();
      await page.mouse.move(x, y + dy, { steps: 4 });
      await page.mouse.up();
    }

    test("Params is the first tab; the sheet opens at a peek height over the model and the handle pulls it up", async ({ page }) => {
      await open(page);
      const bar = page.getByRole("navigation", { name: "Panels" });
      await expect(bar.getByRole("button").first()).toHaveAccessibleName("Params");
      await bar.getByRole("button", { name: "Params" }).click();
      const sheet = page.getByRole("dialog");
      await expect(sheet).toBeVisible();
      await expect(sheet.getByRole("region", { name: "Parameters" })).toBeVisible();
      const peek = await sheet.boundingBox();
      if (!peek) throw new Error("no sheet");
      // About 40 % of the viewport, so the model stays in view above it.
      expect(peek.height).toBeGreaterThan(844 * 0.35);
      expect(peek.height).toBeLessThan(844 * 0.45);
      expect(peek.y).toBeGreaterThan(844 * 0.45);
      await dragHandle(page, -200);
      await expect(sheet).toHaveClass(/full/);
      const full = await sheet.boundingBox();
      expect(full?.height ?? 0).toBeGreaterThan(844 * 0.8);
      // Down once is back to the peek, not closed ...
      await dragHandle(page, 200);
      await expect(sheet).not.toHaveClass(/full/);
      await expect(sheet).toBeVisible();
      // ... and the tab remembers being pulled up while another does not.
      await sheet.getByRole("tab", { name: "Script" }).click();
      await dragHandle(page, -200);
      await expect(sheet).toHaveClass(/full/);
      await sheet.getByRole("tab", { name: "Params" }).click();
      await expect(sheet).not.toHaveClass(/full/);
      await sheet.getByRole("tab", { name: "Script" }).click();
      await expect(sheet).toHaveClass(/full/);
      await page.keyboard.press("Escape");
      await expect(sheet).toBeHidden();
    });

    test("a length reads in the document's feet and inches under its name, and follows the stepper", async ({ page }) => {
      await open(page, `${URL}&static=1`);
      await page.getByRole("navigation", { name: "Panels" }).getByRole("button", { name: "Script" }).click();
      const sheet = page.getByRole("dialog");
      await applyScript(page, sheet, FT_IN_SCRIPT);
      await sheet.getByRole("tab", { name: "Params" }).click();
      const params = sheet.getByRole("region", { name: "Parameters" });
      const width = params.locator(".param-row", { has: page.getByLabel("width (mm)") });
      await expect(width.locator(".param-readout")).toHaveText(`= 2 3/8"`);
      const field = params.getByLabel("span (ft)");
      await expect(field).toHaveValue("12.5");
      const span = params.locator(".param-row", { has: page.getByLabel("span (ft)") });
      const readout = span.locator(".param-readout");
      await readout.scrollIntoViewIfNeeded();
      await expect(readout).toBeVisible();
      await expect(readout).toHaveText(`= 12'-6"`);
      // Under the name, not beside it: the stepper keeps the head's line and
      // the name keeps the width that is left.
      const [name, text, stepper] = await Promise.all([
        span.locator(".param-name").boundingBox(),
        readout.boundingBox(),
        span.getByRole("group", { name: "span stepper" }).boundingBox(),
      ]);
      if (!name || !text || !stepper) throw new Error("the span row is missing a part");
      expect(text.y, "the readout is under the name").toBeGreaterThanOrEqual(name.y + name.height - 1);
      expect(Math.abs(text.x - name.x), "and starts where the name does").toBeLessThan(2);
      expect(text.x + text.width, "and stays clear of the stepper").toBeLessThanOrEqual(stepper.x);
      expect(stepper.y, "the stepper is on the name's line").toBeLessThan(name.y + name.height);
      // A tap on the stepper moves the value, and the readout with it.
      await span.getByRole("button", { name: "Increase span" }).click();
      await expect(field).toHaveValue("13");
      await expect(readout).toHaveText(`= 13'-0"`);
    });

    test("choices are segments, a bool is a switch, a number is a full-width slider with a stepper", async ({ page }) => {
      await open(page);
      // The document is whatever the last test left in the session; the plate
      // with the boss has a choice, a switch and numbers. The layer test
      // before this one leaves it loaded, and then its button is "Loaded".
      await page.getByRole("button", { name: "Model", exact: true }).click();
      const sheet = page.getByRole("dialog");
      await sheet.getByRole("tab", { name: "Examples" }).click();
      const loaded = await evals(page);
      const load = sheet.getByRole("button", { name: "Load example Plate with boss" });
      if (await load.isEnabled()) {
        await load.click();
        await expect(sheet).toBeHidden();
        await expect.poll(() => evals(page)).toBeGreaterThan(loaded);
      } else {
        await page.keyboard.press("Escape");
        await expect(sheet).toBeHidden();
      }

      await page.getByRole("button", { name: "Params" }).click();
      const params = sheet.getByRole("region", { name: "Parameters" });
      await expect(params).toBeVisible();
      // Overrides are session state and the session outlives the desktop
      // suite, so start from the script's own values.
      const resetAll = params.getByRole("button", { name: "Reset all" });
      if (await resetAll.isVisible()) {
        const reset = await evals(page);
        await resetAll.click();
        await expect.poll(() => evals(page)).toBeGreaterThan(reset);
      }
      await expect(params.locator(".param-row.overridden")).toHaveCount(0);

      // Three short choices are a segmented control, and a tap on a segment
      // sets the override with exactly one evaluation.
      const finish = params.getByRole("radiogroup", { name: "finish" });
      await expect(finish.getByRole("radio")).toHaveCount(3);
      await expect(finish.getByRole("radio", { name: "matte" })).toBeChecked();
      const before = await evals(page);
      await finish.getByRole("radio", { name: "gloss" }).click();
      await expect(finish.getByRole("radio", { name: "gloss" })).toBeChecked();
      await expect(params.locator(".param-row.overridden")).toHaveCount(1);
      await expect.poll(() => evals(page)).toBe(before + 1);
      await page.waitForTimeout(700);
      expect(await evals(page), "one tap, one evaluation").toBe(before + 1);

      // A bool is a switch, and the whole row is its label: tapping the name
      // toggles it, and the model loses its boss.
      const sw = params.getByRole("switch", { name: "with_boss" });
      await expect(sw).toBeChecked();
      await params.locator(".param-row-bool .param-name", { hasText: "with_boss" }).click();
      await expect(sw).not.toBeChecked();
      await expect.poll(() => evals(page)).toBe(before + 2);
      await sheet.getByRole("tab", { name: "Model" }).click();
      const tree = sheet.getByRole("tree", { name: "Feature tree" });
      await expect(tree.getByRole("treeitem", { name: /^base extrude/ })).toBeVisible();
      await expect(tree.getByRole("treeitem", { name: /^boss extrude/ })).toHaveClass(/suppressed/);
      await expect(tree.getByRole("treeitem", { name: /^boss 3 faces/ })).toHaveCount(0);
      await sheet.getByRole("tab", { name: "Params" }).click();

      // A number: the slider is full width with a thumb-sized hit area, and
      // the value sits in a stepper that moves one step a tap.
      const width = params.getByLabel("width (mm)");
      await expect(width).toHaveValue("60");
      await expect(width).toHaveAttribute("inputmode", "decimal");
      // The first tap makes an override, and the row's reset appears in the
      // place it kept while hidden: the stepper does not move, so a second
      // tap on "+" is a second step and not a tap on the reset.
      const increase = params.getByRole("button", { name: "Increase width" });
      const plusBefore = await increase.boundingBox();
      await increase.click();
      await expect(width).toHaveValue("61");
      await expect.poll(() => evals(page)).toBe(before + 3);
      await expect(params.getByRole("button", { name: "Reset width to its default" })).toBeVisible();
      const plusAfter = await increase.boundingBox();
      expect(plusAfter?.x, "the stepper stays put when the reset appears").toBeCloseTo(plusBefore?.x ?? Number.NaN, 0);
      await params.getByRole("button", { name: "Decrease width" }).click();
      await params.getByRole("button", { name: "Decrease width" }).click();
      await expect(width).toHaveValue("59");
      const slider = params.getByRole("slider", { name: "width slider" });
      const sb = await slider.boundingBox();
      const pb = await params.boundingBox();
      if (!sb || !pb) throw new Error("no slider");
      expect(sb.height, "a 44 px hit area").toBeGreaterThanOrEqual(44);
      expect(sb.width, "the slider is the row's full width").toBeGreaterThan(pb.width * 0.8);
      await expect(params.locator(".param-row.overridden")).toHaveCount(3);
      // Every tab shares the sheet's one scroller, so where the list sits
      // after the trip to the Model tab depends on how tall the tree was and
      // which row a click scrolled to. The picture starts from the top.
      await sheet.locator(".sheet-body").evaluate((el) => el.scrollTo(0, 0));
      await settled(page);
      await expect(page).toHaveScreenshot("phone-params.png");

      // The per-row reset and Reset all are still there.
      await params.getByRole("button", { name: "Reset width to its default" }).click();
      await expect(width).toHaveValue("60");
      await expect(params.locator(".param-row.overridden")).toHaveCount(2);
      await params.getByRole("button", { name: "Reset all" }).click();
      await expect(params.locator(".param-row.overridden")).toHaveCount(0);
      await expect(sw).toBeChecked();
      await expect(finish.getByRole("radio", { name: "matte" })).toBeChecked();
    });
  });

  // A phone's own layout: a title and one button over the model, the tools
  // in a sheet by the thumb, and a viewport where a tap and a drag are
  // different things.
  test.describe("the viewer on a phone", () => {
    test.use({ viewport: { width: 390, height: 844 } });

    /** One finger down at `from`, along to `to`, and up: real touch input, so the page sees pointer events of type "touch". */
    async function swipe(page: Page, from: { x: number; y: number }, to: { x: number; y: number }, steps = 8): Promise<void> {
      const cdp = await page.context().newCDPSession(page);
      const point = (x: number, y: number) => [{ x: Math.round(x), y: Math.round(y), id: 1 }];
      await cdp.send("Input.dispatchTouchEvent", { type: "touchStart", touchPoints: point(from.x, from.y) });
      for (let i = 1; i <= steps; i++) {
        const t = i / steps;
        await cdp.send("Input.dispatchTouchEvent", { type: "touchMove", touchPoints: point(from.x + (to.x - from.x) * t, from.y + (to.y - from.y) * t) });
        await page.waitForTimeout(16);
      }
      await cdp.send("Input.dispatchTouchEvent", { type: "touchEnd", touchPoints: [] });
      await cdp.detach();
    }

    /** The middle of the model: the fitted view centres it in the viewport. */
    async function centre(page: Page): Promise<{ x: number; y: number }> {
      const box = await page.locator("canvas.viewport-canvas").boundingBox();
      if (!box) throw new Error("no canvas");
      return { x: box.x + box.width / 2, y: box.y + box.height / 2 };
    }

    test("nothing runs off the side: the bar is a title and one button, and the tools are in the View sheet", async ({ page }) => {
      await open(page);
      for (const size of [
        { width: 390, height: 844 },
        { width: 360, height: 800 },
        { width: 844, height: 390 },
      ]) {
        await page.setViewportSize(size);
        await page.waitForTimeout(200);
        const off = await page.evaluate(() => {
          const out: string[] = [];
          const w = window.innerWidth;
          if (document.documentElement.scrollWidth > w) out.push(`page scrolls to ${document.documentElement.scrollWidth}`);
          for (const el of document.querySelectorAll<HTMLElement>(".toolbar *, .tabbar *, .vp-fab *, .triad")) {
            const r = el.getBoundingClientRect();
            if (r.width === 0 && r.height === 0) continue;
            if (r.right > w + 0.5 || r.left < -0.5) out.push(`${el.className || el.tagName} ${Math.round(r.left)}..${Math.round(r.right)}`);
          }
          return out;
        });
        expect(off, `off screen at ${size.width}x${size.height}`).toEqual([]);
      }
      await page.setViewportSize({ width: 390, height: 844 });
      // The bar offers one thing to press.
      const shown = await page.evaluate(() =>
        [...document.querySelectorAll<HTMLElement>(".toolbar button, .toolbar select")].filter((el) => el.getBoundingClientRect().width > 0).map((el) => el.getAttribute("aria-label") ?? el.textContent),
      );
      expect(shown).toEqual(["View and tools"]);
      const more = page.getByRole("button", { name: "View and tools" });
      await expect(more).toHaveAttribute("aria-expanded", "false");
      await more.click();
      await expect(more).toHaveAttribute("aria-expanded", "true");
      const sheet = page.getByRole("dialog");
      await expect(sheet).toHaveAttribute("data-tab", "view");
      // A peek: the model stays whole above the presets.
      await expect(sheet).toHaveAttribute("data-detent", "peek");
      const box = await sheet.boundingBox();
      expect(box?.height ?? 0).toBeLessThan(844 * 0.33);
      const tools = sheet.getByRole("region", { name: "View and tools" });
      for (const name of ["Home", "Iso", "Top", "Front", "Right"]) await expect(tools.getByRole("button", { name, exact: true })).toBeVisible();
      await expect(tools.getByRole("radiogroup", { name: "Display mode" }).getByRole("radio", { name: "Shaded + edges" })).toBeChecked();
      const small = await tools.evaluate((root) => {
        const out: string[] = [];
        for (const el of root.querySelectorAll<HTMLElement>("button, select")) {
          if (el.closest(".menu-panel")) continue;
          const r = el.getBoundingClientRect();
          if (r.width === 0 && r.height === 0) continue;
          if (r.width < 44 || r.height < 44) out.push(`${(el.getAttribute("aria-label") ?? el.textContent ?? "?").trim()} ${Math.round(r.width)}x${Math.round(r.height)}`);
        }
        return out;
      });
      expect(small, "tools under the 44 px touch target").toEqual([]);
      // A display mode is one tap, and the sheet stays for the next.
      await tools.getByRole("radio", { name: "Wireframe" }).click();
      await expect(tools.getByRole("radio", { name: "Wireframe" })).toBeChecked();
      await tools.getByRole("radio", { name: "Shaded + edges" }).click();
      await expect(sheet).toBeVisible();
      await page.keyboard.press("Escape");
      await expect(sheet).toBeHidden();
    });

    test("the sheet rests at a peek, a half and full, and a pull down past the peek puts it away", async ({ page }) => {
      await open(page);
      await page.getByRole("button", { name: "View and tools" }).click();
      const sheet = page.getByRole("dialog");
      const handle = page.locator(".sheet-handle");
      const height = async (): Promise<number> => (await sheet.boundingBox())?.height ?? 0;
      await expect(sheet).toHaveAttribute("data-detent", "peek");
      const peek = await height();
      // A tap on the handle is one step up.
      await handle.click();
      await expect(sheet).toHaveAttribute("data-detent", "half");
      await expect.poll(height).toBeGreaterThan(peek + 60);
      const half = await height();
      expect(half).toBeLessThan(844 * 0.45);
      await handle.click();
      await expect(sheet).toHaveAttribute("data-detent", "full");
      await expect.poll(height).toBeGreaterThan(844 * 0.8);
      // A finger on the handle carries the sheet with it, and it settles on
      // the height the finger reached.
      const hb = await handle.boundingBox();
      if (!hb) throw new Error("no handle");
      const x = hb.x + hb.width / 2;
      const y = hb.y + hb.height / 2;
      await swipe(page, { x, y }, { x, y: 844 - half - 40 });
      await expect(sheet).toHaveAttribute("data-detent", "half");
      const hb2 = await handle.boundingBox();
      if (!hb2) throw new Error("no handle");
      await swipe(page, { x, y: hb2.y + hb2.height / 2 }, { x, y: 830 });
      await expect(sheet).toBeHidden();
      // Where the tab was left is where it opens.
      await page.getByRole("navigation", { name: "Panels" }).getByRole("button", { name: "View" }).click();
      await expect(sheet).toHaveAttribute("data-detent", "half");
      await page.keyboard.press("Escape");
    });

    test("a tap selects, a drag orbits and selects nothing, and a tap on empty space lets go", async ({ page }) => {
      await open(page);
      const canvas = page.locator("canvas.viewport-canvas");
      const card = page.getByRole("region", { name: "Selection" });
      const c = await centre(page);
      await expect(card).toBeHidden();

      // A drag across the model turns it and selects nothing.
      await settled(page);
      const before = await canvas.screenshot();
      await swipe(page, c, { x: c.x + 110, y: c.y + 30 });
      await settled(page);
      expect(Buffer.compare(before, await canvas.screenshot()), "the drag turned the model").not.toBe(0);
      await expect(card).toBeHidden();
      // A short wobble under the slop is still a tap.
      await page.getByRole("button", { name: "Home view" }).click();
      await settled(page);
      await swipe(page, c, { x: c.x + 5, y: c.y + 4 }, 2);
      await expect(card).toBeVisible();
      await expect(card.locator(".selcard-name")).not.toBeEmpty();
      // The card's controls are thumb sized, and it sits clear of the tab bar.
      const small = await card.evaluate((root) => {
        const out: string[] = [];
        for (const el of root.querySelectorAll<HTMLElement>("button")) {
          const r = el.getBoundingClientRect();
          if (r.width === 0 && r.height === 0) continue;
          if (r.width < 44 || r.height < 44) out.push(`${(el.getAttribute("aria-label") ?? el.textContent ?? "?").trim()} ${Math.round(r.width)}x${Math.round(r.height)}`);
        }
        return out;
      });
      expect(small, "selection card controls under the 44 px touch target").toEqual([]);
      const [cardBox, barBox, fabBox] = await Promise.all([card.boundingBox(), page.locator(".tabbar").boundingBox(), page.locator(".vp-fab").boundingBox()]);
      if (!cardBox || !barBox || !fabBox) throw new Error("missing chrome");
      expect(cardBox.y + cardBox.height).toBeLessThanOrEqual(barBox.y);
      expect(fabBox.y + fabBox.height, "Home and Fit move up out of the card's way").toBeLessThanOrEqual(cardBox.y);
      // Orbiting with something selected keeps it selected.
      await swipe(page, { x: c.x, y: c.y - 200 }, { x: c.x + 90, y: c.y - 170 });
      await expect(card).toBeVisible();
      // A tap on empty space lets go of it.
      await page.touchscreen.tap(24, 150);
      await expect(card).toBeHidden();
      // And the card's own button does the same.
      await page.getByRole("button", { name: "Home view" }).click();
      await settled(page);
      await page.touchscreen.tap(c.x, c.y);
      await expect(card).toBeVisible();
      await card.getByRole("button", { name: "Selection details" }).click();
      const sheet = page.getByRole("dialog");
      await expect(sheet).toHaveAttribute("data-tab", "inspector");
      await expect(card).toBeHidden();
      await sheet.getByRole("button", { name: "Close panel" }).click();
      await expect(card).toBeVisible();
      await card.getByRole("button", { name: "Clear selection" }).click();
      await expect(card).toBeHidden();
    });

    // ---- a sheet over the viewport
    //
    // The sheet lies over the lower part of the canvas. The viewer is told how
    // much it covers and draws the model in the middle of what is left, with a
    // view offset on the camera: the orbit does not move, and a tap lands on
    // what is drawn under it.

    interface ViewportDebug {
      insets: { top: number; right: number; bottom: number; left: number };
      pose: { theta: number; phi: number; distance: number; target: { x: number; y: number; z: number } };
      camera: { eye: { x: number; y: number; z: number }; target: { x: number; y: number; z: number } };
    }
    type P3 = { x: number; y: number; z: number };

    const viewportState = (page: Page): Promise<ViewportDebug> =>
      page.evaluate(() => {
        const v = (window as unknown as { __cadpole: { viewport(): ViewportDebug } }).__cadpole.viewport();
        return { insets: v.insets, pose: v.pose, camera: v.camera };
      });

    /** Where model points are drawn, in page pixels (null: off the canvas). */
    async function drawnAt(page: Page, points: P3[]): Promise<({ x: number; y: number } | null)[]> {
      const box = await page.locator("canvas.viewport-canvas").boundingBox();
      if (!box) throw new Error("no canvas");
      const local = await page.evaluate((pts) => {
        const v = (window as unknown as { __cadpole: { viewport(): { screenPoint(p: unknown): { x: number; y: number } | null } } }).__cadpole.viewport();
        return pts.map((p) => v.screenPoint(p));
      }, points);
      return local.map((p) => (p ? { x: p.x + box.x, y: p.y + box.y } : null));
    }

    /** What Home and Fit frame: its centre, the middle of its underside, and its corners. */
    async function framed(page: Page): Promise<{ centre: P3; under: P3; corners: P3[] }> {
      const [x0, y0, z0, x1, y1, z1] = ((await page.locator("html").getAttribute("data-frame-box")) ?? "").split(",").map(Number) as [number, number, number, number, number, number];
      const corners: P3[] = [];
      for (let i = 0; i < 8; i++) corners.push({ x: i & 1 ? x1 : x0, y: i & 2 ? y1 : y0, z: i & 4 ? z1 : z0 });
      return { centre: { x: (x0 + x1) / 2, y: (y0 + y1) / 2, z: (z0 + z1) / 2 }, under: { x: (x0 + x1) / 2, y: (y0 + y1) / 2, z: z0 }, corners };
    }

    const openChat = async (page: Page): Promise<Locator> => {
      await page.getByRole("navigation", { name: "Panels" }).getByRole("button", { name: "Chat" }).click();
      const sheet = page.getByRole("dialog");
      await expect(sheet).toHaveAttribute("data-tab", "messages");
      await settled(page);
      return sheet;
    };

    test("a sheet over the viewport moves the model into what is left, without moving the camera, and closing it puts the view back", async ({ page }) => {
      await open(page);
      await settled(page);
      const canvas = await page.locator("canvas.viewport-canvas").boundingBox();
      if (!canvas) throw new Error("no canvas");
      const f = await framed(page);
      const plain = await viewportState(page);
      expect(plain.insets).toEqual({ top: 0, right: 0, bottom: 0, left: 0 });
      // The orbit target is the framed centre, exactly (the box on <html> is
      // rounded to the millimetre).
      const mid = plain.pose.target;
      const [c0, u0] = await drawnAt(page, [mid, f.under]);
      if (!c0 || !u0) throw new Error("the model is not in view");
      // The fitted view centres the model in the canvas, so its underside is
      // in the lower half: where a sheet lies.
      expect(Math.abs(c0.x - (canvas.x + canvas.width / 2))).toBeLessThan(1.5);
      expect(Math.abs(c0.y - (canvas.y + canvas.height / 2))).toBeLessThan(1.5);
      expect(u0.y).toBeGreaterThan(canvas.y + canvas.height / 2);

      // Chat rests low, and stays there over a keyboard (the class is what a
      // keyboard sets; no keyboard is drawn here).
      const sheet = await openChat(page);
      await expect(sheet).toHaveAttribute("data-detent", "peek");
      const sb = await sheet.boundingBox();
      if (!sb) throw new Error("no sheet");
      expect(sb.height).toBeLessThan(844 * 0.33);
      await sheet.evaluate((el) => el.classList.add("keyboard"));
      expect((await sheet.boundingBox())?.height).toBe(sb.height);
      await sheet.evaluate((el) => el.classList.remove("keyboard"));

      const covered = await viewportState(page);
      expect(covered.insets.bottom).toBeCloseTo(canvas.y + canvas.height - sb.y, 0);
      expect(covered.pose, "the orbit is where it was").toEqual(plain.pose);
      expect(covered.camera).toEqual(plain.camera);
      const [c1, u1] = await drawnAt(page, [mid, f.under]);
      if (!c1 || !u1) throw new Error("the model is not in view under the sheet");
      const clearMid = canvas.y + (sb.y - canvas.y) / 2;
      expect(Math.abs(c1.x - c0.x)).toBeLessThan(1.5);
      expect(Math.abs(c1.y - clearMid), "the framed centre is in the middle of what the sheet leaves").toBeLessThan(1.5);
      expect(u1.y, "the model's underside is above the sheet").toBeLessThan(sb.y);
      // The same picture, moved: the underside is as far under the centre as it was.
      expect(Math.abs(u1.y - c1.y - (u0.y - c0.y))).toBeLessThan(1.5);

      // A keyboard lifts the sheet without resizing it, and the model with it
      // (the inset is written as the page's own tracker writes it; no
      // keyboard is drawn here).
      const keyboard = (px: number): Promise<void> =>
        page.evaluate((px) => {
          document.documentElement.style.setProperty("--kbd", `${px}px`);
          document.querySelector(".sheet")?.classList.toggle("keyboard", px > 0);
          window.dispatchEvent(new Event("cadpole:keyboard"));
        }, px);
      await keyboard(300);
      await settled(page);
      const lifted = await sheet.boundingBox();
      if (!lifted) throw new Error("no sheet");
      expect(lifted.y).toBeCloseTo(sb.y - 300, 0);
      expect(lifted.height).toBe(sb.height);
      expect((await viewportState(page)).insets.bottom).toBeCloseTo(covered.insets.bottom + 300, 0);
      const [c3] = await drawnAt(page, [mid]);
      expect(Math.abs((c3?.y ?? 0) - (canvas.y + (lifted.y - canvas.y) / 2))).toBeLessThan(1.5);
      await keyboard(0);
      await settled(page);
      expect((await viewportState(page)).insets.bottom).toBeCloseTo(covered.insets.bottom, 0);

      // Home and Fit stand clear of the sheet, and Fit fits what is left.
      const fit = page.getByRole("button", { name: "Fit all" });
      const fb = await fit.boundingBox();
      expect((fb?.y ?? 1e9) + (fb?.height ?? 0)).toBeLessThanOrEqual(sb.y);
      await sheet.locator(".sheet-handle").click();
      await expect(sheet).toHaveAttribute("data-detent", "half");
      await settled(page);
      const half = await sheet.boundingBox();
      if (!half) throw new Error("no sheet");
      await fit.click();
      await settled(page);
      for (const p of await drawnAt(page, f.corners)) {
        if (!p) throw new Error("a corner of the model is off the canvas after Fit");
        expect(p.y).toBeLessThan(half.y);
        expect(p.y).toBeGreaterThan(canvas.y);
        expect(p.x).toBeGreaterThan(canvas.x);
        expect(p.x).toBeLessThan(canvas.x + canvas.width);
      }
      const fitted = await viewportState(page);
      // Never from nearer than the whole canvas was: the width decides here,
      // and the sheet takes none of it.
      expect(fitted.pose.distance).toBeGreaterThanOrEqual(plain.pose.distance);

      // Closing the sheet puts the picture's centre back in the middle of the canvas.
      await page.keyboard.press("Escape");
      await expect(sheet).toBeHidden();
      await settled(page);
      expect((await viewportState(page)).insets).toEqual({ top: 0, right: 0, bottom: 0, left: 0 });
      await page.getByRole("button", { name: "Home view" }).click();
      await settled(page);
      const home = await viewportState(page);
      expect(home.pose.distance).toBeCloseTo(plain.pose.distance, 3);
      const [c2] = await drawnAt(page, [home.pose.target]);
      expect(Math.abs((c2?.y ?? 0) - c0.y)).toBeLessThan(1.5);
      expect(Math.abs((c2?.x ?? 0) - c0.x)).toBeLessThan(1.5);
    });

    test("a tap above the sheet selects what is drawn under the finger", async ({ page }) => {
      await open(page);
      await settled(page);
      const card = page.getByRole("region", { name: "Selection" });
      const f = await framed(page);
      // Two places on the model, named by where they are from its centre.
      const offsets = [
        { dx: 0, dy: 0 },
        { dx: -46, dy: 34 },
      ];
      const names: string[] = [];
      for (const o of offsets) {
        const [c] = await drawnAt(page, [f.centre]);
        if (!c) throw new Error("the model is not in view");
        await page.touchscreen.tap(c.x + o.dx, c.y + o.dy);
        await expect(card).toBeVisible();
        names.push(`${await card.locator(".selcard-kind").textContent()} ${await card.locator(".selcard-name").textContent()}`);
        await card.getByRole("button", { name: "Clear selection" }).click();
        await expect(card).toBeHidden();
      }
      expect(names[0]).toMatch(/^face /);

      const sheet = await openChat(page);
      const sb = await sheet.boundingBox();
      if (!sb) throw new Error("no sheet");
      for (const [i, o] of offsets.entries()) {
        const [c] = await drawnAt(page, [f.centre]);
        if (!c) throw new Error("the model is not in view under the sheet");
        expect(c.y + o.dy).toBeLessThan(sb.y - 12);
        await page.touchscreen.tap(c.x + o.dx, c.y + o.dy);
        // The card is the sheet's to hide; it names the selection once the sheet is away.
        await page.keyboard.press("Escape");
        await expect(sheet).toBeHidden();
        await expect(card).toBeVisible();
        expect(`${await card.locator(".selcard-kind").textContent()} ${await card.locator(".selcard-name").textContent()}`).toBe(names[i]);
        await card.getByRole("button", { name: "Clear selection" }).click();
        await openChat(page);
      }
      await page.keyboard.press("Escape");
    });

    test("a view linked or left with a sheet open comes back as the same view", async ({ page }) => {
      await open(page);
      const c = await centre(page);
      await swipe(page, { x: c.x, y: c.y - 150 }, { x: c.x + 80, y: c.y - 120 });
      await settled(page);
      await openChat(page);
      const before = await viewportState(page);
      expect(before.insets.bottom).toBeGreaterThan(100);
      const link = await page.evaluate(() => (window as unknown as { __cadpole: { viewLink(): string } }).__cadpole.viewLink());

      // Left and come back to: the orbit this tab remembered, under the same sheet.
      await page.reload();
      await expect(page.locator("html")).toHaveAttribute("data-evals", /[1-9]/);
      await expect(page.getByRole("dialog")).toHaveAttribute("data-tab", "messages");
      await settled(page);
      const back = await viewportState(page);
      expect(back.insets).toEqual(before.insets);
      for (const k of ["theta", "phi", "distance"] as const) expect(back.pose[k]).toBeCloseTo(before.pose[k], 6);
      const [t0] = await drawnAt(page, [before.pose.target]);
      const canvas = await page.locator("canvas.viewport-canvas").boundingBox();
      if (!canvas || !t0) throw new Error("no canvas");
      expect(Math.abs(t0.y - (canvas.y + (canvas.height - before.insets.bottom) / 2))).toBeLessThan(1.5);

      // The link is the camera, whatever covered the viewport when it was made.
      await page.keyboard.press("Escape");
      await page.goto(link);
      await expect(page.locator("html")).toHaveAttribute("data-evals", /[1-9]/);
      await expect(page.getByRole("dialog")).toBeHidden();
      await settled(page);
      const linked = await viewportState(page);
      // To the link's own rounding of a coordinate.
      for (const k of ["x", "y", "z"] as const) {
        expect(Math.abs(linked.camera.eye[k] - before.camera.eye[k])).toBeLessThan(0.06);
        expect(Math.abs(linked.camera.target[k] - before.camera.target[k])).toBeLessThan(0.06);
      }
      const [t1] = await drawnAt(page, [linked.camera.target]);
      expect(Math.abs((t1?.y ?? 0) - (canvas.y + canvas.height / 2))).toBeLessThan(1.5);
    });

    test("the page itself does not scroll, zoom or select text under a finger", async ({ page }) => {
      await open(page);
      const c = await centre(page);
      await swipe(page, c, { x: c.x, y: c.y - 300 });
      await swipe(page, c, { x: c.x - 150, y: c.y });
      const state = await page.evaluate(() => ({
        x: window.scrollX,
        y: window.scrollY,
        scale: window.visualViewport?.scale ?? 1,
        touch: getComputedStyle(document.querySelector("canvas.viewport-canvas") as Element).touchAction,
        selected: String(getSelection() ?? ""),
        theme: document.querySelector('meta[name="theme-color"]')?.getAttribute("content") ?? "",
      }));
      expect(state.x).toBe(0);
      expect(state.y).toBe(0);
      expect(state.scale).toBe(1);
      expect(state.touch).toBe("none");
      expect(state.selected).toBe("");
      expect(state.theme).not.toBe("");
    });
  });
});
