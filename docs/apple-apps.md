# Apple apps

> **Experimental.** These apps are sources that build, not a supported
> product: they have not been tested end to end, are not distributed, and
> may lag behind the CLI and the web UI. Use the web UI (`cadpole serve`).

Cadpole has two native SwiftUI apps under `src/apple`, built with Bazel:

| App | Target | Bundle id | What it is |
| --- | --- | --- | --- |
| macOS | `//src/apple/CadpoleMac` | `io.github.mikelikespie.cadpole.mac` | Opens a file or a new document with the engine linked into the app, or connects to a running `cadpole serve` |
| iOS and iPadOS | `//src/apple/CadpoleiOS` | `io.github.mikelikespie.cadpole.ios` | A thin client for a running `cadpole serve`; it links no Rust |

Both require macOS 26 / iOS 26 (`minimum_os_version = "26.0"`).

Each app shows a document as a feature tree, a parameter list, an
inspector and a viewport. The viewport is a rendered image (a `Render`
call) with a set of named views, not a live GPU view. Not implemented in
the apps: script editing, the message panel, export and sweeps.

## Architecture

```
src/apple/
  CadpoleProto/        Swift types generated from proto/cadpole/v1/*.proto (committed)
  CadpoleKit/          EngineClient, RemoteEngine and their support; Foundation only
  CadpoleLocalEngine/  LocalEngine: the Rust engine through the C ABI (macOS only)
  CadpoleUI/           the SwiftUI views and the document model, shared by both apps
  CadpoleMac/          the macOS app
  CadpoleiOS/          the iOS app
  CadpoleKitTests/     tests for CadpoleKit
```

### EngineClient

The UI talks to one protocol, `EngineClient`
(`src/apple/CadpoleKit/EngineClient.swift`), and does not know whether the
engine is in the process or on another machine:

```swift
public protocol EngineClient: Sendable {
    var capabilities: EngineCapabilities { get }
    func session() async throws -> SessionSnapshot
    func document() async throws -> Cadpole_V1_Document
    func call(_ request: Cadpole_V1_Request) async throws -> Cadpole_V1_Response
    func setParams(_ overrides: [String: Cadpole_V1_ParamValue]) async throws -> SessionSnapshot
    func setSelection(_ refs: [Cadpole_V1_EntityRef]) async throws -> SessionSnapshot
    func events(after seq: UInt64) -> AsyncThrowingStream<EngineEvent, any Error>
    var connection: AsyncStream<ConnectionState> { get }
}
```

An extension adds `evaluate(tolerance:skipMesh:deadline:)`, `render(_:)`
and `apiDoc()` on top of `call`.

`EngineCapabilities` says what an implementation can do
(`localEvaluation`, `pathTrace`, `stepExport`, `assistant`, `assets`,
`backgroundJobs`), and the UI hides what is not available.
`ConnectionState` is one of `disconnected`, `resolving`, `connecting`,
`live(revision:)`, `degraded(since:)` and `unauthorized`.

There are two implementations:

- **`RemoteEngine`** (CadpoleKit) talks to `cadpole serve` over HTTP: JSON
  on `GET`/`PUT /api/session*`, protobuf on `POST /api/call`, and
  server-sent events on `GET /api/events`. Every request carries
  `Authorization: Bearer <token>` and an `X-Cadpole-Client` id.
- **`LocalEngine`** (CadpoleLocalEngine, macOS only) calls the Rust engine
  in the same process through the C ABI below.

### CadpoleKit

CadpoleKit imports only Foundation, so it builds and its tests run on
Linux. Besides `EngineClient` and `RemoteEngine` it holds:

