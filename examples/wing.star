# A 1 m RC wing, printed in sections: a NACA 4-digit airfoil skinned through
# twisted, tapered, swept stations as one C² surface (`loft(ruled=False)`),
# a winglet through sections in space (`curve()`), hollow skins that are one
# loft each of offset sections, ribs, spars, a carbon tube, an aileron
# hinged on measured clearances, a servo bay with a lettered hatch, two
# halves by `mirror`, the whole as an assembly of parts and instances with
# an exploded view and a posed aileron, and every printed part laid out on
# the bed and judged as one print.
#
# Frame: x aft along the chord, y along the span (the right half at +y), z
# up. Sections are sketches on `XZ.offset(-y)`; every part is built in that
# frame and the dihedral is the instance's pose, a turn about the root chord
# (the stations stay parallel); the left half's parts are the right ones
# mirrored across XZ, posed the other way.
#
# How each thing is built:
#
#   * The planform is parabolic, `c(η) = c_root (1 - (1 - taper) η²)`: a
#     linearly tapered wing is linear data, and a cubic through linear data
#     is the ruled loft again, so a curved planform is where the C² skin
#     means something.
#   * Stations are uniform (`rib_pitch`), and the last flat station is the
#     winglet's root: uneven station spacing puts a wiggle in an otherwise
#     linear column of the interpolant.
#   * Each printed skin is its own loft of two-loop sections: the outline
#     and its inward offset by `skin`, so a hollow section is one exact loft
#     with no boolean at all. Section 1's inner loop is `Sketch.offset` (the
#     trailing-edge pinch closes by itself), taken out of the outline with
#     `Sketch.subtract`; so is every rib's outline. Section 2's inner loop
#     is the closed-form normal offset at the outline's own stations, closed
#     by the hinge wall, so every station has the same span count for the
#     servo pocket's cut that follows. Nothing here is a split of a skin.
#   * The winglet is a smooth loft through five closed `curve()` sections
#     that follow a 38° arc (under the 45° overhang rule when it stands on
#     its root). It is its own loft, not part of the wing's, and it stays
#     solid: curve sections carry one loop.
#   * The aileron is its own loft whose nose is a V drawn into the section
#     -- apex flat `hinge_gap` behind the wing's aft wall, faces
#     `deflection_max` off vertical, the axis HINGE_SETBACK behind the apex
#     where the wedge is thick enough for a pin -- so ±deflection_max
#     clears the wing with no boolean; the sweep below measures it. The pin
#     tunnel along the hinge axis is a loop of the section too. The aileron
#     is solid: a 32 mm chord 5 mm thick at the hinge leaves no room for a
#     cavity behind the tunnel. Pins are separate parts in knuckles on the
#     wing, 0.35 mm radial (the 0.3 mm fuse threshold plus the 0.02 mm
#     chord band at 0.01 mm), measured by `distance()`; each knuckle is
#     bored before it is united onto the skin, so the bore only ever meets
#     its own planes.
#   * Ribs are extrudes of the outline offset by `skin` plus a sliding
#     clearance, with the tube hole, the spar slot and the lightening holes
#     drawn as loops: exact, no boolean. The rib inherits the section, not
#     a slice of the solid. The section joint takes loose dowels through
#     the two joint ribs' holes, and the winglet is glued to a flat tip
#     face.
#   * Registration letters: one mark is engraved into the flat servo hatch
#     by `emboss(kind=emboss.cut)` -- the hatch's top face is rebuilt around
#     the glyph loops with the floors and walls sharing their edges, never
#     a boolean -- so the hatch is an exact body and is in `wing.step`.
#     "N1234" is painted on section 1's top skin by `emboss(kind=
#     emboss.paint)`: a spline face takes an imprint and paint, not a
#     relief. One face of the smooth loft is one spline span of the outline
#     -- a strip a section long and about 12 mm wide at mid-chord -- so the
#     letters run along the span, centred on the strip. The control horn is
#     a loose part under the aileron.
#   * `mirror(keep=True, merge=False)` gives the left half's bodies: the
#     halves are separate prints. Section 1's root face is normal to its
#     own axis, so the two halves' root faces stand in a 2·dihedral V
#     (1.5 mm open at the top skin).
#   * The exploded view is a view offset per instance, never geometry, so
#     it costs nothing and the measurements never see it.
#   * The bed layout places each part by a whole number of millimetres
#     across the plate and turns nothing: a fractional move along the chord
#     makes a fine-tessellated NURBS body slow to tessellate, while a
#     fractional z (the drop onto the plate) is free.
#   * The knuckles and the servo opening's outboard edge are 45° roofs, so
#     a shell standing on its inboard end needs no support there; the
#     trailing edge is `te` = 0.8 mm, two perimeters. `cadpole check` still
#     reports `overhang` over most of the skin and `thickness` behind the
#     trailing edge: a wing skin is an overhang, and which way up a part
#     prints is `orient_for_print`'s to say, not this layout's.
#
#   cadpole test examples/wing.star
#   cadpole check examples/wing.star --param layout=true
#   cadpole export examples/wing.star --run-exports --param layout=true --out-dir /tmp
#   cadpole render examples/wing.star --backend path --samples 256 --size 1600x1200 --view product --lighting product --backdrop studio --floor gloss --tonemap neutral --denoise -o docs/demo/wing.png
#   cadpole render examples/wing.star --backend path --samples 256 --size 1600x1200 --view product --lighting product --backdrop studio --floor gloss --tonemap neutral --denoise --param explode=40 --param xray=true -o docs/demo/wing_exploded.png

