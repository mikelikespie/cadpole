//! Fails when a dependency Cadpole ships has a known advisory against the exact
//! version the lockfile pins.
//!
//! A permissively-licensed crate with a known RCE is still a problem, so this
//! is the other half of `//build_support/licenses:license_test`. It reads:
//!
//!   * `Cargo.lock` and `pnpm-lock.yaml` -- what we actually resolve;
//!   * `rustsec.tsv` -- every advisory in the RustSec database, vendored whole
//!     from one pinned commit whose tarball sha256 is in the file header;
//!   * `npm.tsv` -- every advisory OSV knows about for the npm packages in the
//!     pnpm lockfile, at every version;
//!   * `accepted.tsv` -- the advisories that were reviewed and accepted,
//!     each with a reason.
//!
//! Everything is `include_str!`, so the test does no I/O and reaches no
//! network: the snapshots are refreshed by hand with
//! `build_support/advisories/refresh.py` and the diff is the review, exactly
//! as with `crates.tsv`.
//!
//! The version matching lives in `//build_support/lockfiles` and is tested
//! there against the range spellings RustSec actually uses; a matcher that
//! quietly said "patched" for everything would make this file pass while
//! proving nothing.

use lockfiles::{
    cargo_lock_packages, pnpm_lock_packages, satisfies_any, tsv_header, tsv_rows, Package, Version,
};

const CARGO_LOCK: &str = include_str!("../../Cargo.lock");
const PNPM_LOCK: &str = include_str!("../../pnpm-lock.yaml");
const RUSTSEC: &str = include_str!("rustsec.tsv");
const NPM: &str = include_str!("npm.tsv");
const ACCEPTED: &str = include_str!("accepted.tsv");

/// One row of `rustsec.tsv` / `npm.tsv`.
struct Advisory {
    id: String,
    package: String,
    kind: String,
    vulnerable: String,
    not_vulnerable: String,
    url: String,
    title: String,
}

fn advisories(text: &str) -> Vec<Advisory> {
    tsv_rows(text)
        .into_iter()
        .filter(|f| f.len() >= 7)
        .map(|f| Advisory {
            id: f[0].to_owned(),
            package: f[1].to_owned(),
            kind: f[2].to_owned(),
            vulnerable: f[3].to_owned(),
            not_vulnerable: f[4].to_owned(),
            url: f[5].to_owned(),
            title: f[6].to_owned(),
        })
        .collect()
}

/// `(advisory id, package)` pairs accepted in `accepted.tsv`, with the reason.
fn accepted() -> Vec<(String, String, String)> {
    tsv_rows(ACCEPTED)
        .into_iter()
        .filter(|f| f.len() >= 4)
        .map(|f| (f[0].to_owned(), f[1].to_owned(), f[3].to_owned()))
        .collect()
}

/// Does this advisory apply to this exact version?
fn applies(advisory: &Advisory, version: &str) -> bool {
    let v = Version::parse(version);
    satisfies_any(&v, &advisory.vulnerable) && !satisfies_any(&v, &advisory.not_vulnerable)
}

/// Every (advisory, package) pair that applies to something we ship.
fn live_matches(advisories: &[Advisory], packages: &[Package]) -> Vec<(String, String, String)> {
    let mut out = Vec::new();
    for a in advisories {
        for p in packages
            .iter()
            .filter(|p| p.third_party && p.name == a.package)
        {
            if applies(a, &p.version) {
                out.push((
                    a.id.clone(),
                    p.id(),
                    format!("[{}] {}  {}", a.kind, a.title, a.url),
                ));
            }
        }
    }
    out.sort();
    out
}

fn is_accepted(id: &str, package: &str) -> bool {
    let name = package.split(' ').next().unwrap_or(package);
    accepted().iter().any(|(i, p, _)| i == id && p == name)
}

fn report(kind: &str, unaccepted: &[(String, String, String)], refresh: &str) {
    let lines: Vec<String> = unaccepted
        .iter()
        .map(|(id, pkg, what)| format!("  {pkg}\n    {id}  {what}"))
        .collect();
    panic!(
        "{} {kind} dependencies have an advisory against the version we ship:\n{}\n\n\
         Fix the dependency, or -- if it does not reach anything Cadpole builds, or there is\n\
         no fixed release -- add a row to build_support/advisories/accepted.tsv with the\n\
         reason and the date you looked. Refresh the database with {refresh}.",
        unaccepted.len(),
        lines.join("\n")
    );
}

#[test]
fn no_crate_has_an_unaccepted_advisory() {
    let matches = live_matches(&advisories(RUSTSEC), &cargo_lock_packages(CARGO_LOCK));
    let unaccepted: Vec<_> = matches
        .into_iter()
        .filter(|(id, pkg, _)| !is_accepted(id, pkg))
        .collect();
    if !unaccepted.is_empty() {
        report(
            "Rust",
            &unaccepted,
            "build_support/advisories/refresh.py --rust",
        );
    }
}

