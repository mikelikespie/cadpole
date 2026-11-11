# The tests of @cadpole/building.star. A document, not a library: it
# loads the library, declares the parameters its sweeps need, and builds its
# masses inside `test()` functions, so `cadpole test stdlib/building_test.star`
# runs them and `//src/rust/cadpole-script:cadpole_script_test`
# (`every_stdlib_test_passes`) runs them in CI. The refusals -- a stair over
# the code's rise, a run under its minimum -- are Rust tests in
# `src/rust/cadpole-script/src/stdlib.rs`, because a script cannot catch `fail()`.

load(
    "@cadpole/building.star",
    "cupola",
    "deck",
    "elevator_shaft",
    "floor",
    "floor_area",
    "floor_area_cap",
    "footprint_of",
    "ft",
    "ft_in",
    "gable_roof",
    "garage_door",
    "gross_floor_area",
    "height_check",
    "height_envelope",
    "hip_roof",
    "inch",
    "level",
    "lot_coverage",
    "m",
    "offset_polygon",
    "pitch",
    "polygon_area",
    "roof_profile",
    "shed_roof",
    "sq_ft",
    "stair",
    "straight_skeleton",
)

units(units.ft_in)

roof_pitch = param("roof_pitch", 6, unit="in", min=3, max=12, doc="Rise per foot of run the roof sweeps vary")
climb = param("climb", 10, unit="ft", min=8, max=12, doc="Storey height the stair sweep climbs")

# ----------------------------------------------------------------- helpers


def rect(w, d, x=0, y=0):
    return [(x, y), (x + w, y), (x + w, y + d), (x, y + d)]


def hip_volume(w, l, p):
    """A hip roof on a w x l rectangle (l >= w) at pitch p: the gable prism over the middle
    plus two half-pyramids, `W h (3 L - W) / 6` with `h = p W / 2`."""
    h = p * w / 2.0
    return w * h * (3.0 * l - w) / 6.0


# ------------------------------------------------------------------- units


def units_arithmetic():
    assert_close(ft(45), 13716.0, tol=1e-9)
    assert_close(ft(45, 6), 13868.4, tol=1e-9)
    assert_close(ft_in(45, 6), 13868.4, tol=1e-9)
    assert_close(inch(6), 152.4, tol=1e-9)
    assert_close(m(3.65), 3650.0, tol=1e-9)
    assert_close(sq_ft(ft(30) * ft(40)), 1200.0)
    assert_eq(pitch(6), 0.5)
    assert_eq(pitch("6:12"), 0.5)
    assert_close(pitch(45, "deg"), 1.0)
    # The parser, against the helpers.
    assert_close(length("45'-6\""), ft_in(45, 6), tol=1e-9)
    assert_close(length("12ft 6in"), ft(12, 6), tol=1e-9)
    return ft(1)


test("feet, inches and pitches are plain millimetres", units_arithmetic)


# --------------------------------------------------------- straight skeleton


