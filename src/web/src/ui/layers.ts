// The Layers panel: one row per display layer the script declared
// with `layer(...)` -- a visibility switch, an opacity slider, the doc as a
// tooltip, the name a solo button -- and "Reset to script". Every control
// touches the viewer's per-layer state and nothing else: no evaluation, no
// message to the engine, which is the point of a layer over a bool param().
// Under the feature tree in the Model panel on a wide screen, and a sheet tab
// of its own after Params on a phone (layout.ts); not shown for a
// document without layers.

import type { Actions } from "../actions.js";
import type { AppStore } from "../state.js";
import type { Layer, LayerOverride, Model } from "../types.js";
import type { LayerDisplay } from "../viewer.js";
import { h, replaceChildren } from "./dom.js";
import { cssColor } from "./plates.js";

/** The implicit layer of untagged bodies: `Body.layer` is "" there. */
export const OTHER_LAYER = "";

/** What a row calls a layer: its name, or "Other" for the implicit one. */
export function layerLabel(name: string): string {
  return name === OTHER_LAYER ? "Other" : name;
}

/**
 * The layers a model lists: every declared layer in declaration order and,
 * when at least one is declared and some drawn body is on none, "Other" --
 * so a document that never mentions layers has no panel at all.
 */
export function listLayers(model: Model | null): Layer[] {
  if (!model || model.layers.length === 0) return [];
  const out = [...model.layers].sort((a, b) => a.order - b.order);
  if (model.bodies.some((b) => b.visible && b.layer === OTHER_LAYER)) {
    out.push({ name: OTHER_LAYER, doc: "Bodies on no layer", visible: true, opacity: 1, color: null, order: out.length });
  }
  return out;
}

/** The script's defaults with the user's overrides written over them, by name. */
export function effectiveLayers(model: Model | null, overrides: ReadonlyMap<string, LayerOverride>): ReadonlyMap<string, LayerDisplay> {
  const out = new Map<string, LayerDisplay>();
  for (const l of listLayers(model)) {
    const o = overrides.get(l.name);
    out.set(l.name, { visible: o?.visible ?? l.visible, opacity: o?.opacity ?? l.opacity });
  }
  return out;
}

/** True when `name` is the one layer shown and there are others to hide. */
export function isSoloed(name: string, effective: ReadonlyMap<string, LayerDisplay>): boolean {
  if (effective.size < 2 || !effective.get(name)?.visible) return false;
  for (const [n, d] of effective) if (n !== name && d.visible) return false;
  return true;
}

interface Row {
  el: HTMLElement;
  update(layer: Layer, display: LayerDisplay, override: LayerOverride | undefined, soloed: boolean): void;
}

export function createLayersPanel(store: AppStore, actions: Actions): HTMLElement {
  const list = h("div", { class: "layers", role: "list" });
  const reset = h(
    "button",
    {
      type: "button",
      class: "btn btn-quiet",
      title: "Show every layer as the script declared it",
      on: { click: () => actions.resetLayers() },
    },
    "Reset to script",
  );
  const root = h(
    "section",
    { class: "panel-section layers-section", aria: { label: "Layers" } },
    h("div", { class: "section-head" }, h("h2", { class: "section-title" }, "Layers"), reset),
    list,
  );
  root.hidden = true;
  let rows = new Map<string, Row>();
  let signature = "";

  const rowFor = (layer: Layer): Row => {
    const label = layerLabel(layer.name);
    const id = `layer-${layer.name || "other"}`;
    const name = h(
      "button",
      {
        type: "button",
        class: "layer-name",
        id,
        title: `${layer.doc ? `${layer.doc} — ` : ""}tap to show this layer alone; again to show every layer`,
        aria: { pressed: "false" },
        on: { click: () => actions.soloLayer(layer.name) },
      },
      label,
    );
    const pct = h("span", { class: "layer-pct", aria: { hidden: "true" } }, "100 %");
    // The parameters' switch, in a label that is the tap target: the
    // switch draws 44 x 26, the label makes it 44 x 44 on a coarse pointer.
    const on = h("input", {
      type: "checkbox",
      role: "switch",
      class: "param-switch layer-switch",
      aria: { label: `Show ${label}` },
    });
    on.addEventListener("change", () => actions.setLayerVisible(layer.name, on.checked));
    const range = h("input", {
      type: "range",
      class: "layer-range",
      min: "0",
      max: "100",
      step: "1",
      aria: { label: `${label} opacity` },
    });
    // Display only, so every step of the slider is applied as it moves.
    range.addEventListener("input", () => actions.setLayerOpacity(layer.name, Number(range.value) / 100));
    const swatch = layer.color ? h("span", { class: "layer-swatch", aria: { hidden: "true" }, style: { background: cssColor(layer.color) } }) : null;
    const el = h(
      "div",
      { class: "layer-row", role: "listitem", dataset: { layer: layer.name }, title: layer.doc },
      h("div", { class: "layer-head" }, swatch, name, pct, h("label", { class: "layer-switch-hit" }, on)),
      range,
    );
    return {
      el,
      update(_layer, display, override, soloed) {
        on.checked = display.visible;
        on.setAttribute("aria-checked", String(display.visible));
        const percent = Math.round(display.opacity * 100);
        if (document.activeElement !== range) range.value = String(percent);
        range.setAttribute("aria-valuetext", `${percent} %`);
        pct.textContent = `${percent} %`;
        name.setAttribute("aria-pressed", String(soloed));
        el.classList.toggle("overridden", override !== undefined);
        el.classList.toggle("is-off", !display.visible);
      },
    };
  };

  function build(layers: Layer[]): void {
    rows = new Map();
    const els: HTMLElement[] = [];
    for (const l of layers) {
      const row = rowFor(l);
      rows.set(l.name, row);
      els.push(row.el);
    }
    replaceChildren(list, ...els);
  }

  function sync(): void {
    const s = store.get();
    const layers = listLayers(s.model);
    root.hidden = layers.length === 0;
    if (root.hidden) return;
    const sig = layers.map((l) => `${l.name}:${l.doc}:${l.color?.join(",") ?? ""}`).join("|");
    if (sig !== signature) {
      signature = sig;
      build(layers);
    }
    const effective = effectiveLayers(s.model, s.layerOverrides);
    let any = false;
    for (const l of layers) {
      const ov = s.layerOverrides.get(l.name);
      const d = effective.get(l.name) ?? { visible: l.visible, opacity: l.opacity };
      rows.get(l.name)?.update(l, d, ov, isSoloed(l.name, effective));
      any = any || ov !== undefined;
    }
    reset.hidden = !any;
  }

  store.subscribe((s, prev) => {
    if (s.model !== prev.model || s.layerOverrides !== prev.layerOverrides) sync();
  });
  sync();
  return root;
}
