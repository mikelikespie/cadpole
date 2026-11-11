// Bottom strip: diagnostics (click to jump to the script line), evaluation
// time, engine kind and connection status. Error/warning counts are
// announced through a live region.

import type { Actions } from "../actions.js";
import type { EngineKind } from "../engine.js";
import type { AppState, AppStore, ConnectionStatus } from "../state.js";
import type { Diagnostic } from "../types.js";
import { h, replaceChildren } from "./dom.js";

/** How each engine names itself in the status bar. */
const ENGINE_LABEL: Record<EngineKind, string> = {
  worker: "worker",
  wasm: "main thread",
  remote: "server",
  mock: "mock engine",
  baked: "published",
};

/** A display unit as a reader would say it. */
const UNIT_NAME: Record<string, string> = {
  mm: "millimetres",
  cm: "centimetres",
  m: "metres",
  in: "inches",
  ft: "feet",
  "ft-in": "feet and inches",
};

const STATUS_LABEL: Record<ConnectionStatus, string> = {
  connecting: "connecting…",
  live: "live",
  reconnecting: "reconnecting…",
  offline: "offline",
  static: "static mode",
};

export function createDiagnosticsPanel(store: AppStore, actions: Actions): HTMLElement {
  const summary = h("div", { class: "diag-summary", aria: { live: "polite" } });
  const list = h("ul", { class: "diag-list", aria: { label: "Diagnostics" } });
  const evalInfo = h("span", { class: "status-item eval" });
  const cancelBtn = h(
    "button",
    {
      type: "button",
      class: "btn btn-quiet eval-cancel",
      title: "Stop this evaluation. What is on screen stays on screen.",
      on: { click: () => actions.cancelEvaluate() },
    },
    "Cancel",
  );
  cancelBtn.hidden = true;
  const engine = h("span", { class: "status-item engine" });
  const conn = h("span", { class: "status-item conn" });
  const rev = h("span", { class: "status-item rev" });
  const toggle = h(
    "button",
    {
      type: "button",
      class: "btn btn-quiet diag-toggle",
      aria: { expanded: "false", controls: "diag-list" },
      on: { click: () => setOpen(!open) },
    },
    "Details",
  );
  list.id = "diag-list";
  let open = false;
  const setOpen = (v: boolean): void => {
    open = v;
    list.hidden = !v;
    toggle.setAttribute("aria-expanded", String(v));
    root.classList.toggle("open", v);
  };
  const root = h(
    "section",
    { class: "diagnostics", aria: { label: "Diagnostics and status" } },
    h("div", { class: "diag-bar" }, summary, toggle, h("div", { class: "status" }, evalInfo, cancelBtn, engine, conn, rev)),
    list,
  );
  list.hidden = true;

  function renderDiagnostics(diags: Diagnostic[]): void {
    const errors = diags.filter((d) => d.severity === "error").length;
    const warnings = diags.filter((d) => d.severity === "warning").length;
    const infos = diags.length - errors - warnings;
    const parts: HTMLElement[] = [];
    if (errors) parts.push(h("span", { class: "chip sev-error" }, `${errors} error${errors === 1 ? "" : "s"}`));
    if (warnings) parts.push(h("span", { class: "chip sev-warning" }, `${warnings} warning${warnings === 1 ? "" : "s"}`));
    if (infos) parts.push(h("span", { class: "chip sev-info" }, `${infos} note${infos === 1 ? "" : "s"}`));
    if (parts.length === 0) parts.push(h("span", { class: "chip ok" }, "No issues"));
    replaceChildren(summary, ...parts);
    toggle.hidden = diags.length === 0;
    if (diags.length === 0) setOpen(false);
    else if (errors > 0 && !open) setOpen(true);
    replaceChildren(
      list,
      ...diags.map((d) =>
        h(
          "li",
          { class: `diag sev-${d.severity}` },
          h("span", { class: "diag-sev", aria: { label: d.severity } }, d.severity === "error" ? "✖" : d.severity === "warning" ? "▲" : "ℹ"),
          d.span
            ? h(
                "button",
                {
                  type: "button",
                  class: "link-btn diag-loc",
                  title: "Show in script",
                  on: { click: () => d.span && actions.revealSpan(d.span) },
                },
                `${d.span.path}:${d.span.start_line}`,
              )
            : null,
          h("span", { class: "diag-msg" }, d.message),
          d.feature_id ? h("span", { class: "diag-feature muted" }, d.feature_id) : null,
        ),
      ),
    );
  }

  /**
   * What the status line says about the last evaluation.
   *
   * It names the engine that ran and how long it took, because "why is this
   * slow" has to be answerable from the screen: the engine can change between
   * one evaluation and the next without the user doing anything.
   */
  function evalText(s: AppState): string {
    if (s.evaluating) {
      const stage = s.evalInfo && s.evalInfo.stages > 1 && s.evalInfo.stage === 0 ? "refining" : "evaluating";
      return `${stage} on ${ENGINE_LABEL[s.engineKind]}…`;
    }
    if (s.evalInfo?.error) return `evaluation failed: ${s.evalInfo.error}`;
    if (!s.evalInfo) return "";
    const ms = s.evalInfo.millis;
    const time = ms >= 1000 ? `${(ms / 1000).toFixed(1)} s` : `${ms.toFixed(0)} ms`;
    const coarse = s.evalInfo.stages > 1 && s.evalInfo.stage === 0 ? ` (coarse ${s.evalInfo.tolerance} mm)` : "";
    return `${ENGINE_LABEL[s.engineKind]} ${time}${coarse}`;
  }

  function render(s: AppState): void {
    evalInfo.textContent = evalText(s);
    const why = s.evalInfo?.why ? `Engine chosen because: ${s.evalInfo.why}.` : "";
    const engineMs = s.evalInfo && s.evalInfo.engineMillis > 0 ? ` The engine itself spent ${s.evalInfo.engineMillis.toFixed(0)} ms; the rest is transport.` : "";
    evalInfo.title = `${why}${engineMs}`.trim();
    evalInfo.classList.toggle("sev-error", Boolean(s.evalInfo?.error));
    evalInfo.classList.toggle("busy", s.evaluating);
    cancelBtn.hidden = !s.cancellable;
    engine.textContent = s.engineStatus ?? "";
    engine.hidden = !s.engineStatus;
    engine.classList.toggle("sev-warning", Boolean(s.engineStatus));
  }

  // A published page: there is no evaluation time, connection or
  // revision worth a reader's attention, but there is a unit, and every
  // readout on the page is in it.
  const published = store.get().apiMode === "published";
  const unitItem = h("span", { class: "status-item units", title: "Every length on this page is written in this unit" });
  unitItem.hidden = !published;
  conn.after(unitItem);
  const publishedStatus = (s: AppState): void => {
    if (!published) return;
    evalInfo.textContent = "published · read-only";
    evalInfo.title = "Evaluated ahead of time by cadpole publish: nothing here re-evaluates";
    conn.hidden = true;
    rev.hidden = true;
    unitItem.textContent = `lengths in ${UNIT_NAME[s.model?.display_units?.unit ?? "mm"] ?? s.model?.display_units?.unit ?? "mm"}`;
  };

  store.subscribe((s, prev) => {
    if (s.model !== prev.model) renderDiagnostics(s.model?.diagnostics ?? []);
    if (published && (s.model !== prev.model || s.evalInfo !== prev.evalInfo)) queueMicrotask(() => publishedStatus(store.get()));
    if (
      s.evalInfo !== prev.evalInfo ||
      s.evaluating !== prev.evaluating ||
      s.engineKind !== prev.engineKind ||
      s.engineStatus !== prev.engineStatus ||
      s.cancellable !== prev.cancellable
    ) {
      render(s);
    }
    if (s.connection !== prev.connection) {
      conn.textContent = STATUS_LABEL[s.connection];
      conn.dataset["state"] = s.connection;
    }
    if (s.session?.revision !== prev.session?.revision) rev.textContent = s.session ? `rev ${s.session.revision}` : "";
  });
  renderDiagnostics(store.get().model?.diagnostics ?? []);
  render(store.get());
  conn.textContent = STATUS_LABEL[store.get().connection];
  publishedStatus(store.get());
  return root;
}
