// Event delivery and wake-ups: what reaches an assistant, and when.
//
// Three programs wait on a session's event log for something worth a model
// turn and cost nothing while they wait (docs/assistant-integration.md):
//
//   watcher   `cadpole listen --next` / `--follow`: prints a line, which
//             wakes whatever watches it (src/rust/cadpole-cli/src/main.rs)
//   channel   the channel in `cadpole mcp`: sends a notification
//             (src/rust/cadpole-cli/src/mcp.rs)
//   listener  the limited listener's loop: starts a turn
//             (src/tools/listener/listener.mjs)
//
// All three read with `WaitEvents`, keep a cursor, store it on the server
// with `Ack`, and follow the UI's Listen toggle through the
// `assistant_listening` events in the log. A "hand-over" below is the one
// thing each exists to do: a line printed, a notification sent, a turn
// started. The model says which events are handed over, to whom, how often,
// and what a restart of a reader or of the server does to that.
//
// `eventlog.qnt` covers the log as a bounded window and `session.qnt` the
// cursor protocol in general; here the log is whole and the readers are
// these three. What a turn is, once started, is `listener.qnt`.
//
// What is not promised is in the "not invariants" section, each with a
// scenario that shows it.

module wakeups {
  // ---- constants

  pure val READERS: Set[str] = Set("watcher", "channel", "listener")

  // What the model transcribes from the three readers; //spec:names_test
  // checks each against the code.

  /// The id the server keeps a reader's cursor under. The watcher's and the
  /// listener's are fixed strings (the default of `--client`); the channel's
  /// is a prefix and its process's own id, so every start of `cadpole mcp`
  /// is a client the server has not seen.
  pure val WATCHER_CLIENT: str = "listen"
  pure val LISTENER_CLIENT: str = "listener"
  pure val CHANNEL_CLIENT_PREFIX: str = "mcp:"
  pure val CHANNEL_IDS: List[str] = ["mcp:1", "mcp:2", "mcp:3", "mcp:4"]

  /// The event kinds a hand-over is made for: `Request` is a `message` whose
  /// `from` is `user`, `Note` a `selection_changed` whose `note` is not
  /// empty. All three readers decide by these two and nothing else.
  pure val ACTIONABLE_KINDS: Set[str] = Set("message", "selection_changed")

  /// The kinds the watcher's long poll is filtered by on the server while it
  /// follows the Listen toggle (`releasesTheWatcher`).
  pure val WATCHER_POLL_KINDS: Set[str] = Set("message", "selection_changed", "assistant_listening")

  /// The field of an `assistant_listening` event that says what the toggle
  /// was before it (`Event.was`).
  pure val TOGGLE_WAS_FIELD: str = "was"

  pure val MAX_EVENTS: int = 7     // per generation of the server
  pure val MAX_GEN: int = 2

  // ---- types

  type Kind =
    | Request      // a `message` from the user
    | Note         // a `selection_changed` that carries a note
    | Reply        // a `message` from the assistant
    | Click        // a `selection_changed` without a note
    | Other        // anything else: a parameter, a script edit, a failure
                   // report, a solve event, a part of a streamed reply
    | ListenOn     // `assistant_listening`
    | ListenOff

  /// `on` is the Listen toggle the event was sent under (for a toggle, the
  /// new value); `was` is the toggle before it.
  type Event = { seq: int, kind: Kind, on: bool, was: bool }

  type Reader = {
    up: bool,
    client: str,
    gen: int,              // the server generation it is attached to
    cursor: int,
    hears: bool,           // the toggle as the reader knows it
    mode: str,             // watcher: "next" or "follow"
    printed: int,          // hand-overs in this run
    acked: bool,           // the server has had a cursor from this run
    starts: int,
    // The listener only: the rest of its batch, and the turn in flight.
    inbox: List[Event],
    batchEnd: int,
    flight: int,
  }

  /// An event handed over by a reader.
  /// `from` is where its client was owed events from at that moment.
  type Hand = {
    who: str, client: str, gen: int, seq: int, kind: Kind, sentWhileOn: bool, serverGen: int, from: int,
  }

  /// A reader's first sight of a log: everything after `from` is owed to it.
  type Join = { client: str, gen: int, from: int }

  type State = {
    // the server
    gen: int,
    log: List[Event],
    listenOn: bool,
    stored: str -> int,              // cursors by client id; none is 0
    // the readers
    readers: str -> Reader,
    alwaysListen: str -> bool,       // --always-listen, per reader
    bug: str,                        // "" in the intended design; see the mutants
    // history
    handed: List[Hand],
    atRisk: Set[{ client: str, gen: int, seq: int }],   // handed over, and the reader died before its ack
    joins: List[Join],
    asked: int,                      // actionable events ever posted
    repeats: int,                    // hand-overs put at risk
    idlePolls: int,                  // long polls that ended with nothing
  }

  /// What the last action was, for the replay.
  type Did = { name: str, who: str, kind: str, mode: str }

  // ---- state

  var s: State
  var did: Did

  // ---- helpers

  pure def actionable(k: Kind): bool = k == Request or k == Note

  pure def isToggle(k: Kind): bool = k == ListenOn or k == ListenOff

