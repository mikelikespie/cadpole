// How local clients reach `cadpole serve`: two surfaces, the runtime
// directory, discovery and reconnection.
//
// A server listens on two surfaces. The browser surface is TCP: the web UI,
// its static files and every route the page uses. The agent surface is a Unix
// socket in the user's private runtime directory: the operation registry and
// the two reads an agent client needs, nothing else. Each run mints three
// tokens: the browser's (admitted on both surfaces), the agent owner's and the
// listener's (admitted on the socket, and on TCP only with `--agent-tcp`).
// Which surface a request came in on decides only whether its token is
// admitted and whether its route exists there; what an admitted token may do
// is decided by its role alone.
//
// Each run writes, 0600, into the 0700 runtime directory: its socket, a record
// that says where it is (no secrets), its agent tokens, and the browser link
// when its output is not a terminal. A clean stop removes them; a crash leaves
// them. A starting server removes what dead servers left, after a connect
// shows they are dead, and refuses to start in a directory that is not the
// user's own private directory.
//
// A client started with no `--server` (`cadpole mcp`, `cadpole op`, `cadpole
// listen`, the listener) reads the records, keeps the ones that are the
// user's own private files and whose server answers a connect, and picks one:
// the only one; else the only one whose document is in its directory; else it
// asks. Once it has used a server it is pinned to that server's document: when
// the server goes away it finds a new server of the same document, with that
// run's token, or says that there is none. It retries a request only when the
// connect failed or the token was refused, which is before anything ran. The
// two reads that carry an event-log cursor (`WaitEvents`, `Ack`) are not
// retried on a new server: the failure reaches the reader, which starts over
// on the new log (wakeups.qnt). The calls here are the other operations.
// The limited listener does not follow a restart at all: it stays with the
// run it found and says to be started again.
//
// The code: `src/rust/cadpole-cli/src/runtime.rs` (records, sweep, discovery,
// the endpoint), `server.rs` (`serve`, `bind_agent_socket`, `agent_routes`,
// `build_agent_router`, the guard), `security.rs` (`TokenSet::for_run`,
// `Surface`, `Admission`), `ops_host.rs` (`HttpHost::request`).
// `//spec:names_test` diffs AGENT_ROUTES against `agent_routes` in server.rs.
// `spec/replay/transport_replay.mjs` drives the real programs through this
// model's traces (`stepReplay`) and compares them after every step.

module transport {
  // ---- constants

  pure val DOCS: Set[str] = Set("alpha", "beta")

  // A client's working directory: one document's, or neither.
  pure val CWDS: Set[str] = Set("alpha", "beta", "elsewhere")

  pure val CLIENTS: Set[int] = Set(1, 2)

  // Server instances are numbered from 1; FOREIGN is a live endpoint whose
  // files are not the user's own private files.
  pure val FOREIGN: int = -1
  pure val NONE: int = 0
  pure val MAX_SERVERS: int = 4
  pure val MAX_CALLS: int = 8
  pure val MAX_REQUESTS: int = 10

  // The routes the agent socket serves: `agent_routes` in server.rs.
  pure val AGENT_ROUTES: Set[str] = Set(
    "GET /api/session",
    "GET /api/session/document",
    "GET /api/ops",
    "POST /api/ops/{method}",
    "POST /api/cadpole.v1.CadpoleOps/{method}",
  )

  // A browser-only API route and the page itself, standing for every route
  // the browser router adds to AGENT_ROUTES (the Rust tests list them all).
  pure val BROWSER_ONLY_ROUTES: Set[str] = Set("GET /api/documents", "GET /")
  pure val ROUTES: Set[str] = AGENT_ROUTES.union(BROWSER_ONLY_ROUTES)

  // The two GETs outside the registry a listener token may make.
  pure val LISTENER_READS: Set[str] = Set("GET /api/ops", "GET /api/session/document")
  pure val OP_ROUTES: Set[str] = Set("POST /api/ops/{method}", "POST /api/cadpole.v1.CadpoleOps/{method}")

