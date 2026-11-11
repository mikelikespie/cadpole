// The capability boundary of `cadpole serve`.
//
// The server mints an owner token and a limited listener token. The owner may
// call everything; a listener may call the operations granted in
// `cadpole-ops/src/capability.rs` and nothing else. The server enforces this
// in `capability_refusal` (cadpole-cli, server.rs) before any handler runs.
//
// The claim is that a listener cannot reach a denied operation by any path:
//
//   * routes: the op route, the Connect route, the two read-only GETs, and
//     every other `/api/` route, which a listener is refused whatever it does;
//   * spellings: the op route accepts an rpc name in any case, its snake_case
//     and CLI names, its aliases and dotted-qualified forms; the guard
//     resolves the spelling before deciding, and an unknown one is refused;
//   * effects: each operation carries what it can do, so the model can check
//     that no granted operation has an effect a denial was meant to withhold.
//
// `//spec:names_test` diffs OPS, GRANTED, SPELLINGS, ENGINE_REQUESTS and
// CONNECT_PREFIX against `ops.proto`, `engine.proto` and `capability.rs`.

module capability {
  // ---- constants

  // The registry: every rpc of `service CadpoleOps` in ops.proto.
  pure val OPS: Set[str] = Set(
    "Evaluate", "Render", "Sweep", "Animate", "Export", "PrintCheck", "Inspect", "Drawing",
    "RunTests", "ApiDoc",
    "GetSession", "GetSelection", "SetScript", "SetParams", "SetSelection",
    "Reply", "StreamReply", "PostMessage", "Save", "WaitEvents", "ExplainRef",
    "EditScript", "GetAttachment", "Ack", "ListAssets", "AddAsset", "GetAsset",
    "ReportClientFailure",
    "Solve", "ListJobs", "WaitJobs", "CancelJobs",
  )

  // What a listener may call: `GRANTED` in capability.rs.
  pure val GRANTED: Set[str] = Set(
    "GetSession", "GetSelection", "ExplainRef", "WaitEvents", "Ack",
    "GetAttachment", "ApiDoc", "Evaluate", "Render", "Sweep", "Animate", "PrintCheck",
    "Inspect", "Export", "Drawing", "RunTests", "ListAssets", "GetAsset", "SetParams",
    "EditScript", "Reply", "StreamReply", "ListJobs", "WaitJobs",
  )

  // Everything else is denied, so an unclassified operation is denied too.
  pure val DENIED: Set[str] = OPS.exclude(GRANTED)

