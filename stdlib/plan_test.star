# Tests for @cadpole/plan.star: the plan value's constructors and their refusals, validate() on a clean plan
# and on one broken every way it looks for, the measures, shared walls across a 4.8-in partition, neighbours, the
# door graph and its paths (outside and the stair core included), exterior walls against the plate and derived,
# room_local's frame, placing doors and windows, and feet-inches. Synthetic plans only: the module's toy_house(), a
# two-floor 25 x 60 ft house with a stair core, an L-shaped kitchen, a bath and outside doors, and a one-floor plan
# with a court. `cadpole test stdlib/plan_test.star`.

load(
    "@cadpole/plan.star",
    "TOL",
    "area",
    "bbox",
    "clear_width",
    "check_record",
    "court",
    "depth",
    "dims",
    "door",
    "door_between",
    "door_graph",
    "door_path",
    "exterior_walls",
    "fixture",
    "floor",
    "ftin",
    "get_room",
    "neighbours",
    "normalize",
    "plan",
    "room",
    "room_local",
    "rooms_on",
    "rooms_crossed",
    "plan_diff",
    "shared_walls",
    "size_text",
    "sqft",
    "stack",
    "toy_house",
    "validate",
    "validate_rows",
    "walk_length",
    "width",
    "window",
    "window_on",
)

T = toy_house()

def _has(msgs, *parts):
    """Whether one message holds every part."""
    for m in msgs:
        if all([p in m for p in parts]):
            return True
    return False

def _expect(msgs, *parts):
    assert_true(_has(msgs, *parts), msg = "no problem says %s; got:\n  %s" % (" ... ".join(parts), "\n  ".join(msgs)))

def constructors_fill_defaults():
    bed = room("k1", floor = "L1", kind = "bed", rect = [0, 10, 0, 12])
    assert_eq(bed["rects"], [(0.0, 10.0, 0.0, 12.0)])
    assert_eq(type(bed["rects"][0][1]), "float")
    assert_eq([bed["name"], bed["unit"], bed["habitable"], bed["sleeping"], bed["kept"]], ["k1", "main", True, True, False])
    kit = room("k", floor = "L1", kind = "kitchen", rect = (0, 10, 0, 12))
    assert_eq([kit["habitable"], kit["sleeping"]], [True, False])
    clo = room("c", floor = "L1", kind = "closet", rect = (0, 3, 0, 2))
    assert_eq([clo["habitable"], clo["sleeping"]], [False, False])
    guest = room("g", floor = "L1", kind = "guest", rect = (0, 10, 0, 10), unit = "unit2")
    assert_eq([guest["habitable"], guest["sleeping"], guest["unit"]], [True, True, "unit2"])
    den = room("d", floor = "L1", kind = "den", rect = (0, 10, 0, 10), habitable = False, label = "Den / guest")
    assert_eq([den["habitable"], den["label"]], [False, "Den / guest"])
    ell = room("l", floor = "L1", kind = "living", rects = [(0, 10, 0, 10), (10, 14, 6, 10)])
    assert_eq(len(ell["rects"]), 2)
    d = door("d1", a = "k1", b = "outside", wall = "S", at = (5, 0))
    assert_eq([d["floor"], d["width"], d["swing"], d["hinge"], d["span"]], [None, 2.67, "a", "lo", None])
    w = window("w1", room = "k1", wall = "W", at = (0, 6))
    assert_eq([w["width"], w["sill"], w["head"], w["source"], w["egress"]], [3.0, 2.0, 7.0, "side", False])
    p = plan("t", floors = [floor("L1")], rooms = [bed], doors = [d], windows = [w])
    assert_eq(p["unit"], "ft")
    assert_eq(p["doors"][0]["floor"], "L1")  # from room a
    assert_eq(p["windows"][0]["floor"], "L1")  # from its room
    assert_eq(p["floors"][0]["ceiling"], 8.0)
    assert_eq(validate(p), [])
    return len(p["rooms"])

test("the constructors fill the defaults by kind", constructors_fill_defaults)

