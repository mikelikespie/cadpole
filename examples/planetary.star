# A print-in-place herringbone planetary: a sun, three planets, a ring and
# the carrier that captures the planets on pins, exported as ONE 3MF of six
# bodies that never touch. Printed as it stands here it comes off the bed
# already assembled, and it turns.
#
# Why herringbone: two helical halves of opposite hand make every tooth a V,
# so the sun cannot slide out of the planets and the planets cannot leave the
# ring axially. That is what makes the set print-in-place with no top plate:
# the carrier is a bottom plate with three pins, each pin ending in a 40°
# cone head over its planet, and the only unsupported faces in the whole
# print are those cones, which a printer bridges on its own.
#
# The gears come from `@cadpole/gears.star`: `herringbone` for the sun
# and the planets, `internal(herringbone=True)` for the ring, `planetary()`
# for the ring's tooth count, the assembly condition, the ratios and the
# phase of every planet. Every clearance below is *measured*, with
# `distance()` and `clearance_map()`, never assumed; the tests are
# the fit check, and `cadpole check` judges the 3MF as one print.
#
# Which gaps are which, and why (0.4 mm nozzle, 0.2 mm layers):
#
#   horizontal running gaps, `gap` (0.35 mm), the `fuse` rule's business:
#     * tooth flanks: every gear is thinned at its pitch circle so that the
#       flanks of each centred mesh stand `gap` apart along their common
#       normal -- `b = gap / (cos alpha_t cos beta_b)` per gear;
#     * the ring is not thinned but shifted: a profile shift of
#       `x_r = b / (2 m_t tan alpha_n)` widens its spaces by the same `b`
#       and lifts its tip by `x_r m`, so the ring's tip corner no longer
#       passes below the planet's base circle and through its root fillet,
#       which would be tighter than the flanks;
#     * root clearance: the coefficient `c` is raised so that every tip
#       stands `gap` from the root land it faces, not the textbook 0.25 m;
#     * pin in bore: `bore_clearance` radially, a running fit;
#     * carrier rim to ring tips: 0.5 mm -- nothing meshes there.
#   vertical gaps, whole layers, the `layer_gap` rule's business:
#     * the sun and the planets float two layers (0.4 mm) above the carrier
#       plate -- never 0.3, which the slicer would round to 0.2 or 0.4 at
#       its whim; the layer rule is what says so;
#     * the pin heads start two layers above the planets' top faces.
#   elephant's foot: the carrier plate's first two layers are inset 0.3 mm,
#   so its rim stands 0.65 mm from the ring's teeth where layer 1 squashes;
#   the ring's own foot points outward and into its spaces, harmless.
#
# The chord contract. `distance()` measures triangles, and a
# chord sits inside its true surface by up to the tessellation tolerance, so
# the true gap lies within twice `.tolerance` of the measured one. At the
# document's 0.05 mm that band is 0.1 mm -- a third of a printed backlash --
# so every body here carries `tessellation(tolerance=0.01)`: the band is
# about 0.02 mm (a twisted band can exceed its bound a little), and the
# tests can prove `gap - 2 tol >= printer.min_gap` (0.3 mm) on a
# 0.35 mm gap. The same tessellation is what the 3MF carries. The library's
# outline is thinned to 0.02 mm in the transverse plane (`tolerance=` of the
# gears), which is a modelling error of the flank itself, not a chord error
# of the mesh, and is inside the 0.05 mm the slicer's own path planning adds.
#
# What the checks say, and what they cannot:
#   * `cadpole check examples/planetary.star`: no `fuse`, no `layer_gap`, no
#     `bridge`, no `chord_error`, no `overhang`. It does report `thickness`
#     on every gear: the wall-thickness rule casts rays inside the solid and
#     reads the acute corner where a helical flank meets the gear's end face
#     (90° - beta_b = 62°) as a 0.4 mm wall. That is a corner across layers,
#     not a wall in the layer plane, where the tooth tip land is 1.15 mm
#     wide -- so `assert_printable` below runs with `min_wall=0` and the
#     in-plane tooth thickness is asserted from the closed form instead.
#   * The mesh *in motion*: the six bodies are parts placed as
#     instances in one `assembly`, and `sun_angle` poses the mechanism --
#     the sun turns, each planet spins back by z_s/z_p about its own pin and
#     the ring turns by z_s/z_r, the carrier held -- so a sweep of that one
#     parameter through a tooth pitch re-measures every running gap at each
#     angle. `explode=` is a view offset on the assembly, never geometry:
#     every test measures the assembled position whatever the picture shows.
#   The STEP export carries the exact flanks (every body here is a B-rep:
#   lofts, a glue, an exact union), the 3MF the 0.01 mm tessellation.
#
#   cadpole test examples/planetary.star
#   cadpole check examples/planetary.star
#   cadpole export examples/planetary.star --run-exports --out-dir /tmp
#   cadpole render examples/planetary.star --backend path --samples 256 --size 1600x1200 --view product --lighting product --backdrop studio --floor gloss --tonemap neutral --denoise -o docs/demo/planetary.png
#   cadpole render examples/planetary.star --backend path --samples 256 --size 1600x1200 --view product --lighting product --backdrop studio --floor gloss --tonemap neutral --denoise --param explode=16 -o docs/demo/planetary_exploded.png
#   cadpole sweep examples/planetary.star --param sun_angle=0,10,20,30 -o /tmp/turn.png
#   cadpole animate examples/planetary.star --view product --lighting product --backdrop studio --tonemap neutral --no-grid --no-edges -o /tmp/planetary.gif

