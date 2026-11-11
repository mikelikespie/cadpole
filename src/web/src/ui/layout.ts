// Application shell. Wide screens: left panel (tabs: Model / Script /
// Examples / API; the Model tab is Parameters above the feature tree, and
// the display layers under it), centre viewport with toolbar, right panel
// (Inspector + Messages), full-width status strip. Narrow screens (<= 900px):
// the viewport fills the screen, a bottom tab bar opens the same panels in a
// bottom sheet, with Parameters as its own first tab and Layers
// the next, there only while the document declares a layer. The sheet
// rests at one of three heights -- a peek, a half that leaves the model in
// view while a slider moves, and full -- and follows the finger between them.
// The toolbar is a slim bar over the viewport there (the document's name and
// one button), and its controls are the View sheet (ui/toolbar.ts). A phone on
// its side gets the same parts turned: the tab bar is a rail down the left
// edge and the sheet a panel down the right. Panels are created once and
// moved between slots.

import type { Actions } from "../actions.js";
import type { AppState, AppStore, LeftTab, SheetTab } from "../state.js";
import { isSectioned } from "../section.js";
import { h } from "./dom.js";
import { icon, type IconName } from "./icons.js";
import { coveredInsets, sameInsets, NO_INSETS, type Insets } from "./insets.js";
import { listLayers } from "./layers.js";
import { KEYBOARD_EVENT } from "../pwa.js";

export interface Panels {
  toolbar: HTMLElement;
  /** The toolbar's controls as a panel: the View sheet (ui/toolbar.ts). */
  tools: HTMLElement;
  /** Parameters: the first sheet tab, above the tree on a wide screen. */
  params: HTMLElement;
  /** The display layers: the next sheet tab, under the tree on a wide screen. */
  layers: HTMLElement;
  model: HTMLElement;
  script: HTMLElement;
  examples: HTMLElement;
  api: HTMLElement;
  inspector: HTMLElement;
  messages: HTMLElement;
  diagnostics: HTMLElement;
}

export interface Layout {
  root: HTMLElement;
  /** Container for the viewer canvas and overlays. */
  viewport: HTMLElement;
  banner: HTMLElement;
  /**
   * How much of the viewport's edges the shell's own panels cover, in CSS
   * pixels: called with the current value, and again whenever it changes.
   * Zero on a wide screen, where the panels stand beside the viewport.
   */
  onInsets(cb: (insets: Insets) => void): void;
}

// Narrow, or a phone on its side: the largest phones are wider than 900 px
// that way round and no taller than 440. Kept in step with the `@media` lists
// in styles.css.
const NARROW_QUERY = "(max-width: 900px), (pointer: coarse) and (max-height: 500px)";
/**
 * A phone on its side: the sheet is a side panel there, with no heights to
 * drag between. A narrow layout that is also short and wide -- never a
 * desktop window that merely is.
 */
const SIDE_QUERY = "(orientation: landscape) and (max-height: 500px) and (max-width: 900px), (orientation: landscape) and (max-height: 500px) and (pointer: coarse)";

/** Where the sheet rests. */
type Detent = "peek" | "half" | "full";

const LEFT_TABS: { id: LeftTab; label: string }[] = [
  { id: "model", label: "Model" },
  { id: "script", label: "Script" },
  { id: "examples", label: "Examples" },
  { id: "api", label: "API" },
];

const SHEET_TABS: { id: SheetTab; label: string; icon: IconName }[] = [
  { id: "params", label: "Params", icon: "params" },
  { id: "layers", label: "Layers", icon: "layers" },
  { id: "model", label: "Model", icon: "model" },
  { id: "script", label: "Script", icon: "script" },
  { id: "inspector", label: "Inspect", icon: "inspector" },
  { id: "messages", label: "Chat", icon: "messages" },
  { id: "issues", label: "Issues", icon: "issues" },
  { id: "examples", label: "Examples", icon: "examples" },
  { id: "api", label: "API", icon: "api" },
  // Last, under the right thumb: the view presets, the display mode and the
  // rest of what the desktop toolbar holds.
  { id: "view", label: "View", icon: "view" },
];