  // Every spelling the op route accepts (`MethodInfo::matches` and `method()`
  // in cadpole-ops), mapped to the rpc it resolves to.
  pure val SPELLINGS: str -> str = Map(
    // rpc name
    "Evaluate" -> "Evaluate", "Render" -> "Render", "Sweep" -> "Sweep",
    "Animate" -> "Animate",
    "Export" -> "Export", "PrintCheck" -> "PrintCheck", "Inspect" -> "Inspect",
    "Drawing" -> "Drawing", "RunTests" -> "RunTests",
    "ApiDoc" -> "ApiDoc", "GetSession" -> "GetSession",
    "GetSelection" -> "GetSelection", "SetScript" -> "SetScript",
    "SetParams" -> "SetParams", "SetSelection" -> "SetSelection",
    "Reply" -> "Reply", "StreamReply" -> "StreamReply",
    "PostMessage" -> "PostMessage", "Save" -> "Save",
    "WaitEvents" -> "WaitEvents", "ExplainRef" -> "ExplainRef",
    "EditScript" -> "EditScript", "GetAttachment" -> "GetAttachment",
    "Ack" -> "Ack", "ListAssets" -> "ListAssets", "AddAsset" -> "AddAsset",
    "GetAsset" -> "GetAsset", "ReportClientFailure" -> "ReportClientFailure",
    "Solve" -> "Solve", "ListJobs" -> "ListJobs", "WaitJobs" -> "WaitJobs",
    "CancelJobs" -> "CancelJobs",
    // lower case: the rpc name is compared ignoring case
    "setscript" -> "SetScript", "save" -> "Save", "addasset" -> "AddAsset",
    "postmessage" -> "PostMessage", "setselection" -> "SetSelection",
    "reportclientfailure" -> "ReportClientFailure", "ack" -> "Ack",
    "render" -> "Render", "sweep" -> "Sweep", "export" -> "Export",
    "inspect" -> "Inspect", "animate" -> "Animate",
    "drawing" -> "Drawing", "reply" -> "Reply", "streamreply" -> "StreamReply",
    "api_doc" -> "ApiDoc",
    "printcheck" -> "PrintCheck", "apidoc" -> "ApiDoc",
    "runtests" -> "RunTests",
    "getsession" -> "GetSession", "getselection" -> "GetSelection",
    "setparams" -> "SetParams", "waitevents" -> "WaitEvents",
    "explainref" -> "ExplainRef", "editscript" -> "EditScript",
    "getattachment" -> "GetAttachment", "listassets" -> "ListAssets",
    "getasset" -> "GetAsset",
    "solve" -> "Solve", "listjobs" -> "ListJobs", "waitjobs" -> "WaitJobs",
    "canceljobs" -> "CancelJobs",
    // snake_case
    "set_script" -> "SetScript", "set_params" -> "SetParams",
    "set_selection" -> "SetSelection", "post_message" -> "PostMessage",
    "add_asset" -> "AddAsset", "report_client_failure" -> "ReportClientFailure",
    "edit_script" -> "EditScript", "get_session" -> "GetSession",
    "wait_events" -> "WaitEvents", "run_tests" -> "RunTests",
    "stream_reply" -> "StreamReply",
    // cli_name
    "eval" -> "Evaluate", "check" -> "PrintCheck", "doc" -> "ApiDoc",
    "test" -> "RunTests",
    "session" -> "GetSession", "selection" -> "GetSelection",
    "set-script" -> "SetScript", "set-params" -> "SetParams",
    "set-selection" -> "SetSelection", "post-message" -> "PostMessage",
    "stream-reply" -> "StreamReply",
    "listen" -> "WaitEvents", "explain-ref" -> "ExplainRef",
    "edit-script" -> "EditScript", "attachment" -> "GetAttachment",
    "assets-list" -> "ListAssets", "assets-add" -> "AddAsset",
    "assets-get" -> "GetAsset", "report-failure" -> "ReportClientFailure",
    "jobs" -> "ListJobs", "jobs-wait" -> "WaitJobs", "jobs-cancel" -> "CancelJobs",
    // alias
    "evaluate" -> "Evaluate", "print_check" -> "PrintCheck",
    "draw" -> "Drawing", "get_api_doc" -> "ApiDoc",
    "get_selection" -> "GetSelection", "msg" -> "Reply",
    "wait_event" -> "WaitEvents", "explain_ref" -> "ExplainRef",
    "get_attachment" -> "GetAttachment", "list_assets" -> "ListAssets",
    "get_asset" -> "GetAsset",
    "list_jobs" -> "ListJobs", "wait_jobs" -> "WaitJobs", "cancel_jobs" -> "CancelJobs",
    // dotted-qualified: only the text after the last dot is looked up
    "cadpole.v1.Save" -> "Save", "cadpole.v1.SetScript" -> "SetScript",
    "cadpole.v1.CadpoleOps.AddAsset" -> "AddAsset",
  )

  // Spellings that resolve to nothing. The guard refuses them to a listener.
  pure val UNKNOWN_SPELLINGS: Set[str] = Set(
    "DeleteEverything", "Sav", "save_everything", "..%2fSave", "cadpole.v1.", "",
  )

  // The Connect route's path prefix: `/api` plus the service's full name.
  pure val CONNECT_PREFIX: str = "/api/cadpole.v1.CadpoleOps"

