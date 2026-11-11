//! The agent socket and discovery end to end, on the real binary: `cadpole
//! serve` processes in a private runtime directory, and `cadpole mcp` and
//! `cadpole op` started with no `--server` at all.
//!
//! What only the real programs can show: that a client with no flags finds
//! the one server, refuses to guess between two, follows a restarted server
//! of its own document and never another document's, that a crashed server's
//! files are ignored and then cleaned up, and that no token reaches a log,
//! a record, a command line or the MCP output.

use std::io::{BufRead, BufReader, Write as _};
use std::path::{Path, PathBuf};
use std::process::{Child, ChildStdin, ChildStdout, Command, Stdio};
use std::sync::{Arc, Mutex};
use std::time::Duration;

use serde_json::{Value, json};

use crate::tests::cadpole_binary;

fn scratch(name: &str) -> PathBuf {
    let d = std::env::temp_dir().join(format!("cadpole-transport-{name}-{}", std::process::id()));
    let _ = std::fs::remove_dir_all(&d);
    std::fs::create_dir_all(&d).unwrap();
    d
}

/// One `cadpole serve`, its stderr collected, stopped (SIGTERM) when dropped.
struct Served {
    child: Child,
    said: Arc<Mutex<Vec<String>>>,
    instance: String,
}

impl Served {
    fn start(bin: &Path, runtime: &Path, doc: &Path, extra: &[&str]) -> Served {
        let mut child = Command::new(bin)
            .args(["serve", "--port", "0", "--no-solve-jobs", "--web-dir"])
            .arg(runtime.join("no-web"))
            .args(extra)
            .arg(doc)
            .env("CADPOLE_RUNTIME_DIR", runtime)
            .env("CADPOLE_TOKEN", "")
            .stdin(Stdio::null())
            .stdout(Stdio::null())
            .stderr(Stdio::piped())
            .spawn()
            .expect("cadpole serve starts");
        let said = Arc::new(Mutex::new(Vec::new()));
        let (tx, rx) = std::sync::mpsc::channel();
        {
            let said = said.clone();
            let lines = BufReader::new(child.stderr.take().unwrap()).lines();
            std::thread::spawn(move || {
                for line in lines {
                    let line = line.unwrap_or_default();
                    if let Some(rest) = line.strip_prefix("agents: unix:") {
                        let sock = rest.split(' ').next().unwrap_or("").to_owned();
                        let _ = tx.send(sock);
                    }
                    said.lock().unwrap().push(line);
                }
            });
        }
        let sock = rx.recv_timeout(Duration::from_secs(60)).unwrap_or_else(|_| panic!("cadpole serve did not start:\n{}", said.lock().unwrap().join("\n")));
        let instance = Path::new(&sock).file_stem().unwrap().to_str().unwrap().to_owned();
        Served { child, said, instance }
    }

    fn files(&self, runtime: &Path) -> crate::runtime::RunFiles {
        crate::runtime::RunFiles::new(runtime, &self.instance)
    }

    /// SIGTERM, and wait for the exit: the clean way down.
    fn stop(mut self) -> Vec<String> {
        crate::opt_jobs::sys::terminate(self.child.id(), || false).unwrap();
        let _ = self.child.wait();
        std::mem::take(&mut *self.said.lock().unwrap())
    }

    /// SIGKILL: a crash, which leaves every file behind.
    fn crash(mut self) {
        let _ = self.child.kill();
        let _ = self.child.wait();
    }
}

impl Drop for Served {
    fn drop(&mut self) {
        // Only a child not yet reaped: a reaped one's pid may be someone
        // else's by now.
        if matches!(self.child.try_wait(), Ok(None)) {
            let _ = crate::opt_jobs::sys::terminate(self.child.id(), || false);
            let _ = self.child.wait();
        }
    }
}