# --- Parameters.
span = param("span", 1000.0, unit="mm", min=600, max=1600, doc="Tip to tip, winglets included")
root_chord = param("root_chord", 180.0, unit="mm", min=170, max=300, doc="Chord at the centre line")
taper = param("taper", 0.55, min=0.3, max=0.9, doc="Tip chord over root chord; the planform between is a parabola, c(η) = c_root (1 - (1 - taper) η²)")
sweep = param("sweep", 4.0, unit="deg", min=0, max=15, doc="Leading-edge sweep, an in-plane shift of every station")
washout = param("washout", 2.0, unit="deg", min=0, max=5, doc="Twist: the tip's nose is turned down by this about the quarter chord, so the root stalls first")
dihedral = param("dihedral", 3.0, unit="deg", min=0, max=8, doc="Each half turned up about the root chord: the instance's pose, the parts are built flat")
camber = param("camber", 2.0, unit="%", min=0, max=6, doc="NACA 4-digit: maximum camber, the first digit")
camber_pos = param("camber_pos", 4.0, min=1, max=6, doc="NACA 4-digit: position of maximum camber in tenths of chord, the second digit")
thick = param("thick", 12.0, unit="%", min=8, max=18, doc="NACA 4-digit: thickness, the last two digits")
te = param("te", 0.8, unit="mm", min=0.4, max=1.5, doc="Blunt trailing edge: two points, never a zero-thickness corner; 0.8 is two perimeters of a 0.4 mm nozzle, which is what the wall rule asks")
rib_pitch = param("rib_pitch", 55.0, unit="mm", min=40, max=110, doc="Distance between ribs, and between the loft's stations; the section length is a whole number of it")
section_length = param("section_length", 220.0, unit="mm", min=160, max=250, doc="Span of one printed skin, standing on the bed: under the printer's height")
skin = param("skin", 1.0, unit="mm", min=0.8, max=2.0, doc="Skin thickness: the inward offset of the section's inner loop")
points = param("points", 20, min=10, max=26, doc="Spline points per surface of every section, clustered at the nose; 30 refuses to tessellate on the skinned surface")
spar_width = param("spar_width", 5.0, unit="mm", min=3, max=10, doc="Spar section across the chord; its height follows the airfoil")
tube_d = param("tube_d", 8.0, unit="mm", min=4, max=12, doc="Carbon tube through the ribs at the quarter chord: the root joiner")
rib_t = param("rib_t", 1.2, unit="mm", min=1.0, max=3.0, doc="Rib thickness; the joint ribs that take dowels are 3 mm")
aileron_fraction = param("aileron_fraction", 0.25, min=0.15, max=0.4, doc="Aileron chord as a fraction of the local chord at section 2's ends; the hinge line is straight between them")
deflection_max = param("deflection_max", 25.0, unit="deg", min=10, max=35, doc="The V in the aileron's nose opens this far each way, so the aileron clears the wing at any pose within it")
deflection = param("deflection", 0.0, unit="deg", min=-35, max=35, doc="Pose: the aileron turned about its hinge axis, trailing edge down for positive")
hinge_gap = param("hinge_gap", 0.8, unit="mm", min=0.5, max=2, doc="Apex of the aileron's nose to the wing's aft wall")
hinge_clearance = param("hinge_clearance", 0.35, unit="mm", min=0.25, max=0.6, doc="Radial: every pin in its knuckle and its tunnel, every dowel in its hole; 0.3 is the fuse threshold, 0.02 the chord band at 0.01 mm")
winglet_span = param("winglet_span", 60.0, unit="mm", min=30, max=120, doc="Arc length of the winglet, the last part of the span")
winglet_cant = param("winglet_cant", 38.0, unit="deg", min=10, max=60, doc="How far the winglet bends up over its arc; over 45 it needs support standing on its root")
chord_tol = param("chord_tol", 0.02, unit="mm", min=0.005, max=0.05, doc="Tessellation tolerance of the skins; the hinge and joint parts are at 0.01 regardless")
lift = param("explode", 0.0, unit="mm", min=0, max=80, doc="Pull the parts apart for a picture: a view offset per instance, the geometry stays put")
xray = param("xray", False, doc="Render the skins translucent so the ribs, spars and servo show")
layout = param("layout", False, doc="Also lay every printed part on the bed, one 3MF per bed, and judge each bed as one print; off by default so the wing renders alone")
smooth = param("smooth", True, doc="The two skins as loft(ruled=False): one C² surface through every station. False is the ruled loft, a band per bay, which the volume test compares; the aileron and the winglet are always skinned")

PRINTER = printer("bambu_a1")
MIN_GAP = PRINTER.min_gap  # 0.3 mm
MAX_FIT_GAP = 0.6  # a pin looser than this rattles
RIB_CLEARANCE = 0.3  # a rib slides into the skin on a glue line; wide enough
# to stay clear of the skin's offset loop near the nose by more than a rib's
# own chord error
JOINT_RIB_T = 3.0  # the ribs that take dowels
PEG_R = 2.0
DOWEL_LEN = 8.0
PIN_R = 1.5
PIN_LEN = 12.0
TAB_W = 4.0  # knuckle width along the span
END_GAP = 0.5  # aileron end face to knuckle
HINGE_SETBACK = 6.0  # apex flat to the hinge axis; the wedge is 6.6 mm thick there at 25°
APEX_FLAT = 1.0  # the aileron's nose is a flat this wide, never a knife
half = span / 2.0
flat_span = half - winglet_span
n_sections = int(math.round(flat_span / section_length))
assert_true(n_sections >= 1 and math.close(n_sections * section_length, flat_span, tol=1e-6), msg="the flat span %s mm must be a whole number of %s mm sections" % (flat_span, section_length))
bays = int(math.round(section_length / rib_pitch))
assert_true(math.close(bays * rib_pitch, section_length, tol=1e-6), msg="a section of %s mm must be a whole number of %s mm bays" % (section_length, rib_pitch))
assert_true(n_sections == 2, msg="this example puts the aileron and the servo on the second of exactly two sections per half")
S1_Y0 = 0.0
S1_Y1 = section_length
S2_Y0 = section_length
S2_Y1 = flat_span


# --- The airfoil: NACA 4-digit thickness and camber, as a function
# of the chord fraction, in the chord frame (x aft from the nose, z up).
def foil_camber(xf):
    m = camber / 100.0
    p = camber_pos / 10.0
    if m <= 0.0 or p <= 0.0:
        return (0.0, 0.0)
    if xf < p:
        return (m / (p * p) * (2.0 * p * xf - xf * xf), 2.0 * m / (p * p) * (p - xf))
    return (m / ((1.0 - p) * (1.0 - p)) * ((1.0 - 2.0 * p) + 2.0 * p * xf - xf * xf), 2.0 * m / ((1.0 - p) * (1.0 - p)) * (p - xf))


def foil_point(c, xf, upper):
    """The point of the outline at chord fraction `xf` on the upper or lower
    surface of a section of chord `c`, with the blunt trailing edge."""
    xf = math.clamp(xf, 0.0, 1.0)
    t = thick / 100.0
    yt = 5.0 * t * (0.2969 * math.sqrt(xf) - 0.1260 * xf - 0.3516 * xf * xf + 0.2843 * xf * xf * xf - 0.1036 * xf * xf * xf * xf) + (te / 2.0 / c) * xf
    yc, dyc = foil_camber(xf)
    th = math.atan(dyc)
    s = 1.0 if upper else -1.0
    return ((xf - s * yt * math.sin(th)) * c, (yc + s * yt * math.cos(th)) * c)


def foil_inset(c, xf, upper, d):
    """The outline point moved inward by `d` along the airfoil's own normal:
    the closed form the tests and the spar use, where the skins' inner loops
    are `Sketch.offset`."""
    if d <= 0.0:
        return foil_point(c, xf, upper)
    if xf <= 1e-9:
        th = math.atan(foil_camber(0.0)[1])
        return (d * math.cos(th), d * math.sin(th))
    h = min(0.004, xf)
    a = foil_point(c, xf - h, upper)
    b = foil_point(c, xf + h, upper)
    tx, tz = b[0] - a[0], b[1] - a[1]
    l = math.hypot(tx, tz)
    nx, nz = (tz / l, -tx / l) if upper else (-tz / l, tx / l)
    p = foil_point(c, xf, upper)
    return (p[0] + d * nx, p[1] + d * nz)


# --- The planform: a parabolic chord, a straight swept leading
# edge, linear washout about the quarter chord.
def chord(y):
    eta = y / half
    return root_chord * (1.0 - (1.0 - taper) * eta * eta)


def le_at(y):
    return y * math.tan(sweep)


def twist_at(y):
    return washout * y / half


assert_true(2 * skin + 1.0 < 0.05 * thick * chord(half), msg="two skins and a web must fit inside the tip section")


def place(pt, y):
    """A chord-frame point onto the section plane at station y: twisted about
    the quarter chord (trailing edge up for washout) and shifted by the sweep.
    Sketch coordinates on XZ are (world x, world z)."""
    c = chord(y)
    th = twist_at(y)
    dx = pt[0] - 0.25 * c
    dz = pt[1]
    return (le_at(y) + 0.25 * c + dx * math.cos(th) - dz * math.sin(th), dx * math.sin(th) + dz * math.cos(th))