  // One granted and one denied operation: capability.qnt holds the full lists.
  pure val OPS: Set[str] = Set("GetSession", "SetScript")
  pure val GRANTED: Set[str] = Set("GetSession")

  pure val SURFACES: Set[str] = Set("browser", "agent")

  // What a request presents: one of the server's three tokens, the agent
  // token of an earlier run, a wrong one, or none.
  pure val PRESENTED: Set[str] = Set("browser", "agent", "listener", "stale", "wrong", "none")

  // ---- types

  type Server = {
    doc: str,
    agentTcp: bool,
    running: bool,              // the process exists
    bound: bool,                // its socket answers a connect
    files: Set[str],            // of "sock", "record", "tokens", "url"
  }

  type Client = {
    cwd: str,
    listener: bool,             // `cadpole mcp --listener`
    pinned: bool,
    doc: str,                   // the document it is pinned to
    target: int,                // the server it last used, or NONE
    tokenOf: int,               // the run whose token it holds, or NONE
  }

  // One logical call of a client, and what became of it.
  type Call = {
    client: int,
    logical: int,               // the same for a retry of the same call
    op: str,
    target: int,
    tokenOf: int,               // the run whose token went with it
    boundAtSend: bool,
    pinned: bool,               // pinned when it was sent
    doc: str,                   // and to which document
    stage: str,                 // "sent", "done", "failed"
    outcome: str,               // "reached", "denied", "unauthenticated", or "" while sent
    applied: int,               // how many times the operation ran
  }

  // What a client's look-up found, when it found nothing to use.
  type Lookup = { client: int, error: str, usable: bool }

  // A single request made straight at a surface.
  type Request = {
    server: int,
    surface: str,
    route: str,
    op: str,
    presented: str,
    outcome: str,               // "unauthenticated", "notHere", "denied", "absent", "reached"
  }

  type State = {
    servers: int -> Server,
    next: int,
    dirOk: bool,                // the runtime directory is the user's own and 0700
    foreign: bool,              // a live endpoint with a loose record is there
    clients: int -> Client,
    calls: List[Call],
    lookups: List[Lookup],
    requests: List[Request],
    looseStarts: int,           // servers started in a directory that was not private
    exposed: Set[str],          // where a token was written that is not a token file
    bug: str,                   // "" in the intended design; see the mutants
  }

  /// What the last action was, for the replay.
  type Did = {
    name: str,
    server: int,
    doc: str,
    flag: bool,
    client: int,
    op: str,
    surface: str,
    route: str,
    presented: str,
  }

  // ---- state

  var s: State
  var did: Did

  // ---- helpers

  pure val NOTHING: Did = { name: "", server: NONE, doc: "", flag: false, client: NONE, op: "", surface: "", route: "", presented: "" }

  pure def note(name: str): Did = { ...NOTHING, name: name }

  pure def freshClient(cwd: str, l: bool): Client = { cwd: cwd, listener: l, pinned: false, doc: "", target: NONE, tokenOf: NONE }

  pure def only(xs: Set[int]): int = xs.fold(NONE, (a, x) => if (a == NONE or x < a) x else a)

  def ids: Set[int] = s.servers.keys()

  def server(i: int): Server = s.servers.get(i)

  /// What discovery sees as candidates: servers whose record is there and
  /// whose socket answers a connect. A record alone proves nothing (a crash
  /// leaves it); a loose record is never trusted.
  def candidates: Set[int] =
    val real = ids.filter(i => server(i).files.contains("record") and (server(i).bound or s.bug == "trustsAStaleRecord"))
    if (s.foreign and s.bug == "followsALooseRecord") real.union(Set(FOREIGN)) else real

  def docOf(i: int): str = if (i == FOREIGN) "foreign" else server(i).doc