#[test]
fn no_npm_package_has_an_unaccepted_advisory() {
    let matches = live_matches(&advisories(NPM), &pnpm_lock_packages(PNPM_LOCK));
    let unaccepted: Vec<_> = matches
        .into_iter()
        .filter(|(id, pkg, _)| !is_accepted(id, pkg))
        .collect();
    if !unaccepted.is_empty() {
        report(
            "npm",
            &unaccepted,
            "build_support/advisories/refresh.py --npm",
        );
    }
}

#[test]
fn the_npm_snapshot_covers_every_package_in_the_lockfile() {
    // Unlike the Rust database, the npm snapshot is taken one package at a
    // time, so a package nobody asked OSV about would silently have no
    // advisories. The header records the list that was asked about.
    let covered = tsv_header(NPM, "covered")
        .expect("build_support/advisories/npm.tsv has no `# covered:` header; regenerate it");
    let covered: Vec<&str> = covered.split_whitespace().collect();
    let missing: Vec<String> = pnpm_lock_packages(PNPM_LOCK)
        .iter()
        .filter(|p| !covered.contains(&p.name.as_str()))
        .map(|p| p.name.clone())
        .collect();
    assert!(
        missing.is_empty(),
        "pnpm-lock.yaml has packages the advisory snapshot was never taken for: {}\n\
         Run build_support/advisories/refresh.py --npm and commit the diff.",
        missing.join(", ")
    );
}

#[test]
fn no_accepted_row_is_stale() {
    // An acceptance that no longer matches anything is a lie about the
    // supply chain; delete it so the file keeps meaning what it says.
    let mut live: Vec<(String, String)> = Vec::new();
    for (advisories, packages) in [
        (advisories(RUSTSEC), cargo_lock_packages(CARGO_LOCK)),
        (advisories(NPM), pnpm_lock_packages(PNPM_LOCK)),
    ] {
        for (id, pkg, _) in live_matches(&advisories, &packages) {
            live.push((id, pkg.split(' ').next().unwrap_or(&pkg).to_owned()));
        }
    }
    let stale: Vec<String> = accepted()
        .iter()
        .filter(|(id, pkg, _)| !live.iter().any(|(i, p)| i == id && p == pkg))
        .map(|(id, pkg, _)| format!("{id} ({pkg})"))
        .collect();
    assert!(
        stale.is_empty(),
        "build_support/advisories/accepted.tsv accepts advisories that no longer apply: {}\n\
         Delete those rows.",
        stale.join(", ")
    );
}

#[test]
fn every_acceptance_carries_a_reason() {
    for (id, pkg, reason) in accepted() {
        assert!(
            reason.trim().len() > 20,
            "{id} ({pkg}) is accepted with no real reason: {reason:?}"
        );
    }
    // And the row shape itself: four columns, the third a date.
    for row in tsv_rows(ACCEPTED) {
        assert_eq!(
            row.len(),
            4,
            "accepted.tsv row has {} columns, want 4: {row:?}",
            row.len()
        );
        assert!(
            row[2].len() == 10 && row[2].chars().filter(|c| *c == '-').count() == 2,
            "accepted.tsv `reviewed` is not an ISO date: {:?}",
            row[2]
        );
    }
}

#[test]
fn the_snapshots_are_real() {
    // A truncated or empty snapshot would make every check above vacuous.
    let rust = advisories(RUSTSEC);
    assert!(
        rust.len() > 500,
        "rustsec.tsv holds only {} advisories",
        rust.len()
    );
    assert!(
        rust.iter().any(|a| a.package == "smallvec"),
        "rustsec.tsv does not know about smallvec, which has had five advisories"
    );
    // The header is the provenance: which commit, and the sha256 of the
    // tarball it came out of.
    let header: String = RUSTSEC.lines().take_while(|l| l.starts_with('#')).collect();
    assert!(
        header.contains("Commit:"),
        "rustsec.tsv has no upstream commit in its header"
    );
    assert!(
        header.contains("sha256:"),
        "rustsec.tsv has no tarball sha256 in its header"
    );
    assert!(
        !advisories(NPM).is_empty(),
        "npm.tsv holds no advisories at all"
    );
}

#[test]
fn the_matcher_catches_a_real_advisory() {
    // RUSTSEC-2026-0194 against quick-xml: fixed in 0.41.0, and the version
    // vtkio pins is 0.22.0. If this ever stops being a match the row in
    // accepted.tsv is stale and `no_accepted_row_is_stale` will say so -- but
    // an end-to-end case here is what proves the pipeline is wired up at all.
    let a = Advisory {
        id: "RUSTSEC-2026-0194".into(),
        package: "quick-xml".into(),
        kind: "vulnerability".into(),
        vulnerable: "*".into(),
        not_vulnerable: ">= 0.41.0".into(),
        url: String::new(),
        title: String::new(),
    };
    assert!(applies(&a, "0.22.0"));
    assert!(!applies(&a, "0.41.0"));
    assert!(!applies(&a, "0.42.1"));
}
