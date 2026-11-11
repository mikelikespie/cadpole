// Cadpole web UI entry point: wires the engine, the session API, the store, the
// viewer and the panels together.
//
//   ?mock=1       use MockEngine instead of loading ./cadpole_wasm.js
//   ?engine=main  run the wasm engine on this thread instead of in a worker
//   ?static=1     skip the /api probe and run in static (serverless) mode
//   ?nosw=1       do not register the service worker
//   ?perf=1       record the apply path's phases and long tasks
//   ?baked=URL    open a baked model read-only, with no engine; a page
//                 `cadpole publish` wrote names its bake in a <meta> instead
//   ?section=     open cut: `-y=0`, `z=1200,-y=3000` (mm; src/web/src/section.ts)
//   ?mode=        open in a display mode: shaded, shaded_edges, wireframe, xray, faces
//   ?view=        open on a named view: home, iso, top, bottom, front, back, left, right
//   ?eye=&target= open from a camera, `x,y,z` in mm (src/web/src/opening.ts)

import { LocalApi, probeApi, type Probe, type SessionApi, type WriteOptions } from "./api.js";
import { BakedEngine, bakedSource, fetchBaked, PublishedApi, type Baked } from "./baked.js";
import type { Actions } from "./actions.js";
import { EXAMPLES } from "./examples.js";
import { DEFAULT_TOLERANCE, EnginePanic, type EngineKind, type EngineLike, type LoadedEngine } from "./engine.js";
import { loadLocalEngine, requestedEngine } from "./engine-load.js";
import { loadPreference, savePreference, type EnginePreference } from "./engine-policy.js";
import { Evaluator, EVALUATE_SLOT, type EvaluateStage } from "./evaluator.js";
import { applyBuild, describePlan, type BuildPlan, type HotReloadContext } from "./hot-reload.js";
import { isAbandoned, JobRunner } from "./jobs.js";
import { ParamSync } from "./param-sync.js";
import { RemoteEngine } from "./remote-engine.js";
import {
  adoptServiceWorker,
  adoptWorker,
  describeThrown,
  installFailureReporting,
  reporter,
  type FailureReporter,
} from "./failure.js";
import * as norm from "./normalize.js";
import { enablePerf, perfRequested, span } from "./perf.js";
import { dropServiceWorkerCaches, keepFocusedInputVisible, preventPageZoom, promptInstall, setupPwa, trackKeyboardInset } from "./pwa.js";
import { applyFrame, mergeSnapshot, upsertMessage } from "./reply-stream.js";
import { refKey, refLabel, refListsEqual, toggleRef } from "./selection.js";
import { Store, initialState, postWithQueued, type AppState, type SheetTab } from "./state.js";
import { createBuildBar } from "./ui/build-bar.js";
import { adoptTokenFromUrl } from "./token.js";
import type { ApiDoc, EntityRef, CadpoleEvent, LayerOverride, Message, ParamValue, Session, SourceSpan } from "./types.js";
import { Announcer } from "./ui/a11y.js";
import { createApiDocPanel } from "./ui/apidoc.js";
import { createDiagnosticsPanel } from "./ui/diagnostics.js";
import { h } from "./ui/dom.js";
import { createExamplesPanel } from "./ui/examples.js";
import { createHud } from "./ui/hud.js";
import { createInspectorPanel } from "./ui/inspector.js";
import { createLayout } from "./ui/layout.js";
import { createMessagesPanel } from "./ui/messages.js";
import { createLayersPanel, effectiveLayers, isSoloed, listLayers } from "./ui/layers.js";
import { createParamReadout, createParamsPanel } from "./ui/params.js";
import { PickerPopover } from "./ui/picker.js";
import { createScriptPanel } from "./ui/script.js";
import { createPlatesPanel } from "./ui/plates.js";
import { createSelectionCard } from "./ui/selcard.js";
import { createToolbar } from "./ui/toolbar.js";
import { createTreePanel } from "./ui/tree.js";
import type { WheelMode } from "./controls.js";
import { debounce, overridesEqual, stableJson } from "./util.js";
import { VIEW_DIRS, VIEW_MODES, Viewer, type Candidate, type ViewMode, type ViewPreset } from "./viewer.js";
import { Vector3 } from "three/webgpu";
import { formatSection, isSectioned, parseSection, type SectionState } from "./section.js";
import { createSectionPanel } from "./ui/section.js";
import { openingFrom, safeHomeUrl, viewLink } from "./opening.js";

const WHEEL_MODE_KEY = "cadpole.wheelMode";
/** The `coalesce` slot the API reference runs in. */
const APIDOC_SLOT = "apiDoc";
/** Where the UI state a reload would otherwise lose is parked. */
const UI_STATE_KEY = (docId: string): string => `cadpole:ui:${docId}`;
/** The bottom-sheet tabs, for validating what came back out of storage. */
const SHEET_TABS: readonly SheetTab[] = ["params", "layers", "model", "script", "inspector", "messages", "issues", "examples", "api", "view"];

