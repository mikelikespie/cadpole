# A turbocharger compressor wheel: an open radial impeller whose blades start
# axial at the inducer, like a propeller, and turn radial with backsweep at the
# exducer, a splitter blade in every passage, machined from billet.
#
# What this example demonstrates:
#
#   * a blade that is a genuinely three-dimensional surface, built as a
#     *ruled loft between two curves in space* -- the blade's outline where
#     it meets the hub and its outline where it meets the shroud, each a
#     closed `curve()` drawn from the same station list, so station i at the
#     hub pairs with station i at the tip and every ruling is a radial fibre
#     of the analytic camber surface. This is the flank-milled ruled blade
#     turbomachinery design tools produce. The surface is exact -- one ruled
#     patch per flank segment, no approximation -- and STEP-exportable;
#   * both ends of that loft closed by `cap="ruled"`: the blade tip, which
#     lies on the shroud stream surface and in no plane, is the loop ruled
#     against itself -- physically the ribbon between the two flanks -- and
#     the root, carried inside the hub, is closed the same way and swallowed
#     by the union;
#   * the hub as one loft through circles: rim band, bell, nose boss, no
#     boolean. It stays a *ruled* loft through three dozen sections rather
#     than a `ruled=False` skin, because its profile has a chamfer, and a
#     global interpolant is not shape-preserving -- a chamfer in the sections
#     rounds away. The nose's rim is then rounded on the exact hub
#     by `fillet`, a torus re-wired into the loft's last band and
#     the top disc, before the union takes the hub to the mesh kernel;
#   * one `pattern_circular(merge=True)` fusing a full blade and its splitter
#     six times over, and no trimming anywhere: the tip section *is* the
#     blade tip;
#   * a root flare baked into the loft's sections (`root_fillet`): a radius
#     law along the root generated as solid geometry, so within
#     `root_fillet` of the hub the sections widen by a rolling-ball law. A
#     flare in the sections, exact and STEP-exportable, not a blend;
#   * an `assert_solid` on the result, and a face-count assertion that
#     catches a union that loses face provenance (one face per triangle).
#
# Why not horizontal slices of the camber surface: a loft through
# slices rules between corresponding points of consecutive slices, not along
# the hub-to-tip fibres the blade is designed on, so it approximates the wrong
# surface and needs sampling knobs and an inlet-angle limit that are not
# properties of the part. Lofting the stations directly needs only `stations`.
#
# Thickness convention: `blade_thickness` is measured *normal to the camber
# surface*, in space. The other convention in use, thickness measured in the
# tangential direction on the layer surface, is larger by the secant of the
# local lean angle; a blade drawn to the wrong one is systematically the
# wrong thickness wherever it leans. The leading edge is elliptical, 3:1 (a
# circular nose is the worst case aerodynamically), as two cubic Béziers --
# an ellipse is not a polynomial curve, but the standard 0.5523 handle
# length reproduces a quarter of one to a few hundredths of a percent -- and
# the trailing edge is square.
#
# What a real wheel has that this does not: a true rolling-ball fillet
# where the blades meet the hub. The flare is a radius law in the sections;
# it is not the normal section of a ball rolled along the root curve, and
# the difference grows where the root runs radially (the exducer).
#
# The blade angle is measured from the meridional (streamwise) direction, so
# at the inducer it is the angle from the axis and at the exducer the angle
# from the radius. With `beta1 = 55` the inducer tip leans 55 degrees off the
# axis -- the propeller look -- and with `beta2 = 35` the exit is swept back
# 35 degrees off radial. The wrap that produces is about 60 degrees. Blades
# are "radial-fibred": hub and shroud share the wrap at every station, so a
# ruling from hub to tip lies in a meridional plane and the hub, on its
# shorter path, gets the steeper angle -- plus a lean at the inducer that
# tips the rulings over in the direction of rotation, which gives the tips
# their scimitar sweep.
#
#   cadpole render examples/compressor_wheel.star --backend path --samples 256 \
#       --size 1600x1200 --view az=30,el=28 --fov 24 --lighting product \
#       --backdrop studio --floor gloss --tonemap neutral --edge-break 0.25 \
#       --denoise --exposure 0.85 -o /tmp/wheel.png   # docs/demo/compressor_wheel.png
#   cadpole export examples/compressor_wheel.star --format step -o /tmp/wheel.step

