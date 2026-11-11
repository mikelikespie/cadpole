// The plates view's furniture: a strip of plate tabs to frame one
// plate or all of them, and a legend of the filament slots with their
// colours. Lives in the viewport, over the canvas, and is only shown while
// the plates view is on. The plates themselves are drawn by the viewer.

import type { Actions } from "../actions.js";
import type { AppStore } from "../state.js";
import type { Plate } from "../types.js";
import { h, replaceChildren } from "./dom.js";

/** A plate's tab text: its number and, when it has one, its name. */
export function plateLabel(p: Plate): string {
  return p.name ? `${p.index} · ${p.name}` : `Plate ${p.index}`;
}

/** Linear RGB in 0..1 to a CSS colour, encoded to sRGB the way the viewer draws it. */
export function cssColor(c: [number, number, number]): string {
  const enc = (v: number): number => {
    const x = Math.max(0, Math.min(1, v));
    return Math.round(255 * (x <= 0.0031308 ? 12.92 * x : 1.055 * Math.pow(x, 1 / 2.4) - 0.055));
  };
  return `rgb(${enc(c[0])}, ${enc(c[1])}, ${enc(c[2])})`;
}

/** The slots a set of plates prints in, each once, in slot order. */
export function legendSlots(plates: readonly Plate[]): { slot: number; color: [number, number, number]; filament: string }[] {
  const seen = new Map<number, { slot: number; color: [number, number, number]; filament: string }>();
  for (const p of plates) {
    for (const b of p.bodies) if (!seen.has(b.slot)) seen.set(b.slot, { slot: b.slot, color: b.color, filament: b.filament });
  }
  return [...seen.values()].sort((a, b) => a.slot - b.slot);
}

export function createPlatesPanel(store: AppStore, actions: Actions): HTMLElement {
  const tabs = h("div", { class: "plate-tabs", role: "tablist", aria: { label: "Plates" } });
  const legend = h("ul", { class: "plate-legend", role: "list", aria: { label: "Filaments" } });
  const root = h("div", { class: "plates-overlay" }, tabs, legend);
  root.hidden = true;

  const rebuild = (): void => {
    const s = store.get();
    const plates = s.model?.plates ?? [];
    root.hidden = !s.platesView || plates.length === 0;
    if (root.hidden) return;
    const tab = (label: string, title: string, active: boolean, onClick: () => void): HTMLButtonElement =>
      h(
        "button",
        {
          type: "button",
          role: "tab",
          class: `plate-tab${active ? " active" : ""}`,
          title,
          aria: { selected: String(active) },
          on: { click: onClick },
        },
        label,
      );
    replaceChildren(
      tabs,
      tab("All plates", "Frame every plate", s.activePlate === null, () => actions.setActivePlate(null)),
      ...plates.map((p, i) =>
        tab(
          plateLabel(p),
          `Frame plate ${p.index} of ${p.path}: ${p.bodies.length} ${p.bodies.length === 1 ? "body" : "bodies"}`,
          s.activePlate === i,
          () => actions.setActivePlate(i),
        ),
      ),
    );
    replaceChildren(
      legend,
      ...legendSlots(plates).map((f) =>
        h(
          "li",
          { class: "legend-item", role: "listitem", title: `Filament slot ${f.slot}` },
          h("span", { class: "legend-swatch", aria: { hidden: "true" }, style: { background: cssColor(f.color) } }),
          h("span", null, f.filament ? `${f.slot} · ${f.filament}` : `Slot ${f.slot}`),
        ),
      ),
    );
  };

  store.subscribe((s, prev) => {
    if (s.model !== prev.model || s.platesView !== prev.platesView || s.activePlate !== prev.activePlate) rebuild();
  });
  rebuild();
  return root;
}
