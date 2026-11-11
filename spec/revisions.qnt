// Optimistic concurrency on the wire: `base_revision` and the revision a
// write's response returns.
//
// `session.qnt` states the protocol rule: a write is accepted exactly when its
// base is the current revision. This model adds what the wire adds.
//
//   * `base_revision` is a proto3 `uint64`, so "not set" and zero are the same
//     bytes. The session starts at revision 1, so zero is never a real base.
//   * The handlers in src/rust/cadpole-cli/src/server.rs read the field in two
//     ways. `set_script`, `edit_script` and `put_script` compare the number,
//     so a zero is always a conflict. `add_asset` treats zero as "skip the
//     check", so a zero is an unchecked write.
//   * A write's response returns the revision it committed, and the client
//     sends that as its next base. If the response instead carried the
//     session's revision at the time it was built, a concurrent write would
//     hand the client a revision whose content it never saw.
//
// `step` writes over the `op` route with the committed revision in the
// response, so every invariant holds there by construction. The scenarios show
// what the `asset` reading and the re-read revision break.

module revisions {
  // ---- constants

  pure val MAX_REV: int = 4

  // One entry per route that reads `base_revision` in server.rs:
  //   op    -- `set_script`, `edit_script`: compares the number
  //   json  -- `put_script` (`PUT /api/session/script`): compares the number
  //   asset -- `add_asset`: zero means no check
  pure val ROUTES: Set[str] = Set("op", "json", "asset")

  // ---- types

  /// What the caller meant. The wire cannot tell `Unset` from `At(0)`.
  type Base =
    | Unset        // no optimistic concurrency wanted
    | At(int)      // "I read revision n"

  type Write =
    | NoWrite
    | Wrote({ wire: int, revBefore: int, accepted: bool, checked: bool })

  /// A committed write whose response has not been read yet.
  type Reply =
    | NoReply
    | Committed({ revAtCommit: int })

  // ---- state

  var revision: int          // the session revision; starts at 1
  var writerBase: int        // the revision the writer will send as its next base
  var observed: Set[int]     // revisions whose content the writer has seen
  var lastWrite: Write
  var lostUpdates: int       // accepted, unchecked writes over a revision the writer had not read
  var unobservedWrites: int  // accepted writes over a revision the writer never observed
  var inFlight: Reply
  var echoesTheCurrentRevision: bool  // the response re-reads the revision instead of
                                      // carrying the one it committed; false in `step`

  // ---- helpers

  /// An unset field and a zero are the same bytes.
  pure def encode(b: Base): int =
    match b {
      | Unset => 0
      | At(n) => n
    }

  /// How a route reads the number back: only `asset` reads zero as "no base".
  pure def decode(route: str, wire: int): Base =
    if (wire == 0 and route == "asset") Unset else At(wire)

  /// Whether the route accepts the write at revision `rev`.
  pure def accepts(route: str, wire: int, rev: int): bool =
    match decode(route, wire) {
      | Unset => true
      | At(n) => n == rev
    }

  /// Whether the route compared the base with anything.
  pure def checks(route: str, wire: int): bool = decode(route, wire) != Unset

  // ---- actions

  action init = all {
    revision' = 1,
    writerBase' = 0,
    observed' = Set(),
    lastWrite' = NoWrite,
    lostUpdates' = 0,
    unobservedWrites' = 0,
    inFlight' = NoReply,
    echoesTheCurrentRevision' = false,
  }

  /// The writer reads the session: the revision and the content it names.
  action readSession = all {
    writerBase' = revision,
    observed' = observed.union(Set(revision)),
    revision' = revision, lastWrite' = lastWrite, lostUpdates' = lostUpdates,
    unobservedWrites' = unobservedWrites, inFlight' = inFlight,
    echoesTheCurrentRevision' = echoesTheCurrentRevision,
  }

  /// Another client writes.
  action someoneElseWrites = all {
    revision < MAX_REV,
    revision' = revision + 1,
    writerBase' = writerBase, observed' = observed, lastWrite' = lastWrite,
    lostUpdates' = lostUpdates, unobservedWrites' = unobservedWrites, inFlight' = inFlight,
    echoesTheCurrentRevision' = echoesTheCurrentRevision,
  }

