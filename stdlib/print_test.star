# The tests of @cadpole/print.star. A document, not a library: it loads
# the library and builds its pieces inside `test()` functions, so `cadpole test
# stdlib/print_test.star` runs them and `//src/rust/cadpole-script:cadpole_script_test`
# (`every_stdlib_test_passes`) runs them in CI. The refusals -- a peg that
# does not fit, pieces that do not meet, an option that does not exist -- are
# Rust tests in `src/rust/cadpole-script/src/stdlib.rs`, because a Starlark
# script cannot catch `fail()`.

load("@cadpole/print.star", "PEG", "drop_to_bed", "is_box", "peg_sites", "pegs", "snap_fit", "story_bands", "teardrop")

# ----------------------------------------------------------------- helpers

def walls(z0, z1, t, tag):
    """Four walls `t` thick round a 60 x 40 plan, and one across the middle."""
    h = z1 - z0
    return [
        box(60, t, h, at = (0, 0, z0), name = tag + "_s"),
        box(60, t, h, at = (0, 40 - t, z0), name = tag + "_n"),
        box(t, 40 - 2 * t, h, at = (0, t, z0), name = tag + "_w"),
        box(t, 40 - 2 * t, h, at = (60 - t, t, z0), name = tag + "_e"),
        box(t, 40 - 2 * t, h, at = (30, t, z0), name = tag + "_mid"),
    ]

def two_stories(tag, t = 5):
    """A two-story building of boxes, each story joined: a slab and walls to
    20, then a floor plate and walls to 40. The stories meet on z = 20."""
    s0 = union([box(60, 40, 2, name = tag + "_slab0")] + walls(2, 20, t, tag + "_w0"), weld = 0.01, name = tag + "_s0")
    s1 = union([box(60, 40, 4, at = (0, 0, 20), name = tag + "_slab1")] + walls(24, 40, t, tag + "_w1"), weld = 0.01, name = tag + "_s1")
    return s0, s1

# ------------------------------------------------------------------- pegs

def three_pegs_register_two_stories():
    s0, s1 = two_stories("a")
    v0, v1 = s0.volume(), s1.volume()
    lower, upper = pegs(s0, s1, count = 3, name = "a_reg")
    assert_true(print_check(lower).solid, msg = "the lower story with its pegs is a solid")
    assert_true(print_check(upper).solid, msg = "the upper story with its sockets is a solid")
    o = PEG
    h, hs, c = o["size"] / 2.0, o["size"] / 2.0 + o["clearance"], o["chamfer"]
    peg = (2 * h) * (2 * h) * (o["height"] - c) + (2 * h - 2 * c) * (2 * h - 2 * c) * c
    socket = (2 * hs) * (2 * hs) * (o["height"] + o["depth_clearance"])
    assert_close(lower.volume() - v0, 3 * peg, tol = 0.05, msg = "three pegs added")
    assert_close(v1 - upper.volume(), 3 * socket, tol = 0.05, msg = "three sockets cut")
    # The pegs stand on the walls and reach `height` into the story above.
    lo, hi = lower.bbox()
    assert_close(hi[2], 20 + o["height"], tol = 1e-6)

def the_sites_are_on_walls_spread_out_and_deterministic():
    s0, s1 = two_stories("b")
    sites = peg_sites(s0, s1, count = 4)
    assert_eq(len(sites), 4)
    assert_eq(sites, peg_sites(s0, s1, count = 4), msg = "the same pieces give the same sites")
    h = PEG["size"] / 2.0
    for (x, y) in sites:
        # A peg's footprint is inside a 5-thick wall: within 5 of an outer
        # face, or on the middle wall (30..35).
        on_wall = x - h >= 0 and (x + h <= 5 or x - h >= 55 or (x - h >= 30 and x + h <= 35) or y + h <= 5 or y - h >= 35)
        assert_true(on_wall, msg = "site %s is on a wall" % (str((x, y)),))
    # Spread out: the four are the plan's four corners, more or less.
    xs = sorted([p[0] for p in sites])
    ys = sorted([p[1] for p in sites])
    assert_true(xs[-1] - xs[0] > 50 and ys[-1] - ys[0] > 30, msg = "the sites span the plan")

