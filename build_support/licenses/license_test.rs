//! Fails when a dependency appears whose licence Cadpole cannot ship under.
//!
//! Cadpole is Apache-2.0 and its dependencies are permissive-only: no GPL, LGPL,
//! MPL, CDDL, EPL or SSPL. Everything Cadpole distributes is covered, in the same
//! way and against the same allowlist:
//!
//! * **crates** -- `Cargo.lock` against `crates.tsv`, which `refresh.py`
//!   records from each crate's own `Cargo.toml` in the Bazel cache;
//! * **npm packages** -- `pnpm-lock.yaml` against `npm.tsv`, recorded the same
//!   way from each package's own `package.json`. The web UI is not a dev tool:
//!   `//src/web:dist` is embedded in the CLI binary and served by
//!   `cadpole serve`, so a three.js dependency is as binding as a crate;
//! * **font data** -- `fonts.tsv`, reviewed separately because a font is not
//!   linked code, under the `fonts-only:` terms of `allowed.txt`;
//! * **C and C++ code from Bazel modules** -- `native.tsv`, checked
//!   against the version MODULE.bazel.lock resolves and against what the
//!   shipped artifacts actually link (`native_code.bzl`'s aspect), since no
//!   lockfile lists it.
//!
//! Each of those asks two questions: is every resolved package *recorded*, so
//! a new dependency cannot arrive without someone writing down its licence,
//! and does every recorded licence offer a term we allow.
//!
//! This exists because `ureq`'s default TLS stack silently pulled in
//! `webpki-roots` (MPL-2.0) and `ring`.
//!
//! Advisories are the other half of the supply-chain policy and live in
//! `//build_support/advisories:advisory_test`.

use lockfiles::{cargo_lock_packages, pnpm_lock_packages, tsv_rows, Package};

const CARGO_LOCK: &str = include_str!("../../Cargo.lock");
const PNPM_LOCK: &str = include_str!("../../pnpm-lock.yaml");
const CRATES: &str = include_str!("crates.tsv");
const NPM: &str = include_str!("npm.tsv");
const ALLOWED: &str = include_str!("allowed.txt");
const FONTS: &str = include_str!("fonts.tsv");
const TEXTS: &str = include_str!("texts.tsv");
const NATIVE: &str = include_str!("native.tsv");
const MODULE_LOCK: &str = include_str!("../../MODULE.bazel.lock");

/// One line per external repository whose C or C++ libraries a shipped
/// artifact links, from `native_code.bzl`'s aspect over the CLI, the C FFI
/// library and the wasm module. Read at run time from the test's
/// runfiles: a generated file in `compile_data` would move this crate's
/// sources into `bazel-out`, where the relative `include_str!`s above break.
fn native_code() -> String {
    let path = std::env::var("CADPOLE_NATIVE_CODE").expect("CADPOLE_NATIVE_CODE names the native_code report");
    std::fs::read_to_string(&path).unwrap_or_else(|e| panic!("{path}: {e}"))
}

/// Terms Cadpole may link code under.
fn allowed_terms() -> Vec<String> {
    ALLOWED
        .lines()
        .map(str::trim)
        .filter(|l| !l.is_empty() && !l.starts_with('#') && !l.starts_with("fonts-only:"))
        .map(|l| l.to_ascii_lowercase())
        .collect()
}

/// Terms a bundled font file may carry: everything code may carry, plus the
/// data-only exceptions.
fn font_terms() -> Vec<String> {
    let mut terms = allowed_terms();
    terms.extend(
        ALLOWED
            .lines()
            .map(str::trim)
            .filter_map(|l| l.strip_prefix("fonts-only:"))
            .map(|l| l.trim().to_ascii_lowercase()),
    );
    terms
}

// ------------------------------------------------------------ SPDX parsing

