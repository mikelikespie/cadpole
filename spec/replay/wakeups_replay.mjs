// Replays traces of spec/wakeups.qnt against the three readers.
//
//   bazel test //spec/replay:wakeups_replay_test
//   bazel run  //spec/replay:wakeups_replay -- --traces=400 --steps=40
//
// The model generates the traces (its `stepReplay`); this program makes real
// processes take the same steps and compares what they show with the model's
// state after every one. Everything is the shipped program:
//
//   * `cadpole serve`, restarted on the same address when the model's server
//     restarts;
//   * the user, through the owner's token: PostMessage, SetSelection with
//     and without a note, Reply, SetParams, the Listen toggle;
//   * `cadpole listen --next` / `--follow` as a process, read by its stdout
//     and its exit;
//   * `cadpole mcp` as a process, read by the channel notifications on its
//     stdout;
//   * the listener's loop (`Listener`) over a real `cadpole mcp --listener`,
//     with a driver that records the turn it is asked for and ends it when
//     the model does.
//
// The direction is the model's because the readers' steps can be forced from
// outside: the watcher and the channel poll by themselves as soon as an
// event releases their long poll, which is what `stepReplay` assumes, and
// the listener's loop is stepped here. After every step, for each reader:
// whether it is running, every event it has handed over (in order), the
// cursor the server holds for it, and for the listener its own cursor and
// the turn in flight.
//
// Two things here are not in the model. To know that a freshly started
// channel has attached, the test posts parameter changes until the server
// holds a cursor for it; they are events of a kind no reader acts on, and
// sequence numbers are translated between the model's and the server's. And
// a watcher is suspended while the server is replaced, so that it finds the
// new one at the model's step and not half a batch earlier.

import { spawn } from "node:child_process";
import { existsSync, mkdirSync, mkdtempSync, readFileSync } from "node:fs";
import { request as httpRequest } from "node:http";
import { createServer } from "node:net";
import { tmpdir } from "node:os";
import { join } from "node:path";
import { createInterface } from "node:readline";

import { Listener } from "../../src/tools/listener/listener.mjs";
import { McpStdioClient } from "../../src/tools/listener/mcp_client.mjs";
import { generate, inParallel, inTree, knob } from "./traces.mjs";

const CADPOLE = inTree("src/rust/cadpole-cli/cadpole");
const MODEL = inTree("spec/wakeups.qnt");

/**
 * Actions of the model's `step` that its `stepReplay` leaves out, and so this
 * program: a poll of the watcher or the channel at a moment of the model's
 * choosing (`stepReplay` has them poll as soon as they can, as the processes
 * do), and one of them dying between handing a batch over and storing its
 * cursor, a window of microseconds in a real process. The simulation of
 * `step` covers both.
 */
const NOT_REPLAYED = ["polls", "pollsAndDies"];

/** How long a step may take before the replay gives up on it. */
const PATIENCE_MS = Number(knob("PATIENCE_MS", 30_000));

/** `--verbose=1` prints every step as it is taken. */
const VERBOSE = knob("VERBOSE", "") !== "";

const pause = (ms) => new Promise((r) => setTimeout(r, ms));

/** The server's log lines without the token its banner prints. */
const redacted = (lines) => lines.map((l) => l.replace(/token=[^\s&]+/g, "token=[redacted]")).join("\n");

async function freePort() {
  const probe = createServer();
  await new Promise((r) => probe.listen(0, "127.0.0.1", () => r(undefined)));
  const port = probe.address().port;
  await new Promise((r) => probe.close(() => r(undefined)));
  return port;
}

/** One session with its readers. */
class World {
  constructor(dir, free) {
    this.dir = dir;
    /** --always-listen, per reader. */
    this.free = free;
    this.env = { ...process.env, CADPOLE_RUNTIME_DIR: dir, CADPOLE_TOKEN: "", CADPOLE_LISTENER_TOKEN: "" };
    this.gen = 0;
    /** The server's sequence number of each of the model's events, per generation. */
    this.reals = [[]];
    this.token = "";
    this.unique = 0;
    // What each reader has handed over: [generation, the model's seq].
    this.handed = { watcher: [], channel: [], listener: [] };
    this.watcher = null;
    this.channel = null;
    this.listener = null;
    this.serverLog = [];
  }

  // ---- the server

