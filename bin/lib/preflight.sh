# shellcheck shell=sh
#
# Checks that this machine can build and run Cadpole, sourced by every script
# in bin/. Each check prints a line to stderr, ✓ or ✗, and each ✗ is followed
# by the command that fixes it. Nothing here installs anything.
#
#   preflight        the quick form, at the start of every bin/ script: silent
#                    when a stamp says this machine already passed; otherwise
#                    the required checks, and on any failure the whole report
#   preflight_report the whole report, optional tools included (bin/doctor)
#
# The caller sets CADPOLE_ROOT to the checkout. For tests, CADPOLE_UNAME_S and
# CADPOLE_UNAME_M stand in for `uname -s` and `uname -m`, and CADPOLE_OS_RELEASE
# for /etc/os-release, and CADPOLE_FIRST_BUILD_GIB for the free space required.

# Free space a first build needs: Bazel's output base, its repository cache
# and the disk cache together, in GiB, rounded up.
PF_FIRST_BUILD_GIB=${CADPOLE_FIRST_BUILD_GIB:-25}
PF_XCODE_MAJOR=27

pf_platform() {
    PF_OS=${CADPOLE_UNAME_S:-$(uname -s)}
    PF_ARCH=${CADPOLE_UNAME_M:-$(uname -m)}
    case $PF_ARCH in
        x86_64 | amd64) PF_ARCH=x86_64 PF_GOARCH=amd64 ;;
        aarch64 | arm64) PF_ARCH=arm64 PF_GOARCH=arm64 ;;
        *) PF_GOARCH=$PF_ARCH ;;
    esac
}

pf_stamp_file() {
    printf '%s/cadpole/preflight-ok' "${XDG_CACHE_HOME:-${HOME:-/tmp}/.cache}"
}

# What the stamp is keyed on: OS and architecture, which `bazel` is first on
# PATH and that file's size and time (an upgrade of Bazel or Bazelisk changes
# them), and the pinned version.
pf_stamp_key() {
    pf_bazel_path=$(command -v bazel 2>/dev/null) || pf_bazel_path=none
    pf_bazel_stat=
    [ "$pf_bazel_path" = none ] || pf_bazel_stat=$(ls -lLn "$pf_bazel_path" 2>/dev/null)
    printf 'v1|%s|%s|%s|%s|%s\n' "$PF_OS" "$PF_ARCH" "$pf_bazel_path" "$pf_bazel_stat" \
        "$(cat "$CADPOLE_ROOT/.bazelversion" 2>/dev/null)"
}

pf_reset() {
    PF_REPORT=
    PF_FAILED=0
}

# pf_line MARK TEXT [FIX...]: one checklist line and its fix commands.
pf_line() {
    pf_mark=$1 pf_text=$2
    shift 2
    PF_REPORT="$PF_REPORT  $pf_mark $pf_text
"
    for pf_fix in "$@"; do
        PF_REPORT="$PF_REPORT      $pf_fix
"
    done
}
pf_ok() { pf_line "✓" "$@"; }
pf_fail() {
    PF_FAILED=1
    pf_line "✗" "$@"
}
pf_optional() {
    pf_text=$1
    shift
    pf_line "✗" "optional: $pf_text" "$@"
}

# Which Linux package manager to name: Debian/Ubuntu, Fedora/RHEL or Arch.
pf_distro() {
    pf_ids=
    pf_release=${CADPOLE_OS_RELEASE:-/etc/os-release}
    if [ -r "$pf_release" ]; then
        pf_ids=$(sed -n -e 's/^ID=//p' -e 's/^ID_LIKE=//p' "$pf_release" | tr -d '"' | tr '\n' ' ')
    fi
    case " $pf_ids " in
        *" debian "* | *" ubuntu "*) PF_DISTRO=apt ;;
        *" fedora "* | *" rhel "* | *" centos "*) PF_DISTRO=dnf ;;
        *" arch "*) PF_DISTRO=pacman ;;
        *) PF_DISTRO=unknown ;;
    esac
}

