// Replays traces of spec/transport.qnt against the real programs.
//
//   bazel test //spec/replay:transport_replay_test
//   bazel run  //spec/replay:transport_replay -- --traces=200 --steps=40
//
// The model generates the traces (its `stepReplay`); this program makes the
// shipped `cadpole` take the same steps and compares what it shows with the
// model's state after every one:
//
//   * `start`, `startRefused`, `stop`, `crash`: a real `cadpole serve` of one
//     of two documents, in a runtime directory of this trace's own, started,
//     stopped with SIGTERM or killed with SIGKILL;
//   * `spoilDir`, `restoreDir`: the runtime directory's path becomes a
//     symlink to it (a directory a server and a client must refuse), and back;
//   * `plantForeign`: a live socket with a record anyone could have written
//     beside the real ones;
//   * `send`, `refusedLocally`: a tool call of a long-lived `cadpole mcp` (or
//     `cadpole mcp --listener`) started with no `--server`, from the client's
//     working directory;
//   * `request`: one HTTP request straight at the TCP port or the socket,
//     with one of the run's tokens, an earlier run's, a wrong one or none.
//
// After every step: which servers run; which of their files exist, each
// 0600, in a 0700 directory; what each call and request came to; and that no
// token any server minted appears in any server's output, any record, any
// command line, or anything `cadpole mcp` printed.

import { spawn } from "node:child_process";
import {
  chmodSync,
  existsSync,
  lstatSync,
  mkdirSync,
  mkdtempSync,
  readFileSync,
  readdirSync,
  renameSync,
  rmSync,
  statSync,
  symlinkSync,
  unlinkSync,
  writeFileSync,
} from "node:fs";
import { request as httpRequest } from "node:http";
import { createServer, createConnection } from "node:net";
import { tmpdir } from "node:os";
import { join } from "node:path";
import { createInterface } from "node:readline";

import { generate, inParallel, inTree, knob } from "./traces.mjs";

const CADPOLE = inTree("src/rust/cadpole-cli/cadpole");
const MODEL = inTree("spec/transport.qnt");

/**
 * Actions of the model's `step` that its `stepReplay` leaves out, and so this
 * program: a call delivered at a moment of the model's choosing, and a
 * connection cut between running a call and answering it. A tool call here
 * is one whole call; the simulation of `step` covers both.
 * //spec:names_test checks that this list and the cases below are every
 * action the model has.
 */
const NOT_REPLAYED = ["deliver", "breakAfterRunning"];

const PATIENCE_MS = Number(knob("PATIENCE_MS", 30_000));
const VERBOSE = knob("VERBOSE", "") !== "";

const pause = (ms) => new Promise((r) => setTimeout(r, ms));

function within(ms, what, promise) {
  let timer;
  return Promise.race([
    promise.finally(() => clearTimeout(timer)),
    new Promise((_, bad) => (timer = setTimeout(() => bad(new Error(`timed out: ${what}`)), ms))),
  ]);
}

const modeOf = (p) => lstatSync(p).mode & 0o777;

/** A `cadpole mcp` over stdio, with everything it prints kept. */
class Mcp {
  constructor(cwd, env, listener) {
    this.out = [];
    this.err = [];
    this.waiting = new Map();
    this.next = 1;
    this.child = spawn(CADPOLE, ["mcp", ...(listener ? ["--listener"] : [])], { cwd, env, stdio: ["pipe", "pipe", "pipe"] });
    this.exited = new Promise((r) => this.child.on("exit", (code) => r(code)));
    createInterface({ input: this.child.stderr }).on("line", (l) => this.err.push(l));
    createInterface({ input: this.child.stdout }).on("line", (l) => {
      this.out.push(l);
      let v;
      try {
        v = JSON.parse(l);
      } catch {
        return;
      }
      const w = this.waiting.get(v.id);
      if (w) {
        this.waiting.delete(v.id);
        w(v);
      }
    });
    this.child.stdin.on("error", () => {});
  }

  /** A request, or `{exited: code}` when the process ends first. */
  async request(method, params) {
    const id = this.next++;
    const answer = new Promise((r) => this.waiting.set(id, r));
    this.child.stdin.write(`${JSON.stringify({ jsonrpc: "2.0", id, method, params })}\n`);
    return within(PATIENCE_MS, `cadpole mcp ${method}`, Promise.race([answer, this.exited.then((code) => ({ exited: code }))]));
  }

