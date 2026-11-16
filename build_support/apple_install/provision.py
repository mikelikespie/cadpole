#!/usr/bin/env python3
"""The choosing behind install.sh (ios_install's installer): which devices, which provisioning profiles, which team, what
entitlements for the app and each of its extensions; the checks before an
install (`preflight`, the "doctor"); and what a tool's failure most likely
means (`explain`). Plain data in, plain lines out, so it can be tested with
made-up files on any machine (`self-test`).

One app is described by the common options (--name, --bundle-id,
--entitlements, --extension, --family, --min-os, --install-cmd), which
ios_install passes through install.sh.

Standard library only, and nothing newer than Python 3.9: on the laptop this
runs under the python3 that comes with Xcode.

install.sh decodes each .mobileprovision with `security cms -D -i` first; this
file only ever reads the decoded property lists.
"""

import argparse
import datetime
import hashlib
import json
import os
import plistlib
import re
import sys

# ---- devices -----------------------------------------------------------------

# The JSON `xcrun devicectl list devices --json-output FILE` writes. Checked
# against Xcode 27.2 (devicectl 651.32, "jsonVersion" 5) on 2026-11-14, with a
# Simulator in the list and no real device. What that showed:
#
#   - every device has BOTH the old keys (hardwareProperties, deviceProperties,
#     connectionProperties: "deprecated", to be removed) and a new "properties"
#     dictionary (hardware, software, connection, state);
#   - Simulators are in the list, with a UDID, reality "simulated",
#     visibilityClass "simulators" and transportType "sameMachine";
#   - the new connection state is properties.connection.state (the old
#     connectionProperties.tunnelState); the new name is properties.state.name;
#     the new OS version is a dictionary with a "stringValue".
#
# Not seen, because no real device was there: developerModeStatus (the old
# place is deviceProperties.developerModeStatus; where it sits under
# "properties" is a guess), and the values a real iPad gives for the
# connection. So every field is looked up in several places, and
# developerModeStatus is searched for anywhere in the entry.
JSON_VERSION_CHECKED = 5

_MISSING = object()


def _dig(node, path):
    for part in path.split("."):
        if not isinstance(node, dict) or part not in node:
            return _MISSING
        node = node[part]
    return node


def _first(raw, *paths):
    """The first of these dotted paths that holds something."""
    for path in paths:
        v = _dig(raw, path)
        if v is not _MISSING and v not in (None, "", [], {}):
            return v
    return None


def _find_key(node, key, depth=0):
    """The value of `key` wherever it is inside node, or _MISSING."""
    if depth > 8:
        return _MISSING
    if isinstance(node, dict):
        if key in node and node[key] not in (None, "", {}):
            return node[key]
        for v in node.values():
            found = _find_key(v, key, depth + 1)
            if found is not _MISSING:
                return found
    elif isinstance(node, list):
        for v in node:
            found = _find_key(v, key, depth + 1)
            if found is not _MISSING:
                return found
    return _MISSING


def _text(v):
    """A string from a value that may be a string, a number, or one of
    devicectl's version dictionaries ({"stringValue": "27.2", ...})."""
    if isinstance(v, str):
        return v
    if isinstance(v, bool) or v is None:
        return ""
    if isinstance(v, (int, float)):
        return str(v)
    if isinstance(v, dict):
        for key in ("stringValue", "name", "string", "description"):
            if isinstance(v.get(key), str):
                return v[key]
        comps = v.get("components")
        if isinstance(comps, list) and comps:
            n = v.get("originalComponentsCount")
            if isinstance(n, int) and 0 < n <= len(comps):
                comps = comps[:n]
            return ".".join(str(c) for c in comps)
    return ""


def version_tuple(text):
    """(27, 2) from "27.2"; () when there is no number."""
    out = []
    for part in re.split(r"[.\s]", text or ""):
        m = re.match(r"\d+", part)
        if not m:
            break
        out.append(int(m.group(0)))
    return tuple(out)


def _on_off(v):
    if isinstance(v, bool):
        return "enabled" if v else "disabled"
    t = _text(v).strip().lower()
    if t in ("enabled", "on", "true", "yes"):
        return "enabled"
    if t in ("disabled", "off", "false", "no"):
        return "disabled"
    return "unknown"


_RANK_CONNECTED = 0
_RANK_REACH = 1
_UNREACHABLE = 2


class Device:
    def __init__(self, raw):
        self.raw = raw if isinstance(raw, dict) else {}
        raw = self.raw
        self.udid = _text(_first(raw, "properties.hardware.udid", "hardwareProperties.udid"))
        self.identifier = _text(_first(raw, "identifier"))
        self.kind = _text(_first(raw, "properties.hardware.deviceType", "hardwareProperties.deviceType"))
        self.product = _text(_first(raw, "properties.hardware.productType", "hardwareProperties.productType"))
        if not self.kind:
            m = re.match(r"(iPad|iPhone|iPod|AppleTV|Watch|RealityDevice|Mac)", self.product)
            self.kind = m.group(1) if m else ""
        marketing = _text(_first(raw, "properties.hardware.marketingName", "hardwareProperties.marketingName"))
        self.model = marketing or self.product or self.kind
        self.name = (
            _text(_first(raw, "properties.state.name", "deviceProperties.name", "properties.device.name", "name"))
            or marketing
            or "unnamed device"
        )
        self.reality = _text(_first(raw, "properties.hardware.reality", "hardwareProperties.reality")).lower()
        self.visibility = _text(_first(raw, "properties.state.visibilityClass", "visibilityClass"))
        self.provider = _text(_first(raw, "deviceProperties.provider", "properties.state.provider", "provider"))
        self.os_version = _text(_first(raw, "properties.software.osVersionNumber", "deviceProperties.osVersionNumber"))
        self.boot_state = _text(_first(raw, "properties.state.bootState", "deviceProperties.bootState"))
        self.pairing = (
            _text(_first(raw, "properties.connection.pairingState", "connectionProperties.pairingState")) or "unknown"
        )
        self.tunnel = (
            _text(_first(raw, "properties.connection.state", "properties.connection.tunnelState", "connectionProperties.tunnelState"))
            or "unknown"
        )
        self.transport = _text(_first(raw, "properties.connection.transportType", "connectionProperties.transportType"))
        mode = _first(
            raw,
            "deviceProperties.developerModeStatus",
            "properties.state.developerModeStatus",
            "properties.software.developerModeStatus",
            "properties.security.developerModeStatus",
        )
        if mode is None:
            found = _find_key(raw, "developerModeStatus")
            mode = None if found is _MISSING else found
        self.developer_mode = _on_off(mode) if mode is not None else "unknown"
        ddi = _first(raw, "deviceProperties.ddiServicesAvailable", "properties.state.ddiServicesAvailable")
        self.ddi_ready = ddi if isinstance(ddi, bool) else None

    @property
    def is_physical(self):
        if self.reality in ("simulated", "virtual"):
            return False
        if self.visibility == "simulators" or self.transport == "sameMachine":
            return False
        return "CoreSimulator" not in self.provider

    @property
    def rank(self):
        if self.tunnel == "connected":
            return _RANK_CONNECTED
        # "unavailable" on a paired device: this Mac remembers it, but it is
        # not here now. An unpaired one stays in reach, so that the run stops
        # at "not paired" with the fix, not at "no iPad".
        if self.tunnel == "unavailable" and self.pairing != "unpaired":
            return _UNREACHABLE
        return _RANK_REACH

    def kind_ok(self, families):
        """A real device of a kind the app is built for, with a UDID."""
        return self.is_physical and bool(self.udid) and self.kind in [FAMILY_KINDS[f] for f in families]

    def os_name(self):
        return {"iPad": "iPadOS", "iPhone": "iOS"}.get(self.kind, "iOS")

    def connection(self):
        """Cable or network, in plain words."""
        return {
            "wired": "by cable",
            "localNetwork": "over the network (Wi-Fi)",
            "peerToPeer": "over a direct wireless link",
        }.get(self.transport, self.transport and "by " + self.transport or "")

    def state(self):
        if self.rank == _RANK_CONNECTED:
            how = self.connection()
            return "connected" + (" " + how if how else "")
        if self.rank == _RANK_REACH:
            how = self.connection()
            return "in reach" + (" " + how if how else "") + ", not connected yet"
        return "not in reach"

    def describe(self):
        os_part = ", %s %s" % (self.os_name(), self.os_version) if self.os_version else ""
        return '"%s" (%s%s, %s) %s' % (self.name, self.model, os_part, self.state(), self.udid)

    def sort_key(self):
        return (self.rank, 0 if self.transport == "wired" else 1, self.name.lower(), self.udid)

    def dump(self):
        return json.dumps(self.raw, indent=1, sort_keys=True, default=str)[:6000]


# The ios_application `families` values, and the devicectl deviceType of each.
FAMILY_KINDS = {"iphone": "iPhone", "ipad": "iPad"}


def kinds_text(families, joiner="or"):
    """ "iPhone or iPad", "iPad"."""
    kinds = [FAMILY_KINDS[f] for f in families]
    return (" %s " % joiner).join(kinds)


class Refusal(Exception):
    """A plain-language reason the installer cannot go on."""


class DeviceList:
    def __init__(self, devices, json_version, problems):
        self.devices = devices
        self.json_version = json_version
        self.problems = problems  # physical-looking entries we could not read


def load_device_list(path):
    try:
        with open(path, "rb") as f:
            doc = json.load(f)
    except (OSError, ValueError) as e:
        raise Refusal("Could not read devicectl's device list (%s): %s" % (path, e))
    if not isinstance(doc, dict):
        raise Refusal("devicectl's device list is not a JSON dictionary. Run with --keep and read devices.json.")
    info = doc.get("info") if isinstance(doc.get("info"), dict) else {}
    raw_devices = _dig(doc, "result.devices")
    if raw_devices is _MISSING or not isinstance(raw_devices, list):
        err = _find_key(doc.get("error"), "NSLocalizedDescription")
        raise Refusal(
            'devicectl\'s JSON has no "result.devices" list (looked for the key "result", then "devices" inside it).\n'
            "Its top-level keys are: %s. JSON version: %s.%s\n"
            "Run with --keep and read devices.json; the reading is in build_support/apple_install/provision.py (class Device)."
            % (
                ", ".join(sorted(doc)) or "(none)",
                info.get("jsonVersion", "not given"),
                "" if err is _MISSING else "\ndevicectl said: " + _text(err),
            )
        )
    devices, problems = [], []
    for raw in raw_devices:
        d = Device(raw)
        if d.is_physical and (not d.udid or not d.kind):
            missing = []
            if not d.udid:
                missing.append('the UDID (looked at "properties.hardware.udid", then "hardwareProperties.udid")')
            if not d.kind:
                missing.append(
                    'the kind of device (looked at "properties.hardware.deviceType", "hardwareProperties.deviceType", '
                    'then the start of "productType")'
                )
            problems.append((d, missing))
        devices.append(d)
    return DeviceList(devices, info.get("jsonVersion"), problems)


def load_devices(path):
    return load_device_list(path).devices