| File | Contents |
| --- | --- |
| `HTTPTransport.swift` | the transport protocol `RemoteEngine` uses; `URLSessionTransport` is the Apple-only implementation |
| `SSEParser.swift` | a server-sent-events parser |
| `Reconnection.swift` | `ReconnectPolicy` (retry after 1 s, doubling to 10 s) and `ConnectionMonitor` |
| `ParamCoalescer.swift` | an actor that collapses a burst of parameter changes, such as a slider drag, into the latest value |
| `ServerURL.swift` | `ServerAddress`, which parses the URL `cadpole serve` prints (`http://host:7331/#token=<t>`) |
| `TokenStore.swift` | `KeychainTokenStore`: the token is kept in the Keychain under service `io.github.mikelikespie.cadpole.token`, one item per host, accessible after first unlock on this device only |
| `DisplayUnits.swift` | formatting lengths for display |
| `JSONValue.swift` | a JSON value type for the session API |
| `CadpoleFileTypes.swift` | generated from `build_support/file_types.bzl`: the document file extensions and type identifier |

To connect, paste the URL the server printed into the app's connect
screen. The app stores the token in the Keychain and remembers the server
only after one session read succeeds with it, so a mistyped token is never
kept. A `401` means the server restarted and minted a new token: the
connect screen says so, and a `401` on the event stream later drops the app
back to the connect screen (`ServerLogin`). The server speaks plain HTTP, so
both apps carry an App Transport Security exception for `ts.net` names and
the local network.

Parameters with both `min` and `max` get a slider; others get a text field
and a stepper, as in the web UI (`ParameterControl`).

Both apps open a loose `.star` script (type `net.starlark.script`). It is
packed the way `cadpole eval` packs it (`LooseScript`): the file becomes
`main.star`, and the files it `load()`s come along. A loaded file that
exists but cannot be read, such as a sibling outside the sandbox's grant on
the Mac, is an error that says so. The `./assets` directory is not packed
yet.

### CadpoleProto

The Swift protobuf types in `src/apple/CadpoleProto/cadpole/v1/*.pb.swift`
are generated and committed, because `swift_proto_library` does not work
with rules_swift 3.6.1 on Bazel 9. After editing any `.proto`:

```
bazel run  //src/apple/CadpoleProto:regenerate          # rewrite the committed files
bazel test //src/apple/CadpoleProto:pb_swift_diff_test  # fails when they are stale
```

Both targets run on Linux only.

### The C ABI

`LocalEngine` links `//src/rust/cadpole-ffi:cadpole_ffi_cc`, a static
library, and imports it as the Swift module `CadpoleFFI` (declared by
`src/rust/cadpole-ffi/include/module.modulemap`). The header is
`src/rust/cadpole-ffi/include/cadpole.h`:

```c
#define CADPOLE_OK 0
#define CADPOLE_ERR_INVALID (-1)    /* null pointer, or an unreadable request */
#define CADPOLE_ERR_PANIC (-2)      /* a panic was caught; engine state is suspect */
#define CADPOLE_ERR_CANCELLED (-3)  /* cadpole_cancel was called for this token */
#define CADPOLE_ERR_DEADLINE (-4)   /* EvaluateRequest.deadline_ms elapsed */

int32_t cadpole_call(const uint8_t *request, size_t request_len, uint8_t **out, size_t *out_len);
int32_t cadpole_call_cancellable(const uint8_t *request, size_t request_len, uint8_t **out, size_t *out_len, uint64_t token);
int32_t cadpole_cancel(uint64_t token);
void cadpole_free(uint8_t *ptr, size_t len);
const char *cadpole_version(void);
```

Requests and responses are `cadpole.v1.Request` and `cadpole.v1.Response`
protobuf messages (`proto/cadpole/v1/engine.proto`), the same envelope the
wasm engine and `POST /api/call` use.

Memory rules:

1. Request bytes stay the caller's. Rust reads them only during the call.
2. Response bytes are transferred to the caller once and released with
   `cadpole_free`. From Swift, wrap them in
   `Data(bytesNoCopy:count:deallocator:)` with a deallocator that calls
   `cadpole_free`.
3. A non-zero return means nothing was produced and `*out` is untouched. A
   script error is not a non-zero return; it is a diagnostic inside a
   successful response.
4. `cadpole_version()` returns a static string. `LocalEngine` compares it
   with the version it expects at launch and fails with
   `LaunchError.versionMismatch` when they differ.
