// The build-reload loop.
//
// `cadpole serve` appends a `build_changed` event to the session log when the
// built web UI changes (`spawn_build_watcher`, cadpole-cli/src/server.rs), and
// a page that sees one reloads itself (src/web/src/main.ts). The event stream
// replays from sequence zero on every page load, so the reloaded page sees the
// same event again and needs a guard to tell history from news.
//
// `step` uses the guard the page uses: ignore events at or below the
// `last_event_seq` the session reported at load. The scenarios show why three
// alternatives loop (no guard, a guard on `revision`, a guard on the event's
// build id), and that a sequence guard alone misses a rebuild across a server
// restart unless the page also compares the session's build on load.

module reload {
  // ---- constants

  pure val MAX_SEQ: int = 8
  pure val MAX_BUILDS: int = 2
  pure val MAX_RELOADS: int = 6    // above MAX_BUILDS, so a loop violates an invariant first

  // ---- types

  type EventKind =
    | BuildChanged
    | ContentChanged   // a script or parameter write: bumps `revision`
    | Chatter          // a client joining or leaving, a message: does not

  type Event = { seq: int, kind: EventKind, build: int }   // `build` matters for BuildChanged only

  // ---- state

  var log: List[Event]      // the session event log; seq == index + 1
  var revision: int         // counts content changes only
  var build: int            // the build the server is serving
  var builds: int           // how many times the bundle has changed
  var pageBuild: int        // the build the open page is running
  var guard: int            // the page ignores `build_changed` at or below this
  var cursor: int           // how far the page has consumed the log
  var reloads: int          // how many times the page has reloaded itself
  var connected: bool

  // ---- helpers

  val lastEventSeq: int = log.length()   // `Session.last_event_seq`
  val canEmit: bool = log.length() < MAX_SEQ

  def emit(k: EventKind, b: int): List[Event] =
    log.append({ seq: lastEventSeq + 1, kind: k, build: b })

  /// The next undelivered event is a `build_changed`.
  val nextIsBuild: bool =
    if (cursor < log.length()) log[cursor].kind == BuildChanged else false

  /// The build id the next undelivered event carries; -1 if there is none.
  val nextBuild: int = if (cursor < log.length()) log[cursor].build else -1

  // ---- actions

  action init = all {
    log' = [],
    revision' = 0,
    build' = 0,
    builds' = 0,
    pageBuild' = 0,
    guard' = 0,
    cursor' = 0,
    reloads' = 0,
    connected' = true,
  }

  /// A script or parameter write: one event, and the revision moves with it.
  action contentChange = all {
    canEmit,
    log' = emit(ContentChanged, build),
    revision' = revision + 1,
    build' = build, builds' = builds, pageBuild' = pageBuild, guard' = guard,
    cursor' = cursor, reloads' = reloads, connected' = connected,
  }

  /// An event that does not move the revision.
  action chatter = all {
    canEmit,
    log' = emit(Chatter, build),
    revision' = revision, build' = build, builds' = builds, pageBuild' = pageBuild,
    guard' = guard, cursor' = cursor, reloads' = reloads, connected' = connected,
  }

  /// The web bundle changes under the server.
  action rebuild = all {
    canEmit,
    builds < MAX_BUILDS,
    build' = build + 1,
    builds' = builds + 1,
    log' = emit(BuildChanged, build + 1),
    revision' = revision, pageBuild' = pageBuild, guard' = guard,
    cursor' = cursor, reloads' = reloads, connected' = connected,
  }

  /// The page takes the next event. If `fires`, it reloads: it runs whatever
  /// the server serves now, takes `newGuard`, and replays the log from zero.
  action deliver(fires: bool, newGuard: int): bool = all {
    connected,
    cursor < log.length(),
    reloads < MAX_RELOADS,
    cursor' = if (fires) 0 else cursor + 1,
    pageBuild' = if (fires) build else pageBuild,
    reloads' = if (fires) reloads + 1 else reloads,
    guard' = if (fires) newGuard else guard,
    log' = log, revision' = revision, build' = build, builds' = builds,
    connected' = connected,
  }

