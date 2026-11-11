// A stand-in for the Claude Code executable, for `driver_test.mjs` and
// `cli_test.mjs`: the part of its headless streaming protocol the listener
// uses, and nothing else.
//
//   <this> --version                      prints FAKE_CLI_VERSION
//   <this> -p --input-format stream-json --output-format stream-json ...
//
// In streaming mode it reads one JSON user message per stdin line and answers
// each with the lines the real program writes: a `system/init` naming the
// session's tools and MCP servers, `stream_event` lines for output as it is
// generated (with `--include-partial-messages`), `assistant` and `user`
// messages, and one `result`. It connects to the MCP server its
// `--mcp-config` names and calls that server's `get_session` tool, so a turn
// here goes through the configuration a turn goes through. Markers in the
// message text select the other endings:
//
//   [error]      a result with subtype error_during_execution
//   [auth]       a "success" result with is_error set (not signed in)
//   [max-turns]  a result with subtype error_max_turns
//   [shell]      the model asks for a tool it was not given; it is denied
//   [crash]      some text, then the process exits in the middle of the turn
//   [whole]      no `stream_event` lines for this turn, as without the flag
//   [slow]       a pause in the middle of the text, for a test that looks
//
// FAKE_CLI_LOG names a file that receives one JSON line per start (`argv`,
// `cwd`) and per message (`stdin`), for the test to read back.
// FAKE_CLI_EXTRA_TOOL / FAKE_CLI_EXTRA_SERVER add an entry to what `init`
// reports, as an installation that ignored a restriction would.
//
// FAKE_CLI_CONTROL names a loopback port where a test decides each step
// instead (spec/replay): this program reports every message it is given and
// then does only what it is told, one JSON line at a time:
//
//   -> {"hello": "cli", "pid": N}        once, on start
//   -> {"turn": "<the message text>"}    a message arrived on stdin
//   <- {"init": {"extraTool": "Bash"}}   write the turn's `system/init` line
//   <- {"end": "answered"}               finish the turn: "answered", "error",
//                                        "auth" or "max-turns"
//   <- {"exit": 7}                       exit now, in a turn or between turns

import { appendFileSync } from "node:fs";
import { connect } from "node:net";
import { createInterface } from "node:readline";

import { McpStdioClient } from "./mcp_client.mjs";

const argv = process.argv.slice(2);
if (argv.includes("--version")) {
  process.stdout.write(`${process.env.FAKE_CLI_VERSION ?? "2.1.300 (Claude Code)"}\n`);
  process.exit(0);
}

const record = (/** @type {Record<string, unknown>} */ o) => {
  if (process.env.FAKE_CLI_LOG) appendFileSync(process.env.FAKE_CLI_LOG, `${JSON.stringify(o)}\n`);
};
record({ argv, cwd: process.cwd() });

const value = (/** @type {string} */ name) => {
  const i = argv.indexOf(name);
  return i >= 0 ? argv[i + 1] : undefined;
};
const allowed = (value("--allowedTools") ?? "").split(",").filter(Boolean);
const server = JSON.parse(value("--mcp-config") ?? "{}").mcpServers?.cadpole;
const sessionId = "00000000-0000-4000-8000-000000000001";
const write = (/** @type {Record<string, unknown>} */ o) => process.stdout.write(`${JSON.stringify({ ...o, session_id: sessionId })}\n`);

/** @type {McpStdioClient | null} */
let mcp = null;
let turn = 0;
let cost = 0;

/**
 * The turn's first line: what the session can reach.
 *
 * @param {{extraTool?: string, extraServer?: string}} extra
 */
function init(extra) {
  write({
    type: "system",
    subtype: "init",
    tools: [...allowed, ...(extra.extraTool ? [extra.extraTool] : [])],
    mcp_servers: [{ name: "cadpole", status: "connected" }, ...(extra.extraServer ? [{ name: extra.extraServer, status: "connected" }] : [])],
    plugins: [{ name: "built-in", path: "builtin" }],
    skills: [],
    slash_commands: [],
    permissionMode: value("--permission-mode"),
  });
}

/** @param {string} text */
async function answer(text) {
  turn += 1;
  init({ extraTool: process.env.FAKE_CLI_EXTRA_TOOL, extraServer: process.env.FAKE_CLI_EXTRA_SERVER });
  await finish(text);
}

/**
 * The rest of a turn, after its `init` line.
 *
 * @param {string} text
 */
