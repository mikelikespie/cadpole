// The model seam, against a stand-in executable that speaks the headless
// streaming protocol of the installed Claude Code (`fake_headless_cli.mjs`).
//
//   bazel test //src/tools/listener:driver_test
//
// Pinned here: the exact argv the listener starts a conversation with, the
// exact line it writes per message, how it reads the output stream (text as
// it is generated, tool use, results, errors, an exit in the middle of a
// turn, with and without partial output), the version
// check, and the refusal of a session that reports any tool or MCP server
// beyond the Cadpole ones. That the real program honours each flag is checked
// by hand against a real installation (docs/assistant-integration.md); the
// server's token is what holds if it does not.

import { strict as assert } from "node:assert";
import { chmodSync, existsSync, mkdtempSync, readFileSync, writeFileSync } from "node:fs";
import { tmpdir } from "node:os";
import test from "node:test";
import { fileURLToPath } from "node:url";
import { dirname, join, resolve } from "node:path";

import { MIN_CLAUDE_VERSION, checkClaude, claudeArgs, claudeDriver, initViolations, parseVersion, userLine } from "./driver.mjs";
import { SYSTEM_PROMPT } from "./policy.mjs";

const here = dirname(fileURLToPath(import.meta.url));

/**
 * A directory holding executables for one test: the stand-in for the model's
 * program and a stand-in `cadpole` whose state (the replies posted) the test
 * reads back.
 *
 * @param {Record<string, string>} env set for the stand-in
 */
function setup(env = {}) {
  const dir = mkdtempSync(join(process.env.TEST_TMPDIR ?? tmpdir(), "listener-driver-"));
  const sh = (/** @type {string} */ name, /** @type {string} */ script, /** @type {Record<string, string>} */ vars) => {
    const path = join(dir, name);
    const exports = Object.entries(vars)
      .map(([k, v]) => `export ${k}=${JSON.stringify(v)}\n`)
      .join("");
    writeFileSync(path, `#!/bin/sh\n${exports}exec ${JSON.stringify(process.execPath)} ${JSON.stringify(resolve(here, script))} "$@"\n`);
    chmodSync(path, 0o755);
    return path;
  };
  const log = join(dir, "cli.log");
  return {
    dir,
    log,
    cli: sh("headless-cli", "fake_headless_cli.mjs", { FAKE_CLI_LOG: log, ...env }),
    cadpole: sh("cadpole", "fake_cadpole_mcp.mjs", { CADPOLE_FAKE_STATE: join(dir, "state.json") }),
    calls: () => (existsSync(join(dir, "state.json")) ? (JSON.parse(readFileSync(join(dir, "state.json"), "utf8")).calls ?? []) : []),
    logged: () =>
      readFileSync(log, "utf8")
        .split("\n")
        .filter(Boolean)
        .map((l) => JSON.parse(l)),
  };
}

const TOOLS = ["get_session", "edit_script", "reply"];

/** @param {ReturnType<typeof setup>} s @param {Record<string, unknown>} extra */
function driverFor(s, extra = {}) {
  /** @type {string[]} */
  const logs = [];
  const driver = claudeDriver({
    server: "http://127.0.0.1:7331",
    cadpoleBin: s.cadpole,
    claudeBin: s.cli,
    tools: TOOLS,
    maxTurns: 5,
    log: (l) => logs.push(l),
    ...extra,
  });
  return { driver, logs };
}

/** A sink that records what a turn wrote and the activity lines it showed. */
function recorder() {
  const r = {
    text: "",
    /** @type {string[]} */
    pieces: [],
    /** @type {string[]} */
    activity: [],
    sink: {
      text: (/** @type {string} */ piece) => {
        r.text += piece;
        r.pieces.push(piece);
      },
      // Consecutive repeats are one line on the screen.
      activity: (/** @type {string} */ what) => {
        if (r.activity.at(-1) !== what) r.activity.push(what);
      },
    },
  };
  return r;
}

test("the argv of a conversation is exactly this", () => {
  const args = claudeArgs({ server: "http://127.0.0.1:7331", cadpoleBin: "/opt/cadpole", tools: TOOLS, maxTurns: 5, env: { XDG_RUNTIME_DIR: "/run/user/1000" } });
  assert.deepEqual(args, [
    "-p",
    "--input-format",
    "stream-json",
    "--output-format",
    "stream-json",
    "--verbose",
    "--include-partial-messages",
    "--mcp-config",
    JSON.stringify({
      mcpServers: {
        cadpole: {
          command: "/opt/cadpole",
          args: ["mcp", "--server", "http://127.0.0.1:7331", "--listener"],
          env: { XDG_RUNTIME_DIR: "/run/user/1000" },
        },
      },
    }),
    "--strict-mcp-config",
    "--tools",
    "",
    "--allowedTools",
    "mcp__cadpole__get_session,mcp__cadpole__edit_script",
    "--disallowedTools",
    "mcp__cadpole__reply",
    "--permission-mode",
    "dontAsk",
    "--permission-prompts",
    "none",
    "--setting-sources",
    "",
    "--disable-slash-commands",
    "--no-session-persistence",
    "--system-prompt",
    SYSTEM_PROMPT,
    "--max-turns",
    "5",
  ]);
  const withModel = claudeArgs({ server: "s", cadpoleBin: "c", tools: [], maxTurns: 1, model: "some-model", env: {} });
  assert.deepEqual(withModel.slice(-2), ["--model", "some-model"]);
  assert.doesNotMatch(withModel.join(" "), /"env"/, "no env block when there is nothing to pass");
  // Nothing that could carry a credential: the MCP child finds its own token.
  assert.doesNotMatch(args.join(" "), /token|api[-_]?key|bearer/i);
});

