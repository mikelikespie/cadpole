// API reference rendered from engine.api_doc().

import type { AppStore } from "../state.js";
import type { ApiDoc, ApiFunction, ApiParam } from "../types.js";
import { h, replaceChildren } from "./dom.js";

export function signature(f: ApiFunction): string {
  if (f.constant) return `${f.name}${f.returns ? `: ${f.returns}` : ""}`;
  const ps = f.params.map((p) => (p.required ? p.name : `${p.name}=${p.default_value || "None"}`));
  return `${f.on_type ? `${f.on_type}.` : ""}${f.name}(${ps.join(", ")})${f.returns ? ` -> ${f.returns}` : ""}`;
}

/** The rows of a parameters table: `/` and `*` are separators in the signature, not parameters. */
export function tableParams(params: ApiParam[]): ApiParam[] {
  return params.filter((p) => p.name !== "/" && p.name !== "*");
}

function paramsTable(all: ApiParam[]): HTMLElement | null {
  const params = tableParams(all);
  if (params.length === 0) return null;
  return h(
    "table",
    { class: "args api-params" },
    ...params.map((p) =>
      h(
        "tr",
        null,
        h("th", null, p.name, p.required ? h("span", { class: "req", title: "required" }, "*") : null),
        h("td", { class: "mono muted" }, p.type),
        h("td", null, p.doc),
      ),
    ),
  );
}

function fnBlock(f: ApiFunction): HTMLElement {
  return h(
    "article",
    { class: "api-fn" },
    h("h4", { class: "mono" }, signature(f)),
    f.doc ? h("p", null, f.doc) : null,
    paramsTable(f.params),
  );
}

export function createApiDocPanel(store: AppStore): HTMLElement {
  const filter = h("input", { type: "search", class: "search", placeholder: "Filter…", aria: { label: "Filter API" } });
  const body = h("div", { class: "api-body" });
  const root = h(
    "section",
    { class: "panel-section api-section", aria: { label: "API reference" } },
    h("div", { class: "section-head" }, h("h2", { class: "section-title" }, "API"), filter),
    body,
  );

  function render(doc: ApiDoc | null): void {
    if (!doc) {
      replaceChildren(body, h("div", { class: "empty" }, "API documentation is not available."));
      return;
    }
    const q = filter.value.trim().toLowerCase();
    const match = (s: string): boolean => !q || s.toLowerCase().includes(q);
    const fns = doc.functions.filter((f) => match(f.name) || match(f.doc));
    const types = doc.types
      .map((t) => ({
        ...t,
        methods: t.methods.filter((m) => match(t.name) || match(m.name) || match(m.doc)),
        attrs: t.attrs.filter((a) => match(t.name) || match(a.name) || match(a.doc)),
      }))
      .filter((t) => match(t.name) || match(t.doc) || t.methods.length || t.attrs.length);
    replaceChildren(
      body,
      fns.length ? h("h3", { class: "api-group" }, "Functions") : null,
      ...fns.map(fnBlock),
      types.length ? h("h3", { class: "api-group" }, "Types") : null,
      ...types.map((t) =>
        h(
          "article",
          { class: "api-type" },
          h("h4", { class: "mono" }, t.name),
          t.doc ? h("p", null, t.doc) : null,
          t.attrs.length ? h("h5", null, "Attributes") : null,
          paramsTable(t.attrs),
          t.methods.length ? h("h5", null, "Methods") : null,
          ...t.methods.map(fnBlock),
        ),
      ),
      !fns.length && !types.length ? h("div", { class: "empty" }, "No matches.") : null,
    );
  }

  filter.addEventListener("input", () => render(store.get().apiDoc));
  store.select((s) => s.apiDoc, render);
  return root;
}
