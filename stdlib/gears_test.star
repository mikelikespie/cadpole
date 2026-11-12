# The tests of @cadpole/gears.star. A document, not a library: it loads the
# library, declares the parameters its sweeps need, and builds its gears
# inside `test()` functions, so `cadpole test stdlib/gears_test.star` runs
# them and `//src/rust/cadpole-script:cadpole_script_test` (`every_stdlib_test_passes`)
# runs them in CI. The refusals -- a fillet that does not fit, a planetary
# that cannot be assembled, a hand that is not one -- are Rust tests in
# `src/rust/cadpole-script/src/stdlib.rs`, because a Starlark script cannot
# catch `fail()`.

load(
    "@cadpole/gears.star",
    "auto_shift",
    "base_diameter",
    "bevel",
    "centre_distance",
    "helical",
    "fluted_outline",
    "herringbone",
    "internal",
    "involute_profile",
    "layout",
    "mesh_turn",
    "min_teeth",
    "pitch_diameter",
    "planetary",
    "planetary_set",
    "rack",
    "root_diameter",
    "spur",
    "tip_diameter",
    "tooth_faces",
    "tooth_thickness",
)

M = 2.5
ALPHA = 20.0
teeth = param("teeth", 17, min=6, max=120, doc="Tooth count the sweeps vary")
helix = param("helix", 20, unit="deg", min=5, max=45, doc="Helix angle the sweeps vary")
face = param("face", 14.0, unit="mm", min=8, max=20, doc="Face width the herringbone sweep varies")

# ----------------------------------------------------------------- helpers


def outline(body, centre):
    """The outline of an extruded gear, read back from its vertical edges at
    the bottom face: `(angle, radius, x, y)` about `centre`, sorted by angle,
    which is polygon order because a gear outline is star-shaped."""
    seen = {}
    for e in body.faces(name="side[0.*]").edges():
        if abs(e.start[2] - e.end[2]) < 1e-6:
            continue
        p = e.start if e.start[2] < e.end[2] else e.end
        seen[(math.round(p[0], 6), math.round(p[1], 6))] = p
    out = []
    for x, y in seen.keys():
        dx, dy = x - centre[0], y - centre[1]
        out.append((math.atan2(dy, dx), math.hypot(dx, dy), x, y))
    return sorted(out)


def near(a, centre_angle, span):
    """The vertices of an outline within `span` degrees of `centre_angle`, in
    order round the outline (so a zone across the +-180 seam stays one chain)."""
    out = []
    for v in a:
        d = (v[0] - centre_angle + 180.0) % 360.0 - 180.0
        if abs(d) <= span:
            out.append((d, v))
    return [v for _, v in sorted(out)]


def point_segment(p, a, b):
    dx, dy = b[0] - a[0], b[1] - a[1]
    l2 = dx * dx + dy * dy
    t = 0.0 if l2 == 0 else math.clamp(((p[0] - a[0]) * dx + (p[1] - a[1]) * dy) / l2, 0.0, 1.0)
    return math.hypot(p[0] - (a[0] + t * dx), p[1] - (a[1] + t * dy))


def min_gap(a, b):
    """The smallest distance from a vertex of polyline `a` to a segment of
    polyline `b` and the other way round; both are (angle, r, x, y) lists in
    order, treated as open chains (the zone around a mesh)."""
    best = 1e300
    for p, q in ((a, b), (b, a)):
        for v in p:
            for i in range(len(q) - 1):
                d = point_segment((v[2], v[3]), (q[i][2], q[i][3]), (q[i + 1][2], q[i + 1][3]))
                if d < best:
                    best = d
    return best


def radius_at(zone, angle):
    """The outline's radius at `angle` (degrees, about the outline's centre),
    interpolated on the chord between the two zone vertices that bracket it;
    None when the angle is outside the zone. A gear outline is star-shaped
    about its centre, so a point is inside it exactly when its radius is
    under this."""
    for i in range(len(zone) - 1):
        a0, a1 = zone[i][0], zone[i + 1][0]
        d0 = (angle - a0 + 180.0) % 360.0 - 180.0
        d1 = (a1 - angle + 180.0) % 360.0 - 180.0
        span = (a1 - a0 + 180.0) % 360.0 - 180.0
        if span > 0 and d0 >= 0 and d1 >= 0:
            f = d0 / span
            return zone[i][1] + f * (zone[i + 1][1] - zone[i][1])
    return None


def flank_angle_at(verts, r):
    """Linear interpolation of the outline's angle at radius `r` along a
    flank given as vertices sorted by radius."""
    for i in range(len(verts) - 1):
        r0, r1 = verts[i][1], verts[i + 1][1]
        if r0 <= r and r <= r1:
            f = (r - r0) / (r1 - r0)
            return verts[i][0] + f * (verts[i + 1][0] - verts[i][0])
    fail("no flank vertex pair brackets radius %s" % r)


def left_flank(a, lay):
    """Tooth 0's counter-clockwise side, root to tip: the vertices strictly
    between the root and tip circles on the +y side, sorted by radius."""
    z = lay["teeth"]
    return sorted(
        [(v[0], v[1]) for v in a if v[0] > 0 and v[0] < 180.0 / z and v[1] > lay["r_root"] + 1e-6 and v[1] < lay["r_tip"] - 1e-6],
        key=lambda v: v[1],
    )


