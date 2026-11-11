#!/usr/bin/env python3
"""Regenerate the vendored advisory snapshots.

    python3 build_support/advisories/refresh.py           # both ecosystems
    python3 build_support/advisories/refresh.py --rust    # RustSec only
    python3 build_support/advisories/refresh.py --npm     # OSV/npm only

This is the only thing in the repository that talks to the network, and it is
run by hand, never by a test. It writes two vendored snapshots:

  rustsec.tsv   every advisory in the RustSec database (github.com/rustsec/
                advisory-db), complete and unfiltered, taken from one pinned
                commit whose tarball sha256 is written into the file header.
  npm.tsv       every advisory OSV knows about for the packages in
                pnpm-lock.yaml, taken one package at a time from
                api.osv.dev, with the queried package list in the header.

Then commit the diff: as with `build_support/licenses/refresh.py`, that diff
*is* the review -- it shows exactly which advisories entered or left, and
`bazel test //build_support/advisories:advisory_test` fails until every
advisory that matches a version in `Cargo.lock` or `pnpm-lock.yaml` is either
out of range or recorded in `accepted.tsv` with a reason.

Why the two ecosystems are snapshotted differently: the RustSec database is
1200-odd advisories and 140 KB once indexed, so the whole thing is vendored and
nothing is filtered away by this script. OSV's npm export is 215 MB; the
per-package query returns *all* advisories for a package regardless of version,
so the snapshot is still the checker's input rather than the checker's answer,
and the header records which packages were asked about. The test fails when a
package in the lockfile is not in that list, so adding an npm dependency forces
a refresh.
"""

import argparse
import datetime
import hashlib
import io
import json
import os
import re
import sys
import tarfile
import urllib.request

ROOT = os.path.abspath(os.path.join(os.path.dirname(__file__), "..", ".."))
HERE = os.path.join(ROOT, "build_support", "advisories")
RUST_TSV = os.path.join(HERE, "rustsec.tsv")
NPM_TSV = os.path.join(HERE, "npm.tsv")

ADVISORY_DB_REPO = "rustsec/advisory-db"
ADVISORY_DB_BRANCH = "main"
OSV_QUERY = "https://api.osv.dev/v1/query"

COLUMNS = "id\tpackage\tkind\tvulnerable\tnot_vulnerable\turl\ttitle\n"


def today():
    """The `# Taken:` date both snapshots carry.

    A vendored snapshot is only as good as its age, and the commit hash in
    rustsec.tsv's header says nothing about that without a network round trip.
    CI reads this line (build_support/advisories/age.sh) and warns at 30 days,
    fails at 90.
    """
    return datetime.datetime.now(datetime.timezone.utc).date().isoformat()

# `vulnerable` and `not_vulnerable` hold `;`-separated semver ranges; a range is
# a comma-separated conjunction of comparators, the way Cargo and RustSec write
# them (">= 1.2.0, < 2.0.0"). `*` means every version. A version is vulnerable
# when it matches any `vulnerable` range and no `not_vulnerable` range.


def clean(s):
    """One TSV cell: no tabs, no newlines, no leading/trailing space."""
    return re.sub(r"\s+", " ", (s or "")).strip()


def get(url, data=None, headers=None):
    req = urllib.request.Request(
        url,
        data=json.dumps(data).encode() if data is not None else None,
        headers={"User-Agent": "cadpole-advisory-inventory", **(headers or {})},
    )
    if data is not None:
        req.add_header("Content-Type", "application/json")
    with urllib.request.urlopen(req, timeout=60) as r:
        return r.read()


# ---------------------------------------------------------------- RustSec


def toml_str(frontmatter, key):
    m = re.search(r'^%s = "([^"]*)"' % re.escape(key), frontmatter, re.M)
    return m.group(1) if m else ""


def toml_list(frontmatter, key):
    m = re.search(r"^%s = \[(.*?)\]" % re.escape(key), frontmatter, re.M | re.S)
    return re.findall(r'"([^"]*)"', m.group(1)) if m else []


