# A crank-driven planetary gear toy for a four-year-old: the print-in-place
# herringbone planetary of `examples/planetary.star`, chunkier, on a
# wide rounded foot, with a screw-on lid that covers every mesh, a crank
# whose knob is printed in place on it, and a carousel of figures on the
# carrier that turns once for every four turns of the crank.
#
# Two prints, as plain 3MFs, and the same two as one Bambu Studio project:
#
#   print 1, `gear_toy_core.3mf` -- the print-in-place core: the ring (with
#     its foot and its thread), the carrier (with the carousel), the sun
#     (with its crank post) and three planets, six bodies that never touch.
#     It comes off the bed assembled and turns: the herringbone V locks the
#     planets in the ring and the sun in the planets, the pin heads hold the
#     planets down, and the lid retains the sun and the planets a second time.
#   print 2, `gear_toy_lid.3mf` -- the lid, plate down, beside the crank with
#     its knob printed in place on the crank's pin (a headed pin like the
#     planets', 0.4 mm gap all round): the knob spins and never comes off.
#   `gear_toy.3mf` -- the project: plate 1 "Core (print in place)"
#     is the six bodies as ONE object with six parts, so Bambu Studio cannot
#     move them apart; plate 2 "Lid" the lid; plate 3 "Crank" the crank and
#     its knob as one object. Ring, sun, planets and carrier each take a
#     filament slot from their colour (four slots); the lid, crank and knob
#     are pinned onto those four with `extruder=` rather than opening three
#     more. Every body at 0.01 mm.
#
# The core is `planetary_set(...)` from `@cadpole/gears.star`, the same
# construction `examples/planetary.star` uses: module 2, 12/12/36, 30 deg
# herringbone, 14 mm face, 0.35 mm running gaps, vertical gaps of two whole
# layers, every body at `tessellation(tolerance=0.01)`. Its hooks carry the
# toy: a hex post on the sun instead of the key boss, a threaded collar and
# foot on the ring, the carousel glued on the carrier's plate, and
# `ring_lift` to stand the ring gear two layers up on its own collar.
#
# The thread is `thread()` from `@cadpole/mechanical.star`, with a
# **custom profile**: a symmetric trapezoid 1.15 mm deep whose flanks stand
# 40 deg from the axis, at a 4 mm pitch, two turns. The stock profiles are
# not printable standing on end -- ISO's flanks lie 30 deg from the radial
# plane and ACME's 14.5 deg, so each ridge's underside is a 60-75 deg
# overhang -- while a 40 deg trapezoid is self-supporting both ways up, which
# a lid printed plate-down needs. The lid's groove is the same profile
# with `internal=True, clearance=0.3`, so the 0.3 mm stands on *every*
# surface by construction; the test measures it back with `clearance_map`.
# `thread()` returns a mesh body, so the ring and the lid are mesh bodies
# here, their triangles the operands' own at 0.01 mm, which is the bound
# every measurement against them carries. The gears, the carrier and the
# knob stay exact.
#
# The lock: a radial M3 set screw through the lid's skirt, bearing on the
# ring's wall below the thread, which is turned down 1.5 mm for it. A parent
# drives it with a screwdriver and the lid will not turn; a four-year-old
# cannot. A set screw works at whatever angle the thread stops at, which a
# snap catch in a notch, or a screw down through the lid into a boss, would
# not.
#
# Safety, as design intent -- this is a model, not a certification, and
# nothing here has been near a test lab:
#   * ASTM F963-23 4.6 / EN 71-1 5.1, **small parts**: nothing that can come
#     free fits the small-parts cylinder (31.7 mm across, 57.1 mm deep). The
#     knob is 34 mm across and captive on its pin; the crank is 61 mm long;
#     the lid is 104 mm and the core 115 mm. The sun and the planets are
#     captive twice over. Asserted from bounding boxes.
#   * EN 71-1 4.10 / ASTM F963 4.9, **accessible drive mechanisms**: no tooth
#     mesh is reachable. The lid's hub covers the sun-planet mesh out to
#     2.3 mm past the sun's tips and its outer annulus covers the
#     planet-ring mesh from 2.1 mm inside the ring's tips; the windows are
#     the band between, over the carousel and over nothing that meshes.
#   * EN 71-1 4.5, **finger entrapment**: no opening between 6 mm and 12 mm.
#     Measured: the windows are a 19.8 mm band (over 12); the boss runs in
#     the hub with 0.5 mm, the knob floats 0.4 mm over the arm, the lid
#     stands 1.3 mm over the collar and 0.65 mm off the ring's wall, the
#     screw hole is 2.6 mm (all under 6); the arm stands 14 mm over the lid
#     and the knob's head recess is 13 mm (over 12).
#   * EN 71-1 4.7, **edges**: every rounding is made in the *profile*, not
#     by `chamfer()`. Every profile a hand meets is rounded --
#     `fillet2d` on the carousel's figures, arcs on the crescent, a
#     lofted radius on the foot and the knob's top, a 40 deg chamfer on the
#     lid's rim and the hub's top (a round there would print as an overhang
#     on a part that prints plate down).
#   * The print-in-place contract: no gap between moving parts under
#     0.35 mm, measured within 0.02 mm; `assert_printable` on each print.
#
# Design notes:
#   * The crank is not printed in place on the sun. The lid's centre hole
#     has to be smaller than the sun's tip circle for the lid to cover the
#     sun-planet mesh, so nothing wider than about 23 mm on the sun can
#     pass through it, and the knob is 34 mm on a 35 mm arm. So the crank is
#     print 2's, dropped onto the sun's hex post afterwards and fastened
#     with an M3 screw; the *knob* is print-in-place on the crank.
#   * The lid has two 19.5 x 70 mm windows. The meshes sit at r 11-16 and
#     r 40-45, so the band the lid may open is 19.5 mm wide; anything wider
#     would uncover a mesh.
#   * The crank's clamp hole is cut on the mesh kernel, so the crank is a
#     mesh body and moving or posing it is a vertex pass.
#   * The crank-angle sweep (`crank_angle`) poses the sun, the planets, the
#     ring and the lid with the carrier held, which is the pitch sweep the
#     tests want; `crank_turn` poses the toy as it is played, the ring held,
#     the crank and the knob following the sun. The crank's fit on the post
#     is asserted in the static test.
#   * The scale sweep runs at 0.9 and 1.1: +/-10 % of every length in the
#     mechanism.
#   * The crank's hub, a loft of circles, stands its foot 1 mm into the arm,
#     off the knob pin's foot plane: two lofts whose feet share a plane
#     inside one slab are a contact the second union can trip on.
#   * `cadpole check --support-free` passes the lid and the crank prints
#     and finds one thing in the core: the ring gear's tooth spaces. The
#     ring stands two layers over the carrier plate on its collar, so the
#     underside of each tooth space is a flat ledge held up by the collar
#     alone and reaching 3.7 mm in over the moat round the plate, past the
#     3 mm `max_cantilever` -- about 62 mm2 in all. The gears' undersides,
#     two layers over the plate, rest on it and are not counted;
#     `--max-cantilever 4` passes it.
#
#   cadpole test examples/gear_toy.star
#   cadpole check examples/gear_toy.star
#   cadpole check examples/gear_toy.star --support-free
#   cadpole export examples/gear_toy.star --run-exports --out-dir /tmp
#   cadpole render examples/gear_toy.star --param print_layout=false --backend path --samples 256 --size 1600x1200 --view product --lighting product --backdrop studio --floor gloss --tonemap neutral --denoise -o docs/demo/gear_toy.png
#   cadpole render examples/gear_toy.star --param print_layout=false --param explode=40 --backend path --samples 256 --size 1600x1200 --view product --lighting product --backdrop studio --floor gloss --tonemap neutral --denoise -o docs/demo/gear_toy_open.png
#   cadpole sweep examples/gear_toy.star --param crank_angle=0,10,20,30 -o /tmp/crank.png
#   cadpole animate examples/gear_toy.star --view product --lighting product --backdrop studio --tonemap neutral --no-grid --no-edges -o docs/demo/gear_toy.gif

