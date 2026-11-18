#!/usr/bin/env bash
# Puts an iOS app from this repo on the owner's own iPhones and iPads, from a
# Mac:
#
#   bazel run //path/to/app:install
#   bazel run //path/to/app:install -- --device NAME_OR_UDID --team TEAMID
#   bazel run //path/to/app:install -- --all          # every connected device
#   bazel run //path/to/app:install -- --dry-run      # check everything, change nothing
#   bazel run //path/to/app:doctor                    # the same checks, without building the app
#
# ios_install (//build_support/apple_install:defs.bzl) makes those targets and says,
# through APPLE_INSTALL_* in the environment, which app this is: its name,
# bundle id, entitlements, extensions, device families, minimum OS and team.
#
# Bazel has already built the app for a device, unsigned (device_app.bzl).
# This script checks the Mac and the devices, finds (or has Xcode make) a
# development provisioning profile for the app and for each extension that
# lists every device, signs the extensions and then the app with this Mac's
# development identity, installs it on each device and starts it. The
# choosing, the checks and the reading of error messages are in provision.py.
#
# It also runs with no Bazel at all, on an app built elsewhere, given the
# same APPLE_INSTALL_* values (the doctor target's command line shows them).
#
# Written for the bash 3.2 that macOS has.
#
# For tests only (with --dry-run): --devices-json FILE, --profiles-dir DIR,
# --identities-file FILE, --xcode-defaults FILE, --lock-json FILE, --host-file
# FILE and --now TIME stand in for devicectl, Xcode's profile folders, the
# keychain, Xcode's settings, the devices' lock state, the facts about this
# Mac and the clock. --answer TEXT stands in for the person's answer when
# several devices are in reach. --check-mint-project builds the throwaway
# Xcode project without signing; --check-tools checks that this Xcode's tools
# still take the options this script passes. --sign-adhoc (with the test
# inputs) goes on through the signing step with no identity ("-"), to check
# the codesign commands on a Mac that has no development identity.
set -euo pipefail

say() { printf '%s\n' "$*"; }
step() { printf '\n== %s\n' "$*"; }
FAILED=0
die() {
  FAILED=1
  printf '\nStopped: %s\n' "$*" >&2
  exit 1
}

NAME="${APPLE_INSTALL_NAME:-The app}"
INSTALL_CMD="${APPLE_INSTALL_CMD:-bazel run :install}"

usage() {
  cat <<EOF
Puts $NAME on your iPhone or iPad.

  $INSTALL_CMD
  $INSTALL_CMD -- [options]

  --device NAME_OR_UDID   which device; repeat it for more than one
  --all                   every connected device it can go on
  --team TEAMID           which developer team signs (ten letters and digits)
  --dry-run               check the Mac, the devices and the signing, say what a
                          real run would do, and change nothing (also: --doctor)
  --keep                  keep the working folder with every tool's full output
                          (it is always kept when something fails)
  --ipa FILE              install this already-built app, not Bazel's
  --help
EOF
}

WANTS=()
ALL=0
TEAM=""
DRY_RUN=0
KEEP=0
CHECK_MINT=0
CHECK_TOOLS=0
SIGN_ADHOC=0
DEVICES_JSON=""
PROFILES_DIR=""
IDENTITIES_FILE=""
XCODE_DEFAULTS=""
LOCK_JSON=""
HOST_FILE=""
NOW=""
IPA_ARG=""
ANSWER=""
HAVE_ANSWER=0
while [ $# -gt 0 ]; do
  case "$1" in
    --device) WANTS+=("${2:?--device needs a name or UDID}"); shift 2 ;;
    --device=*) WANTS+=("${1#*=}"); shift ;;
    --all) ALL=1; shift ;;
    --team) TEAM="${2:?--team needs a team id}"; shift 2 ;;
    --team=*) TEAM="${1#*=}"; shift ;;
    --dry-run | --doctor) DRY_RUN=1; shift ;;
    --keep) KEEP=1; shift ;;
    --ipa) IPA_ARG="${2:?--ipa needs a file}"; shift 2 ;;
    --ipa=*) IPA_ARG="${1#*=}"; shift ;;
    --check-mint-project) CHECK_MINT=1; shift ;;
    --check-tools) CHECK_TOOLS=1; shift ;;
    --sign-adhoc) SIGN_ADHOC=1; shift ;;
    --devices-json) DEVICES_JSON="${2:?}"; shift 2 ;;
    --profiles-dir) PROFILES_DIR="${2:?}"; shift 2 ;;
    --identities-file) IDENTITIES_FILE="${2:?}"; shift 2 ;;
    --xcode-defaults) XCODE_DEFAULTS="${2:?}"; shift 2 ;;
    --lock-json) LOCK_JSON="${2:?}"; shift 2 ;;
    --host-file) HOST_FILE="${2:?}"; shift 2 ;;
    --now) NOW="${2:?}"; shift 2 ;;
    --answer) ANSWER="${2-}"; HAVE_ANSWER=1; shift 2 ;;
    -h | --help) usage; exit 0 ;;
    *) usage >&2; die "unknown option: $1" ;;
  esac
done

if [ -n "$TEAM" ] && ! [[ "$TEAM" =~ ^[A-Z0-9]{10}$ ]]; then
  die "--team wants a team id: ten capital letters and digits, like ABCDE12345."
