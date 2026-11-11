# Cadpole web UI

TypeScript + three.js (WebGPU) front end for Cadpole, built Bazel-natively with
aspect `rules_js` / `rules_ts` / `rules_esbuild`. The Rust CLI (`cadpole serve`)
embeds and serves `//src/web:dist`.

```
bazel build //src/web:dist          # -> bazel-bin/src/web/dist/{index.html,styles.css,manifest.webmanifest,sw.js,app.js,engine-worker.js,worker-report.js (+ .map),cadpole_wasm.js,cadpole_wasm_bg.wasm}
bazel run   //src/web:devserver     # static dist + in-memory session server on http://127.0.0.1:7331
bazel run   //src/web:devserver -- --port 7460 --hot   # the same, able to rewrite one file of the bundle at a time
```

Open `http://127.0.0.1:7331/?mock=1` to use the built-in mock engine (no wasm
needed). Query flags (the list of record is the header of `src/main.ts`):

| Flag | Effect |
| --- | --- |
| `?mock=1` | Use `MockEngine` (a parametric plate + boss) instead of loading `./cadpole_wasm.js`. |
| `?engine=main` | Run the wasm engine on the page's thread instead of in its worker. |
| `?static=1` | Skip the `GetSession` probe and run in static / serverless mode. |
| `?nosw=1` | Do not register the service worker (useful while developing). |
| `?perf=1` | Record the apply path's phases and long tasks. |
| `#ex=<id>&p=w:60,d:40` | Static mode: pick an example and parameter overrides from the URL. |

## Layout of this directory

