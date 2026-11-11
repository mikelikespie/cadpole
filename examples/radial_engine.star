# A radial aircraft engine: an odd number of finned cylinders round a
# hollow crankcase, a single-throw crankshaft with a counterweight, a master
# rod on the top cylinder and an articulated rod on every other, pistons with
# ring grooves and wrist pins, two pushrods and a rocker box per head, an
# intake pipe and an exhaust stub per cylinder, and a propeller on the nose.
#
# Kinematics -- exact, not an approximation. The crankpin sits at radius
# r = stroke / 2 at the crank angle. The master piston rides on cylinder 0
# as a plain slider-crank. The master rod is then a rigid body: each link
# pin is fixed on its big end at the knuckle radius, at the angle of its own
# cylinder measured from the master rod's axis, so its position follows from
# the crankpin and the master rod's angle. Each articulated piston sits where
# its cylinder axis meets a circle of the link rod's length about that link
# pin. So the articulated pistons have the slightly different strokes and
# the uneven motion a real master-and-articulated engine has; `report()`
# prints every cylinder's stroke and the tests check the master's against
# the slider-crank formula and every piston's travel inside its bore.
#
# Frame: the crankshaft is the Y axis, the propeller toward -Y (the front
# view looks at it). Cylinder k points along (sin a, 0, cos a) with
# a = 360 k / cylinders: cylinder 0 straight up, the rest clockwise as seen
# from the front. Every cylinder's parts are built once, pointing up +Z, and
# placed by rotating about Y -- which also means a cutaway by the half-space
# y < 0 can be cut in the part's own frame.
#
#   cadpole test radial_engine.star
#   cadpole render radial_engine.star --backend path --samples 256 --size 1600x1200 --view az=-35,el=16 --fov 24 --lighting product --backdrop studio --floor gloss --tonemap neutral --denoise -o radial_hero.png
#   cadpole render radial_engine.star ... --param cutaway=true -o radial_cutaway.png
#   cadpole render radial_engine.star ... --param explode=40 -o radial_exploded.png
#   cadpole animate radial_engine.star --name running_cutaway --view product --lighting product --backdrop studio --tonemap neutral --no-grid --no-edges -o radial.gif

cylinders = param("cylinders", 7, min=3, max=9, step=2, doc="Cylinders: an odd number, so every other one fires in two turns")
bore = param("bore", 32.0, unit="mm", min=20, max=50, doc="Cylinder bore")
stroke = param("stroke", 30.0, unit="mm", min=18, max=44, doc="Piston stroke: twice the crank throw")
fins = param("fins", 9, min=4, max=16, doc="Cooling fins on each barrel; the head carries more")
crank = param("crank", 0.0, unit="deg", min=0, max=100000, doc="Crank angle from top dead centre of cylinder 0, clockwise from the front")
cutaway = param("cutaway", False, doc="Cut away the front half of the crankcase, barrels and heads so the pistons and rods show")
lift = param("explode", 0.0, unit="mm", min=0, max=80, doc="Exploded view: heads, barrels and pistons move out along their cylinders, the propeller forward, the crankshaft back; a picture only")
prop = param("prop", "blades", choices=["blades", "hub", "none"], doc="The propeller: three blades, the hub and spinner only, or nothing")

# --- Dimensions, all from bore and stroke.
B = bore
R_CRANK = stroke / 2.0
ROD = 2.0 * stroke  # master rod, crankpin to wrist pin
KNUCKLE = 0.30 * ROD  # link pins' radius on the master rod's big end
LINK = ROD - KNUCKLE * 0.92  # articulated rods, link pin to wrist pin
PIN_TO_CROWN = 0.62 * B
SKIRT = 0.42 * B  # wrist pin to skirt bottom
BIG_R = KNUCKLE + 0.16 * B  # master rod big end's radius
RC = R_CRANK + BIG_R + 0.10 * B + 3.0  # crankcase outer radius: the big end swings inside
CASE_W = 0.95 * B  # crankcase width along Y
WALL = 3.0
TDC = R_CRANK + ROD  # wrist pin radius at top dead centre
BARREL_TOP = TDC + PIN_TO_CROWN + 2.0
BARREL_BASE = RC - 2.0
HEAD_H = 0.85 * B
LINER_R = B / 2.0
FIN_R = B / 2.0 + 0.42 * B
HEAD_R = B / 2.0 + 0.50 * B
N = cylinders
if N % 2 == 0:
    fail("cylinders must be odd (3, 5, 7 or 9): an even radial cannot fire every other cylinder")

