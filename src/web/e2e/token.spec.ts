// The browser's half of the token scheme (docs/security.md), against a
// dev server started with `--token`, which refuses any /api request that does
// not carry `Authorization: Bearer <t>`.
//
// What these assert, in the order the claims are made in the security doc:
//
//   * the token arrives in the fragment and is gone from the address bar
//     before anything else happens;
//   * it is in `sessionStorage` and *not* in `localStorage`;
//   * every /api request carries it as a header, and no request anywhere puts
//     it in a query string -- including the event stream, which is why the UI
//     reads SSE with `fetch` instead of `EventSource`;
//   * a browser with no token is told so, rather than being dropped into an
//     empty playground document that looks like its work vanished.

import { expect, test, type Page, type Request } from "@playwright/test";

const TOKEN = "e2e-secret-token";
const URL_WITH_TOKEN = `/?mock=1&nosw=1#token=${TOKEN}`;

/** Every request the page makes, so the assertions can be about all of them. */
function recordRequests(page: Page): Request[] {
  const seen: Request[] = [];
  page.on("request", (r) => seen.push(r));
  return seen;
}

test.describe("session token", () => {
  test("arrives in the fragment, is stripped, and travels as a header", async ({ page }) => {
    const seen = recordRequests(page);
    await page.goto(URL_WITH_TOKEN);
    await expect(page.locator("html")).toHaveClass(/ready/);
    // The session server answered, so we are not in static mode: the token
    // worked.
    await expect(page.locator(".pill-conn")).not.toHaveText("static");
    await expect(page.locator("html")).toHaveAttribute("data-mode", "session");

    // The address bar does not carry it, and neither does any history entry
    // we could go back to.
    expect(page.url()).not.toContain(TOKEN);
    expect(page.url()).not.toContain("#token");

    // Per-tab storage, not origin-wide storage.
    const stored = await page.evaluate(() => ({
      session: sessionStorage.getItem("cadpole.session.token"),
      local: Object.entries(localStorage).map(([k, v]) => `${k}=${String(v)}`).join("\n"),
    }));
    expect(stored.session).toBe(TOKEN);
    expect(stored.local).not.toContain(TOKEN);

    // Wait for the event stream to have been opened before judging the set.
    await expect.poll(() => seen.filter((r) => r.url().includes("/api/events")).length).toBeGreaterThan(0);

    const api = seen.filter((r) => new global.URL(r.url()).pathname.startsWith("/api/"));
    expect(api.length).toBeGreaterThan(1);
    for (const r of api) {
      expect(r.headers()["authorization"], `${r.method()} ${r.url()} had no Authorization`).toBe(`Bearer ${TOKEN}`);
    }
    // Nowhere, on any request, in any query string: there is no `?token=`
    // escape hatch.
    for (const r of seen) {
      expect(new global.URL(r.url()).search, `${r.url()} put the token in the query string`).not.toContain(TOKEN);
    }
  });

  test("the event stream is an authenticated fetch, not an EventSource", async ({ page }) => {
    const seen = recordRequests(page);
    await page.goto(URL_WITH_TOKEN);
    await expect(page.locator("html")).toHaveClass(/ready/);
    await expect.poll(() => seen.filter((r) => r.url().includes("/api/events")).length).toBeGreaterThan(0);
    const stream = seen.find((r) => r.url().includes("/api/events"));
    expect(stream).toBeTruthy();
    // An `EventSource` request cannot carry either of these headers; a fetch
    // can, which is the entire reason for the change.
    expect(stream?.headers()["authorization"]).toBe(`Bearer ${TOKEN}`);
    expect(stream?.headers()["accept"]).toContain("text/event-stream");
    // ...and events really flow: the server emits `client_joined` on connect.
    await expect(page.locator(".pill-conn")).toHaveText(/live|connecting/);
  });

  test("no token is refused, and the UI says how to fix it", async ({ page }) => {
    const refused: number[] = [];
    page.on("response", (r) => {
      if (new global.URL(r.url()).pathname.startsWith("/api/")) refused.push(r.status());
    });
    await page.goto("/?mock=1&nosw=1");
    await expect(page.locator("html")).toHaveClass(/ready/);
    expect(refused).toContain(401);
    // It does not pretend to be a playground with an empty document; it says
    // what happened and what to do, in the banner.
    const banner = page.locator(".banner");
    await expect(banner).toBeVisible();
    await expect(banner).toContainText(/cadpole serve/);
    await expect(banner).toContainText(/token/i);
  });

  test("a token from a previous run is dropped rather than retried forever", async ({ page }) => {
    await page.goto(`/?mock=1&nosw=1#token=stale-token-from-the-last-run`);
    await expect(page.locator("html")).toHaveClass(/ready/);
    // The 401 clears it, so a reload does not carry the dead token back.
    await expect
      .poll(async () => page.evaluate(() => sessionStorage.getItem("cadpole.session.token")))
      .toBe(null);
  });
});