def parse_advisory(text):
    """One RustSec `.md` -> a row, or None if it has no TOML frontmatter."""
    m = re.match(r"```toml\n(.*?)\n```", text, re.S)
    if not m:
        return None
    fm = m.group(1)
    title = ""
    tm = re.search(r"^#\s+(.*)$", text[m.end():], re.M)
    if tm:
        title = tm.group(1)
    # RustSec states which versions are fixed (`patched`) and which never had
    # the bug (`unaffected`); everything else is vulnerable.
    not_vulnerable = toml_list(fm, "patched") + toml_list(fm, "unaffected")
    url = toml_str(fm, "url") or "https://rustsec.org/advisories/%s" % toml_str(fm, "id")
    return [
        toml_str(fm, "id"),
        toml_str(fm, "package"),
        toml_str(fm, "informational") or "vulnerability",
        "*",
        ";".join(clean(r) for r in not_vulnerable),
        url,
        clean(title),
    ]


def refresh_rust():
    commit = json.loads(
        get("https://api.github.com/repos/%s/commits/%s" % (ADVISORY_DB_REPO, ADVISORY_DB_BRANCH))
    )["sha"]
    url = "https://codeload.github.com/%s/tar.gz/%s" % (ADVISORY_DB_REPO, commit)
    blob = get(url)
    digest = hashlib.sha256(blob).hexdigest()
    rows = []
    skipped = 0
    with tarfile.open(fileobj=io.BytesIO(blob), mode="r:gz") as tf:
        for member in tf.getmembers():
            # crates/<crate>/RUSTSEC-*.md. `rust/` holds advisories against the
            # language and standard library, which no lockfile row can match.
            parts = member.name.split("/")
            if len(parts) != 4 or parts[1] != "crates" or not parts[3].endswith(".md"):
                continue
            row = parse_advisory(tf.extractfile(member).read().decode("utf-8", "replace"))
            if row is None or not row[0] or not row[1]:
                skipped += 1
                continue
            rows.append(row)
    rows.sort(key=lambda r: (r[1], r[0]))
    with open(RUST_TSV, "w") as f:
        f.write(
            "# Every advisory in the RustSec database, complete and unfiltered.\n"
            "#\n"
            "# Source:  https://github.com/%s\n"
            "# Commit:  %s\n"
            "# Tarball: %s\n"
            "# sha256:  %s\n"
            "# Taken:   %s\n"
            "#\n"
            "# Regenerate: build_support/advisories/refresh.py --rust\n"
            "# Enforced by: bazel test //build_support/advisories:advisory_test\n"
            "#\n"
            "# `vulnerable` and `not_vulnerable` are `;`-separated semver ranges; a\n"
            "# range is a comma-separated conjunction (\">= 1.2.0, < 2.0.0\") and `*` is\n"
            "# every version. RustSec states the fixed and never-affected versions, so\n"
            "# `vulnerable` is always `*` here and `not_vulnerable` is patched+unaffected.\n"
            "# `kind` is `vulnerability`, or RustSec's `informational` value\n"
            "# (unmaintained / unsound / notice).\n"
            % (ADVISORY_DB_REPO, commit, url, digest, today())
        )
        f.write(COLUMNS)
        for r in rows:
            f.write("\t".join(r) + "\n")
    print("rustsec.tsv: %d advisories (%d skipped), commit %s" % (len(rows), skipped, commit[:12]))
    print("            sha256 %s" % digest)


# -------------------------------------------------------------------- npm


def npm_packages():
    """(name, version) for every package in pnpm-lock.yaml's `packages:` map."""
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
        if at <= 0:
            continue
        out.add((spec[:at], spec[at + 1:]))
    return sorted(out)