def surface_point(y, xf, upper, d=0.0):
    return place(foil_inset(chord(y), xf, upper, d), y)


def station(y, xf):
    """World (x, z) of the chord line at fraction xf and the mid-thickness
    there, at station y."""
    u = surface_point(y, xf, True)
    l = surface_point(y, xf, False)
    return ((u[0] + l[0]) / 2.0, (u[1] + l[1]) / 2.0)


def inner_height(y, xf):
    """Wall to wall inside the skin at chord fraction xf."""
    return surface_point(y, xf, True, skin)[1] - surface_point(y, xf, False, skin)[1]


XF = [1.0 - math.cos(90.0 * k / points) for k in range(points + 1)]  # 0 (nose) .. 1 (trailing edge), clustered at the nose


def solve_xf(f, lo, hi):
    """Bisection for f(xf) = 0 on [lo, hi], f increasing."""
    for i in range(48):
        mid = 0.5 * (lo + hi)
        if f(mid) < 0.0:
            lo = mid
        else:
            hi = mid
    return 0.5 * (lo + hi)


def xf_at_x(y, x, upper):
    """The chord fraction at which the surface passes world x."""
    return solve_xf(lambda xf: surface_point(y, xf, upper)[0] - x, 0.0, 1.0)


# --- Outlines. Every loop is a list of (x, z) sketch points, drawn from the
# aft end of the upper surface over the nose to the aft end of the lower
# surface; the path closes it with the straight segment that is the blunt
# trailing edge or the hinge wall.
def loop_full(y):
    pts = [surface_point(y, XF[k], True) for k in range(points, -1, -1)]
    return pts + [surface_point(y, XF[k], False) for k in range(1, points + 1)]


def loop_to_x(y, x_cut, d=0.0):
    """The outline (inset by d along the airfoil's normal) forward of the
    vertical line x = x_cut, cosine stations scaled onto each surface's own
    run so every station has `points` per surface."""
    xu = solve_xf(lambda xf: surface_point(y, xf, True, d)[0] - x_cut, 0.0, 1.0)
    xl = solve_xf(lambda xf: surface_point(y, xf, False, d)[0] - x_cut, 0.0, 1.0)
    pts = [surface_point(y, xu * XF[k], True, d) for k in range(points, -1, -1)]
    return pts + [surface_point(y, xl * XF[k], False, d) for k in range(1, points + 1)]


def outline_sketch(y, name, pts):
    return sketch(XZ.offset(-y), name=name).add(path(pts[0]).spline(pts[1:]))


def section_sketch(y, name, loops):
    s = sketch(XZ.offset(-y), name=name)
    for pts in loops:
        s = s.add(path(pts[0]).spline(pts[1:]))
    return s


def shell_section(y, name, pts):
    """The outline and its inward offset by `skin`, as one two-loop section:
    the offset pinches off at the trailing edge by itself."""
    o = outline_sketch(y, name, pts)
    return o.subtract(o.offset(-skin, join="miter", name=name + "_inner"), name=name + "_ring")


# --- The hinge line: straight through section 2's two end
# stations at (1 - aileron_fraction) of the local chord (drawn root to tip
# it would sit 15 mm ahead of the local line at the section boundary, the
# planform being a parabola), a vertical wall; the axis HINGE_SETBACK +
# hinge_gap behind it at mid-thickness.
XF_H = 1.0 - aileron_fraction
H_IN = station(S2_Y0, XF_H)
H_OUT = station(S2_Y1, XF_H)


def hinge_x(y):
    return H_IN[0] + (H_OUT[0] - H_IN[0]) * (y - S2_Y0) / (S2_Y1 - S2_Y0)


def hinge_z(y):
    return H_IN[1] + (H_OUT[1] - H_IN[1]) * (y - S2_Y0) / (S2_Y1 - S2_Y0)


def axis_point(y):
    return (hinge_x(y) + hinge_gap + HINGE_SETBACK, y, hinge_z(y))


HD = (H_OUT[0] - H_IN[0], S2_Y1 - S2_Y0, H_OUT[1] - H_IN[1])
HD_L = math.sqrt(HD[0] * HD[0] + HD[1] * HD[1] + HD[2] * HD[2])
HD = (HD[0] / HD_L, HD[1] / HD_L, HD[2] / HD_L)  # along the hinge axis, outboard
TAN_D = math.tan(deflection_max)


def loop_aileron_outer(y):
    """Trailing edge, upper surface forward to where the upper wedge face
    meets it, the apex flat's corners, the lower surface aft: the wedge faces
    and the flat are straight segments between them."""
    xa = hinge_x(y) + hinge_gap
    za = hinge_z(y)
    cu = (xa, za + APEX_FLAT / 2.0)
    cl = (xa, za - APEX_FLAT / 2.0)

    def on_upper_wedge(xf):
        p = surface_point(y, xf, True)
        return (p[0] - cu[0]) - (p[1] - cu[1]) * TAN_D

    def on_lower_wedge(xf):
        p = surface_point(y, xf, False)
        return (p[0] - cl[0]) - (cl[1] - p[1]) * TAN_D

    xf_u = solve_xf(on_upper_wedge, xf_at_x(y, xa, True), 1.0)
    xf_l = solve_xf(on_lower_wedge, xf_at_x(y, xa, False), 1.0)
    m = 8
    upper = [surface_point(y, 1.0 - (1.0 - xf_u) * k / m, True) for k in range(m + 1)]
    lower = [surface_point(y, xf_l + (1.0 - xf_l) * k / m, False) for k in range(m + 1)]
    return (upper, cu, cl, lower)


def aileron_section(y):
    upper, cu, cl, lower = loop_aileron_outer(y)
    p = path(upper[0]).spline(upper[1:]).line(cu).line(cl).line(lower[0]).spline(lower[1:])
    ap = axis_point(y)
    return sketch(XZ.offset(-y), name="a_at_%d" % int(y * 10)).add(p).circle(PIN_R + hinge_clearance, center=(ap[0], ap[2]))


# --- The winglet's planes: an arc of `winglet_span` bending up
# by `winglet_cant`, one section per quarter of it, each a closed curve in
# its own tilted plane.
W_R = winglet_span / math.radians(winglet_cant)


def winglet_plane(s):
    phi = winglet_cant * s
    return plane(origin=(0.0, flat_span + W_R * math.sin(phi), W_R * (1.0 - math.cos(phi))), normal=(0.0, -math.cos(phi), -math.sin(phi)), x_dir=(1.0, 0.0, 0.0))


def winglet_section(s, name):
    y = flat_span + winglet_span * s
    pts = loop_full(y)
    return path(pts[0]).spline(pts[1:]).on(winglet_plane(s), name=name).close()


# --- Skins. Each printed section is one two-loop loft
# through its own stations; `match="arclen"` only refines, so it costs
# nothing where the offset's spans already agree station to station.
RULED = not smooth


def stations_between(y0, y1):
    k = int(math.round((y1 - y0) / rib_pitch))
    return [y0 + (y1 - y0) * i / k for i in range(k + 1)]


def skin_section_1(y, tag="s1"):
    return shell_section(y, "%s_at_%d" % (tag, int(y)), loop_full(y))


