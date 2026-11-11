#!/bin/bash
# Asserts the Swift toolchain Bazel selected on Linux is the pinned one, not
# whatever `swiftc` happens to be on PATH. The generated toolchain repo writes
# `swiftc -version` output to swift_version; MODULE.bazel pins the version.
set -euo pipefail
version_file="$1"; want="$2"
got="$(cat "$version_file")"
case "$got" in
  *"Swift version $want"*|*"swiftlang-$want"*) echo "ok: $got"; exit 0 ;;
esac
echo "pinned Swift $want but the selected toolchain reports: $got" >&2
exit 1
