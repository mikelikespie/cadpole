// The seam between the Cadpole listener and the model.
//
// The listener ships no model client and holds no credential. It runs turns
// through the Claude Code the user already has installed, in its headless
// streaming mode: one `claude -p` process per conversation, a user message
// written to its stdin for each Cadpole event, newline-delimited JSON read
// back from its stdout. The process blocks on stdin between events, so an
// idle listener runs no model turn.
//
// What is tested and what is not. The argv below, the lines written to stdin
// and the way the output stream is read are pinned by `driver_test.mjs`
// against a stand-in executable that speaks the same protocol. That the real
// program honours every flag is not something a hermetic test can show, and
// it does not have to be right for the privilege boundary to hold: the server
// refuses whatever the listener token does not grant, however the model's
// process was configured.
//
// A driver is handed a sink with each message: `text(piece)` for what the
// model writes, as it writes it, and `activity(line)` for what it is doing
// while it is not writing. The listener shows both in the session
// (reply_stream.mjs); the driver does not post the reply itself.

/**
 * @typedef {{text: (piece: string) => void, activity: (what: string) => void}} Sink
 * @typedef {{
 *   ask: (prompt: string, sink?: Sink) => Promise<string[]>,
 *   stop: () => Promise<void>,
 *   reset?: () => Promise<void>,
 * }} AgentDriver
 */

import { spawn, spawnSync } from "node:child_process";
import { mkdtempSync, rmSync } from "node:fs";
import { tmpdir } from "node:os";
import { join } from "node:path";
import { createInterface } from "node:readline";

import { MODEL_TOOLS_WITHHELD, SYSTEM_PROMPT } from "./policy.mjs";
import { THINKING, activityFor } from "./reply_stream.mjs";

/** A sink that drops everything, for a caller that only wants the result. */
const NO_SINK = { text: () => {}, activity: () => {} };

/**
 * The oldest Claude Code the listener runs turns through. It is the newest
 * requirement among the flags in `claudeArgs`: `--permission-prompts none`,
 * which makes a call that would ask a person be denied instead of waiting
 * for an answer nobody is there to give.
 */
export const MIN_CLAUDE_VERSION = "2.1.259";

/**
 * `"2.1.317 (Claude Code)"` to `[2, 1, 317]`; null when there is no version.
 *
 * @param {string} text
 * @returns {number[] | null}
 */
export function parseVersion(text) {
  const m = /(\d+)\.(\d+)\.(\d+)/.exec(String(text));
  return m ? [Number(m[1]), Number(m[2]), Number(m[3])] : null;
}

/** @param {number[]} a @param {number[]} b */
function olderThan(a, b) {
  for (let i = 0; i < 3; i++) {
    if (a[i] !== b[i]) return a[i] < b[i];
  }
  return false;
}

/**
 * Find the installed Claude Code and check it is new enough, without
 * starting a session. Returns its version; throws with what to do.
 *
 * @param {string} bin
 * @returns {string}
 */
export function checkClaude(bin) {
  const r = spawnSync(bin, ["--version"], { encoding: "utf8", timeout: 20_000 });
  if (r.error || r.status !== 0) {
    const why = r.error ? r.error.message : `exit ${r.status}: ${String(r.stderr).trim().split("\n")[0]}`;
    throw new Error(
      `cannot run \`${bin} --version\` (${why}). The listener runs its turns through an installed ` +
        `Claude Code, version ${MIN_CLAUDE_VERSION} or later: install it, or point at it with --claude PATH ` +
        "or CADPOLE_CLAUDE_BIN. `--driver echo` runs the loop with no model.",
    );
  }
  const version = parseVersion(r.stdout);
  if (!version) {
    throw new Error(`\`${bin} --version\` printed no version number (${String(r.stdout).trim().slice(0, 80)}); is it Claude Code?`);
  }
  const min = /** @type {number[]} */ (parseVersion(MIN_CLAUDE_VERSION));
  if (olderThan(version, min)) {
    throw new Error(
      `${bin} is Claude Code ${version.join(".")}; the listener needs ${MIN_CLAUDE_VERSION} or later ` +
        "(`claude update`), or point at a newer one with --claude PATH or CADPOLE_CLAUDE_BIN.",
    );
  }
  return version.join(".");
}