/// `cadpole mcp`, spoken to over stdio, with what it prints kept.
struct Mcp {
    child: Child,
    input: ChildStdin,
    output: BufReader<ChildStdout>,
    seen: Vec<String>,
    stderr: Arc<Mutex<Vec<String>>>,
    next: u64,
}

impl Mcp {
    fn start(bin: &Path, runtime: &Path, cwd: &Path) -> Mcp {
        let mut child = Command::new(bin)
            .arg("mcp")
            .current_dir(cwd)
            .env("CADPOLE_RUNTIME_DIR", runtime)
            .env_remove("CADPOLE_TOKEN")
            .env_remove("CADPOLE_SERVER")
            .stdin(Stdio::piped())
            .stdout(Stdio::piped())
            .stderr(Stdio::piped())
            .spawn()
            .expect("cadpole mcp starts");
        let stderr = Arc::new(Mutex::new(Vec::new()));
        {
            let stderr = stderr.clone();
            let lines = BufReader::new(child.stderr.take().unwrap()).lines();
            std::thread::spawn(move || {
                for l in lines {
                    stderr.lock().unwrap().push(l.unwrap_or_default());
                }
            });
        }
        let input = child.stdin.take().unwrap();
        let output = BufReader::new(child.stdout.take().unwrap());
        let mut m = Mcp { child, input, output, seen: Vec::new(), stderr, next: 1 };
        m.request("initialize", json!({"protocolVersion": "2025-06-18", "capabilities": {}, "clientInfo": {"name": "t", "version": "0"}}));
        writeln!(m.input, "{}", json!({"jsonrpc": "2.0", "method": "notifications/initialized"})).unwrap();
        m
    }

    fn request(&mut self, method: &str, params: Value) -> Value {
        let id = self.next;
        self.next += 1;
        writeln!(self.input, "{}", json!({"jsonrpc": "2.0", "id": id, "method": method, "params": params})).unwrap();
        self.input.flush().unwrap();
        loop {
            let mut line = String::new();
            assert!(self.output.read_line(&mut line).unwrap() > 0, "cadpole mcp closed its output");
            self.seen.push(line.clone());
            let v: Value = serde_json::from_str(&line).unwrap();
            if v["id"] == json!(id) {
                return v;
            }
        }
    }

    /// `get_session` as a tool call: the document's name, or the error text.
    fn document(&mut self) -> Result<String, String> {
        let r = self.request("tools/call", json!({"name": "get_session", "arguments": {}}));
        let text = r["result"]["content"][0]["text"].as_str().unwrap_or("").to_owned();
        if r["result"]["isError"].as_bool().unwrap_or(false) || r.get("error").is_some() {
            return Err(format!("{text}{}", r["error"]));
        }
        let v: Value = serde_json::from_str(&text).map_err(|e| format!("{e}: {text}"))?;
        Ok(v["document"]["name"].as_str().unwrap_or("").to_owned())
    }

    fn all_output(&self) -> String {
        format!("{}\n{}", self.seen.join(""), self.stderr.lock().unwrap().join("\n"))
    }
}

impl Drop for Mcp {
    fn drop(&mut self) {
        let _ = self.child.kill();
        let _ = self.child.wait();
    }
}

/// `cadpole op GetSession` with no flags, from `cwd`: the document's name, or
/// the error it printed.
fn op_document(bin: &Path, runtime: &Path, cwd: &Path) -> Result<String, String> {
    let o = Command::new(bin)
        .args(["op", "GetSession"])
        .current_dir(cwd)
        .env("CADPOLE_RUNTIME_DIR", runtime)
        .env_remove("CADPOLE_TOKEN")
        .env_remove("CADPOLE_SERVER")
        .output()
        .unwrap();
    if !o.status.success() {
        return Err(String::from_utf8_lossy(&o.stderr).into_owned());
    }
    let v: Value = serde_json::from_slice(&o.stdout).map_err(|e| format!("{e}"))?;
    Ok(v["document"]["name"].as_str().unwrap_or("").to_owned())
}