def refusals_name_the_record():
    ms = check_record("room", {"key": "x", "floor": "L1", "kind": "attic", "rects": [(0, 1, 0, 1)]})
    _expect(ms, 'room "x"', 'unknown kind "attic"', "the kinds are living, dining")
    ms = check_record("room", {"key": "x", "floor": "L1", "kind": "bed", "rects": [(5, 1, 0, 1)]})
    _expect(ms, 'room "x" rect 0', "wrong order", "u0 5 is not less than u1 1")
    ms = check_record("room", {"key": "x", "floor": "L1", "kind": "bed", "rects": [(0, 1, 3, 3)]})
    _expect(ms, 'room "x" rect 0', "wrong order", "v0 3 is not less than v1 3")
    ms = check_record("room", {"key": "x", "floor": "L1", "kind": "bed", "rects": [(0, 1, 3)]})
    _expect(ms, 'room "x" rect 0', "4 numbers")
    ms = check_record("room", {"key": "x", "floor": "L1", "kind": "bed", "rects": []})
    _expect(ms, 'room "x"', "one or more rects")
    _expect(check_record("door", {"key": "d", "floor": "L1", "a": "x", "b": "y", "wall": "E", "at": (1, 1), "swing": "out"}), 'door "d"', "swing is")
    _expect(check_record("door", {"key": "d", "floor": "L1", "a": "x", "b": "x", "wall": "Q", "at": (1, 1)}), 'door "d"', "same room")
    _expect(check_record("door", {"key": "d", "floor": "L1", "a": "x", "b": "y", "wall": "Q", "at": (1, 1)}), 'door "d"', "wall is the wall of room a")
    _expect(check_record("window", {"key": "w", "floor": "L1", "room": "x", "wall": "N", "at": (1, 1), "source": "garden"}), 'window "w"', "source is")
    _expect(check_record("window", {"key": "w", "floor": "L1", "room": "x", "wall": "N", "at": (1, 1), "sill": 7.0, "head": 6.0}), 'window "w"', "head 6 is not above sill 7")
    _expect(check_record("court", {"key": "c", "kind": "atrium", "rect": (0, 1, 0, 1), "base": "L1", "top": 30}), 'court "c"', 'kind is "court"')
    _expect(check_record("stack", {"key": "s", "at": (1, 1), "floors": []}), 'stack "s"', "floors is a list")
    _expect(check_record("fixture", {"key": "f", "room": "r", "kind": "wc", "rect": (3, 2, 0, 1)}), 'fixture "f"', "wrong order")
    _expect(check_record("wall", {"floor": "L1", "a": (0, 0), "b": (0, 0)}), "wall on L1", "same point")
    _expect(check_record("floor", {"key": "L1", "z": 0, "ceiling": 8, "plate": [(0, 10, 9, 1)]}), 'floor "L1" plate rect 0', "wrong order")
    _expect(check_record("site", {"lot": (0, 25, 100, 0), "street": "S"}), "site lot", "wrong order")
    assert_eq(check_record("room", get_room(T, "kitchen")), [])
    return 0

test("check_record says what is wrong with a record, naming it", refusals_name_the_record)

def the_toy_house_is_clean():
    assert_eq(validate(T), [])
    assert_eq(validate_rows(T), [])
    assert_eq(len(T["floors"]), 2)
    assert_eq(len(T["rooms"]), 16)
    assert_eq([r["key"] for r in rooms_on(T, "L2", kinds = ["bed"])], ["bed1", "bed2", "primary"])
    assert_eq([r["key"] for r in rooms_on(T, "L1", kinds = "wet")], ["powder", "kitchen"])
    return len(T["doors"])

test("the toy house validates clean", the_toy_house_is_clean)

