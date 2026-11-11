#!/usr/bin/env bash
# Passes only when the network is unreachable.
#
# This test is tagged `block-network`. Under linux-sandbox that means a fresh
# network namespace with only loopback, and every connect() below fails at
# once. Under processwrapper-sandbox -- what Bazel falls back to, silently,
# on a host where it cannot create user namespaces -- there is no namespace
# at all, the connects succeed, and so would a download from inside
# //spec:quint_test or the Playwright suite. This turns that silent
# degradation into a failing test. It is only compatible under
# --config=hermetic (see BUILD.bazel), which also makes the fallback itself an
# error.
set -u

reached=()
for target in 1.1.1.1/443 8.8.8.8/53; do
  # bash's /dev/tcp: no curl, no DNS. Five seconds covers a host whose
  # firewall drops rather than refuses.
  if timeout 5 bash -c "exec 3<>/dev/tcp/$target" 2>/dev/null; then
    reached+=("$target")
  fi
done

if (( ${#reached[@]} > 0 )); then
  cat >&2 <<MSG
This block-network test reached ${reached[*]}: the sandbox in use does not
block the network. Bazel registers linux-sandbox only where unprivileged user
namespaces work; on Ubuntu 24.04 check

    sysctl kernel.apparmor_restrict_unprivileged_userns    # must be 0

and the "processes:" line of the build output, which should say linux-sandbox.
MSG
  exit 1
fi
echo "network unreachable, as it should be"
