# A radial aircraft engine: an odd number of finned cylinders on the flats
# of a polygonal crankcase, a single-throw crankshaft with a counterweight, a
# master rod on the top cylinder and an articulated rod on every other,
# pistons with ring grooves and wrist pins, an overhead-valve head on every
# cylinder (two poppet valves, two rocker housings, two spark plugs), pushrods
# in tubes from cam housings in front of and behind the case, an intake pipe
# and an exhaust stack per cylinder, a dual ignition harness, and a
# propeller on a long nose case.
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
cutaway = param("cutaway", False, doc="Cut away everything in front of the crank's mid-plane: the front half of the crankcase, barrels, heads and valves, and the nose case, exhausts and propeller, so the pistons, rods and valves show")
lift = param("explode", 0.0, unit="mm", min=0, max=80, doc="Exploded view: rods, pistons, barrels and heads move out along their cylinders in layers, the pushrods and intakes part from the case, the propeller and nose case forward, the crankshaft and accessory case back; a picture only")
prop = param("prop", "blades", choices=["blades", "hub", "none"], doc="The propeller: three blades, the spinner only, or nothing. The cutaway leaves it off: it lies wholly in front of the cut")

# --- Dimensions, all from bore and stroke.
B = bore
R_CRANK = stroke / 2.0
ROD = 2.0 * stroke  # master rod, crankpin to wrist pin
KNUCKLE = 0.30 * ROD  # link pins' radius on the master rod's big end
LINK = ROD - KNUCKLE * 0.92  # articulated rods, link pin to wrist pin
PIN_TO_CROWN = 0.62 * B
SKIRT = 0.42 * B  # wrist pin to skirt bottom
BIG_R = KNUCKLE + 0.16 * B  # master rod big end's radius
N = cylinders
if N % 2 == 0:
    fail("cylinders must be odd (3, 5, 7 or 9): an even radial cannot fire every other cylinder")
TH = 180.0 / N  # half the angle between two cylinders
WALL = 3.0
LINER_R = B / 2.0
FLANGE_R = LINER_R + 7.5  # the barrel's base flange, with its hold-down nuts
CAV_R = R_CRANK + BIG_R + 0.10 * B  # the crankcase's cavity: the big end swings inside
TDC = R_CRANK + ROD  # wrist pin radius at top dead centre
BARREL_TOP = TDC + PIN_TO_CROWN + 2.0
# The case is a prism with a flat for every cylinder, far enough out that
# neighbouring flanges do not meet, but never so far that the barrel gets
# shorter than its bore.
BARREL_BASE = max(CAV_R + WALL + 2.0, min((FLANGE_R + 1.5) / math.tan(TH), BARREL_TOP - 0.9 * B))
RC = BARREL_BASE  # the flats' distance from the crank axis
CASE_W = 2.0 * FLANGE_R + 3.0  # crankcase width along Y: the flange fits on the flat
LINER_BOT = min(BARREL_BASE - 8.0, CAV_R + 2.0)  # the liner's spigot runs down into the cavity
HEAD_H = 0.85 * B
FIN_R = B / 2.0 + 0.42 * B
HEAD_R = B / 2.0 + 0.50 * B
NOSE_L = 1.3 * B
REAR_L = 1.1 * B

STEEL = dict(color="#b9bcc2", metallic=1.0, roughness=0.22)
BARREL_STEEL = dict(color="#8d939b", metallic=0.55, roughness=0.5)
BLUED = dict(color="#8fa3bb", metallic=0.9, roughness=0.26)  # the master rod: heat-blued, so it reads apart from the links
ALU = dict(color="#d4d7dc", metallic=1.0, roughness=0.28)
CASE = dict(color="#9aa0a8", metallic=0.85, roughness=0.34)  # cast aluminium, bead-blasted
BRASS = dict(preset="brass")
CERAMIC = dict(color="#ece7dc", roughness=0.3)
LEAD = dict(color="#b8603f", roughness=0.55)  # silicone ignition leads
EXHAUST = dict(color="#a0805f", metallic=1.0, roughness=0.32)  # heat-tinted stainless


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


def side_dir(t):
    return v_unit(v_cross(t, (0, 1, 0)) if abs(t[1]) < 0.9 else v_cross(t, (1, 0, 0)))


def rod_between(a, b, r, name):
    """A round bar from a to b."""
    t = v_unit(v_sub(b, a))
    return extrude(sketch(plane(origin=a, normal=t, x_dir=side_dir(t))).circle(r), v_len(v_sub(b, a)), name=name)


