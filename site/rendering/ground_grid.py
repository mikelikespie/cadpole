"""The ground grid under a scene's frames: lines on the plane the model stands
over, drawn in each frame's own perspective from the camera the renderer wrote
for it (--camera-json: docs/views.md, "The camera sidecar").

The grid has no outline. Every line is cut into short pieces and each piece
has its own opacity, the product of three fades:

  radius   distance on the ground from the centre of the model's footprint
  horizon  the angle at which the line of sight meets the ground
  edge     distance, in the picture, from the edge of the frame

so the grid is strongest under the model and is gone before it reaches its own
extent, the horizon or the frame. The lines sit on a lattice fixed in the
world, so they stay put while the camera and the fades move over them.
"""
import math

GRID = {
    "colour": "#2fbf9f",
    "minor_opacity": 0.16,   # a line's opacity where nothing fades it
    "major_opacity": 0.34,
    "major_every": 5,        # every fifth line is a major one
    "line": 1 / 900,         # a minor line's width, as a share of the frame's width; a major line is 1.5 times it
    "per_radius": 14,        # about this many minor squares across the footprint's radius in the first frame
    "radius": (0.8, 1.8) ,  # full strength within, nothing beyond: multiples of the footprint's radius
    "horizon": (4.0, 11.0),  # degrees between the line of sight and the ground: nothing below, full strength above
    "edge": 0.16,            # the fade at the frame's edge, as a share of the frame's shorter side
    "piece": 1 / 40,         # the length of one piece of a line, as a share of the footprint's radius
    "levels": 40,            # opacity steps
}


def smoothstep(a, b, x):
    t = min(1.0, max(0.0, (x - a) / (b - a)))
    return t * t * (3 - 2 * t)


def mul(m, v):
    return [sum(m[r * 4 + c] * v[c] for c in range(4)) for r in range(4)]


def dot(a, b):
    return sum(x * y for x, y in zip(a, b))


def footprint(cam, ground):
    """The centre (in the plane's own u, v) and radius of the model's bounding box seen from above."""
    lo, hi = cam["bounds"]["min"], cam["bounds"]["max"]
    o, u, v = ground["origin"], ground["u"], ground["v"]
    a, b = [], []
    for k in range(8):
        p = [(hi if k >> i & 1 else lo)[i] - o[i] for i in range(3)]
        a.append(dot(p, u))
        b.append(dot(p, v))
    return (min(a) + max(a)) / 2, (min(b) + max(b)) / 2, max(max(a) - min(a), max(b) - min(b)) / 2


def plane(cams):
    """One ground plane for a whole sequence: the lowest any of its frames stands on, so the grid
    stays where it is while the model comes apart above it."""
    return min((c["ground"] for c in cams), key=lambda g: dot(g["origin"], g["normal"]))


def spacing(cam, ground):
    """The minor spacing: the 1, 2, 5 step nearest a set share of the footprint's radius."""
    want = footprint(cam, ground)[2] / GRID["per_radius"]
    decade = 10 ** math.floor(math.log10(want))
    return min((m * decade for m in (1, 2, 5, 10)), key=lambda s: abs(math.log(s / want)))


def svg(cam, size, window, ground, minor):
    """The grid for one frame of `size` (width, height), as an SVG the size of `window`: the part of the
    frame (x, y, width, height in the frame's pixels; it may reach past the frame) the picture is cut from."""
    W, H = size
    wx, wy, ww, wh = window
    V, PR = cam["view"], cam["projection"]
    o, u, v, n = ground["origin"], ground["u"], ground["v"], ground["normal"]
    eye = cam["eye"]
    near = cam["near"] * 0.25
    ca, cb, R = footprint(cam, ground)
    r0, r1 = GRID["radius"][0] * R, GRID["radius"][1] * R
    piece = GRID["piece"] * R
    edge = GRID["edge"] * min(ww, wh)
    sight = [cam["target"][i] - eye[i] for i in range(3)]

    def world(a, b):
        return [o[i] + u[i] * a + v[i] * b for i in range(3)]

    def px(p):
        e = mul(V, p + [1.0])
        if -e[2] <= near:
            return None
        c = mul(PR, e)
        return (c[0] / c[3] + 1) / 2 * W, (1 - c[1] / c[3]) / 2 * H

    def strength(a, b, at):
        p = world(a, b)
        ray = sight if cam.get("orthographic") else [p[i] - eye[i] for i in range(3)]
        graze = math.degrees(math.asin(min(1.0, max(0.0, -dot(ray, n) / math.sqrt(dot(ray, ray))))))
        s = 1 - smoothstep(r0, r1, math.hypot(a - ca, b - cb))
        s *= smoothstep(GRID["horizon"][0], GRID["horizon"][1], graze)
        return s * smoothstep(0, edge, min(at[0] - wx, wx + ww - at[0])) * smoothstep(0, edge, min(at[1] - wy, wy + wh - at[1]))

    paths = {}
    k0, k1 = math.ceil((ca - r1) / minor), math.floor((ca + r1) / minor)
    l0, l1 = math.ceil((cb - r1) / minor), math.floor((cb + r1) / minor)
    for along_v, lines in ((True, range(k0, k1 + 1)), (False, range(l0, l1 + 1))):
        for k in lines:
            fixed = k * minor
            off = fixed - (ca if along_v else cb)
            half = math.sqrt(max(0.0, r1 * r1 - off * off))
            mid = cb if along_v else ca
            count = max(1, math.ceil(2 * half / piece))
            pts = []
            for j in range(count + 1):
                t = mid - half + 2 * half * j / count
                pts.append((fixed, t) if along_v else (t, fixed))
            for (a0, b0), (a1, b1) in zip(pts, pts[1:]):
                p, q = px(world(a0, b0)), px(world(a1, b1))
                if not p or not q:
                    continue
                centre = ((p[0] + q[0]) / 2, (p[1] + q[1]) / 2)
                level = round(strength((a0 + a1) / 2, (b0 + b1) / 2, centre) * GRID["levels"])
                if level:
                    paths.setdefault((k % GRID["major_every"] == 0, level), []).append(
                        f"M{p[0] - wx:.2f} {p[1] - wy:.2f}L{q[0] - wx:.2f} {q[1] - wy:.2f}")
    sw = ww * GRID["line"]
    out = [f'<svg xmlns="http://www.w3.org/2000/svg" width="{ww}" height="{wh}" viewBox="0 0 {ww} {wh}">'
           f'<g fill="none" stroke="{GRID["colour"]}" stroke-linecap="butt">']
    for (major, level), d in sorted(paths.items()):
        opacity = GRID["major_opacity" if major else "minor_opacity"] * level / GRID["levels"]
        out.append(f'<path d="{"".join(d)}" stroke-width="{sw * (1.5 if major else 1):.2f}" stroke-opacity="{opacity:.4f}"/>')
    out.append("</g></svg>")
    return "".join(out)