def osv_ranges(affected):
    """One OSV `affected` entry -> (vulnerable ranges, not_vulnerable ranges)."""
    vulnerable, not_vulnerable = [], []
    for rng in affected.get("ranges", []):
        if rng.get("type") not in ("SEMVER", "ECOSYSTEM"):
            continue
        introduced = None
        for event in rng.get("events", []):
            if "introduced" in event:
                introduced = event["introduced"]
            elif "fixed" in event and introduced is not None:
                lo = "" if introduced == "0" else ">= %s, " % introduced
                vulnerable.append("%s< %s" % (lo, event["fixed"]))
                introduced = None
            elif "last_affected" in event and introduced is not None:
                lo = "" if introduced == "0" else ">= %s, " % introduced
                vulnerable.append("%s<= %s" % (lo, event["last_affected"]))
                introduced = None
        if introduced is not None:
            vulnerable.append("*" if introduced == "0" else ">= %s" % introduced)
    for v in affected.get("versions", []) if not vulnerable else []:
        vulnerable.append("= %s" % v)
    return vulnerable, not_vulnerable


def refresh_npm():
    packages = npm_packages()
    names = sorted({n for n, _ in packages})
    rows = {}
    for name in names:
        body = json.loads(get(OSV_QUERY, data={"package": {"name": name, "ecosystem": "npm"}}))
        for vuln in body.get("vulns", []):
            for affected in vuln.get("affected", []):
                pkg = affected.get("package", {})
                if pkg.get("ecosystem") != "npm" or pkg.get("name") != name:
                    continue
                vulnerable, not_vulnerable = osv_ranges(affected)
                if not vulnerable:
                    continue
                severity = vuln.get("database_specific", {}).get("severity", "")
                rows[(name, vuln["id"])] = [
                    vuln["id"],
                    name,
                    severity.lower() or "vulnerability",
                    ";".join(clean(r) for r in vulnerable),
                    ";".join(clean(r) for r in not_vulnerable),
                    "https://osv.dev/vulnerability/%s" % vuln["id"],
                    clean(vuln.get("summary") or (vuln.get("details") or "")[:120]),
                ]
        print("  %-28s %d advisories" % (name, sum(1 for k in rows if k[0] == name)))
    with open(NPM_TSV, "w") as f:
        f.write(
            "# Every advisory OSV knows about for the npm packages in pnpm-lock.yaml,\n"
            "# at every version -- the version matching is done by the test, not here.\n"
            "#\n"
            "# Source: https://api.osv.dev/v1/query, ecosystem npm, one query per package.\n"
            "# Taken:  %s\n"
            "#\n"
            "# Regenerate: build_support/advisories/refresh.py --npm\n"
            "# Enforced by: bazel test //build_support/advisories:advisory_test\n"
            "#\n"
            "# Columns are the same as rustsec.tsv. OSV states the *affected* ranges, so\n"
            "# here `vulnerable` carries them and `not_vulnerable` is usually empty.\n"
            "#\n"
            "# `covered` is the package list this snapshot was taken for. The test fails\n"
            "# when pnpm-lock.yaml contains a package that is not on it, so a new npm\n"
            "# dependency cannot arrive unaudited.\n" % today()
        )
        f.write("# covered: %s\n" % " ".join(names))
        f.write(COLUMNS)
        for key in sorted(rows):
            f.write("\t".join(rows[key]) + "\n")
    print("npm.tsv: %d advisories over %d packages" % (len(rows), len(names)))


def main():
    ap = argparse.ArgumentParser(description=__doc__)
    ap.add_argument("--rust", action="store_true", help="refresh rustsec.tsv only")
    ap.add_argument("--npm", action="store_true", help="refresh npm.tsv only")
    args = ap.parse_args()
    both = not (args.rust or args.npm)
    if args.rust or both:
        refresh_rust()
    if args.npm or both:
        refresh_npm()
    print("\nNow run: bazel test //build_support/advisories:advisory_test")
    print("Anything it names must be fixed, or recorded in accepted.tsv with a reason.")
    return 0


if __name__ == "__main__":
    sys.exit(main())
