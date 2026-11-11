// Escaping and the page's Starlark colouring.

/** Text for an attribute or an element. */
export const E = (s) => String(s).replace(/&/g, "&amp;").replace(/</g, "&lt;").replace(/>/g, "&gt;").replace(/"/g, "&quot;").replace(/'/g, "&#x27;");
/** Text for an element only: quotes stay as they are. */
export const T = (s) => String(s).replace(/&/g, "&amp;").replace(/</g, "&lt;").replace(/>/g, "&gt;");

const KW = new Set(["def", "return", "for", "in", "if", "else", "elif", "and", "or", "not", "load", "True", "False", "None", "lambda", "pass"]);
const TOK = /(#[^\n]*)|("(?:[^"\\\n]|\\.)*")|(\b\d+(?:\.\d+)?\b)|([A-Za-z_][A-Za-z0-9_]*)(?=(\()?)/g;

/** A script as HTML: comments, strings, numbers, and keywords and calls marked. */
export function hl(src) {
  let out = "", at = 0;
  for (const m of src.matchAll(TOK)) {
    out += T(src.slice(at, m.index));
    at = m.index + m[0].length;
    const [t, c, s, n, w, call] = m;
    if (c) out += `<span class="c">${T(t)}</span>`;
    else if (s) out += `<span class="s">${T(t)}</span>`;
    else if (n) out += `<span class="n">${t}</span>`;
    else if (KW.has(w) || call !== undefined) out += `<span class="k">${t}</span>`;
    else out += t;
  }
  return out + T(src.slice(at));
}

/** The same, one element per line, for a numbered listing. */
export const hlLines = (src) => hl(src).split("\n").map((l) => `<span class="ln">${l}\n</span>`).join("");