test("one message in is one JSON line; the text comes back as it is written, with what each tool call is doing", async () => {
  const s = setup();
  const { driver, logs } = driverFor(s);
  const seen = recorder();
  const said = await driver.ask("first", seen.sink);
  assert.deepEqual(said, ["Looking.", "heard message 1 of this conversation"], "the conversation's text, not a subagent's");
  assert.equal(seen.text, "Looking.\n\nheard message 1 of this conversation", "each block is a paragraph, and nothing is written twice");
  assert.ok(seen.pieces.length > 4, `the text arrived in pieces: ${JSON.stringify(seen.pieces)}`);
  assert.deepEqual(seen.activity, ["", "reading the model…", "thinking…", ""], "writing, the tool call, its result, writing again");
  assert.deepEqual(s.calls().filter((c) => c === "get_session"), ["get_session"], "the Cadpole tool was really called");

  const logged = s.logged();
  assert.deepEqual(logged[0].argv, claudeArgs({ server: "http://127.0.0.1:7331", cadpoleBin: s.cadpole, tools: TOOLS, maxTurns: 5 }));
  assert.equal(logged[1].stdin + "\n", userLine("first"));
  assert.deepEqual(JSON.parse(logged[1].stdin), { type: "user", message: { role: "user", content: "first" }, parent_tool_use_id: null });
  assert.match(logged[0].cwd, /cadpole-listener-/, "it runs in a directory of its own");
  assert.ok(logs.some((l) => /2 model turn\(s\), about \$0\.0100/.test(l)), logs.join("\n"));
  await driver.stop();
  assert.ok(!existsSync(logged[0].cwd), "and the directory is removed with it");
});

test("without partial output the same text arrives a block at a time", async () => {
  const s = setup();
  const { driver } = driverFor(s);
  const seen = recorder();
  await driver.ask("[whole] first", seen.sink);
  assert.equal(seen.text, "Looking.\n\nheard message 1 of this conversation");
  assert.deepEqual(seen.pieces, ["Looking.", "\n\n", "heard message 1 of this conversation"]);
  assert.deepEqual(seen.activity, ["", "reading the model…", "thinking…", ""], "the tool call is still shown");
  await driver.stop();
});

test("an activity line is a few plain words, never a tool name or its input", async () => {
  const { activityFor } = await import("./reply_stream.mjs");
  const { EXPECTED_TOOLS } = await import("./policy.mjs");
  for (const tool of [...EXPECTED_TOOLS, "something_new", ""]) {
    const line = activityFor(`mcp__cadpole__${tool}`);
    assert.match(line, /^[a-zA-Z ]{4,40}…$/, `${tool}: ${line}`);
    assert.doesNotMatch(line, /mcp|__|[{}_]/, `${tool}: ${line}`);
  }
  assert.equal(activityFor("mcp__cadpole__edit_script"), "editing the script…");
  assert.equal(activityFor("mcp__cadpole__render"), "rendering…");
  assert.equal(activityFor("mcp__cadpole__evaluate"), "checking the model…");
  assert.equal(activityFor("mcp__cadpole__something_new"), "working…");
});

test("later messages go to the same process, which remembers", async () => {
  const s = setup();
  const { driver } = driverFor(s);
  await driver.ask("one");
  assert.deepEqual(await driver.ask("two"), ["Looking.", "heard message 2 of this conversation"]);
  assert.equal(s.logged().filter((l) => l.argv).length, 1, "one process for both");
  await driver.stop();
});

test("a new conversation is a new process: on reset, and after a number of messages", async () => {
  const s = setup();
  const { driver, logs } = driverFor(s, { newConversationAfter: 2 });
  await driver.ask("one");
  await driver.reset?.();
  assert.deepEqual((await driver.ask("two")).at(-1), "heard message 1 of this conversation");
  await driver.ask("three");
  assert.deepEqual((await driver.ask("four")).at(-1), "heard message 1 of this conversation", "the third process");
  assert.equal(s.logged().filter((l) => l.argv).length, 3);
  assert.ok(logs.some((l) => /new conversation after 2 messages/.test(l)));
  await driver.stop();
});