/**
 * The argv for one headless conversation.
 *
 * Every restriction here is defence in depth; the listener token is the
 * boundary. What each flag is for:
 *
 * - `-p --input-format stream-json --output-format stream-json --verbose`:
 *   headless, a JSON line in per message and JSON lines out (the output
 *   format requires `--verbose`);
 * - `--include-partial-messages`: the model's text as it is generated and
 *   the start of each tool call, as `stream_event` lines, which is what lets
 *   the reply grow in the UI instead of arriving whole;
 * - `--mcp-config ... --strict-mcp-config`: the Cadpole MCP child in limited
 *   mode and no other MCP server, whatever is configured on this machine;
 * - `--tools ""`: no built-in tool at all, so no shell, files or network;
 * - `--allowedTools`, `--disallowedTools`: exactly the Cadpole tools the
 *   server published, less `reply`, which is withheld because the model's
 *   own text is the reply;
 * - `--permission-mode dontAsk --permission-prompts none`: anything not
 *   allowed is denied, and nothing waits for a person;
 * - `--setting-sources ""`, `--disable-slash-commands`, `--system-prompt`:
 *   the user's and the project's settings, hooks, instructions files, skills
 *   and commands stay out of this conversation, and the prompt is the
 *   listener's own;
 * - `--no-session-persistence`: the conversation is not added to the user's
 *   session history;
 * - `--max-turns`: a bound per message.
 *
 * No token appears here: `cadpole mcp --listener` reads the restricted token
 * from the runtime directory by itself.
 *
 * @param {{server?: string, cadpoleBin: string, tools: string[], maxTurns: number, model?: string, env?: Record<string, string | undefined>}} opts
 * @returns {string[]}
 */
export function claudeArgs(opts) {
  const env = opts.env ?? process.env;
  /** @type {Record<string, string>} */
  const childEnv = {};
  // Where the MCP child looks for the token `cadpole serve` wrote. Paths,
  // not secrets.
  for (const name of ["CADPOLE_RUNTIME_DIR", "XDG_RUNTIME_DIR"]) {
    const v = env[name];
    if (v) childEnv[name] = v;
  }
  const mcp = {
    mcpServers: {
      cadpole: {
        command: opts.cadpoleBin,
        args: ["mcp", ...(opts.server ? ["--server", opts.server] : []), "--listener"],
        ...(Object.keys(childEnv).length ? { env: childEnv } : {}),
      },
    },
  };
  const args = [
    "-p",
    "--input-format",
    "stream-json",
    "--output-format",
    "stream-json",
    "--verbose",
    "--include-partial-messages",
    "--mcp-config",
    JSON.stringify(mcp),
    "--strict-mcp-config",
    "--tools",
    "",
    "--allowedTools",
    opts.tools
      .filter((t) => !MODEL_TOOLS_WITHHELD.includes(t))
      .map((t) => `mcp__cadpole__${t}`)
      .join(","),
    "--disallowedTools",
    MODEL_TOOLS_WITHHELD.map((t) => `mcp__cadpole__${t}`).join(","),
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
    String(opts.maxTurns),
  ];
  if (opts.model) args.push("--model", opts.model);
  return args;
}

/**
 * The line written to the conversation's stdin for one event.
 *
 * @param {string} prompt
 */
export function userLine(prompt) {
  return JSON.stringify({ type: "user", message: { role: "user", content: prompt }, parent_tool_use_id: null }) + "\n";
}

/**
 * What a conversation's `system/init` line may report, checked before any
 * of its output is trusted: only the Cadpole tools that were allowed, the one
 * MCP server, and nothing loaded from the user's or the project's
 * configuration. Returns what is wrong, or an empty list.
 *
 * The flags in `claudeArgs` ask for exactly this; the check is what notices
 * an installed version that reads one of them differently.
 *
 * A withheld Cadpole tool that is still listed is not a violation: the
 * listener token grants it, and it is only kept from the model to avoid a
 * second copy of the reply.
 *
 * @param {any} init
 * @param {string[]} tools the Cadpole tool names (unprefixed) the server published
 * @returns {string[]}
 */
