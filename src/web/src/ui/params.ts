// Parameters panel: controls bound to param_overrides with live re-evaluate.
// Rebuilds only when the parameter list -- a name, kind, range, step, unit,
// choice list or description -- (or the kind of pointer) changes; otherwise
// values are updated in place (never while the control has focus).
//
// What each kind of parameter becomes:
//
//   number    a slider (when it has a range) and a number field. The field
//             sends only values it holds and the range allows: an empty or
//             half-typed field sends nothing and is restored when the edit
//             ends, and an out-of-range entry is clamped then. On a fine
//             pointer they sit side by side. On a coarse pointer -- a phone --
//             the slider is full width with a 44 px hit area and the field is
//             the middle of a stepper, `−  value  +`, one `niceStep` a tap;
//             the field is the value, so tapping the value edits it. A length
//             under the document's `units(...)` also reads in that
//             convention, `= 12'-6"`: after the unit on a fine pointer, under
//             the name on a coarse one.
//   bool      a switch; the whole row is its label, so the whole row toggles.
//   choices   a segmented control (a radio group whose radios are the
//             segments; arrow keys move) when there are few, short choices,
//             else a native <select>, which on a phone is the phone's own
//             picker.
//   text      a text field (a string parameter without choices).
//   vec3      three decimal fields.
//
// Every row keeps its per-row reset, and an overridden row is marked. On a
// coarse pointer a hidden reset keeps its place (styles.css), so nothing
// under the thumb moves when an override appears.

import type { Actions } from "../actions.js";
import type { AppStore } from "../state.js";
import type { DisplayUnits, ParamInfo, ParamValue, Vec3 } from "../types.js";
import { fmt, fmtField, fmtParamLength, isLengthUnit, paramValuesEqual } from "../util.js";
import { h, replaceChildren } from "./dom.js";

interface Row {
  el: HTMLElement;
  update(p: ParamInfo, override: ParamValue | undefined): void;
}

/** A touch screen, or anything else without a fine pointer. */
export const COARSE_QUERY = "(pointer: coarse)";

/** Up to this many choices, each this short, are segments; more is a picker. */
const SEGMENT_MAX = 4;
const SEGMENT_LABEL_MAX = 12;
const SEGMENT_TOTAL_MAX = 32;

export function niceStep(p: ParamInfo): number {
  if (p.step && p.step > 0) return p.step;
  if (p.min !== null && p.max !== null && p.max > p.min) {
    const span = p.max - p.min;
    const raw = span / 100;
    const pow = Math.pow(10, Math.floor(Math.log10(raw)));
    return raw / pow >= 5 ? pow * 5 : raw / pow >= 2 ? pow * 2 : pow;
  }
  return 1;
}

/**
 * Whether a choice list is a segmented control or a picker.
 *
 * Segments have to share one line: four short words fit on a phone, a sixth
 * printer name does not, and a picker that scrolls beats segments that wrap.
 */
export function isSegmented(choices: readonly string[]): boolean {
  return (
    choices.length > 0 &&
    choices.length <= SEGMENT_MAX &&
    choices.every((c) => c.length <= SEGMENT_LABEL_MAX) &&
    choices.reduce((n, c) => n + c.length, 0) <= SEGMENT_TOTAL_MAX
  );
}

/**
 * The next value up or down from `v`: the neighbouring point of the step's
 * grid (anchored at `min`, or at 0), clamped to the range. A value between
 * two grid points goes to the nearest one in the direction pressed, so a
 * typed 60.7 steps to 61 and back to 60, never over a point.
 */
export function stepped(v: number, dir: 1 | -1, p: Pick<ParamInfo, "min" | "max">, step: number): number {
  const origin = p.min ?? 0;
  // Snap away float noise (60.000000001 is 60) before choosing a neighbour.
  const k = Math.round(((v - origin) / step) * 1e9) / 1e9;
  const next = origin + (dir > 0 ? Math.floor(k) + 1 : Math.ceil(k) - 1) * step;
  // Drop float noise (0.30000000000000004) without rounding to a fixed
  // number of decimals, which would flatten a step of 1e-7 to nothing.
  let out = Number(next.toPrecision(12));
  if (p.min !== null) out = Math.max(p.min, out);
  if (p.max !== null) out = Math.min(p.max, out);
  return out;
}