async function main(): Promise<void> {
  const params = new URLSearchParams(location.search);
  const forceStatic = params.get("static") === "1";
  // Before the engine loads, so the very first apply is measured too.
  if (perfRequested()) enablePerf();

  // Before anything makes a request: take `#token=` out of the URL, keep it
  // per-tab, and rewrite the address bar without it (src/web/src/token.ts).
  adoptTokenFromUrl();

  // A published page has its model already: no wasm, no worker, no
  // `/api` probe. Everything below runs as for any other page, against an
  // engine that answers with the bake and a session that will not change.
  const bakedUrl = bakedSource(params);
  const [loaded, probe]: [LoadedEngine, Probe] = bakedUrl
    ? await startPublished(bakedUrl)
    : await Promise.all([loadLocalEngine(requestedEngine(params)), probeApi({ forceStatic })]);
  const { notice, failure: engineFailure } = loaded;
  // How the page opens: the address's cut, display mode and camera over the
  // publisher's (src/web/src/opening.ts). Applied at start-up, below.
  const opening = openingFrom(params, published?.view ?? null);
  /** This tab had a display mode of its own to come back to. */
  let restoredViewMode = false;
  // `let`, because a rebuilt wasm engine is swapped in underneath this page
  // rather than reloading it. Everything that reports which engine is
  // running reads this binding instead of capturing it at start-up, so a crash
  // report after a swap names the build that actually answered.
  let engine: EngineLike = loaded.engine;
  /** How many engines this page has installed. 1 is the one it started with. */
  let engineEpoch = 1;
  const api: SessionApi = probe.api;
  // There is something to offload to only when a session server answered. The
  // static playground is the mode with no server by definition,
  // so the Engine control shows Remote disabled rather than failing later.
  const remote = api.mode === "session" ? new RemoteEngine() : null;
  const authNotice = probe.unauthorized
    ? "This browser has no valid token for the Cadpole server on this machine. `cadpole serve` prints a new one every time it starts — open the URL it printed."
    : null;
  const preference = loadPreference();
  const store = new Store<AppState>(
    initialState(api.mode, engine.kind, authNotice ?? notice, { preference, remoteAvailable: remote !== null }),
  );
  const appEl = document.getElementById("app") ?? document.body.appendChild(h("div", { id: "app" }));
  const announcer = new Announcer(document.body);
  document.documentElement.dataset["mode"] = api.mode;
  document.documentElement.dataset["engine"] = engine.kind;
  document.documentElement.dataset["build"] = engine.buildId;
  document.documentElement.dataset["localEngine"] = engine.kind;
  document.documentElement.dataset["engineEpoch"] = String(engineEpoch);

  // The engine that ran the last evaluation, and the build it was.
  //
  // Declared *here*, above the failure reporter: its context getters close
  // over these two bindings, and the very first report a page can make --
  // `engine_unavailable` at start-up -- is sent from the line below. A `let`
  // declared further down would throw a ReferenceError there, which `flush`
  // swallows, and the report would go out without `engine_kind` and
  // `engine_build`, the one fact it exists to carry.
  let lastEngineKind: EngineKind = engine.kind;
  let lastEngineBuild = engine.buildId;

  // ---------------------------------------------------- failure reporting
  //
  // Installed before anything else can fail: a crash during start-up is the
  // one this feature exists for. In static mode there is no server to
  // report to, so the console is all there is; anywhere else, every uncaught
  // exception, unhandled rejection, worker failure and wasm panic becomes a
  // `client_failure` event the moment the user sees it.
  const failures: FailureReporter = installFailureReporting({
    enabled: api.mode === "session",
    context: {
      revision: () => store.get().session?.revision ?? 0,
      // The *effective* values, not just the override map: a panic that only
      // happens at `wall=0.4` is not reproducible from an empty `{}` plus a
      // script you have to evaluate to learn the defaults of. The script's own
      // values come from the last successful model; the user's overrides go on
      // top, including one that has not been evaluated yet -- which is
      // precisely the one that crashed it.
      params: () => {
        const s = store.get();
        const out: Record<string, ParamValue> = {};
        for (const p of s.model?.params ?? []) out[p.name] = p.value;
        return { ...out, ...s.overrides };
      },
      scriptSource: () => {
        const s = store.get();
        return s.session?.document.scripts.find((f) => f.path === s.activeScript)?.source ?? "";
      },
      // The engine that ran the *last evaluation*, not the one loaded at
      // start-up. With three of them behind one seam, a report that
      // named the local wasm for a model the server produced would send
      // someone hunting a panic in the wrong binary.
      // ...and whether this page is *behind* the build the server is serving.
      // The user can say "not now" to a reload, so a panic can
      // arrive from a page that has been running last hour's `app.js` for an
      // hour. The same stack from the current build and from a stale one are
      // different bug reports, and only one of them is worth chasing.
      engineBuild: () => {
        const behind = store.get().buildUpdate;
        return behind ? `${lastEngineBuild} [page is behind the server: ${behind.reason}]` : lastEngineBuild;
      },
      engineKind: () => lastEngineKind,
    },
  });
  adoptServiceWorker(failures);
  // The debug seam: `__cadpole.failures.acks` is what the server said about each
  // report, and `__cadpole.adoptWorker(w)` is how a worker this module did not
  // create gets its failures forwarded. Both are here because the alternative
  // -- a test double of the reporting path -- proves nothing about the real
  // one.
  (window as unknown as Record<string, unknown>)["__cadpole"] = { failures, adoptWorker, viewLink: () => currentViewLink(), viewport: () => viewportDebug() };

  if (engineFailure) {
    // The mock is standing in for the engine. The banner says so to the user;
    // this says so to the session, because a silent fallback to a hardcoded
    // model is a failure that looks like success.
    failures.report({
      kind: "CLIENT_FAILURE_KIND_ENGINE_UNAVAILABLE",
      message: engineFailure.message,
      stack: engineFailure.stack,
      operation: "startup",
    });
  }

  // ------------------------------------------------------------ syncing

  // Assigned after the panels exist; guarded where a panel may call in earlier.
  let viewer: Viewer;
  let picker: PickerPopover;
  let lastClick: { x: number; y: number; keys: string; at: number } | null = null;
  let wheelModePref: WheelMode = "auto";
  try {
    const saved = localStorage.getItem(WHEEL_MODE_KEY);
    if (saved === "orbit" || saved === "zoom" || saved === "auto") wheelModePref = saved;
  } catch {
    /* ignore */
  }
  let selectionDirty = false;
  let suppressSync = false;
  // Whether the next evaluation is a drag preview: set by every
  // slider move, cleared by the release and by anything else that changes a
  // parameter, so the evaluation after a drag is always the full one.
  let previewing = false;

  const sendSelection = (opts: WriteOptions = {}): Promise<void> => {
    const s = store.get();
    selectionDirty = false;
    return api
      .putSelection({ selected: s.selected, hovered: s.hovered, note: s.note }, opts)
      .then((sess) => applySession(sess, { selection: false }))
      // A sync that fails means the session no longer matches the screen, so
      // it is reported rather than only logged.
      .catch((err: unknown) => reportEngineError(err, "selection_sync"));
  };
  const pushSelection = debounce(() => void sendSelection(), 100);

  const paramSync = new ParamSync({
    current: () => store.get().overrides,
    put: (overrides, opts) => api.putParams(overrides, opts),
    read: () => api.getSession(),
    apply: (sess) => applySession(sess, { selection: false }),
    // A sync that fails means the session no longer matches the screen, so
    // it is reported rather than only logged.
    report: (err) => reportEngineError(err, "params_sync"),
  });
  const sendParams = (opts: WriteOptions = {}): Promise<void> => paramSync.send(opts);
  const pushParams = debounce(() => void sendParams(), 300);

  /**
   * Send whatever is still sitting on a debounce timer, now.
   *
   * The overrides and the selection are session state on the server,
   * and they get there through a 300 ms and a 100 ms trailing debounce.
   * Unflushed, a slider moved in the last 300 ms before a reload, a tab close
   * or a navigation would be lost -- and the page would come back with the
   * value the user had just changed. `keepalive` is what
   * makes the write survive the page: without it the browser cancels the
   * request along with the document.
   *
   * Resolves when the writes have been answered, so a caller that is about to
   * reload can wait for them; it is bounded by its caller, never by this.
   */
  function flushWrites(opts: WriteOptions = {}): Promise<void> {
    const waiting: Promise<void>[] = [];
    if (pushParams.pending()) {
      pushParams.cancel();
      waiting.push(sendParams(opts));
    }
    if (pushSelection.pending()) {
      pushSelection.cancel();
      waiting.push(sendSelection(opts));
    }
    return Promise.all(waiting).then(() => undefined);
  }

  const scheduleEvaluate = debounce(() => evaluateNow(), 120);
  let evalCount = 0;
  let stageCount = 0;
  let frameCount = 0;

  const jobs = new JobRunner();
  const evaluator = new Evaluator(
    jobs,
    { local: engine, remote },
    {
      onStage: applyStage,
      onError: (err, operation) => reportEngineError(err, operation),
      onBusy: (status) => {
        // The pending indicator is the runner's own state, not a flag someone
        // remembered to set: it is on exactly when the slot has work.
        store.set({ evaluating: status.running || status.queued, cancellable: status.running });
        document.documentElement.dataset["evaluating"] = status.running || status.queued ? "1" : "0";
      },
      onNotice: (text) => {
        store.set({ engineStatus: text });
        if (text) announcer.say(text, { assertive: true });
      },
    },
    preference,
  );

  /**
   * Show one result of one evaluation.
   *
   * Called for every rung of the refinement ladder, coarse first, and never
   * after the job it came from was abandoned -- the job runner guarantees
   * both, which is why nothing here re-checks whether this result is still the
   * current one.
   */
  function applyStage(stage: EvaluateStage): void {
    span("apply", () => applyStageNow(stage));
  }

  function applyStageNow(stage: EvaluateStage): void {
    const { evaluated, final } = stage;
    // Belt and braces on the engine swap. The job runner already
    // guarantees this cannot happen -- a swap abandons the slot before the new
    // engine is installed, and an abandoned job's result is dropped rather
    // than delivered -- so reaching here means that guarantee broke, and the
    // one thing worse than a stale picture is a stale picture nobody knows
    // about. Drop it and say so.
    if (stage.producedBy !== engine && stage.producedBy !== remote) {
      reportEngineError(
        new Error(`a result from a retired ${stage.producedBy.kind} engine (${stage.producedBy.buildId}) reached the viewer`),
        "hot_reload_stale_result",
      );
      return;
    }
    const prevDiag = summarizeDiagnostics(store.get().model?.diagnostics ?? []);
    // A refinement is not a change to the model, so it must not set the
    // change-flash off on every body whose triangle count moved.
    span("apply.setModel", () => viewer.setModel(evaluated, { flash: final }));
    lastEngineKind = evaluated.kind;
    // The engine object that actually ran it, by identity: with the local one
    // replaceable mid-session, reading "the engine" from a binding would name
    // whichever build happened to be installed when the answer landed.
    lastEngineBuild = stage.producedBy.buildId;
    document.documentElement.dataset["modelEpoch"] = String(engineEpoch);
    const next: Partial<AppState> = {
      model: evaluated.model,
      engineKind: evaluated.kind,
      engineStatus: stage.fellBack ? store.get().engineStatus : null,
      evalInfo: {
        // What the *user* waited, not what the engine spent. For the remote
        // engine those differ by the round trip and the megabyte of mesh, and
        // a status bar reporting "server 0 ms" after a visible pause is worse
        // than no status bar. `engineMillis` keeps the engine's own figure.
        millis: evaluated.elapsedMs,
        engineMillis: evaluated.model.eval_millis,
        finished_at: performance.now(),
        error: null,
        triangles: viewer.triangleCount,
        kind: evaluated.kind,
        why: stage.why,
        tolerance: evaluated.tolerance,
        stage: stage.stage,
        stages: stage.stages,
      },
    };
    // Every panel that watches the model redraws inside this: the feature
    // tree, the inspector, the diagnostics list and the parameters.
    span("apply.store", () => store.set(next));
    document.documentElement.dataset["engine"] = evaluated.kind;
    // Selection and hover live outside the meshes, so a refinement keeps them:
    // what is selected is a topological *name*, which does not depend on how
    // finely anything was tessellated.
    span("apply.highlight", () => viewer.setHighlight(store.get().selected, store.get().hovered));
    const nowDiag = summarizeDiagnostics(evaluated.model.diagnostics);
    if (nowDiag !== prevDiag && nowDiag) announcer.say(nowDiag);
    // Readiness markers for tests / tooling: `evals` counts finished
    // evaluations, `stages` counts every result including coarse passes.
    document.documentElement.dataset["stages"] = String(++stageCount);
    if (final) document.documentElement.dataset["evals"] = String(++evalCount);
  }

  function evaluateNow(): void {
    const s = store.get();
    if (!s.session) return;
    evaluator
      .request({
        docId: s.session.document.id,
        scripts: s.session.document.scripts,
        overrides: s.overrides,
        tolerance: DEFAULT_TOLERANCE,
        preview: previewing,
        hasAssets: (s.session.document.asset_names ?? []).length > 0,
      })
      .catch((err: unknown) => {
        // Being superseded is the system working: a slider drag produces one
        // of these per frame and none of them is a failure.
        if (isAbandoned(err)) return;
        const msg = err instanceof Error ? err.message : String(err);
        store.set({
          evalInfo: {
            millis: 0,
            engineMillis: 0,
            finished_at: performance.now(),
            error: msg,
            triangles: 0,
            kind: lastEngineKind,
            why: "",
            tolerance: DEFAULT_TOLERANCE,
            stage: 0,
            stages: 1,
          },
        });
        announcer.say(`Evaluation failed: ${msg}`, { assertive: true });
        // A wasm panic arrives here as an EnginePanic carrying the Rust
        // message and the file:line:column the hook recorded.
        reportEngineError(err, "evaluate");
      });
  }

  /**
   * One failure out of an engine or session call, reported rather than
   * swallowed into a status line.
   *
   * A wasm panic is the interesting case: `EnginePanic` carries the Rust
   * message and the `file:line:column` from the engine's panic hook, which is
   * the difference between a report someone can act on and the word
   * "unreachable".
   */
  function reportEngineError(err: unknown, operation: string): void {
    if (err instanceof EnginePanic) {
      failures.report({
        kind: "CLIENT_FAILURE_KIND_WASM_PANIC",
        message: err.message,
        location: err.location,
        stack: err.engineStack || (err.stack ?? ""),
        operation,
      });
      return;
    }
    const d = describeThrown(err);
    failures.report({ kind: "CLIENT_FAILURE_KIND_OPERATION_ERROR", message: d.message, stack: d.stack, operation });
  }

  function summarizeDiagnostics(d: { severity: string }[]): string {
    const e = d.filter((x) => x.severity === "error").length;
    const w = d.filter((x) => x.severity === "warning").length;
    if (!e && !w) return "";
    return [e ? `${e} error${e === 1 ? "" : "s"}` : "", w ? `${w} warning${w === 1 ? "" : "s"}` : ""].filter(Boolean).join(", ");
  }

  /** Merge a Session from the server into the store, keeping references stable when content is equal. */
  function applySession(incoming: Session, opts: { selection: boolean; initial?: boolean }): void {
    const s = store.get();
    const prev = s.session;
    const next: Session = { ...incoming };
    let scriptsChanged = true;
    if (prev) {
      if (stableJson(prev.document.scripts) === stableJson(incoming.document.scripts)) {
        next.document = { ...incoming.document, scripts: prev.document.scripts };
        scriptsChanged = false;
      }
      // Servers may strip screenshot payloads, and a read may be older than
      // what the event stream already delivered of a reply in progress.
      const merged = mergeSnapshot(prev.messages, incoming.messages);
      next.messages = stableJson(prev.messages) === stableJson(merged) ? prev.messages : merged;
      if (overridesEqual(prev.document.param_overrides, incoming.document.param_overrides)) {
        next.document = { ...next.document, param_overrides: prev.document.param_overrides };
      }
    }
    const patch: Partial<AppState> = { session: next };
    let overridesChanged = false;
    if (!paramSync.dirty && !overridesEqual(s.overrides, incoming.document.param_overrides)) {
      patch.overrides = incoming.document.param_overrides;
      overridesChanged = true;
    }
    if (!s.activeScript || !incoming.document.scripts.some((f) => f.path === s.activeScript)) {
      patch.activeScript = incoming.document.scripts[0]?.path ?? "main.star";
    }
    if (opts.selection && !selectionDirty) {
      if (!refListsEqual(s.selected, incoming.selection.selected)) patch.selected = incoming.selection.selected;
      if (incoming.selection.note !== s.note) patch.note = incoming.selection.note;
    }
    suppressSync = true;
    store.set(patch);
    suppressSync = false;
    // The session view reports the build the server is serving, so every
    // refresh is a chance to notice one this page never saw the event for.
    // `cadpole serve` keeps its event log in memory, so a restart starts it again
    // at sequence 1 and the `build_changed` the restart itself produced can be
    // at or below a guard taken before it -- `spec/reload.qnt` calls that
    // `aSequenceGuardMissesABuildAfterARestart`, and this comparison is the
    // fix for it. Guarded on `pageBuild`
    // being set at all, so the initial load does not race its own start-up,
    // and on the build not already being the one on offer: a page holding an
    // unanswered (or dismissed) reload offer is behind by design, and there is
    // nothing left for it to plan about that build.
    const behind = incoming.webBuild;
    if (pageBuild && behind && behind !== pageBuild && store.get().buildUpdate?.build !== behind) onBuildChanged(behind);
    if (opts.initial) evaluateNow();
    else if (scriptsChanged || overridesChanged) scheduleEvaluate();
  }

  function appendMessage(m: Message): void {
    const s = store.get();
    if (!s.session) return;
    const messages = upsertMessage(s.session.messages, m);
    if (messages !== s.session.messages) store.set({ session: { ...s.session, messages } });
  }

  // ------------------------------------------------------------ actions

  const actions: Actions = {
    select(refs, opts = {}) {
      const s = store.get();
      let selected: EntityRef[];
      if (opts.additive) {
        selected = s.selected;
        for (const r of refs) selected = toggleRef(selected, r);
      } else {
        selected = refs;
      }
      if (refListsEqual(selected, s.selected)) return;
      store.set({ selected, note: "" });
      if (opts.announce !== false) {
        const last = selected[selected.length - 1];
        announcer.say(
          last
            ? `Selected ${last.kind} ${refLabel(store.get().model, last)}${selected.length > 1 ? `, ${selected.length} selected` : ""}`
            : "Selection cleared",
        );
      }
    },
    clearSelection() {
      picker.hide();
      actions.select([]);
    },
    hover(ref) {
      const s = store.get();
      if ((ref === null && s.hovered === null) || (ref && s.hovered && refKey(ref) === refKey(s.hovered))) return;
      store.set({ hovered: ref });
    },
    setNote(note) {
      store.set({ note });
    },
    setOverride(name, value: ParamValue, preview = false) {
      paramSync.edited();
      previewing = preview;
      // Always a new object, even for the same value: the release after a
      // drag repeats the last value, and it is the evaluation that runs the
      // fuses the preview skipped.
      store.set({ overrides: { ...store.get().overrides, [name]: value } });
    },
    resetOverride(name) {
      const s = store.get();
      if (!(name in s.overrides)) return;
      const next = { ...s.overrides };
      delete next[name];
      paramSync.edited();
      previewing = false;
      store.set({ overrides: next });
    },
    resetAllOverrides() {
      if (Object.keys(store.get().overrides).length === 0) return;
      paramSync.edited();
      previewing = false;
      store.set({ overrides: {} });
    },
    async sendMessage(text) {
      const m = await postWithQueued(store, (atts) => api.postMessage("user", text, atts));
      appendMessage(m);
    },
    async setListening(listening) {
      const s = store.get();
      if (!s.session || !api.setListening) return;
      await api.setListening(listening);
      const now = store.get().session;
      if (now) store.set({ session: { ...now, assistantListening: listening } });
      announcer.say(listening ? "Listening: messages go to the assistant" : "Not listening");
    },
    async loadExample(id) {
      const ex = EXAMPLES.find((e) => e.id === id);
      if (!ex) return;
      const sess = api instanceof LocalApi ? await api.loadExample(id) : await api.putScript({ path: "main.star", source: ex.source });
      applySession(sess, { selection: true });
      announcer.say(`Loaded example ${ex.name}`);
      if (store.get().narrow) actions.closeSheet();
    },
    async applyScript(path, source) {
      const sess = await api.putScript({ path, source });
      applySession(sess, { selection: false });
      announcer.say("Script applied");
    },
    async save() {
      try {
        const p = await api.save();
        announcer.say(p ? `Saved ${p}` : "Saved");
      } catch (err) {
        announcer.say(`Save failed: ${err instanceof Error ? err.message : String(err)}`, { assertive: true });
        reportEngineError(err, "save");
      }
    },
    revealSpan(span: SourceSpan) {
      const s = store.get();
      if (s.narrow) store.set({ sheet: "script" });
      else store.set({ leftTab: "script" });
      store.set({ revealLine: { path: span.path, line: span.start_line, nonce: (s.revealLine?.nonce ?? 0) + 1 } });
    },
    setLeftTab(tab) {
      store.set({ leftTab: tab });
    },
    openSheet(tab: SheetTab) {
      store.set({ sheet: tab });
    },
    closeSheet() {
      store.set({ sheet: null });
    },
    // The Layers panel: every one of these is a store patch the
    // viewer answers with a redraw. Nothing here schedules an evaluation or
    // writes to the session; the overrides are remembered per document in
    // this browser (below, next to the camera).
    setLayerVisible(name, visible) {
      const next = new Map(store.get().layerOverrides);
      next.set(name, { ...next.get(name), visible });
      store.set({ layerOverrides: next });
      announcer.say(`${visible ? "Shown" : "Hidden"} layer ${name || "Other"}`);
    },
    setLayerOpacity(name, opacity) {
      const o = Math.max(0, Math.min(1, opacity));
      const next = new Map(store.get().layerOverrides);
      next.set(name, { ...next.get(name), opacity: o });
      store.set({ layerOverrides: next });
    },
    soloLayer(name) {
      const s = store.get();
      const layers = listLayers(s.model);
      if (layers.length < 2) return;
      const next = new Map(s.layerOverrides);
      if (isSoloed(name, effectiveLayers(s.model, s.layerOverrides))) {
        // Already alone: every layer back to the script's visibility, the
        // opacities kept.
        for (const l of layers) {
          const o = { ...next.get(l.name) };
          delete o.visible;
          if (Object.keys(o).length === 0) next.delete(l.name);
          else next.set(l.name, o);
        }
        announcer.say("Every layer shown as the script declared");
      } else {
        for (const l of layers) next.set(l.name, { ...next.get(l.name), visible: l.name === name });
        announcer.say(`Only layer ${name || "Other"} shown`);
      }
      store.set({ layerOverrides: next });
    },
    resetLayers() {
      if (store.get().layerOverrides.size === 0) return;
      store.set({ layerOverrides: new Map() });
      announcer.say("Layers reset to the script");
    },
    toggleHidden(bodyId) {
      const next = new Set(store.get().hiddenBodies);
      const hide = !next.has(bodyId);
      if (hide) next.add(bodyId);
      else next.delete(bodyId);
      store.set({ hiddenBodies: next });
      // Said as well as shown: from the keyboard there is no eye to watch.
      const body = store.get().model?.bodies.find((b) => b.id === bodyId);
      announcer.say(`${hide ? "Hidden" : "Shown"} ${body?.name || bodyId}`);
    },
    setView(preset: ViewPreset) {
      viewer.setView(preset);
    },
    fit() {
      viewer.fit();
    },
    focusSelection() {
      viewer.focusSelection(store.get().selected);
    },
    setViewMode(mode: ViewMode) {
      store.set({ viewMode: mode });
      announcer.say(`Display mode ${VIEW_MODES.find((m) => m.id === mode)?.label ?? mode}`);
    },
    cycleViewMode() {
      const i = VIEW_MODES.findIndex((m) => m.id === store.get().viewMode);
      actions.setViewMode((VIEW_MODES[(i + 1) % VIEW_MODES.length] as { id: ViewMode }).id);
    },
    toggleEdges() {
      actions.setViewMode(store.get().viewMode === "shaded_edges" ? "shaded" : "shaded_edges");
    },
    toggleGrid() {
      store.set({ showGrid: !store.get().showGrid });
    },
    toggleLabels() {
      store.set({ showLabels: !store.get().showLabels });
    },
    toggleBBox() {
      store.set({ showBBox: !store.get().showBBox });
    },
    toggleOrtho() {
      store.set({ orthographic: !store.get().orthographic });
      announcer.say(store.get().orthographic ? "Orthographic" : "Perspective");
    },
    toggleHud() {
      store.set({ showHud: !store.get().showHud });
    },
    setPlatesView(on: boolean) {
      const plates = store.get().model?.plates ?? [];
      if (on && plates.length === 0) return;
      if (on === store.get().platesView) return;
      store.set({ platesView: on });
      announcer.say(on ? `Plates view: ${plates.length} ${plates.length === 1 ? "plate" : "plates"}` : "Model view");
    },
    setSection(section: SectionState) {
      store.set({ section });
    },
    toggleSectionPanel(open?: boolean) {
      const next = open ?? !store.get().sectionPanel;
      if (next === store.get().sectionPanel) return;
      // Opening it on an uncut model cuts at half height, so the button does
      // something the moment it is pressed; the panel moves it from there.
      if (next && !isSectioned(store.get().section) && store.get().model) {
        const b = viewer.frameBounds;
        store.set({ section: { horizontal: { axis: 2, at: (b.min.z + b.max.z) / 2, removePositive: true }, vertical: null } });
      }
      store.set({ sectionPanel: next });
      announcer.say(next ? "Section panel open" : "Section panel closed");
    },
    async copyViewLink() {
      const href = currentViewLink();
      // The address bar first: it is the link, whatever the clipboard says.
      try {
        history.replaceState(history.state, "", href);
      } catch {
        /* a page that cannot rewrite its address still copies */
      }
      try {
        await navigator.clipboard.writeText(href);
        announcer.say("Link to this view copied");
      } catch {
        announcer.say("Link to this view is in the address bar");
      }
    },
    sceneBounds() {
      if (!store.get().model) return null;
      const b = viewer.frameBounds;
      return { min: [b.min.x, b.min.y, b.min.z], max: [b.max.x, b.max.y, b.max.z] };
    },
    setActivePlate(index: number | null) {
      const plates = store.get().model?.plates ?? [];
      const next = index !== null && plates[index] ? index : null;
      store.set({ activePlate: next });
      const p = next === null ? null : plates[next];
      announcer.say(p ? `Plate ${p.index}${p.name ? ` ${p.name}` : ""}` : "All plates");
    },
    async snapshot(opts = {}) {
      const s = store.get();
      const annotate = opts.annotate ?? s.selected.length > 0;
      try {
        const att = await viewer.snapshot({ annotate: annotate && s.selected.length > 0 });
        store.set({ pendingAttachments: [...store.get().pendingAttachments, att] });
        announcer.say("Snapshot queued for the next message");
        if (store.get().narrow) store.set({ sheet: "messages" });
      } catch (err) {
        announcer.say(`Snapshot failed: ${err instanceof Error ? err.message : String(err)}`, { assertive: true });
        reportEngineError(err, "snapshot");
      }
    },
    removeAttachment(index) {
      store.set({ pendingAttachments: store.get().pendingAttachments.filter((_a, i) => i !== index) });
    },
    setWheelMode(mode) {
      wheelModePref = mode;
      if (viewer) viewer.wheelMode = mode;
      try {
        localStorage.setItem(WHEEL_MODE_KEY, mode);
      } catch {
        /* ignore */
      }
    },
    wheelMode() {
      return viewer ? viewer.wheelMode : wheelModePref;
    },
    install() {
      return promptInstall(store);
    },
    setEnginePreference(pref: EnginePreference) {
      const s = store.get();
      if (s.enginePreference === pref) return;
      savePreference(pref);
      evaluator.setPreference(pref, s.session?.document.id ?? "");
      store.set({ enginePreference: pref, engineStatus: null });
      announcer.say(pref === "auto" ? "Engine: automatic" : pref === "local" ? "Engine: this machine" : "Engine: the server");
      // The choice only means anything on the next evaluation, so make one.
      scheduleEvaluate();
    },
    cancelEvaluate() {
      evaluator.cancel();
      store.set({ engineStatus: "Evaluation cancelled." });
      announcer.say("Evaluation cancelled");
    },
    focusMessages() {
      const s = store.get();
      if (s.narrow) store.set({ sheet: "messages" });
      store.set({ focusMessage: s.focusMessage + 1 });
    },
    reloadForBuild() {
      reloadNow();
    },
    dismissBuildUpdate() {
      const u = store.get().buildUpdate;
      if (!u || u.dismissed) return;
      // The offer collapses to a chip; it does not disappear. A page that
      // quietly forgot it was behind would leave a browser running an old
      // build with nobody the wiser.
      dismissedBuild = u.build;
      store.set({ buildUpdate: { ...u, dismissed: true } });
      announcer.say("Reload postponed. This page is still running the previous build.");
    },
  };

  // ------------------------------------------------------------- viewer

  const hud = createHud(store);
  // Where a published page's "back" link goes (`cadpole publish --home-url`),
  // checked here as well as at the bake: only an http(s) address is a link.
  const homeHref = safeHomeUrl(published?.view?.homeUrl ?? "", location.href);
  const toolbar = createToolbar(store, actions, homeHref ? { href: homeHref, label: published?.view?.homeLabel || "Back" } : null);
  const panels = {
    toolbar: toolbar.bar,
    tools: toolbar.tools,
    // Parameters first: their own sheet tab on a phone, above the
    // feature tree in the Model tab on a desk. The display layers
    // are the phone's next tab and sit under the tree on a desk.
    // On a published page a read-out of the values the bake was made
    // with, never a slider: nothing there can evaluate a new one.
    params: h("div", { class: "params-panel" }, api.mode === "published" ? createParamReadout(store) : createParamsPanel(store, actions)),
    layers: createLayersPanel(store, actions),
    model: h("div", { class: "model-panel" }, createTreePanel(store, actions)),
    script: createScriptPanel(store, actions),
    examples: createExamplesPanel(store, actions),
    api: createApiDocPanel(store),
    inspector: createInspectorPanel(store, actions),
    messages: createMessagesPanel(store, actions),
    diagnostics: createDiagnosticsPanel(store, actions),
  };
  if (api.mode === "published") {
    // Read-only: a read-out, and inert besides --
    // there is no engine here to evaluate a new one.
    panels.params.inert = true;
    panels.params.classList.add("read-only");
  }
  const layout = createLayout(store, actions, panels);
  appEl.classList.remove("loading");
  appEl.removeAttribute("aria-busy");
  // Above the shell rather than inside it: the bar is about the page, not
  // about the document, and it must not move when a panel opens.
  appEl.replaceChildren(createBuildBar(store, actions), layout.root);

  viewer = new Viewer(layout.viewport, {
    onHover: (c: Candidate | null) => actions.hover(c ? c.ref : null),
    onClick: (cands, additive, x, y) => {
      if (cands.length === 0) {
        picker.hide();
        selcard.setCandidates([]);
        if (!additive) actions.select([]);
        lastClick = null;
        return;
      }
      const keys = cands.map((c) => refKey(c.ref)).join("|");
      const now = performance.now();
      const s = store.get();
      const last = s.selected[s.selected.length - 1];
      const sameSpot =
        lastClick !== null && Math.hypot(lastClick.x - x, lastClick.y - y) < 8 && lastClick.keys === keys && now - lastClick.at < 4000;
      let pick: Candidate;
      if (sameSpot && last && cands.some((c) => refKey(c.ref) === refKey(last))) {
        pick = PickerPopover.next(cands, refKey(last)) ?? (cands[0] as Candidate);
      } else {
        pick = cands[0] as Candidate;
      }
      lastClick = { x, y, keys, at: now };
      actions.select([pick.ref], { additive });
      // On a phone the alternatives are chips on the selection card, under
      // the thumb, and not a popover under the finger that made the tap.
      if (store.get().narrow) {
        picker.hide();
        selcard.setCandidates(cands.length > 1 ? cands : []);
      } else if (cands.length > 1) picker.show(cands, x, y, refKey(pick.ref));
      else picker.hide();
    },
    onContext: (cands, x, y) => {
      if (cands.length === 0) {
        picker.hide();
        return;
      }
      const last = store.get().selected.at(-1);
      picker.show(cands, x, y, last ? refKey(last) : null, { focus: true });
    },
    onFrame: (stats) => {
      hud.frame(stats);
      // A readiness marker like `data-evals`: rendering is on demand, so a
      // counter that stops moving is a viewer that has finished its change
      // flash, and a screenshot test can wait for that instead of guessing.
      document.documentElement.dataset["frames"] = String(++frameCount);
    },
  });
  layout.viewport.appendChild(hud.el);
  layout.viewport.appendChild(createPlatesPanel(store, actions));
  layout.onInsets((insets) => viewer.setViewportInsets(insets));
  const sectionPanel = createSectionPanel(store, actions);
  layout.viewport.appendChild(sectionPanel);
  const selcard = createSelectionCard(store, actions);
  layout.viewport.appendChild(selcard.el);
  // What stands along the bottom of a narrow viewport -- the selection card,
  // or the Section panel -- lifts the axis triad and the view buttons clear
  // of itself (styles.css `--dock-h`).
  const syncDock = (): void => {
    let dock = 0;
    if (store.get().narrow) for (const el of [selcard.el, sectionPanel]) if (!el.hidden) dock = Math.max(dock, el.offsetHeight + 8);
    layout.viewport.style.setProperty("--dock-h", `${dock}px`);
    layout.viewport.classList.toggle("docked-section", store.get().narrow && !sectionPanel.hidden);
  };
  // On the next frame, not in the observer's own turn: what this writes
  // changes the very boxes being observed.
  let dockQueued = false;
  const dockObserver = new ResizeObserver(() => {
    if (dockQueued) return;
    dockQueued = true;
    requestAnimationFrame(() => {
      dockQueued = false;
      syncDock();
    });
  });
  dockObserver.observe(selcard.el);
  dockObserver.observe(sectionPanel);
  picker = new PickerPopover(layout.viewport, (c, additive) => actions.select([c.ref], { additive }));
  viewer.wheelMode = wheelModePref;

  // ------------------------------------------------------- subscriptions

  store.subscribe((s, prev) => {
    if (s.selected !== prev.selected || s.hovered !== prev.hovered) viewer.setHighlight(s.selected, s.hovered);
    if (!suppressSync && (s.selected !== prev.selected || s.hovered !== prev.hovered || s.note !== prev.note)) {
      selectionDirty = true;
      pushSelection();
    }
    if (s.overrides !== prev.overrides && !suppressSync) {
      scheduleEvaluate();
      pushParams();
    }
    if (s.hiddenBodies !== prev.hiddenBodies) viewer.setHidden(s.hiddenBodies);
    // The section: a redraw, and the address says it, so a link to
    // this page opens on the same cut (`?section=`, read at start-up).
    if (s.section !== prev.section) {
      viewer.setSection(s.section);
      try {
        const url = new URL(location.href);
        if (isSectioned(s.section)) url.searchParams.set("section", formatSection(s.section));
        else url.searchParams.delete("section");
        if (url.href !== location.href) history.replaceState(history.state, "", url);
      } catch {
        /* an address that cannot be rewritten is not worth an error */
      }
    }
    // The display layers follow the model and the panel: the
    // effective state is the script's defaults under the user's overrides,
    // and applying it is a redraw, never an evaluation.
    if (s.model !== prev.model || s.layerOverrides !== prev.layerOverrides) {
      viewer.setLayers(effectiveLayers(s.model, s.layerOverrides));
    }
    if (s.layerOverrides !== prev.layerOverrides) rememberLayers(s.layerOverrides);
    // The plates view follows the model: a model without plates
    // ends it, a model with fewer plates than the framed one frames all.
    if (s.model !== prev.model) {
      const n = s.model?.plates.length ?? 0;
      if (s.platesView && n === 0) store.set({ platesView: false, activePlate: null });
      else if (s.activePlate !== null && s.activePlate >= n) store.set({ activePlate: null });
    }
    if (s.platesView !== prev.platesView) {
      viewer.setPlatesView(s.platesView);
      // The selection and hover overlays are built where the bodies stand,
      // so they are rebuilt now that the bodies have moved.
      viewer.setHighlight(s.selected, s.hovered);
      if (s.platesView) viewer.framePlate(s.activePlate);
      document.documentElement.dataset["view"] = s.platesView ? "plates" : "model";
    } else if (s.platesView && s.activePlate !== prev.activePlate) {
      viewer.framePlate(s.activePlate);
    }
    if (s.viewMode !== prev.viewMode) {
      viewer.setViewMode(s.viewMode);
      document.documentElement.dataset["viewMode"] = s.viewMode;
      // Like the section: the address says the mode once it is not the one
      // this page opens in anyway, so the link in the bar opens as shown.
      try {
        const url = new URL(location.href);
        if (s.viewMode === (opening.mode && !opening.modeFromAddress ? opening.mode : "shaded_edges")) url.searchParams.delete("mode");
        else url.searchParams.set("mode", s.viewMode);
        if (url.href !== location.href) history.replaceState(history.state, "", url);
      } catch {
        /* an address that cannot be rewritten is not worth an error */
      }
    }
    if (s.showGrid !== prev.showGrid) viewer.setGridVisible(s.showGrid);
    if (s.showLabels !== prev.showLabels) viewer.setLabelsVisible(s.showLabels);
    if (s.showBBox !== prev.showBBox) viewer.setBBoxVisible(s.showBBox);
    if (s.orthographic !== prev.orthographic) viewer.setProjection(s.orthographic);
    // The script's own `units(...)` wins over the document's stored
    // unit: a house script reads in feet and inches whatever the session says.
    const unitNow = s.model?.display_units?.unit ?? s.session?.document.display_unit ?? "mm";
    const unitBefore = prev.model?.display_units?.unit ?? prev.session?.document.display_unit ?? "mm";
    if (unitNow !== unitBefore) {
      viewer.displayUnit = unitNow;
    }
    if (s.connection !== prev.connection && s.connection !== "connecting") announcer.say(`Connection ${s.connection}`);
    if (s.sheet !== prev.sheet && s.narrow) picker.hide();
  });

  // --------------------------------------------------------- keyboard

  const isTyping = (): boolean => {
    const el = document.activeElement as HTMLElement | null;
    if (!el) return false;
    const tag = el.tagName;
    return tag === "INPUT" || tag === "TEXTAREA" || tag === "SELECT" || el.isContentEditable;
  };
  window.addEventListener("keydown", (ev) => {
    if (ev.key === "Escape") {
      if (picker.open) {
        picker.hide();
        ev.preventDefault();
        return;
      }
      if (isTyping()) {
        (document.activeElement as HTMLElement).blur();
        return;
      }
      if (store.get().sheet) {
        actions.closeSheet();
        return;
      }
      if (store.get().selected.length) actions.clearSelection();
      return;
    }
    if ((ev.ctrlKey || ev.metaKey) && ev.key.toLowerCase() === "s") {
      ev.preventDefault();
      void actions.save();
      return;
    }
    if (isTyping() || ev.ctrlKey || ev.metaKey || ev.altKey) return;
    if (ev.shiftKey && ev.key.toLowerCase() === "s") {
      ev.preventDefault();
      void actions.snapshot();
      return;
    }
    if (ev.shiftKey) return;
    switch (ev.key.toLowerCase()) {
      case "f":
        actions.fit();
        break;
      case ".":
        actions.focusSelection();
        break;
      case "h":
        actions.setView("home");
        break;
      case "i":
        actions.setView("iso");
        break;
      case "7":
        actions.setView("top");
        break;
      case "1":
        actions.setView("front");
        break;
      case "3":
        actions.setView("right");
        break;
      case "5":
        actions.toggleOrtho();
        break;
      case "e":
        actions.toggleEdges();
        break;
      case "v":
        actions.cycleViewMode();
        break;
      case "x":
        actions.setViewMode(store.get().viewMode === "xray" ? "shaded_edges" : "xray");
        break;
      case "g":
        actions.toggleGrid();
        break;
      case "l":
        actions.toggleLabels();
        break;
      case "c":
        actions.toggleSectionPanel();
        break;
      case "b":
        actions.toggleBBox();
        break;
      case "p":
        actions.toggleHud();
        break;
      case "m":
        actions.focusMessages();
        break;
      default:
        return;
    }
    ev.preventDefault();
  });

  // ------------------------------------------------------------ events

  // The web bundle this page is running.
  //
  // Compared against every `build_changed` rather than a sequence number.
  // A sequence cannot survive a server restart, which happens often here:
  // the log restarts at 1, so a page holding the previous run's watermark
  // ignores every event of the new one, while a page holding none reloads on
  // the replayed backlog for ever. Both failures are in the Quint
  // model of this guard (`spec/reload.qnt`).
  let pageBuild = "";
  /**
   * The sequence the event log stood at when this page loaded.
   *
   * The fingerprint comparison above is necessary and not sufficient, and
   * `spec/reload.qnt` says why in as many words
   * (`aBuildIdGuardAloneLoopsOnAStaleEvent`): the stream replays from sequence
   * zero on connect, so a page that has already been through two rebuilds sees
   * *build 1's* `build_changed` on the way back up, and that fingerprint is
   * not the one it is running either. With a reload on the other end that is
   * an infinite loop; with a hot engine swap on the other end it is worse, a
   * page quietly swapping itself back onto an engine two builds old. The
   * hot-reload e2e suite covers the second: it rebuilds several
   * times against one session.
   *
   * `Session.last_event_seq`, not `revision`: they count different things, and
   * the model covers that too.
   */
  let seenThrough = 0;

  // ---------------------------------------------- hot reload

  /**
   * Whether the user has touched this page since it loaded.
   *
   * This is the whole difference between "reloading now costs nothing" and
   * "reloading now lands in the middle of a thought". It is deliberately the
   * coarsest possible measure: any pointer, key or wheel event counts, because
   * the cost of being wrong in one direction is a lost half-typed message and
   * in the other is a bar nobody needed.
   */
  let interacted = false;
  /**
   * The build the user answered "Not now" to.
   *
   * A page that is behind is told so again on every session refresh: the
   * session view names the build the server is serving, every write comes
   * back with the session, and each of those re-plans the same change. The
   * plan is right every time; the *offer* must not be made again for a build
   * the user has already answered, or the bar comes back with both buttons
   * seconds after it was dismissed and the screen reader hears about the same
   * build once per slider move. Only a build newer than this one re-opens the
   * question -- and a dismissed build is never taken unasked either, however
   * idle the tab looks later.
   */
  let dismissedBuild = "";
  for (const kind of ["pointerdown", "keydown", "wheel"] as const) {
    window.addEventListener(
      kind,
      () => {
        interacted = true;
      },
      { passive: true, capture: true },
    );
  }

  /** Builds are applied one at a time, in order. Two rebuilds a second apart
   * must not race two engine workers into the same page. */
  let buildQueue: Promise<unknown> = Promise.resolve();
  let hotNoteTimer = 0;

  function onBuildChanged(build: string): void {
    buildQueue = buildQueue
      .then(() => applyBuild(pageBuild, build, hotContext))
      .catch((err: unknown) => reportEngineError(err, "hot_reload"));
  }

  /** `p`, but never longer than `ms`, and never rejecting: a best effort with a deadline. */
  function settledWithin(p: Promise<unknown>, ms: number): Promise<void> {
    return new Promise<void>((resolve) => {
      const timer = window.setTimeout(resolve, ms);
      void p.then(
        () => {
          window.clearTimeout(timer);
          resolve();
        },
        () => {
          window.clearTimeout(timer);
          resolve();
        },
      );
    });
  }

  /** One line about something that was replaced, gone again in a moment. */
  function showHotNote(text: string): void {
    store.set({ hotNote: text });
    window.clearTimeout(hotNoteTimer);
    hotNoteTimer = window.setTimeout(() => store.set({ hotNote: null }), 6_000);
  }

  /**
   * A build that needs a page load: offered, not taken.
   *
   * Except in the two cases where there is provably nothing to interrupt. A
   * hidden tab is not mid-thought by definition; a page nobody has touched
   * since it loaded holds nothing a reload would cost, because the camera is
   * restored either way and everything else that could be lost is something
   * the user made. Anywhere else the bar goes up and waits.
   */
  function offerReload(plan: BuildPlan, build: string): void {
    // What the reload will actually cost, when it is something the user made.
    // A reload parks the view mode, the panels, the hidden bodies and the
    // composer draft, and lists what is still lost -- but queued snapshots
    // are neither parked nor listed, and they are the one thing in that gap
    // the user built by hand. They are base64 PNGs, too big to park safely,
    // so the honest thing is to say so before they go rather than after.
    const queued = store.get().pendingAttachments.length;
    const cost = queued > 0 ? ` (${queued} queued snapshot${queued === 1 ? "" : "s"} will be lost)` : "";
    const reason = `${plan.reloadBecause || describePlan(plan)}${cost}`;
    if (build === dismissedBuild) {
      // Already answered, for this exact build: the chip stays a chip, and
      // nothing is announced.
      store.set({ buildUpdate: { build, reason, dismissed: true } });
      return;
    }
    const unattended = document.visibilityState === "hidden" || (!interacted && !store.get().evaluating);
    if (unattended) {
      announcer.say("A new build is available. Reloading.", { assertive: true });
      reloadNow();
      return;
    }
    store.set({ buildUpdate: { build, reason, dismissed: false } });
    announcer.say("A new build of Cadpole is available. Reload when you are ready.");
  }

  function reloadNow(): void {
    // Explicitly, not just via `pagehide`: this is the reload that happens
    // while the user is mid-thought, so their camera, view mode, hidden
    // bodies, open panel and half-typed message must all survive it.
    rememberCamera();
    rememberUiState();
    // A slider moved in the last 300 ms is still on its debounce timer, and
    // the overrides are session state: reloading over it puts the page back
    // with the value the user had just changed. Bounded, because a reload
    // that waits on a server that is not answering is worse than a reload
    // that loses one edit -- and `keepalive` means the write can still land
    // after the page is gone.
    const writes = settledWithin(flushWrites({ keepalive: true }), 2_000);
    // Drop the service worker's caches too. It serves code network-first,
    // but a reload that races a slow network would still be handed the old
    // engine from the cache fallback -- a phone loading a broken engine over
    // and over.
    void Promise.all([writes, dropServiceWorkerCaches()]).then(() => window.location.reload());
  }

  const hotContext: HotReloadContext = {
    currentEngine: () => engine,
    abandonInFlight: () => {
      // Both engine slots, because both hold work that came out of the engine
      // being retired. The runner drops an abandoned job's result even when
      // the work finishes anyway, which is what makes "a result from the old
      // engine is never displayed against the new one" true by construction
      // rather than by a comparison somebody has to remember to write.
      jobs.cancel(EVALUATE_SLOT);
      jobs.cancel(APIDOC_SLOT);
    },
    installEngine: (next, doc, reevaluate) => {
      engine = next;
      engineEpoch += 1;
      document.documentElement.dataset["build"] = next.buildId;
      document.documentElement.dataset["localEngine"] = next.kind;
      document.documentElement.dataset["engineEpoch"] = String(engineEpoch);
      evaluator.setLocalEngine(next);
      // The health check already asked the new engine for its API reference,
      // so this costs nothing and keeps the API panel honest about which
      // engine's builtins it is describing.
      store.set({ apiDoc: doc });
      // Yes, re-evaluate -- the triangles on screen were computed by the
      // engine that was just replaced, and "the browser went on running the
      // broken copy" is the failure this whole feature exists to prevent.
      // The old picture stays up until the new one lands, the camera and the
      // selection do not move, and the cost is one evaluation against a reload's one evaluation plus
      // 4.5 MB of wasm and a page boot.
      if (reevaluate) evaluateNow();
    },
    adoptBuild: (b) => {
      pageBuild = b;
      store.set({ buildUpdate: null });
    },
    note: (text) => {
      showHotNote(text);
      announcer.say(text);
    },
    offerReload,
    report: (err, operation) => reportEngineError(err, operation),
  };

  const refreshSession = debounce((reason: "script" | "params" | "selection" | "clients" | "messages") => {
    api
      .getSession()
      .then((sess) => applySession(sess, { selection: reason === "selection" || reason === "clients" }))
      .catch((err: unknown) => reportEngineError(err, "session_refresh"));
  }, 150);

  function onEvent(e: CadpoleEvent): void {
    switch (e.kind) {
      case "script_changed":
        refreshSession("script");
        break;
      case "params_changed":
        refreshSession("params");
        break;
      case "selection_changed":
        refreshSession("selection");
        break;
      case "message": {
        if (norm.isMessageLike(e.data)) {
          const m = norm.message(e.data);
          appendMessage(m);
          if (m.from === "assistant") announcer.say(`Assistant: ${m.text}`);
        } else {
          refreshSession("clients");
        }
        break;
      }
      case "assistant_listening": {
        // The switch moved, here or in another tab.
        const s = store.get();
        const on = Boolean((e.data as { listening?: unknown } | null)?.listening);
        if (s.session && s.session.assistantListening !== on) store.set({ session: { ...s.session, assistantListening: on } });
        break;
      }
      case "reply_stream": {
        // A reply in progress grew. A frame that does not continue what is
        // held (one was missed) is answered by reading the session.
        const s = store.get();
        if (!s.session) break;
        const { messages, resync } = applyFrame(s.session.messages, norm.replyFrame(e.data));
        if (resync) refreshSession("messages");
        else if (messages !== s.session.messages) store.set({ session: { ...s.session, messages } });
        break;
      }
      case "client_joined":
      case "client_left":
        refreshSession("clients");
        break;
      case "solve_finished":
        // A solve job landed and the server has evaluated the
        // session's document again, so its answer is in the server's memo:
        // evaluating now shows it. A replayed event from before this page
        // loaded is history, and the page's first evaluation already read
        // whatever that job left.
        if (e.seq > seenThrough) evaluateNow();
        break;
      case "build_changed": {
        // The server noticed the built web UI change underneath us. A page
        // running an engine that has since been rebuilt would go on showing
        // the old engine's bugs. The fingerprint says which files moved, and a
        // stylesheet or an engine can be replaced with the DOM, the camera,
        // the selection, the messages and the script all left alone.
        const build = (e.data as { build?: unknown } | null)?.build;
        // A replayed event naming the build we are already running is history,
        // and so is anything that was in the log before this page loaded --
        // including a `build_changed` for a build *older* than the one being
        // served, which is the case a fingerprint comparison alone cannot see.
        if (typeof build !== "string" || build === pageBuild) break;
        if (e.seq <= seenThrough) break;
        onBuildChanged(build);
        break;
      }
      default:
        break;
    }
  }

  /** The debug seam's view of the viewport: what covers it, the orbit, and where a model point is drawn. */
  function viewportDebug(): unknown {
    return {
      insets: viewer.viewportInsets(),
      pose: viewer.pose(),
      camera: viewer.cameraState(),
      screenPoint: (p: { x: number; y: number; z: number }) => viewer.screenPoint(p),
      // The picture on screen against the canvas it is on: the buffer's
      // size now, and its size when the picture in it was drawn.
      buffer: { width: viewer.canvas.width, height: viewer.canvas.height },
      drawn: viewer.drawnSize(),
      backend: viewer.backend,
    };
  }

  /** The address that opens this page on the view on screen. */
  function currentViewLink(): string {
    const cam = viewer.cameraState();
    const s = store.get();
    return viewLink(location.href, {
      eye: cam.eye,
      target: cam.target,
      mode: s.viewMode === "shaded_edges" && !opening.mode ? null : s.viewMode,
      section: isSectioned(s.section) ? formatSection(s.section) : "",
    });
  }

  // ---------------------------------------------------------- start up

  await viewer.init();
  restoreUiState();
  // Declared before the restore below reads it: a `const` further down
  // would be in its temporal dead zone here, and the read's `try` would
  // swallow the ReferenceError as "nothing saved".
  const LAYERS_KEY = `cadpole:layers:${probe.session.document.id || probe.session.id}`;
  restoreLayers();
  // The API reference is a job like everything else, on a `coalesce` slot: the
  // answer depends only on which engine build is loaded, so a second request
  // for the same build joins the first rather than starting another.
  void jobs
    .submit<ApiDoc>(APIDOC_SLOT, "coalesce", engine.buildId, (_emit, signal) => engine.apiDoc(signal))
    .then((doc) => store.set({ apiDoc: doc }))
    .catch((err: unknown) => {
      if (!isAbandoned(err)) reportEngineError(err, "api_doc");
    });
  applySession(probe.session, { selection: true, initial: true });

  // Put the camera back where it was, or fit if this page has never seen this
  // document.
  //
  // Live reload made this matter. Every rebuild reloads the page, and a fresh
  // page fitted to home and eased into it, so the viewport visibly reset and
  // then flew somewhere on every reload — while the user was looking at a
  // particular corner of the model, which is exactly when they least want the
  // camera moved. A restored pose is snapped rather than animated, because an
  // eased restore looks like the same reset with extra steps.
  const CAMERA_KEY = `cadpole:camera:${probe.session.document.id || probe.session.id}`;
  const savedPose = ((): ReturnType<typeof viewer.pose> | null => {
    try {
      const raw = sessionStorage.getItem(CAMERA_KEY);
      if (!raw) return null;
      const p = JSON.parse(raw) as ReturnType<typeof viewer.pose>;
      return Number.isFinite(p?.distance) && Number.isFinite(p?.theta) && Number.isFinite(p?.phi) && p.target ? p : null;
    } catch {
      // Private browsing, cleared storage, a half-written value: fit instead.
      return null;
    }
  })();
  // How a published page asks to open: what the camera frames, the
  // grid, a section, and where Home looks from. Set before the first fit.
  const view = published?.view ?? null;
  if (view) {
    viewer.setFrameLayers(view.frameLayers);
    if (view.grid === "off") store.set({ showGrid: false });
    else if (view.grid === "dim") viewer.setGridStrength(0.35);
  }
  // The cut, the display mode and the camera (src/web/src/opening.ts): the
  // address's on any page, over the publisher's. A value that does not
  // parse is a console line, not a broken page.
  if (opening.ignored.length) console.warn("cadpole: ignoring", opening.ignored.join(", "));
  if (opening.section) {
    try {
      // The panel opens with the cut where there is room for both. On a
      // phone it would cover the lower half of the model the link was sent to
      // show, so the cut alone opens and the viewport's section button
      // (ui/layout.ts) brings the panel up.
      store.set({ section: parseSection(opening.section), sectionPanel: !store.get().narrow });
    } catch (err) {
      console.warn("cadpole: ignoring section", opening.section, err);
    }
  }
  // A mode from the address is the one asked for now; the publisher's is
  // where the page starts, until this tab has chosen its own.
  if (opening.mode && (opening.modeFromAddress || !restoredViewMode)) store.set({ viewMode: opening.mode });
  document.documentElement.dataset["viewMode"] = store.get().viewMode;
  const openingView = (): void => {
    const cam = opening.cameraFromAddress ? null : (published?.scriptCamera ?? null);
    if (opening.eye) {
      // `--eye` or `?eye=`: exactly there; Home comes back to it, framed.
      const t = opening.target ?? viewer.frameBounds.getCenter(new Vector3());
      const target = new Vector3(t.x, t.y, t.z);
      const eye = new Vector3(opening.eye.x, opening.eye.y, opening.eye.z);
      if (eye.distanceToSquared(target) > 0) {
        viewer.setHomeDirection(eye.clone().sub(target));
        viewer.lookAt(eye, target);
        return;
      }
    }
    const preset = opening.preset && opening.preset !== "home" ? opening.preset : null;
    if (preset) {
      viewer.setHomeDirection(new Vector3(...VIEW_DIRS[preset]));
    } else if (cam) {
      // The script's own camera: its direction, framed on what the page
      // frames when the publisher named framing layers, exactly where it
      // stands otherwise.
      const target = cam.target ? new Vector3(cam.target.x, cam.target.y, cam.target.z) : viewer.frameBounds.getCenter(new Vector3());
      const eye = new Vector3(cam.eye.x, cam.eye.y, cam.eye.z);
      viewer.setHomeDirection(eye.clone().sub(target));
      if ((view?.frameLayers.length ?? 0) === 0) {
        viewer.lookAt(eye, target);
        return;
      }
    }
    viewer.setView("home");
  };
  const unsubFit = store.select(
    (s) => s.model,
    (m) => {
      if (m) {
        // A camera in the address is the view asked for, so it beats the
        // pose this tab remembered; an unaddressed page comes back where
        // it was.
        if (opening.cameraFromAddress) openingView();
        else if (savedPose) viewer.restorePose(savedPose);
        else if (published) openingView();
        else viewer.setView("home");
        window.setTimeout(unsubFit, 0);
      }
    },
  );
  // Store on the way out rather than on every frame: a camera write per frame
  // of an orbit is a lot of JSON for something only a reload ever reads.
  const rememberCamera = (): void => {
    try {
      sessionStorage.setItem(CAMERA_KEY, JSON.stringify(viewer.pose()));
    } catch {
      /* storage may be unavailable; losing the camera is not worth an error */
    }
  };
  window.addEventListener("pagehide", rememberCamera);
  document.addEventListener("visibilitychange", () => {
    if (document.visibilityState === "hidden") rememberCamera();
  });

  /**
   * The rest of what a reload would otherwise throw away: the view
   * mode, which bodies are hidden, which panel is open, which script is
   * showing and whatever is half-typed in the composer. Most rebuilds do not
   * reload at all, but the ones that do should cost as little as the ones
   * that do not.
   *
   * Three things are deliberately *not* here, because each already survives
   * for a better reason than this record: the **selection** and the
   * **parameter overrides** are session state on the server, and the
   * **messages** come back with the session. Writing local copies of them
   * would create a second source of truth that can disagree with the first.
   *
   * `sessionStorage`, per document, like the camera: a tab's worth of state
   * that a new tab should not inherit.
   */
  interface SavedUi {
    viewMode: string;
    showGrid: boolean;
    showLabels: boolean;
    showBBox: boolean;
    orthographic: boolean;
    showHud: boolean;
    hidden: string[];
    leftTab: string;
    sheet: string | null;
    activeScript: string;
    /** The composer's unsent text: the one thing here the user actually typed. */
    draft: string;
  }

  function composer(): HTMLTextAreaElement | null {
    return document.querySelector<HTMLTextAreaElement>("textarea.msg-input");
  }

  function rememberUiState(): void {
    const s = store.get();
    const saved: SavedUi = {
      viewMode: s.viewMode,
      showGrid: s.showGrid,
      showLabels: s.showLabels,
      showBBox: s.showBBox,
      orthographic: s.orthographic,
      showHud: s.showHud,
      hidden: [...s.hiddenBodies],
      leftTab: s.leftTab,
      sheet: s.sheet,
      activeScript: s.activeScript,
      draft: composer()?.value ?? "",
    };
    try {
      sessionStorage.setItem(UI_STATE_KEY(probe.session.document.id || probe.session.id), JSON.stringify(saved));
    } catch {
      /* storage may be unavailable; this is a nicety, not a correctness thing */
    }
  }

  function restoreUiState(): void {
    let saved: Partial<SavedUi> | null = null;
    try {
      const raw = sessionStorage.getItem(UI_STATE_KEY(probe.session.document.id || probe.session.id));
      saved = raw ? (JSON.parse(raw) as Partial<SavedUi>) : null;
    } catch {
      saved = null;
    }
    if (!saved) return;
    // Every field is checked rather than trusted: this is storage a previous
    // build wrote, and a page that crashes on its own leftovers is a worse
    // bug than a page that forgot where its panels were.
    const patch: Partial<AppState> = {};
    if (VIEW_MODES.some((m) => m.id === saved.viewMode)) {
      patch.viewMode = saved.viewMode as ViewMode;
      restoredViewMode = true;
    }
    if (typeof saved.showGrid === "boolean") patch.showGrid = saved.showGrid;
    if (typeof saved.showLabels === "boolean") patch.showLabels = saved.showLabels;
    if (typeof saved.showBBox === "boolean") patch.showBBox = saved.showBBox;
    if (typeof saved.orthographic === "boolean") patch.orthographic = saved.orthographic;
    if (typeof saved.showHud === "boolean") patch.showHud = saved.showHud;
    if (Array.isArray(saved.hidden) && saved.hidden.every((b) => typeof b === "string")) {
      patch.hiddenBodies = new Set(saved.hidden);
    }
    if (saved.leftTab === "model" || saved.leftTab === "script" || saved.leftTab === "examples" || saved.leftTab === "api") {
      patch.leftTab = saved.leftTab;
    }
    if (saved.sheet !== null && SHEET_TABS.includes(saved.sheet as SheetTab)) patch.sheet = saved.sheet as SheetTab;
    if (typeof saved.activeScript === "string" && saved.activeScript) patch.activeScript = saved.activeScript;
    if (Object.keys(patch).length > 0) store.set(patch);
    if (typeof saved.draft === "string" && saved.draft) {
      const input = composer();
      if (input && !input.value) input.value = saved.draft;
    }
  }

  /**
   * The Layers panel's overrides, per document, in `localStorage`
   * rather than the session-scoped record above: a layer someone switched
   * off to look at a site is a standing choice about that document, not a
   * tab's worth of state, and it should be there tomorrow on the phone as
   * well as after a reload. Not `ViewState`: the web UI never writes the
   * document's view state (its camera lives here too), and a proto field
   * nobody writes would be a second, empty source of truth.
   */
  function rememberLayers(overrides: ReadonlyMap<string, LayerOverride>): void {
    try {
      if (overrides.size === 0) localStorage.removeItem(LAYERS_KEY);
      else localStorage.setItem(LAYERS_KEY, JSON.stringify(Object.fromEntries(overrides)));
    } catch {
      /* storage may be unavailable; forgetting a switch is not worth an error */
    }
  }

  function restoreLayers(): void {
    let saved: unknown = null;
    try {
      const raw = localStorage.getItem(LAYERS_KEY);
      saved = raw ? (JSON.parse(raw) as unknown) : null;
    } catch {
      saved = null;
    }
    if (!saved || typeof saved !== "object") return;
    // Checked field by field: this is storage a previous build wrote.
    const next = new Map<string, LayerOverride>();
    for (const [name, v] of Object.entries(saved as Record<string, unknown>)) {
      if (!v || typeof v !== "object") continue;
      const o = v as Record<string, unknown>;
      const out: LayerOverride = {};
      if (typeof o["visible"] === "boolean") out.visible = o["visible"];
      if (typeof o["opacity"] === "number" && Number.isFinite(o["opacity"])) out.opacity = Math.max(0, Math.min(1, o["opacity"]));
      if (Object.keys(out).length > 0) next.set(name, out);
    }
    if (next.size > 0) store.set({ layerOverrides: next });
  }

  window.addEventListener("pagehide", () => {
    rememberUiState();
    // The page may not come back. Anything on a debounce timer goes now, with
    // `keepalive`, so the write outlives the document that asked for it.
    void flushWrites({ keepalive: true });
  });
  // A tab being hidden is the other way a page stops running without a
  // `pagehide`: on mobile it is often the last event a tab ever gets. So the
  // panel state is parked here too, and the moment a panel changes:
  // a phone that kills the tab in the background hands back the same sheet.
  document.addEventListener("visibilitychange", () => {
    if (document.visibilityState === "hidden") {
      rememberUiState();
      void flushWrites({ keepalive: true });
    }
  });
  store.subscribe((s, prev) => {
    if (s.sheet !== prev.sheet || s.leftTab !== prev.leftTab) rememberUiState();
  });
  // What this page is running, from the session it just loaded.
  //
  // `lastEventSeq`, not `revision`. They count different things -- events
  // against content changes -- and the Quint model of this guard shows that
  // the revision can sit below the sequence it is compared against, so a
  // `build_changed` replayed on connect would get through and the page would
  // reload for ever.
  pageBuild = probe.session.webBuild;
  seenThrough = probe.session.lastEventSeq;
  api.subscribe(
    0,
    onEvent,
    (status) => store.set({ connection: status }),
    // The token this tab holds is not this server's any more, which in
    // practice means `cadpole serve` was restarted. Until now the only sign was
    // the pill reading "offline" -- indistinguishable from a network blip --
    // while every edit silently stopped syncing, and the banner explaining it
    // appeared only after a reload.
    () =>
      store.set({
        engineNotice:
          "The Cadpole server restarted, so this tab's token is no longer valid and your edits have stopped syncing. `cadpole serve` prints a new URL every time it starts — open the one it printed.",
      }),
    // The same tab reconnected to a restarted server (a pinned token or
    // `--no-auth`): a new session whose event log starts again at 1. What the
    // page shows -- messages, script, parameters -- is the old run's, and the
    // watermark the `solve_finished` / `build_changed` guards compare against
    // counts in the old log, so it would hide the new run's events until its
    // log caught up. Adopt the new run's state and its watermark.
    (sess) => {
      seenThrough = sess.lastEventSeq;
      // Message seqs count from 1 again too, so the old run's messages must
      // not be matched against the new run's by seq: a carried-over
      // screenshot would land on a different message.
      const old = store.get().session;
      if (old) store.set({ session: { ...old, messages: [] } });
      applySession(sess, { selection: true });
    },
  );
  // A published page is a page: no service worker, and `cadpole publish` ships
  // none for it to register.
  if (api.mode !== "published") setupPwa(store);
  trackKeyboardInset();
  keepFocusedInputVisible();
  preventPageZoom();
  document.documentElement.classList.add("ready");
}

