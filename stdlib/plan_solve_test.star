# The tests of @cadpole/plan_solve.star on synthetic floors with round-number geometry, solved to a gap of 1e-6.
# Every floor here has a fixed room (a lobby, an entry); a fixed room is drawn to its inside faces and keeps a room's
# partition. Asserted: objectives, scores, counts and the rules a plan must meet -- not tied coordinates.
#
#   cadpole test stdlib/plan_solve_test.star

load("@cadpole/plan.star", "shared_walls", "validate")
load("@cadpole/plan_solve.star", "annex", "binary", "build", "court", "face", "face_ref", "floor", "ftin", "hold",
     "layout_of", "not_over", "objective_term", "options", "problem", "read", "rect", "room", "row", "rule", "run",
     "score", "solve", "stacks", "structure", "touch", "wall")

GAP = 1e-6  # mip_rel_gap

def floor_a(columns = None):
    """30 x 30 ft inside, a 10 x 6.5 stair block at the south-west, a fixed 4 x 6.5 lobby beside it (the door tree's
    root), escape walls west (north of v 10) and north."""
    return floor("A", zones = {"A": rect(0.5, 30.5, 0.5, 30.5)}, obstacles = {"stair": rect(0.5, 10.5, 0.5, 7.0)},
                 fixed = {"lobby": rect(10.5, 14.5, 0.5, 7.0)}, roots = ["lobby"],
                 egress = [wall("W", 0.5, (10.0, 30.5)), wall("N", 30.5, (0.5, 30.5))], columns = columns)

def rooms_a(n):
    rota = ["bed", "bath", "bed", "living", "closet"]
    rooms = [room("hall", floor = "A", kind = "hall", value = 0.2, w = (3.0, 30.0), d = (3.0, 30.0), opens_to = ["lobby"])]
    beds = []
    for i in range(n - 1):
        k = rota[i]
        key = "%s%d" % (k, i)
        if k == "bed":
            rooms.append(room(key, floor = "A", kind = "bed", area = (100.0, 150.0), w = (9.5, 15.0), d = (9.5, 15.0), aspect = 1.4,
                              opens_to = ["hall", "lobby"]))
            beds.append(key)
        elif k == "bath":
            rooms.append(room(key, floor = "A", kind = "bath", area = (40.0, 60.0), w = (5.0, 9.0), d = (5.0, 9.0), orient = (5.0, 7.5),
                              wet = False, opens_to = ["hall", "lobby"]))
        elif k == "living":
            rooms.append(room(key, floor = "A", kind = "living", area = (150.0, 400.0), w = (10.0, 30.0), d = (10.0, 20.0),
                              knee = (250.0, 0.3), opens_to = ["hall", "lobby"]))
        else:
            rooms.append(room(key, floor = "A", kind = "closet", area = (15.0, 40.0), w = (3.0, 8.0), d = (3.0, 8.0), opens_to = ["hall", "lobby"]))
    return rooms, [("A", beds[i], beds[i + 1]) for i in range(len(beds) - 1)]

def problem_a(n, name, columns = None, rules = None):
    rooms, sym = rooms_a(n)
    return problem(name, floors = [floor_a(columns)], rooms = rooms, sym = sym, rules = rules)

def ps(plan):
    return plan["meta"]["plan_solve"]

def room_of(plan, key):
    for r in plan["rooms"]:
        if r["key"] == key:
            return r
    fail("no room " + key)

def apart(a, b, g):
    return a[1] + g <= b[0] + 1e-6 or b[1] + g <= a[0] + 1e-6 or a[3] + g <= b[2] + 1e-6 or b[3] + g <= a[2] + 1e-6

# ------------------------------------------------------------------------------------------------ the reference floors