```
BUILD.bazel            npm_link_all_packages, ts_project (tsc, strict), esbuild bundle, copy_to_directory -> dist, devserver, e2e_test
package.json           deps: three; dev: @types/three, typescript, @playwright/test   (lockfile: //:pnpm-lock.yaml, workspace: //:pnpm-workspace.yaml)
.npmrc                 pnpm settings (no hoisting)
tsconfig.json          ES2022, strict, noUncheckedIndexedAccess, verbatimModuleSyntax
index.html             app shell (PWA meta, manifest link, loads ./app.js)
styles.css             theme + layout (wide: 3 columns; narrow: viewport + tab bar + bottom sheet)
manifest.webmanifest   PWA manifest ("Cadpole", standalone, inline SVG icons)
sw.js                  service worker: precache static files (+ wasm when present), SWR for assets, network-first for /api/*, never /api/events
devserver.mjs          node static server + in-memory session API (used by `bazel run :devserver` and by the e2e tests); `--script f.star` serves a different document
src/
  main.ts              bootstrap and wiring (engine, API, store, viewer, panels, keyboard); the query flags are documented at its top
  engine.ts            EngineLike — evaluate(req, signal) -> Promise<Evaluated> — plus WasmEngine (the engine on this thread), EnginePanic, MockEngine's contract
  engine-load.ts       picking the local engine at start-up: worker, then main thread, then mock, each step down reported
  engine-worker.ts     the engine, off the main thread: a module worker that loads cadpole_wasm.js (its own esbuild entry point, dist/engine-worker.js)
  worker-engine.ts     WorkerEngine: the page's handle on that worker; terminated to cancel, respawned after a Rust panic
  engine-protocol.ts   the messages between the page and the engine worker
  remote-engine.ts     RemoteEngine: one `Evaluate` rpc on `cadpole serve`, answered with a model whose bodies carry their meshes
  engine-policy.ts     which engine runs an evaluation, and whether it runs coarse-then-fine (measured, remembered)
  evaluator.ts         evaluation as one job with a state: supersede, cancel, fallback to local, the coarse-then-fine ladder
  jobs.ts              the job runner: named slots, one job at a time, supersede / coalesce / queue, cancellation
  hot-reload.ts        what a rebuilt bundle costs and the least that answers it: swap a stylesheet, swap the engine, or offer a reload
  mock.ts              MockEngine: plate + cylindrical boss with real topology, face ids, diagnostics, api_doc
  api.ts               SessionApi: HttpApi (the op registry over unary Connect + the /api/events SSE stream) and LocalApi (static mode: localStorage + URL hash); probeApi()
  connect.ts           the Connect protocol, unary, in the browser: the client half of src/rust/cadpole-cli/src/connect.rs, built from the generated service descriptor
  wire.ts              the only module that encodes or decodes protobuf: request/response shapes, cadpole.v1.Model -> the UI's Model plus meshes
  token.ts             the session token: read from the URL fragment, kept in sessionStorage, sent as Authorization: Bearer
  failure.ts           front-end failure reporting: panics, uncaught exceptions, rejections and swallowed operation errors -> ReportClientFailure
  worker-report.ts     the worker half of failure reporting (its own bundle, dist/worker-report.js)
  baked.ts             a published static site's baked models (see `cadpole publish`)
  section.ts           section planes in the viewer, cut client-side
  examples.ts          example scripts for the gallery / static mode
  state.ts             tiny reactive Store + AppState
  actions.ts           the Actions interface the panels call
  types.ts             TS shapes mirroring proto/cadpole/v1 (snake_case JSON)
  normalize.ts         defensive coercion of engine/server JSON into those shapes
  selection.ts         EntityRef helpers (keys, toggling, resolving against the model)
  opening.ts           how a page opens: `?section=`, `?mode=`, `?view=`, `?eye=`/`&target=` over the publisher's choices; the link to the current view
  viewer.ts            three.js WebGPU viewport: meshes, batched edges, overlays, labels, modes, ortho, bbox, snapshot; keeps a body whose triangles did not change
  drawing-buffer.ts    the canvas's drawing buffer: its size from the viewport's box, the pixel ratio and the level drawn; a resize is drawn into at once
  controls.ts          camera controller (trackpad/mouse/touch mapping, critically damped easing, inertia)
  picking.ts           face picking (Raycaster + mesh_face_ids) and screen-space edge picking
  pwa.ts               service worker registration + install prompt
  perf.ts              where the apply path's time goes: named spans + long tasks, on under `?perf=1`
  util.ts              debounce, number formatting, equality helpers
  ui/                  layout.ts (shell, tabs, bottom sheet), toolbar.ts, tree.ts, params.ts (the controls), inspector.ts,
                       messages.ts, diagnostics.ts, script.ts, examples.ts, apidoc.ts, picker.ts (popover),
                       build-bar.ts ("a new build is available"), hud.ts (perf),
                       plates.ts (the plates view's tabs and filament legend), layers.ts (the Layers panel),
                       section.ts (the Section panel), a11y.ts (live region, roving tabindex), dom.ts (h())
e2e/                   Playwright config and the projects' specs (see "Tests"): cadpole.spec.ts (mock mode,
                       screenshots in __screenshots__), resize.spec.ts (the viewport through a change of size,
                       on WebGL2 and on WebGPU), token.spec.ts, real-engine.spec.ts + real-engine.star,
                       engine.spec.ts + engine.star, perf.spec.ts + perf.star, hot-reload.spec.ts,
                       crash.spec.ts + crash-worker.js, toy.spec.ts (opt-in: the gear toy's plates)
```

## The apply-path benchmark

```
bazel test //src/web:e2e_test --test_arg=--project=perf --test_output=all --nocache_test_results
```

Drives the edits that matter -- a first load, a change to one body of forty, a
change to all forty, and a script edit that shifts every body id without moving
a triangle -- against `e2e/perf.star`, at two model sizes, and prints the phase
breakdown the app records under `?perf=1`. It **asserts counts** -- bodies
rebuilt, reused and flashed, triangles delivered -- and only prints times, so it
cannot go flaky on a loaded machine.

The same spec runs against `examples/gridfinity_baselid.star`, a model that
takes about thirteen seconds an evaluation, when asked:

