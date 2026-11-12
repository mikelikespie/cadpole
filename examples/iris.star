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
#   * Each blade carries a drive pin on a lever, `lever` from its pivot and
#     45 degrees off the radial, so as the blade swings the pin moves mostly
#     inward. The ring turns by psi = psi_max (1 - opening): linear in the
#     opening, so the scale on the housing is evenly spaced. Each pin rides
#     in a cam slot cut along its own path as the ring sees it,
#         S(opening) = Rot(-psi) (P + Rot(beta) (D0 - P)),
#     a shallow spiral. The ring turns several times further than it would
#     in a radial slot, so a small turn of the hand is a fine change of
#     aperture, and the tests check that each slot crosses its pin's path
#     steeply enough to drive it.
#   * The blades are flat plates stacked helically: blade k floats k
#     whole-layer pitches up the posts, `gap_layers` (two layers) clear of
#     its neighbours, so N overlapping sickles never touch. Each blade
#     tapers from a broad root, blended into its pivot boss and lever with
#     fillets, to a narrow rounded tip.
#
# Printing: every part is a flat print as it stands -- the housing with its
# posts up, each blade with its drive pin up, the ring flat -- and assembles
# by dropping the blades on the posts in order and the ring over the pins.
# `cadpole check` also passes the assembled stack as one print (no fused pair,
# every horizontal gap a whole number of layers).
#
# Units are millimetres.

blades = param("blades", 8, min=5, max=12, step=1, doc="Number of blades")
aperture = param("aperture", 38.0, unit="mm", min=30, max=46, doc="Open aperture diameter: the lip of the housing. The pivot circle, the blades and the outside diameter all scale from it")
opening = param("opening", 1.0, min=0, max=1, doc="0 = shut (the blades overlap past the centre), 1 = fully open (the blades hidden behind the lip). Linear in the aperture's diameter and in the ring's turn")
layer_h = param("layer", 0.2, unit="mm", min=0.1, max=0.3, doc="Printer layer height; every vertical gap and plate is a whole number of these")
blade_layers = param("blade_layers", 5, min=3, max=8, step=1, doc="Blade thickness in layers")
gap_layers = param("gap_layers", 2, min=1, max=4, step=1, doc="Vertical gap between stacked blades, in layers")
pin_clear = param("pin_clearance", 0.35, unit="mm", min=0.15, max=0.6, doc="Radial clearance of every pin in its hole or slot")
post_d = param("post", 3.0, unit="mm", min=2, max=5, doc="Pivot post and drive pin diameter")
lever = param("lever", 6.5, unit="mm", min=4, max=10, doc="Pivot-to-drive-pin distance on each blade")
overlap = param("overlap", 1.5, unit="mm", min=0, max=3, doc="How far past the centre the blade edges travel when shut")
base_layers = param("base_layers", 10, min=6, max=20, step=1, doc="Base plate thickness, in layers")
ring_layers = param("ring_layers", 14, min=6, max=24, step=1, doc="Actuator ring thickness, in layers")
finish = param("finish", "filament", choices=["filament", "metal"], doc="Colourway: printed filament (a warm gradient of blades, cream ring, slate housing) or machined metal (anodised blades, brass ring, aluminium housing)")
wall = param("wall", 2.4, unit="mm", min=1.6, max=4, doc="Housing wall thickness")

N = blades
tb = blade_layers * layer_h          # blade plate
gap = gap_layers * layer_h           # blade-to-blade and ring-to-blade gap
pitch = tb + gap
plate = base_layers * layer_h        # base plate
ring_t = ring_layers * layer_h       # actuator ring
pr = post_d / 2                      # post and pin radius

# --- the plan geometry, all from the aperture --------------------------------
Ra = aperture / 2                    # the housing lip
Ri = Ra + 1.2                        # blade cutting edge, open: hidden behind the lip
Rp = 1.0 * aperture                  # pivot circle: wide blades cover the hole shut
Ro = Rp - pr - 0.9                   # blade back edge at its widest, clear of every other post
w = Ro - Ri                          # blade width at the root
w_tip = w * min(1.0, 0.6 + 0.15 * max(0, 8 - N))   # ... and at the tip: fewer blades need wider tips to cover
PHI0 = -4.0                          # blade sector, degrees about the axis from its own pivot
PHI1 = max(92.0, 720.0 / N)          # long enough that N blades cover the hole when shut
LEVER_DEG = 45.0                     # lever direction from the radial at the pivot, open
beta_max = 2 * math.asin((Ri + overlap) / (2 * Rp))
psi_max = min(45.0, 0.8 * 360.0 / N) # the ring's whole turn: most of a blade pitch
boss = pr + pin_clear + 1.6          # material round the pivot hole
pin_boss = pr + 1.1                  # material round the drive pin's foot
D0 = (Rp + lever * math.cos(LEVER_DEG), lever * math.sin(LEVER_DEG))


