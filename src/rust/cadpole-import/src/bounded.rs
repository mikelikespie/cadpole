//! A parse with a deadline and a stack of its own.
//!
//! `ruststep`'s parameter grammar recurses without a depth limit and a Rust
//! stack overflow is an abort, not a panic; a parse of a large file cannot be
//! interrupted by the evaluation deadline, which starlark polls between
//! builtins. So every reader runs through [`run`]: natively, on a thread
//! whose stack size is Cadpole's choice (64 MB, so the bound the [`crate::prescan`]
//! enforces is a bound on *that* stack, not the OS default's), and whose
//! result is waited for with a timeout. When the timeout passes the caller
//! gets a diagnostic and moves on; the thread finishes on its own — a thread
//! cannot be killed — and its result is dropped. Its memory is what the caps
//! allow, and no second import of the same bytes starts until the first is
//! forgotten, because the caller's memo key is the file's hash.
//!
//! A panic inside the closure — the lexer's `i64` overflow, a knot vector
//! `unwrap`, an index in `truck` — is caught and returned as the message it
//! carried. It is kept off stderr by `cadpole_core::kernel::quietly`, as
//! `cadpole_core::kernel::guarded` does, because the message goes to the
//! script, not to stderr.
//!
//! In wasm there are no threads and no clock (docs/security.md): the closure
//! runs inline under `catch_unwind`, and the triangle and entity caps are
//! what bound it.

use std::panic::{AssertUnwindSafe, catch_unwind};

/// The stack the parse thread gets, in bytes.
pub const STACK_BYTES: usize = 64 << 20;

/// What stopped a bounded run.
#[derive(Clone, Debug, PartialEq, Eq)]
pub enum Stopped {
    /// The budget passed; the work was abandoned.
    Deadline { budget_ms: u64 },
    /// The closure panicked; the message it carried.
    Panic(String),
    /// The thread could not be started, or died without answering.
    Thread(String),
}

impl std::fmt::Display for Stopped {
    fn fmt(&self, f: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
        match self {
            Stopped::Deadline { budget_ms } => write!(
                f,
                "did not finish within its {budget_ms} ms budget and was abandoned (a file that \
                 takes this long is either far over the caps or hostile; the evaluation deadline \
                 is `--eval-budget-secs` on the CLI, `cadpole serve --eval-deadline-ms` or \
                 EvaluateRequest.deadline_ms)"
            ),
            Stopped::Panic(m) => write!(f, "the parser failed: {m}"),
            Stopped::Thread(m) => write!(f, "the parse thread failed: {m}"),
        }
    }
}

fn panic_message(e: Box<dyn std::any::Any + Send>) -> String {
    e.downcast_ref::<String>()
        .cloned()
        .or_else(|| e.downcast_ref::<&str>().map(|s| s.to_string()))
        .unwrap_or_else(|| "unknown panic".to_owned())
}

/// Run `f` with a `budget_ms` deadline (0 means no deadline beyond the
/// panic guard) and a 64 MB stack, returning what it produced or why it
/// stopped.
#[cfg(not(target_arch = "wasm32"))]
pub fn run<T: Send + 'static>(budget_ms: u64, f: impl FnOnce() -> T + Send + 'static) -> Result<T, Stopped> {
    use std::sync::mpsc;
    use std::time::Duration;

    let (tx, rx) = mpsc::channel();
    // The parse thread's panics are kept off stderr — the message is
    // returned, not printed — by the process-wide hook that is quiet only
    // for a thread inside `quietly`. Nothing global is swapped, so runs on
    // several threads cannot undo each other.
    let spawned = std::thread::Builder::new().name("cadpole-import".into()).stack_size(STACK_BYTES).spawn(move || {
        let r = cadpole_core::kernel::quietly(|| catch_unwind(AssertUnwindSafe(f)));
        let _ = tx.send(r);
    });
    let out = match spawned {
        Err(e) => Err(Stopped::Thread(e.to_string())),
        Ok(handle) => {
            let got = if budget_ms == 0 {
                rx.recv().map_err(|_| mpsc::RecvTimeoutError::Disconnected)
            } else {
                rx.recv_timeout(Duration::from_millis(budget_ms))
            };
            match got {
                Ok(Ok(v)) => {
                    let _ = handle.join();
                    Ok(v)
                }
                Ok(Err(p)) => {
                    let _ = handle.join();
                    Err(Stopped::Panic(panic_message(p)))
                }
                Err(mpsc::RecvTimeoutError::Timeout) => Err(Stopped::Deadline { budget_ms }),
                Err(mpsc::RecvTimeoutError::Disconnected) => {
                    Err(Stopped::Thread("the parse thread ended without a result".into()))
                }
            }
        }
    };
    out
}

