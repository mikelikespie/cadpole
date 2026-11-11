// The limited listener's session lifecycle.
//
// `//src/tools/listener` answers Chat messages through one long-lived
// headless session per conversation (docs/assistant-integration.md, "The
// limited listener"). The model covers one listener and what it starts:
//
//   * the version check before anything is attached;
//   * the loop: one long poll, then the batch in order, one turn at a time;
//   * the session process: started on the first message, replaced after
//     `/new` or a number of messages, gone when it exits;
//   * the check of what the session reports at the start of every turn, and
//     the refusal that follows a bad report;
//   * how a turn ends: an answer, a failure told once per distinct failure,
//     the process exiting in the middle. The answer is a reply written a
//     piece at a time (`reply_stream.qnt`); here it is one line in Chat.
//     A failure after the reply has started always ends that reply with its
//     line; the failures modelled here come before the model has written
//     anything, which is when "told once" applies;
//   * stopping the listener, and the next run resuming from the server's
//     cursor.
//
// The listener is one event loop, so between two things that happen to it,
// it runs until it blocks: in the long poll, or on the session's next line.
// An action here is one such thing followed by everything the listener does
// until it blocks again (`settle`). `step` and the replay against the real
// program (spec/replay) use the same actions.
//
// What starts a turn at all, and the cursor, are `wakeups.qnt`'s subject and
// are repeated here only as far as the lifecycle needs them.

module listener {
  // ---- constants

  /// Events posted in one trace.
  pure val MAX_EVENTS: int = 7

  // What the model transcribes from the listener; //spec:names_test checks
  // each against the code and the documentation.

  /// The oldest Claude Code the listener runs turns through (`Usable` is
  /// this or later, `TooOld` anything before it).
  pure val MIN_CLAUDE_VERSION: str = "2.1.259"

  /// The message that starts a new conversation (`NewConversation`).
  pure val NEW_CONVERSATION_TEXT: str = "/new"

  /// The default of --new-conversation-after; the model draws 0 (never) or
  /// a small number so that a replacement is reachable in a short trace.
  pure val DEFAULT_NEW_CONVERSATION_AFTER: int = 30

  /// The reasons a turn fails, as the user is told them. Two failures are
  /// "the same" when the sentence is the same.
  pure val REASONS: Set[str] = Set("not signed in", "error", "turn limit", "exited", "refused")

  // ---- types

  type Kind =
    | Request           // a message from the user
    | NewConversation   // the message `/new`
    | Noise             // anything that is not worth a turn
    | ListenOn          // the UI's Listen toggle
    | ListenOff

  /// `on` is the Listen toggle the event was sent under (for a toggle, the
  /// new value); `was` is the toggle before it.
  type Event = { seq: int, kind: Kind, on: bool, was: bool }

  type Binary = Usable | TooOld | Missing

  /// Where the listener is blocked. `Batch` is never a resting state.
  type Proc = Down | Waiting | Batch | InTurn

  type Stage = Written | Checked

  type Flight = { open: bool, seq: int, stage: Stage }

  type Child = { alive: bool, turns: int }

  /// A line in the Chat panel that is not the user's.
  type Line =
    | Answer(int)                             // the session's reply to this seq
    | Notice({ seq: int, reason: str })       // "The listener could not finish this: ..."
    | Fresh(int)                              // "Started a new conversation ..."

  /// One line written to a session's stdin, with what held at that moment.
  type Write = {
    runNo: int,              // the listener run that wrote it
    child: int,            // which session process, counted from 1
    seq: int,
    sentWhileOn: bool,     // the event's own `on`
    turnWasOpen: bool,     // another turn was in flight
    afterRefusal: bool,    // this run had refused a session
    afterStop: bool,       // the listener was stopping
  }

  /// How a turn ended, in the order turns ended.
  type Outcome = { runNo: int, seq: int, ending: str, told: bool, checked: bool }

