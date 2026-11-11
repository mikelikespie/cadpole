#!/bin/bash
# Copy the freshly generated attribution file over the committed one.
#
#   bazel run //build_support/licenses:attribution_update
#
# Then `bazel test //build_support/licenses:attribution_test` passes again and
# the diff is the licence review, the same way `refresh.py`'s diff is.
set -euo pipefail

if [[ -z "${BUILD_WORKSPACE_DIRECTORY:-}" ]]; then
  echo "run me with \`bazel run\`, not directly" >&2
  exit 2
fi

src="${CADPOLE_ATTRIBUTION_FILE}"
if [[ ! -f "$src" ]]; then
  echo "missing generated file: $src" >&2
  exit 1
fi

install -m 0644 "$src" "${BUILD_WORKSPACE_DIRECTORY}/THIRD-PARTY-NOTICES.md"
echo "updated THIRD-PARTY-NOTICES.md"