  // The other `/api/` routes, each named by the operation it performs.
  pure val LEGACY_ROUTES: Set[str] = Set(
    "SetScript",        // PUT /api/session/script
    "SetSelection",     // PUT /api/session/selection
    "SetParams",        // PUT /api/session/params
    "PostMessage",      // POST /api/session/messages
    "Render",           // POST /api/render
    "WaitEvents",       // GET /api/events
    "GetSession",       // GET /api/session
    "GetAttachment",    // GET /api/session/messages/{seq}/attachment/{i}
    "Save",             // POST /api/session/save
  )

  // The variants of `engine.proto`'s `Request` oneof, which `POST /api/call`
  // dispatches without the registry. `Format` is not an rpc of `CadpoleOps`,
  // so capability.rs does not classify it.
  pure val ENGINE_REQUESTS: Set[str] = Set("Evaluate", "Export", "ApiDoc", "Format", "Render")

  // ---- types

  type Route =
    | OpRoute(str)       // POST /api/ops/<spelling>
    | ConnectRoute(str)  // POST /api/cadpole.v1.CadpoleOps/<Rpc>
    | ListOps            // GET /api/ops
    | DocumentRead       // GET /api/session/document
    | Legacy(str)        // any other /api/ route, by the operation it performs
    | Engine(str)        // POST /api/call and POST /api/render: the engine envelope

  // What an operation can do, at the grain the denial reasons argue at.
  type Effect =
    | ReadSession                            // document, script text, revision, messages
    | ReadModel                              // evaluate or tessellate a document
    | ReadAsset
    | ReadAttachment
    | EvaluateCallerSuppliedScript           // the request may carry its own `document`
    | ReplaceWholeFileBlind                  // overwrite a file without naming its text
    | CreateScriptFile
    | ReplaceNamedTextUnderBaseRevision      // the EditScript shape: named text, a line range or a feature's arguments
    | WriteParams
    | WriteSelection
    | WriteAsset
    | WriteDisk
    | CallerTextOnTheAssistantChannel        // caller-chosen text in the event log
    | UnattributedTextOnTheAssistantChannel  // ...not marked as coming from the caller
    | SpeakAsTheUser
    | WriteOwnCursor
    | StartLongSolves                        // jobs that outlive the request
    | StopSolveJobs                          // and forget their records
    | ReadSolveJobs                          // ids, states, timings, origins
    | KernelWorkOutsideTheRenderGate         // kernel time no wall-clock budget bounds (`lifecycle.qnt`)

  // ---- effects