  /// `runtime::select`: the one candidate; else the only one of the client's
  /// directory; else an error. A pinned client looks only at its document.
  def pick(c: Client): { id: int, error: str } =
    val pool = if (c.pinned and s.bug != "switchesDocument") candidates.filter(i => docOf(i) == c.doc) else candidates
    val here = pool.filter(i => docOf(i) == c.cwd)
    if (not(s.dirOk)) { id: NONE, error: "dir" }
    else if (pool.size() == 0) { id: NONE, error: if (c.pinned) "gone" else "none" }
    else if (pool.size() == 1) { id: only(pool), error: "" }
    else if (here.size() == 1) { id: only(here), error: "" }
    else { id: NONE, error: "ambiguous" }

  /// Is there a live server this client could use right now?
  def usableFor(c: Client): bool =
    ids.exists(i => server(i).bound and server(i).files.contains("record") and (not(c.pinned) or server(i).doc == c.doc))

  def isLive(i: int): bool = i == FOREIGN or (ids.contains(i) and server(i).bound)

  /// What the server answers a token on a surface, for a route and an
  /// operation: the guard's order, then the route.
  def decide(i: int, surface: str, route: str, op: str, presented: str): str =
    val sv = server(i)
    val known = presented == "browser" or presented == "agent" or presented == "listener" or s.bug == "acceptsAnyToken"
    val admitted =
      presented == "browser"
      or surface == "agent"
      or sv.agentTcp
      or s.bug == "admitsAgentTokensOnTcp"
      or not(presented == "agent" or presented == "listener")
    val listener = presented == "listener"
    val served = surface == "browser" or AGENT_ROUTES.contains(route) or s.bug == "servesTheUiOnTheSocket"
    val mayCall =
      not(listener)
      or GRANTED.contains(op)
      or (s.bug == "decidesBySurface" and surface == "agent")
    if (route == "GET /") { if (served) "reached" else "absent" }
    else if (not(known)) "unauthenticated"
    else if (not(admitted)) "notHere"
    else if (listener and not(OP_ROUTES.contains(route)) and not(LISTENER_READS.contains(route))) "denied"
    else if (OP_ROUTES.contains(route) and not(mayCall)) "denied"
    else if (not(served)) "absent"
    else "reached"

  // ---- actions