  /// The guard the page uses: reload on a `build_changed` above the
  /// `last_event_seq` read at load.
  action deliverBySeq = deliver(nextIsBuild and cursor + 1 > guard, lastEventSeq)

  /// The stream drops. A reconnect resumes after the page's own last
  /// sequence, so only a reload replays the log.
  action disconnect = all {
    connected,
    connected' = false,
    log' = log, revision' = revision, build' = build, builds' = builds,
    pageBuild' = pageBuild, guard' = guard, cursor' = cursor, reloads' = reloads,
  }

  action reconnect = all {
    connected' = true,
    log' = log, revision' = revision, build' = build, builds' = builds,
    pageBuild' = pageBuild, guard' = guard, cursor' = cursor, reloads' = reloads,
  }

  // ---- step

  action step = any {
    contentChange,
    chatter,
    rebuild,
    deliverBySeq,
    disconnect,
    reconnect,
  }

  // ---- invariants

  /// A page reloads at most once per build, so it cannot loop.
  val reloadsAreBoundedByBuilds: bool = reloads <= builds

  /// A page that has never seen a new build never reloads.
  val noBuildMeansNoReload: bool = builds == 0 implies reloads == 0

  /// A connected page that has consumed the whole log runs the served build.
  val caughtUpMeansCurrentBuild: bool =
    (connected and cursor == lastEventSeq) implies pageBuild == build

  val allInvariants: bool = and {
    reloadsAreBoundedByBuilds,
    noBuildMeansNoReload,
    caughtUpMeansCurrentBuild,
  }

  // ---- scenario-only actions
  //
  // Not in `step`: three guards that do not work, a server restart, and the
  // check on load that covers the restart.

  /// No guard: reload on every `build_changed`.
  action deliverUnguarded = deliver(nextIsBuild, guard)

  /// A sequence comparison against a guard taken from `revision`, which
  /// counts fewer things than `seq` does.
  action deliverByRevision = deliver(nextIsBuild and cursor + 1 > guard, revision)

  /// Reload when the event's build id differs from the page's own.
  action deliverByBuildId = deliver(nextIsBuild and nextBuild != pageBuild, guard)

  /// `cadpole serve` restarts: the in-memory log starts again at seq 1, and
  /// the open page keeps the guard it had.
  action serverRestart = all {
    log' = [],
    revision' = 0,
    cursor' = 0,
    build' = build, builds' = builds, pageBuild' = pageBuild, guard' = guard,
    reloads' = reloads, connected' = connected,
  }

  /// On load, the page compares the build the session reports with its own.
  action reloadIfSessionBuildDiffers = all {
    reloads < MAX_RELOADS,
    pageBuild != build,
    pageBuild' = build,
    reloads' = reloads + 1,
    guard' = lastEventSeq,
    cursor' = 0,
    log' = log, revision' = revision, build' = build, builds' = builds,
    connected' = connected,
  }

  // ---- scenarios

  // One rebuild, one reload; the replay after the reload fires nothing.
  run aRebuildReloadsThePageExactlyOnce =
    init
      .then(chatter)                 // seq 1
      .then(rebuild)                 // seq 2: build 1
      .then(deliverBySeq)
      .then(deliverBySeq)            // reload; guard := 2
      .expect(reloads == 1 and pageBuild == 1 and guard == 2 and cursor == 0)
      .then(deliverBySeq)
      .then(deliverBySeq)
      .expect(reloads == 1 and cursor == 2)
      .expect(allInvariants)

