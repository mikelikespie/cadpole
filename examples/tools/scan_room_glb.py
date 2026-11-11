#!/usr/bin/env python3
"""A synthetic room scan as binary glTF (GLB), for examples/scan_underlay.star and benchmarks.

    examples/tools/scan_room_glb.py                      # writes examples/assets/scan_room.glb
    examples/tools/scan_room_glb.py --triangles 2000000 -o /tmp/scan_2m.glb

The file has the layout a phone's LiDAR scanner writes: one node
named `scan`, one mesh with one triangle primitive -- POSITION and NORMAL as
float32, COLOR_0 as normalised uint8 RGBA, uint32 indices -- and one white,
unlit material, glTF's own frame of +Y up in metres.

The room is 4 m across (+X), 3 m deep and 2.5 m high, open at the top so a
viewer from above sees in: a floor, three walls and a front wall with a
doorway, each a grid of its own (a scan's surfaces are not welded where they
meet), every vertex jittered a few millimetres along its surface's normal the
way a depth camera's noise is, and every triangle wound to face into the
room. Colours are an oak floor with planks, warm white walls with a darker
skirting board and a pale blue window on the back wall -- given as sRGB,
written linear, as the glTF specification says COLOR_0 is.

Everything is deterministic: the jitter comes from a fixed-seed generator
and the JSON is written with sorted keys, so the same arguments give the same
bytes and the committed asset is reproducible. Only the standard library.
"""

import argparse
import json
import math
import os
import struct
import sys

W, D, H = 4.0, 3.0, 2.5  # metres: across (x), deep (z), high (y)
DOOR = (1.4, 2.3, 2.05)  # doorway in the front wall: x from, x to, height
WINDOW = (1.2, 2.8, 1.0, 2.0)  # on the back wall: x from, x to, y from, y to
JITTER = 0.003  # metres, along the normal


class Lcg:
    """Numerical Recipes' 32-bit LCG: small, fixed, the same everywhere."""

    def __init__(self, seed):
        self.s = seed & 0xFFFFFFFF

    def unit(self):
        self.s = (1664525 * self.s + 1013904223) & 0xFFFFFFFF
        return self.s / 4294967296.0 * 2.0 - 1.0


def srgb_to_linear(c):
    return c / 12.92 if c <= 0.04045 else ((c + 0.055) / 1.055) ** 2.4


def colour(surface, p):
    """The sRGB colour of a point on a surface."""
    x, y, z = p
    if surface == "floor":
        plank = int(x / 0.18)
        shade = 0.9 + 0.1 * ((plank * 37) % 7) / 6.0
        return (0.62 * shade, 0.43 * shade, 0.26 * shade)
    if y < 0.1:
        return (0.35, 0.33, 0.31)  # skirting board
    if surface == "back" and WINDOW[0] <= x <= WINDOW[1] and WINDOW[2] <= y <= WINDOW[3]:
        return (0.62, 0.76, 0.9)  # the window's glass
    # Warm white, a touch darker toward the floor where a room's light falls off.
    k = 0.9 + 0.08 * min(y / H, 1.0)
    return (0.93 * k, 0.9 * k, 0.84 * k)


def surfaces(n):
    """Each surface as (name, origin, u axis, v axis, u length, v length,
    normal into the room, cells along u, cells along v, a cell to leave out)."""
    cu = lambda length: max(1, round(length * n))
    door = lambda u0, v0, u1, v1: DOOR[0] <= (u0 + u1) / 2 <= DOOR[1] and (v0 + v1) / 2 <= DOOR[2]
    return [
        ("floor", (0.0, 0.0, 0.0), (1, 0, 0), (0, 0, 1), W, D, (0, 1, 0), cu(W), cu(D), None),
        ("back", (0.0, 0.0, 0.0), (1, 0, 0), (0, 1, 0), W, H, (0, 0, 1), cu(W), cu(H), None),
        ("left", (0.0, 0.0, 0.0), (0, 0, 1), (0, 1, 0), D, H, (1, 0, 0), cu(D), cu(H), None),
        ("right", (W, 0.0, 0.0), (0, 0, 1), (0, 1, 0), D, H, (-1, 0, 0), cu(D), cu(H), None),
        ("front", (0.0, 0.0, D), (1, 0, 0), (0, 1, 0), W, H, (0, 0, -1), cu(W), cu(H), door),
    ]