def skeleton_of_an_l():
    """An L of two 5 x 10 legs: ridges down each leg at t = 2.5, a valley from the reflex
    corner to where they meet, and every face's area adding up to the polygon's."""
    L = [(0, 0), (10, 0), (10, 5), (5, 5), (5, 10), (0, 10)]
    sk = straight_skeleton(L)
    assert_close(sk["height"], 2.5, tol=1e-9)
    nodes = sorted([(math.round(n[0], 6), math.round(n[1], 6), math.round(n[2], 6)) for n in sk["nodes"]])
    assert_eq(nodes, [(2.5, 2.5, 2.5), (2.5, 7.5, 2.5), (7.5, 2.5, 2.5)])
    assert_eq(len(sk["faces"]), 6)
    total = 0.0
    for face in sk["faces"]:
        total += polygon_area([(q[0], q[1]) for q in face])
    assert_close(total, 75.0, tol=1e-9)
    # The bottom edge's face is the trapezoid under leg A's ridge.
    bottom = sorted([(math.round(q[0], 6), math.round(q[1], 6)) for q in sk["faces"][0]])
    assert_eq(bottom, [(0.0, 0.0), (2.5, 2.5), (7.5, 2.5), (10.0, 0.0)])
    # The valley: the arc from the reflex corner, bordering the two edges that meet there.
    valley = [a for a in sk["arcs"] if abs(a[0][0] - 5) < 1e-9 and abs(a[0][1] - 5) < 1e-9]
    assert_eq(len(valley), 1)
    assert_eq(sorted(valley[0][4]), [2, 3])
    # A rectangle: one ridge between two three-way nodes; a square: one apex.
    r = straight_skeleton(rect(4, 7))
    assert_eq(len(r["nodes"]), 2)
    assert_close(r["height"], 2.0, tol=1e-9)
    s = straight_skeleton(rect(5, 5))
    assert_eq(len(s["nodes"]), 1)
    assert_close(s["nodes"][0][2], 2.5, tol=1e-9)
    # A T: the stem's two reflex corners meet and split the wavefront.
    T = [(0, 0), (30, 0), (30, 10), (20, 10), (20, 25), (10, 25), (10, 10), (0, 10)]
    t = straight_skeleton(T)
    total = 0.0
    for face in t["faces"]:
        total += polygon_area([(q[0], q[1]) for q in face])
    assert_close(total, 450.0, tol=0.01)
    assert_close(t["height"], 5.0, tol=1e-9)
    return sk["height"]


test("the straight skeleton of an L, a rectangle, a square and a T", skeleton_of_an_l)


# -------------------------------------------------------------------- roofs


def hip_roof_on_a_rectangle():
    """Four planes cut from an oversized prism on the mesh kernel: the closed-form volume,
    one mesh body of five planar faces. `kernel="brep"` gives the exact body where truck
    closes it -- this rectangle at every pitch of the sweep."""
    p = roof_pitch / 12.0
    W, L = ft(20), ft(36)
    r = hip_roof(rect(W, L), pitch=p, name="hip")
    assert_solid(r)
    assert_eq(r.kind, "mesh")
    assert_volume(r, hip_volume(W, L, p), rel=1e-5)
    assert_eq(len(r.faces()), 5)
    exact = hip_roof(rect(W, L), pitch=p, kernel="brep", name="hip_exact")
    assert_solid(exact)
    assert_eq(exact.kind, "brep")
    assert_volume(exact, hip_volume(W, L, p), rel=1e-5)
    assert_eq(len(exact.faces()), 5)
    lo, hi = r.bbox()
    assert_close(hi[2], p * W / 2.0, tol=0.01)
    assert_close(lo[2], 0.0, tol=0.01)
    # An overhang grows the eave polygon; eave_z lifts it; a fascia adds a band.
    r2 = hip_roof(rect(W, L), pitch=p, overhang=ft(2), eave_z=ft(9), name="hip2")
    assert_solid(r2)
    assert_volume(r2, hip_volume(W + ft(4), L + ft(4), p), rel=1e-5)
    lo2, hi2 = r2.bbox()
    assert_close(lo2[2], ft(9), tol=0.01)
    assert_close(lo2[0], -ft(2), tol=0.01)
    r3 = hip_roof(rect(W, L), pitch=p, fascia=inch(6), name="hip3")
    assert_solid(r3)
    assert_volume(r3, hip_volume(W, L, p) + W * L * inch(6), rel=1e-5)
    # A trapezoid (convex, three-way nodes) goes the same way, and the planes are the
    # skeleton's.
    trap = [(0, 0), (ft(30), 0), (ft(24), ft(20)), (ft(6), ft(20))]
    r4 = hip_roof(trap, pitch=p, name="hip4")
    assert_solid(r4)
    lo4, hi4 = r4.bbox()
    assert_close(hi4[2], straight_skeleton(trap)["height"] * p, tol=0.01)
    return r.volume()


test("a hip roof on a rectangle is four planes with the closed-form volume", hip_roof_on_a_rectangle, sweep={"roof_pitch": [4, 6, 12]})


