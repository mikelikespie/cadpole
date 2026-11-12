# Cadpole: notes for coding assistants

Parametric CAD for bots: a model is a script a coding agent writes,
evaluates, measures, renders and tests, and a person watches and steers.
Two kinds of work on one engine, parts and
assemblies and buildings and plans: Rust core, Starlark scripts,
Bazel-native, protobuf persistence, wasm + native + C FFI. Read
`docs/design.md` (how it works), `docs/architecture.md` (crates and front
ends), `docs/starlark-api.md` (the shared API), `docs/parts.md` (assemblies,
gears, fasteners) and `docs/buildings.md` (massing, sites, sun and shadow,
`opt`, floor plans). Setup on a new machine: `docs/getting-started.md`.

## Build, test, run

```
C=bin/cadpole        # builds the CLI with --config=opt when stale (debug booleans are ~10x slower), then runs it
$C eval examples/bracket.star
$C render examples/planter.star -o /tmp/p.png
$C sweep examples/planter.star --param top_radius=40,55,70 -o /tmp/s.png
$C test examples/planter.star                       # the script's test(...) declarations
$C animate examples/gear_toy.star -o /tmp/toy.gif   # the script's animation(...) declarations
$C render examples/shadow_study.star --shadow-study --sun 38,-121,2026-06-21,12:00,-8 -o /tmp/study.png
$C export examples/drawing_sheet.star --run-exports --out-dir /tmp      # dimensioned SVG/PDF/DXF drawings
$C export examples/house_set.star --run-exports --out-dir /tmp          # a multi-page sheet set
$C export examples/color_plan.star --view top --section z=1200 -o /tmp/plan.pdf   # a vector plan
$C export examples/cabin_site.star --model-unit ft -o /tmp/cabin.glb    # glTF
$C export examples/storey_kit.star --run-exports --out-dir /tmp/kit -p printer=bambu_h2d -p slots=4   # a Bambu project
$C publish examples/cabin_site.star --model-unit ft --out-dir /tmp/cabin   # a read-only static site
$C check examples/story_kit.star --layer kit --max-bridge 20            # printability checks
$C check examples/thumbscrew.star -p layout=print -p thread_form=print --support-free   # does it print without support
$C plate examples/print_plan.star --printer bambu_h2c -o /tmp/plates.3mf   # plate a model for printing: ALWAYS make print files this way
$C inspect examples/impeller.star                   # per body: counts, solid verdict, kernel decisions
$C op Drawing --doc examples/bracket.star --view three-view --format svg
$C test examples/room_layout_opt.star               # `opt`: LP/MILP solved by HiGHS (native only)
$C solve examples/plan_parti_compare.star -p refit=cross_stair   # long solves run as jobs into ~/.cache/cadpole/opt
$C serve examples/bracket.star                      # the web UI on loopback; prints a URL with a token
$C listen --next                                    # wait (silently) for the next Chat message, print it, exit

bin/doctor                                          # what this machine is missing, and the command for each
bin/install                                         # `cadpole` on PATH (a link to bin/cadpole); --copy, --uninstall
bin/test [targets]                                  # the Linux sweep below, or just the targets given
bazel test //bin:bin_test                           # the bin/ scripts, against stub tools
bazel build //src/rust/cadpole-wasm:cadpole_wasm_opt    # wasm engine (what the web app ships)
bazel build //src/web:dist                          # web UI
bazel test //src/web:e2e_test                       # Playwright
bazel run //src/web:devserver -- --port 7460 --hot  # dev server with per-file rebuilds
bazel test //src/tools/listener:all                 # the limited-privilege listener
bazel test //spec/...                               # the Quint models
bazel run //spec:quint_check -- --samples=20000 --steps=60   # a deeper model-check pass
bazel run //spec/replay:listener_replay -- --traces=1000    # a longer replay of a model against the program
bazel run //spec/replay:transport_replay -- --traces=60 --steps=40   # spec/transport.qnt against the real programs
CARGO_BAZEL_REPIN=1 bazel build //src/rust/cadpole-core:cadpole_core   # after editing any Cargo.toml
```

Two traps:

- **Never time anything under `bazel-bin/`.** That symlink follows whichever
  configuration built last, so any `bazel test` repoints it at the debug
  binary. `--config=opt` (also expanded by `--config=release` and
  `--config=fast`) sets `--symlink_prefix=bazel-opt-`, so `bazel-opt-bin/` is
  always the optimized build. Quote timings from there, from
  `bazel run --config=opt`, or from `build_support/bench/cadpole-opt`.
- **`bin/cadpole` and everything in `bin/` print only to stderr.** stdout
  belongs to the CLI (`cadpole mcp` speaks JSON-RPC on it), so Bazel's output
  is redirected there too; keep it that way. Under `bazel run`, the CLI moves
  to `BUILD_WORKING_DIRECTORY` itself, so relative paths work with
  `bazel run //:cadpole -- ...` as well.

