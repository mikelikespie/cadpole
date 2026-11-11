// A tiny reactive store: shallow-merged state, listeners get (next, prev).
// UI modules subscribe and compare the slices they care about by reference.

import type { ApiMode } from "./api.js";
import type { EngineKind } from "./engine.js";
import type { EnginePreference } from "./engine-policy.js";
import type { ApiDoc, Attachment, EntityRef, LayerOverride, Model, ParamValue, Session } from "./types.js";
import { NO_SECTION, type SectionState } from "./section.js";
import type { ViewMode } from "./viewer.js";

export type Listener<T> = (state: T, prev: T) => void;

export class Store<T extends object> {
  private state: T;
  private readonly listeners = new Set<Listener<T>>();
  private notifying = false;
  private pending: Partial<T>[] = [];

  constructor(initial: T) {
    this.state = initial;
  }

  get(): T {
    return this.state;
  }

  /** Shallow-merge a patch. Listeners run only if some key actually changed. */
  set(patch: Partial<T> | ((s: T) => Partial<T>)): void {
    const p = typeof patch === "function" ? patch(this.state) : patch;
    if (this.notifying) {
      // Re-entrant set from a listener: apply after the current notification.
      this.pending.push(p);
      return;
    }
    this.apply(p);
    while (this.pending.length > 0) this.apply(this.pending.shift() as Partial<T>);
  }

  private apply(p: Partial<T>): void {
    let changed = false;
    for (const k of Object.keys(p) as (keyof T)[]) {
      if (!Object.is(p[k], this.state[k])) {
        changed = true;
        break;
      }
    }
    if (!changed) return;
    const prev = this.state;
    this.state = { ...prev, ...p };
    this.notifying = true;
    try {
      for (const l of this.listeners) l(this.state, prev);
    } finally {
      this.notifying = false;
    }
  }

  subscribe(l: Listener<T>): () => void {
    this.listeners.add(l);
    return () => {
      this.listeners.delete(l);
    };
  }

  /** Subscribe to a derived value; fires immediately and whenever it changes. */
  select<V>(
    selector: (s: T) => V,
    cb: (value: V, prev: V | undefined) => void,
    eq: (a: V, b: V) => boolean = Object.is,
  ): () => void {
    let last = selector(this.state);
    cb(last, undefined);
    return this.subscribe((s) => {
      const v = selector(s);
      if (!eq(v, last)) {
        const prev = last;
        last = v;
        cb(v, prev);
      }
    });
  }
}

export type ConnectionStatus = "connecting" | "live" | "reconnecting" | "offline" | "static";
export type LeftTab = "model" | "script" | "examples" | "api";
/** Panels available in the narrow-screen bottom sheet. Parameters first, then the display layers. */
export type SheetTab = "params" | "layers" | "model" | "script" | "inspector" | "messages" | "issues" | "examples" | "api" | "view";

export interface EvalInfo {
  /** Wall clock the user waited, end to end. */
  millis: number;
  /** What the engine itself reported spending, when it said (0 = it did not). */
  engineMillis: number;
  /** performance.now() when the evaluation finished. */
  finished_at: number;
  /** Set when evaluate() threw (as opposed to reporting diagnostics). */
  error: string | null;
  triangles: number;
  /** Which engine actually produced this. */
  kind: EngineKind;
  /** Why that engine: "local takes 6.6 s", "you chose remote", … */
  why: string;
  /** The tessellation tolerance this was computed at, in mm. */
  tolerance: number;
  /** Rung of the refinement ladder, from 0, and how many there are. */
  stage: number;
  stages: number;
}

/**
 * A rebuilt bundle this page could not take on without reloading.
 *
 * It is offered, never taken: a reload that interrupts someone mid-thought is
 * worse than a stale tab, so the page says so and waits. `dismissed` hides the
 * interruption and not the fact -- the bar collapses to a chip that stays for
 * as long as the page is behind.
 */
export interface BuildUpdate {
  /** The fingerprint the server is serving now. */
  build: string;
  /** Why nothing cheaper would do, in one sentence. */
  reason: string;
  /** The user pressed "Not now". */
  dismissed: boolean;
}

