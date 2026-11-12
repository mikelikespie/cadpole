# Security

Cadpole is a single-user tool. `cadpole serve` is meant for one person on
loopback or a private network, not for the public internet, and it has no
multi-tenant isolation. It does evaluate scripts an assistant wrote or
someone pasted from the web, it accepts render jobs that can occupy a
machine, and its MCP bridge puts session text in front of a coding agent
that has real privileges. This document describes what protects each of
those.

## Threat model

| Actor or vector | Defence |
| --- | --- |
| The internet, through a misconfigured bind | Loopback by default; every API call needs a token |
| Another device on the same private network | Token on every API call; no filesystem browsing |
| A malicious web page in the user's own browser | No CORS headers; `Origin` and `Sec-Fetch-Site` checked on reads and writes; no cookies and no `?token=`; unguessable token |
| A script written by an assistant or pasted from the web | Starlark sandbox: no I/O, bounded steps, memory, recursion and time |
| Prompt injection through session text (messages, selection notes, script comments) into an assistant | Channel content is labelled as data; the limited listener's rights are a capability on its token, refused by the server |
| A render or sweep that overloads the host | Clamps, a concurrency limit and a bounded queue |
| A malformed imported file | Pre-scan, size caps, a deadline and `catch_unwind` around every parser |
| Supply chain (crates, npm, Bazel modules) | Lockfiles, pinned versions, licence and advisory tests |

Out of scope: physical access, a compromised operating system, a compromised
private-network key.

## Network binding

`cadpole serve` binds `127.0.0.1:7331` by default (`--port` changes the
port). `--listen` takes one of:

| Value | Binds |
| --- | --- |
| `loopback` | `127.0.0.1` only (the default) |
| `tailscale` | the Tailscale interface (an address in `100.64.0.0/10`), if present |
| `all` | every interface; only behind a firewall |

`--no-auth` disables authentication for development and is refused off
loopback.

## Two surfaces

The browser and the agents reach the server by different ways in:

| Surface | Where | Serves | Admits |
| --- | --- | --- | --- |
| Browser | the TCP port above (`--no-tcp` turns it off) | the web UI, its files and every route the page uses | the browser token; the session and listener tokens only with `--agent-tcp` |
| Agent | a Unix socket in the runtime directory, `<instance>.sock` (`--socket PATH` moves it, `--no-socket` turns it off) | the operation registry (`POST /api/ops/<Operation>`, `POST /api/cadpole.v1.CadpoleOps/<Rpc>`, `GET /api/ops`), `GET /api/session` and `GET /api/session/document`; no web UI, no static file, no other route | all three tokens |

The surface decides only two things: which tokens are admitted, and which
routes exist. A token admitted on the wrong surface is refused with `403`
before its role is looked at; a route the socket does not serve is `404`.
What an admitted token may do is decided by its role alone: the guard and
`capability_refusal` are one code path for both surfaces, and
`capability_refusal` does not take the surface. So the same operation gets
the same answer on every surface that admits the token, and the listener
token gains nothing on the socket. The bearer token is required on the
socket too: the socket's file mode is a second lock, not a replacement for
the first.

Why the agent tokens stay off TCP by default: the port is reachable by every
process on the machine, by every page the browser opens, and with `--listen`
by the network, while the socket is reachable only through a 0700 directory
of the user's. An agent's token that leaks (into a transcript, a log, a
prompt-injected tool call) then opens nothing on the port, and the listener's
restricted token is never valid on the network. `--agent-tcp` is for an agent
on another machine, or a client that cannot use a Unix socket; then `--server
http://host:port` works as before. The server judges tokens, not programs: a
client that holds the browser token is treated as a browser on either
surface.

The origin, CORS and `Host` checks below run on the socket as well, because
it is the same guard. They protect against a browser being used as a
confused deputy, and a browser cannot open a Unix socket, so on the socket
they never decide anything; they are kept rather than switched off so that
there is one code path to reason about.

On Windows there is no socket: the server is TCP only, and agents need
`--agent-tcp` and `--server http://...`.

