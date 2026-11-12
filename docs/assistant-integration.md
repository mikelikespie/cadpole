# Assistant integration

How a coding assistant works with a running Cadpole session: you type in the
web UI's Chat panel, the assistant edits the script and answers there.

Nothing here costs anything while you are not typing. Every way of attaching
an assistant described below waits without running a model turn: no polling
by the model, no heartbeat.

There are two modes, and both talk to the same `cadpole serve`:

| | Interactive | Limited listener |
| --- | --- | --- |
| What it is | your own open Claude Code session (or another MCP client), with the `cadpole` MCP server and `cadpole listen` running in the background | `//src/tools/listener`, a small program that runs its turns with your installed Claude Code in headless mode (`claude -p`, streaming JSON) |
| How a chat message reaches the model | `cadpole listen --next` exits with the message; the session is woken by that | the listener writes the message to a waiting headless process |
| How the answer appears in Chat | whole, when the assistant calls `reply` | as it is written, with a short line saying what the assistant is doing |
| Idle cost | none | none |
| What it can touch | everything your session can: files, shell, the whole project, plus every Cadpole tool | only the document, through the operations the listener token grants; no shell, no files, no network |
| Who watches | you, in the terminal, approving what your session asks to do | nobody; it runs unattended |
| Context | your session's: the project, what you discussed | one conversation per listener run, about the document only; `/new` starts over |
| Cost to watch | your session's usual usage | each message is a model conversation turn with the whole conversation so far; `--max-turns` and `--new-conversation-after` bound it |
| Needs | Claude Code, as you normally use it; the `cadpole` MCP server registered (`claude mcp add`, or `.mcp.json`) | Claude Code 2.1.259 or later, whose `claude -p` works on this machine; started and stopped by hand |

Choose the interactive mode when you are building a model with the assistant
at the keyboard and want it to use the rest of your project. Choose the
listener when you want something that reacts to the web UI on its own and
can only touch the document.

What can go wrong at start, and what you see:

| | Interactive | Limited listener |
| --- | --- | --- |
| No `cadpole serve` running | `cadpole listen` exits at once: `no session at <url>` | the listener exits: it cannot attach |
| Claude Code missing or too old | n/a | the listener refuses to start and says which version it needs |
| `claude -p` cannot run a turn | your session says so | the first message is answered in Chat with the failure, once |
| The UI's Listen toggle is off | `cadpole listen` waits and prints nothing | the listener waits and acts on nothing |

Authentication to the Cadpole server is described in docs/security.md.
Neither mode involves Cadpole in running the model: turns run in your own
Claude Code, and Cadpole holds no credential for it.

## Interactive mode

### Setup

The commands below call `cadpole` by name: run `./bin/install` in the
checkout once to put it on `PATH` (`docs/getting-started.md`).

1. Start a session:

   ```
   cadpole serve examples/bracket.star
   ```

2. Register the MCP server with Claude Code, from the project's folder:

   ```
   claude mcp add cadpole -- cadpole mcp
   ```

   That adds it for you in this folder (Claude Code's default `local`
   scope). `--scope project` writes it to the folder's `.mcp.json` instead,
   for everyone who works on the project. `claude mcp list` shows it and
   whether it connects; `claude mcp remove cadpole` takes it out (add
   `--scope project` for the `.mcp.json` entry).

   Another MCP client, or registering by hand, uses the same command and
   arguments; as a `.mcp.json`:

   ```json
   {
     "mcpServers": {
       "cadpole": { "command": "cadpole", "args": ["mcp"] }
     }
   }
   ```

   No URL and no port: `cadpole mcp` finds the running server itself (see
   "Finding the server" below).

3. Start Claude Code in that folder and ask it to wait for Cadpole:

   ```
   Wait for messages from Cadpole in the background and act on them.
   ```

   The MCP server's instructions tell the assistant how: it runs

   ```
   cadpole listen --next
   ```

   as a background command. That command prints nothing and costs nothing
   until you send a message; then it prints the message as one line of JSON
   and exits, which wakes the session. The assistant acts, replies in Chat
   and starts the command again.

