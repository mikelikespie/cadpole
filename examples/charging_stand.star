# A three-in-one magnetic charging stand: a weighted round base with a pad on
# it, a slim steel arm arcing up out of the back of it, a magnetic puck at the
# top angled to hold a phone, and a smaller one for a watch on a stem off the
# arm's side.
#
# **This is a product model, not a printable part.** Nothing here is checked
# against a nozzle or a bed; it is in `examples/` because the arm is the shape
# no other example has -- a tube that follows a curve through space.
#
# What this example demonstrates:
#
#   * **a tube along a curve, as a `loft`.** The arm is `segments + 1`
#     circles, each on its
#     own XY plane at that point's height and **centred on a quadratic
#     Bezier**, lofted. `loft` pairs loop 0 to loop 0 in construction order,
#     so the sections chain into one tube that follows the curve -- and since
#     each circle's radius falls with the parameter, the arm tapers as it
#     climbs. It is an exact B-rep body: no contouring, no cell size, no
#     mesh, and `cadpole check` calls it solid;
#   * **the same trick for a straight tube in an arbitrary direction.** The
#     watch stem is the two-section case: two circles on planes whose normal
#     *is* the stem's own direction, each centred on its plane's origin. That
#     is an exact cylinder along any axis, without composing two `rotate`s to
#     get there;
#   * **why this is a loft and not a field.** The same arm can be made of
#     `field.capsule`s chained along the Bezier and contoured with `mesh()`,
#     the construction `examples/soap_dish.star` uses for its ribs, but a
#     tapered chain has a radius step at every joint and a branch folds the
#     blend surface through itself, so the contour is not reliably a solid;
#   * **`rotate(body, angle, axis=(1,0,0))` to aim a face.** A puck is a disc
#     on +Z; the phone one has to end up with its face normal `tilt` degrees
#     above horizontal and pointing forward, which is a rotation of
#     `90 - tilt` about X. The same two lines with a different angle aim the
#     watch puck. Each puck's soft face is then placed *along that same
#     normal*, so changing the tilt moves the face with the puck instead of
#     leaving it hanging in space;
#   * **`loft` of concentric circles as a turned profile**, for the base and
#     its pad -- the construction `examples/table_lamp.star` arrives at from
#     the other direction, and for the same reason: a `revolve` of the
#     equivalent lathe profile has thin bands in it that the tessellator
#     drops;
#   * **four materials that have to be told apart by their roughness** rather
#     than by their colour: brushed steel, a machined puck rim, a soft-touch
#     pad and a silicone puck face are all grey.
#
# What the loft gives up is that its sections are **horizontal rather than
# perpendicular to the curve**: where the arm leans `a` from vertical its true
# cross-section is an ellipse squashed to `cos(a)` across the lean -- 0.82 at
# the tip, where the tangent is 35 degrees off vertical. A charging stand's
# arm is a flattened oval in life, so it reads correctly here; where it would
# not, `sweep_along` (see `examples/spring_and_bolt.star`) carries a profile
# perpendicular to its path.
#
# The proportions are those of a typical stand of this kind: a disc
# about 120 mm across and 13 mm thick with a slightly proud pad on top; the
# arm leaves the *back* of the base and arcs forward over it, so the puck ends
# up roughly above the base's centre rather than cantilevered past it; the
# phone puck is the larger of the two and sits at the very top; the watch puck
# is smaller, mounted part way down, out to the side, and faces much more
# nearly upward. The thicknesses, the 18-degree phone tilt and the 55-degree
# watch tilt are typical values.
#
#   cadpole render examples/charging_stand.star --backend path --samples 192 \
#       --size 1200x900 --view az=30,el=18 --environment -o /tmp/charging_stand.png
#   cadpole render examples/charging_stand.star --param segments=3 -o /tmp/faceted.png
#   cadpole check  examples/charging_stand.star