/// An SPDX licence expression, parsed rather than pattern-matched.
///
/// The shape matters for the answer: `OR` offers a choice, so one acceptable
/// branch is enough, while `AND` imposes every part at once, so all of them
/// must be acceptable. Precedence is SPDX's -- `AND` binds tighter than `OR` --
/// and parentheses override it. Cargo's legacy `MIT/Apache-2.0` spelling means
/// `OR`.
///
/// Getting this from a parser instead of from `split(" or ")` is not pedantry:
/// the split version reads `(MIT OR Apache-2.0) AND GPL-3.0` as "MIT is on
/// offer, fine" and lets a copyleft `AND` term through. `wasm-bindgen`'s
/// `(MIT OR Apache-2.0) AND Unicode-3.0` is the shape that exposed it.
enum Expr {
    Term(String),
    Or(Vec<Expr>),
    And(Vec<Expr>),
}

struct Parser<'a> {
    tokens: Vec<&'a str>,
    at: usize,
}

/// `MIT OR Apache-2.0` -> ["mit", "or", "apache-2.0"]. Parentheses are their
/// own tokens; `/` is `OR`; `WITH` stays inside the term it qualifies.
fn tokenize(expr: &str) -> Vec<String> {
    let mut out = Vec::new();
    let mut word = String::new();
    for c in expr.chars() {
        match c {
            '(' | ')' | '/' => {
                if !word.trim().is_empty() {
                    out.push(word.trim().to_ascii_lowercase());
                }
                word.clear();
                out.push(if c == '/' {
                    "or".to_owned()
                } else {
                    c.to_string()
                });
            }
            c if c.is_whitespace() => {
                if !word.trim().is_empty() {
                    out.push(word.trim().to_ascii_lowercase());
                }
                word.clear();
            }
            c => word.push(c),
        }
    }
    if !word.trim().is_empty() {
        out.push(word.trim().to_ascii_lowercase());
    }
    out
}

impl<'a> Parser<'a> {
    fn peek(&self) -> Option<&'a str> {
        self.tokens.get(self.at).copied()
    }

    fn bump(&mut self) -> Option<&'a str> {
        let t = self.peek();
        self.at += 1;
        t
    }

    /// expr := and ( "or" and )*
    fn expr(&mut self) -> Expr {
        let mut parts = vec![self.and()];
        while self.peek() == Some("or") {
            self.at += 1;
            parts.push(self.and());
        }
        if parts.len() == 1 {
            parts.pop().unwrap()
        } else {
            Expr::Or(parts)
        }
    }

    /// and := primary ( "and" primary )*
    fn and(&mut self) -> Expr {
        let mut parts = vec![self.primary()];
        while self.peek() == Some("and") {
            self.at += 1;
            parts.push(self.primary());
        }
        if parts.len() == 1 {
            parts.pop().unwrap()
        } else {
            Expr::And(parts)
        }
    }

    /// primary := "(" expr ")" | id [ "with" exception ]
    fn primary(&mut self) -> Expr {
        if self.peek() == Some("(") {
            self.at += 1;
            let inner = self.expr();
            if self.peek() == Some(")") {
                self.at += 1;
            }
            return inner;
        }
        let Some(id) = self.bump() else {
            return Expr::Term(String::new());
        };
        // `Apache-2.0 WITH LLVM-exception` is one term: the exception changes
        // what the licence permits, so the pair is what allowed.txt lists.
        if self.peek() == Some("with") {
            self.at += 1;
            let exception = self.bump().unwrap_or("");
            return Expr::Term(format!("{id} with {exception}"));
        }
        Expr::Term(id.to_owned())
    }
}

fn evaluate(expr: &Expr, allowed: &[String]) -> bool {
    match expr {
        Expr::Term(t) => allowed.iter().any(|a| a == t),
        Expr::Or(parts) => parts.iter().any(|p| evaluate(p, allowed)),
        Expr::And(parts) => parts.iter().all(|p| evaluate(p, allowed)),
    }
}

/// Does this SPDX expression offer terms Cadpole can ship under?
fn is_allowed(expr: &str, allowed: &[String]) -> bool {
    let tokens = tokenize(expr);
    if tokens.is_empty() {
        return false;
    }
    let refs: Vec<&str> = tokens.iter().map(String::as_str).collect();
    let mut parser = Parser {
        tokens: refs,
        at: 0,
    };
    let parsed = parser.expr();
    parser.at == parser.tokens.len() && evaluate(&parsed, allowed)
}