4. In the web UI, turn **Listen** on and type in the Chat panel.

The first start in a folder asks whether you trust the folder (and, for a
server in the project's `.mcp.json`, whether to use it); answer yes, or the
assistant has no Cadpole tools and edits the file on disk instead of the
session.

`"command": "cadpole"` is looked up on the `PATH` of your login shell, which
is what Claude Code starts MCP servers with, not the `PATH` of the terminal
you started it from. If `cadpole` is not on it, the session reports
`Executable not found in $PATH: cadpole` and has no Cadpole tools: put the
binary's absolute path in `command`, and the same path in the permission
rule below (`Bash(/path/to/cadpole listen *)`).

No token needs to be configured: `cadpole serve` writes the session token
to its runtime directory on every start, and `cadpole mcp` and `cadpole
listen` read it from there. `--token`, `CADPOLE_TOKEN`, `--token-file` and
`CADPOLE_TOKEN_FILE` override that.

### Finding the server

`cadpole serve` listens twice: on a loopback TCP port for the browser, and on
a Unix socket for agents, `$XDG_RUNTIME_DIR/cadpole/<instance>.sock` (where
there is no `XDG_RUNTIME_DIR`, as on macOS, `~/.local/state/cadpole`). Beside
the socket it writes a small record (where the socket and the port are, which
document it serves; no token), all 0600 in a 0700 directory, and removes them
when it stops. `cadpole mcp`, `cadpole op`, `cadpole listen` and the listener
read the records and use the server that answers: the only one running; with
several, the one whose document is in the current directory; otherwise they
list them and ask for `--server`. A client stays with its document: when that
server restarts it reconnects to the new one, and when it is gone it says so
rather than moving to another document.

`--server unix:/path/to.sock` names a socket. `--server http://host:port`
still works for a client on another machine or one that cannot use a Unix
socket, but only against a server started with `--agent-tcp`: by default the
TCP port is the browser's and refuses the agent tokens (docs/security.md,
"Two surfaces").

To skip the permission prompts for this, allow the Cadpole tools and the one
command in the project's `.claude/settings.json` (the assistant is told the
command under the name the MCP server was registered to start `cadpole`
with):

```json
{
  "permissions": {
    "allow": ["mcp__cadpole__*", "Bash(cadpole listen *)"]
  }
}
```

### `cadpole listen`

```
cadpole listen             # the events in the log, then exit
cadpole listen --next      # wait for the next event worth acting on, print it, exit
cadpole listen --follow    # the same, without exiting
```

`--next` and `--follow` exist to be watched by a program that is woken by
their output, so they are silent unless there is something to act on:

- **Nothing is printed while nothing happens.** No heartbeat, no banner.
  Events go to stdout, one JSON line each; the few notes (the server went
  away, Listen is off) go to stderr.
- **Only events worth a model turn are printed**: a `message` typed by the
  user, and a `selection_changed` that carries a note. A bare selection, a
  parameter drag, a script edit or a camera move prints nothing; the
  assistant reads that state with `get_session` and `get_selection` when it
  next runs. `--kinds message,params_changed,...` prints those kinds
  instead, and `--all` prints everything.
- **The Listen toggle is honoured.** What is sent while it is off is never
  printed, also not by a later run after it has been turned on; what was
  sent while it was on is printed even if it has been turned off since.
  `--always-listen` ignores the toggle.
- **The position is kept by the server**, under `--client ID` (default
  `listen`), after every batch. The next `--next` starts after the last line
  the previous one printed, so a message sent while the assistant was busy
  is printed at once by the next run and is not lost. A client the server
  has not seen starts at the end of the log. A line is printed twice only
  when a run ends between printing it and storing its position: it is
  killed there, or the server does not take the position.