load("@cadpole/gears.star", "planetary_set", "tip_diameter")
load("@cadpole/mechanical.star", "thread")

# --- Parameters. The mechanism scales with `kid_size`; the printer numbers
# -- the gaps, the layers, the thread, the knob, the finger probes -- do not.
kid_size = param("kid_size", 1.0, min=0.9, max=1.1, doc="Scale of the mechanism: module, face width, crank radius, foot and figures. The gaps, the thread and the knob are printer and hand numbers and stay where they are")
crank_angle = param("crank_angle", 0.0, unit="deg", min=0, max=30, doc="Turn the sun by this; the planets spin back and the ring with its lid turn a third as far the other way, the carrier held, so the train stays in mesh and the tests re-measure every gap")
lift = param("explode", 0.0, unit="mm", min=0, max=80, doc="Lid-off view: the lid lifts this much and the crank with its knob twice as far; a picture only, never geometry -- every test measures the assembled position. Over its last two turns of travel (two pitches) the lid also turns by the thread's lead, so it screws on as it descends")
crank_lift = param("crank_lift", 0.0, unit="mm", min=0, max=80, doc="The crank and its knob stand this much above the post, on top of what `explode` gives them; a picture only. The animation drops them last")
crank_turn = param("crank_turn", 0.0, unit="deg", min=0, max=1440, doc="Turn the crank by this with the ring held, which is how the toy is played: the sun turns with it, the carrier and its carousel a quarter as far, each planet spins back on its pin as it rides round, and the crank and the knob follow the sun. What the animation drives; a pose, never geometry, and every test measures at 0")
gap = param("gap", 0.35, unit="mm", min=0.3, max=0.5, doc="The running gap every flank, tip and rim stands from its neighbour in the core print")
layer = param("layer", 0.2, unit="mm", min=0.1, max=0.3, doc="Layer height the vertical gaps are a whole number of")
print_layout = param("print_layout", True, doc="Build print 2's layout -- the lid turned over and the crank beside it -- as well as the assembled toy. The tests need it; a picture does not, so the renders below turn it off")
chord = param("chord", 0.01, unit="mm", min=0.002, max=0.05, doc="Tessellation tolerance of every body: what the 3MFs carry and what every measurement below is measured at")

K = kid_size
PRINTER = printer("bambu_a1")
MIN_GAP = PRINTER.min_gap  # 0.3 mm: what survives an extrusion's width and a bore's shrink
NOZZLE = PRINTER.nozzle
MAX_MESH_GAP = 0.6  # a flank gap over this and the teeth no longer share a line of action
MAX_OVERHANG = 12.0  # mm2 per print: the thread's run-out on the ring in print 1; the lid's run-out and the teardropped tips of the two M3 cross-holes in print 2. Every real face is at 40 deg or less
LAYERS_UP = 2
FINGER_LO, FINGER_HI = 6.0, 12.0  # no opening may fall between these
SMALL_PARTS = (31.7, 57.1)  # the small-parts cylinder: diameter, depth

# The gear set.
module = 2.0 * K
width = 14.0 * K
TEETH_SUN, TEETH_PLANET, PLANETS, HELIX = 12, 12, 3, 30.0
PIN, PLATE = 8.0, 3.0
BORE_CLEARANCE = 0.35
RIM_GEAR = 2.0  # the ring gear's own housing wall; the collar adds the rest
WALL = 4.0 * K  # the collar's wall over the gear's root circle: the thread's major radius
FIG_H = 5.0 * K  # the carousel's figures stand this proud of the carrier plate

# The foot and the thread.
FOOT_H = 4.0
FOOT_REACH = 9.0 * K
FOOT_ROUND = 3.0
FOOT_INSET = 0.3  # the first two layers, against elephant's foot
PITCH = 4.0
DEPTH = 0.9
FLANK = 50.0  # the profile's `angle` is measured from the *radial*: 50 deg there is 40 deg from the axis, which is the printable direction (measured: the whole lid reads 40.0 deg)
BOTTOM = 1.0
THREAD_GAP = 0.34  # what the gap measures along the flank's normal, which is what the nozzle has to clear
THREAD_CL = 0.42  # in the profile's own u and v; the gap it leaves along the flank's normal is this times cos(FLANK - 10 deg), measured at 0.30 mm -- the number that has to clear the nozzle
TURNS = 2.0
THREAD_LEN = TURNS * PITCH
WALL_RELIEF = 1.5  # the ring's wall is turned down by this below the thread
PROFILE = {"depth": DEPTH, "top": BOTTOM + 2 * DEPTH * math.tan(FLANK), "bottom": BOTTOM, "angle": FLANK}

# The lid.
LID_WALL = 3.0
PLATE_T = 3.0
PLATE_GAP = 1.3  # the plate's underside over the collar's top
PLATE_CROSS = 0.6  # how far the skirt reaches past the plate's underside, so the two cross rather than share a plane
SKIRT_BELOW = 4.0  # the skirt reaches this far below the thread, for the set screw
SCREW_R = 1.3  # a 2.6 mm hole: an M3 forms its own thread in PLA
SCREW_ANGLE = 210.0
HUB_CL = 0.5  # the lid's hub hole round the sun's post
SPOKE = 5.0  # half the width of the bar between the two windows
WINDOW_FILLET = 4.0
CHAMFER = 1.0  # the lid's top outer edge, at 40 deg

# The crank, the post and the knob.
BOSS_AF = 12.0  # across the flats
BOSS_R = BOSS_AF / 2 / math.cos(30.0)
SOCKET_CL = 0.35
ARM_CLEAR = 14.0  # the arm's underside over the lid's top: clear of the 12 mm probe
ARM_T = 6.0
HUB_R = 13.0
HUB_H = 24.0  # tall enough for the socket and its 40 deg pyramid roof
SOCKET_DEPTH = 12.0
CRANK_R = 35.0 * K
KNOB_R = 17.0  # 34 mm across: over the small-parts cylinder even if it ever came free
KNOB_H = 25.0
KNOB_CL = 0.4
PIN_R = 4.0
HEAD_FLARE, HEAD_RISE, HEAD_HEIGHT = 1.0, 1.2, 1.5
PRINT_DX = 95.0 * K  # where print 2 puts the crank, beside the lid


def polar(r, angle):
    return (r * math.cos(angle), r * math.sin(angle))


def teardrop(r, length, up, name):
    """A horizontal screw hole's tool, teardropped: the hull of the bore and
    a thin cylinder `1.4 r` towards `up` in the tool's own frame, so the
    hole's ceiling is a 45 deg peak instead of a 2.6 mm bridge. `up` is
    where world +z will be once the tool is turned (`(-1, 0, 0)` for a tool
    turned about +y, `(0, 1, 0)` for one turned about +x)."""
    bore = cylinder(r, length, name=name + "_bore")
    peak = cylinder(0.25, length, at=(1.4 * r * up[0], 1.4 * r * up[1], 0), name=name + "_peak")
    t = hull(bore, peak, name=name)
    t.tessellation(tolerance=chord)
    return t


# --- Heights, in the world frame.
z_gears = PLATE + LAYERS_UP * layer  # the gears float this high over the carrier plate
ring_height = 2 * z_gears + width  # the ring gear's own height
z_gear_top = ring_height
z_collar_top = z_gear_top + 1.0  # the collar rises past the gear: no coplanar pair in the union
z_th0 = z_collar_top - THREAD_LEN
z_p0 = z_collar_top + PLATE_GAP  # the lid's plate, underside
z_p1 = z_p0 + PLATE_T
z_skirt0 = z_th0 - SKIRT_BELOW
z_screw = z_skirt0 + 1.8
z_arm0 = z_p1 + ARM_CLEAR
z_arm1 = z_arm0 + ARM_T
z_boss_top = z_arm1 + SOCKET_DEPTH - 0.5
z_notch = z_arm0 + ARM_T / 2 + 0.3  # the clamp screw's height, and the post's notch
assert_true(z_th0 > FOOT_H + 4.0, msg="the thread starts at %s, too close to the foot at %s" % (z_th0, FOOT_H))
assert_true(z_screw < z_th0 - 1.0, msg="the set screw at %s is inside the thread" % z_screw)