# How a package install is run here: plain as root, through sudo where there
# is one, and otherwise by someone who is root.
pf_sudo() {
    PF_SUDO='' PF_AS_ROOT=''
    if [ "$(id -u 2>/dev/null)" = 0 ]; then
        return
    fi
    if command -v sudo >/dev/null 2>&1; then
        PF_SUDO="sudo "
    else
        PF_AS_ROOT="   # as root: there is no sudo here"
    fi
}

# pf_pkg APT DNF PACMAN: the install command for this distribution.
pf_pkg() {
    pf_sudo
    case $PF_DISTRO in
        apt) printf '%sapt install %s%s' "$PF_SUDO" "$1" "$PF_AS_ROOT" ;;
        dnf) printf '%sdnf install %s%s' "$PF_SUDO" "$2" "$PF_AS_ROOT" ;;
        pacman) printf '%spacman -S %s%s' "$PF_SUDO" "$3" "$PF_AS_ROOT" ;;
        *) printf 'install %s (apt), %s (dnf) or %s (pacman)%s' "$1" "$2" "$3" "$PF_AS_ROOT" ;;
    esac
}

pf_bazelisk_fix() {
    case $PF_OS in
        Darwin)
            if command -v brew >/dev/null 2>&1; then
                printf '%s\n' "brew install bazelisk"
            else
                printf '%s\n' "brew install bazelisk   (after installing Homebrew from https://brew.sh), or put" \
                    "bazelisk-darwin-arm64 from https://github.com/bazelbuild/bazelisk/releases on PATH as bazel"
            fi
            ;;
        *)
            if ! command -v curl >/dev/null 2>&1; then
                pf_distro
                pf_pkg curl curl curl
                printf '\n'
            fi
            printf '%s\n' "mkdir -p ~/.local/bin" \
                "curl -fsSL -o ~/.local/bin/bazel https://github.com/bazelbuild/bazelisk/releases/latest/download/bazelisk-linux-$PF_GOARCH" \
                "chmod +x ~/.local/bin/bazel   # then put ~/.local/bin on PATH" \
                "(other ways: https://github.com/bazelbuild/bazelisk#installation)"
            ;;
    esac
}

# pf_fail_lines TEXT FIXES: a failure whose fixes are the lines of FIXES.
pf_fail_lines() {
    pf_text=$1
    pf_fixes=$2
    PF_FAILED=1
    PF_REPORT="$PF_REPORT  ✗ $pf_text
"
    while IFS= read -r pf_fix; do
        PF_REPORT="$PF_REPORT      $pf_fix
"
    done <<EOF
$pf_fixes
EOF
}