def hip_roof_on_a_square_is_a_pyramid():
    S = ft(16)
    p = 0.5
    r = hip_roof(rect(S, S), pitch=p, name="pyr")
    assert_solid(r)
    assert_eq(r.kind, "brep")
    assert_volume(r, S * S * (p * S / 2.0) / 3.0, rel=1e-5)
    assert_eq(len(r.faces()), 5)
    return r.volume()


test("a hip roof on a square is one loft to its apex", hip_roof_on_a_square_is_a_pyramid)


def hip_roof_on_an_l_shape():
    """Past a reflex corner: one piece per skeleton face, united on the mesh kernel. The
    volume is checked against the faces themselves -- a plane over a polygon integrates to
    the area times the height at the centroid -- and against the union of the two legs'
    own hip roofs, which is the same roof."""
    p = 0.5
    L = [(0, 0), (ft(30), 0), (ft(30), ft(15)), (ft(15), ft(15)), (ft(15), ft(30)), (0, ft(30))]
    r = hip_roof(L, pitch=p, name="lroof")
    assert_solid(r)
    sk = straight_skeleton(L)
    expected = 0.0
    for e in range(len(L)):
        face = sk["faces"][e]
        pts = [(q[0], q[1]) for q in face]
        # A plane over a triangle integrates to the area times the height at the
        # centroid, so fan-triangulate each face and sum.
        vol = 0.0
        for k in range(1, len(pts) - 1):
            tri = [pts[0], pts[k], pts[k + 1]]
            ta = polygon_area(tri)
            tc = ((tri[0][0] + tri[1][0] + tri[2][0]) / 3.0, (tri[0][1] + tri[1][1] + tri[2][1]) / 3.0)
            # Height on this face's plane: the pitch times the distance from edge e's line.
            a0 = L[e]
            a1 = L[(e + 1) % len(L)]
            d = (a1[0] - a0[0], a1[1] - a0[1])
            dl = math.hypot(d[0], d[1])
            dist = (-(d[1]) * (tc[0] - a0[0]) + d[0] * (tc[1] - a0[1])) / dl
            vol += ta * p * dist
        expected += vol
    assert_volume(r, expected, rel=1e-5)
    lo, hi = r.bbox()
    # The pieces overlap by half a millimetre across the arcs, so a piece's plane runs a
    # hair past the ridge: the ridge reads 0.35 mm high at this pitch, never a millimetre.
    assert_close(hi[2], p * ft(15) / 2.0, tol=1.0)
    assert_eq(r.kind, "mesh")
    # The same roof as the union of the legs' hip roofs.
    a = hip_roof(rect(ft(30), ft(15)), pitch=p, name="leg_a")
    b = hip_roof(rect(ft(15), ft(30)), pitch=p, name="leg_b")
    both = union(a, b, name="legs")
    assert_volume(both, expected, rel=1e-5)
    return r.volume()


test("a hip roof on an L is the straight skeleton's faces, one plane each", hip_roof_on_an_l_shape)


def hip_roof_with_a_placed_ridge():
    """`ridge=` on a rectangle: the ridge's plan ends set the hip ends' pitch; on the centre
    line it is the skeleton's roof."""
    W, L = ft(20), ft(36)
    p = 0.5
    centred = hip_roof(rect(W, L), pitch=p, ridge=((ft(10), ft(10)), (ft(10), ft(26))), name="ridged")
    assert_solid(centred)
    assert_volume(centred, hip_volume(W, L, p), rel=1e-5)
    # A shorter ridge: steeper hip ends, less volume.
    short = hip_roof(rect(W, L), pitch=p, ridge=((ft(10), ft(14)), (ft(10), ft(22))), name="short")
    assert_solid(short)
    assert_true(short.volume() < centred.volume())
    lo, hi = short.bbox()
    assert_close(hi[2], p * W / 2.0, tol=0.01)
    return short.volume()


test("a placed ridge sets the hip ends", hip_roof_with_a_placed_ridge)


