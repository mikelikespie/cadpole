#!/usr/bin/env bash
# Reclaim Bazel build output for workspaces that are finished with.
#
#   build_support/bazel-gc.sh            # say what would go, delete nothing
#   build_support/bazel-gc.sh --delete   # do it
#
# Bazel derives an output base from the workspace *path*, so every git worktree
# gets its own, and a full build of this repo is about 7 GB. Bazel never cleans
# them up. This finds the ones nobody can be using and removes them.
#
# Two categories, both safe:
#
#   orphaned   the workspace directory named in DO_NOT_BUILD_HERE is gone, so
#              no Bazel invocation can ever address this output base again.
#   merged     the workspace is a git worktree under .claude/worktrees whose
#              branch is an ancestor of origin/main, so its work has landed.
#
# A worktree with uncommitted changes or unmerged commits is never touched, and
# neither is one whose Bazel server is still running -- a running server means
# someone may be mid-build, and a clean tree is not proof they have finished.
# Only an orphaned base, whose workspace directory no longer exists and which
# nothing can address again, is shut down and removed while its server lives.
# What is deleted is build output: the cost of being wrong is a rebuild, which
# the shared disk cache in .bazelrc makes cheap.

set -uo pipefail

REPO="$(cd "$(dirname "${BASH_SOURCE[0]}")/.." && pwd)"
# The output user root: $BAZEL_OUTPUT_USER_ROOT, else the parent of this
# workspace's output base (which follows a custom --output_user_root), else
# Bazel's default.
ROOT="${BAZEL_OUTPUT_USER_ROOT:-}"
if [[ -z "$ROOT" ]]; then
  ob="$(cd "$REPO" && bazel info output_base 2>/dev/null)"
  ROOT="${ob:+$(dirname "$ob")}"
fi
ROOT="${ROOT:-${HOME}/.cache/bazel/_bazel_${USER:-$(id -un)}}"
DELETE=0
[[ "${1:-}" == "--delete" ]] && DELETE=1

[[ -d "$ROOT" ]] || { echo "no Bazel cache at $ROOT"; exit 0; }

live_servers() { ps ax -o args= 2>/dev/null | grep -o 'output_base=[^ ]*' | sed 's|.*/||' | sort -u; }
SERVERS="$(live_servers)"

human() { du -sh "$1" 2>/dev/null | cut -f1; }

total_kb=0
victims=()
unpushed=()

for base in "$ROOT"/*/; do
  name="$(basename "$base")"
  # Only output bases (an MD5 of the workspace path): a custom output root may
  # hold other directories, and those are not ours to judge.
  [[ "$name" =~ ^[0-9a-f]{32}$ ]] || continue
  ws="$(cat "$base/DO_NOT_BUILD_HERE" 2>/dev/null)"
  reason=""

  if [[ -z "$ws" || ! -d "$ws" ]]; then
    reason="orphaned (${ws:-no workspace recorded})"
  elif [[ "$ws" == */.claude/worktrees/* ]]; then
    # A running server means a build may still be under way; a tree that has
    # just been committed looks clean from outside.
    if grep -qx "$name" <<<"$SERVERS"; then
      continue
    fi
    # A worktree is finished with when it is clean and its branch has landed.
    if [[ -n "$(git -C "$ws" status --porcelain 2>/dev/null)" ]]; then
      continue
    fi
    head="$(git -C "$ws" rev-parse HEAD 2>/dev/null)" || continue
    if git -C "$REPO" merge-base --is-ancestor "$head" origin/main 2>/dev/null; then
      reason="merged worktree ($(git -C "$ws" rev-parse --abbrev-ref HEAD 2>/dev/null))"
    elif git -C "$REPO" merge-base --is-ancestor "$head" main 2>/dev/null; then
      # Merged locally but not pushed. Deliberately not reclaimed (the work
      # must be durably on the remote), but reported: a push is often the
      # largest easy win.
      unpushed+=("$(git -C "$ws" rev-parse --abbrev-ref HEAD 2>/dev/null) [$(human "$base")]")
      continue
    else
      continue
    fi
  else
    continue
  fi

  size="$(human "$base")"
  kb="$(du -sk "$base" 2>/dev/null | cut -f1)"
  total_kb=$(( total_kb + ${kb:-0} ))
  printf '%-7s %-40s %s\n' "$size" "$reason" "$name"
  victims+=("$base")
done

report_unpushed() {
  [[ ${#unpushed[@]} -eq 0 ]] && return
  printf '\n%d worktree(s) merged into local main but NOT pushed, so not reclaimable yet:\n' "${#unpushed[@]}"
  printf '  %s\n' "${unpushed[@]}"
  echo "  push main, then re-run: their output bases are the largest easy win."
}

if [[ ${#victims[@]} -eq 0 ]]; then
  echo "nothing to reclaim"
  report_unpushed
  exit 0
fi

printf '\n%s GB in %d output base(s)\n' "$(( total_kb / 1024 / 1024 ))" "${#victims[@]}"
report_unpushed
if [[ $DELETE -eq 0 ]]; then
  echo "re-run with --delete to remove them"
  exit 0
fi

for base in "${victims[@]}"; do
  name="$(basename "$base")"
  if grep -qx "$name" <<<"$SERVERS"; then
    echo "shutting down the server for $name"
    bazel --output_base="$base" shutdown >/dev/null 2>&1
  fi
  # Bazel leaves output directories read-only; rm alone cannot remove them.
  chmod -R u+w "$base" 2>/dev/null
  rm -rf "$base"
  echo "removed $name"
done

df -h "$ROOT" | tail -1
