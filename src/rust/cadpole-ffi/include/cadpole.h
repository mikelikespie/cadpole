/* Cadpole engine C ABI. Requests and responses are cadpole.v1.Request /
 * cadpole.v1.Response protobuf messages (see proto/cadpole/v1/engine.proto).
 * Apache-2.0.
 *
 * Memory ownership, as five rules (docs/apple-apps.md 2.2):
 *
 *   1. Request bytes stay the caller's. Rust only reads them for the duration
 *      of the call; never hand it a pointer that outlives the call.
 *   2. Response bytes are Rust's, transferred to the caller exactly once.
 *      From Swift:
 *        Data(bytesNoCopy: out, count: outLen,
 *             deallocator: .custom { p, l in cadpole_free(p, l) })
 *      That is the zero-copy path and it makes a double free structurally
 *      impossible: cadpole_free is never called by hand anywhere in the app.
 *   3. A non-zero return means nothing was produced. *out is untouched; do
 *      not free it. Script errors are never a non-zero return -- they are
 *      Diagnostics inside a successful Response, or Response.error.
 *   4. cadpole_version() is static. Assert at launch that it matches the version
 *      the Swift protos were generated from; a mismatch is fatal, not a
 *      warning.
 *   5. Nothing borrows engine memory. Meshes are copied into the response.
 *
 * PANICS. Unwinding across an extern "C" boundary is undefined behaviour, so
 * the staticlib is built with `panic = "abort"` -- `rustc_flags =
 * ["-Cpanic=abort"]` on //src/rust/cadpole-ffi:cadpole_ffi, which is the only build
 * of this library anyone links. Belt and braces, every entry point below also
 * catches unwinding and returns CADPOLE_ERR_PANIC, which is what makes the rlib
 * form (used by the crate's own tests) safe as well. Either way a panic is
 * never undefined behaviour, and a caught one means the engine's state is
 * suspect: report it and rebuild the document rather than continuing.
 *
 * THREADING. A call can block for the whole interactive deadline. Swift actors
 * run on the cooperative pool (one thread per core), so blocking one freezes
 * the app; call these from a dedicated serial DispatchQueue and bridge with
 * withCheckedThrowingContinuation. Raw pointers must never escape that queue.
 */
#ifndef CADPOLE_H
#define CADPOLE_H

#include <stddef.h>
#include <stdint.h>

#ifdef __cplusplus
extern "C" {
#endif

/* Return codes. */
#define CADPOLE_OK 0
/* A pointer argument was null, or the request could not be read. */
#define CADPOLE_ERR_INVALID (-1)
/* A panic was caught at the boundary; engine state is suspect. */
#define CADPOLE_ERR_PANIC (-2)
/* cadpole_cancel was called for this token. */
#define CADPOLE_ERR_CANCELLED (-3)
/* EvaluateRequest.deadline_ms elapsed. */
#define CADPOLE_ERR_DEADLINE (-4)

/* Process a request. Returns CADPOLE_OK and sets *out / *out_len to a buffer
 * that must be released with cadpole_free. Returns a negative code otherwise,
 * leaving *out untouched. */
int32_t cadpole_call(const uint8_t *request, size_t request_len, uint8_t **out, size_t *out_len);

/* As cadpole_call, but abandonable.
 *
 * `token` is chosen by the caller and names this call until it returns.
 * cadpole_cancel(token) releases the caller with CADPOLE_ERR_CANCELLED; a non-zero
 * EvaluateRequest.deadline_ms releases it with CADPOLE_ERR_DEADLINE. Token 0
 * means "nobody can cancel me" -- the deadline still applies; only token 0
 * with no deadline is exactly cadpole_call. Cancelling a token before its call
 * starts is allowed and makes that call return at once -- which is the race a
 * slider drag actually produces.
 *
 * What this does NOT do yet: it releases the CALLER, not the CPU. The
 * evaluation runs on a worker thread and nothing can interrupt Starlark or the
 * kernel until the engine grows a Deadline they check, so an
 * abandoned evaluation finishes in the background and its result is dropped.
 * Enough to keep the UI responsive and to stop work queueing behind the user's
 * finger; not enough to bound CPU. */
int32_t cadpole_call_cancellable(const uint8_t *request, size_t request_len, uint8_t **out, size_t *out_len, uint64_t token);

/* Abandon the cadpole_call_cancellable running under `token`. Idempotent, and
 * safe for a token that has not started or has already finished. */
int32_t cadpole_cancel(uint64_t token);

/* Free a buffer returned by cadpole_call or cadpole_call_cancellable. */
void cadpole_free(uint8_t *ptr, size_t len);

/* Engine version, e.g. "0.1.0". Static; do not free. */
const char *cadpole_version(void);

#ifdef __cplusplus
}
#endif

#endif /* CADPOLE_H */
