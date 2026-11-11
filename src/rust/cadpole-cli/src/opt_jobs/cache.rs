//! The persistent solution cache: one file per answered question,
//! under the user's cache directory, shared by every `cadpole` process on the
//! machine.
//!
//! **Keyed by the question.** The name of an entry is the SHA-256 of
//! `cadpole_core::opt::digest` -- the model in declaration order, the options
//! that decide the answer and the solver's identity -- with this format's
//! number and the platform, because the Linux and macOS builds may
//! break a tie differently: a home directory shared between the two never
//! serves one's tie to the other.
//!
//! **Safe with any number of writers.** Every file is written into `tmp/` and
//! renamed into place, which is atomic within one filesystem, so a reader
//! sees a whole file or none. Two writers of one answer write the same bits
//! and the last rename wins. There is no fsync: an entry is disposable, and
//! one torn by a power cut fails its checksum (`codec`) and is a miss.
//!
//! **Bounded.** Least recently used goes first: a hit touches the entry's
//! modification time, and when a process has written an eighth of the cap
//! since it last looked, it deletes the oldest entries until the rest fit in
//! three quarters of it.

use std::path::{Path, PathBuf};
use std::sync::atomic::{AtomicU64, Ordering};
use std::time::{Duration, SystemTime};

use cadpole_engine::cadpole_core::memo::sha256;

use super::codec::{self, Answer};

/// The cap when `CADPOLE_OPT_CACHE_MB` says nothing.
pub const DEFAULT_CAP_MB: u64 = 512;

/// The directory under the cache root that this format owns. A future format
/// gets its own and leaves this one to be deleted by hand.
const VERSION_DIR: &str = "v1";

/// How old a temp file must be before a collection takes it for a crashed
/// writer's.
const STALE_TMP: Duration = Duration::from_secs(3600);

pub struct Cache {
    /// `<root>/v1`.
    dir: PathBuf,
    cap: u64,
    /// Bytes this process has written since it last collected.
    written: AtomicU64,
}

/// What a collection did.
#[derive(Debug, Default, Clone, Copy, PartialEq, Eq)]
pub struct Collected {
    pub removed: usize,
    pub bytes_before: u64,
    pub bytes_after: u64,
}

/// `os/arch` of this build, part of every key.
pub fn platform() -> String {
    format!("{}/{}", std::env::consts::OS, std::env::consts::ARCH)
}

/// The name an answer to `digest` is filed under, on `platform`.
pub fn key_for(digest: &[u8; 32], platform: &str) -> [u8; 32] {
    let mut b = Vec::with_capacity(64 + platform.len());
    b.extend_from_slice(b"cadpole.opt.cache\0");
    b.extend_from_slice(&codec::FORMAT.to_le_bytes());
    b.extend_from_slice(digest);
    b.extend_from_slice(platform.as_bytes());
    sha256(&b)
}

pub fn hex(bytes: &[u8]) -> String {
    bytes.iter().map(|b| format!("{b:02x}")).collect()
}

/// The id of the job that answers `key`: its first twelve hex digits, so the
/// same question gets the same job in every process.
pub fn job_id(key: &[u8; 32]) -> String {
    hex(&key[..6])
}

/// Whether `id` is shaped like a job id: what a request names is checked
/// before it is ever part of a path.
pub fn is_job_id(id: &str) -> bool {
    id.len() == 12 && id.bytes().all(|b| b.is_ascii_digit() || (b'a'..=b'f').contains(&b))
}

/// Where the cache lives, from the environment `var` reads: `CADPOLE_OPT_CACHE`
/// (`off` for no cache at all), else `$XDG_CACHE_HOME/cadpole/opt` when that is
/// an absolute path, else `$HOME/.cache/cadpole/opt`.
pub fn location_from(var: impl Fn(&str) -> Option<String>) -> Option<PathBuf> {
    if let Some(v) = var("CADPOLE_OPT_CACHE").filter(|v| !v.trim().is_empty()) {
        let v = v.trim();
        if matches!(v.to_ascii_lowercase().as_str(), "off" | "0" | "none" | "false") {
            return None;
        }
        return Some(PathBuf::from(v));
    }
    if let Some(x) = var("XDG_CACHE_HOME").filter(|x| Path::new(x).is_absolute()) {
        return Some(PathBuf::from(x).join("cadpole").join("opt"));
    }
    var("HOME").filter(|h| !h.is_empty()).map(|h| PathBuf::from(h).join(".cache").join("cadpole").join("opt"))
}

/// The cap from `CADPOLE_OPT_CACHE_MB`, in bytes.
pub fn cap_from(var: impl Fn(&str) -> Option<String>) -> u64 {
    var("CADPOLE_OPT_CACHE_MB").and_then(|v| v.trim().parse::<u64>().ok()).unwrap_or(DEFAULT_CAP_MB).max(1) * 1024 * 1024
}

