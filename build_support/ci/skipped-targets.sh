#!/usr/bin/env bash
# What the Linux sweep does not run, and whether that set has changed.
#
# The sweep is `bazel test --build_tests_only -- $SWEEP` (below; CLAUDE.md,
# docs/getting-started.md and the exporter's standalone check use the same
# patterns), which discovers new tests on its own -- with two silent
# exceptions. A test tagged `manual` is not
# matched by a wildcard, and a test under an excluded pattern is not either.
# Both are legitimate; neither should happen by accident: a `manual` tag added
# to make a red test go away, or a Linux-only test living under a subtracted
# directory, would never be noticed.
#
# So the set is written down here, and this fails when reality diverges. To
# change it, change EXPECTED and say why in the commit message -- the diff is
# the review, the same way it is for the licence inventories.
#
#   bazel run //build_support/ci:skipped_targets
#
# SWEEP may be overridden from the environment to check another pattern.
set -euo pipefail
cd "${BUILD_WORKSPACE_DIRECTORY:-$(dirname "$0")/../..}"

# The two app targets fail toolchain resolution during *analysis* on Linux
# (no macOS execution platform), which aborts the whole invocation rather than
# skipping them, so they are subtracted by name. Everything else under
# //src/apple either builds on Linux (CadpoleKit, the wire-format tests, the
# pb.swift staleness diffs, which only run on Linux) or is skipped as
# incompatible.
SWEEP="${SWEEP:-//... -//src/apple/CadpoleiOS:CadpoleiOS -//src/apple/CadpoleMac:CadpoleMac}"

# Tests the sweep will not run, with the reason each one is exempt.
#
#   //src/apple/CadpoleKitTests:SimulatorTests
#       needs the iOS Simulator on the Mac worker (--config=apple-remote).
#   //src/apple/CadpoleLocalEngine:LocalEngineTests
#       macOS-only (target_compatible_with), the FFI round trip.
#   //src/web:ts_transitive_typecheck_test
#       typechecks every transitive .d.ts; minutes, and //src/web:ts_typecheck_test
#       covers our own sources on every run.
EXPECTED="//src/apple/CadpoleKitTests:SimulatorTests
//src/apple/CadpoleLocalEngine:LocalEngineTests
//src/web:ts_transitive_typecheck_test"

# The sweep's patterns as a query expression: `-x` is `except x`.
expr=""
for p in $SWEEP; do
  case "$p" in
    -*) expr="$expr except ${p#-}" ;;
    *) expr="${expr:+$expr + }$p" ;;
  esac
done

query() {
  local out
  if ! out="$(bazel query --noshow_progress "$1" 2>&1)"; then
    echo "::error::bazel query failed: $1" >&2
    printf '%s\n' "$out" >&2
    exit 1
  fi
  printf '%s\n' "$out" | grep '^//' | sort || true
}
manual="$(query 'attr(tags, "manual", tests(//...))')"
excluded="$(query "tests(//...) except tests($expr)")"
actual="$(printf '%s\n%s\n' "$manual" "$excluded" | sed '/^$/d' | sort -u)"
expected="$(printf '%s\n' "$EXPECTED" | sort)"

if [[ "$actual" != "$expected" ]]; then
  echo "::error::the set of tests the sweep does not run has changed"
  echo "sweep: $SWEEP"
  diff <(printf '%s\n' "$expected") <(printf '%s\n' "$actual") \
    --label expected --label actual -u || true
  echo
  echo "A test tagged 'manual', or under a pattern the sweep subtracts, does not"
  echo "run. If that is intended, add it to EXPECTED in"
  echo "build_support/ci/skipped-targets.sh with the reason."
  exit 1
fi

echo "sweep: $SWEEP"
echo "tests it does not run, as expected:"
printf '  %s\n' $actual
