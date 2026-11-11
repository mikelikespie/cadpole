// The session and event protocol.
//
// One `cadpole serve` process holds one document; a web UI, the CLI and an
// assistant read and write it through the session API (docs/design.md;
// `ServerHost` in cadpole-cli/src/server.rs). The model covers:
//
//   * script writes with optimistic concurrency on the revision;
//   * selection refs kept across script writes and re-resolved as live or
//     stale against the current model;
//   * a dense event log with ordered, at-least-once delivery per client;
//   * cursors stored on the server (the `Ack` operation), so a client that
//     reconnects resumes from what it acknowledged;
//   * a server restart, which starts a new log and drops the stored cursors.
//
// Geometry is abstracted away: a script is the set of feature ids it yields.

module session {
  // ---- constants

  pure val CLIENTS: Set[str] = Set("ui", "cli", "assistant")
  pure val FEATURES: Set[str] = Set("base", "lid", "hole")
  pure val MAX_SEQ: int = 8

  // ---- types

  type EventKind =
    | ScriptChanged
    | ParamsChanged
    | SelectionChanged
    | Message
    | ClientJoined
    | ClientLeft

  type Event = { seq: int, kind: EventKind, rev: int, by: str }

  type Attempt =
    | NoAttempt
    | Tried({ by: str, base: int, revBefore: int, accepted: bool })

  // ---- state

  var revision: int                 // content revision (script + params)
  var features: Set[str]            // feature ids the current script yields
  var selection: Set[str]           // retained refs whose feature exists
  var stale: Set[str]               // retained refs whose feature does not
  var log: List[Event]              // event log; seq == index + 1
  var connected: Set[str]           // clients with an open event stream
  var cursor: str -> int            // last seq each client has consumed
  var acked: str -> int             // the cursor the server stores per client
  var delivered: str -> Set[int]    // every seq each client ever received
  var assistantBase: int            // revision the assistant last read
  var lastAttempt: Attempt          // the most recent script write attempt

  // ---- helpers

  val canEmit: bool = log.length() < MAX_SEQ

  def emit(k: EventKind, r: int, who: str): List[Event] =
    log.append({ seq: log.length() + 1, kind: k, rev: r, by: who })

  /// Every retained ref. Whether a ref is live or stale is derived from the
  /// current model; only a selection write changes this set.
  val refs: Set[str] = selection.union(stale)

  // ---- actions

  action init = all {
    revision' = 0,
    features' = Set("base"),
    selection' = Set(),
    stale' = Set(),
    log' = [],
    connected' = Set(),
    cursor' = CLIENTS.mapBy(c => 0),
    acked' = CLIENTS.mapBy(c => 0),
    delivered' = CLIENTS.mapBy(c => Set()),
    assistantBase' = 0,
    lastAttempt' = NoAttempt,
  }

  /// The UI replaces the retained refs with `s`. The server does not validate
  /// them, so `s` may include refs that are stale now.
  action selectRefs(s: Set[str]): bool = all {
    canEmit,
    connected.contains("ui"),
    selection' = s.intersect(features),
    stale' = s.exclude(features),
    log' = emit(SelectionChanged, revision, "ui"),
    revision' = revision, features' = features, connected' = connected, cursor' = cursor,
    acked' = acked, delivered' = delivered, assistantBase' = assistantBase,
    lastAttempt' = lastAttempt,
  }

  action uiSelect = {
    nondet s = FEATURES.powerset().oneOf()
    selectRefs(s)
  }

  /// The user sends a message.
  action uiMessage = all {
    canEmit,
    connected.contains("ui"),
    log' = emit(Message, revision, "ui"),
    revision' = revision, features' = features, selection' = selection, stale' = stale,
    connected' = connected, cursor' = cursor, acked' = acked, delivered' = delivered,
    assistantBase' = assistantBase, lastAttempt' = lastAttempt,
  }

  /// A parameter override: bumps the revision, leaves the feature set alone.
  action uiParams = all {
    canEmit,
    connected.contains("ui"),
    revision' = revision + 1,
    log' = emit(ParamsChanged, revision + 1, "ui"),
    features' = features, selection' = selection, stale' = stale, connected' = connected,
    cursor' = cursor, acked' = acked, delivered' = delivered,
    assistantBase' = assistantBase, lastAttempt' = lastAttempt,
  }