fi
if [ "$ALL" = 1 ] && [ ${#WANTS[@]} -gt 0 ]; then
  die "--all and --device do not go together: --all is every connected device."
fi

# ---- where things are --------------------------------------------------------

# `bazel run` starts this in its runfiles; the paths Bazel gives are relative
# to there. Fall back to the files next to this script.
here="$(cd "$(dirname "${BASH_SOURCE[0]}")" && pwd)"
resolve() {
  local p="$1"
  if [ -n "$p" ] && [ -e "$p" ]; then
    (cd "$(dirname "$p")" && printf '%s/%s\n' "$(pwd)" "$(basename "$p")")
  elif [ -n "$p" ] && [ -n "${RUNFILES_DIR:-}" ] && [ -e "$RUNFILES_DIR/_main/$p" ]; then
    printf '%s\n' "$RUNFILES_DIR/_main/$p"
  elif [ -n "${2:-}" ] && [ -e "$2" ]; then
    printf '%s\n' "$2"
  else
    return 1
  fi
}

PROVISION_PY="$(resolve "${APPLE_INSTALL_PROVISION_PY:-}" "$here/provision.py")" || die "provision.py is missing. Run this with: $INSTALL_CMD"

# The python3 that comes with Xcode on a Mac; any python3 elsewhere (tests).
if [ -x /usr/bin/python3 ]; then
  PYTHON=/usr/bin/python3
else
  PYTHON="$(command -v python3)" || die "python3 is missing. On a Mac it comes with Xcode: run xcode-select --install."
fi
py() { "$PYTHON" -I "$PROVISION_PY" "$@"; }


# The app, as ios_install describes it.
BUNDLE_ID="${APPLE_INSTALL_BUNDLE_ID:?APPLE_INSTALL_BUNDLE_ID is not set: run this through its ios_install target}"
DEFAULT_TEAM="${APPLE_INSTALL_DEFAULT_TEAM-}"
MIN_OS="${APPLE_INSTALL_MIN_OS:-26.0}"
APP_ARGS=(--name "$NAME" --bundle-id "$BUNDLE_ID" --min-os "$MIN_OS" --install-cmd "$INSTALL_CMD")
if [ -n "${APPLE_INSTALL_ENTITLEMENTS:-}" ]; then
  ents="$(resolve "$APPLE_INSTALL_ENTITLEMENTS")" || die "the app's entitlements file is missing ($APPLE_INSTALL_ENTITLEMENTS)."
  APP_ARGS+=(--entitlements "$ents")
fi
for ext in ${APPLE_INSTALL_EXTENSIONS:-}; do
  case "$ext" in
    *=*)
      ents="$(resolve "${ext#*=}")" || die "the entitlements file of ${ext%%=*} is missing (${ext#*=})."
      APP_ARGS+=(--extension "${ext%%=*}=$ents")
      ;;
    *) APP_ARGS+=(--extension "$ext") ;;
  esac
done
[ -n "${APPLE_INSTALL_NOTE:-}" ] && APP_ARGS+=(--note "$APPLE_INSTALL_NOTE")
for family in ${APPLE_INSTALL_FAMILIES:-iphone ipad}; do
  APP_ARGS+=(--family "$family")
done
KINDS="$(printf '%s' "${APPLE_INSTALL_FAMILIES:-iphone ipad}" | sed 's/iphone/iPhone/; s/ipad/iPad/; s/ / or /')"

FAKE_INPUTS=0
if [ -n "$DEVICES_JSON$PROFILES_DIR$IDENTITIES_FILE$XCODE_DEFAULTS$LOCK_JSON$HOST_FILE$NOW" ]; then
  FAKE_INPUTS=1
  [ "$DRY_RUN" = 1 ] || die "--devices-json, --profiles-dir, --identities-file and the other test inputs are for tests, and only with --dry-run."
fi
if [ "$SIGN_ADHOC" = 1 ] && [ "$FAKE_INPUTS" = 0 ]; then
  die "--sign-adhoc is for tests, with the test inputs."
fi

IS_MAC=0
[ "$(uname -s)" = Darwin ] && IS_MAC=1
if [ "$IS_MAC" = 0 ] && ! { [ -n "$DEVICES_JSON" ] && [ -n "$IDENTITIES_FILE" ] && [ -n "$PROFILES_DIR" ]; }; then
  die "this runs on a Mac only. Installing on an $KINDS needs Xcode, and this is $(uname -s).
On a Mac, in a checkout of this repository:  $INSTALL_CMD"
fi

WORK="$(mktemp -d "${TMPDIR:-/tmp}/ios-install.XXXXXX")"
cleanup() {
  if [ "$KEEP" = 1 ] || [ "$FAILED" = 1 ]; then
    say ""
    say "Every tool's full output is kept in: $WORK"
  else
    rm -rf "$WORK"
  fi
}
trap cleanup EXIT

# ---- the throwaway Xcode project (also what --check-mint-project tests) ---------

# Builds an empty app target per bundle (the app and each extension) with
# their bundle ids, capabilities and the team. With signing on, for a device,
# Xcode registers the device with the team, turns on the capabilities and
# creates a development profile for each. Output goes to $WORK/xcodebuild.log.
mint_build() { # team, then extra xcodebuild arguments
  local team="$1"
  shift
  rm -rf "$WORK/mint"
  py mint-project --out "$WORK/mint" --team "$team" "${APP_ARGS[@]}" >/dev/null
  xcodebuild \
    -project "$WORK/mint/ProfileMint.xcodeproj" \
    -scheme ProfileMint \
    -configuration Release \
    -derivedDataPath "$WORK/mint/derived" \
    "$@" \
    build >"$WORK/xcodebuild.log" 2>&1
}

if [ "$CHECK_MINT" = 1 ]; then
  step "Building the throwaway Xcode project, unsigned"
  if mint_build "${TEAM:-${DEFAULT_TEAM:-AAAAAAAAAA}}" -destination "generic/platform=iOS" CODE_SIGNING_ALLOWED=NO; then
    say "Xcode built it."
    exit 0
  fi
  tail -n 40 "$WORK/xcodebuild.log" >&2
  die "Xcode could not build the throwaway project."
fi

# Every option this script passes to Xcode's tools, looked up in the tools' own
# help. If an Xcode update renames one, this says which, before a real run
# finds out half way.
if [ "$CHECK_TOOLS" = 1 ]; then
  step "Checking that Xcode's tools take the options this script passes"
  missing=0
  has() { # what, help text file, option
    if grep -q -e "$3" "$2"; then
      say "  ok  $1 $3"
    else
      say "  NO  $1 no longer lists $3"
      missing=1
    fi
  }
  xcrun devicectl list devices --help >"$WORK/h-list.txt" 2>&1 || true
  has "devicectl list devices" "$WORK/h-list.txt" "--json-output"
  xcrun devicectl device install app --help >"$WORK/h-install.txt" 2>&1 || true
  has "devicectl device install app" "$WORK/h-install.txt" "--device"
  has "devicectl device install app" "$WORK/h-install.txt" "--json-output"
  xcrun devicectl device process launch --help >"$WORK/h-launch.txt" 2>&1 || true
  has "devicectl device process launch" "$WORK/h-launch.txt" "--device"
  has "devicectl device process launch" "$WORK/h-launch.txt" "--terminate-existing"
  has "devicectl device process launch" "$WORK/h-launch.txt" "bundle-identifier-or-path"
  xcrun devicectl device info lockState --help >"$WORK/h-lock.txt" 2>&1 || true
  has "devicectl device info lockState" "$WORK/h-lock.txt" "--timeout"
  xcrun devicectl device info details --help >"$WORK/h-details.txt" 2>&1 || true
  has "devicectl device info details" "$WORK/h-details.txt" "--json-output"
  xcrun devicectl manage pair --help >"$WORK/h-pair.txt" 2>&1 || true
  has "devicectl manage pair" "$WORK/h-pair.txt" "--device"
  xcodebuild -help >"$WORK/h-xcodebuild.txt" 2>&1 || true
  for opt in -allowProvisioningUpdates -allowProvisioningDeviceRegistration -destination-timeout -checkFirstLaunchStatus -runFirstLaunch -derivedDataPath -downloadPlatform; do
    has "xcodebuild" "$WORK/h-xcodebuild.txt" "$opt"
  done
  [ "$missing" = 0 ] || die "this Xcode's tools have changed. Fix the options in build_support/apple_install/install.sh."
  exit 0
fi

# ---- 1. facts about this Mac -----------------------------------------------------

# key=value lines for provision.py's preflight. Nothing here changes anything.
gather_host() {
  local out="$WORK/host.txt" v rc
  if [ -n "$HOST_FILE" ]; then
    cp "$HOST_FILE" "$out"
    return
  fi
  {
    say "os=$(uname -s)"
    if [ "$IS_MAC" = 1 ]; then
      say "macos=$(sw_vers -productVersion 2>/dev/null || true)"
      say "xcode_path=$(xcode-select -p 2>/dev/null || true)"
      if v="$(xcodebuild -version 2>"$WORK/xcodebuild-version.err")"; then
        say "xcode_version=$(printf '%s\n' "$v" | sed -n 's/^Xcode //p' | head -n 1)"
        say "xcode_build=$(printf '%s\n' "$v" | sed -n 's/^Build version //p' | head -n 1)"
        rc=0
        xcodebuild -checkFirstLaunchStatus >"$WORK/first-launch.log" 2>&1 || rc=$?
        say "first_launch_rc=$rc"
      else
        say "xcodebuild_error=$(head -n 1 "$WORK/xcodebuild-version.err" | tr -d '\r')"
      fi
      say "devicectl=$(xcrun --find devicectl 2>/dev/null || true)"
      # Only a version number: before Xcode has installed its parts, devicectl
      # first prints its own progress ("Install Started") here.
      say "devicectl_version=$(xcrun devicectl --version 2>/dev/null | grep -E '^[0-9]+(\.[0-9]+)*' | head -n 1 || true)"
      if [ -z "$IDENTITIES_FILE" ]; then
        say "dev_certs=$(security find-certificate -a -c 'Apple Development' -Z 2>/dev/null | grep -c '^SHA-1' || true)"
      fi
    fi
    if [ -n "${BUILD_WORKSPACE_DIRECTORY:-}" ]; then
      say "via_bazel=1"
      say "bazel_wanted=$(head -n 1 "$BUILD_WORKSPACE_DIRECTORY/.bazelversion" 2>/dev/null || true)"
      if [ -n "$(git -C "$BUILD_WORKSPACE_DIRECTORY" status --porcelain -- MODULE.bazel.lock 2>/dev/null || true)" ]; then
        say "lockfile_dirty=1"
      fi
    else
      say "bazel=$(command -v bazel 2>/dev/null || true)"
    fi
  } >"$out"
}

step "Checking this Mac, the devices and the signing"
gather_host

# ---- 2. the devices --------------------------------------------------------------

list_devices() {
  rm -f "$WORK/devices.json"
  xcrun devicectl list devices --json-output "$WORK/devices.json" >"$WORK/devicectl-list.log" 2>&1
}

WANT_ARGS=()
set_want_args() {
  WANT_ARGS=()
  local w
  for w in ${WANTS[@]+"${WANTS[@]}"}; do
    WANT_ARGS+=(--want "$w")
  done
  [ "$ALL" = 1 ] && WANT_ARGS+=(--all)
  return 0
}
choose() { # writes $WORK/chosen.txt; returns provision.py's exit code
  local rc=0
  set_want_args
  py choose --json "$WORK/devices.json" ${WANT_ARGS[@]+"${WANT_ARGS[@]}"} "${APP_ARGS[@]}" >"$WORK/chosen.txt" 2>"$WORK/chosen-notes.txt" || rc=$?
  return $rc
}

CHOOSE_RC=2
if [ -n "$DEVICES_JSON" ]; then
  cp "$DEVICES_JSON" "$WORK/devices.json"
  CHOOSE_RC=0
  choose || CHOOSE_RC=$?
elif xcrun --find devicectl >/dev/null 2>&1; then
  if ! list_devices; then
    rm -f "$WORK/devices.json"
    say "devicectl_error=$(grep -v '^ *$' "$WORK/devicectl-list.log" | tail -n 1 | tr -d '\r')" >>"$WORK/host.txt"
  else
    CHOOSE_RC=0
    choose || CHOOSE_RC=$?
    if [ "$CHOOSE_RC" = 2 ]; then
      # devicectl finds devices in the background; one just plugged in can
      # take a few seconds to show. Look once more.
      sleep 4
      list_devices || true
      CHOOSE_RC=0
      choose || CHOOSE_RC=$?
    fi
  fi
fi

# Several in reach and nothing said which: ask (a real run), or check them
# all (a dry run).
if [ "$CHOOSE_RC" = 5 ] && [ "$DRY_RUN" = 0 ]; then
  if [ "$HAVE_ANSWER" = 0 ] && ! [ -t 0 ]; then
    {
      say ""
      say "More than one $KINDS is connected:"
      n=0
      while IFS=$'\t' read -r _ _ _ _ words; do
        n=$((n + 1))
        say "  $n. $words"
      done <"$WORK/chosen.txt"
    } >&2
    die "which one? Nothing here can ask (no terminal). Name it:
  $INSTALL_CMD -- --device NAME_OR_UDID     (repeat it for more than one)
  $INSTALL_CMD -- --all                     (every one of them)"
  fi
  say ""
  say "More than one $KINDS is connected. Which should get $NAME?"
  n=0
  while IFS=$'\t' read -r _ _ _ _ words; do
    n=$((n + 1))
    say "  $n. $words"
  done <"$WORK/chosen.txt"
  printf 'Type a number, several (1 3), or a for all of them. Enter alone stops: '
  if [ "$HAVE_ANSWER" = 0 ]; then
    IFS= read -r ANSWER || ANSWER=""
  else
    say "$ANSWER"
  fi
  case "$ANSWER" in
    a | A | all | ALL | All) ALL=1 ;;
    "") die "nothing chosen. Nothing was changed." ;;
    *)
      for pick in $ANSWER; do
        case "$pick" in
          *[!0-9]*) die "\"$pick\" is not one of the numbers. Nothing was changed." ;;
        esac
        udid="$(sed -n "${pick}p" "$WORK/chosen.txt" | cut -f1)"
        [ -n "$udid" ] && [ "$pick" -gt 0 ] || die "there is no number $pick in the list. Nothing was changed."
        WANTS+=("$udid")
      done
      ;;
  esac
  CHOOSE_RC=0
  choose || CHOOSE_RC=$?