load(
    "@cadpole/gears.star",
    "base_diameter",
    "centre_distance",
    "pitch_diameter",
    "planetary_set",
    "tip_diameter",
    "tooth_thickness",
)

# --- Parameters.
module = param("module", 2.0, unit="mm", min=1.5, max=3, doc="Normal module: tooth size as the printer sees it. Below 2 mm the thinned tooth tips fall under two perimeters")
teeth_sun = param("teeth_sun", 12, min=8, max=24, doc="Teeth on the sun; the ring has teeth_sun + 2 teeth_planet")
teeth_planet = param("teeth_planet", 12, min=8, max=24, doc="Teeth on each planet")
planets = param("planets", 3, min=1, max=6, doc="Number of planets: (teeth_sun + teeth_ring) must divide by it, or planetary() refuses")
helix = param("helix", 30, unit="deg", min=15, max=40, doc="Helix angle of every herringbone half; 30° keeps a 12-tooth gear clear of undercut")
width = param("width", 12, unit="mm", min=6, max=24, doc="Face width of every gear, the V apex at half of it")
gap = param("gap", 0.35, unit="mm", min=0.2, max=0.5, doc="The running gap every flank, tip and rim stands from its neighbour; what the printer needs so nothing fuses")
layer = param("layer", 0.2, unit="mm", min=0.1, max=0.3, doc="Layer height the vertical gaps are a whole number of")
bore_clearance = param("bore_clearance", 0.35, unit="mm", min=0.2, max=0.6, doc="Radial gap between a carrier pin and its planet's bore")
pin = param("pin", 8, unit="mm", min=6, max=12, doc="Carrier pin diameter")
plate = param("plate", 3, unit="mm", min=2, max=6, doc="Carrier plate thickness")
rim = param("rim", 4, unit="mm", min=2, max=10, doc="Ring wall outside the root circle")
chord = param("chord", 0.01, unit="mm", min=0.002, max=0.05, doc="Tessellation tolerance of every body: what the 3MF and every measurement below carry")
lift = param("explode", 0.0, unit="mm", min=0, max=60, doc="Exploded view: the carrier stays, the ring lifts this much, the planets twice it, the sun three times; a picture only, the fit tests measure the assembled position")
sun_angle = param("sun_angle", 0.0, unit="deg", min=0, max=360, doc="Turn the sun by this; the planets spin back and the ring turns with it, the carrier held, so the set stays in mesh and the tests re-measure every gap (through a tooth pitch); the animation takes it round once")