  /// The kinds the watcher's long poll is filtered by on the server
  /// (`ListenFilter::server_kinds`): `message` and `selection_changed`, and
  /// `assistant_listening` when the toggle is followed.
  pure def releasesTheWatcher(k: Kind, gated: bool): bool =
    k == Request or k == Note or k == Reply or k == Click or (gated and isToggle(k))

  pure def maxOf(a: int, b: int): int = if (a > b) a else b

  pure def lastSeq(st: State): int = st.log.length()

  pure def heldFor(st: State, client: str): int =
    if (st.stored.keys().contains(client)) st.stored.get(client) else 0

  /// `Ack`: the stored cursor never moves backwards and never passes the log.
  pure def ack(st: State, client: str, cursor: int): State = {
    val clamped = if (cursor > lastSeq(st)) lastSeq(st) else cursor
    { ...st, stored: st.stored.put(client, maxOf(heldFor(st, client), clamped)) }
  }

  pure def backlog(st: State, who: str): List[Event] =
    st.log.select(e => e.seq > st.readers.get(who).cursor)

  /// Whether a reader's long poll returns at once: something it is filtered
  /// by is in the log after its cursor. Otherwise it returns when it expires.
  pure def released(st: State, who: str): bool = {
    val gated = not(st.alwaysListen.get(who))
    if (who == "watcher") backlog(st, who).select(e => releasesTheWatcher(e.kind, gated)).length() > 0
    else backlog(st, who).length() > 0
  }

  pure def current(st: State, who: str): bool =
    st.readers.get(who).up and st.readers.get(who).gen == st.gen

  /// What a reader makes of a batch: the events it hands over, and the
  /// toggle as it knows it afterwards. Each event is judged by the toggle it
  /// was sent under: a toggle event changes it for what follows, and the
  /// first one in the batch says what it was for what came before.
  pure def judge(st: State, who: str, batch: List[Event]): { hears: bool, out: List[Event] } = {
    val r = st.readers.get(who)
    val free = st.alwaysListen.get(who)
    val seen =
      if (who == "watcher") batch.select(e => releasesTheWatcher(e.kind, not(free))) else batch
    val toggles = seen.select(e => isToggle(e.kind))
    val before =
      if (toggles.length() > 0 and st.bug != "trustsTheToggleAsItIsNow") toggles[0].was else r.hears
    seen.foldl({ hears: before, out: [] }, (acc, e) =>
      if (isToggle(e.kind)) { ...acc, hears: e.on }
      else if (not(acc.hears) and not(free)) acc
      else if (actionable(e.kind) or (st.bug == "wakesOnEveryEvent" and who == "channel"))
        { ...acc, out: acc.out.append(e) }
      else acc)
  }

  /// Where a client is owed events from, in a generation: where it last
  /// came to that log as a client the server held no cursor for. -1 when it
  /// never attached there. (A reader holds a place only once the server
  /// stores a cursor above zero for it: see the not-invariants.)
  pure def owedSince(st: State, client: str, gen: int): int =
    st.joins.select(j => j.client == client and j.gen == gen).foldl(-1, (acc, j) => j.from)

  pure def handOf(st: State, who: str, e: Event): Hand = {
    from: owedSince(st, st.readers.get(who).client, st.readers.get(who).gen),
    who: who,
    client: st.readers.get(who).client,
    gen: st.readers.get(who).gen,
    seq: e.seq,
    kind: e.kind,
    sentWhileOn: e.on,
    serverGen: st.gen,
  }

  /// A reader starts and attaches: `Ack` with cursor 0 asks what the server
  /// holds for its id. A client it has not seen adopts the end of the log;
  /// the watcher and the listener record that at once.
  pure def attach(st: State, who: str, mode: str): State = {
    val r = st.readers.get(who)
    val client = if (who == "channel") CHANNEL_IDS[r.starts] else r.client
    val held = heldFor(st, client)
    val isNew = held == 0
    val at =
      if (not(isNew)) held
      else if (st.bug == "aNewClientReadsFromTheStart") 0
      else lastSeq(st)
    val records = isNew and at > 0 and who != "channel" and st.bug != "doesNotRecordTheAdoptedEnd"
    val recorded = if (records) ack(st, client, at) else st
    {
      ...recorded,
      joins: if (isNew) st.joins.append({ client: client, gen: st.gen, from: lastSeq(st) }) else st.joins,
      readers: st.readers.set(who, {
        ...r,
        up: true, client: client, gen: st.gen, cursor: at, hears: st.listenOn, mode: mode,
        printed: 0, acked: records, starts: r.starts + 1, inbox: [], batchEnd: at, flight: 0,
      }),
    }
  }

  /// One long poll of the watcher or the channel, and what follows: the
  /// batch is handed over, then the cursor is stored. With `acked` false the
  /// reader dies in between (or its ack is lost and it exits), and what it
  /// handed over is at risk of being handed over again.
  pure def pollOnce(st: State, who: str, acked: bool): State = {
    val r = st.readers.get(who)
    val batch = backlog(st, who)
    val verdict = judge(st, who, batch)
    val cursor = if (batch.length() > 0) batch[batch.length() - 1].seq else r.cursor
    val hands = verdict.out.foldl([], (acc, e) => acc.append(handOf(st, who, e)))
    val printed = r.printed + hands.length()
    val after = {
      ...st,
      handed: st.handed.concat(hands),
      idlePolls: if (batch.length() == 0) st.idlePolls + 1 else st.idlePolls,
    }
    if (acked) {
      // The watcher acks when the cursor moved; the channel on every round.
      val stored = if (cursor > r.cursor or who == "channel") ack(after, r.client, cursor) else after
      {
        ...stored,
        readers: stored.readers.set(who, {
          ...r, cursor: cursor, hears: verdict.hears, printed: printed,
          acked: r.acked or cursor > r.cursor,
          // `--next` exits once a batch printed something.
          up: not(who == "watcher" and r.mode == "next" and printed > 0),
        }),
      }
    } else {
      ...after,
      atRisk: after.atRisk.union(hands.foldl(Set(), (acc, h) =>
        acc.union(Set({ client: h.client, gen: h.gen, seq: h.seq })))),
      repeats: after.repeats + hands.length(),
      readers: after.readers.set(who, { ...r, up: false }),
    }
  }