# --- The sun's post: a hex bar with a notch for the crank's clamp screw and
# a lead-in chamfer, one ruled loft, glued on the sun's top face.
def sun_post(z_top, info):
    def hexs(z_world, r):
        return sketch(XY.offset(z_world - z_gears), name="post_%s" % math.round(z_world, 2)).ngon(6, r)

    r_notch = BOSS_R - 0.65
    return loft(
        [
            hexs(z_gears + z_top, BOSS_R),  # on the sun's top face
            hexs(z_notch - 1.2, BOSS_R),
            hexs(z_notch - 0.6, r_notch),  # the notch's floor faces up: any slope will print
            hexs(z_notch + 0.9, r_notch),
            hexs(z_notch + 1.65, BOSS_R),  # and its ceiling grows back at 40 deg from vertical
            hexs(z_boss_top - 1.2, BOSS_R),
            hexs(z_boss_top, BOSS_R - 1.0),  # a lead-in for the crank's socket
        ],
        ruled=True,
        name="post",
    )


# --- The ring's collar: the foot and the wall as one ruled loft, the thread
# as `thread()` bored out, united with the gear on the mesh kernel (the
# thread is a mesh body); every operand is tessellated at `chord` first, so
# the union keeps 0.01 mm triangles.
def ring_collar(gear, info):
    r_wall = info["lay_ring"]["r_root"] + WALL
    r_in = info["lay_ring"]["r_root"] + RIM_GEAR - 0.5  # half a millimetre inside the gear's housing wall
    r_foot = r_wall + FOOT_REACH
    def sec(z, r, name):
        return sketch(XY.offset(z), name=name).circle(r).circle(r_in)

    secs = [
        sec(0.0, r_foot - FOOT_INSET, "foot_bed"),  # the first two layers inset, against elephant's foot
        sec(LAYERS_UP * layer, r_foot, "foot_rise"),
        sec(FOOT_H - FOOT_ROUND, r_foot, "foot_side"),
    ]
    for i, a in enumerate([45.0, 90.0]):
        secs.append(sec(FOOT_H - FOOT_ROUND + FOOT_ROUND * math.sin(a), r_foot - FOOT_ROUND + FOOT_ROUND * math.cos(a), "foot_round_%d" % i))
    secs.extend(
        [
            sec(FOOT_H + 0.3, r_wall - WALL_RELIEF, "foot_top"),  # a 12 deg cone up to the turned-down wall
            sec(z_th0 + 1.0, r_wall - WALL_RELIEF, "wall_top"),  # the wall runs 1 mm *past* the thread's start, inside the rod: the two must cross, not meet
        ]
    )
    collar = loft(secs, ruled=True, name="collar_wall")
    collar.tessellation(tolerance=chord)
    rod = thread(2 * r_wall, PITCH, THREAD_LEN, profile=PROFILE, start=(0, 0, z_th0), tolerance=chord, name="collar_thread")
    bore = cylinder(r_in, THREAD_LEN + 4.0, at=(0, 0, z_th0 - 2.0), name="collar_bore")
    bore.tessellation(tolerance=chord)
    rod = subtract(rod, bore, kernel="mesh", name="collar_bored")
    # The step from the turned-down wall up to the thread's crest is cut
    # *into the rod* as a 45 deg chamfer, not added as a lofted cone: a cone
    # whose top edge lands exactly on the rod's cylinder meets it along a
    # circle, and the mesh union pinches there. Cut this way the wall is
    # strictly inside the rod wherever they overlap, so they cross.
    step = WALL_RELIEF + 1.1  # the chamfer's own slope: 2.1 mm out over 2.6 mm up is 39 deg from vertical, under the overhang rule
    chamfer_tool = loft(
        [
            sketch(XY.offset(z_th0 - 0.4), name="collar_chamfer_0").circle(r_wall + 8.0).circle(r_wall - WALL_RELIEF - 0.4),
            sketch(XY.offset(z_th0 - 0.4 + step), name="collar_chamfer_1").circle(r_wall + 8.0).circle(r_wall + 0.2),
        ],
        ruled=True,
        name="collar_chamfer",
    )
    chamfer_tool.tessellation(tolerance=chord)
    rod = subtract(rod, chamfer_tool, kernel="mesh", name="collar_threaded")
    gear.tessellation(tolerance=chord)
    return union(gear, collar, rod, kernel="mesh", name="ring_body")


# --- The carousel: three figures glued on the carrier's plate before its
# pins are united, while that face is still untrimmed. The star and the fish
# are polygons rounded with `fillet2d`, the crescent is `subtract2d` of two
# circles: all three are lines and arcs, which `glue` keys edge for edge.
def figure_pts(kind, size):
    if kind == "star":
        return [polar(size if i % 2 == 0 else 0.45 * size, 90.0 + 36.0 * i) for i in range(10)]
    if kind == "fish":
        pts = [(1.0, 0.0), (0.55, 0.42), (0.0, 0.5), (-0.45, 0.3), (-0.55, 0.1), (-0.9, 0.45), (-1.0, 0.15), (-1.0, -0.15), (-0.9, -0.45), (-0.55, -0.1), (-0.45, -0.3), (0.0, -0.5), (0.55, -0.42)]
        return [(x * size, y * size) for x, y in pts]
    fail("no figure %s" % kind)


def place_pts(pts, r_at, angle):
    c = polar(r_at, angle)
    return [(c[0] + x * math.cos(angle) - y * math.sin(angle), c[1] + x * math.sin(angle) + y * math.cos(angle)) for x, y in pts]


def carousel(plate_body, info):
    r_fig = 0.5 * (info["lay_sun"]["r_tip"] + info["lay_ring"]["r_tip"]) + 0.3  # the middle of the lid's window band
    size = 8.0 * K
    out = plate_body
    for kind, angle, r_round in [("star", 60.0, 0.9 * K), ("moon", 180.0, 0.0), ("fish", 300.0, 0.5 * K)]:
        if kind == "moon":
            c = polar(r_fig, angle)
            bite = (c[0] + 0.40 * size * math.cos(angle + 180.0), c[1] + 0.40 * size * math.sin(angle + 180.0))
            # The cusps are rounded too: a crescent's horns taper to a
            # point, which `print_check` reads as a thin wall.
            profile = fillet2d(
                subtract2d(
                    sketch(XY.offset(PLATE), name="moon_disc").circle(0.92 * size, center=c),
                    sketch(XY.offset(PLATE), name="moon_bite").circle(0.74 * size, center=bite),
                    name="moon_crescent",
                ),
                0.8 * K,
                name="moon_sketch",
            )
        else:
            profile = fillet2d(sketch(XY.offset(PLATE), name="%s_polygon" % kind).polygon(place_pts(figure_pts(kind, size), r_fig, angle)), r_round, name="%s_sketch" % kind)
        out = glue(out, extrude(profile, FIG_H, name=kind), name="plate_with_%s" % kind)
    return out


S = planetary_set(
    module,
    TEETH_SUN,
    TEETH_PLANET,
    width,
    planets=PLANETS,
    helix_angle=HELIX,
    gap=gap,
    layer=layer,
    layers_up=LAYERS_UP,
    bore_clearance=BORE_CLEARANCE,
    pin=PIN,
    plate=PLATE,
    rim=RIM_GEAR,
    rim_gap=0.5,
    socket=0.0,  # no hex key socket: a 6 mm hole through a toy is a finger trap
    tolerance=chord,
    sun_boss=sun_post,
    ring_extra=ring_collar,
    plate_extra=carousel,
)
P = S["P"]
a = S["a"]
alpha_t = S["alpha_t"]
backlash = S["backlash"]
lay_sun, lay_planet, lay_ring = S["lay_sun"], S["lay_planet"], S["lay_ring"]
r_wall = lay_ring["r_root"] + WALL
r_in = lay_ring["r_root"] + RIM_GEAR - 0.5
r_minor = r_wall - DEPTH
sun, planet_bodies, ring, carrier = S["sun"], S["planets"], S["ring"], S["carrier"]
sun.material(preset="pla_silk", color="#e8b84a")
for g in planet_bodies:
    g.material(preset="pla_silk", color="#2f8f8f")