PRINTER = printer("bambu_a1")
MIN_GAP = PRINTER.min_gap  # 0.3 mm: what survives an extrusion's width and a bore's shrink
NOZZLE = PRINTER.nozzle
MAX_MESH_GAP = 0.6  # a flank gap over this and the teeth no longer share a line of action

# --- The set. `planetary_set` is this construction, in the library, and
# `examples/gear_toy.star` builds on the same implementation:
#   * the arithmetic: each external gear thinned by `b = gap / (cos alpha_t
#     cos beta_b)`; the ring shifted by `x_r = b / (2 m_t tan alpha_n)` and
#     thinned by `ring_trim` (0.02: the internal mesh stands slightly closer
#     than the external one at the same numbers -- the loft's interpolation
#     between its sections); the root clearance coefficient
#     `max(0.25, gap / module)` so tips stand `gap` from root lands;
#   * the sun: a right-hand herringbone with a hex boss glued on its top
#     face (the boss's hexagon lies strictly inside the top face and becomes
#     a hole in it: no boolean), then placed as an instance. Glued first,
#     moved second, so the face names the tests select the sun's flanks by
#     are the gear's own;
#   * the planets: one left-hand herringbone with a bore, `planets`
#     instances of it, each turned to the phase planetary() gives about its
#     own axis and then moved onto its pin, so it meshes with the sun and
#     the ring at once;
#   * the ring: a left-hand herringbone ring standing on the bed. Its section
#     at the height where the gears start must be the un-turned outline
#     planetary()'s phases assume; a left-hand ring has turned clockwise by
#     z tan(beta) / r by then, so it is turned back by as much;
#   * the carrier: a plate with a hex socket through it, its first two layers
#     inset against elephant's foot, and three pins, each a cylinder that
#     flares into a self-supporting head over its planet. The socket is a
#     loop of the plate's sections; the pins reach 0.5 mm into the plate and
#     are united on the exact kernel (a cylinder through a plane), not set
#     flush and glued.
# Every number the tests below use comes back in the dict.
RING_TRIM = 0.02
RIM_GAP = 0.5  # the plate turns inside the ring's tips; nothing meshes there, so it takes the widest gap that still hides the pins' feet
layers_up = 2  # the vertical gaps, in layers
head_rise = 1.2  # the head cone grows by 1.0 over this: 39.8°, under the 45° overhang rule with the facets' margin
head_height = 1.5
S = planetary_set(
    module,
    teeth_sun,
    teeth_planet,
    width,
    planets=planets,
    helix_angle=helix,
    gap=gap,
    layer=layer,
    layers_up=layers_up,
    bore_clearance=bore_clearance,
    pin=pin,
    plate=plate,
    rim=rim,
    rim_gap=RIM_GAP,
    socket=3.5,  # a 6 mm hex key in the carrier
    head_rise=head_rise,
    head_height=head_height,
    ring_trim=RING_TRIM,
    tolerance=chord,
)
P = S["P"]
teeth_ring = S["teeth_ring"]
a = S["a"]  # sun to planet, transverse: m (z_s + z_p) / (2 cos beta)
alpha_t = S["alpha_t"]  # transverse pressure angle
backlash = S["backlash"]  # each gear thinned by this at its pitch circle
lay_sun, lay_planet, lay_ring = S["lay_sun"], S["lay_planet"], S["lay_ring"]
z_gears = S["z_gears"]  # the gears float this high over the carrier plate
ring_height = S["ring_height"]  # the ring's apex at the planets' mid-plane, its bottom on the bed
r_plate = S["r_plate"]
pin_height = S["pin_height"]  # plate top to two layers over the planets
sun = S["sun"]
planet_bodies = S["planets"]
ring = S["ring"]
carrier = S["carrier"]
sun.material(preset="pla_silk", color="#d9a441")
for g in planet_bodies:
    g.material(preset="pla_silk", color="#2f8f8f")
ring.material(preset="pla_silk", color="#3b3f46")
carrier.material(preset="pla_silk", color="#c8552b")