STEEL = dict(color="#b9bcc2", metallic=1.0, roughness=0.22)
DARK_STEEL = dict(color="#8d939b", metallic=0.55, roughness=0.5)
ALU = dict(color="#d4d7dc", metallic=1.0, roughness=0.28)
CASE = dict(color="#9aa0a8", metallic=0.85, roughness=0.34)  # cast aluminium, bead-blasted
BRASS = dict(preset="brass")


def v_add(a, b):
    return (a[0] + b[0], a[1] + b[1], a[2] + b[2])


def v_sub(a, b):
    return (a[0] - b[0], a[1] - b[1], a[2] - b[2])


def v_mul(a, k):
    return (a[0] * k, a[1] * k, a[2] * k)


def v_len(a):
    return math.sqrt(a[0] * a[0] + a[1] * a[1] + a[2] * a[2])


def v_unit(a):
    l = v_len(a)
    return (a[0] / l, a[1] / l, a[2] / l)


def v_cross(a, b):
    return (a[1] * b[2] - a[2] * b[1], a[2] * b[0] - a[0] * b[2], a[0] * b[1] - a[1] * b[0])


# Sketch planes. PZ: sketch x is world x, sketch y is world z (a radial
# profile for a revolve about Z). XY: sketch x is the radius, y the axis, for
# a revolve about Y.
PZ = plane(origin=(0, 0, 0), normal=(0, -1, 0), x_dir=(1, 0, 0))


def rev_z(points, name):
    return revolve(sketch(PZ).polygon(points), axis=(0, 0, 1), name=name)


def rev_y(points, name):
    return revolve(sketch(XY).polygon(points), axis=(0, 1, 0), name=name)


def rod_between(a, b, r, name):
    """A round bar from a to b."""
    d = v_sub(b, a)
    t = v_unit(d)
    x = v_cross(t, (0, 1, 0)) if abs(t[1]) < 0.9 else v_cross(t, (1, 0, 0))
    return extrude(sketch(plane(origin=a, normal=t, x_dir=v_unit(x))).circle(r), v_len(d), name=name)


def front_cut(body, name):
    """The cutaway: remove everything at y < 0 (toward the propeller)."""
    big = 4.0 * (BARREL_TOP + HEAD_H)
    return subtract(body, box(2.0 * big, big, 2.0 * big, at=(-big, -big, -big), name=name + "_cutter"), name=name, kernel="mesh")


# =========================================================== kinematics
def cyl_dir(k):
    a = 360.0 * k / N
    return (math.sin(a), math.cos(a))  # (x, z)


def crankpin(phi):
    return (R_CRANK * math.sin(phi), R_CRANK * math.cos(phi))


def solve(phi):
    """Every wrist pin's distance out along its cylinder, the master rod's
    angle, and every link pin's position, at crank angle phi."""
    c = crankpin(phi)
    # master piston on cylinder 0 (the +z axis): slider-crank
    s0 = c[1] + math.sqrt(ROD * ROD - c[0] * c[0])
    beta = math.atan2(0.0 - c[0], s0 - c[1])  # master rod's angle from +z, toward +x
    out = {"s": [s0], "beta": beta, "pins": [None], "c": c}
    for k in range(1, N):
        a = 360.0 * k / N + beta  # the link pin's direction from the crankpin
        kp = (c[0] + KNUCKLE * math.sin(a), c[1] + KNUCKLE * math.cos(a))
        u = cyl_dir(k)
        d = kp[0] * u[0] + kp[1] * u[1]
        s = d + math.sqrt(LINK * LINK - (kp[0] * kp[0] + kp[1] * kp[1]) + d * d)
        out["s"].append(s)
        out["pins"].append(kp)
    return out


K = solve(crank)