def _broken():
    t = toy_house()
    rooms = list(t["rooms"]) + [
        {"key": "bad", "name": "Bad", "floor": "L1", "kind": "storage", "rects": [(30.0, 26.0, 0.0, 5.0)]},
        room("nook", floor = "L1", kind = "den", rect = (10.0, 13.0, 20.0, 27.0)),
        room("lost", floor = "L3", kind = "den", rect = (0.0, 5.0, 0.0, 5.0)),
        room("split", floor = "L1", kind = "storage", rects = [(30.0, 32.0, 0.0, 2.0), (35.0, 37.0, 0.0, 2.0)]),
        room("bed1", floor = "L2", kind = "bed", rect = (40.0, 45.0, 0.0, 5.0)),
    ]
    doors = list(t["doors"]) + [
        door("d_far", floor = "L1", a = "living", b = "family", wall = "N", at = (7.0, 28.0)),
        door("d_long", floor = "L1", a = "entry", b = "hall1", wall = "N", at = (19.0, 18.0), width = 3.0),
        door("d_ghost", floor = "L1", a = "entry", b = "ghost", wall = "E", at = (24.5, 14.0)),
        door("d_shut", floor = "L1", a = "hall1", b = "outside", wall = "W", at = (14.4, 30.0)),
    ]
    windows = list(t["windows"]) + [
        window("w_inner", floor = "L1", room = "hall1", wall = "W", at = (14.4, 30.0), source = "side"),
        window("w_nowhere", floor = "L1", room = "attic", wall = "N", at = (1.0, 1.0)),
        window("w_court", floor = "L2", room = "bed2", wall = "W", at = (0.5, 28.0), source = "court:lw"),
    ]
    fixtures = list(t["fixtures"]) + [
        fixture("f_lost", room = "garage", kind = "sink", rect = (0, 1, 0, 1)),
        fixture("f_out", room = "powder", kind = "lav", rect = (18.0, 21.0, 31.0, 32.0)),
    ]
    stacks = list(t["stacks"]) + [stack("s_bad", at = (1.0, 1.0), floors = ["L1", "L3"])]
    return dict(t, rooms = rooms, doors = doors, windows = windows, fixtures = fixtures, stacks = stacks)

def validate_finds_each_problem():
    ms = validate(_broken())
    _expect(ms, 'room "bad" rect 0', "wrong order", "u0 30 is not less than u1 26")
    _expect(ms, 'rooms "living" and "nook" overlap on L1', "3'-0\" x 7'-0\"", "21.0 sq ft")
    _expect(ms, 'room "lost" is on floor "L3"', "does not have")
    _expect(ms, 'room "split"', "do not all reach")
    _expect(ms, 'room key "bed1" is used by two rooms')
    _expect(ms, 'door "d_far"', 'rooms "living" and "family" share no wall')
    _expect(ms, 'door "d_long"', "is not on the wall", '"entry" shares with "hall1"', "14.4..19.6")
    _expect(ms, 'door "d_ghost"', 'room b "ghost" is not in the plan')
    _expect(ms, 'door "d_shut" to outside', 'not on an exterior wall of "hall1"')
    _expect(ms, 'window "w_inner"', 'not on an exterior wall of "hall1"\'s W side')
    _expect(ms, 'window "w_nowhere"', 'its room "attic" is not in the plan')
    _expect(ms, 'window "w_court"', 'looks onto court:lw', 'no court "lw"')
    _expect(ms, 'fixture "f_lost"', 'room "garage" is not in the plan')
    _expect(ms, 'fixture "f_out"', 'runs outside room "powder"')
    _expect(ms, 'stack "s_bad" passes floor "L3"', "does not have")
    rows = validate_rows(_broken())
    assert_eq(len(rows), len(ms))
    for r in rows:
        assert_eq(r["ok"], False)
        assert_eq(r["rule"], "plan")
        assert_true(r["severity"] in ["fail", "warn", "note"])
        assert_true(type(r["detail"]) == "string" and r["detail"] != "")
    over = [r for r in rows if r["key"] == "plan:overlap:living:nook"][0]
    assert_close(over["rect"], (10.0, 13.0, 20.0, 27.0))
    assert_eq(over["floor"], "L1")
    far = [r for r in rows if r["key"] == "plan:door:d_far"][0]
    assert_close(far["at"], (7.0, 28.0))
    return len(ms)

test("validate finds every kind of problem, and says where", validate_finds_each_problem)