`spec/transport.qnt` models both surfaces, the tokens each admits and the
routes each serves (invariants `theWebUiIsNeverOnTheSocket`,
`agentTokensStayOffTcpUnlessAllowed`,
`aTokensDecisionNeverDependsOnTheSurface`,
`theListenerNeverRunsADeniedOperation`,
`aRequestWithoutAGoodTokenIsRefused`).

## Runtime directory

The socket, the run's agent tokens and a record of the run live in the
runtime directory, all 0600, in a directory that must be the user's own and
0700. A server creates the directory 0700, tightens it if it is the user's
and looser, and refuses to start if it is a symlink or belongs to someone
else. When the directory's path is too long for a socket (a deep `$TMPDIR`),
the socket goes in `/tmp/cadpole-<uid>` instead, under the same checks.

Each run's files are named after a random instance id:

| File | Holds |
| --- | --- |
| `<instance>.sock` | the agent socket |
| `<instance>.json` | the record: instance, pid, socket path, HTTP URL, whether `--agent-tcp` is on, document path, start time. No token |
| `<instance>.session-token`, `<instance>.listener-token` | this run's agent tokens (also copied to `session-token` and `listener-token`, the newest run's, for older clients) |
| `<instance>.url` | the browser link with its token, written instead of printing it when the server's output is not a terminal |

A server removes its files when it is stopped (`SIGTERM` or Ctrl-C), before
it waits up to 3 s for open requests. A crash leaves them; the next server
to start removes a dead run's files after a connect to its socket fails, and
never touches a socket that answers or a file that is not the user's own
private file. A client uses a record only if the record and the socket are
the user's own 0600 files in a private directory and the socket answers a
connect; it never follows any other. `spec/transport.qnt` states this as
`callsGoOnlyToLiveServers`, `clientsNeverFollowForeignFiles`,
`cleanupNeverRemovesALiveServersFiles` and
`serversStartOnlyInAPrivateDirectory`, and `spec/replay/transport_replay.mjs`
checks the real programs against it.

## Authentication

Every start mints three tokens for that run: the **browser token**, the
**session token**, both of which may do everything, and the **listener
token**, which is restricted (see "Capabilities"). Each is 32 random bytes in
base64url. The session and listener tokens are written at mode 0600 into a
0700 runtime directory as `<instance>.session-token` and
`<instance>.listener-token` (and `session-token` and `listener-token`); the
browser token is written to no token file. The runtime directory is the first
of these that is set:

1. `$CADPOLE_RUNTIME_DIR`
2. `$CADPOLE_CONFIG_DIR`
3. `$XDG_RUNTIME_DIR/cadpole`
4. `$XDG_STATE_HOME/cadpole`
5. `~/.local/state/cadpole`

On a terminal the server prints the URL `http://127.0.0.1:7331/#token=<t>`
with the browser token, and the paths of the token files. When its output is
not a terminal (a log, a pipe, a service manager) it prints no token at all
and writes the URL to `<instance>.url` instead. The session and listener
tokens are never printed. Tokens do not survive a restart, so a bookmarked
URL stops working when the server does. If the runtime directory cannot be
written, the server still runs, with tokens that exist only in the process,
and says so. `--token` (or `CADPOLE_TOKEN`) pins the browser token instead of
minting one.

- **Web UI.** Reads `#token=` from the URL fragment on first load, removes
  it from the address bar with `history.replaceState` before the first
  request, keeps it in memory and `sessionStorage`, and sends
  `Authorization: Bearer <t>` on every fetch, the event stream included
  (the stream is read with `fetch`, not `EventSource`, so it can carry the
  header). A 401 clears the token and the UI asks for the URL the server
  printed.
- **No `?token=`.** No route accepts the token in a query string, so it
  never lands in browser history, a `Referer`, or a proxy log.
- **CLI and MCP server.** `--token`, then `--token-file` /
  `CADPOLE_TOKEN_FILE`, then the token file of the run they found (or, with
  `--server`, of the run that owns that socket), then `session-token`. A
  client that found its server itself looks again, and re-reads the token,
  when its connect fails or the token is turned down with `401`; it sends a
  request again only then, never after the request was sent, so nothing runs
  twice. The two reads that carry an event-log cursor are not sent again on a
  new run: the reader is told, and starts over on the new log.
  `CADPOLE_TOKEN` fills `--token` from the environment; prefer it or the
  file to a command-line token, which shows up in `ps`.
  `cadpole mcp --listener` reads the listener token and refuses to start
  when it is missing rather than fall back to the session token.
