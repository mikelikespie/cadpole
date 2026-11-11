// Session API client. Two implementations behind one interface:
//
// - HttpApi: the `cadpole serve` operation registry over unary Connect
//   (`POST /api/cadpole.v1.CadpoleOps/<Rpc>`, binary protobuf in and out --
//   docs/architecture.md) plus the /api/events SSE stream.
// - LocalApi: static / serverless mode (no /api). State lives in
//   localStorage and the URL hash; events are dispatched locally.
//
// `probeApi()` decides between them with a single GetSession call.
//
// Encoding lives in `wire.ts` and nowhere else: this file is the interface the
// rest of the UI is written against, in the plain shapes of `types.ts`.

import { EXAMPLES, defaultExample, type Example } from "./examples.js";
import * as norm from "./normalize.js";
import type { ConnectionStatus } from "./state.js";
import { authHeaders, clearToken } from "./token.js";
import * as wire from "./wire.js";
import { ApiError, ops } from "./wire.js";
import type {
  Attachment,
  EntityRef,
  CadpoleDocument,
  CadpoleEvent,
  Message,
  ParamValue,
  ScriptFile,
  Selection,
  Session,
} from "./types.js";

/** `published`: a baked, read-only document with no server (baked.ts). */
export type ApiMode = "session" | "static" | "published";

export interface SelectionPut {
  selected: EntityRef[];
  hovered?: EntityRef | null;
  note?: string;
}

/** Per-write extras; see `putParams`. */
export interface WriteOptions {
  keepalive?: boolean;
}

export interface SessionApi {
  readonly mode: ApiMode;
  getSession(): Promise<Session>;
  putScript(file: ScriptFile): Promise<Session>;
  /**
   * `opts.keepalive` is for a write issued as the page goes away: without it
   * the browser cancels the request along with the document, and an edit made
   * inside the debounce window before a reload is simply lost.
   */
  putParams(overrides: Record<string, ParamValue>, opts?: WriteOptions): Promise<Session>;
  putSelection(sel: SelectionPut, opts?: WriteOptions): Promise<Session>;
  postMessage(from: "user" | "assistant", text: string, attachments?: Attachment[]): Promise<Message>;
  /**
   * Turn the Listen switch on or off: whether what the user types is passed
   * to an attached assistant. Only a session server has one.
   */
  setListening?(listening: boolean): Promise<void>;
  /** Persist the document. Returns the path written, or null when not applicable. */
  save(): Promise<string | null>;
  /** Subscribe to events after `after`; returns an unsubscribe function. */
  subscribe(
    after: number,
    onEvent: (e: CadpoleEvent) => void,
    onStatus: (s: ConnectionStatus) => void,
    /**
     * The server refused this tab's token, so nothing will sync again until
     * the page is reopened with a new one. "offline" alone does not say that:
     * it reads as a network blip that will pass, and this one never does.
     */
    onAuthLost?: () => void,
    /**
     * The stream reconnected to a different run of the server: a restarted
     * `cadpole serve` is a new session (a new `id`) whose event log starts
     * again at 1. Events the page missed, and any sequence-number watermark it
     * holds, belong to the old run; `session` is the new run's state, and the
     * stream resumes after its `lastEventSeq`.
     */
    onRestart?: (session: Session) => void,
  ): () => void;
}

export { ApiError } from "./wire.js";

// ---------------------------------------------------------------------------
// HTTP session server

export class HttpApi implements SessionApi {
  readonly mode = "session" as const;

  /**
   * The last session the server described, kept so a write can return one.
   *
   * The operation registry answers a write with what the write did
   * (`SetParamsResponse` is the revision and the overrides in force), not with
   * a whole session; the UI's callers want a `Session`. So a write applies its
   * authoritative result to this copy and hands that back. Nothing depends on
   * the copy being right for long: every write also emits a session event, the
   * UI does not filter its own, and the refresh it triggers is a real
   * `GetSession` 150 ms later.
   */
  private last: Session | null = null;

  private remember(s: Session): Session {
    this.last = s;
    return s;
  }