def areas_dims_and_bbox():
    k = get_room(T, "kitchen")
    assert_close(area(k), 13.5 * 21.6 + 10.5 * 9.6, tol = 1e-9)
    assert_close(dims(k), (13.5, 21.6), tol = 1e-9)
    assert_close(width(k), 13.5, tol = 1e-9)
    assert_close(depth(k), 21.6, tol = 1e-9)
    assert_close(bbox(k), (0.5, 24.5, 28.4, 50.0))
    assert_close(area((0, 2, 0, 3)), 6.0)
    assert_close(area([(0, 2, 0, 3), (1, 3, 0, 3)]), 9.0)  # a union: the overlap counts once
    assert_close(area(T, floor = "L1"), 1368.72, tol = 1e-6)
    assert_close(area(T, floor = "L2"), 1365.12, tol = 1e-6)
    assert_close(area(T, kinds = ["bed"]), 155.25 + 179.4 + 326.4, tol = 1e-6)
    assert_close(area(T, floor = "L2", kinds = "outdoor"), 362.4, tol = 1e-6)
    assert_close(bbox(T, floor = "L2"), (0.5, 24.5, 10.5, 69.5))
    assert_eq(size_text(k), "13'-6\" x 21'-7\"")
    return area(k)

test("area in sq ft, the main rect's width and depth, and bboxes", areas_dims_and_bbox)

def walls_across_a_partition():
    sw = shared_walls(T, "living", "hall1")
    assert_eq(len(sw), 1)
    w = sw[0]
    assert_eq(w["wall"], "E")
    assert_close([w["line"], w["far"], w["gap"], w["length"]], [14.0, 14.4, 0.4, 9.6], tol = 1e-9)
    assert_close(w["span"], (18.4, 28.0), tol = 1e-9)
    two = shared_walls(T, "kitchen", "hall1")  # the L's main rect on the hall's west, its second rect on the hall's north
    assert_eq([(x["wall"], x["a_rect"]) for x in two], [("E", 0), ("S", 1)])
    assert_close([x["length"] for x in two], [11.6, 5.2], tol = 1e-9)
    assert_eq(shared_walls(T, "living", "family"), [])
    assert_eq(shared_walls(T, "bed1", "living"), [])  # different floors
    # a corner that only touches is not a wall
    a = room("a", floor = "G", kind = "den", rect = (0, 10, 0, 10))
    b = room("b", floor = "G", kind = "den", rect = (10.4, 20, 10.4, 20))
    assert_eq(shared_walls(None, a, b), [])
    # a 1'-2" wall is past the default 1'-0"; gap= takes it
    c = room("c", floor = "G", kind = "den", rect = (11.2, 20, 0, 10))
    assert_eq(shared_walls(None, a, c), [])
    assert_eq([x["wall"] for x in shared_walls(None, a, c, gap = 1.5)], ["E"])
    assert_eq(neighbours(T, "hall1"), ["living", "entry", "stair1", "powder", "closet1", "kitchen"])
    assert_eq(neighbours(T, "entry"), ["living", "hall1", "stair1"])
    assert_eq(neighbours(T, "primary"), ["hall2", "laundry", "bed2", "deck"])
    return len(neighbours(T, "hall1"))

test("shared walls across a 4.8-in partition, and neighbours", walls_across_a_partition)

def doors_as_a_graph():
    g = door_graph(T)
    assert_eq(sorted([e["to"] for e in g["outside"]]), ["entry", "family"])
    assert_eq([e["via"] for e in g["stair1"] if e["to"] == "stair2"], ["core:st"])
    assert_eq(door_path(T, "bed1", "outside"), ["bed1", "hall2", "stair2", "stair1", "hall1", "entry", "outside"])
    assert_eq(door_path(T, "kitchen", "kitchen"), ["kitchen"])
    assert_eq(door_path(T, "powder", "living", avoid = ["entry"]), ["powder", "hall1", "kitchen", "living"])
    assert_eq(door_path(T, "family", "entry", avoid_kinds = ["kitchen"]), None)
    assert_eq(door_path(T, "family", "entry", avoid_kinds = ["kitchen"], through_outside = True), ["family", "outside", "entry"])
    assert_eq(door_path(T, "bed1", "family", cores = False), None)
    steps = door_path(T, "bed2", "living", edges = True)
    assert_eq([s["door"] for s in steps], ["d_bed2", "d_stair2", None, "d_stair1", "d_entry_hall", "d_entry_living"])
    assert_eq(steps[2]["via"], "core:st")
    one = door_graph(T, floor = "L2")
    assert_true("stair1" not in one)
    assert_eq(sorted([e["to"] for e in one["primary"]]), ["deck", "hall2"])
    return len(g)