def _unreadable(problems):
    out = ["devicectl listed a device this installer could not read."]
    for d, missing in problems:
        out.append('For "%s" it could not find %s.' % (d.name, " or ".join(missing)))
        out.append("The device's entry, as devicectl wrote it:")
        out.append(d.dump())
    out.append("The reading is in build_support/apple_install/provision.py (class Device): add the key there.")
    return "\n".join(out)


class Several(Refusal):
    """More than one device could be meant, and nothing said which: the
    installer asks (or, with no one to ask, stops with the list)."""

    def __init__(self, text, devices):
        Refusal.__init__(self, text)
        self.devices = devices


def _plain(text):
    # iOS names a device with a curly apostrophe ("Nora’s iPhone"); a
    # keyboard types a straight one.
    return text.strip().lower().replace("’", "'")


def several_text(devices, app, install_cmd):
    out = ["Found %d devices this can go on, and nothing says which:" % len(devices)]
    for i, d in enumerate(devices, 1):
        out.append("  %d. %s" % (i, d.describe()))
    out.append("To choose:  %s -- --device NAME_OR_UDID   (repeat it for more than one)" % install_cmd)
    out.append("For every one of them:  %s -- --all" % install_cmd)
    return "\n".join(out)


def pick_devices(devices, wants=(), all_=False, families=("iphone", "ipad"), app="The app",
                 install_cmd="bazel run :install", problems=()):
    """Returns (devices to install on, notes). Raises Refusal when there is
    nothing to pick, Several when more than one is in reach and neither
    --device nor --all said which."""
    notes = []
    real = [d for d in devices if d.is_physical]
    kinds = kinds_text(families)
    if wants:
        chosen = []
        for want in wants:
            w = _plain(want)
            hits = [d for d in real if w in (d.udid.lower(), d.identifier.lower(), _plain(d.name))]
            if not hits:
                sims = [d for d in devices if not d.is_physical and w in (d.udid.lower(), d.identifier.lower(), _plain(d.name))]
                if sims:
                    raise Refusal('"%s" is a Simulator, not a real %s. %s goes on a real one.' % (sims[0].name, kinds, app))
                seen = "\n".join("  " + d.describe() for d in real) or "  (none)"
                raise Refusal('No device called "%s". This Mac knows:\n%s' % (want, seen))
            hits.sort(key=Device.sort_key)
            d = hits[0]
            if not d.kind_ok(families):
                if not d.kind or not d.udid:
                    raise Refusal(_unreadable([p for p in problems if p[0] is d] or [(d, ["what it is"])]))
                raise Refusal('"%s" is %s, not an %s. %s goes on an %s only.' % (
                    d.name, d.model or "another kind of device", kinds, app, kinds))
            if len(hits) > 1:
                notes.append('More than one device is called "%s". Using %s. Pass its UDID to choose another.' % (want, d.udid))
            if d not in chosen:
                chosen.append(d)
        return chosen, notes

    fits = sorted((d for d in real if d.kind_ok(families)), key=Device.sort_key)
    reach = [d for d in fits if d.rank < _UNREACHABLE]
    if not reach:
        if problems:
            raise Refusal(_unreadable(problems))
        if fits:
            seen = "\n".join("  " + d.describe() for d in fits)
            raise Refusal(
                "This Mac knows these devices, but none is in reach now:\n%s\n"
                "Plug one in with a cable, unlock it, and run this again." % seen
            )
        others = [d for d in real if d.rank < _UNREACHABLE and d.udid]
        wrong = [d for d in others if d.kind in FAMILY_KINDS.values()]
        if wrong:
            raise Refusal(
                'No %s found, only %s ("%s"). %s goes on an %s only.\n'
                "Plug the %s in with a cable, unlock it, and tap Trust if it asks." % (
                    kinds, "an " + wrong[0].kind, wrong[0].name, app, kinds, kinds)
            )
        extra = ""
        if others:
            extra = "\nOther devices in reach (not an %s):\n" % kinds + "\n".join("  " + d.describe() for d in others)
        raise Refusal(
            "No %s found. Plug it in with a cable, unlock it, and tap Trust "
            "if it asks. Then run this again." % kinds + extra
        )
    # Devices connected now come before ones only remembered as in reach:
    # with one connected, that is the one, whatever else the Mac remembers.
    connected = [d for d in reach if d.rank == _RANK_CONNECTED]
    pool = connected or reach
    if all_:
        chosen = pool
    elif len(pool) == 1:
        chosen = pool
    else:
        raise Several(several_text(pool, app, install_cmd), pool)
    rest = [o for o in fits if o not in chosen]
    if rest and not all_:
        notes.append("Using %s." % chosen[0].describe())
        notes.append("Also known to this Mac, not connected now:")
        notes.extend("  " + o.describe() for o in rest)
        notes.append("To choose another: %s -- --device NAME_OR_UDID" % install_cmd)
    for p, missing in problems:
        notes.append('Passed over "%s": could not find %s in its entry.' % (p.name, " or ".join(missing)))
    return chosen, notes


def read_lock_state(path):
    """"locked", "unlocked" or "unknown", from `devicectl device info lockState
    --json-output`. The key names (passcodeRequired, unlockedSinceBoot) are in
    devicectl 651.32's binary; what a real iPad writes was not seen, so this
    only ever gives advice, it never stops a run."""
    try:
        with open(path, "rb") as f:
            doc = json.load(f)
    except (OSError, ValueError):
        return "unknown"
    result = doc.get("result") if isinstance(doc, dict) else None
    for key, locked_when in (("passcodeRequired", True), ("isLocked", True), ("deviceLocked", True), ("locked", True)):
        v = _find_key(result, key)
        if isinstance(v, bool):
            return "locked" if v == locked_when else "unlocked"
    err = _find_key(doc.get("error") if isinstance(doc, dict) else None, "NSLocalizedDescription")
    if err is not _MISSING and "locked" in _text(err).lower():
        return "locked"
    return "unknown"


# ---- what the app asks for ---------------------------------------------------

# Entitlements every development build gets; anything else in a bundle's
# entitlements file is a capability its App ID must have turned on.
_BASE_KEYS = (
    "application-identifier",
    "com.apple.developer.team-identifier",
    "get-task-allow",
    "keychain-access-groups",
)

# What each capability's entitlement is called on developer.apple.com
# (Certificates, Identifiers & Profiles > Identifiers > the App ID).
CAPABILITY_NAMES = {
    "aps-environment": "Push Notifications",
    "com.apple.developer.usernotifications.time-sensitive": "Time Sensitive Notifications",
    "com.apple.developer.usernotifications.communication": "Communication Notifications",
    "com.apple.developer.usernotifications.critical-alerts": "Critical Alerts",
    "com.apple.developer.associated-domains": "Associated Domains",
    "com.apple.developer.kernel.increased-memory-limit": "Increased Memory Limit",
    "com.apple.developer.kernel.extended-virtual-addressing": "Extended Virtual Address Space",
    "com.apple.security.application-groups": "App Groups",
    "com.apple.developer.applesignin": "Sign In with Apple",
    "com.apple.developer.icloud-services": "iCloud",
    "com.apple.developer.icloud-container-identifiers": "iCloud",
    "com.apple.developer.ubiquity-kvstore-identifier": "iCloud",
    "com.apple.developer.healthkit": "HealthKit",
    "com.apple.developer.homekit": "HomeKit",
    "com.apple.developer.networking.wifi-info": "Access Wi-Fi Information",
    "com.apple.developer.nfc.readersession.formats": "NFC Tag Reading",
    "com.apple.developer.siri": "SiriKit",
    "com.apple.developer.devicecheck.appattest-environment": "App Attest",
    "com.apple.developer.weatherkit": "WeatherKit",
}

# Capabilities whose value in a development profile is not the one a
# distribution build declares: the profile's own value is signed with.
_FROM_PROFILE = ("aps-environment", "com.apple.developer.devicecheck.appattest-environment")


def capability_name(key):
    return CAPABILITY_NAMES.get(key, key)


def read_entitlements(path):
    """The entitlements a bundle declares (its .entitlements file), {} for none."""
    if not path:
        return {}
    with open(path, "rb") as f:
        doc = plistlib.load(f)
    if not isinstance(doc, dict):
        raise Refusal("%s is not a property list dictionary." % path)
    return doc


class Bundle:
    """The app, or one of its extensions: a bundle id and what it declares."""

    def __init__(self, bundle_id, entitlements_path="", is_app=False):
        self.bundle_id = bundle_id
        self.path = entitlements_path
        self.is_app = is_app
        self.declared = read_entitlements(entitlements_path)
        self.capabilities = sorted(k for k in self.declared if k not in _BASE_KEYS)

    def label(self):
        return ("the app %s" if self.is_app else "the extension %s") % self.bundle_id


def parse_bundles(bundle_id, entitlements, extensions):
    """[Bundle], the app first, from --bundle-id, --entitlements and the
    --extension BUNDLE_ID[=ENTITLEMENTS] values."""
    out = [Bundle(bundle_id, entitlements, is_app=True)]
    for ext in extensions:
        bid, _, path = ext.partition("=")
        out.append(Bundle(bid, path))
    return out


# ---- profiles ----------------------------------------------------------------

# Why a profile cannot be used; the self-test names these.
OK = "ok"
UNREADABLE = "unreadable"
PLATFORM = "platform"
EXPIRED = "expired"
NOT_DEVELOPMENT = "not-development"
TEAM = "team"
APP_ID = "app-id"
DEVICE = "device"
CAPABILITY = "capability"
IDENTITY = "identity"

_REASON_TEXT = {
    UNREADABLE: "could not be read",
    PLATFORM: "is not for iOS",
    EXPIRED: "has expired",
    NOT_DEVELOPMENT: "is a distribution profile, not a development one",
    TEAM: "belongs to another team",
    APP_ID: "is for another app",
    DEVICE: "does not list every device",
    CAPABILITY: "lacks a capability the app needs",
    IDENTITY: "needs a signing certificate that is not in this Mac's keychain",
}


def read_identities(path):
    """SHA-1 -> name, in keychain order, from `security find-identity -v -p codesigning`."""
    out = {}
    with open(path, "r", errors="replace") as f:
        for line in f:
            m = re.match(r'\s*\d+\)\s+([0-9A-Fa-f]{40})\s+"(.*)"', line)
            if m:
                out.setdefault(m.group(1).upper(), m.group(2))
    return out


def _as_utc(when):
    if when.tzinfo is None:
        return when.replace(tzinfo=datetime.timezone.utc)
    return when.astimezone(datetime.timezone.utc)