def pegs_go_where_they_are_told_and_round_pins_work():
    s0, s1 = two_stories("c")
    lower, upper = pegs(s0, s1, at = [(2.5, 2.5), (32.5, 20)], shape = "round", size = 2.4, name = "c_reg")
    assert_true(print_check(lower).solid and print_check(upper).solid)
    lo, hi = lower.bbox()
    assert_close(hi[2], 22, tol = 1e-3, msg = "a 2-tall pin over the plane z = 20")

def thin_walls_take_a_smaller_peg():
    # 1.6-thick walls, like a 0.5 ft wall at 1:96: the 3 mm default does
    # not fit on them, a 1.2 mm peg does -- on the middle wall, where the
    # floor plate above has room for a socket and its wall; on an outer wall
    # the socket would come within 0.8 of the plate's edge.
    s0, s1 = two_stories("d", t = 1.6)
    assert_eq(peg_sites(s0, s1, count = 3), [], msg = "a 3 mm peg fits nowhere on a 1.6 mm wall")
    sites = peg_sites(s0, s1, count = 3, size = 1.2, chamfer = 0.2)
    assert_eq(len(sites), 2, msg = "one at each end of the middle wall: " + str(sites))
    for (x, y) in sites:
        assert_true(x > 30 and x < 31.6, msg = "on the middle wall: " + str((x, y)))
    lower, upper = pegs(s0, s1, at = sites, size = 1.2, chamfer = 0.2, name = "d_reg")
    assert_true(print_check(lower).solid and print_check(upper).solid)

def by_filament(tag, t = 5, seam = 32.5):
    """The two stories of `two_stories` as a print kit cuts them, a body per
    filament: below, the slab and the walls; above, the floor plate in two
    colours meeting on x = `seam` (over the middle wall, under a socket
    there), and the walls."""
    slab0 = union(box(60, 40, 2, name = tag + "_slab0"), weld = 0.01, name = tag + "_slab0u")
    walls0 = union(walls(2, 20, t, tag + "_w0"), weld = 0.01, name = tag + "_walls0")
    plate_a = union(box(seam, 40, 4, at = (0, 0, 20), name = tag + "_pa"), weld = 0.01, name = tag + "_plate_a")
    plate_b = union(box(60 - seam, 40, 4, at = (seam, 0, 20), name = tag + "_pb"), weld = 0.01, name = tag + "_plate_b")
    walls1 = union(walls(24, 40, t, tag + "_w1"), weld = 0.01, name = tag + "_walls1")
    return [slab0, walls0], [plate_a, plate_b, walls1]