test("the door graph, outside and the stair core included, and paths that avoid rooms", doors_as_a_graph)

def exterior_walls_by_plate_and_derived():
    def brief(k, mode = None):
        return [(x["rect"], x["wall"], x["faces"], (math.round(x["span"][0], 3), math.round(x["span"][1], 3))) for x in exterior_walls(T, k, mode = mode)]
    assert_eq(brief("living"), [(0, "S", "street", (0.5, 14.0)), (0, "W", "side", (10.5, 28.0))])
    assert_eq(brief("kitchen"), [(0, "W", "side", (28.4, 50.0)), (1, "E", "side", (40.4, 50.0))])
    assert_eq(brief("primary"), [(0, "N", "deck:deck", (0.5, 24.5)), (0, "E", "side", (40.4, 54.0)), (0, "W", "side", (40.4, 54.0))])
    assert_eq(brief("hall1"), [])
    for k in ["living", "kitchen", "primary", "hall1", "bed1", "bath", "family", "entry", "stair2"]:
        assert_eq(brief(k, "derived"), brief(k, "plate"), msg = k)
    return len(brief("primary"))

test("exterior walls against the floor plate and derived from the rooms agree", exterior_walls_by_plate_and_derived)

def a_room_in_its_own_frame():
    k = room_local(T, "kitchen")
    assert_close(k["origin"], (0.5, 28.4))
    assert_close([k["w"], k["d"], k["ceiling"]], [13.5, 21.6, 9.0], tol = 1e-9)
    assert_close(k["rects"], [(0.0, 13.5, 0.0, 21.6), (13.5, 24.0, 12.0, 21.6)], tol = 1e-9)
    assert_eq([(d["key"], d["wall"], d["rect"], d["to"], d["swing"]) for d in k["doors"]], [
        ("d_hall_kitchen", "S", 1, "hall1", "open"),
        ("d_living_kitchen", "S", 0, "living", "open"),
        ("d_kitchen_family", "N", 0, "family", "open"),
    ])
    assert_close([d["offset"] for d in k["doors"]], [3.0, 6.75, 6.75], tol = 1e-9)  # the opening's centre from its wall's start
    hk = k["doors"][0]
    assert_close([hk["lo"], hk["hi"], hk["width"]], [1.5, 4.5, 3.0], tol = 1e-9)
    assert_close(hk["point"], (16.5, 11.6), tol = 1e-9)
    assert_eq([(w["key"], w["wall"]) for w in k["windows"]], [("w_kitchen", "W")])
    assert_close(k["windows"][0]["offset"], 10.6, tol = 1e-9)
    assert_close(k["fixtures"][0]["rect"], (0.0, 2.0, 12.6, 15.6), tol = 1e-9)
    assert_close(k["stacks"], [(-0.3, 13.6)], tol = 1e-9)
    b = room_local(T, "bed1")
    assert_eq([b["kind"], b["furnish"], b["ceiling"]], ["bed", "queen", 8.5])
    d = b["doors"][0]
    assert_eq([d["key"], d["wall"], d["swing"], d["hinge"], d["to"]], ["d_bed1", "E", "in", "hi", "hall2"])
    assert_close([d["offset"], d["width"], d["lo"], d["hi"]], [10.7, 2.67, 10.7 - 1.335, 10.7 + 1.335], tol = 1e-9)
    w = b["windows"][0]
    assert_eq([w["wall"], w["egress"], w["source"]], ["S", True, "street"])
    assert_close([w["offset"], w["width"], w["sill"], w["head"]], [5.75, 4.0, 2.0, 7.0], tol = 1e-9)
    assert_eq(b["stacks"], [])
    h = room_local(T, "hall2")
    prim = [x for x in h["doors"] if x["key"] == "d_primary"][0]
    assert_eq([prim["wall"], prim["swing"], prim["to"]], ["N", "out", "primary"])  # room b's side: the leaf swings into a
    assert_close(prim["offset"], 3.6, tol = 1e-9)
    assert_eq(h["windows"], [])  # the skylight is not in a wall
    ba = room_local(T, "bath")
    assert_close(ba["stacks"], [(12.3, 5.5)], tol = 1e-9)
    assert_eq([f["kind"] for f in ba["fixtures"]], ["tub", "wc", "lav"])
    assert_close(ba["fixtures"][1]["rect"], (9.6, 12.1, 2.5, 4.0), tol = 1e-9)
    c = room_local(T, "closet1")
    assert_eq(c["doors"][0]["swing"], "out")
    assert_eq([x["swing"] for x in room_local(T, "hall1")["doors"] if x["key"] == "d_closet1"], ["in"])
    return len(k["doors"])

