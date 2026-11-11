// A minimal MCP client over a child process's stdio (JSON-RPC 2.0,
// newline-delimited), enough to drive `cadpole mcp --listener`.
//
// Why the listener talks to Cadpole this way rather than over HTTP: it then holds
// no token at all. The bearer token is read by the child (`cadpole mcp` finds the
// run's listener-token file by itself), so the only thing in this process is a
// pipe to a program whose privileges the *server* has already decided. There
// is no second HTTP client, no second place to leak a credential, and no way
// for this process to call an endpoint the child does not expose.
//
// Deliberately small: initialize, tools/list, tools/call, and notifications are
// dropped on the floor. Anything more belongs in a real MCP library, and this
// file existing at all is a consequence of not taking an npm dependency for
// nine hundred bytes of JSON-RPC.

import { spawn } from "node:child_process";
import { createInterface } from "node:readline";

export class McpError extends Error {
  /** True when the child process could not be started at all (ENOENT, EACCES). */
  spawnFailed = false;
}

export class McpStdioClient {
  /**
   * @param {{command: string, args: string[], env?: Record<string,string>, onLog?: (line: string) => void}} opts
   */
  constructor(opts) {
    this.opts = opts;
    /** @type {import("node:child_process").ChildProcessWithoutNullStreams | null} */
    this.child = null;
    this.nextId = 1;
    /** @type {Map<number, {resolve: (v: any) => void, reject: (e: Error) => void}>} */
    this.pending = new Map();
    this.closed = false;
    /** Readline interfaces over the child's pipes, closed by `stop`. */
    this.readers = /** @type {import("node:readline").Interface[]} */ ([]);
    /** Resolves when the child has actually gone. */
    this.exited = /** @type {Promise<void> | null} */ (null);
  }

  /** Spawn the child and complete the MCP handshake. Returns `serverInfo`. */
  async start() {
    const env = { ...process.env, ...(this.opts.env ?? {}) };
    this.child = /** @type {any} */ (spawn(this.opts.command, this.opts.args, { env, stdio: ["pipe", "pipe", "pipe"] }));
    const child = /** @type {any} */ (this.child);
    /** @param {Error} err */
    const fail = (err) => {
      this.closed = true;
      for (const [, p] of this.pending) p.reject(err);
      this.pending.clear();
    };
    // A child that never started emits `error` and no `exit`; one that did
    // emits `exit`. Either way it is gone.
    this.exited = new Promise((done) => {
      child.once("exit", () => done());
      child.once("error", () => done());
    });
    child.on("exit", (/** @type {number | null} */ code) => {
      fail(new McpError(`the cadpole mcp child exited with code ${code}`));
    });
    // Without a listener, a failed spawn (ENOENT, EACCES) is thrown as an
    // uncaught exception that no caller's `catch` can see.
    child.on("error", (/** @type {NodeJS.ErrnoException} */ err) => {
      const why = err.code === "ENOENT" ? "not found" : err.message;
      const e = new McpError(`cannot run ${this.opts.command}: ${why}`);
      e.spawnFailed = true;
      fail(e);
    });
    // Writing to a child that has died raises EPIPE on the pipe; the `exit` /
    // `error` handlers above already say what happened.
    child.stdin.on("error", () => {});
    this.readers = [
      createInterface({ input: child.stderr }).on("line", (line) => this.opts.onLog?.(`[cadpole mcp] ${line}`)),
      createInterface({ input: child.stdout }).on("line", (line) => this.#onLine(line)),
    ];

    const init = await this.request("initialize", {
      protocolVersion: "2025-06-18",
      capabilities: {},
      clientInfo: { name: "cadpole-listener", version: "0.1.0" },
    });
    this.notify("notifications/initialized", {});
    return init;
  }

  /** @param {string} line */
  #onLine(line) {
    if (!line.trim()) return;
    let msg;
    try {
      msg = JSON.parse(line);
    } catch {
      this.opts.onLog?.(`[cadpole mcp] unparseable line: ${line.slice(0, 200)}`);
      return;
    }
    // A notification (the channel pump) has no id and nothing is waiting.
    if (msg.id === undefined || msg.id === null) return;
    const waiter = this.pending.get(msg.id);
    if (!waiter) return;
    this.pending.delete(msg.id);
    if (msg.error) waiter.reject(new McpError(`${msg.error.message ?? "mcp error"} (${msg.error.code ?? "?"})`));
    else waiter.resolve(msg.result);
  }

  /**
   * @param {string} method
   * @param {unknown} params
   * @returns {Promise<any>}
   */
  request(method, params) {
    if (this.closed || !this.child) return Promise.reject(new McpError("the cadpole mcp child is not running"));
    const id = this.nextId++;
    const p = new Promise((resolve, reject) => this.pending.set(id, { resolve, reject }));
    /** @type {any} */ (this.child).stdin.write(`${JSON.stringify({ jsonrpc: "2.0", id, method, params })}\n`);
    return p;
  }

  /** @param {string} method @param {unknown} params */
  notify(method, params) {
    if (this.closed || !this.child) return;
    /** @type {any} */ (this.child).stdin.write(`${JSON.stringify({ jsonrpc: "2.0", method, params })}\n`);
  }

  /** The tool names this child publishes. */
  async toolNames() {
    const r = await this.request("tools/list", {});
    return (r.tools ?? []).map((/** @type {any} */ t) => String(t.name));
  }

  /**
   * Call one tool and return its first text block parsed as JSON.
   *
   * MCP hands results back as content blocks; every Cadpole tool puts its JSON
   * response in the first text block, with an image block after it when there
   * is a picture. `isError` is a refusal from the server -- including a 403
   * from the capability check -- and is raised, not returned.
   *
   * @param {string} name @param {Record<string, unknown>} args
   */
  async callTool(name, args) {
    const r = await this.request("tools/call", { name, arguments: args });
    const text = (r.content ?? []).find((/** @type {any} */ c) => c.type === "text")?.text ?? "";
    if (r.isError) throw new McpError(`${name}: ${text}`);
    try {
      return JSON.parse(text);
    } catch {
      return { text };
    }
  }

  /**
   * Shut the child down and wait for it to go.
   *
   * Awaiting matters. A readline interface over a live pipe is an open handle,
   * and so is a child that has been signalled but not reaped, so a `stop` that
   * only fires SIGTERM leaves an event loop that never drains: under Node's
   * test runner the process hangs after the last assertion passes, and in a
   * long-running listener it leaks one `cadpole mcp` per reconnect.
   *
   * SIGTERM first, so the child can finish writing; SIGKILL if it is still
   * there after the grace period, because a wedged child must not wedge us.
   *
   * @param {number} graceMs
   */
  async stop(graceMs = 2000) {
    this.closed = true;
    const child = /** @type {any} */ (this.child);
    for (const r of this.readers) r.close();
    this.readers = [];
    if (!child) return;
    child.stdin.end();
    if (child.exitCode !== null || child.signalCode !== null) return;
    child.kill("SIGTERM");
    let timer;
    const forced = new Promise((done) => {
      timer = setTimeout(() => {
        child.kill("SIGKILL");
        done(undefined);
      }, graceMs);
      // An unref'd timer does not itself hold the loop open.
      /** @type {any} */ (timer).unref?.();
    });
    await Promise.race([this.exited ?? Promise.resolve(), forced]);
    clearTimeout(timer);
    await this.exited;
  }
}