- **Comparison** is constant-time, and every configured token is compared
  with no early exit. A missing or wrong token is `401` with no detail; a
  valid token without the right capability is `403` naming the operation
  and the reason.

## Capabilities

The limited listener (docs/assistant-integration.md, "The limited
listener") is an agent attached to a session with a small set of
operations. The limit is not its prompt or the tool list its client shows;
it is the token it holds.

`src/rust/cadpole-ops/src/capability.rs` classifies every rpc of
`cadpole.v1.CadpoleOps` as granted or denied for the listener role, each
with a reason. The test `every_operation_is_classified` fails when an rpc
is added without a decision, and an unclassified operation is denied. The
server resolves the presented token to a role before any handler runs.

A listener token reaches exactly:

- a granted operation, over `POST /api/cadpole.v1.CadpoleOps/<Rpc>`
  (Connect) or `POST /api/ops/<Operation>`;
- `GET /api/ops`;
- `GET /api/session/document`.

Everything else under `/api/` is `403`, so a route added later does not
widen the listener's rights. Both operation routes go through one function
(`capability_refusal` in `src/rust/cadpole-cli/src/server.rs`) and one
policy. An alias such as `eval` is resolved to its rpc name before the
decision, and a percent-encoded operation name is refused.
`spec/capability.qnt` models both routes; its invariant
`theSameOperationIsDeniedOnEveryRoute` states that the route cannot change
the answer.

| | Operations | Why |
| --- | --- | --- |
| Granted, read-only | `GetSession`, `GetSelection`, `ExplainRef`, `ApiDoc`, `ListAssets`, `GetAsset`, `PrintCheck`, `Inspect`, `GetAttachment`, `WaitEvents`, `ListJobs`, `WaitJobs` | They read the session the listener is attached to and change nothing |
| Granted, pure compute | `Evaluate`, `Render`, `Sweep`, `Animate`, `Export`, `Drawing`, `RunTests` | Functions of a document it may already read, under the same sandbox, deadline, render gate and clamps as every caller. Results are returned as bytes, not written to disk. A listener's evaluation reads cached solve answers and attaches to running solve jobs but never starts one |
| Granted, bounded writes | `Ack`, `SetParams`, `EditScript`, `Reply`, `StreamReply` | `Ack` writes one cursor under the client's own id. `SetParams` is an override map: no code, one revision. Each `EditScript` edit names what it replaces (exact text, a line range or a feature's arguments) in a file that already exists, under `base_revision`; the result is parsed before it is stored and is size-capped, and each edit is a revision the user can revert. A line range can cover a whole file, so a listener asked to can rewrite an existing file this way; it cannot create one. `Reply` appends a message the server attributes to the assistant. `StreamReply` has the same effect a piece at a time, under the rules below; it is hidden, so it is not in `tools/list` or `GET /api/ops` |
| Denied | `SetScript` | Replaces a whole file and can create new ones. Rewriting an existing file stays possible through `EditScript`, as above |
| Denied | `SetSelection` | Moving the user's selection changes what their next request refers to |
| Denied | `AddAsset` | Writes caller-chosen bytes into the document under a name scripts later resolve with `asset("name")` |
| Denied | `ReportClientFailure` | Appends caller-written text to the event log every attached assistant reads; a listener could forge an engine failure |
| Denied | `PostMessage` | Posts as the user. It is also hidden, so it is not in `tools/list` or `GET /api/ops` |
| Denied | `Save` | Writes the session's document to disk; a listener's changes stay in the session until the user saves |
| Denied | `Solve` | Starts long-running solve jobs |
| Denied | `CancelJobs` | Stops the user's jobs |