/// The wasm form: inline, panic-guarded, no clock.
#[cfg(target_arch = "wasm32")]
pub fn run<T: Send + 'static>(_budget_ms: u64, f: impl FnOnce() -> T + Send + 'static) -> Result<T, Stopped> {
    catch_unwind(AssertUnwindSafe(f)).map_err(|p| Stopped::Panic(panic_message(p)))
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn a_result_comes_back() {
        assert_eq!(run(1000, || 42).unwrap(), 42);
        assert_eq!(run(0, || "no deadline").unwrap(), "no deadline");
    }

    #[test]
    fn a_panic_is_a_message_not_a_crash() {
        let e = run(1000, || -> u32 { panic!("knot vector is not clamped") }).unwrap_err();
        assert_eq!(e, Stopped::Panic("knot vector is not clamped".into()));
        assert!(e.to_string().contains("knot vector"));
    }

    #[cfg(not(target_arch = "wasm32"))]
    #[test]
    fn the_deadline_fires_and_the_caller_is_released() {
        let started = std::time::Instant::now();
        let e = run(20, || {
            std::thread::sleep(std::time::Duration::from_millis(400));
            1u8
        })
        .unwrap_err();
        assert!(matches!(e, Stopped::Deadline { budget_ms: 20 }), "{e:?}");
        assert!(started.elapsed().as_millis() < 300, "released at the deadline, not at the end of the work");
        assert!(e.to_string().contains("budget"));
    }

    #[cfg(not(target_arch = "wasm32"))]
    #[test]
    fn the_stack_is_ours() {
        // 20 MB of recursion overflows a default 2 MB test-thread stack;
        // on the parse thread it is fine.
        fn deep(n: usize) -> usize {
            let pad = [n as u8; 1024];
            if n == 0 { pad[0] as usize } else { deep(n - 1) + pad[1] as usize }
        }
        assert!(run(5000, || deep(20_000)).is_ok());
    }
    /// Imports run from a sweep's threads at once. The guard used to swap
    /// the process-wide panic hook for the whole wait, so one import's
    /// restore could unsilence another's parse thread, or leave the process
    /// silent. Run in a child process, because stderr is what is tested:
    /// no caught parser panic printed, every other panic printed.
    #[cfg(not(target_arch = "wasm32"))]
    #[test]
    fn concurrent_runs_keep_caught_panics_quiet_and_others_loud() {
        const CHILD: &str = "CADPOLE_BOUNDED_RACE_CHILD";
        const LOUD: usize = 2 * 40;
        if std::env::var_os(CHILD).is_some() {
            let quiet: Vec<_> = (0..6)
                .map(|i| {
                    std::thread::spawn(move || {
                        for n in 0..100 {
                            let e = run(0, move || -> u8 { panic!("parser-panic-{i}-{n}") }).unwrap_err();
                            assert!(matches!(e, Stopped::Panic(_)));
                        }
                    })
                })
                .collect();
            let loud: Vec<_> = (0..2)
                .map(|_| {
                    std::thread::spawn(|| {
                        for _ in 0..LOUD / 2 {
                            let _ = catch_unwind(|| panic!("unrelated-panic"));
                            std::thread::yield_now();
                        }
                    })
                })
                .collect();
            for t in quiet.into_iter().chain(loud) {
                t.join().unwrap();
            }
            return;
        }
        let out = std::process::Command::new(std::env::current_exe().unwrap())
            .args(["--exact", "bounded::tests::concurrent_runs_keep_caught_panics_quiet_and_others_loud", "--nocapture", "--test-threads=1"])
            .env(CHILD, "1")
            .output()
            .unwrap();
        let err = String::from_utf8_lossy(&out.stderr);
        assert!(out.status.success(), "the child failed: {err}");
        let leaked = err.matches("parser-panic-").count();
        let printed = err.matches("unrelated-panic").count();
        assert_eq!(leaked, 0, "{leaked} caught parser panics reached stderr");
        assert_eq!(printed, LOUD, "{} of {LOUD} unrelated panics were swallowed", LOUD - printed);
    }

}