blades = param("blades", 7, min=3, max=12, doc="Full blades; a splitter follows each one")
splitters = param("splitters", True, doc="A half-length splitter blade in every passage")
r_tip = param("tip_radius", 44.0, unit="mm", min=15, max=150, doc="Exducer radius: the wheel's overall size")
r_ind = param("inducer_radius", 30.0, unit="mm", min=8, max=120, doc="Blade tip radius at the inlet")
r_nose = param("nose_radius", 10.0, unit="mm", min=4, max=40, doc="Hub radius at the inlet, where the blade roots begin")
z_nose = param("nose_height", 42.0, unit="mm", min=42, max=60, doc="Height of the hub nose above the back face; its boss stands 2 mm higher and must clear the blade tips")
z_ind = param("inducer_height", 43.0, unit="mm", min=30, max=43, doc="Height of the blade tips at the inlet, below the nose boss")
b2 = param("exit_height", 6.5, unit="mm", min=1.5, max=30, doc="Blade height at the exducer")
back_t = param("backplate", 5.0, unit="mm", min=2, max=20)
beta1 = param("inlet_angle", 55.0, unit="deg", min=20, max=75, doc="Blade angle off the axis at the inducer tip")
beta2 = param("exit_angle", 35.0, unit="deg", min=0, max=60, doc="Backsweep off radial at the exducer")
lean = param("lean", 14.0, unit="deg", min=0, max=30, doc="How far the inducer tip leads the root, in the direction of rotation")
blade_t = param("blade_thickness", 1.8, unit="mm", min=0.6, max=5,
                doc="At the root, measured normal to the camber surface (not in the tangential direction, which is larger by the secant of the lean); the tip is a little over half of it")
root_fillet = param("root_fillet", 1.2, unit="mm", min=0.5, max=1.2,
                    doc="Root flare radius: within this height of the hub the blade sections widen by a rolling-ball law. A flare baked into the loft's sections, not a blend")
chamfer = param("chamfer", 1.2, unit="mm", min=0, max=5, doc="On the rim's back edge")
nose_r = param("nose_fillet", 1.0, unit="mm", min=0.2, max=3, doc="Round on the nose boss's rim, exact")
bore = param("bore", 10.0, unit="mm", min=3, max=30, doc="Shaft bore diameter")
cbore = param("counterbore", 16.0, unit="mm", min=0, max=50, doc="Nut seat diameter in the nose; 0 for none")
cbore_depth = param("counterbore_depth", 3.0, unit="mm", min=0, max=20)
split_at = param("splitter_start", 0.42, min=0.1, max=0.9, doc="Where along the passage the splitter's leading edge sits")
stations = param("stations", 48, min=12, max=120, doc="Samples along each streamline: one spline point per station on each flank")

assert_true(math.close(math.atan(1.0), 45.0), msg="math.atan answers in degrees")
assert_true(r_ind < r_tip - 5.0, msg="the inducer sits well inside the exducer")
# Checked here, before any blade is built: a nose far below the blade tips
# twists the blade into a self-crossing surface that takes minutes to mesh
# and then fails, and the finished wheel would fail the same check below.
assert_true(z_ind < z_nose + 2.0, msg="the blade tips stand above the nose boss: raise nose_height or lower inducer_height")
assert_true(r_nose > bore / 2.0 + 2.0, msg="the nose needs a wall around the bore")
assert_true(cbore / 2.0 < r_nose - 1.0 or cbore <= 0, msg="the counterbore must fit inside the nose")


def polar(r, a):
    return (r * math.cos(a), r * math.sin(a))


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