  /// A script write, accepted only if `base` is the current revision. An
  /// accepted write re-resolves every retained ref against the new features.
  action setScript(who: str, base: int, newFeatures: Set[str]): bool =
    if (base == revision) all {
      canEmit,
      revision' = revision + 1,
      features' = newFeatures,
      selection' = refs.intersect(newFeatures),
      stale' = refs.exclude(newFeatures),
      log' = emit(ScriptChanged, revision + 1, who),
      lastAttempt' = Tried({ by: who, base: base, revBefore: revision, accepted: true }),
      connected' = connected, cursor' = cursor, acked' = acked, delivered' = delivered,
      assistantBase' = assistantBase,
    } else all {
      lastAttempt' = Tried({ by: who, base: base, revBefore: revision, accepted: false }),
      revision' = revision, features' = features, selection' = selection, stale' = stale,
      log' = log, connected' = connected, cursor' = cursor, acked' = acked,
      delivered' = delivered, assistantBase' = assistantBase,
    }

  /// The assistant reads the current revision.
  action assistantRefresh = all {
    assistantBase' = revision,
    revision' = revision, features' = features, selection' = selection, stale' = stale,
    log' = log, connected' = connected, cursor' = cursor, acked' = acked,
    delivered' = delivered, lastAttempt' = lastAttempt,
  }

  /// The assistant writes from the base it last read, which may be stale.
  action assistantSetScript = {
    nondet nf = FEATURES.powerset().oneOf()
    setScript("assistant", assistantBase, nf)
  }

  /// The CLI writes from any base up to the current revision.
  action cliSetScript = {
    nondet base = 0.to(revision).oneOf()
    nondet nf = FEATURES.powerset().oneOf()
    setScript("cli", base, nf)
  }

  /// `c` opens an event stream and continues from cursor `from`.
  action connectClient(c: str, from: int): bool = all {
    canEmit,
    not(connected.contains(c)),
    connected' = connected.union(Set(c)),
    log' = emit(ClientJoined, revision, c),
    cursor' = cursor.set(c, from),
    revision' = revision, features' = features, selection' = selection, stale' = stale,
    acked' = acked, delivered' = delivered, assistantBase' = assistantBase,
    lastAttempt' = lastAttempt,
  }

  /// `c` connects and keeps the cursor it holds itself.
  action joinClient(c: str): bool = connectClient(c, cursor.get(c))

  action clientJoin = {
    nondet c = CLIENTS.exclude(connected).oneOf()
    joinClient(c)
  }

  /// `c` reconnects from the cursor the server stored. It may see again what
  /// it already received; it does not miss what was sent while it was away.
  action resumeClient(c: str): bool = connectClient(c, acked.get(c))

  action clientResume = {
    nondet c = CLIENTS.exclude(connected).oneOf()
    resumeClient(c)
  }

  action leaveClient(c: str): bool = all {
    canEmit,
    connected' = connected.exclude(Set(c)),
    log' = emit(ClientLeft, revision, c),
    revision' = revision, features' = features, selection' = selection, stale' = stale,
    cursor' = cursor, acked' = acked, delivered' = delivered,
    assistantBase' = assistantBase, lastAttempt' = lastAttempt,
  }

  action clientLeave = {
    nondet c = connected.oneOf()
    leaveClient(c)
  }

  /// The stream delivers the next event (cursor + 1) to `c`.
  action sseDeliverTo(c: str): bool = {
    val nxt = cursor.get(c) + 1
    all {
      connected.contains(c),
      nxt <= log.length(),
      cursor' = cursor.set(c, nxt),
      delivered' = delivered.setBy(c, s => s.union(Set(nxt))),
      revision' = revision, features' = features, selection' = selection, stale' = stale,
      log' = log, connected' = connected, acked' = acked, assistantBase' = assistantBase,
      lastAttempt' = lastAttempt,
    }
  }

  action sseDeliver = {
    nondet c = connected.oneOf()
    sseDeliverTo(c)
  }

  /// The transport resends the last delivered event; the client dedupes by seq.
  action sseRedeliver = {
    nondet c = connected.oneOf()
    all {
      cursor.get(c) >= 1,
      delivered' = delivered.setBy(c, s => s.union(Set(cursor.get(c)))),
      revision' = revision, features' = features, selection' = selection, stale' = stale,
      log' = log, connected' = connected, cursor' = cursor, acked' = acked,
      assistantBase' = assistantBase, lastAttempt' = lastAttempt,
    }
  }

