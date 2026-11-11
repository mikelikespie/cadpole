#!/usr/bin/env bash
# Make a GitHub-hosted Ubuntu runner fit Cadpole's toolchains. Run it before
# setup-bazel in the CI workflow.
#
# Without it, CI dies in the repository-fetch phase with "No space left on
# device" before a single action runs. The arithmetic:
#
#   * toolchains_llvm fetches LLVM-20.1.8-Linux-X64.tar.xz: 2.0 GB, and the
#     official distribution unpacks to ~11 GB (bin/ 8.1 GB, lib/ 2.7 GB -- it
#     ships flang, mlir, lldb and clangd next to the clang and lld Cadpole uses),
#     so the fetch needs ~13 GB while the tarball copy and the tree coexist;
#   * the pinned Swift toolchain for the Apple-on-Linux tests is 1.1 GB, and
#     unpacks to 3.3 GB;
#   * setup-bazel restores a ~5 GB repository cache onto the same disk first;
#   * an ubuntu-24.04 runner starts with roughly 20 GB free on /, and carries
#     ~30 GB of SDKs (Android, .NET, Haskell, CodeQL, ...) Cadpole never uses.
#
# So: remove those SDKs; put Bazel's output base on the runner's scratch disk
# (/mnt) when that has more room than $HOME; print df before and after, so the
# numbers are in every log. And lift Ubuntu 24.04's AppArmor restriction on
# unprivileged user namespaces: without it Bazel cannot register linux-sandbox,
# falls back to processwrapper-sandbox without a word, and every
# `block-network` tag in the tree is ignored (--config=hermetic in .bazelrc,
# //build_support:network_is_blocked_test).
#
# Deletes nothing unless GITHUB_ACTIONS=true; `--dry-run` prints the plan
# anywhere.
set -euo pipefail

dry_run=0
if [[ "${1:-}" == "--dry-run" ]]; then
  dry_run=1
elif [[ "${GITHUB_ACTIONS:-}" != "true" ]]; then
  echo "not a GitHub-hosted runner: dry run only" >&2
  dry_run=1
fi

run() {
  if (( dry_run )); then
    echo "would run: $*"
  else
    echo "+ $*"
    "$@"
  fi
}

echo "== disk before"
df -h / "$HOME" /mnt 2>/dev/null || true

# 1. SDKs the image carries and Cadpole never touches. Paths are the ones in the
#    ubuntu-24.04 image; a path that is not there is simply skipped. Bazel is
#    Bazelisk's own download, Node and Chromium are Bazel-fetched and pinned,
#    Swift is the pinned swift.org toolchain, so none of this is used.
for d in \
    /usr/local/lib/android \
    /usr/share/dotnet \
    /usr/local/.ghcup \
    /opt/ghc \
    /opt/hostedtoolcache/CodeQL \
    /usr/share/swift \
    /usr/local/share/powershell \
    /usr/local/share/chromium \
    /usr/lib/google-cloud-sdk \
    /opt/az \
    /opt/microsoft \
    /usr/share/miniconda \
    /usr/local/julia*; do
  if [[ -e "$d" ]]; then
    run sudo rm -rf "$d"
  fi
done
if command -v docker >/dev/null 2>&1; then
  run docker image prune --all --force
fi

# 2. The output base. setup-bazel puts it at $HOME/.bazel; that is where the
#    ~11 GB LLVM tree, the ~1.5 GB of Rust toolchains and every build output
#    land. Azure runners mount a scratch disk on /mnt that is often larger
#    than what is left on /; use it when it is.
if [[ -d /mnt && ! -e "$HOME/.bazel" ]]; then
  home_free=$(df --output=avail -k "$HOME" | tail -1)
  mnt_free=$(df --output=avail -k /mnt | tail -1)
  if (( mnt_free > home_free )); then
    echo "/mnt has $((mnt_free / 1024 / 1024)) GB free, $HOME $((home_free / 1024 / 1024)) GB: output base goes on /mnt"
    run sudo mkdir -p /mnt/bazel
    run sudo chown "$(id -u):$(id -g)" /mnt/bazel
    run ln -s /mnt/bazel "$HOME/.bazel"
  else
    echo "/mnt has $((mnt_free / 1024 / 1024)) GB free, $HOME $((home_free / 1024 / 1024)) GB: output base stays in $HOME"
  fi
fi

# 3. Let Bazel have linux-sandbox, so `block-network` means something.
if [[ -e /proc/sys/kernel/apparmor_restrict_unprivileged_userns ]]; then
  echo "kernel.apparmor_restrict_unprivileged_userns = $(cat /proc/sys/kernel/apparmor_restrict_unprivileged_userns)"
  run sudo sysctl -w kernel.apparmor_restrict_unprivileged_userns=0
fi

echo "== disk after"
df -h / "$HOME" /mnt 2>/dev/null || true