  async start() {
    const r = await this.request("initialize", { protocolVersion: "2025-06-18", capabilities: {}, clientInfo: { name: "replay", version: "0" } });
    if (r.exited !== undefined) return false;
    this.child.stdin.write(`${JSON.stringify({ jsonrpc: "2.0", method: "notifications/initialized" })}\n`);
    return true;
  }

  stop() {
    if (this.child.exitCode === null) this.child.kill("SIGKILL");
    return this.exited;
  }

  text() {
    return `${this.out.join("\n")}\n${this.err.join("\n")}`;
  }
}

/** What a tool call or a startup came to, in the model's words. */
function classify(text) {
  if (/is not available to a listener token|may not call/.test(text)) return "denied";
  if (/missing or wrong bearer token/.test(text)) return "unauthenticated";
  if (/no running `cadpole serve` found/.test(text)) return "none";
  if (/is no longer running/.test(text)) return "gone";
  if (/was restarted; start the listener again/.test(text)) return "replaced";
  if (/instances are running/.test(text)) return "ambiguous";
  if (/a symlink is not accepted|must be 0700|belongs to another user/.test(text)) return "dir";
  return "reached";
}

/** One HTTP request over TCP or a Unix socket: [status, body]. */
function http({ base, socketPath, method, path, headers, body }) {
  return within(
    PATIENCE_MS,
    `${method} ${path}`,
    new Promise((ok, bad) => {
      const u = new URL(base);
      const opts = socketPath ? { socketPath, path, method, headers } : { hostname: u.hostname, port: u.port, path, method, headers };
      const req = httpRequest(opts, (res) => {
        const chunks = [];
        res.on("data", (c) => chunks.push(c));
        res.on("end", () => ok([res.statusCode, Buffer.concat(chunks).toString("utf8")]));
      });
      req.on("error", bad);
      if (body) req.write(body);
      req.end();
    }),
  );
}

function requestOutcome(status, body) {
  if (status === 401) return "unauthenticated";
  if (status === 403) return /is not accepted on/.test(body) ? "notHere" : "denied";
  if (status === 404) return "absent";
  return "reached";
}

/** Does anything accept connections on this socket? */
function connects(path) {
  return new Promise((r) => {
    const c = createConnection(path);
    c.on("connect", () => {
      c.destroy();
      r(true);
    });
    c.on("error", () => r(false));
  });
}