  /**
   * Apply one write's answer to the copy *as it is when the answer arrives*,
   * and hand that back.
   *
   * Not to the copy the write started from, because writes overlap. After a
   * "Reset all" that grows the feature tree under a parked pointer, the tree
   * hovers a body and the selection's 100 ms debounce fires within a
   * millisecond or two of the parameters' 300 ms one, so a `SetSelection` is
   * in flight while the reset's `SetParams` is. Built on its starting copy,
   * the selection's answer would put the pre-reset overrides back into this
   * cache and hand them to the page, which would evaluate parameters the
   * server no longer held and flip back on the next `GetSession`. An answer says
   * what its own write did and nothing else, so that is all it may change.
   */
  private fold(before: Session, change: (now: Session) => Session): Session {
    return this.remember(change(this.last ?? before));
  }

  /** The cached session, or a fresh one when this instance has never read one. */
  private async base(): Promise<Session> {
    return this.last ?? (await this.getSession());
  }

  async getSession(): Promise<Session> {
    return this.getSessionWithin();
  }

  /** `getSession`, abortable: the probe gives up after a few seconds. */
  async getSessionWithin(signal?: AbortSignal): Promise<Session> {
    return this.remember(wire.sessionFrom(await ops.getSession({}, signal)));
  }

  async putScript(file: ScriptFile): Promise<Session> {
    const before = await this.base();
    let res = await ops.setScript({ path: file.path, source: file.source, baseRevision: BigInt(before.revision) });
    // `SetScript` refuses a stale base revision instead of clobbering. The UI
    // is the interactive writer and its last word wins, as it did when this
    // was a PUT with no base revision at all -- but it now costs one extra
    // round trip to say so, and it happens only when someone else wrote in
    // between.
    if (res.conflict) {
      res = await ops.setScript({ path: file.path, source: file.source, baseRevision: res.currentRevision });
      if (res.conflict) throw new ApiError(409, `the session moved on while saving ${file.path}`);
    }
    const revision = Number(res.revision);
    return this.fold(before, (now) => {
      const scripts = now.document.scripts.some((s) => s.path === file.path)
        ? now.document.scripts.map((s) => (s.path === file.path ? { ...file } : s))
        : [...now.document.scripts, { ...file }];
      return { ...now, revision, document: { ...now.document, scripts } };
    });
  }

  async putParams(overrides: Record<string, ParamValue>, opts: WriteOptions = {}): Promise<Session> {
    const before = await this.base();
    // The registry's SetParams merges; this call has always replaced. Name
    // the overrides that are going away and it does the same thing.
    const clear = Object.keys(before.document.param_overrides).filter((k) => !(k in overrides));
    const res = await ops.setParams({ overrides: wire.paramValuesTo(overrides), clear }, opts);
    const inForce = wire.paramValuesFrom(res.paramOverrides);
    return this.fold(before, (now) => ({
      ...now,
      revision: Number(res.revision),
      document: { ...now.document, param_overrides: inForce },
    }));
  }

  /**
   * The Listen switch. Not an operation of `CadpoleOps`, so that no assistant
   * holds a tool that turns its own input on: it is the owner's route,
   * `PUT /api/session/assistant`, and a listener token is refused it.
   */
  async setListening(listening: boolean): Promise<void> {
    const res = await fetch("/api/session/assistant", {
      method: "PUT",
      headers: { "content-type": "application/json", ...authHeaders() },
      body: JSON.stringify({ listening }),
    });
    if (!res.ok) throw new Error(`the Listen switch was not changed (${res.status})`);
    if (this.last) this.last = { ...this.last, assistantListening: listening };
  }

  async putSelection(sel: SelectionPut, opts: WriteOptions = {}): Promise<Session> {
    const before = await this.base();
    const note = sel.note ?? before.selection.note;
    const res = await ops.setSelection(
      {
        selected: sel.selected.map(wire.refTo),
        ...(sel.hovered ? { hovered: wire.refTo(sel.hovered) } : {}),
        note,
      },
      opts,
    );
    return this.fold(before, (now) => ({
      ...now,
      revision: Number(res.revision),
      selection: {
        selected: sel.selected.map((r) => ({ ...r })),
        hovered: sel.hovered ? { ...sel.hovered } : null,
        note,
      },
    }));
  }