def hex_between(a, b, r, name):
    """A hexagonal bar from a to b, r across the corners."""
    t = v_unit(v_sub(b, a))
    pts = [(r * math.cos(60.0 * i), r * math.sin(60.0 * i)) for i in range(6)]
    return extrude(sketch(plane(origin=a, normal=t, x_dir=side_dir(t))).polygon(pts), v_len(v_sub(b, a)), name=name)


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
# --- Crankcase: a hollow prism with a machined flat for every cylinder,
# its corners turned round and its edges chamfered, a bore through every
# flat for the barrel's spigot, and a ring of bolts on its front face.
PLANE_FRONT = plane(origin=(0, -CASE_W / 2.0, 0), normal=(0, 1, 0), x_dir=(1, 0, 0))  # sketch y is world -z
RC_CORNER = RC / math.cos(TH)
case_poly = []
for k in range(N):
    a = 360.0 * k / N + TH
    case_poly.append((RC_CORNER * math.sin(a), -RC_CORNER * math.cos(a)))
case_prism = extrude(sketch(PLANE_FRONT).polygon(case_poly), CASE_W, name="case_prism")
R_TURN = math.sqrt(RC * RC + (FLANGE_R + 0.5) * (FLANGE_R + 0.5))  # turned just outside the flange's corners
CH = 5.0
case_turn = rev_y([(1.0, -CASE_W / 2.0), (R_TURN - CH, -CASE_W / 2.0), (R_TURN, -CASE_W / 2.0 + CH), (R_TURN, CASE_W / 2.0 - CH), (R_TURN - CH, CASE_W / 2.0), (1.0, CASE_W / 2.0)], "case_turn")
case_cavity = rev_y([(8.0, -CASE_W / 2.0 + WALL), (CAV_R, -CASE_W / 2.0 + WALL), (CAV_R, CASE_W / 2.0 - WALL), (8.0, CASE_W / 2.0 - WALL)], "case_cavity")
case_axis = rod_between((0, -CASE_W, 0), (0, CASE_W, 0), 12.0, "case_axis_bore")
SPIGOT_R = LINER_R + 3.0
HOLE_IN = math.sqrt(CAV_R * CAV_R - (SPIGOT_R + 0.5) * (SPIGOT_R + 0.5)) - 1.0  # inside the cavity's curved wall all the way across
holes = [rod_between((cyl_dir(k)[0] * HOLE_IN, 0.0, cyl_dir(k)[1] * HOLE_IN), (cyl_dir(k)[0] * (RC + 5.0), 0.0, cyl_dir(k)[1] * (RC + 5.0)), SPIGOT_R + 0.5, "bore_%d" % k) for k in range(N)]
crankcase = subtract(intersect(case_prism, case_turn, name="case_outer", kernel="mesh"), [case_cavity, case_axis] + holes, name="crankcase", kernel="mesh")
R_BOLT = 0.82 * RC


def make_case_bolts():
    return [hex_between((R_BOLT * math.sin(360.0 * i / (2 * N) + TH / 2.0), -CASE_W / 2.0, R_BOLT * math.cos(360.0 * i / (2 * N) + TH / 2.0)), (R_BOLT * math.sin(360.0 * i / (2 * N) + TH / 2.0), -CASE_W / 2.0 - 0.05 * B, R_BOLT * math.cos(360.0 * i / (2 * N) + TH / 2.0)), 0.07 * B, "case_bolt_%d" % i) for i in range(2 * N)]


def make_nose():
    """Built only when drawn: a body made and never placed would show on its own."""
    y0 = -CASE_W / 2.0
    return rev_y([(11.0, y0), (RC * 0.72, y0), (RC * 0.72, y0 - 0.12 * B), (RC * 0.62, y0 - NOSE_L * 0.45), (RC * 0.40, y0 - NOSE_L * 0.9), (RC * 0.36, y0 - NOSE_L), (11.0, y0 - NOSE_L)], "nose_case")


rear = rev_y([(11.0, CASE_W / 2.0), (RC * 0.86, CASE_W / 2.0), (RC * 0.86, CASE_W / 2.0 + REAR_L * 0.45), (RC * 0.55, CASE_W / 2.0 + REAR_L), (11.0, CASE_W / 2.0 + REAR_L)], "accessory_case")
if cutaway:
    crankcase = front_cut(crankcase, "crankcase_cut")

