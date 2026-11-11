// devserver.mjs serves its dist directory and nothing next to it.
//
// The containment check was `file.startsWith(dist)` with no separator, so a
// sibling whose name merely starts with the dist directory's (`dist-old`,
// `dist.bak`, `site-private` beside `site`) passed it: `/..%2fsite-private/x`
// decodes to `/../site-private/x` after URL parsing, normalizes to
// `<parent>/site-private/x`, and was served.

import assert from "node:assert/strict";
import { spawn } from "node:child_process";
import { mkdirSync, mkdtempSync, readFileSync, rmSync, symlinkSync, writeFileSync } from "node:fs";
import { request } from "node:http";
import { createServer } from "node:net";
import { tmpdir } from "node:os";
import { join, resolve } from "node:path";
import { after, before, test } from "node:test";

const devserver = resolve("devserver.mjs");

function freePort() {
  return new Promise((ok, fail) => {
    const s = createServer();
    s.once("error", fail);
    s.listen(0, "127.0.0.1", () => {
      const { port } = s.address();
      s.close(() => ok(port));
    });
  });
}

/** GET a path exactly as written: no client-side normalization. */
function get(port, path, method = "GET") {
  return new Promise((ok, fail) => {
    const req = request({ host: "127.0.0.1", port, path, method }, (res) => {
      let body = "";
      res.setEncoding("utf8");
      res.on("data", (c) => (body += c));
      res.on("end", () => ok({ status: res.statusCode, body }));
    });
    req.once("error", fail);
    req.end();
  });
}

/** Start a dev server and wait for its banner. */
async function start(args) {
  const port = await freePort();
  const child = spawn(process.execPath, [devserver, ...args, "--port", String(port)], { stdio: ["ignore", "pipe", "pipe"] });
  let err = "";
  child.stderr.on("data", (c) => (err += c));
  await new Promise((ok, fail) => {
    let out = "";
    child.stdout.on("data", (c) => {
      out += c;
      if (out.includes("cadpole devserver:")) ok();
    });
    child.once("exit", (code) => fail(new Error(`devserver exited ${code}: ${err}`)));
  });
  return { port, child };
}

let root;
const servers = [];
before(() => {
  root = mkdtempSync(join(tmpdir(), "devserver-static-"));
  for (const [dir, file, text] of [
    ["site", "ok.txt", "INSIDE"],
    ["site-private", "p.txt", "SIBLING-PRIVATE"],
    ["other", "o.txt", "OTHER"],
    ["tok", "t.txt", "TOKEN-DIR"],
  ]) {
    mkdirSync(join(root, dir));
    writeFileSync(join(root, dir, file), text);
  }
});
after(() => {
  for (const s of servers) s.child.kill();
  rmSync(root, { recursive: true, force: true });
});

test("a sibling whose name starts with the dist directory's is not served", async () => {
  const s = await start([join(root, "site"), "--static"]);
  servers.push(s);
  assert.deepEqual(await get(s.port, "/ok.txt"), { status: 200, body: "INSIDE" });
  const sibling = await get(s.port, "/..%2fsite-private/p.txt");
  assert.notEqual(sibling.body, "SIBLING-PRIVATE", "served a file outside the dist directory");
  assert.equal(sibling.status, 403);
  assert.equal((await get(s.port, "/..%2fother/o.txt")).status, 403);
});

test("--token's value is not taken for the dist directory", async () => {
  const s = await start(["--token", join(root, "tok"), join(root, "site"), "--static"]);
  servers.push(s);
  assert.deepEqual(await get(s.port, "/ok.txt"), { status: 200, body: "INSIDE" });
  assert.equal((await get(s.port, "/t.txt")).status, 404);
});

test("--hot rewrites its own copy, not the files a dist directory links to", async () => {
  // Under Bazel every file of dist is a symlink into the shared build output.
  mkdirSync(join(root, "built"));
  writeFileSync(join(root, "built", "styles.css"), "body {}\n");
  mkdirSync(join(root, "linked"));
  symlinkSync(join(root, "built", "styles.css"), join(root, "linked", "styles.css"));
  const s = await start([join(root, "linked"), "--hot"]);
  servers.push(s);
  assert.equal((await get(s.port, "/api/hot/styles", "POST")).status, 200);
  assert.match((await get(s.port, "/styles.css")).body, /--cadpole-hot/);
  assert.equal(readFileSync(join(root, "built", "styles.css"), "utf8"), "body {}\n", "the rewrite reached the linked file");
});