# --- The assembly. Nothing moves here: it groups the six instances
# so the view can explode them as one, and so `export` and the tests can
# take the set by name. `explode` is a view offset ranked along z -- the
# carrier stays, the ring (the larger part at the planets' height) lifts by
# `explode`, the planets by twice it, the sun by three times -- and it never
# reaches the geometry, the exports or a measurement.
gearbox = assembly(S["instances"], name="gearbox")
explode(gearbox, lift, along="z")

# --- The pose: kinematics-lite. With the carrier held, turning the sun by
# `sun_angle` spins each planet back by z_s/z_p about its own pin and turns
# the ring by z_s/z_r the other way; every gap below is then measured in
# that position, and the pitch sweep at the end walks it through a tooth.
if sun_angle:
    pose(sun, rotate=sun_angle)
    for k in range(planets):
        pose(planet_bodies[k], rotate=-sun_angle * teeth_sun / teeth_planet)
    pose(ring, rotate=-sun_angle * teeth_sun / teeth_ring)

bodies = gearbox.bodies
export(gearbox, "planetary.3mf")
export(gearbox, "planetary.step")

# --- The animation: the sun through one turn at constant speed, the
# planets and the ring following the pose above, in four seconds, looping.
# `cadpole animate examples/planetary.star -o /tmp/planetary.gif`. Every frame
# after the first is a pose: zero kernel ops, zero tessellations.
animation("turn", [keyframe(0, sun_angle=0, ease=ease.linear), keyframe(4, sun_angle=360)], fps=30, loop=True)


# --- Tests (`cadpole test examples/planetary.star`). Every one measures.
def part_of(face, lay):
    """Which part of a tooth a gear face is -- root, fillet_right, flank_right,
    tip, flank_left, fillet_left -- from its outline segment (the names survive
    the loft, the glue and every transform); any other face keeps its name."""
    i = face.find("side[0.")
    if i < 0:
        return face
    j = face.find("]", i)
    s = int(face[i + 7:j]) % lay["segments"]
    for part, ranges in lay["parts"].items():
        for lo, hi in ranges:
            if lo <= s and s < hi:
                return part
    return face


def running_gap(c, what, hi=MAX_MESH_GAP):
    """The horizontal running gap `c` (a Clearance) is one the printer can
    make and the mechanism can use: no shared material, `gap - 2 tol` at
    least the printer's fuse threshold, and no wider than `hi`."""
    assert_true(not c.overlap, msg="%s: %s and %s share material at %s" % (what, c.face_a, c.face_b, c.at))
    assert_true(c.distance - 2 * c.tolerance >= MIN_GAP, msg="%s: %s mm between %s and %s at %s, measured at ±%s mm, cannot prove the %s mm fuse threshold" % (what, math.round(c.distance, 4), c.face_a, c.face_b, c.at, c.tolerance, MIN_GAP))
    assert_true(c.distance <= hi, msg="%s: %s mm between %s and %s is too loose to run" % (what, math.round(c.distance, 4), c.face_a, c.face_b))
    return c.distance


def flank_gap(a_body, b_body, lay_a, lay_b):
    """The closest flank-to-flank region of a mesh, from the clearance map."""
    best = None
    for d, pa, pb, fa, fb in clearance_map(a_body, b_body, below=MAX_MESH_GAP):
        if part_of(fa, lay_a).startswith("flank") and part_of(fb, lay_b).startswith("flank"):
            best = d if best == None else min(best, d)
    assert_true(best != None, msg="no flank of %s faces a flank of %s within %s mm" % (a_body.name, b_body.name, MAX_MESH_GAP))
    return best


