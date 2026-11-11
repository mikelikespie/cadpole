//! C ABI for embedding the engine (Swift on iOS/macOS). One function carries
//! the protobuf envelope; memory returned by `cadpole_call` is freed with
//! `cadpole_free`. See `include/cadpole.h`.
//!
//! Three properties the Apple apps rely on (`docs/apple-apps.md`):
//!
//! * **Nothing unwinds across `extern "C"`.** Unwinding over the ABI boundary
//!   is undefined behaviour. `kernel::guarded` converts kernel panics but
//!   nothing covers the rest, so every entry point here is wrapped in
//!   `catch_unwind` and a panic becomes [`CADPOLE_ERR_PANIC`] instead of UB.
//!   The staticlib is *additionally* meant to be built with `panic = "abort"`;
//!   see the note in `include/cadpole.h`.
//! * **A deadline.** `EvaluateRequest.deadline_ms` bounds how long a caller
//!   waits (`docs/security.md`'s evaluation `Deadline`).
//! * **Cancellation.** [`cadpole_call_cancellable`] and [`cadpole_cancel`]: a slider
//!   drag supersedes the evaluation behind it, so `LocalEngine` cancels rather
//!   than queueing.
//!
//! What cancellation and the deadline do *not* do: they
//! release the **caller**, not the CPU. The evaluation runs on a worker thread
//! and there is no way to interrupt Starlark or the kernel. So an abandoned evaluation finishes in the background
//! and its result is dropped. That is enough to keep the UI responsive and to
//! stop the queue growing behind the user's finger, and it is not enough to
//! bound CPU. `include/cadpole.h` says the same thing to the Swift side.

use std::collections::HashMap;
use std::os::raw::c_char;
use std::panic::{AssertUnwindSafe, catch_unwind};
use std::sync::{Arc, Condvar, LazyLock, Mutex};
use std::time::{Duration, Instant};

use cadpole_engine::cadpole_proto::prost::Message;
use cadpole_engine::cadpole_proto::v1 as pb;

/// Success.
pub const CADPOLE_OK: i32 = 0;
/// A pointer argument was null, or the request could not be read.
pub const CADPOLE_ERR_INVALID: i32 = -1;
/// A panic was caught at the boundary. The engine's state should be
/// considered suspect; the app is expected to report and restart the document.
pub const CADPOLE_ERR_PANIC: i32 = -2;
/// `cadpole_cancel` was called for this token.
pub const CADPOLE_ERR_CANCELLED: i32 = -3;
/// `EvaluateRequest.deadline_ms` elapsed.
pub const CADPOLE_ERR_DEADLINE: i32 = -4;

/// Process a `cadpole.v1.Request` and write a `cadpole.v1.Response`.
///
/// # Safety
/// `request` must point to `request_len` readable bytes; `out`/`out_len` must
/// be valid pointers. The buffer written to `*out` must be released with
/// [`cadpole_free`].
#[unsafe(no_mangle)]
pub unsafe extern "C" fn cadpole_call(request: *const u8, request_len: usize, out: *mut *mut u8, out_len: *mut usize) -> i32 {
    let Some(req) = (unsafe { checked_request(request, request_len, out, out_len) }) else {
        return CADPOLE_ERR_INVALID;
    };
    match catch_unwind(AssertUnwindSafe(|| cadpole_engine::call(req))) {
        Ok(resp) => {
            unsafe { hand_off(resp, out, out_len) };
            CADPOLE_OK
        }
        Err(_) => CADPOLE_ERR_PANIC,
    }
}

