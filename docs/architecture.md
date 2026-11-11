# Cadpole architecture

Cadpole is a *script-first* parametric CAD tool. The Starlark script is the model;
the UI is for looking, selecting, and talking. Assistants edit the
script and read the selection.

This document is the architecture of the software. Modelling buildings with
Cadpole is `docs/buildings.md`.

Cadpole does two kinds of work, parts and assemblies (`docs/parts.md`) and
buildings and plans (`docs/buildings.md`), and nothing in the layout below
is split between them: one kernel, one script crate and one set of front
ends serve both. The building side is the Starlark libraries under
`stdlib/` (`building`, `site`, the `plan` family), the terrain, sun and
shadow code in `cadpole-core`, the sightline queries in `cadpole-script`,
and `cadpole-opt`.

```
proto/cadpole/v1/*.proto     persistence format, engine API and the CadpoleOps service (protobuf)
src/rust/                    the Rust crates (below)
src/web/                     web UI (TypeScript, three.js WebGPU), built with rules_js + esbuild
src/apple/                   Swift apps for iOS and macOS (rules_apple), linking cadpole-ffi
src/tools/listener/          the limited-privilege listener (docs/security.md)
stdlib/                      the `@cadpole/...` Starlark libraries
examples/                    .star examples
assets/                      bundled fonts
spec/                        Quint models of the protocols (spec/README.md)
build_support/               toolchains, platforms, licence and advisory gates, benchmarks
docs/                        starlark-api.md is the shared API reference, parts.md and buildings.md the two kinds of work; design.md the design
```

## Crates

| Crate | What it is |
| --- | --- |
| `cadpole-proto` | Protobuf types generated from `proto/cadpole/v1/*.proto`. |
| `cadpole-core` | The feature tree, sketches, the kernel bindings, tessellation and the CPU renderer (rasterizer and path tracer). Never sees Starlark. |
| `cadpole-field` | Field bodies: signed distance fields and what is built on them. |
| `cadpole-sketch-solve` | The 2D sketch constraint solver. |
| `cadpole-font` | Text as geometry: shaped glyph outlines turned into sketch loops. |
| `cadpole-assets` | Assets: the bytes a document refers to that did not come from a script. |
| `cadpole-import`, `cadpole-import-2d` | 3D import; SVG and DXF read into sketches. |
| `cadpole-drawing`, `cadpole-drawing-out` | The 2D drawing pipeline (docs/drawings.md) and its PDF and DXF writers. |
| `cadpole-export-3mf`, `cadpole-export-step` | The 3MF and STEP writers. |
| `cadpole-print` | Print checks: is this mesh printable, and which way up. |
| `cadpole-opt` | The LP/MILP solver binding (HiGHS) behind the `opt` namespace; native only. |
| `cadpole-script` | The Starlark API (starlark-rust) driving cadpole-core. Never sees protobuf requests. |
| `cadpole-engine` | One `call(request_bytes) -> response_bytes` over the proto envelope, plus the JSON session view. The only crate that knows the envelope. |
| `cadpole-ops` | The operation registry: turns `service CadpoleOps` into every front end. |
| `cadpole-render-wgpu` | The GPU renderer (wgpu/WGSL). |
| `cadpole-cli` | The `cadpole` binary: `eval`, `render`, `animate`, `sweep`, `export`, `check`, `test`, `inspect`, `bake`, `publish`, `doc`, `serve`, `listen`, `selection`, `op`, `mcp`, `solve`, `jobs`. |
| `cadpole-wasm` | wasm-bindgen wrapper for the web UI. |
| `cadpole-ffi` | C ABI static library for iOS and macOS (Swift). |
| `cadpole-bench` | A development tool that times the pipeline phases (build_support/bench). |

## One engine, three transports

`cadpole-engine` exposes `fn call(request_bytes: &[u8]) -> Vec<u8>` over
`cadpole.v1.Request` / `cadpole.v1.Response`. That single function is what the wasm
module, the C FFI and the HTTP server expose. Script errors are *diagnostics*
inside a successful response; only malformed requests produce `Response.error`.

For the web UI, `cadpole-engine` also has a JSON *session view* (`view.rs`) so the
browser does not need a protobuf runtime: the model minus meshes as JSON, and
meshes as typed arrays straight out of wasm memory.