async function finish(text) {
  const result = (/** @type {Record<string, unknown>} */ o) => {
    cost += 0.01;
    write({ type: "result", is_error: false, num_turns: 1, total_cost_usd: cost, permission_denials: [], ...o });
  };
  const partial = argv.includes("--include-partial-messages") && !text.includes("[whole]");
  const event = (/** @type {Record<string, unknown>} */ e) => {
    if (partial) write({ type: "stream_event", parent_tool_use_id: null, event: e });
  };
  const pause = (/** @type {number} */ ms) => new Promise((r) => setTimeout(r, ms));
  /** A text block: pieces as they are generated, then the block whole. */
  const say = async (/** @type {string} */ t, /** @type {{stopAfter?: number}} */ o = {}) => {
    event({ type: "message_start", message: { role: "assistant", content: [] } });
    event({ type: "content_block_start", index: 0, content_block: { type: "text", text: "" } });
    const pieces = t.match(/(?:\S+\s*){1,2}/g) ?? [t];
    for (const [i, piece] of pieces.entries()) {
      if (o.stopAfter !== undefined && i >= o.stopAfter) return;
      event({ type: "content_block_delta", index: 0, delta: { type: "text_delta", text: piece } });
      if (text.includes("[slow]")) await pause(150);
    }
    write({ type: "assistant", parent_tool_use_id: null, message: { role: "assistant", content: [{ type: "text", text: t }] } });
    event({ type: "content_block_stop", index: 0 });
    event({ type: "message_stop" });
  };
  if (text.includes("[error]")) return result({ subtype: "error_during_execution", is_error: true, errors: ["boom"] });
  if (text.includes("[auth]")) return result({ subtype: "success", is_error: true, result: "Not logged in · Please run /login" });
  if (text.includes("[max-turns]")) return result({ subtype: "error_max_turns", is_error: true, errors: [] });
  if (text.includes("[crash]")) {
    await say("starting on it now", { stopAfter: 1 });
    process.stderr.write("fatal: out of memory\n");
    // Let the listener read what was written before the exit is seen.
    await pause(50);
    process.exit(7);
  }
  if (text.includes("[shell]")) {
    await say("I have no shell here.");
    return result({
      subtype: "success",
      result: "I have no shell here.",
      permission_denials: [{ tool_name: "Bash", tool_use_id: "t0", tool_input: { command: "ls" } }],
    });
  }
  // An ordinary turn: a sentence, one Cadpole tool call, another sentence.
  if (!mcp) {
    mcp = new McpStdioClient({ command: server.command, args: server.args, env: server.env });
    await mcp.start();
  }
  await say("Looking.");
  event({ type: "message_start", message: { role: "assistant", content: [] } });
  event({ type: "content_block_start", index: 0, content_block: { type: "tool_use", id: `t${turn}`, name: "mcp__cadpole__get_session", input: {} } });
  event({ type: "content_block_delta", index: 0, delta: { type: "input_json_delta", partial_json: "{}" } });
  write({
    type: "assistant",
    parent_tool_use_id: null,
    message: { role: "assistant", content: [{ type: "tool_use", id: `t${turn}`, name: "mcp__cadpole__get_session", input: {} }] },
  });
  event({ type: "content_block_stop", index: 0 });
  event({ type: "message_stop" });
  const out = await mcp.callTool("get_session", {});
  write({
    type: "user",
    parent_tool_use_id: null,
    message: { role: "user", content: [{ type: "tool_result", tool_use_id: `t${turn}`, content: JSON.stringify(out) }] },
  });
  // What a subagent says is not the conversation's answer.
  write({ type: "stream_event", parent_tool_use_id: "t-sub", event: { type: "content_block_delta", index: 0, delta: { type: "text_delta", text: "aside" } } });
  write({ type: "assistant", parent_tool_use_id: "t-sub", message: { role: "assistant", content: [{ type: "text", text: "aside" }] } });
  const reply = `heard message ${turn} of this conversation`;
  await say(reply);
  result({ subtype: "success", result: reply, num_turns: 2 });
}

// One message at a time, in the order they arrive.
let chain = Promise.resolve();
const lines = createInterface({ input: process.stdin });
if (process.env.FAKE_CLI_CONTROL) {
  // Told what to do, step by step (see the header).
  const control = connect({ host: "127.0.0.1", port: Number(process.env.FAKE_CLI_CONTROL) });
  const tell = (/** @type {Record<string, unknown>} */ o) => control.write(`${JSON.stringify(o)}\n`);
  const leave = async (/** @type {number} */ code) => {
    await mcp?.stop();
    process.exit(code);
  };
  // Written at once, not on `connect`: what is written before the connection
  // is up is sent in order, and a message must not overtake this line.
  tell({ hello: "cli", pid: process.pid });
  control.on("error", () => process.exit(3));
  // The test has gone: nothing is left to take orders from.
  control.on("close", () => process.exit(3));
  /** The messages waiting for their turn's orders. */
  const waiting = /** @type {string[]} */ ([]);
  lines.on("line", (line) => {
    if (!line.trim()) return;
    const text = String(JSON.parse(line)?.message?.content ?? "");
    waiting.push(text);
    tell({ turn: text });
  });
  createInterface({ input: control }).on("line", (line) => {
    const order = JSON.parse(line);
    if (order.exit !== undefined) {
      void leave(Number(order.exit));
    } else if (order.init) {
      turn += 1;
      init(order.init);
    } else if (order.end) {
      const text = waiting.shift() ?? "";
      const marker = { answered: "", error: "[error]", auth: "[auth]", "max-turns": "[max-turns]" }[String(order.end)] ?? "";
      // The marker decides the ending, as it does without a controller.
      chain = chain.then(() => finish(`${text.replaceAll("[", "(")} ${marker}`));
    }
  });
  lines.on("close", () => void leave(0));
} else {
  lines.on("line", (line) => {
    if (!line.trim()) return;
    record({ stdin: line });
    const msg = JSON.parse(line);
    chain = chain.then(() => answer(String(msg?.message?.content ?? "")));
  });
  lines.on("close", () => {
    chain = chain.then(async () => {
      await mcp?.stop();
      process.exit(0);
    });
  });
}