/// Like [`cadpole_call`], but the caller can give up on it.
///
/// `token` is chosen by the caller and identifies this call until it returns;
/// [`cadpole_cancel`] with the same token releases the caller with
/// [`CADPOLE_ERR_CANCELLED`]. A non-zero `EvaluateRequest.deadline_ms` releases it
/// with [`CADPOLE_ERR_DEADLINE`]. In both cases `*out` is left untouched and must
/// not be freed.
///
/// Token 0 means "nobody can cancel me". The deadline still applies; only
/// with token 0 *and* no deadline is this exactly [`cadpole_call`].
/// Cancelling a token *before* the call starts is allowed and makes the call
/// return immediately; that is the race a slider drag actually produces.
///
/// # Safety
/// As [`cadpole_call`].
#[unsafe(no_mangle)]
pub unsafe extern "C" fn cadpole_call_cancellable(
    request: *const u8,
    request_len: usize,
    out: *mut *mut u8,
    out_len: *mut usize,
    token: u64,
) -> i32 {
    let Some(req) = (unsafe { checked_request(request, request_len, out, out_len) }) else {
        return CADPOLE_ERR_INVALID;
    };
    let deadline = deadline_of(req).map(|d| Instant::now() + d);
    // Nothing to wait for and nobody to wait for it: run it on this thread.
    if token == 0 && deadline.is_none() {
        return unsafe { cadpole_call(request, request_len, out, out_len) };
    }

    // Token 0 with a deadline is still bounded; it just cannot be cancelled,
    // so its state is private and never enters the registry.
    let state = if token == 0 { Arc::new(CallState::default()) } else { register(token) };
    // A cancel that arrived first has already decided this call: nothing
    // has started, so nothing is started -- no worker, no copy of the
    // request. Only work already running is beyond reach (module docs).
    let cancelled_first = state.slot.lock().unwrap_or_else(|e| e.into_inner()).cancelled;
    if cancelled_first {
        unregister(token);
        return CADPOLE_ERR_CANCELLED;
    }
    let owned: Vec<u8> = req.to_vec();
    let worker_state = state.clone();
    let work = move || {
        let result = catch_unwind(AssertUnwindSafe(|| cadpole_engine::call(&owned)));
        let mut slot = worker_state.slot.lock().unwrap_or_else(|e| e.into_inner());
        slot.result = Some(result.map_err(|_| ()));
        worker_state.ready.notify_all();
    };
    // `std::thread::spawn` panics when the OS refuses a thread, and a panic
    // here -- outside any `catch_unwind`, in an `extern "C"` function --
    // aborts the host app. A refused thread is a resource condition, not a
    // bug: run the call on the caller's thread instead, as the engine's own
    // evaluation stack does (`on_eval_stack`). It then cannot be cancelled
    // or released at the deadline from here, but the evaluation's own
    // deadline still bounds it.
    let status = match spawn_worker(token, work) {
        Ok(()) => state.wait(deadline),
        Err(_) => {
            let result = catch_unwind(AssertUnwindSafe(|| cadpole_engine::call(req)));
            Wait::Ready(result.map_err(|_| ()))
        }
    };
    if token != 0 {
        unregister(token);
    }

    match status {
        Wait::Ready(Ok(resp)) => {
            unsafe { hand_off(resp, out, out_len) };
            CADPOLE_OK
        }
        Wait::Ready(Err(())) => CADPOLE_ERR_PANIC,
        Wait::Cancelled => CADPOLE_ERR_CANCELLED,
        Wait::Deadline => CADPOLE_ERR_DEADLINE,
    }
}

/// Abandon the [`cadpole_call_cancellable`] running under `token`.
///
/// Idempotent, and safe to call for a token that has not started or has already
/// finished. Returns [`CADPOLE_OK`] always; there is nothing a caller could do
/// differently on a miss.
///
/// A cancel that finds no call is remembered for `UNCLAIMED_GRACE`, so a call
/// that crosses it on the wire still loses. Past that it is forgotten, which
/// is what keeps the registry bounded: tokens are never reused, so an
/// unclaimed record has no other way out.
#[unsafe(no_mangle)]
pub extern "C" fn cadpole_cancel(token: u64) -> i32 {
    if token == 0 {
        return CADPOLE_OK;
    }
    let state = {
        let mut registry = registry().lock().unwrap_or_else(|e| e.into_inner());
        // A cancel that finds no call is recorded so that a call arriving a
        // moment later still loses -- but nothing else ever removes such an
        // entry, because `unregister` only runs at the end of a call under
        // that same token and tokens are never reused (they come from a
        // counter: `LocalEngine.newCancellationToken`). So the entry has to
        // be swept, or every late cancel leaks one for the life of the
        // process -- and a late cancel is the *ordinary* case, since a slider
        // drag cancels the request it superseded and by then it has usually
        // already finished.
        sweep_unclaimed(&mut registry);
        let entry = registry.entry(token).or_insert_with(Entry::unclaimed);
        entry.state.clone()
    };
    let mut slot = state.slot.lock().unwrap_or_else(|e| e.into_inner());
    slot.cancelled = true;
    state.ready.notify_all();
    CADPOLE_OK
}