class Profile:
    def __init__(self, path):
        self.path = path
        self.error = None
        try:
            with open(path, "rb") as f:
                self.raw = plistlib.load(f)
            if not isinstance(self.raw, dict):
                raise ValueError("not a dictionary")
        except Exception as e:  # a broken file is one more profile to skip
            self.raw = {}
            self.error = str(e)
        ent = self.raw.get("Entitlements") or {}
        self.entitlements = ent if isinstance(ent, dict) else {}
        self.name = self.raw.get("Name") or os.path.basename(path)
        self.uuid = self.raw.get("UUID") or ""
        self.platforms = self.raw.get("Platform") or []
        self.expires = self.raw.get("ExpirationDate")
        self.app_id = self.entitlements.get("application-identifier") or ""
        self.get_task_allow = self.entitlements.get("get-task-allow") is True
        self.devices = self.raw.get("ProvisionedDevices") or []
        teams = self.raw.get("TeamIdentifier") or []
        self.team = (teams[0] if teams else "") or self.entitlements.get("com.apple.developer.team-identifier") or self.app_id.split(".")[0]
        self.cert_hashes = [
            hashlib.sha1(bytes(c)).hexdigest().upper() for c in (self.raw.get("DeveloperCertificates") or [])
        ]

    @property
    def exact(self):
        return not self.app_id.endswith("*")

    def identity_in(self, identities):
        for sha in identities:  # keychain order
            if sha in self.cert_hashes:
                return sha
        return None

    def missing_devices(self, udids):
        have = [d.lower() for d in self.devices]
        return [u for u in udids if u.lower() not in have]

    def missing_capabilities(self, keys):
        return [k for k in keys if k not in self.entitlements]

    def usable(self, now):
        """A development profile for iOS that has not run out."""
        if self.error or "iOS" not in self.platforms or not self.get_task_allow:
            return False
        return isinstance(self.expires, datetime.datetime) and _as_utc(self.expires) > now

    def covers(self, bundle_id):
        return self.app_id in (self.team + "." + bundle_id, self.team + ".*")

    def check(self, bundle_id, udids, identities, now, team=None, capabilities=()):
        """OK, or the first reason this profile cannot sign bundle_id for
        every one of udids with these capabilities."""
        if self.error:
            return UNREADABLE
        if "iOS" not in self.platforms:
            return PLATFORM
        if not isinstance(self.expires, datetime.datetime) or _as_utc(self.expires) <= now:
            return EXPIRED
        if not self.get_task_allow:
            return NOT_DEVELOPMENT
        if team and self.team != team:
            return TEAM
        if not self.covers(bundle_id):
            return APP_ID
        if self.missing_devices(udids):
            return DEVICE
        if self.missing_capabilities(capabilities):
            return CAPABILITY
        if not self.identity_in(identities):
            return IDENTITY
        return OK

    def why(self, reason, udids=(), names=None, capabilities=()):
        """The reason in words, naming the devices or capabilities."""
        if reason == DEVICE:
            names = names or {}
            miss = self.missing_devices(udids)
            return "does not list %s" % ", ".join('"%s"' % names.get(u, u) for u in miss)
        if reason == CAPABILITY:
            return "lacks %s" % ", ".join(capability_name(k) for k in self.missing_capabilities(capabilities))
        return _REASON_TEXT[reason]


def load_profiles(dirs):
    out = []
    for d in dirs:
        if not os.path.isdir(d):
            continue
        for name in sorted(os.listdir(d)):
            if name.endswith(".plist"):
                out.append(Profile(os.path.join(d, name)))
    return out


def _rank(p):
    # A profile for exactly this bundle before a team wildcard, then the one
    # that lasts longest.
    return (0 if p.exact else 1, -_as_utc(p.expires).timestamp(), p.path)


def pick_profile(profiles, bundle, udids, identities, now, team=None):
    """Returns (the best profile or None, [(profile, reason)] passed over)."""
    good, bad = [], []
    for p in profiles:
        why = p.check(bundle.bundle_id, udids, identities, now, team, bundle.capabilities)
        if why == OK:
            good.append(p)
        else:
            bad.append((p, why))
    good.sort(key=_rank)
    return (good[0] if good else None), bad


class Pick:
    """A profile for every bundle, one team, one signing identity."""

    def __init__(self, profiles, sha):
        self.profiles = profiles  # bundle id -> Profile
        self.sha = sha

    @property
    def team(self):
        return list(self.profiles.values())[0].team

    @property
    def expires(self):
        return min(_as_utc(p.expires) for p in self.profiles.values())


def pick_set(profiles, bundles, udids, identities, now, team=None):
    """Returns (Pick or None, {bundle id: [(profile, reason)]} passed over).

    The app's profiles are tried best first; for each, every extension needs
    a usable profile of the same team signed by the same certificate."""
    goods, passed = {}, {}
    for b in bundles:
        good, bad = [], []
        for p in profiles:
            why = p.check(b.bundle_id, udids, identities, now, team, b.capabilities)
            (good if why == OK else bad).append(p if why == OK else (p, why))
        good.sort(key=_rank)
        goods[b.bundle_id] = good
        passed[b.bundle_id] = bad
    app = bundles[0]
    for p in goods[app.bundle_id]:
        for sha in identities:  # keychain order
            if sha not in p.cert_hashes:
                continue
            chosen = {app.bundle_id: p}
            for b in bundles[1:]:
                fits = [q for q in goods[b.bundle_id] if q.team == p.team and sha in q.cert_hashes]
                if not fits:
                    break
                chosen[b.bundle_id] = fits[0]
            else:
                return Pick(chosen, sha), passed
    return None, passed


def devices_to_register(profiles, bundles, udids, now, team=None):
    """The devices no usable development profile of the team lists for the
    app yet: Xcode is asked to register each (and so make a profile that
    lists them all). When every device is listed but something else is
    missing (a capability, an extension's profile), the first device, to
    have Xcode make the profiles again."""
    app = bundles[0]
    listed = set()
    for p in profiles:
        if p.usable(now) and (not team or p.team == team) and p.covers(app.bundle_id):
            listed.update(d.lower() for d in p.devices)
    missing = [u for u in udids if u.lower() not in listed]
    return missing or list(udids[:1])


def capability_gaps(profiles, bundles, udids, identities, now, team=None):
    """{bundle id: [capability keys]} for bundles whose otherwise good
    profiles all lack capabilities: the App ID does not have them on."""
    gaps = {}
    for b in bundles:
        if not b.capabilities:
            continue
        near = [p for p in profiles if p.check(b.bundle_id, udids, identities, now, team, ()) == OK and p.exact]
        if near and not [p for p in near if not p.missing_capabilities(b.capabilities)]:
            near.sort(key=_rank)
            gaps[b.bundle_id] = near[0].missing_capabilities(b.capabilities)
    return gaps


def capability_steps(gaps, team):
    """What the owner clicks on developer.apple.com when Xcode did not turn
    a capability on by itself."""
    out = []
    for bid, keys in sorted(gaps.items()):
        out.append("The App ID %s does not have %s turned on, so no profile carries %s." % (
            bid, " and ".join(capability_name(k) for k in keys), "them" if len(keys) > 1 else "it"))
    out += [
        "Xcode normally turns them on itself when it makes the profile; a real run asks it to.",
        "If it did not, by hand, once:",
        "  1. Open https://developer.apple.com/account/resources/identifiers/list and sign in (team %s)." % (team or "the team"),
        "  2. Click the App ID (%s)." % ", ".join(sorted(gaps)),
        "     Not listed? Run the install once: Xcode registers it. Then come back here.",
        "  3. Under Capabilities, tick %s. Save, then Confirm." % "; ".join(
            sorted(set(capability_name(k) for keys in gaps.values() for k in keys))),
        "  4. Run the install again. It asks Xcode for a fresh profile, which then has them.",
    ]
    return out


def candidate_teams(profiles, identities, now, default_team=None):
    """Teams this Mac can sign development builds for, judged by its profiles:
    unexpired development profiles whose certificate is in the keychain,
    whatever app or device they are for. The repo's own team first."""
    teams = []
    for p in profiles:
        if p.error or not p.get_task_allow or not p.team:
            continue
        if not isinstance(p.expires, datetime.datetime) or _as_utc(p.expires) <= now:
            continue
        if p.identity_in(identities) and p.team not in teams:
            teams.append(p.team)
    teams.sort(key=lambda t: (0 if t == default_team else 1, t))
    return teams


def xcode_teams(path, default_team=None):
    """Team ids in an export of Xcode's preferences (`defaults export
    com.apple.dt.Xcode -`): any dictionary with a teamID. Paid teams before
    free personal ones, the repo's own team first."""
    try:
        with open(path, "rb") as f:
            doc = plistlib.load(f)
    except Exception:
        return []
    found = []

    def walk(node):
        if isinstance(node, dict):
            team = node.get("teamID")
            if isinstance(team, str) and re.match(r"^[A-Z0-9]{10}$", team):
                free = node.get("isFreeProvisioningTeam") is True
                if team not in [t for t, _ in found]:
                    found.append((team, free))
            for v in node.values():
                walk(v)
        elif isinstance(node, list):
            for v in node:
                walk(v)

    walk(doc)
    found.sort(key=lambda tf: (0 if tf[0] == default_team else 1, 1 if tf[1] else 0))
    return [t for t, _ in found]


def _substitute(value, team):
    """$(AppIdentifierPrefix) and $(TeamIdentifierPrefix), as Xcode fills them."""
    if isinstance(value, str):
        return value.replace("$(AppIdentifierPrefix)", team + ".").replace("$(TeamIdentifierPrefix)", team + ".")
    if isinstance(value, list):
        return [_substitute(v, team) for v in value]
    if isinstance(value, dict):
        return dict((k, _substitute(v, team)) for k, v in value.items())
    return value


def entitlements_for(profile, bundle):
    """What a bundle is signed with: the four entitlements every development
    build has, then what its entitlements file declares, with the profile's
    own value for the keys a development profile sets differently (a
    development profile's aps-environment is "development", whatever a
    distribution build declares). Nothing else the profile might allow."""
    team = profile.team
    app_id = team + "." + bundle.bundle_id
    ents = {
        "application-identifier": app_id,
        "com.apple.developer.team-identifier": team,
        "get-task-allow": True,
        "keychain-access-groups": [app_id],
    }
    for key, value in bundle.declared.items():
        if key in ("application-identifier", "com.apple.developer.team-identifier", "get-task-allow"):
            continue
        if key in _FROM_PROFILE and key in profile.entitlements:
            value = profile.entitlements[key]
            if isinstance(value, list):  # a profile may list several values
                value = "development" if "development" in value else value[0]
        ents[key] = _substitute(value, team)
    return ents


def dry_entitlements(bundle, team):
    """The entitlements a run would sign with, before there is a profile."""
    fake = Profile.__new__(Profile)
    fake.team = team
    fake.entitlements = {"aps-environment": "development", "com.apple.developer.devicecheck.appattest-environment": "development"}
    return entitlements_for(fake, bundle)


# ---- the throwaway Xcode project that makes Xcode mint the profiles -------------

# One empty app target per bundle (the app and each extension: an App ID is
# an App ID, so an extension's profile can be minted by an app target with
# its bundle id), with automatic signing and the bundle's capabilities in an
# entitlements file. Building it for a device with -allowProvisioningUpdates
# -allowProvisioningDeviceRegistration is what makes Xcode register the
# device, register each App ID, turn on the capabilities its entitlements
# file asks for, and create a development profile for each.


def _oid(target, n):
    """A 24-digit object id: target index, then a slot."""
    return "5C%02d%020d" % (target, n)


