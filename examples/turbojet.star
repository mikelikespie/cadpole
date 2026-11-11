# A small axial turbojet with named stages: an ogive spinner and an
# eleven-blade fan skinned through five sections (`loft(ruled=False)`),
# three compressor stages of ruled blades and vanes through closed
# `curve()` sections, a keyed shaft, two ball bearings with exact
# torus grooves and `sphere` balls, a combustor liner with primary,
# dilution and 216 effusion holes normal to its cone, a turbine disc with
# 29 skewed fir-tree slots and a row of fir-tree-rooted blades under a
# shroud ring fused from overlapping segments, five bolted casings with an
# M3 bolt and nut from `@cadpole/mechanical.star` at every hole, a
# fuel manifold swept round the combustor on exact tori, an exhaust cone
# on struts; 108 instances in one nested assembly (rotor in core in engine)
# that spools, explodes and animates, a longitudinal section sheet, a
# cutaway, a STEP of the turned parts and a 3MF of everything.
#
# Frame: the engine axis is +X, flow from the spinner at negative x to the
# nozzle at the far end. Every part of revolution is a `revolve` about
# (1, 0, 0) of a profile drawn in the XY plane with x axial and y the radius;
# `pt(x, r, a)` is a point at radius r and angle a degrees about the axis,
# measured from +Y toward +Z.
#
# How each thing is built:
#
#   * A blade is one loft through closed 3D sections of a parabolic camber
#     line with an elliptical nose and a blunt trailing edge, ruled for the
#     compressor and turbine (two sections) and smooth for the fan (five).
#     Two rules. A trailing edge thinner than about 0.5 mm tessellates into
#     flanks that pass through each other at the 0.05 mm document
#     tolerance: every trailing edge here is 55 % of the blade's thickness
#     (0.7-1.0 mm). And the nose must join the flank at a station, with
#     that station's own half-thickness, or the spline steps sideways there
#     and crosses the nose.
#   * A revolve about x has seam edges at 0, 120 and 240 degrees, and a
#     boolean whose cutting face contains one, or whose tool straddles one,
#     is fragile on the exact kernel: the shaft's keys and both keyways sit
#     at 180 degrees, the cutaway wedge's faces at 110 and 200. The bolt
#     holes through the flanges are cut on the mesh kernel, and both
#     keyways on whichever kernel takes them, so the STEP carries the
#     turned blanks.
#   * A pattern's fuse is a concatenation only when no two copies meet;
#     `distance(blade, rotate(blade, pitch))` names a contact. The turbine
#     blade's platform is a prism on the slot's own skewed plane, the
#     airfoil leans with it, and the row composes in one step. The shroud
#     is patterned on its own, 28 unions of a small sector, then joined to
#     the row in one union.
#   * Every slip fit is at least the two meshes' chord bands apart: two
#     cylinders 0.05 mm apart, both meshed at 0.05, cross in their chords,
#     and a mesh subtract's clean-up may move a vertex by the tolerance
#     besides, so the hub and discs sit 0.08 off the shaft and the turbine
#     disc 0.15. The shaft is meshed at 0.01 and the bearings at 0.005, so
#     a 0.05 gap is provable.
#   * The library phases every thread by height, so the nut is built on the
#     bolt's own axis at the height it sits at and both are placed by one
#     transform. The feed line comes down radially, clear of the flanges.
#   * The 216 effusion holes are `pattern_on_surface` on the liner's cone:
#     one cutter at the origin, rows measured on the cone, every
#     copy normal to it, one compose and one subtract. The same holes
#     placed by Starlark frames are kept behind `--param effusion=frames`
#     (one cutter per row, the six unioned into a column tool patterned
#     once) for the comparison.
#   * The igniter's boss and line are united with `kernel="mesh"` stated
#     outright, so the script evaluates the same way natively and on wasm.
#
#   cadpole test examples/turbojet.star
#   cadpole check examples/turbojet.star
#   cadpole export examples/turbojet.star --run-exports --param section_sheet=true --out-dir /tmp
#   cadpole render examples/turbojet.star --backend path --samples 256 --size 1600x1200 --view az=-30,el=18 --fov 24 --lighting product --backdrop studio --floor gloss --tonemap neutral --denoise -o docs/demo/turbojet.png
#   cadpole render examples/turbojet.star --backend path --samples 256 --size 1600x1200 --view az=30,el=18 --fov 24 --lighting product --backdrop studio --floor gloss --tonemap neutral --denoise --param explode=30 -o docs/demo/turbojet_exploded.png
#   cadpole render examples/turbojet.star --backend path --samples 256 --size 1600x1200 --view az=-30,el=18 --fov 24 --lighting product --backdrop studio --floor gloss --tonemap neutral --denoise --param cutaway=true -o docs/demo/turbojet_cutaway.png
#   cadpole animate examples/turbojet.star --view product --lighting product --backdrop studio --tonemap neutral --no-grid --no-edges -o docs/demo/turbojet.gif
load("@cadpole/mechanical.star", "bolt", "nut")

# --- Parameters.
fan_blades = param("fan_blades", 11, min=5, max=19, doc="Fan blades")
rotor_blades = param("rotor_blades", 19, min=9, max=41, doc="Blades on the first compressor rotor; each later stage has four more")
stator_vanes = param("stator_vanes", 21, min=9, max=43, doc="Vanes in the first stator; each later stage has four more")
stages = param("stages", 3, min=1, max=3, doc="Compressor stages, rotor and stator each")
turbine_blades = param("turbine_blades", 29, min=11, max=47, doc="Turbine blades, and fir-tree slots in the disc")
fan_radius = param("fan_radius", 40.0, unit="mm", min=25, max=80, doc="Fan tip radius: the engine's overall size")
tip_clearance = param("tip_clearance", 0.5, unit="mm", min=0.2, max=1.5, doc="Radial gap from every rotor blade tip to the casing over it; the tests measure it")
casing_wall = param("casing_wall", 2.5, unit="mm", min=1.5, max=5, doc="Wall of every casing")
shaft_d = param("shaft_d", 10.0, unit="mm", min=6, max=16, doc="Shaft diameter")
balls = param("balls", 9, min=5, max=16, doc="Balls in each bearing")
bolts = param("bolts", 8, min=4, max=16, doc="Bolts on each casing flange")
effusion_rows = param("effusion_rows", 6, min=0, max=16, doc="Rows of effusion cooling holes on the liner's conical exit")
effusion_holes = param("effusion_holes", 36, min=8, max=80, doc="Holes in each effusion row")
stations = param("stations", 14, min=8, max=40, doc="Samples along each blade flank: one spline point per station on each flank")
spool = param("spool", 0.0, unit="deg", min=0, max=100000, doc="Turn the spool (fan, compressor, shaft, turbine) by this about the axis; the bearings' balls follow at cage speed")
lift = param("explode", 0.0, unit="mm", min=0, max=80, doc="Exploded view: every assembly spreads its members along the axis by this per rank, the casings lift clear as well; a picture only, never geometry")
cutaway = param("cutaway", False, doc="Cut a 90-degree wedge out of every static part of revolution so the rotor shows")


def pt(x, r, a):
    return (x, r * math.cos(a), r * math.sin(a))


def vadd(a, b):
    return (a[0] + b[0], a[1] + b[1], a[2] + b[2])


def vsub(a, b):
    return (a[0] - b[0], a[1] - b[1], a[2] - b[2])


def vscale(a, k):
    return (a[0] * k, a[1] * k, a[2] * k)


def vlen(a):
    return math.sqrt(a[0] * a[0] + a[1] * a[1] + a[2] * a[2])


def vunit(a):
    l = vlen(a)
    return (a[0] / l, a[1] / l, a[2] / l)


def cross(a, b):
    return (a[1] * b[2] - a[2] * b[1], a[2] * b[0] - a[0] * b[2], a[0] * b[1] - a[1] * b[0])


def rev(points, name, angle=360.0):
    """A solid of revolution about the engine axis from an (x, r) polygon."""
    return revolve(sketch(XY).polygon(points), axis=(1, 0, 0), angle=angle, name=name)


def rev_path(p, name):
    return revolve(sketch(XY).add(p), axis=(1, 0, 0), name=name)