  // Every evaluating operation accepts a caller-supplied `document`. Render,
  // Sweep and Animate run inside the render gate; PrintCheck, Inspect, Export,
  // Drawing and RunTests do their kernel work outside it.
  pure val EFFECTS: str -> Set[Effect] = Map(
    "ApiDoc" -> Set(),
    "GetSession" -> Set(ReadSession),
    "WaitEvents" -> Set(ReadSession),
    "ListAssets" -> Set(ReadSession),
    "GetAsset" -> Set(ReadSession, ReadAsset),
    "GetAttachment" -> Set(ReadAttachment),
    "Ack" -> Set(WriteOwnCursor),
    "GetSelection" -> Set(ReadSession, ReadModel),
    "ExplainRef" -> Set(ReadSession, ReadModel),
    "Evaluate" -> Set(ReadSession, ReadModel, EvaluateCallerSuppliedScript),
    "Render" -> Set(ReadSession, ReadModel, EvaluateCallerSuppliedScript),
    "Sweep" -> Set(ReadSession, ReadModel, EvaluateCallerSuppliedScript),
    "Animate" -> Set(ReadSession, ReadModel, EvaluateCallerSuppliedScript),
    "PrintCheck" -> Set(ReadSession, ReadModel, EvaluateCallerSuppliedScript, KernelWorkOutsideTheRenderGate),
    "Inspect" -> Set(ReadSession, ReadModel, EvaluateCallerSuppliedScript, KernelWorkOutsideTheRenderGate),
    "Export" -> Set(ReadSession, ReadModel, EvaluateCallerSuppliedScript, KernelWorkOutsideTheRenderGate),
    "Drawing" -> Set(ReadSession, ReadModel, EvaluateCallerSuppliedScript, KernelWorkOutsideTheRenderGate),
    "RunTests" -> Set(ReadSession, ReadModel, EvaluateCallerSuppliedScript, KernelWorkOutsideTheRenderGate),
    "SetParams" -> Set(WriteParams),
    // EditScript replaces only what each edit names (exact text, a line
    // range, a feature's arguments), under a matching `base_revision`, in a
    // file that already exists. Every line of a file can be named, so it can
    // rewrite a file; it cannot create one.
    "EditScript" -> Set(ReadSession, ReplaceNamedTextUnderBaseRevision),
    "Reply" -> Set(CallerTextOnTheAssistantChannel),
    // The same effect a piece at a time; reply_stream.qnt has its rules.
    "StreamReply" -> Set(CallerTextOnTheAssistantChannel),
    "SetScript" -> Set(ReplaceWholeFileBlind, CreateScriptFile),
    "SetSelection" -> Set(WriteSelection),
    "AddAsset" -> Set(WriteAsset),
    "PostMessage" -> Set(CallerTextOnTheAssistantChannel, SpeakAsTheUser),
    "ReportClientFailure" ->
      Set(CallerTextOnTheAssistantChannel, UnattributedTextOnTheAssistantChannel),
    "Save" -> Set(ReadSession, WriteDisk),
    // Solve is the only operation that starts jobs; Evaluate reads cached
    // answers and attaches to running jobs.
    "Solve" -> Set(ReadSession, ReadModel, EvaluateCallerSuppliedScript, StartLongSolves, ReadSolveJobs),
    "ListJobs" -> Set(ReadSolveJobs),
    "WaitJobs" -> Set(ReadSolveJobs),
    "CancelJobs" -> Set(ReadSolveJobs, StopSolveJobs),
  )

  pure def effectsOf(ops: Set[str]): Set[Effect] =
    ops.fold(Set(), (acc, o) => acc.union(EFFECTS.get(o)))

  /// The effects only a denied operation has.
  pure val DENIED_ONLY_EFFECTS: Set[Effect] =
    effectsOf(DENIED).exclude(effectsOf(GRANTED))

  // ---- state

  var role: str                 // "owner" | "listener" | "anon"
  var authOn: bool              // false when the server minted no tokens
  var executed: Set[str]        // rpcs whose handler has run for this caller
  var reached: Set[Effect]      // what those handlers could do
  var refusals: int
  var calls: str -> Set[str]    // which handler invokes which; empty unless a scenario adds an edge

  // ---- helpers

  /// The op route resolves a spelling to its rpc name; an unknown spelling
  /// stays as it is, and no listener is granted it.
  pure def resolve(spelling: str): str =
    if (SPELLINGS.keys().contains(spelling)) SPELLINGS.get(spelling) else spelling

  /// The Connect route accepts the rpc name only, so resolution is the identity.
  pure def resolveConnect(method: str): str = method

  /// The policy: with no tokens minted every caller is the owner.
  def mayCall(r: str, op: str): bool =
    if (not(authOn)) true
    else if (r == "owner") true
    else if (r == "anon") false
    else GRANTED.contains(op)

  /// The guard. Both registry routes use the same policy; the two GETs need
  /// any valid token; every other route is the owner's alone.
  def allowed(route: Route): bool =
    match route {
      | OpRoute(s) => mayCall(role, resolve(s))
      | ConnectRoute(m) => mayCall(role, resolveConnect(m))
      | DocumentRead => authOn implies role != "anon"
      | ListOps => authOn implies role != "anon"
      | Legacy(_) => not(authOn) or role == "owner"
      | Engine(_) => not(authOn) or role == "owner"
    }