test("room_local: a room in its own frame, doors at their centres", a_room_in_its_own_frame)

def placing_doors_and_windows():
    d = door_between(T, "dx", "bed2", "hall2")
    assert_eq([d["wall"], d["floor"], d["a"], d["b"]], ["E", "L2", "bed2", "hall2"])
    assert_close(d["at"], (12.0, 32.2), tol = 1e-9)
    y = door_between(T, "dy", "family", "outside:yard", wall = "N", along = 20.0)
    assert_close(y["at"], (20.0, 69.5))
    w = window_on(T, "wx", "bed2", "W")
    assert_close(w["at"], (0.5, 32.2), tol = 1e-9)
    assert_eq(w["source"], "side")
    assert_eq(window_on(T, "wy", "primary", "N")["source"], "deck")
    t2 = dict(T, doors = list(T["doors"]) + [d, y], windows = list(T["windows"]) + [w])
    assert_eq(validate(t2), [])
    return 0

test("door_between and window_on put openings on walls the plan has", placing_doors_and_windows)

def a_court_names_the_walls_that_face_it():
    p = plan(
        "court_toy",
        floors = [floor("G", z = 0.0, ceiling = 9.0, plate = [(0.5, 19.5, 0.5, 14.5), (0.5, 12.0, 14.5, 29.5)])],
        rooms = [
            room("front", floor = "G", kind = "living", rect = (0.5, 19.5, 0.5, 14.5)),
            room("back", floor = "G", kind = "bed", rect = (0.5, 12.0, 14.9, 29.5)),
        ],
        courts = [court("c1", kind = "court", rect = (12.5, 20.0, 15.0, 30.0), base = "G", top = 10.0, reaches = "rear")],
        windows = [
            window("w_back", room = "back", wall = "E", at = (12.0, 22.0), source = "court:c1", egress = True),
            window("w_front", room = "front", wall = "N", at = (16.0, 14.5), source = "court:c1"),
        ],
        doors = [door("d1", a = "front", b = "back", wall = "N", at = (6.0, 14.5))],
    )
    assert_eq(validate(p), [])
    back = [(x["wall"], x["faces"]) for x in exterior_walls(p, "back")]
    assert_eq(back, [("N", "rear"), ("E", "court:c1"), ("W", "side")])
    fr = [x for x in exterior_walls(p, "front") if x["wall"] == "N"]
    assert_eq([(x["faces"], math.round(x["span"][0], 3), math.round(x["span"][1], 3)) for x in fr], [("court:c1", 12.0, 19.5)])
    bad = dict(p, windows = [window("w_s", floor = "G", room = "front", wall = "S", at = (6.0, 0.5), source = "court:c1")])
    _expect(validate(bad), 'window "w_s" says it looks onto court:c1', "faces street")
    return len(back)