# ----------------------------------------------------------------- closed forms


def closed_forms():
    assert_close(tooth_thickness(M), math.pi * M / 2)
    assert_close(tooth_thickness(M, backlash=0.1), math.pi * M / 2 - 0.1)
    assert_close(tooth_thickness(M, profile_shift=0.3), M * (math.pi / 2 + 2 * 0.3 * math.tan(ALPHA)))
    assert_close(tip_diameter(M, 17), M * (17 + 2))
    assert_close(tip_diameter(M, 17, profile_shift=0.3), M * (17 + 2 + 0.6))
    assert_close(root_diameter(M, 17), M * (17 - 2.5))
    assert_close(root_diameter(M, 17, profile_shift=0.3, clearance=0.2), M * (17 - 2.4 + 0.6))
    assert_close(pitch_diameter(M, 17), M * 17)
    assert_close(pitch_diameter(M, 17, helix_angle=30), M * 17 / math.cos(30))
    assert_close(base_diameter(M, 17), M * 17 * math.cos(ALPHA))
    assert_close(centre_distance(M, 17, 29), M * (17 + 29) / 2)
    assert_close(centre_distance(M, 17, 29, shift1=0.2, shift2=-0.2), M * 23)
    assert_close(centre_distance(M, 12, -36), M * 12)
    # Shifted: the working pressure angle solves inv(a_w) = inv(a) + 2 (x1 + x2) tan(a) / (z1 + z2).
    a = centre_distance(M, 17, 29, shift1=0.3, shift2=0.2)
    a0 = M * 23
    cos_w = a0 * math.cos(ALPHA) / a
    a_w = math.acos(cos_w)
    inv = lambda d: math.tan(d) - math.radians(d)
    assert_close(inv(a_w), inv(ALPHA) + 2 * 0.5 * math.tan(ALPHA) / 46, tol=1e-9)
    assert_true(a > a0, msg="positive shifts push the gears apart")
    # The undercut limit: 2 / sin^2(20) = 17.10 for a sharp cutter with no
    # clearance, and the ISO rack lands on the same number by coincidence
    # (0.38 (1 - sin 20) = 0.25 = the clearance it adds).
    assert_close(min_teeth(clearance=0, fillet=0), 2 / (math.sin(ALPHA) * math.sin(ALPHA)), tol=1e-9)
    assert_close(min_teeth(), 17.1, tol=0.01)
    assert_close(min_teeth(pressure_angle=25), 2 * (1.25 - 0.38 * (1 - math.sin(25))) / (math.sin(25) * math.sin(25)), tol=1e-9)
    assert_true(auto_shift(9) > 0 and auto_shift(41) == 0, msg="auto_shift lifts a small pinion and leaves a wheel alone")
    assert_close(min_teeth(profile_shift=auto_shift(9)), 9, tol=1e-9)
    assert_eq(mesh_turn(29), 0.0)
    assert_close(mesh_turn(30), 6.0)


test("the closed forms: thickness, diameters, centre distance, undercut limit", closed_forms)


def planetary_arithmetic():
    p = planetary(9, 6)
    assert_eq(p["ring"], 21)
    assert_eq((p["sun"] + p["ring"]) % p["planets"], 0)
    assert_close(p["ratio_fixed_ring"], 1 + 21 / 9)
    assert_close(p["ratio_fixed_carrier"], -21 / 9)
    assert_close(p["ratio_fixed_sun"], 1 + 9 / 21)
    assert_close(p["planet_angles"], [0.0, 120.0, 240.0])
    q = planetary(12, 12, planets=3, module=1.5)
    assert_eq(q["ring"], 36)
    assert_close(q["centre_distance"], 1.5 * 12)
    assert_close(q["ring_turn"], 180.0 / 36)  # an even planet shows the ring a space, so the ring shows a tooth
    assert_eq(len(q["planet_turns"]), 3)
    for k in range(3):
        # Every planet shows the sun a space: its phase toward the sun is a half pitch.
        phase = ((q["planet_angles"][k] + 180.0 - q["planet_turns"][k]) * 12 / 360.0) % 1.0
        assert_close(phase, 0.5, tol=1e-9)
    # A herringbone set runs on the transverse module: the same centre distance
    # `centre_distance(helix_angle=)` gives, m (z_s + z_p) / (2 cos beta), which
    # the planetary example's planets sit on; the phases do not depend on it.
    h = planetary(12, 12, planets=3, module=1.5, helix_angle=30)
    assert_close(h["centre_distance"], centre_distance(1.5, 12, 12, helix_angle=30))
    assert_close(h["centre_distance"], 1.5 * 12 / math.cos(30))
    assert_close(h["planet_turns"], q["planet_turns"])
    assert_close(h["ring_turn"], q["ring_turn"])
    assert_eq(h["helix_angle"], 30)


test("planetary(9, 6) is a 21-tooth ring; 12/12/36 assembles with three planets; a helix angle sets the centre distance", planetary_arithmetic)

# ----------------------------------------------------------------- the profile