  // Two rebuilds before the page sees either: one reload lands on the newest
  // build and its guard covers the second event.
  run twoRebuildsInARowStillReloadOnce =
    init
      .then(rebuild)                 // seq 1: build 1
      .then(rebuild)                 // seq 2: build 2
      .then(deliverBySeq)            // reload; guard := 2, pageBuild := 2
      .expect(reloads == 1 and pageBuild == 2)
      .then(deliverBySeq)
      .then(deliverBySeq)
      .expect(reloads == 1 and cursor == 2)
      .expect(allInvariants)

  // With no guard the replayed event reloads the page every time.
  run withoutAGuardTheReplayedEventLoopsForEver =
    init
      .then(rebuild)                 // seq 1: build 1
      .then(deliverUnguarded)        // reload 1
      .then(deliverUnguarded)        // the same event again: reload 2
      .then(deliverUnguarded)        // reload 3
      .expect(reloads == 3 and builds == 1)
      .expect(not(reloadsAreBoundedByBuilds))

  // A guard taken from `revision` sits below the sequence it is compared
  // with, because chatter and `build_changed` move `seq` and not `revision`.
  run aRevisionGuardStillLoops =
    init
      .then(chatter)                 // seq 1; revision stays 0
      .then(rebuild)                 // seq 2; revision stays 0
      .expect(revision == 0 and lastEventSeq == 2)
      .then(deliverByRevision)
      .then(deliverByRevision)       // reload 1; guard := 0
      .expect(reloads == 1 and guard == 0 and cursor == 0)
      .then(deliverByRevision)
      .then(deliverByRevision)       // seq 2 > 0: reload 2
      .expect(reloads == 2)
      .then(deliverByRevision)
      .then(deliverByRevision)
      .expect(reloads == 3 and builds == 1)
      .expect(not(reloadsAreBoundedByBuilds))

  // A build-id guard loops on an older `build_changed` still in the log: the
  // page is on build 2 and build 1's event names a different id.
  run aBuildIdGuardAloneLoopsOnAStaleEvent =
    init
      .then(rebuild)                 // seq 1: build 1
      .then(rebuild)                 // seq 2: build 2
      .then(deliverByBuildId)        // seq 1 fires; pageBuild := 2
      .expect(reloads == 1 and pageBuild == 2)
      .then(deliverByBuildId)        // seq 1 again: 1 != 2
      .expect(reloads == 2)
      .then(deliverByBuildId)
      .expect(reloads == 3 and builds == 2)
      .expect(not(reloadsAreBoundedByBuilds))

  // After a restart the new log starts below the page's guard, so the page
  // ignores a real rebuild and stays on the old build without looping.
  run aSequenceGuardMissesABuildAfterARestart =
    init
      .then(chatter)
      .then(chatter)
      .then(chatter)
      .then(deliverBySeq)
      .then(deliverBySeq)
      .then(deliverBySeq)
      .then(rebuild)                 // seq 4: build 1
      .then(deliverBySeq)            // reload 1; guard := 4
      .expect(reloads == 1 and pageBuild == 1 and guard == 4)
      .then(serverRestart)
      .then(rebuild)                 // seq 1: build 2
      .expect(build == 2 and lastEventSeq == 1)
      .then(deliverBySeq)            // seq 1 <= guard 4: ignored
      .expect(reloads == 1 and pageBuild == 1 and cursor == 1)
      .expect(reloadsAreBoundedByBuilds)
      .expect(not(caughtUpMeansCurrentBuild))

  // Comparing the session's build on load does not depend on the log, so it
  // catches the rebuild the sequence guard missed.
  run checkingTheSessionBuildOnLoadSurvivesARestart =
    init
      .then(rebuild)                 // seq 1: build 1
      .then(deliverBySeq)            // reload 1; guard := 1
      .then(serverRestart)
      .then(rebuild)                 // seq 1: build 2
      .then(deliverBySeq)            // ignored: seq 1 <= guard 1
      .expect(pageBuild == 1 and build == 2)
      .then(reloadIfSessionBuildDiffers)
      .expect(reloads == 2 and pageBuild == 2 and builds == 2)
      .expect(allInvariants)
}