ring.material(preset="pla_matte", color="#3b3f46")
carrier.material(preset="pla_silk", color="#e0602c")

# The figures turn with the carrier and never meet a planet.
r_fig = 0.5 * (lay_sun["r_tip"] + lay_ring["r_tip"]) + 0.3
for angle in [60.0, 180.0, 300.0]:
    for k in range(PLANETS):
        d = math.hypot(r_fig * math.cos(angle) - S["pin_centres"][k][0], r_fig * math.sin(angle) - S["pin_centres"][k][1])
        assert_true(d - 8.0 * K >= lay_planet["r_tip"] + 2.0, msg="the figure at %s deg reaches within 2 mm of planet %d's tip circle" % (angle, k))
assert_true(r_fig - 8.0 * K >= lay_sun["r_tip"] + 2.0, msg="the figures reach the sun's tips")

# --- The lid: a skirt with the thread cut into it, a rim, and a plate with
# a hub over the sun's mesh, an annulus over the planet-ring mesh and two
# windows between them. The thread tool reaches 3.7 mm past each end of the
# thread, so it is subtracted from the skirt *alone* and the plate is united
# afterwards -- cut from the whole lid it would bore the plate away.
r_bore = r_wall + THREAD_CL  # the skirt's bore at the thread's crest
r_o = r_bore + LID_WALL
r_hub = BOSS_R + HUB_CL
r_hub_cover = lay_sun["r_tip"] + 2.3  # the hub's disc covers the sun-planet mesh
r_win = lay_ring["r_tip"] - 2.1  # and the outer annulus the planet-ring mesh
def window(sign):
    """One window: the band from the hub's disc to the outer annulus, between
    the two spokes, as two lines and two arcs."""
    y_h = math.sqrt(r_hub_cover * r_hub_cover - SPOKE * SPOKE)
    y_w = math.sqrt(r_win * r_win - SPOKE * SPOKE)
    x = sign * SPOKE
    return path((x, y_h)).line((x, y_w)).arc((x, -y_w), via=(sign * r_win, 0.0)).line((x, -y_h)).arc((x, y_h), via=(sign * r_hub_cover, 0.0))


def lid_face(z, r_out, name):
    """A section of the lid's top: the outer circle, the hub's hole and the
    two windows, in that order in every section so the loft pairs loop to
    loop. `fillet2d` rounds the windows' four corners; the circles have no
    corners to round."""
    return fillet2d(sketch(XY.offset(z), name=name).circle(r_out).circle(r_hub).add(window(1.0)).add(window(-1.0)), WINDOW_FILLET, name=name + "_rounded")


# The skirt: a plain tube with the groove cut into it. The thread tool
# reaches 3.7 mm past each end of the thread, so it is subtracted from the
# skirt *alone*; taken out of the whole lid it would bore the plate away.
skirt = extrude(sketch(XY.offset(z_skirt0), name="skirt_profile").circle(r_o).circle(r_minor + 0.4), z_p0 + PLATE_CROSS - z_skirt0, name="skirt_blank")
skirt.tessellation(tolerance=chord)
groove = thread(2 * r_wall, PITCH, THREAD_LEN, profile=PROFILE, internal=True, clearance=THREAD_CL, start=(0, 0, z_th0), tolerance=chord, name="lid_groove")
skirt = subtract(skirt, groove, kernel="mesh", name="skirt")
# The top: the plate and its rim as **one** loft, not two bodies -- a plate
# whose underside is the rim's underside shares that plane with it, and a
# mesh union pinches on the shared edge. Only the
# outer circle moves between the sections, so the windows and the hub's
# hole run straight through, and the last 1 mm of the outer wall leans in
# at 40 deg: the chamfer that lets the lid print plate down.
lid_top = loft(
    [
        lid_face(z_p0, r_o, "lid_top_0"),
        lid_face(z_p1 - CHAMFER / math.tan(40.0), r_o, "lid_top_1"),
        lid_face(z_p1, r_o - CHAMFER, "lid_top_2"),
    ],
    ruled=True,
    name="lid_top",
)
lid_top.tessellation(tolerance=chord)
lid_solid = union(skirt, lid_top, kernel="mesh", name="lid_solid")
set_screw_blank = teardrop(SCREW_R, LID_WALL + 4.0, (-1, 0, 0), "set_screw_tool")
set_screw = set_screw_blank.rotate(90, axis=(0, 1, 0), name="set_screw_radial").rotate(SCREW_ANGLE, axis=(0, 0, 1), name="set_screw_turned").translate(polar(r_bore - 1.0, SCREW_ANGLE) + (z_screw,), name="set_screw_placed")
lid_body = subtract(lid_solid, set_screw, kernel="mesh", name="lid_body")
lid_body.material(preset="pla_matte", color="#4a7fd6")


# --- The crank and its knob. Each is built once, where it is used; the
# copies print 2 lays out are `copy`s moved aside, which is cheap because
# the crank is a **mesh** body by then (a mesh transform is a remap) and
# the knob is one loft with no boolean in it.
def build_crank(x0, z0, tag):
    """The arm with the knob's headed pin, the hub with the hex socket and
    its 50 deg roof, the clamp screw's cross-drilling and the spiral on the
    hub's top. `z0` is the arm's underside."""

    def nm(s):
        return "%s_%s" % (tag, s)

    arm = extrude(sketch(XY.offset(z0), name=nm("arm_profile")).slot(CRANK_R + 2 * HUB_R, 2 * HUB_R, center=(x0 + CRANK_R / 2, 0)), ARM_T, name=nm("arm_slab"))
    z_pin_top = z0 + ARM_T + KNOB_CL + KNOB_H - 4.0 + KNOB_CL
    pin = loft(
        [
            sketch(XY.offset(z0 + ARM_T - 0.5), name=nm("pin_foot")).circle(PIN_R, center=(x0 + CRANK_R, 0)),
            sketch(XY.offset(z_pin_top), name=nm("pin_neck")).circle(PIN_R, center=(x0 + CRANK_R, 0)),
            sketch(XY.offset(z_pin_top + HEAD_RISE), name=nm("pin_flare")).circle(PIN_R + HEAD_FLARE, center=(x0 + CRANK_R, 0)),
            sketch(XY.offset(z_pin_top + HEAD_RISE + HEAD_HEIGHT), name=nm("pin_head")).circle(PIN_R + HEAD_FLARE, center=(x0 + CRANK_R, 0)),
        ],
        ruled=True,
        name=nm("knob_pin"),
    )
    body = union(arm, pin, kernel="brep", name=nm("arm_with_pin"))
    # The hub is a loft of circles. Its foot stands 1 mm into the arm,
    # **not** on the pin's foot plane 0.5 mm down: two feet in one plane
    # inside the slab are a contact the exact union can trip on.
    hub = loft(
        [
            sketch(XY.offset(z0 + ARM_T - 1.0), name=nm("hub_foot")).circle(HUB_R - 0.75, center=(x0, 0)),
            sketch(XY.offset(z0 + ARM_T + HUB_H - 2.0), name=nm("hub_shoulder")).circle(HUB_R - 0.75, center=(x0, 0)),
            sketch(XY.offset(z0 + ARM_T + HUB_H), name=nm("hub_top")).circle(HUB_R - 2.4, center=(x0, 0)),  # a 40 deg chamfer
        ],
        ruled=True,
        name=nm("hub"),
    )
    body = union(body, hub, kernel="brep", name=nm("arm_with_hub"))
    # The socket: **one** hex tool cut up through the arm from below, its
    # top drawn out to a point so the roof is a 40 deg pyramid. A second
    # tool for the roof would either leave a flat ceiling round its base or,
    # at exactly the socket's width, be an edge-on-face contact.
    socket_r = (BOSS_AF + 2 * SOCKET_CL) / 2 / math.cos(30.0)
    roof_h = socket_r * math.tan(50.0)  # 50 deg from horizontal is 40 from vertical
    z_socket_top = z0 + ARM_T + SOCKET_DEPTH
    body = subtract(
        body,
        loft(
            [
                sketch(XY.offset(z0 - 1.0), name=nm("socket_profile")).ngon(6, socket_r, center=(x0, 0)),
                sketch(XY.offset(z_socket_top), name=nm("socket_top")).ngon(6, socket_r, center=(x0, 0)),
                (x0, 0.0, z_socket_top + roof_h),
            ],
            ruled=True,
            name=nm("socket_tool"),
        ),
        kernel="brep",
        name=nm("socketed"),
    )
    assert_true(ARM_T + SOCKET_DEPTH + roof_h < ARM_T + HUB_H - 2.0, msg="the socket's roof reaches the hub's top")
    # The clamp screw, through the arm's flat side into the socket, level
    # with the post's notch, cut on the **mesh** kernel so the crank stays
    # cheap to move.
    body.tessellation(tolerance=chord)
    screw_blank = teardrop(SCREW_R, HUB_R + 6.0, (0, 1, 0), nm("clamp_tool"))
    screw = screw_blank.rotate(90, axis=(1, 0, 0), name=nm("clamp_radial")).translate((x0, HUB_R + 2.0, z0 + (z_notch - z_arm0)), name=nm("clamp_placed"))
    body = subtract(body, screw, kernel="mesh", name=nm("drilled"))
    # The spiral on the hub's top, two turns: it turns with the sun, four
    # times for every turn of the carousel under the windows. (On the hub,
    # not on the knob: the knob spins free on its pin, so a mark on it
    # follows the child's fingers, not the mechanism.)
    pts = []
    n_sp = 36
    for i in range(2 * n_sp + 1):
        th = 360.0 * i / n_sp
        pts.append(polar(3.5 + 6.0 * th / 720.0, th))
    for i in range(2 * n_sp, -1, -1):
        th = 360.0 * i / n_sp
        pts.append(polar(3.5 + 6.0 * th / 720.0 - 2.2, th))
    spiral = extrude(sketch(XY.offset(z0 + ARM_T + HUB_H - 0.5), name=nm("spiral_profile")).polygon([(x0 + x, y) for x, y in pts]), 2.0, name=nm("spiral"))
    spiral.tessellation(tolerance=chord)
    return union(body, spiral, kernel="mesh", name=nm("crank"))