def the_set_assembles():
    assert_eq(teeth_ring, teeth_sun + 2 * teeth_planet)
    assert_eq((teeth_sun + teeth_ring) % planets, 0, msg="the planets could not be spaced equally")
    assert_close(a, centre_distance(module, teeth_sun, teeth_planet, helix_angle=helix))
    assert_close(a, centre_distance(module, teeth_planet, -teeth_ring, helix_angle=helix), msg="the ring runs on the same centre distance as the sun")
    assert_close(P["ratio_fixed_ring"], 1.0 + teeth_ring / teeth_sun, msg="sun in, carrier out, ring held")
    assert_close(P["ratio_fixed_carrier"], -teeth_ring / teeth_sun, msg="sun in, ring out, carrier held: reversed")
    assert_close(P["ratio_fixed_sun"], 1.0 + teeth_sun / teeth_ring, msg="ring in, carrier out, sun held")
    assert_close(P["ratio_fixed_ring"] - 1.0, -P["ratio_fixed_carrier"], msg="the two reductions differ by exactly one")
    assert_true(not lay_sun["undercut"] and not lay_planet["undercut"], msg="no gear here is undercut")
    return P["ratio_fixed_ring"]


test("the set assembles and its ratios are the closed forms", the_set_assembles)


def every_body_is_a_solid():
    for b in bodies:
        assert_solid(b)
        assert_eq(b.kind, "brep", msg="%s is exact: lofts, a glue and an exact union, nothing went to the mesh kernel" % b.name)
    lo, hi = ring.body().bbox()
    assert_close(lo[2], 0.0, tol=1e-6, msg="the ring stands on the bed")
    assert_close(hi[2], ring_height, tol=1e-6)
    lo, hi = carrier.body().bbox()
    assert_close(lo[2], 0.0, tol=1e-6, msg="the carrier stands on the bed")
    assert_close(hi[2], plate + pin_height + head_rise + head_height, tol=1e-6)
    assert_close(hi[0], r_plate, tol=0.05, msg="the plate reaches its radius")
    lo, hi = sun.body().bbox()
    assert_close(lo[2], z_gears, tol=1e-6, msg="the sun floats two layers over the plate")
    assert_close(hi[2], z_gears + width + 6.0, tol=1e-6, msg="the boss stands on the sun")


test("every body is an exact solid where the stack puts it", every_body_is_a_solid)


def sun_and_planets():
    worst = MAX_MESH_GAP
    for k in range(planets):
        c = distance(sun, planet_bodies[k])
        worst = min(worst, running_gap(c, "sun to planet %d" % k))
        f = flank_gap(sun, planet_bodies[k], lay_sun, lay_planet)
        assert_close(f, gap, tol=0.03, msg="planet %d: the sun's flanks stand %s mm from its, not the %s asked for" % (k, math.round(f, 4), gap))
    return worst


test("every planet meshes with the sun at the gap asked for", sun_and_planets)


def planets_and_ring():
    worst = MAX_MESH_GAP
    for k in range(planets):
        c = distance(planet_bodies[k], ring)
        worst = min(worst, running_gap(c, "planet %d to the ring" % k))
        f = flank_gap(planet_bodies[k], ring, lay_planet, lay_ring)
        assert_close(f, gap, tol=0.03, msg="planet %d: the ring's flanks stand %s mm from its, not the %s asked for" % (k, math.round(f, 4), gap))
        # The shift lifted the ring's tip corner clear of the planet's fillet: the
        # tightest place in the mesh is a flank, not the fillet passage.
        assert_true(part_of(c.face_a, lay_planet).startswith("flank") or part_of(c.face_b, lay_ring).startswith("flank"), msg="planet %d: the ring is closest at %s/%s, not on a flank" % (k, part_of(c.face_a, lay_planet), part_of(c.face_b, lay_ring)))
    return worst


test("every planet meshes with the ring at the gap asked for", planets_and_ring)