export interface AppState {
  apiMode: ApiMode;
  session: Session | null;
  model: Model | null;
  apiDoc: ApiDoc | null;
  /** The engine that ran the most recent evaluation. */
  engineKind: EngineKind;
  /** Visible banner text when the wasm engine could not be loaded. */
  engineNotice: string | null;
  /** What the user asked for: auto, local or remote. */
  enginePreference: EnginePreference;
  /** Whether there is a server to offload to at all. */
  remoteAvailable: boolean;
  /** A transient note about the engine, e.g. a remote failure that fell back. */
  engineStatus: string | null;
  /** An evaluation is running and could be given up on. */
  cancellable: boolean;
  connection: ConnectionStatus;
  selected: EntityRef[];
  hovered: EntityRef | null;
  note: string;
  /** Local parameter overrides (mirrors document.param_overrides). */
  overrides: Record<string, ParamValue>;
  hiddenBodies: ReadonlySet<string>;
  /**
   * The Layers panel's state: what the user changed about each
   * display layer, by name ("" is the implicit "Other" layer), over the
   * script's own `layer(...)` defaults. Display-side only: a change here
   * reaches the viewer and never the engine, so it survives re-evaluation
   * and is remembered per document.
   */
  layerOverrides: ReadonlyMap<string, LayerOverride>;
  leftTab: LeftTab;
  /** Narrow layout: which bottom sheet is open (null = closed). */
  sheet: SheetTab | null;
  narrow: boolean;
  activeScript: string;
  /** Script line to reveal (1-based) plus a nonce so repeated reveals re-fire. */
  revealLine: { path: string; line: number; nonce: number } | null;
  evaluating: boolean;
  evalInfo: EvalInfo | null;
  viewMode: ViewMode;
  showGrid: boolean;
  showLabels: boolean;
  showBBox: boolean;
  orthographic: boolean;
  showHud: boolean;
  /**
   * The plates view: the model's bodies drawn where `cadpole export`
   * would put them on their print plates, instead of where the script put
   * them. Only meaningful, and only offered, when `model.plates` is not
   * empty; switched off by a model that has none.
   */
  platesView: boolean;
  /**
   * The section: up to one horizontal and one vertical cutting
   * plane, millimetres, as `cadpole render --section` spells them. A redraw,
   * never an evaluation; works the same on a published page.
   */
  section: SectionState;
  /** The Section panel is open. */
  sectionPanel: boolean;
  /** Which plate the plates view frames: an index into `model.plates`, or null for all of them. */
  activePlate: number | null;
  /** Snapshots waiting to be sent with the next message. */
  pendingAttachments: Attachment[];
  /** A deferred PWA install prompt is available. */
  installable: boolean;
  /** Service worker reports the app is available offline. */
  offlineReady: boolean;
  /** Increment to ask the messages panel to focus its input. */
  focusMessage: number;
  /** A rebuilt bundle that needs a page load, waiting on the user. */
  buildUpdate: BuildUpdate | null;
  /** A transient line about something that was replaced without one. */
  hotNote: string | null;
}

export function initialState(
  apiMode: ApiMode,
  engineKind: EngineKind,
  engineNotice: string | null,
  engine: { preference: EnginePreference; remoteAvailable: boolean } = { preference: "auto", remoteAvailable: false },
): AppState {
  return {
    apiMode,
    session: null,
    model: null,
    apiDoc: null,
    engineKind,
    engineNotice,
    enginePreference: engine.preference,
    remoteAvailable: engine.remoteAvailable,
    engineStatus: null,
    cancellable: false,
    connection: "connecting",
    selected: [],
    hovered: null,
    note: "",
    overrides: {},
    hiddenBodies: new Set(),
    layerOverrides: new Map(),
    leftTab: "model",
    sheet: null,
    narrow: false,
    activeScript: "main.star",
    revealLine: null,
    evaluating: false,
    evalInfo: null,
    viewMode: "shaded_edges",
    showGrid: true,
    showLabels: true,
    showBBox: false,
    orthographic: false,
    showHud: false,
    platesView: false,
    activePlate: null,
    section: NO_SECTION,
    sectionPanel: false,
    pendingAttachments: [],
    installable: false,
    offlineReady: false,
    focusMessage: 0,
    buildUpdate: null,
    hotNote: null,
  };
}

export type AppStore = Store<AppState>;

/**
 * Post a message with the screenshots queued for it, and take those -- and
 * only those -- off the queue once it has gone. On failure the queue is left
 * as it was.
 */
export async function postWithQueued<T>(
  store: Store<AppState>,
  post: (attachments: Attachment[] | undefined) => Promise<T>,
): Promise<T> {
  const atts = store.get().pendingAttachments;
  const m = await post(atts.length ? atts : undefined);
  // Only the ones that went: the Snapshot button stays live while the post is
  // in flight, and what it queued then is for the next message.
  store.set({ pendingAttachments: store.get().pendingAttachments.filter((a) => !atts.includes(a)) });
  return m;
}
