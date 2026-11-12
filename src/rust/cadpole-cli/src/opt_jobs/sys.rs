//! The few process controls a solve job needs, declared by hand against the
//! C library `std` already links: limits on the child (`setrlimit`), a lower
//! priority (`getpriority`, `setpriority`), and signals by pid (`kill`). No
//! crate: these are a handful of POSIX calls with stable signatures, and the constants below
//! are the two platforms Cadpole builds natively for.

#[cfg(unix)]
mod imp {
    use std::ffi::c_int;

    #[repr(C)]
    struct RLimit {
        cur: u64,
        max: u64,
    }

    // `rlim_t` is a 64-bit unsigned integer on Linux (glibc, x86_64 and
    // aarch64) and on macOS; `id_t` is 32 bits on both.
    unsafe extern "C" {
        fn setrlimit(resource: c_int, rlim: *const RLimit) -> c_int;
        fn getpriority(which: c_int, who: u32) -> c_int;
        fn setpriority(which: c_int, who: u32, prio: c_int) -> c_int;
        #[cfg(target_os = "linux")]
        fn __errno_location() -> *mut c_int;
        #[cfg(target_os = "macos")]
        fn __error() -> *mut c_int;
        fn kill(pid: c_int, sig: c_int) -> c_int;
    }

    #[cfg(target_os = "linux")]
    mod consts {
        pub const RLIMIT_CPU: i32 = 0;
        pub const RLIMIT_FSIZE: i32 = 1;
        pub const RLIMIT_CORE: i32 = 4;
        pub const RLIMIT_NOFILE: i32 = 7;
        pub const RLIMIT_AS: i32 = 9;
    }
    #[cfg(target_os = "macos")]
    mod consts {
        pub const RLIMIT_CPU: i32 = 0;
        pub const RLIMIT_FSIZE: i32 = 1;
        pub const RLIMIT_CORE: i32 = 4;
        pub const RLIMIT_AS: i32 = 5;
        pub const RLIMIT_NOFILE: i32 = 8;
    }
    pub use consts::*;

    pub const PRIO_PROCESS: i32 = 0;
    pub const SIGKILL: i32 = 9;
    pub const SIGTERM: i32 = 15;

    pub fn set_limit(resource: i32, soft: u64, hard: u64) -> std::io::Result<()> {
        let r = RLimit { cur: soft, max: hard };
        // SAFETY: `r` is a valid `struct rlimit` for the duration of the call.
        if unsafe { setrlimit(resource, &r) } == 0 { Ok(()) } else { Err(std::io::Error::last_os_error()) }
    }

    fn errno() -> *mut c_int {
        // SAFETY: both return this thread's `errno`, valid for its lifetime.
        #[cfg(target_os = "linux")]
        unsafe {
            __errno_location()
        }
        #[cfg(target_os = "macos")]
        unsafe {
            __error()
        }
    }

    /// This process's nice value. -1 is a valid one, so `errno` is cleared
    /// first and checked after, as POSIX prescribes.
    pub fn priority() -> std::io::Result<i32> {
        // SAFETY: `errno()` points at this thread's errno; plain integers.
        unsafe {
            *errno() = 0;
            let p = getpriority(PRIO_PROCESS, 0);
            if p == -1 && *errno() != 0 { Err(std::io::Error::last_os_error()) } else { Ok(p) }
        }
    }

    /// At least nice `prio`: a process already running nicer (a job started
    /// by a server that was itself started under `nice`) is left where it
    /// is, since moving it back up is not permitted without privilege and is
    /// not wanted anyway.
    pub fn nice_at_least(prio: i32) -> std::io::Result<()> {
        if priority()? >= prio {
            return Ok(());
        }
        // SAFETY: plain integers; `who` 0 is this process.
        if unsafe { setpriority(PRIO_PROCESS, 0, prio) } == 0 { Ok(()) } else { Err(std::io::Error::last_os_error()) }
    }

    pub fn signal(pid: u32, sig: i32) -> std::io::Result<()> {
        let pid = c_int::try_from(pid).map_err(|_| std::io::Error::other("no such pid"))?;
        if pid <= 0 {
            // 0 and negative pids name process groups; a job is never one.
            return Err(std::io::Error::other("refusing to signal a process group"));
        }
        // SAFETY: plain integers.
        if unsafe { kill(pid, sig) } == 0 { Ok(()) } else { Err(std::io::Error::last_os_error()) }
    }
}