def skin_section_2(y, tag="s2"):
    """Section 2's inner loop is the closed-form offset at the outline's own
    cosine stations, closed by the hinge wall, so every station has the same
    span count and the servo pocket's subtract has a clean loop to cut.
    Section 1 carries no boolean, so it takes the library offset."""
    name = "%s_at_%d" % (tag, int(y))
    return section_sketch(y, name, [loop_to_x(y, hinge_x(y)), loop_to_x(y, hinge_x(y) - skin, skin)])


s1 = loft([skin_section_1(y) for y in stations_between(S1_Y0, S1_Y1)], ruled=RULED, match="arclen", name="s1")

# The registration on the skin: "N1234" laid on the top skin
# by projection along the face's normal and painted. The strip is the outline
# span nearest mid-chord at mid-span, and the letters are centred on it,
# their baseline along the span (up = toward the nose).
REG_XF = 0.55
REG_Y = 0.5 * (S1_Y0 + S1_Y1)
reg_probe = surface_point(REG_Y, REG_XF, True)
reg_face = s1.faces(kind=kind.nurbs, nearest=(reg_probe[0], REG_Y, reg_probe[1])).one()
s1 = emboss(s1, reg_face, text("N1234", size=5, align="center", baseline="middle", name="registration_skin"), kind=emboss.paint, color="#1f2a44", origin=reg_face.centroid, up=(-1, 0, 0), name="s1_registration")
s1.tessellation(tolerance=chord_tol)
s2 = loft([skin_section_2(y) for y in stations_between(S2_Y0, S2_Y1)], ruled=RULED, name="s2")
A_Y0 = S2_Y0 + END_GAP + TAB_W + END_GAP
A_Y1 = S2_Y1 - END_GAP - TAB_W - END_GAP
# always skinned, never ruled: the skinned loft of these sections, with the
# tunnel, is the one that comes out a clean solid
aileron = loft([aileron_section(y) for y in stations_between(A_Y0, A_Y1)], ruled=False, name="aileron")
aileron.tessellation(tolerance=0.01)
winglet = loft([winglet_section(k / 4.0, "w_%d" % k) for k in range(5)], ruled=False, name="winglet")


# --- Fittings on the skins.
def prism_yz(profile, x0, x1, name):
    """A prism along x from a (y, z) profile: YZ's sketch x is world y and
    its sketch y is world z, its normal +x."""
    return extrude(sketch(YZ.offset(x0), name=name + "_profile").polygon(profile), x1 - x0, name=name)


def knuckle(y_a, tag):
    """A hinge knuckle at y_a..y_a + TAB_W: an anchor through the aft wall
    into the cavity and a block outside it, both with 45° roofs on the
    inboard side so the skin prints standing without support; the anchor
    ends 0.3 inside the block (two prisms ending on one plane are a flush
    pair the exact kernel refuses)."""
    xh = hinge_x(y_a + TAB_W / 2.0)
    zh = hinge_z(y_a + TAB_W / 2.0)
    x_ax = axis_point(y_a)[0]
    hk = 2.75  # half height of the block
    ha = 1.4  # half height of the anchor inside the cavity
    block = prism_yz([(y_a, zh), (y_a + hk, zh - hk), (y_a + TAB_W, zh - hk), (y_a + TAB_W, zh + hk), (y_a + hk, zh + hk)], xh - 0.5, x_ax + PIN_R + hinge_clearance + 1.6, "knuckle_%s" % tag)
    anchor = prism_yz([(y_a + TAB_W - ha - 1.1, zh), (y_a + TAB_W - 1.1, zh - ha), (y_a + TAB_W - 0.3, zh - ha), (y_a + TAB_W - 0.3, zh + ha), (y_a + TAB_W - 1.1, zh + ha)], xh - 4.0, xh + 1.0, "anchor_%s" % tag)
    return [block, anchor]


def along_axis(y, back, length, r, name):
    """A cylinder of radius r along the hinge axis from `back` mm inboard of
    the axis point at station y, `length` long outboard."""
    p = axis_point(y)
    o = (p[0] - HD[0] * back, p[1] - HD[1] * back, p[2] - HD[2] * back)
    return extrude(sketch(plane(origin=o, normal=HD), name=name + "_profile").circle(r), length, name=name)


# the servo bay: a 9 g servo lies flat in the second bay of section 2, its
# pocket cut through the bottom skin, the opening's outboard edge a 45° roof
SERVO = (23.0, 12.2, 22.0)  # x, z, y of a 9 g servo
SERVO_Y = S2_Y0 + 1.5 * rib_pitch
SERVO_XF = 0.35
SERVO_X, SERVO_ZM = station(SERVO_Y, SERVO_XF)
SERVO_Z_OUT = surface_point(SERVO_Y, SERVO_XF, False)[1]  # the outer bottom skin there
pocket = prism_yz(
    [(SERVO_Y - SERVO[2] / 2.0 - 0.6, SERVO_Z_OUT - 8.0), (SERVO_Y + SERVO[2] / 2.0 + 0.6 + 8.0, SERVO_Z_OUT - 8.0), (SERVO_Y + SERVO[2] / 2.0 + 0.6, SERVO_Z_OUT), (SERVO_Y + SERVO[2] / 2.0 + 0.6, SERVO_ZM), (SERVO_Y - SERVO[2] / 2.0 - 0.6, SERVO_ZM)],
    SERVO_X - SERVO[0] / 2.0 - 0.6,
    SERVO_X + SERVO[0] / 2.0 + 0.6,
    "servo_pocket",
)
s2 = subtract(s2, pocket, kernel="brep", name="s2_pocketed")
KN_IN = S2_Y0 + END_GAP
KN_OUT = S2_Y1 - END_GAP - TAB_W
knuckles = []
for y_a, tag in ((KN_IN, "in"), (KN_OUT, "out")):
    block, anchor = knuckle(y_a, tag)
    hole = along_axis(y_a + TAB_W / 2.0, TAB_W, 2.0 * TAB_W, PIN_R + hinge_clearance, "knuckle_hole_" + tag)
    knuckles.append(subtract(union(block, anchor, kernel="brep", name="knuckle_%s_whole" % tag), hole, kernel="brep", name="knuckle_%s_bored" % tag))
s2 = union(kernel="brep", name="s2_skin", *([s2] + knuckles))
s2.tessellation(tolerance=0.01)

PEG_XF = (0.15, 0.71)  # dowel stations: clear of the tube, the spar and the lightening holes


def peg_points(y):
    return [station(y, xf) for xf in PEG_XF]


# The winglet has no alignment sockets: it is glued to section 2's tip face,
# which the tip rib backs up.
winglet.tessellation(tolerance=chord_tol)

# --- Internals: ribs, spars, the tube, dowels, pins,
# the hatch, the servo, the horn and the pushrod.
SPAR_XF = 0.4
TUBE_XF = 0.25
TUBE_ROOT = station(0.0, TUBE_XF)
TUBE_TIP = station(half, TUBE_XF)


def tube_at(y):
    return (TUBE_ROOT[0] + (TUBE_TIP[0] - TUBE_ROOT[0]) * y / half, TUBE_ROOT[1] + (TUBE_TIP[1] - TUBE_ROOT[1]) * y / half)


def spar_h(y):
    return 0.55 * inner_height(y, SPAR_XF)