export function initViolations(init, tools) {
  const allowed = new Set(tools.map((t) => `mcp__cadpole__${t}`));
  /** @type {string[]} */
  const wrong = [];
  const names = (/** @type {any} */ v) => (Array.isArray(v) ? v.map((x) => (typeof x === "string" ? x : String(x?.name ?? "?"))) : []);
  if (!Array.isArray(init?.tools)) wrong.push("it reported no tool list");
  const extraTools = names(init?.tools).filter((t) => !allowed.has(t));
  if (extraTools.length) wrong.push(`tools other than the Cadpole ones: ${extraTools.join(", ")}`);
  const servers = names(init?.mcp_servers);
  if (servers.length !== 1 || servers[0] !== "cadpole") wrong.push(`MCP servers other than exactly \`cadpole\`: ${servers.join(", ") || "none"}`);
  const plugins = (Array.isArray(init?.plugins) ? init.plugins : []).filter((/** @type {any} */ p) => p?.path !== "builtin").map((/** @type {any} */ p) => String(p?.name ?? "?"));
  if (plugins.length) wrong.push(`plugins: ${plugins.join(", ")}`);
  if (names(init?.skills).length) wrong.push(`skills: ${names(init.skills).join(", ")}`);
  if (names(init?.slash_commands).length) wrong.push(`commands: ${names(init.slash_commands).join(", ")}`);
  return wrong;
}

/**
 * The real driver: one long-lived headless conversation with the installed
 * Claude Code.
 *
 * The process is started on the first event and kept, so the model remembers
 * what it just changed. `ask` writes one message and resolves when that
 * message's `result` line arrives; the listener loop awaits each `ask`, so
 * messages are handled in order, one at a time. A conversation is replaced
 * by a fresh process when the old one exits (the turn in flight fails and
 * says so), after `newConversationAfter` messages, or on `reset()`.
 *
 * Fail closed: every turn begins with a `system/init` line listing what the
 * conversation can reach. If that is anything but the allowed Cadpole tools
 * (`initViolations`), the process is killed before its output is read and
 * the driver refuses every later message with the same reason.
 *
 * @param {{server?: string, cadpoleBin: string, claudeBin: string, tools: string[], maxTurns: number,
 *          model?: string, newConversationAfter?: number, log: (s: string) => void}} opts
 * @returns {AgentDriver}
 */
