// The event log as a bounded window, and what a reader that resumes is owed.
//
// `session.qnt` models the log as unbounded. In src/rust/cadpole-cli/src/server.rs
// it is a window: `App::emit_by` drops the oldest entries past a cap, a reader
// resumes by sequence number alone, the SSE stream can fall behind, and a
// restart begins the numbering again.
//
// The rule: a reader that resumes from its last sequence number either sees
// every event since, or is told it cannot (`nothingIsLostSilently`). That
// needs three things, each a switch in this model:
//
//   * the low end of the window is checked on resume (`wait_events` does
//     this with `truncated` and `first_seq`; `get_events` and `poll_events`
//     do not);
//   * a lagging SSE stream reports the lag (`get_events` drops it);
//   * the cursor on the wire carries the generation that counted it (it is a
//     bare number on every route; `ack` reports only a cursor past the end
//     of the log, as `epoch_reset`).
//
// `init` turns all three on, so the invariants hold in `step`. The scenarios
// turn one off at a time and show the loss.

module eventlog {
  // ---- constants

  pure val READERS: Set[str] = Set("ui", "mcp")

  // How each reader reads:
  //   sse  -- `GET /api/events`, the web client
  //   poll -- `WaitEvents`, used by `cadpole mcp` and `cadpole op listen`
  pure val TRANSPORT: str -> str = Map("ui" -> "sse", "mcp" -> "poll")

  // The cap is `10_000` in `App::emit_by`; //spec:names_test checks that number.
  // It is small here so that eviction is reachable in a short trace.
  pure val LOG_CAP: int = 3
  pure val MAX_SEQ: int = 7
  pure val MAX_EPOCH: int = 2

  // ---- types

  /// An event: its sequence number and the generation that issued it.
  type Ev = { ep: int, sq: int }

  // ---- state

  var epoch: int               // the server's generation; a restart starts a new one
  var log: List[int]           // the sequence numbers the server still holds
  var nextSeq: int
  var cursors: str -> int      // the cursors the server stores on `Ack`
  var after: str -> int        // each reader's own resume point
  var atEpoch: str -> int      // the generation `after` was counted in
  var connected: str -> bool
  var seen: str -> Set[Ev]
  var missed: str -> Set[Ev]   // events lost without the reader being told
  var told: str -> bool        // the server said it could not serve the reader's cursor

  var checksTheLowEndOfTheWindow: bool
  var sseReportsLag: bool
  var theWireCarriesAGeneration: bool

  // ---- helpers

  val firstSeq: int = if (log.length() == 0) 0 else log[0]
  val lastSeq: int = if (log.length() == 0) 0 else log[log.length() - 1]

  pure def atLeast(a: int, b: int): int = if (a > b) a else b

  def evsInLogUpTo(n: int): Set[Ev] =
    log.indices().fold(Set(), (acc, i) =>
      if (log[i] <= n) acc.union(Set({ ep: epoch, sq: log[i] })) else acc)

  /// `WaitEvents` reads `after == 0` as "use my stored cursor"; the SSE route
  /// reads it as "from the beginning".
  def resolveAfter(c: str, requested: int): int =
    if (TRANSPORT.get(c) == "poll" and requested == 0) cursors.get(c) else requested

  /// The cursor cannot be served in full: it is past the end of the log, from
  /// an earlier generation, or below the window.
  def unserviceable(c: str, ask: int): bool =
    or {
      ask > lastSeq,
      atEpoch.get(c) != epoch,
      log.length() > 0 and ask + 1 < firstSeq,
    }

  /// The events lost if an unserviceable cursor is served anyway.
  def gapFor(c: str, ask: int): Set[Ev] =
    if (ask > lastSeq) evsInLogUpTo(lastSeq)
    else if (atEpoch.get(c) != epoch) evsInLogUpTo(ask)
    else (ask + 1).to(firstSeq - 1).map(s => { ep: epoch, sq: s })

  /// Whether the server, as switched, tells the reader.
  def reports(c: str, ask: int): bool =
    if (ask > lastSeq) true
    else if (atEpoch.get(c) != epoch) theWireCarriesAGeneration
    else checksTheLowEndOfTheWindow

  // ---- actions