# =========================================================== parts
# --- Crankcase: a hollow drum with a bore for every cylinder, a nose case in
# front and an accessory case behind.
case_profile = [
    (12.0, -CASE_W / 2.0), (RC - 4.0, -CASE_W / 2.0), (RC, -CASE_W / 2.0 + 4.0), (RC, CASE_W / 2.0 - 4.0),
    (RC - 4.0, CASE_W / 2.0), (12.0, CASE_W / 2.0), (12.0, CASE_W / 2.0 - WALL), (RC - WALL, CASE_W / 2.0 - WALL),
    (RC - WALL, -CASE_W / 2.0 + WALL), (12.0, -CASE_W / 2.0 + WALL),
]
case_shell = rev_y(case_profile, "case_shell")
holes = [rod_between((0, 0, 0), (cyl_dir(k)[0] * (RC + 5.0), 0.0, cyl_dir(k)[1] * (RC + 5.0)), LINER_R + 2.0, "bore_%d" % k) for k in range(N)]
crankcase = subtract(case_shell, holes, name="crankcase", kernel="mesh")
NOSE_L = 0.9 * B
def make_nose():
    """Built only when drawn: a body made and never placed would show on its own."""
    return rev_y([(11.0, -CASE_W / 2.0 + 1.0), (RC * 0.72, -CASE_W / 2.0 + 1.0), (RC * 0.62, -CASE_W / 2.0 - NOSE_L * 0.55), (RC * 0.36, -CASE_W / 2.0 - NOSE_L), (11.0, -CASE_W / 2.0 - NOSE_L)], "nose_case")
REAR_L = 1.1 * B
rear = rev_y([(11.0, CASE_W / 2.0 - 1.0), (RC * 0.86, CASE_W / 2.0 - 1.0), (RC * 0.86, CASE_W / 2.0 + REAR_L * 0.45), (RC * 0.55, CASE_W / 2.0 + REAR_L), (11.0, CASE_W / 2.0 + REAR_L)], "accessory_case")
if cutaway:
    crankcase = front_cut(crankcase, "crankcase_cut")

# --- Barrel: a liner with a comb of fins, revolved in one go.
barrel_pts = [(LINER_R, BARREL_BASE - 8.0)]
z0 = BARREL_BASE + 4.0
z1 = BARREL_TOP - 1.0
pitch = (z1 - z0) / fins
barrel_pts.append((LINER_R + 3.0, BARREL_BASE - 8.0))
barrel_pts.append((LINER_R + 3.0, BARREL_BASE))
barrel_pts.append((LINER_R + 6.0, BARREL_BASE))  # base flange
barrel_pts.append((LINER_R + 6.0, BARREL_BASE + 3.0))
barrel_pts.append((LINER_R + 3.0, BARREL_BASE + 3.0))
FIN_T = min(1.6, 0.42 * pitch)
for f in range(fins):
    zf = z0 + pitch * (f + 0.5)
    rf = FIN_R - (FIN_R - LINER_R - 5.0) * 0.35 * (1.0 - float(f) / max(1, fins - 1))  # fins grow toward the hot end
    barrel_pts.append((LINER_R + 3.0, zf - FIN_T / 2.0))
    barrel_pts.append((rf, zf - FIN_T / 2.0 + 0.3))
    barrel_pts.append((rf, zf + FIN_T / 2.0 - 0.3))
    barrel_pts.append((LINER_R + 3.0, zf + FIN_T / 2.0))
barrel_pts.append((LINER_R + 3.0, BARREL_TOP))
barrel_pts.append((LINER_R, BARREL_TOP))
barrel = rev_z(barrel_pts, "barrel")

# --- Head: a finned cap with a dome inside, two rocker boxes, a spark plug,
# an intake port behind and an exhaust port in front.
head_fins = fins + 3
hz0 = BARREL_TOP
hz1 = BARREL_TOP + HEAD_H
hp = (hz1 - hz0 - 4.0) / head_fins
head_pts = [(0.0, hz0 + 0.18 * B), (LINER_R, hz0), (HEAD_R - 6.0, hz0)]
HFIN_T = min(1.4, 0.45 * hp)
for f in range(head_fins):
    zf = hz0 + 2.0 + hp * (f + 0.5)
    q = float(f) / max(1, head_fins - 1)
    rf = HEAD_R - (HEAD_R - LINER_R - 6.0) * q * q * 0.85  # a domed silhouette
    head_pts.append((HEAD_R - 6.0 - 6.0 * q * q, zf - HFIN_T / 2.0))
    head_pts.append((rf, zf - HFIN_T / 2.0 + 0.25))
    head_pts.append((rf, zf + HFIN_T / 2.0 - 0.25))
    head_pts.append((HEAD_R - 6.0 - 6.0 * q * q, zf + HFIN_T / 2.0))