def pegs_register_pieces_of_several_bodies():
    # A socket over the middle wall straddles the plate's two
    # colours. Judged on one plate it has no wall on the seam's side; judged
    # on the piece -- both plates -- it does, and it is cut from both.
    lower, upper = by_filament("m")
    o = PEG
    hs = o["size"] / 2.0 + o["clearance"]
    site = [(32.5, 20)]
    sites = peg_sites(lower, upper, count = 4)
    assert_eq(len(sites), 4, msg = str(sites))
    assert_true(peg_sites(lower[1], upper[0], count = 4) != sites, msg = "one plate alone is not the piece")
    v_walls, v_slab = lower[1].volume(), lower[0].volume()
    va, vb, vw = upper[0].volume(), upper[1].volume(), upper[2].volume()
    lo2, up2 = pegs(lower, upper, at = site, name = "m_reg")
    assert_eq(len(lo2), 2, msg = "a list in, a list out")
    assert_eq(len(up2), 3)
    h, c = o["size"] / 2.0, o["chamfer"]
    peg = (2 * h) * (2 * h) * (o["height"] - c) + (2 * h - 2 * c) * (2 * h - 2 * c) * c
    assert_close(lo2[1].volume() - v_walls, peg, tol = 0.01, msg = "the peg joins the walls, the body under it")
    assert_close(lo2[0].volume(), v_slab, tol = 1e-9, msg = "the slab takes nothing")
    d = o["height"] + o["depth_clearance"]
    cut_a, cut_b = va - up2[0].volume(), vb - up2[1].volume()
    assert_close(cut_a + cut_b, (2 * hs) * (2 * hs) * d, tol = 0.01, msg = "one socket, cut across both plates")
    assert_true(cut_a > 0 and cut_b > 0, msg = "both plates are cut: %s, %s" % (cut_a, cut_b))
    assert_close(up2[2].volume(), vw, tol = 1e-9, msg = "the walls above are not reached")
    for b in lo2 + up2:
        assert_true(print_check(b).solid, msg = b.name + " is a solid")
    # onto= puts the peg on another body.
    lower, upper = by_filament("n")
    vs = lower[0].volume()
    lo3, _ = pegs(lower, upper, at = site, onto = lower[0], name = "n_reg")
    # Away from the walls the hair it reaches down by (0.003) is its own.
    assert_close(lo3[0].volume() - vs, peg + (2 * h) * (2 * h) * 0.003, tol = 0.002, msg = "onto= puts the peg on the slab body")
    assert_close(lo3[0].bbox()[1][2], 20 + o["height"], tol = 1e-6)

def thin_walls_take_key_columns_in_their_corners():
    # 1.6 mm walls carry no 3 mm peg on their tops; key columns 3.2
    # across in the room corners do, each from the bed to the joint, against
    # both walls -- but not the corner beside the window, where the wall is
    # missing at mid-height.
    def story(tag):
        base = [box(60, 40, 2, name = tag + "_slab0")] + walls(2, 20, 1.6, tag + "_w0")
        # The south wall with a window from x 2 to 10, z 7 to 14.
        base = [b for b in base if b.name != tag + "_w0_s"]
        base += [box(2, 1.6, 18, at = (0, 0, 2), name = tag + "_sw0"), box(50, 1.6, 18, at = (10, 0, 2), name = tag + "_sw1"),
                 box(8, 1.6, 5, at = (2, 0, 2), name = tag + "_sill"), box(8, 1.6, 6, at = (2, 0, 14), name = tag + "_head")]
        s0 = union(base, weld = 0.01, name = tag + "_s0")
        s1 = union([box(60, 40, 4, at = (0, 0, 20), name = tag + "_slab1")] + walls(24, 40, 1.6, tag + "_w1"), weld = 0.01, name = tag + "_s1")
        return s0, s1
    s0, s1 = story("p")
    assert_eq(peg_sites(s0, s1, count = 3), [], msg = "no wall top takes a 3 mm peg")
    # Of the rooms' eight corners, seven have walls all along both sides of
    # a column; the two pairs across the middle wall are 4.8 apart, closer
    # than two sockets and their walls, so one of each pair is kept: five.
    sites = peg_sites(s0, s1, count = 8, column = 3.2)
    assert_eq(len(sites), 5, msg = str(sites))
    for (x, y) in sites:
        assert_true(not (x < 10 and y < 10), msg = "not by the window: " + str((x, y)))
        near = [(cx, cy) for cx in (3.2, 28.4, 33.2, 56.8) for cy in (3.2, 36.8) if abs(x - cx) < 1e-4 and abs(y - cy) < 1e-4]
        assert_eq(len(near), 1, msg = "a column's centre, 1.6 from both walls of a corner: " + str((x, y)))
    v0 = s0.volume()
    three = peg_sites(s0, s1, count = 3, column = 3.2)
    lower, upper = pegs(s0, s1, at = three, column = 3.2, name = "p_reg")
    assert_true(print_check(lower).solid and print_check(upper).solid)
    o = PEG
    h, c = o["size"] / 2.0, o["chamfer"]
    peg = (2 * h) * (2 * h) * (o["height"] - c) + (2 * h - 2 * c) * (2 * h - 2 * c) * c
    column = 3.2 * 3.2 * 18    # over the slab, from 2 to 20
    assert_close(lower.volume() - v0, 3 * (column + peg), tol = 0.05, msg = "three columns, three pegs")
    # Each column stands in its corner: its footprint is in the room, against the walls.
    lo, hi = lower.bbox()
    assert_close(hi[2], 20 + o["height"], tol = 1e-6)

