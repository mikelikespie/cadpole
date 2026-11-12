# The tests of @cadpole/mechanical.star. A document, not a library: it loads
# the library and builds its bodies inside `test()` functions, so
# `cadpole test stdlib/mechanical_test.star` runs them and
# `//src/rust/cadpole-script:cadpole_script_test` (`every_stdlib_test_passes`)
# runs them in CI. The refusals -- a wire thicker than the pitch, a hand that
# is not one, a profile whose flanks do not close -- are Rust tests in
# `src/rust/cadpole-script/src/stdlib.rs`, because a Starlark script cannot
# catch `fail()`.

load(
    "@cadpole/mechanical.star",
    "acme_profile",
    "bolt",
    "iso_profile",
    "minor_diameter",
    "nut",
    "spring",
    "square_profile",
    "thread",
    "thread_depth",
)

SQRT3 = 1.7320508075688772
clearance = param("clearance", 0.15, unit="mm", min=0.05, max=0.18, doc="The nut's clearance the sweep varies (an M8 x 1.25 ISO nut keeps a crest land up to about 0.18 mm)")


def profiles_are_the_standards():
    """ISO 68-1: depth 5H/8, a groove 7p/8 wide at the major diameter and
    p/4 at the bottom, 30 degree flanks; square p/2; ACME 29 degrees."""
    p = 2.0
    iso = iso_profile(p)
    assert_close(iso["depth"], 5.0 * SQRT3 / 2.0 * p / 8.0)
    assert_close(iso["top"], 7.0 * p / 8.0)
    assert_close(iso["bottom"], p / 4.0)
    assert_close(iso["top"] - iso["bottom"], 2.0 * iso["depth"] * math.tan(30))
    assert_close(thread_depth(p), 0.541266 * p, tol=1e-5)
    assert_close(minor_diameter(10, p), 10 - 1.082532 * p, tol=1e-5)
    sq = square_profile(p)
    assert_close(sq["depth"], 1.0)
    assert_close(sq["top"], sq["bottom"])
    ac = acme_profile(p)
    assert_close(ac["top"] - ac["bottom"], 2.0 * ac["depth"] * math.tan(14.5), tol=1e-3)
    return iso["depth"]


test("the profiles are the standards", profiles_are_the_standards)


def spring_volume_is_pi_r_squared_l():
    s = spring(2, 16, 5, 4, name="s")
    assert_solid(s)
    length = 4 * math.hypot(math.pi * 16, 5)
    assert_close(s.volume(), math.pi * length, rel=0.01)
    # Ground ends: flat at the bottom and at the top of the last turn.
    g = spring(2, 16, 5, 4, ends="ground", name="g")
    assert_solid(g)
    lo, hi = g.bbox()
    assert_close(lo[2], 0, tol=0.01)
    assert_close(hi[2], 20, tol=0.01)
    assert_true(g.volume() < s.volume())
    # A left-hand spring is the mirror image in volume.
    l = spring(2, 16, 5, 4, hand=hand.left, name="l")
    assert_close(l.volume(), s.volume(), rel=1e-3)
    return s.volume()


test("a spring is a round wire along its coil, open or ground, either hand", spring_volume_is_pi_r_squared_l)


def threaded_rod_is_between_the_cylinders():
    """The rod's volume lies between the minor and the major cylinders', and
    a square or an ACME groove cuts a rod just the same."""
    rod = thread(8, 1.25, 10, name="rod")
    assert_solid(rod)
    r_minor = minor_diameter(8, 1.25) / 2.0
    v_minor = math.pi * r_minor * r_minor * 10
    v_major = math.pi * 16 * 10
    assert_true(rod.volume() > v_minor and rod.volume() < v_major)
    lo, hi = rod.bbox()
    assert_close(hi[2] - lo[2], 10, tol=1e-6)
    sq = thread(10, 2, 8, profile="square", name="sq")
    assert_solid(sq)
    assert_true(sq.volume() < math.pi * 25 * 8 and sq.volume() > math.pi * 16 * 8)
    ac = thread(10, 2, 8, profile="acme", hand=hand.left, name="ac")
    assert_solid(ac)
    return rod.volume()


test("a threaded rod is cut between its minor and major cylinders, in any profile", threaded_rod_is_between_the_cylinders)


def nut_clears_bolt_by_the_clearance():
    # The chord contract: a mesh sits inside its surface by
    # up to the tolerance and the mesh kernel's cleanup moves a trimmed
    # edge by up to it, so the measured gap is never more than the
    # clearance and within twice the tolerance under it. At the default
    # 0.05 mm this pair measures 0.065 for 0.1; `cadpole test --tolerance
    # 0.01` measures 0.090.
    b = bolt(8, 1.25, 12, 13, 5, name="b")
    n = nut(8, 1.25, 13, 6, clearance=clearance, start=(0, 0, 3), name="n")
    assert_solid(b)
    assert_solid(n)
    gap = distance(b, n)
    assert_true(not gap.overlap)
    assert_true(gap.distance <= clearance + 1e-6, msg="a chorded mesh sits inside its surface")
    assert_true(gap.distance >= clearance - 2.0 * gap.tolerance, msg="the gap is the clearance within the chord contract")
    return gap.distance


test("the nut clears the bolt by the clearance, at every clearance", nut_clears_bolt_by_the_clearance, sweep={"clearance": [0.1, 0.15]})


def bolts_and_nuts_are_chamfered_unless_asked_not_to_be():
    # The 30 degree chamfer turns a hex's corners off at the head's top
    # face and at both faces of a nut; `chamfer=False` keeps the bare hex,
    # whose section is the full hexagon at every height.
    def area(body, z):
        return body.section(plane(origin=(0, 0, z), normal=(0, 0, 1))).area()

    hexagon = 13 * 13 * math.sqrt(3) / 2
    b = bolt(8, 1.25, 12, 13, 5, name="cb")
    n = nut(8, 1.25, 13, 6, start=(0, 0, 3), name="cn")
    assert_solid(b)
    assert_solid(n)
    assert_true(area(b, 16.95) < 0.9 * hexagon, msg="the head's top corners are turned off")
    assert_close(area(b, 12.5), hexagon, rel=0.01, msg="the head's bearing face is the whole hex")
    assert_true(area(n, 3.05) < 0.9 * area(n, 6), msg="the nut's lower corners are turned off")
    assert_true(area(n, 8.95) < 0.9 * area(n, 6), msg="the nut's upper corners are turned off")
    assert_true(area(b, 0.05) < math.pi * 3.4 * 3.4, msg="the tip is chamfered to the thread's root")
    bare = bolt(8, 1.25, 12, 13, 5, chamfer=False, name="bb")
    bare_nut = nut(8, 1.25, 13, 6, start=(0, 0, 3), chamfer=False, name="bn")
    assert_close(area(bare, 16.95), hexagon, rel=0.01, msg="chamfer=False keeps the bare head")
    assert_close(area(bare_nut, 3.05), area(bare_nut, 6), rel=0.02, msg="chamfer=False keeps the bare nut")
    assert_true(distance(b, n).distance > 0, msg="a chamfered nut still runs free on a chamfered bolt")


test("bolts and nuts are chamfered unless asked not to be", bolts_and_nuts_are_chamfered_unless_asked_not_to_be)