  /// One write over one route.
  action write(route: str, meant: Base): bool = {
    val wire = encode(meant)
    val ok = accepts(route, wire, revision)
    val checked = checks(route, wire)
    all {
      ROUTES.contains(route),
      revision < MAX_REV,
      revision' = if (ok) revision + 1 else revision,
      lastWrite' = Wrote({ wire: wire, revBefore: revision, accepted: ok, checked: checked }),
      lostUpdates' =
        if (ok and not(checked) and writerBase != revision) lostUpdates + 1 else lostUpdates,
      unobservedWrites' =
        if (ok and not(observed.contains(revision))) unobservedWrites + 1 else unobservedWrites,
      inFlight' = if (ok) Committed({ revAtCommit: revision + 1 }) else inFlight,
      writerBase' = writerBase, observed' = observed,
      echoesTheCurrentRevision' = echoesTheCurrentRevision,
    }
  }

  /// A write over `op` with no base or with a base the writer has read.
  action anyWrite = {
    nondet meant = Set(Unset).union(observed.map(n => At(n))).oneOf()
    write("op", meant)
  }

  /// The writer reads the response and keeps its revision as the next base.
  /// It has observed that revision only if it is the one its own write made.
  action takeResponse = {
    val revAtCommit = match inFlight { | NoReply => 0 | Committed(p) => p.revAtCommit }
    val echoed = if (echoesTheCurrentRevision) revision else revAtCommit
    all {
      inFlight != NoReply,
      writerBase' = echoed,
      observed' = if (echoed == revAtCommit) observed.union(Set(revAtCommit)) else observed,
      inFlight' = NoReply,
      revision' = revision, lastWrite' = lastWrite, lostUpdates' = lostUpdates,
      unobservedWrites' = unobservedWrites,
      echoesTheCurrentRevision' = echoesTheCurrentRevision,
    }
  }

  /// The next write, based on the revision the last response returned.
  action writeAtTheEchoedRevision(route: str): bool = write(route, At(writerBase))

  // ---- step

  action step = any {
    readSession,
    someoneElseWrites,
    anyWrite,
    takeResponse,
    writeAtTheEchoedRevision("op"),
  }

  // ---- invariants

  /// The session never has revision zero, so a zero on the wire can only mean
  /// "unset".
  val zeroIsNeverALegitimateBase: bool = revision >= 1

  /// Every accepted write compared its base with the current revision.
  val everyAcceptedWriteChecked: bool =
    match lastWrite {
      | NoWrite => true
      | Wrote(w) => w.accepted implies w.checked
    }

  /// No unchecked write is accepted over a revision the writer had not read.
  val noLostUpdates: bool = lostUpdates == 0

  /// No accepted write is based on a revision whose content the writer never
  /// saw, even when the base it sent was current.
  val noWriteIsBasedOnAnUnobservedRevision: bool = unobservedWrites == 0

  /// A write is accepted exactly when the base it carried was current.
  val staleBaseRejectedOnTheWire: bool =
    match lastWrite {
      | NoWrite => true
      | Wrote(w) => w.accepted == (w.wire == w.revBefore)
    }

  val allInvariants: bool = and {
    zeroIsNeverALegitimateBase,
    everyAcceptedWriteChecked,
    noLostUpdates,
    noWriteIsBasedOnAnUnobservedRevision,
    staleBaseRejectedOnTheWire,
  }

  // ---- not invariants

  /// Every route gives the last write the same answer. False: a zero is a
  /// conflict on `op` and `json` and an unchecked write on `asset`.
  val allRoutesAgree: bool =
    match lastWrite {
      | NoWrite => true
      | Wrote(w) => ROUTES.forall(r => accepts(r, w.wire, w.revBefore) == w.accepted)
    }

  /// A write with no base can succeed on `op` at some revision. False: zero
  /// never equals a revision that starts at 1.
  val anAbsentBaseCanEverSucceed: bool =
    0.to(MAX_REV).exists(r => accepts("op", 0, r) and r >= 1)

  // ---- scenario-only actions
  //
  // Not in `step`: they choose which revision a write's response carries.

  action setEcho(current: bool): bool = all {
    echoesTheCurrentRevision' = current,
    revision' = revision, writerBase' = writerBase, observed' = observed,
    lastWrite' = lastWrite, lostUpdates' = lostUpdates,
    unobservedWrites' = unobservedWrites, inFlight' = inFlight,
  }

  /// The response carries the session's revision at the time it is built.
  /// The server must not do this.
  action theResponseRereadsTheRevisionAfterDroppingTheLock = setEcho(true)

  /// The response carries the revision the write committed, as the server does.
  action theResponseCarriesTheRevisionItCommitted = setEcho(false)

  // ---- scenarios

