// Front-end failure reporting.
//
// Without this, a wasm panic shows as `evaluation failed: unreachable` in the
// status bar and nothing else: no event, no line in the server's log, and the
// Rust message that would explain it goes to `console.error`, where nothing
// is looking.
//
// So every way the browser can fail becomes one `POST /api/ops/
// ReportClientFailure`, which becomes one `client_failure` event, which
// `cadpole listen` and an attached assistant see at the moment the user does.
//
//   * a wasm panic          -- the Rust message and `file:line:col` out of the
//                              engine's own panic hook (`take_panic`), not the
//                              `RuntimeError: unreachable` JavaScript sees
//   * `window.onerror`      -- uncaught exceptions on the page
//   * `unhandledrejection`  -- promises nobody caught
//   * a worker              -- neither of the two above fires for a worker, so
//                              a worker posts its failures to the page and the
//                              page forwards them (`adoptWorker`, and the
//                              service worker in sw.js)
//   * an operation error    -- an evaluate, render or session write the UI
//                              would otherwise have shown only in a status
//                              line
//
// **What a report carries, and what it must not.** Enough to act on without
// asking the user: the message, a stack, the operation, the script revision,
// the effective parameters, the engine build, and which surface it came from.
// Nothing that is not the user's own model or the code's own state: no page
// URL, no storage contents, no user agent, no cookies, no script text (a short
// content digest instead -- the server already has the script).
//
// **Caps, on both sides.** A panic inside a render loop fires thousands of
// times a second. The client coalesces identical failures and sends a count;
// past a per-page cap it stops sending at all. The server has its own token
// bucket and session cap and answers a refusal with a reason, which is logged
// here rather than swallowed. See docs/security.md, "Client failure reports".

import { authHeaders } from "./token.js";
import type { ParamValue } from "./types.js";

export type FailureKind =
  | "CLIENT_FAILURE_KIND_WASM_PANIC"
  | "CLIENT_FAILURE_KIND_EXCEPTION"
  | "CLIENT_FAILURE_KIND_REJECTION"
  | "CLIENT_FAILURE_KIND_OPERATION_ERROR"
  | "CLIENT_FAILURE_KIND_ENGINE_UNAVAILABLE";

export type FailureSurface = "CLIENT_SURFACE_PAGE" | "CLIENT_SURFACE_WORKER" | "CLIENT_SURFACE_SERVICE_WORKER";

/** What the caller knows about one failure. Everything else is filled in. */
export interface Failure {
  kind: FailureKind;
  message: string;
  /** `file:line:column`, when the failure names a place. */
  location?: string;
  stack?: string;
  /** Defaults to the page. */
  surface?: FailureSurface;
  /** "evaluate", "render", "startup", … */
  operation?: string;
}

/** The app state a report is stamped with, read at the moment of the failure. */
export interface FailureContext {
  revision(): number;
  params(): Record<string, ParamValue>;
  scriptSource(): string;
  engineBuild(): string;
  engineKind(): string;
}

export interface FailureReporterOptions {
  context: FailureContext;
  /**
   * False in static / playground mode: there is no server to report to, so
   * the console is the only place left. Reporting is not silently disabled
   * anywhere a server exists.
   */
  enabled: boolean;
  /** Test seam. Defaults to `fetch`. */
  send?: (body: string) => Promise<FailureAck | null>;
}

/** What the server says about a report. */
export interface FailureAck {
  seq?: number;
  accepted?: boolean;
  refused?: string;
  dropped?: number;
  remaining?: number;
  truncated?: string[];
}

/** Identical failures inside this window become one report with a count. */
const COALESCE_MS = 2_000;
/**
 * Reports one page load will send, whatever the server would allow. The
 * server's caps protect the session; this one protects the page, whose main
 * thread is the thing already in trouble.
 */
const MAX_REPORTS_PER_PAGE = 50;

const STARTED_AT = Date.now();

interface Pending {
  failure: Failure;
  count: number;
}

/**
 * A 64-bit FNV-1a digest of the script, as 16 hex digits.
 *
 * Not a security primitive and not trying to be: it answers "is this the same
 * text the crash happened on", which the revision number stops answering the
 * moment the user edits again. Synchronous on purpose -- `crypto.subtle` is a
 * promise, and this runs on a path where the next thing that happens may be
 * the tab dying.
 */