# ---------------------------------------------------------- meridional curves
#
# Hub and shroud streamlines are quarter-ellipses in the (r, z) half-plane,
# radial at the exducer and axial at the inlet, entered a few degrees in from
# the radial end so `r` is a proper function of `z` for the hub's loft of
# circles. Returned from the radial end to the axial end, i.e. against the
# flow.
PHI0 = 6.0


def quarter(r_start, z_start, r_end, z_end, n):
    s0 = math.sin(PHI0)
    c0 = math.cos(PHI0)
    a = (r_start - r_end) / (1.0 - s0)
    b = (z_end - z_start) / c0
    rc = r_end + a
    zc = z_end
    pts = []
    for k in range(n + 1):
        phi = PHI0 + (90.0 - PHI0) * k / n
        pts.append((rc - a * math.sin(phi), zc - b * math.cos(phi)))
    return pts


hub_curve = quarter(r_tip, 0.0, r_nose, z_nose, stations - 1)
# The blade's own streamlines stop inside the rim by a hair plus the trailing
# edge's half-thickness, so the trailing edge sits just inside the hub's rim
# and never coincides with it: two bodies sharing a face is the tangent case
# the kernels refuse.
RIM = 0.3 + blade_t / 2.0
blade_hub = quarter(r_tip - RIM, 0.0, r_nose, z_nose, stations - 1)
blade_shroud = quarter(r_tip - RIM, b2, r_ind, z_ind, stations - 1)
# Station 0 is the inlet, station `stations - 1` the exducer: with the flow.
H = [blade_hub[i] for i in range(stations - 1, -1, -1)]
S = [blade_shroud[i] for i in range(stations - 1, -1, -1)]

# Meridional arc length along the shroud, and the wrap integrated from the
# blade-angle schedule: tan(beta) = r dtheta / ds.
s_acc = [0.0]
for i in range(1, stations):
    s_acc.append(s_acc[-1] + math.hypot(S[i][0] - S[i - 1][0], S[i][1] - S[i - 1][1]))
s_total = s_acc[-1]
theta = [0.0]
for i in range(1, stations):
    m_mid = (s_acc[i - 1] + s_acc[i]) / 2.0 / s_total
    beta = beta1 + (beta2 - beta1) * m_mid
    r_mid = (S[i][0] + S[i - 1][0]) / 2.0
    ds = s_acc[i] - s_acc[i - 1]
    theta.append(theta[-1] + math.degrees(math.tan(beta) * ds / r_mid))
wrap = theta[-1]
assert_true(wrap > 20.0 and wrap < 160.0, msg="the wrap came out at %s degrees; check the angles" % int(wrap))

# Per station: hub (r, z, angle) and shroud (r, z, angle). The lean tips the
# ruling over at the inducer and vanishes by the exducer.
ST = []
for i in range(stations):
    m = s_acc[i] / s_total
    ST.append((H[i][0], H[i][1], theta[i], S[i][0], S[i][1], theta[i] + lean * (1.0 - m) * (1.0 - m)))


def station(i):
    """The station tuple at a fractional index."""
    i0 = int(i)
    i0 = max(0, min(stations - 2, i0))
    f = i - i0
    a = ST[i0]
    b = ST[i0 + 1]
    return tuple([a[k] + (b[k] - a[k]) * f for k in range(6)])


def surface(i, sig):
    """A point on the blade's camber surface: station `i`, fraction `sig` of
    the way from hub to shroud along the ruling (past either end when outside
    0..1)."""
    rh, zh, th, rs, zs, ts = station(i)
    r = rh + sig * (rs - rh)
    z = zh + sig * (zs - zh)
    t = th + sig * (ts - th)
    return (r * math.cos(t), r * math.sin(t), z)


def span_of(i):
    """Length of the ruling from hub to shroud at station `i`, mm."""
    return vlen(vsub(surface(i, 1.0), surface(i, 0.0)))