def build(n, seed=210):
    rng = Lcg(seed)
    pos, nrm, col, idx = [], [], [], []
    for name, o, ua, va, ul, vl, normal, nu, nv, skip in surfaces(n):
        base = len(pos) // 3
        for j in range(nv + 1):
            for i in range(nu + 1):
                u, v = ul * i / nu, vl * j / nv
                p = [o[k] + ua[k] * u + va[k] * v for k in range(3)]
                interior = 0 < i < nu and 0 < j < nv
                e = JITTER * rng.unit() if interior else 0.0
                p = [p[k] + normal[k] * e for k in range(3)]
                pos.extend(p)
                nrm.extend(normal)
                rgb = colour(name, p)
                col.extend([round(255 * srgb_to_linear(c)) for c in rgb] + [255])
        for j in range(nv):
            for i in range(nu):
                if skip and skip(ul * i / nu, vl * j / nv, ul * (i + 1) / nu, vl * (j + 1) / nv):
                    continue
                a = base + j * (nu + 1) + i
                b, c, d = a + 1, a + nu + 2, a + nu + 1
                # Counter-clockwise seen from the room: (u, v, normal) is a
                # right-handed frame for the floor, the left wall and the
                # front wall, left-handed for the other two.
                right_handed = cross(ua, va) == list(normal)
                if right_handed:
                    idx.extend([a, b, c, a, c, d])
                else:
                    idx.extend([a, c, b, a, d, c])
    return pos, nrm, col, idx


def cross(a, b):
    return [a[1] * b[2] - a[2] * b[1], a[2] * b[0] - a[0] * b[2], a[0] * b[1] - a[1] * b[0]]


def glb(pos, nrm, col, idx):
    views, blob = [], bytearray()

    def view(data, target):
        while len(blob) % 4:
            blob.append(0)
        views.append({"buffer": 0, "byteOffset": len(blob), "byteLength": len(data), "target": target})
        blob.extend(data)
        return len(views) - 1

    nv = len(pos) // 3
    f32 = lambda xs: struct.pack("<%df" % len(xs), *xs)
    lo = [min(pos[k::3]) for k in range(3)]
    hi = [max(pos[k::3]) for k in range(3)]
    # The bounds as float32 would round them, so the file's min/max agree
    # with its own positions.
    r32 = lambda x: struct.unpack("<f", struct.pack("<f", x))[0]
    accessors = [
        {"bufferView": view(f32(pos), 34962), "componentType": 5126, "count": nv, "type": "VEC3",
         "min": [r32(x) for x in lo], "max": [r32(x) for x in hi]},
        {"bufferView": view(f32(nrm), 34962), "componentType": 5126, "count": nv, "type": "VEC3"},
        {"bufferView": view(bytes(col), 34962), "componentType": 5121, "normalized": True, "count": nv, "type": "VEC4"},
        {"bufferView": view(struct.pack("<%dI" % len(idx), *idx), 34963), "componentType": 5125, "count": len(idx), "type": "SCALAR"},
    ]
    while len(blob) % 4:
        blob.append(0)
    doc = {
        "asset": {"version": "2.0", "generator": "cadpole examples/tools/scan_room_glb.py"},
        "extensionsUsed": ["KHR_materials_unlit"],
        "scene": 0,
        "scenes": [{"nodes": [0]}],
        "nodes": [{"mesh": 0, "name": "scan"}],
        "materials": [{
            "pbrMetallicRoughness": {"baseColorFactor": [1, 1, 1, 1], "metallicFactor": 0, "roughnessFactor": 1},
            "doubleSided": True,
            "extensions": {"KHR_materials_unlit": {}},
        }],
        "meshes": [{"primitives": [{
            "attributes": {"POSITION": 0, "NORMAL": 1, "COLOR_0": 2},
            "indices": 3, "material": 0, "mode": 4,
        }]}],
        "accessors": accessors,
        "bufferViews": views,
        "buffers": [{"byteLength": len(blob)}],
    }
    js = json.dumps(doc, separators=(",", ":"), sort_keys=True).encode()
    js += b" " * (-len(js) % 4)
    total = 12 + 8 + len(js) + 8 + len(blob)
    return (struct.pack("<4sII", b"glTF", 2, total) + struct.pack("<I4s", len(js), b"JSON") + js
            + struct.pack("<I4s", len(blob), b"BIN\0") + bytes(blob))


def area():
    return W * D + W * H + 2 * D * H + W * H - (DOOR[1] - DOOR[0]) * DOOR[2]


def main():
    here = os.path.dirname(os.path.abspath(__file__))
    ap = argparse.ArgumentParser(description=__doc__.split("\n\n")[0])
    ap.add_argument("--triangles", type=int, default=2000, help="about how many triangles (default 2000: the example asset)")
    ap.add_argument("-o", "--out", default=os.path.join(here, "..", "assets", "scan_room.glb"))
    a = ap.parse_args()
    n = math.sqrt(a.triangles / (2 * area()))  # cells per metre
    pos, nrm, col, idx = build(n)
    data = glb(pos, nrm, col, idx)
    with open(a.out, "wb") as f:
        f.write(data)
    print(f"{a.out}: {len(idx) // 3} triangles, {len(pos) // 3} vertices, {len(data)} bytes", file=sys.stderr)


if __name__ == "__main__":
    main()