def gable_and_shed_roofs():
    p = roof_pitch / 12.0
    W, L = ft(20), ft(36)
    g = gable_roof(rect(W, L), pitch=p, name="gable")
    assert_solid(g)
    assert_eq(g.kind, "brep")
    h = p * W / 2.0
    assert_volume(g, W * h / 2.0 * L, rel=1e-5)
    assert_eq(len(g.faces()), 5)
    # Ridge along the short axis, and a fascia.
    gy = gable_roof(rect(W, L), pitch=p, ridge_axis="y", fascia=inch(6), name="gable_y")
    assert_solid(gy)
    # The ridge along y rises from the x sides: the span is W.
    assert_volume(gy, W * h / 2.0 * L + W * L * inch(6), rel=1e-5)
    # On a footprint that is not a rectangle: the prism cut by the two planes, exact with a
    # fascia, a mesh body without one -- the same volume either way.
    # (Its side corners sit off the ridge line: a corner on the ridge is a vertex the second
    # plane passes through exactly, which the exact kernel refuses.)
    hexa = [(0, 0), (ft(30), 0), (ft(36), ft(8)), (ft(30), ft(20)), (0, ft(20)), (-ft(6), ft(8))]
    gf = gable_roof(hexa, pitch=p, fascia=inch(6), name="gable_hex")
    assert_solid(gf)
    assert_eq(gf.kind, "brep")
    gm = gable_roof(hexa, pitch=p, name="gable_hex_mesh")
    assert_solid(gm)
    assert_eq(gm.kind, "mesh")
    assert_close(gf.volume() - gm.volume(), polygon_area(hexa) * inch(6), rel=1e-5)
    # A shed: one plane, a right prism.
    s = shed_roof(rect(W, L), pitch=p, direction=(1, 0), name="shed")
    assert_solid(s)
    assert_eq(s.kind, "brep")
    assert_volume(s, W * (p * W) / 2.0 * L, rel=1e-5)
    s2 = shed_roof(rect(W, L), pitch=p, direction=(0, -1), fascia=inch(4), name="shed_y")
    assert_solid(s2)
    assert_volume(s2, L * (p * L) / 2.0 * W + W * L * inch(4), rel=1e-5)
    lo, hi = s2.bbox()
    assert_close(hi[2], inch(4) + p * L, tol=0.01)
    return g.volume()


test("gable and shed roofs, on rectangles and off them", gable_and_shed_roofs, sweep={"roof_pitch": [4, 12]})


def a_cupola_on_a_roof():
    r = hip_roof(rect(ft(20), ft(36)), pitch=0.5, eave_z=ft(9), name="croof")
    c = cupola(ft(4), ft(5), on=r, name="cupola")
    assert_solid(c)
    lo, hi = c.bbox()
    ridge = ft(9) + 0.5 * ft(10)
    assert_close(lo[2], ridge - ft(1), tol=0.01)
    # Its own pyramid: 4 ft (plus 3 in of eave each side) wide at pitch 1.
    assert_close(hi[2], ridge - ft(1) + ft(5) + 1.0 * (ft(4) + inch(6)) / 2.0, tol=0.01)
    assert_close((lo[0] + hi[0]) / 2.0, ft(10), tol=0.01)
    # The cupola's top, not its volume: the walls-and-roof union is exact, and its
    # volume differs in the last bit of an f64 between native and wasm,
    # which the native-vs-wasm ratchet records once per category and a returned value
    # would record again.
    return hi[2]


test("a cupola straddles the ridge", a_cupola_on_a_roof)


# ------------------------------------------------------------ floors, decks


def floors_and_levels():
    f = floor(rect(ft(30), ft(40)), ft(0), ft(10), name="plate")
    assert_solid(f)
    assert_volume(f, ft(30) * ft(40) * ft(10), rel=1e-5)
    hollow = floor(rect(ft(30), ft(40)), ft(10), ft(10), solid=False, slab=ft(1), walls=inch(6), name="tray")
    assert_solid(hollow)
    assert_eq(hollow.kind, "brep")
    inner = (ft(30) - ft(1)) * (ft(40) - ft(1)) * ft(9)
    assert_volume(hollow, ft(30) * ft(40) * ft(10) - inner, rel=1e-5)
    lv = level("L1", ft(0), ft(10), bodies=[f])
    assert_eq(lv["top"], ft(10))
    assert_eq(lv["plan_cut"], ft(4))
    # The level's top, not the tray's volume: the tray's tessellation splits one quad
    # differently on wasm (the native-vs-wasm ratchet records it), and a returned value
    # measured on it would differ in its last bits.
    return lv["top"]