def build_knob(x0, z0, tag):
    """The knob: a 40 deg cone over the arm, a barrel, a rounded top and the
    recess the pin's head sits in. It floats `KNOB_CL` over the arm and runs
    on the pin with the same gap, as a planet does on its own."""

    def nm(s):
        return "%s_%s" % (tag, s)

    c = (x0 + CRANK_R, 0)
    z_k0 = z0 + ARM_T + KNOB_CL
    z_k1 = z_k0 + KNOB_H
    cone_h = (KNOB_R - 7.5) * math.tan(50.0)
    secs = [
        (z_k0, 7.5, PIN_R + KNOB_CL),
        (z_k0 + cone_h, KNOB_R, PIN_R + KNOB_CL),
        (z_k1 - 4.0, KNOB_R, PIN_R + KNOB_CL),
        (z_k1 - 3.8, KNOB_R, PIN_R + HEAD_FLARE + 1.5),  # the head's recess: 13 mm across, over the finger band
    ]
    for ang in [30.0, 60.0, 90.0]:
        secs.append((z_k1 - 3.0 + 3.0 * math.sin(ang), KNOB_R - 3.0 + 3.0 * math.cos(ang), PIN_R + HEAD_FLARE + 1.5))
    return loft([sketch(XY.offset(z), name=nm("knob_%d" % i)).circle(r, center=c).circle(h, center=c) for i, (z, r, h) in enumerate(secs)], ruled=True, name=nm("knob"))


crank_body = build_crank(0.0, z_arm0, "c")
crank_body.material(preset="pla_silk", color="#d94a3d")
knob_body = build_knob(0.0, z_arm0, "c")
knob_body.tessellation(tolerance=chord)
knob_body.material(preset="pla_silk", color="#6cb33f")
# Print 2's layout: the lid turned over so its plate is on the bed, and the
# crank with its knob set down beside it, arm on the bed.
if print_layout:
    lid_print = copy(lid_body, name="lid_copy").rotate(180, axis=(1, 0, 0), name="lid_turned").translate((0, 0, z_p1), name="lid_print")
    crank_print = copy(crank_body, name="crank_copy").translate((PRINT_DX, 0, -z_arm0), name="crank_print")
    knob_print = copy(knob_body, name="knob_copy").translate((PRINT_DX, 0, -z_arm0), name="knob_print")
lid = instance(part(lid_body, name="lid_part"), name="lid")
crank = instance(part(crank_body, name="crank_part"), name="crank")
knob = instance(part(knob_body, name="knob_part"), name="knob")

# --- The assembly and the views. `explode` and `crank_lift` are view
# offsets, never geometry: every measurement below is of the assembled
# position whatever the picture shows. The crank-angle sweep turns the sun,
# the planets, the ring and the lid with the carrier held, which is the whole
# train through a tooth pitch; `crank_turn` is the toy as it is played, the
# ring held. A pose re-expresses an instance's solid where it sits -- for the
# mesh bodies here a vertex pass, for the exact ones a map of control points
# -- and never a kernel op, which is what lets the animation below evaluate
# a frame in Starlark time.
toy = assembly(S["instances"] + [lid, crank, knob], name="toy")
explode(lid, (0, 0, lift))
explode(crank, (0, 0, 2 * lift + crank_lift))
explode(knob, (0, 0, 2 * lift + crank_lift))
# The lid screws on: a right-hand thread advances downward when turned
# clockwise seen from above (a negative turn about +z), so a lid lifted
# `lift` stands turned back by the lead of the travel still to go, up to the
# thread's two turns, and unwinds onto the ring as it descends.
lid_turn = 360.0 * min(lift, THREAD_LEN) / PITCH
if lid_turn:
    pose(lid, rotate=lid_turn)
if crank_angle:
    pose(sun, rotate=crank_angle)
    for k in range(PLANETS):
        pose(planet_bodies[k], rotate=-crank_angle * TEETH_SUN / TEETH_PLANET)
    pose(ring, rotate=-crank_angle * TEETH_SUN / S["teeth_ring"])
    pose(lid, rotate=-crank_angle * TEETH_SUN / S["teeth_ring"])
if crank_turn:
    # The ring held: the carrier turns 1/(1 + z_r/z_s) of the sun, and each
    # planet spins about its own pin by -z_s/z_p of the sun's turn relative
    # to the carrier, then rides round with the carrier.
    carrier_turn = crank_turn / P["ratio_fixed_ring"]
    pose(sun, rotate=crank_turn)
    pose(carrier, rotate=carrier_turn)
    for k in range(PLANETS):
        pose(planet_bodies[k], rotate=-(crank_turn - carrier_turn) * TEETH_SUN / TEETH_PLANET)
        pose(planet_bodies[k], rotate=carrier_turn, origin=(0, 0, 0))
    pose(crank, rotate=crank_turn)
    pose(knob, rotate=crank_turn)

core = S["instances"]
export(core, "gear_toy_core.3mf")
if print_layout:
    export([lid_print, crank_print, knob_print], "gear_toy_lid.3mf")
