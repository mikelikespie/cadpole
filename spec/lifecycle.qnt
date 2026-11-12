// The life of one request, over both front doors.
//
// A heavy request arrives over HTTP or over the C ABI, is admitted or refused,
// runs, may be cancelled or time out, and is answered exactly once. The model
// asks what, if anything, ends each request:
//
//   * over HTTP, `post_op` (cadpole-cli, server.rs) sends the operations in
//     its `heavy` match through `render_job`: a wall-clock budget, a permit
//     limit and a queue bound. Other operations run with none of the three;
//   * over the C ABI, `deadline_of` (cadpole-ffi) arms a waiter for `Evaluate`
//     and for nothing else.
//
// `step` is the intended machine, in which both doors bound every heavy
// operation. `HTTP_BOUNDS_TODAY` and `FFI_BOUNDS_TODAY` are what the code
// bounds, `//spec:names_test` keeps them equal to the code, and the scenarios
// show what those sets leave unbounded. Time is discrete, as in
// `deadlines.qnt`: the model is about the order of events, not milliseconds.

module lifecycle {
  // ---- constants

  pure val MAX_TIME: int = 8
  pure val IDS: Set[int] = Set(1, 2, 3)

  pure val BUDGET: int = 3     // the render budget, or `deadline_ms`
  pure val SLOTS: int = 1      // render permits
  pure val QUEUE: int = 2      // gated callers `render_job` accepts before refusing

  pure val DOORS: Set[str] = Set("http", "ffi")

  // Operations that cost real time in the interpreter or the kernel.
  pure val HEAVY: Set[str] = Set("Evaluate", "Render", "Sweep", "Animate", "Export", "PrintCheck", "Drawing")

  // What each door bounds in the code: `post_op`'s `heavy` match and the
  // request `deadline_of` reads. Diffed against the Rust by `//spec:names_test`.
  pure val HTTP_BOUNDS_TODAY: Set[str] = Set("Render", "Sweep", "Animate")
  pure val FFI_BOUNDS_TODAY: Set[str] = Set("Evaluate")

  // ---- state

  var now: int
  var door: int -> str      // "" until the request arrives
  var op: int -> str
  var kernel: int -> bool   // the work is in the kernel, where no interpreter deadline is checked
  var work: int -> int      // work left
  var armed: int -> int     // when this caller's clock fires; -1 = not armed
  var result: int -> str    // "" while pending, then "replied", "refused", "timeout" or "cancelled"
  var replies: int -> int   // answers the caller has received
  var burning: Set[int]     // requests whose worker thread is still running
  var poisoned: Set[int]    // FFI tokens holding a cancel that no call has claimed
  var budgeted: str -> Set[str]   // per door, the operations given a wall-clock bound
  var gated: str -> Set[str]      // per door, the operations that go through the render gate

  // ---- helpers

  val arrived: Set[int] = IDS.filter(i => door.get(i) != "")
  val pending: Set[int] = arrived.filter(i => result.get(i) == "")

  /// Callers inside the render gate that have not been answered yet.
  val permitsHeld: Set[int] =
    pending.filter(i => gated.get(door.get(i)).contains(op.get(i)))

  /// Pending requests whose clock has come due.
  val due: Set[int] =
    pending.filter(i => armed.get(i) >= 0 and now >= armed.get(i))

  /// The interpreter's own deadline can end script work but not kernel work.
  def interpreterCanEnd(i: int): bool = not(kernel.get(i))

  // ---- actions

  /// Every heavy operation is bounded on both doors; HTTP also gates them.
  action init = all {
    now' = 0,
    door' = IDS.mapBy(i => ""),
    op' = IDS.mapBy(i => ""),
    kernel' = IDS.mapBy(i => false),
    work' = IDS.mapBy(i => 0),
    armed' = IDS.mapBy(i => -1),
    result' = IDS.mapBy(i => ""),
    replies' = IDS.mapBy(i => 0),
    burning' = Set(),
    poisoned' = Set(),
    budgeted' = Map("http" -> HEAVY, "ffi" -> HEAVY),
    gated' = Map("http" -> HEAVY, "ffi" -> Set()),
  }