/// Every token file a server wrote, read while it runs.
fn tokens_of(runtime: &Path, s: &Served) -> Vec<String> {
    let f = s.files(runtime);
    let mut out: Vec<String> = [f.session_token(), f.listener_token()].iter().filter_map(|p| std::fs::read_to_string(p).ok()).map(|t| t.trim().to_owned()).collect();
    // The browser token, from the link a non-terminal run writes instead of
    // printing it.
    let url = std::fs::read_to_string(f.url()).expect("a server whose output is not a terminal writes its link to a file");
    out.push(url.trim().rsplit("#token=").next().unwrap().to_owned());
    assert!(out.iter().all(|t| t.len() >= 40), "three real tokens");
    out
}

fn mode(p: &Path) -> u32 {
    use std::os::unix::fs::PermissionsExt as _;
    std::fs::symlink_metadata(p).unwrap().permissions().mode() & 0o777
}

/// The whole path a person takes: `claude mcp add cadpole -- cadpole mcp`, no
/// URL, no port -- and what happens around it when servers come and go.
#[test]
fn a_client_with_no_flags_finds_follows_and_never_confuses_its_server() {
    let Some(bin) = cadpole_binary() else {
        eprintln!("cadpole binary not in runfiles; skipping");
        return;
    };
    let base = scratch("disc");
    let runtime = base.join("run");
    let (dir_a, dir_b) = (base.join("a"), base.join("b"));
    std::fs::create_dir_all(&dir_a).unwrap();
    std::fs::create_dir_all(&dir_b).unwrap();
    let (doc_a, doc_b) = (dir_a.join("alpha.star"), dir_b.join("beta.star"));
    std::fs::write(&doc_a, "b = box(10, 10, 10)\n").unwrap();
    std::fs::write(&doc_b, "b = box(20, 20, 20)\n").unwrap();
    let mut leaks: Vec<String> = Vec::new();
    let mut tokens: Vec<String> = Vec::new();

    // Zero servers: a clear error, not a hang and not a port.
    let e = op_document(&bin, &runtime, &base).unwrap_err();
    assert!(e.contains("no running `cadpole serve`"), "{e}");

    // One server: found with no flags, over its socket.
    let a = Served::start(&bin, &runtime, &doc_a, &[]);
    let fa = a.files(&runtime);
    let record = std::fs::read_to_string(fa.record()).unwrap();
    leaks.push(record.clone());
    let r: crate::runtime::Record = serde_json::from_str(&record).unwrap();
    // In the runtime directory, or in the short private fallback when the
    // runtime directory's path is too long for a socket.
    let sock_a = r.socket.clone().expect("a socket");
    assert_eq!(sock_a, crate::runtime::default_socket_path(&fa).unwrap());
    assert_eq!(mode(&runtime), 0o700);
    assert_eq!(mode(sock_a.parent().unwrap()), 0o700);
    for p in [sock_a.clone(), fa.record(), fa.session_token(), fa.listener_token(), fa.url()] {
        assert_eq!(mode(&p), 0o600, "{}", p.display());
    }
    tokens.extend(tokens_of(&runtime, &a));
    assert!(r.http.as_deref().unwrap_or("").starts_with("http://127.0.0.1:"));
    assert!(!r.agent_tcp);
    assert_eq!(op_document(&bin, &runtime, &base).unwrap(), "alpha");

    let mut mcp = Mcp::start(&bin, &runtime, &base);
    assert_eq!(mcp.document().unwrap(), "alpha", "cadpole mcp with no flags reaches the one server");
    let args = std::fs::read(format!("/proc/{}/cmdline", mcp.child.id())).unwrap_or_default();
    leaks.push(String::from_utf8_lossy(&args).into_owned());

    // An agent token is refused on the TCP port by default.
    let agent_token = std::fs::read_to_string(fa.session_token()).unwrap();
    let status = ureq::Agent::config_builder().http_status_as_error(false).build();
    let resp = ureq::Agent::new_with_config(status)
        .post(&format!("{}/api/ops/GetSession", r.http.as_deref().unwrap()))
        .header("Authorization", &format!("Bearer {}", agent_token.trim()))
        .header("Content-Type", "application/json")
        .send("{}")
        .unwrap();
    assert_eq!(resp.status().as_u16(), 403, "an agent call over TCP is refused without --agent-tcp");

    // Two servers: no guess from an unrelated directory, the right one from
    // a document's own directory.
    let b = Served::start(&bin, &runtime, &doc_b, &[]);
    tokens.extend(tokens_of(&runtime, &b));
    let two = op_document(&bin, &runtime, &base).unwrap_err();
    assert!(two.contains("2 `cadpole serve` instances") && two.contains("--server unix:"), "{two}");
    assert_eq!(op_document(&bin, &runtime, &dir_b).unwrap(), "beta");
    assert_eq!(op_document(&bin, &runtime, &dir_a).unwrap(), "alpha");
    // The MCP client chose alpha before b started; it stays there.
    assert_eq!(mcp.document().unwrap(), "alpha", "a running client never moves to another document");

    // alpha's server stops cleanly: its files go with it, and the client says
    // so rather than moving to beta.
    let said_a = a.stop();
    leaks.extend(said_a);
    for p in [sock_a.clone(), fa.record(), fa.session_token(), fa.listener_token(), fa.url()] {
        assert!(!p.exists(), "{} outlived its server", p.display());
    }
    let gone = mcp.document().unwrap_err();
    assert!(gone.contains("no longer running") && gone.contains("alpha.star"), "{gone}");
    assert!(!gone.contains("beta"), "{gone}");

    // alpha comes back: new instance, new socket, new tokens. The same client
    // reconnects with no human step.
    let a2 = Served::start(&bin, &runtime, &doc_a, &[]);
    assert_ne!(a2.instance, fa.instance);
    tokens.extend(tokens_of(&runtime, &a2));
    assert_eq!(mcp.document().unwrap(), "alpha", "reconnected to the restarted server of the same document");

    // beta crashes: everything it wrote stays, and none of it is mistaken
    // for a live server.
    let fb = b.files(&runtime);
    let sock_b = crate::runtime::read_record(&fb.record()).unwrap().socket.unwrap();
    b.crash();
    assert!(sock_b.exists() && fb.record().exists());
    assert_eq!(op_document(&bin, &runtime, &base).unwrap(), "alpha", "a dead server is not a candidate");
    // The next server to start cleans it up, and leaves the live one alone.
    let b2 = Served::start(&bin, &runtime, &doc_b, &[]);
    assert!(!sock_b.exists() && !fb.record().exists(), "the crashed server's files are swept");
    let sock_a2 = crate::runtime::read_record(&a2.files(&runtime).record()).unwrap().socket.unwrap();
    assert!(sock_a2.exists(), "a live server's socket is never swept");
    tokens.extend(tokens_of(&runtime, &b2));

    // `--server unix:` and the old `--server http://` still work; the second
    // only with --agent-tcp.
    let o = Command::new(&bin)
        .args(["op", "GetSession", "--server"])
        .arg(format!("unix:{}", sock_a2.display()))
        .current_dir(&base)
        .env("CADPOLE_RUNTIME_DIR", &runtime)
        .env_remove("CADPOLE_TOKEN")
        .output()
        .unwrap();
    assert!(o.status.success(), "{}", String::from_utf8_lossy(&o.stderr));
    leaks.push(String::from_utf8_lossy(&o.stdout).into_owned());
    leaks.push(String::from_utf8_lossy(&o.stderr).into_owned());

    leaks.push(mcp.all_output());
    drop(mcp);
    leaks.extend(a2.stop());
    leaks.extend(b2.stop());
    let everything = leaks.join("\n");
    for t in &tokens {
        assert!(!everything.contains(t.as_str()), "a token reached a log, a record, a command line or the MCP output");
    }
    let _ = std::fs::remove_dir_all(&base);
}

