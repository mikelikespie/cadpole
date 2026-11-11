#!/usr/bin/env bash
# The standard library rides inside the wasm engine: `cadpole-script`
# `include_str!`s every `//stdlib/*.star` module, and the wasm build compiles
# that same crate, so the module sources are bytes in the `.wasm`. This test
# looks for them, so a build that stopped embedding them -- a `compile_data`
# dropped, a module missing from `MODULES` -- fails here rather than in a
# browser with "no standard library module named ...".
set -euo pipefail

wasm=""
for f in "$@"; do
  case "$f" in
    *_bg.wasm) wasm="$f" ;;
  esac
done
if [ -z "$wasm" ]; then
  echo "no *_bg.wasm among the data files: $*" >&2
  exit 1
fi

status=0
for label in "@cadpole/gears.star" "def involute_profile(" "def planetary(" "@cadpole/mechanical.star" "def thread(" "def spring(" "@cadpole/site.star" "def envelope_above_grade(" "def parcel("; do
  if grep -a -q -F "$label" "$wasm"; then
    echo "ok: $label is in $(basename "$wasm")"
  else
    echo "MISSING: $label is not in $(basename "$wasm")" >&2
    status=1
  fi
done
exit $status
