// What the page links to and what it says: every local address has a file,
// "Full viewer" stays inside the site, the repository links go to one place,
// the words the page does not use are not on it, and the directory holds
// nothing that is not the site.
import { existsSync, readFileSync, readdirSync, statSync } from "node:fs";
import { join } from "node:path";

import { expect, root, test, whole } from "./site.js";

const REPO = "https://github.com/mikelikespie/cadpole";
const html = readFileSync(join(root, "index.html"), "utf8");
const attrs = (re: RegExp) => [...html.matchAll(re)].map((m) => m[1]!);

test("every local address has a file", () => {
  const missing: string[] = [];
  for (const u of attrs(/\s(?:href|src|data-pic|data-svg|data-draw|data-poster|data-sheet3d|data-open3d|data-model)="([^"#?]+)[^"]*"/g)) {
    if (/^(https?:|mailto:|data:)/.test(u)) continue;
    const p = join(root, u);
    if (!(existsSync(p) && (statSync(p).isFile() || existsSync(join(p, "index.html"))))) missing.push(u);
  }
  expect([...new Set(missing)]).toEqual([]);
  for (const base of attrs(/data-video="([^"]+)"/g)) {
    expect(existsSync(join(root, `${base}-poster.webp`)), `${base}-poster.webp`).toBe(true);
    if (whole) for (const ext of ["mp4", "webm"]) expect(existsSync(join(root, `${base}.${ext}`)), `${base}.${ext}`).toBe(true);
  }
});

test("each scene's frames are all there, or only its first", () => {
  for (const m of html.matchAll(/data-seq="(\w+)"(?: data-xl="1")? data-n="(\d+)" data-nm="(\d+)"/g)) {
    const [, seq, n, nm] = m;
    for (const [dir, count] of [["d", n], ["m", nm]] as const) {
      const have = readdirSync(join(root, "seq", seq!, dir)).length;
      expect(have === +count! || (!whole && have === 1), `seq/${seq}/${dir}: ${have} of ${count} frames`).toBe(true);
    }
  }
});

