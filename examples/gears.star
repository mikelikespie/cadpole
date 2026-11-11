# Four gears from the standard library: a spur pinion running inside a ring
# gear, a helical gear and a herringbone beside them.
#
# What this example demonstrates:
#
#   * `load("@cadpole/gears.star", ...)`: the library is built into the engine,
#     so this document ships no copy of it and evaluates the same in the CLI,
#     the browser and on a phone. Loading it builds nothing; every
#     gear below is a call;
#   * `involute_profile` as a Sketch you can add loops to -- the pinion gets
#     a keyed bore and lightening holes drawn onto the library's outline, then
#     one `extrude`, so it stays an exact B-rep with no boolean anywhere;
#   * `internal` (the ring: its tooth space is the external tooth of the same
#     base circle), `helical` (a smooth loft of the transverse profile turned
#     by `width tan(helix) / r`) and `herringbone` (two such halves glued on
#     their shared mid-plane), each one feature;
#   * the arithmetic: `centre_distance(m, z1, -z2)` for an internal mesh,
#     `tooth_thickness` and `min_teeth`, `layout` + `tooth_faces` to pick the
#     flanks of one tooth by name -- here to prove the pinion's tooth 0 points
#     at the ring's space 0, and to give tooth 0 its own colour.
#
#   cadpole render examples/gears.star --view product --lighting product --backdrop studio --floor gloss --tonemap neutral --edge-break 0.25 --denoise -o /tmp/gears.png
#   cadpole test examples/gears.star
#   cadpole export examples/gears.star --format step -o /tmp/gears.step

load(
    "@cadpole/gears.star",
    "centre_distance",
    "helical",
    "herringbone",
    "internal",
    "involute_profile",
    "layout",
    "min_teeth",
    "spur",
    "tooth_faces",
    "tooth_thickness",
)

module = param("module", 2.5, unit="mm", min=1, max=6, doc="Tooth size of the pinion and the ring: pitch diameter = module x teeth")
n_pinion = param("teeth_pinion", 17, min=10, max=30, doc="Teeth on the pinion")
n_ring = param("teeth_ring", 41, min=30, max=80, doc="Teeth on the ring gear (at least 10 more than the pinion)")
width = param("width", 8, unit="mm", min=3, max=30, doc="Face width of the pinion and the ring")
backlash = param("backlash", 0.1, unit="mm", min=0, max=0.5, doc="How much thinner than theory each tooth is at its pitch circle")
bore = param("bore", 8, unit="mm", min=4, max=20, doc="Pinion shaft diameter (wider than the keyway)")
key = param("key", 3, unit="mm", min=0, max=8, doc="Keyway width; 0 for a plain bore")
holes = param("lightening_holes", 5, min=0, max=12, doc="Holes in the pinion's web; 0 for a solid web")
helix = param("helix", 25, unit="deg", min=5, max=45, doc="Helix angle of the helical and herringbone gears")

assert_true(n_ring - n_pinion >= 10, msg="a ring needs at least ten more teeth than its pinion or the tips foul")


def polar(r, angle):
    return (r * math.cos(angle), r * math.sin(angle))


# --- The pinion: the library's outline plus a keyed bore and lightening holes.
lay = layout(module, n_pinion, backlash=backlash)
profile = involute_profile(module, n_pinion, backlash=backlash, name="pinion_profile")
r_bore = bore / 2.0
assert_true(key < bore, msg="the keyway is as wide as the bore or wider: make key smaller than bore, or 0 for a plain bore")
if key > 0:
    a0 = math.asin(key / 2.0 / r_bore)  # where the keyway's flat meets the circle
    steps = 48
    loop = [polar(r_bore, a0 + (360.0 - 2 * a0) * i / steps) for i in range(steps + 1)]
    loop.append((r_bore + key / 2.0, -key / 2.0))
    loop.append((r_bore + key / 2.0, key / 2.0))
    profile = profile.polygon(loop)
else:
    profile = profile.circle(r_bore)
hub = r_bore + key / 2.0 + 3.0
r_mid = (hub + lay["r_root"]) / 2.0
r_hole = min((lay["r_root"] - hub) / 2.0 - 2.5, r_mid * math.sin(180.0 / holes) - 2.5) if holes > 0 else 0
if holes > 0 and r_hole >= 1.5:
    for i in range(holes):
        profile = profile.circle(r_hole, center=polar(r_mid, 360.0 * i / holes + 180.0 / holes))
