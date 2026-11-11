#!/usr/bin/env python3
"""Regenerate the licence inventories: crates.tsv and npm.tsv.

    python3 build_support/licenses/refresh.py              # both
    python3 build_support/licenses/refresh.py --crates     # Rust only
    python3 build_support/licenses/refresh.py --npm        # npm only

Each row is a dependency Cadpole ships, the version the lockfile pins, the licence
its own metadata declares, and the copyright notice its licence obliges us to
reproduce. Everything is read out of the Bazel repository cache -- the crate's
own `Cargo.toml` and `LICENSE*` files, the npm package's own `package.json` and
the `LICENSE` inside its tarball -- which is the authoritative statement of what
the author granted. Only packages this platform never fetches (the Windows-only
crates, mostly) fall back to crates.io or the npm registry.

Run it after adding or updating a dependency, then commit the diff: the diff is
the licence review. Three Bazel tests fail until it has been run:

    bazel test //build_support/licenses:license_test        # recorded + allowed
    bazel test //build_support/licenses:attribution_test    # NOTICES is current
    bazel test //build_support/advisories:advisory_test     # (separate script)

The `copyright` column is what makes `THIRD-PARTY-NOTICES.md` a real document
rather than a list: Apache-2.0 section 4(d) and the MIT/BSD notice clauses each
require the copyright notice to travel with a binary distribution, and a
notices file with no notices in it satisfies neither.
"""

import argparse
import collections
import glob
import hashlib
import io
import json
import os
import re
import sys
import tarfile
import time
import urllib.request

ROOT = os.path.abspath(os.path.join(os.path.dirname(__file__), "..", ".."))
HERE = os.path.join(ROOT, "build_support", "licenses")
CRATES_TSV = os.path.join(HERE, "crates.tsv")
NPM_TSV = os.path.join(HERE, "npm.tsv")
FONTS_TSV = os.path.join(HERE, "fonts.tsv")
TEXTS_TSV = os.path.join(HERE, "texts.tsv")
TEXTS_DIR = os.path.join(HERE, "texts")

# The canonical licence texts, from SPDX's own list at a pinned tag. MIT, BSD
# and ISC oblige a binary distribution to carry "this permission notice", and
# Apache-2.0 s4 the licence itself; a table of SPDX identifiers is not that.
SPDX_TAG = "v3.27.0"
SPDX_TEXT_URL = "https://raw.githubusercontent.com/spdx/license-list-data/%s/text/%%s.txt" % SPDX_TAG

BAZEL_CACHE = os.path.expanduser("~/.cache/bazel/_bazel_*/*/external")
CRATE_REPOS = BAZEL_CACHE + "/rules_rust++crate+crates__*"
NPM_REPOS = BAZEL_CACHE + "/aspect_rules_js++npm+npm__*"

# Files that carry the notice a permissive licence asks us to reproduce, most
# specific first.
NOTICE_FILES = ("NOTICE", "COPYRIGHT", "LICENSE", "LICENCE", "COPYING")

COLUMNS = "name\tversion\tlicense\tcopyright\n"


def cell(s):
    """One TSV cell: no tabs, no newlines, collapsed whitespace."""
    return re.sub(r"\s+", " ", (s or "")).strip()


def copyright_lines(text, limit=4):
    """The copyright notices in a licence file, deduplicated, in order.

    A licence file is mostly boilerplate; the part we are obliged to carry is
    the `Copyright (c) <year> <holder>` line(s). Apache-2.0's own template line
    ("Copyright [yyyy] [name of copyright owner]") is not a notice and is
    dropped.
    """
    out = []
    for line in text.splitlines():
        line = line.strip().strip("*#/ ")
        if not re.match(r"^(copyright|\(c\)|©)\b", line, re.I):
            continue
        if not re.search(r"\d{4}|©|\(c\)", line, re.I):
            continue
        if re.search(r"\[yyyy\]|\[name of copyright owner\]|<year>|<name of author>", line, re.I):
            continue
        line = cell(line)
        if line and line not in out:
            out.append(line)
        if len(out) >= limit:
            break
    return out