def normal(i, sig):
    """Unit normal of the camber surface at (i, sig), from the two parametric
    tangents: streamwise, and along the ruling. This is the direction the
    blade's thickness is measured in."""
    p = surface(i, sig)
    di = 0.25 if i + 0.25 <= stations - 1 else -0.25
    n = cross(vsub(surface(i + di, sig), p), vsub(surface(i, sig + 0.05), p))
    return vscale(vunit(n), 1.0 if di > 0 else -1.0)


# ------------------------------------------------------------ blade sections
#
# One closed curve per section, in world coordinates, drawn from the middle
# of the trailing edge: half the trailing edge, the pressure flank as a
# spline back to the inlet, the elliptical nose as two quarter-ellipse
# Béziers meeting at the apex, the suction flank as a spline to the exit, and
# the other half of the trailing edge. Every section is built from the same
# station list, so the loft's construction-order pairing sends station i to
# station i and each flank segment to its counterpart -- which is the radial
# fibre, the physical ruling. That is why the loft below takes the default
# match="index" and NOT match="arclen": arc length shared out over the whole
# loop would drift the nose against the flank and skew every ruling.
#
# The seam is the trailing-edge midpoint and the half-way point is at the
# nose, so `cap="ruled"` splits the loop into its two flanks: the tip cap is
# the ribbon between them, the corner at the seam is the straight trailing
# edge, and the corner at the nose is the flat face between the two
# quarter-ellipses, which share a plane.
#
# The blade is carried past the hub (sig down to -0.15 at every station),
# which the union swallows, and ends exactly on the shroud line (sig = 1).
SIG_LO = -0.15
SIG_HI = 1.0
KAPPA = 0.5522847498  # cubic Bézier handle for a quarter ellipse


def thickness(sig):
    """Half the blade thickness at span fraction `sig`: thinner towards the
    tip, as a machined blade is."""
    return blade_t / 2.0 * (1.0 - 0.45 * max(0.0, min(1.0, sig)))


def flare(h):
    """The root flare's widening at height `h` above the hub, by the
    rolling-ball law: the full radius on and below the hub surface, nothing
    at `root_fillet` and above."""
    if root_fillet <= 0.0 or h >= root_fillet:
        return 0.0
    if h <= 0.0:
        return root_fillet
    return root_fillet - math.sqrt(root_fillet * root_fillet - (root_fillet - h) * (root_fillet - h))


def half_width(i, sig):
    """Half the blade's width at (i, sig): the thickness, plus the root flare
    where the section is within `root_fillet` of the hub."""
    return thickness(sig) + flare(sig * span_of(i))


def le_stations(i_first):
    """How many stations the leading-edge ellipse spans, measured on the root
    section, whose ellipse is the longest (the root is the thickest, and its
    streamline the shortest). The same count on every section keeps the
    segment counts equal, which is what the loft pairs by."""
    a = 3.0 * half_width(i_first, SIG_LO)
    run = 0.0
    k = 0
    for i in range(i_first, stations - 2):
        run += vlen(vsub(surface(i + 1, SIG_LO), surface(i, SIG_LO)))
        k = i + 1 - i_first
        if run >= a:
            break
    return max(1, k)


def along(i_first, sig, target):
    """The fractional station, from `i_first`, at which the camber surface's
    streamwise arc length from station `i_first` reaches `target`."""
    run = 0.0
    for i in range(i_first, stations - 1):
        step = vlen(vsub(surface(i + 1, sig), surface(i, sig)))
        if run + step >= target:
            return i + (target - run) / step
        run += step
    return float(stations - 1)