# --- the kinematics ----------------------------------------------------------
def rot(p, a, c=(0.0, 0.0)):
    """Point `p` rotated by `a` degrees about `c`."""
    x, y = p[0] - c[0], p[1] - c[1]
    return (c[0] + x * math.cos(a) - y * math.sin(a), c[1] + x * math.sin(a) + y * math.cos(a))


def aperture_r(o):
    return math.lerp(-overlap, Ri, o)                    # negative: overlapped


def beta_at(o):
    return 2 * math.asin((Ri - aperture_r(o)) / (2 * Rp))   # every blade's swing about its post


def psi_at(o):
    return psi_max * (1 - o)                             # the ring's turn


def drive_pin(b):
    """The drive pin of blade 0, in its pivot frame, after a swing of `b`."""
    return rot(D0, b, (Rp, 0.0))


def slot_point(o):
    """Blade 0's drive pin at opening `o`, in the ring's own frame."""
    return rot(drive_pin(beta_at(o)), -psi_at(o))


r_ap = aperture_r(opening)
beta = beta_at(opening)
psi = psi_at(opening)

SLOT_M = 24
slot_pts = [slot_point(1.0 - i / SLOT_M) for i in range(SLOT_M + 1)]   # open to shut
rs = [math.hypot(q[0], q[1]) for q in slot_pts]
r_lo, r_hi = min(rs), max(rs)
Rw = r_hi + pin_boss + 0.8           # housing wall, inside: clear of every lever
Rout = Rw + wall


# --- heights -----------------------------------------------------------------
def blade_z(k):
    return plate + gap + k * pitch


stack_top = blade_z(N - 1) + tb      # top of the top blade = top of the posts
act_z = stack_top + gap              # actuator ring floats two layers over the stack and the wall
act_top = act_z + ring_t
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


def swept_slot(pts, h):
    """A slot of half-width `h` along the polyline `pts`: both walls are
    splines through the centre line's offsets, closed by round ends."""
    M = len(pts) - 1
    tang = []
    for i in range(M + 1):
        a = pts[max(0, i - 1)]
        b = pts[min(M, i + 1)]
        L = math.hypot(b[0] - a[0], b[1] - a[1])
        tang.append(((b[0] - a[0]) / L, (b[1] - a[1]) / L))
    left = [(pts[i][0] - h * tang[i][1], pts[i][1] + h * tang[i][0]) for i in range(M + 1)]
    right = [(pts[i][0] + h * tang[i][1], pts[i][1] - h * tang[i][0]) for i in range(M + 1)]
    p = path(left[0]).spline(left[1:])
    p = p.arc(right[M], via=(pts[M][0] + h * tang[M][0], pts[M][1] + h * tang[M][1]))
    p = p.spline([right[M - 1 - i] for i in range(M)])
    p = p.arc(left[0], via=(pts[0][0] - h * tang[0][0], pts[0][1] - h * tang[0][1]))
    return sketch().add(p)


def circle_through(a, b, c):
    """Centre and radius of the circle through three points."""
    d = 2 * (a[0] * (b[1] - c[1]) + b[0] * (c[1] - a[1]) + c[0] * (a[1] - b[1]))
    a2, b2, c2 = a[0] * a[0] + a[1] * a[1], b[0] * b[0] + b[1] * b[1], c[0] * c[0] + c[1] * c[1]
    ux = (a2 * (b[1] - c[1]) + b2 * (c[1] - a[1]) + c2 * (a[1] - b[1])) / d
    uy = (a2 * (c[0] - b[0]) + b2 * (a[0] - c[0]) + c2 * (b[0] - a[0])) / d
    return (ux, uy), math.hypot(a[0] - ux, a[1] - uy)


# --- the blade outline ---------------------------------------------------------
# The cutting edge is the arc Ri about the axis (the aperture formula needs
# nothing else); the back edge is one arc from a narrow tip to a root that
# runs into the pivot boss. Every corner of the union -- root, tip, boss,
# lever -- is filleted, so there is no neck anywhere.
TIP_R = w_tip / 2
ROOT_OUT = (Rp + 0.3 * lever * math.cos(LEVER_DEG), 0.3 * lever * math.sin(LEVER_DEG))   # where the back edge lands: on the lever
BACK_MID = polar(Ri + (w + w_tip) / 2, (PHI0 + PHI1) / 2)
BLADE_FILLET = 1.2
ROOT_FILLET = 0.3 * w