  /// The listener works through its batch until a turn starts or the batch
  /// is done (`Listener.step`).
  pure def listenerSettles(st: State): State =
    range(0, MAX_EVENTS + 1).foldl(st, (acc, _) => {
      val r = acc.readers.get("listener")
      if (r.flight > 0 or r.batchEnd == r.cursor and r.inbox.length() == 0) acc
      else if (r.inbox.length() == 0)
        // The batch is done: the cursor covers everything examined.
        { ...ack(acc, r.client, r.batchEnd), readers: acc.readers.set("listener", { ...r, cursor: r.batchEnd }) }
      else {
        val e = r.inbox.head()
        val rest = { ...r, inbox: r.inbox.tail() }
        if (isToggle(e.kind)) { ...acc, readers: acc.readers.set("listener", { ...rest, hears: e.on }) }
        else if (actionable(e.kind) and (rest.hears or acc.alwaysListen.get("listener")))
          {
            ...acc,
            handed: acc.handed.append(handOf(acc, "listener", e)),
            readers: acc.readers.set("listener", { ...rest, flight: e.seq, printed: rest.printed + 1 }),
          }
        else { ...acc, readers: acc.readers.set("listener", rest) }
      }
    })

  /// What the watcher and the channel do by themselves once an event is in
  /// the log: a reader that is up polls at once when the event releases its
  /// long poll. The replay drives real processes, which do exactly this, so
  /// its steps are an action followed by these polls.
  pure def eager(st: State): State =
    ["watcher", "channel"].foldl(st, (acc, who) =>
      if (current(acc, who) and released(acc, who)) pollOnce(acc, who, true) else acc)

  pure def reader0(client: str): Reader = {
    up: false, client: client, gen: 0, cursor: 0, hears: false, mode: "", printed: 0, acked: false, starts: 0,
    inbox: [], batchEnd: 0, flight: 0,
  }

  pure def initial(free: str -> bool, bug: str): State = {
    gen: 1,
    log: [],
    listenOn: false,
    stored: Map(),
    readers: Map(
      "watcher" -> reader0(WATCHER_CLIENT), "channel" -> reader0(""), "listener" -> reader0(LISTENER_CLIENT)),
    alwaysListen: free,
    bug: bug,
    handed: [],
    atRisk: Set(),
    joins: [],
    asked: 0,
    repeats: 0,
    idlePolls: 0,
  }

  pure def note(name: str, who: str): Did = { name: name, who: who, kind: "", mode: "" }

  pure def kindName(k: Kind): str =
    match k {
      | Request => "request"
      | Note => "note"
      | Reply => "reply"
      | Click => "click"
      | Other => "other"
      | ListenOn => "listen-on"
      | ListenOff => "listen-off"
    }

  pure def post(st: State, kind: Kind): State = {
    val on = if (kind == ListenOn) true else if (kind == ListenOff) false else st.listenOn
    {
      ...st,
      log: st.log.append({ seq: lastSeq(st) + 1, kind: kind, on: on, was: st.listenOn }),
      listenOn: on,
      asked: if (actionable(kind)) st.asked + 1 else st.asked,
    }
  }

  pure def restarted(st: State): State =
    // The log and the stored cursors are in memory, and the toggle starts off.
    { ...st, gen: st.gen + 1, log: [], listenOn: false, stored: Map() }

  /// The watcher finds a new session behind the address: it reads the new
  /// log from its start, with the toggle as it is now.
  pure def reconnected(st: State): State = {
    val r = st.readers.get("watcher")
    {
      ...st,
      joins: st.joins.append({ client: r.client, gen: st.gen, from: 0 }),
      readers: st.readers.set("watcher", {
        ...r,
        gen: st.gen,
        cursor: if (st.bug == "keepsItsCursorAcrossAServerRestart") r.cursor else 0,
        hears: st.listenOn,
      }),
    }
  }

  pure def killed(st: State, who: str): State = {
    val r = st.readers.get(who)
    val cut = who == "listener" and r.flight > 0
    {
      ...st,
      // A turn in flight was handed over and never recorded.
      atRisk: if (cut) st.atRisk.union(Set({ client: r.client, gen: r.gen, seq: r.flight })) else st.atRisk,
      repeats: if (cut) st.repeats + 1 else st.repeats,
      readers: st.readers.set(who, { ...r, up: false, inbox: [], flight: 0, batchEnd: r.cursor }),
    }
  }