test("viewer links stay in the site and open a page that is there", () => {
  const full = attrs(/<a class="[^"]*" href="([^"]+)" data-full=/g);
  expect(full.length).toBeGreaterThan(10);
  for (const u of full) {
    expect(u).toMatch(/^view\/[a-z0-9_-]+\/$/);
    // The page loads the one copy of the app the site keeps.
    expect(readFileSync(join(root, u, "index.html"), "utf8"), u).toMatch(/"\.\.\/app\.[0-9a-f]+\.js"/);
    expect(readdirSync(join(root, u)).filter((f) => f.endsWith(".cadpole")).length, `${u}: one model`).toBe(1);
  }
  const names = Object.keys(JSON.parse(/id="full-viewers">([^<]+)</.exec(html)![1]!));
  expect(readdirSync(join(root, "view")).filter((f) => !f.includes(".")).sort()).toEqual(names.sort());
  for (const n of names) expect(existsSync(join(root, "models", `${n}.cpmesh`)), `models/${n}.cpmesh`).toBe(true);
});

/** The page's text, without its tags: what a reader sees, and what a copy button copies. */
const text = html.replace(/<[^>]*>/g, "");

test("outside links: the repository, the files in it, and the two installers the text names", () => {
  const out = [...new Set(attrs(/href="(https?:[^"]+)"/g))];
  for (const u of out) {
    if (u.startsWith("https://github.com/")) expect(u === "https://github.com/bazelbuild/bazelisk#installation" || u === REPO || /^https:\/\/github\.com\/mikelikespie\/cadpole\/(blob|tree)\/main\/(docs\/[\w.-]+\.md|examples\/[\w.-]+\.star|bin\/[\w.-]+|README\.md|LICENSE)$/.test(u), u).toBe(true);
    else expect(["https://brew.sh", "https://code.claude.com/docs/en/headless"], u).toContain(u);
  }
  expect(out).toContain(REPO);
  expect(text).toContain(`git clone ${REPO}.git`);
});

test("the repository's address and paths in the text are links, in prose and in the boxes to copy", () => {
  // Text a link could carry but does not: outside a link, a button, a summary, a listing or the head.
  const prose = html.replace(/<(head|script|style|textarea|pre)\b[\s\S]*?<\/\1>/gi, "").replace(/<(a|button|summary)\b[\s\S]*?<\/\1>/gi, "").replace(/<[^>]*>/g, " ");
  expect(prose).not.toMatch(/github\.com\/mikelikespie\/cadpole/);
  expect(prose).not.toMatch(/(?<![\w./~-])(?:\.\/)?(docs|examples|bin)\/[\w.-]+/);
  // Each link names what it opens.
  for (const m of html.matchAll(/<a class="ref" href="([^"]+)">([^<]+)<\/a>/g)) {
    const [, href, t] = m;
    if (t!.includes("github.com/")) expect(href).toBe(REPO);
    else expect(href!.replace(/^.*\/(blob|tree)\/main\//, "")).toBe(t!.replace(/^\.\//, "").replace(/\/$/, ""));
  }
  const prompt = /<div class="cmds"><div class="cmd multi prompt"><code>([\s\S]*?)<\/code>/.exec(html.slice(html.indexOf('id="connect"')))![1]!;
  expect(prompt).toContain(`<a class="ref" href="${REPO}">github.com/mikelikespie/cadpole</a>`);
  expect(prompt).toContain(`<a class="ref" href="${REPO}/blob/main/docs/assistant-integration.md">docs/assistant-integration.md</a>`);
});

test("the words the page does not use", () => {
  // The agent section and its setup further down are the one place the product is named.
  let agent = "", rest = html;
  for (const id of ["agent", "connect"]) {
    const a = rest.indexOf(`id="${id}"`), b = rest.indexOf("</section>", a);
    expect(a, `#${id}`).toBeGreaterThan(0);
    agent += rest.slice(a, b);
    rest = rest.slice(0, a) + rest.slice(b);
  }
  for (const w of [/claude/i, /formal(ly)? verif/i, /model.check/i, /\binvariant/i, /supply.chain/i, /licen[cs]e (check|scan|audit)/i, /advisor/i, /\/home\//, /\/Users\//, /localhost|127\.0\.0\.1:\d+\/[a-z]/]) expect(rest).not.toMatch(w);
  // The one product the section is about, named where the steps need it and not as a maker or a backer.
  expect((agent.match(/Claude/g) ?? []).length).toBeLessThanOrEqual(6);
  expect(agent).toContain("not affiliated with or endorsed by");
  expect(agent).toContain("your own Claude Code install");
  expect(agent).not.toMatch(/subscription|log ?in|signed in|api key/i);
  expect(html).toContain("Xcode 27");
  expect(html).toContain("https://github.com/bazelbuild/bazelisk#installation");
  expect(html).not.toContain("bazelisk-linux-amd64");
  expect(html).toContain("./bin/install");
  // Link previews: absolute addresses for the page and its picture.
  expect(html).toContain('<meta property="og:url" content="https://mikelikespie.github.io/cadpole/">');
  expect(html).toContain('<meta property="og:image" content="https://mikelikespie.github.io/cadpole/og.png">');
  expect(html).toContain('<meta name="twitter:card" content="summary_large_image">');
  expect(html).not.toContain("bazel-opt-bin");
  expect(html).toContain("Apache-2.0");
});

test("the directory is the site and nothing else", () => {
  expect(existsSync(join(root, ".nojekyll"))).toBe(true);
  const top = readdirSync(root).filter((f) => f !== ".git" && f !== "CNAME").sort();
  expect(top).toEqual([".nojekyll", "agent", "apple-touch-icon.png", "draw", "engine", "icon-192.png", "icon-32.png", "icon-512.png", "icon.svg", "index.html", "media", "models", "og.png", "print", "seq", "site.css", "site.js", "site.webmanifest", "site3d.js", "view"]);
  // The recording the page plays, and no other; the print files are plated projects.
  expect(readdirSync(join(root, "agent")).filter((f) => !/^agent(-poster\.webp|\.mp4|\.webm)$/.test(f))).toEqual([]);
  expect(readdirSync(join(root, "print")).filter((f) => !f.endsWith(".3mf"))).toEqual([]);
});
