# A knurled control knob with a helical grip and a D-shaft bore, built as one
# loft -- no kernel boolean anywhere, so it is exact and exports STEP.
#
# What this example demonstrates:
#
#   * `loft` through *rotated* sections: the grip is a star polygon lofted
#     through sections that each turn a little further, so every ridge is a
#     helix and the loft's ruled faces are the flanks. Loft pairs segment to
#     segment in construction order, so the twist per section is kept under
#     half a ridge pitch, which is what keeps it pairing ridge to ridge (and
#     keeps the "the loft is probably twisted" warning quiet, because here it
#     is twisted on purpose);
#   * the skirt and the cap as the first and last sections of the *same*
#     loft: a ring of as many segments as the star, turned to match the star
#     next to it, so each ridge tapers out into the chamfer as its own face.
#     A separate cone glued on would need the two touching faces to have the
#     same outline, and a chamfer cone cannot end on a star;
#   * a hole loop carried through every section -- the D bore is drawn in each
#     sketch, unrotated -- so the bore is a straight prism through a twisted
#     solid, exactly;
#   * a metal under the path tracer: `aluminum`, with the roughness brought
#     down so the helical flanks catch the environment (`--environment`).
#
#   cadpole render examples/knurled_knob.star --backend path --environment --view az=30,el=18 -o /tmp/knob.png
#   cadpole export examples/knurled_knob.star --format step -o /tmp/knob.step

radius = param("radius", 14, unit="mm", min=6, max=40, doc="Radius over the ridges")
height = param("height", 16, unit="mm", min=6, max=60, doc="Height of the knurled grip")
teeth = param("teeth", 30, min=8, max=90, doc="Ridges round the grip")
depth = param("depth", 0.9, unit="mm", min=0.3, max=3, doc="Depth of the grooves")
twist = param("twist", 40, unit="deg", min=0, max=120, doc="How far a ridge turns over the grip's height")
skirt = param("skirt", 2.5, unit="mm", min=0, max=10, doc="Chamfered skirt under the grip")
cap = param("cap", 3, unit="mm", min=1, max=10, doc="Chamfered cap over the grip")
shaft = param("shaft", 6, unit="mm", min=2, max=20, doc="Shaft diameter")
flat = param("flat", 0.5, unit="mm", min=0, max=5, doc="Depth of the D flat on the shaft; 0 for a round bore")

assert_true(depth < radius / 3, msg="grooves that deep leave no core")
assert_true(shaft / 2.0 + 2.0 < radius - depth, msg="the bore needs 2 mm of wall around it")


def polar(r, angle):
    return (r * math.cos(angle), r * math.sin(angle))


def d_bore():
    """The bore's outline: a circle with a flat, unrotated, as one loop."""
    r = shaft / 2.0
    if flat <= 0:
        return [polar(r, 360.0 * i / 48) for i in range(48)]
    a0 = math.acos((r - flat) / r)   # half the angle the flat subtends
    steps = 40
    return [polar(r, a0 + (360.0 - 2 * a0) * i / steps) for i in range(steps + 1)]


def star(r_out, rot):
    """The grip's outline: `teeth` ridges, alternating `r_out` and the groove."""
    n = 2 * teeth
    return [polar(r_out if i % 2 == 0 else r_out - depth, 360.0 * i / n + rot) for i in range(n)]


def ring(r, rot):
    """A circle drawn with the star's segment count, so the loft can pair it."""
    n = 2 * teeth
    return [polar(r, 360.0 * i / n + rot) for i in range(n)]


def section(z, loop):
    return sketch(XY.offset(z)).polygon(loop).polygon(d_bore())


# Enough grip sections that each turns by less than 0.45 of a ridge pitch, so
# construction-order pairing is also the nearest pairing.
pitch = 180.0 / teeth                            # segment angle of the star
steps = max(1, int(math.ceil(twist / (0.45 * pitch))))
# One loft through steps + 3 sections of 2 x teeth segments each: its cost
# grows faster than the face count, about a minute at 90 ridges and 45
# sections and six at the sliders' far corner (90 ridges, 120 degrees), which
# is past the evaluation budget. Refuse that up front rather than spend it.
assert_true(teeth * steps <= 4500,
            msg="%d ridges twisted %d degrees need %d grip sections of %d segments, too large a loft to build in the evaluation budget: fewer teeth or less twist" % (teeth, twist, steps + 1, 2 * teeth))
sections = []
if skirt > 0:
    sections.append(section(0, ring(radius - skirt * 0.8, 0)))
for i in range(steps + 1):
    sections.append(section(skirt + height * i / steps, star(radius, twist * i / steps)))
top = skirt + height
sections.append(section(top + cap, ring(radius - cap * 0.8, twist)))

knob = loft(sections, name="knob")
assert_eq(knob.kind, "brep", msg="a loft never leaves the exact kernel")

lo, hi = knob.bbox()
assert_true(math.close(hi[2], skirt + height + cap, tol=0.01), msg="the knob is skirt + grip + cap tall")
assert_true(math.close(lo[2], 0.0, tol=0.01), msg="the knob sits on z = 0")
assert_true(math.close(hi[0] - lo[0], 2 * radius, tol=0.05), msg="the ridges set the width")
core = math.pi * (radius - depth) * (radius - depth) * height
assert_true(knob.volume() > core, msg="the ridges add material to the core")

knob.material(preset="aluminum", roughness=0.22)

export(knob, "knurled_knob.step")
export(knob, "knurled_knob.stl")


# The twist slider's far end builds. The most ridges (teeth=90) at the
# default twist also builds, in about a minute, half the evaluation budget:
# too slow to run as a test.
def the_knob_is_a_solid():
    assert_solid(knob)


test("the most twist", the_knob_is_a_solid, sweep={"twist": [120]})