base_d = param("disc_diameter", 120, unit="mm", min=50, max=200, doc="The weighted disc the stand stands on")
base_h = param("disc_height", 13, unit="mm", min=5, max=40)
base_ease = param("base_ease", 2, unit="mm", min=0, max=6, doc="Chamfer round the base's top and bottom rims")
pad_d = param("pad_diameter", 100, unit="mm", min=20, max=190, doc="The proud pad on the base's top -- where earbuds go")
pad_h = param("pad_height", 2.5, unit="mm", min=0.5, max=10, doc="How far that pad stands proud: a step, not a dish")

arm_h = param("arm_height", 128, unit="mm", min=60, max=300, doc="Height of the arm's tip, where the phone puck mounts")
arm_back = param("arm_back", 34, unit="mm", min=0, max=90, doc="How far behind centre the arm leaves the base")
arm_reach = param("arm_reach", 34, unit="mm", min=0, max=120, doc="How far forward of that the tip ends up")
arm_r0 = param("arm_radius", 6.5, unit="mm", min=3, max=20, doc="Arm radius at the foot")
arm_r1 = param("arm_tip_radius", 4.5, unit="mm", min=2, max=20, doc="Arm radius at the tip")
segments = param("segments", 20, min=2, max=64, doc="Loft sections along the arc. Drop it to 4 to watch the curve become a polygon")

puck_d = param("puck_diameter", 56, unit="mm", min=25, max=120, doc="The phone puck")
puck_h = param("puck_height", 8, unit="mm", min=3, max=20)
tilt = param("tilt", 18, unit="deg", min=0, max=80, doc="How far the phone leans back from vertical")

watch_d = param("watch_diameter", 36, unit="mm", min=15, max=90)
watch_h = param("watch_height", 6, unit="mm", min=2, max=16)
watch_tilt = param("watch_tilt", 55, unit="deg", min=0, max=90, doc="The watch puck faces much more nearly upward than the phone one")
watch_at = param("watch_at", 0.46, min=0.05, max=0.95, doc="Where along the arm the watch stem branches off, 0 at the foot")
watch_out = param("watch_out", 30, unit="mm", min=10, max=90, doc="How far out to the side the watch puck sits")

assert_true(pad_d < base_d - 2 * base_ease, msg="the pad has to sit inside the base's top rim")
assert_true(watch_d < puck_d, msg="the watch puck is the smaller of the two")
assert_true(2 * arm_r1 < watch_d, msg="a watch puck narrower than the arm carrying it would be a knob")
assert_true(arm_reach < arm_back + base_d / 2, msg="the arm would reach past the front of its own base and tip it over")


def steel(body):
    """Brushed stainless. Polished harder than this and the tracer fireflies."""
    return body.material(color="#d2d6dc", roughness=0.2, metallic=1.0)


def disc(r, z, name):
    """One section of a turned solid: a circle at height `z`."""
    return sketch(XY.offset(z), name=name).circle(r)


# --- the base -----------------------------------------------------------------
# Four lofted circles: a chamfer up off the floor, the straight wall, a
# chamfer in at the top. A stack of exact conical frusta with flat caps, and
# no boolean. Its bodies and features are named `disc` rather than `base`
# because feature ids are document-global and
# `examples/nightstand_scene.star` loads this file alongside
# `table_lamp.star`, whose lamp base owns that name.
# With no ease the base is the straight wall alone: the chamfer sections
# would sit on the wall's, bands of no height.
assert_true(2 * base_ease < base_h, msg="the base's two chamfers take its whole height: keep base_ease under half disc_height")
base_r = base_d / 2.0
base = loft([
    disc(base_r - base_ease, 0.0, "disc_floor"),
    disc(base_r, base_ease, "disc_lower"),
    disc(base_r, base_h - base_ease, "disc_upper"),
    disc(base_r - base_ease, base_h, "disc_top"),
] if base_ease > 0 else [
    disc(base_r, 0.0, "disc_lower"),
    disc(base_r, base_h, "disc_upper"),
], name="disc")
base.material(color="#33363a", roughness=0.5)

