//! The command line and what an evaluation is given.

use super::*;

fn args(a: &[&str]) -> anyhow::Result<Args> {
    parse_args(a.iter().map(|s| s.to_string()).collect())
}

/// A flag that takes a value, given last without one, is a usage error
/// naming the flag -- not a panic.
#[test]
fn a_flag_without_its_value_is_a_usage_error() {
    for flag in ["--repeat", "--drag", "--param", "--size"] {
        let err = args(&["x.star", flag]).expect_err(flag).to_string();
        assert!(err.contains(flag) && err.contains("needs a value"), "{flag}: {err}");
    }
    let a = args(&["--repeat", "2", "--size", "64x48", "--param", "n=3", "x.star"]).unwrap();
    assert_eq!((a.repeat, a.size, a.files), (2, (64, 48), vec!["x.star".to_owned()]));
}

/// `--repeat 0` would print timings for runs that never happened.
#[test]
fn repeat_zero_is_refused() {
    let err = args(&["--repeat", "0", "x.star"]).expect_err("repeat 0").to_string();
    assert!(err.contains("--repeat"), "{err}");
}

/// A script that loads a sibling is given its siblings, as `cadpole eval`
/// gives them, so it is timed running rather than failing at its first line.
#[test]
fn a_script_that_loads_a_sibling_is_given_it() {
    let base = std::env::var("TEST_TMPDIR").map(std::path::PathBuf::from).unwrap_or_else(|_| std::env::temp_dir());
    let d = base.join(format!("cadpole-bench-load-{}", std::process::id()));
    let _ = std::fs::remove_dir_all(&d);
    std::fs::create_dir_all(d.join("lib")).unwrap();
    std::fs::write(d.join("parts.star"), "load(\"lib/sizes.star\", \"w\")\ndef part():\n    return box(w(), 2, 2)\n").unwrap();
    std::fs::write(d.join("lib/sizes.star"), "def w():\n    return 3\n").unwrap();
    let main = d.join("model.star");
    std::fs::write(&main, "load(\"parts.star\", \"part\")\nb = part()\n").unwrap();
    let scripts = scripts_for(main.to_str().unwrap()).unwrap();
    let ctx = cadpole_engine::cadpole_script::evaluate(EvalInput { scripts, ..Default::default() });
    let errors: Vec<_> = ctx
        .diagnostics
        .iter()
        .filter(|d| d.severity == cadpole_engine::cadpole_core::context::Severity::Error)
        .map(|d| d.message.clone())
        .collect();
    assert!(errors.is_empty(), "{errors:?}");
    assert_eq!(ctx.bodies.len(), 1);
    let _ = std::fs::remove_dir_all(&d);
}
