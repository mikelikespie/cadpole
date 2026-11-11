# A mechanical iris: N sickle blades on fixed pivot posts, driven by one
# actuator ring. Turn the ring and every blade swings in together until the
# aperture closes to a point and overlaps; turn it back and the blades tuck
# away behind the lip. Every dimension follows from four numbers -- the blade
# count, the open aperture, the opening and the printer's layer -- and the
# tests sweep the blade count and the opening to prove it closes, opens and
# never collides anywhere in its travel.
#
#   cadpole render  iris.star --view az=-30,el=50 -o iris.png
#   cadpole render  iris.star --param opening=0.4 -o half.png
#   cadpole animate iris.star -o iris.gif            # closes and opens, looping
#   cadpole test    iris.star                        # blade-count x opening sweep
#   cadpole check   iris.star --printer bambu_h2d    # the stack as one print
#
# How it works (all of it is plain geometry, no approximation):
#
#   * Blade k pivots on a post at P_k, radius `Rp` from the axis, at
#     angle 360 k / N. Its inner (cutting) edge is an arc of radius `Ri`
#     about the axis when the iris is fully open, so open it sits just
#     behind the lip of the housing (radius aperture / 2).
#   * Swinging a blade by `beta` about its post carries that arc's centre
#     from the axis to a point 2 Rp sin(beta / 2) away, so the round hole
#     left between the N arcs -- the aperture -- has radius
#         r = Ri - 2 Rp sin(beta / 2).
#     The model inverts that: `opening` sets r = lerp(-overlap, Ri, opening)
#     and each blade's pose is beta = 2 asin((Ri - r) / (2 Rp)). At
#     opening = 0 the edges have gone `overlap` past the axis: shut.
#   * Each blade carries a drive pin on a short lever, `lever` from its
#     pivot, standing up into a radial slot in the actuator ring. A pin in
#     a radial slot sits at the slot's polar angle, so the ring's angle is
#     exactly the change in the pin's polar angle:
#         D(beta) = P + Rot(beta) (D0 - P),  phi = atan2(D(beta)) - atan2(D0).
#     The lever starts at -beta_max / 2 from the radial, so the pin swings
#     symmetrically about the radial and never leaves the ring band.
#   * The blades are flat plates stacked helically: blade k floats k
#     whole-layer pitches up the posts, `gap_layers` (two layers) clear of
#     its neighbours, so N overlapping sickles never touch.
#
# Printing: every part is a flat print as it stands -- the housing with its
# posts up, each blade with its drive pin up, the ring flat -- and assembles
# by dropping the blades on the posts in order and the ring over the pins.
# `cadpole check` also passes the assembled stack as one print (no fused pair,
# every horizontal gap a whole number of layers). The blade-and-pin union
# runs on the mesh kernel on purpose: posed B-rep blades hung the evaluator
# at some angles, the mesh blades do not, at every opening tried.
#
# Units are millimetres.

blades = param("blades", 8, min=5, max=12, step=1, doc="Number of blades")
aperture = param("aperture", 38.0, unit="mm", min=30, max=46, doc="Open aperture diameter: the lip of the housing. The pivot circle, the blades and the outside diameter all scale from it")
opening = param("opening", 1.0, min=0, max=1, doc="0 = shut (the blades overlap past the centre), 1 = fully open (the blades hidden behind the lip). Linear in the aperture's diameter")
layer_h = param("layer", 0.2, unit="mm", min=0.1, max=0.3, doc="Printer layer height; every vertical gap and plate is a whole number of these")
blade_layers = param("blade_layers", 5, min=3, max=8, step=1, doc="Blade thickness in layers")
gap_layers = param("gap_layers", 2, min=1, max=4, step=1, doc="Vertical gap between stacked blades, in layers")
pin_clear = param("pin_clearance", 0.35, unit="mm", min=0.15, max=0.6, doc="Radial clearance of every pin in its hole or slot")
post_d = param("post", 3.0, unit="mm", min=2, max=5, doc="Pivot post and drive pin diameter")
lever = param("lever", 6.5, unit="mm", min=4, max=10, doc="Pivot-to-drive-pin distance on each blade")
overlap = param("overlap", 1.5, unit="mm", min=0, max=3, doc="How far past the centre the blade edges travel when shut")
base_layers = param("base_layers", 10, min=6, max=20, step=1, doc="Base plate and actuator ring thickness, in layers")
finish = param("finish", "filament", choices=["filament", "metal"], doc="Colourway: printed filament (a warm gradient of blades, cream ring, white housing) or machined metal (anodised blades, brass ring, aluminium housing)")
wall = param("wall", 2.4, unit="mm", min=1.6, max=4, doc="Housing wall thickness")