def _pbxproj(bundles, team, min_os):
    objects = []
    targets, products, children = [], [], []
    main = _oid(99, 1)
    children.append("%s /* main.swift */" % main)
    objects.append('%s /* main.swift */ = {isa = PBXFileReference; lastKnownFileType = sourcecode.swift; path = main.swift; sourceTree = "<group>"; };' % main)
    for i, b in enumerate(bundles):
        name = "Mint%d" % i
        build_file, product, sources, target, conf_list, conf, ents = (_oid(i, n) for n in range(1, 8))
        objects += [
            "%s /* main.swift in Sources */ = {isa = PBXBuildFile; fileRef = %s /* main.swift */; };" % (build_file, main),
            "%s /* %s.app */ = {isa = PBXFileReference; explicitFileType = wrapper.application; includeInIndex = 0; path = %s.app; sourceTree = BUILT_PRODUCTS_DIR; };" % (product, name, name),
            '%s /* %s.entitlements */ = {isa = PBXFileReference; lastKnownFileType = text.plist.entitlements; path = %s.entitlements; sourceTree = "<group>"; };' % (ents, name, name),
            "%s /* Sources */ = {isa = PBXSourcesBuildPhase; buildActionMask = 2147483647; files = (%s /* main.swift in Sources */, ); runOnlyForDeploymentPostprocessing = 0; };" % (sources, build_file),
            "%s /* %s */ = {isa = PBXNativeTarget; buildConfigurationList = %s; buildPhases = (%s /* Sources */, ); buildRules = (); dependencies = (); name = %s; productName = %s; productReference = %s /* %s.app */; productType = \"com.apple.product-type.application\"; };"
            % (target, name, conf_list, sources, name, name, product, name),
            "%s = {isa = XCConfigurationList; buildConfigurations = (%s /* Release */, ); defaultConfigurationIsVisible = 0; defaultConfigurationName = Release; };" % (conf_list, conf),
            "%s /* Release */ = {isa = XCBuildConfiguration; buildSettings = {"
            "CODE_SIGN_ENTITLEMENTS = %s.entitlements; CODE_SIGN_STYLE = Automatic; CURRENT_PROJECT_VERSION = 1; "
            'DEVELOPMENT_TEAM = "%s"; GENERATE_INFOPLIST_FILE = YES; INFOPLIST_KEY_UILaunchScreen_Generation = YES; '
            'MARKETING_VERSION = 1.0; PRODUCT_BUNDLE_IDENTIFIER = "%s"; PRODUCT_NAME = "$(TARGET_NAME)"; '
            'SWIFT_VERSION = 5.0; TARGETED_DEVICE_FAMILY = "1,2"; }; name = Release; };' % (conf, name, team, b.bundle_id),
        ]
        targets.append("%s /* %s */" % (target, name))
        products.append("%s /* %s.app */" % (product, name))
        children.append("%s /* %s.entitlements */" % (ents, name))
    group, project, plist, pconf = _oid(98, 1), _oid(98, 2), _oid(98, 3), _oid(98, 4)
    objects += [
        '%s = {isa = PBXGroup; children = (%s, ); sourceTree = "<group>"; };' % (group, ", ".join(children + products)),
        "%s = {isa = XCConfigurationList; buildConfigurations = (%s /* Release */, ); defaultConfigurationIsVisible = 0; defaultConfigurationName = Release; };" % (plist, pconf),
        '%s /* Release */ = {isa = XCBuildConfiguration; buildSettings = {IPHONEOS_DEPLOYMENT_TARGET = "%s"; SDKROOT = iphoneos; }; name = Release; };' % (pconf, min_os),
        '%s /* Project object */ = {isa = PBXProject; attributes = {BuildIndependentTargetsInParallel = 1; }; buildConfigurationList = %s; compatibilityVersion = "Xcode 14.0"; developmentRegion = en; hasScannedForEncodings = 0; knownRegions = (en, Base, ); mainGroup = %s; productRefGroup = %s; projectDirPath = ""; projectRoot = ""; targets = (%s, ); };'
        % (project, plist, group, group, ", ".join(targets)),
    ]
    return "// !$*UTF8*$!\n{\n\tarchiveVersion = 1;\n\tclasses = {\n\t};\n\tobjectVersion = 56;\n\tobjects = {\n%s\n\t};\n\trootObject = %s /* Project object */;\n}\n" % (
        "\n".join("\t\t" + o for o in objects), project)


def _xcscheme(bundles):
    entries = []
    for i in range(len(bundles)):
        entries.append(
            '         <BuildActionEntry buildForTesting = "YES" buildForRunning = "YES" buildForProfiling = "YES" buildForArchiving = "YES" buildForAnalyzing = "YES">\n'
            '            <BuildableReference BuildableIdentifier = "primary" BlueprintIdentifier = "%s" BuildableName = "Mint%d.app" BlueprintName = "Mint%d" ReferencedContainer = "container:ProfileMint.xcodeproj">\n'
            "            </BuildableReference>\n"
            "         </BuildActionEntry>" % (_oid(i, 4), i, i)
        )
    return (
        '<?xml version="1.0" encoding="UTF-8"?>\n<Scheme version = "1.7">\n'
        '   <BuildAction parallelizeBuildables = "YES" buildImplicitDependencies = "YES">\n      <BuildActionEntries>\n'
        + "\n".join(entries)
        + '\n      </BuildActionEntries>\n   </BuildAction>\n   <LaunchAction buildConfiguration = "Release">\n   </LaunchAction>\n'
        '   <ArchiveAction buildConfiguration = "Release">\n   </ArchiveAction>\n</Scheme>\n'
    )


def mint_entitlements(bundle):
    """A bundle's capabilities as the throwaway target declares them: the
    development value for push, the declared value otherwise."""
    out = {}
    for key in bundle.capabilities:
        value = bundle.declared[key]
        if key in _FROM_PROFILE:
            value = "development"
        out[key] = value
    return out


def write_mint_project(out, bundles, team, min_os):
    """The throwaway project: an empty app target per bundle, this team,
    automatic signing. Returns the .xcodeproj's path."""
    for value in [b.bundle_id for b in bundles] + [team, min_os]:
        if not re.match(r"^[A-Za-z0-9.\-]+$", value):
            raise Refusal("Cannot write an Xcode project with the value %r." % value)
    proj = os.path.join(out, "ProfileMint.xcodeproj")
    schemes = os.path.join(proj, "xcshareddata", "xcschemes")
    os.makedirs(schemes)
    with open(os.path.join(proj, "project.pbxproj"), "w") as f:
        f.write(_pbxproj(bundles, team, min_os))
    with open(os.path.join(schemes, "ProfileMint.xcscheme"), "w") as f:
        f.write(_xcscheme(bundles))
    with open(os.path.join(out, "main.swift"), "w") as f:
        f.write("// Only here so Xcode has something to build and sign.\nprint(\"profile mint\")\n")
    for i, b in enumerate(bundles):
        with open(os.path.join(out, "Mint%d.entitlements" % i), "wb") as f:
            plistlib.dump(mint_entitlements(b), f)
    return proj


# ---- what Xcode's settings say about Apple IDs and teams -----------------------


def xcode_account_info(path):
    """What an export of Xcode's preferences shows of the sign-in:
    {"readable", "accounts" (how many Apple IDs), "teams" [(id, name, free)]}.

    The key names are in Xcode 27.2's binaries (DVTDeveloperAccountManagerAppleIDLists
    in DVTFoundation; IDEProvisioningTeamByIdentifier and IDEProvisioningTeams
    in IDEFoundation). The shape of their values is from older Xcode versions
    and was not seen on a signed-in Xcode 27, so this is read loosely and only
    ever gives advice."""
    info = {"readable": False, "accounts": 0, "teams": []}
    try:
        with open(path, "rb") as f:
            doc = plistlib.load(f)
    except Exception:
        return info
    if not isinstance(doc, dict):
        return info
    info["readable"] = True
    names = set()

    def count(node):
        if isinstance(node, dict):
            user = node.get("username")
            if isinstance(user, str) and user:
                names.add(user.lower())
            for v in node.values():
                count(v)
        elif isinstance(node, list):
            for v in node:
                count(v)

    count(doc.get("DVTDeveloperAccountManagerAppleIDLists"))
    seen = {}

    def walk(node):
        if isinstance(node, dict):
            team = node.get("teamID")
            if isinstance(team, str) and re.match(r"^[A-Z0-9]{10}$", team) and team not in seen:
                seen[team] = (node.get("teamName") if isinstance(node.get("teamName"), str) else "", node.get("isFreeProvisioningTeam") is True)
            for v in node.values():
                walk(v)
        elif isinstance(node, list):
            for v in node:
                walk(v)

    walk(doc)
    info["accounts"] = len(names)
    info["teams"] = [(t, seen[t][0], seen[t][1]) for t in seen]
    return info


# ---- what a tool's failure most likely means ------------------------------------

