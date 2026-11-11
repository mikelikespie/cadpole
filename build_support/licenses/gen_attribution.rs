//! Writes `THIRD-PARTY-NOTICES.md`: what Cadpole ships, and under what terms.
//!
//! Apache-2.0 section 4 and the notice clauses of MIT, BSD-2/3-Clause and ISC
//! all say the same thing about a *binary* distribution: the copyright notice
//! and the licence text have to travel with it. Cadpole ships a single linked
//! binary with ~500 crates inside it, a wasm module, and a web bundle with
//! three.js in it, so "the licences are on crates.io" is not compliance. This
//! file is the artifact that satisfies the obligation, and
//! `//build_support/licenses:attribution_test` fails when the committed copy
//! is not what this program would write today.
//!
//! Everything comes from the three inventories `refresh.py` and the font
//! review maintain, so the document can never claim a licence the licence test
//! is not also enforcing. It is deliberately not a build of the dependency
//! *graph*: the graph is Cargo.lock's job, and a notices file has to list
//! everything shipped whether or not this build configuration links it.
//!
//! Usage (both driven by Bazel, never by hand):
//!     gen_attribution <crates.tsv> <npm.tsv> <fonts.tsv> <native.tsv> \
//!         <allowed.txt> <texts.tsv> <out.md> [licence text files...]

use std::io::Write;

/// One line of the inventory: name, version, licence, copyright notice.
struct Entry {
    name: String,
    version: String,
    license: String,
    copyright: String,
}

fn read(path: &str) -> String {
    std::fs::read_to_string(path).unwrap_or_else(|e| panic!("cannot read {path}: {e}"))
}

fn entries(tsv: &str) -> Vec<Entry> {
    lockfiles::tsv_rows(tsv)
        .into_iter()
        .filter(|f| f.len() >= 3)
        .map(|f| Entry {
            name: f[0].to_owned(),
            version: f[1].to_owned(),
            license: f[2].to_owned(),
            copyright: f.get(3).unwrap_or(&"").trim().to_owned(),
        })
        .collect()
}

/// The licences actually in use, most used first, so the summary is a real
/// answer to "what terms is Cadpole bound by".
fn licence_histogram(entries: &[&Entry]) -> Vec<(String, usize)> {
    let mut counts: Vec<(String, usize)> = Vec::new();
    for e in entries {
        match counts.iter_mut().find(|(l, _)| *l == e.license) {
            Some((_, n)) => *n += 1,
            None => counts.push((e.license.clone(), 1)),
        }
    }
    counts.sort_by(|a, b| b.1.cmp(&a.1).then(a.0.cmp(&b.0)));
    counts
}

fn table(out: &mut String, entries: &[&Entry]) {
    out.push_str("| Package | Version | Licence | Copyright |\n");
    out.push_str("| --- | --- | --- | --- |\n");
    for e in entries {
        let notice = if e.copyright.is_empty() {
            "*no notice published by the author*".to_owned()
        } else {
            e.copyright.replace('|', "\\|")
        };
        out.push_str(&format!(
            "| `{}` | {} | {} | {} |\n",
            e.name,
            e.version,
            e.license.replace('|', "\\|"),
            notice
        ));
    }
}