def notice_from_files(read, names):
    """Walk candidate licence file names through `read(name) -> text|None`."""
    found = []
    for prefix in NOTICE_FILES:
        for name in sorted(n for n in names if os.path.basename(n).upper().startswith(prefix)):
            text = read(name)
            if not text:
                continue
            found.extend(c for c in copyright_lines(text) if c not in found)
        if found:
            break
    return found


def fetch(url, headers=None):
    req = urllib.request.Request(url, headers={"User-Agent": "cadpole-license-inventory", **(headers or {})})
    with urllib.request.urlopen(req, timeout=30) as r:
        return r.read()


# ------------------------------------------------------------------- crates


def crate_cache():
    """(name, version) -> (licence expression, copyright notices)."""
    out = {}
    for d in glob.glob(CRATE_REPOS):
        base = os.path.basename(d).replace("rules_rust++crate+crates__", "")
        m = re.match(r"(.+)-(\d+\.\d+\.\d+.*)$", base)
        if not m:
            continue
        manifest = os.path.join(d, "Cargo.toml")
        if not os.path.exists(manifest):
            continue
        text = open(manifest, errors="replace").read()
        lic = re.search(r'^license = "([^"]+)"', text, re.M)
        lic_file = re.search(r'^license-file = "([^"]+)"', text, re.M)
        license_expr = lic.group(1) if lic else (f"file:{lic_file.group(1)}" if lic_file else "UNKNOWN")

        def read(name, d=d):
            try:
                return open(os.path.join(d, name), errors="replace").read()
            except OSError:
                return None

        try:
            names = os.listdir(d)
        except OSError:
            names = []
        notice = notice_from_files(read, names)
        if not notice:
            # Many crates ship dtolnay-style licence files with no notice at
            # all. The `authors` field is then the only statement of who holds
            # the copyright, and it is what cargo-about falls back to too.
            authors = re.search(r"^authors = \[(.*?)\]", text, re.M | re.S)
            if authors:
                who = ", ".join(re.findall(r'"([^"]*)"', authors.group(1))[:3])
                if who:
                    notice = [f"Copyright (c) the {m.group(1)} authors: {who}"]
        out[(m.group(1), m.group(2))] = (license_expr, " | ".join(notice))
    return out


def crate_from_crates_io(name, version):
    """(licence expression, copyright notices) for a crate not in the cache.

    The API's `license` field is the licence, but it carries no notice, and a
    crate this platform never fetches has no directory to read one out of. So
    fetch the published `.crate` tarball -- the same bytes cargo would -- and
    read its LICENSE files exactly as `crate_cache()` reads an unpacked one.

    This matters because the alternative is not "no data": it is
    THIRD-PARTY-NOTICES.md printing "no notice published by the author" for a
    crate that publishes one. Most blank notices in crates.tsv would come
    from this path, and most of those crates carry a real
    `Copyright ...` line in the tarball. A notices file that says the author
    published nothing, when the author did, is worse than an incomplete one.
    """
    body = json.loads(fetch(f"https://crates.io/api/v1/crates/{name}/{version}"))
    license_expr = body["version"].get("license") or "UNKNOWN"

    notice = []
    try:
        blob = fetch(f"https://crates.io/api/v1/crates/{name}/{version}/download")
        with tarfile.open(fileobj=io.BytesIO(blob), mode="r:gz") as tf:
            members = {m.name: m for m in tf.getmembers() if m.isfile()}

            def read(n, tf=tf, members=members):
                return tf.extractfile(members[n]).read().decode("utf-8", "replace")

            # Only the crate root: `<name>-<version>/LICENSE`, never a
            # vendored dependency's licence deeper in the tree.
            roots = [n for n in members if n.count("/") == 1]
            notice = notice_from_files(read, roots)
            if not notice:
                manifest = f"{name}-{version}/Cargo.toml"
                if manifest in members:
                    authors = re.search(
                        r"^authors = \[(.*?)\]", read(manifest), re.M | re.S
                    )
                    if authors:
                        who = ", ".join(re.findall(r'"([^"]*)"', authors.group(1))[:3])
                        if who:
                            notice = [f"Copyright (c) the {name} authors: {who}"]
    except (OSError, ValueError, tarfile.TarError) as e:
        print(f"  ! {name} {version}: no notice from the tarball ({type(e).__name__})",
              file=sys.stderr)

    return license_expr, " | ".join(notice)