def measured_thickness(z, backlash, shift):
    g = spur(M, z, 4, backlash=backlash, profile_shift=shift, tolerance=0.003, name="t_%s_%s_%s" % (z, backlash, shift))
    a = outline(g, (0, 0))
    lay = layout(M, z, backlash=backlash, profile_shift=shift, tolerance=0.003)
    rp = lay["r_pitch"]
    flank = [v for v in left_flank(a, lay) if v[1] >= lay["r_base"] - 1e-6]
    theta = flank_angle_at(flank, rp)
    return 2 * rp * math.radians(theta)


def thickness_on_the_pitch_circle():
    # Read back from the outline: pi m / 2, minus the backlash, plus the shift's 2 x m tan(alpha).
    assert_close(measured_thickness(29, 0.0, 0.0), math.pi * M / 2, tol=0.01)
    assert_close(measured_thickness(29, 0.2, 0.0), math.pi * M / 2 - 0.2, tol=0.01)
    assert_close(measured_thickness(29, 0.0, 0.3), tooth_thickness(M, profile_shift=0.3), tol=0.01)
    assert_close(measured_thickness(17, 0.1, 0.0), tooth_thickness(M, backlash=0.1), tol=0.01)


test("the outline's tooth thickness on the pitch circle is pi m / 2 - backlash (+ 2 x m tan alpha)", thickness_on_the_pitch_circle)


def tip_and_root_radii():
    for z in (9, 17, 41):
        lay = layout(M, z)
        g = spur(M, z, 4, name="r_%d" % z)
        a = outline(g, (0, 0))
        r_max = max([v[1] for v in a])
        r_min = min([v[1] for v in a])
        assert_close(r_max, tip_diameter(M, z) / 2, tol=1e-6, msg="tip radius, %d teeth" % z)
        assert_close(r_min, root_diameter(M, z) / 2, tol=1e-6, msg="root radius, %d teeth" % z)
        assert_close(lay["r_tip"], r_max, tol=1e-5)
        assert_close(lay["r_root"], r_min, tol=1e-5)
        assert_eq(len(g.faces()), lay["segments"] * z + 2, msg="one face per outline segment plus the caps, %d teeth" % z)
        lo, hi = g.bbox()
        assert_true(hi[0] <= lay["r_tip"] + 1e-6 and hi[0] >= lay["r_tip"] * math.cos(4), msg="tooth 0 reaches the tip on +x")
        assert_true(math.pi * r_min * r_min * 4 < g.volume() and g.volume() < math.pi * r_max * r_max * 4)


test("tip and root radii are m (z + 2) / 2 and m (z - 2.5) / 2, and the faces are the segments", tip_and_root_radii)


def undercut_is_generated():
    # Below the limit the trochoid crosses the involute: walking a flank from
    # the root outward, the angle dips under the involute's start angle and
    # comes back -- the neck. Above the limit it falls monotonically.
    lay9 = layout(M, 9, tolerance=0.003)
    assert_true(lay9["undercut"], msg="9 teeth are undercut at 20 degrees")
    assert_true(not layout(M, 41)["undercut"])
    g = spur(M, 9, 4, tolerance=0.003, name="u9")
    assert_solid(g)
    flank = left_flank(outline(g, (0, 0)), lay9)
    angles = [v[0] for v in flank]
    dips = 0
    for i in range(len(angles) - 1):
        if angles[i + 1] > angles[i] + 1e-9:
            dips += 1
    assert_true(dips > 0, msg="the undercut neck: the flank angle rises again above the neck")
    w = spur(M, 41, 4, tolerance=0.003, name="u41")
    angles = [v[0] for v in left_flank(outline(w, (0, 0)), layout(M, 41, tolerance=0.003))]
    for i in range(len(angles) - 1):
        assert_true(angles[i + 1] <= angles[i] + 1e-9, msg="a 41-tooth flank narrows all the way from root to tip")
    # A shifted small pinion is not undercut.
    lay = layout(M, 9, profile_shift=auto_shift(9))
    assert_true(not lay["undercut"], msg="auto_shift(9) avoids the undercut")


test("9 teeth get the undercut trochoid, 41 do not, and the auto shift avoids it", undercut_is_generated)


def tooth_face_names():
    z = 17
    lay = layout(M, z)
    g = spur(M, z, 6, bore=8, name="named")
    f = tooth_faces(g, lay, 0)
    assert_eq(f["tip"].count(), 1)
    tip = f["tip"].one()
    assert_close(tip.centroid[1], 0.0, tol=1e-6, msg="tooth 0's tip land is centred on +x")
    assert_close(math.hypot(tip.centroid[0], tip.centroid[1]), lay["r_tip"] * math.cos(2.0), tol=0.02)
    assert_true(f["flank_left"].nth(0).centroid[1] > 0, msg="flank_left is on the +y side of tooth 0")
    assert_true(f["flank_right"].nth(0).centroid[1] < 0, msg="flank_right is on the -y side")
    assert_eq(f["root"].count(), len(lay["parts"]["root"]), msg="one root face per land run (a short land is one segment)")
    assert_eq(f["fillet_left"].count(), f["fillet_right"].count())
    # Tooth 5 is the same set, turned.
    f5 = tooth_faces(g, lay, 5)
    tip5 = f5["tip"].one()
    assert_close(math.atan2(tip5.centroid[1], tip5.centroid[0]), 360.0 * 5 / z, tol=1e-6)
    # The bore is loop 1.
    assert_true(g.faces(name="side[1.*]").count() >= 1)
    # The names survive a transform.
    moved = g.rotate(30, axis=(0, 0, 1), name="named_turned").translate((50, 0, 0), name="named_moved")
    tipm = tooth_faces(moved, lay, 0)["tip"].one()
    assert_close(math.atan2(tipm.centroid[1], tipm.centroid[0] - 50), 30.0, tol=1e-6)


