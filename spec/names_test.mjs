// Diffs what the Quint models transcribe from the code against the code, so a
// change on either side fails the build until the other follows.
//
//   capability.qnt  OPS, GRANTED, SPELLINGS   vs ops.proto and capability.rs
//   capability.qnt  ENGINE_REQUESTS           vs engine.proto's Request oneof
//   capability.qnt  CONNECT_PREFIX            vs ops.proto's package and service
//   lifecycle.qnt   HTTP_BOUNDS_TODAY         vs `post_op`'s `heavy` match
//   lifecycle.qnt   FFI_BOUNDS_TODAY          vs `deadline_of`'s arms
//   eventlog.qnt    the event-log cap         vs `App::emit_by`
//   revisions.qnt   how each handler reads `base_revision`
//   wakeups.qnt     what is worth a model turn, the client ids, the toggle's
//                   `was`                     vs main.rs, mcp.rs, listener.mjs, server.rs
//   listener.qnt    the version floor, `/new`, the default conversation
//                   length                    vs driver.mjs, listener.mjs, main.mjs and the docs
//   transport.qnt   AGENT_ROUTES, OPS, GRANTED vs `agent_routes` in server.rs, capability.rs
//   listener.qnt, wakeups.qnt, session_progress.qnt, transport.qnt
//                   every invariant and mutant is listed in BUILD.bazel, every
//                   action is known to the model's replay, and every invariant
//                   the code names exists
//
// The eventlog.qnt and revisions.qnt checks also pin server behaviour those
// models describe, including behaviour the models show to be lossy. When the
// server changes, the check fails and the model is updated with it.
//
// The last section is not about a model: it diffs the entity-kind names in
// document.proto against the copies in normalize.ts and RemoteEngine.swift.

import { readFileSync } from "node:fs";
import { isAbsolute, join } from "node:path";

const runfiles = process.env["JS_BINARY__RUNFILES"] ?? process.env["RUNFILES_DIR"] ?? "";

const problems = [];
function check(ok, message) {
  if (!ok) problems.push(message);
}

function read(envVar) {
  const p = process.env[envVar];
  if (!p) {
    console.error(`names_test: ${envVar} is not set`);
    process.exit(1);
  }
  return readFileSync(isAbsolute(p) ? p : join(runfiles, p), "utf8");
}

const proto = read("OPS_PROTO");
const enginePr = read("ENGINE_PROTO");
const rust = read("CAPABILITY_RS");
const server = read("SERVER_RS");
const ffi = read("FFI_RS");
const model = read("CAPABILITY_QNT");
const lifecycle = read("LIFECYCLE_QNT");
const eventlog = read("EVENTLOG_QNT");
const revisions = read("REVISIONS_QNT");
const proto0 = read("DOCUMENT_PROTO");
const wakeups = read("WAKEUPS_QNT");
const listenerModel = read("LISTENER_QNT");
const progress = read("SESSION_PROGRESS_QNT");
const specBuild = read("SPEC_BUILD");
const mainRs = read("MAIN_RS");
const mcpRs = read("MCP_RS");
const listenerMjs = read("LISTENER_MJS");
const listenerMain = read("LISTENER_MAIN_MJS");
const driverMjs = read("DRIVER_MJS");
const listenerReplay = read("LISTENER_REPLAY_MJS");
const wakeupsReplay = read("WAKEUPS_REPLAY_MJS");
const assistantDoc = read("ASSISTANT_DOC");
const normalizeTs = read("NORMALIZE_TS");
const remoteSwift = read("REMOTE_ENGINE_SWIFT");
const transport = read("TRANSPORT_QNT");
const transportReplay = read("TRANSPORT_REPLAY_MJS");

// ---------------------------------------------------------------- the proto

/** rpc name -> { cli, aliases[] } for every rpc of `service CadpoleOps`. */
const rpcs = new Map();
for (const m of proto.matchAll(/rpc\s+(\w+)\s*\([^)]*\)\s*returns\s*\([^)]*\)\s*\{([\s\S]*?)\n {2}\}/g)) {
  const [, name, body] = m;
  const cli = /cli_name:\s*"([^"]+)"/.exec(body);
  const aliases = [...body.matchAll(/alias:\s*"([^"]+)"/g)].map((a) => a[1]);
  rpcs.set(name, { cli: cli ? cli[1] : null, aliases });
}
check(rpcs.size > 0, "no rpcs found in ops.proto; the parser needs updating");

// ----------------------------------------------------------------- the Rust