// ------------------------------------------------------------- inventories

/// (name, version) -> licence expression, for one inventory file.
fn recorded(tsv: &str) -> Vec<((String, String), String)> {
    tsv_rows(tsv)
        .into_iter()
        .filter(|f| f.len() >= 3)
        .map(|f| ((f[0].to_owned(), f[1].to_owned()), f[2].to_owned()))
        .collect()
}

/// Rows of `fonts.tsv`: name, version, licence, reserved font names, path,
/// licence path.
fn bundled_fonts() -> Vec<[String; 6]> {
    tsv_rows(FONTS)
        .into_iter()
        .filter(|f| f.len() >= 6)
        .map(|f| {
            [
                f[0].to_owned(),
                f[1].to_owned(),
                f[2].to_owned(),
                f[3].to_owned(),
                f[4].to_owned(),
                f[5].to_owned(),
            ]
        })
        .collect()
}

/// Shared shape of the two "is everything written down" checks.
fn assert_every_package_is_recorded(what: &str, packages: &[Package], tsv: &str, refresh: &str) {
    let recorded = recorded(tsv);
    let missing: Vec<String> = packages
        .iter()
        .filter(|p| {
            !recorded
                .iter()
                .any(|((n, v), _)| *n == p.name && *v == p.version)
        })
        .map(Package::id)
        .collect();
    assert!(
        missing.is_empty(),
        "{} {what} dependencies are not recorded: {}\nRun {refresh} and review the diff.",
        missing.len(),
        missing.join(", ")
    );
}

/// Shared shape of the two "is every licence acceptable" checks. Only packages
/// we actually resolve matter; the inventory may keep history.
fn assert_every_recorded_licence_is_allowed(what: &str, packages: &[Package], tsv: &str) {
    let allowed = allowed_terms();
    let bad: Vec<String> = recorded(tsv)
        .into_iter()
        .filter(|((n, v), _)| packages.iter().any(|p| p.name == *n && p.version == *v))
        .filter(|(_, lic)| !is_allowed(lic, &allowed))
        .map(|((n, v), lic)| format!("{n} {v}: {lic}"))
        .collect();
    assert!(
        bad.is_empty(),
        "{what} dependencies whose licence is not permitted by \
         build_support/licenses/allowed.txt:\n  {}",
        bad.join("\n  ")
    );
}

// ------------------------------------------------------------------- crates

#[test]
fn every_crate_has_a_recorded_licence() {
    assert_every_package_is_recorded(
        "Rust",
        &cargo_lock_packages(CARGO_LOCK),
        CRATES,
        "build_support/licenses/refresh.py --crates",
    );
}

#[test]
fn every_crate_licence_is_one_we_can_ship_under() {
    assert_every_recorded_licence_is_allowed("Rust", &cargo_lock_packages(CARGO_LOCK), CRATES);
}

#[test]
fn cadpoles_own_crates_are_recorded_as_apache() {
    // A crate with no `source` in Cargo.lock is one of ours. `refresh.py` used
    // to decide that from a hand-kept list, and a crate left off it was looked
    // up on crates.io -- where it does not exist -- and recorded as UNKNOWN.
    // Deriving it from the lockfile on both sides makes that impossible.
    let recorded = recorded(CRATES);
    let ours: Vec<Package> = cargo_lock_packages(CARGO_LOCK)
        .into_iter()
        .filter(|p| !p.third_party)
        .collect();
    assert!(
        ours.len() >= 10,
        "only {} workspace crates found in Cargo.lock?",
        ours.len()
    );
    for p in &ours {
        assert!(
            p.name.starts_with("cadpole-"),
            "{} has no `source` in Cargo.lock but is not one of Cadpole's crates",
            p.id()
        );
        let lic = recorded
            .iter()
            .find(|((n, v), _)| *n == p.name && *v == p.version)
            .map(|(_, l)| l.as_str());
        assert_eq!(
            lic,
            Some("Apache-2.0"),
            "{} is a Cadpole crate but crates.tsv records {lic:?}; run \
             build_support/licenses/refresh.py",
            p.id()
        );
    }
}