pf_check_platform() {
    case $PF_OS/$PF_ARCH in
        Darwin/arm64) pf_ok "macOS on Apple silicon" ;;
        Linux/x86_64 | Linux/arm64) pf_ok "Linux $PF_ARCH" ;;
        Darwin/*) pf_fail "macOS on $PF_ARCH is not supported: Cadpole builds on Apple silicon Macs only" ;;
        MINGW* | MSYS* | CYGWIN*) pf_fail "Windows is not supported" "use WSL 2 with a Linux x86_64 distribution, and run this there" ;;
        *) pf_fail "$PF_OS $PF_ARCH is not supported: Cadpole builds on macOS (Apple silicon) and Linux (x86_64, arm64)" ;;
    esac
}

pf_check_bazel() {
    pf_want=$(tr -d ' \r\n' <"$CADPOLE_ROOT/.bazelversion" 2>/dev/null)
    if ! pf_path=$(command -v bazel 2>/dev/null); then
        pf_fail_lines "bazel not found: install Bazelisk as \`bazel\` (it fetches Bazel $pf_want)" "$(pf_bazelisk_fix)"
        return
    fi
    # Bazelisk reads .bazelversion from the working directory and answers
    # with the version it runs, so a match means Bazelisk or the right Bazel.
    pf_got=$(cd "$CADPOLE_ROOT" && bazel --version 2>/dev/null </dev/null | sed -n 's/^bazel //p' | head -n 1)
    if [ -z "$pf_got" ]; then
        pf_fail_lines "\`$pf_path --version\` failed (Bazelisk needs the network the first time)" "$(pf_bazelisk_fix)"
        return
    fi
    pf_kind=Bazel
    pf_real=$pf_path
    while [ -L "$pf_real" ]; do
        pf_link=$(readlink "$pf_real")
        case $pf_link in /*) pf_real=$pf_link ;; *) pf_real=$(dirname "$pf_real")/$pf_link ;; esac
    done
    # Bazelisk under any name: its module path is in the binary (or, for the
    # npm package, its package name in the script); Bazel's own has neither.
    case $pf_real in
        *bazelisk*) pf_kind=Bazelisk ;;
        *) if grep -qaE 'bazelbuild/bazelisk|@bazel/bazelisk' "$pf_real" 2>/dev/null; then pf_kind=Bazelisk; fi ;;
    esac
    if [ "$pf_got" = "$pf_want" ]; then
        pf_ok "bazel $pf_got ($pf_kind, $pf_path)"
    else
        pf_fail_lines "$pf_path is $pf_kind $pf_got, but this checkout pins $pf_want (.bazelversion): replace it with Bazelisk" "$(pf_bazelisk_fix)"
    fi
}

pf_check_macos() {
    if ! pf_dev=$(xcode-select -p 2>/dev/null); then
        pf_fail "Xcode not found" "install Xcode $PF_XCODE_MAJOR from the App Store, then:" \
            "sudo xcode-select -s /Applications/Xcode.app"
        return
    fi
    case $pf_dev in
        *CommandLineTools*)
            pf_fail "xcode-select points at the Command Line Tools; Bazel needs Xcode $PF_XCODE_MAJOR" \
                "install Xcode from the App Store, then: sudo xcode-select -s /Applications/Xcode.app"
            return
            ;;
    esac
    pf_xcode=$(xcodebuild -version 2>/dev/null | sed -n 's/^Xcode //p' | head -n 1)
    pf_major=${pf_xcode%%.*}
    case $pf_major in
        '' | *[!0-9]*) pf_fail "xcodebuild -version failed for $pf_dev" "sudo xcode-select -s /Applications/Xcode.app" ;;
        *)
            if [ "$pf_major" -ge "$PF_XCODE_MAJOR" ]; then
                pf_ok "Xcode $pf_xcode ($pf_dev)"
            else
                pf_fail "Xcode $pf_xcode; Cadpole needs Xcode $PF_XCODE_MAJOR or later" "update Xcode from the App Store"
            fi
            ;;
    esac
    if xcodebuild -license check >/dev/null 2>&1; then
        pf_ok "Xcode licence accepted"
    else
        pf_fail "Xcode licence not accepted" "sudo xcodebuild -license accept"
    fi
}

pf_check_brew() {
    if command -v brew >/dev/null 2>&1; then
        pf_ok "Homebrew"
    else
        pf_optional "Homebrew not found (the simplest way to install Bazelisk and ibazel)" "see https://brew.sh"
    fi
}

pf_check_linux() {
    pf_distro
    pf_cxx=
    for pf_c in g++ c++; do
        if command -v "$pf_c" >/dev/null 2>&1; then
            pf_cxx=$pf_c
            break
        fi
    done
    pf_gxx_fix=$(pf_pkg g++ gcc-c++ gcc)
    if [ -z "$pf_cxx" ]; then
        pf_fail "no C++ compiler: Bazel's clang builds against the system C++ headers" "$pf_gxx_fix"
    elif printf '#include <cstdio>\n#include <string>\n' | "$pf_cxx" -fsyntax-only -x c++ - >/dev/null 2>&1; then
        pf_ok "C++ headers ($pf_cxx)"
    else
        pf_fail "$pf_cxx cannot compile against the C++ standard headers" "$pf_gxx_fix"
    fi
    if command -v git >/dev/null 2>&1; then
        pf_ok "git"
    else
        pf_fail "git not found" "$(pf_pkg git git git)"
    fi
}

pf_check_disk() {
    if [ -x "$CADPOLE_ROOT/bazel-opt-bin/src/rust/cadpole-cli/cadpole" ]; then
        return
    fi
    pf_out=$(cd "$CADPOLE_ROOT" && pwd -P)
    pf_free=$(df -Pk "$pf_out" 2>/dev/null | awk 'NR == 2 { print int($4 / 1048576) }')
    case $pf_free in
        '' | *[!0-9]*) return ;;
    esac
    if [ "$pf_free" -ge "$PF_FIRST_BUILD_GIB" ]; then
        pf_ok "$pf_free GiB free (a first build takes about $PF_FIRST_BUILD_GIB GiB)"
    else
        pf_fail "$pf_free GiB free; a first build takes about $PF_FIRST_BUILD_GIB GiB" \
            "free some space, or point Bazel elsewhere: startup --output_user_root=/a/bigger/disk in ~/.bazelrc"
    fi
}

# version_ge A B: A >= B, both dotted numbers.
pf_version_ge() {
    awk -v a="$1" -v b="$2" 'BEGIN {
        na = split(a, x, "."); nb = split(b, y, ".");
        for (i = 1; i <= (na > nb ? na : nb); i++) {
            if (x[i] + 0 > y[i] + 0) exit 0;
            if (x[i] + 0 < y[i] + 0) exit 1;
        }
        exit 0
    }'
}

pf_check_optional() {
    if [ "$PF_OS" = Darwin ]; then
        pf_check_brew
    fi
    if command -v ibazel >/dev/null 2>&1; then
        pf_ok "ibazel (for bin/dev)"
    else
        case $PF_OS in
            Darwin) pf_optional "ibazel not found (only bin/dev uses it)" "brew install ibazel" ;;
            *) pf_optional "ibazel not found (only bin/dev uses it)" \
                "download ibazel_linux_$PF_GOARCH from https://github.com/bazelbuild/bazel-watcher/releases onto PATH as ibazel" ;;
        esac
    fi
    pf_min=$(sed -n 's/^export const MIN_CLAUDE_VERSION = "\([^"]*\)";.*/\1/p' \
        "$CADPOLE_ROOT/src/tools/listener/driver.mjs" 2>/dev/null)
    if pf_claude=$(command -v claude 2>/dev/null); then
        pf_cv=$(claude --version 2>/dev/null </dev/null | sed -n 's/^\([0-9][0-9.]*\).*/\1/p' | head -n 1)
        if [ -z "$pf_min" ] || { [ -n "$pf_cv" ] && pf_version_ge "$pf_cv" "$pf_min"; }; then
            pf_ok "claude ${pf_cv:-?} ($pf_claude)"
        else
            pf_optional "claude ${pf_cv:-of unknown version}; the listener needs $pf_min or later" "claude update"
        fi
    else
        pf_optional "claude not found (for the MCP server and the listener; docs/assistant-integration.md)" \
            "https://docs.claude.com/en/docs/claude-code/setup"
    fi
    if pf_on_path=$(command -v cadpole 2>/dev/null); then
        pf_ok "cadpole on PATH ($pf_on_path)"
    else
        pf_optional "cadpole is not on PATH" "$CADPOLE_ROOT/bin/install"
    fi
}

