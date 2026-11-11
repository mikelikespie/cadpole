// Screen-reader announcements via an aria-live region, and a roving-tabindex
// helper for list/tree keyboard navigation.

import { h } from "./dom.js";

export class Announcer {
  readonly polite: HTMLElement;
  readonly assertive: HTMLElement;
  private timer: number | null = null;

  constructor(parent: HTMLElement) {
    this.polite = h("div", { class: "sr-only", aria: { live: "polite", atomic: "true" } });
    this.assertive = h("div", { class: "sr-only", aria: { live: "assertive", atomic: "true" } });
    parent.append(this.polite, this.assertive);
  }

  /** Coalesces rapid announcements (e.g. hover) into the latest one. */
  say(text: string, opts: { assertive?: boolean; delay?: number } = {}): void {
    const target = opts.assertive ? this.assertive : this.polite;
    if (this.timer !== null) window.clearTimeout(this.timer);
    this.timer = window.setTimeout(() => {
      this.timer = null;
      // Clearing first makes repeated identical messages re-announce.
      target.textContent = "";
      window.setTimeout(() => {
        target.textContent = text;
      }, 20);
    }, opts.delay ?? 150);
  }
}

/**
 * Roving tabindex over `items` (in DOM order). Arrow keys move focus,
 * Home/End jump, Enter/Space activate via the element's click handler.
 */
export function rovingKeydown(ev: KeyboardEvent, items: HTMLElement[], current: HTMLElement): boolean {
  const i = items.indexOf(current);
  if (i < 0 || items.length === 0) return false;
  let next = -1;
  switch (ev.key) {
    case "ArrowDown":
      next = Math.min(items.length - 1, i + 1);
      break;
    case "ArrowUp":
      next = Math.max(0, i - 1);
      break;
    case "Home":
      next = 0;
      break;
    case "End":
      next = items.length - 1;
      break;
    case "Enter":
    case " ":
      ev.preventDefault();
      current.click();
      return true;
    default:
      return false;
  }
  ev.preventDefault();
  for (const it of items) it.tabIndex = -1;
  const target = items[next] as HTMLElement;
  target.tabIndex = 0;
  target.focus();
  return true;
}

export function setRovingTabindex(items: HTMLElement[], active: HTMLElement | null): void {
  let found = false;
  for (const it of items) {
    const on = it === active;
    it.tabIndex = on ? 0 : -1;
    found = found || on;
  }
  if (!found && items[0]) items[0].tabIndex = 0;
}
