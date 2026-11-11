#!/usr/bin/env bash
# How old are the vendored advisory snapshots?
#
# rustsec.tsv and npm.tsv are refreshed by hand (refresh.py) and read by a
# test that does no I/O -- but a snapshot nobody
# refreshes is a database that knows nothing about this year's advisories,
# and nothing in the test can tell. CI runs this instead: it prints each
# snapshot's age from the `# Taken:` line refresh.py writes, warns past 30
# days and fails past 90. It reads the clock, which is why it is a CI step and
# not a Bazel test.
set -euo pipefail
cd "$(dirname "$0")"

status=0
now=$(date -u +%s)
for f in rustsec.tsv npm.tsv; do
  taken=$(sed -n 's/^# Taken: *//p' "$f" | head -1)
  if [[ -z "$taken" ]]; then
    echo "::error::build_support/advisories/$f has no '# Taken:' header; run build_support/advisories/refresh.py"
    status=1
    continue
  fi
  age=$(( (now - $(date -u -d "$taken" +%s)) / 86400 ))
  echo "build_support/advisories/$f: taken $taken, $age days ago"
  if (( age > 90 )); then
    echo "::error::build_support/advisories/$f is $age days old; run build_support/advisories/refresh.py and commit the diff"
    status=1
  elif (( age > 30 )); then
    echo "::warning::build_support/advisories/$f is $age days old; run build_support/advisories/refresh.py and commit the diff"
  fi
done
exit $status
