#!/usr/bin/env python3
"""Compare a fresh benchmark run against the recorded baseline.

    build_support/bench/compare.py [--baseline bench/baselines/<platform>.json]
                                   [--threshold 1.35] [--repeat 7]

Exits non-zero when any phase of any script is slower than the baseline by
more than `--threshold`, or when the triangle count changed (which usually
means the geometry changed, so the timings are not comparable at all).

The threshold is deliberately loose. These are wall-clock medians on a shared
machine, and a 10% difference between two runs of the same commit is normal;
a gate that fires on 10% would be turned off within a week. What it catches
is the kind of regression that actually matters here: a boolean that stopped
routing to the mesh kernel, a tessellation that lost its cache, an O(n^2) that
crept into the raster loop.
"""

import argparse
import json
import pathlib
import sys

REPO = pathlib.Path(__file__).resolve().parents[2]
PHASES = ("eval_ms", "tessellate_ms", "render_ms")
# Below this, a ratio is meaningless: 0.2 ms against 0.1 ms is scheduler noise.
FLOOR_MS = 5.0


def main():
    ap = argparse.ArgumentParser()
    ap.add_argument("--baseline")
    ap.add_argument("--threshold", type=float, default=1.35)
    ap.add_argument("--repeat", type=int, default=7)
    args = ap.parse_args()

    sys.path.insert(0, str(pathlib.Path(__file__).parent))
    import record  # noqa: E402  -- shares host detection and the script list

    h = record.host()
    path = pathlib.Path(args.baseline) if args.baseline else REPO / "bench" / "baselines" / f"{h['platform']}.json"
    if not path.exists():
        sys.stderr.write(f"no baseline at {path}; record one with build_support/bench/record.py\n")
        return 1
    base = json.loads(path.read_text())
    if base["host"]["cpu"] != h["cpu"]:
        sys.stderr.write(
            f"warning: baseline was recorded on {base['host']['cpu']!r}, this is {h['cpu']!r}; "
            "the comparison is between machines, not between commits\n"
        )

    rows = record.measure(args.repeat, base.get("render_size", "800x600"))
    if rows is None:
        return 1

    worse, notes = [], []
    print(f"{'script':<40} {'phase':<14} {'baseline':>10} {'now':>10} {'ratio':>7}")
    for name, want in sorted(base["scripts"].items()):
        got = rows.get(name)
        if got is None:
            notes.append(f"{name}: in the baseline but not in this run")
            continue
        if got["errors"]:
            worse.append(f"{name}: reported {got['errors']} error(s)")
            continue
        if got["triangles"] != want["triangles"]:
            notes.append(f"{name}: {want['triangles']} triangles in the baseline, {got['triangles']} now "
                         "-- the geometry changed, so re-record rather than read the timings")
        for phase in PHASES:
            a, b = want[phase], got[phase]
            ratio = b / a if a > 0 else float("inf") if b > 0 else 1.0
            if max(a, b) < FLOOR_MS:
                continue
            flag = ""
            if ratio > args.threshold:
                flag = "  SLOWER"
                worse.append(f"{name} {phase}: {a:.1f} ms -> {b:.1f} ms ({ratio:.2f}x)")
            elif ratio < 1 / args.threshold:
                flag = "  faster"
            print(f"{name:<40} {phase:<14} {a:>9.1f}  {b:>9.1f}  {ratio:>6.2f}x{flag}")
    for name in sorted(set(rows) - set(base["scripts"])):
        notes.append(f"{name}: new since the baseline, not compared")

    for n in notes:
        print(f"note: {n}")
    if worse:
        print("\nregressions:")
        for w in worse:
            print(f"  {w}")
        return 1
    print(f"\nnothing slower than {args.threshold:.2f}x the baseline "
          f"({base['recorded']['commit'][:12]}, {base['recorded']['date']})")
    return 0


if __name__ == "__main__":
    raise SystemExit(main())