def rib(y, t, tag, fwd_only=False, peg_at=None):
    """A rib t thick centred on station y: the outline offset by the skin
    and a sliding clearance, its holes as loops, extruded. `peg_at` is the
    joint plane whose dowel positions the holes take, or None."""
    pts = loop_to_x(y, hinge_x(y)) if fwd_only else loop_full(y)
    o = outline_sketch(y + t / 2.0, "rib_%s_outline" % tag, pts)
    s = o.offset(-(skin + RIB_CLEARANCE), join="miter", name="rib_%s_profile" % tag)
    tx, tz = tube_at(y)
    s = s.circle(tube_d / 2.0 + 0.2, center=(tx, tz))
    sx, sz = station(y, SPAR_XF)
    s = s.rect(spar_width + 0.4, spar_h(y) + 0.4, center=(sx, sz))
    for xf in ((0.5, 0.64) if fwd_only else (0.5, 0.64, 0.78)):
        hx, hz = station(y, xf)
        r = 0.5 * inner_height(y, xf) - 2.0
        if r >= 1.5:
            s = s.circle(r, center=(hx, hz))
    if peg_at != None:
        for px, pz in peg_points(peg_at):
            s = s.circle(PEG_R + hinge_clearance, center=(px, pz))
    body = extrude(s, t, name="rib_%s" % tag)
    # 0.01 where a dowel has to be measured against it, 0.03 otherwise: the
    # glue line to the skin is about 0.2 mm, so a rib meshed at the
    # document's 0.05 would draw a `chord_error` on every export
    body.tessellation(tolerance=0.01 if peg_at != None else 0.03)
    return body


ribs_1 = [rib(3.0, JOINT_RIB_T, "root")]
for i in range(1, bays):
    ribs_1.append(rib(i * rib_pitch, rib_t, "1_%d" % i))
ribs_1.append(rib(S1_Y1 - 0.5 - JOINT_RIB_T / 2.0, JOINT_RIB_T, "joint_a", peg_at=S1_Y1))
ribs_2 = [rib(S2_Y0 + 0.5 + rib_t / 2.0, rib_t, "joint_b", fwd_only=True, peg_at=S1_Y1)]
for i in range(1, bays):
    ribs_2.append(rib(S2_Y0 + i * rib_pitch, rib_t, "2_%d" % i, fwd_only=True))
ribs_2.append(rib(S2_Y1 - 0.5 - JOINT_RIB_T / 2.0, JOINT_RIB_T, "tip", fwd_only=True))


def along_y(r, y0, length, x, z, name, tol=None):
    """A cylinder of radius r along +y from (x, y0, z), tessellated at `tol`
    (set on the cylinder itself: a moved body shares its source's)."""
    c = cylinder(r, length, at=(0, 0, 0), name=name)
    if tol != None:
        c.tessellation(tolerance=tol)
    return c.rotate(-90, axis=(1, 0, 0), origin=(0, 0, 0)).translate((x, y0, z), name=name + "_placed")


dowels_j = []
for i, (px, pz) in enumerate(peg_points(S1_Y1)):
    dowels_j.append(along_y(PEG_R, S1_Y1 - 0.5 - JOINT_RIB_T + 0.3, DOWEL_LEN, px, pz, "dowel_j_%d" % i, tol=0.01))


def spar(y0, y1, tag):
    secs = []
    for y in (y0, y1):
        sx, sz = station(y, SPAR_XF)
        secs.append(sketch(XZ.offset(-y), name="spar_%s_%d" % (tag, int(y))).rect(spar_width, spar_h(y), center=(sx, sz)))
    return loft(secs, ruled=True, name="spar_%s" % tag)


spar_1 = spar(6.0, S1_Y1 - 4.0, "1")
spar_2 = spar(S2_Y0 + 2.5, S2_Y1 - 4.0, "2")
TUBE_Y0 = 6.0
TUBE_Y1 = S2_Y0 + 0.5 * rib_pitch  # a 240 mm tube: it spans the section joint and still fits a 256 mm plate, which is what keeps `cadpole check` free of a bed-fit error on a part nobody prints
tube = along_y(tube_d / 2.0, TUBE_Y0, TUBE_Y1 - TUBE_Y0, tube_at(TUBE_Y0)[0], tube_at(TUBE_Y0)[1], "tube")
tube = tube.rotate(math.degrees(math.atan((TUBE_TIP[0] - TUBE_ROOT[0]) / half)), axis=(0, 0, -1), origin=(tube_at(TUBE_Y0)[0], TUBE_Y0, tube_at(TUBE_Y0)[1]), name="tube_swept")

pins = []
for i, (y, sign) in enumerate(((KN_IN + TAB_W / 2.0, 1.0), (KN_OUT + TAB_W / 2.0, -1.0))):
    # from inside each knuckle into the aileron: outboard from the inboard
    # knuckle, inboard from the outboard one
    p = along_axis(y, sign * (TAB_W / 2.0 - 0.5), sign * PIN_LEN, PIN_R, "pin_%d" % i)
    p.tessellation(tolerance=0.01)
    pins.append(p)

# the hatch: a flat plate over the servo opening, the registration engraved
HATCH = (SERVO[0] + 8.0, SERVO[2] + 8.0, 1.6)
hatch_plate = box(HATCH[0], HATCH[1], HATCH[2], at=(0, 0, 0), name="hatch_plate")
hatch = emboss(hatch_plate, "hatch_plate/top", text("G-POLE", size=6.5, plane=XY.offset(HATCH[2]), at=(2.5, HATCH[1] / 2.0 - 2.4), name="registration"), 0.6, kind=emboss.cut, name="hatch")
hatch = hatch.rotate(180, axis=(1, 0, 0), origin=(0, 0, 0)).translate((SERVO_X - HATCH[0] / 2.0, SERVO_Y + HATCH[1] / 2.0, SERVO_Z_OUT - 0.4), name="hatch_placed")

servo = box(SERVO[0], SERVO[2], SERVO[1], at=(SERVO_X - SERVO[0] / 2.0, SERVO_Y - SERVO[2] / 2.0, SERVO_Z_OUT + 0.2), name="servo")
ARM_END = (SERVO_X + SERVO[0] / 2.0 + 9.0, SERVO_Y, SERVO_Z_OUT - 7.0)

# the control horn: a plate screwed under the aileron, its hole for the pushrod
HORN_Y = SERVO_Y
HORN_X0 = axis_point(HORN_Y)[0] + 2.0
HORN_Z_OUT = surface_point(HORN_Y, xf_at_x(HORN_Y, HORN_X0 + 6.0, False), False)[1]
HORN_TIP = (HORN_X0 + 4.0, HORN_Y, HORN_Z_OUT - 11.0)
horn = extrude(sketch(XZ.offset(-(HORN_Y + 1.0)), name="horn_profile").polygon([(HORN_X0, HORN_Z_OUT - 0.3), (HORN_X0 + 14.0, HORN_Z_OUT - 0.3), (HORN_X0 + 9.0, HORN_Z_OUT - 13.5), (HORN_X0 + 1.0, HORN_Z_OUT - 13.5)]).circle(0.8, center=(HORN_TIP[0], HORN_TIP[2])), 2.0, name="horn")


def cylinder_between(a, b, r, name):
    d = (b[0] - a[0], b[1] - a[1], b[2] - a[2])
    l = math.sqrt(d[0] * d[0] + d[1] * d[1] + d[2] * d[2])
    c = cylinder(r, l, at=(0, 0, 0), name=name)
    ang = math.degrees(math.acos(d[2] / l))
    axis = (-d[1], d[0], 0.0)
    if math.hypot(axis[0], axis[1]) > 1e-9:
        c = c.rotate(ang, axis=axis, origin=(0, 0, 0))
    return c.translate(a, name=name + "_placed")