  async startServer() {
    this.port ??= await freePort();
    // `--agent-tcp`: the readers and the user here all reach the server over
    // its TCP port, with the agent token, as a client on another machine
    // would (transport.qnt covers the socket).
    const child = spawn(CADPOLE, ["serve", "--port", String(this.port), "--agent-tcp", "--web-dir", join(this.dir, "no-web")], {
      env: this.env,
      stdio: ["ignore", "ignore", "pipe"],
    });
    this.base = await new Promise((resolve, reject) => {
      const timer = setTimeout(() => reject(new Error(`cadpole serve did not start:\n${redacted(this.serverLog)}`)), PATIENCE_MS);
      createInterface({ input: child.stderr }).on("line", (line) => {
        this.serverLog.push(line);
        const m = /listening on (http:\/\/[^/]+)\//.exec(line);
        if (m) {
          clearTimeout(timer);
          resolve(m[1]);
        }
      });
      child.on("exit", (code) => reject(new Error(`cadpole serve exited with ${code}:\n${redacted(this.serverLog)}`)));
    });
    this.server = child;
    // The tokens are this run's own.
    const file = join(this.dir, "session-token");
    const before = this.token;
    const until = Date.now() + PATIENCE_MS;
    for (;;) {
      const now = existsSync(file) ? readFileSync(file, "utf8").trim() : "";
      if (now && now !== before) {
        this.token = now;
        break;
      }
      if (Date.now() > until) throw new Error("cadpole serve wrote no new session token");
      await pause(5);
    }
    this.gen += 1;
    this.reals[this.gen] = [];
  }

  /** This generation's events, as the server numbers them. */
  get real() {
    return this.reals[this.gen];
  }

  async stopServer() {
    const gone = new Promise((r) => this.server.once("exit", () => r(undefined)));
    this.server.kill("SIGTERM");
    await gone;
  }

  /**
   * One request on a connection of its own. `fetch` keeps connections open
   * and would send the first request after a restart down a socket the old
   * server closed (ECONNRESET).
   */
  request(method, path, body) {
    const payload = JSON.stringify(body);
    const url = new URL(`${this.base}${path}`);
    return new Promise((resolve, reject) => {
      const req = httpRequest(
        {
          host: url.hostname,
          port: url.port,
          path: url.pathname,
          method,
          agent: false,
          headers: {
            "content-type": "application/json",
            "content-length": Buffer.byteLength(payload),
            authorization: `Bearer ${this.token}`,
            "x-cadpole-client": "ui",
          },
        },
        (res) => {
          const chunks = [];
          res.on("data", (c) => chunks.push(c));
          res.on("end", () => {
            const text = Buffer.concat(chunks).toString("utf8");
            if (res.statusCode !== 200) reject(new Error(`${method} ${path}: ${res.statusCode} ${text}`));
            else resolve(JSON.parse(text));
          });
        },
      );
      req.on("error", (err) => reject(new Error(`${method} ${path}: ${err.message}\n${redacted(this.serverLog.slice(-6))}`)));
      req.end(payload);
    });
  }

  op(name, body) {
    return this.request("POST", `/api/ops/${name}`, body);
  }

  async lastSeq() {
    return Number((await this.op("GetSession", {})).last_event_seq ?? 0);
  }

  /** The cursor the server holds under a client id. */
  async held(client) {
    return Number((await this.op("Ack", { client, cursor: 0 })).cursor ?? 0);
  }

  /**
   * One thing done in the UI; returns the sequence number of its event. An
   * event of the model's is recorded as its next one.
   */
  async does(kind, ofTheModel = true) {
    const before = await this.lastSeq();
    const n = ++this.unique;
    if (kind === "request") await this.op("PostMessage", { text: `request ${n}` });
    else if (kind === "note") await this.op("SetSelection", { selected: [], note: `note ${n}` });
    else if (kind === "reply") await this.op("Reply", { text: `reply ${n}` });
    else if (kind === "click") await this.op("SetSelection", { selected: [{ feature_id: `f${n}`, kind: "face" }], note: "" });
    else if (kind === "other") await this.op("SetParams", { overrides: { w: { number: 40 + n } } });
    else await this.request("PUT", "/api/session/assistant", { listening: kind === "listen-on" });
    const after = await this.lastSeq();
    if (after !== before + 1) throw new Error(`${kind} added ${after - before} events to the log, not one`);
    if (ofTheModel) this.real.push(after);
    return after;
  }