/// The old URL form, with the server's consent: `--agent-tcp` and `--server
/// http://...` together, and a server with no socket at all.
#[test]
fn the_url_form_still_works_when_the_server_allows_it() {
    let Some(bin) = cadpole_binary() else { return };
    let base = scratch("url");
    let runtime = base.join("run");
    let doc = base.join("gamma.star");
    std::fs::write(&doc, "b = box(1, 2, 3)\n").unwrap();
    let s = Served::start(&bin, &runtime, &doc, &["--agent-tcp"]);
    let record: crate::runtime::Record = serde_json::from_slice(&std::fs::read(s.files(&runtime).record()).unwrap()).unwrap();
    assert!(record.agent_tcp);
    let o = Command::new(&bin)
        .args(["op", "GetSession", "--server", record.http.as_deref().unwrap()])
        .env("CADPOLE_RUNTIME_DIR", &runtime)
        .env_remove("CADPOLE_TOKEN")
        .output()
        .unwrap();
    assert!(o.status.success(), "{}", String::from_utf8_lossy(&o.stderr));
    let v: Value = serde_json::from_slice(&o.stdout).unwrap();
    assert_eq!(v["document"]["name"], "gamma");
    drop(s);
    let _ = std::fs::remove_dir_all(&base);
}

/// A runtime directory someone else could enter stops the server before it
/// binds anything.
#[test]
fn a_loose_runtime_directory_stops_the_server() {
    use std::os::unix::fs::PermissionsExt as _;
    let Some(bin) = cadpole_binary() else { return };
    let base = scratch("loose");
    let runtime = base.join("run");
    std::fs::create_dir_all(&runtime).unwrap();
    // Readable by others and not fixable by us? It is ours, so the server
    // tightens it; a directory it cannot tighten is refused. Simulate the
    // second with a symlink, which is never accepted.
    std::fs::set_permissions(&runtime, std::fs::Permissions::from_mode(0o755)).unwrap();
    let link = base.join("link");
    std::os::unix::fs::symlink(&runtime, &link).unwrap();
    let doc = base.join("d.star");
    std::fs::write(&doc, "b = box(1, 1, 1)\n").unwrap();
    let o = Command::new(&bin)
        .args(["serve", "--port", "0"])
        .arg(&doc)
        .env("CADPOLE_RUNTIME_DIR", &link)
        .env("CADPOLE_TOKEN", "")
        .stdin(Stdio::null())
        .output()
        .unwrap();
    assert!(!o.status.success());
    let e = String::from_utf8_lossy(&o.stderr);
    assert!(e.contains("symlink") || e.contains("not a directory"), "{e}");
    assert!(std::fs::read_dir(&runtime).unwrap().next().is_none(), "nothing was written");
    let _ = std::fs::remove_dir_all(&base);
}

/// A SIGTERM the moment the server says it is up still removes everything it
/// wrote. The handler used to be installed after the banner, and a stop sent
/// in that window took the default action: the process died with its socket,
/// record and tokens on disk. Found by the trace replay of
/// spec/transport.qnt; kept here as its counterexample.
#[test]
fn a_stop_right_after_the_banner_still_cleans_up() {
    let Some(bin) = cadpole_binary() else { return };
    let base = scratch("early-stop");
    let runtime = base.join("run");
    let doc = base.join("delta.star");
    std::fs::write(&doc, "b = box(1, 1, 1)\n").unwrap();
    for _ in 0..5 {
        let s = Served::start(&bin, &runtime, &doc, &[]);
        let files = s.files(&runtime);
        let sock = crate::runtime::read_record(&files.record()).unwrap().socket.unwrap();
        s.stop();
        for p in [sock, files.record(), files.session_token(), files.listener_token(), files.url()] {
            assert!(!p.exists(), "{} outlived a server stopped right after it started", p.display());
        }
    }
    let _ = std::fs::remove_dir_all(&base);
}
