# The tests of @cadpole/plan_furniture.star: the catalog is well formed, every set resolves to pieces with
# shapes, and the clearances the codes ask for are the ones the pieces carry. `cadpole test stdlib/plan_furniture_test.star`.
# No solver runs here.

load("@cadpole/plan_furniture.star", "PIECES", "SETS", "SET_FOR_KIND", "SOURCES", "TALL", "FIT_TABLE_KIND", "set_items", "catalog_probe")

def _clear_depth(piece, tag):
    """The depth of the use space named `tag` in a piece's first shape (in)."""
    for (u0, v0, u1, v1, kind, t) in PIECES[piece]["shapes"][0]["rects"]:
        if t == tag:
            return v1 - v0
    return None

def every_piece_is_well_formed():
    n = 0
    for name, pc in PIECES.items():
        assert_true(len(pc["shapes"]) >= 1, msg = name)
        assert_true(pc["h"] > 0, msg = name)
        for src in pc["source"]:
            assert_true(src in SOURCES, msg = "%s cites %s" % (name, src))
        for sh in pc["shapes"]:
            assert_true(sh["w"] > 0 and sh["d"] > 0, msg = name)
            solids = [r for r in sh["rects"] if r[4] == "solid"]
            assert_true(len(solids) >= 1, msg = "%s has no solid" % name)
            for (u0, v0, u1, v1, kind, tag) in sh["rects"]:
                assert_true(u0 < u1 and v0 < v1, msg = "%s %s" % (name, tag))
                assert_true(kind in ("solid", "clear", "slide"), msg = "%s %s" % (name, kind))
                assert_true(u0 >= -1e-9 and u1 <= sh["w"] + 1e-9, msg = "%s %s runs past its length" % (name, tag))
                assert_true(v0 >= -1e-9, msg = "%s %s behind its wall" % (name, tag))
            for pn, (u, v) in sh["points"].items():
                assert_true(u >= 0 and u <= sh["w"], msg = "%s point %s" % (name, pn))
            assert_true(sh["corner"] in (None, "lo", "hi"), msg = name)
            n += 1
    return n

test("every piece has shapes, a height, sources it cites, and rects inside its length", every_piece_is_well_formed)

def every_set_resolves():
    n = 0
    for name, st in SETS.items():
        items = set_items(name)
        assert_eq(len(items), len(st["items"]))
        for it in items:
            assert_true(it["piece"] in PIECES, msg = "%s: %s" % (name, it["piece"]))
            assert_true(it["walls"] == None or it["walls"] == "any" or type(it["walls"]) == "tuple", msg = name)
        assert_true(st["min_d"] > 0, msg = name)
        n += len(items)
    for kind, st in SET_FOR_KIND.items():
        assert_true(st in SETS, msg = kind)
    for dl, st in FIT_TABLE_KIND.items():
        assert_true(st in SETS, msg = dl)
    return n

test("every set resolves to pieces, and every room kind's default set exists", every_set_resolves)

def the_codes_clearances():
    # California: CPC 402.5's 24 in in front of a water closet and a lavatory; the model IRC's 21 in.
    assert_eq(_clear_depth("wc", "wc front"), 24.0)
    assert_eq(_clear_depth("lav", "lav front"), 24.0)
    assert_eq(_clear_depth("wc_irc", "wc front"), 21.0)
    assert_eq(_clear_depth("lav_irc", "lav front"), 21.0)
    # a WC in its 30-in space: 15 in from its centre to a side wall
    assert_eq(PIECES["wc"]["shapes"][0]["w"], 30.0)
    # CPC 408.6: a shower of at least 1,024 sq in that holds a 30-in circle
    for name in ("shower", "shower_60", "shower_36"):
        sh = PIECES[name]["shapes"][0]
        assert_true(sh["w"] * sh["d"] >= 1024.0 and min(sh["w"], sh["d"]) >= 30.0, msg = name)
    # NKBA KPG 6: a 42-in work aisle in front of each kitchen run
    for name in ("sink_run", "range_run"):
        assert_eq(_clear_depth(name, "aisle"), 42.0)
    return True

test("the bath and kitchen pieces carry the codes' clearances", the_codes_clearances)

def tall_means_taller_than_a_sill():
    # closets, shelves and the fridge stay out of the escape-window zone, and nothing else: with TALL as
    # the zone's sill, the heights here say so.
    tall = sorted([n for n, pc in PIECES.items() if pc["h"] > TALL])
    for n in ("reach_in", "bookshelf", "coat_closet", "fridge_run", "linen", "pantry_shelf", "rod_6ft"):
        assert_true(n in tall, msg = n)
    for n in ("bed_full", "bed_queen", "desk", "dresser", "dresser_60", "sleeper_sofa", "media_group", "dining8", "sink_run", "island"):
        assert_true(n not in tall, msg = n)
    return len(tall)

test("tall pieces are the ones kept from under a window", tall_means_taller_than_a_sill)

def the_full_bed_has_three_ways_in():
    shapes = PIECES["bed_full"]["shapes"]
    assert_eq([s["corner"] for s in shapes], [None, "lo", "hi"])
    assert_eq(shapes[0]["w"], 102.0)  # 24 in each side of a 54-in bed
    assert_eq(PIECES["bed_queen"]["shapes"][0]["w"], 120.0)  # 30 in each side of a 60-in queen
    assert_eq(catalog_probe(), (len(PIECES), len(SETS), 3))
    return True

test("the full bed sits centred or in either corner", the_full_bed_has_three_ways_in)