- **A server that stops answering is waited for.** After a restart of
  `cadpole serve` the command reads the new session's log from its start.
  What it had not read of the old session is gone with that session, and a
  `cadpole listen` started only after the restart is new to the server and
  begins at the end of the new log. With no server at all when it starts,
  it exits with an error.

Which one to use depends on what the assistant's tooling can watch.
`--next` suits a background command whose *exit* wakes the assistant: in an
interactive Claude Code session a background command has no time limit, so
the wait costs nothing however long it lasts. `--follow` suits a tool that
wakes the assistant for each *line* a running command prints; in Claude
Code that is the Monitor tool, whose watches end after at most 30 minutes
and notify the assistant when they do, so a `--follow` watch there costs one
short turn per expiry and `--next` is the better default.

`wait_events` is not a way to wait for the user. It is a tool call, and for
an assistant every call is a model turn, also when it returns nothing. Use
it for one read: the events since you joined, the `client_failure` events,
or a short wait for something you just started.

### `cadpole mcp`

`cadpole mcp` is a stdio MCP server (JSON-RPC 2.0, written in Rust inside
`cadpole-cli`). It exposes the Cadpole operations as tools.

| Flag | Meaning |
| --- | --- |
| `--server URL` | the `cadpole serve` to attach to: `unix:/path/to.sock` or `http://host:port` (default: the running one, found as above) |
| `--token`, `--token-file` | the bearer token, or a file holding it |
| `--out-dir DIR` | where rendered images are written (default: `cadpole-mcp` under the system temp directory) |
| `--always-listen` | send channel notifications even when the UI's Listen toggle is off |
| `--listener` | limited mode: use the listener token and publish only the operations it grants |

`--always-listen` concerns the channel notifications below.

### Tools

The tools are generated. Each visible rpc of `service CadpoleOps` in
`proto/cadpole/v1/ops.proto` is one MCP tool, one `cadpole op` subcommand
and one HTTP route, all built from the service descriptor at run time. To
see the current list:

```
cadpole op --list            # every operation, with its CLI name
cadpole op <Rpc> --help      # one operation's flags
```

`GET /api/ops` on a running server and `tools/list` over MCP return the
same registry.

- **Names.** The rpc name is canonical. The MCP tool name is its
  snake_case: `Render` is `render`, `GetSelection` is `get_selection`,
  `WaitEvents` is `wait_events`. `POST /api/ops/<Rpc>` and `cadpole op <Rpc>`
  also accept the operation's CLI name and aliases (`Evaluate` is also
  `eval`). The Connect route, `POST /api/cadpole.v1.CadpoleOps/<Rpc>`,
  accepts only the rpc name.
- **Descriptions.** A tool's `description` is the rpc's leading comment in
  the `.proto`. Each input property's `description` is the field's comment.
- **Hidden fields.** Request fields marked `(cadpole.field).hidden` are not
  in the schema. `document` is one: the session supplies it. An operation
  marked `needs_session` fails with a clear error when no `cadpole serve`
  is reachable.
- **Results.** Every tool returns one text block holding the response as
  proto3 JSON. When the rpc has an `image_field` (for example `Render.png`
  and `Sweep.png`), the image is written under `--out-dir`, under a name unique to the
  `cadpole mcp` process and never over an existing file, and also
  returned as an `image` content block; the text block carries `path` in
  place of the bytes. Errors are `isError: true` with a message.

The operations an assistant uses most:

| Tool | Use |
| --- | --- |
| `get_session` | the scripts, parameters, current revision and counts of messages and failures |
| `get_selection` | what the user has selected, with geometry (surface kind, area, normal, bounding box) |
| `explain_ref` | the chain of features that made a selected entity, each with its source span |
| `edit_script` | replace exact text, a line range or a feature's arguments, under `base_revision` |
| `set_params` | set parameter overrides without touching the script |
| `evaluate`, `run_tests`, `print_check` | check the result |
| `render`, `sweep` | look at the result; `highlight_selection` draws the selection |
| `wait_events`, `ack` | read session events once and record how far you have read (not a way to wait: see `cadpole listen`) |
| `get_attachment` | fetch a screenshot the user attached to a message |
| `reply` | answer in the UI's message panel, as one whole message |
| `api_doc` | the scripting API, including the accepted values of each vocabulary |