async function replayOne(trace, index, work) {
  const base = mkdtempSync(join(work, `t${index}-`));
  const run = join(base, "run");
  const real = join(base, "real");
  mkdirSync(run, { mode: 0o700 });
  const dirs = { alpha: join(base, "alpha"), beta: join(base, "beta"), elsewhere: join(base, "elsewhere") };
  for (const d of Object.values(dirs)) mkdirSync(d);
  const docs = { alpha: join(dirs.alpha, "alpha.star"), beta: join(dirs.beta, "beta.star") };
  writeFileSync(docs.alpha, "b = box(10, 10, 10)\n");
  writeFileSync(docs.beta, "b = box(20, 20, 20)\n");
  const web = join(base, "web");
  mkdirSync(web);
  writeFileSync(join(web, "index.html"), "<!doctype html><title>replay</title>\n");
  const root = join(base, "documents");
  mkdirSync(root);
  const env = { ...process.env, CADPOLE_RUNTIME_DIR: run, CADPOLE_TOKEN: "", CADPOLE_LISTENER_TOKEN: "", CADPOLE_SERVER: "", HOME: base };
  delete env.CADPOLE_TOKEN_FILE;

  /** model id -> {child, said, instance, socket, http, tokens, exited} */
  const servers = new Map();
  /** client id -> Mcp */
  const clients = new Map();
  const tokens = [];
  const records = [];
  const transcripts = [];
  let foreign = null;
  const foreignDirs = [];
  let dirSpoiled = false;

  const fail = (step, what) => {
    throw new Error(`trace ${index}, step ${step} (${trace[step]?.did?.name}): ${what}`);
  };

  async function startServer(doc, agentTcp) {
    const said = [];
    const child = spawn(
      CADPOLE,
      ["serve", docs[doc], "--port", "0", "--no-solve-jobs", "--web-dir", web, "--root", root, ...(agentTcp ? ["--agent-tcp"] : [])],
      { env, stdio: ["ignore", "ignore", "pipe"] },
    );
    // `close`, not `exit`: the output is read to its end first, which is
    // what a refusal is judged by.
    const exited = new Promise((r) => child.on("close", (code) => r(code)));
    const ready = new Promise((ok) => {
      createInterface({ input: child.stderr }).on("line", (l) => {
        said.push(l);
        const m = /^agents: unix:(\S+)/.exec(l);
        if (m) ok(m[1]);
      });
    });
    const socket = await within(PATIENCE_MS, "cadpole serve to start", Promise.race([ready, exited.then((code) => ({ exited: code }))]));
    return { child, said, exited, socket };
  }

  for (let step = 1; step < trace.length; step++) {
    const st = trace[step].s;
    const did = trace[step].did;
    if (NOT_REPLAYED.includes(did.name)) fail(step, `stepReplay took ${did.name}, which this replay leaves to the simulation`);
    if (VERBOSE) console.log(`trace ${index} step ${step}: ${JSON.stringify(did)}`);

    switch (did.name) {
      case "start": {
        const r = await startServer(did.doc, did.flag);
        if (r.socket.exited !== undefined) fail(step, `cadpole serve exited with ${r.socket.exited}:\n${r.said.join("\n")}`);
        const instance = r.socket.split("/").pop().replace(/\.sock$/, "");
        const files = {
          record: join(run, `${instance}.json`),
          session: join(run, `${instance}.session-token`),
          listener: join(run, `${instance}.listener-token`),
          url: join(run, `${instance}.url`),
          sock: r.socket,
        };
        const record = JSON.parse(readFileSync(files.record, "utf8"));
        records.push(JSON.stringify(record));
        const t = {
          agent: readFileSync(files.session, "utf8").trim(),
          listener: readFileSync(files.listener, "utf8").trim(),
          browser: readFileSync(files.url, "utf8").trim().split("#token=")[1],
        };
        tokens.push(t.agent, t.listener, t.browser);
        servers.set(did.server, { ...r, instance, files, http: record.http, tokens: t, record });
        break;
      }
      case "startRefused": {
        const r = await startServer(did.doc, did.flag);
        if (r.socket.exited === undefined) fail(step, "a server started in a directory it should have refused");
        if (r.socket.exited === 0) fail(step, "a refused start exited 0");
        if (!/symlink is not accepted/.test(r.said.join("\n"))) fail(step, `refused for another reason:\n${r.said.join("\n")}`);
        transcripts.push(r.said);
        break;
      }
      case "stop": {
        const sv = servers.get(did.server);
        sv.child.kill("SIGTERM");
        await within(PATIENCE_MS, "cadpole serve to stop", sv.exited);
        break;
      }
      case "crash": {
        const sv = servers.get(did.server);
        sv.child.kill("SIGKILL");
        await within(PATIENCE_MS, "cadpole serve to die", sv.exited);
        break;
      }
      case "spoilDir":
        renameSync(run, real);
        symlinkSync(real, run);
        dirSpoiled = true;
        break;
      case "restoreDir":
        unlinkSync(run);
        renameSync(real, run);
        dirSpoiled = false;
        break;
      case "plantForeign": {
        // A short directory of its own: a socket path has a length limit.
        const sock = join(mkdtempSync("/tmp/cadpole-replay-"), "ffffffffffff.sock");
        foreignDirs.push(sock);
        foreign = createServer((c) => c.end("HTTP/1.1 200 OK\r\ncontent-length: 2\r\nconnection: close\r\n\r\n{}"));
        await new Promise((r) => foreign.listen(sock, r));
        chmodSync(sock, 0o666);
        const rec = join(run, "ffffffffffff.json");
        writeFileSync(rec, JSON.stringify({ instance: "ffffffffffff", pid: process.pid, socket: sock, document: docs.alpha, started: "2026-01-01T00:00:00Z" }));
        chmodSync(rec, 0o644);
        break;
      }
      case "send":
      case "refusedLocally": {
        const model = st.clients.get(did.client);
        let mcp = clients.get(did.client);
        if (!mcp || mcp.child.exitCode !== null) {
          mcp = new Mcp(dirs[model.cwd], env, model.listener);
          clients.set(did.client, mcp);
          if (!(await mcp.start())) {
            await mcp.exited;
            transcripts.push([mcp.text()]);
            clients.delete(did.client);
            const got = classify(mcp.text());
            // A listener cannot start without a server and its token; that
            // the model refuses a denied call before looking is the same
            // outcome: nothing ran.
            if (did.name === "refusedLocally") break;
            const lookup = st.lookups[st.lookups.length - 1];
            if (trace[step - 1].s.lookups.length === st.lookups.length) fail(step, `cadpole mcp --listener exited (${got}) where the model made a call`);
            if (got !== lookup.error) fail(step, `cadpole mcp --listener says ${got}, the model ${lookup.error}:\n${mcp.text()}`);
            break;
          }
        }
        const tool = did.op === "GetSession" ? "get_session" : "set_script";
        const args = did.op === "GetSession" ? {} : { path: "main.star", source: "b = box(1, 1, 1)\n", base_revision: 0 };
        const r = await mcp.request("tools/call", { name: tool, arguments: args });
        const text = r.result?.content?.[0]?.text ?? JSON.stringify(r.error ?? r);
        const isError = Boolean(r.result?.isError || r.error);
        const got = isError ? classify(text) : "reached";
        if (did.name === "refusedLocally") {
          if (got !== "denied") fail(step, `a listener's denied call came back ${got}: ${text}`);
          // A real process settles on a server as it starts (its event pump
          // asks at once); a client the model has not pinned is a client
          // that has not, so it starts afresh next time.
          if (!model.pinned) {
            await mcp.stop();
            transcripts.push([mcp.text()]);
            clients.delete(did.client);
          }
          break;
        }
        const before = trace[step - 1].s;
        if (st.lookups.length > before.lookups.length) {
          const lookup = st.lookups[st.lookups.length - 1];
          if (got !== lookup.error) fail(step, `cadpole mcp says ${got}, the model ${lookup.error}: ${text}`);
          // A client the model leaves unpinned looks afresh next time: so
          // does a new process.
          if (!model.pinned) {
            await mcp.stop();
            transcripts.push([mcp.text()]);
            clients.delete(did.client);
          }
          break;
        }
        const call = st.calls[st.calls.length - 1];
        if (got !== call.outcome) fail(step, `the call came back ${got}, the model says ${call.outcome}: ${text}`);
        if (did.op === "GetSession" && got === "reached") {
          const name = JSON.parse(text).document?.name;
          const want = st.servers.get(call.target).doc;
          if (name !== want) fail(step, `the call reached ${name}, the model's ${want}`);
        }
        break;
      }
      case "request": {
        const sv = servers.get(did.server);
        const earlier = [...servers.entries()].find(([id]) => id !== did.server)?.[1];
        const token = {
          browser: sv.tokens.browser,
          agent: sv.tokens.agent,
          listener: sv.tokens.listener,
          stale: earlier ? earlier.tokens.agent : "an-earlier-run-token-0000000000000000000000",
          wrong: "not-a-token-of-this-run-000000000000000000",
          none: null,
        }[did.presented];
        const path = did.route.split(" ")[1].replace("{method}", did.op);
        const method = did.route.split(" ")[0];
        const headers = { "content-type": "application/json" };
        if (path.startsWith("/api/cadpole.v1")) headers["connect-protocol-version"] = "1";
        if (token) headers.authorization = `Bearer ${token}`;
        const body = method === "POST" ? (did.op === "GetSession" ? "{}" : JSON.stringify({ path: "main.star", source: "b = box(1, 1, 1)\n", base_revision: 0 })) : undefined;
        const [status, text] = await http({
          base: sv.http,
          socketPath: did.surface === "agent" ? sv.socket : undefined,
          method,
          path,
          headers,
          body,
        });
        const got = requestOutcome(status, text);
        const want = st.requests[st.requests.length - 1].outcome;
        if (got !== want) fail(step, `${did.surface} ${did.route} (${did.op}) with the ${did.presented} token: HTTP ${status} is ${got}, the model says ${want}: ${text.slice(0, 300)}`);
        for (const t of tokens) if (text.includes(t)) fail(step, "a token came back in a response");
        break;
      }
      default:
        fail(step, `the replay does not know the action ${did.name}`);
    }

    // ---- compare the state
    for (const [id, m] of st.servers) {
      const sv = servers.get(id);
      const running = sv.child.exitCode === null && sv.child.signalCode === null;
      if (m.running !== running) fail(step, `server ${id} is ${running ? "running" : "not running"}, the model says ${m.running ? "running" : "not"}`);
      const want = new Set(m.files);
      for (const [kind, path] of [["record", sv.files.record], ["tokens", sv.files.session], ["tokens", sv.files.listener], ["url", sv.files.url], ["sock", sv.files.sock]]) {
        const there = existsSync(path);
        if (there !== want.has(kind)) fail(step, `server ${id}'s ${kind} file ${there ? "is there" : "is gone"}, the model says ${want.has(kind) ? "there" : "gone"}`);
        if (there && modeOf(path) !== 0o600) fail(step, `${path} is mode ${modeOf(path).toString(8)}`);
      }
      if (m.bound !== (running && (await connects(sv.files.sock)))) fail(step, `server ${id}'s socket ${m.bound ? "does not answer" : "answers"}`);
    }
    const dir = dirSpoiled ? real : run;
    if (modeOf(dir) !== 0o700) fail(step, `the runtime directory is mode ${modeOf(dir).toString(8)}`);
    if (st.dirOk === dirSpoiled) fail(step, "the runtime directory and the model disagree about whether it is private");
    // No token anywhere but its own file.
    const said = [
      ...[...servers.values()].map((sv) => sv.said.join("\n")),
      ...[...clients.values()].map((c) => c.text()),
      ...transcripts.map((t) => t.join("\n")),
      ...records,
    ];
    for (const p of [...servers.values()].filter((sv) => sv.child.exitCode === null).map((sv) => sv.child.pid).concat([...clients.values()].map((c) => c.child.pid))) {
      try {
        said.push(readFileSync(`/proc/${p}/cmdline`, "utf8"));
      } catch {
        // gone, or not Linux
      }
    }
    const all = said.join("\n");
    for (const t of tokens) if (all.includes(t)) fail(step, "a token was written into a log, a record, a command line or the MCP output");
    for (const f of readdirSync(dir)) {
      if (/^[0-9a-f]{12}\.json$/.test(f) && f !== "ffffffffffff.json") {
        const text = readFileSync(join(dir, f), "utf8");
        for (const t of tokens) if (text.includes(t)) fail(step, `${f} carries a token`);
      }
    }
  }

  for (const c of clients.values()) await c.stop();
  for (const sv of servers.values()) {
    if (sv.child.exitCode === null && sv.child.signalCode === null) {
      sv.child.kill("SIGKILL");
      await sv.exited;
    }
  }
  if (foreign) await new Promise((r) => foreign.close(r));
  for (const p of foreignDirs) rmSync(join(p, ".."), { recursive: true, force: true });
  if (dirSpoiled) {
    unlinkSync(run);
    renameSync(real, run);
  }
  rmSync(base, { recursive: true, force: true });
}