Tests: `every_operation_is_classified` (cadpole-ops),
`a_listener_token_may_only_call_granted_operations` (server.rs) and
`//src/tools/listener:listener_e2e_test`, which runs a real
`cadpole mcp --listener` with a real listener token.

### A reply written a piece at a time

`StreamReply` lets the listener show a model's answer while it is being
written. What keeps that from being more than `Reply`:

| Property | Enforcement |
| --- | --- |
| A streamed message is the assistant's | The server sets `from`; the request has no field for it |
| Only its author writes to it | The token's role and the `X-Cadpole-Client` id that started the message must both match on every later call. The id is chosen by the caller, so this separates clients that cooperate (a listener and the model's own MCP process) and is not a secret; a holder of the same token that copies the id can write to the message, as it could post its own |
| A finished message is final | After `done` or `failed` every call naming the message is refused |
| It cannot grow without bound | The appended text is capped at the message limit (64 KiB), the activity line at 120 bytes on one line |
| It cannot flood the page | 8 calls back to back per message, then one per 200 ms; the rest are answered `throttled` and change nothing |
| It cannot flood the event log or wake an assistant | Pieces are not events. One `message` event is logged when a message ends, as `Reply` logs one, and it is not from the user, so `cadpole listen` and the channel pass nothing on |
| It cannot be left "typing" | No call for 30 s, or 30 min in progress, and the server ends the message as failed. One message in progress per client, four per session |

The rules are `src/rust/cadpole-cli/src/reply_stream.rs`; the tests are the
`StreamReply` group in `server.rs`, named for the invariants of
`spec/reply_stream.qnt`, whose traces are also replayed against the handler.

## Origin and CORS

Any page the user visits can `fetch("http://127.0.0.1:7331/...")`, so the
server checks where a request came from:

- No CORS headers are sent, so a cross-origin page cannot read a response.
- Every request other than `HEAD` and `OPTIONS` is refused with `403` when
  `Sec-Fetch-Site` is anything but `same-origin` or `none`, or when
  `Origin` does not match the server's own `Host`. This applies to reads as
  well as writes. A CLI sends neither header and is allowed; it already
  holds the token.
- DNS rebinding defeats both checks: once `evil.example` resolves to
  127.0.0.1, the browser sends `Origin` and `Host` for `evil.example` and
  calls it same-origin. With a token that page still has no bearer; a
  `--no-auth` server has nothing else, so it also requires the `Host` to be a
  loopback name (`localhost`, `*.localhost`, a loopback IP) or the host of an
  `--allow-origin`, for the page as well as the API; otherwise `403`
  (`security::check_host`).
- A request body must be `application/json`, a protobuf type or a Connect
  type. None of these is a type an HTML form can post without a preflight.
- `--allow-origin URL` (repeatable) admits one more origin, for a web dev
  server on another port. Nothing is allowed implicitly.
- Every response carries:

  ```
  X-Content-Type-Options: nosniff
  Referrer-Policy: no-referrer
  Content-Security-Policy: default-src 'self'; connect-src 'self'; worker-src 'self' blob:; img-src 'self' data: blob:; script-src 'self' 'wasm-unsafe-eval'; style-src 'self' 'unsafe-inline'; font-src 'self' data:; base-uri 'none'; form-action 'none'; frame-ancestors 'none'; object-src 'none'
  Cross-Origin-Opener-Policy: same-origin
  Cross-Origin-Embedder-Policy: require-corp
  Cross-Origin-Resource-Policy: same-origin
  ```

  `'wasm-unsafe-eval'` is there because instantiating a WebAssembly module
  counts as eval; `'unsafe-inline'` is for styles only.

## Filesystem access

- The HTTP API never takes a path. Documents are addressed by id
  (`/api/documents/<id>`) and resolved under one root (`~/Documents/cadpole`,
  or `--root`). An id is 1 to 64 ASCII letters, digits, `-` and `_`, checked
  before the disk is touched, and the resolved path is checked against the
  canonical root afterwards, so a symlink inside the root cannot lead out;
  a dangling one, which cannot be checked, is refused.