  action init = all {
    epoch' = 1,
    log' = [],
    nextSeq' = 1,
    cursors' = READERS.mapBy(c => 0),
    after' = READERS.mapBy(c => 0),
    atEpoch' = READERS.mapBy(c => 1),
    connected' = READERS.mapBy(c => false),
    seen' = READERS.mapBy(c => Set()),
    missed' = READERS.mapBy(c => Set()),
    told' = READERS.mapBy(c => false),
    checksTheLowEndOfTheWindow' = true,
    sseReportsLag' = true,
    theWireCarriesAGeneration' = true,
  }

  /// The server appends an event and drops the front of the log past the cap.
  action emit = {
    val appended = log.append(nextSeq)
    all {
      nextSeq <= MAX_SEQ,
      log' =
        if (appended.length() > LOG_CAP)
          appended.slice(appended.length() - LOG_CAP, appended.length())
        else appended,
      nextSeq' = nextSeq + 1,
      epoch' = epoch, cursors' = cursors, after' = after, atEpoch' = atEpoch,
      connected' = connected, seen' = seen, missed' = missed, told' = told,
      checksTheLowEndOfTheWindow' = checksTheLowEndOfTheWindow,
      sseReportsLag' = sseReportsLag,
      theWireCarriesAGeneration' = theWireCarriesAGeneration,
    }
  }

  /// A connected reader receives the next event above its cursor. The read is
  /// a filter on `seq > after`, so it starts at the low end of the window
  /// without noticing a hole below it.
  action deliverTo(c: str): bool = {
    val nxt = atLeast(after.get(c) + 1, firstSeq)
    all {
      connected.get(c),
      atEpoch.get(c) == epoch,
      log.length() > 0,
      nxt <= lastSeq,
      after' = after.set(c, nxt),
      seen' = seen.setBy(c, s => s.union(Set({ ep: epoch, sq: nxt }))),
      epoch' = epoch, log' = log, nextSeq' = nextSeq, cursors' = cursors,
      atEpoch' = atEpoch, connected' = connected, missed' = missed, told' = told,
      checksTheLowEndOfTheWindow' = checksTheLowEndOfTheWindow,
      sseReportsLag' = sseReportsLag,
      theWireCarriesAGeneration' = theWireCarriesAGeneration,
    }
  }

  action anyDelivery = {
    nondet c = READERS.oneOf()
    deliverTo(c)
  }

  action disconnect(c: str): bool = all {
    connected.get(c),
    connected' = connected.set(c, false),
    epoch' = epoch, log' = log, nextSeq' = nextSeq, cursors' = cursors,
    after' = after, atEpoch' = atEpoch, seen' = seen, missed' = missed, told' = told,
    checksTheLowEndOfTheWindow' = checksTheLowEndOfTheWindow,
    sseReportsLag' = sseReportsLag,
    theWireCarriesAGeneration' = theWireCarriesAGeneration,
  }

  action anyDisconnect = {
    nondet c = READERS.filter(r => connected.get(r)).oneOf()
    disconnect(c)
  }

  /// A reader reconnects and asks to resume from `requested`. If the cursor is
  /// unserviceable, either the server says so and the reader restarts from the
  /// low end of the window, or the events in the gap are lost silently.
  action resumeWith(c: str, requested: int): bool = {
    val ask = resolveAfter(c, requested)
    val gap = unserviceable(c, ask)
    val heard = gap and reports(c, ask)
    all {
      not(connected.get(c)),
      requested >= 0,
      connected' = connected.set(c, true),
      after' = after.set(c, if (heard) atLeast(firstSeq - 1, 0) else ask),
      atEpoch' = atEpoch.set(c, epoch),
      told' = told.set(c, told.get(c) or heard),
      missed' =
        if (gap and not(heard)) missed.setBy(c, m => m.union(gapFor(c, ask))) else missed,
      epoch' = epoch, log' = log, nextSeq' = nextSeq, cursors' = cursors, seen' = seen,
      checksTheLowEndOfTheWindow' = checksTheLowEndOfTheWindow,
      sseReportsLag' = sseReportsLag,
      theWireCarriesAGeneration' = theWireCarriesAGeneration,
    }
  }