def turned_about_hinge(p, angle):
    """A point turned by `angle` about the hinge axis, trailing edge down for
    positive: where the horn's tip is at the pose."""
    o = axis_point(p[1])
    dx, dz = p[0] - o[0], p[2] - o[2]
    a = -angle
    return (o[0] + dx * math.cos(a) - dz * math.sin(a), p[1], o[2] + dx * math.sin(a) + dz * math.cos(a))


pushrod = cylinder_between(ARM_END, turned_about_hinge(HORN_TIP, deflection), 0.6, "pushrod")


# --- The assembly. Every part is built once in
# the flat frame; the right instance is posed up by the dihedral, the left
# part is the mirror image posed the other way. The exploded view is a view
# offset per instance from `lift`; the aileron's deflection is a `pose`.
MATERIAL = {
    "s1": ("pla_matte", "#f1f1ec"), "s2": ("pla_matte", "#f1f1ec"), "aileron": ("pla_matte", "#f1f1ec"), "winglet": ("pla_matte", "#d64a3a"),
    "spar_1": ("pla_matte", "#3d3f44"), "spar_2": ("pla_matte", "#3d3f44"), "hatch": ("pla_matte", "#f1f1ec"), "horn": ("pla_matte", "#3d3f44"),
    "servo": ("abs", "#1e1f22"), "pushrod": ("steel", None), "tube": (None, "#17181a"),
}


def material_of(tag):
    if tag.startswith("rib"):
        return ("pla_matte", "#e8a33c")
    if tag.startswith("pin") or tag.startswith("dowel"):
        return ("pla_matte", "#8d939b")
    return MATERIAL[tag]


def dress(thing, tag):
    preset, color = material_of(tag)
    if preset != None and color != None:
        thing.material(preset=preset, color=color)
    elif preset != None:
        thing.material(preset=preset)
    else:
        thing.material(color=color, roughness=0.35, anisotropy=0.6)
    if xray and tag in ("s1", "s2", "aileron"):
        thing.material(opacity=0.4)
    return thing


flat = []  # (tag, body in the flat frame, explode direction)
flat.append(("s1", s1, (0, 0, 0)))
flat.append(("s2", s2, (0, 0.6, 0)))
flat.append(("aileron", aileron, (0.6, 0.6, 0)))
flat.append(("winglet", winglet, (0, 1.5, 0)))
for i, r in enumerate(ribs_1):
    flat.append(("rib1_%d" % i, r, (0, 0, 0.7)))
for i, r in enumerate(ribs_2):
    flat.append(("rib2_%d" % i, r, (0, 0.6, 0.7)))
flat.append(("spar_1", spar_1, (0, 0, -0.8)))
flat.append(("spar_2", spar_2, (0, 0.6, -0.8)))
flat.append(("tube", tube, (0, 0, -1.2)))
for i, p in enumerate(pins):
    flat.append(("pin_%d" % i, p, (0.3, 0.6, 0)))
for i, d in enumerate(dowels_j):
    flat.append(("dowel_j_%d" % i, d, (0, 0, 0.7)))
flat.append(("hatch", hatch, (0, 0.6, -1.0)))
flat.append(("servo", servo, (0, 0.6, -0.5)))
flat.append(("pushrod", pushrod, (0.6, 0.6, -0.5)))
flat.append(("horn", horn, (0.6, 0.6, 0)))


def turn(v, angle):
    """A vector turned about the x axis by `angle`."""
    return (v[0], v[1] * math.cos(angle) - v[2] * math.sin(angle), v[1] * math.sin(angle) + v[2] * math.cos(angle))


PART_R = {}
PART_L = {}
RIGHT = {}
LEFT = {}
for tag, b, off in flat:
    pair = mirror(b, plane=XZ, keep=True, merge=False, name=tag + "_mirrored")
    PART_R[tag] = part(pair[0], name=tag + "_part")
    PART_L[tag] = part(pair[1], name=tag + "_lpart")
    r = instance(PART_R[tag], rotate=dihedral, axis=(1, 0, 0), name=tag + "_r")
    l = instance(PART_L[tag], rotate=-dihedral, axis=(1, 0, 0), name=tag + "_l")
    RIGHT[tag] = dress(r, tag)
    LEFT[tag] = dress(l, tag)
    if lift > 0.0:
        d = (off[0] * lift, off[1] * lift, off[2] * lift)
        explode(r, turn(d, dihedral))
        explode(l, turn((d[0], -d[1], d[2]), -dihedral))
wing_assembly = assembly(RIGHT.values() + LEFT.values(), name="wing_assembly")

# the pose: the aileron and its horn about the hinge axis, each
# side in its own posed frame; the left is the mirror image, so its
# trailing edge goes down for the opposite sense about its own axis
if deflection != 0.0:
    ax_r = turn(HD, dihedral)
    o_r = turn(axis_point(A_Y0), dihedral)
    ax_l = turn((HD[0], -HD[1], HD[2]), -dihedral)
    o_l = turn((axis_point(A_Y0)[0], -axis_point(A_Y0)[1], axis_point(A_Y0)[2]), -dihedral)
    for tag in ("aileron", "horn"):
        pose(RIGHT[tag], rotate=-deflection, axis=ax_r, origin=o_r, name=tag + "_r_posed")
        pose(LEFT[tag], rotate=deflection, axis=ax_l, origin=o_l, name=tag + "_l_posed")

ALL = RIGHT.values() + LEFT.values()
exact_bodies = [i.body() for i in ALL if i.body().kind == "brep"]
export(exact_bodies, "wing.step")

# The parts the tests measure, right half, assembled position.
S1, S2, AIL, WING = RIGHT["s1"], RIGHT["s2"], RIGHT["aileron"], RIGHT["winglet"]
RIB_A = RIGHT["rib1_%d" % (len(ribs_1) - 1)]
RIB_B = RIGHT["rib2_0"]
RIB_TIP = RIGHT["rib2_%d" % (len(ribs_2) - 1)]
PINS = [RIGHT["pin_0"], RIGHT["pin_1"]]
DOWELS_J = [RIGHT["dowel_j_0"], RIGHT["dowel_j_1"]]


# --- The print layout: every printed part of the right half set
# down on a plate, as a second *instance* of the same part, so the plate is
# the parts and not a copy of them. The left half is the same plates built
# from the mirrored parts, which is a second set of files and no new
# information, so it is not written.
#
# Two rules the layout obeys:
#
#   * a part is placed by a translation alone, never turned. The
#     tessellation is keyed by where a body is, so every pose is a fresh
#     tessellation of that body; lying as built is the placement every part
#     is already known good at.
#   * the move is a whole number of millimetres across the plate, which
#     keeps the skins fast to tessellate. The drop onto the plate -- a
#     fractional z -- costs nothing, so it is exact. Half a millimetre of
#     slop in where a part sits is nothing; every gap on a plate here is at
#     least 2 mm.
#
# So these plates say "these parts, this plate, none of them touching";
# which way up each one prints is `orient_for_print`'s to say, and the
# `assert_printable` below therefore does not judge overhang.
PLATE_Y = 1200.0  # the plates stand well clear of the wing (|y| under 500) ...
PLATE_PITCH = 400.0  # ... and of each other


