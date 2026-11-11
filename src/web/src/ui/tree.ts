// Feature tree: features -> bodies, with selection mirroring, visibility
// toggles, and full keyboard navigation (roving tabindex, arrows, Home/End,
// Left/Right to collapse/expand, Enter/Space to select, Shift for additive).

import type { Actions } from "../actions.js";
import { bodyRef, featureRef, refKey } from "../selection.js";
import type { AppStore } from "../state.js";
import type { Body, EntityRef, Model } from "../types.js";
import { rovingKeydown, setRovingTabindex } from "./a11y.js";
import { h, replaceChildren } from "./dom.js";

const KIND_GLYPH: Record<string, string> = {
  sketch: "SK",
  extrude: "EX",
  revolve: "RV",
  sweep: "SW",
  loft: "LF",
  fillet: "FL",
  chamfer: "CH",
  union: "UN",
  subtract: "SB",
  intersect: "IN",
  shell: "SH",
  pattern: "PT",
  mirror: "MR",
  transform: "TR",
};

export function createTreePanel(store: AppStore, actions: Actions): HTMLElement {
  const tree = h("div", { class: "tree", role: "tree", aria: { label: "Feature tree" } });
  const root = h(
    "section",
    { class: "panel-section tree-section", aria: { label: "Features" } },
    h("h2", { class: "section-title" }, "Features"),
    tree,
  );
  const rows = new Map<string, HTMLElement>();
  const expanded = new Set<string>();
  let items: HTMLElement[] = [];

  const rowFor = (ref: EntityRef, label: string, glyph: string, detail: string, depth: number, ...extra: (HTMLElement | null | undefined)[]): HTMLElement => {
    const row = h(
      "div",
      {
        class: `tree-row depth-${depth} kind-${ref.kind}`,
        role: "treeitem",
        tabIndex: -1,
        aria: { selected: "false", level: String(depth + 1) },
        dataset: { key: refKey(ref) },
        on: {
          click: (ev) => actions.select([ref], { additive: ev.shiftKey || ev.ctrlKey || ev.metaKey }),
          pointerenter: () => actions.hover(ref),
          pointerleave: () => actions.hover(null),
          focus: () => actions.hover(ref),
          blur: () => actions.hover(null),
          keydown: (ev) => onKey(ev, row, ref),
        },
      },
      h("span", { class: "tree-glyph", aria: { hidden: "true" } }, glyph),
      h("span", { class: "tree-label" }, label),
      detail ? h("span", { class: "tree-detail" }, detail) : null,
      ...extra,
    );
    rows.set(refKey(ref), row);
    return row;
  };

  const onKey = (ev: KeyboardEvent, row: HTMLElement, ref: EntityRef): void => {
    if (ev.key === "ArrowRight" && ref.kind === "feature") {
      if (!expanded.has(ref.feature_id)) {
        expanded.add(ref.feature_id);
        render(store.get().model);
        rows.get(refKey(ref))?.focus();
      }
      ev.preventDefault();
      return;
    }
    if (ev.key === "ArrowLeft") {
      const fid = ref.feature_id;
      if (ref.kind === "feature" && expanded.has(fid)) {
        expanded.delete(fid);
        render(store.get().model);
        rows.get(refKey(ref))?.focus();
      } else if (ref.kind === "body") {
        rows.get(refKey({ feature_id: fid, kind: "feature" }))?.focus();
      }
      ev.preventDefault();
      return;
    }
    if (ev.key === "Enter" || ev.key === " ") {
      ev.preventDefault();
      actions.select([ref], { additive: ev.shiftKey });
      return;
    }
    if ((ev.key === "Delete" || ev.key === "Backspace") && ref.kind === "body" && ref.body_id) {
      // The eye button is deliberately not a tab stop (one roving stop per
      // tree), so the row answers for it. Both keys are free of the viewport
      // bindings. The re-render replaces this row; focus goes back to its
      // successor, so a screen reader stays where it was.
      ev.preventDefault();
      actions.toggleHidden(ref.body_id);
      rows.get(refKey(ref))?.focus();
      return;
    }
    rovingKeydown(ev, items, row);
  };

  const eyeButton = (body: Body): HTMLElement => {
    const hidden = store.get().hiddenBodies.has(body.id);
    return h(
      "button",
      {
        type: "button",
        class: `icon-btn eye ${hidden ? "is-hidden" : ""}`,
        title: hidden ? "Show body" : "Hide body",
        tabIndex: -1,
        aria: { label: `${hidden ? "Show" : "Hide"} ${body.name || body.id}`, pressed: String(!hidden) },
        on: {
          click: (ev) => {
            ev.stopPropagation();
            actions.toggleHidden(body.id);
          },
        },
      },
      h("span", { aria: { hidden: "true" } }, hidden ? "◌" : "◉"),
    );
  };

  /** The layer a body is on, as a small badge, or nothing. */
  const layerBadge = (body: Body): HTMLElement | null =>
    body.layer ? h("span", { class: "tree-layer", title: `On layer ${body.layer}` }, body.layer) : null;

  function render(model: Model | null): void {
    rows.clear();
    if (!model) {
      replaceChildren(tree, h("div", { class: "empty" }, "No model yet."));
      items = [];
      return;
    }
    if (model.features.length === 0) {
      replaceChildren(tree, h("div", { class: "empty" }, "The script produced no features."));
      items = [];
      return;
    }
    const bodiesByFeature = new Map<string, Body[]>();
    for (const b of model.bodies) {
      const list = bodiesByFeature.get(b.feature_id) ?? [];
      list.push(b);
      bodiesByFeature.set(b.feature_id, list);
    }
    const children: HTMLElement[] = [];
    for (const f of model.features) {
      const bodies = bodiesByFeature.get(f.id) ?? model.bodies.filter((b) => f.outputs.includes(b.id));
      const hasBodies = bodies.length > 0;
      if (hasBodies && !expanded.has(f.id) && !collapsedByUser.has(f.id)) expanded.add(f.id);
      const isOpen = hasBodies && expanded.has(f.id);
      const errs = f.diagnostics.filter((d) => d.severity === "error").length;
      const warns = f.diagnostics.filter((d) => d.severity === "warning").length;
      const badge =
        errs || warns
          ? h(
              "span",
              { class: `tree-badge ${errs ? "err" : "warn"}`, title: `${errs} error(s), ${warns} warning(s)` },
              String(errs || warns),
            )
          : undefined;
      const row = rowFor(featureRef(f), f.name || f.id, KIND_GLYPH[f.kind] ?? f.kind.slice(0, 2).toUpperCase(), f.kind, 0, badge);
      if (f.suppressed) row.classList.add("suppressed");
      if (hasBodies) {
        row.setAttribute("aria-expanded", String(isOpen));
        const twisty = h(
          "button",
          {
            type: "button",
            class: "twisty",
            tabIndex: -1,
            aria: { label: isOpen ? "Collapse" : "Expand", hidden: "true" },
            on: {
              click: (ev) => {
                ev.stopPropagation();
                if (expanded.has(f.id)) {
                  expanded.delete(f.id);
                  collapsedByUser.add(f.id);
                } else {
                  expanded.add(f.id);
                  collapsedByUser.delete(f.id);
                }
                render(store.get().model);
              },
            },
          },
          isOpen ? "▾" : "▸",
        );
        row.insertBefore(twisty, row.firstChild);
      } else {
        row.insertBefore(h("span", { class: "twisty-space" }), row.firstChild);
      }
      children.push(row);
      if (isOpen) {
        const group = h("div", { role: "group", class: "tree-group" });
        for (const b of bodies) {
          const brow = rowFor(bodyRef(b), b.name || b.id, "▣", `${b.topology.faces.length} faces`, 1, layerBadge(b), eyeButton(b));
          if (store.get().hiddenBodies.has(b.id)) brow.classList.add("is-hidden");
          group.appendChild(brow);
        }
        children.push(group);
      }
    }
    replaceChildren(tree, ...children);
    items = [...tree.querySelectorAll<HTMLElement>('[role="treeitem"]')];
    applySelection(store.get().selected);
  }

  const collapsedByUser = new Set<string>();

  function applySelection(selected: EntityRef[]): void {
    let active: HTMLElement | null = null;
    for (const [key, row] of rows) {
      const on = selected.some((r) => refKey(r) === key);
      row.setAttribute("aria-selected", String(on));
      row.classList.toggle("selected", on);
      if (on) active = row;
    }
    setRovingTabindex(items, active ?? items.find((i) => i.tabIndex === 0) ?? null);
  }

  store.select((s) => s.model, render);
  store.select(
    (s) => s.selected,
    (sel) => applySelection(sel),
  );
  store.select(
    (s) => s.hiddenBodies,
    () => render(store.get().model),
  );
  store.select(
    (s) => s.hovered,
    (hov) => {
      for (const [key, row] of rows) row.classList.toggle("hovered", hov !== null && refKey(hov) === key);
    },
  );
  // Reveal the row of a newly selected entity (a face/edge reveals its body).
  store.select(
    (s) => s.selected,
    (sel) => {
      const last = sel[sel.length - 1];
      if (!last) return;
      const key =
        last.kind === "face" || last.kind === "edge" || last.kind === "vertex"
          ? refKey({ feature_id: last.feature_id, body_id: last.body_id, kind: "body" })
          : refKey(last);
      rows.get(key)?.scrollIntoView({ block: "nearest" });
    },
  );
  return root;
}