# The pad: a second body standing `pad_h` proud, a visible step on the disc.
# Soft-touch, so it is its
# *roughness* that tells it apart from the base and not its colour.
pad = loft([
    disc(pad_d / 2.0, base_h, "pad_bottom"),
    disc(pad_d / 2.0 - 0.8, base_h + pad_h, "pad_top"),
], name="pad")
pad.material(color="#2b2e32", roughness=0.8)

# --- the arm ------------------------------------------------------------------
# A quadratic Bezier in the YZ plane. +Y is towards the wall, so the arm
# leaves the base at +`arm_back`, rises almost straight -- the control point
# is directly above the foot, which is what makes the bottom vertical -- and
# turns forward into the tip.
foot = (0.0, arm_back, base_h * 0.45)
elbow = (0.0, arm_back, arm_h * 0.62)
tip = (0.0, arm_back - arm_reach, arm_h)


def bezier(t):
    """The point at parameter `t` on the quadratic through foot/elbow/tip."""
    u = 1.0 - t
    a, b, c = u * u, 2.0 * u * t, t * t
    return (a * foot[0] + b * elbow[0] + c * tip[0],
            a * foot[1] + b * elbow[1] + c * tip[1],
            a * foot[2] + b * elbow[2] + c * tip[2])


# The arm is one `loft` through `segments + 1` circles, each on its own XY
# plane at that point's height and **centred on the curve**. `loft` pairs loop
# 0 to loop 0 in construction order, so the sections chain into one tube that
# follows the Bezier, and because the sections are circles of a radius that
# falls with t, the arm tapers as it climbs. It is an exact B-rep body: no
# contouring, no cell size, no mesh.
#
# The sections are horizontal rather than perpendicular to the curve, which is
# the one compromise. Where the arm leans `a` from vertical its true
# cross-section is an ellipse squashed to `cos(a)` across the lean -- 0.82 at
# the tip, where the tangent is 35 degrees off vertical. A charging stand's
# arm is a flattened oval in real life, so this reads correctly.
def arm_radius(t):
    """The arm thins as it climbs, which is what stops it reading as pipe."""
    return math.lerp(arm_r0, arm_r1, t)


def arm_section(i):
    t = float(i) / segments
    p = bezier(t)
    face = plane(origin=(p[0], p[1], p[2]), normal=(0, 0, 1), x_dir=(1, 0, 0))
    return sketch(face, name="arm_section_%d" % i).circle(arm_radius(t))


arm = loft([arm_section(i) for i in range(segments + 1)], name="arm")
assert_solid(arm, msg="the swept arm is a closed solid")
steel(arm)

alo, ahi = arm.bbox()
assert_true(math.close(ahi[2], arm_h, tol=0.5), msg="the arm reaches `arm_height`")
assert_true(alo[1] < 0 and ahi[1] > 0, msg="the arm arcs from behind the base's centre to in front of it")
assert_true(math.close(ahi[0] - alo[0], 2 * arm_r0, tol=0.5), msg="the arm is widest at its foot")

# The watch stem is straight, so it is the two-section case of the same idea:
# two circles on planes that share the stem's own direction as their normal,
# each centred on its plane's origin. That is an exact cylinder along an
# arbitrary axis, without composing two `rotate`s to get there.
stem_root = bezier(watch_at)
stem_tip = (watch_out, stem_root[1] - 5.0, stem_root[2] - 8.0)
stem_dir = (stem_tip[0] - stem_root[0], stem_tip[1] - stem_root[1], stem_tip[2] - stem_root[2])
stem_r = arm_r1 * 0.85
stem = loft([
    sketch(plane(origin=stem_root, normal=stem_dir, x_dir=(0, 0, 1)), name="stem_root").circle(stem_r),
    sketch(plane(origin=stem_tip, normal=stem_dir, x_dir=(0, 0, 1)), name="stem_tip").circle(stem_r),
], name="watch_stem")
assert_solid(stem, msg="the watch stem is a closed solid")
steel(stem)

# --- the pucks ----------------------------------------------------------------
# Aiming a puck is one rotation about X: turning +Z by `90 - angle` lands the
# axis on `(0, -cos angle, sin angle)`, which points forward and `angle`
# degrees above horizontal -- exactly the face a phone leaning `tilt` back
# from vertical rests against.
def face_normal(angle):
    return (0.0, -math.cos(angle), math.sin(angle))