/// Free a buffer returned by `cadpole_call` or `cadpole_call_cancellable`.
///
/// # Safety
/// `ptr`/`len` must come from a single successful call and not be freed twice.
#[unsafe(no_mangle)]
pub unsafe extern "C" fn cadpole_free(ptr: *mut u8, len: usize) {
    if ptr.is_null() {
        return;
    }
    unsafe {
        drop(Box::from_raw(std::ptr::slice_from_raw_parts_mut(ptr, len)));
    }
}

/// Engine version as a static NUL-terminated string.
#[unsafe(no_mangle)]
pub extern "C" fn cadpole_version() -> *const c_char {
    static VERSION: &str = concat!(env!("CARGO_PKG_VERSION"), "\0");
    VERSION.as_ptr() as *const c_char
}

// ---------------------------------------------------------------------------
// Internals
// ---------------------------------------------------------------------------

/// Validate the caller's pointers and borrow the request bytes. The bytes stay
/// the caller's for the duration of the call and are never retained.
unsafe fn checked_request<'a>(request: *const u8, request_len: usize, out: *mut *mut u8, out_len: *mut usize) -> Option<&'a [u8]> {
    if (request.is_null() && request_len > 0) || out.is_null() || out_len.is_null() {
        return None;
    }
    Some(if request_len == 0 { &[][..] } else { unsafe { std::slice::from_raw_parts(request, request_len) } })
}

/// Transfer ownership of the response bytes to the caller, exactly once.
unsafe fn hand_off(resp: Vec<u8>, out: *mut *mut u8, out_len: *mut usize) {
    let boxed = resp.into_boxed_slice();
    let len = boxed.len();
    let ptr = Box::into_raw(boxed) as *mut u8;
    unsafe {
        *out = ptr;
        *out_len = len;
    }
}

/// The wall-clock budget this request asked for, if any. Only `Evaluate`
/// carries one today; `engine.proto` is where a second one would go.
fn deadline_of(request: &[u8]) -> Option<Duration> {
    let req = pb::Request::decode(request).ok()?;
    match req.request? {
        pb::request::Request::Evaluate(e) if e.deadline_ms > 0 => Some(Duration::from_millis(e.deadline_ms)),
        _ => None,
    }
}

/// Start the worker thread for a call under `token`.
fn spawn_worker(token: u64, work: impl FnOnce() + Send + 'static) -> std::io::Result<()> {
    let _ = token;
    #[cfg(test)]
    {
        if REFUSE_SPAWN.with(|r| r.get()) {
            return Err(std::io::Error::new(std::io::ErrorKind::WouldBlock, "the test refused the thread"));
        }
        SPAWNED.lock().unwrap_or_else(|e| e.into_inner()).push(token);
    }
    std::thread::Builder::new().name("cadpole-ffi-call".into()).spawn(work).map(|_| ())
}

#[cfg(test)]
thread_local! {
    /// Make this thread's next spawns fail, as an OS out of threads would.
    static REFUSE_SPAWN: std::cell::Cell<bool> = const { std::cell::Cell::new(false) };
}

/// The tokens of the calls that started a worker.
#[cfg(test)]
static SPAWNED: Mutex<Vec<u64>> = Mutex::new(Vec::new());

#[cfg(test)]
fn spawned_for(token: u64) -> bool {
    SPAWNED.lock().unwrap_or_else(|e| e.into_inner()).contains(&token)
}

#[derive(Default)]
struct Slot {
    /// `Err(())` means the worker panicked.
    result: Option<Result<Vec<u8>, ()>>,
    cancelled: bool,
}

#[derive(Default)]
struct CallState {
    slot: Mutex<Slot>,
    ready: Condvar,
}

enum Wait {
    Ready(Result<Vec<u8>, ()>),
    Cancelled,
    Deadline,
}

impl CallState {
    fn wait(&self, deadline: Option<Instant>) -> Wait {
        let mut slot = self.slot.lock().unwrap_or_else(|e| e.into_inner());
        loop {
            if let Some(result) = slot.result.take() {
                return Wait::Ready(result);
            }
            if slot.cancelled {
                return Wait::Cancelled;
            }
            match deadline {
                None => {
                    slot = self.ready.wait(slot).unwrap_or_else(|e| e.into_inner());
                }
                Some(deadline) => {
                    let Some(remaining) = deadline.checked_duration_since(Instant::now()) else {
                        return Wait::Deadline;
                    };
                    if remaining.is_zero() {
                        return Wait::Deadline;
                    }
                    let (next, timeout) = self.ready.wait_timeout(slot, remaining).unwrap_or_else(|e| e.into_inner());
                    slot = next;
                    if timeout.timed_out() && slot.result.is_none() && !slot.cancelled {
                        return Wait::Deadline;
                    }
                }
            }
        }
    }
}

