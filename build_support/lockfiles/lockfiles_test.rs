//! The lockfile parsers and the version algebra, checked against both real
//! lockfiles and against the exact range spellings the RustSec database uses.
//!
//! This matters more than it looks: the advisory test's answer is only as good
//! as `satisfies`, and a matcher that quietly said "patched" for everything
//! would make `//build_support/advisories:advisory_test` pass while proving
//! nothing.

use lockfiles::{
    cargo_lock_packages, pnpm_lock_packages, satisfies, satisfies_any, tsv_header, tsv_rows,
    Package, Version,
};

const CARGO_LOCK: &str = include_str!("../../Cargo.lock");
const PNPM_LOCK: &str = include_str!("../../pnpm-lock.yaml");

fn v(s: &str) -> Version {
    Version::parse(s)
}

#[test]
fn the_cargo_lockfile_parses() {
    let packages = cargo_lock_packages(CARGO_LOCK);
    assert!(
        packages.len() > 100,
        "only {} packages parsed out of Cargo.lock",
        packages.len()
    );
    assert!(packages.iter().any(|p| p.name == "serde" && p.third_party));
    // Cadpole's own crates have no `source`, which is how they are recognised.
    let ours: Vec<&Package> = packages.iter().filter(|p| !p.third_party).collect();
    assert!(
        ours.iter().any(|p| p.name == "cadpole-core"),
        "cadpole-core is not a workspace member?"
    );
    assert!(
        ours.iter().all(|p| p.name.starts_with("cadpole-")),
        "a crate with no source is not one of ours: {:?}",
        ours.iter()
            .filter(|p| !p.name.starts_with("cadpole-"))
            .collect::<Vec<_>>()
    );
    // No duplicate (name, version).
    let mut ids: Vec<String> = packages.iter().map(Package::id).collect();
    ids.sort();
    let before = ids.len();
    ids.dedup();
    assert_eq!(before, ids.len(), "Cargo.lock parsed with duplicate rows");
}

#[test]
fn the_pnpm_lockfile_parses() {
    let packages = pnpm_lock_packages(PNPM_LOCK);
    assert!(
        !packages.is_empty(),
        "no npm packages parsed out of pnpm-lock.yaml"
    );
    assert!(
        packages.iter().any(|p| p.name == "three"),
        "three is a dependency of //src/web but did not parse: {packages:?}"
    );
    assert!(
        packages.iter().any(|p| p.name == "@types/three"),
        "a scoped package name did not parse: {packages:?}"
    );
    // The version must not have swallowed the scope's `@`.
    for p in &packages {
        assert!(!p.name.is_empty() && !p.version.is_empty(), "bad row {p:?}");
        assert!(
            !p.version.contains('@'),
            "version of {} kept an @: {}",
            p.name,
            p.version
        );
        assert!(
            p.version.chars().next().unwrap().is_ascii_digit(),
            "bad version for {p:?}"
        );
    }
    // `importers:` entries (our own workspace packages) must not leak in.
    assert!(!packages.iter().any(|p| p.name == "src/web"));
}

#[test]
fn tsv_parsing_drops_comments_and_the_header() {
    let text = "# a comment\n# another\nname\tversion\tlicense\nfoo\t1.0\tMIT\n\nbar\t2.0\tISC\n";
    let rows = tsv_rows(text);
    assert_eq!(
        rows,
        vec![vec!["foo", "1.0", "MIT"], vec!["bar", "2.0", "ISC"]]
    );
    let text = "# covered: a b c\nid\tpackage\n";
    assert_eq!(tsv_header(text, "covered").as_deref(), Some("a b c"));
    assert_eq!(tsv_header(text, "nope"), None);
}