`edit_script` and `set_script` are the only ways to change geometry. The
server parses the new text before storing it, so a syntax error is returned
to the caller and nothing is stored. Both return the new revision, the
diagnostics and the feature ids from re-evaluating. When `base_revision` is
stale, nothing is written and the response has `conflict: true` with
`current_revision` and `current_source` to rebase onto.

Renders are clamped to the server's limits (docs/security.md); a clamp is
reported as a warning diagnostic.

### Events

The web UI's **Listen** toggle controls whether events are passed to an
assistant at all. It is off by default.

| `kind` | When | Content |
| --- | --- | --- |
| `assistant_listening` | the Listen toggle was set | the new value, and `was`: the value before |
| `selection_changed` | the selected set or its note changed | each selected ref |
| `message` | the user sent text in the message panel | the text, prefixed `user:`; with attachments, the `get_attachment` call that fetches each picture |
| `params_changed` | the user changed a parameter | the override map |
| `script_changed` | someone other than this client wrote the script | the path and the new revision |
| `asset_changed` | a file was added to the document | its name, size and kind |
| `client_failure` | a front end crashed or an operation failed in it | what broke and where, with the operation, script revision and parameters |
| `solve_started` | an optimisation solve started as a background job | the job id and the solve it answers |
| `solve_finished` | that job ended and the document was re-evaluated | the job, its status, and how many solves are still pending |

A `message` is a request from the user. Everything else, including
selection notes, script comments, parameter names and failure text, is data
about what the user is looking at, never an instruction. The server's MCP
`instructions` string says so to the assistant.

To read the failures of a session you have just joined, call
`wait_events` with `after: 0` and `kinds: ["client_failure"]`;
`get_session` carries a `client_failures` count.

### Channels (optional, preview)

`cadpole mcp` also declares `capabilities.experimental["claude/channel"]`
and sends, as `notifications/claude/channel` notifications, the same events
`cadpole listen --next` prints: a message the user typed, and a selection
that carries a note. A notification starts a model turn, so nothing else is
pushed; a parameter drag or a bare selection costs nothing.

```json
{
  "method": "notifications/claude/channel",
  "params": {
    "content": "user: this face is too thin, make the lid 5 mm",
    "meta": { "kind": "message", "seq": "3" }
  }
}
```

