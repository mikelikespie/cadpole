# A pierced lamp shade, printed in translucent PETG, with the bulb in it.
#
# What this example demonstrates:
#
#   * `plane(origin=, normal=)` as a *working* plane: each window is a
#     `rounded_rect` sketched on a plane standing outside the shade and facing
#     it, then extruded inward through the wall. That is how you cut a shape
#     into a curved surface without computing where the surface is -- the
#     cutter is drawn flat, in the place it will cut;
#   * `pattern_circular(..., merge=True)`: one window becomes a ring of them
#     fused into a single cutter body, so the shade is pierced by one
#     `subtract` per row rather than by `windows` of them;
#   * a leaning wall done honestly: a wall `wall` thick measured *through* the
#     surface is `wall / cos(lean)` measured horizontally, so the inner cone
#     is offset by that and not by `wall`. Measure a shade built the naive way
#     and its wall is thinner than asked everywhere, by the cosine of its own
#     flare;
#   * **two bodies with different materials, one of them a light.** The shade
#     is `petg_translucent` -- it refracts, absorbs over 14 mm and scatters
#     over 3 mm -- and the bulb is `led`, which under `--backend path` is an
#     actual emitter, so the shade is lit from inside and the windows throw
#     light rather than merely being holes. Under the rasterizer both are
#     flat colours: this is the example that needs the path tracer.
#
# Kernels: the shell is `"brep"` and the window cut is `"mesh"`, each pinned
# because the other kernel gets that step wrong (the comments at each say
# how). `cadpole check` reports the shade watertight with some pairs of
# self-crossing triangles where a window's rounded corner leaves the thin
# conical wall at a grazing angle; they are the mesh kernel's own output, and
# a slicer closes them. The exact kernel refuses this cut.
#
#   cadpole render examples/lamp_shade.star --backend path --samples 192 \
#       --view az=28,el=12 --environment --exposure 1.15 -o /tmp/shade.png
#   cadpole render examples/lamp_shade.star --param bulb=false -o /tmp/shade_alone.png

r_bottom = param("bottom_radius", 54, unit="mm", min=25, max=120)
r_top = param("top_radius", 33, unit="mm", min=12, max=100)
height = param("height", 76, unit="mm", min=30, max=200)
wall = param("wall", 1.5, unit="mm", min=0.8, max=4, doc="Wall thickness, measured through the surface")
rim = param("rim", 4.0, unit="mm", min=0, max=10, doc="Solid band left at the top and bottom edges")
rows = param("rows", 3, min=1, max=5, doc="Rows of windows")
windows = param("windows", 11, min=3, max=24, doc="Windows in each row")
win_w = param("window_width", 12, unit="mm", min=4, max=60, doc="Window width at the surface")
win_r = param("window_radius", 4, unit="mm", min=0, max=20, doc="Corner radius of a window")
stagger = param("stagger", True, doc="Offset every other row by half a window")
bulb = param("bulb", True, doc="Include the emissive bulb (only the path tracer lights with it)")

assert_true(r_top < r_bottom, msg="this shade flares downward")
assert_true(2 * win_r <= win_w, msg="the corner radius cannot exceed half the window's width")

# --- the shell ---------------------------------------------------------------
# The outer surface leans by `lean` from vertical. A wall of thickness `wall`
# measured perpendicular to that surface is `wall / cos(lean)` wide measured
# horizontally, which is the offset the inner cone needs.
lean = math.atan((r_bottom - r_top) / height)
horizontal_wall = wall / math.cos(lean)
assert_true(horizontal_wall > wall, msg="a leaning wall is always wider than it is thick")

# A solid frustum with a second one taken out of it, rather than the
# four-point annular profile a shell like this wants. Revolving that profile
# builds two concentric cones a millimetre and a half apart, which the exact
# kernel cannot tessellate: it drops faces, and a body whose mesh would have
# a hole in it is refused rather than returned.
#
# Two solids and a boolean build the same shell and do tessellate. The
# boolean is pinned to `"brep"`: the two cones cross transversally, which is
# exactly what the exact kernel is for, and the mesh kernel -- which `"auto"`
# would reach for once the face count rises -- leaves self-crossing triangles
# where the two nearly-parallel conical surfaces meet the rims.
outer = cone(r_bottom, r_top, height, name="outer")
inner = cone(r_bottom - horizontal_wall, r_top - horizontal_wall, height + 2.0, at=(0, 0, -1), name="inner")
shell = subtract(outer, inner, name="shell", kernel="brep")

