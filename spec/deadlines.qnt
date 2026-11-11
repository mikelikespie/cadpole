// The two deadlines of one call over the C ABI.
//
// `EvaluateRequest.deadline_ms` arms two independent timers:
//
//   * the FFI waiter (`cadpole_call_cancellable`, cadpole-ffi): on expiry the
//     call returns `CADPOLE_ERR_DEADLINE` and writes nothing through `*out`;
//   * the interpreter's own deadline (`limits_from_request`, cadpole-script):
//     the script stops itself and the call returns `CADPOLE_OK` with a
//     response whose model carries a `deadline` diagnostic.
//
// Which one fires first is a scheduling question, so a timeout can arrive
// either way. Only `Evaluate` arms the waiter, and the interpreter cannot see
// time spent in the geometry kernel. Time is discrete: the model is about the
// order of events, not milliseconds.

module deadlines {
  // ---- constants

  pure val MAX_TIME: int = 12

  // ---- types

  type Kind =
    | Evaluate           // the only request whose `deadline_ms` the FFI reads
    | Render             // stands for every other request

  type Outcome =
    | Pending
    | OkResult           // CADPOLE_OK, the script finished
    | OkDeadlineDiag     // CADPOLE_OK with a `deadline` diagnostic
    | ErrDeadline        // CADPOLE_ERR_DEADLINE
    | ErrCancelled       // CADPOLE_ERR_CANCELLED

  // ---- state

  var now: int
  var budget: int          // `deadline_ms`; 0 means none
  var kind: Kind
  var waiterAt: int        // when the FFI waiter fires; -1 = not armed
  var starlarkAt: int      // when the interpreter's deadline is due; -1 = not armed
  var spawned: bool        // the worker thread has started
  var workLeft: int        // work the script has left
  var inKernel: bool       // that work is in the kernel, where no deadline is checked
  var outcome: Outcome
  var timedOut: bool       // the call ended because of time

  // ---- helpers

  /// The interpreter checks its deadline only while running script code.
  val starlarkDue: bool =
    starlarkAt >= 0 and now >= starlarkAt and not(inKernel) and workLeft > 0

  val waiterDue: bool = waiterAt >= 0 and now >= waiterAt

  // ---- actions

  action init = all {
    now' = 0,
    budget' = 0,
    kind' = Evaluate,
    waiterAt' = -1,
    starlarkAt' = -1,
    spawned' = false,
    workLeft' = 0,
    inKernel' = false,
    outcome' = Pending,
    timedOut' = false,
  }

  /// One call: `d` is `deadline_ms`, `w` the work the script needs, `kernel`
  /// whether that work is in the kernel.
  action call(k: Kind, d: int, w: int, kernel: bool): bool = all {
    outcome == Pending,
    not(spawned),
    kind' = k,
    budget' = d,
    waiterAt' = if (k == Evaluate and d > 0) now + d else -1,
    workLeft' = w,
    inKernel' = kernel,
    now' = now, starlarkAt' = starlarkAt, spawned' = spawned,
    outcome' = outcome, timedOut' = timedOut,
  }

  action anyCall = {
    nondet k = Set(Evaluate, Render).oneOf()
    nondet d = Set(0, 2, 4).oneOf()
    nondet w = Set(1, 3, 9).oneOf()
    nondet kernel = Set(false, true).oneOf()
    call(k, d, w, kernel)
  }

  /// The worker thread starts, at or after the instant the waiter was armed,
  /// and arms the interpreter's deadline from the same budget.
  action startWorker = all {
    not(spawned),
    workLeft > 0,
    spawned' = true,
    starlarkAt' = if (budget > 0) now + budget else -1,
    now' = now, budget' = budget, kind' = kind, waiterAt' = waiterAt,
    workLeft' = workLeft, inKernel' = inKernel, outcome' = outcome, timedOut' = timedOut,
  }

  /// Time passes, but not while a deadline is due and the call is pending.
  /// This turns "the deadline releases the caller" into a safety property.
  action tick = all {
    now < MAX_TIME,
    outcome == Pending,
    not(waiterDue),
    not(starlarkDue),
    now' = now + 1,
    budget' = budget, kind' = kind, waiterAt' = waiterAt, starlarkAt' = starlarkAt,
    spawned' = spawned, workLeft' = workLeft, inKernel' = inKernel,
    outcome' = outcome, timedOut' = timedOut,
  }

  /// The script makes progress and returns when the work is done.
  action work = all {
    spawned,
    workLeft > 0,
    outcome == Pending,
    workLeft' = workLeft - 1,
    outcome' = if (workLeft == 1) OkResult else Pending,
    now' = now, budget' = budget, kind' = kind, waiterAt' = waiterAt,
    starlarkAt' = starlarkAt, spawned' = spawned, inKernel' = inKernel, timedOut' = timedOut,
  }

  /// The interpreter stops at its own deadline: a successful call that timed out.
  action starlarkFires = all {
    starlarkDue,
    outcome == Pending,
    outcome' = OkDeadlineDiag,
    timedOut' = true,
    workLeft' = 0,
    now' = now, budget' = budget, kind' = kind, waiterAt' = waiterAt,
    starlarkAt' = starlarkAt, spawned' = spawned, inKernel' = inKernel,
  }

  /// The FFI waiter expires with no result yet. The worker's result is dropped.
  action waiterFires = all {
    waiterDue,
    outcome == Pending,
    outcome' = ErrDeadline,
    timedOut' = true,
    now' = now, budget' = budget, kind' = kind, waiterAt' = waiterAt,
    starlarkAt' = starlarkAt, spawned' = spawned, workLeft' = workLeft, inKernel' = inKernel,
  }

