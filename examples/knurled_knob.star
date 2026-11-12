# A control knob with a helical knurled grip, a skirt, a blind D-shaft bore
# and an anodised cap with a pointer line. Exact B-rep throughout, so it
# exports STEP.
#
#   * The body is one `loft` through rotated sections: each ridge is a helix,
#     with a flat land on its crest and a V groove between ridges.
#   * The skirt, the shoulder and the radiused top are sections of the same
#     loft, drawn as rings of arcs, so they come out as exact cones and
#     cylinders. The ridges fade in above the shoulder and run out into the
#     top radius instead of ending in a step.
#   * The D bore is a hole carried through every section. The cap sits on the
#     top face and closes it, so the shaft hole is blind from the outside.
#   * The pointer is cut into the cap's top with `emboss` and painted.
#
#   cadpole render examples/knurled_knob.star --backend path --environment -o knob.png
#   cadpole export examples/knurled_knob.star --format step -o knob.step

radius = param("radius", 14, unit="mm", min=6, max=40, doc="Radius over the ridges")
height = param("height", 13, unit="mm", min=6, max=60, doc="Height of the knurled grip")
teeth = param("teeth", 30, min=8, max=36, doc="Ridges round the grip")
depth = param("depth", 0.9, unit="mm", min=0.3, max=3, doc="Depth of the grooves")
twist = param("twist", 30, unit="deg", min=0, max=60, doc="How far a ridge turns over the grip's height")
skirt = param("skirt", 3, unit="mm", min=0, max=10, doc="Height of the skirt under the grip; 0 for none")
cap = param("cap", 3, unit="mm", min=1, max=10, doc="Radius rounding the top of the grip")
shaft = param("shaft", 6, unit="mm", min=2, max=20, doc="Shaft diameter")
flat = param("flat", 1.5, unit="mm", min=0, max=5, doc="Depth of the D flat on the shaft; 0 for a round bore")
pointer = param("pointer", -150, unit="deg", min=-180, max=180, doc="Direction of the pointer line")

assert_true(depth < radius / 3, msg="grooves that deep leave no core")
assert_true(shaft / 2.0 + 2.0 < radius - depth, msg="the bore needs 2 mm of wall around it")
assert_true(flat < shaft / 2.0, msg="the D flat must leave part of the round bore")
assert_true(radius - cap > shaft / 2.0 + 3.5, msg="a top radius that large leaves no room for the cap over the bore")

FLARE = 1.5          # how far the skirt stands out past the ridges
LAND = 0.3           # the share of a ridge pitch that is flat crest
INSERT_H = 1.6       # the cap insert's thickness
FADE = 1.2           # the height over which the ridges grow in above the shoulder


def polar(r, angle):
    return (r * math.cos(angle), r * math.sin(angle))


def with_bore(s):
    """Adds the bore to a section: a circle with a flat on +x, unrotated, as
    one arc and one line, so its walls are an exact cylinder and a plane."""
    r = shaft / 2.0
    if flat <= 0:
        return s.circle(r)
    a0 = math.acos((r - flat) / r)   # half the angle the flat subtends
    return s.add(path(polar(r, a0)).arc(polar(r, -a0), via=(-r, 0)).line(polar(r, a0)))


pitch = 360.0 / teeth


def angles(rot):
    """The vertex angles of every section: a crest land, then the groove root."""
    out = []
    for k in range(teeth):
        base = k * pitch + rot
        out += [base - LAND * pitch / 2, base + LAND * pitch / 2, base + pitch / 2]
    return out


def star(r_out, d, rot):
    """The grip's outline: lands at `r_out`, roots `d` deeper."""
    return [polar(r_out if i % 3 != 2 else r_out - d, a) for i, a in enumerate(angles(rot))]


def ring(r, rot):
    """A true circle drawn as arcs between the star's vertex angles, so the
    loft pairs it segment for segment and every band to it is exact."""
    a = angles(rot)
    n = len(a)
    p = path(polar(r, a[0]))
    for i in range(n):
        a1 = a[(i + 1) % n] + (360.0 if i + 1 == n else 0.0)
        p = p.arc(polar(r, a[(i + 1) % n]), via=polar(r, (a[i] + a1) / 2))
    return p


def section(z, outline):
    s = sketch(XY.offset(z))
    s = s.add(outline) if type(outline) != "list" else s.polygon(outline)
    return with_bore(s)


# Each grip section turns by under 0.15 of a ridge pitch, half the narrowest
# segment, so construction-order pairing is also the nearest pairing.
z0 = skirt
z1 = skirt + height
rate = twist / height                             # degrees per mm up the grip
steps = max(4, int(math.ceil(twist / (0.15 * pitch))))
ARC = 6                                           # sections round the top radius
count = steps + 1 + ARC + (4 if skirt > 0 else 0)
# The loft's cost grows faster than its face count; past about 4200 side
# faces it no longer builds in an interactive time.
assert_true(3 * teeth * count <= 4200,
            msg="%d ridges twisted %d degrees need %d sections of %d segments: fewer teeth or less twist" % (teeth, twist, count, 3 * teeth))

