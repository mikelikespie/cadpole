// The asynchronous job seam.
//
// Every heavy operation goes through one seam: a request becomes a job, the
// caller is not blocked, a newer request supersedes an older one, and a result
// computed from an input that has since changed is not displayed as current.
//
// Per kind there is at most one job in flight and at most one pending request.
// A new request replaces the pending one (`ParamCoalescer`,
// src/apple/CadpoleKit/ParamCoalescer.swift, is this rule on the client) and
// supersedes the job in flight. A finished job is accepted only if its
// revision is still current; otherwise it is dropped and what stays on screen
// is marked stale. When the server is unreachable the request runs on the
// local engine, which refuses the kinds it cannot answer.
//
// Cancelling releases the caller, not the CPU, and `cadpole_cancel`
// (cadpole-ffi/src/lib.rs) keeps an entry for a token that never ran. Both
// are stated under "not invariants".

module jobs {
  // ---- constants

  pure val KINDS: Set[str] = Set("Evaluate", "Render", "Sweep", "Export", "Drawing", "PrintCheck")

  // What the browser's engine answers without a server (`EngineLike`,
  // src/web/src/engine.ts). An engine that answers more is a larger set here.
  pure val LOCAL_KINDS: Set[str] = Set("Evaluate")

  pure val MAX_REV: int = 3
  pure val MAX_JOBS: int = 6

  pure val IDLE: int = 0        // no job in flight
  pure val NOTHING: int = -1    // no revision

  // ---- types

  type JobState =
    | Running       // the seam is waiting for it
    | Accepted      // finished, and its result is what is displayed
    | Dropped       // finished or cancelled, and its result was thrown away
    | Refused       // the engine it landed on cannot answer this kind

  type Job = { kind: str, rev: int, at: str, state: JobState }   // `at`: "remote" or "local"

  // ---- state

  var revision: int              // content revision of the document
  var jobs: int -> Job           // every job ever created
  var nextId: int                // the next job id, which is its cancellation token
  var inflight: str -> int       // per kind: the job the seam is waiting for, or IDLE
  var pending: str -> int        // per kind: the revision asked for but not started, or NOTHING
  var shown: str -> int          // per kind: the revision whose result is displayed, or NOTHING
  var shownFrom: str -> str      // per kind: the engine that produced what is displayed
  var staleShown: Set[str]       // kinds whose displayed result is marked as behind
  var completed: str -> Set[int] // per kind: the revisions whose results were accepted
  var remoteUp: bool             // `cadpole serve` is reachable
  var abandoned: Set[int]        // superseded or cancelled jobs whose work was not stopped
  var cancelRegistry: Set[int]   // tokens `cadpole_cancel` holds an entry for

  // ---- helpers

  pure def canAnswer(at: str, k: str): bool =
    at == "remote" or LOCAL_KINDS.contains(k)

  val running: Set[int] = jobs.keys().filter(j => jobs.get(j).state == Running)

  def isCurrent(j: int): bool = jobs.get(j).rev == revision

  /// The engine a request issued now would land on.
  val engineNow: str = if (remoteUp) "remote" else "local"

  // ---- actions

  action init = all {
    revision' = 0,
    jobs' = Map(),
    nextId' = 1,
    inflight' = KINDS.mapBy(k => IDLE),
    pending' = KINDS.mapBy(k => NOTHING),
    shown' = KINDS.mapBy(k => NOTHING),
    shownFrom' = KINDS.mapBy(k => ""),
    staleShown' = Set(),
    completed' = KINDS.mapBy(k => Set()),
    remoteUp' = true,
    abandoned' = Set(),
    cancelRegistry' = Set(),
  }

  /// The document changes. Everything displayed is now behind and is marked so.
  action edit = all {
    revision < MAX_REV,
    revision' = revision + 1,
    staleShown' = staleShown.union(KINDS.filter(k => shown.get(k) >= 0)),
    jobs' = jobs, nextId' = nextId, inflight' = inflight, pending' = pending,
    shown' = shown, shownFrom' = shownFrom, completed' = completed, remoteUp' = remoteUp,
    abandoned' = abandoned, cancelRegistry' = cancelRegistry,
  }