  pure def listenerPolled(st: State): State = {
    val r = st.readers.get("listener")
    val batch = backlog(st, "listener")
    val toggles = batch.select(e => isToggle(e.kind))
    listenerSettles({
      ...st,
      idlePolls: if (batch.length() == 0) st.idlePolls + 1 else st.idlePolls,
      readers: st.readers.set("listener", {
        ...r,
        inbox: batch,
        batchEnd: if (batch.length() > 0) batch[batch.length() - 1].seq else r.cursor,
        hears:
          if (toggles.length() > 0 and st.bug != "trustsTheToggleAsItIsNow") toggles[0].was else r.hears,
      }),
    })
  }

  pure def turnEnded(st: State): State = {
    val r = st.readers.get("listener")
    // The position is recorded after every event acted on.
    val recorded = if (st.bug == "recordsItsPlaceOncePerBatch") st else ack(st, r.client, r.flight)
    listenerSettles({
      ...recorded,
      readers: recorded.readers.set("listener", {
        ...r,
        flight: 0,
        cursor: if (st.bug == "recordsItsPlaceOncePerBatch") r.cursor else r.flight,
      }),
    })
  }

  // ---- actions
  //
  // Each takes `react`: the function applied to the state after the action
  // itself. `step` passes the identity, so a poll is an action of its own
  // and may come at any time; `stepReplay` passes `eager`.

  action initWith(bug: str): bool = {
    nondet w = Set(false, true).oneOf()
    nondet c = Set(false, true).oneOf()
    nondet l = Set(false, true).oneOf()
    all {
      s' = initial(Map("watcher" -> w, "channel" -> c, "listener" -> l), bug),
      did' = note("init", ""),
    }
  }

  action init = initWith("")

  action uiDoes(kind: Kind, react: State => State): bool = all {
    lastSeq(s) < MAX_EVENTS,
    s' = react(post(s, kind)),
    did' = { ...note("uiDoes", ""), kind: kindName(kind) },
  }

  action ui(react: State => State): bool = {
    nondet pick = 1.to(12).oneOf()
    uiDoes(
      if (pick <= 4) Request
      else if (pick == 5) Note
      else if (pick == 6) Reply
      else if (pick == 7) Click
      else if (pick == 8) Other
      else if (pick <= 10) ListenOn
      else ListenOff,
      react)
  }

  action serverRestarts(react: State => State): bool = all {
    s.gen < MAX_GEN,
    s' = react(restarted(s)),
    did' = note("serverRestarts", ""),
  }

  action starts(who: str, mode: str, react: State => State): bool = all {
    not(s.readers.get(who).up),
    s.readers.get(who).starts < CHANNEL_IDS.length(),
    s' = react(attach(s, who, mode)),
    did' = { ...note("starts", who), mode: mode },
  }

  action anyStart(react: State => State): bool = {
    nondet who = READERS.oneOf()
    nondet mode = Set("next", "follow").oneOf()
    starts(who, if (who == "watcher") mode else "", react)
  }

  action kills(who: str): bool = all {
    s.readers.get(who).up,
    s' = killed(s, who),
    did' = note("kills", who),
  }

  action anyKill = {
    nondet who = READERS.oneOf()
    kills(who)
  }

  /// A long poll of the watcher or the channel returns, by an event or by
  /// its expiry, and the reader goes through the batch and stores its cursor.
  action polls(who: str): bool = all {
    who != "listener",
    current(s, who),
    s' = pollOnce(s, who, true),
    did' = note("polls", who),
  }

  /// The same, and the reader dies after handing the batch over and before
  /// its cursor is stored.
  action pollsAndDies(who: str): bool = all {
    who != "listener",
    current(s, who),
    judge(s, who, backlog(s, who)).out.length() > 0,
    s' = pollOnce(s, who, false),
    did' = note("pollsAndDies", who),
  }

  action anyPoll = {
    nondet who = Set("watcher", "channel").oneOf()
    polls(who)
  }

  action anyPollAndDeath = {
    nondet who = Set("watcher", "channel").oneOf()
    pollsAndDies(who)
  }

  action watcherReconnects(react: State => State): bool = all {
    s.readers.get("watcher").up,
    s.readers.get("watcher").gen != s.gen,
    s' = react(reconnected(s)),
    did' = note("watcherReconnects", "watcher"),
  }

  /// The listener's long poll returns, and it works through the batch until
  /// a turn starts.
  action listenerPolls = all {
    current(s, "listener"),
    s.readers.get("listener").flight == 0,
    s' = listenerPolled(s),
    did' = note("listenerPolls", "listener"),
  }

  /// The turn in flight ends, and the listener carries on with its batch.
  action listenerTurnEnds = all {
    current(s, "listener"),
    s.readers.get("listener").flight > 0,
    s' = turnEnded(s),
    did' = note("listenerTurnEnds", "listener"),
  }

  /// A listener or a channel that was attached when the server restarted
  /// holds the token of the run that is gone: every call it makes is
  /// refused, so it hands over nothing more until it is started again.
  action listenerPollFails = all {
    s.readers.get("listener").up,
    s.readers.get("listener").gen != s.gen,
    s.readers.get("listener").flight == 0,
    s' = s,
    did' = note("listenerPollFails", "listener"),
  }

  // ---- step

  pure def asIs(st: State): State = st