  pure def expand(s: Set[str], g: str -> Set[str]): Set[str] =
    s.fold(s, (acc, o) => acc.union(g.get(o)))

  /// Every operation that running `s` reaches through `calls`. Four rounds
  /// is a fixpoint for any graph the scenarios build.
  def closure(s: Set[str]): Set[str] =
    1.to(4).fold(s, (acc, _) => expand(acc, calls))

  // ---- actions

  action init = all {
    role' = "anon",
    authOn' = true,
    executed' = Set(),
    reached' = Set(),
    refusals' = 0,
    calls' = OPS.mapBy(o => Set()),
  }

  /// A new caller presents a token of role `r`.
  action present(r: str): bool = all {
    Set("owner", "listener", "anon").contains(r),
    role' = r,
    executed' = Set(),
    reached' = Set(),
    refusals' = 0,
    authOn' = authOn, calls' = calls,
  }

  action anyPresent = {
    nondet r = Set("owner", "listener", "anon").oneOf()
    present(r)
  }

  /// One request. If the guard allows it, the handler runs, and so does
  /// everything that handler calls.
  action request(route: Route): bool = {
    val ran =
      match route {
        | OpRoute(s) => if (OPS.contains(resolve(s))) closure(Set(resolve(s))) else Set()
        | ConnectRoute(m) => if (OPS.contains(m)) closure(Set(m)) else Set()
        | Legacy(op) => closure(Set(op))
        | Engine(op) => if (OPS.contains(op)) closure(Set(op)) else Set()
        | DocumentRead => Set()
        | ListOps => Set()
      }
    all {
      executed' = if (allowed(route)) executed.union(ran) else executed,
      reached' =
        if (allowed(route))
          reached.union(effectsOf(ran))
            .union(if (route == DocumentRead) Set(ReadSession) else Set())
        else reached,
      refusals' = if (allowed(route)) refusals else refusals + 1,
      role' = role, authOn' = authOn, calls' = calls,
    }
  }

  action anyOpRoute = {
    nondet s = SPELLINGS.keys().union(UNKNOWN_SPELLINGS).oneOf()
    request(OpRoute(s))
  }

  /// Any name on the Connect route: rpc names, the op route's other
  /// spellings, and unknown ones.
  action anyConnectRoute = {
    nondet m = OPS.union(SPELLINGS.keys()).union(UNKNOWN_SPELLINGS).oneOf()
    request(ConnectRoute(m))
  }

  action anyLegacyRoute = {
    nondet op = LEGACY_ROUTES.oneOf()
    request(Legacy(op))
  }

  action anyEngineRoute = {
    nondet op = ENGINE_REQUESTS.oneOf()
    request(Engine(op))
  }

  // ---- step

  action step = any {
    anyPresent,
    anyOpRoute,
    anyConnectRoute,
    anyLegacyRoute,
    anyEngineRoute,
    request(ListOps),
    request(DocumentRead),
  }

  // ---- invariants

  /// No route, spelling or delegation runs a denied operation for a listener.
  val listenerNeverRunsADeniedOperation: bool =
    (authOn and role == "listener") implies executed.intersect(DENIED) == Set()

  /// Nothing a listener runs has an effect that only denied operations have.
  val listenerNeverReachesADeniedEffect: bool =
    (authOn and role == "listener") implies reached.intersect(DENIED_ONLY_EFFECTS) == Set()

  /// Everything a listener reaches is an effect of some granted operation.
  /// Unlike the one above, this also covers effects no denied operation has.
  val theListenersEffectsAreASubsetOfItsGrants: bool =
    (authOn and role == "listener") implies reached.subseteq(effectsOf(GRANTED))

  /// A caller without a valid token runs nothing.
  val anonymousRunsNothing: bool =
    (authOn and role == "anon") implies (executed == Set() and reached == Set())