// ---------------------------------------------------------------------- npm

#[test]
fn every_npm_package_has_a_recorded_licence() {
    assert_every_package_is_recorded(
        "npm",
        &pnpm_lock_packages(PNPM_LOCK),
        NPM,
        "build_support/licenses/refresh.py --npm",
    );
}

#[test]
fn every_npm_licence_is_one_we_can_ship_under() {
    assert_every_recorded_licence_is_allowed("npm", &pnpm_lock_packages(PNPM_LOCK), NPM);
}

#[test]
fn the_npm_inventory_is_not_empty() {
    // //src/web has real dependencies; an empty inventory would make both npm
    // checks vacuously true, which is exactly the hole this closes.
    let packages = pnpm_lock_packages(PNPM_LOCK);
    assert!(
        packages.len() >= 5,
        "only {} npm packages parsed?",
        packages.len()
    );
    assert!(!recorded(NPM).is_empty(), "npm.tsv records nothing");
}

// -------------------------------------------------------------------- fonts

#[test]
fn every_bundled_font_is_recorded_and_permitted() {
    let terms = font_terms();
    let fonts = bundled_fonts();
    assert!(
        !fonts.is_empty(),
        "fonts.tsv records no fonts, but cadpole-font bundles one"
    );
    for [name, version, lic, rfn, path, license_path] in fonts {
        assert!(
            is_allowed(&lic, &terms),
            "bundled font {name} {version} is {lic}, which build_support/licenses/allowed.txt \
             does not permit even for font data"
        );
        // Cadpole reserves the right to subset a bundled face, and under the OFL
        // a subset of a font with Reserved Font Names has to be renamed.
        assert_eq!(
            rfn.to_ascii_lowercase(),
            "none",
            "bundled font {name} declares Reserved Font Names ({rfn}); pick a face that does not, \
             or commit to never subsetting it"
        );
        assert!(!path.is_empty(), "{name} has no path in fonts.tsv");
        assert!(
            !license_path.is_empty(),
            "{name} has no licence file in fonts.tsv; the OFL requires the text to ship with the font"
        );
    }
}

#[test]
fn the_font_exception_never_reaches_a_crate() {
    // OFL-1.1 is acceptable for a font file and for nothing that is linked.
    assert!(
        !is_allowed("OFL-1.1", &allowed_terms()),
        "no crate may be OFL-1.1"
    );
    assert!(
        is_allowed("OFL-1.1", &font_terms()),
        "a bundled font may be OFL-1.1"
    );
    // The marker line itself must not be read as an ordinary term.
    assert!(!allowed_terms().iter().any(|t| t.contains("fonts-only")));
    assert!(font_terms().iter().any(|t| t == "ofl-1.1"));
}

// -------------------------------------------------------------- native code

/// Rows of `native.tsv`: (name, version, licence, module@version).
fn native_rows() -> Vec<[String; 4]> {
    tsv_rows(NATIVE)
        .into_iter()
        .filter(|f| f.len() >= 5)
        .map(|f| [f[0].to_owned(), f[1].to_owned(), f[2].to_owned(), f[4].to_owned()])
        .collect()
}

/// The module versions the graph resolves: MODULE.bazel.lock records the
/// `source.json` of exactly the selected version of every module (the other
/// versions it consulted appear by their MODULE.bazel only).
fn resolved_modules() -> Vec<String> {
    MODULE_LOCK
        .lines()
        .filter_map(|l| {
            let rest = l.trim().strip_prefix("\"https://bcr.bazel.build/modules/")?;
            let (path, _) = rest.split_once('"')?;
            let mut parts = path.split('/');
            let (name, version, file) = (parts.next()?, parts.next()?, parts.next()?);
            (file == "source.json").then(|| format!("{name}@{version}"))
        })
        .collect()
}