# --- Barrel: a liner with a comb of fins, revolved in one go. The spigot
# below the flange drops through the case into the cavity; the fins near the
# base are trimmed where a neighbour's would meet them.
barrel_pts = [(LINER_R, LINER_BOT)]
z0 = BARREL_BASE + 4.0
z1 = BARREL_TOP - 1.0
pitch = (z1 - z0) / fins
barrel_pts.append((SPIGOT_R, LINER_BOT))
barrel_pts.append((SPIGOT_R, BARREL_BASE))
barrel_pts.append((FLANGE_R, BARREL_BASE))  # base flange
barrel_pts.append((FLANGE_R, BARREL_BASE + 3.0))
barrel_pts.append((SPIGOT_R, BARREL_BASE + 3.0))
FIN_T = min(1.6, 0.42 * pitch)
for f in range(fins):
    zf = z0 + pitch * (f + 0.5)
    rf = FIN_R - (FIN_R - LINER_R - 5.0) * 0.35 * (1.0 - float(f) / max(1, fins - 1))  # fins grow toward the hot end
    rf = max(SPIGOT_R + 1.5, min(rf, (zf - FIN_T / 2.0) * math.tan(TH) - 0.8))
    barrel_pts.append((SPIGOT_R, zf - FIN_T / 2.0))
    barrel_pts.append((rf, zf - FIN_T / 2.0 + 0.3))
    barrel_pts.append((rf, zf + FIN_T / 2.0 - 0.3))
    barrel_pts.append((SPIGOT_R, zf + FIN_T / 2.0))
barrel_pts.append((SPIGOT_R, BARREL_TOP))
barrel_pts.append((LINER_R, BARREL_TOP))
NUT_HR = min(0.055 * B, 2.0)  # across the corners; capped so the nut stays on the flange
NUT_R = LINER_R + max(5.2, 3.4 + NUT_HR)  # its inner corner keeps 0.4 mm off the spigot
nut = hex_between((NUT_R, 0.0, BARREL_BASE + 3.0), (NUT_R, 0.0, BARREL_BASE + 3.0 + 0.06 * B), NUT_HR, "flange_nut")
barrel = union(rev_z(barrel_pts, "barrel_fins"), pattern_circular(nut, 8, axis=(0, 0, 1), merge=True, name="flange_nuts"), name="barrel", kernel="mesh")

# --- Head: a finned cap with a dome inside; two poppet valves, the exhaust
# (+x) and the intake (-x), seated in pockets in the dome, each with its port
# out through a boss, front and back; a rocker housing over each valve
# reaching out to its pushrod; a spark plug boss front and back.
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

VX = 0.21 * B  # each valve's offset from the bore's axis
VR = 0.16 * B  # valve head radius
STEM_R = 0.04 * B
V_FACE = hz0 + 2.0  # the valve faces, just inside the dome
PX = 0.45 * B  # pushrods' offset across the cylinder
PY_TOP = HEAD_R + 0.22 * B  # pushrods' tops, out past the head fins
PZ_TOP = hz1 - 0.38 * B
PZ_BOT = 0.92 * RC  # pushrods' feet on the case faces
PR_R = 0.10 * B  # pushrod tube radius
PLUG_Z = hz0 + 0.55 * B
PLUG_OUT = HEAD_R - 2.5  # the plug bosses' faces, from the bore's axis


def head_fit(sx):
    """One side's valve, rocker housing and port, sx = +1 exhaust (front),
    -1 intake (back): (solid additions, cuts, valve)."""
    sy = -sx
    tag = "ex" if sx > 0 else "in"
    rocker = hull(sphere(0.15 * B, center=(sx * VX, sy * 0.05 * B, hz1 - 0.05 * B)), sphere(0.17 * B, center=(sx * PX, sy * PY_TOP, PZ_TOP + 1.0)), name="rocker_housing_" + tag)
    boss = rod_between((0.0, sy * 0.1 * B, hz0 + 0.33 * B), (0.0, sy * (HEAD_R + 2.0), hz0 + 0.29 * B), 0.25 * B, "port_boss_" + tag)
    pocket = rod_between((sx * VX, 0.0, hz0 - 1.0), (sx * VX, 0.0, V_FACE + 5.8), VR + 0.4, "valve_pocket_" + tag)
    port = rod_between((sx * VX, 0.0, V_FACE + 4.0), (0.0, sy * (HEAD_R + 3.0), hz0 + 0.29 * B), 0.15 * B, "port_" + tag)
    guide = rod_between((sx * VX, 0.0, V_FACE + 5.0), (sx * VX, 0.0, hz1 + 0.5), STEM_R + 0.35, "stem_guide_" + tag)
    valve_pts = [(0.01, 0.0), (VR, 0.0), (VR, 0.9), (VR - 1.0, 1.6), (STEM_R + 1.2, 3.4), (STEM_R, 5.5), (STEM_R, hz1 - 0.5 - V_FACE), (0.01, hz1 - 0.5 - V_FACE)]
    valve = translate(rev_z(valve_pts, "valve_x_" + tag), (sx * VX, 0.0, V_FACE), name="valve_" + tag)
    return ([rocker, boss], [pocket, port, guide], valve)