# ------------------------------------------------------------ story bands

def story_bands_cut_join_peg_and_drop():
    # The same two stories as one pile of boxes, cut at 0, 20, 40: two
    # pieces, each one solid, pegged, on the bed. A box crossing a cut by a
    # hair (the post to 20.001) leaves a sliver that is dropped.
    parts = [box(60, 40, 2, name = "e_slab0")] + walls(2, 20, 5, "e_w0") + \
            [box(60, 40, 4, at = (0, 0, 20), name = "e_slab1")] + walls(24, 40, 5, "e_w1") + \
            [box(2, 2, 18.001, at = (10, 10, 2), name = "e_post")]
    out = story_bands(parts, [0, 20, 40], pegs_per_joint = 3, name = "e")
    assert_eq([label for label, _ in out], ["main.0", "main.1"])
    for label, b in out:
        r = print_check(b)
        assert_true(r.solid, msg = label + " is a solid")
        lo, hi = b.bbox()
        assert_close(lo[2], 0, tol = 1e-5, msg = label + " is on the bed")
    # The upper piece has three sockets in its floor: its bottom face has
    # three holes, so its cross-section just above the bed is the plate less
    # three socket squares.
    up = out[1][1]
    hs = PEG["size"] / 2.0 + PEG["clearance"]
    plate = up.section(plane(origin = (0, 0, 0.5))).area()
    assert_close(plate, 60 * 40 - 3 * (2 * hs) * (2 * hs), tol = 1e-3)

def story_bands_scale_and_regions():
    # A wing whose floor is 1 lower than the main block's, as its own
    # stack; everything in feet, printed at 1:96.
    S = 304.8 / 96.0
    main = [box(20, 10, 0.5, name = "f_slab"), box(20, 0.5, 9, at = (0, 0, 0.5), name = "f_wall"),
            box(20, 10, 0.5, at = (0, 0, 9.5), name = "f_slab1"), box(20, 0.5, 9, at = (0, 0, 10), name = "f_wall1")]
    wing = [box(8, 8, 0.5, at = (30, 0, -1), name = "g_slab"), box(8, 0.5, 9, at = (30, 0, -0.5), name = "g_wall")]
    out = story_bands(main + wing, [0, 9.5, 19], regions = [("wing", ((25, -5), (45, 15)), [-1, 8.5])], scale = S, name = "f")
    assert_eq([label for label, _ in out], ["main.0", "main.1", "wing.0"])
    for label, b in out:
        lo, hi = b.bbox()
        assert_close(lo[2], 0, tol = 1e-5, msg = label + " is on the bed (to the float32 of a mesh)")
    lo, hi = out[2][1].bbox()
    assert_close(hi[2] - lo[2], 9.5 * S, tol = 1e-4, msg = "the wing is cut at its own levels")
    assert_close(hi[0] - lo[0], 8 * S, tol = 1e-4, msg = "and scaled to 1:96")

