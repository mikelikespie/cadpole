// Script viewer: read-only <pre> with line numbers, the selected feature's
// source span highlighted, diagnostic markers, and reveal-on-demand. In
// static mode an Edit toggle swaps in a textarea (Ctrl/Cmd+Enter applies).

import type { Actions } from "../actions.js";
import type { AppStore } from "../state.js";
import type { Diagnostic, ScriptFile, SourceSpan } from "../types.js";
import { h, replaceChildren } from "./dom.js";

export function createScriptPanel(store: AppStore, actions: Actions): HTMLElement {
  const files = h("div", { class: "file-tabs", role: "tablist", aria: { label: "Script files" } });
  const pre = h("pre", { class: "script", tabIndex: 0, aria: { label: "Script source" } });
  const editor = h("textarea", { class: "script-editor", spellcheck: false, aria: { label: "Script editor" } });
  editor.hidden = true;
  const editBtn = h("button", { type: "button", class: "btn btn-quiet", on: { click: () => setEditing(!editing) } }, "Edit");
  const applyBtn = h("button", { type: "button", class: "btn btn-primary", on: { click: () => apply() } }, "Apply");
  const cancelBtn = h("button", { type: "button", class: "btn btn-quiet", on: { click: () => setEditing(false) } }, "Cancel");
  const tools = h("div", { class: "script-tools" }, editBtn, applyBtn, cancelBtn);
  const root = h(
    "section",
    { class: "panel-section script-section", aria: { label: "Script" } },
    h("div", { class: "section-head" }, h("h2", { class: "section-title" }, "Script"), tools),
    files,
    pre,
    editor,
  );

  let editing = false;
  let lineEls: HTMLElement[] = [];
  let renderedSource = "";
  let renderedPath = "";

  function currentFile(): ScriptFile | null {
    const s = store.get();
    const scripts = s.session?.document.scripts ?? [];
    return scripts.find((f) => f.path === s.activeScript) ?? scripts[0] ?? null;
  }

  function setEditing(on: boolean): void {
    editing = on;
    const file = currentFile();
    editor.hidden = !on;
    pre.hidden = on;
    applyBtn.hidden = !on;
    cancelBtn.hidden = !on;
    editBtn.hidden = on;
    if (on) {
      editor.value = file?.source ?? "";
      editor.focus();
    }
  }
  editor.addEventListener("keydown", (ev) => {
    if ((ev.ctrlKey || ev.metaKey) && ev.key === "Enter") {
      ev.preventDefault();
      apply();
    } else if (ev.key === "Escape") {
      setEditing(false);
    } else if (ev.key === "Tab") {
      ev.preventDefault();
      const s = editor.selectionStart;
      editor.setRangeText("    ", s, editor.selectionEnd, "end");
    }
  });

  async function apply(): Promise<void> {
    const file = currentFile();
    if (!file) return;
    await actions.applyScript(file.path, editor.value);
    setEditing(false);
  }

  function renderFiles(): void {
    const s = store.get();
    const scripts = s.session?.document.scripts ?? [];
    files.hidden = scripts.length <= 1;
    replaceChildren(
      files,
      ...scripts.map((f) =>
        h(
          "button",
          {
            type: "button",
            role: "tab",
            class: `file-tab ${f.path === (currentFile()?.path ?? "") ? "active" : ""}`,
            aria: { selected: String(f.path === (currentFile()?.path ?? "")) },
            on: { click: () => store.set({ activeScript: f.path }) },
          },
          f.path,
        ),
      ),
    );
  }

  function renderSource(): void {
    const file = currentFile();
    const source = file?.source ?? "";
    const path = file?.path ?? "";
    if (source === renderedSource && path === renderedPath) return;
    renderedSource = source;
    renderedPath = path;
    const lines = source.split("\n");
    lineEls = lines.map((text, i) =>
      h(
        "div",
        { class: "line", dataset: { line: String(i + 1) } },
        h("span", { class: "ln", aria: { hidden: "true" } }, String(i + 1)),
        h("span", { class: "code" }, text.length ? text : "​"),
      ),
    );
    replaceChildren(pre, ...lineEls);
    applyMarks();
    applyHighlight();
  }

  function applyMarks(): void {
    const s = store.get();
    const path = currentFile()?.path ?? "";
    for (const el of lineEls) {
      el.classList.remove("has-error", "has-warning");
      el.removeAttribute("title");
    }
    const diags: Diagnostic[] = s.model?.diagnostics ?? [];
    for (const d of diags) {
      if (!d.span || (d.span.path && d.span.path !== path)) continue;
      const el = lineEls[d.span.start_line - 1];
      if (!el) continue;
      el.classList.add(d.severity === "error" ? "has-error" : "has-warning");
      el.title = d.message;
    }
  }

  let highlighted: SourceSpan | null = null;
  function applyHighlight(): void {
    const s = store.get();
    const path = currentFile()?.path ?? "";
    for (const el of lineEls) el.classList.remove("hl");
    highlighted = null;
    const sel = s.selected[s.selected.length - 1];
    if (!sel || !s.model) return;
    let span: SourceSpan | null = null;
    if (sel.kind === "param") span = s.model.params.find((p) => p.name === sel.topo_name)?.span ?? null;
    else span = s.model.features.find((f) => f.id === sel.feature_id)?.span ?? null;
    if (!span || (span.path && span.path !== path)) return;
    highlighted = span;
    for (let l = span.start_line; l <= span.end_line; l++) lineEls[l - 1]?.classList.add("hl");
  }

  function reveal(line: number): void {
    const el = lineEls[line - 1];
    if (!el) return;
    el.scrollIntoView({ block: "center", behavior: window.matchMedia("(prefers-reduced-motion: reduce)").matches ? "auto" : "smooth" });
    el.classList.add("flash");
    window.setTimeout(() => el.classList.remove("flash"), 900);
  }

  store.subscribe((s, prev) => {
    if (s.session?.document.scripts !== prev.session?.document.scripts || s.activeScript !== prev.activeScript) {
      renderFiles();
      renderSource();
      if (editing && s.session?.document.scripts !== prev.session?.document.scripts) setEditing(false);
    }
    if (s.model !== prev.model) applyMarks();
    if (s.selected !== prev.selected || s.model !== prev.model) {
      applyHighlight();
      if (highlighted && s.selected !== prev.selected && s.leftTab === "script") reveal(highlighted.start_line);
    }
    if (s.revealLine !== prev.revealLine && s.revealLine) {
      if (s.revealLine.path && s.revealLine.path !== (currentFile()?.path ?? "")) {
        store.set({ activeScript: s.revealLine.path });
        renderFiles();
        renderSource();
      }
      // Wait for layout (tab/sheet switch) before scrolling.
      const line = s.revealLine.line;
      requestAnimationFrame(() => reveal(line));
    }
    if (s.apiMode !== prev.apiMode) tools.hidden = s.apiMode !== "static";
  });
  tools.hidden = store.get().apiMode !== "static";
  setEditing(false);
  renderFiles();
  renderSource();
  return root;
}