def ring(x0, x1, r_in, r_out, name):
    return rev([(x0, r_in), (x1, r_in), (x1, r_out), (x0, r_out)], name)


def radial_plane(x, r, a=0.0):
    """A sketch plane at (x, r, a) whose normal points radially outward and
    whose sketch x runs along the engine axis."""
    n = (0.0, math.cos(a), math.sin(a))
    return plane(origin=pt(x, r, a), normal=n, x_dir=(1, 0, 0))


def strut(x0, x1, r0, r1, width, name):
    """A stadium-section strut from radius r0 to r1 at angle 0, chord x0..x1."""
    return extrude(sketch(radial_plane((x0 + x1) / 2.0, r0)).slot(x1 - x0, width), r1 - r0, name=name)


def radial_cylinder(r_hole, x, r0, r1, name, a=0.0):
    """A cylinder of radius r_hole along the radial direction at angle a, from radius r0 to r1."""
    return extrude(sketch(radial_plane(x, r0, a)).circle(r_hole), r1 - r0, name=name)


# --- The gas path. Hub and casing radii as functions of x through the
# compressor: the annulus contracts as the air is compressed.
W = casing_wall
SHAFT_R = shaft_d / 2.0
X_SPIN = -28.0  # spinner tip
X_FAN = 0.0  # fan leading edge
FAN_CHORD = 12.0
X_HUB1 = X_FAN + FAN_CHORD + 2.0  # fan hub's rear face
HUB_R0 = 15.0  # fan hub radius at the fan's leading edge
HUB_R1 = 18.0  # ... and at its rear face
X_COMP0 = 28.0  # compressor inlet, the inlet casing's rear flange
X_COMP1 = X_COMP0 + 16.0 * stages + 2.0  # compressor exit
R_HUB0 = HUB_R1
R_HUB1 = 24.0
R_CAS0 = fan_radius - 1.5
R_CAS1 = fan_radius - 6.5
R_IN = fan_radius + tip_clearance  # fan casing bore
X_RF0 = X_COMP1  # rear frame
X_RF1 = X_RF0 + 6.0
X_COMB0 = X_RF1 + 8.0  # compressor-to-combustor flange
R_COMB = 40.5  # combustor casing bore
X_TURB0 = X_COMB0 + 50.0  # combustor-to-turbine flange
R_TS = 34.5  # turbine shroud bore
X_TB0 = X_TURB0 + 5.0  # turbine disc and blades
X_TB1 = X_TB0 + 8.0
X_NOZ0 = X_TURB0 + 20.0  # turbine-to-nozzle flange
X_END = X_NOZ0 + 34.0
R_EXIT = 26.0
R_FL = R_COMB + W + 6.0  # flange outer radius, front joints
R_FL2 = R_TS + 8.0  # flange outer radius, nozzle joint
R_BOLT = R_COMB + W + 3.0  # bolt circle of the front joints
R_BOLT2 = R_TS + 5.0  # ... and of the nozzle joint
LINER_R_OUT = 36.0
LINER_R_IN = 22.0
X_L0 = X_COMB0 + 4.0  # liner dome
X_L1 = X_TURB0 - 3.0  # liner exit
X_LC = X_L1 - 15.0  # where the liner's outer wall starts converging
LINER_R_EXIT = 34.0


def r_hub(x):
    """Rotor drum radius through the compressor."""
    return R_HUB0 + (R_HUB1 - R_HUB0) * (x - X_COMP0) / (X_COMP1 - X_COMP0)


def r_cas(x):
    """Compressor casing bore."""
    return R_CAS0 + (R_CAS1 - R_CAS0) * (x - X_COMP0) / (X_COMP1 - X_COMP0)


def r_fan_hub(x):
    return HUB_R0 + (HUB_R1 - HUB_R0) * (x - X_FAN) / (X_HUB1 - X_FAN)


def x_rotor(k):
    """Leading edge of rotor stage k's blades, and its disc's front face."""
    return X_COMP0 + 2.0 + 16.0 * k


# --- Blades. A blade is a ruled (or, for the fan, smoothly skinned) loft
# through closed 3D sections, each the outline of the blade at one span
# fraction: the same construction as examples/compressor_wheel.star, for an
# axial row. The camber line at every radius is a parabola in the
# (axial, tangential) plane whose slope is tan(beta1) at the leading edge
# and tan(beta2) at the trailing edge (angles from the axial direction,
# positive toward +theta), and the blade angles twist linearly from root to
# tip. Every section is drawn from the same station list, so station i at
# the root pairs with station i at the tip and each ruling is a radial
# fibre. The seam is the trailing-edge midpoint and the half-way point is
# the nose, which is what `cap="ruled"` wants.
KAPPA = 0.5522847498


def airfoil(x_le, chord, r_root, r_tip, beta_root, beta_tip, t_root, t_tip, name, bury_root=2.0, bury_tip=0.0, smooth=False, stack=0.5, shift=0.0, lean=0.0):
    """One blade. `r_root(x)` / `r_tip(x)` are the root and tip radii along
    the chord (the hub cone and the casing cone), `beta_*` are (beta1,
    beta2) pairs in degrees, `t_*` the maximum thickness at root and tip;
    the root section is carried `bury_root` mm inside the hub and the tip
    `bury_tip` mm past the tip line (into a shroud), for the union. The
    sections are stacked radially on the camber line's point at chord
    fraction `stack`: mid-chord, or further aft for a blade that turns the
    flow so hard its trailing edge would otherwise hang over its platform.
    `shift` moves the whole blade tangentially (mm, at the leading edge)
    and `lean` adds `lean` mm of tangential offset per mm of chord: a blade
    whose root sits in a skewed slot follows the skew."""
    n = stations

    def camber_t(m, b1, b2):
        return chord * (b1 * m - (b1 - b2) * m * m / 2.0)

    def surface(i, sig):
        i = max(0.0, min(float(n - 1), i))
        m = i / (n - 1)
        x = x_le + chord * m
        rr = r_root(x)
        rt = r_tip(x)
        r = rr + sig * (rt - rr)
        # blade angles at this span fraction: tan(beta) interpolated, and
        # extrapolated below the hub: the buried root is the same surface
        # continued (clamping it makes the smooth skin cross itself)
        s = sig
        b1 = math.tan(beta_root[0]) + s * (math.tan(beta_tip[0]) - math.tan(beta_root[0]))
        b2 = math.tan(beta_root[1]) + s * (math.tan(beta_tip[1]) - math.tan(beta_root[1]))
        t = camber_t(m, b1, b2) - camber_t(stack, b1, b2) + shift + lean * (x - x_le)
        return pt(x, r, math.degrees(t / r))

    def normal(i, sig):
        p = surface(i, sig)
        di = 0.25 if i + 0.25 <= n - 1 else -0.25
        nn = cross(vsub(surface(i + di, sig), p), vsub(surface(i, sig + 0.05), p))
        return vscale(vunit(nn), 1.0 if di > 0 else -1.0)

    def span(i):
        x = x_le + chord * max(0.0, min(1.0, i / (n - 1)))
        return r_tip(x) - r_root(x)

    def half_t(i, sig):
        m = max(0.0, min(1.0, i / (n - 1)))
        s = sig
        t = t_root + (t_tip - t_root) * s
        if m <= 0.3:
            f = 0.8 + 0.2 * m / 0.3
        else:
            q = (m - 0.3) / 0.7
            f = 1.0 - 0.45 * q * q  # a 55 % trailing edge: thinner than ~0.5 mm the two flanks' tessellations cross at the document tolerance
        return t / 2.0 * f

    def along(sig, target):
        run = 0.0
        for i in range(0, n - 1):
            step = vlen(vsub(surface(i + 1, sig), surface(i, sig)))
            if run + step >= target:
                return i + (target - run) / step
            run += step
        return float(n - 1)

    def sig_root(i):
        return -bury_root / span(i)

    def sig_tip(i):
        return 1.0 + bury_tip / span(i)

    # The leading-edge ellipse joins the flanks at station k_le, judged on
    # the root: the first station about three half-thicknesses along the
    # camber line. It joins at that station's own half-thickness, and its
    # long semi-axis is the in-plane distance from there to the true leading
    # edge, so the spline does not step sideways at the junction.
    a0 = 3.0 * half_t(0, sig_root(0))
    k_le = max(1, int(along(sig_root(0), a0) + 0.5) + 1)

    def section(sig_at, tag):
        sigk = sig_at(k_le)
        centre = surface(k_le, sigk)
        n0 = normal(k_le, sigk)
        b = half_t(k_le, sigk)
        d = vsub(surface(0, sig_at(0)), centre)
        d = vsub(d, vscale(n0, d[0] * n0[0] + d[1] * n0[1] + d[2] * n0[2]))
        a = vlen(d)
        fwd = vscale(d, 1.0 / a)
        apex = vadd(centre, d)
        press = [vadd(centre, vscale(n0, b))]
        suct = [vadd(centre, vscale(n0, -b))]
        for i in range(k_le + 1, n):
            sig = sig_at(i)
            c = surface(i, sig)
            nn = normal(i, sig)
            h = half_t(i, sig)
            press.append(vadd(c, vscale(nn, h)))
            suct.append(vadd(c, vscale(nn, -h)))
        last = len(press) - 1
        te = vscale(vadd(press[last], suct[last]), 0.5)
        p1 = vadd(press[0], vscale(fwd, KAPPA * a))
        p2 = vadd(apex, vscale(n0, KAPPA * b))
        q1 = vadd(apex, vscale(n0, -KAPPA * b))
        q2 = vadd(suct[0], vscale(fwd, KAPPA * a))
        return (curve(te, name=tag)
                .line(press[last])
                .spline([press[i] for i in range(last - 1, -1, -1)])
                .bezier(apex, controls=[p1, p2])
                .bezier(suct[0], controls=[q1, q2])
                .spline([suct[i] for i in range(1, last + 1)])
                .line(te)
                .close())

    if smooth:
        secs = [section(sig_root, name + "_s0")]
        for k, s in enumerate([0.25, 0.5, 0.75]):
            secs.append(section(lambda i, s=s: s, "%s_s%d" % (name, k + 1)))
        secs.append(section(sig_tip, name + "_s4"))
        b = loft(secs, ruled=False, cap="ruled", name=name)
    else:
        b = loft([section(sig_root, name + "_root"), section(sig_tip, name + "_tip")], ruled=True, cap="ruled", name=name)
    ONE[name] = b  # the single blade, for the tests to count its faces against the row's
    return b