test("a turn that ends in an error is raised with its reason", async () => {
  const s = setup();
  const { driver } = driverFor(s);
  await assert.rejects(driver.ask("[error]"), /error_during_execution.*boom/);
  await assert.rejects(driver.ask("[auth]"), /the turn failed \(success\): Not logged in/);
  await assert.rejects(driver.ask("[max-turns]"), /limit of 5 model turns/);
  assert.deepEqual((await driver.ask("fine")).at(-1), "heard message 4 of this conversation", "the conversation survives a failed turn");
  await driver.stop();
});

test("a tool outside the allow-list is denied and the denial is logged", async () => {
  const s = setup();
  const { driver, logs } = driverFor(s);
  assert.deepEqual(await driver.ask("[shell] run ls"), ["I have no shell here."]);
  assert.ok(logs.some((l) => /denied by the tool policy: Bash/.test(l)), logs.join("\n"));
  await driver.stop();
});

test("a process that exits in the middle of a turn fails that turn; the next starts a new one", async () => {
  const s = setup();
  const { driver } = driverFor(s);
  await driver.ask("one");
  const seen = recorder();
  await assert.rejects(driver.ask("[crash]", seen.sink), /exited \(code 7\) in the middle of the turn: fatal: out of memory/);
  assert.equal(seen.text, "starting on ", "the text written before the exit was passed on");
  assert.deepEqual((await driver.ask("again")).at(-1), "heard message 1 of this conversation");
  assert.equal(s.logged().filter((l) => l.argv).length, 2);
  await driver.stop();
});

test("a session that reports a tool beyond the Cadpole ones is killed and refused", async () => {
  for (const [env, what] of [
    [{ FAKE_CLI_EXTRA_TOOL: "Bash" }, /tools other than the Cadpole ones: Bash/],
    [{ FAKE_CLI_EXTRA_TOOL: "mcp__cadpole__set_script" }, /tools other than the Cadpole ones: mcp__cadpole__set_script/],
    [{ FAKE_CLI_EXTRA_SERVER: "mail" }, /MCP servers other than exactly `cadpole`: cadpole, mail/],
  ]) {
    const s = setup(/** @type {Record<string, string>} */ (env));
    const { driver } = driverFor(s);
    await assert.rejects(driver.ask("hello"), (err) => {
      assert.match(String(err), /refusing to run/);
      assert.match(String(err), /** @type {RegExp} */ (what));
      return true;
    });
    assert.deepEqual(s.calls(), [], "nothing the session did was let through");
    // Final: it is not started again for the next message.
    await assert.rejects(driver.ask("again"), /refusing to run/);
    assert.equal(s.logged().filter((l) => l.argv).length, 1);
    await driver.stop();
  }
});

test("what an init line may report", () => {
  const ok = { tools: ["mcp__cadpole__reply"], mcp_servers: [{ name: "cadpole", status: "connected" }], plugins: [{ name: "x", path: "builtin" }], skills: [], slash_commands: [] };
  assert.deepEqual(initViolations(ok, TOOLS), []);
  assert.equal(initViolations({ ...ok, tools: undefined }, TOOLS).length, 1, "no tool list is not an empty one");
  assert.equal(initViolations({ ...ok, mcp_servers: [] }, TOOLS).length, 1);
  assert.equal(initViolations({ ...ok, plugins: [{ name: "p", path: "/somewhere" }] }, TOOLS).length, 1);
  assert.equal(initViolations({ ...ok, skills: ["deploy"] }, TOOLS).length, 1);
  assert.equal(initViolations({ ...ok, slash_commands: ["review"] }, TOOLS).length, 1);
});

test("the version check: missing, too old, new enough", () => {
  assert.deepEqual(parseVersion("2.1.317 (Claude Code)"), [2, 1, 317]);
  assert.equal(parseVersion("nonsense"), null);
  assert.throws(() => checkClaude("/nonexistent/headless-cli"), /cannot run .*--version.*--claude PATH.*--driver echo/s);
  const old = setup({ FAKE_CLI_VERSION: "2.1.258 (Claude Code)" });
  assert.throws(() => checkClaude(old.cli), new RegExp(`is Claude Code 2\\.1\\.258; the listener needs ${MIN_CLAUDE_VERSION.replaceAll(".", "\\.")} or later`));
  assert.equal(checkClaude(setup({ FAKE_CLI_VERSION: `${MIN_CLAUDE_VERSION} (Claude Code)` }).cli), MIN_CLAUDE_VERSION);
  assert.equal(checkClaude(setup({ FAKE_CLI_VERSION: "3.0.0 (Claude Code)" }).cli), "3.0.0");
  assert.throws(() => checkClaude(setup({ FAKE_CLI_VERSION: "something else" }).cli), /printed no version number/);
});