head_pts.append((LINER_R * 0.5, hz1))
head_pts.append((0.0, hz1))
head_core = revolve(sketch(PZ).polygon([(0.01, p[1]) if p[0] == 0.0 else p for p in head_pts]), axis=(0, 0, 1), name="head_core")
ROCKER_Y = 0.30 * B
rockers = [box(0.5 * B, 0.36 * B, 0.36 * B, at=(-0.25 * B, sy * ROCKER_Y - 0.18 * B, hz1 - 0.42 * B), name="rocker_box_%d" % i) for i, sy in enumerate((-1.0, 1.0))]
rocker_caps = [rod_between((0.0, sy * ROCKER_Y, hz1 - 0.08 * B), (0.0, sy * ROCKER_Y, hz1 + 0.06 * B), 0.17 * B, "rocker_cap_%d" % i) for i, sy in enumerate((-1.0, 1.0))]
port_in = rod_between((0.0, 0.1 * B, hz0 + 0.35 * B), (0.0, HEAD_R + 2.0, hz0 + 0.30 * B), 0.24 * B, "intake_port")
port_ex = rod_between((0.0, -0.1 * B, hz0 + 0.35 * B), (0.0, -HEAD_R - 2.0, hz0 + 0.28 * B), 0.26 * B, "exhaust_port")
head = union(head_core, rockers[0], rockers[1], rocker_caps[0], rocker_caps[1], port_in, port_ex, name="head", kernel="mesh")
plug = union(
    rod_between((0.22 * B, 0.0, hz1 - 0.3 * B), (0.30 * B, 0.0, hz1 + 0.25 * B), 0.11 * B, "plug_body"),
    rod_between((0.29 * B, 0.0, hz1 + 0.2 * B), (0.33 * B, 0.0, hz1 + 0.5 * B), 0.06 * B, "plug_tip"),
    name="spark_plug", kernel="mesh",
)

# --- Exhaust stub and intake pipe, in the cylinder's frame.
ex_a = (0.0, -HEAD_R - 1.0, hz0 + 0.28 * B)
# a short stub that runs forward and sweeps outward, the way a racing radial's do
ex_curve = curve(ex_a, name="exhaust_path").bezier((0.0, -HEAD_R - 0.95 * B, hz0 + 0.75 * B), controls=[(0.0, -HEAD_R - 0.45 * B, hz0 + 0.28 * B), (0.0, -HEAD_R - 0.8 * B, hz0 + 0.45 * B)])
exhaust = sweep_along(sketch(plane(origin=ex_a, normal=(0, -1, 0), x_dir=(1, 0, 0))).circle(0.26 * B).circle(0.20 * B), ex_curve, name="exhaust_stub")
in_a = (0.0, HEAD_R + 1.0, hz0 + 0.30 * B)
in_b = (0.0, CASE_W / 2.0 + REAR_L * 0.55, RC * 0.80)
in_curve = curve(in_a, name="intake_path").bezier(in_b, controls=[(0.0, HEAD_R + 0.7 * B, hz0 + 0.30 * B), (0.0, CASE_W / 2.0 + REAR_L * 0.55, RC * 0.80 + 0.9 * B)])
intake = sweep_along(sketch(plane(origin=in_a, normal=(0, 1, 0), x_dir=(1, 0, 0))).circle(0.22 * B), in_curve, name="intake_pipe")

# --- Pushrods: two per cylinder, from the cam ring behind the case to the
# rocker boxes, in polished tubes.
PR_X = 0.12 * B
pushrods = []
for i, sy in enumerate((-1.0, 1.0)):
    a = (sy * PR_X, CASE_W / 2.0 + 0.25 * B, RC - 6.0)
    b = (sy * 0.10 * B, sy * ROCKER_Y * 0.4 + 0.5 * ROCKER_Y + 0.05 * B, hz1 - 0.30 * B)
    b = (sy * 0.10 * B, ROCKER_Y + 0.12 * B if sy > 0 else 0.5 * ROCKER_Y + 0.25 * B, hz1 - 0.30 * B)
    pushrods.append(rod_between(a, b, 0.07 * B, "pushrod_%d" % i))
pushrod_pair = union(pushrods[0], pushrods[1], name="pushrods", kernel="mesh")

