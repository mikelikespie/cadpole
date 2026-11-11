// The Parameters panel itself (createParamsPanel), on a small fake DOM: what
// a number field commits as it is edited, what it shows, and when the rows are
// rebuilt. Only what the panel touches is faked; the panel code is the real
// transpiled module.

import assert from "node:assert/strict";
import { test } from "node:test";
import { createParamsPanel } from "../src/ui/params.js";
import { fmt } from "../src/util.js";

class FakeEl {
  constructor(tag) {
    this.tagName = tag.toUpperCase();
    this.children = [];
    this.listeners = {};
    this.attrs = {};
    this.style = {};
    this.dataset = {};
    this.className = "";
    this.value = "";
    this.hidden = false;
    this.validity = { badInput: false };
    const classes = new Set();
    this.classList = {
      toggle: (c, on) => (on ?? !classes.has(c) ? classes.add(c) : classes.delete(c), classes.has(c)),
      contains: (c) => classes.has(c),
    };
  }
  addEventListener(k, f) {
    (this.listeners[k] ??= []).push(f);
  }
  dispatch(k) {
    for (const f of this.listeners[k] ?? []) f({ key: "", target: this });
  }
  setAttribute(k, v) {
    this.attrs[k] = String(v);
  }
  appendChild(c) {
    this.children.push(c);
    return c;
  }
  removeChild(c) {
    this.children.splice(this.children.indexOf(c), 1);
    return c;
  }
  blur() {}
  get firstChild() {
    return this.children[0] ?? null;
  }
  set textContent(s) {
    this.children = [{ text: String(s) }];
  }
  get textContent() {
    return this.children.map((c) => (c instanceof FakeEl ? c.textContent : c.text)).join("");
  }
  findAll(pred, out = []) {
    if (pred(this)) out.push(this);
    for (const c of this.children) if (c instanceof FakeEl) c.findAll(pred, out);
    return out;
  }
}

/** A panel over one model; returns the controls and what it committed. */
function panel(t, params, { coarse = false } = {}) {
  const saved = { document: globalThis.document, window: globalThis.window };
  globalThis.document = { createElement: (tag) => new FakeEl(tag), createTextNode: (s) => ({ text: String(s) }), activeElement: null };
  globalThis.window = { matchMedia: () => ({ matches: coarse, addEventListener() {} }) };
  t.after(() => Object.assign(globalThis, saved));
  let state = { model: { params }, overrides: {} };
  const subs = [];
  const store = {
    get: () => state,
    subscribe: (f) => subs.push(f),
  };
  const commits = [];
  const actions = {
    setOverride: (name, value, preview = false) => commits.push({ name, value, preview }),
    resetOverride() {},
    resetAllOverrides() {},
  };
  const root = createParamsPanel(store, actions);
  return {
    root,
    commits,
    field: () => root.findAll((e) => e.tagName === "INPUT" && e.className === "param-number")[0],
    button: (label) => root.findAll((e) => e.tagName === "BUTTON" && e.attrs["aria-label"] === label)[0],
    descriptions: () => root.findAll((e) => e.className === "param-desc").map((e) => e.textContent),
    setModel(next) {
      const prev = state;
      state = { ...state, model: next };
      for (const f of subs) f(state, prev);
    },
  };
}

const num = (name, value, extra = {}) => ({
  name,
  value,
  default_value: value,
  min: null,
  max: null,
  step: null,
  unit: "",
  choices: [],
  description: "",
  ...extra,
});

test("an emptied or half-typed number field commits nothing", (t) => {
  const p = panel(t, [num("width", 60, { min: 20, max: 200 })]);
  const f = p.field();
  assert.equal(f.value, "60");
  f.value = "";
  f.dispatch("input");
  f.value = "";
  f.validity.badInput = true; // what a number input reports for "-"
  f.dispatch("input");
  f.validity.badInput = false;
  assert.deepEqual(p.commits, [], "an empty field is not the value 0");
  f.value = "75";
  f.dispatch("input");
  assert.deepEqual(p.commits, [{ name: "width", value: 75, preview: false }]);
});

test("a number outside the declared range is not committed while typing, and is clamped when the edit ends", (t) => {
  const p = panel(t, [num("width", 60, { min: 20, max: 200 })]);
  const f = p.field();
  f.value = "5"; // on the way to "50"
  f.dispatch("input");
  assert.deepEqual(p.commits, [], "below min is not sent mid-edit");
  f.dispatch("change");
  assert.deepEqual(p.commits, [{ name: "width", value: 20, preview: false }], "the finished edit is clamped to min");
  assert.equal(f.value, "20");
  f.value = "";
  f.dispatch("change");
  assert.equal(f.value, "60", "leaving the field empty puts the value back");
  assert.equal(p.commits.length, 1);
});

test("an emptied vector component commits nothing and gets its value back", (t) => {
  const p = panel(t, [num("at", { x: 1, y: 2, z: 3 })]);
  const [x, y, z] = p.root.findAll((e) => e.tagName === "INPUT" && e.className === "param-number param-vec");
  assert.deepEqual([x.value, y.value, z.value], ["1", "2", "3"]);
  y.value = "";
  y.dispatch("change");
  z.value = "";
  z.validity.badInput = true; // a lone "-"
  z.dispatch("change");
  z.validity.badInput = false;
  assert.deepEqual(p.commits, [], "an empty component is not the value 0");
  assert.equal(y.value, "2", "the emptied field shows the value again");
  assert.equal(z.value, "3");
  x.value = "4";
  x.dispatch("change");
  assert.deepEqual(p.commits, [{ name: "at", value: { x: 4, y: 2, z: 3 }, preview: false }]);
});

test("a number field shows the value it holds, and the stepper steps from it", (t) => {
  const p = panel(t, [num("n", 1234567), num("m", 0.00001234, { step: 1.234e-7 })], { coarse: true });
  const [big, small] = p.root.findAll((e) => e.tagName === "INPUT" && e.className === "param-number");
  assert.equal(Number(big.value), 1234567, `field shows ${big.value}`);
  assert.equal(Number(small.value), 0.00001234, `field shows ${small.value}`);
  p.button("Increase n").dispatch("click");
  p.button("Increase m").dispatch("click");
  assert.equal(p.commits[0].value, 1234568);
  assert.ok(Math.abs(p.commits[1].value - 0.0000124634) < 1e-15, `stepped to ${p.commits[1].value}`);
});

test("fmt strips trailing zeros only after a decimal point", () => {
  assert.equal(fmt(100, 0), "100");
  assert.equal(fmt(1500, 0), "1500");
  assert.equal(fmt(12.5, 3), "12.5");
  assert.equal(fmt(12, 3), "12");
});

test("a description-only change to a parameter reaches the panel", (t) => {
  const p = panel(t, [num("width", 60, { min: 20, max: 200, description: "OLD description" })]);
  assert.deepEqual(p.descriptions(), ["OLD description"]);
  p.setModel({ params: [num("width", 60, { min: 20, max: 200, description: "NEW description" })] });
  assert.deepEqual(p.descriptions(), ["NEW description"]);
});