pf_required() {
    pf_platform
    pf_reset
    pf_check_platform
    [ "$PF_FAILED" = 0 ] || return 0
    pf_check_bazel
    case $PF_OS in
        Darwin) pf_check_macos ;;
        Linux) pf_check_linux ;;
    esac
    pf_check_disk
}

pf_print() {
    {
        printf 'cadpole preflight (%s):\n' "$CADPOLE_ROOT"
        printf '%s' "$PF_REPORT"
        if [ "$PF_FAILED" = 0 ]; then
            printf 'Ready.\n'
        else
            printf 'Fix the ✗ items above and run this again (%s/bin/doctor shows the same list).\n' "$CADPOLE_ROOT"
        fi
    } >&2
}

preflight_report() {
    pf_required
    pf_check_optional
    pf_print
    [ "$PF_FAILED" = 0 ] && pf_write_stamp
    return "$PF_FAILED"
}

pf_write_stamp() {
    pf_stamp=$(pf_stamp_file)
    mkdir -p "$(dirname "$pf_stamp")" 2>/dev/null && pf_stamp_key >"$pf_stamp" 2>/dev/null
    return 0
}

preflight() {
    pf_platform
    pf_stamp=$(pf_stamp_file)
    if [ -r "$pf_stamp" ] && [ "$(cat "$pf_stamp")" = "$(pf_stamp_key)" ]; then
        return 0
    fi
    pf_required
    if [ "$PF_FAILED" = 0 ]; then
        pf_write_stamp
        return 0
    fi
    pf_check_optional
    pf_print
    return 1
}