# The Bambu Studio project: three calls to one path, one plate each.
# The core's bodies carry their own 0.01 mm tessellation; the call-level
# `tolerance=` is what the lid, crank and knob copies are meshed at. Slots
# 1-4 are the carrier, the ring, the planets and the sun, in the order the
# core's bodies first use them; the lid prints in the ring's slot, the crank
# in the carrier's, the knob in the sun's, so the project has four filaments.
export(core, "gear_toy.3mf", flavor=flavor.bambu, plate="Core (print in place)", name="core", tolerance=chord)
if print_layout:
    export(lid_print, "gear_toy.3mf", flavor=flavor.bambu, plate="Lid", tolerance=chord, extruder=2)
    export([crank_print, knob_print], "gear_toy.3mf", flavor=flavor.bambu, plate="Crank", name="crank", tolerance=chord, extruder=[1, 4])
# And a shareable picture of each plate beside the project, plus a contact
# sheet of the three: `gear_toy_plate_1_Core_print_in_place.png`, ... and
# `gear_toy_plates.png`, written by `--run-exports` with the file.
plate_images("gear_toy.3mf")

# --- The animation: `cadpole animate examples/gear_toy.star -o x.gif`.
# The lid and the crank come down from the exploded view -- the lid turning
# on over its last two turns of thread, the crank dropping onto the post
# last -- a half-second beat, then two turns of the crank with the ring
# held, the carousel passing the windows, the camera drifting 20 degrees
# round. Every frame is a pose: after the first, zero kernel ops and zero
# tessellations (`cadpole animate` prints the counts).
animation(
    "assemble_and_turn",
    [
        keyframe(0.0, explode=30, crank_lift=0, print_layout=False, camera=view(az=30, el=18), ease=ease.ease_out),
        keyframe(2.0, explode=0, crank_lift=24),
        keyframe(2.5, crank_lift=0),
        keyframe(3.0, crank_turn=0, ease=ease.linear),
        keyframe(9.0, crank_turn=720, camera=view(az=50)),
    ],
    fps=30,
    loop=True,
)


# --- Tests (`cadpole test examples/gear_toy.star`). Every one measures.
def part_of(face, lay):
    """Which part of a tooth a gear face is, from its outline segment."""
    i = face.find("side[0.")
    if i < 0:
        return face
    j = face.find("]", i)
    s = int(face[i + 7:j]) % lay["segments"]
    for part_name, ranges in lay["parts"].items():
        for lo, hi in ranges:
            if lo <= s and s < hi:
                return part_name
    return face


def running_gap(c, what, hi=MAX_MESH_GAP, tol=None):
    """`c` (a Clearance) is a gap the printer can make and the mechanism can
    use. `tol` overrides the Clearance's own bound for a pair involving the
    ring or the lid: both are mesh bodies, whose reported tolerance is the
    document's 0.05 while their triangles are their operands' own at
    `chord` -- a mesh union keeps every triangle it does not cut, and
    nothing here cuts a tooth or a thread flank."""
    tol = c.tolerance if tol == None else tol
    assert_true(not c.overlap, msg="%s: %s and %s share material at %s" % (what, c.face_a, c.face_b, c.at))
    assert_true(c.distance - 2 * tol >= MIN_GAP, msg="%s: %s mm between %s and %s at %s, measured at +/-%s mm, cannot prove the %s mm fuse threshold" % (what, math.round(c.distance, 4), c.face_a, c.face_b, c.at, tol, MIN_GAP))
    assert_true(c.distance <= hi, msg="%s: %s mm between %s and %s is too loose to run" % (what, math.round(c.distance, 4), c.face_a, c.face_b))
    return c.distance


def flank_gap(a_body, b_body, lay_a, lay_b):
    best = None
    for d, pa, pb, fa, fb in clearance_map(a_body, b_body, below=MAX_MESH_GAP):
        if part_of(fa, lay_a).startswith("flank") and part_of(fb, lay_b).startswith("flank"):
            best = d if best == None else min(best, d)
    assert_true(best != None, msg="no flank of %s faces a flank of %s within %s mm" % (a_body.name, b_body.name, MAX_MESH_GAP))
    return best


def thread_clearance():
    """The closest approach of the lid's groove to the ring's thread over the
    engaged turns, by position: both bodies are mesh unions, whose faces do
    not all keep the lofts' names."""
    best = None
    for d, pa, pb, fa, fb in clearance_map(lid, ring, below=1.0, spacing=2.0, limit=512):
        r_a = math.hypot(pa[0], pa[1])
        if z_th0 + 0.5 <= pa[2] and pa[2] <= z_collar_top - 0.5 and r_minor - 0.2 <= r_a and r_a <= r_wall + THREAD_CL + 0.2:
            best = d if best == None else min(best, d)
    assert_true(best != None, msg="no thread face of the lid is within 1 mm of the ring's")
    return best


def fits_small_parts(body):
    lo, hi = body.bbox()
    d = sorted([hi[0] - lo[0], hi[1] - lo[1], hi[2] - lo[2]])
    return d[2] <= SMALL_PARTS[1] and math.hypot(d[0], d[1]) <= SMALL_PARTS[0]


def the_train():
    assert_close(P["ratio_fixed_ring"], 4.0, msg="the carrier turns once for every four turns of the crank, the ring held")
    assert_eq(S["teeth_ring"], TEETH_SUN + 2 * TEETH_PLANET)
    assert_eq((TEETH_SUN + S["teeth_ring"]) % PLANETS, 0)
    assert_true(not lay_sun["undercut"] and not lay_planet["undercut"])
    return P["ratio_fixed_ring"]


test("the train: four crank turns for one of the carousel, and the set assembles", the_train)


def every_body_is_where_the_stack_puts_it():
    for b in core + [lid, crank, knob]:
        assert_solid(b.body())
    if print_layout:
        for b in [lid_print, crank_print, knob_print]:
            assert_solid(b)
    for b, kind in [(S["sun_body"], "brep"), (S["planet_body"], "brep"), (S["carrier_body"], "brep"), (knob_body, "brep"), (S["ring_body"], "mesh"), (lid_body, "mesh"), (crank_body, "mesh")]:
        assert_eq(b.kind, kind, msg="%s is %s" % (b.name, kind))
    lo, hi = ring.body().bbox()
    assert_close(lo[2], 0.0, tol=1e-4, msg="the ring stands on the bed, on its foot")
    assert_close(hi[2], z_collar_top, tol=1e-4, msg="the collar's top")
    assert_close(hi[0], r_wall + FOOT_REACH, tol=0.05, msg="the foot reaches its radius")
    lo, hi = sun.body().bbox()
    assert_close(hi[2], z_boss_top, tol=1e-4, msg="the post reaches the crank's socket")  # a bbox is read off f32 vertices
    lo, hi = carrier.body().bbox()
    assert_close(lo[2], 0.0, tol=1e-4, msg="the carrier stands on the bed")
    assert_true(hi[2] >= PLATE + FIG_H - 1e-6, msg="the figures stand proud of the plate")
    lo, hi = lid.body().bbox()
    assert_close(hi[2], z_p1, tol=1e-4, msg="the lid's top")
    assert_close(lo[2], z_skirt0, tol=1e-4, msg="the skirt's bottom")
    lo, hi = knob.body().bbox()
    assert_close(hi[0] - lo[0], 2 * KNOB_R, tol=0.05, msg="the knob is %s mm across" % (2 * KNOB_R))
    return hi[0] - lo[0]


test("every body is the solid the stack puts where it belongs", every_body_is_where_the_stack_puts_it)