def puck(at, angle, d, h, rim_name, face_name):
    """A disc `d` across and `h` thick, centred on `at`, facing `angle` above
    horizontal. Built centred on the origin so the rotation needs no origin of
    its own, then translated into place."""
    n = face_normal(angle)
    body = cylinder(d / 2.0, h, at=(0, 0, -h / 2.0), name=rim_name + "_blank")
    body = rotate(body, 90.0 - angle, axis=(1, 0, 0), name=rim_name + "_aim")
    body = translate(body, at, name=rim_name)
    steel(body)
    # The magnet face: a shallow silicone disc on the front, offset along the
    # puck's own normal so it stays on the front whatever the tilt is.
    skin = 1.4
    face_at = (at[0] + n[0] * h / 2.0, at[1] + n[1] * h / 2.0, at[2] + n[2] * h / 2.0)
    face = cylinder(d / 2.0 - 3.0, skin, at=(0, 0, -skin / 2.0), name=face_name + "_blank")
    face = rotate(face, 90.0 - angle, axis=(1, 0, 0), name=face_name + "_aim")
    face = translate(face, face_at, name=face_name)
    face.material(color="#3b3e43", roughness=0.85)
    return body, face


# The phone puck sits just forward of the arm's tip, along its own normal, so
# the tip is buried in its back rather than standing beside it.
pn = face_normal(tilt)
phone_at = (tip[0], tip[1] + pn[1] * 2.0, tip[2] + pn[2] * 2.0)
phone_puck, phone_face = puck(phone_at, tilt, puck_d, puck_h, "phone_puck", "phone_face")

wn = face_normal(watch_tilt)
watch_centre = (stem_tip[0] + wn[0] * 2.0, stem_tip[1] + wn[1] * 2.0, stem_tip[2] + wn[2] * 2.0)
watch_puck, watch_face = puck(watch_centre, watch_tilt, watch_d, watch_h, "watch_puck", "watch_face")

# --- checks against the reference proportions ---------------------------------
plo, phi = phone_puck.bbox()
wlo, whi = watch_puck.bbox()
blo, bhi = base.bbox()

assert_true(math.close(bhi[0] - blo[0], base_d, tol=0.6), msg="the base is `disc_diameter` across")
assert_true(math.close(bhi[2], base_h, tol=0.01), msg="and `disc_height` thick")
assert_true(phi[2] > whi[2], msg="the phone puck is the higher of the two")
assert_true(phi[0] - plo[0] > whi[0] - wlo[0], msg="and the larger")
assert_true(whi[0] > bhi[0] * 0.7, msg="the watch puck hangs out to the side")
# The phone puck ends up over the base rather than beyond it, which is what
# `arm_reach` is for: a stand whose puck overhangs its own base tips over.
assert_true(plo[1] > blo[1] and phi[1] < bhi[1], msg="the phone puck stays inside the base's footprint")
# And the faces really are faces: the phone's is nearly upright, so it is
# taller than it is deep, while the watch's -- nearly flat -- is not.
assert_true(phi[2] - plo[2] > phi[1] - plo[1], msg="the phone puck stands up")
assert_true(whi[1] - wlo[1] > whi[2] - wlo[2], msg="the watch puck lies back")

# Every body of the stand, in one list, so a scene script can `load()` this
# file and place the whole thing -- `translate` consumes what it moves, so a
# caller needs a handle on all of them or the originals stay at the origin.
# `STAND_HEIGHT` is what a caller has to clear above it.
STAND = [base, pad, arm, stem, phone_puck, phone_face, watch_puck, watch_face]
STAND_HEIGHT = phi[2]

export([base, pad, phone_puck, watch_puck], "charging_stand.step")


# The ends of the declared ranges build: no chamfer at all, and the widest.
def the_base_is_solid():
    assert_solid(base)


test("the base at the ends of base_ease", the_base_is_solid, sweep={"base_ease": [0, 6]})