  action initWith(bug: str): bool = {
    nondet cwd1 = CWDS.oneOf()
    nondet cwd2 = CWDS.oneOf()
    nondet l1 = Set(false, true).oneOf()
    nondet l2 = Set(false, true).oneOf()
    all {
      s' = {
        servers: Map(),
        next: 1,
        dirOk: true,
        foreign: false,
        clients: Map(1 -> freshClient(cwd1, l1), 2 -> freshClient(cwd2, l2)),
        calls: List(),
        lookups: List(),
        requests: List(),
        looseStarts: 0,
        exposed: Set(),
        bug: bug,
      },
      did' = note("init"),
    }
  }

  action init = initWith("")

  /// `cadpole serve DOC [--agent-tcp]`: sweep, bind, write. Refused in a
  /// directory that is not private.
  action start(doc: str, agentTcp: bool): bool = all {
    s.next <= MAX_SERVERS,
    if (not(s.dirOk) and s.bug != "startsInALooseDirectory") all {
      s' = s,
      did' = { ...note("startRefused"), doc: doc, flag: agentTcp },
    } else {
      // The sweep: files of servers that do not answer a connect. The
      // mutant sweeps without asking.
      val swept = ids.mapBy(i =>
        if (not(server(i).bound) or s.bug == "sweepsWithoutConnect") { ...server(i), files: Set() } else server(i))
      all {
        s' = {
          ...s,
          servers: swept.put(s.next, { doc: doc, agentTcp: agentTcp, running: true, bound: true, files: Set("sock", "record", "tokens", "url") }),
          next: s.next + 1,
          looseStarts: if (s.dirOk) s.looseStarts else s.looseStarts + 1,
          exposed: if (s.bug == "logsTheToken") s.exposed.union(Set("log")) else s.exposed,
        },
        did' = { ...note("start"), server: s.next, doc: doc, flag: agentTcp },
      }
    }
  }

  /// A call in flight to a server that goes away ends as a failure the
  /// client reports.
  def failCallsTo(i: int): List[Call] =
    s.calls.foldl(List(), (acc, k) => acc.append(if (k.target == i and k.stage == "sent") { ...k, stage: "failed" } else k))

  /// SIGTERM or Ctrl-C: the files go first, then the process.
  action stop(i: int): bool = all {
    ids.contains(i),
    server(i).running,
    s' = { ...s, servers: s.servers.put(i, { ...server(i), running: false, bound: false, files: Set() }), calls: failCallsTo(i) },
    did' = { ...note("stop"), server: i },
  }

  /// SIGKILL: every file stays behind.
  action crash(i: int): bool = all {
    ids.contains(i),
    server(i).running,
    s' = { ...s, servers: s.servers.put(i, { ...server(i), running: false, bound: false }), calls: failCallsTo(i) },
    did' = { ...note("crash"), server: i },
  }

  /// The runtime directory stops being the user's own private directory
  /// (in the replay: its path becomes a symlink), and back.
  action spoilDir: bool = all { s.dirOk, s' = { ...s, dirOk: false }, did' = note("spoilDir") }
  action restoreDir: bool = all { not(s.dirOk), s' = { ...s, dirOk: true }, did' = note("restoreDir") }

  /// A live endpoint appears with a record others could have written.
  action plantForeign: bool = all { not(s.foreign), s' = { ...s, foreign: true }, did' = note("plantForeign") }

  /// Where client `c` sends its next request, and with whose token: the
  /// server it used, while that answers; else a fresh look-up, with that
  /// run's token. The mutant `neverLooksAgain` sticks to a dead server.
  def route(c: Client): { id: int, tokenOf: int, error: str, client: Client } =
    if (c.target != NONE and isLive(c.target)) { id: c.target, tokenOf: c.tokenOf, error: "", client: c }
    else if (c.target != NONE and s.bug == "neverLooksAgain") { id: NONE, tokenOf: NONE, error: "gone", client: c }
    else
      val found = pick(c)
      // (The listener keeps the run it found even then.)
      if (found.error != "") { id: NONE, tokenOf: NONE, error: found.error, client: if (c.listener) c else { ...c, target: NONE } }
      // The limited listener stays with the run it found: a new run of its
      // document is an error that says to start it again.
      else if (c.listener and c.target != NONE) { id: NONE, tokenOf: NONE, error: "replaced", client: c }
      else
        val token = if (s.bug == "keepsTheStaleToken" and c.tokenOf != NONE) c.tokenOf else found.id
        val pinned = { ...c, target: found.id, tokenOf: token, pinned: true, doc: if (c.pinned) c.doc else docOf(found.id) }
        { id: found.id, tokenOf: token, error: "", client: pinned }

  /// What the server does with a call that reaches it.
  def answer(k: Call): str =
    val presented = if (k.tokenOf != k.target) "stale" else if (s.clients.get(k.client).listener) "listener" else "agent"
    if (k.target == FOREIGN) "reached" else decide(k.target, "agent", "POST /api/cadpole.v1.CadpoleOps/{method}", k.op, presented)

  /// A client sends a call; `atomic` also delivers it in the same step (the
  /// replay, where a call is one tool call of a real `cadpole mcp`).
  action send(c: int, op: str, atomic: bool): bool = all {
    s.calls.length() < MAX_CALLS,
    val cl = s.clients.get(c)
    val r = route(cl)
    // `cadpole mcp --listener` publishes only the granted operations and
    // refuses the rest itself, before looking for a server; the server's own
    // refusal is the `request` actions' business.
    if (cl.listener and not(GRANTED.contains(op))) all {
      s' = s,
      did' = { ...note("refusedLocally"), client: c, op: op, flag: atomic },
    } else if (r.error != "") all {
      s' = {
        ...s,
        clients: s.clients.put(c, r.client),
        lookups: s.lookups.append({ client: c, error: r.error, usable: usableFor(cl) }),
      },
      did' = { ...note("send"), client: c, op: op, flag: atomic },
    } else {
      val k0 = {
        client: c, logical: s.calls.length(), op: op, target: r.id, tokenOf: r.tokenOf,
        boundAtSend: isLive(r.id), pinned: cl.pinned, doc: cl.doc,
        stage: "sent", outcome: "", applied: 0,
      }
      val out = answer(k0)
      val k = if (atomic) { ...k0, stage: "done", outcome: out, applied: if (out == "reached") 1 else 0 } else k0
      all {
        s' = { ...s, clients: s.clients.put(c, r.client), calls: s.calls.append(k) },
        did' = { ...note("send"), client: c, op: op, server: r.id, flag: atomic },
      }
    }
  }

  /// The server answers call `n`.
  action deliver(n: int): bool = all {
    n < s.calls.length(),
    s.calls[n].stage == "sent",
    isLive(s.calls[n].target),
    val out = answer(s.calls[n])
    s' = { ...s, calls: s.calls.replaceAt(n, { ...s.calls[n], stage: "done", outcome: out, applied: s.calls[n].applied + if (out == "reached") 1 else 0 }) },
    did' = note("deliver"),
  }

  /// The server runs call `n` and the connection breaks before the answer:
  /// the client reports a failure. The mutant sends it again.
  action breakAfterRunning(n: int): bool = all {
    n < s.calls.length(),
    s.calls[n].stage == "sent",
    isLive(s.calls[n].target),
    answer(s.calls[n]) == "reached",
    val k = { ...s.calls[n], stage: "failed", applied: s.calls[n].applied + 1 }
    val retry = { ...k, stage: "sent", applied: 0 }
    s' = {
      ...s,
      calls: if (s.bug == "retriesAfterSend" and s.calls.length() < MAX_CALLS) s.calls.replaceAt(n, k).append(retry) else s.calls.replaceAt(n, k),
    },
    did' = note("breakAfterRunning"),
  }

  /// One request straight at a surface of server `i`.
  action request(i: int, surface: str, route: str, op: str, presented: str): bool = all {
    ids.contains(i),
    server(i).bound,
    s.requests.length() < MAX_REQUESTS,
    val out = decide(i, surface, route, op, presented)
    s' = { ...s, requests: s.requests.append({ server: i, surface: surface, route: route, op: op, presented: presented, outcome: out }) },
    did' = { ...note("request"), server: i, surface: surface, route: route, op: op, presented: presented },
  }

  action anyRequest: bool = {
    nondet i = ids.oneOf()
    nondet surface = SURFACES.oneOf()
    nondet route = ROUTES.oneOf()
    nondet op = OPS.oneOf()
    nondet presented = PRESENTED.oneOf()
    request(i, surface, route, op, presented)
  }

  action anyStart: bool = {
    nondet doc = DOCS.oneOf()
    nondet agentTcp = Set(false, true).oneOf()
    start(doc, agentTcp)
  }

  /// The replay starts a server only for a document that has no live one:
  /// with two live servers of one document, which of them a client that
  /// reconnects by itself (its event pump polls every few seconds) has
  /// already settled on is a matter of timing, not of the design.
  action anyStartOnePerDocument: bool = {
    nondet doc = DOCS.filter(d => not(ids.exists(i => server(i).bound and server(i).doc == d))).oneOf()
    nondet agentTcp = Set(false, true).oneOf()
    start(doc, agentTcp)
  }

  action anyStop: bool = {
    nondet i = ids.oneOf()
    stop(i)
  }
  action anyCrash: bool = {
    nondet i = ids.oneOf()
    crash(i)
  }

  action anySend(atomic: bool): bool = {
    nondet c = CLIENTS.oneOf()
    nondet op = OPS.oneOf()
    send(c, op, atomic)
  }

  action anyDeliver: bool = {
    nondet n = 0.to(MAX_CALLS - 1).oneOf()
    deliver(n)
  }
  action anyBreak: bool = {
    nondet n = 0.to(MAX_CALLS - 1).oneOf()
    breakAfterRunning(n)
  }

  action step = any {
    anyStart, anyStart, anyStart,
    anyStop, anyCrash,
    spoilDir, restoreDir, restoreDir,
    plantForeign,
    anySend(false), anySend(false), anySend(true), anySend(true),
    anyDeliver, anyDeliver, anyBreak,
    anyRequest, anyRequest,
  }

  /// What the replay can make real programs do one step at a time: calls are
  /// whole tool calls, so `deliver` and `breakAfterRunning` (a connection cut
  /// between running and answering, microseconds wide) are left to the
  /// simulation of `step`.
  action stepReplay = any {
    anyStartOnePerDocument, anyStartOnePerDocument, anyStartOnePerDocument,
    anyStop, anyCrash,
    spoilDir, restoreDir, restoreDir,
    plantForeign,
    anySend(true), anySend(true), anySend(true),
    anyRequest, anyRequest,
  }

  // ---- invariants: reconnect and faults

  /// Every call went to a server that answered a connect when it was sent:
  /// never to a stale socket, never to a dead server's record.
  val callsGoOnlyToLiveServers: bool = s.calls.indices().forall(n => s.calls[n].boundAtSend)

  /// Every call carried the token of the run it went to: a client that
  /// follows a restarted server takes that run's token too.
  val callsCarryTheirServersToken: bool = s.calls.indices().forall(n => s.calls[n].tokenOf == s.calls[n].target)

  /// No client ever follows a record or socket that is not the user's own
  /// private file.
  val clientsNeverFollowForeignFiles: bool = s.calls.indices().forall(n => s.calls[n].target != FOREIGN)

  /// The sweep never removes a live server's socket or record.
  val cleanupNeverRemovesALiveServersFiles: bool =
    ids.forall(i => server(i).bound implies server(i).files == Set("sock", "record", "tokens", "url"))

  /// A pinned client never moves to another document.
  val aClientNeverSwitchesDocuments: bool =
    s.calls.indices().forall(n => s.calls[n].pinned implies docOf(s.calls[n].target) == s.calls[n].doc)

  /// An operation runs at most once per call, retries included; a call that
  /// finished ran exactly when it was let through.
  val aCallRunsAtMostOnce: bool =
    s.calls.indices().forall(n =>
      s.calls.indices().filter(m => s.calls[m].logical == s.calls[n].logical).fold(0, (a, m) => a + s.calls[m].applied) <= 1)

  val aFinishedCallRanExactlyWhenAllowed: bool =
    s.calls.indices().forall(n => s.calls[n].stage == "done" implies (s.calls[n].applied == (if (s.calls[n].outcome == "reached") 1 else 0)))

  /// A client says "no server" or "gone" only when there is no live server
  /// it could use: after a restart it reconnects with no human step.
  val aClientRecoversWhenItsServerIsBack: bool =
    s.lookups.indices().forall(n => (s.lookups[n].error == "gone" or s.lookups[n].error == "none") implies not(s.lookups[n].usable))

  // ---- invariants: nothing insecure leaks

  /// The web UI and the browser-only routes are never served on the socket.
  val theWebUiIsNeverOnTheSocket: bool =
    s.requests.indices().forall(n => (s.requests[n].surface == "agent" and BROWSER_ONLY_ROUTES.contains(s.requests[n].route)) implies s.requests[n].outcome != "reached")

  /// An agent token is admitted on TCP only when the server was started
  /// with `--agent-tcp`.
  val agentTokensStayOffTcpUnlessAllowed: bool =
    s.requests.indices().forall(n =>
      val r = s.requests[n]
      (r.surface == "browser" and (r.presented == "agent" or r.presented == "listener") and r.route != "GET /" and not(server(r.server).agentTcp))
        implies r.outcome == "notHere")

  /// A token's decision for an operation is the same on every surface that
  /// admits it.
  val aTokensDecisionNeverDependsOnTheSurface: bool =
    s.requests.indices().forall(a => s.requests.indices().forall(b =>
      val x = s.requests[a]
      val y = s.requests[b]
      (x.server == y.server and x.presented == y.presented and x.route == y.route and x.op == y.op
        and OP_ROUTES.contains(x.route)
        and x.outcome != "notHere" and y.outcome != "notHere")
        implies x.outcome == y.outcome))

  /// The listener token never runs a denied operation, on any surface or
  /// through any client.
  val theListenerNeverRunsADeniedOperation: bool = and {
    s.requests.indices().forall(n =>
      val r = s.requests[n]
      (r.presented == "listener" and OP_ROUTES.contains(r.route) and not(GRANTED.contains(r.op))) implies r.outcome != "reached"),
    s.calls.indices().forall(n =>
      (s.clients.get(s.calls[n].client).listener and not(GRANTED.contains(s.calls[n].op))) implies s.calls[n].applied == 0),
  }

  /// No token, a wrong one, or an earlier run's, is refused on both surfaces.
  val aRequestWithoutAGoodTokenIsRefused: bool =
    s.requests.indices().forall(n =>
      val r = s.requests[n]
      (r.route != "GET /" and (r.presented == "none" or r.presented == "wrong" or r.presented == "stale")) implies r.outcome == "unauthenticated")

  /// No server ever ran in a directory that was not the user's own and 0700.
  val serversStartOnlyInAPrivateDirectory: bool = s.looseStarts == 0

  /// No token is written anywhere but its own 0600 file: not the record, not
  /// a log, not a command line, not the MCP output.
  val tokensNeverLeak: bool = s.exposed == Set()

  val allInvariants: bool = and {
    callsGoOnlyToLiveServers,
    callsCarryTheirServersToken,
    clientsNeverFollowForeignFiles,
    cleanupNeverRemovesALiveServersFiles,
    aClientNeverSwitchesDocuments,
    aCallRunsAtMostOnce,
    aFinishedCallRanExactlyWhenAllowed,
    aClientRecoversWhenItsServerIsBack,
    theWebUiIsNeverOnTheSocket,
    agentTokensStayOffTcpUnlessAllowed,
    aTokensDecisionNeverDependsOnTheSurface,
    theListenerNeverRunsADeniedOperation,
    aRequestWithoutAGoodTokenIsRefused,
    serversStartOnlyInAPrivateDirectory,
    tokensNeverLeak,
  }

  // ---- mutants
  //
  // Each is the intended design with one thing done wrong, and each is
  // simulated in the tests with the expectation that the named invariant
  // fails: an invariant no mutant can break is not known to check anything.

  /// Discovery trusts a record without a connect: `callsGoOnlyToLiveServers`.
  action initTrustsAStaleRecord = initWith("trustsAStaleRecord")
  /// Keeps the old run's token after following a restart: `callsCarryTheirServersToken`.
  action initKeepsTheStaleToken = initWith("keepsTheStaleToken")
  /// Follows a loose record: `clientsNeverFollowForeignFiles`.
  action initFollowsALooseRecord = initWith("followsALooseRecord")
  /// Sweeps without a connect test: `cleanupNeverRemovesALiveServersFiles`.
  action initSweepsWithoutConnect = initWith("sweepsWithoutConnect")
  /// Looks again without the pin: `aClientNeverSwitchesDocuments`.
  action initSwitchesDocument = initWith("switchesDocument")
  /// Retries a call that was already sent: `aCallRunsAtMostOnce`.
  action initRetriesAfterSend = initWith("retriesAfterSend")
  /// Never looks again once its server is gone: `aClientRecoversWhenItsServerIsBack`.
  action initNeverLooksAgain = initWith("neverLooksAgain")
  /// Serves the browser routes on the socket too: `theWebUiIsNeverOnTheSocket`.
  action initServesTheUiOnTheSocket = initWith("servesTheUiOnTheSocket")
  /// Admits the agent tokens on TCP without `--agent-tcp`: `agentTokensStayOffTcpUnlessAllowed`.
  action initAdmitsAgentTokensOnTcp = initWith("admitsAgentTokensOnTcp")
  /// Lets the socket decide what the listener may do: `aTokensDecisionNeverDependsOnTheSurface`
  /// and `theListenerNeverRunsADeniedOperation`.
  action initDecidesBySurface = initWith("decidesBySurface")
  /// Admits any token: `aRequestWithoutAGoodTokenIsRefused`.
  action initAcceptsAnyToken = initWith("acceptsAnyToken")
  /// Starts in a loose directory: `serversStartOnlyInAPrivateDirectory`.
  action initStartsInALooseDirectory = initWith("startsInALooseDirectory")
  /// Prints the link with its token to a log: `tokensNeverLeak`.
  action initLogsTheToken = initWith("logsTheToken")

  // ---- scenarios

  /// One server, a client with no flags, a call.
  run aClientWithNoFlagsFindsTheOneServer =
    init
      .then(start("alpha", false))
      .then(send(1, "GetSession", true))
      .then(all {
        assert(s.calls.length() == 1),
        assert(s.calls[0].target == 1 and s.calls[0].outcome == "reached"),
        s' = s, did' = did,
      })

  /// Two servers and a client in neither document's directory: it asks.
  run twoServersAndNoDirectoryToGoByIsAQuestion =
    init
      .then(all { s' = { ...s, clients: s.clients.put(1, { ...s.clients.get(1), cwd: "elsewhere" }) }, did' = did })
      .then(start("alpha", false))
      .then(start("beta", false))
      .then(send(1, "GetSession", true))
      .then(all { assert(s.calls.length() == 0 and s.lookups[0].error == "ambiguous"), s' = s, did' = did })

  /// A pinned client follows a restart of its document and not another.
  run aRestartIsFollowedAndAnotherDocumentIsNot =
    init
      .then(all { s' = { ...s, clients: s.clients.put(1, { ...s.clients.get(1), cwd: "elsewhere", listener: false }) }, did' = did })
      .then(start("alpha", false))
      .then(send(1, "GetSession", true))
      .then(start("beta", false))
      .then(stop(1))
      .then(send(1, "GetSession", true))
      .then(all { assert(s.lookups[0].error == "gone"), s' = s, did' = did })
      .then(start("alpha", false))
      .then(send(1, "GetSession", true))
      .then(all { assert(s.calls[1].target == 3 and s.calls[1].tokenOf == 3 and s.calls[1].outcome == "reached"), s' = s, did' = did })

  /// A crashed server's files stay, are not used, and go at the next start.
  run aCrashIsNotMistakenForALiveServer =
    init
      .then(start("alpha", false))
      .then(crash(1))
      .then(send(1, "GetSession", true))
      .then(all { assert(s.lookups[0].error == "none" and server(1).files.contains("sock")), s' = s, did' = did })
      .then(start("beta", false))
      .then(all { assert(server(1).files == Set() and server(2).bound), s' = s, did' = did })

  /// Over TCP the agent tokens are refused unless `--agent-tcp`; on the
  /// socket the listener's denial is the same either way.
  run theSurfacesAdmitAndDecideAsDocumented =
    init
      .then(start("alpha", false))
      .then(request(1, "browser", "POST /api/ops/{method}", "GetSession", "agent"))
      .then(request(1, "agent", "POST /api/ops/{method}", "SetScript", "listener"))
      .then(request(1, "browser", "POST /api/ops/{method}", "SetScript", "browser"))
      .then(request(1, "agent", "GET /", "GetSession", "browser"))
      .then(all {
        assert(s.requests[0].outcome == "notHere"),
        assert(s.requests[1].outcome == "denied"),
        assert(s.requests[2].outcome == "reached"),
        assert(s.requests[3].outcome == "absent"),
        s' = s, did' = did,
      })
      .then(start("beta", true))
      .then(request(2, "browser", "POST /api/ops/{method}", "SetScript", "listener"))
      .then(all { assert(s.requests[4].outcome == "denied"), s' = s, did' = did })

  /// A directory that is not private stops a server from starting.
  run aSpoiledDirectoryStopsTheServer =
    init
      .then(spoilDir)
      .then(start("alpha", false))
      .then(all { assert(s.servers.keys().size() == 0 and did.name == "startRefused"), s' = s, did' = did })
}