def thin_boxes_are_thickened_to_print():
    # A pane 0.08 ft thick between two piers: at 1:96 it is 0.25 mm, and
    # thicken=0.8 makes it a 0.8 mm wall that joins the piers.
    S = 304.8 / 96.0
    parts = [box(2, 0.5, 8, name = "k_p0"), box(2, 0.5, 8, at = (6, 0, 0), name = "k_p1"),
             box(4, 0.08, 8, at = (2, 0.21, 0), name = "k_pane")]
    out = story_bands(parts, [0, 8], scale = S, thicken = 0.8, name = "k")
    b = out[0][1]
    assert_true(print_check(b).solid)
    pane = 4 * S * 0.8 * 8 * S
    piers = 2 * (2 * S * 0.5 * S * 8 * S)
    assert_close(b.volume(), piers + pane, tol = 0.5, msg = "the pane is 0.8 mm thick, centred where it was")
    thin = story_bands([box(2, 0.5, 8, name = "j_p0"), box(2, 0.5, 8, at = (6, 0, 0), name = "j_p1"),
                        box(4, 0.08, 8, at = (2, 0.21, 0), name = "j_pane")], [0, 8], scale = S, name = "j")
    assert_close(thin[0][1].volume(), piers + 4 * S * 0.08 * S * 8 * S, tol = 0.5, msg = "without it, as drawn")

def the_helpers():
    a = box(1, 2, 3, at = (4, 5, 6), name = "h_box")
    assert_true(is_box(a))
    assert_true(not is_box(cylinder(1, 2, name = "h_cyl")))
    d = drop_to_bed(a, name = "h_bed")
    lo, hi = d.bbox()
    assert_close(lo, (4, 5, 0), tol = 1e-9)

def block_with_hole(tag, d, tear):
    """A 30 x 12 x 24 block on the bed with a horizontal hole `d` across,
    along y, 12 up: round, or teardropped."""
    blk = box(30, 12, 24, at = (-15, -6, 0), name = tag + "_blk")
    if tear:
        tool = teardrop(d, 16, at = (0, 0, 12), name = tag + "_tool")
    else:
        tool = rotate(cylinder(d / 2.0, 16, at = (0, 0, -8), name = tag + "_cyl"), 90, axis = (1, 0, 0), name = tag + "_tool0")
        tool = translate(tool, (0, 0, 12), name = tag + "_tool")
    return subtract(blk, tool, name = tag)

def a_teardrop_hole_prints_without_support_and_a_round_one_does_not():
    round_ = block_with_hole("sf_round", 10, False)
    r = print_check(round_)
    assert_true(not r.support_free, msg = "a round 10 mm hole lying down needs its top supported")
    assert_eq(r.wide_holes, 1)
    assert_true(len([s for s in r.support if s.startswith("[horizontal_hole]")]) == 1, msg = str(r.support))
    # With a printer that bridges wider holes, it passes.
    assert_support_free(round_, max_horizontal_hole = 12)
    tear = block_with_hole("sf_tear", 10, True)
    assert_support_free(tear)
    assert_true(print_check(tear).support_free)
    # The teardrop is the full circle below its peak.
    lo, hi = teardrop(10, 16, name = "sf_probe").bbox()
    assert_close(lo[2], -5, tol = 1e-6)
    assert_close(hi[2], 5 * math.sqrt(2.0), tol = 1e-6)
    # A small round hole is bridged as it is.
    assert_support_free(block_with_hole("sf_small", 5, False))

def a_hanging_finger_is_an_island_and_a_ledge_within_reach_is_not():
    post = box(4, 4, 30, name = "sf_post")
    arm = box(14, 4, 4, at = (4, 0, 26), name = "sf_arm")
    finger = box(2, 4, 8, at = (14, 0, 18), name = "sf_finger")
    hook = union(post, arm, finger, name = "sf_hook")
    r = print_check(hook)
    assert_eq(r.islands, 1)
    assert_close(r.deepest_island, 8, tol = 1e-6)
    assert_true(len([s for s in r.support if s.startswith("[island]")]) == 1, msg = str(r.support))
    # A lip sticking out 2 mm over the wall under it is a ledge within reach.
    shelf = union(box(10, 10, 20, name = "sf_wall"), box(12, 10, 2, at = (0, 0, 20), name = "sf_lip"), name = "sf_shelf")
    assert_support_free(shelf)
    assert_eq(print_check(shelf).islands, 0)
    long = union(box(10, 10, 20, name = "sf_wall2"), box(16, 10, 2, at = (0, 0, 20), name = "sf_lip2"), name = "sf_long")
    lr = print_check(long)
    assert_true(not lr.support_free)
    assert_close(lr.longest_cantilever, 6, tol = 1e-6)
    assert_support_free(long, max_cantilever = 6.5)