```
bazel test //src/web:e2e_test --test_arg=--project=baselid \
  --test_env=CADPOLE_PERF_BASELID=1 --test_output=all --nocache_test_results
```

`--test_env=CADPOLE_PERF_TRACE=1` adds every span with its timestamp.

## Build pipeline

`MODULE.bazel` (section `# ---- Web (src/web)`) declares `aspect_rules_js`
3.4.1, `aspect_rules_ts` 3.10.1, `aspect_rules_esbuild` 0.27.0 (esbuild
0.28.0 toolchain) and `rules_nodejs` 6.7.5 (Node 22.22.0). npm packages come
from the pnpm lockfile at the repo root:

```
pnpm install --lockfile-only     # from the repo root, after editing package.json (pnpm 10; see pnpm-workspace.yaml)
```

`ts_project` type-checks and transpiles `src/**/*.ts` with `tsc` (strict);
`esbuild` bundles three entry points into ESM targeting `chrome130` —
`src/main.js` (`app.js`), `src/engine-worker.js` (`engine-worker.js`) and
`src/worker-report.js` (`worker-report.js`); `copy_to_directory` collects
them and the static files into the `dist` directory artifact that
`//src/rust/cadpole-cli` embeds. The protobuf message types and the `CadpoleOps`
service descriptor (`//src/web:cadpole_pb`, protoc-gen-es) are generated on every
build and never committed.

The wasm engine (`cadpole_wasm.js` + `cadpole_wasm_bg.wasm`, from
`//src/rust/cadpole-wasm:cadpole_wasm`) is **not** bundled: `engine-worker.js`
imports `./cadpole_wasm.js` at runtime next to `index.html`, in a dedicated module
worker. If the worker cannot start, `engine-load.ts` falls back to the
engine on the main thread, then to `MockEngine`, and each step down is
reported with a banner rather than only noticed.

## Two modes

At startup the app makes one `GetSession` call (`probeApi()` in `api.ts`):

- **Session mode** (`cadpole serve` or the dev server): every call to the server
  is one unary Connect rpc of `cadpole.v1.CadpoleOps` in binary protobuf —
  `POST /api/cadpole.v1.CadpoleOps/<Rpc>` (`wire.ts` is the only
  module that encodes anything). Evaluation runs in the engine worker by
  default and on the server (`Evaluate`) when the document is measured to be
  worth it; the page mirrors selection with `SetSelection` (debounced
  100 ms) and parameters with `SetParams`, listens to `/api/events` (SSE, JSON
  payloads, the one thing not on protobuf) and re-fetches /
  re-evaluates on `script_changed` / `params_changed`, applies assistant
  selections on `selection_changed`, posts chat with `PostMessage` (with
  screenshot attachments when snapshots are queued), writes with
  `SetScript`/`Save`, reports its own crashes with `ReportClientFailure`,
  and answers `build_changed` by swapping what changed.
- **Static mode** (no `/api`): the same UI with local-only state in
  `localStorage` and the URL hash, an examples gallery, an editable script
  (Edit / Apply, Ctrl+Enter), local notes instead of chat, and the API reference
  rendered from `engine.api_doc()`. Works offline once the service worker has
  cached the bundle (and the wasm files when present).

## Viewport controls

Trackpad and mouse follow macOS Preview / Xcode:

| Input | Action |
| --- | --- |
| Two-finger scroll | Orbit (tumble) around the target |
| Pinch | Zoom toward the cursor |
| Shift + two-finger scroll | Pan |
| Mouse wheel | Zoom toward the cursor (auto-detected; force with the "Scroll:" menu) |
| Left drag | Orbit |
| Middle / right drag, Shift + left drag | Pan |
| Double-click on geometry | Set the orbit target to the hit point and re-centre |
| Right-click | Picker popover (face / edge / body / feature under the cursor) |