ONE = {}


# =========================================================== the rotor
#
# --- Spinner and fan hub: one ogive revolve, the region kept 0.5 mm off the
# axis (a revolve whose region touches the axis makes degenerate faces),
# with a keyed bore for the shaft's front step.
KEY_W = 3.0
KEY_R = SHAFT_R + 1.5  # the keyed steps' radius
FIT = 0.08  # the slip fits on the shaft: over the chord bands of the two meshes (0.05 + 0.01), so they never cross
ogive = [(X_SPIN + (X_FAN - X_SPIN) * k / 8.0, 0.5 + (HUB_R0 - 0.5) * math.sin(90.0 * k / 8.0)) for k in range(1, 8)]
hub_profile = (path((X_SPIN, 0.5)).spline(ogive + [(X_FAN, HUB_R0)])
               .line((X_HUB1, HUB_R1)).line((X_HUB1, KEY_R + FIT)).line((X_FAN, KEY_R + FIT)).line((X_FAN, 0.5)))
hub_blank = rev_path(hub_profile, "hub")
BLANKS = [hub_blank]  # the turned parts before their keyways, rows, slots and holes: what the STEP carries exactly
# The keys sit at 180 degrees, on the far side from every revolve's seam
# edge (at 0 degrees). The cut is still the kernel's to route, exact or
# mesh, so the blank above is the turned spinner before the keyway,
# whichever way the cut goes.
hub_keyway = box(X_HUB1 - X_FAN + 1.0, 1.55, KEY_W + 0.1, at=(X_FAN - 0.5, -KEY_R - 1.05, -(KEY_W + 0.1) / 2.0), name="hub_keyway")
hub = subtract(hub_blank, hub_keyway, name="fan_hub")

fan_blade = airfoil(X_FAN + 1.0, FAN_CHORD - 2.0, r_fan_hub, lambda x: fan_radius, (55.0, 25.0), (68.0, 55.0), 2.2, 1.2, "fan_blade", bury_root=2.5, smooth=True)
fan_ring = pattern_circular(fan_blade, fan_blades, axis=(1, 0, 0), merge=True, name="fan_blades")
fan = union([hub, fan_ring], name="fan")

# --- The shaft: a stepped revolve, a key at each step. The front step
# carries the fan hub, the rear one the turbine disc.
X_SH0 = X_FAN + 0.3  # the tip sits 0.3 mm inside the hub bore: the spinner is solid ahead of X_FAN
X_SH1 = X_TB1 + 1.5
shaft_profile = [(X_SH0, 0.5), (X_SH1, 0.5), (X_SH1, SHAFT_R), (X_TB1 + 0.5, SHAFT_R), (X_TB1 + 0.5, KEY_R), (X_TB0 - 0.5, KEY_R), (X_TB0 - 0.5, SHAFT_R),
                 (X_HUB1 - 0.5, SHAFT_R), (X_HUB1 - 0.5, KEY_R), (X_FAN + 0.5, KEY_R), (X_FAN + 0.5, SHAFT_R), (X_SH0, SHAFT_R)]
key_front = box(X_HUB1 - X_FAN - 3.0, 1.5, KEY_W, at=(X_FAN + 1.5, -KEY_R - 1.0, -KEY_W / 2.0), name="key_front")
key_rear = box(X_TB1 - X_TB0 - 1.0, 1.5, KEY_W, at=(X_TB0 + 0.5, -KEY_R - 1.0, -KEY_W / 2.0), name="key_rear")
shaft = union([rev(shaft_profile, "shaft_blank"), key_front, key_rear], kernel="brep", name="shaft")
shaft.tessellation(tolerance=0.01)  # at the document's 0.05 the revolve meshed 0.17 mm off; the fits on it are measured


# --- Compressor rotors: an I-section disc with a drum reaching aft to the
# next stage, and its blade row, roots buried 2 mm into the rim.
def rotor_disc(k):
    x0 = x_rotor(k)
    sr = SHAFT_R + FIT  # a slip fit on the shaft
    rw = r_hub(x0) - 3.5  # rim underside
    x_end = x0 + 15.5
    prof = [(x0, sr), (x0 + 6.0, sr), (x0 + 6.0, sr + 4.0), (x0 + 4.5, sr + 4.0), (x0 + 4.5, rw), (x0 + 6.0, rw),
            (x0 + 6.0, r_hub(x0 + 6.0) - 2.0), (x_end, r_hub(x_end) - 2.0), (x_end, r_hub(x_end)), (x0, r_hub(x0)),
            (x0, rw), (x0 + 1.5, rw), (x0 + 1.5, sr + 4.0), (x0, sr + 4.0)]
    return rev(prof, "disc_%d" % (k + 1))


def rotor_row(k):
    x0 = x_rotor(k)
    n = rotor_blades + 4 * k
    b = airfoil(x0, 6.0, r_hub, lambda x: r_cas(x) - tip_clearance, (50.0, 20.0), (62.0, 45.0), 1.8, 1.3, "blade_%d" % (k + 1), bury_root=2.0)
    return pattern_circular(b, n, axis=(1, 0, 0), merge=True, name="blades_%d" % (k + 1))


discs = [rotor_disc(k) for k in range(stages)]
rotors = [union([discs[k], rotor_row(k)], name="rotor_%d" % (k + 1)) for k in range(stages)]
BLANKS += discs