  async postMessage(from: "user" | "assistant", text: string, attachments?: Attachment[]): Promise<Message> {
    // Two operations, because who a message is from is decided by which one
    // posted it and never by a field the caller sets (capability.rs): the
    // user's own client posts with PostMessage, an assistant with Reply.
    let m: Message;
    if (from === "assistant") {
      const res = await ops.reply({ text });
      m = { seq: Number(res.seq), from: "assistant", text, at: new Date().toISOString() };
    } else {
      const res = await ops.postMessage({ text, attachments: (attachments ?? []).map(wire.attachmentTo) });
      m = res.message ? wire.messageFrom(res.message) : { seq: 0, from: "user", text, at: new Date().toISOString() };
    }
    // Keep the images locally: the server never echoes the bytes back.
    if (attachments && attachments.length && !m.attachments) m.attachments = attachments;
    return m;
  }

  async save(): Promise<string | null> {
    const res = await ops.save({});
    return res.path || null;
  }

  /**
   * The event stream, read with `fetch` rather than `EventSource`.
   *
   * `EventSource` cannot set a request header, which is the only reason the
   * server ever accepted `?token=` on this one route -- and a token in a query
   * string is a token in the browser's history, in any `Referer`, and in every
   * proxy log between here and there. `fetch` can set `Authorization`, and SSE
   * is a trivial text format, so parsing the frames here costs about thirty
   * lines and buys the token one consistent home.
   *
   * What is reimplemented, because `EventSource` was doing it: reconnection
   * with backoff (ours also keeps `after=` current, which `EventSource` could
   * not), and named-event dispatch.
   *
   * This is the one thing the browser does not do in the wire format, and it
   * stays that way on purpose. SSE frames are text, so protobuf over
   * them would mean base64 in `data:` lines -- paying a third to use a binary
   * encoding -- or giving up SSE for a framing of our own, along with
   * reconnection and `Last-Event-ID`. An event is a kind, a sequence number
   * and a path or a revision; image bytes are kept out of events by design.
   * Callers that want the events as protobuf have `WaitEvents`, which returns
   * `repeated cadpole.v1.Event` and is what the CLI and MCP use.
   */
  subscribe(
    after: number,
    onEvent: (e: CadpoleEvent) => void,
    onStatus: (s: ConnectionStatus) => void,
    onAuthLost?: () => void,
    onRestart?: (session: Session) => void,
  ): () => void {
    let lastSeq = after;
    // Which run of the server `lastSeq` counts in. A sequence number means
    // nothing across a restart: the new run's log starts again at 1, and
    // `get_events` answers `after=<old seq>` with nothing until its own log
    // passes it, so a cursor kept across a restart leaves the page "live" and
    // deaf. The session id is regenerated on every start, which is how a
    // reconnection tells a blip from a restart.
    let epoch: string | null = this.last?.id ?? null;
    let connected = false;
    let closed = false;
    let backoff = 1000;
    let timer: number | null = null;
    let abort: AbortController | null = null;

    const deliver = (kind: string, id: string, data: string): void => {
      let raw: unknown;
      try {
        raw = JSON.parse(data);
      } catch {
        return;
      }
      const o = (raw ?? {}) as Record<string, unknown>;
      const e: CadpoleEvent =
        typeof o["kind"] === "string" ? norm.event(o) : { seq: Number(id) || lastSeq + 1, at: "", kind, data: raw };
      if (e.seq <= lastSeq && e.seq !== 0) return;
      if (e.seq > lastSeq) lastSeq = e.seq;
      onEvent(e);
    };

    /** One SSE frame: `event:`/`id:`/`data:` lines, `data` possibly repeated. */
    const frame = (text: string): void => {
      let kind = "message";
      let id = "";
      const data: string[] = [];
      for (const line of text.split("\n")) {
        if (line.startsWith(":")) continue; // a comment; the keep-alive is one
        const colon = line.indexOf(":");
        const field = colon === -1 ? line : line.slice(0, colon);
        const value = colon === -1 ? "" : line.slice(colon + 1).replace(/^ /, "");
        if (field === "event") kind = value;
        else if (field === "id") id = value;
        else if (field === "data") data.push(value);
      }
      if (data.length) deliver(kind, id, data.join("\n"));
    };

    const open = async (): Promise<void> => {
      if (closed) return;
      onStatus(connected ? "reconnecting" : "connecting");
      abort = new AbortController();
      if (connected || epoch === null) {
        let sess: Session | null = null;
        try {
          sess = await this.getSessionWithin(abort.signal);
        } catch (err) {
          // A refused token is the stream's to report, below; anything else
          // means the server is not back yet.
          if (!(err instanceof ApiError && err.status === 401)) return retry();
        }
        if (closed) return;
        if (sess) {
          if (epoch === null) epoch = sess.id;
          else if (sess.id !== epoch || sess.lastEventSeq < lastSeq) {
            epoch = sess.id;
            lastSeq = sess.lastEventSeq;
            onRestart?.(sess);
          }
        }
      }
      let res: Response;
      try {
        // `live=1` asks for the frames of replies in progress as well as the
        // log. A frame has no id and its `seq` is 0, so `deliver` passes it
        // on without moving `lastSeq`.
        res = await fetch(`/api/events?after=${lastSeq}&live=1`, {
          headers: { accept: "text/event-stream", ...authHeaders() },
          cache: "no-store",
          signal: abort.signal,
        });
      } catch {
        return retry();
      }
      if (res.status === 401) {
        // The token is stale (the server restarted). Nothing here can fix
        // that, so stop rather than hammering the endpoint -- and say so:
        // the pill going to "offline" reads as a blip that will pass, while
        // in fact every edit from here on is going nowhere.
        clearToken();
        closed = true;
        onStatus("offline");
        onAuthLost?.();
        return;
      }
      if (!res.ok || !res.body) return retry();
      backoff = 1000;
      connected = true;
      onStatus("live");
      const reader = res.body.getReader();
      const decoder = new TextDecoder();
      let buf = "";
      try {
        for (;;) {
          const { done, value } = await reader.read();
          if (done) break;
          buf += decoder.decode(value, { stream: true });
          // Frames are separated by a blank line; \r\n is legal too.
          let cut = buf.search(/\r?\n\r?\n/);
          while (cut !== -1) {
            const end = buf.indexOf("\n\n", cut) !== -1 ? buf.indexOf("\n\n", cut) + 2 : buf.indexOf("\r\n\r\n", cut) + 4;
            frame(buf.slice(0, cut).replace(/\r/g, ""));
            buf = buf.slice(end);
            cut = buf.search(/\r?\n\r?\n/);
          }
        }
      } catch {
        /* the stream broke; retry below */
      }
      retry();
    };

    const retry = (): void => {
      abort = null;
      if (closed) return;
      onStatus("reconnecting");
      timer = window.setTimeout(() => void open(), backoff);
      backoff = Math.min(backoff * 2, 10000);
    };

    void open();

    return () => {
      closed = true;
      if (timer !== null) window.clearTimeout(timer);
      abort?.abort();
      abort = null;
      onStatus("offline");
    };
  }
}

