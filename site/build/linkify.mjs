// Links in the page's text: the repository's address and the paths of files in
// it (docs/parts.md, examples/bracket.star, ./bin/install, README.md) become
// links to GitHub, wherever they are written: prose, notes, and the commands
// and requests a visitor copies. A copy button copies its box's textContent,
// which a link inside the box does not change.
//
// It works on the finished page, so new content gets its links without asking.
// Only text between tags is read; the head, attributes, existing links, buttons,
// the scripts' listings (pre), and script, style and textarea elements are left
// as they are.
import { existsSync, statSync } from "node:fs";
import { join } from "node:path";

/** The repository's address, as it is written in text: with or without the scheme, with or without .git. */
const REPO_TEXT = /(?:https?:\/\/)?github\.com\/mikelikespie\/cadpole(?:\.git)?(?![\w/-])/;
/** A path from the repository's root: a top-level directory and a file or directory in it, or a top-level file. */
const PATH_TEXT = /(?<![\w./~-])(?:\.\/)?(?:(?:docs|examples|bin|src|spec|proto|stdlib|site|assets|build_support)\/[\w.-]+(?:\/[\w.-]+)*\/?|README\.md|LICENSE|CLAUDE\.md|THIRD-PARTY-NOTICES\.md)(?![\w/-])/;
const TOKEN = new RegExp(`${REPO_TEXT.source}|${PATH_TEXT.source}`, "g");
/** Elements copied as they are, contents and all: their text is code, or not text. */
const RAW = /<(head|script|style|textarea|pre)\b[\s\S]*?<\/\1>/gi;
/** Elements whose text is a label or a link already: nothing in them is linked. */
const SKIP = new Set(["a", "button", "summary"]);

/**
 * The page with its repository mentions linked.
 * `repo` is the repository's address; `root` the directory that holds the
 * repository's files, to check a path names a file that is there. A path that
 * is not there stops the build: a link to it would be a dead one.
 */
export function linkify(html, { repo, root }) {
  const missing = new Set();
  const link = (text) => {
    // A sentence's full stop is not part of the path.
    const trail = /\.+$/.exec(text)?.[0] ?? "";
    const t = trail ? text.slice(0, -trail.length) : text;
    if (/github\.com\//.test(t)) return `<a class="ref" href="${repo}">${t}</a>${trail}`;
    const path = t.replace(/^\.\//, "").replace(/\/$/, "");
    const p = join(root, path);
    if (!existsSync(p)) { missing.add(path); return text; }
    const kind = statSync(p).isDirectory() ? "tree" : "blob";
    return `<a class="ref" href="${repo}/${kind}/main/${path}">${t}</a>${trail}`;
  };
  let skip = 0;
  const prose = (seg) => seg.replace(/(<!--[\s\S]*?-->|<[^>]*>)|([^<]+)/g, (_, tag, text) => {
    if (!tag) return skip ? text : text.replace(TOKEN, link);
    const t = /^<(\/?)([a-zA-Z][\w-]*)/.exec(tag);
    if (t && SKIP.has(t[2].toLowerCase())) skip = Math.max(0, skip + (t[1] ? -1 : 1));
    return tag;
  });
  let out = "", at = 0;
  for (const m of html.matchAll(RAW)) { out += prose(html.slice(at, m.index)) + m[0]; at = m.index + m[0].length; }
  out += prose(html.slice(at));
  if (missing.size) throw new Error(`the page names repository paths that are not in it (fix the text, or add the file to the site's inputs in BUILD.bazel): ${[...missing].join(", ")}`);
  return out;
}