  /// The owner is allowed every name on either registry route. A fact about
  /// the policy; no step changes it.
  val theOwnerIsNotRestricted: bool =
    OPS.union(SPELLINGS.keys()).union(UNKNOWN_SPELLINGS).forall(s => mayCall("owner", resolve(s)))

  /// Only real operations run: an unknown spelling executes nothing, for anybody.
  val onlyRealOperationsRun: bool = executed.subseteq(OPS)

  /// Each denied operation has an effect no granted operation has. A fact
  /// about the tables; no step changes it.
  val everyDenialBuysSomething: bool =
    DENIED.forall(op => EFFECTS.get(op).exclude(effectsOf(GRANTED)) != Set())

  /// Each operation is granted or denied, never both. A fact about the tables.
  val everyOperationIsClassified: bool =
    OPS.forall(o => GRANTED.contains(o) != DENIED.contains(o))

  /// Every spelling resolves to an operation that exists. A fact about the tables.
  val everySpellingResolves: bool =
    SPELLINGS.keys().forall(s => OPS.contains(SPELLINGS.get(s)))

  /// The op route and the Connect route agree about every operation, for the
  /// caller in hand: a second transport adds no privilege.
  val theSameOperationIsDeniedOnEveryRoute: bool =
    OPS.forall(op => allowed(OpRoute(op)) == allowed(ConnectRoute(op)))

  val allInvariants: bool = and {
    listenerNeverRunsADeniedOperation,
    listenerNeverReachesADeniedEffect,
    theListenersEffectsAreASubsetOfItsGrants,
    anonymousRunsNothing,
    theOwnerIsNotRestricted,
    onlyRealOperationsRun,
    everyDenialBuysSomething,
    everyOperationIsClassified,
    everySpellingResolves,
    theSameOperationIsDeniedOnEveryRoute,
  }

  // ---- not invariants
  //
  // Guarantees the boundary does not give. Each is false, and a scenario
  // below shows it.

  /// Every engine-envelope request is a classified operation. False: `Format`
  /// is not an rpc. A listener is kept from the envelope by the route alone.
  pure val everyEngineRequestIsAClassifiedOperation: bool = ENGINE_REQUESTS.subseteq(OPS)

  /// A listener's kernel work is all inside the render gate. False: five
  /// granted operations do kernel work outside it.
  val listenerWorkIsInsideTheRenderGate: bool =
    (authOn and role == "listener") implies not(reached.contains(KernelWorkOutsideTheRenderGate))

  // ---- scenario-only actions
  //
  // Not in `step`.

  /// The server runs with no tokens: every caller is the owner.
  action openTheServer = all {
    authOn,
    authOn' = false,
    role' = role, executed' = executed, reached' = reached, refusals' = refusals, calls' = calls,
  }

  /// Suppose `caller`'s handler invoked `callee`. No handler does; this shows
  /// the invariants would catch one that did.
  action pretendThatCalls(caller: str, callee: str): bool = all {
    OPS.contains(caller),
    OPS.contains(callee),
    calls' = calls.setBy(caller, s => s.union(Set(callee))),
    role' = role, authOn' = authOn, executed' = executed, reached' = reached, refusals' = refusals,
  }

  // ---- scenarios

  // Every kind of spelling of a denied operation is refused on the op route.
  run noSpellingOfADeniedOperationGetsThrough =
    init
      .then(present("listener"))
      .then(request(OpRoute("SetScript")))
      .then(request(OpRoute("setscript")))
      .then(request(OpRoute("set_script")))
      .then(request(OpRoute("set-script")))
      .then(request(OpRoute("cadpole.v1.SetScript")))
      .then(request(OpRoute("Save")))
      .then(request(OpRoute("save")))
      .then(request(OpRoute("AddAsset")))
      .then(request(OpRoute("add_asset")))
      .then(request(OpRoute("assets-add")))
      .then(request(OpRoute("cadpole.v1.CadpoleOps.AddAsset")))
      .then(request(OpRoute("PostMessage")))
      .then(request(OpRoute("post_message")))
      .then(request(OpRoute("post-message")))
      .then(request(OpRoute("SetSelection")))
      .then(request(OpRoute("ReportClientFailure")))
      .then(request(OpRoute("report-failure")))
      .expect(executed == Set() and refusals == 17)
      .expect(allInvariants)