  type State = {
    // the server
    log: List[Event],
    listenOn: bool,
    stored: int,                 // the cursor the server keeps for this listener
    chat: List[Line],
    // the listener
    binary: Binary,
    every: int,                  // --new-conversation-after; 0 for never
    alwaysListen: bool,                // --always-listen
    bug: str,                    // "" in the intended design; see the mutants
    runNo: int,
    proc: Proc,
    cursor: int,
    hears: bool,                 // the toggle as the listener knows it
    inbox: List[Event],
    batchEnd: int,
    lastFailure: str,
    refused: bool,
    // the session process
    child: Child,
    children: int,
    flight: Flight,
    // history
    joined: int,                 // where a first run adopted the end of the log
    writes: List[Write],
    outcomes: List[Outcome],
    boundaries: List[bool],      // per replaced conversation: was a turn in flight?
    interrupted: Set[int],       // turns cut short by stopping the listener
    stops: int,                  // how many times that happened
    badReports: int,             // sessions that reported more than the Cadpole tools
    fatals: int,                 // starts refused by the version check
  }

  /// What the last action was, for the replay.
  type Did = { name: str, kind: str, ok: bool, ending: str }

  // ---- state

  var s: State
  var did: Did

  // ---- helpers

  pure val NO_FLIGHT: Flight = { open: false, seq: 0, stage: Written }

  pure def lastSeq(st: State): int = st.log.length()

  pure def actionable(k: Kind): bool = k == Request or k == NewConversation

  pure def isToggle(k: Kind): bool = k == ListenOn or k == ListenOff

  pure def maxOf(a: int, b: int): int = if (a > b) a else b

  /// `reached` in listener.mjs: record the position, on the server too.
  pure def reached(st: State, seq: int): State =
    if (seq > st.cursor) { ...st, cursor: seq, stored: maxOf(st.stored, seq) } else st

  /// The conversation's process is ended and the next message starts another.
  pure def endConversation(st: State): State =
    if (st.child.alive)
      { ...st, child: { alive: false, turns: 0 }, boundaries: st.boundaries.append(st.flight.open) }
    else st

  /// A turn failed: the user is told, once per distinct failure in a row.
  pure def failTurn(st: State, seq: int, reason: str, checked: bool): State = {
    val tell = st.bug == "tellsEveryFailure" or reason != st.lastFailure
    val told = {
      ...st,
      lastFailure: reason,
      chat: if (tell) st.chat.append(Notice({ seq: seq, reason: reason })) else st.chat,
      outcomes: st.outcomes.append({ runNo: st.runNo, seq: seq, ending: reason, told: tell, checked: checked }),
      flight: NO_FLIGHT,
      proc: Batch,
    }
    if (st.bug == "forgetsAFailedTurn") told else reached(told, seq)
  }

  /// `driver.ask`: write the message to the session, starting or replacing
  /// the process first when there is none or the conversation is full.
  pure def ask(st: State, ev: Event, stopping: bool): State =
    if (st.refused and st.bug != "asksAgainAfterARefusal")
      failTurn(st, ev.seq, "refused", false)
    else {
      val full = and {
        st.child.alive, st.every > 0, st.child.turns >= st.every,
        st.bug != "neverReplacesAConversation",
      }
      val ended = if (full) endConversation(st) else st
      val started = not(ended.child.alive)
      val children = if (started) ended.children + 1 else ended.children
      {
        ...ended,
        child: { alive: true, turns: ended.child.turns + 1 },
        children: children,
        flight: { open: true, seq: ev.seq, stage: Written },
        writes: ended.writes.append({
          runNo: st.runNo,
          child: children,
          seq: ev.seq,
          sentWhileOn: ev.on,
          turnWasOpen: st.flight.open,
          afterRefusal: st.refused,
          afterStop: stopping,
        }),
        proc: if (st.bug == "doesNotWaitForTheTurn") Batch else InTurn,
      }
    }

  /// The next event of the batch, as `Listener.step` treats it.
  pure def handle(st: State, stopping: bool): State = {
    val ev = st.inbox.head()
    val rest = { ...st, inbox: st.inbox.tail() }
    if (isToggle(ev.kind)) { ...rest, hears: ev.on }
    else if (not(actionable(ev.kind))) rest
    else if (not(rest.hears) and not(rest.alwaysListen)) rest
    else if (ev.kind == NewConversation) {
      val fresh = endConversation(rest)
      reached({ ...fresh, chat: fresh.chat.append(Fresh(ev.seq)) }, ev.seq)
    }
    else ask(rest, ev, stopping)
  }