def plug_boss_axis(sy):
    """A spark plug's seat on the head, on the side away from the pushrod: (point, outward direction)."""
    x = sy * PX
    y = sy * math.sqrt(PLUG_OUT * PLUG_OUT - x * x)
    return ((x, y, PLUG_Z), v_unit((0.0, sy, 0.35)))


fit_ex = head_fit(1.0)
fit_in = head_fit(-1.0)
plug_bosses = []
for sy in (-1.0, 1.0):
    p, d = plug_boss_axis(sy)
    plug_bosses.append(rod_between(v_sub(p, v_mul(d, 9.0)), p, 0.15 * B, "plug_boss_%s" % ("front" if sy < 0 else "rear")))
head = subtract(union([head_core] + fit_ex[0] + fit_in[0] + plug_bosses, name="head_shell", kernel="mesh"), fit_ex[1] + fit_in[1], name="head", kernel="mesh")
valves = [fit_ex[2], fit_in[2]]


def spark_plug(sy, name):
    """A plug on its seat: a steel hex, a ceramic insulator and a brass terminal."""
    p, d = plug_boss_axis(sy)
    hexb = hex_between(p, v_add(p, v_mul(d, 0.16 * B)), 0.15 * B, name + "_hex")
    ins = rod_between(v_add(p, v_mul(d, 0.16 * B)), v_add(p, v_mul(d, 0.48 * B)), 0.085 * B, name + "_insulator")
    term = rod_between(v_add(p, v_mul(d, 0.48 * B)), v_add(p, v_mul(d, 0.57 * B)), 0.05 * B, name + "_terminal")
    return (hexb.material(**STEEL), ins.material(**CERAMIC), term.material(**BRASS), v_add(p, v_mul(d, 0.57 * B)), d)


# --- Ignition harness: a ring in front of the case and one behind, a lead
# from each to its plug on every head.
RING_R = RC * 0.86 + 0.12 * B
RING_Y_FRONT = -(CASE_W / 2.0 + 0.25 * B)
RING_Y_REAR = CASE_W / 2.0 + 0.30 * B
LEAD_R = 0.035 * B


def lead(sy, t, d, name):
    ring_y = RING_Y_FRONT if sy < 0 else RING_Y_REAR
    x = t[0]
    end = (x, ring_y, math.sqrt(RING_R * RING_R - x * x))
    # gentle bends: a tight one would fold the tube over itself
    c1 = v_add(t, v_mul(d, 14.0))
    c2 = (x, ring_y + sy * 0.375 * B, end[2] + 1.2 * B)
    path = curve(t, name=name + "_path").bezier(end, controls=[c1, c2])
    return sweep_along(sketch(plane(origin=t, normal=d, x_dir=side_dir(d))).circle(LEAD_R), path, name=name).material(**LEAD)


# --- Exhaust stack and intake pipe, in the cylinder's frame. The stack
# leaves the front port, turns outward and ends pointing away from the
# crank, short of the propeller's disc.
ex_a = (0.0, -HEAD_R - 1.0, hz0 + 0.29 * B)
ex_curve = curve(ex_a, name="exhaust_path").bezier((0.0, -HEAD_R - 0.42 * B, hz1 + 0.2 * B), controls=[(0.0, -HEAD_R - 0.55 * B, hz0 + 0.29 * B), (0.0, -HEAD_R - 0.5 * B, hz0 + 0.75 * B)])
in_a = (0.0, HEAD_R + 1.0, hz0 + 0.29 * B)
in_b = (0.0, CASE_W / 2.0 + REAR_L * 0.55, RC * 0.80)
in_curve = curve(in_a, name="intake_path").bezier(in_b, controls=[(0.0, HEAD_R + 0.7 * B, hz0 + 0.30 * B), (0.0, CASE_W / 2.0 + REAR_L * 0.55, RC * 0.80 + 0.9 * B)])
intake = sweep_along(sketch(plane(origin=in_a, normal=(0, 1, 0), x_dir=(1, 0, 0))).circle(0.22 * B).circle(0.16 * B), in_curve, name="intake_pipe")