The repin build must depend on `@crates`: Bazel evaluates only the module
extensions a build needs, so repinning through a target without third-party
deps exits 0 without touching `Cargo.lock`. Check with `git diff Cargo.lock`.

Do not run `bazel clean`: first builds fetch toolchains and compile ~400
crates. Each git worktree gets its own output base; `.bazelrc` shares a
size-capped disk cache between them, and `build_support/bazel-gc.sh` (dry run
by default, `--delete` to act) removes output bases of finished worktrees.

### The Linux sweep

```
bazel test //build_support/...                                  # the supply-chain gate
bazel test --build_tests_only -- //... -//src/apple/CadpoleiOS:CadpoleiOS -//src/apple/CadpoleMac:CadpoleMac
                                                                 # every test a Linux box can run
bazel run //build_support/ci:skipped_targets                    # and nothing else is skipped
bazel build --config=opt //src/rust/cadpole-cli:cadpole //src/web:dist   # what ships
```

`--config=ci` is for a CI runner (see `build_support/ci/prepare-runner.sh`,
which makes room for the toolchains).
`//build_support/ci:skipped_targets` lists the tests the sweep does not
reach and why, and fails if a test is tagged `manual` or falls under the
sweep's exclusions without being listed. A bare `bazel test //...` fails on
Linux: the two Apple app targets cannot resolve a toolchain without a macOS
execution platform, so the sweep subtracts exactly those two. The rest of
`//src/apple` stays in: CadpoleKitTests and the `*.pb.swift` staleness diffs
run only on Linux.

`--config=ci` includes `--config=hermetic`, which requires `linux-sandbox` and
runs `//build_support:network_is_blocked_test` to prove that `block-network`
tags block the network. On Ubuntu 24.04 that needs
`sudo sysctl kernel.apparmor_restrict_unprivileged_userns=0`; otherwise Bazel
silently falls back to `processwrapper-sandbox` and the tags do nothing.

### Apple apps, experimental (`src/apple`, `docs/apple-apps.md`)

On a Mac with Xcode 27:

```
bazel build //src/apple/...
bazel test  --ios_simulator_device="iPhone 17 Pro" --ios_simulator_version=27.2 \
    //src/apple/CadpoleKitTests:SimulatorTests //src/apple/CadpoleLocalEngine:LocalEngineTests
```

On Linux (both tests are part of the Linux sweep too):

```
bazel test //src/apple/CadpoleKitTests                  # EngineClient, wire format, SSE, coalescing
bazel test //src/apple/CadpoleProto:pb_swift_diff_test  # committed *.pb.swift match the schema
bazel run  //src/apple/CadpoleProto:regenerate          # after editing proto/cadpole/v1/*.proto
```

`--config=remote`, `--config=remotecache` and `--config=apple-remote` need a
remote cache or build farm configured in `.bazelrc.user`
(`docs/getting-started.md`).

## This tree is public

Everything here is published. Write every file, comment, string, test fixture,
example, asset and commit message as if a stranger will read it.

Never put any of these in the tree:

- **Private data.** Credentials or tokens; email addresses, phone numbers and
  street addresses; names of real people other than the copyright line; host
  names, private IP addresses, home-directory or machine paths; anything about
  a real person's home, property, money or devices. Examples use invented
  buildings, invented names and reserved example domains and address ranges.
- **Conversation.** What someone said, asked for, preferred or decided; quoted
  or paraphrased instructions; "as requested"; mentions of a chat, a session,
  an assistant, an agent, a reviewer or a review round.
- **Provenance.** Who found or fixed something, when something was read or
  tried, dated work logs, decision or requirement numbers, "the photo shows",
  "the research says", references to notes or documents that are not in this
  tree. A comment gives the technical reason and nothing else.
- **Other projects.** Names or paths of private repositories and tools, and
  the names this project had before Cadpole.
- **Affiliation.** Any statement or hint of endorsement by, or affiliation
  with, a company. No AI model names or identifiers, in files or in commits.
- **Asset metadata.** Author, user name, path, serial number or location
  fields inside images, models and archives. Strip them before adding a file.

If a fact is only true of one person or one machine, it does not belong here.
When unsure, leave it out.

## Conventions

- **Crate boundaries.** cadpole-core never sees Starlark; cadpole-script never
  sees protobuf requests; cadpole-engine is the only crate that knows the
  envelope; cadpole-ops turns `service CadpoleOps` into every front end;
  cadpole-cli is the only user-facing binary (`cadpole-bench` and
  `cadpole-sketch-solve:bench` are dev tools). New functionality goes in the
  lowest crate that can express it.