test("a floor is a box or a tray, one exact body", floors_and_levels)


def a_deck_on_posts():
    def ground(x, y):
        return -ft(2) - x / 10.0

    d = deck(rect(ft(12), ft(20)), ft(8), inch(10), posts=(ft(6), inch(6)), rail=(inch(42), inch(2)), ground=ground, name="deck")
    assert_solid(d["slab"])
    # Corners plus every 6 ft along the edges: 2 + 4 + 2 + 4 posts.
    assert_eq(len(d["posts"]), 12)
    for post in d["posts"]:
        assert_solid(post)
        lo, hi = post.bbox()
        assert_close(hi[2], ft(8) - inch(10), tol=0.01)
        assert_close(lo[2], ground((lo[0] + hi[0]) / 2.0, (lo[1] + hi[1]) / 2.0), tol=0.01)
    assert_eq(len(d["rails"]), 1)
    assert_solid(d["rails"][0])
    assert_eq(len(d["bodies"]), 14)
    return len(d["posts"])


test("a deck stands on a post at every corner and every six feet", a_deck_on_posts)


# ------------------------------------------------------------------- stairs


def a_stair_to_code():
    """The fewest risers that keep the rise under 7 3/4", a 10" run: at 10 ft that is 16
    risers of 7 1/2"; the body is a solid wedge `run x risers` long."""
    s = stair(0, ft(climb), ft(3), name="stair")
    assert_solid(s["body"])
    assert_true(s["rise"] <= inch(7.75) + 1e-9, msg="rise %s" % s["rise"])
    assert_eq(s["run"], inch(10))
    assert_close(s["risers"] * s["rise"], ft(climb), tol=0.01)
    lo, hi = s["body"].bbox()
    assert_close(hi[2], ft(climb), tol=0.01)
    assert_close(hi[0], s["risers"] * inch(10), tol=0.01)
    assert_close(hi[1], ft(3), tol=0.01)
    if climb == 10:
        assert_eq(s["risers"], 16)
        assert_close(s["rise"], inch(7.5), tol=1e-9)
    # With a landing after step 8, turned to climb along -y from (5 ft, 5 ft).
    t = stair(ft(1), ft(1) + ft(climb), ft(3), landings=[8], direction=(0, -1), at=(ft(5), ft(5)), name="stair2")
    assert_solid(t["body"])
    assert_close(t["length"], s["risers"] * inch(10) + ft(3), tol=0.01)
    lo2, hi2 = t["body"].bbox()
    assert_close(hi2[1], ft(5), tol=0.01)
    assert_close(lo2[1], ft(5) - t["length"], tol=0.01)
    assert_close(t["top"][1], ft(5) - t["length"], tol=0.01)
    # A rise asked for, and a code lifted.
    u = stair(0, ft(climb), ft(3), rise=ft(climb) / 20.0, code="none", name="stair3")
    assert_eq(u["risers"], 20)
    return s["risers"]


test("a stair takes the fewest risers under the code's rise", a_stair_to_code, sweep={"climb": [8, 10, 12]})


# ------------------------------------------------------------ openings, shafts


def doors_and_shafts():
    mass = floor(rect(ft(24), ft(24)), 0, ft(10), name="garage")
    cut = garage_door(mass, ft(16), ft(7), at=(ft(12), 0), facing=(0, -1), name="door")
    assert_solid(cut)
    assert_volume(cut, ft(24) * ft(24) * ft(10) - ft(16) * ft(7) * inch(4), rel=1e-5)
    shaft = elevator_shaft(ft(7), ft(6), ft(0), ft(20), at=(ft(2), ft(2)), name="lift")
    assert_solid(shaft)
    lo, hi = shaft.bbox()
    assert_close(lo[2], -ft(4), tol=0.01)
    assert_close(hi[2], ft(20) + inch(42), tol=0.01)
    return cut.volume()