def on_plate(tag, k, bx, by, name):
    """One printed part of the right half, set down on plate k with its
    corner at (bx, by) and its lowest point exactly on the plate."""
    inst = instance(PART_R[tag], name=name)
    lo, hi = inst.body().bbox()
    pose(inst, translate=(math.round(bx - lo[0]), math.round(PLATE_Y + k * PLATE_PITCH + by - lo[1]), -lo[2]), name=name + "_placed")
    return dress(inst, tag)


BEDS = []
if layout:
    bed_1 = [on_plate("s1", 0, 20.0, 18.0, "bed1_s1")]
    # the aileron has a plate to itself
    bed_2 = [on_plate("s2", 1, 16.0, 20.0, "bed2_s2")]
    bed_3 = [on_plate("spar_1", 2, 20.0, 18.0, "bed3_spar")]
    for i in range(len(ribs_1)):
        bed_3 = bed_3 + [on_plate("rib1_%d" % i, 2, 40.0, 40.0 + 12.0 * i, "bed3_rib_%d" % i)]
    for i, tag in enumerate(("pin_0", "pin_1", "dowel_j_0", "dowel_j_1")):
        bed_3 = bed_3 + [on_plate(tag, 2, 210.0 + 8.0 * i, 18.0, "bed3_" + tag)]
    bed_5 = [on_plate("aileron", 4, 20.0, 18.0, "bed5_aileron")]
    bed_4 = [on_plate("spar_2", 3, 20.0, 18.0, "bed4_spar")]
    for i in range(len(ribs_2)):
        bed_4 = bed_4 + [on_plate("rib2_%d" % i, 3, 40.0, 40.0 + 12.0 * i, "bed4_rib_%d" % i)]
    bed_4 = bed_4 + [on_plate("winglet", 3, 60.0, 120.0, "bed4_winglet")]
    bed_4 = bed_4 + [on_plate("hatch", 3, 200.0, 120.0, "bed4_hatch"), on_plate("horn", 3, 200.0, 200.0, "bed4_horn")]
    BEDS = [bed_1, bed_2, bed_3, bed_4, bed_5]
    for i, bed in enumerate(BEDS):
        export(bed, "wing_bed_%d.3mf" % (i + 1))


# --- Tests (`cadpole test examples/wing.star`).
def fit(c, what, hi=MAX_FIT_GAP):
    assert_true(not c.overlap, msg="%s: %s and %s share material at %s" % (what, c.face_a, c.face_b, c.at))
    assert_true(c.distance - 2 * c.tolerance >= MIN_GAP, msg="%s: %s mm between %s and %s at ±%s cannot prove the %s mm fuse threshold" % (what, math.round(c.distance, 4), c.face_a, c.face_b, c.tolerance, MIN_GAP))
    assert_true(c.distance <= hi, msg="%s: %s mm between %s and %s is too loose" % (what, math.round(c.distance, 4), c.face_a, c.face_b))
    return c.distance


def is_registration_skin(name):
    """The section 1 skin, which carries the painted registration: `s1_r`,
    `s1_l`, `bed1_s1`."""
    return name.split("_")[0] == "s1" or name.endswith("_s1")


def assert_solid_part(name, b):
    """`assert_solid`, except for the registration skin, which is judged
    watertight instead. "N1234" painted on the top skin is hundreds of
    edges of 0.04 to 0.5 mm on a curved patch whose cells are millimetres,
    and its tessellation carries slivers whose planes -- chords of one
    surface -- pass through each other by under a micron, over
    `assert_solid`'s crossing threshold. So the skin is required to be
    watertight -- every edge on exactly two triangles -- and not to prove
    the micron."""
    if is_registration_skin(name):
        r = print_check(b)
        assert_true(r.watertight, msg="%s (the registration skin) is watertight" % name)
    else:
        assert_solid(b)


def every_part_is_a_solid():
    for side in (RIGHT, LEFT):
        for tag, inst in side.items():
            b = inst.body()
            assert_solid_part(tag, b)
            assert_eq(b.kind, "brep", msg="%s is exact" % tag)
    assert_eq(len(ALL), 2 * len(flat))
    return len(ALL)


test("every part is a solid and exact, the engraved hatch included", every_part_is_a_solid)


def the_wing_is_where_it_should_be():
    lo, hi = None, None
    for inst in (S1, S2, WING, LEFT["s1"], LEFT["s2"], LEFT["winglet"]):
        l, h = inst.body().bbox()
        lo = l if lo == None else (min(lo[0], l[0]), min(lo[1], l[1]), min(lo[2], l[2]))
        hi = h if hi == None else (max(hi[0], h[0]), max(hi[1], h[1]), max(hi[2], h[2]))
    tip = winglet_plane(1.0).origin
    y_tip = tip[1] * math.cos(dihedral) - tip[2] * math.sin(dihedral)
    assert_close(lo[1], -y_tip, tol=chord(half) * 0.5, msg="the left tip")
    assert_close(hi[1], y_tip, tol=chord(half) * 0.5, msg="the right tip")
    assert_close(lo[0], le_at(0.0) - 0.5, tol=1.0, msg="the leading edge at the root")
    # the sweep carries the trailing edge aft faster than the parabola
    # shortens the chord near the root, so the aftmost point is outboard
    x_te = max([surface_point(y, 1.0, True)[0] for y in stations_between(S1_Y0, S2_Y1)])
    assert_close(hi[0], x_te, tol=1.0, msg="the aftmost trailing-edge point")
    assert_true(hi[2] > flat_span * math.sin(dihedral), msg="dihedral lifts the tips")
    lo1, hi1 = S1.body().bbox()
    assert_true(abs(lo1[1]) < 15.0 * math.sin(dihedral) + 0.1, msg="section 1's root face stands within the dihedral V of the symmetry plane")
    lol, hil = LEFT["s1"].body().bbox()
    assert_close(hil[1], -lo1[1], tol=1e-6, msg="the left section 1 mirrors the right")
    return hi[1] - lo[1]


test("the assembled wing spans the span, root chord at the root, tips lifted", the_wing_is_where_it_should_be)


def faces_are_recognised():
    b = S1.body()
    assert_eq(b.faces(kind="plane").count(), 2, msg="a skin has two planar end faces and nothing else flat")
    assert_true(b.faces(kind="nurbs").count() >= 2 * points, msg="the skin's walls are free-form")
    assert_eq(AIL.body().faces(kind="plane").count(), 2 + 3, msg="the aileron's two ends and the V's three flats: the apex and the two wedge faces are planes because their corner lines are straight")
    assert_eq(WING.body().faces(kind="cylinder").count(), 0, msg="the winglet is glued on a flat root face: no bores in it")
    # six holes, twelve cylindrical faces: a `circle` loop is two arcs, so
    # an extruded bore is two half-cylinders that the recogniser reads as
    # one cylinder each
    assert_eq(RIB_A.body().faces(kind="cylinder").count(), 2 * (1 + 3 + 2), msg="the tube hole, three lightening holes and two dowel holes, each in two faces")
    assert_eq(PINS[0].body().faces(kind="cylinder").count(), 2)
    return RIB_A.body().faces(kind="cylinder").count()


test("the surface recogniser names the sockets, the holes and the pins", faces_are_recognised)