fi

# Each device's lock state, and more detail if the list did not say whether
# Developer Mode is on. Both only read.
mkdir "$WORK/lock" "$WORK/details"
if [ "$CHOOSE_RC" = 0 ] || [ "$CHOOSE_RC" = 5 ]; then
  while IFS=$'\t' read -r udid _ pairing devmode _ <&3; do
    if [ -n "$LOCK_JSON" ]; then
      cp "$LOCK_JSON" "$WORK/lock/$udid.json"
    elif [ "$FAKE_INPUTS" = 0 ] && [ "$pairing" != unpaired ]; then
      xcrun devicectl device info lockState --device "$udid" --timeout 20 --json-output "$WORK/lock/$udid.json" >"$WORK/lock/$udid.log" 2>&1 </dev/null || true
      if [ "$devmode" = unknown ]; then
        xcrun devicectl device info details --device "$udid" --timeout 20 --json-output "$WORK/details/$udid.json" >"$WORK/details/$udid.log" 2>&1 </dev/null || true
      fi
    fi
  done 3<"$WORK/chosen.txt"
fi

# ---- 3. the app Bazel built -------------------------------------------------------

APP=""
IPA=""
if [ "${APPLE_INSTALL_NEED_IPA:-}" = 1 ] && [ -z "$IPA_ARG" ] && [ "$DRY_RUN" = 0 ]; then
  die "this target installs an app built elsewhere: give it with -- --ipa FILE"