  /// A request arrives. A gated operation is refused when the queue is full;
  /// an FFI call on a poisoned token returns cancelled without running.
  action arrive(i: int, d: str, o: str, k: bool, w: int): bool = {
    val refuse = gated.get(d).contains(o) and permitsHeld.size() >= QUEUE
    val poison = d == "ffi" and poisoned.contains(i)
    val runs = not(refuse) and not(poison)
    all {
      IDS.contains(i),
      DOORS.contains(d),
      HEAVY.contains(o),
      w > 0,
      door.get(i) == "",
      door' = door.set(i, d),
      op' = op.set(i, o),
      kernel' = kernel.set(i, k),
      work' = work.set(i, if (runs) w else 0),
      armed' = armed.set(i, if (runs and budgeted.get(d).contains(o)) now + BUDGET else -1),
      result' = result.set(i, if (refuse) "refused" else if (poison) "cancelled" else ""),
      replies' = replies.set(i, if (runs) 0 else 1),
      burning' = if (runs) burning.union(Set(i)) else burning,
      poisoned' = poisoned.exclude(Set(i)),
      now' = now, budgeted' = budgeted, gated' = gated,
    }
  }

  action anyArrival = {
    nondet i = IDS.filter(j => door.get(j) == "").oneOf()
    nondet d = DOORS.oneOf()
    nondet o = HEAVY.oneOf()
    nondet k = Set(false, true).oneOf()
    nondet w = Set(1, 2, 5).oneOf()
    arrive(i, d, o, k, w)
  }

  /// The worker makes progress. When it finishes, the caller is answered if
  /// it is still waiting.
  action progress(i: int): bool = {
    val done = work.get(i) == 1
    val answers = done and result.get(i) == ""
    all {
      burning.contains(i),
      work.get(i) > 0,
      work' = work.set(i, work.get(i) - 1),
      burning' = if (done) burning.exclude(Set(i)) else burning,
      result' = if (answers) result.set(i, "replied") else result,
      replies' = if (answers) replies.set(i, replies.get(i) + 1) else replies,
      now' = now, door' = door, op' = op, kernel' = kernel, armed' = armed,
      poisoned' = poisoned, budgeted' = budgeted, gated' = gated,
    }
  }

  action anyProgress = {
    nondet i = burning.oneOf()
    progress(i)
  }

  /// The caller's clock fires: a 504 over HTTP, `CADPOLE_ERR_DEADLINE` over
  /// the C ABI. The worker is abandoned, not stopped, so it stays in `burning`.
  action budgetFires(i: int): bool = all {
    due.contains(i),
    result' = result.set(i, "timeout"),
    replies' = replies.set(i, replies.get(i) + 1),
    now' = now, door' = door, op' = op, kernel' = kernel, work' = work, armed' = armed,
    burning' = burning, poisoned' = poisoned, budgeted' = budgeted, gated' = gated,
  }

  action anyBudgetFires = {
    nondet i = due.oneOf()
    budgetFires(i)
  }

  /// The interpreter stops at its own deadline: a successful reply carrying a
  /// `deadline` diagnostic. Only possible for script work.
  action interpreterStops(i: int): bool = all {
    burning.contains(i),
    result.get(i) == "",
    work.get(i) > 0,
    interpreterCanEnd(i),
    result' = result.set(i, "replied"),
    replies' = replies.set(i, replies.get(i) + 1),
    work' = work.set(i, 0),
    burning' = burning.exclude(Set(i)),
    now' = now, door' = door, op' = op, kernel' = kernel, armed' = armed,
    poisoned' = poisoned, budgeted' = budgeted, gated' = gated,
  }

  action anyInterpreterStop = {
    nondet i = burning.oneOf()
    interpreterStops(i)
  }

  /// `cadpole_cancel(token)` for a running call. The worker is abandoned.
  action cancel(i: int): bool = all {
    pending.contains(i),
    door.get(i) == "ffi",
    result' = result.set(i, "cancelled"),
    replies' = replies.set(i, replies.get(i) + 1),
    now' = now, door' = door, op' = op, kernel' = kernel, work' = work, armed' = armed,
    burning' = burning, poisoned' = poisoned, budgeted' = budgeted, gated' = gated,
  }

  action anyCancel = {
    nondet i = pending.filter(j => door.get(j) == "ffi").oneOf()
    cancel(i)
  }

  /// `cadpole_cancel(token)` with no call in flight: the cancel stays in the
  /// registry until a call on that token claims it.
  action cancelAnIdleToken(i: int): bool = all {
    IDS.contains(i),
    door.get(i) == "",
    poisoned' = poisoned.union(Set(i)),
    now' = now, door' = door, op' = op, kernel' = kernel, work' = work, armed' = armed,
    result' = result, replies' = replies, burning' = burning,
    budgeted' = budgeted, gated' = gated,
  }

  action anyIdleCancel = {
    nondet i = IDS.filter(j => door.get(j) == "").oneOf()
    cancelAnIdleToken(i)
  }

  /// Time passes, but not while a due clock has a caller still waiting. This
  /// turns "the bound releases the caller" into a safety property.
  action tick = all {
    now < MAX_TIME,
    due == Set(),
    now' = now + 1,
    door' = door, op' = op, kernel' = kernel, work' = work, armed' = armed,
    result' = result, replies' = replies, burning' = burning, poisoned' = poisoned,
    budgeted' = budgeted, gated' = gated,
  }