sections = []
if skirt > 0:
    flange = radius + FLARE
    e = min(0.4, skirt / 4.0)                     # the edge breaks
    sections.append(section(0, ring(flange - e, 0)))
    sections.append(section(e, ring(flange, 0)))
    sections.append(section(skirt - 2 * e, ring(flange, 0)))
    sections.append(section(skirt - e, ring(flange - 0.4 * e, 0)))
sections.append(section(z0, ring(radius, 0)))
for i in range(1, steps + 1):
    z = z0 + height * i / steps
    grow = min(1.0, (z - z0) / FADE)
    sections.append(section(z, star(radius, depth * (3 * grow * grow - 2 * grow * grow * grow), rate * (z - z0))))
# The top radius: a quarter circle of radius `cap`, the ridges running out
# over the first part of it and the last sections plain circles. The runout
# sections stay at the grip's last turn, so each ridge leaves the helix and
# runs straight up into the radius, the way a knurl meets a turned edge.
for j in range(1, ARC + 1):
    t = 90.0 * j / ARC
    z = z1 + cap * math.sin(t)
    r = radius - cap * (1 - math.cos(t))
    fade = max(0.0, 1.0 - 1.6 * j / ARC)
    if fade > 0:
        sections.append(section(z, star(r, depth * fade, twist)))
    else:
        sections.append(section(z, ring(r, twist)))

knob = loft(sections, name="knob")
assert_eq(knob.kind, "brep", msg="a loft never leaves the exact kernel")
top = z1 + cap
top_r = radius - cap

# The cap: an anodised disc with a broken edge, standing on the top face and
# closing the bore.
ins_r = top_r - 1.2
insert = loft([sketch(XY.offset(top)).circle(ins_r),
               sketch(XY.offset(top + INSERT_H - 0.4)).circle(ins_r),
               sketch(XY.offset(top + INSERT_H)).circle(ins_r - 0.4)], name="cap_disc")
line = sketch(XY.offset(top + INSERT_H)).add(
    path(polar(0.4 * ins_r, pointer)).line(polar(ins_r - 1.4, pointer)), close=False)
insert = emboss(insert, insert.faces(kind="plane")[-1], line, 0.3,
                kind=emboss.scribe, width=1.0, color="#f4f1e8", name="cap")

knob.material(preset="aluminum", roughness=0.3, edge_break=0.15)
insert.material(preset="aluminum", color="#2bb3a3", roughness=0.35, texture="turned", scale=0.2)

export([knob, insert], "knurled_knob.step")
export([knob, insert], "knurled_knob.stl")


def the_knob_is_one_solid():
    assert_solid(knob)
    assert_solid(insert)


def the_cap_sits_on_the_top_and_closes_the_bore():
    lo, hi = knob.bbox()
    ilo, ihi = insert.bbox()
    assert_close(lo[2], 0.0, tol=0.01, msg="the knob sits on z = 0")
    assert_close(hi[2], top, tol=0.01, msg="the knob is skirt + grip + radius tall")
    assert_close(ilo[2], top, tol=0.01, msg="the cap stands on the top face, neither sunk into it nor floating")
    assert_close(ihi[2], top + INSERT_H, tol=0.01)
    assert_true(ins_r < top_r - 0.5, msg="the cap leaves a ring of the top face showing")
    assert_true(ins_r > shaft / 2.0 + 1.0, msg="the cap covers the bore with a margin")


def the_skirt_stands_out_past_the_ridges():
    lo, hi = knob.bbox()
    want = 2 * (radius + FLARE) if skirt > 0 else 2 * radius
    assert_close(hi[0] - lo[0], want, tol=0.05)


def the_d_flat_reads():
    # A 6 mm shaft with a 1.5 mm flat measures 4.5 mm across the flat: a
    # plane facing into the bore at x = r - flat, as wide as the flat's chord
    # and as tall as the knob.
    r = shaft / 2.0
    flats = [f for f in knob.faces(kind="plane") if f.normal[0] < -0.99 and math.close(f.centroid[0], r - flat, tol=0.01)]
    assert_true(len(flats) > 0, msg="the bore has its D flat")
    chord = 2 * math.sqrt(r * r - (r - flat) * (r - flat))
    area = 0.0
    for f in flats:
        area += f.area
    assert_close(area, chord * top, rel=0.01, msg="the flat runs the bore's full length")


def the_pointer_is_cut_and_painted():
    assert_true(len(insert.faces(name="cap/end")) > 0, msg="the pointer groove has a floor")


test("the knob", the_knob_is_one_solid)
test("the cap", the_cap_sits_on_the_top_and_closes_the_bore)
test("the skirt", the_skirt_stands_out_past_the_ridges)
test("the D flat", the_d_flat_reads)
test("the pointer", the_pointer_is_cut_and_painted)
# The far end of the twist slider, and the plain end of every slider at once:
# few ridges, no twist, no skirt, a round bore.
test("the most twist", the_knob_is_one_solid, sweep={"twist": [60]})
test("the plainest knob", the_knob_is_one_solid, sweep={"teeth": [8], "twist": [0], "skirt": [0], "flat": [0]})