def four_rooms_match_the_prototype():
    pl = solve(problem_a(4, "a4"), gap = GAP)
    s = ps(pl)
    assert_eq(s["status"], "optimal")
    # the lobby is walled: the bath takes its east side, 9 x 6'-6"
    assert_close(s["objective"], 426.5632101844859, tol = 1e-6)
    assert_close(s["score"], 426.6426421404682, tol = 1e-6)
    assert_eq(s["stats"]["rows"], 330)
    assert_close(s["areas"]["A:hall"], 369.963, tol = 1e-3)
    for k in ("bed0", "bed2"):
        assert_close(s["areas"]["A:" + k], 150.0, tol = 1e-6)
    assert_close(s["areas"]["A:bath1"], 58.5, tol = 1e-6)
    assert_eq(sorted(["%s>%s" % (d["a"], d["b"]) for d in pl["doors"]]), ["bath1>lobby", "bed0>hall", "bed2>hall", "hall>lobby"])
    return s["score"]

test("four rooms: the prototype's rows; the optimum, areas and door tree with the lobby walled", four_rooms_match_the_prototype)

def five_rooms_match_the_prototype():
    s = ps(solve(problem_a(5, "a5"), gap = GAP))
    # the four-room arrangement, the hall and the bath a partition north of the lobby
    assert_close(s["objective"], 633.3194513328798, tol = 1e-6)
    assert_close(s["score"], 633.4132488873673, tol = 1e-6)
    assert_eq(s["stats"]["rows"], 429)
    assert_close(s["areas"]["A:living3"], 295.296, tol = 1e-3)
    return s["score"]

test("five rooms: the prototype's rows and arrangement, a value curve on the living room", five_rooms_match_the_prototype)

def zones_annex_must_hold_and_touch():
    F = floor("B", zones = {"W": rect(0.5, 18.5, 0.5, 24.5), "NE": rect(19.0, 33.0, 8.5, 24.5)},
              zone_walls = [("W", "NE", 0.5)], fixed = {"entry": rect(4.5, 8.5, 0.5, 4.5)}, roots = ["entry"],
              egress = [wall("N", 24.5, (0.5, 33.0))], must_hold = [hold(rect(5.0, 8.0, 4.9, 7.0), ["hall", "den"])])
    R = [
        room("hall", floor = "B", kind = "hall", value = 0.2, w = (3.0, 16.0), d = (3.0, 20.0), opens_to = ["entry"], zones = ["W"]),
        room("den", floor = "B", kind = "living", area = (90.0, 260.0), w = (10.0, 16.0), d = (8.0, 16.0), opens_to = ["hall", "entry"],
             zones = ["W"], knee = (180.0, 0.4)),
        room("bed", floor = "B", kind = "bed", area = (100.0, 140.0), w = (9.5, 14.0), d = (9.5, 12.0), aspect = 1.4, opens_to = ["hall"],
             zones = ["W", "NE"]),
        room("bath", floor = "B", kind = "bath", area = (35.0, 50.0), w = (5.0, 8.0), d = (5.0, 8.0), orient = (5.0, 7.0), wet = False,
             opens_to = ["hall", "bed"], zones = ["W", "NE"]),
        room("play", floor = "B", kind = "play", area = (60.0, 160.0), w = (6.0, 14.0), d = (6.0, 10.0), habitable = False,
             opens_to = ["hall", "den", "bed"], zones = ["NE"], touches = [touch("N", 24.5, (19.0, 33.0), 6.0)],
             annex = annex(side = "S", w = (0.0, 8.0), d = (0.0, 6.0))),
    ]
    pl = solve(problem("b", floors = [F], rooms = R), gap = GAP)
    s = ps(pl)
    # with the entry's wall the 9'-7" left on its east face is under the den's 10 ft, so the den goes north of the
    # entry (holding the must-hold, a partition off it) and the hall west of it
    assert_close(s["objective"], 459.26619988503415, tol = 1e-6)
    assert_close(s["score"], 459.38187403993857, tol = 1e-6)
    entry = (4.5, 8.5, 0.5, 4.5)
    for k in ("hall", "den", "bed", "bath", "play"):
        for q in room_of(pl, k)["rects"]:
            assert_eq(apart(q, entry, 0.4), True)  # every room keeps its partition off the fixed entry
    play = room_of(pl, "play")
    assert_eq(len(play["rects"]), 2)  # its L-piece exists, flush on its south wall
    main, l = play["rects"]
    assert_close(l[3], main[2], tol = 1e-6)
    assert_close(main[3], 24.5, tol = 1e-6)  # on the north line
    held = [k for k in ("hall", "den") if room_of(pl, k)["rects"][0][0] <= 5.0 + 1e-6 and room_of(pl, k)["rects"][0][1] >= 8.0 - 1e-6 and
            room_of(pl, k)["rects"][0][2] <= 4.9 + 1e-6 and room_of(pl, k)["rects"][0][3] >= 7.0 - 1e-6]
    assert_eq(len(held) >= 1, True)
    # the zone wall: the play area's door to the den passes it, half a foot
    d = [x for x in pl["doors"] if x["a"] == "play"][0]
    assert_eq((d["b"], d["wall"]), ("den", "W"))
    return s["score"]

