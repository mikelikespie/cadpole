//! `native_probe`: the determinism probe as a native binary.
//!
//! Reads one `cadpole_engine::probe::ProbeIn` JSON document on stdin, writes
//! the `ProbeOut` JSON on stdout. The wasm engine's `Engine.probe` is the
//! same function behind the same input; `native_vs_wasm_test.mjs` runs both
//! and diffs them. Test-only: `cadpole` is the user-facing binary.

use std::io::{Read, Write};

fn main() {
    let mut input = String::new();
    if let Err(e) = std::io::stdin().read_to_string(&mut input) {
        eprintln!("native_probe: cannot read stdin: {e}");
        std::process::exit(2);
    }
    match cadpole_engine::probe::probe_json(&input) {
        Ok(out) => {
            let mut stdout = std::io::stdout().lock();
            if stdout.write_all(out.as_bytes()).and_then(|_| stdout.flush()).is_err() {
                std::process::exit(2);
            }
        }
        Err(e) => {
            eprintln!("native_probe: {e}");
            std::process::exit(1);
        }
    }
}