  /// Ask for `k` at the current revision. The request replaces any pending one
  /// and supersedes the job in flight: the caller stops waiting, the work goes on.
  action submit(k: str): bool = {
    val old = inflight.get(k)
    all {
      KINDS.contains(k),
      pending' = pending.set(k, revision),
      inflight' = inflight.set(k, IDLE),
      jobs' = if (old == IDLE) jobs else jobs.setBy(old, j => { ...j, state: Dropped }),
      abandoned' = if (old == IDLE) abandoned else abandoned.union(Set(old)),
      cancelRegistry' = if (old == IDLE) cancelRegistry else cancelRegistry.union(Set(old)),
      revision' = revision, nextId' = nextId, shown' = shown, shownFrom' = shownFrom,
      staleShown' = staleShown, completed' = completed, remoteUp' = remoteUp,
    }
  }

  action anySubmit = {
    nondet k = KINDS.oneOf()
    submit(k)
  }

  /// The seam starts one job for the pending request. A separate step, so
  /// several submits can land before a job starts and only the newest survives.
  action start(k: str): bool = all {
    KINDS.contains(k),
    inflight.get(k) == IDLE,
    pending.get(k) != NOTHING,
    nextId <= MAX_JOBS,
    jobs' = jobs.put(nextId, { kind: k, rev: pending.get(k), at: engineNow, state: Running }),
    inflight' = inflight.set(k, nextId),
    pending' = pending.set(k, NOTHING),
    nextId' = nextId + 1,
    revision' = revision, shown' = shown, shownFrom' = shownFrom, staleShown' = staleShown,
    completed' = completed, remoteUp' = remoteUp, abandoned' = abandoned,
    cancelRegistry' = cancelRegistry,
  }

  action anyStart = {
    nondet k = KINDS.oneOf()
    start(k)
  }

  /// A job finishes. Its result is accepted only if the seam is waiting for it
  /// and its revision is still current; otherwise it is dropped.
  action finish(j: int): bool = {
    val job = jobs.get(j)
    val k = job.kind
    val keep = inflight.get(k) == j and isCurrent(j)
    all {
      jobs.keys().contains(j),
      job.state == Running,
      canAnswer(job.at, k),
      jobs' = jobs.setBy(j, x => { ...x, state: if (keep) Accepted else Dropped }),
      inflight' = if (inflight.get(k) == j) inflight.set(k, IDLE) else inflight,
      shown' = if (keep) shown.set(k, job.rev) else shown,
      shownFrom' = if (keep) shownFrom.set(k, job.at) else shownFrom,
      completed' = if (keep) completed.setBy(k, s => s.union(Set(job.rev))) else completed,
      staleShown' = if (keep) staleShown.exclude(Set(k)) else staleShown,
      revision' = revision, nextId' = nextId, pending' = pending, remoteUp' = remoteUp,
      abandoned' = abandoned, cancelRegistry' = cancelRegistry,
    }
  }

  action anyFinish = {
    nondet j = running.oneOf()
    finish(j)
  }

  /// The engine the job landed on cannot answer its kind. The job is refused
  /// and the display does not change.
  action refuse(j: int): bool = {
    val job = jobs.get(j)
    val k = job.kind
    all {
      jobs.keys().contains(j),
      job.state == Running,
      not(canAnswer(job.at, k)),
      jobs' = jobs.setBy(j, x => { ...x, state: Refused }),
      inflight' = if (inflight.get(k) == j) inflight.set(k, IDLE) else inflight,
      revision' = revision, nextId' = nextId, pending' = pending, shown' = shown,
      shownFrom' = shownFrom, staleShown' = staleShown, completed' = completed,
      remoteUp' = remoteUp, abandoned' = abandoned, cancelRegistry' = cancelRegistry,
    }
  }

  action anyRefuse = {
    nondet j = running.oneOf()
    refuse(j)
  }

  /// A job is cancelled, or its deadline releases the caller. A second cancel
  /// of the same token changes nothing.
  action cancel(j: int): bool = {
    val job = jobs.get(j)
    all {
      jobs.keys().contains(j),
      cancelRegistry' = cancelRegistry.union(Set(j)),
      jobs' = if (job.state == Running) jobs.setBy(j, x => { ...x, state: Dropped }) else jobs,
      inflight' = if (inflight.get(job.kind) == j) inflight.set(job.kind, IDLE) else inflight,
      abandoned' = if (job.state == Running) abandoned.union(Set(j)) else abandoned,
      revision' = revision, nextId' = nextId, pending' = pending, shown' = shown,
      shownFrom' = shownFrom, staleShown' = staleShown, completed' = completed,
      remoteUp' = remoteUp,
    }
  }

  action anyCancel = {
    nondet j = jobs.keys().oneOf()
    cancel(j)
  }