  // ---- step

  action step = any {
    anyArrival,
    anyProgress,
    anyBudgetFires,
    anyInterpreterStop,
    anyCancel,
    anyIdleCancel,
    tick,
  }

  // ---- invariants

  /// A request that has a result was answered once; one that has none was not
  /// answered at all.
  val exactlyOneReplyPerRequest: bool =
    IDS.forall(i => replies.get(i) == (if (arrived.contains(i) and result.get(i) != "") 1 else 0))

  /// A refused request has no worker.
  val aRefusedRequestNeverRuns: bool =
    IDS.forall(i => result.get(i) == "refused" implies not(burning.contains(i)))

  /// With a clock armed, a request is never still pending after it.
  val callerIsReleasedByItsBound: bool =
    IDS.forall(i =>
      (arrived.contains(i) and result.get(i) == "") implies (armed.get(i) < 0 or now <= armed.get(i)))

  /// Every pending request with work left has something that will end it: an
  /// armed clock, or script work the interpreter's deadline can stop.
  val everyAdmittedRequestTerminates: bool =
    IDS.forall(i =>
      (arrived.contains(i) and result.get(i) == "" and work.get(i) > 0) implies
        (armed.get(i) >= 0 or interpreterCanEnd(i)))

  /// Both doors bound every heavy operation. A fact about `budgeted`, which
  /// no action in `step` changes; the scenario-only actions below break it.
  val everyDoorBoundsEveryHeavyOperation: bool =
    DOORS.forall(d => HEAVY.subseteq(budgeted.get(d)))

  val allInvariants: bool = and {
    exactlyOneReplyPerRequest,
    aRefusedRequestNeverRuns,
    callerIsReleasedByItsBound,
    everyAdmittedRequestTerminates,
    everyDoorBoundsEveryHeavyOperation,
  }

  // ---- not invariants

  /// The permit limit bounds running workers. False: a timeout releases the
  /// caller's permit while its abandoned worker keeps running.
  val theGateBoundsRunningWork: bool =
    burning.filter(i => gated.get(door.get(i)).contains(op.get(i))).size() <= SLOTS

  // Two facts about the sets the code bounds. Both are true and a scenario
  // asserts them; they describe the gap between the doors, not a guarantee.

  /// No operation is bounded on both doors.
  pure val theTwoDoorsBoundNothingInCommon: bool =
    HTTP_BOUNDS_TODAY.intersect(FFI_BOUNDS_TODAY) == Set()

  /// Every heavy operation is unbounded on at least one door.
  pure val everyHeavyOperationIsUnboundedSomewhere: bool =
    HEAVY.forall(o => not(HTTP_BOUNDS_TODAY.contains(o)) or not(FFI_BOUNDS_TODAY.contains(o)))

  // ---- scenario-only actions
  //
  // Not in `step`: they switch a door between the intended bounds and the
  // bounds the code has.

  /// The HTTP door bounds and gates only `HTTP_BOUNDS_TODAY`.
  action httpBoundsOnlyWhatTheCodeBounds = all {
    budgeted' = budgeted.set("http", HTTP_BOUNDS_TODAY),
    gated' = gated.set("http", HTTP_BOUNDS_TODAY),
    now' = now, door' = door, op' = op, kernel' = kernel, work' = work, armed' = armed,
    result' = result, replies' = replies, burning' = burning, poisoned' = poisoned,
  }

  /// The C ABI bounds only `FFI_BOUNDS_TODAY`.
  action ffiBoundsOnlyWhatTheCodeBounds = all {
    budgeted' = budgeted.set("ffi", FFI_BOUNDS_TODAY),
    gated' = gated,
    now' = now, door' = door, op' = op, kernel' = kernel, work' = work, armed' = armed,
    result' = result, replies' = replies, burning' = burning, poisoned' = poisoned,
  }

  /// Both doors bound every heavy operation again, as in `init`.
  action bindEveryHeavyOperationOnEveryDoor = all {
    budgeted' = DOORS.mapBy(d => HEAVY),
    gated' = Map("http" -> HEAVY, "ffi" -> Set()),
    now' = now, door' = door, op' = op, kernel' = kernel, work' = work, armed' = armed,
    result' = result, replies' = replies, burning' = burning, poisoned' = poisoned,
  }

  // ---- scenarios