export function createLayout(store: AppStore, actions: Actions, panels: Panels): Layout {
  const sections: Record<SheetTab, HTMLElement> = {
    params: panels.params,
    layers: panels.layers,
    model: panels.model,
    script: panels.script,
    examples: panels.examples,
    api: panels.api,
    inspector: panels.inspector,
    messages: panels.messages,
    issues: panels.diagnostics,
    view: panels.tools,
  };

  // ---- wide: left panel with tabs
  const leftTabButtons = LEFT_TABS.map((t) =>
    h(
      "button",
      {
        type: "button",
        role: "tab",
        class: "tab",
        id: `tab-${t.id}`,
        aria: { selected: "false", controls: `tabpanel-${t.id}` },
        on: {
          click: () => actions.setLeftTab(t.id),
          keydown: (ev) => {
            const i = LEFT_TABS.findIndex((x) => x.id === t.id);
            if (ev.key === "ArrowRight" || ev.key === "ArrowLeft") {
              ev.preventDefault();
              const n = LEFT_TABS.length;
              const next = LEFT_TABS[(i + (ev.key === "ArrowRight" ? 1 : -1) + n) % n] as { id: LeftTab };
              actions.setLeftTab(next.id);
              leftTabButtons[LEFT_TABS.findIndex((x) => x.id === next.id)]?.focus();
            }
          },
        },
      },
      t.label,
    ),
  );
  // The Model tab on a wide screen: Parameters, the feature tree, then the
  // layers. Each has a slot of its own so that coming back from the narrow
  // layout, where Parameters and Layers live in the sheet, puts each back
  // where it was, not after the tree.
  const paramsHome = h("div", { class: "panel-slot" });
  const treeHome = h("div", { class: "panel-slot" });
  const layersHome = h("div", { class: "panel-slot" });
  const leftPanels: Record<LeftTab, HTMLElement> = {
    model: h(
      "div",
      { class: "tabpanel", role: "tabpanel", id: "tabpanel-model", aria: { labelledby: "tab-model" } },
      paramsHome,
      treeHome,
      layersHome,
    ),
    script: h("div", { class: "tabpanel", role: "tabpanel", id: "tabpanel-script", aria: { labelledby: "tab-script" } }),
    examples: h("div", { class: "tabpanel", role: "tabpanel", id: "tabpanel-examples", aria: { labelledby: "tab-examples" } }),
    api: h("div", { class: "tabpanel", role: "tabpanel", id: "tabpanel-api", aria: { labelledby: "tab-api" } }),
  };
  const left = h(
    "aside",
    { class: "panel panel-left", aria: { label: "Model panel" } },
    h("div", { class: "tabs", role: "tablist", aria: { label: "Left panel" } }, ...leftTabButtons),
    leftPanels.model,
    leftPanels.script,
    leftPanels.examples,
    leftPanels.api,
  );

  // ---- wide: right panel
  const rightTop = h("div", { class: "right-top" });
  const rightBottom = h("div", { class: "right-bottom" });
  const right = h("aside", { class: "panel panel-right", aria: { label: "Inspector panel" } }, rightTop, rightBottom);

  // ---- centre
  const banner = h("div", { class: "banner", role: "status" });
  banner.hidden = true;
  const viewport = h("div", { class: "viewport" }, banner);
  // No callout, no text selection, no long-press menu over the model: a
  // finger held on it is a gesture of the viewer's.
  viewport.addEventListener("contextmenu", (ev) => ev.preventDefault());
  const toolbarSlot = h("div", { class: "toolbar-slot" }, panels.toolbar);
  const statusSlot = h("div", { class: "status-slot" });

  // ---- narrow: tab bar + sheet
  const badge = h("span", { class: "tab-badge" });
  badge.hidden = true;
  const tabbarButtons = SHEET_TABS.filter((t) => t.id !== "examples" && t.id !== "api").map((t) =>
    h(
      "button",
      {
        type: "button",
        class: "tabbar-btn",
        dataset: { tab: t.id },
        aria: { label: t.label, pressed: "false" },
        on: {
          click: () => {
            const s = store.get();
            if (s.sheet === t.id) actions.closeSheet();
            else actions.openSheet(t.id);
          },
        },
      },
      h("span", { class: "tabbar-glyph", aria: { hidden: "true" } }, icon(t.icon)),
      h("span", { class: "tabbar-label" }, t.label),
      t.id === "issues" ? badge : null,
    ),
  );
  const tabbar = h("nav", { class: "tabbar", aria: { label: "Panels" } }, ...tabbarButtons);

  const sheetTitle = h("h2", { class: "sheet-title", id: "sheet-title" }, "");
  const sheetClose = h(
    "button",
    { type: "button", class: "icon-btn sheet-close", aria: { label: "Close panel" }, on: { click: () => actions.closeSheet() } },
    icon("close", 20),
  );
  const sheetTabs = h(
    "div",
    { class: "sheet-tabs", role: "tablist", aria: { label: "Panel" } },
    ...SHEET_TABS.filter((t) => t.id !== "view").map((t) =>
      h(
        "button",
        {
          type: "button",
          role: "tab",
          class: "tab",
          dataset: { tab: t.id },
          aria: { selected: "false" },
          on: { click: () => actions.openSheet(t.id) },
        },
        t.label,
      ),
    ),
  );
  const sheetBody = h("div", { class: "sheet-body" });
  const handle = h("div", { class: "sheet-handle", aria: { hidden: "true" } });
  const sheetHead = h("div", { class: "sheet-head" }, sheetTitle, sheetClose);
  const sheet = h("section", { class: "sheet", role: "dialog", aria: { modal: "false", labelledby: "sheet-title" } }, handle, sheetHead, sheetTabs, sheetBody);
  sheet.hidden = true;
  sheet.addEventListener("keydown", (ev) => {
    if (ev.key === "Escape") {
      ev.preventDefault();
      actions.closeSheet();
    }
  });

  // ---- the sheet's heights
  //
  // Three resting heights (styles.css `--sheet-peek`, `--sheet-half`, and
  // full), and the sheet follows the finger between them: the handle and the
  // title row are the grip. Where a tab was left is remembered per tab while
  // the page lives -- the script reads better tall, a slider is dragged with
  // the model in view, the view presets want the model nearly whole.
  const rest = new Map<SheetTab, Detent>();
  const side = window.matchMedia(SIDE_QUERY);
  // The view presets and the chat open low: both are used with an eye on the
  // model, and what a message changes should be in view when it changes.
  const detentOf = (tab: SheetTab): Detent => rest.get(tab) ?? (tab === "view" || tab === "messages" ? "peek" : "half");
  const applyHeight = (): void => {
    const tab = store.get().sheet;
    const d = tab ? detentOf(tab) : "half";
    sheet.classList.toggle("full", d === "full");
    sheet.classList.toggle("peek", d === "peek");
    sheet.dataset["detent"] = d;
  };
  /** The resting heights in px: the same arithmetic as styles.css `--sheet-peek` and `--sheet-half`. */
  const detents = (): Record<Detent, number> => {
    const vh = window.innerHeight;
    const full = Math.max(120, vh - tabbar.offsetHeight - 8);
    return { peek: Math.min(Math.max(190, Math.min(vh * 0.27, 250)), full), half: Math.min(vh * 0.42, full), full };
  };
  /** When the last drag ended: the click that follows a drag on the handle is not a tap. */
  let draggedAt = -1e9;
  let drag: { id: number; y0: number; h0: number; from: Detent; moved: boolean } | null = null;
  const onGripDown = (ev: PointerEvent): void => {
    const tab = store.get().sheet;
    if (!tab || side.matches || drag) return;
    if ((ev.target as HTMLElement).closest("button")) return;
    drag = { id: ev.pointerId, y0: ev.clientY, h0: sheet.getBoundingClientRect().height, from: detentOf(tab), moved: false };
    (ev.currentTarget as HTMLElement).setPointerCapture(ev.pointerId);
  };
  const onGripMove = (ev: PointerEvent): void => {
    if (!drag || ev.pointerId !== drag.id) return;
    const dy = ev.clientY - drag.y0;
    if (!drag.moved && Math.abs(dy) < 4) return;
    drag.moved = true;
    sheet.classList.add("dragging");
    sheet.style.height = `${Math.max(64, Math.min(detents().full, drag.h0 - dy))}px`;
  };
  const onGripUp = (ev: PointerEvent): void => {
    if (!drag || ev.pointerId !== drag.id) return;
    const d = drag;
    drag = null;
    sheet.classList.remove("dragging");
    sheet.style.height = "";
    const tab = store.get().sheet;
    if (!tab || !d.moved || ev.type === "pointercancel") return;
    draggedAt = performance.now();
    const hs = detents();
    const height = Math.max(0, d.h0 - (ev.clientY - d.y0));
    const order: Detent[] = ["peek", "half", "full"];
    const at = order.indexOf(d.from);
    // A pull settles on the first resting height in its direction that the
    // finger reached (within a little): a short pull moves one step, a long
    // one goes all the way, and a pull down past the peek puts the sheet away.
    const SLOP = 40;
    let to: Detent | null = d.from;
    if (height > d.h0 + SLOP) {
      to = order.slice(at + 1).find((o) => hs[o] >= height - SLOP) ?? "full";
    } else if (height < d.h0 - SLOP) {
      to =
        order
          .slice(0, at)
          .reverse()
          .find((o) => hs[o] <= height + SLOP) ?? null;
      if (to === null && height > hs.peek - SLOP && at > 0) to = "peek";
    }
    if (to === null) {
      actions.closeSheet();
      return;
    }
    rest.set(tab, to);
    applyHeight();
  };
  for (const grip of [handle, sheetHead]) {
    grip.addEventListener("pointerdown", onGripDown);
    grip.addEventListener("pointermove", onGripMove);
    grip.addEventListener("pointerup", onGripUp);
    grip.addEventListener("pointercancel", onGripUp);
  }
  // A tap on the handle steps the sheet up one height, and from full back to
  // the half: the way to resize it without a drag.
  handle.addEventListener("click", () => {
    const tab = store.get().sheet;
    if (!tab || side.matches || performance.now() - draggedAt < 400) return;
    const d = detentOf(tab);
    rest.set(tab, d === "peek" ? "half" : d === "half" ? "full" : "half");
    applyHeight();
  });

  // ---- narrow: the view buttons that are always under the thumb. The
  // section button stands there only while the view is cut -- a page that
  // opens sectioned keeps its panel shut on a phone, where it would cover the
  // model, and this is how a reader finds the cut and turns it off.
  const fabSection = h("button", { type: "button", class: "fab-btn fab-section", title: "Section", hidden: true, aria: { label: "Section", pressed: "false" }, on: { click: () => actions.toggleSectionPanel() } }, icon("section"));
  const fab = h(
    "div",
    { class: "vp-fab", role: "group", aria: { label: "View" } },
    fabSection,
    h("button", { type: "button", class: "fab-btn", title: "Home view", aria: { label: "Home view" }, on: { click: () => actions.setView("home") } }, icon("home")),
    h("button", { type: "button", class: "fab-btn", title: "Fit all", aria: { label: "Fit all" }, on: { click: () => actions.fit() } }, icon("fit")),
  );

  const root = h("div", { class: "app" }, toolbarSlot, left, viewport, right, statusSlot, tabbar, sheet);

  // ---- placement
  const homes = new Map<HTMLElement, HTMLElement>([
    [panels.params, paramsHome],
    [panels.model, treeHome],
    [panels.layers, layersHome],
    [panels.script, leftPanels.script],
    [panels.examples, leftPanels.examples],
    [panels.api, leftPanels.api],
    [panels.inspector, rightTop],
    [panels.messages, rightBottom],
    [panels.diagnostics, statusSlot],
  ]);
  let lastFocus: HTMLElement | null = null;

  // ---- what the sheet covers
  //
  // The sheet lies over the viewport, not beside it, so the viewer is told
  // how far up it reaches and draws the model in what is left
  // (`Viewer.setViewportInsets`); the triad and the view buttons stand clear
  // of it by the same measure (styles.css `--cover-b`). Measured from the
  // boxes, so every height the sheet takes -- a rest, a drag, the keyboard --
  // and every layout it has is the same code: on its side the sheet is a
  // column beside the viewport and covers nothing.
  let insets: Insets = NO_INSETS;
  const insetListeners: ((i: Insets) => void)[] = [];
  const syncInsets = (): void => {
    // The keyboard covers the bottom of the page (`--kbd`, pwa.ts), and the
    // sheet stands on it.
    const kbd = Number.parseFloat(document.documentElement.style.getPropertyValue("--kbd")) || 0;
    const covers = [sheet.getBoundingClientRect(), { left: 0, right: window.innerWidth, top: window.innerHeight - kbd, bottom: window.innerHeight }];
    const next = sheet.hidden || !store.get().narrow ? NO_INSETS : coveredInsets(viewport.getBoundingClientRect(), covers);
    if (sameInsets(next, insets)) return;
    insets = next;
    viewport.style.setProperty("--cover-b", `${Math.round(next.bottom)}px`);
    for (const cb of insetListeners) cb(next);
  };
  const insetObserver = new ResizeObserver(syncInsets);
  insetObserver.observe(sheet);
  insetObserver.observe(viewport);
  window.addEventListener(KEYBOARD_EVENT, syncInsets);

  function place(): void {
    placePanels();
    syncInsets();
  }

  function placePanels(): void {
    const s = store.get();
    root.classList.toggle("narrow", s.narrow);
    if (!s.narrow) {
      for (const [el, home] of homes) if (el.parentElement !== home) home.appendChild(el);
      sheet.hidden = true;
      return;
    }
    if (s.sheet) {
      const el = sections[s.sheet];
      if (el.parentElement !== sheetBody) sheetBody.replaceChildren(el);
      sheetTitle.textContent = SHEET_TABS.find((t) => t.id === s.sheet)?.label ?? "";
      sheet.hidden = false;
      sheet.dataset["tab"] = s.sheet;
      applyHeight();
    } else {
      sheet.hidden = true;
      sheet.classList.remove("full", "peek");
    }
    root.classList.toggle("sheet-open", s.sheet !== null);
    for (const b of tabbarButtons) b.setAttribute("aria-pressed", String(b.dataset["tab"] === s.sheet));
    for (const b of sheetTabs.querySelectorAll<HTMLElement>("[role=tab]")) {
      const on = b.dataset["tab"] === s.sheet;
      b.setAttribute("aria-selected", String(on));
      b.classList.toggle("active", on);
      // The strip scrolls; the tab that is open should be the one in view.
      if (on && s.sheet) b.scrollIntoView({ block: "nearest", inline: "nearest" });
    }
  }

  function applyLeftTab(tab: LeftTab): void {
    for (const b of leftTabButtons) {
      const on = b.id === `tab-${tab}`;
      b.setAttribute("aria-selected", String(on));
      b.classList.toggle("active", on);
      b.tabIndex = on ? 0 : -1;
    }
    for (const [id, p] of Object.entries(leftPanels)) p.hidden = id !== tab;
  }

  // The banner is painted here rather than only on a change, because a notice
  // that is already in the initial state -- "this browser has no valid token",
  // say -- has no previous value to differ from and would otherwise never be
  // shown.
  function showNotice(notice: string | null): void {
    banner.hidden = !notice;
    banner.replaceChildren(
      h("span", null, notice ?? ""),
      h(
        "button",
        {
          type: "button",
          class: "icon-btn",
          aria: { label: "Dismiss" },
          on: {
            click: () => {
              banner.hidden = true;
            },
          },
        },
        "\u00d7",
      ),
    );
  }

  // Layers is a tab only while the document declares a layer, as the section
  // is only shown then; an empty slot would still draw its rule. A
  // Layers sheet left open when the layers go -- a restored sheet, a script
  // edited elsewhere -- falls back to Params rather than stand empty.
  function showLayersTab(s: AppState): void {
    const has = listLayers(s.model).length > 0;
    layersHome.hidden = !has;
    for (const b of [...tabbarButtons, ...sheetTabs.querySelectorAll<HTMLElement>("[role=tab]")]) {
      if (b.dataset["tab"] === "layers") b.hidden = !has;
    }
    if (!has && s.model && s.sheet === "layers") actions.openSheet("params");
  }

  // A published page is for looking: with nothing to report, Issues
  // is not a tab there. In the app it stays, as the place a mistake shows up.
  function showIssuesTab(s: AppState): void {
    const none = s.apiMode === "published" && (s.model?.diagnostics.length ?? 0) === 0;
    for (const b of tabbarButtons) if (b.dataset["tab"] === "issues") b.hidden = none;
    if (none && s.model && s.sheet === "issues") actions.closeSheet();
  }

  const syncFabSection = (s: AppState): void => {
    fabSection.hidden = !isSectioned(s.section) && !s.sectionPanel;
    fabSection.setAttribute("aria-pressed", String(s.sectionPanel));
  };
  syncFabSection(store.get());

  const mq = window.matchMedia(NARROW_QUERY);
  const syncNarrow = (): void => store.set({ narrow: mq.matches });
  mq.addEventListener("change", syncNarrow);
  syncNarrow();

  store.subscribe((s, prev) => {
    if (s.narrow !== prev.narrow || s.sheet !== prev.sheet) {
      const opening = s.sheet && s.sheet !== prev.sheet;
      const closing = !s.sheet && prev.sheet;
      if (opening) lastFocus = document.activeElement as HTMLElement | null;
      place();
      if (opening && s.narrow) sheetClose.focus();
      if (closing) {
        const btn = tabbarButtons.find((b) => b.dataset["tab"] === prev.sheet);
        (btn ?? lastFocus)?.focus();
      }
    }
    if (s.section !== prev.section || s.sectionPanel !== prev.sectionPanel) syncFabSection(s);
    if (s.leftTab !== prev.leftTab) applyLeftTab(s.leftTab);
    if (s.model !== prev.model) showLayersTab(s);
    if (s.model !== prev.model || s.apiMode !== prev.apiMode) showIssuesTab(s);
    if (s.engineNotice !== prev.engineNotice) showNotice(s.engineNotice);
    if (s.model !== prev.model) {
      const n = s.model?.diagnostics.filter((d) => d.severity !== "info").length ?? 0;
      badge.hidden = n === 0;
      badge.textContent = String(n);
      badge.classList.toggle("err", Boolean(s.model?.diagnostics.some((d) => d.severity === "error")));
    }
  });
  applyLeftTab(store.get().leftTab);
  showLayersTab(store.get());
  showIssuesTab(store.get());
  viewport.appendChild(fab);
  showNotice(store.get().engineNotice);
  place();
  return {
    root,
    viewport,
    banner,
    onInsets(cb) {
      insetListeners.push(cb);
      cb(insets);
    },
  };
}