test("zones with a wall between them, an L-shaped room, a must-hold and a line to run along", zones_annex_must_hold_and_touch)

def the_answer_is_a_plan_value():
    pl = solve(problem_a(4, "a4_plan"))
    assert_eq(pl["unit"], "ft")
    assert_eq(pl["source"], "plan_solve:a4_plan")
    keys = [r["key"] for r in pl["rooms"]]
    for k in ("hall", "bed0", "bath1", "bed2", "lobby", "stair"):
        assert_eq(k in keys, True)
    solved = [r for r in pl["rooms"] if r["key"] in ("hall", "bed0", "bath1", "bed2")]
    for r in solved:
        q = r["rects"][0]
        assert_eq(q[0] >= 0.5 - 1e-6 and q[1] <= 30.5 + 1e-6 and q[2] >= 0.5 - 1e-6 and q[3] <= 30.5 + 1e-6, True)
    for i in range(len(solved)):
        for j in range(i + 1, len(solved)):
            assert_eq(apart(solved[i]["rects"][0], solved[j]["rects"][0], 0.4), True)
    # each bedroom's escape window on an escape wall, 3 ft of it inside the wall's span
    for k in ("bed0", "bed2"):
        ws = [w for w in pl["windows"] if w["room"] == k]
        assert_eq(len(ws), 1)
        w = ws[0]
        assert_eq(w["egress"], True)
        q = room_of(pl, k)["rects"][0]
        if w["wall"] == "W":
            assert_close(q[0], 0.5, tol = 1e-6)
        else:
            assert_close(q[3], 30.5, tol = 1e-6)
    # a door's opening is on the wall it names and inside the shared stretch
    for d in pl["doors"]:
        assert_eq(d["span"][1] - d["span"][0] >= 2.5 - 1e-6, True)
    assert_eq(ps(pl)["binding_by_family"]["R310"] >= 2, True)
    assert_eq(validate(pl), [])  # @cadpole/plan.star finds nothing wrong with it
    return len(pl["doors"])

test("the answer is a plan value: rooms inside the floor and apart, escape windows, doors on shared walls", the_answer_is_a_plan_value)

def the_proxy_the_two_stage_and_the_column_grid():
    proxy = ps(solve(problem_a(5, "a5_proxy"), mode = "proxy", gap = GAP))
    assert_close(proxy["objective"], 649.5360775510204, tol = 1e-6)  # the tangent plane's objective, not the plan's
    assert_close(proxy["score"], 592.1746917950854, tol = 1e-6)
    # the line at 14.5 is the lobby's east face, and grid walls are centred on their lines; a span may start the
    # lobby's whole wall off it (14.9), and the hall is walled off the lobby's north face
    grid = ps(solve(problem_a(5, "a5_grid", columns = [0.5, 10.5, 14.5, 20.5, 30.5]), mode = "grid", gap = GAP))
    assert_close(grid["objective"], 587.9100434782608, tol = 1e-6)
    assert_eq(grid["stats"]["rows"], 447)
    bath = grid["layout"]["A"]["bath1"]
    assert_close(bath[0], 14.9, tol = 1e-6)
    fast = ps(solve(problem_a(5, "a5_fast"), mode = "fast", gap = GAP))
    assert_eq(fast["stage"], "proxy arrangement, exact sizes")
    # the proxy's tied optima size to different plans: between the proxy's own plan and the exact optimum
    assert_eq(fast["score"] >= proxy["score"] - 1e-6 and fast["score"] <= 633.4132488873673 + 1e-6, True)
    return fast["score"]

