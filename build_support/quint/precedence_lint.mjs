// Flags Quint assignments whose right-hand side is an unbracketed `and`/`or`.
//
// In Quint `and` and `or` bind more loosely than `=`, so
//
//     x' = a and b
//
// parses as `(x' = a) and b`: x gets `a`, and `b` becomes a guard on the whole
// step. Written by mistake, that silently disables every step where `b` is
// false, and no invariant notices (a model that never moves violates nothing).
// Bracket the value, `x' = (a and b)`, or the conjunction, `(x' = a) and b`.
//
// Exempt: a conjunction of assignments, `x' = 1 and y' = 2` (the operand after
// `and`/`or` is itself a primed assignment), and `.and(...)` / `.or(...)`.
//
// runner.mjs runs it on every model before `quint typecheck`.

const TOKEN =
  /(\s+)|("(?:[^"\\]|\\.)*")|([A-Za-z_][A-Za-z0-9_]*'?)|(\d[\d_]*)|(==|!=|<=|>=|=>|->|::|\.\.|[(){}[\],.=<>+\-*/%^&|!:;#@?])|([\s\S])/g;
const KINDS = ["ws", "str", "ident", "num", "op", "other"];
const OPEN = "([{";
const CLOSE = ")]}";
// Declarations that start something new: an assignment never runs past one.
const DECL = new Set([
  "val", "def", "action", "pure", "temporal", "run", "var", "const",
  "module", "import", "export", "type", "assume",
]);

/** Blanks out // and /* *\/ comments, keeping line numbers and strings. */
export function stripComments(src) {
  let out = "";
  let i = 0;
  const n = src.length;
  while (i < n) {
    if (src.startsWith("//", i)) {
      let j = src.indexOf("\n", i);
      if (j < 0) j = n;
      out += " ".repeat(j - i);
      i = j;
    } else if (src.startsWith("/*", i)) {
      let j = src.indexOf("*/", i + 2);
      j = j < 0 ? n : j + 2;
      out += src.slice(i, j).replace(/[^\n]/g, " ");
      i = j;
    } else if (src[i] === '"') {
      const m = /"(?:[^"\\]|\\.)*"/y;
      m.lastIndex = i;
      const r = m.exec(src);
      const j = r ? i + r[0].length : n;
      out += src.slice(i, j);
      i = j;
    } else {
      out += src[i];
      i += 1;
    }
  }
  return out;
}

function tokens(src) {
  const toks = [];
  let line = 1;
  for (const m of src.matchAll(TOKEN)) {
    const g = m.slice(1).findIndex((x) => x !== undefined);
    const kind = KINDS[g];
    const text = m[0];
    if (kind !== "ws") toks.push({ kind, text, line });
    line += (text.match(/\n/g) || []).length;
  }
  return toks;
}

function isAssignment(toks, k) {
  return (
    k + 1 < toks.length &&
    toks[k].kind === "ident" &&
    toks[k].text.endsWith("'") &&
    toks[k + 1].text === "="
  );
}

/** Findings (strings) for one model's source; `name` labels them. */
export function lint(name, source) {
  const toks = tokens(stripComments(source));
  const findings = [];
  for (let k = 0; k < toks.length; k++) {
    if (!isAssignment(toks, k)) continue;
    let depth = 0;
    for (let j = k + 2; j < toks.length; j++) {
      const { kind, text, line } = toks[j];
      if (OPEN.includes(text)) depth += 1;
      else if (CLOSE.includes(text)) {
        depth -= 1;
        if (depth < 0) break;
      } else if (depth === 0 && text === ",") break;
      else if (depth === 0 && kind === "ident" && DECL.has(text)) break;
      else if (depth === 0 && (text === "and" || text === "or") && toks[j - 1].text !== ".") {
        if (!isAssignment(toks, j + 1)) {
          const x = toks[k].text;
          findings.push(
            `${name}:${toks[k].line}: \`${x} = ... ${text} ...\` parses as ` +
              `\`(${x} = ...) ${text} ...\`; bracket the value or the conjunction ` +
              `(line ${line} has the \`${text}\`)`,
          );
        }
        break;
      }
    }
  }
  return findings;
}