  /** A server cursor in the model's numbering: how many of its events are at or below it. */
  toModel(cursor) {
    return this.real.filter((r) => r <= cursor).length;
  }

  /**
   * What was handed over, in the model's numbering. Translated when it is
   * compared, not when it arrives: a reader can print an event before the
   * call that posted it has returned its number here.
   */
  inModel(hands) {
    return hands.map(([gen, seq]) => {
      const i = (this.reals[gen] ?? []).indexOf(seq);
      return [gen, i >= 0 ? i + 1 : `an event the model did not post (#${seq})`];
    });
  }

  // ---- the readers

  startWatcher(mode) {
    const child = spawn(CADPOLE, ["listen", "--url", this.base, `--${mode}`, ...(this.free.get("watcher") ? ["--always-listen"] : [])], {
      env: this.env,
      stdio: ["ignore", "pipe", "pipe"],
    });
    const w = { child, up: true, notes: [] };
    createInterface({ input: child.stdout }).on("line", (l) => {
      const line = JSON.parse(l);
      this.handed.watcher.push([this.gen, typeof line.seq === "number" ? line.seq : `a line without a seq: ${l}`]);
    });
    createInterface({ input: child.stderr }).on("line", (l) => w.notes.push(l));
    child.on("exit", () => (w.up = false));
    this.watcher = w;
  }

  async startChannel() {
    const out = join(this.dir, `mcp-out-${this.unique}`);
    const child = spawn(CADPOLE, ["mcp", "--server", this.base, "--out-dir", out, ...(this.free.get("channel") ? ["--always-listen"] : [])], {
      env: this.env,
      stdio: ["pipe", "pipe", "pipe"],
    });
    const c = { child, up: true, gen: this.gen, client: `mcp:${child.pid}`, answered: false };
    child.stdin.on("error", () => {});
    createInterface({ input: child.stdout }).on("line", (l) => {
      const msg = JSON.parse(l);
      if (msg.id === 1) c.answered = true;
      if (msg.method === "notifications/claude/channel") {
        const seq = Number(msg.params?.meta?.seq ?? NaN);
        this.handed.channel.push([this.gen, Number.isNaN(seq) ? `a notification without a seq: ${l}` : seq]);
      }
    });
    child.on("exit", () => (c.up = false));
    this.channel = c;
    const send = (o) => child.stdin.write(`${JSON.stringify(o)}\n`);
    send({ jsonrpc: "2.0", id: 1, method: "initialize", params: { protocolVersion: "2025-06-18", capabilities: {}, clientInfo: { name: "replay", version: "0" } } });
    await this.until(() => c.answered, "cadpole mcp to answer initialize");
    send({ jsonrpc: "2.0", method: "notifications/initialized", params: {} });
    // Attached, and pumping: an event of a kind nobody acts on comes back as
    // the cursor the server holds for this process.
    const until = Date.now() + PATIENCE_MS;
    for (;;) {
      const probe = await this.does("other", false);
      const patience = Date.now() + 1000;
      while (Date.now() < patience && (await this.held(c.client)) < probe) await pause(10);
      if ((await this.held(c.client)) >= probe) break;
      if (Date.now() > until) throw new Error("the channel never acknowledged an event");
    }
  }

  async startListener() {
    const client = new McpStdioClient({ command: CADPOLE, args: ["mcp", "--server", this.base, "--listener", "--out-dir", join(this.dir, "listener-out")], env: this.env });
    await client.start();
    const l = { client, up: true, gen: this.gen, ask: null, stepping: false, failed: null };
    l.loop = new Listener({
      cadpole: client,
      client: "listener",
      alwaysListen: this.free.get("listener"),
      pollMs: 40,
      driver: {
        // A turn: recorded when it is asked for, ended when the model ends it.
        ask: (prompt) =>
          new Promise((resolve) => {
            const seq = Number(/seq="(\d+)"/.exec(prompt)?.[1] ?? NaN);
            this.handed.listener.push([this.gen, seq]);
            l.ask = { resolve, seq };
          }),
        stop: async () => {},
      },
    });
    await l.loop.resume();
    this.listener = l;
  }