fi
if [ -n "$IPA_ARG" ]; then
  [ -f "$IPA_ARG" ] || die "there is no file $IPA_ARG (--ipa)."
  IPA="$(resolve "$IPA_ARG")"
elif [ -n "${APPLE_INSTALL_IPA:-}" ]; then
  IPA="$(resolve "$APPLE_INSTALL_IPA")" || die "the built app is missing ($APPLE_INSTALL_IPA). Run this with: $INSTALL_CMD"
fi
: >"$WORK/app.txt"
if [ -n "$IPA" ]; then
  mkdir "$WORK/app"
  if unzip -q "$IPA" -d "$WORK/app" 2>"$WORK/unzip.log"; then
    for candidate in "$WORK/app/Payload/"*.app; do
      [ -d "$candidate" ] && APP="$candidate" && break
    done
  fi
  if [ -n "$APP" ]; then
    {
      say "bundle_id=$(py bundle-id "$APP")"
      say "name=$(basename "$APP")"
      say "size=$(du -sh "$APP" | cut -f1 | tr -d ' ')"
      while IFS= read -r appex; do
        say "extension=$(py bundle-id "$appex")"
      done < <(find "$APP" -mindepth 1 -name '*.appex' -type d | sort)
    } >"$WORK/app.txt"
  else
    say "error=The built app could not be unpacked: $(basename "$IPA") holds no Payload/*.app." >"$WORK/app.txt"
  fi