  // An export over HTTP is outside the render gate and its work is in the
  // kernel: no clock is armed and nothing ends the request.
  run anExportOverHttpIsBoundedByNothing =
    init
      .then(httpBoundsOnlyWhatTheCodeBounds)
      .then(arrive(1, "http", "Export", true, 5))
      .expect(armed.get(1) == -1)
      .then(tick)
      .then(tick)
      .then(tick)
      .then(tick)
      .then(tick)
      .expect(now == 5 and result.get(1) == "" and burning.contains(1))
      .expect(callerIsReleasedByItsBound)      // holds trivially: no bound is armed
      .expect(not(everyAdmittedRequestTerminates))

  // Over the C ABI only `Evaluate` is bounded, so a render is not; and the
  // two doors' bounded sets do not overlap.
  run theCAbiBoundsTheOtherHalfAndTheirIntersectionIsEmpty =
    init
      .then(ffiBoundsOnlyWhatTheCodeBounds)
      .then(arrive(1, "ffi", "Render", true, 5))
      .expect(armed.get(1) == -1)
      .then(arrive(2, "ffi", "Evaluate", true, 5))
      .expect(armed.get(2) == 3)
      .expect(not(everyAdmittedRequestTerminates))
      .expect(theTwoDoorsBoundNothingInCommon)
      .expect(everyHeavyOperationIsUnboundedSomewhere)

  // With every heavy operation bounded on every door, the same export is
  // ended by its budget.
  run bindingEveryHeavyOperationRestoresTermination =
    init
      .then(bindEveryHeavyOperationOnEveryDoor)
      .then(arrive(1, "http", "Export", true, 5))
      .expect(armed.get(1) == 3)
      .then(tick)
      .then(tick)
      .then(tick)
      .expect(due.contains(1))
      .then(budgetFires(1))
      .expect(result.get(1) == "timeout" and replies.get(1) == 1)
      .expect(everyAdmittedRequestTerminates)
      .expect(allInvariants)

  // The permit limit bounds callers, not workers: two renders time out and
  // leave their workers running, and a third is admitted. Each caller is
  // still answered exactly once.
  run anAbandonedWorkerKeepsItsCpuAndItsPermitIsGivenAway =
    init
      .then(arrive(1, "http", "Render", true, 9))
      .then(tick)
      .then(tick)
      .then(tick)
      .then(budgetFires(1))
      .expect(result.get(1) == "timeout" and burning.contains(1))
      .then(arrive(2, "http", "Render", true, 9))
      .expect(permitsHeld == Set(2))          // one caller in the gate, two workers
      .then(tick)
      .then(tick)
      .then(tick)
      .then(budgetFires(2))
      .then(arrive(3, "http", "Render", true, 9))
      .expect(burning == Set(1, 2, 3))
      .expect(not(theGateBoundsRunningWork))
      .expect(exactlyOneReplyPerRequest)
      .expect(everyAdmittedRequestTerminates)

  // A gated request that finds the queue full is refused and never runs.
  run theQueueBoundRefusesRatherThanGrowing =
    init
      .then(arrive(1, "http", "Render", true, 9))
      .then(arrive(2, "http", "Sweep", true, 9))
      .expect(permitsHeld.size() == 2)
      .then(arrive(3, "http", "Render", true, 9))
      .expect(result.get(3) == "refused" and replies.get(3) == 1)
      .expect(aRefusedRequestNeverRuns)
      .expect(exactlyOneReplyPerRequest)

  // A cancel sent while no call is in flight cancels the next call on that
  // token before it does any work. A fresh token is unaffected.
  run aReusedTokenInheritsAnOldCancel =
    init
      .then(cancelAnIdleToken(1))
      .expect(poisoned == Set(1))
      .then(arrive(1, "ffi", "Evaluate", false, 3))
      .expect(result.get(1) == "cancelled" and work.get(1) == 0 and not(burning.contains(1)))
      .expect(replies.get(1) == 1)
      .then(arrive(2, "ffi", "Evaluate", false, 3))
      .expect(result.get(2) == "" and burning.contains(2))
      .expect(allInvariants)

  // Script work with no clock armed is still ended by the interpreter's own
  // deadline...
  run scriptBoundWorkIsEndedByTheInterpreter =
    init
      .then(httpBoundsOnlyWhatTheCodeBounds)
      .then(arrive(1, "http", "Evaluate", false, 5))
      .expect(armed.get(1) == -1)
      .expect(everyAdmittedRequestTerminates)
      .then(interpreterStops(1))
      .expect(result.get(1) == "replied" and replies.get(1) == 1)
      .expect(exactlyOneReplyPerRequest)

  // ...and the same operation with its work in the kernel is not.
  run theSameOperationHangsWhenItsTimeIsInTheKernel =
    init
      .then(httpBoundsOnlyWhatTheCodeBounds)
      .then(arrive(1, "http", "Evaluate", true, 5))
      .expect(armed.get(1) == -1)
      .expect(not(everyAdmittedRequestTerminates))
}