test("a court: the walls that face it say so, and a window's source is checked against it", a_court_names_the_walls_that_face_it)

def a_plan_built_by_hand():
    h = {
        "name": "by_hand",
        "floors": [{"key": "G"}],
        "rooms": [
            {"key": "a", "floor": "G", "kind": "bed", "rects": [[0, 10, 0, 10]]},
            {"key": "b", "floor": "G", "kind": "hall", "rects": [(10.4, 14, 0, 10)]},
        ],
        "doors": [{"key": "d", "a": "a", "b": "b", "wall": "E", "at": (10.0, 5.0)}],
    }
    n = normalize(h)
    assert_eq(n["unit"], "ft")
    assert_eq([n["rooms"][0]["habitable"], n["rooms"][0]["sleeping"], n["rooms"][0]["unit"]], [True, True, "main"])
    assert_eq(n["rooms"][1]["habitable"], False)
    assert_eq(n["rooms"][0]["rects"], [(0.0, 10.0, 0.0, 10.0)])
    assert_eq([n["doors"][0]["floor"], n["doors"][0]["swing"], n["doors"][0]["width"]], ["G", "a", 2.67])
    assert_eq([n["floors"][0]["z"], n["floors"][0]["ceiling"]], [0.0, 8.0])
    assert_eq(validate(n), [])
    assert_eq(n["windows"], [])
    _expect(validate(h), 'door "d": floor is a non-empty string')  # unnormalized, the door has no floor
    return len(n["rooms"])

test("normalize fills a plan built by hand the way the constructors would", a_plan_built_by_hand)

def feet_and_inches():
    assert_eq(ftin(2.5), "2'-6\"")
    assert_eq(ftin(2.0 + 11.9375 / 12.0, 16), "2'-11 15/16\"")
    assert_eq(ftin(0.0), "0'-0\"")
    assert_eq(ftin(11.999), "12'-0\"")
    assert_eq(ftin(-0.5), "-0'-6\"")
    assert_eq(ftin(3.0 + 0.5 / 12.0, 16), "3'-0 1/2\"")
    assert_eq(ftin(1.0 / 192.0, 16), "0'-0 1/16\"")
    assert_eq(ftin(10.25), "10'-3\"")
    assert_eq(ftin(0.52, 8), "0'-6 1/4\"")
    assert_eq(sqft(1234.4), "1,234 sq ft")
    assert_eq(sqft(52.26, 1), "52.3 sq ft")
    assert_eq(TOL, 0.05)
    return 0

test("ftin and sqft write imperial", feet_and_inches)

def walls_that_run_on_and_cores_that_land():
    # an L whose two rects share one face: a door across both pieces is on one wall
    a = room("ell", floor = "G", kind = "foyer", rects = [(0, 10, 3, 6), (6, 10, 0, 3)])
    b = room("b", floor = "G", kind = "laundry", rect = (10.4, 15, 0, 6))
    p = plan("runs", floors = [floor("G")], rooms = [a, b], doors = [door("d", a = "ell", b = "b", wall = "E", at = (10.0, 3.0), width = 3.0, swing = "open")])
    sw = shared_walls(p, "ell", "b")
    assert_eq(len(sw), 1)
    assert_close(sw[0]["span"], (0.0, 6.0))
    assert_eq(validate(p), [])
    # a glass box beyond a wall is still the room's: its pieces face each other across one wall
    din = room("din", floor = "G", kind = "dining", rects = [(0, 10, 0.5, 6), (3, 7, -3, 0)])
    assert_eq(validate(plan("box", floors = [floor("G")], rooms = [din])), [])
    # a core may join a short flight to the hall it lands in; a stair and a lift may not share one
    f2 = [floor("G", z = 0.0), floor("H", z = 2.0)]
    st = room("steps", floor = "G", kind = "stair", rect = (0, 3, 0, 4), core = "k")
    hl = room("hall", floor = "H", kind = "hall", rect = (0, 3, 4.4, 10), core = "k")
    q = plan("half", floors = f2, rooms = [st, hl])
    assert_eq(validate(q), [])
    assert_eq(door_path(q, "steps", "hall"), ["steps", "hall"])
    lf = room("lift", floor = "H", kind = "lift", rect = (5, 8, 0, 4), core = "s")
    st2 = room("st2", floor = "G", kind = "stair", rect = (5, 8, 0, 4), core = "s")
    _expect(validate(plan("mixed", floors = f2, rooms = [lf, st2])), 'core "s" is a stair on one floor and a lift on another')
    return len(sw)