export function scriptDigest(source: string): string {
  let lo = 0x811c9dc5;
  let hi = 0x811c9dc5;
  for (let i = 0; i < source.length; i++) {
    const c = source.charCodeAt(i);
    lo = Math.imul(lo ^ (c & 0xff), 0x01000193) >>> 0;
    hi = Math.imul(hi ^ ((c >>> 8) ^ (i & 0xff)), 0x01000193) >>> 0;
  }
  return hi.toString(16).padStart(8, "0") + lo.toString(16).padStart(8, "0");
}

/**
 * A `ParamValue` in proto3 JSON.
 *
 * The UI carries parameter values flat (`40`, `true`, `"mm"`, `{x,y,z}`)
 * because the hand-written session routes accept them that way. `/api/ops/*`
 * is the generated front door and speaks proto3 JSON, where `ParamValue` is a
 * oneof: `{"number": 40}`. Sending the flat form is a 400, which is the report
 * failing to be reported.
 */
function paramValueJson(v: ParamValue): Record<string, unknown> | null {
  if (typeof v === "number") return Number.isFinite(v) ? { number: v } : null;
  if (typeof v === "boolean") return { boolean: v };
  if (typeof v === "string") return { text: v };
  if (v && typeof v === "object") return { vector: { x: v.x, y: v.y, z: v.z } };
  return null;
}

/** One line for `location` out of an ErrorEvent, or "" when it names nowhere. */
export function errorEventLocation(ev: { filename?: string; lineno?: number; colno?: number }): string {
  if (!ev.filename) return "";
  const line = ev.lineno ? `:${ev.lineno}` : "";
  const col = ev.colno ? `:${ev.colno}` : "";
  return `${ev.filename}${line}${col}`;
}

/** The message and stack of anything that can be thrown or rejected with. */
export function describeThrown(value: unknown): { message: string; stack: string } {
  if (value instanceof Error) return { message: `${value.name}: ${value.message}`, stack: value.stack ?? "" };
  if (typeof value === "string") return { message: value, stack: "" };
  try {
    return { message: JSON.stringify(value) ?? String(value), stack: "" };
  } catch {
    return { message: String(value), stack: "" };
  }
}

/**
 * The server will not accept reports from this page at all.
 *
 * Distinct from "this report did not get through": there is nothing to retry
 * and nothing to reduce, so the reporter stops instead of sending a second
 * doomed request for every failure.
 */
export class ReportRefused extends Error {
  constructor(message: string) {
    super(message);
    this.name = "ReportRefused";
  }
}

export class FailureReporter {
  private sent = 0;
  private stopped: string | null = null;
  private readonly pending = new Map<string, Pending>();
  private readonly send: (body: string) => Promise<FailureAck | null>;
  /** Every ack the server gave, newest last. The e2e test reads this. */
  readonly acks: FailureAck[] = [];

  constructor(private readonly opts: FailureReporterOptions) {
    this.send = opts.send ?? postReport;
  }

  /**
   * Record one failure. Identical failures inside `COALESCE_MS` are folded
   * into one report carrying `repeat`, which is what keeps a panicking render
   * loop from becoming ten thousand requests.
   */
  report(failure: Failure): void {
    // The console keeps working regardless: a report that cannot be sent must
    // not be a failure that cannot be seen.
    console.error(`cadpole: ${failure.kind} in ${failure.surface ?? "CLIENT_SURFACE_PAGE"}:`, failure.message, failure.location ?? "");
    if (!this.opts.enabled || this.stopped !== null) return;
    if (this.sent >= MAX_REPORTS_PER_PAGE) {
      this.stop(`this page has already sent ${MAX_REPORTS_PER_PAGE} failure reports`);
      return;
    }
    const key = [failure.kind, failure.surface ?? "", failure.operation ?? "", failure.message, failure.location ?? ""].join("|");
    const open = this.pending.get(key);
    if (open) {
      open.count += 1;
      return;
    }
    const entry: Pending = { failure, count: 1 };
    this.pending.set(key, entry);
    // Send the first one immediately -- the user is looking at a broken screen
    // now -- and hold the key open so the repeats behind it are counted
    // instead of sent.
    void this.flush(entry);
    globalThis.setTimeout(() => {
      this.pending.delete(key);
      // Everything that piled up behind the first one, as a count rather than
      // as that many requests.
      if (entry.count > 1) void this.flush({ failure, count: entry.count - 1 });
    }, COALESCE_MS);
  }