test("tooth_faces names the tip, flanks, fillets and root of any tooth, after transforms too", tooth_face_names)

# ----------------------------------------------------------------- meshing


def spur_pair_gap(backlash):
    # Sampled at 0.006 mm over 1.2 pitches either side of the line of
    # centres: Starlark boxes every float and collects nothing while a test
    # runs, so a document's tests share one 512 MB heap.
    z1, z2 = 17, 29
    p = spur(M, z1, 4, backlash=backlash, tolerance=0.006, name="pin_%s" % backlash)
    w = spur(M, z2, 4, backlash=backlash, tolerance=0.006, name="whl_%s" % backlash)
    a = centre_distance(M, z1, z2)
    w = w.rotate(mesh_turn(z2), axis=(0, 0, 1), name="whl_t_%s" % backlash).translate((a, 0, 0), name="whl_p_%s" % backlash)
    pa = near(outline(p, (0, 0)), 0.0, 1.2 * 360.0 / z1)
    wa = near(outline(w, (a, 0)), 180.0, 1.2 * 360.0 / z2)
    return min_gap(pa, wa)


def two_spurs_mesh_with_the_backlash_asked_for():
    # Zero backlash: the involutes touch (the chords sit inside them by the
    # sampling tolerance at most). With each tooth thinned by b, the flanks
    # of the centred mesh stand b cos(alpha) apart along the normal.
    assert_true(spur_pair_gap(0.0) < 0.015, msg="zero-backlash flanks touch")
    for b in (0.15, 0.3):
        assert_close(spur_pair_gap(b), b * math.cos(ALPHA), tol=0.015, msg="backlash %s" % b)


test("two spurs at the centre distance mesh with the backlash asked for", two_spurs_mesh_with_the_backlash_asked_for)


def rack_meshes_with_a_pinion():
    z = 17
    b = 0.2
    r = rack(M, 80, 4, 8, backlash=b, tolerance=0.006, name="rk")
    lay = layout(M, z, backlash=b, tolerance=0.006)
    # The pinion above the pitch line, a space pointing down at the rack's tooth 0.
    p = spur(M, z, 4, backlash=b, tolerance=0.006, name="rp").rotate(-90 + 180.0 / z, axis=(0, 0, 1), name="rp_t").translate((0, lay["r_pitch"], 0), name="rp_p")
    lo, hi = r.bbox()
    assert_close(hi[1], M, tol=1e-6, msg="rack tips at +module")
    assert_close(lo[1], -8, tol=1e-6)
    assert_close(hi[0] - lo[0], 80, tol=1e-6)
    pa = near(outline(p, (0, lay["r_pitch"])), -90.0, 1.2 * 360.0 / z)
    ra = [v for v in outline(r, (0, -100)) if abs(v[2]) < 1.2 * math.pi * M and v[3] > -3]
    ra = sorted(ra, key=lambda v: v[2])
    assert_close(min_gap(pa, ra), b * math.cos(ALPHA), tol=0.02)


test("a rack meshes with a pinion on its pitch line with the backlash asked for", rack_meshes_with_a_pinion)