  // On `op` an unset or zero base is a conflict at every revision; the write
  // that succeeds names the revision it read.
  run aZeroBaseAlwaysConflictsOnTheOpRoute =
    init
      .then(write("op", Unset))
      .expect(revision == 1)
      .expect(match lastWrite { | NoWrite => false | Wrote(w) => not(w.accepted) and w.wire == 0 })
      .then(write("op", At(0)))
      .expect(match lastWrite { | NoWrite => false | Wrote(w) => not(w.accepted) })
      .expect(not(anAbsentBaseCanEverSucceed))
      .then(readSession)
      .then(write("op", At(1)))
      .expect(revision == 2)

  // The same bytes are refused on `op` and accepted unchecked on `asset`.
  run theSameRequestIsAcceptedOnOneRouteAndRefusedOnTheOther =
    init
      .then(write("asset", Unset))
      .expect(revision == 2)
      .expect(match lastWrite { | NoWrite => false | Wrote(w) => w.accepted and not(w.checked) })
      .expect(not(allRoutesAgree))
      .expect(not(everyAcceptedWriteChecked))

  // `json` makes the same decision as `op`.
  run theScriptRouteNowMakesTheSameDecisionAsSetScript =
    init
      .then(write("json", Unset))
      .expect(revision == 1)
      .expect(match lastWrite { | NoWrite => false | Wrote(w) => not(w.accepted) })
      .then(readSession)
      .then(write("json", At(1)))
      .expect(revision == 2)
      .expect(allInvariants)

  // With a stale writer, an unset base is a conflict on `op` and an accepted,
  // unchecked write on `asset`.
  run addAssetReadsTheSameFieldTheOppositeWayToSetScript =
    init
      .then(readSession)
      .then(someoneElseWrites)
      .then(write("op", Unset))
      .expect(revision == 2)
      .then(write("asset", Unset))
      .expect(revision == 3 and lostUpdates == 1)
      .expect(not(everyAcceptedWriteChecked))
      .expect(not(allRoutesAgree))

  // An unchecked write discards an edit the writer never saw.
  run anUncheckedWriteLosesSomebodyElsesEdit =
    init
      .then(readSession)
      .then(someoneElseWrites)
      .expect(writerBase == 1 and revision == 2)
      .then(write("asset", Unset))
      .expect(revision == 3 and lostUpdates == 1)
      .expect(not(noLostUpdates))
      .expect(not(noWriteIsBasedOnAnUnobservedRevision))

  // If the response re-reads the revision, a concurrent write between commit
  // and response hands the writer a base that is current but names content it
  // never saw. Its next write passes the check and overwrites that content.
  run theEchoedRevisionIsSomebodyElsesWork =
    init
      .then(theResponseRereadsTheRevisionAfterDroppingTheLock)
      .then(readSession)
      .then(write("op", At(1)))
      .expect(revision == 2 and unobservedWrites == 0)
      .then(someoneElseWrites)
      .then(takeResponse)
      .expect(writerBase == 3 and not(observed.contains(3)))
      .then(writeAtTheEchoedRevision("op"))
      .expect(revision == 4)
      .expect(everyAcceptedWriteChecked)
      .expect(noLostUpdates)
      .expect(unobservedWrites == 1)
      .expect(not(noWriteIsBasedOnAnUnobservedRevision))

  // With the committed revision in the response, the same interleaving ends
  // in a conflict instead.
  run carryingTheCommittedRevisionRestoresTheInvariant =
    init
      .then(theResponseCarriesTheRevisionItCommitted)
      .then(readSession)
      .then(write("op", At(1)))
      .then(someoneElseWrites)
      .then(takeResponse)
      .expect(writerBase == 2 and observed.contains(2))
      .then(writeAtTheEchoedRevision("op"))
      .expect(revision == 3)
      .expect(match lastWrite { | NoWrite => false | Wrote(w) => not(w.accepted) })
      .expect(allInvariants)

  // A stale writer with no base is refused on `op`; nothing is lost.
  run rejectingAnAbsentBaseIsTheRuleThatHolds =
    init
      .then(readSession)
      .then(someoneElseWrites)
      .then(write("op", Unset))
      .expect(revision == 2 and lostUpdates == 0)
      .then(readSession)
      .then(write("op", At(2)))
      .expect(revision == 3)
      .expect(allInvariants)

  // A current base is accepted and a stale one is refused.
  run aCurrentBaseIsAcceptedAndAStaleOneIsNot =
    init
      .then(readSession)
      .then(write("op", At(1)))
      .expect(revision == 2)
      .then(write("op", At(1)))
      .expect(revision == 2)
      .then(readSession)
      .then(write("op", At(2)))
      .expect(revision == 3)
      .expect(allInvariants)
}