def blade_outline_sketch():
    mid = (PHI0 + PHI1) / 2
    p = path(polar(Ri, PHI0))
    p = p.arc(polar(Ri, PHI1), via=polar(Ri, mid))
    tm = polar(Ri + TIP_R, PHI1)
    p = p.arc(polar(Ri + w_tip, PHI1), via=(tm[0] - TIP_R * math.sin(PHI1), tm[1] + TIP_R * math.cos(PHI1)))
    p = p.arc(ROOT_OUT, via=BACK_MID)
    p = p.line(polar(Ri, PHI0))
    s = sketch().add(p).fillet2d(ROOT_FILLET, corners=[0])   # the root's inner corner, behind the lip
    # The boss drawn with its seam on the lever side, inside the lever.
    e = [rot((Rp + boss, 0.0), LEVER_DEG + 90.0 * i, (Rp, 0.0)) for i in range(4)]
    b = path(e[0]).arc(e[2], via=e[1]).arc(e[0], via=e[3])
    s = s.union(sketch().add(b))
    s = s.union(capsule((Rp, 0.0), D0, pin_boss))
    return fillet2d(s, BLADE_FILLET)


blade_outline_s = blade_outline_sketch()
blade_pts = blade_outline_s.points(tolerance=0.05)[0]


def blade_sketch():
    return blade_outline_s.subtract(sketch().circle(pr + pin_clear, center=(Rp, 0.0)))


def blade_outline(b, k):
    """Blade k's outline as a polygon, swung by `b`: for the 2D coverage tests."""
    return [rot(rot(q, b, (Rp, 0.0)), 360.0 * k / N) for q in blade_pts]


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
    t = k / max(1, N - 1)
    if finish == "metal":
        body.material(color=mix("#5f7590", "#9aa8b8", t), metallic=0.8, roughness=0.3)
    else:
        body.material(preset="pla_silk", color=mix("#e5482e", "#f5c443", t))


def ring_material(body):
    if finish == "metal":
        body.material(preset="brass", color="#c8a35a", roughness=0.22)
    else:
        body.material(preset="pla_matte", color="#ece4cf")


def housing_material(body):
    if finish == "metal":
        body.material(preset="aluminum", color="#9aa1a9", roughness=0.35)
    else:
        body.material(preset="pla_matte", color="#55606b")


# --- the blades --------------------------------------------------------------
# Each blade is built in its own pivot frame (post on +x) at its own level, with
# its drive pin reaching up through the levels above into the actuator's slot.
COLLAR_H = 0.6                       # the drive pin's collar, above the plate
blade_bodies = []
blade_insts = []
for k in range(N):
    z = blade_z(k)
    # Built at z = 0 and lifted. A long pin stands on a collar a little
    # wider than itself, so the plate and the collar, then the collar and the
    # pin, are two short exact cuts rather than one long one; the top blades'
    # short pins go straight in, clear of the ring above them.
    b = extrude(blade_sketch(), tb, name="blade_plate_%d" % k)
    pin_len = pin_top - z - tb / 2
    if pin_len > 8.0:
        collar = cylinder(pr + 0.5, tb / 2 + COLLAR_H, at=(D0[0], D0[1], tb / 2), name="drive_pin_collar_%d" % k)
        b = union(b, collar, name="blade_with_collar_%d" % k)
        pin = cylinder(pr, pin_top - z - tb - COLLAR_H + 0.5, at=(D0[0], D0[1], tb + COLLAR_H - 0.5), name="drive_pin_%d" % k)
    else:
        pin = cylinder(pr, pin_len, at=(D0[0], D0[1], tb / 2), name="drive_pin_%d" % k)
    b = union(b, pin, name="blade_body_%d" % k)
    b.tessellation(tolerance=0.02)   # the pins stay round in a close view
    b = b.translate((0, 0, z))
    blade_material(b, k)
    blade_bodies.append(b)
for k in range(N):
    th = 360.0 * k / N
    inst = instance(part(blade_bodies[k], name="blade_part_%d" % k), rotate=th, name="blade_%d" % k)
    blade_insts.append(inst)