async function main() {
  const traces = Number(knob("TRACES", 6));
  const steps = Number(knob("STEPS", 25));
  const seed = knob("SEED", "0x5eed0f50c4e7");
  const width = Number(knob("WIDTH", 2));
  const work = mkdtempSync(join(process.env.TEST_TMPDIR ?? tmpdir(), "transport-replay-"));
  const started = Date.now();
  // `--init=initAdmitsAgentTokensOnTcp` and the like replay a mutant's
  // traces, which the real programs must then fail.
  const init = knob("INIT", "init");
  const all = generate({ model: MODEL, init, step: "stepReplay", traces, steps, seed, dir: join(work, "itf") });
  const counts = {};
  for (const t of all) for (const st of t.slice(1)) counts[st.did.name] = (counts[st.did.name] ?? 0) + 1;
  await inParallel(all, width, (trace, i) => replayOne(trace, i, work));
  const took = ((Date.now() - started) / 1000).toFixed(1);
  console.log(`transport_replay: ${all.length} traces, ${all.reduce((a, t) => a + t.length - 1, 0)} steps, seed ${seed}, ${took}s`);
  console.log(`  steps by action: ${JSON.stringify(counts)}`);

  rmSync(work, { recursive: true, force: true });
}

main().catch((e) => {
  console.error(e.stack ?? String(e));
  process.exit(1);
});