if cutaway:
    barrel = front_cut(barrel, "barrel_cut")
    head = front_cut(head, "head_cut")

# --- Piston: crown, three ring grooves, skirt, and a wrist pin along Y,
# built with the wrist pin at the origin.
pr = LINER_R - 0.25
piston_pts = [
    (0.01, PIN_TO_CROWN - 0.06 * B), (pr - 1.0, PIN_TO_CROWN), (pr, PIN_TO_CROWN - 1.0),
]
gz = PIN_TO_CROWN - 0.10 * B
for g in range(3):
    zg = gz - g * 0.075 * B
    piston_pts.append((pr, zg))
    piston_pts.append((pr - 1.2, zg - 0.3))
    piston_pts.append((pr - 1.2, zg - 0.03 * B))
    piston_pts.append((pr, zg - 0.03 * B - 0.3))
piston_pts.append((pr - 0.2, -SKIRT))
piston_pts.append((pr - 2.5, -SKIRT))
piston_pts.append((pr - 2.5, PIN_TO_CROWN - 0.25 * B))
piston_pts.append((0.01, PIN_TO_CROWN - 0.25 * B))
piston = rev_z(piston_pts, "piston")
wrist = rod_between((0.0, -pr + 1.5, 0.0), (0.0, pr - 1.5, 0.0), 0.12 * B, "wrist_pin")
piston = union(piston, wrist, name="piston_and_pin", kernel="mesh")

# --- Connecting rods: built from the big end at the origin up +Z.
ROD_T = 0.22 * B  # thickness along Y


def rod_shape(length, r_big, r_small, name):
    w0 = 0.14 * B
    w1 = 0.09 * B
    sk = sketch(PZ).polygon([(-w0, 0.0), (w0, 0.0), (w1, length), (-w1, length)])
    sk = sk.union(sketch(PZ).circle(r_big)).union(sketch(PZ).circle(r_small, center=(0.0, length)))
    sk = sk.subtract(sketch(PZ).circle(r_small * 0.55, center=(0.0, length)))
    return translate(extrude(sk, ROD_T, name=name + "_x"), (0.0, ROD_T / 2.0, 0.0), name=name)


link_rod = rod_shape(LINK, 0.12 * B, 0.15 * B, "link_rod")
# Master rod: its big end is a disc round the crankpin carrying the link pins.
master_sk = sketch(PZ).circle(BIG_R)
for k in range(1, N):
    a = 360.0 * k / N
    master_sk = master_sk.subtract(sketch(PZ).circle(0.07 * B, center=(KNUCKLE * math.sin(a), KNUCKLE * math.cos(a))))
master_sk = master_sk.subtract(sketch(PZ).circle(0.24 * B))
w0 = 0.17 * B
master_sk = master_sk.union(sketch(PZ).polygon([(-w0, BIG_R - 4.0), (w0, BIG_R - 4.0), (0.10 * B, ROD), (-0.10 * B, ROD)]))
master_sk = master_sk.union(sketch(PZ).circle(0.16 * B, center=(0.0, ROD))).subtract(sketch(PZ).circle(0.09 * B, center=(0.0, ROD)))
master_rod = translate(extrude(master_sk, ROD_T * 1.15, name="master_rod_x"), (0.0, ROD_T * 0.575, 0.0), name="master_rod")
link_pins = union([rod_between((KNUCKLE * math.sin(360.0 * k / N), -ROD_T * 0.7, KNUCKLE * math.cos(360.0 * k / N)), (KNUCKLE * math.sin(360.0 * k / N), ROD_T * 0.7, KNUCKLE * math.cos(360.0 * k / N)), 0.065 * B, "link_pin_%d" % k) for k in range(1, N)], name="link_pins", kernel="mesh")