# --- Compressor stators: an outer ring that sits in the casing bore, an
# inner shroud ring 0.5 mm over the drum, and the vane row buried 1 mm into
# each; one body per stage.
def stator(k):
    x0 = x_rotor(k) + 7.5
    x1 = x0 + 7.0
    n = stator_vanes + 4 * k
    outer = rev([(x0, r_cas(x0) - 2.0), (x1, r_cas(x1) - 2.0), (x1, r_cas(x1) - 0.05), (x0, r_cas(x0) - 0.05)], "stator_outer_%d" % (k + 1))
    inner = rev([(x0, r_hub(x0) + 0.5), (x1, r_hub(x1) + 0.5), (x1, r_hub(x1) + 2.0), (x0, r_hub(x0) + 2.0)], "stator_inner_%d" % (k + 1))
    v = airfoil(x0 + 0.5, 6.0, lambda x: r_hub(x) + 2.0, lambda x: r_cas(x) - 2.0, (-45.0, -10.0), (-55.0, -30.0), 1.8, 1.3, "vane_%d" % (k + 1), bury_root=1.0, bury_tip=1.0)
    vanes = pattern_circular(v, n, axis=(1, 0, 0), merge=True, name="vanes_%d" % (k + 1))
    # the two rings never touch, so their union is a concatenation (no
    # boolean) and the vanes then join both in ONE mesh union, not two
    rings = union([outer, inner], name="stator_rings_%d" % (k + 1))
    return union([rings, vanes], name="stator_%d" % (k + 1))


stators = [stator(k) for k in range(stages)]

# --- The turbine: a disc with skewed fir-tree slots, and a row of blades
# with fir-tree roots, platforms and overlapping shroud segments, fused
# into one ring.
T_PITCH = 360.0 / turbine_blades
T_SKEW = 15.0
T_RIM = 22.0  # disc rim radius
T_PLAT = T_RIM + 1.15  # platform top
T_SHROUD = 32.5  # shroud inner radius
T_TIP = R_TS - tip_clearance  # shroud outer radius
skew_dir = (math.cos(T_SKEW), 0.0, math.sin(T_SKEW))
skew_x = (math.sin(T_SKEW), 0.0, -math.cos(T_SKEW))  # in-plane, so that sketch y is radial
slot_plane = plane(origin=(X_TB0 - 1.0, T_RIM, 0.0), normal=skew_dir, x_dir=skew_x)
slot_len = (X_TB1 - X_TB0 + 2.0) / math.cos(T_SKEW)


def fir_tree(top, inset):
    """A fir-tree outline in (tangential, radial) coordinates, radial zero on
    the rim: the slot with inset 0, the root a hair inside it."""
    half = [(1.7, top), (1.7, 0.0), (1.1, -0.8), (1.1, -1.3), (1.8, -2.0), (1.8, -2.5), (1.0, -3.2), (1.0, -3.7), (1.6, -4.3), (1.6, -4.8), (0.8, -5.4)]
    half = [(u - inset, v + inset) for (u, v) in half]
    return half + [(-u, v) for (u, v) in reversed(half)]


slot = extrude(sketch(slot_plane).polygon(fir_tree(1.2, 0.0)), slot_len, name="slot")
slots = pattern_circular(slot, turbine_blades, axis=(1, 0, 0), merge=True, name="slots")
T_FIT = 0.15  # the turbine disc's bore: its keyway and slots are one mesh subtract whose clean-up may move a bore vertex by the 0.05 tolerance, so this fit is over two chord bands
t_sr = SHAFT_R + T_FIT
turbine_disc_profile = [(X_TB0, KEY_R + T_FIT), (X_TB1, KEY_R + T_FIT), (X_TB1, t_sr + 4.5), (X_TB1 - 1.5, t_sr + 4.5), (X_TB1 - 1.5, T_RIM - 4.0), (X_TB1, T_RIM - 4.0), (X_TB1, T_RIM),
                        (X_TB0, T_RIM), (X_TB0, T_RIM - 4.0), (X_TB0 + 1.5, T_RIM - 4.0), (X_TB0 + 1.5, t_sr + 4.5), (X_TB0, t_sr + 4.5)]
turbine_keyway = box(X_TB1 - X_TB0 + 1.0, 1.55, KEY_W + 0.1, at=(X_TB0 - 0.5, -KEY_R - 1.05, -(KEY_W + 0.1) / 2.0), name="turbine_keyway")
# The keyway and the slots are one mesh subtract from the turned blank: the
# slots put the disc on the mesh kernel anyway, and the plain revolve's bore
# meshes within its bound.
turbine_disc_blank = rev(turbine_disc_profile, "turbine_disc_blank")
BLANKS.append(turbine_disc_blank)
turbine_disc = subtract(turbine_disc_blank, [turbine_keyway, slots], name="turbine_disc")

t_root = extrude(sketch(slot_plane).polygon(fir_tree(0.3, 0.1)), slot_len, name="t_root")
# The platform is a prism on the same skewed plane as the root, so its sides
# are parallel to the root's and to the neighbours' at a constant 0.5 mm.
PLAT_HALF = (2.0 * math.pi * (T_RIM + 0.6) / turbine_blades - 0.5) / 2.0
platform = extrude(sketch(slot_plane).polygon([(-PLAT_HALF, 0.15), (PLAT_HALF, 0.15), (PLAT_HALF, T_PLAT - T_RIM), (-PLAT_HALF, T_PLAT - T_RIM)]), slot_len, name="t_platform")
shroud_angle = T_PITCH + math.degrees(0.5 / T_SHROUD)  # neighbours overlap by 0.5 mm, so the pattern fuses them into a ring
shroud_seg = rotate(rev([(X_TB0 - 0.5, T_SHROUD), (X_TB1 + 0.5, T_SHROUD), (X_TB1 + 0.5, T_TIP), (X_TB0 - 0.5, T_TIP)], "t_shroud", angle=shroud_angle),
                    -shroud_angle / 2.0, axis=(1, 0, 0), name="t_shroud_c")
# The airfoil turns the flow through 90 degrees, so it is stacked at 80 % of
# the chord: stacked at mid-chord its trailing edge would hang over the edge
# of its own 4.4 mm platform and into the neighbour's. Here the blade sits
# within its platform and the row is a concatenation.
T_LEAN = math.tan(T_SKEW)  # the airfoil follows the slot's skew: its stacking line is the prism's centreline
# 80 degrees of turning at the root and 1.8 mm thick keeps the leading
# edge's flank inside the 2.2 mm half-platform.
t_airfoil = airfoil(X_TB0 + 0.5, 7.0, lambda x: T_PLAT, lambda x: T_SHROUD, (30.0, -50.0), (25.0, -60.0), 1.8, 1.5, "t_blade", bury_root=0.8, bury_tip=0.8, stack=0.8,
                    shift=(X_TB0 + 0.5 - (X_TB0 - 1.0)) * T_LEAN, lean=T_LEAN)
t_blade = union([t_root, platform, t_airfoil], name="turbine_blade")
# The row of blades is a concatenation (no two touch: 0.5 mm between
# platforms); the shroud segments overlap, so their pattern fuses them into
# one ring, 28 small unions; then one union joins the two, so no union
# carries the whole growing row.
t_row = pattern_circular(t_blade, turbine_blades, axis=(1, 0, 0), merge=True, name="turbine_blade_row")
t_shroud = pattern_circular(shroud_seg, turbine_blades, axis=(1, 0, 0), merge=True, name="turbine_shroud")
turbine_blades_ring = union([t_row, t_shroud], name="turbine_blades")

# =========================================================== the statics
#
# --- Inlet casing with a rounded bellmouth lip and the rear flange; the
# front frame (bearing housing and five struts) sits inside it.
R_OUT = R_IN + W
lip = (path((X_FAN, R_IN)).arc((X_FAN - 3.0, R_IN + 3.0), via=(X_FAN - 2.1213, R_IN + 0.8787)).arc((X_FAN, R_IN + 6.0), via=(X_FAN - 2.1213, R_IN + 5.1213))
       .line((X_FAN + 2.0, R_IN + 6.0)).line((X_FAN + 2.0, R_OUT)).line((X_COMP0 - 3.0, R_OUT)).line((X_COMP0 - 3.0, R_FL)).line((X_COMP0, R_FL)).line((X_COMP0, R_IN)))
inlet_casing = rev_path(lip, "inlet_casing_blank")

