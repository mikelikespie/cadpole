// A reply posted a piece at a time: `StreamReply`.
//
// A client starts a message, appends text to it and ends it as done or
// failed; the page shows it growing. The rules are in
// src/rust/cadpole-cli/src/reply_stream.rs and the handler that applies them
// is `ServerHost::stream_reply_at` in server.rs. This model states them:
//
//   * only the client that started a message writes to it;
//   * a message that is done or failed never changes;
//   * the server attributes it to the assistant, never to the user;
//   * its text changes only by appending: the author's text, or the one
//     closing line the server adds when it ends the message itself;
//   * the text a caller may append is capped, calls past a burst are
//     answered `throttled`, a client has one message in progress and a
//     session a few;
//   * a message nobody feeds is ended by the server, so none stays in
//     progress for more than a bounded time;
//   * an append is not a session event. The log gains one `message` event
//     when a streamed message ends, and that event starts no turn in a
//     watcher, which acts only on a message from the user.
//
// `step` is the server as built, so the invariants hold in it. The
// scenario-only actions at the end each leave one rule out, and a scenario
// shows which invariant notices.
//
// The traces of `step` are replayed against the handler by
// `the_model_traces_replay_against_the_handler` (server.rs), which reads
// `last` to know what to call and compares the messages, the log and the
// outcome after every step. The constants below are the limits that test
// runs the handler under; `cadpole serve` runs with larger ones
// (`Limits::default`).

module reply_stream {
  // ---- constants

  // Who may call. Three, so that the session's cap on messages in progress
  // (two here) can be reached with one in progress each.
  pure val CLIENTS: Set[str] = Set("listener", "second", "third")

  // What a call may append: nothing, or one byte.
  pure val CHUNKS: Set[str] = Set("", "a", "b")
  pure val ACTIVITIES: Set[str] = Set("", "rendering")
  // How a call leaves the message: in progress, done or failed.
  pure val FINISHES: Set[str] = Set("", "done", "failed")

  pure val MAX_TEXT: int = 3      // bytes a caller may append to one message
  pure val BURST: int = 2         // calls accepted back to back
  pure val IDLE_TICKS: int = 2    // ticks without an accepted call before the server ends it
  pure val LIFE_TICKS: int = 5    // ticks a message may stay in progress at all
  pure val MAX_OPEN: int = 2      // messages in progress in the session
  pure val MAX_MSGS: int = 9      // bounds the trace

  // The server's closing line, as one element of a text.
  pure val NOTE: str = "!"

  // ---- types

  type Msg = {
    from: str,        // "user" or "assistant"
    author: str,      // the client that started it; "" for a message posted whole
    state: str,       // "" (posted whole), "streaming", "done" or "failed"
    text: List[str],
    activity: str,
    appended: int,    // bytes the author appended
    idle: int,        // ticks since the last accepted call
    age: int,         // ticks since it started
    tokens: int,      // calls it may still accept before a tick
  }

  type Call = {
    op: str,      // "init", "user_post", "start", "update" or "tick"
    client: str,
    seq: int,         // the message named by the call; for "start", the one created
    chunk: str,
    activity: str,
    finish: str,
    outcome: str,     // "ok", "throttled", "not_in_progress", "not_author", "too_long", "too_many"
  }

  // ---- state

  var msgs: int -> Msg                          // the session's message list, by seq
  var nextSeq: int
  var log: List[{ seq: int, from: str }]        // the `message` events in the event log
  var wakes: int                                // turns started in a watcher of the log
  var last: Call                                // the step just taken
  var prev: int -> Msg                          // `msgs` before it
  var prevLog: int                              // the length of `log` before it

  // ---- helpers

  pure def bytes(chunk: str): int = if (chunk == "") 0 else 1
  pure def withChunk(text: List[str], chunk: str): List[str] =
    if (chunk == "") text else text.append(chunk)
  pure def atMost(a: int, b: int): int = if (a < b) a else b

  def inProgress(s: int): bool = msgs.keys().contains(s) and msgs.get(s).state == "streaming"
  val open: Set[int] = msgs.keys().filter(s => inProgress(s))

  /// The server ends a message itself: failed, with its closing line.
  pure def endedByTheServer(m: Msg): Msg =
    { ...m, state: "failed", activity: "", text: m.text.append(NOTE) }

  /// The events owed to the messages in `ended`, oldest first.
  pure def announce(l: List[{ seq: int, from: str }], ended: Set[int]): List[{ seq: int, from: str }] =
    range(1, MAX_MSGS + 1).foldl(l, (acc, s) =>
      if (ended.contains(s)) acc.append({ seq: s, from: "assistant" }) else acc)