def make_exhaust():
    return sweep_along(sketch(plane(origin=ex_a, normal=(0, -1, 0), x_dir=(1, 0, 0))).circle(0.26 * B).circle(0.20 * B), ex_curve, name="exhaust_stack").material(**EXHAUST)


# --- Pushrods: one per valve, each in a polished tube from its cam housing
# (the nose case for the exhaust, the accessory case for the intake) up to
# the outer end of its rocker housing, with a gland nut at each end.
def pushrod_tube(sy, name):
    x = -sy * PX
    a = (x, sy * (CASE_W / 2.0 + 0.5), PZ_BOT)
    b = (x, sy * PY_TOP, PZ_TOP)
    t = v_unit(v_sub(b, a))
    return union(
        rod_between(a, b, PR_R, name + "_tube"),
        hex_between((x, sy * CASE_W / 2.0, PZ_BOT), v_add(a, v_mul(t, 0.12 * B)), 0.15 * B, name + "_foot"),
        rod_between(v_sub(b, v_mul(t, 0.32 * B)), v_sub(b, v_mul(t, 0.1 * B)), 0.14 * B, name + "_gland"),
        name=name,
    )


# --- Piston: a flat crown, three ring grooves, skirt, and a wrist pin along
# Y, built with the wrist pin at the origin.
pr = LINER_R - 0.25
piston_pts = [
    (0.01, PIN_TO_CROWN), (pr - 1.0, PIN_TO_CROWN), (pr, PIN_TO_CROWN - 1.0),
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

# --- Propeller: a spinner and three blades on the end of the crankshaft,
# well forward of the exhaust stacks.
Y_HUB = -CASE_W / 2.0 - NOSE_L - 0.12 * B
SPIN_R = 0.64 * B

# --- Crankshaft: main journals on Y, a web with a counterweight, the crankpin
# at radius R_CRANK on +z (built at crank angle 0). The front journal runs
# out through the nose case to the spinner.
GAP = ROD_T * 0.6 + 1.5
web_sk = sketch(PZ).circle(0.36 * B, center=(0.0, R_CRANK)).union(sketch(PZ).polygon([(-0.30 * B, R_CRANK), (0.30 * B, R_CRANK), (0.62 * B, -0.2 * B), (0.0, -0.95 * R_CRANK - 0.35 * B), (-0.62 * B, -0.2 * B)]))
web_sk = fillet2d(web_sk, 0.12 * B)
web_t = 0.22 * B
webs = [translate(extrude(web_sk, web_t, name="web_x_%d" % i), (0.0, sy * (GAP + web_t) if sy > 0 else -GAP, 0.0), name="web_%d" % i) for i, sy in enumerate((-1.0, 1.0))]
crankpin_b = rod_between((0.0, -GAP - 0.5, R_CRANK), (0.0, GAP + 0.5, R_CRANK), 0.22 * B, "crankpin")
# (the cutaway stops it at the case's front face, where the nose case it ran in is gone)
journal_front = rod_between((0.0, -GAP - web_t + 0.5, 0.0), (0.0, -CASE_W / 2.0 if cutaway else Y_HUB + 0.04 * B, 0.0), 0.30 * B, "journal_front")
journal_rear = rod_between((0.0, GAP + web_t - 0.5, 0.0), (0.0, CASE_W / 2.0 + 0.6 * REAR_L, 0.0), 0.28 * B, "journal_rear")
crankshaft = union(webs[0], webs[1], crankpin_b, journal_front, journal_rear, name="crankshaft", kernel="mesh")

# The spinner's skirt is a cylinder the blade roots are buried in, so the
# blades grow out of its surface with no gap at the root.
spin_pts = [(SPIN_R * math.sqrt(max(0.0, 1.0 - (t / 10.0) * (t / 10.0))) + 0.01, Y_HUB - 0.5 * B - 1.2 * B * t / 10.0) for t in range(1, 10)]


def make_spinner():
    return revolve(sketch(XY).add(path((0.01, Y_HUB + 0.04 * B)).line((SPIN_R, Y_HUB + 0.04 * B)).line((SPIN_R, Y_HUB - 0.5 * B)).spline(spin_pts + [(0.01, Y_HUB - 1.7 * B)])), axis=(0, 1, 0), name="spinner")


PROP_R = 1.55 * (BARREL_TOP + HEAD_H)
PROP_Y = Y_HUB - 0.25 * B
# A real planform: a round, thick root in the spinner, the widest chord at
# about 40 % of the radius, a rounded narrow tip; the pitch eases from root
# to tip. Each row: (fraction of span, chord / bore, thickness / bore, pitch).
BLADE_STATIONS = [
    (0.0, 0.42, 0.34, 64.0),
    (0.12, 0.80, 0.22, 54.0),
    (0.32, 1.08, 0.15, 41.0),
    (0.52, 1.02, 0.11, 32.0),
    (0.72, 0.86, 0.08, 26.0),
    (0.88, 0.62, 0.06, 22.0),
    (0.97, 0.38, 0.05, 20.0),
    (1.0, 0.22, 0.04, 19.0),
]


def af_point(t, chord, th):
    """A point on a smooth cambered section, t in degrees round it: an
    ellipse thickened toward the leading edge (+x), its back (+y) curved and
    its face nearly flat. Closed and smooth, so the spline through it cannot
    loop at a sharp trailing edge."""
    w = 1.0 + 0.35 * math.cos(t)
    y = 0.5 * th * math.sin(t) * w
    return (0.5 * chord * math.cos(t) + 0.15 * chord, y if y > 0.0 else 0.2 * y)


def blade(k):
    """A blade lofted through cambered sections: a nearly flat face toward
    the engine, a curved back toward the front."""
    secs = []
    for i, st in enumerate(BLADE_STATIONS):
        f, cb, tb, pitch_a = st
        rr = 0.45 * B + f * (PROP_R - 0.45 * B)
        chord = cb * B
        th = tb * B
        loop = [af_point(360.0 * j / 16.0, chord, th) for j in range(16)]
        pts = []
        for p in loop:
            px = p[0] * math.cos(pitch_a) - p[1] * math.sin(pitch_a)
            py = p[0] * math.sin(pitch_a) + p[1] * math.cos(pitch_a)
            pts.append((px, PROP_Y - py, rr))
        c = curve(pts[0], name="blade_%d_s%d" % (k, i)).spline(pts[1:] + [pts[0]])
        secs.append(c.close())
    b = loft(secs, ruled=False, name="blade_%d_z" % k)
    return rotate(b, 360.0 * k / 3.0 + 30.0, axis=(0, 1, 0), name="blade_%d" % k)


# =========================================================== assembly
def inst(body, name):
    return instance(part(body, name=name + "_part"), name=name + "_i")


if cutaway:
    barrel = front_cut(barrel, "barrel_cut")
    head = front_cut(head, "head_cut")
    valves = [front_cut(v, v.name + "_cut") for v in valves]

crankcase.material(**CASE)
rear.material(**CASE)
barrel.material(**BARREL_STEEL)
head.material(**ALU)
intake.material(**ALU)
piston.material(color="#c7c9cc", metallic=1.0, roughness=0.32)
link_rod.material(**STEEL)
master_rod.material(**BLUED)
link_pins.material(**STEEL)
crankshaft.material(color="#9da1a8", metallic=1.0, roughness=0.18)
for v in valves:
    v.material(**STEEL)

# Everything wholly in front of the cut -- the nose case, the front harness
# ring and bolts, the exhaust stacks, the front plugs, leads and pushrods,
# and the propeller -- is left off in the cutaway. The harness rings stay
# with the crankcase in the explode, the leads with the pushrods.
case_bodies = [crankcase, torus(RING_R, 0.06 * B, center=(0, RING_Y_REAR, 0), axis=(0, 1, 0), name="harness_rear").material(**BRASS)]
if not cutaway:
    case_bodies += [torus(RING_R, 0.06 * B, center=(0, RING_Y_FRONT, 0), axis=(0, 1, 0), name="harness_front").material(**BRASS)] + [b.material(**STEEL) for b in make_case_bolts()]
case_i = inst(case_bodies, "crankcase")
rear_i = inst(rear, "accessory_case")
nose_i = None
if not cutaway:
    nose_i = inst(make_nose().material(**CASE), "nose_case")
static_i = [case_i, rear_i] + ([nose_i] if nose_i else [])

head_bodies = [head] + valves
pipe_bodies = [intake]
for sy in ((1.0,) if cutaway else (-1.0, 1.0)):
    side = "front" if sy < 0 else "rear"
    plug = spark_plug(sy, "plug_" + side)
    head_bodies += [plug[0], plug[1], plug[2]]
    pipe_bodies += [pushrod_tube(sy, "pushrod_" + side).material(**STEEL), lead(sy, plug[3], plug[4], "lead_" + side)]
if not cutaway:
    head_bodies.append(make_exhaust())

p_barrel = part(barrel, name="barrel_part")
p_head = part(head_bodies, name="head_part")
p_pipes = part(pipe_bodies, name="pipes_part")
p_piston = part(piston, name="piston_part")
p_link = part(link_rod, name="link_part")

# The explode moves each layer further than the one inside it: rods,
# pistons, barrels, heads; the pushrods, intakes and ignition leads part
# from the case a little, so the barrels slide out past them.
EX_ROD = 0.35
EX_PISTON = 0.9
EX_BARREL = 1.45
EX_HEAD = 2.0
EX_PIPES = 0.5

cyl_i = []
piston_i = []
rod_i = []
head_i = []
pipes_i = []
for k in range(N):
    a = 360.0 * k / N
    u = cyl_dir(k)
    bi = instance(p_barrel, rotate=a, axis=(0, 1, 0), name="barrel_%d" % k)
    hi = instance(p_head, rotate=a, axis=(0, 1, 0), name="head_%d" % k)
    pi_ = instance(p_pipes, rotate=a, axis=(0, 1, 0), name="pipes_%d" % k)
    s = K["s"][k]
    pis = instance(p_piston, at=(u[0] * s, 0.0, u[1] * s), rotate=a, axis=(0, 1, 0), name="piston_%d" % k)
    cyl_i.append(bi)
    pipes_i.append(pi_)
    head_i.append(hi)
    piston_i.append(pis)
    if lift > 0.0:
        explode(bi, (u[0] * lift * EX_BARREL, 0.0, u[1] * lift * EX_BARREL))
        explode(pi_, (u[0] * lift * EX_PIPES, 0.0, u[1] * lift * EX_PIPES))
        explode(hi, (u[0] * lift * EX_HEAD, 0.0, u[1] * lift * EX_HEAD))
        explode(pis, (u[0] * lift * EX_PISTON, 0.0, u[1] * lift * EX_PISTON))
    if k > 0:
        kp = K["pins"][k]
        pin_pt = (u[0] * s, u[1] * s)
        g = math.atan2(pin_pt[0] - kp[0], pin_pt[1] - kp[1])
        ri = instance(p_link, at=(kp[0], 0.0, kp[1]), rotate=g, axis=(0, 1, 0), name="link_rod_%d" % k)
        if lift > 0.0:
            explode(ri, (u[0] * lift * EX_ROD, 0.0, u[1] * lift * EX_ROD))
        rod_i.append(ri)

c = K["c"]
p_master = part([master_rod, link_pins], name="master_part")
master_i = instance(p_master, at=(c[0], 0.0, c[1]), rotate=K["beta"], axis=(0, 1, 0), name="master_rod_i")
crank_i = inst(crankshaft, "crankshaft")
pose(crank_i, rotate=crank, axis=(0, 1, 0), origin=(0, 0, 0))
prop_bodies = []
if prop != "none" and not cutaway:
    prop_bodies = [make_spinner().material(color="#d5d8dc", metallic=0.7, roughness=0.42)]
    if prop == "blades":
        for k in range(3):
            b = blade(k)
            b.material(color="#8a5530", roughness=0.3)  # varnished wood
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
    explode(rear_i, (0.0, 2.2 * lift, 0.0))
    if nose_i:
        explode(nose_i, (0.0, -0.7 * lift, 0.0))

engine = assembly(static_i + cyl_i + pipes_i + head_i + piston_i + rod_i + [master_i, crank_i] + prop_i, name="radial_engine")

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
            assert_true(s + PIN_TO_CROWN - 0.30 * B > LINER_BOT, msg="cylinder %d: rings leave the liner at s=%s" % (k, s))
            assert_true(s + PIN_TO_CROWN < V_FACE - 2.0, msg="cylinder %d: the crown reaches the valves at s=%s" % (k, s))
    strokes = [max([p["s"][k] for p in SWEEP]) - min([p["s"][k] for p in SWEEP]) for k in range(N)]
    for k in range(N):
        assert_close(strokes[k], stroke, rel=0.12, msg="cylinder %d's stroke" % k)
    return max(strokes) - min(strokes)


test("every piston stays in its bore through a full turn", pistons_stay_in_their_bores, sweep={"cylinders": [3, 5, 7, 9], "stroke": [20.0, 30.0, 40.0]})


def engine_inst(name):
    for i in engine.instances:
        if i.name == name:
            return i
    fail("no instance " + name)


def ib(name, body):
    """The posed body `body` of instance `name`."""
    return engine_inst(name).body(body)


def clear(a, b, what, gap=0.0):
    c = distance(a, b)
    assert_true(not c.overlap, msg="%s: %s and %s share material at %s" % (what, c.face_a, c.face_b, c.at))
    if gap > 0.0:
        assert_true(c.distance >= gap, msg="%s: only %s mm apart at %s" % (what, math.round(c.distance, 2), c.at))
    return c.distance


def pistons_clear_the_barrels_and_case():
    case = ib("crankcase_i", "crankcase")
    for k in range(N):
        clear(piston_i[k].body(), ib("barrel_%d" % k, "barrel"), "piston %d in its barrel" % k)
        clear(piston_i[k].body(), case, "piston %d in the case's bore" % k)
        clear(piston_i[k].body(), piston_i[(k + 1) % N].body(), "pistons %d and %d" % (k, (k + 1) % N))
    return N


test("pistons never touch their barrels, the case or each other", pistons_clear_the_barrels_and_case, sweep={"crank": [0.0, 60.0, 135.0, 250.0]})


def the_case_is_one_solid():
    r = print_check(crankcase, bed=(1000, 1000, 1000))
    assert_true(r.solid, msg="the crankcase is not a closed solid")
    assert_eq(r.components, 1, msg="the crankcase falls apart into pieces")
    for k in range(N):
        clear(ib("barrel_%d" % k, "barrel"), ib("crankcase_i", "crankcase"), "barrel %d on its flat" % k)
    return r.components


test("the crankcase is one solid and every barrel sits on it", the_case_is_one_solid, sweep={"cylinders": [5, 7, 9]})


def neighbours_clear():
    m = 1e9
    for k in range(N):
        n = (k + 1) % N
        m = min(m, clear(ib("barrel_%d" % k, "barrel"), ib("barrel_%d" % n, "barrel"), "barrels %d and %d" % (k, n)))
        m = min(m, clear(ib("head_%d" % k, "head"), ib("head_%d" % n, "head"), "heads %d and %d" % (k, n)))
        for pb in engine_inst("pipes_%d" % k).bodies:
            for j in (k, n, (k + N - 1) % N):
                m = min(m, clear(pb, ib("barrel_%d" % j, "barrel"), "cylinder %d's %s against barrel %d" % (k, pb.name, j), gap=0.5))
            for j in (n, (k + N - 1) % N):
                clear(pb, ib("head_%d" % j, "head"), "cylinder %d's %s against head %d" % (k, pb.name, j))
    return m


test("no cylinder's parts cross a neighbour's, and the pushrods run outside the fins", neighbours_clear)


def propeller_clears_the_exhausts():
    m = 1e9
    for bl in prop_i[0].bodies:
        for k in range(N):
            for b in [ib("head_%d" % k, "exhaust_stack"), ib("pipes_%d" % k, "pushrod_front"), ib("pipes_%d" % k, "lead_front")]:
                m = min(m, clear(bl, b, "%s against cylinder %d's %s" % (bl.name, k, b.name), gap=5.0))
    return m


test("the propeller turns clear of every exhaust stack, pushrod and lead", propeller_clears_the_exhausts, sweep={"crank": [0.0, 20.0, 40.0, 60.0, 80.0, 100.0]})


def valves_sit_free():
    for v in valves:
        clear(v, head, v.name + " in its pocket and guide", gap=0.2)
    return len(valves)


test("both valves sit in their pockets and guides without touching the head", valves_sit_free)


def cutaway_leaves_nothing_in_front():
    n = 0
    for i in engine.instances:
        for b in i.bodies:
            bb = b.bbox()
            assert_true(bb[1][1] > -0.5, msg="%s of %s lies wholly in front of the cut" % (b.name, i.name))
            n += 1
    assert_eq(prop_i, [], msg="the propeller would hide the rods")
    return n


test("the cutaway leaves nothing floating in front of the cut", cutaway_leaves_nothing_in_front, sweep={"cutaway": [True]})


def tree():
    assert_eq(len(piston_i), N)
    assert_eq(len(rod_i), N - 1)
    assert_eq(len(head_bodies), 1 + 2 + 2 * 3 + 1, msg="a head, two valves, two plugs of three pieces, an exhaust")
    assert_eq(len(pipe_bodies), 5, msg="an intake, two pushrods, two ignition leads")
    return len(engine.instances)


test("one piston per cylinder, one articulated rod per cylinder but the master's, every head fitted out", tree, sweep={"cylinders": [3, 5, 7, 9]})


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