/// A directory only this user can read, made with its parents.
pub fn private_dir(p: &Path) -> std::io::Result<()> {
    let mut b = std::fs::DirBuilder::new();
    b.recursive(true);
    #[cfg(unix)]
    {
        use std::os::unix::fs::DirBuilderExt as _;
        b.mode(0o700);
    }
    b.create(p)
}

/// A file only this user can read, created or truncated.
fn private_file(p: &Path) -> std::io::Result<std::fs::File> {
    let mut o = std::fs::OpenOptions::new();
    o.write(true).create(true).truncate(true);
    #[cfg(unix)]
    {
        use std::os::unix::fs::OpenOptionsExt as _;
        o.mode(0o600);
    }
    o.open(p)
}

impl Cache {
    /// The cache under `root`, created if need be.
    pub fn open(root: &Path, cap: u64) -> std::io::Result<Cache> {
        let dir = root.join(VERSION_DIR);
        for d in ["answers", "jobs", "tmp"] {
            private_dir(&dir.join(d))?;
        }
        Ok(Cache { dir, cap, written: AtomicU64::new(0) })
    }

    /// `<root>/v1`.
    pub fn dir(&self) -> &Path {
        &self.dir
    }

    /// Where jobs keep their records, problems, results and logs.
    pub fn jobs_dir(&self) -> PathBuf {
        self.dir.join("jobs")
    }

    pub fn cap(&self) -> u64 {
        self.cap
    }

    fn answer_path(&self, key: &[u8; 32]) -> PathBuf {
        let h = hex(key);
        self.dir.join("answers").join(&h[..2]).join(format!("{h}.kopt"))
    }

    /// The answer filed under `key`, if there is a sound one. A file that
    /// does not decode, or that is filed under the wrong name, is deleted.
    pub fn get(&self, key: &[u8; 32]) -> Option<Answer> {
        let path = self.answer_path(key);
        let bytes = std::fs::read(&path).ok()?;
        match codec::decode_answer(&bytes) {
            Ok(a) if a.key == *key => {
                // Least recently used is decided by this: best effort, since
                // a read-only cache still answers.
                if let Ok(f) = std::fs::File::options().write(true).open(&path) {
                    let _ = f.set_modified(SystemTime::now());
                }
                Some(a)
            }
            _ => {
                let _ = std::fs::remove_file(&path);
                None
            }
        }
    }

    /// File `a` under its key, replacing whatever was there.
    pub fn put(&self, a: &Answer) -> std::io::Result<()> {
        let path = self.answer_path(&a.key);
        if let Some(parent) = path.parent() {
            private_dir(parent)?;
        }
        let bytes = codec::encode_answer(a);
        self.write_atomic(&path, &bytes)?;
        let total = self.written.fetch_add(bytes.len() as u64, Ordering::Relaxed) + bytes.len() as u64;
        if total > self.cap / 8 {
            self.written.store(0, Ordering::Relaxed);
            self.collect();
        }
        Ok(())
    }

    /// Write `bytes` to `path` so that no reader ever sees part of them: into
    /// `tmp/` first, then a rename over `path`.
    pub fn write_atomic(&self, path: &Path, bytes: &[u8]) -> std::io::Result<()> {
        use std::io::Write as _;
        static SEQ: AtomicU64 = AtomicU64::new(0);
        let nanos = SystemTime::now().duration_since(SystemTime::UNIX_EPOCH).map(|d| d.subsec_nanos()).unwrap_or(0);
        let tmp = self.dir.join("tmp").join(format!(
            "{}-{}-{nanos}.part",
            std::process::id(),
            SEQ.fetch_add(1, Ordering::Relaxed)
        ));
        let result = (|| {
            let mut f = private_file(&tmp)?;
            f.write_all(bytes)?;
            drop(f);
            std::fs::rename(&tmp, path)
        })();
        if result.is_err() {
            let _ = std::fs::remove_file(&tmp);
        }
        result
    }

    /// Every entry: its path, size and last use.
    fn entries(&self) -> Vec<(PathBuf, u64, SystemTime)> {
        let mut out = Vec::new();
        let Ok(fans) = std::fs::read_dir(self.dir.join("answers")) else { return out };
        for fan in fans.flatten() {
            let Ok(files) = std::fs::read_dir(fan.path()) else { continue };
            for f in files.flatten() {
                let p = f.path();
                if p.extension().and_then(|e| e.to_str()) != Some("kopt") {
                    continue;
                }
                if let Ok(m) = f.metadata() {
                    out.push((p, m.len(), m.modified().unwrap_or(SystemTime::UNIX_EPOCH)));
                }
            }
        }
        out
    }