/// The modules whose C or C++ code a shipped artifact links. A repository's
/// canonical name starts with its module's (`highs+`; `mod++ext+name` for one
/// a module's extension made).
fn linked_native_modules() -> Vec<String> {
    let mut out: Vec<String> = native_code()
        .lines()
        .map(str::trim)
        .filter(|l| !l.is_empty())
        .map(|repo| repo.split('+').next().unwrap_or(repo).to_owned())
        .collect();
    out.dedup();
    out
}

#[test]
fn every_native_licence_is_one_we_can_ship_under() {
    let allowed = allowed_terms();
    let rows = native_rows();
    assert!(!rows.is_empty(), "native.tsv records nothing, but the CLI links HiGHS");
    for [name, version, lic, _] in rows {
        assert!(
            is_allowed(&lic, &allowed),
            "{name} {version} is {lic}, which build_support/licenses/allowed.txt does not permit"
        );
    }
}

#[test]
fn every_native_module_is_at_the_version_the_graph_resolves() {
    // A module bumped in MODULE.bazel without native.tsv reviewed fails here.
    let resolved = resolved_modules();
    assert!(resolved.len() > 20, "only {} resolved modules in MODULE.bazel.lock?", resolved.len());
    for [name, _, _, module] in native_rows() {
        assert!(
            resolved.contains(&module),
            "native.tsv records {name} from {module}, but MODULE.bazel.lock resolves {}",
            resolved
                .iter()
                .filter(|m| m.split('@').next() == module.split('@').next())
                .cloned()
                .collect::<Vec<_>>()
                .join(", ")
        );
    }
}

#[test]
fn every_linked_native_library_is_recorded_and_nothing_else() {
    // native_code.bzl walks the CLI, the C FFI library and the wasm module.
    // A module whose C/C++ code arrives without a row fails the first
    // assert; a row for code nothing links any more fails the second.
    let linked = linked_native_modules();
    assert!(linked.iter().any(|m| m == "highs"), "the aspect found no HiGHS in the CLI, so it is not looking: {linked:?}");
    let recorded: Vec<String> = native_rows().into_iter().map(|[_, _, _, m]| m.split('@').next().unwrap_or("").to_owned()).collect();
    let missing: Vec<&String> = linked.iter().filter(|m| !recorded.contains(m)).collect();
    assert!(
        missing.is_empty(),
        "these modules' C or C++ code is linked into a shipped artifact and native.tsv does not record it: {missing:?}\n\
         Review the licences of what each compiles and add a row per licence."
    );
    let stale: Vec<&String> = recorded.iter().filter(|m| !linked.contains(m)).collect();
    assert!(stale.is_empty(), "native.tsv records {stale:?}, which no shipped artifact links");
}

// ------------------------------------------------------------------- policy

// -------------------------------------------------------------- texts

/// The SPDX ids `texts.tsv` carries a canonical text for.
fn texts() -> Vec<String> {
    tsv_rows(TEXTS)
        .into_iter()
        .filter(|f| f.len() >= 2)
        .map(|f| f[0].to_ascii_lowercase())
        .collect()
}

/// The terms of `allowed.txt`, with `A WITH B` split into its two documents.
fn allowed_documents() -> Vec<String> {
    let mut out = Vec::new();
    for term in font_terms() {
        for part in term.split(" with ") {
            let part = part.trim().to_owned();
            if !part.is_empty() && !out.contains(&part) {
                out.push(part);
            }
        }
    }
    out
}

#[test]
fn every_allowed_licence_has_its_text() {
    // THIRD-PARTY-NOTICES.md reproduces the full text of every term Cadpole may
    // ship under, because MIT/BSD/ISC require their permission notice to
    // travel with a binary and Apache-2.0 s4(a) a copy of the licence. Adding
    // a term to allowed.txt without its text would put a licence in force that
    // the notices file does not state.
    let texts = texts();
    let missing: Vec<String> = allowed_documents()
        .into_iter()
        .filter(|t| !texts.contains(t))
        .collect();
    assert!(
        missing.is_empty(),
        "build_support/licenses/allowed.txt permits terms with no vendored text: {}\n\
         Run build_support/licenses/refresh.py --texts and commit the diff.",
        missing.join(", ")
    );
}