  /// The ordinary reconnect: the reader sends the last sequence number it saw.
  action anyResume = {
    nondet c = READERS.filter(r => not(connected.get(r))).oneOf()
    resumeWith(c, after.get(c))
  }

  /// The broadcast channel lags under a slow SSE reader. Either the lag is
  /// reported and the reader keeps its cursor, or the stream skips to the tail.
  action lagSse(c: str): bool = {
    val skipped =
      if (after.get(c) + 1 > lastSeq) Set()
      else atLeast(after.get(c) + 1, firstSeq).to(lastSeq).map(s => { ep: epoch, sq: s })
    all {
      connected.get(c),
      TRANSPORT.get(c) == "sse",
      atEpoch.get(c) == epoch,
      skipped != Set(),
      after' = after.set(c, if (sseReportsLag) after.get(c) else lastSeq),
      told' = told.set(c, told.get(c) or sseReportsLag),
      missed' = if (sseReportsLag) missed else missed.setBy(c, m => m.union(skipped)),
      epoch' = epoch, log' = log, nextSeq' = nextSeq, cursors' = cursors,
      atEpoch' = atEpoch, connected' = connected, seen' = seen,
      checksTheLowEndOfTheWindow' = checksTheLowEndOfTheWindow,
      sseReportsLag' = sseReportsLag,
      theWireCarriesAGeneration' = theWireCarriesAGeneration,
    }
  }

  action anyLag = {
    nondet c = READERS.filter(r => TRANSPORT.get(r) == "sse").oneOf()
    lagSse(c)
  }

  /// `Ack`: the server stores the cursor, clamped to the end of the log, and
  /// reports a cursor that was past it.
  action ackClient(c: str, n: int): bool = {
    val ahead = n > lastSeq
    all {
      n >= 0,
      cursors' = cursors.set(c, if (ahead) lastSeq else n),
      told' = told.set(c, told.get(c) or ahead),
      epoch' = epoch, log' = log, nextSeq' = nextSeq, after' = after,
      atEpoch' = atEpoch, connected' = connected, seen' = seen, missed' = missed,
      checksTheLowEndOfTheWindow' = checksTheLowEndOfTheWindow,
      sseReportsLag' = sseReportsLag,
      theWireCarriesAGeneration' = theWireCarriesAGeneration,
    }
  }

  action anyAck = {
    nondet c = READERS.oneOf()
    ackClient(c, after.get(c))
  }

  /// The server restarts. The log and the stored cursors are in memory and
  /// begin again; the readers keep the numbers they had.
  action serverRestart = all {
    epoch < MAX_EPOCH,
    epoch' = epoch + 1,
    log' = [],
    nextSeq' = 1,
    cursors' = READERS.mapBy(c => 0),
    connected' = READERS.mapBy(c => false),
    after' = after, atEpoch' = atEpoch, seen' = seen, missed' = missed, told' = told,
    checksTheLowEndOfTheWindow' = checksTheLowEndOfTheWindow,
    sseReportsLag' = sseReportsLag,
    theWireCarriesAGeneration' = theWireCarriesAGeneration,
  }

  // ---- step

  action step = any {
    emit,
    anyDelivery,
    anyDisconnect,
    anyResume,
    anyLag,
    anyAck,
    serverRestart,
  }

  // ---- invariants

  /// No reader loses an event without being told.
  val nothingIsLostSilently: bool =
    READERS.forall(c => missed.get(c) == Set())

  /// A reader counting in the current generation is never past the end of the log.
  val noCurrentReaderIsPastTheEndOfTheLog: bool =
    READERS.forall(c => (atEpoch.get(c) == epoch) implies after.get(c) <= lastSeq)

  /// Every event a reader has seen in this generation is at or below its cursor.
  val everySeenEventIsAtOrBelowTheCursor: bool =
    READERS.forall(c =>
      seen.get(c).forall(e => e.ep != epoch or e.sq <= after.get(c)))

  /// The log is consecutive: eviction removes a prefix and never leaves a hole.
  val theWindowIsDense: bool =
    log.indices().forall(i => log[i] == firstSeq + i)

  /// A stored cursor never exceeds the last sequence number in the log.
  val cursorNeverPassesTheLog: bool =
    READERS.forall(c => cursors.get(c) <= lastSeq)