  /** Stop reporting, with the reason, once. */
  private stop(why: string): void {
    if (this.stopped !== null) return;
    this.stopped = why;
    console.warn(`cadpole: no more failure reports will be sent -- ${why}`);
  }

  private async flush(entry: Pending): Promise<void> {
    this.sent += 1;
    const c = this.opts.context;
    const f = entry.failure;
    const body: Record<string, unknown> = {
      kind: f.kind,
      message: f.message.slice(0, 2_000),
      surface: f.surface ?? "CLIENT_SURFACE_PAGE",
      uptime_ms: Date.now() - STARTED_AT,
      repeat: entry.count,
    };
    if (f.location) body["location"] = f.location.slice(0, 300);
    if (f.stack) body["stack"] = f.stack.slice(0, 8_000);
    if (f.operation) body["operation"] = f.operation;
    try {
      body["revision"] = c.revision();
      const params: Record<string, unknown> = {};
      for (const [name, value] of Object.entries(c.params())) {
        const encoded = paramValueJson(value);
        if (encoded) params[name] = encoded;
      }
      body["params"] = params;
      body["script_digest"] = scriptDigest(c.scriptSource());
      body["engine_build"] = c.engineBuild();
      body["engine_kind"] = c.engineKind();
    } catch {
      // Reading the app's own state must never be why a crash goes
      // unreported: send what we have.
    }
    let ack: FailureAck | null = null;
    try {
      ack = await this.send(JSON.stringify(body));
      if (!ack) {
        // The server would not take it -- a schema that has moved, a field it
        // does not like. Losing the whole report over a field would defeat
        // the point, so try again with only what is certainly valid: it is better to know *that* it crashed than nothing at all.
        const minimal = { kind: body["kind"], message: body["message"], surface: body["surface"], operation: body["operation"] ?? "" };
        ack = await this.send(JSON.stringify(minimal));
        if (ack) console.warn("cadpole: the full failure report was refused; sent a reduced one");
      }
    } catch (err) {
      // A refusal is not a failure to deliver: the server is there and will
      // not take this, and it will not take the next one either. Retrying a
      // reduced body against a 401 only turns one dead request into two.
      // A refusal is not a failure to deliver: the server is there and will
      // not take this, and it will not take the next one either. Retrying a
      // reduced body against a 401 only turns one dead request into two.
      if (err instanceof ReportRefused) {
        this.stop(err.message);
        return;
      }
      console.warn("cadpole: could not report the failure to the server", err);
      return;
    }
    if (!ack) return;
    this.acks.push(ack);
    // The server's caps are visible here too, rather than being a silent 200.
    if (ack.truncated?.length) console.warn(`cadpole: the server shortened ${ack.truncated.join(", ")} in the failure report`);
    if (ack.accepted === false) console.warn(`cadpole: the server refused the failure report: ${ack.refused ?? "no reason given"}`);
    if (ack.remaining === 0) this.stop(`the session will not record any more (${ack.dropped ?? 0} dropped)`);
  }
}

/**
 * The one call the UI makes with `fetch` rather than through `wire.ts`.
 *
 * It is still the Connect route and still proto3 JSON -- Connect's JSON codec
 * *is* the proto3 mapping, so the body is unchanged -- but it is written out
 * by hand because of `keepalive`: the page may be on its way down when this
 * runs, and a crash reporter that needs the generated client to still be
 * loadable is a crash reporter that misses the crashes worth having.
 */
async function postReport(body: string): Promise<FailureAck | null> {
  const res = await fetch("/api/cadpole.v1.CadpoleOps/ReportClientFailure", {
    method: "POST",
    headers: {
      "content-type": "application/json",
      accept: "application/json",
      "connect-protocol-version": "1",
      "x-cadpole-client": "ui",
      ...authHeaders(),
    },
    // The page may be on its way down when this runs; `keepalive` is what lets
    // the request outlive it.
    keepalive: true,
    cache: "no-store",
    body,
  });
  if (res.status === 401 || res.status === 403) {
    // This tab's token is not this server's -- almost always a restart. Every
    // later report would get the same answer, so this is not the "minimal
    // body" retry a non-OK response gets: the server did not dislike a field.
    throw new ReportRefused(`the server refused this tab's token (${res.status})`);
  }
  if (!res.ok) {
    console.warn(`cadpole: POST /api/ops/ReportClientFailure -> ${res.status}`);
    return null;
  }
  return (await res.json()) as FailureAck;
}