/// What a Cadpole binary contains that no lockfile lists.
///
/// The three inventories cover what Cargo and pnpm resolve. They do not cover
/// what the *toolchains* put in the binary: rustc links its own standard
/// library into every Rust artifact, the C toolchain links libgcc's unwinder,
/// and wasm-bindgen writes the JavaScript that loads the wasm module. Those
/// are third-party code in the thing we ship, so they are named here.
///
/// Each line: what, the terms, and where the claim can be checked.
const TOOLCHAIN_NOTICES: &[(&str, &str, &str)] = &[
    (
        "Rust standard library (`core`, `alloc`, `std`)",
        "MIT OR Apache-2.0",
        "Statically linked into `cadpole`, `cadpole_wasm` and the C FFI library by \
         rustc itself. Copyright the Rust Project Developers; the same terms as \
         the `rustc-std-workspace-*` rows in the crate table.",
    ),
    (
        "`libgcc_s.so.1` (GCC runtime: unwinder, soft-float helpers)",
        "GPL-3.0-or-later WITH GCC-exception-3.1",
        "Dynamically linked into the Linux `cadpole` binary (`ldd` lists it). The \
         GCC Runtime Library Exception is what makes this permissible in a \
         non-GPL program: it grants the right to propagate the runtime's object \
         code in a work that is not GPL, so no obligation flows to Cadpole. This is \
         the one component here that `allowed.txt` would reject if it were a \
         crate, which is why it is named rather than left implicit.",
    ),
    (
        "libstdc++ (GCC's C++ standard library)",
        "GPL-3.0-or-later WITH GCC-exception-3.1",
        "Statically linked into the Linux `cadpole` binary, because HiGHS \
         is C++: the hermetic toolchain links `-l:libstdc++.a`, the host GCC's, and \
         only the parts HiGHS calls end up in the binary. The GCC Runtime Library Exception is \
         what permits this in a non-GPL program: everything in the binary is \
         compiled by Clang or rustc, which are Eligible Compilation Processes, so \
         the exception grants the right to propagate the combined object code \
         under Cadpole's own terms and no obligation flows to Cadpole. Named, like \
         `libgcc_s`, because `allowed.txt` would reject it if it were a crate.",
    ),
    (
        "libc++ (the macOS system C++ library)",
        "Apache-2.0 WITH LLVM-exception",
        "Dynamically linked into the macOS `cadpole` binary, because \
         HiGHS is C++: Xcode's toolchain links the system's `/usr/lib/libc++.1.dylib`, \
         which ships with macOS and is not copied into any Cadpole artifact.",
    ),
    (
        "wasm-bindgen generated glue (`cadpole_wasm.js`, `cadpole_wasm_bg.wasm.d.ts`)",
        "MIT OR Apache-2.0",
        "Emitted by the `wasm-bindgen` crate (in the crate table) and shipped \
         verbatim in `//src/web:dist`, so the generator's terms travel with it.",
    ),
];

/// Licence texts, keyed by SPDX id, in the order `texts.tsv` lists them.
fn licence_texts(texts_tsv: &str, files: &[String]) -> Vec<(String, String, String)> {
    lockfiles::tsv_rows(texts_tsv)
        .into_iter()
        .filter(|f| f.len() >= 2)
        .map(|f| {
            let (id, wanted) = (f[0], f[1]);
            // Bazel gives the files under an execroot-relative path; match on
            // the name recorded in the inventory rather than assuming a layout.
            let path = files
                .iter()
                .find(|p| {
                    std::path::Path::new(p)
                        .file_name()
                        .is_some_and(|n| n == wanted)
                })
                .unwrap_or_else(|| {
                    panic!("texts.tsv names {wanted}, which was not passed to gen_attribution")
                });
            (id.to_owned(), wanted.to_owned(), read(path))
        })
        .collect()
}