5. Nothing borrows engine memory. Meshes are copied into the response.

Other behaviour to know:

- **Panics.** The static library is built with `-Cpanic=abort`, and every
  entry point also catches unwinding and returns `CADPOLE_ERR_PANIC`.
- **Threading.** A call blocks until the evaluation finishes. `LocalEngine`
  makes every call from one serial `DispatchQueue` so it never blocks
  Swift's cooperative thread pool.
- **Cancellation.** `cadpole_cancel(token)` and a deadline release the
  caller, not the CPU: the abandoned evaluation finishes on a worker thread
  and its result is dropped.

### CadpoleUI

| File | Contents |
| --- | --- |
| `DocumentModel.swift` | the observable model behind a document window |
| `DocumentScreen.swift` | the document layout: a split view with an inspector |
| `FeatureTree.swift`, `ParameterList.swift`, `InspectorView.swift` | the panels |
| `ViewportView.swift` | the rendered image and the named-view controls |
| `ConnectView.swift` | paste a server URL; recent servers |
| `DocumentFile.swift` | reads a packed document or a `.star` script from disk |

The macOS app runs in the App Sandbox with the network-client and
user-selected-file entitlements (`CadpoleMac/CadpoleMac.entitlements`).

## Build and test

On a Mac with Xcode 27:

```
bazel build //src/apple/...
bazel test  --ios_simulator_device="iPhone 17 Pro" --ios_simulator_version=27.2 \
    //src/apple/CadpoleKitTests:SimulatorTests //src/apple/CadpoleLocalEngine:LocalEngineTests
```

`SimulatorTests` and `LocalEngineTests` are tagged `manual`, so a wildcard
does not pick them up; name them. `--config=ios-sim` and `--config=ios`
select the Simulator and device architectures. Pick a simulator your Xcode
has (`xcrun simctl list devices available`).

To run on your own device or sign the Mac app, use your own team: change
`bundle_id` in `src/apple/CadpoleiOS/BUILD.bazel` and
`src/apple/CadpoleMac/BUILD.bazel` to IDs you own, and add a
`provisioning_profile` for the iOS app (the comment there says where). Do not
commit those changes.

On Linux:

```
bazel test //src/apple/CadpoleKitTests                  # EngineClient, wire format, SSE, reconnection, coalescing
bazel test //src/apple/CadpoleProto:pb_swift_diff_test  # the committed *.pb.swift match the schema
bazel test //src/rust/cadpole-ffi:cadpole_ffi_test      # the C ABI
```

The apps, CadpoleUI and CadpoleLocalEngine need a macOS execution platform
and do not build on Linux. A bare `bazel test //...` therefore fails on
Linux; exclude them with `-//src/apple/...` and name the Linux targets
above.

## Toolchain pins

Set in `MODULE.bazel` and `.bazelrc`:

| Pin | Why |
| --- | --- |
| `rules_swift` 3.6.1, `rules_apple` 4.5.3, `apple_support` 2.8.1 | rules_swift 4.x makes SwiftProtobuf a repository a downstream module cannot use, and swift-protobuf is not in the Bazel Central Registry. SwiftProtobuf comes from rules_swift's own `@com_github_apple_swift_protobuf` |
| `//build_support/patches:rules_swift_toolchain_env.patch` | lets rules_swift's Linux tools run under a strict action environment |
| Apple CC toolchains registered before the LLVM toolchain | otherwise Swift test targets fail on a Mac with "Compiling objc_library targets requires the Apple CC toolchain" |
| Swift 6.4.2 for Linux (`//build_support/swift:toolchain.bzl`) | a pinned Swift toolchain download, so Linux builds do not depend on an installed Swift |
| `--macos_minimum_os=26.0`, `--host_macos_minimum_os=26.0` | the apps' deployment target |
| Rust targets `aarch64-apple-darwin`, `aarch64-apple-ios`, `aarch64-apple-ios-sim` | the static library for the Apple platforms |

The version shown in each app comes from `//src/apple:CadpoleVersion`.