def ring_and_planet_mesh(sun, planet, ring_gap):
    # Every gear thinned by b, so each mesh stands b cos(alpha) apart on the
    # normal -- the ring's backlash widens its spaces as a planet's narrows
    # its teeth -- and every planet vertex is strictly inside the ring's space.
    # Sampled at 0.01 mm and checked over the mesh zones only: Starlark boxes
    # every float and collects nothing between the tests of one run, so the
    # whole document's test arithmetic counts against one 512 MB heap.
    b = 0.1
    P = planetary(sun, planet, planets=3, module=M)
    ring = internal(M, P["ring"], 4, 3, backlash=b, tolerance=0.01, name="ring_%d_%d" % (sun, planet)).rotate(P["ring_turn"], axis=(0, 0, 1), name="ring_%d_%d_t" % (sun, planet))
    sun_body = spur(M, sun, 4, backlash=b, tolerance=0.01, name="sun_%d_%d" % (sun, planet))
    a = P["centre_distance"]
    planets = []
    for k in range(3):
        g = spur(M, planet, 4, backlash=b, tolerance=0.01, name="planet_%d_%d_%d" % (sun, planet, k))
        g = g.rotate(P["planet_turns"][k], axis=(0, 0, 1), name="planet_%d_%d_%d_t" % (sun, planet, k))
        c = (a * math.cos(P["planet_angles"][k]), a * math.sin(P["planet_angles"][k]))
        planets.append((g.translate((c[0], c[1], 0), name="planet_%d_%d_%d_p" % (sun, planet, k)), c))
    ring_loop = outline(ring, (0, 0))
    sun_loop = outline(sun_body, (0, 0))
    assert_true(len(ring_loop) == layout(M, P["ring"], backlash=b, tolerance=0.01, internal=True)["segments"] * P["ring"], msg="the ring's tooth loop is loop 0")
    p_pitch, r_pitch, s_pitch = 360.0 / planet, 360.0 / P["ring"], 360.0 / sun
    for k in range(3):
        g, c = planets[k]
        pl = outline(g, c)
        ang = P["planet_angles"][k]
        # The zones: the planet's vertices facing the ring and the sun, and
        # the ring's and sun's vertices facing the planet.
        p_out = near(pl, ang, 1.2 * p_pitch)
        p_in = near(pl, ang + 180.0, 1.2 * p_pitch)
        rz = near(ring_loop, ang, 2.2 * r_pitch)
        sz = near(sun_loop, ang, 1.6 * s_pitch)
        for v in p_out:
            # In world polar coordinates about the ring's centre.
            wa, wr = math.atan2(v[3], v[2]), math.hypot(v[2], v[3])
            r_ring = radius_at(rz, wa)
            assert_true(r_ring != None and wr < r_ring, msg="planet %d vertex (%s, %s) is inside the ring's tooth loop" % (k, v[2], v[3]))
        for v in p_in:
            wa, wr = math.atan2(v[3], v[2]), math.hypot(v[2], v[3])
            r_sun = radius_at(sz, wa)
            assert_true(r_sun != None and wr > r_sun, msg="planet %d vertex (%s, %s) is outside the sun" % (k, v[2], v[3]))
        # The gaps in the two mesh zones.
        gs = min_gap(sz, p_in)
        gr = min_gap(rz, p_out)
        assert_close(gs, b * math.cos(ALPHA), tol=0.012, msg="planet %d to the sun: the backlash gap" % k)
        assert_close(gr, ring_gap, tol=0.012, msg="planet %d to the ring" % k)


def planetary_12_18_48():
    # Nothing undercut on the ring side: the ring gap is the backlash.
    ring_and_planet_mesh(12, 18, 0.1 * math.cos(ALPHA))


def planetary_12_12_36():
    # The 12-tooth planet is undercut, and the
    # 36-tooth ring's tip reaches below the planet's base circle, so its tip
    # corner passes the planet's undercut neck 0.021 mm closer than the
    # flanks stand: 0.073 mm, not 0.094 (the same at 0.003 mm sampling, so
    # it is the geometry of the set, not the chords) -- a shifted planet
    # (`auto_shift`) or a shortened ring tip would open it, and a
    # print-in-place set should know it.
    ring_and_planet_mesh(12, 12, 0.0731)


test("a 12/18/48 planetary: every planet sits between the sun and the ring at the phases planetary() gives, with the backlash asked for", planetary_12_18_48)
test("a 12/12/36 planetary meshes at planetary()'s phases; the ring's tip passes the undercut planet's neck closer than the flanks", planetary_12_12_36)

# ----------------------------------------------------------------- bodies


def helical_is_the_twisted_prism():
    beta = 25.0
    s = extrude(involute_profile(M, 17, helix_angle=beta, name="sec"), 10, name="prism")
    h = helical(M, 17, 10, beta, name="hel")
    assert_solid(h)
    assert_eq(h.kind, "brep")
    assert_close(h.volume(), s.volume(), rel=0.002, msg="a twisted prism has the prism's volume")
    assert_close(h.bbox()[1][2], 10, tol=1e-6)
    lay = layout(M, 17, helix_angle=beta)
    assert_eq(len(h.faces()), lay["segments"] * 17 + 2, msg="a smooth loft has one face per segment column")
    # Right and left hands are mirror images: same volume, the tooth turned the other way at the top.
    # To 1e-4, not to the last digit: the two skins are mirror images but a
    # twisted cell's two triangles are not, since which diagonal a cocircular
    # cell takes is the triangulation's tie-break, so the mirrored
    # meshes differ by a few chord errors.
    left = helical(M, 17, 10, beta, hand="left", name="hel_l")
    assert_close(left.volume(), h.volume(), rel=1e-4)
    ft = tooth_faces(h, lay, 0)["tip"].one().centroid
    fl = tooth_faces(left, lay, 0)["tip"].one().centroid
    # Tessellation centroids, mirror images only to their diagonals: to a micron.
    assert_close(ft[1], -fl[1], tol=1e-3, msg="the two hands lean the tip land opposite ways")
    twist = math.degrees(10 * math.tan(beta) / lay["r_pitch"])
    assert_true(ft[1] > 0, msg="a right-hand tooth turns counter-clockwise going up")
    assert_close(math.atan2(ft[1], ft[0]), twist / 2, tol=0.2, msg="the tip's centroid sits at half the twist")


test("a helical gear is the transverse profile twisted by width tan(beta) / r, right hand counter-clockwise", helical_is_the_twisted_prism)