# --- the windows -------------------------------------------------------------
# One window, drawn flat on a plane that stands `REACH` outside the shade and
# faces inward, then extruded as far as the axis. The sketch plane's own y
# axis is world +Z, so a window's height is its `h`.
#
# How far it reaches matters twice over. A cutter long enough to leave the
# far side would cut a second window out of the *back* of the shade, so every
# window would be punched twice. And a cutter that runs all the way to the
# axis overlaps the eight others in the middle, which makes the merged ring a
# star of crossing slabs -- watertight, but with thousands of self-crossing
# triangles that `cadpole check` rightly complains about. So each one stops just
# inside the wall it cuts: the copies then never touch each other at all.
REACH = r_bottom + 20.0
band = (height - 2 * rim) / rows          # vertical share of one row
win_h = band - rim                        # leaving `rim` of material between rows
assert_true(win_h > 2 * win_r, msg="the windows are too short for that corner radius: fewer rows, or a smaller radius")

def wall_radius(z):
    """Radius of the outer surface at height `z`."""
    return r_bottom + (r_top - r_bottom) * z / height


cutters = []
for row in range(rows):
    z = rim + band * (row + 0.5)
    # The shade narrows with height, so the window's top edge is where its
    # inner surface sits closest to the axis; clearing that clears the rest.
    stop = wall_radius(z + win_h / 2.0) - horizontal_wall - 2.0
    assert_true(stop > 0, msg="the shade is too narrow at that height for a window cutter to stop inside it")
    face = plane(origin=(0.0, -REACH, z), normal=(0.0, 1.0, 0.0), x_dir=(1.0, 0.0, 0.0))
    one = extrude(sketch(face, name="window_%d" % row).rounded_rect(win_w, win_h, win_r), REACH - stop,
                  name="window_cut_%d" % row)
    # A whole turn divides by `windows`, so the last copy stops one step short
    # of the first -- exactly what a ring wants.
    #
    # `merge=True` fuses the ring into one cutter body, so the shade is
    # pierced by one boolean per row rather than by `windows` of them. It
    # fuses on the mesh kernel and says so -- and here it has to, because the
    # copies deliberately do not touch and the exact kernel cannot hold
    # separate lumps in one solid ("the exact kernel cannot hold two separate
    # lumps in one solid"), which is what it answers if asked.
    turn = 180.0 / windows if (stagger and row % 2 == 1) else 0.0
    cutters.append(pattern_circular(one.rotate(turn, axis=(0, 0, 1), name="window_turn_%d" % row),
                                    windows, axis=(0, 0, 1), merge=True, name="window_ring_%d" % row))

assert_eq(len(cutters), rows, msg="one fused ring of cutters per row")
shade = subtract(shell, cutters, name="shade", kernel="mesh")
assert_true(shade.volume() < shell.volume(), msg="the windows removed material")

lo, hi = shade.bbox()
assert_true(math.close(hi[2], height, tol=0.05), msg="the shade is `height` tall")
assert_true(math.close(lo[2], 0.0, tol=0.05), msg="the shade sits on z = 0")
assert_true(math.close(hi[0] - lo[0], 2 * r_bottom, tol=0.05), msg="the bottom rim is the widest part")

shade.material(preset="petg_translucent", color="#e8b465", roughness=0.3)

# --- the bulb ----------------------------------------------------------------
# A stubby emitter down in the shade, clear of the wall everywhere. `led` is
# emissive, and under the path tracer an emissive surface is a light: this is
# what makes the windows read as lit rather than as holes.
if bulb:
    bulb_r = min(14.0, r_top - horizontal_wall - 6.0)
    assert_true(bulb_r > 3.0, msg="no room for a bulb inside a shade this narrow")
    cylinder(bulb_r, 22, at=(0, 0, height * 0.3), name="bulb").material(preset="led")

export(shade, "lamp_shade.stl")
