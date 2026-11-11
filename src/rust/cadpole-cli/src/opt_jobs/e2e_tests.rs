//! Solve jobs end to end, on the real binary: `cadpole serve` with its default 20-s
//! evaluation budget, a market split that takes several seconds to solve
//! (well past the 0.5-s inline window it is given), an evaluation that comes
//! back pending, the job that lands, and the evaluation after it that reads
//! the answer -- the same answer an evaluation with time enough computes
//! inline.

use std::io::{BufRead, BufReader};
use std::path::Path;
use std::process::{Child, Command, Stdio};
use std::time::{Duration, Instant};

use serde_json::{Value, json};

use super::tests::{cadpole_binary, scratch};

/// The market split of `cadpole-opt`'s tests, as a script: 3 rows and 24
/// binaries, seed 11 -- 88,626 nodes, the optimum a miss of 1.
pub(crate) const MARKET: &str = r#"
ROWS = int(param("rows", 3))
N = int(param("n", 24))
SEED = int(param("seed", 11))

def market(rows, n, seed):
    p = opt.problem(name = "market")
    xs = [p.binary("x%d" % j) for j in range(n)]
    state = seed
    miss = []
    for i in range(rows):
        a = []
        for j in range(n):
            state = (state * 1103515245 + 12345) % 2147483648
            a.append((state // 65536) % 100)
        total = 0
        for v in a:
            total += v
        over = p.var("over%d" % i, min = 0)
        under = p.var("under%d" % i, min = 0)
        p.constrain(opt.dot(a, xs) + over - under, equals = total // 2, name = "market %d" % i)
        miss.append(over)
        miss.append(under)
    p.minimize(opt.sum(miss))
    return p, xs

p, xs = market(ROWS, N, SEED)
sol = p.solve()
if sol.ok:
    print("answer", sol.status, sol.objective, sol.nodes, [sol.value(x) for x in xs])
else:
    print("no answer yet:", sol.status)
"#;

/// `cadpole serve`, stopped when dropped.
struct Served {
    child: Child,
    port: u16,
}

impl Drop for Served {
    fn drop(&mut self) {
        let _ = self.child.kill();
        let _ = self.child.wait();
    }
}

fn serve(cadpole: &Path, doc: &Path, cache: &Path) -> Served {
    let mut child = Command::new(cadpole)
        .args(["serve", "--no-auth", "--port", "0", "--eval-deadline-ms", "20000", "--solve-inline-ms", "500"])
        .arg(doc)
        .env("CADPOLE_OPT_CACHE", cache)
        // Its socket and record go beside the cache, not into the user's
        // runtime directory.
        .env("CADPOLE_RUNTIME_DIR", cache.with_extension("run"))
        .stdin(Stdio::null())
        .stdout(Stdio::null())
        .stderr(Stdio::piped())
        .spawn()
        .expect("cadpole serve starts");
    let mut lines = BufReader::new(child.stderr.take().unwrap()).lines();
    let mut port = None;
    for line in lines.by_ref() {
        let line = line.unwrap_or_default();
        if let Some(rest) = line.strip_prefix("cadpole serve listening on http://127.0.0.1:") {
            port = rest.trim_end_matches('/').split(|c: char| !c.is_ascii_digit()).next().and_then(|p| p.parse().ok());
            break;
        }
    }
    // Keep reading, so the server never blocks on a full pipe.
    std::thread::spawn(move || for _ in lines {});
    Served { child, port: port.expect("cadpole serve said where it listens") }
}

fn op(port: u16, name: &str, body: Value, timeout: Duration) -> Value {
    let agent = ureq::Agent::new_with_defaults();
    let mut resp = agent
        .post(&format!("http://127.0.0.1:{port}/api/ops/{name}"))
        .config()
        .timeout_global(Some(timeout))
        .build()
        .header("Content-Type", "application/json")
        .send_json(&body)
        .unwrap_or_else(|e| panic!("{name}: {e}"));
    resp.body_mut().read_json::<Value>().unwrap()
}

fn messages(model: &Value) -> Vec<String> {
    model["model"]["diagnostics"].as_array().into_iter().flatten().filter_map(|d| d["message"].as_str().map(str::to_owned)).collect()
}

fn answer_line(messages: &[String]) -> Option<String> {
    messages.iter().find_map(|m| m.strip_prefix("print: answer ").map(str::to_owned))
}

#[test]
fn a_solve_past_serves_budget_goes_pending_and_lands() {
    let Some(cadpole) = cadpole_binary() else {
        eprintln!("skipped: no cadpole binary in the runfiles");
        return;
    };
    let root = scratch("e2e");
    std::fs::create_dir_all(&root).unwrap();
    let doc = root.join("main.star");
    std::fs::write(&doc, MARKET).unwrap();
    let cache = root.join("cache");

    // The answer with time enough, computed inline in another process at
    // the same time: no cache, no jobs, the CLI's 120 s.
    let inline = {
        let (cadpole, doc) = (cadpole.clone(), doc.clone());
        std::thread::spawn(move || {
            let out = Command::new(&cadpole).arg("eval").arg(&doc).env("CADPOLE_OPT_CACHE", "off").output().expect("cadpole eval runs");
            let text = format!("{}{}", String::from_utf8_lossy(&out.stdout), String::from_utf8_lossy(&out.stderr));
            text.lines().find_map(|l| l.trim().strip_prefix("info: print: answer ").map(str::to_owned))
        })
    };

    let served = serve(&cadpole, &doc, &cache);
    let clock = Instant::now();
    let first = op(served.port, "Evaluate", json!({}), Duration::from_secs(60));
    let took = clock.elapsed();
    let msgs = messages(&first);
    let jobs = super::pending_jobs(msgs.iter().map(String::as_str));
    assert_eq!(jobs.len(), 1, "one pending solve, naming its job: {msgs:#?}");
    let job = jobs[0].clone();
    assert!(msgs.iter().any(|m| m == "print: no answer yet: pending"), "{msgs:#?}");
    assert!(took < Duration::from_secs(20), "the evaluation came back inside its budget, in {took:?}");
    eprintln!("e2e: the first evaluation came back pending, naming job {job}, in {took:?}");
    let landing_clock = Instant::now();
    assert!(answer_line(&msgs).is_none());

    // The job lands; serve evaluates the document again and says so.
    let mut finished = None;
    let mut started = false;
    let deadline = Instant::now() + Duration::from_secs(600);
    let mut after = 0;
    while finished.is_none() && Instant::now() < deadline {
        let r = op(
            served.port,
            "WaitEvents",
            json!({"after": after, "kinds": ["solve_started", "solve_finished"], "timeout_ms": 60000}),
            Duration::from_secs(90),
        );
        after = r["cursor"].as_u64().unwrap_or(after);
        for e in r["events"].as_array().into_iter().flatten() {
            let data: Value = match &e["data"] {
                Value::String(s) => serde_json::from_str(s).unwrap_or(Value::Null),
                v => v.clone(),
            };
            if data["job"] != json!(job) {
                continue;
            }
            match e["kind"].as_str() {
                Some("solve_started") => started = true,
                Some("solve_finished") => finished = Some(data),
                _ => {}
            }
        }
    }
    let finished = finished.expect("solve_finished for the job within ten minutes");
    eprintln!("e2e: solve_finished {:?} after the pending evaluation: {finished}", landing_clock.elapsed());
    assert!(started, "solve_started came first");
    assert_eq!(finished["state"], "done", "{finished}");
    assert_eq!(finished["status"], "optimal", "{finished}");
    assert_eq!(finished["pending"], json!([]), "nothing pending after the re-evaluation: {finished}");

    // The next evaluation -- the viewer's, on that event -- has the answer.
    let clock = Instant::now();
    let after_landing = op(served.port, "Evaluate", json!({}), Duration::from_secs(60));
    let msgs = messages(&after_landing);
    let answer = answer_line(&msgs).unwrap_or_else(|| panic!("an answer after the job landed: {msgs:#?}"));
    assert!(answer.starts_with("optimal "), "{answer}");
    assert!(clock.elapsed() < Duration::from_secs(5), "read from the memo and the cache, not solved: {:?}", clock.elapsed());
    eprintln!("e2e: the evaluation after the landing took {:?}: {answer}", clock.elapsed());
    assert!(super::pending_jobs(msgs.iter().map(String::as_str)).is_empty());

    // The same answer, to the last digit and node, as the inline solve.
    let inline = inline.join().unwrap().expect("cadpole eval solved it inline");
    assert_eq!(answer, inline, "a job's answer is the inline one");

    // ...and the CLI reads serve's job's answer from the shared cache.
    let out = Command::new(&cadpole).arg("eval").arg(&doc).env("CADPOLE_OPT_CACHE", &cache).output().unwrap();
    let err = String::from_utf8_lossy(&out.stderr);
    assert!(err.contains("opt: 1 answer from the cache, 0 solves here"), "{err}");
    drop(served);
    let _ = std::fs::remove_dir_all(&root);
}
