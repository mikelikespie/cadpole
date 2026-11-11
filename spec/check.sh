#!/usr/bin/env bash
# Run the model checks through Bazel.
#
#   spec/check.sh                           # quick tier: bazel test //spec/...
#   SAMPLES=20000 STEPS=60 spec/check.sh    # deep tier: bazel run //spec:quint_check
#   SEED=0x... spec/check.sh                # run a reported trace again
#   TRACES=1000 spec/check.sh               # longer replays against the programs
set -euo pipefail

here="$(cd "$(dirname "${BASH_SOURCE[0]}")" && pwd)"
cd "$here/.."

if [[ -z "${TRACES:-}" && ( -n "${SAMPLES:-}" || -n "${STEPS:-}" || -n "${SEED:-}" ) ]]; then
  args=()
  [[ -n "${SAMPLES:-}" ]] && args+=("--samples=${SAMPLES}")
  [[ -n "${STEPS:-}" ]] && args+=("--steps=${STEPS}")
  [[ -n "${SEED:-}" ]] && args+=("--seed=${SEED}")
  exec bazel run //spec:quint_check -- "${args[@]}"
fi

if [[ -n "${TRACES:-}" ]]; then
  bazel run //spec/replay:listener_replay -- "--traces=${TRACES}" "--steps=${STEPS:-40}"
  bazel run //spec/replay:transport_replay -- "--traces=${TRACES}" "--steps=${STEPS:-40}"
  exec bazel run //spec/replay:wakeups_replay -- "--traces=${TRACES}" "--steps=${STEPS:-40}"
fi

exec bazel test //spec/... --test_output=all