def herringbone_halves_mirror():
    beta, w = 30.0, 12.0
    g = herringbone(1.5, 12, w, beta, bore=4, name="hb")
    assert_solid(g)
    assert_eq(g.kind, "brep")
    s = extrude(involute_profile(1.5, 12, helix_angle=beta, name="hb_sec").circle(2), w, name="hb_prism")
    # Two tessellations of the same flanks, chords on different stations:
    # 0.25 % apart at 0.05.
    assert_close(g.volume(), s.volume(), rel=0.004)
    lay = layout(1.5, 12, helix_angle=beta)
    for s_ in (0, 5, lay["segments"] // 2, lay["segments"] * 7 + 3):
        lo = g.faces(name="hb_lower/side[0.%d]" % s_).one().centroid
        up = g.faces(name="hb_upper/side[0.%d]" % s_).one().centroid
        assert_close((lo[0], lo[1], w - lo[2]), (up[0], up[1], up[2]), tol=0.005, msg="segment %d mirrors about z = w/2 (centroids come from the tessellation)" % s_)
        # The end sections are exact: the lower half's z = 0 outline is the upper half's z = w outline, vertex for vertex.
        lo_e = [e for e in g.faces(name="hb_lower/side[0.%d]" % s_).edges() if abs(e.start[2]) < 1e-9 and abs(e.end[2]) < 1e-9]
        up_e = [e for e in g.faces(name="hb_upper/side[0.%d]" % s_).edges() if abs(e.start[2] - w) < 1e-9 and abs(e.end[2] - w) < 1e-9]
        assert_true(len(lo_e) == 1 and len(up_e) == 1, msg="one bottom edge and one top edge on segment %d" % s_)
        assert_close(sorted([lo_e[0].start[:2], lo_e[0].end[:2]]), sorted([up_e[0].start[:2], up_e[0].end[:2]]), tol=1e-9)
    # tooth_faces sees both halves.
    f = tooth_faces(g, lay, 0)
    assert_eq(f["tip"].count(), 2)
    zs = [t.centroid[2] for t in f["tip"]]
    # Centroids come from the tessellation, and the two halves' meshes are
    # mirror images only to their diagonals, so to a micron.
    assert_close((zs[0] + zs[1]) / 2, w / 2, tol=1e-3)


test("a herringbone's two halves are mirror images about the mid-plane and glue into one solid", herringbone_halves_mirror)


def internal_gear():
    z = 36
    g = internal(M, z, 5, 4, name="ring36")
    assert_solid(g)
    lay = layout(M, z, internal=True)
    assert_close(lay["r_tip"], M * z / 2 - M)
    assert_close(lay["r_root"], M * z / 2 + 1.25 * M)
    lo, hi = g.bbox()
    assert_close(hi[0], lay["r_root"] + 4, tol=1e-6, msg="the housing is rim beyond the root circle")
    a = outline(g, (0, 0))
    assert_close(min([v[1] for v in a]), lay["r_tip"], tol=1e-6)
    assert_close(max([v[1] for v in a]), lay["r_root"], tol=1e-6)
    # Space 0 on +x: the vertex nearest +x is the bottom of space 0 (a fillet
    # end a fraction of a degree off the axis), and a tooth tip sits at half a pitch.
    v0 = min(a, key=lambda v: abs(v[0]))
    assert_true(abs(v0[0]) < 0.5, msg="a vertex within half a degree of +x")
    assert_close(v0[1], lay["r_root"], tol=1e-6, msg="and it is the bottom of space 0")
    tip = tooth_faces(g, lay, 0)["tip"].one()
    assert_close(math.atan2(tip.centroid[1], tip.centroid[0]), 180.0 / z, tol=1e-6)
    assert_close(math.hypot(tip.centroid[0], tip.centroid[1]), lay["r_tip"], tol=0.02)
    # A small ring's addendum is shortened to the base circle.
    small = layout(M, 21, internal=True)
    assert_close(small["r_tip"], small["r_base"], tol=1e-9)
    assert_solid(internal(M, 21, 4, 3, name="ring21"))
    # Helical and herringbone rings build.
    assert_solid(internal(M, z, 6, 4, helix_angle=25, name="ring_h"))


test("an internal gear: spaces are external teeth, addendum and dedendum swap, space 0 on +x", internal_gear)


def fluted_ring():
    # The outline: lands on the circle, grooves `depth` deep, a vertex at
    # each groove's ends shared with the land, counter-clockwise.
    r, n, depth = 50.0, 24, 0.8
    pts = fluted_outline(r, n, depth, radius=1.6, tolerance=0.005)
    radii = [p[0] for p in pts]
    assert_close(max(radii), r, tol=1e-9)
    assert_close(min(radii), r - depth, tol=1e-9, msg="each groove bottoms out `depth` in")
    lands = [p for p in pts if abs(p[0] - r) < 1e-9]
    assert_true(len(lands) >= 2 * n, msg="every groove starts and ends on the circle")
    turns = [(pts[i + 1][1] - pts[i][1]) % 360.0 for i in range(len(pts) - 1)]
    assert_true(all([t > 0 and t < 10 for t in turns]), msg="the outline runs counter-clockwise without doubling back")
    # A herringbone ring with the grip in its section: exact, one solid, the
    # housing faces the outline's segments in both halves, and no loft warns
    # that it is twisted (the twist is the helix).
    lay = layout(M, 36, internal=True, helix_angle=30)
    g = internal(M, 36, 12, 4, helix_angle=30, herringbone=True, flutes=n, flute_depth=depth, flute_radius=1.6, name="ring_grip")
    assert_solid(g)
    assert_eq(g.kind, "brep")
    housing = fluted_outline(lay["r_root"] + 4, n, depth, radius=1.6, tolerance=0.005)
    assert_eq(len(g.faces(name="*side[1.*")), 2 * len(housing))
    lo, hi = g.bbox()
    assert_close(hi[2] - lo[2], 12, tol=1e-6)
    return len(housing)


test("a ring's grip is part of its section: grooves on the housing loop, chevrons on a herringbone, one exact solid", fluted_ring)


def bevel_gear():
    z, zm = 20, 30
    b = bevel(M, z, zm, bore=6, name="bev")
    assert_solid(b)
    assert_eq(b.kind, "brep")
    delta = math.atan2(1.0, zm / z)
    r = M * z / 2
    cone = r / math.sin(delta)
    z_heel = r / math.tan(delta)
    lo, hi = b.bbox()
    assert_close(hi[2], z_heel, tol=1e-6, msg="the heel is at r / tan(delta)")
    assert_close(lo[2], z_heel * (cone - cone / 3) / cone, tol=1e-6, msg="the toe a third of the cone distance in")
    r_tip = r + M * math.cos(delta)
    assert_true(hi[0] <= r_tip + 1e-6 and hi[0] >= r_tip * math.cos(4), msg="the heel's tip radius is r + m cos(delta)")
    # A ruled loft of a scaled copy: every side face is a segment column, every flank line hits the apex.
    lay = layout(M, z)
    assert_eq(len(b.faces()), len(b.faces(name="side[0.*]")) + 2 + len(b.faces(name="side[1.*]")))
    e = b.faces(name="side[0.*]").edges()
    for k in range(0, len(e), 37):
        ed = e[k]
        if abs(ed.start[2] - ed.end[2]) < 1e-6:
            continue
        # The line through start and end passes through the origin.
        d = (ed.end[0] - ed.start[0], ed.end[1] - ed.start[1], ed.end[2] - ed.start[2])
        t = -ed.start[2] / d[2]
        assert_close((ed.start[0] + t * d[0], ed.start[1] + t * d[1]), (0.0, 0.0), tol=1e-6, msg="flank line through the apex")
    # Its mate, turned to the shaft angle about the origin, has its heel pitch circle meeting this one's.
    mate = bevel(M, zm, z, name="bev_mate").rotate(90, axis=(0, 1, 0), name="bev_mate_t")
    assert_solid(mate)
    mlo, mhi = mate.bbox()
    assert_close(max(abs(mlo[0]), abs(mhi[0])), M * zm / 2 / math.tan(math.atan2(1.0, z / zm)), tol=1e-6, msg="the mate's heel plane after the turn")


test("a straight bevel: heel at r / tan(delta), tip r + m cos(delta), flank lines through the apex", bevel_gear)


def solid_at_every_count():
    g = spur(M, teeth, 4, name="sweep_spur")
    assert_solid(g)
    assert_eq(g.kind, "brep")
    return g.volume()


test("a solid spur at every tooth count, undercut ones included", solid_at_every_count, sweep={"teeth": [6, 8, 17, 41, 120]}, monotone="increasing")


def solid_at_every_helix():
    g = helical(M, 17, 8, helix, name="sweep_helical")
    assert_solid(g)
    h = herringbone(M, 17, 8, helix, name="sweep_herringbone")
    assert_solid(h)
    # Two tessellations of one shape, each chorded at the tolerance (
    # 2.2e-3 apart at helix=20), so half a percent.
    assert_close(g.volume(), h.volume(), rel=0.005)
    return g.volume()


test("a solid helical and herringbone at every helix angle", solid_at_every_helix, sweep={"helix": [5, 20, 45]})


def herringbone_at_every_face_width():
    """A herringbone (module 2, 12 teeth, 30 deg) glues its halves at every
    face width, including those whose quarter height falls on a half-cell of
    the glue's 0.1 mm grid (12.6 / 4 = 3.15 = 31.5 cells). Every width here
    is one body of the two halves' volume."""
    g = herringbone(2.0, 12, face, 30.0, name="sweep_face")
    assert_solid(g)
    assert_eq(g.kind, "brep")
    s = extrude(involute_profile(2.0, 12, helix_angle=30.0, name="sweep_face_sec"), face, name="sweep_face_prism")
    assert_close(g.volume(), s.volume(), rel=0.005)  # 0.3 % at this twist: the flanks' tessellation, not the seam
    return g.volume()


test("a herringbone glues its halves at every face width, the half-cell ones included", herringbone_at_every_face_width, sweep={"face": [12.0, 12.6, 13.3, 14.0, 15.4]}, monotone="increasing")


# ----------------------------------------------------------------- planetary_set


def a_planetary_set_is_the_planetary_example():
    """`planetary_set` builds what examples/planetary.star measures: the
    arithmetic, the instances where the stack puts them, the hooks
    called with the arithmetic, `ring_lift` lifting the ring and shortening
    its gear by as much, and a name prefix on every feature."""
    seen = {}

    def boss(z_top, info):
        seen["boss"] = info["r_boss"]
        return extrude(sketch(XY.offset(z_top), name="mini_boss_profile").ngon(6, 4.0), 5.0, name="mini_boss")

    def plate_figure(plate_body, info):
        seen["plate"] = info["r_plate"]
        block = extrude(sketch(XY.offset(3.0), name="mini_block_profile").rect(6, 6, center=(0.6 * info["r_plate"] * math.cos(60.0), 0.6 * info["r_plate"] * math.sin(60.0))), 2.0, name="mini_block")  # between planets 0 and 1
        return glue(plate_body, block, name="mini_plate_with_block")

    def ring_as_is(ring_body, info):
        seen["ring"] = info["lay_ring"]["r_root"]
        return ring_body

    gap, width, lift = 0.35, 6.0, 0.4
    S = planetary_set(2.0, 12, 12, width, planets=3, helix_angle=30.0, gap=gap, tolerance=0.02, sun_boss=boss, plate_extra=plate_figure, ring_extra=ring_as_is, ring_lift=lift, name="mini")
    # The arithmetic, against the closed forms the example spells out.
    P = planetary(12, 12, planets=3, module=2.0, helix_angle=30.0)
    assert_eq(S["teeth_ring"], 36)
    assert_close(S["P"]["ratio_fixed_ring"], 4.0)
    assert_close(S["a"], P["centre_distance"])
    alpha_t = math.atan(math.tan(20.0) / math.cos(30.0))
    beta_b = math.atan(math.tan(30.0) * math.cos(alpha_t))
    assert_close(S["alpha_t"], alpha_t)
    assert_close(S["backlash"], gap / (math.cos(alpha_t) * math.cos(beta_b)))
    assert_close(S["ring_shift"], S["backlash"] / (2 * S["m_t"] * math.tan(20.0)))
    assert_close(S["clearance"], 0.25)
    assert_close(S["z_gears"], 3.4)
    assert_close(S["ring_height"], 2 * (3.4 - lift) + width, msg="the ring gear is shorter by the lift")
    assert_close(S["r_plate"], S["lay_ring"]["r_tip"] - 0.5)
    assert_close(S["r_bore"], 4.35)
    assert_close(S["pin_height"], width + 0.8)
    assert_eq(len(S["pin_centres"]), 3)
    assert_close(S["pin_centres"][0][0], S["a"])
    # The hooks ran, each with the arithmetic.
    assert_close(seen["boss"], S["r_boss"])
    assert_close(seen["plate"], S["r_plate"])
    assert_close(seen["ring"], S["lay_ring"]["r_root"])
    # Instances where the stack puts them; names carry the prefix.
    sun, ring, carrier = S["sun"], S["ring"], S["carrier"]
    assert_eq(sun.name, "mini_sun")
    assert_eq(ring.name, "mini_ring")
    assert_eq(len(S["planets"]), 3)
    assert_eq(S["instances"][0].name, "mini_carrier")
    lo, hi = sun.body().bbox()
    assert_close(lo[2], 3.4, tol=1e-6, msg="the sun floats two layers over the plate")
    assert_close(hi[2], 3.4 + width + 5.0, tol=1e-6, msg="the hook's boss stands on the sun")
    lo, hi = ring.body().bbox()
    assert_close(lo[2], lift, tol=1e-6, msg="the ring is lifted")
    assert_close(hi[2], lift + S["ring_height"], tol=1e-6)
    lo, hi = carrier.body().bbox()
    assert_close(lo[2], 0.0, tol=1e-6)
    assert_close(hi[2], 3.0 + S["pin_height"] + 1.2 + 1.5, tol=1e-6, msg="the pin heads")
    assert_true(hi[0] >= S["r_plate"] - 0.05)
    for b in S["instances"]:
        assert_solid(b.body())
    assert_eq(len(S["instances"]), 6)
    # The block was glued on the plate's top before the pins: a hole in the
    # plate's top face and the block's faces, none of them a boolean's.
    assert_true(S["carrier_body"].faces(name="mini_block/end").count() == 1, msg="the block's top is on the carrier")
    # The meshes run at the gap asked for, within the chord band.
    c = distance(sun, S["planets"][0])
    assert_true(not c.overlap)
    assert_close(c.distance, gap, tol=0.03 + 2 * c.tolerance, msg="sun to planet: %s" % c.distance)
    c = distance(S["planets"][0], ring)
    assert_true(not c.overlap)
    assert_close(c.distance, gap, tol=0.03 + 2 * c.tolerance, msg="planet to ring: %s" % c.distance)
    c = distance(S["planets"][0], carrier)
    assert_close(c.distance, 0.35, tol=0.02 + 2 * c.tolerance, msg="the pin in its bore: %s" % c.distance)
    return c.distance


test("planetary_set builds the planetary example's set: its arithmetic, its instances, its hooks, a lifted ring", a_planetary_set_is_the_planetary_example)
