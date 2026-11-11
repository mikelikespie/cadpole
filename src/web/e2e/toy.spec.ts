// The gear toy's plates on the real engine: `examples/gear_toy.star`
// declares three Bambu plates -- the print-in-place core, the lid, the crank
// with its knob -- and four filaments. Opt-in (`CADPOLE_E2E_TOY=1`, see
// playwright.config.ts), because the toy takes tens of seconds to evaluate
// in wasm; what it produces is the picture of the plates view, saved to
// `CADPOLE_E2E_TOY_SHOT` when that is set. No baseline: the toy's tessellation
// is not something a screenshot diff should hold still.

import { expect, test } from "@playwright/test";

test("the gear toy's three plates in the plates view", async ({ page }) => {
  const pageErrors: string[] = [];
  page.on("pageerror", (err) => pageErrors.push(err.message));
  await page.goto("/?nosw=1");
  await expect(page.locator("html")).toHaveClass(/ready/);
  await expect(page.locator("html")).toHaveAttribute("data-evals", /[1-9]/, { timeout: 540_000 });
  await expect(page.locator("html")).toHaveAttribute("data-engine", "worker");
  const platesBtn = page.getByRole("button", { name: "Plates", exact: true });
  await expect(platesBtn).toBeVisible();
  await page.waitForTimeout(1600);
  await platesBtn.click();
  await expect(page.locator("html")).toHaveAttribute("data-view", "plates");
  const tabs = page.getByRole("tablist", { name: "Plates" });
  await expect(tabs.getByRole("tab")).toHaveCount(4);
  await expect(tabs.getByRole("tab", { name: "1 · Core (print in place)" })).toBeVisible();
  await expect(tabs.getByRole("tab", { name: "2 · Lid" })).toBeVisible();
  await expect(tabs.getByRole("tab", { name: "3 · Crank" })).toBeVisible();
  const legend = page.getByRole("list", { name: "Filaments" });
  await expect(legend.getByRole("listitem")).toHaveCount(4);
  await expect(page.locator(".platelabel")).toHaveCount(3);
  await page.waitForTimeout(500);
  const shot = process.env["CADPOLE_E2E_TOY_SHOT"];
  if (shot) await page.screenshot({ path: shot, fullPage: true });
  else await page.screenshot({ path: "toy-plates.png", fullPage: true });
  expect(pageErrors, `page errors: ${pageErrors.join(" | ")}`).toEqual([]);
});