Touch: one finger orbits, two fingers pinch-zoom and pan at once, double-tap
focuses, long-press opens the picker popover with context for the hovered
face/edge. All camera changes are critically damped springs with inertia on
release; `prefers-reduced-motion` makes them instant.

Selection: hover shows a soft outline; a click selects the most specific
candidate (edge within 6 px, else face); clicking the same spot again cycles
face -> edge -> body -> feature, and the popover lists all candidates. Shift /
Ctrl-click adds. Esc clears. Selected entities get an inline label in the
viewport and are mirrored in the tree and inspector.

Keyboard (when focus is not in a text field):

| Key | Action |
| --- | --- |
| `F` / `.` | Fit all / focus on selection |
| `H` `I` `1` `3` `7` | Home, iso, front, right, top |
| `5` | Perspective / orthographic |
| `V` / `E` / `X` | Cycle display mode / toggle edges / toggle x-ray |
| `G` `L` `B` `P` | Grid, labels, bounding box with dimensions, perf HUD |
| `Shift+S` | Snapshot the viewport for the next message (Shift-click the button for no labels) |
| `M` | Focus the message box |
| `Delete` / `Backspace` | On a body row in the feature tree: hide or show that body (the eye button is not a tab stop) |
| `Ctrl/Cmd+S` | Save |
| `Esc` | Close popover / sheet, then clear selection |
| Tree: arrows, Home/End, Left/Right, Enter/Space, Shift+Enter | Navigate, collapse/expand, select, add |

Display modes (toolbar select): shaded, shaded + edges (default), wireframe,
x-ray (translucent bodies + edges), faces (stable per-face colours). The
toolbar's "View" menu holds perspective/orthographic and the overlays:
selection labels, grid, bounding box with dimensions, performance HUD, plus
the scroll-behaviour setting. The axis triad in the viewport doubles as a view
cube (click any of the six axis ends for that view). Rendering is on demand:
frames are drawn only while the camera moves or something changed.

## Parameters

`ui/params.ts` builds one row per `param(...)`, from the kind of its default
and, for a string, its `choices`:

| Parameter | Control | On a coarse pointer (a phone) |
| --- | --- | --- |
| number | a slider (when it has `min`/`max`) beside a number field, `id="param-<name>"`, named `<name> (<unit>)`; a length under the document's `units(...)` also reads in that convention after its unit, `= 12'-6"` | the slider full width with a 44 px hit area, and the field in a stepper: `−  value unit  +`, one step (`step=`, else a round hundredth of the range) a tap; the field *is* the value, so tapping it types, with `inputmode="decimal"`; the length readout sits under the name |
| bool | a switch (`input type=checkbox role=switch`) whose label is the whole row, so the whole row toggles | the same, 44 px tall |
| string with `choices` | a segmented control when there are at most four choices of at most twelve characters (32 in all): a `radiogroup` of native radios styled as segments, arrow keys move; otherwise a native `<select>` | the same; the select is the phone's own picker |
| string | a text field | |
| vec3 | three decimal fields | |

Every row keeps its reset (shown while the parameter is overridden; on a
coarse pointer its place is kept while hidden, so the stepper never moves
under a thumb), an overridden row is marked with an accent bar, and "Reset
all" clears them. The rows are rebuilt only when the parameter list or the
pointer kind changes; values are written in place, never into a field that has
focus.

Where the panel lives: on a wide screen at the top of the **Model** tab, above
the feature tree (`.panel-slot`s in `layout.ts`); on a narrow screen it is the
first tab of the bottom bar, **Params**, so the Model tab keeps the tree. The
sheet opens at its half height (`--sheet-half`, 42 dvh; the heights are in
`layout.ts`) so the model stays in
view while a slider moves -- a slider owns the touches that start on it
(`touch-action: none`), so a drag never scrolls the list or moves the sheet --
and the handle (the whole strip above the title) pulls it up to full, which
is remembered per tab. The open sheet is parked in the per-document UI state
the moment it changes and when the tab goes into the background, so a phone
that kills the page comes back to the same panel.

