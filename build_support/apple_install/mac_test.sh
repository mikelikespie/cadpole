#!/usr/bin/env bash
# One app's installer on a Mac, as far as it goes with no device and no
# development identity (ios_install makes this test as <name>_mac_test; run
# it on the farm's Mac: bazel test --config=apple-remote-macos //<pkg>:<name>_mac_test):
#
#   0. the checks against this Mac's real Xcode and devicectl: Xcode is found
#      and ready, the device list is read, a Simulator is not taken for a
#      device, and it stops at "No ... found"; every option the script
#      passes is in the tools' own help;
#   1. the real device build: one arm64 iphoneos app, each extension inside
#      it (arm64 too) with the bundle ids ios_install names, not signed;
#   2. a dry run on that build with a made-up device and made-up profiles,
#      then the signing step with no identity (ad hoc): the app and each
#      extension come out signed, inside out, each with its own entitlements
#      (the app's capabilities on the app only);
#   3. Xcode builds the throwaway project that mints the profiles (one
#      target per bundle, with the capabilities), unsigned. Nothing reaches Apple.
set -euo pipefail

install="$APPLE_INSTALL_SH"
data="$APPLE_INSTALL_TESTDATA"
out="${TEST_TMPDIR:?}"
export TMPDIR="$out"
unset LANG LC_ALL LC_CTYPE

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
case " ${APPLE_INSTALL_FAMILIES:-iphone ipad} " in
  *" iphone "*) devices=iphone_only.json udid=00008150-000C55556666401C ;;
  *) devices=one_ipad.json udid=00008103-000A11112222001E ;;
esac
args=(--name "$APPLE_INSTALL_NAME" --bundle-id "$APPLE_INSTALL_BUNDLE_ID" --min-os "${APPLE_INSTALL_MIN_OS:-26.0}")
[ -n "${APPLE_INSTALL_ENTITLEMENTS:-}" ] && args+=(--entitlements "$APPLE_INSTALL_ENTITLEMENTS")
for ext in ${APPLE_INSTALL_EXTENSIONS:-}; do args+=(--extension "$ext"); done
for f in ${APPLE_INSTALL_FAMILIES:-iphone ipad}; do args+=(--family "$f"); done

echo "--- the checks, on this Mac as it is: no device here"
# HOME is a scratch folder on the farm, so Xcode's settings and the keychain
# are empty: those lines must be question marks, not failures.
if "$install" --dry-run >"$out/real.doctor.log" 2>&1; then
  cat "$out/real.doctor.log" >&2
  fail "the checks passed on a Mac with no device"
fi
cat "$out/real.doctor.log"
want "$out/real.doctor.log" "ok macOS "
want "$out/real.doctor.log" "ok Xcode 27."
want "$out/real.doctor.log" "ok Xcode's first-launch parts are installed"
want "$out/real.doctor.log" "ok devicectl "
want "$out/real.doctor.log" "No development signing identity in the keychain yet."
want "$out/real.doctor.log" " found."
want "$out/real.doctor.log" "Dry run: nothing was changed."
if grep -q "JSON version" "$out/real.doctor.log"; then
  fail "this Xcode's devicectl writes a newer JSON than the installer was checked with (provision.py, JSON_VERSION_CHECKED)"
fi

echo "--- the options the script passes are in this Xcode's help"
"$install" --check-tools >"$out/tools.log" 2>&1 || {
  cat "$out/tools.log" >&2
  fail "an option the installer passes is gone from this Xcode"
}
cat "$out/tools.log"

echo "--- the built app"
mkdir "$out/ipa"
unzip -q "$APPLE_INSTALL_IPA" -d "$out/ipa"
app="$(echo "$out"/ipa/Payload/*.app)"
check_bundle() { # bundle, what
  local b="$1" binary archs platform
  binary="$b/$(/usr/libexec/PlistBuddy -c 'Print :CFBundleExecutable' "$b/Info.plist")"
  archs="$(lipo -archs "$binary")"
  [ "$archs" = arm64 ] || fail "$2: the slices are '$archs', want arm64 alone"
  platform="$(/usr/libexec/PlistBuddy -c 'Print :DTPlatformName' "$b/Info.plist")"
  [ "$platform" = iphoneos ] || fail "$2: built for '$platform', want iphoneos"
  if codesign -d --entitlements - "$binary" 2>/dev/null | grep -q application-identifier; then
    fail "$2: the build signed it; the installer is meant to"
  fi
  echo "$2: $(/usr/libexec/PlistBuddy -c 'Print :CFBundleIdentifier' "$b/Info.plist"), arm64, iphoneos, not signed"
}
check_bundle "$app" "the app"
[ "$(/usr/libexec/PlistBuddy -c 'Print :CFBundleIdentifier' "$app/Info.plist")" = "$APPLE_INSTALL_BUNDLE_ID" ] || fail "the app's bundle id is not $APPLE_INSTALL_BUNDLE_ID"
[ -f "$app/embedded.mobileprovision" ] || fail "the build embedded no profile (the installer replaces it, but rules_apple puts one in)"
found=""
while IFS= read -r appex; do
  check_bundle "$appex" "extension $(basename "$appex")"
  found="$found $(/usr/libexec/PlistBuddy -c 'Print :CFBundleIdentifier' "$appex/Info.plist")"