  // A branch listed more often is taken more often by the simulator.
  action step = any {
    ui(asIs), ui(asIs), ui(asIs), ui(asIs),
    anyStart(asIs), anyStart(asIs), anyStart(asIs),
    anyPoll, anyPoll, anyPoll, anyPoll,
    listenerPolls, listenerPolls,
    listenerTurnEnds, listenerTurnEnds,
    anyPollAndDeath,
    anyKill,
    serverRestarts(asIs),
    watcherReconnects(asIs),
    listenerPollFails,
  }

  /// A reader started on a log that holds something. On an empty log
  /// nothing outside the reader shows when it has attached, and whether an
  /// event sent at that moment is before or after its start is a race the
  /// replay cannot decide.
  action startOnALog = all {
    lastSeq(s) > 0,
    anyStart(eager),
  }

  /// A restart of the server while a reader is attached, which is when it
  /// has something to show. A watcher that is running has been heard from by
  /// then: until it stores a cursor nothing outside it shows that it has
  /// attached, and one that attaches only after the restart is a new client
  /// of the new server, not one that ran across it.
  action restartUnderAReader = all {
    READERS.exists(who => s.readers.get(who).up),
    not(s.readers.get("watcher").up) or s.readers.get("watcher").acked,
    lastSeq(s) >= 3,
    serverRestarts(eager),
  }

  /// The steps a replay can make real processes take: the watcher and the
  /// channel poll as soon as there is something for them, as the programs
  /// do. Every such step is one step of `step` followed by `polls` steps.
  action stepReplay = any {
    ui(eager), ui(eager), ui(eager), ui(eager), ui(eager),
    ui(eager), ui(eager), ui(eager), ui(eager), ui(eager),
    startOnALog, startOnALog, startOnALog,
    listenerPolls, listenerPolls,
    listenerTurnEnds, listenerTurnEnds, listenerTurnEnds, listenerTurnEnds,
    anyKill,
    restartUnderAReader,
    watcherReconnects(eager), watcherReconnects(eager), watcherReconnects(eager),
    listenerPollFails,
  }

  // ---- invariants

  def owedFrom(client: str, gen: int): int = owedSince(s, client, gen)

  def wasHanded(client: str, gen: int, seq: int): bool =
    s.handed.select(h => h.client == client and h.gen == gen and h.seq == seq).length() > 0

  /// Only a message from the user and a selection that carries a note are
  /// handed over, by any reader. A bare selection, the assistant's own
  /// reply, a parameter or script change, a failure report, a solve event,
  /// a part of a streamed reply and the toggle itself never are.
  val onlyActionableEventsAreHandedOver: bool =
    s.handed.indices().forall(i => actionable(s.handed[i].kind))

  /// Nothing is handed over that was sent while Listen was off, unless the
  /// reader was started with --always-listen: also not later, once Listen
  /// is on again.
  val everyHandOverWasSentWhileListening: bool =
    s.handed.indices().forall(i =>
      s.handed[i].sentWhileOn or s.alwaysListen.get(s.handed[i].who))

  /// A client id is handed an event at most once. The exception is an event
  /// whose reader died after handing it over and before its cursor reached
  /// the server: the next reader under that id hands it over again.
  val handedOverAtMostOnceUnlessInterrupted: bool =
    s.handed.indices().forall(i => s.handed.indices().forall(j => {
      val a = s.handed[i]
      val b = s.handed[j]
      (i < j and a.client == b.client and a.gen == b.gen and a.seq == b.seq) implies
        s.atRisk.contains({ client: a.client, gen: a.gen, seq: a.seq })
    }))

  /// Nothing is passed over: once the server's cursor for the watcher or
  /// the listener is at or past an actionable event that was sent while
  /// Listen was on, after where that client started, the event was handed
  /// over. This is what a restart of the reader, and the gap between two
  /// `listen --next` runs, must not lose.
  val nothingActionableIsSkipped: bool =
    Set("watcher", "listener").forall(who => {
      val client = s.readers.get(who).client
      s.log.indices().forall(i => {
        val e = s.log[i]
        val owed = and {
          actionable(e.kind),
          e.on or s.alwaysListen.get(who),
          owedFrom(client, s.gen) >= 0,
          e.seq > owedFrom(client, s.gen),
          e.seq <= heldFor(s, client),
        }
        owed implies wasHanded(client, s.gen, e.seq)
      })
    })

  /// A client the server has not seen starts at the end of the log: nothing
  /// from before it first attached is handed to it.
  val aNewClientStartsAtTheEnd: bool =
    s.handed.indices().forall(i => s.handed[i].from >= 0 and s.handed[i].seq > s.handed[i].from)

  /// The watcher and the listener are new to a log once: after adopting its
  /// end they hold a place there, and a later run resumes from it.
  val aReaderAdoptsTheEndOnce: bool =
    Set(WATCHER_CLIENT, LISTENER_CLIENT).forall(client => 1.to(MAX_GEN).forall(g =>
      s.joins.select(j => j.client == client and j.gen == g and j.from > 0).length() <= 1))

  /// Every start of the channel is a new client: it keeps no place across
  /// its own restart, so it hands over nothing from before it started.
  val aRestartedChannelStartsAtTheEnd: bool = and {
    // One client id per start, each a client the server had not seen.
    CHANNEL_IDS.indices().forall(k =>
      s.joins.select(j => j.client == CHANNEL_IDS[k]).length() == (if (k < s.readers.get("channel").starts) 1 else 0)),
    s.handed.indices().forall(i =>
      (s.handed[i].who == "channel") implies (s.handed[i].seq > s.handed[i].from)),
  }

