# A chess rook: a turned profile with a cove, a swept collar and a crenellated,
# cupped crown.
#
# What this example demonstrates:
#
#   * `path()` as a lathe profile -- `.h()`, `.v()`, `.line()`, a concave
#     `.arc(to, via=)` for the cove and a `.tangent_arc(to)` that leaves the
#     shaft smoothly for the collar -- revolved with `revolve()`. Arcs become
#     exact revolved surfaces, so the profile is what a woodturner would cut;
#   * `pattern_circular` of a cutter around Z, then one `subtract` with the
#     list of cutters plus a cup, which is how the crown's notches and hollow
#     are all cut in one boolean;
#   * a `kernel` choice that is `"auto"` or `"mesh"` only. The exact kernel
#     cuts the cup but refuses a box across the crown, so `"auto"` tries it,
#     says so, and hands the cut to the mesh kernel;
#   * a turned piece in the `wood` texture: the rings run around Z, which on
#     a lathe-turned part is where the grain really is.
#
# The profile starts `AXIS_GAP` off the axis rather than on it: a region that
# touches the axis makes degenerate faces in the current kernel, so the piece
# has a 0.6 mm pinhole down its centre that the slicer bridges without
# noticing.
#
#   cadpole render examples/chess_rook.star --backend path --view az=25,el=14 -o /tmp/rook.png
#   cadpole render examples/chess_rook.star --param notches=6 --param cup_d=0 -o /tmp/rook_six.png

height = param("height", 50, unit="mm", min=25, max=120, doc="Overall height")
foot_r = param("foot_r", 15, unit="mm", min=8, max=40, doc="Base radius")
shaft_r = param("shaft_r", 7.5, unit="mm", min=4, max=25, doc="Radius at the top of the shaft")
crown_r = param("crown_r", 11.5, unit="mm", min=5, max=30, doc="Radius of the crown")
crown_h = param("crown_h", 9, unit="mm", min=5, max=30, doc="Height of the crown's straight wall")
notches = param("notches", 4, min=2, max=8, doc="Crenellations round the crown (an even number cuts as `notches / 2` slots)")
notch_w = param("notch_w", 5, unit="mm", min=2, max=12, doc="Width of each notch")
notch_d = param("notch_d", 4, unit="mm", min=1, max=12, doc="Depth of each notch")
cup_d = param("cup_d", 2.5, unit="mm", min=0, max=10, doc="Depth of the hollow in the crown's top; 0 for a flat top")
kernel = param("kernel", "auto", choices=["auto", "mesh"],
               doc="Geometry kernel for the crown cut. `brep` is not offered: see the header")

AXIS_GAP = 0.3
assert_true(shaft_r < crown_r, msg="the collar has to flare out to the crown")
assert_true(crown_r < foot_r, msg="a rook stands on a base wider than its crown")
assert_true(notch_d < crown_h, msg="the notches are cut into the crown's straight wall")

# --- the turned profile ----------------------------------------------------
# (x, y) on the XZ plane is (radius, height). Bottom to top, out and back.
base_h = 4.5
cove_top = 0.3 * height              # where the cove meets the shaft
collar_z = height - crown_h - 5.0    # where the shaft starts to flare
cove_start = (foot_r - 1.0, base_h)
cove_end = (shaft_r + 1.2, cove_top)
# The cove is a concave arc: its through-point sits inside the chord, towards
# the axis and down, which is what makes it hollow rather than bulged. The
# offset is along the chord's normal so the cove stays a cove whatever the
# foot and shaft radii are.
chord = (cove_end[0] - cove_start[0], cove_end[1] - cove_start[1])
chord_len = math.hypot(chord[0], chord[1])
cove_via = (
    (cove_start[0] + cove_end[0]) / 2.0 - chord[1] / chord_len * 2.2,
    (cove_start[1] + cove_end[1]) / 2.0 + chord[0] / chord_len * 2.2,
)

profile = (
    path((AXIS_GAP, 0))
    .h(foot_r)                                   # the foot
    .v(base_h - 1.5)
    .line(cove_start)                            # a chamfer on the foot's top edge
    .arc(cove_end, via=cove_via)                 # the cove
    .line((shaft_r, collar_z))                   # the shaft, tapering gently
    .tangent_arc((crown_r, height - crown_h))    # the collar, tangent to the shaft
    .v(height)                                   # the crown's wall
    .h(AXIS_GAP)                                 # the top, back to the axis
)
turned = revolve(sketch(XZ, name="profile").add(profile), axis=(0, 0, 1), name="turned")
assert_eq(turned.kind, "brep")

# --- the crown ---------------------------------------------------------------
# One box across the whole crown makes two notches; `notches / 2` of them
# spread over a half turn make `notches`. `angle=180` shares the arc out so
# the last copy would land *on* 180 degrees -- the same slot as the first --
# so the count is spread over 180 - 180 / n instead.
slots = notches // 2
cutter = box(2 * crown_r + 2, notch_w, 2 * notch_d, center=True, at=(0, 0, height - notch_d), name="notch")
cutters = pattern_circular(cutter, slots, angle=180.0 - 180.0 / slots, name="notches") if slots > 1 else [cutter]
tools = list(cutters)
if cup_d > 0:
    # The cup's floor sits inside the material and its wall inside the crown's,
    # so nothing is coincident with anything.
    tools.append(cylinder(crown_r - 2.5, cup_d + 1.0, at=(0, 0, height - cup_d), name="cup"))
rook = subtract(turned, tools, name="rook", kernel=kernel)

lo, hi = rook.bbox()
assert_true(math.close(hi[2], height, tol=0.01), msg="the rook is `height` tall")
assert_true(math.close(lo[2], 0.0, tol=0.01), msg="the rook stands on z = 0")
assert_true(math.close(hi[0] - lo[0], 2 * foot_r, tol=0.05), msg="the foot is the widest part")
assert_true(rook.volume() < turned.volume(), msg="the crown cut removed material")

# Walnut: the oak preset's grain in a darker wood, the rings tightened to the
# size of the piece.
rook.material(preset="wood_oak", color="#4e3220", scale=26, roughness=0.42)

export(rook, "chess_rook.stl")