/// One token's state, plus what the registry needs to know to let go of it.
struct Entry {
    state: Arc<CallState>,
    /// True once a call is running under this token. An unclaimed entry was
    /// made by `cadpole_cancel` alone and only `sweep_unclaimed` will free it.
    claimed: bool,
    /// When an unclaimed entry was made; meaningless once claimed.
    since: Instant,
}

impl Entry {
    fn unclaimed() -> Self {
        Self { state: Arc::new(CallState::default()), claimed: false, since: Instant::now() }
    }
}

/// How long a cancel waits for the call it was meant for. The race this
/// exists to win -- cancel and call crossing on the wire -- is settled in
/// milliseconds; a minute is generous past the point of argument, and it is
/// what bounds the registry by the recent cancel rate rather than by the
/// uptime of the process.
const UNCLAIMED_GRACE: Duration = Duration::from_secs(60);

fn sweep_unclaimed(registry: &mut HashMap<u64, Entry>) {
    let now = Instant::now();
    registry.retain(|_, e| e.claimed || now.duration_since(e.since) < UNCLAIMED_GRACE);
}

fn registry() -> &'static Mutex<HashMap<u64, Entry>> {
    static REGISTRY: LazyLock<Mutex<HashMap<u64, Entry>>> = LazyLock::new(Default::default);
    &REGISTRY
}

/// Claim `token`. A cancel that arrived first is preserved, so cancelling
/// before the call starts still wins.
fn register(token: u64) -> Arc<CallState> {
    let mut registry = registry().lock().unwrap_or_else(|e| e.into_inner());
    let entry = registry.entry(token).or_insert_with(Entry::unclaimed);
    entry.claimed = true;
    entry.state.clone()
}

fn unregister(token: u64) {
    registry().lock().unwrap_or_else(|e| e.into_inner()).remove(&token);
}

#[cfg(test)]
mod tests {
    use super::*;

    fn api_doc_request() -> Vec<u8> {
        pb::Request { request: Some(pb::request::Request::ApiDoc(pb::ApiDocRequest {})) }.encode_to_vec()
    }

    /// An evaluation slow enough that a deadline or a cancel can beat it: a
    /// Starlark loop, not a sleep, so it exercises the real path.
    /// A script that finishes quickly, for checking that a generous deadline
    /// stays out of the way.
    fn quick_evaluate_request(deadline_ms: u64) -> Vec<u8> {
        let source = "b = box(10, 10, 10)\n".to_owned();
        let document = cadpole_engine::document_from_scripts("quick", vec![("main.star".to_owned(), source)]);
        pb::Request {
            request: Some(pb::request::Request::Evaluate(pb::EvaluateRequest { document: Some(document), deadline_ms, ..Default::default() })),
        }
        .encode_to_vec()
    }

    fn slow_evaluate_request(deadline_ms: u64) -> Vec<u8> {
        // Starlark has no top-level `for`, so the work lives in a function.
        // The loop must be far longer than any deadline under test could
        // allow: an interpreter that got faster must not turn "the deadline
        // fired" into "the script finished", which would read as
        // a flake rather than as a regression.
        let source = "
def slow():
    n = 0
    for i in range(2000000000):
        n = n + i
    return n

total = slow()
"
        .to_owned();
        let document = cadpole_engine::document_from_scripts("slow", vec![("main.star".to_owned(), source)]);
        pb::Request {
            request: Some(pb::request::Request::Evaluate(pb::EvaluateRequest { document: Some(document), deadline_ms, ..Default::default() })),
        }
        .encode_to_vec()
    }

    fn call_cancellable(req: &[u8], token: u64) -> (i32, Vec<u8>) {
        let mut out: *mut u8 = std::ptr::null_mut();
        let mut out_len = 0usize;
        let rc = unsafe { cadpole_call_cancellable(req.as_ptr(), req.len(), &mut out, &mut out_len, token) };
        if rc != CADPOLE_OK {
            return (rc, Vec::new());
        }
        let bytes = unsafe { std::slice::from_raw_parts(out, out_len) }.to_vec();
        unsafe { cadpole_free(out, out_len) };
        (rc, bytes)
    }