  /// Across a restart of the server nothing is handed over twice and nothing
  /// is mistaken for something else: an event is handed over only by a
  /// reader attached to the log it is in, while that log is the server's.
  val everyHandOverIsFromTheLogItWasReadFrom: bool =
    s.handed.indices().forall(i => s.handed[i].gen == s.handed[i].serverGen)

  /// Waiting costs nothing: a reader hands over at most once per actionable
  /// event the user sent (and once more per hand-over put at risk), however
  /// many long polls expire in between.
  val nothingIsHandedOverWhileNothingIsAsked: bool =
    READERS.forall(who =>
      s.handed.select(h => h.who == who).length() <= s.asked + s.repeats)

  /// No cursor is past the end of the log it counts in.
  val noCursorIsPastTheLog: bool = and {
    s.stored.keys().forall(c => s.stored.get(c) <= lastSeq(s)),
    READERS.forall(who => current(s, who) implies s.readers.get(who).cursor <= lastSeq(s)),
  }

  /// The listener has one turn in flight at most, and only while it is up.
  val aTurnIsInFlightOnlyWhileTheListenerIsUp: bool =
    (s.readers.get("listener").flight > 0) implies s.readers.get("listener").up

  val allInvariants: bool = and {
    onlyActionableEventsAreHandedOver,
    everyHandOverWasSentWhileListening,
    handedOverAtMostOnceUnlessInterrupted,
    nothingActionableIsSkipped,
    aNewClientStartsAtTheEnd,
    aReaderAdoptsTheEndOnce,
    aRestartedChannelStartsAtTheEnd,
    everyHandOverIsFromTheLogItWasReadFrom,
    nothingIsHandedOverWhileNothingIsAsked,
    noCursorIsPastTheLog,
    aTurnIsInFlightOnlyWhileTheListenerIsUp,
  }

  // ---- not invariants
  //
  // Properties that do not hold, each with a scenario below that reaches a
  // state where it is false.

  /// Every actionable event sent while Listen was on reaches every reader
  /// that was ever started. False across a restart of the server (what was
  /// unread in the old log is gone, and a reader started afterwards skips
  /// what the new log already holds) and across a restart of the channel.
  def everyRequestReaches(who: str): bool =
    s.log.indices().forall(i =>
      (actionable(s.log[i].kind) and s.log[i].on) implies
        s.handed.select(h => h.who == who and h.gen == s.gen and h.seq == s.log[i].seq).length() > 0)

  /// A reader that has attached to a log holds a place in it. False for one
  /// that attached while the log was empty and read nothing: there is no
  /// cursor above zero to store, so its next start is a new client again and
  /// begins at the end, past what arrived in between.
  def everyRequestSinceItFirstAttachedReaches(who: str): bool = {
    val client = s.readers.get(who).client
    val first = s.joins.select(j => j.client == client and j.gen == s.gen)
      .foldl(-1, (acc, j) => if (acc < 0) j.from else acc)
    s.log.indices().forall(i => {
      val e = s.log[i]
      (actionable(e.kind) and e.on and first >= 0 and e.seq > first and e.seq <= heldFor(s, client)) implies
        wasHanded(client, s.gen, e.seq)
    })
  }

  /// An event is handed over once, whoever reads. False with two readers:
  /// each has its own cursor.
  val oneHandOverPerEvent: bool =
    s.handed.indices().forall(i => s.handed.indices().forall(j =>
      (i < j) implies not(s.handed[i].gen == s.handed[j].gen and s.handed[i].seq == s.handed[j].seq)))

  // ---- mutants
  //
  // The intended design with one thing done wrong; each is simulated in the
  // tests with the expectation that the named invariant fails.

  /// Judges a backlog by the toggle as it is when the reader attaches:
  /// `everyHandOverWasSentWhileListening`.
  action initTrustsTheToggleAsItIsNow = initWith("trustsTheToggleAsItIsNow")

  /// The same, seen from a message typed while Listen was on:
  /// `nothingActionableIsSkipped`.
  action initPassesOverWhatWasSentBeforeListenWentOff = initWith("trustsTheToggleAsItIsNow")

  /// The channel pushes every event it can describe:
  /// `onlyActionableEventsAreHandedOver`.
  action initWakesOnEveryEvent = initWith("wakesOnEveryEvent")

  /// A client the server has not seen replays the log:
  /// `aNewClientStartsAtTheEnd`.
  action initANewClientReadsFromTheStart = initWith("aNewClientReadsFromTheStart")

  /// A new client adopts the end of the log without telling the server, so
  /// its next run is new again and jumps past what arrived in between:
  /// `aReaderAdoptsTheEndOnce`.
  action initDoesNotRecordTheAdoptedEnd = initWith("doesNotRecordTheAdoptedEnd")

  /// The watcher carries its cursor into the restarted server's log:
  /// `nothingActionableIsSkipped`.
  action initKeepsItsCursorAcrossAServerRestart = initWith("keepsItsCursorAcrossAServerRestart")

  /// The listener stores its cursor once per batch, so dying in a later turn
  /// repeats the turns before it: `handedOverAtMostOnceUnlessInterrupted`.
  action initRecordsItsPlaceOncePerBatch = initWith("recordsItsPlaceOncePerBatch")