/// The limits a job's process puts on itself before it solves.
#[derive(Clone, Copy, Debug, PartialEq)]
pub struct JobLimits {
    /// CPU seconds; `None` for none. HiGHS runs one thread, so this is about
    /// the wall time of the solve.
    pub cpu_seconds: Option<u64>,
    /// Address space in MB; 0 for none.
    pub mem_mb: u64,
}

/// Apply `l` to this process, plus the ones every job gets: no core file,
/// a 1 GB cap on any file it writes, a modest descriptor count, and at least
/// nice 10 so a solve never starves the server it is answering. Errors name what
/// failed; a limit the platform will not take is not worth refusing a job
/// over, so the caller logs and carries on.
pub fn limit_self(l: JobLimits) -> Vec<String> {
    let mut problems = Vec::new();
    #[cfg(unix)]
    {
        let mut set = |what: &str, r: i32, soft: u64, hard: u64| {
            if let Err(e) = imp::set_limit(r, soft, hard) {
                problems.push(format!("{what}: {e}"));
            }
        };
        set("core", imp::RLIMIT_CORE, 0, 0);
        set("file size", imp::RLIMIT_FSIZE, 1 << 30, 1 << 30);
        set("open files", imp::RLIMIT_NOFILE, 256, 256);
        if let Some(s) = l.cpu_seconds {
            // The soft limit sends SIGXCPU, the hard one SIGKILL ten seconds
            // later, for a process that ignores the first.
            set("cpu", imp::RLIMIT_CPU, s.max(1), s.max(1) + 10);
        }
        // macOS accepts RLIMIT_AS but does not enforce it: there the
        // parent's wall-clock watchdog and the time limit are the bounds.
        if l.mem_mb > 0 {
            let bytes = l.mem_mb.saturating_mul(1024 * 1024);
            set("address space", imp::RLIMIT_AS, bytes, bytes);
        }
        if let Err(e) = imp::nice_at_least(10) {
            problems.push(format!("priority: {e}"));
        }
    }
    #[cfg(not(unix))]
    {
        let _ = l;
        problems.push("process limits are not implemented on this platform".into());
    }
    problems
}

/// Whether `pid` is a live process (or a zombie not yet reaped).
pub fn alive(pid: u32) -> bool {
    #[cfg(unix)]
    {
        match imp::signal(pid, 0) {
            Ok(()) => true,
            // EPERM: it exists and is someone else's -- not ours to own, but
            // alive.
            Err(e) => e.raw_os_error() == Some(1),
        }
    }
    #[cfg(not(unix))]
    {
        let _ = pid;
        false
    }
}

/// Whether `pid` is the job `id`'s process: on Linux its command line names
/// the hidden subcommand and the id, so a recycled pid is never mistaken for
/// a job (and never signalled as one). Elsewhere, being alive is all there
/// is to go on.
pub fn is_job_process(pid: u32, id: &str) -> bool {
    if !alive(pid) {
        return false;
    }
    #[cfg(target_os = "linux")]
    {
        match std::fs::read(format!("/proc/{pid}/cmdline")) {
            Ok(raw) => {
                let args: Vec<&[u8]> = raw.split(|&b| b == 0).collect();
                args.iter().any(|a| *a == b"opt-job") && args.iter().any(|a| *a == id.as_bytes())
            }
            Err(_) => false,
        }
    }
    #[cfg(not(target_os = "linux"))]
    {
        let _ = id;
        true
    }
}

/// Ask a job's process to stop, then make it. `still` says whether `pid` is
/// still that job -- asked again before the second signal, because once the
/// job has died and been waited for, its pid can be handed to a stranger.
pub fn terminate(pid: u32, still: impl Fn() -> bool) -> std::io::Result<()> {
    #[cfg(unix)]
    {
        imp::signal(pid, imp::SIGTERM)?;
        for _ in 0..20 {
            if !still() {
                return Ok(());
            }
            std::thread::sleep(std::time::Duration::from_millis(100));
        }
        if still() {
            let _ = imp::signal(pid, imp::SIGKILL);
        }
        Ok(())
    }
    #[cfg(not(unix))]
    {
        let _ = (pid, still);
        Err(std::io::Error::other("cannot signal a process on this platform"))
    }
}