The Layers section (`ui/layers.ts`) follows the same rule one slot
down: under the feature tree on a wide screen, and on a narrow one the
bottom bar's second tab, **Layers**, right after Params. That tab is there
only while the document declares a layer, as the section is; a Layers sheet
left open when the layers go (a restored sheet, a script changed elsewhere)
falls back to Params. Its visibility toggles are the parameters' switch,
each in a label that is a 44 px target on a coarse pointer.

### What lies over the viewport

On a narrow screen the sheet lies over the canvas instead of beside it, so
the viewer is told how much of each edge is covered and draws the model in
the middle of what is left:

```ts
viewer.setViewportInsets({ top, right, bottom, left }, { snap? });  // CSS px; all zero is the plain view
viewer.viewportInsets();                                            // what was last set
```

The insets become a view offset on both cameras (`Camera.setViewOffset`):
the centre of projection, where the orbit target is drawn, moves to the
middle of the uncovered rectangle, eased over about a fifth of a second, and
the scale of the picture does not change. Because the offset is part of the
projection matrix, picking, the labels, a section's planes and caps and the
edge test all agree with what is drawn without knowing about it; the one
place that measures from the centre of the canvas by hand, the zoom about the
pointer, reads the offset from `CameraController.centre`. Nothing moves in
the world: the orbit pose, the eye and the target are what they were, so the
pose kept across a reload and a link to the view are the same camera whatever
covered the viewport when they were made. Home, Fit and Focus fit the
uncovered rectangle. A snapshot is drawn without the offset, so that it goes
with the camera recorded beside it. An axis with less than 120 px left
uncovered is treated as uncovered.

`ui/layout.ts` is the only caller: it measures the sheet's box against the
viewport's, with the keyboard's as a second cover (`ui/insets.ts`: the insets
bound the largest stretch that no band across or down the viewport covers; a
corner card or a panel beside the viewport is no band), follows it with a
`ResizeObserver` through rests and drags and with the `cadpole:keyboard`
event when the keyboard moves it, hands the result to `Layout.onInsets`, and
writes the bottom inset to `--cover-b` so the axis triad and the Home and Fit
buttons stand above the sheet. Anything else that covers the viewport joins
by adding its box to that measurement.

### The drawing buffer

The canvas's drawing buffer is the viewport's box in whole CSS pixels times
the screen's pixel ratio, two at most. On a touch device, while a finger
turns the model or the camera eases to a pose and two frames running have
come slowly, the viewer draws a lighter picture: one device pixel a CSS
pixel, and after two more slow frames no edge lines; a moment after the view
comes to rest the full picture is back (`data-lean` on the viewport says
which level is drawn). The picture sliding out from under a sheet is not
such a move and is drawn at full resolution.

A canvas whose buffer changes size is cleared, and the browser composites
whatever the buffer holds at the end of the frame, which for a size that
changed in a `ResizeObserver` callback is before the next animation frame.
So everything that can change the size goes through one place
(`drawing-buffer.ts`, `Viewer.syncBuffer`): the viewport's `ResizeObserver`,
a media query on the current pixel ratio (a window moved to another display,
a page zoomed), and the lighter picture and its way back. A change resizes
the buffer, recomputes the camera's aspect and view offset, and draws the
scene into the new buffer before returning; the same size again does
nothing. A viewport with no area (hidden, collapsed) keeps the buffer it
had. `Viewer.drawnSize()` is the buffer's size when the picture in it was
drawn, which is the canvas's size at every paint.

One thing in the scene is tied to the buffer's size: a translucent fat line
(the hover outline) blends with a copy of the frame drawn so far, a texture
as large as the buffer, and the renderer does not re-bind a line's material
when it replaces that texture. Left alone, the line samples a destroyed
texture, WebGPU rejects every frame that draws it, and the canvas stays
empty from the first resize on. `Viewer.rebindViewportCopies` gives each
line material a change the renderer does watch for whenever the buffer is
resized.