export function createParamsPanel(store: AppStore, actions: Actions): HTMLElement {
  const list = h("div", { class: "params" });
  const resetAll = h(
    "button",
    {
      type: "button",
      class: "btn btn-quiet",
      title: "Reset all parameters to the script defaults",
      on: { click: () => actions.resetAllOverrides() },
    },
    "Reset all",
  );
  const root = h(
    "section",
    { class: "panel-section params-section", aria: { label: "Parameters" } },
    h("div", { class: "section-head" }, h("h2", { class: "section-title" }, "Parameters"), resetAll),
    list,
  );
  let rows = new Map<string, Row>();
  let signature = "";
  // The pointer decides a number row's shape. It is part of the signature
  // below, so a tablet that gains a mouse (or loses one) rebuilds the rows.
  const coarse = typeof window !== "undefined" && "matchMedia" in window ? window.matchMedia(COARSE_QUERY) : null;
  coarse?.addEventListener("change", () => sync());
  // The document's `units(...)`, read by the length rows' readouts.
  let display: DisplayUnits | undefined;

  const description = (p: ParamInfo): HTMLElement | null => (p.description ? h("div", { class: "param-desc" }, p.description) : null);

  const numberRow = (p: ParamInfo): Row => {
    const id = `param-${p.name}`;
    const step = niceStep(p);
    const ranged = p.min !== null && p.max !== null && p.max > p.min;
    const num = h("input", {
      type: "number",
      id,
      class: "param-number",
      step: String(step),
      inputMode: "decimal",
      aria: { label: `${p.name}${p.unit ? ` (${p.unit})` : ""}` },
    });
    if (p.min !== null) num.min = String(p.min);
    if (p.max !== null) num.max = String(p.max);
    const range = ranged
      ? h("input", {
          type: "range",
          class: "param-range",
          min: String(p.min),
          max: String(p.max),
          step: String(step),
          aria: { label: `${p.name} slider` },
        })
      : null;
    // The one place a number is written into its field: exactly, because the
    // field is also read back (the stepper steps from it, an edit starts from
    // it), and a rounded display would become a rounded value.
    const show = (v: number): string => fmtField(v);
    let current = 0;
    const commit = (v: number, preview = false): void => {
      if (!Number.isFinite(v)) return;
      actions.setOverride(p.name, v, preview);
    };
    // The number the field holds, or null when it holds none: an empty field,
    // or a half-typed "-" or "1e", which a number input reports as "" with
    // `badInput`. `Number("")` is 0, and 0 is not what an empty field means.
    const typed = (): number | null => {
      if (num.value.trim() === "" || num.validity?.badInput) return null;
      const v = Number(num.value);
      return Number.isFinite(v) ? v : null;
    };
    const clamp = (v: number): number => Math.min(p.max ?? Infinity, Math.max(p.min ?? -Infinity, v));
    // While typing, only a value the parameter can take is sent: "5" on the
    // way to "50" under a min of 20 is a keystroke, not a request.
    num.addEventListener("input", () => {
      const v = typed();
      if (v !== null && clamp(v) === v) commit(v);
    });
    // When the edit ends (blur, Enter), an empty or unparsable field goes back
    // to the value, and one out of range is clamped into it and sent.
    num.addEventListener("change", () => {
      const v = typed();
      if (v === null) {
        num.value = show(current);
        return;
      }
      const c = clamp(v);
      if (c !== v) {
        num.value = show(c);
        commit(c);
      }
    });
    num.addEventListener("keydown", (ev) => {
      if (ev.key === "Enter") num.blur();
    });
    // While the slider moves every value is a preview: the engine
    // skips the fuses the drag changed and shows their operands. `change`
    // fires once, when the pointer is released, and that one is the full
    // evaluation.
    range?.addEventListener("input", () => commit(Number(range.value), true));
    range?.addEventListener("change", () => commit(Number(range.value)));
    const reset = resetButton(p.name);
    const unit = p.unit ? h("span", { class: "param-unit" }, p.unit) : null;
    // A length parameter under the document's `units(...)` also reads
    // in that convention — `45'-6"` beside a `45.5 ft` input. Hidden, and
    // nothing in the layout, for a document that declared none.
    const readout = h("span", { class: "param-unit param-readout" });
    readout.hidden = true;
    // The stepper: from what the field shows (a half-typed edit included),
    // falling back to the last value the model reported.
    const stepButton = (dir: 1 | -1): HTMLButtonElement =>
      h(
        "button",
        {
          type: "button",
          class: "stepper-btn",
          aria: { label: `${dir > 0 ? "Increase" : "Decrease"} ${p.name}` },
          on: {
            click: () => {
              const from = typed() ?? current;
              const next = stepped(from, dir, p, step);
              num.value = show(next);
              commit(next);
            },
          },
        },
        h("span", { aria: { hidden: "true" } }, dir > 0 ? "+" : "−"),
      );
    // On a coarse pointer the name is not the field's label: a tap on the
    // name would focus the field, and on a phone that raises the keyboard
    // over the very row that was being read. The field is named by aria.
    const el = coarse?.matches
      ? h(
          "div",
          { class: "param-row param-row-number coarse" },
          h(
            "div",
            { class: "param-head" },
            // The readout goes under the name, not beside it: the stepper
            // needs the line's width, and the name keeps what is left.
            h("span", { class: "param-title" }, h("span", { class: "param-name" }, p.name), readout),
            h(
              "div",
              { class: "stepper", role: "group", aria: { label: `${p.name} stepper` } },
              stepButton(-1),
              h("label", { class: "stepper-value" }, num, unit),
              stepButton(1),
            ),
            reset,
          ),
          range ? h("div", { class: "param-controls" }, range) : null,
          description(p),
        )
      : h(
          "div",
          { class: "param-row param-row-number" },
          h("div", { class: "param-head" }, h("label", { class: "param-name", htmlFor: id }, p.name), unit, readout, reset),
          h("div", { class: "param-controls" }, range, num),
          description(p),
        );
    return {
      el,
      update(info, override) {
        const v = typeof info.value === "number" ? info.value : 0;
        current = v;
        if (document.activeElement !== num) num.value = show(v);
        if (range && document.activeElement !== range) range.value = String(v);
        const shown = display !== undefined && isLengthUnit(info.unit);
        readout.hidden = !shown;
        if (shown) readout.textContent = `= ${fmtParamLength(v, info.unit, display)}`;
        el.classList.toggle("overridden", override !== undefined);
        reset.hidden = override === undefined;
      },
    };
  };

  const boolRow = (p: ParamInfo): Row => {
    const id = `param-${p.name}`;
    const cb = h("input", { type: "checkbox", id, class: "param-check param-switch", role: "switch" });
    cb.addEventListener("change", () => actions.setOverride(p.name, cb.checked));
    const reset = resetButton(p.name);
    // The label wraps the name, the description and the switch: a tap
    // anywhere on the row toggles it.
    const el = h(
      "div",
      { class: "param-row param-row-bool" },
      h(
        "div",
        { class: "param-head" },
        h(
          "label",
          { class: "param-switch-row", htmlFor: id },
          h("span", { class: "param-switch-text" }, h("span", { class: "param-name" }, p.name), p.description ? h("span", { class: "param-desc" }, p.description) : null),
          cb,
        ),
        reset,
      ),
    );
    return {
      el,
      update(info, override) {
        cb.checked = info.value === true;
        el.classList.toggle("overridden", override !== undefined);
        reset.hidden = override === undefined;
      },
    };
  };

  const choiceRow = (p: ParamInfo): Row => {
    const id = `param-${p.name}`;
    const choices = p.choices;
    const reset = resetButton(p.name);
    let control: HTMLElement;
    let radios: HTMLInputElement[] = [];
    let select: HTMLSelectElement | null = null;
    let name: HTMLElement;
    if (isSegmented(choices)) {
      // Native radios, visually hidden under their segment: the browser
      // supplies the group's keyboard (arrows move and select), the roles and
      // the form semantics, and a tap on the segment is a click on its label.
      radios = choices.map((c) => h("input", { type: "radio", class: "segment-input", name: `${id}-choice`, value: c }));
      for (const r of radios) {
        r.addEventListener("change", () => {
          if (r.checked) actions.setOverride(p.name, r.value);
        });
      }
      control = h(
        "div",
        { class: "segmented", role: "radiogroup", id, aria: { label: p.name } },
        ...choices.map((c, i) => h("label", { class: "segment" }, radios[i] ?? null, h("span", { class: "segment-label" }, c))),
      );
      name = h("span", { class: "param-name" }, p.name);
    } else {
      const sel = h(
        "select",
        { class: "select param-select", id, aria: { label: p.name } },
        ...choices.map((c) => h("option", { value: c }, c)),
      );
      sel.addEventListener("change", () => actions.setOverride(p.name, sel.value));
      select = sel;
      control = sel;
      name = h("label", { class: "param-name", htmlFor: id }, p.name);
    }
    const el = h(
      "div",
      { class: "param-row param-row-choice" },
      h("div", { class: "param-head" }, name, reset),
      h("div", { class: "param-controls" }, control),
      description(p),
    );
    return {
      el,
      update(info, override) {
        const v = typeof info.value === "string" ? info.value : "";
        for (const r of radios) r.checked = r.value === v;
        if (select && document.activeElement !== select) select.value = v;
        el.classList.toggle("overridden", override !== undefined);
        reset.hidden = override === undefined;
      },
    };
  };

  const textRow = (p: ParamInfo): Row => {
    const id = `param-${p.name}`;
    const input = h("input", { type: "text", id, class: "param-text" });
    input.addEventListener("change", () => actions.setOverride(p.name, input.value));
    const reset = resetButton(p.name);
    const el = h(
      "div",
      { class: "param-row" },
      h("div", { class: "param-head" }, h("label", { class: "param-name", htmlFor: id }, p.name), reset),
      h("div", { class: "param-controls" }, input),
      description(p),
    );
    return {
      el,
      update(info, override) {
        if (document.activeElement !== input) input.value = typeof info.value === "string" ? info.value : "";
        el.classList.toggle("overridden", override !== undefined);
        reset.hidden = override === undefined;
      },
    };
  };

  const vecRow = (p: ParamInfo): Row => {
    const inputs = (["x", "y", "z"] as const).map((axis) =>
      h("input", {
        type: "number",
        class: "param-number param-vec",
        step: "0.1",
        inputMode: "decimal",
        aria: { label: `${p.name} ${axis}` },
      }),
    );
    // The last value the model reported, to put back in a component left
    // empty or unparseable: Number("") is 0, which would commit 0 for that
    // axis as an override nobody typed.
    let current: Vec3 | null = null;
    const commit = (): void => {
      const blank = inputs.map((i) => i.value.trim() === "" || Boolean(i.validity?.badInput));
      if (blank.some(Boolean)) {
        if (current) {
          const was = [current.x, current.y, current.z];
          inputs.forEach((i, k) => {
            if (blank[k]) i.value = fmtField(was[k] as number);
          });
        }
        return;
      }
      const [x, y, z] = inputs.map((i) => Number(i.value)) as [number, number, number];
      if ([x, y, z].every(Number.isFinite)) actions.setOverride(p.name, { x, y, z });
    };
    for (const i of inputs) i.addEventListener("change", commit);
    const reset = resetButton(p.name);
    const el = h(
      "div",
      { class: "param-row" },
      h("div", { class: "param-head" }, h("span", { class: "param-name" }, p.name), reset),
      h("div", { class: "param-controls param-vec-controls" }, ...inputs),
      description(p),
    );
    return {
      el,
      update(info, override) {
        const v = info.value as Vec3;
        if (typeof v === "object") {
          current = v;
          inputs.forEach((i, k) => {
            if (document.activeElement !== i) i.value = fmtField([v.x, v.y, v.z][k] as number);
          });
        }
        el.classList.toggle("overridden", override !== undefined);
        reset.hidden = override === undefined;
      },
    };
  };

  function resetButton(name: string): HTMLButtonElement {
    return h(
      "button",
      {
        type: "button",
        class: "icon-btn reset",
        title: "Reset to default",
        aria: { label: `Reset ${name} to its default` },
        on: { click: () => actions.resetOverride(name) },
      },
      h("span", { aria: { hidden: "true" } }, "↺"),
    );
  }

  function build(params: ParamInfo[]): void {
    rows = new Map();
    if (params.length === 0) {
      replaceChildren(list, h("div", { class: "empty" }, "No param(...) calls in the script."));
      return;
    }
    const els: HTMLElement[] = [];
    for (const p of params) {
      const row =
        typeof p.default_value === "boolean"
          ? boolRow(p)
          : typeof p.default_value === "string"
            ? p.choices.length > 0
              ? choiceRow(p)
              : textRow(p)
            : typeof p.default_value === "object"
              ? vecRow(p)
              : numberRow(p);
      rows.set(p.name, row);
      els.push(row.el);
    }
    replaceChildren(list, ...els);
  }

  function sync(): void {
    const s = store.get();
    const params = s.model?.params ?? [];
    display = s.model?.display_units;
    const sig =
      (coarse?.matches ? "coarse|" : "fine|") +
      // Everything a row is built from and `update` does not refresh. JSON,
      // so a description containing ":" or "|" cannot alias another list.
      JSON.stringify(params.map((p) => [p.name, typeof p.default_value, p.min, p.max, p.step, p.unit, p.choices, p.description]));
    if (sig !== signature) {
      signature = sig;
      build(params);
    }
    let any = false;
    for (const p of params) {
      const ov = s.overrides[p.name];
      const eff: ParamInfo = ov !== undefined && !paramValuesEqual(ov, p.value) ? { ...p, value: ov } : p;
      rows.get(p.name)?.update(eff, ov);
      any = any || ov !== undefined;
    }
    resetAll.hidden = !any;
  }

  store.subscribe((s, prev) => {
    if (s.model !== prev.model || s.overrides !== prev.overrides) sync();
  });
  sync();
  return root;
}

