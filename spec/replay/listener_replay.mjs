// Replays traces of spec/listener.qnt against the limited listener.
//
//   bazel test //spec/replay:listener_replay_test
//   bazel run  //spec/replay:listener_replay -- --traces=1000 --steps=40
//
// The model generates the traces; this program makes the real listener take
// the same steps and compares what it shows with the model's state after
// every one. The direction is the model's because every action of the model
// is something a test can do to the listener from outside, and the listener
// then runs until it blocks, which is what an action of the model is.
//
// What is real: the shipped entry point (`main.mjs`, so the version check,
// the event loop, the driver and the shutdown), as a process of its own.
// What stands in, each told what to do one step at a time over a loopback
// socket: the server (`fake_cadpole_mcp.mjs` passes every tool call here,
// and this program answers a long poll only when the model's poll returns)
// and the headless session (`fake_headless_cli.mjs` reports each message it
// is given and writes its report, its result, or exits when told).
//
// After every step:
//
//   * where the listener is blocked: in a poll (and the cursor it asked
//     from), on the session, or exited (and its exit code);
//   * the cursor the server holds for it;
//   * every line it or a session posted to Chat, in order;
//   * how many session processes were started, whether one is alive, and
//     every message written to one, in order.

import { spawn } from "node:child_process";
import { chmodSync, existsSync, mkdirSync, mkdtempSync, writeFileSync } from "node:fs";
import { createServer } from "node:net";
import { tmpdir } from "node:os";
import { join } from "node:path";
import { createInterface } from "node:readline";

import { generate, inParallel, inTree, knob, same } from "./traces.mjs";

const MAIN = inTree("src/tools/listener/main.mjs");
const FAKE_CLI = inTree("src/tools/listener/fake_headless_cli.mjs");
const FAKE_MCP = inTree("src/tools/listener/fake_cadpole_mcp.mjs");
const MODEL = inTree("spec/listener.qnt");

/** How long a step may take before the replay gives up on it. */
const PATIENCE_MS = 30_000;

/** The model's endings, as orders to the stand-in session. */
const ENDINGS = { answered: "answered", "not signed in": "auth", error: "error", "turn limit": "max-turns" };