  /// A reader never holds an event the server did not issue.
  val everySeenEventWasIssued: bool =
    READERS.forall(c =>
      seen.get(c).forall(e => e.sq >= 1 and (e.ep < epoch or e.sq < nextSeq)))

  val allInvariants: bool = and {
    nothingIsLostSilently,
    noCurrentReaderIsPastTheEndOfTheLog,
    everySeenEventIsAtOrBelowTheCursor,
    theWindowIsDense,
    cursorNeverPassesTheLog,
    everySeenEventWasIssued,
  }

  // ---- scenario-only actions
  //
  // Not in `step`. The first three each turn off one of the switches; each is
  // what some route of the server does.

  action setSwitches(low: bool, lag: bool, generation: bool): bool = all {
    checksTheLowEndOfTheWindow' = low,
    sseReportsLag' = lag,
    theWireCarriesAGeneration' = generation,
    epoch' = epoch, log' = log, nextSeq' = nextSeq, cursors' = cursors,
    after' = after, atEpoch' = atEpoch, connected' = connected,
    seen' = seen, missed' = missed, told' = told,
  }

  /// A resume below the window is served the window with no notice
  /// (`get_events`, `poll_events`).
  action noReaderChecksTheLowEndOfTheWindow =
    setSwitches(false, sseReportsLag, theWireCarriesAGeneration)

  /// The SSE stream drops the lag error and carries on from the tail
  /// (`get_events`).
  action theSseTailSwallowsALag =
    setSwitches(checksTheLowEndOfTheWindow, false, theWireCarriesAGeneration)

  /// A cursor is a bare number that does not say which generation counted it
  /// (every route).
  action theWireIsJustANumber =
    setSwitches(checksTheLowEndOfTheWindow, sseReportsLag, false)

  /// The same lag on a polling reader: the poll re-reads the log, so nothing
  /// changes.
  action lagPoll(c: str): bool = all {
    connected.get(c),
    TRANSPORT.get(c) == "poll",
    setSwitches(checksTheLowEndOfTheWindow, sseReportsLag, theWireCarriesAGeneration),
  }

  // ---- scenarios

  // With the low end unchecked, a reader that resumes below the window is
  // served the rest as if it were complete: seq 2 is lost and nothing says so.
  run aResumerBelowTheWindowIsServedATruncatedStream =
    init
      .then(noReaderChecksTheLowEndOfTheWindow)
      .then(resumeWith("mcp", 0))
      .then(emit)
      .then(deliverTo("mcp"))
      .expect(after.get("mcp") == 1)
      .then(disconnect("mcp"))
      .then(emit).then(emit).then(emit).then(emit)
      .expect(firstSeq == 3 and lastSeq == 5)
      .then(resumeWith("mcp", 1))
      .expect(after.get("mcp") == 1 and not(told.get("mcp")))
      .expect(missed.get("mcp") == Set({ ep: 1, sq: 2 }))
      .expect(not(nothingIsLostSilently))
      .then(deliverTo("mcp"))
      .expect(after.get("mcp") == 3)
      .expect(everySeenEventIsAtOrBelowTheCursor)

  // The same trace with the low end checked: the reader is told and restarts
  // from the low end of the window.
  run tellingTheResumerRestoresTheGuarantee =
    init
      .then(resumeWith("mcp", 0))
      .then(emit)
      .then(deliverTo("mcp"))
      .then(disconnect("mcp"))
      .then(emit).then(emit).then(emit).then(emit)
      .then(resumeWith("mcp", 1))
      .expect(told.get("mcp") and after.get("mcp") == 2)
      .expect(missed.get("mcp") == Set())
      .then(deliverTo("mcp"))
      .expect(after.get("mcp") == 3)
      .expect(allInvariants)

