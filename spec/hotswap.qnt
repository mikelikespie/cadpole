// What a running page does when the server rebuilds part of its bundle.
//
// `reload.qnt` models how the page learns of a rebuild; this models what it
// does next (`applyBuild` in src/web/src/hot-reload.ts). The build id names
// each file separately, so there are three cases:
//
//   * the stylesheet: swap the `<link>`;
//   * the wasm engine: start a second engine worker, check it answers, retire
//     the first, and evaluate again;
//   * the app bundle: a page cannot replace the code it is running, so it
//     offers a reload.
//
// Versions are integers that only go up: all the page does with a fingerprint
// is compare it.

module hotswap {
  // ---- constants

  pure val MAX_BUILDS: int = 3
  pure val MAX_RELOADS: int = 5   // above MAX_BUILDS, so an extra reload violates an invariant

  // ---- types

  /// The file a rebuild touched.
  type Part =
    | Styles
    | Engine
    | App

  // ---- state

  var serverStyles: int
  var serverEngine: int
  var serverApp: int
  var pageStyles: int
  var pageEngine: int
  var pageApp: int

  var engines: int         // engine workers the page holds; 2 during a swap
  var candidate: int       // version of the replacement being checked; 0 = none
  var running: int         // engine version evaluating now; 0 = idle
  var displayed: int       // engine version whose model is on screen
  var late: int            // a result abandoned by a swap and still in flight; 0 = none
  var staleShown: bool     // a result from a retired engine reached the screen

  var offered: bool        // the reload offer is up
  var dismissed: bool      // ...and collapsed by the user
  var pendingApp: bool     // an app rebuild the page has not yet handled

  var builds: int
  var appBuilds: int
  var reloads: int
  var swaps: int           // engine swaps completed
  var fallbacks: int       // engine swaps that failed and fell back to the offer

  // ---- helpers

  val swapping: bool = candidate != 0

  val upToDate: bool =
    pageStyles == serverStyles and pageEngine == serverEngine and pageApp == serverApp

  // ---- actions

  /// A page is open, current and idle, showing its engine's model.
  action init = all {
    serverStyles' = 1,
    serverEngine' = 1,
    serverApp' = 1,
    pageStyles' = 1,
    pageEngine' = 1,
    pageApp' = 1,
    engines' = 1,
    candidate' = 0,
    running' = 0,
    displayed' = 1,
    late' = 0,
    staleShown' = false,
    offered' = false,
    dismissed' = false,
    pendingApp' = false,
    builds' = 0,
    appBuilds' = 0,
    reloads' = 0,
    swaps' = 0,
    fallbacks' = 0,
  }

  /// The server rebuilds one file.
  action rebuild(part: Part): bool = all {
    builds < MAX_BUILDS,
    builds' = builds + 1,
    appBuilds' = if (part == App) appBuilds + 1 else appBuilds,
    serverStyles' = if (part == Styles) serverStyles + 1 else serverStyles,
    serverEngine' = if (part == Engine) serverEngine + 1 else serverEngine,
    serverApp' = if (part == App) serverApp + 1 else serverApp,
    pendingApp' = if (part == App) true else pendingApp,
    pageStyles' = pageStyles, pageEngine' = pageEngine, pageApp' = pageApp,
    engines' = engines, candidate' = candidate, running' = running,
    displayed' = displayed, late' = late, staleShown' = staleShown,
    offered' = offered, dismissed' = dismissed,
    reloads' = reloads, swaps' = swaps, fallbacks' = fallbacks,
  }

  action rebuildStyles = rebuild(Styles)
  action rebuildEngine = rebuild(Engine)
  action rebuildApp = rebuild(App)

  /// The page swaps the stylesheet. Nothing else is touched.
  action swapStyles = all {
    pageStyles != serverStyles,
    pageStyles' = serverStyles,
    serverStyles' = serverStyles, serverEngine' = serverEngine, serverApp' = serverApp,
    pageEngine' = pageEngine, pageApp' = pageApp,
    engines' = engines, candidate' = candidate, running' = running,
    displayed' = displayed, late' = late, staleShown' = staleShown,
    offered' = offered, dismissed' = dismissed, pendingApp' = pendingApp,
    builds' = builds, appBuilds' = appBuilds, reloads' = reloads,
    swaps' = swaps, fallbacks' = fallbacks,
  }

