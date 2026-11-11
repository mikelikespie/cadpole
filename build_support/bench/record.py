#!/usr/bin/env python3
"""Record a performance baseline for the example scripts.

    build_support/bench/record.py [--repeat 7] [--out bench/baselines/<platform>.json]

This records every example plus the parameterised bench cases in `SWEEPS`,
which are swept so a cost that is quadratic in something shows up as the shape
of a curve rather than as one number that could be anything.

Run this on an otherwise idle machine. The numbers are wall-clock medians, so
a parallel build, a busy CPU or a thermal-throttled laptop moves them by more
than most regressions do; a baseline recorded under load hides the very thing
it exists to catch. `compare.py` refuses to be impressed by small differences
for the same reason.

What the phases mean:

  eval_ms        the script, including every kernel boolean. Dominates.
  tessellate_ms  turning bodies into triangles. Reads 0 for scripts whose
                 final boolean routed to the mesh kernel, because that path
                 already holds triangles and tessellation is then a lookup.
  render_ms      one 800x600 CPU raster frame.
"""

import argparse
import json
import pathlib
import platform
import re
import subprocess
import sys

REPO = pathlib.Path(__file__).resolve().parents[2]

# Bench cases that are not examples: shapes chosen to isolate one cost, each
# run at several sizes. A boolean fan-out -- one body, many tools, one call --
# is quadratic in the tool count if every tool gets its own whole-body boolean
# and linear if they go in together, and no single size tells those
# two apart. `n` is swept so the baseline records the shape of the curve.
SWEEPS = {
    "src/rust/cadpole-bench/cases/boolean_fanout.star": [{"n": str(n)} for n in (4, 8, 16, 32, 64)],
}


# The scripts worth tracking: every example that runs today. A planned-api
# script is skipped by the same marker the examples test uses.
def scripts():
    out = []
    for p in sorted((REPO / "examples").glob("*.star")):
        if "cadpole: planned-api" in p.read_text().splitlines()[0]:
            continue
        out.append(p)
    return out


def sweeps():
    """(label, path, params) for every parameterised bench case."""
    out = []
    for rel, variants in sorted(SWEEPS.items()):
        p = REPO / rel
        for params in variants:
            tag = ",".join(f"{k}={v}" for k, v in sorted(params.items()))
            out.append((f"{p.name}[{tag}]", p, params))
    return out


def run_bench(paths, params, repeat, size):
    """One `cadpole-bench` invocation; returns its JSON rows, or None on failure."""
    cmd = ["bazel", "run", "-c", "opt", "//src/rust/cadpole-bench", "--",
           "--repeat", str(repeat), "--size", size]
    for k, v in sorted(params.items()):
        cmd += ["--param", f"{k}={v}"]
    cmd += [str(p) for p in paths]
    proc = subprocess.run(cmd, cwd=REPO, capture_output=True, text=True)
    if proc.returncode != 0:
        sys.stderr.write(proc.stderr)
        return None
    if proc.stderr.strip():
        sys.stderr.write(proc.stderr)
    return [json.loads(l) for l in proc.stdout.splitlines() if l.startswith("{")]


def measure(repeat, size):
    """label -> measurement row, for the examples and every sweep."""
    rows = {}
    got = run_bench(scripts(), {}, repeat, size)
    if got is None:
        return None
    for r in got:
        rows[pathlib.Path(r["script"]).name] = r
    for label, path, params in sweeps():
        got = run_bench([path], params, repeat, size)
        if got is None:
            return None
        for r in got:
            rows[label] = r
    return rows


def host():
    cpu = ""
    try:
        for line in pathlib.Path("/proc/cpuinfo").read_text().splitlines():
            if line.startswith("model name"):
                cpu = line.split(":", 1)[1].strip()
                break
    except OSError:
        cpu = platform.processor()
    mem = 0
    try:
        m = re.search(r"MemTotal:\s+(\d+) kB", pathlib.Path("/proc/meminfo").read_text())
        mem = round(int(m.group(1)) / 1024 / 1024) if m else 0
    except OSError:
        pass
    import os
    return {
        "platform": f"{platform.system().lower()}-{platform.machine()}",
        "cpu": cpu,
        "cores": os.cpu_count(),
        "memory_gb": mem,
        "kernel": platform.release(),
    }


def main():
    ap = argparse.ArgumentParser()
    ap.add_argument("--repeat", type=int, default=7)
    ap.add_argument("--size", default="800x600")
    ap.add_argument("--out")
    ap.add_argument("--date", help="ISO date to stamp; defaults to today")
    args = ap.parse_args()

    h = host()
    out = pathlib.Path(args.out) if args.out else REPO / "bench" / "baselines" / f"{h['platform']}.json"

    rows = measure(args.repeat, args.size)
    if rows is None:
        return 1
    if not rows:
        sys.stderr.write("cadpole-bench printed no measurements\n")
        return 1
    failed = [label for label, r in rows.items() if r["errors"]]
    if failed:
        # Timing a script that errored measures how fast it fails.
        sys.stderr.write("these scripts reported errors, so the run is not a baseline:\n  " + "\n  ".join(failed) + "\n")
        return 1

    import datetime
    sha = subprocess.check_output(["git", "rev-parse", "HEAD"], cwd=REPO, text=True).strip()
    doc = {
        "_comment": [
            "Measured timings for the example scripts, recorded by //src/rust/cadpole-bench.",
            "Refresh with build_support/bench/record.py on an idle machine; compare a",
            "later run with build_support/bench/compare.py. Wall-clock medians on one",
            "machine: a tripwire for a large regression, not a contract.",
        ],
        "host": h,
        "recorded": {
            "commit": sha,
            "date": args.date or datetime.date.today().isoformat(),
            "profile": "opt",
            "repeat": args.repeat,
        },
        "render_size": args.size,
        "scripts": {},
    }
    for label, r in sorted(rows.items()):
        doc["scripts"][label] = {
            "eval_ms": round(r["eval_ms"], 3),
            "tessellate_ms": round(r["tessellate_ms"], 3),
            "render_ms": round(r["render_ms"], 3),
            "bodies": r["bodies"],
            "triangles": r["triangles"],
        }
    out.parent.mkdir(parents=True, exist_ok=True)
    out.write_text(json.dumps(doc, indent=2) + "\n")
    print(f"wrote {out.relative_to(REPO)} ({len(doc['scripts'])} scripts)")
    return 0


if __name__ == "__main__":
    raise SystemExit(main())