done < <(find "$app" -name '*.appex' -type d | sort)
for ext in ${APPLE_INSTALL_EXTENSIONS:-}; do
  case " $found " in *" ${ext%%=*} "*) ;; *) fail "the extension ${ext%%=*} is not in the app (found:$found)" ;; esac
done
for bid in $found; do
  case " ${APPLE_INSTALL_EXTENSIONS:-} " in *" $bid "* | *" $bid="*) ;; *) fail "the app holds the extension $bid, which ios_install does not list" ;; esac
done

echo "--- dry run on the real build, then signed ad hoc by the installer's own commands"
/usr/bin/python3 -I "$APPLE_INSTALL_PROVISION_PY" fake-profiles --out "$out/profiles" --udid "$udid" "${args[@]}"
"$install" --dry-run --sign-adhoc \
  --devices-json "$data/devices/$devices" \
  --profiles-dir "$out/profiles" \
  --identities-file "$data/identities.txt" \
  --team ABCDE12345 >"$out/dry.log" 2>&1 || {
  cat "$out/dry.log" >&2
  fail "dry run failed"
}
cat "$out/dry.log"
want "$out/dry.log" "ok $(basename "$app") ($APPLE_INSTALL_BUNDLE_ID)"
want "$out/dry.log" "The codesign commands work."
want "$out/dry.log" "\"application-identifier\":\"ABCDE12345.$APPLE_INSTALL_BUNDLE_ID\""
for ext in ${APPLE_INSTALL_EXTENSIONS:-}; do
  want "$out/dry.log" "\"application-identifier\":\"ABCDE12345.${ext%%=*}\""
done
# What each bundle really carries now, out of its signature (printed by the
# --sign-adhoc step, one JSON line a bundle).
/usr/bin/python3 -I - "$out/dry.log" "${APPLE_INSTALL_ENTITLEMENTS:-}" "$APPLE_INSTALL_BUNDLE_ID" ${APPLE_INSTALL_EXTENSIONS:-} <<'PY'
import json, plistlib, re, sys

log, app_ents, app_id = sys.argv[1:4]
exts = sys.argv[4:]
text = open(log).read().split("== Test: signing with no identity")[1]
signed = {}
for blob in re.findall(r"^\{.*\}$", text, re.M):
    d = json.loads(blob)
    signed[d["application-identifier"].split(".", 1)[1]] = d
base = {"application-identifier", "com.apple.developer.team-identifier", "get-task-allow", "keychain-access-groups"}
decl = plistlib.load(open(app_ents, "rb")) if app_ents else {}
problems = []
app = signed.get(app_id)
if not app:
    problems.append("the app's signature has no entitlements")
else:
    for k in set(decl) - base:
        if k not in app:
            problems.append("the app's signature lacks %s" % k)
    if "aps-environment" in decl and app.get("aps-environment") != "development":
        problems.append("the app is signed with aps-environment %r" % app.get("aps-environment"))
for e in exts:
    bid, _, path = e.partition("=")
    edecl = plistlib.load(open(path, "rb")) if path else {}
    got = signed.get(bid)
    if not got:
        problems.append("%s: its signature has no entitlements" % bid)
        continue
    extra = set(got) - base - set(edecl)
    if extra:
        problems.append("%s is signed with entitlements it does not declare: %s" % (bid, sorted(extra)))
for bid, d in sorted(signed.items()):
    print("signed %s: %s" % (bid, ", ".join(sorted(d))))
if problems:
    print("\n".join(problems))
    sys.exit(1)
PY

echo "--- Xcode builds the throwaway project (a target per bundle, unsigned)"
"$install" --check-mint-project >"$out/mint.log" 2>&1 || {
  cat "$out/mint.log" >&2
  fail "the throwaway project did not build"
}
cat "$out/mint.log"
echo PASS
