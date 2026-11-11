#!/bin/bash
# Copy the freshly generated *.pb.swift over the committed ones.
#
#   bazel run //src/apple/CadpoleProto:regenerate
#
# Then `bazel test //src/apple/CadpoleProto:pb_swift_diff_test` passes again and
# the diff is the schema review.
set -euo pipefail

if [[ -z "${BUILD_WORKSPACE_DIRECTORY:-}" ]]; then
  echo "run me with \`bazel run\`, not directly" >&2
  exit 2
fi

dest="${BUILD_WORKSPACE_DIRECTORY}/src/apple/CadpoleProto/cadpole/v1"
mkdir -p "$dest"
for f in ${CADPOLE_PB_SWIFT_FILES}; do
  src="src/apple/CadpoleProto/_generated/cadpole/v1/$f"
  if [[ ! -f "$src" ]]; then
    echo "missing generated file: $src" >&2
    exit 1
  fi
  install -m 0644 "$src" "$dest/$f"
  echo "updated src/apple/CadpoleProto/cadpole/v1/$f"
done