X_FF0 = X_HUB1 + 0.5
X_FF1 = X_COMP0 + 1.5
front_hub = ring(X_FF0, X_FF1, 13.0 + FIT, HUB_R1, "front_hub")  # the outer race slips in
front_struts = pattern_circular(strut(X_FF0 + 2.5, X_FF0 + 12.5, HUB_R1 - 0.5, R_IN - 0.1, 3.0, "front_strut"), 5, axis=(1, 0, 0), merge=True, name="front_struts")
front_frame = union([front_hub, front_struts], name="front_frame")

# --- Compressor casing: the converging bore over the rotors, straight over
# the rear frame, the diffuser cone out to the combustor bore, a flange at
# each end.
comp_profile = [(X_COMP0, R_CAS0), (X_COMP1, R_CAS1), (X_RF1, R_CAS1), (X_COMB0, R_COMB), (X_COMB0, R_FL), (X_COMB0 - 3.0, R_FL), (X_COMB0 - 3.0, R_COMB + W),
                (X_RF1, R_CAS1 + W), (X_COMP1, R_CAS1 + W), (X_COMP0 + 3.0, R_CAS0 + W), (X_COMP0 + 3.0, R_FL), (X_COMP0, R_FL)]
comp_casing = rev(comp_profile, "compressor_casing_blank")

rear_hub = ring(X_RF0, X_RF1, 13.0 + FIT, R_HUB1, "rear_hub")
rear_struts = pattern_circular(strut(X_RF0 + 0.5, X_RF1 - 0.5, R_HUB1 - 0.5, R_CAS1 - 0.1, 2.5, "rear_strut"), 5, axis=(1, 0, 0), merge=True, name="rear_struts")
rear_frame = union([rear_hub, rear_struts], name="rear_frame")

# --- Combustor casing, turbine casing, nozzle.
comb_profile = [(X_COMB0, R_COMB), (X_TURB0, R_COMB), (X_TURB0, R_FL), (X_TURB0 - 3.0, R_FL), (X_TURB0 - 3.0, R_COMB + W), (X_COMB0 + 3.0, R_COMB + W), (X_COMB0 + 3.0, R_FL), (X_COMB0, R_FL)]
comb_casing = rev(comb_profile, "combustor_casing_blank")

turb_profile = [(X_TURB0, R_COMB), (X_TURB0 + 4.0, R_TS), (X_NOZ0, R_TS), (X_NOZ0, R_FL2), (X_NOZ0 - 3.0, R_FL2), (X_NOZ0 - 3.0, R_TS + W),
                (X_TURB0 + 4.0, R_TS + W), (X_TURB0 + 3.0, R_COMB + W), (X_TURB0 + 3.0, R_FL), (X_TURB0, R_FL)]
turb_casing = rev(turb_profile, "turbine_casing_blank")

noz_profile = [(X_NOZ0, R_TS), (X_END, R_EXIT), (X_END, R_EXIT + 2.0), (X_NOZ0 + 3.0, R_TS + 2.0 - 0.75), (X_NOZ0 + 3.0, R_FL2), (X_NOZ0, R_FL2)]
nozzle = rev(noz_profile, "nozzle_blank")

# --- Exhaust cone on four struts inside the nozzle.
X_EC0 = X_TB1 + 2.0  # clear of the shaft's end and the fir-tree roots' skewed overhang
X_EC1 = X_END - 4.0
cone_body = rev([(X_EC0, 0.5), (X_EC1, 0.5), (X_EC1, 1.2), (X_EC0, 20.0)], "exhaust_cone_blank")


def cone_r(x):
    return 20.0 - (20.0 - 1.2) * (x - X_EC0) / (X_EC1 - X_EC0)


def noz_r(x):
    return R_TS - (R_TS - R_EXIT) * (x - X_NOZ0) / (X_END - X_NOZ0)


x_es = X_NOZ0 + 7.0
# the bore converges, so the struts' flat ends stop short of it at their aft corner
exhaust_struts = pattern_circular(strut(x_es - 3.0, x_es + 3.0, cone_r(x_es) - 0.5, noz_r(x_es + 3.0) - 0.15, 2.0, "exhaust_strut"), 4, axis=(1, 0, 0), merge=True, name="exhaust_struts")
exhaust_cone = union([cone_body, exhaust_struts], name="exhaust_cone")


# --- Bolt holes through every flange pair: one cylinder per hole, every
# joint's fan of tools folded in one subtract, on the mesh kernel. So the
# casings are mesh bodies from here on and their blanks are what the STEP
# carries. The holes sit half a pitch round from the seam, where the
# fastener instances are.
JOINTS = [(X_COMP0, R_BOLT), (X_COMB0, R_BOLT), (X_TURB0, R_BOLT), (X_NOZ0, R_BOLT2)]


def drilled(body, joints, name):
    tools = []
    for j in joints:
        xj, rj = JOINTS[j]
        hole = cylinder(1.65, 8.0, at=(0, 0, -4.0), name="%s_hole_%d" % (name, j)).rotate(90.0, axis=(0, 1, 0)).translate((xj, rj, 0.0)).rotate(180.0 / bolts, axis=(1, 0, 0))
        tools += pattern_circular(hole, bolts, axis=(1, 0, 0), merge=False, name="%s_holes_%d" % (name, j))  # a list: the copies, unmerged
    return subtract(body, tools, kernel="mesh", name=name)


BLANKS += [inlet_casing, comp_casing, comb_casing, turb_casing, nozzle, cone_body, front_hub, rear_hub]
inlet_casing = drilled(inlet_casing, [0], "inlet_casing")
comp_casing = drilled(comp_casing, [0, 1], "compressor_casing")
comb_casing = drilled(comb_casing, [1, 2], "combustor_casing")
turb_casing = drilled(turb_casing, [2, 3], "turbine_casing")
nozzle = drilled(nozzle, [3], "nozzle")

# --- Combustor liner: an annular can with a dome, its outer wall
# converging over the last 15 mm; primary and dilution holes through both
# walls, and rows of effusion holes normal to the conical section.
liner_profile = [(X_L0, LINER_R_OUT), (X_LC, LINER_R_OUT), (X_L1, LINER_R_EXIT), (X_L1, LINER_R_EXIT - 1.0), (X_LC, LINER_R_OUT - 1.0), (X_L0 + 1.0, LINER_R_OUT - 1.0),
                 (X_L0 + 1.0, LINER_R_IN + 1.0), (X_L1, LINER_R_IN + 1.0), (X_L1, LINER_R_IN), (X_L0, LINER_R_IN)]
liner_blank = rev(liner_profile, "liner_blank")
BLANKS.append(liner_blank)
primary = pattern_circular(radial_cylinder(2.0, X_L0 + 12.0, LINER_R_IN - 1.0, LINER_R_OUT + 1.0, "primary_hole"), 8, axis=(1, 0, 0), merge=True, name="primary_holes")
dilution = pattern_circular(radial_cylinder(1.5, X_L0 + 24.0, LINER_R_IN - 1.0, LINER_R_OUT + 1.0, "dilution_hole").rotate(360.0 / 24.0, axis=(1, 0, 0)), 12, axis=(1, 0, 0), merge=True, name="dilution_holes")
cone_slope = (LINER_R_EXIT - LINER_R_OUT) / (X_L1 - X_LC)
cone_tilt = -math.degrees(math.atan(-cone_slope))  # the cone's outward normal, turned from the radial toward -x


def liner_r(x):
    return LINER_R_OUT + cone_slope * (x - X_LC)


