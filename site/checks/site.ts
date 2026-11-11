// What every check shares: the site on a loopback port of its own, and the
// five windows the page is held to.
import { test as base, type Browser, type BrowserContext } from "@playwright/test";
import { existsSync } from "node:fs";
import { dirname, resolve } from "node:path";
import { fileURLToPath } from "node:url";

import { serve } from "../tools/serve.mjs";

const here = dirname(fileURLToPath(import.meta.url));

/** The site under test: an assembled copy when one is named, else the build's. */
export const root = process.env["CADPOLE_SITE_ROOT"] || resolve(here, "..", process.env["CADPOLE_SITE_DIST"] ?? "../bazel-bin/site/dist");
/** Whether every frame of every scene is there, or the first of each only. */
export const whole = existsSync(resolve(root, "seq/tj/d/001.avif"));

export const VIEWPORTS: Record<string, { width: number; height: number; dpr: number; touch: boolean }> = {
  "phone-390": { width: 390, height: 844, dpr: 3, touch: true },
  "phone-360": { width: 360, height: 800, dpr: 3, touch: true },
  "tablet-820": { width: 820, height: 1180, dpr: 2, touch: true },
  "land-844": { width: 844, height: 390, dpr: 3, touch: true },
  "desk-1440": { width: 1440, height: 900, dpr: 1, touch: false },
};

export function context(browser: Browser, name: string): Promise<BrowserContext> {
  const v = VIEWPORTS[name]!;
  return browser.newContext({ viewport: { width: v.width, height: v.height }, deviceScaleFactor: v.dpr, hasTouch: v.touch, isMobile: v.touch });
}

export const test = base.extend<object, { site: string }>({
  site: [
    async ({}, use) => {
      const server = await serve(root, 0);
      const { port } = server.address() as { port: number };
      await use(`http://127.0.0.1:${port}/`);
      server.closeAllConnections();
      server.close();
    },
    { scope: "worker" },
  ],
});
export { expect } from "@playwright/test";
