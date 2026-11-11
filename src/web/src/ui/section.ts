// The Section panel: a horizontal cut at a height and up to one
// vertical cut along X or Y, each with a slider, a field in the document's
// display unit and a flip. Floats over the viewport; works the same on a
// published page, because a section is the viewer's and never the engine's.
//
// It writes `AppState.section`, in millimetres, and shows the spec in
// `cadpole render --section`'s spelling, so a cut seen here can be rendered
// (`cadpole render ... --section z=2400`) or published (`cadpole publish
// --section`) as it is.

import type { Actions } from "../actions.js";
import { formatSection, isSectioned, type CutPlane, type SectionState } from "../section.js";
import type { AppState, AppStore } from "../state.js";
import type { LengthUnit } from "../types.js";
import { fmtLen, parseLen } from "../util.js";
import { h } from "./dom.js";

/** The steps a slider has across the model. */
const SLIDER_STEPS = 1000;

interface Row {
  el: HTMLElement;
  sync(s: AppState): void;
}

export function createSectionPanel(store: AppStore, actions: Actions): HTMLElement {
  const unit = (): LengthUnit => store.get().model?.display_units?.unit ?? store.get().session?.document.display_unit ?? "mm";
  const bounds = (axis: 0 | 1 | 2): [number, number] => {
    const b = actions.sceneBounds();
    if (!b) return [0, 1];
    const lo = b.min[axis];
    const hi = b.max[axis];
    return hi > lo ? [lo, hi] : [lo - 1, lo + 1];
  };
  // Slider moves coalesce to one section per frame: a cut re-slices the
  // drawn bodies, and a drag sends many more events than frames.
  let pending: SectionState | null = null;
  const commit = (next: SectionState): void => {
    if (pending === null) {
      requestAnimationFrame(() => {
        const s = pending;
        pending = null;
        if (s) actions.setSection(s);
      });
    }
    pending = next;
  };

  /**
   * One plane's row. `kind` is which slot of the section it edits; the
   * vertical row also picks its axis.
   */
  const row = (kind: "horizontal" | "vertical"): Row => {
    const label = kind === "horizontal" ? "Horizontal cut" : "Vertical cut";
    const toggle = h("input", { type: "checkbox", class: `section-on section-${kind}-on`, aria: { label: `${label} on` } });
    const axisSel =
      kind === "vertical"
        ? h(
            "select",
            { class: "select section-axis", aria: { label: "Vertical cut along" }, title: "The vertical plane: across X (a plane x = const) or Y" },
            h("option", { value: "x" }, "X"),
            h("option", { value: "y" }, "Y"),
          )
        : null;
    const range = h("input", { type: "range", class: `section-range section-${kind}-range`, min: "0", max: String(SLIDER_STEPS), step: "1", aria: { label: `${label} position` } });
    const field = h("input", {
      type: "text",
      class: `section-value section-${kind}-value`,
      inputMode: "decimal",
      spellcheck: false,
      aria: { label: kind === "horizontal" ? "Cut height" : "Cut position" },
      title: "A length in the document's unit, or with its own: 4'-6\", 1200 mm, 1.2 m",
    });
    const flip = h(
      "button",
      {
        type: "button",
        class: `btn btn-quiet section-flip section-${kind}-flip`,
        aria: { pressed: "false" },
        title: kind === "horizontal" ? "Keep what is above the cut instead (look up at a ceiling)" : "Keep the other side of the cut",
      },
      "Flip",
    );
    const el = h(
      "div",
      { class: `section-row section-${kind}`, role: "group", aria: { label } },
      h("label", { class: "section-label" }, toggle, h("span", null, kind === "horizontal" ? "Horizontal" : "Vertical")),
      axisSel,
      range,
      field,
      flip,
    );

    const current = (): CutPlane | null => store.get().section[kind];
    const axisOf = (): 0 | 1 | 2 => (kind === "horizontal" ? 2 : axisSel?.value === "y" ? 1 : 0);
    const put = (plane: CutPlane | null): void => {
      const s = store.get().section;
      commit({ ...(pending ?? s), [kind]: plane });
    };
    /** A plane at this row's default place: the middle of the drawn model along its axis. */
    const fresh = (): CutPlane => {
      const axis = axisOf();
      const [lo, hi] = bounds(axis);
      return { axis, at: lo + (hi - lo) * 0.5, removePositive: true };
    };

    toggle.addEventListener("change", () => put(toggle.checked ? (current() ?? fresh()) : null));
    axisSel?.addEventListener("change", () => {
      const p = current();
      // Choosing the axis of a cut that is off only says where it will be.
      if (!p) {
        sync(store.get());
        return;
      }
      const axis = axisOf();
      const [lo, hi] = bounds(axis);
      put({ axis, at: lo + (hi - lo) * 0.5, removePositive: p?.removePositive ?? true });
    });
    range.addEventListener("input", () => {
      const p = current() ?? fresh();
      const [lo, hi] = bounds(p.axis);
      put({ ...p, at: lo + ((hi - lo) * Number(range.value)) / SLIDER_STEPS });
    });
    const fromField = (): void => {
      const mm = parseLen(field.value, unit());
      if (mm === null) {
        field.setAttribute("aria-invalid", "true");
        return;
      }
      field.removeAttribute("aria-invalid");
      put({ ...(current() ?? fresh()), at: mm });
    };
    field.addEventListener("change", fromField);
    field.addEventListener("keydown", (ev) => {
      if (ev.key === "Enter") {
        ev.preventDefault();
        fromField();
      }
    });
    flip.addEventListener("click", () => {
      const p = current() ?? fresh();
      put({ ...p, removePositive: !p.removePositive });
    });

    const sync = (s: AppState): void => {
      const p = s.section[kind];
      toggle.checked = p !== null;
      el.classList.toggle("off", p === null);
      if (axisSel && p) axisSel.value = p.axis === 1 ? "y" : "x";
      const axis = p?.axis ?? axisOf();
      const [lo, hi] = bounds(axis);
      const at = p?.at ?? lo + (hi - lo) * 0.5;
      range.value = String(Math.round(((at - lo) / (hi - lo)) * SLIDER_STEPS));
      if (document.activeElement !== field) field.value = fmtLen(at, unit());
      flip.setAttribute("aria-pressed", String(p !== null && !p.removePositive));
    };
    return { el, sync };
  };

  const horizontal = row("horizontal");
  const vertical = row("vertical");
  const spec = h("code", { class: "section-spec", title: "The same cut for cadpole render --section or cadpole publish --section (millimetres)" });
  const clear = h(
    "button",
    { type: "button", class: "btn btn-quiet section-clear", title: "Remove every cut", on: { click: () => actions.setSection({ horizontal: null, vertical: null }) } },
    "Clear",
  );
  const close = h(
    "button",
    { type: "button", class: "btn btn-quiet section-close", aria: { label: "Close the Section panel" }, title: "Close (C). The cut stays.", on: { click: () => actions.toggleSectionPanel(false) } },
    "×",
  );
  const root = h(
    "div",
    { class: "section-panel", role: "group", aria: { label: "Section" } },
    h("div", { class: "section-head" }, h("span", { class: "section-title" }, "Section"), spec, clear, close),
    horizontal.el,
    vertical.el,
  );
  root.hidden = true;

  const sync = (s: AppState): void => {
    root.hidden = !s.sectionPanel;
    horizontal.sync(s);
    vertical.sync(s);
    spec.textContent = isSectioned(s.section) ? formatSection(s.section) : "no cut";
    clear.disabled = !isSectioned(s.section);
  };
  store.subscribe((s, prev) => {
    if (s.section !== prev.section || s.sectionPanel !== prev.sectionPanel) sync(s);
    // A new model's bounds are only final once every subscriber has run
    // (the layers are applied in main.ts's), so the sliders are placed then.
    if (s.model !== prev.model || s.layerOverrides !== prev.layerOverrides) queueMicrotask(() => sync(store.get()));
  });
  sync(store.get());
  return root;
}