effusion_by = param("effusion", "surface", choices=["surface", "frames"], doc="How the effusion holes are laid on the liner's cone: `surface` is pattern_on_surface, `frames` a Starlark frame per hole")
effusion = []
if effusion_rows:
    if effusion_by == "surface":
        # The cutter is drawn once at the origin straddling z = 0 and
        # the pattern stands a copy on a grid measured on the cone -- rows are
        # circles about the axis, `count=` the total shared out by row
        # length, every copy's +z along the outward normal, the odd rows
        # staggered by half a pitch -- as one compose, so the liner takes
        # every hole in one subtract.
        cutter = cylinder(0.3, 3.0, at=(0, 0, -1.5), name="effusion_cutter")
        effusion = [pattern_on_surface(cutter, liner_blank.faces(name="seg[0.1]*"), rows=effusion_rows, count=effusion_rows * effusion_holes, margin=1.5, stagger=True, merge=True, name="effusion_holes")]
    else:
        # The same holes from Starlark frames: one cutter per row, turned
        # normal to the cone, the odd rows phased by half a pitch; the rows'
        # cutters never touch, so their union is a concatenation, and that
        # column tool patterned once round the axis composes in one step too.
        column = []
        for row in range(effusion_rows):
            x = X_LC + 1.5 + (X_L1 - X_LC - 3.0) * row / max(1, effusion_rows - 1)
            column.append(cylinder(0.3, 3.0, at=(0, 0, -1.5), name="effusion_%d" % row).rotate(-90.0, axis=(1, 0, 0)).rotate(cone_tilt, axis=(0, 0, 1))
                          .translate((x, liner_r(x) - 0.5, 0.0)).rotate(360.0 / effusion_holes / 2.0 * (row % 2), axis=(1, 0, 0)))
        tool = column[0] if effusion_rows == 1 else union(column, name="effusion_column")
        effusion = [pattern_circular(tool, effusion_holes, axis=(1, 0, 0), merge=True, name="effusion_holes")]
liner = subtract(liner_blank, [primary, dilution] + effusion, name="liner")


# --- Bearings: races with a toroidal groove (an arc in the revolve
# profile), and the balls as instances of one sphere.
BALL_R = 1.5
GROOVE_R = 1.55
PITCH_R = 9.0
RACE_IN_OD = 8.3
RACE_OUT_ID = 9.7


def races(x0, x1, tag):
    xm = (x0 + x1) / 2.0
    half = math.sqrt(GROOVE_R * GROOVE_R - (RACE_IN_OD - PITCH_R) * (RACE_IN_OD - PITCH_R))
    inner = rev_path(path((x0, SHAFT_R)).line((x1, SHAFT_R)).line((x1, RACE_IN_OD)).line((xm + half, RACE_IN_OD)).arc((xm - half, RACE_IN_OD), via=(xm, PITCH_R - GROOVE_R)).line((x0, RACE_IN_OD)), tag + "_inner_race")
    half = math.sqrt(GROOVE_R * GROOVE_R - (PITCH_R - RACE_OUT_ID) * (PITCH_R - RACE_OUT_ID))
    outer = rev_path(path((x0, 13.0)).line((x1, 13.0)).line((x1, RACE_OUT_ID)).line((xm + half, RACE_OUT_ID)).arc((xm - half, RACE_OUT_ID), via=(xm, PITCH_R + GROOVE_R)).line((x0, RACE_OUT_ID)), tag + "_outer_race")
    return inner, outer


X_BF0 = X_FF0 + 2.5
X_BR0 = X_RF0 + 0.5
front_inner, front_outer = races(X_BF0, X_BF0 + 5.0, "front")
rear_inner, rear_outer = races(X_BR0, X_BR0 + 5.0, "rear")
front_ball = sphere(BALL_R, center=(X_BF0 + 2.5, PITCH_R, 0.0), name="front_ball")
rear_ball = sphere(BALL_R, center=(X_BR0 + 2.5, PITCH_R, 0.0), name="rear_ball")
# the 0.05 mm ball-to-groove gap is measured, so these are meshed ten times
# finer than the document: the test's bound is then ±0.01
for b in (front_inner, front_outer, rear_inner, rear_outer, front_ball, rear_ball):
    b.tessellation(tolerance=0.005)

# --- Fasteners from the library: one M3 bolt and one nut, built once,
# turned to lie along the axis on the front joints' bolt circle, and
# instanced round every flange.
# The bolt stands on its tip along +z; the nut is built on the same axis at
# the height it sits at (3.6 mm up the 12 mm shank, so the two flanges are
# between it and the head), because the library phases every thread by
# height: built in one frame they mesh, and one transform places both.
BOLT_TO_X = lambda z: X_COMP0 + 9.0 - z  # the bolt's z on the engine's x
m3 = bolt(3.0, 0.5, 12.0, 5.5, 2.0, name="m3_bolt")
m3n = nut(3.0, 0.5, 5.5, 2.4, clearance=0.05, start=(0, 0, 3.6), name="m3_nut")
m3 = translate(rotate(m3, -90.0, axis=(0, 1, 0)), (BOLT_TO_X(0.0), R_BOLT, 0.0), name="m3_bolt_placed")
m3n = translate(rotate(m3n, -90.0, axis=(0, 1, 0)), (BOLT_TO_X(0.0), R_BOLT, 0.0), name="m3_nut_placed")

# --- Fuel manifold: a 3 mm tube swept round the combustor casing on three
# arcs (a swept path must stay open, so the ring is 300 degrees), a feed
# that comes down radially at the top and arcs into the ring's start
# tangentially, six injector stubs on the ring, and an igniter with its
# boss.
X_M = X_COMB0 + 20.0
R_M = R_COMB + W + 4.0
ring_route = curve(pt(X_M, R_M, 0.0)).arc(pt(X_M, R_M, 100.0), via=pt(X_M, R_M, 50.0)).arc(pt(X_M, R_M, 200.0), via=pt(X_M, R_M, 150.0)).arc(pt(X_M, R_M, 300.0), via=pt(X_M, R_M, 250.0))
manifold_ring = sweep_along(sketch(plane(origin=pt(X_M, R_M, 0.0), normal=(0, 0, 1), x_dir=(1, 0, 0))).circle(1.5).circle(1.0), ring_route, name="manifold_ring")
feed_route = (curve((X_M, R_M + 20.0, -6.0)).line((X_M, R_M + 6.0, -6.0))
              .arc((X_M, R_M, 0.0), via=(X_M, R_M + 6.0 - 6.0 * math.cos(45.0), -6.0 * math.sin(45.0)))
              .line((X_M, R_M, 2.0)))  # 2 mm into the ring's tube, for the union
feed_line = sweep_along(sketch(plane(origin=(X_M, R_M + 20.0, -6.0), normal=(0, -1, 0), x_dir=(1, 0, 0))).circle(1.5).circle(1.0), feed_route, name="feed_line")
manifold = union([manifold_ring, feed_line], name="manifold")
injectors = pattern_circular(radial_cylinder(1.25, X_M, R_COMB + W - 0.5, R_M + 0.3, "injector", a=25.0), 6, axis=(1, 0, 0), angle=250.0, merge=True, name="injectors")  # 25..275, all on the 300-degree ring
X_IG = X_M + 12.0
A_IG = 320.0
ig_boss = radial_cylinder(2.5, X_IG, R_COMB + W - 0.5, R_COMB + W + 4.0, "igniter_boss", a=A_IG)
ig_route = curve(pt(X_IG, R_COMB + W + 3.0, A_IG)).line(pt(X_IG, R_COMB + W + 8.0, A_IG))
ig_top = pt(X_IG, R_COMB + W + 8.0, A_IG)
ig_n = vunit(vsub(ig_top, pt(X_IG, R_COMB + W + 3.0, A_IG)))
ig_route = ig_route.arc(vadd(ig_top, vadd(vscale(ig_n, 4.0), (-4.0, 0.0, 0.0))), via=vadd(ig_top, vadd(vscale(ig_n, 4.0 * math.sin(45.0)), (-4.0 + 4.0 * math.cos(45.0), 0.0, 0.0)))).line(vadd(ig_top, vadd(vscale(ig_n, 4.0), (-14.0, 0.0, 0.0))))
igniter_line = sweep_along(sketch(plane(origin=pt(X_IG, R_COMB + W + 3.0, A_IG), normal=ig_n, x_dir=(1, 0, 0))).circle(1.0).circle(0.6), ig_route, name="igniter_line")
# `kernel="mesh"` is stated, not left to "auto": the native engine can fall
# back from the exact kernel to the mesh kernel on this pair, wasm32 cannot,
# so saying it keeps the two builds identical.
igniter = union([ig_boss, igniter_line], kernel="mesh", name="igniter")
# the injectors and the igniter boss pass through ports in the casing wall,
# 0.1 mm larger than they are
ports = pattern_circular(radial_cylinder(1.35, X_M, R_COMB - 1.0, R_COMB + W + 1.0, "injector_port", a=25.0), 6, axis=(1, 0, 0), angle=250.0, merge=True, name="injector_ports")
comb_casing = subtract(comb_casing, [ports, radial_cylinder(2.6, X_IG, R_COMB - 1.0, R_COMB + W + 1.0, "igniter_port", a=A_IG)], kernel="mesh", name="combustor_casing_ported")