fi

# ---- 4. signing identities, profiles, Xcode's sign-in ---------------------------------

if [ -n "$IDENTITIES_FILE" ]; then
  cp "$IDENTITIES_FILE" "$WORK/identities.txt"
  : >"$WORK/identities-all.txt"
else
  security find-identity -v -p codesigning >"$WORK/identities.txt" 2>/dev/null || true
  security find-identity -p codesigning >"$WORK/identities-all.txt" 2>/dev/null || true
fi

if [ -n "$XCODE_DEFAULTS" ]; then
  cp "$XCODE_DEFAULTS" "$WORK/xcode-defaults.plist"
elif [ "$FAKE_INPUTS" = 0 ]; then
  defaults export com.apple.dt.Xcode "$WORK/xcode-defaults.plist" >/dev/null 2>&1 || true
fi

if [ -n "$PROFILES_DIR" ]; then
  PROFILE_DIRS=("$PROFILES_DIR")
else
  PROFILE_DIRS=(
    "$HOME/Library/Developer/Xcode/UserData/Provisioning Profiles"
    "$HOME/Library/MobileDevice/Provisioning Profiles"
  )
fi

# A profile is a property list wrapped in Apple's signature. `security` unwraps
# it; where it cannot reach a keychain (a build machine), openssl does. The
# signature is not checked here: the device checks it.
decode_one() { # profile, output
  security cms -D -i "$1" -o "$2" 2>>"$WORK/decode.log" && [ -s "$2" ] && return 0
  openssl smime -inform der -verify -noverify -in "$1" -out "$2" 2>>"$WORK/decode.log" && [ -s "$2" ] && return 0
  rm -f "$2"
  return 1
}

# Decodes every profile into $WORK/decoded/N.plist, with the file it came from
# in N.src. A fixture already decoded (.plist) is taken as it is.
decode_profiles() { # folders or files
  local n=0 src f
  rm -rf "$WORK/decoded"
  mkdir "$WORK/decoded"
  for src in "$@"; do
    [ -e "$src" ] || continue
    while IFS= read -r f; do
      n=$((n + 1))
      case "$f" in
        *.plist) cp "$f" "$WORK/decoded/$n.plist" ;;
        *) decode_one "$f" "$WORK/decoded/$n.plist" || continue ;;
      esac
      printf '%s\n' "$f" >"$WORK/decoded/$n.src"
    done < <(find -L "$src" -maxdepth 1 \( -name '*.mobileprovision' -o -name '*.plist' \) -type f 2>/dev/null | sort)
  done
}

decode_profiles "${PROFILE_DIRS[@]}"

# ---- 5. the preflight: a tick or a cross a line -----------------------------------------

set_want_args
PRE=(--host "$WORK/host.txt" --identities "$WORK/identities.txt"
  --identities-all "$WORK/identities-all.txt" --xcode-defaults "$WORK/xcode-defaults.plist"
  --dir "$WORK/decoded" --team "$TEAM" --default-team "$DEFAULT_TEAM"
  --now "$NOW" --app-facts "$WORK/app.txt" --out "$WORK/plan"
  --lock-dir "$WORK/lock" --details-dir "$WORK/details" "${APP_ARGS[@]}")
[ ${#WANT_ARGS[@]} -gt 0 ] && PRE+=("${WANT_ARGS[@]}")
[ -e "$WORK/devices.json" ] && PRE+=(--devices-json "$WORK/devices.json")
[ "$DRY_RUN" = 1 ] && PRE+=(--dry-run)
case "${LC_ALL:-${LC_CTYPE:-${LANG:-}}}" in
  *UTF-8* | *utf8* | *UTF8* | *utf-8*) ;;
  *) PRE+=(--ascii) ;;