# Each cause: a name, substrings to look for (lower case) in the tool's output,
# and what to do. The first cause listed that matches is printed first.
#
# Where the substrings come from. "27.2": the sentence is in Xcode 27.2's own
# files (devicectl, CoreDevice, IDEFoundation, DVTPortal, iPhoneRemoteDevice
# .strings), read on the build farm's Mac on 2026-11-14. "older": remembered
# from earlier Xcode versions, not confirmed on 27. None was seen coming out
# of a real failure, so the raw output is always printed too.
_CAUSES = [
    (
        "keychain-locked",
        ["errsecinternalcomponent", "user interaction is not allowed", "errsecinteractionnotallowed"],  # older
        [
            "The Mac's keychain would not hand over the signing key (it is locked, or this is an ssh session).",
            "Fix: run this in the Terminal app on the Mac itself, logged in. If it still fails:",
            "       security unlock-keychain ~/Library/Keychains/login.keychain-db",
            "     then run this again. If a window asks to use the key, choose Always Allow.",
        ],
    ),
    (
        "developer-mode",
        [
            "developer mode is turned off",  # 27.2
            "turn on developer mode",  # 27.2
            "developer mode is no longer enabled",  # 27.2
            "developer mode disabled",  # older (xcodebuild)
            "developer mode is not enabled",  # older
            "developermodedisabled",  # 27.2 (the error's name)
        ],
        [
            "Developer Mode is off on the device.",
            "Fix: on the device, Settings > Privacy & Security > Developer Mode, switch it on, tap Restart.",
            "     After the restart, unlock the device, tap Turn On, type the passcode. Then run this again.",
            "     (The switch only shows once the device has been plugged in to a Mac with Xcode.)",
        ],
    ),
    (
        "locked",
        [
            "device is locked",  # 27.2
            "device was still locked",  # 27.2
            "is currently locked",  # 27.2
            "is locked, waiting for unlock",  # 27.2
            "unlock the device",  # 27.2
            "could not be, unlocked",  # older (launch)
            "for reason: locked",  # older (launch)
            "unlock ipad to continue",  # older (xcodebuild)
            "passwordprotected",  # 27.2 (log)
        ],
        [
            "The device is locked.",
            "Fix: unlock it, leave it on the Home Screen with the screen on, and run this again.",
        ],
    ),
    (
        "not-paired",
        [
            "not paired",  # older
            "unpaired",  # 27.2 (the state's name)
            "pairing failed",  # 27.2
            "pairingfailed",  # 27.2
            "userrejectedpairingprompt",  # 27.2
            "trust this computer",  # older
            "must be paired",  # older
            "pairing of this device is already in progress",  # 27.2
        ],
        [
            "The device does not trust this Mac yet (it is not paired).",
            "Fix: plug it in with a cable, unlock it, tap Trust on the device and type its passcode.",
            "     If nothing asks:  xcrun devicectl manage pair --device {udid}",
            "     If you tapped Don't Trust: unplug, plug in again, and it asks again.",
        ],
    ),
    (
        "ios-too-old",
        [
            "minimum os version",  # 27.2
            "too high to be installed",  # 27.2
            "deviceosversiontoolow",  # older
            "requires a newer version of",  # older
            "deployment target",  # older (xcodebuild)
        ],
        [
            "The device's iOS is older than {app} needs ({min_os}).",
            "Fix: on the device, Settings > General > Software Update. Then run this again.",
        ],
    ),
    (
        "wrong-device-kind",
        ["device family", "devicefamily"],  # older
        [
            "That device is not one {app} is built for (its families in BUILD.bazel).",
            "Fix: plug in the right one, or name it:  ... -- --device NAME_OR_UDID",
        ],
    ),
    (
        "no-apple-id",
        [
            "no account for team",  # older; the best known of these
            "add a new account in accounts settings",  # older
            "unable to log in with account",  # older
            "session has expired",  # older
            "dcsessionexpired",  # 27.2
            "please log in",  # older
        ],
        [
            "Xcode has no working sign-in for team {team}.",
            "Fix: open Xcode > Settings > Accounts. Add the Apple ID that belongs to team {team},",
            "     or sign in again if it shows a warning. Then run this again.",
            "     If the Mac is on another team:  ... -- --team TEAMID",
        ],
    ),
    (
        "no-team",
        ["requires a development team", "is not a member", "unknown team"],  # older
        [
            "Xcode does not know team {team}, or the Apple ID is not on it.",
            "Fix: Xcode > Settings > Accounts shows the teams of each Apple ID. Pass the right one:",
            "       {install_cmd} -- --team TEAMID",
        ],
    ),
    (
        "agreement",
        ["agreement", "pla update"],  # older
        [
            "Apple wants a new licence agreement accepted before it does anything for the team.",
            "Fix: the team's Account Holder signs in at https://developer.apple.com/account and accepts it.",
        ],
    ),
    (
        "max-devices",
        ["maximum number of"],  # older
        [
            "The team has used up its device slots for the year (Apple allows 100 of each kind a year).",
            "Fix: nothing this command can do. Slots come back when the membership renews",
            "     (https://developer.apple.com/account/resources/devices), or use another team: -- --team TEAMID",
        ],
    ),
    (
        "cannot-register-device",
        [
            "permission to register devices",  # 27.2
            "team can't register devices",  # 27.2
            "do not have permission to register",  # 27.2
        ],
        [
            "This Apple ID is not allowed to add devices to team {team}.",
            "Fix: an Admin of the team adds the device by hand at",
            "     https://developer.apple.com/account/resources/devices  (UDID {udid}),",
            "     or gives the Apple ID the Admin or App Manager role. Then run this again.",
        ],
    ),
    (
        "device-not-registered",
        [
            "is not registered",  # 27.2
            "not registered to your team",  # 27.2
            "not registered on the developer website",  # 27.2
            "unregistered devices",  # 27.2
            "doesn't include the currently selected device",  # older
            "team has no devices",  # older
            'does not list "',  # this installer's own words
        ],
        [
            "The device is not on the team's device list, so no profile covers it.",
            "Fix: run this again with the device plugged in and unlocked: Xcode registers it.",
            "     If it keeps failing, add it by hand at https://developer.apple.com/account/resources/devices",
            "     (UDID {udid}), then run this again.",
        ],
    ),
    (
        "bundle-id-taken",
        [
            "cannot be registered to your development team because it is not available",  # 27.2
            "automatic signing cannot register bundle identifier",  # 27.2
            "automatic signing cannot register bundle identifiers",  # 27.2
        ],
        [
            "Apple would not give the app id {bundle_id} to team {team}",
            "(another team may own it, or this Apple ID may not create app ids).",
            "Fix: an Admin registers {bundle_id} at https://developer.apple.com/account/resources/identifiers,",
            "     or use the team that owns it: -- --team TEAMID",
        ],
    ),
    (
        "certificate",
        [
            "no signing certificate",  # older
            "not installed in your keychain or is missing its private key",  # 27.2
            "missing private key",  # 27.2
            "missing its private key",  # 27.2
            "already has an apple development signing certificate for this machine",  # older
            "cssmerr_tp_cert_revoked",  # 27.2
            "certificate has been revoked",  # 27.2
            "cssmerr_tp_cert_expired",  # older
            "certificate has expired",  # older
            "no identity found",  # older (codesign)
            "unable to build chain to self-signed root",  # older (codesign)
        ],
        [
            "The signing certificate is missing, has run out, was revoked, or its private key is not on this Mac.",
            "Fix: Xcode > Settings > Accounts > the team > Manage Certificates. Press + and choose",
            "     Apple Development. (A certificate made on another Mac has its key on that Mac only:",
            "     make a new one here.) Then run this again.",
        ],
    ),
    (
        "capability",
        [
            "doesn't support the",  # older: "...profile doesn't support the Push Notifications capability"
            "doesn't include the aps-environment entitlement",  # older
            "doesn't include the com.apple.developer",  # older
            "entitlement is missing from the provisioning profile",  # older
        ],
        [
            "The App ID {bundle_id} lacks a capability the app's entitlements ask for, so Xcode's profile cannot carry it.",
            "Fix: developer.apple.com/account/resources/identifiers, click the App ID, tick the capability",
            "     named above under Capabilities, Save, Confirm. Then run this again. (The doctor names them.)",
        ],
    ),
    (
        "profile-refused",
        [
            "automatic signing is disabled",  # 27.2
            "unable to generate a profile",  # 27.2
            "no profiles for",  # older
            "couldn't find any ios app development provisioning profiles",  # older
            "failed to create provisioning profile",  # older
            "communication with apple failed",  # 27.2
        ],
        [
            "Xcode could not make a development profile for {bundle_id} with team {team}.",
            "Fix: check the Mac is online and Xcode > Settings > Accounts shows team {team} with no warning.",
            "     Then run this again. The lines above say what Apple answered.",
        ],
    ),
    (
        "ambiguous-identity",
        ["ambiguous (matches"],  # older (codesign)
        [
            "The keychain has two certificates with the same name.",
            "Fix: open Keychain Access, My Certificates, and delete the older Apple Development one.",
        ],
    ),
    (
        "detritus",
        ["detritus not allowed"],  # older (codesign)
        [
            "codesign found Finder leftovers in the app. Run this again; if it repeats, say so (a bug here).",
        ],
    ),
    (
        "app-id-mismatch",
        ["application-identifier entitlement", "mismatchedapplicationidentifierentitlement"],  # older
        [
            "A {app} signed by another team is already on the device, and iOS will not swap teams.",
            "Fix: use the same team as before (-- --team TEAMID). Deleting the app from the device",
            "     also works, but it deletes the app's data with it.",
        ],
    ),
    (
        "entitlements-refused",
        [
            "signed with invalid entitlements",  # older
            "0xe8008016",  # older: invalid entitlements
            "entitlements that are not permitted",  # older
        ],
        [
            "The device would not accept the entitlements the app was signed with: the profile does not allow one of them.",
            "Fix: run the doctor (-- --dry-run). It names the capability the App ID lacks and where to turn it on.",
        ],
    ),
    (
        "signature-refused",
        [
            "0xe8008015",  # older: no valid profile for this executable
            "0xe8008012",  # older
            "0xe8008014",  # older: invalid signature
            "0xe8008016",  # older: invalid entitlements
            "0xe8008018",  # older: identity no longer valid
            "0xe800801c",  # older: no code signature
            "a valid provisioning profile for this executable was not found",  # older
            "applicationverificationfailed",  # older
            "provisioning profile is incompatible with the device",  # 27.2
            "provisioning profile has expired",  # 27.2
            "profile signature verification failed",  # 27.2
            "no code signature found",  # older
            "invalid signature",  # older
        ],
        [
            "The device would not accept the app's signature or its profile.",
            "Fix: run this again (a fresh profile is made if the old one ran out or lacks this device).",
            "     If it repeats: -- --keep, and read install.log in the folder it names.",
        ],
    ),
    (
        "not-trusted-yet",
        [
            "has not been explicitly trusted",  # older
            "invalid code signature, inadequate entitlements",  # older
            "untrusted developer",  # older
            "unable to verify",  # older
            "integrity could not be verified",  # older
        ],
        [
            "iOS has not checked the developer with Apple yet. The first start needs the internet.",
            "Fix: put the device on Wi-Fi and tap {app}'s icon. If it says the developer is not trusted:",
            "     Settings > General > VPN & Device Management, tap the developer, Trust.",
        ],
    ),
    (
        "support-files",
        [
            "developer disk image",  # 27.2
            "core device content installed on your mac needs to be updated",  # 27.2
        ],
        [
            "Xcode could not put its support files on the device (it does this the first time).",
            "Fix: keep the Mac online, the device unlocked and on a cable, and run this again.",
            "     If it repeats:  xcrun devicectl manage ddis update",
        ],
    ),
    (
        "busy",
        [
            "device is busy",  # 27.2
            "is busy:",  # older
            "preparing device",  # 27.2
            "preparing ipad",  # older
            "ready for development",  # older
            "waiting to reconnect",  # older
        ],
        [
            "Xcode is still getting the device ready (the first time takes a few minutes).",
            "Fix: keep it unlocked and plugged in, wait two minutes, and run this again.",
            "     Xcode > Window > Devices and Simulators shows its progress.",
        ],
    ),
    (
        "platform-missing",
        ["download and install the platform", "platform is not installed"],  # older
        [
            "Xcode does not have the iOS platform installed.",
            "Fix:  xcodebuild -downloadPlatform iOS   (or Xcode > Settings > Components).",
        ],
    ),
    (
        "xcode-setup",
        ["xcodebuild -license", "agree to the xcode", "-runfirstlaunch"],  # older
        [
            "Xcode has not been set up on this Mac.",
            "Fix:  sudo xcodebuild -license accept   then   sudo xcodebuild -runFirstLaunch",
        ],
    ),
    (
        "mac-offline",
        [
            "requires an active network connection",  # 27.2
            "connected to the internet",  # 27.2
            "internet connection appears to be offline",  # older
            "activation servers",  # 27.2
        ],
        [
            "The Mac needs the internet for this step and could not reach Apple.",
            "Fix: check the Mac's Wi-Fi and run this again.",
        ],
    ),
    (
        "device-not-found",
        [
            "the specified device was not found",  # 27.2 (seen: CoreDeviceError 1000)
            "unable to find a destination matching",  # older (xcodebuild)
        ],
        [
            "The Mac lost sight of the device.",
            "Fix: plug it in with a cable, unlock it, and run this again.",
            "     xcrun devicectl list devices   shows what the Mac can see.",
        ],
    ),
    (
        "connection-lost",
        [
            "device became unavailable",  # 27.2
            "not connected",  # 27.2
            "device tunnel",  # 27.2
            "timed out",  # 27.2
            "no longer connected",  # older
            "connection was interrupted",  # older
            "connection was invalidated",  # older
        ],
        [
            "The link to the device dropped. Over Wi-Fi this happens; a cable is the surest.",
            "Fix: plug the device in, keep it unlocked, and run this again.",
        ],
    ),
]


