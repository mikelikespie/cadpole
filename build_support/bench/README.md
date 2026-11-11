# Performance baselines

`//src/rust/cadpole-bench` times three phases of each example script and prints
one JSON line per script. Two wrappers turn that into something you can
track across commits:

```
build_support/bench/record.py                 # write bench/baselines/<platform>.json
build_support/bench/compare.py                # run again and diff against it
build_support/bench/compare.py --threshold 1.2
```

Record on an idle machine: a parallel Bazel build moves these numbers by more
than most regressions do. The comparison's default threshold is 1.35x and it
ignores anything under 5 ms, so it does not fire on run-to-run noise.

`tessellate_ms` reads zero for scripts whose last boolean routed to the mesh
kernel: that path already holds triangles, so tessellation is a lookup.

`build_support/bench/cadpole-opt` runs the optimized `cadpole`, whatever
configuration the working tree built last; use it when timing the CLI by hand.

**Bench cases.** Besides the examples, the baseline records the scripts under
`src/rust/cadpole-bench/cases/`: shapes reduced until only one cost is left in
them. `record.py`'s `SWEEPS` runs those at several sizes, because some
regressions are a *shape* rather than a number: `boolean_fanout.star` is one
body and `n` tools in one `subtract`, and whether that is quadratic or linear
in `n` only shows across several values of `n`. Add a sweep whenever "how
does this scale" is the question.

**Proving a change did not move the geometry.** `cadpole-bench --signature`
prints, per visible body, its mesh as a digest, its volume, whether it is
watertight, and digests of every face, edge and vertex name; `--long` prints
the names themselves, `--param k=v` sets a parameter. Run it with the binary
from before a change and with the binary from after, and diff. It is not a
golden file: it compares two builds on one machine, so nothing in it depends
on float formatting agreeing across platforms.

The baseline is not a test target: wall-clock timing inside a sandboxed,
parallel Bazel action measures the machine's load, not the code.
