#!/bin/sh
# Tests for bin/: see BUILD.bazel.
set -u

R=${TEST_SRCDIR:?}/_main
T=${TEST_TMPDIR:?}/bin_test
rm -rf "$T"
mkdir -p "$T"
REAL_CADPOLE=$R/src/rust/cadpole-cli/cadpole
export REAL_CADPOLE

failures=0
fail() {
    echo "FAIL: $*" >&2
    failures=$((failures + 1))
}

# A scratch checkout: the scripts, a pinned version and the listener's
# version floor.
REPO=$T/repo
mkdir -p "$REPO/bin/lib" "$REPO/src/tools/listener"
for f in cadpole install doctor dev test lib/preflight.sh lib/build.sh; do
    cp "$R/bin/$f" "$REPO/bin/$f"
done
chmod +x "$REPO/bin/cadpole" "$REPO/bin/install" "$REPO/bin/doctor" "$REPO/bin/dev" "$REPO/bin/test"
cp "$R/src/tools/listener/driver.mjs" "$REPO/src/tools/listener/driver.mjs"
echo 9.9.9 >"$REPO/.bazelversion"
MIN_CLAUDE=$(sed -n 's/^export const MIN_CLAUDE_VERSION = "\([^"]*\)";.*/\1/p' "$REPO/src/tools/listener/driver.mjs")
[ -n "$MIN_CLAUDE" ] || fail "no MIN_CLAUDE_VERSION in driver.mjs"

# Only the system tools the scripts use, so a missing compiler or Bazel is
# really missing.
TOOLS=$T/tools
mkdir -p "$TOOLS"
for t in sh cat sed tr awk ls dirname readlink head mkdir rm ln cp chmod df grep id; do
    p=$(command -v "$t") || { echo "no $t on this machine" >&2; exit 1; }
    ln -s "$p" "$TOOLS/$t"
done

# Stubs, each in its own directory so a case picks which ones exist.
STUBS=$T/stubs
mkdir -p "$STUBS/bazel" "$STUBS/bazelisk" "$STUBS/gxx" "$STUBS/git" "$STUBS/mac" "$STUBS/brew" "$STUBS/claude" "$STUBS/sudo" "$STUBS/curl"
cat >"$STUBS/bazel/bazel" <<'EOF'
#!/bin/sh
echo "$*" >>"$STUB_LOG"
if [ "$1" = --version ]; then
    echo "bazel ${STUB_BAZEL_VERSION:-9.9.9}"
    exit 0
fi
echo "NOISE: stub bazel on stdout"
echo "progress: stub bazel on stderr" >&2
case " $* " in
    *" --check_up_to_date "*) [ -f "$STUB_STATE/built" ]; exit ;;
esac
out=bazel-opt-bin/src/rust/cadpole-cli
mkdir -p "$out/cadpole.runfiles/_main"
echo runfile >"$out/cadpole.runfiles/_main/marker"
ln -sf "$REAL_CADPOLE" "$out/cadpole"
: >"$STUB_STATE/built"
EOF
cp "$STUBS/bazel/bazel" "$STUBS/bazelisk/bazelisk"
mkdir -p "$STUBS/bazelisk-named-bazel"
{ cat "$STUBS/bazel/bazel"; echo "# github.com/bazelbuild/bazelisk"; } >"$STUBS/bazelisk-named-bazel/bazel"
ln -s bazelisk "$STUBS/bazelisk/bazel"
printf '#!/bin/sh\nexit 0\n' >"$STUBS/gxx/g++"
printf '#!/bin/sh\nexit 0\n' >"$STUBS/git/git"
printf '#!/bin/sh\nexit 0\n' >"$STUBS/brew/brew"
printf '#!/bin/sh\nexit 0\n' >"$STUBS/sudo/sudo"
printf '#!/bin/sh\nexit 0\n' >"$STUBS/curl/curl"
printf '#!/bin/sh\necho "${STUB_CLAUDE_VERSION:-%s} (Claude Code)"\n' "$MIN_CLAUDE" >"$STUBS/claude/claude"
cat >"$STUBS/mac/xcode-select" <<'EOF'
#!/bin/sh
echo "${STUB_XCODE_DIR:-/Applications/Xcode.app/Contents/Developer}"
EOF
cat >"$STUBS/mac/xcodebuild" <<'EOF'
#!/bin/sh
case $1 in
    -version) echo "Xcode ${STUB_XCODE_VERSION:-27.2}"; echo "Build version 27C1" ;;
    -license) [ "${STUB_XCODE_LICENSE:-yes}" = yes ] ;;
