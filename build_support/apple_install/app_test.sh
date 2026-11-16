#!/usr/bin/env bash
# One app's ios_install settings, as dry runs on made-up inputs (ios_install
# makes this test as <name>_test; it runs on Linux and on a Mac). Checks that
# the app's entitlements files read, that the app and each extension are
# named and get the entitlements they should (the app what its file declares,
# push as "development"; an extension only its own), and that a device of a
# kind the app is not built for is refused.
set -euo pipefail

install="$APPLE_INSTALL_SH"
data="$APPLE_INSTALL_TESTDATA"
out="${TEST_TMPDIR:?}"
export TMPDIR="$out"
unset LANG LC_ALL LC_CTYPE APPLE_INSTALL_IPA
NOW=2026-11-14T00:00:00Z
py=python3
[ -x /usr/bin/python3 ] && py=/usr/bin/python3

fail() {
  echo "FAIL: $*" >&2
  exit 1
}
want() { # file, text
  grep -q -F -- "$2" "$1" || {
    cat "$1" >&2
    fail "expected: $2"
  }
}

cat >"$out/host.txt" <<'HOST'
os=Darwin
macos=27.2
xcode_path=/Applications/Xcode.app/Contents/Developer
xcode_version=27.2
xcode_build=27B52
first_launch_rc=0
devicectl=/Applications/Xcode.app/Contents/Developer/usr/bin/devicectl
devicectl_version=651.32
via_bazel=1
HOST

# A device it goes on, and one it does not.
case " ${APPLE_INSTALL_FAMILIES:-iphone ipad} " in
  *" iphone "*) devices=iphone_only.json udid=00008150-000C55556666401C wrong="" ;;
  *) devices=one_ipad.json udid=00008103-000A11112222001E wrong=iphone_only.json ;;
esac

# The same description of the app as install.sh builds, for provision.py.
args=(--name "$APPLE_INSTALL_NAME" --bundle-id "$APPLE_INSTALL_BUNDLE_ID" --min-os "${APPLE_INSTALL_MIN_OS:-26.0}")
[ -n "${APPLE_INSTALL_ENTITLEMENTS:-}" ] && args+=(--entitlements "$APPLE_INSTALL_ENTITLEMENTS")
for ext in ${APPLE_INSTALL_EXTENSIONS:-}; do args+=(--extension "$ext"); done
for f in ${APPLE_INSTALL_FAMILIES:-iphone ipad}; do args+=(--family "$f"); done

doctor() { # log, wanted exit code, then options
  local log="$out/$1.log" code="$2" rc=0
  shift 2
  "$install" --dry-run --host-file "$out/host.txt" --now "$NOW" \
    --identities-file "$data/identities.txt" --xcode-defaults "$data/xcode_defaults.plist" "$@" >"$log" 2>&1 || rc=$?
  [ "$rc" = "$code" ] || {
    cat "$log" >&2
    fail "$1: exit $rc, want $code"
  }
}

echo "--- $APPLE_INSTALL_NAME, no profiles yet: Xcode would be asked for one per bundle"
mkdir "$out/none"
doctor none 0 --devices-json "$data/devices/$devices" --profiles-dir "$out/none"
cat "$out/none.log"
bundles="$APPLE_INSTALL_BUNDLE_ID"
for ext in ${APPLE_INSTALL_EXTENSIONS:-}; do bundles="$bundles and ${ext%%=*}"; done
want "$out/none.log" "creates a development profile for $bundles."
want "$out/none.log" "Sign the app $APPLE_INSTALL_BUNDLE_ID"
for ext in ${APPLE_INSTALL_EXTENSIONS:-}; do want "$out/none.log" "Sign the extension ${ext%%=*}"; done

echo "--- with a profile for every bundle: each signed with what it declares, and nothing else"
"$py" -I "$APPLE_INSTALL_PROVISION_PY" fake-profiles --out "$out/profiles" --udid "$udid" "${args[@]}"
doctor have 0 --devices-json "$data/devices/$devices" --profiles-dir "$out/profiles"
cat "$out/have.log"
want "$out/have.log" "They list every device and match a certificate in the keychain"
"$py" -I - "$out/have.log" "${APPLE_INSTALL_ENTITLEMENTS:-}" "$APPLE_INSTALL_BUNDLE_ID" ${APPLE_INSTALL_EXTENSIONS:-} <<'PY'
import plistlib, re, sys

log, app_ents, app_id = sys.argv[1:4]
exts = sys.argv[4:]
text = open(log).read()
plan = text.split("== What a real run would do")[1]
# "N. Sign the app X ..." / "N. Sign the extension Y ..." and the key = value lines under each
sections = {}
for m in re.finditer(r"\d+\. Sign the (app|extension) (\S+) with .*?\n((?: {9,}[a-z].*\n)*)", plan):
    sections[m.group(2)] = dict(
        (k.strip(), v.strip()) for k, v in (l.split(" = ", 1) for l in m.group(3).strip().splitlines())
    )
base = {"application-identifier", "com.apple.developer.team-identifier", "get-task-allow", "keychain-access-groups"}


def declared(path):
    return plistlib.load(open(path, "rb")) if path else {}


problems = []
wants = [(app_id, declared(app_ents))] + [(e.split("=", 1)[0], declared(e.split("=", 1)[1]) if "=" in e else {}) for e in exts]
for bid, decl in wants:
    got = sections.get(bid)
    if got is None:
        problems.append("no signing step for %s" % bid)
        continue
    if got.get("application-identifier") != "ABCDE12345." + bid:
        problems.append("%s: application-identifier %r" % (bid, got.get("application-identifier")))
    caps = set(decl) - base
    extra = set(got) - base - caps
    if extra:
        problems.append("%s would get entitlements it does not declare: %s" % (bid, sorted(extra)))
    for k in caps:
        if k not in got:
            problems.append("%s: %s is missing" % (bid, k))
    if "aps-environment" in caps and got.get("aps-environment") != "development":
        problems.append("%s: aps-environment is %r, want development" % (bid, got.get("aps-environment")))
    print("%s: %s" % (bid, ", ".join(sorted(set(got) - base)) or "no capabilities"))
if problems:
    print("\n".join(problems))
    sys.exit(1)
PY

if [ -n "$wrong" ]; then
  echo "--- an iPhone is refused: $APPLE_INSTALL_NAME is not built for one"
  doctor wrong 1 --devices-json "$data/devices/$wrong" --profiles-dir "$out/profiles"
  want "$out/wrong.log" "goes on an iPad only."
fi
echo PASS