test("the proxy, the two-stage (proxy arrangement, exact sizes) and walls on column lines", the_proxy_the_two_stage_and_the_column_grid)

def two_floors_stand_on_each_other():
    core = rect(0.5, 8.5, 0.5, 6.5)
    lob = rect(8.5, 12.5, 0.5, 6.5)
    eg = [wall("W", 0.5, (8.0, 26.5)), wall("N", 26.5, (0.5, 24.5))]
    G = floor("G", zones = {"A": rect(0.5, 24.5, 0.5, 26.5)}, obstacles = {"stair": core}, fixed = {"lobby": lob}, roots = ["lobby"], egress = eg)
    U = floor("U", z = 10.0, zones = {"A": rect(0.5, 24.5, 0.5, 26.5)}, obstacles = {"stair": core}, fixed = {"lobby": lob}, roots = ["lobby"], egress = eg)
    R = [room("hall", floor = "G", kind = "hall", value = 0.2, w = (3.0, 20.0), d = (3.0, 20.0), opens_to = ["lobby"]),
         room("fam", floor = "G", kind = "living", area = (150.0, 300.0), w = (10.0, 24.0), d = (10.0, 18.0), knee = (220.0, 0.3), opens_to = ["hall", "lobby"]),
         room("kit", floor = "G", kind = "kitchen", area = (90.0, 160.0), w = (8.0, 14.0), d = (8.0, 14.0), dry = False, opens_to = ["hall", "fam"]),
         room("uhall", floor = "U", kind = "hall", value = 0.2, w = (3.0, 20.0), d = (3.0, 20.0), opens_to = ["lobby"]),
         room("bed", floor = "U", kind = "bed", area = (100.0, 150.0), w = (9.5, 14.0), d = (9.5, 14.0), aspect = 1.4, opens_to = ["uhall", "lobby"]),
         room("bath", floor = "U", kind = "bath", area = (40.0, 60.0), w = (5.0, 9.0), d = (5.0, 9.0), orient = (5.0, 7.5), opens_to = ["uhall", "bed"])]
    P = problem("house2", floors = [G, U], rooms = R, stacks = stacks(count = 2, cost = 15.0, region = (0.5, 24.5, 0.5, 26.5)),
                structure = structure(lines = 1, floors = ["G"], extent = {"G": (0.0, 25.0)}),
                not_over = [not_over("U", "bed", "G", ["kit"])])
    pl = solve(P, gap = GAP)
    s = ps(pl)
    # the family room, the bath and the upper hall each a partition off the lobby
    assert_close(s["objective"], 638.1722736887621, tol = 1e-6)
    assert_close(s["score"], 653.2560396698346, tol = 1e-6)
    assert_eq(s["stats"]["rows"], 565)
    assert_eq(len(pl["stacks"]), 1)  # one stack serves the kitchen and the bath above it
    st = pl["stacks"][0]["at"]
    for fl, k in (("G", "kit"), ("U", "bath")):
        q = [r for r in pl["rooms"] if r["floor"] == fl and r["key"] == k][0]["rects"][0]
        assert_eq(st[0] >= q[0] - 0.6 - 1e-6 and st[0] <= q[1] + 0.6 + 1e-6 and st[1] >= q[2] - 0.6 - 1e-6 and st[1] <= q[3] + 0.6 + 1e-6, True)
    assert_close(s["lines"]["G"][0], 12.7, tol = 1e-6)
    assert_eq(s["beams"], [])  # every room on one side of the line: no beam
    bed = [r for r in pl["rooms"] if r["floor"] == "U" and r["key"] == "bed"][0]["rects"][0]
    kit = [r for r in pl["rooms"] if r["floor"] == "G" and r["key"] == "kit"][0]["rects"][0]
    assert_eq(apart(bed, kit, 0.0), True)
    return s["score"]