pinion = extrude(profile, width, name="pinion")

# --- The ring the pinion runs in. Its space 0 is on +x; the pinion's tooth 0
# points +x, so placed on +x at the internal centre distance they mesh.
ring = internal(module, n_ring, width, 4.0, backlash=backlash, name="ring")
a_ring = centre_distance(module, n_pinion, -n_ring)
pinion = pinion.translate((a_ring, 0, 0), name="pinion_placed")

# --- A helical gear and a herringbone beside the ring, as parts on a tray.
r_ring_outside = layout(module, n_ring, internal=True)["r_root"] + 4.0
x_helical = r_ring_outside + 28.0
hel = helical(2.0, 21, 12, helix, hand="right", bore=6, name="helical")
hel = hel.translate((x_helical, 0, 0), name="helical_placed")
x_herring = x_helical + 40.0
hb = herringbone(1.5, 14, 12, helix, bore=4, name="herringbone")
hb = hb.translate((x_herring, 0, 0), name="herringbone_placed")

# --- Names: tooth 0 of the pinion by its faces, coloured as a key.
faces0 = tooth_faces(pinion, lay, 0)
pinion.material(preset="brass", roughness=0.5)
paint(pinion, faces=faces0["flank_left"] | faces0["flank_right"] | faces0["tip"], color="#d0342c", name="pinion_marked")
ring.material(preset="steel", roughness=0.45)
hel.material(preset="aluminum", roughness=0.3)
hb.material(preset="pla_silk", color="#2f6fd0")

export([pinion, ring, hel, hb], "gears.step")
export([pinion, ring, hel, hb], "gears.stl")


# --- Tests (`cadpole test examples/gears.star`).
def every_gear_is_a_solid():
    for g in (pinion, ring, hel, hb):
        assert_solid(g)
        assert_eq(g.kind, "brep", msg="every gear here is one exact feature")


test("every gear is an exact solid", every_gear_is_a_solid)


def the_pinion_meets_the_ring():
    # Tooth 0 of the pinion reaches its tip circle on +x of its own centre;
    # the ring's tip circle is a root clearance further in, on the same axis.
    tip = faces0["tip"].one()
    assert_close(tip.centroid[1], 0.0, tol=1e-6, msg="the pinion's tooth 0 points at the ring's space 0")
    assert_close(math.hypot(tip.centroid[0] - a_ring, tip.centroid[1]), lay["r_tip"], tol=0.05)
    assert_close(tooth_thickness(module, backlash=backlash), math.pi * module / 2 - backlash)
    assert_true(n_pinion >= min_teeth() or lay["undercut"], msg="a pinion under 17 teeth gets the undercut trochoid")
    lo, hi = ring.bbox()
    assert_close(hi[0], r_ring_outside, tol=1e-6)
    plo, phi = pinion.bbox()
    assert_true(phi[0] < layout(module, n_ring, internal=True)["r_root"], msg="the pinion's tips stay inside the ring's root circle")
    assert_true(plo[0] > 0, msg="and clear of the ring's far side")


test("the pinion sits inside the ring, tooth to space, tips clear", the_pinion_meets_the_ring)


def helical_and_herringbone():
    lo, hi = hel.bbox()
    assert_close(hi[2] - lo[2], 12, tol=1e-6)
    assert_true(hi[0] - lo[0] > 2.0 * 21 / math.cos(helix), msg="a helical gear's pitch diameter is m z / cos(helix)")
    hlo, hhi = hb.bbox()
    assert_close((hlo[0] + hhi[0]) / 2, x_herring, tol=0.05, msg="the herringbone is centred where it was put")
    assert_true(len(hb.faces(name="herringbone_lower/side[0.*]")) == len(hb.faces(name="herringbone_upper/side[0.*]")), msg="two mirror halves")


test("the helical and herringbone gears have their widths and their halves", helical_and_herringbone)


def volume():
    assert_solid(pinion)
    return pinion.volume()


test("a bigger pinion is a bigger part", volume, sweep={"teeth_pinion": [12, 17, 24]}, monotone="increasing")


# The smallest bore still takes the default keyway.
def the_pinion_is_solid():
    assert_solid(pinion)


test("the smallest bore takes the keyway", the_pinion_is_solid, sweep={"bore": [4]})