def cargo_lock_packages():
    """[(name, version, third_party)] for every [[package]] in Cargo.lock.

    A package with no `source` is one of Cadpole's own crates. Deriving that from
    the lockfile rather than from a hand-kept list is deliberate: the previous
    hand-kept `WORKSPACE_CRATES` set had to be edited every time a crate was
    added, and a crate left off it was silently looked up on crates.io and came
    back UNKNOWN. `//build_support/licenses:license_test` checks the same
    property from the other side.
    """
    out = []
    lock = open(os.path.join(ROOT, "Cargo.lock")).read()
    for block in lock.split("[[package]]")[1:]:
        name = re.search(r'name = "([^"]+)"', block)
        version = re.search(r'version = "([^"]+)"', block)
        if name and version:
            out.append((name.group(1), version.group(1), "source = " in block))
    return out


def recorded_notices(path):
    """(name, version) -> the copyright already recorded in a .tsv.

    A crate is only in the Bazel cache once this platform has fetched it, so a
    refresh run on Linux cannot read the notice of a macOS-only crate that a
    previous run on a Mac recorded. Without this the refresh would quietly blank
    those rows and `THIRD-PARTY-NOTICES.md` would drop real notices -- which is
    the opposite of what the file is for, and a licence obligation rather than
    a formatting preference.
    """
    out = {}
    try:
        with open(path) as f:
            for line in f:
                if line.startswith("#") or line.startswith("name\t"):
                    continue
                parts = line.rstrip("\n").split("\t")
                if len(parts) >= 4 and parts[3]:
                    out[(parts[0], parts[1])] = parts[3]
    except OSError:
        pass
    return out


def do_crates():
    cache = crate_cache()
    known = recorded_notices(CRATES_TSV)
    rows = []
    unknown = []
    kept = []
    for name, version, third_party in sorted(cargo_lock_packages()):
        if not third_party:
            rows.append((name, version, "Apache-2.0", "Copyright (c) the Cadpole authors"))
            continue
        lic, notice = cache.get((name, version), ("UNKNOWN", ""))
        if not notice and (name, version) in known:
            # Not fetched on this platform; keep what was recorded rather than
            # losing the notice.
            notice = known[(name, version)]
            kept.append(f"{name} {version}")
        if lic == "UNKNOWN":
            try:
                lic, from_io = crate_from_crates_io(name, version)
                # The tarball is the author's own statement, so it wins over a
                # notice an earlier run on another platform recorded; but never
                # replace something with nothing.
                if from_io:
                    notice = from_io
                time.sleep(0.05)
            except Exception as e:
                print(f"  ! {name} {version}: {type(e).__name__}", file=sys.stderr)
        if lic == "UNKNOWN":
            unknown.append(f"{name} {version}")
        rows.append((name, version, cell(lic), cell(notice)))
    with open(CRATES_TSV, "w") as f:
        f.write(
            "# Licence and copyright notice of every crate in Cargo.lock.\n"
            "# Read from each crate's own Cargo.toml and LICENSE files in the Bazel\n"
            "# cache, falling back to crates.io for crates this platform never fetches.\n"
            "# A crate with no `source` in Cargo.lock is one of Cadpole's own.\n"
            "#\n"
            "# `copyright` is the notice Apache-2.0 s4(d) and the MIT/BSD notice clauses\n"
            "# oblige a binary distribution to carry; it feeds THIRD-PARTY-NOTICES.md.\n"
            "# Multiple notices are separated by ` | `.\n"
            "#\n"
            "# Regenerate: build_support/licenses/refresh.py --crates\n"
            "# Enforced by: bazel test //build_support/licenses:license_test\n"
        )
        f.write(COLUMNS)
        for r in rows:
            f.write("\t".join(r) + "\n")
    counts = collections.Counter(r[2] for r in rows)
    print(f"crates.tsv: {len(rows)} crates")
    for lic, n in counts.most_common():
        print(f"  {n:4}  {lic}")
    missing = [r for r in rows if not r[3]]
    if missing:
        print(f"  {len(missing)} with no copyright notice: {', '.join(r[0] for r in missing[:8])}")
    if kept:
        print(f"  {len(kept)} notice(s) kept from the previous inventory (not fetched on this platform): {', '.join(kept[:6])}")
    if unknown:
        print(f"  ! UNKNOWN licence: {', '.join(unknown)}", file=sys.stderr)