fn main() {
    let args: Vec<String> = std::env::args().skip(1).collect();
    let [crates_tsv, npm_tsv, fonts_tsv, native_tsv, allowed_txt, texts_tsv, output, text_files @ ..] =
        args.as_slice()
    else {
        eprintln!(
            "usage: gen_attribution <crates.tsv> <npm.tsv> <fonts.tsv> <native.tsv> \
             <allowed.txt> <texts.tsv> <out.md> [licence text files...]"
        );
        std::process::exit(2);
    };

    let crates = entries(&read(crates_tsv));
    let npm = entries(&read(npm_tsv));
    let native = entries(&read(native_tsv));
    let fonts_text = read(fonts_tsv);
    let allowed = read(allowed_txt);
    let texts = licence_texts(&read(texts_tsv), text_files);

    // Cadpole's own crates carry Cadpole's licence and are not third-party notices;
    // they are named in the header instead.
    let (ours, third_party): (Vec<&Entry>, Vec<&Entry>) =
        crates.iter().partition(|e| e.name.starts_with("cadpole-"));
    let npm_refs: Vec<&Entry> = npm.iter().collect();
    let native_refs: Vec<&Entry> = native.iter().collect();

    let mut out = String::new();
    out.push_str(
        "# Third-party notices\n\
         \n\
         Cadpole is Copyright the Cadpole authors and is distributed under the Apache\n\
         License, Version 2.0; the full text is in [`LICENSE`](LICENSE). A Cadpole\n\
         binary, the `cadpole_wasm` module and the web bundle each contain third-party\n\
         code, and this file is the notice that Apache-2.0 section 4(d) and the\n\
         notice clauses of MIT, BSD-2-Clause, BSD-3-Clause and ISC require to travel\n\
         with them.\n\
         \n\
         **This file is generated.** `bazel run //build_support/licenses:attribution_update`\n\
         rewrites it from `build_support/licenses/{crates,npm,fonts,native}.tsv`, and\n\
         `bazel test //build_support/licenses:attribution_test` fails when the\n\
         committed copy has drifted. Every licence named here has also been checked\n\
         against `build_support/licenses/allowed.txt` by\n\
         `//build_support/licenses:license_test`, and every version here has been\n\
         checked against the RustSec and OSV advisory snapshots by\n\
         `//build_support/advisories:advisory_test`.\n\
         \n\
         Where a package publishes no copyright line of its own, this file says so\n\
         rather than inventing one; the grant still comes from the licence its own\n\
         metadata declares, recorded in the same row.\n\
         \n",
    );

    out.push_str("## Summary\n\n");
    out.push_str(&format!(
        "| | Count |\n| --- | --- |\n\
         | Cadpole's own crates (Apache-2.0) | {} |\n\
         | Third-party crates | {} |\n\
         | npm packages in the web UI | {} |\n\
         | C and C++ libraries from Bazel modules | {} |\n\
         | Bundled fonts | {} |\n\n",
        ours.len(),
        third_party.len(),
        npm_refs.len(),
        native_refs.len(),
        lockfiles::tsv_rows(&fonts_text).len()
    ));

    out.push_str("Licence terms in force across the third-party code:\n\n");
    out.push_str("| Licence | Packages |\n| --- | --- |\n");
    let mut combined: Vec<&Entry> = third_party.clone();
    combined.extend(npm_refs.iter().copied());
    combined.extend(native_refs.iter().copied());
    for (lic, n) in licence_histogram(&combined) {
        out.push_str(&format!("| {} | {} |\n", lic.replace('|', "\\|"), n));
    }
    out.push('\n');

    out.push_str(
        "The policy behind that list -- which terms Cadpole may ship under, and why\n\
         MPL, CDDL and every flavour of GPL are refused -- is\n\
         `build_support/licenses/allowed.txt`, reproduced at the end of this file.\n\n",
    );

    out.push_str("## Cadpole's own crates\n\n");
    out.push_str("Apache-2.0, Copyright the Cadpole authors.\n\n");
    for e in &ours {
        out.push_str(&format!("- `{}` {}\n", e.name, e.version));
    }
    out.push('\n');

    out.push_str("## Third-party crates\n\n");
    out.push_str(&format!(
        "Every crate in `Cargo.lock`, whether or not a given build configuration\n\
         links it: `Cargo.lock` resolves for every platform Cadpole targets, and a\n\
         notice file that changed with the host would be useless. {} crates.\n\n",
        third_party.len()
    ));
    table(&mut out, &third_party);
    out.push('\n');

    out.push_str("## npm packages\n\n");
    out.push_str(
        "Every package in `pnpm-lock.yaml`. `//src/web:dist` is embedded in the\n\
         `cadpole` binary and served by `cadpole serve`, so these ship too.\n\n",
    );
    table(&mut out, &npm_refs);
    out.push('\n');

    out.push_str("## C and C++ libraries\n\n");
    out.push_str(
        "C and C++ code a Bazel module compiles into a Cadpole binary, which no\n\
         lockfile lists: `build_support/licenses/native.tsv`, which\n\
         `//build_support/licenses:license_test` checks against the module graph\n\
         and against what the shipped artifacts link. All of it is HiGHS,\n\
         linked into the `cadpole` CLI only -- never the wasm module or the C FFI\n\
         library -- and the third-party code inside it that its build compiles.\n\n",
    );
    table(&mut out, &native_refs);
    out.push('\n');

    out.push_str("## Toolchain and runtime\n\n");
    out.push_str(
        "Third-party code that the toolchains put into a Cadpole artifact, which no\n\
         lockfile lists and the tables above therefore do not cover.\n\n",
    );
    for (what, terms, note) in TOOLCHAIN_NOTICES {
        out.push_str(&format!("- **{what}** — {terms}. {note}\n"));
    }
    out.push_str(
        "\nNot listed, because they are build- and test-time only and reach no\n\
         shipped artifact: the pinned Chromium headless shell\n\
         (`//build_support/playwright`), the Quint evaluator\n\
         (`//build_support/quint`), the Swift.org Linux toolchain\n\
         (`//build_support/swift`), the LLVM toolchain, Node, and esbuild —\n\
         esbuild's *output* is Cadpole's own code plus the npm packages already\n\
         listed above.\n\n",
    );

    out.push_str("## Bundled fonts\n\n");
    out.push_str(
        "Font *data*, not linked code. The SIL Open Font License requires its text\n\
         to ship next to the face, which is what the `licence file` column points\n\
         at; `reserved font names` is `none` for every face Cadpole bundles so that\n\
         subsetting needs no rename.\n\n\
         | Face | Version | Licence | Reserved font names | File | Licence file |\n\
         | --- | --- | --- | --- | --- | --- |\n",
    );
    for f in lockfiles::tsv_rows(&fonts_text) {
        if f.len() < 6 {
            continue;
        }
        out.push_str(&format!(
            "| {} | {} | {} | {} | `{}` | `{}` |\n",
            f[0], f[1], f[2], f[3], f[4], f[5]
        ));
    }
    out.push('\n');

    out.push_str("## Licence policy\n\n");
    out.push_str("`build_support/licenses/allowed.txt`, verbatim:\n\n```\n");
    out.push_str(allowed.trim_end());
    out.push_str("\n```\n");

    // The texts themselves. MIT, BSD-2/3-Clause and ISC each require "this
    // permission notice" to accompany a copy; Apache-2.0 section 4(a) requires
    // a copy of the licence. The per-package copyright lines are in the tables
    // above; these are the terms those notices are attached to.
    out.push_str("\n## Licence texts\n\n");
    out.push_str(&format!(
        "The full text of every licence Cadpole may be bound by — the SPDX canonical\n\
         text of each term in `allowed.txt`, vendored under\n\
         `build_support/licenses/texts/` by `refresh.py --texts` and reproduced\n\
         here because MIT, BSD-2-Clause, BSD-3-Clause and ISC each require their\n\
         permission notice to accompany a distribution, and Apache-2.0 section\n\
         4(a) a copy of the licence. {} licences.\n\n\
         A licence a dependency merely *offers* as one branch of a disjunction we\n\
         do not take is not reproduced: `r-efi` is `MIT OR Apache-2.0 OR\n\
         LGPL-2.1-or-later`, Cadpole takes one of the first two, and printing the\n\
         LGPL here would say something untrue about what Cadpole ships.\n\n",
        texts.len()
    ));
    for (id, file, body) in &texts {
        out.push_str(&format!(
            "### {id}\n\n`build_support/licenses/texts/{file}`\n\n```\n{}\n```\n\n",
            body.trim_end()
        ));
    }

    let mut file =
        std::fs::File::create(output).unwrap_or_else(|e| panic!("cannot write {output}: {e}"));
    file.write_all(out.as_bytes()).expect("write failed");
}