# --- Crankshaft: main journals on Y, a web with a counterweight, the crankpin
# at radius R_CRANK on +z (built at crank angle 0).
GAP = ROD_T * 0.6 + 1.5
web_sk = sketch(PZ).circle(0.36 * B, center=(0.0, R_CRANK)).union(sketch(PZ).polygon([(-0.30 * B, R_CRANK), (0.30 * B, R_CRANK), (0.62 * B, -0.2 * B), (0.0, -0.95 * R_CRANK - 0.35 * B), (-0.62 * B, -0.2 * B)]))
web_sk = fillet2d(web_sk, 0.12 * B)
web_t = 0.22 * B
webs = [translate(extrude(web_sk, web_t, name="web_x_%d" % i), (0.0, sy * (GAP + web_t) if sy > 0 else -GAP, 0.0), name="web_%d" % i) for i, sy in enumerate((-1.0, 1.0))]
crankpin_b = rod_between((0.0, -GAP - 0.5, R_CRANK), (0.0, GAP + 0.5, R_CRANK), 0.22 * B, "crankpin")
journal_front = rod_between((0.0, -GAP - web_t + 0.5, 0.0), (0.0, -CASE_W / 2.0 - NOSE_L - 0.35 * B, 0.0), 0.30 * B, "journal_front")
journal_rear = rod_between((0.0, GAP + web_t - 0.5, 0.0), (0.0, CASE_W / 2.0 + 0.6 * REAR_L, 0.0), 0.28 * B, "journal_rear")
crankshaft = union(webs[0], webs[1], crankpin_b, journal_front, journal_rear, name="crankshaft", kernel="mesh")

# --- Propeller: hub, spinner and three blades, turning with the crank.
Y_HUB = -CASE_W / 2.0 - NOSE_L - 0.15 * B
def make_hub():
    return rev_y([(8.0, Y_HUB), (0.55 * B, Y_HUB), (0.55 * B, Y_HUB - 0.5 * B), (8.0, Y_HUB - 0.5 * B)], "prop_hub")
# The spinner's skirt runs back over the hub, so each blade root is buried
# in it and the blades grow out of its surface, with no gap at the root.
SPIN_R = 0.64 * B
spin_pts = [(SPIN_R * math.sqrt(max(0.0, 1.0 - (t / 10.0) * (t / 10.0))) + 0.01, Y_HUB - 0.5 * B - 1.2 * B * t / 10.0) for t in range(1, 10)]


def make_spinner():
    return revolve(sketch(XY).add(path((0.01, Y_HUB + 0.04 * B)).line((SPIN_R, Y_HUB + 0.04 * B)).line((SPIN_R, Y_HUB - 0.5 * B)).spline(spin_pts + [(0.01, Y_HUB - 1.7 * B)])), axis=(0, 1, 0), name="spinner")


PROP_R = 1.35 * (BARREL_TOP + HEAD_H)


def blade(k):
    """A blade of four sections, wide and pitched near the hub, thin at the tip."""
    secs = []
    for i, f in enumerate([0.0, 0.35, 0.75, 1.0]):
        rr = 0.45 * B + f * (PROP_R - 0.45 * B)
        chord = (0.95 * B) * (1.0 - 0.35 * f * f) if f > 0.0 else 0.5 * B
        th = (0.20 * B) * (1.0 - 0.6 * f) if f > 0.0 else 0.34 * B
        pitch_a = 52.0 - 34.0 * f
        pts = []
        for j in range(12):
            t = 360.0 * j / 12.0
            x = 0.5 * chord * math.cos(t)
            y = 0.5 * th * math.sin(t) * (1.0 if math.sin(t) > 0 else 0.55)
            # pitch: turn the section about the blade's axis
            px = x * math.cos(pitch_a) - y * math.sin(pitch_a)
            py = x * math.sin(pitch_a) + y * math.cos(pitch_a)
            pts.append((px, Y_HUB - 0.25 * B + py, rr))
        c = curve(pts[0], name="blade_%d_s%d" % (k, i)).spline(pts[1:] + [pts[0]])
        secs.append(c.close())
    b = loft(secs, ruled=False, name="blade_%d_z" % k)
    return rotate(b, 360.0 * k / 3.0 + 30.0, axis=(0, 1, 0), name="blade_%d" % k)


# =========================================================== assembly
def inst(body, name):
    return instance(part(body, name=name + "_part"), name=name + "_i")


crankcase.material(**CASE)
rear.material(**CASE)
barrel.material(**DARK_STEEL)
head.material(**ALU)
plug.material(color="#e9e4da", roughness=0.3)
exhaust.material(color="#a0805f", metallic=1.0, roughness=0.32)  # heat-tinted stainless
intake.material(**ALU)
pushrod_pair.material(**STEEL)
piston.material(color="#c7c9cc", metallic=1.0, roughness=0.32)
link_rod.material(**STEEL)
master_rod.material(**STEEL)
link_pins.material(**STEEL)
crankshaft.material(color="#9da1a8", metallic=1.0, roughness=0.18)