  /// Run the listener until it blocks: on a session, or in the next poll.
  pure def settleWith(st: State, stopping: bool): State =
    range(0, MAX_EVENTS + 1).foldl(st, (acc, _) =>
      if (acc.proc != Batch) acc
      else if (acc.inbox.length() == 0)
        // The batch is done: the cursor covers everything examined.
        { ...reached(acc, acc.batchEnd), proc: Waiting }
      else handle(acc, stopping))

  pure def settle(st: State): State = settleWith(st, false)

  pure def initial(binary: Binary, every: int, alwaysListen: bool, listenOn: bool, bug: str): State = {
    log: [],
    listenOn: listenOn,
    stored: 0,
    chat: [],
    binary: binary,
    every: every,
    alwaysListen: alwaysListen,
    bug: bug,
    runNo: 0,
    proc: Down,
    cursor: 0,
    hears: false,
    inbox: [],
    batchEnd: 0,
    lastFailure: "",
    refused: false,
    child: { alive: false, turns: 0 },
    children: 0,
    flight: NO_FLIGHT,
    joined: 0,
    writes: [],
    outcomes: [],
    boundaries: [],
    interrupted: Set(),
    stops: 0,
    badReports: 0,
    fatals: 0,
  }

  pure def note(name: str): Did = { name: name, kind: "", ok: true, ending: "" }

  // ---- actions

  action initWith(bug: str): bool = {
    nondet binary = Set(Usable, TooOld, Missing).oneOf()
    nondet every = Set(0, 2).oneOf()
    nondet alwaysListen = Set(false, true).oneOf()
    nondet listenOn = Set(false, true).oneOf()
    all {
      s' = initial(binary, every, alwaysListen, listenOn, bug),
      did' = note("init"),
    }
  }

  action init = initWith("")

  /// The same with a usable Claude Code, which is where the lifecycle is.
  action initUsable = {
    nondet every = Set(0, 2).oneOf()
    nondet alwaysListen = Set(false, true).oneOf()
    nondet listenOn = Set(false, true).oneOf()
    all {
      s' = initial(Usable, every, alwaysListen, listenOn, ""),
      did' = note("init"),
    }
  }

