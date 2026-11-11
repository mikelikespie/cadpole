# Getting started

How to build, test and run Cadpole on your own machine. Each step is a
command and a way to check it worked, so a person or a coding agent can
follow it from top to bottom.

## 1. Tools

`./bin/install` (next section) checks all of this before it does anything
and prints the command for whatever is missing; `./bin/doctor` prints the
same checklist on its own, with the optional tools too. Nothing is ever
installed for you.

- **Bazelisk**, installed as `bazel`. The repo pins Bazel in `.bazelversion`;
  Bazelisk fetches it. Distro and Homebrew `bazel` packages are usually the
  wrong version, and the checks say so. Follow the official instructions for
  your platform: https://github.com/bazelbuild/bazelisk#installation. Two
  shortcuts that pick the right build for the machine:
  `brew install bazelisk` on macOS, `npm install -g @bazel/bazelisk`
  wherever Node is installed. Check: `bazel --version` in the checkout prints
  the version in `.bazelversion`.
- **Linux: the system C++ headers.** The clang Bazel downloads still compiles
  against the machine's glibc and libstdc++ headers: `sudo apt install g++`
  on Debian or Ubuntu, `sudo dnf install gcc-c++` on Fedora,
  `sudo pacman -S gcc` on Arch.
- **macOS: Xcode 27.x**, selected with
  `sudo xcode-select -s /Applications/Xcode.app`, its licence accepted
  (`sudo xcodebuild -license accept`). Bazel uses its compiler for the CLI as
  well as for the Apple apps.
- **Disk:** about 25 GiB free for the first build (toolchains, ~400 crates
  and the caches).
- **Nothing else for the CLI and the web UI.** Bazel downloads the Rust
  toolchain, the crates, Node and the npm packages, LLVM and protoc; you never
  run `cargo` or `npm`.
