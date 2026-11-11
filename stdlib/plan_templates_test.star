# The tests of @cadpole/plan_templates.star: the four 25-ft partis as data and the transforms that make their
# parameters. No solver runs here; @cadpole/plan_parti.star's precheck reads the data.

load("@cadpole/plan_parti.star", "precheck", "resolve")
load("@cadpole/plan_templates.star", "DOOR_FIXES", "EXTRA_ROOMS", "LIBRARY", "PROGRAM", "PROGRAM_KEYS", "SITE",
     "door_rule_is_vacuous", "door_rules", "floors_only", "knob", "library", "mirror", "optional_court", "program_with",
     "short_columns", "side_hall", "single_floor", "split_column", "with_light_well", "without_court")

def the_program_and_the_library():
    assert_eq(sorted(PROGRAM.keys()), sorted(PROGRAM_KEYS))
    assert_eq(len(PROGRAM_KEYS), 16)
    assert_eq([t["key"] for t in library()], ["side_hall", "cross_stair", "court", "open_core"])
    for t in library():
        keys = [r["key"] for r in t["rooms"]]
        assert_eq(len(keys), len({k: 1 for k in keys}), msg = t["key"] + ": room keys are unique")
        placed = [k for k in PROGRAM_KEYS if k in keys]
        assert_eq(len(placed), 16, msg = t["key"] + ": every program room has a place")
        for r in t["rooms"]:
            for sp in r["spans"]:
                assert_true(0 <= sp[0] and sp[0] <= sp[1] and sp[1] <= len(t["lines"]), msg = "%s %s" % (r["key"], sp))
    assert_eq(sorted(program_with("bed4").keys()), sorted(PROGRAM_KEYS + ["bed4"]))
    return len(library())

test("the program's 16 rooms each have a place in all four partis", the_program_and_the_library)

def mirror_is_an_involution():
    for t in library():
        mm = mirror(mirror(t))
        for a, b in zip(mm["lines"], t["lines"]):
            assert_close(a, b, tol = 1e-9)
        assert_eq([r["spans"] for r in mm["rooms"]], [r["spans"] for r in t["rooms"]])
        assert_eq(mm["stair"], t["stair"])
        assert_eq(mm["stacks"], t["stacks"])
        assert_eq([r["ok"] for r in precheck(mirror(t)) if not r["ok"]], [])
    return 4

test("mirroring twice gives the template back, and a mirror passes the precheck", mirror_is_an_involution)

def the_parameters():
    four = side_hall(floors = 4)
    assert_eq(four["floors"], ["G", "M", "U", "T"])
    assert_eq([r["ok"] for r in precheck(four) if not r["ok"]], [])
    lw = side_hall(light_well = {"side": "E", "y": 18.0, "d": 12.0})
    assert_eq([w["pre"] for w in lw["wells"]], ["well", "lw"])
    nc = side_hall(court = False)
    assert_eq(nc["wells"], [])
    oc = optional_court(side_hall())
    assert_eq([r["optional"] for r in oc["rooms"] if r.get("well") == "well"], [True, True])
    sp = side_hall(split = [(2, 12.0)])
    assert_eq(len(sp["lines"]), 4)
    assert_eq([r["ok"] for r in precheck(sp) if not r["ok"]], [])
    sc = short_columns(side_hall(), cols = [0])
    assert_eq(sc["short"], [("G", 0), ("M", 0), ("U", 0)])
    one = single_floor(side_hall(), "U")
    assert_eq(one["floors"], ["U"])
    assert_eq(one["stair"], None)
    assert_eq([r["ok"] for r in precheck(one) if not r["ok"]], [])
    pm = knob(side_hall(), "primary_at_street")
    assert_eq([o for o in pm["orders"] if o[0] == "U"], [("U", "primary", "bed2")])
    return len(sp["rooms"])

test("mirror, floors, splits, a court, a neighbour's well, short columns and knobs are parameters", the_parameters)

def the_architects_door_rules():
    n = 0
    for k in LIBRARY:
        t = LIBRARY[k]()
        fx = door_rules(t, "all")
        assert_eq(fx["key"], k + "__fix")
        assert_eq(len(fx["doors"]["must"]), 1)
        for w in ("pantry", "garage", "bath", "powder"):
            one = door_rules(t, w)
            assert_eq(one["key"], "%s__only_%s" % (k, w))
            if not door_rule_is_vacuous(t, w):
                n += 1
    # 16 single rules, of which the powder rule changes nothing where the main floor has no hall (court, open core)
    assert_eq(n, 14)
    assert_eq(sorted(DOOR_FIXES.keys()), sorted(LIBRARY.keys()))
    return n

test("an architect's 14 door rules, as template edits", the_architects_door_rules)

def extra_rooms_need_a_place():
    t = resolve(side_hall(), SITE, program_with("bed4"))
    assert_eq(t["unplaced"], [])
    assert_eq(len([r for r in t["rooms"] if r["key"] == "bed4"]), 1)
    t2 = resolve(side_hall(), SITE, program_with({"sauna": EXTRA_ROOMS["study"]}))
    assert_eq(t2["unplaced"], ["sauna"])
    return len(t["rooms"])

test("a program may add the rooms a template has a place for", extra_rooms_need_a_place)

def floors_only_keeps_the_stair_between_them():
    two = floors_only(side_hall(), ["M", "U"])
    assert_eq(two["floors"], ["M", "U"])
    assert_eq(two["stair"], side_hall()["stair"])
    hall = [r for r in two["rooms"] if r["key"] == "hallM"][0]
    assert_eq(hall["parents"], ["stairM"])  # the lowest floor kept: its arrival rooms open off its stair room
    assert_eq([r["ok"] for r in precheck(two) if not r["ok"]], [])
    return len(two["rooms"])

test("floors_only keeps some floors and the stair between them", floors_only_keeps_the_stair_between_them)