export function claudeDriver(opts) {
  const args = claudeArgs(opts);
  const every = opts.newConversationAfter ?? 0;
  /** @type {{child: import("node:child_process").ChildProcess, cwd: string, turns: number, announced: boolean, exited: Promise<void>, stderr: string[]} | null} */
  let conv = null;
  /** The turn in flight: its `result` line or the child's exit settles it. */
  let pending = /** @type {{resolve: (said: string[]) => void, reject: (e: Error) => void, said: string[], sink: Sink,
                             wrote: string, streamed: boolean, announced: Set<string>} | null} */ (null);

  /**
   * Text of the conversation, a piece or a whole block. Blocks are separate
   * paragraphs: the model writes a sentence, calls a tool, writes another.
   *
   * @param {string} piece @param {boolean} startsBlock
   */
  const write = (piece, startsBlock) => {
    const p = pending;
    if (!p || !piece) return;
    if (startsBlock && p.wrote && !p.wrote.endsWith("\n\n")) {
      const gap = p.wrote.endsWith("\n") ? "\n" : "\n\n";
      p.sink.text(gap);
      p.wrote += gap;
    }
    // Writing is the activity now.
    p.sink.activity("");
    p.sink.text(piece);
    p.wrote = (p.wrote + piece).slice(-4);
  };

  /** @param {Error} err */
  const fail = (err) => {
    const p = pending;
    pending = null;
    p?.reject(err);
  };

  /** Set when a conversation reported tools it must not have; final. */
  let refused = /** @type {Error | null} */ (null);

  /** @param {any} msg */
  const onMessage = (msg) => {
    if (msg?.type === "system" && msg.subtype === "init") {
      const wrong = initViolations(msg, opts.tools);
      if (wrong.length) {
        refused = new Error(
          `refusing to run: the Claude Code session reported ${wrong.join("; ")}. ` +
            "The listener only runs a session whose tools are the Cadpole ones",
        );
        const c = conv;
        conv = null;
        c?.child.kill("SIGKILL");
        fail(refused);
      } else if (conv && !conv.announced) {
        conv.announced = true;
        opts.log(`  session tools: ${msg.tools.length}, all Cadpole (${msg.tools.join(", ")}); MCP servers: cadpole; nothing else loaded`);
      }
      return;
    }
    if (!pending) return;
    if (msg?.type === "stream_event" && !msg.parent_tool_use_id) {
      // The model's output as it is generated: text a piece at a time, and
      // the start of each tool call.
      const e = msg.event ?? {};
      if (e.type === "content_block_start") {
        const block = e.content_block ?? {};
        if (block.type === "text") pending.streamed = false;
        if (block.type === "tool_use") {
          pending.announced.add(String(block.id ?? ""));
          pending.sink.activity(activityFor(block.name));
        }
      } else if (e.type === "content_block_delta" && e.delta?.type === "text_delta" && e.delta.text) {
        write(e.delta.text, !pending.streamed);
        pending.streamed = true;
      }
    } else if (msg?.type === "assistant" && !msg.parent_tool_use_id) {
      // The same output once a block is complete. What already arrived in
      // pieces is not written twice; without pieces this is all there is.
      for (const block of msg.message?.content ?? []) {
        if (block?.type === "text" && block.text) {
          pending.said.push(block.text);
          if (pending.streamed) pending.streamed = false;
          else write(block.text, true);
        } else if (block?.type === "tool_use" && !pending.announced.has(String(block.id ?? ""))) {
          pending.sink.activity(activityFor(block.name));
        }
      }
    } else if (msg?.type === "user" && !msg.parent_tool_use_id) {
      // A tool's result went back to the model, which is now deciding what
      // to do with it.
      pending.sink.activity(THINKING);
    } else if (msg?.type === "result") {
      const denied = Array.isArray(msg.permission_denials) ? msg.permission_denials.map((/** @type {any} */ d) => d?.tool_name).filter(Boolean) : [];
      if (denied.length) opts.log(`  denied by the tool policy: ${denied.join(", ")}`);
      const cost = typeof msg.total_cost_usd === "number" ? `, about $${msg.total_cost_usd.toFixed(4)} so far in this conversation` : "";
      opts.log(`  ${msg.num_turns ?? "?"} model turn(s)${cost}`);
      if (msg.is_error || msg.subtype !== "success") {
        // A failure inside the run (the turn could not be run at all) is a
        // "success" with is_error set and its text in `result`; the rest
        // carry `errors`.
        const why =
          Array.isArray(msg.errors) && msg.errors.length
            ? msg.errors.join("; ")
            : typeof msg.result === "string"
              ? msg.result
              : "";
        const what = msg.subtype === "error_max_turns" ? `it reached the limit of ${opts.maxTurns} model turns for one message` : `${msg.subtype}`;
        fail(new Error(`the turn failed (${what})${why ? `: ${why}` : ""}`));
        return;
      }
      const p = pending;
      pending = null;
      p.resolve(p.said);
    }
  };

  const start = () => {
    // An empty directory of its own: no project files for it to find.
    const cwd = mkdtempSync(join(tmpdir(), "cadpole-listener-"));
    const child = spawn(opts.claudeBin, args, { cwd, stdio: ["pipe", "pipe", "pipe"] });
    /** @type {string[]} */
    const stderr = [];
    const c = {
      child,
      cwd,
      turns: 0,
      announced: false,
      stderr,
      exited: /** @type {Promise<void>} */ (
        new Promise((resolve) => {
          const gone = (/** @type {string} */ how) => {
            if (conv === c) conv = null;
            rmSync(cwd, { recursive: true, force: true });
            const said = stderr.join(" ").trim().slice(-300);
            fail(new Error(`Claude Code ${how} in the middle of the turn${said ? `: ${said}` : ""}`));
            resolve();
          };
          child.on("error", (err) => gone(`could not be run (${err.message})`));
          child.on("exit", (code, signal) => gone(`exited (${signal ?? `code ${code}`})`));
        })
      ),
    };
    child.stderr?.on("data", (d) => {
      stderr.push(String(d));
      if (stderr.length > 20) stderr.shift();
    });
    // A write to a child that has gone is reported by its exit, above.
    child.stdin?.on("error", () => {});
    createInterface({ input: /** @type {import("node:stream").Readable} */ (child.stdout) }).on("line", (line) => {
      if (conv !== c) return;
      let msg;
      try {
        msg = JSON.parse(line);
      } catch {
        return;
      }
      onMessage(msg);
    });
    return c;
  };

  /** @param {NonNullable<typeof conv>} c */
  const end = async (c) => {
    if (conv === c) conv = null;
    c.child.stdin?.end();
    const timer = setTimeout(() => c.child.kill("SIGTERM"), 3000);
    await c.exited;
    clearTimeout(timer);
  };

  return {
    async ask(prompt, sink = NO_SINK) {
      if (refused) throw refused;
      if (conv && every > 0 && conv.turns >= every) {
        opts.log(`starting a new conversation after ${conv.turns} messages`);
        await end(conv);
      }
      if (!conv) conv = start();
      const c = conv;
      c.turns += 1;
      return await new Promise((resolve, reject) => {
        pending = { resolve, reject, said: [], sink, wrote: "", streamed: false, announced: new Set() };
        c.child.stdin?.write(userLine(prompt));
      });
    },
    async reset() {
      if (conv) await end(conv);
    },
    async stop() {
      const c = conv;
      if (!c) return;
      // Not a failure of the turn's making, but nobody will read its result.
      fail(new Error("the listener is stopping"));
      await end(c);
    },
  };
}