def the_core_runs():
    worst = MAX_MESH_GAP
    for k in range(PLANETS):
        worst = min(worst, running_gap(distance(sun, planet_bodies[k]), "sun to planet %d" % k))
        assert_close(flank_gap(sun, planet_bodies[k], lay_sun, lay_planet), gap, tol=0.03, msg="planet %d: the sun's flanks" % k)
        worst = min(worst, running_gap(distance(planet_bodies[k], ring), "planet %d to the ring" % k, tol=chord))
        assert_close(flank_gap(planet_bodies[k], ring, lay_planet, lay_ring), gap, tol=0.04, msg="planet %d: the ring's flanks" % k)
        c = distance(planet_bodies[k], carrier)
        running_gap(c, "planet %d on its pin" % k)
        assert_true("pin_" in c.face_b and "side[1." in c.face_a, msg="planet %d: the closest approach to the carrier is %s/%s, not the pin in the bore" % (k, c.face_a, c.face_b))
        worst = min(worst, c.distance)
        float_gap, head_gap = None, None
        for d, pa, pb, fa, fb in clearance_map(planet_bodies[k], carrier, below=1.0, spacing=2.0, limit=512):
            if "plate" in fb and fa.endswith("/start"):
                float_gap = d if float_gap == None else min(float_gap, d)
            elif "pin_" in fb and fa.endswith("/end"):
                head_gap = d if head_gap == None else min(head_gap, d)
        assert_close(float_gap, LAYERS_UP * layer, tol=1e-3, msg="planet %d floats %s mm over the plate, not %s layers" % (k, float_gap, LAYERS_UP))
        assert_true(head_gap != None and head_gap - 2 * c.tolerance >= MIN_GAP, msg="planet %d: the pin head is %s mm over the planet's top" % (k, head_gap))
    running_gap(distance(carrier, ring), "the carrier's rim to the ring's teeth", tol=chord)
    s = distance(sun, carrier)
    assert_close(s.distance, LAYERS_UP * layer, tol=1e-3, msg="the sun floats %s mm over the carrier at %s/%s" % (math.round(s.distance, 4), s.face_a, s.face_b))
    return worst


test("the core runs: every mesh at the gap asked for, every planet on its pin, the sun and the planets two layers up", the_core_runs)


def the_lid_threads_on():
    t = thread_clearance()
    # `clearance=` is an offset in the profile's own u and v; what the
    # printer needs is the gap along the flank's normal, and that is what
    # this measures. 0.42 in the profile leaves 0.34 mm on the normal.
    assert_close(t, THREAD_GAP, tol=0.03, msg="the lid's groove stands %s mm from the ring's thread on the normal, not %s" % (math.round(t, 4), THREAD_GAP))
    assert_true(t - 2 * chord >= MIN_GAP, msg="the thread's gap of %s mm, measured at +/-%s, cannot prove the %s mm fuse threshold" % (math.round(t, 4), chord, MIN_GAP))
    c = distance(lid, ring)
    assert_true(not c.overlap, msg="the lid and the ring share material at %s (%s/%s)" % (c.at, c.face_a, c.face_b))
    assert_true(c.distance - 2 * chord >= 0.2, msg="the lid's closest approach to the ring is %s mm at %s/%s" % (math.round(c.distance, 4), c.face_a, c.face_b))
    over, relief = None, None
    for d, pa, pb, fa, fb in clearance_map(lid, ring, below=2.5, spacing=3.0, limit=512):
        if pa[2] > z_p0 - 1e-3 and pa[2] < z_p0 + 0.2 and pb[2] > z_collar_top - 0.2:
            over = d if over == None else min(over, d)
        if pa[2] < z_th0 - 1.0 and pb[2] < z_th0 - 1.0:
            relief = d if relief == None else min(relief, d)
    assert_true(over != None and abs(over - PLATE_GAP) < 0.05, msg="the plate sits %s mm over the collar's top, not %s" % (over, PLATE_GAP))
    assert_true(relief != None and relief > 0.4 and relief < FINGER_LO, msg="the skirt stands %s mm off the ring's turned-down wall" % relief)
    lo, hi = lid.body().bbox()
    assert_close(hi[2] - lo[2], z_p1 - z_skirt0, tol=1e-4)
    assert_close(2 * hi[0], 2 * r_o, tol=0.1, msg="the lid is %s mm across" % (2 * r_o))
    return t


test("the lid threads on: 0.3 mm of clearance on the thread, the plate over the collar, the skirt off the wall", the_lid_threads_on)


def the_lid_retains_and_covers():
    c = distance(lid, sun)
    assert_close(c.distance, HUB_CL, tol=0.02 + 2 * chord, msg="the post runs in the hub with %s mm, not %s (%s/%s)" % (math.round(c.distance, 4), HUB_CL, c.face_a, c.face_b))
    for k in range(PLANETS):
        d = distance(lid, planet_bodies[k]).distance
        assert_true(d > 1.0 and d < FINGER_LO, msg="planet %d is %s mm under the lid: neither a finger's gap nor a graze" % (k, d))
    d = distance(lid, carrier).distance
    assert_true(d > 1.0 and d < FINGER_LO, msg="the pin heads are %s mm under the lid" % d)
    # Nothing that meshes is reachable through a window.
    assert_true(r_hub_cover > lay_sun["r_tip"] + 2.0, msg="the hub does not cover the sun's mesh")
    assert_true(r_win < lay_ring["r_tip"] - 2.0, msg="the annulus does not cover the ring's mesh")
    return c.distance


test("the lid retains the sun a second time and covers every mesh", the_lid_retains_and_covers)


def the_crank_and_its_knob():
    c = distance(crank, sun)
    assert_true(not c.overlap, msg="the crank and the post share material at %s" % (c.at,))
    assert_close(c.distance, SOCKET_CL, tol=0.03 + 2 * chord, msg="the socket runs on the post with %s mm, not %s (%s/%s)" % (math.round(c.distance, 4), SOCKET_CL, c.face_a, c.face_b))
    k = distance(knob, crank)
    running_gap(k, "the knob on its pin", tol=chord)
    float_gap, head_gap = None, None
    for d, pa, pb, fa, fb in clearance_map(knob, crank, below=1.0, spacing=2.0, limit=512):
        if pa[2] < z_arm1 + 1.0:
            float_gap = d if float_gap == None else min(float_gap, d)
        elif pa[2] > z_arm1 + KNOB_H - 6.0:
            head_gap = d if head_gap == None else min(head_gap, d)
    assert_close(float_gap, KNOB_CL, tol=0.02, msg="the knob floats %s mm over the arm, not %s" % (float_gap, KNOB_CL))
    assert_true(head_gap != None and head_gap - 2 * chord >= MIN_GAP, msg="the pin's head is %s mm from the knob's recess" % head_gap)
    # The knob is captive: its bore is narrower than the head that holds it.
    assert_true(PIN_R + KNOB_CL < PIN_R + HEAD_FLARE - 0.3, msg="the pin's head does not hold the knob")
    return k.distance


test("the crank fits the post with 0.35 mm and the knob runs on its pin with 0.4, captive under its head", the_crank_and_its_knob)


def safe_by_the_rules():
    # No opening anywhere between 6 mm and 12 mm.
    assert_true(r_win - r_hub_cover >= FINGER_HI, msg="the windows are %s mm wide, inside the finger band" % (r_win - r_hub_cover))
    assert_true(2 * SPOKE + 2 * WINDOW_FILLET >= FINGER_HI or 2 * SPOKE + 2 * WINDOW_FILLET < FINGER_LO, msg="the gap the two windows leave round the bar")
    assert_true(2 * (PIN_R + HEAD_FLARE + 1.5) >= FINGER_HI, msg="the knob's head recess is inside the finger band")
    assert_true(2 * SCREW_R < FINGER_LO, msg="the set screw's hole")
    assert_true(distance(crank, lid).distance >= FINGER_HI, msg="the arm stands inside the finger band over the lid")
    assert_true(distance(knob, lid).distance >= FINGER_HI, msg="the knob stands inside the finger band over the lid")
    assert_true(distance(knob, crank).distance < FINGER_LO)
    # The gap between the knob and the hub it turns beside, from the radii:
    # they are both the crank's, but the knob spins against the hub.
    assert_true(CRANK_R - KNOB_R - (HUB_R - 0.75) < FINGER_LO, msg="the knob stands %s mm from the hub, inside the finger band" % math.round(CRANK_R - KNOB_R - (HUB_R - 0.75), 2))
    assert_true(distance(lid, sun).distance < FINGER_LO, msg="the hub's hole round the post")
    assert_true(PLATE_GAP < FINGER_LO)
    # Small parts: nothing loose fits the cylinder, and the knob is over
    # 32 mm across whether or not it ever came off its pin.
    if print_layout:
        for b, what in [(lid_print, "the lid"), (crank_print, "the crank"), (knob_print, "the knob")]:
            assert_true(not fits_small_parts(b), msg="%s fits the small-parts cylinder" % what)
    assert_true(not fits_small_parts(ring.body()), msg="the ring fits the small-parts cylinder")
    assert_true(2 * KNOB_R >= 32.0, msg="the knob is under 32 mm across")
    return r_win - r_hub_cover