  /// `cadpole serve` becomes unreachable. Every remote job in flight is lost
  /// and its request becomes pending again, to start on the local engine.
  action serverDown = all {
    remoteUp,
    remoteUp' = false,
    jobs' = jobs.keys().fold(jobs, (acc, j) =>
      if (jobs.get(j).state == Running and jobs.get(j).at == "remote")
        acc.setBy(j, x => { ...x, state: Dropped })
      else acc),
    pending' = KINDS.mapBy(k =>
      val j = inflight.get(k)
      if (j != IDLE and jobs.get(j).at == "remote") jobs.get(j).rev else pending.get(k)),
    inflight' = KINDS.mapBy(k =>
      val j = inflight.get(k)
      if (j != IDLE and jobs.get(j).at == "remote") IDLE else j),
    revision' = revision, nextId' = nextId, shown' = shown, shownFrom' = shownFrom,
    staleShown' = staleShown, completed' = completed, abandoned' = abandoned,
    cancelRegistry' = cancelRegistry,
  }

  action serverUp = all {
    not(remoteUp),
    remoteUp' = true,
    revision' = revision, jobs' = jobs, nextId' = nextId, inflight' = inflight,
    pending' = pending, shown' = shown, shownFrom' = shownFrom, staleShown' = staleShown,
    completed' = completed, abandoned' = abandoned, cancelRegistry' = cancelRegistry,
  }

  // ---- step

  action step = any {
    edit,
    anySubmit,
    anyStart,
    anyFinish,
    anyRefuse,
    anyCancel,
    serverDown,
    serverUp,
  }

  // ---- invariants

  /// What is displayed is the newest result ever accepted for that kind, so
  /// an older job that finishes later cannot replace a newer one.
  val displayIsNewestCompleted: bool =
    KINDS.forall(k =>
      val done = completed.get(k)
      if (done == Set()) shown.get(k) == NOTHING
      else shown.get(k) == done.fold(NOTHING, (a, b) => if (a > b) a else b))

  /// A displayed result older than the document is marked stale.
  val supersededIsNeverShownAsCurrent: bool =
    KINDS.forall(k =>
      (shown.get(k) != NOTHING and shown.get(k) < revision) implies staleShown.contains(k))

  /// A displayed result is never from a revision that has not happened yet.
  val displayIsNotFromTheFuture: bool =
    KINDS.forall(k => shown.get(k) <= revision)

  /// Every running job is the in-flight job for its kind.
  val noJobOutlivesItsRequest: bool =
    jobs.keys().forall(j =>
      jobs.get(j).state == Running implies inflight.get(jobs.get(j).kind) == j)

  /// However many requests arrive, at most one job per kind is running, and
  /// every in-flight slot names a running job.
  val burstIsBounded: bool = and {
    running.size() <= KINDS.size(),
    KINDS.forall(k => inflight.get(k) == IDLE or jobs.get(inflight.get(k)).state == Running),
  }

  /// Nothing displayed came from an engine that cannot answer that kind.
  val fallbackRefusesRatherThanGuesses: bool =
    KINDS.forall(k => shown.get(k) == NOTHING or canAnswer(shownFrom.get(k), k))

  /// Every accepted job's revision is recorded as completed for its kind.
  val onlyAwaitedResultsAreAccepted: bool =
    jobs.keys().forall(j =>
      jobs.get(j).state == Accepted implies completed.get(jobs.get(j).kind).contains(jobs.get(j).rev))

  val allInvariants: bool = and {
    displayIsNewestCompleted,
    supersededIsNeverShownAsCurrent,
    displayIsNotFromTheFuture,
    noJobOutlivesItsRequest,
    burstIsBounded,
    fallbackRefusesRatherThanGuesses,
    onlyAwaitedResultsAreAccepted,
  }

  // ---- not invariants
  //
  // Guarantees the seam does not give. Each is false in some state a scenario
  // below reaches.

  /// The cancel registry holds only tokens of real jobs. False: `cadpole_cancel`
  /// inserts an entry for any token, and only a call that claims it removes it.
  val cancelRegistryIsBounded: bool = cancelRegistry.subseteq(jobs.keys())

  /// At most `n` jobs are abandoned. False for every `n`: a superseded job
  /// releases its caller and keeps running.
  def abandonedAtMost(n: int): bool = abandoned.size() <= n

  // ---- scenario-only actions
  //
  // Not in `step`: two display rules the seam rejects, a request that skips
  // the seam, and a cancel for a token that never ran.

  /// Show a finished result if it is newer than what is displayed.
  action finishAndShowAnything(j: int): bool = {
    val job = jobs.get(j)
    val k = job.kind
    val keep = job.rev > shown.get(k)
    all {
      jobs.keys().contains(j),
      job.state == Running,
      jobs' = jobs.setBy(j, x => { ...x, state: if (keep) Accepted else Dropped }),
      inflight' = if (inflight.get(k) == j) inflight.set(k, IDLE) else inflight,
      shown' = if (keep) shown.set(k, job.rev) else shown,
      shownFrom' = if (keep) shownFrom.set(k, job.at) else shownFrom,
      completed' = if (keep) completed.setBy(k, s => s.union(Set(job.rev))) else completed,
      staleShown' = if (keep) staleShown.exclude(Set(k)) else staleShown,
      revision' = revision, nextId' = nextId, pending' = pending, remoteUp' = remoteUp,
      abandoned' = abandoned, cancelRegistry' = cancelRegistry,
    }
  }

  /// Show whatever finished last.
  action finishAndShowLast(j: int): bool = {
    val job = jobs.get(j)
    val k = job.kind
    all {
      jobs.keys().contains(j),
      job.state == Running,
      jobs' = jobs.setBy(j, x => { ...x, state: Accepted }),
      inflight' = if (inflight.get(k) == j) inflight.set(k, IDLE) else inflight,
      shown' = shown.set(k, job.rev),
      shownFrom' = shownFrom.set(k, job.at),
      completed' = completed.setBy(k, s => s.union(Set(job.rev))),
      staleShown' = if (job.rev == revision) staleShown.exclude(Set(k)) else staleShown,
      revision' = revision, nextId' = nextId, pending' = pending, remoteUp' = remoteUp,
      abandoned' = abandoned, cancelRegistry' = cancelRegistry,
    }
  }

  /// A request that goes straight to the engine: nothing is coalesced or
  /// superseded, and the seam does not track the job.
  action startUnseamed(k: str): bool = all {
    KINDS.contains(k),
    nextId <= MAX_JOBS,
    jobs' = jobs.put(nextId, { kind: k, rev: revision, at: engineNow, state: Running }),
    nextId' = nextId + 1,
    revision' = revision, inflight' = inflight, pending' = pending, shown' = shown,
    shownFrom' = shownFrom, staleShown' = staleShown, completed' = completed,
    remoteUp' = remoteUp, abandoned' = abandoned, cancelRegistry' = cancelRegistry,
  }

  /// `cadpole_cancel(t)` for a token no call ever claimed.
  action cancelUnknownToken(t: int): bool = all {
    not(jobs.keys().contains(t)),
    cancelRegistry' = cancelRegistry.union(Set(t)),
    revision' = revision, jobs' = jobs, nextId' = nextId, inflight' = inflight,
    pending' = pending, shown' = shown, shownFrom' = shownFrom, staleShown' = staleShown,
    completed' = completed, remoteUp' = remoteUp, abandoned' = abandoned,
  }

  // ---- scenarios

  // A burst of requests during a drag leaves one request pending, then one
  // job running.
  run aBurstOfRequestsLeavesOneJobRunning =
    init
      .then(submit("Evaluate"))
      .then(start("Evaluate"))
      .then(edit)
      .then(submit("Evaluate"))
      .then(edit)
      .then(submit("Evaluate"))
      .then(edit)
      .then(submit("Evaluate"))
      .expect(running.size() == 0 and pending.get("Evaluate") == 3)
      .expect(burstIsBounded and noJobOutlivesItsRequest)
      .then(start("Evaluate"))
      .expect(running.size() == 1 and burstIsBounded)

  // A result computed before the last edit is dropped, and what stays on
  // screen is marked stale.
  run aSupersededResultIsDroppedAndTheViewSaysSo =
    init
      .then(submit("Evaluate"))
      .then(start("Evaluate"))
      .then(finish(1))
      .expect(shown.get("Evaluate") == 0 and not(staleShown.contains("Evaluate")))
      .then(edit)
      .expect(staleShown.contains("Evaluate") and supersededIsNeverShownAsCurrent)
      .then(submit("Evaluate"))
      .then(start("Evaluate"))
      .then(edit)
      .then(finish(2))               // job 2 is from revision 1; the document is at 2
      .expect(shown.get("Evaluate") == 0 and staleShown.contains("Evaluate"))
      .expect(allInvariants)

  // "Show it if it is newer" displays a revision 1 result as current while
  // the document is at revision 2.
  run aSupersededResultMustNotBeDisplayed =
    init
      .then(submit("Evaluate"))
      .then(start("Evaluate"))
      .then(edit)
      .then(submit("Evaluate"))
      .then(start("Evaluate"))
      .then(edit)
      .then(finishAndShowAnything(2))
      .expect(shown.get("Evaluate") == 1 and revision == 2)
      .expect(not(supersededIsNeverShownAsCurrent))

  // Through the seam a second request drops the first job before the second
  // starts, so the older result has no way back onto the display.
  run anOlderResultThatFinishesLaterIsNotDisplayed =
    init
      .then(submit("Render"))
      .then(start("Render"))
      .then(edit)
      .then(submit("Render"))        // job 1 is superseded: dropped, still running
      .then(start("Render"))
      .then(finish(2))
      .expect(shown.get("Render") == 1 and displayIsNewestCompleted)
      .expect(jobs.get(1).state == Dropped and abandoned.contains(1))
      .expect(allInvariants)

  // Without the seam both jobs run; the newer lands first, the older second,
  // and "last wins" puts the older model on screen.
  run withoutASeamTheDisplayFlickersBackwards =
    init
      .then(startUnseamed("Render"))
      .then(edit)
      .then(startUnseamed("Render"))
      .expect(running.size() == 2 and not(noJobOutlivesItsRequest))
      .then(finishAndShowLast(2))
      .expect(shown.get("Render") == 1 and displayIsNewestCompleted)
      .then(finishAndShowLast(1))
      .expect(shown.get("Render") == 0 and revision == 1)
      .expect(not(displayIsNewestCompleted))

  // A second cancel of the same job changes nothing.
  run cancellingTwiceChangesNothing =
    init
      .then(submit("Sweep"))
      .then(start("Sweep"))
      .then(cancel(1))
      .expect(inflight.get("Sweep") == IDLE and jobs.get(1).state == Dropped)
      .then(cancel(1))
      .expect(inflight.get("Sweep") == IDLE and jobs.get(1).state == Dropped)
      .expect(allInvariants)

  // Cancelling a token that never ran leaves a registry entry behind.
  run cancellingATokenThatNeverRanLeaksAnEntry =
    init
      .then(submit("Evaluate"))
      .then(start("Evaluate"))
      .then(cancelUnknownToken(99))
      .expect(not(cancelRegistryIsBounded))

  // The server goes away mid-evaluation: the same request runs locally and
  // its result is accepted.
  run whenTheServerGoesAwayTheEvaluationRunsLocally =
    init
      .then(submit("Evaluate"))
      .then(start("Evaluate"))
      .expect(jobs.get(1).at == "remote")
      .then(serverDown)
      .expect(pending.get("Evaluate") == 0 and inflight.get("Evaluate") == IDLE)
      .then(start("Evaluate"))
      .expect(jobs.get(2).at == "local")
      .then(finish(2))
      .expect(shown.get("Evaluate") == 0 and shownFrom.get("Evaluate") == "local")
      .expect(allInvariants)

  // A kind the local engine cannot answer is refused; the last server result
  // stays on screen, marked stale.
  run aKindTheLocalEngineCannotDoIsRefusedNotApproximated =
    init
      .then(submit("Export"))
      .then(start("Export"))
      .then(finish(1))
      .expect(shown.get("Export") == 0 and shownFrom.get("Export") == "remote")
      .then(edit)
      .then(serverDown)
      .then(submit("Export"))
      .then(start("Export"))
      .expect(jobs.get(2).at == "local")
      .then(refuse(2))
      .expect(shown.get("Export") == 0 and staleShown.contains("Export"))
      .expect(fallbackRefusesRatherThanGuesses and allInvariants)

  // The seam is per kind: a render superseding a render leaves the
  // evaluation in flight.
  run oneKindSupersedingItselfLeavesTheOthersAlone =
    init
      .then(submit("Evaluate"))
      .then(start("Evaluate"))
      .then(submit("Render"))
      .then(start("Render"))
      .then(submit("Render"))
      .expect(inflight.get("Evaluate") == 1 and inflight.get("Render") == IDLE)
      .expect(burstIsBounded and noJobOutlivesItsRequest)
      .then(finish(1))
      .expect(shown.get("Evaluate") == 0 and allInvariants)

  // Three supersedes leave three abandoned evaluations whose work goes on.
  run aBurstAbandonsWorkThatKeepsBurningCpu =
    init
      .then(submit("Evaluate"))
      .then(start("Evaluate"))
      .then(submit("Evaluate"))
      .then(start("Evaluate"))
      .then(submit("Evaluate"))
      .then(start("Evaluate"))
      .then(submit("Evaluate"))
      .expect(abandoned.size() == 3 and burstIsBounded)
      .expect(not(abandonedAtMost(2)))
}