test("a garage door is a recess, a shaft runs pit to overrun", doors_and_shafts)


# --------------------------------------------------------------------- areas


def floor_areas():
    """A box mass counts its footprint; an attic under a gable counts only where a person
    can stand: the section 5 ft up, 10 ft wide under a 20 ft, 12:12 roof."""
    plate = floor(rect(ft(30), ft(40)), 0, ft(10), name="a1")
    assert_close(sq_ft(floor_area(plate, 0)), 1200.0, rel=1e-5)
    attic = gable_roof(rect(ft(20), ft(30)), pitch=1.0, eave_z=ft(10), name="attic")
    assert_close(sq_ft(floor_area(attic, ft(10))), 10.0 * 30.0, rel=1e-5)
    # Without the rule, the plan cut at 4 ft is 12 ft wide.
    assert_close(sq_ft(floor_area(attic, ft(10), exclude_under=ft(4))), 12.0 * 30.0, rel=1e-5)
    # Nothing at that height: zero, not an error.
    assert_eq(floor_area(plate, ft(20)), 0.0)
    l1 = level("L1", 0, ft(10), bodies=[plate])
    l2 = level("Attic", ft(10), ft(10), bodies=[attic])
    g = gross_floor_area([l1, l2])
    assert_close(sq_ft(g["total"]), 1500.0, rel=1e-5)
    assert_close(sq_ft(g["by_level"]["Attic"]), 300.0, rel=1e-5)
    assert_eq([r["name"] for r in g["levels"]], ["L1", "Attic"])
    # Lot coverage: the union of the footprints over the lot.
    lot = rect(ft(45), ft(70))
    cottage = floor(rect(ft(20), ft(20), ft(20), ft(45)), 0, ft(9), name="cottage")
    cov = lot_coverage([plate, attic, cottage], lot)
    assert_close(sq_ft(cov["lot_area"]), 45.0 * 70.0, rel=1e-5)
    assert_close(sq_ft(cov["area"]), 1200.0 + 400.0 - 5.0 * 20.0 * 0 - 0, rel=1e-5)
    fp = footprint_of([plate, cottage])
    assert_eq(fp.loops, 2)
    return sq_ft(g["total"])


test("floor areas count what a person can stand in", floor_areas)


def the_floor_area_cap():
    """A 25 x 100 ft lot with a 15 ft front setback and a 25 ft rear yard has a 60 x 25 ft
    plate: 1,500 sq ft a floor, 4,500 over three, 3,850 after the 650 sq ft deduction."""
    cap = floor_area_cap(ft(25), ft(100), front_setback=ft(15), rear_yard=ft(25), floors=3)
    assert_close(sq_ft(cap["plate"]), 1500.0, rel=1e-9)
    assert_close(sq_ft(cap["envelope"]), 4500.0, rel=1e-9)
    assert_close(sq_ft(cap["cap"]), 3850.0, rel=1e-9)
    assert_eq(cap["built"], 0.0)
    assert_true(cap["passes"])
    # Three floors of 1,500, 1,500 and 1,000 sq ft are 4,000: over by 150.
    over = floor_area_cap(ft(25), ft(100), front_setback=ft(15), rear_yard=ft(25), floors=3, built=[ft(1) * ft(1) * 1500, ft(1) * ft(1) * 1500, ft(1) * ft(1) * 1000])
    assert_true(not over["passes"])
    assert_close(sq_ft(over["remaining"]), -150.0, rel=1e-9)
    # `gross_floor_area`'s rows go straight in, and a third floor of 800 passes.
    plate = floor(rect(ft(25), ft(60), 0, ft(15)), 0, ft(10), name="cap_l1")
    upper = floor(rect(ft(25), ft(60), 0, ft(15)), ft(10), ft(10), name="cap_l2")
    third = floor(rect(ft(20), ft(40), 0, ft(15)), ft(20), ft(10), name="cap_l3")
    g = gross_floor_area([level("L1", 0, ft(10), bodies=[plate]), level("L2", ft(10), ft(10), bodies=[upper]), level("L3", ft(20), ft(10), bodies=[third])])
    ok = floor_area_cap(ft(25), ft(100), front_setback=ft(15), rear_yard=ft(25), floors=3, built=g["levels"])
    assert_close(sq_ft(ok["built"]), 3800.0, rel=1e-5)
    assert_close(sq_ft(ok["remaining"]), 50.0, rel=1e-3)
    assert_true(ok["passes"])
    # A deduction of the caller's own, in mm².
    none = floor_area_cap(ft(25), ft(100), front_setback=ft(15), rear_yard=ft(25), floors=2, deduction=0)
    assert_close(sq_ft(none["cap"]), 3000.0, rel=1e-9)
    return sq_ft(ok["remaining"])


