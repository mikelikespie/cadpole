#!/usr/bin/env bash
# bazel run --config=opt //site:render -- <set> [...]: see render.py.
set -euo pipefail
cadpole="$(pwd)/$1"
shift
ws="${BUILD_WORKSPACE_DIRECTORY:?run with bazel run}"
exec python3 "$ws/site/rendering/render.py" --cadpole "$cadpole" --workspace "$ws" "$@"