N = blades
tb = blade_layers * layer_h          # blade plate
gap = gap_layers * layer_h           # blade-to-blade and ring-to-blade gap
pitch = tb + gap
plate = base_layers * layer_h        # base plate and actuator ring
pr = post_d / 2                      # post and pin radius

# --- the plan geometry, all from the aperture --------------------------------
Ra = aperture / 2                    # the housing lip
Ri = Ra + 1.2                        # blade cutting edge, open: hidden behind the lip
Rp = 1.0 * aperture                 # pivot circle: wide blades cover the hole shut
Ro = Rp - pr - 0.9                   # blade outer edge, clear of every other post
w = Ro - Ri                          # blade width
PHI0 = -4.0                          # blade sector, degrees about the axis from its own pivot
PHI1 = 132.0
beta_max = 2 * math.asin((Ri + overlap) / (2 * Rp))
alpha = -beta_max / 2                # lever direction from the radial at the pivot
boss = pr + pin_clear + 1.6          # material round the pivot hole
pin_boss = pr + 1.1                  # material round the drive pin's foot
D0 = (Rp + lever * math.cos(alpha), lever * math.sin(alpha))
Rw = Rp + lever + pin_boss + 0.6     # housing wall, inside
Rout = Rw + wall

# --- the kinematics ----------------------------------------------------------
r_ap = math.lerp(-overlap, Ri, opening)          # aperture radius (negative: overlapped)
beta = 2 * math.asin((Ri - r_ap) / (2 * Rp))      # every blade's swing about its post


def rot(p, a, c=(0.0, 0.0)):
    """Point `p` rotated by `a` degrees about `c`."""
    x, y = p[0] - c[0], p[1] - c[1]
    return (c[0] + x * math.cos(a) - y * math.sin(a), c[1] + x * math.sin(a) + y * math.cos(a))


def drive_pin(b):
    """The drive pin of blade 0, in its pivot frame, after a swing of `b`."""
    return rot(D0, b, (Rp, 0.0))


phi = math.atan2(drive_pin(beta)[1], drive_pin(beta)[0]) - math.atan2(D0[1], D0[0])
phi_max = math.atan2(drive_pin(beta_max)[1], drive_pin(beta_max)[0]) - math.atan2(D0[1], D0[0])

# --- heights -----------------------------------------------------------------
def blade_z(k):
    return plate + gap + k * pitch


stack_top = blade_z(N - 1) + tb      # top of the top blade = top of the posts
act_z = stack_top + gap              # actuator ring floats two layers over the stack and the wall
act_top = act_z + plate
pin_top = act_top + 0.6              # pins stand proud of the ring


# --- 2D helpers ----------------------------------------------------------------
def polar(r, a):
    return (r * math.cos(a), r * math.sin(a))