# ---------------------------------------------------------------------- npm


def pnpm_lock_packages():
    """[(name, version)] for every package in pnpm-lock.yaml's `packages:` map."""
    lock = open(os.path.join(ROOT, "pnpm-lock.yaml")).read()
    out = set()
    in_packages = False
    for line in lock.splitlines():
        if not line.strip() or line.lstrip().startswith("#"):
            continue
        if not line.startswith(" "):
            in_packages = line.startswith("packages:")
            continue
        if not in_packages:
            continue
        m = re.match(r"^  '?([^':]+?)'?:\s*$", line)
        if not m:
            continue
        spec = re.sub(r"\(.*\)$", "", m.group(1))
        at = spec.rfind("@")
        if at > 0:
            out.add((spec[:at], spec[at + 1:]))
    return sorted(out)


# npm `license` strings that are not SPDX but name exactly one well-known
# licence, mapped to the SPDX id, with the evidence that says so.
#
# The policy is that an expression is *parsed*, never pattern-matched, so a
# malformed string must not simply be added to allowed.txt: that would let any
# unparseable disjunction through. This table is the narrow alternative --
# keyed on the exact string, only for a single unambiguous licence, never for
# an AND/OR, and only where the package's own files agree.
#
#   "Apache 2.0"  -- @informalsystems/quint. The tarball ships no LICENSE
#                    file, and all 22 of its source files carry
#                    "Copyright <year> Informal Systems / Licensed under the
#                    Apache License, Version 2.0"; the upstream repository is
#                    Apache-2.0.
NON_SPDX_LICENSE_SPELLINGS = {
    "Apache 2.0": "Apache-2.0",
}


def npm_license_expr(meta):
    """npm's `license` field has three historical shapes."""
    lic = meta.get("license")
    if isinstance(lic, str):
        spelled = lic.strip()
        return NON_SPDX_LICENSE_SPELLINGS.get(spelled, spelled)
    if isinstance(lic, dict):
        return (lic.get("type") or "").strip()
    licenses = meta.get("licenses")
    if isinstance(licenses, list):
        terms = [l.get("type", "") if isinstance(l, dict) else str(l) for l in licenses]
        terms = [t for t in terms if t]
        if terms:
            return " OR ".join(terms)
    return "UNKNOWN"


def npm_cache():
    """(name, version) -> (licence expression, copyright notices).

    rules_js extracts only `package.json` out of each package and keeps the
    rest as `package.tgz`, so the notice comes out of the tarball.
    """
    out = {}
    for d in glob.glob(NPM_REPOS):
        manifest = os.path.join(d, "package", "package.json")
        if not os.path.exists(manifest):
            continue
        try:
            meta = json.load(open(manifest, errors="replace"))
        except (OSError, ValueError):
            continue
        name, version = meta.get("name"), meta.get("version")
        if not name or not version:
            continue
        notice = []
        tgz = os.path.join(d, "package.tgz")
        if os.path.exists(tgz):
            try:
                with tarfile.open(tgz, "r:gz") as tf:
                    members = {m.name: m for m in tf.getmembers() if m.isfile()}

                    def read(n, tf=tf, members=members):
                        return tf.extractfile(members[n]).read().decode("utf-8", "replace")

                    # Only the package root, never a bundled dependency's licence.
                    roots = [n for n in members if n.count("/") == 1]
                    notice = notice_from_files(read, roots)
            except (OSError, tarfile.TarError):
                pass
        if not notice:
            author = meta.get("author")
            if isinstance(author, dict):
                author = author.get("name")
            if author:
                notice = [f"Copyright (c) {cell(author)}"]
        out[(name, version)] = (npm_license_expr(meta), " | ".join(notice))
    return out