# =========================================================== cutaway, materials
#
# A 270-degree wedge, turned so that neither of its faces lies in a
# revolve's seam plane, removes the quadrant that faces the product camera
# from every static part of revolution. The rotor is never cut.
if cutaway:
    wedge = rotate(rev([(X_SPIN - 5.0, 0.3), (X_END + 5.0, 0.3), (X_END + 5.0, 60.0), (X_SPIN - 5.0, 60.0)], "wedge", angle=270.0), 200.0, axis=(1, 0, 0), name="wedge_turned")
    inlet_casing = intersect(inlet_casing, wedge, name="inlet_casing_cut")
    comp_casing = intersect(comp_casing, wedge, name="compressor_casing_cut")
    comb_casing = intersect(comb_casing, wedge, name="combustor_casing_cut")
    turb_casing = intersect(turb_casing, wedge, name="turbine_casing_cut")
    nozzle = intersect(nozzle, wedge, name="nozzle_cut")
    liner = intersect(liner, wedge, name="liner_cut")
    stators = [intersect(stators[k], wedge, name="stator_%d_cut" % (k + 1)) for k in range(stages)]

ALU = dict(preset="aluminum", color="#d7d9dc", roughness=0.3)
STEEL = dict(color="#c4c9cf", metallic=1.0, roughness=0.32)  # bright steel
CREAM = dict(color="#ece3cb", metallic=0.0, roughness=0.45)  # enamelled casing
COPPER = dict(color="#d08a5c", metallic=1.0, roughness=0.38)
GOLD = dict(color="#d9b76e", metallic=1.0, roughness=0.36)  # nickel alloy, warm
ORANGE = dict(color="#e08a3c", metallic=1.0, roughness=0.42)
WARM = dict(color="#c4b5a2", metallic=1.0, roughness=0.42)  # the exhaust, straw-tinted titanium
fan.material(color="#eceef0", roughness=0.18, metallic=1.0, anisotropy=0.5, texture="machined")
shaft.material(**STEEL)
for r in rotors:
    r.material(color="#3fc6a8", roughness=0.3, metallic=1.0)
for s in stators:
    s.material(**STEEL)
turbine_disc.material(**GOLD)
turbine_blades_ring.material(color="#e3c88f", metallic=1.0, roughness=0.36)
inlet_casing.material(**ALU)
front_frame.material(**ALU)
comp_casing.material(**ALU)
rear_frame.material(**STEEL)
comb_casing.material(**CREAM)
liner.material(**COPPER)
turb_casing.material(**ORANGE)
nozzle.material(**WARM)
exhaust_cone.material(**WARM)
for b in (front_inner, front_outer, rear_inner, rear_outer, front_ball, rear_ball, m3, m3n):
    b.material(**STEEL)
manifold.material(preset="brass")
injectors.material(preset="brass")
igniter.material(**STEEL)

# =========================================================== the assembly
#
# engine
#   core
#     rotor: fan, discs, shaft, turbine disc and blades, the inner races
#     stators, the liner, the frames, the outer races, the balls
#   the casings and the nozzle
#   fasteners, fuel system
CAGE = 0.5 * (1.0 - BALL_R / PITCH_R)  # the cage turns at this fraction of the shaft speed


def inst(body, name):
    """A part built once and its one instance; the instance is `<name>_i`
    (a body and an instance cannot share a feature name)."""
    return instance(part(body, name=name + "_part"), name=name + "_i")


def turning(body, name, rate=1.0):
    i = inst(body, name)
    if spool:
        pose(i, rotate=spool * rate, axis=(1, 0, 0), origin=(0, 0, 0))
    return i


fan_i = turning(fan, "fan")
shaft_i = turning(shaft, "shaft")
rotor_i = [turning(rotors[k], "rotor_%d" % (k + 1)) for k in range(stages)]
tdisc_i = turning(turbine_disc, "turbine_disc")
tblades_i = turning(turbine_blades_ring, "turbine_blades")
front_inner_i = turning(front_inner, "front_inner_race")
rear_inner_i = turning(rear_inner, "rear_inner_race")
rotor_asm = assembly([fan_i] + rotor_i + [shaft_i, tdisc_i, tblades_i, front_inner_i, rear_inner_i], name="rotor")

stator_i = [inst(stators[k], "stator_%d" % (k + 1)) for k in range(stages)]
liner_i = inst(liner, "liner")
front_frame_i = inst(front_frame, "front_frame")
rear_frame_i = inst(rear_frame, "rear_frame")
front_outer_i = inst(front_outer, "front_outer_race")
rear_outer_i = inst(rear_outer, "rear_outer_race")
front_ball_p = part(front_ball, name="front_ball_part")
rear_ball_p = part(rear_ball, name="rear_ball_part")
ball_i = []
for k in range(balls):
    a = 360.0 * k / balls
    for tag, p in (("front", front_ball_p), ("rear", rear_ball_p)):
        i = instance(p, rotate=a, axis=(1, 0, 0), name="%s_ball_%d" % (tag, k))
        if spool:
            pose(i, rotate=spool * CAGE, axis=(1, 0, 0), origin=(0, 0, 0))
        ball_i.append(i)
core = assembly([rotor_asm] + stator_i + [liner_i, front_frame_i, rear_frame_i, front_outer_i, rear_outer_i] + ball_i, name="core")

casing_i = [inst(inlet_casing, "inlet_casing"), inst(comp_casing, "compressor_casing"), inst(comb_casing, "combustor_casing"), inst(turb_casing, "turbine_casing"), inst(nozzle, "nozzle"), inst(exhaust_cone, "exhaust_cone")]

bolt_p = part(m3, name="m3_bolt_part")
nut_p = part(m3n, name="m3_nut_part")
fastener_i = []
for j, (xj, rj) in enumerate(JOINTS):
    for k in range(bolts):
        a = 360.0 * (k + 0.5) / bolts
        dr = rj - R_BOLT
        shift = (xj - X_COMP0, dr * math.cos(a), dr * math.sin(a))
        fastener_i.append(instance(bolt_p, at=shift, rotate=a, axis=(1, 0, 0), name="bolt_%d_%d" % (j + 1, k)))
        fastener_i.append(instance(nut_p, at=shift, rotate=a, axis=(1, 0, 0), name="nut_%d_%d" % (j + 1, k)))
fasteners = assembly(fastener_i, name="fasteners")
fuel = assembly([inst(manifold, "fuel_manifold"), inst(injectors, "injectors"), inst(igniter, "igniter")], name="fuel_system")
engine = assembly([core] + casing_i + [fasteners, fuel], name="engine")

explode(engine, lift, along="x")
if lift > 0.0:
    for i in casing_i:
        explode(i, (0.0, 0.0, 1.2 * lift))
    for i in fastener_i:
        explode(i, (0.0, 0.0, 1.2 * lift))

# =========================================================== exports, drawing, animation
#
# STEP: what the exact kernel holds -- the shaft, the four races, the balls
# -- and the turned blanks (hub, discs, casings, liner, cone, frame hubs)
# before their rows, slots and holes; every bladed row and every drilled
# flange is a mesh body and goes to the 3MF, one object per part.
exact = [i.body() for i in engine.instances if i.body().kind == "brep"]
export(exact, "turbojet.step")
blanks = [b for b in BLANKS if b.kind == "brep"]  # a blank whose cut the kernel routed to triangles cannot go in a STEP
export(blanks, "turbojet_blanks.step")
export(engine, "turbojet.3mf")

section_sheet = param("section_sheet", False, doc="Draw the longitudinal section sheet (cadpole export --run-exports --param section_sheet=true): off by default, the projection of a hundred bodies is not free")
if section_sheet:
    sheet = drawing(view="section:y=0", sheet="A2", title="Turbojet, longitudinal section", name="section")
    export_svg(sheet, "turbojet_section.svg")