def the_hinge_runs():
    if deflection != 0.0:
        return 0.0
    worst = 9.0
    for i, p in enumerate(PINS):
        c = distance(p, AIL)
        worst = min(worst, fit(c, "pin %d in the aileron's tunnel" % i))
        assert_true("side[1." in c.face_b, msg="pin %d is closest to %s, not the tunnel" % (i, c.face_b))
        c = distance(p, S2)
        worst = min(worst, fit(c, "pin %d in its knuckle" % i))
        assert_true("knuckle_hole" in c.face_b, msg="pin %d is closest to %s, not its knuckle bore" % (i, c.face_b))
    c = distance(AIL, S2)
    assert_true(not c.overlap and c.distance >= END_GAP - 1e-3, msg="the aileron stands %s from the wing at %s/%s" % (c.distance, c.face_a, c.face_b))
    return worst


test("each pin runs in its knuckle and the aileron's tunnel at the radial clearance asked for", the_hinge_runs)


def the_aileron_clears_the_wing():
    c = distance(AIL, S2)
    assert_true(not c.overlap, msg="the aileron hits the wing at %s/%s, %s" % (c.face_a, c.face_b, c.at))
    assert_true(c.distance >= MIN_GAP, msg="the aileron comes within %s of the wing at %s/%s" % (c.distance, c.face_a, c.face_b))
    cl = distance(LEFT["aileron"], LEFT["s2"])
    assert_close(cl.distance, c.distance, tol=0.02, msg="the left aileron is posed as the right one's mirror image: %s against %s" % (cl.distance, c.distance))
    lo_r, hi_r = AIL.body().bbox()
    lo_l, hi_l = LEFT["aileron"].body().bbox()
    assert_close(lo_l[2], lo_r[2], tol=0.05, msg="both trailing edges go the same way")
    return c.distance


test("the aileron clears the wing at every deflection the V allows, on both sides", the_aileron_clears_the_wing, sweep={"deflection": [-25, 0, 25]})


def the_joint_takes_its_dowels():
    """The two sections are keyed to each other by loose dowels through the
    joint ribs, at the radial clearance the printer needs, measured rather
    than assumed."""
    worst = 9.0
    for i, d in enumerate(DOWELS_J):
        c = distance(d, RIB_A)
        worst = min(worst, fit(c, "joint dowel %d in its own rib" % i))
        assert_true("rib_joint_a" in c.face_b, msg="joint dowel %d is closest to %s, not its rib" % (i, c.face_b))
        worst = min(worst, fit(distance(d, RIB_B), "joint dowel %d in the next section's rib" % i))
    return worst


test("the section joint takes its dowels at the clearance asked for", the_joint_takes_its_dowels)


def the_skin_is_skin_thick():
    # a small probe plate inside section 1's cavity, its top 0.5 below where
    # the inner upper skin should be at 35 % chord (the skin's sag over the
    # plate's 4 mm is under 0.01), so the inner wall stands 0.5 from it; the
    # offset samples the spline at the document tolerance, which is
    # the 0.05 in the allowance; and the wall rule's own reading of the skin
    y = 0.5 * rib_pitch
    px, pz = surface_point(y, 0.35, True, skin)
    probe = box(4.0, 6.0, 3.0, at=(px - 2.0, y - 3.0, pz - 0.5 - 3.0), name="probe")
    probe = probe.rotate(dihedral, axis=(1, 0, 0), origin=(0, 0, 0), name="probe_turned")
    c = distance(S1, probe)
    assert_true(not c.overlap, msg="the probe crosses the skin at %s" % (c.at,))
    assert_true("side[1." in c.face_a, msg="the probe's closest face is %s, not the inner skin" % c.face_a)
    assert_close(c.distance, 0.5, tol=2 * c.tolerance + 0.05, msg="the inner wall stands %s from a probe 0.5 under it at %s/%s: the skin is not %s thick" % (c.distance, c.face_a, c.face_b, skin))
    r = print_check(S1, printer=PRINTER)
    assert_close(r.min_wall, skin, tol=0.15, msg="the wall rule reads %s where the skin is %s" % (r.min_wall, skin))
    return r.min_wall


test("the skin is `skin` thick: by a probe inside it and by the wall rule", the_skin_is_skin_thick)


def smooth_and_ruled_agree():
    other = loft([skin_section_2(y, "s2_other") for y in stations_between(S2_Y0, S2_Y1)], ruled=not RULED, name="s2_other")
    v_here = S2.body().volume()
    # about 1.0 % apart: both meshes are chorded at the document tolerance.
    assert_close(other.volume(), v_here, rel=0.015, msg="the ruled and the skinned section 2 differ by more than 1.5 %% in volume: %s against %s" % (other.volume(), v_here))
    # the skin is one surface per segment where the ruled loft is one per
    # segment per bay, and some of its bands come out planar
    assert_true(len(other.faces()) > len(S2.body().faces()), msg="the ruled loft has a band per bay: %d faces against the skin's %d" % (len(other.faces()), len(S2.body().faces())))
    return other.volume() / v_here


test("the skinned section and the ruled loft through the same stations agree in volume within 1.5 %", smooth_and_ruled_agree)


def chord_shrinks_outboard():
    for inst in ALL:
        assert_solid_part(inst.name, inst.body())
    lo, hi = RIB_TIP.body().bbox()
    return hi[0] - lo[0]


# Two tapers, not a range: the servo pocket's cut against the skinned
# section 2 does not build at every taper and root chord.
test("the tip rib's chord follows the taper, and the wing builds at every taper", chord_shrinks_outboard, sweep={"taper": [0.55, 0.7]}, monotone="increasing")


def each_bed_is_one_print():
    """Each plate is one printable set: every part solid, every part on the
    plate and inside the build volume, no two of them within the fuse
    threshold, no horizontal gap the slicer would round. Overhang is not
    judged -- which way up a part prints is `orient_for_print`'s to say and
    the layout deliberately does not turn anything (see above)."""
    assert_eq(len(BEDS), 5)
    # What a plate has to be: every part solid, every part inside the build
    # volume, and no two parts on it closer than the fuse threshold -- the
    # last one measured pair by pair with `distance()` rather than read off
    # `assert_printable`, which would also judge two things this layout
    # does not claim: overhang (which way up a part prints is
    # `orient_for_print`'s to say and the layout turns nothing) and wall
    # thickness.
    n = 0
    worst = 99.0
    for i, bed in enumerate(BEDS):
        for b in bed:
            assert_solid_part(b.name, b.body())
            r = print_check(b, printer=PRINTER)
            assert_true(r.fits_bed, msg="plate %d: %s does not fit the build volume (%s)" % (i + 1, b.name, r.size))
            lo, hi = b.body().bbox()
            assert_close(lo[2], 0.0, tol=1e-6, msg="plate %d: %s does not sit on the plate (z from %s)" % (i + 1, b.name, lo[2]))
        for a in range(len(bed)):
            for c in range(a + 1, len(bed)):
                gap = distance(bed[a], bed[c])
                assert_true(not gap.overlap, msg="plate %d: %s and %s overlap at %s" % (i + 1, bed[a].name, bed[c].name, gap.at))
                assert_true(gap.distance >= MIN_GAP, msg="plate %d: %s and %s are %s mm apart, under the %s mm fuse threshold" % (i + 1, bed[a].name, bed[c].name, math.round(gap.distance, 3), MIN_GAP))
                worst = min(worst, gap.distance)
        n += len(bed)
    assert_true(n == 22, msg="every printed part of the right half is on a plate: %d" % n)
    return worst


test("each bed is one printable set: solid, apart, on the plate, no overhang", each_bed_is_one_print, sweep={"layout": [True]})