#[test]
fn no_vendored_text_is_unused() {
    // The other direction: a text for a licence the policy no longer permits
    // would be reproduced in the notices file, claiming terms Cadpole is not
    // actually under. `refresh.py --texts` deletes those; this catches a
    // hand-edit that did not.
    let allowed = allowed_documents();
    let extra: Vec<String> = texts()
        .into_iter()
        .filter(|t| !allowed.contains(t))
        .collect();
    assert!(
        extra.is_empty(),
        "build_support/licenses/texts.tsv carries texts for terms allowed.txt does \
         not permit: {}\nRun build_support/licenses/refresh.py --texts.",
        extra.join(", ")
    );
}

#[test]
fn the_texts_inventory_is_well_formed() {
    let rows = tsv_rows(TEXTS);
    assert!(rows.len() >= 10, "texts.tsv records only {} texts?", rows.len());
    for row in rows {
        assert_eq!(
            row.len(),
            4,
            "texts.tsv row has {} columns, want id/file/sha256/source: {row:?}",
            row.len()
        );
        assert_eq!(row[1], format!("{}.txt", row[0]), "text file name should be <id>.txt");
        assert!(
            row[2].len() == 64 && row[2].chars().all(|c| c.is_ascii_hexdigit()),
            "texts.tsv sha256 is not a digest: {:?}",
            row[2]
        );
        assert!(
            row[3].starts_with("https://"),
            "texts.tsv source is not a URL: {:?}",
            row[3]
        );
    }
}

// ------------------------------------------------------------------- policy

#[test]
fn the_policy_itself_rejects_copyleft() {
    let allowed = allowed_terms();
    for term in [
        "GPL-3.0", "AGPL-3.0", "LGPL-2.1", "MPL-2.0", "CDDL-1.0", "SSPL-1.0",
    ] {
        assert!(
            !is_allowed(term, &allowed),
            "{term} must not be allowed on its own"
        );
    }
    // ...but a disjunction that offers a permissive branch is fine.
    assert!(is_allowed(
        "MIT OR Apache-2.0 OR LGPL-2.1-or-later",
        &allowed
    ));
    assert!(is_allowed("MIT/Apache-2.0", &allowed));
    assert!(is_allowed("Apache-2.0 / MIT", &allowed));
    assert!(is_allowed(
        "Apache-2.0 WITH LLVM-exception OR MIT",
        &allowed
    ));
    assert!(!is_allowed("MIT AND MPL-2.0", &allowed));
}

#[test]
fn precedence_and_parentheses_are_respected() {
    let allowed = allowed_terms();
    // `AND` binds tighter than `OR`, so this is MIT OR (Apache AND GPL).
    assert!(is_allowed("MIT OR Apache-2.0 AND GPL-3.0", &allowed));
    // Parentheses override that, and a copyleft `AND` term must then bite.
    // A `split(" or ")` reading of this says "MIT is on offer" and lets it
    // through; that is the bug this parser exists to prevent.
    assert!(!is_allowed("(MIT OR Apache-2.0) AND GPL-3.0", &allowed));
    assert!(!is_allowed("(MIT OR Apache-2.0) AND MPL-2.0", &allowed));
    // The real expression from wasm-bindgen, which is fine: Unicode-3.0 is
    // allowed, so both halves of the AND are.
    assert!(is_allowed("(MIT OR Apache-2.0) AND Unicode-3.0", &allowed));
    // A `WITH` pair is one term, not two.
    assert!(is_allowed("Apache-2.0 WITH LLVM-exception", &allowed));
    assert!(!is_allowed(
        "GPL-2.0 WITH Classpath-exception-2.0",
        &allowed
    ));
    // Nothing is not a licence.
    assert!(!is_allowed("", &allowed));
    assert!(!is_allowed("UNKNOWN", &allowed));
}