## wasm JS API (`cadpole_wasm`)

```ts
import init, { Engine } from "./cadpole_wasm.js";
await init();
const engine = new Engine();
// scripts: [{path, source}], overrides: {name: number|bool|string|[x,y,z]}
const model = JSON.parse(engine.evaluate(JSON.stringify({ scripts, overrides, tolerance: 0.05 })));
// model: { params, features, bodies: [{id, feature_id, name, bbox, topology, mesh_index}], diagnostics, eval_millis }
const pos = engine.mesh_positions(mesh_index);   // Float32Array (xyz)
const nor = engine.mesh_normals(mesh_index);     // Float32Array
const idx = engine.mesh_indices(mesh_index);     // Uint32Array
const fid = engine.mesh_face_ids(mesh_index);    // Uint32Array, one per triangle -> body.topology.faces[i]
const api = JSON.parse(engine.api_doc());        // { functions: [...], types: [...] }
```

`model` JSON field names match `cadpole.v1.Model` (snake_case) so the two never
drift; the only difference is `Body.mesh` is replaced by `mesh_index`.

The page does not call this directly. The engine runs in
`dist/engine-worker.js`, a dedicated module worker, which hands the model back
as JSON and the four typed arrays back as transferables; `src/web/src/engine.ts`
is the interface the UI talks to. `?engine=main` puts it on the main thread.

## Published pages (`cadpole publish`)

A fourth way to run the web app, beside a `cadpole serve` session, the static
playground and the dev server: a **published page**. `cadpole bake` evaluates a
document and writes a `cadpole.v1.BakedModel` (eval.proto) -- the `Model`
`Evaluate` returns, every mesh inline, plus a name, the tolerance, the
generator and a sha256 of the sources -- zlib-deflated, as a baked model file.
Unless `--include-sources`, nothing the author wrote survives but names: no
diagnostics, spans, descriptions, feature arguments or animations.
`--model-unit ft` scales a script drawn in feet to millimetres once and
records feet and inches as its display units.

`cadpole publish` writes that bake into a directory beside the dist's
`index.html`, `styles.css` and `app.js`, the last two renamed by content
hash, with `<meta name="cadpole-baked" content="./<name>.<hash>.<baked extension>">` in
the page and a `cadpole-publish.json` manifest (what it wrote, so the next
publish there removes exactly that). No wasm, no worker, no service worker.

The page (`src/web/src/baked.ts`) finds the bake by `?baked=URL` or the
meta, inflates it with `DecompressionStream("deflate")` (512 MB cap),
decodes it with the generated `BakedModelSchema` and hands the `Model` to
`wire.modelFrom` -- the remote engine's own conversion. `BakedEngine`
answers every evaluation with it; `PublishedApi` (`mode: "published"`) is a
read-only, in-memory session. Everything else -- viewer, tree, layers,
inspector, selection -- is the ordinary app.

A bake may carry how the page opens (`BakedModel.view`): a camera
preset or eye and target, the grid (on, dim, off), a section, and the
layers Home and Fit frame (`--frame-layer`). `--hide-layer` and
`--show-params` change the baked model itself (a layer's default
visibility; which parameters survive). A published page shows its
parameters as a read-out, never as sliders.

### How a page opens: the publisher's choices and the address

The publisher chooses with flags; whoever shares a link chooses with the
address, which wins choice by choice. `src/web/src/opening.ts` is the one
place the two are merged, and it works the same on a served page.