test("two floors: a stack through both, a bearing line, a bedroom not over the kitchen", two_floors_stand_on_each_other)

# ---------------------------------------------------------------------------------------------- this library's own

def pinned_rooms_and_a_broken_rule():
    P = problem_a(4, "a4_pin_base")
    best = solve(P)
    lay = ps(best)["layout"]
    held = solve(P, pin = lay, name = "a4_pinned")
    assert_close(ps(held)["score"], ps(best)["score"], tol = 1e-6)
    assert_eq(ps(held)["stats"]["binaries"] < ps(best)["stats"]["binaries"], True)  # sizes given: no width bits, no sides
    # bed0 drawn 1 ft into the hall, across the side they are apart on: the check names the rule it breaks and solves
    # the rest
    bad = dict(lay["A"])
    h = bad["hall"]
    b0 = bad["bed0"]
    if b0[1] <= h[0] + 0.5:
        dx, dy = 1.4, 0.0
    elif h[1] <= b0[0] + 0.5:
        dx, dy = -1.4, 0.0
    elif b0[3] <= h[2] + 0.5:
        dx, dy = 0.0, 1.4
    else:
        dx, dy = 0.0, -1.4
    bad["bed0"] = (b0[0] + dx, b0[1] + dx, b0[2] + dy, b0[3] + dy)
    chk = solve(P, pin = {"A": bad}, name = "a4_broken", broken = "skip")
    rules = [x["rule"] for x in ps(chk)["broken"]]
    assert_eq("pair:A:hall|bed0:apart (overlap or closer than 0'-5\")" in rules, True)
    return len(rules)

test("rooms pinned at their sizes; a drawn overlap is named, not just infeasible", pinned_rooms_and_a_broken_rule)

def an_owner_rule_and_a_moving_face():
    # the owner: a 5-ft hall; the floor's north face may move north (a face), each foot past 30.5 costing 25 of score
    F = floor("A", zones = {"A": rect(0.5, 30.5, 0.5, 34.5)}, obstacles = {"stair": rect(0.5, 10.5, 0.5, 7.0)},
              fixed = {"lobby": rect(10.5, 14.5, 0.5, 7.0)}, roots = ["lobby"],
              egress = [wall("W", 0.5, (10.0, 30.5)), wall("N", face_ref("north"), (0.5, 30.5))],
              faces = {("A", "N"): face_ref("north")})
    rooms, sym = rooms_a(4)
    def price(M):
        objective_term(M, -25.0 * (M["faces"]["north"] - 30.5))
    P = problem("a4_face", floors = [F], rooms = rooms, sym = sym, faces = [face("north", min = 30.5, max = 34.5)],
                rules = [rule("A", "hall", "w", min = 5.0, why = "the owner")], hooks = [price])
    pl = solve(P, gap = GAP)
    s = ps(pl)
    assert_eq(s["status"], "optimal")
    assert_eq("owner:A:hall:w >= 5'-0\" (the owner)" in s["binding"] or room_of(pl, "hall")["rects"][0][1] - room_of(pl, "hall")["rects"][0][0] >= 5.0 - 1e-6, True)
    f = s["faces"]["north"]
    assert_eq(f >= 30.5 - 1e-6 and f <= 34.5 + 1e-6, True)
    for r in pl["rooms"]:
        if r["key"] in ("hall", "bed0", "bath1", "bed2"):
            assert_eq(r["rects"][0][3] <= f + 1e-6, True)
    return f

test("an owner's rule, and a face of the floor that moves at a price", an_owner_rule_and_a_moving_face)