def capsule(a, b, r):
    """A stadium from point a to point b, radius r, as a sketch loop."""
    L = math.hypot(b[0] - a[0], b[1] - a[1])
    u = ((b[0] - a[0]) / L, (b[1] - a[1]) / L)
    n = (-u[1], u[0])
    p = path((a[0] + r * n[0], a[1] + r * n[1]))
    p = p.line((b[0] + r * n[0], b[1] + r * n[1]))
    p = p.arc((b[0] - r * n[0], b[1] - r * n[1]), via=(b[0] + r * u[0], b[1] + r * u[1]))
    p = p.line((a[0] - r * n[0], a[1] - r * n[1]))
    p = p.arc((a[0] + r * n[0], a[1] + r * n[1]), via=(a[0] - r * u[0], a[1] - r * u[1]))
    return sketch().add(p)


def sickle_path():
    """The blade's sickle: the annular sector Ri..Ro over PHI0..PHI1, round at both ends."""
    rm = (Ri + Ro) / 2
    hw = w / 2
    mid = (PHI0 + PHI1) / 2
    p = path(polar(Ri, PHI0))
    p = p.arc(polar(Ri, PHI1), via=polar(Ri, mid))
    t1 = polar(rm, PHI1)
    p = p.arc(polar(Ro, PHI1), via=(t1[0] - hw * math.sin(PHI1), t1[1] + hw * math.cos(PHI1)))
    p = p.arc(polar(Ro, PHI0), via=polar(Ro, mid))
    t0 = polar(rm, PHI0)
    p = p.arc(polar(Ri, PHI0), via=(t0[0] + hw * math.sin(PHI0), t0[1] - hw * math.cos(PHI0)))
    return p


def blade_sketch():
    s = sketch().add(sickle_path())
    s = s.union(capsule((Rp, 0.0), D0, pin_boss))
    s = s.union(sketch().circle(boss, center=(Rp, 0.0)))
    return s.subtract(sketch().circle(pr + pin_clear, center=(Rp, 0.0)))


def blade_outline(b, k):
    """Blade k's sickle as a sampled polygon, swung by `b`: for the 2D coverage tests."""
    pts = []
    M = 48
    for i in range(M + 1):
        pts.append(polar(Ri, PHI0 + (PHI1 - PHI0) * i / M))
    for i in range(M + 1):
        pts.append(polar(Ro, PHI1 - (PHI1 - PHI0) * i / M))
    return [rot(rot(q, b, (Rp, 0.0)), 360.0 * k / N) for q in pts]