/**
 * A published page's parameters: the values the bake was made
 * with, read-only -- a name and a value a row, no slider, no field, no
 * reset, because nothing on a published page can evaluate a new one. The
 * bake carries only the parameters the publisher listed
 * (`cadpole publish --show-params`), none by default, and with none the panel
 * is not shown at all (`html[data-params="0"]`).
 */
export function createParamReadout(store: AppStore): HTMLElement {
  const list = h("dl", { class: "param-readout" });
  const root = h(
    "section",
    { class: "panel-section params-section params-readout", aria: { label: "Parameters, as published" } },
    h("div", { class: "section-head" }, h("h2", { class: "section-title" }, "Parameters"), h("span", { class: "muted section-note" }, "as published")),
    list,
  );
  const value = (p: ParamInfo, display: DisplayUnits | undefined): string => {
    const v = p.value;
    if (typeof v === "number") return fmtParamLength(v, p.unit, display);
    if (typeof v === "boolean") return v ? "yes" : "no";
    if (typeof v === "string") return v;
    return `${fmt(v.x)}, ${fmt(v.y)}, ${fmt(v.z)}`;
  };
  const sync = (): void => {
    const model = store.get().model;
    const params = model?.params ?? [];
    document.documentElement.dataset["params"] = String(params.length);
    replaceChildren(
      list,
      ...params.flatMap((p) => [h("dt", { class: "param-name" }, p.name), h("dd", { class: "param-value" }, value(p, model?.display_units))]),
    );
  };
  store.subscribe((s, prev) => {
    if (s.model !== prev.model) sync();
  });
  sync();
  return root;
}