def a_court_through_two_floors():
    eg = [wall("W", 0.5, (8.0, 26.5))]
    G = floor("G", zones = {"A": rect(0.5, 24.5, 0.5, 26.5)}, fixed = {"lobby": rect(0.5, 4.5, 0.5, 6.5)}, roots = ["lobby"], egress = eg)
    U = floor("U", z = 10.0, zones = {"A": rect(0.5, 24.5, 0.5, 26.5)}, fixed = {"lobby": rect(0.5, 4.5, 0.5, 6.5)}, roots = ["lobby"], egress = eg)
    R = [room("liv", floor = "G", kind = "living", area = (100.0, 500.0), w = (10.0, 24.0), d = (10.0, 26.0), opens_to = ["lobby"]),
         room("bed", floor = "U", kind = "bed", area = (100.0, 300.0), w = (9.5, 24.0), d = (9.5, 26.0), opens_to = ["lobby"])]
    P = problem("court2", floors = [G, U], rooms = R,
                courts = [court("lw", base = "G", floors = ["G", "U"], w = (6.0, 8.0), d = (6.0, 8.0), area = (40.0, None), top = 20.0)])
    pl = solve(P)
    assert_eq(len(pl["courts"]), 1)
    c = pl["courts"][0]["rect"]
    assert_eq((c[1] - c[0]) * (c[3] - c[2]) >= 40.0 - 1e-6, True)
    for r in pl["rooms"]:
        if r["key"] in ("liv", "bed"):
            assert_eq(apart(r["rects"][0], c, 0.5), True)  # an exterior wall off the court, on both floors
    return c

test("a court as one rectangle from its base up, an exterior wall off it on every floor", a_court_through_two_floors)

def lobby_problem(name, part = 0.5, inside = True, opens_to = None, open_to = None, piece = None):
    """An 8-ft-wide floor whose fixed 8 x 4 ft lobby (the door tree's root) runs across its south end -- inside the
    zone, or (inside=False) south of a zone drawn to its face -- and an 8-ft den north of it, as deep as the floor lets
    it be: where the den's south face lands is the wall it keeps off the lobby."""
    zone = rect(0.5, 8.5, 0.5, 20.5) if inside else rect(0.5, 8.5, 4.5, 20.5)
    F = floor("L", zones = {"Z": zone}, fixed = {"lobby": rect(0.5, 8.5, 0.5, 4.5)}, roots = ["lobby"], part = part)
    R = [room("den", floor = "L", kind = "den", w = (8.0, 8.0), d = (3.0, 20.0), opens_to = opens_to, open_to = open_to,
              annex = piece)]
    return problem(name, floors = [F], rooms = R)

def off_the_lobby(pl):
    return room_of(pl, "den")["rects"][0][2] - 4.5  # the den's south face less the lobby's north

def a_room_keeps_its_wall_off_a_fixed_room():
    # a fixed room is drawn to its inside faces, as a room is: beside it, the floor's partition (half a foot here, not
    # the 0.4-ft default).
    wall = solve(lobby_problem("lobby_wall"))
    assert_close(off_the_lobby(wall), 0.5, tol = 1e-6)
    assert_close(ps(wall)["score"], 8.0 * 15.5, tol = 1e-6)
    # its door onto the lobby is in that wall, across it, over at least 2'-6" of it (the door tree's root)
    door = solve(lobby_problem("lobby_door", opens_to = ["lobby"]))
    assert_eq(ps(door)["status"], "optimal")
    assert_close(off_the_lobby(door), 0.5, tol = 1e-6)
    ds = door["doors"]
    assert_eq([(x["a"], x["b"], x["wall"]) for x in ds], [("den", "lobby", "S")])
    assert_eq(ds[0]["span"][1] - ds[0]["span"][0] >= 2.5 - 1e-6, True)
    assert_close(shared_walls(door, "den", "lobby")[0]["gap"], 0.5, tol = 1e-6)  # the wall the plan derives
    assert_eq(validate(door), [])
    # open to the lobby: no wall, and its L-piece with it (south of the den it cannot exist, and is parked against
    # the lobby at the den's corner)
    opened = solve(lobby_problem("lobby_open", opens_to = ["lobby"], open_to = ["lobby"],
                                 piece = annex(side = "S", w = (0.0, 4.0), d = (0.0, 4.0))))
    assert_close(off_the_lobby(opened), 0.0, tol = 1e-6)
    assert_close(ps(opened)["score"], 8.0 * 16.0, tol = 1e-6)
    # a zone drawn to the lobby's face: the wall comes out of the zone
    zone = solve(lobby_problem("lobby_zone", inside = False))
    assert_close(off_the_lobby(zone), 0.5, tol = 1e-6)
    # an open-plan floor (no partitions): the den meets the lobby, its door an opening
    open_plan = solve(lobby_problem("lobby_open_plan", part = 0.0, opens_to = ["lobby"]))
    assert_close(off_the_lobby(open_plan), 0.0, tol = 1e-6)
    assert_eq([(x["a"], x["b"]) for x in open_plan["doors"]], [("den", "lobby")])
    return off_the_lobby(wall)