/** The first string of each `(` ... `)` tuple in a `const NAME: &[(&str, &str)]` list. */
function rustList(name) {
  const start = rust.indexOf(`pub const ${name}: &[(&str, &str)] = &[`);
  check(start >= 0, `${name} not found in capability.rs`);
  if (start < 0) return [];
  const end = rust.indexOf("\n];", start);
  const body = rust.slice(start, end);
  return [...body.matchAll(/\(\s*\n?\s*"([A-Za-z]\w*)"/g)].map((m) => m[1]);
}

const granted = new Set(rustList("GRANTED"));
const denied = new Set(rustList("DENIED"));

// ---------------------------------------------------------------- the model

/** The contents of `pure val NAME: Set[str] = Set( ... )`. */
function modelSet(name, src = model, where = "capability.qnt") {
  // Either a multi-line `Set(\n … \n  )` or a one-liner `Set("a", "b")`.
  const multi = new RegExp(`pure val ${name}: Set\\[str\\] = Set\\(([\\s\\S]*?)\\n {2}\\)`);
  const single = new RegExp(`pure val ${name}: Set\\[str\\] = Set\\(([^\\n]*)\\)`);
  // `single` first: a one-line Set has no `\n  )` for `multi` to stop at, so
  // `multi` would run on to the next definition that does.
  const m = single.exec(src) ?? multi.exec(src);
  check(m !== null, `${name} not found in ${where}`);
  if (!m) return new Set();
  return new Set([...m[1].matchAll(/"([^"]*)"/g)].map((x) => x[1]));
}

const modelOps = modelSet("OPS");
const modelGranted = modelSet("GRANTED");

/** The `"spelling" -> "Rpc"` pairs of `pure val SPELLINGS`. */
const spellings = new Map();
{
  const m = /pure val SPELLINGS: str -> str = Map\(([\s\S]*?)\n {2}\)/.exec(model);
  check(m !== null, "SPELLINGS not found in capability.qnt");
  if (m) {
    for (const s of m[1].matchAll(/"([^"]*)"\s*->\s*"([^"]+)"/g)) spellings.set(s[1], s[2]);
  }
}

// ------------------------------------------------------------------- checks

const diff = (a, b) => [...a].filter((x) => !b.has(x)).sort();

check(
  diff(new Set(rpcs.keys()), modelOps).length === 0,
  `capability.qnt OPS is missing rpcs from ops.proto: ${diff(new Set(rpcs.keys()), modelOps).join(", ")}`,
);
check(
  diff(modelOps, new Set(rpcs.keys())).length === 0,
  `capability.qnt OPS names operations ops.proto does not have: ${diff(modelOps, new Set(rpcs.keys())).join(", ")}`,
);

check(
  diff(granted, modelGranted).length === 0,
  `capability.rs grants operations the model does not: ${diff(granted, modelGranted).join(", ")}`,
);
check(
  diff(modelGranted, granted).length === 0,
  `the model grants operations capability.rs does not: ${diff(modelGranted, granted).join(", ")}`,
);

// The model derives DENIED as OPS minus GRANTED; the Rust lists it.
const modelDenied = new Set([...modelOps].filter((o) => !modelGranted.has(o)));
check(
  diff(denied, modelDenied).length === 0 && diff(modelDenied, denied).length === 0,
  `capability.rs DENIED and the model's OPS-minus-GRANTED differ: ` +
    `rust-only [${diff(denied, modelDenied).join(", ")}], model-only [${diff(modelDenied, denied).join(", ")}]`,
);

// Every spelling the op route accepts is in the model, mapped to its rpc.
for (const [name, { cli, aliases }] of rpcs) {
  for (const spelling of [name, name.toLowerCase(), ...(cli ? [cli] : []), ...aliases]) {
    check(
      spellings.has(spelling),
      `capability.qnt SPELLINGS does not know ${JSON.stringify(spelling)} (a spelling of ${name})`,
    );
    if (spellings.has(spelling)) {
      check(
        spellings.get(spelling) === name,
        `capability.qnt maps ${JSON.stringify(spelling)} to ${spellings.get(spelling)}, but it resolves to ${name}`,
      );
    }
  }
}

// ...and nothing in the model resolves to an operation that does not exist.
for (const [spelling, op] of spellings) {
  check(rpcs.has(op), `capability.qnt maps ${JSON.stringify(spelling)} to ${op}, which is not an rpc of CadpoleOps`);
}

// ---------------------------------------------------- the engine envelope

// `engine.proto`'s `Request` oneof: what `POST /api/call`, the C ABI and the
// wasm worker accept. It is not the same set as the operation registry.
{
  const m = /oneof request \{([\s\S]*?)\n {2}\}/.exec(enginePr);
  check(m !== null, "the `oneof request` of engine.proto was not found; the parser needs updating");
  const variants = new Set(m ? [...m[1].matchAll(/^\s*(\w+)Request\s+\w+\s*=/gm)].map((v) => v[1]) : []);
  const modelEngine = modelSet("ENGINE_REQUESTS");
  check(
    diff(variants, modelEngine).length === 0 && diff(modelEngine, variants).length === 0,
    `capability.qnt ENGINE_REQUESTS and engine.proto's Request oneof differ: ` +
      `proto-only [${diff(variants, modelEngine).join(", ")}], model-only [${diff(modelEngine, variants).join(", ")}]`,
  );
}

// ------------------------------------------- what each front door bounds

// `post_op`'s `heavy` match names the operations that go through
// `render_job`: the wall-clock budget, the semaphore and the queue bound.
{
  const m = /let heavy = matches!\(name\.as_str\(\),([^)]*)\)/.exec(server);
  check(m !== null, "`let heavy = matches!(name.as_str(), ...)` was not found in server.rs");
  const gated = new Set(m ? [...m[1].matchAll(/"(\w+)"/g)].map((x) => x[1]) : []);
  const modelGated = modelSet("HTTP_BOUNDS_TODAY", lifecycle, "lifecycle.qnt");
  check(
    diff(gated, modelGated).length === 0 && diff(modelGated, gated).length === 0,
    `lifecycle.qnt HTTP_BOUNDS_TODAY and post_op's \`heavy\` match differ: ` +
      `code-only [${diff(gated, modelGated).join(", ")}], model-only [${diff(modelGated, gated).join(", ")}]`,
  );
}

// `deadline_of` in cadpole-ffi: the request variants whose `deadline_ms` arms
// the waiter.
{
  const start = ffi.indexOf("fn deadline_of(");
  check(start >= 0, "`fn deadline_of` was not found in cadpole-ffi/src/lib.rs");
  const body = start < 0 ? "" : ffi.slice(start, ffi.indexOf("\n}", start));
  const armed = new Set([...body.matchAll(/pb::request::Request::(\w+)\(/g)].map((x) => x[1]));
  const modelArmed = modelSet("FFI_BOUNDS_TODAY", lifecycle, "lifecycle.qnt");
  check(
    diff(armed, modelArmed).length === 0 && diff(modelArmed, armed).length === 0,
    `lifecycle.qnt FFI_BOUNDS_TODAY and deadline_of's arms differ: ` +
      `code-only [${diff(armed, modelArmed).join(", ")}], model-only [${diff(modelArmed, armed).join(", ")}]`,
  );
}

// --------------------------------------- server behaviour the models describe
//
// Each pattern is checked inside the function that it is about, not anywhere
// in server.rs, so one handler cannot satisfy a check meant for another.

/** The body of one function in server.rs, by the head of its signature. */
function fnBody(src, header, where) {
  const start = src.indexOf(header);
  check(start >= 0, `${where}: \`${header}\` was not found in server.rs; this test needs updating`);
  if (start < 0) return "";
  let depth = 0;
  for (let j = src.indexOf("{", start); j < src.length; j++) {
    if (src[j] === "{") depth++;
    else if (src[j] === "}" && --depth === 0) {
      const body = src.slice(start, j + 1);
      // A brace inside a string literal would make the slice overrun.
      check(
        !/\n {0,4}(?:pub )?(?:async )?fn \w+\(/.test(body.slice(header.length)),
        `${where}: the extracted body of \`${header}\` runs into the next function; the brace scan needs updating`,
      );
      return body;
    }
  }
  return src.slice(start);
}

// eventlog.qnt: the log is capped and drops its oldest entries.
{
  const inCode = /st\.events\.len\(\) > (\d[\d_]*)/.exec(server)?.[1];
  check(
    inCode !== undefined,
    "App::emit_by no longer caps the event log; eventlog.qnt's whole premise (a window, not a prefix) needs revisiting",
  );
  // The model's LOG_CAP is small so eviction is reachable; its comment names
  // the real cap, and that is what is compared.
  const inModel = /`(\d[\d_]*)` in `App::emit_by`/.exec(eventlog)?.[1];
  check(
    inModel !== undefined,
    "eventlog.qnt no longer names the real event-log cap as `N` in `App::emit_by`; that comment is what this test diffs",
  );
  const bare = (n) => n.replace(/_/g, "");
  if (inCode !== undefined && inModel !== undefined) {
    check(
      bare(inCode) === bare(inModel),
      `App::emit_by caps the event log at ${inCode}, but eventlog.qnt's comment names ${inModel}; ` +
        "bring the model's comment (and MAX_SEQ, if the window's shape changed) forward with the code",
    );
  }
}
check(
  /st\.events\.drain\(\.\.st\.events\.len\(\) - /.test(server),
  "the event log is no longer drained from the front; eventlog.qnt's `theWindowIsDense` assumed a prefix drop",
);

// revisions.qnt: how each handler reads `base_revision`.
const handlers = {
  set_script: fnBody(server, "fn set_script(&self", "revisions.qnt"),
  edit_script: fnBody(server, "fn edit_script(&self", "revisions.qnt"),
  add_asset: fnBody(server, "fn add_asset(&self", "revisions.qnt"),
  put_script: fnBody(server, "async fn put_script(", "revisions.qnt"),
};
const readings = [
  ["set_script", "compares the number", /req\.base_revision != st\.revision/],
  ["edit_script", "compares the number it read under the lock", /req\.base_revision != revision/],
  ["add_asset", "treats zero as 'no check'", /req\.base_revision != 0 && req\.base_revision != st\.revision/],
  ["put_script", "compares the number like set_script", /if body\.base_revision != st\.revision/],
];
for (const [fn, what, re] of readings) {
  check(
    re.test(handlers[fn]),
    `revisions.qnt models how each route reads base_revision, and ${fn}'s reading has changed (${what}); ` +
      "update the model with it",
  );
}
// Only add_asset exempts zero. Checked for absence too, because the exempting
// form contains the comparing form.
for (const fn of ["set_script", "edit_script", "put_script"]) {
  check(
    !/base_revision != 0/.test(handlers[fn]),
    `${fn} now treats base_revision 0 as "no check", which is add_asset's reading; ` +
      "revisions.qnt models that reading on the asset route only",
  );
}
check(
  modelSet("ROUTES", revisions, "revisions.qnt").size === 3,
  "revisions.qnt ROUTES no longer has one entry per route that reads base_revision " +
    "(op: set_script and edit_script; json: put_script; asset: add_asset)",
);

// A write's response carries the revision committed under the lock, not a
// second read of the state.
for (const fn of ["set_script", "edit_script"]) {
  check(
    /\n {12}revision: rev,/.test(handlers[fn]),
    `${fn}'s response no longer carries \`rev\`, the revision committed under the lock; that is ` +
      "revisions.qnt's `theEchoedRevisionIsSomebodyElsesWork`, where a writer is handed somebody " +
      "else's revision and its next base_revision ratifies the lost update",
  );
  check(
    !/revision: (?:self\.app\.state|\{)/.test(handlers[fn]),
    `${fn} reads the state again to fill its response revision; see above`,
  );
}
check(
  !/revision: self\.app\.state\.lock\(\)\.unwrap\(\)\.revision/.test(server),
  "a write response re-reads the revision after dropping the lock again; that is " +
    "revisions.qnt's `theEchoedRevisionIsSomebodyElsesWork`, where a writer is handed " +
    "somebody else's revision and its next base_revision ratifies the lost update",
);

// eventlog.qnt: the SSE tail drops a lag notice.
check(
  /BroadcastStream::new\(rx\)\.filter_map\(move \|r\| r\.ok\(\)/.test(server),
  "the SSE tail no longer drops `BroadcastStream`'s Lagged with `.ok()`; " +
    "update eventlog.qnt's `theSseTailSwallowsALag` and the comments that describe it",
);

// The Connect route is `/api/` plus the service's full name.
{
  const pkg = /^package\s+([\w.]+);/m.exec(proto)?.[1];
  const service = /^service\s+(\w+)\s*\{/m.exec(proto)?.[1];
  check(pkg !== undefined && service !== undefined, "ops.proto has no package or no service; the parser needs updating");
  if (pkg && service) {
    const expected = `/api/${pkg}.${service}`;
    const found = /pure val CONNECT_PREFIX: str = "([^"]*)"/.exec(model)?.[1];
    check(
      found === expected,
      `capability.qnt CONNECT_PREFIX is ${JSON.stringify(found)} but ops.proto makes the Connect route ${JSON.stringify(expected)}`,
    );
  }
}

// ------------------------------------------------- entity kinds, three copies
//
// `cadpole.v1.EntityKind` is the schema; normalize.ts and RemoteEngine.swift
// each keep a table of its names. A kind missing from a table reads as a
// different valid kind, so every enum value must appear in all of them.
{
  const wanted = [...proto0.matchAll(/^\s*ENTITY_KIND_(\w+) = \d+;/gm)]
    .map((m) => m[1].toLowerCase())
    .filter((n) => n !== "unspecified");

  const tsList = /export function entityKind[\s\S]*?\[([\s\S]*?)\] as const/.exec(normalizeTs);
  check(tsList !== null, "normalize.ts's entityKind numeric table was not found; the parser needs updating");
  const tsNumeric = new Set(tsList ? [...tsList[1].matchAll(/"(\w+)"/g)].map((m) => m[1]) : []);
  const tsCases = new Set([...normalizeTs.matchAll(/^\s*case "(\w+)":$/gm)].map((m) => m[1]));
  const swiftNames = new Set([...remoteSwift.matchAll(/case \.\w+: return "(\w+)"/g)].map((m) => m[1]));
  const swiftKinds = new Set([...remoteSwift.matchAll(/case "(\w+)": return \.\w+/g)].map((m) => m[1]));

  for (const [what, have] of [
    ["normalize.ts's numeric table", tsNumeric],
    ["normalize.ts's string cases", tsCases],
    ["RemoteEngine.swift's EntityKindNames.name", swiftNames],
    ["RemoteEngine.swift's EntityKindNames.kind", swiftKinds],
  ]) {
    const missing = wanted.filter((n) => !have.has(n));
    check(
      missing.length === 0,
      `cadpole.v1.EntityKind has ${missing.join(", ")} and ${what} does not, so those refs silently read as another kind`,
    );
  }
}

// ------------------------------------------- wakeups.qnt: the three readers
//
// What is worth a model turn is decided in three places, one per reader, and
// the model has one definition (`actionable`). The same for the client ids
// the cursors are stored under and the field that says what the Listen
// toggle was.
{
  const str = (name, src, where) => {
    const m = new RegExp(`pure val ${name}: str = "([^"]*)"`).exec(src);
    check(m !== null, `${name} not found in ${where}`);
    return m ? m[1] : "";
  };
  const sameSet = (a, b) => a.size === b.size && [...a].every((x) => b.has(x));
  const actionableKinds = modelSet("ACTIONABLE_KINDS", wakeups, "wakeups.qnt");
  const pollKinds = modelSet("WATCHER_POLL_KINDS", wakeups, "wakeups.qnt");
  const wasField = str("TOGGLE_WAS_FIELD", wakeups, "wakeups.qnt");

  // `cadpole listen`: ListenFilter::Wake in `prints` and in `server_kinds`.
  const prints = /ListenFilter::Wake => match kind \{([\s\S]*?)\n {12}\},/.exec(mainRs)?.[1] ?? "";
  check(prints !== "", "main.rs: ListenFilter::Wake's arm of `prints` was not found; the parser needs updating");
  const printsKinds = new Set([...prints.matchAll(/^\s*"(\w+)" =>/gm)].map((m) => m[1]));
  check(
    sameSet(printsKinds, actionableKinds),
    `main.rs: \`cadpole listen\` prints ${[...printsKinds].join(", ")} by default; wakeups.qnt ACTIONABLE_KINDS is ${[...actionableKinds].join(", ")}`,
  );
  check(
    /"message" => data\["from"\]\.as_str\(\) == Some\("user"\)/.test(prints) &&
      /"selection_changed" => data\["note"\]\.as_str\(\)\.is_some_and\(\|n\| !n\.is_empty\(\)\)/.test(prints),
    "main.rs: `cadpole listen` no longer prints exactly a message from the user and a selection with a note; " +
      "wakeups.qnt's `Request` and `Note` are those two",
  );
  const wakeKinds = /ListenFilter::Wake => vec!\[([^\]]*)\]/.exec(mainRs)?.[1] ?? "";
  const gatedKind = /if gated && !kinds\.iter\(\)\.any\(\|k\| k == "(\w+)"\)/.exec(mainRs)?.[1];
  const serverKinds = new Set([...wakeKinds.matchAll(/"(\w+)"/g)].map((m) => m[1]).concat(gatedKind ? [gatedKind] : []));
  check(
    sameSet(serverKinds, pollKinds),
    `main.rs: \`cadpole listen\` filters its long poll by ${[...serverKinds].join(", ")}; wakeups.qnt WATCHER_POLL_KINDS is ${[...pollKinds].join(", ")}`,
  );

  // The channel: `wakes` in mcp.rs.
  const wakes = /\nfn wakes\(e: &Value\) -> bool \{([\s\S]*?)\n\}/.exec(mcpRs)?.[1] ?? "";
  check(wakes !== "", "mcp.rs: `fn wakes` was not found; the parser needs updating");
  const wakesKinds = new Set([...wakes.matchAll(/Some\("(\w+)"\) =>/g)].map((m) => m[1]));
  check(
    sameSet(wakesKinds, actionableKinds),
    `mcp.rs: the channel pushes ${[...wakesKinds].join(", ")}; wakeups.qnt ACTIONABLE_KINDS is ${[...actionableKinds].join(", ")}`,
  );
  check(
    /Some\("message"\) => e\["data"\]\["from"\]\.as_str\(\) == Some\("user"\)/.test(wakes) &&
      /Some\("selection_changed"\) => e\["data"\]\["note"\]\.as_str\(\)\.is_some_and\(\|n\| !n\.is_empty\(\)\)/.test(wakes),
    "mcp.rs: the channel no longer pushes exactly a message from the user and a selection with a note; " +
      "wakeups.qnt's `Request` and `Note` are those two",
  );
  check(
    /if !\(\*listening \|\| always\) \|\| !wakes\(e\) \{\s*continue;/.test(mcpRs),
    "mcp.rs: `pushes` no longer skips what `wakes` rejects; wakeups.qnt's `onlyActionableEventsAreHandedOver` is about that line",
  );

  // The listener: `isTurnWorthy` in listener.mjs.
  const worthy = /export function isTurnWorthy\(ev\) \{([\s\S]*?)\n\}/.exec(listenerMjs)?.[1] ?? "";
  check(worthy !== "", "listener.mjs: `isTurnWorthy` was not found; the parser needs updating");
  const worthyKinds = new Set([...worthy.matchAll(/ev\?\.kind === "(\w+)"/g)].map((m) => m[1]));
  check(
    sameSet(worthyKinds, actionableKinds),
    `listener.mjs: the listener acts on ${[...worthyKinds].join(", ")}; wakeups.qnt ACTIONABLE_KINDS is ${[...actionableKinds].join(", ")}`,
  );
  check(
    /if \(ev\?\.kind === "message"\) return ev\?\.data\?\.from === "user";/.test(worthy) &&
      /if \(ev\?\.kind === "selection_changed"\) return Boolean\(ev\?\.data\?\.note\);/.test(worthy) &&
      /return false;\s*$/.test(worthy),
    "listener.mjs: `isTurnWorthy` is no longer exactly a message from the user and a selection with a note",
  );

  // The client ids the cursors are stored under.
  const listenDefault = /env = "CADPOLE_LISTEN_CLIENT", default_value = "([^"]*)"/.exec(mainRs)?.[1];
  check(
    listenDefault === str("WATCHER_CLIENT", wakeups, "wakeups.qnt"),
    `main.rs: \`cadpole listen --client\` defaults to ${JSON.stringify(listenDefault)}; wakeups.qnt WATCHER_CLIENT differs`,
  );
  const listenerDefault = /const clientId = flag\("--client"\) \?\? process\.env\.CADPOLE_LISTENER_CLIENT \?\? "([^"]*)";/.exec(listenerMain)?.[1];
  check(
    listenerDefault === str("LISTENER_CLIENT", wakeups, "wakeups.qnt"),
    `main.mjs: the listener's client id defaults to ${JSON.stringify(listenerDefault)}; wakeups.qnt LISTENER_CLIENT differs`,
  );
  const channelId = /let client = format!\("\{\}:\{\}", if args\.listener \{ "listener" \} else \{ "(\w+)" \}, std::process::id\(\)\);/.exec(mcpRs)?.[1];
  check(
    channelId !== undefined && `${channelId}:` === str("CHANNEL_CLIENT_PREFIX", wakeups, "wakeups.qnt"),
    "mcp.rs: the channel's client id is no longer a fixed prefix and the process id; wakeups.qnt models every start of " +
      "`cadpole mcp` as a new client (`aRestartedChannelStartsAtTheEnd`), which holds only while the id is per process",
  );

  // The toggle's `was`: written by the server, read by all three.
  check(
    new RegExp(`"assistant_listening", serde_json::json!\\(\\{"listening": l, "${wasField}": was\\}\\)`).test(server),
    `server.rs: the assistant_listening event no longer carries "${wasField}"; wakeups.qnt's readers judge a backlog by it`,
  );
  for (const [where, src, re] of [
    ["main.rs (`cadpole listen`)", mainRs, new RegExp(`data\\["${wasField}"\\]\\.as_bool\\(\\)`)],
    ["mcp.rs (the channel)", mcpRs, new RegExp(`e\\["data"\\]\\["${wasField}"\\]\\.as_bool\\(\\)`)],
    ["listener.mjs", listenerMjs, new RegExp(`change\\?\\.data\\?\\.${wasField} === "boolean"`)],
  ]) {
    check(re.test(src), `${where} no longer reads "${wasField}" from the first assistant_listening event of a batch; see wakeups.qnt's \`judge\``);
  }
}

// ------------------------------------------------- listener.qnt: the listener
{
  const str = (name) => new RegExp(`pure val ${name}: str = "([^"]*)"`).exec(listenerModel)?.[1];
  const floor = /export const MIN_CLAUDE_VERSION = "([^"]*)";/.exec(driverMjs)?.[1];
  check(floor !== undefined && floor === str("MIN_CLAUDE_VERSION"), `driver.mjs MIN_CLAUDE_VERSION is ${floor}; listener.qnt says ${str("MIN_CLAUDE_VERSION")}`);
  const documented = [...assistantDoc.matchAll(/(?:Claude Code|older than) (\d+\.\d+\.\d+)/g)].map((m) => m[1]);
  check(documented.length > 0, "docs/assistant-integration.md names no minimum Claude Code version; the parser needs updating");
  check(
    documented.every((v) => v === floor),
    `docs/assistant-integration.md says Claude Code ${[...new Set(documented)].join(" and ")}; driver.mjs requires ${floor}`,
  );
  const fresh = /export const NEW_CONVERSATION = "([^"]*)";/.exec(listenerMjs)?.[1];
  check(fresh !== undefined && fresh === str("NEW_CONVERSATION_TEXT"), `listener.mjs NEW_CONVERSATION is ${JSON.stringify(fresh)}; listener.qnt says ${JSON.stringify(str("NEW_CONVERSATION_TEXT"))}`);
  const every = /process\.env\.CADPOLE_LISTENER_NEW_AFTER \?\? (\d+)\)/.exec(listenerMain)?.[1];
  const modelEvery = /pure val DEFAULT_NEW_CONVERSATION_AFTER: int = (\d+)/.exec(listenerModel)?.[1];
  check(every !== undefined && every === modelEvery, `main.mjs --new-conversation-after defaults to ${every}; listener.qnt says ${modelEvery}`);
  check(
    new RegExp(`--new-conversation-after N\\\` \\| start a fresh conversation after N messages \\(default ${every};`).test(assistantDoc),
    `docs/assistant-integration.md no longer says --new-conversation-after defaults to ${every}`,
  );
}