# The animation: the spool winds up over four seconds, the engine
# comes apart along its axis while it turns and the camera swings round,
# then a last second at speed. Every frame after the first is poses only.
animation(
    "spool_up_and_explode",
    [
        keyframe(0.0, spool=0, explode=0, camera=view(az=30, el=18), ease=ease.ease_in),
        keyframe(4.0, spool=1440),
        keyframe(7.0, spool=2520, explode=40, camera=view(az=50)),
        keyframe(8.0, spool=2880),
    ],
    fps=20,
    loop=False,
)


# =========================================================== tests (`cadpole test examples/turbojet.star`)
def clear(c, what):
    assert_true(not c.overlap, msg="%s: %s and %s share material at %s" % (what, c.face_a, c.face_b, c.at))
    return c


def every_instance_is_a_solid():
    for i in engine.instances:
        assert_solid(i.body(), msg=i.name)
    return len(engine.instances)


test("every instance is a solid", every_instance_is_a_solid)


def the_tree():
    assert_eq(len(rotor_asm.instances), 6 + stages, msg="the rotor: fan, discs, shaft, turbine disc and blades, two inner races")
    assert_eq(len(core.instances), len(rotor_asm.instances) + stages + 5 + 2 * balls, msg="the core: the rotor, stators, liner, frames, outer races, balls")
    assert_eq(len(fasteners.instances), 2 * bolts * len(JOINTS))
    assert_eq(len(engine.instances), len(core.instances) + len(casing_i) + len(fasteners.instances) + 3)
    return len(engine.instances)


test("the tree: the rotor in the core in the engine, six casings, the fasteners, the fuel system", the_tree)


def blade_counts():
    """Every row has its count of blades: a blade's cap faces (or a vane's
    flank faces, both its caps being buried) survive the mesh union under
    their own names, so the row's count is a multiple of one blade's."""
    per = ONE["fan_blade"].faces(name="fan_blade/end*").count()
    assert_eq(fan_i.body().faces(name="fan_blade/end*").count(), fan_blades * per, msg="fan tips")
    for k in range(stages):
        tag = "blade_%d" % (k + 1)
        per = ONE[tag].faces(name=tag + "/end*").count()
        assert_eq(rotor_i[k].body().faces(name=tag + "/end*").count(), (rotor_blades + 4 * k) * per, msg="stage %d rotor tips" % (k + 1))
        tag = "vane_%d" % (k + 1)
        per = ONE[tag].faces(name=tag + "/side*").count()
        assert_eq(stator_i[k].body().faces(name=tag + "/side*").count(), (stator_vanes + 4 * k) * per, msg="stage %d vanes" % (k + 1))
    assert_eq(tblades_i.body().faces(name="t_root/start*").count(), turbine_blades, msg="turbine blade roots")
    n_slot = tdisc_i.body().faces(name="slot/side*").count()
    assert_eq(n_slot % turbine_blades, 0, msg="the fir-tree slots' %d faces are not %d equal sets" % (n_slot, turbine_blades))
    assert_true(n_slot // turbine_blades >= 16, msg="a fir-tree slot keeps at least 16 of its 22 flanks inside the rim: %d" % (n_slot // turbine_blades))
    return n_slot // turbine_blades


test("blade counts: the fan, every rotor and stator row, the turbine blades and their slots", blade_counts)


def tip_clearances():
    """Every rotor's tips clear the casing over them by `tip_clearance`,
    measured with `distance()`: the fan in the inlet casing, each
    compressor stage in the compressor casing, the turbine shroud in the
    turbine casing. A mesh body measures at the document's 0.05, so the
    bound is 0.11 on a 0.5 mm gap."""
    pairs = [(fan_i, casing_i[0], "the fan in the inlet casing")]
    pairs += [(rotor_i[k], casing_i[1], "rotor %d in the compressor casing" % (k + 1)) for k in range(stages)]
    pairs.append((tblades_i, casing_i[3], "the turbine shroud in the turbine casing"))
    worst = None
    for rot, cas, what in pairs:
        c = clear(distance(rot, cas), what)
        assert_close(c.distance, tip_clearance, tol=2 * c.tolerance + 0.01, msg="%s: %s mm between %s and %s, asked %s" % (what, math.round(c.distance, 4), c.face_a, c.face_b, tip_clearance))
        worst = c.distance if worst == None else min(worst, c.distance)
    return worst


test("every rotor tip clears its casing by tip_clearance", tip_clearances)
test("... and at a tighter and a looser clearance", tip_clearances, sweep={"tip_clearance": [0.3, 0.8]})


def the_bearings_run():
    """Each ball sits in its two grooves with the 0.05 mm the profiles give
    it (groove radius 1.55 on a 1.5 ball), balls and races meshed at 0.005."""
    gap = GROOVE_R - BALL_R
    for ball, inner, outer, tag in ((ball_i[0], front_inner_i, front_outer_i, "front"), (ball_i[1], rear_inner_i, rear_outer_i, "rear")):
        for race, what in ((inner, "inner"), (outer, "outer")):
            c = clear(distance(ball, race), "%s ball in its %s race" % (tag, what))
            assert_close(c.distance, gap, tol=2 * c.tolerance + 0.002, msg="%s ball to the %s race: %s mm at %s, asked %s" % (tag, what, math.round(c.distance, 4), c.at, gap))
    return gap


test("the bearings run: every ball 0.05 mm off both grooves", the_bearings_run)


def the_spool_fits_its_shaft():
    """The hub and the turbine disc on their keyed steps, every compressor
    disc on the shaft, the outer races in the frames: slip fits, so nothing
    overlaps and nothing is looser than the fit plus the chord bands."""
    fits = [(shaft_i, fan_i, "the fan hub on the front key", FIT), (shaft_i, tdisc_i, "the turbine disc on the rear key", T_FIT)]
    fits += [(shaft_i, rotor_i[k], "disc %d on the shaft" % (k + 1), FIT) for k in range(stages)]
    fits += [(front_frame_i, front_outer_i, "the front outer race in its frame", FIT), (rear_frame_i, rear_outer_i, "the rear outer race in its frame", FIT)]
    for a, b, what, fit in fits:
        c = clear(distance(a, b), what)
        assert_true(c.distance <= fit + 2 * c.tolerance, msg="%s: %s mm between %s and %s is looser than the %s mm slip fit" % (what, math.round(c.distance, 4), c.face_a, c.face_b, fit))
    return len(fits)


test("the spool fits its shaft: keyed hub and turbine disc, slip-fit discs", the_spool_fits_its_shaft)


def the_first_joint_bolts_up():
    """Every bolt and nut of the inlet joint passes its hole and sits on its
    flange without sharing material with either casing."""
    for k in range(bolts):
        for i in (fastener_i[2 * k], fastener_i[2 * k + 1]):
            for cas in (casing_i[0], casing_i[1]):
                clear(distance(i, cas), "%s in %s" % (i.name, cas.name))
    return bolts


test("the inlet joint bolts up: eight bolts and nuts through both flanges, none touching material", the_first_joint_bolts_up)


def the_annulus_contracts():
    tips = [rotor_i[k].body().bbox()[1][1] for k in range(stages)]
    hubs = [r_hub(x_rotor(k)) for k in range(stages)]
    for k in range(1, stages):
        assert_true(tips[k] < tips[k - 1], msg="stage %d's tip radius %s is not below stage %d's %s" % (k + 1, tips[k], k, tips[k - 1]))
        assert_true(hubs[k] > hubs[k - 1], msg="stage %d's hub radius is not above stage %d's" % (k + 1, k))
    return (tips[0] - hubs[0]) - (tips[-1] - hubs[-1])


test("the annulus contracts stage by stage", the_annulus_contracts)


def the_cutaway_holds():
    """With the wedge taken out, every static part is still a solid and the
    rotor is untouched."""
    for i in casing_i + stator_i + [liner_i]:
        assert_solid(i.body(), msg=i.name)
    assert_solid(fan_i.body())
    return len(casing_i) + len(stator_i) + 1


test("the cutaway: every cut static part is a solid", the_cutaway_holds, sweep={"cutaway": [True]})

print("bodies", len(engine.bodies))