    #[test]
    fn call_roundtrip_through_c_abi() {
        let req = api_doc_request();
        let mut out: *mut u8 = std::ptr::null_mut();
        let mut out_len = 0usize;
        let rc = unsafe { cadpole_call(req.as_ptr(), req.len(), &mut out, &mut out_len) };
        assert_eq!(rc, CADPOLE_OK);
        let bytes = unsafe { std::slice::from_raw_parts(out, out_len) }.to_vec();
        unsafe { cadpole_free(out, out_len) };
        let resp = pb::Response::decode(bytes.as_slice()).unwrap();
        assert!(matches!(resp.response, Some(pb::response::Response::ApiDoc(_))));
        let v = unsafe { std::ffi::CStr::from_ptr(cadpole_version()) };
        assert_eq!(v.to_str().unwrap(), env!("CARGO_PKG_VERSION"));
    }

    /// One release names itself one way through every front end: the FFI's
    /// `cadpole_version()`, the engine's `VERSION` (the CLI's `--version`,
    /// MCP's serverInfo, glTF's generator, published sites, the wasm
    /// `Engine.version()`) and both names in a STEP header. rules_rust
    /// stamps "0.0.0" on any crate whose rule gives no `version`.
    #[test]
    fn every_front_end_reports_the_one_version() {
        let v = env!("CARGO_PKG_VERSION");
        assert_ne!(v, "0.0.0");
        assert_eq!(cadpole_engine::VERSION, v, "the engine's version");
        let document = cadpole_engine::document_from_scripts("v", vec![("main.star".to_owned(), "b = box(1, 1, 1)\n".to_owned())]);
        let req = pb::Request {
            request: Some(pb::request::Request::Export(pb::ExportRequest {
                document: Some(document),
                format: pb::ExportFormat::Step as i32,
                ..Default::default()
            })),
        }
        .encode_to_vec();
        let (rc, bytes) = call_cancellable(&req, 0);
        assert_eq!(rc, CADPOLE_OK);
        let Some(pb::response::Response::Export(e)) = pb::Response::decode(bytes.as_slice()).unwrap().response else {
            panic!("not an export response");
        };
        let step = String::from_utf8_lossy(&e.data);
        let header = step.lines().find(|l| l.starts_with("FILE_NAME")).expect("a FILE_NAME line");
        assert_eq!(header.matches(&format!("'Cadpole {v}'")).count(), 2, "{header}");
    }

    #[test]
    fn bad_pointers_are_rejected_without_touching_out() {
        let req = api_doc_request();
        let mut out_len = 0usize;
        let rc = unsafe { cadpole_call(req.as_ptr(), req.len(), std::ptr::null_mut(), &mut out_len) };
        assert_eq!(rc, CADPOLE_ERR_INVALID);
        assert_eq!(out_len, 0, "a rejected call must not write through out_len");
    }

    #[test]
    fn cancellable_with_token_zero_and_no_deadline_is_a_plain_call() {
        let (rc, bytes) = call_cancellable(&api_doc_request(), 0);
        assert_eq!(rc, CADPOLE_OK);
        let resp = pb::Response::decode(bytes.as_slice()).unwrap();
        assert!(matches!(resp.response, Some(pb::response::Response::ApiDoc(_))));
    }

    #[test]
    fn token_zero_still_honours_the_deadline() {
        // "Not cancellable" must not quietly mean "not bounded": the protocol
        // extension in Swift calls this for a plain evaluate.
        let (rc, _) = call_cancellable(&slow_evaluate_request(30), 0);
        assert_eq!(rc, CADPOLE_ERR_DEADLINE);
    }

    #[test]
    fn cancellable_returns_the_response_when_nothing_interrupts_it() {
        let (rc, bytes) = call_cancellable(&api_doc_request(), 101);
        assert_eq!(rc, CADPOLE_OK);
        assert!(!bytes.is_empty());
        assert!(!registry().lock().unwrap().contains_key(&101), "the token must be released when the call returns");
    }

