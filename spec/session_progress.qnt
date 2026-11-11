// A bounded form of `session.qnt`'s two temporal properties.
//
// `catchUp` and `messagesReachTheAssistant` say that, with fair delivery, a
// client that stays connected consumes the whole log and every user message
// reaches a connected assistant. They are liveness properties: random
// simulation cannot check "eventually", and nothing here proves them.
//
// What simulation can check is the same claim with the bound written out.
// This module runs `session.qnt` under a scheduler that is fair within a
// bound (a reader that is behind and has gone `FAIR` states without a
// delivery is delivered to before anything else happens) and counts states:
//
//   * every reader that is behind is served in its turn, however many are
//     behind at once;
//   * a reader gets no more deliveries than the log has events before it has
//     caught up, so the deliveries make progress;
//   * hence a reader that stays connected is caught up, and a message is with
//     a connected assistant, within `BOUND` states.
//
// The counters lag one state behind the session's variables, because a
// definition cannot read the next state; the `...Now` definitions add the
// current state to them.

module session_progress {
  import session as S from "./session"

  // ---- constants

  /// How many states a reader that is behind may go without a delivery
  /// before the scheduler owes it one.
  pure val FAIR: int = 1

  /// With every reader owed a delivery at once, the last is served this many
  /// states after it fell behind.
  pure val TURN: int = FAIR + S::CLIENTS.size()

  /// A reader needs at most one delivery per event in the log.
  pure val BOUND: int = TURN * (S::MAX_SEQ + 1)

  // ---- state

  var lag: str -> int          // consecutive earlier states in which the reader was behind
  var got: str -> int          // deliveries to it in that stretch
  var waited: str -> int       // consecutive earlier states behind without a delivery
  var seenCursor: str -> int   // its cursor one state ago
  var unseenFor: int           // consecutive earlier states with a message the assistant lacks
  var fair: bool               // false in the mutant

  // ---- helpers

  def behind(c: str): bool =
    S::connected.contains(c) and S::cursor.get(c) < S::log.length()

  def moved(c: str): bool = S::cursor.get(c) > seenCursor.get(c)

  def lagNow(c: str): int = if (behind(c)) lag.get(c) + 1 else 0

  def gotNow(c: str): int =
    if (not(behind(c))) 0 else if (moved(c)) got.get(c) + 1 else got.get(c)

  def waitedNow(c: str): int = if (behind(c) and not(moved(c))) waited.get(c) + 1 else 0

  /// A user message is in the log that a connected assistant has not received.
  val aMessageIsUnseen: bool = and {
    S::connected.contains("assistant"),
    S::log.indices().exists(i =>
      S::log[i].kind == S::Message and not(S::delivered.get("assistant").contains(S::log[i].seq))),
  }

  val unseenNow: int = if (aMessageIsUnseen) unseenFor + 1 else 0

  def due(c: str): bool = behind(c) and waitedNow(c) >= FAIR

  // ---- actions

  action count: bool = all {
    lag' = S::CLIENTS.mapBy(c => lagNow(c)),
    got' = S::CLIENTS.mapBy(c => gotNow(c)),
    waited' = S::CLIENTS.mapBy(c => waitedNow(c)),
    seenCursor' = S::cursor,
    unseenFor' = unseenNow,
    fair' = fair,
  }

  action initWith(f: bool): bool = all {
    S::init,
    lag' = S::CLIENTS.mapBy(c => 0),
    got' = S::CLIENTS.mapBy(c => 0),
    waited' = S::CLIENTS.mapBy(c => 0),
    seenCursor' = S::CLIENTS.mapBy(c => 0),
    unseenFor' = 0,
    fair' = f,
  }

  action init = initWith(true)

  /// A delivery to the reader that has been owed one the longest. (Serving
  /// any reader that is owed one is not enough: two could take turns for
  /// ever in front of a third.)
  action serveOneThatIsDue = {
    val longest = S::CLIENTS.fold(0, (m, c) => if (due(c) and waitedNow(c) > m) waitedNow(c) else m)
    nondet c = S::CLIENTS.filter(x => due(x) and waitedNow(x) == longest).oneOf()
    S::sseDeliverTo(c)
  }

  // ---- step

  /// The session's own step, except that a reader owed a delivery gets it
  /// first.
  action step = all {
    if (fair and S::CLIENTS.exists(c => due(c))) serveOneThatIsDue else S::step,
    count,
  }

  // ---- invariants

  /// Under the fair scheduler no reader starves: one that is behind has had
  /// a delivery for every `TURN` states it has been behind.
  val everyReaderIsServedInTurn: bool =
    S::CLIENTS.forall(c => lagNow(c) <= TURN * (gotNow(c) + 1))

  /// Deliveries make progress: before it has caught up, a reader receives at
  /// most one per event in the log.
  val deliveriesMakeProgress: bool =
    S::CLIENTS.forall(c => gotNow(c) <= S::MAX_SEQ)

  /// The bounded form of `catchUp`: a reader that stays connected is at the
  /// end of the log within `BOUND` states.
  val catchUpWithinTheBound: bool =
    S::CLIENTS.forall(c => lagNow(c) <= BOUND)

  /// The bounded form of `messagesReachTheAssistant`: a user message is with
  /// an assistant that stays connected within `BOUND` states.
  val messagesReachTheAssistantWithinTheBound: bool =
    unseenNow <= BOUND

  /// The session's own invariants still hold under this scheduler.
  val theSessionInvariantsHold: bool = S::allInvariants

  val allInvariants: bool = and {
    everyReaderIsServedInTurn,
    deliveriesMakeProgress,
    catchUpWithinTheBound,
    messagesReachTheAssistantWithinTheBound,
    theSessionInvariantsHold,
  }

  // ---- mutants

  /// No fairness: the scheduler may leave a reader behind for as long as it
  /// likes, and `everyReaderIsServedInTurn` fails. This is why the temporal
  /// properties carry `weakFair` as their premise.
  action initWithoutFairness = initWith(false)

  // ---- scenarios

  // Without fairness the assistant can be left behind while other things
  // happen: after `TURN` states with no delivery it has not been served.
  run withoutFairnessAReaderCanBeLeftBehind =
    initWith(false)
      .then(all { S::joinClient("ui"), count })
      .then(all { S::joinClient("assistant"), count })
      .then(all { S::uiMessage, count })
      .then((TURN + 1).reps(_ => all { S::assistantRefresh, count }))
      .expect(aMessageIsUnseen and not(everyReaderIsServedInTurn))

  // Served, the assistant has the message after as many deliveries as there
  // are events up to it.
  run threeDeliveriesBringTheMessage =
    init
      .then(all { S::joinClient("assistant"), count })
      .then(all { S::joinClient("ui"), count })
      .then(all { S::uiMessage, count })
      .expect(aMessageIsUnseen)
      .then(all { S::sseDeliverTo("assistant"), count })
      .then(all { S::sseDeliverTo("assistant"), count })
      .then(all { S::sseDeliverTo("assistant"), count })
      .expect(not(aMessageIsUnseen) and theSessionInvariantsHold)
      .expect(gotNow("assistant") == 0 and lagNow("assistant") == 0)
}