def explain(text, **values):
    """[(cause name, [lines])] for every known cause found in a tool's output,
    the likeliest first. Empty when nothing is recognised."""
    low = text.lower().replace("\u2019", "'")
    defaults = {"udid": "<the device's UDID>", "team": "<the team>", "bundle_id": "the app", "min_os": "26.0",
                "app": "The app", "install_cmd": "bazel run :install"}
    defaults.update((k, v) for k, v in values.items() if v)
    out = []
    for name, needles, lines in _CAUSES:
        if any(n in low for n in needles):
            out.append((name, [line.format(**defaults) for line in lines]))
    return out


# ---- the preflight ("doctor") -----------------------------------------------------


class Report:
    """Check lines with a tick, a cross or a question mark, and the fix under
    each cross. A cross blocks; a question mark is something worth a look."""

    def __init__(self, ascii_only=False):
        self.marks = ("ok", "NO", "??") if ascii_only else ("✓", "✗", "?")
        self.lines = []
        self.blocked = []
        self.skipped = []

    def section(self, title):
        self.lines.append("")
        self.lines.append("== " + title)

    def _add(self, mark, text, fix):
        self.lines.append("  %s %s" % (mark, text))
        for line in fix:
            self.lines.append("      " + line)

    def ok(self, text, *more):
        self._add(self.marks[0], text, more)

    def bad(self, text, *fix):
        self.blocked.append(text)
        self._add(self.marks[1], text, fix)

    def unsure(self, text, *fix):
        self._add(self.marks[2], text, fix)

    def note(self, text):
        self.lines.append("      " + text)

    def skip(self, text, *fix):
        """A cross for one device that is left out while others go on."""
        self.skipped.append(text)
        self._add(self.marks[1], text, fix)


def read_facts(path):
    """key=value lines install.sh writes about the Mac."""
    facts = {}
    if not path:
        return facts
    try:
        with open(path, "r", errors="replace") as f:
            for line in f:
                key, sep, value = line.rstrip("\n").partition("=")
                if sep:
                    facts[key.strip()] = value.strip()
    except OSError:
        pass
    return facts


def read_all_identities(path):
    """[(name, problem)] for identities `security find-identity -p codesigning`
    lists as not valid, such as (CSSMERR_TP_CERT_EXPIRED)."""
    out = []
    if not path:
        return out
    try:
        with open(path, "r", errors="replace") as f:
            for line in f:
                m = re.match(r'\s*\d+\)\s+[0-9A-Fa-f]{40}\s+"(.*)"\s+\((CSSMERR_[A-Z_]+|[A-Za-z ]*expired[A-Za-z ]*|[A-Za-z ]*revoked[A-Za-z ]*)\)', line)
                if m and (m.group(1), m.group(2)) not in out:
                    out.append((m.group(1), m.group(2)))
    except OSError:
        pass
    return out


_DEV_IDENTITY = re.compile(r"^(Apple Development|iPhone Developer|iOS Development)\b")



def _mac_checks(a, rep, facts):
    rep.section("This Mac")
    if facts.get("os") and facts.get("os") != "Darwin":
        rep.unsure("This is %s, not a Mac: the Xcode checks are skipped (test inputs)." % facts.get("os"))
        return
    if not facts:
        return
    mac = facts.get("macos", "")
    if mac:
        if version_tuple(mac) and version_tuple(mac)[0] < 26:
            rep.unsure(
                "macOS %s. The repo builds its Mac tools for macOS 26 or later." % mac,
                "Fix if the build fails: update macOS (and Xcode with it).",
            )
        else:
            rep.ok("macOS %s" % mac)
    xpath = facts.get("xcode_path", "")
    xver = facts.get("xcode_version", "")
    if not xpath:
        rep.bad(
            "No Xcode is selected (xcode-select -p gave nothing).",
            "Fix: install Xcode from the App Store, open it once, then:",
            "       sudo xcode-select -s /Applications/Xcode.app",
        )
    elif "CommandLineTools" in xpath:
        rep.bad(
            "The Command Line Tools are selected, not Xcode (%s)." % xpath,
            "Fix: sudo xcode-select -s /Applications/Xcode.app",
            "     (install Xcode from the App Store first if it is not there).",
        )
    elif not xver:
        err = facts.get("xcodebuild_error", "")
        rep.bad(
            "Xcode is at %s but xcodebuild does not run%s" % (xpath, ": " + err if err else "."),
            "Fix: open Xcode once and let it finish, or:",
            "       sudo xcodebuild -license accept",
            "       sudo xcodebuild -runFirstLaunch",
        )
    else:
        build = facts.get("xcode_build", "")
        label = "Xcode %s%s, selected (%s)" % (xver, " (%s)" % build if build else "", xpath)
        major = (version_tuple(xver) or (0,))[0]
        need = (version_tuple(a.min_os) or (26,))[0]
        if major < need:
            rep.bad(
                label + ". %s needs the iOS %s SDK: Xcode %d or later." % (a.name, a.min_os, need),
                "Fix: update Xcode from the App Store, open it once, then run this again.",
            )
        elif major != 27:
            rep.unsure(label + ". This installer was checked with Xcode 27.2 only.")
        else:
            rep.ok(label)
        if facts.get("first_launch_rc", "0") != "0":
            rep.bad(
                "Xcode has not finished installing its parts (xcodebuild -checkFirstLaunchStatus says so).",
                "Fix: sudo xcodebuild -runFirstLaunch     (or open Xcode once and wait for it)",
            )
        else:
            rep.ok("Xcode's first-launch parts are installed")
        if facts.get("devicectl"):
            dv = facts.get("devicectl_version", "")
            rep.ok("devicectl%s (Xcode's tool that talks to the devices)" % (" " + dv if dv else ""))
        else:
            rep.bad(
                "Xcode's devicectl is missing (xcrun --find devicectl found nothing).",
                "Fix: open Xcode once and let it install its parts; sudo xcodebuild -runFirstLaunch",
            )
    if facts.get("via_bazel") == "1":
        wanted = facts.get("bazel_wanted", "")
        rep.ok("Bazel ran this" + (" (the repo pins %s; Bazelisk fetches it)" % wanted if wanted else ""))
    elif facts.get("bazel"):
        rep.ok("bazel is on the PATH (%s)" % facts.get("bazel"))
    else:
        rep.unsure(
            "No bazel on the PATH. Fine when the app was built elsewhere (--ipa).",
            "To build here: brew install bazelisk",
        )
    if facts.get("lockfile_dirty") == "1":
        rep.unsure(
            "Bazel changed MODULE.bazel.lock on this Mac. A later git pull may refuse because of it.",
            "Fix, before git pull:  git checkout MODULE.bazel.lock",
        )


def _signing_checks(rep, ids, a, facts):
    xinfo = xcode_account_info(a.xcode_defaults) if a.xcode_defaults else {"readable": False, "accounts": 0, "teams": []}
    xteams = [t for t, _, _ in xinfo["teams"]]
    team_words = "; ".join(
        "%s%s%s" % (t, " " + n if n else "", " (free personal team: 7-day installs)" if free else "") for t, n, free in xinfo["teams"]
    )
    if not xinfo["readable"]:
        rep.unsure(
            "Could not read Xcode's settings to see which Apple ID is signed in.",
            "Check by hand: Xcode > Settings > Accounts should list the Apple ID and its team.",
        )
    elif xinfo["accounts"] or xteams:
        who = "%d Apple ID%s" % (xinfo["accounts"], "" if xinfo["accounts"] == 1 else "s") if xinfo["accounts"] else "an Apple ID"
        rep.ok("Xcode is signed in with %s%s" % (who, ". Teams: " + team_words if team_words else ""))
    else:
        rep.unsure(
            "Xcode's settings show no Apple ID. (A best guess: Xcode 27 may keep this elsewhere.)",
            "Check: Xcode > Settings > Accounts. Add the Apple ID of the developer team if it is not there.",
        )

    valid_dev = [(sha, name) for sha, name in ids.items() if _DEV_IDENTITY.match(name)]
    broken = [(n, why) for n, why in read_all_identities(a.identities_all) if _DEV_IDENTITY.match(n)]
    if valid_dev:
        rep.ok("Development signing identity in the keychain: " + "; ".join(name for _, name in valid_dev[:4]))
    elif broken:
        rep.unsure(
            "The keychain's development certificate is no good: "
            + "; ".join("%s (%s)" % (n, why.replace("CSSMERR_TP_CERT_", "").lower()) for n, why in broken[:3]),
            "A real run asks Xcode to make a new one. If that fails:",
            "Xcode > Settings > Accounts > the team > Manage Certificates, +, Apple Development.",
        )
    else:
        certs = facts.get("dev_certs", "")
        if certs and certs != "0":
            rep.unsure(
                "The keychain has an Apple Development certificate but not its private key.",
                "The key stays on the Mac that made the certificate. A real run asks Xcode for a new one;",
                "if that fails: Xcode > Settings > Accounts > the team > Manage Certificates, +, Apple Development.",
            )
        else:
            rep.unsure(
                "No development signing identity in the keychain yet.",
                "A real run asks Xcode to make one, with the profile. It needs the Apple ID in Xcode.",
            )
    return xinfo