test("collinear wall pieces merge, L pieces face across a wall, cores land in halls", walls_that_run_on_and_cores_that_land)

def walks_widths_and_crossings():
    assert_close(clear_width(get_room(T, "hall1")), 5.2, tol = 1e-9)
    assert_close(clear_width(get_room(T, "kitchen")), 9.6, tol = 1e-9)  # the L's narrower piece
    assert_close(clear_width((0, 3, 0, 10)), 3.0)
    # bedroom 1 to the bath: its centre, its door, the bath's door, the bath's centre
    assert_close(walk_length(T, "bed1", "bath"), 16.5779, tol = 1e-3)
    assert_eq(walk_length(T, "kitchen", "kitchen"), 0.0)
    assert_eq(walk_length(T, "family", "entry", avoid_kinds = ["kitchen"]), None)
    # the bearing line at u 14.2 on L1, v 10.5..50: through the kitchen's second rect only
    assert_eq(rooms_crossed(T, "L1", "v", 14.2, span = (10.5, 50.0)), [("kitchen", 1, (40.4, 50.0))])
    assert_eq([k for k, _, _ in rooms_crossed(T, "L2", "u", 20.0)], ["bed1", "hall2", "stair2"])
    # a way kept inside one dwelling unit
    assert_eq(door_path(T, "bed1", "outside", units = ["main"]), ["bed1", "hall2", "stair2", "stair1", "hall1", "entry", "outside"])
    assert_eq(door_path(T, "bed1", "outside", units = ["unit2"]), None)
    # furniture in a room's own frame
    tf = dict(T, furniture = [{"room": "bed1", "kind": "bed_queen", "rect": (1.0, 6.0, 11.0, 17.7), "front": "E"}])
    fu = room_local(tf, "bed1")["furniture"]
    assert_eq([x["kind"] for x in fu], ["bed_queen"])
    assert_close(fu[0]["rect"], (0.5, 5.5, 0.5, 7.2), tol = 1e-9)
    return walk_length(T, "bed1", "bath")

test("walks, clear widths, the rooms a line crosses, and ways kept in one unit", walks_widths_and_crossings)

def clear_dimensions_round_down():
    assert_eq(ftin(2.0 + 11.9375 / 12.0, down = True), "2'-11\"")
    assert_eq(ftin(2.0 + 11.9375 / 12.0), "3'-0\"")
    assert_eq(ftin(12.0 - 1e-12, down = True), "12'-0\"")  # float noise is not a lost inch
    assert_eq(ftin(2.9948, 16, down = True), "2'-11 15/16\"")
    return 0

test("clear dimensions round down", clear_dimensions_round_down)

def two_plans_compared():
    v = dict(T, rooms = [r if r["key"] != "bed2" else room("bed2", "Bedroom 2", floor = "L2", kind = "bed", rect = (0.5, 10.0, 24.4, 40.0)) for r in T["rooms"] if r["key"] != "laundry"] +
             [room("study", floor = "L2", kind = "study", rect = (20.0, 24.5, 30.4, 40.0))])
    d = plan_diff(T, v)
    assert_eq(d["added"], ["study"])
    assert_eq(d["removed"], ["laundry"])
    assert_eq([c["key"] for c in d["changed"]], ["bed2"])
    assert_close(d["changed"][0]["moved"], (-1.0, 0.0), tol = 1e-9)
    assert_close(d["changed"][0]["area"], -2.0 * 15.6, tol = 1e-9)
    assert_eq(d["same"], 14)
    return len(d["changed"])

test("two plans compared room by room", two_plans_compared)