# the nose case lies wholly in front of the cut, so the cutaway leaves it off
static_i = [inst(crankcase, "crankcase")] + ([] if cutaway else [inst(make_nose().material(**CASE), "nose_case")]) + [inst(rear, "accessory_case")]
p_barrel = part(barrel, name="barrel_part")
p_head = part([head, plug], name="head_part")
p_pipes = part([exhaust, intake, pushrod_pair], name="pipes_part")
p_piston = part(piston, name="piston_part")
p_link = part(link_rod, name="link_part")

cyl_i = []
piston_i = []
rod_i = []
head_i = []
for k in range(N):
    a = 360.0 * k / N
    u = cyl_dir(k)
    bi = instance(p_barrel, rotate=a, axis=(0, 1, 0), name="barrel_%d" % k)
    hi = instance(p_head, rotate=a, axis=(0, 1, 0), name="head_%d" % k)
    pi_ = instance(p_pipes, rotate=a, axis=(0, 1, 0), name="pipes_%d" % k)
    s = K["s"][k]
    pis = instance(p_piston, at=(u[0] * s, 0.0, u[1] * s), rotate=a, axis=(0, 1, 0), name="piston_%d" % k)
    cyl_i += [bi, pi_]
    head_i.append(hi)
    piston_i.append(pis)
    if lift > 0.0:
        explode(bi, (u[0] * lift * 1.0, 0.0, u[1] * lift * 1.0))
        explode(pi_, (u[0] * lift * 1.6, 0.0, u[1] * lift * 1.6))
        explode(hi, (u[0] * lift * 1.6, 0.0, u[1] * lift * 1.6))
        explode(pis, (u[0] * lift * 0.55, 0.0, u[1] * lift * 0.55))
    if k > 0:
        kp = K["pins"][k]
        pin_pt = (u[0] * s, u[1] * s)
        g = math.atan2(pin_pt[0] - kp[0], pin_pt[1] - kp[1])
        ri = instance(p_link, at=(kp[0], 0.0, kp[1]), rotate=g, axis=(0, 1, 0), name="link_rod_%d" % k)
        if lift > 0.0:
            explode(ri, (u[0] * lift * 0.3, 0.0, u[1] * lift * 0.3))
        rod_i.append(ri)

c = K["c"]
p_master = part([master_rod, link_pins], name="master_part")
master_i = instance(p_master, at=(c[0], 0.0, c[1]), rotate=K["beta"], axis=(0, 1, 0), name="master_rod_i")
crank_i = inst(crankshaft, "crankshaft")
pose(crank_i, rotate=crank, axis=(0, 1, 0), origin=(0, 0, 0))
prop_bodies = []
if prop != "none":
    prop_bodies = [make_hub().material(**STEEL), make_spinner().material(color="#d5d8dc", metallic=0.7, roughness=0.42)]
    if prop == "blades":
        for k in range(3):
            b = blade(k)
            b.material(color="#7a4a26", roughness=0.28)  # varnished wood
            prop_bodies.append(b)
prop_i = []
if prop_bodies:
    pp = instance(part(prop_bodies, name="prop_part"), name="propeller_i")
    pose(pp, rotate=crank, axis=(0, 1, 0), origin=(0, 0, 0))
    prop_i = [pp]

if lift > 0.0:
    explode(crank_i, (0.0, 1.2 * lift, 0.0))
    explode(master_i, (0.0, 0.0, 0.0))
    for p in prop_i:
        explode(p, (0.0, -2.4 * lift, 0.0))
    explode(static_i[-1], (0.0, 2.2 * lift, 0.0))
    if not cutaway:
        explode(static_i[1], (0.0, -1.2 * lift, 0.0))

engine = assembly(static_i + cyl_i + head_i + piston_i + rod_i + [master_i, crank_i] + prop_i, name="radial_engine")

# =========================================================== reports and animation
SWEEP = [solve(15.0 * i) for i in range(25)]
for k in range(N):
    ss = [p["s"][k] for p in SWEEP]
    report("stroke_cyl_%d_mm" % k, math.round(max(ss) - min(ss), 2))
report("displacement_cc", math.round(N * math.pi * B * B / 4.0 * stroke / 1000.0, 1))

