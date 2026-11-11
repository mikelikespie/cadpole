// Minimal DOM helpers; no framework.

export type Child = Node | string | number | null | undefined | false;

type Props<K extends keyof HTMLElementTagNameMap> = Partial<
  Omit<HTMLElementTagNameMap[K], "style" | "children" | "dataset" | "classList">
> & {
  class?: string;
  style?: Partial<CSSStyleDeclaration>;
  dataset?: Record<string, string>;
  aria?: Record<string, string>;
  on?: { [E in keyof HTMLElementEventMap]?: (ev: HTMLElementEventMap[E]) => void };
};

export function h<K extends keyof HTMLElementTagNameMap>(
  tag: K,
  props: Props<K> | null = null,
  ...children: Child[]
): HTMLElementTagNameMap[K] {
  const el = document.createElement(tag);
  if (props) {
    const { class: cls, style, dataset, aria, on, ...rest } = props;
    if (cls) el.className = cls;
    if (style) Object.assign(el.style, style);
    if (dataset) for (const [k, v] of Object.entries(dataset)) el.dataset[k] = v;
    if (aria) for (const [k, v] of Object.entries(aria)) el.setAttribute(`aria-${k}`, v);
    if (on) {
      for (const [k, fn] of Object.entries(on)) {
        if (fn) el.addEventListener(k, fn as EventListener);
      }
    }
    Object.assign(el, rest);
  }
  append(el, ...children);
  return el;
}

export function append(el: Node, ...children: Child[]): void {
  for (const c of children) {
    if (c === null || c === undefined || c === false) continue;
    el.appendChild(typeof c === "string" || typeof c === "number" ? document.createTextNode(String(c)) : c);
  }
}

export function clear(el: Element): void {
  while (el.firstChild) el.removeChild(el.firstChild);
}

export function replaceChildren(el: Element, ...children: Child[]): void {
  clear(el);
  append(el, ...children);
}

/** A toolbar/icon button with an accessible name. */
export function button(
  label: string,
  opts: { title?: string; class?: string; pressed?: boolean; onClick?: (ev: MouseEvent) => void; icon?: string } = {},
): HTMLButtonElement {
  const b = h(
    "button",
    {
      type: "button",
      class: `btn ${opts.class ?? ""}`.trim(),
      title: opts.title ?? label,
      on: opts.onClick ? { click: opts.onClick } : undefined,
    },
    opts.icon ? h("span", { class: "btn-icon", aria: { hidden: "true" } }, opts.icon) : null,
    h("span", { class: opts.icon ? "btn-text" : "" }, label),
  );
  if (opts.pressed !== undefined) b.setAttribute("aria-pressed", String(opts.pressed));
  return b;
}