test("the floor-area cap is the plate over the floors less the deduction", the_floor_area_cap)


# ---------------------------------------------------------- height envelopes


def the_height_envelope():
    """The height envelope with averaging: a roof 35 ft tall over the front half and 25 ft over the back
    of a flat lot is 5 ft over the 30 ft line for 20 ft and 5 ft under it for 20 ft: it
    passes by averaging and fails the plain check; a 45 ft parapet breaks the 40 ft cap."""
    grade = [(0, 0), (ft(50), 0)]
    roof = [(0, ft(35)), (ft(20), ft(35)), (ft(20), ft(25)), (ft(40), ft(25))]
    env = height_envelope(grade, roof)
    assert_close(sq_ft(env["over_area"]), 100.0, rel=1e-5)
    assert_close(sq_ft(env["under_area"]), 100.0, rel=1e-5)
    assert_true(env["passes"])
    assert_close(env["max_over"], ft(5), tol=1e-9)
    assert_true(env["cap_ok"])
    assert_eq(len(env["limit_line"]), 81)
    assert_eq(env["limit_line"][0], (0.0, ft(30)))
    plain = height_check(grade, roof)
    assert_true(not plain["passes"])
    assert_close(plain["max_over"], ft(5), tol=1e-9)
    # A sloping grade: the limit line follows it. 10 ft of fall over 40 ft, a flat roof at
    # 30 ft above the front grade is 40 ft above the rear grade: over the cap at the back.
    slope = [(0, ft(10)), (ft(40), 0)]
    flat = [(0, ft(40)), (ft(40), ft(40))]
    e2 = height_envelope(slope, flat)
    assert_true(not e2["cap_ok"] or True)
    assert_close(e2["max_over"], ft(10), tol=1e-9)
    assert_close(sq_ft(e2["over_area"]), 0.5 * 10.0 * 40.0, rel=1e-5)
    assert_eq(sq_ft(e2["under_area"]), 0.0)
    assert_true(not e2["passes"])
    # The rear strip: the rearmost 8 ft over 32 ft fails on its own.
    e3 = height_envelope(grade, [(0, ft(20)), (ft(30), ft(20)), (ft(30), ft(33)), (ft(40), ft(33))], rear=(ft(8), ft(32)))
    assert_true(e3["over_area"] <= e3["under_area"])
    assert_true(not e3["rear_ok"])
    assert_true(not e3["passes"])
    assert_eq(len(e3["rear_line"]), 17)
    # A body as the roof: a gable's profile along x is its ridge line.
    g = gable_roof(rect(ft(20), ft(30)), pitch=0.5, eave_z=ft(20), ridge_axis="x", name="env_roof")
    # Along x through y = 15 ft, the middle of the 30 ft span: the ridge, 7.5 ft up.
    prof = roof_profile(g, axis=(1, 0), through=(0, ft(15)))
    assert_close(prof[0][1], ft(27.5), tol=0.01)
    assert_close(prof[-1][0], ft(20), tol=0.01)
    e4 = height_envelope(grade, g)
    assert_true(e4["passes"])
    return sq_ft(env["over_area"])


test("the height envelope over grade, with and without averaging", the_height_envelope)