  /// `Ack`: the server stores how far `c` has consumed. A client cannot ack
  /// past what it was delivered, and a stored cursor never moves backwards.
  action ackClient(c: str, n: int): bool = all {
    n <= cursor.get(c),
    n >= acked.get(c),
    acked' = acked.set(c, n),
    revision' = revision, features' = features, selection' = selection, stale' = stale,
    log' = log, connected' = connected, cursor' = cursor, delivered' = delivered,
    assistantBase' = assistantBase, lastAttempt' = lastAttempt,
  }

  action clientAck = {
    nondet c = CLIENTS.oneOf()
    ackClient(c, cursor.get(c))
  }

  /// `cadpole serve` restarts: the in-memory log and the stored cursors are
  /// gone and every client re-reads. The document is reloaded unchanged.
  action serverRestart = all {
    log' = [],
    revision' = 0,
    connected' = Set(),
    cursor' = CLIENTS.mapBy(c => 0),
    acked' = CLIENTS.mapBy(c => 0),
    delivered' = CLIENTS.mapBy(c => Set()),
    assistantBase' = 0,
    lastAttempt' = NoAttempt,
    features' = features, selection' = selection, stale' = stale,
  }

  // ---- step

  action step = any {
    uiSelect,
    uiMessage,
    uiParams,
    assistantRefresh,
    assistantSetScript,
    cliSetScript,
    clientJoin,
    clientLeave,
    clientAck,
    clientResume,
    serverRestart,
    sseDeliver,
    sseRedeliver,
  }

  // ---- invariants

  /// The log is dense: the i-th event has seq i + 1.
  val seqMonotonic: bool =
    log.indices().forall(i => log[i].seq == i + 1)

  /// The revision equals the number of content events in the log.
  val revisionMatchesLog: bool =
    revision == log.select(e => e.kind == ScriptChanged or e.kind == ParamsChanged).length()

  /// A script write is accepted exactly when its base is the current revision.
  val staleBaseRejected: bool =
    match lastAttempt {
      | NoAttempt => true
      | Tried(a) => a.accepted == (a.base == a.revBefore)
    }

  /// Live refs name existing features, stale refs name none, and no ref is both.
  val selectionValid: bool = and {
    selection.forall(f => features.contains(f)),
    stale.forall(f => not(features.contains(f))),
    selection.intersect(stale) == Set(),
  }

  /// What each client has received is a gap-free prefix 1..n of the log.
  val deliveryIsPrefix: bool =
    CLIENTS.forall(c => delivered.get(c) == 1.to(delivered.get(c).size()))

  /// Everything up to a client's cursor has been delivered to it.
  val cursorCovered: bool =
    CLIENTS.forall(c => 1.to(cursor.get(c)).subseteq(delivered.get(c)))

  /// No cursor is past the end of the log, and the stored cursor is never
  /// past the client's own.
  val cursorBounded: bool =
    CLIENTS.forall(c => and {
      cursor.get(c) <= log.length(),
      acked.get(c) <= cursor.get(c),
      acked.get(c) <= log.length(),
    })

  val allInvariants: bool = and {
    seqMonotonic,
    revisionMatchesLog,
    staleBaseRejected,
    selectionValid,
    deliveryIsPrefix,
    cursorCovered,
    cursorBounded,
  }

  // ---- temporal properties
  //
  // Stated for `quint verify`; random simulation cannot check "eventually".
  // `session_progress.qnt` checks both with the bound written out, under a
  // scheduler that is fair within a bound.

  /// With fair delivery, a client that stays connected consumes the whole log.
  temporal catchUp: bool =
    weakFair(sseDeliver, (cursor, delivered)) implies
      always(CLIENTS.forall(c =>
        connected.contains(c) implies
          eventually(cursor.get(c) == log.length() or not(connected.contains(c)))))

  /// With fair delivery, every user message reaches an assistant that stays
  /// connected.
  temporal messagesReachTheAssistant: bool =
    weakFair(sseDeliver, (cursor, delivered)) implies
      always(log.indices().forall(i =>
        (log[i].kind == Message and connected.contains("assistant")) implies
          eventually(delivered.get("assistant").contains(log[i].seq)
                     or not(connected.contains("assistant")))))