esac

pre_rc=0
py preflight "${PRE[@]}" || pre_rc=$?
if [ "$pre_rc" != 0 ] && [ "$pre_rc" != 10 ]; then
  die "the checks themselves broke (provision.py preflight, exit $pre_rc). This is a bug in the installer."
fi
plan() { cat "$WORK/plan/$1" 2>/dev/null || true; }
plan devices | sed '/^$/d' >"$WORK/devices.txt"
plan skipped | sed '/^$/d' >"$WORK/skipped.txt"
NEED_MINT="$(plan need_mint)"
MINT_TEAM="$(plan team)"
MINT_UDIDS="$(plan mint_udids)"
UDID_ARGS=()
while IFS=$'\t' read -r udid _; do
  UDID_ARGS+=(--udid "$udid")
done <"$WORK/devices.txt"
FIRST_UDID="$(head -n 1 "$WORK/devices.txt" | cut -f1)"

pick_profiles() { # fills IDENTITY_* and PROFILE_*; false if no set will do
  py pick --dir "$WORK/decoded" ${UDID_ARGS[@]+"${UDID_ARGS[@]}"} --identities "$WORK/identities.txt" \
    --team "$TEAM" --now "$NOW" --out "$WORK/sign" "${APP_ARGS[@]}" >"$WORK/pick.txt" 2>"$WORK/profile-why.txt" || return 1
  {
    IFS= read -r IDENTITY_SHA
    IFS= read -r IDENTITY_NAME
    IFS= read -r PROFILE_TEAM
    IFS= read -r PROFILE_EXPIRES
  } <"$WORK/pick.txt"
}

# What a tool's failure most likely means, after its own words. Never in
# place of them: the caller prints the raw output first.
explain() { # udid, then log files
  local udid="$1" logs=() f
  shift
  for f in "$@"; do
    logs+=(--log "$f")
  done
  if py explain "${logs[@]}" --udid "$udid" --team "${PROFILE_TEAM:-$MINT_TEAM}" --bundle-id "$BUNDLE_ID" \
    --min-os "$MIN_OS" --name "$NAME" --install-cmd "$INSTALL_CMD" >"$WORK/explain.txt"; then
    say ""
    say "What this most likely means:"
    sed 's/^/  /' "$WORK/explain.txt"
  else
    say ""
    say "This installer does not know this failure. The tool's own words are above;"
    say "its full output is in the folder named below."
  fi
}

# ---- 6. sign (defined here: the dry run's test uses it too) ---------------------------------

# One bundle: its profile inside it, then codesign with its entitlements.
sign_bundle() { # identity, bundle folder, profile file, entitlements file, what
  cp "$3" "$2/embedded.mobileprovision"
  if ! codesign --force --sign "$1" --timestamp=none \
    --entitlements "$4" --generate-entitlement-der "$2" 2>"$WORK/codesign.log"; then
    cat "$WORK/codesign.log" >&2
    explain "" "$WORK/codesign.log" >&2
    die "codesign could not sign $5. If a window asked to use the key, choose Always Allow and run this again."
  fi
}

# Inside out: frameworks and libraries first (no profile, no entitlements),
# then each extension with its own profile and entitlements, the app last.
sign_app() { # identity; the profiles and entitlements are in $WORK/sign/sign.txt
  local identity="$1" nested bid line profile ents
  # Finder leftovers make codesign refuse ("detritus not allowed").
  xattr -cr "$APP" 2>/dev/null || true
  while IFS= read -r nested; do
    codesign --force --sign "$identity" --timestamp=none "$nested" 2>"$WORK/codesign.log" || {
      cat "$WORK/codesign.log" >&2
      explain "" "$WORK/codesign.log" >&2
      die "codesign could not sign ${nested#"$APP"/}."
    }
  done < <(find "$APP" -mindepth 1 -depth \( -name '*.framework' -o -name '*.dylib' \))
  while IFS= read -r nested; do
    bid="$(py bundle-id "$nested")"
    line="$(awk -F'\t' -v b="$bid" '$1 == b' "$WORK/sign/sign.txt")"
    [ -n "$line" ] || die "the app holds the extension $bid, and there is no profile for it.
Add it to extensions in the app's ios_install (BUILD.bazel)."
    profile="$(printf '%s' "$line" | cut -f2)"
    ents="$(printf '%s' "$line" | cut -f3)"
    sign_bundle "$identity" "$nested" "$profile" "$ents" "the extension $bid"
  done < <(find "$APP" -mindepth 1 -depth -name '*.appex' -type d)
  line="$(awk -F'\t' -v b="$BUNDLE_ID" '$1 == b' "$WORK/sign/sign.txt")"
  sign_bundle "$identity" "$APP" "$(printf '%s' "$line" | cut -f2)" "$(printf '%s' "$line" | cut -f3)" "the app"
  codesign --verify --strict --deep "$APP" 2>"$WORK/codesign.log" || {
    cat "$WORK/codesign.log" >&2
    die "the signature does not check out."
  }
}