# --- the base: lip plate, wall, pivot posts and the aperture scale -----------
# One revolved L-section -- plate and wall, edges chamfered -- so the exact
# kernel never meets two coaxial faces. It is turned half a blade pitch so
# its seam runs between two posts, and the posts stand in it from half a
# millimetre down.
cb = 0.8                             # outside chamfers
cl = 0.6                             # lip chamfer
section = sketch(XZ, name="housing_section").polygon([
    (Ra + cl, 0), (Rout - cb, 0), (Rout, cb), (Rout, stack_top - cb), (Rout - cb, stack_top),
    (Rw, stack_top), (Rw, plate), (Ra + cl, plate), (Ra, plate - cl), (Ra, cl),
])
base = revolve(section, axis=(0, 0, 1), name="housing_shell").rotate(180.0 / N)
posts = []
for k in range(N):
    c = polar(Rp, 360.0 * k / N)
    posts.append(cylinder(pr, stack_top - plate + 0.5, at=(c[0], c[1], plate - 0.5), name="post_%d" % k))
base = union([base] + posts, name="housing_body")

# The scale: a tick on the wall for every quarter of `opening`, where the
# ring's pointer stands at that opening. Long ticks at shut, half and open.
POINTER_AT = -120.0 - psi_max / 2    # the pointer's angle on the ring, and the open tick's: the scale faces -120
TICK_D = 0.6
SCALE = [0.0, 0.25, 0.5, 0.75, 1.0]
for o in SCALE:
    major = o in (0.0, 0.5, 1.0)
    h = 5.0 if major else 3.2
    tick = box(2 * TICK_D, 1.0, h + 1, center=True, at=(Rout, 0, stack_top - h), name="tick_%d" % int(o * 100))
    base = subtract(base, rotate(tick, POINTER_AT + psi_at(o)), name="housing_scale_%d" % int(o * 100))
housing_material(base)
housing = instance(part(base, name="housing_part"), name="housing")

# --- the actuator ring: lip, cam slots, windows, a scalloped grip and a pointer -
# In the ring's own frame, pin k rides the slot `slot_pts` turned to 360 k / N.
# The ring overhangs the housing by a finger's grip, scalloped all round but
# for the pointer, with windows between the spokes.
R_grip = Rout + 2.4
SCALLOPS = 48
act_s = sketch().circle(R_grip).circle(Ra)
slot0 = swept_slot(slot_pts, pr + pin_clear)
act_s = act_s.subtract(slot0.pattern2d_circular(N, merge=True))
scallop_pitch = 360.0 / SCALLOPS
for i in range(SCALLOPS):
    a = POINTER_AT + scallop_pitch * (i + 0.5)
    d = (a - POINTER_AT + 180.0) % 360.0 - 180.0
    if abs(d) < 1.6 * scallop_pitch:
        continue
    act_s = act_s.subtract(sketch().circle(1.5, center=polar(R_grip + 0.3, a)))