animation(
    "running_cutaway",
    [
        keyframe(0.0, crank=0, cutaway=True, prop="none", ease=ease.linear),
        keyframe(4.0, crank=720),
    ],
    fps=20,
    loop=True,
)
animation(
    "run_and_explode",
    [
        keyframe(0.0, crank=0, explode=0, camera=view(az=-35, el=16), ease=ease.linear),
        keyframe(2.0, crank=360, explode=0, ease=ease.smooth),
        keyframe(4.0, crank=540, explode=40, camera=view(az=-55, el=22)),
        keyframe(5.0, crank=540, explode=40),
        keyframe(7.0, crank=720, explode=0, camera=view(az=-35, el=16)),
    ],
    fps=20,
    loop=True,
)


# =========================================================== tests
def master_is_a_slider_crank():
    for i in range(0, 24):
        phi = 15.0 * i
        p = solve(phi)
        want = R_CRANK * math.cos(phi) + math.sqrt(ROD * ROD - R_CRANK * R_CRANK * math.sin(phi) * math.sin(phi))
        assert_close(p["s"][0], want, tol=1e-9)
    return R_CRANK * 2.0


test("the master piston is an exact slider-crank", master_is_a_slider_crank)


def pistons_stay_in_their_bores():
    lo_ok = BARREL_BASE - 8.0  # the liner's bottom
    for p in SWEEP:
        for k in range(N):
            s = p["s"][k]
            assert_true(s + PIN_TO_CROWN < BARREL_TOP + 0.18 * B, msg="cylinder %d: crown above the head's dome at s=%s" % (k, s))
            # the skirt's two bottom corners stay clear of the master rod's big end
            u = cyl_dir(k)
            c = p["c"]
            for side in (-1.0, 1.0):
                q = (u[0] * (s - SKIRT) + side * u[1] * pr, u[1] * (s - SKIRT) - side * u[0] * pr)
                gap = math.hypot(q[0] - c[0], q[1] - c[1]) - BIG_R
                assert_true(gap > 1.0, msg="cylinder %d: the skirt comes within %s mm of the big end" % (k, math.round(gap, 2)))
            assert_true(s + PIN_TO_CROWN - 0.30 * B > lo_ok, msg="cylinder %d: rings leave the liner at s=%s" % (k, s))
    strokes = [max([p["s"][k] for p in SWEEP]) - min([p["s"][k] for p in SWEEP]) for k in range(N)]
    for k in range(N):
        assert_close(strokes[k], stroke, rel=0.12, msg="cylinder %d's stroke" % k)
    return max(strokes) - min(strokes)


test("every piston stays in its bore through a full turn", pistons_stay_in_their_bores, sweep={"cylinders": [3, 5, 7, 9], "stroke": [20.0, 30.0, 40.0]})


def pistons_clear_the_barrels():
    for k in range(N):
        c = distance(piston_i[k].body(), engine_body("barrel_%d" % k))
        assert_true(not c.overlap, msg="piston %d rubs its barrel at %s" % (k, c.at))
    return N


def engine_body(name):
    for i in engine.instances:
        if i.name == name:
            return i.body()
    fail("no instance " + name)


test("pistons never touch their barrels", pistons_clear_the_barrels, sweep={"crank": [0.0, 60.0, 135.0, 250.0]})


def tree():
    assert_eq(len(piston_i), N)
    assert_eq(len(rod_i), N - 1)
    return len(engine.instances)


test("one piston per cylinder, one articulated rod per cylinder but the master's", tree, sweep={"cylinders": [3, 5, 7, 9]})


def fins_count():
    assert_eq(len(barrel_pts), 8 + 4 * fins, msg="four profile points to a fin, eight for the liner and flange")
    return fins


test("the barrel carries `fins` fins", fins_count, sweep={"fins": [4, 9, 16]})

animation("scroll_story", [
    keyframe(0.0, crank=0, explode=0, cutaway=False, ease=ease.linear, camera=view(az=-35, el=16, fov=24)),
    keyframe(2.5, crank=180, explode=45, ease=ease.smooth, camera=view(az=-55, el=22, fov=24)),
    keyframe(3.0, crank=200, explode=45),
    keyframe(3.05, crank=200, explode=45, cutaway=True),
    keyframe(5.0, crank=360, explode=0, cutaway=True, camera=view(az=-20, el=14, fov=24)),
], fps=20)