  /// The page starts a replacement engine. The old one keeps answering.
  action startReplacement = all {
    pageEngine != serverEngine,
    not(swapping),
    candidate' = serverEngine,
    engines' = engines + 1,
    serverStyles' = serverStyles, serverEngine' = serverEngine, serverApp' = serverApp,
    pageStyles' = pageStyles, pageEngine' = pageEngine, pageApp' = pageApp,
    running' = running, displayed' = displayed, late' = late, staleShown' = staleShown,
    offered' = offered, dismissed' = dismissed, pendingApp' = pendingApp,
    builds' = builds, appBuilds' = appBuilds, reloads' = reloads,
    swaps' = swaps, fallbacks' = fallbacks,
  }

  /// The replacement does not start or fails its health check. The page keeps
  /// the old engine and offers a reload instead.
  action replacementFails = all {
    swapping,
    candidate' = 0,
    engines' = engines - 1,
    offered' = true,
    fallbacks' = fallbacks + 1,
    serverStyles' = serverStyles, serverEngine' = serverEngine, serverApp' = serverApp,
    pageStyles' = pageStyles, pageEngine' = pageEngine, pageApp' = pageApp,
    running' = running, displayed' = displayed, late' = late, staleShown' = staleShown,
    dismissed' = dismissed, pendingApp' = pendingApp,
    builds' = builds, appBuilds' = appBuilds, reloads' = reloads, swaps' = swaps,
  }

  /// The page installs the checked replacement: the running evaluation is
  /// abandoned (its result becomes `late`), the old engine is retired, and a
  /// fresh evaluation starts. What is on screen stays until that one finishes.
  action installReplacement = all {
    swapping,
    candidate' = 0,
    engines' = engines - 1,
    pageEngine' = candidate,
    late' = running,
    running' = candidate,
    swaps' = swaps + 1,
    serverStyles' = serverStyles, serverEngine' = serverEngine, serverApp' = serverApp,
    pageStyles' = pageStyles, pageApp' = pageApp,
    displayed' = displayed, staleShown' = staleShown,
    offered' = offered, dismissed' = dismissed, pendingApp' = pendingApp,
    builds' = builds, appBuilds' = appBuilds, reloads' = reloads, fallbacks' = fallbacks,
  }

  /// The page handles an app rebuild by offering a reload. A newer build
  /// raises an offer the user had collapsed.
  action offerReload = all {
    pendingApp,
    offered' = true,
    dismissed' = false,
    pendingApp' = false,
    serverStyles' = serverStyles, serverEngine' = serverEngine, serverApp' = serverApp,
    pageStyles' = pageStyles, pageEngine' = pageEngine, pageApp' = pageApp,
    engines' = engines, candidate' = candidate, running' = running,
    displayed' = displayed, late' = late, staleShown' = staleShown,
    builds' = builds, appBuilds' = appBuilds, reloads' = reloads,
    swaps' = swaps, fallbacks' = fallbacks,
  }

  /// The user collapses the offer. `offered` stays true.
  action dismiss = all {
    offered,
    not(dismissed),
    dismissed' = true,
    serverStyles' = serverStyles, serverEngine' = serverEngine, serverApp' = serverApp,
    pageStyles' = pageStyles, pageEngine' = pageEngine, pageApp' = pageApp,
    engines' = engines, candidate' = candidate, running' = running,
    displayed' = displayed, late' = late, staleShown' = staleShown,
    offered' = offered, pendingApp' = pendingApp,
    builds' = builds, appBuilds' = appBuilds, reloads' = reloads,
    swaps' = swaps, fallbacks' = fallbacks,
  }

