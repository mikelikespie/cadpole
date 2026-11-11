// Inspector: details for the current selection (face / edge / body / feature /
// param), links to related entities, source span, and the selection note.

import type { Actions } from "../actions.js";
import { bodyRef, edgeRef, faceRef, featureRef, refKey, refLabel, resolveRef } from "../selection.js";
import type { AppStore } from "../state.js";
import type { Body, EntityRef, Feature, LengthUnit, Model, SourceSpan, TopoEdge, TopoFace } from "../types.js";
import { fmtArea, fmtLen, fmtParamLength, fmtPoint, fmtVec } from "../util.js";
import { h, replaceChildren } from "./dom.js";

export function createInspectorPanel(store: AppStore, actions: Actions): HTMLElement {
  const body = h("div", { class: "inspector-body" });
  const note = h("textarea", {
    class: "note-input",
    rows: 2,
    placeholder: "Note for the assistant (e.g. “make this thicker”)",
    aria: { label: "Selection note" },
  });
  let noteTimer: number | null = null;
  note.addEventListener("input", () => {
    if (noteTimer !== null) window.clearTimeout(noteTimer);
    noteTimer = window.setTimeout(() => actions.setNote(note.value), 400);
  });
  const noteWrap = h("div", { class: "note-wrap" }, h("label", { class: "field-label" }, "Note", note));
  const root = h(
    "section",
    { class: "panel-section inspector", aria: { label: "Inspector" } },
    h("h2", { class: "section-title" }, "Inspector"),
    body,
    noteWrap,
  );

  const link = (ref: EntityRef, text: string): HTMLElement =>
    h(
      "button",
      {
        type: "button",
        class: "link-btn",
        on: {
          click: (ev) => actions.select([ref], { additive: ev.shiftKey }),
          pointerenter: () => actions.hover(ref),
          pointerleave: () => actions.hover(null),
        },
      },
      text,
    );

  const kv = (k: string, v: string | HTMLElement | HTMLElement[]): HTMLElement =>
    h("div", { class: "kv" }, h("dt", null, k), h("dd", null, ...(Array.isArray(v) ? v : [v])));

  const spanRow = (span: SourceSpan | null): HTMLElement | null => {
    if (!span) return null;
    return kv(
      "Source",
      h(
        "button",
        { type: "button", class: "link-btn", title: "Show in script", on: { click: () => actions.revealSpan(span) } },
        `${span.path}:${span.start_line}:${span.start_col}`,
      ),
    );
  };

  const faceView = (b: Body, f: TopoFace, unit: LengthUnit): HTMLElement =>
    h(
      "dl",
      { class: "kvs" },
      kv("Face", f.name),
      kv("Kind", f.surface_kind || "–"),
      kv("Body", link(bodyRef(b), b.name || b.id)),
      kv("Normal", fmtVec(f.normal)),
      kv("Centroid", fmtPoint(f.centroid, unit)),
      kv("Area", fmtArea(f.area, unit)),
      kv(
        "Edges",
        f.edges.map((en) => {
          const e = b.topology.edges.find((x) => x.name === en);
          return e ? link(edgeRef(b, e), en.replace(/^edge:/, "")) : h("span", null, en);
        }),
      ),
    );

  const edgeView = (b: Body, e: TopoEdge, unit: LengthUnit): HTMLElement =>
    h(
      "dl",
      { class: "kvs" },
      kv("Edge", e.name),
      kv("Kind", e.curve_kind || "–"),
      kv("Body", link(bodyRef(b), b.name || b.id)),
      kv("Length", fmtLen(e.length, unit)),
      kv("Start", fmtPoint(e.start, unit)),
      kv("End", fmtPoint(e.end, unit)),
      kv(
        "Faces",
        [...new Set(e.faces)].map((fn) => {
          const f = b.topology.faces.find((x) => x.name === fn);
          return f ? link(faceRef(b, f), fn.replace(/^face:/, "")) : h("span", null, fn);
        }),
      ),
    );

  const bodyView = (model: Model, b: Body, unit: LengthUnit): HTMLElement => {
    const size = { x: b.bbox.max.x - b.bbox.min.x, y: b.bbox.max.y - b.bbox.min.y, z: b.bbox.max.z - b.bbox.min.z };
    const f = model.features.find((x) => x.id === b.feature_id);
    return h(
      "dl",
      { class: "kvs" },
      kv("Body", b.name || b.id),
      kv("Id", b.id),
      kv("Feature", f ? link(featureRef(f), f.name || f.id) : b.feature_id),
      kv("Size", `${fmtLen(size.x, unit)} × ${fmtLen(size.y, unit)} × ${fmtLen(size.z, unit)}`),
      kv("Min", fmtPoint(b.bbox.min, unit)),
      kv("Max", fmtPoint(b.bbox.max, unit)),
      kv("Topology", `${b.topology.faces.length} faces · ${b.topology.edges.length} edges · ${b.topology.vertices.length} vertices`),
      kv("Hidden", store.get().hiddenBodies.has(b.id) ? "yes" : "no"),
    );
  };

  const featureView = (model: Model, f: Feature): HTMLElement => {
    const args = Object.entries(f.args);
    return h(
      "dl",
      { class: "kvs" },
      kv("Feature", f.name || f.id),
      kv("Id", f.id),
      kv("Kind", f.kind),
      f.suppressed ? kv("Suppressed", "yes") : null,
      args.length
        ? kv(
            "Args",
            h("table", { class: "args" }, ...args.map(([k, v]) => h("tr", null, h("th", null, k), h("td", null, v)))),
          )
        : null,
      f.inputs.length
        ? kv(
            "Inputs",
            f.inputs.map((id) => {
              const inf = model.features.find((x) => x.id === id);
              return inf ? link(featureRef(inf), inf.name || id) : h("span", null, id);
            }),
          )
        : null,
      f.outputs.length
        ? kv(
            "Outputs",
            f.outputs.map((id) => {
              const b = model.bodies.find((x) => x.id === id);
              return b ? link(bodyRef(b), b.name || id) : h("span", null, id);
            }),
          )
        : null,
      spanRow(f.span),
      f.diagnostics.length
        ? kv(
            "Issues",
            h(
              "ul",
              { class: "issue-list" },
              ...f.diagnostics.map((d) => h("li", { class: `sev-${d.severity}` }, d.message)),
            ),
          )
        : null,
    );
  };

  const paramView = (model: Model, name: string): HTMLElement => {
    const p = model.params.find((x) => x.name === name);
    if (!p) return h("div", { class: "empty" }, `Unknown parameter ${name}`);
    // A length parameter's value is in the unit it declared (`10 ft`); under
    // the document's `units(...)` it reads in that convention too.
    const value = typeof p.value === "number" ? p.value : 0;
    const readout = fmtParamLength(value, p.unit, model.display_units);
    return h(
      "dl",
      { class: "kvs" },
      kv("Param", p.name),
      kv("Value", readout),
      kv("Default", String(p.default_value)),
      p.min !== null || p.max !== null ? kv("Range", `${p.min ?? "–"} … ${p.max ?? "–"}`) : null,
      spanRow(p.span),
    );
  };

  function render(): void {
    const s = store.get();
    const model = s.model;
    const unit: LengthUnit = model?.display_units?.unit ?? s.session?.document.display_unit ?? "mm";
    const sel = s.selected;
    if (sel.length === 0) {
      replaceChildren(
        body,
        h(
          "div",
          { class: "empty" },
          "Nothing selected.",
          h("br"),
          h("span", { class: "hint" }, "Click a face or edge in the viewport, or a feature in the tree. Shift-click adds."),
        ),
      );
      noteWrap.hidden = true;
      return;
    }
    noteWrap.hidden = false;
    const primary = sel[sel.length - 1] as EntityRef;
    const chips = sel.map((r) =>
      h(
        "span",
        { class: `chip chip-${r.kind} ${refKey(r) === refKey(primary) ? "primary" : ""}` },
        h("span", { class: "chip-kind" }, r.kind),
        h("span", null, refLabel(model, r)),
        h(
          "button",
          {
            type: "button",
            class: "chip-x",
            aria: { label: `Deselect ${refLabel(model, r)}` },
            on: {
              click: () => actions.select([r], { additive: true }),
            },
          },
          "×",
        ),
      ),
    );
    const res = resolveRef(model, primary);
    let detail: HTMLElement;
    if (!model) detail = h("div", { class: "empty" }, "Model not evaluated yet.");
    else if (primary.kind === "face" && res.body && res.face) detail = faceView(res.body, res.face, unit);
    else if (primary.kind === "edge" && res.body && res.edge) detail = edgeView(res.body, res.edge, unit);
    else if (primary.kind === "body" && res.body) detail = bodyView(model, res.body, unit);
    else if (primary.kind === "param") detail = paramView(model, primary.topo_name ?? "");
    else if (res.feature) detail = featureView(model, res.feature);
    else detail = h("div", { class: "empty" }, `${primary.kind} ${refLabel(model, primary)} is not in the current model.`);
    replaceChildren(
      body,
      h("div", { class: "chips", role: "list", aria: { label: "Selected entities" } }, ...chips),
      detail,
    );
  }

  store.subscribe((s, prev) => {
    if (s.selected !== prev.selected || s.model !== prev.model || s.hiddenBodies !== prev.hiddenBodies) render();
    if (s.note !== prev.note && document.activeElement !== note) note.value = s.note;
    if (s.apiMode !== prev.apiMode) {
      note.placeholder = s.apiMode === "session" ? "Note for the assistant (e.g. “make this thicker”)" : "Note (local only)";
    }
  });
  render();
  return root;
}