// ---------------------------------------------------------------------------
// Installation

let active: FailureReporter | null = null;

/** The reporter in use, for code that failed somewhere without a handle. */
export function reporter(): FailureReporter | null {
  return active;
}

/**
 * Catch everything the page can throw at us, and start reporting.
 *
 * `error` and `unhandledrejection` are page-scoped: a worker's failures do not
 * reach them, which is why workers post theirs instead (`adoptWorker`).
 */
export function installFailureReporting(opts: FailureReporterOptions): FailureReporter {
  const r = new FailureReporter(opts);
  active = r;
  // V8 keeps 10 frames by default, which for a wasm panic is often all
  // trampoline. The stack is the thing a report is for.
  try {
    const E = Error as unknown as { stackTraceLimit?: number };
    if (typeof E.stackTraceLimit === "number" && E.stackTraceLimit < 50) E.stackTraceLimit = 50;
  } catch {
    /* not V8 */
  }
  window.addEventListener("error", (ev: ErrorEvent) => {
    // A failed <script>/<img> load also fires `error`, with no `error` object
    // and no message worth sending.
    if (!ev.message && !ev.error) return;
    const d = describeThrown(ev.error ?? ev.message);
    r.report({
      kind: "CLIENT_FAILURE_KIND_EXCEPTION",
      message: d.message || ev.message,
      stack: d.stack,
      location: errorEventLocation(ev),
    });
  });
  window.addEventListener("unhandledrejection", (ev: PromiseRejectionEvent) => {
    const d = describeThrown(ev.reason);
    r.report({ kind: "CLIENT_FAILURE_KIND_REJECTION", message: d.message, stack: d.stack });
  });
  return r;
}

// ---------------------------------------------------------------------------
// Workers
//
// A dedicated worker's `error` and `unhandledrejection` do not bubble to the
// page's handlers, and a worker has no token and no session of its own. So the
// worker reports to the page (src/web/src/worker-report.ts) and the page
// forwards. The message shape is the contract between the two, and sw.js
// hand-rolls the same shape because a service worker cannot import the bundle.

export const WORKER_FAILURE_MESSAGE = "cadpole:failure";

export interface WorkerFailureMessage {
  __cadpole: typeof WORKER_FAILURE_MESSAGE;
  kind: FailureKind;
  message: string;
  location?: string;
  stack?: string;
  operation?: string;
}

function isWorkerFailure(data: unknown): data is WorkerFailureMessage {
  const o = data as Record<string, unknown> | null;
  return Boolean(o) && o?.["__cadpole"] === WORKER_FAILURE_MESSAGE && typeof o["message"] === "string";
}

/**
 * Forward one worker's failures. Call this for every worker the page owns; it
 * is what makes `CLIENT_SURFACE_WORKER` reports possible at all.
 */
export function adoptWorker(worker: Worker, r: FailureReporter | null = active): () => void {
  const onMessage = (ev: MessageEvent): void => {
    if (!isWorkerFailure(ev.data)) return;
    r?.report({ ...ev.data, surface: "CLIENT_SURFACE_WORKER" });
  };
  // `error` on the Worker object fires for an uncaught error the worker did
  // not report itself (one thrown before the shim was installed, or a worker
  // script that failed to load at all).
  const onError = (ev: ErrorEvent): void => {
    r?.report({
      kind: "CLIENT_FAILURE_KIND_EXCEPTION",
      message: ev.message || "the worker failed before it could say why",
      location: errorEventLocation(ev),
      surface: "CLIENT_SURFACE_WORKER",
    });
  };
  worker.addEventListener("message", onMessage);
  worker.addEventListener("error", onError as EventListener);
  return () => {
    worker.removeEventListener("message", onMessage);
    worker.removeEventListener("error", onError as EventListener);
  };
}

/** The same, for the service worker, which posts to its clients instead. */
export function adoptServiceWorker(r: FailureReporter | null = active): void {
  if (!("serviceWorker" in navigator)) return;
  navigator.serviceWorker.addEventListener("message", (ev: MessageEvent) => {
    if (!isWorkerFailure(ev.data)) return;
    r?.report({ ...ev.data, surface: "CLIENT_SURFACE_SERVICE_WORKER" });
  });
}