  // With the lag unreported, an SSE reader that lags skips to the tail and
  // loses the events between. Its own state stays self-consistent.
  run anSseReaderThatLagsIsNeverTold =
    init
      .then(theSseTailSwallowsALag)
      .then(resumeWith("ui", 0))
      .then(emit)
      .then(deliverTo("ui"))
      .then(emit).then(emit)
      .expect(log == [1, 2, 3] and after.get("ui") == 1)
      .then(lagSse("ui"))
      .expect(after.get("ui") == 3 and not(told.get("ui")))
      .expect(missed.get("ui") == Set({ ep: 1, sq: 2 }, { ep: 1, sq: 3 }))
      .expect(not(nothingIsLostSilently))
      .expect(everySeenEventIsAtOrBelowTheCursor)
      .expect(noCurrentReaderIsPastTheEndOfTheLog)

  // The same lag on a polling reader loses nothing.
  run thePollingReadersRecoverFromTheSameLag =
    init
      .then(theSseTailSwallowsALag)
      .then(resumeWith("mcp", 0))
      .then(emit).then(emit).then(emit)
      .then(lagPoll("mcp"))
      .expect(after.get("mcp") == 0 and missed.get("mcp") == Set())
      .then(deliverTo("mcp"))
      .then(deliverTo("mcp"))
      .then(deliverTo("mcp"))
      .expect(seen.get("mcp") == Set({ ep: 1, sq: 1 }, { ep: 1, sq: 2 }, { ep: 1, sq: 3 }))
      .expect(allInvariants)

  // After a restart, `Ack` reports a cursor that is past the end of the new log.
  run ackCatchesTheRestartWhenTheCursorIsPastTheNewLog =
    init
      .then(resumeWith("mcp", 0))
      .then(emit).then(emit).then(emit)
      .then(deliverTo("mcp")).then(deliverTo("mcp")).then(deliverTo("mcp"))
      .then(ackClient("mcp", 3))
      .expect(cursors.get("mcp") == 3 and not(told.get("mcp")))
      .then(serverRestart)
      .then(emit)
      .then(ackClient("mcp", 3))
      .expect(told.get("mcp") and cursors.get("mcp") == 1)
      .expect(allInvariants)

  // With no generation on the wire, a cursor from before a restart that fits
  // inside the new log passes every numeric check, and the new events at or
  // below it are lost.
  run aCursorThatFitsTheNewLogIsReadAsIfItWereCurrent =
    init
      .then(theWireIsJustANumber)
      .then(resumeWith("mcp", 0))
      .then(emit).then(emit).then(emit)
      .then(deliverTo("mcp")).then(deliverTo("mcp")).then(deliverTo("mcp"))
      .expect(after.get("mcp") == 3)
      .then(serverRestart)
      .then(emit).then(emit).then(emit).then(emit)
      .expect(epoch == 2 and firstSeq == 2 and lastSeq == 4)
      .then(resumeWith("mcp", 3))
      .expect(not(told.get("mcp")) and after.get("mcp") == 3)
      .expect(missed.get("mcp") == Set({ ep: 2, sq: 2 }, { ep: 2, sq: 3 }))
      .expect(not(nothingIsLostSilently))

  // The same trace with a generation on the wire is a reported reset.
  run aGenerationOnTheWireCatchesTheOneAckCannot =
    init
      .then(resumeWith("mcp", 0))
      .then(emit).then(emit).then(emit)
      .then(deliverTo("mcp")).then(deliverTo("mcp")).then(deliverTo("mcp"))
      .then(serverRestart)
      .then(emit).then(emit).then(emit).then(emit)
      .then(resumeWith("mcp", 3))
      .expect(told.get("mcp") and after.get("mcp") == 1)
      .expect(missed.get("mcp") == Set())
      .expect(allInvariants)

  // `after: 0` means "from my stored cursor" over `WaitEvents` and "from the
  // beginning" over SSE, so a polling reader cannot ask for the whole log.
  run aPollingReaderCannotAskForTheWholeLog =
    init
      .then(resumeWith("mcp", 0))
      .then(emit).then(emit)
      .then(deliverTo("mcp")).then(deliverTo("mcp"))
      .then(ackClient("mcp", 2))
      .then(disconnect("mcp"))
      .expect(cursors.get("mcp") == 2)
      .then(resumeWith("mcp", 0))
      .expect(after.get("mcp") == 2)
      .then(disconnect("mcp"))
      .then(resumeWith("ui", 0))
      .expect(after.get("ui") == 0)
      .expect(allInvariants)
}