def planets_and_carrier():
    worst = 9.0
    for k in range(planets):
        g = planet_bodies[k]
        # The closest approach is the pin in its bore, a horizontal running
        # fit; the two-layer float over the plate and the head over the top
        # are read off the map by their faces.
        c = distance(g, carrier)
        running_gap(c, "planet %d on its pin" % k)
        assert_true("pin_" in c.face_b and "side[1." in c.face_a, msg="planet %d: the closest approach to the carrier is %s/%s, not the pin in the bore" % (k, c.face_a, c.face_b))
        pin_gap, float_gap, head_gap = None, None, None
        for d, pa, pb, fa, fb in clearance_map(g, carrier, below=1.0, spacing=2.0, limit=512):
            if "pin_" in fb and "side[1." in fa:
                pin_gap = d if pin_gap == None else min(pin_gap, d)
            elif "plate" in fb and fa.endswith("/start"):
                float_gap = d if float_gap == None else min(float_gap, d)
            elif "pin_" in fb and fa.endswith("/end"):
                head_gap = d if head_gap == None else min(head_gap, d)
        assert_true(pin_gap != None, msg="planet %d: no pin faces its bore within 1 mm" % k)
        assert_true(pin_gap - 2 * c.tolerance >= MIN_GAP, msg="planet %d: the pin runs %s mm from the bore, measured at ±%s" % (k, math.round(pin_gap, 4), c.tolerance))
        assert_true(pin_gap <= MAX_MESH_GAP, msg="planet %d: the pin rattles %s mm in its bore" % (k, math.round(pin_gap, 4)))
        assert_close(float_gap, layers_up * layer, tol=1e-3, msg="planet %d floats %s mm over the plate, not %s layers" % (k, float_gap, layers_up))
        assert_true(head_gap != None and head_gap - 2 * c.tolerance >= MIN_GAP, msg="planet %d: the pin head is %s mm from the planet's top" % (k, head_gap))
        worst = min(worst, pin_gap)
    return worst


test("each planet runs on its pin: the bore gap, the two-layer float, the head over it", planets_and_carrier)


def carrier_ring_and_sun():
    c = distance(carrier, ring)
    running_gap(c, "the carrier's rim to the ring's teeth")
    assert_true("plate" in c.face_a, msg="the carrier meets the ring at %s, not at its plate" % c.face_a)
    s = distance(sun, carrier)
    assert_true(not s.overlap)
    assert_close(s.distance, layers_up * layer, tol=1e-3, msg="the sun floats %s mm over the carrier at %s/%s" % (math.round(s.distance, 4), s.face_a, s.face_b))
    return c.distance


test("the carrier clears the ring's teeth and floats the sun two layers up", carrier_ring_and_sun)


def the_map_is_sane():
    regions = clearance_map(sun, planet_bodies[0], below=MAX_MESH_GAP, spacing=1.0)
    assert_true(len(regions) >= 6, msg="a meshing pair has several teeth in reach; %d regions" % len(regions))
    ds = [d for d, pa, pb, fa, fb in regions]
    assert_true(all([ds[i] <= ds[i + 1] for i in range(len(ds) - 1)]), msg="closest first")
    assert_close(ds[0], distance(sun, planet_bodies[0]).distance, tol=1e-9, msg="the map's first entry is distance()")
    assert_true(all([d - 2 * chord >= MIN_GAP for d in ds]), msg="every region of the sun-planet map is above the fuse threshold: %s" % ds)
    parts = {}
    for d, pa, pb, fa, fb in regions:
        parts[(part_of(fa, lay_sun), part_of(fb, lay_planet))] = True
    assert_true(("flank_left", "flank_left") in parts and ("flank_right", "flank_right") in parts, msg="both flanks of the mesh are in reach: %s" % parts.keys())
    return len(regions)


test("the sun-planet clearance map: several teeth, both flanks, closest first, all above the threshold", the_map_is_sane)


def inv(angle):
    return math.tan(angle) - math.radians(angle)


def tip_land(teeth):
    """The transverse tooth thickness on the tip circle, `d_a (s / d + inv alpha_t - inv alpha_a)`:
    the width of the tip land in the layer plane, which is the wall a slicer prints."""
    d = pitch_diameter(module, teeth, helix_angle=helix)
    d_a = tip_diameter(module, teeth, helix_angle=helix)
    d_b = base_diameter(module, teeth, helix_angle=helix)
    s = tooth_thickness(module, backlash=backlash, helix_angle=helix)
    alpha_a = math.acos(d_b / d_a)
    return d_a * (s / d + inv(alpha_t) - inv(alpha_a))