/** The bake a published page opened, for how it asks to be shown. */
let published: Baked | null = null;

/**
 * Fetch and decode the bake, and stand up the engine and the session that
 * serve it. A bake that cannot be read is a page that cannot start,
 * and says so through the fatal screen below rather than a blank viewport.
 */
async function startPublished(url: string): Promise<[LoadedEngine, Probe]> {
  // The splash says how far along a large model is, then that it is being
  // unpacked: the two waits a phone has before there is anything to look at.
  const note = document.querySelector<HTMLElement>("#app.loading .splash-note");
  const say = (text: string): void => {
    if (note) note.textContent = text;
  };
  say("Loading model…");
  let shown = -1;
  const baked = await fetchBaked(url, (loaded, fraction) => {
    const pct = fraction === null ? null : Math.floor(fraction * 100);
    if (pct === shown) return;
    shown = pct ?? -1;
    say(pct === null ? `Loading model… ${(loaded / 1e6).toFixed(1)} MB` : pct >= 100 ? "Preparing model…" : `Loading model… ${pct}%`);
  });
  published = baked;
  const api = new PublishedApi(baked);
  if (!document.querySelector('meta[name="cadpole-baked"]')) document.title = `${baked.name} · Cadpole`;
  document.documentElement.dataset["sources"] = baked.scripts.length > 0 ? "1" : "0";
  return [{ engine: new BakedEngine(baked), notice: null, failure: null }, { api, session: await api.getSession() }];
}

main().catch((err: unknown) => {
  console.error(err);
  // Start-up is where the reporter may not exist yet; when it does, a UI that
  // never came up is worth saying out loud.
  const d = describeThrown(err);
  reporter()?.report({ kind: "CLIENT_FAILURE_KIND_EXCEPTION", message: d.message, stack: d.stack, operation: "startup" });
  const el = document.getElementById("app") ?? document.body;
  el.replaceChildren(
    h(
      "div",
      { class: "fatal", role: "alert" },
      h("h1", null, "Cadpole could not start"),
      h("pre", null, err instanceof Error ? (err.stack ?? err.message) : String(err)),
    ),
  );
});