    /// A call cancelled before it starts never starts its evaluation: no
    /// worker, no copy of the request. It used to spawn one anyway, so each
    /// cancel of a slider drag left a whole evaluation running.
    #[test]
    fn a_call_cancelled_before_it_starts_runs_nothing() {
        let token = 204;
        assert_eq!(cadpole_cancel(token), CADPOLE_OK);
        let (rc, _) = call_cancellable(&slow_evaluate_request(0), token);
        assert_eq!(rc, CADPOLE_ERR_CANCELLED);
        assert!(!spawned_for(token), "a pre-cancelled call started a worker");
        // Uncancelled, the same path does start one.
        let (rc, _) = call_cancellable(&api_doc_request(), 205);
        assert_eq!(rc, CADPOLE_OK);
        assert!(spawned_for(205));
    }

    /// When the OS refuses a thread the call still answers, on the caller's
    /// thread, rather than panicking across the C boundary (which, in the
    /// `panic = "abort"` staticlib, kills the app).
    #[test]
    fn a_refused_thread_runs_the_call_here() {
        REFUSE_SPAWN.with(|r| r.set(true));
        let (rc, bytes) = call_cancellable(&api_doc_request(), 206);
        REFUSE_SPAWN.with(|r| r.set(false));
        assert_eq!(rc, CADPOLE_OK);
        let resp = pb::Response::decode(bytes.as_slice()).unwrap();
        assert!(matches!(resp.response, Some(pb::response::Response::ApiDoc(_))));
        assert!(!registered(206), "the token is released");
    }

    #[test]
    fn a_cancel_that_arrives_first_wins() {
        // The race a slider drag actually produces: the app cancels the
        // previous evaluation as it issues the next one.
        assert_eq!(cadpole_cancel(202), CADPOLE_OK);
        let (rc, _) = call_cancellable(&api_doc_request(), 202);
        assert_eq!(rc, CADPOLE_ERR_CANCELLED);
        assert!(!registry().lock().unwrap().contains_key(&202));
    }

    #[test]
    fn cancel_releases_a_call_in_flight() {
        let token = 303;
        let handle = std::thread::spawn(move || call_cancellable(&slow_evaluate_request(0), token));
        // Cancel repeatedly until the waiter has registered the token; the call
        // may not have reached `register` yet, and an early cancel is also
        // valid (it is the case above).
        let start = Instant::now();
        while start.elapsed() < Duration::from_secs(10) {
            assert_eq!(cadpole_cancel(token), CADPOLE_OK);
            if handle.is_finished() {
                break;
            }
            std::thread::sleep(Duration::from_millis(5));
        }
        let (rc, _) = handle.join().unwrap();
        assert_eq!(rc, CADPOLE_ERR_CANCELLED);
    }

    fn registered(token: u64) -> bool {
        registry().lock().unwrap_or_else(|e| e.into_inner()).contains_key(&token)
    }

    /// Cancelling a call that has already finished must leave nothing behind.
    ///
    /// This is not the exotic case, it is the ordinary one. `LocalEngine`
    /// hands every call a token from a counter, never reuses one, and
    /// documents `cancel` as safe "after it finishes" -- which is exactly
    /// what a slider drag does when it supersedes a request that had already
    /// returned. `cadpole_cancel` records a cancel it cannot deliver so that a
    /// call arriving a moment later still loses, and nothing else removes
    /// that record: `unregister` runs only at the end of a call under the
    /// same token, and that token never comes back. So each late cancel used
    /// to leak one entry for the life of the process.
    #[test]
    fn a_cancel_that_arrives_after_the_call_finished_leaks_nothing() {
        let tokens: Vec<u64> = (91_000..91_016).collect();
        for &token in &tokens {
            let (rc, _) = call_cancellable(&api_doc_request(), token);
            assert_eq!(rc, CADPOLE_OK);
            assert!(!registered(token), "the call must release its own token");
            cadpole_cancel(token); // the supersede arrives late, as it usually does
        }
        // Each of those is now an unclaimed entry that nothing else will ever
        // remove. Age them past the grace window instead of sleeping a
        // minute, then let one more cancel run the sweep. Checking them while
        // they are still fresh would pass with or without the sweep, which is
        // the same as not testing it.
        backdate(&tokens);
        cadpole_cancel(91_999);
        let leaked: Vec<u64> = tokens.iter().copied().filter(|&t| registered(t)).collect();
        assert!(leaked.is_empty(), "a late cancel leaked an entry per call: {leaked:?}");
    }

