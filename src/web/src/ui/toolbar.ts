// Slim app toolbar: document name, view presets, fit/focus, display mode,
// snapshot, a "View" menu with the overlay/projection/scroll options, and
// status pills. Every gesture has a button here so nothing is gesture-only.
//
// That is the bar on a desk. On a phone the same controls do not fit a 390 px
// row, so the narrow layout keeps the bar to the document's name and one
// button, and the controls themselves move -- the same elements, not copies --
// into the View sheet (`tools`), where they are laid out for a thumb: view
// presets as a grid, the display mode as chips, the overlays as switches.

import type { Actions } from "../actions.js";
import type { EngineKind } from "../engine.js";
import type { EnginePreference } from "../engine-policy.js";
import type { AppState, AppStore } from "../state.js";
import { VIEW_MODES, type ViewMode } from "../viewer.js";
import { button, h } from "./dom.js";
import { cadpoleMark, icon } from "./icons.js";

const ENGINE_PILL: Record<EngineKind, string> = { worker: "worker", wasm: "main thread", remote: "server", mock: "mock", baked: "published" };
const ENGINE_TITLE: Record<EngineKind, string> = {
  worker: "Evaluating in a worker, off the main thread",
  wasm: "Evaluating on the main thread: a slow model will freeze the page",
  remote: "Evaluating on the Cadpole server",
  mock: "Mock engine",
  baked: "A published model, evaluated ahead of time by cadpole publish: read-only",
};

export interface Toolbar {
  /** The bar itself: everything on a wide screen, the name and one button on a narrow one. */
  bar: HTMLElement;
  /** The View sheet's panel: where the bar's controls live while the layout is narrow. */
  tools: HTMLElement;
}

/** A published page's way back to the site it belongs to (`cadpole publish --home-url`). */
export interface HomeLink {
  /** An http(s) address, already checked (`safeHomeUrl`). */
  href: string;
  label: string;
}