def _device_checks(a, device, out):
    """[(mark, text, fix lines)] for one device; mark is ok, bad or unsure."""
    def add(mark, text, *fix):
        out.append((mark, text, list(fix)))

    osn = device.os_name()
    how = device.connection()
    if device.rank == _RANK_CONNECTED:
        add("ok", "Connected %s" % (how or "(devicectl does not say how)"))
        if device.transport and device.transport != "wired":
            out.append(("note", "A cable is quicker and surer than the network for the first install.", []))
    else:
        add("unsure", "In reach%s, but not connected yet (devicectl connects when asked)." % (" " + how if how else ""),
            "Keep it unlocked and near. A cable is the surest.")
    if device.pairing == "paired":
        add("ok", "Paired with this Mac (it trusts this Mac)")
    elif device.pairing == "unpaired":
        add("bad", "Not paired with this Mac.",
            "Fix: plug it in with a cable, unlock it, tap Trust on it and type its passcode.",
            "     If nothing asks:  xcrun devicectl manage pair --device " + device.udid)
    else:
        add("unsure", 'Pairing state: "%s" (not a state this installer knows).' % device.pairing)
    if device.os_version:
        if version_tuple(device.os_version) < version_tuple(a.min_os):
            add("bad", "%s %s. %s needs %s %s or later." % (osn, device.os_version, a.name, osn, a.min_os),
                "Fix: on the device, Settings > General > Software Update.")
        else:
            add("ok", "%s %s (%s needs %s or later)" % (osn, device.os_version, a.name, a.min_os))
    else:
        add("unsure", "devicectl did not give the %s version. %s needs %s or later." % (osn, a.name, a.min_os))
    mode = device.developer_mode
    details = os.path.join(a.details_dir, device.udid + ".json") if a.details_dir else ""
    if mode == "unknown" and details and os.path.exists(details):
        try:
            with open(details, "rb") as f:
                found = _find_key(json.load(f).get("result"), "developerModeStatus")
            if found is not _MISSING:
                mode = _on_off(found)
        except (OSError, ValueError, AttributeError):
            pass
    if device.pairing == "unpaired":
        add("unsure", "Developer Mode cannot be checked until it is paired.")
    elif mode == "enabled":
        add("ok", "Developer Mode is on")
    elif mode == "disabled":
        add("bad", "Developer Mode is off.",
            "Fix: on the device, Settings > Privacy & Security > Developer Mode, switch it on, tap Restart.",
            "     After the restart, unlock it, tap Turn On and type the passcode.")
    else:
        add("unsure", "devicectl did not say whether Developer Mode is on.",
            "Check on the device: Settings > Privacy & Security > Developer Mode must be on.")
    lock_file = os.path.join(a.lock_dir, device.udid + ".json") if a.lock_dir else ""
    lock = read_lock_state(lock_file) if lock_file else "unknown"
    if lock == "locked":
        add("unsure", "It looks locked.", "Unlock it and leave it on the Home Screen while this runs.")
    elif lock == "unlocked":
        add("ok", "Unlocked")
    if device.ddi_ready is False:
        out.append(("note", "Xcode may first copy its support files to it. That takes a few minutes,", []))
        out.append(("note", "and the Mac must be online for it.", []))