  /** One round of the listener's loop, up to where it blocks. */
  async listenerPolls() {
    const l = this.listener;
    l.stepping = true;
    l.loop.step().then(
      () => (l.stepping = false),
      (err) => {
        l.failed = err;
        l.stepping = false;
      },
    );
    await this.until(() => !l.stepping || l.ask !== null, "the listener's loop to block");
    if (l.failed) throw new Error(`the listener's poll failed: ${l.failed.message}`);
  }

  async listenerTurnEnds() {
    const l = this.listener;
    const ask = l.ask;
    l.ask = null;
    ask.resolve([]);
    await this.until(() => !l.stepping || l.ask !== null, "the listener's loop to block");
  }

  async kill(who) {
    if (who === "listener") {
      // The process is gone: its turn never ends and its loop never resumes.
      this.listener.up = false;
      await this.listener.client.stop(200);
      return;
    }
    const r = who === "watcher" ? this.watcher : this.channel;
    if (!r.up) return;
    r.child.kill("SIGKILL");
    await this.until(() => !r.up, `the ${who} to die`);
  }

  async until(cond, what) {
    const until = Date.now() + PATIENCE_MS;
    while (!(await cond())) {
      if (Date.now() > until) throw new Error(`gave up waiting for ${what}`);
      await pause(5);
    }
  }

  // ---- what is compared

  /** What the readers show, in the model's terms. */
  async shown(s) {
    const out = {
      watcherUp: Boolean(this.watcher?.up),
      channelUp: Boolean(this.channel?.up),
      listenerUp: Boolean(this.listener?.up),
      watcher: this.inModel(this.handed.watcher),
      channel: this.inModel(this.handed.channel),
      listener: this.inModel(this.handed.listener),
      heldForListen: this.toModel(await this.held("listen")),
      heldForListener: this.toModel(await this.held("listener")),
    };
    // A reader that is attached to this server: its own cursor too.
    if (this.channel?.up && this.channel.gen === this.gen) out.channelCursor = this.toModel(await this.held(this.channel.client));
    if (this.listener?.up && this.listener.gen === this.gen) {
      out.listenerCursor = this.toModel(this.listener.loop.cursor);
      out.flight = this.listener.ask ? this.inModel([[this.gen, this.listener.ask.seq]])[0][1] : 0;
    }
    return out;
  }

  /** The same, as the model's state says it. */
  said(s) {
    const by = (who) => s.handed.filter((h) => h.who === who).map((h) => [h.gen, h.seq]);
    const r = (who) => s.readers.get(who);
    const out = {
      watcherUp: r("watcher").up,
      channelUp: r("channel").up,
      listenerUp: r("listener").up,
      watcher: by("watcher"),
      channel: by("channel"),
      listener: by("listener"),
      heldForListen: s.stored.get("listen") ?? 0,
      heldForListener: s.stored.get("listener") ?? 0,
    };
    if (r("channel").up && r("channel").gen === s.gen) out.channelCursor = r("channel").cursor;
    if (r("listener").up && r("listener").gen === s.gen) {
      out.listenerCursor = r("listener").cursor;
      out.flight = r("listener").flight;
    }
    return out;
  }

  /** Wait until the readers show what the model says; fail with both when they never do. */
  async agrees(s, where) {
    const want = JSON.stringify(this.said(s));
    const until = Date.now() + PATIENCE_MS;
    let got = "";
    for (;;) {
      got = JSON.stringify(await this.shown(s));
      if (got === want) return;
      if (Date.now() > until) break;
      await pause(10);
    }
    throw new Error(`${where}: the readers never showed what the model says\n  shown: ${got}\n  model: ${want}`);
  }

  async close() {
    this.watcher?.child.kill("SIGKILL");
    this.channel?.child.kill("SIGKILL");
    await this.listener?.client.stop(100).catch(() => {});
    this.server?.kill("SIGKILL");
  }
}

