// The entry point's own edges: a cadpole binary that is not there, a Claude
// Code that is missing or too old, and --help.
//
//   bazel test //src/tools/listener:cli_test

import { strict as assert } from "node:assert";
import { spawnSync } from "node:child_process";
import { chmodSync, mkdtempSync, writeFileSync } from "node:fs";
import { tmpdir } from "node:os";
import test from "node:test";
import { fileURLToPath } from "node:url";
import { dirname, join, resolve } from "node:path";

import { McpError, McpStdioClient } from "./mcp_client.mjs";

const here = dirname(fileURLToPath(import.meta.url));
const main = resolve(here, "main.mjs");
const missing = "/nonexistent/cadpole";

/** @param {string[]} args */
const run = (args) =>
  spawnSync(process.execPath, [main, ...args], {
    encoding: "utf8",
    env: { ...process.env, CADPOLE_BIN: missing },
  });

test("start() rejects with a named error when the binary cannot be run", async () => {
  const client = new McpStdioClient({ command: missing, args: ["mcp"] });
  await assert.rejects(client.start(), (err) => {
    assert.ok(err instanceof McpError, String(err));
    assert.match(err.message, /cannot run \/nonexistent\/cadpole/);
    assert.match(err.message, /ENOENT|not found/);
    return true;
  });
  await client.stop();
});

test("a missing cadpole binary is a one-line fatal error, not a Node stack", () => {
  const r = run(["--cadpole", missing, "--driver", "echo"]);
  assert.equal(r.status, 1, r.stderr);
  assert.match(r.stderr, /cadpole-listener: fatal: cannot run \/nonexistent\/cadpole/);
  assert.match(r.stderr, /--cadpole/, "says how to point it at the binary");
  assert.doesNotMatch(r.stderr, /Unhandled 'error' event|node:events/);
});

test("--help prints usage and attaches to nothing", () => {
  for (const flag of ["--help", "-h"]) {
    const r = run([flag]);
    assert.equal(r.status, 0, r.stderr);
    assert.match(r.stdout, /usage: cadpole-listener/);
    assert.match(r.stdout, /--driver claude\|echo/);
    assert.match(r.stdout, /--claude PATH/);
    assert.doesNotMatch(r.stderr, /attaching|ENOENT/);
  }
});

test("a missing or too-old Claude Code is refused before anything is attached", () => {
  const gone = run(["--claude", "/nonexistent/headless-cli"]);
  assert.equal(gone.status, 1, gone.stderr);
  assert.match(gone.stderr, /cadpole-listener: fatal: cannot run `\/nonexistent\/headless-cli --version`/);
  assert.match(gone.stderr, /--claude PATH/);
  assert.match(gone.stderr, /--driver echo/);
  assert.doesNotMatch(gone.stderr, /attaching/);

  const dir = mkdtempSync(join(process.env.TEST_TMPDIR ?? tmpdir(), "listener-cli-"));
  const old = join(dir, "headless-cli");
  writeFileSync(
    old,
    `#!/bin/sh\nFAKE_CLI_VERSION="2.0.9 (Claude Code)" exec ${JSON.stringify(process.execPath)} ${JSON.stringify(resolve(here, "fake_headless_cli.mjs"))} "$@"\n`,
  );
  chmodSync(old, 0o755);
  const r = run(["--claude", old]);
  assert.equal(r.status, 1, r.stderr);
  assert.match(r.stderr, /is Claude Code 2\.0\.9; the listener needs \d+\.\d+\.\d+ or later/);
  assert.doesNotMatch(r.stderr, /attaching/);
});

test("an unknown driver is refused by name", () => {
  const r = run(["--driver", "other"]);
  assert.equal(r.status, 1);
  assert.match(r.stderr, /--driver is claude or echo, not other/);
});