def preflight(a, rep):
    """Runs every check, fills rep, and returns what the install needs (a
    dictionary install.sh reads back, one file a value)."""
    plan = {}
    facts = read_facts(a.host)
    now = _now(a.now)
    install_cmd = a.install_cmd
    bundles = parse_bundles(a.bundle_id, a.entitlements, a.extension)
    kinds = kinds_text(a.family)

    _mac_checks(a, rep, facts)

    rep.section("Signing")
    ids = read_identities(a.identities) if a.identities else {}
    profiles = load_profiles(a.dir)
    xinfo = _signing_checks(rep, ids, a, facts)
    xteams = [t for t, _, _ in xinfo["teams"]]

    # ---- the devices
    rep.section("The devices (%s)" % kinds)
    chosen = []
    if not a.devices_json or not os.path.exists(a.devices_json):
        why = facts.get("devicectl_error", "")
        rep.bad(
            "Could not ask Xcode for its devices (xcrun devicectl list devices)%s" % (": " + why if why else "."),
            "Fix: open Xcode once, plug the device in, and run this again.",
        )
    else:
        notes = []
        try:
            dl = load_device_list(a.devices_json)
            if isinstance(dl.json_version, int) and dl.json_version > JSON_VERSION_CHECKED:
                rep.unsure(
                    "devicectl writes JSON version %s; this installer was checked with version %d."
                    % (dl.json_version, JSON_VERSION_CHECKED),
                    "If the details below look wrong: run with --keep and read devices.json.",
                )
            try:
                chosen, notes = pick_devices(dl.devices, a.want, a.all, a.family, a.name, install_cmd, dl.problems)
            except Several as e:
                if not a.dry_run:
                    raise
                chosen = e.devices
                notes = ["Found %d. A real run asks which (or: --device NAME_OR_UDID, repeated; --all)." % len(chosen),
                         "Each is checked below."]
        except Several:
            raise
        except Refusal as e:
            lines = str(e).split("\n")
            rep.bad(lines[0], *lines[1:])
        for n in notes:
            rep.note(n)
        results = []
        for d in chosen:
            out = []
            _device_checks(a, d, out)
            results.append((d, out))
        ready = [d for d, out in results if not [m for m, _, _ in out if m == "bad"]]
        for d, out in results:
            rep.ok('%s: "%s", %s, UDID %s' % (d.kind, d.name, d.model, d.udid))
            for mark, text, fix in out:
                if mark == "ok":
                    rep.ok("  " + text, *fix)
                elif mark == "unsure":
                    rep.unsure("  " + text, *fix)
                elif mark == "note":
                    rep.note("  " + text)
                elif ready:
                    # Others are ready: this one is left out, not the run.
                    rep.skip("  %s: %s" % (d.name, text), *fix)
                else:
                    rep.bad("  %s: %s" % (d.name, text) if len(results) > 1 else "  " + text, *fix)
        skipped = [d for d, _ in results if d not in ready]
        if ready and skipped:
            rep.note("Left out, the rest go on: " + ", ".join('"%s"' % d.name for d in skipped))
        chosen = ready
        plan["devices"] = "\n".join("%s\t%s" % (d.udid, d.name) for d in ready)
        plan["skipped"] = "\n".join("%s\t%s" % (d.udid, d.name) for d in skipped)
    udids = [d.udid for d in chosen]
    names = dict((d.udid, d.name) for d in chosen)

    # ---- the app
    rep.section("The app")
    if getattr(a, "note", ""):
        lines = a.note.split("\n")
        rep.unsure(lines[0], *lines[1:])
    app = read_facts(a.app_facts)
    built_ext = [v for k, v in _read_pairs(a.app_facts) if k == "extension"]
    if app.get("bundle_id"):
        if app["bundle_id"] != a.bundle_id:
            rep.bad(
                "The built app is %s, but this installer is set up for %s." % (app["bundle_id"], a.bundle_id),
                "Fix: bundle_id in the ios_install of the app's BUILD.bazel must match the app's bundle_id.",
            )
        else:
            rep.ok("%s (%s)%s, built for a device, not signed yet" % (app.get("name", "The app"), a.bundle_id, ", " + app["size"] if app.get("size") else ""))
        want_ext = [b.bundle_id for b in bundles[1:]]
        for e in built_ext:
            if e not in want_ext:
                rep.bad(
                    "The app holds the extension %s, which the installer has no entry for." % e,
                    "Fix: add it to extensions in the ios_install of the app's BUILD.bazel.",
                )
            else:
                rep.ok("  with the extension %s" % e)
        for e in want_ext:
            if e not in built_ext:
                rep.unsure("The installer expects the extension %s, but the app does not hold it." % e)
    elif app.get("error"):
        rep.bad(app["error"], "Fix: run it through Bazel:  " + install_cmd)
    else:
        rep.unsure("No built app was given, so it is not checked here.", "A real run builds it:  " + install_cmd)
        for b in bundles[1:]:
            rep.note("Extension in the build: %s" % b.bundle_id)

    # ---- the profiles
    rep.section("The provisioning profiles")
    pick, passed = (None, {})
    if udids:
        pick, passed = pick_set(profiles, bundles, udids, ids, now, a.team or None)
    team = a.team
    if not team:
        known = candidate_teams(profiles, ids, now, a.default_team or None)
        paid = [t for t, _, free in sorted(xinfo["teams"], key=lambda x: (0 if x[0] == a.default_team else 1, 1 if x[2] else 0))]
        known += [t for t in paid if t not in known]
        team = known[0] if known else a.default_team
    need_mint = "0"
    mint_udids = []
    gaps = {}
    if pick:
        days = int((pick.expires - now).total_seconds() // 86400)
        expires = pick.expires.strftime("%Y-%m-%d")
        for b in bundles:
            p = pick.profiles[b.bundle_id]
            scope = "this bundle only" if p.exact else "any app of the team"
            caps = ", with " + ", ".join(capability_name(k) for k in b.capabilities) if b.capabilities else ""
            rep.ok("%s: %s (team %s, %s)%s" % (b.bundle_id, p.name, p.team, scope, caps))
        rep.ok("They list every device and match a certificate in the keychain (%s)" % ids[pick.sha])
        if days < RENEW_DAYS:
            need_mint = "renew"
            mint_udids = udids[:1]
            rep.unsure(
                "The first runs out on %s (%d day%s left)." % (expires, days, "" if days == 1 else "s"),
                "A real run asks Xcode for fresh ones first, and uses these if that fails.",
            )
        else:
            rep.ok("They run out on %s (%d days left). After that day the app will not open until you run this again." % (expires, days))
        plan.update(identity_sha=pick.sha, identity_name=ids[pick.sha], profile_team=pick.team, profile_expires=expires)
        team = pick.team
    elif udids:
        need_mint = "1"
        mint_udids = devices_to_register(profiles, bundles, udids, now, a.team or None)
        gaps = capability_gaps(profiles, bundles, udids, ids, now, a.team or team or None)
        for b in bundles:
            bad = passed.get(b.bundle_id, [])
            relevant = [(p, why) for p, why in bad if why not in (APP_ID,)]
            if relevant:
                rep.note("For %s, the profiles on this Mac and why each will not do:" % b.bundle_id)
                for p, why in relevant[:8]:
                    rep.note("  %s: %s" % (p.name, p.why(why, udids, names, b.capabilities)))
                if len(relevant) > 8:
                    rep.note("  ... and %d more" % (len(relevant) - 8))
        if not team:
            rep.bad(
                "No usable profiles on this Mac, and no way to tell which developer team to ask.",
                "Fix: pass the team:  %s -- --team TEAMID" % install_cmd,
            )
        else:
            rep.ok(
                "None on this Mac fit yet. A real run asks Xcode to make them: it registers %s" % (
                    ", ".join('"%s"' % names[u] for u in mint_udids)),
                "with team %s and creates a development profile for %s." % (team, " and ".join(b.bundle_id for b in bundles)),
                "That needs the internet, and the team's Apple ID in Xcode > Settings > Accounts.",
            )
            if gaps:
                rep.unsure(*capability_steps(gaps, team))
            if xteams and team not in xteams:
                rep.unsure(
                    "Xcode's settings list the teams %s, but not %s." % (", ".join(xteams), team),
                    "Fix: sign in with that team's Apple ID, or pass another team: -- --team TEAMID",
                )
    else:
        rep.unsure("Not checked: a profile is for devices, and no device was found.")
    plan["team"] = team or ""
    plan["need_mint"] = need_mint
    plan["mint_udids"] = " ".join(mint_udids)

    # ---- what a run would do
    if a.dry_run:
        rep.section("What a real run would do")
        if rep.blocked:
            rep.note("Nothing yet: fix the lines marked %s above first." % rep.marks[1])
        else:
            n = 1
            if need_mint != "0":
                rep.note("%d. Ask Xcode (xcodebuild -allowProvisioningUpdates) to register %s with team %s" % (
                    n, ", ".join('"%s"' % names[u] for u in mint_udids), team))
                rep.note("   and make development profiles for %s. This is the only step that talks to Apple." % (
                    " and ".join(b.bundle_id for b in bundles)))
                n += 1
            who = plan.get("identity_name") or "the team's development identity"
            for b in reversed(bundles):
                ents = entitlements_for(pick.profiles[b.bundle_id], b) if pick else dry_entitlements(b, team)
                rep.note("%d. Sign %s with %s and these entitlements:" % (n, b.label(), who))
                for k, v in sorted(ents.items()):
                    rep.note("     %s = %s" % (k, v))
                n += 1
            for d in chosen:
                rep.note('%d. Install it on "%s" (xcrun devicectl device install app) and start it.' % (n, d.name))
                n += 1
    return plan


def _read_pairs(path):
    """key=value lines, in order, keys repeated."""
    out = []
    if not path:
        return out
    try:
        with open(path, "r", errors="replace") as f:
            for line in f:
                key, sep, value = line.rstrip("\n").partition("=")
                if sep:
                    out.append((key.strip(), value.strip()))
    except OSError:
        pass
    return out


def fake_profiles(out, bundles, udids, team):
    """Made-up, already decoded development profiles, one per bundle, with its
    capabilities, these devices and the fixture certificate of
    testdata/identities.txt (for tests only: no device would take them)."""
    os.makedirs(out, exist_ok=True)
    for i, b in enumerate(bundles):
        ents = {
            "application-identifier": team + "." + b.bundle_id,
            "com.apple.developer.team-identifier": team,
            "get-task-allow": True,
            "keychain-access-groups": [team + ".*"],
        }
        ents.update(mint_entitlements(b))
        doc = {
            "AppIDName": "Test",
            "ApplicationIdentifierPrefix": [team],
            "CreationDate": datetime.datetime(2026, 10, 1),
            "DeveloperCertificates": [b"fixture certificate: development"],
            "Entitlements": ents,
            "ExpirationDate": datetime.datetime(2099, 1, 1),
            "Name": "Test: " + b.bundle_id,
            "Platform": ["iOS"],
            "ProvisionedDevices": list(udids),
            "TeamIdentifier": [team],
            "TeamName": "Test Team",
            "TimeToLive": 365,
            "UUID": "00000000-0000-4000-8000-%012d" % i,
            "Version": 1,
        }
        with open(os.path.join(out, "test_%d.plist" % i), "wb") as f:
            plistlib.dump(doc, f)


RENEW_DAYS = 7


# ---- command line --------------------------------------------------------------


def _now(text):
    if text:
        return _as_utc(datetime.datetime.strptime(text, "%Y-%m-%dT%H:%M:%SZ"))
    return datetime.datetime.now(datetime.timezone.utc)


def _app_args(s):
    """The options that describe the app (ios_install passes them on)."""
    s.add_argument("--name", default="The app")
    s.add_argument("--bundle-id", required=True)
    s.add_argument("--entitlements", default="")
    s.add_argument("--extension", action="append", default=[], help="BUNDLE_ID[=ENTITLEMENTS]")
    s.add_argument("--family", action="append", default=[])
    s.add_argument("--min-os", default="26.0")
    s.add_argument("--install-cmd", default="bazel run :install")
    s.add_argument("--note", default="")


def main(argv):
    ap = argparse.ArgumentParser(description=__doc__.split("\n")[0])
    sub = ap.add_subparsers(dest="cmd", required=True)

    s = sub.add_parser("choose", help="which devices, from devicectl's JSON: UDID<TAB>name a line")
    s.add_argument("--json", required=True)
    s.add_argument("--want", action="append", default=[])
    s.add_argument("--all", action="store_true")
    _app_args(s)

    s = sub.add_parser("pick", help="a profile for every bundle; writes what signing needs")
    s.add_argument("--dir", action="append", default=[])
    s.add_argument("--udid", action="append", default=[])
    s.add_argument("--identities", required=True)
    s.add_argument("--team", default="")
    s.add_argument("--now", default="")
    s.add_argument("--out", required=True)
    _app_args(s)

    s = sub.add_parser("gaps", help="capabilities the App IDs lack, and what to click")
    s.add_argument("--dir", action="append", default=[])
    s.add_argument("--udid", action="append", default=[])
    s.add_argument("--identities", required=True)
    s.add_argument("--team", default="")
    s.add_argument("--now", default="")
    _app_args(s)

    s = sub.add_parser("mint-project", help="write the throwaway Xcode project")
    s.add_argument("--out", required=True)
    s.add_argument("--team", required=True)
    _app_args(s)

    s = sub.add_parser("fake-profiles", help="tests only: made-up decoded profiles for every bundle")
    s.add_argument("--out", required=True)
    s.add_argument("--udid", action="append", default=[])
    s.add_argument("--team", default="ABCDE12345")
    _app_args(s)

    s = sub.add_parser("bundle-id", help="print a bundle's CFBundleIdentifier")
    s.add_argument("app")

    s = sub.add_parser("preflight", help="every check before an install, with a tick or a cross a line")
    s.add_argument("--host", default="", help="key=value facts about this Mac")
    s.add_argument("--devices-json", default="")
    s.add_argument("--want", action="append", default=[])
    s.add_argument("--all", action="store_true")
    s.add_argument("--details-dir", default="")
    s.add_argument("--lock-dir", default="")
    s.add_argument("--identities", default="")
    s.add_argument("--identities-all", default="")
    s.add_argument("--xcode-defaults", default="")
    s.add_argument("--dir", action="append", default=[])
    s.add_argument("--team", default="")
    s.add_argument("--default-team", default="")
    s.add_argument("--now", default="")
    s.add_argument("--app-facts", default="")
    s.add_argument("--dry-run", action="store_true")
    s.add_argument("--ascii", action="store_true")
    s.add_argument("--out", default="", help="folder for the plan, one file a value")
    _app_args(s)

    s = sub.add_parser("explain", help="what a tool's failure most likely means")
    s.add_argument("--log", action="append", default=[])
    s.add_argument("--udid", default="")
    s.add_argument("--team", default="")
    s.add_argument("--bundle-id", default="")
    s.add_argument("--min-os", default="")
    s.add_argument("--name", default="")
    s.add_argument("--install-cmd", default="")

    a = ap.parse_args(argv)
    if hasattr(a, "family") and not a.family:
        a.family = ["iphone", "ipad"]
    for stream in (sys.stdout, sys.stderr):
        try:  # device names are often not ASCII (a curly apostrophe)
            stream.reconfigure(encoding="utf-8", errors="replace")
        except (AttributeError, ValueError):
            pass
    try:
        if a.cmd == "choose":
            dl = load_device_list(a.json)
            try:
                ds, notes = pick_devices(dl.devices, a.want, a.all, a.family, a.name, a.install_cmd, dl.problems)
            except Several as e:
                ds, notes, rc = e.devices, [], 5
            else:
                rc = 0
            for n in notes:
                print(n, file=sys.stderr)
            # Per device: the UDID, the name, its pairing and Developer Mode
            # as devicectl's list says them, and the device in words;
            # install.sh reads them back. Exit 5: several, nothing said which.
            for d in ds:
                print("%s\t%s\t%s\t%s\t%s" % (d.udid, d.name, d.pairing, d.developer_mode, d.describe()))
            return rc

        if a.cmd == "preflight":
            rep = Report(ascii_only=a.ascii)
            try:
                plan = preflight(a, rep)
            except Several as e:
                print(str(e), file=sys.stderr)
                return 5
            print("\n".join(rep.lines))
            if a.out:
                os.makedirs(a.out, exist_ok=True)
                for key, value in plan.items():
                    with open(os.path.join(a.out, key), "w") as f:
                        f.write(str(value) + "\n")
            if rep.blocked:
                print("")
                print("%d thing%s to fix first (marked %s above)." % (len(rep.blocked), "" if len(rep.blocked) == 1 else "s", rep.marks[1]))
                return 10
            return 0

        if a.cmd == "explain":
            text = ""
            for path in a.log:
                try:
                    with open(path, "r", errors="replace") as f:
                        text += f.read() + "\n"
                except OSError:
                    pass
            found = explain(text, udid=a.udid, team=a.team, bundle_id=a.bundle_id, min_os=a.min_os, app=a.name,
                            install_cmd=a.install_cmd)
            if not found:
                return 3
            for i, (_, lines) in enumerate(found[:3]):
                if i:
                    print("Or:")
                for line in lines:
                    print(line)
            return 0

        if a.cmd == "pick":
            bundles = parse_bundles(a.bundle_id, a.entitlements, a.extension)
            ids = read_identities(a.identities)
            now = _now(a.now)
            profiles = load_profiles(a.dir)
            pick, passed = pick_set(profiles, bundles, a.udid, ids, now, a.team or None)
            if not pick:
                any_listed = False
                for b in bundles:
                    bad = [(p, why) for p, why in passed.get(b.bundle_id, []) if why != APP_ID]
                    if bad:
                        any_listed = True
                        print("For %s, the profiles on this Mac and why each will not do:" % b.bundle_id, file=sys.stderr)
                        for p, why in bad:
                            print("  %s: %s" % (p.name, p.why(why, a.udid, None, b.capabilities)), file=sys.stderr)
                    else:
                        print("For %s there is no development profile on this Mac." % b.bundle_id, file=sys.stderr)
                if not any_listed and not profiles:
                    print("This Mac has no provisioning profiles at all.", file=sys.stderr)
                return 3
            os.makedirs(a.out, exist_ok=True)
            with open(os.path.join(a.out, "sign.txt"), "w") as sign:
                for i, b in enumerate(bundles):
                    p = pick.profiles[b.bundle_id]
                    ents = os.path.join(a.out, "entitlements.%d.plist" % i)
                    with open(ents, "wb") as f:
                        plistlib.dump(entitlements_for(p, b), f)
                    src = p.path[: -len(".plist")] + ".src"
                    original = p.path
                    if os.path.exists(src):
                        with open(src) as f:
                            original = f.read().strip()
                    sign.write("%s\t%s\t%s\t%s\n" % (b.bundle_id, original, ents, p.name))
            for v in (pick.sha, ids[pick.sha], pick.team, pick.expires.strftime("%Y-%m-%d")):
                print(v)
            return 0

        if a.cmd == "gaps":
            bundles = parse_bundles(a.bundle_id, a.entitlements, a.extension)
            gaps = capability_gaps(load_profiles(a.dir), bundles, a.udid, read_identities(a.identities), _now(a.now), a.team or None)
            if not gaps:
                return 3
            for line in capability_steps(gaps, a.team):
                print(line)
            return 0

        if a.cmd == "mint-project":
            bundles = parse_bundles(a.bundle_id, a.entitlements, a.extension)
            print(write_mint_project(a.out, bundles, a.team, a.min_os))
            return 0

        if a.cmd == "fake-profiles":
            fake_profiles(a.out, parse_bundles(a.bundle_id, a.entitlements, a.extension), a.udid, a.team)
            return 0

        if a.cmd == "bundle-id":
            with open(os.path.join(a.app, "Info.plist"), "rb") as f:
                print(plistlib.load(f)["CFBundleIdentifier"])
            return 0
    except Refusal as e:
        print(str(e), file=sys.stderr)
        return 2
    return 64


if __name__ == "__main__":
    sys.exit(main(sys.argv[1:]))