If the browser takes the graphics device away, the viewport says so
(`.viewport-notice`, `data-graphics="restoring"`), makes a new renderer on
the same canvas -- at once for a WebGPU device, when the browser restores
it for a WebGL context -- and draws the scene with it; after three losses
in a minute, or if no device can be had, the notice offers a reload instead
(`data-graphics="failed"`).

Chat opens at the sheet's peek height, like the View sheet, and keeps its
height while the keyboard is up (every other tab takes the full height
then): what is typed there is about the model, and the answer is a change to
it.

## Accessibility

Every action is reachable by keyboard (roving tabindex in the tree, tab lists
with arrow keys, Escape closes the bottom sheet and returns focus, the
segmented control is a native radio group so its arrow keys are the
browser's), the viewport gestures all have toolbar buttons, the stepper's
buttons are named ("Increase width"), live regions announce selection
changes, diagnostics counts and connection changes, and the theme keeps text
at WCAG AA contrast.

## Tests

Playwright drives the built `dist` through the dev server. **Everything is
Bazel-native and hermetic**: the Chromium headless shell that matches the
pinned `@playwright/test` is downloaded and sha256-pinned by
`//build_support/playwright` (see `build_support/playwright/browsers.bzl`), so
nothing needs `playwright install` and nothing reads `~/.cache/ms-playwright`.

```
bazel test //src/web:e2e_test       # compare against e2e/__screenshots__
bazel run  //src/web:e2e_update     # regenerate those baselines in-tree
bazel test //src/web:unit_test      # the pure modules on node:test: the job runner, the stylesheet's shape (test/)
bazel test //src/web:examples_test  # every bundled example (src/examples.ts) through `cadpole eval`
```

`unit_test` is `node:test` over the transpiled output of `:ts`, with no
browser and no framework: anything that is a function of its inputs goes in
`test/*.test.mjs` rather than behind a Playwright server. It includes a scan
of `styles.css` for an unclosed block or a rule nested inside a style rule --
Chromium parses everything after an unclosed `{` as CSS Nesting, which
silently disables the rest of the sheet and no screenshot notices.
`examples_test` keeps the gallery scripts honest against the real engine,
not only the mock's scraper.

`bazel test` writes the HTML report and, on failure, the screenshots and traces
to `bazel-testlogs/src/web/e2e_test/test.outputs/` (`outputs.zip`):

```
unzip -o bazel-testlogs/src/web/e2e_test/test.outputs/outputs.zip -d /tmp/e2e
open /tmp/e2e/playwright-report/index.html
```

Several dev servers run for the suite (`e2e/playwright.config.ts`), and the
projects split across them. Each run takes free loopback ports from the
kernel, so concurrent runs from several checkouts never collide; a
`CADPOLE_E2E_*_PORT` variable in the environment pins one server's port.

| Project | Server | What it covers |
| --- | --- | --- |
| `desktop`, `phone` | the built-in sample script | `e2e/cadpole.spec.ts`: the whole UI in mock mode (`?mock=1`), with screenshots; `phone` also runs the parameter controls at 390 × 844 (the Params tab, the peek height, segments, the switch, the stepper, the ft-in readouts of a `units(...)` document) and the Layers tab after Params with its 44 px targets, and the phone's own chrome ("the viewer on a phone": nothing off the side at three sizes, the View sheet, the sheet's three heights, tap against drag, deselect) |
| `resize`, `resize-phone`, `resize-webgpu`, `resize-webgpu-phone` | the built-in sample script | `e2e/resize.spec.ts`: the viewport through a change of size, with a translucent outline on screen. At every paint the picture in the buffer was drawn at the buffer's size (a `ResizeObserver` made after the viewer's looks at each one), and once settled the model is read back from a screenshot with its centre in the middle of the uncovered area: a window dragged, turned on its side, resized during a camera move and several times in one task, hidden and collapsed, moved to another pixel ratio; a phone turned with a sheet open, the lighter picture under a finger and the full one at rest, a sheet opening without a drop in resolution; a lost WebGL context said and restored. The `-webgpu` projects run the same file on WebGPU (Dawn on SwiftShader's Vulkan), where a frame that uses a destroyed texture is dropped whole  The same file sweeps eighty window sizes from 360 × 440 to 1920 × 1100 with a mouse: the viewport is the largest pane and no pane lies over another (a short, wide window is the desktop layout, not the phone-on-its-side one) |
| `token` | started with `--token` | `e2e/token.spec.ts`: the browser's half of the token scheme — the fragment, `sessionStorage`, the `Authorization` header, and a server that refuses without it |
| `real` | `--script e2e/real-engine.star` | `e2e/real-engine.spec.ts`: no `?mock=1`, so the page loads `./cadpole_wasm.js` and the real engine evaluates a script written against the actual Starlark API |
| `engine` | `--script e2e/engine.star` | `e2e/engine.spec.ts`: where evaluation runs — the worker by default, the server when asked, local again when the server fails or stalls; `engine.star`'s `server_fail`/`server_slow_ms` parameters tell the dev server how to answer `Evaluate` |
| `perf` | `--script e2e/perf.star` | `e2e/perf.spec.ts`: the apply-path benchmark, asserting counts and printing times |
| `baselid` | opt-in with `CADPOLE_PERF_BASELID=1` | the same spec against `examples/gridfinity_baselid.star`, about thirteen seconds an evaluation |
| `hot` | `--hot --script e2e/engine.star` | `e2e/hot-reload.spec.ts`: what a rebuilt bundle costs a running page. The only server started with `--hot`, which serves a *writable copy* of `dist` and exposes `POST /api/hot/{styles,engine,glue,worker,app}` — one file of the bundle rewritten the way a `bazel build` would, and the `build_changed` that follows. Two of its describe blocks run *without* `?nosw=1`, so the service worker is registered and controls the page: that is where a hot swap meets the cache |
| `crash` | a real `cadpole serve` | `e2e/crash.spec.ts`: failure reports need the real op registry and event log, which `devserver.mjs` does not have, so this project starts `//src/rust/cadpole-cli:cadpole` with the test-hooks wasm (`e2e_dist`) and provokes a real Rust panic |

Baselines are per-environment (they bake in the renderer and the font
rendering), so regenerate them with `bazel run //src/web:e2e_update`, which
writes into `src/web/e2e/__screenshots__` in the source tree. Headless Chromium
usually has no WebGPU adapter, so three's `WebGPURenderer` falls back to its
WebGL2 backend on SwiftShader; `CADPOLE_E2E_WEBGPU=1` switches to WebGPU on
SwiftShader instead. The tests use a fixed viewport,
`reducedMotion: reduce`, animations disabled and a 3 % pixel tolerance.

The `hot` project also prints what each kind of update costs next to a full
`location.reload()`; `--test_output=all` shows it. To measure a
document whose evaluation is seconds rather than milliseconds, point that one
server somewhere else for a single run:

```
CADPOLE_E2E_HOT_SCRIPT=$PWD/examples/gridfinity_baselid.star \
  bazel run //src/web:e2e_update -- --project=hot --grep "what each kind of update costs"
```

Only the measurement survives that swap: the rest of the suite drives
parameters that only `e2e/engine.star` declares.

Outside Bazel (needs Node >= 20 and a browser of your own):

```
bazel build //src/web:dist
cd src/web && pnpm install && pnpm exec playwright install chromium-headless-shell
pnpm e2e          # compares against e2e/__screenshots__
pnpm e2e:update   # regenerates the baselines
```

The `crash` project needs the `cadpole` binary and the test-hooks wasm, so it
runs only under Bazel.