// ---------------------------------------------------------------------------
// Static / serverless mode

const STORAGE_KEY = "cadpole.local.v1";

interface LocalState {
  exampleId: string;
  document: CadpoleDocument;
  selection: Selection;
  messages: Message[];
  revision: number;
}

/**
 * The second decoding of a string override. writeHash encodes the value
 * before URLSearchParams encodes the whole `p`, so a link the app wrote needs
 * both; a hand-typed or truncated one (`100%`, a `%` cut off by a chat client)
 * has already been read leniently by URLSearchParams and is not valid
 * percent-encoding a second time. That value is the one the user typed.
 */
function decodeOnce(v: string): string {
  try {
    return decodeURIComponent(v);
  } catch {
    return v;
  }
}

/** The static app's `#ex=<id>&p=<name>:<value>,...` share link. Exported for tests. */
export function parseHash(hash: string = location.hash): { exampleId: string | null; overrides: Record<string, ParamValue> } {
  const h = hash.replace(/^#/, "");
  const params = new URLSearchParams(h);
  const overrides: Record<string, ParamValue> = {};
  const p = params.get("p");
  if (p) {
    for (const pair of p.split(",")) {
      const [k, v] = pair.split(":");
      if (!k || v === undefined) continue;
      if (v === "true" || v === "false") overrides[k] = v === "true";
      else if (v !== "" && Number.isFinite(Number(v))) overrides[k] = Number(v);
      else overrides[k] = decodeOnce(v);
    }
  }
  return { exampleId: params.get("ex"), overrides };
}

function writeHash(exampleId: string, overrides: Record<string, ParamValue>): void {
  const params = new URLSearchParams();
  params.set("ex", exampleId);
  const parts: string[] = [];
  for (const [k, v] of Object.entries(overrides)) {
    if (typeof v === "object") continue; // vectors are not URL-encoded
    parts.push(`${k}:${typeof v === "string" ? encodeURIComponent(v) : String(v)}`);
  }
  if (parts.length) params.set("p", parts.join(","));
  const next = `#${params.toString()}`;
  if (location.hash !== next) history.replaceState(null, "", next);
}

function loadLocal(): LocalState | null {
  try {
    const raw = localStorage.getItem(STORAGE_KEY);
    if (!raw) return null;
    const o = JSON.parse(raw) as Record<string, unknown>;
    return {
      exampleId: typeof o["exampleId"] === "string" ? o["exampleId"] : "custom",
      document: norm.document(o["document"]),
      selection: norm.selection(o["selection"]),
      messages: Array.isArray(o["messages"]) ? o["messages"].map(norm.message) : [],
      revision: Number(o["revision"]) || 0,
    };
  } catch {
    return null;
  }
}

function docFromExample(ex: Example): CadpoleDocument {
  return {
    id: `local-${ex.id}`,
    name: ex.name,
    display_unit: "mm",
    scripts: [{ path: "main.star", source: ex.source }],
    param_overrides: {},
  };
}

export class LocalApi implements SessionApi {
  readonly mode = "static" as const;
  private state: LocalState;
  private seq = 0;
  private readonly listeners = new Set<(e: CadpoleEvent) => void>();

  constructor() {
    const hash = parseHash();
    const stored = loadLocal();
    const hashExample = hash.exampleId ? EXAMPLES.find((e) => e.id === hash.exampleId) : undefined;
    if (hashExample && (!stored || stored.exampleId !== hashExample.id)) {
      this.state = {
        exampleId: hashExample.id,
        document: docFromExample(hashExample),
        selection: { selected: [], hovered: null, note: "" },
        messages: [],
        revision: 1,
      };
    } else if (stored) {
      this.state = stored;
    } else {
      const ex = defaultExample();
      this.state = {
        exampleId: ex.id,
        document: docFromExample(ex),
        selection: { selected: [], hovered: null, note: "" },
        messages: [],
        revision: 1,
      };
    }
    if (Object.keys(hash.overrides).length > 0) {
      this.state.document.param_overrides = { ...this.state.document.param_overrides, ...hash.overrides };
    }
    this.persist();
    window.addEventListener("hashchange", () => this.onHashChange());
  }

  /** Static mode extra: replace the script with an example from the gallery. */
  loadExample(id: string): Promise<Session> {
    const ex = EXAMPLES.find((e) => e.id === id);
    if (!ex) return Promise.reject(new Error(`unknown example ${id}`));
    this.state.exampleId = ex.id;
    this.state.document = docFromExample(ex);
    this.state.selection = { selected: [], hovered: null, note: "" };
    this.bump("script_changed", { path: "main.star" });
    return Promise.resolve(this.session());
  }

  private onHashChange(): void {
    const hash = parseHash();
    if (hash.exampleId && hash.exampleId !== this.state.exampleId) {
      const ex = EXAMPLES.find((e) => e.id === hash.exampleId);
      if (ex) {
        this.state.exampleId = ex.id;
        this.state.document = docFromExample(ex);
        this.state.document.param_overrides = hash.overrides;
        this.bump("script_changed", { path: "main.star" });
        return;
      }
    }
    this.state.document.param_overrides = hash.overrides;
    this.bump("params_changed", { overrides: hash.overrides });
  }

  private session(): Session {
    // Deep-ish copy so callers cannot mutate our state through the result.
    return norm.session({
      id: this.state.document.id,
      revision: this.state.revision,
      document: this.state.document,
      selection: this.state.selection,
      messages: this.state.messages,
      clients: 1,
    });
  }

  private persist(): void {
    try {
      // Screenshots are kept in memory only (localStorage quota).
      const slim = {
        ...this.state,
        messages: this.state.messages.map((m) => (m.attachments ? { ...m, attachments: undefined } : m)),
      };
      localStorage.setItem(STORAGE_KEY, JSON.stringify(slim));
    } catch {
      /* private mode / quota: state is still live in memory */
    }
    writeHash(this.state.exampleId, this.state.document.param_overrides);
  }

  private bump(kind: CadpoleEvent["kind"], data: unknown): void {
    this.state.revision += 1;
    this.persist();
    const e: CadpoleEvent = { seq: ++this.seq, at: new Date().toISOString(), kind, data };
    // Deliver asynchronously, like a network event would arrive.
    window.setTimeout(() => {
      for (const l of this.listeners) l(e);
    }, 0);
  }

  getSession(): Promise<Session> {
    return Promise.resolve(this.session());
  }
  putScript(file: ScriptFile): Promise<Session> {
    const scripts = this.state.document.scripts;
    const i = scripts.findIndex((s) => s.path === file.path);
    if (i >= 0) scripts[i] = { ...file };
    else scripts.push({ ...file });
    const ex = EXAMPLES.find((e) => e.id === this.state.exampleId);
    if (!ex || ex.source !== file.source) this.state.exampleId = "custom";
    this.bump("script_changed", { path: file.path });
    return Promise.resolve(this.session());
  }
  putParams(overrides: Record<string, ParamValue>): Promise<Session> {
    this.state.document.param_overrides = { ...overrides };
    this.bump("params_changed", { overrides });
    return Promise.resolve(this.session());
  }
  putSelection(sel: SelectionPut): Promise<Session> {
    this.state.selection = {
      selected: sel.selected.map((r) => ({ ...r })),
      hovered: sel.hovered ? { ...sel.hovered } : null,
      note: sel.note ?? "",
    };
    this.persist();
    return Promise.resolve(this.session());
  }
  postMessage(from: "user" | "assistant", text: string, attachments?: Attachment[]): Promise<Message> {
    const seq = (this.state.messages.at(-1)?.seq ?? 0) + 1;
    const m: Message = { seq, from, text, at: new Date().toISOString() };
    if (attachments && attachments.length) m.attachments = attachments;
    this.state.messages.push(m);
    this.bump("message", m);
    return Promise.resolve(m);
  }
  save(): Promise<string | null> {
    // No server: offer the script as a download.
    const main = this.state.document.scripts[0];
    if (!main) return Promise.resolve(null);
    const blob = new Blob([main.source], { type: "text/plain" });
    const url = URL.createObjectURL(blob);
    const a = document.createElement("a");
    a.href = url;
    a.download = main.path;
    a.click();
    window.setTimeout(() => URL.revokeObjectURL(url), 1000);
    return Promise.resolve(main.path);
  }
  subscribe(
    _after: number,
    onEvent: (e: CadpoleEvent) => void,
    onStatus: (s: ConnectionStatus) => void,
    _onAuthLost?: () => void,
  ): () => void {
    this.listeners.add(onEvent);
    onStatus("static");
    return () => {
      this.listeners.delete(onEvent);
    };
  }
}

// ---------------------------------------------------------------------------

export interface Probe {
  api: SessionApi;
  session: Session;
  /**
   * Set when a session server answered but refused us. The caller must say so
   * rather than quietly dropping into static mode: falling back would show the
   * user an empty playground document while their real one sits on the server,
   * which reads as data loss.
   */
  unauthorized?: boolean;
}

/** One GetSession call decides between session mode and static mode. */
export async function probeApi(opts: { forceStatic?: boolean; timeoutMs?: number } = {}): Promise<Probe> {
  let unauthorized = false;
  if (!opts.forceStatic) {
    const ctl = new AbortController();
    const t = window.setTimeout(() => ctl.abort(), opts.timeoutMs ?? 4000);
    try {
      // A static host has no /api, so this 404s (or fails outright) and we
      // fall through to the playground. A server answers with a Session, and
      // the HttpApi that made the call keeps it as its baseline.
      const api = new HttpApi();
      const session = await api.getSessionWithin(ctl.signal);
      window.clearTimeout(t);
      return { api, session };
    } catch (err) {
      window.clearTimeout(t);
      // 401/403 is "there is a server here and it will not talk to us",
      // which is a different thing from "there is no server here".
      if (err instanceof ApiError && (err.status === 401 || err.status === 403)) {
        unauthorized = true;
        console.info("cadpole: the session server refused this token; open the URL `cadpole serve` printed");
      } else {
        console.info("cadpole: no session server, using static mode:", err instanceof Error ? err.message : err);
      }
    }
  }
  const api = new LocalApi();
  return { api, session: await api.getSession(), unauthorized };
}