test("a room keeps its wall off a fixed room (the lobby), its door in that wall, unless open to it", a_room_keeps_its_wall_off_a_fixed_room)

def what_cannot_be_solved_says_so():
    R = [room("big", floor = "A", kind = "living", area = (1000.0, None), w = (10.0, 40.0), d = (10.0, 40.0), opens_to = ["lobby"])]
    pl = solve(problem("too_big", floors = [floor_a()], rooms = R))
    s = ps(pl)
    assert_eq((s["status"], s["feasible"]), ("infeasible", False))
    return s["status"]

test("an impossible program comes back infeasible, with no plan", what_cannot_be_solved_says_so)

def three_bedrooms_on_two_escape_walls():
    """Three bedrooms, two short escape walls: no plan, though the LP relaxation has one (integrality is part of the
    clash, so the builder's conflict() would be a deletion filter, many solves)."""
    F = floor("K", zones = {"K": rect(0.5, 30.5, 0.5, 24.5)}, obstacles = {"stair": rect(20.5, 30.5, 0.5, 7.0)},
              fixed = {"lobby": rect(16.5, 20.5, 0.5, 7.0)}, roots = ["lobby"],
              egress = [wall("W", 0.5, (8.0, 24.5)), wall("N", 24.5, (0.5, 9.0))])
    def bed(k):
        return room(k, floor = "K", kind = "bed", area = (100.0, 150.0), w = (9.5, 14.0), d = (9.5, 14.0), aspect = 1.4, opens_to = ["hall"])
    R = [room("hall", floor = "K", kind = "hall", value = 0.2, w = (3.0, 30.0), d = (3.0, 24.0), opens_to = ["lobby"]),
         bed("kid1"), bed("kid2"), bed("kid3")]
    return problem("kids3", floors = [F], rooms = R)

def reading_an_infeasible_solve_searches_for_nothing():
    P = three_bedrooms_on_two_escape_walls()
    s = ps(solve(P, name = "kids3_read"))
    assert_eq(s["status"], "infeasible")
    assert_eq("conflict" in s, False)  # read() ran no conflict search
    # asked (why=True), the builder's family pass, bounded: the escape windows among what cannot all hold
    w = ps(solve(P, why = True, name = "kids3_why"))
    fams = {}
    for n in w["conflict"]:
        fams[n.split(":")[0]] = True
    assert_eq("R310" in fams, True)
    return len(w["conflict"])

test("reading an infeasible solve searches for no conflict unless asked (why=True)", reading_an_infeasible_solve_searches_for_nothing)

def the_probe_is_pure_and_the_names_feet_inches():
    assert_eq(ftin(5.75), "5'-9\"")
    assert_eq(ftin(2.5), "2'-6\"")
    assert_eq(ftin(-0.5), "-0'-6\"")
    return ftin(3.0)

test("ftin writes feet-inches", the_probe_is_pure_and_the_names_feet_inches)

# ----------------------------------------------------------------------------------------------------- options

def max_move(a, b):
    m = 0.0
    for k, r in a.items():
        q = b.get(k)
        if q != None:
            for i in range(4):
                m = max(m, abs(r[i] - q[i]))
    return m

def max_move_up_to_twins(a, b, twins):
    """max_move, each pair of interchangeable rooms (the problem's sym pairs) matched either way round."""
    m = max_move(a, b)
    for p, q in twins:
        c = dict(b)
        c[p] = b[q]
        c[q] = b[p]
        m = min(m, max_move(a, c))
    return m