  // ---- scenarios

  // A write from a stale base is rejected; after a refresh it is accepted.
  run conflictIsRejected =
    init
      .then(assistantRefresh)
      .then(setScript("cli", 0, Set("base", "lid")))
      .then(setScript("assistant", assistantBase, Set("base")))
      .expect(revision == 1 and features == Set("base", "lid") and staleBaseRejected)
      .then(assistantRefresh)
      .then(setScript("assistant", assistantBase, Set("base")))
      .expect(revision == 2 and features == Set("base"))

  // Removing a selected feature makes its ref stale; the ref is not dropped.
  run removedSelectionGoesStale =
    init
      .then(joinClient("ui"))
      .then(setScript("cli", 0, Set("base", "lid")))
      .then(selectRefs(Set("lid")))
      .then(setScript("cli", 1, Set("base")))
      .expect(selection == Set() and stale == Set("lid"))

  // A stale ref is live again once its feature returns.
  run staleRefRevivesWhenFeatureReturns =
    init
      .then(joinClient("ui"))
      .then(selectRefs(Set("base")))
      .then(setScript("cli", 0, Set("hole")))
      .expect(selection == Set() and stale == Set("base") and selectionValid)
      .then(setScript("cli", 1, Set("base", "hole", "lid")))
      .expect(selection == Set("base") and stale == Set() and selectionValid)

  // The UI may send refs that are stale now; they are kept as stale.
  run uiMayResendStaleRefs =
    init
      .then(joinClient("ui"))
      .then(setScript("cli", 0, Set("base", "lid")))
      .then(selectRefs(Set("base", "lid")))
      .then(setScript("cli", 1, Set("base")))
      .then(selectRefs(Set("lid", "hole")))
      .expect(selection == Set() and stale == Set("lid", "hole") and selectionValid)

  // A duplicate delivery leaves the client with a clean prefix.
  run redeliveryIsIdempotent =
    init
      .then(joinClient("ui"))
      .then(uiMessage)
      .then(sseDeliver)
      .then(sseRedeliver)
      .then(sseDeliver)
      .expect(deliveryIsPrefix and cursorBounded and cursorCovered)

  // A message sent while the assistant was disconnected is delivered after it
  // resumes from its acked cursor.
  run aMessageSentWhileTheAssistantWasAwayIsStillAhead =
    init
      .then(joinClient("ui"))              // seq 1
      .then(joinClient("assistant"))       // seq 2
      .then(sseDeliverTo("assistant"))
      .then(sseDeliverTo("assistant"))
      .then(ackClient("assistant", 2))
      .then(leaveClient("assistant"))      // seq 3
      .then(uiMessage)                     // seq 4: sent while it was away
      .then(resumeClient("assistant"))     // seq 5
      .expect(cursor.get("assistant") == 2 and acked.get("assistant") == 2)
      .then(sseDeliverTo("assistant"))
      .expect(cursor.get("assistant") == 3 and cursorCovered)
      .then(sseDeliverTo("assistant"))
      .expect(delivered.get("assistant").contains(4) and deliveryIsPrefix)

  // A client that never acked resumes from 0 and replays the log; it does not
  // skip to the tail.
  run withoutAnAckAClientReplaysRatherThanSkips =
    init
      .then(joinClient("ui"))
      .then(joinClient("assistant"))
      .then(sseDeliverTo("assistant"))
      .then(leaveClient("assistant"))
      .then(uiMessage)
      .then(resumeClient("assistant"))
      .expect(cursor.get("assistant") == 0 and cursorCovered and deliveryIsPrefix)

  // After a restart the log is empty and no cursor points into the old one.
  run aRestartIsANewEpoch =
    init
      .then(joinClient("ui"))
      .then(uiMessage)
      .then(sseDeliverTo("ui"))
      .then(ackClient("ui", 1))
      .expect(acked.get("ui") == 1)
      .then(serverRestart)
      .expect(log.length() == 0 and revision == 0)
      .expect(CLIENTS.forall(c => acked.get(c) == 0 and cursor.get(c) == 0))
      .expect(allInvariants)
}