  /// The offer is taken, by the user or by an unattended page.
  action reloadNow = all {
    offered,
    reloads < MAX_RELOADS,
    reloads' = reloads + 1,
    pageStyles' = serverStyles,
    pageEngine' = serverEngine,
    pageApp' = serverApp,
    displayed' = serverEngine,
    engines' = 1,
    candidate' = 0,
    running' = 0,
    late' = 0,
    offered' = false,
    dismissed' = false,
    pendingApp' = false,
    serverStyles' = serverStyles, serverEngine' = serverEngine, serverApp' = serverApp,
    staleShown' = staleShown, builds' = builds, appBuilds' = appBuilds,
    swaps' = swaps, fallbacks' = fallbacks,
  }

  /// An evaluation starts on the installed engine.
  action startEvaluation = all {
    running == 0,
    running' = pageEngine,
    serverStyles' = serverStyles, serverEngine' = serverEngine, serverApp' = serverApp,
    pageStyles' = pageStyles, pageEngine' = pageEngine, pageApp' = pageApp,
    engines' = engines, candidate' = candidate,
    displayed' = displayed, late' = late, staleShown' = staleShown,
    offered' = offered, dismissed' = dismissed, pendingApp' = pendingApp,
    builds' = builds, appBuilds' = appBuilds, reloads' = reloads,
    swaps' = swaps, fallbacks' = fallbacks,
  }

  /// An evaluation finishes on the installed engine and is displayed.
  action finishRunning = all {
    running != 0,
    running == pageEngine,
    displayed' = running,
    running' = 0,
    serverStyles' = serverStyles, serverEngine' = serverEngine, serverApp' = serverApp,
    pageStyles' = pageStyles, pageEngine' = pageEngine, pageApp' = pageApp,
    engines' = engines, candidate' = candidate, late' = late, staleShown' = staleShown,
    offered' = offered, dismissed' = dismissed, pendingApp' = pendingApp,
    builds' = builds, appBuilds' = appBuilds, reloads' = reloads,
    swaps' = swaps, fallbacks' = fallbacks,
  }

  /// The abandoned evaluation finishes anyway and its result is dropped.
  action dropLateResult = all {
    late != 0,
    late' = 0,
    serverStyles' = serverStyles, serverEngine' = serverEngine, serverApp' = serverApp,
    pageStyles' = pageStyles, pageEngine' = pageEngine, pageApp' = pageApp,
    engines' = engines, candidate' = candidate, running' = running,
    displayed' = displayed, staleShown' = staleShown,
    offered' = offered, dismissed' = dismissed, pendingApp' = pendingApp,
    builds' = builds, appBuilds' = appBuilds, reloads' = reloads,
    swaps' = swaps, fallbacks' = fallbacks,
  }

  // ---- step

  action step = any {
    rebuildStyles,
    rebuildEngine,
    rebuildApp,
    swapStyles,
    startReplacement,
    replacementFails,
    installReplacement,
    offerReload,
    dismiss,
    reloadNow,
    startEvaluation,
    finishRunning,
    dropLateResult,
  }

  // ---- invariants

  /// The page always has an engine, including during a swap.
  val alwaysAnEngine: bool = engines >= 1

  /// The page never holds more than the old engine and one replacement.
  val enginesAreBounded: bool = engines <= 2

  /// A result computed by a retired engine is never displayed.
  val noStaleDisplay: bool = not(staleShown)

  /// The page reloads only for an app rebuild or a failed engine swap. A
  /// stylesheet rebuild or a successful engine swap costs no reload.
  val reloadsOnlyForTheAppBundle: bool = reloads <= appBuilds + fallbacks

  /// Once the page has handled an app rebuild, it shows the offer for as long
  /// as it is behind. Dismissing does not clear it.
  val aStalePageAlwaysSaysSo: bool =
    (pageApp != serverApp and not(pendingApp)) implies offered

  /// When nothing is running or swapping, the model on screen came from the
  /// installed engine.
  val displayedComesFromTheLiveEngineWhenIdle: bool =
    (running == 0 and not(swapping) and late == 0) implies displayed == pageEngine

  val allInvariants: bool = and {
    alwaysAnEngine,
    enginesAreBounded,
    noStaleDisplay,
    reloadsOnlyForTheAppBundle,
    aStalePageAlwaysSaysSo,
    displayedComesFromTheLiveEngineWhenIdle,
  }

  // ---- not invariants