def npm_from_registry(name, version):
    body = json.loads(fetch(f"https://registry.npmjs.org/{name}/{version}"))
    return npm_license_expr(body), ""


def do_npm():
    cache = npm_cache()
    rows = []
    unknown = []
    for name, version in pnpm_lock_packages():
        lic, notice = cache.get((name, version), ("UNKNOWN", ""))
        if lic == "UNKNOWN":
            try:
                lic, notice = npm_from_registry(name, version)
                time.sleep(0.05)
            except Exception as e:
                print(f"  ! {name} {version}: {type(e).__name__}", file=sys.stderr)
        if lic == "UNKNOWN" or not lic:
            unknown.append(f"{name} {version}")
            lic = "UNKNOWN"
        rows.append((name, version, cell(lic), cell(notice)))
    with open(NPM_TSV, "w") as f:
        f.write(
            "# Licence and copyright notice of every npm package in pnpm-lock.yaml.\n"
            "# Read from each package's own package.json and the LICENSE inside its\n"
            "# tarball, both in the Bazel cache (rules_js extracts package.json and\n"
            "# keeps package.tgz), falling back to registry.npmjs.org.\n"
            "#\n"
            "# The web UI is part of what Cadpole ships -- //src/web:dist is embedded in\n"
            "# the CLI binary and served by `cadpole serve` -- so an npm package's licence\n"
            "# is exactly as binding as a crate's, and is checked against the same\n"
            "# build_support/licenses/allowed.txt.\n"
            "#\n"
            "# Regenerate: build_support/licenses/refresh.py --npm\n"
            "# Enforced by: bazel test //build_support/licenses:license_test\n"
        )
        f.write(COLUMNS)
        for r in rows:
            f.write("\t".join(r) + "\n")
    counts = collections.Counter(r[2] for r in rows)
    print(f"npm.tsv: {len(rows)} packages")
    for lic, n in counts.most_common():
        print(f"  {n:4}  {lic}")
    if unknown:
        print(f"  ! UNKNOWN licence: {', '.join(unknown)}", file=sys.stderr)


# -------------------------------------------------------------------- texts


def spdx_terms(tsv_text):
    """Every SPDX identifier named anywhere in an inventory's licence column.

    A crude tokenizer, not the parser `license_test.rs` uses: here the shape of
    the expression does not matter, only which identifiers appear, because
    every one of them is a set of terms Cadpole might be relying on and therefore
    a text the notices file has to carry. `WITH` binds its exception to the
    licence before it, and both halves get a text of their own.
    """
    terms = set()
    for row in tsv_rows_py(tsv_text):
        if len(row) < 3:
            continue
        tokens = re.split(r"[()\s/]+", row[2])
        for t in tokens:
            if t and t.upper() not in ("AND", "OR", "WITH"):
                terms.add(t)
    return terms


def tsv_rows_py(text):
    return [
        line.rstrip("\n").split("\t")
        for line in text.splitlines()
        if line and not line.startswith("#") and not line.startswith("name\t")
    ]


