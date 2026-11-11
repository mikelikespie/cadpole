# Formal models

Fourteen [Quint](https://quint.sh) models of the protocols that Cadpole's
server and its clients have to agree on. Four of them are also replayed
against the running programs (`replay/`, and the `StreamReply` handler).

## The models

| File | What it models | Main code |
| --- | --- | --- |
| `session.qnt` | The session and event protocol: revisions, selection staleness, the event log, per-client cursors, a server restart (docs/design.md) | `cadpole-cli/src/server.rs`, `cadpole-engine/src/selection.rs` |
| `revisions.qnt` | Optimistic concurrency on the wire: how each route reads `base_revision`, and the revision a write's response returns | `cadpole-cli/src/server.rs` |
| `eventlog.qnt` | The event log as a bounded window: what a reader that resumes by sequence number sees or is told it missed | `cadpole-cli/src/server.rs` |
| `capability.qnt` | The limited listener token: roles, routes, spellings, operations and their effects | `cadpole-ops/src/capability.rs`, `cadpole-cli/src/server.rs` |
| `lifecycle.qnt` | One request from arrival to its single reply, over HTTP and over the C ABI: admission, budget, render gate, cancellation | `cadpole-cli/src/server.rs`, `cadpole-ffi/src/lib.rs` |
| `deadlines.qnt` | The two deadlines that `EvaluateRequest.deadline_ms` arms | `cadpole-ffi/src/lib.rs`, `cadpole-script` |
| `jobs.qnt` | The asynchronous job seam: supersede, coalesce, cancel, staleness, fallback to the local engine | `src/web/src/jobs.ts`, `src/apple/CadpoleKit/ParamCoalescer.swift` |
| `reply_stream.qnt` | A reply posted a piece at a time (`StreamReply`): who may write to it, finality, attribution, the caps, the server ending an abandoned one, and what reaches the event log | `cadpole-cli/src/reply_stream.rs`, `cadpole-cli/src/server.rs` |
| `reload.qnt` | The `build_changed` event, the replay of the log on connect, and the page reloading itself | `src/web/src/main.ts` |
| `hotswap.qnt` | What the page does with a rebuilt bundle: swap the stylesheet, swap the engine, or offer a reload | `src/web/src/hot-reload.ts` |
| `wakeups.qnt` | What reaches an assistant and when: the three programs that wait on the event log (`cadpole listen`, the channel in `cadpole mcp`, the limited listener's loop), the Listen toggle, cursors by client id, a restart of a reader and of the server | `cadpole-cli/src/main.rs`, `cadpole-cli/src/mcp.rs`, `src/tools/listener/listener.mjs`, `cadpole-cli/src/server.rs` |
| `listener.qnt` | The limited listener's session lifecycle: the version check, one turn at a time, the session process and its replacement, the check of what a session reports, how a turn ends, stopping | `src/tools/listener/main.mjs`, `listener.mjs`, `driver.mjs` |
| `transport.qnt` | How local clients reach a server: the two surfaces (browser TCP, agent Unix socket) and the tokens and routes of each, the runtime directory and its checks, discovery, reconnection after a restart or a crash, retries, and where a token may be written | `cadpole-cli/src/runtime.rs`, `server.rs`, `security.rs`, `ops_host.rs` |
| `session_progress.qnt` | The two temporal properties of `session.qnt` with their bound written out, under a scheduler that is fair within a bound | (a check on `session.qnt`) |

Every file has the same layout: constants, types, state, helpers, actions,
`step`, invariants, then scenarios.

- `step` is the intended behaviour. Random simulation runs `step` and checks
  the invariants listed for the model in `BUILD.bazel`.
- A `run` is a scenario: a fixed sequence of actions with expectations.
- Some models have a "not invariants" section: properties that do not hold,
  each with a scenario that reaches a state where it is false. They record
  what the interface does not guarantee.
- Some models have "scenario-only actions", which are not in `step`. They
  model behaviour the intended design excludes, so a scenario can show which
  invariant it breaks. In `revisions.qnt`, `eventlog.qnt` and `lifecycle.qnt`
  some invariants hold in `step` by construction, and these scenarios are what
  exercise them.
- `wakeups.qnt`, `listener.qnt`, `session_progress.qnt` and `transport.qnt` have a "mutants"
  section: `init` actions that start the model with one thing done wrong.
  `BUILD.bazel` lists each with the invariant that must catch it, and the
  tests simulate each mutant and fail if that invariant still holds. An
  invariant no mutant can break is not known to check anything.
- `wakeups.qnt` and `listener.qnt` keep their state in one record and write
  each action as a pure function of it, so that an action can be "this
  happens, and the program then runs until it blocks", which is the step a
  replay can make a real process take. Both record the action taken in a
  variable `did`, for the replay to read.

## Running the checks

Run from the repository root.

```sh
bazel test //spec/...                                         # quick tier, with the trace replays
bazel run  //spec:quint_check -- --samples=20000 --steps=60   # deep tier
bazel run  //spec:quint_check -- --seed=0x4a9e2638ec81e0b     # run a reported trace again
bazel run  //spec/replay:listener_replay -- --traces=1000 --steps=40   # a longer replay
bazel run  //spec/replay:wakeups_replay  -- --traces=400 --steps=40
bazel run  //spec/replay:transport_replay -- --traces=60 --steps=40
```

`spec/check.sh` runs the quick tier, the deep tier when `SAMPLES`, `STEPS`
or `SEED` is set, or the longer replays when `TRACES` is set.

The quick tier is five tests and is part of `bazel test //...`:

- `//spec:quint_test` checks each model for the `x' = a and b` precedence
  mistake, typechecks it, simulates 2,000 traces of up to 40 steps against its
  invariants, runs every scenario, and simulates every mutant, which must
  violate its invariant. It takes about three minutes.
- `//spec:names_test` compares the models with the code (below).
- `//spec/replay:listener_replay_test`, `//spec/replay:wakeups_replay_test`
  and `//spec/replay:transport_replay_test` replay model traces against the
  programs (below). Each takes under half a minute.

The deep tier runs the same checks with more and longer traces and takes a
few minutes. It is run by hand, as are the longer replays.

Quint is pinned in `package.json`, and its Rust evaluator is fetched by Bazel
with a checksum (`build_support/quint/evaluator.bzl`). The tests use no network
and need nothing installed.

## How the models are tied to the code

**Transcribed lists.** `//spec:names_test` (`names_test.mjs`) fails when a
model and the code disagree about:

- `capability.qnt`: `OPS`, `GRANTED` and `SPELLINGS` against `ops.proto` and
  `capability.rs`; `ENGINE_REQUESTS` against `engine.proto`; `CONNECT_PREFIX`
  against the proto package and service name.
- `lifecycle.qnt`: `HTTP_BOUNDS_TODAY` against the `heavy` match in `post_op`;
  `FFI_BOUNDS_TODAY` against `deadline_of`.
- `eventlog.qnt`: the event-log cap, the front drain and the SSE tail in
  `server.rs`.
- `revisions.qnt`: how each write handler reads `base_revision` and fills the
  response revision.
- `wakeups.qnt`: what each of the three readers treats as worth a model turn
  (`ListenFilter::Wake` in `main.rs`, `wakes` in `mcp.rs`, `isTurnWorthy` in
  `listener.mjs`), the kinds `cadpole listen` filters its long poll by, the
  client ids the cursors are stored under, and the `was` field of the
  `assistant_listening` event in `server.rs` and its three readers.
- `listener.qnt`: the minimum Claude Code version (`driver.mjs` and
  docs/assistant-integration.md), the `/new` text and the default of
  `--new-conversation-after`.
- For `wakeups.qnt`, `listener.qnt` and `session_progress.qnt`: every
  invariant in `allInvariants` and every mutant is listed in `BUILD.bazel`;
  each replay has a case for every action of its model; and an invariant
  that a comment in the code names (as `spec/<model>.qnt` and a name in
  backticks) exists in that model.

When one of these fails, change the model and the code together.

**Trace replay.** `replay/` ties three models to the programs by behaviour.
Quint simulates the model and writes each trace it walks as a list of states
(`--out-itf`); the replay makes the real program take the same steps and
compares what it shows with the model's state after every one, failing at
the first difference with both. The traces come from a fixed seed, so a
failure can be run again; `--seed`, `--traces` and `--steps` change them.

All three replays go from the model to the program. Every action of these models
is something a test can do to the program from outside, after which the
program runs by itself until it blocks, and where it blocks can be seen from
outside; so the program can be made to follow the model step by step. The
other direction, recording the programs under random schedules and checking
each recorded step against the model, would need the order of steps inside
several processes, which cannot be seen from outside.

- `//spec/replay:listener_replay_test` (`listener.qnt`). Real: the shipped
  entry point `main.mjs` as a process, so the version check, the loop, the
  driver and the shutdown. Played by the test over a loopback socket, one
  step at a time: the server (`fake_cadpole_mcp.mjs` passes every tool call
  to the test, which answers a long poll only when the model's poll returns)
  and the headless session (`fake_headless_cli.mjs` reports each message and
  writes its report, its result, or exits when told). Compared after every
  step: where the listener is blocked and the cursor it polls from, the
  cursor the server holds, every line in Chat, the session processes started
  and whether one is alive, and every message written to one. 48 traces of
  30 steps, 1,440 steps.
- `//spec/replay:wakeups_replay_test` (`wakeups.qnt`). Real: `cadpole serve`
  (restarted on the same address when the model's server restarts), the user
  through the owner's token, `cadpole listen --next` and `--follow` as
  processes, `cadpole mcp` as a process read by its channel notifications,
  and the listener's loop over a real `cadpole mcp --listener` with a driver
  that ends a turn when the model does. Compared after every step, per
  reader: whether it is running, every event it has handed over, the cursor
  the server holds for it, and for the listener its own cursor and the turn
  in flight. The traces come from the model's `stepReplay`, in which the
  watcher and the channel poll as soon as an event releases their long poll,
  as the processes do; every such step is one step of `step` followed by
  polls. 48 traces of 30 steps, 1,440 steps.

- `//spec/replay:transport_replay_test` (`transport.qnt`). Real: `cadpole
  serve` processes of two documents in a runtime directory of the test's
  own, started, stopped with `SIGTERM` and killed with `SIGKILL`; the
  directory's path replaced by a symlink and restored; a live socket with a
  record anyone could have written; long-lived `cadpole mcp` and `cadpole mcp
  --listener` clients started with no `--server` from a document's directory
  or neither; and requests straight at the TCP port and the socket with each
  kind of token. Compared after every step: which servers run, which of their
  files exist, each 0600 in a 0700 directory, whether each socket answers,
  what each tool call and request came to, which document a call reached,
  and that no token appears in any server's output, record, command line or
  anything `cadpole mcp` printed. The traces come from `stepReplay`, which
  starts a server only for a document with no live one: with two live
  servers of one document, which of them a reconnecting client's event pump
  settled on first is a matter of timing. 6 traces of 25 steps;
  `--init=<mutant>` replays a mutant's traces, which the programs must fail.
  The replay found that a `SIGTERM` sent right after start-up left a run's
  files behind (the handler was installed after the banner);
  `a_stop_right_after_the_banner_still_cleans_up` keeps the counterexample.
- `//src/rust/cadpole-cli:cadpole_cli_test`, the test
  `the_model_traces_replay_against_the_handler` (`reply_stream.qnt`).
  `//spec:reply_stream_traces` runs `quint run` on the model's `step` from a
  fixed seed and writes 300 traces of 30 steps (`traces.mjs`); the test makes
  each step as the call it stands for against `ServerHost::stream_reply_at`,
  under the model's limits and a clock the test moves. Compared after every
  one of the 9,000 steps: the outcome of the call, every message (author,
  state, text, activity), the `message` events in the log, the frames sent
  and the number of events `cadpole mcp`'s channel would act on. The test
  fails if the traces stop reaching any outcome the handler has (accepted,
  throttled, each refusal). The model's variable `last` says which call a
  step was.

What the replays leave to simulation: a poll of the watcher or the channel
at a moment of the model's choosing and either of them dying between a
hand-over and its ack (`polls`, `pollsAndDies` in `wakeups.qnt`); a reader
started on an empty log, and a server restarted under a watcher that has
not yet stored a cursor (nothing outside the process shows when it has
attached); a long poll that expires in `cadpole listen` or the channel
(thirty seconds, fixed); a listener killed rather than stopped; and a
session that replies and then exits before its result line. In
`transport.qnt`, a call delivered at a moment of the model's choosing and a
connection cut between running a call and answering it (`deliver`,
`breakAfterRunning`).

The other ten models are not replayed. `session.qnt`, `revisions.qnt`,
`eventlog.qnt`, `capability.qnt` and `lifecycle.qnt` describe the server
(and, for `lifecycle.qnt`, the C ABI), `deadlines.qnt` the C ABI,
`jobs.qnt`, `reload.qnt` and `hotswap.qnt` the web page, and `jobs.qnt`
also the Swift coalescer. They are tied to the code by the transcribed
lists above and by the tests in the table below.

**Tests named for invariants.** These tests hold the code to an invariant by
hand:

| Model | Invariant or action | Test |
| --- | --- | --- |
| `session.qnt` | `staleBaseRejected` | `a_stale_base_revision_is_refused_and_hands_back_the_current_source`, `the_generated_ops_answer_over_http` (`cadpole-cli/src/server.rs`) |
| `session.qnt` | `selectionValid` | `a_missing_feature_is_stale`, `a_lost_name_is_moved_by_fingerprint_or_stale`, `a_live_face_resolves_with_geometry_and_a_span` (`cadpole-engine/src/selection.rs`) |
| `session.qnt` | `ackClient`, `cursorBounded` | `ack_stores_a_cursor_and_flags_a_new_epoch` (`server.rs`) |
| `session.qnt` | `resumeClient` | `wait_events_resumes_from_the_stored_cursor_and_skips_the_callers_own_writes` (`server.rs`) |
| `eventlog.qnt` | `cursorNeverPassesTheLog` | `ack_stores_a_cursor_and_flags_a_new_epoch` (`server.rs`) |
| `capability.qnt` | `everyOperationIsClassified` | `every_operation_is_classified` (`cadpole-ops/src/capability.rs`) |
| `capability.qnt` | `theOwnerIsNotRestricted` | `the_owner_may_do_everything_and_the_listener_may_not` (`capability.rs`) |
| `capability.qnt` | `listenerNeverRunsADeniedOperation`, `theListenersEffectsAreASubsetOfItsGrants` | the 403 tests over HTTP in `server.rs` |
| `deadlines.qnt` | `callerIsNeverHeldPastTheDeadline`, `everyTimeoutIsRecognisable` | `the_deadline_releases_the_caller` (`cadpole-ffi/src/lib.rs`) |
| `jobs.qnt` | `burstIsBounded` | `ParamCoalescerTests.swift` |
| `wakeups.qnt` | `everyHandOverWasSentWhileListening`, `nothingActionableIsSkipped` | `a_run_judges_a_backlog_by_the_toggle_each_event_was_sent_under` (`cadpole-cli/src/main.rs`), `a_batch_is_judged_by_the_toggle_each_event_was_sent_under` (`mcp.rs`), `a_listen_toggle_event_says_what_the_toggle_was` (`server.rs`), "a backlog is judged by the toggle each event was sent under" (`listener_test.mjs`) |
| `wakeups.qnt` | `onlyActionableEventsAreHandedOver` | `the_channel_carries_only_events_worth_a_turn` (`mcp.rs`), "only a note or a message is worth a turn" (`listener_test.mjs`) |
| `wakeups.qnt` | `handedOverAtMostOnceUnlessInterrupted` | "the cursor is recorded after each event acted on" (`listener_test.mjs`) |
| `listener.qnt` | `nothingStartsAfterAStop`, `nothingIsDroppedSilently` | "stopping in the middle of a turn starts nothing more and leaves the batch for the next run" (`listener_test.mjs`) |
| `listener.qnt` | `afterARefusalNothingIsWritten`, `aRepeatedFailureIsToldOnce`, `noTurnWithoutAUsableBinary` | `driver_test.mjs`, `listener_test.mjs`, `cli_test.mjs` |
| `reply_stream.qnt` | `onlyTheAuthorWritesToItsMessage` | `only_the_client_that_started_a_streamed_message_may_continue_it` (`server.rs`) |
| `reply_stream.qnt` | `aFinishedMessageNeverChanges` | `a_finished_streamed_message_does_not_change` (`server.rs`) |
| `reply_stream.qnt` | `aStreamedMessageIsNeverFromTheUser` | `a_streamed_message_is_the_assistants_whoever_calls` (`server.rs`) |
| `reply_stream.qnt` | `theLimitsHold`, `aRefusedCallChangesNothing` | `a_streamed_reply_is_capped_and_rate_limited` (`server.rs`) |
| `reply_stream.qnt` | `everyMessageInProgressEndsInBoundedTime` | `an_abandoned_streamed_message_is_ended_as_failed` (`server.rs`) |
| `reply_stream.qnt` | `anUpdateIsNeverAnEvent`, `noStreamedMessageStartsATurn` | `the_pieces_of_a_streamed_reply_are_frames_and_only_its_end_is_an_event` (`server.rs`), `//src/tools/listener:listen_command_e2e_test` |
| `reply_stream.qnt` | `theTextChangesOnlyByAppending` | the replay above; on the page, `test/reply-stream.test.mjs` (`src/web`) |

| `transport.qnt` | `theWebUiIsNeverOnTheSocket`, `agentTokensStayOffTcpUnlessAllowed`, `aTokensDecisionNeverDependsOnTheSurface`, `theListenerNeverRunsADeniedOperation`, `aRequestWithoutAGoodTokenIsRefused` | `the_same_request_is_answered_the_same_on_both_surfaces`, `agent_tokens_stay_off_the_tcp_port_and_a_denial_is_the_same_everywhere`, `the_web_ui_and_browser_routes_are_not_served_on_the_socket` (`server.rs`) |
| `transport.qnt` | `callsGoOnlyToLiveServers`, `aClientNeverSwitchesDocuments`, `aClientRecoversWhenItsServerIsBack`, `cleanupNeverRemovesALiveServersFiles`, `tokensNeverLeak` | `a_client_with_no_flags_finds_follows_and_never_confuses_its_server` (`transport_e2e_tests.rs`), `discovery_finds_zero_one_or_two_servers_and_never_a_dead_one` (`runtime.rs`) |

The other invariants have no implementation test yet.

## Limits

- `reply_stream.qnt` models time as ticks and the rate limit as one call
  earned per tick; the replay maps a tick to one second. The activity line's
  size cap and the trimming of the message list to its last 500 are not
  modelled, and the page's handling of frames is tested in `src/web`, not
  modelled.
- The checks are random simulation, not exhaustive model checking. The two
  temporal properties in `session.qnt` (`catchUp`, `messagesReachTheAssistant`)
  are liveness properties, which simulation cannot check and `quint verify`
  is not run here. `session_progress.qnt` checks a bounded form of each: under
  a scheduler that delivers to a reader that is behind within a fixed number
  of states, a reader that stays connected is caught up, and a message is
  with a connected assistant, within a stated number of states. That is
  evidence for the properties, not a proof of them.
- A replay shows that the program does what the model does on the traces
  taken, with a fixed seed in the test sweep; it is a test, not a proof of
  refinement.
- State is bounded by constants such as `MAX_SEQ`, `MAX_REV` and `MAX_TIME`,
  chosen so a violation shows up before a bound is reached.
- Time is discrete: `deadlines.qnt` and `lifecycle.qnt` model the order of
  events, not durations. Reconnect backoff and retry are not modelled.
- Geometry is abstracted away: a script is the set of feature ids it yields.
- `jobs.qnt` has one result per job; progressive results are not modelled.
- `session.qnt` has one UI client; add a name to `CLIENTS` for a second tab.
- The event log is in memory; a persisted log is not modelled.
- `wakeups.qnt` has the whole log (the window is `eventlog.qnt`'s), so the
  `events_truncated` line a reader prints when it fell out of the window is
  not modelled; one reader of each kind; and a restart of the server always
  changes the token, as `cadpole serve` does.
- `listener.qnt` takes a turn's answer and its result line as one step, and
  has the listener stopped but never killed; what a killed reader repeats is
  `wakeups.qnt`'s `handedOverAtMostOnceUnlessInterrupted`.