def a_hamming_no_good_returns_the_same_plan():
    # the failure: solve(count=2, distinct=<the pairs' side binaries>) differs in a
    # binary that decides nothing -- two rooms apart on two sides at once, or two interchangeable bedrooms side by
    # side swapped (the symmetry row orders them north to south, and level they tie) -- so the "next" plan is the first
    M = build(problem_a(4, "a4_hamming"))
    sides = [b for n, b in M["bins"].items() if "|" in n and "#" not in n and "@" not in n]
    sols = M["p"].solve(count = 2, distinct = sides)
    assert_eq(len(sols), 2)
    moved = max_move_up_to_twins(layout_of(M, sols[0])["A"], layout_of(M, sols[1])["A"], [("bed0", "bed2")])
    assert_eq(moved < 0.5, True)  # the bedrooms swapped here
    assert_close(sols[1].objective, sols[0].objective, tol = 1e-3)
    return moved

test("a Hamming no-good on the arrangement binaries returns the same plan", a_hamming_no_good_returns_the_same_plan)

def options_by_arrangement_cuts():
    ops = options(problem_a(4, "a4_options"), count = 4, differ = 3.0)
    assert_eq(len(ops), 4)
    prev = None
    for o in ops:
        op = ps(o)["option"]
        if op["rank"] > 1:
            assert_eq(op["least_move_ft"] >= 3.0 - 1e-6, True)  # some room 3 ft past where it was in every earlier one
            assert_eq(op["score"] <= prev + 1e-6, True)
        prev = op["score"]
        # the symmetry row is in from the start: bed 0's centre north of (or level with) bed 2's, every option
        lay = ps(o)["layout"]["A"]
        assert_eq(lay["bed0"][2] + lay["bed0"][3] >= lay["bed2"][2] + lay["bed2"][3] - 1e-6, True)
    assert_close(ps(ops[0])["score"], 426.6426421404682, tol = 1e-3)  # the four-room optimum
    return [ps(o)["option"]["score"] for o in ops]

test("options: each moves a room 3 ft past another, the symmetry row in from the start", options_by_arrangement_cuts)

def mirror_images_count_as_one():
    S = floor("S", zones = {"S": rect(0.5, 24.5, 0.5, 20.5)}, fixed = {"lobby": rect(10.5, 14.5, 0.5, 4.5)}, roots = ["lobby"],
              egress = [wall("W", 0.5, (0.5, 20.5)), wall("E", 24.5, (0.5, 20.5))])
    R = [room("hall", floor = "S", kind = "hall", value = 0.2, w = (3.0, 24.0), d = (3.0, 20.0), opens_to = ["lobby"]),
         room("bed1", floor = "S", kind = "bed", area = (100.0, 150.0), w = (9.5, 14.0), d = (9.5, 14.0), aspect = 1.4, opens_to = ["hall", "lobby"]),
         room("bath", floor = "S", kind = "bath", area = (40.0, 60.0), w = (5.0, 9.0), d = (5.0, 9.0), orient = (5.0, 7.5), opens_to = ["hall", "lobby"])]
    P = problem("sym", floors = [S], rooms = R)
    twins = options(P, count = 2, differ = 3.0, mirrors = False, name = "sym_twins")
    assert_close(ps(twins[1])["score"], ps(twins[0])["score"], tol = 1e-6)  # without the mirror cut: the mirror image
    one = options(P, count = 2, differ = 3.0, mirrors = True, name = "sym_one")
    op = ps(one[1])["option"]
    assert_eq(op["mirrors"], ["u"])  # the floor mirrors onto itself east-west
    assert_eq(op["least_move_vs_mirrors_ft"] >= 3.0 - 1e-6, True)
    assert_eq(op["score"] < ps(one[0])["score"] - 1.0, True)  # a different plan, not the twin
    return op["score"]

test("mirror images count as one: a floor symmetric east-west gets a cut against each mirror", mirror_images_count_as_one)