  /// What a watcher of the log acts on: `wakes` in cadpole-cli/src/mcp.rs and
  /// the filter of `cadpole listen --next`.
  pure def startsATurn(e: { seq: int, from: str }): bool = e.from == "user"

  action record(c: Call): bool = all {
    last' = c,
    prev' = msgs,
    prevLog' = log.length(),
  }

  /// A call that is refused, or throttled, changes nothing.
  action unchanged(c: Call): bool = all {
    msgs' = msgs, nextSeq' = nextSeq, log' = log, wakes' = wakes,
    record(c),
  }

  // ---- actions

  action init = all {
    msgs' = Map(),
    nextSeq' = 1,
    log' = [],
    wakes' = 0,
    last' = { op: "init", client: "", seq: 0, chunk: "", activity: "", finish: "", outcome: "ok" },
    prev' = Map(),
    prevLog' = 0,
  }

  /// The user types a message (`PostMessage`): one event, and a watcher acts
  /// on it.
  action userPost = all {
    nextSeq <= MAX_MSGS,
    msgs' = msgs.put(nextSeq, {
      from: "user", author: "", state: "", text: ["u"], activity: "",
      appended: 0, idle: 0, age: 0, tokens: 0,
    }),
    nextSeq' = nextSeq + 1,
    log' = log.append({ seq: nextSeq, from: "user" }),
    wakes' = wakes + 1,
    record({ op: "user_post", client: "", seq: nextSeq, chunk: "", activity: "", finish: "", outcome: "ok" }),
  }

  /// `StreamReply` with `seq` 0: a new message. The caller's earlier message,
  /// if one is still in progress, is ended by the server first.
  action start(c: str, chunk: str, activity: str, finish: str): bool = {
    val call = { op: "start", client: c, seq: 0, chunk: chunk, activity: activity, finish: finish, outcome: "ok" }
    val mine = open.filter(s => msgs.get(s).author == c)
    val others = open.exclude(mine)
    val closed = mine.fold(msgs, (acc, s) => acc.put(s, endedByTheServer(acc.get(s))))
    val fresh = {
      from: "assistant", author: c,
      state: if (finish == "") "streaming" else finish,
      text: withChunk([], chunk),
      activity: if (finish == "") activity else "",
      appended: bytes(chunk), idle: 0, age: 0, tokens: BURST,
    }
    all {
      nextSeq <= MAX_MSGS,
      if (others.size() >= MAX_OPEN)
        unchanged({ ...call, outcome: "too_many" })
      else all {
        msgs' = closed.put(nextSeq, fresh),
        nextSeq' = nextSeq + 1,
        log' = announce(log, if (finish == "") mine else mine.union(Set(nextSeq))),
        wakes' = wakes,
        record({ ...call, seq: nextSeq }),
      },
    }
  }

  /// `StreamReply` naming a message: append, set the activity line, maybe
  /// end it. The checks run in this order.
  action update(c: str, s: int, chunk: str, activity: str, finish: str): bool = {
    val call = { op: "update", client: c, seq: s, chunk: chunk, activity: activity, finish: finish, outcome: "ok" }
    if (not(inProgress(s)))
      unchanged({ ...call, outcome: "not_in_progress" })
    else {
      val m = msgs.get(s)
      if (m.author != c)
        unchanged({ ...call, outcome: "not_author" })
      else if (m.appended + bytes(chunk) > MAX_TEXT)
        unchanged({ ...call, outcome: "too_long" })
      else if (finish == "" and m.tokens == 0)
        unchanged({ ...call, outcome: "throttled" })
      else {
        val grown = {
          ...m,
          text: withChunk(m.text, chunk),
          appended: m.appended + bytes(chunk),
          idle: 0,
        }
        all {
          msgs' = msgs.put(s,
            if (finish == "") { ...grown, activity: activity, tokens: m.tokens - 1 }
            else { ...grown, activity: "", state: finish }),
          nextSeq' = nextSeq,
          // The call that ends the message is the only one that is an event.
          log' = if (finish == "") log else announce(log, Set(s)),
          wakes' = wakes,
          record(call),
        }
      }
    }
  }

  /// Time passes: every message in progress earns a call back and gets
  /// older, and the server ends the ones that are due.
  action tick = {
    val aged = msgs.keys().mapBy(s => {
      val m = msgs.get(s)
      if (m.state == "streaming")
        { ...m, idle: m.idle + 1, age: m.age + 1, tokens: atMost(m.tokens + 1, BURST) }
      else m
    })
    val due = open.filter(s => aged.get(s).idle >= IDLE_TICKS or aged.get(s).age >= LIFE_TICKS)
    all {
      msgs' = aged.keys().mapBy(s => if (due.contains(s)) endedByTheServer(aged.get(s)) else aged.get(s)),
      nextSeq' = nextSeq,
      log' = announce(log, due),
      wakes' = wakes,
      record({ op: "tick", client: "", seq: 0, chunk: "", activity: "", finish: "", outcome: "ok" }),
    }
  }

  /// The author continues one of the messages in progress, more often than
  /// not without ending it. `update` alone would mostly name a message that
  /// is not there.
  action feed = {
    nondet s = oneOf(open.union(Set(0)))
    nondet chunk = oneOf(CHUNKS)
    nondet activity = oneOf(ACTIVITIES)
    nondet finish = oneOf(FINISHES)
    nondet goesOn = oneOf(Set(true, false))
    all {
      s != 0,
      update(msgs.get(s).author, s, chunk, activity, if (goesOn) "" else finish),
    }
  }

  action step = any {
    userPost,
    tick,
    feed,
    nondet c = oneOf(CLIENTS)
    nondet chunk = oneOf(CHUNKS)
    nondet activity = oneOf(ACTIVITIES)
    nondet finish = oneOf(FINISHES)
    start(c, chunk, activity, finish),
    nondet c = oneOf(CLIENTS)
    nondet s = oneOf(1.to(MAX_MSGS))
    nondet chunk = oneOf(CHUNKS)
    nondet activity = oneOf(ACTIVITIES)
    nondet finish = oneOf(FINISHES)
    update(c, s, chunk, activity, finish),
  }

  // ---- invariants

  def streamed(s: int): bool = msgs.get(s).author != ""
  def eventsFor(s: int): int = log.select(e => e.seq == s).length()
  val before: Set[int] = prev.keys()

  /// Only the client that started a message writes to it. The server's own
  /// changes come with a tick, or with the author starting its next message.
  val onlyTheAuthorWritesToItsMessage: bool =
    last.op == "tick" or
      before.forall(s => msgs.get(s) != prev.get(s) implies prev.get(s).author == last.client)

  /// Done or failed is final, and a message posted whole is never touched.
  val aFinishedMessageNeverChanges: bool =
    before.forall(s => prev.get(s).state != "streaming" implies msgs.get(s) == prev.get(s))

  /// A streamed message, and the event that announces it, is the assistant's.
  val aStreamedMessageIsNeverFromTheUser: bool = and {
    msgs.keys().forall(s => streamed(s) implies msgs.get(s).from == "assistant"),
    log.indices().forall(i => streamed(log[i].seq) implies log[i].from == "assistant"),
  }

  /// The text of a message changes in two ways only: the author's call
  /// appends its chunk, or the server ends the message and appends its
  /// closing line.
  val theTextChangesOnlyByAppending: bool =
    before.forall(s => {
      val was = prev.get(s)
      val now = msgs.get(s)
      or {
        now.text == was.text,
        and {
          last.op == "update", last.seq == s, last.outcome == "ok", last.chunk != "",
          now.text == was.text.append(last.chunk),
        },
        and {
          was.state == "streaming", now.state == "failed",
          now.text == was.text.append(NOTE),
        },
      }
    })

  /// The caps: on the appended text, on the calls a message accepts between
  /// ticks, on the messages in progress per client and per session. The
  /// activity line belongs to a message in progress and to no other.
  val theLimitsHold: bool = and {
    msgs.keys().forall(s => {
      val m = msgs.get(s)
      and {
        m.appended <= MAX_TEXT,
        m.text.select(t => t != NOTE).length() == (if (streamed(s)) m.appended else 1),
        m.text.select(t => t == NOTE).length() <= 1,
        m.tokens >= 0, m.tokens <= BURST,
        m.state != "streaming" implies m.activity == "",
      }
    }),
    open.size() <= MAX_OPEN,
    CLIENTS.forall(c => open.filter(s => msgs.get(s).author == c).size() <= 1),
  }

  /// No message stays in progress: one that is not fed for IDLE_TICKS, or is
  /// LIFE_TICKS old, has been ended.
  val everyMessageInProgressEndsInBoundedTime: bool =
    open.forall(s => msgs.get(s).idle < IDLE_TICKS and msgs.get(s).age < LIFE_TICKS)

  /// An append is not an event: the log grows only by a message the user
  /// posted or by a streamed message ending, once each.
  val anUpdateIsNeverAnEvent: bool = and {
    msgs.keys().forall(s => eventsFor(s) == (if (inProgress(s)) 0 else 1)),
    log.length() - prevLog ==
      before.filter(s => prev.get(s).state == "streaming" and msgs.get(s).state != "streaming").size()
        + msgs.keys().exclude(before).filter(s => not(inProgress(s))).size(),
  }

  /// Nothing about a streamed message starts a turn in a watcher of the log.
  val noStreamedMessageStartsATurn: bool = and {
    wakes == msgs.keys().filter(s => msgs.get(s).from == "user").size(),
    log.indices().forall(i => startsATurn(log[i]) implies not(streamed(log[i].seq))),
  }

  /// A call that is refused or throttled leaves everything as it was.
  val aRefusedCallChangesNothing: bool =
    last.outcome != "ok" implies (msgs == prev and log.length() == prevLog)

  val allInvariants: bool = and {
    onlyTheAuthorWritesToItsMessage,
    aFinishedMessageNeverChanges,
    aStreamedMessageIsNeverFromTheUser,
    theTextChangesOnlyByAppending,
    theLimitsHold,
    everyMessageInProgressEndsInBoundedTime,
    anUpdateIsNeverAnEvent,
    noStreamedMessageStartsATurn,
    aRefusedCallChangesNothing,
  }

  // ---- scenario-only actions
  //
  // Not in `step`. Each is the handler with one rule left out.

  /// The append of `update` with no check of who is calling, or of whether
  /// the message has ended.
  action appendUnchecked(c: str, s: int, chunk: str): bool = all {
    msgs.keys().contains(s),
    msgs' = msgs.put(s, { ...msgs.get(s), text: msgs.get(s).text.append(chunk) }),
    nextSeq' = nextSeq, log' = log, wakes' = wakes,
    record({ op: "update", client: c, seq: s, chunk: chunk, activity: "", finish: "", outcome: "ok" }),
  }

  /// A message started under the name the caller asks for.
  action startAs(c: str, from: str): bool = all {
    msgs' = msgs.put(nextSeq, {
      from: from, author: c, state: "streaming", text: [], activity: "",
      appended: 0, idle: 0, age: 0, tokens: BURST,
    }),
    nextSeq' = nextSeq + 1, log' = log, wakes' = wakes,
    record({ op: "start", client: c, seq: nextSeq, chunk: "", activity: "", finish: "", outcome: "ok" }),
  }

  /// Time passes and nothing looks for messages that are due.
  action tickWithoutExpiry = all {
    msgs' = msgs.keys().mapBy(s => {
      val m = msgs.get(s)
      if (m.state == "streaming") { ...m, idle: m.idle + 1, age: m.age + 1 } else m
    }),
    nextSeq' = nextSeq, log' = log, wakes' = wakes,
    record({ op: "tick", client: "", seq: 0, chunk: "", activity: "", finish: "", outcome: "ok" }),
  }

  /// An append that is also written to the event log, as the user's
  /// messages are, and so wakes a watcher.
  action appendAsAnEvent(c: str, s: int, chunk: str): bool = all {
    inProgress(s),
    msgs.get(s).author == c,
    msgs' = msgs.put(s, { ...msgs.get(s), text: msgs.get(s).text.append(chunk), appended: msgs.get(s).appended + 1 }),
    nextSeq' = nextSeq,
    log' = log.append({ seq: s, from: "user" }),
    wakes' = wakes + 1,
    record({ op: "update", client: c, seq: s, chunk: chunk, activity: "", finish: "", outcome: "ok" }),
  }

  // ---- scenarios

  // The ordinary reply: started, grown twice with an activity line in
  // between, ended. One event, at the end, and no turn started.
  run aReplyGrowsAndSettles =
    init
      .then(userPost)
      .expect(wakes == 1 and log.length() == 1)
      .then(start("listener", "", "rendering", ""))
      .expect(msgs.get(2).state == "streaming" and msgs.get(2).activity == "rendering")
      .then(update("listener", 2, "a", "", ""))
      .then(tick)
      .then(update("listener", 2, "b", "", "done"))
      .expect(msgs.get(2).text == ["a", "b"] and msgs.get(2).state == "done")
      .expect(log == [{ seq: 1, from: "user" }, { seq: 2, from: "assistant" }])
      .expect(wakes == 1)
      .expect(allInvariants)

  // Another client is refused, and so is the author once the message ended.
  run onlyTheAuthorAndOnlyUntilTheEnd =
    init
      .then(start("listener", "a", "", ""))
      .then(update("second", 1, "b", "", ""))
      .expect(last.outcome == "not_author" and msgs.get(1).text == ["a"])
      .then(update("second", 1, "", "", "done"))
      .expect(last.outcome == "not_author" and msgs.get(1).state == "streaming")
      .then(update("listener", 1, "", "", "failed"))
      .expect(msgs.get(1).state == "failed")
      .then(update("listener", 1, "b", "", ""))
      .expect(last.outcome == "not_in_progress" and msgs.get(1).text == ["a"])
      .expect(allInvariants)

  // The caps: a third call without a tick is throttled and a tick earns one
  // back; text past the cap is refused, and the message can still be ended.
  run theBurstAndTheSizeCap =
    init
      .then(start("listener", "a", "", ""))
      .then(update("listener", 1, "a", "", ""))
      .then(update("listener", 1, "a", "", ""))
      .expect(msgs.get(1).tokens == 0 and msgs.get(1).appended == 3)
      .then(update("listener", 1, "", "", ""))
      .expect(last.outcome == "throttled")
      .then(tick)
      .then(update("listener", 1, "b", "", ""))
      .expect(last.outcome == "too_long")
      .then(update("listener", 1, "", "", "done"))
      .expect(msgs.get(1).state == "done" and msgs.get(1).text == ["a", "a", "a"])
      .expect(allInvariants)

  // An author that goes away: the server ends the message, with its line,
  // and says so in one event.
  run anAbandonedMessageIsEndedByTheServer =
    init
      .then(start("listener", "a", "rendering", ""))
      .then(tick)
      .expect(msgs.get(1).state == "streaming")
      .then(tick)
      .expect(msgs.get(1).state == "failed" and msgs.get(1).text == ["a", NOTE])
      .expect(msgs.get(1).activity == "" and log == [{ seq: 1, from: "assistant" }])
      .expect(allInvariants)

  // An author that keeps feeding its message is still bounded by its age.
  run aMessageFedForEverIsEndedToo =
    init
      .then(start("listener", "", "", ""))
      .then(tick).then(update("listener", 1, "", "", ""))
      .then(tick).then(update("listener", 1, "", "", ""))
      .then(tick).then(update("listener", 1, "", "", ""))
      .then(tick).then(update("listener", 1, "", "", ""))
      .expect(msgs.get(1).state == "streaming" and msgs.get(1).age == 4)
      .then(tick)
      .expect(msgs.get(1).state == "failed")
      .expect(allInvariants)

  // One in progress per client, a few per session.
  run oneInProgressPerClient =
    init
      .then(start("listener", "a", "", ""))
      .then(start("listener", "b", "", ""))
      .expect(msgs.get(1).state == "failed" and msgs.get(1).text == ["a", NOTE])
      .expect(msgs.get(2).state == "streaming")
      .then(start("second", "", "", ""))
      .then(start("third", "", "", ""))
      .expect(last.outcome == "too_many" and nextSeq == 4)
      .expect(allInvariants)

  // Without the author check, another client's text lands in the message.
  run withoutTheAuthorCheckAnyoneWrites =
    init
      .then(start("listener", "a", "", ""))
      .then(appendUnchecked("second", 1, "b"))
      .expect(not(onlyTheAuthorWritesToItsMessage))

  // Without the check for the end, a finished message changes.
  run withoutTheEndCheckAFinishedMessageChanges =
    init
      .then(start("listener", "a", "", "done"))
      .then(appendUnchecked("listener", 1, "b"))
      .expect(not(aFinishedMessageNeverChanges))

  // With the caller choosing the name, a streamed message is the user's.
  run withACallerChosenNameItSpeaksAsTheUser =
    init
      .then(startAs("listener", "user"))
      .expect(not(aStreamedMessageIsNeverFromTheUser))

  // Without the expiry, an abandoned message stays in progress.
  run withoutTheExpiryAMessageStaysInProgress =
    init
      .then(start("listener", "", "rendering", ""))
      .then(tickWithoutExpiry)
      .then(tickWithoutExpiry)
      .expect(msgs.get(1).state == "streaming")
      .expect(not(everyMessageInProgressEndsInBoundedTime))

  // With appends in the log, each one is an event and starts a turn.
  run withAppendsInTheLogEveryPieceWakesAWatcher =
    init
      .then(start("listener", "", "", ""))
      .then(appendAsAnEvent("listener", 1, "a"))
      .expect(not(anUpdateIsNeverAnEvent))
      .expect(not(noStreamedMessageStartsATurn))
}