def do_texts():
    """Vendor the canonical text of every licence Cadpole may be bound by.

    The set is `allowed.txt`, not "every identifier the inventories mention",
    and the difference matters. `r-efi` offers
    `MIT OR Apache-2.0 OR LGPL-2.1-or-later`; we take one of the first two, and
    reproducing the LGPL text would say in the notices file that Cadpole ships
    LGPL code -- the exact thing the policy refuses. A disjunction's
    unused branch obliges us to nothing.

    Every dependency is satisfied by some term in this file --
    `license_test.rs` is what guarantees that -- so a text per allowed term
    covers everything actually shipped.
    """
    terms = set()
    for line in open(os.path.join(HERE, "allowed.txt")):
        line = line.strip()
        if not line or line.startswith("#"):
            continue
        line = line[len("fonts-only:"):].strip() if line.startswith("fonts-only:") else line
        # `Apache-2.0 WITH LLVM-exception` is one term for the policy, but two
        # documents: the licence and the exception that modifies it.
        for t in re.split(r"\s+WITH\s+", line):
            if t:
                terms.add(t.strip())

    os.makedirs(TEXTS_DIR, exist_ok=True)
    rows, missing = [], []
    for term in sorted(terms, key=str.lower):
        try:
            body = fetch(SPDX_TEXT_URL % term)
        except Exception as e:
            missing.append(f"{term} ({type(e).__name__})")
            continue
        name = f"{term}.txt"
        with open(os.path.join(TEXTS_DIR, name), "wb") as f:
            f.write(body)
        rows.append((term, name, hashlib.sha256(body).hexdigest(), SPDX_TEXT_URL % term))
        time.sleep(0.05)

    # Drop texts for terms nothing names any more, so the directory cannot
    # accumulate licences Cadpole does not use.
    keep = {r[1] for r in rows}
    for name in sorted(os.listdir(TEXTS_DIR)):
        if name.endswith(".txt") and name not in keep:
            os.remove(os.path.join(TEXTS_DIR, name))
            print(f"  removed texts/{name} (no longer named by any dependency)")

    with open(TEXTS_TSV, "w") as f:
        f.write(
            "# The canonical text of every licence Cadpole's dependencies are offered\n"
            "# under, and of every term allowed.txt permits.\n"
            "#\n"
            "# MIT, BSD-2/3-Clause and ISC each require \"this permission notice\" to\n"
            "# be included with a binary distribution, and Apache-2.0 section 4(a) a\n"
            "# copy of the licence itself. A list of SPDX identifiers is not that, so\n"
            "# THIRD-PARTY-NOTICES.md reproduces these texts in full; the per-package\n"
            "# copyright notices that go with them are in crates.tsv / npm.tsv.\n"
            "#\n"
            "# Source: SPDX license-list-data, tag %s, text/<id>.txt.\n"
            "# `sha256` is of the file as fetched, so a text cannot drift unnoticed.\n"
            "#\n"
            "# Regenerate: build_support/licenses/refresh.py --texts\n"
            "# Enforced by: bazel test //build_support/licenses:license_test\n"
            "#              (every term in use has a text, and every text matches)\n"
            % SPDX_TAG
        )
        f.write("id\tfile\tsha256\tsource\n")
        for r in rows:
            f.write("\t".join(r) + "\n")
    print(f"texts.tsv: {len(rows)} licence texts")
    if missing:
        print(f"  ! no SPDX text for: {', '.join(missing)}", file=sys.stderr)
        return 1
    return 0


def main():
    ap = argparse.ArgumentParser(description=__doc__)
    ap.add_argument("--crates", action="store_true", help="refresh crates.tsv only")
    ap.add_argument("--npm", action="store_true", help="refresh npm.tsv only")
    ap.add_argument("--texts", action="store_true", help="refresh texts/ + texts.tsv only")
    args = ap.parse_args()
    both = not (args.crates or args.npm or args.texts)
    if args.crates or both:
        do_crates()
    if args.npm or both:
        do_npm()
    # After the inventories, so a newly arrived licence gets its text in the
    # same run that records it.
    if args.texts or both:
        do_texts()
    print("\nNow run:")
    print("  bazel test //build_support/licenses:license_test")
    print("  bazel run  //build_support/licenses:attribution_update   # refresh THIRD-PARTY-NOTICES.md")
    return 0


if __name__ == "__main__":
    sys.exit(main())