  /// The user does something in the UI.
  action userDoes(kind: Kind): bool = {
    val on = if (kind == ListenOn) true else if (kind == ListenOff) false else s.listenOn
    all {
      lastSeq(s) < MAX_EVENTS,
      s' = {
        ...s,
        log: s.log.append({ seq: lastSeq(s) + 1, kind: kind, on: on, was: s.listenOn }),
        listenOn: on,
      },
      did' = {
        ...note("userDoes"),
        kind:
          match kind {
            | Request => "request"
            | NewConversation => "new"
            | Noise => "noise"
            | ListenOn => "listen-on"
            | ListenOff => "listen-off"
          },
      },
    }
  }

  action user = {
    nondet pick = 1.to(10).oneOf()
    userDoes(
      if (pick <= 5) Request
      else if (pick == 6) NewConversation
      else if (pick <= 8) Noise
      else if (pick == 9) ListenOn
      else ListenOff)
  }

  /// The listener is started: the version check, then `resume`.
  action launch = all {
    s.proc == Down,
    if (s.binary != Usable and s.bug != "skipsTheVersionCheck")
      s' = { ...s, fatals: s.fatals + 1 }
    else {
      val fresh = s.stored == 0
      val at = if (fresh) lastSeq(s) else s.stored
      s' = {
        ...s,
        runNo: s.runNo + 1,
        proc: Waiting,
        cursor: at,
        // A new client adopts the end of the log, and says so at once.
        stored: at,
        joined: if (fresh) at else s.joined,
        hears: s.listenOn,
        inbox: [],
        batchEnd: at,
        lastFailure: "",
        refused: false,
        child: { alive: false, turns: 0 },
        flight: NO_FLIGHT,
      }
    },
    did' = note("launch"),
  }

  /// The long poll returns: what the log holds after the cursor, possibly
  /// nothing. The listener then works through the batch.
  action pollReturns = {
    val batch = s.log.select(e => e.seq > s.cursor)
    val changes = batch.select(e => isToggle(e.kind))
    // The first change in the batch says what the toggle was for the events
    // in front of it.
    val hears =
      if (changes.length() > 0 and s.bug != "trustsTheToggleAsItIsNow") changes[0].was else s.hears
    all {
      s.proc == Waiting,
      s' = settle({
        ...s,
        inbox: batch,
        batchEnd: if (batch.length() > 0) batch[batch.length() - 1].seq else s.cursor,
        hears: hears,
        proc: Batch,
      }),
      did' = note("pollReturns"),
    }
  }

  /// The session's first line of a turn: what it can reach. Anything beyond
  /// the Cadpole tools and the process is killed and the driver refuses from
  /// then on.
  action sessionReports(ok: bool): bool = all {
    s.flight.open,
    s.flight.stage == Written,
    s.child.alive,
    if (ok)
      s' = { ...s, flight: { ...s.flight, stage: Checked } }
    else
      s' = settle(failTurn(
        { ...s, refused: true, child: { alive: false, turns: 0 }, badReports: s.badReports + 1 },
        s.flight.seq, "refused", false)),
    did' = { ...note("sessionReports"), ok: ok },
  }

  action report = {
    nondet pick = 1.to(6).oneOf()
    sessionReports(pick > 1)
  }

  /// The turn's `result` line.
  action turnEnds(ending: str): bool = all {
    s.flight.open,
    s.child.alive,
    s.flight.stage == Checked or s.bug == "readsTheAnswerBeforeTheCheck",
    if (ending == "answered")
      s' = settle(reached({
        ...s,
        chat: s.chat.append(Answer(s.flight.seq)),
        outcomes: s.outcomes.append({
          runNo: s.runNo, seq: s.flight.seq, ending: "answered", told: false,
          checked: s.flight.stage == Checked,
        }),
        lastFailure: "",
        flight: NO_FLIGHT,
        proc: Batch,
      }, s.flight.seq))
    else
      s' = settle(failTurn(s, s.flight.seq, ending, s.flight.stage == Checked)),
    did' = { ...note("turnEnds"), ending: ending },
  }

  action finish = {
    nondet pick = 1.to(8).oneOf()
    turnEnds(
      if (pick <= 5) "answered"
      else if (pick == 6) "not signed in"
      else if (pick == 7) "error"
      else "turn limit")
  }

  /// The session process exits by itself: in the middle of a turn, or idle.
  action sessionExits = all {
    s.child.alive,
    if (s.flight.open)
      s' = settle(failTurn(
        { ...s, child: { alive: false, turns: 0 }, refused: s.refused or s.bug == "refusesAfterAnExit" },
        s.flight.seq, "exited", s.flight.stage == Checked))
    else
      s' = { ...s, child: { alive: false, turns: 0 } },
    did' = note("sessionExits"),
  }

  /// The listener is stopped (SIGINT, SIGTERM). A turn in flight is cut
  /// short and nothing is recorded for it, so the next run asks again.
  action stop = all {
    s.proc == Waiting or s.proc == InTurn,
    if (s.flight.open and s.bug == "startsATurnWhileStopping")
      // The batch is carried on with while the process is going down.
      s' = {
        ...settleWith({ ...s, flight: NO_FLIGHT, child: { alive: false, turns: 0 }, proc: Batch }, true),
        proc: Down, inbox: [], flight: NO_FLIGHT, child: { alive: false, turns: 0 },
        interrupted: s.interrupted.union(Set(s.flight.seq)),
        stops: s.stops + 1,
      }
    else if (s.flight.open and s.bug == "recordsAnInterruptedTurnAsRead")
      s' = {
        ...reached(s, s.batchEnd),
        proc: Down, inbox: [], flight: NO_FLIGHT, child: { alive: false, turns: 0 },
        interrupted: s.interrupted.union(Set(s.flight.seq)),
        stops: s.stops + 1,
      }
    else
      s' = {
        ...s,
        proc: Down, inbox: [], flight: NO_FLIGHT, child: { alive: false, turns: 0 },
        interrupted: if (s.flight.open) s.interrupted.union(Set(s.flight.seq)) else s.interrupted,
        stops: if (s.flight.open) s.stops + 1 else s.stops,
      },
    did' = note("stop"),
  }

  // ---- step

  // The simulator picks one enabled branch at random, so a branch listed
  // more often is taken more often. Stopping the listener and a session
  // exiting by itself are always possible; taken as often as anything else
  // they would leave few traces that get as far as a turn ending.
  action step = any {
    user, user, user, user,
    launch, launch,
    pollReturns, pollReturns, pollReturns, pollReturns,
    report, report, report, report, report, report,
    finish, finish, finish, finish, finish, finish,
    sessionExits,
    stop,
  }

  // ---- invariants

  /// An event the listener owes a turn: a request sent while Listen was on
  /// (or any request, with --always-listen), after the place it started at.
  def owed(e: Event): bool = and {
    e.kind == Request,
    e.on or s.alwaysListen,
    e.seq > s.joined,
  }

  def outcomesOf(seq: int): List[Outcome] = s.outcomes.select(o => o.seq == seq)

  /// No answer is read from a session that has not reported, in that turn,
  /// that it holds exactly the allowed tools. (The message itself is written
  /// before the report: the session reports only once it has a message.)
  val noAnswerFromAnUncheckedSession: bool =
    s.outcomes.indices().forall(i => s.outcomes[i].ending == "answered" implies s.outcomes[i].checked)

  /// After a bad report the process is gone and nothing is written to any
  /// session for the rest of that listener run.
  val afterARefusalNothingIsWritten: bool = and {
    s.refused implies not(s.child.alive),
    s.writes.indices().forall(i => not(s.writes[i].afterRefusal)),
  }

  /// A refusal follows a bad report and nothing else: a session that exits,
  /// or a turn that fails, does not stop later messages from being answered.
  val onlyABadReportRefuses: bool =
    s.refused implies s.badReports > 0

  /// At most one turn at a time: nothing is written while a turn is open.
  val oneTurnAtATime: bool =
    s.writes.indices().forall(i => not(s.writes[i].turnWasOpen))

  /// Within one listener run, messages go to the session in arrival order
  /// and each at most once.
  val handledInArrivalOrder: bool =
    s.writes.indices().forall(i => s.writes.indices().forall(j =>
      (i < j and s.writes[i].runNo == s.writes[j].runNo) implies s.writes[i].seq < s.writes[j].seq))

  /// A message goes to a session a second time only in a later run, and only
  /// if stopping the listener cut its first turn short.
  val askedAgainOnlyAfterAnInterruption: bool =
    s.writes.indices().forall(i => s.writes.indices().forall(j =>
      (i < j and s.writes[i].seq == s.writes[j].seq) implies
        (s.writes[i].runNo < s.writes[j].runNo and s.interrupted.contains(s.writes[i].seq))))

  /// Every turn that was started and ended has exactly one ending, and no
  /// message is both answered and reported as failed, or answered twice.
  val oneEndingPerMessage: bool =
    s.log.indices().forall(i => outcomesOf(s.log[i].seq).length() <= 1)

  /// Nothing behind the server's cursor was dropped without a word: every
  /// owed request there was answered, or failed and was told, or failed the
  /// same way as the turn before it (which was told).
  val nothingIsDroppedSilently: bool =
    s.log.indices().forall(i =>
      (owed(s.log[i]) and s.log[i].seq <= s.stored) implies outcomesOf(s.log[i].seq).length() == 1)

  /// A failure is told once: a turn that fails with the same sentence as the
  /// turn before it, in the same run, says nothing; any other failure is
  /// told.
  val aRepeatedFailureIsToldOnce: bool =
    s.outcomes.indices().forall(i => {
      val o = s.outcomes[i]
      val repeat = i > 0 and s.outcomes[i - 1].runNo == o.runNo and s.outcomes[i - 1].ending == o.ending
      val failed = o.ending != "answered"
      failed implies (o.told == not(repeat))
    })

  /// The Chat panel holds one answer per answered turn and one notice per
  /// failure told, and nothing else of the listener's but the lines about a
  /// new conversation.
  val chatMatchesTheEndings: bool = and {
    s.chat.select(l => match l { | Answer(_) => true | _ => false }).length() ==
      s.outcomes.select(o => o.ending == "answered").length(),
    s.chat.select(l => match l { | Notice(_) => true | _ => false }).length() ==
      s.outcomes.select(o => o.told).length(),
  }

  /// Nothing is written to a session for an event sent while Listen was off
  /// (unless the listener was started with --always-listen).
  val nothingIsWrittenForWhatWasSentWhileListenWasOff: bool =
    s.alwaysListen or s.writes.indices().forall(i => s.writes[i].sentWhileOn)

  /// Nothing is written while nothing is asked: every line written to a
  /// session is for a request in the log, so a listener that is polled for
  /// ever with no new request writes nothing.
  val nothingIsWrittenWhileIdle: bool = and {
    s.writes.indices().forall(i => {
      val seq = s.writes[i].seq
      seq >= 1 and seq <= lastSeq(s) and s.log[seq - 1].kind == Request
    }),
    s.writes.length() <= s.log.select(e => e.kind == Request).length() + s.stops,
    s.children <= s.writes.length(),
  }

  /// A Claude Code that is missing or older than the minimum never leads to
  /// a turn: nothing is attached, started or written.
  val noTurnWithoutAUsableBinary: bool =
    (s.binary != Usable) implies and {
      s.writes.length() == 0,
      s.children == 0,
      s.runNo == 0,
      s.proc == Down,
    }

  /// A conversation is never replaced (by `/new`, or after a number of
  /// messages) while one of its turns is in flight.
  val aNewConversationCarriesNoTurn: bool =
    s.boundaries.indices().forall(i => not(s.boundaries[i]))

  /// With --new-conversation-after N, a process answers at most N messages.
  val aConversationIsBounded: bool =
    (s.every > 0) implies s.child.turns <= s.every

  /// Once the listener is stopping, no session is started and nothing is
  /// written.
  val nothingStartsAfterAStop: bool =
    s.writes.indices().forall(i => not(s.writes[i].afterStop))

  /// A stopped listener holds nothing: no process, no turn.
  val aStoppedListenerHoldsNothing: bool =
    (s.proc == Down) implies (not(s.child.alive) and not(s.flight.open) and s.inbox.length() == 0)

  /// The listener rests only where it blocks, and a turn is open exactly
  /// while it waits on the session.
  val restsOnlyWhereItBlocks: bool = and {
    s.proc != Batch,
    s.flight.open iff (s.proc == InTurn),
    s.flight.open implies s.child.alive,
    s.stored <= lastSeq(s),
    s.cursor <= lastSeq(s),
  }

  val allInvariants: bool = and {
    noAnswerFromAnUncheckedSession,
    afterARefusalNothingIsWritten,
    onlyABadReportRefuses,
    oneTurnAtATime,
    handledInArrivalOrder,
    askedAgainOnlyAfterAnInterruption,
    oneEndingPerMessage,
    nothingIsDroppedSilently,
    aRepeatedFailureIsToldOnce,
    chatMatchesTheEndings,
    nothingIsWrittenForWhatWasSentWhileListenWasOff,
    nothingIsWrittenWhileIdle,
    noTurnWithoutAUsableBinary,
    aNewConversationCarriesNoTurn,
    aConversationIsBounded,
    nothingStartsAfterAStop,
    aStoppedListenerHoldsNothing,
    restsOnlyWhereItBlocks,
  }

  // ---- mutants
  //
  // Each is the intended design with one thing done wrong, and each is
  // simulated in the tests with the expectation that the named invariant
  // fails: an invariant no mutant can break is not known to check anything.

  /// Carries on with the batch while stopping: `nothingStartsAfterAStop`.
  action initStartsATurnWhileStopping = initWith("startsATurnWhileStopping")

  /// Acks the batch although a turn was cut short: `nothingIsDroppedSilently`.
  action initRecordsAnInterruptedTurnAsRead = initWith("recordsAnInterruptedTurnAsRead")

  /// Starts a new session for the next message after a refusal:
  /// `afterARefusalNothingIsWritten`.
  action initAsksAgainAfterARefusal = initWith("asksAgainAfterARefusal")

  /// Takes a result line before the session's report:
  /// `noAnswerFromAnUncheckedSession`.
  action initReadsTheAnswerBeforeTheCheck = initWith("readsTheAnswerBeforeTheCheck")

  /// Does not wait for a turn before the next event: `oneTurnAtATime`, and
  /// with `/new` in the batch `aNewConversationCarriesNoTurn`.
  action initDoesNotWaitForTheTurn = initWith("doesNotWaitForTheTurn")

  /// The same, seen from `/new`: `aNewConversationCarriesNoTurn`.
  action initStartsOverWithATurnInFlight = initWith("doesNotWaitForTheTurn")

  /// Treats a session that exits like one that reported badly:
  /// `onlyABadReportRefuses`.
  action initRefusesAfterAnExit = initWith("refusesAfterAnExit")

  /// Keeps one process whatever --new-conversation-after says:
  /// `aConversationIsBounded`.
  action initNeverReplacesAConversation = initWith("neverReplacesAConversation")

  /// Records nothing for a turn that failed, so a later run asks again:
  /// `askedAgainOnlyAfterAnInterruption`.
  action initForgetsAFailedTurn = initWith("forgetsAFailedTurn")

  /// Says every failure, repeated or not: `aRepeatedFailureIsToldOnce`.
  action initTellsEveryFailure = initWith("tellsEveryFailure")

  /// Attaches without checking the version: `noTurnWithoutAUsableBinary`.
  action initSkipsTheVersionCheck = initWith("skipsTheVersionCheck")

  /// Judges a backlog by the toggle as it is when the batch arrives:
  /// `nothingIsWrittenForWhatWasSentWhileListenWasOff`.
  action initTrustsTheToggleAsItIsNow = initWith("trustsTheToggleAsItIsNow")

  // ---- scenarios

  action start(listenOn: bool, every: int, bug: str): bool = all {
    s' = initial(Usable, every, false, listenOn, bug),
    did' = note("init"),
  }

  // A session that exits in the middle of a turn: the user is told once, the
  // message is not asked again, and the next message gets a new session.
  run anExitInTheMiddleOfATurnIsToldAndTheNextMessageWorks =
    start(true, 0, "")
      .then(launch)
      .then(userDoes(Request))
      .then(pollReturns)
      .then(sessionReports(true))
      .then(sessionExits)
      .expect(s.chat == [Notice({ seq: 1, reason: "exited" })] and s.stored == 1)
      .expect(s.proc == Waiting and not(s.child.alive))
      .then(userDoes(Request))
      .then(pollReturns)
      .expect(s.children == 2 and s.flight.seq == 2)
      .then(sessionReports(true))
      .then(turnEnds("answered"))
      .expect(s.chat == [Notice({ seq: 1, reason: "exited" }), Answer(2)])
      .expect(s.writes.length() == 2 and allInvariants)

  // A session that reports a tool it must not have: killed, said once, and
  // every later message refused without a word and without a process.
  run aBadReportIsFinalForTheRun =
    start(true, 0, "")
      .then(launch)
      .then(userDoes(Request))
      .then(userDoes(Request))
      .then(userDoes(Request))
      .then(pollReturns)
      .then(sessionReports(false))
      .expect(s.refused and not(s.child.alive) and s.proc == Waiting)
      .expect(s.chat == [Notice({ seq: 1, reason: "refused" })])
      .expect(s.writes.length() == 1 and s.children == 1 and s.stored == 3)
      .expect(allInvariants)
      // A new run of the listener starts again from nothing.
      .then(stop)
      .then(launch)
      .then(userDoes(Request))
      .then(pollReturns)
      .expect(s.children == 2 and not(s.refused))

  // Two messages in one batch: the second waits for the first.
  run aSecondMessageWaitsForTheFirst =
    start(true, 0, "")
      .then(launch)
      .then(userDoes(Request))
      .then(userDoes(Request))
      .then(pollReturns)
      .expect(s.flight.seq == 1 and s.inbox.length() == 1 and s.stored == 0)
      .then(sessionReports(true))
      .then(turnEnds("answered"))
      .expect(s.flight.seq == 2 and s.stored == 1 and s.children == 1)
      .then(sessionReports(true))
      .then(turnEnds("answered"))
      .expect(s.chat == [Answer(1), Answer(2)] and s.stored == 2 and s.proc == Waiting)
      .expect(allInvariants)

  // The same failure twice in a row is said once; a different one is said.
  run theSameFailureIsSaidOnce =
    start(true, 0, "")
      .then(launch)
      .then(userDoes(Request)).then(userDoes(Request)).then(userDoes(Request))
      .then(pollReturns)
      .then(sessionReports(true)).then(turnEnds("not signed in"))
      .then(sessionReports(true)).then(turnEnds("not signed in"))
      .then(sessionReports(true)).then(turnEnds("turn limit"))
      .expect(s.chat == [Notice({ seq: 1, reason: "not signed in" }), Notice({ seq: 3, reason: "turn limit" })])
      .expect(s.children == 1 and allInvariants)

  // `/new` replaces the conversation without a turn, between turns.
  run newStartsAConversationWithoutATurn =
    start(true, 0, "")
      .then(launch)
      .then(userDoes(Request))
      .then(userDoes(NewConversation))
      .then(userDoes(Request))
      .then(pollReturns)
      .then(sessionReports(true))
      .then(turnEnds("answered"))
      .expect(s.chat == [Answer(1), Fresh(2)] and s.children == 2 and s.flight.seq == 3)
      .expect(s.boundaries == [false] and s.writes.length() == 2)
      .expect(allInvariants)

  // After N messages the next one goes to a new process.
  run aFullConversationIsReplaced =
    start(true, 2, "")
      .then(launch)
      .then(userDoes(Request)).then(userDoes(Request)).then(userDoes(Request))
      .then(pollReturns)
      .then(sessionReports(true)).then(turnEnds("answered"))
      .then(sessionReports(true)).then(turnEnds("answered"))
      .expect(s.children == 2 and s.flight.seq == 3 and s.child.turns == 1)
      .expect(allInvariants)

  // Stopped in the middle of a batch: the second message is not started,
  // nothing is recorded, and the next run answers both.
  run aStopLeavesTheBatchForTheNextRun =
    start(true, 0, "")
      .then(launch)
      .then(userDoes(Noise))
      .then(pollReturns)
      .then(userDoes(Request))
      .then(userDoes(Request))
      .then(pollReturns)
      .then(sessionReports(true))
      .then(stop)
      .expect(s.stored == 1 and s.chat == [] and s.writes.length() == 1 and s.interrupted == Set(2))
      .then(launch)
      .then(pollReturns)
      .expect(s.flight.seq == 2 and s.children == 2)
      .then(sessionReports(true)).then(turnEnds("answered"))
      .then(sessionReports(true)).then(turnEnds("answered"))
      .expect(s.chat == [Answer(2), Answer(3)] and s.stored == 3)
      .expect(allInvariants)

  // Carrying on with the batch while stopping: the second message starts a
  // turn in a new process while the listener is going down.
  run carryingOnWhileStoppingStartsATurn =
    start(true, 0, "startsATurnWhileStopping")
      .then(launch)
      .then(userDoes(Request))
      .then(userDoes(Request))
      .then(pollReturns)
      .then(sessionReports(true))
      .then(stop)
      .expect(s.writes.length() == 2 and s.children == 2)
      .expect(not(nothingStartsAfterAStop))

  // A message sent while Listen was off is not written to a session, also
  // when Listen is on by the time the batch arrives; one sent while it was
  // on is written, also when it is off by then.
  run aBatchIsJudgedByTheToggleEachEventWasSentUnder =
    start(false, 0, "")
      .then(launch)
      .then(userDoes(Noise))
      .then(pollReturns)
      .then(userDoes(Request))       // seq 2, sent while off
      .then(userDoes(ListenOn))
      .then(userDoes(Request))       // seq 4, sent while on
      .then(userDoes(ListenOff))
      .then(pollReturns)
      .expect(s.flight.seq == 4 and s.writes.length() == 1)
      .expect(allInvariants)

  // A Claude Code that is too old: nothing is attached, however often the
  // listener is started.
  run anOldClaudeCodeNeverLeadsToATurn =
    (all { s' = initial(TooOld, 0, false, true, ""), did' = note("init") })
      .then(launch)
      .then(userDoes(Request))
      .then(launch)
      .expect(s.fatals == 2 and s.proc == Down and s.writes.length() == 0 and s.stored == 0)
      .expect(allInvariants)
}