/**
 * A driver that runs a fixed script of tool calls instead of a model.
 *
 * This is what makes the listener loop testable without a network: the test
 * hands it a function that receives the prompt, an object that can call
 * Cadpole tools and the sink, and it does whatever a model would have done.
 *
 * @param {{call: (name: string, args: Record<string, unknown>) => Promise<any>,
 *          turn: (prompt: string, cadpole: {call: (name: string, args: Record<string, unknown>) => Promise<any>}, sink: Sink) => Promise<string[]>}} opts
 * @returns {AgentDriver}
 */
export function scriptedDriver(opts) {
  return {
    ask: (prompt, sink = NO_SINK) => opts.turn(prompt, { call: opts.call }, sink),
    stop: async () => {},
  };
}

/**
 * The `echo` driver: answers every event with one short reply saying what it
 * saw, written a few words at a time after a tool call, the way a model's
 * reply arrives. No model and no network beyond the Cadpole server:
 * `--driver echo` checks that events arrive, the Listen gate works and a
 * reply grows and settles in the UI.
 *
 * @param {{call: (name: string, args: Record<string, unknown>) => Promise<any>}} cadpole
 * @param {{pieceMs?: number}} [opts] the pause between pieces (default 200 ms)
 * @returns {AgentDriver}
 */
export function echoDriver(cadpole, opts = {}) {
  const pieceMs = opts.pieceMs ?? 200;
  const pause = () => new Promise((r) => setTimeout(r, pieceMs));
  return scriptedDriver({
    call: cadpole.call,
    async turn(prompt, k, sink) {
      const kind = /kind="([^"]+)"/.exec(prompt)?.[1] ?? "event";
      sink.activity(activityFor("get_session"));
      const session = await k.call("get_session", {});
      await pause();
      const text = `(echo driver, no model) saw a ${kind} at revision ${session.revision ?? "?"}.`;
      sink.activity("");
      // A word or two a piece, slowly enough to be seen arriving.
      for (const piece of text.match(/(?:\S+\s*){1,2}/g) ?? [text]) {
        sink.text(piece);
        await pause();
      }
      return [text];
    },
  });
}