// ---------------------------------------------------------------- transport.qnt
//
// AGENT_ROUTES is every route `agent_routes` in server.rs registers: the
// socket's whole surface, so a route added there is a route the model has to
// hear about. The Connect route's pattern comes from the service descriptor
// at run time, so it is matched by its call, not its text.
{
  const body = fnBody(server, "fn agent_routes() -> Router<Shared>", "transport.qnt");
  const routes = new Set();
  for (const m of body.matchAll(/\.route\(\s*("([^"]+)"|&connect::route_pattern\(\))\s*,\s*(get|post|axum::routing::put)\(/g)) {
    const method = m[3].replace("axum::routing::", "").toUpperCase();
    const path = m[2] ?? "/api/cadpole.v1.CadpoleOps/{method}";
    routes.add(`${method} ${path}`);
  }
  check(routes.size > 0, "no routes found in `agent_routes`; this test needs updating");
  const modelRoutes = modelSet("AGENT_ROUTES", transport, "transport.qnt");
  check(
    diff(routes, modelRoutes).length === 0 && diff(modelRoutes, routes).length === 0,
    `transport.qnt AGENT_ROUTES ${JSON.stringify([...modelRoutes].sort())} != agent_routes in server.rs ${JSON.stringify([...routes].sort())}`,
  );
  // Its operations stand for the two classes: each must be classified the
  // way capability.rs classifies it.
  const ops = modelSet("OPS", transport, "transport.qnt");
  const g = modelSet("GRANTED", transport, "transport.qnt");
  for (const op of ops) {
    check(granted.has(op) || denied.has(op), `transport.qnt names ${op}, which capability.rs does not classify`);
    check(g.has(op) === granted.has(op), `transport.qnt says ${op} is ${g.has(op) ? "granted" : "denied"}; capability.rs disagrees`);
  }
  check([...ops].some((o) => g.has(o)) && [...ops].some((o) => !g.has(o)), "transport.qnt needs one granted and one denied operation");
}

// ------------------- the models that are replayed: lists that must not drift
//
// An invariant that is not listed in BUILD.bazel is never simulated, a
// mutant that is not listed is never run, and an action the replay does not
// know stops a replay only when a trace happens to take it.
{
  /** The entries of `"<file>": [ ... ]` or `"<file>": { ... }` in spec/BUILD.bazel, in order of appearance. */
  const buildBlocks = (file) =>
    [...specBuild.matchAll(new RegExp(`"${file.replace(".", "\\.")}": ([\\[{])([\\s\\S]*?)\\n {8}[\\]}],`, "g"))].map((m) => ({
      open: m[1],
      body: m[2],
    }));
  // The last entry is how deep the replay's `default:` of its action switch
  // is indented, which is where the parser stops reading cases.
  for (const [file, src, replay, replayName, indent = 10] of [
    ["listener.qnt", listenerModel, listenerReplay, "listener_replay.mjs"],
    ["wakeups.qnt", wakeups, wakeupsReplay, "wakeups_replay.mjs"],
    ["session_progress.qnt", progress, null, ""],
    ["transport.qnt", transport, transportReplay, "transport_replay.mjs", 6],
  ]) {
    const all = /val allInvariants: bool = and \{([\s\S]*?)\n {2}\}/.exec(src)?.[1] ?? "";
    const declared = new Set([...all.matchAll(/^\s*(\w+),$/gm)].map((m) => m[1]));
    check(declared.size > 0, `${file}: allInvariants was not found; the parser needs updating`);
    const defined = new Set([...src.matchAll(/^ {2}val (\w+): bool =/gm)].map((m) => m[1]));
    const blocks = buildBlocks(file);
    const listed = new Set([...(blocks.find((b) => b.open === "[")?.body ?? "").matchAll(/"(\w+)"/g)].map((m) => m[1]));
    check(
      diff(declared, listed).length === 0 && diff(listed, declared).length === 0,
      `${file}: allInvariants and the list in spec/BUILD.bazel differ: ` +
        `${[...diff(declared, listed), ...diff(listed, declared)].join(", ")}`,
    );

    const mutantBody = blocks.find((b) => b.open === "{")?.body ?? "";
    const listedMutants = new Map([...mutantBody.matchAll(/"(\w+)": "(\w+)"/g)].map((m) => [m[1], m[2]]));
    const mutants = new Set([...src.matchAll(/^ {2}action (init[A-Z]\w+) = initWith\(/gm)].map((m) => m[1]));
    check(mutants.size > 0, `${file}: no mutant found; every model that is replayed keeps at least one`);
    check(
      diff(mutants, new Set(listedMutants.keys())).length === 0 && diff(new Set(listedMutants.keys()), mutants).length === 0,
      `${file}: the mutants in the model and in spec/BUILD.bazel differ: ` +
        `${[...diff(mutants, new Set(listedMutants.keys())), ...diff(new Set(listedMutants.keys()), mutants)].join(", ")}`,
    );
    for (const [init, invariant] of listedMutants) {
      check(defined.has(invariant), `${file}: spec/BUILD.bazel says mutant ${init} violates ${invariant}, which the model does not define`);
    }

    if (replay !== null) {
      const actions = new Set([...src.matchAll(/note\("(\w+)"/g)].map((m) => m[1]).filter((n) => n !== "init"));
      // The replay's own switch over the model's actions, and the actions it
      // says it leaves to the model checker.
      const taken = new RegExp(`switch \\(did\\.name\\) \\{([\\s\\S]*?)\\n {${indent}}default:`).exec(replay)?.[1] ?? "";
      const left = /const NOT_REPLAYED = \[([^\]]*)\]/.exec(replay)?.[1] ?? "";
      const cases = new Set([...taken.matchAll(/^\s*case "(\w+)":/gm), ...left.matchAll(/"(\w+)"/g)].map((m) => m[1]));
      check(actions.size > 0 && taken !== "", `${file}: no action names found; the parser needs updating`);
      check(
        diff(actions, cases).length === 0 && diff(cases, actions).length === 0,
        `${file} and spec/replay/${replayName} disagree about the model's actions: ` +
          `${[...diff(actions, cases), ...diff(cases, actions)].join(", ")}`,
      );
    }
  }

  // An invariant named next to code, as `spec/<model>.qnt ... \`name\``,
  // exists in that model.
  const models = { "wakeups.qnt": wakeups, "listener.qnt": listenerModel, "transport.qnt": transport };
  for (const [where, src] of [
    ["server.rs", server],
    ["main.rs", mainRs],
    ["mcp.rs", mcpRs],
    ["listener.mjs", listenerMjs],
  ]) {
    for (const m of src.matchAll(/spec\/(\w+\.qnt)((?:[^.;)]|\n)*?)[.;)]/g)) {
      const model = models[m[1]];
      if (!model) continue;
      for (const name of m[2].matchAll(/`(\w+)`/g)) {
        check(
          new RegExp(`^ {2}(?:val|def|pure def|action) ${name[1]}\\b`, "m").test(model),
          `${where} refers to \`${name[1]}\` in spec/${m[1]}, which does not define it`,
        );
      }
    }
  }
}

if (problems.length) {
  console.error(`names_test: ${problems.length} mismatch(es) between the model and the code:\n`);
  for (const p of problems) console.error(`  - ${p}`);
  console.error("\nUpdate the model named in each line to match, then re-run //spec:quint_test.");
  process.exit(1);
}

console.log(
  `names_test: ${rpcs.size} operations, ${granted.size} granted, ${denied.size} denied, ` +
    `${spellings.size} spellings; capability.qnt matches capability.rs and ops.proto, ` +
    `lifecycle.qnt / eventlog.qnt / revisions.qnt match server.rs, engine.proto and cadpole-ffi, ` +
    `and wakeups.qnt / listener.qnt match the three readers, the listener and their replays`,
);