    /// Pretend these cancels happened a while ago.
    fn backdate(tokens: &[u64]) {
        let mut registry = registry().lock().unwrap_or_else(|e| e.into_inner());
        for token in tokens {
            if let Some(entry) = registry.get_mut(token) {
                entry.since = Instant::now() - UNCLAIMED_GRACE * 2;
            }
        }
    }

    /// The sweep is what bounds the registry, so test it directly rather than
    /// sleeping a minute: an unclaimed entry past the grace window goes, a
    /// claimed one stays however old it is.
    #[test]
    fn the_sweep_drops_stale_cancels_and_keeps_running_calls() {
        let mut registry: HashMap<u64, Entry> = HashMap::new();
        registry.insert(1, Entry { since: Instant::now() - UNCLAIMED_GRACE * 2, ..Entry::unclaimed() });
        registry.insert(2, Entry::unclaimed());
        registry.insert(3, Entry { claimed: true, since: Instant::now() - UNCLAIMED_GRACE * 2, ..Entry::unclaimed() });
        sweep_unclaimed(&mut registry);
        assert!(!registry.contains_key(&1), "a cancel nobody claimed inside the window is not kept for ever");
        assert!(registry.contains_key(&2), "a fresh cancel must still be able to win its race");
        assert!(registry.contains_key(&3), "a call in flight is never swept, however long it runs");
    }

    /// A script that will not finish must not hold the caller, and the caller
    /// must be told a deadline is why.
    ///
    /// Note what is *not* asserted: which of two mechanisms got there first.
    /// `deadline_of` reads `EvaluateRequest.deadline_ms`, and the very same
    /// field bounds the Starlark evaluation inside the worker, so both are
    /// armed for the same instant. Either the waiter here times out and
    /// returns `CADPOLE_ERR_DEADLINE`, or the interpreter stops itself a moment
    /// sooner and the worker hands back a response carrying a deadline
    /// diagnostic. Both are correct and the winner depends on how loaded the
    /// machine is; pinning one of them makes this test fail under a parallel
    /// build, which is a flake that teaches the reader to distrust the suite.
    #[test]
    fn the_deadline_releases_the_caller() {
        let start = Instant::now();
        let (rc, bytes) = call_cancellable(&slow_evaluate_request(30), 404);
        if rc == CADPOLE_OK {
            let resp = pb::Response::decode(bytes.as_slice()).expect("a response");
            let Some(pb::response::Response::Evaluate(e)) = resp.response else {
                panic!("expected an evaluate response, got {:?}", resp.response);
            };
            let said: Vec<&str> =
                e.model.iter().flat_map(|m| m.diagnostics.iter()).map(|d| d.message.as_str()).collect();
            assert!(
                said.iter().any(|m| m.contains("deadline")),
                "the interpreter stopped first, so the response must say a deadline did it: {said:?}"
            );
        } else {
            assert_eq!(rc, CADPOLE_ERR_DEADLINE);
        }
        // 60 s, not 10. What is under test is that the deadline releases the
        // caller *at all* rather than waiting for a two-billion-iteration loop,
        // which takes minutes; anything inside a minute proves that. A tighter
        // bound measures how loaded the machine is, and this test fails
        // on a box saturated by a parallel build, which is a flake rather than
        // a regression and teaches the wrong lesson.
        assert!(start.elapsed() < Duration::from_secs(60), "the deadline must release the caller, took {:?}", start.elapsed());
        assert!(!registry().lock().unwrap().contains_key(&404));
    }

    #[test]
    fn a_generous_deadline_does_not_fire() {
        let (rc, bytes) = call_cancellable(&quick_evaluate_request(600_000), 505);
        assert_eq!(rc, CADPOLE_OK);
        let resp = pb::Response::decode(bytes.as_slice()).unwrap();
        assert!(matches!(resp.response, Some(pb::response::Response::Evaluate(_))));
    }

    #[test]
    fn cancelling_an_unknown_token_is_harmless() {
        assert_eq!(cadpole_cancel(0), CADPOLE_OK);
        assert_eq!(cadpole_cancel(u64::MAX), CADPOLE_OK);
        // A cancel for a token nobody claims leaves an entry behind; the next
        // call with that token consumes it. Clean up so the registry
        // assertions in the other tests stay meaningful.
        let (rc, _) = call_cancellable(&api_doc_request(), u64::MAX);
        assert_eq!(rc, CADPOLE_ERR_CANCELLED);
    }
}