/** A line of the Chat panel, as the model's `Line`. */
function lineOf(text, inFlight) {
  if (text.startsWith("Started a new conversation")) return { tag: "Fresh" };
  const failed = /^The listener could not finish this: (.*)$/.exec(text);
  if (!failed) return { tag: "Answer", value: inFlight };
  const why = failed[1];
  const reason = why.startsWith("refusing to run")
    ? "refused"
    : /Not logged in/.test(why)
      ? "not signed in"
      : /reached the limit/.test(why)
        ? "turn limit"
        : /exited \(/.test(why)
          ? "exited"
          : /error_during_execution/.test(why)
            ? "error"
            : `unrecognised: ${why}`;
  return { tag: "Notice", value: { reason } };
}

/** One listener under test, with the server and the sessions it talks to. */
class Rig {
  constructor(dir) {
    this.dir = dir;
    // The server.
    this.log = [];
    this.listenOn = false;
    this.cursors = /** @type {Record<string, number>} */ ({});
    this.chat = [];
    /** Replies written a piece at a time, by the seq the rig gave them. */
    this.streams = new Map();
    // The listener.
    this.proc = null;
    this.exitCode = /** @type {number | null} */ (null);
    this.stderr = [];
    /** The long poll the listener is blocked in, if it is. */
    this.poll = null;
    this.mcpConnections = 0;
    // The sessions, in the order they started.
    this.clis = [];
    this.writes = [];
    this.waiters = new Set();
    this.sockets = new Set();
  }

  async open() {
    this.server = createServer((sock) => this.accept(sock));
    await new Promise((r) => this.server.listen(0, "127.0.0.1", () => r(undefined)));
    this.port = this.server.address().port;
    const sh = (name, script) => {
      const path = join(this.dir, name);
      writeFileSync(path, `#!/bin/sh\nexec ${JSON.stringify(process.execPath)} ${JSON.stringify(script)} "$@"\n`);
      chmodSync(path, 0o755);
      return path;
    };
    this.cadpole = sh("cadpole", FAKE_MCP);
    this.claude = sh("headless-cli", FAKE_CLI);
  }

  accept(sock) {
    this.sockets.add(sock);
    /** What this connection turned out to be. */
    let cli = null;
    sock.on("error", () => {});
    sock.on("close", () => {
      this.sockets.delete(sock);
      if (cli) cli.connected = false;
      if (this.poll?.sock === sock) this.poll = null;
      this.poke();
    });
    // A process killed while writing resets its socket; that is its close.
    const lines = createInterface({ input: sock });
    lines.on("error", () => {});
    lines.on("line", (line) => {
      const msg = JSON.parse(line);
      if (msg.hello === "cli") {
        cli = { pid: msg.pid, sock, connected: true, pending: 0 };
        this.clis.push(cli);
      } else if (msg.hello === "mcp") {
        this.mcpConnections += 1;
      } else if (cli && msg.turn !== undefined) {
        const seq = Number(/seq="(\d+)"/.exec(msg.turn)?.[1] ?? -1);
        cli.pending += 1;
        cli.seq = seq;
        this.writes.push({ child: this.clis.indexOf(cli) + 1, seq });
      } else if (msg.call) {
        this.call(sock, msg);
      }
      this.poke();
    });
  }

  /** One tool call, answered as the server answers it. */
  call(sock, msg) {
    const answer = (result) => sock.write(`${JSON.stringify({ id: msg.id, result })}\n`);
    const last = this.log.length;
    switch (msg.call) {
      case "get_session":
        return answer({ id: "replay", revision: 1, last_event_seq: last, assistant_listening: this.listenOn });
      case "ack": {
        const client = String(msg.args.client ?? "");
        const asked = Number(msg.args.cursor ?? 0);
        const held = Math.max(this.cursors[client] ?? 0, Math.min(asked, last));
        this.cursors[client] = held;
        return answer({ client, cursor: held, last_event_seq: last, epoch_reset: asked > last });
      }
      case "wait_events":
        // Held until the model's poll returns.
        this.poll = { sock, id: msg.id, after: Number(msg.args.after ?? 0), client: String(msg.args.client ?? "") };
        return undefined;
      case "reply": {
        const inFlight = this.clis.findLast((c) => c.pending > 0)?.seq ?? -1;
        const line = lineOf(String(msg.args.text ?? ""), inFlight);
        this.chat.push(line);
        return answer({ seq: this.chat.length });
      }
      case "stream_reply": {
        // A reply written a piece at a time is one line in Chat, counted
        // when it ends, for the turn that was in flight when it started.
        let seq = Number(msg.args.seq ?? 0);
        if (seq === 0) {
          seq = 1000 + this.streams.size;
          this.streams.set(seq, { text: "", inFlight: this.clis.findLast((c) => c.pending > 0)?.seq ?? -1 });
        }
        const m = this.streams.get(seq);
        if (!m || m.ended) return sock.write(`${JSON.stringify({ id: msg.id, error: `message ${seq} is not in progress` })}\n`);
        m.text += String(msg.args.append ?? "");
        const finish = String(msg.args.finish ?? "");
        if (finish) {
          m.ended = true;
          const failure = /\n?(The listener could not finish this: .*)$/.exec(m.text);
          this.chat.push(finish === "REPLY_FINISH_FAILED" && failure ? lineOf(failure[1], m.inFlight) : { tag: "Answer", value: m.inFlight });
        }
        return answer({ seq, state: finish ? "done" : "streaming", length: m.text.length, throttled: false });
      }
      default:
        return answer({});
    }
  }

  poke() {
    for (const w of [...this.waiters]) w();
  }

  /** Resolves when `cond` holds; it is looked at again whenever anything arrives. */
  until(cond, what) {
    return new Promise((resolve, reject) => {
      const timer = setTimeout(() => {
        this.waiters.delete(check);
        reject(new Error(`gave up waiting for ${what}\n${this.describe()}`));
      }, PATIENCE_MS);
      const check = () => {
        if (!cond()) return;
        clearTimeout(timer);
        this.waiters.delete(check);
        resolve(undefined);
      };
      this.waiters.add(check);
      check();
    });
  }

  describe() {
    return [
      `  listener: ${this.proc ? (this.exitCode === null ? "running" : `exited ${this.exitCode}${this.signal ? ` (${this.signal})` : ""}`) : "not started"}`,
      `  poll held: ${this.poll ? `after ${this.poll.after}` : "none"}`,
      `  sessions: ${JSON.stringify(this.clis.map((c) => ({ connected: c.connected, pending: c.pending })))}`,
      `  writes: ${JSON.stringify(this.writes)}`,
      `  chat: ${JSON.stringify(this.chat)}`,
      `  cursors: ${JSON.stringify(this.cursors)}`,
      `  stderr:\n    ${this.stderr.slice(-12).join("\n    ")}`,
    ].join("\n");
  }

  // ---- what the model's actions do

  launch(s) {
    const binary = s.binary.tag;
    this.exitCode = null;
    this.signal = null;
    // A poll of the process before this one is not this one's: its socket
    // can close after the process has exited, so it may still be here.
    this.poll = null;
    this.proc = spawn(
      process.execPath,
      [
        MAIN,
        "--server",
        "http://127.0.0.1:1",
        "--cadpole",
        this.cadpole,
        "--claude",
        binary === "Missing" ? join(this.dir, "no-such-program") : this.claude,
        "--new-conversation-after",
        String(s.every),
        "--poll-ms",
        "600000",
        ...(s.alwaysListen ? ["--always-listen"] : []),
      ],
      {
        env: {
          ...process.env,
          CADPOLE_FAKE_CONTROL: String(this.port),
          FAKE_CLI_CONTROL: String(this.port),
          FAKE_CLI_VERSION: binary === "TooOld" ? "2.1.100 (Claude Code)" : "2.1.300 (Claude Code)",
        },
        stdio: ["ignore", "ignore", "pipe"],
      },
    );
    createInterface({ input: this.proc.stderr }).on("line", (l) => this.stderr.push(l));
    this.proc.on("exit", (code, signal) => {
      this.exitCode = code ?? (signal ? -1 : 0);
      this.signal = signal;
      this.poke();
    });
  }

  post(kind) {
    const seq = this.log.length + 1;
    const ev = { seq, at: "t", by: "ui" };
    if (kind === "request") Object.assign(ev, { kind: "message", data: { from: "user", text: `request ${seq}` } });
    else if (kind === "new") Object.assign(ev, { kind: "message", data: { from: "user", text: " /new " } });
    else if (kind === "listen-on" || kind === "listen-off") {
      const on = kind === "listen-on";
      Object.assign(ev, { kind: "assistant_listening", data: { listening: on, was: this.listenOn } });
      this.listenOn = on;
    } else {
      // Everything that is not worth a turn, in rotation.
      const noise = [
        { kind: "params_changed", data: { overrides: { w: seq } } },
        { kind: "selection_changed", data: { selected: [{ feature_id: "a", kind: "face" }], note: "" } },
        { kind: "message", data: { from: "assistant", text: "a reply" } },
        { kind: "client_failure", data: { message: "the page failed" } },
        { kind: "script_changed", data: { path: "main.star", revision: seq } },
        { kind: "solve_finished", data: { job: "j" } },
      ];
      Object.assign(ev, noise[seq % noise.length]);
    }
    this.log.push(ev);
  }

  pollReturns() {
    const { sock, id, after } = this.poll;
    this.poll = null;
    const events = this.log.filter((e) => e.seq > after);
    const cursor = events.length ? events[events.length - 1].seq : after;
    sock.write(`${JSON.stringify({ id, result: { events, cursor } })}\n`);
  }

  session() {
    return this.clis[this.clis.length - 1];
  }

  order(o) {
    this.session().sock.write(`${JSON.stringify(o)}\n`);
  }

  /** The listener has run until it blocked where the model says it does. */
  async settled(s) {
    const where = s.proc.tag;
    if (where === "Waiting") await this.until(() => this.poll !== null, "the listener's next long poll");
    else if (where === "InTurn") {
      await this.until(() => this.writes.length === s.writes.length && this.session()?.pending > 0, "a message written to a session");
    } else await this.until(() => this.exitCode !== null, "the listener to exit");
    await this.until(() => Boolean(this.session()?.connected) === s.child.alive, `the session process to be ${s.child.alive ? "running" : "gone"}`);
  }

  /** What the implementation shows against the model's state. */
  compare(s, where) {
    same(this.cursors["listener"] ?? 0, s.stored, `${where}: the server's cursor`);
    same(
      this.chat,
      s.chat.map((l) => (l.tag === "Notice" ? { tag: "Notice", value: { reason: l.value.reason } } : l.tag === "Fresh" ? { tag: "Fresh" } : l)),
      `${where}: the Chat lines`,
    );
    same(this.clis.length, s.children, `${where}: session processes started`);
    same(
      this.writes,
      s.writes.map((w) => ({ child: w.child, seq: w.seq })),
      `${where}: messages written to sessions`,
    );
    same(Boolean(this.session()?.connected), s.child.alive, `${where}: a session process is alive`);
    if (s.proc.tag === "Waiting") same(this.poll?.after, s.cursor, `${where}: the cursor the listener polls from`);
    if (s.proc.tag === "InTurn") same(this.session().seq, s.flight.seq, `${where}: the message in flight`);
    if (s.proc.tag === "Down") same(this.exitCode !== null || this.proc === null, true, `${where}: the listener is not running`);
  }

  async close() {
    this.proc?.kill("SIGKILL");
    for (const sock of this.sockets) sock.destroy();
    await new Promise((r) => this.server.close(() => r(undefined)));
  }
}

/** The process has been reaped by its parent: nothing of it is left in /proc. */
async function gone(pid) {
  const until = Date.now() + PATIENCE_MS;
  while (existsSync(`/proc/${pid}`)) {
    if (Date.now() > until) throw new Error(`process ${pid} is still there`);
    await new Promise((r) => setTimeout(r, 2));
  }
  // And its parent has run the callbacks that follow.
  await new Promise((r) => setTimeout(r, 20));
}

/** Make the listener take one trace's steps; throws at the first difference. */
async function replay(trace, dir) {
  const rig = new Rig(dir);
  await rig.open();
  rig.listenOn = trace[0].s.listenOn;
  let steps = 0;
  try {
    for (let i = 1; i < trace.length; i++) {
      const before = trace[i - 1].s;
      const { did, s } = trace[i];
      const where = `step ${i} (${did.name}${did.kind ? ` ${did.kind}` : ""}${did.ending ? ` ${did.ending}` : ""}${did.ok ? "" : " bad"})`;
      try {
        switch (did.name) {
          case "userDoes":
            rig.post(did.kind);
            break;
          case "launch": {
            const connections = rig.mcpConnections;
            rig.launch(s);
            if (s.proc.tag === "Down") {
              await rig.until(() => rig.exitCode !== null, "the listener to refuse to start");
              same(rig.exitCode, 1, `${where}: the exit code of a start the version check refused`);
              same(rig.mcpConnections, connections, `${where}: attached to the server`);
            } else await rig.settled(s);
            break;
          }
          case "pollReturns":
            rig.pollReturns();
            await rig.settled(s);
            break;
          case "sessionReports":
            // Alternately a tool and an MCP server that should not be there.
            rig.order({ init: did.ok ? {} : i % 2 ? { extraTool: "Bash" } : { extraServer: "elsewhere" } });
            if (!did.ok) await rig.settled(s);
            break;
          case "turnEnds":
            rig.order({ end: ENDINGS[did.ending] });
            await rig.until(() => rig.chat.length === s.chat.length, "the turn's line in Chat");
            rig.session().pending -= 1;
            await rig.settled(s);
            break;
          case "sessionExits": {
            const session = rig.session();
            rig.order({ exit: 7 });
            if (before.flight.open) {
              await rig.until(() => !session.connected, "the session to exit");
              session.pending = 0;
              await rig.settled(s);
            } else await gone(session.pid);
            break;
          }
          case "stop":
            rig.proc.kill("SIGINT");
            await rig.until(() => rig.exitCode !== null, "the listener to stop");
            same(rig.exitCode, 0, `${where}: the exit code of a stopped listener`);
            rig.poll = null;
            for (const c of rig.clis) c.pending = 0;
            await rig.settled(s);
            break;
          default:
            throw new Error(`the model took an action this replay does not know: ${did.name}`);
        }
        if (did.name === "sessionReports" && !did.ok) rig.session().pending = 0;
        rig.compare(s, where);
        steps += 1;
      } catch (err) {
        throw new Error(`${where}: ${err instanceof Error ? err.message : err}\n${rig.describe()}`);
      }
    }
  } finally {
    await rig.close();
  }
  return steps;
}

const traces = Number(knob("TRACES", 48));
/** `--only=N` replays trace N alone, to look at one failure. */
const only = knob("ONLY", "");
const steps = Number(knob("STEPS", 30));
const seed = knob("SEED", "0x5eed");
const width = Number(knob("WIDTH", 4));
const tmp = mkdtempSync(join(process.env.TEST_TMPDIR ?? tmpdir(), "listener-replay-"));

const started = Date.now();
// Most traces with a usable Claude Code, where the lifecycle is; the rest
// from the model's own `init`, which also draws one that is missing or old.
const sets = [
  generate({ model: MODEL, init: "initUsable", traces: Math.max(1, Math.round(traces * 0.85)), steps, seed, dir: tmp }),
  generate({ model: MODEL, init: "init", traces: Math.max(1, traces - Math.round(traces * 0.85)), steps, seed, dir: tmp }),
];
const all = sets.flat();
const generated = Date.now();

let done = 0;
const taken = /** @type {Record<string, number>} */ ({});
let failure = null;
await inParallel(all, width, async (trace, index) => {
  if (failure || (only !== "" && index !== Number(only))) return;
  const dir = join(tmp, `trace-${index}`);
  mkdirSync(dir);
  try {
    const steps = await replay(trace, dir);
    done += steps;
    for (const st of trace.slice(1)) {
      const d = st.did;
      const name = d.name === "turnEnds" ? `turnEnds ${d.ending}` : d.name === "sessionReports" ? `sessionReports ${d.ok ? "good" : "bad"}` : d.name === "userDoes" ? `userDoes ${d.kind}` : d.name;
      taken[name] = (taken[name] ?? 0) + 1;
    }
  } catch (err) {
    failure = new Error(`trace ${index} of seed ${seed} (--traces=${traces} --steps=${steps}):\n${err instanceof Error ? err.message : err}`);
  }
});
if (failure) {
  console.error(`listener replay: FAILED\n${failure.message}`);
  process.exit(1);
}

console.log(
  `listener replay: ${all.length} traces, ${done} steps, every one matched ` +
    `(${((generated - started) / 1000).toFixed(1)} s to generate, ${((Date.now() - generated) / 1000).toFixed(1)} s to replay, seed ${seed})`,
);
console.log(
  Object.entries(taken)
    .sort()
    .map(([name, n]) => `  ${String(n).padStart(5)}  ${name}`)
    .join("\n"),
);
// A replay that never reaches a turn's end, a refusal or a stop checks
// little; a change to the model that starves one of these is reported.
for (const needed of ["launch", "pollReturns", "sessionReports good", "sessionReports bad", "turnEnds answered", "sessionExits", "stop", "userDoes new"]) {
  if (!taken[needed]) {
    console.error(`listener replay: no trace took the action "${needed}"; raise --traces or look at the weights in listener.qnt's step`);
    process.exit(1);
  }
}