| What | `cadpole publish` | In the address |
| --- | --- | --- |
| A cut | `--section z=4`, `--section -y=10,z=4` (the model's unit) | `?section=-y=0`, `?section=z=1200,-y=3000` (millimetres) |
| The display mode | `--mode shaded\|shaded_edges\|wireframe\|xray\|faces` | `?mode=xray` (`x-ray`, `shaded-edges` and `edges` are read too) |
| A named view | `--view iso\|top\|front\|...` | `?view=top` (`home`, `iso`, `top`, `bottom`, `front`, `back`, `left`, `right`) |
| A free camera | `--eye x,y,z --target x,y,z` (the model's unit) | `?eye=x,y,z&target=x,y,z` (millimetres; without `target`, the centre of what is framed) |
| A way back to the site | `--home-url ../ --home-label "All models"` | none: a link cannot add one |

So `https://example.org/engine/?section=-y=0&eye=560,-450,380&target=84,9,0`
opens the engine cut along its axis from that camera, and the same address
with `mode=xray` in place of `section=` opens it see-through from the same
place. The section spelling is `cadpole render --section`'s: an axis, a
position, and a leading `-` to keep the other side.

The rules, in the order they matter:

- The address beats the publisher, and an address camera or mode beats what
  the tab remembered from an earlier visit. The publisher's mode only
  applies until the tab has chosen its own.
- A value that does not parse is ignored with a console warning; the page
  opens as if it were absent.
- The page keeps the address current: changing the cut rewrites `section=`,
  changing the mode rewrites `mode=` (dropped when it is the page's own
  default). The camera is written only on request: **View > Copy link to
  this view** puts the cut, the mode and the camera in the address and on
  the clipboard.
- With a cut in force the Section button is drawn as cutting, and the
  Section panel's Clear turns it off. A wide page opens the panel with the
  cut; a phone leaves it shut, since it would cover the lower half of the
  model, and shows a Section button beside Home and Fit for as long as
  there is a cut.
- `--home-url` takes `http(s)` or a relative address, nothing else; the
  page checks again before it makes the link. It shows as the Cadpole mark
  with a back arrow at the left of the toolbar.

## Lighting in the viewer

The viewport is lit the way `cadpole render`'s studio is
(`Lights::studio()` in `cadpole-core/src/render/shading.rs`), so a model
looks like its render: a procedural environment -- a 256 x 128 gradient of
sky, horizon and ground built at start-up, no image file -- that every
material reflects, and a key, a fill and a rim light on a rig that turns
with the camera, so the lit side is always the side being looked at. The
environment is what lights a metal: a metallic surface has no diffuse
colour, and with nothing to reflect it is black. `STUDIO` in
`src/web/src/viewer.ts` holds every number; `e2e/shading.spec.ts` holds the
result to luma bands for a dark metal, a white part, a painted part and a
dark matte one, and to `cadpole render`'s picture of the same bodies.

## Sections in the viewer

`src/web/src/section.ts` is the whole of it outside the viewer, and pure:
`parseSection`/`formatSection` (the spelling `cadpole render --section`
uses, a horizontal and a vertical plane), `sliceMesh` (a mesh against an
axis plane, segments in the plane's 2D coordinates), `joinLoops` (exact
endpoint match) and `capTriangles` (nesting by depth, Earcut). The viewer
puts every model object under one `ClippingGroup` whose planes are the
section's, and each plane's cap and outline under a group clipped by the
other plane; the picker skips what the planes remove and maps a cap hit
to its body. `AppState.section` holds the cut in millimetres; the Section
panel (`ui/section.ts`) edits it in the display unit and the address
carries it as `?section=`. No part of it calls the engine.

## HTTP session API (`cadpole serve`)

`cadpole serve [DOC|main.star] --port 7331` serves the web UI and one
session. Documents persist as protobuf (packed documents) on disk.

Every call the web UI makes to the server is one rpc of
`cadpole.v1.CadpoleOps` in binary protobuf over the **Connect protocol**: `POST /api/cadpole.v1.CadpoleOps/<Rpc>` with
`Content-Type: application/proto`, answered in kind, with errors as
`{"code", "message"}` and the HTTP status Connect assigns to the code. The
TypeScript message types *and* the service descriptor the client is built from
are generated from `proto/cadpole/v1/*.proto` (`//src/web:cadpole_pb`). `cadpole op`
and `cadpole mcp` use the same route.

The base URL is `/api`, which is a client's ordinary `baseUrl` setting and is
what keeps the transport behind the one guard (docs/security.md). Any Connect
client works:

```
buf curl --protocol connect --schema cadpole_v1_descriptor_set.bin \
  -H 'authorization: Bearer <token>' -d '{}' \
  http://127.0.0.1:7331/api/cadpole.v1.CadpoleOps/ApiDoc

curl -X POST -H 'content-type: application/json' -H 'authorization: Bearer <token>' \
  --data '{"feature_id":"blk"}' \
  http://127.0.0.1:7331/api/cadpole.v1.CadpoleOps/ExplainRef
```

`application/grpc-web+proto` is served on the same path for clients that want
that framing. gRPC over HTTP/2 is **not** served: it needs trailers a browser
cannot read, and its clients cannot be given a URL prefix, so it would need a
route outside `/api/`.

`POST /api/ops/<Rpc>` is an alias: same registry, same guard, Cadpole's own
error shape, and the lenient spellings (`eval`, `set_script`) the Connect
route does not accept. `GET /api/ops` lists the visible rpcs with their JSON
Schemas, and is the always-current reference. The hand-written JSON routes
in this table are also served, and are what `CadpoleKit` and `curl` use;
they are a second spelling of the same session. The event stream is shared
by all of them and is SSE with JSON payloads. Every `/api/` route requires
the bearer token (docs/security.md).

| Method | Path | Body / Response |
| --- | --- | --- |
| GET | `/api/session` | `Session` (below) |
| PUT | `/api/session/script` | `{path, source}` → `Session`; bumps `revision`, emits `script_changed` |
| PUT | `/api/session/params` | `{overrides: {name: value}}` → `Session`; emits `params_changed` |
| PUT | `/api/session/selection` | `{selected: [EntityRef], hovered?: EntityRef, note?: string}` → `Session`; emits `selection_changed` |
| POST | `/api/session/messages` | `{from: "user"\|"assistant", text}` → `Message`; emits `message` |
| PUT | `/api/session/assistant`, `/api/session/camera` | the assistant's listening state; the UI's camera |
| POST | `/api/session/save`, `/api/session/save-as` | `{}` → `{path}` writes the packed document protobuf |
| GET | `/api/session/document`, `/api/documents`, `/api/documents/{id}` | the session's document; the documents under the served root |
| GET | `/api/session/messages/{seq}/attachment/{i}` | a message's attachment |
| POST | `/api/cadpole.v1.CadpoleOps/<Rpc>` | Connect unary: `application/proto` or `application/json` (also `application/grpc-web+proto`). What the web UI, `cadpole op` and `cadpole mcp` use |
| POST | `/api/ops/<Rpc>` | The same registry, Cadpole's own shape: proto3 JSON, or `application/x-protobuf` bytes both ways |
| GET | `/api/ops` | The visible rpcs and their JSON Schemas |
| POST | `/api/call` | One `cadpole.v1.Request` → `cadpole.v1.Response`, as bytes |
| GET | `/api/events?after=<seq>` | Server-Sent Events stream of `Event`. With `&live=1`, also `reply_stream` frames: what a reply in progress gained (docs/design.md, "Streamed replies") |
| GET | `/api/events/poll` | The same events, by polling |
| GET | `/api/model` | `cadpole.v1.Model` as protobuf bytes (`application/x-protobuf`), evaluated natively |
| POST | `/api/render` | → PNG bytes |

The main fields of the session and event JSON:

```ts
type Session = {
  id: string; revision: number;
  document: { id, name, display_unit, scripts: [{path, source}], param_overrides: {} };
  selection: { selected: EntityRef[]; hovered?: EntityRef; note: string };
  messages: Message[];                   // chat / notes between user and assistant
  clients: number;                       // connected UIs
};
type EntityRef = { feature_id: string; body_id?: string; kind: "feature"|"body"|"face"|"edge"|"vertex"|"sketch"|"param"; topo_name?: string };
type Message = { seq: number; from: "user"|"assistant"; text: string; at: string /* RFC3339 */ };
type Event = { seq: number; at: string; kind: "script_changed"|"params_changed"|"selection_changed"|"message"|"client_joined"|"client_left"|"assistant_listening"|"asset_changed"|"build_changed"|"client_failure"|"solve_started"|"solve_finished"; data: any; by?: string };
```

**Solve jobs.** An `opt` solve too long for one evaluation comes back
pending, and `cadpole serve` runs it as a job: its own
`cadpole opt-job` process, which runs no Starlark, under rlimits and nice 10,
filing its answer in the solution cache (`~/.cache/cadpole/opt`) that every
native evaluation reads first. `--solve-jobs N` run at once (0, the
default, is `max(1, min(4, cores/4))`), each under `--job-time-limit S`
(3600) and `--job-mem-mb MB` (8192) unless the solve gives its own
`time_limit=`; `--solve-inline-ms` (10,000) is the longest a solve runs
inside an evaluation before it becomes a job, never more than half of what
the evaluation has left; `--no-solve-jobs` leaves such solves pending. The
limited listener's evaluations, renders and sweeps never start a job. When
a job starts the stream carries `solve_started` (`{job, origin,
time_limit_s, owner}`); when it lands, serve evaluates the session's
document again (which reads the answer and starts whatever it unblocks)
and emits `solve_finished` (`{job, state, status, objective, gap,
elapsed_ms, origin, message, pending, revision}`), on which the web UI
re-evaluates. The jobs are the `Solve`, `ListJobs`, `WaitJobs` and
`CancelJobs` rpcs, and on the command line `cadpole solve` and `cadpole jobs`
(docs/buildings.md, "the `opt` namespace").

The web UI evaluates in wasm locally, in a worker, and can also evaluate on
the server; the server evaluates natively. Both run the same engine, so
results agree. The modules, all under `src/web/src`:

| module | what it is |
| --- | --- |
| `jobs.ts` | the job runner: named slots, one job at a time, `supersede` / `coalesce` / `queue`, cancellation, and results as an ordered sequence |
| `engine.ts` | `EngineLike` — `evaluate(req, signal) -> Promise<Evaluated>`, where `Evaluated` carries the model *and* its meshes |
| `worker-engine.ts` + `engine-worker.ts` | the wasm engine in a dedicated module worker (`dist/engine-worker.js`), the default everywhere; terminated to cancel, and respawned after a Rust panic |
| `remote-engine.ts` | one `Evaluate` rpc over Connect (`POST /api/cadpole.v1.CadpoleOps/Evaluate`; `POST /api/ops/Evaluate` is the alias), decoding `cadpole.v1.Model` and its inline `Mesh` through `wire.ts` |
| `engine-policy.ts` + `evaluator.ts` | which engine runs, whether to show a coarse mesh first, and the fallback to local |

`cadpole.v1.Body.mesh` carries packed `positions`, `normals`, `indices` and
`face_ids`, so a remote evaluation comes back with the triangles already
computed.

## Assistant integration

- `cadpole listen` prints events as JSON lines. `--next` waits, printing
  nothing, for the next message from the user, prints it and exits;
  `--follow` does the same without exiting. An assistant runs one of them
  as a background command and is woken by it, so waiting for the user costs
  no model turn.
- `cadpole selection` prints the current selection with resolved geometry
  (face normal/centroid, edge length, feature args), i.e. "what the user is
  pointing at".
- `cadpole mcp` runs an MCP stdio server exposing the same as tools
  (`get_session`, `edit_script`, `render`, `sweep`, ...). See
  docs/assistant-integration.md.
- `//src/tools/listener` answers Chat messages unattended with a restricted
  token, running each turn through the installed Claude Code in headless
  mode.

## Headless rendering

`cadpole render main.star -o out.png --view iso --param w=40` renders headlessly
with the built-in software rasterizer (no GPU needed, runs in CI and in wasm).
`cadpole sweep main.star --param w=30,40,50 --param h=10,20 -o sheet.png` renders a
labelled contact sheet over the cartesian product so an assistant can compare
variants in one image. The options are in docs/views.md.

## SIMD / performance

- wasm: built with `+simd128,+bulk-memory,+multivalue,...`; only latest Chrome is a target.
- native: `--config=fast` adds `-Ctarget-cpu=native`.

## Licensing

Apache-2.0. All dependencies are Apache-2.0 / MIT / BSD (truck, starlark-rust,
prost, three.js, wasm-bindgen). `THIRD-PARTY-NOTICES.md` is the generated
statement of exactly what ships and under what terms.

`bazel test //build_support/...` enforces it: `//build_support/licenses:license_test`
checks every crate in `Cargo.lock` and every package in `pnpm-lock.yaml`
against `build_support/licenses/allowed.txt`,
`//build_support/advisories:advisory_test` checks every version in both
lockfiles against a vendored RustSec/OSV snapshot, and
`//build_support/licenses:attribution_test` checks that the committed
`THIRD-PARTY-NOTICES.md` is what the inventories say. Not covered: the Bazel
module graph, the Swift packages under `src/apple`, and the pinned toolchain
archives.
