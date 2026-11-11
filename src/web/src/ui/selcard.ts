// The selection card: what a tap on the model answers with on a phone.
//
// On a desk the Inspector is always in view beside the model. On a phone it is
// a sheet tab, and a tap that needed a second tap on a tab to say what was
// tapped would be half an answer. So the narrow layout shows a small card over
// the bottom of the viewport while something is selected and no sheet is open:
// what it is, its name, the one or two measurements that matter (in the
// document's own units), and three buttons -- details (the Inspector sheet),
// focus, and dismiss. When the tap was ambiguous (a face, its body, the
// feature that made it), the alternatives are chips on the card, where the
// desktop puts a popover at the cursor.

import type { Actions } from "../actions.js";
import { refKey, refLabel, resolveRef } from "../selection.js";
import type { AppState, AppStore } from "../state.js";
import type { EntityRef, LengthUnit } from "../types.js";
import { fmtArea, fmtLen, fmtParamLength } from "../util.js";
import type { Candidate } from "../viewer.js";
import { h, replaceChildren } from "./dom.js";
import { icon } from "./icons.js";

export interface SelectionCard {
  el: HTMLElement;
  /** The entities under the last tap, when there was more than one; empty to forget them. */
  setCandidates(cands: Candidate[]): void;
}

/** The measurements worth a line on the card, most telling first. */
export function selectionFacts(s: AppState, ref: EntityRef): string[] {
  const model = s.model;
  if (!model) return [];
  const unit: LengthUnit = model.display_units?.unit ?? s.session?.document.display_unit ?? "mm";
  const res = resolveRef(model, ref);
  if (ref.kind === "face" && res.face) {
    return [res.face.surface_kind, `area ${fmtArea(res.face.area, unit)}`].filter(Boolean);
  }
  if (ref.kind === "edge" && res.edge) {
    return [res.edge.curve_kind, `length ${fmtLen(res.edge.length, unit)}`].filter(Boolean);
  }
  if (ref.kind === "body" && res.body) {
    const b = res.body.bbox;
    return [`${fmtLen(b.max.x - b.min.x, unit)} × ${fmtLen(b.max.y - b.min.y, unit)} × ${fmtLen(b.max.z - b.min.z, unit)}`];
  }
  if (ref.kind === "param") {
    const p = model.params.find((x) => x.name === ref.topo_name);
    if (!p) return [];
    return [typeof p.value === "number" ? fmtParamLength(p.value, p.unit, model.display_units) : String(p.value)];
  }
  if (res.feature) {
    const n = res.feature.outputs.length;
    return [res.feature.kind, n === 1 ? "1 body" : n > 1 ? `${n} bodies` : ""].filter(Boolean);
  }
  return [];
}

export function createSelectionCard(store: AppStore, actions: Actions): SelectionCard {
  const kind = h("span", { class: "selcard-kind" });
  const name = h("span", { class: "selcard-name" });
  const facts = h("div", { class: "selcard-facts" });
  const alts = h("div", { class: "selcard-alts", role: "group", aria: { label: "Also under the tap" } });
  const more = h("span", { class: "selcard-more" });
  const details = h(
    "button",
    { type: "button", class: "selcard-main", aria: { label: "Selection details" }, on: { click: () => actions.openSheet("inspector") } },
    h("span", { class: "selcard-line" }, kind, name, more),
    facts,
  );
  const focus = h(
    "button",
    { type: "button", class: "icon-btn selcard-btn", title: "Focus on selection", aria: { label: "Focus on selection" }, on: { click: () => actions.focusSelection() } },
    icon("focus"),
  );
  const close = h(
    "button",
    { type: "button", class: "icon-btn selcard-btn", title: "Clear selection", aria: { label: "Clear selection" }, on: { click: () => actions.select([]) } },
    icon("close", 20),
  );
  const el = h("div", { class: "selcard", role: "region", aria: { label: "Selection" } }, h("div", { class: "selcard-row" }, details, focus, close), alts);
  el.hidden = true;
  let candidates: Candidate[] = [];

  function render(): void {
    const s = store.get();
    const sel = s.selected;
    const primary = sel[sel.length - 1];
    if (!s.narrow || !primary || s.sheet !== null) {
      el.hidden = true;
      return;
    }
    kind.textContent = primary.kind;
    kind.className = `selcard-kind chip-${primary.kind}`;
    name.textContent = refLabel(s.model, primary);
    more.textContent = sel.length > 1 ? `+${sel.length - 1}` : "";
    more.hidden = sel.length <= 1;
    const lines = selectionFacts(s, primary);
    facts.textContent = lines.join(" · ");
    facts.hidden = lines.length === 0;
    // The alternatives are offered while the selection is one of them: once
    // it is something else (a tree row, a link), they are about another tap.
    const key = refKey(primary);
    if (!candidates.some((c) => refKey(c.ref) === key)) candidates = [];
    replaceChildren(
      alts,
      ...candidates.map((c) => {
        const on = refKey(c.ref) === key;
        return h(
          "button",
          { type: "button", class: "chip-btn", aria: { pressed: String(on) }, on: { click: () => actions.select([c.ref]) } },
          h("span", { class: "chip-kind" }, c.kind),
          " ",
          c.label,
        );
      }),
    );
    alts.hidden = candidates.length < 2;
    el.hidden = false;
  }

  store.subscribe((s, prev) => {
    if (s.selected !== prev.selected || s.model !== prev.model || s.narrow !== prev.narrow || s.sheet !== prev.sheet) render();
  });
  render();
  return {
    el,
    setCandidates(cands) {
      candidates = cands;
      render();
    },
  };
}