esac
EOF
chmod +x "$STUBS"/*/*

mkdir -p "$T/etc"
printf 'ID=ubuntu\nID_LIKE=debian\n' >"$T/etc/debian"
printf 'ID=fedora\n' >"$T/etc/fedora"
printf 'ID=arch\n' >"$T/etc/arch"

STUB_STATE=$T/state
STUB_LOG=$T/bazel.log
export STUB_STATE STUB_LOG
mkdir -p "$STUB_STATE"

# run PATHDIRS SCRIPT ARGS...: run a bin/ script with a fresh HOME (no stamp)
# unless KEEP_HOME is set, stubs from PATHDIRS (a space-separated list of
# stub directory names), stdout and stderr into $T/out and $T/err.
run() {
    dirs=$1
    shift
    p=
    for d in $dirs; do p=$p$STUBS/$d:; done
    if [ -z "${KEEP_HOME:-}" ]; then
        rm -rf "$T/home"
    fi
    mkdir -p "$T/home"
    env -i HOME="$T/home" PATH="$p$TOOLS" \
        CADPOLE_UNAME_S="${UNAME_S:-Linux}" CADPOLE_UNAME_M="${UNAME_M:-x86_64}" \
        CADPOLE_OS_RELEASE="${OS_RELEASE:-$T/etc/debian}" CADPOLE_FIRST_BUILD_GIB=0 \
        STUB_STATE="$STUB_STATE" STUB_LOG="$STUB_LOG" REAL_CADPOLE="$REAL_CADPOLE" \
        STUB_BAZEL_VERSION="${STUB_BAZEL_VERSION:-9.9.9}" STUB_XCODE_VERSION="${STUB_XCODE_VERSION:-27.2}" \
        STUB_XCODE_LICENSE="${STUB_XCODE_LICENSE:-yes}" STUB_XCODE_DIR="${STUB_XCODE_DIR:-/Applications/Xcode.app/Contents/Developer}" \
        STUB_CLAUDE_VERSION="${STUB_CLAUDE_VERSION:-$MIN_CLAUDE}" \
        ${EXTRA_ENV:-} "$@" >"$T/out" 2>"$T/err" </dev/null
    code=$?
    return 0
}
expect_code() { [ "$code" = "$1" ] || fail "$2: exit $code, wanted $1; stderr: $(cat "$T/err")"; }
expect_err() { grep -qF -- "$1" "$T/err" || fail "$2: stderr lacks '$1': $(cat "$T/err")"; }
expect_no_err() { ! grep -qF -- "$1" "$T/err" || fail "$2: stderr has '$1': $(cat "$T/err")"; }
expect_quiet_stdout() { [ ! -s "$T/out" ] || fail "$1: stdout not empty: $(cat "$T/out")"; }

# ---- Preflight: each missing piece, with the command that fixes it.

run "gxx git curl" "$REPO/bin/cadpole" --version
expect_code 1 "no bazel"
expect_err "✗ bazel not found" "no bazel"
expect_err "bazelisk-linux-amd64" "no bazel"
expect_err "✓ C++ headers (g++)" "no bazel"
expect_quiet_stdout "no bazel"

UNAME_M=aarch64 run "gxx git" "$REPO/bin/doctor"
expect_code 1 "no bazel, arm64"
expect_err "✓ Linux arm64" "arm64"
expect_err "apt install curl" "no curl: the Bazelisk steps start with it"
expect_err "bazelisk-linux-arm64" "arm64"
expect_no_err "amd64" "arm64"

STUB_BAZEL_VERSION=7.4.1 run "bazel gxx git" "$REPO/bin/cadpole" --version
expect_code 1 "wrong bazel"
expect_err "is Bazel 7.4.1, but this checkout pins 9.9.9" "wrong bazel"

run "bazelisk-named-bazel gxx git" "$REPO/bin/doctor"
expect_err "✓ bazel 9.9.9 (Bazelisk" "bazelisk named bazel"
run "bazelisk gxx git claude" "$REPO/bin/doctor"
expect_code 0 "bazelisk"
expect_err "✓ bazel 9.9.9 (Bazelisk" "bazelisk"
expect_err "✓ claude $MIN_CLAUDE" "claude at the floor"
expect_err "✗ optional: ibazel" "ibazel is optional"
expect_err "✗ optional: cadpole is not on PATH" "cadpole on PATH"
expect_err "Ready." "bazelisk"

STUB_CLAUDE_VERSION=0.0.1 run "bazelisk gxx git claude" "$REPO/bin/doctor"
expect_code 0 "old claude is optional"
expect_err "the listener needs $MIN_CLAUDE or later" "old claude"

run "bazel git sudo" "$REPO/bin/cadpole" --version
expect_code 1 "no g++, debian"
expect_err "✗ no C++ compiler" "no g++"
expect_err "apt install g++" "no g++, debian"
if [ "$(id -u)" != 0 ]; then
    expect_err "sudo apt install g++" "no g++, debian, with sudo"
    run "bazel git" "$REPO/bin/cadpole" --version
    expect_err "apt install g++   # as root: there is no sudo here" "no g++, no sudo"
fi
OS_RELEASE=$T/etc/fedora run "bazel git" "$REPO/bin/cadpole" --version
expect_err "dnf install gcc-c++" "no g++, fedora"
OS_RELEASE=$T/etc/arch run "bazel gxx" "$REPO/bin/cadpole" --version
expect_err "pacman -S git" "no git, arch"

UNAME_S=Darwin UNAME_M=arm64 run "bazel mac brew" "$REPO/bin/doctor"
expect_code 0 "macOS"
expect_err "✓ macOS on Apple silicon" "macOS"
expect_err "✓ Xcode 27.2" "macOS"
expect_err "brew install ibazel" "macOS"
expect_err "✓ Homebrew" "macOS"
UNAME_S=Darwin UNAME_M=arm64 run "mac brew" "$REPO/bin/doctor"
expect_err "brew install bazelisk" "macOS, no bazel"
UNAME_S=Darwin UNAME_M=arm64 run "mac" "$REPO/bin/doctor"
expect_err "https://brew.sh" "macOS, no Homebrew"
expect_err "✗ optional: Homebrew not found" "macOS, no Homebrew"
UNAME_S=Darwin UNAME_M=arm64 STUB_XCODE_LICENSE=no run "bazel mac" "$REPO/bin/doctor"
expect_code 1 "licence"
expect_err "sudo xcodebuild -license accept" "licence"
UNAME_S=Darwin UNAME_M=arm64 STUB_XCODE_VERSION=26.4 run "bazel mac" "$REPO/bin/doctor"
expect_code 1 "old Xcode"
expect_err "Xcode 26.4; Cadpole needs Xcode 27" "old Xcode"
UNAME_S=Darwin UNAME_M=arm64 STUB_XCODE_DIR=/Library/Developer/CommandLineTools run "bazel mac" "$REPO/bin/doctor"
expect_err "Command Line Tools" "CLT only"
UNAME_S=Darwin UNAME_M=x86_64 run "bazel mac" "$REPO/bin/cadpole" --version
expect_code 1 "Intel Mac"
expect_err "not supported" "Intel Mac"
UNAME_S=FreeBSD UNAME_M=amd64 run "bazel" "$REPO/bin/install" --prefix "$T/nope"
expect_code 1 "FreeBSD"
expect_err "FreeBSD x86_64 is not supported" "FreeBSD"
[ ! -e "$T/nope" ] || fail "install went ahead after a failed preflight"

# ---- The wrapper: a build's output never reaches stdout.

rm -f "$STUB_STATE/built" "$STUB_LOG"
rm -rf "$REPO/bazel-opt-bin"
printf '%s\n' '{"jsonrpc":"2.0","id":1,"method":"initialize","params":{"protocolVersion":"2025-06-18","capabilities":{},"clientInfo":{"name":"t","version":"0"}}}' >"$T/init.json"
p=$STUBS/bazel:$STUBS/gxx:$STUBS/git:$TOOLS
mkdir -p "$T/home"
env -i HOME="$T/home" PATH="$p" CADPOLE_UNAME_S=Linux CADPOLE_UNAME_M=x86_64 \
    CADPOLE_OS_RELEASE="$T/etc/debian" CADPOLE_FIRST_BUILD_GIB=0 \
    STUB_STATE="$STUB_STATE" STUB_LOG="$STUB_LOG" REAL_CADPOLE="$REAL_CADPOLE" \
    "$REPO/bin/cadpole" mcp <"$T/init.json" >"$T/out" 2>"$T/err"
code=$?
expect_code 0 "mcp with a build"
expect_err "NOISE: stub bazel on stdout" "the build ran and its stdout went to stderr"
expect_err "building //src/rust/cadpole-cli:cadpole" "mcp build notice"
grep -q '"jsonrpc":"2.0"' "$T/out" || fail "mcp: no JSON-RPC reply on stdout: $(cat "$T/out")"
while IFS= read -r line; do
    case $line in
        '{'*'}') ;;
        *) fail "mcp: stdout line is not a JSON-RPC message: $line" ;;
    esac
done <"$T/out"

# Up to date: one quiet check, nothing printed.
KEEP_HOME=1 run "bazel gxx git" "$REPO/bin/cadpole" --version
expect_code 0 "up to date"
[ ! -s "$T/err" ] || fail "up to date: stderr not empty: $(cat "$T/err")"
grep -q "^cadpole " "$T/out" || fail "up to date: no version on stdout"

# The stamp: a passed preflight is not repeated until what it is keyed on
# changes.
: >"$STUB_LOG"
KEEP_HOME=1 run "bazel gxx git" "$REPO/bin/cadpole" --version
grep -q -- "--version" "$STUB_LOG" && fail "stamp: preflight ran again"
echo 9.9.8 >"$REPO/.bazelversion"
KEEP_HOME=1 run "bazel gxx git" "$REPO/bin/cadpole" --version
expect_code 1 "stamp: a new pin is checked"
expect_err "pins 9.9.8" "stamp: a new pin is checked"
echo 9.9.9 >"$REPO/.bazelversion"

: >"$STUB_LOG"
EXTRA_ENV=CADPOLE_NO_BUILD=1 KEEP_HOME=1 run "bazel gxx git" "$REPO/bin/cadpole" --version
expect_code 0 "CADPOLE_NO_BUILD"
grep -q build "$STUB_LOG" && fail "CADPOLE_NO_BUILD still called bazel build"

# ---- From another directory, relative paths are the caller's.

mkdir -p "$T/work/models"
printf 'a = box(10, 10, 10)\n' >"$T/work/models/part.star"
code=$(cd "$T/work" && KEEP_HOME=1 run "bazel gxx git" "$REPO/bin/cadpole" export models/part.star -o part.stl && echo "$code")
[ "$code" = 0 ] || fail "export from another directory: $(cat "$T/err")"
[ -s "$T/work/part.stl" ] || fail "export from another directory wrote nothing in that directory"

# ---- install, idempotent, and uninstall.

PFX=$T/prefix
run "bazel gxx git" "$REPO/bin/install" --prefix "$PFX"
expect_code 0 "install"
[ -L "$PFX/cadpole" ] || fail "install made no symlink"
[ "$(readlink "$PFX/cadpole")" = "$REPO/bin/cadpole" ] || fail "install symlink points at $(readlink "$PFX/cadpole")"
expect_err "is not on your PATH" "install PATH hint"
expect_err "export PATH=\"$PFX:\$PATH\"" "install PATH hint"
KEEP_HOME=1 run "bazel gxx git" "$REPO/bin/install" --prefix "$PFX"
expect_code 0 "install again"
(cd "$T/work" && KEEP_HOME=1 run "bazel gxx git" "$PFX/cadpole" --version)
grep -q "^cadpole " "$T/out" || fail "the installed symlink does not run: $(cat "$T/err")"
KEEP_HOME=1 run "bazel gxx git" sh -c "PATH=$PFX:\$PATH; \"$REPO/bin/install\" --prefix \"$PFX\""
expect_no_err "is not on your PATH" "no hint when the prefix is on PATH"
KEEP_HOME=1 run "bazel gxx git" "$REPO/bin/install" --prefix "$PFX" --uninstall
expect_code 0 "uninstall"
[ ! -e "$PFX/cadpole" ] && [ ! -L "$PFX/cadpole" ] || fail "uninstall left $PFX/cadpole"
KEEP_HOME=1 run "bazel gxx git" "$REPO/bin/install" --prefix "$PFX" --uninstall
expect_code 0 "uninstall twice"

echo "not ours" >"$PFX/cadpole"
KEEP_HOME=1 run "bazel gxx git" "$REPO/bin/install" --prefix "$PFX"
expect_code 1 "install over a foreign file"
[ "$(cat "$PFX/cadpole")" = "not ours" ] || fail "install replaced a foreign file"
KEEP_HOME=1 run "bazel gxx git" "$REPO/bin/install" --prefix "$PFX" --uninstall
expect_code 1 "uninstall of a foreign file"
rm "$PFX/cadpole"

KEEP_HOME=1 run "bazel gxx git" "$REPO/bin/install" --prefix "$PFX" --copy
expect_code 0 "install --copy"
[ -f "$PFX/cadpole" ] && [ ! -L "$PFX/cadpole" ] || fail "--copy made no launcher"
[ -f "$T/home/.local/share/cadpole/cadpole.runfiles/_main/marker" ] || fail "--copy left out the runfiles"
(cd "$T/work" && KEEP_HOME=1 run "" "$PFX/cadpole" --version)
grep -q "^cadpole " "$T/out" || fail "the copy does not run: $(cat "$T/err")"
KEEP_HOME=1 run "bazel gxx git" "$REPO/bin/install" --prefix "$PFX" --uninstall
[ ! -e "$PFX/cadpole" ] && [ ! -e "$T/home/.local/share/cadpole" ] || fail "uninstall left the copy"

# ---- Only bin/test runs tests, with the sweep's targets or the ones given.

grep -q "^test" "$STUB_LOG" && fail "a run or install script called bazel test: $(grep "^test" "$STUB_LOG")"
: >"$STUB_LOG"
KEEP_HOME=1 run "bazel gxx git" "$REPO/bin/test"
expect_code 0 "bin/test"
grep -qx "test --build_tests_only -- //... -//src/apple/CadpoleiOS:CadpoleiOS -//src/apple/CadpoleMac:CadpoleMac" "$STUB_LOG" ||
    fail "bin/test ran: $(cat "$STUB_LOG")"
: >"$STUB_LOG"
KEEP_HOME=1 run "bazel gxx git" "$REPO/bin/test" //src/web/... --test_output=errors
grep -qx "test --build_tests_only //src/web/... --test_output=errors" "$STUB_LOG" || fail "bin/test with targets ran: $(cat "$STUB_LOG")"
run "gxx git" "$REPO/bin/test"
expect_code 1 "bin/test runs the preflight"

if [ "$failures" -gt 0 ]; then
    echo "$failures failure(s)" >&2
    exit 1
fi
echo "all bin/ checks passed"