export function createToolbar(store: AppStore, actions: Actions, home: HomeLink | null = null): Toolbar {
  const name = h("span", { class: "doc-name", title: "Document" }, "Cadpole");
  // Before the title, where a back control is looked for; the mark says
  // whose page it is, the label says where it goes.
  const homeLink = home
    ? h("a", { class: "home-link", href: home.href, title: home.label, aria: { label: home.label } }, cadpoleMark(), icon("back"))
    : null;
  const enginePill = h("span", { class: "pill", role: "status" });
  const connPill = h("span", { class: "pill", role: "status" });
  const installBtn = button("Install", { title: "Install Cadpole as an app", class: "btn-quiet install-btn", onClick: () => void actions.install() });
  installBtn.hidden = true;

  const views = h(
    "div",
    { class: "btn-group", role: "group", aria: { label: "View presets" } },
    button("Home", { title: "Home view (H)", icon: "⌂", onClick: () => actions.setView("home") }),
    button("Iso", { title: "Isometric (I)", onClick: () => actions.setView("iso") }),
    button("Top", { title: "Top (7)", onClick: () => actions.setView("top") }),
    button("Front", { title: "Front (1)", onClick: () => actions.setView("front") }),
    button("Right", { title: "Right (3)", onClick: () => actions.setView("right") }),
  );
  const fitBtn = button("Fit", { title: "Fit all (F)", icon: "⛶", onClick: () => actions.fit() });
  const focusBtn = button("Focus", { title: "Focus on selection (.)", icon: "◎", onClick: () => actions.focusSelection() });
  const camGroup = h("div", { class: "btn-group", role: "group", aria: { label: "Camera" } }, fitBtn, focusBtn);
  // A section: the panel that cuts the view, on any page. Named by
  // aria-label, since a narrow toolbar shows only its icon.
  const sectionBtn = button("Section", { title: "Cut the view with a horizontal and a vertical plane (C)", icon: "✂", class: "section-btn", pressed: false, onClick: () => {
    actions.toggleSectionPanel();
    // The panel opens over the viewport; on a phone the sheet this button
    // sits in would be covering it.
    if (store.get().narrow) actions.closeSheet();
  } });
  sectionBtn.setAttribute("aria-label", "Section");

  // ---- model view <-> plates view
  //
  // Only offered when the document has print plates: a script with no
  // `export(..., flavor=flavor.bambu)` has nothing for the plates view to
  // show, and a button that does nothing is worse than none.
  const modelBtn = button("Model", { title: "The model where the script built it", pressed: true, onClick: () => actions.setPlatesView(false) });
  const platesBtn = button("Plates", {
    title: "The print plates: every body where cadpole export would put it, in its filament's colour",
    icon: "▦",
    pressed: false,
    onClick: () => actions.setPlatesView(true),
  });
  const viewGroup = h("div", { class: "btn-group view-group", role: "group", aria: { label: "View" } }, modelBtn, platesBtn);
  viewGroup.hidden = true;

  const modeSel = h(
    "select",
    { class: "select", title: "Display mode (V cycles, E toggles edges, X toggles x-ray)", aria: { label: "Display mode" } },
    ...VIEW_MODES.map((m) => h("option", { value: m.id }, m.label)),
  );
  modeSel.addEventListener("change", () => actions.setViewMode(modeSel.value as ViewMode));

  const snapBtn = button("Snapshot", {
    title: "Capture the viewport for the next message (Shift+S). Shift-click: without selection labels.",
    icon: "◫",
    onClick: (ev) => void actions.snapshot({ annotate: !ev.shiftKey }),
  });

  // ---- "View" menu (overlays, projection, scroll behaviour)
  const check = (label: string, title: string, onToggle: () => void): HTMLButtonElement =>
    h(
      "button",
      {
        type: "button",
        role: "menuitemcheckbox",
        class: "menu-item",
        title,
        aria: { checked: "false" },
        on: { click: onToggle },
      },
      h("span", { class: "menu-check", aria: { hidden: "true" } }, "✓"),
      h("span", null, label),
    );
  const orthoItem = check("Orthographic", "Toggle orthographic projection (5)", () => actions.toggleOrtho());
  const gridItem = check("Grid", "Toggle ground grid (G)", () => actions.toggleGrid());
  const labelsItem = check("Selection labels", "Toggle selection labels (L)", () => actions.toggleLabels());
  const bboxItem = check("Bounding box", "Toggle bounding box with dimensions (B)", () => actions.toggleBBox());
  const hudItem = check("Performance HUD", "Toggle performance HUD (P)", () => actions.toggleHud());
  const LINK_LABEL = "Copy link to this view";
  const linkLabel = h("span", null, LINK_LABEL);
  const linkItem = h(
    "button",
    {
      type: "button",
      class: "menu-item view-link",
      role: "menuitem",
      title: "Put a link to this view (camera, display mode, section) in the address bar and on the clipboard",
      on: {
        click: () =>
          void actions.copyViewLink().then(() => {
            linkLabel.textContent = "Link copied";
            window.setTimeout(() => (linkLabel.textContent = LINK_LABEL), 1800);
          }),
      },
    },
    h("span", { class: "menu-check", aria: { hidden: "true" } }, "✓"),
    linkLabel,
  );
  const wheelSel = h(
    "select",
    { class: "select", id: "wheel-mode", title: "Scroll wheel / trackpad behaviour" },
    h("option", { value: "auto" }, "Auto-detect"),
    h("option", { value: "orbit" }, "Pan (trackpad)"),
    h("option", { value: "zoom" }, "Zoom (mouse wheel)"),
  );
  wheelSel.value = actions.wheelMode();
  wheelSel.addEventListener("change", () => actions.setWheelMode(wheelSel.value as "auto" | "orbit" | "zoom"));
  const menuPanel = h(
    "div",
    { class: "menu-panel", role: "menu", aria: { label: "View options" } },
    orthoItem,
    h("div", { class: "menu-sep", role: "separator" }),
    gridItem,
    labelsItem,
    bboxItem,
    hudItem,
    h("div", { class: "menu-sep", role: "separator" }),
    linkItem,
    h("div", { class: "menu-sep", role: "separator" }),
    h("label", { class: "menu-field", htmlFor: "wheel-mode" }, h("span", null, "Scroll"), wheelSel),
  );
  const summary = h("summary", { class: "btn", title: "View options" }, h("span", { class: "btn-icon", aria: { hidden: "true" } }, "⚙"), h("span", null, "View"));
  const menu = h("details", { class: "menu" }, summary, menuPanel);
  const items = [orthoItem, gridItem, labelsItem, bboxItem, hudItem];
  // The toolbar scrolls horizontally when narrow (which also clips vertical
  // overflow), so the panel is fixed-positioned from the button's rect.
  const place = (): void => {
    const r = summary.getBoundingClientRect();
    const w = menuPanel.offsetWidth;
    const left = Math.max(8, Math.min(window.innerWidth - w - 8, r.right - w));
    menuPanel.style.left = `${left}px`;
    menuPanel.style.top = `${r.bottom + 4}px`;
  };
  menu.addEventListener("toggle", () => {
    summary.setAttribute("aria-expanded", String(menu.open));
    if (menu.open) {
      place();
      items[0]?.focus();
    }
  });
  window.addEventListener("resize", () => {
    if (menu.open) place();
  });
  menu.addEventListener("keydown", (ev) => {
    if (ev.key === "Escape" && menu.open) {
      ev.preventDefault();
      menu.open = false;
      summary.focus();
      return;
    }
    if (ev.key === "ArrowDown" || ev.key === "ArrowUp") {
      const i = items.indexOf(document.activeElement as HTMLButtonElement);
      if (i < 0) return;
      ev.preventDefault();
      items[(i + (ev.key === "ArrowDown" ? 1 : -1) + items.length) % items.length]?.focus();
    }
  });
  document.addEventListener("pointerdown", (ev) => {
    if (menu.open && !menu.contains(ev.target as Node)) menu.open = false;
  });

  const saveBtn = button("Save", { title: "Save document (Ctrl/Cmd+S)", class: "btn-quiet", onClick: () => void actions.save() });

  // ---- where evaluation runs
  //
  // Visible and overridable on purpose: the automatic policy changes engines
  // between one evaluation and the next based on what it measured, and "why is
  // this slow" is unanswerable if the user cannot see which one ran or insist
  // on the other.
  const engineSel = h(
    "select",
    { class: "select engine-select", id: "engine-mode", aria: { label: "Where evaluation runs" } },
    h("option", { value: "auto" }, "Engine: auto"),
    h("option", { value: "local" }, "Engine: this machine"),
    h("option", { value: "remote" }, "Engine: the server"),
  );
  const remoteOption = engineSel.querySelector('option[value="remote"]') as HTMLOptionElement;
  engineSel.addEventListener("change", () => actions.setEnginePreference(engineSel.value as EnginePreference));

  const center = h("div", { class: "toolbar-center" }, views, camGroup, sectionBtn, viewGroup, modeSel, snapBtn, menu);
  const right = h("div", { class: "toolbar-right" }, engineSel, saveBtn, installBtn, enginePill, connPill);
  const root = h("div", { class: "toolbar", role: "toolbar", aria: { label: "Viewport toolbar" } }, h("div", { class: "toolbar-left" }, ...(homeLink ? [homeLink] : []), name), center, right);

  // ---- narrow: the one button, and the View sheet the controls move into
  const moreBtn = h(
    "button",
    {
      type: "button",
      class: "icon-btn toolbar-more",
      title: "View and tools",
      aria: { label: "View and tools", haspopup: "dialog", expanded: "false" },
      on: {
        click: () => {
          if (store.get().sheet === "view") actions.closeSheet();
          else actions.openSheet("view");
        },
      },
    },
    icon("more"),
  );
  // The display mode as chips: five choices are one tap each, where the
  // desktop's <select> is a tap, a wheel and a "Done".
  const modeChips = h(
    "div",
    { class: "chips-row mode-chips", role: "radiogroup", aria: { label: "Display mode" } },
    ...VIEW_MODES.map((m) =>
      h(
        "button",
        { type: "button", class: "chip-btn", role: "radio", dataset: { mode: m.id }, aria: { checked: "false" }, on: { click: () => actions.setViewMode(m.id) } },
        m.label,
      ),
    ),
  );
  const setModeChips = (mode: ViewMode): void => {
    for (const c of modeChips.querySelectorAll<HTMLElement>("[role=radio]")) c.setAttribute("aria-checked", String(c.dataset["mode"] === mode));
  };
  const group = (title: string, cls: string, ...children: HTMLElement[]): HTMLElement =>
    h("div", { class: `tools-group ${cls}`, role: "group", aria: { label: title } }, h("h3", { class: "tools-title" }, title), ...children);
  const viewsRow = h("div", { class: "tools-row tools-views" });
  const displayRow = h("div", { class: "tools-row tools-display" });
  const docRow = h("div", { class: "tools-row tools-doc" });
  const showGroup = group("Show", "tools-show");
  const docGroup = group("Document", "tools-document", docRow);
  const tools = h(
    "section",
    { class: "panel-section tools", aria: { label: "View and tools" } },
    group("Views", "tools-camera", viewsRow),
    group("Display", "tools-mode", modeChips, displayRow),
    showGroup,
    docGroup,
  );
  let homedNarrow = false;
  /** Put every control where this layout shows it. The elements move; nothing is built twice. */
  const rehome = (narrow: boolean): void => {
    if (narrow === homedNarrow) return;
    homedNarrow = narrow;
    root.classList.toggle("compact", narrow);
    if (narrow) {
      menu.open = false;
      menuPanel.style.left = "";
      menuPanel.style.top = "";
      viewsRow.append(views, camGroup);
      displayRow.append(sectionBtn, viewGroup);
      showGroup.append(menuPanel);
      docRow.append(engineSel, saveBtn, snapBtn, installBtn, enginePill, connPill);
      right.append(moreBtn);
    } else {
      moreBtn.remove();
      center.append(views, camGroup, sectionBtn, viewGroup, modeSel, snapBtn, menu);
      menu.append(menuPanel);
      right.append(engineSel, saveBtn, installBtn, enginePill, connPill);
    }
  };

  const setChecked = (el: HTMLElement, on: boolean): void => {
    el.setAttribute("aria-checked", String(on));
    el.classList.toggle("checked", on);
  };

  /** The pill names the engine that ran the last evaluation, not the loaded one. */
  function setEnginePill(s: AppState): void {
    enginePill.textContent = ENGINE_PILL[s.engineKind];
    enginePill.className = `pill pill-${s.engineKind}`;
    enginePill.title =
      s.engineKind === "mock"
        ? (s.engineNotice ?? "Mock engine: no wasm build is loaded")
        : s.evalInfo?.why
          ? `${ENGINE_TITLE[s.engineKind]} — ${s.evalInfo.why}`
          : ENGINE_TITLE[s.engineKind];
  }

  function setRemoteAvailable(available: boolean): void {
    remoteOption.disabled = !available;
    remoteOption.title = available ? "" : "No server to offload to";
    engineSel.title = available
      ? "Where evaluation runs. Auto measures this document and picks."
      : "Evaluation runs in this browser: there is no server to offload to.";
  }

  const setPlatesView = (s: AppState): void => {
    const has = (s.model?.plates.length ?? 0) > 0;
    viewGroup.hidden = !has;
    modelBtn.setAttribute("aria-pressed", String(!s.platesView));
    platesBtn.setAttribute("aria-pressed", String(s.platesView));
  };

  store.subscribe((s, prev) => {
    if (s.narrow !== prev.narrow) rehome(s.narrow);
    if (s.sheet !== prev.sheet) moreBtn.setAttribute("aria-expanded", String(s.sheet === "view"));
    if (s.session !== prev.session) name.textContent = s.session?.document.name || "Cadpole";
    if (s.model !== prev.model || s.platesView !== prev.platesView) setPlatesView(s);
    if (s.engineKind !== prev.engineKind || s.engineNotice !== prev.engineNotice || s.evalInfo !== prev.evalInfo) {
      setEnginePill(s);
    }
    if (s.enginePreference !== prev.enginePreference) engineSel.value = s.enginePreference;
    if (s.remoteAvailable !== prev.remoteAvailable) setRemoteAvailable(s.remoteAvailable);
    if (s.connection !== prev.connection) {
      connPill.textContent = s.connection;
      connPill.className = `pill pill-conn conn-${s.connection}`;
      connPill.title = s.connection === "static" ? "No session server: state is kept in this browser" : `Session server connection: ${s.connection}`;
    }
    if (s.viewMode !== prev.viewMode) {
      modeSel.value = s.viewMode;
      setModeChips(s.viewMode);
    }
    if (s.sectionPanel !== prev.sectionPanel || s.section !== prev.section) {
      sectionBtn.setAttribute("aria-pressed", String(s.sectionPanel));
      sectionBtn.classList.toggle("cutting", s.section.horizontal !== null || s.section.vertical !== null);
    }
    if (s.showGrid !== prev.showGrid) setChecked(gridItem, s.showGrid);
    if (s.showLabels !== prev.showLabels) setChecked(labelsItem, s.showLabels);
    if (s.showBBox !== prev.showBBox) setChecked(bboxItem, s.showBBox);
    if (s.orthographic !== prev.orthographic) setChecked(orthoItem, s.orthographic);
    if (s.showHud !== prev.showHud) setChecked(hudItem, s.showHud);
    if (s.installable !== prev.installable) installBtn.hidden = !s.installable;
    if (s.apiMode !== prev.apiMode) saveBtn.title = s.apiMode === "session" ? "Save the document on the server" : "Download main.star";
    if (s.pendingAttachments !== prev.pendingAttachments) {
      const n = s.pendingAttachments.length;
      snapBtn.setAttribute("aria-label", n ? `Snapshot (${n} queued)` : "Snapshot");
      snapBtn.classList.toggle("has-badge", n > 0);
      snapBtn.dataset["count"] = String(n);
    }
  });
  const s0 = store.get();
  // A published page has nothing to save, no engine to choose and
  // no message to attach a snapshot to; its connection pill says so.
  if (s0.apiMode === "published") {
    saveBtn.hidden = true;
    engineSel.hidden = true;
    snapBtn.hidden = true;
    connPill.hidden = true;
    // Which leaves the Document group nothing to offer but "published".
    docGroup.hidden = true;
  }
  name.textContent = s0.session?.document.name || "Cadpole";
  setEnginePill(s0);
  engineSel.value = s0.enginePreference;
  setRemoteAvailable(s0.remoteAvailable);
  connPill.textContent = s0.connection;
  connPill.className = `pill pill-conn conn-${s0.connection}`;
  modeSel.value = s0.viewMode;
  setModeChips(s0.viewMode);
  setChecked(gridItem, s0.showGrid);
  setChecked(labelsItem, s0.showLabels);
  setChecked(bboxItem, s0.showBBox);
  setChecked(orthoItem, s0.orthographic);
  setChecked(hudItem, s0.showHud);
  setPlatesView(s0);
  rehome(s0.narrow);
  // Which way the bar can still scroll, for the fades in styles.css.
  const syncOverflow = (): void => {
    const rest = root.scrollWidth - root.clientWidth - root.scrollLeft;
    root.classList.toggle("more-left", root.scrollLeft > 4);
    root.classList.toggle("more-right", rest > 4);
  };
  root.addEventListener("scroll", syncOverflow, { passive: true });
  const overflowObserver = new ResizeObserver(syncOverflow);
  for (const el of [root, center, right]) overflowObserver.observe(el);

  return { bar: root, tools };
}
