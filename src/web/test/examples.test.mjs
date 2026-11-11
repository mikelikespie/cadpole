// Every bundled example evaluates on the real engine.
//
// The gallery's scripts are the first thing a new user loads, and in session
// mode `loadExample` writes the one they picked into the document as
// main.star -- so an example that only the mock understands replaces a working
// script with one that errors on its first line. They were written for the
// mock once (`param(description=)`, `sketch(plane="XY")`, `circle(d=)`), and
// nothing noticed, because the only test that loaded one ran against the
// mock. This is what stops it happening again: each source goes into a
// scratch document and through `cadpole eval` -- the real parser, the real
// builtins, the real kernel -- and has to come out clean. The one script that
// is *supposed* to fail has to fail for exactly the reasons it says.
//
// `//src/web:examples_test`. The `cadpole` binary comes from runfiles.

import assert from "node:assert/strict";
import { spawnSync } from "node:child_process";
import { existsSync, mkdtempSync, writeFileSync } from "node:fs";
import { tmpdir } from "node:os";
import { join, resolve } from "node:path";
import { test } from "node:test";
import { EXAMPLES } from "../src/examples.js";

// The cwd is src/web (runfiles under Bazel, the source tree outside it).
const cadpole = [resolve("../rust/cadpole-cli/cadpole"), resolve("../../bazel-bin/src/rust/cadpole-cli/cadpole")].find((p) => existsSync(p));
assert.ok(cadpole, "cannot find the cadpole binary; `bazel build //src/rust/cadpole-cli:cadpole`");

/** `cadpole eval` on a script, as the document's main.star. */
function evaluate(source) {
  const dir = mkdtempSync(join(process.env["TEST_TMPDIR"] ?? tmpdir(), "cadpole-example-"));
  const main = join(dir, "main.star");
  writeFileSync(main, source);
  const r = spawnSync(cadpole, ["eval", main], { encoding: "utf8", timeout: 60_000 });
  if (r.error) throw r.error;
  return { status: r.status, out: `${r.stdout}${r.stderr}` };
}

/** The diagnostics `cadpole eval` printed: `error: ...` and `warning: ...` lines. */
const diagnostics = (out) => out.split("\n").filter((line) => /^(error|warning):/.test(line));

for (const ex of EXAMPLES) {
  if (ex.id === "diagnostics") continue;
  test(`the "${ex.name}" example evaluates on the real engine with no diagnostics`, () => {
    const { status, out } = evaluate(ex.source);
    assert.deepEqual(diagnostics(out), [], out);
    assert.equal(status, 0, out);
    // It also has to be the model it claims: a plate, and a boss on it only
    // when it asked for one.
    assert.match(out, /extrude\s+base\b/, out);
    const wantsBoss = /^boss_h = param\("boss_h", (\d+)/m.exec(ex.source);
    assert.ok(wantsBoss, "the example declares boss_h");
    const bossBodies = (out.match(/^\s+body (boss|post)#/gm) ?? []).length;
    assert.equal(bossBodies, Number(wantsBoss[1]) > 0 ? 1 : 0, out);
  });
}

test('the "Diagnostics demo" fails for the reasons it demonstrates, and no others', () => {
  const ex = EXAMPLES.find((e) => e.id === "diagnostics");
  assert.ok(ex);
  const { status, out } = evaluate(ex.source);
  assert.notEqual(status, 0, "the demo calls fail(), so evaluation has to fail");
  const diags = diagnostics(out);
  assert.equal(diags.length, 2, out);
  assert.match(diags[0], /^warning: main\.star:\d+:\d+: .*boss diameter 50 does not fit/, out);
  assert.match(diags[1], /^error: main\.star:\d+:\d+: .*demonstrate error diagnostics/, out);
});
