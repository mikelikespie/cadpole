#!/usr/bin/env python3
"""Slice Cadpole's Bambu Studio projects headless with the official slicers.

    bambu_slice_check.py PROJECT.3mf [PROJECT.3mf ...] [--expect '1:1,2,3;2:4'] [--slicer bambu|orca|both]

Each project is sliced, every plate, by the Bambu Studio and/or OrcaSlicer
command-line slicer inside a container, and the script prints, per plate,
the filaments its G-code header lists and the time it estimates. With
`--expect`, each named plate must list exactly those filaments; the script
exits 1 when a slice fails or a plate does not match.

**Only the slicer binaries are run**: their own release AppImages,
extracted, mounted into a container that provides an X virtual
framebuffer and the system libraries (Bambu Studio's read-only;
OrcaSlicer writes a directory into its own). Nothing of either slicer is
read or shipped by Cadpole. Point the script at them with:

    CADPOLE_BAMBU_APP=/path/to/BambuStudio/squashfs-root   (holds bin/bambu-studio)
    CADPOLE_ORCA_APP=/path/to/OrcaSlicer/squashfs-root     (holds bin/orca-slicer)
    CADPOLE_BAMBU_IMAGE=bbs-cli:local  CADPOLE_ORCA_IMAGE=bbs-cli:orca   (the defaults)

When Docker, an image or an app is missing the check for that slicer is
**skipped**, with the reason, and the script exits 0: it is a check for a
machine that has the slicers, not a test the tree depends on. Checked with
Bambu Studio 2.8.2.61 and OrcaSlicer 2.4.2.

Stdlib only.
"""
import argparse
import json
import os
import re
import shutil
import subprocess
import sys
import tempfile

SLICERS = {
    "bambu": {
        "app_env": "CADPOLE_BAMBU_APP",
        "image_env": "CADPOLE_BAMBU_IMAGE",
        "image": "bbs-cli:local",
        "binary": "bin/bambu-studio",
        "env": "LD_LIBRARY_PATH=/app/bin",
        "mount": "ro",
    },
    "orca": {
        "app_env": "CADPOLE_ORCA_APP",
        "image_env": "CADPOLE_ORCA_IMAGE",
        "image": "bbs-cli:orca",
        "binary": "bin/orca-slicer",
        "env": "APPDIR=/app LD_LIBRARY_PATH=/app/lib:/app/bin",
        # OrcaSlicer 2.4.2 creates a directory inside its own app directory
        # and aborts (boost create_directories) when it cannot.
        "mount": "rw",
    },
}


def available(name):
    """(app dir, image) for a slicer, or the reason it cannot run here."""
    s = SLICERS[name]
    if shutil.which("docker") is None:
        return None, "docker is not on PATH"
    app = os.environ.get(s["app_env"], "")
    if not app or not os.path.isfile(os.path.join(app, s["binary"])):
        return None, f"{s['app_env']} does not name an extracted app holding {s['binary']}"
    image = os.environ.get(s["image_env"], s["image"])
    ok = subprocess.run(["docker", "image", "inspect", image], capture_output=True).returncode == 0
    if not ok:
        return None, f"the image {image} is not present"
    return (os.path.abspath(app), image), None


def plates_of(out_dir):
    """Per plate number: (filaments listed in the G-code header, estimated time)."""
    found = {}
    for f in sorted(os.listdir(out_dir)):
        m = re.match(r"plate_(\d+)\.gcode$", f)
        if not m:
            continue
        with open(os.path.join(out_dir, f), errors="replace") as g:
            head = g.read(6000)
        fil = re.search(r"^; filament: ([^\n]*)", head, re.M)
        tm = re.search(r"total estimated time: ([^;\n]*)", head)
        found[int(m.group(1))] = (
            sorted(int(x) for x in fil.group(1).split(",")) if fil else [],
            tm.group(1).strip() if tm else "?",
        )
    return found


def slice_one(name, app, image, project):
    """Slice every plate of `project`; (return code, error text, plates)."""
    s = SLICERS[name]
    with tempfile.TemporaryDirectory(prefix="cadpole-slice-") as work:
        os.makedirs(os.path.join(work, "out"))
        os.makedirs(os.path.join(work, "data"))
        os.makedirs(os.path.join(work, "home"))
        shutil.copy(project, os.path.join(work, "in.3mf"))
        script = (
            f"cd /app && export {s['env']} LC_ALL=C && "
            f"timeout 600 xvfb-run -a {s['binary']} --datadir /w/data --outputdir /w/out "
            "--slice 0 --export-3mf out.3mf /w/in.3mf > /w/out/log.txt 2>&1"
        )
        subprocess.run(
            ["docker", "run", "--rm", "-u", f"{os.getuid()}:{os.getgid()}", "-e", "HOME=/w/home",
             "-v", f"{app}:/app:{s['mount']}", "-v", f"{work}:/w", image, "bash", "-c", script],
            capture_output=True,
        )
        out = os.path.join(work, "out")
        try:
            with open(os.path.join(out, "result.json")) as f:
                r = json.load(f)
        except (OSError, ValueError):
            return -1, "no result.json (the slicer crashed)", {}
        return r.get("return_code", -1), r.get("error_string", ""), plates_of(out)


def parse_expect(text):
    want = {}
    for blk in filter(None, (text or "").split(";")):
        plate, fils = blk.split(":")
        want[int(plate)] = sorted(int(x) for x in fils.split(","))
    return want


def main():
    ap = argparse.ArgumentParser(description=__doc__, formatter_class=argparse.RawDescriptionHelpFormatter)
    ap.add_argument("projects", nargs="+")
    ap.add_argument("--expect", default="", help="plate:filaments;... e.g. '1:1,2,3,4;2:1,2;3:3'")
    ap.add_argument("--slicer", default="both", choices=["bambu", "orca", "both"])
    a = ap.parse_args()
    want = parse_expect(a.expect)
    failed = False
    for name in (["bambu", "orca"] if a.slicer == "both" else [a.slicer]):
        where, why = available(name)
        if where is None:
            print(f"{name}: skipped: {why}")
            continue
        for project in a.projects:
            rc, err, plates = slice_one(name, where[0], where[1], project)
            print(f"{name}: {project}: return code {rc} {err}")
            for n, (fils, tm) in sorted(plates.items()):
                mark = ""
                if n in want:
                    mark = "  ok" if fils == want[n] else f"  EXPECTED {want[n]}"
                    failed |= fils != want[n]
                print(f"  plate {n}: filaments {fils}, {tm}{mark}")
            failed |= rc != 0
    sys.exit(1 if failed else 0)


if __name__ == "__main__":
    main()