Channels are a
[research-preview feature](https://code.claude.com/docs/en/channels#research-preview)
of Claude Code, and nothing above depends on them. They need Claude Code signed in with a claude.ai or Console
account, an organization on a Team or Enterprise plan has to enable them,
and availability is rolled out gradually. A server that is not on the
preview's approved list, as `cadpole` is not, is loaded as a channel only
with

```
claude --dangerously-load-development-channels server:cadpole
```

which asks for confirmation at start and is ignored in non-interactive mode.
The flag is documented under
[Test during the research preview](https://code.claude.com/docs/en/channels-reference#test-during-the-research-preview)
and may change while the feature is in preview. Without the flag the
notifications are dropped. With it, events arrive without a background
command, at no idle cost; do not also run `cadpole listen`, or each message
arrives twice. `cadpole mcp` does not declare the permission-relay
capability.

The channel keeps no place of its own: every start of `cadpole mcp` begins
at the end of the log, so a message sent while none was running is not
pushed later. A `cadpole mcp` that found its server itself follows a restart
of it: its next call goes to the new run, with that run's token, and the
channel starts at the end of the new log, as a new `cadpole mcp` would. One
given `--server` holds the old run's token; its calls are refused and it
pushes nothing until it is started again.

### An example exchange

The user selects a face and asks for a change; the assistant finds the
line that made the face, edits it and replies. Responses are abbreviated.

```
$ cadpole listen --next          # in the background; exits with:
{"seq": 3, "kind": "message", "by": "ui",
 "data": {"from": "user", "text": "this face is too thin, make the lid 5 mm"}}

--> get_selection {}
<-- {"refs": [{"ref": {"feature_id": "lid", "kind": "ENTITY_KIND_FACE", ...},
               "face": {"surface_kind": "plane", "normal": {"z": 1.0}, ...}}]}

--> explain_ref {}                      # no argument: explain the selection
<-- {"chain": [{"feature_id": "lid",  "kind": "subtract", "span": {...}},
               {"feature_id": "slab", "kind": "extrude",  "span": {"path": "main.star", "start_line": 5, ...}},
               ...],
     "summary": "..."}

--> get_session {}
<-- {"revision": 1, ...}

--> edit_script {"path": "main.star", "base_revision": 1,
                 "edits": [{"old_str": "param(\"lid_thickness\", 3.0",
                            "new_str": "param(\"lid_thickness\", 5.0"}]}
<-- {"revision": 2, "applied": 1, "diff": "...", "features": [...], "diagnostics": [...]}

--> render {"width": 640, "height": 480, "highlight_selection": true}
<-- text {"path": ".../cadpole-render-1.png", "width": 640, "height": 480, ...} + image

--> reply {"text": "The lid is 5 mm now. I changed the lid_thickness parameter, so the slider still works."}

$ cadpole listen --next          # started again; silent until the next message
```

## The limited listener

`//src/tools/listener` is a Node program that answers Chat messages on its
own, with a restricted set of operations. It runs its turns with the Claude
Code installed on the machine, in headless mode: `claude -p` with streaming
JSON in and out ([headless mode](https://code.claude.com/docs/en/headless) in the Claude Code docs). It contains
no model client and holds no credential.

```
cadpole serve                         one session; mints two tokens per run
   │    session-token    the web UI, cadpole op, an interactive cadpole mcp
   │    listener-token   only the granted operations, enforced by the server
   │
   └── cadpole mcp --listener         Authorization: Bearer <listener-token>
            ▲                    ▲
            │ stdio (MCP)        │ stdio (MCP), spawned by the headless process
            │                    │
     //src/tools/listener ───► claude -p (headless) ───► the model
       the event loop            one process per conversation
```

Start it with a server already running:

```
bazel run //src/tools/listener
bazel run //src/tools/listener -- --driver echo   # no model
```

| Flag | Meaning |
| --- | --- |
| `--server URL` | the `cadpole serve` to attach to (`CADPOLE_SERVER`) |
| `--cadpole PATH` | the `cadpole` binary (`CADPOLE_BIN`, default `cadpole`) |
| `--driver claude\|echo` | `claude` runs turns through the installed Claude Code; `echo` runs the loop with no model (`CADPOLE_LISTENER_DRIVER`) |
| `--claude PATH` | the Claude Code executable (`CADPOLE_CLAUDE_BIN`, default `claude`) |
| `--model NAME` | the model the turns run on (`CADPOLE_LISTENER_MODEL`; default: Claude Code's own) |
| `--always-listen` | act even when the UI's Listen toggle is off |
| `--max-turns N` | model turns per message (default 12) |
| `--new-conversation-after N` | start a fresh conversation after N messages (default 30; 0 for never) |
| `--poll-ms N` | long-poll timeout (default 30000) |
| `--once` | handle one poll and exit |

The listener prints the tools it was given and then waits. With the UI's
Listen toggle off it follows the event stream and acts on nothing.

### How a turn runs

On start the listener runs `claude --version` and refuses to go on if the
program is missing or older than 2.1.259 (the first version with every flag
below). `--driver echo` needs no Claude Code at all.

On the first message it starts one headless process and keeps it:

```
claude -p --input-format stream-json --output-format stream-json --verbose
       --include-partial-messages
       --mcp-config '{"mcpServers":{"cadpole":{"command":"cadpole","args":["mcp","--listener"]}}}'
       --strict-mcp-config
       --tools ""
       --allowedTools mcp__cadpole__evaluate,mcp__cadpole__render,...
       --disallowedTools mcp__cadpole__reply
       --permission-mode dontAsk --permission-prompts none
       --setting-sources "" --disable-slash-commands
       --no-session-persistence
       --system-prompt <the listener's own>
       --max-turns 12
```

Each message is written to that process's stdin as one line of JSON, and
the listener reads the answer from its stdout. Between messages the process
is blocked reading stdin: no model turn runs while nobody types. The
listener's own long poll of the Cadpole server is an HTTP request, not a
model call.

**The reply appears as it is written.** `--include-partial-messages` makes
the process print the model's text as it is generated and the start of each
tool call. The listener starts a message in Chat as soon as the model
writes or starts a tool call, appends the text to it a few times a second, and shows what a tool call is
doing as a short line beside it: "reading the model…", "editing the
script…", "rendering…", "checking the model…". The model's own text is the
reply, so its session is started without the `reply` tool. When the turn
ends the message settles; if it fails, the message keeps what was written
and ends with one line that says why. The server operation behind this is
`StreamReply` (docs/design.md, "Streamed replies"): pieces of a reply are
not session events, so they wake no other assistant, and a reply whose
listener died is ended by the server after 30 seconds rather than left in
progress.

- **One conversation per run.** The process stays, so the model remembers
  what it just changed ("undo that" works). The conversation grows with
  every message, and each turn sends all of it; that is the cost to watch.
  `--new-conversation-after N` replaces the process after N messages, and
  the message `/new` in Chat does so at once, without a model turn.
- **Messages are handled in order**, one at a time. A message sent while a
  turn runs waits for it.
- **`--max-turns` is per message.** A message that needs more model turns
  than that ends with a line in Chat saying so.
- **A failed turn is answered in Chat** with one line saying why:
  `claude -p` failing to run the turn, the process exiting in the middle of
  a turn, the turn limit. If the reply had started, it keeps what was
  written and ends with that line, marked as failed. If the model had done
  nothing yet, the line is a message of its own, said once per distinct
  failure in a row, so a listener that cannot run at all does not answer
  every message with the same line.
  After an exit the next message starts a new process (and a new
  conversation); the listener's position in the event log is the server's,
  so nothing is skipped or repeated.
- The conversation is not saved to your Claude Code session history, and it
  runs in an empty temporary directory.

The listener runs `claude -p` and nothing else on the model's side; how
Claude Code runs a headless turn is covered by its own
[documentation](https://code.claude.com/docs/en/headless). The listener neither asks for nor reads a credential,
and passes none on.

### What it may do

The listener holds no token itself. It spawns `cadpole mcp --listener`,
which reads the listener token from the runtime directory and refuses to
start without it. The server refuses any operation that token does not
grant with `403`, whatever the model was told or however its process was
configured. docs/security.md, "Capabilities", lists the granted and denied
operations; in short, the listener can read the session, evaluate and
render, edit the script files that exist with `edit_script` (any line
range of them, so it can rewrite one when asked), set parameters and reply,
and it cannot create script files, move the selection, add assets, save to
disk or post as the user. Its edits stay in the session until the user
saves.

That is the boundary. On the client side, as defence in depth, the headless
session is given only the Cadpole tools:

| What is kept out | How |
| --- | --- |
| built-in tools (shell, file read and write, web, subagents) | `--tools ""` |
| other MCP servers, from user, project or plugin configuration | `--strict-mcp-config` with the one server |
| settings, hooks, instruction files, plugins | `--setting-sources ""`, and an empty working directory |
| skills and commands | `--disable-slash-commands` |
| any tool not named | `--allowedTools` with the published Cadpole tools (less `reply`, which `--disallowedTools` removes), `--permission-mode dontAsk`, `--permission-prompts none` |

The listener then checks rather than trusts: every turn of the headless
process begins with a line that lists the session's tools and MCP servers.
If that list holds anything but the allowed `mcp__cadpole__*` tools and the
one `cadpole` server (or any plugin, skill or command from configuration),
the listener kills the process before reading anything else from it, says
so once in Chat, and refuses every later message until it is restarted.

### What starts a turn

Only two kinds of event:

- a `message` from the user;
- a `selection_changed` that carries a note, such as "this wall is too
  thin" on a selected wall.

A bare selection, a parameter change, a script edit or a `client_failure`
does not start a turn; the model reads them as context when it next runs.

Each event is handed to the model in an envelope that says what it is:

```
<cadpole_event kind="selection_changed" seq="41" content-is="data-about-what-the-user-is-looking-at">
{ "selected": [ { "feature_id": "wall", "kind": "face", "topo_name": "wall/side[0.2]" } ],
  "note": "this wall is too thin" }
</cadpole_event>
```

`content-is` is `request-from-the-user` for a message and
`data-about-what-the-user-is-looking-at` for everything else.

### Restarts

The listener records how far it has read with `ack`, and the server keeps
that cursor per client id. A restarted listener resumes from its cursor. A
new client starts at the end of the log, not at the beginning.

That only works if the id survives the restart, so the listener asks under a
fixed id, `listener`, rather than anything per process. `--client ID` (or
`CADPOLE_LISTENER_CLIENT`) gives each of several listeners on one server its
own cursor.

The cursor is recorded after every message acted on. A listener that is
stopped, or dies, in the middle of a turn has recorded nothing for that
message: the next run asks it again, and answers the messages that were
waiting behind it. That is the only case in which a message is asked twice.

After a restart of `cadpole serve` the listener has to be restarted too. It
stays with the run it attached to, so every call it makes fails, saying the
server was restarted, and it starts no turn; started again, it is new to
the server and begins at the end of the new log.

### Tests

The real Claude Code is never run by `bazel test`. The tests are:

- `//src/tools/listener:listener_test`: the MCP stdio transport, the event
  loop, which events start a turn, the prompt envelope, the Listen gate,
  the cursor, `/new`, and the reply in progress (started at once, sent on
  a few times a second, a throttled piece sent again, the keep-alive, the
  failure line, a server without `StreamReply`), with a scripted driver;
- `//src/tools/listener:driver_test`: the exact argv and stdin of the
  headless process and how its output is read (text as it is generated or a
  block at a time, a tool call and its activity line, errors and an exit in
  the middle of a turn after some text), the version check, and the refusal
  of a session that reports an extra tool or MCP server, against a stand-in
  executable;
- `//src/tools/listener:listener_e2e_test`: a real `cadpole serve` and a
  real `cadpole mcp --listener`, checking that granted operations work, a
  streamed reply lands as one finished message that only its author could
  write, and denied operations are refused;
- `//src/tools/listener:listen_command_e2e_test`: a real `cadpole listen
  --next` and the channel of a real `cadpole mcp` stay silent while a reply
  is written a piece at a time, and wake on the user's next message.

Two more tests tie this page to the models in `spec/` (`spec/README.md`):
`//spec/replay:listener_replay_test` makes the shipped entry point take the
steps of traces generated from `spec/listener.qnt`, and
`//spec/replay:wakeups_replay_test` does the same for `cadpole listen`, the
channel and the listener's loop against a real `cadpole serve` with traces
from `spec/wakeups.qnt`.

`bazel run //src/tools/listener:live_check` is the same ground against the
real `claude -p`: it runs real model turns with the Claude Code of whoever
starts it, so it is tagged `manual` and no sweep runs it. Its usage
is at the top of `live_check.mjs`.

Not implemented: there is no route that starts a listener from the web UI
(start it yourself); an interactive assistant's `reply` appears whole, since
only the listener sees a model's output as it is generated; and nothing caps
total usage beyond `--max-turns` per message and `--new-conversation-after`.