- Optional: [ibazel](https://github.com/bazelbuild/bazel-watcher) for
  `bin/dev`, and Claude Code for the MCP server and the listener
  (`docs/assistant-integration.md`).

Linux (x86_64, arm64) and macOS (Apple silicon) are supported. The Linux
test sweep below is what CI runs.

## 2. The CLI

```
./bin/install                                   # checks, builds, links ~/.local/bin/cadpole to bin/cadpole
cadpole eval   examples/bracket.star
cadpole render examples/planter.star -o /tmp/planter.png
cadpole serve  examples/bracket.star            # the web UI; prints a URL with a token
claude mcp add cadpole -- cadpole mcp           # the MCP server, for Claude Code
```

- `bin/cadpole` builds the optimized CLI (`--config=opt`; debug booleans are
  about 10x slower) when the checkout has changed, then runs it in your
  directory with your arguments. When nothing changed this costs one quiet
  Bazel round trip, a fraction of a second; `CADPOLE_NO_BUILD=1` skips it.
  Bazel's output goes to stderr, so `cadpole mcp` keeps stdout to itself.
- `./bin/install --prefix DIR` links into `DIR` instead of `~/.local/bin` and
  says how to put it on `PATH` if it is not; `--copy` installs a standalone
  copy that no longer follows the checkout; `--uninstall` removes either.
- Without installing: `./bin/cadpole ...`, or `bazel run //:cadpole -- ...`
  from anywhere in the checkout. Under `bazel run`, relative paths resolve
  against the directory you ran it from.
- The binary itself is at `bazel-opt-bin/src/rust/cadpole-cli/cadpole`,
  which no test run moves (plain `bazel-bin/` follows whichever configuration
  built last).
- `cadpole serve` listens on loopback (`127.0.0.1:7331`) by default and prints a
  URL whose `#token=` fragment logs the browser in, and reloads the script
  when you save it. `docs/security.md` covers serving to other machines.
- It also listens on a Unix socket in your runtime directory for agents. To
  attach Claude Code, run `claude mcp add cadpole -- cadpole mcp` once in the
  project folder: `cadpole mcp`, `cadpole op` and `cadpole listen` find the
  running server with no `--server` and no port
  (`docs/assistant-integration.md`, "Finding the server").
- Working on Cadpole itself: `bin/dev examples/bracket.star` runs `cadpole
  serve` under ibazel, which rebuilds and restarts it when the Rust or the
  web UI changes. Editing `.star` files does not need it.

`cadpole --help` lists every subcommand (`export`, `sweep`, `animate`,
`publish`, `check`, `inspect`, `solve`, `jobs`, ...), and `CLAUDE.md` has an
example of each. The examples in `examples/` all run.

Cadpole does two kinds of work on the same engine, and each has a guide
that starts with a script to run:

- **Parts and assemblies** (`docs/parts.md`): mechanical parts, mechanisms
  and assemblies, in millimetres. Start with
  `cadpole render examples/bracket.star -o /tmp/bracket.png`.
- **Buildings and plans** (`docs/buildings.md`): massing, sites, floor
  plans, sun and shadow, in feet and inches. Start with
  `cadpole export examples/house_set.star --run-exports --out-dir /tmp`
  (a four-sheet drawing set). A building script declares
  `units(units.ft_in)`; one drawn in plain feet takes `--model-unit ft` on
  `export`, `bake` and `publish`. A shadow study is
  `cadpole render --shadow-study --sun ...`, and a layout optimisation too
  long for one evaluation runs with `cadpole solve`.

The language and kernel both share are in `docs/starlark-api.md`.

### Work with a coding agent (optional)

With your own Claude Code installed and `cadpole` on your `PATH`:

```
cadpole serve examples/bracket.star        # keep this running
claude mcp add cadpole -- cadpole mcp      # once per project folder
claude mcp list                            # check: "cadpole: ... Connected"
claude                                     # then ask: "Wait for messages from Cadpole in the background and act on them."
```

Turn **Listen** on in the web UI and type in the Messages panel.
`--scope project` on `claude mcp add` writes the entry to the folder's
`.mcp.json` for everyone on the project; `claude mcp remove cadpole` takes
it out. `docs/assistant-integration.md` covers the rest, including other MCP
clients and the unattended listener.

## 3. Tests

```
bin/test                                                        # every test a Linux box runs (the sweep below), after the same checks as bin/install
bin/test //src/web/...                                          # just these targets; other arguments go to `bazel test`
bazel test //build_support/...                                  # supply chain: licences, advisories, lockfiles
bazel test --build_tests_only -- //... -//src/apple/CadpoleiOS:CadpoleiOS -//src/apple/CadpoleMac:CadpoleMac   # everything else a Linux box runs
bazel run //build_support/ci:skipped_targets                       # and nothing else is skipped
bazel test //src/web:e2e_test                                    # Playwright against the web UI (Linux)
```

After editing a `Cargo.toml`: `CARGO_BAZEL_REPIN=1 bazel build
//src/rust/cadpole-core:cadpole_core` (Bazel maintains `Cargo.lock`).

## 4. Experimental: the native apps (on a Mac)

`src/apple` holds native app sources. They are experimental and untested
end to end, and nothing above needs them; `docs/apple-apps.md` describes
them and how to build them.

## 5. Your own remote cache or remote execution (optional)

Nothing needs this. `.bazelrc` defines three opt-in configs with no endpoint
in them; put yours in `.bazelrc.user` at the workspace root (gitignored,
imported last):

```
build:remotecache --remote_cache=grpc://buildfarm.example.invalid:8980
build:remotecache --remote_instance_name=my-instance
build:remote --remote_executor=grpc://buildfarm.example.invalid:8980
```

- `--config=remotecache` reads and writes the remote cache; actions still run
  locally.
- `--config=remote` adds remote execution. It expects a Linux worker and,
  for Apple targets, an Apple-silicon macOS worker with Xcode; actions route
  by the `OSFamily` property of the platforms in `build_support/platforms`
  (`linux_remote`, `macos_remote`).
- `--config=apple-remote` builds and tests `//src/apple/...` from Linux on
  the macOS worker. `build_support/platforms:remote_xcodes` must name the
  Xcode and SDK versions that worker really has.

A remote cache that others can write to can make your builds run their code:
keep the endpoint on a network you trust.

## 6. Where to read next

- `docs/design.md`: how Cadpole works.
- `docs/parts.md` and `docs/buildings.md`: the two kinds of work, each from a first example.
- `docs/starlark-api.md`: the scripting API both share.
- `docs/architecture.md`: crates and the engine.
- `CLAUDE.md`: commands and conventions (also read by coding assistants).