def span_section(sig_at, i_first, k_le, name):
    """The blade's closed outline in world coordinates on the section whose
    span fraction at station `i` is `sig_at(i)`, starting at station
    `i_first` (the splitter is the same curves truncated), with the leading
    edge's ellipse spanning `k_le` stations."""
    # The nose: a 3:1 ellipse whose apex is the camber surface's own leading
    # edge at station `i_first`, drawn *back* from it -- so the blade ends
    # where the design says it ends (the tip apex at the inducer height, the
    # root apex inside the hub nose). Its semi-axes are the section's
    # half-width across and three times that along the camber, and the
    # flanks leave it at the ends of its minor axis, a fractional station
    # downstream, then run through the integer stations from `i_first +
    # k_le` on. Every section uses the same stations, so station i pairs with
    # station i across the loft.
    sig0 = sig_at(i_first)
    apex = surface(i_first, sig0)
    b = half_width(i_first, sig0)
    a = 3.0 * b
    i_le = along(i_first, sig0, a)
    centre = surface(i_le, sig0)
    n0 = normal(i_le, sig0)
    fwd = vunit(vsub(apex, centre))
    fwd = vunit(vsub(fwd, vscale(n0, fwd[0] * n0[0] + fwd[1] * n0[1] + fwd[2] * n0[2])))
    apex = vadd(centre, vscale(fwd, a))
    press = [vadd(centre, vscale(n0, b))]
    suct = [vadd(centre, vscale(n0, -b))]
    for i in range(i_first + k_le, stations):
        sig = sig_at(i)
        c = surface(i, sig)
        n = normal(i, sig)
        h = half_width(i, sig)
        press.append(vadd(c, vscale(n, h)))
        suct.append(vadd(c, vscale(n, -h)))
    last = len(press) - 1
    te = vscale(vadd(press[last], suct[last]), 0.5)
    p1 = vadd(press[0], vscale(fwd, KAPPA * a))
    p2 = vadd(apex, vscale(n0, KAPPA * b))
    q1 = vadd(apex, vscale(n0, -KAPPA * b))
    q2 = vadd(suct[0], vscale(fwd, KAPPA * a))
    return (curve(te, name=name)
            .line(press[last])
            .spline([press[i] for i in range(last - 1, -1, -1)])
            .bezier(apex, controls=[p1, p2])
            .bezier(suct[0], controls=[q1, q2])
            .spline([suct[i] for i in range(1, last + 1)])
            .line(te)
            .close())


def blade_body(i_first, name):
    """One blade: a ruled loft from its root outline, inside the hub, up its
    flare to its tip outline on the shroud, both ends ruled shut."""
    k_le = le_stations(i_first)
    sections = [span_section(lambda i: SIG_LO, i_first, k_le, name + "_root")]
    if root_fillet > 0.0:
        # The flare: sections at fixed heights above the hub surface, where
        # the outline is widened by the rolling-ball law, and its top exactly
        # on the fibre surface (no widening), so the flank above it is the
        # exact ruled surface. Three chords of the quarter circle, which is
        # within 0.04 mm of it at the default radius.
        for k in range(1, 4):
            h = root_fillet * k / 3.0
            sections.append(span_section(lambda i, h=h: h / span_of(i), i_first, k_le, name + "_flare%s" % k))
    sections.append(span_section(lambda i: SIG_HI, i_first, k_le, name + "_tip"))
    return loft(sections, ruled=True, cap="ruled", name=name)


blade = blade_body(0, "blade")
pair = blade
if splitters:
    i_split = int(split_at * (stations - 1))
    splitter = blade_body(i_split, "splitter").rotate(180.0 / blades, axis=(0, 0, 1), name="splitter_placed")
    pair = union([blade, splitter], name="blade_pair")
ring = pattern_circular(pair, blades, angle=360.0, merge=True, name="blades")