test("safe by the rules: no finger-sized opening, no reachable mesh, no small part, a captive knob", safe_by_the_rules)


def inv(angle):
    return math.tan(angle) - math.radians(angle)


def tip_land(teeth):
    """The tooth's thickness on its tip circle: the wall a slicer prints in
    the layer plane, where the wall-thickness rule reads the 62 deg corner
    at a helical tooth's end instead."""
    d = module * teeth / math.cos(HELIX)
    d_a = tip_diameter(module, teeth, helix_angle=HELIX)
    d_b = d * math.cos(alpha_t)
    s = module / math.cos(HELIX) * math.pi / 2 - backlash
    alpha_a = math.acos(d_b / d_a)
    return d_a * (s / d + inv(alpha_t) - inv(alpha_a))


def at_of(w):
    """The (x, y, z) a print finding names, or None."""
    i = w.find("at (")
    if i < 0:
        return None
    j = w.find(")", i)
    p = [float(v.strip()) for v in w[i + 4:j].split(",")]
    return (p[0], p[1], p[2])


def in_the_thread(pos, z_lo, z_hi):
    """A finding on a thread: within the crest band, over the thread's own
    height plus the pitch its run-out needs at each end."""
    if pos == None:
        return False
    r = math.hypot(pos[0], pos[1])
    return r_wall - WALL_RELIEF - 0.8 <= r and r <= r_wall + THREAD_CL + 0.8 and z_lo - PITCH <= pos[2] and pos[2] <= z_hi + PITCH


def judged(bodies, ends, z_lo, z_hi, what):
    """`print_check` on one print, with every finding classified. Nothing may
    fuse, no gap may miss the layer grid, nothing may print in the air and
    no chord error may eat a gap -- those are errors here. What is allowed,
    and why, is exactly two things:

      * a **thin wall** at a gear's end face, which is the acute corner
        where a helical flank meets it (90 - beta_b = 62 deg) read as a
        0.4 mm wall -- a corner across layers, not a wall in the layer
        plane, where the tip land is asserted from the closed form below;
      * a **thin wall or a fold on the thread's crest band**: a thread that
        runs out feathers its last crest to nothing, which is what every
        printed thread does, and the mesh boolean leaves a fin or two there
        where the run-out cuts the end face.

    Overhang is summed rather than forgiven by place: the whole print may
    carry `MAX_OVERHANG` mm2 of it, which is what the thread's run-out
    (about 1 mm2) and the teardropped tips of the two M3 cross-holes
    (about 3 mm2) come to. Every real face is at 40 deg or less.

    Anything else fails, with the finding quoted."""
    bad = []
    area = 0.0
    for r in print_check(bodies, printer=PRINTER, min_gap=MIN_GAP, layer_height=layer, min_wall=0.05):
        for e in r.errors:
            bad.append("%s: error: %s" % (r.profile, e))
        for w in r.warnings:
            pos = at_of(w)
            if w.find("of overhang past") > 0:
                area = area + float(w.split(" mm")[0])
                continue
            if in_the_thread(pos, z_lo, z_hi):
                continue
            if w.startswith("wall is") and pos != None and len(ends) > 0 and min([abs(pos[2] - e) for e in ends]) < layer:
                continue
            bad.append(w)
    assert_true(area <= MAX_OVERHANG, msg="%s: %s mm2 of overhang past 45 deg, over the %s mm2 the run-out and the two teardropped cross-holes account for" % (what, math.round(area, 2), MAX_OVERHANG))
    assert_true(len(bad) == 0, msg="%s: %s" % (what, " | ".join(bad)))
    return area


def crest_land():
    """What is left of the thread's crest between two turns, in mm: the
    pitch less the groove's opening. Two perimeters wide is the test."""
    return PITCH - PROFILE["top"]


def print_one_the_core():
    judged(core, [0.0, z_gears, z_gears + width, z_gear_top], z_th0, z_collar_top, "print 1")
    for teeth, what in [(TEETH_SUN, "sun"), (TEETH_PLANET, "planet")]:
        assert_true(tip_land(teeth) >= 2 * NOZZLE, msg="the %s's tip land is %s mm in the layer plane, under two perimeters" % (what, math.round(tip_land(teeth), 3)))
    assert_true(crest_land() >= 2 * NOZZLE, msg="the thread's crest land is %s mm, under two perimeters" % math.round(crest_land(), 3))
    return tip_land(TEETH_SUN)


test("print 1: the core is one print-in-place print -- nothing fuses, every vertical gap whole layers, nothing in the air", print_one_the_core)


def print_two_the_lid_and_the_crank():
    if not print_layout:
        return 0.0
    # The lid prints turned over, so its thread band is mirrored about z_p1.
    judged([lid_print, crank_print, knob_print], [], z_p1 - z_collar_top, z_p1 - z_th0, "print 2")
    lo, hi = lid_print.bbox()
    assert_close(lo[2], 0.0, tol=1e-4, msg="the lid prints plate down")
    lo, hi = crank_print.bbox()
    assert_close(lo[2], 0.0, tol=1e-4, msg="the crank prints on its arm")  # a mesh body's f32 vertices land within a micron of the bed
    return distance(lid_print, crank_print).distance


test("print 2: the lid plate down beside the crank with its captive knob, one print", print_two_the_lid_and_the_crank)


def gaps_at_this_size():
    """The gaps are printer numbers, not toy numbers: scaling the mechanism
    must not scale them."""
    worst = MAX_MESH_GAP
    for k in range(PLANETS):
        worst = min(worst, running_gap(distance(sun, planet_bodies[k]), "sun to planet %d" % k))
        worst = min(worst, running_gap(distance(planet_bodies[k], ring), "planet %d to the ring" % k, tol=chord))
        worst = min(worst, running_gap(distance(planet_bodies[k], carrier), "planet %d on its pin" % k))
    t = thread_clearance()
    assert_close(t, THREAD_GAP, tol=0.03, msg="the thread's gap on the normal is %s at this size" % t)
    assert_close(distance(lid, sun).distance, HUB_CL, tol=0.02 + 2 * chord)
    assert_close(distance(crank, sun).distance, SOCKET_CL, tol=0.03 + 2 * chord)
    running_gap(distance(knob, crank), "the knob on its pin", tol=chord)
    assert_true(r_win - r_hub_cover >= FINGER_HI, msg="the window band shrank into the finger range")
    assert_true(2 * KNOB_R >= 32.0)
    return min(worst, t)


test("the gaps are printer numbers: they hold when the toy is scaled", gaps_at_this_size, sweep={"kid_size": [0.9, 1.1]})


def in_mesh_at_this_angle():
    worst = MAX_MESH_GAP
    for k in range(PLANETS):
        worst = min(worst, running_gap(distance(sun, planet_bodies[k]), "sun to planet %d at %s deg" % (k, crank_angle)))
        worst = min(worst, running_gap(distance(planet_bodies[k], ring), "planet %d to the ring at %s deg" % (k, crank_angle), tol=chord))
    t = thread_clearance()
    assert_close(t, THREAD_GAP, tol=0.03, msg="the thread's gap on the normal at %s deg is %s" % (crank_angle, t))
    assert_true(distance(lid, sun).distance - 2 * chord > 0.3, msg="the post rubs the hub at %s deg" % crank_angle)
    return min(worst, t)


test("the crank turns through a tooth pitch: the meshes and the thread keep their gaps", in_mesh_at_this_angle, sweep={"crank_angle": [10, 20, 30]})


# The widest declared gap builds (0.6 met an edge that starts and ends at one
# point in the gear library's glue).
test("the widest gap", every_body_is_where_the_stack_puts_it, sweep={"gap": [0.5]})