/** Make the readers take one trace's steps; throws at the first difference. */
async function replay(trace, dir) {
  const world = new World(dir, trace[0].s.alwaysListen);
  let steps = 0;
  try {
    await world.startServer();
    for (let i = 1; i < trace.length; i++) {
      const { did, s } = trace[i];
      const where = `step ${i} (${[did.name, did.who, did.kind, did.mode].filter(Boolean).join(" ")})`;
      if (VERBOSE) console.error(`${dir.split("/").pop()} ${where}`);
      try {
        switch (did.name) {
          case "uiDoes":
            await world.does(did.kind);
            break;
          case "starts":
            if (did.who === "watcher") world.startWatcher(did.mode);
            else if (did.who === "channel") await world.startChannel();
            else await world.startListener();
            break;
          case "kills":
            await world.kill(did.who);
            break;
          case "listenerPolls":
            await world.listenerPolls();
            break;
          case "listenerTurnEnds":
            await world.listenerTurnEnds();
            break;
          case "listenerPollFails": {
            // The token is the old run's: the call is refused.
            const refused = await world.listener.loop.step().then(
              () => false,
              () => true,
            );
            if (!refused) throw new Error("a listener attached before the restart polled the new server");
            break;
          }
          case "serverRestarts":
            // Suspended, so it meets the new server at the model's step.
            if (world.watcher?.up) world.watcher.child.kill("SIGSTOP");
            await world.stopServer();
            await world.startServer();
            break;
          case "watcherReconnects":
            world.watcher.child.kill("SIGCONT");
            await world.until(() => world.watcher.notes.some((n) => /the server was restarted/.test(n)) || !world.watcher.up, "the watcher to find the restarted server");
            break;
          default:
            throw new Error(
              NOT_REPLAYED.includes(did.name)
                ? `${did.name} is not an action of stepReplay; the traces were generated with another step`
                : `the model took an action this replay does not know: ${did.name}`,
            );
        }
        await world.agrees(s, where);
        steps += 1;
      } catch (err) {
        const notes = world.watcher ? `\n  the watcher's notes: ${JSON.stringify(world.watcher.notes)}` : "";
        throw new Error(`${where}: ${err instanceof Error ? err.message : err}${notes}`);
      }
    }
  } finally {
    await world.close();
  }
  return steps;
}

const traces = Number(knob("TRACES", 48));
const steps = Number(knob("STEPS", 30));
const seed = knob("SEED", "0x5eed");
const width = Number(knob("WIDTH", 4));
const tmp = mkdtempSync(join(process.env.TEST_TMPDIR ?? tmpdir(), "wakeups-replay-"));

const started = Date.now();
const all = generate({ model: MODEL, step: "stepReplay", traces, steps, seed, dir: tmp });
const generated = Date.now();

let done = 0;
const taken = /** @type {Record<string, number>} */ ({});
let failure = null;
await inParallel(all, width, async (trace, index) => {
  if (failure) return;
  const dir = join(tmp, `trace-${index}`);
  mkdirSync(dir);
  try {
    const n = await replay(trace, dir);
    done += n;
    for (const st of trace.slice(1)) {
      const d = st.did;
      const name = [d.name, d.who, d.kind, d.mode].filter(Boolean).join(" ");
      taken[name] = (taken[name] ?? 0) + 1;
    }
  } catch (err) {
    failure = new Error(`trace ${index} of seed ${seed} (--traces=${traces} --steps=${steps}):\n${err instanceof Error ? err.message : err}`);
  }
});
if (failure) {
  console.error(`wakeups replay: FAILED\n${failure.message}`);
  process.exit(1);
}

const handed = all.reduce((n, trace) => n + trace[trace.length - 1].s.handed.length, 0);
console.log(
  `wakeups replay: ${all.length} traces, ${done} steps, ${handed} hand-overs, every step matched ` +
    `(${((generated - started) / 1000).toFixed(1)} s to generate, ${((Date.now() - generated) / 1000).toFixed(1)} s to replay, seed ${seed})`,
);
console.log(
  Object.entries(taken)
    .sort()
    .map(([name, n]) => `  ${String(n).padStart(5)}  ${name}`)
    .join("\n"),
);
for (const needed of ["starts watcher next", "starts watcher follow", "starts channel", "starts listener", "listenerTurnEnds listener", "serverRestarts", "watcherReconnects watcher", "uiDoes request", "uiDoes note", "uiDoes listen-on", "uiDoes listen-off"]) {
  if (!taken[needed]) {
    console.error(`wakeups replay: no trace took the action "${needed}"; raise --traces or look at the weights in wakeups.qnt's stepReplay`);
    process.exit(1);
  }
}
if (!handed) {
  console.error("wakeups replay: no trace handed anything over");
  process.exit(1);
}
// The stand-in processes and their pipes would keep the event loop alive.
process.exit(0);