if [ "$DRY_RUN" = 1 ]; then
  if [ "$pre_rc" != 0 ]; then
    say ""
    say "Dry run: nothing was changed. Fix the lines marked above, then run it again."
    exit 1
  fi
  say ""
  say "Dry run: everything needed is in place. Nothing was changed."
  say "The real thing:  $INSTALL_CMD"
  if [ "$SIGN_ADHOC" = 1 ]; then
    step "Test: signing with no identity"
    [ -n "$APP" ] || die "--sign-adhoc needs the built app."
    pick_profiles || die "--sign-adhoc needs profiles that fit."
    sign_app -
    while IFS= read -r nested; do
      say "$(basename "$nested"):"
      codesign -d --entitlements - --xml "$nested" 2>/dev/null | plutil -convert json -o - - || true
      say ""
    done < <(find "$APP" -name '*.appex' -type d; printf '%s\n' "$APP")
    say "The codesign commands work."
  fi
  exit 0
fi

if [ "$pre_rc" != 0 ]; then
  die "fix the lines marked above, then run this again. Nothing was changed.
To see the checks alone:  $INSTALL_CMD -- --dry-run"
fi
[ -n "$APP" ] || die "there is no built app to install. Run this with: $INSTALL_CMD
(or give one built elsewhere: --ipa FILE)"
[ -s "$WORK/devices.txt" ] || die "no device is ready. Nothing was changed."

# ---- 7. profiles: Xcode makes them if none fit ------------------------------------------

human_steps() {
  cat >&2 <<EOF

What to do, once, by hand:
  1. Open Xcode. Xcode > Settings > Accounts. Sign in with the Apple ID of the
     developer team, and check the team ($MINT_TEAM) is listed there with no warning.
  2. Plug the device in with a cable and unlock it. Tap Trust if it asks.
  3. On the device: Settings > Privacy & Security > Developer Mode, on, Restart.
  4. In Xcode: Window > Devices and Simulators. Wait until the device shows with
     no yellow bar ("preparing").
  5. Run this again:  $INSTALL_CMD
If the Mac has more than one team:  ... -- --team TEAMID
The sure way, if this keeps failing: in Xcode, make any new iOS app project
with the bundle id $BUNDLE_ID and team $MINT_TEAM, pick the device as the run
destination and press Run once. That makes the same profile. Then run this again.
EOF
}

# Xcode's own complaints, out of a long build log.
mint_errors() {
  grep -i -E -A3 'error[: ]|ineligible|failed' "$WORK/xcodebuild.log" >"$WORK/mint-errors.txt" 2>/dev/null || true
  [ -s "$WORK/mint-errors.txt" ] || tail -n 25 "$WORK/xcodebuild.log" >"$WORK/mint-errors.txt"
  cat "$WORK/mint-errors.txt" >>"$WORK/mint-errors-all.txt"
}

# The profiles are in Xcode's folder, and inside the apps Xcode just built.
MINTED=0
collect_minted() {
  local f
  mkdir -p "$WORK/minted"
  while IFS= read -r f; do
    MINTED=$((MINTED + 1))
    cp "$f" "$WORK/minted/$MINTED.mobileprovision"
  done < <(find "$WORK/mint/derived" -name embedded.mobileprovision -type f 2>/dev/null)
}

device_name() { # udid
  awk -F'\t' -v u="$1" '$1 == u { print $2 }' "$WORK/devices.txt"
}

if [ "$NEED_MINT" != 0 ]; then
  [ -n "$MINT_TEAM" ] || die "cannot tell which developer team to use. Pass it: -- --team TEAMID"
  if [ "$NEED_MINT" = renew ]; then
    step "Asking Xcode for fresh profiles (team $MINT_TEAM): the old ones run out soon"
  else
    step "Asking Xcode to make the profiles (team $MINT_TEAM)"
  fi
  say "Xcode registers each device with the team and creates a development profile for"
  say "$BUNDLE_ID${APPLE_INSTALL_EXTENSIONS:+ and its extensions}. This needs the internet and the team's"
  say "Apple ID signed in to Xcode. About a minute a device. Keep them unlocked."
  : >"$WORK/mint-errors-all.txt"
  mint_ok=1
  for udid in $MINT_UDIDS; do
    say "  for \"$(device_name "$udid")\""
    if ! mint_build "$MINT_TEAM" -destination "id=$udid" -destination-timeout 60 \
      -allowProvisioningUpdates -allowProvisioningDeviceRegistration; then
      mint_ok=0
      mint_errors
      cp "$WORK/xcodebuild.log" "$WORK/xcodebuild-$udid.log"
    fi
    collect_minted
  done
  # A new certificate may have been made too: read the keychain again.
  security find-identity -v -p codesigning >"$WORK/identities.txt" 2>/dev/null || true
  decode_profiles "${PROFILE_DIRS[@]}" "$WORK/minted"
  if ! pick_profiles && [ "$mint_ok" = 0 ] && grep -q -i 'destination' "$WORK/mint-errors-all.txt"; then
    # Xcode could not use a device as a build destination (locked, busy,
    # still being prepared). A profile needs no destination if the devices
    # are already on the team's list: ask again without one.
    say "Xcode could not reach a device to build for it. Asking again without it."
    if ! mint_build "$MINT_TEAM" -destination "generic/platform=iOS" \
      -allowProvisioningUpdates -allowProvisioningDeviceRegistration; then
      mint_errors
    fi
    collect_minted
    security find-identity -v -p codesigning >"$WORK/identities.txt" 2>/dev/null || true
    decode_profiles "${PROFILE_DIRS[@]}" "$WORK/minted"
  fi
  TEAM="${TEAM:-$MINT_TEAM}"
  if ! pick_profiles; then
    {
      say ""
      say "Xcode said:"
      sort -u "$WORK/mint-errors-all.txt" | grep -v '^--$' | head -n 20 | sed 's/^/  /'
      if [ -s "$WORK/profile-why.txt" ]; then
        say ""
        sed 's/^/  /' "$WORK/profile-why.txt"
      fi
      if py gaps --dir "$WORK/decoded" ${UDID_ARGS[@]+"${UDID_ARGS[@]}"} --identities "$WORK/identities.txt" \
        --team "$TEAM" --now "$NOW" "${APP_ARGS[@]}" >"$WORK/gaps.txt"; then
        say ""
        sed 's/^/  /' "$WORK/gaps.txt"
      fi
      explain "$FIRST_UDID" "$WORK/mint-errors-all.txt" "$WORK/profile-why.txt"
    } >&2
    [ -s "$WORK/gaps.txt" ] || human_steps
    die "there are still no development profiles for $BUNDLE_ID${APPLE_INSTALL_EXTENSIONS:+ and its extensions} that list every device."
  fi
  if [ "$mint_ok" = 0 ]; then
    say "Xcode complained, but profiles that fit are here. Going on with them."
  fi
