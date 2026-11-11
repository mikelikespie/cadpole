# An organic vase: a smooth spline profile (x = radius, y = height) revolved
# about Z. The wall is one closed path: outer spline up, rim, inner spline
# down, floor. A small drainage hole keeps the profile off the axis.
#
# The inner wall is offset along the **surface normal**, not horizontally.
# A horizontal offset leaves a wall of `wall * cos(lean)`, so wherever the
# profile leans the part is thinner than asked, and at a flared rim it can be
# too thin to print. `cadpole check` reports thin walls.

h = param("height", 120, unit="mm", min=60, max=250)
belly = param("belly", 48, unit="mm", min=25, max=90, doc="Widest radius")
belly_at = param("belly_at", 0.55, min=0.2, max=0.85, doc="Height fraction of the widest point")
neck = param("neck", 26, unit="mm", min=12, max=60, doc="Narrowest radius near the top")
lip = param("lip", 34, unit="mm", min=12, max=70, doc="Radius at the rim")
foot = param("foot", 30, unit="mm", min=15, max=60, doc="Radius at the base")
wall = param("wall", 2.4, unit="mm", min=1.2, max=6)
floor = param("floor", 4, unit="mm", min=2, max=10)
drain = param("drain", 3, unit="mm", min=1, max=10)

# Control points of the outer surface, bottom to top, as (radius, height).
outer = [(foot, 0.0), (belly, h * belly_at), (neck, h * 0.88), (lip, h)]


def tangent_at(pts, i):
    """Unit tangent at `pts[i]`, by central difference (one-sided at the ends)."""
    a = pts[max(0, i - 1)]
    b = pts[min(len(pts) - 1, i + 1)]
    tr, tz = b[0] - a[0], b[1] - a[1]
    length = math.hypot(tr, tz)
    if length < 1e-9:
        fail("two control points of the vase profile coincide at %s" % str(pts[i]))
    return (tr / length, tz / length)


def offset_inward(pts, d):
    """`pts` moved distance `d` towards the axis, perpendicular to the curve.

    Going up the outside, the inward normal is the tangent turned a quarter
    turn: `(dr, dz)` becomes `(-dz, dr)`. Offsetting along that is what keeps
    the wall `d` thick wherever the profile leans. Offsetting horizontally
    does not.
    """
    out = []
    for i in range(len(pts)):
        tr, tz = tangent_at(pts, i)
        out.append((pts[i][0] - tz * d, pts[i][1] + tr * d))
    return out


def slide_to_height(pts, i, z):
    """`pts[i]` moved along its own tangent until it sits at height `z`.

    An offset curve's ends do not land on the planes the original's did: the
    rim point comes out above the rim and the foot point below the floor. This
    puts them back without bending the curve, so the rim stays flat at exactly
    `height` and the floor flat at exactly `floor`.
    """
    tr, tz = tangent_at(pts, i)
    if math.fabs(tz) < 1e-9:
        fail("the vase profile runs horizontally at %s, so it cannot reach that height" % str(pts[i]))
    t = (z - pts[i][1]) / tz
    return (pts[i][0] + tr * t, z)


inner = offset_inward(outer, wall)
last = len(inner) - 1
rim_inner = slide_to_height(inner, last, h)          # flat rim at the top
floor_inner = slide_to_height(inner, 0, floor)       # flat floor inside

profile = (
    path((drain, 0.0))
    .h(outer[0][0])                                  # base, out to the foot
    .spline(outer[1:], tension=0.5)                  # outer wall, up to the rim
    .line(rim_inner)                                 # across the rim
    .spline(list(reversed(inner[1:last])) + [floor_inner], tension=0.5)
    .h(drain)                                        # floor, in to the drainage hole
)

vase = revolve(sketch(XZ, name="profile").add(profile), axis=(0, 0, 1), name="vase")
vase.material(color="#b9c7d2", roughness=0.35)

# The part is exactly as tall as asked, and sits on the bed.
lo, hi = vase.bbox()
assert_true(math.close(hi[2], h, tol=0.01), msg="the vase is `height` tall")
assert_true(math.close(lo[2], 0.0, tol=0.01), msg="the vase sits on z = 0")

export(vase, "vase.stl")