    /// How many answers, and how many bytes they take.
    pub fn usage(&self) -> (usize, u64) {
        let e = self.entries();
        (e.len(), e.iter().map(|(_, n, _)| n).sum())
    }

    /// Delete the least recently used entries until the rest fit in three
    /// quarters of the cap -- if they do not fit in the cap now -- and temp
    /// files a crashed writer left.
    pub fn collect(&self) -> Collected {
        let now = SystemTime::now();
        if let Ok(tmp) = std::fs::read_dir(self.dir.join("tmp")) {
            for f in tmp.flatten() {
                let old = f.metadata().and_then(|m| m.modified()).map(|t| now.duration_since(t).unwrap_or_default() > STALE_TMP);
                if old.unwrap_or(false) {
                    let _ = std::fs::remove_file(f.path());
                }
            }
        }
        let mut entries = self.entries();
        let before: u64 = entries.iter().map(|(_, n, _)| n).sum();
        let mut out = Collected { removed: 0, bytes_before: before, bytes_after: before };
        if before <= self.cap {
            return out;
        }
        entries.sort_by_key(|(_, _, t)| *t);
        let target = self.cap / 4 * 3;
        for (path, n, _) in entries {
            if out.bytes_after <= target {
                break;
            }
            if std::fs::remove_file(&path).is_ok() {
                out.removed += 1;
                out.bytes_after -= n;
            }
        }
        out
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use cadpole_engine::cadpole_core::opt::{Solution, Status};

    use crate::opt_jobs::codec::Source;

    pub(crate) fn scratch(name: &str) -> PathBuf {
        let p = std::env::temp_dir().join(format!("cadpole-opt-cache-test-{name}-{}", std::process::id()));
        let _ = std::fs::remove_dir_all(&p);
        p
    }

    fn answer(key: [u8; 32], objective: f64) -> Answer {
        let mut s = Solution::without_point(Status::Optimal);
        s.values = Some(vec![objective, 1.0]);
        s.objective = Some(objective);
        Answer {
            key,
            identity: "test solver".into(),
            platform: platform(),
            made_unix_ms: 0,
            source: Source::Inline,
            limited: None,
            elapsed_ms: 1.0,
            solutions: vec![s],
        }
    }

    #[test]
    fn the_location_follows_the_environment() {
        let vars = |pairs: &'static [(&'static str, &'static str)]| move |k: &str| pairs.iter().find(|(n, _)| *n == k).map(|(_, v)| v.to_string());
        assert_eq!(location_from(vars(&[("HOME", "/home/u")])), Some(PathBuf::from("/home/u/.cache/cadpole/opt")));
        assert_eq!(location_from(vars(&[("HOME", "/home/u"), ("XDG_CACHE_HOME", "/x")])), Some(PathBuf::from("/x/cadpole/opt")));
        // The XDG spec ignores a relative path, and so does this.
        assert_eq!(location_from(vars(&[("HOME", "/home/u"), ("XDG_CACHE_HOME", "rel")])), Some(PathBuf::from("/home/u/.cache/cadpole/opt")));
        assert_eq!(location_from(vars(&[("HOME", "/home/u"), ("CADPOLE_OPT_CACHE", "/tmp/c")])), Some(PathBuf::from("/tmp/c")));
        assert_eq!(location_from(vars(&[("HOME", "/home/u"), ("CADPOLE_OPT_CACHE", "off")])), None);
        assert_eq!(location_from(vars(&[])), None);
        assert_eq!(cap_from(vars(&[("CADPOLE_OPT_CACHE_MB", "64")])), 64 << 20);
        assert_eq!(cap_from(vars(&[])), DEFAULT_CAP_MB << 20);
    }

    #[test]
    fn the_key_is_the_question_on_this_platform() {
        let d = [5u8; 32];
        let k = key_for(&d, "linux/x86_64");
        assert_eq!(k, key_for(&d, "linux/x86_64"));
        assert_ne!(k, key_for(&d, "macos/aarch64"), "a tie broken on one platform is not served to another");
        assert_ne!(k, key_for(&[6u8; 32], "linux/x86_64"));
        let id = job_id(&k);
        assert!(is_job_id(&id), "{id}");
        for bad in ["", "../../etc/pa", "3F9A2C1B7E44", "3f9a2c1b7e4", "3f9a2c1b7e44/", "3f9a2c1b7e4g"] {
            assert!(!is_job_id(bad), "{bad:?}");
        }
    }

    #[test]
    fn a_filed_answer_is_read_back_and_a_bad_one_is_a_miss_that_goes() {
        let root = scratch("roundtrip");
        let c = Cache::open(&root, 1 << 30).unwrap();
        let k = key_for(&[1; 32], &platform());
        assert!(c.get(&k).is_none());
        c.put(&answer(k, 42.0)).unwrap();
        assert_eq!(c.get(&k).unwrap().solutions[0].objective, Some(42.0));
        assert_eq!(c.usage().0, 1);
        // A torn file is a miss, and is deleted rather than read again.
        let path = c.answer_path(&k);
        let bytes = std::fs::read(&path).unwrap();
        std::fs::write(&path, &bytes[..bytes.len() / 2]).unwrap();
        assert!(c.get(&k).is_none());
        assert!(!path.exists());
        // A sound file filed under another name is not that name's answer.
        let other = key_for(&[2; 32], &platform());
        std::fs::create_dir_all(c.answer_path(&other).parent().unwrap()).unwrap();
        std::fs::write(c.answer_path(&other), codec::encode_answer(&answer(k, 1.0))).unwrap();
        assert!(c.get(&other).is_none());
        #[cfg(unix)]
        {
            use std::os::unix::fs::PermissionsExt as _;
            c.put(&answer(k, 42.0)).unwrap();
            let mode = std::fs::metadata(c.answer_path(&k)).unwrap().permissions().mode();
            assert_eq!(mode & 0o777, 0o600, "answers are the user's alone");
            let mode = std::fs::metadata(c.dir()).unwrap().permissions().mode();
            assert_eq!(mode & 0o777, 0o700);
        }
        let _ = std::fs::remove_dir_all(&root);
    }

    #[test]
    fn concurrent_writers_never_leave_a_partial_file() {
        let root = scratch("concurrent");
        let c = std::sync::Arc::new(Cache::open(&root, 1 << 30).unwrap());
        let shared = key_for(&[3; 32], &platform());
        let threads: Vec<_> = (0..8)
            .map(|t| {
                let c = c.clone();
                std::thread::spawn(move || {
                    for i in 0..25 {
                        c.put(&answer(shared, 7.0)).unwrap();
                        let mut d = [9u8; 32];
                        (d[0], d[1]) = (t as u8, i as u8);
                        let own = key_for(&d, &platform());
                        c.put(&answer(own, t as f64)).unwrap();
                        // Every read in the storm is a whole answer.
                        assert_eq!(c.get(&shared).expect("never torn").solutions[0].objective, Some(7.0));
                        assert_eq!(c.get(&own).unwrap().solutions[0].objective, Some(t as f64));
                    }
                })
            })
            .collect();
        for t in threads {
            t.join().unwrap();
        }
        assert_eq!(c.usage().0, 8 * 25 + 1);
        assert_eq!(std::fs::read_dir(c.dir().join("tmp")).unwrap().count(), 0, "no temp file is left behind");
        let _ = std::fs::remove_dir_all(&root);
    }

    #[test]
    fn a_collection_keeps_the_most_recently_used_under_the_cap() {
        let root = scratch("collect");
        let c = Cache::open(&root, 1 << 30).unwrap();
        let keys: Vec<[u8; 32]> = (0..10u8).map(|i| key_for(&[i; 32], &platform())).collect();
        let base = SystemTime::now() - Duration::from_secs(10_000);
        for (i, k) in keys.iter().enumerate() {
            c.put(&answer(*k, i as f64)).unwrap();
            let f = std::fs::File::options().write(true).open(c.answer_path(k)).unwrap();
            f.set_modified(base + Duration::from_secs(i as u64 * 10)).unwrap();
        }
        // Reading the oldest makes it the newest.
        assert!(c.get(&keys[0]).is_some());
        let one = std::fs::metadata(c.answer_path(&keys[0])).unwrap().len();
        // A cap of four entries: down to three (three quarters of it).
        let small = Cache { dir: c.dir.clone(), cap: one * 4, written: AtomicU64::new(0) };
        let got = small.collect();
        assert_eq!(got.removed, 7, "{got:?}");
        let kept: Vec<usize> = (0..10).filter(|&i| small.get(&keys[i]).is_some()).collect();
        assert_eq!(kept, vec![0, 8, 9], "the touched one and the two newest");
        // Under the cap, nothing goes.
        assert_eq!(small.collect().removed, 0);
        // A stale temp file goes; a fresh one (a writer at work) stays.
        let stale = c.dir().join("tmp").join("1-1-1.part");
        let fresh = c.dir().join("tmp").join("1-2-1.part");
        std::fs::write(&stale, b"x").unwrap();
        std::fs::write(&fresh, b"x").unwrap();
        std::fs::File::options().write(true).open(&stale).unwrap().set_modified(SystemTime::now() - Duration::from_secs(7200)).unwrap();
        c.collect();
        assert!(!stale.exists() && fresh.exists());
        let _ = std::fs::remove_dir_all(&root);
    }
}