# --- colours -----------------------------------------------------------------
def mix(c0, c1, t):
    a = [int(c0[i:i + 2], 16) for i in (1, 3, 5)]
    b = [int(c1[i:i + 2], 16) for i in (1, 3, 5)]
    out = "#"
    for i in range(3):
        v = int(math.round(a[i] + (b[i] - a[i]) * t))
        out += "0123456789abcdef"[v // 16] + "0123456789abcdef"[v % 16]
    return out


def blade_material(body, k):
    if finish == "metal":
        body.material(color="#7b8696", metallic=0.75, roughness=0.3)
    else:
        body.material(preset="pla_silk", color=mix("#e8452c", "#f6c445", k / max(1, N - 1)))


def ring_material(body):
    if finish == "metal":
        body.material(preset="brass", color="#c8a35a", roughness=0.22)
    else:
        body.material(preset="pla_matte", color="#e9e2d0")


def housing_material(body):
    if finish == "metal":
        body.material(preset="aluminum", color="#9aa1a9", roughness=0.35)
    else:
        body.material(preset="pla_matte", color="#e9e6df")


# --- the blades --------------------------------------------------------------
# Each blade is built in its own pivot frame (post on +x) at its own level, with
# its drive pin reaching up through the levels above into the actuator's slot.
blade_bodies = []
blade_insts = []
for k in range(N):
    z = blade_z(k)
    b = extrude(blade_sketch(), tb, name="blade_plate_%d" % k).translate((0, 0, z))
    pin = cylinder(pr, pin_top - z - 0.3, at=(D0[0], D0[1], z + 0.3), name="drive_pin_%d" % k)
    b = union(b, pin, name="blade_body_%d" % k, kernel="mesh")
    blade_material(b, k)
    blade_bodies.append(b)
for k in range(N):
    th = 360.0 * k / N
    inst = instance(part(blade_bodies[k], name="blade_part_%d" % k), rotate=th, name="blade_%d" % k)
    blade_insts.append(inst)

# --- the base: lip plate, wall and pivot posts -----------------------------
# One revolved L-section -- plate and wall -- so the exact kernel never meets
# two coaxial faces; the posts stand in it from half a millimetre down.
section = sketch(XZ, name="housing_section").polygon([
    (Ra, 0), (Rout, 0), (Rout, stack_top), (Rw, stack_top), (Rw, plate), (Ra, plate),
])
base = revolve(section, axis=(0, 0, 1), name="housing_shell")
posts = []
for k in range(N):
    c = polar(Rp, 360.0 * k / N)
    posts.append(cylinder(pr, stack_top - plate + 0.5, at=(c[0], c[1], plate - 0.5), name="post_%d" % k))
base = union([base] + posts, name="housing_body")
housing_material(base)
housing = instance(part(base, name="housing_part"), name="housing")

# --- the actuator ring: lip, drive slots, windows and a scalloped grip --------
# The slots are radial, as long as the drive pin's radius varies over the
# whole travel; in this ring's own frame pin k sits at the polar angle of D0_k.
# The ring overhangs the housing by a finger's grip, scalloped all round, with
# windows between the spokes.
rs = [math.hypot(drive_pin(beta_max * i / 16)[0], drive_pin(beta_max * i / 16)[1]) for i in range(17)]
r_lo, r_hi = min(rs), max(rs)
a0 = math.atan2(D0[1], D0[0])
R_grip = Rout + 1.6
SCALLOPS = 40
act_s = sketch().circle(R_grip).circle(Ra)
for k in range(N):
    a = a0 + 360.0 * k / N
    act_s = act_s.subtract(capsule(polar(r_lo - 0.01, a), polar(r_hi + 0.01, a), pr + pin_clear))
for i in range(SCALLOPS):
    act_s = act_s.subtract(sketch().circle(1.5, center=polar(R_grip + 0.5, 360.0 * (i + 0.5) / SCALLOPS)))
# Windows between N spokes show the blades and posts at work under the ring.
r_win_in = Ra + 2.4
r_win_out = r_lo - pr - pin_clear - 1.8
spoke = 4.0
win = sketch().circle(r_win_out).circle(r_win_in)
for k in range(N):
    a = a0 + 360.0 * k / N
    win = win.subtract(capsule(polar(r_win_in - 2, a), polar(r_win_out + 2, a), spoke / 2))
win = fillet2d(win, 1.2)
act_s = act_s.subtract(win)
act = extrude(act_s, plate, name="actuator_plate").translate((0, 0, act_z), name="actuator")
ring_material(act)
actuator = instance(part(act, name="actuator_part"), name="actuator_ring", layer=layer("actuator", doc="The actuator ring; hide it to watch the blades"))

# --- the pose: the iris at `opening` -------------------------------------------
for k in range(N):
    if beta:
        pose(blade_insts[k], rotate=beta, origin=(Rp * math.cos(360.0 * k / N), Rp * math.sin(360.0 * k / N), 0))
if phi:
    pose(actuator, rotate=phi)

iris = assembly([housing] + blade_insts + [actuator], name="iris")
export(iris, "iris.3mf")

report("aperture_diameter_mm", max(0.0, 2 * r_ap))
report("blade_swing_deg", beta)
report("ring_turn_deg", phi)
report("outside_diameter_mm", 2 * Rout)
report("height_mm", act_top)

# --- the animation: shut, open, shut, looping --------------------------------
animation("iris", [
    keyframe(0, opening=1.0),
    keyframe(0.6, opening=1.0),
    keyframe(2.4, opening=0.0),
    keyframe(3.0, opening=0.0),
    keyframe(4.8, opening=1.0),
], fps=24, loop=True)


# --- tests -------------------------------------------------------------------
def disk(r):
    return sketch().circle(r)


def covered_area(b):
    """Area of the housing's hole that the N sickles cover, swung by `b`."""
    u = sketch().polygon(blade_outline(b, 0))
    for k in range(1, N):
        u = u.union(sketch().polygon(blade_outline(b, k)))
    return u.intersect(disk(Ra)).area()


def shut_means_shut():
    hole = disk(Ra).area()
    got = covered_area(beta_max)
    assert_true(got > hole * (1 - 1e-4), msg="shut, the blades leave %s mm2 of the %s mm2 hole open" % (math.round(hole - got, 3), math.round(hole, 1)))


test("shut, the blades cover the whole hole", shut_means_shut, sweep={"blades": [5, 6, 7, 8, 9, 10, 11, 12], "aperture": [30, 38, 46]})


def open_means_hidden():
    assert_true(covered_area(0.0) == 0.0, msg="open, a blade shows inside the lip")


test("open, every blade is hidden behind the lip", open_means_hidden, sweep={"blades": [5, 8, 12]})


def aperture_is_the_formula():
    # A probe cylinder a hair smaller than the formula's aperture clears every
    # blade by that hair, and one a hair larger cuts into one: the hole really
    # is r = Ri - 2 Rp sin(beta/2).
    if r_ap < 1.0:
        return
    worst = 1e9
    probe = cylinder(r_ap - 0.2, stack_top - plate, at=(0, 0, plate), name="aperture_probe")
    for k in range(N):
        c = distance(probe, blade_insts[k].body())
        worst = min(worst, c.distance)
    assert_close(worst, 0.2, tol=0.06, msg="the aperture's radius is not the formula's %s" % r_ap)


test("the hole is exactly the aperture the formula gives", aperture_is_the_formula, sweep={"blades": [5, 8, 12], "opening": [0.25, 0.5, 0.75, 1.0]})


def nothing_collides():
    # Every moving body against every other, at this point of the travel: the
    # blades clear each other, the posts, the pins, the wall and the ring.
    bodies = [housing.body()] + [i.body() for i in blade_insts] + [actuator.body()]
    worst = 1e9
    for i in range(len(bodies)):
        for j in range(i + 1, len(bodies)):
            c = distance(bodies[i], bodies[j])
            assert_true(not c.overlap and c.distance > 0.1, msg="%s and %s are %s mm apart at %s" % (bodies[i].name, bodies[j].name, math.round(c.distance, 3), c.at))
            worst = min(worst, c.distance)
    return worst


test("nothing collides anywhere in the travel", nothing_collides, sweep={"blades": [5, 8, 12], "opening": [0.0, 0.33, 0.66, 1.0]})


def prints_as_one_stack():
    # The assembled stack as one print: no two bodies closer than the fuse
    # gap, every horizontal gap a whole number of layers, nothing in the air.
    bodies = [housing.body()] + [i.body() for i in blade_insts] + [actuator.body()]
    assert_printable(bodies, layer_height=layer_h, min_gap=0.25, min_wall=0.75, printer="bambu_h2d")


test("the stack is printable: whole-layer gaps, nothing fused", prints_as_one_stack, sweep={"blades": [5, 8, 12]})


def ring_turns_monotonically():
    return phi


test("the ring turns further as the iris opens less", ring_turns_monotonically, sweep={"opening": [1.0, 0.75, 0.5, 0.25, 0.0]}, monotone="increasing")

# Shut to open at an even rate, one frame per step of `opening`.
animation("scroll_story", [
    keyframe(0.0, opening=0.0, ease=ease.linear, camera=view(az=-24, el=58, fov=26)),
    keyframe(2.0, opening=1.0, camera=view(az=-24, el=58, fov=26)),
], fps=24)