# The hub loft takes three dozen bands: each band is an exact cone shaded
# with its own normal, so with a dozen the bell reads as terraces; with three
# dozen the kinks are two degrees and the seam smoothing hides them. It stays
# ruled -- its profile has a chamfer, and a `ruled=False` skin would round
# it away.
BANDS = 36
coarse = [k * (stations - 1) // BANDS for k in range(1, BANDS + 1)]

# --------------------------------------------------------------- the hub
#
# One loft: a chamfer and the rim band from the back face up to the exducer,
# the bell along the hub streamline, and the nose boss up to its flat top.
# The blade roots start 15% of the passage height inside it, so the union is
# a crossing, never a touch.
hub_profile = [(-back_t, r_tip - chamfer), (-back_t + chamfer, r_tip), (0.0, r_tip)]
hub_profile += [(hub_curve[k][1], hub_curve[k][0]) for k in coarse]
hub_profile += [(z_nose + 2.0, r_nose)]
for i in range(1, len(hub_profile)):
    assert_true(hub_profile[i][0] > hub_profile[i - 1][0] + 1e-6, msg="hub sections must climb")
hub = loft([sketch(XY.offset(z)).circle(r) for (z, r) in hub_profile], ruled=True, name="hub")

# The nose's rim, rounded exactly: the rim circle is a closed loop on
# the top disc, so the blend is a torus re-wired into the disc and the
# loft's last band with no boolean. It runs on the exact hub, before the
# union with the blades takes the hub to the mesh kernel. The contact circle
# on the disc sits at `r_nose - nose_r`; the counterbore's mouth must stay
# inside it, or the fillet would reach the seat.
assert_true(cbore <= 0 or cbore / 2.0 < r_nose - nose_r - 0.5, msg="the counterbore must stay inside the nose fillet's contact circle")
hub = fillet(hub, nose_r, edges=hub.edges(on_plane=XY.offset(z_nose + 2.0)), name="hub_nose")

wheel = union([hub, ring], name="wheel_solid")
cuts = [extrude(sketch(XY.offset(-back_t - 1.0)).circle(bore / 2.0), back_t + z_nose + 8.0, name="bore_cut")]
if cbore > 0 and cbore_depth > 0:
    cuts.append(extrude(sketch(XY.offset(z_nose + 2.0 - cbore_depth)).circle(cbore / 2.0), cbore_depth + 1.0, name="counterbore_cut"))
wheel = subtract(wheel, union(cuts, name="cuts") if len(cuts) > 1 else cuts[0], name="wheel")

# ------------------------------------------------------------- material
#
# Polished billet aluminium: bright (for a metal, `color` is reflectance),
# with the highlight stretched along the tool path -- `anisotropy` was made
# for FDM layer lines and does the same for tool marks. `texture="machined"`
# is the tool marks themselves: fine turning grooves about the axis on the
# hub, the bore, the chamfers and the back face, and flank-milling scallops
# along the rulings on the blades, 0.15 mm apart, filtered by the pixel so
# they hand over to the anisotropic sheen when the camera is too far to
# resolve them (docs/starlark-api.md, "Materials").
wheel.material(color="#f2f3f5", roughness=0.16, metallic=1.0, anisotropy=0.5, texture="machined")

assert_solid(wheel)
lo, hi = wheel.bbox()
assert_true(hi[0] <= r_tip + 0.01 and hi[0] > r_tip - 0.01, msg="the rim is the widest point, and nothing stands proud of it: %s" % hi[0])
assert_true(math.close(lo[2], -back_t, tol=0.02), msg="the back face is the lowest")
assert_true(math.close(hi[2], z_nose + 2.0, tol=0.02), msg="the nose boss is the highest point")
# Fourteen blades of four bands (one without the flare), a hundred faces a
# band, plus two ribbons each: some six thousand faces with the flare and
# under three thousand without. One face per triangle would be a hundred
# thousand.
assert_true(len(wheel.faces()) < 10000,
            msg="the wheel has a few thousand faces, not one per triangle: %s" % len(wheel.faces()))


# The ends of the declared nose and inducer heights build, each in seconds:
# a nose far below the blade tips used to be a declared value that spent
# minutes meshing a self-crossing blade before it failed.
def the_wheel_is_solid():
    assert_solid(wheel)


test("the tallest nose", the_wheel_is_solid, sweep={"nose_height": [60]})
test("the lowest inducer", the_wheel_is_solid, sweep={"inducer_height": [30]})
