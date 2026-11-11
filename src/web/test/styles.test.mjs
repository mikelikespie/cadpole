// styles.css parses as the flat list of rules it is written as.
//
// An unclosed `{` does not fail anything on its own. Chromium parses everything
// after it as CSS Nesting *inside* the rule that was never closed, so the rest
// of the file silently applies only to descendants of that rule's selector, and
// the page looks merely a little off. So the braces are checked here, outside
// comments and strings, and a rule opened inside a style rule (rather than
// inside an at-rule such as `@media`) is a failure too: this stylesheet does
// not use nesting, and the one way it acquires some is by accident.

import assert from "node:assert/strict";
import { readFileSync } from "node:fs";
import { test } from "node:test";

const STYLESHEET = new URL("../styles.css", import.meta.url);

/**
 * Every `{` and `}` of a stylesheet, with what opened each block.
 *
 * Returns the blocks left open at the end, the closers that had nothing to
 * close, and the blocks opened inside a style rule. A block's `prelude` is
 * the text between the previous `;`, `{` or `}` and its own `{`: a selector,
 * or `@media (...)`.
 */
export function scanBlocks(css) {
  const open = [];
  const stray = [];
  const nested = [];
  let line = 1;
  let prelude = "";
  for (let i = 0; i < css.length; i++) {
    const c = css[i];
    if (c === "\n") line++;
    if (c === "/" && css[i + 1] === "*") {
      const end = css.indexOf("*/", i + 2);
      const stop = end === -1 ? css.length : end + 2;
      for (let j = i; j < stop; j++) if (css[j] === "\n") line++;
      i = stop - 1;
      continue;
    }
    if (c === '"' || c === "'") {
      let j = i + 1;
      while (j < css.length && css[j] !== c) {
        if (css[j] === "\\") j++;
        else if (css[j] === "\n") line++;
        j++;
      }
      prelude += css.slice(i, j + 1);
      i = j;
      continue;
    }
    if (c === "{") {
      const parent = open[open.length - 1];
      const block = { line, prelude: prelude.trim() };
      if (parent && !parent.prelude.startsWith("@")) nested.push({ ...block, inside: parent });
      open.push(block);
      prelude = "";
    } else if (c === "}") {
      if (open.length === 0) stray.push(line);
      else open.pop();
      prelude = "";
    } else if (c === ";") {
      prelude = "";
    } else {
      prelude += c;
    }
  }
  return { unclosed: open, stray, nested };
}

const describe = (b) => `line ${b.line}: \`${b.prelude.replace(/\s+/g, " ").slice(0, 60)}\``;

test("styles.css closes every rule it opens", () => {
  const css = readFileSync(STYLESHEET, "utf8");
  const { unclosed, stray } = scanBlocks(css);
  assert.deepEqual(
    unclosed.map(describe),
    [],
    "a rule was never closed; everything after it is parsed as nested inside it and does not apply",
  );
  assert.deepEqual(stray, [], "a `}` with nothing to close");
});

test("styles.css nests nothing inside a style rule", () => {
  const css = readFileSync(STYLESHEET, "utf8");
  const { nested } = scanBlocks(css);
  assert.deepEqual(
    nested.map((b) => `${describe(b)} inside ${describe(b.inside)}`),
    [],
    "a block opened inside a style rule only applies under that rule's selector",
  );
});

test("the scanner sees through comments and strings", () => {
  const ok = scanBlocks('a::before { content: "{"; } /* { */ @media (x) { b { c: "}" } }');
  assert.deepEqual(ok, { unclosed: [], stray: [], nested: [] });
  const bad = scanBlocks("a {\n color: red;\n/* -- */\nb { x: y }\n");
  assert.equal(bad.unclosed.length, 1);
  assert.equal(bad.unclosed[0].line, 1);
  assert.equal(bad.nested.length, 1);
  assert.equal(bad.nested[0].line, 4);
});
