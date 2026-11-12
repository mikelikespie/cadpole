#!/usr/bin/env bash
# Runs a Rust test binary (the libtest harness) as one shard of a Bazel test.
#
#   libtest_sharded.sh <test binary> [libtest flags...]
#
# libtest knows nothing of Bazel's sharding, and rules_rust's rust_test does
# not either, so a sharded target is an sh_test around the binary: it lists
# the binary's tests in libtest's (sorted) order and runs every
# TEST_TOTAL_SHARDS-th one, starting at TEST_SHARD_INDEX. Taking them in turn
# rather than in blocks spreads neighbouring slow tests, which tend to share a
# name prefix, over different shards. Unsharded, it runs the binary as is.
set -euo pipefail

bin="$1"
shift

if [[ -z "${TEST_TOTAL_SHARDS:-}" ]]; then
  exec "$bin" "$@"
fi
# Tells Bazel this runner understands sharding.
if [[ -n "${TEST_SHARD_STATUS_FILE:-}" ]]; then
  touch "$TEST_SHARD_STATUS_FILE"
fi

listing="$("$bin" --list --format terse)"
names=()
i=0
while IFS= read -r line; do
  [[ "$line" == *": test" ]] || continue
  if (( i % TEST_TOTAL_SHARDS == TEST_SHARD_INDEX )); then
    names+=("${line%: test}")
  fi
  i=$((i + 1))
done <<<"$listing"

if (( ${#names[@]} == 0 )); then
  echo "shard $TEST_SHARD_INDEX of $TEST_TOTAL_SHARDS: no tests ($i in all)"
  exit 0
fi
echo "shard $TEST_SHARD_INDEX of $TEST_TOTAL_SHARDS: ${#names[@]} of $i tests"
exec "$bin" --exact "$@" "${names[@]}"