- Save writes to the document's own path. `POST /api/session/save-as` takes
  a name, reduces it to a valid id, and the server chooses the location
  under the root. Because that reduction is many-to-one, a file already
  under the id is replaced only when it is the session's own save target
  holding a document of the same name; anything else is a `409` unless the
  request says `"overwrite": true`. The write goes to a temporary file that
  is renamed into place, so it never truncates the old file and never
  writes through a symlink.
- Asset names are names, not paths: `/`, `\`, `..` and a leading dot are
  refused.
- Exports are returned as bytes. Only the CLI, running as the user, writes
  to a path the user chose.

## Starlark sandbox

Scripts run in `starlark-rust` with no I/O builtins, no environment access
and no clock. `load()` resolves only against the document's own script
files, never the filesystem or a URL. The limits:

| Limit | Mechanism | Default |
| --- | --- | --- |
| `print` | a `PrintHandler` that appends info diagnostics; nothing goes to stderr | 200 lines |
| Recursion | `Evaluator::set_max_callstack_size` | 200 frames |
| Steps | `Evaluator::set_max_tick_count`; a tick is a function call or a loop back-edge. Deterministic, and it works in wasm, where there is no clock | 200 M ticks |
| Wall clock | `Evaluator::set_check_cancelled`, plus a check on entry to every Cadpole builtin | 20 s in `cadpole serve` (`--eval-deadline-ms`), 120 s in the CLI (`--eval-budget-secs N` or `CADPOLE_EVAL_BUDGET_SECS`; 0 is none), off in the library |
| Memory | `Evaluator::set_max_heap_size` | 512 MB per file. A native process may change it with `cadpole --max-heap-mb N` or `CADPOLE_MAX_HEAP_MB`; the browser engine and the apps keep 512 MB |
| Request budget | `cadpole serve` evaluates and renders on a blocking worker; when the budget passes the caller gets `504` and the worker is left to the deadline above | 60 s (`--render-budget-ms`) |

The deadline cannot interrupt a single native operation that is already
running: a boolean that takes 15 s runs to completion and the deadline
fires at the next check. The real bound is the budget plus one kernel
operation. There are no caps on triangle or body counts.

In the browser, evaluation runs in a Web Worker inside the wasm sandbox.
There is no clock on `wasm32-unknown-unknown`, so the wall-clock deadline
never fires there; the tick, heap and recursion limits bound a script, and
the UI can terminate the worker.

### Optimisation solves

A solve can legitimately run for minutes, so it does not run to the
evaluation deadline. An inline attempt gets a window of the remaining
budget (at most `--solve-inline-ms`, 10 s by default). A solve that needs
longer is reported as pending and runs as a job: a separate `cadpole opt-job`
child process that runs no Starlark, sets resource limits and a lower
priority on itself, and is killed by its parent if it overruns.

| Flag | Default |
| --- | --- |
| `--solve-jobs` | `max(1, min(4, cores/4))` jobs at once |
| `--no-solve-jobs` | off; when set, no job is started |
| `--job-time-limit` | 3600 s |
| `--job-mem-mb` | 8192 |
| `--solve-inline-ms` | 10000 |

Only an evaluation made with the session token may start a job. Cached
answers and job files are under `~/.cache/cadpole/opt`.

## Render and sweep limits

| Limit | Default | Enforced in |
| --- | --- | --- |
| Pixels per image | 16 M, clamped keeping the aspect ratio | `cadpole_engine::limits::clamp_render` |
| Width or height | 8192 | same |
| Samples | 1024 | same |
| Threads / bounces | 256 / 64 | same |
| Sweep cells | 64 per request | `clamp_sweep` |
| Animation frames | 4096 per request (`cadpole_core::animation::MAX_FRAMES`) | `animate` |
| Time | 60 s per request (`--render-budget-ms`); the caller gets `504`, and no partial image is returned | `server.rs` |
| Concurrency | `--render-jobs`, default `min(2, max(1, cores/4))`, with a queue of 16; `429` beyond that | one semaphore shared by every render route |
| Listener operations | 8 at once (`LISTENER_OP_SLOTS`); `WaitEvents` holds no thread | `run_op` in `server.rs`; the restricted token's other calls wait for a slot without holding a thread |
| Request body | 8 MB by default (`--max-body`) | `DefaultBodyLimit`, and the same number re-checked in `post_op`, `post_connect` and `post_call` |

A clamp produces a warning diagnostic that says what was rendered instead,
not an error. An oversized body and a script that does not terminate are
refused. A running render cannot be cancelled.

## Imports

An import is where a document carries arbitrary bytes into a parser:
`import_step` (STEP), `import_mesh` (STL, OBJ, 3MF, glTF) and `import_dxf`
(ASCII DXF), all in `cadpole-import`. `AddAsset` is denied to the listener,
but a downloaded file is still untrusted input. In order:

1. **Pre-scan** (`cadpole_import::prescan`). Before any parser runs, the
   bytes are checked against these caps, and a violation is a diagnostic
   with the line number:

   | Format | Caps |
   | --- | --- |
   | any | 1 GB |
   | STEP | 64 MB of text, 2 M entity instances, parentheses 64 deep, 18 digits in an integer |
   | STEP, OBJ, ASCII STL, DXF | 1 MB per line |
   | STL, OBJ, 3MF, glTF | 5 M triangles |
   | OBJ, glTF | 10 M vertices |
   | glTF | 256 MB of JSON; scene graph 1024 deep, never cyclic |
   | 3MF | 256 zip entries, 256 MB inflated in all, 100:1 per entry, no `..` or absolute entry names, 64 MB per model part, components 8 deep and never in a cycle, at most 1 M object placements and 10 M placed vertices in a build (a component is a reference, so shared objects multiply) |
   | DXF | 64 MB of text, 8 M pairs, 1 M entities before `INSERT` expansion and 2 M after, 10 M vertices, `INSERT` 8 deep and never cyclic, 4 KB per text, 10 000 points per spline |

2. **Asset size.** One asset is at most 32 MB wherever bytes travel (an
   upload, a packed document, the browser). A native run reading `./assets`
   from disk allows 512 MB for a mesh; `cadpole --max-asset-mb` sets one
   cap for every asset.
3. **Deadline.** Every import runs under `cadpole_import::bounded::run`.
   When the deadline passes the caller gets a diagnostic and the parsing
   thread is abandoned to finish on its own.
4. **Panics.** Third-party parser calls are wrapped in `catch_unwind`; a
   panic becomes a diagnostic.
5. **Labels.** Names read from a file (a STEP name, an OBJ group, a 3MF
   object, a glTF node) pass through `cadpole_import::label`: control
   characters and the naming grammar's reserved characters are replaced and
   the result is capped at 64 characters.
6. **glTF buffers** resolve only to a `data:` URI or an asset of the same
   document. A URL or a path is refused and nothing is fetched.

On wasm there are no threads and no clock, so the readers run inline with
the same pre-scan and caps.

The malformed files under `src/rust/cadpole-import/testdata/fuzz` run in
`//src/rust/cadpole-import:cadpole_import_test`; each must produce a
diagnostic, not a panic.