  /// An idle page with no offer is current. False between a rebuild and the
  /// page acting on it, so the scenarios assert it only at their end.
  val settledMeansCurrent: bool =
    (running == 0 and not(swapping) and not(offered)) implies upToDate

  // ---- scenario-only actions
  //
  // Not in `step`: behaviour the page must not have.

  /// The abandoned result is displayed instead of dropped.
  action applyLateResult = all {
    late != 0,
    displayed' = late,
    staleShown' = late != pageEngine,
    late' = 0,
    serverStyles' = serverStyles, serverEngine' = serverEngine, serverApp' = serverApp,
    pageStyles' = pageStyles, pageEngine' = pageEngine, pageApp' = pageApp,
    engines' = engines, candidate' = candidate, running' = running,
    offered' = offered, dismissed' = dismissed, pendingApp' = pendingApp,
    builds' = builds, appBuilds' = appBuilds, reloads' = reloads,
    swaps' = swaps, fallbacks' = fallbacks,
  }

  // ---- scenarios

  // A stylesheet rebuild costs no reload, no engine swap and no evaluation.
  run aStylesheetRebuildCostsNothing =
    init
      .then(rebuildStyles)
      .then(swapStyles)
      .expect(reloads == 0 and swaps == 0 and displayed == 1 and upToDate)
      .expect(allInvariants)
      .expect(settledMeansCurrent)

  // An engine rebuild is swapped in under a live page: two engines during the
  // check, and the old model on screen until the new engine produces one.
  run anEngineRebuildSwapsWithoutReloading =
    init
      .then(rebuildEngine)
      .then(startReplacement)
      .expect(engines == 2 and pageEngine == 1 and displayed == 1)
      .then(installReplacement)
      .expect(engines == 1 and pageEngine == 2 and displayed == 1 and running == 2)
      .then(finishRunning)
      .expect(displayed == 2 and reloads == 0 and swaps == 1)
      .expect(allInvariants)
      .expect(settledMeansCurrent)

  // A replacement that fails leaves the old engine in place and an offer up.
  run aFailedSwapKeepsTheOldEngine =
    init
      .then(rebuildEngine)
      .then(startReplacement)
      .then(replacementFails)
      .expect(engines == 1 and pageEngine == 1 and displayed == 1 and offered)
      .expect(alwaysAnEngine)
      .expect(reloadsOnlyForTheAppBundle)

  // An evaluation in flight when the swap installs is dropped, not displayed.
  run anInFlightEvaluationIsAbandonedRatherThanShown =
    init
      .then(startEvaluation)
      .then(rebuildEngine)
      .then(startReplacement)
      .expect(running == 1 and engines == 2)
      .then(installReplacement)
      .expect(late == 1 and running == 2)
      .then(dropLateResult)
      .expect(late == 0 and displayed == 1 and not(staleShown))
      .then(finishRunning)
      .expect(displayed == 2)
      .expect(allInvariants)

  // The same trace with the late result displayed violates `noStaleDisplay`.
  run applyingALateResultIsTheBugTheOrderingPrevents =
    init
      .then(startEvaluation)
      .then(rebuildEngine)
      .then(startReplacement)
      .then(installReplacement)
      .then(applyLateResult)
      .expect(displayed == 1 and pageEngine == 2)
      .expect(not(noStaleDisplay))

  // Only an app rebuild costs a reload, and it is offered, not taken. A
  // dismissed offer still stands while the page is behind.
  run onlyTheAppBundleReloads =
    init
      .then(rebuildStyles)
      .then(swapStyles)
      .then(rebuildEngine)
      .then(startReplacement)
      .then(installReplacement)
      .then(finishRunning)
      .expect(reloads == 0 and appBuilds == 0)
      .then(rebuildApp)
      .then(offerReload)
      .then(dismiss)
      .expect(offered and dismissed and pageApp == 1 and serverApp == 2)
      .expect(aStalePageAlwaysSaysSo)
      .then(reloadNow)
      .expect(reloads == 1 and appBuilds == 1 and upToDate and not(offered))
      .expect(allInvariants)
      .expect(settledMeansCurrent)
}