  /// `cadpole_cancel` for this call's token.
  action cancelCall = all {
    outcome == Pending,
    outcome' = ErrCancelled,
    now' = now, budget' = budget, kind' = kind, waiterAt' = waiterAt,
    starlarkAt' = starlarkAt, spawned' = spawned, workLeft' = workLeft,
    inKernel' = inKernel, timedOut' = timedOut,
  }

  /// The caller makes another call once the last one has returned, so one
  /// simulated trace covers many calls.
  action nextCall = all {
    outcome != Pending,
    now < MAX_TIME,
    outcome' = Pending,
    timedOut' = false,
    spawned' = false,
    workLeft' = 0,
    waiterAt' = -1,
    starlarkAt' = -1,
    inKernel' = false,
    budget' = 0,
    kind' = kind, now' = now,
  }

  // ---- step

  action step = any {
    anyCall,
    nextCall,
    startWorker,
    tick,
    work,
    starlarkFires,
    waiterFires,
    cancelCall,
  }

  // ---- invariants

  /// With a waiter armed, a call is never still pending after its deadline.
  val callerIsNeverHeldPastTheDeadline: bool =
    outcome == Pending implies (waiterAt < 0 or now <= waiterAt)

  /// Both ways a call can time out are marked as a timeout.
  val everyTimeoutIsRecognisable: bool =
    (outcome == ErrDeadline or outcome == OkDeadlineDiag) implies timedOut

  /// A call that finished its work is never marked as a timeout.
  val aFinishedCallIsNeverATimeout: bool =
    outcome == OkResult implies not(timedOut)

  /// `CADPOLE_ERR_DEADLINE` is never returned before the deadline.
  val noDeadlineFiresEarly: bool =
    outcome == ErrDeadline implies (waiterAt >= 0 and now >= waiterAt)

  val allInvariants: bool = and {
    callerIsNeverHeldPastTheDeadline,
    everyTimeoutIsRecognisable,
    aFinishedCallIsNeverATimeout,
    noDeadlineFiresEarly,
  }

  // ---- not invariants
  //
  // Guarantees the interface does not give. Each is false in some reachable
  // state, and a scenario below reaches it.

  /// A timeout always shows in the return code. False: it can be `CADPOLE_OK`
  /// with a diagnostic.
  val timeoutMeansErrDeadline: bool = timedOut implies outcome != OkDeadlineDiag

  /// Every running call has a waiter. False: only `Evaluate` with a non-zero
  /// `deadline_ms` arms one.
  val everyHeavyCallIsBounded: bool =
    (workLeft > 0 and outcome == Pending) implies waiterAt >= 0

  // ---- scenarios

  // Work in the kernel is invisible to the interpreter's deadline, so the
  // waiter is what releases the caller.
  run workInTheKernelCanOnlyBeEndedByTheWaiter =
    init
      .then(call(Evaluate, 2, 9, true))
      .then(startWorker)
      .then(tick)
      .then(work)
      .then(tick)
      .expect(now == 2 and waiterDue and not(starlarkDue))
      .then(waiterFires)
      .expect(outcome == ErrDeadline and timedOut)
      .expect(allInvariants)

  // With both deadlines due, the waiter may fire first...
  run theSameRequestCanTimeOutAsAnError =
    init
      .then(call(Evaluate, 2, 9, false))
      .then(startWorker)
      .then(tick)
      .then(tick)
      .expect(waiterDue and starlarkDue)
      .then(waiterFires)
      .expect(outcome == ErrDeadline and timedOut)
      .expect(allInvariants)

  // ...or the interpreter may, and the same request returns `CADPOLE_OK`.
  // A test of the timeout must accept both.
  run theSameRequestCanTimeOutAsASuccess =
    init
      .then(call(Evaluate, 2, 9, false))
      .then(startWorker)
      .then(tick)
      .then(tick)
      .expect(waiterDue and starlarkDue)
      .then(starlarkFires)
      .expect(outcome == OkDeadlineDiag and timedOut)
      .expect(allInvariants)
      .expect(not(timeoutMeansErrDeadline))

  // A deadline longer than the work never fires.
  run aGenerousDeadlineDoesNotFire =
    init
      .then(call(Evaluate, 4, 1, false))
      .then(startWorker)
      .then(work)
      .expect(outcome == OkResult and not(timedOut))
      .expect(allInvariants)

  // A request other than `Evaluate` arms no waiter: only a cancel ends it.
  run aRenderOverTheFfiIsBoundedByNothing =
    init
      .then(call(Render, 4, 9, true))
      .then(startWorker)
      .then(tick)
      .then(tick)
      .then(tick)
      .then(tick)
      .then(tick)
      .expect(now == 5 and waiterAt == -1 and outcome == Pending)
      .expect(callerIsNeverHeldPastTheDeadline)
      .expect(not(everyHeavyCallIsBounded))
      .then(cancelCall)
      .expect(outcome == ErrCancelled)

  // An `Evaluate` with `deadline_ms` of zero arms neither deadline.
  run aZeroDeadlineOnKernelWorkBoundsNothing =
    init
      .then(call(Evaluate, 0, 9, true))
      .then(startWorker)
      .then(tick)
      .then(tick)
      .then(tick)
      .then(tick)
      .expect(waiterAt == -1 and starlarkAt == -1 and outcome == Pending)
      .expect(not(everyHeavyCallIsBounded))
}
