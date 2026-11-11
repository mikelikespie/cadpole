// The "picker" popover: lists the entities under the cursor (face, edge,
// body, feature) so an ambiguous click can be disambiguated with one more
// click or the keyboard. Shown after a click on geometry, on right-click,
// and on long-press.

import { refKey } from "../selection.js";
import type { Candidate } from "../viewer.js";
import { h, replaceChildren } from "./dom.js";

const KIND_GLYPH: Record<string, string> = { face: "▱", edge: "╱", body: "▣", feature: "ƒ" };

export class PickerPopover {
  readonly el: HTMLElement;
  private items: HTMLButtonElement[] = [];
  private onDocPointer = (ev: PointerEvent): void => {
    if (!this.el.contains(ev.target as Node)) this.hide();
  };

  constructor(
    private readonly container: HTMLElement,
    private readonly onPick: (c: Candidate, additive: boolean) => void,
  ) {
    this.el = h("div", { class: "picker", role: "menu", aria: { label: "Select entity" } });
    this.el.hidden = true;
    this.el.addEventListener("keydown", (ev) => this.onKey(ev));
    container.appendChild(this.el);
  }

  get open(): boolean {
    return !this.el.hidden;
  }

  show(candidates: Candidate[], x: number, y: number, currentKey: string | null, opts: { focus?: boolean } = {}): void {
    if (candidates.length === 0) {
      this.hide();
      return;
    }
    this.items = candidates.map((c) => {
      const checked = currentKey !== null && refKey(c.ref) === currentKey;
      const b = h(
        "button",
        {
          type: "button",
          role: "menuitemradio",
          class: `picker-item ${checked ? "checked" : ""}`,
          tabIndex: -1,
          aria: { checked: String(checked) },
          on: {
            click: (ev) => {
              this.onPick(c, ev.shiftKey || ev.ctrlKey || ev.metaKey);
              this.hide();
            },
          },
        },
        h("span", { class: "picker-glyph", aria: { hidden: "true" } }, KIND_GLYPH[c.kind] ?? "•"),
        h("span", { class: "picker-kind" }, c.kind),
        h("span", { class: "picker-label" }, c.label),
        c.detail ? h("span", { class: "picker-detail muted" }, c.detail) : null,
      );
      return b;
    });
    replaceChildren(this.el, ...this.items);
    this.el.hidden = false;
    // Position inside the viewport, flipping when near the edges.
    const cw = this.container.clientWidth;
    const ch = this.container.clientHeight;
    const w = this.el.offsetWidth;
    const hgt = this.el.offsetHeight;
    let left = x + 14;
    let top = y + 14;
    if (left + w > cw - 8) left = Math.max(8, x - w - 14);
    if (top + hgt > ch - 8) top = Math.max(8, y - hgt - 14);
    this.el.style.left = `${left}px`;
    this.el.style.top = `${top}px`;
    document.addEventListener("pointerdown", this.onDocPointer, true);
    if (opts.focus) {
      const first = this.items.find((i) => i.classList.contains("checked")) ?? this.items[0];
      if (first) {
        first.tabIndex = 0;
        first.focus();
      }
    }
  }

  hide(): void {
    if (this.el.hidden) return;
    this.el.hidden = true;
    document.removeEventListener("pointerdown", this.onDocPointer, true);
    this.items = [];
  }

  private onKey(ev: KeyboardEvent): void {
    const i = this.items.findIndex((it) => it === document.activeElement);
    if (ev.key === "Escape") {
      ev.preventDefault();
      this.hide();
      return;
    }
    if (ev.key === "ArrowDown" || ev.key === "ArrowUp") {
      ev.preventDefault();
      const n = this.items.length;
      const next = ((i < 0 ? 0 : i + (ev.key === "ArrowDown" ? 1 : -1)) + n) % n;
      for (const it of this.items) it.tabIndex = -1;
      const t = this.items[next];
      if (t) {
        t.tabIndex = 0;
        t.focus();
      }
    }
  }

  /** Cycle helper: the candidate after `currentKey`, wrapping around. */
  static next(candidates: Candidate[], currentKey: string | null): Candidate | null {
    if (candidates.length === 0) return null;
    const i = currentKey === null ? -1 : candidates.findIndex((c) => refKey(c.ref) === currentKey);
    return candidates[(i + 1) % candidates.length] ?? null;
  }
}