- **Adding a user-facing operation.**
  1. An rpc in `proto/cadpole/v1/ops.proto` with its request/response
     messages, `(cadpole.op)` options (`cli_name`, `idempotent`,
     `needs_session`, `image_field`, `alias`) and `(cadpole.field)` options
     (`flag`, `short`, `positional`, `example`, `required`, `hidden`). The
     rpc's leading comment is the MCP tool description and the CLI help; a
     field's comment is its JSON Schema `description`. Write them for someone
     who has never seen Cadpole.
  2. One arm in `handle()` in `src/rust/cadpole-ops/src/lib.rs`. The MCP tool,
     the `POST /api/ops/<Rpc>` route and `cadpole op <Rpc>` are derived from
     the descriptor at run time; do not add a table anywhere.
  3. Classify it in `src/rust/cadpole-ops/src/capability.rs` as granted or
     denied to the limited listener, with the reason
     (`every_operation_is_classified`; docs/security.md).
  4. Teach `spec/capability.qnt` about it: `//spec:names_test` diffs the
     models' operation lists, granted set and spellings against the Rust and
     the proto. The same test pins the server behaviour `eventlog.qnt` and
     `revisions.qnt` describe, so changing it means updating the model too
     (`spec/README.md`). An operation that emits a new kind of event needs
     nothing in `spec/wakeups.qnt` as long as no reader starts a model turn
     for it (the model calls every such kind `Other`); a kind that should
     start a turn is a change to `cadpole listen`, the channel, the listener
     and that model together.
  5. If it is `needs_session` and its request carries no `document`, implement
     it on `HttpHost` in `src/rust/cadpole-cli/src/ops_host.rs`
     (`every_session_operation_is_actually_proxied`).

  Then run `bazel test //src/rust/cadpole-ops:cadpole_ops_test` and update the
  golden `src/rust/cadpole-ops/testdata/render_request.schema.json` if the
  schema moved.
- **Supply chain: `bazel test //build_support/...` is the whole gate**.
  Permissive licences only (no GPL/LGPL/MPL/CDDL) for every crate in
  `Cargo.lock` and package in `pnpm-lock.yaml`; RustSec and OSV advisories
  from vendored snapshots, with accepted matches triaged in
  `build_support/advisories/accepted.tsv`; and a generated, diff-tested
  `THIRD-PARTY-NOTICES.md`. After adding a dependency:

  ```
  build_support/licenses/refresh.py                        # crates.tsv + npm.tsv
  bazel run //build_support/licenses:attribution_update    # THIRD-PARTY-NOTICES.md
  build_support/advisories/refresh.py                      # only when the snapshots are stale
  ```

  Commit those diffs; never edit them by hand. Watch default features: ureq's
  default TLS stack pulls in MPL-2.0 code, and truck-meshalgo's default `vtk`
  feature brings two unpatched CVEs (see `accepted.tsv`).
- **Protos are the persistence format**; JSON views mirror proto field names.
  The Swift types under `src/apple/CadpoleProto/cadpole/v1/` are generated and
  committed (`swift_proto_library` is broken under rules_swift 3.6.1 +
  Bazel 9): run `bazel run //src/apple/CadpoleProto:regenerate` after any
  `.proto` edit. The TypeScript types and the typed Connect client are
  generated on every build and not committed. The web UI, `cadpole op` and
  `cadpole mcp` talk to `cadpole serve` only through unary Connect,
  `POST /api/cadpole.v1.CadpoleOps/<Rpc>` in binary protobuf;
  `src/rust/cadpole-cli/src/connect.rs` is the server side,
  `src/web/src/wire.ts` the only browser module that encodes anything, and
  `src/web/devserver.mjs` answers the same operations for the e2e suite.
- **Names.** Feature ids are an explicit `name=` or `<kind>_<n>`; body ids are
  `feature#n` (`n` counts bodies across the document). Topological names
  (`blk/top`, `extrude_1/side[0.2]`,
  `blk/top|hole/side#0`) are generated by the feature that made the geometry,
  never by tessellation order. `cadpole_core::naming` is the only place the
  grammar lives; `docs/design.md` (Topological naming) is its normative
  statement.
- **Print files are plated, never hand-placed.** To make a 3MF for a
  printer, use `print_plan(...)` in the script or `cadpole plate` / the
  `Plate` operation (docs/starlark-api.md §6.5, docs/plating.md): it orients,
  groups print-in-place sets, packs plates and explains itself. Do not
  export the assembled model, and do not rotate and translate copies onto
  a bed by hand; pin what the planner cannot know with `print_part(...)`.
- **Errors from scripts are diagnostics with spans, never panics.**
- **Authentication is a capability on the token, server side.** `cadpole serve`
  mints a browser token, a session token and a listener token per run, and
  writes the last two into `$XDG_RUNTIME_DIR/cadpole` (0600 in a 0700
  directory); every route requires `Authorization: Bearer`, and no route takes
  `?token=`. It serves two surfaces: the browser's TCP port and an agent Unix
  socket in that directory, where local clients find it with no `--server`.
  The surface decides which tokens are admitted and which routes exist, never
  what a token may do (docs/security.md, "Two surfaces"; spec/transport.qnt).
- Commit messages describe the change.