#[test]
fn versions_order_the_way_semver_says() {
    assert!(v("1.2.3") < v("1.2.4"));
    assert!(v("1.2.3") < v("1.3.0"));
    assert!(v("0.9.9") < v("1.0.0"));
    assert!(v("1.0.0-rc.1") < v("1.0.0"));
    assert!(v("1.0.0-alpha") < v("1.0.0-beta"));
    // Build metadata is not part of the order.
    assert_eq!(v("0.38.0+1.3.281"), v("0.38.0"));
    // Missing components are zero, and the precision is remembered.
    assert_eq!(v("1"), v("1.0.0"));
    assert_eq!(v("1").precision, 1);
    assert_eq!(v("1.2").precision, 2);
}

#[test]
fn comparators_mean_what_cargo_means() {
    assert!(satisfies(&v("1.2.3"), ">= 1.0.0"));
    assert!(!satisfies(&v("1.2.3"), "> 1.2.3"));
    assert!(satisfies(&v("1.2.3"), ">= 1.2.3"));
    assert!(satisfies(&v("0.9.9"), "< 1.0.0"));
    assert!(satisfies(&v("0.9.9"), "*"));
    // RustSec writes `>=0.4.20` with no space, and `^ 0.3.1` with one.
    assert!(satisfies(&v("0.4.45"), ">=0.4.20"));
    assert!(satisfies(&v("0.3.4"), "^ 0.3.1"));
    // Caret, all eight Cargo cases.
    assert!(satisfies(&v("1.9.0"), "^1.2.3") && !satisfies(&v("2.0.0"), "^1.2.3"));
    assert!(satisfies(&v("1.9.0"), "^1") && !satisfies(&v("2.0.0"), "^1"));
    assert!(satisfies(&v("0.2.9"), "^0.2.3") && !satisfies(&v("0.3.0"), "^0.2.3"));
    assert!(satisfies(&v("0.2.9"), "^0.2") && !satisfies(&v("0.3.0"), "^0.2"));
    assert!(satisfies(&v("0.0.3"), "^0.0.3") && !satisfies(&v("0.0.4"), "^0.0.3"));
    assert!(satisfies(&v("0.0.9"), "^0.0") && !satisfies(&v("0.1.0"), "^0.0"));
    assert!(satisfies(&v("0.9.0"), "^0") && !satisfies(&v("1.0.0"), "^0"));
    // A bare version is a caret, which is how `patched = ["0.13.10"]` reads.
    assert!(satisfies(&v("0.13.11"), "0.13.10") && !satisfies(&v("0.14.0"), "0.13.10"));
    // Tilde.
    assert!(satisfies(&v("1.2.9"), "~1.2.3") && !satisfies(&v("1.3.0"), "~1.2.3"));
    assert!(satisfies(&v("1.9.0"), "~1") && !satisfies(&v("2.0.0"), "~1"));
    // Equality, including the partial form RustSec uses for `unaffected`.
    assert!(satisfies(&v("0.2.4"), "=0.2.4") && !satisfies(&v("0.2.5"), "=0.2.4"));
    assert!(satisfies(&v("0.2.9"), "=0.2") && !satisfies(&v("0.3.0"), "=0.2"));
    // `>= 0` is how a withdrawn advisory says "no version is affected".
    assert!(satisfies(&v("0.9.9"), ">= 0"));
}

#[test]
fn a_range_is_a_conjunction_and_a_field_is_a_disjunction() {
    assert!(satisfies(&v("0.9.5"), "< 0.10.0, >= 0.9.3"));
    assert!(!satisfies(&v("0.10.0"), "< 0.10.0, >= 0.9.3"));
    assert!(!satisfies(&v("0.9.2"), "< 0.10.0, >= 0.9.3"));
    // The real shape of RUSTSEC-2026-0097's `patched`.
    let patched = ">= 0.10.1;< 0.10.0, >= 0.9.3;< 0.9.0, >= 0.8.6";
    assert!(satisfies_any(&v("0.10.2"), patched));
    assert!(satisfies_any(&v("0.9.5"), patched));
    assert!(satisfies_any(&v("0.8.8"), patched));
    assert!(!satisfies_any(&v("0.8.5"), patched));
    // An empty field matches nothing: `patched = []` means nothing is fixed.
    assert!(!satisfies_any(&v("1.0.0"), ""));
}