## Published pages

`cadpole publish` writes a static site that can go on the open web.

- **No author text except names.** By default the published model carries
  no diagnostics, source spans, descriptions or scripts
  (`publish::strip_author_text`). `--include-sources` adds the scripts the
  entry file reaches by `load()`.
- **No parameters by default.** `--show-params a,b` (or `all`) publishes
  the named parameters, read-only, with their overrides.
- **Existing directories.** A publish refuses a non-empty directory that
  holds no earlier publish; `--force` overrides that.
- **The page** reads only the model it was published with, inflates it
  under a 512 MB cap and draws it. Nothing in the file is executed. A
  published page holds no token and calls no `/api` route.

## Client failure reports

`ReportClientFailure` lets a front end record a crash (a wasm panic, an
uncaught exception) in the session's event log.

- It is authenticated like every other call, and denied to the listener
  token.
- The text is untrusted. Control characters in `message` and `stack` are
  replaced before the report is stored, so a report cannot forge a log line
  or break an event-stream frame. `kind` and `surface` are proto enums. The
  MCP channel presents the text as data.
- It is capped, and no cap is silent:

  | Limit | Value |
  | --- | --- |
  | `message` | 2000 bytes, truncated; the response's `truncated` names the field |
  | `stack` | 8000 bytes |
  | other text fields | 300 bytes each |
  | `params` entries | 200 |
  | burst | 10 reports |
  | steady rate | 1 per 2 s |
  | per session | 200 recorded reports |

  A refused report is answered with which cap refused it, and the first
  refusal in each 10-second window is itself recorded as a
  `client_failure` event.