def a_list_is_one_print_and_a_body_in_the_air_needs_support():
    base = box(20, 20, 4, name = "sf_base")
    lid = box(20, 20, 2, at = (0, 0, 9), name = "sf_lid")
    assert_support_free([base])
    r = print_check([base, lid])
    assert_true(not r[1].support_free or not r[0].support_free, msg = "the lid floats 5 mm over the base")

test("assert_support_free: a 10 mm teardrop hole passes, a round one fails and passes with max_horizontal_hole", a_teardrop_hole_prints_without_support_and_a_round_one_does_not)
test("assert_support_free: a hanging finger is an island; a ledge within max_cantilever passes, a longer one fails", a_hanging_finger_is_an_island_and_a_ledge_within_reach_is_not)
test("print_check on a list: a body in the air is a support finding", a_list_is_one_print_and_a_body_in_the_air_needs_support)

def snap_fit_strain():
    # A uniform arm is the textbook 1.5 t y / L^2, peaking at the root.
    s = snap_fit(10.0, 1.2, 0.7)
    assert_close(s["strain"], 1.5 * 1.2 * 0.7 / 100.0, tol = 1e-6)
    assert_close(s["factor"], 1.0, tol = 1e-4)
    assert_eq(s["at"], 0.0)
    # Halving the thickness toward the load point, or tapering the width to
    # nothing, spreads the bend: about 0.61 and exactly 2/3 of it.
    assert_close(snap_fit(10.0, 1.2, 0.7, thickness_tip = 0.6)["factor"], 0.611, tol = 2e-3)
    assert_close(snap_fit(10.0, 1.2, 0.7, width = 6.0, width_tip = 0.0)["factor"], 2.0 / 3.0, tol = 1e-3)
    # Strain scales with t y / L^2; the force with E b t^3 y / (4 L^3).
    assert_close(snap_fit(20.0, 1.2, 2.8)["strain"], s["strain"], tol = 1e-9)
    f = snap_fit(10.0, 1.2, 0.7, width = 6.0, modulus = 3500.0)
    assert_close(f["force"], 3500.0 * 6.0 * 1.2 * 1.2 * 1.2 * 0.7 / (4 * 1000.0), tol = 1e-3)
    assert_close(f["stiffness"] * 0.7, f["force"], tol = 1e-9)
    assert_true(snap_fit(10.0, 1.2, 0.7, max_strain = 0.02)["strain"] < 0.02)

test("three pegs register two stories: solids, the pegs' and sockets' volumes, the pegs' height", three_pegs_register_two_stories)
test("peg_sites finds places on the walls, spread over the plan, the same every time", the_sites_are_on_walls_spread_out_and_deterministic)
test("pegs go where at= says; round pins", pegs_go_where_they_are_told_and_round_pins_work)
test("a 3 mm peg fits nowhere on a 1.6 mm wall, a 1.2 mm one does where its socket has a wall", thin_walls_take_a_smaller_peg)
test("pegs and peg_sites take a body per filament: judged on their union, the peg on the body under it, the socket cut from every body it reaches", pegs_register_pieces_of_several_bodies)
test("key columns in the room corners carry pegs the thin walls cannot, but not beside a window", thin_walls_take_key_columns_in_their_corners)
test("story_bands cuts, drops a sliver, joins, pegs and puts every piece on the bed", story_bands_cut_join_peg_and_drop)
test("story_bands scales, and a region is cut at its own levels as its own stack", story_bands_scale_and_regions)
test("thicken= widens boxes thinner than a printable wall, in plan, about their centre line", thin_boxes_are_thickened_to_print)
test("is_box and drop_to_bed", the_helpers)
test("snap_fit: the textbook strain for a uniform arm, less for a tapered one, and the force", snap_fit_strain)