  // The Connect route refuses every denied operation by its rpc name, and
  // refuses the op route's other spellings because they are not methods there.
  run noDeniedOperationGetsThroughTheConnectRouteEither =
    init
      .then(present("listener"))
      .then(request(ConnectRoute("SetScript")))
      .then(request(ConnectRoute("SetSelection")))
      .then(request(ConnectRoute("AddAsset")))
      .then(request(ConnectRoute("PostMessage")))
      .then(request(ConnectRoute("Save")))
      .then(request(ConnectRoute("ReportClientFailure")))
      .then(request(ConnectRoute("set_script")))
      .then(request(ConnectRoute("set-script")))
      .then(request(ConnectRoute("setscript")))
      .then(request(ConnectRoute("post-message")))
      .then(request(ConnectRoute("assets-add")))
      .then(request(ConnectRoute("cadpole.v1.SetScript")))
      .expect(executed == Set() and refusals == 12)
      .expect(allInvariants)

  // A listener's granted operations work over Connect by rpc name. A CLI
  // spelling is refused there and runs nothing.
  run theListenerCanDoItsJobOverConnect =
    init
      .then(present("listener"))
      .then(request(ConnectRoute("GetSession")))
      .then(request(ConnectRoute("GetSelection")))
      .then(request(ConnectRoute("Evaluate")))
      .then(request(ConnectRoute("EditScript")))
      .then(request(ConnectRoute("Reply")))
      .then(request(ConnectRoute("WaitEvents")))
      .then(request(ConnectRoute("Ack")))
      .expect(refusals == 0)
      .expect(executed == Set("GetSession", "GetSelection", "Evaluate", "EditScript",
                              "Reply", "WaitEvents", "Ack"))
      .then(request(ConnectRoute("eval")))
      .then(request(ConnectRoute("msg")))
      .expect(refusals == 2)
      .expect(executed == Set("GetSession", "GetSelection", "Evaluate", "EditScript",
                              "Reply", "WaitEvents", "Ack"))
      .expect(allInvariants)

  // The other `/api/` routes are refused to a listener whatever they do, even
  // those whose operation is granted.
  run theHandWrittenRoutesAreRefusedByShape =
    init
      .then(present("listener"))
      .then(request(Legacy("SetScript")))
      .then(request(Legacy("SetSelection")))
      .then(request(Legacy("PostMessage")))
      .then(request(Legacy("Save")))
      .then(request(Legacy("WaitEvents")))
      .then(request(Legacy("Render")))
      .expect(executed == Set() and refusals == 6)
      .expect(allInvariants)

  // A listener's granted operations and the two read-only GETs all work on
  // the op route, under any spelling.
  run theListenerCanDoItsJob =
    init
      .then(present("listener"))
      .then(request(ListOps))
      .then(request(DocumentRead))
      .then(request(OpRoute("GetSession")))
      .then(request(OpRoute("get_selection")))
      .then(request(OpRoute("eval")))
      .then(request(OpRoute("edit-script")))
      .then(request(OpRoute("msg")))
      .then(request(OpRoute("listen")))
      .then(request(OpRoute("ack")))
      .expect(refusals == 0)
      .expect(executed == Set("GetSession", "GetSelection", "Evaluate", "EditScript",
                              "Reply", "WaitEvents", "Ack"))
      .expect(allInvariants)