The web client (`FailureReporter` in `src/web/src/failure.ts`) also
coalesces identical failures before sending.

## MCP server and listener

`cadpole mcp` is a stdio process a coding assistant spawns.

- Its tools are the visible rpcs of `cadpole.v1.CadpoleOps` and nothing
  else: no file paths, no shell, no generic fetch.
- It talks to `cadpole serve` with a bearer token like any other client and
  has no rights beyond that token's.
- Channel events are forwarded only while the session's Listen toggle is
  on. Each carries its `kind` and `seq`, and the server's `instructions`
  tell the assistant that a message is a request from the user while
  selection notes and script comments are data.
- It does not declare the permission-relay capability, so nobody who can
  post a message to the UI can approve tool calls in the coding session.
- Events reach an interactive assistant through `cadpole listen`, which
  prints only a user's `message` and a noted selection, and only while the
  Listen toggle is on.
- The limited listener (`//src/tools/listener`) runs its model turns in a
  headless Claude Code process that is given the Cadpole tools and nothing
  else: no built-in tools (`--tools ""`), no MCP server but
  `cadpole mcp --listener` (`--strict-mcp-config`), no user or project
  settings, hooks, plugins, skills or commands (`--setting-sources ""`,
  `--disable-slash-commands`, an empty working directory), an allow-list of
  exactly the published Cadpole tools, and a permission mode that denies
  instead of asking. The listener reads the tool and MCP-server list that
  process reports at the start of every turn and kills it, refusing to run,
  if the list holds anything else. None of that is the security boundary:
  the boundary is the capability on its token, which the server enforces
  whatever the process was given.

## Secrets

Tokens live only in the runtime directory and are rewritten on every start.
`.gitignore` covers `token`, `session-token`, `listener-token` and `*.env`.
The listener's model turns run in the user's installed Claude Code, as
`claude -p` ([headless mode](https://code.claude.com/docs/en/headless)); Cadpole holds no model credential and
does not read, print or forward one.

## Supply chain

`bazel test //build_support/...` is the whole gate.

- **Pinned inputs.** `MODULE.bazel.lock`, `Cargo.lock` and `pnpm-lock.yaml`
  are committed. `Cargo.lock` is written only by Bazel
  (`CARGO_BAZEL_REPIN=1`). `onlyBuiltDependencies: []` in
  `pnpm-workspace.yaml` keeps npm lifecycle scripts off.
- **Licences.** `//build_support/licenses:license_test` checks every crate
  in `Cargo.lock` (against `crates.tsv`) and every package in
  `pnpm-lock.yaml` (against `npm.tsv`) with the allowlist
  `build_support/licenses/allowed.txt`. It fails on anything outside the
  list, unrecorded or unknown. SPDX expressions are parsed, so
  `(MIT OR Apache-2.0) AND GPL-3.0` is refused.
  `build_support/licenses/refresh.py` regenerates the two inventories.
- **Advisories.** `//build_support/advisories:advisory_test` needs no
  network: `rustsec.tsv` is a vendored copy of the RustSec database and
  `npm.tsv` holds the OSV advisories for the packages in `pnpm-lock.yaml`.
  Every advisory that matches a shipped version must be fixed or recorded
  with a reason in `accepted.tsv`. `build_support/advisories/refresh.py`
  refreshes the snapshots.
- **Attribution.** `THIRD-PARTY-NOTICES.md` is generated from the same
  inventories; `//build_support/licenses:attribution_test` diffs it against
  the committed copy and
  `bazel run //build_support/licenses:attribution_update` rewrites it.

Bazel modules, the Swift toolchain archives and other toolchain downloads
are pinned but are not covered by the licence and advisory tests.