/// `SIGKILL` now, for a child past its wall-clock limit.
pub fn kill_now(pid: u32) {
    #[cfg(unix)]
    {
        let _ = imp::signal(pid, imp::SIGKILL);
    }
    #[cfg(not(unix))]
    {
        let _ = pid;
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn this_process_is_alive_and_is_no_job() {
        let me = std::process::id();
        assert!(alive(me));
        assert!(!is_job_process(me, "3f9a2c1b7e44"));
        // A pid that cannot exist on any system Cadpole runs on.
        assert!(!alive(i32::MAX as u32 - 1));
        assert!(imp_refuses_groups());
    }

    #[cfg(unix)]
    fn imp_refuses_groups() -> bool {
        super::imp::signal(0, 0).is_err()
    }

    #[cfg(not(unix))]
    fn imp_refuses_groups() -> bool {
        true
    }

    /// "At least nice 10" holds from any starting niceness, including one
    /// above 10 that this process is not permitted to lower. In a child, so
    /// the test process keeps its own priority.
    #[cfg(target_os = "linux")]
    #[test]
    fn a_job_under_a_niced_parent_keeps_its_niceness() {
        for start in [0, 15, 19] {
            let child = std::process::Command::new("nice")
                .args(["-n", &start.to_string()])
                .arg(std::env::current_exe().unwrap())
                .args(["--exact", "opt_jobs::sys::tests::niceness_in_a_child", "--ignored", "--nocapture"])
                .env("CADPOLE_SYS_TEST_CHILD", "1")
                .output()
                .expect("the test binary runs under nice");
            let text = String::from_utf8_lossy(&child.stdout);
            assert!(child.status.success() && text.contains("niceness kept"), "from +{start}: {text}\n{}", String::from_utf8_lossy(&child.stderr));
        }
    }

    /// Run only as the child of the test above.
    #[cfg(unix)]
    #[test]
    #[ignore]
    fn niceness_in_a_child() {
        if std::env::var("CADPOLE_SYS_TEST_CHILD").is_err() {
            return;
        }
        let before = super::imp::priority().unwrap();
        super::imp::nice_at_least(10).unwrap();
        let after = super::imp::priority().unwrap();
        assert_eq!(after, before.max(10), "from {before}");
        println!("niceness kept");
    }

    /// The limits bite: a child that sets a 1-second CPU limit on itself and
    /// then spins is killed by the kernel, not by us.
    #[cfg(target_os = "linux")]
    #[test]
    fn a_cpu_limit_stops_a_spinning_child() {
        // `sh -c` stands in for a job: ulimit is the same setrlimit call.
        let out = std::process::Command::new("sh")
            .args(["-c", "ulimit -t 1; while :; do :; done"])
            .status()
            .expect("sh runs");
        assert!(!out.success(), "a spinning shell under a 1-s CPU limit must die");
        // ...and our own call takes effect on this process's soft limits
        // without error, in a child so the test process keeps its own.
        let child = std::process::Command::new(std::env::current_exe().unwrap())
            .args(["--exact", "opt_jobs::sys::tests::limits_apply_in_a_child", "--ignored", "--nocapture"])
            .env("CADPOLE_SYS_TEST_CHILD", "1")
            .output()
            .expect("the test binary runs");
        let text = String::from_utf8_lossy(&child.stdout);
        assert!(child.status.success() && text.contains("limits applied"), "{text}\n{}", String::from_utf8_lossy(&child.stderr));
    }

    /// Run only as the child of the test above.
    #[test]
    #[ignore]
    fn limits_apply_in_a_child() {
        if std::env::var("CADPOLE_SYS_TEST_CHILD").is_err() {
            return;
        }
        let problems = limit_self(JobLimits { cpu_seconds: Some(3600), mem_mb: 16 * 1024 });
        assert!(problems.is_empty(), "{problems:?}");
        let limits = std::fs::read_to_string("/proc/self/limits").unwrap();
        let line = |name: &str| limits.lines().find(|l| l.starts_with(name)).unwrap_or("").to_owned();
        assert!(line("Max cpu time").contains("3600"), "{}", line("Max cpu time"));
        assert!(line("Max core file size").split_whitespace().nth(4) == Some("0"), "{}", line("Max core file size"));
        assert!(line("Max address space").contains(&(16u64 << 30).to_string()), "{}", line("Max address space"));
        assert!(super::imp::priority().unwrap() >= 10);
        println!("limits applied");
    }
}