def thin_spot_height(body):
    """Where the wall-thickness rule found `body` thinnest: the z of the spot
    its warning names, or None when it found nothing under two perimeters."""
    for w in print_check(body, printer=PRINTER).warnings:
        if w.startswith("wall is"):
            i = w.find("at (")
            j = w.find(")", i)
            return float(w[i + 4:j].split(",")[2].strip())
    return None


def printable_as_one_print():
    # The six bodies as one print: solid each, no fuse, every horizontal gap a
    # whole number of layers, nothing in the air, no chord error over a third
    # of a gap. The wall rule is answered separately, below: it reads the
    # acute corner where a helical flank meets a gear's end face as a wall
    # (header), so this call runs with a min_wall under anything it can find
    # (0 would mean the default), and the next two assertions say what the
    # rule would have said if it knew a corner from a wall.
    assert_printable(bodies, printer=PRINTER, min_gap=MIN_GAP, layer_height=layer, min_wall=0.05)
    # Every gear's thinnest reading sits on one of its end faces -- the corner --
    # and the carrier has no thin wall at all.
    for b, ends in ((sun, (z_gears, z_gears + width)), (planet_bodies[0], (z_gears, z_gears + width)), (ring, (0.0, ring_height))):
        z = thin_spot_height(b)
        assert_true(z != None and min([abs(z - e) for e in ends]) < layer, msg="%s: the thinnest reading is at z = %s, not on an end face %s" % (b.name, z, ends))
    assert_true(thin_spot_height(carrier) == None, msg="the carrier has a wall under two perimeters")
    # And in the layer plane the tooth tips are two perimeters wide.
    for teeth, what in ((teeth_sun, "sun"), (teeth_planet, "planet")):
        assert_true(tip_land(teeth) >= 2 * NOZZLE, msg="the %s's tip land is %s mm in the layer plane, under two perimeters" % (what, math.round(tip_land(teeth), 3)))
    return tip_land(teeth_sun)


test("the six bodies are one printable print; the wall rule's thin spots are tooth-end corners; every tip is two perimeters wide", printable_as_one_print)


def assembles_at_this_point():
    worst = 9.0
    for b in bodies:
        assert_solid(b)
    for k in range(planets):
        worst = min(worst, running_gap(distance(sun, planet_bodies[k]), "sun to planet %d" % k))
        worst = min(worst, running_gap(distance(planet_bodies[k], ring), "planet %d to the ring" % k))
    worst = min(worst, running_gap(distance(carrier, ring), "carrier to ring"))
    return worst


test("the set still assembles at other modules and planet counts", assembles_at_this_point, sweep={"module": [1.5, 2.5], "planets": [3, 4]})


def in_mesh_at_this_angle():
    # `sun_angle` has posed the sun, the planets and the ring (the carrier
    # held); the running gaps must hold in that position as they do in the
    # assembled one -- conjugate flanks keep their backlash along the whole
    # line of action, and this measures it rather than assuming it.
    worst = MAX_MESH_GAP
    for k in range(planets):
        worst = min(worst, running_gap(distance(sun, planet_bodies[k]), "sun to planet %d at %s deg" % (k, sun_angle)))
        worst = min(worst, running_gap(distance(planet_bodies[k], ring), "planet %d to the ring at %s deg" % (k, sun_angle)))
    s = distance(sun, carrier)
    assert_close(s.distance, layers_up * layer, tol=1e-3, msg="the sun still floats two layers up at %s deg" % sun_angle)
    return worst


test("the sun turns through a tooth pitch and never touches a planet, nor a planet the ring", in_mesh_at_this_angle, sweep={"sun_angle": [0, 7.5, 15, 22.5, 30]})


# The declared ends that once failed build: the widest gap (0.6 met an edge
# that starts and ends at one point in the gear library's glue) and the
# thinnest pin (4 and 5 fail a carrier union inside the kernel).
test("the widest gap and the thinnest pin", every_body_is_a_solid, sweep={"gap": [0.5], "pin": [6]})
