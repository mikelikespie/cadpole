// Examples gallery. Loading an example replaces the script (locally in static
// mode, via SetScript in session mode).

import type { Actions } from "../actions.js";
import { EXAMPLES } from "../examples.js";
import type { AppStore } from "../state.js";
import { h, replaceChildren } from "./dom.js";

export function createExamplesPanel(store: AppStore, actions: Actions): HTMLElement {
  const list = h("div", { class: "examples", role: "list" });
  const root = h(
    "section",
    { class: "panel-section examples-section", aria: { label: "Examples" } },
    h("h2", { class: "section-title" }, "Examples"),
    h("p", { class: "muted small" }, "Loading an example replaces the current script."),
    list,
  );

  function render(): void {
    const current = store.get().session?.document.scripts[0]?.source ?? "";
    replaceChildren(
      list,
      ...EXAMPLES.map((ex) => {
        const active = ex.source === current;
        return h(
          "div",
          { class: `example-card ${active ? "active" : ""}`, role: "listitem" },
          h("div", { class: "example-head" }, h("strong", null, ex.name), active ? h("span", { class: "pill" }, "current") : null),
          h("p", { class: "muted small" }, ex.description),
          h(
            "button",
            {
              type: "button",
              class: "btn",
              disabled: active,
              aria: { label: `Load example ${ex.name}` },
              on: { click: () => void actions.loadExample(ex.id) },
            },
            active ? "Loaded" : "Load",
          ),
        );
      }),
    );
  }

  store.subscribe((s, prev) => {
    if (s.session?.document.scripts !== prev.session?.document.scripts) render();
  });
  render();
  return root;
}