# The pointer: a short rounded tab past the grip.
tip = polar(R_grip + 2.6, POINTER_AT)
act_s = act_s.union(capsule(polar(R_grip - 1.5, POINTER_AT), tip, 1.3))
# Windows between N spokes show the blades and posts at work under the ring.
a_mid = math.atan2(slot_pts[SLOT_M // 2][1], slot_pts[SLOT_M // 2][0])
r_win_in = Ra + 2.4
r_win_out = r_lo - pr - pin_clear - 1.8
spoke = 4.0
win = sketch().circle(r_win_out).circle(r_win_in)
for k in range(N):
    a = a_mid + 360.0 / N * (k + 0.5)
    win = win.subtract(capsule(polar(r_win_in - 2, a), polar(r_win_out + 2, a), spoke / 2))
win = fillet2d(win, 1.2)
act_s = act_s.subtract(win)
act = extrude(act_s, ring_t, name="actuator_plate").translate((0, 0, act_z), name="actuator")
ring_material(act)
actuator = instance(part(act, name="actuator_part"), name="actuator_ring", layer=layer("actuator", doc="The actuator ring; hide it to watch the blades"))

# --- the pose: the iris at `opening` -------------------------------------------
for k in range(N):
    if beta:
        pose(blade_insts[k], rotate=beta, origin=(Rp * math.cos(360.0 * k / N), Rp * math.sin(360.0 * k / N), 0))
if psi:
    pose(actuator, rotate=psi)

iris = assembly([housing] + blade_insts + [actuator], name="iris")
export(iris, "iris.3mf")

report("aperture_diameter_mm", max(0.0, 2 * r_ap))
report("blade_swing_deg", beta)
report("ring_turn_deg", psi)
report("outside_diameter_mm", 2 * R_grip)
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
    """Area of the housing's hole that the N blades cover, swung by `b`."""
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


test("nothing collides anywhere in the travel", nothing_collides, sweep={"blades": [5, 8, 12], "opening": [0.0, 0.15, 0.33, 0.66, 1.0]})


def slots_guide_the_pins():
    # Each drive pin sits in its cam slot with the pin clearance all round:
    # the ring touches it at the clearance, so the ring really is what holds
    # every blade at this opening, and the slot never pinches it.
    for k in range(N):
        c = distance(blade_insts[k].body(), actuator.body())
        assert_close(c.distance, pin_clear, tol=0.06, msg="blade %d's pin clears its slot by %s, not %s" % (k, c.distance, pin_clear))


test("every drive pin rides its slot at the pin clearance", slots_guide_the_pins, sweep={"blades": [5, 8, 12], "opening": [0.0, 0.3, 0.5, 0.77, 1.0]})


def ring_drives_the_pins():
    # Where the ring meets a pin, the slot must cross the pin's own circle
    # about its post: if the two ran parallel the ring could turn without
    # moving the blade. Returns the smallest crossing angle over the travel.
    worst = 90.0
    M = 32
    for i in range(M):
        o0, o1 = 1.0 - i / M, 1.0 - (i + 1) / M
        p0, p1 = drive_pin(beta_at(o0)), drive_pin(beta_at(o1))
        s0, s1 = rot(slot_point(o0), psi_at(o0)), rot(slot_point(o1), psi_at(o0))
        va = (p1[0] - p0[0], p1[1] - p0[1])          # the pin's own path
        vs = (s1[0] - s0[0], s1[1] - s0[1])          # the slot, as it stands at o0
        cross = va[0] * vs[1] - va[1] * vs[0]
        dot = va[0] * vs[0] + va[1] * vs[1]
        ang = abs(math.atan2(cross, dot))
        worst = min(worst, min(ang, 180.0 - ang))
    assert_true(worst > 40.0, msg="the slot meets the pin's path at only %s degrees" % worst)
    return worst


test("the ring's slots cross the pins' paths steeply", ring_drives_the_pins, sweep={"blades": [5, 8, 12], "aperture": [30, 38, 46]})


def ring_turns_far():
    # Far enough to see and to set by hand: a fifth of a turn or more.
    assert_true(psi_max >= 20.0, msg="the ring turns only %s degrees" % psi_max)
    return psi_max


test("the ring turns most of a blade pitch", ring_turns_far, sweep={"blades": [5, 8, 12]})


def prints_as_one_stack():
    # The assembled stack as one print: no two bodies closer than the fuse
    # gap, every horizontal gap a whole number of layers, nothing in the air.
    bodies = [housing.body()] + [i.body() for i in blade_insts] + [actuator.body()]
    assert_printable(bodies, layer_height=layer_h, min_gap=0.25, min_wall=0.75, printer="bambu_h2d")


test("the stack is printable: whole-layer gaps, nothing fused", prints_as_one_stack, sweep={"blades": [5, 8, 12]})


def moving_parts_are_exact_solids():
    # One B-rep shell each, no mesh fallback: the pins, fillets and cam
    # slots stay true curves in export and render.
    for b in [actuator.body()] + [i.body() for i in blade_insts]:
        assert_eq(b.kind, "brep", msg="%s is a %s body" % (b.name, b.kind))


test("the blades and the ring are exact solids", moving_parts_are_exact_solids, sweep={"blades": [5, 8, 12]})


def blade_has_no_sharp_corner():
    # Root, tip, boss and lever all run into each other through fillets.
    assert_eq(len(blade_outline_s.corners()), 0, msg="corners left: %s" % blade_outline_s.corners())


test("the blade outline has no sharp corner", blade_has_no_sharp_corner, sweep={"blades": [5, 8, 12], "aperture": [30, 46]})


def scale_has_its_ticks():
    # One tick cut per mark of the scale, each where the pointer stands.
    names = [f.name for f in housing.body().faces()]
    for o in SCALE:
        assert_true(len([n for n in names if ("tick_%d/" % int(o * 100)) in n]) > 0, msg="no tick for opening %s" % o)


test("the housing carries the opening scale", scale_has_its_ticks)


def ring_turns_monotonically():
    return psi


test("the ring turns further as the iris opens less", ring_turns_monotonically, sweep={"opening": [1.0, 0.75, 0.5, 0.25, 0.0]}, monotone="increasing")

# Nearly shut to open at an even rate, one frame per step of `opening`.
animation("scroll_story", [
    keyframe(0.0, opening=0.0, ease=ease.linear, camera=view(az=-30, el=46, fov=26)),
    keyframe(2.0, opening=1.0, camera=view(az=-30, el=46, fov=26)),
], fps=24)