else
  pick_profiles || die "the profiles the checks found are gone. Run this again."
fi
while IFS=$'\t' read -r bid _ _ pname; do
  say "Profile for $bid: $pname"
done <"$WORK/sign/sign.txt"
say "They run out on $PROFILE_EXPIRES (team $PROFILE_TEAM). After that day the app will not open until you run this again."
say "Signing as: $IDENTITY_NAME"

step "Signing the app"
say "If a window asks to use a key from the keychain, choose Always Allow."
sign_app "$IDENTITY_SHA"
say "Signed."

# ---- 8. install and start, device by device ------------------------------------------------

# Runs a devicectl command, showing its own progress lines as they come and
# keeping them in a log. devicectl also writes what went wrong, with error
# codes, to a JSON file beside it.
devicectl_run() { # log name, the app or bundle id, then the devicectl command and its options
  local name="$1" target="$2"
  shift 2
  xcrun devicectl "$@" --json-output "$WORK/$name.json" "$target" </dev/null 2>&1 | tee "$WORK/$name.log" | sed 's/^/    /'
}

COUNT="$(wc -l <"$WORK/devices.txt" | tr -d ' ')"
RUNNING=()
NOT_STARTED=()
NOT_INSTALLED=()
n=0
while IFS=$'\t' read -r udid dname <&3; do
  n=$((n + 1))
  sfx=""
  [ "$COUNT" -gt 1 ] && sfx="-$n"
  step "Installing on \"$dname\""
  say "The first time, Xcode first copies its support files to the device. That can take a few minutes."
  if ! devicectl_run "install$sfx" "$APP" device install app --device "$udid"; then
    explain "$udid" "$WORK/install$sfx.log" "$WORK/install$sfx.json" >&2
    say "Not installed on \"$dname\"." >&2
    NOT_INSTALLED+=("$dname")
    continue
  fi
  say "Installed."
  step "Starting $NAME on \"$dname\""
  if ! devicectl_run "launch$sfx" "$BUNDLE_ID" device process launch --device "$udid" --terminate-existing; then
    # --terminate-existing is "not supported on all platforms": once more without.
    cp "$WORK/launch$sfx.log" "$WORK/launch$sfx-first.log"
    if ! devicectl_run "launch$sfx" "$BUNDLE_ID" device process launch --device "$udid"; then
      explain "$udid" "$WORK/launch$sfx.log" "$WORK/launch$sfx.json" "$WORK/launch$sfx-first.log" >&2
      say "" >&2
      say "$NAME IS on \"$dname\"; only starting it from here failed. Tap its icon." >&2
      say "The first start needs the device on Wi-Fi once, so iOS can check the developer with Apple." >&2
      NOT_STARTED+=("$dname")
      continue
    fi
  fi
  RUNNING+=("$dname")
done 3<"$WORK/devices.txt"

say ""
if [ "$COUNT" = 1 ] && [ ${#RUNNING[@]} = 1 ] && [ ! -s "$WORK/skipped.txt" ]; then
  say "$NAME is running on \"${RUNNING[0]}\"."
else
  step "Done"
  for d in ${RUNNING[@]+"${RUNNING[@]}"}; do say "  running on \"$d\""; done
  for d in ${NOT_STARTED[@]+"${NOT_STARTED[@]}"}; do say "  installed on \"$d\", but not started: tap its icon"; done
  for d in ${NOT_INSTALLED[@]+"${NOT_INSTALLED[@]}"}; do say "  NOT installed on \"$d\" (why is above)"; done
  while IFS=$'\t' read -r _ d; do say "  left out: \"$d\" (the checks above say why)"; done <"$WORK/skipped.txt"
fi
say "It will open until $PROFILE_EXPIRES. To renew, run this again."
if [ ${#NOT_INSTALLED[@]} -gt 0 ]; then
  die "$NAME was not installed on ${#NOT_INSTALLED[@]} of $COUNT device(s)."
fi
if [ ${#NOT_STARTED[@]} -gt 0 ]; then
  FAILED=1
  exit 1
fi