  // ---- scenarios

  action begin(listenOn: bool, bug: str): bool = all {
    s' = {
      ...initial(Map("watcher" -> false, "channel" -> false, "listener" -> false), bug),
      listenOn: listenOn,
    },
    did' = note("init", ""),
  }

  def handedBy(who: str): List[int] =
    s.handed.select(h => h.who == who).foldl([], (acc, h) => acc.append(h.seq))

  // A message sent between two `listen --next` runs is printed by the next
  // run, once.
  run nothingIsLostBetweenTwoNextRuns =
    begin(true, "")
      .then(uiDoes(Other, asIs))                    // 1
      .then(starts("watcher", "next", asIs))        // new: adopts 1
      .expect(heldFor(s, "listen") == 1)
      .then(uiDoes(Request, asIs))                  // 2
      .then(polls("watcher"))
      .expect(handedBy("watcher") == [2] and not(s.readers.get("watcher").up))
      .then(uiDoes(Request, asIs))                  // 3: nobody is waiting
      .then(uiDoes(Other, asIs))                    // 4
      .then(starts("watcher", "next", asIs))
      .expect(s.readers.get("watcher").cursor == 2)
      .then(polls("watcher"))
      .expect(handedBy("watcher") == [2, 3] and heldFor(s, "listen") == 4)
      .expect(allInvariants)

  // Waiting costs nothing: with nothing asked, any number of polls hands
  // over nothing, and a parameter change or a bare selection neither.
  run waitingHandsOverNothing =
    begin(true, "")
      .then(uiDoes(Other, asIs))
      .then(starts("watcher", "follow", asIs))
      .then(starts("channel", "", asIs))
      .then(starts("listener", "", asIs))
      .then(polls("watcher")).then(polls("channel")).then(listenerPolls)
      .then(uiDoes(Click, asIs)).then(uiDoes(Other, asIs)).then(uiDoes(Reply, asIs))
      .then(polls("watcher")).then(polls("channel")).then(listenerPolls)
      .then(polls("watcher")).then(polls("channel")).then(listenerPolls)
      .expect(s.handed == [] and s.idlePolls >= 6)
      .expect(heldFor(s, "listen") == 4 and heldFor(s, "listener") == 4)
      .expect(allInvariants)

  // A backlog is judged by the toggle each event was sent under: what was
  // typed while Listen was off stays unprinted after it is turned on, and
  // what was typed while it was on is printed after it is turned off.
  run aBacklogIsJudgedByTheToggleEachEventWasSentUnder =
    begin(false, "")
      .then(uiDoes(Other, asIs))                    // 1
      .then(starts("watcher", "next", asIs))
      .then(kills("watcher"))
      .then(uiDoes(Request, asIs))                  // 2: Listen is off
      .then(uiDoes(ListenOn, asIs))                 // 3
      .then(uiDoes(Request, asIs))                  // 4: Listen is on
      .then(uiDoes(ListenOff, asIs))                // 5
      .then(starts("watcher", "next", asIs))
      .expect(not(s.readers.get("watcher").hears))
      .then(polls("watcher"))
      .expect(handedBy("watcher") == [4] and heldFor(s, "listen") == 5)
      .expect(allInvariants)

  // The same backlog read by the toggle as it is when the reader attaches:
  // the message typed while Listen was off is printed.
  run trustingTheToggleAsItIsNowHandsOverWhatWasTypedWhileOff =
    begin(false, "trustsTheToggleAsItIsNow")
      .then(uiDoes(Other, asIs))
      .then(starts("watcher", "next", asIs))
      .then(kills("watcher"))
      .then(uiDoes(Request, asIs))                  // 2: Listen is off
      .then(uiDoes(ListenOn, asIs))                 // 3
      .then(starts("watcher", "next", asIs))
      .then(polls("watcher"))
      .expect(handedBy("watcher") == [2])
      .expect(not(everyHandOverWasSentWhileListening))

  // A reader that dies between handing a batch over and storing its cursor
  // hands the batch over again; that is the only repeat.
  run aReaderThatDiesBeforeItsAckRepeatsTheBatch =
    begin(true, "")
      .then(uiDoes(Other, asIs))
      .then(starts("watcher", "follow", asIs))
      .then(uiDoes(Request, asIs))                  // 2
      .then(pollsAndDies("watcher"))
      .expect(handedBy("watcher") == [2] and heldFor(s, "listen") == 1)
      .then(starts("watcher", "follow", asIs))
      .then(polls("watcher"))
      .expect(handedBy("watcher") == [2, 2] and heldFor(s, "listen") == 2)
      .expect(allInvariants)

  // A listener that dies in the second turn of a batch repeats that turn
  // and not the first.
  run aListenerThatDiesInATurnRepeatsOnlyThatTurn =
    begin(true, "")
      .then(uiDoes(Other, asIs))
      .then(starts("listener", "", asIs))
      .then(uiDoes(Request, asIs))                  // 2
      .then(uiDoes(Request, asIs))                  // 3
      .then(listenerPolls)
      .then(listenerTurnEnds)
      .expect(heldFor(s, "listener") == 2 and s.readers.get("listener").flight == 3)
      .then(kills("listener"))
      .then(starts("listener", "", asIs))
      .then(listenerPolls)
      .expect(handedBy("listener") == [2, 3, 3])
      .then(listenerTurnEnds)
      .expect(heldFor(s, "listener") == 3 and allInvariants)

  // A restarted channel is a new client: what was sent while none was
  // running is not delivered.
  run aRestartedChannelMissesWhatWasSentMeanwhile =
    begin(true, "")
      .then(uiDoes(Other, asIs))
      .then(starts("channel", "", asIs))
      .then(uiDoes(Request, asIs))                  // 2
      .then(polls("channel"))
      .then(kills("channel"))
      .then(uiDoes(Request, asIs))                  // 3: no channel is running
      .then(starts("channel", "", asIs))
      .then(polls("channel"))
      .expect(handedBy("channel") == [2] and s.readers.get("channel").client == "mcp:2")
      .expect(not(everyRequestReaches("channel")))
      .expect(allInvariants)

  // A reader that attached to an empty log and read nothing has no place in
  // it: started again, it begins at the end, past a message sent in between.
  run aReaderThatSawOnlyAnEmptyLogIsNewAgain =
    begin(true, "")
      .then(starts("listener", "", asIs))
      .expect(s.stored == Map())
      .then(kills("listener"))
      .then(uiDoes(Request, asIs))                  // 1: no listener is running
      .then(starts("listener", "", asIs))
      .expect(heldFor(s, "listener") == 1)
      .then(listenerPolls)
      .expect(s.handed == [] and not(everyRequestSinceItFirstAttachedReaches("listener")))
      .expect(allInvariants)

  // Two readers on one session each hand the message over.
  run twoReadersEachHandTheMessageOver =
    begin(true, "")
      .then(uiDoes(Other, asIs))
      .then(starts("watcher", "follow", asIs))
      .then(starts("channel", "", asIs))
      .then(uiDoes(Request, asIs))
      .then(polls("watcher"))
      .then(polls("channel"))
      .expect(s.handed.length() == 2 and not(oneHandOverPerEvent))
      .expect(allInvariants)

  // A restart of the server. What the watcher had not read of the old log
  // is lost with the log. Running across the restart, it reads the new log
  // from its start, so nothing of the new log is lost and nothing is
  // repeated.
  run aWatcherRunningAcrossARestartReadsTheNewLogFromItsStart =
    begin(true, "")
      .then(uiDoes(Other, asIs))
      .then(starts("watcher", "follow", asIs))
      .then(uiDoes(Request, asIs))                  // old log, seq 2: never read
      .then(serverRestarts(asIs))
      .expect(s.log == [] and s.stored == Map() and not(s.listenOn))
      .then(uiDoes(ListenOn, asIs))                 // new log, 1
      .then(uiDoes(Request, asIs))                  // new log, 2
      .then(watcherReconnects(asIs))
      .expect(s.readers.get("watcher").cursor == 0)
      .then(polls("watcher"))
      .expect(s.handed.length() == 1 and s.handed[0].gen == 2 and s.handed[0].seq == 2)
      .expect(allInvariants)

  // A reader started after the restart is a new client there: it starts at
  // the end of the new log and does not see what was sent before it came.
  run aReaderStartedAfterARestartSkipsWhatTheNewLogHolds =
    begin(true, "")
      .then(uiDoes(Other, asIs))
      .then(starts("watcher", "next", asIs))
      .then(kills("watcher"))
      .then(serverRestarts(asIs))
      .then(uiDoes(ListenOn, asIs))
      .then(uiDoes(Request, asIs))                  // new log, 2
      .then(starts("watcher", "next", asIs))
      .expect(s.readers.get("watcher").cursor == 2 and heldFor(s, "listen") == 2)
      .then(polls("watcher"))
      .expect(s.handed == [] and not(everyRequestReaches("watcher")))
      .expect(allInvariants)

  // A listener that was attached when the server restarted hands over
  // nothing more: its calls are refused until it is started again, and then
  // it is a new client.
  run aListenerAttachedAcrossARestartIsDeafUntilRestarted =
    begin(true, "")
      .then(uiDoes(Other, asIs))
      .then(starts("listener", "", asIs))
      .then(serverRestarts(asIs))
      .then(uiDoes(ListenOn, asIs))
      .then(uiDoes(Request, asIs))
      .then(listenerPollFails)
      .expect(s.handed == [])
      .then(kills("listener"))
      .then(starts("listener", "", asIs))
      .then(uiDoes(Request, asIs))                  // new log, 3
      .then(listenerPolls)
      .expect(s.handed.length() == 1 and s.handed[0].seq == 3)
      .expect(allInvariants)

  // The watcher carrying its cursor into the new log passes over the
  // messages at or below it.
  run carryingACursorAcrossARestartPassesOverNewMessages =
    begin(true, "keepsItsCursorAcrossAServerRestart")
      .then(uiDoes(Other, asIs))
      .then(uiDoes(Other, asIs))
      .then(starts("watcher", "follow", asIs))      // cursor 2
      .then(serverRestarts(asIs))
      .then(uiDoes(ListenOn, asIs))                 // 1
      .then(uiDoes(Request, asIs))                  // 2: at the old cursor
      .then(uiDoes(Request, asIs))                  // 3
      .then(watcherReconnects(asIs))
      .then(polls("watcher"))
      .expect(s.handed.length() == 1 and s.handed[0].seq == 3)
      .expect(not(nothingActionableIsSkipped))
}