  // An unknown spelling is refused to a listener and runs nothing even for
  // the owner, so the spelling table need not be complete to be safe.
  run anUnknownSpellingRunsNothing =
    init
      .then(present("listener"))
      .then(request(OpRoute("DeleteEverything")))
      .then(request(OpRoute("..%2fSave")))
      .then(request(OpRoute("save_everything")))
      .then(request(OpRoute("")))
      .expect(executed == Set() and refusals == 4)
      .then(present("owner"))
      .then(request(OpRoute("DeleteEverything")))
      .expect(executed == Set() and onlyRealOperationsRun)
      .expect(allInvariants)

  // If a granted operation's handler called a denied one, the invariants
  // would catch it.
  run aGrantedOperationCallingADeniedOneWouldBreakTheBoundary =
    init
      .then(present("listener"))
      .then(pretendThatCalls("EditScript", "SetScript"))
      .then(request(OpRoute("edit-script")))
      .expect(executed.contains("SetScript"))
      .expect(not(listenerNeverRunsADeniedOperation))
      .expect(not(listenerNeverReachesADeniedEffect))

  // The owner is refused nothing, on any route.
  run theOwnerReachesEverything =
    init
      .then(present("owner"))
      .then(request(OpRoute("SetScript")))
      .then(request(OpRoute("Save")))
      .then(request(Legacy("PostMessage")))
      .expect(refusals == 0 and executed == Set("SetScript", "Save", "PostMessage"))
      .expect(allInvariants)

  // With no tokens minted there is no boundary: a caller with no token runs
  // a denied operation. The invariants are guarded on `authOn`, so they hold.
  run openModeIsNotABoundary =
    init
      .then(present("anon"))
      .then(openTheServer)
      .then(request(OpRoute("Save")))
      .expect(executed == Set("Save") and refusals == 0)
      .expect(allInvariants)

  // `Reply` already puts caller-chosen text on the assistant channel. What
  // the denied `ReportClientFailure` adds is text with no attribution.
  run replyAlreadyReachesTheAssistantChannel =
    init
      .then(present("listener"))
      .then(request(OpRoute("Reply")))
      .expect(reached.contains(CallerTextOnTheAssistantChannel))
      .expect(not(reached.contains(UnattributedTextOnTheAssistantChannel)))
      .expect(allInvariants)

  // The engine envelope is refused to a listener by its route, not by
  // classification: `Format` is in neither list. For the owner, `Format`
  // runs no registry operation.
  run theEngineEnvelopeIsRefusedByShapeRatherThanByClassification =
    init
      .then(present("listener"))
      .then(request(Engine("Evaluate")))
      .then(request(Engine("Export")))
      .then(request(Engine("Render")))
      .then(request(Engine("Format")))
      .expect(executed == Set() and refusals == 4)
      .expect(allInvariants)
      .expect(not(everyEngineRequestIsAClassifiedOperation))
      .then(present("owner"))
      .then(request(Engine("Format")))
      .expect(executed == Set() and onlyRealOperationsRun)

  // A listener can run kernel work outside the render gate, over a document
  // it supplies. The boundary as classified still holds. Render and Sweep,
  // which are gated, do not have the effect.
  run aListenerReachesTheKernelOutsideTheRenderGate =
    init
      .then(present("listener"))
      .then(request(OpRoute("Export")))
      .then(request(OpRoute("draw")))
      .then(request(OpRoute("check")))
      .then(request(OpRoute("test")))
      .expect(refusals == 0)
      .expect(reached.contains(KernelWorkOutsideTheRenderGate))
      .expect(not(listenerWorkIsInsideTheRenderGate))
      .expect(allInvariants)
      .then(present("listener"))
      .then(request(OpRoute("render")))
      .then(request(OpRoute("sweep")))
      .expect(not(reached.contains(KernelWorkOutsideTheRenderGate)))
      .expect(listenerWorkIsInsideTheRenderGate)

  // A listener can evaluate a script of its own choosing, not only the
  // session's document.
  run aListenerCanEvaluateItsOwnScript =
    init
      .then(present("listener"))
      .then(request(OpRoute("eval")))
      .expect(reached.contains(EvaluateCallerSuppliedScript))
      .expect(allInvariants)
}
